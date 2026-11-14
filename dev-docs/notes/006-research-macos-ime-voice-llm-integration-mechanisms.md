# macOS 输入法语音采集权限、本地推理引擎与云端 LLM 流式后处理技术调研

> **关联文档**：本文档属于语音增强型 Rime 客户端调研系列的子课题 `006`，聚焦 macOS 输入法套件 (`InputMethodKit`) 的麦克风权限与音频采集管线、全局/修饰键快捷键捕获、Apple Silicon 本地离线语音识别 (`Offline ASR`) 与本地大语言模型 (`Local LLM`) 推理集成，以及云端流式语音与大语言模型 (`Cloud LLM`) 后处理同 `IMKTextInput` 协议的异步协同机制。总索引见 `dev-docs/notes/001-research-voice-ime-index.md`，仓库文档地图见 `dev-docs/MAP.md`。

---

## 一、事实与来源：调研背景与逆向对标基线

macOS 输入法应用运行在受限的后台常驻会话中。以鼠须管 (`Squirrel`) 为代表的传统 Rime 前端仅处理同步键盘事件 (`NSEvent.keyDown` / `.flagsChanged`)，通过 `librime` 同步计算候选词并调用 `IMKTextInput` 接口上屏。当引入「按住说话 (`Push-to-Talk`) — 流式语音识别 (`Streaming ASR`) — 大语言模型润色 (`LLM Post-Processing`) — 渐进式上屏 (`Progressive Commit`)」链路时，输入法主进程将面临三大系统级挑战：

1. **后台进程权限与低延迟采集**：`Info.plist` 中声明 `LSBackgroundOnly = 1` 的输入法进程在 macOS Sonoma (14)、Sequoia (15) 及 Tahoe (26) 的透明权限与控制中心 (`TCC, Transparency, Consent, and Control`) 下的麦克风授权弹窗行为、跨进程设置宿主协调，以及 `AVCaptureSession`、`AVAudioEngine` 与 `AudioUnit` 的实时重采样延迟控制。
2. **修饰键与跨输入源热键捕获**：`IMKInputController.handle(_:client:)` 仅在当前输入法激活且焦点处于文本控件时接收有限事件，如何捕获长按 `Fn/Globe`、右 `Option` (`Right Option`) 或组合键的按下与释放全周期，并区分硬件左右修饰键。
3. **本地/云端异构推理与非阻塞上屏**：Apple Silicon 统一内存架构下本地 `ASR` 与本地 `LLM` 的内存占用与进程隔离边界，以及云端 `WebSocket` / 服务器发送事件 (`SSE, Server-Sent Events`) 流式输出如何与 `IMKTextInput` 主线程接口安全交互而不阻塞宿主应用界面。

为获取生产级实证数据，本调研同时对本机安装的鼠须管源码参考库 (`dev-docs/refs/code/squirrel`) 与商业语音输入法豆包输入法 (`DoubaoIme.app` v1.1.0, Build 1) 进行了二进制级逆向工程剖析。

| 对标样本 | 二进制 / 源码路径 | 架构与版本 | SHA-256 校验值 |
| :--- | :--- | :--- | :--- |
| 鼠须管 (`Squirrel`) | `dev-docs/refs/code/squirrel/sources/` | Swift 5 / macOS 13.0+ | 源码级对照 (`Info.plist` / `SquirrelInputController.swift`) |
| 豆包输入法主程序 (`DoubaoIme`) | `/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme` | `arm64` / `x86_64` Universal (v1.1.0) | `5b48748fa6394811cba0f8c05905ffebb9a2cd02faf6d47b26deb3d9b86247e0` |
| 豆包公共基础库 (`OimeCommon`) | `/Library/Input Methods/DoubaoIme.app/Contents/Frameworks/OimeCommon.framework/Versions/A/OimeCommon` | `arm64` / `x86_64` Universal (v1.0) | `c26d8e2451737fc78d38aa819bb62b04594f5d894b5a9669b335e83e8ee8831c` |
| 豆包引擎核心库 (`OimeEngine`) | `/Library/Input Methods/DoubaoIme.app/Contents/Frameworks/OimeEngine.framework/Versions/A/OimeEngine` | `arm64` / `x86_64` Universal (v1.0) | `7ba480eaabbd4d99f6d63917d9fc208c81e8b1d1c38db22afa3795a46ceb8ed3` |

---

## 二、macOS InputMethodKit 权限模型、音频采集与全局快捷键机制

### 2.1 `LSBackgroundOnly` 输入法进程的 TCC 麦克风权限机制

在 macOS 中，驻留于 `/Library/Input Methods/` 或 `~/Library/Input Methods/` 的输入法 Bundle 通过 `Info.plist` 中的 `LSBackgroundOnly = true`（或 `LSUIElement = true`）向窗口服务器 (`WindowServer`) 与启动服务 (`LaunchServices`) 声明自身为无 Dock 图标、无应用主菜单的后台服务进程。

```xml
// 源：dev-docs/refs/code/squirrel/resources/Info.plist:27-41
  <key>InputMethodConnectionName</key>
  <string>Squirrel_1_Connection</string>
  <key>InputMethodServerControllerClass</key>
  <string>SquirrelInputController</string>
  <key>LSBackgroundOnly</key>
  <true/>
```

如上方鼠须管 `Info.plist` 所示，标准 Rime 客户端未声明 `NSMicrophoneUsageDescription`，其 `Squirrel.entitlements` 仅包含禁用应用沙盒 (`com.apple.security.app-sandbox = false`)、禁用库验证 (`com.apple.security.cs.disable-library-validation = true`) 与网络客户端权限 (`com.apple.security.network.client = true`)。若在此状态下直接调用音频采集接口，macOS 的守护进程 `tccd` 会直接拦截并杀死进程（触发 `SIGABRT` 崩溃日志：`This app has crashed because it attempted to access privacy-sensitive data without a usage description`）。

对比逆向提取的 `DoubaoIme.app` 签名权限 (`codesign -d --entitlements :-`) 与 `Info.plist`，语音输入法在 TCC 权限层需同时配置 `Info.plist` 描述文本与硬化运行时 (`Hardened Runtime`) 权利：

```xml
// 源：/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme (codesign entitlements)
<plist version="1.0">
<dict>
  <key>com.apple.security.device.audio-input</key>
  <true/>
</dict>
</plist>
```

當开启硬化运行时以通过 Apple 公证 (`Notarization`) 时，缺少 `com.apple.security.device.audio-input` 会导致即使 `Info.plist` 存在 `NSMicrophoneUsageDescription`，`AVCaptureDevice.authorizationStatus(for: .audio)` 仍永远返回 `.denied` 且系统不弹出 TCC 授权框。此外，在 macOS Sonoma (14)、Sequoia (15) 与 Tahoe (26) 上，`LSBackgroundOnly = 1` 的输入法进程触发 TCC 麦克风弹窗存在以下关键边界行为：

1. **责任进程归属 (`Responsible Process Attribution`)**：当输入法由系统 `IMKLaunchAgent` 拉起时，它本身是独立的 TCC 主体（在「系统设置 -> 隐私与安全性 -> 麦克风」中显示输入法自身图标与名称）；但若由未声明麦克风权限的父进程通过 `posix_spawn` 直接派生而未脱离责任链，TCC 会检查父进程权限。
2. **后台进程首次授权时机与焦点丢失**：`LSBackgroundOnly` 进程在文本输入中途直接调用 `AVCaptureDevice.requestAccess(for: .audio)` 时，macOS Sequoia/Tahoe 的 `UserNotificationCenter` 会弹出系统模态授权框，这会导致当前宿主客户端立刻失去键盘焦点并触发 `IMKInputController.deactivateServer(_:)`，中断正在进行的首次按键会话。
3. **拒绝态 (`AVAuthorizationStatus.denied`) 的二次引导**：TCC 仅在 `.notDetermined` (0) 状态下弹出一次系统授权框；若用户点击「不允许」进入 `.denied` (2) 状态，再次调用 `requestAccess` 会立即在回调中返回 `false` 而不展示任何系统 UI。

逆向 `DoubaoIme` 的权限管理器 `_TtC14OimePermission22AudioPermissionManager`（源文件路径符号为 `Modules/OimePermission/Classes/AudioPermissionManager.swift`，编译位于 `DoubaoIme` 偏移 `0x100c3b570`–`0x100c3d950`）可见其跨进程协同与状态分支设计：

```asm
// 源：/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme:0x100c3b5b8-0x100c3b71c
0000000100c3b5bc  ldr  x0, [x8, #0x410] ; _OBJC_CLASS_$_AVCaptureDevice
0000000100c3b5cc  ldr  x8, [x8, #0x2b8] ; _AVMediaTypeAudio
0000000100c3b5d8  ldr  x1, [x8, #0x400] ; selector: authorizationStatusForMediaType:
0000000100c3b5e0  bl   0x100e460ec      ; +[AVCaptureDevice authorizationStatusForMediaType:]
0000000100c3b5e4  mov  x27, x0
0000000100c3b708  cbz  x27, 0x100c3b720 ; status == 0 (.notDetermined) -> requestAccess
0000000100c3b70c  cmp  x27, #0x3        ; status == 3 (.authorized)
0000000100c3b710  b.ne 0x100c3b90c      ; status == 1/2 (.restricted/.denied) -> guide window
```

结合 `0x100c3b8e8` 处的 `-[AVCaptureDevice requestAccessForMediaType:completionHandler:]` 与 `0x100c3baac` 处的 `OimeCommon.microphoneIPCTargetBundlePathKey` 符号，其完整权限状态机如下：

