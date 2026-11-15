# 080-research-session-collision-fix：换窗口之后语音失效的定位、两处改动与验收

本文记的是「切换窗口之后语音失效」这条的原因、落在哪两处、怎么验到「三枚新的门各自 begin
引擎都接受」，以及**真机上切换窗口这一步没有覆盖**这件事的实情。

这一阶段没有改产品代码：`code/voice-engine` 与 `code/VoiceInterface` 的产品路径都是 078 / 079
留下的形状，本次只加了两条验证用例（Swift 一条、Rust 一条）与一个 C 探针，然后重编归档与
Swift 产物、重打包、装进 `~/Library/Input Methods/RimeClient.app`。

没有改 `dev-docs/refs`，没有调用 inject，没有改 librime。

## 1. 原因回顾

两条寿命对不上：

| | 活多久 | 号在哪 |
| --- | --- | --- |
| `VoiceHandleGate` / `VoiceMachine` | 跟着 `IMKInputController` 实例 | 078 之前是机器自己从 1 数 |
| 引擎的会话表（`voice-ffi/src/engine.rs` 的 `sessions: HashMap<u64, Session<...>>`） | 跟着引擎进程（预热之后全进程一份） | 号是 key |

换窗口、换输入源之后 `IMKInputController` 是新实例，新门里的 `nextID` 从 1 重新计数；
而 075 的取消（失焦、看门狗）只停采集、**故意不发 `finish`**，引擎里那枚会话一直挂着占号。
新门发 `begin(1)` 撞上还挂着的 1，引擎在 `Command::Begin` 里直接 `reply.send(-1)`
（078 记下的 `engine.rs:338`），Swift 侧把它翻成「会话未被接受」，界面上就是按了没反应。
`voice.log` 里号从 5、6 回到 1 就是这两份寿命各自计数的样子。

## 2. 两处改动（078 + 079 定下的形状）

**发号改成进程一份。** `VoiceSessionNumbers` 是 `NSLock` 护住的一枚 `UInt64`，
`VoiceMachine.press()` 走 `numbers.next()` 取号，`VoiceHandleGate` 把 `numbers`（默认
`.shared`）透传给内部机器，并给出只读的 `sessionID`。还有什么比门活得久、引擎进程一重启
会话表就空、为什么不是每门自增也不是随机大数，都在 078 里，本文不重述。

**取消真的把引擎那一枚收掉，撞号也有兜底。** `VoiceHandleGate.abandon(reason:)` 在
`capture.stop()` 之后发 `{"cmd":"discard","session":N}`；Rust 侧新增
`voice_session_discard`（C ABI 第 10 枚声明）与 `Command::Discard`，只从表里取走并释放，
有没有那枚都回 0。另外 `Command::Begin` 撞上同号会话时不再 `-1`：先把旧会话连同解码器
`drop`，再按这次的热词建新的，并从回复口发一行 `kind="replaced"`。细节与既有 ABI 用例的
逐条对照在 079。

**会话号写进日志。** 五种行都以号开头，撞号时按号对齐就能把「谁开、谁关、谁被拒、谁被替换」
串起来：

| 时机 | 行 |
| --- | --- |
| 新会话 begin | `voice session 7 begin bundle=com.apple.Safari` |
| 外部取消（失焦、看门狗） | `voice session 7 abandoned deactivate` / `… watchdog` |
| 定稿落到当前会话 | `voice session 7 final` |
| 引擎拒了这一路 | `voice session 7 rejected 会话未被接受` |
| 同号旧会话被替换 | `voice session 7 replaced 同号旧会话已丢弃` |

## 3. 验证结果

### 3.1 三套套件与打开 metal 的三套闸

```text
cd code/VoiceInterface && swift test                 → 44 通过、0 失败（078/079 的 43 + 本次 1）
cd code/Client        && swift test                  →  9 通过、0 失败
cd code/voice-engine  && cargo test --workspace --offline
  voice-asr 10、voice-ffi 17、voice-mel 12、voice-parity 7+4（4 条 #[ignore]）、
  voice-tensor 6、voice-tokenizer 4+5
                                                     → 65 通过、4 忽略、0 失败

source /tmp/metal-env.sh   # SDKROOT / LIBRARY_PATH / MLX_SYS_SYSTEM_SDK / 八个 -U 符号
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test encoder_parity --test decoder_parity --test end_to_end_parity -- --test-threads=1
  encoder_parity 3、decoder_parity 6、end_to_end_parity 4  → 13 通过、0 失败
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline --lib
  → 5 通过、1 忽略、0 失败
```

没有一条既有断言被放宽，也没有新增 `#[ignore]`。三套闸的通过条数与 070 记的 3/6/4 相同；
容差一个没动，本次没有触碰数值路径。

### 3.2 「三枚新的门各自 begin」的可复现验证

分三层，各证一半：

**Swift（号不重复）** `VoiceSessionNumberTests.testThreeFreshGatesTakeThreeDistinctNumbers`：
连着建三枚门（三个 `IMKInputController` 实例），各按一次右 Option，断言三个号互不相同、
严格递增、每一枚都按自己的号发 `start`，日志逐行等于三条 `begin`——没有 `rejected`。

**Rust（引擎三次都接受）** `voice-ffi` 的
`three_fresh_sessions_begin_in_a_row_are_all_accepted`：走 `Worker::spawn` 与带标签的解码器，
`begin(1)` / `begin(2)` / `begin(3)` 的返回码断言为 `[0, 0, 0]`；三枚会话各自 `feed` 一步、
各自 `finish` 回 0；每一步都带着自己的热词，定稿之后三枚解码器全部释放。

