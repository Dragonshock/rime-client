//! 文本解码器：把提示 token 与音频特征送进 28 层，取最后一个位置的 logits。
//!
//! 权重全部来自 `weights.safetensors` 的 `model.*`：词嵌入与 `lm_head` 是同一张
//! 151936×1024 的表（`tie_word_embeddings`，表里没有 `lm_head.*`），4 位分组
//! 量化、组大小 64；28 层是 `RMSNorm(eps 1e-6) + GQA(16/8, head_dim 128) +
//! q_norm/k_norm + MRoPE + SwiGLU`。线性层不带偏置。
//!
//! 前向按参考 `mlx_qwen3_asr` 的 `TextDecoder`：
//!
//! 1. 词嵌入按 token 取行；音频占位（`151676`）处换成编码器末尾投影的行。
//! 2. MRoPE：三个轴的位置相同，都是 `0..len`；频率按 `[24, 20, 20]` 交错覆盖，
//!    `emb = [freqs | freqs]`，`cos/sin` 在 f32 上算完再转成激活 dtype。
//! 3. 因果遮罩是加法 mask：上三角填 `-1e9`，再按 f16 存（即 `-inf`）。
//! 4. 每层 `x += Attn(RMSNorm(x)); x += down(silu(gate(RMSNorm(x))) * up(...))`。
//! 5. 末尾 `RMSNorm` 后只把最后一个位置送进 `lm_head`，分块取 logits。
//!
//! 分块取 logits 的理由：常驻的是那张表的打包值、缩放与偏置三块（4 位、组 64），
//! 不拼 f16 稠密表。`lm_head` 就是这张表本身（`tie_word_embeddings`），逐块取
//! 打包行做量化矩阵乘再取 argmax：一次只在设备上多出一块打包行（`LM_HEAD_BLOCK`
//! 行约 4 MB），整张词表从不物化。行之间互不影响，所以分块算与整表乘给同一个
//! argmax。
//!
//! 28 层的七个线性层同样是分组权重，前向走 `TensorOps::linear` 里的量化矩阵乘：
//! 打包值、缩放、偏置三块直接进核，不先拼 f16 整表。`QuantPolicy` 可以单独关掉
//! 注意力或 MLP 那一组，只给精度归因用。

use voice_tensor::{
    AxisSlice, DType, KvCache, Linear, RmsNorm, TensorMeta, TensorOps, AUDIO_PAD, GROUP_SIZE,
    HIDDEN, KV_HEADS, MROPE_SECTION, QUERY_HEADS, RMS_EPS, ROPE_SECTIONS, ROPE_THETA,
    TEXT_GROUP_BITS, TEXT_HEAD_DIM, TEXT_LAYERS, VOCAB_SIZE,
};

use super::weights::SafeWeights;
use super::{MlxBackend, MlxError, MlxTensor};

/// `lm_head` 一次取多少行打包权重。只影响分成几块，不影响每个 logit 的算法。
///
/// 32768 行的打包值在 4 位下是 16 MB（缩放、偏置各 1 MB），词表分成 5 块，
/// 整张 151936 行从不一次取齐。块再小一些会让每块的设备往返变多：同一台机器上
/// 8192 行（19 块）每次定稿比这一档慢 0.013 s，因为 `lm_head` 一次定稿要跑三遍
/// （预填充一次、每步解码一次）。
pub const LM_HEAD_BLOCK: usize = 32768;

/// 文本塔的结束符 `<|endoftext|>`。
pub const EOS: u32 = 151_643;
/// 对话结束符 `<|im_end|>`。
pub const IM_END: u32 = 151_645;
/// 默认最多新生成多少个 token。够一段口述，也挡住跑飞。
pub const MAX_NEW_TOKENS: usize = 256;

/// 参考里因果遮罩上三角填的数，再按 f16 存。
const CAUSAL_MASK_FILL: f32 = -1e9;

/// 旋转位置编码的取法。`RotateHalf` 是 042 与官方一致的那条；
/// `SinOnly` 是「乘 sin 不旋转」的错误取法，只用于归因对照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RopeMode {
    /// `x * cos + rotate_half(x) * sin`。
    RotateHalf,
    /// `x * sin`。少了 rotate_half，用来证明对照能分辨这两种取法。
    SinOnly,
}