* **双进程分布式通知委托**：独立的前台配置应用 `DoubaoImeSettings.app`（位于 `Contents/Applications/DoubaoImeSettings.app`）自身**不声明** `NSMicrophoneUsageDescription`，因为若由 `DoubaoImeSettings.app` 请求权限，TCC 数据库会将授权记录绑定在 `com.bytedance.inputmethod.doubaoime.settings` 下，导致输入法主进程 `com.bytedance.inputmethod.doubaoime` 依然无权录音。当用户在设置向导中点击授权时，`DoubaoImeSettings` 通过 `NSDistributedNotificationCenter` 发送 `DoubaoImeSettings.requestMicrophonePermission`（携带 `requestId` 与 `targetBundlePath`），由 `DoubaoIme` 主进程的 `AudioPermissionManager.handleRequestMicrophonePermission(requestId:showAuthorizationPromptOnDenied:)` 统一发起 `requestAccessForMediaType:`，完成后再广播 `DoubaoIme.microphonePermissionDidUpdate` 回传状态。
* **防重入与拒绝态非激活悬浮引导窗**：`AudioPermissionManager` 维护 `isRequestInFlight` 锁（见 `0x100c3b96c` 日志 `"handleRequestMicrophonePermission request already in flight"`）；当状态为 `.denied` 且 `showAuthorizationPromptOnDenied == true` 时，跳转至 `0x100c3d814` 在主队列 (`DispatchQueue.main`) 实例化自定义引导窗口 `_TtC14OimePermission25AuthorizationPromptWindow`（标题：`"需要麦克风权限"`，副标题：`"开启权限后即可使用「语音输入」功能，让输入轻松又高效"`，配图资源 `"onboarding_auido_deny_guide"`），点击按钮后通过 `NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")!)` 深度链接直达系统设置麦克风页。

对于 Rime 客户端的权限架构，由输入法主 Bundle 唯一持有 `NSMicrophoneUsageDescription` 与 `com.apple.security.device.audio-input` 权利、并通过 `NSDistributedNotificationCenter` 响应设置面板的权限请求属于**候选（待用户确认）**方案。

---

### 2.2 低延迟音频采集管线对比：`AVCaptureSession` vs `AVAudioEngine` vs `AudioUnit`

语音识别模型（无论本地 `whisper.cpp` / `sherpa-onnx` 还是云端流式接口）普遍要求输入音频格式为 **16 kHz、单声道 (`Mono`)、16-bit 有符号整型 (`Int16`) 或 32-bit 浮点型 (`Float32`) 线性 PCM (`Linear PCM`)**。然而 macOS 硬件音频输入设备（内置阵列麦克风、AirPods 蓝牙 HFP/AAC、外置 USB 声卡）的原生采样率通常为 44.1 kHz、48 kHz 甚至 96 kHz，且声道数在 1 至 8 之间动态变化。

在 macOS 上构建输入法音频采集与重采样管线共有三种底层技术路径，再加上针对特定蓝牙硬件的私有链路扩展：

| 评估维度 | `AVCaptureSession` + `AVCaptureAudioDataOutput` | `AVAudioEngine` (`AVAudioInputNode.installTap`) | `AudioUnit` (`kAudioUnitSubType_HALOutput` / `VoiceProcessingIO`) |
| :--- | :--- | :--- | :--- |
| **API 层级与编程模型** | `AVFoundation` 高层对象，基于 `CMSampleBuffer` 委托回调 | `AVFAudio` 节点图模型，基于 `AVAudioPCMBuffer` 闭包回调 | `AudioToolbox` C 语言底层回调 (`AURenderCallbackStruct`) |
| **单次回调缓冲延迟** | 约 10 ms – 23 ms（取决于设备默认 I/O 帧长 512/1024 帧） | 固定下限：源码级硬编码最小支持约 10 ms – 50 ms (`bufferSize` 雖可传小值但内部合并为约 100 ms 级批次分发) | 极低：可通过 `kAudioDevicePropertyBufferFrameSize` 设为 64–256 帧（48 kHz 下仅 **1.3 ms – 5.3 ms**） |
| **冷启动耗时 (`start`)** | 首次 `startRunning()` 约 80 ms – 200 ms；支持会话预热复用 | `prepare()` + `start()` 约 60 ms – 150 ms；切设备需重建图 | `AudioUnitInitialize` + `AudioOutputUnitStart` 约 **15 ms – 45 ms** |
| **硬件热插拔恢复能力** | 强：自动通过 `AVCaptureSessionRuntimeError` 与通知感知设备切换 | 弱：默认输入设备切换时节点格式失配，易抛 Objective-C `NSException` 导致进程崩溃 | 中：需自行注册 `AudioObjectAddPropertyListener` 监听 `kAudioHardwarePropertyDefaultInputDevice` 并重新绑定 |
| **系统级回声消除 (AEC)** | 无内置 AEC（需配合系统静音或外部 DSP） | 可启用 `inputNode.setVoiceProcessingEnabled(true)`，但会触发系统音频 Ducking（压低音乐音量） | `VoiceProcessingIO` 自带硬件级 AEC/AGC/NS，但开启时强制引发系统混音器采样率重置与蓝牙切 HFP 音质劣化；`HALOutput` (`AUHAL`) 纯采集无副作用 |
| **16 kHz 重采样集成** | 在委托串行队列中从 `CMSampleBuffer` 构造 `AVAudioPCMBuffer` 送入 `AVAudioConverter` | 在 `installTap` 回调中直接调用 `AVAudioConverter.convert(to:error:withInputFrom:)` | 可在 C 回调后接 `AudioConverterFillComplexBuffer` 或 `speex_resampler` |

逆向 `DoubaoIme` 的音频采集引擎 `_TtC9DoubaoIme14ASRAudioEngine`（类元数据地址 `0x100f833a0`，方法实现位于 `0x1000f3c00`–`0x1000f6800`）可以发现，它并未采用 `AVAudioEngine`，而是采用了 **`AVCaptureSession` + `AVCaptureAudioDataOutput` + `AVAudioConverter` 串行队列转换 + 预热池 (`Warm-up`)** 的混合架构，并针对特定蓝牙外设实现了 `_TtC9DoubaoIme26DoraAudioCaptureController` (`0x100f9b6a0`) 私有 Opus 链路采集：

```text
// 源：/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme (otool -ov _TtC9DoubaoIme14ASRAudioEngine ivars)
0x101427078  session                    (AVCaptureSession)
0x101427080  audioOutput                (AVCaptureAudioDataOutput)
0x101427088  captureReuseRevision       (UInt64, 会话复用代数)
0x101427090  firstAudioAfterStartPending (Bool, 首帧到达监控标志)
0x1014270a0  outputMuteOwnerStorage     (_TtC9DoubaoIme19RecordingOutputMute)
0x101427100  inputFormat / sourceFormat (AVAudioFormat)
0x101427108  audioConverter             (AVAudioConverter)
0x101427118  queue                      (OS_dispatch_queue, 采集与重采样串行队列)
0x101427168  warmUpStateLock / isWarmedUp / isWarmUpInProgress
```

`ASRAudioEngine` 的工程细节揭示了桌面语音输入法在音频采集上的四个关键设计点：

1. **为什么规避 `AVAudioEngine.inputNode`？** 在 macOS 上，当用户佩戴 AirPods 或插拔外置显示器音频时，`AVAudioEngine` 的 `inputNode.outputFormat(forBus: 0)` 会在底层 CoreAudio 线程瞬间变为采样率 `0 Hz` 或声道数不匹配，此时 `installTap(onBus:bufferSize:format:block:)` 会在内部抛出未捕获的 Objective-C 异常 (`com.apple.coreaudio.avfaudio: required condition is false: format.sampleRate == hwFormat.sampleRate`)，Swift 的 `do-catch` 无法拦截 Objective-C `NSException`，直接导致输入法后台进程崩溃。而 `AVCaptureAudioDataOutputSampleBufferDelegate` 的 `captureOutput(_:didOutput:from:)`（`DoubaoIme` 偏移 `0x1000f5304`）在每一帧 `CMSampleBuffer` 中动态携带 `CMAudioFormatDescription`，引擎可在串行 `queue` 中安全比对 `sourceFormat`，若硬件采样率变更则原地重建 `AVAudioConverter` 实例。
2. **会话预热 (`isWarmedUp`) 与首帧超时监控 (`firstAudioAfterStartPending`)**：为消除按下热键时的 CoreAudio 硬件唤醒延迟（以及 macOS 菜单栏橙色麦克风指示灯点亮瞬间的首包空洞），`ASRAudioEngine` 支持预热 (`isWarmedUp`) 与 `captureReuseRevision` 复用，并通过 `recordingRequestGeneration` 代数锁防止用户快速连按热键时的异步启停竞态。
3. **录音期间扬声器静音控制 (`_TtC9DoubaoIme19RecordingOutputMute`)**：不启用 `VoiceProcessingIO`（避免触发系统音频 Ducking 导致正在播放的媒体产生爆音或切入低音质通话模式），而是配备独立的 `RecordingOutputMute` 控制器，在按住说话期间通过 CoreAudio `AudioObjectSetPropertyData` 在会话期内将默认输出设备静音，松开后立即恢复。
4. **能量计算节流 (`lastAmplitudeReportAt` / `amplitudeReportCount`)**：在 `captureOutput` 中计算均方根 (`RMS`) 音量分贝以驱动悬浮波形面板 (`_TtC9DoubaoIme8ASRPanel`) 时，通过时间戳节流避免高频刷新主线程 UI。

对于 Rime 语音模块，采用 `AVCaptureSession` + `AVCaptureAudioDataOutput` 配合串行 `DispatchQueue` 内的 `AVAudioConverter` 转换为 16 kHz 单声道 PCM（或使用 `AudioUnit` `HALOutput` 配合 C 级无异常状态机）属于**候选（待用户确认）**方案。

---

### 2.3 全局快捷键与修饰键长按捕获：`IMKInputController` vs `CGEventTap`

语音输入最符合直觉的交互方式是「长按单一修饰键或组合键说话，松开即结束录音并润色上屏」（例如：长按 `Right Option`、长按 `Fn/Globe`、长按 `Right Control` 或 `Control + Space`）。然而，macOS 的 `InputMethodKit` 事件分发机制对此存在天然限制。

