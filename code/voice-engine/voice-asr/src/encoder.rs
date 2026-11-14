//! 音频塔与文本塔的一次前向。假后端只检查形状，不产生数值。

use voice_tensor::{
    AxisSlice, Conv2d, DType, LayerNorm, Linear, PadWidth, RmsNorm, TensorMeta, TensorOps,
    QWEN3_ASR_0_6B,
};

use crate::error::{backend, graph, EngineError};

/// 卷积茎、18 层编码器和窗后投影。
pub struct EncoderWeights<T> {
    conv: [Conv2d<T>; 3],
    stem: Linear<T>,
    layers: Vec<EncoderLayer<T>>,
    ln_post: LayerNorm<T>,
    proj1: Linear<T>,
    proj2: Linear<T>,
}

struct EncoderLayer<T> {
    attn_norm: LayerNorm<T>,
    q: Linear<T>,
    k: Linear<T>,
    v: Linear<T>,
    out: Linear<T>,
    ffn_norm: LayerNorm<T>,
    fc1: Linear<T>,
    fc2: Linear<T>,
}

/// 文本塔。词嵌入与输出头各有一份同形状的表。
pub struct TextWeights<T> {
    embed: T,
    layers: Vec<TextLayer<T>>,
    final_norm: RmsNorm<T>,
    lm_head: Linear<T>,
}

struct TextLayer<T> {
    input_norm: RmsNorm<T>,
    q: Linear<T>,
    k: Linear<T>,
    v: Linear<T>,
    q_norm: RmsNorm<T>,
    k_norm: RmsNorm<T>,
    out: Linear<T>,
    post_norm: RmsNorm<T>,
    gate: Linear<T>,
    up: Linear<T>,
    down: Linear<T>,
}

impl<T: TensorMeta + Clone> EncoderWeights<T> {
    /// 用全零句柄把各层的形状立起来。
    pub fn build<B>(ops: &mut B, dtype: DType) -> Result<Self, EngineError<B::Error>>
    where
        B: TensorOps<Tensor = T>,
    {
        let profile = QWEN3_ASR_0_6B;
        let conv = [
            dense_conv(
                ops,
                profile.conv_stem[0].in_ch,
                profile.conv_stem[0].out_ch,
                dtype,
            )?,
            dense_conv(
                ops,
                profile.conv_stem[1].in_ch,
                profile.conv_stem[1].out_ch,
                dtype,
            )?,
            dense_conv(
                ops,
                profile.conv_stem[2].in_ch,
                profile.conv_stem[2].out_ch,
                dtype,
            )?,
        ];
        let stem = dense_linear(ops, profile.stem_linear_in, profile.d_model, false, dtype)?;
        let mut layers = Vec::with_capacity(profile.encoder_layers);
        for _ in 0..profile.encoder_layers {
            layers.push(EncoderLayer {
                attn_norm: dense_layer_norm(ops, profile.d_model, dtype)?,
                q: dense_linear(ops, profile.d_model, profile.d_model, true, dtype)?,
                k: dense_linear(ops, profile.d_model, profile.d_model, true, dtype)?,
                v: dense_linear(ops, profile.d_model, profile.d_model, true, dtype)?,
                out: dense_linear(ops, profile.d_model, profile.d_model, true, dtype)?,
                ffn_norm: dense_layer_norm(ops, profile.d_model, dtype)?,
                fc1: dense_linear(ops, profile.d_model, profile.encoder_ffn, true, dtype)?,
                fc2: dense_linear(ops, profile.encoder_ffn, profile.d_model, true, dtype)?,
            });
        }
        Ok(Self {
            conv,
            stem,
            layers,
            ln_post: dense_layer_norm(ops, profile.d_model, dtype)?,
            proj1: dense_linear(ops, profile.d_model, profile.d_model, true, dtype)?,
            proj2: dense_linear(ops, profile.d_model, profile.projection_out, true, dtype)?,
        })
    }
}

