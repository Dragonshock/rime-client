# 045-derivation-voice-mlx-metal-link

`042` 把 Metal 放在 `voice-mlx` 一箱。本文写这一箱怎么接上 `mlx-rs` 0.34，以及 `voice-ffi` 在编译开关两侧各用哪个后端。打开 `metal` 的编译已通过，产物与 `otool` 结果在 `code/voice-engine/BUILD.txt`。Squirrel 调试产物里有 `voice_engine_*`，`otool -L` 出现 `Metal.framework`。

## 1. crates.io 上的特性

`mlx-rs` 0.34.0（2026-11-04）依赖 `mlx-sys` `=0.7.0`。`mlx-sys` 声明 `links = "mlx"`。README 的版本表写明 `mlx-rs` 0.34 构建的 MLX 是 0.32.2，`mlx-sys` 这一修订钉住的 mlx-c 提交是 `a341b4925024`，其 CMake 同样钉 MLX `v0.32.2`。MSRV 是 Rust 1.88.0。本机 rustc 是 1.99.1。

crates.io 上 `mlx-rs` 0.34.0 的 feature：

| feature | 展开 |
| --- | --- |
| `default` | `accelerate`、`metal` |
| `metal` | `mlx-sys/metal` |
| `accelerate` | `mlx-sys/accelerate` |
| `metal-jit` | `metal`，再加 `mlx-sys/metal-jit` |
| `safetensors` | 可选依赖 `safetensors`、`bytemuck` |

`mlx-sys` 0.7.0 的 feature 名字相同，`metal` 与 `accelerate` 本身不再向下展开，构建脚本用 `cfg(feature = "metal")` 把 CMake 的 `MLX_BUILD_METAL` 设为 `ON`。`metal-jit` 再设 `MLX_METAL_JIT=ON`。构建依赖是 `cmake` `^0.1`、`cc` `^1`、`bindgen` `^0.72.1`。本机 cmake 是 4.4.4。

`voice-mlx` 不使用 `default`。依赖写成 `default-features = false`、`features = ["metal"]`、`version = "=0.34.0"`。因此 Accelerate 不进这次链接，`MLX_BUILD_ACCELERATE` 保持 `OFF`。`safetensors` 也不打开。权重文件不在这次配置里下载。

## 2. 系统依赖

源码构建 Metal 需要完整 Xcode，不能只用 Command Line Tools。`mlx-sys` 的构建脚本在 `MLX_SYS_SYSTEM_SDK=1` 时走 `xcode-select -p` 指向的 Xcode：用 `xcrun --sdk macosx` 取 SDK、`clang`、`clang++`，并执行 `xcrun metal --version`。本机 `xcode-select -p` 是 `/Applications/Xcode.app/Contents/Developer`，`xcrun --find metal` 能找到 `XcodeDefault.xctoolchain` 里的 `metal`，但执行失败：

```text
error: cannot execute tool 'metal' due to missing Metal Toolchain; use: xcodebuild -downloadComponent MetalToolchain
```

`metal-jit` 同样要这支 Metal 编译器。README 写明：只有改用预编译 MLX 时，Command Line Tools 才够用。预编译前缀用环境变量 `MLX_SYS_PREBUILT` 指向绝对路径，目录里要有 `include/`、`share/cmake/MLX/`、`lib/libmlx.dylib`、`lib/libjaccl.dylib`、`lib/mlx.metallib`。版本必须是 mlx-c 钉住的那一版。该库必须打开 Metal 且关闭 JIT，否则 `metal` 特性或 `metal-jit` 会被拒绝。CI 使用的轮子是 `mlx_metal-0.32.2-py3-none-macosx_26_0_arm64.whl`，其中 `libmlx.dylib` 要求 macOS 26.2 或更新；更旧的系统要换同一 MLX 版本的 `macosx_14_0` 或 `macosx_15_0` 轮子。这次没有设置 `MLX_SYS_PREBUILT`，走的是源码构建。

