# 067-research-load-cut-result：装载这一刀的结果、剩余瓶颈与验收

测量日 2026-11-15。这一篇只做三件事：把 065（按文件位宽直接建数组 + 末尾一次 eval）与
066（装载分两段）两刀的**前后完整对照**摆在一起、把**还剩下的瓶颈**按实测拆开、把**验收**
（三套闸、重编归档、重打包装包、签名与包内资产）记成人能照着复核的样子。

这一篇没有改算术：层数、位宽（文本 4 位 / 音频 8 位）、组大小 64、`QuantPolicy`、
`lm_head` 分块 argmax、容差口径、梅尔前端、分词器、`stream` 的切窗规则都没动。
`dev-docs/refs` 没动，librime 没动，`inject` 没调。

## 0. 结论速览

| 项 | 改前 | 现在 | 依据 |
| --- | --- | --- | --- |
| `voice_engine_new` | **4.841 s**（六次中位；4.755–5.300） | **0.137 s**（九次中位；0.133–0.286） | 065 §1、本篇 §4 |
| 两塔装载（含末尾 eval，一次装完口径） | **4.785 s** | **0.757 s**（0.617 构造 + 0.140 一次 eval） | 065 §2、本篇 §3 |
| 主机侧转换 | **3.731 s**（78.0 %） | **0.077 s** | 065 §2、本篇 §3 |
| MLX 侧 eval 与上传 | 0.491 s（722 次同步） | 0.140 s（1 次） | 同上 |
| 字节读取 | 0.391 s（1227 次、537 490 432 字节） | 0.378 s（同次数、同字节数） | 同上 |
| 暖定稿（2 秒探针） | 0.199–0.206 s | **0.200 s**（六次中位）／**0.193 s**（装包资产三次） | 065 §6、本篇 §5 |
| 冷定稿（启动后第一次） | 0.403–0.464 s | 0.847–1.098 s（六次中位 **1.024**）｜装包资产 0.842–1.176（中位 **0.972**） | 本篇 §5.2 |
| token | `[106287, 1773, 151645]` | `[106287, 1773, 151645]` | `end_to_end_parity` 4 条 |
| 文本 | `嗯。` | `嗯。` | 同上 + 装包资产实跑 |
| 解码器主闸首 64 维最大差 | 7.8125e-2（容差 8.203125e-2，余量 1.05 倍） | 逐位不变 | §6.1 |
| `cargo test --workspace --offline` | 51 通过、4 忽略、0 失败 | **54 通过**、4 忽略、0 失败 | §6.1 |
| 峰值最大 RSS | 629 637 120 字节 | 707 280 896 字节 | §5.3 |
| 装好的包 | 14 Nov 16:57 的产物 | 15 Nov 06:59 重打重装，`codesign --verify --deep --strict` exit 0 | §6.3 |

一句话：**4.8 秒里 3.73 秒是主机侧把 f16 字节逐元素解码成 `Vec<f32>` 再拷回 f16，
不是「eval 太多次」；改成按文件位宽直接建数组、末尾一次 eval 之后，装载从 4.785 落到
0.757 秒，再把它切成两段（第一段进 `voice_engine_new`、第二段跟在 `begin` 之后）之后
`engine_new` 只剩 0.137 秒。** 数值一个比特没变，token 与文本与基线逐位相同。

冷定稿那一行**变长了**，这是分段的代价，不是回退：第二段（分词器 + 文本塔）现在落在
第一次会话里等，§5.2 单列。

## 1. 三刀各自落在哪

