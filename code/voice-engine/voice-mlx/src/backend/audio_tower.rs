//! 音频塔：权重来源与一次前向。
//!
//! 权重全部来自 `weights.safetensors` 的 `audio_tower.*`：三层卷积是稠密 f16，
//! 线性层是 8 位分组权重（组大小 64，键名带 `.scales` 与 `.biases`），
//! LayerNorm 是稠密 f16。卷积茎与 `conv_out` 之外的层全部是
//! `LayerNorm + 双向注意力 + 前馈`，没有 RoPE，没有因果遮罩。
//!
//! 前向按 042 第 4 节：每 100 个梅尔帧一块，逐块过卷积茎；
//! `(频, 时, 通道)` 转成 `(时, 通道*频)` 后过 `conv_out`；
//! 每块从位置 0 加正弦表；注意力按 800 帧一个窗切成块对角；末尾
//! `ln_post → proj1 → GELU → proj2`。

use voice_tensor::{
    f16_clip_limit, AxisSlice, Conv2d, DType, LayerNorm, Linear, PadWidth, TensorMeta, TensorOps,
    AUDIO_GROUP_BITS, CHUNK_FRAMES, D_MODEL, ENCODER_HEADS, ENCODER_HEAD_DIM, ENCODER_LAYERS,
    GROUP_SIZE, MAX_POSITION, MEL_BINS, N_WINDOW_INFER, PROJECTION_OUT, STEM_FREQ, STEM_LINEAR_IN,
    TOKENS_PER_CHUNK, WINDOW_SPLIT_AT,
};

use super::weights::SafeWeights;
use super::{MlxBackend, MlxError, MlxTensor};

/// 音频塔的层归一化稳定项。参考用 `nn.LayerNorm` 的默认值。
pub const LAYER_NORM_EPS: f32 = 1e-5;

/// 正弦位置表的分母。官方用 `half_dim - 1`；`HalfDim` 只用于归因对照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionDenominator {
    /// `log(10000) / (half_dim - 1)`。042 第 4 节与官方一致的那一条。
    HalfDimMinusOne,
    /// `log(10000) / half_dim`。差一位，用来证明对照能分辨这两种公式。
    HalfDim,
}

impl PositionDenominator {
    fn increment(self, half: usize) -> f32 {
        let divisor = match self {
            Self::HalfDimMinusOne => half as f64 - 1.0,
            Self::HalfDim => half as f64,
        };
        ((10_000.0f64).ln() / divisor) as f32
    }
}

/// 前向的三个可替换口径。默认值是 042 与官方实现的取法；
/// 换掉其中一条，就是为了看差值落在哪一步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncoderOptions {
    /// 位置表的分母。
    pub position_denominator: PositionDenominator,
    /// 每一块是否从位置 0 重新编号。官方是。
    pub restart_positions_each_chunk: bool,
    /// 一个注意力窗覆盖多少个梅尔帧。官方是 800，即 8 秒。
    pub window_frames: usize,
}

impl Default for EncoderOptions {
    fn default() -> Self {
        Self {
            position_denominator: PositionDenominator::HalfDimMinusOne,
            restart_positions_each_chunk: true,
            window_frames: N_WINDOW_INFER,
        }
    }
}

/// 音频塔的全部权重。
pub struct AudioTower {
    conv: [Conv2d<MlxTensor>; 3],
    conv_out: Linear<MlxTensor>,
    layers: Vec<AudioLayer>,
    ln_post: LayerNorm<MlxTensor>,
    proj1: Linear<MlxTensor>,
    proj2: Linear<MlxTensor>,
}

struct AudioLayer {
    attn_norm: LayerNorm<MlxTensor>,
    q: Linear<MlxTensor>,
    k: Linear<MlxTensor>,
    v: Linear<MlxTensor>,
    out: Linear<MlxTensor>,
    ffn_norm: LayerNorm<MlxTensor>,
    fc1: Linear<MlxTensor>,
    fc2: Linear<MlxTensor>,
}

impl AudioTower {
    /// 打开 `model_dir/weights.safetensors` 并读出音频塔的全部权重。
    pub fn open(model_dir: &str, dtype: DType) -> Result<Self, MlxError> {
        let mut weights = SafeWeights::open(model_dir)?;
        Self::load(&mut weights, dtype)
    }