**C 探针（真归档、真引擎线程、真会话表）** `/tmp/voice-abi/session_probe.c`，用
`/tmp/voice-abi/link.sh` 链新归档（打开 metal），模型与 metallib 取装好的包里的那一份。
跑出来的实际返回码：

```text
A. 三枚新的 VoiceMachine 各自 begin（共享计数器发 1、2、3）
    | {"decoded":true,"kind":"final","ok":true,"session":1,"text":"嗯。"}
  session 1: begin=0 feed=0 finish=0
    | {"decoded":true,"kind":"final","ok":true,"session":2,"text":"嗯。"}
  session 2: begin=0 feed=0 finish=0
    | {"decoded":true,"kind":"final","ok":true,"session":3,"text":"嗯。"}
  session 3: begin=0 feed=0 finish=0
B. 同一个号 begin 两次（修复前的撞号现场）
    | {"decoded":false,"kind":"replaced","message":"同号旧会话已丢弃","ok":true,"session":9,"text":""}
    | {"decoded":true,"kind":"final","ok":true,"session":9,"text":"嗯。"}
  session 9: begin#1=0 begin#2=0 feed=0 finish=0
C. 取消那条路：discard 之后同一个号还能开
  session 12: begin=0 discard=0 begin_again=0
```

三枚号各一次 `begin=0`；同一个号连着 `begin` 两次是 `0 / 0`（替换语义，另有一行 `replaced`）；
`discard(12)` 回 0 之后 `begin(12)` 又回 0。**整段没有一条 -1。**

### 3.3 重编、重打包、装包与签名

```text
cd code/voice-engine && source /tmp/metal-env.sh
cargo build -p voice-ffi --features metal --target aarch64-apple-darwin
  → 归档 target/aarch64-apple-darwin/debug/libvoice_ffi.a，285 773 512 字节
  → nm -gU 见到 _voice_session_begin / _feed / _finish / _discard

cd code/Client && rm -f .build/out/Products/Debug/Squirrel && swift build --product Squirrel
  → Build complete!（1.20 秒），产物 40 754 096 字节；只有既存的
    「归档按 macOS 27.2 编、链到 14.0」的 ld 警告

bash code/Client/scripts/make-app.sh
  → code/Client/dist/Squirrel.app（八条 dylib 就地重签）
bash code/scripts/package-release.sh
  → dist/RimeClient-20261115.tar.zst，9 271 916 字节
    sha256 0c3a363468f332a8c6d28a4e7a379e5807e9ec41b1b224e992191a15239a07d9
bash code/scripts/install-app.sh --assets code/assets
  → 停止旧进程、ditto、补权重与 metallib、重签、资产哈希与包内清单一致
```

装好之后单独复核：

```text
codesign --verify --deep --strict "$HOME/Library/Input Methods/RimeClient.app"   → exit 0
codesign -dv → Identifier=im.dragonshock.inputmethod.RimeClient, Format=app bundle, adhoc,
               Sealed Resources version=2 rules=13 files=22
nm …/Contents/MacOS/Squirrel | grep voice_session
  → _voice_session_begin / _voice_session_discard / _voice_session_feed / _voice_session_finish
可执行文件 40 754 400 字节（比构建产物多 304 字节，是 make-app 的 install_name_tool 改写）
```

## 4. 真机上切换窗口：**没有覆盖**

如实记：这一条没有在真机上跑过。

- 这台机器的已启用输入源里没有本输入法：`defaults read com.apple.HIToolbox
  AppleEnabledInputSources` 里查不到 `im.dragonshock.inputmethod.RimeClient`，当前输入源是
  `com.apple.keylayout.ABC`。要按出一次真实的右 Option 得先启一条输入源、再在一个有文本框的
  窗口里按住说话，这两步都要人来做，我没有去改用户的输入源列表。
- 也没有用合成按键去代替：合成的 `flagsChanged` 能不能被 `IMKInputController` 收到取决于
  辅助功能授权与当前焦点，就算收到了也没有麦克风里的语音，得到的证据比上面三层替身更弱。
- 所以「换窗口之后第一句不再被拒」目前由三层证据支持：Swift 侧号不重复（真实的门、
  假的传输）、引擎线程三次 `begin` 都回 0（真的引擎线程、假的解码器）、真归档上一号重开与
  三个号连开都回 0（C 探针）。**剩下的那一步——真实 `IMKInputController` 换实例——没有走到。**

要补这一步：启用输入源 → 在窗口 A 按住右 Option 说一句 → 切到窗口 B 再说一句 → 看
`~/Library/Logs/RimeClient/voice.log`。成功的样子是两行号不同的 `begin`、各自一行 `final`，
**没有** `rejected 会话未被接受`，也**没有** `replaced`。

## 5. 还没定的

- **引擎侧仍然不会自己回收静默会话。** 现在靠取消时的 `discard` 收场；若某一枚客户端不发
  `discard`（更老的包、或取消路径之外的异常退出），撞号仍会落到 `replaced` 这条兜底上，
  旧会话的内存要到被替换那一刻才释放。回收条件与代价没量过。
- **单测会往真实 `voice.log` 写行。** `VoiceLog.sink` 为 nil 时直接追加
  `~/Library/Logs/RimeClient/voice.log`，而只有一部分用例换了 sink。于是上午 03:47–03:51
  那几段「号又回到 1」的行其实是当天测试跑出来的，不是输入法自己的。这不影响本次结论
  （每行都带号与 bundle，真机行与测试行能靠时间戳与 `bundle=-` 分开），但下次按日志判撞号
  时先看这两处，别把测试行当成现场。
- **多枚输入控制器各一枚门与采集**仍在「进行中」：这次让号全进程唯一之后，两枚门同时说话
  不会再互撞号，但两个采集源并行时的内存与时延仍没有量过。
