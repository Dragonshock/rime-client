# 025-derivation-gap-fill-binding：四处核对如何改写 018 的候选绑定

本文只做一件事：用 `021`、`022`、`023`、`024` 已经核对的返回字段和数字，改写 `018` 第 4 节的候选配对与第 6 节的候选倾向。原语 4、5、9 的对外类型不改。某一格仍然没有字段或没有数字时，格子留空，不补成一种拼接、一种权重或一种修订指针。豆包输入法只说明适配器交还字符串之后，外面还可以做的比较和判定；那些函数不进入候选制品。

## 1. 018 里被这四份碰到的候选

以下是 `018` 已经写下的候选，不是新结论。

识别流有两行。波形已经成段、快照文本要进系统提示时，候选制品是 `mlx-audio` 的 Qwen3-ASR：`generate` / `stream_transcribe`，Swift 为 `Qwen3ASRModel.fromPretrained` 之后的 `generate` / `generateStream`。`018` 把 Swift 与 Python 写在同一行，并写 `generate` 另有 `system_prompt` 与 `hotwords`，适配器再「把快照收成系统提示」。帧还在到达时，`018` 在 `mlx-audio` 的 `qwen3_asr.py` 里检索 `create_streaming_session` 为空，候选改到 Python `VoxtralStreamingSession.feed` / `step` / `close`。非空快照在这一行没有参数。Swift 的 Voxtral `feed` 源码当时未读，不与 Python 写成同一签名。

整段覆盖 `state.text` 的函数，`018` 记在 vLLM 上，并且该函数拒绝非 vLLM。`018` 因此不把它写入 MLX 绑定。适配器侧的包法是：制品给出整段最新文本时，新假设用这份整段；制品只给出追加片段时，把上一条文本接上片段。两种包法的修订目标都由适配器指向上一条。`018` 写明这不证明模型会改掉已经吐出的字。`018` 全文没有 `moona3k`、`feed_audio`，也没有 `host_app`、`field_role`、`cursor_before`、`corrections` 这些槽名。

改写流的候选制品是 `mlx_lm` 或 mlx-swift-lm 能按 `qwen2` / `qwen3` / `qwen3_5` 加载的 Qwen 指令权重。「不替换」或替换文本由适配器比较全文得到。`018` 用 ASR-EC 上 2023 年 Qwen-7B 的 CER 高于识别基线，把候选停在输出类型，不把「字符串不同」写成应当替换。0.6B、4B、27B 不按 tok/s 挑选。有序帧在 Apple Silicon 上的毫秒，`018` 没有来源，不补。

`018` 还写：一次会话接一行识别制品和一行改写制品；不把 Qwen3-ASR 与 Voxtral 串成前后两级。本文沿用这句，不把新出现的第三行识别制品串进去。

## 2. 事实

四份笔记的数字与字段按各自口径分开。下面不把时延、CER、内存字节放进同一张表。

### 2.1 返回值里有什么、没有什么

`021` 通读了三份 Python 入口，没有喂入音频。

`moona3k/mlx-qwen3-asr` 的 `feed_audio` 返回 `StreamingState`，不是增量，也不是假设标识。活动窗口在前 `unfixed_chunk_num` 块（文件常量 2）不带旧文本前缀；其后丢掉末尾 `unfixed_token_num`（文件常量 5）再加可能的提交回退，然后重新解码。调用方若已交出上一份 `state.text`，下一份仍是一整段新的 `state.text`，尾部字符可以不同。`_rewrite_events` 只在新整段不以旧整段为前缀时加一。它是会话上的次数，没有假设标识，没有被改字符串的下标，没有编辑区间。

```python
# 来源: https://raw.githubusercontent.com/moona3k/mlx-qwen3-asr/main/mlx_qwen3_asr/streaming.py 的 feed_audio
        if state.text != prev_text:
            state._text_updates += 1
        if prev_text and state.text and not state.text.startswith(prev_text):
            state._rewrite_events += 1
```

`stable_text` 的注释写不会再变。函数体不是公共前缀：新稳定串更短才留旧串；长度不少于旧串时直接换成新串，不检查它是不是旧串的前缀。`StreamingState` 上这次读到的文本名字没有 `hypothesis`、`revision`、`replaces`。`_append_chunk_text` 不在 `feed_audio` 的调用链上；仓库内检索该符号无结果。