/// 哪些线性层走量化矩阵乘。默认两组都走。
///
/// 唯一的回退口子：哪一组换回「先反量化再乘」都应当是同一组数，只有出现偏差时
/// 用它定位到底哪一组带来的。产品路径用默认值，即两个都走。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuantPolicy {
    /// `self_attn` 的 q/k/v/o 四个投影。
    pub attention: bool,
    /// `mlp` 的 gate/up/down 三个投影。
    pub mlp: bool,
}

impl Default for QuantPolicy {
    fn default() -> Self {
        Self {
            attention: true,
            mlp: true,
        }
    }
}

impl QuantPolicy {
    /// 一组都不走：七个投影全部先还原再乘，即 057 的做法。
    pub const fn dequantized() -> Self {
        Self {
            attention: false,
            mlp: false,
        }
    }
}

/// 前向的两个可替换口径。默认值是参考的取法。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecoderOptions {
    /// 旋转位置编码的取法。
    pub rope: RopeMode,
    /// 是否对每个头做 q_norm/k_norm。参考做。
    pub qk_norm: bool,
}

impl Default for DecoderOptions {
    fn default() -> Self {
        Self {
            rope: RopeMode::RotateHalf,
            qk_norm: true,
        }
    }
}

/// 一次预填充的结果。
#[derive(Debug, Clone)]
pub struct PrefillLogits {
    /// 提示长度。
    pub tokens: usize,
    /// 词表大小。
    pub vocab: usize,
    /// 最后一个位置的 logits 前若干维（按请求的维数取）。
    pub head: Vec<f32>,
    /// 全词表 argmax 的 token id。
    pub argmax: usize,
    /// argmax 处的 logit。
    pub argmax_value: f32,
}

/// 贪心解码为什么停下来。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// 生成了 `<|endoftext|>`（151643）。
    EndOfSequence,
    /// 生成了 `<|im_end|>`（151645）。参考的提示以它结束对话。
    ImEnd,
    /// 到了新 token 上限，没有遇到停止符。
    MaxNewTokens,
}

/// 一次贪心解码的结果。`tokens` 含最后那个停止符，与参考资产
/// `greedy_tokens_3` 的口径一致（参考自带的 `generate_with_info` 会剥掉 EOS，
/// 资产保留原样）。
#[derive(Clone, Debug)]
pub struct Generation {
    /// 提示长度。
    pub prompt_tokens: usize,
    /// 新生成的 token id，含停止符。
    pub tokens: Vec<u32>,
    /// 停下来的原因。
    pub stopped_by: StopReason,
}

/// 文本解码器的权重。打开时就把 28 层、末尾归一化与词嵌入表全部读上设备并留在
/// 张量后端上；分组权重保持三块（打包值、缩放、偏置），不预先拼成 f16。
///
/// 常驻的代价是显存：28 层的打包值约 300 MB，缩放与偏置各约 19 MB；词嵌入表
/// （也就是 `lm_head`）151936×1024 的打包值约 78 MB，缩放与偏置各约 5 MB。057 的
/// 做法是就地拼出 f16 的 820 MB 与 311 MB；两条路径的每个元素都是同一组量化值，
/// 差别只在还原发生在什么时候。KV 缓存每层只有
/// `(1, 8, 位置数, 128)` 两片，几十个位置不到 10 MB。
pub struct TextDecoder {
    layers: Vec<Layer>,
    final_norm: RmsNorm<MlxTensor>,
    embed: MlxTensor,
    dtype: DType,
    policy: QuantPolicy,
}

impl TextDecoder {
    /// 打开 `model_dir/weights.safetensors`，一次读完全部权重。
    ///
    /// 文件句柄在本函数返回前关掉，方法之外没有任何东西能再读到那张文件。
    pub fn open(model_dir: &str, dtype: DType) -> Result<Self, MlxError> {
        Self::open_with(model_dir, dtype, QuantPolicy::default())
    }

    /// 按给定口径打开：哪一组线性层走量化矩阵乘。
    pub fn open_with(
        model_dir: &str,
        dtype: DType,
        policy: QuantPolicy,
    ) -> Result<Self, MlxError> {
        let mut weights = SafeWeights::open(model_dir)?;
        let mut layers = Vec::with_capacity(TEXT_LAYERS);
        for index in 0..TEXT_LAYERS {
            layers.push(Layer::load(&mut weights, index, dtype, policy)?);
        }
        let norm_weight = weights.dense_as("model.norm.weight", dtype)?;
        let final_norm =
            RmsNorm::new(norm_weight, RMS_EPS).map_err(|err| MlxError::new(err.to_string()))?;
        // 词嵌入表常驻的是打包值、缩放、偏置三块，不拼稠密表。
        let embed = weights.affine_table_packed(
            "model.embed_tokens",
            GROUP_SIZE,
            TEXT_GROUP_BITS,
            dtype,
        )?;
        Ok(Self {
            layers,
            final_norm,
            embed,
            dtype,
            policy,
        })
    }

