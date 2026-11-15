# 064-research-portability-boundaries：资产与打包路径显式化，移植边界写清（仍是 arm64 + Metal + MLX）

测量日 2026-11-14。对象是 `code/scripts/fetch-model.sh`（新）、`code/scripts/install-app.sh`（新）、
`code/Client/scripts/make-app.sh`、`code/Client/Package.swift`。数值一个数没改：
`voice-mel`、`voice-mlx`、`voice-asr`、`voice-ffi` 的算术这次一行没动，容差一个数没改，
`dev-docs/refs` 没动，librime 没动，`inject` 没调。包已按新脚本重打、重装、签名校验通过。

## 0. 结论速览

| 项 | 改前 | 现在 |
| --- | --- | --- |
| 权重从哪来 | 只认本机 `~/.cache/huggingface/hub/.../snapshots/<refs/main>` | HF `moona3k/mlx-qwen3-asr-0.6b-4bit` 的固定修订 `4c59c53…`，逐个文件核 sha256 |
| metallib 从哪来 | `find target -path '*/mlx-sys-*/out/build/lib/mlx.metallib' \| ls -t \| head -1` | 显式 `--metallib` / `--metallib-from-build`，哈希对不上就拒 |
| 大文件进 git | 不进（`.gitignore` 排除 dist 内资产） | 不进；默认落到 `code/assets/`，已加进 `code/.gitignore` |
| make-app.sh 输入 | 全靠猜：find + `refs/main` | `VOICE_MODEL_DIR` / `VOICE_METALLIB` / `OUT_APP` 或 `--model-dir` / `--metallib` / `--app` |
| 缺文件时 | `test -f` 静默退出 1 | 一行中文原因 + 下一步该跑什么命令 |
| Package.swift 里的绝对路径 | 3 处写死本机（`/Users/dragon/…` 两条、`/nix/store/…` 一条） | `CLIENT_ROOT` / `LIBRIME_PREFIX` / `VOICE_FFI_ARCHIVE` 可覆盖，缺省仍是本机值 |
| 一次完整取资产 | — | 537 626 967 字节，**2 分 13 秒**，7 个文件全部核过哈希 |
| 一次打包 / 一次装包 | — | 打包 6.4 秒、装包 2.7 秒；包 740 M（与改前同体积） |
| `codesign --verify --deep --strict`（装好的包） | exit 0 | exit 0 |
| 解码器主闸 | 7.8125e-2（容差 8.203125e-2，余量 1.05 倍） | 7.8125e-2（同左，逐位不变） |
| `cargo test --workspace --offline` | 53 通过、4 忽略、0 失败 | 53 通过、4 忽略、0 失败 |
| 打开 metal 的三套闸 | 13 通过、0 失败 | 13 通过、0 失败（差值逐位相同） |

一句话：**这一刀换来的是「换一台机器知道该跑哪两条命令、缺文件时知道缺哪个」，
不是余量。** 主闸仍是 7.8125e-2 对 8.203125e-2，encoder 仍是 3.90625e-3 / 1.1855885e-3，
一个字都没变好——因为这次没碰算术。

## 1. 资产：HF 上有什么、没有什么

用 HF tree API 核过该仓库在固定修订上的全部文件（8 个）：6 个权重相关文件，加
`.gitattributes` 与 `README.md`。**没有 `mlx.metallib`**。

```
curl -s "https://huggingface.co/api/models/moona3k/mlx-qwen3-asr-0.6b-4bit/tree/4c59c533f95c84afb796655e814709034f826f04"
```

脚本里写死的校验表（`fetch-model.sh` 注释与 `MODEL_FILES` 数组，两处同一份值）：