`mlx-audio` 的 Qwen3-ASR：`StreamingResult` 的字段是 `text`、`is_final`、`start_time`、`end_time`、`language`、`prompt_tokens`、`generation_tokens`。`stream_transcribe` 交出的 `text` 是 `last_segment` 的新切片，已读循环不修改先前 yield 出去的对象。`is_final` 只在最后一块为真，标记的是音频块边界，不是「改哪一条」。`generate(..., stream=True)` 转到 `stream_transcribe`。该文件检索 `create_streaming_session` 仍无匹配。`last_segment` 推进 offset 之后，不再写回已经交出去的切片。

Voxtral 的 `step` 返回 `list[str]`。已读循环只往 `generated` 追加 token，不接收上一次返回的列表。类注释写较短的 `transcription_delay_ms` 会让部分假设更常翻转；返回值里没有与这句话对应的一项。`transcription_delay_ms` 默认 480。构造参数没有 `context`、`system_prompt`、`hotwords`，也没有假设标识。Nemotron 的 `streaming.py` 这次没有打开。Swift 的 `Delta` 不在 `021` 重开的三份入口里，本文不新写它的字段。

### 2.2 五槽名字落在哪些形参上

`022` 核对的五槽是 `host_app`、`field_role`、`cursor_before`、`hotwords`、`corrections`。缺席是「没有这个槽」，不是空字符串。本文不把缺席改成空字符串再拼进提示。

同名形参 `hotwords: Optional[List[str]]` 只出现在 `mlx-audio` Python 的 `generate`。空则 `system_prompt` 原样返回；非空则用 `", "` 拼进 `system_prompt`。`stream_transcribe` 自身没有这个形参。热词列表能进流式路径，只发生在先调用 `generate` 的时候。

官方 `qwen_asr` 的文本形参叫 `context`，类型是一段字符串；`List[str]` 是按音频条数广播，不是一条音频上的热词列表。`mlx-audio` Swift 的 `generate` / `generateStream` 形参是 `context: String`，没有 `hotwords`。接受 `STTGenerateParameters` 的重载把 `context` 写成 `""`；`STTGenerateParameters.swift` 的猜测路径返回 HTTP 404，字段表不补。`moona3k` 的 `init_streaming` 文本形参也是 `context: str`。README 把它说明成空格分隔的领域词；省略时系统提示为空。`max_context_sec` 乘的是采样率，截的是音频，不是文本槽。`Qwen/Qwen3-ASR-1.7B-hf` 模型卡的形参名是 `prompt`，标题写成 “Context / hotwords”，没有成对字段。论文 HTML 里 `hotword`、`cursor`、`correction`、`host_app` 的子串计数是 0。

`corrections`、`cursor_before`、`host_app`、`field_role` 在上述已打开入口里没有形参。Voxtral Python 的 `create_streaming_session` 只有 `max_tokens`、`temperature`、`transcription_delay_ms`。Swift `VoxtralRealtimeStreamSession.init` 是 `temperature`、`maxTokens`、`transcriptionDelayMs`。两处都没有文本槽。`**kwargs` 在已读函数体里没有被读取，不因此变成落点。

官方 `qwen_asr` 不是 `018` 那张表里的 MLX 调用。本文不因为它有一个叫 `context` 的字符串，就把它补进行列。

### 2.3 替换文本与识别假设的分数，分属不同测试集

`023` 没有给出「某一仓库路径、某一次 MLX 量化、替换文本 CER 低于识别假设 CER」这样一对可以拿来挑权重的数字。下面两张表的测试集、提示和检查点写法都不同，不并成一张高低表。

来源是 *Selective Gating for LLM-Based Chinese ASR Error Correction* 的 PDF。名字是 `Qwen2.5-3B-Instruct`，不是仓库路径。部署是本地 4-bit NF4、贪心、1-best。这不是 `016` 里 MLX 4-bit 的 group size。语料是 Common Voice Scripted Speech 26.0 的 zh-CN，独立测试集 1,000 条。标点已在评分前去掉。

