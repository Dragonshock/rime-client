# 055-research-end-to-end-parity：一段探针走完梅尔到分词器，贪心 token 与参考逐位相同

测量日：2026-11-14。对象是 `code/voice-engine/voice-mlx` 新增的
`backend/transcribe.rs`（整条 PCM → 文本）与 `backend/text_decoder.rs` 里新增的
KV 缓存逐步解码，以及接回 `code/voice-engine/voice-ffi` 定稿路径这件事。
参考资产来自 `code/voice-oracle/assets`，参考实现是
`code/voice-worker/.venv` 里的 `mlx_qwen3_asr` 0.4.4，只作对照标准。

## 0. 结论速览

| 步 | 比什么 | 参考 | 实测 | 结论 |
| --- | --- | --- | --- | --- |
| 梅尔 | `mel.bin` (128, 200) | f32 | 最大绝对差 **3.2138824e-4**（下标 17189） | 在 1e-3 内 |
| 编码器 | `conv_stem_out` (16,13,480) | f16 | 最大绝对差 **0** | 逐位相同 |
| 编码器 | `encoder_layer0_out` (1,26,896) | f16 | 最大绝对差 **3.90625e-3**（下标 1697） | 容差 1.2954712e-2 内 |
| 编码器 | `encoder_proj_out` (26,1024) | f16 | 最大绝对差 **1.1855885e-3**（下标 20688） | 容差 1.9779205e-3 内 |
| 预填充 | `prefill_logits_64` 前 64 维 | f16 | 最大绝对差 **4.6875e-2**（下标 0：实际 19.75、参考 19.703125） | 容差 8.203125e-2 内，全词表 argmax 106287 |
| 提示 | `manifest.prompt.tokens` 44 个 id | — | 分词器现场拼出，**44/44 逐位相同** | 相同 |
| 贪心 | `greedy_tokens_3` | `[106287, 1773, 151645]` | **`[106287, 1773, 151645]`** | 逐位相同 |
| 文本 | `manifest.greedy.text_from_tokens` 去标记 | `"嗯。"` | **`"嗯。"`** | 相同 |

实际 token 序列：`[106287, 1773, 151645]`。
期望 token 序列：`[106287, 1773, 151645]`（`greedy_tokens_3.bin`，第三位是
`<|im_end|>`，参考自带的 `generate_with_info` 会剥掉它，资产保留原样）。
实际文本：`"嗯。"`。参考 `text_from_tokens` 是 `"嗯。<|im_end|>"`，去掉特殊
token 字面量之后同样是 `"嗯。"`。

一次完整的 `transcribe` 里，提示 44 个 id、音频 token 26、梅尔帧 200，都与
`manifest` 记的参考数相同。这次不是把资产里的提示喂进去：提示是
`voice-tokenizer` 按参考模板现场拼的（见第 1 节）。

## 1. 端到端路径

`voice-mlx::transcribe(model_dir, pcm)` 一次走完五步，中间不留手写常数：

1. **梅尔**：`voice-mel` 的 `MelFrontend::log_mel` 出 `(128, frames)`，
   `frames = 样本数 / 160`。32000 个采样 → 200 帧，与参考一致。
2. **编码器**：`AudioTower::open(model_dir, F16)` + `encode_mel` 出
   `EncoderOutput { tokens, proj }`。音频 token 数取 `tokens`（实际行数），
   不按帧数套公式；`proj` 必须是 `tokens × 1024`，不是就报错。
3. **提示**：`build_prompt` 按参考 `Tokenizer.build_prompt_tokens` 的模板拼：

   ```text
   <|im_start|>system
   {context}<|im_end|>
   <|im_start|>user
   <|audio_start|><|audio_pad|>…{n}…<|audio_end|><|im_end|>
   <|im_start|>assistant
   language {language}<asr_text>
   ```

   模板走 `voice-tokenizer` 的编码（`system\n`、`user\n`、`assistant\n` 各自成段），
   音频占位按第 2 步的行数重复。参考那次对照的语言是 `Chinese`、上下文为空，
   本路径的默认值相同（`PROMPT_LANGUAGE`、空 `context`）。
