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
use voice_tensor::{DType, AUDIO_PAD, HIDDEN, MEL_BINS};
use voice_tokenizer::Tokenizer;

use super::stream::{stream_step_with_opened, Stream, StreamOpened, StreamStep};
use super::text_decoder::{Generation, QuantPolicy, StopReason, TextDecoder, MAX_NEW_TOKENS};
use super::weights::SafeWeights;
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

/// 提示的三段：音频占位之前、之后各一段，中间是一串 `<|audio_pad|>`。
///
/// 三段按 id 直接拼出来与 [`build_prompt`] 对整段文字分词给的是同一串：切点落在
/// 特殊 token 上，分词器又是先把特殊 token 切出来、剩下的文字各自分词，所以切与
/// 不切是同一个结果（`prompt_splicing_matches_one_pass_tokenization` 逐个数比过）。
/// 跨步复用键值要的正是这个性质：提示头与已结束各窗的行都在 `head` 加音频占位里。
pub struct PromptParts {
    /// 从开头到 `<|audio_start|>` 的 id。
    pub head: Vec<u32>,
    /// 从 `<|audio_end|>` 到 `language …<asr_text>` 的 id。
    pub tail: Vec<u32>,
}

/// 把提示拆成「音频占位之前」与「之后的固定尾巴」两段。
///
/// 与 [`build_prompt`] 用同一个模板，只是不分词整段：音频占位可能上千个，逐个
/// 写进字符串再分词比直接拼 id 多一倍活。
pub fn prompt_parts(tokenizer: &Tokenizer, language: Option<&str>, context: &str) -> PromptParts {
    let mut head = String::new();
    head.push_str("<|im_start|>system\n");
    head.push_str(context);
    head.push_str("<|im_end|>\n");
    head.push_str("<|im_start|>user\n");
    head.push_str("<|audio_start|>");
    let mut tail = String::new();
    tail.push_str("<|audio_end|><|im_end|>\n");
    tail.push_str("<|im_start|>assistant\n");
    if let Some(language) = language {
        tail.push_str("language ");
        tail.push_str(language);
        tail.push_str("<asr_text>");
    }
    PromptParts {
        head: tokenizer.encode(&head),
        tail: tokenizer.encode(&tail),
    }
}