    /// 层数。
    pub fn layer_count(&self) -> usize {
        TEXT_LAYERS
    }

    /// 打开时用的口径：哪一组线性层走量化矩阵乘。
    pub fn policy(&self) -> QuantPolicy {
        self.policy
    }

    /// 取词嵌入表（也就是 `lm_head`）的若干整行，行优先展平成 f32。
    ///
    /// 给对照用：同一行在不同取法下必须是同一组数——分组量化沿最后一维按 64
    /// 一组切开，行之间互不影响。这也是 `lm_head` 可以分块算的前提。
    pub fn embedding_rows(
        &mut self,
        ops: &mut MlxBackend,
        rows: &[usize],
    ) -> Result<Vec<f32>, MlxError> {
        let table = self.table_rows(ops, rows)?;
        ops.to_f32(&table)
    }

    /// 从常驻的词嵌入表里取若干整行，形状 `(行数, HIDDEN)`。
    fn table_rows(&self, ops: &mut MlxBackend, rows: &[usize]) -> Result<MlxTensor, MlxError> {
        if rows.iter().any(|row| *row >= VOCAB_SIZE) {
            return Err(MlxError::new("词嵌入行下标越界"));
        }
        ops.take_rows(&self.embed, rows)
    }

    /// 预填充一次，取最后一个位置的 logits 前 `head_dims` 维与全词表 argmax。
    ///
    /// `tokens` 是提示 token id；`audio` 是编码器末尾投影的行优先展平
    /// `(n_audio, 1024)`，按音频占位出现的顺序逐行注入。
    pub fn prefill(
        &mut self,
        ops: &mut MlxBackend,
        tokens: &[u32],
        audio: &[f32],
        head_dims: usize,
    ) -> Result<PrefillLogits, MlxError> {
        self.prefill_with(ops, tokens, audio, head_dims, &DecoderOptions::default())
    }

    /// 按给定口径预填充。
    pub fn prefill_with(
        &mut self,
        ops: &mut MlxBackend,
        tokens: &[u32],
        audio: &[f32],
        head_dims: usize,
        options: &DecoderOptions,
    ) -> Result<PrefillLogits, MlxError> {
        let seq = tokens.len();
        if seq == 0 {
            return Err(MlxError::new("提示是空的"));
        }
        if head_dims == 0 || head_dims > VOCAB_SIZE {
            return Err(MlxError::new(format!(
                "要取的 logits 维数 {head_dims} 不合法"
            )));
        }

        let seq = tokens.len();
        let hidden = self.embed_prompt(ops, tokens, audio)?;
        let (cos, sin) = self.rope_tables(ops, seq)?;
        let mask = causal_mask(ops, seq, self.dtype)?;
        let hidden = self.run_layers(ops, &hidden, &cos, &sin, Some(&mask), None, options)?;
        let last = self.final_norm(ops, &hidden, seq)?;
        let (head, argmax, argmax_value) = self.head_logits(ops, &last, head_dims)?;
        Ok(PrefillLogits {
            tokens: seq,
            vocab: VOCAB_SIZE,
            head,
            argmax,
            argmax_value,
        })
    }

    /// 贪心解码：预填充一次，之后每步只喂 1 个 token，键值走 KV 缓存。
    ///
    /// 停止条件与参考的 `generate` 一致：下一个 token 是 151643 或 151645 就
    /// 收工，那个停止符也记进返回值；没遇到停止符就到 `max_new_tokens` 为止。
    /// 位置从提示长度开始，单步没有因果遮罩（只有 1 个查询位置，缓存里的
    /// 前缀都可见），与参考 `decoder.py` 的 `L == 1` 分支一致。
    pub fn generate(
        &mut self,
        ops: &mut MlxBackend,
        prompt: &[u32],
        audio: &[f32],
        max_new_tokens: usize,
    ) -> Result<Generation, MlxError> {
        self.generate_with(ops, prompt, audio, max_new_tokens, &DecoderOptions::default())
    }