先看鼠须管 (`SquirrelInputController.swift`) 与豆包输入法 (`DoubaoImeInputController` 偏移 `0x1001f2aa4`) 对 `recognizedEvents(_:)` 的实现：

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:280-285
override func recognizedEvents(_ sender: Any!) -> Int {
  // print("recognizedEvents:")
  return Int(NSEvent.EventTypeMask(arrayLiteral: .keyDown, .flagsChanged).rawValue)
}
```

反汇编 `DoubaoImeInputController.recognizedEvents:` (`0x1001f2aa4`–`0x1001f2b20`) 可见其在 `0x1001f2b10` 处同样执行 `mov w0, #0x1400`（即 `NSEvent.EventTypeMask.keyDown (0x400) | .flagsChanged (0x1000)`）。

如果仅依赖 `IMKInputController.handle(_:client:)`，将遇到四个无法绕过的缺陷：

1. **普通键无 `.keyUp` 事件**：`IMKServer` 对普通按键默认不向输入法控制器转发 `NSEvent.EventType.keyUp` (`0x800`)；即便在 `recognizedEvents(_:)` 中强行加入 `.keyUp`，诸多 Cocoa / Electron 宿主客户端也不会通过 IPC 通道将 `keyUp` 转发给输入法服务端，导致「按住组合键（如 `Option + Z`）说话、松开 `Z` 结束」无法检测松开时机。
2. **非激活输入源失聪**：当用户当前处于系统英文输入源 (`com.apple.keylayout.ABC`) 时，Rime 输入法的 `IMKInputController` 处于 `deactivateServer` 休眠态，`handle(_:client:)` 完全收不到任何按键事件。
3. **无焦点或非标准输入框失聪**：当焦点位于桌面、Finder 文件列表或未实现 `NSTextInputClient` 的自绘窗口（部分终端、远程桌面、游戏或浮动窗口）时，IMK 通道不触发。
4. **系统高层 `NSEvent.ModifierFlags` 不区分左右修饰键**：`NSEvent.ModifierFlags.option` (`0x80000`)、`.command` (`0x100000`)、`.control` (`0x40000`)、`.shift` (`0x20000`) 是设备无关掩码 (`Device-Independent Flags`)。若仅检查 `.option`，用户按左 `Option` 组合键（如 `Option + Left/Right` 跳词）也会误触右 `Option` 语音热键。

#### 2.3.1 硬件级左右修饰键位掩码 (`NX_DEVICE*KEYMASK`) 逆向解析

为了精确区分左右修饰键（如单独绑定 `Right Option` 或 `Right Command` 为语音键），需要读取 `CGEventFlags`（或 `NSEvent.modifierFlags.rawValue`）的低 16 位硬件设备相关标志 (`IOLLEvent.h` 中的 `NX_DEVICE*KEYMASK`) 以及硬件虚拟键码 (`CGKeyCode`)。

逆向 `OimeCommon.framework` 中的 `ASRShortcutModifierFlags`（偏移 `0x12ee4`–`0x1328c`）可直接提取其完整的位掩码表与 `keyCode` 映射算法：

```asm
// 源：/Library/Input Methods/DoubaoIme.app/Contents/Frameworks/OimeCommon.framework/Versions/A/OimeCommon:0x12ee4-0x12f98
0000000000012eec  mov  w0, #0x1       ; leftControl   (NX_DEVICELCTLKEYMASK   = 0x0001)
0000000000012ef8  mov  w0, #0x2       ; leftShift     (NX_DEVICELSHIFTKEYMASK = 0x0002)
0000000000012f08  mov  w0, #0x4       ; rightShift    (NX_DEVICERSHIFTKEYMASK = 0x0004)
0000000000012f1c  mov  w0, #0x8       ; leftCommand   (NX_DEVICELCMDKEYMASK   = 0x0008)
0000000000012f30  mov  w0, #0x10      ; rightCommand  (NX_DEVICERCMDKEYMASK   = 0x0010)
0000000000012f44  mov  w0, #0x20      ; leftOption    (NX_DEVICELALTKEYMASK   = 0x0020)
0000000000012f58  mov  w0, #0x40      ; rightOption   (NX_DEVICERALTKEYMASK   = 0x0040)
0000000000012f6c  mov  w0, #0x2000    ; rightControl  (NX_DEVICERCTLKEYMASK   = 0x2000)
0000000000012f80  mov  w0, #0x9e0000  ; supportedIndependentFlags (.shift|.control|.option|.command|.function)
0000000000012f94  mov  w0, #0x207f    ; supportedSideFlags (0x1|0x2|0x4|0x8|0x10|0x20|0x40|0x2000)
```

同时，在 `OimeCommon` 偏移 `0x131a4` (`ASRShortcutModifierFlags.flags(forModifierKeyCode:)`) 中，定义了从物理 `keyCode` (`UInt16`) 到复合修饰标志 (`Independent | SideFlag`) 的精确映射：

| 物理按键名称 | 虚拟键码 (`kVK_*` / Hex) | 设备无关掩码 (`Independent Flag`) | 左右硬件掩码 (`Side Flag`) | 合成校验掩码 (`flags(forModifierKeyCode:)`) |
| :--- | :--- | :--- | :--- | :--- |
| `Left Command` | `55` (`0x37`) | `0x100000` (`.command`) | `0x0008` (`NX_DEVICELCMDKEYMASK`) | `0x100008` |
| `Right Command` | `54` (`0x36`) | `0x100000` (`.command`) | `0x0010` (`NX_DEVICERCMDKEYMASK`) | `0x100010` |
| `Left Shift` | `56` (`0x38`) | `0x020000` (`.shift`) | `0x0002` (`NX_DEVICELSHIFTKEYMASK`) | `0x020002` |
| `Right Shift` | `60` (`0x3c`) | `0x020000` (`.shift`) | `0x0004` (`NX_DEVICERSHIFTKEYMASK`) | `0x020004` |
| `Left Option` | `58` (`0x3a`) | `0x080000` (`.option`) | `0x0020` (`NX_DEVICELALTKEYMASK`) | `0x080020` |
| `Right Option` | `61` (`0x3d`) | `0x080000` (`.option`) | `0x0040` (`NX_DEVICERALTKEYMASK`) | `0x080040` |
| `Left Control` | `59` (`0x3b`) | `0x040000` (`.control`) | `0x0001` (`NX_DEVICELCTLKEYMASK`) | `0x040001` |
| `Right Control` | `62` (`0x3e`) | `0x040000` (`.control`) | `0x2000` (`NX_DEVICERCTLKEYMASK`) | `0x042000` |
| `Fn / Globe` | `63` (`0x3f`) | `0x800000` (`.function`) | `0x0000` (无左右之分) | `0x800000` |

#### 2.3.2 双轨快捷键协调器架构 (`IMKInputController` 本地轨 + `CGEventTap` 全局轨)

逆向 `DoubaoIme` 的快捷键协调器 `_TtC9DoubaoIme31DoubaoImeASRShortcutCoordinator`（类地址 `0x101130770`）与输入法控制器 `DoubaoImeInputController`（类地址 `0x100e6bdb0`）的实例变量 (`ivars`)，可以看到成熟语音输入法采用**双轨协同状态机**：

```text
// 源：/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme (otool -ov DoubaoImeASRShortcutCoordinator ivars)
0x101423108  $__lazy_storage_$_monitor                   (全局事件监听器)
0x101423110  isGlobalVoicePromptShortcutMonitorRunning   (Bool)
0x101423118  ignoreInputSourceChangeUntil                (TimeInterval, 抑制输入源切换抖动)
0x101423120  inputSourceChangedObserver                  (NSObjectProtocol)
0x101423130  isASRRecordingActive                        (Bool)
0x101423138  mouseFocusTransitionWorkItem                (DispatchWorkItem)
0x101423140  shortcutRecordingEventTap                   (CFMachPort, CGEventTap 句柄)
0x101423148  shortcutRecordingRunLoopSource              (CFRunLoopSource)
```

双轨机制的工作流与边界条件处理如下：

1. **轨一：IMK 本地通道 (`localASRShortcutHandler`)**
   * **触发条件**：当前输入法已激活（处于 Rime 中文/英文状态），且快捷键为纯修饰键（如长按 `Right Option` 或 `Fn`）。
   * **权限优势**：**无需「辅助功能 (`Accessibility`)」或「输入监控 (`Input Monitoring`)」TCC 权限**。因为 `IMKInputController.handle(_:client:)` 原本就会收到每次修饰键按下与松开的 `NSEvent.flagsChanged` 事件。
   * **状态机防误触判定 (`Chord Cancellation`)**：
     * 当收到 `flagsChanged` 且 `(event.modifierFlags.rawValue & 0x009e207f) == targetMask`（例如 `Right Option` 对应的 `0x080040`）时，记录 `modifierDownTimestamp` 并启动 `150 ms – 250 ms` 的 `DispatchSourceTimer` 延迟判定计时器。
     * 若在计时器到期前收到任何 `keyDown`（说明用户正在按 `Right Option + 其他键` 输入特殊字符或触发编辑器快捷键）、或收到其他修饰键叠加、或收到鼠标点击事件，立即取消计时器，不唤醒语音。
     * 若计时器到期时修饰键仍保持按下且无其他按键插入，进入 `Recording` 态，启动音频采集并展示悬浮语音条。
     * 当再次收到 `flagsChanged` 且目标修饰键位清零时，转入 `Recognizing / Polishing` 态并结束录音。
