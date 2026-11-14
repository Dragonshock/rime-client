# 058-research-quantized-weights：线性层直接吃 4 位分组权重，`lm_head` 逐块取 argmax

测量日：2026-11-14。对象是 `code/voice-engine/voice-mlx` 的 `backend/weights.rs`、
`backend.rs` 的 `linear` 与 `backend/text_decoder.rs` 的 28 层文本塔与 `lm_head`。
基线是 `057` 的常驻实例：同一探针 `finish` 暖档 0.466 秒、`engine_new` 4.908 秒。
探针仍是 `code/voice-oracle/assets/probe_pcm.bin`（2.00 秒、32000 样本），模型与
metallib 取自已安装包
`~/Library/Input Methods/RimeClient.app/Contents/Resources`。计时程序是
`/tmp/voice-abi/bench`（单次）与 `bench2`（同进程三次），两者都对
`libvoice_ffi.a` 直接链接，口径与 056/057 相同。编译是 `dev` 档案（与产品归档同档），
下面所有秒数都带着这一点。

改动只在 `code/voice-engine`；`code/voice-ffi` 不需要改，它走的还是
`Engine::transcribe` 那一条。Python 没有进产品路径，`inject`、librime 与
`dev-docs/refs` 都没有动。

## 0. 结论速览

| 项 | 057 | 现在 | 说明 |
| --- | ---: | ---: | --- |
| `engine_new`（页缓存暖） | 4.908 s | **4.646–4.723 s** | 读盘与拼表少了一段 |
| `engine_new`（本机首读，页缓存冷） | 5.217 s | **4.779 s** | 同一台机器、同一份探针的首次读盘 |
| `finish` 第一次（页缓存暖） | 0.807 s | **0.424–0.486 s** | 启动后第一次定稿 |
| `finish` 第一次（页缓存冷） | 1.065 s | **0.811 s** | 首次读盘那一档 |
| `finish` 第二次及以后（暖） | 0.466 s | **0.405–0.424 s** | 同进程再按一次 |
| 冷起整条路（new + 第一次 finish，暖页） | 5.715 s | 5.07–5.21 s | |
| token 序列 | `[106287, 1773, 151645]` | `[106287, 1773, 151645]` | 逐位相同 |
| 文本 | `嗯。` | `嗯。` | 相同 |
| 峰值内存 footprint | 2.92 GB | **1.63 GB** | 常驻的 f16 整表不再出现 |
| 峰值 resident set size | 323 MB | 634 MB | 打包字节在装载时同时占着主机缓冲与设备副本 |

057 的两个数取自 057 的实测记录；「本机首读」那一对是本次在改动前、改动后各跑
一次同一个 `bench`，条件一致，用来把读盘那一档单独拿出来看。当前口径的逐项数在
下面第 3 节。

## 1. 改了什么

一句话：分组权重从现在起**原样**留在设备上（打包值 + 缩放 + 偏置三块），前向直接走
MLX 的量化矩阵乘，`f16` 的整层稠密表从不产生。

| 位置 | 改动 |
| --- | --- |
| `voice-mlx/src/backend/weights.rs` | 新增 `grouped_tensor`（读三块，校验打包宽度与组大小、缩放形状）、`affine_packed`（线性层用打包权重）、`affine_table_packed`（整表用打包权重）；删掉 057 的 `affine_table`（那一条是「按行分块反量化成整张 f16 常驻表」） |
| `voice-mlx/src/backend.rs` | `MlxTensor` 多带一块 `biases`；`linear` 见分组权重且激活是 f16 时走 `ops::quantized_matmul`（`transpose: true`、affine、组大小与位数取 `Storage::Grouped`），其余情况退回 `material_weight`（先还原再乘，数值同一条）；`take_rows` 对分组权重按同样下标取三块，取出后仍是分组权重；新增 `materialize`，`to_f32` 遇到分组权重先还原那一小块 |
| `voice-mlx/src/backend/text_decoder.rs` | `Layer::load` 七个投影改用 `affine_packed`；词嵌入表（也就是 `lm_head`）常驻打包三块，不再拼 311 MB 的 f16 表；`head_logits` 逐块取打包行做量化矩阵乘再取 argmax；新增 `QuantPolicy`（注意力、MLP 两组开关）与 `TextDecoder::open_with` |
| `voice-mlx/tests/decoder_parity.rs` | 新增一条归因闸 `quantized_paths_are_compared_group_by_group`：四个口径逐个对同一份参考资产求差 |
| `voice-mlx/tests/phase_timing.rs` | 新增一条 `#[ignore]` 的分段计时（打开、梅尔、编码器、预填充、贪心各多少），顺带断言 token 序列 |

