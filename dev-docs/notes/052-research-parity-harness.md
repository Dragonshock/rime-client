# 052-research-parity-harness：逐层对照箱子已能跑，梅尔对上，其余四层仍是空入口

测量日：2026-11-14。对象是新增的 `code/voice-engine/voice-parity`。
参考资产由 `code/voice-oracle/dump_reference.py` 导出（见 051），参考实现是
`code/voice-worker/.venv` 里的 `mlx_qwen3_asr` 0.4.4。参考只作对照标准，不进安装包，
也没有任何产品代码路径读它。

## 0. 结论速览

- 新增 `voice-parity` 箱子：纯 std 读 `code/voice-oracle/assets` 的 `.bin` 与 `manifest.json`，比较后给出最大绝对差、首个超差下标与两侧数值。没有接 mlx-rs 的测试钩子，也没有引入序列化依赖（manifest 用一个两百行的递归下降解析器读）。
- 梅尔前端（`voice-mel`）对 `probe_pcm.bin` 的输出与参考 `mel.bin`（128×200）**实测最大绝对差 3.2138824e-4**，出现在下标 17189（频点 85、帧 189）：实际 `-8.3754385e-1`，参考 `-8.3786523e-1`。全部元素在 1e-3 容差内，无超差。
- 编码器、解码器、logits、token 序列四个入口各留一个 `#[ignore]` 测试。四个都断言「尚未实现」并打印参考形状，打开时直接看到还缺什么；它们现在**是失败的**，失败信息就是当前差值。
- `cargo test -p voice-parity --offline` 通过（2 项单测 + 3 项对照测试，4 项忽略）；`cargo test -p voice-parity --offline -- --ignored` 4 项全部失败，每项写明差在哪。全工作区 `cargo test --workspace --offline` 仍通过。

## 1. 箱子长什么样

`code/voice-engine/voice-parity`：

| 文件 | 作用 |
| --- | --- |
| `src/json.rs` | 只读 manifest 用的极小 JSON 解析器，含 `\u` 转义与多字节 UTF-8；两条自测 |
| `src/lib.rs` | 资产载入（`Reference`）、`Tensor`、差值报告（`Report`）、`compare_slices` |
| `tests/parity.rs` | 梅尔真对照 + 资产自洽性 + 参考内部交叉核对 + 四个 `#[ignore]` 入口 |

依赖只有一个：`voice-mel`（纯 Rust，无张量后端）。箱子不进产品路径，`voice-ffi` 与
安装包都不引用它。

### 1.1 载入

```rust
let reference = voice_parity::Reference::load()?;          // 默认目录
let tensor = reference.tensor("encoder_proj_out");          // 参考量
let report = reference.compare("mel", &actual, 1e-3);       // 比较
println!("{report}");
```

资产目录默认是 `../../voice-oracle/assets`（相对箱子，即 `code/voice-oracle/assets`），
环境变量 `VOICE_ORACLE_ASSETS` 可覆盖。载入时按 `tensors[].file` 读裸小端 f32，
同时校验字节数是 4 的倍数。

### 1.2 报告里有什么

`Report` 的字段：参考形状、参考元素数、实际元素数、长度差额、最大绝对差及其下标与两侧数值、
首个超差（下标 + 实际 + 参考 + 差 + 容差）、超差元素个数。`Display` 直接打成中文四行：

```text
对照量：mel（参考形状 (128, 200)）
元素数：参考 25600，实际 25600
最大绝对差：3.2138824e-4（下标 17189：实际 -8.3754385e-1，参考 -8.3786523e-1）
首个超差：无（全部在容差 1e-3 内）
```

`within_tolerance()` 只在「长度相等且有可比元素、无超差」时为真——长度不等不吞掉。
比较本身不做任何改判：超差就是超差，断言与否由调用方决定。

### 1.3 另两条测试守的是「资产没被改歪」

- `assets_match_manifest`：六个键与 051 记的一致；每件张量的形状乘积等于落盘元素数；dtype 是 `float32`；字节数是元素数 4 倍；无 NaN/Inf；直接读文件和按 manifest 读得到同一份 `mel`。
- `reference_greedy_tokens_are_consistent`：`greedy_tokens_3.bin` 与 `manifest.greedy.tokens` 都是 `[106287, 1773, 151645]`；提示 44 个 token；采样率 16000。