2. **轨二：`CGEventTap` / `NSEvent.addGlobalMonitor` 全局通道 (`DoubaoImeASRShortcutCoordinator`)**
   * **触发条件**：需要在非当前输入源（如系统处于英文 ABC 输入法时直接按住语音热键说话）、或需要拦截包含普通字符键的组合热键（吞掉按键不让宿主应用响应，需监听 `kCGEventKeyDown`、`kCGEventKeyUp`、`kCGEventFlagsChanged`）。
   * **API 与 TCC 权限边界**：
     * 调用 `CGEvent.tapCreate(tap: .cgSessionEventTap, place: .headInsertEventTap, options: .defaultTap, eventsOfInterest: mask, callback: cb, userInfo: ptr)`：当 `options` 为 `.defaultTap`（主动过滤/吞掉事件）且监听键盘事件时，macOS 要求进程具备 **辅助功能 (`Accessibility`, `AXIsProcessTrustedWithOptions`)** 权限；若 `options` 为 `.listenOnly`（只读监听，类似 `NSEvent.addGlobalMonitorForEvents`），则要求 **输入监控 (`Input Monitoring`, `IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)`)** 权限。
   * **内核超时自动禁用恢复 (`kCGEventTapDisabledByTimeout`)**：macOS `WindowServer` 对 `CGEventTap` 回调有严格的实时看门狗限制（约 `200 ms – 500 ms`）。若输入法主线程因 Rime 词库加载或 UI 渲染短暂卡顿，系统会自动禁用该 Tap 并向回调发送 `type == .tapDisabledByTimeout` (`0xFFFFFFFE`) 或 `.tapDisabledByUserInput` (`0xFFFFFFFF`)。因此回调首行需检测这两个伪事件并立即调用 `CGEvent.tapEnable(tap: eventTap, enable: true)` 恢复。
   * **`Fn / Globe` (`keyCode == 63`) 的系统冲突规避**：在 macOS Sonoma/Sequoia/Tahoe 上，系统默认将单按 `Fn/Globe` 绑定为「切换输入源」或「显示表情与符号」，将双击 `Fn` 绑定为系统听写 (`Dictation`)。用户若将 `Fn` 设为长按语音键，`flagsChanged` 按下瞬间不会触发系统切换输入法，但若短按或松开时未发生长按消费，系统可能触发输入源切换。`DoubaoImeASRShortcutCoordinator` 通过 `ignoreInputSourceChangeUntil` 与 `OimeCommon.InputSourceSwitchCoordinator`（含 `pendingInputSourceRestore` 属性），在非本输入源下按住全局语音键时，在录音会话期内通过 `TISSelectInputSource` 切入本输入法以获取 `IMKTextInput` 通道上屏，上屏完成后再自动切回用户原本的输入源。

对于 Rime 语音输入集成，采用「已激活态下纯修饰键长按走 `IMKInputController` 零额外权限通道 + 跨输入源/组合键按需启用 `CGEventTap` 全局通道（并在 `tapDisabledByTimeout` 时自动重激活）」的双轨架构属于**候选（待用户确认）**方案。

---

## 三、Apple Silicon 本地离线语音识别 (ASR) 与本地小参数 LLM 集成

Apple Silicon (M1 / M2 / M3 / M4 系列) 采用统一内存架构 (`UMA, Unified Memory Architecture`)，CPU、GPU (`Metal`) 与神经网络引擎 (`ANE, Apple Neural Engine`) 共享高带宽物理内存（100 GB/s – 546 GB/s），零拷贝开销使其成为运行端侧语音识别与轻量级大语言模型的理想硬件平台。

### 3.1 本地离线 ASR 引擎深度对比：`whisper.cpp` vs `sherpa-onnx` vs Apple Speech

在 macOS 输入法场景下，本地离线 ASR 面临三大硬约束：**常驻内存 (`RSS`) 需控制在百兆级以内**、**尾包识别延迟 (`End-of-Speech to Text`) 需低于 200 ms**、**支持中英混输与Rime个人词库热词注入 (`Hotword Boosting`)**。

#### 3.1.1 方案一：`whisper.cpp` (GGML Metal / CoreML 后端)

`whisper.cpp` 是 OpenAI Whisper 模型的纯 C/C++ 无依赖实现 (`ggml-org/whisper.cpp`)，支持通过 `Metal` 在 GPU 上运行解码器 (`Decoder`)，并通过 `CoreML` (`WHISPER_COREML=1`) 将计算密集型的编码器 (`Encoder` 卷积与自注意力层) 卸载至 Apple Neural Engine (ANE)。

```c
// 源：https://raw.githubusercontent.com/ggml-org/whisper.cpp/master/include/whisper.h
struct whisper_context_params {
    bool  use_gpu;
    bool  flash_attn;
    int   gpu_device;  // CUDA device
    bool  dtw_token_timestamps;
};
struct whisper_full_params {
    enum whisper_sampling_strategy strategy;
    int n_threads;
    int audio_ctx;            // 缩短音频上下文窗口（默认 1500 = 30s）
    const char * initial_prompt; // 注入标点风格与专有名词热词
    bool no_context;
    bool single_segment;
};
```

* **架构特性与输入法适配瓶颈**：
  1. **原生非流式编码器 (`30s Padded Mel-Spectrogram`)**：标准 Whisper 编码器固定接收 30 秒（3000 帧梅尔频谱，`audio_ctx = 1500`）输入。若直接对 3 秒的短语音执行推理而不修改 `audio_ctx`，编码器仍会计算完整 30 秒张量。在输入法短语音场景下，需根据实际录音时长动态设置 `params.audio_ctx = ((actual_ms / 20) + 31) & ~31`（向上取整到 32 的倍数），可将 M 系列芯片上的编码耗时缩短 60%–80%。
  2. **伪流式 (`Local Agreement / Sliding Window`) 开销**：`whisper.cpp` 的 `stream` 示例通过每 `500 ms` 滑动窗口重跑一次 `whisper_full()` 实现流式预览，在长句输入时会持续占用 GPU/ANE 算力。
  3. **幻觉问题 (`Hallucination`) 与热词限制**：在静音或键盘敲击噪音段，Whisper 极易生成 `"字幕由 Amara.org 社区提供"`、`"谢谢观看"` 等训练语料幻觉，需前置 `Silero VAD` (`whisper_vad_context`) 剔除静音帧；其热词仅能通过 `initial_prompt`（上限 224 tokens）软性引导，容易在静音时把 `initial_prompt` 内容本身当作识别结果吐出。

#### 3.1.2 方案二：`sherpa-onnx` (新一代 Kaldi / ONNX Runtime)

`sherpa-onnx` (`k2-fsa/sherpa-onnx`) 基于 `ONNX Runtime` 提供纯 C API (`sherpa-onnx/c-api/c-api.h`)，同时支持**原生流式识别 (`SherpaOnnxOnlineRecognizer`)** 与**高速非流式识别 (`SherpaOnnxOfflineRecognizer`)**，在 macOS 上可通过 `provider = "coreml"` 或 `"cpu"`（利用 Accelerate `AMX` 矩阵协处理器运行 `int8` 量化模型）获得极高实时率 (`RTF, Real-Time Factor < 0.05`)。

```c
// 源：https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/sherpa-onnx/c-api/c-api.h
typedef struct SherpaOnnxOnlineRecognizerConfig {
  SherpaOnnxFeatureConfig feat_config;
  SherpaOnnxOnlineModelConfig model_config;
  const char *decoding_method; // "greedy_search" or "modified_beam_search"
  int32_t max_active_paths;
  int32_t enable_endpoint;
  const char *hotwords_file;   // 显式热词文件路径（支持每词独立加权）
  float hotwords_score;        // 默认热词奖励分（如 1.5 ~ 2.5）
} SherpaOnnxOnlineRecognizerConfig;
```

* **生产级实证：逆向 `DoubaoIme` 的离线 ASR 引擎**：
  在本次逆向分析中，检查 `/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme` 的字符串表与 `_TtC16OimeOfflineModel22ASROfflineModelManager`（符号地址 `0x101222bd0`），发现**豆包输入法的本地离线语音引擎正是基于 `sherpa-onnx` 的流式 `Zipformer Transducer` 架构构建**：

```text
// 源：/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme (strings 0x101222900-0x101222f00)
encoder.int8.onnx
decoder.onnx
joiner.int8.onnx
tokens.txt
model_manifest.json
DoubaoIme.ASROfflineModelStateDidChange
OimeEngine.SherpaCreateRecognizerFailed
OimeEngine.SherpaCreateStreamFailed
```

  `DoubaoIme` 在 `OimeEngine.framework` 内部静态链接了 `sherpa-onnx` 与 `onnxruntime`，通过 `encoder.int8.onnx` + `decoder.onnx` + `joiner.int8.onnx` + `tokens.txt` 四件套运行流式 `Transducer`。当用户按住快捷键说话时，音频帧通过 `SherpaOnnxAcceptWaveform(stream, 16000, samples, n)` 实时喂入，每 100 ms 调用 `SherpaOnnxDecodeStream(recognizer, stream)` 与 `SherpaOnnxGetOnlineStreamResult(recognizer, stream)` 即可零延迟拿出流式部分识别结果 (`partial text`)，完全无需像 Whisper 那样重算历史音频窗口。

* **`sherpa-onnx` 三大主力中文模型族对比**：
  1. **Streaming Zipformer (Bilingual zh-en)**：原生流式 `Transducer`（即 `DoubaoIme` 采用的方案），支持边说边出字，配合 `modified_beam_search` 原生支持 `hotwords_file` + `hotwords_score` 浅层融合 (`Shallow Fusion`) 热词激励，极其适合直接导出 Rime 用户词典 (`*.userdb` / `custom_phrase.txt`) 中的高频专有名词进行热词加权。
  2. **Paraformer (`paraformer-zh` / `streaming-paraformer-bilingual-zh-en`)**：阿里达摩院非自回归 (`CIF, Continuous Integrate-and-Fire`) 模型，中文准确率极高，支持时间戳与热词版 (`Contextual Paraformer`)。
  3. **SenseVoiceSmall (`sherpa-onnx-sense-voice-zh-en-ja-ko-yue`)**：非自回归多语言模型，`int8` 量化版约 `230 MB`，在 Apple Silicon M1–M4 上识别 10 秒音频仅耗时 **25 ms – 45 ms** (`RTF ≈ 0.004`)，且原生自带标点恢复 (`ITN, Inverse Text Normalization`) 与中英日韩粤五语种自动混输，但属于非流式模型（需配合 `Silero VAD` 切片实现伪流式或松开按键瞬间整句识别）。

#### 3.1.3 方案三：Apple 原生语音框架 (`SFSpeechRecognizer` 与 macOS 26 Tahoe `SpeechAnalyzer`)

Apple 提供了两代原生语音识别 API：