impl<T: TensorMeta + Clone> TextWeights<T> {
    /// 用全零句柄把文本塔的形状立起来。线性层不带偏置。
    pub fn build<B>(ops: &mut B, dtype: DType) -> Result<Self, EngineError<B::Error>>
    where
        B: TensorOps<Tensor = T>,
    {
        let profile = QWEN3_ASR_0_6B;
        let embed = backend(ops.zeros(&[profile.vocab, profile.hidden], dtype))?;
        let q_out = profile.query_heads * profile.text_head_dim;
        let kv_out = profile.kv_heads * profile.text_head_dim;
        let mut layers = Vec::with_capacity(profile.text_layers);
        for _ in 0..profile.text_layers {
            layers.push(TextLayer {
                input_norm: dense_rms(ops, profile.hidden, profile.rms_eps, dtype)?,
                q: dense_linear(ops, profile.hidden, q_out, false, dtype)?,
                k: dense_linear(ops, profile.hidden, kv_out, false, dtype)?,
                v: dense_linear(ops, profile.hidden, kv_out, false, dtype)?,
                q_norm: dense_rms(ops, profile.text_head_dim, profile.rms_eps, dtype)?,
                k_norm: dense_rms(ops, profile.text_head_dim, profile.rms_eps, dtype)?,
                out: dense_linear(ops, q_out, profile.hidden, false, dtype)?,
                post_norm: dense_rms(ops, profile.hidden, profile.rms_eps, dtype)?,
                gate: dense_linear(ops, profile.hidden, profile.intermediate, false, dtype)?,
                up: dense_linear(ops, profile.hidden, profile.intermediate, false, dtype)?,
                down: dense_linear(ops, profile.intermediate, profile.hidden, false, dtype)?,
            });
        }
        Ok(Self {
            embed,
            layers,
            final_norm: dense_rms(ops, profile.hidden, profile.rms_eps, dtype)?,
            lm_head: dense_linear(ops, profile.hidden, profile.vocab, false, dtype)?,
        })
    }
}

/// 把若干梅尔帧编成 `(1, token, 1024)`。不满一块时先补到块长再卷积，然后切回有效 token。
pub fn encode_frames<B: TensorOps>(
    ops: &mut B,
    weights: &EncoderWeights<B::Tensor>,
    frames: usize,
    dtype: DType,
) -> Result<B::Tensor, EngineError<B::Error>> {
    let profile = QWEN3_ASR_0_6B;
    let mut chunks = Vec::new();
    let mut offset = 0;
    while offset < frames {
        let valid = (frames - offset).min(profile.chunk_frames);
        chunks.push(encode_chunk(ops, weights, valid, dtype)?);
        offset += valid;
    }
    let hidden = concat_time(ops, &chunks)?;
    let mut hidden = run_encoder_layers(ops, weights, hidden, dtype)?;
    hidden = backend(ops.layer_norm(&hidden, &weights.ln_post))?;
    hidden = backend(ops.linear(&hidden, &weights.proj1))?;
    hidden = backend(ops.gelu(&hidden))?;
    backend(ops.linear(&hidden, &weights.proj2))
}

