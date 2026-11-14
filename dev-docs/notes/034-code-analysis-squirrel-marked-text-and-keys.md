# 034-code-analysis-squirrel-marked-text-and-keys：预编辑缓存、普通键与失焦提交

本文只回答三件事：语音侧若复用 `show(preedit:selRange:caretPos:)` 与 `commit(string:)`，一份预编辑已经写进控制器缓存并调用过 `setMarkedText` 之后，随后一枚普通键进入 `handle` 与 `processKey` 会碰到哪些状态；`insertText` 与 `setMarkedText` 是否共用控制器上的 `preedit` 缓存；走到 `commitComposition` 时提交的是什么。`031` 仍是未确认蓝图，本文不改它。A、D、G、J、L 五项不重新打开。交付仍不调用 Type4Me 的 `inject`，也不改 librime 的键盘路径。

基线：`dev-docs/refs/code/squirrel` 提交 `b7970a93b05ae1ca1bcd97888fd6aa3cd0458d3e`；`dev-docs/refs/code/librime` 提交 `bb56876890ba60f446fa2640c244b1794b965eda`。行号按这两棵树。`005` 里若干旧行号与现行文件不一致，本文不沿用。

「字段上已经有预编辑」在源码里只能落到控制器：`show` 把参数写入 `self.preedit` 并调用 `client.setMarkedText`（`SquirrelInputController.swift:586-605`）。宿主屏幕上当时显示什么，源码未写出。下面说的状态都是这两棵树里读得到的字段、选项和分支，不是屏幕观察。

普通键在本文里指：`event.type == .keyDown`、修饰不含 `.command`、译出的 `rimeKeycode != 0`。这条路径才会进入 `processKey`（`95-116`）。含 `.command` 时 `handle` 在 `97-99` 直接跳出，不调用 `processKey`，也不调用 `rimeUpdate`。

## 1. 普通键进入 handle 与 processKey 碰到的状态

`show(preedit:)` 不调用 `process_key`、`set_input` 或 `clear_composition`。它留下的 `preedit`、`selRange`、`caretPos` 不是 `handle` 或 `processKey` 的输入。这两个函数也不读 `inlinePreedit`、`inlineCandidate`，也不读客户上的标记文本。

`handle`（`32-124`）在调用 `processKey` 之前碰到：

1. `session` 是否为 0，以及 `rimeAPI.find_session(session)`（`40-45`）。无效则 `createSession`：重设 `currentApp`，`session = create_session()`，`schemaId = ""`（`362-375`，赋值在 `368-370`）。已有会话不进这个分支。
2. `client`，用 `?=` 换成本次 `sender`（`47`；`T?` 重载在 `BridgingFunctions.swift:52-56`，右侧为 nil 时不覆盖）。
3. `currentApp` 与 `client.bundleIdentifier()`（`48-51`）。不一致则 `updateAppOptions`，按配置 `set_option`，并在 `unsafe/report_bundleid` 为真时 `set_property(..., "client_app", ...)`（`377-392`，写入在 `384` 与 `389`）。
4. `lastModifiers` 只参与函数开头的 `changes`（`35`）。`.keyDown` 分支不写它。
5. 修饰里的 `.command`（`97-99`）。
6. `keyCode`、`charactersIgnoringModifiers` 或 `characters`、`.shift`、`.capsLock`，合成 `rimeKeycode` 与 `rimeModifiers`（`101-113`）。

`processKey`（`402-435`）碰到：

1. 候选窗 `panel.linear`、`panel.vertical` 与会话选项 `_linear`、`_vertical`。不一致就 `set_option`（`403-410`）。
2. `rimeAPI.process_key(session, Int32(rimeKeycode), Int32(rimeModifiers))` 的返回值（`412`）。实现是 `RimeProcessKey`（`librime/src/rime_api_impl.h:171-178`）到 `Session::ProcessKey`（`librime/src/rime/service.cc:26-27`）到 `ConcreteEngine::ProcessKey`（`librime/src/rime/engine.cc:167-195`）。引擎按已启用的 `processors_` 调用 `ProcessKeyEvent`；未被接受且非合成键时写入 `commit_history`（`181-183`）；再走 `post_processors_` 与 `unhandled_key_notifier`（`185-193`）。`Compose` 使用的是 `ctx->input()` 与 `composition()`（`engine.cc:231-237`），不是控制器 `preedit`。哪一个 processor 接受这一枚普通键，`SquirrelInputController.swift` 未写出。
3. 返回假：读选项 `vim_mode`、`ascii_mode`。键是 Esc，或 Ctrl 加 `c` / `C` / `[`，且 `vim_mode` 为真、`ascii_mode` 为假时，把 `ascii_mode` 设为真（`414-419`）。
4. 返回真：键是否落在空格到 `~` 或左右 Control / Alt / Shift；再读 `_chord_typing`。为真则 `updateChord` 写入 `chordKeyCodes`、`chordModifiers`、`chordKeyCount`、`chordDuration`、`chordTimer`（`421-428`，写入在 `332-349`）。否则若该键不带释放掩码，`clearChord`（`429-430`，函数在 `352-360`）。

