# 066-research-staged-load：装载分两段，第二段跟说话重叠

测量日 2026-11-15。改动只在 `code/`：`voice-mlx/src/backend/transcribe.rs`（`Engine` 分两段）、
`voice-ffi/src/{decoder,engine,backend,lib}.rs`（第二段在 `begin` 之后那一步装上、`finish`
等它就绪），新增 `voice-mlx/tests/staged_load.rs`（metal 侧两段测试）与 `voice-ffi` 一条时序
测试。层数、位宽、组大小、算术语序、容差口径、梅尔前端、分词器、`lm_head` 分块 argmax 一行
没动；`dev-docs/refs` 没动，librime 没动，`inject` 没调。

## 0. 结论速览

| 项 | 改前 | 改后 |
| --- | --- | --- |
| `voice_engine_new`（只装第一段） | 0.740–0.813 s（十二次中位 **0.756**） | 0.128–0.150 s（十六次中位 **0.136**；页缓存冷的两发 0.198 / 0.389） |
| 进程起→可以接受按键（`keyprobe`，真实预热 + 真实门） | 0.749–0.770 s | **0.128–0.140 s** |
| 按下→出字（2 秒说话，4096 样本一块实时送） | 2.266–2.289 s | 2.268–2.376 s |
| 进程起→出字（同上） | 3.006–3.118 s | **2.401–2.513 s** |
| 按下→出字（0.3 秒极短） | 0.488–0.492 s | 0.792–0.812 s |
| 进程起→出字（0.3 秒极短） | 1.233–1.273 s | **0.926–0.950 s** |
| 按下→出字（2 秒一次喂完、立刻松手） | 0.233–0.255 s | 0.831–0.848 s |
| 进程起→出字（同上） | 0.994–1.022 s | **0.960–0.979 s** |
| 按下→出字（同进程第二个会话，第二段已装好） | 0.203–0.206 s | 0.202–0.211 s |
| 第一段（梅尔 + 音频塔） | — | **0.106–0.225 s**（四发中位 0.13） |
| 第二段（分词器 + 文本塔） | — | **0.627–0.847 s**（中位 0.637；分词器 0.42–0.43 是主体） |
| token | `[106287, 1773, 151645]` | `[106287, 1773, 151645]` |
| 文本 | `嗯。` | `嗯。` |
| 解码器主闸首 64 维最大差 | 7.8125e-2（容差 8.203125e-2） | 逐位相同 |
| 峰值最大 RSS（`/usr/bin/time -l`） | 709 574 656–713 785 344 字节 | 710 033 408–714 391 552 字节（差在噪声内） |
| `cargo test --workspace --offline` | 53 通过、4 忽略、0 失败 | **54 通过**、4 忽略、0 失败 |

一句话：**`voice_engine_new` 现在只装音频塔与梅尔（中位 0.756 → 0.136 秒），文本塔与分词器
（合 0.643 秒）在 `begin` 的回发出之后才装，跟用户说话重叠。** 说话满 2 秒的那一档，
按下→出字仍是被说话本身钉住的 2.27 秒（改前 2.28）；变好的是「进程起」这一侧：进程起→
可以接受按键 0.75 → 0.13–0.14 秒，进程起→出字 3.01–3.12 → 2.40–2.51 秒。极短那一档
（0.3 秒、或按下就松手）按下→出字反而慢了 0.3–0.6 秒，因为第二段现在要在这个会话里等
完才定稿——这一条在 §5 单列，不是漏掉。token 与文本逐位仍是基线。

## 1. 两段的分界与量具

### 1.1 两段各是什么

| 段 | 内容 | 什么时候装 |
| --- | --- | --- |
| 第一段 | `MelFrontend::new()`（滤波器组与旋转因子表）、`weights.safetensors` 表头、`audio_tower.*` 523 张张量（202 118 144 字节，8 位分组 + 稠密）、收尾一次 `eval` | `voice_engine_new` 里，引擎线程起来的时候 |
| 第二段 | 分词器（`vocab.json` + `merges.txt`）、`model.layers.*` 704 张张量（335 372 288 字节，4 位分组 + 稠密）、`model.norm`、`model.embed_tokens`（也就是 `lm_head`）、收尾一次 `eval` | 引擎线程处理 `Command::Begin` 时，`begin` 的回**已经发出之后** |

