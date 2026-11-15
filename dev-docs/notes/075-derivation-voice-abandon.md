# 075-derivation-voice-abandon：从外部取消一条语音会话

本文记的是「取消入口」这件事的推理与落地形状：为什么取消要停采集而不是补一次提交，
接口长什么样，状态怎么迁移，以及单测怎么证明麦克风真的停了。

这次没有改 `dev-docs/refs`，没有调用 inject，没有改 librime，没有动
`/Library/Input Methods/RimeClient.app`。

## 1. 缺口：麦克风的开关只挂在正常路径上

`code/VoiceInterface/Sources/VoiceInterface/VoiceHandleGate.swift` 里，麦克风只有一个开头和一个结尾：
`.startCapture` 效果里 `capture.start`，`.stopCapture` 效果里 `capture.stop`。
`.stopCapture` 只由 `VoiceMachine.release()` 产生，而 `release()` 只由「配置键松开」这一个事件触发。
于是三条路都会让采集停在半空：

1. **松手事件丢了。** 输入法进程没收到 `flagsChanged(pressed: false)`，`release()` 没跑过，
   `capture.stop` 一次也没调。
2. **按住期间 `deactivateServer`。** `code/Client/sources/SquirrelInputController.swift` 的
   `deactivateServer` 只 `hidePalettes()` 加 `commitComposition(sender)`，不碰语音门，采集继续。
3. **主线程卡住。** 按键走主线程，卡住期间松手事件排不上队，采集与喂帧都还在继续。

`VoiceHandleGate` 原本的对外面只有 `consume` / `push` / `receive`：`consume` 要一个按键事件，
`receive` 要一个引擎回包。两者都不是「从外面叫停」。所以缺的不是状态位，是一个入口。

## 2. 接口

新增的东西都在 `code/VoiceInterface/Sources/VoiceInterface/`，客户端侧一行没改。

**`Types.swift` —— 取消的由来，只进日志。**

```swift
public struct VoiceAbandonReason: Equatable, Sendable, CustomStringConvertible {
    public let label: String
    public init(_ label: String)
    public static let deactivate   // 失焦、切输入源、客户端消失
    public static let watchdog     // 松手事件没到或按住太久
}
```

用一个小结构而不是裸 `String`：调用点写 `.deactivate` 读得出来，日志里的字面量稳定；
又不像 `enum` 那样把第三处调用方挡在外面。

**`Session.swift` —— 机器侧的取消。**

```swift
public mutating func abandon(reason: VoiceAbandonReason) -> [VoiceEffect]
```

会话开着（`life == .open`，不管 `closeRequested` 是真是假）时：置 `life = .closed`、
`closeRequested = true`、清 `recognition` / `rewrite` / `lastShown`、记下 `abandonReason`，
返回空列表。会话已关或从来没有过时返回空列表，什么都不改。`nextID` 不消耗、`id` 不换。

与已有的 `abandon(sessionID:host:)` 分工不同，两个都留着：

| | `abandon(sessionID:host:)` | `abandon(reason:)` |
| --- | --- | --- |
| 谁叫的 | 引擎回包说这次被拒 | 外部（失焦、看门狗） |
| 看不看文本框 | 看，`deliveryAllowed` / 预编辑比对 | 不看，不拿 `HostSnapshot` |
| 发不发效果 | 可以发一条清缓存的空 `show` | 永远空列表 |
| 认不认会话号 | 要，号不对就不动 | 不认，只认「当前开着的那一枚」 |

**`VoiceHandleGate.swift` —— 外部入口。**

```swift
@discardableResult
public func abandon(reason: VoiceAbandonReason) -> Bool

public var sessionLife: VoiceLife?   // 只读，当前会话的生命期
```

`abandon` 拿锁、确认当前会话是 `.open`、调 `machine.abandon(reason:)`、解锁；
只有确实关掉了一枚开着的会话时才 `capture.stop()` 并写一行
`voice session <id> abandoned <reason>`（`VoiceLog`）。重复调用返回 `false`，不再停采集、不再写日志。

两处刻意的选择：

- **`stop` 与日志在锁外。** 音频回调 `push` 要拿同一把锁；`capture.stop()` 会摘掉输入节点上的 tap，
  没有理由让它握着门锁做。
- **机器返回的效果被丢掉，不交给 `perform`。** 取消一个文本都不写，这是契约不是巧合：
  走 `perform` 的话，将来任何人往这条分支加一个效果，就自动有了写文本框的能力。

## 3. 状态迁移

| 起点 | `abandon(reason:)` | 终点 | 采集 | 文本 |
| --- | --- | --- | --- | --- |
| 没有会话 | 幂等 | 没有会话 | 不动 | 无 |
| `.open`，`closeRequested == false`（按住中） | 关 | `.closed` | `stop()` 一次 | 无 |
| `.open`，`closeRequested == true`（已松手、等定稿） | 关 | `.closed` | 再 `stop()` 一次（真实采集源没在跑就直接返回） | 无 |
| `.closed`（已定稿或被拒关掉） | 幂等 | `.closed` | 不动 | 无 |