/// 预填充。音频占位用累积和下标换成音频向量，只留下最后一个位置的 logits。
pub fn decode_prefill<B: TensorOps>(
    ops: &mut B,
    weights: &TextWeights<B::Tensor>,
    audio: &B::Tensor,
    dtype: DType,
) -> Result<(), EngineError<B::Error>> {
    let profile = QWEN3_ASR_0_6B;
    let audio_tokens = audio.dims().get(1).copied().unwrap_or(0);
    if audio_tokens == 0 {
        return Ok(());
    }
    let seq = audio_tokens + 2;
    let ids = backend(ops.zeros(&[1, seq], DType::I32))?;
    let text = backend(ops.gather(&weights.embed, &ids, 0))?;
    let marker = backend(ops.zeros(&[1, seq], DType::I32))?;
    let cursor = backend(ops.cumsum(&marker, 1))?;
    let gathered = backend(ops.gather(audio, &cursor, 1))?;
    let gathered = backend(ops.reshape(&gathered, &[1, seq as isize, profile.hidden as isize]))?;
    let cond = backend(ops.zeros(&[1, seq, 1], DType::Bool))?;
    let mut hidden = backend(ops.where_op(&cond, &gathered, &text))?;
    let positions = mrope_positions(ops, seq, dtype)?;
    for layer in &weights.layers {
        hidden = text_layer(ops, layer, hidden, &positions, dtype)?;
    }
    hidden = backend(ops.rms_norm(&hidden, &weights.final_norm))?;
    let last = backend(ops.slice(
        &hidden,
        &[
            AxisSlice::full(1),
            AxisSlice::new(seq - 1, seq),
            AxisSlice::full(profile.hidden),
        ],
    ))?;
    let logits = backend(ops.linear(&last, &weights.lm_head))?;
    let _ = backend(ops.max(&logits, 2, false))?;
    Ok(())
}

fn encode_chunk<B: TensorOps>(
    ops: &mut B,
    weights: &EncoderWeights<B::Tensor>,
    valid_frames: usize,
    dtype: DType,
) -> Result<B::Tensor, EngineError<B::Error>> {
    let profile = QWEN3_ASR_0_6B;
    let mut x = backend(ops.zeros(&[1, profile.mel_bins, valid_frames, 1], dtype))?;
    if valid_frames < profile.chunk_frames {
        x = backend(ops.pad(
            &x,
            &[
                PadWidth::none(),
                PadWidth::none(),
                PadWidth::new(0, profile.chunk_frames - valid_frames),
                PadWidth::none(),
            ],
        ))?;
    }
    for conv in &weights.conv {
        x = backend(ops.conv2d(&x, conv))?;
        x = backend(ops.gelu(&x))?;
    }
    x = backend(ops.transpose(&x, &[0, 2, 1, 3]))?;
    let time = x.dims()[1];
    x = backend(ops.reshape(&x, &[1, time as isize, profile.stem_linear_in as isize]))?;
    x = backend(ops.linear(&x, &weights.stem))?;
    let valid_tokens = crate::config::conv_out_len(valid_frames).min(time);
    if valid_tokens < time {
        x = backend(ops.slice(
            &x,
            &[
                AxisSlice::full(1),
                AxisSlice::new(0, valid_tokens),
                AxisSlice::full(profile.d_model),
            ],
        ))?;
    }
    add_chunk_positions(ops, x, dtype)
}

fn add_chunk_positions<B: TensorOps>(
    ops: &mut B,
    tokens: B::Tensor,
    dtype: DType,
) -> Result<B::Tensor, EngineError<B::Error>> {
    let width = QWEN3_ASR_0_6B.d_model;
    let count = tokens.dims()[1];
    let half = width / 2;
    let pos = backend(ops.arange(0, count as i64, 1, dtype))?;
    let freq = backend(ops.arange(0, half as i64, 1, dtype))?;
    let pos = backend(ops.reshape(&pos, &[count as isize, 1]))?;
    let freq = backend(ops.reshape(&freq, &[1, half as isize]))?;
    let angles = backend(ops.mul(&pos, &freq))?;
    let sin = backend(ops.sin(&angles))?;
    let cos = backend(ops.cos(&angles))?;
    let table = backend(ops.concatenate(&[&sin, &cos], 1))?;
    let table = backend(ops.reshape(&table, &[1, count as isize, width as isize]))?;
    backend(ops.add(&tokens, &table))
}

