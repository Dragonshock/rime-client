# 029-code-analysis-type4me-session-orchestration：快捷键到 inject 的会话编排

本篇只追 Type4Me 一条调用链：快捷键打开录音、音频缓冲、SenseVoice 与 Qwen3 怎样变成事件、大模型在哪被调用、`inject` 在哪被调用。仓库是 `dev-docs/refs/code/type4me`，提交 `28353340207c5288f9dcaf2e71928cc3487d49cf`。工作区是稀疏检出，`Session/`、`Audio/`、`Input/`、`Services/` 没有落在磁盘上；下面的 `file:line` 来自该提交的 git 树（`git show HEAD:路径`），不是工作区缺文件就没有这些符号。

`027` 已经核对识别函数和 `inject` 的函数体。本篇不重复函数体，也不把那条剪贴板路径写成交付。旁路（改口、询问选区、Mac 操作、恢复中断）只在它们会提前离开这条链时点名。

## 1. 这条链选中的客户端

热键回调不自己挑识别器。`AppDelegate.requestRecordingStart` 把模式交给 `RecognitionSession.startRecording`。会话读当前识别供应商，再 `ASRProviderRegistry.createClient`。本地供应商 `.sherpa` 在 `HAS_SHERPA_ONNX` 下注册的是 `SenseVoiceASRClient`，能力是 `batch(supportsRealtimeRecognition: true)`。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/ASR/ASRProviderRegistry.swift:268-271
dict[.sherpa] = ProviderEntry(
    configType: SherpaASRConfig.self,
    createClient: { SenseVoiceASRClient() },
    capabilities: .batch(supportsRealtimeRecognition: true)
)
```

`ASRProviderCapabilities.batch` 把 `isStreaming` 写成 `false`，同时 `supportsRealtimeRecognition` 为 `true`（`ASRProviderRegistry.swift:8-37`）。会话因此把 sherpa 当成「停止时在 `endAudio` 里出定稿」的供应商，但录音过程中仍会收到实时预览事件。`audioInput` 默认 `.pcmData`，音频管道走 `sendAudio(Data)`，不走 `sendAudioBuffer`。

`SenseVoiceWSClient` 仍实现 `SpeechRecognizer`，但 `Sources/**/*.swift` 里没有 `SenseVoiceWSClient()` 构造。`SenseVoiceServerManager.qwen3WSURL`（`SenseVoiceServerManager.swift:66-68`）也只有定义。候选：这是旧的 WebSocket 客户端残留，当前热键链不进入它。`server.py` 的 `/ws` 仍会发 `is_final` 为假或真的 JSON，会话不订阅。

Qwen3 进程不在 `startRecording` 里拉起。选中 `.sherpa` 且 `ModelManager.isQwen3ASRBundled` 时，`Type4MeApp.applicationDidFinishLaunching` 里 `SenseVoiceServerManager.shared.start()`（`Type4MeApp.swift:523-531`）。端口记在 `currentQwen3Port`。

## 2. 符号索引

| 符号 | 位置 | 在这条链上的角色 |
| --- | --- | --- |
| `HotkeyManager.start` | `Input/HotkeyManager.swift:548` | 安装事件 tap，回调 `hotkeyCallback`（`1724`） |
| `HotkeyManager.handleTogglePress` | `Input/HotkeyManager.swift:958` | 空闲时开始，同一归属再按时停止 |
| `HotkeyManager.startRecording(with:)` | `Input/HotkeyManager.swift:1038` | 调用绑定的 `onStart` |
| `HotkeyManager.stopActiveRecording` | `Input/HotkeyManager.swift:1050` | 调用绑定的 `onStop` |
| `AppDelegate.registerHotkeys` | `Type4MeApp.swift:968` | 把 `onStart` / `onStop` 接到会话 |
| `AppDelegate.requestRecordingStart` | `Type4MeApp.swift:754` | 界面进入准备，再 `session.startRecording`（`797`） |
| `AppState.startRecording` | `UI/AppState.swift:1620` | 只改悬浮条相位，不碰麦克风 |
| `AppState.setLiveTranscript` | `UI/AppState.swift:1756` | 部分文本与已确认片段的展示 |
| `RecognitionSession.startRecording(purpose:)` | `Session/RecognitionSession.swift:1057` | 先开采集，再连接识别 |
| `AudioCaptureEngine.start` / `stop` | `Audio/AudioCaptureEngine.swift:319` / `436` | 采集；`stop` 会 `flushRemaining`（`677`） |
| `AudioCaptureEngine.emitFullChunks` | `Audio/AudioCaptureEngine.swift:632` | 满块才回调 `onAudioChunk` |
| `AudioChunkBuffer.append` / `drain` | `Audio/AudioChunkBuffer.swift:10` / `15` | 连接完成前暂存块 |
| `RecognitionSession.setupAudioChunkPipeline` | `Session/RecognitionSession.swift:3306` | 连接后把块送给 `sendAudio` |
| `SenseVoiceASRClient.sendAudio` | `ASR/SenseVoiceASRClient.swift:356` | 累积整段 PCM，并做 SenseVoice 部分解码 |
| `SenseVoiceASRClient.updatePartial` | `ASR/SenseVoiceASRClient.swift:587` | 部分文本，`isFinal == false` |
| `SenseVoiceASRClient.confirmSegment` | `ASR/SenseVoiceASRClient.swift:575` | VAD 切段后的已确认片段，仍 `isFinal == false` |
| `SenseVoiceASRClient.emitTranscript` | `ASR/SenseVoiceASRClient.swift:598` | 唯一的转写事件出口 |
| `SenseVoiceASRClient.endAudio` | `ASR/SenseVoiceASRClient.swift:462` | 收尾后调用 Qwen3，再发定稿 |
| `SenseVoiceASRClient.runQwen3Calibration` | `ASR/SenseVoiceASRClient.swift:637` | `POST /transcribe` |
| `server.transcribe_http` | `Sources/qwen3-asr-server/server.py:236` | 整段 PCM 的一次转写 |
| `RecognitionSession.handleASREvent` | `Session/RecognitionSession.swift:3119` | 写入 `currentTranscript`，并转给界面 |
| `RecognitionSession.stopRecording` | `Session/RecognitionSession.swift:1872` | 停采集、`endAudio`、排空事件 |
| `RecognitionSession.finishTextOutput` | `Session/RecognitionSession.swift:2245` | 片段替换、大模型、然后才 `inject` |
| `LLMClient.process` | `LLM/LLMClient.swift:97` | 会话在 `RecognitionSession.swift:2449` 调用 |
| `TextInjectionEngine.inject` | `Injection/TextInjectionEngine.swift:109` | 被 `RecognitionSession.swift:2709` 调用 |
| `TextInjectionEngine.injectTracked` | `Injection/TextInjectionEngine.swift:116` | 被 `RecognitionSession.swift:2700` 调用 |

`RecognitionSession.toggleRecording`（`931`）在 `Sources/**/*.swift` 里只有定义，热键不调用它。热键走的是 `startRecording` / `stopRecording`。

## 3. 快捷键怎样打开录音

`HotkeyManager.start` 在辅助功能权限可用时创建 `CGEvent` tap，掩码含键按下、抬起和修饰键变化，回调 `hotkeyCallback` → `handleEvent`（`724`）。`registerHotkeys` 为每个模式的每个热键做一个 `ModeBinding`，三个回调共用：

- `onStart`（`Type4MeApp.swift:973`）：相位已是 `.recording` 或 `.preparing` 时改走停止或取消；`.processing` 时直接返回；否则 `requestRecordingStart(source: .hotkey)`。
- `onStop`（`1072`）：`appState.stopRecording()`，准备中则 `cancelRecording`，否则 `session.stopRecording()`。
- 按住式绑定是按下走 `onStart`、抬起走 `onStop`（`HotkeyManager.handleHoldPress` `985`，`handleHoldRelease` `1021`）。切换式是第二次按下走 `stopActiveRecording`。

`requestRecordingStart`（`754`）先 `appState.startRecording()`，悬浮条变为 `.preparing`。然后等 `session.awaitIdle()`，再 `session.startRecording(mode:requestedAt:awaitIdleDuration:isAutomation:)`（`797`）。界面函数不启动 `AVCapture`。

`startRecording` 在 `finishing`、`injecting`、`postProcessing`、`recovering` 时直接返回（`1066`）。通过后 `sessionGeneration` 加一，状态改为 `.starting`，记下前台应用（自己的进程则目标为空）。

## 4. 音频缓冲：连接完成前不丢块

非手工输入时，麦克风早于识别连接启动。

1. `AudioChunkBuffer()`（`1124`）。
2. `audioEngine.onAudioChunk` 只做 `audioBuffer.append`（`1142`）。
3. `audioEngine.start()`（`1187`）。失败则状态回 `.idle`，发出 `.error` 与 `.completed`，没有识别事件。
4. 读模型配置，`createClient` 得到 `SenseVoiceASRClient`，`state = .recording`（`1442`），`markReadyIfNeeded` 发出 `.sessionReady`。此时识别器还没连上，采集已经在写缓冲。
5. `client.connect`（`1451`）。`SenseVoiceASRClient.connect`（`221`）读 `tf_sensevoiceEnabled`，默认真。为真时加载 sherpa-onnx 识别器与 VAD，发 `.ready`。为假时不建识别器，只发一条占位转写，日志写 Qwen3-only，然后返回。
6. 连接返回后才 `setupAudioChunkPipeline`（`1518`），`audioBuffer.drain()`（`1521`）把积压块 `yield` 进管道，然后把 `onAudioChunk` 换成直接 `yield`（`1529`），再 `drain` 一次补上切换缝（`1537`）。

采集侧格式在 `AudioCaptureEngine`：`sampleRate` 16000（`23`），`targetFormat` 为 16 kHz（`28`），默认 `chunkByteSize` 6400（`27`），注释对应约 200 毫秒的 PCM16。`convert`（`585`）把样本转成字节，追加到内部 `buffer` 和 `accumulatedAudio`。`emitFullChunks`（`632`）只有 `buffer.count >= activeChunkByteSize` 才回调。停录时 `stop`（`436`）调用 `flushRemaining`（`677`），把不足一块的尾巴也交出去。`getRecordedAudio`（`624`）返回的是 `accumulatedAudio`，供失败后的整段重试，不是悬浮条文本。

管道任务（`3306`）对 `.pcmData` 调用 `client.sendAudio(data)`。发送失败置 `uploadFailureFlag`，并 `beginStreamRecovery`。这条任务是 `Task.detached`，避免阻塞会话 actor 上的 `stopRecording`。

`SenseVoiceASRClient.sendAudio`（`356`）对每个块做两件事：`allAudioData.append(data)`，留给停止时的 Qwen3；若预览开着，再转 float32 送 VAD。开头 `skipInitialSamples = 6400`（`61`，`374`）不送进 VAD，注释写的是躲开开始提示音。整段 `allAudioData` 不含这个跳过。

## 5. 哪一步产出部分文本

部分文本只在预览开着、且 `endAudio` 尚未把 `finalized` 置真时出现。`sendAudio` 在 VAD 认为正在说话时把样本放进 `speechBuffer`。每 `partialRecognitionInterval = 3200` 个样本（约 200 毫秒，`76`）且没有在途的部分解码时，后台 `recognizer.decode`，再 `updatePartial`（`427-443`，`587`）。

VAD 弹出一段完整语音时，另起任务 `decode`，`confirmSegment`（`575`）把加过标点的文本追加到 `confirmedSegments`，并清空 `currentPartialText`。这一步的 `emitTranscript` 仍传入 `isFinal: false`。它是「这一段 SenseVoice 已切完」，不是本段录音的定稿。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/ASR/SenseVoiceASRClient.swift:598-608
private func emitTranscript(isFinal: Bool) {
    let composedText = (confirmedSegments + (currentPartialText.isEmpty ? [] : [currentPartialText])).joined()
    let transcript = RecognitionTranscript(
        confirmedSegments: confirmedSegments,
        partialText: currentPartialText,
        authoritativeText: isFinal ? composedText : "",
        isFinal: isFinal
    )
    eventContinuation?.yield(.transcript(transcript))
}
```

