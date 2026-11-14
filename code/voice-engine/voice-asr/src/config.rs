//! Qwen3-ASR 0.6B 的音频塔与文本塔配置。1.7B 只替换这些数字，层种类不变。

/// 卷积茎里的一层。输入按 NHWC 解释，核固定为 3，步幅 2，填充 1。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conv2dSpec {
    pub in_ch: usize,
    pub out_ch: usize,
    pub kernel: usize,
    pub stride: usize,
    pub pad: usize,
}

impl Conv2dSpec {
    pub const fn stem(in_ch: usize, out_ch: usize) -> Self {
        Self {
            in_ch,
            out_ch,
            kernel: 3,
            stride: 2,
            pad: 1,
        }
    }
}

/// 音频塔。注意力双向，没有 RoPE；窗与窗之间不互看。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EncoderConfig {
    /// 梅尔频点数。
    pub mel_bins: usize,
    /// 变换层数。
    pub layers: usize,
    /// 注意力头数。
    pub heads: usize,
    /// 每头维度。0.6B 为 `d_model / heads`。
    pub head_dim: usize,
    /// 前馈宽度，激活是 GELU。
    pub ffn: usize,
    /// 编码器隐层宽度。
    pub d_model: usize,
    /// 投影到文本隐层的宽度。
    pub proj_out: usize,
    /// 官方块参数。块长是 `n_window * 2` 个梅尔帧。
    pub n_window: usize,
    /// 一块的梅尔帧数。
    pub chunk_frames: usize,
    /// 推理时一个注意力窗的梅尔帧数，8 秒。
    pub n_window_infer: usize,
    /// 卷积茎的通道数，也是下采样隐层。
    pub downsample_hidden: usize,
    /// 三次 stride-2 之后频率维剩 16，线性层输入宽度是 `downsample_hidden * 16`。
    pub stem_linear_in: usize,
    /// 满块卷积后的音频 token 数。
    pub tokens_per_full_chunk: usize,
    /// 正弦位置表的最大位置。每一块从 0 重新加。
    pub max_position: usize,
    /// 正弦表分母用的底。
    pub position_base: f64,
    /// 窗数达到该值时按窗切开算注意力，否则用一块对角加法 mask。
    pub window_split_at: usize,
    /// `float16` 时每层输出裁到该类型最大值减去这个余量。
    pub f16_clip_margin: i32,
    /// 注意力与前馈的线性层、以及 LayerNorm，都带 bias。
    pub linear_bias: bool,
    /// 位置表以 f32 存放，加进隐层之前要转成隐层的 dtype。
    pub position_table_f32: bool,
    /// 每一块的位置从 0 重新编号。
    pub positions_restart_each_chunk: bool,
    pub conv_stem: [Conv2dSpec; 3],
}

impl EncoderConfig {
    /// Qwen3-ASR 0.6B 音频塔。
    pub const fn qwen3_asr_0_6b() -> Self {
        let downsample_hidden = 480;
        Self {
            mel_bins: 128,
            layers: 18,
            heads: 14,
            head_dim: 64,
            ffn: 3584,
            d_model: 896,
            proj_out: 1024,
            n_window: 50,
            chunk_frames: 100,
            n_window_infer: 800,
            downsample_hidden,
            stem_linear_in: downsample_hidden * 16,
            tokens_per_full_chunk: 13,
            max_position: 1500,
            position_base: 10_000.0,
            window_split_at: 20,
            f16_clip_margin: 1000,
            linear_bias: true,
            position_table_f32: true,
            positions_restart_each_chunk: true,
            conv_stem: [
                Conv2dSpec::stem(1, downsample_hidden),
                Conv2dSpec::stem(downsample_hidden, downsample_hidden),
                Conv2dSpec::stem(downsample_hidden, downsample_hidden),
            ],
        }
    }

    /// `log(base) / (half_dim - 1)`。分母是 `half_dim - 1`，不是 `half_dim`。
    pub fn sinusoidal_log_increment(self) -> f64 {
        let half = self.d_model / 2;
        self.position_base.ln() / (half as f64 - 1.0)
    }
}

/// 文本塔。线性层无 bias，词嵌入与 `lm_head` 绑同一张表。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextTowerConfig {
    pub vocab: usize,
    pub hidden: usize,
    pub intermediate: usize,
    pub layers: usize,
    pub query_heads: usize,
    pub kv_heads: usize,
    pub head_dim: usize,
    /// SiLU 门控，也就是 SwiGLU。
    pub silu_swiglu: bool,
    /// 三维 MRoPE 的位置编号形状是 `(B, 3, L)`。
    pub mrope_axes: usize,
    pub rope_theta: f64,
    pub rms_eps: f64,
    pub linear_bias: bool,
    pub tied_embeddings: bool,
    pub audio_start: u32,
    pub audio_pad: u32,
    pub audio_end: u32,
    /// 最近若干 token 允许被下一编码步替换。
    pub unfixed_tokens: usize,
}

impl TextTowerConfig {
    /// Qwen3-ASR 0.6B 文本塔。
    pub const fn qwen3_asr_0_6b() -> Self {
        Self {
            vocab: 151_936,
            hidden: 1024,
            intermediate: 3072,
            layers: 28,
            query_heads: 16,
            kv_heads: 8,
            head_dim: 128,
            silu_swiglu: true,
            mrope_axes: 3,
            rope_theta: 1_000_000.0,
            rms_eps: 1e-6,
            linear_bias: false,
            tied_embeddings: true,
            audio_start: 151_669,
            audio_pad: 151_676,
            audio_end: 151_670,
            unfixed_tokens: 5,
        }
    }
}