fn run_encoder_layers<B: TensorOps>(
    ops: &mut B,
    weights: &EncoderWeights<B::Tensor>,
    mut hidden: B::Tensor,
    dtype: DType,
) -> Result<B::Tensor, EngineError<B::Error>> {
    let profile = QWEN3_ASR_0_6B;
    let time = hidden.dims()[1];
    let mask = if 1 < profile.window_split_at {
        Some(backend(ops.zeros(&[1, 1, time, time], dtype))?)
    } else {
        None
    };
    for layer in &weights.layers {
        let normed = backend(ops.layer_norm(&hidden, &layer.attn_norm))?;
        let query = project_heads(
            ops,
            &normed,
            &layer.q,
            profile.encoder_heads,
            profile.encoder_head_dim,
        )?;
        let key = project_heads(
            ops,
            &normed,
            &layer.k,
            profile.encoder_heads,
            profile.encoder_head_dim,
        )?;
        let value = project_heads(
            ops,
            &normed,
            &layer.v,
            profile.encoder_heads,
            profile.encoder_head_dim,
        )?;
        let attended = backend(ops.attention(&query, &key, &value, mask.as_ref()))?;
        let attended = merge_heads(ops, attended, profile.d_model)?;
        let attended = backend(ops.linear(&attended, &layer.out))?;
        hidden = backend(ops.add(&hidden, &attended))?;
        let normed = backend(ops.layer_norm(&hidden, &layer.ffn_norm))?;
        let ff = backend(ops.linear(&normed, &layer.fc1))?;
        let ff = backend(ops.gelu(&ff))?;
        let ff = backend(ops.linear(&ff, &layer.fc2))?;
        hidden = backend(ops.add(&hidden, &ff))?;
        if dtype == DType::F16 {
            let limit = voice_tensor::f16_clip_limit();
            hidden = backend(ops.clip(&hidden, -limit, limit))?;
        }
    }
    Ok(hidden)
}

fn text_layer<B: TensorOps>(
    ops: &mut B,
    layer: &TextLayer<B::Tensor>,
    hidden: B::Tensor,
    positions: &B::Tensor,
    dtype: DType,
) -> Result<B::Tensor, EngineError<B::Error>> {
    let profile = QWEN3_ASR_0_6B;
    let normed = backend(ops.rms_norm(&hidden, &layer.input_norm))?;
    let mut query = project_heads(
        ops,
        &normed,
        &layer.q,
        profile.query_heads,
        profile.text_head_dim,
    )?;
    let mut key = project_heads(
        ops,
        &normed,
        &layer.k,
        profile.kv_heads,
        profile.text_head_dim,
    )?;
    let value = project_heads(
        ops,
        &normed,
        &layer.v,
        profile.kv_heads,
        profile.text_head_dim,
    )?;
    query = backend(ops.rms_norm(&query, &layer.q_norm))?;
    key = backend(ops.rms_norm(&key, &layer.k_norm))?;
    query = apply_rope(ops, query, positions)?;
    key = apply_rope(ops, key, positions)?;
    let seq = query.dims()[2];
    let mask = backend(ops.zeros(&[1, 1, seq, seq], dtype))?;
    let attended = backend(ops.attention(&query, &key, &value, Some(&mask)))?;
    let q_width = profile.query_heads * profile.text_head_dim;
    let attended = merge_heads(ops, attended, q_width)?;
    let attended = backend(ops.linear(&attended, &layer.out))?;
    let hidden = backend(ops.add(&hidden, &attended))?;
    let normed = backend(ops.rms_norm(&hidden, &layer.post_norm))?;
    let gate = backend(ops.linear(&normed, &layer.gate))?;
    let gate = backend(ops.silu(&gate))?;
    let up = backend(ops.linear(&normed, &layer.up))?;
    let mixed = backend(ops.mul(&gate, &up))?;
    let down = backend(ops.linear(&mixed, &layer.down))?;
    backend(ops.add(&hidden, &down))
}

fn mrope_positions<B: TensorOps>(
    ops: &mut B,
    seq: usize,
    dtype: DType,
) -> Result<B::Tensor, EngineError<B::Error>> {
    let axes = QWEN3_ASR_0_6B.rope_sections;
    let positions = backend(ops.zeros(&[1, axes, seq], dtype))?;
    let summed = backend(ops.sum(&positions, 1, false))?;
    backend(ops.reshape(&summed, &[1, 1, seq as isize, 1]))
}