`isFinal == false` 时 `authoritativeText` 为空。`RecognitionTranscript.displayText`（`SpeechRecognizer.swift:76-77`）在权威文本为空时退回 `composedText`，也就是已确认片段加上当前部分文本。

`handleASREvent`（`3119`）先核对 `sessionGeneration`。非 `.ready` 的事件在 `3142` 转给 `onASREvent`。`.transcript`（`3148`）写入 `currentTranscript`。占位串不把 `hasReceivedASRText` 设真。非空展示文本会放行 `firstStreamingTextCont`；只有 `isFinal` 为真才放行 `finalTranscriptCont`（`isTranscriptEffectivelyFinal` 只看这个标志，`3945-3947`）。

`Type4MeApp` 把 `.transcript` 交给 `appState.setLiveTranscript`（`Type4MeApp.swift:312`，`AppState.swift:1756`）。已确认片段标成已确认，`partialText` 另成一条未确认。这里没有 `inject`，也没有 `LLMClient.process`。

预览关掉时，`connect` 发的占位 `partialText` 是 `ASRRecognitionMode.batchRecordingPlaceholder`，`sendAudio` 只追加 `allAudioData`。录音中途没有 SenseVoice 部分解码。

`server.py` 的 `/ws` 每新增约 2 秒发 `is_final: False`（`server.py:88`，`143-155`，`171-182`）。那条 JSON 不进入 `handleASREvent`。本链上的部分文本来自 sherpa-onnx 的 `decode`，不是 Qwen3。

