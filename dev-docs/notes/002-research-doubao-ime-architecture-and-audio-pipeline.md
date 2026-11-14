# 豆包输入法（macOS）逆向调研（一）：包结构、进程拓扑、音频采集与 IMK 上屏机制

> **关联索引**：`dev-docs/notes/001-research-voice-ime-index.md`
> **逆向样本**：`/Library/Input Methods/DoubaoIme.app`（`CFBundleShortVersionString = 1.1.0`，`Wave Build Version = 1.1.0.5`，`Wave Build Version Number = 1010005`，`BuildMachineOSBuild = 24G90`，`DTSDKName = macosx26.0`，`DTXcode = 2600`）
> **方法论声明**：本文全部证据均来自本地已安装样本的 Mach-O 二进制静态分析（`mcp__rea` 派发元数据提取与 Ghidra 反编译、`xcrun llvm-objdump --macho -d` ARM64 反汇编、`xcrun nm` + `swift-demangle` 符号还原、`otool` 段/依赖表解析、`plutil` 与 `afinfo` 资源检查）。文中所有技术形态归纳与面向鼠须管（Squirrel / `rime-client`）的架构映射均标记为 **候选（待用户确认）**。

---

## 1. 事实与来源：完整包结构与 Mach-O 二进制资产盘点（Package Topology & Binary Inventory）

### 1.1 应用包目录拓扑（Bundle Directory Hierarchy）

`/Library/Input Methods/DoubaoIme.app` 采用单输入法主包内嵌独立设置应用（`Contents/Applications/DoubaoImeSettings.app`）、独立自动更新守护程序（`Contents/MacOS/DoubaoImeUpdater`）以及 11 个私有框架与动态库（`Contents/Frameworks/*`）的复合目录结构：

```text
// 源：/Library/Input Methods/DoubaoIme.app 包顶层目录结构
/Library/Input Methods/DoubaoIme.app/Contents
├── Info.plist / PkgInfo / embedded.provisionprofile
├── MacOS/{DoubaoIme, DoubaoImeUpdater}         # 输入法主进程与独立后台更新器
├── Applications/DoubaoImeSettings.app          # 独立设置与词库/账号/语音配置 GUI 进程
├── Frameworks/
│   ├── OimeEngine.framework / OimeCommon.framework
│   ├── libDoubaoImeDylib.dylib / libime_net_sdk.dylib / libsscronet.dylib
│   ├── libTTNetDownloaderCrossPlatform.dylib / libapplogrs.dylib / libMetaSecML.dylib
│   └── Parfait.framework / WCDBSwift.framework / sqlcipher.framework / SwifterSwift.framework
└── Resources/{Assets.car, update.sh, RecordingSounds/*.caf, WaveAssetsModule.bundle}
```

### 1.2 全量 Mach-O 二进制清单与哈希校验（Mach-O Binary Inventory）

包内所有可执行文件与动态库均为 `x86_64` + `arm64` 通用二进制（Universal Binary）。下表给出全部 16 个核心 Mach-O 二进制的通用文件大小、通用 SHA-256、提取后的 `arm64` 薄切片（Thin Slice，位于 `/tmp/doubao-thin-arm64/`）大小与 SHA-256，以及各自的符号剥离状态与模块职责：

| 二进制相对路径（相对于 `DoubaoIme.app/Contents/`） | Universal 大小 (Bytes) | Universal SHA-256 | `arm64` 切片大小 (Bytes) | `arm64` 切片 SHA-256 | 技术栈与职责定位 |
| :--- | ---: | :--- | ---: | :--- | :--- |
| `MacOS/DoubaoIme` | 44,768,736 | `5b48748fa6394811cba0f8c05905ffebb9a2cd02faf6d47b26deb3d9b86247e0` | 21,962,208 | `a6a3ac886111eda4bb271ddfd37769fd94ecdd57d04112d77a60e32d74e36cbf` | Swift + ObjC + C++（静态内嵌 `OimeAudio`、`DoraMacSDK`、`OimeOfflineModel`、`SAMI` 语音 SDK、Flute/MNN/Panther 端侧 ASR 推理引擎） |
| `Frameworks/OimeEngine.framework/Versions/A/OimeEngine` | 18,789,456 | `7ba480eaabbd4d99f6d63917d9fc208c81e8b1d1c38db22afa3795a46ceb8ed3` | 8,811,600 | `4659d574d5a0042febb9264e4eba077c7a1296296e64198c07faafa12f696507` | Swift + C++（`OimeEngine` 单例、`WaveEngineThread`、C++ 拼音解码器与 `marisa-0.2.6` 词图检索） |
| `Frameworks/OimeCommon.framework/Versions/A/OimeCommon` | 1,277,664 | `c26d8e2451737fc78d38aa819bb62b04594f5d894b5a9669b335e83e8ee8831c` | 622,304 | `c810ce1106262bcedfaf9448ee85b2be4afa1d6137d27560d9fd8ea07e9e06ed` | Swift（`AppInfo`、`PathManager`、`DoubaoImeInstanceLock`、`ASRResult`、`ASRMetersGenerator`、`OimeFocusAwareFloatingPanel`） |
| `Frameworks/libDoubaoImeDylib.dylib` | 282,032 | `fcaf6c7a42192049b44b67b2a180e1aac777d21813d0eb5c8c7ea0eff025ceee` | 134,576 | `847238388e8b981b41b0b4d141aecce8119a784b5c5577f4cb41173731ae689a` | ObjC++（`ImeEngine` 类方法桥接、`Obric Keyboard Engine` C++ 封装、`ClipBoardUtil`、`DoublePressHelper`） |
| `Frameworks/libime_net_sdk.dylib` | 15,657,424 | `8d9bb637e613d991b7dbab1e807b2aa7f7df0a063852811eefe0cc172a4a1f90` | 7,645,648 | `9233ce2bd5d1740a5a36f476624660a59db6ce4a83bc8fc0b4df5db77790107b` | Rust + C FFI（`ime_net_sdk_*` C 导出接口、Tokio 异步运行时、Reqwest/Rusls、跨端剪贴板与云端请求调度） |
| `Applications/DoubaoImeSettings.app/Contents/MacOS/DoubaoImeSettings` | 17,382,304 | `8e62bd44104ea60e8126fdc66217f4a1a16f98192b88464d0809fcd4b20f36b4` | 8,354,720 | `8164d48d49441f4934877d4b1cc3a4f89aeaeca9961411405cff1076fc2b6417` | Swift + SwiftUI/AppKit（独立设置进程，链接 `OimeCommon`、`libDoubaoImeDylib.dylib` 与 `libtt_account_sdk.dylib`） |
| `MacOS/DoubaoImeUpdater` | 348,352 | `15776754e80764404d88c902d795d0c6ee26d02121aa737e54cfcdf018581aff` | 168,128 | `86c807254da209a16066d760025233fa889f8e148334626b5c6a98a2520fb63f` | Swift（后台静默更新检查、DMG 签名校验与 `update.sh` 触发器） |
| `Frameworks/libsscronet.dylib` | 17,420,160 | `abee40a3a03a862d5c85ba318320047ba7e9b078f8aa032f30ee0ea811a584b4` | — | — | C++（TTNet / Cronet QUIC + HTTP/3 + WebSocket 底层传输引擎） |
| `Frameworks/libMetaSecML.dylib` | 3,437,232 | `0885bc9411cf25447e20d78a889ccb01c7f33c04596c18b6a27b472a23a15af7` | — | — | C/C++（字节跳动终端安全风控与请求签名 SDK） |
| `Frameworks/libapplogrs.dylib` | 3,201,472 | `1b56c7edf1d93c5d871fb876a78b36841cc857d1167d4d0f70fb72a85f4b2002` | — | — | Rust（AppLog 埋点聚合与批量上报引擎） |
| `Frameworks/Parfait.framework/Versions/A/Parfait` | 2,004,240 | `70f98583c9cf9d78517b5e77f9e3cac7ba8f7d4edd1133a9a7395d85601a892b` | — | — | ObjC/C++（Mach 异常捕获、崩溃堆栈回溯与性能监控） |
| `Frameworks/sqlcipher.framework/Versions/A/sqlcipher` | 1,876,608 | `0fa2c2160cf5603e724a55884ae06cb7812e5cf6f1459abe629a13c16948df28` | — | — | C（SQLCipher AES-256 加密 SQLite 引擎） |
| `Frameworks/WCDBSwift.framework/Versions/A/WCDBSwift` | 1,463,424 | `f2dd89ae7759771dd5e788ec96e56a5a4c2a24b5c9abbd6231ada790ff5e61c8` | — | — | Swift/C++（腾讯 WCDB ORM 框架） |
| `Frameworks/libswift_Concurrency.dylib` | 1,124,608 | `fd2a63ab53efa5a445b6b4af576cf89e92bc5d5dd76a2eb60c6c5b0eced74683` | — | — | Swift（Swift Concurrency 向后兼容运行时） |
| `Frameworks/SwifterSwift.framework/Versions/A/SwifterSwift` | 906,960 | `6d085e8860b8b1793d68bd0805af03a21f10d66caa5a370bd181b1708f371208` | — | — | Swift（标准库与 AppKit 扩展工具集） |
| `Frameworks/libTTNetDownloaderCrossPlatform.dylib` | 721,872 | `7612d3608712bb4c4ecf6e8278bf6bf9c12d8d8943ce428eb6ba5d9f477db52c` | — | — | C++（断点续传与分片校验下载器） |

