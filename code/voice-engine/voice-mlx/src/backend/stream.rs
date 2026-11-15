//! 说话中出字：已结束的 8 秒窗留在 `Stream` 里，每步只编码活动窗，文本只重算未固定的尾巴。
//!
//! 042 第 5 节的表在这里落地。一步做五件事：
//!
//! 1. 把这一步的新样本接进增量梅尔（[`voice_mel::MelStream`]），只对新样本算梅尔。
//!    已定的帧留在缓存里，末尾那一帧按新的右端反射重算。整形（先减「全局最大值减 8」）
//!    推迟到取值时，按当前前缀的 max 套用：这是参考实现里唯一的非因果依赖，也是
//!    「不能整段重算」与「必须整段一起整形」之间那道缝，缝的宽度是一帧。
//! 2. 满 800 个梅尔帧就编一段并接进 `prefix`（已结束的窗）。窗边界与一次性编码
//!    那条路完全一致：窗宽 800 帧是块长 100 帧的 8 倍，位置表本来每块从 0 起算，
//!    窗与窗之间互不看，所以切开逐窗编与整段编给出同一组数。
//! 3. 活动窗（不足 800 帧的那段）每步重编。它是唯一会随新样本改写的一段。
//! 4. 解码：提示是模板加「已固定的文本」，只有未固定的尾巴（默认 5 个 token）重算。
//! 5. 键值：提示头与已结束各窗的行只在第一次算一次，之后每步把它们之前那次算出的
//!    键值留着，只对「活动窗的行 + 固定尾巴 + 已固定文本」这一段算注意力。理由见
//!    [`Stream`] 上的注释与 `notes/069`。
//!
//! 音频占位个数每步都变，`<|audio_pad|>` 之后的位置整体平移，所以缓存的复用不是
//! 「整条提示照搬」，而是**只照搬仍然是前缀的那一段**：位置 `0..valid_prefix`
//! 的内容不再变，后面的每步截掉重算。截断与续写都按绝对位置对齐，拼接就是
//! `kv_write` 那一次沿时间轴的 `concatenate`，后端不需要支持「两段键值」的新形状。

use std::time::Instant;

use voice_mel::MelStream;
use voice_tensor::{
    AxisSlice, DType, KvCache, TensorOps, HIDDEN, N_WINDOW_INFER, TEXT_LAYERS,
};

use super::text_decoder::{DecoderOptions, StopReason, TextDecoder, EOS, IM_END};
use super::transcribe::{build_prompt, prompt_parts, splice_prompt};
use super::{encode_mel, AudioTower, MlxBackend, MlxError, MlxTensor};
use voice_tokenizer::Tokenizer;

/// 一次非定稿步的结果。数值都是这一步的，不是累计的。
#[derive(Clone, Debug)]
pub struct StreamStep {
    /// 这一步开始时累积的梅尔帧数（整段）。
    pub mel_frames: usize,
    /// 音频 token 总数（前缀加活动窗）。
    pub audio_tokens: usize,
    /// 已结束各窗的 token 数，换到下一步仍然有效。
    pub encoder_prefix_tokens: usize,
    /// 这一步新冻结进前缀的 token 数。
    pub frozen_tokens: usize,
    /// 这一步重编的活动窗帧数。
    pub active_frames: usize,
    /// 提示长度（含音频占位与已固定文本）。
    pub prompt_tokens: usize,
    /// 这一步真的送进解码器的位置数（提示长度减去复用的那一截）。
    pub prefill_tokens: usize,
    /// 这一步直接沿用上一步键值的位置数（提示头加已结束各窗的行）。
    pub reused_tokens: usize,
    /// 写进提示的已固定 token 数。
    pub stable_tokens: usize,
    /// 这一步新生成的 token 数（含停止符）。
    pub new_tokens: usize,
    /// 整段假设，含停止符。
    pub tokens: Vec<u32>,
    /// 去掉特殊 token 字面量之后的整段文本。
    pub text: String,
    /// 贪心为什么停下来。
    pub stopped_by: Option<StopReason>,
    /// 这一步新样本的梅尔墙钟（DFT 加滤波器组），只算新样本。
    pub mel_seconds: f64,
    /// 这一步把要编码的那几列整形出来的墙钟（只算这几列，与历史长度无关）。
    pub mel_shape_seconds: f64,
    /// 这一步编码的墙钟（冻结窗加活动窗）。
    pub encode_seconds: f64,
    /// 这一步预填充的墙钟：截掉缓存里失效的那一截，加上重算 `prefill_tokens` 个位置。
    pub prefill_seconds: f64,
    /// 预填充加贪心的墙钟。
    pub decode_seconds: f64,
}

