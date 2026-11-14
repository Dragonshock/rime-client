# 048-research-voice-path-self-check：按住右 Option 到上屏这条链的逐段自检

测量日：2026-11-14。现场是 `~/Library/Input Methods/RimeClient.app`（包内主程序 mtime `2026-11-13 10:32:41 CST`，字节 39616624）与仓库 `code/`。
没有调用 inject，没有写剪贴板，没有改 `/Library/Input Methods/Squirrel.app`，没有把 `/usr/bin/python3` 加回，没有改任何代码。
识别时延：未知。定稿 token 的下标：未知。有没有真实收到过一次回复：本次日志无法判定，见第 6 节。

## 0. 结论速览

- 第 1 段（按键）：源码与已安装二进制两边都显示 `handle` 先取走右 Option，取走之后交给的运输层是 `VoiceEngine`。旧 Python 运输层在已安装二进制里没有任何构造点。
- 第 2 段（引擎）：`voice_engine_new` 返回非空之后立刻 `voice_engine_set_callback`，回调 trampoline 经 `ReplyBridge` 落到 `VoiceHandleGate.receive`。两侧都能核到。
- 第 3 段（回调形状）：`RecognitionReplyParser` 的 JSON 形状与 `voice-ffi` 的实际输出不同形——`voice-ffi` 根本不写 JSON，它走 C 函数指针。进程内路径不经过该解析器，所以这个不同形在这次不会丢回复。
- 第 4 段（解码）：定稿文本最多只有 1 个词表下标，而且送进模型的不是梅尔谱。这一段确定产不出可读文本。`finish` 失败时 `VoiceHandleGate` 只写一行拒绝日志，不改会话状态，会话停在「已请求收束但未关闭」。
- 第 5 段（提交）：空定稿不产生 commit，只把会话置为关闭。这与「没有内容上屏」一致；反过来，若定稿非空，`commit(string:)` 会无条件 `insertText`，那时应该看得见字。

## 1. 按键：consume 的取走规则，右 Option 是否真的进入 VoiceEngine

### 1.1 源码

`handle(_:client:)` 在进 `switch event.type` 之前先过语音门：`Client/sources/SquirrelInputController.swift:58` 调 `ensureVoiceGate().consume(voiceKeyEvent(from: event, modifiers: modifiers))`，取走时更新 `lastModifiers` 并 `return true`（`:59-62`），不再走下面的 `processKey`。这一段的注释写在 `:57`（「取走则不进入 processKey，也不 rimeUpdate」）。

取走规则在 `Trigger.swift`：

- `VoiceTrigger.decide` 先要求 `configuredKey` 非空且属于 `ModifierKeycode.all`（`code/VoiceInterface/Sources/VoiceInterface/Trigger.swift:46`）。`rightOption` 是 `0x3D`（`:33`），在 `all` 里（`:38-41`）。
- `flagsChanged`：键码不等于配置键就放行；相等时 `pressed ? .takePress : .takeRelease`（`:48-50`）。
- `keyDown`：带 Command 且键码不同则放行，否则键码相等才给 `.takePress`（`:51-55`）。
- 「是否按下」由 `voiceFlagIsDown` 从当前修饰位推出来，右 Option 落在 `.option`（`SquirrelInputController.swift:642-643`）。

配置键是右 Option：`var voiceConfiguredKey: UInt16 = 0x3D`（`SquirrelInputController.swift:32`）。

运输层是 `VoiceEngine`，不是解释器进程：`ensureVoiceGate()`（`SquirrelInputController.swift:594-621`）里直接 `VoiceEngine(modelDirectory:metallibPath:onReply:)`（`:599-605`）配 `EnginePCMSource()`（`:614`），并用弱引用把 `ReplyBridge` 接到 `gate.receive`（`:616-618`）。引擎不可用时它只写一行日志（`:606-608`）。

