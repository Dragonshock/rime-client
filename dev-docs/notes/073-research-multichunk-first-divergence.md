# 073-research-multichunk-first-divergence：300 / 400 帧上的逐层对照，与第一处偏离在哪

测量日：2026-11-15。参考实现是 `code/voice-worker/.venv` 里的 `mlx_qwen3_asr 0.4.4`（只作对照标准）。
参考量是 072 导出的 `code/voice-oracle/assets/multichunk/`：`s1_3.0`（300 帧 / 39 个音频 token）、
`s1_4.0`（400 帧 / 52 个音频 token），两段都是整块，没有尾块。

新增的对照是 `code/voice-engine/voice-mlx/tests/multichunk_divergence.rs`（三条，`#![cfg(feature = "metal")]`）。
产品代码一个算子没改：梅尔、卷积茎、位置表、注意力、投影、提示、解码循环、流式路径都是 069 / 070 的形态。

## 1. 结论

**编码器没有被截，逐层都没有第一处偏离。** 300 帧给 39 个音频 token、400 帧给 52 个，
与参考逐个相同；两段的差值与 200 帧那条闸同一档，没有一个元素超差。

「3 个整块起只输出前 2 块文本」不是引擎的行为，是**量具**的行为：
`/tmp/voice-abi/bench7` 其实是 `bench3.c` 编出来的（那份源码自己的注释是「等外部把
`weights.safetensors` 移走，再跑两次定稿」），它只读前 **32000 个采样**：

```c
int16_t pcm[32000];
size_t n = fread(pcm, sizeof(int16_t), 32000, f);   // 后面的音频一律不读
```

32000 个采样 = 2.000 s = 200 个梅尔帧 = 2 个整块 = **26 个音频 token**。
判据不是猜的，三条都能当场复验：

1. `bench7` 每次运行都固定多花 6 秒（`real 7.32` 对 `bench7-clean` 的 `real 1.08`）——
   `bench3.c` 里那句 `sleep(6)`；`bench7` 的二进制里也留着 `/tmp/voice-abi/booted.flag` 这个字符串。
2. `bench3.c` 的 `main` 只看 `argv[1..3]`，**没有第四个参数**。071 记的「一次喂完与 4096 一块结果相同」
   之所以相同，是因为块长那个参数从头到尾没被读过，两次跑的是同一条路径。
3. 把整段 PCM 真正喂进去的驱动（`bench.c` 编出的 `bench7-clean`、`bench6.c` 编出的
   `bench6-step`，后者链接的是 07:54 那份旧归档）在同一批音频上都给出完整句子。

所以「26 个 token」是探针只喂了 200 帧的结果，不是编码输出被截。

## 2. 逐层差值表

口径与三条老闸一致：容差 = 参考张量的绝对值上界 × 2^-11 × 允许单位数
（`voice_parity::tolerance`，卷积茎 4、第 0 层 4、投影 32），梅尔用绝对容差 `1e-3`。
编码器那几行喂的是**参考的 mel**（与 053 的闸同一条做法），把梅尔那一点算术差留在表外；
表里「首个超差下标」一列全是「无」，即没有任何元素超出容差。

### 300 帧（`s1_3.0`，3 个整块）

| 层 | 元素数 | max\|d\| | @下标 | 容差 | 折成 f16 单位 | 首个超差下标 | 超差数 |
| --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| 梅尔 `mel`（本仓整段梅尔 vs 参考） | 38400 | 1.629591e-4 | 22326 | 1e-3 | — | 无 | 0 |
| 卷积茎批量输出 `conv_stem_out[0]` | 99840 | 0 | 0 | 8.102417e-3 | 0.0 | 无 | 0 |
| 第 0 块（单独编）`conv_stem_out[0]` | 99840 | 0 | 0 | 7.7552795e-3 | 0.0 | 无 | 0 |
| 第 1 块（单独编）`conv_stem_out[1]` | 99840 | 0 | 0 | 7.411957e-3 | 0.0 | 无 | 0 |
| 第 2 块（单独编）`conv_stem_out[2]` | 99840 | 0 | 0 | 7.484436e-3 | 0.0 | 无 | 0 |
| 第 0 层 `encoder_layer0_out` | 34944 | 1.953125e-3 | 15486 | 1.8432617e-2 | 0.4 | 无 | 0 |
| 第 0 层 token 0..13 | 11648 | 9.765625e-4 | 2726 | 1.8432617e-2 | 0.2 | 无 | 0 |
| 第 0 层 token 13..26 | 11648 | 1.953125e-3 | 3838 | 1.8432617e-2 | 0.4 | 无 | 0 |
| 第 0 层 token 26..39 | 11648 | 1.953125e-3 | 3344 | 1.8432617e-2 | 0.4 | 无 | 0 |
| 投影 `encoder_proj_out` | 39936 | 2.193749e-4 | 28871 | 1.6241074e-3 | 4.3 | 无 | 0 |
| 投影 token 0..13 | 13312 | 1.3360381e-4 | 5318 | 1.6241074e-3 | 2.6 | 无 | 0 |
| 投影 token 13..26 | 13312 | 1.5078485e-4 | 12524 | 1.6241074e-3 | 2.9 | 无 | 0 |
| 投影 token 26..39 | 13312 | 2.193749e-4 | 2247 | 1.6241074e-3 | 4.3 | 无 | 0 |

