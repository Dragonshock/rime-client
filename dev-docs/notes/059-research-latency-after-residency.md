# 059-research-latency-after-residency：常驻权重之后，2 秒探针的 finish 从 12.534 秒降到 0.443 秒

测量日：2026-11-14。对象是 `code/voice-engine`（`voice-mlx` 的 `Engine` 与 `voice-ffi`
的常驻槽）与已安装包 `~/Library/Input Methods/RimeClient.app`。基线是本次给的
12.534 秒（同一探针、同一计时程序、同一口径）。探针是
`code/voice-oracle/assets/probe_pcm.bin`（2.00 秒、32000 样本），模型目录取自已安装包的
`Contents/Resources/Models/Qwen3-ASR-0.6B-4bit`，metallib 取自已安装包的
`Contents/Resources/mlx.metallib`。计时程序仍是 `/tmp/voice-abi/bench.c`
（`/tmp/voice-abi/bench-r`，用 `link.sh` 对新归档重编）、`bench2.c`（同进程三次）。
编译是 `dev` 档案（与产品归档同档），下面所有秒数都带着这一点。

本次没有改这些箱子的代码（只在 `code/voice-engine/BUILD.txt` 里补了这段记录）：12.534 秒那一版
跑的是**安装包里 14:05 的旧产物**，而源码里的常驻改造在 057/058 已经落地（14:31）。
下面第 1 节是这条诊断，第 2 至 4 节是重编、重装之后的实测。`dev-docs/refs`、
`inject`、librime 都没动，Python 没有进产品路径。

## 0. 结论速览

| 项 | 基线（旧产物） | 现在（新产物） | 说明 |
| --- | ---: | ---: | --- |
| `finish`（每次进程内第一次，三次中位） | **12.534 s** | **0.443 s** | 同一 `bench`、同一探针；三次 0.502 / 0.443 / 0.440 |
| `finish`（另一组三次，页缓存刚热） | — | 0.635 s | 0.712 / 0.635 / 0.515；六次全距 0.440–0.712 |
| `finish` 同进程第二次及以后（暖） | — | **0.410–0.424 s** | `bench2`：冷 0.608、暖 0.424、暖 0.410 |
| `voice_engine_new` | **0.035 s** | **4.735–4.958 s** | 513 MB 读盘与反量化搬到了引擎线程起来的时候 |
| `feed` 32000 样本 | 0.000 s | 0.000 s | 只入队，不在调用线程上算 |
| token 序列 | `[106287, 1773, 151645]` | `[106287, 1773, 151645]` | 逐位相同（端到端对照测试断言） |
| 文本 | `嗯。` | `嗯。` | 相同（`bench` 的 JSON 回复） |
| 安装包 | 14:05 的旧产物 | **已装 14:57 的新产物** | `codesign --verify --deep --strict` exit 0 |

12.534 与 0.035 那一对是本次给的基线；其余是本次实测。六次 `bench` 全距 0.440–0.712 秒，
差别来自页缓存与 Metal 首次建图，不是两次不同的代码。

## 1. 12.534 秒是怎么来的

`transcribe.rs` 里有两个入口，行为不同，混起来就得到那对奇怪的数：

1. **模块级的 `transcribe_with`**（`voice-mlx/src/backend/transcribe.rs:114`）每次调用都
   `Engine::open`（同文件 `:148`），里面是 `AudioTower::open`（`:152`）与
   `TextDecoder::open`（`:153`）。这条口子现开现用，注释写明「只给对照与单次实验」。
   它一次 `transcribe` 要读两遍权重：音频塔一遍、文本塔一遍。
   055 那一节在 `code/voice-engine/BUILD.txt` 里记的「11.7 秒（含两次权重装载）」就是这条。
2. **C ABI 的产品路径**（`voice-ffi`）：`voice_engine_new` 启线程时
   `backend::boot` → `decoder::boot_resident` 把一份 `voice_mlx::Engine` 放进引擎线程的
   `thread_local!` 槽（`voice-ffi/src/decoder.rs:61`、`:81`），之后每个会话只
   `transcribe_on_resident` 借用它，`WeightDecoder` 不带模型目录。这条路径的 `finish`
   不读盘。

