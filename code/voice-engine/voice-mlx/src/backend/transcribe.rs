//! 一段 PCM 走到底：梅尔 → 编码器 → 预填充 → 逐 token 贪心 → 分词器。
//!
//! 这条路径与参考实现的 `transcribe` 同序：`WhisperFeatureExtractor` 出
//! `(128, frames)`，音频塔出 `(tokens, 1024)`，接到提示里的音频占位处，
//! 文本塔贪心出 token id，最后按词表还原成文本。
//!
//! 三处口径写在这里，不藏在调用方：
//!
//! 1. 提示模板与参考 `Tokenizer.build_prompt_tokens` 一致（见 [`build_prompt`]）。
//! 2. 音频 token 数取编码器实际吐出的行数，不按帧数推公式。
//! 3. 还原文本时去掉特殊 token 的字面量（`<|im_end|>`、`<asr_text>` 等），
//!    提交给输入法的只有识别出来的那串字。
//!
//! 引擎线程上用 [`Engine`] 把两座塔、分词器与梅尔前端打开一次并常驻，之后每次
//! 定稿只借用同一份实例；模块级的 [`transcribe`] / [`transcribe_with`] 保留原
//! 签名，内部现开现用，只给对照与单次实验。

use std::path::Path;

use voice_mel::MelFrontend;
use voice_tensor::{DType, HIDDEN, MEL_BINS};
use voice_tokenizer::Tokenizer;

use super::text_decoder::{Generation, StopReason, TextDecoder, MAX_NEW_TOKENS};
use super::{encode_mel, AudioTower, MlxBackend, MlxError};

/// 参考 manifest 里那次对照用的语言。提示里写 `language Chinese<asr_text>`。
pub const PROMPT_LANGUAGE: &str = "Chinese";

/// 一次识别的结果。
#[derive(Clone, Debug)]
pub struct Transcription {
    /// 梅尔帧数（`样本数 / 160`）。
    pub mel_frames: usize,
    /// 编码器吐出的音频 token 数。
    pub audio_tokens: usize,
    /// 提示长度（含音频占位）。
    pub prompt_tokens: usize,
    /// 新生成的 token id，含停止符。
    pub tokens: Vec<u32>,
    /// 去掉特殊 token 字面量之后的文本。
    pub text: String,
    /// 贪心为什么停下来。空音频没有解码，这里为 `None`。
    pub stopped_by: Option<StopReason>,
}

/// 按参考的模板拼提示 token。
///
/// ```text
/// <|im_start|>system
/// {context}<|im_end|>
/// <|im_start|>user
/// <|audio_start|><|audio_pad|>…{n_audio_tokens}…<|audio_end|><|im_end|>
/// <|im_start|>assistant
/// language {language}<asr_text>
/// ```
///
/// 与参考一致的两处细节：`system\n`、`user\n`、`assistant\n` 各自成 token 段；
/// 只有给了语言才追加最后一行。
pub fn build_prompt(
    tokenizer: &Tokenizer,
    audio_tokens: usize,
    language: Option<&str>,
    context: &str,
) -> Vec<u32> {
    let mut text = String::new();
    text.push_str("<|im_start|>system\n");
    text.push_str(context);
    text.push_str("<|im_end|>\n");
    text.push_str("<|im_start|>user\n");
    text.push_str("<|audio_start|>");
    for _ in 0..audio_tokens {
        text.push_str("<|audio_pad|>");
    }
    text.push_str("<|audio_end|><|im_end|>\n");
    text.push_str("<|im_start|>assistant\n");
    if let Some(language) = language {
        text.push_str("language ");
        text.push_str(language);
        text.push_str("<asr_text>");
    }
    tokenizer.encode(&text)
}

/// 打开模型目录里的分词器，按参考模板给出提示。对照用：先确认模板这一层。
pub fn prompt_tokens(
    model_dir: &str,
    audio_tokens: usize,
    language: Option<&str>,
    context: &str,
) -> Result<Vec<u32>, MlxError> {
    let tokenizer = load_tokenizer(model_dir)?;
    Ok(build_prompt(&tokenizer, audio_tokens, language, context))
}

fn load_tokenizer(model_dir: &str) -> Result<Tokenizer, MlxError> {
    Tokenizer::load(Path::new(model_dir)).map_err(MlxError::new)
}

/// 用默认上限与默认语言识别一段 16 kHz 单声道样本。
///
/// 这条口子每次调用都重新打开权重，只给对照与单次实验用；引擎线程上的常驻
/// 实例见 [`Engine`]。
pub fn transcribe(model_dir: &str, pcm: &[i16]) -> Result<Transcription, MlxError> {
    transcribe_with(model_dir, pcm, MAX_NEW_TOKENS, Some(PROMPT_LANGUAGE), "")
}

/// 与 [`transcribe`] 相同，但可以换新 token 上限、语言与上下文。
///
/// 语言给 `None` 时不追加 `language …<asr_text>` 那一行，与参考的
/// `build_prompt_tokens(language=None)` 一致。
///
/// 每次调用现场打开两份权重。同一进程里要重复用，别走这条，走 [`Engine`]。
pub fn transcribe_with(
    model_dir: &str,
    pcm: &[i16],
    max_new_tokens: usize,
    language: Option<&str>,
    context: &str,
) -> Result<Transcription, MlxError> {
    if pcm.is_empty() {
        return Ok(empty_transcription());
    }
    let mut ops = MlxBackend::on_this_thread()?;
    let mut engine = Engine::open(model_dir, DType::F16)?;
    engine.transcribe_with(&mut ops, pcm, max_new_tokens, language, context)
}

