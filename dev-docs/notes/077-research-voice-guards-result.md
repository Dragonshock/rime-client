# 077-research-voice-guards-result：兜底两项的验收与装包结果

075 定的是「取消入口」的形状，076 定的是「谁按下它」（看门狗窗口、计时器队列）与客户端的
两处接线。本文是验收：把已有的东西重编、重打、重装，逐条把数字跑出来，并如实列出
「麦克风仍可能长开」的几种情况。

本阶段没有改行为，也没有改 `dev-docs/refs`、没有改 librime、没有调用 inject、没有重编
Rust 归档（引擎代码这次没动）。

## 1. 新增路径

| 文件 | 新增的东西 | 作用 |
| --- | --- | --- |
| `code/VoiceInterface/Sources/VoiceInterface/Types.swift` | `VoiceAbandonReason`（`label`，`.deactivate`，`.watchdog`） | 取消的由来，只进日志与 `VoiceSession.abandonReason`，不参与效果计算 |
| `code/VoiceInterface/Sources/VoiceInterface/Session.swift` | `VoiceMachine.abandon(reason:) -> [VoiceEffect]`；`VoiceSession.abandonReason` | 会话开着就置 `.closed`、`closeRequested = true`、清 `recognition` / `rewrite` / `lastShown`、记由来，返回空数组；已关或没有会话幂等返回 `[]`，不换 `id`、不消耗 `nextID` |
| `code/VoiceInterface/Sources/VoiceInterface/VoiceHandleGate.swift` | `abandon(reason:) -> Bool`、`sessionLife`、`defaultWatchdogSeconds = 60`、`watchdogSeconds` / `watchdogQueue` 两个带默认值的构造参数、`armWatchdog` / `disarmWatchdog` / `watchdogDeadline` / `noteReply` | 外部取消入口与看门狗。锁内只改机器状态，`capture.stop()` 与日志在锁外；机器返回的效果被丢弃、不经过 `perform`，所以这条路径没有写文本框的能力 |
| `code/Client/sources/SquirrelInputController.swift` | `deactivateServer` 与 `commitComposition` 的**第一行** `voiceGate?.abandon(reason: .deactivate)` | 失焦、切输入源、回车、换输入法、窗口关闭都能停采集；两处都调是有意的，取消幂等 |
| `code/VoiceInterface/Tests/VoiceInterfaceTests/VoiceHandleGateTests.swift` | 3 条取消用例 + 4 条看门狗用例（含 `spin(seconds:)` 辅助） | 见 §3 |
| `code/VoiceInterface/Tests/VoiceInterfaceTests/VoiceMachineTests.swift` | 4 条 | 外部取消零效果、幂等、不换会话、松手后取消仍零效果 |

状态迁移、为什么不走「先提交再关」、为什么取消不发 `.stopCapture`（那个效果在 `perform` 里
还会 `transport.send(.finish(...))`），075 已经写过，这里不复述。看门狗窗口 60 秒的理由、
计时器为什么在自己那条串行队列而不是主队列、撤表三处，076 已经写过。

这次只补一条本阶段的观察：**看门狗的口径是「N 秒没动静」，不是「按住超过 N 秒」。**
`noteReply` 在任何回给本会话的识别回复之后都重新起表，所以持续回话的长句不会被腰斩——
这也同时是 §5 第一条长开窗口的成因。

## 2. 正常路径没退

- 松手：`.stopCapture` 效果照旧 `capture.stop()` + `transport.send(.finish(...))`，会话留在
  `.open` 等定稿。
- 定稿：`acceptFinal` 照旧只认「`kind == .finalText` 且文本等于改写原文」，照样提交一次。
  `acceptPartial` / `acceptFinal` / `deliver` 的守卫一条没改（`Session.swift` 的改动是纯增量）。
- 取消与看门狗都不产生 `show` / `commit`；测试里 `host.shows` / `host.commits` 全空。
- 三条既有测试（`testReleaseFinishesAndCommitsOriginal`、`testJsonReplyFromTheEngineTakesTheSamePath`、
  `testRejectedFinalLogsTheReasonAndLetsTheNextPressStartAgain`）一行没改，仍然通过。

## 3. 测试结果

| 命令 | 结果 |
| --- | --- |
| `cd code/VoiceInterface && swift test` | 38 通过、0 失败（`VoiceHandleGateTests` 13、`VoiceMachineTests` 15、`VoiceEngineWarmupTests` 6、`VoiceEngineTests` 3、`VoicePythonPathTests` 1） |
| `cd code/Client && swift test` | 9 通过、0 失败（`VoiceInputControllerTests` 7、`VoiceWarmStartWiringTests` 2） |
| `cd code/Client && rm .build/out/Products/Debug/Squirrel && swift build --product Squirrel` | `Build complete!`（0.82 秒），产物 40 728 592 字节；只有既存的 `libvoice_ffi.a` 为更新 macOS 构建的 ld 警告 |
| `cd code/voice-engine && cargo test --workspace --offline` | 61 通过、4 忽略、0 失败 |
| `--test multichunk_acceptance --features metal`（voice-mlx） | 1 通过（5.30 秒） |