    /// 按 `audio_tower.*` 的键名把权重读进来，统一转成 `dtype`。
    ///
    /// 参考里权重与激活同为 f16，`nn.QuantizedLinear` 反量化后也是 f16；
    /// 这里同样在装载时定死成 `dtype`，不在前向里做类型提升。
    pub(crate) fn load(weights: &mut SafeWeights, dtype: DType) -> Result<Self, MlxError> {
        let conv = [
            weights.conv2d("audio_tower.conv2d1", dtype)?,
            weights.conv2d("audio_tower.conv2d2", dtype)?,
            weights.conv2d("audio_tower.conv2d3", dtype)?,
        ];
        let affine = |weights: &mut SafeWeights, name: &str| {
            weights.affine_as(name, GROUP_SIZE, AUDIO_GROUP_BITS, dtype)
        };
        let conv_out = affine(weights, "audio_tower.conv_out")?;
        let mut layers = Vec::with_capacity(ENCODER_LAYERS);
        for index in 0..ENCODER_LAYERS {
            let at = |suffix: &str| format!("audio_tower.layers.{index}.{suffix}");
            layers.push(AudioLayer {
                attn_norm: weights.layer_norm(
                    &at("self_attn_layer_norm"),
                    LAYER_NORM_EPS,
                    dtype,
                )?,
                q: affine(weights, &at("self_attn.q_proj"))?,
                k: affine(weights, &at("self_attn.k_proj"))?,
                v: affine(weights, &at("self_attn.v_proj"))?,
                out: affine(weights, &at("self_attn.out_proj"))?,
                ffn_norm: weights.layer_norm(&at("final_layer_norm"), LAYER_NORM_EPS, dtype)?,
                fc1: affine(weights, &at("fc1"))?,
                fc2: affine(weights, &at("fc2"))?,
            });
        }
        Ok(Self {
            conv,
            conv_out,
            layers,
            ln_post: weights.layer_norm("audio_tower.ln_post", LAYER_NORM_EPS, dtype)?,
            proj1: affine(weights, "audio_tower.proj1")?,
            proj2: affine(weights, "audio_tower.proj2")?,
        })
    }

    /// 层数。参考是 18。
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }
}

/// 一次前向的结果。三个中间量按对照口径排好：
///
/// - `stem_chunk0`：第 0 块的卷积茎输出，`(STEM_FREQ, tokens_per_chunk, 480)`，NHWC；
/// - `layer0`：第 0 层含残差的输出，`(1, tokens, 896)`；
/// - `proj`：`ln_post/proj1/proj2` 之后，`(tokens, 1024)`。
pub struct EncoderOutput {
    /// 音频 token 数。
    pub tokens: usize,
    /// 第 0 块的卷积茎输出，行优先 f32。
    pub stem_chunk0: Vec<f32>,
    /// 第 0 块的时间维长度。满块是 13。
    pub stem_time: usize,
    /// 第 0 层输出，行优先 f32。
    pub layer0: Vec<f32>,
    /// 投影输出，行优先 f32。
    pub proj: Vec<f32>,
    /// 注意力窗口的累计边界，例如 `[0, 26]`。
    pub cu_seqlens: Vec<usize>,
}

impl EncoderOutput {
    /// 卷积茎第 0 块的形状。
    pub fn stem_chunk0_shape(&self) -> [usize; 3] {
        [STEM_FREQ, self.stem_time, 480]
    }

    /// 第 0 层输出的形状。
    pub fn layer0_shape(&self) -> [usize; 3] {
        [1, self.tokens, D_MODEL]
    }

    /// 投影输出的形状。
    pub fn proj_shape(&self) -> [usize; 2] {
        [self.tokens, PROJECTION_OUT]
    }
}

/// 用一段梅尔谱跑完音频塔。
///
/// `mel` 是 `(MEL_BINS, frames)` 的行优先 f32，参考在送进编码器之前才转成
/// `dtype`，所以这里也先按 f32 收，再按 `dtype` 放上设备。
pub fn encode_mel(
    ops: &mut MlxBackend,
    tower: &AudioTower,
    mel: &[f32],
    frames: usize,
    dtype: DType,
) -> Result<EncoderOutput, MlxError> {
    encode_mel_with(ops, tower, mel, frames, dtype, &EncoderOptions::default())
}

