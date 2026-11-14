# Squirrel 与 librime 源码分析（含符号/模块/调用图索引）：IMK 事件分发、会话生命周期与非侵入式扩展边界

## 1. 分析范围与源码基线

本文档针对 macOS 平台 Rime 输入法前端 **Squirrel（鼠须管）** 与 **librime** C API 层进行源码级剖析，构建全局模块索引、符号索引与端到端调用图，定位 `InputMethodKit`（IMK）事件分发、会话生命周期、同步 IPC 交互以及 TextKit 2 候选窗渲染机制，并在不修改 `librime` 源码且不改变常规键盘输入行为的前提下，识别语音输入与大模型（LLM）后处理的非侵入式扩展接缝。

- **Squirrel 源码基线**：`dev-docs/refs/code/squirrel`（Git Commit: `b7970a93b05ae1ca1bcd97888fd6aa3cd0458d3e`）
- **librime 源码基线**：`dev-docs/refs/code/librime`（Git Commit: `bb56876890ba60f446fa2640c244b1794b965eda`）
- **关联索引**：全局目标与编号映射参见 `dev-docs/notes/001-research-voice-ime-index.md`。

---

## 2. 全局代码索引（Global Code Index）

### 2.1 模块职责索引表（Module Responsibility Index）

下表列出 `dev-docs/refs/code/squirrel` 全部源码/资源文件与 `dev-docs/refs/code/librime` C API 桥接文件的行数、所属架构层级与模块职责：

| 仓库 | 文件路径 | 行数 | 架构层级 | 模块职责与边界说明 |
|---|---|---:|---|---|
| `squirrel` | `sources/Main.swift` | 178 | 进程入口与守护层 | 定义 `@main struct SquirrelApp`，处理 CLI 启动参数（安装、构建、同步、重载、退出），创建 `IMKServer`，注册跨进程通知监听并运行 `NSApplication` 主事件循环 |
| `squirrel` | `sources/SquirrelApplicationDelegate.swift` | 455 | 应用生命周期与引擎管理层 | 实现 `NSApplicationDelegate`，管理全局单例 `panel` 与 `config`，封装 `setupRime`/`startRime`/`deploy`/`syncUserData`，并通过 C 函数指针接收 `librime` 异步状态通知 |
| `squirrel` | `sources/SquirrelInputController.swift` | 642 | IMK 会话与事件控制器层 | 继承 `IMKInputController`，每个客户端输入上下文对应一个实例；拦截 `NSEvent`（`.keyDown`/`.flagsChanged`）、维护修饰键与并击（Chord）状态机、管理 `RimeSessionId` 生命周期、驱动 `IMKTextInput` 同步 IPC 与 `SquirrelPanel` 更新 |
| `squirrel` | `sources/SquirrelPanel.swift` | 571 | 候选窗控制器与排版组装层 | 继承 `NSPanel`，管理无焦点浮动窗口（`.nonactivatingPanel`）、处理候选窗鼠标点击与滚轮事件、组装预编辑（Preedit）与候选词（Candidates）的 `NSAttributedString`、计算多显示器防溢出屏幕坐标 |
| `squirrel` | `sources/SquirrelView.swift` | 756 | TextKit 2 文本测量与图层渲染层 | 继承 `NSView`，封装 `NSTextView`（TextKit 2 `NSTextLayoutManager` / `NSTextContentStorage` / `NSTextContainer`），基于文本片段几何矩形构造三次贝塞尔平滑圆角路径（`CAShapeLayer`），实现高亮框、背景遮罩与点击命中测试（Hit-Testing） |
| `squirrel` | `sources/SquirrelTheme.swift` | 364 | 主题与视觉样式解析层 | 解析 `squirrel.yaml` 与方案配置中的色彩空间（Display P3 / sRGB）、字体级联列表（Cascade List）、段落样式、边距、圆角半径、阴影及候选词格式化模板 |
| `squirrel` | `sources/SquirrelConfig.swift` | 148 | Rime 配置读取与缓存封装层 | 封装 `RimeConfig` 与 `RimeConfigIterator` C API，提供带内存字典缓存（`cache: [String: Any]`）与 `baseConfig` 链式回退的布尔、浮点、字符串、颜色及 `app_options` 读取接口 |
| `squirrel` | `sources/MacOSKeyCodes.swift` | 232 | 键码与修饰键映射层 | 定义 `SquirrelKeycode`，将 macOS Carbon 虚拟键码（`kVK_*`）、字符与 `NSEvent.ModifierFlags` 转换为 X11/IBus/Rime 键码（`XK_*`）与掩码（`kShiftMask` 等），并提供修饰键键码推断 |
| `squirrel` | `sources/BridgingFunctions.swift` | 83 | Swift-C ABI 桥接工具层 | 定义 `DataSizeable` 协议与 `rimeStructInit()`，自动计算 C 结构体自版本控制字段 `data_size`；提供 `setCString`、条件赋值运算符 `?=` 以及 `NSRange`/`NSPoint` 几何扩展 |
| `squirrel` | `sources/InputSource.swift` | 123 | 系统输入源注册管理层 | 定义 `SquirrelInstaller`，通过 Carbon `TISInputSource` API（`TISRegisterInputSource`、`TISEnableInputSource`、`TISSelectInputSource`）管理简/繁输入模式注册与启用 |
| `squirrel` | `sources/ReservedProperty.swift` | 58 | 插件与前端协同属性解析层 | 定义 `ReservedPropertyKey`（`_comment_highlight`、`_comment_warning`、`_refresh_ui`）与 `ReservedPropertyValue`，解析 URL Query 风格或逗号分隔的候选词索引集合 |
| `squirrel` | `sources/Squirrel-Bridging-Header.h` | 7 | C 头文件桥接声明层 | 向 Swift 暴露 `<rime_api_stdbool.h>` 与 `<rime/key_table.h>` |
| `squirrel` | `resources/Info.plist` | 115 | 束配置与 IMK 契约声明层 | 声明 `InputMethodConnectionName`（`Squirrel_Connection`）、`InputMethodServerControllerClass`（`Squirrel.SquirrelInputController`）、`LSUIElement=true`、`TICapsLockLanguageSwitchCapable=true` |
| `squirrel` | `resources/Squirrel.entitlements` | 12 | 进程签名权限声明层 | 声明 `com.apple.security.app-sandbox=false`、`com.apple.security.cs.disable-library-validation=true`（允许加载第三方 Rime 插件动态库）与 `com.apple.security.network.client=true` |
| `squirrel` | `data/squirrel.yaml` | 482 | 默认前端行为与外观配置层 | 定义 `keyboard_layout`、`chord_duration`、`show_notifications_when`、`status_icon`、默认 `style`、预设配色方案（`preset_color_schemes`）及宿应用初始状态表（`app_options`） |
| `librime` | `src/rime_api.h` | 608 | C API 公共头文件与结构体契约 | 定义 `RimeTraits`、`RimeComposition`、`RimeCandidate`、`RimeMenu`、`RimeCommit`、`RimeContext`、`RimeStatus`、`RimeApi` 函数指针表及 `RIME_STRUCT_INIT` 宏 |
| `librime` | `src/rime_api_stdbool.h` | 22 | `stdbool` 变体宏定义层 | 引入 `<stdbool.h>`，定义 `#define RIME_FLAVORED(name) name##_stdbool` 与 `#define Bool bool`，使 Swift 可直接将 C `bool` 桥接为原生 `Bool` |
| `librime` | `src/rime_api.cc` | 29 | 标准 `int` 布尔 C API 编译单元 | 包含 `rime_api.h` 与 `rime_api_impl.h`，生成 `Bool = int` 版本的 `rime_get_api()` 及全局导出函数 |
| `librime` | `src/rime_api_impl.h` | 1284 | C API 函数体与 `s_api` 组装实现 | 实现会话管理（`RimeCreateSession`）、按键处理（`RimeProcessKey`）、上下文/提交/状态提取（`RimeGetContext`/`RimeGetCommit`/`RimeGetStatus`）、候选操作及 `RIME_FLAVORED(rime_get_api)` 函数表初始化 |

---

### 2.2 全局符号索引表（Global Symbol Index）

下表索引 Squirrel 与 `librime` C API 交互链路上的类、结构体、函数与属性及其精确源码坐标：