音频 token 数：本仓 **39**，参考 **39**，差 **0**。
整条 `transcribe`（本仓自己的梅尔，不经参考）：梅尔 300 帧、音频 token 39、提示 57 个，
tokens `[100644, 102172, 112055, 113716, 37945, 104154, 1773, 151645]`，
文本 **今天下午三点开会，请提前。**，停在 `<|im_end|>`——与参考逐字相同。

### 400 帧（`s1_4.0`，4 个整块）

| 层 | 元素数 | max\|d\| | @下标 | 容差 | 折成 f16 单位 | 首个超差下标 | 超差数 |
| --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| 梅尔 `mel` | 51200 | 1.629591e-4 | 29726 | 1e-3 | — | 无 | 0 |
| 卷积茎批量输出 `conv_stem_out[0]` | 99840 | 0 | 0 | 8.102417e-3 | 0.0 | 无 | 0 |
| 第 0 块（单独编）`conv_stem_out[0]` | 99840 | 0 | 0 | 7.7552795e-3 | 0.0 | 无 | 0 |
| 第 1 块（单独编）`conv_stem_out[1]` | 99840 | 0 | 0 | 7.411957e-3 | 0.0 | 无 | 0 |
| 第 2 块（单独编）`conv_stem_out[2]` | 99840 | 0 | 0 | 7.484436e-3 | 0.0 | 无 | 0 |
| 第 3 块（单独编）`conv_stem_out[3]` | 99840 | 0 | 0 | 8.102417e-3 | 0.0 | 无 | 0 |
| 第 0 层 `encoder_layer0_out` | 46592 | 1.953125e-3 | 21456 | 1.8447876e-2 | 0.4 | 无 | 0 |
| 第 0 层 token 0..13 | 11648 | 9.765625e-4 | 1017 | 1.8447876e-2 | 0.2 | 无 | 0 |
| 第 0 层 token 13..26 | 11648 | 1.953125e-3 | 9808 | 1.8447876e-2 | 0.4 | 无 | 0 |
| 第 0 层 token 26..39 | 11648 | 1.953125e-3 | 6796 | 1.8447876e-2 | 0.4 | 无 | 0 |
| 第 0 层 token 39..52 | 11648 | 9.765625e-4 | 940 | 1.8447876e-2 | 0.2 | 无 | 0 |
| 投影 `encoder_proj_out` | 53248 | 3.8785487e-4 | 28817 | 1.7175674e-3 | 7.2 | 无 | 0 |
| 投影 token 0..13 | 13312 | 1.3000146e-4 | 7382 | 1.7175674e-3 | 2.4 | 无 | 0 |
| 投影 token 13..26 | 13312 | 1.932364e-4 | 13169 | 1.7175674e-3 | 3.6 | 无 | 0 |
| 投影 token 26..39 | 13312 | 3.8785487e-4 | 2193 | 1.7175674e-3 | 7.2 | 无 | 0 |
| 投影 token 39..52 | 13312 | 1.9018352e-4 | 7227 | 1.7175674e-3 | 3.5 | 无 | 0 |

音频 token 数：本仓 **52**，参考 **52**，差 **0**。
整条 `transcribe`：梅尔 400 帧、音频 token 52、提示 70 个，
文本 **今天下午三点开会，请提前十分钟到会。**，停在 `<|im_end|>`——与参考逐字相同。

