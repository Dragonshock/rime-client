# 040-derivation-voice-commit-fix：识别进程改用带模块的解释器，上屏仍走 show/commit

这是修改方案，不是一组待选做法。已拍板的交付是 `voiceShow` / `voiceCommit` → `show` / `commit` → `IMKTextInput.setMarkedText` / `insertText`，不调用 `inject`。`037` 与 `033` 写的就是这条。`039` 读过 Type4Me：它的外部上屏不是这条路径。本方案不推翻已拍板的交付。

`~/Library/Logs/RimeClient/voice.log` 在 `2026-11-12T12:47Z` 连续出现 `recognition rejected No module named 'mlx_qwen3_asr'`。同一分钟更早两行是 `launch /usr/bin/python3` 加包内 `worker.py`，以及 `audio engine started`。按键和麦克风已经发生。识别没有返回文本，所以 `show` / `commit` 没有收到文字。先让识别进程使用带 `mlx_qwen3_asr` 的解释器，并让失败只记一次日志。然后再对照 Type4Me 的上屏。文字到达之后，调用现有的 `voiceShow` / `voiceCommit`。若改用 Type4Me 那种必须写剪贴板或合成按键的通道，那是需要另行确认的候选，不在本方案的改动里。

## 1. 日志与现有代码

写这篇时 `voice.log` 共 450 行：`launch` 7 行，`audio engine started` 7 行，`recognition rejected` 436 行。436 行的句子都是 `No module named 'mlx_qwen3_asr'`。`12:47:01Z` 的开头是：

```text
201:2026-11-12T12:47:01Z launch /usr/bin/python3 /Users/dragon/Library/Input Methods/RimeClient.app/Contents/Resources/worker.py
202:2026-11-12T12:47:01Z audio engine started
203:2026-11-12T12:47:01Z recognition rejected No module named 'mlx_qwen3_asr'
204:2026-11-12T12:47:01Z recognition rejected No module named 'mlx_qwen3_asr'
205:2026-11-12T12:47:01Z recognition rejected No module named 'mlx_qwen3_asr'
```

`launch` 来自 `PythonRecognitionTransport` 的启动日志。进程的可执行文件就是这一行里的解释器，参数是 worker 脚本。

```swift
// 源：code/VoiceInterface/Sources/VoiceInterface/RecognitionTransport.swift:17-20
public init(pythonURL: URL, workerURL: URL, onReply: @escaping (RecognitionReply) -> Void) {
    self.onReply = onReply
    process.executableURL = pythonURL
    process.arguments = [workerURL.path]
```

```swift
// 源：code/VoiceInterface/Sources/VoiceInterface/RecognitionTransport.swift:32-34
do {
    VoiceLog.write("launch \(pythonURL.path) \(workerURL.path)")
    try process.run()
```

`ok` 为假时，`applyReply` 写下拒绝并返回。返回之前没有 `acceptPartial`，也没有 `acceptFinal`。

```swift
// 源：code/VoiceInterface/Sources/VoiceInterface/VoiceHandleGate.swift:77-84
private func applyReply(_ reply: RecognitionReply) {
    guard let host else { return }
    guard reply.ok, let sessionID = reply.sessionID else {
        VoiceLog.write("recognition rejected \(reply.message ?? "")")
        return
    }
```

`show` 与 `commit` 只出现在效果已经产生之后。拒绝分支不走到这里。

```swift
// 源：code/VoiceInterface/Sources/VoiceInterface/VoiceHandleGate.swift:69-73
case .show(_, let text, let caret):
    host?.voiceShow(text: text, caretUTF16: caret)
case .commit(_, let text):
    host?.voiceCommit(text)
```

worker 把异常字符串放进 `ok` 为假的回复。`start`、`feed`、`finish` 各自在函数体内导入 `mlx_qwen3_asr.streaming`。导入失败时，主循环对这一条命令回一句，不回 `kind: partial` 或 `kind: final`。

```python
# 源：code/voice-worker/worker.py:24-27
def handle_start(message):
    global state, session_id
    from mlx_qwen3_asr.streaming import init_streaming
```

```python
# 源：code/voice-worker/worker.py:43-45
def handle_feed(message):
    global state
    from mlx_qwen3_asr.streaming import feed_audio
```

```python
# 源：code/voice-worker/worker.py:104-112
elif cmd == "finish":
    handle_finish(message)
elif cmd == "stop":
    reply({"ok": True, "kind": "stopped"})
    return
else:
    fail("unknown cmd")
except Exception as exc:
    fail(str(exc), message.get("session"))
```