没有 `.closed → .open` 的边：取消不复活会话，下一次按下开新的，`nextID` 照常加一。
`acceptPartial` / `acceptFinal` / `deliver` 的守卫一条没改，`life == .closed` 之后迟到的回包照旧被挡在门外，
所以「取消之后才到的定稿」不写、不交。

**取消不产生 `.stopCapture` 效果。** 那个效果在 `perform` 里除了 `capture.stop()` 还会
`transport.send(.finish(...))`，等于请引擎再给一条定稿。取消这条路要的只是停麦克风，
所以门直接调 `capture.stop()`，不给引擎发 `finish`。

## 4. 为什么不走「先提交再关」

把取消做成「拿手上最后一段文本提交，然后关会话」看起来省事，实际上是三个错处叠在一起：

1. **那个时刻没有定稿可提交。** 松手丢了，引擎可能连 `finish` 都没收到；`deactivateServer` 时
   用户已经在别的输入框或别的应用里。要提交就得把最后一条部分结果当定稿，或者编一段文本。
   部分结果按现有规则不上屏，这个例外一开，`acceptFinal` 里
   「`kind == .finalText` 且文本等于改写原文」的守卫就不再是唯一入口。
2. **提交需要客户端，而取消的前提正是客户端不在了。** `host.voiceCommit` 在失焦、切源之后
   会落到别的应用或者干脆丢掉。先提交再关，等于把文本往一个已经不明的落点上推。
3. **取消的语义是「这段不算」。** 提交是全流程唯一的写入点。先交再关把「不算」变成
   「算了一点」：用户按 Esc、切走、松手丢了，都变成一次不可撤销的输入。

反过来看，取消真正需要做的只有一件事：**把麦克风关掉**。关会话是顺带的后果——不关，
`push` 还会往传输里灌帧，下一次按下也开不了新会话。所以这条路径的形状是
「先关门、再停采集」，不是「先交文本、再关门」。这也解释了为什么它不带 `HostSnapshot`：
它不打算读文本框，也不打算写。

## 5. 测试证据

采集停没停，不能只看会话状态。`VoiceInterface` 的假采集源 `FakeCapture`
现在带一个 `isRunning` 和一枚 `staleOnFrame`：`stop()` 之后它和 `EnginePCMSource` 一样不再上报帧，
但 `staleOnFrame` 保留了那一枚回调，用来模拟「停之前已经拿到、停之后才跑到主线程」的帧。

`code/VoiceInterface/Tests/VoiceInterfaceTests/VoiceHandleGateTests.swift`：

| 测试 | 断言的是 |
| --- | --- |
| `testAbandonStopsCaptureAndSendsNoText` | `stopped == 1`、`isRunning == false`、`sessionLife == .closed`；停之后再推一帧、再投一枚已经在路上的帧，传输里的 `feed` 行数都不变；迟到的 partial 与 final 一条 `show` / `commit` 都不加；日志只有一行 |
| `testSecondAbandonDoesNothing` | 第二次返回 `false`，`stopped` 还是 1，日志还是 1 行；会话仍是 `.closed`，再按下开的是第 2 次采集 |
| `testAbandonAfterReleaseStillCommitsNothing` | 先松手（`stopped == 1`、仍是 `.open`）再取消：life 变 `.closed`，随后的定稿不 `commit` 也不 `show`，再按下重新开采集 |

`VoiceMachineTests` 补的是机器侧的形状：外部取消不发任何效果（连清缓存的空 `show` 也不发，
与引擎拒绝那条 `abandon(sessionID:host:)` 正相反）、`recognition` / `rewrite` 被清掉、
`abandonReason` 记下来由、重复调用不覆盖第一次的由来也不换会话、松手后取消仍然零效果。

`swift test`：34 条全过（原有 27 条一条没改）。

## 6. 候选：还没定的

- **看门狗在哪一层、阈值多少。** 本文只给了 `VoiceAbandonReason.watchdog` 这个由来和入口；
  谁来计按住时长、松手事件拖多久算丢，未定。
- **客户端在哪一处调 `gate.abandon`。** `deactivateServer` 现在仍只 `commitComposition`；
  本文没有改它，因为取消一旦接上去就改变了失焦路径的行为，那是一次单独的决定。
- **取消时要不要给引擎补一条 `finish` 回收解释器侧资源。** 现在不发：发了会换来一条
  必然被丢掉、还要再过一遍解析的定稿。
- **被取消会话已经写出去的预编辑怎么办。** 现在不清（`lastShown` 只在本机清掉，不产生 `show`）；
  失焦场景下那段预编辑本来就不该再由我们去碰。

<!-- derivation-complete -->
