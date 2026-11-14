# 054-research-decoder-parity：预填充最后一个位置的 logits 对上，argmax 与贪心首 token 一致

测量日：2026-11-14。对象是 `code/voice-engine/voice-mlx` 新加的
`backend/text_decoder.rs`（文本塔权重与前向），以及 `backend/weights.rs` 里
新增的「按行取分组权重」。参考资产来自 `code/voice-oracle/assets`，参考实现是
`code/voice-worker/.venv` 里的 `mlx_qwen3_asr` 0.4.4，只作对照标准。

## 0. 结论速览

| 闸 | 参考形状 | 最大绝对差 | 最大差位置 | 参考绝对值上界 | 容差 | top-1 |
| --- | --- | --- | --- | --- | --- | --- |
| 主闸：注入本仓编码器输出 | (64) | **4.6875e-2** | 下标 0：实际 19.75、参考 19.703125 | 21 | 8.203125e-2 | **一致**（106287） |
| 归因口径：注入参考 `encoder_proj_out` | (64) | **3.125e-2** | 下标 1：实际 17.171875、参考 17.203125 | 21 | 8.203125e-2 | **一致**（106287） |

两项都在容差内，超差元素 0 个，容差余量 1.75 倍。全词表 argmax 是 106287，
与 `manifest.greedy.tokens` 的第一位（也是参考 `generate_with_info` 的第一个
token）一致。

「首 64 维里最大的那一维」是下标 13，不是 106287。所以 top-1 必须在全词表
151936 维上取 argmax：只比前 64 维判断不出下一个 token 是哪一个。这也是
`lm_head` 要跑完整张表的原因（见第 6 节）。

## 1. 参考的 dtype 与资产口径

- `manifest.json` 写 `model_dtype: float16`、`activation_dtype: mlx.core.float16`。
- 参考 `prefill()` 是 `lm_head(hidden[:, -1:, :])`，`lm_head` 与词嵌入同表，整条链
  在 f16 上跑完，`prefill_logits_64` 落的是 f16 值在 f32 里的展开。
- 因此两侧的比较是同一层量化下的数：我们的 logits 也是 f16 矩阵乘的输出，
  主机上再展开成 f32。
- 21.0 落在 f16 的 `[16, 32)` 区间，该区间格点间距是 `2^-6 = 1.5625e-2`。
  实测两个差分别是这个间距的 3 倍与 2 倍，即 **3 个 ULP 与 2 个 ULP**，
  不是模型接错那种量级。

## 2. 权重来源

`weights.safetensors` 是「8 字节表头长度 + JSON 表头 + 裸数据」。解码器只用
`model.*` 这组键：

| 名字 | 存放 | 读法 |
| --- | --- | --- |
| `model.embed_tokens` | 4 位分组，打包 `U32 (151936, 128)`，`scales`/`biases` `F16 (151936, 16)` | `affine_rows`，只取要用的行 |
| `model.layers.N.self_attn.{q,k,v,o}_proj` | 4 位分组 | `affine_as` |
| `model.layers.N.mlp.{gate,up,down}_proj` | 4 位分组 | `affine_as` |
| `model.layers.N.{input,post_attention}_layernorm`、`q_norm`、`k_norm`、`model.norm` | 稠密 `F16` | `dense_as` + eps |

位数与组大小不是猜的：`quantization_config.json` 写 `bits: 4`、`group_size: 64`；
`q_proj` 的打包是 `(2048, 128)` 配缩放 `(2048, 16)`，`1024 * 4 / 32 = 128`、
`1024 / 64 = 16`；`down_proj` 是 `(1024, 384)` 配 `(1024, 48)`，
`3072 * 4 / 32 = 384`、`3072 / 64 = 48`。只有 4 位这一组数能同时对上。

**`lm_head` 没有独立张量。** `text_config.tie_word_embeddings` 为真、
`output_size == vocab_size`，所以参考的 `load_models` 在缺 `lm_head.scales` 时
把 `model.embed_tokens` 的三件张量物化成 `lm_head.*`，加载完再把
`lm_head.weight` 指回同一张表。我们的做法是同一个意思：`lm_head` 就是词嵌入表，
用同一份打包权重、同一份缩放与偏置。

装载时把权重统一转成激活 dtype（f16）。参考也会把浮点参数整体投到 f16，
否则 f32 的缩放会把整条链提升成 f32，与参考就不是同一个算术。