第二段用的还是第一段打开的那份 `SafeWeights`：文件不重开、表头不重解，装完就把句柄与表头
一起放开。两段各一次 `eval`（065 是两塔共用一次；现在两段之间隔着一段用户说话的时间，
合不到一起）。

### 1.2 量具

- **`/tmp/voice-abi/bench6.c`（新增变体，不在 `code/` 内）**：一个钟量到底。`main` 一进来
  就记 `t=0`（进程起），之后依次打 `engine_new` 返回、`按下`（`begin` 之前）、`松手`
  （最后一个 `feed` 之后）、`finish` 返回、定稿回调到达。四种口径：
  `press`（2 秒音频按 4096 样本一块实时送，与麦克风同形）、`short`（只送 0.3 秒）、
  `instant`（整段一次喂完、紧接着 `finish`，即「按下就松手」）、`warm`（同 `instant` 连跑
  两个会话，看第二段是不是只付一次）。`bench3.c` 只量 `finish` 那一小段（`feed` 与 `begin` 都在计时之外），
  量不出「按下→出字」，所以另写这一只。
- **`/tmp/voice-abi/keyprobe`（061 就有的 Swift 探针，重编）**：走真实 `VoiceEngineWarmup`
  + 真实 `VoiceHandleGate` + 假 `PCMSource`，量「进程起→预热完成（= 可以接受按键）」与
  「按下→定稿」。`warm-ready` 模式：预热完再按，8 帧 4096 样本一次灌完。
- **`voice-mlx/tests/staged_load.rs`（新增）**：Rust 侧量两段各自的墙钟，并把第二段的值当闸
  用：只装第一段时定稿必须报「文本塔还没装好」（不返回空串），装完第二段后 token 与文本必须
  与一次装完逐位相同、与参考贪心序列逐位相同。
- **`voice-ffi` 的 `the_second_stage_runs_after_begin_and_again_before_finish`（新增）**：
  数第二段被叫了几次——`begin` 之后至少一次、`finish` 之前又多一次。这条不依赖 `metal`。

### 1.3 两份归档的来源（前后各自只用一份）

| | 归档 | 链接出的探针 |
| --- | --- | --- |
| 改后 | `/tmp/voice-abi/libvoice_ffi-after.a`（15 Nov 06:54，285 348 856 字节，md5 `5160b33e5d39c79a1447c1659fe14159`，与仓里 `target/aarch64-apple-darwin/debug/libvoice_ffi.a` 同一份字节） | `/tmp/voice-abi/bench6`（06:54）、`/tmp/voice-abi/keyprobe/.build/release/keyprobe`（06:55） |
| 改前 | `/tmp/voice-abi/libvoice_ffi-before.a`（15 Nov 06:43，285 334 176 字节）：当前树的一份副本，**唯一差别**是 `decoder.rs` 的 `boot_resident` 把 `Engine::open_audio` 换成 `Engine::open`——两塔仍在 `voice_engine_new` 里装完，末尾一次 `eval`，也就是 065 的装载形状 | `/tmp/voice-abi/bench6-before`（06:43）、`/tmp/voice-abi/keyprobe-before/release/keyprobe`（06:52） |

两只探针、两份归档用的是同一个 `bench6.c` / 同一份 `keyprobe` 源码、同一探针
（`code/voice-oracle/assets/probe_pcm.bin`，2.00 秒 32000 样本）、同一模型目录
（`code/assets/qwen3-asr-0.6b-4bit`）、同一份 `mlx.metallib`（已安装包里的那份）。改前那份
归档的 `engine_new` 十二次落在 0.740–0.813，与 065 记的 0.733–0.746 同一档，说明这份副本确实
就是改前的形状。

## 2. 两个阶段各占多少

`staged_load` 那条（一个进程里依次量：第一段、第二段、再一次装完），`dev` 档案，四次：