## 6. 哪一步产出定稿

停止由热键 `onStop` 调用 `stopRecording`（`1872`）。状态先改成 `.finishing`（`1879`），然后 `audioEngine.stop()`（`1901`）把尾巴推进管道，`finishAudioChunkPipeline` 等发送任务排空。电平从未越过说话阈值时，断开识别器，发空的 `.processingResult` 和 `.completed`，回到 `.idle`，不进入 `finishTextOutput`。

sherpa 的 `isStreaming` 为假，因此不走「短录音先等部分文本」那一截（`1952` 起，条件是 `providerIsStreaming`）。`endAudio` 用 `BatchASRHTTP.requestTimeout + 5` 秒作超时（`2032-2036`；该常量是 120，`BufferedBatchASRClient.swift:109`）。`client.endAudio()` 返回之前，定稿事件已经从客户端发出。

`SenseVoiceASRClient.endAudio`（`462`）：

- 预览关着：`finalizeQwen3Only`（`613`）。短于 0.3 秒则空定稿。否则只信 `runQwen3Calibration` 的返回值。
- 预览开着：短于 0.3 秒则空的 `isFinal` 转写加 `.completed`。否则等在途的 `confirmSegment`（最多约 3 秒），`finalized = true` 以丢掉迟到的部分结果，`vad.flush()`，把剩余段解码并加标点。VAD 没有吐出段时，才用 `speechBuffer` 解码一次，避免和已弹出的段重复。
- 然后用整段 `allAudioData` 做 Qwen3。成功则 `confirmedSegments = [calibratedText]`，盖住刚才的 SenseVoice 片段。失败、开关关闭、没有端口、或音频不超过 3200 字节时，函数返回 `nil`，已有的 SenseVoice 片段保留。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/ASR/SenseVoiceASRClient.swift:542-548
if let calibratedText = await runQwen3Calibration(fallbackText: confirmedSegments.joined()) {
    confirmedSegments = [calibratedText]
}
allAudioData = Data()
emitTranscript(isFinal: true)
eventContinuation?.yield(.completed)
```

`runQwen3Calibration`（`637`）读 `tf_qwen3FinalEnabled`（默认真）和 `SenseVoiceServerManager.currentQwen3Port`。`qwen3Calibrate`（`664`）对 `http://127.0.0.1:<port>/transcribe` 发 `POST`，体是原始 PCM，超时 120 秒。响应 JSON 的 `text` 再经 `ASRHotwordLeakSanitizer.sanitize`。热词不在这个 HTTP 体里；`calibrationHotwords` 只供给这个清洗。服务进程的 `context` 仍是 `027` 写过的进程级字符串。