| 刀 | 改的文件 | 做了什么 | 下一篇 |
| --- | --- | --- | --- |
| 装载路径 | `voice-mlx/src/backend/weights.rs`、`text_decoder.rs`、`audio_tower.rs`、`transcribe.rs` | 裸字节读进 8 字节对齐缓冲，`Array::from_raw_data` 按文件位宽直接建数组；`SafeWeights.pending` 攒图，`finish()` 一次 `eval`；`Engine::open` 两塔共用一份权重 | 064 之后的第一刀，记在 065 |
| 分两段 | `transcribe.rs`（`open_audio` / `load_text`）、`voice-ffi/src/{decoder,engine,backend,lib}.rs` | `voice_engine_new` 只装梅尔 + 音频塔；分词器 + 文本塔在 `begin` 的回发出之后装，`finish` 等它就绪 | 066 |
| 本篇 | 无代码改动 | 复测、验收、写结果与剩余瓶颈 | 本篇 |

两刀的收益不能相加：第一刀把「转换」从 3.731 压到 0.077 秒，第二刀把剩下的 0.62–0.66 秒
从 `engine_new` 挪到会话里。下面所有「改前」指第一刀之前（改动前的
`weights.rs`/`text_decoder.rs`/`audio_tower.rs`/`transcribe.rs` 编出的归档）。

## 2. 本次复测的口径

同一台机器（M2 Pro / 16 GB）、同一份探针 `code/voice-oracle/assets/probe_pcm.bin`（2.0 秒、
32 000 个采样）、同一份 metallib（装好的包里的 `183 870 464` 字节那份），`dev` 档案。

- 归档：`code/voice-engine/target/aarch64-apple-darwin/debug/libvoice_ffi.a`，285 348 856 字节，
  15 Nov 06:54，`md5 5160b33e5d39c79a1447c1659fe14159`。带 `SDKROOT` / `LIBRARY_PATH` /
  `MLX_SYS_SYSTEM_SDK=1` / `--target aarch64-apple-darwin` 重编一次，`cargo build` 报
  `Finished` 0.04 秒、`find -newer` 一个源文件都没有——即「无改动」。
- bench：`/tmp/voice-abi/bench7`（15 Nov 06:58，`md5 94c1e8a9b4872dcf07a43b49ff35b643`，
  37 722 040 字节），源是 `/tmp/voice-abi/bench3.c`，链的就是上面那份归档：

```
cd /tmp/voice-abi
xcrun clang -O2 bench3.c \
  /Users/dragon/repo/tools/rime-client/code/voice-engine/target/aarch64-apple-darwin/debug/libvoice_ffi.a \
  -o bench7 -liconv -lc++ -lobjc -framework Foundation -framework Metal \
  -framework Accelerate -lSystem -lc -lm \
  -Xlinker -U -Xlinker _MTLTensorDomain -Xlinker -U -Xlinker _MTLLogStateErrorDomain \
  -Xlinker -U -Xlinker _NSDeviceCertificationiPhonePerformanceGaming \
  -Xlinker -U -Xlinker _NSProcessInfoPerformanceProfileDidChangeNotification \
  -Xlinker -U -Xlinker _NSProcessPerformanceProfileDefault \
  -Xlinker -U -Xlinker _NSProcessPerformanceProfileSustained \
  -Xlinker -U -Xlinker ___fmaxf16 -Xlinker -U -Xlinker ___fminf16
```

`bench3.c` 一次进程里 `voice_engine_new` 量一次、`cold finish` 与 `warm finish` 各一次；
中间 `sleep(6)` 是留给外部挪走 `weights.safetensors` 验证常驻的，不影响计时。

`-Xlinker -U` 那八个符号与 `BUILD.txt` 里的说明同一条理由（MLX 的 `device.cpp` 与
`reduce.cpp` 引用了当前 SDK 只在 arm64e 段导出的弱符号），缺了不是警告而是链接失败。

## 3. 装载四段：改前 / 现在

量具是 `weights.rs` 的单元测试 `backend::weights::tests::load_phases_are_reported_one_by_one`
（`#[ignore]`，只在 `cfg(test)` 下存在，产品归档里没有 `Instant`）。本次复跑：