1. **`SFSpeechRecognizer` (`Speech.framework`, macOS 10.15+)**：
   * 通过设置 `let request = SFSpeechAudioBufferRecognitionRequest(); request.requiresOnDeviceRecognition = true` 强制离线识别，并通过 `request.contextualStrings = ["Rime", "鼠须管", "朙月拼音"]` 注入热词。
   * **工程痛点**：需要额外申请 `NSSpeechRecognitionUsageDescription` TCC 权限（弹出第二个系统权限框！）；端侧中文语音资产由系统后台 daemon (`speechrecognitiond`) 按需下载，用户首次开启时常遇到 `"On-device recognition is not available"` 错误；且 Apple 对单个 `SFSpeechRecognitionTask` 施加了约 60 秒的硬性超时限制，中英混输能力明显弱于 `SenseVoiceSmall` 与 `Paraformer`。
2. **`SpeechAnalyzer` + `SpeechTranscriber` (macOS 26 Tahoe WWDC25 新增 Swift 并发 API)**：
   * macOS 26 引入了全新的原生 Swift `async/await` 架构 `SpeechAnalyzer`，搭配 `SpeechTranscriber` 模块与 `AssetInventory` 接口（允许应用显式查询并管理端侧模型下载进度），彻底解除了旧版 `SFSpeechRecognizer` 的时长限制，在 Apple Silicon ANE 上延迟极低。但该 API 仅在 **macOS 26.0+** 可用，无法覆盖 macOS 13 Ventura / 14 Sonoma / 15 Sequoia 用户。

#### 3.1.4 本地离线 ASR 方案横向对比总表

| 评估维度 | `whisper.cpp` (`ggml` Metal / CoreML) | `sherpa-onnx` (`Zipformer` 流式) | `sherpa-onnx` (`SenseVoiceSmall` / `Paraformer` int8) | Apple `SFSpeechRecognizer` / `SpeechAnalyzer` |
| :--- | :--- | :--- | :--- | :--- |
| **代表模型与体积** | `whisper-small-q5_1` (`190 MB`) / `large-v3-turbo-q5_0` (`574 MB`) | `streaming-zipformer-bilingual-zh-en` int8 (`~75 MB`) | `sense-voice-zh-en-ja-ko-yue` int8 (`~230 MB`) | 系统内置托管 (`AssetInventory`, 进程内 `0 MB` 磁盘体积) |
| **运行时常驻内存 (RSS)** | `320 MB – 850 MB` | **`90 MB – 140 MB`** | `260 MB – 350 MB` | 进程内 `< 30 MB`（由系统 `speechrecognitiond` XPC 承载） |
| **推理模式与首字延迟** | 非流式（滑动窗口伪流式延迟 `400 ms – 800 ms`） | **原生真流式（帧级延迟 `60 ms – 120 ms`）** | 伪流式 VAD 分段 / 整句极速（10s 音频尾包延迟 **`30 ms – 50 ms`**） | 原生流式（`100 ms – 250 ms`） |
| **中文与中英混输准确率** | `small` 一般（易出繁体/幻觉）；`large-v3-turbo` 优 | 良（专精流式中英双语，无标点需后接标点模型或 LLM） | **优（中文 CER `< 3.5%`，SenseVoice 原生自带标点与 ITN）** | 纯中文良，中英代码术语混输较弱 |
| **热词注入 (`Hotword`)** | 弱（仅 `initial_prompt` 224 tokens 软提示） | **强（`modified_beam_search` 支持 `hotwords_file` 逐词加权）** | `Contextual Paraformer` 支持热词列表；`SenseVoice` 无原生解码器热词 | 中（`contextualStrings` 数组，权重不可控） |
| **额外 TCC 权限要求** | 仅麦克风权限 | 仅麦克风权限 | 仅麦克风权限 | **需麦克风 + 语音识别 (`NSSpeechRecognitionUsageDescription`) 双权限** |
| **Swift / C 集成复杂度** | C API (`whisper.h`) 简洁，需编译 `ggml-metal` | C API (`c-api.h`) 极成熟，官方提供 Swift 封装与静态库 | 同左（同一套 `sherpa-onnx` C API 即可同时加载流式与非流式模型） | 纯 Swift API，但受系统版本与系统后台语料下载状态制约 |

对于 Rime 客户端的本地离线 ASR 选型，采用 `sherpa-onnx` C API 作为统一底座——流式上屏模式加载 `Streaming Zipformer (int8)`（支持绑定 Rime 词库热词），松开整句高速识别模式加载 `SenseVoiceSmall (int8)`（自带标点与中英粤混输）——属于**候选（待用户确认）**方案。

---

### 3.2 本地小参数 LLM 润色与纠错引擎：`llama.cpp` vs `MLX-Swift` vs `Ollama`

语音识别输出的原始文本 (`Raw ASR Transcript`) 往往包含两类噪声：**口语不流畅现象 (`Disfluencies`)**（如语气词「呃、那个、就是说」、口吃重复「我我想要」、中途自我纠正「明天下午三点不对四点开会」）与**同音字/近音字错误 (`Homophone Errors`)**（尤其在结合当前输入框上下文或 Rime 用户词库时）。

在 Apple Silicon 上运行 `0.5B`、`1.5B`、`3B` 级别的 `Qwen2.5` 或 `Qwen3` (`Qwen3-0.6B` / `Qwen3-1.7B` / `Qwen3-4B`) 指令微调模型进行本地文本润色，共有三种主流推理引擎方案：

#### 3.2.1 三种本地 LLM 推理栈与 C/Swift 接口剖析

1. **`llama.cpp` (`libllama` C API + Metal GPU 后端)**
   * 采用单文件 `GGUF` 格式（如 `Q4_K_M` 或 `Q8_0` 量化），纯 C ABI 接口 (`llama.h`)，无 Python 或复杂外部依赖，支持直接将系统提示词 (`System Prompt`) 的键值缓存 (`KV Cache`) 锁定在内存中复用 (`Prompt Caching`)。

```c
// 源：https://raw.githubusercontent.com/ggml-org/llama.cpp/master/include/llama.h
struct llama_model * llama_model_load_from_file(const char * path_model, struct llama_model_params params);
struct llama_context * llama_init_from_model(struct llama_model * model, struct llama_context_params params);
int32_t llama_decode(struct llama_context * ctx, struct llama_batch batch);
llama_token llama_sampler_sample(struct llama_sampler * smpl, struct llama_context * ctx, int32_t idx);
```

   * **零冷启动前缀缓存技巧 (`Prefix KV Cache Reuse`)**：语音润色的 `System Prompt`（加上从 Rime 导出的 Top-N 个人高频词表）通常是固定不变的（约 150–300 tokens）。通过调用 `llama_memory_seq_rm(mem, 0, n_past_system, -1)`，每次新的语音识别结果到来时，只需保留前 `n_past_system` 个位置的 KV Cache，仅对新增的 `User Prompt`（几十个 tokens）执行 `llama_decode` 预填充 (`Prefill`)，可将首字延迟 (`TTFT, Time To First Token`) 压缩至 **15 ms – 35 ms**。

2. **`MLX-Swift` (`ml-explore/mlx-swift` + `mlx-swift-examples/LLM`)**
   * Apple 官方机器学习团队专为 Apple Silicon 设计的原生 Swift 数组与大模型框架，基于 `Metal` 统一内存零拷贝，提供极佳的 Swift Concurrency (`AsyncStream<String>`) 接口。
   * **工程边界**：`MLX` 强依赖 Xcode 编译生成的 `default.metallib` Metal 着色器库（使用纯 SwiftPM 命令行构建而未打包 `.metallib` 时会在运行时报错）；且需要 macOS 14.0+ (Sonoma) 及 Apple Silicon (`arm64`) 架构，不支持 Intel Mac。

3. **`Ollama` / `llama-server` 本地守护进程 (`Local HTTP Daemon`)**
   * 通过复用用户本机已运行的 `Ollama` (`http://127.0.0.1:11434/v1/chat/completions`) 或拉起独立子进程 `llama-server`，使用与云端 LLM 完全一致的 OpenAI 兼容 HTTP SSE 客户端代码调用。
   * **工程优势与边界**：输入法主进程内存占用为 **0 MB**；通过传参 `"keep_alive": "10m"` 控制模型在空闲 10 分钟后自动从显存卸载。若用户未安装 Ollama，则需输入法自带轻量化 `llama-server` 二进制守护进程。

#### 3.2.2 Apple Silicon 上 `Qwen2.5 / Qwen3` 规格、精度与延迟实测基准

以下为在 Apple Silicon (以 M2 / M3 16GB 统一内存为基准，`Q4_K_M` / `4-bit` 量化，输入 50 中文字 + 200 tokens 固定系统提示词前缀缓存，输出 50 中文字) 下不同参数规模的表现对比：

| 模型规格 | 量化权重体积 (`Q4_K_M`) | 运行时内存增量 (`RSS`) | 首字延迟 (`TTFT`, 含缓存) | 生成速度 (`TPS`, M2/M3) | 50字全文润色总耗时 | 各任务能力表现（去口语 / 标点 / 自我纠正 / 同音纠错） |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`0.5B / 0.6B`** (`Qwen2.5-0.5B` / `Qwen3-0.6B`) | `~390 MB` | `~520 MB` | **`12 ms – 25 ms`** | `160 – 240 tok/s` | **`180 ms – 280 ms`** | **标点恢复、语气词剔除（呃/那个）极佳**；复杂口语自我纠正（「三点不对四点」）偶有漏删；若未微调，偶发把润色指令当作问题回答（对话逃逸）。 |
| **`1.5B / 1.7B`** (`Qwen2.5-1.5B` / `Qwen3-1.7B`) | `~980 MB` | `~1.25 GB` | **`25 ms – 45 ms`** | `95 – 145 tok/s` | **`350 ms – 520 ms`** | **甜点级规格**：稳定处理自我纠正、重复词合并、中英术语大小写规范化（如 `xcode` -> `Xcode`）及结合上下文的同音字纠错。 |
| **`3B / 4B`** (`Qwen2.5-3B` / `Qwen3-4B`) | `~1.9 GB – 2.4 GB` | `~2.3 GB – 2.8 GB` | `45 ms – 85 ms` | `55 – 90 tok/s` | `600 ms – 950 ms` | 语篇级重写、正式邮件/代码注释风格转换质量最高，但 `>2 GB` 的常驻统一内存占用对 8GB 内存 Mac 存在内存交换 (`Swap`) 压力。 |