改到的层，逐个数：

1. **28 层文本塔的 7 个投影 × 28 层 = 196 个线性层**，权重是 4 位、组 64
   （`quantization_config.json` 的 `bits: 4`、`group_size: 64`）。现在走
   `quantized_matmul`，`transpose: true` 是因为文件里的打包权重按 `(输出, 输入)` 排，
   与 `Linear` 的逻辑形状一致。
2. **词嵌入 / `lm_head`**：151936×1024 的同一张表（`tie_word_embeddings`）。原来是
   按 8192 行分块反量化、拼成 311 MB 的 f16 常驻表，再按行取；现在常驻打包值
   （78 MB）、缩放与偏置（各 5 MB），取行时只还原那几十行。
3. **`lm_head` 的 argmax**：原来从 f16 常驻表里取 8192 行的子表再乘；现在取的是
   `LM_HEAD_BLOCK` 行的打包三块（32768 行 = 16 MB 打包值），乘完取这一块的
   argmax，整张词表从不一次取齐。
4. **未改**：音频塔的 8 位分组线性层（试过、按第 2 节的闸回退）、卷积茎、各类
   LayerNorm / RMSNorm、RoPE、注意力与 KV 缓存。

量化矩阵乘的边：`quantized_matmul` 只在激活是 f16 时走；激活不是 f16 时退回
「先还原再乘」（`material_weight`），两条路径的每个权重元素是同一组量化值。

## 2. 精度

三套对照闸都打开 metal 跑，`--test-threads=1`，模型目录与 metallib 指到已安装包：

```
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test encoder_parity --test decoder_parity --test end_to_end_parity \
  -- --test-threads=1 --nocapture
```

三条全过：解码器 6 条、编码器 3 条、端到端 4 条，13 通过、0 失败。

### 2.1 解码器主闸：过了，但余量变薄

| 量 | 057 实测 | 现在 | 054 记的容差 |
| --- | ---: | ---: | ---: |
| 预填充 logits 前 64 维最大差（本仓编码器 → 解码器） | 3.125e-2 | **7.8125e-2** | 8.203125e-2（8 个 f16 单位） |
| 折成 f16 单位 | 3.1 | **7.6** | 8 |
| 全词表 argmax | 106287 | 106287 | 106287 |
| 注入参考音频特征那一档的最大差 | 3.0 个单位 | 6.1 个单位（6.25e-2） | 8 |

token 序列仍是 `[106287, 1773, 151645]`，文本仍是 `嗯。`，端到端四条包括
「缓存单步 1773 与重算扩展提示的 argmax 1773」那一对都过。

余量从 2.6 倍掉到 1.05 倍，原因拆开看是两段：层内的量化矩阵乘、以及 `lm_head`
那一段也换成了量化矩阵乘。把两段分开量（新加的那条归因闸）：

| 口径（只差哪一组走量化矩阵乘） | 首 64 维最大差 | f16 单位 | argmax |
| --- | ---: | ---: | ---: |
| 两组都走（产品口径） | 7.8125e-2 | 7.6 | 106287 |
| 只走注意力（MLP 先拼 f16） | 6.25e-2 | 6.1 | 106287 |
| 只走 MLP（注意力先拼 f16） | 3.125e-2 | 3.0 | 106287 |
| 一组都不走（层内是 057 的算术） | 4.6875e-2 | 4.6 | 106287 |

四个数的容差都是 8.203125e-2。读法：

- 「一组都不走」这一档与 057 记的 3.125e-2 不再是同一个数，因为 `lm_head` 与
  词嵌入取行已经换成打包块算（这两处没有开关）；差出来的 1.5 个单位属于 `lm_head`
  那一段。