4. **预填充 + 逐 token 贪心**：`TextDecoder::generate`，见第 2 节。
5. **分词器**：`Tokenizer::decode_without_special` 去掉 `<|im_start|>`、
   `<|im_end|>`、`<asr_text>`、`<|audio_start|>`、`<|audio_pad|>`、`<|audio_end|>`
   这些字面量，只留识别出来的字。

`prompt_tokens(model_dir, n, language, context)` 单独暴露第 3 步，给对照用：
它不跑数值前向，只回答「这一层拼出来的提示是不是参考那 44 个 id」。

## 2. 解码循环的口径

参考的 `generate` 是「预填充一次，之后逐步把上一个 token 送回 `model.step`」。
这里同序，三处细节照参考写：

1. **KV 缓存**：每层一份 `KvCache`，预填充时写入提示的键值，单步时先拼上新
   键值再拿整段键值做注意力。位置从提示长度继续：新 token 的位置是
   `prompt_len`、`prompt_len + 1`……，不是从 0 重数。
2. **遮罩**：预填充用加法因果 mask（上三角 `-1e9` 按 f16 存，即 `-inf`），形状
   `(1, 1, L, L)`；单步只有 1 个查询位置，缓存里的前缀全部可见，遮罩为空。
   这与参考 `decoder.py` 的 `cache.offset > 0 && L == 1 → attention_mask = None`
   同一分支。
3. **停止**：argmax 出 `151643`（`<|endoftext|>`）或 `151645`（`<|im_end|>`）就收工，
   那个停止符记进返回值；没遇到停止符就到 `MAX_NEW_TOKENS = 256` 为止。返回的
   `stopped_by` 说明是哪一种，测试里断言的是 `Some(StopReason::ImEnd)`——不是「跑满
   三个就停」；空音频没有解码，那里是 `None`。

一条只碰解码器的自检：`cached_step_matches_a_fresh_prefill`。先用 `generate`
生成 1 个 token，再把它拼回提示做一次完整前向，取 argmax；另一次 `generate`
取两个 token，第二位应当等于那个 argmax。实测两边都是 **1773**。这条挡的是
位置编号或遮罩写错：缓存路径与重算路径一旦分叉，两个数就不同。

KV 缓存用的是「每步沿时间轴拼一次」，不是参考的 `KVCache(max_seq_len=…)`
预分配后原地写。两种写法的键值内容相同，差别只在显存分配方式；参考的预分配
分支在这次代码里没有执行到（见第 7 节）。

## 3. 四步实测

### 3.1 梅尔

`voice-parity` 的 `mel_matches_reference`：

```text
对照量：mel（参考形状 (128, 200)）
元素数：参考 25600，实际 25600
最大绝对差：3.2138824e-4（下标 17189：实际 -8.3754385e-1，参考 -8.3786523e-1）
首个超差：无（全部在容差 1e-3 内）
```

容差 1e-3 的理由沿用 050：参考走 FFT，`voice-mel` 走直接 DFT，求和顺序不同。
这也是端到端那条路径与 053/054 的差别之一：053 的编码器闸吃的是参考 `mel.bin`，
端到端吃的是 `voice-mel` 的输出，两者差 3.2e-4，因此端到端的结果里也带着这一层差。

### 3.2 编码器

`encoder_parity.rs` 的三件（输入是参考 `mel.bin`，与 053 同口径，本次重跑确认未变）：

| 参考量 | 参考绝对值上界 | 容差 | 最大绝对差 | 超差元素 |
| --- | --- | --- | --- | --- |
| `conv_stem_out` | 2.0253906 | 3.955841e-3 | 0 | 0 |
| `encoder_layer0_out` | 6.6328125 | 1.2954712e-2 | 3.90625e-3（下标 1697） | 0 |
| `encoder_proj_out` | 0.12658691 | 1.9779205e-3 | 1.1855885e-3（下标 20688） | 0 |

容差写法与 053 一致：`参考该张量的绝对值上界 × 2^-11 × 乘数`（4 / 4 / 32 个单位）。