取走之后的效果：`VoiceMachine.handle` 依 `decide` 分派（`Session.swift:53-65`），`.takePress` 走 `press(host:)`（`Session.swift:122-129`，要求没有已开启会话且客户端在位），产出 `.startCapture`；`.takeRelease` 走 `release()`（`:131-136`），产出 `.stopCapture`。`perform` 把 `.startCapture` 翻成 `transport.send(.start)` 加 `capture.start`（`VoiceHandleGate.swift:60-67`），把 `.stopCapture` 翻成 `capture.stop()` 加 `transport.send(.finish)`（`:68-70`）。

### 1.2 已安装二进制

`nm` 里这一段的符号都在：`handle(_:client:)` 在 `0x100029868`，`VoiceHandleGate.consume` 在 `0x100086d90`，`ensureVoiceGate` 在 `0x10002bcc8`。反汇编 `handle` 的顺序是：`ensureVoiceGate`（`bl 0x10002bcc8`，`0x100029fa8`）→ 造键事件（`0x100029fc0`）→ `VoiceHandleGate.consume`（`bl 0x100086d90`，`0x100029fe8`）→ 之后才比对 `NSEventType` 并进键盘链（`0x10002a00c` 起）。与源码顺序一致。

右 Option 的常量也核到了：`voiceConfiguredKey` 的默认值取值器 `...voiceConfiguredKeys6UInt16Vvpfi` 在 `0x100029620`，函数体是 `mov w0, #0x3d; ret`。`ModifierKeycode.rightOption` 取值器在 `0x10007afac`，同样返回 `0x3d`；`ModifierKeycode.all` 的集合字面量 `0x10007b054` 处的 `mov w8, #0x3d` 就是这一项。

`ensureVoiceGate` 的反汇编里能看到同一份构造顺序：`VoiceEngine` 初始化 `bl` 在 `0x10002bde8`，`EnginePCMSource()` 在 `0x10002becc`，`VoiceHandleGate` 初始化在 `0x10002bf20`。反过来的传输层（`PythonRecognitionTransport`）只有一个定义，没有调用点：它的初始化 `...ctcfC` 在 `0x100070d44`，整套二进制里对该符号的引用只有它自己那一处，`0x100070da4` 是它的函数体。

### 1.3 日志能证明到哪一步

`EnginePCMSource.start` 里成功启动后才写 `audio engine started`（`code/VoiceInterface/Sources/VoiceInterface/PCMSource.swift:66-68`），而 `EnginePCMSource.start` 只有一个调用点：`VoiceHandleGate.perform` 的 `.startCapture` 分支（`VoiceHandleGate.swift:65`），它只由 `press()` 产出，`press()` 只由 `.takePress` 调到，`.takePress` 只在键码等于 `0x3D` 时给出。因此 `2026-11-14T04:00:14Z` 与 `04:00:21Z` 那两行 `audio engine started` 本身就证明了这一次按键的取走规则按上表生效，并且 `transport.send(.start)` 之后的 `voice_session_begin` 被引擎接受（返回非 0 会走 `reject`，`VoiceEngine.swift:107-111`，并在 `VoiceHandleGate.swift:83-87` 写 `recognition rejected 会话未被接受`；日志里没有这一行）。

旧路径的日志签名是 `launch <解释器路径> <worker.py>`（`RecognitionTransport.swift:34`），`2026-11-12` 那批日志里有，`2026-11-14` 那两条前后没有。

**这一段的结论**：右 Option 走的是 `VoiceEngine`，不是旧 Python 路径；取走规则、按键常量、门的构造顺序在源码与已安装二进制两处对得上。这一段没有断。日志无法区分「这一下是右 Option」还是「键码同为 0x3D 的其它来源」，但键码匹配这一步确实成立。

## 2. 引擎：set_callback 是否装上，trampoline 是否送到 receive

### 2.1 源码