| 符号名称 | 符号类别 | 源码位置（`file:line`） | 签名 / 类型摘要 | 上游调用方 | 下游被调方 / 作用 |
|---|---|---|---|---|---|
| `SquirrelApp.main()` | 静态入口方法 | `sources/Main.swift:85` | `static func main() -> Void` | macOS `dyld` / Swift Runtime | 解析命令行参数、构建 `IMKServer`、调用 `setupRime()` 与 `startRime()`、启动 `NSApp.run()` |
| `SquirrelApp.server` | 静态属性 | `sources/Main.swift:127` | `IMKServer?` | `SquirrelApp.main()` | 持有 `IMKServer(name: "Squirrel_Connection", bundleIdentifier: ...)` 实例 |
| `SquirrelApplicationDelegate` | 类（Class） | `sources/SquirrelApplicationDelegate.swift:14` | `final class SquirrelApplicationDelegate: NSObject, NSApplicationDelegate` | `NSApp.squirrelAppDelegate` | 持有 `rimeAPI`、`config`、`panel`、部署与通知状态 |
| `setupRime()` | 实例方法 | `sources/SquirrelApplicationDelegate.swift:149` | `func setupRime() -> Void` | `SquirrelApp.main()` (`Main.swift:135`) | 初始化 `RimeTraits`，调用 `rimeAPI.setup(&traits)` 与 `rimeAPI.set_notification_handler` |
| `startRime(fullCheck:)` | 实例方法 | `sources/SquirrelApplicationDelegate.swift:167` | `func startRime(fullCheck: Bool) -> Void` | `SquirrelApp.main()` / `deploy()` | 调用 `rimeAPI.initialize(nil)` 与 `rimeAPI.start_maintenance(fullCheck)` |
| `loadSettings()` | 实例方法 | `sources/SquirrelApplicationDelegate.swift:213` | `func loadSettings() -> Void` | `SquirrelApp.main()` / `notificationHandler` | 打开 `squirrel.yaml`（`config.openBaseConfig()`）并调用 `panel?.load(config:forDarkMode:)` |
| `loadSchemaSpecificSettings(_:)` | 实例方法 | `sources/SquirrelApplicationDelegate.swift:235` | `func loadSchemaSpecificSettings(_ schemaID: String)` | `notificationHandler` (`.schema`) | 打开 `<schema_id>.schema.yaml`，合并 `baseConfig` 覆写 `SquirrelTheme` |
| `notificationHandler` | 私有静态闭包 | `sources/SquirrelApplicationDelegate.swift:391` | `RimeNotificationHandler` (`@convention(c)`) | `librime` `Service::Notify` | 处理 `"deploy"`、`"schema"`、`"option"` 三类引擎事件通知 |
| `SquirrelInputController` | 类（Class） | `sources/SquirrelInputController.swift:10` | `final class SquirrelInputController: IMKInputController` | `IMKServer` 动态实例化 | 单个客户端输入会话的控制器，管理事件拦截、Rime 会话与 UI 同步 |
| `handle(_:client:)` | 实例方法 | `sources/SquirrelInputController.swift:28` | `override func handle(_ event: NSEvent!, client sender: Any!) -> Bool` | `IMKServer` IPC 分发 | 拦截 `.flagsChanged` 与 `.keyDown`，驱动按键转换、并击定时器与 `processKey` |
| `activateServer(_:)` | 实例方法 | `sources/SquirrelInputController.swift:198` | `override func activateServer(_ sender: Any!)` | `IMKServer` 焦点激活 | 确保 `session` 存活，应用宿应用 `appOptions`，按配置覆写键盘布局 |
| `deactivateServer(_:)` | 实例方法 | `sources/SquirrelInputController.swift:253` | `override func deactivateServer(_ sender: Any!)` | `IMKServer` 焦点切出 | 调用 `hidePalettes()` 与 `commitComposition(sender)` |
| `commitComposition(_:)` | 实例方法 | `sources/SquirrelInputController.swift:179` | `override func commitComposition(_ sender: Any!)` | `IMKServer` / `deactivateServer` | 读取 `rimeAPI.get_input(session)` 调 `commit(string:)` 并 `clearComposition` |
| `recognizedEvents(_:)` | 实例方法 | `sources/SquirrelInputController.swift:247` | `override func recognizedEvents(_ sender: Any!) -> Int` | `IMKServer` 查询掩码 | 返回 `NSEvent.EventTypeMask(arrayLiteral: .keyDown, .flagsChanged).rawValue` |
| `createSession()` | 实例方法 | `sources/SquirrelInputController.swift:227` | `func createSession() -> Void` | `activateServer` / `handle` | 调用 `rimeAPI.create_session()`、设置 `_app_name` 属性并加载 `appOptions` |
| `destroySession()` | 实例方法 | `sources/SquirrelInputController.swift:258` | `func destroySession() -> Void` | `deinit` (`222`) | 调用 `rimeAPI.destroy_session(session)` 并调用 `clearChord()` |
| `processKey(_:modifiers:)` | 实例方法 | `sources/SquirrelInputController.swift:343` | `func processKey(_ rimeKeycode: UInt32, modifiers rimeModifiers: UInt32) -> Bool` | `handle` / `updateChord` / `clearChord` | 调用 `rimeAPI.process_key`、`rimeUpdate()`、补发 `kReleaseMask` 及处理 `Control+x` 粘滞键 |
| `rimeUpdate()` | 实例方法 | `sources/SquirrelInputController.swift:605` | `func rimeUpdate() -> Void` | `processKey` / `selectCandidate` / `page` / `moveCaret` | 依次调用 `rimeConsumeCommittedText()` 与 `updateUI()` |
| `rimeConsumeCommittedText()` | 实例方法 | `sources/SquirrelInputController.swift:578` | `func rimeConsumeCommittedText() -> Bool` | `rimeUpdate()` | 调用 `rimeAPI.get_commit(session, &commitText)`，触发 `commit(string:)` 并释放内存 |
| `updateUI()` | 实例方法 | `sources/SquirrelInputController.swift:438` | `func updateUI() -> Void` | `rimeUpdate()` / `activateServer` | 读取 `RimeStatus_stdbool` 与 `RimeContext_stdbool`，同步 `show(preedit:...)` 与 `panel.showPanel(...)` |
| `commit(string:)` | 私有实例方法 | `sources/SquirrelInputController.swift:386` | `func commit(string: String) -> Void` | `rimeConsumeCommittedText` / `commitComposition` | 调用 `client.insertText(string, replacementRange: .empty)` 并置 `inlinePreedit = false` |
| `show(preedit:selRange:caretPos:)` | 私有实例方法 | `sources/SquirrelInputController.swift:393` | `func show(preedit: String, selRange: NSRange, caretPos: Int)` | `updateUI()` | 调用 `client.setMarkedText` 更新行内预编辑，并通过 `client.firstRect` 测量光标坐标赋给 `panel.position` |
| `applyReservedProperties(in:)` | 私有实例方法 | `sources/SquirrelInputController.swift:591` | `func applyReservedProperties(in session: RimeSessionId)` | `updateUI()` (`467`) | 读取 `_comment_highlight`、`_comment_warning`、`_refresh_ui` 属性，驱动面板高亮与刷新 |
| `SquirrelPanel` | 类（Class） | `sources/SquirrelPanel.swift:11` | `final class SquirrelPanel: NSPanel` | `SquirrelApplicationDelegate` / `SquirrelInputController` | 管理候选窗视图层级、屏幕定位、文本属性组装与鼠标事件转发 |
| `SquirrelPanel.sendEvent(_:)` | 实例方法 | `sources/SquirrelPanel.swift:100` | `override func sendEvent(_ event: NSEvent)` | AppKit 窗口事件分发 | 处理 `.leftMouseDown`/`.leftMouseUp`/`.rightMouseUp`/`.mouseMoved`/`.scrollWheel`，反向调用 `inputController` |
| `SquirrelPanel.show()` | 实例方法 | `sources/SquirrelPanel.swift:218` | `func show() -> Void` | `showPanel(...)` / `showStatus(_:)` | 测量文本宽度、折算窗口尺寸与屏幕边界翻转、更新 `alphaValue` 与 `orderFront(nil)` |
| `SquirrelPanel.update(...)` | 实例方法 | `sources/SquirrelPanel.swift:346` | `func update(preedit:selRange:caretPos:candidates:comments:labels:highlighted:update:)` | `SquirrelInputController.updateUI()` | 将 `preedit`、`labels`、`candidates`、`comments` 拼接为带 `.noBreak` 属性的 `NSMutableAttributedString` 并交给 `SquirrelView` |
| `SquirrelView` | 类（Class） | `sources/SquirrelView.swift:25` | `final class SquirrelView: NSView` | `SquirrelPanel` | 持有 `NSTextView` 与 `CAShapeLayer`，执行 TextKit 2 排版测量、圆角几何绘制与点击命中 |
| `SquirrelView.draw(_:)` | 实例方法 | `sources/SquirrelView.swift:130` | `override func draw(_ dirtyRect: NSRect)` | AppKit 渲染管线 | 计算背景、预编辑、候选词与翻页箭头 `CGPath`，重建 `CAShapeLayer` 子图层树与遮罩 |
| `SquirrelView.click(at:)` | 实例方法 | `sources/SquirrelView.swift:303` | `func click(at clickPoint: NSPoint) -> (Int?, Int?, Bool?)` | `SquirrelPanel.sendEvent(_:)` | 命中测试翻页箭头路径或通过 `textLayoutManager.textLayoutFragment(for:)` 反查候选词/预编辑字符下标 |
| `DataSizeable.rimeStructInit()` | 协议扩展方法 | `sources/BridgingFunctions.swift:22` | `static func rimeStructInit() -> Self` | Squirrel 全模块 | 分配清零内存并将 `data_size` 设为 `MemoryLayout<Self>.size - sizeof(data_size)` |
| `rime_get_api_stdbool` | C 导出函数 | `src/rime_api_impl.h:1203` | `RimeApi_stdbool* rime_get_api_stdbool(void)` | 各模块 `rimeAPI` 属性初始化 | 初始化并返回静态 `RimeApi_stdbool` 函数指针表 `s_api` |
| `RimeProcessKey` | C API 实现函数 | `src/rime_api_impl.h:171` | `Bool RimeProcessKey(RimeSessionId, int, int)` | `SquirrelInputController.processKey` | 查找 `Session` 并调用 `session->ProcessKey(KeyEvent(keycode, mask))` |
| `RimeGetContext` | C API 实现函数 | `src/rime_api_impl.h:209` | `Bool RimeGetContext(RimeSessionId, RimeContext_stdbool*)` | `SquirrelInputController.updateUI` | 提取 `Preedit`、`commit_text_preview`、当前页 `RimeCandidate` 数组与 `select_labels` |
| `RimeGetCommit` | C API 实现函数 | `src/rime_api_impl.h:309` | `Bool RimeGetCommit(RimeSessionId, RimeCommit*)` | `rimeConsumeCommittedText` | 拷贝 `session->commit_text()` 并调用 `session->ResetCommitText()` |
| `RimeGetStatus` | C API 实现函数 | `src/rime_api_impl.h:335` | `Bool RimeGetStatus(RimeSessionId, RimeStatus_stdbool*)` | `SquirrelInputController.updateUI` | 提取 `schema_id`、`schema_name`、`is_composing`、`is_ascii_mode` 等布尔状态位 |