### 3.3 预填充 logits

`decoder_parity.rs` 的两条口径（本次重跑确认与 054 相同）：

| 口径 | 首 64 维最大绝对差 | 最大差位置 | 全词表 argmax |
| --- | --- | --- | --- |
| 注入本仓编码器输出（主闸） | **4.6875e-2** | 下标 0：实际 19.75、参考 19.703125 | 106287 |
| 注入参考 `encoder_proj_out`（归因） | **3.125e-2** | 下标 1：实际 17.171875、参考 17.203125 | 106287 |

参考绝对值上界 21，容差 `21 × 2^-11 × 8 = 8.203125e-2`，超差元素 0 个；两条
余量分别是 1.75 倍与 2.6 倍。4.6875e-2 是 f16 在 `[16, 32)` 区间格点间距
`1.5625e-2` 的 3 倍，即 3 个 ULP。

### 3.4 贪心与文本

`end_to_end_parity.rs` 的实跑输出：

```text
梅尔帧 200、音频 token 26、提示 44、新生成 3（Some(ImEnd)），墙钟 11.656028042s
token 序列：greedy_tokens_3
期望（3 个）：[106287, 1773, 151645]
实际（3 个）：[106287, 1773, 151645]
开头相同 3 个
首处不同：无（逐位一致）

参考文本 "嗯。<|im_end|>"（去标记后 "嗯。"），实际文本 "嗯。"
```

墙钟约 11.7 秒是 2 秒探针、未预热的一次 `transcribe`：含音频塔与文本塔两次权重
装载、44 token 预填充与 2 步解码。它不是从麦克风到文字的时延（见第 7 节）。

## 4. 文本与特殊 token

`voice-tokenizer` 新增 `decode_without_special` 与 `is_special_token`：

- `decode` 保留原行为：特殊 token 还原成字面量（`"嗯。<|im_end|>"`），
  这是与参考 `tokenizer.decode(skip_special_tokens=False)` 对照时需要的样子。
- `decode_without_special` 先剔除特殊 token 的 id 再解码，得到 `"嗯。"`。
  只有标记时给空串，不回退成字面量。

这条在 `voice-tokenizer/tests/reference.rs` 有真测试，用的就是参考那三个 id：

```text
decode([106287, 1773, 151645])                  = "嗯。<|im_end|>"
decode_without_special([106287, 1773, 151645])  = "嗯。"
```

`voice-ffi` 提交给输入法的字符串取后者。

## 5. 产品路径与假后端

`voice-mlx/src/backend/finish.rs`（64 个均值的 8x8 图、只接第 0 层的骨架）已删除，
`transcribe_finish` 不再存在。现在：

- 打开 `metal`：`Worker::start` 用 `WeightDecoder`，定稿时把累积样本交给
  `voice_mlx::transcribe`，`StepOutput { text, tokens }` 都来自真实数值。
- 不打开 `metal`：仍是 `UnlinkedDecoder`，空串，不编 MLX。
- 空音频：`Session::finish` 在 `total_samples == 0` 时直接回空串定稿、
  `is_final` 为真，`finish` 返回 0；`WeightDecoder` 那一层也兜底回空串且不记失败。
- 失败（打不开权重、某一步报错）：文本为空串，原因 `定稿计算失败：<错误>` 进
  `failure_reason`，`voice_session_finish` 返回非 0，JSON 口发 `ok:false`。
- `voice-ffi/src/vocab.rs`（283 行的词表解析副本）随旧路径一起删掉：
  分词器现在只由 `voice-tokenizer` 提供一份，定稿文本不再经过 `voice-ffi` 自己的词表。

C ABI 实测（`/tmp/voice-abi/e2e`，不在 `code/` 内；用已安装包的 `mlx.metallib`
与安装包内的模型目录 `Resources/Models/Qwen3-ASR-0.6B-4bit`）：

```text
begin 0
samples 32000
feed 0
json {"decoded":true,"kind":"final","ok":true,"session":7,"text":"嗯。"}
finish 0
last_error 0 []
begin-empty 0
json {"decoded":true,"kind":"final","ok":true,"session":8,"text":""}
finish-empty 0
```