### 表里能读出的三件事

1. **批量多块与逐块分解一致。** 把每一块的 100 帧单独送进编码器，它的「第 0 块卷积茎」
   就是整批里的第 k 块，四块全部 `max|d| = 0`（逐位相同）。所以「批量多块卷积」这一层
   没有偏差，通道重排 `(块, 频, 时, 通道) → (块·时, 通道·频)` 也没有偏差。
2. **每块从位置 0 重排的正弦表是对的。** 位置表加在 `conv_out` 之后、进注意力之前，
   它的任何偏差都会在同一块的 13 行里直接显形。300 帧的 token 26..39 与 400 帧的
   token 39..52（最后一块，就是「第 3 / 第 4 块」的位置）与其它块同一档，没有一块跳出来。
   窗只有 1 个（39 / 52 都远小于 104），`_create_windowed_mask` 返回 `None`，
   窗内注意力就是整段一次双向注意力，与 072 第 4 节记的窗口事实一致。
3. **`ln_post` / `proj1` / `proj2` 也没有偏差。** 投影上四个 13 行段的差值都在
   1.3e-4 – 3.9e-4 之间，最大的一处（3.8785487e-4，28817）落在第 3 块内部，
   折 7.2 个单位，容差 32 个单位，余量 4.43 倍；400 帧比 300 帧大一点是 18 层 f16
   舍入的累计次序不同，不是某一层错位。

## 3. 第一处偏离：在量具的 `fread`，不在引擎里

- 最小复现的那一处是**输入边界**，不是网络里的某一层：`bench3.c` 的
  `fread(pcm, 2, 32000, f)` 把 3.00 s / 4.00 s / 4.55 s / 5.09 s 的音频一律截成 2.00 s。
- 200 帧正好是 2 个整块、26 个音频 token，于是解出来的文本就是这 2 块对应的那一句：
  s1_3.0 / s1_4.0 → 今天下午三点开会。；s2 → 幫我把這段文字翻譯成。；s3 → 下周的行程已经确认了。
- 「与音频还剩多少无关」也由此成立：剩下的部分根本没进 `voice_session_feed`。
  「尾块被截」之所以被排除，是因为 3.0 / 4.0 都是整块——但两段在探针里都只剩前 200 帧，
  这个「排除」当时没有检查探针喂了多少。

四条判别实验（同一份音频、同一份模型目录、同一份 `mlx.metallib`）：

| 实验 | 驱动 | 喂进去的长度 | s1_3.0 的文本 | s1_4.0 的文本 |
| --- | --- | ---: | --- | --- |
| 1 | `bench7`（= `bench3.c`，06:58 归档） | 2.00 s（32000 采样） | 今天下午三点开会。 | 今天下午三点开会。 |
| 2 | `bench7-step`（= `bench3.c`，07:54 归档） | 2.00 s（32000 采样） | 今天下午三点开会。 | 今天下午三点开会。 |
| 3 | `bench6-step`（`bench6.c` 的 `instant`，07:54 归档） | 3.00 / 4.00 s 全部 | 今天下午三点开会，请提前。 | 今天下午三点开会，请提前十分钟到会。 |
| 4 | `bench7-clean`（`bench.c`，本次重编的归档） | 整段（按 4096 一块送） | 今天下午三点开会，请提前。 | 今天下午三点开会，请提前十分钟到会。 |

第 3 行是关键：它链接的是 07:54 那份**旧归档**，把整段 PCM 真喂进去之后，旧归档也给出完整句子。
所以缺陷不在「旧归档 / 新归档」的差别里，只在探针读了多少字节。

另有一条把这件事钉死：给 `s1.pcm` 前面加 2.00 s 静音（共 6.61 s），
第一段 2.00 s 全是静音、后面才是人声。

| 驱动 | 输入 | 输出 |
| --- | --- | --- |
| `bench7`（只读 32000） | 静音 2.00 s + s1 | 嗯。 |
| `bench7-clean`（整段 6.61 s） | 同上 | 今天下午三點開會，請提前十分鐘到會議室。 |

前者只看见静音（解出「嗯。」正是 2 秒探针 `probe_pcm.bin` 那句），后者拿到整句。
（后者给繁体是模型自己的选择，与 071 记的 s2 同一现象，不是缺陷。）