    /// 按给定口径贪心解码。
    pub fn generate_with(
        &mut self,
        ops: &mut MlxBackend,
        prompt: &[u32],
        audio: &[f32],
        max_new_tokens: usize,
        options: &DecoderOptions,
    ) -> Result<Generation, MlxError> {
        let seq = prompt.len();
        if seq == 0 {
            return Err(MlxError::new("提示是空的"));
        }
        if max_new_tokens == 0 {
            return Err(MlxError::new("新 token 上限是 0"));
        }

        let hidden = self.embed_prompt(ops, prompt, audio)?;
        let (cos, sin) = self.rope_tables(ops, seq)?;
        let mask = causal_mask(ops, seq, self.dtype)?;
        let mut cache: Vec<KvCache<MlxTensor>> =
            (0..TEXT_LAYERS).map(|_| KvCache::new()).collect();
        let hidden = self.run_layers(
            ops,
            &hidden,
            &cos,
            &sin,
            Some(&mask),
            Some(&mut cache),
            options,
        )?;
        let last = self.final_norm(ops, &hidden, seq)?;
        let (_, mut next, _) = self.head_logits(ops, &last, 1)?;

        let mut tokens: Vec<u32> = Vec::with_capacity(max_new_tokens);
        let mut stopped_by = StopReason::MaxNewTokens;
        loop {
            let token = u32::try_from(next)
                .map_err(|_| MlxError::new(format!("贪心下标 {next} 不是合法 token")))?;
            tokens.push(token);
            match token {
                EOS => {
                    stopped_by = StopReason::EndOfSequence;
                    break;
                }
                IM_END => {
                    stopped_by = StopReason::ImEnd;
                    break;
                }
                _ => {}
            }
            if tokens.len() >= max_new_tokens {
                break;
            }
            let position = seq + tokens.len() - 1;
            let hidden = self.embed_token(ops, token)?;
            let (cos, sin) = self.rope_tables_at(ops, &[position as f32])?;
            let hidden = self.run_layers(ops, &hidden, &cos, &sin, None, Some(&mut cache), options)?;
            let last = self.final_norm(ops, &hidden, 1)?;
            next = self.head_logits(ops, &last, 1)?.1;
        }
        Ok(Generation {
            prompt_tokens: seq,
            tokens,
            stopped_by,
        })
    }

    /// 从第 0 层跑到第 27 层。`cache` 给上时每层顺便写入键值。
    fn run_layers(
        &self,
        ops: &mut MlxBackend,
        hidden: &MlxTensor,
        cos: &MlxTensor,
        sin: &MlxTensor,
        mask: Option<&MlxTensor>,
        mut cache: Option<&mut [KvCache<MlxTensor>]>,
        options: &DecoderOptions,
    ) -> Result<MlxTensor, MlxError> {
        let mut hidden = hidden.clone();
        for index in 0..TEXT_LAYERS {
            let layer = &self.layers[index];
            let slot = match cache.as_mut() {
                Some(slots) => Some(&mut slots[index]),
                None => None,
            };
            hidden = decode_layer(ops, layer, &hidden, cos, sin, mask, slot, options)?;
        }
        Ok(hidden)
    }

    /// 末尾 RMSNorm 之后只留最后一个位置，形状 `(1, 1, 1024)`。
    fn final_norm(
        &self,
        ops: &mut MlxBackend,
        hidden: &MlxTensor,
        seq: usize,
    ) -> Result<MlxTensor, MlxError> {
        let hidden = ops.rms_norm(hidden, &self.final_norm)?;
        ops.slice(
            &hidden,
            &[
                AxisSlice::new(0, 1),
                AxisSlice::new(seq - 1, seq),
                AxisSlice::full(HIDDEN),
            ],
        )
    }

    /// 单步解码的输入嵌入：按 token id 取词嵌入表的一行。
    fn embed_token(&self, ops: &mut MlxBackend, token: u32) -> Result<MlxTensor, MlxError> {
        if token as usize >= VOCAB_SIZE {
            return Err(MlxError::new(format!("token id {token} 超出词表")));
        }
        let table = self.table_rows(ops, &[token as usize])?;
        let rows = ops.to_f32(&table)?;
        if rows.len() != HIDDEN {
            return Err(MlxError::new(format!(
                "词嵌入一行 {} 个元素，应为 {HIDDEN}",
                rows.len()
            )));
        }
        ops.literal(&rows, &[1, 1, HIDDEN], self.dtype)
    }