/// 与 [`encode_mel`] 相同，但可以替换位置表分母、位置编号与窗宽。
///
/// 对照测试用非默认口径跑一遍，看差值从哪一步开始变大；产品路径只用默认值。
pub fn encode_mel_with(
    ops: &mut MlxBackend,
    tower: &AudioTower,
    mel: &[f32],
    frames: usize,
    dtype: DType,
    options: &EncoderOptions,
) -> Result<EncoderOutput, MlxError> {
    if frames == 0 {
        return Err(MlxError::new("梅尔帧数为 0"));
    }
    if mel.len() != MEL_BINS * frames {
        return Err(MlxError::new(format!(
            "梅尔元素个数 {} 与 {MEL_BINS}×{frames} 不一致",
            mel.len()
        )));
    }
    let mel = ops.literal(mel, &[MEL_BINS, frames], dtype)?;

    // --- 逐块卷积茎 ---
    let n_full = frames / CHUNK_FRAMES;
    let mut parts: Vec<MlxTensor> = Vec::new();
    let mut chunk_lens: Vec<usize> = Vec::new();
    let mut stem_chunk0: Option<Vec<f32>> = None;
    let mut stem_time = 0usize;

    if n_full > 0 {
        let used = n_full * CHUNK_FRAMES;
        let full = ops.slice(&mel, &[AxisSlice::full(MEL_BINS), AxisSlice::new(0, used)])?;
        let full = ops.reshape(
            &full,
            &[MEL_BINS as isize, n_full as isize, CHUNK_FRAMES as isize],
        )?;
        let full = ops.transpose(&full, &[1, 0, 2])?;
        let full = ops.reshape(
            &full,
            &[n_full as isize, MEL_BINS as isize, CHUNK_FRAMES as isize, 1],
        )?;
        let stem = conv_stem(ops, tower, &full)?;
        let time = stem.dims()[2];
        chunk_lens.extend(std::iter::repeat(time).take(n_full));
        stem_chunk0 = Some(take_chunk0(ops, &stem)?);
        stem_time = time;
        parts.push(flatten_stem(ops, &stem)?);
    }

    // --- 尾块：先按官方方式补到块长再卷积，再切回有效 token ---
    let used = n_full * CHUNK_FRAMES;
    if used < frames {
        let tail_len = frames - used;
        let valid_tail_tokens = (tail_len + 7) / 8;
        let mut tail = ops.slice(
            &mel,
            &[AxisSlice::full(MEL_BINS), AxisSlice::new(used, frames)],
        )?;
        if n_full > 0 {
            tail = ops.pad(
                &tail,
                &[PadWidth::none(), PadWidth::new(0, CHUNK_FRAMES - tail_len)],
            )?;
        }
        let width = tail.dims()[1];
        let tail = ops.reshape(&tail, &[1, MEL_BINS as isize, width as isize, 1])?;
        let stem = conv_stem(ops, tower, &tail)?;
        let time = stem.dims()[2].min(valid_tail_tokens);
        let stem = ops.slice(
            &stem,
            &[
                AxisSlice::full(1),
                AxisSlice::full(STEM_FREQ),
                AxisSlice::new(0, time),
                AxisSlice::full(480),
            ],
        )?;
        chunk_lens.push(time);
        if stem_chunk0.is_none() {
            stem_chunk0 = Some(take_chunk0(ops, &stem)?);
            stem_time = time;
        }
        parts.push(flatten_stem(ops, &stem)?);
    }

    let flat = if parts.len() == 1 {
        parts.remove(0)
    } else {
        let refs: Vec<&MlxTensor> = parts.iter().collect();
        ops.concatenate(&refs, 0)?
    };

    // --- 通道优先重排之后过 conv_out ---
    let hidden = ops.linear(&flat, &tower.conv_out)?;
    let tokens = hidden.dims()[0];
    if hidden.dims()[1] != D_MODEL {
        return Err(MlxError::new(format!(
            "conv_out 输出宽度 {}，需要 {D_MODEL}",
            hidden.dims()[1]
        )));
    }

    // --- 每块从位置 0 加正弦表 ---
    let table = sinusoidal_table(
        &chunk_lens,
        D_MODEL,
        options.position_denominator,
        options.restart_positions_each_chunk,
    )?;
    let table = ops.literal(&table, &[tokens, D_MODEL], dtype)?;
    let hidden = ops.add(&hidden, &table)?;
    let hidden = ops.reshape(&hidden, &[1, tokens as isize, D_MODEL as isize])?;

    // --- 窗与窗之间互不看 ---
    if options.window_frames == 0 || !options.window_frames.is_multiple_of(CHUNK_FRAMES) {
        return Err(MlxError::new(format!(
            "窗宽 {} 不是块长 {CHUNK_FRAMES} 的整数倍",
            options.window_frames
        )));
    }
    let tokens_per_window = TOKENS_PER_CHUNK * (options.window_frames / CHUNK_FRAMES);
    let mut cu_seqlens = vec![0usize];
    let mut pos = 0usize;
    while pos < tokens {
        let end = (pos + tokens_per_window).min(tokens);
        cu_seqlens.push(end);
        pos = end;
    }
    let windows = cu_seqlens.len() - 1;
    let mask = if windows <= 1 {
        None
    } else if windows < WINDOW_SPLIT_AT {
        Some(additive_mask(&cu_seqlens, tokens, dtype, ops)?)
    } else {
        None
    };

    let mut layer0: Option<Vec<f32>> = None;
    let hidden = if windows < WINDOW_SPLIT_AT {
        let mut hidden = hidden;
        for (index, layer) in tower.layers.iter().enumerate() {
            hidden = audio_layer(ops, layer, &hidden, mask.as_ref(), dtype)?;
            if index == 0 {
                layer0 = Some(ops.to_f32(&hidden)?);
            }
        }
        hidden
    } else {
        // 窗数达到门槛时按窗切开算，数学上等价于块对角 mask。
        let mut hidden = hidden;
        for (index, layer) in tower.layers.iter().enumerate() {
            let mut pieces = Vec::with_capacity(windows);
            for window in 0..windows {
                let (start, end) = (cu_seqlens[window], cu_seqlens[window + 1]);
                let piece = ops.slice(
                    &hidden,
                    &[
                        AxisSlice::full(1),
                        AxisSlice::new(start, end),
                        AxisSlice::full(D_MODEL),
                    ],
                )?;
                pieces.push(audio_layer(ops, layer, &piece, None, dtype)?);
            }
            let refs: Vec<&MlxTensor> = pieces.iter().collect();
            hidden = ops.concatenate(&refs, 1)?;
            if index == 0 {
                layer0 = Some(ops.to_f32(&hidden)?);
            }
        }
        hidden
    };
    // --- 窗后投影 ---
    let hidden = ops.reshape(&hidden, &[tokens as isize, D_MODEL as isize])?;
    let hidden = ops.layer_norm(&hidden, &tower.ln_post)?;
    let hidden = ops.linear(&hidden, &tower.proj1)?;
    let hidden = ops.gelu(&hidden)?;
    let projected = ops.linear(&hidden, &tower.proj2)?;
    let proj = ops.to_f32(&projected)?;

    Ok(EncoderOutput {
        tokens,
        stem_chunk0: stem_chunk0.unwrap_or_default(),
        stem_time,
        layer0: layer0.unwrap_or_default(),
        proj,
        cu_seqlens,
    })
}