## 4. 与「只输出前 2 块文本」的因果关系

一条链，四个环节，每一环都能单独验证：

```text
bench3.c: fread(..., 32000, f)      → 只喂前 2.00 s
2.00 s × 16 kHz ÷ 160 = 200 帧      → 2 个整块（200 帧，末块整）
200 帧 → 13 token/块 × 2 = 26 token → 提示里 26 个 <|audio_pad|>（本仓 200 帧实测 26）
2 块音频的语义只覆盖「今天下午三点开会。」 → 文本停在「开会。」
```

「3 个整块起才截断」只是表象：2.1 s、2.5 s 的切片本来就只有 2 个整块 + 尾块，
探针截到 2.00 s 之后剩下的仍是 2 个整块，所以文本不变；4.61 s 的 `s1` 同样只剩前 2 块。
换句话说，071 那张表的每一行都能用「探针只喂 2.00 s」这一条解释完，不需要引擎里有第二个原因。

## 5. 三条老闸与老资产没受影响

本轮只加了一个对照文件，产品代码一字未改；200 帧的闸与 072 记的资产照旧：

| 闸 | 结果 |
| --- | --- |
| `cargo test --workspace --offline` | 全过（`voice-mlx` 的 metal 用例不在其中） |
| `--test encoder_parity` | 3 通过 |
| `--test decoder_parity` | 6 通过（主闸仍是 7.8125e-2 对容差 8.203125e-2） |
| `--test end_to_end_parity` | 4 通过 |
| `--test multichunk_divergence`（本次新增） | 3 通过 |

## 6. 复现

```text
cd /Users/dragon/repo/tools/rime-client/code/voice-engine
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"; export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
export RUSTFLAGS="-C link-arg=-framework -C link-arg=Accelerate \
  -C link-arg=-Wl,-U,_MTLTensorDomain -C link-arg=-Wl,-U,_MTLLogStateErrorDomain \
  -C link-arg=-Wl,-U,_NSDeviceCertificationiPhonePerformanceGaming \
  -C link-arg=-Wl,-U,_NSProcessInfoPerformanceProfileDidChangeNotification \
  -C link-arg=-Wl,-U,_NSProcessPerformanceProfileDefault \
  -C link-arg=-Wl,-U,_NSProcessPerformanceProfileSustained \
  -C link-arg=-Wl,-U,___fmaxf16 -C link-arg=-Wl,-U,___fminf16"

cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test multichunk_divergence -- --nocapture --test-threads=1
```

量具这边（`bench.c` 才是「整段 PCM 一次喂完」的那份源码）：

```text
cd /Users/dragon/repo/tools/rime-client/code/voice-engine
cargo build -p voice-ffi --features metal --target aarch64-apple-darwin
cd /tmp/voice-abi
xcrun clang -O2 bench.c \
  /Users/dragon/repo/tools/rime-client/code/voice-engine/target/aarch64-apple-darwin/debug/libvoice_ffi.a \
  -liconv -lc++ -lobjc -framework Foundation -framework Metal -framework Accelerate \
  -lSystem -lc -lm -o bench7-clean
RES="$HOME/Library/Input Methods/RimeClient.app/Contents/Resources"
./bench7-clean "$RES/Models/Qwen3-ASR-0.6B-4bit" "$RES/mlx.metallib" \
  /Users/dragon/repo/tools/rime-client/code/voice-oracle/assets/zh/s1_4.0.pcm 4096
```

## 7. 边界

- 本节量的是「整段 PCM 一次喂完 / 按 4096 一块送」这一条路。说话中的 `partial` 走
  `voice-mlx::stream`，本轮没有改它，也没有用它来解释缺陷。
- 安装包里的 `Squirrel` 没有重装、没有重打包：本轮不改产品代码，装包那次的数量在 067 / 070。
  要判断「装好的包是否也截断」，得在这台机器上按一次真实按键；`bench6-step`
  （同一份 07:54 归档、整段喂）给的是完整句子，说明归档这一侧没有问题。
- 071 那张表里的「本仓」一列来自 `bench7` 的输出，本轮把它的来处查清了：
  它量的是 2.00 s 的前缀，不是整段。那张表的参考一列仍然有效。

<!-- research-complete -->