```
表头 0.0004 s（136535 字节）
音频塔：墙钟 0.226 s｜表头 0.0000｜字节读取 0.165（523 次、202118144 字节）｜转换 0.027｜求值与上传 0.000（0 次）｜四格和 0.192
文本塔：墙钟 0.391 s｜表头 0.0000｜字节读取 0.213（704 次、335372288 字节）｜转换 0.049｜求值与上传 0.000（0 次）｜四格和 0.263
一次 eval：墙钟 0.140 s｜字节读取 0.000（0 次）｜转换 0.000｜求值与上传 0.140（1 次）
两塔合计：墙钟 0.617 s｜字节读取 0.378（1227 次、537490432 字节）｜转换 0.077｜四格和 0.455
整个装载四格和 0.596 s，其中表头 0.0004，四项之外（locate/建层）0.162 s
```

| 段 | 改前（065 实测） | 现在（本次复测） | 差 |
| --- | --- | --- | --- |
| 表头（`SafeWeights::open`，136 535 字节） | 0.0005 s | 0.0004 s | 持平 |
| 字节读取（1227 次 seek+read、537 490 432 字节） | 0.391 s（1.37 GB/s） | 0.378 s（1.42 GB/s） | −0.013，已是页缓存速度 |
| 主机侧转换 | **3.731 s** | **0.077 s** | **−3.654 s** |
| MLX 侧 eval 与上传 | 0.491 s（**722 次**设备同步，每次约 0.7 ms） | 0.140 s（**1 次**） | −0.351 s |
| 四项之外（`locate` 在表头里搜键名、建层结构） | 0.172 s | 0.162 s | 持平 |
| 两塔装载（四项之和 + 上面那一行） | **4.785 s** | **0.757 s**（0.617 + 0.140） | **−4.028 s** |

分塔（改前 / 现在）：

| 塔 | 张量 | 字节 | 改前 | 现在 |
| --- | --- | --- | --- | --- |
| 音频塔（8 位、18 层） | 523 | 202 118 144 | 1.744 s（读 0.134 / 转 1.362 / eval 0.203×412） | 0.226 s（读 0.165 / 转 0.027 / eval 并入那次） |
| 文本塔（4 位、28 层） | 704 | 335 372 288 | 3.041 s（读 0.257 / 转 2.369 / eval 0.288×310） | 0.391 s（读 0.213 / 转 0.049 / eval 同上） |
| 两塔共用的那一次 eval | — | — | — | 0.140 s |

省掉的三次逐元素劳动（`weights.rs` 的注释里也写着）：

1. 逐元素 f16→f32 解码（`f16_to_f32` 与它的单测一起删了），打包权重那条走 `u32_le` 逐元素拼 `Vec<u32>`；
2. `from_slice` 把 f32 拷进 MLX（拷贝量从 4 字节/元素降到 2 字节/元素）；
3. `as_dtype` 的 GPU 逐元素转换（文件 F16 + 目标 F16 时不挂节点）。

f16→f32→f16 本是精确往返，所以第三个动作本来就不改一个比特，去掉它不影响数值。

两塔装载的**分段口径**（`voice-mlx/tests/staged_load.rs` 这次复跑）：第一段 0.104 s
（梅尔 0.001 + 音频塔 0.103），第二段 0.603 s（分词器 0.393 + 文本塔 0.211），两段和
0.707 s，一次装完 0.656 s。

## 4. `engine_new`：九次实测

`bench7` 对同一份归档、同一模型目录、同一 metallib，九次（前六次用 `code/assets` 里的资产，
后三次用**装好的包**里的资产，两者哈希相同）：

```
第 1 次  0.254 s   ← 页缓存与 MLX 都凉
第 2 次  0.134 s
第 3 次  0.134 s
第 4 次  0.176 s
第 5 次  0.137 s
第 6 次  0.133 s
装包资产  0.286 s   ← 又一次冷
装包资产  0.137 s
装包资产  0.133 s
```

排序后中位 **0.137 s**（六次那一组中位 0.136 s，与 066 的十六次中位 0.136 s 同一档）。
两次偏高的 0.254 / 0.286 都是「这一发是冷启动」，不是分布变宽。

