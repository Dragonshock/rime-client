# 027-research-type4me-pipeline：Type4Me 的识别、改写与上屏机制

本篇只核对 [joewongjc/type4me](https://github.com/joewongjc/type4me) 在本机浅克隆 `dev-docs/refs/code/type4me`（`28353340207c5288f9dcaf2e71928cc3487d49cf`，2026-11-12 读取）里，语音怎样变成文字、文字怎样进宿主字段。不把「接进来可以少写」写成设计。豆包输入法不在本篇拆解。

仓库自述是 macOS 14 及以上的菜单栏语音输入应用，许可证字段为 MIT。本地版说明内置 SenseVoice 与 Qwen3-ASR，约占 8 GB 内存，并写建议 32 GB 以上机器。云端版识别与大模型使用调用方自己的 API Key。这两句来自仓库 `README.md`，不是本机测量。

## 1. 包的边界

`Package.swift` 的产品是一个可执行文件 `Type4Me`，加上 `Type4MeUI`、`Type4MeIntelliSenseCore`、`Type4MeReviseCore`、`Type4MeTransferCore`、`Type4MeCorrectionCore`。识别客户端、大模型客户端和上屏引擎都在可执行目标 `Sources/Type4Me` 里，不在上述 library 产品里。

```swift
// 源：dev-docs/refs/code/type4me/Package.swift:99-107
products: [
    .executable(name: "Type4Me", targets: ["Type4Me"]),
    .library(name: "Type4MeUI", targets: ["Type4MeUI"]),
    .library(name: "Type4MeIntelliSenseCore", targets: ["Type4MeIntelliSenseCore"]),
    .library(name: "Type4MeReviseCore", targets: ["Type4MeReviseCore"]),
    .library(name: "Type4MeTransferCore", targets: ["Type4MeTransferCore"]),
    .library(name: "Type4MeCorrectionCore", targets: ["Type4MeCorrectionCore"]),
],
```

因此调用方不能 `import` 一个公开的识别库或上屏库。本地 SenseVoice 还依赖仓库外、由 `scripts/build-sherpa.sh` 生成的 `Frameworks/sherpa-onnx.xcframework`；该框架不在 git 树里。`AGENTS.md` 写 Qwen3-ASR 是 Apple Silicon 上的 Python WebSocket 服务，由 `SenseVoiceServerManager` 拉起。

## 2. 识别：SenseVoice 的部分结果，Qwen3-ASR 的定稿

`AGENTS.md` 的本地架构写明三档：只走 SenseVoice 流式、只走 Qwen3 定稿、或 SenseVoice 流式加 Qwen3 定稿校准。`SenseVoiceASRClient.connect` 把本次热词放进 `calibrationHotwords`。流式预览关掉时，它不建 SenseVoice 识别器，只发一条占位转写，日志写的是 Qwen3-only。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/ASR/SenseVoiceASRClient.swift:221-255
func connect(config: any ASRProviderConfig, options: ASRRequestOptions) async throws {
    guard let sherpaConfig = config as? SherpaASRConfig else {
        throw SherpaASRError.unsupportedConfig
    }
    calibrationHotwords = options.hotwords
    streamingPreviewEnabled = UserDefaults.standard.object(forKey: "tf_sensevoiceEnabled") as? Bool ?? true
    if !streamingPreviewEnabled {
        // Qwen3-only：不启动 SenseVoice，只占位
        return
    }
}
```

转写记录是 `RecognitionTranscript`：`confirmedSegments`、`partialText`、`authoritativeText`、`isFinal`。展示用 `authoritativeText`，空则把已确认片段和部分文本拼起来。这里没有「修订了哪一条假设」的字段。`SpeechRecognizer` 的输入是连接配置、音频字节或 `AVAudioPCMBuffer`，输出是 `AsyncStream<RecognitionEvent>`。

Qwen3 侧 `SenseVoiceWSClient` 把整段 PCM 攒到 `endAudio`，再请求本机服务做定稿；非空结果替换 `confirmedSegments`。推测性部分结果由 `scheduleSpeculativeQwen3` 另排。服务端在 `Sources/qwen3-asr-server/server.py`：WebSocket `/ws` 收 PCM16-LE，采样率写死 16000，每新增约 2 秒跑一次部分转写，部分转写去掉标点，空字节表示结束并做带标点的定稿。模型是 `mlx_qwen3_asr.Session`，调用 `sess.transcribe(audio, context=_hotword_context)`。短于 0.3 秒或近乎静音则返回空字符串，注释写这是为了避免模型把热词表念出来。

```python
# 源：dev-docs/refs/code/type4me/Sources/qwen3-asr-server/server.py:219-225
        if len(audio) < min_samples or np.sqrt(np.mean(audio ** 2)) < 1e-4:
            return ""
        try:
            result = sess.transcribe(audio, context=_hotword_context)
```

部分结果使用最近至多 20 秒音频，定稿使用累积的整段。这和 `024` 已测的 `feed_audio` 不是同一个函数：这里是服务进程里的 `Session.transcribe`，热词是进程级字符串 `_hotword_context`，不是五槽快照。

## 3. 文本优化：远程或 llama.cpp，不是 mlx-lm

Swift 侧 `LLMClient.process` 吃识别文本、提示词、`LLMConfig`（`apiKey`、`model`、`baseURL`）。`LLMPreparedPrompt` 把 `{text}`、`{selected}`、`{clipboard}` 换成转写、选中文字和剪贴板。隔离模式下转写放进 `<transcript>`，并写明把它当待处理文本，不当成要执行的指令。返回值是一段字符串，另有 `processStreaming` 的增量回调。没有「不替换」这个返回种类。

同一 Python 进程还有 `POST /v1/chat/completions`。模型用 `llama_cpp.Llama` 加载，`n_gpu_layers=-1`，与 ASR 共用 `_inference_lock`，避免同时进 Metal。用户消息若不是以 `/no_think` 开头，会补上这一行。这是 llama.cpp，不是 `017` 核对过的 `mlx_lm.load`。

## 4. 上屏：剪贴板与合成 Command-V

`TextInjectionEngine.inject` 不接收 `IMKTextInput`。外部应用在前台时，它把文本写入系统剪贴板，再投递带 Command 的 V 键按下与抬起。事件带进程内随机标记，供热键管理器忽略。粘贴发出后固定 `usleep(100_000)`；若要还原剪贴板，再 `usleep(300_000)`。注释写 Electron 应用可能要 200–500 毫秒才读完剪贴板。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Injection/TextInjectionEngine.swift:236-254
private func simulatePaste() -> Bool {
    let vKeyCode: CGKeyCode = 9 // 'v'
    guard let keyDown = CGEvent(keyboardEventSource: nil, virtualKey: vKeyCode, keyDown: true),
          let keyUp = CGEvent(keyboardEventSource: nil, virtualKey: vKeyCode, keyDown: false)
    else { return false }
    keyDown.flags = .maskCommand
    keyUp.flags = .maskCommand
    Self.markAsSyntheticInput(keyDown)
    Self.markAsSyntheticInput(keyUp)
    keyDown.post(tap: .cghidEventTap)
    keyUp.post(tap: .cghidEventTap)
    return true
}
```

自己的窗口在前台时，改走 `NSTextView.insertText`，避免把字粘进别的应用。没有可粘贴目标、或建不了事件时，结果留在剪贴板，结果值是 `.copiedToClipboard`。辅助功能用来在粘贴前后给聚焦控件做快照（角色、值、选区），供之后的改口观察，不作为写入通道。需要辅助功能权限。README 还写麦克风权限。

`type4me://toggle` 一类 URL 用来让外部脚本开关录音，不把转写返回给调用方。

## 5. 边界

- 识别与上屏绑在同一个可执行目标里。要只拿识别，需要从该目标里拆，仓库没有现成的识别产品。
- 流式部分结果来自 sherpa-onnx SenseVoice，定稿来自 MLX 的 Qwen3-ASR。本机 16 GB 上 `024` 只测过 `feed_audio` 的 0.6B 4-bit，没有测这个 WebSocket 服务，也没有测 SenseVoice 加 Qwen3 同时驻留的内存。
- 热词只进入 Qwen3 的 `context` 字符串。`host_app`、`field_role`、`cursor_before`、`corrections` 不是这些函数的形参。
- 大模型文本优化的本机实现是 llama.cpp 或外部 HTTP，不是 mlx-lm。
- 上屏会占用系统剪贴板一小段时间，并向当前前台应用发送 Command-V。它不调用 `setMarkedText`，也不区分键盘输入法自己的预编辑。

## 检索日志

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | 读取仓库元数据 | `gh api repos/joewongjc/type4me` | Swift，MIT，默认分支 main |
| 2 | 读取 README | 仓库根 `README.md` | 菜单栏应用；本地版 SenseVoice + Qwen3-ASR |
| 3 | 浅克隆 | `dev-docs/refs/code/type4me` @ `2835334` | sparse checkout 得到源码 |
| 4 | 读 `Package.swift` | 产品与目标 | 识别代码在可执行目标内 |
| 5 | 读 `AGENTS.md` 本地 ASR 节 | 双引擎描述 | SenseVoice 流式，Qwen3 定稿 |
| 6 | 读 `SpeechRecognizer.swift` | `RecognitionTranscript`、`SpeechRecognizer` | 无修订指针 |
| 7 | 读 `SenseVoiceASRClient.swift` | `connect` | 热词进入校准；可关流式预览 |
| 8 | 读 `SenseVoiceWSClient.swift` | `endAudio` | 整段 PCM 换 Qwen3 定稿 |
| 9 | 读 `server.py` | `/ws` 与 `_transcribe_sync` | PCM16、16 kHz、`context` 热词 |
| 10 | 读 `server.py` | `/v1/chat/completions` | `llama_cpp.Llama`，与 ASR 互斥锁 |
| 11 | 读 `LLMClient.swift` | `LLMClient`、`LLMPreparedPrompt` | 文本进、字符串出 |
| 12 | 读 `TextInjectionEngine.swift` | `inject`、`simulatePaste` | 剪贴板 + Command-V |
| 13 | 检索 `insertText` / `IMK` | 注入文件 | 只有自己窗口的 `NSTextView.insertText` |
| 14 | 读 README URL Scheme | `type4me://toggle` | 开关录音，不返回文本 |

<!-- research-complete -->