| 方法 | Macro CER | Corpus CER | 接受条数 | 变好 | 不变 | 变差 |
| --- | --- | --- | --- | --- | --- | --- |
| Whisper | 34.75% | 24.50% | 0 | — | — | — |
| Always LLM | 44.75% | 30.56% | 1000 | 72 | 459 | 469 |
| Confidence Gate | 39.26% | 25.28% | 243 | 16 | 907 | 77 |
| Final Joint Gate | 34.79% | 24.53% | 91 | 0 | 995 | 5 |

直接接受全部输出时，两项 CER 都高于 Whisper。Final Joint Gate 接受 91 条，即 9.10%；这 91 条里变好为 0，变差为 5。门控后的两项 CER 都略高于 Whisper。论文写没有做显著性检验，也不把拒绝大部分替换描述成纠错准确率的提高。这张表只覆盖这个名字、NF4、贪心、1-best 和这次抽样。

另一来源是 arXiv:2509.23630v1 的 HTML，测试集是《Arena Breakout》玩家实况语音 3,872 条。表中的行名是 `Qwen-72B w/o SFT` 与 `Qwen-1.5B w/ SFT`。HTML 没有写出 Instruct 或 Base，也没有仓库路径。72B 那行无领域微调，提示含 N-best 与游戏背景；1.5B 那行在游戏 ASR 输出与参考转写上微调，不接入该文的检索。两行的提示不相同。

| 表中的行 | CER | SER |
| --- | --- | --- |
| Vanilla ASR Output | 9.87% | 46.51% |
| Qwen-72B w/o SFT | 7.07% | 34.68% |
| Qwen-1.5B w/ SFT | 6.79% | 24.66% |

同文表 IV 的 `GO-AEC w/o SFT` 行，CER 13.66%，SER 50.10%，都高于同表 Vanilla 的 9.87% 与 46.51%。行名不是 `Qwen2.5-1.5B`。13.66% 不记成该检查点的零样本 CER。

arXiv:2602.12287v1 点了 Qwen3 的 0.6B 与 8B，AISHELL-1 的表没有未纠错识别假设的 CER。识别假设 CER：未知。这两档是否使替换文本错误更少：未知。格子不转入本文。口语清理方面，点名 Qwen 文本检查点的 F1 仍是空结果，不写成零分。上述任何一行在 Apple Silicon 或 MLX 上的复测：未知。`023` 没有本地测量。

### 2.4 本机到达块的墙钟，只属于一次条件

`024` 的进程从 2026-11-12 03:52:04 HKT 到 03:52:43 HKT。机器读数是 MacBook Pro，Model Identifier `Mac14,10`，Apple M2 Pro，12 核（8 个性能核、4 个能效核），内存 16 GB，macOS 27.2.1，Build `26B131`，Darwin 27.2.0。虚拟环境在 `/tmp/gap024/venv`，包是 `mlx-qwen3-asr==0.4.4` 与 `mlx==0.32.4`。权重是 `moona3k/mlx-qwen3-asr-0.6b-4bit`，快照 `4c59c533f95c84afb796655e814709034f826f04`。`quantization_config.json` 是 `bits` 4、`group_size` 64、`audio_tower_bits` 8。带 `bits` 属性的模块：`lm_head` 1 个 4-bit，`model` 197 个 4-bit，`audio_tower` 111 个 8-bit，组大小都是 64。`weights.safetensors` 为 537626967 字节。加载墙钟 25.463470 秒，包含 6 个文件下载。缓存命中后的纯加载秒数：未知。

音频 279983 帧，16 kHz，时长 17.4989375 秒。调用是 `init_streaming` / `feed_audio`，显式参数含 `chunk_size_sec=2.0`、`max_context_sec=30.0`、`sample_rate=16000`、`language="Chinese"`。默认含 `unfixed_chunk_num=2`、`unfixed_token_num=5`、`reuse_window_prefix=True`、`commit_at_silence=True`。一块是 32000 个样本。8 次调用之后剩余 23983 个样本（1.4989375 秒）没有送入。计时包住 `feed_audio` 返回之后的 `mx.synchronize()`。预热 1 次不计入中位，再跑 10 次。README 写这条默认路径会跨块复用编码器输出和解码器 KV。下表不是「每块从零重算整段窗口」的墙钟。10 次的 `streaming_metrics` 都是 `text_updates=8`、`rewrite_events=7`。块序 0 没有上一份文本，7 次改写落在块序 1 至 7。

