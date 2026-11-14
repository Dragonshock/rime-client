# 039-code-analysis-type4me-text-commit：Type4Me 的语音上屏，以及不改 show/commit 时能改什么

本篇只回答四件事：Type4Me 的最终文字用什么 API 进入目标应用；部分文字会不会进入目标应用；注入前是否依赖辅助功能或剪贴板；这和本项目 `voiceShow` / `voiceCommit` → `show` / `commit` → `setMarkedText` / `insertText` 差在哪。仓库是 `dev-docs/refs/code/type4me`，提交 `28353340207c5288f9dcaf2e71928cc3487d49cf`。`Session/RecognitionSession.swift` 与 `Services/InjectedTextResolver.swift` 这次补进了稀疏检出，文件内容没有改。`UI/AppState.swift` 仍不在工作区，它的行号来自同一提交的 `git show`。

已拍板的交付是 `show` / `commit`，不调用 `inject`。本篇不把这句重开。Type4Me 的上屏不是这条路径。修改方案因此分成两截：不推翻已拍板时能改什么；若要改用 Type4Me 的上屏通道，那是需要另行确认的候选。

`029` 写过「部分文本不调用 `inject`」。那句话对录音中的 `.transcript` 仍然成立。流中断恢复是例外，见第 4 节。`docs/features/current-focus-injection/development-design.md` 里的 `inject(_:mode:)` 与 `postPasteKeyEvent(to:)` 不是当前 `TextInjectionEngine.swift` 的函数；该文档写的实现基线是 `0da8693`。下面的函数体以当前提交为准。

这次运行事实来自 `~/Library/Logs/RimeClient/voice.log`（2026-11-12 读取，450 行）。按键和麦克风已经发生：有 `launch` 与 `audio engine started`。识别没有返回文本：拒绝句是 `No module named 'mlx_qwen3_asr'`。第 7 节把这句接到代码。日志里没有带字的识别回复，所以这次不能把「宿主文档里有没有出现字」写成已经看过的屏幕结果。

## 1. 符号索引

| 符号 | 位置 | 在上屏上的角色 |
| --- | --- | --- |
| `TextInjectionEngine.inject` | `Injection/TextInjectionEngine.swift:109` | 最终文字的外部投递入口，转给 `injectViaClipboard` |
| `TextInjectionEngine.injectTracked` | 同文件 `116` | 同样走剪贴板与粘贴，并额外拍焦点快照 |
| `TextInjectionEngine.injectViaClipboard` | 同文件 `158` | 写 `NSPasteboard.general`，再 `simulatePaste` |
| `TextInjectionEngine.simulatePaste` | 同文件 `238` | `CGEvent` 合成 Command-V，发到 `.cghidEventTap` |
| `TextInjectionEngine.insertIntoOwnFocusedTextView` | 同文件 `66` | Type4Me 自己的 `NSTextView.insertText` |
| `TextInjectionEngine.copyToClipboard` | 同文件 `149` | 只留剪贴板，不发按键 |
| `RecognitionSession.finishTextOutput` | `Session/RecognitionSession.swift:2245` | 定稿与大模型之后才进入注入 |
| `engine.injectTracked` / `engine.inject` | 同文件 `2700` / `2709` | 允许注入时的两条调用 |
| `RecognitionSession.handleASREvent` 的 `.transcript` | 同文件 `3148` | 写入 `currentTranscript`，不调用 `inject` |
| `RecognitionSession.injectRecoveryPartial` | 同文件 `3065` | 流中断时把当时的展示文本交给 `inject` |
| `AppState.setLiveTranscript` | `UI/AppState.swift:1756` | 悬浮条片段，不是目标应用字段 |
| `Type4MeApp` 的 `.transcript` | `Type4MeApp.swift:312` | 把转写交给 `setLiveTranscript` |
| `TrackedTextReplacementEngine.replace` | `Injection/TrackedTextReplacementEngine.swift:75` | 改口替换；可写 `AXSelectedText`，不是口述最终文字的 `inject` |
| `SquirrelInputController.voiceShow` / `voiceCommit` | `code/Client/sources/SquirrelInputController.swift:586` / `590` | 转到 `show` / `commit` |
| `show` / `commit` | 同文件 `729` / `705` | `IMKTextInput.setMarkedText` / `insertText` |
| `VoiceHandleGate.applyReply` | `code/VoiceInterface/Sources/VoiceInterface/VoiceHandleGate.swift:77` | `ok` 为假时写拒绝日志并返回 |
| `SquirrelInputController.voicePythonURL` | `code/Client/sources/SquirrelInputController.swift:659` | 选择启动 worker 的解释器 |

