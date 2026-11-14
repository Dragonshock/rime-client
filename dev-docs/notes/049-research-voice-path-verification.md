# 049-research-voice-path-verification：修完第 3、4 段之后每步实测

测量日：2026-11-14。对象是仓库 `code/` 与装到 `~/Library/Input Methods/RimeClient.app` 的那一份包。
没有调用 inject，没有写剪贴板，没有改 `/Library/Input Methods/Squirrel.app`，没有把 `/usr/bin/python3` 加回。
改动只落在 `code/`；`dev-docs/` 只加了本篇与 MAP 的状态、进行中、notes 索引。
识别时延：未知。这次没有真实麦克风语句，也没有按下右 Option。

## 0. 结论速览

- 第 3 段的两套回复形状现在合成一套：`voice-ffi` 写一行 JSON，键就是 `RecognitionReplyParser` 读的六个；`VoiceEngine` 用同一个解析器收。文本指针留着当旧归档的退路，装上 JSON 口时不再被调用。
- 第 2 段（`created` 之后立刻装回调）在 048 已由源码与已安装二进制的两条路径核过。本次补到的是运行证据：一个临时程序用同一份静态归档跑出 `voice_engine_new` 非空、`begin` 返回 0、JSON 口收到带 `kind` 的行，而文本指针一次都没被调用。
- 第 4 段照实说：当前解码器只能给一个 token。定稿不再把那个词片当文本交出去，文本为空串，`voice_session_finish` 回 -1，原因同时写进 JSON 行与 `voice_engine_last_error`，`VoiceHandleGate` 把它写进 `voice.log`。这一次的实测原因里带着词表下标 106074 与词片 `åĢŁéī´`——它本身就是两段 UTF-8 字节按 Latin-1 显示的结果，不是一句话。
- 定稿失败不再把会话卡在「已请求收束但未关闭」。门现在把该会话置为关闭，下一次按下能开出新会话（单测覆盖）。
- 四步必跑的命令全过：`cargo test --workspace`、`cargo build -p voice-ffi --features metal --target aarch64-apple-darwin`、`code/VoiceInterface` 的 `swift test`、`code/Client` 的 `swift build --product Squirrel`。
- 包已重装：`MacOS/Squirrel` 39680272 字节、`Resources/mlx.metallib` 183870464 字节、`Resources/Models/Qwen3-ASR-0.6B-4bit/weights.safetensors` 537626967 字节，模型目录符号链接数 0，`codesign --verify --deep --strict` exit 0。装完的包内主程序 md5 与 `code/Client/dist/Squirrel.app` 那份一致。
- 「创建成功」「回调装上」「回复形状对上」都不等于「能识别出字」。这次仍然没有识别出字，而且现在连单 token 也不上屏。

## 1. 第 3 段：回复改成同一形状的一行 JSON

### 1.1 改动

- `code/voice-engine/voice-ffi/src/json_line.rs`（新文件）：`reply_line(session, text, kind)` 与 `failure_line(session, message)`。键按字典序输出：`decoded`、`kind`、`message`、`ok`、`session`、`text`，缺的键不写。转义只处理 JSON 必需的那几种（引号、反斜杠、`\n`、`\r`、`\t`、`\b`、`\f`、其余 `< 0x20` 走 `\u00xx`）。
- `code/voice-engine/voice-ffi/include/voice_engine.h`：新增 `typedef void (*voice_reply_fn)(const char *json_line, void *ctx);` 与 `void voice_engine_set_reply_sink(void *engine, voice_reply_fn fn, void *ctx);`。注释写明装上之后不再调 `voice_text_fn`、定稿失败也在这个口发 `ok:false`。
- `code/voice-engine/voice-ffi/src/engine.rs`：两个槽位合成 `Sinks { text, reply }`（`engine.rs:54`）。`SessionSink::on_transcript`（`engine.rs:88`）先看 JSON 口，有就发一行并 `return`；没有才走旧文本指针。新增 `emit_failure`（`engine.rs:111`）只走 JSON 口——文本指针没有位置放原因。
- `code/VoiceInterface/Sources/VoiceInterface/VoiceEngine.swift`：`VoiceSymbols` 多两条可选符号（`:293`、`:294`）；`init` 里 `setReplySink(created, voiceReplyTrampoline, opaque)`（`:82`），符号缺了才退回 `setCallback(created, voiceTextTrampoline, opaque)`。`voiceReplyTrampoline`（`:220`）只做一件事：`RecognitionReplyParser.parse(String(cString: line))`，解析器仍然只有这一处。