- 先创建：`VoiceEngine.init` 用 `modelDirectory.withCString` 调 `symbols.create(model, metal)`（`VoiceEngine.swift:64-71`），`created` 为空则 `unavailableReason = "模型目录打不开"` 并释放盒子（`:72-78`）。
- 再装回调：`symbols.setCallback(created, voiceTextTrampoline, opaque)`（`VoiceEngine.swift:79`），紧跟 `engine = created`（`:80`）与 `callback = opaque`（`:81`）。上下文是 `Unmanaged.passRetained(CallbackBox(onReply:))`（`:62-63`）。
- 落点：`ensureVoiceGate` 的 `onReply` 只做 `bridge.onReply?(reply)`（`SquirrelInputController.swift:602-604`），`bridge.onReply` 被设为 `gate?.receive(reply)`（`:616-618`）。
- 门里：`receive` 把 `applyReply` 异步投到 `callbackQueue`（`VoiceHandleGate.swift:54-58`），`SquirrelInputController` 没传该参数，取默认 `.main`（`VoiceHandleGate.swift:20`）。
- 引擎侧：`voice_engine_set_callback` 把 `function` 与 `ctx` 包成 `Callback` 交给 `Worker::set_callback`（`code/voice-engine/voice-ffi/src/lib.rs:120-130`），`SessionSink::on_transcript` 每次发射前现取一份回调，空则直接返回（`code/voice-engine/voice-ffi/src/engine.rs:61-75`，`set_callback` 在 `:143-145`）。因为取的是共享槽位，回调可以在 `begin` 之后才装也不会漏。

### 2.2 已安装二进制

`VoiceEngine` 的方法符号被内联掉了，只剩类元数据；能直接核到的是它的初始化体 `...ctcfc`（`0x100081c60`）与符号装载 `VoiceSymbols.load`（`0x100080b0c`）：

- `load` 里六条 `dlsym` 名字字面量按 `voice_engine_new`、`voice_engine_free`、`voice_session_begin`、`voice_session_feed`、`voice_session_finish`、`voice_engine_set_callback` 排开（`0x100080b38`、`0x100080bb8`、`0x100080c38`、`0x100080cb8`、`0x100080d38`、`0x100080db8`），每条后面跟一次 `symbol(_:)`。装载器不认 `setCallback` 之外的任何顺序差异，六条都在。
- 初始化体在 `0x100081fc0` 这一段（`created` 非空分支）里做三件事：`x0` 取 `created`、`x1` 取 `0x100084adc`、`x2` 取盒子指针，然后 `blr x8`（`0x100081fdc`）。`0x100084adc` 正是 `voiceTextTrampoline...XCvpfiyAE_AikLtcfU_To`，也就是那个对外 C 约定的 trampoline 入口。之后把 `created` 存到 `self + 0x50`（`engine`）、把盒子指针存到 `self + 0x58`（`callback`）（`0x100082014`、`0x100082044`）。与 `VoiceEngine.swift:79-81` 同序。
- trampoline 本体（`0x100084ae4` 起）里能看到 `""`、`"final"`、`"partial"` 三个字面量（`0x1000847e8`、`0x100084868`、`0x100084890`），随后调 `RecognitionReply` 的成员初始化（`bl ...RecognitionReplyV9sessionID4text7decoded4kind2ok7messageAC...cfC`，`0x1000848f0`），再 `blr x8` 调盒子里的 `onReply`（`0x1000848fc`）。

**这一段的结论**：装回调这一步在源码（`VoiceEngine.swift:79`）与已安装二进制（`0x100081fc0`-`0x100081fdc`）两边都成立；trampoline 会把 `is_final` 翻成 `kind` 并调 `onReply`，门那边接的是 `VoiceHandleGate.receive`。这一段没有断。至于这段路在这次有没有真的跑过一次，见第 6 节。

## 3. 回调：`RecognitionReplyParser` 的形状与 voice-ffi 的实际回写

### 3.1 解析器只认 JSON

`RecognitionReplyParser.parse` 从一行 JSON 里取键：`session`、`text`、`decoded`、`kind`、`ok`、`message`（`code/VoiceInterface/Sources/VoiceInterface/WorkerProtocol.swift:60-74`），`ok` 缺省为 `false`、`decoded` 缺省为 `false`（`:69`、`:71`）。`RecognitionReply` 的字段见 `:42-58`。