`InjectedTextResolver.swift` 这次在检出里。它不出现在 `inject`、`injectTracked`、`injectRecoveryPartial` 的调用点上。它是否参与别的历史文本解析，本篇没有往下追，标成待加深。

## 2. 调用图

正常停录，外部应用在前台，且允许注入：

`finishTextOutput` → `onASREvent(.willInject)` → `state = .injecting` → `injectTracked`（`2700`）或 `inject`（`2709`）→ `injectViaClipboard` → `copyToClipboard` → `simulatePaste`。

Type4Me 自己是前台，且自己的文本框可编辑：

`injectViaClipboard` → `insertIntoOwnFocusedTextView` → `NSTextView.insertText`。这条不发 Command-V。源码注释写这条也不碰剪贴板；`shouldRestoreClipboard` 为假时，插入成功后仍会 `copyToClipboard`。

没有外部目标，或粘贴事件创建失败：

`copyToClipboard`，结果是 `.copiedToClipboard`。不调用 `setMarkedText`，也不调用 `insertText`。

录音进行中的转写：

识别事件 `.transcript` → `handleASREvent` 写入 `currentTranscript` → `Type4MeApp` → `setLiveTranscript` → `segments`。这条不到 `inject`。

流中断，且当时展示文本非空：

`beginStreamRecovery` → `injectRecoveryPartial` → `engine.inject`。恢复出的整段在 `finishRecovery` 里不再次调用 `inject`（`3012` 起只在策略要求保留时 `copyToClipboard`）。

本项目，识别回复 `ok` 且带文本：

`applyReply` → `acceptPartial` 或 `acceptFinal` → `voiceShow` / `voiceCommit` → `show` / `commit` → `setMarkedText` / `insertText`。

本项目，识别回复 `ok` 为假：

`applyReply` 写 `recognition rejected` 后返回。不调用 `acceptPartial` / `acceptFinal`。

## 3. 最终文字用什么 API 进入目标应用

`inject` 不接收输入法客户，也不接收标记文本。空字符串直接当已插入返回，否则进剪贴板路径。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Injection/TextInjectionEngine.swift:109-112
func inject(_ text: String) -> InjectionOutcome {
    guard !text.isEmpty else { return .inserted }
    return injectViaClipboard(text, trackingMetadata: nil).outcome
}
```

外部目标存在时，先把字符串放进系统剪贴板，再合成粘贴。`simulatePaste` 的键是虚拟键码 9，注释写的是 `v`，旗标是 `.maskCommand`。事件发到 `.cghidEventTap`，函数参数里没有目标 pid。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Injection/TextInjectionEngine.swift:193-197
copyToClipboard(text, transient: shouldRestoreClipboard)
let postWriteChangeCount = NSPasteboard.general.changeCount
usleep(50_000)

guard simulatePaste() else {
```

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Injection/TextInjectionEngine.swift:238-252
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