同一份归档、换成 HF 快照目录再跑一次，结果相同（`"嗯。"`，13.6 秒）。

`cargo test --workspace --offline`：exit 0，44 通过、4 忽略、0 失败
（`voice-asr` 9、`voice-ffi` 11、`voice-mel` 3、`voice-tokenizer` 4+5、
`voice-parity` 2+4 另有 4 条忽略、`voice-tensor` 6；`voice-mlx` 不打开 `metal`
时没有测试）。`cargo build -p voice-ffi --features metal --target aarch64-apple-darwin`：exit 0。

`voice-mlx` 打开 `metal` 的三套测试（链接参数见 `code/voice-engine/BUILD.txt`）：`encoder_parity` 3 条、`decoder_parity` 5 条、
`end_to_end_parity` 4 条，全过。

## 6. 改动清单

| 文件 | 改动 |
| --- | --- |
| `voice-mlx/src/backend/transcribe.rs` | 新增：梅尔 → 编码器 → 提示 → 贪心 → 分词器的整条路径，`build_prompt` / `prompt_tokens` |
| `voice-mlx/src/backend/text_decoder.rs` | 新增 `Generation` / `StopReason` / `EOS` / `IM_END` / `MAX_NEW_TOKENS`，`generate` 逐步解码，`run_layers` / `final_norm` / `embed_token` / `rope_tables_at`，`decode_layer` 带 KV 缓存与可选遮罩 |
| `voice-mlx/src/backend/finish.rs` | 删除（64 均值 8x8 图那条骨架） |
| `voice-mlx/Cargo.toml` | 新增 `voice-mel`、`voice-tokenizer` 依赖（都是纯 Rust） |
| `voice-tokenizer/src/lib.rs` | 新增 `decode_without_special`、`is_special_token` |
| `voice-tokenizer/tests/reference.rs` | 新增 `decode_without_special_drops_the_markers` |
| `voice-parity/src/lib.rs` | 新增 token 闸 `compare_tokens` / `TokenReport` / `TokenMismatch` |
| `voice-parity/tests/parity.rs` | `greedy_tokens_match_reference` 改指实对照；新增 token 闸自身的行为测试 |
| `voice-mlx/tests/end_to_end_parity.rs` | 新增：提示 44 个 id、贪心序列 + 文本、空音频、缓存单步与重算一致 |
| `voice-ffi/src/decoder.rs` | 定稿改调 `voice_mlx::transcribe`，交出文本与 token 序列 |
| `voice-ffi/src/vocab.rs` | 删除（与 `voice-tokenizer` 重复，旧路径专用） |
| `voice-ffi/src/lib.rs` | 去掉 `mod vocab;` |

## 7. 仍未对上的项

- **贪心序列只有 3 个 token 的参考**。第 4 位以后没有资产可对；这段探针在
  `151645` 就停了，长句的后续 token 只能靠模型自身与分词器，没有参考闸。
- **说话中的部分文本仍是空串**。`WeightDecoder` 只在 `is_final` 时算，非定稿步
  直接回空串；产品现在只在松手后给一次定稿文本，按住说话期间没有部分结果。
- **热词没有进提示**。`build_prompt` 有 `context` 位置（参考塞进 system 段），
  但 `voice-ffi` 收下的 `hotwords` 没有传给 `transcribe`，这条调用链现在是空字符串。
- **长音频的两条路径没有端到端参考**：探针 200 帧正好是两个满块（100×2），
  尾块（帧数非 100 倍数）的补块与切回逻辑没有执行到；窗数 ≥ 20 的按窗切分路径
  也没走到。两条路径都没有参考资产，只能靠归因测试里的口径对照。
- **参考的 KV 预分配缓存没有对照**。本实现每步沿时间轴拼接；参考
  `KVCache(max_seq_len=…)` 的原地写分支没有执行，两种写法的数值一致这一点
  没有直接对照，只有「缓存单步 = 重算」这一条间接约束。
