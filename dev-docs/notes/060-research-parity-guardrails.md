# 060-research-parity-guardrails：容差口径收到一处，解码器主闸带两条归因断言

测量日：2026-11-14。对象是 `code/voice-engine/voice-parity`（新增 `src/tolerance.rs`）、
`voice-parity/tests/parity.rs`、`voice-mlx/tests/encoder_parity.rs` 与
`voice-mlx/tests/decoder_parity.rs`。探针仍是 `code/voice-oracle/assets/probe_pcm.bin`
（2.00 秒、32000 样本），模型目录与 metallib 取自已安装包
`~/Library/Input Methods/RimeClient.app/Contents/Resources`（`Models/Qwen3-ASR-0.6B-4bit`
与 `mlx.metallib`）。三套对照都打开 metal 跑，`--test-threads=1`。

这一步只动「容差怎么算」和「主闸凭什么说自己对上了」，**没有改一个算术**，所以下面
每一个实测差值都与改前逐位相同。数字没变好，就是没变好；这一步买到的是口径不再散落
与两条能证伪的归因断言，不是余量。

## 0. 结论速览

| 项 | 改前 | 改后 | 说明 |
| --- | ---: | ---: | --- |
| 容差公式的定义处 | 2 个测试文件各写一遍（`F16_UNIT` 重复两份、6 个倍数常量） | **1 处**：`voice-parity/src/tolerance.rs` | 编码器、解码器、梅尔三条闸都引用它 |
| 容差公式 | 上界 × `1.0/2048.0` × 倍数（口径对，但只写在注释里） | **同一个式子，写成一个函数** `f16_tolerance(上界, 倍数)` | 依据（参考是 f16、`2^-11` 的来历、倍数怎么取）写成模块头注释 |
| 解码器主闸的归因断言 | 0 条（只在旁边那条测试里量） | **2 条**：RoPE 只乘 sin 必须更差、去掉每头 q/k 归一化必须更差 | 两条都另加「必须差到容差外」，否则归因与主闸各说各话 |
| `mel` 容差 | 1e-3，写在 `voice-parity/tests/parity.rs` | 同一个数，挪进 `tolerance::MEL_TOLERANCE` | 它不走 f16 折算（参考 FFT 对直接 DFT），注释写明 |
| 解码器主闸首 64 维最大差 | 7.8125e-2 | **7.8125e-2** | 容差 8.203125e-2，余量 1.05 倍 |
| `encoder_proj_out` 最大差 | 1.1855885e-3 | **1.1855885e-3** | 容差 1.9779205e-3，余量 1.67 倍 |
| token 序列 / 文本 | `[106287, 1773, 151645]` / `嗯。` | 同左 | 逐位相同 |
| `cargo test --workspace --offline` | 44 通过、4 忽略、0 失败 | **49 通过、4 忽略、0 失败** | 多出的 5 条是容差模块自己的单元测试 |
| 打开 metal 的三套对照 | 13 通过、0 失败 | **13 通过、0 失败** | 编码器 3、解码器 6、端到端 4 |

## 1. 容差口径怎么写的

全仓只有一处定义，`code/voice-engine/voice-parity/src/tolerance.rs`：

```text
容差 = 参考张量的绝对值上界 × 2^-11 × 允许的单位数
```

三段的依据直接写在该文件的模块头与每个常量旁边：

1. **2^-11 的来历**：参考是 f16（`manifest.json` 的 `model_dtype`，
   `dump_reference.py` 落盘前 `astype(float16)`）。f16 10 位尾数，`[2^e, 2^(e+1))` 内
   相邻值间距 `2^(e-10)`，对量级 `m` 的元素间距落在 `[m·2^-10, m·2^-9)`；取间距的一半
   作为一次存储的舍入上界，就是 `m·2^-11`（4.8828125e-4）。所以一个「单位」是
   「元素按自身量级折算的半个 f16 间距」的上界。
2. **为什么尺度取参考张量的绝对值上界**：元素量级落在 `[上界/2, 上界]`，用上界折算
   至多放宽不到 2 倍；逐元素折算则没法用一个数在报告里说清「差了几层」。
3. **倍数按层数与实测取整值**，不是「层数就是倍数」：残差是加法，舍入有正有负，
   不线性累加；取多少、实测多少写在每个常量的注释里。

接口一共四个，测试只调这四个，不再自己乘：

| 接口 | 作用 |
| --- | --- |
| `F16_UNIT` | `2^-11`，唯一定义 |
| `f16_tolerance(上界, 倍数)` | 算容差 |
| `f16_units(上界, 差)` | 把实测差折成单位数（`f16_tolerance` 的逆） |
| `Margin::new(上界, 允许倍数, 实测差)` | 打印与断言共用的一笔账：差值、容差、折成几个单位、余量几倍、是否在容差内 |