因此每一帧 `feed` 都会再导入一次。导入失败就再回一句。这与 `12:47` 在 `audio engine started` 之后连续拒绝一致。这些回复不进入 `voiceShow` / `voiceCommit`。`VoiceLog` 不记录这两次调用，所以「没有调用」来自上面的 `return`，不是来自日志里的一条否定记录。

包内 `Resources/worker.py`、仓库 `code/voice-worker/worker.py`、`code/Client/dist/Squirrel.app/Contents/Resources/worker.py` 的 shasum 都是 `68de6ad18d938ed2d639ae60ab08460c61c4f764`。这次拒绝不来自脚本分叉。本方案不改 `worker.py`，也不改 `voiceWorkerURL()`。这次 `launch` 用的是包内脚本：`voiceWorkerURL()` 先取 `Bundle.main` 的 `worker.py`。注释写的源码树回退没有出现在这行 `launch` 里。

解释器来自 `voicePythonURL()`。可执行的 venv 不存在时，它返回 `/usr/bin/python3`。

```swift
// 源：code/Client/sources/SquirrelInputController.swift:659-672
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

`#filePath` 是 `code/Client/sources/SquirrelInputController.swift`。向上两级是 `code/Client`。拼出来的解释器是 `code/Client/voice-worker/.venv/bin/python3`。写这篇时该路径不存在。能 `import mlx_qwen3_asr` 的是 `code/voice-worker/.venv/bin/python3`：导入得到 `code/voice-worker/.venv/lib/python3.12/site-packages/mlx_qwen3_asr/__init__.py`。同一时刻 `/usr/bin/python3 -c "import mlx_qwen3_asr"` 的结果是 `ModuleNotFoundError: No module named 'mlx_qwen3_asr'`，与日志句子一致。

已安装的 `~/Library/Input Methods/RimeClient.app/Contents/MacOS/Squirrel`（修改时间 2026-11-12 20:45，本地时区）的字符串里有这份 `#filePath`，也有 `voice-worker` 和 `/usr/bin/python3`。字符串同时存在，只说明字面量编进了二进制。运行时证据是日志的 `launch /usr/bin/python3`。本篇没有单步调试那一次 `isExecutableFile` 的返回值。源码的回退与这行 `launch` 一致；源码构造的 `code/Client/voice-worker/.venv/bin/python3` 在磁盘上不存在。

`VoiceClientLocation.workerScript` 从 `code/Client/ClientLib` 向上三级，落到 `code`，再拼 `voice-worker/worker.py`。`sources` 与 `ClientLib` 同在 `Client` 下一级。`voicePythonURL()` 少退了一级，所以落到 `code/Client` 而不是 `code`。

`VoiceInputController` 的默认 `pythonURL` 仍是 `/usr/bin/python3`。它的默认 worker 是 `VoiceClientLocation.workerScript`，不是包内 `Resources/worker.py`。这次日志的 worker 路径对不上那条默认。输入法进程走的是 `SquirrelInputController.ensureVoiceGate`。本方案不改 `VoiceInputController`。

## 2. 修改：识别进程使用带该模块的解释器

改 `SquirrelInputController.voicePythonURL()`。从 `#filePath` 向上三级（`sources` → `Client` → `code`），再拼 `voice-worker/.venv/bin/python3`。该文件可执行时返回它。不可执行时返回 `nil`，不返回 `/usr/bin/python3`。

改后的 `voicePythonURL()` 如下。这是方案要写成的函数，不是已经落地的源码。

```swift
private static func voicePythonURL() -> URL? {
  var url = URL(fileURLWithPath: #filePath)
  url.deleteLastPathComponent()
  url.deleteLastPathComponent()
  url.deleteLastPathComponent()
  let venv = url
    .appendingPathComponent("voice-worker")
    .appendingPathComponent(".venv")
    .appendingPathComponent("bin/python3")
  guard FileManager.default.isExecutableFile(atPath: venv.path) else {
    VoiceLog.write("recognition interpreter missing \(venv.path)")
    return nil
  }
  return venv
}
```

这只在 `ensureVoiceGate` 创建门的那一次调用。`voiceGate` 已经存在时不再调用。缺文件时的那一行日志因此只写一次，不按帧重复。不要把这行日志挪到 `applyReply`：解释器缺席时进程没有启动，没有识别回复。

`LazyWorkerTransport` 保存 `URL?`。`send` 在 URL 为 `nil` 时直接返回，不创建 `PythonRecognitionTransport`，不写第二行。URL 非 `nil` 时，仍用现在的构造函数启动进程。启动日志会带上这个解释器的路径。