这个解析器只有一个调用点：`PythonRecognitionTransport.consume`（`RecognitionTransport.swift:59-70`，`parse` 在 `:66`）。也就是说它是**解释器进程那条路的行协议**。

### 3.2 voice-ffi 不写 JSON

`voice-ffi` 的回写是 C 函数指针，不是 JSON 行：头文件里 `typedef void (*voice_text_fn)(uint64_t session, const char *utf8, int is_final, void *ctx);`（`code/voice-engine/voice-ffi/include/voice_engine.h:14`），装载点是 `voice_engine_set_callback`（`lib.rs:120-130`），发射点是 `SessionSink::on_transcript`：`(callback.function)(self.session, c_text.as_ptr(), flag, callback.ctx)`（`engine.rs:71-73`），其中 `flag` 是 `is_final ? 1 : 0`（`engine.rs:70`）。`voice-ffi` 三个源文件里没有写 stdout 的代码，也没有 JSON 序列化。

进程内路径根本不经过解析器：引擎回调 → trampoline 直接构造 `RecognitionReply`（`VoiceEngine.swift:184-196`）→ `onReply` → `gate.receive`。

### 3.3 不匹配的键

按「解析器要的」对「voice-ffi 给的」逐键对：

| 解析器读的键（`WorkerProtocol.swift:65-72`） | voice-ffi 的位置 | 是否对得上 |
| --- | --- | --- |
| `session` | 第 1 个形参 `uint64_t session` | 值有，键没有 |
| `text` | 第 2 个形参 `const char *utf8` | 值有，键没有；名字也不同（`utf8`） |
| `kind` | 无；只有 `int is_final`（`0`/`1`） | 缺，且 `is_final` 这个名字解析器不读 |
| `ok` | 无。成功路径恒为真（`VoiceEngine.swift:192`） | 缺 |
| `decoded` | 无；trampoline 恒填 `true`（`VoiceEngine.swift:191`） | 缺 |
| `message` | 无；失败走 `reject` 的另一条路（`VoiceEngine.swift:155-164`） | 缺 |

这就是解析器形状与 `voice-ffi` 形状的全部差集：`ok`、`kind`、`decoded`、`message` 四个键在 C 侧不存在，而 C 侧的 `utf8`、`is_final` 两个名字解析器不认；`session` 与 `text` 只是同值不同名。

同一张对照里，解释器那条路是对得上的：`code/voice-worker/worker.py` 回 `{"ok":…,"session":…,"kind":…,"text":…}`（`worker.py:22`、`:40`、`:52`、`:61-64`、`:78-80`）。

**这一段的结论**：形状不符的不是 C 结构体字段，而是「有没有 JSON」这件事本身——`RecognitionReplyParser` 只服务解释器路径，`voice-ffi` 一律走函数指针。因为进程内路径不经过解析器，这个不同形**不会**把回复吞掉。这一段也没有断，它只是两套契约并存。

## 4. 解码：一个 token 为什么不可能读出可读文本，以及 finish 失败时门怎么处理

### 4.1 一次定稿只有一条下标

- `transcribe_finish(model_dir, pcm) -> Result<u32, _>`（`code/voice-engine/voice-mlx/src/backend/finish.rs:22-29`）返回的是**一个** `u32`，没有序列、没有循环、没有停止符。
- 取法是 `argmax_axis` 在最后一维上取最大值，再 reshape 成 `[1]` 取第 0 个（`finish.rs:108-127`）。
- 上层把它装成 `tokens: vec![token]`，`text` 是该 token 在 `vocab.json` 里那一个词片（`code/voice-engine/voice-ffi/src/decoder.rs:88-101`）；词片只是把 `Ġ` 换成空格，别的原样（`vocab.rs:27-38`）。

所以定稿文本的长度上限是**一个字节级 BPE 词片**。一个词片是子词片段，不是句子；它可能是半个汉字、一个带前导空格的 Latin 片段，或者一个特殊控制 token 的名字。要读出「一句话」，这条路径在结构上没有位置放第 2 个到第 N 个 token。

### 4.2 送进模型的也不是谱

