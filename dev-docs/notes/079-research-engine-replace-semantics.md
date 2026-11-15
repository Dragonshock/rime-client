# 079-research-engine-replace-semantics：同号 begin 改成替换，取消顺手丢弃

078 把号改由进程级共享计数器发，撞号从那以后不该再出现。这一篇补的是撞号真的发生时引擎
那一侧的动作：**不拒掉新的，先把旧的丢掉，再按新参数建**。另加一条取消路径的手
（`voice_session_discard`），让「只停采集、不请定稿」的取消也能把引擎里那枚会话收掉。

这次没有改 `dev-docs/refs`，没有调用 inject，librime 一个字节没动，输入法包没有重打重装。
改的都在 `code/`：`voice-ffi`（引擎）、`VoiceInterface`（Swift 壳）、`voice-worker/worker.py`
（解释器那条旧路上的一条 `discard`），以及 `Client/Package.swift` 里的一行链接参数。

## 1. 撞号时原来发生什么，现在发生什么

原来 `engine.rs` 的 `Begin` 分支第一件事是查表，同号已经在就直接 `reply.send(-1)`。
Swift 侧 `VoiceEngine` 把它翻成「会话未被接受」并派给门，门写一行 `rejected` 之后收尾，
用户看到的是「按了没反应」。而占着那个号的那枚旧会话，属于换窗口之前的那扇门，
再没有人会来 `finish` 它，于是它一直挂在引擎的 `sessions` 里，占着号、占着解码器与状态。

现在同一处改成：把旧的那枚从表里取出来、**当着面释放**（解码器与状态一起 drop），
再按这次传进来的热词建新的。`begin` 两次都回 0。日志上留一行带号的通知，
说明这一枚号上的旧会话已经被替换。

## 2. 改动点

**`code/voice-engine/voice-ffi/src/engine.rs`**

- `Command::Begin`：同号会话存在时不再 `-1`。`sessions.remove(&session)` 取出旧的、
　`drop(stale)` 立刻释放，`emit_replaced(sinks, session)` 发一行通知，然后走原来那条
　「建 sink → new_backend → `Session::new`」的路。`finishing` 位在建新会话前清零：
　它描述的是「引擎线程上那一枚正在定稿」，那一枚已经被丢了，留着会让新会话的部分步整段被跳过。
- 新增 `Command::Discard { session, reply }`：只从表里取走并释放，不定稿、不下发文本。
　有没有那枚会话都回 0（取消允许重复，重复丢弃不该变成一次失败）。
- `Worker::discard`：与 `begin` / `finish` 同形状（发命令、等回话），引擎已停时返回非 0。
- `Command::Finish`：会话在**回话之前**就 drop（`drop(slot)`），调用线程拿到 0 的时候
　引擎表里已经没有这一枚。顺序变了、返回值没变。
- 新增 `emit_replaced`：优先走 JSON 口发一行 `kind="replaced"`；只装了文本口的旧归档
　没有位置放一条不带文本的通知，退回 `eprintln!`。

**`code/voice-engine/voice-ffi/src/json_line.rs`**

- 新增 `replaced_line(session)`：`{"decoded":false,"kind":"replaced","message":"同号旧会话已丢弃","ok":true,"session":N,"text":""}`。
　`kind` 从两种变三种，模块头与头文件都写明第三种只进日志、不当回包。

**`code/voice-engine/voice-ffi/src/lib.rs` + `include/voice_engine.h`**

- 新增 `int voice_session_discard(void *engine, uint64_t session)`：引擎指针为空回 -1，
  否则转发 `Worker::discard`。头文件加了声明，并把 `begin` 的替换语义写进注释。
- 头文件声明从九个变十个；`header_lists_nine_declarations` 跟着改名与改数（见第 4 节）。

**`code/VoiceInterface/Sources/VoiceInterface/`**

- `WorkerProtocol.swift`：`RecognitionCommand` 加 `case discard(sessionID:)`，`jsonLine()`
  写 `{"cmd":"discard","session":N}`。
- `VoiceEngine.swift`：`VoiceSymbols` 加**可选**的 `discard`（`dlsym` 找不到就是 nil，
  与 `setReplySink` / `lastError` 同一套做法）；`send` 的 `.discard` 分支调它，
  符号缺失时一条不发也不报错——那时行为就是现状。回复口那一行 `kind == "replaced"`
  的 JSON 由 `VoiceEngine` 直接写 `voice.log`，不派给门：它不是回包，门没有可做的事。
- `VoiceEngineWarmup.swift`：`VoiceEnginePreheatedTransport` 把 `.discard` 并进
  「一次也不等」那一组。
- `VoiceHandleGate.swift`：`abandon(reason:)` 在 `capture.stop()` 之后、写 `abandoned`
  那一行之前发 `.discard(sessionID:)`。取消仍然不碰文本框，一条 `show` / `commit` 也不发。

**`code/Client/Package.swift`**：链接参数加 `-u _voice_session_discard`（原来是九枚
`-u`，少的那一枚会被 `-dead_strip` 丢掉，`dlsym` 就找不到）。归档里没有这枚符号时
链接会直接报错——要退回旧行为，删掉这一行即可。

**`code/voice-worker/worker.py`**：`cmd == "discard"` 时丢掉本进程的 stream 状态，不回包。
不回包是为了不让解释器那条旧路多写一行假的 `rejected unknown cmd`。

## 3. 日志形状

撞号替换那一行也带号，行首就是号，与 078 的四种行对齐：