`processKey` 返回后，只要 `rimeKeycode != 0`，`handle` 就调用 `rimeUpdate()`（`112-115`）。`handled` 来自 `processKey`（`114`），在 `123` 交回 IMK。`37` 行注释：返回真则消费该事件，返回假则把事件交给客户应用。交给客户应用之后标记文本变成什么，源码未写出。

`rimeUpdate`（`448-560`）仍在这一枚键的 `handle` 里，语音缓存是在这里被改写的。顺序是先取提交，再取上下文：

1. `specialCommentIndices` 在 `clearReservedComments` 为真时清空（`449-451`）。`handle` 这次调用使用默认参数，会清空。
2. `rimeConsumeCommittedText`（`452`，函数 `437-445`）。`get_commit` 在 librime 里只在 `commit_text` 非空时返回真并 `ResetCommitText`（`rime_api_impl.h:309-324`）。有文本则 `commit(string:)`（`441`）。此时若语音留下的 `preedit` 非空，`force_marked_text_for_direct_commit` 那次 `setMarkedText` 不进入（`565-567` 读选项，`572-579` 还要求 `preedit.isEmpty`）。`insertText` 的参数是 librime 的提交串，不是语音缓存（`581`）。调用后 `preedit = ""`（`582`）。这次 `insertText` 作用到已有标记文本上的屏幕结果，源码未写出。
3. `get_status`。方案标识变化时重算 `inlinePreedit`、`inlineCandidate`，并 `set_option(session, "soft_cursor", !inlinePreedit)`（`454-467`，赋值在 `461-463`）。语音 `show` 不写这两个布尔值。
4. `get_context` 在传入结构、会话和 `context` 都在时返回真，包括不在组合中（假返回在 `rime_api_impl.h:211-219`，成功落在 `282`；`254` 是隐藏候选菜单时的另一处真返回）。只有 `IsComposing()` 才填写 `composition.preedit`（`220-235`）。`IsComposing` 为 `input_` 或 `composition_` 非空（`context.cc:48-50`）。语音 `show` 不写这两项。Squirrel 把空指针收成 `""`（`471`）。成功分支仍会 `show`（`477-527`）：`inlineCandidate` 用 `commit_text_preview` 拼出的预览（`516-519`）；否则 `inlinePreedit` 用这份 librime 预编辑字符串（`522`）；否则预编辑空则 `""`，非空则全角空格 `"　"`（`523-526`。`524-525` 的注释只解释 iTerm2 与半角占位，不是语音字符串）。`show` 在缓存与新参数不一致时改写 `self.preedit` 并再 `setMarkedText`（`588-605`）。因此活会话上，这一枚键会用 librime 的组合文本（不在组合中就是空串）盖过语音写下的缓存。`setMarkedText` 之后宿主屏幕变成什么，源码未写出。
5. `get_context` 返回假时只 `hidePalettes()`（`557-559`），不调用 `show` 或 `commit`，控制器 `preedit` 保持进入 `rimeUpdate` 之前的值。标记文本是否还在字段上，源码未写出。活会话且 context 存在时，现行 `RimeGetContext` 不走这个假分支。

候选窗另有一份 `preedit`（`SquirrelPanel.swift:22`）。`show(preedit:)` 不写它。同一枚键只有走到 `showPanel`（控制器 `553`、`609`）且 `update` 为真时，才在 `SquirrelPanel.update` 里替换那一份（`153-155`）。

## 2. insertText 与 setMarkedText 是否共用 preedit 缓存

不共用为同一份文本载荷。控制器上只有一个 `preedit: String`（`16`）。全树 `insertText` 一处，`setMarkedText` 两处，都在 `SquirrelInputController.swift`。

- `show`（`586-606`）：先比较 `self.preedit`、`self.caretPos`、`self.selRange`（`588-590`），三者相同则返回，不调用 `setMarkedText`。否则先写入这三项（`592-594`），再 `setMarkedText`（`605`）。这里标记文本的字符串就是刚写入的缓存。
- `commit` 里的 `setMarkedText`（`565-579`）：仅当会话选项 `force_marked_text_for_direct_commit` 为真、`preedit.isEmpty`、且参数 `string` 非空。载荷是参数 `string`（`573`），不是缓存里的旧文本。调用前不写缓存。`569-571` 的注释只说明有的客户在接受 `insertText` 之前需要一次标记文本，没有写已经存在标记文本时的屏幕结果。
- `insertText`（`581`）：参数同样是 `commit(string:)` 的 `string`，不读取 `preedit` 的内容。调用之后把缓存写成 `""`（`582`），并 `hidePalettes()`（`583`）。

两处 `replacementRange` 都是 `.empty`，即 `NSRange(location: NSNotFound, length: 0)`（`BridgingFunctions.swift:59`）。这个范围在已有标记文本时替换哪一段，源码未写出。