    /// 组装提示的输入嵌入 `(1, seq, 1024)`。
    ///
    /// 音频占位处的行来自 `audio`；其余位置按 token id 从词嵌入表取行。
    /// 参考是先取词嵌入再用 `mx.where` 换成音频特征，占位处的词嵌入行用不到，
    /// 所以这里不读那 26 行。
    fn embed_prompt(
        &self,
        ops: &mut MlxBackend,
        tokens: &[u32],
        audio: &[f32],
    ) -> Result<MlxTensor, MlxError> {
        let seq = tokens.len();
        if !audio.len().is_multiple_of(HIDDEN) {
            return Err(MlxError::new(format!(
                "音频特征长度 {} 不是 {HIDDEN} 的倍数",
                audio.len()
            )));
        }
        let audio_rows = audio.len() / HIDDEN;
        let placeholders = tokens.iter().filter(|id| **id == AUDIO_PAD).count();
        if placeholders != audio_rows {
            return Err(MlxError::new(format!(
                "音频占位 {placeholders} 个，音频特征 {audio_rows} 行，对不上"
            )));
        }
        for id in tokens {
            if *id as usize >= VOCAB_SIZE {
                return Err(MlxError::new(format!("token id {id} 超出词表")));
            }
        }

        let mut wanted: Vec<usize> = Vec::new();
        for id in tokens {
            if *id != AUDIO_PAD && !wanted.contains(&(*id as usize)) {
                wanted.push(*id as usize);
            }
        }
        let table = self.table_rows(ops, &wanted)?;
        let rows = ops.to_f32(&table)?;

        let mut embeds = vec![0.0f32; seq * HIDDEN];
        let mut used = 0usize;
        for (position, id) in tokens.iter().enumerate() {
            let start = position * HIDDEN;
            if *id == AUDIO_PAD {
                let from = used * HIDDEN;
                embeds[start..start + HIDDEN].copy_from_slice(&audio[from..from + HIDDEN]);
                used += 1;
                continue;
            }
            let row = wanted
                .iter()
                .position(|other| *other == *id as usize)
                .ok_or_else(|| MlxError::new("词嵌入行没取到"))?;
            let from = row * HIDDEN;
            embeds[start..start + HIDDEN].copy_from_slice(&rows[from..from + HIDDEN]);
        }
        ops.literal(&embeds, &[1, seq, HIDDEN], self.dtype)
    }

    /// MRoPE 的 `cos/sin`，形状 `(1, seq, head_dim)`。
    fn rope_tables(
        &self,
        ops: &mut MlxBackend,
        seq: usize,
    ) -> Result<(MlxTensor, MlxTensor), MlxError> {
        let positions: Vec<f32> = (0..seq).map(|step| step as f32).collect();
        self.rope_tables_at(ops, &positions)
    }

    /// 给定位置的 MRoPE `cos/sin`，形状 `(1, 1, 位置数, head_dim)`。
    ///
    /// 预填充传 `0..seq`，单步解码只传当前位置。位置轴不是从 0 重数：缓存里
    /// 已经有 `seq` 个位置，新 token 的位置是 `seq`、`seq + 1`……
    fn rope_tables_at(
        &self,
        ops: &mut MlxBackend,
        positions: &[f32],
    ) -> Result<(MlxTensor, MlxTensor), MlxError> {
        let seq = positions.len();
        let (cos, sin) = mrope_cos_sin([positions, positions, positions]);
        let cos = ops.literal(&cos, &[1, seq, TEXT_HEAD_DIM], self.dtype)?;
        let sin = ops.literal(&sin, &[1, seq, TEXT_HEAD_DIM], self.dtype)?;
        // (1, seq, head_dim) → (1, 1, seq, head_dim)，按头广播。
        let cos = ops.reshape(&cos, &[1, 1, seq as isize, TEXT_HEAD_DIM as isize])?;
        let sin = ops.reshape(&sin, &[1, 1, seq as isize, TEXT_HEAD_DIM as isize])?;
        Ok((cos, sin))
    }

