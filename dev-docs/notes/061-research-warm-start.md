# 061-research-warm-start：引擎挪到启动预热，预热完的首键从卡 4.8 秒变成 0.46 秒

测量日：2026-11-14。对象是 `code/VoiceInterface`（新增 `VoiceEngineWarmup.swift`）、
`code/Client/sources/Main.swift`、`code/Client/sources/SquirrelInputController.swift`。
探针仍是 `code/voice-oracle/assets/probe_pcm.bin`（2.00 秒、32000 样本），模型目录与
metallib 取自已安装包 `~/Library/Input Methods/RimeClient.app/Contents/Resources`
（`Models/Qwen3-ASR-0.6B-4bit` 与 `mlx.metallib`）。

这一步没有改任何算术：引擎建得跟以前一样快（一样慢），变的是**它什么时候建**、
**建的时候谁在等**。所以逐层闸的实测差值与 058/060 逐位相同，token 与文本也一样。

## 0. 结论速览

| 项 | 改前 | 改后 |
| --- | --- | --- |
| 引擎什么时候建 | 第一次 `handle` 里，主线程现场建（`ensureVoiceGate`） | `IMKServer` 起来之后、`app.run()` 之前，后台队列 |
| 首键（预热已完成） | 不存在这种情形 | `handle` 返回 **0.003 秒**，按下→定稿 **0.45–0.46 秒** |
| 首键（进程起 0.00 秒就按，预热中） | 主线程卡 **4.79–4.82 秒**，**5.25–5.30 秒**出定稿 | 主线程卡 **≤2.00 秒**（上限）后拒，不出定稿；预热完成后下一次按键 0.56–0.62 秒出字 |
| `voice_engine_new` 墙钟 | 4.79–4.82 秒（探针）/ 4.82–5.47 秒（`bench2`） | 同左：没有变快，只是换了发生时刻 |
| token / 文本 | `[106287, 1773, 151645]` / `嗯。` | 同左 |
| 逐层闸（打开 metal 三套） | 13 通过、0 失败 | 13 通过、0 失败，实测差值逐位相同 |
| `cargo test --workspace --offline` | 49 通过、4 忽略、0 失败 | 49 通过、4 忽略、0 失败 |
| 单测 | VoiceInterface 21 / Client 7 | **VoiceInterface 27 / Client 9** |

一句话：**首键不再把输入法主线程按在 5 秒里**。预热跑完再按，首键是 0.46 秒；
按在预热窗口里，最坏是卡 2 秒然后收尾，不再无声挂住，下一次按键照常出字。

## 1. 改动点

| 位置 | 改动 |
| --- | --- |
| `code/VoiceInterface/Sources/VoiceInterface/VoiceEngineWarmup.swift` | **新增**（234 行）。`VoiceEngineWarmup`：状态机（`idle` / `warming` / `ready` / `failed(原因)`）+ 后台建 + 有上限的等待 + 可换的回复落点；`VoiceEnginePreheatedTransport`：按键那一侧的传输壳 |
| `code/Client/sources/Main.swift:129-135` | `IMKServer(...)` 建完之后、`NSApplication.shared` 与 `app.run()` 之前调 `VoiceEngineWarmup.shared.start(modelDirectory:metallibPath:)`。`start` 自己开后台队列（`voice.engine.warmup`，`qos: .userInitiated`），失败只写 `voice.log` |
| `code/Client/sources/SquirrelInputController.swift:596-641` | `ensureVoiceGate` 不再自己建引擎：预热走过（`status != .idle`）就装 `warmup.route` + `VoiceEnginePreheatedTransport`；只有预热根本没开始（单测、别的宿主）才退回原来的懒建 |
| `code/VoiceInterface/Tests/VoiceInterfaceTests/VoiceEngineWarmupTests.swift` | **新增** 6 条：`idle` 状态、只建一次、失败留原因、预热中按上限拒且 `.feed` 一秒不等、就绪后不再等、落点可换 |
| `code/Client/Tests/ClientTests/VoiceInputControllerTests.swift` | 新增 2 条源码断言：预热点确实在 `IMKServer` 之后、`app.run()` 之前；`ensureVoiceGate` 里确实复用预热、且没有 `Thread.sleep` / `.wait()` |

`code/VoiceInterface/Sources/VoiceInterface/VoiceEngine.swift` 一行未改：预热用的仍是同一个
`VoiceEngine(modelDirectory:metallibPath:onReply:)`，只是调用点从按键挪到启动。

## 2. 预热的形状