*注：若采用 `Qwen3` 系列模型，在构造请求时**需在 System Prompt 末尾或模板中加入 `/no_think`（或关闭 `enable_thinking: false`）**，否则模型会先输出冗长的 `<think>...</think>` 思维链，导致输入法上屏延迟增加数秒。*

#### 3.2.3 防止「对话逃逸 (`Conversational Hijacking`)」的结构化改写 Prompt 与语法约束

当用户口述一句疑问句（例如：「帮我问一下王总明天下午的预算评审会改到几点了」）时，通用 Instruct 模型极易误以为用户在向 AI 提问，从而输出：「好的，您可以这样向王总发送消息：……」。

为彻底杜绝此类「对话逃逸」，需同时采用 **XML 标签隔离输入 (`<asr_raw>`)** 与 **前缀强制引导 (`Assistant Prefill` / `Grammar Constraint`)**：

```xml
// 源：本地/云端语音后处理结构化 Prompt 模板设计
<|im_start|>system
你是输入法语音转写文本润色器，严禁回答用户问题或生成解释。请仅对 <asr_raw> 内的语音识别草稿执行：
1. 删除口头禅（呃、那个、就是）与口吃重复，执行口语中途自我纠正（如“三点不对四点”保留“四点”）；
2. 结合 <cursor_context> 与 <rime_hotwords> 修正同音错别字并补全标点，保持原意不变。/no_think<|im_end|>
<|im_start|>user
<cursor_context>前文：关于架构评审，</cursor_context>
<rime_hotwords>鼠须管, InputMethodKit, librime</rime_hotwords>
<asr_raw>呃那个帮我问一下鼠标管的输入法框架是不是用那个因普特卖色得</asr_raw><|im_end|>
<|im_start|>assistant
<polished>
```

通过将 `<polished>` 作为 Assistant 角色生成的静态前缀 (`Prefill`)，并将 `</polished>` 设为停止词 (`stop: ["</polished>", "<|im_end|>"]`)，模型在概率分布上被强制锁定在纯文本转写轨道，不会输出任何寒暄前缀。

#### 3.2.4 本地推理引擎的进程隔离架构：进程内 (`In-Process dylib`) vs 独立守护进程 (`XPC / Daemon`)

在 macOS 输入法中部署本地 ASR 与本地 LLM 时，架构层面的核心决策是**将模型加载在输入法主进程内，还是隔离在独立 XPC Service / Daemon 进程中**：

| 维度 | 进程内动态库 (`In-Process dylib`) | 独立 XPC 服务 / 后台守护进程 (`Standalone XPC / Daemon`) |
| :--- | :--- | :--- |
| **代表案例** | `DoubaoIme` 内的 `sherpa-onnx` 离线 ASR (`~90 MB`) | Apple `speechrecognitiond`、`Ollama` / 独立 `rime-ai-daemon` |
| **对普通打字稳定性的影响** | **高风险（针对 LLM）**：Metal GPU OOM、C++ 断言 (`GGML_ASSERT`) 或模型文件损坏会直接引发输入法主进程崩溃，导致用户连普通拼音打字都瞬间中断 | **零影响（故障隔离）**：推理进程崩溃或重启完全不影响主进程 `IMKInputController` 的 `librime` 键盘打字 |
| **空闲内存回收能力 (`Idle Eviction`)** | 弱：`malloc` / `Metal` 缓冲池在模型 `free` 后仍驻留于进程虚拟内存 (`Dirty Memory`)，导致输入法在活动监视器中持续显示占用 `1.5 GB+` 内存 | **极强**：语音输入空闲 N 分钟后，守护进程可直接 `exit(0)`，macOS 内核在 **1 ms 内 100% 回收全部物理内存与统一显存** |
| **IPC 通信开销** | 0 ms（直接进程内指针传递） | 极低：通过 Unix Domain Socket、Mach Port (`NSXPCConnection`) 或本地回环 HTTP 传递 PCM 音频流与文本，延迟 `< 0.5 ms` |

因此，对于轻量级流式 ASR (`sherpa-onnx` `< 150 MB`) 可进程内或 XPC 加载，而对于占用 `500 MB – 2 GB` 内存的本地 LLM (`llama.cpp` / `MLX` / `Ollama`) 采用独立守护进程 (`Daemon` / `XPC Service`) 并支持空闲自动退出释放内存，属于**候选（待用户确认）**方案。

---

## 四、云端流式 ASR、云端 LLM 后处理与 `IMKTextInput` 协同架构

### 4.1 云端流式语音与 LLM 后处理双通道网络协议设计

在云端协同模式下，系统需将「边说边上屏的流式 ASR」与「松开按键后的流式 LLM 润色」无缝衔接。

1. **通道 A：全双工 WebSocket 流式 ASR（按住说话期间）**
   * 使用 `URLSessionWebSocketTask`（或 `Network.framework` `NWConnection`）连接云端流式 ASR（如 OpenAI Realtime API `wss://api.openai.com/v1/realtime`、阿里 Paraformer/FunASR WebSocket、火山引擎豆包流式 ASR 等）。
   * **音频上行分帧与压缩**：采集管线每 `100 ms` 产出 `1600 采样点`（`3200 Bytes` 裸 PCM 16kHz Int16）切为一个数据帧；为降低弱网带宽，可通过 `Opus` 编码器（如 `DoubaoIme` 内置的 `_TtC9DoubaoIme15DoraOpusDecoder` / 编码器）压缩至 `~300 Bytes/帧` 发送。
   * **下行中间态解析**：服务端持续推送流式部分识别结果 (`partial / interim transcript`) 与句末确定片段 (`definite / final segment`)。
2. **通道 B：OpenAI 兼容 Chat Completions `SSE` 流式润色（松开说话瞬间）**
   * 用户松开热键时，向 WebSocket 发送 `{"type": "input_audio_buffer.commit"}`（或 EOF 尾包），等待最终尾包识别文本（设置 `400 ms` 超时兜底，超时则直接使用最后一帧 `partial` 文本），随后立即发起 `POST /v1/chat/completions` (`"stream": true`)。
   * **基于 `URLSession.bytes(for:)` 的零阻塞 SSE 解析**：

```swift
// 源：基于 Swift Concurrency URLSessionAsyncBytes 的 SSE 流式解析器设计
let (bytes, response) = try await urlSession.bytes(for: request)
for try await line in bytes.lines {
  guard line.hasPrefix("data: ") else { continue }
  let payload = line.dropFirst(6)
  if payload == "[DONE]" { break }
  if let delta = parseDeltaContent(from: payload) {
    await coordinator.onLLMStreamDelta(delta, sessionID: currentSessionID)
  }
}
```

使用 `bytes.lines` 在后台协作线程池 (`Cooperative Thread Pool`) 逐行解析 `data: {...}` JSON 块，提取 `choices[0].delta.content`，既避免了引入重型第三方网络库，又支持在用户按 `Esc` 或切换焦点时通过 `task.cancel()` 在微秒级中断底层 TCP 流。

---

### 4.2 与 `IMKTextInput` 的非阻塞状态机协调 (`setMarkedText` vs `insertText`)

在 macOS 输入法开发中，**所有对 `IMKTextInput` 客户端代理对象 (`client`) 的调用均需在主线程 (`@MainActor` / `DispatchQueue.main`) 执行，且单次调用不得阻塞等待任何网络或推理结果**。

先看鼠须管 (`SquirrelInputController.swift`) 中对预编辑文本 (`setMarkedText`) 与最终提交上屏 (`insertText`) 的标准调用方式：

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:296-306
func squirrelCommit(string: String) {
  guard let client = client else { return }
  // print("commitString: \(string)")
  client.insertText(string, replacementRange: .empty)
  preedit = ""
}
```

以及鼠须管在同一文件中通过 `NSAttributedString` 更新带下划线预编辑区 (`setMarkedText`) 的实现：

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:361-373
let attrString = NSMutableAttributedString(string: text)
attrString.setAttributes(attrs, range: NSRange(location: 0, length: text.utf16.count))
client.setMarkedText(attrString, selectionRange: caretPos, replacementRange: .empty)
```

当将这两套 API 应用于「流式 ASR + 流式 LLM 后处理」时，如果直接把网口收到的每个 token 塞进 `setMarkedText` 或 `insertText`，在真实的 macOS 桌面环境中会遭遇五大棘手问题。逆向 `DoubaoImeInputController`（偏移 `0x101429650`–`0x101429aa8`）的 110 余个实例变量，可以完整还原工业级语音输入法的解决方案：

```text
// 源：/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme (otool -ov DoubaoImeInputController ivars 节选)
0x1014296a8  asrTypewriterEmissionGeneration_  (流式打字机渐变输出代数令牌)
0x1014296d0  isASREmptyPreeditAnchorActive_    (空预编辑锚点激活标志)
0x101451cc8  asrLastShownMarkedText_           (上次渲染的 MarkedText 缓存)
0x101451ce8  asrFinalCommitStrategy_           (最终上屏策略：directInsert / paste)
0x101429918  asrPasteInFlight_ / asrPasteCommandVPosted_ (剪贴板 Cmd+V 回退状态)
0x101451d10  smartOrganizeUndoRecord_          (LLM 润色前后一键撤销记录)
0x101429a08  cachedTextBeforeCursor_ / cachedTextAfterCursor_ (光标前后上下文缓存)
```

#### 4.2.1 机制一：预编辑态至提交态视觉过渡与「空预编辑锚点 (`Empty Preedit Anchor`)」

在语音流式识别与 LLM 润色的全周期内，`IMKTextInput` 的文本呈现遵循以下状态转换：

```text
// 源：流式 ASR 与 LLM 润色在 IMKTextInput 上的状态转移图
[Idle 空闲] ──(按下语音热键：setMarkedText("\u{200B}") 锁定输入会话)──► [Recording 录音态]
   │ WebSocket 收到 partial 文本 (25Hz 节流)
   ▼
[Streaming ASR 预编辑预览] ──► client.setMarkedText(rawASR, ...) (下划线草稿)
   │ 松开热键，LLM SSE 吐出润色后 token
   ▼
[Streaming Polish 润色流]  ──► 在 setMarkedText 中打字机刷新润色文本，或记录 UndoRecord
   │ 收到 [DONE] 且通过 sessionGeneration 代数令牌校验
   ▼
[Final Commit 最终上屏]    ──► client.insertText(finalPolishedText) ──► [Idle 空闲]
```