    /// 把最后一个位置的隐层送进 `lm_head`（即词嵌入表），按块取 argmax。
    ///
    /// 每块只把打包权重、缩放、偏置的三段行取上设备，量化矩阵乘给出这一块的
    /// logits，取完 argmax 就丢掉。整张 151936×1024 的表（以及它的 f16 稠密
    /// 折叠）从不产生，一次驻留的只有 `LM_HEAD_BLOCK` 行。
    fn head_logits(
        &self,
        ops: &mut MlxBackend,
        last: &MlxTensor,
        head_dims: usize,
    ) -> Result<(Vec<f32>, usize, f32), MlxError> {
        let mut head = vec![0.0f32; head_dims];
        let mut best = (0usize, f32::NEG_INFINITY);
        let mut start = 0usize;
        while start < VOCAB_SIZE {
            let count = LM_HEAD_BLOCK.min(VOCAB_SIZE - start);
            let rows: Vec<usize> = (start..start + count).collect();
            let table = self.table_rows(ops, &rows)?;
            let layer = Linear::new(table, None).map_err(|err| MlxError::new(err.to_string()))?;
            let logits = ops.linear(last, &layer)?;
            let values = ops.to_f32(&logits)?;
            for (offset, value) in values.iter().enumerate() {
                if *value > best.1 {
                    best = (start + offset, *value);
                }
            }
            if start == 0 {
                if values.len() < head_dims {
                    return Err(MlxError::new("第一块 logits 比要取的维数还短"));
                }
                head.copy_from_slice(&values[..head_dims]);
            }
            start += count;
        }
        Ok((head, best.0, best.1))
    }
}

/// 一层解码器的权重。线性层是 4 位分组权重，两个 RMSNorm 是稠密 f16。
struct Layer {
    input_norm: RmsNorm<MlxTensor>,
    q: Linear<MlxTensor>,
    k: Linear<MlxTensor>,
    v: Linear<MlxTensor>,
    o: Linear<MlxTensor>,
    q_norm: RmsNorm<MlxTensor>,
    k_norm: RmsNorm<MlxTensor>,
    post_norm: RmsNorm<MlxTensor>,
    gate: Linear<MlxTensor>,
    up: Linear<MlxTensor>,
    down: Linear<MlxTensor>,
}

impl Layer {
    fn load(
        weights: &mut SafeWeights,
        index: usize,
        dtype: DType,
        policy: QuantPolicy,
    ) -> Result<Self, MlxError> {
        let at = |suffix: &str| format!("model.layers.{index}.{suffix}");
        // `weight` 走量化矩阵乘（三块常驻），否则走 057 的路（先拼 f16 整表）。
        // 两者用的是文件里同一组量化值，差别只在还原发生在什么时候。
        let affine = |weights: &mut SafeWeights,
                      suffix: &str,
                      quantized: bool|
         -> Result<Linear<MlxTensor>, MlxError> {
            if quantized {
                weights.affine_packed(&at(suffix), GROUP_SIZE, TEXT_GROUP_BITS, dtype)
            } else {
                weights.affine_as(&at(suffix), GROUP_SIZE, TEXT_GROUP_BITS, dtype)
            }
        };
        let norm =
            |weights: &mut SafeWeights, suffix: &str| -> Result<RmsNorm<MlxTensor>, MlxError> {
                let weight = weights.dense_as(&at(&format!("{suffix}.weight")), dtype)?;
                RmsNorm::new(weight, RMS_EPS).map_err(|err| MlxError::new(err.to_string()))
            };
        Ok(Self {
            input_norm: norm(weights, "input_layernorm")?,
            q: affine(weights, "self_attn.q_proj", policy.attention)?,
            k: affine(weights, "self_attn.k_proj", policy.attention)?,
            v: affine(weights, "self_attn.v_proj", policy.attention)?,
            o: affine(weights, "self_attn.o_proj", policy.attention)?,
            q_norm: norm(weights, "self_attn.q_norm")?,
            k_norm: norm(weights, "self_attn.k_norm")?,
            post_norm: norm(weights, "post_attention_layernorm")?,
            gate: affine(weights, "mlp.gate_proj", policy.mlp)?,
            up: affine(weights, "mlp.up_proj", policy.mlp)?,
            down: affine(weights, "mlp.down_proj", policy.mlp)?,
        })
    }
}

fn decode_layer(
    ops: &mut MlxBackend,
    layer: &Layer,
    hidden: &MlxTensor,
    cos: &MlxTensor,
    sin: &MlxTensor,
    mask: Option<&MlxTensor>,
    cache: Option<&mut KvCache<MlxTensor>>,
    options: &DecoderOptions,
) -> Result<MlxTensor, MlxError> {
    let seq = hidden.dims()[1];
    let normed = ops.rms_norm(hidden, &layer.input_norm)?;
    let attended = self_attention(ops, &normed, layer, seq, cos, sin, mask, cache, options)?;
    let mut x = ops.add(hidden, &attended)?;

    let normed = ops.rms_norm(&x, &layer.post_norm)?;
    let gate = ops.linear(&normed, &layer.gate)?;
    let gate = ops.silu(&gate)?;
    let up = ops.linear(&normed, &layer.up)?;
    let gated = ops.mul(&gate, &up)?;
    let feed = ops.linear(&gated, &layer.down)?;
    x = ops.add(&x, &feed)?;
    Ok(x)
}