```
第一段 0.152 s｜第二段 0.643 s｜两段和 0.795 s｜一次装完 0.736 s    （拆开：梅尔 0.001、音频塔 0.151；分词器 0.429、文本塔 0.214）
第一段 0.106 s｜第二段 0.630 s｜两段和 0.736 s｜一次装完 0.854 s    （梅尔 0.001、音频塔 0.105；分词器 0.429、文本塔 0.201）
第一段 0.114 s｜第二段 0.627 s｜两段和 0.740 s｜一次装完 0.719 s    （梅尔 0.001、音频塔 0.113；分词器 0.425、文本塔 0.202）
第一段 0.225 s｜第二段 0.847 s｜两段和 1.072 s｜一次装完 0.968 s    （梅尔 0.001、音频塔 0.224；分词器 0.421、文本塔 0.427）
分段装完的定稿 0.31–0.33 s：文本 "嗯。" tokens [106287, 1773, 151645]
```

三点要摆清楚：

1. **第一段 0.106–0.152 秒（四发里三发；0.225 那发是刚编完的第一发）**，其中梅尔前端
   0.001 秒（纯主机表），音频塔 0.105–0.151 秒（含表头 0.0013 秒与 523 张张量 202 MB 的
   读取、建数组、一次 `eval`）。这一位就是「进程起→可以接受按键」的下界。
2. **第二段 0.63 秒里，0.42–0.43 秒是分词器**，不是张量。`Tokenizer::load` 解
   `vocab.json`（约 2.7 MB JSON）与 `merges.txt` 并建合并表；这段是纯主机活，跟 GPU 无关。
   文本塔那 704 张张量（335 MB）只要 0.20–0.21 秒。所以把分词器留在第一段会让「可以接受
   按键」从 0.13 秒回到 0.54 秒（那一版实测 0.541–0.556），这一档的取舍写在 §5。
3. 两段和（0.736–1.072）与「一次装完」（0.719–0.968）两个带子**互相压着**，差异是页缓存
   而不是分段：`一次装完` 那一发在同一个进程里跑第二遍，537 MB 权重已经在缓存里；两段那一发
   是第一遍。测这四位要在同一个进程里连着量，但两段的墙钟只能从「第一遍」取。

## 3. 总墙钟前后

### 3.1 `bench6`：四种口径各三次（同一份探针，`dev` 档案）

```
改前（bench6-before）
press   engine_new 0.813 / 0.754 / 0.750  按下→出字 2.280 / 2.273 / 2.289  进程起→出字 3.093 / 3.027 / 3.039
short   engine_new 0.759 / 0.754 / 0.742  按下→出字 0.492 / 0.488 / 0.491  进程起→出字 1.251 / 1.242 / 1.233
instant engine_new 0.759 / 0.767 / 0.761  按下→出字 0.254 / 0.255 / 0.233  进程起→出字 1.013 / 1.022 / 0.994

改后（bench6）
press   engine_new 0.150 / 0.136 / 0.133  按下→出字 2.363 / 2.292 / 2.268  进程起→出字 2.513 / 2.428 / 2.401
short   engine_new 0.138 / 0.134 / 0.139  按下→出字 0.812 / 0.792 / 0.794  进程起→出字 0.950 / 0.926 / 0.933
instant engine_new 0.128 / 0.135 / 0.131  按下→出字 0.842 / 0.842 / 0.848  进程起→出字 0.970 / 0.977 / 0.979
```

表外还有两发单记（`press`）：`engine_new` 0.389 与 0.198，配的按下→出字 2.521 / 2.326。
**页缓存冷的那两发就是 0.389 与 0.424 秒**（音频塔那 202 MB 要真读一遍，约 0.5 GB/s），
其余十四发落在 0.128–0.150。

`press` 那一路每次都打 `丢帧 0`：`feed` 全部返回 0。这是要盯的一位——第二段在引擎线程上
跑的时候，音频排在 `PCM_QUEUE_BOUND = 64` 的有界队列里；客户端每帧 4096 样本（16 kHz 下
0.256 秒），0.64 秒的第二段最多压住 3 帧，余量 20 倍以上。

### 3.2 `keyprobe`：真实预热 + 真实门（同一支探针，`warm-ready`，三次）