### 1.2 逐键对照

| 解析器读的键（`WorkerProtocol.swift:65-72`） | `voice-ffi` 现在给的 |
| --- | --- |
| `session` | `"session":7`，数字 |
| `text` | `"text":"…"`，字符串；失败行是 `""` |
| `decoded` | `true`：这条回复带着一段解出来的文本 |
| `kind` | `"final"` 或 `"partial"`；失败行不写 |
| `ok` | `true`；失败行 `false` |
| `message` | 只有失败行有，写原因 |

旧形状（C 函数指针 `voice_text_fn(uint64_t, const char*, int, void*)`）的差集在这里闭合：那四个键 `ok`、`kind`、`decoded`、`message` 现在都在；`utf8` 改名成 `text`；`is_final` 变成 `kind` 的取值。

`decoded` 一开始被我写成「定稿才为真」，随后发现 `VoiceHandleGate.applyReply` 的部分分支要读这一位（`VoiceHandleGate.swift:102`），写成假会让预编辑永远出不来。改成恒真，并在 `VoiceHandleGateTests.testJsonReplyFromTheEngineTakesTheSamePath` 里用一行 `kind:"partial"` 的 JSON 钉住这条：先上预编辑，再提交定稿。

### 1.3 实测：JSON 口在真进程里收到行

临时程序（`/tmp/voice-abi/check.c`，不在 `code/` 内）链同一份 `libvoice_ffi.a`，只做 `voice_engine_new`（参数是已安装包内的模型目录与 `mlx.metallib`）、同时装文本指针与 JSON 口、`begin(42)`、`finish(42)`（没有 feed）。输出：

```
new non-null
begin 0
json {"decoded":true,"kind":"final","ok":true,"session":42,"text":""}
finish 0
last_error 0 []
freed
```

文本指针那一行一次都没出现——JSON 口在位时它确实不被调用。这一条对应 048 里「回调装上」的运行面证据；048 的源码与反汇编核对仍然有效。

## 2. 第 4 段：一个 token 不当文本，原因进 voice.log

### 2.1 改动

- `code/voice-engine/voice-asr/src/session.rs`：`DecodeText` 多一个 `failure_reason(&self) -> Option<String>`（默认 `None`）。`weight_failed` 留着不动。
- `code/voice-engine/voice-ffi/src/decoder.rs:96`：`WeightDecoder` 仍然跑那张图、仍然取到那一个下标，但**不再把词片当文本返回**：`text` 是空串，`failure` 记下 `当前解码器只能给一个 token（词表下标 {token}，词片 {piece:?}），不是可提交文本`。权重打不开或计算失败时写成 `定稿计算失败：{error}`。`tokens` 里仍然留着那个下标，只用于诊断，不进文本。
- `code/voice-engine/voice-ffi/src/engine.rs:339` 的 `Command::Finish` 分支：先取 `slot.decoder().failure_reason()`（`:346`），有原因就发一行 `ok:false`、把原因写进 `last_error`、`reply` 回 -1。会话不在引擎线程上时原因写成 `这个会话不在引擎线程上`。
- `code/voice-engine/voice-ffi/src/lib.rs:159`：`voice_engine_last_error(engine, utf8, n)`，返回写入字节数（不含结尾 NUL）；没有失败返回 0；指针为空或缓冲区不够返回 -1。
- `code/VoiceInterface/Sources/VoiceInterface/VoiceEngine.swift`：`send(.finish)` 见到非 0 就 `reject(sessionID, lastErrorReason() ?? "定稿未被接受")`；`send(.start)` 同理用 `会话未被接受` 兜底。原因经 `VoiceHandleGate` 的 `recognition rejected <message>`（`VoiceHandleGate.swift:85`）落进 `~/Library/Logs/RimeClient/voice.log`。
- `code/voice-engine/BUILD.txt` 增了「当前解码器只能给一个 token」一节，把这条限制、它为什么不能出句、以及现在的处理写在构建记录里。

### 2.2 实测

`/tmp/voice-abi/check_feed.c`：同样链同一份归档，`begin(7)`、喂 16000 个 440 Hz 的 `i16`（1 秒）、`finish(7)`，全程 1.697 秒（含引擎创建与权重读取；这两段各占多少：未知）。