/// 一次按住里的流式状态。只带逐次变化的东西，权重仍在常驻的 [`Engine`] 里。
///
/// 这里的 `prefix` 是 1024 维投影输出，与编码器窗一起留到下一步用。
///
/// 解码器的键值也跨步留，但只留**仍然是前缀**的那一段：提示头（`head_tokens`
/// 个位置）加已结束各窗的行。这一步的活动窗换了内容，它后面的位置整体跟着变，
/// 所以缓存在这一步开头先截到 `valid_prefix`，再把这一段重算一遍接上去。
/// 截断的位置与续写的位置都是绝对位置，两者之间正好是一次 `concatenate`。
///
/// 为什么这样还等于整段一次预填充：注意力的因果遮罩让位置 `p` 的键值只取决于
/// `0..=p` 的输入。提示头与已结束各窗的行前面没有别的东西，它们的位置每步都一样，
/// 内容每步也一样（窗一旦满 800 帧就只编一次），所以重算只会得到同一组数。
/// 反过来，活动窗的行每一帧的编码结果都随窗宽变，它们和它们后面的固定尾巴、
/// 已固定文本都必须每步重算——这一段的长度与历史无关，是「一窗 + 十几个 token」。
///
/// 有一个时刻要老实交代：某一步刚好凑满 800 帧、窗从活动窗变成已结束窗时，
/// 那片行在缓存里还是上一步按活动窗编出来的旧内容（例如 600 帧编出来的 78 行），
/// 与整窗编出来的 104 行不同。所以那一步 `valid_prefix` 取「上一步就有效的那一段」
/// 与「这一步的前缀边界」的较小值，把整窗重算一遍再接上。
///
/// 梅尔也是跨步复用的：帧网格固定，已定帧的 400 个样本不会再变，所以它们的原始值
/// 留在 [`MelStream`] 里，每步只对新样本做 DFT 与滤波器组。整形要的那个「全局最大
/// 值减 8」按当前前缀增量维护，取值时套用。
///
/// 固定窗宽占位（把活动窗补到 800 帧、让位置从此不动）这条没有选：见 `notes/069`
/// 第 2 节，补出来的行要么白算一遍前向，要么依赖一个只会越补越多的预算，反而比
/// 「截到仍然是前缀的地方」贵。
///
/// [`Engine`]: super::Engine
pub struct Stream {
    /// 还没进梅尔的新样本。`push` 只入账，DFT 在下一步里做（那一步才量得到墙钟）。
    pending: Vec<i16>,
    /// 增量梅尔：已定帧的整形前原始值、未定尾帧、整形用的前缀最大值。
    mel: MelStream,
    /// 已结束各窗的投影输出，行优先 `(token, 1024)`。
    prefix: Vec<f32>,
    /// 已经进 `prefix` 的梅尔帧数。
    encoded_frames: usize,
    /// 上一步的整段假设，含停止符。
    tokens: Vec<u32>,
    /// 允许下一步改写的尾巴长度。
    unfixed: usize,
    /// 解码器 28 层的键值缓存，位置 `[0, cache_tokens)`。
    cache: Vec<KvCache<MlxTensor>>,
    /// 缓存里现有的位置数。
    cache_tokens: usize,
    /// 缓存开头内容仍然有效的长度：提示头加上一步就已结束各窗的行。
    valid_prefix: usize,
    /// 上一步提示头的 token 数。上下文或语言换了它就会变，那时位置对不上，不复用。
    head_tokens: usize,
}