改前 4.841 s（065 六次中位，`bench3-baseline` 链改动前的归档）；066 的改前是 0.756 s
（十二次中位，那时已经过第一刀）。这一串关系是：4.841 → 0.742/0.756（第一刀）→ 0.137
（第二刀，只装第一段）。

## 5. 定稿：暖、冷、内存

### 5.1 暖定稿

`bench7` 同一个进程的第二次 `finish`（权重与文本塔都在位）：

```
六次（code/assets 资产）  0.183 / 0.203 / 0.202 / 0.205 / 0.197 / 0.191 → 中位 0.200 s
三次（装包资产）          0.208 / 0.188 / 0.193                        → 中位 0.193 s
```

改前 0.199–0.206 s（065）。**暖定稿没有变**——这一刀只动装载，定稿的算术一步没少。

### 5.2 冷定稿变长，是分段的代价

`bench7` 的第一次 `finish`（`begin` → `feed` → `finish`，进程刚起）：

```
六次  1.016 / 1.098 / 1.044 / 1.031 / 0.926 / 0.847 → 中位 1.024 s
三次  1.176 / 0.842 / 0.972                       → 中位 0.972 s
```

065 那一版是 0.222–0.263 s，因为那时第二段在 `engine_new` 里就装完了，冷定稿只剩解码。
现在第二段（0.603–0.66 s）落在第一次会话里等，冷定稿 = 第二段 + 解码 ≈ 0.97–1.02 s。
用户侧看到的是「进程起→出字」：066 实测 2 秒说话那一档 3.006–3.118 → **2.401–2.513 s**，
极短那一档 1.233–1.273 → **0.926–0.950 s**，都是往好的方向；只有「按下就松手、且进程刚起」
这一档按下→出字从 0.233–0.255 变成 0.831–0.848 s，因为它在会话里等第二段。
这条在 066 §5 已经单列，本篇不改口。

### 5.3 峰值内存

```
/usr/bin/time -l bench7 … → 707 280 896  maximum resident set size
```

改前 629 637 120 字节（065），第一刀之后 723 402 752 字节。现在 707 MB 落在两者之间——
差额全部来自「一次 eval 之前，主机侧的常量要一起等到那次 eval」，与 065 §4 记的那笔账一致。

## 6. 还剩下的瓶颈

装载这一刀把「读盘 + 转换 + eval」从 4.8 秒压到 0.76 秒（分段之后 `engine_new` 只剩
0.14 秒），下面这些**没动**，都是这次实测出来的具体位置。

### 6.1 第二段里分词器占 2/3

```
第二段 0.603 s = 分词器 0.393 + 文本塔 0.211
```

`voice-tokenizer` 的 `Tokenizer::load` 把 `vocab.json` 的 151 643 条写成
`pieces: Vec<Vec<u8>>` 与 `ids: HashMap<Vec<u8>, u32>`，每条一次堆分配；`merges.txt`
再建 `ranks: HashMap<(Vec<u8>, Vec<u8>), u32>`，每条键是两段字节串。0.393 秒基本是这
三十万次小分配加上哈希建表。换成一遍扫下来的紧凑结构（偏移表 + 大缓冲，哈希只存
`(u32, u32)`）能吃掉这一块，但会改 `voice-tokenizer` 的对外形状，本篇没做。

### 6.2 每一步都整段重算梅尔

`phase_timing` 这次的定稿分段（`dev` 档案，200 帧）：

```
梅尔 0.050 s（200 帧）
编码器 0.129 s（26 个音频 token）
预填充 0.069 s（44 个提示 token，argmax 106287）
贪心 0.127 s（3 个新 token）
```

`stream` 的每一步也重算整段梅尔：2 秒处的步 0.189 s 里梅尔 0.049（26 %）、编码 0.031、
解码 0.108；14 秒探针的步墙钟 0.189 / 0.438 / 0.745 / 1.112 / 1.521 / 1.994 s 随已收的
音频线性长，其中梅尔每次都从第一个样本重算（只把「已冻结窗的投影」跨步复用）。
梅尔是 O(整段时长) 的重活，本来可以按窗增量算。