### 1.3 主程序 `DoubaoIme` 的 Mach-O 段布局与静态内嵌模块边界（Mach-O Section Layout & Embedded Source Map）

对 `/tmp/doubao-thin-arm64/DoubaoIme` 执行 `otool -l` 与源码路径字面量扫描（共提取出 359 个内嵌源文件路径，见 `/var/folders/l0/8zb79jwj4ss92hxxm41yb7qh0000gn/T/pi-bash-d8b3ea66462a808f.log`），可知 `DoubaoIme` 主程序（`__TEXT,__text` 段范围 `0x100002dc0..0x100e3fccb`，共 14,929,676 字节）将四大代码域静态链接进了同一个可执行文件中：

1. **Swift 输入法应用与 UI/IMK 协调层**（`0x100002dc0..0x10027a000`，源码根路径 `/Users/u8dham1tjvu3mz0c/114603/obric-ime-engine4mac/DoubaoIme/...`）：
   - `main.swift`（`0x1000037e0..0x10000dfb4`，38 处引用）：进程锁、Bundle 校验、`setupIMKServer()`、Sparkle/自研更新调度。
   - `DoubaoImeASRShortcutCoordinator.swift`（`0x100016f00..0x1002102bc`，137 处引用）：全局语音快捷键（Fn / 右 Command / 长按与单击切换）状态机与输入源自动挂载。
   - `ASR/ASRTipPresenter.swift`（`0x10003735c..0x10026a278`）、`CandidatesWindow.swift`（`0x100049ea4..0x1001f3fe8`）、`CandidatesPanelView.swift`（`0x10005a3d4..0x100063770`）。
   - `DoubaoImeInputController.swift`（`0x10004bf70..0x10005a000` 及 `0x1001a08fc..0x1002229b8`，共 225 处引用）：`IMKInputController` 核心子类，实现键盘输入、流式语音上屏、智能整理（SmartOrganize）与剪贴板/常用语回退。
   - `ASR/AudioManager.swift`（`0x1000e311c..0x1000f4f5c`）、`ASR/ASRAudioEngine.swift`（`0x1000e7e68..0x100102000`）、`ASR/AudioDeviceManager.swift`（`0x100102710..0x100113cf4`）、`ASR/ASRManager.swift`（`0x10014d000..0x10015b000`）。
2. **Dora 蓝牙耳机私有音频采集模块**（`0x10027b000..0x10028c000`，模块名 `DoraMacSDK`）：
   - 包含 `DoraAudioCaptureController.swift`（`0x10027c118..0x10027ca7c`）、`BLEAudioDeviceManager`、`BLEAudioCaptureSession` 与 `DoraOpusDecoder`（基于 CoreBluetooth GATT 特征值接收 Ola Friend 智能耳机 Opus 音频流并解码）。
3. **Flute / Panther / MNN 端侧神经网络语音推理引擎**（`0x1004276cc..0x100e3a5cc`，源码根路径 `/Users/hcoymu818v3occgv/82635/flute/flute/...`）：
   - 算子与推理后端：`nn/compatibility.cc`（`0x1004276cc`）、`nn/tensor.cc`（`0x1004282bc`）、`nn/panther_inference.cc`（`0x10042e754`）、`nn/mnn_inference.cc`（`0x100437af0..0x100439650`）。
   - 端侧流式 ASR 与投机解码（EAGLE Draft）MNN 模型加载（`0x100452c8c..0x10047ce18`）：硬编码加载 `/encoder_stream.mnn`（`0x100453124`）、`/encoder.mnn`（`0x10047c8c4`）、`/decoder.mnn`（`0x100452c8c`）、`/eagle_draft.mnn`（`0x1004532ec`）、`/eagle_proj.mnn`（`0x10047ce18`）、`/embed.mnn`（`0x10047cacc`）、`/logit.mnn`（`0x10047c958`）与 `/logit_topkv_1.mnn`（`0x10047ca58`）。
   - 声学前端特征提取：`feature/feature_extractor.cc`（`0x10058427c`）、`feature/panther_feature_extractor.cc`（`0x100586d90`）、`feature/mel_bank_feature_extractor.cc`（`0x1005878e0`）、`feature/subband_feature_extractor.cc`（`0x100587c24`）及 `feature/internal/panther_feature_extractor_kernel.cc`（`0x100588d78`）。
   - 分词器：`module/asr/utils/gpt2_tokenizer.cc`（`0x10058f244..0x1005930c4`）与 `module/asr/utils/hf_tokenizer.cc`（`0x1005948d4..0x100596814`）。