```swift
// 改后的 send。URL 缺席时不启动进程。
func send(_ command: RecognitionCommand) {
  guard let pythonURL else { return }
  if inner == nil {
    let bridge = bridge
    inner = PythonRecognitionTransport(pythonURL: pythonURL, workerURL: workerURL) { reply in
      bridge.onReply?(reply)
    }
  }
  inner?.send(command)
}
```

写这篇时，上面这个 venv 路径可执行，并且能导入 `mlx_qwen3_asr`。按本方案重新构建并替换 `~/Library/Input Methods/RimeClient.app` 之后，新进程的 `launch` 行应是该 `python3` 加上当时的 worker 路径。只改仓库源码不会改已经在跑的那份 `Squirrel`：日志里的 `launch /usr/bin/python3` 来自已安装二进制。进程何时重新读入新二进制，本篇没有在这次日志之后做切换实验，不写成已经验证的操作步骤。

`#filePath` 是编译进二进制的源码路径，不是 `RimeClient.app` 包内路径。改完之后，解释器仍在源码树的 `code/voice-worker/.venv`，不在输入法包里。这是现有函数的定位方式，只修正退级次数，并去掉不能导入该模块的系统解释器。

不在启动前再跑一次 `python -c "import mlx_qwen3_asr"`。导入发生在 worker 处理 `start` 或 `feed` 的时候。解释器选错时，失败回复仍走第 3 节，只记一次。

## 3. 修改：同一条失败只记一次日志

`applyReply` 的拒绝分支仍在调用 `acceptPartial` / `acceptFinal` 之前返回。同一条 `reply.message` 只写一次 `recognition rejected`。消息不同时，新的那条再写一次。记住的是 `VoiceHandleGate` 上的一条字符串，生命周期跟这枚门相同。`ensureVoiceGate` 会复用已有的门，所以同一次输入法进程里，同一句拒绝不会因为下一次按住再写一行。

```swift
// 改后的拒绝分支。成功分支保持现在的 acceptPartial / acceptFinal。
guard reply.ok, let sessionID = reply.sessionID else {
  let message = reply.message ?? ""
  if loggedRejection != message {
    loggedRejection = message
    VoiceLog.write("recognition rejected \(message)")
  }
  return
}
```

`loggedRejection` 只在 `applyReply` 里读写。`receive` 把 `applyReply` 投递到 `callbackQueue`，默认是主队列。不在 `VoiceLog.write` 里对所有句子去重。`audio engine started` 与 `launch` 仍按现在的次数写。

不因为记一次日志就停掉采集或停掉后续 `feed`。按住期间的帧仍可发送。`ok` 为假的回复仍然不产生 `.show` / `.commit`。worker 仍对每条失败命令回一句；主进程不再为同一句追加日志。不改 `worker.py` 的导入位置。

第 2 节的 `recognition interpreter missing` 与本节的 `recognition rejected` 是两种失败。前者没有回复。后者是回复被拒绝。各按自己的句子只记一次。

## 4. 文字到达后调用现有 show/commit

不新增上屏函数。不调用 `TextInjectionEngine.inject` 或 `injectTracked`。不写 `NSPasteboard`。不合成 Command-V。不改 librime 的键盘路径。

worker 回 `ok` 为真、且带会话标识时，仍走现在的门：

- `decoded` 为真的非最终回复：`acceptPartial` → `deliver` 的预编辑 → `.show` → `voiceShow` → `show` → `setMarkedText`。
- `kind` 为 `final`：`acceptFinal`。定稿原文为空时不调用 `commit`；若控制器预编辑仍等于本会话最后一次 `show` 的非空字符串，先 `.show` 空串。定稿原文非空时，改写记录按现在的 `RewriteAdapter.record` 写下，再 `deliver` 提交意图。
- 提交：`voiceCommit` → `commit` → `insertText`。若上次展示的非空字符串仍等于宿主预编辑，`deliver` 先给一个空串 `.show`，再 `.commit`。

`voiceShow` 与 `voiceCommit` 本身不判断识别是否成功。它们只转发。

```swift
// 源：code/Client/sources/SquirrelInputController.swift:586-592
func voiceShow(text: String, caretUTF16: Int) {
  show(preedit: text, selRange: NSRange(location: 0, length: 0), caretPos: caretUTF16)
}

func voiceCommit(_ text: String) {
  commit(string: text)
}
```