/// 已经打开的两座塔、分词器与梅尔前端。引擎线程上打开一次，之后每个会话只借用。
///
/// 常驻的就是这条路径上的三段固定开销：513 MB 的权重读盘与反量化、词嵌入表
/// 的分块取行、以及梅尔滤波器组与旋转因子表。会话只带逐次变化的东西：PCM、
/// 提示、KV 缓存。
///
/// 字段顺序有意让音频塔先于文本塔断开：两者同时退出时，音频塔的 454 MB 先放手。
pub struct Engine {
    mel: MelFrontend,
    tower: AudioTower,
    decoder: TextDecoder,
    tokenizer: Tokenizer,
    dtype: DType,
}

impl Engine {
    /// 在引擎线程上把权重与梅尔前端各打开一次。
    ///
    /// 需要先调用 [`MlxBackend::prepare`]，否则反量化没有 GPU stream。
    pub fn open(model_dir: &str, dtype: DType) -> Result<Self, MlxError> {
        // 梅尔前端是纯主机表，先建；它不依赖权重能不能打开。
        let mel = MelFrontend::new();
        let tokenizer = load_tokenizer(model_dir)?;
        let tower = AudioTower::open(model_dir, dtype)?;
        let decoder = TextDecoder::open(model_dir, dtype)?;
        Ok(Self {
            mel,
            tower,
            decoder,
            tokenizer,
            dtype,
        })
    }

    /// 用默认上限与默认语言识别一段 PCM。引擎线程上的会话走这条。
    pub fn transcribe(
        &mut self,
        ops: &mut MlxBackend,
        pcm: &[i16],
    ) -> Result<Transcription, MlxError> {
        self.transcribe_with(ops, pcm, MAX_NEW_TOKENS, Some(PROMPT_LANGUAGE), "")
    }

    /// 换新 token 上限、语言与上下文。口径与模块级 [`transcribe_with`] 相同。
    pub fn transcribe_with(
        &mut self,
        ops: &mut MlxBackend,
        pcm: &[i16],
        max_new_tokens: usize,
        language: Option<&str>,
        context: &str,
    ) -> Result<Transcription, MlxError> {
        let opened = Opened {
            mel: &self.mel,
            tower: &self.tower,
            decoder: &mut self.decoder,
            tokenizer: &self.tokenizer,
            dtype: self.dtype,
        };
        transcribe_with_opened(ops, opened, pcm, max_new_tokens, language, context)
    }

    /// 常驻的梅尔前端。
    pub fn mel(&self) -> &MelFrontend {
        &self.mel
    }

    /// 常驻的音频塔。
    pub fn tower(&self) -> &AudioTower {
        &self.tower
    }

    /// 常驻的文本解码器。会话只借用，不重建。
    pub fn decoder(&mut self) -> &mut TextDecoder {
        &mut self.decoder
    }

    /// 常驻的分词器。
    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }
}

/// 已经打开的实例的借用：梅尔前端、两座塔、分词器与激活类型。
///
/// 这是 [`Engine`] 的内核：调用方把常驻的实例递进来，识别本身不读盘、
/// 不反量化、不重建梅尔前端。
pub struct Opened<'a> {
    pub mel: &'a MelFrontend,
    pub tower: &'a AudioTower,
    pub decoder: &'a mut TextDecoder,
    pub tokenizer: &'a Tokenizer,
    pub dtype: DType,
}

/// 用已经打开的实例识别一段 PCM。[`Engine::transcribe_with`] 走的就是这里。
pub fn transcribe_with_opened(
    ops: &mut MlxBackend,
    opened: Opened<'_>,
    pcm: &[i16],
    max_new_tokens: usize,
    language: Option<&str>,
    context: &str,
) -> Result<Transcription, MlxError> {
    if pcm.is_empty() {
        return Ok(empty_transcription());
    }
    let mel = opened.mel.log_mel(pcm);
    let mel_frames = mel.len() / MEL_BINS;
    if mel_frames == 0 {
        return Ok(empty_transcription());
    }

    let dtype = opened.dtype;
    let encoded = encode_mel(ops, opened.tower, &mel, mel_frames, dtype)?;
    let audio_tokens = encoded.tokens;
    if encoded.proj.len() != audio_tokens * HIDDEN {
        return Err(MlxError::new(format!(
            "编码器投影 {} 个元素，与 {audio_tokens} 行的形状不一致",
            encoded.proj.len()
        )));
    }

    let prompt = build_prompt(opened.tokenizer, audio_tokens, language, context);
    let Generation {
        tokens,
        stopped_by,
        ..
    } = opened
        .decoder
        .generate(ops, &prompt, &encoded.proj, max_new_tokens)?;
    let text = opened.tokenizer.decode_without_special(&tokens);
    Ok(Transcription {
        mel_frames,
        audio_tokens,
        prompt_tokens: prompt.len(),
        tokens,
        text,
        stopped_by: Some(stopped_by),
    })
}

fn empty_transcription() -> Transcription {
    Transcription {
        mel_frames: 0,
        audio_tokens: 0,
        prompt_tokens: 0,
        tokens: Vec::new(),
        text: String::new(),
        stopped_by: None,
    }
}