会话在 `2700` 与 `2709` 调用它们。调用前已经发出 `.willInject`，状态是 `.injecting`（`2576` 一带，`029` 记过）。允许注入为假时不调用这两个函数，改为 `copyToClipboard`（`2717`）。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Session/RecognitionSession.swift:2697-2711
if allowInjection {
    DebugFileLogger.log(injectLog)
    if shouldTrackInjection {
        result = engine.injectTracked(
            finalText,
            sourceText: rawText,
            sourceRecordID: recordId,
            modeID: modeID,
            shouldCaptureApp: authorizeTrackedCapture
        )
    } else {
        result = TrackedInjectionResult(
            outcome: engine.inject(finalText),
```

前台是 Type4Me 自己时，`resolveDeliveryTarget` 得到 `.fallbackToClipboard`。若自己的可编辑 `NSTextView` 拿得到焦点，插入用的是 AppKit 的 `insertText`，不是 `IMKTextInput`。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Injection/TextInjectionEngine.swift:66-73
static func insertIntoOwnFocusedTextView(_ text: String) -> Bool {
    let insert: @MainActor () -> Bool = {
        guard let textView = ownEditableTextView(
            isAppActive: NSApp.isActive,
            firstResponder: NSApp.keyWindow?.firstResponder
        ) else { return false }
        textView.insertText(text, replacementRange: textView.selectedRange())
        return true
```

因此：外部应用里的最终文字，源码写明的通道是 `NSPasteboard.setString` 加合成 Command-V。自己的文本框是 `NSTextView.insertText`。两处都没有 `IMKTextInput.setMarkedText`，也没有 `IMKTextInput.insertText`。`Injection/` 与 `Session/RecognitionSession.swift` 里没有这两个 IMK 符号。

`inject` 返回 `.inserted` 的时机是粘贴事件已经 `post`，不是读回目标字段证明字已经在文档里。`InjectionOutcome` 的 `.pasteAttemptedClipboardRetained` 注释写「发送了 Command-V，但插入无法证明」（`SpeechRecognizer.swift:84-88`）。当前 `injectViaClipboard` 在 `simulatePaste` 返回真时把结果记成 `.inserted`（`209`），不在该函数里改写成「无法证明」。

改口是另一条通道。`TrackedTextReplacementEngine.replace` 在 `AXSelectedText` 可写时调用 `setSelectedText`；否则再走剪贴板加 `accessibilityClient.paste()`（`TrackedTextReplacementEngine.swift:139-184`）。`SystemReviseAccessibilityClient.focusedControl` 在 `AXIsProcessTrusted()` 为假时直接返回没有元素的快照（`ReviseAccessibilityClient.swift:48`）。这不是 `finishTextOutput` 里那次 `inject(finalText)`。

## 4. 部分文字是否进入目标应用

录音中的转写只更新会话里的 `currentTranscript`，再交给悬浮条。`3148` 到下一条 `case .error`（`3176`）之间没有 `inject`。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Session/RecognitionSession.swift:3148-3150
case .transcript(let transcript):
    currentTranscript = transcript
    let trimmedDisplay = transcript.displayText.trimmingCharacters(in: .whitespacesAndNewlines)
```

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Type4MeApp.swift:312-313
case .transcript(let transcript):
    appState.setLiveTranscript(transcript)
```

`setLiveTranscript` 写的是 `segments`：已确认片段一条条放进去，`partialText` 非空时再追加一条未确认。过期的非最终转写会丢掉，不调用注入。

```swift
// 源：dev-docs/refs/code/type4me 提交 2835334，git show HEAD:Sources/Type4Me/UI/AppState.swift:1776-1781
segments = transcript.confirmedSegments.map {
    TranscriptionSegment(text: $0, isConfirmed: true)
}
if !transcript.partialText.isEmpty {
    segments.append(TranscriptionSegment(text: transcript.partialText, isConfirmed: false))
}
```

`displayText` 在 `authoritativeText` 为空时等于已确认片段拼上 `partialText`，否则等于权威文本（`SpeechRecognizer.swift:71-78`）。悬浮条用的是这个结构，不是目标应用的字段。

流中断是例外。恢复开始时取的是当时的 `displayText`，规范化后若非空，就调用 `injectRecoveryPartial`。函数名是 partial，装进去的字符串是展示文本，可以已经包含已确认片段，不只有未确认尾巴。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Session/RecognitionSession.swift:2923-2924
let rawPartialText = currentTranscript.displayText
let partialText = normalizedRecoveryText(rawPartialText)
```

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Session/RecognitionSession.swift:2959-2962
if !recoveryPartialText.isEmpty {
    await saveRecoveryHistory(status: "stream_partial_saved", finalText: recoveryPartialText)
    injectRecoveryPartial(recoveryPartialText)
}
```

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Session/RecognitionSession.swift:3065-3072
private func injectRecoveryPartial(_ text: String) {
    let engine = injectionEngine
    let retainsClipboardResult = clipboardOutputPolicy.retainsNormalResult
    Task.detached {
        engine.clipboardRetention = retainsClipboardResult
            ? .retainResult
            : .restoreOriginal
        _ = engine.inject(text)
```

`finishRecovery` 在恢复出非空整段时，只在 `retainsNormalResult` 为真时 `copyToClipboard`（`3024-3026`），并更新 `currentTranscript` 与悬浮条事件。这段没有 `inject`。非流式供应商若没有 `isFinal`，`resolveEffectiveTranscript` 换成 `.empty`，注释写的是避免把未确认片段送去注入（`3993-4004`）。那是停录定稿的过滤，不是悬浮条更新。

所以：正常说话过程中的部分结果不写入目标应用字段。流中断且当时展示文本非空时，会用与最终文字相同的 `inject` 写一次。恢复得到的整段不会在 `finishRecovery` 里再注入一次。

## 5. 注入前是否依赖辅助功能或剪贴板

剪贴板：外部投递依赖。`injectViaClipboard` 在 `simulatePaste` 之前调用 `copyToClipboard`，后者对 `NSPasteboard.general` 做 `clearContents` 与 `setString`（`149-156`）。自己的文本框插入成功、且策略要求恢复原剪贴板时，这条不把结果留在剪贴板上（`171-178` 的注释与分支）。没有外部目标或粘贴事件创建失败时，结果留在剪贴板上。停录日志字符串写的是 `method=clipboard`（`2665`）。`ClipboardOutputPolicy` 决定完成后留不留，不决定这一次粘贴前写不写；默认是 `.cancelProcessed`（`ClipboardOutputPolicy.swift:13`），`retainsNormalResult` 只在 `.alwaysCopy` 时为真（`52`）。

辅助功能：`inject` 与 `simulatePaste` 的函数体没有 `AXIsProcessTrusted`。粘贴不先查询控件是否可编辑。`injectTracked` 会在粘贴前后 `captureFocusedElementSnapshot`；未受信任时 `focusedElementSnapshot` 返回空元素快照（`TextInjectionEngine.swift:291-304`），粘贴本身已经在快照之前发出。`enableEnhancedAX` 在未受信任时直接返回（`256`）。

热键入口另说。`startHotkeyWithRetry` 在 `hotkeyManager.start()` 失败时打出需要辅助功能权限（`Type4MeApp.swift:1528-1531`）。重试先看 `PermissionManager.hasAccessibilityPermission`（`1564`）。自动化目标激活失败时，`activateAndConfirmFrontmost` 会写 `kAXFrontmostAttribute`，注释写 Type4Me 已经受辅助功能信任（`RecognitionSession.swift:869-873`）。那是把目标应用拉到前台的回退，不是 `inject` 的函数体。

因此不能从 `inject` 的函数体推出「没有辅助功能权限则粘贴一定失败」。源码写明的是：外部注入前一定写剪贴板；辅助功能检查在热键安装、自动化前台回退，以及追踪快照和改口替换上。`CGEvent.post` 到 `.cghidEventTap` 在操作系统上是否还要求进程受信任，这份函数体没有判断，本篇不把它写成已从源码确立的前提。

## 6. 和本项目 show/commit 的差

本项目的宿主是 `IMKTextInput`（`SquirrelInputController.swift:15`）。`voiceShow` 调用 `show`，`voiceCommit` 调用 `commit`。

```swift
// 源：code/Client/sources/SquirrelInputController.swift:586-591
func voiceShow(text: String, caretUTF16: Int) {
  show(preedit: text, selRange: NSRange(location: 0, length: 0), caretPos: caretUTF16)
}

func voiceCommit(_ text: String) {
  commit(string: text)
}
```

`show` 把字符串做成带 TSM 高亮的属性串，调用 `setMarkedText`。`commit` 调用 `insertText`，然后把控制器上的 `preedit` 清空。`force_marked_text_for_direct_commit` 为真、且当前预编辑为空时，`commit` 会先 `setMarkedText` 再 `insertText`。这两个函数里没有 `NSPasteboard`，没有 `CGEvent`，没有 `AXUIElement`。

```swift
// 源：code/Client/sources/SquirrelInputController.swift:720-724
client.insertText(string, replacementRange: .empty)
preedit = ""
hidePalettes()
```

```swift
// 源：code/Client/sources/SquirrelInputController.swift:745-748
let attrs = mark(forStyle: kTSMHiliteSelectedRawText, at: remainingRange)! as! [NSAttributedString.Key: Any]
attrString.setAttributes(attrs, range: remainingRange)
client.setMarkedText(attrString, selectionRange: NSRange(location: caretPos, length: 0), replacementRange: .empty)
```

门在识别回复上才产生这两种效果。部分回复且 `decoded` 为真时 `acceptPartial`，最终回复时 `acceptFinal`（`VoiceHandleGate.swift:87-91`）。`deliver` 里预编辑变成 `show`，提交变成 `commit`；若上次展示的字符串仍等于宿主预编辑，提交前先 `show` 空串（`Session.swift:145-168`）。

| | Type4Me 正常最终文字 | 本项目已拍板的交付 |
| --- | --- | --- |
| 进入目标应用的 API | 外部：`NSPasteboard.setString` 后 `CGEvent` Command-V。自己的文本框：`NSTextView.insertText` | `IMKTextInput.setMarkedText` 与 `insertText` |
| 谁持有目标 | 前台应用，或自己的 `NSTextView` | 输入法客户 `client` |
| 说话中的部分文字 | 悬浮条 `segments`。不调用 `inject` | 回复 `ok` 且 `decoded` 时，`voiceShow` → `setMarkedText` |
| 流中断时的展示文本 | 非空则 `injectRecoveryPartial` → `inject` | 这次日志没有这条恢复 |
| 注入前的剪贴板 | 外部投递要先写入 | `show` / `commit` 不写 |
| 注入前的辅助功能查询 | `inject` / `simulatePaste` 里没有 | `show` / `commit` 里没有 |
| 字已经在文档里 | `.inserted` 表示粘贴事件已发出 | `insertText` 已调用。屏幕结果源码未写，`034` 已记 |

两边都有一个名叫 `insertText` 的调用。Type4Me 那个是 `NSTextView` 的，只覆盖自己的文本框。本项目这个是 `IMKTextInput` 的，覆盖输入法正在服务的客户。不能把这两个 `insertText` 当成同一个通道。

## 7. 这次日志：识别没有把字送到交付

`voice.log` 共 450 行。其中 `launch` 7 行，`audio engine started` 7 行，`recognition rejected` 436 行，没有别的句子。`2026-11-12T12:47Z` 从第 201 行起连续是启动、采集开始，然后是拒绝。下面 12 行是该分钟开头：

```text
201:2026-11-12T12:47:01Z launch /usr/bin/python3 /Users/dragon/Library/Input Methods/RimeClient.app/Contents/Resources/worker.py
202:2026-11-12T12:47:01Z audio engine started
203:2026-11-12T12:47:01Z recognition rejected No module named 'mlx_qwen3_asr'
204:2026-11-12T12:47:01Z recognition rejected No module named 'mlx_qwen3_asr'
205:2026-11-12T12:47:01Z recognition rejected No module named 'mlx_qwen3_asr'
206:2026-11-12T12:47:01Z recognition rejected No module named 'mlx_qwen3_asr'
207:2026-11-12T12:47:01Z recognition rejected No module named 'mlx_qwen3_asr'
208:2026-11-12T12:47:01Z recognition rejected No module named 'mlx_qwen3_asr'
209:2026-11-12T12:47:01Z recognition rejected No module named 'mlx_qwen3_asr'
210:2026-11-12T12:47:01Z recognition rejected No module named 'mlx_qwen3_asr'
211:2026-11-12T12:47:02Z recognition rejected No module named 'mlx_qwen3_asr'
212:2026-11-12T12:47:02Z recognition rejected No module named 'mlx_qwen3_asr'
```

`launch` 的格式来自 `PythonRecognitionTransport`（`RecognitionTransport.swift:34`）。`audio engine started` 来自 `PCMSource.swift:68`。`recognition rejected` 只在 `applyReply` 里、`ok` 为假或没有会话标识时写下，随后返回。

```swift
// 源：code/VoiceInterface/Sources/VoiceInterface/VoiceHandleGate.swift:78-82
guard reply.ok, let sessionID = reply.sessionID else {
    VoiceLog.write("recognition rejected \(reply.message ?? "")")
    return
}
```

worker 把异常字符串放进 `ok` 为假的回复（`code/voice-worker/worker.py:21-22`，`108-109`）。`handle_start` 与 `handle_feed` 在检查会话之前导入 `mlx_qwen3_asr.streaming`（`27`、`45`）。导入失败时，每个 `start` 或 `feed` 都会回同一句，不会变成 `kind: partial` 或 `kind: final`。这与 12:47 的连续拒绝一致。这些回复不进入 `voiceShow` / `voiceCommit`，因此也不会为它们调用 `setMarkedText` / `insertText`。

包内 `worker.py` 与仓库 `code/voice-worker/worker.py` 的 shasum 都是 `68de6ad18d938ed2d639ae60ab08460c61c4f764`。拒绝不来自两份脚本分叉。

解释器是日志里的 `/usr/bin/python3`。当前源码里，输入法控制器把它当作可执行 venv 不存在时的回退：

```swift
// 源：code/Client/sources/SquirrelInputController.swift:659-671
private static func voicePythonURL() -> URL {
  var url = URL(fileURLWithPath: #filePath)
  url.deleteLastPathComponent()
  url.deleteLastPathComponent()
  let venv = url
    .appendingPathComponent("voice-worker")
    .appendingPathComponent(".venv")
    .appendingPathComponent("bin")
    .appendingPathComponent("python3")
  if FileManager.default.isExecutableFile(atPath: venv.path) {
    return venv
  }
  return URL(fileURLWithPath: "/usr/bin/python3")
}
```

`ensureVoiceGate` 把这个 URL 交给传输（`600`）。已安装 `RimeClient.app/Contents/MacOS/Squirrel` 的字符串里有 `#filePath` 对应的 `.../code/Client/sources/SquirrelInputController.swift`，也有 `voice-worker`、`.venv` 和 `/usr/bin/python3`。字符串同时存在，只说明两段字面量编进了二进制，不能单凭字符串证明那一次走了哪一支。运行时证据是日志的 `launch /usr/bin/python3`。

按这份源码，`#filePath` 向上两级是 `code/Client`，拼出来的解释器是 `code/Client/voice-worker/.venv/bin/python3`。2026-11-12 读取时该路径不存在。能 `import mlx_qwen3_asr` 的是 `code/voice-worker/.venv/bin/python3`（导入得到该包的 `__init__.py`）。同一时刻 `/usr/bin/python3 -c "import mlx_qwen3_asr"` 的结果是 `ModuleNotFoundError: No module named 'mlx_qwen3_asr'`，与日志句子一致。本篇没有单步调试已安装进程里那一次 `isExecutableFile` 的返回值。

`VoiceLog` 不记录 `voiceShow` / `voiceCommit`。这次日志里也没有别的句子可以代替那两次调用。所以「拒绝分支没有调用它们」来自上面的 `return`，不是来自日志里的一条否定记录。

## 8. 修改方案

Type4Me 的上屏不是 `show` / `commit`。这次日志里的未上屏发生在识别回复被拒绝，交付函数还没有被调用。把交付改成 `inject` 也拿不到文字：worker 没有交出文本。

### 8.1 不推翻已拍板时能改什么

交付保持 `voiceShow` → `show` → `setMarkedText`，以及 `voiceCommit` → `commit` → `insertText`。不调用 `TextInjectionEngine.inject` 或 `injectTracked`，不写剪贴板，不合成 Command-V，不改 librime 键盘路径。

改动落在 `voicePythonURL()` 选出的解释器。让它返回能导入 `mlx_qwen3_asr` 的那个解释器。当前仓库里这是 `code/voice-worker/.venv/bin/python3`，不是向上两级后的 `code/Client/voice-worker/.venv/bin/python3`，也不是没有该模块的 `/usr/bin/python3`。包内 `worker.py` 与仓库脚本一致，不必为了对齐 Type4Me 去换上屏 API。

解释器能导入之后，既有门才会看见 `ok` 为真的回复。`decoded` 的部分文本走 `voiceShow`。`kind` 为 `final` 的文本走 `voiceCommit`，还要过 `VoiceMachine.deliver` 里已经写下的条件：交付仍允许、客户还在、bundle id 与会话打开时相同、定稿时收束已经请求、改写记录已到且字符串等于原文（`Session.swift:138-168`）。这次日志没有 `ok` 为真的回复，所以上面这些条件有没有挡住下一次成功识别，本篇没有样本，不写成已经发生。`setMarkedText` / `insertText` 在目标应用里的屏幕结果，源码未写，仍以 `034` 为准。

说话中的部分文字会进输入法预编辑，这是已拍板路径，也是和第 4 节的差别：Type4Me 正常录音不把部分文字注入目标应用。不推翻已拍板时，不把部分文字改成「只留在悬浮条、结束时再粘贴」。

### 8.2 若要改用 Type4Me 的上屏通道

这是候选，需要另行确认。确认之前不改交付。

若确认，外部应用上的最终文字才改成第 3 节那条：先 `NSPasteboard`，再合成 Command-V。自己的窗口若也要覆盖，才是 `NSTextView.insertText`。那时候 `voiceShow` / `voiceCommit` 不再是进入目标应用的 API。部分文字是否仍用 `setMarkedText`，要单独定；Type4Me 的正常转写不注入，只有流中断才 `injectRecoveryPartial`。改口用的 `AXSelectedText` 不是这条口述最终文字通道，不把它当成 `inject(finalText)` 的替身。

这条候选会离开已拍板的「交付走预编辑与提交，不接剪贴板粘贴」。它不能解释这次日志：拒绝发生在任何上屏 API 之前。

## 9. 检索

- 读 `TextInjectionEngine.swift`、`RecognitionSession.swift` 的 `inject` 调用、`Type4MeApp.swift` 的 `.transcript`、`ClipboardOutputPolicy.swift`、`ReviseAccessibilityClient.swift`、`TrackedTextReplacementEngine.swift` 的替换段。
- `git show HEAD:Sources/Type4Me/UI/AppState.swift` 的 `setLiveTranscript`（`1756-1782`）。
- 读 `SquirrelInputController.swift` 的 `voiceShow`、`voiceCommit`、`show`、`commit`、`voicePythonURL`，以及 `VoiceHandleGate.swift`、`Session.swift` 的 `deliver`、`worker.py`。
- `rg` 在 `Injection/` 与 `RecognitionSession.swift` 中查 `setMarkedText`、`insertText`、`inject(`、`AXIsProcessTrusted`。IMK 的两个符号只在本项目控制器里出现。
- 读 `~/Library/Logs/RimeClient/voice.log` 全文并计数：450 行，7 次 `launch`，7 次 `audio engine started`，436 次 `recognition rejected`。上表是 12:47 的前 12 行。
- `/usr/bin/python3 -c "import mlx_qwen3_asr"` 得到 `ModuleNotFoundError`。`code/voice-worker/.venv/bin/python3` 的导入得到该包路径。`code/Client/voice-worker/.venv/bin/python3` 不存在。
- 已安装二进制字符串含源码路径与 `/usr/bin/python3`。两份 `worker.py` 的 shasum 相同，见第 7 节。

<!-- research-complete -->