- 「只走 MLP」反而比「一组都不走」更接近参考（3.0 对 4.6 个单位），说明这些都是
  乘加顺序带来的舍入抽样，不是某一组带来的系统性偏差：换一条路径，抽样换一次，
  落在 3–8 个单位之间。token、文本、argmax 三样都没动。
- 若要按「最大差不得变大」这条更严的口径执行，`QuantPolicy` 是现成的分层回退：
  `QuantPolicy { attention: false, mlp: false }` 就是 057 的算术，单独关一组按上表
  也有对应的数。产品路径用的是两组都走。

### 2.2 编码器：没有变化；音频塔的 8 位试过，回退

音频塔的线性层是 8 位、组 64，与文本塔同一套打包格式，量化矩阵乘也支持 8 位。
把 `audio_tower.rs` 里 8 个线性层（`conv_out`、`proj1`、`proj2`、每层 q/k/v/out、
`fc1`/`fc2`）也换成打包之后：

| 量 | 容差 | 打包后实测 |
| --- | ---: | ---: |
| `conv_stem_out` | 3.955841e-3 | 0（逐位相同） |
| `encoder_layer0_out` | 1.2954712e-2 | 3.90625e-3 |
| `encoder_proj_out` | 1.9779205e-3 | 1.5753019e-3 |

三条都在容差内，但 `wrong_encoder_options_make_the_difference_bigger` 这一条挂了：
它要求「换错口径（位置表分母用 half、位置跨块连续编号、窗宽 100 帧）应当让差值变大」。
打包之后末尾投影上，换错分母那一档是 1.5524197e-3，小于默认的 1.5753019e-3，
「换错应更差」这条语义闸不再成立。这不是容差问题，是对照分辨力的问题，因此整段
回退：`audio_tower.rs` 与改动前逐字节相同，编码器三条闸的实测数与 057 一致
（`encoder_layer0_out` 3.90625e-3、`encoder_proj_out` 1.1855885e-3）。

## 3. 墙钟

### 3.1 整条路

```
engine_new 4.646–4.723 s（含常驻打包三块）
cold finish 0.424–0.486 s status=0   # 启动后第一次
warm finish 0.405–0.424 s status=0   # 同进程第二次、第三次
```

单次进程的 `bench`（页缓存冷，即本机第一次读这份权重）：`load 4.779 s`、
`finish 0.811 s`；页缓存暖之后是 `load 4.646 s`、`finish 0.427–0.433 s`。
057 的对应记录是 `engine_new 4.908 s`、冷 `finish 0.807 s`、暖 `finish 0.462–0.469 s`。

### 3.2 每一段花在哪

同一份探针、同一条 `phase_timing` 闸，把两种算术放在同一棵树上量。另一档用
`VOICE_QUANT_POLICY=dequantized`，七个投影即 057 的「先拼 f16 再乘」（`lm_head` 与
词嵌入取行两处没有开关，两档相同，所以表里的差只属于层内那七个投影）：

| 段 | 层内先拼 f16 | 量化矩阵乘 | 差 |
| --- | ---: | ---: | ---: |
| 音频塔 open | 1.585 s | 1.592 s | 未改 |
| 解码器 open | 2.707 s | 2.680 s | −27 ms |
| 梅尔（主机） | 0.273 s | 0.273 s | 未改 |
| 编码器（26 个音频 token） | 0.061 s | 0.060 s | 未改 |
| 预填充（44 个提示 token） | 0.060 s | **0.048 s** | −20% |
| 贪心（3 个新 token） | 0.148 s | **0.119 s** | −20% |

两件事说得清楚：

1. 「先反量化成 f16 再算」这段浪费确实被去掉了一层：矩阵乘那两步快了 20%。但它在
   定稿墙钟里只占 0.21 秒，所以整体 `finish` 从 0.466 秒降到 0.41 秒上下（−13%）。
   剩下的大头是主机上的梅尔（0.27 秒，`dev` 档案、优化关闭）与编码器。
2. 装载时的反量化省下来的是 27 毫秒，因为 `engine_new` 的大头是读 513 MB 与
   主机→设备的拷贝（解码器 open 2.68 秒里打包字节约 336 MB）；真正的收益是常驻的
   内存，见第 4 节。

### 3.3 `lm_head` 的块大小

`LM_HEAD_BLOCK` 从 8192 行改成 32768 行（19 块 → 5 块）。同一台机器、同一份探针，
连跑两档各两轮：