4. **`OimeAudio` 与 `OimeOfflineModel` 静态模块**（`0x100bb1500..0x100be0000`，源码根路径 `/Users/u8dham1tjvu3mz0c/114603/obric-ime-engine4mac/Modules/...`）：
   - `Modules/OimeAudio/Classes/Core/UI/Typewriter/ASRTypeWriter.swift`（`0x100bb1500..0x100bb3500`）：流式识别文本打字机平滑推进器。
   - `OimeOfflineModel.ASROfflineModelManager`：端侧离线语音模型下载、解压、MD5 校验与生命周期管理。

### 1.4 签名、权限声明（Entitlements）与资源资产规格

1. **代码签名与非沙盒权限配置**：
   - 签名主体：`Developer ID Application: Beijing Chuntian Zhiyun Technology Co., Ltd. (96L78H6LMH)`，启用 Hardened Runtime（`flags=0x12000(library-validation,runtime)`）。
   - Entitlements 字典：
     - `com.apple.security.app-sandbox = false`（脱离 App Sandbox 运行，可直接访问全局 AXUIElement 辅助功能 API、CGEventTap、文件系统锁与本地 Unix/Mach 通信）。
     - `com.apple.security.device.audio-input = true`（允许在 Hardened Runtime 下调用 `AVCaptureSession` / CoreAudio 采集麦克风）。
     - `com.apple.security.files.user-selected.read-only = true`。
   - `Info.plist` 隐私权限声明：
     - `NSMicrophoneUsageDescription = "我们需要访问您的麦克风来进行语音识别。"`
     - `NSBluetoothAlwaysUsageDescription = "我们需要访问蓝牙信息以识别 Ola Friend 智能耳机。"`
2. **提示音资源（`Contents/Resources/RecordingSounds/`）**：
   - 包含 3 个 CoreAudio `.caf` 音效文件（`dt-begin.caf`、`dt-confirm.caf`、`dt-cancel.caf`），由 `afinfo` 测得统一编码规格为：双声道立体声（2 ch Stereo L/R）、采样率 `48000 Hz`、`lpcm` 24-bit 小端有符号整型（`source bit depth: I24`，比特率 `2,304,000 bps`），时长分别为 `0.314625 s`（15,102 帧 / 90,612 字节）、`0.339229 s`（16,283 帧 / 97,698 字节）与 `0.339229 s`（16,283 帧 / 97,698 字节）。
3. **视觉与引擎资源包**：
   - `Contents/Resources/WaveAssetsModule.bundle`：内含 `Assets.car` 及 10 个 Lottie 动画 JSON（`smart_organize_loading_lottie.json`、`star_sparkle_lottie.json`、`voice_guide_realtime_on_screen_{light,dark}.json`、`voice_guide_mixed_dialect_{light,dark}.json`、`voice_guide_smart_correction_{light,dark}.json`、`super_transfer_{light,dark}.json`）。
   - `Contents/Frameworks/OimeEngine.framework/Versions/A/Resources/OimeEngineResources.bundle`：内含 22 个哈希命名的拼音/语言模型数据文件（含架构特化的 `d9cce8a55257.micro` / `.micro.x64`、`3aa2ea70d0b2.micro` / `.micro.x64`）以及 `asr/` 子目录下的 3 个声学/VAD 资源文件（`90afacc4f089`、`2c57d1948fe3`、`281be8420195`）。

---

## 2. 核心机制（一）：进程拓扑、`IMKServer` 生命周期与 `DoubaoImeInputController` 交互机制

### 2.1 进程启动、单实例文件锁与 `setupIMKServer()` 引导链路

`DoubaoIme` 主进程入口位于 `0x100002dc0`（`DoubaoIme/main.swift`）。在初始化任何 UI 或 `IMKServer` 之前，入口函数在 `0x100002fe0..0x100003014` 处强制获取基于当前用户 UID 的单实例文件锁：

```asm
// 源：/tmp/doubao-thin-arm64/DoubaoIme (main.swift, 0x100002fe0..0x100003000)
100002fe0: bl  0x100e45da4 ; _getuid
100002fe4: mov x8, x22
100002fe8: bl  0x100e407dc ; OimeCommon.DoubaoImeInstanceLock.lockFileURL(userID: UInt32) -> Foundation.URL
100002fec: mov x0, x22
100002ff0: mov x21, #0x0
100002ff4: bl  0x100e407e8 ; OimeCommon.SharedInstallFileLock.acquire(at: Foundation.URL) throws -> Handle
100002ff8: cbnz x21, 0x100003038 ; 若抛出 LockError 则跳转至异常退出分支
100002ffc: mov x20, x0     ; 持有 LockHandle 直至进程结束
```

上述汇编展示了 `DoubaoIme` 防止多实例竞争的机制：调用 `_getuid()` 生成用户隔离的锁文件路径（通过 `OimeCommon.DoubaoImeInstanceLock.lockFileURL(userID:)`），再由 `OimeCommon.SharedInstallFileLock.acquire(at:)` 持有文件锁句柄 `x20`。随后引导流程依次执行：

1. **`validateBundleId()`（`0x10000b300..0x10000b58c`）**：校验主包 `CFBundleIdentifier` 是否匹配预期值 `com.bytedance.inputmethod.doubaoime`。
2. **`setupIMKServer()`（`0x10000b590..0x10000b89c`）**：从 `Bundle.main.object(forInfoDictionaryKey: "InputMethodConnectionName")` 读取连接名（`Info.plist` 中配置为 `"DoubaoIme_Connection"`；若缺失则回退拼接 `"\(OimeCommon.AppInfo.shared.bundleName)_Connection"`），随后实例化 `IMKServer`：

```asm
// 源：/tmp/doubao-thin-arm64/DoubaoIme (setupIMKServer() in main.swift, 0x10000b84c..0x10000b870)
10000b84c: adrp x8, 5124         ; 0x10140f000
10000b850: ldr  x0, [x8, #0x290] ; _OBJC_CLASS_$_IMKServer
10000b854: bl   0x100e46008      ; _objc_allocWithZone
10000b858: adrp x8, 5117         ; 0x101408000
10000b85c: ldr  x1, [x8, #0x7d0] ; selector: initWithName:bundleIdentifier:
10000b860: mov  x2, x19          ; name: "DoubaoIme_Connection"
10000b864: mov  x3, x20          ; bundleIdentifier: "com.bytedance.inputmethod.doubaoime"
10000b868: bl   0x100e460ec      ; -[IMKServer initWithName:bundleIdentifier:]
10000b86c: bl   0x100e4614c      ; _objc_release
```

在 `0x10000b868..0x10000b86c` 中，`setupIMKServer()` 调用 `-[IMKServer initWithName:bundleIdentifier:]` 后立即对返回值执行了 `_objc_release`（对应 Swift 代码 `_ = IMKServer(name: connectionName, bundleIdentifier: Bundle.main.bundleIdentifier)`），依赖 `InputMethodKit` 框架内部全局表持有 `IMKServer` 实例。
3. **`Bootstrap preDid start` 与 `runAfterGetDid(oimeBootScanMs:)`（`0x1000037ec..0x10000c200`）**：异步初始化设备标识（DID）、启动 `OimeEngine.shared`、注册全局语音快捷键协调器 `DoubaoImeASRShortcutCoordinator`、并启动后台更新定时器 `startBackgroundUpdateCheckScheduler()`（`0x100005840`）。