```
begin 0
feed 0
json {"decoded":true,"kind":"final","ok":true,"session":7,"text":""}
json {"decoded":false,"message":"当前解码器只能给一个 token（词表下标 106074，词片 \"åĢŁéī´\"），不是可提交文本","ok":false,"session":7,"text":""}
finish -1
last_error 109 [当前解码器只能给一个 token（词表下标 106074，词片 "åĢŁéī´"），不是可提交文本]
freed
```

两条 JSON 的顺序是固定的：先一条空串定稿（会话确实没有文本），再一条 `ok:false` 带原因。两次运行（改 `decoded` 前后）拿到的下标都是 106074，同一个输入给出同一个下标。

`finish` 回 -1、JSON 行 `ok:false`、`last_error` 109 字节，三者带的是同一条字符串，所以门那边「同一句只写一次」的判据（`loggedRejection`）会把它们合成一行日志。

### 2.3 一次失败之后门怎么走

旧行为：`applyReply` 碰到 `ok == false` 只写一行日志就 `return`，会话停在「已请求收束但未关闭」，此后按键被取走却不再开新会话（048 第 4.3 节）。

现在：

- `code/VoiceInterface/Sources/VoiceInterface/Session.swift:116` 新增 `abandon(sessionID:host:)`：该会话仍是 `.open` 且 id 对得上时，先按老规矩判断要不要清预编辑（预编辑仍在且等于自己写的那一串才发一次空 `show`），再把 `life` 置 `.closed`。
- `VoiceHandleGate.swift:91`：`ok == false` 的回复先记日志，再 `machine.abandon(...)` 并执行它给出的效果。
- 单测 `testRejectedFinalLogsTheReasonAndLetsTheNextPressStartAgain`：按下、松开、收到 `ok:false`（原因里带那个词片）→ 日志恰好一行、不提交、不上屏；再按一下 → `capture.started == 2`、`"cmd":"start"` 出现两次。

一处既有单测因此改了预期：`testSameRejectionMessageIsLoggedOnce` 原先在被拒之后继续往同一个会话塞部分文本，并断言它会显示。按现在的契约，被拒的会话已经关闭，那一条部分文本被丢掉；这条单测改成先断言「不显示」，再按一下开新会话，断言「显示」。它要盯的「同一句只记一次」没有变。

另外：`metal` 解码器在部分步返回的文本就是空串，`Session::apply` 只在文本变化时发事件，所以这条路径现在根本不发部分事件——没有假的部分文本，也就没有假预编辑。空的定稿走 `acceptFinal("")`：只关会话，不 commit（`Session.swift:92`）。

## 3. 每步实测命令与结果

| # | 命令（工作目录） | 退出码 | 结果 |
| --- | --- | --- | --- |
| 1 | `cargo test --workspace`（`code/voice-engine`，不带 `metal`） | 0 | `voice-asr` 9、`voice-ffi` 13、`voice-tensor` 的 `shapes` 6，其余 0 条；无 warning |
| 2 | `cargo build -p voice-ffi --features metal --target aarch64-apple-darwin`（同上，带三个 MLX 环境变量） | 0 | 增量 0.10～1.2 秒；`libvoice_ffi.a` 281503888 字节；`nm -gU` 有 `_voice_engine_last_error`、`_voice_engine_set_reply_sink` |
| 3 | `swift test`（`code/VoiceInterface`） | 0 | 21 条 0 失败（`VoiceEngine` 3、`VoiceHandleGate` 6、`VoiceMachine` 11、`VoicePythonPath` 1） |
| 4 | `swift build --product Squirrel`（`code/Client`） | 0 | `Build complete`；`nm -gU` 里 8 条 `voice_*` 都是 `T`；`otool -L` 有 Metal.framework 与 Accelerate.framework |
| 5 | `swift test`（`code/Client`） | 0 | 7 条 0 失败 |
| 6 | `bash code/Client/scripts/make-app.sh`（仓库根） | 0 | `code/Client/dist/Squirrel.app`：主程序 39680272、`mlx.metallib` 183870464、`weights.safetensors` 537626967 字节 |
| 7 | `pkill -f "RimeClient.app/Contents/MacOS/Squirrel"` 三次，确认 `ps` 无该路径 | — | 先停掉 11-13 10:43:43 起的那个进程（pid 1051）；它被系统重启成 30369，一并停掉 |
| 8 | `ditto code/Client/dist/Squirrel.app "$HOME/Library/Input Methods/RimeClient.app"` | 0 | 包内主程序 md5 `427fccdc19d2654d5dab24eee3f23375`，与 dist 那份相同 |
| 9 | `stat`／`find -type l`／`codesign --verify --deep --strict`（已安装包） | 0 | 三个文件字节见上；模型目录符号链接 0；签名校验通过 |
| 10 | 链并按 1.3、2.2 跑 `/tmp/voice-abi/check`、`check_feed` | 0 | 输出见前两节 |