/// 拼好一整条提示：`head` + `audio_tokens` 个音频占位 + `tail`。
pub fn splice_prompt(parts: &PromptParts, audio_tokens: usize) -> Vec<u32> {
    let mut ids = Vec::with_capacity(parts.head.len() + audio_tokens + parts.tail.len());
    ids.extend_from_slice(&parts.head);
    ids.extend(std::iter::repeat(AUDIO_PAD).take(audio_tokens));
    ids.extend_from_slice(&parts.tail);
    ids
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
/// 打开分两段：[`Engine::open_audio`] 只装梅尔与音频塔（第一段），
/// [`Engine::load_text`] 再装文本塔（第二段）。`open` 是两段连做。分段的用处是
/// 「按键先把第一段装上、第二段跟用户说话重叠」，装载的分段墙钟见 `notes/066`。
///
/// 字段顺序有意让音频塔先于文本塔断开：两者同时退出时，音频塔的 454 MB 先放手。
pub struct Engine {
    mel: MelFrontend,
    tokenizer: Option<Tokenizer>,
    tower: AudioTower,
    decoder: Option<TextDecoder>,
    /// 第二段还要读的那份权重文件。文本塔装好之后放开（表头 136 KB 与文件句柄一起走）。
    weights: Option<SafeWeights>,
    /// 第二段还要读的分词器与权重的目录。装完之后放开。
    model_dir: Option<String>,
    dtype: DType,
}

impl Engine {
    /// 第一段：梅尔前端与音频塔。
    ///
    /// 需要先调用 [`MlxBackend::prepare`]，否则反量化没有 GPU stream。
    ///
    /// 这一行只碰音频一侧：`audio_tower.*` 的 523 张张量（202 MB）与梅尔那几张表。
    /// 分词器不在这一行——它跟文本塔一起装，因为两处都只有出文本时才用得上
    /// （`Tokenizer::load` 解 `vocab.json` 与 `merges.txt`，本身要 0.4 秒）。
    ///
    /// 权重文件留在 `self.weights` 里不关：第二段接着读同一个文件，不必重解表头。
    /// 到这一行为止只 `eval` 过一次（音频塔那些常量），文本塔的张量一个字节没读。
    pub fn open_audio(model_dir: &str, dtype: DType) -> Result<Self, MlxError> {
        // 梅尔前端是纯主机表，先建；它不依赖权重能不能打开。
        let mel = MelFrontend::new();
        let mut weights = SafeWeights::open(model_dir)?;
        let tower = AudioTower::load(&mut weights, dtype)?;
        weights.finish()?;
        Ok(Self {
            mel,
            tokenizer: None,
            tower,
            decoder: None,
            weights: Some(weights),
            model_dir: Some(model_dir.to_string()),
            dtype,
        })
    }

    /// 第二段：文本塔（4-bit 的 `model.*` 与 `lm_head`）与分词器。
    ///
    /// 装过就返回 `Ok(false)`，不读盘、不建图——所以在 `begin` 之后与 `finish` 之前
    /// 各叫一次也只付一次钱。装好之后权重文件放开：表头与句柄不留在进程里。
    ///
    /// 装失败时把权重文件还回槽里，下一段还能重试；失败原因照原样往上抛，
    /// 不吞成「没有文本」。
    pub fn load_text(&mut self) -> Result<bool, MlxError> {
        if self.decoder.is_some() {
            return Ok(false);
        }
        if self.tokenizer.is_none() {
            let dir = self
                .model_dir
                .as_deref()
                .ok_or_else(|| MlxError::new("模型目录已经放开，分词器装不上"))?;
            self.tokenizer = Some(load_tokenizer(dir)?);
        }
        let mut weights = self
            .weights
            .take()
            .ok_or_else(|| MlxError::new("权重文件已经放开，文本塔装不上"))?;
        let decoder = match TextDecoder::load(&mut weights, self.dtype, QuantPolicy::default()) {
            Ok(decoder) => decoder,
            Err(error) => {
                self.weights = Some(weights);
                return Err(error);
            }
        };
        if let Err(error) = weights.finish() {
            self.weights = Some(weights);
            return Err(error);
        }
        self.weights = None;
        self.model_dir = None;
        self.decoder = Some(decoder);
        Ok(true)
    }

    /// 文本塔（与分词器）装好没有。定稿要用它们；`false` 时 [`Engine::transcribe`]
    /// 报错，不返回空串。
    pub fn text_ready(&self) -> bool {
        self.decoder.is_some()
    }

    /// 两段连着做。给一次性的对照与单次实验用。
    ///
    /// 两座塔挂在同一份 [`SafeWeights`] 上：权重文件只开一次，求值只在末尾
    /// 一次 `eval` 里做（以前两座塔各开一次文件、每张量各一次 `eval`）。
    pub fn open(model_dir: &str, dtype: DType) -> Result<Self, MlxError> {
        let mut engine = Self::open_audio(model_dir, dtype)?;
        engine.load_text()?;
        Ok(engine)
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
        // 文本塔还没装好时报错，不给空串：定稿要么是识别结果，要么是一条原因。
        let (Some(decoder), Some(tokenizer)) = (self.decoder.as_mut(), self.tokenizer.as_ref())
        else {
            return Err(MlxError::new("文本塔还没装好"));
        };
        let opened = Opened {
            mel: &self.mel,
            tower: &self.tower,
            decoder,
            tokenizer,
            dtype: self.dtype,
        };
        transcribe_with_opened(ops, opened, pcm, max_new_tokens, language, context)
    }

    /// 跑一个非定稿步：已结束的 8 秒窗留在 `stream` 里，只编活动窗，只重算未固定的尾巴。
    ///
    /// 口语路径与 [`Engine::transcribe`] 同序，区别是编码器前缀与已固定的文本
    /// 跨步留着。逐步的字段见 [`StreamStep`]。
    ///
    /// [`StreamStep`]: super::StreamStep
    pub fn stream_step(
        &mut self,
        ops: &mut MlxBackend,
        stream: &mut Stream,
        max_new_tokens: usize,
        language: Option<&str>,
        context: &str,
    ) -> Result<StreamStep, MlxError> {
        let (Some(decoder), Some(tokenizer)) = (self.decoder.as_mut(), self.tokenizer.as_ref())
        else {
            return Err(MlxError::new("文本塔还没装好"));
        };
        let opened = StreamOpened {
            mel: &self.mel,
            tower: &self.tower,
            decoder,
            tokenizer,
            dtype: self.dtype,
        };
        stream_step_with_opened(ops, opened, stream, max_new_tokens, language, context)
    }

    /// 常驻的梅尔前端。
    pub fn mel(&self) -> &MelFrontend {
        &self.mel
    }

    /// 常驻的音频塔。
    pub fn tower(&self) -> &AudioTower {
        &self.tower
    }

    /// 常驻的文本解码器。会话只借用，不重建；还没装好时为 `None`。
    pub fn decoder(&mut self) -> Option<&mut TextDecoder> {
        self.decoder.as_mut()
    }

    /// 常驻的分词器。跟文本塔一起装；还没装好时为 `None`。
    pub fn tokenizer(&self) -> Option<&Tokenizer> {
        self.tokenizer.as_ref()
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