### 2.2 `DoubaoImeInputController` 类层次、协议一致性与方法派发表

在 `Info.plist` 中，`InputMethodServerControllerClass` 与 `InputMethodServerDelegateClass` 均绑定为 `DoubaoImeInputController`。通过 `mcp__rea` 提取其 ObjC/Swift 元数据（见 `/tmp/doubao-thin-arm64/classes-dump.txt` 与 `pi-bash-8b540148b3760d8b.log`）：

- **ObjC 运行时类名**：`DoubaoImeInputController`（继承自 `IMKInputController`，类对象地址 `0x101414db8`，实例大小 `376` 字节，包含 `29` 个实例变量 ivars）。
- **实现的 7 个 Swift/ObjC 协议（Witness Tables）**：
  1. `InputDelegate`（Witness Table `0x10020ef68..0x10020f1e4`，16 个槽位）：接收底层 `OimeEngine` 候选词、预编辑拼音与提交回调。
  2. `ASRResultReceiver`（14 个槽位）：接收 `ASRManager` 推送的流式语音识别状态、部分识别结果（Partial Result）、最终结果（Final Result）、智能整理（SmartOrganize）改写结果与 VAD 事件。
  3. `ASRGlobalContextSettleable`（Witness Table 含 `0x100207734`，15 个槽位）：处理跨应用或无焦点场景下语音会话的上下文结算与所有权交接。
  4. `ASRShortcutEventPreflightHandling`（1 个槽位）：处理语音快捷键按下前的焦点与输入源预检。
  5. `CandidatesWindowInteractionDelegate`（6 个槽位）：处理候选栏点击、展开、设置与反馈交互。
  6. `DoubaoImeDistributedObserverDelegate`（7 个槽位）：响应来自 `DoubaoImeSettings.app` 的 `NSDistributedNotificationCenter` 跨进程配置变更通知。
  7. `KeyboardGlobalContextServicesDelegate`（Witness Table `0x10020f2c8..0x10020f42c`，6 个槽位）：全局键盘上下文感知回调。

- **暴露给 `IMKServer` 的核心 ObjC 选择器与虚拟地址**：
  - `initWithServer:delegate:client:`：`0x1001a23f0`（注册 `onWaveIMEFlagsChanged:` 通知监听，弱引用绑定 `-[IMKInputController client]`，并将当前控制器注册至 `OimeInputControllerManager`）。
  - `activateServer:`：`0x1001a5c14`（识别宿主 Bundle ID，过滤 `com.apple.systemuiserver` 与 `com.apple.controlcenter`，同步输入状态并挂载活动 `ASRResultReceiver`）。
  - `deactivateServer:`：`0x1001a6518`（包含精细的语音防中断保护窗口，见 2.3 节）。
  - `handleEvent:client:`：`0x1001aa9c4`（键盘事件主入口，拦截语音态按键中断 `keyboardInterrupt`、快捷键与候选词导航）。
  - `commitComposition:`：`0x1001a5b98`
  - `candidates:`：`0x1001a7180`
  - `menu`：`0x1001b95c0` / `onMenuItemClick:`：`0x1001b9e90` / `openSettings`：`0x1001ba324`

### 2.3 `deactivateServer:` 防误断保护与全局语音输入源自动抢占（Receiver Recovery）

macOS 输入法在弹窗（如系统麦克风权限弹窗、系统菜单栏交互、或按下 Fn 键触发系统级行为）时，系统会向当前 `IMKInputController` 发送 `deactivateServer:`。若按常规输入法逻辑在 `deactivateServer:` 中无条件清理状态并终止语音会话，会导致语音输入被意外掐断。逆向 `0x1001a64a4..0x1001a70d4`（`DoubaoImeInputController.deactivateServer(_:)`）与 `0x10003eb14..0x10003f170`（`DoubaoImeASRShortcutCoordinator`）揭示了豆包输入法的两项关键状态机设计（**候选（待用户确认）**）：

1. **`deactivateServer(_:)` 四重豁免门控（Guard Clauses，`0x1001a6518..0x1001a70d4`）**：
   - **Fn 快捷键忽略窗口**：检查 `NSProcessInfo.processInfo.systemUptime` 与最近一次 Fn 触发时间戳的差值，若在忽略窗口内，记录日志 `"[ASRShortcut][Fn] deactivateServer within ignore window, keep ASR"`（`0x1001a6ce0`），保持语音采集不中断。
   - **麦克风 TCC 权限弹窗过渡期豁免**：若当前正处于首次麦克风授权切换期，记录 `"[ASR] deactivateServer within microphone permission transition, keep ASR"`（`0x1001a6dac`），拒绝停止录音。
   - **系统菜单栏交互保留 Marked Text**：在 `onASRSystemMenuInteraction()`（`0x1001bb730`）中，通过 `DispatchQueue.main.asyncAfter` 维持组合态，记录 `"[ASR][SystemMenu] keep marked text while system menu is active, logID="`（`0x1001bb670`）。
   - **智能整理（SmartOrganize）回退上屏飞行中豁免**：若 LLM 智能整理正在执行异步回退提交，记录 `"[SmartOrganize][Interrupt] skip_asr_force_stop reason=fallback_commit_in_flight"`（`0x1001a6fcc`）。
2. **跨输入源全局语音唤醒与 Receiver 挂载恢复（`waitForPreparedDoubaoInputSource`，`0x10003eb14`）**：
   - 当用户处于系统英文或其他输入源下按下全局语音快捷键时，`DoubaoImeASRShortcutCoordinator.handleShortcut(action:)`（`0x100016f0c`）首先检查 `"global_voice_disabled"`（`0x1000172bc`）。
   - 若允许全局唤醒，调用 `waitForPreparedDoubaoInputSource(desiredID:startedAt:generation:baselineReceiverAttachGeneration:requiresNewReceiverAttachment:strategy:didAttemptReceiverRecovery:targetFrontmostBundleID:logID:action:type:start:)`（`0x10003eb14`），按三级策略（`"shortcut_then_direct_selection"` `0x10003ebb8` $\rightarrow$ `"direct_selection"` `0x10003ed08` $\rightarrow$ `"bridge_from_system_input_source"` `0x10003ed30`）切换 TIS 输入源，并轮询比对 `receiverAttachGeneration` 与 `baselineReceiverAttachGeneration`（`0x10003ef68..0x10003efc0`），确保新的宿主 `DoubaoImeInputController` 已完成 `activateServer:` 绑定后再派发语音上屏。

---

## 3. 核心机制（二）：音频采集、蓝牙私有协议与前端信号处理管线（Audio Capture & Front-End Pipeline）

### 3.1 音频子系统类结构与双通道采集架构（System AVCapture vs. Dora BLE Private Capture）

豆包输入法在 `DoubaoIme` 主进程内构建了由 `AudioManager`、`ASRAudioEngine`、`AudioDeviceManager`、`SystemCaptureCache` 与 `DoraAudioCaptureController` 组成的双轨音频采集架构：

