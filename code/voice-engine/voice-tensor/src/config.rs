use crate::ops::{CONV_KERNEL, CONV_PAD, CONV_STRIDE};

/// 音频塔卷积茎的一层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConvStem {
    /// 输入通道。
    pub in_ch: usize,
    /// 输出通道。
    pub out_ch: usize,
    /// 核大小。
    pub kernel: usize,
    /// 步长。
    pub stride: usize,
    /// 填充。
    pub pad: usize,
}

/// Qwen3-ASR 0.6B 的音频塔与文本塔。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AsrProfile {
    /// 梅尔频点数。
    pub mel_bins: usize,
    /// 编码器层数。
    pub encoder_layers: usize,
    /// 编码器头数。
    pub encoder_heads: usize,
    /// 编码器头维，等于 `d_model / encoder_heads`。
    pub encoder_head_dim: usize,
    /// 编码器前馈宽度。
    pub encoder_ffn: usize,
    /// 音频塔宽度。
    pub d_model: usize,
    /// 投影到文本塔的宽度。
    pub projection_out: usize,
    /// 窗参数。块长是它的两倍。
    pub n_window: usize,
    /// 一块的梅尔帧数。
    pub chunk_frames: usize,
    /// 推理注意力窗的梅尔帧数。
    pub n_window_infer: usize,
    /// 下采样隐层宽度。
    pub downsample_hidden: usize,
    /// 编码器的线性层带偏置。
    pub encoder_linear_bias: bool,
    /// 三层卷积茎。
    pub conv_stem: [ConvStem; 3],
    /// 128 个梅尔频点三次下采样后的长度。
    pub stem_freq: usize,
    /// 卷积茎之后线性层的输入宽度。
    pub stem_linear_in: usize,
    /// 满块 100 帧三次卷积后的音频 token 数。
    pub tokens_per_chunk: usize,
    /// 正弦位置表的最大位置。
    pub max_position: usize,
    /// 达到这个窗数时按窗切开计算。
    pub window_split_at: usize,
    /// float16 裁剪时从类型最大值减去的余量。
    pub f16_clip_margin: i32,
    /// 词表大小。
    pub vocab: usize,
    /// 文本隐层。
    pub hidden: usize,
    /// 文本中间层。
    pub intermediate: usize,
    /// 文本层数。
    pub text_layers: usize,
    /// 查询头。
    pub query_heads: usize,
    /// 键值头。
    pub kv_heads: usize,
    /// 文本头维。它不等于隐层除以查询头。
    pub text_head_dim: usize,
    /// 三维 MRoPE 的 theta。
    pub rope_theta: f32,
    /// MRoPE 的位置轴数。位置形状是 `(B, rope_sections, L)`。
    pub rope_sections: usize,
    /// RMSNorm 的 eps。
    pub rms_eps: f32,
    /// 文本塔线性层不带偏置。
    pub text_linear_bias: bool,
    /// 词嵌入与 `lm_head` 绑同一张表。
    pub tie_embeddings: bool,
    /// 音频开始 token。
    pub audio_start: u32,
    /// 音频占位 token。
    pub audio_pad: u32,
    /// 音频结束 token。
    pub audio_end: u32,
    /// 分组存放的组大小。
    pub group_size: usize,
}

/// 梅尔频点。
pub const MEL_BINS: usize = 128;
/// 编码器层数。
pub const ENCODER_LAYERS: usize = 18;
/// 编码器头数。
pub const ENCODER_HEADS: usize = 14;
/// 音频塔宽度。
pub const D_MODEL: usize = 896;
/// 编码器头维。
pub const ENCODER_HEAD_DIM: usize = D_MODEL / ENCODER_HEADS;
/// 编码器前馈宽度。
pub const ENCODER_FFN: usize = 3584;
/// 投影输出，与文本隐层相同。
pub const PROJECTION_OUT: usize = 1024;
/// 窗参数。
pub const N_WINDOW: usize = 50;
/// 一块的梅尔帧数。
pub const CHUNK_FRAMES: usize = N_WINDOW * 2;
/// 推理注意力窗，800 帧即 8 秒。
pub const N_WINDOW_INFER: usize = 800;
/// 下采样隐层。
pub const DOWNSAMPLE_HIDDEN: usize = 480;
/// 卷积茎之后的频点长度。
pub const STEM_FREQ: usize = conv_stem_out_len(conv_stem_out_len(conv_stem_out_len(MEL_BINS)));
/// 卷积茎线性层的输入宽度。
pub const STEM_LINEAR_IN: usize = DOWNSAMPLE_HIDDEN * STEM_FREQ;
/// 满块卷积后的 token 数。
pub const TOKENS_PER_CHUNK: usize =
    conv_stem_out_len(conv_stem_out_len(conv_stem_out_len(CHUNK_FRAMES)));