各闸的倍数常量与实测（括号里是本次实测折出来的单位数）：

| 常量 | 值 | 依据 |
| --- | ---: | --- |
| `CONV_STEM_UNITS` | 4 | 三层卷积 + 三次 GELU，实测 0（0.0 个单位） |
| `ENCODER_LAYER0_UNITS` | 4 | 一次双向注意力 + 两层前馈，实测 1.2 个单位 |
| `ENCODER_PROJ_UNITS` | 32 | 18 层全走完，实测 19.2 个单位；这一闸余量只有 1.67 倍 |
| `PREFILL_LOGITS_UNITS` | 8 | 28 层文本塔 + 末尾 RMSNorm + `lm_head` 1024 项点积，实测 7.6 / 6.1 个单位 |
| `MEL_TOLERANCE` | 1e-3（绝对） | 参考 FFT 对直接 DFT，与 f16 存储无关；实测 3.2138824e-4，余量 3.11 倍 |

两条硬约束也写在模块头：不为了让失败变成功去放大倍数；不把不同闸合成一个容差。
本次没有任何容差被放宽——`PREFILL_LOGITS_UNITS` 仍是 8，与 054/058 记的 8.203125e-2
同一个数。

## 2. 改了什么

| 位置 | 改动 |
| --- | --- |
| `voice-parity/src/tolerance.rs` | 新增。公式、依据、四个接口、五个闸常量、5 条单元测试（`F16_UNIT` 的十进制值、上界×单位×倍数、折单位与算容差互逆、余量口径、常量取值） |
| `voice-parity/src/lib.rs` | `pub mod tolerance;` 与 `f16_tolerance` / `f16_units` / `Margin` / `F16_UNIT` 的再导出；模块头补一句「容差只在一处」 |
| `voice-parity/tests/parity.rs` | 删掉本地的 `MEL_TOLERANCE`，改用 `tolerance::MEL_TOLERANCE`；梅尔那条闸多打印一行「实测差 / 容差 / 余量」 |
| `voice-mlx/tests/encoder_parity.rs` | 删掉本地的 `F16_UNIT` 与三个倍数常量，改用 `Margin` + 三个闸常量；上界、差值、容差、余量一次打印；断言与打印同源（`assert_eq!(report.max_abs_diff, Some(margin.diff))`） |
| `voice-mlx/tests/decoder_parity.rs` | 同上改用 `Margin` + `PREFILL_LOGITS_UNITS`；`compare_logits` 返回 `Margin`；**主闸加两条归因断言**；旁边那条归因测试也补「必须差到容差外」 |

主闸新增的两条断言（`decoder_prefill_matches_reference`，用本仓编码器输出注入）：

- `RoPE 只乘 sin`（`RopeMode::SinOnly`，少了 `rotate_half`）**必须**让首 64 维最大差
  变大，并且大到容差外；
- `去掉每头 q/k 归一化`（`DecoderOptions { qk_norm: false }`）同上。

方向是「换成错的必须更差」，不是「换口径让失败变成功」。原来那条
`wrong_decoder_options_make_the_difference_bigger`（注入参考 `encoder_proj_out`，把编码器
的差排除在外）保留，两处各自独立。

## 3. 当前各闸实测

### 3.1 编码器三条

```
  conv_stem_out       上界 2.0253906e0，实测差 0e0（0.0 个单位），容差 3.955841e-3（4 个单位），余量 无穷（差为 0）
  encoder_layer0_out  上界 6.6328125e0，实测差 3.90625e-3（1.2 个单位），容差 1.2954712e-2（4 个单位），余量 3.32 倍
  encoder_proj_out    上界 1.2658691e-1，实测差 1.1855885e-3（19.2 个单位），容差 1.9779205e-3（32 个单位），余量 1.67 倍
```

三项都在容差内，超差元素 0 个。与 053 记的差值一致（3.90625e-3 / 1.1855885e-3）。
`encoder_proj_out` 的实测已经吃掉 32 个单位里的 19.2 个，是编码器侧余量最薄的一条。

### 3.2 解码器主闸

| 量 | 实测差 | 折成单位 | 容差（倍数） | 余量 | argmax |
| --- | ---: | ---: | ---: | ---: | ---: |
| 主闸（本仓编码器输出注入） | **7.8125e-2** | 7.6 | 8.203125e-2（8） | **1.05 倍** | 106287 |
| 注入参考 `encoder_proj_out` | **6.25e-2** | 6.1 | 8.203125e-2（8） | 1.31 倍 | 106287 |