| 类名（Swift Demangled） | 元数据地址 | 实例大小 / Ivars | 遵循协议 / 父类 | 核心职责与关联队列 |
| :--- | :--- | :--- | :--- | :--- |
| `DoubaoIme.AudioManager` | `0x101426fb8` | 120 B / 13 ivars | `_SwiftObject` | 统一音频会话编排、采集源路由（Dora 蓝牙私有协议 vs 系统 `AVCaptureSession`）与故障自动回退 |
| `DoubaoIme.ASRAudioEngine` | `0x101413550` | 376 B / 35 ivars | `NSObject`, `AVCaptureAudioDataOutputSampleBufferDelegate` | 管理 `AVCaptureSession`、音频重采样/PCM 帧切分、VAD 与向 `ASRManager` 推送音频块 |
| `DoubaoIme.AudioDeviceManager` | `0x1014137c0` | 248 B / 28 ivars | `NSObject`, `AVCaptureAudioDataOutputSampleBufferDelegate` | 枚举 CoreAudio / IOBluetooth 麦克风、盒盖状态（Clamshell）策略与设置页实时音量计分发（队列 `"com.oime.AudioDeviceManager.monitor"`） |
| `DoubaoIme.SystemCaptureCache` | `0x101426ee8` | 88 B / 7 ivars | `_SwiftObject` | 缓存并复用预热的 `AVCaptureSession` 实例以降低按键起音延迟（清理队列 `"com.oime.SystemCaptureCache.cleanup"`） |
| `DoubaoIme.RecordingOutputMute` / `RecordingDeviceMute` | `0x101427270` / `0x101427360` | 96 B (11 ivars) / 24 B (1 ivar) | `_SwiftObject` | 录音期间可选的系统扬声器静音控制与恢复 |
| `DoraMacSDK.DoraAudioCaptureController` | `0x10027c118` (代码区) | — | `_SwiftObject` | 通过 CoreBluetooth 私有协议采集 Ola Friend 耳机 Opus 语音流并由 `DoraOpusDecoder` 解码（队列 `"com.oime.DoraAudioCaptureController"`） |

### 3.2 麦克风设备选择策略与 Dora 私有采集自动降级（Device Policy & Fallback）

1. **盒盖感知（Clamshell Mode）与首选麦克风热缓存（`0x10010af78..0x10010c110`）**：
   - `AudioDeviceManager.refreshMicrophoneListAndDefaultPolicy(reason:force:)`（`0x10010b43c`）在设备变更或启动时读取 IOKit 盒盖状态（日志 `"[MicCapturePolicy] clamshell="` 于 `0x10010b7e0`），当 MacBook 处于合盖外接显示器状态时自动规避内置麦克风并切换至外接音频输入源。
   - 若设备列表与首选麦克风 `uniqueID` 未发生变化，输出日志 `"[MicDeviceList] preferred microphone unchanged, skip audio reset and warm-up, id="`（`0x10010bc74`），直接复用 `SystemCaptureCache` 中已预热的采集通道，避免每次按键重新构建 `AVCaptureSession` 带来的数百毫秒首帧延迟（**候选（待用户确认）**）。
2. **Dora 蓝牙耳机探测与私有流回退（`0x1000f4010..0x1000f4b6c` & `0x100102710..0x100103508`）**：
   - `AudioDeviceManager.doraBluetoothDevices(diagnosticLogID:diagnosticDeviceUID:)`（`0x100102f8c`）调用 `+[IOBluetoothDevice pairedDevices]`（`0x100102b98`），读取蓝牙 MAC `addressString` 与 `services`，归一化为大写十六进制 UID 以匹配字节跳动 Ola Friend 耳机。
   - 当 Dora 私有 BLE 采集发生超时或 GATT 特征值错误（`"serviceNotConfigured"` `0x10027c118` / `"characteristicNotFound"` `0x10027c1dc`）时，`AudioManager.recoverDoraPrivateCapture(reason:error:)`（`0x1000f401c`）先尝试单次私有协议重连，失败后无缝回退至系统级 `AVCaptureSession`：

```asm
// 源：/tmp/doubao-thin-arm64/DoubaoIme (AudioManager.swift, 0x1000f4010..0x1000f41a0)
1000f4010: add  x8, x8, #0x790 ; "/Users/.../DoubaoIme/ASR/AudioManager.swift"
1000f4014: sub  x28, x8, #0x20
1000f4018: adrp x8, 3728       ; 0x100f84000
1000f401c: add  x8, x8, #0x410 ; "recoverDoraPrivateCapture(reason:error:)"
...
1000f4130: ldr  x19, [x8, #0xc8] ; 读取私有采集重试计数器偏移
1000f4134: ldr  x8, [x22, x19]
1000f4138: cmp  x8, #0x0
1000f413c: b.le 0x1000f4164      ; 若尚未重试 (<=0)，跳转至 retry private capture 分支
1000f4160: b    0x1000f4450      ; 否则降级切换至系统 AVCaptureSession
```

在 `0x1000f4130..0x1000f4160` 中，代码检查实例偏移 `[x22, x19]` 处的重试计数：首次故障时自增计数器并记录 `"[DoraAudio] retry private capture, reason="`（`0x1000f419c`）；若再次失败则跳转至 `0x1000f4450` 启动系统 `AVCaptureSession`，成功后记录 `"[DoraAudio] switched to system AVCapture after private capture unavailable, reason="`（`0x1000f499c`）。

### 3.3 `CMSampleBuffer` 采集回调、声学特征提取与跨进程音量波形推送

1. **实时语音采集回调（`ASRAudioEngine.captureOutput(_:didOutput:from:)`，`0x1000f5304`）**：
   - `ASRAudioEngine` 实现 `AVCaptureAudioDataOutputSampleBufferDelegate` 的 `captureOutput:didOutputSampleBuffer:fromConnection:`（`0x1000f5304..0x1000f5370`），对传入的 `CMSampleBuffer`（寄存器 `x3` $\rightarrow$ `x23`）执行 `_objc_retain` 后立即转发至内部帧处理函数 `0x1000f6244`：

```asm
// 源：/tmp/doubao-thin-arm64/DoubaoIme (ASRAudioEngine.captureOutput, 0x1000f5318..0x1000f5360)
1000f5318: mov x19, x4        ; connection: AVCaptureConnection
1000f531c: mov x20, x3        ; sampleBuffer: CMSampleBuffer
1000f5320: mov x21, x0        ; self: ASRAudioEngine
1000f5324: mov x0, x2         ; output: AVCaptureOutput
1000f5328: bl  0x100e46158    ; _objc_retain
...
1000f5354: mov x0, x22        ; output
1000f5358: mov x1, x23        ; sampleBuffer
1000f535c: bl  0x1000f6244    ; 内部 PCM 提取、重采样与 VAD/SAMI 推流入口
```

