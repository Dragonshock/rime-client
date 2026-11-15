# 078-derivation-unique-session-ids：会话号在进程内唯一

本文记的是「切换窗口之后第一句就被引擎拒掉」这一条的原因、修法的形状、为什么是共享计数器，
以及日志写成什么样才够下一次撞号时一眼看出。

这次没有改 `dev-docs/refs`，没有调用 inject，没有改 librime，没有动
`/Library/Input Methods/RimeClient.app`。Rust 侧一行没改，`voice-engine` 这次没有重编。

## 1. 撞号的现场

`VoiceMachine.nextID` 原来是一枚**每台机器自己的**计数器，从 1 起算。而语音这条路有两份寿命
对不上的东西：

| | 活多久 | 号在哪 |
| --- | --- | --- |
| `VoiceHandleGate` / `VoiceMachine` | 跟着 `IMKInputController` 实例 | 机器自己数 |
| 引擎的会话表（`voice-ffi/src/engine.rs` 里的 `sessions: HashMap<u64, Session<...>>`） | 跟着引擎进程（预热之后全进程一份） | 号是 key |

切换窗口、换输入源之后 `IMKInputController` 是新实例，新门里的 `nextID` 又从 1 开始；
而 075 定的取消路径**故意不发 `finish`**（取消的语义是「这段不算」，不请引擎再给一条定稿），
所以失焦或看门狗关掉的那枚会话在引擎的 `sessions` 里一直挂着，号一直占着。
新门 `begin(1)` 撞上还挂着的 1，引擎在 `engine.rs:338` 直接 `reply.send(-1)`，
Swift 侧 `VoiceEngine` 把它翻成「会话未被接受」，界面上就是「语音没反应」。

`voice.log` 里能看见号从 5、6 回到 1，就是这两份寿命各自计数的样子。

所以缺的不是重试、也不是「被拒之后换号重发」，而是**号本身不该由门来发**。

## 2. 接口

改动都在 `code/VoiceInterface/Sources/VoiceInterface/`，客户端两处构造门的地方一行没改。

**`Session.swift` —— 编号源。**

```swift
public final class VoiceSessionNumbers: @unchecked Sendable {
    public init(startingAt issued: UInt64 = 0)   // 已发出的最大号，第一枚是 startingAt + 1
    public func next() -> UInt64                 // 加锁自增，取一个
    public static let shared: VoiceSessionNumbers // 进程一份
}
```

一个类而不是一个 `struct`：`VoiceMachine` 是值类型，被复制、被塞进闭包都不会走到 `deinit`，
号的所有权只能放在引用上。内部一把 `NSLock` 护住 `issued`；包里没有 `swift-atomics` 这个依赖，
取号是一次加锁自增，够用，也读得出来。

**`Session.swift` —— 机器侧的注入点。**

```swift
public init(
    configuredKey: UInt16? = nil,
    hotwords: [String]? = nil,
    numbers: VoiceSessionNumbers = .shared
)
```

前两个参数保持原样、保持有默认值，所以既有的构造调用（客户端、测试）一个都不用改。
第三个参数默认就是进程级共享的那一枚，逐字写 `VoiceSessionNumbers()` 才能拿到一枚独立的。

`press()` 里取号从 `nextID` 改成 `numbers.next()`；`nextID` 留着（公开属性不删），但退成
本机流水：上一次取到的号加一。它不再参与分配，读它拿不到「下一枚会是什么号」——这件事
在共享计数器上本来就回答不了。

**`VoiceHandleGate.swift` —— 透传 + 只读出口。**

```swift
public init(..., capture: PCMSource, numbers: VoiceSessionNumbers = .shared, ...)
public var sessionID: UInt64?   // 只读，当前（或最近一枚）会话的号
```

门自己不取号，只把编号源交给它建的那台机器。默认参数意味着两处生产构造点（
`ClientLib/VoiceInputController.swift`、`SquirrelInputController.swift`）不用动，
它们拿到的就是进程共享那一枚。

## 3. 为什么是共享计数器，不是每门自增

**引擎的 key 空间是进程，不是门。** 引擎 `sessions` 是一张三十二位地址空间里的 `HashMap`，
一份引擎服务全部客户端；门的数量与它无关。号在引擎那一侧是唯一主键，那么发号的一方就必须是
与引擎同寿命的东西——同一进程里的一个计数器，而不是一堆各自数数的门。

**取消不回收号，这是上一阶段定的。** 075 的取消只停采集、不发 `finish`；看门狗同理。
既然号不会被主动还回去，任何「每门从 1 开始」的方案都会在门换掉之后立刻撞上没还的那枚。
反过来，如果把取消改成「补一条 `finish` 回收号」，那是把「这段不算」改成「请引擎给一份结果
再丢掉」，还要多走一遍解码；撞号这件事的正确解不是让旧会话消失得快一点，而是新会话不要拿旧号。

**号要比门活得久，而且只见增不减。** 计数器只在进程退出时归零。引擎进程重启时会话表本来就是空的，
所以「同进程内单调」正好是「与引擎表同寿命」，不需要持久化，也不需要从引擎那边把号要回来。