采集停没停这件事，测试断言的是采集源自己的状态与帧流向，不是会话状态位：
`FakeCapture.isRunning` 变假、`capture.stopped` 计数、停后再 `deliver` 与投一枚停之前残留的
`deliverStale` 都不再增加 `feed` 行数；`EnginePCMSource.stop()` 是真 `removeTap` 加
`engine.stop()`，不是只翻一个布尔。

四条看门狗用例各自证明的：

- `testWatchdogStopsCaptureAndCommitsNothing`：0.05 秒窗口、既不松手也不回话 → `stopped == 1`、
  `isRunning == false`、`.closed`、日志恰为 `voice session 1 abandoned watchdog`；停后两枚帧都进不去，
  迟到定稿不 `commit` 不 `show`。
- `testWatchdogDoesNotFireWhenTheKeyIsReleasedInTime`：及时松手 → 0.2 秒后 `stopped` 仍是 1、
  会话仍 `.open`、日志为空，随后定稿照常提交 `["定稿"]`。
- `testWatchdogWindowRestartsWhileTheEngineKeepsReplying`：0.1 秒窗口、7 条部分结果、间隔 0.02 秒，
  总时长 0.14 秒已超一个窗口 → `stopped == 0`、仍在跑、仍 `.open`；松手后定稿提交 `["定稿"]`。
- `testWatchdogLeavesTheNextSessionAlone`：第一枚被停掉后，第二次按下 `started == 2`、`isRunning == true`、
  `.open`；第二枚当场松手 `stopped == 2`，再等 0.2 秒不再变、日志仍只有第一枚那一行。

## 4. 六段真实语音与装好的包

`voice-mlx` 的 `multichunk_acceptance`：整段 PCM 一次读进来，`mel_frames` 与对同一份 PCM
现算的帧数相等，音频 token 数按参考公式算，文本逐字。第五列是引擎自己报的停止原因。

| 片段 | 时长 | 梅尔帧（现算） | 音频 token（参考） | 文本 | 停止 |
| --- | ---: | ---: | ---: | --- | --- |
| `s1` | 4.61 s | 460（460） | 60（60） | 今天下午三点开会，请提前十分钟到会议室。 | `ImEnd` |
| `s1_2.0` | 2.00 s | 200（200） | 26（26） | 今天下午三点开会。 | `ImEnd` |
| `s1_3.0` | 3.00 s | 300（300） | 39（39） | 今天下午三点开会，请提前。 | `ImEnd` |
| `s1_4.0` | 4.00 s | 400（400） | 52（52） | 今天下午三点开会，请提前十分钟到会。 | `ImEnd` |
| `s2` | 4.55 s | 455（455） | 59（59） | 幫我把這段文字翻譯成英文，然後發到郵箱。 | `ImEnd` |
| `s3` | 5.09 s | 508（508） | 66（66） | 下周的行程已经确认了吗？如果没有，我再联系一下。 | `ImEnd` |

`multichunk_acceptance` 的权重默认取 HuggingFace 缓存、metallib 默认取装好的包。为了把
**装好的包自己的权重**也过一遍，再用 `/tmp/voice-abi/bench7`（`bench.c` 编的、真读整段）
对着 `~/Library/Input Methods/RimeClient.app/Contents/Resources` 跑六段：

| 片段 | 文本 | load（权重 + metallib） | finish | 合计 |
| --- | --- | ---: | ---: | ---: |
| `s1` | 今天下午三点开会，请提前十分钟到会议室。 | 0.193 s | 1.356 s | 1.552 s |
| `s2` | 幫我把這段文字翻譯成英文，然後發到郵箱。 | 0.136 s | 1.445 s | 1.585 s |
| `s3` | 下周的行程已经确认了吗？如果没有，我再联系一下。 | 0.140 s | 1.463 s | 1.607 s |
| `s1_2.0` | 今天下午三点开会。 | — | — | 1.173 s |
| `s1_3.0` | 今天下午三点开会，请提前。 | — | — | 1.300 s |
| `s1_4.0` | 今天下午三点开会，请提前十分钟到会。 | — | — | 1.280 s |

`s1` / `s2` / `s3` 与参考逐字一致，与 074 记下的那一次相同；`s2` 仍给繁体，那是模型自己的选择。

## 5. 装包状态