fn apply_rope<B: TensorOps>(
    ops: &mut B,
    states: B::Tensor,
    positions: &B::Tensor,
) -> Result<B::Tensor, EngineError<B::Error>> {
    let half = QWEN3_ASR_0_6B.text_head_dim / 2;
    let freq = backend(ops.arange(0, half as i64, 1, states.dtype()))?;
    let freq = backend(ops.reshape(&freq, &[1, 1, 1, half as isize]))?;
    let angles = backend(ops.mul(positions, &freq))?;
    let sin = backend(ops.sin(&angles))?;
    let cos = backend(ops.cos(&angles))?;
    let sin = backend(ops.concatenate(&[&sin, &sin], 3))?;
    let cos = backend(ops.concatenate(&[&cos, &cos], 3))?;
    let turned = backend(ops.mul(&states, &sin))?;
    let kept = backend(ops.mul(&states, &cos))?;
    backend(ops.add(&kept, &turned))
}

fn project_heads<B: TensorOps>(
    ops: &mut B,
    hidden: &B::Tensor,
    layer: &Linear<B::Tensor>,
    heads: usize,
    head_dim: usize,
) -> Result<B::Tensor, EngineError<B::Error>> {
    let projected = backend(ops.linear(hidden, layer))?;
    let seq = projected.dims()[1];
    let shaped = backend(ops.reshape(
        &projected,
        &[1, seq as isize, heads as isize, head_dim as isize],
    ))?;
    backend(ops.transpose(&shaped, &[0, 2, 1, 3]))
}

fn merge_heads<B: TensorOps>(
    ops: &mut B,
    states: B::Tensor,
    width: usize,
) -> Result<B::Tensor, EngineError<B::Error>> {
    let seq = states.dims()[2];
    let shaped = backend(ops.transpose(&states, &[0, 2, 1, 3]))?;
    backend(ops.reshape(&shaped, &[1, seq as isize, width as isize]))
}

fn concat_time<B: TensorOps>(
    ops: &mut B,
    chunks: &[B::Tensor],
) -> Result<B::Tensor, EngineError<B::Error>> {
    if chunks.len() == 1 {
        return Ok(chunks[0].clone());
    }
    let refs: Vec<&B::Tensor> = chunks.iter().collect();
    backend(ops.concatenate(&refs, 1))
}

fn dense_linear<B: TensorOps>(
    ops: &mut B,
    input: usize,
    output: usize,
    bias: bool,
    dtype: DType,
) -> Result<Linear<B::Tensor>, EngineError<B::Error>> {
    let weight = backend(ops.zeros(&[output, input], dtype))?;
    let bias = if bias {
        Some(backend(ops.zeros(&[output], dtype))?)
    } else {
        None
    };
    graph(Linear::new(weight, bias))
}

fn dense_conv<B: TensorOps>(
    ops: &mut B,
    in_ch: usize,
    out_ch: usize,
    dtype: DType,
) -> Result<Conv2d<B::Tensor>, EngineError<B::Error>> {
    let weight = backend(ops.zeros(&[out_ch, 3, 3, in_ch], dtype))?;
    let bias = Some(backend(ops.zeros(&[out_ch], dtype))?);
    graph(Conv2d::new(weight, bias))
}

fn dense_layer_norm<B: TensorOps>(
    ops: &mut B,
    dim: usize,
    dtype: DType,
) -> Result<LayerNorm<B::Tensor>, EngineError<B::Error>> {
    let weight = backend(ops.zeros(&[dim], dtype))?;
    let bias = backend(ops.zeros(&[dim], dtype))?;
    graph(LayerNorm::new(weight, Some(bias), 1e-5))
}

fn dense_rms<B: TensorOps>(
    ops: &mut B,
    dim: usize,
    eps: f32,
    dtype: DType,
) -> Result<RmsNorm<B::Tensor>, EngineError<B::Error>> {
    let weight = backend(ops.zeros(&[dim], dtype))?;
    graph(RmsNorm::new(weight, eps))
}