---

### 2.3 端到端调用图（End-to-End Call Graphs）

#### （1）进程启动、`IMKServer` 注册与 `librime` 初始化调用图

```text
// 源：dev-docs/refs/code/squirrel/sources/Main.swift:85-169 & SquirrelApplicationDelegate.swift:149-177
SquirrelApp.main() [Main.swift:85]
 ├─ autoreleasepool { parse CLI args (--quit / --reload / --install / --build / --sync) } [Main.swift:86-125]
 ├─ SquirrelApp.server = IMKServer(name: "Squirrel_Connection", bundleIdentifier: Bundle.main.bundleIdentifier) [Main.swift:127-130]
 ├─ NSApp.squirrelAppDelegate.setupRime() [Main.swift:135 -> SquirrelApplicationDelegate.swift:149]
 │   ├─ rimeAPI.setup(&traits) [SquirrelApplicationDelegate.swift:163 -> rime_api_impl.h:25]
 │   └─ rimeAPI.set_notification_handler(notificationHandler, Unmanaged.passUnretained(self).toOpaque()) [SquirrelApplicationDelegate.swift:164]
 ├─ DispatchQueue.main.async { startRime(fullCheck: false); loadSettings() } [Main.swift:136-138]
 │   ├─ rimeAPI.initialize(nil) [SquirrelApplicationDelegate.swift:172 -> rime_api_impl.h:51]
 │   ├─ rimeAPI.start_maintenance(false) [SquirrelApplicationDelegate.swift:174 -> rime_api_impl.h:65]
 │   └─ config.openBaseConfig() -> panel.load(config:forDarkMode:) [SquirrelApplicationDelegate.swift:214-224]
 └─ NSApp.run() [Main.swift:167]
```

**工程分析**：Squirrel 在 `Main.swift:127-130` 构造 `IMKServer` 时，通过 `Info.plist:92-97` 中声明的 `InputMethodConnectionName`（`Squirrel_Connection`）向系统的输入法守护服务注册 Mach/XPC 服务端点，并将控制器类映射至 `Squirrel.SquirrelInputController`。随后同步执行 `setupRime()` 注册全局日志与通知回调，并将耗时的 `startRime(fullCheck: false)` 与 `loadSettings()` 投递到主队列首个 RunLoop 周期执行，避免阻塞 `IMKServer` 握手。

#### （2）按键拦截、`RimeProcessKey`、`IMKTextInput` 上屏与 `SquirrelPanel` 渲染调用图

```text
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:28-140, 343-496, 578-634
IMKServer IPC -> SquirrelInputController.handle(_ event: NSEvent!, client sender: Any!) [SquirrelInputController.swift:28]
 ├─ update client = sender as? IMKTextInput; ensure session via createSession() [30, 38-40]
 ├─ case .flagsChanged: diff lastModifiers ^ modifiers -> processKey(rimeKeycode, modifiers: ...) [41-91]
 └─ case .keyDown: filter Command/Option -> osxKeycodeToRime(...) -> processKey(rimeKeycode, modifiers: rimeModifiers) [92-135]
     ├─ rimeAPI.process_key(session, Int32(rimeKeycode), Int32(rimeModifiers)) [355 -> rime_api_impl.h:171]
     └─ rimeUpdate() [356 -> 605]
         ├─ rimeConsumeCommittedText() [606 -> 578]
         │   └─ rimeAPI.get_commit(session, &commitText) -> commit(string:) -> client.insertText(string, replacementRange: .empty) [580-585, 387]
         └─ updateUI() [613 -> 438]
             ├─ rimeAPI.get_status(session, &status) & rimeAPI.get_context(session, &ctx) [441-462 -> rime_api_impl.h:335, 209]
             ├─ applyReservedProperties(in: session) [467 -> 591]
             ├─ show(preedit:selRange:caretPos:) -> client.setMarkedText(...) & client.firstRect(...) [476 -> 393-428]
             └─ panel.showPanel(...) -> panel.update(...) -> view.drawView(...) -> panel.show() [489-495 -> SquirrelPanel.swift:310-343]
```

**工程分析**：每次键盘事件均在主线程同步完成「`NSEvent` 解码 → `rimeAPI.process_key` 状态机推演 → `rimeConsumeCommittedText` 提取上屏文本并调 `client.insertText` → `updateUI` 提取 `RimeContext_stdbool` 并调 `client.setMarkedText` 与 ` client.firstRect` → `SquirrelPanel` 排版与重绘」，并向 `IMKServer` 返回 `handled: Bool`。若返回 `true`，宿主应用消费该按键；若返回 `false`，系统将该 `NSEvent` 原样发往宿主应用当前第一响应者视图。

#### （3）候选窗鼠标点击与滚轮交互反向调用图

```text
// 源：dev-docs/refs/code/squirrel/sources/SquirrelPanel.swift:100-175 & SquirrelView.swift:303-341
AppKit WindowServer -> SquirrelPanel.sendEvent(_ event: NSEvent) [SquirrelPanel.swift:100]
 ├─ view.click(at: clickPoint) -> returns (candidateIndex: Int?, preeditIndex: Int?, pageUp: Bool?) [SquirrelPanel.swift:104 -> SquirrelView.swift:303]
 ├─ case .leftMouseUp:
 │   ├─ if pageUp != nil -> inputController.page(up: pageUp) -> rimeAPI.change_page(...) -> rimeUpdate() [SquirrelPanel.swift:120 -> SquirrelInputController.swift:324]
 │   ├─ if preeditIndex >= caretPos -> inputController.moveCaret(backward: true/false) -> rimeAPI.set_caret_pos(...) [SquirrelPanel.swift:124-131 -> SquirrelInputController.swift:335]
 │   └─ if candidateIndex != nil -> inputController.selectCandidate(index) -> rimeAPI.select_candidate_on_current_page(...) [SquirrelPanel.swift:133 -> SquirrelInputController.swift:316]
 └─ case .scrollWheel: accumulate scrollDirection -> inputController.page(up:) [SquirrelPanel.swift:153-172]
```

**工程分析**：由于 `SquirrelPanel` 采用 `.nonactivatingPanel` 样式掩码（`SquirrelPanel.swift:48`），点击候选窗不会抢夺宿主应用的键盘焦点。`SquirrelPanel` 重写 `sendEvent(_:)` 直接拦截鼠标事件，通过 `SquirrelView.click(at:)` 利用 TextKit 2 的 `NSTextLayoutFragment` 将窗口坐标还原为字符偏移量，再回调当前绑定的 `inputController` 执行选词、翻页或移动光标。

---

## 3. 逐模块深度源码分析（Module-by-Module Deep Analysis）

### 3.1 进程配置、Swift-C ABI 桥接与 `SquirrelApplicationDelegate`

#### 3.1.1 进程权限配置与后台 UI 元素模型（`Info.plist` & `Squirrel.entitlements`）

```xml
<!-- 源：dev-docs/refs/code/squirrel/resources/Info.plist:92-103 -->
	<key>InputMethodConnectionName</key>
	<string>Squirrel_Connection</string>
	<key>InputMethodServerControllerClass</key>
	<string>Squirrel.SquirrelInputController</string>
	<key>InputMethodServerDelegateClass</key>
	<string>Squirrel.SquirrelInputController</string>
	<key>LSBackgroundOnly</key>
	<false/>
	<key>LSUIElement</key>
	<true/>
```

**工程分析**：`Info.plist:98-101` 将 `LSBackgroundOnly` 设为 `false`、将 `LSUIElement` 设为 `true`。这意味着 Squirrel 属于不显示在 Dock 与 Command+Tab 应用切换器中的代理应用（Agent App），但拥有完整的 WindowServer 图层连接能力、菜单栏状态图标（`NSStatusBar`）及非激活浮动面板（`NSPanel`）渲染权限。同时，`resources/Squirrel.entitlements:5-10` 关闭了 App Sandbox（`com.apple.security.app-sandbox = false`），开启了 `com.apple.security.cs.disable-library-validation = true` 与 `com.apple.security.network.client = true`，使其既能直接读写 `~/Library/Rime` 用户目录与加载外部编译的 Rime 插件，也能直接发起网络请求。

#### 3.1.2 `rime_api_stdbool.h` 宏风味化与 `DataSizeable` 结构体自版本控制

```c
// 源：dev-docs/refs/code/librime/src/rime_api_stdbool.h:8-20
#include <stdbool.h>

#define RIME_FLAVORED(name) name##_stdbool
#define RIME_DEPRECATED static
#define Bool bool
#define False false
#define True true

#include "rime_api.h"
```