`MACOSX_DEPLOYMENT_TARGET` 未设置时，MLX 用 `sw_vers -productVersion` 的宿主版本。本机是 27.2.1。改部署目标要丢掉 opt-in 的 CMake 树，包括已经编过的 Metal 对象：`cargo clean -p mlx-sys`。NAX 内核需要 macOS 26.2+ 的 SDK 和同样的部署目标。要支持更旧系统时，README 的例子是 `MACOSX_DEPLOYMENT_TARGET=14.0`。

另外，本机不设 `SDKROOT` 时，`mlx-sys` 的构建脚本在链接时找不到 `-liconv`。`libiconv.tbd` 在 MacOSX SDK 的 `usr/lib`。打开 `metal` 时要把 SDK 放进链接搜索路径：

```sh
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
```

`MLX_SYS_SYSTEM_SDK` 由构建脚本编译期的 `option_env!` 读入，必须在 `cargo build` 的环境里，并且改它之后要让 `mlx-sys` 的构建脚本重新编译。

目标三元组是 `aarch64-apple-darwin`。本机已安装该 target。

## 3. metallib 放在哪里

源码构建（`mlx-sys` 0.7.0 的 `build.rs` 与 `metallib.cmake`）：

- 未设置 `MLX_RS_METAL_PATH` 时，目录是 `~/.mlx/lib/<mlx-c-key>-<平台>-<部署目标>-<配置>-<哈希>/`。
- `<mlx-c-key>` 是钉住的 mlx-c 提交的前 12 位十六进制。发布到 crates.io 的源码包没有 `.git`，就改用 mlx-c 头文件和 `CMakeLists.txt` 的 FNV 哈希的前 12 位。
- `<哈希>` 是 `SHA256(系统名 + 换行 + 部署目标 + 换行 + SDK 绝对路径 + 换行 + CMAKE_BUILD_TYPE)`。
- `metal-jit` 在 key 后面加 `-jit`，metallib 从大约 136 MB 收到大约 1.5 MB。JIT 库不能给非 JIT 构建用。进程内会缓存已编译内核，系统 Metal 着色器缓存可以加快下一次，但第一次用到的内核仍要现编。
- `MLX_RS_METAL_PATH` 把 CMake 的 `MLX_METAL_PATH` 设成给出的目录本身，不再读 `HOME`，也不追加平台、部署目标、配置或 `-jit` 后缀。不同平台、SDK、配置、JIT 模式要用不同目录。
- 构建脚本把最终目录写到 `OUT_DIR/build/mlx-metal-path.txt`。若 `mlx.metallib` 不存在，只打出 `cargo:warning`，链接仍可能继续；这次没有走到那一步，文件没有生成。

运行时和构建缓存不是同一个路径。`mlx_rs::metal::set_metallib_path` 在 Metal 初始化之前改进程全局的加载路径，对应 C API `mlx_metal_set_metallib_path`。`voice-mlx` 在引擎线程上调用它，参数是 Swift 传入的那个文件。包内位置仍是 `042` 写的 `Contents/Resources/mlx.metallib`。`VoiceEngine.bundledMetallib()` 已经从 `Bundle.main` 取 `mlx.metallib`，缺失时仍返回包内应有的路径。构建缓存在 `~/.mlx/lib/...`，可以丢；丢了之后用包内这份再交给 `set_metallib_path`。预编译 dylib 的情况不同：MLX 从 `libmlx.dylib` 旁边加载 `mlx.metallib`，`MLX_RS_METAL_PATH` 改不了轮子里的内核。

`Array` 不是 `Send`，也不是 `Sync`。GPU stream 用 `Stream::new_with_device(&Device::gpu())` 建在引擎线程上，再用 `with_stream` 包住后续运算。不能把 `Array` 交给音频线程或主线程。

## 4. Cargo 依赖与 feature

工作区成员加上 `voice-mlx`。

`code/voice-engine/voice-mlx/Cargo.toml`：

```toml
[features]
default = []
metal = ["dep:mlx-rs"]

[dependencies]
voice-tensor = { path = "../voice-tensor" }
mlx-rs = { version = "=0.34.0", default-features = false, features = ["metal"], optional = true }
```

`voice-ffi`：

```toml
[features]
default = []
metal = ["dep:voice-mlx", "voice-mlx/metal"]

[dependencies]
voice-asr = { path = "../voice-asr" }
voice-tensor = { path = "../voice-tensor" }
voice-mlx = { path = "../voice-mlx", optional = true }
```