impl Stream {
    /// `unfixed` 是最近多少个 token 允许被改写，取 042 第 5 节表的 5。
    pub fn new(unfixed: usize) -> Self {
        Self {
            pending: Vec::new(),
            mel: MelStream::new(),
            prefix: Vec::new(),
            encoded_frames: 0,
            tokens: Vec::new(),
            unfixed,
            cache: (0..TEXT_LAYERS).map(|_| KvCache::new()).collect(),
            cache_tokens: 0,
            valid_prefix: 0,
            head_tokens: 0,
        }
    }

    /// 接一段 16 kHz 单声道样本。只入账，不碰张量，也不算梅尔：
    /// 梅尔在下一步里只对这一段的样本算，代价与已收音频长度无关。
    pub fn push(&mut self, pcm: &[i16]) {
        self.pending.extend_from_slice(pcm);
    }

    /// 收进来的样本数（含还没进梅尔的那一段）。
    pub fn samples(&self) -> usize {
        self.mel.samples() + self.pending.len()
    }

    /// 缓存里现有的位置数。给测试与日志用。
    pub fn cached_tokens(&self) -> usize {
        self.cache_tokens
    }

    // 状态只经 `StreamStep` 交出去：外面只能 `push` 与走一步，读不到内部张量。
}

/// 已经打开的实例的借用。与 [`Opened`] 同形，只是多了一条流式状态。
///
/// [`Opened`]: super::Opened
pub struct StreamOpened<'a> {
    pub mel: &'a voice_mel::MelFrontend,
    pub tower: &'a AudioTower,
    pub decoder: &'a mut TextDecoder,
    pub tokenizer: &'a Tokenizer,
    pub dtype: DType,
}