这两条不涉及 Rust 实现，只保证对照用的口径没变。

## 2. 梅尔：实测差值

| 项 | 值 |
| --- | --- |
| 输入 | `probe_pcm.bin`，32000 个 i16 采样（2 秒，`(i * 37) % 2001 - 1000`） |
| 参考 | `mel.bin`，`(128, 200)`，f32 |
| 实际 | `MelFrontend::log_mel(&pcm)`，`(128, 200)`，f32 |
| 容差 | 1e-3 |
| **实测最大绝对差** | **3.2138824e-4** |
| 最大差位置 | 下标 17189 = 频点 85、帧 189；实际 `-0.83754385`，参考 `-0.83786523` |
| 超差元素 | 0 |

容差沿用 050 的 1e-3：参考走 FFT，`voice-mel` 走直接 DFT（f64 累加），求和顺序不同，
逐元素本就差一截。050 那次 128×50 的对照只记了「小于 1e-3」，没记具体最大差；这次
把实际值落下来，容差不动。

参考梅尔是编码器吃之前的 f32 值（manifest 的 `note` 写明了），所以这里不需要先做 fp16
截断。要改比「编码器实际吃到的梅尔」，得自己再截一次 fp16。

## 3. 四个待实现的入口

四项都在 `tests/parity.rs` 里标了 `#[ignore]`，打开时会先打印参考形状，再以
`unimplemented!` 失败并写明差在哪。下面「当前差值」一栏**不是小于某个数**，而是
「没有可比的输出」——没有实际前向，就没有差值可算，如实记成无法计算。

### 3.1 编码器 `encoder_matches_reference`

要对比三件参考量（`voice-mel` 之后的整条音频塔）：

| 参考键 | 形状 | 参考来源 | 口径 |
| --- | --- | --- | --- |
| `conv_stem_out` | `(16, 13, 480)` | `AudioEncoder._apply_conv_stem` | 首次调用的第 0 块，NHWC；送进 `conv_out` 前参考 transpose 成 `(13, 480*16)`；整次调用形状是 `[2, 16, 13, 480]` |
| `encoder_layer0_out` | `(1, 26, 896)` | `AudioEncoderLayer.__call__`（layers[0]） | 第 0 层含残差输出；本次只有一个注意力窗口，mask 为 None |
| `encoder_proj_out` | `(26, 1024)` | `_encode_single` 末尾 `ln_post`/`gelu(proj1)`/`proj2` | 注入文本嵌入的那份音频特征 |

当前差值：**无从计算**。`voice-asr` / `voice-mlx` 的编码器仍是形状骨架，没有
conv_stem、layer0、proj 的实际输出可以喂进 `compare`。

### 3.2 解码器 `decoder_matches_reference`

要对比的是文本塔的逐层前向。**参考资产里没有解码器层输出**——051 那次导出只抓了卷积茎、
编码器第 0 层、编码器末尾、预填充 logits 与贪心 token。所以这个入口眼下只有两处间接量：
`prefill_logits_64`（最后位置 logits 前 64 维）与 `greedy_tokens_3`（解码闭环的结果）。
要做逐层对照，得先往 `dump_reference.py` 里补一份解码器层输出资产；那会改导出脚本，
不在本箱子范围内。

当前差值：**无从计算**，且缺资产。`voice-asr` 的 `decode_prefill` 仍是全零提示 id、
乘法 RoPE（不是 `rotate_half`）、无因果遮罩的形状骨架。

### 3.3 logits `logits_match_reference`

要对比 `prefill_logits_64`：`(64,)`，`Qwen3ASRModel.prefill` → `lm_head[:, -1:, :64]`。
参考前 8 维是 `[19.703125, 17.203125, 20.078125, 17.09375, 15.6171875, 13.515625, 16.1875, 17.46875]`。
全词表 151936 维只落了 64 维，因此这项只能比这 64 维的数值，**不能**用来判断下一个
token 是哪个（第一个 token 106287 的下标在 64 之外）。

当前差值：**无从计算**。`voice-mlx` 还没有走完 28 层文本塔，`lm_head` 未被调用。

### 3.4 token 序列 `greedy_tokens_match_reference`