`transcribe_http`（`server.py:236`）把整段字节交给 `_transcribe(..., strip_punct=False)`。短于 100 字节直接 `{"text": ""}`。`_transcribe_sync`（`205`）在样本短于 0.3 秒或近乎静音时返回空字符串。这一次调用用的是全部累积音频，不是 `/ws` 那条「最近至多 20 秒」的部分窗口。

`emitTranscript(isFinal: true)` 把 `authoritativeText` 设成当时的 `composedText`。Qwen3 成功时，已确认片段只有这一条校准文本，部分文本已清空，所以权威文本与拼接文本相同。`setLiveTranscript` 里「权威文本不同于拼接文本」的分支因此不会因这次替换而单独触发；界面看到的是那一条已确认片段。

会话侧：`endAudio` 返回后，因为不是流式供应商，`finalArrived` 为假，进入事件排空（`2073` 一带）。消费任务见到 `.completed` 就结束（启动处在 `1508-1514`）。排空期间 `handleASREvent` 已把这条 `isFinal` 转写写进 `currentTranscript`。

`resolveEffectiveTranscript`（`3994`）对非流式供应商的规则是：没有 `isFinal` 就换成 `.empty`，避免把未确认片段送去上屏。SenseVoice 这条 `endAudio` 在 `.completed` 之前发了 `isFinal: true`，所以文本留下来。随后：

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Session/RecognitionSession.swift:2165-2171
currentTranscript = Self.resolveEffectiveTranscript(
    currentTranscript: currentTranscript,
    providerIsStreaming: providerIsStreaming
)
let effectiveText = currentTranscript.displayText
```

`effectiveText` 就是本链的识别定稿：优先 `authoritativeText`。它还不是 `inject` 的参数。空字符串时 `finishTextOutput` 不调用 `inject`，只发空的 `.processingResult` 和 `.completed`。

`canFireLLMAtStop`（`2183`）要求 `providerIsStreaming`。sherpa 不满足，`stopRecording` 里 `2214` 的 `client.process` 不会执行。流式供应商才会在进 `finishTextOutput` 之前把大模型任务先挂上。

## 7. 哪一步调用大模型

`finishTextOutput`（`2245`）收到的是上面的 `effectiveText`。`recordingPurpose` 为改口、或模式是询问选区时，在大模型和 `inject` 之前返回（`2268`、`2272`）。

`shouldRunInputModeLLM`（`4004-4009`）为真的条件是：录音目的是 `.input`，`mode.prompt` 非空，且 `executionKind == .recording`。内置快速模式 `ProcessingMode.direct` 的 `prompt` 是空字符串（`AppState.swift:653-659`），描述写「快速转写，不进行后处理」。这条链在快速模式下不调用 `LLMClient.process`。

提示非空时，sherpa 走 `finishTextOutput` 里 `finalLLMTask == nil` 的同步分支（`2376` 起）。顺序是：

1. 短文本豁免：`shortTextExemption > 0` 且字数更小，则 `needsLLM` 变假（`2314`）。
2. `state = .postProcessing`，发 `.enterProcessing`。
3. `resolveLLMRuntime`（`236`）取出 `LLMClient` 与 `LLMConfig`。
4. 先 `applyOutputSnippets`，再 `promptForCurrentMode(text:)`。
5. `client.process(text:prompt:config:inputBoundary:invocationContext:)`（`2449`）。等待上限约 15 秒，超时或空结果把 `llmFailed` 置真，展示文本退回识别原文。
6. 非翻译、非 Mac 操作时，`applyIntelliSenseGuard` 可能改写或退回原文，并发 `.processingResult`。

`.processingResult` 只更新悬浮条（`AppState.showProcessingResult`，`1798`）。它不调用 `inject`。

翻译模式失败或 Mac 操作走各自的完成函数并 `return`，不到下面的 `inject`。普通录音模式在大模型失败时仍继续，待写入的字符串保持识别定稿（经过片段替换之后的那份）。取消意图若使 `cancellationSkipsLLM` 为真（`3406`），会丢掉大模型结果，改回识别原文。

大模型的输入是字符串，不是 PCM。本机 HTTP 实现若被选成供应商，才会再进 `server.py` 的 `/v1/chat/completions`；那一步不在识别定稿的 `/transcribe` 里。

## 8. 哪一步才调用 inject

识别定稿和大模型返回之后，`finishTextOutput` 才做上屏。`finalText` 再经过 `formattedOutputText`（`2579`）。然后：

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Session/RecognitionSession.swift:2580-2582
onASREvent?(.willInject)
state = .injecting
```