```
改前  预热完成 0.770 / 0.749 / 0.762   按下→定稿 0.300 / 0.235 / 0.223   进程起→定稿 1.070 / 0.984 / 0.985
改后  预热完成 0.140 / 0.135 / 0.128   按下→定稿 1.023 / 0.842 / 0.831   进程起→定稿 1.162 / 0.977 / 0.960
```

「预热完成」这一位就是任务里那句**进程起→可以接受按键**：0.75 → 0.13–0.14 秒。061 记的是
4.80 秒、065 之后是 0.74 秒，这一段把它压到 0.14 秒。

`按下→定稿` 这一列要看清楚它量的是什么：`keyprobe` 把 8 帧一次灌完（没有真实说话时长），
等于 `bench6` 的 `instant` 口径——所以改后 0.83–1.02 秒里含第二段剩下的那一段。真实说话 2 秒
时这一档是 `bench6 press` 的 2.27 秒，与改前同一档。

### 3.3 为什么 2 秒说话那一档「没变快」

2.00 秒的音频要 2.00 秒才说完，这是下限。改前的 2.28 秒 = 2.00（说话）+ 0.28（定稿：
梅尔 + 编码 + 44 token 预填充 + 3 个新 token）。改后的 2.27 秒 = 2.00（说话）+ 0.27（同
一段定稿；第二段在说话的第 0.64 秒就装完了，落在说话里头）。**能省的只有「在按下之前
等引擎」的那一段**，所以省下来的 0.6 秒出现在「进程起→出字」这一列，不在「按下→出字」。

## 4. 极短语音：`begin` 之后立刻 `finish` 走的是什么路

时序（都在引擎线程上，一条命令一条命令处理）：

```
调用线程：begin ──► 引擎线程：立会话、回 0 ──► 装第二段（0.643 s）──► 出队 Feed… ──► 出队 Finish ──► 定稿
          ▲ 立即返回                        ▲ 调用线程这时已经在送音频 / 已经松手
```

三件事因此同时成立：

1. **`begin` 不等第二段**：会话立起来、回 0 的那一行在前，装第二段在后。调用线程拿到 0 就
   开麦克风，那 0.64 秒的说话不会丢（这也是没把第二段做成「启动时空转着装」的原因：空转时
   按键会让 `begin` 阻塞，音频采集要迟 0.6 秒才开）。
2. **`finish` 一定在第二段之后**：`Finish` 是排在 `Begin` 后面的命令，同一个线程按序处理，
   结构上就到不了「文本塔没装好就定稿」这一支。`Finish` 那一支另外还再叫一次
   `load_text_deferred()`（幂等，装过就 `Ok(false)`），兜住「`begin` 那一步失败过」与
   「`finish` 是别的入口排进来的」两种情形；真装不上，`last_error` 里是
   `文本塔装载失败：<原因>`，`finish` 返回非 0，不是空串。
3. **`Engine::transcribe` 自己也不给空串**：文本塔不在位时它返回 `文本塔还没装好` 这个错
   （`staged_load` 有一条断言拦这一位），产品路径上走到那里就是上面那条失败回复。

实测（`bench6`）：

| 口径 | 改前 | 改后 | 文本 |
| --- | --- | --- | --- |
| 0.3 秒音频（说明得极短） | 按下→出字 0.488–0.492 s；进程起→出字 1.233–1.273 s | 按下→出字 0.792–0.812 s；进程起→出字 0.926–0.950 s | `嗯。` |
| 2 秒音频一次喂完、立刻松手 | 按下→出字 0.233–0.255 s；进程起→出字 0.994–1.022 s | 按下→出字 0.831–0.848 s；进程起→出字 0.960–0.979 s | `嗯。` |

`warm` 那一档是把 `instant` 在一个进程里连跑两个会话：改后第一个会话 0.840 / 0.865 / 1.001 秒
（含第二段），第二个会话 0.202 / 0.206 / 0.211 秒；改前两个会话都是 0.20–0.35 秒。也就是说
**第二段一个进程只付一次**，装过之后 `load_text` 回 `Ok(false)`，第二次不再读盘、不再建图。

两档都是 `ok:true`、`kind:"final"`、文本 `嗯。`，没有一次空串，也没有一次失败回复。
**极短语音仍正确**，代价是这一档的「按下→出字」要等第二段装完（§5）。