**工程分析**：在标准 C 语言接口 `rime_api.h:45-47` 中，`Bool` 被宏定义为 `int`，导致 Swift 导入时所有布尔返回值和结构体字段均变为 `Int32`。`rime_api_stdbool.h` 通过重定义 `RIME_FLAVORED(name)` 为 `name##_stdbool` 并将 `Bool` 定义为 C99 `<stdbool.h>` 的 `bool`，生成了 `RimeApi_stdbool`、`RimeContext_stdbool`、`RimeStatus_stdbool`、`RimeMenu_stdbool` 以及入口函数 `rime_get_api_stdbool()`。Swift 通过 `Squirrel-Bridging-Header.h:6` 导入该头文件后，所有 C API 布尔签名自动映射为 Swift 原生 `Bool`。

```swift
// 源：dev-docs/refs/code/squirrel/sources/BridgingFunctions.swift:21-30
extension DataSizeable {
  static func rimeStructInit() -> Self {
    let valuePointer = UnsafeMutablePointer<Self>.allocate(capacity: 1)
    memset(valuePointer, 0, MemoryLayout<Self>.size)
    var value = valuePointer.move()
    valuePointer.deallocate()
    let offset = MemoryLayout.size(ofValue: \Self.data_size)
    value.data_size = Int32(MemoryLayout<Self>.size - offset)
    return value
  }
```

**工程分析**：`librime` 的 C 结构体（`RimeTraits`、`RimeCommit`、`RimeContext_stdbool`、`RimeStatus_stdbool`）首字段均为 `int data_size`，在 C 端通过宏 `RIME_STRUCT_HAS_MEMBER`（`rime_api.h:62-64`）校验调用方编译时的结构体字节大小以实现二进制向后兼容。由于 C 宏 `RIME_STRUCT_INIT` 含有 `sizeof(Type)`，无法被 Swift Clang Importer 直接翻译为函数，`BridgingFunctions.swift:21-30` 通过 `DataSizeable.rimeStructInit()` 在堆上分配清零内存并计算 `MemoryLayout<Self>.size - MemoryLayout.size(ofValue: \Self.data_size)`，完成了与 C 宏完全等价的 ABI 初始化。

#### 3.1.3 `SquirrelApplicationDelegate` 引擎初始化与跨线程通知回调

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelApplicationDelegate.swift:152-165
    var traits = RimeTraits.rimeStructInit()
    traits.setCString(Bundle.main.sharedSupportPath!, to: \.shared_data_dir)
    traits.setCString(SquirrelApp.userDir.path(), to: \.user_data_dir)
    traits.setCString("Squirrel", to: \.distribution_code_name)
    traits.setCString("鼠鬚管", to: \.distribution_name)
    traits.setCString(Bundle.main.object(forInfoDictionaryKey: kCFBundleVersionKey as String) as! String,
                      to: \.distribution_version)
    traits.setCString("rime.squirrel", to: \.app_name)
    traits.setCString(SquirrelApp.logDir.path(), to: \.log_dir)
    rimeAPI.setup(&traits)
    rimeAPI.set_notification_handler(notificationHandler, Unmanaged.passUnretained(self).toOpaque())
  }
```

**工程分析**：`setupRime()` 通过 `traits.setCString`（内部调用 `strdup` 分配持久 C 字符串，见 `BridgingFunctions.swift:32-41`）设置共享目录 `/Library/Input Methods/Squirrel.app/Contents/SharedSupport`、用户目录 `~/Library/Rime` 与日志目录 `$TMPDIR/rime.squirrel`，随后调用 `rimeAPI.setup(&traits)` 并把 `self` 的非托管裸指针（`Unmanaged.passUnretained(self).toOpaque()`）作为 `context_object` 注册给 `rimeAPI.set_notification_handler`。

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelApplicationDelegate.swift:391-405
  private static let notificationHandler: RimeNotificationHandler = { contextObject, sessionId, messageTypeC, messageValueC in
    let delegate: SquirrelApplicationDelegate = Unmanaged<SquirrelApplicationDelegate>.fromOpaque(contextObject!).takeUnretainedValue()
    let messageType = messageTypeC.map { String(cString: $0) }
    let messageValue = messageValueC.map { String(cString: $0) }
    if messageType == "deploy" {
      switch messageValue {
      case "start":
        Self.showStatusMessage(msgTextLong: NSLocalizedString("deploy_start", comment: ""), msgTextShort: "⏳")
      case "success":
        Self.showStatusMessage(msgTextLong: NSLocalizedString("deploy_success", comment: ""), msgTextShort: "✅")
        DispatchQueue.main.async {
          delegate.loadSettings()
        }
```

**工程分析**：`notificationHandler` 是一个符合 C 调用约定的闭包，会被 `librime` 在三类场景下触发：
1. `messageType == "deploy"`：由后台维护线程（Maintenance Thread）触发，当 `messageValue == "success"` 时，通过 `DispatchQueue.main.async` 切回主线程调用 `delegate.loadSettings()` 重新加载 `squirrel.yaml` 与主题，避免后台线程直接竞争 `SquirrelConfig` 或 AppKit 视图。
2. `messageType == "schema"`：当用户在输入过程中切换输入方案时由主线程同步触发（`SquirrelApplicationDelegate.swift:412-418`），截取 `/` 后的方案显示名调用 `showStatusMessage` 弹出状态气泡，并调用 `delegate.loadSchemaSpecificSettings(schemaId)` 加载方案专属外观。
3. `messageType == "option"`：当切换中英文（`ascii_mode`）、简繁（`simplification`）等开关时触发（`419-427`），调用 `rimeAPI.get_state_label_abbreviated` 查询短标签与长标签，更新候选窗状态气泡及系统菜单栏状态图标（`SquirrelApp.updateStatusBarLabel()`）。

---

### 3.2 `SquirrelInputController`：事件拦截、修饰键状态机与会话生命周期