2. **前端信号处理、音量计平滑与跨进程通知**：
   - **UI 波形音量计（`OimeCommon.ASRMetersGenerator`）**：`OimeCommon` 提供 `ASRMetersGenerator`（vtable `0x17848`）、`MeterGroup`（`0x170c4`）与 `Meter`，将实时音频帧的 RMS/峰值振幅转换为驱动 `ASRPanel` 胶囊悬浮窗声波动画的归一化浮点数组。
   - **设置页麦克风测试跨进程广播（`0x100113b28..0x100113cf4`）**：当用户在 `DoubaoImeSettings.app` 中测试麦克风时，`AudioDeviceManager.captureOutput(_:didOutput:from:)`（`0x1001102ac` -> `0x100113b28`）通过 `+[NSDistributedNotificationCenter defaultCenter]`（`0x100113ba4`）发送通知 `OimeCommon.updateMicrophoneMonitorAmplitudeNotification`（`0x100113bb4`），将实时振幅跨进程推送给设置面板。
   - **端侧声学特征提取管线（Flute Feature Extractor，`0x10057fe5c..0x100588d78`）**：当启用离线或混合语音识别时，16kHz 单声道 PCM 帧进入 Flute C++ 特征提取器（`feature/mel_bank_feature_extractor.cc` `0x1005878e0`、`feature/subband_feature_extractor.cc` `0x100587c24` 与 `feature/panther_feature_extractor.cc` `0x100586d90`），计算 Mel Filterbank / Subband 特征张量，交由 `LatencyOptimizedExecutor`（`0x1005791e0`）驱动 MNN 流式编码器（`/encoder_stream.mnn`）与 EAGLE 投机解码器（`/eagle_draft.mnn` + `/decoder.mnn`）。
   - **云端流式推流管线（`SAMIHandleObserver` & Pool Session，`0x10014e344..0x100159830`）**：`ASRManager` 实现 `SAMIHandleObserver`（Witness Table `0x1001596c8..0x100159830`），通过 `beginPoolSession(_:)`（`0x10014f238`，日志 `"[ASR][Lifecycle] pool_session_begin logID="`）复用预建立的 SAMI 语音网关连接池会话，将压缩后的音频分片实时上传并接收流式增量识别结果与 LLM 智能整理（SmartOrganize）结果。

---

## 4. 核心机制（三）：流式 ASR / LLM 结果上屏状态机与 `IMKTextInput` 交付机制

### 4.1 `ASRTypeWriter`：基于公共前缀（Common Prefix）的流式打字机平滑算法

流式 ASR 与 LLM 润色在识别过程中会高频修正局部字词。若每次收到云端或端侧的 Partial Result 都直接全量刷新宿主组合文本（Marked Text），会造成视觉跳动并破坏部分宿主应用的 Undo 栈。逆向 `Modules/OimeAudio/Classes/Core/UI/Typewriter/ASRTypeWriter.swift`（`0x100bb1500..0x100bb3500`）揭示了 `ASRTypeWriter` 的核心算法：

```asm
// 源：/tmp/doubao-thin-arm64/DoubaoIme (ASRTypeWriter.updateResult(result:block:), 0x100bb1a6c..0x100bb1be0)
100bb1a6c: mov w0, #0xf
100bb1a70: mov x1, x23 ; self.currentIndex
100bb1a74: mov x2, x20 ; self.targetText (low)
100bb1a78: mov x3, x21 ; self.targetText (high)
100bb1a7c: bl  0x100e420c0 ; String.subscript(Range<String.Index>) -> Substring (提取 currentPartialResult)
...
100bb1b98: bl  0x100e42798 ; StringProtocol.commonPrefix(with:options:) -> String
...
100bb1bb8: bl  0x100e41f4c ; String.count.getter (计算公共前缀字符数)
...
100bb1bdc: bl  0x100e42720 ; Substring.index(_:offsetBy:limitedBy:) (将游标重定位至公共前缀末尾)
```

其状态推进逻辑如下（**候选（待用户确认）**）：
1. **提取已展示前缀（`currentPartialResult`）**：在 `updateResult(result:block:)`（`0x100bb1a04`）中，首先从当前目标字符串切片 `targetText[..<currentIndex]`（`0x100bb1a7c`）得到屏幕已渲染出的文本 `currentPartialResult`。
2. **最长公共前缀比对（LCP Alignment）**：调用 `StringProtocol.commonPrefix(with:options:)`（`0x100bb1b98`）计算新到达的 `result` 与 `currentPartialResult` 的公共前缀长度 `commonPrefix.count`（`0x100bb1bb8`）。
3. **游标无感重定位与逐字步进**：通过 `Substring.index(_:offsetBy:limitedBy:)`（`0x100bb1bdc`）将打字机游标 `currentIndex` 重定位至公共前缀末尾（已稳定前缀保持不动，仅对新增或改写的后缀部分按 `0x100e66ec0` 处的固定时间步长通过 `OS_dispatch_queue.async` 逐字回调上屏）。

### 4.2 实时预编辑态（Marked Text）渲染链路（`0x10020e4d8..0x10020e680`）

在语音录制与流式回传阶段，`DoubaoImeInputController` 通过 `IMKTextInput` 的 `setMarkedText:selectionRange:replacementRange:` 将实时识别文本作为带下划线样式的组合态（Composition / Marked Text）注入宿主应用。`0x10020e520..0x10020e668` 处的反汇编精确还原了该调用链：

```asm
// 源：/tmp/doubao-thin-arm64/DoubaoIme (DoubaoImeInputController.swift, 0x10020e524..0x10020e664)
10020e524: ldr  x1, [x8, #0x758] ; selector: replacementRange
10020e52c: bl   0x100e460ec      ; -[IMKInputController replacementRange]
10020e544: ldr  x1, [x8, #0x768] ; selector: markForStyle:atRange:
10020e54c: mov  w2, #0x4         ; style = kTSMHiliteNoHilite (4)
10020e554: bl   0x100e460ec      ; -[IMKInputController markForStyle:atRange:(0, NSNotFound)]
...
10020e618: ldr  x1, [x8, #0x880] ; selector: initWithString:attributes:
10020e628: bl   0x100e460ec      ; -[NSAttributedString initWithString:attributes:]
10020e648: ldr  x1, [x8, #0xfd0] ; selector: setMarkedText:selectionRange:replacementRange:
10020e664: bl   0x100e460ec      ; -[client setMarkedText:selectionRange:replacementRange:]
```

该段代码执行了标准的 IMK 富文本标记流程：
1. 调用 `-[IMKInputController replacementRange]`（`0x10020e52c`）获取替换范围。
2. 调用 `-[IMKInputController markForStyle:atRange:]`（`0x10020e554`，传入 `style = 4`、范围 `(0, NSNotFound)`）生成符合宿主主题的文本属性字典，经 `0x1001a4b60` 补充自定义样式后构造 `NSAttributedString`（`0x10020e628`）。
3. 获取当前有效客户端对象 `x19`（由 `0x100210cb8` 校验返回），调用 `-[client setMarkedText:selectionRange:replacementRange:]`（`0x10020e664`）实时刷新光标处的预编辑文本。

### 4.3 最终上屏事务结算（Final Commit Resolution）与多通道回退机制

当用户松开快捷键或语音 VAD 判定结束时，`DoubaoImeInputController` 并非简单调用一次 `insertText:replacementRange:`，而是进入一套带事务 ID（`transactionID`）的宿主提交解析状态机（位于 `0x1001dcb6c..0x1001de228` 与 `0x10020d7fc..0x10020e4d4`）：