参考绝对值上界 21（`prefill_logits_64` 的最大绝对值），与 058 记的同一档。主闸余量
1.05 倍仍是全仓最紧的一条：差值 7.8125e-2 距容差 8.203125e-2 只差 3.90625e-3
（0.38 个单位）。这条数字本次没有变化，也没有为了让它好看去动倍数。

### 3.3 两条归因断言实测

主闸里跑（同一份提示、同一份本仓编码器输出、只换 RoPE 或 q/k 归一化）：

| 口径 | 首 64 维最大差 | 折成单位 | argmax |
| --- | ---: | ---: | ---: |
| 默认（`rotate_half` + 每头 q/k 归一化） | 7.8125e-2 | 7.6 | 106287 |
| RoPE 只乘 sin | **2.5214844e1** | 2459.0 | 102393 |
| 去掉每头 q/k 归一化 | **2.1436523e1** | 2090.6 | 4806 |

两个错口径都比默认差大 270 倍以上，也都在容差（8 个单位）外 260 倍以上，argmax 同时
从 106287 跑掉。旁边那条注入参考音频的归因测试给出同一量级：只乘 sin 2.5238281e1
（2461.3 个单位，argmax 102393）、不做归一化 2.1428223e1（2089.8 个单位，argmax 4806）。

量化分组那四口径不变，四行都在容差内且 argmax 都是 106287：

| 口径 | 首 64 维最大差 | 折成单位 |
| --- | ---: | ---: |
| 两组都走量化矩阵乘（产品口径） | 7.8125e-2 | 7.6 |
| 只走注意力（MLP 先拼 f16） | 6.25e-2 | 6.1 |
| 只走 MLP（注意力先拼 f16） | 3.125e-2 | 3.0 |
| 一组都不走（057 的算术） | 4.6875e-2 | 4.6 |

### 3.4 梅尔与端到端

- 梅尔：实测差 3.2138824e-4，容差 1e-3（绝对），余量 3.11 倍，超差元素 0。
- 提示：44 个 id 与 `manifest.prompt.tokens` 逐位相同（26 个音频占位 `151676`）。
- 贪心：`[106287, 1773, 151645]` 与 `greedy_tokens_3` 逐位相同；文本 `嗯。`；停在
  `ImEnd`；梅尔 200 帧、音频 token 26。
- 缓存单步：缓存第二步 1773，重算扩展提示的 argmax 1773。

## 4. 跑了什么

不打开 metal：

```
cd code/voice-engine
cargo test --workspace --offline
```

exit 0。**49 通过、4 忽略、0 失败**（改前 44 通过；多出的 5 条是 `tolerance` 模块的
单元测试，4 忽略仍是 `voice-parity` 里指向 MLX 实对照的入口）。`voice-mlx` 在未打开
`metal` 时没有测试。

打开 metal，三套对照一次跑完（链接参数见 `BUILD.txt`）：

```
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
export VOICE_MODEL_DIR="$HOME/Library/Input Methods/RimeClient.app/Contents/Resources/Models/Qwen3-ASR-0.6B-4bit"
export VOICE_METALLIB="$HOME/Library/Input Methods/RimeClient.app/Contents/Resources/mlx.metallib"
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test encoder_parity --test decoder_parity --test end_to_end_parity \
  -- --test-threads=1 --nocapture
```

exit 0。**13 通过、0 失败**（解码器 6、编码器 3、端到端 4），三块分别 37.35 秒、
5.28 秒、8.96 秒。端到端那条一次 `transcribe` 的墙钟 5.35 秒（含现开权重，`dev` 档案）。

每一刀之后都重跑过这套闸：先抽 `tolerance` 模块（数字逐位不变、13 条全过），再给主闸
加两条归因断言（数字仍不变、13 条全过）。没有哪一刀是靠放宽容差过的。

## 5. 没变好、也没打算在这一步变好的地方

- **一个实测差值都没动。** 这一步是口径与断言的整理，不是算术优化；主闸余量仍是
  1.05 倍，`encoder_proj_out` 仍是 1.67 倍。要提余量只能查算术（量化矩阵乘的两组
  分别量、`lm_head` 分块那一段），不能放大 `PREFILL_LOGITS_UNITS`。
- **归因断言不能替代容差。** 它们证明「换成错的会差到容差外」，不证明「默认口径
  对得上」——后者仍只由主闸的数值承担。
- **`Margin` 的余量是容差除以实测差**，实测差为 0 时打印「无穷」，不编造比值；
  它与 `Report::within_tolerance` 同一口径（差等于容差算过）。
- 产品路径、`code/Client`、`inject`、librime 与 `dev-docs/refs` 都没动，Python 没有
  进产品路径。所有秒数都是 `dev` 档案。

<!-- research-complete -->