/// 一层自注意力。`cache` 给上时先写入新的键值，再用整段缓存做注意力。
#[allow(clippy::too_many_arguments)]
fn self_attention(
    ops: &mut MlxBackend,
    normed: &MlxTensor,
    layer: &Layer,
    seq: usize,
    cos: &MlxTensor,
    sin: &MlxTensor,
    mask: Option<&MlxTensor>,
    cache: Option<&mut KvCache<MlxTensor>>,
    options: &DecoderOptions,
) -> Result<MlxTensor, MlxError> {
    let query = heads(ops, normed, &layer.q, QUERY_HEADS, seq)?;
    let key = heads(ops, normed, &layer.k, KV_HEADS, seq)?;
    let value = heads(ops, normed, &layer.v, KV_HEADS, seq)?;
    // 参考的顺序：投影 → 按 (B, L, 头, 头维) reshape → 对最后一维归一化（q/k）→ 转置。
    let query = if options.qk_norm {
        ops.rms_norm(&query, &layer.q_norm)?
    } else {
        query
    };
    let key = if options.qk_norm {
        ops.rms_norm(&key, &layer.k_norm)?
    } else {
        key
    };
    let query = to_heads(ops, &query)?;
    let key = to_heads(ops, &key)?;
    let value = to_heads(ops, &value)?;
    let (query, key) = rope(ops, &query, &key, cos, sin, options.rope)?;
    let (key, value) = match cache {
        Some(cache) => {
            ops.kv_write(cache, &key, &value)?;
            ops.kv_read(cache)?
        }
        None => (key, value),
    };
    let attended = ops.attention(&query, &key, &value, mask)?;
    let merged = merge_heads(ops, &attended, QUERY_HEADS * TEXT_HEAD_DIM)?;
    ops.linear(&merged, &layer.o)
}

/// 投影后按 `(B, L, 头数, 头维)` reshape。头维与归一化口径是参考的顺序。
fn heads(
    ops: &mut MlxBackend,
    hidden: &MlxTensor,
    layer: &Linear<MlxTensor>,
    count: usize,
    seq: usize,
) -> Result<MlxTensor, MlxError> {
    let projected = ops.linear(hidden, layer)?;
    ops.reshape(
        &projected,
        &[1, seq as isize, count as isize, TEXT_HEAD_DIM as isize],
    )
}

/// `(B, L, 头, 头维)` → `(B, 头, L, 头维)`，给注意力的形状。
fn to_heads(ops: &mut MlxBackend, shaped: &MlxTensor) -> Result<MlxTensor, MlxError> {
    ops.transpose(shaped, &[0, 2, 1, 3])
}

fn mrope_cos_sin(positions: [&[f32]; ROPE_SECTIONS]) -> (Vec<f32>, Vec<f32>) {
    let half = TEXT_HEAD_DIM / 2;
    let seq = positions[0].len();
    let inv_freq: Vec<f32> = (0..half)
        .map(|index| 1.0f32 / ROPE_THETA.powf((2 * index) as f32 / TEXT_HEAD_DIM as f32))
        .collect();
    let mut owner = [0usize; TEXT_HEAD_DIM / 2];
    for dim in 1..ROPE_SECTIONS {
        let stop = (MROPE_SECTION[dim] * ROPE_SECTIONS).min(half);
        let mut index = dim;
        while index < stop {
            owner[index] = dim;
            index += ROPE_SECTIONS;
        }
    }
    let mut cos = vec![0.0f32; seq * TEXT_HEAD_DIM];
    let mut sin = vec![0.0f32; seq * TEXT_HEAD_DIM];
    for step in 0..seq {
        for index in 0..half {
            let angle = positions[owner[index]][step] * inv_freq[index];
            let (sine, cosine) = (angle.sin(), angle.cos());
            let at = step * TEXT_HEAD_DIM + index;
            cos[at] = cosine;
            sin[at] = sine;
            cos[at + half] = cosine;
            sin[at + half] = sine;
        }
    }
    (cos, sin)
}

