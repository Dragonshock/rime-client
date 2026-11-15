//! `voice-tensor` 的 Metal 实现。
//!
//! 不打开 `metal` 特性时，本箱不链接 MLX。打开之后，引擎线程先调用
//! [`MlxBackend::prepare`] 设置 `mlx.metallib` 并在这根线程上建立 GPU stream。
//! `Array` 不是 `Send`，张量不得离开这根线程。

#![cfg_attr(not(feature = "metal"), forbid(unsafe_code))]

#[cfg(feature = "metal")]
mod backend;

#[cfg(feature = "metal")]
pub use backend::{
    build_prompt, encode_mel, encode_mel_with, prompt_parts, prompt_tokens, splice_prompt,
    transcribe, transcribe_with, transcribe_with_opened, AudioTower, DecoderOptions,
    EncoderOptions, EncoderOutput, Engine, Generation, MlxBackend, MlxError, MlxTensor, Opened,
    PositionDenominator, PrefillLogits, PromptParts, QuantPolicy, RopeMode, StopReason, Stream,
    StreamOpened, StreamStep, TextDecoder, Transcription, EOS, IM_END, LM_HEAD_BLOCK,
    MAX_NEW_TOKENS, PROMPT_LANGUAGE,
};