## 5. 代价：极短那一档按下→出字慢了，还有 RSS

- **极短 / 瞬间松手那一档，按下→出字慢了 0.30–0.60 秒**（0.49 → 0.80、0.24 → 0.84）。
  这是把 0.643 秒的第二段搬进会话的直接后果：说满 2 秒时它藏在说话里，只说 0.3 秒时藏不住，
  剩下的 0.35 秒就得等。同一档的「进程起→出字」仍然更早（1.25 → 0.93、1.01 → 0.97），
  因为等待的位置从「按下之前」挪到了「松手之后」。两条都列在上表里，没有拿一档去换另一档的
  好看法。
- **取舍的另一种摆法**：把分词器（0.429 秒）留在第一段，「可以接受按键」会从 0.14 秒回到
  0.54 秒，而极短那一档的按下→出字会回到 0.49–0.56 秒（这一档在分词器还在第一段的那一版
  上量到 0.491 / 0.561，同一支探针）。这一次按任务给的分界办——第一段只装
  「音频塔与梅尔」，分词器属于文本一侧（它只在拼提示与还原文本时用得上），所以放进第二段。
- **峰值内存看不出上涨**：两段之间 `SafeWeights`（表头 136 KB 与文件句柄）比改前多活一段
  时间，另外多存一个模型目录字符串，量级是几 MB。实测改前 709.6–713.8 MB、改后
  710.0–714.4 MB，两带子互相压着，差在测量噪声内（各三次，`/usr/bin/time -l`）。
- **有界队列的余量**：第二段跑的时候 `feed` 进队列。客户端 0.256 秒一帧，0.64 秒最多 3 帧，
  队列 64 格。这一次没有改 `PCM_QUEUE_BOUND`，也没有加「装载时收音频」的旁路。
- **`voice_engine_free` 撞上第二段**会等它装完再退（`shutdown` 与装载在同一个线程上），
  最坏 0.64 秒。没有另开线程，`Array` 不是 `Send`，这条线程是唯一能碰张量的地方。

## 6. 改了什么

`code/voice-engine/voice-mlx/src/backend/transcribe.rs`

- `Engine` 的字段：`decoder: Option<TextDecoder>`、`tokenizer: Option<Tokenizer>`、
  `weights: Option<SafeWeights>`、`model_dir: Option<String>`。`open_audio` 只建梅尔、开权重、
  装音频塔、`finish` 一次；`load_text` 装分词器与文本塔，再 `finish` 一次，装完把权重与目录
  放开（表头与文件句柄不留在进程里）。装失败时权重还回槽里，能重试。
- `Engine::open` 仍是两段连做，签名不变；`AudioTower::open`、`TextDecoder::open_with`、
  `TextDecoder::open` 一字未改。
- `transcribe_with` / `stream_step` 在文本塔不在位时返回 `文本塔还没装好`，不返回空串；
  `text_ready()` 给调用方一位判断；`decoder()` 改成 `Option<&mut TextDecoder>`、
  `tokenizer()` 改成 `Option<&Tokenizer>`（仓里没有别的调用点）。

`code/voice-engine/voice-ffi/src/`

- `decoder.rs`：`boot_resident` 改成 `Engine::open_audio`（第一段）；新增
  `load_text_resident()`（第二段，幂等）与 `text_ready()`；`transcribe_on_resident` 定稿前先
  `load_text_resident()`；说话中的部分步在 `text_ready()` 为假时直接回空（那几帧过不了文本塔，
  跳过不报错，定稿照旧整段重算）。不打开 `metal` 时 `load_text_resident` 回 `Ok(false)`、
  `text_ready` 回 `true`。
- `backend.rs`：`boot` 的注释改成两段；新增 `load_text_deferred()`（`begin` 之后那一步的入口，
  测试里数调用次数）。
- `engine.rs`：`drive` 的 `Begin` 分支在 `reply.send(0)` **之后**调
  `load_text_deferred()`；`Finish` 分支在 `slot.finish()` 之前再调一次，失败时把
  `文本塔装载失败：<原因>` 写进 `last_error` 并回非 0。