| 文件 | 字节 | sha256（2026-11-14） |
| --- | ---: | --- |
| `config.json` | 6193 | `76d3ae4601ce939830b2517f4a6cadb86cc51316c3900af6b020b051c21a478c` |
| `merges.txt` | 1671853 | `8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5` |
| `quantization_config.json` | 60 | `067339b858243ad9d5e22764b3b82c759a765c9f0aadfe4939d0c233f15b9e7d` |
| `tokenizer_config.json` | 12487 | `4942d005604266809309cabc9f4e9cb89ce855d59b14681fdc0e1cc62ea26c4c` |
| `vocab.json` | 2776833 | `ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910` |
| `weights.safetensors` | 537626967 | `9a62bb9830dcbcd0cf1c1f40200161d4e5f007a4cd99b83111902890a9f087d3` |
| `mlx.metallib` | 183870464 | `d523d6022efced2817e305fdca37c325868f670d96487aaf24a6864bcda3c722` |

`mlx.metallib` 是 MLX 的 C++ 内核编译产物（`mlx-sys` 在 `cargo build -p voice-ffi --features metal`
里生成），不是权重仓库的附件，所以脚本给它两处合法来源：显式路径（`--metallib PATH` /
`$VOICE_METALLIB`）或本机构建目录（`--metallib-from-build`，只把选中的那一条路径打出来）。
本机 `code/voice-engine/target/aarch64-apple-darwin/debug/build/` 下六个 `mlx-sys-*` 目录的
metallib 哈希全是 `d523d602…`，同一 MLX 版本同一条工具链下这份产物是确定的，钉哈希成立；
换 Xcode 或换 MLX 版本它就会变，那时用 `--accept-new-metallib` 显式接受并把新哈希写进
`MANIFEST.sha256`。

脚本落盘的形状（`make-app.sh` 只认这两条路径）：

```
<dest>/qwen3-asr-0.6b-4bit/{config.json,merges.txt,quantization_config.json,
                           tokenizer_config.json,vocab.json,weights.safetensors}
<dest>/mlx.metallib
<dest>/MANIFEST.sha256      # 本次实际哈希，装包时复核
```

默认 `<dest>` 是 `code/assets`（`code/.gitignore` 里一行 `assets/`）。不走 git-lfs，
不用 `git` 存这些大文件：仓库里没有任何一处跟踪权重或 metallib。

实测三条失败路径都真退出（不是只打警告）：

| 动作 | 输出（首行） | exit |
| --- | --- | ---: |
| 往 `config.json` 尾巴加 1 字节后 `--verify-only` | `config.json 的 sha256 不对：实际 9dbd11b9…（6194 字节），脚本记录 76d3ae46…（6193 字节）` | 1 |
| 目录里没 metallib 也不给来源 | `没有 mlx.metallib 的来源。这条给三种办法（Hugging Face 上没有 metallib 镜像，脚本不会去猜）` | 1 |
| 给一份 4096 字节的假 metallib | `mlx.metallib 的 sha256 与脚本记录的不同：实际 346d3fe1…（4096 字节），记录 d523d602…（183870464 字节）` | 1 |

`--verify-only` 不联网，逐个文件与脚本里的表比，再按 `MANIFEST.sha256` 复核一遍：
已经取好的 `code/assets` 上 **4.453 秒**（7 个文件 + 694 MB 数据），全过 exit 0；
篡改任意一个字节就 exit 1 并点名那一条。`--weights-only` 只取权重六个文件、写完清单 exit 0，
之后补 metallib 再跑一次会重写含 7 行的清单。

## 2. make-app.sh：参数化，且先核对再动 dist

改前的两处「猜」都删了：`find … -path '*/mlx-sys-*/out/build/lib/mlx.metallib' | xargs ls -t | head -1`，
以及 `tr -d '[:space:]' < $HF_ROOT/refs/main` 拼出来的快照路径。现在是三个旋钮：

| 旋钮 | 环境变量 | 命令行 | 缺省 |
| --- | --- | --- | --- |
| 模型目录 | `VOICE_MODEL_DIR` | `--model-dir DIR` | `code/assets/qwen3-asr-0.6b-4bit` |
| metallib | `VOICE_METALLIB` | `--metallib FILE` | `code/assets/mlx.metallib` |
| 输出包 | `OUT_APP` | `--app PATH` | `code/Client/dist/Squirrel.app` |