fn conv_stem(
    ops: &mut MlxBackend,
    tower: &AudioTower,
    input: &MlxTensor,
) -> Result<MlxTensor, MlxError> {
    let mut x = ops.conv2d(input, &tower.conv[0])?;
    x = ops.gelu(&x)?;
    x = ops.conv2d(&x, &tower.conv[1])?;
    x = ops.gelu(&x)?;
    x = ops.conv2d(&x, &tower.conv[2])?;
    ops.gelu(&x)
}

/// `(n, 频, 时, 通道)` → `(n*时, 通道*频)`。通道在前，与 `conv_out` 的权重布局一致。
fn flatten_stem(ops: &mut MlxBackend, stem: &MlxTensor) -> Result<MlxTensor, MlxError> {
    let batch = stem.dims()[0];
    let time = stem.dims()[2];
    let channels = stem.dims()[3];
    if stem.dims()[1] != STEM_FREQ || channels != 480 {
        return Err(MlxError::new(format!(
            "卷积茎输出形状 {:?} 与 (n, {STEM_FREQ}, t, 480) 不一致",
            stem.dims()
        )));
    }
    let ordered = ops.transpose(stem, &[0, 2, 3, 1])?;
    ops.reshape(
        &ordered,
        &[batch as isize * time as isize, STEM_LINEAR_IN as isize],
    )
}

/// 取第 0 块，把 `(1, 频, 时, 通道)` 读回主机。
fn take_chunk0(ops: &mut MlxBackend, stem: &MlxTensor) -> Result<Vec<f32>, MlxError> {
    let chunk = ops.slice(
        stem,
        &[
            AxisSlice::new(0, 1),
            AxisSlice::full(STEM_FREQ),
            AxisSlice::full(stem.dims()[2]),
            AxisSlice::full(stem.dims()[3]),
        ],
    )?;
    ops.to_f32(&chunk)
}