| 时机 | 行 | 写在哪 |
| --- | --- | --- |
| 新会话 begin | `voice session 7 begin bundle=com.apple.Safari` | `VoiceHandleGate.perform` |
| 外部取消 | `voice session 7 abandoned deactivate` / `… watchdog` | `VoiceHandleGate.abandon` |
| 定稿落到当前会话 | `voice session 7 final` | `VoiceHandleGate.applyReply` |
| 引擎拒了这一路 | `voice session 7 rejected 会话未被接受` | `VoiceHandleGate.applyReply` |
| **同号旧会话被替换** | `voice session 7 replaced 同号旧会话已丢弃` | `VoiceEngine` 的回复口（新增） |

最后一行是这次加的：引擎在这一侧没有日志文件，能过界的只有那枚回复回调，所以它跟着
已有的一行 JSON 过来，由 Swift 落在 `voice.log` 上。`message` 用的是引擎给的那句原因，
不是 Swift 现编的文案。

## 4. 单测

**引擎侧两条**（`voice-ffi/src/lib.rs`，都走 `Worker::spawn` + 一枚带标签的解码器）：

| 测试 | 断言的是 |
| --- | --- |
| `same_number_begin_replaces_the_old_session` | 同号 `begin` 两次都回 0；第一次那枚解码器在替换时就被 drop（计数 1）；随后 `feed` 满 32000 与 `finish` 走的两步都只落在新会话上，并且带的是**新的**热词（`[("新",false,true), ("新",true,true)]`，旧标签一步都没有）；JSON 口收到一行 `"kind":"replaced"` 且 `"session":4`；定稿之后两枚解码器都已释放 |
| `discard_drops_the_session_so_the_same_number_begins_again` | `begin(6)` → `feed` → `discard(6) == 0`，解码器立刻释放；此后 `finish(6) == -1`（表里已经没有那一枚）；重复 `discard(6)` 仍是 0；再 `begin(6)` 回 0，`feed` / `finish` 都作用在新会话上（旧标签只有它自己那一步，没有定稿） |

另加一条 `json_line` 用例 `replaced_line_carries_no_text_and_says_so_in_the_message`：
替换通知不携带文本、`ok` 为真、`decoded` 为假、号在。

**Swift 侧一条**（`VoiceHandleGateTests`）：`testAbandonAsksTheEngineToDropTheSession`——
`abandon(.watchdog)` 之后传输上恰好一行 `{"cmd":"discard","session":1}`；重复取消
（已经没有开着的会话）不再发第二行。

## 5. 既有 5 条 ABI 测试受不受影响

`voice-ffi` 里直接摸 C ABI 的那 5 条，逐条说：

| 用例 | 影响 |
| --- | --- |
| `header_lists_ten_declarations`（原 `…_nine_declarations`） | **改了**。声明数 9 → 10，清单加 `voice_session_discard`，用例名跟着改。它验的就是「头文件与符号表对得上」，加一枚符号必须动它 |
| `missing_model_dir_returns_null` | **加了一条断言**：`voice_session_discard(null, 1) == -1`。既有的五条空指针断言一条没删 |
| `c_abi_empty_finish_callbacks_empty_final` | 未改，全过 |
| `c_abi_feed_then_finish_runs_on_engine_thread` | 未改，全过 |
| `finish_failure_reason_reaches_the_json_sink_and_last_error` | 未改，全过（`finish` 提前 drop 会话不影响原因与返回码） |

还有一条不是 ABI、但断言了旧语义：`engine_thread_feeds_and_finishes_through_voice_asr`
里原来有一句 `assert_eq!(worker.begin(7, None), -1)`。同号 `begin` 现在回 0，这一句删掉，
并在原地留注释指向新用例：**没有放宽**——旧的那句在新用例里变成了更强的三条
（两次都回 0、两步只落在新会话、旧解码器被释放），而不是从断言退成「不检查」。

## 6. 验收

| 命令 | 结果 |
| --- | --- |
| `cd code/voice-engine && cargo test -p voice-ffi` | 16 通过、0 失败（原 13 + 引擎 2 + `json_line` 1） |
| `cd code/voice-engine && cargo test --workspace` | 64 通过、4 忽略、0 失败（原 61 + 3） |
| `cd code/VoiceInterface && swift test` | 43 通过、0 失败（原 42 + 门的丢弃 1） |
| `cd code/Client && swift test` | 9 通过、0 失败 |
| `cargo build -p voice-ffi --features metal --target aarch64-apple-darwin` | 归档重编，`nm` 里能看到 `_voice_session_discard` |
| `cd code/Client && swift build` | `Squirrel` 可执行文件链上了新的 `-u _voice_session_discard`，`nm` 里能看到 `_voice_session_discard` |

## 7. 还没定的

- **替换之后，排在同一号上的旧音频归谁。** 命令按序出队，撞号那一刻之前已经入队的 `feed`
  会被新会话收下（引擎只认号，不认门）。078 之后同一号只有一扇门在用，正常路径上不存在
  这种残留；万一真有，这一段的处理与「同一号连着说两句」没有区别。要不要在替换时连
  `feed` 队列一起清掉，没有量过，先不猜。
- **取消只在门这一侧发 `discard`。** 被引擎拒掉的那一路（`applyReply` 的失败分支）不发——
  引擎刚说过它没有这一枚，再丢一次只是多一次来回。这一条要不要统一，等有第二种拒绝原因再定。
- **引擎自己要不要在静默过久时回收会话。** 现在靠取消那条路上的 `discard` 收场；
  门如果崩了、进程没退，那枚会话还是挂着，只是号不再会撞上它。
- **`replaced` 这个 `kind` 的边界。** 它现在是「不当回包的第三种回复」，解析器照样读得出来，
  但语义只写在 `json_line.rs`、头文件与本篇里；将来若再加一种生命周期通知，该不该
  并到同一个口上，没有定。

<!-- research-complete -->