还把顺序改了：**资产核对全部挪到 `rm -rf "$APP"` 之前**，所以缺文件时不会留下一个半个包。
核对项：metallib 必须是普通文件、非符号链接、非空；模型目录六个文件逐个在、非链接、非空、
目录里没有任何符号链接；`<assets>/MANIFEST.sha256` 存在时用 `shasum -a 256 -c` 逐个复核。

缺文件时现在是两句（第一句是什么没了，第二句是怎么办）：

```
make-app.sh: 没有 mlx.metallib：/tmp/emptyassets/mlx.metallib
取资产：code/scripts/fetch-model.sh --metallib-from-build
或者用 --metallib FILE（等价环境变量 VOICE_METALLIB）显式给出
```

```
make-app.sh: 没有模型目录：/tmp/emptyassets/qwen3-asr-0.6b-4bit
取资产：code/scripts/fetch-model.sh --dest "$ASSETS"
或者用 --model-dir DIR（等价环境变量 VOICE_MODEL_DIR）显式给出
```

包里多了一件可核对的东西：`Contents/Resources/voice-assets.sha256`（696 字节，从
`<assets>/MANIFEST.sha256` 拷来，7 行）。`install-app.sh` 装完就按它复核包内资产。

## 3. Package.swift：三处写死的绝对路径改成可覆盖

改前写死在本机的位置：bridging header 与两处 `-I` 头路径（`/Users/dragon/repo/tools/rime-client/code/Client/…`）、
链接参数 `-L/nix/store/i3r73fca2jfd0m75xr1zlfmdp3zblhya-librime-1.17.0/lib`、
以及 `-Xlinker /Users/dragon/repo/tools/rime-client/code/voice-engine/target/aarch64-apple-darwin/debug/libvoice_ffi.a`。

现在三处各自可覆盖，缺省值仍是原来那三条字符串，所以这台机器上的构建行为不变：

| 变量 | 缺省 |
| --- | --- |
| `CLIENT_ROOT` | `#filePath` 所在目录（`code/Client`） |
| `LIBRIME_PREFIX` | `/nix/store/i3r73fca2jfd0m75xr1zlfmdp3zblhya-librime-1.17.0` |
| `VOICE_FFI_ARCHIVE` | `<CLIENT_ROOT>/../voice-engine/target/aarch64-apple-darwin/debug/libvoice_ffi.a` |

改完 `swift build --product Squirrel` 1.17 秒完成（增量），`nm -gU` 里 8 条被 `-u` 拉住
的符号仍在（`_voice_engine_new` / `_voice_engine_free` / `_voice_engine_set_callback` /
`_voice_engine_set_reply_sink` / `_voice_engine_last_error` / `_voice_session_begin` /
`_voice_session_feed` / `_voice_session_finish`）。装好的包 `otool -L` 只有
`@rpath/librime.1.dylib` 与 `@rpath/Sparkle.framework/…`，没有 `/nix` 路径：
librime 及其依赖（libgflags / libglog / libleveldb / liblua / libopencc / libsnappy /
libyaml-cpp）都是打包时从 `LIBRIME_PREFIX` 拷进 `Contents/Frameworks` 并改 id 的。

**注意这条仍是构建期依赖**：新机器上还是得先有一份 librime 1.17.0（Nix 或 Homebrew 都行，
给出前缀即可），再把 `libvoice_ffi.a` 编出来（`code/voice-engine/BUILD.txt` 那段带 metal 的命令）。

## 4. 三件事实（都在这台机器上是实测过的）

### 4.1 当前产物只有 arm64

- `lipo -info code/Client/dist/Squirrel.app/Contents/MacOS/Squirrel` →
  `Non-fat file … architecture: arm64`；`codesign -dv` 写 `Mach-O thin (arm64)`。
- 链接进可执行文件的 `libvoice_ffi.a` 来自 `target/aarch64-apple-darwin/debug/`，
  归档里是 arm64 目标文件。
- `Package.swift` 只声明 `platforms: [.macOS(.v14)]`，没有第二个架构的产物；
  `code/voice-engine` 侧从未在 `x86_64-apple-darwin` 上编过，也没有任何 x86_64 数字。