### 6.3 解码器跨步不复用 KV

音频占位个数每步都变，`<pad>` 之后的 RoPE 位置整体平移，所以每一步都要把 44–52 个提示
token 重新预填充（0.108 s/步）。跨步复用键值要先解决位置编码随音频长度平移这件事，
现在没有解法写进代码。

### 6.4 贪心按 token 计费

3 个新 token 0.127 s ≈ **42 ms/token**：28 层、隐层 1024、16/8 头，加 `lm_head` 是对整张
词表（151 936 行）按 5 块 × 32768 行做 argmax。长句按 3 token 的 3 倍、10 倍线性涨。
这条是「定稿 0.19 秒」里最大的一块，也是长音频定稿的主项。

### 6.5 定稿仍整段重算

`finish` 不借用最后一步已经算好的部分结果，整段重算梅尔 + 编码 + 预填充。2 秒探针这
0.19 秒里，最后一步的部分结果本来已经有过一份接近的。

### 6.6 一次 eval 的 0.14 秒与剩下的拷贝

`Array::from_raw_data` 底下 `mlx_array_set_data` 还要把主机缓冲再拷进 MLX 的常量区。
`mlx_array_new_data_managed_payload`（`mlx-sys` 里有）能免掉这次拷贝，但要直连 `mlx-sys`
并把缓冲按权重寿命钉在主机侧；065 §4 记的条件是「不为不到 0.15 秒这么写」，本篇不改。

### 6.7 所有数字都是 `dev` 档案

`release` 的读盘、转换、梅尔、解码一个数都没有。`dev` 档案的绝对数是上限，不是发布值。

## 7. 校验

### 7.1 测试

```
cd code/voice-engine && cargo test --workspace --offline
```

**54 通过、4 忽略、0 失败**（`voice-asr` 10、`voice-ffi` 13、`voice-mel` 5、`voice-parity`
7+4、`voice-tensor` 的 `shapes` 6、`voice-tokenizer` 4+5；忽略的是 `voice-parity`
`tests/parity.rs` 里四个指向 MLX 实对照的入口）。`voice-mlx` 在未打开 `metal` 时没有测试。

打开 `metal`（参数见 `BUILD.txt` 与 `encoder_parity.rs` 文件头），全部 `--test-threads=1`：

| 目标 | 结果 | 关键实测 |
| --- | --- | --- |
| `--lib` | 4 通过 + 1 忽略 | 忽略的是分段计时那条 |
| `--lib load_phases -- --ignored` | 1 通过 | §3 的四段数 |
| `--test encoder_parity` | 3 通过 | 卷积茎差 0、第 0 层 3.90625e-3、末尾投影 1.1855885e-3 |
| `--test decoder_parity` | 6 通过 | 主闸 7.8125e-2 对容差 8.203125e-2；注入参考特征 6.25e-2；两条归因 2.5214844e1 / 2.1436523e1；四口径分组 7.8125e-2 / 6.25e-2 / 3.125e-2 / 4.6875e-2，argmax 全 106287 |
| `--test end_to_end_parity` | 4 通过 | 提示 44 个 id 逐位相同、贪心 `[106287, 1773, 151645]` 逐位相同、文本 `嗯。`、缓存第二步 1773；一次 `transcribe` 墙钟 0.886 s |
| `--test staged_load` | 1 通过 | 第一段 0.104 / 第二段 0.603；分段装完定稿「嗯。」与一次装完逐位相同 |
| `--test streaming -- --ignored` | 2 通过 | 切窗与整段编码投影差 1.0691956e-4（闸 1e-3）；2 秒处部分步 0.189 s 出「嗯。」，定稿与整段重算逐位相同 |
| `--test phase_timing -- --ignored` | 1 通过 | §6.2 的分段 |