1. **核心状态机函数签名**：
   - `deferASRFinalCommitUntilHostResolutionIfNeeded(transactionID:continuation:)`（`0x10020df54`）
   - `commitASRResultAfterTypeWriterReset(result:commitSource:fallbackToCachedHistoryWhenEmpty:reportInputVADResult:triggerCommitEngine:failedLogMessage:closeASRPanelAfterCommit:preflight:willPostPasteCommandV:preferredPasteTargetPID:pasteRequiresVerifiedTargetPID:routedTextInputClient:pastePreSendGate:modifyPairPolicy:smartTextTrackingCycle:smartTextDisposition:punctuationRestorePolicy:target:resultCompletion:didCallDirect:completion:)`（`0x10020da48`）
2. **宿主焦点与选区一致性校验（`0x1001dcb6c..0x1001de228`）**：
   - 状态机维护了以下关键状态转换字面量：`"composition_request_exit"`（`0x1001dcb6c`）、`"writeAlreadySelected"`（`0x1001dcb78`）、`"drainingTypeWriter"`（`0x1001dcc10`）、`"waitingBeforeCommandV"`（`0x1001dcc4c`）、`"strategySelected"`（`0x1001dcc70`）、`"imeCommitRequired"`（`0x1001dce78`）、`"smartTargetRejected"`（`0x1001dd32c`）、`"transactionChanged"`（`0x1001dd400`）、`"trustedHostCommitPolicy"`（`0x1001dd4e0`）。
   - 在提交前读取宿主的实时 `-[client markedRange]`（`0x1001dd3b4`）与 `-[client selectedRange]`（`0x1001dd3cc`），并与录音开始时的快照比对，输出诊断日志：`"[ASR][FinalCommit] host_commit_resolved request=..., originalMarkedRange=..., liveMarkedRange=..., originalSelectedRange=..."`（`0x1001ddc18..0x1001de1cc`）。
   - 若检测到过期的打字机回调（`transactionID` 已推进），立即丢弃并记录 `"[ASR][FinalCommit] result skipped transaction=..., reason=staleTypewriterCallback, host="`（`0x10020d7fc..0x10020d9d0`）。
3. **三级上屏交付通道（Three-Tier Text Delivery，候选（待用户确认））**：
   - **主通道（Direct IMK Commit）**：当宿主 `IMKTextInput` 客户端有效且处于可信焦点策略（`"trustedHostCommitPolicy"`）时，通过 `Engine.swift`（`0x1001f4550..0x1001f4670`）调用 `-[OimeEngine clearContext]`（`0x1001f45d0`）及 `-[client insertText:replacementRange:]`（`0x1001f4670`）完成原生 IMK 上屏，并调用 `+[ImeEngine addASRResultNERResponse:]`（`0x10020e4c0`）将识别结果的命名实体（NER）写回拼音引擎上下文以辅助后续个人词库自学习。
   - **智能整理就地替换（SmartOrganize In-Place Replacement）**：当开启 LLM 智能整理（口语顺滑/纠错）时，先通过 Marked Text 或首次上屏展示原始 ASR 文本，待云端 LLM 返回润色结果后，结合 `modifyPairPolicy`、`smartTextTrackingCycle`、`smartTextDisposition` 与 `punctuationRestorePolicy`（`0x10020da48` 参数列表），校验宿主光标是否仍在原文本末尾（若宿主焦点已转移或用户已手动编辑，则触发 `"smartTargetRejected"` / `"failedToSmartOriginal"` `0x100208248`），校验通过后替换原区间文本。
   - **剪贴板 + `CGEvent` 合成按键回退通道（Command+V Fallback）**：当目标应用不支持非激活态 IMK 写入、或跨输入源唤醒时 `routedTextInputClient` 不可用，状态机切换至 `"waitingBeforeCommandV"`（`0x1001dcc4c`），校验目标进程 PID（`preferredPasteTargetPID` / `pasteRequiresVerifiedTargetPID`），通过 `ClipboardInsertionTarget`（`0x100078f70..0x10007e020`）暂存原剪贴板、写入待上屏文本，并在 `0x10021b220..0x10021b24c` 处调用 `_CGEventSourceCreate` 与 `_CGEventCreateKeyboardEvent` 合成 `Command+V` 键盘事件完成强制上屏。

---

## 5. 边界与限制及开放问题（Boundaries, Limitations & Open Questions）

### 5.1 边界与限制

1. **单二进制巨石耦合与内存常驻开销**：`DoubaoIme.app` 将 UI 控制层、音频管线、端侧 Flute/MNN/Panther 推理运行时与 SAMI 语音客户端静态编译进单一主程序 `MacOS/DoubaoIme`（Universal 体积 44.77 MB，`arm64` 切片 21.96 MB），导致输入法常驻后台进程基础体积与符号表规模庞大。
2. **非沙盒权限与辅助功能（AX）强依赖**：完整体验三级上屏回退（特别是跨输入源热键唤醒、`AXUIElement` 焦点校验与 `CGEvent` 合成 `Command+V` 粘贴）要求用户授予辅助功能（Accessibility）权限与麦克风（`com.apple.security.device.audio-input`）权限；若辅助功能未授权，跨应用焦点校验与剪贴板合成按键回退链将受限。
3. **私有蓝牙协议（Dora BLE）的硬件锁定**：`DoraAudioCaptureController` 依赖字节跳动自有外设 GATT 服务 UUID 与 Opus 帧格式，普通蓝牙耳机仍回退至 macOS 系统级 `AVCaptureSession`（触发蓝牙 HFP 采样率切换）。

### 5.2 开放问题

1. **`deactivateServer:` 保护窗口的具体时长阈值**：在 `0x1001a6518..0x1001a70d4` 中，`DoubaoImeInputController` 基于 `systemUptime` 差值过滤瞬态失焦事件；在鼠须管（Squirrel）中若引入类似保护窗口，如何在防止系统弹窗打断录音与允许用户主动点击切换宿主窗口之间设定最优时间阈值？
2. **剪贴板回退在剪贴板管理器环境下的数据无损恢复**：当通过 `ClipboardInsertionTarget`（`0x100078f70..0x10007e020`）执行 `Command+V` 合成粘贴后，延迟恢复原剪贴板快照的窗口期若遇宿主应用主线程繁忙（消费 `Command+V` 超过 250 ms），如何避免恢复过早导致粘贴出旧剪贴板内容？

---

## 6. 面向鼠须管（Squirrel / `rime-client`）的架构映射与候选启示

基于上述对 `DoubaoIme.app` 的逆向证据，针对在本项目（`~/repo/tools/rime-client`）中引入非侵入式语音输入与 LLM 优化接口的目标，归纳出以下五项技术形态观察（全部标记为 **候选（待用户确认）**）：