安装包里 14:05 的那份产物**早于**常驻改造（源码 14:31），跑的仍是第 1 条那条「每次现开」
的形态：
`voice_engine_new` 只起线程就返回（0.035 秒），513 MB 的读盘与反量化整个落在 `finish`
里，还因为两座塔各自打开而读了两遍 —— 这就是 12.534 秒。`bench` 的 `load` 一行当时
只有 0.035 秒、`finish` 一行 12.534 秒，两行加起来才是那一次按键的实际等待。

所以本次的修法不是改算术，而是**把源码里已有的常驻实例真正装进包里**：重编归档、重编
Swift 产物、重打包、重装。第 2 至 4 节是对新产物的实测与验收。

## 2. 现在量到什么

### 2.1 三次 `bench`（每次一个新进程）

```
./bench-r <模型目录> <mlx.metallib> probe_pcm.bin        # 三次，取中位

load 4.937 s / finish 0.502 s / reply {"text":"嗯。"}
load 4.771 s / finish 0.443 s / reply {"text":"嗯。"}
load 4.735 s / finish 0.440 s / reply {"text":"嗯。"}
```

中位 **0.443 秒**，对 12.534 秒是 28.3 倍。装包之前用同一份新归档、同一个
`bench-r` 还量过三次（页缓存刚热，`load` 4.88–5.34 秒）：0.712 / 0.635 / 0.515，
中位 0.635 秒。两组只差在进程冷热，没有别的变量。

`begin` 每次 0.003–0.005 秒、`feed` 0.000 秒，与基线一致：这两步不碰张量。

### 2.2 同进程三次 `bench2`

```
engine_new 4.958 s（含常驻权重）
cold finish 0.608 s status=0
warm finish 0.424 s status=0
warm finish 0.410 s status=0
```

暖档 0.410–0.424 秒与 058 的 0.405–0.424 秒在同一档；冷档 0.608 秒是这条进程里第一次
定稿（Metal 首次建图与页缓存）。

### 2.3 定稿内部分段（`phase_timing`，`#[ignore]`）

```
音频塔 open 1.592 s
解码器 open 2.719 s
梅尔 0.267 s（200 帧）
编码器 0.093 s（26 个音频 token）
预填充 0.050 s（44 个提示 token，argmax 106287）
贪心 0.109 s（3 个新 token）
```

两行 `open` 加起来 4.31 秒，只在引擎线程起来时付一次；一次定稿里真正跑的是后面四行，
合计约 0.52 秒 —— 与上面 0.44–0.50 的 `finish` 同一量级（差在首次建图）。这与 058 记的
「墙钟大头不在矩阵乘」一致：梅尔 0.267 秒仍是定稿里最大的一块，且它是主机侧、`dev` 档案。

## 3. 验收

### 3.1 token 与文本

- `bench` 的 JSON 回复三次都是 `{"decoded":true,"kind":"final","ok":true,"session":1,"text":"嗯。"}`，
  `finish` 返回 0，`last_error` 为空。
- token 序列由 `end_to_end_parity.rs` 的 `greedy_sequence_and_text_match_reference` 逐位断言：
  实测与 `manifest.json` 的 `greedy.tokens = [106287, 1773, 151645]` 相同，文本与
  `text_from_tokens`（去 `im_end` 字面量后）相同，`stopped_by = ImEnd`。
  C ABI 的口子只回文本，不回 token，所以 token 这一项以这条对照测试为准；它走的是同一条
  `transcribe_with_opened`，只差实例从哪来。

### 3.2 测试

```
cd code/voice-engine
cargo test --workspace --offline
```

44 通过、4 忽略、0 失败（`voice-mlx` 在未打开 `metal` 时不参与）。

```
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
export VOICE_MODEL_DIR="$HOME/Library/Input Methods/RimeClient.app/Contents/Resources/Models/Qwen3-ASR-0.6B-4bit"
export VOICE_METALLIB="$HOME/Library/Input Methods/RimeClient.app/Contents/Resources/mlx.metallib"

cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test encoder_parity --test decoder_parity --test end_to_end_parity \
  -- --test-threads=1
```