## 3. 前向口径

1. **提示**：`manifest.prompt.tokens` 那 44 个 id，结构是
   `[151644, 8948, 198, 151645, 198, 151644, 872, 198,` + `151669` +
   26 个 `151676` + `151670, 151645, 198, 151644, 77091, 198, 11528, 8453, 151704]`。
   26 个占位等于音频 token 数（`audio_frames.n_audio_tokens`）。
2. **组装嵌入**：占位处换成编码器末尾投影的对应行（`(26, 1024)` 的第 n 行给第 n 个
   占位）；其余 18 个 token 按 id 取词嵌入行。参考是先 gather 再用 `mx.where`
   替换，占位处的词嵌入行不参与结果，所以那 26 行不读也不影响数值。
3. **MRoPE**：三个轴的位置相同，都是 `0..43`。`inv_freq = 1 / theta^(2i/128)`，
   `theta = 1e6`，`i < 64`；频率按 `[24, 20, 20]` 以步长 3 交错覆盖（时间轴打底，
   高/宽覆写）；`emb = [freqs | freqs]` 长 128；`cos/sin` 在 f32 上算完再转 f16。
   三个轴同值时交错覆盖是恒等操作，代码仍照参考写全，不靠这个巧合省掉。
4. **因果遮罩**：加法 mask，上三角填 `-1e9` 再按 f16 存（即 `-inf`），形状
   `(1, 1, 44, 44)`。
5. **28 层**：`pre-norm RMSNorm(eps 1e-6)`（无偏置）→ 投影 `q(2048) / k,v(1024)` →
   reshape `(1, L, 头, 128)` → **对每个头做 RMSNorm(128)**（q 与 k 都做）→
   转置 `(1, 头, L, 128)` → `x*cos + rotate_half(x)*sin`（不是只乘 sin）→
   `fast SDPA`（scale `1/sqrt(128)`，8 个 KV 头铺成 16 个查询头）→ `o_proj` → 残差；
   随后 `post-norm` → `silu(gate) * up` → `down` → 残差。全程 f16，
   **没有逐层钳位**（参考只在音频塔里钳到 64504，文本塔没有）。
6. **末尾**：`model.norm`（eps 1e-6）→ 只取最后一个位置 → `lm_head`（词嵌入表）
   分块算 logits。

## 4. 容差怎么定

单位沿用 053 的写法：`2^-11 = 4.8828125e-4`（f16 在 1.0 附近的格点间距），
容差写成

```text
容差 = 参考该张量的绝对值上界 × 2^-11 × 乘数
```

本闸乘数取 **8**：`21 × 2^-11 × 8 = 8.203125e-2`。依据与实测：

| 口径 | 最大绝对差 | 折到上界单位 | 局部 ULP | 容差 | 余量 |
| --- | --- | --- | --- | --- | --- |
| 注入本仓编码器输出 | 4.6875e-2 | 4.6 | 3 | 8.203125e-2 | 1.75 倍 |
| 注入参考 `encoder_proj_out` | 3.125e-2 | 3.0 | 2 | 8.203125e-2 | 2.6 倍 |

乘数按「28 层残差每层往隐层里叠一次 f16 存储 + 末尾 1024 项 f16 点积」定，
取 8；没有按「每层一个单位、28 层线性累加」取 28，因为残差是加法，舍入有正有负，
不是线性累加。第一版测试里写的是 64 个单位，实测回来后**收到 8** 仍然全过——
这里记的是收紧，不是放宽。

两条口径的差是 3 个单位与 4.6 个单位，差的这 1.6 个单位来自音频特征本身：
053 记的 `encoder_proj_out` 最大差 `1.1855885e-3` 会一路带进 logits。
所以主闸的数包含编码器的差，归因口径的数只属于解码器。

## 5. 归因：换掉口径差值从哪一步变大

`wrong_decoder_options_make_the_difference_bigger` 用 `DecoderOptions` 把两条口径
各换一次，同一份提示、同一份音频特征重跑。参考音频特征注入，差值只属于解码器：

| 口径 | 首 64 维最大差 | 全词表 argmax |
| --- | --- | --- |
| 默认（`rotate_half` + q/k 归一化） | 3.125e-2 | **106287** |
| RoPE 只乘 sin（不旋转） | **25.238281** | 102393 |
| 不做 q/k 归一化 | **21.422363** | 4806 |