1. **进程内轻量协调 vs. 独立重型推理的边界划分（候选（待用户确认））**：
   - `DoubaoIme` 将 Flute/MNN 端侧推理引擎、Cronet 网络栈与 Rust SDK 全部打包进应用包，导致单包体积超过 110 MB（仅 `DoubaoIme` 主二进制即达 44.7 MB）。对于鼠须管（Squirrel）而言，若希望保持输入法主进程的轻量与非侵入性，可将 `SquirrelInputController` 仅作为快捷键状态机、音频采集（或音频委托）与 `IMKTextInput` 上屏执行器，而将 ASR/LLM 推理由外部本地服务或可插拔后端承载。
2. **`deactivateServer:` 防误断保护窗口的必要性（候选（待用户确认））**：
   - macOS 在弹出系统权限框、按下 Fn 键或点击状态栏菜单时会向 `IMKInputController` 派发 `deactivateServer:`。豆包输入法在 `0x1001a6518..0x1001a70d4` 中基于 `systemUptime` 时间窗与会话状态标志位抑制了 `deactivateServer:` 对活动录音会话的强制中断。鼠须管在扩展语音输入模式时，同样需要避免 `deactivateServer:` 无条件清空正在进行的语音会话。
3. **预热音频采集缓存（`SystemCaptureCache`）降低起音延迟（候选（待用户确认））**：
   - `AudioDeviceManager` 与 `SystemCaptureCache`（`0x1000e311c`、`0x10010bc74`）在首选麦克风未变更时跳过音频会话重置与预热，从而消除按下语音键时的 `AVCaptureSession` 冷启动吃字现象。
4. **流式上屏的公共前缀打字机（`ASRTypeWriter`）与 `setMarkedText` 组合态（候选（待用户确认））**：
   - 在流式 ASR 阶段使用 `-[IMKTextInput setMarkedText:selectionRange:replacementRange:]`（`0x10020e664`）承载增量文本，并通过 `commonPrefix(with:options:)`（`0x100bb1b98`）对齐新旧部分识别结果，既能让用户实时看到流式上屏反馈，又不会污染宿主编辑器的正式提交历史（Undo Stack）。
5. **带事务 ID（`transactionID`）的两阶段提交与剪贴板回退（候选（待用户确认））**：
   - 语音识别收尾与 LLM 润色存在天然的异步延迟。豆包输入法通过 `transactionID` 过滤过期回调（`staleTypewriterCallback`），并在最终上屏前比对 `originalMarkedRange` 与 `liveMarkedRange`（`0x1001de110..0x1001de16c`），在 IMK 原生 `insertText:replacementRange:` 失效时回退至 PID 校验后的剪贴板 + `CGEvent` 粘贴，这套容错状态机可直接作为鼠须管异步结果上屏的参考蓝本。

---

## 7. 检索日志（Query Log）

| # | 阶段 / 工具 | 查询目标或命令摘要 | 关键产出与证据定位 |
| :--- | :--- | :--- | :--- |
| 1 | `read` | 读取 `dev-docs/MAP.md`、`dev-docs/GUIDE.md`、`dev-docs/notes/001-research-voice-ime-index.md` | 明确文档规范、禁语表与课题 002 范围 |
| 2 | `bash` (`find` / `shasum -a 256`) | 扫描 `/Library/Input Methods/DoubaoIme.app` 全目录 16 个通用 Mach-O 二进制并计算大小与 SHA-256 | 完成 `DoubaoIme`、`OimeEngine`、`OimeCommon`、`libime_net_sdk.dylib` 等 16 个通用二进制资产盘点 |
| 3 | `bash` (`lipo -thin arm64` / `otool -L`) | 提取 7 个核心二进制的 `arm64` 薄切片至 `/tmp/doubao-thin-arm64/` 并解析动态库依赖图与段大小 | 获得 `arm64` 切片精确字节数、SHA-256 及 `__TEXT`/`__DATA` 段分布 |
| 4 | `bash` (`plutil -p` / `codesign -d --entitlements` / `afinfo`) | 解析 `Info.plist`、Entitlements 字典与 `RecordingSounds/*.caf` 音频头信息 | 确认 `DoubaoIme_Connection`、非沙盒 `com.apple.security.device.audio-input` 权限及 48kHz/24-bit 提示音规格 |
| 5 | `mcp__rea` (`rea_inspect_native_dispatch_metadata` + `rea_export_evidence_bundle`) | 对 7 个 `arm64` 二进制提取 ObjC 类、选择器、协议与 Swift vtable/witness table | 导出 `/tmp/doubao-thin-arm64/evidence-all.json` 与 `classes-dump.txt`（9,834 行） |
| 6 | `mcp__rea` (`rea_decompile_functions`) | 对 `libDoubaoImeDylib.dylib` 执行 Ghidra 反编译 | 还原 `+[ImeEngine ...]` 桥接层、`ClipBoardUtil` 与 `DoublePressHelper` 逻辑 |
| 7 | `mcp__rea` (`rea_decompile_functions`) | 对 `OimeCommon.framework` 与 `DoubaoImeUpdater` 执行 Ghidra 反编译 | 还原 `DoubaoImeInstanceLock`、`ASRResult`、`ASRMetersGenerator` 与后台更新器逻辑 |
| 8 | `bash` (`xcrun llvm-objdump --macho -d` + `nm` + `swift-demangle`) | 反汇编 `DoubaoIme`（21.9 MB）与 `OimeEngine`（8.8 MB）并生成符号映射表 | 生成 `DoubaoIme.disasm.txt`、`OimeEngine.disasm.txt` 与 `.syms.txt` |
| 9 | `bash` (`inspect_disasm.py` 源码路径扫描) | 提取 `DoubaoIme` 与 `OimeEngine` 代码段内嵌的 359 个源文件路径字面量及地址区间 | 精确定位 `main.swift`、`DoubaoImeInputController.swift`、`AudioManager.swift`、`ASRTypeWriter.swift` 与 Flute C++ 引擎边界 |
| 10 | `bash` (`inspect_disasm.py full` `0x100002fe0..0x10003f300`) | 反汇编 `main.swift` 与 `AppDelegate.swift` 启动及单实例锁区间 | 完整还原 `DoubaoImeInstanceLock.acquire()`、`setupIMKServer()` 及主进程守护初始化链路 |
| 11 | `bash` (`inspect_disasm.py full` `0x1000e3000..0x10015b000`) | 反汇编 `AudioManager.swift`、`ASRAudioEngine.swift`、`DoraAudioCaptureController.swift` 区间 | 还原 `SystemCaptureCache` 预热池、合盖状态检测、Dora BLE 私有采集与 `AVCaptureSession` 双轨降级管线 |
| 12 | `bash` (`inspect_disasm.py full` `0x1001a08fc..0x100223000`) | 反汇编 `DoubaoImeInputController+ASR.swift`、`Engine.swift` 与 `FinalCommit` 状态机区间 | 还原 `deactivateServer:` 防误断门控、三级上屏通道（IMK 直连、SmartOrganize 替换、剪贴板 `Command+V` 回退） |
| 13 | `bash` (`inspect_disasm.py full` `0x100bb1500..0x100bb3500`) | 反汇编 `ASRTypeWriter.swift` 流式打字机算法区间 | 完整还原基于 `commonPrefix(with:options:)` 的流式增量上屏与事务 ID（`transactionID`）过滤逻辑 |

<!-- research-complete -->