- `mlx.metallib` 只有一份，由 aarch64 构建产生；它绑的是 Metal 功能的查询，
  与 CPU 架构无关，但产生它的那条构建只在 arm64 上跑过。

### 4.2 MLX 这条数值路径依赖 Metal，缺 metallib 就整条文本链不工作

- `voice-mlx` 的 `metal` 特性才引入 `mlx-rs 0.34`（`features = ["metal"]`）；
  `voice-ffi` 的 `metal` 特性才引入 `voice-mlx`。不打开它，`backend::boot` 用
  `FakeBackend`，`decoder::boot_resident` 用 `UnlinkedDecoder`：**文本保持空串，不编造**。
- `voice_engine_new(model_dir, metallib)` 在 `metal` 打开时，metallib 为空或不是普通文件
  就返回空指针（`voice-ffi/src/lib.rs`），引擎根本不会起来。包里少了
  `Contents/Resources/mlx.metallib`，语音入口就是闭的——这条是硬依赖，不是降级。
- 二进制链 `Metal.framework`、`Accelerate.framework`（MLX 的 CPU 对象引用 BNNS 与 `cblas`），
  测试二进制还要额外放行 8 个只在 arm64e 段导出的弱符号（`BUILD.txt` 的 `RUSTFLAGS` 那 8 条 `-U`）。
- 因此「有 Metal 才有一切数值」：不打开 metal 的 `cargo test --workspace` 跑的是形状图
  （`FakeBackend` 记录每次调用的形状），不是数值对照；13 条数值闸只在 `--features metal` 下跑。

### 4.3 Intel 需要另一套数值内核（现在没有，也没有测过）

MLX 的 Metal 后端只服务 Apple 芯片的 GPU；本仓库在 Intel 上没有任何可运行产物
（`libvoice_ffi.a`、`mlx.metallib`、可执行文件都只有 arm64）。Intel Mac 想跑同一条
PCM→文本 的路径，缺的不是「换个编译开关」，而是一套等价的数值实现。可选落点只有两个方向，
两个都还没做：

1. 换张量后端：自己实现 `voice-tensor` 的 `TensorOps`（CPU 上走 Accelerate / BNNS / vDSP，
   或走 x86_64 的 Metal），`voice-mlx` 保持不动、只作为第三个实现。
2. 换整条引擎：把 `voice-asr` 的算子图交给另一套推理栈。

现在产品里能跑的第二份实现只有 `FakeBackend`（只记形状、不做数值），所以「第二套内核」
这件事今天连数值都没有——不能读成已经支持 Intel。

### 4.4 换后端的落点是 `voice-tensor` 的 `TensorOps`

`code/voice-engine/voice-tensor` 是纯描述层：形状、dtype、存放方式（`Dense` 与
`Grouped { group_size, bits }`）、以及 `TensorOps` 的运算清单。它**不出现 MLX 的类型名**，
也不做数值求值。`voice-asr`/`voice-ffi` 的图只通过这层说话，`voice-mlx` 是它的一个实现。

`TensorOps` 的运算清单（`voice-tensor/src/ops.rs`）就是换后端时要凑齐的一整套：

| 类别 | 运算 |
| --- | --- |
| 造张量 | `zeros` `arange` `normal` `pcm_to_f32` |
| 形状 | `reshape` `transpose` `slice` `pad` `concatenate` `where_op` `cumsum` `gather` |
| 逐元素 | `add` `mul` `rsqrt` `exp` `sin` `cos` `clip` `gelu` `silu` |
| 带权重 | `linear`（逻辑形状 `(output, input)`，分组权重在求值时反量化） `conv2d`（核 3、步长 2、填充 1） |
| 归一与注意力 | `layer_norm` `rms_norm` `attention` `kv_write` |

两个已存在的实现：`voice-tensor/src/fake.rs:483`（`FakeBackend`，记录形状）与
`voice-mlx/src/backend.rs:396`（`MlxBackend`）。换后端的接线点在
`voice-ffi/src/backend.rs`：那里只有 `#[cfg(feature = "metal")] use voice_mlx::MlxBackend;`
与 `#[cfg(not(...))] use voice_tensor::FakeBackend;` 两条，`Active` 是个类型别名——
新增一个后端就是在这一处换 `Active`，`voice-asr` 与 `voice-ffi` 的其余代码不动。