- **argmax 处的 logit 没有参考值**，资产只落前 64 维；`106287` 只能与 token id 比。
- **文本塔 28 层没有逐层参考**，只能经末尾 logits 与 token 序列间接约束。
- **语言固定 `Chinese`**。参考那次对照的语言也是 Chinese；其它语言、`language=None`
  那条分支与热词上下文拼进 system 段的写法都没有参考。
- **识别时延不是麦克风到文字的时延**。这里只有 2 秒探针的墙钟 11.7 秒
  （进程内一次调用）与 13.6 秒（含进程启动、权重装载的 C ABI 调用），都未预热，
  而且输入不是麦克风。
- **提示是分词器现场拼的，但只有这一种语言与空上下文**。拼法对上了参考那 44 个 id；
  `context` 非空时 system 段会多一段编码，这条没有参考。

## 检索

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | 读参考的提示拼法 | `code/voice-worker/.venv/.../mlx_qwen3_asr/tokenizer.py` | `build_prompt_tokens`：system/user/assistant 三段 + 音频占位 + `language {lang}<asr_text>` |
| 2 | 核对提示模板的每一段 | 同文件 `Tokenizer.__init__` | `system\n`、`user\n`、`assistant\n` 各自预编码；`IM_END_ID = 151645` |
| 3 | 读参考的生成循环 | 同目录 `generate.py` | 预填充一次，`for step in 1..max_new`：EOS 检查 → `model.step` → argmax → append |
| 4 | 读参考的单步位置 | 同文件 `_build_decode_positions` | 位置从 `seq_len` 开始，三个轴相同，形状 `(B, 3, T)` |
| 5 | 读参考的缓存 | 同目录 `decoder.py` `KVCache.update` | 无 `max_seq_len` 时沿 axis=2 拼接；有预分配时原地写 |
| 6 | 读参考的遮罩分支 | 同文件 `__call__` | `cache.offset > 0 && L == 1 → attention_mask = None`；否则全因果 mask |
| 7 | 读参考的 `prefill` / `step` | 同目录 `model.py` | `prefill` 注入音频特征后只取最后一个位置过 `lm_head`；`step` 单 token |
| 8 | 读参考的 EOS 集合 | 同目录 `generate.py` `GenerationConfig` | `eos_token_ids = [151643, 151645]` |
| 9 | 核对模型目录的特殊 token | 模型目录 `tokenizer_config.json` | `151704 = <asr_text>`、`eos_token = <|im_end|>` |
| 10 | 核对提示 id 的含义 | 模型目录 `vocab.json` + `added_tokens_decoder` | `8948 system`、`872 user`、`77091 assistant`、`11528 language`、`8453 " Chinese"` |
| 11 | 核对参考资产 | `code/voice-oracle/assets/manifest.json` | `greedy.tokens = [106287, 1773, 151645]`、`text_from_tokens = "嗯。<|im_end|>"`、`n_audio_tokens = 26` |
| 12 | 写整条路径 | `voice-mlx/src/backend/transcribe.rs` | 梅尔 → 编码器 → 提示 → 贪心 → 分词器，音频 token 数取编码器行数 |
| 13 | 写逐步解码 | `voice-mlx/src/backend/text_decoder.rs` | 28 层 KV 缓存、位置续接、单步无遮罩、停止符与上限 |
| 14 | 写 token 闸 | `voice-parity/src/lib.rs` | `compare_tokens`：逐位比、长度不等不算过、报告带两侧序列 |
| 15 | 跑端到端 | `cargo test -p voice-mlx --features metal ... --test end_to_end_parity` | 4 条全过，序列与文本一致 |
| 16 | 跑 C ABI | `/tmp/voice-abi/e2e`（安装包模型目录 + `mlx.metallib`） | `"嗯。"`、空会话 `""`、`last_error` 0 |
| 17 | 跑全工作区与 metal 构建 | `cargo test --workspace --offline`、`cargo build -p voice-ffi --features metal` | 44 通过 4 忽略；构建 exit 0 |

<!-- research-complete -->