`deliver` 里这些条件保持不动：会话标识相同、寿命开放、交付仍允许、客户还在、bundle id 与打开时相同。预编辑还要求改写返回串缺席，且字符串等于识别记录。提交还要求识别记录已是定稿、改写记录不是未到、意图字符串等于改写记录里的原文。

`deliver` 的预编辑与定稿判断在 `code/VoiceInterface/Sources/VoiceInterface/Session.swift:141-155`。

```swift
guard current.deliveryAllowed, !host.clientMissing, host.bundleIdentifier == current.bundleIdentifier else {
    return []
}
switch intent.kind {
case .preedit:
    guard current.rewrite?.returned == nil else { return [] }
    guard let recognition = current.recognition, recognition.text == intent.text else { return [] }
    current.lastShown = intent.text
    session = current
    return [.show(sessionID: current.id, text: intent.text, caretUTF16: intent.text.utf16.count)]
case .commit:
    guard let recognition = current.recognition,
          recognition.kind == .finalText,
          recognition.text == intent.text
    else { return [] }
```

```swift
// 源：code/VoiceInterface/Sources/VoiceInterface/Session.swift:156-167
guard let rewrite = current.rewrite, rewrite.verdict != .notArrived else { return [] }
guard intent.text == rewrite.original else { return [] }
let shown = current.lastShown
current.lastShown = nil
session = current
if let shown, !shown.isEmpty, host.preedit == shown {
    return [
        .show(sessionID: current.id, text: "", caretUTF16: 0),
        .commit(sessionID: current.id, text: intent.text),
    ]
}
return [.commit(sessionID: current.id, text: intent.text)]
```

这次日志没有 `ok` 为真的回复。上面这些条件有没有挡住下一次成功识别，没有样本，不写成已经发生。`setMarkedText` / `insertText` 之后宿主屏幕变成什么，源码未写，仍以 `034` 与 `037` 为准。本方案不把屏幕上出现文字写成完成条件。完成指：解释器是第 2 节那个可执行文件；识别回复 `ok` 为真并带文本时，效果仍从现有的 `voiceShow` / `voiceCommit` 进去。

说话中的部分文字继续进预编辑。这是 `033` 的 D，也是 `037` 的交付。不把部分文字改成只留在悬浮条、结束时再粘贴。

## 5. Type4Me 的上屏和 setMarkedText / insertText 的差别

Type4Me 仓库是 `dev-docs/refs/code/type4me`，提交 `28353340207c5288f9dcaf2e71928cc3487d49cf`。外部应用上的最终文字走剪贴板，再合成 Command-V。`inject` 不接收 `IMKTextInput`。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Injection/TextInjectionEngine.swift:109-112
func inject(_ text: String) -> InjectionOutcome {
    guard !text.isEmpty else { return .inserted }
    return injectViaClipboard(text, trackingMetadata: nil).outcome
}
```

粘贴前写入 `NSPasteboard.general`。键是虚拟键码 9，旗标 `.maskCommand`，发到 `.cghidEventTap`。函数参数里没有目标 pid。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Injection/TextInjectionEngine.swift:193-197
copyToClipboard(text, transient: shouldRestoreClipboard)
let postWriteChangeCount = NSPasteboard.general.changeCount
usleep(50_000)

guard simulatePaste() else {
```

`simulatePaste` 在 `TextInjectionEngine.swift:238-252`。

```swift
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
```

前台是 Type4Me 自己、且自己的可编辑文本框拿得到焦点时，插入用的是 `NSTextView.insertText`，不是 `IMKTextInput.insertText`。

```swift
// 源：dev-docs/refs/code/type4me/Sources/Type4Me/Injection/TextInjectionEngine.swift:66-72
static func insertIntoOwnFocusedTextView(_ text: String) -> Bool {
    let insert: @MainActor () -> Bool = {
        guard let textView = ownEditableTextView(
            isAppActive: NSApp.isActive,
            firstResponder: NSApp.keyWindow?.firstResponder
        ) else { return false }
        textView.insertText(text, replacementRange: textView.selectedRange())
```

本项目的宿主调用在 `IMKTextInput` 上。`commit` 调用 `insertText` 后把控制器 `preedit` 清成空串。`show` 把属性串交给 `setMarkedText`。这两个函数里没有 `NSPasteboard`，没有 `CGEvent`。