`pcm_grid` 把整段 PCM 按位置分到 64 个桶里，每桶取平均，得到 64 个 `f32`，reshape 成 `[1, 8, 8, 1]`（`finish.rs:137-154`）。这不是梅尔谱、不是帧序列，长度与内容都只保留成一个 8×8 的均值图。随后只接了 `audio_tower.conv2d1`、`audio_tower.conv_out`、第 0 层 `q/k/v`、`audio_tower.proj2`，最后用 `model.embed_tokens` 当分类头（`finish.rs:31-73`、`:208-238`）。这条链路没有梅尔前端、没有逐层编码器、没有文本解码器层、没有终局 norm。也就是说，即使这一次形状全部对上，argmax 得到的也不是「说出来的那个字」。

两句合起来：单 token 与伪谱两件事各自都足以让定稿不可能是可读文本；它们同时成立。这一次日志里没有留下任何 token 下标，「这个 token 具体是什么」：未知。

### 4.3 finish 失败时 VoiceHandleGate 的处理

失败的分支有三处：`transcribe_finish` 返回 `Err`、词片查不到、以及 `pcm` 为空（`decoder.rs:72-77` 直接返回空文本且**不**记失败）。前两者把 `failed = true`（`decoder.rs:80-86`、`:90-96`），引擎线程据此把 `voice_session_finish` 的返回码标成 `-1`（`engine.rs:268-278`，判据是 `slot.decoder().weight_failed()`，`:274`）。

Swift 侧：`send(.finish)` 见到非 0 就 `reject(sessionID, "定稿未被接受")`（`VoiceEngine.swift:117-121`），`reject` 造一条 `ok: false` 的回复（`:155-164`）。门里 `applyReply` 碰到 `ok == false` 只做一件事：同一句只写一次 `recognition rejected <message>`，然后 `return`（`VoiceHandleGate.swift:81-88`）。

后果是状态不动：

- 会话仍是 `.open`，因为只有 `acceptFinal` 才会把它置 `.closed`（`Session.swift:100`、`:107-110`）。
- `closeRequested` 已在 `release()` 里置真（`Session.swift:131-136`），所以此后 `push` 里的帧全被丢掉（`VoiceHandleGate.swift:44-52`）。
- 再按一次也开不出新会话：`press` 要求 `life != .open`（`Session.swift:123`），于是这一次按键被取走、却什么都不发生（`VoiceHandleGate.swift:39-41` 仍然返回 `true`）。

也就是说一次定稿失败就把这个控制器的语音路卡住，直到拿到新的 `IMKInputController`（换了客户端/输入框）或进程重启。日志只能看到那一行拒绝，看不到后续。

另外一处与「松开后没有反应」直接相关：`voice_session_finish` 是同步等待的——`Worker::finish` 在调用线程上 `reply_rx.recv()`（`engine.rs:174-187`，C 侧 `lib.rs:108-113`），而调用它的线程正是处理按键的线程（`VoiceHandleGate.perform` 在 `consume` 之后同步执行，`VoiceHandleGate.swift:39-41`、`:68-70`）。回调又要回到主队列（`:54-58`）。所以定稿这一下会先把调用线程按住，等引擎线程算完；一次算多久：未知。

**这一段的结论**：这一段确定不通——文本源头只有一个词表下标，而且不是从谱上算出来的。这是链上第一处「无论如何也接不出可读文本」的地方。失败时门的处理是「记一行、什么都不改」，会把会话留在半关闭状态。

## 5. 提交：空定稿为什么不 commit，与「没有内容上屏」是否一致

`acceptFinal` 先记下 `recognition.kind = .finalText`（`Session.swift:91`），然后看文本：