| 块序 | 该块结束时已送入 | 次数 | 中位（秒） | 最小（秒） | 最大（秒） |
| --- | --- | --- | --- | --- | --- |
| 1 | 4.0 秒 | 10 | 0.100019 | 0.098852 | 0.103165 |
| 2 | 6.0 秒 | 10 | 0.113075 | 0.112377 | 0.114901 |
| 3 | 8.0 秒 | 10 | 0.115692 | 0.115111 | 0.117534 |
| 4 | 10.0 秒 | 10 | 0.140109 | 0.138917 | 0.142307 |
| 5 | 12.0 秒 | 10 | 0.110996 | 0.109887 | 0.112240 |
| 6 | 14.0 秒 | 10 | 0.119115 | 0.117680 | 0.121229 |
| 7 | 16.0 秒 | 10 | 0.127331 | 0.125818 | 0.127921 |

第 0 次计时会话里，块序 1 把上一份「下面是一段用来测量。」换成「下面是一段用来测量到达正的普通话。经」。块序 4 把「见面，先讨论预算。」换成「见面。先讨论预算，再确认下周的行程。」。逗号换成了句号。这是这一次保存下来的字符串，不是错误率。`stable_text` 在这次会话的块序 0 和 1 仍是空字符串，从块序 2 起是更短前缀并且一次比一次长。其余 9 次的字符串没有保存，不能把「只变长」写成那 9 次的观察。返回值仍没有「改的是哪一条假设」。

块序 0 是第一份文本，10 次中位 0.064944 秒，最小 0.063924 秒，最大 0.066072 秒。它不是上一张表的改写行。预热八块墙钟依次是 1.858804、0.116143、0.155544、0.115539、0.227112、0.111271、0.118015、0.126137 秒。多出来的时间没有拆开，不记成着色器编译。

内存来自不同接口，不相加。

| 接口 | 字节 |
| --- | --- |
| `mx.metal.get_peak_memory`（预热之后清零，10 次计时跑完） | 825819094 |
| `mx.metal.get_active_memory`（进程末） | 545453458 |
| `mx.metal.get_cache_memory`（进程末） | 20990536 |

| 接口 | 字节 |
| --- | --- |
| `ru_maxrss`（整个进程，含下载、加载和预热） | 1007730688 |

`man 2 getrusage` 写明 `ru_maxrss` 的单位是字节。同一条命令前后，`vm.swapusage` 都是 `total = 3072.00M  used = 1831.69M  free = 1240.31M`。交换区占用没有在这条命令的前后之间上升。开始之前已经用了 1831.69M，这不是一张空闲 16 GB 上的读数。

尾部 23983 个样本没有送入，`finish_streaming` 看到空缓冲就返回。收尾解码的时延：未知。`transcribe()` 整段进、整段出这次没有跑：未知。0.6B 的 8-bit 与 1.7B 这次没有加载。`mlx-qwen3-asr` README 上的 M4 Pro 整段表、`015` 里硬件未点名的 TTFT，都不是这张表，不并进来。

### 2.5 豆包只说明适配器外面可以做什么

`002` §4.1 的 `ASRTypeWriter.updateResult` 在调用方已经拿到的新字符串和屏幕上已经推进到的前缀之间做 `commonPrefix`。`021` 沿用该节地址，没有新的反汇编。比较发生在识别文本已经交出来之后。`004` §2.2 写部分结果带着未定稿分句和 `prefetch`，再交给这个打字机。部分结果里没有「修订的是哪一条假设」。

`003` §2.4 的 `PromptProcessor` 把 `HOTWORDS`、`USER_MODIFY`、`CUR_CONTEXT`、`BOT_DESC` 分成不同的处理函数。`022` 写明这张投影不抄到 Qwen 或 Voxtral 的形参上。`004` §3.2 的 `rewriteDecision` 可以在判定之后留下原始 ASR 文本；同一节没有任一 Qwen 检查点的 CER。`003` 的算子名 `W4A8ImeAsrQwen3Transformer` 位于识别解码一侧，`023` 没有它相对识别假设的 CER。