- **只在启动序列里建一次。** 第二次 `start` 返回 `false`，不建第二个引擎——权重两份会把
  峰值内存再抬一份（057 记的峰值 2.92 GB）。
- **回复落点后装。** 引擎在预热时就建好了，`route` 可以在之后把落点换成门的 bridge；
  引擎线程给的每一行都先落到「最后一次 `route` 进来的闭包」上。`forward` 只在取落点时
  拿一下锁，回调本身在锁外。
- **按键的等待有上限。** `VoiceEngineWarmup.keyPressBudget = 2.0` 秒。`.start` 在有上限
  之内等，等到就直接交给引擎；等到上限还没好，就发一行 `ok:false`，`VoiceHandleGate` 那边
  按既有路径 `abandon` 收尾（`voice.log` 两行：`首键拒绝 …`、`recognition rejected …`）。
- **`.feed` / `.finish` 一次也不等。** 预热没好之前直接丢掉；那次会话已经由那行拒绝收尾，
  不会把一串音频帧排在那里等。
- **门照旧缓存。** `voiceGate` 仍是懒建一次、之后一直用；但传输壳自带「有就交、没有就拒」，
  所以预热完成后同一个门就恢复正常，不需要重建（`warm-retry` 那条实测就是这个）。
- **失败只记日志。** 引擎给不出原因（未链入、模型目录打不开）时状态是 `failed(原因)`，
  `voice.log` 一行 `预热 预热失败（原因）`，`handle` 那条路拿到的是同一个引擎，它自己回
  一行原因——与改前「首次按键才报错」同一个文案，只是时刻提前到启动之后几秒。

## 3. 实测

### 3.1 `voice_engine_new` 墙钟（`/tmp/voice-abi/bench2`，三次）

```
engine_new 5.470 s   cold finish 0.926 s   warm finish 0.466 / 0.464 s
engine_new 4.826 s   cold finish 0.790 s   warm finish 0.460 / 0.461 s
engine_new 4.819 s   cold finish 0.528 s   warm finish 0.463 / 0.464 s
```

中位 **4.826 秒**（含常驻权重）。这一位就是预热要等的时间，跟 057 记的 4.908 秒、
059 记的 4.735–4.958 秒同一量级：没有变快。

### 3.2 首键墙钟（新写的探针 `/tmp/voice-abi/keyprobe`，不在 `code/` 内）

探针与输入法同一条路：真的 `VoiceHandleGate` + 真的 `VoiceEngine` / 预热传输 + 一个假的
`PCMSource`（把 2 秒探针 PCM 按 4096 样本一帧喂进去）。四种模式，各跑三次：

| 模式 | 按键时刻 | `handle` 卡多久 | 按下→定稿 | 进程起→定稿 | 文本 |
| --- | --- | ---: | ---: | ---: | --- |
| `lazy`（改前：handle 里现场建） | 进程起 0.000 | **4.82 / 4.81 / 4.79** | 5.28 / 5.25 / 5.30 | 5.28 / 5.25 / 5.30 | 嗯。 |
| `warm-ready`（预热完再按） | 进程起 4.80 / 4.76 / 4.77 | **0.003** | **0.46 / 0.45 / 0.46** | 5.25 / 5.21 / 5.23 | 嗯。 |
| `warm-now`（预热中立刻按） | 进程起 0.000 | **2.004 / 2.012 / 2.011** | 无（拒） | 拒于 2.005 / 2.012 / 2.012 | 无 |
| `warm-retry`（先被拒，预热完再按） | 第一次 0.000，第二次预热完 | 第一次 2.01，第二次 0.003 | 第二次 **0.56 / 0.62** | 5.61 / 5.45 | 嗯。 |

三件事要摆清楚：

1. **「进程起→定稿」这个数几乎没有变好**：5.25–5.30 → 5.21–5.25。因为 `engine_new`
   本身要 4.8 秒，这段一秒也省不掉，零点在进程起时它必然在账上。变好的是**按键那一下
   等多久**（4.8 秒 → 0.003 秒）与**有没有把主线程按住**。
2. **按键落在预热窗口里，这一次的语音是丢的。** 2.0 秒上限一到就拒，`voice.log` 留两行；
   用户重新按一次就正常（`warm-retry` 实测 0.56–0.62 秒出字，token 与文本仍对）。
   这一位不是「变好了」，只是「不再无声挂住」。
3. `warm-ready` 的 0.455 秒比改前同进程第二次的 0.460–0.466 秒（本次 `bench2` 的两次 warm）略快，也没有更好：
   预热唤醒的那次定稿仍是「这个进程的第一次」，GPU 侧冷的成分还在，实测落在 0.45–0.62
   秒的区间里。