两条错口径都把差值放大到 21 以上（分别是 800 倍与 686 倍），argmax 也跟着变。说明这一闸能
分辨「旋转」与「只乘 sin」、以及「每个头做 RMSNorm」与「不做」，对上不是碰上的。

## 6. 按行取权重与分块算 logits

词嵌入表是 151936×1024：整表反量化到 f16 要 311 MB，`lm_head` 还要再乘一次。
两处都按「行之间互不影响」避开整表：

- 分组量化沿最后一维按 64 一组切开，一行的反量化只用到这一行自己的打包字、
  缩放与偏置。`weights.affine_rows` 按行号 seek 读出这些字节，拼成
  `(行数, …)` 再交给 `mlx` 的 `dequantize`。
- 组装嵌入时只读提示里那 18 个文本 token 的行。
- `lm_head` 按 8192 行一块（`LM_HEAD_BLOCK`）算：`(1,1024) × (1024, 8192)`，
  每块取 argmax 与第一块的前 64 维，19 块跑完整张表。每个 logit 仍是
  `h · emb[j]` 这 1024 项，行块大小不改变它。
- 28 层的权重在循环里**按需装载、用完即弃**：每层 `11264 × 1024` 个 f16 ≈ 23 MB，
  峰值不会叠成 646 MB。

这个「行独立」不是一句推断：`embedding_rows_do_not_depend_on_the_block` 用三种
取法取同一行——单独取第 7 行、在 16 行块里取、在 64 行块里取——三种给同一组数；
再把 64 行拆成两段 32 行读，拼起来与整块读逐位相同。

## 7. 命令与实测

```text
cd code/voice-engine
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
export RUSTFLAGS="-C link-arg=-framework -C link-arg=Accelerate \
  -C link-arg=-Wl,-U,_MTLTensorDomain -C link-arg=-Wl,-U,_MTLLogStateErrorDomain \
  -C link-arg=-Wl,-U,_NSDeviceCertificationiPhonePerformanceGaming \
  -C link-arg=-Wl,-U,_NSProcessInfoPerformanceProfileDidChangeNotification \
  -C link-arg=-Wl,-U,_NSProcessPerformanceProfileDefault \
  -C link-arg=-Wl,-U,_NSProcessPerformanceProfileSustained \
  -C link-arg=-Wl,-U,___fmaxf16 -C link-arg=-Wl,-U,___fminf16"
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test decoder_parity -- --nocapture --test-threads=1
```

```text
running 5 tests
test decoder_prefill_matches_reference ... ok         最大绝对差 4.6875e-2，argmax 106287
test decoder_with_reference_audio_features ... ok     最大绝对差 3.125e-2，argmax 106287
test wrong_decoder_options_make_the_difference_bigger ... ok
test embedding_rows_do_not_depend_on_the_block ... ok
test text_decoder_weights_load_from_safetensors ... ok
test result: ok. 5 passed; 0 failed   （22.5 秒）
```

链接参数为什么要写这么长：见 053 第 6 节与 `code/voice-engine/BUILD.txt`，
MLX 的静态库引用了 `Accelerate` 的 BNNS 与 cblas，还引用了当前 SDK 只在 arm64e
段导出的一组 Metal/Foundation 弱符号与两个 `_Float16` 内建。

单元测试（mrope 的交错覆盖与两半相同）与不编 MLX 的整套：

```text
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline --lib
  test backend::text_decoder::tests::mrope_angles_follow_the_frequency_owner ... ok
  test result: ok. 4 passed

cargo test --workspace --offline     # exit 0，不编 MLX
cargo test -p voice-parity --offline # 3 通过 / 4 忽略
```

`voice-parity` 里 `decoder_matches_reference` 与 `logits_match_reference` 仍是
`#[ignore]`，理由改成「实对照在 `voice-mlx/tests/decoder_parity.rs`：本箱子不链接
MLX」，打印参考形状与本次记下的差值，不再说「无从计算」。`greedy_tokens_match_reference`
仍是「无从计算」——解码循环还没做（见第 8 节）。

改了哪些文件：

