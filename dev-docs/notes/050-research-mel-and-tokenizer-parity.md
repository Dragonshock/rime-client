# 050-research-mel-and-tokenizer-parity：梅尔前端与分词器已与参考实现对齐

测量日：2026-11-14。对象是新增的 `code/voice-engine/voice-mel` 与 `code/voice-engine/voice-tokenizer`。
参考实现是 `code/voice-worker/.venv` 里的 `mlx_qwen3_asr`。它只作对照标准，不进安装包。

## 1. 梅尔前端

`voice-mel` 是纯 Rust，无张量后端依赖。

- 参数：16 kHz、`n_fft = 400`、`hop = 160`、128 个频点、`fmin = 0`、`fmax = 8000`。
- 滤波器组与 Hann 窗直接从参考实现的资产导出，落成 `assets/mel_filters_128.bin`（128×201，f32 小端）与 `assets/hann_400.bin`。不重新推导 Slaney 公式。
- 三步整形与参考一致：功率谱取 `log10` 并截到 `1e-10`；丢掉最后一帧；先减去「全局最大值减 8」，再 `(x + 4) / 4`。

两处曾经写错，都是照参考核对之后改的：

| 项 | 错的样子 | 参考的样子 |
| --- | --- | --- |
| 帽 | `n - 1` 分母（对称 Hann） | `np.hanning(401)[:-1]`，周期 Hann |
| 幅值 | `sqrt(re² + im²)` | `re² + im²`（功率谱） |
| 补边 | 左侧按原序 | 左侧倒序（`numpy` 的 `reflect`） |
| 帧数 | `len / hop + 1` | `len / hop`（最后一帧被丢） |

对照结果：同一段 8000 采样、`i = (i * 37) % 2001 - 1000` 的确定性信号，128×50 个元素与参考逐元素最大差小于 `1e-3`。门槛不放更严，因为参考走 FFT、这里走直接 DFT，求和顺序不同。参考数据与样本一起放在 `assets/reference_pcm.bin` 与 `assets/reference_mel.bin`，测试 `matches_reference_mel_within_1e_3` 每次都会重跑。

`cargo test -p voice-mel` 3 项通过。

## 2. 分词器

`voice-tokenizer` 是纯 Rust，不引入 JSON 依赖：`vocab.json` 是一层扁平对象，自己解析；特殊 token 从 `tokenizer_config.json` 的 `added_tokens_decoder` 取 id；合并表读 `merges.txt`。

与参考逐条对照（`cargo test -p voice-tokenizer` 里的 `tests/reference.rs`，需要环境变量 `VOICE_MODEL_DIR` 指向模型目录）：

| 输入 | 参考 | 本实现 |
| --- | --- | --- |
| `language Chinese` | `[11528, 8453]` | 一致 |
| `你好` | `[108386]` | 一致 |
| `今天开会` | `[100644, 113716]` | 一致 |
| `<\|audio_start\|>` 等六个特殊 token | 151669 / 151676 / 151670 / 151644 / 151645 / 151704 | 一致 |
| `decode([151669, 151676, 151670])` | 字面量 | 一致 |

三处曾经写错：

1. 字节表用 `' '` 当空位标记，导致字节 `0x20`（空格）没有被映射成 `Ġ`（U+0120），`" Chinese"` 因此断成 `Chinese`，命中 44923 而不是 8453。改成 `'\0'` 当占位符。
2. 合并表里同一 pair 出现多次时，`HashMap::insert` 保留最后一次的名次；改成 `entry().or_insert()`，先出现的名次更靠前。
3. 预切分把 `> 0x2FFF` 一律当字母，把全角逗号也算进单词。改成只把表意文字、假名、谚文算字母。

## 3. 还没做的部分

出字需要的三段里，这两段好了，第三段还是空的：

- 编码器与解码器的数值前向：`voice-asr` 与 `voice-mlx` 现在只有形状骨架，权重没有被送进那些层。`voice-mlx/src/backend/finish.rs` 的 `transcribe_finish` 把整段 PCM 压成 64 个均值的 8×8 图，只读 `conv2d1`、`conv_out`、第 0 层 qkv、`proj2`、`embed_tokens`，只解一个 token。它不可能产出一句话。
- 预填充的低标：`voice-asr` 的 `decode_prefill` 用全零 id 造提示，RoPE 用乘法而不是 `rotate_half`，注意力没有因果遮罩。这些都要按参考重写。

因此现在按住右 Option 仍然不出字。`voice-ffi` 会明确回一条失败原因，不再提交假文本。

## 检索

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | 读参考梅尔 | `mlx_qwen3_asr/audio.py` | `n_fft=400`、`hop=160`、功率谱、丢最后一帧 |
| 2 | 读滤波器资产 | `assets/mel_filters.npz` | 键 `mel_128`，形状 128×201 |
| 3 | 导出资产 | 同上 | `mel_filters_128.bin`、`hann_400.bin` |
| 4 | 生成参考梅尔 | `log_mel_spectrogram` | 128×50 |
| 5 | 对照 Rust 梅尔 | `cargo test -p voice-mel` | 最大差 < 1e-3 |
| 6 | 读参考分词器 | `mlx_qwen3_asr/tokenizer.py` | 字节级 BPE，`_bytes_to_unicode` |
| 7 | 取特殊 token id | `tokenizer_config.json` | 151669 / 151676 / 151670 / 151644 / 151645 / 151704 / 151643 |
| 8 | 取得参考编码 | `_NativeQwenBPETokenizer.encode` | 三串六 token |
| 9 | 对照 Rust 分词器 | `cargo test -p voice-tokenizer` | 4 + 4 项通过 |
| 10 | 全工作区 | `cargo test --workspace --offline` | 39 项通过 |

<!-- research-complete -->