/// 跑一个非定稿步。[`Engine::stream_step`] 走的就是这里。
///
/// [`Engine::stream_step`]: super::Engine::stream_step
pub fn stream_step_with_opened(
    ops: &mut MlxBackend,
    opened: StreamOpened<'_>,
    stream: &mut Stream,
    max_new_tokens: usize,
    language: Option<&str>,
    context: &str,
) -> Result<StreamStep, MlxError> {
    // --- 只对新样本求梅尔 ---
    let clock = Instant::now();
    let fresh = std::mem::take(&mut stream.pending);
    stream.mel.push_i16(opened.mel, &fresh);
    let mel_seconds = clock.elapsed().as_secs_f64();
    stream.pending = fresh;
    stream.pending.clear();
    let mel_frames = stream.mel.frames();
    if mel_frames == 0 {
        return Err(MlxError::new("还没有凑出一个梅尔帧"));
    }

    // --- 已结束的窗：满 800 帧就编一次，接进前缀 ---
    let mut encode_seconds = 0.0f64;
    let mut mel_shape_seconds = 0.0f64;
    let mut frozen_tokens = 0usize;
    while mel_frames - stream.encoded_frames >= N_WINDOW_INFER {
        let start = stream.encoded_frames;
        let end = start + N_WINDOW_INFER;
        let clock = Instant::now();
        let patch = stream.mel.shaped_columns(start, end);
        mel_shape_seconds += clock.elapsed().as_secs_f64();
        let clock = Instant::now();
        let encoded = encode_mel(ops, opened.tower, &patch, N_WINDOW_INFER, opened.dtype)?;
        encode_seconds += clock.elapsed().as_secs_f64();
        check_rows(encoded.proj.len(), encoded.tokens)?;
        stream.prefix.extend_from_slice(&encoded.proj);
        stream.encoded_frames = end;
        frozen_tokens += encoded.tokens;
    }

    // --- 活动窗：每步重编 ---
    let active_frames = mel_frames - stream.encoded_frames;
    let mut active_rows: Vec<f32> = Vec::new();
    if active_frames > 0 {
        let clock = Instant::now();
        let patch = stream.mel.shaped_columns(stream.encoded_frames, mel_frames);
        mel_shape_seconds += clock.elapsed().as_secs_f64();
        let clock = Instant::now();
        let encoded = encode_mel(ops, opened.tower, &patch, active_frames, opened.dtype)?;
        encode_seconds += clock.elapsed().as_secs_f64();
        check_rows(encoded.proj.len(), encoded.tokens)?;
        active_rows = encoded.proj;
    }

    let mut audio: Vec<f32> = Vec::with_capacity(stream.prefix.len() + active_rows.len());
    audio.extend_from_slice(&stream.prefix);
    audio.extend_from_slice(&active_rows);
    let audio_tokens = audio.len() / HIDDEN;

    // --- 拼提示：提示头 + 音频占位 + 固定尾巴 + 已固定文本 ---
    let stable = stable_tokens(&stream.tokens, stream.unfixed).to_vec();
    let parts = prompt_parts(opened.tokenizer, language, context);
    let head_tokens = parts.head.len();
    let mut prompt = splice_prompt(&parts, audio_tokens);
    prompt.extend_from_slice(&stable);

    // 复用键值的前提是「按 id 拼出来的提示」与「整段分词得到的提示」是同一串，
    // 而且提示头的位置与上一步一样长。任何一条不成立就整条重算，退回老路。
    let spliced = {
        let mut whole = build_prompt(opened.tokenizer, audio_tokens, language, context);
        whole.extend_from_slice(&stable);
        whole == prompt
    };
    let prefix_rows = stream.prefix.len() / HIDDEN;
    let reusable = if spliced && head_tokens == stream.head_tokens {
        stream
            .valid_prefix
            .min(head_tokens + prefix_rows)
            .min(stream.cache_tokens)
    } else {
        0
    };
    // --- 预填充：把缓存截到仍然有效的地方，再只算之后的那一段 ---
    let clock = Instant::now();
    if reusable < stream.cache_tokens {
        if reusable == 0 {
            stream.cache = (0..TEXT_LAYERS).map(|_| KvCache::new()).collect();
        } else {
            truncate_cache(ops, &mut stream.cache, reusable)?;
        }
        stream.cache_tokens = reusable;
    }
    let start = stream.cache_tokens;
    let audio_from = start.saturating_sub(head_tokens);
    let logits = opened.decoder.prefill_chunk(
        ops,
        &mut stream.cache,
        &prompt[start..],
        &audio[audio_from * HIDDEN..],
        start,
        1,
        &DecoderOptions::default(),
    )?;
    let next = logits.argmax;
    let prefill_seconds = clock.elapsed().as_secs_f64();

    // --- 增量解码：只有未固定的尾巴重算 ---
    let (generation, cache_after) = opened.decoder.greedy_from_cache(
        ops,
        &mut stream.cache,
        prompt.len(),
        next,
        max_new_tokens,
        &DecoderOptions::default(),
    )?;
    let decode_seconds = clock.elapsed().as_secs_f64();

    // 这一步把 `0..head + prefix_rows` 重算过了（起点不晚于这个边界），所以
    // 从下一步开始这一段可以整段留着。
    stream.cache_tokens = cache_after;
    stream.valid_prefix = head_tokens + prefix_rows;
    stream.head_tokens = head_tokens;

    let mut tokens = stable.clone();
    tokens.extend_from_slice(&generation.tokens);
    let text = opened.tokenizer.decode_without_special(&tokens);
    stream.tokens = tokens.clone();

    Ok(StreamStep {
        mel_frames,
        audio_tokens,
        encoder_prefix_tokens: prefix_rows,
        frozen_tokens,
        active_frames,
        prompt_tokens: prompt.len(),
        prefill_tokens: prompt.len() - start,
        reused_tokens: start,
        stable_tokens: stable.len(),
        new_tokens: generation.tokens.len(),
        tokens,
        text,
        stopped_by: Some(generation.stopped_by),
        mel_seconds,
        mel_shape_seconds,
        encode_seconds,
        prefill_seconds,
        decode_seconds,
    })
}

