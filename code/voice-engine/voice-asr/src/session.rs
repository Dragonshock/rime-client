//! 流式会话。每满 2.0 秒跑一次编码步；松开时把余下样本一次送完。

use voice_tensor::{DType, TensorOps};

use crate::config::{audio_tokens, AsrConfig};
use crate::encoder::{decode_prefill, encode_frames, EncoderWeights, TextWeights};
use crate::error::EngineError;
use crate::prefix::{build_prefix, PrefixPart};
use crate::state::StreamState;

/// 交给调用方的一次整段文本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptEvent {
    pub text: String,
    /// 定稿为真，部分结果为假。
    pub is_final: bool,
}

/// 假后端跑完形状之后，由这一步给出整段文本。
///
/// 形状后端不携带 logits 的下标。整段文本由实现这个特质的一方给出。
pub struct DecodeView<'a> {
    /// 这一步新纳入的样本数。部分步是 32000，定稿步是余下样本，可以是 0。
    pub new_samples: usize,
    pub total_samples: usize,
    pub is_final: bool,
    pub prefix: &'a [PrefixPart],
    pub stable_tokens: &'a [u32],
    pub active_frames: usize,
    pub committed_tokens: usize,
    /// 这一步新纳入的 16 kHz 单声道样本。数值解码只能从这里取声音。
    pub samples: &'a [i16],
}

/// 贪心解码看到的整段结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepOutput {
    pub text: String,
    pub tokens: Vec<u32>,
}

/// 把一步的前缀和音频 token 收成整段文本。
pub trait DecodeText {
    fn decode_text(&mut self, view: &DecodeView<'_>) -> StepOutput;

    /// 为真时，这一步自己用数值后端产出文本，会话不再跑零权重形状图。
    fn supplies_numeric_text(&self) -> bool {
        false
    }

    /// 权重打不开，或贪心解码没有产出词表文本。定稿回调空串，调用方把返回码标成失败。
    fn weight_failed(&self) -> bool {
        false
    }

    /// 定稿失败的完整原因。`weight_failed` 为真时应给出非空的说明，
    /// 调用方把它写进日志，用户才知道这一次为什么没有文本。
    fn failure_reason(&self) -> Option<String> {
        None
    }
}

/// 接收部分结果与定稿。
pub trait TranscriptSink {
    fn on_transcript(&mut self, event: &TranscriptEvent);
}

impl TranscriptSink for Vec<TranscriptEvent> {
    fn on_transcript(&mut self, event: &TranscriptEvent) {
        self.push(event.clone());
    }
}

/// 一次按住。`B` 是记录形状的假后端，或有数值的求值后端。
pub struct Session<B: TensorOps, H, S> {
    config: AsrConfig,
    hotwords: Option<String>,
    state: StreamState,
    ops: B,
    encoder: EncoderWeights<B::Tensor>,
    text: TextWeights<B::Tensor>,
    prefix_audio: Option<B::Tensor>,
    active_audio: Option<B::Tensor>,
    dtype: DType,
    decoder: H,
    sink: S,
    total_samples: usize,
    pending: Vec<i16>,
    hops_in_step: usize,
    finished: bool,
}

impl<B, H, S> Session<B, H, S>
where
    B: TensorOps,
    H: DecodeText,
    S: TranscriptSink,
{
    /// `hotwords` 为 `None` 或空串时，会话不保存空热词。
    pub fn new(
        hotwords: Option<&str>,
        mut ops: B,
        decoder: H,
        sink: S,
    ) -> Result<Self, EngineError<B::Error>> {
        let dtype = DType::F32;
        let encoder = EncoderWeights::build(&mut ops, dtype)?;
        let text = TextWeights::build(&mut ops, dtype)?;
        let config = AsrConfig::qwen3_asr_0_6b();
        let state = StreamState::new(&config);
        let hotwords = match hotwords {
            Some(text) if !text.is_empty() => Some(text.to_string()),
            _ => None,
        };
        Ok(Self {
            config,
            hotwords,
            state,
            ops,
            encoder,
            text,
            prefix_audio: None,
            active_audio: None,
            dtype,
            decoder,
            sink,
            total_samples: 0,
            pending: Vec::new(),
            hops_in_step: 0,
            finished: false,
        })
    }

    pub fn state(&self) -> &StreamState {
        &self.state
    }

    pub fn prefix(&self) -> Vec<PrefixPart> {
        build_prefix(self.hotwords.as_deref(), &self.config.language)
    }

    pub fn sink(&self) -> &S {
        &self.sink
    }

    pub fn decoder(&self) -> &H {
        &self.decoder
    }

    /// 假后端，或调用方放进来的张量后端。
    pub fn tensor(&self) -> &B {
        &self.ops
    }

    /// 推入 16 kHz 单声道 `i16`。凑满 32000 个样本才回调一次部分结果。
    pub fn feed(&mut self, pcm: &[i16]) -> Result<(), EngineError<B::Error>> {
        if self.finished || pcm.is_empty() {
            return Ok(());
        }
        self.total_samples += pcm.len();
        self.pending.extend_from_slice(pcm);
        self.state.pcm_tail.extend_from_slice(pcm);
        let hop = self.config.hop_length;
        let hops_needed = self.config.frames_per_step();
        while self.state.pcm_tail.len() >= hop {
            self.state.pcm_tail.drain(..hop);
            self.state.mel_pending.frames += 1;
            self.hops_in_step += 1;
            if self.hops_in_step == hops_needed {
                self.hops_in_step = 0;
                let samples: Vec<i16> = self.pending.drain(..self.config.step_samples).collect();
                self.dispatch(&samples, false)?;
            }
        }
        Ok(())
    }

    /// 松开。没有样本时回调空串，且 `is_final` 为真。有样本时不再等待 2 秒。
    ///
    /// 部分步的空串不下发（见 `apply`），定稿的空串一定下发。
    pub fn finish(&mut self) -> Result<(), EngineError<B::Error>> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        if self.total_samples == 0 {
            self.emit("", true);
            return Ok(());
        }
        if !self.state.pcm_tail.is_empty() {
            self.state.mel_pending.frames += 1;
            self.state.pcm_tail.clear();
        }
        let rest = std::mem::take(&mut self.pending);
        self.hops_in_step = 0;
        self.dispatch(&rest, true)?;
        Ok(())
    }