/// 正弦位置表的最大位置。
pub const MAX_POSITION: usize = 1500;
/// 按窗切开的窗数门槛。
pub const WINDOW_SPLIT_AT: usize = 20;
/// float16 有限值的绝对值上界。
pub const F16_ABS_MAX: f32 = 65504.0;
/// float16 裁剪余量。
pub const F16_CLIP_MARGIN: i32 = 1000;
/// 词表大小。
pub const VOCAB_SIZE: usize = 151936;
/// 文本隐层。
pub const HIDDEN: usize = 1024;
/// 文本中间层。
pub const INTERMEDIATE: usize = 3072;
/// 文本层数。
pub const TEXT_LAYERS: usize = 28;
/// 查询头。
pub const QUERY_HEADS: usize = 16;
/// 键值头。
pub const KV_HEADS: usize = 8;
/// 文本头维。
pub const TEXT_HEAD_DIM: usize = 128;
/// MRoPE 的 theta。
pub const ROPE_THETA: f32 = 1_000_000.0;
/// MRoPE 的位置轴数。
pub const ROPE_SECTIONS: usize = 3;
/// 三维 MRoPE 按轴分配的频率数：时间、高、宽。和等于 `TEXT_HEAD_DIM / 2`。
pub const MROPE_SECTION: [usize; ROPE_SECTIONS] = [24, 20, 20];
/// RMSNorm 的 eps。
pub const RMS_EPS: f32 = 1e-6;
/// 音频开始。
pub const AUDIO_START: u32 = 151669;
/// 音频占位。
pub const AUDIO_PAD: u32 = 151676;
/// 音频结束。
pub const AUDIO_END: u32 = 151670;
/// 分组存放的组大小。两座塔共用这一个数。
pub const GROUP_SIZE: usize = 64;
/// 文本塔分组权重每组几位。
pub const TEXT_GROUP_BITS: usize = 4;
/// 音频塔分组权重每组几位。与 `quantization_config.json` 的 `audio_tower_bits` 一致。
pub const AUDIO_GROUP_BITS: usize = 8;
/// 编码器线性层带偏置。
pub const ENCODER_LINEAR_BIAS: bool = true;
/// 文本塔线性层不带偏置。
pub const TEXT_LINEAR_BIAS: bool = false;
/// 词嵌入与输出头绑表。
pub const TIE_EMBEDDINGS: bool = true;

/// 卷积茎一次下采样后的长度。核 3、步长 2、填充 1，等于 `(n - 1) / 2 + 1`。
///
/// `len` 至少为 1。
pub const fn conv_stem_out_len(len: usize) -> usize {
    (len + 2 * CONV_PAD - CONV_KERNEL) / CONV_STRIDE + 1
}

/// 正弦位置编码的半维。分母用这个数减 1。
pub const fn position_half_dim(width: usize) -> usize {
    width / 2
}

/// float16 层输出的裁剪上界。
pub const fn f16_clip_limit() -> f32 {
    F16_ABS_MAX - F16_CLIP_MARGIN as f32
}

const fn stem(in_ch: usize, out_ch: usize) -> ConvStem {
    ConvStem {
        in_ch,
        out_ch,
        kernel: CONV_KERNEL,
        stride: CONV_STRIDE,
        pad: CONV_PAD,
    }
}

/// 0.6B 配置。
pub const QWEN3_ASR_0_6B: AsrProfile = AsrProfile {
    mel_bins: MEL_BINS,
    encoder_layers: ENCODER_LAYERS,
    encoder_heads: ENCODER_HEADS,
    encoder_head_dim: ENCODER_HEAD_DIM,
    encoder_ffn: ENCODER_FFN,
    d_model: D_MODEL,
    projection_out: PROJECTION_OUT,
    n_window: N_WINDOW,
    chunk_frames: CHUNK_FRAMES,
    n_window_infer: N_WINDOW_INFER,
    downsample_hidden: DOWNSAMPLE_HIDDEN,
    encoder_linear_bias: ENCODER_LINEAR_BIAS,
    conv_stem: [
        stem(1, DOWNSAMPLE_HIDDEN),
        stem(DOWNSAMPLE_HIDDEN, DOWNSAMPLE_HIDDEN),
        stem(DOWNSAMPLE_HIDDEN, DOWNSAMPLE_HIDDEN),
    ],
    stem_freq: STEM_FREQ,
    stem_linear_in: STEM_LINEAR_IN,
    tokens_per_chunk: TOKENS_PER_CHUNK,
    max_position: MAX_POSITION,
    window_split_at: WINDOW_SPLIT_AT,
    f16_clip_margin: F16_CLIP_MARGIN,
    vocab: VOCAB_SIZE,
    hidden: HIDDEN,
    intermediate: INTERMEDIATE,
    text_layers: TEXT_LAYERS,
    query_heads: QUERY_HEADS,
    kv_heads: KV_HEADS,
    text_head_dim: TEXT_HEAD_DIM,
    rope_theta: ROPE_THETA,
    rope_sections: ROPE_SECTIONS,
    rms_eps: RMS_EPS,
    text_linear_bias: TEXT_LINEAR_BIAS,
    tie_embeddings: TIE_EMBEDDINGS,
    audio_start: AUDIO_START,
    audio_pad: AUDIO_PAD,
    audio_end: AUDIO_END,
    group_size: GROUP_SIZE,
};