- 文本为空：只把会话置 `.closed`（`:100`），并在「预编辑区还在且内容等于上次显示的串」时补一个清空显示（`:95-99`、判据 `shouldClearVoiceCache` 在 `:114-120`）。这条分支**不**产出 `.commit`。
- 文本非空：记一条改写记录（`:104`，`Rewrite.swift:3-8` 恒返回 `verdict = .unchanged`、`returned = nil`），再走 `deliver(.commit)`（`:106`）。`deliver` 的提交分支要求识别记录是定稿且同串（`:151-155`）、改写记录已到（`:156`，`.unchanged` 满足「不是 `.notArrived`」）、且交付许可与客户端标识都成立（`:141`）；满足时给出 `.commit`（`:167`），`perform` 把它翻成 `host?.voiceCommit(text)`（`VoiceHandleGate.swift:73-75`）。

`voiceCommit` 落到 `commit(string:)`（`SquirrelInputController.swift:590-592`），那里只有在 `client == nil` 时才悄悄返回（`:658`），否则就是一次 `client.insertText(string, replacementRange: .empty)`（`:676`）。

**与观察是否一致**：一致。空定稿的唯一出口是「关会话 + 可能清一下预编辑」，不上屏，也不写日志。所以「只有 `audio engine started`、没有任何内容上屏」这一组观察与「回调到了、但定稿是空串」完全自洽。

反过来也要写清楚：如果定稿非空，`commit(string:)` 会无条件 `insertText`，那么应该看得见那一个词片。观察里没看见字，这更支持「定稿为空」或「回调没到」，不支持「文本到了但被别处吞掉」这种说法——因为从 `acceptFinal` 到 `insertText` 之间没有别的静默出口（唯一的 `guard let client` 已被 `deliver` 的 `clientMissing` 判定提前挡掉，`Session.swift:141`）。

## 6. 这次现场读到的与读不到的

### 6.1 已安装程序与日志的归属

- 主程序 mtime `2026-11-13 10:32:41 CST`，字节 39616624；`nm` 里六个 `voice_*` 都是 `T`（`_voice_engine_new` 在 `0x1000a5d5c`），与 `047` 记的地址一致。
- 当前进程 pid 1051，启动时间 `2026-11-13 10:43:43 CST`，仍是 `~/Library/Input Methods/RimeClient.app/Contents/MacOS/Squirrel`。
- `voice.log` 里 `引擎未链入` 的三行在 `2026-11-13T01:35:56Z`、`01:38:11Z`、`02:19:59Z`，即 `09:35:56`、`10:19:59` CST 等，都早于 10:32 的安装和 10:43 的这次启动；它们是**上一份**二进制留下的。
- 这次进程写的只有 `2026-11-14T04:00:14Z` 与 `04:00:21Z`（`12:00:14`、`12:00:21` CST）两行 `audio engine started`，前后没有任何 `引擎未链入`、`recognition rejected …`、`feed 未被接受`。
- 这个进程的打开文件里有包内 `Resources/mlx.metallib`（fd 21，只读，183870464 字节）。把 metallib 交给 MLX 的只有 `voice_engine_new` 之后的 `prepare` 这条线（`lib.rs:35-47`），所以可以判读为：这一份已安装主程序里 `voice_engine_new` 被真的调用过，并且没有在这一步失败。这不表示「能识别出字」。

### 6.2 日志不能判定的事

两行 `audio engine started` 之间隔 7 秒。两种落地都能解释这一对：

- 同一个控制器里两次按下。此时第二行成立的前提是中间有一次 `acceptFinal` 把会话置 `.closed`（`Session.swift:100`、`:107-110`），也就是说那次定稿是空串（否则会 `insertText`）。这条解释把「回调到了但定稿为空」当成事实。
- 两个 `IMKInputController`（不同客户端/输入框各一个实例）各按一次，每次都在松开后没有可观察的落地。

第二种解释不需要任何定稿成立。因此「回调到底有没有到达」这次没有测到，判为**未知**；同理「定稿是不是空串」「这一次的 token 下标」都是未知。

若确实有一次回调到达且定稿为空、同时 `voice_session_finish` 返回 0，那么空文本只可能来自两处：会话样本数为 0（`session.rs:178-181` 那条 `total_samples == 0` 分支）或 `WeightDecoder` 里累积样本为空（`decoder.rs:72-77`）。这两处都意味着音频样本没有进到解码器。`feed` 侧不是静默的（队列满/断开都会记一行，`VoiceEngine.swift:112-116`），日志里也没有那一行——但「一帧都没到」同样不会留下日志，两者区分不开。