`SquirrelPanel.preedit` 不参与 `insertText` 或这两次 `setMarkedText`。

## 3. 走到 commitComposition 时提交的是什么

`SquirrelInputController.swift` 没有「失焦」这两个字。本文件调用 `commitComposition` 的入口只有 `deactivateServer`（`221-225`）：先 `hidePalettes()`，再 `commitComposition(sender)`，再把 `client` 置为 `nil`。

`SquirrelApplicationDelegate.swift:380-391` 写的是输入源被别的进程切走：macOS 26 不调用 `deactivateServer`，通知里用 `finalizeStrandedComposition` 再调一次 `deactivateServer`；菜单栏切换会先调用 `deactivateServer`。这是输入源切换，不是字段失焦。字段失焦时 IMK 调用的是 `commitComposition` 还是 `deactivateServer`，源码未写出。

一旦执行到 `commitComposition`，提交的不是控制器 `preedit`，也不是 `composition.preedit`：

```swift
// 来源: dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:232-240
override func commitComposition(_ sender: Any!) {
  self.client ?= sender as? IMKTextInput
  if session != 0 {
    if let input = rimeAPI.get_input(session) {
      commit(string: String(cString: input))
      rimeAPI.clear_composition(session)
    }
  }
}
```

`get_input` 在会话与 context 都在时返回 `ctx->input().c_str()`（`rime_api_impl.h:1107-1114`）。`input()` 返回 `input_`（`context.h:66`），即原始编码。语音只调用过 `show(preedit:)` 时没有写入 `input_`。`if let` 只判断这个指针是否为 nil。指针为 nil（无会话或无 context，`1109-1113`）时不调用 `commit`，也不 `clear_composition`，控制器 `preedit` 保持不动。

`clear_composition` 调用 `Session::ClearComposition` → `AbortComposition` → `Clear`：清空 `input_`、把 `caret_pos_` 置 0、清空 `composition_`（`service.cc:45-48`，`context.cc:106-116`）。

`commit(string:)` 仍对该原始编码执行 `insertText`，并把控制器 `preedit` 写成 `""`（`581-582`）。这段 `insertText` 作用到调用前已有标记文本上的屏幕结果，源码未写出。

## 检索日志

| # | 动作 | 地址或查询 | 结果 |
| --- | --- | --- | --- |
| 1 | 列出源码 | `dev-docs/refs/code/squirrel/sources/*.swift` | 控制器在 `SquirrelInputController.swift`，642 行 |
| 2 | 检索 IMK 调用 | `setMarkedText`、`insertText`、`commitComposition` | 标记文本两处（`574`、`605`），插入一处（`581`），组合提交在 `232` |
| 3 | 读控制器 | `SquirrelInputController.swift` 全文 | `handle` `32-124`，`processKey` `402-435`，`show` `586-606`，`commit` `562-584`，`deactivateServer` `221-225` |
| 4 | 核对旧笔记行号 | `005` 对 `commit` / `show` 的行号 | 与现行 `562`、`586` 不一致，本文按现行树 |
| 5 | 读 `get_input` | `librime/src/rime_api_impl.h:1107-1114`，`context.h:66` | 返回 `input_` 的 `c_str()`，不是 `composition.preedit` |
| 6 | 读 `process_key` | `rime_api_impl.h:171-178`，`engine.cc:167-195` 与 `231-237` | 走 processor 与 `input_` / `composition()`，不读控制器 `preedit` |
| 7 | 读上下文返回值 | `rime_api_impl.h:209-282`，`context.cc:48-50` | 不在组合中仍在 `282` 返回真；预编辑指针只在 `220-235` 填写 |
| 8 | 读清空组合 | `service.cc:45-48`，`context.cc:106-116` | `clear_composition` 清空 `input_`、光标与 `composition_` |
| 9 | 读提交文本 | `rime_api_impl.h:309-324` | `get_commit` 只在 `commit_text` 非空时返回真 |
| 10 | 读停用入口 | `SquirrelApplicationDelegate.swift:380-391` | 注释写的是输入源切换，不是字段失焦 |
| 11 | 读替换范围 | `BridgingFunctions.swift:52-59` | `?=` 在右侧非 nil 时覆盖；`.empty` 为 `NSNotFound` 长度 0 |
| 12 | 读候选窗缓存 | `SquirrelPanel.swift:22`、`153-155` | 窗上另有 `preedit`，只在 `update == true` 时写入；`show(preedit:)` 不写它 |
| 13 | 核对提交 | 两仓库 `git rev-parse HEAD` | squirrel `b7970a93`，librime `bb568768` |
| 14 | 复核键路径行号 | `handle` 的 `.keyDown`、`processKey`、`rimeConsumeCommittedText` | 普通键入口是 `95-116`，不是早先误记的 `104`；`process_key` 调用在 `412` |

宿主屏幕在「已有标记文本再 `setMarkedText` / `insertText`」以及「返回假的键交给客户应用」这两处没有写出结果。本次没有调用 REA。

<!-- research-complete -->