三套闸的差值与 065/066 逐位相同，没有一条容差被放宽。

### 7.2 重编归档与 Squirrel

```
cd code/voice-engine
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
cargo build -p voice-ffi --features metal --target aarch64-apple-darwin
```

`Finished` 0.04 秒，归档 285 348 856 字节（15 Nov 06:54，`md5 5160b33e…`）——源码没有新改动，
这一刀之前就已经编进去了。

```
cd ../Client
rm -f .build/out/Products/Debug/Squirrel
swift build --product Squirrel
```

`Build complete!`，产物 `Debug/Squirrel` **40 605 120 字节**；`nm -gU` 里
`backend10transcribe` **57** 次、`_voice_engine_new` / `_voice_session_finish` 都在
（`-Xlinker -u` 拉住的那几个）。链接里唯一的噪声是既有的部署目标告警：MLX 的对象按
macOS 27.2 编、Squirrel 部署目标 14.0。

### 7.3 打包、装包、签名

```
bash code/Client/scripts/make-app.sh
bash code/scripts/install-app.sh
```

装到 `~/Library/Input Methods/RimeClient.app`，脚本自己走「停进程 → ditto → `codesign
--verify --deep --strict` → 核对包内资产」四步，四步都过：

```
校验签名：codesign --verify --deep --strict
在  Contents/Info.plist  (2791 字节)
在  Contents/Resources/mlx.metallib  (183870464 字节)
在  Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/weights.safetensors  (537626967 字节)
在  Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/vocab.json  (2776833 字节)
资产哈希与包内清单一致
```

再单独跑一次 `codesign --verify --deep --strict "$HOME/Library/Input Methods/RimeClient.app"`，
**exit 0**。包内可执行文件 40 605 424 字节（比打包前的 40 605 120 多 304 字节，是
`codesign --force --sign -` 重写签名造成的），`md5 d427a1d977e16181eeea8d45d2db3c27`，
与 `code/Client/dist/Squirrel.app` 里的那份逐字节相同。

装好之后用**包里的资产**实跑 `bench7`：文本仍是 `嗯。`，三次 `engine_new` 0.286 / 0.137 /
0.133 s、暖定稿 0.208 / 0.188 / 0.193 s，即这一刀在装好的包上同样成立。

### 7.4 这一刀没有拿什么换

- 没有跳过层：音频塔 18 层、文本塔 28 层，`load_phases` 与 `staged_load` 都断言了层数。
- 没有降精度：文本 4 位、音频 8 位、组 64、`QuantPolicy` 双开，容差一个数没改。
- 没有改算术语序：`decoder_parity` 的四口径分组对照与两条归因断言逐位与 065 相同。
- 没有调 `inject`，没有改 librime，没有写进 `dev-docs/refs`。

## 8. 复跑

```text
# 四段装载
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
  --lib load_phases -- --ignored --nocapture --test-threads=1

# 定稿分段
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test phase_timing -- --ignored --nocapture --test-threads=1

# engine_new 与暖定稿（重复六次）
bash /tmp/voice-abi/link.sh /tmp/voice-abi/bench3.c /tmp/voice-abi/bench7
/tmp/voice-abi/bench7 \
  "$HOME/Library/Input Methods/RimeClient.app/Contents/Resources/Models/Qwen3-ASR-0.6B-4bit" \
  "$HOME/Library/Input Methods/RimeClient.app/Contents/Resources/mlx.metallib" \
  code/voice-oracle/assets/probe_pcm.bin
```

## 9. 本篇没做的事

- `release` 档案一个数都没有（§6.7）。
- 分词器的紧凑化（§6.1）、梅尔的增量重算（§6.2）、跨步复用 KV（§6.3）、贪心的优化
  （§6.4）都只写了位置与实测代价，没有动手——它们会改算术边界之外的形状，得单独一篇。
- 真实麦克风语句到上屏的整段墙钟仍没有（066 的 `keyprobe` 只到「可以接受按键」）。

<!-- research-complete -->