这些笔记说明的是适配器边界之外的两件事：拿到两段字符串之后，可以再比较哪些字符还相同；在把另一段文本交出去之前，可以留下原假设。它们不是本文的候选制品，也不提供可与第 2.4 节相减的本机毫秒。

## 3. 推论：候选配对改哪几格

以下全部是候选。原语 4 仍是有序帧、快照、会话标识进，带修订目标的假设出。原语 5 仍是假设文本和同一份快照进，「不替换」或替换文本出，两者都留下原假设。原语 9 仍按制品实际接受的输入类型接角色。契约对不上的格子空着。

### 3.1 帧还在到达：补一行整段返回，不替换 Voxtral 那一行

`018` 写「已读的 Qwen3-ASR MLX 函数填不上到达中的帧」，依据是 `mlx-audio` 那份 `qwen3_asr.py` 没有 `create_streaming_session`。`021` 对这份文件的检索仍是空的，这句对 `mlx-audio` 的 Qwen3-ASR 仍然成立。

另一件 MLX 制品有到达中的入口。`feed_audio` 接受陆续送来的 PCM，返回新的整段 `state.text`。`024` 在第 2.4 节的条件下，10 次都出现 7 次「新整段不以旧整段为前缀」，并且第 0 次计时会话里已经交出的句号和逗号被换成了别的字符。`018` 第 2.2 节把「整段文本被覆盖」只记在 vLLM 上。那条拒绝非 vLLM 的函数仍然不进入 MLX 绑定。改掉的是「MLX 上没有整段返回」这一句：`moona3k` 的 `feed_audio` 是另一行候选，不是把 vLLM 函数写回来。

这一行的返回形状是整段 `text`，不是 `018` 为 Voxtral 写下的追加片段。`018` 为「整段最新文本」准备的包法因此有了一个 MLX 落点：新假设的文本用这份整段。修订目标仍然不在返回值里。`_rewrite_events` 不充当「改的是哪一条」。`stable_text` 既不是公共前缀，也没有在 10 次里被保存成对照字符串；不把它写成修订目标，也不把它写成已经测得不变的前缀。

Voxtral 那一行保持追加切片。`021` 读到的 `step` 不改写上一次返回的列表。类注释里的翻转没有字段。`024` 没有测量 Voxtral，不用上一节的中位去比它，也不把 moona3k 上看到的改字写成 Voxtral 或 `stream_transcribe` 已经改字。`018` 对追加片段的反论，对这两份入口仍然成立。

Swift Voxtral 的 `init` 已由 `022` 读到，仍然没有文本槽，仍然不与 Python `feed` 写成同一签名。`021` 没有重开 `Delta`，本文不给它补修订字段。Nemotron 仍未打开，不进入候选配对。

两行到达帧候选的输入都是还在到达的采样，返回形状不同。不从中挑一行，也不把它们串成前后两级。0.6B 8-bit 与 1.7B 没有这次的到达块中位，不拿 0.6B 4-bit 的中位去挑它们。内存字节同样只描述这一次进程，不拿去留下或划掉某一档权重。

### 3.2 波形已经成段：快照不再收成一整份系统提示

`mlx-audio` Python 的 Qwen3-ASR `generate` 仍是「整段波形进、文本出」的候选。`021` 确认 `StreamingResult` 没有修订目标，`last_segment` 不回写上一条。这一格与 `018` 相同。

改掉的是适配器补上的「把快照收成系统提示」。五槽里，只有 `hotwords` 在这个 `generate` 上有同名形参，类型是 `List[str]`，由 `merge_hotwords` 用逗号进入 `system_prompt`。另外四槽没有形参。不把它们写进 `system_prompt` 来冒充落点，也不新写一种分隔符。直接调用 `stream_transcribe` 时，连 `hotwords` 这个名字都没有。

Swift 的 Qwen3-ASR 从 `018` 的同一行拆出来。它的文本形参是 `context: String`，不是 `hotwords`，也不是 `system_prompt`。`STTGenerateParameters` 重载传入的是空字符串；结构体还有哪些字段，404 之后仍为空。不把 Swift 写成已经接受热词列表。