`.willInject` 只让界面 `markWillInject`（`AppState.swift:1784`）。真正的调用在随后的 `Task.detached` 里，因为 `inject` 内部有 `usleep`，注释写不要堵住 actor。

普通、未取消、且目标计划允许写入时：

- `shouldTrackInjection` 为真：`engine.injectTracked(finalText, ...)`（`2700`）。
- 否则：`engine.inject(finalText)`（`2709`）。

取消且策略要求留下结果时走 `copyToClipboard`，不调用 `inject`。取消且不保留时结果是 `.discarded`。目标计划不允许写入当前前台时也是 `copyToClipboard`。函数体仍是 `027` 读过的 `TextInjectionEngine`；本篇只确定调用点，不把它写成端口交付。

detached 任务在 `inject` 返回后立刻发 `.finalized(text:injection:llmFailed:)`（`2724`）。`Type4MeApp` 把它交给 `appState.finalize`（`1829`）。历史记录在 actor 恢复之后写入，晚于这次 `inject`。

同一文件里还有 `injectRecoveryPartial`（`3065`）调用 `engine.inject`（`3072`）。那是流中断恢复，不是热键松开后的这条链。

空识别文本不到 `2580`。快速模式可以没有大模型，仍然在定稿非空时调用 `inject`。有提示词的模式是：部分文本不上屏，定稿先作为 `effectiveText`，大模型成功则替换该字符串，然后同一次 `inject` 写入替换后的 `finalText`。