关闭 `metal` 时 `voice-mlx` 不进入 `voice-ffi` 的依赖图，`mlx` 这个名字不会被 import。`voice-tensor` 仍然禁止 unsafe，也仍然不依赖 MLX。打开 `metal` 时只有 `voice-mlx` import `mlx_rs`。

打开 Metal 的命令：

```sh
cd code/voice-engine
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
cargo build -p voice-ffi --features metal --target aarch64-apple-darwin
```

不打开的命令是同一条去掉 `--features metal`，也不需要 `MLX_SYS_SYSTEM_SDK`。产物是 `target/debug/libvoice_ffi.a`（宿主就是 `aarch64-apple-darwin`）。`cargo rustc -p voice-ffi --lib --crate-type staticlib -- --print native-static-libs` 对这份未打开 `metal` 的归档打印：

```text
native-static-libs: -lSystem -lc -lm
```

## 5. 打开 metal 之后的链接参数

`mlx-sys` 源码构建在 `metal` 打开时向 rustc 发出这些链接指令（`build.rs`）：

- 搜索路径：`${dst}/build/lib`，以及 GGUF 用的 `${dst}/build/_deps/mlx-build/mlx/io`
- 静态库，按 `native-libraries.txt`：`mlx`、`mlxc`、`gguflib`；若 `jaccl/libjaccl.a` 存在再加 `jaccl`，并增加 `${dst}/build/jaccl`
- `-lc++`
- `-lobjc`
- `-framework Foundation`
- `-framework Metal`
- 能找到 `libclang_rt.osx.a` 时再加它的目录和 `-lclang_rt.osx`
- `accelerate` 特性才会再加 `-framework Accelerate`。这次没开

`cargo rustc -p voice-ffi --features metal --target aarch64-apple-darwin --lib --crate-type staticlib -- --print native-static-libs` 打印：

```text
native-static-libs: -liconv -lc++ -lobjc -framework Foundation -framework Metal -lSystem -lc -lm
```

`mlx`、`mlxc`、`gguflib`、`jaccl` 与 `clang_rt.osx` 的目标文件已经在这份 `libvoice_ffi.a` 里，所以这一行没有再列搜索路径和 `-lmlx`。`mlx-rs` 没有打开 `accelerate`，构建脚本也把 `MLX_BUILD_ACCELERATE` 设成 `OFF`。MLX 的 CMake 在找到 `Accelerate.framework` 时仍把该开关改回 `ON`，归档里的 `cblas` 与 BNNS 对象仍引用 Accelerate。链接 Squirrel 时要另加 `-framework Accelerate`，否则这些符号没有定义。

预编译 dylib 若以后改用 `MLX_SYS_PREBUILT`，还要能解析 `@rpath/libmlx.dylib` 和 `@rpath/libjaccl.dylib`。README 给的 rpath 是：

```toml
[target.aarch64-apple-darwin]
rustflags = ["-C", "link-arg=-Wl,-rpath,/absolute/path/to/mlx/lib"]
```

安装包里对应 `@executable_path/../Frameworks`。`DEP_MLX_LIB_DIR` 只在直接依赖 `mlx-sys` 的构建脚本里出现，而且 0.7.0 的 README 把 `DEP_MLX_LIB_DIR` 标成 0.7.0 之后尚未发布的项。源码静态链接不使用这条 rpath。这次也没有走预编译。

## 6. voice-ffi 怎样切换后端

`voice-ffi` 的 `backend` 模块：

- `metal` 关闭：`Active` 是 `voice_tensor::FakeBackend`。`boot` 只确认模型目录能读；`metallib` 指针若非空，只做一次 `metadata`，不创建 GPU stream。`new_backend` 返回 `FakeBackend::new()`。现有测试走这条路径，`voice_engine_new` 在模型目录可打开时返回非空，即使传入的 metallib 字符串不是文件。
- `metal` 打开：`Active` 是 `voice_mlx::MlxBackend`。`voice_engine_new` 在 `metallib` 为空或不是已存在文件时直接返回空指针，Swift 不创建会话。引擎线程调用 `MlxBackend::prepare`：`mlx_rs::metal::set_metallib_path`，然后 `Stream::new_with_device(&Device::gpu())`，再在这条 stream 上对长度为 1 的 `zeros::<f32>` 做 `eval`，迫使 Metal 按该路径加载着色器。成功后 stream 留在这根线程的 thread-local 里。每个新会话的 `Session::new` 用 `MlxBackend::on_this_thread()`，张量运算都包在 `with_stream` 里。`prepare` 失败时线程回复未准备好，`voice_engine_new` 返回空指针。