#### 3.2.1 事件掩码注册与 `handle(_:client:)` 入口守卫

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:28-40
  override func handle(_ event: NSEvent!, client sender: Any!) -> Bool {
    guard let event = event else { return false }
    let modifiers = event.modifierFlags
    let changes = lastModifiers.symmetricDifference(modifiers)
    var handled = false
    if session == 0 || !rimeAPI.find_session(session) {
      createSession()
      if session == 0 {
        return false
      }
    }
    self.client ?= sender as? IMKTextInput
```

**工程分析**：`recognizedEvents(_:)`（`SquirrelInputController.swift:247-251`）向 `IMKServer` 声明仅拦截 `[.keyDown, .flagsChanged]` 两类事件（不拦截鼠标事件，因为候选窗鼠标事件由 `SquirrelPanel.sendEvent` 自行处理）。进入 `handle(_:client:)` 后，控制器首先执行两项防御检查：
1. 检查 `session == 0 || !rimeAPI.find_session(session)`：若此前执行过 `deploy()`（会调用 `rimeAPI.finalize()` 销毁所有会话），则立即调用 `createSession()` 重建会话。
2. 使用条件赋值运算符 `self.client ?= sender as? IMKTextInput` 更新当前宿主客户端代理引用。

#### 3.2.2 修饰键状态机（`.flagsChanged`）与 Caps Lock 特殊处理

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:42-55
    case .flagsChanged:
      if lastModifiers == modifiers {
        handled = true
        break
      }
      var rimeModifiers: UInt32 = SquirrelKeycode.osxModifiersToRime(modifiers: modifiers)
      var releaseMask: UInt32 = 0
      if !changes.isEmpty {
        if changes.contains(.capsLock) {
          // Send both press and release when capsLock is toggled
        } else if !lastModifiers.intersection(changes).isEmpty {
          releaseMask = kReleaseMask.rawValue
        }
```

**工程分析**：macOS AppKit 不会为 Shift/Control/Option/Command/CapsLock 分发独立的 `.keyDown` 与 `.keyUp` 事件，而是统一分发 `.flagsChanged` 事件。`SquirrelInputController` 在 `41-91` 行实现了一个基于位对称差集（`let changes = lastModifiers.symmetricDifference(modifiers)`）的修饰键状态机：
1. **按下与释放方向判定**：若 `lastModifiers.intersection(changes)` 非空，说明发生变化的修饰位原本处于按下状态、现在消失了，故置 `releaseMask = kReleaseMask.rawValue`（`0x40000000`）；反之则为按下事件。
2. **合成事件键码回退推断**：部分虚拟键盘或辅助功能（`CGEvent`）注入的 `.flagsChanged` 事件不携带有效的修饰键 `keyCode`（不在 `SquirrelKeycode.modifierKeycodes` 集合中，`MacOSKeyCodes.swift:68-75`）。此时通过 `SquirrelKeycode.inferModifierKeycode(from: changes)`（`MacOSKeyCodes.swift:77-90`）按 `capsLock -> shift -> control -> option -> command` 顺序反推虚拟键码。
3. **Caps Lock 双脉冲合成**（`SquirrelInputController.swift:67-76`）：由于 `Info.plist:110-111` 开启了 `TICapsLockLanguageSwitchCapable = true`，且 macOS 的 Caps Lock 状态位是翻转锁存（Toggle）而非瞬态按压，当检测到 `keycode == kVK_CapsLock` 时，Squirrel 连续调用两次 `processKey`——第一次不带 `kReleaseMask`，第二次附带 `kReleaseMask.rawValue`（并修正 `rimeModifiers ^= kLockMask.rawValue`），从而确保 `librime` 的 `ascii_composer` 能完整走完 Caps Lock 按下与释放的切换逻辑。
4. **并击（Chord）提交触发**（`78-84`）：当任意非 CapsLock 修饰键释放时，立即调用 `clearChord()` 将并击缓冲区中的按键序列刷入 `librime`。

#### 3.2.3 `.keyDown` 快捷键放行、并击（Chording）与粘滞键（Sticky Key）机制

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:92-105
    case .keyDown:
      // ignore Command+X hotkeys.
      if modifiers.contains(.command) || modifiers.contains(.option) {
        break
      }
      let keyCode = event.keyCode
      var keyChars = event.charactersIgnoringModifiers
      if let firstChar = keyChars?.first {
        if !firstChar.isASCII {
          keyChars = event.characters
          if let firstAsciiChar = keyChars?.first, !firstAsciiChar.isASCII {
            break
          }
```

**工程分析**：在 `.keyDown` 分支中：
1. **系统快捷键放行**：若修饰键包含 `.command` 或 `.option`（`94-96`），直接 `break` 并返回 `handled = false`，将快捷键交还给宿主应用。
2. **字符清洗与键码转换**：提取 `event.charactersIgnoringModifiers`，过滤掉 `Control+A` 产生的不可见 ASCII 控制字符（`\u{1}`），然后调用 `SquirrelKeycode.osxKeycodeToRime(keycode:keychar:shift:caps:)`（`MacOSKeyCodes.swift:33-66`）转换为 X11 `XK_*` 键码。

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:346-357
    // Sticky key: Left Control + x acts as prefix
    var actualModifiers = rimeModifiers
    if chordPrefixControlX && rimeModifiers == 0 {
      actualModifiers |= kControlMask.rawValue
      chordPrefixControlX = false
    }
    // with linear candidate list, arrow keys may behave differently.
    if let panel = NSApp.squirrelAppDelegate.panel {
      if panel.linear != rimeAPI.get_option(session, "_linear") {
        rimeAPI.set_option(session, "_linear", panel.linear)
      }
```

**工程分析**：`processKey(_:modifiers:)`（`343-376`）在调用 `rimeAPI.process_key` 前后执行了三项前端适配：
1. **面板布局状态同步**（`353-354`）：检查 `panel.linear` 与 `panel.vertical` 是否与当前 Rime 会话的 `_linear` / `_vertical` 选项一致，若不一致则调用 `rimeAPI.set_option` 同步，使 `librime` 的 `navigator` / `selector` 知道上下左右方向键应当移动光标还是切换候选词。
2. **按键释放事件补发**（`356-360`）：由于 `recognizedEvents` 未订阅 `.keyUp`（在 IMK 中订阅 `.keyUp` 会增加一倍的跨进程 IPC 开销），若 `rimeKeycode != XK_VoidSymbol` 且当前是按下事件（`actualModifiers & kReleaseMask == 0`），Squirrel 在 `rimeUpdate()` 后紧接着向 `librime` 发送一次带 `kReleaseMask` 的 `rimeAPI.process_key` 调用，防止并击方案（如宫保拼音）因缺少 KeyRelease 事件而卡在缓冲态。
3. **`Control+x` 粘滞键状态机**（`348-351, 367-374`）：支持 Emacs 风格 `C-x` 前缀组合键（如 `C-x C-g`、`C-x q`、`C-x h`、`C-x u`），在收到 `Left Control + x` 时置 `chordPrefixControlX = true` 并吞掉该事件，下一枚无修饰按键到来时自动补上 `kControlMask`。

#### 3.2.4 会话生命周期：`activateServer`、`deactivateServer`、`commitComposition` 与 `hidePalettes`

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:179-196
  override func commitComposition(_ sender: Any!) {
    self.client ?= sender as? IMKTextInput
    if session != 0 {
      if let rawInput = rimeAPI.get_input(session) {
        commit(string: String(cString: rawInput))
        rimeAPI.clear_composition(session)
      }
    }
  }

  override func hidePalettes() {
    NSApp.squirrelAppDelegate.panel?.hide()
  }
```

**工程分析**：结合 `SquirrelInputController.swift:179-264` 的源码，Squirrel 的会话生命周期具有明确的职责分离：
1. **`activateServer(_:)`（`198-219`）**：当宿主应用获得输入焦点或用户切换到鼠须管时调用。
   - 若 `session == 0` 或会话已失效，调用 `createSession()`（`227-245`）创建 `RimeSessionId`，通过 `rimeAPI.set_property(session, "_app_name", app)` 注入宿主 Bundle ID，并加载 `squirrel.yaml` 中针对该 Bundle ID 的 `app_options`（如在特定 IDE 中自动开启 `ascii_mode`）。
   - 若 `session` 已存在且宿主 Bundle ID 发生变化（`200-204`），重新应用新应用的 `appOptions`。
   - 根据 `keyboard_layout` 配置（`"last"` / `"default"` / 自定义布局名），调用 `client?.overrideKeyboard(withKeyboardNamed:)` 覆写物理键盘布局映射。
   - 清空 `preedit = ""` 以强制在随后的状态同步中刷新 UI。
2. **`commitComposition(_:)`（`179-192`）**：当用户在宿主应用中点击鼠标切换输入位置、或输入会话中断时由 IMK 调用。注意：此处并非调用 `rimeAPI.commit_composition(session)` 提交首选候选词，而是调用 `rimeAPI.get_input(session)` 取出用户已敲击的**原始编码串（`rawInput`）**，通过 `commit(string:)` 直接上屏并调用 `rimeAPI.clear_composition(session)` 清空编码。
3. **`hidePalettes()`（`194-196`）与 `deactivateServer(_:)`（`253-256`）**：当焦点切出当前应用或切换输入法时，`deactivateServer` 依次调用 `hidePalettes()`（隐藏 `SquirrelPanel`）与 `commitComposition(sender)`（将残留原始编码上屏）。**注意**：`deactivateServer` 不会调用 `destroySession()`，因此每个 `SquirrelInputController` 实例的 `RimeSessionId`（连同其用户手动切换的 `ascii_mode` 等会话态选项）在焦点来回切换期间持续保留，直至宿主销毁输入上下文触发 `SquirrelInputController.deinit`（`221-225`）时才调用 `destroySession()`（`258-264`）。

---

### 3.3 `IMKTextInput` 同步 IPC 交互细节：调用时机、顺序约束与阻塞风险

#### 3.3.1 `commit(string:)` 与 `clearMarkedText()` 的行内预编辑清理协议

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:386-391, 430-436
  func commit(string: String) {
    guard let client = client else { return }
    client.insertText(string, replacementRange: .empty)
    preedit = ""
    inlinePreedit = false
  }
```

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:605-614
  func rimeUpdate() {
    let committed = rimeConsumeCommittedText()
    if committed && preedit.isEmpty && inlinePreedit {
      // If commit happens without an active preedit, explicitly clear the
      // client's marked text so residual inline composition doesn't linger.
      clearMarkedText()
    }
    updateUI()
  }
```

**工程分析**：`IMKTextInput` 协议运行在跨进程 Mach/XPC 同步 IPC 之上。`commit(string:)` 调用 `client.insertText(string, replacementRange: .empty)`（其中 `.empty` 为 `NSRange(location: NSNotFound, length: 0)`，见 `BridgingFunctions.swift:58-60`）。在 macOS Cocoa 文本系统（`NSTextInputClient`）中，当宿主视图存在行内预编辑文本（Marked Text）时，调用 `insertText(_:replacementRange: NSNotFound)` 会自动替换并清除宿主的 Marked Text，因此 `commit(string:)` 无需再发一次 `setMarkedText("", ...)`，而是直接将本地标记 `inlinePreedit` 置为 `false` 以节省一次跨进程 IPC 调用。
但存在一个边界情况（见 `607-611`）：若某次按键触发了直接上屏（`committed == true`），但此时 Rime 侧的 `preedit` 已经为空且此前的 `inlinePreedit` 仍为 `true`（或随后的 `updateUI()` 中预编辑为空），部分宿主应用可能残留空的 Marked Text 状态。为此 `rimeUpdate()` 显式调用 `clearMarkedText()`（向 `client.setMarkedText("", selectionRange: NSMakeRange(0, 0), replacementRange: .empty)` 发送同步清空指令）。

#### 3.3.2 `show(preedit:selRange:caretPos:)` 中 `setMarkedText` 与 `firstRect` 的严格时序

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:411-424
      client.setMarkedText(attrString, selectionRange: NSRange(location: caretPos, length: 0), replacementRange: .empty)
    }
    var caretRect = NSRect.zero
    client.firstRect(forCharacterRange: NSRange(location: caretPos, length: 0), actualRange: nil)
    caretRect = client.firstRect(forCharacterRange: NSRange(location: caretPos, length: 0), actualRange: nil)
    if caretRect == .zero {
      // Fall back to the selected range when the client has not updated layout yet.
      caretRect = client.firstRect(forCharacterRange: client.selectedRange(), actualRange: nil)
    }
```

**工程分析**：`SquirrelInputController.swift:393-428` 展示了 `IMKTextInput` 交互中对时序最敏感的片段：
1. **UTF-8 字节偏移到 UTF-16 码元偏移的转换**（`updateUI()` 第 `472-476` 行）：`librime` 的 `RimeComposition`（`cursor_pos`、`sel_start`、`sel_end`）均以 UTF-8 字节下标表示，而 Cocoa `NSRange` 与 `IMKTextInput` 以 UTF-16 码元（`utf16.count`）表示。`updateUI()` 先通过 `preedit.utf8.index(preedit.utf8.startIndex, offsetBy: ...)` 转换为 `String.Index`，再通过 `NSRange(..., in: preedit)` 折算为 UTF-16 坐标传入 `show(preedit:selRange:caretPos:)`。
2. **先 `setMarkedText` 后 `firstRect` 的顺序约束**：在 `inlinePreedit` 模式下，Squirrel 先构造分段下划线属性字符串（已选分词段使用单下划线 `.single`，未决编码段使用粗下划线 `.thick`），调用 `client.setMarkedText(attrString, selectionRange: NSRange(location: caretPos, length: 0), replacementRange: .empty)`。只有当宿主应用收到 `setMarkedText` 将预编辑文本插入文档流后，紧随其后的 `client.firstRect(forCharacterRange: NSRange(location: caretPos, length: 0), actualRange: nil)` 才能查询到最新的行内光标屏幕矩形。
3. **双重 `firstRect` 调用与 `.zero` 回退（`414-423`）**：源码第 `414` 行与第 `415` 行连续调用了两次 `client.firstRect(forCharacterRange: NSRange(location: caretPos, length: 0), actualRange: nil)`——第一次调用促使部分延迟排版的宿主应用（如基于 WebView/Electron 或自定义文本引擎的应用）刷新内部光标缓存，第二次读取返回值；若仍返回 `NSRect.zero`（例如宿主尚未完成异步排版），则回退调用 `client.selectedRange()` 获取当前选区起点再查一次 `firstRect`，避免候选窗跳回屏幕左下角 `(0, 0)`。

#### 3.3.3 `IMKTextInput` 全量调用清单与同步 IPC 阻塞风险矩阵

下表汇总 Squirrel 中所有 `IMKTextInput` 跨进程调用的位置、触发频率与阻塞风险：

| `IMKTextInput` 方法 | Squirrel 调用位置（`file:line`） | 触发时机与频率 | 同步 IPC 机制与主线程阻塞风险分析 |
|---|---|---|---|
| `client.bundleIdentifier()` | `SquirrelInputController.swift:200, 232` | `activateServer` 与 `createSession` 时各调用 1 次 | 同步 IPC 查询宿主 Bundle ID；仅在会话激活时触发，开销低 |
| `client.overrideKeyboard(withKeyboardNamed:)` | `SquirrelInputController.swift:212, 214` | `activateServer` 时调用 1 次 | 同步 IPC 通知宿主切换虚拟键码映射表 |
| `client.insertText(_:replacementRange:)` | `SquirrelInputController.swift:387` | 每次候选词上屏或 `commitComposition` 时调用 | 同步阻塞等待宿主应用主线程完成文本插入与 DOM/TextStorage 变更通知；若在非主线程调用会破坏 IMK IPC 序列 |
| `client.setMarkedText(_:selectionRange:replacementRange:)` | `SquirrelInputController.swift:411, 433` | 每次按键导致 `preedit` 变化或清空预编辑时调用 | 高频同步 IPC；每次按键均阻塞等待宿主应用主线程更新 Marked Text |
| `client.firstRect(forCharacterRange:actualRange:)` | `SquirrelInputController.swift:414, 415, 422` | 每次 `show(preedit:...)` 更新候选窗位置时调用 2～3 次 | **最高频且最易受宿主卡顿影响的同步 IPC**；单次按键内连续发起 2～3 次跨进程几何查询，若宿主主线程正忙或输入法主线程被耗时计算阻塞，极易引发双方主线程互锁超时（IMK 默认约 1～2 秒看门狗超时） |
| `client.selectedRange()` | `SquirrelInputController.swift:422` | 当 `firstRect` 返回 `.zero` 时回退调用 | 同步 IPC 查询宿主当前光标选区，与第三次 `firstRect` 串联执行 |

---

### 3.4 `SquirrelPanel` 与 `SquirrelView`：预编辑与候选词渲染管线

#### 3.4.1 `SquirrelPanel` 窗口层级、富文本组装与保留属性（`ReservedProperty`）渲染

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelPanel.swift:46-59
  init(position: NSRect) {
    self.position = position
    self.view = SquirrelView(frame: position)
    self.back = NSVisualEffectView()
    super.init(contentRect: position, styleMask: .nonactivatingPanel, backing: .buffered, defer: true)
    self.level = .init(Int(CGShieldingWindowLevel()))
    self.hasShadow = true
    self.isOpaque = false
    self.backgroundColor = .clear
    let contentView = NSView()
    self.view.frame = contentView.bounds
    self.back.frame = contentView.bounds
    self.back.blendingMode = .behindWindow
```

**工程分析**：`SquirrelPanel` 将窗口层级设为 `Int(CGShieldingWindowLevel())`（屏蔽窗口级，高于普通应用窗口、Dock 与全屏应用），确保在全屏游戏或演示模式下候选窗依然可见。内容视图由底层毛玻璃视图 `back`（`NSVisualEffectView`，通过 `shape` 图层作为 `mask`）与顶层 `SquirrelView`（内嵌 `view.textView`）叠加构成。

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelPanel.swift:421-435
      let labelRange = NSRange(location: line.length, length: labelString.utf16.count)
      let candidateRange = NSRange(location: labelRange.upperBound, length: candidateString.utf16.count)
      let commentRange = NSRange(location: candidateRange.upperBound, length: commentString.utf16.count)
      line.addAttributes(index == highlighted ? theme.labelHighlightedAttrs : theme.labelAttrs, range: labelRange)
      line.addAttributes(index == highlighted ? theme.highlightedAttrs : theme.attrs, range: candidateRange)
      line.addAttributes(commentAttrs(index: index, highlighted: highlighted, theme: theme), range: commentRange)
      if candidateRange.upperBound < line.length {
        line.addAttribute(.noBreak, value: true, range: NSRange(location: 0, length: candidateRange.upperBound))
      }
      if index > 0 && theme.linear {
        let separator = NSMutableAttributedString(string: Self.candidateSeparator, attributes: theme.attrs)
        separator.addAttribute(.noBreak, value: true, range: NSRange(location: 0, length: separator.length))
        text.append(separator)
      }
```

**工程分析**：在 `SquirrelPanel.update(preedit:selRange:caretPos:candidates:comments:labels:highlighted:update:)`（`346-484`）中：
1. **候选词模板展开与不换行约束（`.noBreak`）**：根据 `theme.candidateFormat`（默认 `"[label]. [candidate] [comment]"`）将每个候选词的序号标签、词条正文与注释拼接为单段属性字符串，并对 `[label]` 与 `[candidate]` 区间打上自定义属性 `.noBreak = true`。`SquirrelLayoutDelegate`（`SquirrelView.swift:10-19`）实现了 `NSTextLayoutManagerDelegate.textLayoutManager(_:shouldBreakLineBefore:hyphenating:)`，当检测到某字符带有 `.noBreak` 时禁止在此处折行，从而保证横向（`linear`）候选窗因超宽自动换行时，单个候选词内部绝不会被拆断到两行。
2. **插件保留属性着色（`ReservedProperty`）**：`SquirrelInputController.applyReservedProperties(in:)`（`SquirrelInputController.swift:591-640`）在每次 `updateUI()` 时查询 `_comment_highlight` 与 `_comment_warning` 属性，解析出需要强调或警告的候选词索引集合存入 `panel.highlightedCommentIndices` 与 `panel.warningCommentIndices`；`SquirrelPanel.commentAttrs(...)`（`SquirrelPanel.swift:329-344`）据此用 `theme.accentCommentTextColor` 或 `theme.warningCommentTextColor` 覆写对应候选词注释的前景色。

#### 3.4.2 多显示器边界防溢出与防抖动记忆（`SquirrelPanel.show()`）

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelPanel.swift:276-288
    if windowRect.maxX > screenRect.maxX {
      windowRect.origin.x = screenRect.maxX - windowRect.width
    }
    if windowRect.minX < screenRect.minX {
      windowRect.origin.x = screenRect.minX
    }
    if windowRect.minY < screenRect.minY {
      windowRect.origin.y = min(screenRect.maxY - windowRect.height,
                                position.maxY + SquirrelTheme.offsetHeight)
    }
    if windowRect.maxY > screenRect.maxY {
      windowRect.origin.y = screenRect.maxY - windowRect.height
    }
```

**工程分析**：`SquirrelPanel.show()`（`218-308`）根据 `position`（即 `IMKTextInput.firstRect` 测得的光标矩形）定位当前所在屏幕 `screenRect`：
- 常规情况下，水平排布的候选窗底边置于光标下方：`windowRect.origin.y = position.minY - SquirrelTheme.offsetHeight - windowRect.height`（`245`）。
- 当用户在屏幕底部边界附近输入导致 `windowRect.minY < screenRect.minY` 时，第 `279-282` 行自动将候选窗翻转到光标上方（`position.maxY + SquirrelTheme.offsetHeight`）。
- 当开启 `theme.memorizeSize`（`258-274`）且候选窗贴近屏幕右侧边界或底部边界时，Squirrel 记录历史最大宽度/高度 `maxRect`，防止翻页或继续键入时因候选词长短变化导致窗口左右/上下高频抖动。

#### 3.4.3 `SquirrelView`：TextKit 2 片段测量、三次贝塞尔平滑路径与点击命中测试

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelView.swift:105-116
  func contentRect(range: NSTextRange) -> NSRect {
    // swiftlint:disable:next identifier_name
    var x0 = CGFloat.infinity, x1 = -CGFloat.infinity, y0 = CGFloat.infinity, y1 = -CGFloat.infinity
    textLayoutManager.enumerateTextSegments(in: range, type: .selection, options: [.rangeNotRequired]) { _, rect, _, _ in
      x0 = min(rect.minX, x0)
      x1 = max(rect.maxX, x1)
      y0 = min(rect.minY, y0)
      y1 = max(rect.maxY, y1)
      return true
    }
    return NSRect(x: x0, y: y0, width: x1-x0, height: y1-y0)
  }
```

**工程分析**：`SquirrelView` 采用 macOS 12+ 的 **TextKit 2** 架构（`NSTextLayoutManager` + `NSTextContentStorage` + `NSTextContainer`）：
1. **文本几何测量**：`convert(range: NSRange) -> NSTextRange?`（`78-83`）将 UTF-16 `NSRange` 转换为 `NSTextLocation` 区间，再由 `contentRect(range:)`（`105-116`）与 `multilineRects(forRange:extraSurounding:bounds:)`（`413-480`）调用 `textLayoutManager.enumerateTextSegments(in:type:.selection)` 枚举每一行的排版片段矩形。
2. **多行高亮多边形与三次贝塞尔连续圆角**：当某个候选词或预编辑跨越多行时，`multilineVertex(leadingRect:bodyRect:trailingRect:)`（`483-510`）将其拆解为首行、中间块与尾行三个矩形并连接为 6～8 顶点的闭合多边形顶点序列，再由 `drawSmoothLines(_:straightCorner:alpha:beta:)`（`355-399`）使用三次贝塞尔曲线（`path.addCurve(to:control1:control2:)`，`alpha = 0.3 * radius`，`beta = 1.4 * radius`）生成曲率连续的平滑圆角 `CGPath`。
3. **图层遮罩合成（`draw(_:)`，`130-301`）**：将 `backgroundPath`、`preeditPath`、`highlightedPreeditPath`、`candidatePaths`、`highlightedPath` 分别封装为 `fillRule = .evenOdd` 的 `CAShapeLayer` 添加到 `panelLayer` 下，并将合成后的外轮廓赋给 `self.shape.path`，供 `SquirrelPanel` 的毛玻璃视图 `back.mask` 裁剪窗口异形外框（如右侧凸出的上下翻页小三角 `pagingLayer`，`723-755`）。
4. **鼠标点击反向命中测试（`click(at:)`，`303-341`）**：先检测点击坐标是否落在 `downPath` / `upPath` 翻页三角内；若落在面板主体内，则通过 `textLayoutManager.textLayoutFragment(for: point)` 与 `lineFragment.characterIndex(for: point)` 精确反算出被点击字符在全文中的 UTF-16 下标 `index`，再与 `preeditRange` 及 `candidateRanges` 比对，返回 `(candidateIndex, preeditIndex, pageUp)` 三元组。

---

## 4. 非侵入式语音输入与大模型后处理扩展接缝识别（Non-Intrusive Extension Seams）

在不修改 `librime` C++ 源码、不破坏现有 `SquirrelInputController` 键盘输入处理逻辑的前提下，结合上述源码索引，可在 Squirrel 层识别出以下 5 个非侵入式扩展接缝（均为**候选（待用户确认）**）：

### 4.1 扩展接缝总览表 —— 候选（待用户确认）

| 接缝编号 | 接缝名称 | 挂载宿主文件与行号锚点 | 职责边界 | 对 `librime` 与常规键盘输入的影响 |
|---|---|---|---|---|
| **接缝 S1** | 应用级语音与大模型协调器挂载点（候选（待用户确认）） | `SquirrelApplicationDelegate.swift:18-45, 167-177, 213-233`<br>`Main.swift:41-44` | 持有音频采集引擎、本地/云端 ASR 客户端与 LLM 后处理器生命周期；复用 `SquirrelConfig` 读取配置 | **零修改 `librime`**；与 `setupRime`/`startRime` 并行独立初始化 |
| **接缝 S2** | 事件分发前置热键拦截与状态机共存边界（候选（待用户确认）） | `SquirrelInputController.swift:28-91, 92-135, 247-251` | 在 `handle(_:client:)` 入口识别语音触发键（如特定修饰键长按/双击或组合键）与语音态下的 `Escape`/`Return` 控制键 | 非语音态下 100% 透传至原有 `.flagsChanged` 与 `.keyDown` 分支，键盘输入零损耗 |
| **接缝 S3** | `IMKTextInput` 流式预编辑与最终上屏复用接缝（候选（待用户确认）） | `SquirrelInputController.swift:386-436, 605-614` | 语音流式识别期间复用 `show(preedit:selRange:caretPos:)`（或 `client.setMarkedText`）实时展示流式文本；LLM 润色完成后复用 `commit(string:)`（`client.insertText`）原子替换上屏 | 不读写 `RimeSessionId` 内部编码缓冲区，避免污染 Rime 造词词频与用户词典 |
| **接缝 S4** | `SquirrelPanel` 多候选/状态浮窗复用接缝（候选（待用户确认）） | `SquirrelPanel.swift:100-147, 218-308, 310-343, 346-484` | 复用 `panel.showPanel(...)` 或 `panel.showStatus(_:)` 展示录音状态、ASR 原始文本与多个 LLM 改写候选词，并复用数字键/鼠标点击选词 | 共享同一个 `SquirrelPanel` 实例与 `SquirrelTheme` 视觉主题，无需额外创建独立窗口 |
| **接缝 S5** | 会话切换守卫与宿应用上下文提取接缝（候选（待用户确认）） | `SquirrelInputController.swift:179-225, 253-256`<br>`rime_api.h:176-188` | 在 `activateServer`/`deactivateServer`/`commitComposition` 中绑定/取消异步语音任务；读取 `client.bundleIdentifier()`、宿主选区上下文及 `RimeStatus_stdbool` 简繁/标点状态供 LLM 提示词使用 | 只读查询 `RimeStatus_stdbool`，并在焦点切出时防止异步 LLM 结果跨窗口误上屏 |

---

### 4.2 各接缝源码级协同机制分析 —— 候选（待用户确认）

#### 接缝 S1：`SquirrelApplicationDelegate` 全局服务生命周期与配置读取 —— 候选（待用户确认）

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelApplicationDelegate.swift:14-24, 213-225
final class SquirrelApplicationDelegate: NSObject, NSApplicationDelegate {
  static let rimeWikiURL = URL(string: "https://github.com/rime/home/wiki")!
  static let updateNotification = Notification.Name("SquirrelReloadNotification")

  let rimeAPI: RimeApi_stdbool = rime_get_api_stdbool().pointee
  var config: SquirrelConfig?
  var panel: SquirrelPanel?
```

**工程分析（候选（待用户确认））**：
1. **单例挂载**：`NSApp.squirrelAppDelegate`（定义于 `SquirrelApplicationDelegate.swift:449-453`）是整个输入法进程的生命周期中枢，已被 `SquirrelInputController` 与 `SquirrelPanel` 广泛引用。在此处挂载语音与大模型协调器属性（例如 `voiceController`），可避免为每个 `SquirrelInputController` 会话重复加载本地 ASR/LLM 模型权重或重建网络连接池。
2. **配置统一读取**：在 `loadSettings()`（`SquirrelApplicationDelegate.swift:213-233`）中，`config = SquirrelConfig()` 打开了 `squirrel.yaml`（及用户的 `squirrel.custom.yaml`）。由于 `SquirrelConfig` 底层直接封装了 `rimeAPI.config_get_bool` / `config_get_double` / `config_get_cstring`（`SquirrelConfig.swift:56-90`），任何新增的 `voice/*` 或 `llm/*` YAML 配置项均可直接通过 `config.getString("voice/...")` 读取并享受内存缓存，无需引入额外的配置文件解析库，且在用户点击「重新部署」触发 `loadSettings()` 时自动热重载。

#### 接缝 S2：`SquirrelInputController.handle(_:client:)` 热键分流与键盘输入隔离 —— 候选（待用户确认）

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:38-45
    if session == 0 || !rimeAPI.find_session(session) {
      createSession()
      if session == 0 {
        return false
      }
    }

    self.client ?= sender as? IMKTextInput

    switch event.type {
    case .flagsChanged:
```

**工程分析（候选（待用户确认））**：
1. **前置分流位置**：在 `SquirrelInputController.swift:40`（完成 `self.client ?= sender as? IMKTextInput` 之后、`switch event.type` 之前）是介入按键流的理想切入点：
   - 当语音控制器处于**空闲（Idle）**状态且当前 `event` 未命中语音触发热键时，直接向下进入原有的 `switch event.type`（`41-138`），常规打字、修饰键切换中英文、并击输入的行为与耗时保持 100% 原生状态。
   - 当用户按下语音触发键时：若此时 `librime` 正处于编码态（`!preedit.isEmpty` 或 `rimeAPI.get_status` 中 `is_composing == true`），可先调用 `commitComposition(client)` 或 `rimeConsumeCommittedText()` 清理现有键盘编码，再启动音频采集。
   - 当语音控制器处于**录音中（Recording）或大模型后处理中（Post-Processing）**时：可在该切入点拦截 `kVK_Escape`（取消本次语音输入并调 `clearMarkedText()` / `panel.hide()`）、`kVK_Return` / `kVK_Space`（立即采纳当前 ASR 原始文本并跳过或中断 LLM 后处理直接上屏）、或数字键 `1`～`9`（从面板中选择特定候选版本），而当用户直接敲击普通字母键时，可自动将当前语音结果上屏并无缝转入常规 `librime` 键盘编码处理。

#### 接缝 S3 & S4：`IMKTextInput` 与 `SquirrelPanel` 的双通道输出协同 —— 候选（待用户确认）

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:476-495
    show(preedit: preedit, selRange: selRange, caretPos: caretPos)
    if status.is_composing {
      let candidates = (0..<numCandidates).map { i in
        String(cString: ctx.menu.candidates[i].text)
      }
      // ...
      panel?.showPanel(preedit: panelPreedit, selRange: selRange, caretPos: caretPos,
                       candidates: candidates, comments: comments, labels: labels,
                       highlighted: Int(ctx.menu.highlighted_candidate_index),
                       page: Int(ctx.menu.page_no), lastPage: ctx.menu.is_last_page,
                       update: update)
```

**工程分析（候选（待用户确认））**：
Squirrel 的 UI 输出在 `SquirrelInputController.swift:475-495` 中天然解耦为「宿主行内 `IMKTextInput`（`show(preedit:...)` + `commit(string:)`）」与「浮动面板 `SquirrelPanel`（`panel.showPanel(...)`）」两条纯 Swift 方法调用，它们并不依赖 `RimeContext_stdbool` 指针本身，只接收普通 Swift `String` 与 `[String]` 数组：
1. **流式 ASR 阶段（Streaming Preedit）—— 候选（待用户确认）**：
   - 音频流实时识别出部分文本（Partial Transcript）时，可在主线程调用 `show(preedit: partialText, selRange: NSRange(location: 0, length: partialText.utf16.count), caretPos: partialText.utf16.count)`。
   - 这会通过 `client.setMarkedText` 将流式识别结果实时显示在宿主应用光标处（带下划线），并通过 `client.firstRect` 更新 `panel.position`；同时可调用 `panel.showPanel(...)` 在光标旁显示录音波形/耗时提示（置于 `preedit` 或 `comments` 中）。
2. **大模型后处理与多版本候选阶段（LLM Post-Processing & Selection）—— 候选（待用户确认）**：
   - **直接替换上屏模式**：当 LLM 后处理返回润色后的文本 `refinedText` 时，直接调用 `commit(string: refinedText)`（`SquirrelInputController.swift:386-391`）。由于此前流式 ASR 文本是以 Marked Text 形式存在的，`client.insertText(refinedText, replacementRange: .empty)` 会原子性地将行内预编辑文本替换为最终润色文本，无需模拟退格键（Backspace）。
   - **候选对比选择模式**：若希望同时提供「1. LLM 润色结果」与「2. ASR 原始识别结果」供用户确认，可直接调用 `panel.showPanel(preedit: ..., candidates: [refinedText, rawText], comments: ["✨ 润色", "🎙 原文"], labels: ["1", "2"], ...)`，复用 `SquirrelView` 现有的 TextKit 2 渲染与鼠标点击选词机制。

#### 接缝 S5：异步任务线程安全、`deactivateServer` 会话守卫与上下文读取 —— 候选（待用户确认）

```swift
// 源：dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift:253-256
  override func deactivateServer(_ sender: Any!) {
    hidePalettes()
    commitComposition(sender)
  }
```

**工程分析（候选（待用户确认））**：
1. **跨线程与跨会话守卫**：由于 `IMKTextInput` 的所有方法（`insertText`、`setMarkedText`、`firstRect`）均为与宿主应用主线程绑定的同步 IPC，本地 ASR/LLM 推理或云端流式网络请求**不可在主线程同步等待**，而应在后台队列/Swift `Task` 中执行，并在每次产出增量结果时切回 `@MainActor` / `DispatchQueue.main.async`。切回主线程后，需校验当前 `SquirrelInputController` 是否仍处于激活态（在 `deactivateServer(_:)` 与 `commitComposition(_:)` 中取消或标记当前语音任务失效），防止用户在等待 LLM 响应期间切换了窗口，导致迟到的 `client.insertText` 写入错误的目标应用或阻塞已失焦的客户端。
2. **宿主上下文与 Rime 状态只读桥接**：在构造 LLM 后处理提示词（Prompt）时，可直接从 `SquirrelInputController` 读取三类上下文信息而不侵入 `librime`：
   - **宿主应用身份**：`client?.bundleIdentifier()`（如 `com.apple.Terminal`、`com.microsoft.VSCode`、`com.tencent.xinWeChat`），用于适配代码术语或口语风格。
   - **光标周边上文**：通过 `IMKTextInput` 标准接口 `client?.selectedRange()` 与 `client?.attributedSubstring(from:)` 获取光标前若干字符作为语境参考。
   - **当前 Rime 简繁与标点状态**：通过 `rimeAPI.get_status(session, &status)`（`SquirrelInputController.swift:441`）只读获取 `status.is_simplified`、`status.is_traditional`、`status.is_ascii_punct`（定义于 `rime_api.h:184-187`），使 LLM 输出直接对齐用户当前的简繁与中西文标点偏好。

---

## 5. 检索日志（Query Log）

| # | 检索目标与查询内容 | 使用工具与命令摘要 | 命中文件与行数范围 |
|---:|---|---|---|
| 1 | 校验 `squirrel` 与 `librime` 仓库 HEAD Commit Hash 及目录结构 | `bash`: `git rev-parse HEAD` & `find dev-docs/refs/code/squirrel` | `squirrel` (`b7970a9`)、`librime` (`bb56876`) |
| 2 | 统计 Squirrel 全部源码文件与 `librime` C API 文件精确行数 | `bash`: `wc -l dev-docs/refs/code/squirrel/sources/* dev-docs/refs/code/librime/src/rime_api*` | `sources/*.swift` (共 12 个文件)、`rime_api.h` (608 行)、`rime_api_impl.h` (1284 行) |
| 3 | 分析 `librime` `stdbool` 变体宏定义与 `rime_get_api_stdbool` 实现机制 | `bash`: `nl -ba dev-docs/refs/code/librime/src/rime_api_stdbool.h` & `rg -n 'rime_get_api' dev-docs/refs/code/librime/src/` | `rime_api_stdbool.h:1-22`、`rime_api_impl.h:1203-1284` |
| 4 | 读取 `SquirrelInputController.swift` 全文，定位事件拦截、修饰键状态机与会话生命周期 | `bash`: `nl -ba dev-docs/refs/code/squirrel/sources/SquirrelInputController.swift \| sed -n '1,642p'` | `SquirrelInputController.swift:1-642` |
| 5 | 检索 Squirrel 中所有 `IMKTextInput`（`client.`）同步 IPC 调用点 | `bash`: `rg -n 'client\??\.' dev-docs/refs/code/squirrel/sources/` | `SquirrelInputController.swift:200, 212, 214, 232, 387, 411, 414, 415, 422, 433` |
| 6 | 读取 `SquirrelPanel.swift` 全文，分析无焦点窗口配置、多屏定位、富文本组装与鼠标事件分发 | `bash`: `nl -ba dev-docs/refs/code/squirrel/sources/SquirrelPanel.swift \| sed -n '1,571p'` | `SquirrelPanel.swift:1-571` |
| 7 | 读取 `SquirrelView.swift` 全文，分析 TextKit 2 排版测量、贝塞尔圆角路径生成与点击命中测试 | `bash`: `nl -ba dev-docs/refs/code/squirrel/sources/SquirrelView.swift \| sed -n '1,756p'` | `SquirrelView.swift:1-756` |
| 8 | 读取 `SquirrelApplicationDelegate.swift` 与 `Main.swift` 全文，分析进程启动、`IMKServer` 初始化与 `notificationHandler` | `bash`: `nl -ba dev-docs/refs/code/squirrel/sources/SquirrelApplicationDelegate.swift` & `Main.swift` | `SquirrelApplicationDelegate.swift:1-455`、`Main.swift:1-178` |
| 9 | 读取 `SquirrelTheme.swift` 与 `SquirrelConfig.swift` 全文，分析主题加载与 `RimeConfig` 缓存机制 | `bash`: `nl -ba dev-docs/refs/code/squirrel/sources/SquirrelTheme.swift` & `SquirrelConfig.swift` | `SquirrelTheme.swift:1-364`、`SquirrelConfig.swift:1-148` |
| 10 | 读取 `BridgingFunctions.swift`、`MacOSKeyCodes.swift`、`InputSource.swift`、`ReservedProperty.swift` | `bash`: `nl -ba dev-docs/refs/code/squirrel/sources/{BridgingFunctions,MacOSKeyCodes,InputSource,ReservedProperty}.swift` | `BridgingFunctions.swift:1-83`、`MacOSKeyCodes.swift:1-232`、`InputSource.swift:1-123`、`ReservedProperty.swift:1-58` |
| 11 | 读取 `Info.plist`、`Squirrel.entitlements` 与 `data/squirrel.yaml`，核对 IMK 契约与权限声明 | `bash`: `nl -ba dev-docs/refs/code/squirrel/resources/{Info.plist,Squirrel.entitlements}` & `data/squirrel.yaml` | `Info.plist:1-115`、`Squirrel.entitlements:1-12`、`squirrel.yaml:1-120` |
| 12 | 读取 `rime_api.h` 与 `rime_api_impl.h` 中会话管理、按键处理、上下文提取与候选操作实现 | `bash`: `nl -ba dev-docs/refs/code/librime/src/rime_api.h` & `rime_api_impl.h` | `rime_api.h:1-608`、`rime_api_impl.h:25-365, 974-1284` |

<!-- research-complete -->