encoder 3、decoder 6、end_to_end 4 全过；含 `wrong_encoder_options_make_the_difference_bigger`、
`quantized_paths_are_compared_group_by_group`、`cached_step_matches_a_fresh_prefill`。
三条闸各持一份常驻实例，必须 `--test-threads=1`。

Swift 侧同时跑过：`code/Client` 7 条、`code/VoiceInterface` 21 条，全通过。

### 3.3 归档与产物

```
cargo build -p voice-ffi --features metal --target aarch64-apple-darwin   # 增量 0.16 秒，已是最新
swift build --product Squirrel                                          # 删掉旧产物后重链，1.16 秒
```

新产物 `code/Client/.build/out/Products/Debug/Squirrel` 40499476 字节（重链之前是
40429344 字节），`nm` 里 `backend10transcribe` 出现 43 次
（`voice_mlx7backend10transcribe` 一族，含 `Engine` 与 `Transcription`，以及
`boot_resident`/`transcribe_on_resident` 里那两个闭包），八条 `voice_*` 仍是 `T`；
`dist/Squirrel.app` 与已安装包里同一份二进制同样是 43 次。旧产物已被删除，没有留下
它的符号表，因此这一节只记新产物的数。

### 3.4 包

```
bash code/Client/scripts/make-app.sh
ditto code/Client/dist/Squirrel.app "$HOME/Library/Input Methods/RimeClient.app"
codesign --verify --deep --strict "$HOME/Library/Input Methods/RimeClient.app"   # exit 0
```

装之前先停掉用户目录里那个包（`RimeClient.app`）的 `Squirrel` 进程（pid 68133）。
装完的包：

| 项 | 值 |
| --- | --- |
| `Contents/Info.plist` 的 `CFBundleIdentifier` | `im.dragonshock.inputmethod.RimeClient` |
| `Contents/MacOS/Squirrel` | 40499776 字节，14:57 |
| `Contents/Resources/mlx.metallib` | 183870464 字节 |
| `Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/weights.safetensors` | 537626967 字节 |
| `Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/vocab.json` | 2776833 字节 |
| 模型目录里的符号链接数 | 0 |
| `codesign --verify --deep --strict` | 通过（`valid on disk`、`satisfies its Designated Requirement`） |

另外核过：`config.json`、`merges.txt`、`tokenizer_config.json`、
`quantization_config.json` 都在包内，`entitlements.plist` 与 `rime.pdf` 也在。

## 4. 仍然未知与仍然偏慢的地方

- **麦克风到屏幕的墙钟仍是未知。** 这次没有做真实麦克风语句，全部是文件探针喂进
  `voice_session_feed`。按住说话到文字上屏的整段（含采集、上屏与预编辑）没有测。
- **第一次按键要付引擎创建。** `Client/sources/SquirrelInputController.swift` 的
  `ensureVoiceGate()`（`:594`）是在 `handle` 里懒建的，`voice_engine_new` 又要等引擎线程
  把权重读完才返回（`Worker::spawn` 的 `ready_rx.recv()`），所以输入法起来之后的
  **第一次按键**会等 `dev` 档案下的 4.7–4.9 秒。之后的按键不再付这一段。
  这条落在 `code/Client` 侧，本次的范围只到 `code/voice-engine` 与 `code/voice-ffi`，
  因此只记不改编排。
- **`dev` 档案。** 上面所有秒数都是 `dev` 档案、页缓存有内容时量的；`release` 的读盘、
  反量化与梅尔都还没有量。
- **部分文本仍未接。** 说话中不发部分结果（`WeightDecoder` 只在 `is_final` 时算），
  这一条按 058 与 `进行中` 的记录未变。
- **同进程多次 `open` 的代价。** 模块级 `transcribe_with` / `transcribe` 仍按原样每次
  现开（对照用）。`phase_timing` 这类测试每次进程都付 4.3 秒的 `open`，这是测试的
  形状，不是产品路径。

<!-- research-complete -->
