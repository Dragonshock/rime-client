//! 一次按住对应的流式状态。已经结束的注意力窗留在 `encoder_prefix`，活动窗留在 `mel_pending`。

use crate::config::{AsrConfig, EncoderConfig, TextTowerConfig};

/// 尚未构成完整注意力窗的梅尔帧。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MelPending {
    pub bins: usize,
    pub frames: usize,
}

/// 已结束各窗的 `(token, width)` 拼接。宽度与文本隐层相同。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncoderPrefix {
    pub token_count: usize,
    pub width: usize,
}

/// 一层解码器里，只覆盖稳定前缀的 KV。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvLayer {
    pub len: usize,
    pub kv_heads: usize,
    pub head_dim: usize,
}

/// 28 层解码器缓存。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvCache {
    pub layers: Vec<KvLayer>,
}

impl KvCache {
    pub fn new(text: TextTowerConfig) -> Self {
        Self {
            layers: (0..text.layers)
                .map(|_| KvLayer {
                    len: 0,
                    kv_heads: text.kv_heads,
                    head_dim: text.head_dim,
                })
                .collect(),
        }
    }

    pub fn set_stable_len(&mut self, len: usize) {
        for layer in &mut self.layers {
            layer.len = len;
        }
    }
}

/// 流式状态。字段与设计表一致。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamState {
    /// 不足一个梅尔帧的 `i16` 样本。
    pub pcm_tail: Vec<i16>,
    /// 尚未构成完整窗的梅尔帧。
    pub mel_pending: MelPending,
    /// 已结束各窗的编码器输出。
    pub encoder_prefix: EncoderPrefix,
    /// 只含稳定前缀的解码器缓存。
    pub kv: KvCache,
    /// 当前整段假设的 token。`unfixed` 是它的后缀。
    pub tokens: Vec<u32>,
    /// 最近若干 token。下一编码步允许替换它们。
    pub unfixed: Vec<u32>,
    /// 上一轮交给调用方的整段 UTF-8。
    pub published: String,
}

impl StreamState {
    pub fn new(config: &AsrConfig) -> Self {
        Self::from_towers(config.encoder, config.text)
    }

    pub fn from_towers(encoder: EncoderConfig, text: TextTowerConfig) -> Self {
        Self {
            pcm_tail: Vec::new(),
            mel_pending: MelPending {
                bins: encoder.mel_bins,
                frames: 0,
            },
            encoder_prefix: EncoderPrefix {
                token_count: 0,
                width: encoder.proj_out,
            },
            kv: KvCache::new(text),
            tokens: Vec::new(),
            unfixed: Vec::new(),
            published: String::new(),
        }
    }

    /// 已发布 token 去掉未固定后缀，作为下一步的解码起点。
    pub fn stable_tokens(&self) -> &[u32] {
        let drop_n = self.unfixed.len().min(self.tokens.len());
        &self.tokens[..self.tokens.len() - drop_n]
    }
}
