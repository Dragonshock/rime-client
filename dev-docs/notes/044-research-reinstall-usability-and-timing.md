# 044-research-reinstall-usability-and-timing：重装后的签名、输入源与墙钟

测量日：2026-11-13。安装包路径是 `/Users/dragon/Library/Input Methods/RimeClient.app`。没有调用 inject，没有改写 `/Library/Input Methods/Squirrel.app`。

## 1. 事实

### 1.1 签名

命令：`codesign --verify --deep --strict ~/Library/Input Methods/RimeClient.app`。

退出码 0。`codesign -dv` 读到：

| 字段 | 值 |
| --- | --- |
| Executable | `/Users/dragon/Library/Input Methods/RimeClient.app/Contents/MacOS/Squirrel` |
| Identifier | `im.dragonshock.inputmethod.RimeClient` |
| Format | app bundle，Mach-O thin，arm64 |
| Signature | adhoc |
| TeamIdentifier | not set |

### 1.2 TIS

用一次性 Swift 调用 `TISCreateInputSourceList(nil, true)`，再读 `kTISPropertyInputSourceID`、`kTISPropertyLocalizedName`、`kTISPropertyInputSourceIsEnabled`、`kTISPropertyInputSourceIsSelectCapable`。`true` 表示列表包含未启用的源。本次没有执行 `--enable-input-source`，也没有改启用位。

`im.dragonshock.inputmethod.RimeClient.Hans`：

| 属性 | 值 |
| --- | --- |
| localizedName | RimeClient |
| enabled | false |
| selectable | true |
| category | TISCategoryKeyboardInputSource |
| type | TISTypeKeyboardInputMode |

同一列表里还有父源 `im.dragonshock.inputmethod.RimeClient`（localizedName `RimeClient`，enabled false，selectable false）和 `im.dragonshock.inputmethod.RimeClient.Hant`（localizedName `RimeClient Traditional`，enabled true，selectable true）。

包内本地化文件只有 `en.lproj` 与 `zh-Hans.lproj`。两份都把 Hans 的显示字符串写成 `RimeClient`。`zh-Hans.lproj` 把 Hant 写成 `RimeClient 繁体`；`en.lproj` 写成 `RimeClient Traditional`。TIS 对 Hant 返回的是后者。`zh-Hant.lproj` 不存在。当时 `AppleLocale` 是 `zh_HK`，`AppleLanguages` 首项是 `zh-Hant-HK`。

### 1.3 已安装二进制里的语音入口

二进制：`RimeClient.app/Contents/MacOS/Squirrel`。

字节搜索：`/usr/bin/python3` 出现 0 次。`python3` 出现 1 次，紧挨着的 C 字符串是 `voice-worker`、`.venv`、`bin`、`python3`，不是 `/usr/bin/python3`。

`SquirrelInputController.ensureVoiceGate` 的机器码调用的是 `VoiceEngine` 的元类型、`VoiceEngine.bundledModelDirectory`、`VoiceEngine.bundledMetallib` 和 `VoiceEngine.init(modelDirectory:metallibPath:onReply:)`，随后交给 `VoiceHandleGate`。这条路径没有调用 `PythonRecognitionTransport` 的构造符号。`VoiceEngine` 这个名字在二进制里出现 7 次。字符串 `引擎未链入` 在二进制里出现 1 次，旁边是 `voice_engine_new`、`voice_engine_free` 这些供查找的 C 字符串。

`nm` 里没有已定义或未定义的 `voice_engine_*` 符号。`otool -L` 没有列出 voice 或 mlx 的动态库。包内没有 `metallib`，也没有 `libvoice_ffi.a`。`Contents/Resources/worker.py` 仍在包里（3175 字节），它不是上述 `ensureVoiceGate` 的调用目标。

源码侧同一入口是 `code/Client/sources/SquirrelInputController.swift` 里构造 `VoiceEngine`；`code/VoiceInterface/Sources/VoiceInterface/VoiceEngine.swift` 在 `VoiceSymbols.load()` 失败时把原因写成 `引擎未链入`，并写明不启动解释器进程。这次没有另起进程去打日志，所以没有一条运行中的日志行可引用。

### 1.4 voice-mlx 与 Metal

`code/voice-engine/BUILD.txt` 首行是「voice-mlx 尚未链接 Metal。」同文写明：`voice-ffi` 不依赖 `mlx-rs`；张量走 `voice-tensor` 的假后端，只记录形状，不在 GPU 上求值；`voice-mlx` 没有编进 `libvoice_ffi.a`，也没有打开 metal 特性。