    fn dispatch(&mut self, samples: &[i16], is_final: bool) -> Result<(), EngineError<B::Error>> {
        if self.decoder.supplies_numeric_text() {
            return self.emit_numeric(samples, is_final);
        }
        if !samples.is_empty() {
            crate::error::backend(self.ops.pcm_to_f32(samples))?;
        }
        self.run_encoder()?;
        if let Some(audio) = self.combined_audio()? {
            decode_prefill(&mut self.ops, &self.text, &audio, self.dtype)?;
        }
        self.emit_from_decoder(samples, is_final);
        Ok(())
    }

    /// 数值解码器自己跑线性、卷积和注意力。这里不再用全零权重走一遍形状图。
    fn emit_numeric(
        &mut self,
        samples: &[i16],
        is_final: bool,
    ) -> Result<(), EngineError<B::Error>> {
        self.emit_from_decoder(samples, is_final);
        Ok(())
    }

    fn emit_from_decoder(&mut self, samples: &[i16], is_final: bool) {
        let prefix = build_prefix(self.hotwords.as_deref(), &self.config.language);
        let stable = self.state.stable_tokens().to_vec();
        let view = DecodeView {
            new_samples: samples.len(),
            total_samples: self.total_samples,
            is_final,
            prefix: &prefix,
            stable_tokens: &stable,
            active_frames: self.state.mel_pending.frames,
            committed_tokens: self.state.encoder_prefix.token_count,
            samples,
        };
        let output = self.decoder.decode_text(&view);
        self.apply(output, is_final);
    }

    /// 满窗的梅尔帧编完后留在 `encoder_prefix`，活动窗每次重算。
    fn run_encoder(&mut self) -> Result<(), EngineError<B::Error>> {
        let window = self.config.encoder.n_window_infer;
        while self.state.mel_pending.frames >= window {
            let encoded = encode_frames(&mut self.ops, &self.encoder, window, self.dtype)?;
            self.push_prefix(encoded)?;
            self.state.mel_pending.frames -= window;
            let tokens = audio_tokens(window, self.config.encoder);
            self.state.encoder_prefix.token_count += tokens;
            self.state.encoder_prefix.width = self.config.encoder.proj_out;
        }
        if self.state.mel_pending.frames > 0 {
            let frames = self.state.mel_pending.frames;
            let encoded = encode_frames(&mut self.ops, &self.encoder, frames, self.dtype)?;
            self.active_audio = Some(encoded);
        } else {
            self.active_audio = None;
        }
        Ok(())
    }

    fn push_prefix(&mut self, encoded: B::Tensor) -> Result<(), EngineError<B::Error>> {
        if let Some(prev) = self.prefix_audio.clone() {
            let joined = crate::error::backend(self.ops.concatenate(&[&prev, &encoded], 1))?;
            self.prefix_audio = Some(joined);
        } else {
            self.prefix_audio = Some(encoded);
        }
        Ok(())
    }

    fn combined_audio(&mut self) -> Result<Option<B::Tensor>, EngineError<B::Error>> {
        match (self.prefix_audio.clone(), self.active_audio.clone()) {
            (Some(prefix), Some(active)) => {
                let joined = crate::error::backend(self.ops.concatenate(&[&prefix, &active], 1))?;
                Ok(Some(joined))
            }
            (Some(prefix), None) => Ok(Some(prefix)),
            (None, Some(active)) => Ok(Some(active)),
            (None, None) => Ok(None),
        }
    }

    fn apply(&mut self, output: StepOutput, is_final: bool) {
        if !is_final && output.text.is_empty() {
            // 部分步可以什么都不说。空串在这里表示「这一步没有新文本」，不是
            // 「整段变空」：已经写出去的预编辑留着，定稿再决定清不清。
            // 说话中每 2 秒回调一次的契约不变，只是一步的空结果不下发。
            return;
        }
        let keep = self.config.text.unfixed_tokens.min(output.tokens.len());
        self.state.tokens = output.tokens;
        let split = self.state.tokens.len() - keep;
        self.state.unfixed = self.state.tokens[split..].to_vec();
        self.state.kv.set_stable_len(split);
        if is_final {
            self.state.published = output.text;
            let text = self.state.published.clone();
            self.emit(&text, true);
        } else if output.text != self.state.published {
            self.state.published = output.text;
            let text = self.state.published.clone();
            self.emit(&text, false);
        }
    }

    fn emit(&mut self, text: &str, is_final: bool) {
        self.sink.on_transcript(&TranscriptEvent {
            text: text.to_string(),
            is_final,
        });
    }
}