- `lib.rs`：`voice_engine_new` / `voice_session_begin` / `voice_session_finish` 的文档写清两段
  与「谁等谁」。C ABI 的九个符号、头文件、JSON 行的六个键一个没变。

新增测试两条：`voice-mlx/tests/staged_load.rs`（metal 侧：第一段之后定稿报错、第二段之后
token 与文本与一次装完及参考逐位相同、第二段重入为 `Ok(false)`，顺带打两段墙钟）与
`voice-ffi` 的 `the_second_stage_runs_after_begin_and_again_before_finish`（不打开 `metal`
也跑：数第二段在 `begin` 之后与 `finish` 之前各被叫过）。

## 7. 校验

```
cd code/voice-engine && cargo test --workspace --offline
```

**54 通过、4 忽略、0 失败**（`voice-ffi` 12 → 13，新增的是上面那条时序测试；其余与 065 同一笔账）。

打开 `metal`（参数见 `BUILD.txt`）：

- `--lib`：4 通过 + 1 忽略（`load_phases_are_reported_one_by_one`）。
- `--test staged_load`：1 通过。第一段 0.152 s、第二段 0.643 s；分段装完的定稿
  `嗯。` / `[106287, 1773, 151645]`，与一次装完逐位相同，也与参考贪心序列逐位相同。
- `--test encoder_parity`：3 通过。第 0 层 3.90625e-3、末尾投影 1.1855885e-3，与 065 相同。
- `--test decoder_parity`：6 通过。主闸 7.8125e-2 对容差 8.203125e-2（余量 1.05 倍）；
  注入参考特征那条 6.25e-2；两条归因 2.5214844e1 / 2.1436523e1；四口径分组对照
  7.8125e-2 / 6.25e-2 / 3.125e-2 / 4.6875e-2，argmax 全为 106287。逐位与 065 相同。
- `--test end_to_end_parity`：4 通过。提示 44 个 id、贪心 `[106287, 1773, 151645]`、
  文本 `嗯。`、缓存第二步 1773；一次 `transcribe` 墙钟 1.39 秒。
- `--test streaming -- --ignored`：2 通过。切窗与整段编码的投影差 1.0691956e-4（闸 1e-3）；
  2 秒处的部分步 0.214 秒出 `嗯。`，定稿仍与整段重算逐位相同。

## 8. 复跑

```text
# 两段墙钟 + 第二段的三条断言
cd code/voice-engine
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
export RUSTFLAGS="-C link-arg=-framework -C link-arg=Accelerate \
  -C link-arg=-Wl,-U,_MTLTensorDomain -C link-arg=-Wl,-U,_MTLLogStateErrorDomain \
  -C link-arg=-Wl,-U,_NSDeviceCertificationiPhonePerformanceGaming \
  -C link-arg=-Wl,-U,_NSProcessInfoPerformanceProfileDidChangeNotification \
  -C link-arg=-Wl,-U,_NSProcessPerformanceProfileDefault \
  -C link-arg=-Wl,-U,_NSProcessPerformanceProfileSustained \
  -C link-arg=-Wl,-U,___fmaxf16 -C link-arg=-Wl,-U,___fminf16"
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test staged_load -- --nocapture --test-threads=1

# 一句话墙钟（四种口径换最后一个参数：press / short / instant / warm）
cargo build -p voice-ffi --features metal --target aarch64-apple-darwin
bash /tmp/voice-abi/link.sh /tmp/voice-abi/bench6.c /tmp/voice-abi/bench6
/tmp/voice-abi/bench6 \
  code/assets/qwen3-asr-0.6b-4bit \
  "$HOME/Library/Input Methods/RimeClient.app/Contents/Resources/mlx.metallib" \
  code/voice-oracle/assets/probe_pcm.bin press

# 真实预热 + 真实门：进程起→可以接受按键
cd /tmp/voice-abi/keyprobe && swift build -c release
.build/release/keyprobe warm-ready \
  <模型目录> "$HOME/Library/Input Methods/RimeClient.app/Contents/Resources/mlx.metallib" \
  <探针 pcm> 4096
```

<!-- research-complete -->