/// 正弦位置表：`log(10000) / (half_dim - 1)`，`[sin | cos]`，f32 存表。
///
/// 每一块从位置 0 重新编号，所以这里按块长拼接同一张表的前若干行。
fn sinusoidal_table(
    chunk_lens: &[usize],
    width: usize,
    denominator: PositionDenominator,
    restart_each_chunk: bool,
) -> Result<Vec<f32>, MlxError> {
    let half = width / 2;
    if half < 2 {
        return Err(MlxError::new("位置表的半维太小"));
    }
    let longest = chunk_lens
        .iter()
        .copied()
        .max()
        .ok_or_else(|| MlxError::new("没有块"))?;
    if longest > MAX_POSITION {
        return Err(MlxError::new(format!(
            "块长 {longest} 超过位置表上限 {MAX_POSITION}"
        )));
    }
    // 分母是 half - 1，不是 half。参考的实现与官方一致。
    let increment = denominator.increment(half);
    let inv_timescales: Vec<f32> = (0..half)
        .map(|index| -increment * index as f32)
        .map(f32::exp)
        .collect();
    let mut total = 0usize;
    for len in chunk_lens {
        total += len;
    }
    let mut table = Vec::with_capacity(total * width);
    let mut base = 0usize;
    for len in chunk_lens {
        for position in 0..*len {
            let time = if restart_each_chunk {
                position
            } else {
                base + position
            } as f32;
            for inv in &inv_timescales {
                table.push((time * inv).sin());
            }
            for inv in &inv_timescales {
                table.push((time * inv).cos());
            }
        }
        base += *len;
    }
    Ok(table)
}

/// 块对角加法遮罩：同一窗内为 0，跨窗为 `dtype` 的最小有限值。
fn additive_mask(
    cu_seqlens: &[usize],
    tokens: usize,
    dtype: DType,
    ops: &mut MlxBackend,
) -> Result<MlxTensor, MlxError> {
    let floor = match dtype {
        DType::F16 => -65504.0f32,
        _ => f32::MIN,
    };
    let mut window_of = vec![0usize; tokens];
    for window in 0..cu_seqlens.len() - 1 {
        for position in cu_seqlens[window]..cu_seqlens[window + 1] {
            window_of[position] = window;
        }
    }
    let mut mask = Vec::with_capacity(tokens * tokens);
    for row in 0..tokens {
        for column in 0..tokens {
            mask.push(if window_of[row] == window_of[column] {
                0.0
            } else {
                floor
            });
        }
    }
    ops.literal(&mask, &[1, 1, tokens, tokens], dtype)
}

fn audio_layer(
    ops: &mut MlxBackend,
    layer: &AudioLayer,
    hidden: &MlxTensor,
    mask: Option<&MlxTensor>,
    dtype: DType,
) -> Result<MlxTensor, MlxError> {
    let normed = ops.layer_norm(hidden, &layer.attn_norm)?;
    let query = project_heads(ops, &normed, &layer.q)?;
    let key = project_heads(ops, &normed, &layer.k)?;
    let value = project_heads(ops, &normed, &layer.v)?;
    let attended = ops.attention(&query, &key, &value, mask)?;
    let merged = merge_heads(ops, &attended)?;
    let attended = ops.linear(&merged, &layer.out)?;
    let mut x = ops.add(hidden, &attended)?;
    let normed = ops.layer_norm(&x, &layer.ffn_norm)?;
    let ff = ops.linear(&normed, &layer.fc1)?;
    let ff = ops.gelu(&ff)?;
    let ff = ops.linear(&ff, &layer.fc2)?;
    x = ops.add(&x, &ff)?;
    if dtype == DType::F16 {
        let limit = f16_clip_limit();
        x = ops.clip(&x, -limit, limit)?;
    }
    Ok(x)
}

fn project_heads(
    ops: &mut MlxBackend,
    hidden: &MlxTensor,
    layer: &Linear<MlxTensor>,
) -> Result<MlxTensor, MlxError> {
    let projected = ops.linear(hidden, layer)?;
    let seq = projected.dims().get(1).copied().unwrap_or(0);
    let shaped = ops.reshape(
        &projected,
        &[
            1,
            seq as isize,
            ENCODER_HEADS as isize,
            ENCODER_HEAD_DIM as isize,
        ],
    )?;
    ops.transpose(&shaped, &[0, 2, 1, 3])
}

fn merge_heads(ops: &mut MlxBackend, states: &MlxTensor) -> Result<MlxTensor, MlxError> {
    let seq = states.dims().get(2).copied().unwrap_or(0);
    let shaped = ops.transpose(states, &[0, 2, 1, 3])?;
    ops.reshape(&shaped, &[1, seq as isize, D_MODEL as isize])
}