`moona3k` 的 `context` 是一段字符串。README 的例子是空格分隔的领域词。这与 Python `hotwords` 的逗号列表不是同一个类型。哪些槽名、用空格还是逗号进入这段字符串，源码没有槽位表。拼接方式保持未知，不在本篇补一种模板。空快照时，`018` 已写 Qwen 的系统提示可以为空、Voxtral 的构造参数里本来就没有快照。`022` 与这句一致，空快照条款不改。

### 3.3 改写流：输出类型不动，分数不挑权重

改写候选制品的范围与 `018` 相同：能按 `qwen2` / `qwen3` / `qwen3_5` 加载的 Qwen 指令权重。比较全文仍然发生在适配器里，不在模型返回值里。恒等改写（始终「不替换」）仍然满足原语 5 的核对句。

`023` 改变的是 `018` 那条反论的材料，不是制品范围。`018` 只有 ASR-EC 上 2023 年 Qwen-7B 这一条「CER 高于识别原文」。现在同一方向上还有第 2.3 节第一张表：直接接受 `Qwen2.5-3B-Instruct` 的 NF4 输出，两项 CER 都高于该文的 Whisper 假设；联合门控把大部分输出留在原文之后，两项 CER 仍不低于该假设。相反方向上有第二张表的两行，CER 与 SER 低于该文的单路识别，但提示、测试集和检查点变体都不同，而且有一行去掉微调之后高于识别原文。方向依行而变。

因此不把「字符串不同」补成应当替换，也不把「出现过上升」补成每一档都不替换。没有仓库路径、没有 MLX 量化、没有「低于识别假设」这一对数字的档，保持不挑。Qwen3 0.6B 与 8B 因为缺少识别假设 CER，不写入改写候选的成绩。`004` 的 `rewriteDecision` 只说明适配器外面可以留下原文；它没有 CER，不成为改写制品，也不把第一张表的门控写成已经量过豆包。

`021` 至 `024` 没有打开 jinja。`018` 里思考块不进入替换文本的那条候选，不因这四份而改。

### 3.4 时延格：只填进 moona3k 这一行

`018` 的开放问题 7 当时没有 Apple Silicon 上、针对有序帧的毫秒。现在第 2.4 节的中位可以写在 `feed_audio` 这一行旁边，条件是 M2 Pro、16 GB、该 0.6B 4-bit 权重（音频塔 8-bit）、2.0 秒一块、`reuse_window_prefix=True`、预热之后的 10 次。块序 1 至 7 的中位不收成一个数，因为窗长随块序增加。块序 0、预热、加载秒数、尾块解码、整段 `transcribe()` 都不写入这七个中位。

Voxtral 行、`mlx-audio` Qwen3-ASR 的整段行、Swift 行的时延格仍然空着。空着就不补一个数，也不借用 README 的 M4 Pro 整段中位或论文里的 TTFT。

## 4. 舍弃了什么

舍弃把 `ASRTypeWriter`、`PromptProcessor`、`rewriteDecision` 或 `W4A8ImeAsrQwen3Transformer` 写成绑定制品。公共前缀和「留下原文」只说明适配器外面可以做什么。

舍弃在 Voxtral 的追加切片与 `feed_audio` 的整段 `text` 之间挑一个担任「帧还在到达」。也舍弃把二者或与 Qwen3-ASR 的整段 `generate` 串成两级。已读源码里没有这种组合。

舍弃把 `hotwords` 的逗号拼接推广成五槽模板，也舍弃把缺席槽写成空字符串。舍弃把官方 `qwen_asr` 的 `context` 补进 MLX 绑定。

舍弃用 CER 下降行或上升行划定改写权重。两张表不并。舍弃把 `_rewrite_events` 或 `stable_text` 写成修订指针。舍弃把第 2.4 节的中位写成 Voxtral、1.7B 或 0.6B 8-bit 的时延。

vLLM 上拒绝非 vLLM 的流式函数，仍然舍弃，不因为 `moona3k` 也返回整段文本而写回来。

## 5. 当前倾向（候选）

候选倾向如下。

