# 076-research-voice-guards-wiring：看门狗与失焦接线

075 记的是「取消入口」长什么样。本文记两件事：那个入口由谁按下（看门狗的窗口怎么定、
计时器放在哪个队列），以及客户端在哪一行调它。取消本身不提交任何文本，这一点在 075 里
已经定死，本文没有放宽。

改动只在 `code/VoiceInterface/Sources/VoiceInterface/VoiceHandleGate.swift`、
`code/VoiceInterface/Tests/VoiceInterfaceTests/VoiceHandleGateTests.swift` 与
`code/Client/sources/SquirrelInputController.swift`。没有改 `dev-docs/refs`，没有调用 inject，
没有改 librime，没有动 `VoiceLog` 的落点。

## 1. 窗口 N：为什么默认 60 秒

`VoiceHandleGate.init` 多一个参数 `watchdogSeconds: TimeInterval`，默认值取自
`VoiceHandleGate.defaultWatchdogSeconds = 60`。取值理由：

- **正常按住只有两三秒。** 配置键是一枚修饰键，说话时按着、说完松手。一句长话也就十几秒到
  三十秒。60 秒是常见时长的若干倍，正常按住不可能被看门狗误停——误停的代价是这句话白说了。
- **真断了要多快多快。** 松手事件丢了、或者主线程卡住时，麦克风是实实在在开着的。等待时间越长，
  越可能把后面的对话录进去、也越耗电。一分钟是「用户已经觉得不对」的量级，不是半小时。
- **它只作兜底，不作正常收尾。** 正常收尾仍然是松手 → `stopCapture`，正常提交仍然只发生在定稿。
  60 秒这一条永远只在异常路径上兑现，因此宁长不宁短。
- **可注入。** 单测要证明它真的会开火，不能等 60 秒，所以窗口是构造参数：测试里传 0.05。

「毫秒级的短窗口」这件事只在测试里出现；生产唯一的一处构造（`SquirrelInputController.ensureVoiceGate`）
不传这个参数，吃默认值。

## 2. 计时器在哪个队列，什么时候撤

- **起表**：`perform` 里 `.startCapture` 的最后一个动作（`transport.send(.start)` 之后、
  `capture.start` 之后）。采集一旦开起来就有人在看表。
- **计时器跑在自己的串行队列**（`DispatchQueue(label: "rime.voice.watchdog")`，构造参数可换）。
  刻意不用主队列：主线程卡住正是这枚看门狗要治的三种情况之一，计时器跟着主队列走就一起卡死了，
  恰好在最需要它的场景里失效。
- **到点的动作**：`watchdogDeadline` 持锁核对两件事——(a) 自己是不是最新那一枚表
  （`watchdogGeneration`），(b) 会话还是不是「同一枚、`life == .open`、`closeRequested == false`」。
  两条都成立才调 `abandon(reason: .watchdog)`；否则直接返回，不 stop、不写日志。
- **撤表**：三处。`.stopCapture` 效果（松手）、`abandon(reason:)`（外部取消）、
  以及任何回给本会话的识别回复把会话带离「听着」的状态时。每次起表都换一个号，
  所以旧表到点也认不出新会话，不会误停下一个会话。
- **回话即重新起算**：引擎回了本会话的一条部分结果，窗口从这一枚回复重新计时。
  守住的口径是「N 秒没动静」：按住期间一直在出部分结果的，不会被看门狗打断；
  一句话说完却没有松手、之后也不再回话的，N 秒后停。
- **已经松手的不触发**：松手把 `closeRequested` 置真并撤表，看门狗即使已经在路上，
  到点时的 `closeRequested == false` 这一条也不成立。

停采集发生在看门狗队列上，不在主线程。`EnginePCMSource.stop()` 自带锁、`AVAudioEngine`
允许从任意线程 stop，所以这里不需要再跳回主队列；反过来说，如果要跳回主队列，
这项能力在主线程卡住时就没了。

看门狗走的是取消那条路：`abandon` 丢弃机器返回的效果、不调 `perform`，因此**没有任何 `show`、
没有任何 `commit`**，也不会 `transport.send(.finish)` 去问引擎要一条必然被丢掉的定稿。

## 3. 接线位置

`code/Client/sources/SquirrelInputController.swift` 两个方法，都是**第一行**：

```swift
override func deactivateServer(_ sender: Any!) {
  voiceGate?.abandon(reason: .deactivate)   // 新增：先关语音会话
  hidePalettes()
  commitComposition(sender)                 // 原有：提交键盘组合
  client = nil
}

override func commitComposition(_ sender: Any!) {
  voiceGate?.abandon(reason: .deactivate)   // 新增
  self.client ?= sender as? IMKTextInput    // 原有逻辑从这里开始，一字未动
  ...
}
```