换后端之后怎么证明没走样：三道闸的资产与判据都在 `voice-parity`，容差口径只在
`voice-parity/src/tolerance.rs` 一处定义（容差 = 参考绝对值上界 × 2^-11 × 倍数，倍数按
f16 的 2^-11 分辨率来）。换实现时**不改焦距**：仍用同一份参考资产比较，梅尔闸 1e-3、
编码器三处、解码器 8 个 f16 单位、token 与文本逐位。哪一条过不去就是新内核的算术问题，
不是把容差调大——这一步没有放宽容差，也不打算放。

`voice-mel` 不在 `TensorOps` 之下，它是纯 Rust 加 Accelerate 的五条 80 点 vDSP 实数 DFT
（`voice-mel/src/lib.rs` 模块头，见 062）。vDSP 在 Intel 上也有，所以梅尔这一层不是
「MLX 依赖」，但它仍然只有 macOS 版实现（`extern "C"` 直接调 `vDSP_DFT_*`）。

## 5. 装包实测（这一刀真重打重装了）

`code/scripts/install-app.sh` 是新的，顺序写死：**停进程 → ditto → `codesign --verify --deep --strict`
→ 核对包内资产**，任一步不过就退出。

| 步骤 | 实测 |
| --- | --- |
| 停进程 | `pgrep -f "$DEST/Contents/MacOS/Squirrel"` 命中后 `pkill`，0.1 秒一轮最多等 5 秒，确认进程消失才继续 |
| 打包 | `make-app.sh`（无参数，走 `code/assets` 缺省）**6.4 秒**，输出 740 M |
| 装包 | `ditto dist/Squirrel.app → ~/Library/Input Methods/RimeClient.app` **2.7 秒** |
| 签名 | `codesign --verify --deep --strict` **exit 0**；`codesign -dv` 写 `Signature=adhoc`、`Format=app bundle with Mach-O thin (arm64)` |
| 包内资产 | `Info.plist` 2791 B、`mlx.metallib` 183870464 B、`weights.safetensors` 537626967 B、`vocab.json` 2776833 B 都在 |
| 包内哈希 | `voice-assets.sha256` 七行逐个复核通过（装包脚本自己跑的这一条，输出「资产哈希与包内清单一致」） |
| 符号链接 | `Contents/Resources` 下 0 条；`Sparkle.framework` 里 9 条（framework 的 `Versions/Current` 等常规布局） |
| 装好的包还能起引擎 | `bench2` 用装好的 `Models/Qwen3-ASR-0.6B-4bit` 与 `mlx.metallib`：`engine_new 5.275 s`、冷定稿 0.612 s、暖定稿 0.470 / 0.462 s、文本「嗯。」 |

包体积前后都是 740 M（权重与 metallib 没变，变化的只是「这两份东西从哪来、怎么核对」）。

## 6. 闸与回归：数字逐位不变

```
cd code/voice-engine && cargo test --workspace --offline          # 53 通过 / 4 忽略 / 0 失败
cd code/VoiceInterface && swift test                             # 27 通过
cd code/Client && swift test                                     # 9 通过
```

打开 metal 的三套（`RUSTFLAGS` 8 条 `-U`，见 `BUILD.txt`）：

| 闸 | 这次实测 | 与 063 基线 |
| --- | --- | --- |
| `encoder_matches_reference` | 第 0 层差 3.90625e-3、末尾投影差 1.1855885e-3（窗 `[0, 26]`） | 逐位相同 |
| `decoder_prefill_matches_reference` | 7.8125e-2（7.6 / 8 个单位），容差 8.203125e-2，余量 1.05 倍 | 逐位相同 |
| `decoder_with_reference_audio_features` | 6.25e-2（6.1 / 8 个单位），余量 1.31 倍 | 逐位相同 |
| `greedy_sequence_and_text_match_reference` | 梅尔 200 帧、音频 token 26、提示 44、新生成 3（`Some(ImEnd)`） | 逐位相同 |
| `cached_step_matches_a_fresh_prefill` | 缓存第二步 1773 = 重算扩展提示 argmax | 相同 |