## 7. 边界

- 没有改任何代码；没有 inject；没有写剪贴板；没有碰 `/Library/Input Methods/Squirrel.app`。
- 第 1 至 3 段与第 5 段的判断依据是源码，加上已安装二进制里的符号与反汇编定位（十六进制地址都是这台机器上这一份主程序里的）。
- 第 4 段是源码级判定，不是这次跑出来的读数；这次没有调用 `voice_session_finish`，没有真实麦克风语句。
- 「引擎创建成功」「回调装上」「按键取走」都不等于「能识别出字」。

## 8. 仍未知

- 这次两行 `audio engine started` 各自属于哪个控制器、有没有一次定稿被接受：未知。
- 定稿回调是否到达过、到达时的 token 下标与文本：未知。
- 按住到出字的总时延：未知。
- 那一次 `voice_session_begin` 在引擎线程上建会话用了多久：未知。
- `EnginePCMSource` 的 `AVAudioConverter` 在这次是否真的交出了帧：未知。

## 检索

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | `grep -rn` 关键字 | `code/VoiceInterface`、`code/Client/sources` | `handle` 先 `consume` 再 `switch`；运输层为 `VoiceEngine` |
| 2 | `nm -n` | 已安装 `Contents/MacOS/Squirrel` | `handle` `0x100029868`；`consume` `0x100086d90`；`ensureVoiceGate` `0x10002bcc8` |
| 3 | `llvm-objdump --macho --disassemble` | 同上 | `handle` 里 `bl ensureVoiceGate` → `bl consume` 在 `0x100029fa8` / `0x100029fe8` |
| 4 | 同上 | 同上 | `voiceConfiguredKey` 默认值取值器 `0x100029620` 返回 `0x3d`；`all` 字面量含 `0x3d`（`0x10007b054`） |
| 5 | 同上 | 同上 | `ensureVoiceGate` 里 `bl VoiceEngine.init` `0x10002bde8`、`bl EnginePCMSource()` `0x10002becc`、`bl VoiceHandleGate.init` `0x10002bf20` |
| 6 | `nm` | 同上 | `PythonRecognitionTransport` 初始化 `0x100070d44` 只有定义，无调用点；`worker.py` 字面量 0 次 |
| 7 | 同上 | 同上 | `VoiceSymbols.load` `0x100080b0c` 六条 `dlsym` 名字，`set_callback` 在 `0x100080db8` |
| 8 | 同上 | 同上 | `VoiceEngine` 初始化体 `0x100081c60`；成功分支 `0x100081fc0` 用 `x1 = 0x100084adc` 调 `blr x8`（setCallback） |
| 9 | 同上 | 同上 | trampoline 本体 `0x100084ae4`：`""`/`"final"`/`"partial"` → `RecognitionReply` 初始化 → `blr x8` |
| 10 | `grep` | `code/voice-ffi`、`code/voice-worker` | `voice-ffi` 无 JSON 写出；`worker.py` 回 `ok/session/kind/text` |
| 11 | 读码 | `finish.rs`、`decoder.rs`、`session.rs` | `transcribe_finish -> Result<u32>`；`tokens: vec![token]`；`pcm_grid` 64 桶均值 |
| 12 | `tail`/时间换算 | `~/Library/Logs/RimeClient/voice.log` | `01:35/01:38/02:19Z` 早于安装；`11-14T04:00:14Z`、`04:00:21Z` 两点只 `audio engine started` |
| 13 | `ps -o lstart` | pid 1051 | 启动于 `2026-11-13 10:43:43 CST`，即包内主程序 |
| 14 | `lsof -p 1051` | 同上 | 打开包内 `Resources/mlx.metallib`（fd 21，只读） |
| 15 | `stat` | 已安装主程序 | 39616624 字节，mtime `2026-11-13 10:32:41 CST` |

<!-- research-complete -->