为什么是这个位置：

- **放在最前**。失焦之后松手事件不会再来，采集越早停越好；同时 `voiceGate` 为 `nil`（从没按过
  语音键）时 `?.` 直接短路，这一行对纯键盘用户是零成本的。
- **放在 `commitComposition` 之前**。`deactivateServer` 会转调 `commitComposition`，两处都调是
  有意的：`commitComposition` 还会被系统单独调到（回车、换输入法、窗口关闭），只接在
  `deactivateServer` 上会漏。取消是幂等的——第二处调用返回 `false`，不再 `stop` 一次、不再写一行日志。
- **不影响原有提交键盘组合**。`abandon(reason:)` 只碰语音门的会话状态与麦克风，不碰 `preedit`、
  不碰 `client`、不碰 rime 的 commit 路径。`deactivateServer` 里原有的 `hidePalettes()` →
  `commitComposition(sender)` → `client = nil` 顺序与语义都保持原样。
- **`voiceGate` 保持 `private`**。调用点在同一个类里，不需要放宽可见性。

`code/Client` 里没有新的测试（这个目标下的用例只测纯逻辑），接线靠编译与既有 9 条用例守住。

## 4. 四条单测各证了什么

都在 `VoiceHandleGateTests`，窗口一律传 0.05（第 3 条传 0.1，见下），
辅助函数 `spin(seconds:)` 一边转主 runloop 一边把时间走过去——不转 runloop 的话
`receive` 派回主队列的回包永远不会被应用。

| 用例 | 证明了什么 |
| --- | --- |
| `testWatchdogStopsCaptureAndCommitsNothing` | 传 0.05 秒窗口，按下后既不松手也不回话，等 0.2 秒：`capture.stopped == 1`、`isRunning == false`、`sessionLife == .closed`，日志恰为 `voice session 1 abandoned watchdog`；停之后 `capture.deliver` 与 `capture.deliverStale` 都不再新增 feed（停之前拿到、停之后才跑到的那枚回调也进不去），迟到的定稿不 `commit`、不 `show`。这条证的是「采集真的停了」，不是只变了一个状态位。 |
| `testWatchdogDoesNotFireWhenTheKeyIsReleasedInTime` | 同样 0.05 秒窗口，按下后立刻松手，再等 0.2 秒：`stopped` 仍是 1（没有第二次 stop）、`sessionLife` 仍是 `.open`（看门狗没有把会话关掉）、日志为空；随后定稿照原样提交为 `["定稿"]`。这条证的是「已经松手的会话不触发」，也就是正常路径没被看门狗改动。 |
| `testWatchdogWindowRestartsWhileTheEngineKeepsReplying` | 窗口 0.1 秒，按住期间连发 7 条部分结果、间隔 0.02 秒，总时长 0.14 秒已经超过一个窗口：`stopped == 0`、采集仍在跑、会话仍 `.open`、无提交。松手后定稿照常提交 `["定稿"]`，再等 0.2 秒也没有多余 stop、没有日志。这条证的是窗口按「最后一次回话」起算，长句不会被看门狗腰斩。用 0.1 秒而不是 0.05 秒，是为了让 0.02 秒的喂入间隔离窗口有三倍以上余量，避免调度抖动造成假失败。 |
| `testWatchdogLeavesTheNextSessionAlone` | 第一枚会话被看门狗停掉（`stopped == 1`、`.closed`）之后，再按下：`started == 2`、`isRunning == true`、`.open`，采集重新开起来了；第二枚会话当场松手（`stopped == 2`），再等 0.2 秒 `stopped` 仍是 2、会话仍 `.open`、日志仍只有第一枚那一行。这条证的是「已经被停掉的会话不再触发」，以及看门狗不会把语音路卡死。 |

`swift test`（`code/VoiceInterface`）：38 条全过、0 失败（原有 34 条一条未改，新增 4 条）。
连跑三遍结果一致。`code/Client`：`swift build --product Squirrel` 通过，`swift test` 9 条通过。

## 5. 还没定的

- **松手丢了、但引擎一直在回部分结果。** 这种组合下滑动窗口会被一次次续上，麦克风不会停。
  本文这条守卫治的是「静默」，不是「没人松手」；要治后者得另加一条「按住时长上限」之类的判断。
- **看门狗停采集时要不要给引擎补 `finish`。** 现在不补，理由同 075：补来的是必然被丢掉的定稿。
  解释器侧的会话资源靠下一次 `start` 覆盖。
- **被取消会话已经写出去的预编辑不清。** 失焦场景下那段预编辑本来就不该再由我们去碰；
  用户在别的窗口里看到它，靠该窗口自己重绘。

<!-- research-complete -->