| 步 | 做什么 | 结果 |
| --- | --- | --- |
| 重链 | 先删 `.build/out/Products/Debug/Squirrel`，再 `swift build --product Squirrel` | `Build complete!`，产物 40 728 592 字节 |
| 打包 | `code/Client/scripts/make-app.sh`（资产先核哈希再动 `dist`） | `dist/Squirrel.app` 重打，主二进制 40 728 896 字节；8 个 dylib 与 Sparkle 逐个重签 |
| 装包 | `code/scripts/install-app.sh` | 停旧进程 → ditto → 签名校验 → 核包内资产，四步全过 |
| 签名 | 再单独跑一次 `codesign --verify --deep --strict "$DEST"` | exit 0；`adhoc`、Mach-O thin (arm64)，标识 `im.dragonshock.inputmethod.RimeClient` |
| 资产 | `mlx.metallib` 183 870 464 字节、`weights.safetensors` 537 626 967 字节、`vocab.json` 2 776 833 字节 | 与包内 `voice-assets.sha256` 一致 |

Rust 归档这次没有重编：引擎代码在本阶段没有改动，`libvoice_ffi.a` 仍是 285 728 424 字节那一份。
`swift build` 只重链，`make-app.sh` 只重打包与重签。

## 6. 麦克风仍可能长开的情况

如实列出，一条也没有在这一阶段修。

1. **松手丢了，但引擎一直在回非空的部分结果。** 看门狗的口径是「N 秒没动静」，
   `noteReply` 会在每条回给本会话的回复之后重新起表。环境里持续有人声（旁边有人说话、
   电视、会议外放）时，引擎会一直回文本，窗口被一次次续上，**不会停**。要治这一条得另加
   「按住时长上限」之类的判断，那是另一件事。空串部分结果不下发（`apply` 在非定稿且文本为空时
   直接 return），所以纯静音不会续窗——能续窗的是「有声音且被识别出文本」。
2. **首次按下的麦克风授权回调与取消交错。** `EnginePCMSource.start` 在 `.notDetermined` 时异步
   请求授权，授权回来的 `granted` 分支**不看 `running`** 就调 `startEngine`。如果这期间会话
   被取消（失焦或看门狗），`stop()` 已把 `running` 置假，但授权回来仍会把 `AVAudioEngine`
   起起来、装上 tap：此时会话已 `.closed`、表也已撤，`push` 只把帧丢掉，麦克风则一直开着，
   直到下一次按下再松手。
3. **失焦路径完全没人叫。** 既没收到松手事件，系统也没调 `deactivateServer` /
   `commitComposition`（例如只有字段切换、或这些回调在别的路径上被吃掉）时，只能等 60 秒
   的表到点。默认窗口本身就是最长 60 秒的麦克风。
4. **主线程在持门锁时卡住。** `consume` 在锁内调 `host.voiceSnapshot()`（IMK 同步 IPC）。
   看门狗到点要取同一把锁，取不到就停不了采集。这是「主线程卡住」这条要治的场景里剩下的
   一个窄口：卡在 `push` / `perform` 之外的地方没问题，卡在持锁的那一段就没招。
5. **多枚输入控制器各有一份门与采集。** `voiceGate` 是每个 `SquirrelInputController` 实例
   各自一枚，`EnginePCMSource` 也在门里；取消只作用于被系统调到的那个实例。系统同时留着两枚
   实例时，另一枚的采集不受这次取消影响。
6. **把表关掉就等于关掉兜底。** `watchdogSeconds <= 0` 时 `armWatchdog` 首行直接返回。生产唯一
   构造点（`ensureVoiceGate`）不传这个参数，吃 60 秒默认值。
7. **取消不给引擎补 `finish`。** 解释器侧那一段流式会话留到下一次 `begin` 覆盖。这不占麦克风，
   但两次按下之间引擎侧的会话状态是残留的。
8. **被取消会话已经写出去的预编辑不清。** `lastShown` 只在本机清掉，不产生 `show`；失焦场景下
   那段预编辑交由目标窗口自己重绘。
9. **取消与 `.startCapture` 的交错没有测试覆盖。** `perform` 在门锁外执行，理论上外部队列上的
   `abandon` 可以落在 `handle` 与 `perform` 之间，那样采集会在会话已关闭之后才被起起来，而看门狗
   到点因会话非「开着」也不会停它。目前生产上唯一的外部队列调用者是看门狗本身，而它在会话
   「已经开着」时按下不会产出 `startCapture`，所以现在走不到；要动 `perform` 的加锁边界时
   得重新看这一条。

另外两条与「长开」无关、但同样还没在真实环境里验过：装好的包上没有按过一次真实的右 Option
（六段验收都走 `/tmp/voice-abi/bench7`，麦克风采集与 Swift 侧 `voice_session_feed` 的调用方
不在量程），以及看门狗与失焦取消只在 `FakeCapture` 上验过、真实 `AVAudioEngine` 的
`removeTap` / `stop` 路径没有单测。

<!-- research-complete -->