会话仍按制品声称的输入类型接角色，不按仓库名字并成一行。整段波形走 `mlx-audio` Python `generate` 时，候选制品仍是 Qwen3-ASR；五槽里只有 `hotwords` 对得上 `List[str]`。Swift 的文本形参是另一段 `context` 字符串，单独成行。帧还在到达并且调用方要追加切片时，候选仍是 Python Voxtral 的 `feed` / `step`，非空快照仍然没有参数。帧还在到达并且调用方要一份新的整段文本时，候选补上 `moona3k` 的 `feed_audio`；修订次数不是修订指针，时延只引用第 2.4 节那七个中位。改写候选仍是可按 `qwen2` / `qwen3` / `qwen3_5` 加载的 Qwen 指令权重，输出类型由全文比较填上，不从第 2.3 节的两张表里挑一档。一次会话只接一行识别候选和一行改写候选。

用户对 Qwen 的偏好只解释为什么整段波形行和 `feed_audio` 行先核对 Qwen 仓库。它不把 Voxtral 的追加切片写成 Qwen 已经具备，也不把某档文本权重写成替换已经更正确。

反论：第 2.4 节证明的是这一句口播、这一次默认复用前缀的路径上，`text` 可以不再以上一份全文为前缀。它不证明已经放进 `stable_text` 的字会被改掉，也不证明 Voxtral 已经打印的切片会被模型收回。另一反论：第 2.3 节的 CER 有升有降，口径不同。候选停在输出类型，不把差异写成优化已经成立，也不把上升写成改写角色空着。

仍然空着、不补成选择的有：三份已读 MLX 返回值里的修订指针；四槽进入 `context` 或 `system_prompt` 的拼法；Voxtral、`stream_transcribe`、1.7B、0.6B 8-bit、尾块解码和整段 `transcribe()` 的到达帧毫秒；点名仓库路径并且在 MLX 上低于识别假设的改写 CER。

## 6. 检索日志

| # | 动作 | 地址或查询 | 结果 |
| --- | --- | --- | --- |
| 1 | 本地列举 | `dev-docs/notes` | 读到 018 至 024 的路径；写入前没有 025 |
| 2 | 本地阅读 | `018` 全文 | 两行识别候选加一行改写；无 `moona3k`，无五槽名 |
| 3 | 本地阅读 | `019`、`020` | 四处未知与 025 的职责：只改写 018 的候选绑定 |
| 4 | 本地阅读 | `021` 全文 | `feed_audio` 返回整段状态；另外两份只交新切片；三份都无修订指针 |
| 5 | 本地阅读 | `022` 全文 | 同名 `hotwords` 只在 Python `generate`；四槽无落点；Voxtral 无文本槽 |
| 6 | 本地阅读 | `023` 全文 | 两套 CER 口径不同；Qwen3 相对识别假设的 CER 未知；口语清理 F1 仍空 |
| 7 | 本地阅读 | `024` 全文 | M2 Pro 上 0.6B 4-bit 的 `feed_audio` 中位；尾块与整段时延未知 |
| 8 | 本地阅读 | `007` 原语 4、5、6、9 | 对外类型以原语为准；本文不改类型 |
| 9 | 本地阅读 | `002` §4.1、§4.3 | 公共前缀在已拿到的字符串上；宿主焦点校验不是 CER |
| 10 | 本地阅读 | `003` §2.3、§2.4 与篇末图注 | 分函数槽位与双路字样；不把算子名写入候选制品 |
| 11 | 本地阅读 | `004` §2.2、§3.2 | 部分结果交给打字机；`rewriteDecision` 可留下原文；无 Qwen CER |
| 12 | 本地阅读 | `GUIDE` 4.3 | 推导写推理链、舍弃与候选倾向；缺口可以留着 |
| 13 | 本地检索 | `018` 中的 `moona3k`、`feed_audio`、`host_app`、`field_role`、`cursor_before`、`corrections` | 无匹配。空结果 |
| 14 | 本地检索 | `021` 中的 `hypothesis`、`revision`、`replaces` | 命中的是「返回值里没有这些名字」；`003` 的 revision 是目录号 |
| 15 | 路径检查 | `dev-docs/notes/025-derivation-gap-fill-binding.md` | 写入前不存在。空结果 |

第 13 条没有在 `018` 里找到可直接改写的 moona3k 行或五槽行，所以第 3.1 节是补行，不是改原表里的一个已有单元格名称。第 6 条里的口语清理 F1、第 7 条里的尾块解码与整段 `transcribe()`，都没有新数字。本次没有调用 REA。

<!-- derivation-complete -->