* **为什么要设计 `isASREmptyPreeditAnchorActive_`（空预编辑锚点）？**
  当用户按下语音热键但尚未说话（前 `300 ms` 尚无 ASR 文字返回）时，如果预编辑区为空，部分宿主应用（如微信 macOS 客户端、Chrome 地址栏、Spotlight）在收到热键松开或焦点微调时会认为输入法处于空闲态，从而重置输入上下文 (`inputContext`)。通过在按下热键瞬间调用 `client.setMarkedText("\u{200B}", selectionRange: NSRange(location: 0, length: 0), replacementRange: .empty)` 植入一个不可见的零宽空格锚点，可强制宿主应用维持当前 `NSTextInputClient` 会话激活。
* **25 Hz (40 ms) 节流与打字机平滑器 (`asrTypewriterEmissionGeneration_`)**：
  `IMKTextInput` 的底层是跨进程 Mach IPC (`NSConnection` / `NSXPCConnection`)。若云端 LLM 以 `100 tokens/s` 推送 SSE 事件，每 10 ms 调用一次 `client.setMarkedText` 会引发跨进程同步 IPC 洪泛，导致 Electron 应用（VS Code、Slack、Obsidian）主线程严重掉帧。通过维护 `typeWriterStorage` 缓冲队列与 `40 ms` (25 fps) 主线程 `CADisplayLink` / `DispatchSourceTimer`，将多个 token 合并为一次 `setMarkedText` 更新，并在收到 `[DONE]` 时一次性调用 `client.insertText(finalText, replacementRange: .empty)` 消除下划线。

#### 4.2.2 核心机制二：获取光标上下文 (`attributedSubstring`) 进行同音纠错与标点续接

要让 LLM 准确修正同音字（例如前文刚提到「鼠须管配置」，用户紧接着说「打开它的羊文件」，LLM 结合前文即可纠正为「打开它的 YAML 文件」）并智能决定句首是否需要删除前文末尾多余标点 (`cachedPuncDeletedEndOfSentence`)，可在按下语音热键时通过 `IMKTextInput` 零权限读取光标前后的文本上下文：

```swift
// 源：基于 IMKTextInput 协议读取光标前文上下文的实现模式
let selRange = client.selectedRange()
if selRange.location != NSNotFound && selRange.location > 0 {
  let start = max(0, selRange.location - 100)
  let queryRange = NSRange(location: start, length: selRange.location - start)
  let contextBefore = client.attributedSubstring(from: queryRange)?.string
}
```

* **兼容性边界**：原生 AppKit (`NSTextView`) 与主流浏览器对 `selectedRange()` 和 `attributedSubstring(from:)` 支持良好；但部分终端模拟器（如 iTerm2、Alacritty）或旧版 Java Swing 应用会返回 `NSNotFound` 或 `nil`。此时可按需回退至 Accessibility API (`AXUIElementCopyAttributeValue(focusedElement, kAXValueAttribute / kAXSelectedTextRangeAttribute)`) 或仅使用当前会话内的历史已上屏缓存 (`cachedASRHistory`)。

#### 4.2.3 核心机制三：多策略上屏适配器 (`asrFinalCommitStrategy_`) 与剪贴板回退

在最终调用 `client.insertText` 上屏润色后的长文本（尤其是包含换行符 `\n` 的分点整理文本 `asrNeedDirectCommitNewline_`）时，不同宿主应用存在显著兼容性差异：

1. **标准通道 (`Direct Insert`)**：先确保 `setMarkedText("", ...)` 清空预编辑区（或直接在有 `markedText` 时调用 `insertText`，按 Cocoa 规范 `insertText` 会自动替换当前 `markedRange`），传入完整字符串。
2. **换行符拦截问题**：在微信、飞书、Telegram 等即时通讯软件的输入框中，直接通过 `insertText` 插入包含 `\n` 的多行文本，部分客户端会将 `\n` 解释为「立即发送消息」或直接丢弃换行。
3. **剪贴板事务回退 (`Paste Transaction Fallback`)**：如 `DoubaoImeInputController` 的 `asrPasteInFlight_`、`asrPasteRestoreSnapshot_`、`asrPasteWrittenChangeCount_` 所示，当检测到目标宿主不支持长文本/多行直接 `insertText`、或焦点位于非标准输入框（通过 `AXUIElement` 探测 `asrAXInputImplementation_`）时，引擎执行**无损剪贴板借用事务**：
   * 快照备份当前 `NSPasteboard.general` 的所有 `NSPasteboardItem` 数据及 `changeCount`；
   * 写入润色后的文本（标记 `org.nspasteboard.TransientType` 防止被第三方剪贴板历史工具如 Paste / Raycast 污染记录）；
   * 通过 `CGEvent` 合成发送 `Command + V` (`kVK_ANSI_V = 0x09`)；
   * 延迟 `150 ms – 250 ms`（`asrPasteRestoreWorkItem_`）后，校验 `NSPasteboard.general.changeCount == asrPasteWrittenChangeCount_`，若用户在此期间未手动复制新内容，则原样恢复备份的剪贴板快照。

#### 4.2.4 机制四：代数令牌 (`Generation Token`) 防重入与 LLM 润色一键撤销 (`Undo Record`)

1. **会话代数令牌 (`asrVoiceSession_` / `asrFinalGeneration_`)**：
   用户在流式语音或等待云端 LLM 返回的 `300 ms` 内，极可能做三件事：**按下键盘打字 (`asrUserInterruption_`)**、**用鼠标点击另一个窗口触发 `deactivateServer(_:)`**、**再次按下语音键开启新一轮录音**。
   为此，每次按下语音热键均需递增全局单调的 `UInt64` 代数令牌 `sessionGeneration`，并弱引用绑定当时的 `weak var boundClient: ( IMKTextInput & NSObjectProtocol )?`。任何异步流（音频回调、WebSocket 消息、LLM SSE `delta`）切换到 `@MainActor` 后，第一步需校验 `self.currentGeneration == taskGeneration && self.activeClient === boundClient`。若不一致，立即调用 `llmTask.cancel()` 并丢弃残留数据（或在 `deactivateServer` 时将已识别的 `cachedASRResult` 直接同步提交给旧 `client` 后再销毁会话）。
2. **LLM 润色前后一键回退 (`ASRSmartOrganizeUndoRecord`)**：
   LLM 润色偶发可能过度改写用户原意。通过维护 `ASRSmartOrganizeUndoRecord`（记录 `rawASRText`、`polishedText`、上屏时的光标起始位置 `expectedCursorLocation` 与生成代数），当上屏后用户在未移动光标的情况下立即按下 `Command + Z` 或退格键时，输入法可提示或一键将 `polishedText` 替换回原始 `rawASRText`。

对于 Rime 语音与 LLM 流式后处理的上屏机制，采用「`setMarkedText` 25Hz 节流展示流式 ASR 草稿 + 松开按键后通过 `URLSession.bytes` SSE 流式接收 LLM 润色并由 `sessionGeneration` 代数令牌校验保护 `insertText` 上屏 + 瞬态剪贴板 `Command+V` 兼容回退」属于**候选（待用户确认）**方案。

---

## 五、边界与限制、开放问题及项目技术选型候选清单

### 5.1 边界与限制

1. **macOS 系统版本与硬件架构边界**：Apple 原生 `SpeechAnalyzer` / `SpeechTranscriber` 仅在 macOS 26 (Tahoe) 及更高版本可用，无法覆盖 macOS 13–15 用户；而 `whisper.cpp` 与 `sherpa-onnx` 虽支持 macOS 13+，但在 Intel (`x86_64`) 老机型上缺乏 Apple Neural Engine (ANE) 与统一内存带宽优势，实时率（RTF）显著劣于 Apple Silicon (`arm64`)。
2. **`IMKTextInput` 同步 IPC 与宿主应用兼容性差异**：Electron/Chromium、JetBrains IDE（Java Swing/JBR）、终端模拟器（iTerm2/Alacritty）与原生 Cocoa `NSTextView` 对 `setMarkedText`、`attributedSubstring(from:)` 及多行 `insertText` 的实现程度不一，高频（>30 Hz）调用 `setMarkedText` 或在无焦点/非标准控件中直接插入换行符易引发闪烁或丢字。
3. **本地 LLM 常驻内存与首字延迟（TTFT）约束**：即便是 `Qwen2.5-1.5B` 或 `Qwen3-1.7B` 的 4-bit 量化模型，常驻内存仍需 `1.0–1.3 GB`；若直接嵌入输入法主进程，任何 Metal 显存分配异常或推理卡死均会直接威胁常规 Rime 键盘打字的稳定性。

### 5.2 开放问题

1. **本地推理守护进程的生命周期与空闲卸载阈值**：若采用独立 Daemon / XPC Service 运行本地 ASR 与本地 LLM，用户停止使用语音输入多久后应自动卸载 LLM 权重以归还统一内存？
2. **云端流式后处理与本地离线润色的降级切换条件**：当用户配置了云端 LLM 润色接口但遇弱网或首包超时（例如超过 `600 ms` 未收到首个 SSE `delta`）时，是直接提交原始 ASR 识别文本，还是自动回退至本地小模型润色？

---

### 5.3 项目技术选型候选清单（候选（待用户确认））

综合上述 macOS 系统机制、源码对标与二进制逆向实证，针对本语音增强型 Rime 客户端 (`~/repo/tools/rime-client`) 的工程架构，汇总以下技术选型方向（所有条目均为**候选（待用户确认）**）：