已安装主程序只弱链接 `/usr/lib/swift/libswiftMetal.dylib`。这是 Swift 运行库，不是 voice-mlx，也不是一份 metallib。

### 1.5 墙钟

工作目录 `code/voice-engine`。命令：

`/usr/bin/time -p cargo test -p voice-asr --lib tests::feed_32000_then_finish -- --exact`

结果：`tests::feed_32000_then_finish ... ok`；cargo 打印该测试 `finished in 0.00s`。`/usr/bin/time`：real 0.02 秒，user 0.01 秒，sys 0.00 秒，退出码 0。配置是未优化的 test profile，这次没有重新编译（`Finished test profile ... in 0.00s`）。该测试在 `voice-asr` 里用假后端记录卷积与注意力的形状，喂入 32000 个零样本后再 `finish`。它不是麦克风，也不是 Metal 上的识别。

已安装的 RimeClient 可执行文件：

`/usr/bin/time -p ~/Library/Input Methods/RimeClient.app/Contents/MacOS/Squirrel --help`

打印帮助后退出。real 0.02 秒，user 0.00 秒，sys 0.00 秒，退出码 0。没有留下新进程。`code/Client/sources/Main.swift` 里 `--help` 在进入 `app.run()` 之前返回。

系统里原有的 Squirrel，只执行、不覆盖：

`/usr/bin/time -p "/Library/Input Methods/Squirrel.app/Contents/MacOS/Squirrel" --help`

打印帮助后退出。real 0.01 秒，user 0.00 秒，sys 0.00 秒，退出码 0。测量时已有进程 96157 在跑该二进制（启动早于这次 `--help`，累计 CPU 时间约 3 分 7 秒）。这次 `--help` 没有替换那个进程，也没有再留下一个同路径进程。

### 1.6 麦克风到文字

未知。`BUILD.txt` 写明 voice-mlx 尚未链接 Metal，安装包里也没有识别用的 metallib 与 `voice_engine_*` 符号。假后端 `feed_32000_then_finish` 的 0.02 秒不拿来充当识别时延。

## 2. 机制

`codesign --verify --deep --strict` 只检查已嵌入的签名与密封资源是否自洽。这次自洽的签名是 adhoc，没有 TeamIdentifier。校验通过不等于另一台机器会接受这个包。

TIS 的 enabled 与 selectable 是两条属性。Hans 这次是 selectable 真、enabled 假：源在「包含未启用项」的列表里，并且被标成可以选用，但启用位是关的。本次没有去翻这个位，所以不知道菜单里实际能不能切到它。

语音入口在已安装主程序的 `ensureVoiceGate` 上是 `VoiceEngine`。`VoiceEngine` 按 C 字符串查找 `voice_engine_new` 一类符号；符号不在进程里时，源码把原因固定成「引擎未链入」，不启动解释器。二进制里没有 `/usr/bin/python3`，与这条失败路径一致。`PythonRecognitionTransport` 的类型仍留在二进制里，但上述构造序列没有调用它。

`--help` 在建立 `IMKServer` 与 `app.run()` 之前结束，所以 0.02 秒与 0.01 秒只覆盖参数分支，不覆盖输入法会话挂上系统之后的时间。

## 3. 边界

- 墙钟来自 `/usr/bin/time` 的十分之一秒显示和 cargo 的 `0.00s`。没有更细的采样，不把它们写成毫秒。
- `feed_32000_then_finish` 只证明假后端上的形状记录能跑完。它不加载 0.6B 权重，不走麦克风，不走 Metal。
- 没有测量从按住说话到 `setMarkedText` 的时间。该数是未知，不是 0.02 秒。
- 没有启用或选择 Hans，没有对系统 Squirrel 做除 `--help` 以外的动作。
- 包内仍有 `worker.py`。这只说明资源文件还在，不说明 `ensureVoiceGate` 会用 `/usr/bin/python3`。

## 4. 未闭合

- 真实麦克风到文字的时延：未知。voice-mlx 未链接 Metal。
- Hans 在 enabled 保持 false 时，输入菜单是否列出它：这次没有打开菜单，未知。
- 不带 `--help`、直到 `IMKServer` 挂上之后的启动墙钟：未知。那条路径会进入 `app.run()`。

<!-- research-complete -->