/// 采样、编码步与两座塔。
#[derive(Clone, Debug, PartialEq)]
pub struct AsrConfig {
    pub encoder: EncoderConfig,
    pub text: TextTowerConfig,
    pub sample_rate: u32,
    /// 凑满这么多个 16 kHz 样本才跑一次非定稿编码步。
    pub step_samples: usize,
    /// 梅尔帧移。不足一帧的样本留在 `pcm_tail`。
    pub hop_length: usize,
    /// 中文输入写进助手前缀的语言标签。
    pub language: String,
}

impl AsrConfig {
    pub fn qwen3_asr_0_6b() -> Self {
        Self {
            encoder: EncoderConfig::qwen3_asr_0_6b(),
            text: TextTowerConfig::qwen3_asr_0_6b(),
            sample_rate: 16_000,
            step_samples: 32_000,
            hop_length: 160,
            language: "Chinese".to_string(),
        }
    }

    pub fn frames_per_step(&self) -> usize {
        self.step_samples / self.hop_length
    }
}

/// 一次卷积的时间维输出长度：`((n - 1) // 2) + 1`，重复三次。
pub fn conv_out_len(frames: usize) -> usize {
    let mut n = frames;
    for _ in 0..3 {
        n = n.saturating_sub(1) / 2 + 1;
    }
    n
}

/// 梅尔帧换成音频 token。满块直接是 13。尾块先补到块长再卷积，这里只取有效 token 个数。
pub fn audio_tokens(frames: usize, encoder: EncoderConfig) -> usize {
    if frames == 0 {
        return 0;
    }
    let chunk = encoder.chunk_frames;
    let full = frames / chunk;
    let rem = frames % chunk;
    let tail = if rem == 0 {
        0
    } else {
        conv_out_len(rem).min(encoder.tokens_per_full_chunk)
    };
    full * encoder.tokens_per_full_chunk + tail
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_matches_section_4() {
        let c = EncoderConfig::qwen3_asr_0_6b();
        assert_eq!(c.mel_bins, 128);
        assert_eq!(c.layers, 18);
        assert_eq!(c.heads, 14);
        assert_eq!(c.head_dim, c.d_model / c.heads);
        assert_eq!(c.head_dim, 64);
        assert_eq!(c.ffn, 3584);
        assert_eq!(c.d_model, 896);
        assert_eq!(c.proj_out, 1024);
        assert_eq!(c.n_window, 50);
        assert_eq!(c.chunk_frames, c.n_window * 2);
        assert_eq!(c.n_window_infer, 800);
        assert_eq!(c.downsample_hidden, 480);
        assert_eq!(c.stem_linear_in, 480 * 16);
        assert_eq!(c.stem_linear_in, 7680);
        assert_eq!(c.tokens_per_full_chunk, 13);
        assert_eq!(conv_out_len(c.chunk_frames), 13);
        assert_eq!(c.max_position, 1500);
        assert!(c.linear_bias);
        assert!(c.position_table_f32);
        assert!(c.positions_restart_each_chunk);
        assert_eq!(c.window_split_at, 20);
        assert_eq!(c.f16_clip_margin, 1000);
        assert_eq!(c.conv_stem[0], Conv2dSpec::stem(1, 480));
        assert_eq!(c.conv_stem[1], Conv2dSpec::stem(480, 480));
        assert_eq!(c.conv_stem[2], Conv2dSpec::stem(480, 480));
        let half = c.d_model / 2;
        let expect = 10_000f64.ln() / (half as f64 - 1.0);
        assert_eq!(c.sinusoidal_log_increment(), expect);
    }

    #[test]
    fn text_tower_matches_section_4() {
        let c = TextTowerConfig::qwen3_asr_0_6b();
        assert_eq!(c.vocab, 151_936);
        assert_eq!(c.hidden, 1024);
        assert_eq!(c.intermediate, 3072);
        assert_eq!(c.layers, 28);
        assert_eq!(c.query_heads, 16);
        assert_eq!(c.kv_heads, 8);
        assert_eq!(c.head_dim, 128);
        assert!(c.silu_swiglu);
        assert_eq!(c.mrope_axes, 3);
        assert_eq!(c.rope_theta, 1_000_000.0);
        assert_eq!(c.rms_eps, 1e-6);
        assert!(!c.linear_bias);
        assert!(c.tied_embeddings);
        assert_eq!(c.audio_start, 151_669);
        assert_eq!(c.audio_pad, 151_676);
        assert_eq!(c.audio_end, 151_670);
        assert_eq!(c.unfixed_tokens, 5);
        assert_eq!(c.hidden, voice_tensor::HIDDEN);
    }

    #[test]
    fn partial_chunk_is_not_a_full_thirteen() {
        let c = EncoderConfig::qwen3_asr_0_6b();
        assert_eq!(audio_tokens(0, c), 0);
        assert_eq!(audio_tokens(100, c), 13);
        assert_eq!(audio_tokens(200, c), 26);
        assert_eq!(audio_tokens(800, c), 104);
        assert!(audio_tokens(50, c) < 13);
    }
}