fn rope(
    ops: &mut MlxBackend,
    query: &MlxTensor,
    key: &MlxTensor,
    cos: &MlxTensor,
    sin: &MlxTensor,
    mode: RopeMode,
) -> Result<(MlxTensor, MlxTensor), MlxError> {
    let minus_one = ops.literal(&[-1.0], &[1], query.dtype())?;
    let apply = |ops: &mut MlxBackend, x: &MlxTensor| -> Result<MlxTensor, MlxError> {
        match mode {
            RopeMode::RotateHalf => {
                let turned = rotate_half(ops, x, &minus_one)?;
                let turned = ops.mul(&turned, sin)?;
                let scaled = ops.mul(x, cos)?;
                ops.add(&scaled, &turned)
            }
            RopeMode::SinOnly => ops.mul(x, sin),
        }
    };
    Ok((apply(ops, query)?, apply(ops, key)?))
}

/// `[-x2, x1]`：最后一维对半切开，前半取负的后半，后半取前半。
fn rotate_half(
    ops: &mut MlxBackend,
    x: &MlxTensor,
    minus_one: &MlxTensor,
) -> Result<MlxTensor, MlxError> {
    let dims = x.dims().to_vec();
    let head = *dims
        .last()
        .ok_or_else(|| MlxError::new("rotate_half 没有维度"))?;
    let mid = head / 2;
    let prefix: Vec<AxisSlice> = dims[..dims.len() - 1]
        .iter()
        .map(|dim| AxisSlice::full(*dim))
        .collect();
    let mut first = prefix.clone();
    first.push(AxisSlice::new(0, mid));
    let mut second = prefix;
    second.push(AxisSlice::new(mid, head));
    let first = ops.slice(x, &first)?;
    let second = ops.slice(x, &second)?;
    let negated = ops.mul(&second, minus_one)?;
    ops.concatenate(&[&negated, &first], dims.len() - 1)
}

/// `(B, 头, L, 头维)` → `(B, L, 头 * 头维)`。
fn merge_heads(
    ops: &mut MlxBackend,
    states: &MlxTensor,
    width: usize,
) -> Result<MlxTensor, MlxError> {
    let seq = states.dims().get(2).copied().unwrap_or(0);
    let ordered = ops.transpose(states, &[0, 2, 1, 3])?;
    ops.reshape(&ordered, &[1, seq as isize, width as isize])
}

/// 加法因果遮罩 `(1, 1, L, L)`：上三角 `-1e9`，其余 0，按激活 dtype 存。
fn causal_mask(ops: &mut MlxBackend, seq: usize, dtype: DType) -> Result<MlxTensor, MlxError> {
    let mut mask = vec![0.0f32; seq * seq];
    for row in 0..seq {
        for column in (row + 1)..seq {
            mask[row * seq + column] = CAUSAL_MASK_FILL;
        }
    }
    ops.literal(&mask, &[1, 1, seq, seq], dtype)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mrope_angles_follow_the_frequency_owner() {
        // 三个轴位置相同时，交错覆盖不改变数值：cos/sin 两半相同。
        let positions = [0.0f32, 1.0, 2.0];
        let (cos, sin) = mrope_cos_sin([&positions, &positions, &positions]);
        assert_eq!(cos.len(), 3 * TEXT_HEAD_DIM);
        assert_eq!(sin.len(), 3 * TEXT_HEAD_DIM);
        let half = TEXT_HEAD_DIM / 2;
        for step in 0..3 {
            let base = step * TEXT_HEAD_DIM;
            for index in 0..half {
                assert_eq!(cos[base + index], cos[base + index + half]);
                assert_eq!(sin[base + index], sin[base + index + half]);
            }
        }
        // 第 0 个位置角度全是 0。
        assert_eq!(cos[0], 1.0);
        assert_eq!(sin[0], 0.0);
        // 位置 1、频率 0 的角度是 1（theta^0），位置 2 是 2。
        let one = 1.0f32 * 1.0f32;
        assert!((cos[TEXT_HEAD_DIM] - one.cos()).abs() < 1e-6);
        assert!((sin[TEXT_HEAD_DIM] - one.sin()).abs() < 1e-6);
        let two = 2.0f32;
        assert!((cos[2 * TEXT_HEAD_DIM] - two.cos()).abs() < 1e-6);
        assert!((sin[2 * TEXT_HEAD_DIM] - two.sin()).abs() < 1e-6);
    }
}