`voice.log` 里现在的样子（节选自上面几次跑）：

```
… 预热 预热完成 引擎 4.80 秒
… 首键拒绝 引擎未就绪（等了 2.00 秒，预热中）
… recognition rejected 引擎未就绪（等了 2.00 秒，预热中）
```

### 3.3 逐层闸与单测

不打开 metal：`cargo test --workspace --offline` → **49 通过、4 忽略、0 失败**（与 060 同一笔账）。

打开 metal 三套一次跑完（参数见 `code/voice-engine/BUILD.txt`）：

- 解码器 6 条：主闸 7.8125e-2 对容差 8.203125e-2（7.6/8 个单位，余量 1.05 倍，
  argmax 106287）；注入参考编码器特征那条 6.25e-2（6.1/8 个单位，余量 1.31 倍）；
  两条归因断言仍是 2.5214844e1 / 2.1436523e1；四口径分组对照 7.8125e-2 / 6.25e-2 /
  3.125e-2 / 4.6875e-2，argmax 全为 106287。
- 编码器 3 条：`conv_stem_out` 差 0、`encoder_layer0_out` 3.90625e-3（余量 3.32 倍）、
  `encoder_proj_out` 1.1855885e-3（19.2/32 个单位，余量 1.67 倍）。
- 端到端 4 条：提示 44 个 id 一致、贪心 `[106287, 1773, 151645]` 一致、文本 `嗯。`、
  缓存第二步 1773；一次 `transcribe` 墙钟 5.30 秒。

**13 通过、0 失败**，每个数字都与 060 逐位相同——这一步没有碰容差，也没有碰算术。

Swift 侧：`code/VoiceInterface` 21 → **27 通过**（多 6 条预热单测）；
`code/Client` 7 → **9 通过**（多 2 条启动接线断言，其中一条从 `IMKServer` 到 `app.run()`
逐段比下标，确认预热点夹在中间）。`swift build --product Squirrel` exit 0。

## 4. 代价与没变好的地方

- **预热是「提前付费」，不是「少付费」。** 4.8 秒的 `engine_new` 与 0.85 GB 的常驻峰值
  （057：峰值内存 2.92 GB，基线 2.07 GB）从「第一次按键」挪到「进程起来之后几秒」。
  不用语音的人这一次也要付。
- **预热窗口里的那一次按键是丢的**，只留日志。这就是 2.0 秒上限买来的东西：不挂住主线程。
- **上限取 2.0 秒是权衡，不是最优解。** 调大（比如 6 秒）等于回到改前那种卡法，
  只是卡的时长变了；调小（比如 0.5 秒）会让预热尾巴上按的那几次都白按。
  常量在 `VoiceEngineWarmup.keyPressBudget` 一处，改一个数就能重测。
- **没做「排队回放」。** 按键落在预热窗口里时，把 `.start`/`.feed`/`.finish` 缓冲起来，
  预热完按原序灌进去，语音就不丢了。没做是因为它的收益算得出来：`warm-now` 那种按法最多
  省下 0.13 秒（5.43 → 5.30，因为权重读取那段跑不掉），换来的是新的失败面
  （预热失败要清队列、缓冲要设上限、回放与直发要互斥）。这条留在这里当备选，不是遗漏。
- **没到真实输入法进程里按键验证。** 不调 inject、不动已安装包，所以「App 真起来之后按
  一次右 Option」这一步没有按键事件可发；替代证据是三条：`Main.swift` 的接线有源码断言
  卡住位置、预热本身在探针里用同一份模型跑出 `预热 预热完成 引擎 4.80 秒`、
  `voice.log` 里那行确实由后台队列写出。

## 5. 跑了什么

```
cd code/VoiceInterface && swift test          # 27 通过
cd code/Client && swift build --product Squirrel && swift test   # 9 通过
cd code/voice-engine && cargo test --workspace --offline         # 49 通过 / 4 忽略

# 打开 metal（RUSTFLAGS 六条 -U 与 BUILD.txt 一致）
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test encoder_parity --test decoder_parity --test end_to_end_parity \
  -- --test-threads=1 --nocapture            # 13 通过 / 0 失败

/tmp/voice-abi/bench2 <模型目录> <metallib> probe_pcm.bin              # engine_new 三次
/tmp/voice-abi/keyprobe/.build/release/keyprobe lazy|warm-now|warm-ready|warm-retry \
  <模型目录> <metallib> probe_pcm.bin                                  # 各三次
```

<!-- research-complete -->