/// 把 28 层的键值缓存截到前 `keep` 个位置。
///
/// 截的是序列轴（`(B, H, T, D)` 的第 2 轴）。截掉的那一段这一步会重算：活动窗
/// 换了内容，它后面的位置整体跟着变。
fn truncate_cache(
    ops: &mut MlxBackend,
    cache: &mut [KvCache<MlxTensor>],
    keep: usize,
) -> Result<(), MlxError> {
    for slot in cache.iter_mut() {
        let (Some(key), Some(value)) = (slot.key(), slot.value()) else {
            return Err(MlxError::new("键值缓存是空的，截不了"));
        };
        let dims = key.dims.to_vec();
        if dims.len() != 4 || keep > dims[2] {
            return Err(MlxError::new(format!(
                "键值缓存的形状 {dims:?} 截不到 {keep} 个位置"
            )));
        }
        let ranges = [
            AxisSlice::new(0, dims[0]),
            AxisSlice::new(0, dims[1]),
            AxisSlice::new(0, keep),
            AxisSlice::full(dims[3]),
        ];
        let key = ops.slice(key, &ranges)?;
        let value = ops.slice(value, &ranges)?;
        slot.store(key, value);
    }
    Ok(())
}

/// 下一步的解码起点：上一步整段假设去掉停止符，再去掉最近 `unfixed` 个 token。
///
/// 停止符不写进提示：它是「这里结束」的记号，不是要预测的文本。
pub fn stable_tokens(tokens: &[u32], unfixed: usize) -> &[u32] {
    let mut end = tokens.len();
    while end > 0 && matches!(tokens[end - 1], IM_END | EOS) {
        end -= 1;
    }
    let keep = end.saturating_sub(unfixed);
    &tokens[..keep]
}

fn check_rows(len: usize, tokens: usize) -> Result<(), MlxError> {
    if len != tokens * HIDDEN {
        return Err(MlxError::new(format!(
            "编码器投影 {len} 个元素，与 {tokens} 行的形状不一致"
        )));
    }
    Ok(())
}

/// 提示里第一个音频占位的位置是不是 `head` 的末尾。给测试用。
#[cfg(test)]
fn head_ends_before_the_pads(ids: &[u32], head: usize) -> bool {
    ids.len() > head
        && ids[..head]
            .iter()
            .all(|id| *id != voice_tensor::AUDIO_PAD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_prefix_drops_stop_tokens_and_the_unfixed_tail() {
        let tokens = [10, 11, 12, 13, 14, 15, 16, 17, IM_END];
        assert_eq!(stable_tokens(&tokens, 5), &[10, 11, 12]);
        assert_eq!(stable_tokens(&tokens, 0), &[10, 11, 12, 13, 14, 15, 16, 17]);
        assert_eq!(stable_tokens(&[], 5), &[] as &[u32]);
        assert_eq!(stable_tokens(&[IM_END], 5), &[] as &[u32]);
        assert_eq!(stable_tokens(&[1, 2], 5), &[] as &[u32]);
    }

    /// 送进来的样本数算得对：还没进梅尔的那一段也要算上。
    #[test]
    fn pushed_samples_are_counted_before_the_mel_runs() {
        let mut stream = Stream::new(5);
        stream.push(&[0i16; 1_000]);
        stream.push(&[0i16; 24]);
        assert_eq!(stream.samples(), 1_024);
        assert_eq!(stream.mel.frames(), 0);
        // 新开的流没有可复用的位置。
        assert_eq!(stream.cached_tokens(), 0);
    }

    /// 拼接的提示里，音频占位从提示头之后开始：复用键值的那一段正好落在占位之前
    /// 与占位开头，不会跨到 `tail` 里。
    #[test]
    fn pads_start_right_after_the_head() {
        let parts = super::super::transcribe::PromptParts {
            head: vec![1, 2, 3],
            tail: vec![9, 8],
        };
        let ids = splice_prompt(&parts, 3);
        let pad = voice_tensor::AUDIO_PAD;
        assert_eq!(ids, vec![1, 2, 3, pad, pad, pad, 9, 8]);
        assert!(head_ends_before_the_pads(&ids, 3));
        assert!(!head_ends_before_the_pads(&ids, 5));
    }
}