第 4 步第一次跑时 `swift build` 只回 `Build complete (0.78秒)`，而 `libvoice_ffi.a` 已经换过——为确认主程序真的重链过，删掉 `.build/out/Products/Debug/Squirrel` 再编，产物 39679968 字节、mtime 12:21:34，晚于归档的 12:21:15。之后才 `make-app.sh` 与 `ditto`，所以装进包里的那份含新归档。

第 4 步完整重链时 ld 会给一串「MLX 目标文件按 macOS 27.2.1 编译、Squirrel 部署目标 14.0」的警告（048 也记过）。增量这一次计数为 0，链接仍然完成。

## 4. 边界与仍未知

- 已装好的包我没有按一次右 Option，也没有说话。日志最后两行仍是 `2026-11-14T04:00:14Z`、`04:00:21Z` 的 `audio engine started`，那两行属于上一份二进制。所以「装好之后按下会怎样」这次没有测到。
- `voice_engine_last_error` 与 `voice_engine_set_reply_sink` 在已安装主程序里是已定义符号（`T`），`dlsym` 能找到；但「真实输入法进程里 JSON 行真的被 gate 收到」仍是未知，因为这次没有按键、也没有麦克风输入。
- 下一次定稿的 token 下标：未知。这次 106074 来自 1 秒 440 Hz 正弦，不是人声。
- 一次 `finish` 里权重读取、反量化与那张伪图各占多少：未知。只测到 1.697 秒的总时长，含引擎创建。
- 麦克风到文字的识别时延：未知，也仍然做不到——解码器只能给一个 token。
- 那条原因在日志里长这个样子（下次有人按右 Option 时可以直接 grep）：

  ```
  recognition rejected 当前解码器只能给一个 token（词表下标 …，词片 "…"），不是可提交文本
  ```

## 检索

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | 读 048 | `dev-docs/notes/048-research-voice-path-self-check.md` | 第一处确定断点在第 4 段（解码），第 3 段是形状不同形 |
| 2 | 读码 | `voice-ffi/{lib,engine,decoder}.rs`、`include/voice_engine.h` | 回复只有 C 函数指针；`transcribe_finish -> Result<u32>`；`tokens: vec![token]` |
| 3 | 读码 | `VoiceInterface/Sources/VoiceInterface/{VoiceEngine,VoiceHandleGate,Session,WorkerProtocol}.swift` | 解析器只读六个键；拒绝只记一行；`acceptFinal("")` 不 commit |
| 4 | 写码 | `voice-ffi/src/json_line.rs`、`engine.rs`、`lib.rs`、`decoder.rs`、头文件 | 一行 JSON 的回复口、`last_error`、单 token 记为失败 |
| 5 | 写码 | `VoiceEngine.swift`、`VoiceHandleGate.swift`、`Session.swift`、`Client/Package.swift` | JSON 口与解析器对接；拒绝即关会话；两个 `-u` 拉住新符号 |
| 6 | `cargo test --workspace` | `code/voice-engine` | exit 0；9／13／6 |
| 7 | `cargo build -p voice-ffi --features metal --target aarch64-apple-darwin` | 同上 | exit 0；归档 281503888 字节 |
| 8 | `swift test` | `code/VoiceInterface` | exit 0；21 条 0 失败 |
| 9 | `swift build --product Squirrel`、`swift test` | `code/Client` | exit 0；7 条 0 失败 |
| 10 | `make-app.sh`、`ditto`、`stat`、`codesign` | `code/Client/dist`、`~/Library/Input Methods/RimeClient.app` | 见第 3 节的 6～9 行 |
| 11 | `nm -gU`、`otool -L` | 已安装 `Contents/MacOS/Squirrel` | 8 条 `voice_*` 为 `T`；有 Metal.framework |
| 12 | 临时程序（`/tmp/voice-abi`） | 已安装包内模型目录与 metallib | 空会话：JSON 口收到 `final` 且文本指针未被调用；1 秒正弦：`finish` -1 且原因可达 |
| 13 | `tail` | `~/Library/Logs/RimeClient/voice.log` | 仍是上一份二进制留下的两行 `audio engine started` |

<!-- research-complete -->