```swift
// 源：code/Client/sources/SquirrelInputController.swift:724-726
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

两边都有名叫 `insertText` 的调用。Type4Me 那个是 `NSTextView` 的，只覆盖自己的文本框。本项目这个是 `IMKTextInput` 的，覆盖输入法正在服务的客户。不是同一个通道。

正常录音中的部分结果，Type4Me 写进悬浮条的 `segments`。`handleASREvent` 的 `.transcript` 从 `3148` 到下一条 `case .error`（`3176`）之间没有 `inject`。`Type4MeApp.swift:312-313` 把这条交给 `setLiveTranscript`。流中断且当时展示文本非空时，才 `injectRecoveryPartial` → `engine.inject`（`RecognitionSession.swift:3065-3072`）。那是例外，不是本方案要接上的路径。`inject` 与 `simulatePaste` 的函数体没有 `AXIsProcessTrusted`。`CGEvent.post` 到 `.cghidEventTap` 在操作系统上是否还要求进程受信任，这份函数体没有判断，不写成已从源码确立的前提。

| | Type4Me 正常最终文字 | 本方案保持的交付 |
| --- | --- | --- |
| 进入目标应用的 API | 外部：`NSPasteboard.setString` 后 `CGEvent` Command-V。自己的文本框：`NSTextView.insertText` | `setMarkedText` 与 `IMKTextInput.insertText` |
| 谁持有目标 | 前台应用，或自己的 `NSTextView` | 输入法客户 `client` |
| 说话中的部分文字 | 悬浮条。不调用 `inject` | `ok` 且 `decoded` 时 `voiceShow` |
| 剪贴板 | 外部投递要先写入 | `show` / `commit` 不写 |
| 这次日志 | 解释不了未上屏 | 拒绝发生在这两个调用之前 |

把交付换成 `inject` 也拿不到这次的文字：worker 没有交出文本。

## 6. 候选：改用 Type4Me 的上屏通道

这是候选，需要用户另行确认。确认之前不改 `voiceShow`、`voiceCommit`、`show`、`commit`，不调用 `inject` 或 `injectTracked`，不写剪贴板，不合成 Command-V。

若另行确认，外部应用上的最终文字才改成第 5 节那条：先 `NSPasteboard`，再合成 Command-V。自己的窗口若也要覆盖，才是 `NSTextView.insertText`。那时候 `voiceShow` / `voiceCommit` 不再是进入目标应用的 API。部分文字是否仍用 `setMarkedText`，要单独定；Type4Me 的正常转写不注入，只有流中断才 `injectRecoveryPartial`。改口用的 `AXSelectedText` 不是 `inject(finalText)` 那条通道，不把它当成替身。

这条候选离开已拍板的「交付走预编辑与提交，不接剪贴板粘贴」。它不能解释这次日志。

## 7. 不写成已经确立的事

- 已安装进程那一次 `isExecutableFile` 的返回值。没有单步。确立的是日志里的解释器路径、源码的回退，以及写这篇时两条 venv 路径哪条存在、哪条能导入。
- 解释器改完之后，宿主文档里一定出现字。`insertText` 已调用与屏幕结果不是同一件事。
- `deliver` 的客户与 bundle id 条件会不会拒绝下一次成功识别。这次没有 `ok` 为真的样本。
- 采集回调与 `send` 是否同一线程。缺解释器时的日志放在创建门的那一次调用上，不依赖这个判断。
- `CGEvent` 发到 `.cghidEventTap` 是否另外要求辅助功能权限。

## 8. 检索

- 读 `039`、`037`、`033`、`038`。`038` 写的是按 `037` 写代码时没有另类缺口；它没有覆盖这次日志里的解释器。
- 读 `SquirrelInputController.swift` 的 `voiceShow`、`voiceCommit`、`voiceWorkerURL`、`voicePythonURL`、`show`、`commit`，以及 `VoiceHandleGate.swift`、`Session.swift` 的 `deliver`、`RecognitionTransport.swift`、`worker.py`、`VoiceLog.swift`、`VoiceInputController.swift`。
- 读 Type4Me 的 `TextInjectionEngine.swift`：`inject`、`injectViaClipboard` 的剪贴板写入、`simulatePaste`、`insertIntoOwnFocusedTextView`。行号对应当前检出。
- `voice.log` 全文计数，见第 1 节。`shasum` 三份 `worker.py` 相同。
- `code/voice-worker/.venv/bin/python3` 可执行且能导入 `mlx_qwen3_asr`。`code/Client/voice-worker/.venv/bin/python3` 不存在。`/usr/bin/python3` 导入失败，句子与日志相同。
- 已安装 `Squirrel` 的 `strings` 含源码路径、`voice-worker`、`/usr/bin/python3`。

<!-- derivation-complete -->