要对比 `greedy_tokens_3`：`(3,)`，`[106287, 1773, 151645]`，用 f32 存整数。
参考 `generate_with_info` 会剥掉末尾 EOS（151645），回 `[106287, 1773]`；这里按资产
原样比三位。解码文本是 `"嗯。<|im_end|>"`。

当前差值：**无从计算**——解码循环尚未实现，实际 token 序列长度 0，与参考差 3 个。
入口里的报告已经把这条长度差打出来了（`元素数：参考 3，实际 0（差 -3）`）。

## 4. 两条命令的实测输出

```text
$ cargo test -p voice-parity --offline
running 2 tests            # src/lib.rs：json 解析
test result: ok. 2 passed
running 7 tests            # tests/parity.rs
test assets_match_manifest ... ok
test reference_greedy_tokens_are_consistent ... ok
test mel_matches_reference ... ok
test decoder_matches_reference ... ignored
test encoder_matches_reference ... ignored
test greedy_tokens_match_reference ... ignored
test logits_match_reference ... ignored
test result: ok. 3 passed; 0 failed; 4 ignored
```

```text
$ cargo test -p voice-parity --offline -- --ignored
running 4 tests
test encoder_matches_reference ... FAILED
test decoder_matches_reference ... FAILED
test logits_match_reference ... FAILED
test greedy_tokens_match_reference ... FAILED
test result: FAILED. 0 passed; 4 failed
```

每项的失败信息（即「差在哪」）：

| 入口 | 失败信息 |
| --- | --- |
| 编码器 | `not implemented: 编码器前向尚未实现：先接 conv_stem，再逐层到 proj2` |
| 解码器 | `not implemented: 解码器逐层前向尚未实现，且缺少可对照的解码器层资产：先补参考层的输出再谈逐层差值` |
| logits | `not implemented: 预填充 logits 尚未实现：先做完整提示前向，再取最后位置的 64 维` |
| token 序列 | `not implemented: 贪心解码尚未实现：token 序列为空，与参考的 3 个 token 对不上` |

`--ignored` 这条命令按预期以非零码退出：四个入口确实还没实现，不是被跳过。

全工作区 `cargo test --workspace --offline` 通过（`voice-asr` 9、`voice-ffi` 13、
`voice-mel` 3、`voice-parity` 5、`voice-tensor` 6、`voice-tokenizer` 8，其余 0）。

## 5. 边界

- 资产与 Python 参考都不进安装包；`voice-parity` 不在产品路径上，产品 crate 不依赖它。
- 没有改变 `dev-docs/refs`，没有改 `code/voice-worker`，也没有改任何产品前向。
- 梅尔这一层已经对上，不代表编码器以后的层对上。四个 `#[ignore]` 入口打开的顺序会先撞上
  「后端还没有把权重送进各层」这一条，而不是「数值差多少」。
- 对照箱子只报差值，不替调用方决定容差。后续重写时若要设容差，应把依据写在提交里，
  不把超差改成通过。

## 检索

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | 读资产清单 | `code/voice-oracle/assets/manifest.json` | 六个张量键、形状、dtype、来源函数名 |
| 2 | 读 050 的梅尔容差依据 | `dev-docs/notes/050-research-mel-and-tokenizer-parity.md` | 参考 FFT vs 直接 DFT，容差 1e-3 |
| 3 | 读 051 的导出口径 | `dev-docs/notes/051-research-oracle-dump.md` | 梅尔存 f32、卷积茎只落第 0 块、logits 只落 64 维 |
| 4 | 读 `voice-mel` 接口 | `voice-mel/src/lib.rs` | `MelFrontend::log_mel(&[i16]) -> Vec<f32>`，行优先 |
| 5 | 写对照箱子 | `voice-parity/src/lib.rs`、`src/json.rs` | 纯 std 载入与比较 |
| 6 | 写对照测试 | `voice-parity/tests/parity.rs` | 梅尔实测 3.2138824e-4；四入口 ignore |
| 7 | 跑 `--offline` | `cargo test -p voice-parity --offline` | 5 通过 / 4 忽略 |
| 8 | 跑 `--offline -- --ignored` | 同上加 `--ignored` | 4 失败，均写明尚未实现 |
| 9 | 跑全工作区 | `cargo test --workspace --offline` | 全部通过 |

<!-- research-complete -->