会话类型是 `Session<Active, H, SessionSink>`。`drive` 不在音频线程上碰张量。`Array` 留在名为 `voice-engine` 的那根线程上。

`MlxBackend` 实现 `TensorOps`。稠密线性层是把权重转置后 `matmul`，再加偏置。分组权重若带了缩放，用 `ops::dequantize`（仿射、4 bit、组大小来自 `Storage::Grouped`）再做同一个乘法；没有缩放则返回错误，不把逻辑形状当成已经反量化的数值。卷积走 `ops::conv2d`，NHWC，核 3、步长 2、填充 1 来自层参数。注意力在头数不整除时失败，否则把键值头重复到查询头数，缩放是 `1/sqrt(头维)`，mask 交给 `fast::scaled_dot_product_attention`。

未打开 `metal` 的 `cargo test --workspace` 已通过：`voice-asr`、`voice-ffi`、`voice-tensor` 的原有测试没有改断言。

## 7. Swift

`VoiceEngine` 仍用 `dlsym(RTLD_DEFAULT)` 找 `voice_engine.h` 里的七个符号，源码里不出现 `/usr/bin/python3`。符号没有被 Swift 按名字引用，普通 `-lvoice_ffi` 会被 `-dead_strip` 丢掉。这份打开 `metal` 的归档里同时有 Rust `compiler_builtins` 与 `libclang_rt.osx.a` 的目标文件，`-force_load` 会报重复符号。Squirrel 改用 `-Xlinker -u` 拉住六个 C 符号，再把归档路径交给 ld：

```text
-L/nix/store/i3r73fca2jfd0m75xr1zlfmdp3zblhya-librime-1.17.0/lib
-lrime
-Xlinker -u -Xlinker _voice_engine_new
-Xlinker -u -Xlinker _voice_engine_free
-Xlinker -u -Xlinker _voice_engine_set_callback
-Xlinker -u -Xlinker _voice_session_begin
-Xlinker -u -Xlinker _voice_session_feed
-Xlinker -u -Xlinker _voice_session_finish
-Xlinker /Users/dragon/repo/tools/rime-client/code/voice-engine/target/aarch64-apple-darwin/debug/libvoice_ffi.a
-liconv -lc++ -lobjc -framework Foundation -framework Metal -framework Accelerate
-lSystem -lc -lm
```

`swift build --product Squirrel` 已通过。产物 `code/Client/.build/out/Products/Debug/Squirrel` 里有：

```text
_voice_engine_new
_voice_engine_free
_voice_engine_set_callback
_voice_session_begin
_voice_session_feed
_voice_session_finish
```

`otool -L` 这份产物出现 `Metal.framework`，也出现 `Accelerate.framework`。链接时 ld 警告 MLX 目标文件的最低系统是 27.2.1，Squirrel 的部署目标是 14.0。`VoiceInterface` 的测试宿主不链这个归档，`VoiceEngine.linkStatus()` 在测试里仍是「引擎未链入」。那是测试进程，不是 Squirrel。

## 8. 这次编译

未打开 `metal` 的 `cargo test --workspace` 通过，没有编译 MLX。打开 `metal` 的命令是：

```sh
cd code/voice-engine
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
cargo build -p voice-ffi --features metal --target aarch64-apple-darwin
```

exit 0。产物是 `code/voice-engine/target/aarch64-apple-darwin/debug/libvoice_ffi.a`。同次生成的 `mlx.metallib` 在 `mlx-sys` 构建目录的 `out/build/lib/mlx.metallib`。用上文的链接参数做成的 Squirrel 产物上，`otool -L` 出现 `Metal.framework`。详细路径在 `code/voice-engine/BUILD.txt`。

<!-- derivation-complete -->