## 9. 对照

| 步骤 | 产出 | `isFinal` | 调用 `inject` |
| --- | --- | --- | --- |
| `updatePartial` / `confirmSegment` | 部分文本或中途已确认片段，`authoritativeText` 为空 | 假 | 否 |
| `endAudio` 内的 SenseVoice 收尾，再 `runQwen3Calibration` | 一条转写；Qwen3 有字则整段替换，否则留下 SenseVoice 片段 | 真 | 否 |
| `finishTextOutput` 里的 `LLMClient.process` | 另一段字符串，或失败时沿用识别定稿 | 不是转写事件 | 否 |
| `state = .injecting` 之后的 `inject` / `injectTracked` | 无新文本；把 `finalText` 交给引擎 | — | 是 |

部分文本的消费者是 `handleASREvent` 与 `setLiveTranscript`。定稿的消费者是 `currentTranscript.displayText`，再作为 `finishTextOutput` 的 `effectiveText`。`inject` 的参数是其后的 `finalText`，中间还可能经过片段替换、大模型、守卫和 `formattedOutputText`。

## 检索日志

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | 读前两篇笔记 | `dev-docs/notes/027-research-type4me-pipeline.md`、`028-derivation-type4me-seam.md` | 识别与 `inject` 函数体已核对；本篇只追编排 |
| 2 | 看工作区与提交 | `dev-docs/refs/code/type4me`，`git rev-parse HEAD` | `2835334`；稀疏检出不含 `Session/` |
| 3 | 列提交内目录 | `git ls-tree Sources/Type4Me` | 有 `Session`、`Audio`、`Input`、`Services` |
| 4 | 搜 `inject(` | `git grep` `Sources/Type4Me/**/*.swift` | 会话内在 `2700`、`2709`、`3072` |
| 5 | 列 `RecognitionSession` 函数 | `git grep func` | `startRecording` `1057`，`stopRecording` `1872`，`finishTextOutput` `2245`，`handleASREvent` `3119` |
| 6 | 读热键分发 | `HotkeyManager.swift` `934-1100` | `handleTogglePress` → `onStart` / `onStop` |
| 7 | 读应用接线 | `Type4MeApp.swift` `149-420`、`754-860`、`968-1360` | 热键不调用 `toggleRecording`；`.transcript` 进悬浮条 |
| 8 | 读 `startRecording` | `RecognitionSession.swift` `1057-1560` | 先 `audioEngine.start`，缓冲，再 `connect`，再 `drain` |
| 9 | 读音频块 | `AudioChunkBuffer.swift` 全文；`AudioCaptureEngine.swift` `436-692` | 16 kHz，默认 6400 字节一块；停止时冲掉尾巴 |
| 10 | 读 SenseVoice 客户端 | `SenseVoiceASRClient.swift` `221-700` | 部分解码与 `endAudio` 里的 `POST /transcribe` |
| 11 | 读注册表 | `ASRProviderRegistry.swift` `1-50`、`268-276` | `.sherpa` 构造 `SenseVoiceASRClient`，`isStreaming == false` |
| 12 | 搜 `SenseVoiceWSClient(` | `Sources/**/*.swift` 与 `docs/` | 只有类型定义和归档计划；会话不构造 |
| 13 | 读 `server.py` | `100-270` | `/ws` 发部分与定稿 JSON；`/transcribe` 是整段一次，会话走后者 |
| 14 | 读停止与收尾 | `RecognitionSession.swift` `1872-2900`、`3994-4010` | sherpa 不在停止早段调用大模型；`inject` 在 `2580` 之后 |
| 15 | 读转写结构与界面 | `SpeechRecognizer.swift` `49-77`；`AppState.swift` `1756-1788` | 部分文本用拼接；`.willInject` 不上屏 |
| 16 | 读服务启动 | `Type4MeApp.swift` `523-531`；`SenseVoiceServerManager.swift` `66-150` | 启动时拉起进程；`qwen3WSURL` 无调用点 |
| 17 | 核对 `toggleRecording` 引用 | `git grep toggleRecording` | 仅 `931` 定义 |

<!-- research-complete -->