| 文件 | 改动 |
| --- | --- |
| `voice-mlx/src/backend/text_decoder.rs` | 新增：`model.*` 权重装载、44 token 前向、`lm_head` 分块取 logits、`DecoderOptions` 两个可换口径 |
| `voice-mlx/src/backend/weights.rs` | 新增 `affine_rows`（按行取分组权重）、`read_rows`，把反量化的尾巴抽成 `dequantize_block` 与 `dequant_as` 共用 |
| `voice-tensor/src/config.rs` | 新增 `MROPE_SECTION = [24, 20, 20]` |
| `voice-mlx/tests/decoder_parity.rs` | 新增：主闸、归因口径、两条错口径的归因、按行取权重的一致性、权重可装载 |
| `voice-parity/tests/parity.rs` | 两个入口改成指向实对照并打印本次的数 |

## 8. 仍未对上的项

- **贪心 token 第 2、3 位（1773、151645）没有对上**。本次只有预填充一次前向，
  没有 KV 缓存与逐步解码，`greedy_tokens_3` 只对上了第一位。参考的
  `_build_decode_positions` 与 `KVCache.step` 那条路径没有被这段代码执行到。
- **argmax 处的 logit 没有参考值**。资产只落前 64 维，106287 维的值无从比；
  全词表 argmax 只能与 token id 比。
- **文本塔 28 层没有逐层参考**。资产里没有解码器隐层，`layers.N` 只能经末尾
  logits 间接约束；要逐层报差值，得往 `dump_reference.py` 里补隐层输出。
- **KV 缓存与预分配缓存没有对照**。参考用 `KVCache(max_seq_len=…)` 预分配并
  原地写入，本次一次前向不需要缓存，那段代码没有执行。
- **解码器还没有接进产品路径**。`voice-asr` 的 `decode_prefill` 仍是全零提示 +
  乘法 RoPE + 无因果遮罩的形状骨架，`voice-mlx::transcribe_finish` 仍是 64 采样
  均值那条骨架；本次对上的是 `TextDecoder::prefill` 这条真实数值路径。
- **提示用的是资产里的 44 个 id**，不是 `voice-tokenizer` 现场生成的。
  接 `build_prompt_tokens` 会换输入，差值不与本闸可比。
- **音频占位用的是本仓编码器输出**（主闸）或参考 `encoder_proj_out`（归因口径）。
  前者含 053 记的编码器差值，后者不含；端到端那条数没有单独报。

## 检索

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | 读参考的解码器层 | `code/voice-worker/.venv/.../mlx_qwen3_asr/decoder.py` | pre-norm RMSNorm、GQA、q/k 每头归一化、`fast SDPA`、SwiGLU、加法因果 mask |
| 2 | 读参考的位置编码 | 同目录 `mrope.py` | `[24,20,20]` 步长 3 交错、`emb=[freqs|freqs]`、`rotate_half` 不是只乘 sin |
| 3 | 读顶层模型 | 同目录 `model.py` | `prefill` 注入音频特征后只取最后一个位置过 `lm_head` |
| 4 | 读参考的权重装载 | 同目录 `load_models.py` | 缺 `lm_head.scales` 时把词嵌入的 scales/biases 物化，再把 `lm_head.weight` 指回同表 |
| 5 | 核对打包形状与键名 | 模型目录 `weights.safetensors` 表头 | `model.embed_tokens.weight (151936,128)` 配 `(151936,16)`；没有 `lm_head.*` |
| 6 | 读量化配置 | 模型目录 `quantization_config.json` | `bits: 4`、`group_size: 64`（文本），音频塔 8 位 |
| 7 | 读参考资产口径 | `code/voice-oracle/assets/manifest.json` | logits 也是 f16；只落前 64 维；提示 44 个 id |
| 8 | 写权重按行取 | `voice-mlx/src/backend/weights.rs` | `affine_rows` / `read_rows`，反量化尾巴与整表共用 |
| 9 | 写解码器前向 | `voice-mlx/src/backend/text_decoder.rs` | 28 层 + MRoPE + 因果 mask + 分块 `lm_head` |
| 10 | 写五条测试 | `voice-mlx/tests/decoder_parity.rs` | 4.6875e-2 / 3.125e-2 / argmax 一致 / 25.24 vs 21.42 / 行独立 |
| 11 | 跑全工作区 | `cargo test --workspace --offline` | exit 0 |

<!-- research-complete -->
