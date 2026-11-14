//! 张量运算特质与 Qwen3-ASR 0.6B 配置。
//!
//! 本箱描述形状、数据类型和识别图可以调用的运算。它不出现 MLX 的类型名，也不做数值求值。
//! [`FakeBackend`] 记录每次调用的形状，供识别图的测试使用。

#![forbid(unsafe_code)]

mod config;
mod dtype;
mod error;
mod fake;
mod ops;
mod shape;

pub use config::{
    conv_stem_out_len, f16_clip_limit, position_half_dim, AsrProfile, ConvStem, AUDIO_END,
    AUDIO_GROUP_BITS, AUDIO_PAD, AUDIO_START, CHUNK_FRAMES, DOWNSAMPLE_HIDDEN, D_MODEL,
    ENCODER_FFN, ENCODER_HEADS, ENCODER_HEAD_DIM, ENCODER_LAYERS, ENCODER_LINEAR_BIAS, F16_ABS_MAX,
    F16_CLIP_MARGIN, GROUP_SIZE, HIDDEN, INTERMEDIATE, KV_HEADS, MAX_POSITION, MEL_BINS,
    MROPE_SECTION, N_WINDOW, N_WINDOW_INFER, PROJECTION_OUT, QUERY_HEADS, QWEN3_ASR_0_6B, RMS_EPS,
    ROPE_SECTIONS, ROPE_THETA, STEM_FREQ, STEM_LINEAR_IN, TEXT_GROUP_BITS, TEXT_HEAD_DIM,
    TEXT_LAYERS, TEXT_LINEAR_BIAS, TIE_EMBEDDINGS, TOKENS_PER_CHUNK, VOCAB_SIZE, WINDOW_SPLIT_AT,
};
pub use dtype::DType;
pub use error::TensorError;
pub use fake::{
    AttentionShape, ConvShape, FakeBackend, FakeTensor, LinearShape, RecordedCall, ReduceOp,
    UnaryOp,
};
pub use ops::{
    AxisSlice, Conv2d, KvCache, LayerNorm, Linear, PadWidth, RmsNorm, Storage, TensorMeta,
    TensorOps, CONV_KERNEL, CONV_PAD, CONV_STRIDE,
};
pub use shape::{broadcast_all, broadcast_shape, conv2d_spatial_out, Shape};