合计 13 通过、0 失败。最紧的一闸仍是解码器主闸（7.8125e-2 对 8.203125e-2），
**这次没碰解码器的任何算术，所以它一个数也没动**；想提余量只能查量化矩阵乘的两组与
`lm_head` 分块那两段（058 的边界），不是这一刀的事。

## 7. 仍然未知 / 边界清单

- **没有第二台干净机器上跑过**。这次的「换机器」是用 `/tmp/voice-assets-test` 换个目录模拟的：
  同一台机器、同一份 Xcode、同一个 Nix 前缀。真到另一台机器上，最先撞的是
  `LIBRIME_PREFIX`（librime 1.17.0 要先装）与 `cargo build --features metal` 那段
  （`SDKROOT` / `LIBRARY_PATH` / `MLX_SYS_SYSTEM_SDK=1` / 8 条 `-U`）。
- **Intel 什么都没做**：没编过、没测过、没有数字。第 4 节只写了「要做哪两件事、接口落在
  `TensorOps`」，读成「已支持 Intel」是错的。
- **metallib 的哈希是这条工具链的**，换 Xcode 27 → 别的版本会变；脚本默认拒绝不一致的文件，
  新机器上要显式 `--accept-new-metallib` 并把新哈希写进清单。MLX 内核本身没有 HF 镜像，
  所以「一条命令从网上下全」在这份资产上不成立。
- **包内仍带 `worker.py`（3175 字节）**：`PythonRecognitionTransport` 还在代码里，但
  `SquirrelInputController.ensureVoiceGate` 只用 `VoiceEngine` / `VoiceEnginePreheatedTransport`，
  已经没有任何调用点会启动 Python。产品路径里没有 Python，这份文件是历史遗留；
  README「在一台新电脑上如何安装」一节仍写着解释器路径与虚拟环境，那一段已过期（未改，不在本刀范围）。
- 首次按键仍付引擎创建：`engine_new` 4.8–5.3 秒（这次 bench2 是 5.275 秒），预热把它
  从第一次按键挪到 `IMKServer` 起来之后（061），并没有让它变快。
- 麦克风到屏幕的整段墙钟仍未测（探针是文件，不是真麦克风）。

## 8. 跑了什么

```
# 取资产（真下了 537 626 967 字节，七件全部核过哈希）
code/scripts/fetch-model.sh --dest /tmp/voice-assets-test --metallib-from-build
code/scripts/fetch-model.sh --verify-only                        # 4.453 s（含清单复核）

# 三条失败路径（都 exit 1，见第 1 节表）
code/scripts/fetch-model.sh --dest /tmp/voice-assets-test --verify-only      # 篡改后
code/scripts/fetch-model.sh --dest /tmp/voice-noMetallib                     # 无来源
code/scripts/fetch-model.sh --dest /tmp/voice-noMetallib --metallib /tmp/bogus.metallib

# 打包与装包
cd code/Client && swift build --product Squirrel
code/Client/scripts/make-app.sh                # 6.4 s，缺参数时走 code/assets
code/scripts/install-app.sh                    # 2.7 s，停进程 → ditto → codesign → 资产四点
codesign --verify --deep --strict "$HOME/Library/Input Methods/RimeClient.app"   # exit 0

# 闸
cd code/voice-engine && cargo test --workspace --offline
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test encoder_parity --test decoder_parity --test end_to_end_parity \
  -- --test-threads=1 --nocapture              # 13 通过 / 0 失败
cd code/VoiceInterface && swift test ; cd ../Client && swift test
/tmp/voice-abi/bench2 "$INST/Models/Qwen3-ASR-0.6B-4bit" "$INST/mlx.metallib" \
  code/voice-oracle/assets/probe_pcm.bin
```

<!-- research-complete -->