| 块大小 | 块数 | 冷 `finish` | 暖 `finish` |
| --- | ---: | ---: | ---: |
| 8192 行（4 MB/块） | 19 | 0.446–0.579 s | 0.425–0.440 s |
| 32768 行（16 MB/块） | 5 | 0.447–0.453 s | 0.418–0.424 s |

第 3.2 节那张表是在 8192 行这一档量的（两档口径用同一个块大小），所以那里的
`贪心 0.119 s` 不含这一节的 0.013 秒。差 0.013 秒：一次定稿里 `lm_head` 要跑三遍（预填充一次、每一步解码一次），每块都要
一次设备往返。两档都不物化整张词表；16 MB 一块的峰值更省事，取这一档。

## 4. 内存：常驻的 f16 整表不再产生

同一棵树、同一条 `phase_timing` 闸，两档各一次，`/usr/bin/time -l`：

| 口径 | maximum resident set size | peak memory footprint |
| --- | ---: | ---: |
| 层内先拼 f16（28 层约 820 MB + 词嵌入 311 MB 常驻） | 368 MB | 2.32 GB |
| 量化矩阵乘（打包值约 300 + 78 MB，缩放与偏置约 19 + 5 MB） | 588 MB | **1.53 GB** |

整条 FFI 路（`bench2`）上是 634 MB / **1.63 GB**，057 记的是 323 MB / 2.92 GB。
footprint 降了约 1.3 GB，等于那两张 f16 常驻表的和；resident set 升了，因为装载时
一块打包字节同时占着主机缓冲与设备副本。峰值里省下来的是常驻的 f16 表，不是精度。

## 5. 已知边界

1. **解码器主闸余量 1.05 倍**（7.6 / 8 个单位）。逐组归因的数见 2.1：三个组的数落在
   3–8 个单位之间，是乘加顺序的舍入抽样，不是偏差方向；token、文本、argmax 都没动。
   分层回退口是 `QuantPolicy`（`dequantized()` 就是 057 的算术）。
2. **音频塔仍是 8 位、仍先拼 f16**。打包路径在 `encoder_proj_out` 这一档会让
   「换错口径应更差」的语义闸失效（1.5524e-3 对 1.5753e-3），按第 2.2 节回退。
   要再往前走，得先把那条闸的判据说清楚，而不是先改数值。
3. **`finish` 的大头不是矩阵乘**。梅尔 0.27 秒是主机上的纯 Rust 计算，`dev` 档案
   没开优化；编码器 0.06 秒仍是先拼 f16 的那一档。两者都不在本次改动里。
4. **`engine_new` 的大头是读盘与拷贝**，量化矩阵乘只省掉 27 毫秒的反量化。要再压
   这一段，方向是少读、少拷贝（例如按需分块读），不是换矩阵乘。
5. **`dev` 档案的绝对值**。所有秒数都是 `dev` 档案；同一份代码在发布档案下会整体
   变小，但第 3.2 节的相对差是同一份档案、同一个进程形状下的对比。
6. **对照测试要限并发。** 三条闸各持一份常驻实例，`--test-threads=1` 或 2 稳定通过；
   四线程并行会超过本机 16 GB。`quantized_paths_are_compared_group_by_group` 一条
   里顺序打开四份解码器，也只在 `--test-threads=1` 下跑。

## 6. 复跑

```
cd code/voice-engine
cargo test --workspace --offline                       # 44 通过、4 忽略，不编 MLX

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
export VOICE_MODEL_DIR="/Users/dragon/Library/Input Methods/RimeClient.app/Contents/Resources/Models/Qwen3-ASR-0.6B-4bit"
export VOICE_METALLIB="/Users/dragon/Library/Input Methods/RimeClient.app/Contents/Resources/mlx.metallib"

cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test encoder_parity --test decoder_parity --test end_to_end_parity \
  -- --test-threads=1 --nocapture

cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test phase_timing -- --ignored --nocapture --test-threads=1
VOICE_QUANT_POLICY=dequantized cargo test -p voice-mlx --features metal \
  --target aarch64-apple-darwin --offline --test phase_timing -- \
  --ignored --nocapture --test-threads=1
```

<!-- research-complete -->