**没有选的三条路：**

- **随机大数（UUID 截断之类）。** 引擎的 key 是 `u64`，随机当然能不撞，但日志里的号就不可读了，
  「5、6 之后回到 1」这种一眼能看出来的形状也就没了。
- **号里带门的身份（`gate<<32 | local`）。** 门没有稳定的身份，`IMKInputController` 实例换一个就是
  新的一枚；要给它编号又得回到「谁来发号」这一层，等于绕回共享计数器，还多一层位运算。
- **被拒之后换号重发。** 引擎拒 `begin` 时那枚会话还挂着，重发只是再撞一次；而且「同一句话被
  引擎拒了两次才成」会让上层的时间线无法解释。号从一开始就不撞，这条路才不用存在。

## 4. 日志格式

四种行都带号，行首就是号，撞号时按号对齐就能把「谁开、谁关、谁被拒」串起来：

| 时机 | 行 |
| --- | --- |
| 新会话 begin | `voice session 7 begin bundle=com.apple.Safari` |
| 外部取消（失焦、看门狗） | `voice session 7 abandoned deactivate` / `voice session 7 abandoned watchdog` |
| 定稿落到当前会话 | `voice session 7 final` |
| 引擎拒了这一路 | `voice session 7 rejected 会话未被接受` |

规则三条：

- **号永远在最前。** 不写成 `begin session=7`，因为要对齐的是号，不是动词。
- **`begin` 带 bundle。** 换窗口这条路上最要紧的两个事实是「哪个号」与「哪个客户端」；
  同一秒里两枚门各开一枚会话时，`bundle=` 是唯一能区分它们的字段（拿不到就写 `bundle=-`）。
- **`final` 只在定稿确实落到当前会话上时才写。** 号不对、会话已经关掉的迟到定稿不写这一行，
  否则日志里会出现「定稿了」而实际一条文本都没交的假象。拒绝那一行拿不到号时退回
  `rejected <原因>`，不编一个号出来。

写日志的位置都在锁外：`begin` 在 `perform` 的 `.startCapture` 分支，`final` 在
`applyReply` 解锁之后，`abandoned` 沿用 075 的做法（先停采集、再写日志）。`VoiceMachine`
自己一行日志都不写——它是纯的，测试里换掉 sink 就能断言全部输出。

## 5. 测试

`code/VoiceInterface/Tests/VoiceInterfaceTests/VoiceSessionNumberTests.swift`（新增 4 条，走默认
编号源，不注入）：

| 测试 | 断言的是 |
| --- | --- |
| `testTwoGatesAlternatingNeverReuseASessionNumber` | 两枚门交替开、各取消一次、第一枚再开：三个号互不相等且严格递增；日志恰为 `begin` / `abandoned` ×2 + 第三次 `begin`，逐行按号对上 |
| `testSameGateReopeningNeverReusesASessionNumber` | 同一枚门按下、松手、定稿关掉、再按下：第二个号大于第一个；日志 `begin 7` / `final 7` / `begin 8` 形状 |
| `testTwoMachinesWithTheDefaultSourceTakeDistinctNumbers` | 不经门、直接两枚默认机器的号不等 |
| `testInjectedNumbersContinueWhereTheSourceIsTold` | `VoiceSessionNumbers(startingAt: 40)` 发 41，再给一台机器发 42 |

既有用例一条断言都没放宽，改的是两件事：

1. **构造处各注入一枚独立编号源**（`VoiceMachineTests` 14 处、`VoiceHandleGateTests` 13 处）。
   这些用例断言的是「1 号会话被取消」「2 号是新会话」这类**相对**形状，字面量 1 / 2 只有在
   每条用例自己从 1 起算时才成立。共享计数器由新增的 4 条专门证，两边各管一件事。
2. **日志断言补上 `begin` 行**（以及 `rejected` 行的新形状）。既有断言原本写的是
   `["voice session 1 abandoned deactivate"]`，现在同一个场景必然多一行 `begin`，这是行为变了，
   断言跟着改才对；条数与顺序仍然逐行相等，没有改成「包含」或「长度大于」。

结果：`cd code/VoiceInterface && swift test` → **42 通过、0 失败**（原有 38 条全过 + 新增 4 条）。
`cd code/Client && swift test` → 9 通过、0 失败（门的构造参数是带默认值的追加，客户端不用改）。

## 6. 还没定的

- **引擎侧要不要在会话静默过久时自己回收。** 现在号由客户端保证不撞，引擎里挂着的那枚旧会话
  照样占内存（解释器与张量）。回收条件与代价没量过，这一阶段不碰 Rust。
- **号要不要在日志之外再露一手。** 现在只有 `voice.log` 与门的 `sessionID`；要不要在按键被拒时
  弹一句带号的提示，是界面上的决定。
- **多客户端同时说话。** 引擎的会话表本来就是多枚并存，号也各自不同；但两个客户端同时按住
  （两枚引擎会话并行解码）时的内存与延迟没有量过。

<!-- derivation-complete -->