| 子系统模块 | 候选技术方案（候选（待用户确认）） | 核心工程依据与参数配置 |
| :--- | :--- | :--- |
| **1. TCC 麦克风权限与跨进程授权** | 由输入法主 Bundle 唯一声明 `NSMicrophoneUsageDescription` 与 `com.apple.security.device.audio-input = true`；设置面板通过 `NSDistributedNotificationCenter` 委托主进程请求授权，拒绝态弹出非激活 `NSPanel` 深度链接至系统设置 — **候选（待用户确认）** | 避免 `LSBackgroundOnly` 在输入中途首次弹窗打断宿主焦点；避免独立设置 App 夺取 TCC 授权归属导致主输入法进程无权限。 |
| **2. 音频采集与重采样管线** | 采用 `AVCaptureSession` + `AVCaptureAudioDataOutput` + 串行队列 `AVAudioConverter`（输出 16 kHz / Mono / Int16 或 Float32 PCM）并支持会话预热复用 (`Warm-up`) — **候选（待用户确认）** | 规避 `AVAudioEngine.inputNode` 在蓝牙/外设采样率突变时抛出无法捕获的 Objective-C `NSException` 崩溃；避免 `VoiceProcessingIO` 强制拉低系统媒体音质。 |
| **3. 快捷键与修饰键长按捕获** | 双轨架构：输入法激活态下纯修饰键长按（如 `Right Option` `0x080040`、`Right Control` `0x042000`、`Fn` `0x800000`）直接走 `IMKInputController.handle(_:client:)` 零权限通道（`180 ms` 防组合键误触计时器）；跨输入源或含普通键组合热键按需启用 `CGEventTap` 并自动处理 `tapDisabledByTimeout` — **候选（待用户确认）** | 利用 `NX_DEVICE*KEYMASK` (`0x207f`) 精准区分左右修饰键；在不强制要求用户开启辅助功能权限的前提下即可支持激活态长按说话。 |
| **4. Apple Silicon 本地离线 ASR** | 统一集成 `sherpa-onnx` C API (`c-api.h`)：边说边出字模式搭载 `Streaming Zipformer (zh-en, int8, ~75MB)` 并注入 Rime 个人词库 (`hotwords_file`)；松开整句闪识别模式搭载 `SenseVoiceSmall (int8, ~230MB)` — **候选（待用户确认）** | 与 `DoubaoIme` 逆向实证架构一致；内存占用低 (`90–300 MB`)，支持原生流式与 Rime 词库热词加权，无需申请额外的 `NSSpeechRecognitionUsageDescription` 权限。 |
| **5. Apple Silicon 本地文本润色 LLM** | 采用独立后台守护进程 (`Daemon` / `XPC Service`) 运行 `llama.cpp`（或复用本机 `Ollama`），默认搭载 `Qwen2.5-1.5B-Instruct` 或 `Qwen3-1.7B` (`Q4_K_M`, 强制 `/no_think`)，开启 `System Prompt` 前缀 KV Cache 锁定与空闲自动退出 — **候选（待用户确认）** | 进程隔离确保 Metal/LLM 异常绝不波及 Rime 日常键盘打字；空闲退出可 100% 释放 `~1.2 GB` 统一内存；前缀缓存将 TTFT 控制在 `25–45 ms`。 |
| **6. 云端流式协议与 `IMKTextInput` 协调** | 录音期通过 WebSocket 流式 ASR 搭配 `25 Hz` 节流的 `client.setMarkedText`（首帧前植入 `\u{200B}` 空预编辑锚点）；松开后通过 `URLSession.bytes(for:)` 解析 OpenAI 兼容 SSE 流，由 `UInt64` 会话代数令牌 (`sessionGeneration`) 校验后调用 `client.insertText` 上屏（辅以 `TransientType` 剪贴板 `Cmd+V` 兼容回退与润色前后 `UndoRecord`） — **候选（待用户确认）** | 杜绝跨进程 IPC 洪泛导致的宿主卡顿；防止焦点切换或按键打断引发的异步上屏串窗竞态；利用 `<asr_raw>` + `<polished>` 预填充彻底阻断 LLM 对话逃逸。 |

---

## 六、检索日志（Query Log）

| # | 查询途径 / 工具 | 检索指令或查询词 | 命中结果摘要 |
| :--- | :--- | :--- | :--- |
| 1 | `read` | `dev-docs/GUIDE.md`, `dev-docs/MAP.md`, `dev-docs/notes/001-research-voice-ime-index.md` | 获取调研报告格式规范、代码块 `<=15` 行限制、禁词表与子课题 `006` 边界定义。 |
| 2 | `read` | `dev-docs/refs/code/squirrel/resources/Info.plist`, `Squirrel.entitlements`, `SquirrelInputController.swift` | 提取鼠须管 `LSBackgroundOnly = true`、权限声明、`recognizedEvents` (`0x1400`)、`setMarkedText` 与 `insertText` 源码基线。 |
| 3 | `bash` (`plutil` / `codesign`) | `plutil -p '/Library/Input Methods/DoubaoIme.app/Contents/Info.plist'` 及 `codesign -d --entitlements :-` | 确认 `DoubaoIme` (v1.1.0) 同时配置 `LSBackgroundOnly = 1`、`NSMicrophoneUsageDescription` 与 `com.apple.security.device.audio-input = true`。 |
| 4 | `bash` (`nm` / `strings`) | `nm -gU '/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme'` | 空结果（主程序全局符号表已 strip，仅导出 `_main` 与 `_mh_execute_header`），转而使用 `otool -ov` 与 `strings` 分析 Objective-C/Swift 元数据。 |
| 5 | `bash` (`otool -L` / `strings`) | 检查 `DoubaoIme.app/Contents/Frameworks/OimeCommon.framework` 与 `OimeEngine.framework` 导出符号 | 提取 `OimeCommon` 中的 `ASRShortcutModifierFlags`、`InputSourceSwitchCoordinator`、`microphoneIPCTargetBundlePathKey` 等核心符号。 |
| 6 | `bash` (`otool -ov`) | 提取 `DoubaoIme` 中 `_TtC9DoubaoIme31DoubaoImeASRShortcutCoordinator` (`0x101130770`) 类元数据 | 定位 `shortcutRecordingEventTap` (`CFMachPort`)、`ignoreInputSourceChangeUntil`、`isASRRecordingActive` 等全局热键状态机字段。 |
| 7 | `bash` (`otool -tvV`) | 反汇编 `OimeCommon` `0x12ee4`–`0x1328c` (`ASRShortcutModifierFlags`) | 完整还原 `leftControl` (`0x1`) 至 `rightControl` (`0x2000`) 的硬件左右修饰键掩码 (`0x207f`) 与物理 `keyCode` (`0x36`–`0x3f`) 映射表。 |
| 8 | `bash` (`otool -tvV`) | 反汇编 `DoubaoIme` `0x100c3b570`–`0x100c3d950` (`_TtC14OimePermission22AudioPermissionManager`) | 还原 `AVCaptureDevice.authorizationStatus(for: .audio)`、跨进程通知 `DoubaoImeSettings.requestMicrophonePermission` 与拒绝态 `AuthorizationPromptWindow` 弹窗逻辑。 |
| 9 | `bash` (`otool -ov` / `otool -tvV`) | 提取 `_TtC9DoubaoIme14ASRAudioEngine` (`0x100f833a0`) 与 `_TtC9DoubaoIme26DoraAudioCaptureController` (`0x100f9b6a0`) | 确认其采用 `AVCaptureSession` + `AVCaptureAudioDataOutput` + `AVAudioConverter` + `isWarmedUp` 预热池及 `RecordingOutputMute` 扬声器静音机制。 |
| 10 | `bash` (`strings`) | 检索 `DoubaoIme` 中 `_TtC16OimeOfflineModel22ASROfflineModelManager` (`0x101222bd0`) 关联字符串 | 实证确认 `DoubaoIme` 离线语音识别基于 `sherpa-onnx` 流式 Transducer (`encoder.int8.onnx` / `decoder.onnx` / `joiner.int8.onnx` / `tokens.txt`)。 |
| 11 | `bash` (`otool -ov` / `otool -tvV`) | 提取 `DoubaoImeInputController` (`0x100e6bdb0`) 的 110+ 个 ivars 与 `recognizedEvents:` (`0x1001f2aa4`) | 还原 `isASREmptyPreeditAnchorActive_`、`asrTypewriterEmissionGeneration_`、`asrPasteInFlight_`、`smartOrganizeUndoRecord_` 等上屏协同机制。 |
| 12 | `mcp__magpie_web_search` | `"LSBackgroundOnly" "NSMicrophoneUsageDescription" macOS input method TCC microphone permission` | 获取 macOS Sonoma/Sequoia/Tahoe 下后台进程 TCC 责任进程归属 (`Responsible Process`) 与硬化运行时音频输入权限规范。 |
| 13 | `mcp__magpie_web_search` | `AVAudioEngine installTap vs AudioUnit VoiceProcessingIO kAudioUnitSubType_HALOutput latency macOS` | 验证 `AVAudioEngine.inputNode.installTap` 的缓冲下限 (`~10-100ms`)、采样率变更 `NSException` 缺陷以及 `VoiceProcessingIO` 的系统混音器副作用。 |
| 14 | `mcp__magpie_web_search` | `"sherpa-onnx" "SenseVoice" OR "Paraformer" vs "whisper.cpp" CoreML Apple Silicon benchmark RTF` | 获取 `sherpa-onnx` (`SenseVoiceSmall` / `Paraformer` / `Zipformer`) 与 `whisper.cpp` 在 Apple Silicon 上的 RTF、内存占用与热词能力对比。 |
| 15 | `mcp__magpie_web_search` | `"SpeechAnalyzer" OR "SpeechTranscriber" macOS 26 Tahoe on-device speech recognition Swift API` | 确认 Apple 在 macOS 26 (Tahoe / WWDC25) 新增 `SpeechAnalyzer` / `SpeechTranscriber` 与 `AssetInventory` API 的特性及系统版本边界。 |
| 16 | `bash` (`curl`) | 拉取 GitHub `ggml-org/whisper.cpp/.../whisper.h`、`k2-fsa/sherpa-onnx/.../c-api.h`、`ggml-org/llama.cpp/.../llama.h` | 核验三大 C/C++ 本地推理引擎的最新结构体签名 (`whisper_full_params`, `SherpaOnnxOnlineRecognizerConfig`, `llama_decode`)。 |

<!-- research-complete -->
