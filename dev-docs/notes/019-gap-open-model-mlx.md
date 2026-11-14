# 019-gap-open-model-mlx：开源模型与 MLX 路径上仍未闭合的缺口

本文只记录仍然未知、并且能追溯到本轮用户原话或 `007` 原语的问题。每条给出未知本身、存在依据、现在是否需要补证。`015` 至 `018` 已经核对过的调用形状，不在这里再写成倾向。`012` 已经记下的宿主预编辑、文档历史与豆包契约空位，本文不重开。

用户原话是：「确认，构建 workflow 进行详细工程分析，调研有什么开源模型可用，性能如何，首选 qwen 系列，并研究如何集成，集成路径研究 MLX。豆包输入法只作机制参考，开始调研填补空白。如果有任何阻力，调度 REA 工具用逆向工程的方法研究一下竞品，看看竞品是怎么实现的。」

下文原语均指 `dev-docs/notes/007-derivation-voice-input-primitives.md`。豆包输入法只作机制参考。公开的模型卡、README 与源码已经把下面的空位指到返回字段。本文没有调用 REA。

`018` 的开放问题 6 不再列入。`mlx-community/Qwen3-4B-Instruct-2507-4bit` 的 `chat_template.jinja` 已经打开，全文没有 `enable_thinking` 这个名字。同一份模板在回放含 `</think>` 的助手历史时仍会写出 think 标签。这是已读到的模板行为，不是未打开的文件。

## 1. 有序帧到一条可修订假设的毫秒，还没有与整段转写表同一口径的测量

**未知：** 在 MLX 上，从一块仍在到达的采样帧，到一条可以改掉已经交出的假设文本，时延与内存是多少。硬件、量化、块长要写在同一次测量上。下面几组已经能引用的数字都不是这一次测量，也不能排进同一张表。

`https://github.com/moona3k/mlx-qwen3-asr` 的 README 有一节 “Performance on Apple Silicon”。硬件写的是 Apple M4 Pro（48 GB）、macOS 26、v0.4.0。时延是 10 次运行的中位。0.6B fp16 对大约 2.5 秒的片段是 0.17 秒，对 10 秒片段是 0.30 秒，10 秒片段的 RTF 是 0.029。这是整段片段进、整段文本出的表。同页流式段落写 multilingual-100 的 primary error 流式 11.3%、离线 9.5%，长音频 11.9% 对 10.6%。那段没有毫秒，也没有把 M4 Pro 写进这句比较。

`015` 记下的 Qwen3-ASR 论文效率表，单路 TTFT 是大约 2 分钟请求上的 92 ms 与 102 ms，原文的计算资源没有点名，也不是 2 秒流式块的用户侧时延。`mlx-community/Voxtral-Mini-4B-Realtime-2602-4bit` 的 README 把 FLEURS 表标成 “Benchmarks (from upstream)”。那一节没有写这些格子是 int4 MLX 文件、在 Apple Silicon 上测出来的。`018` 因此没有把那些格子记成这件权重的成绩。这次重读该 README，标题仍然是 from upstream。

**存在依据：** 用户原话要求调研开源模型「性能如何」，集成路径研究 MLX，首选 Qwen。原语 4 的输出是会话内可修订的假设，不是一次整段转写，也不是提交文本。`015` 开放问题 2 与 8 写明：2 秒块的用户侧毫秒，论文没有给；官方 ASR 表没有 Apple Silicon 时延。`018` 开放问题 7 写明：打开的论文与当时读过的 MLX 源码没有「有序帧、Apple Silicon」这一份毫秒。`015` 同时写明第三方 MLX 自测表当时没有打开。上面的 M4 Pro 表是这次补读才进入视野的整段测量，它不填原语 4 的那一格。

**现在是否需要补证：** 需要一份写明芯片、量化、以及 feed 或分块协议的测量，对象是到达中的帧。不把 M4 Pro 上的整段中位时延、论文里未点名硬件的 TTFT、upstream FLEURS 的 WER 改写成这一份。没有这份测量时，不据此挑选 0.6B 或 1.7B，也不改原语 4 的输入输出类型。`016` 已记下的文本 tok/s 是另一口径，不与识别时延相除。

## 2. 已读的 MLX 流式返回值仍然没有「修订了哪一条」

**未知：** 下一次解码会不会改掉上一次已经交出的字符；若会改，改的是哪一条假设。三份已读入口都没有这个字段。

`mlx-audio` 的 Qwen3-ASR 源文件里，`018` 检索 `create_streaming_session` 为空。`stream_transcribe` 交出的是新增片段。官方覆盖整段 `state.text` 的函数在非 vLLM 后端上抛错，不进入 MLX 绑定。

另一份 MLX 包有到达中的 PCM 入口。`feed_audio` 的签名返回整份状态，不返回假设标识：

```python
# 来源: https://raw.githubusercontent.com/moona3k/mlx-qwen3-asr/main/mlx_qwen3_asr/streaming.py
def feed_audio(
    pcm: np.ndarray,
    state: StreamingState,
    model: Optional[Qwen3ASRModel] = None,
) -> StreamingState:
    """Feed audio to the streaming session, re-decoding the window per chunk."""
```

同文件已读到的常量是 `UNFIXED_CHUNK_NUM = 2`、`UNFIXED_TOKEN_NUM = 5`。文件头写：每来一块，就把累积窗口重新编码，先前文本减去最后若干 token 再当前缀；又写 partial results are stable。`init_streaming` 已读到的形参包括 `context`、`unfixed_chunk_num`、`unfixed_token_num`、`chunk_size_sec`（默认 2.0）、`max_context_sec`（默认 30.0）、`sample_rate`（默认 16000）。切片在 `sample_rate` 处截断，不把未读到的形参写成不存在。`feed_audio` 已读片段把 `stable_text` 更新为 `_split_stable_unstable` 的前半，并增加 `_rewrite_events`。`stable_text` 是否只增不减、`text` 里不稳定的那一段是否被换成别的字，这两句没有落在返回字段上。

Python 侧 Voxtral 的 `step` 按 `018` 返回追加片段。类注释写较短的 `transcription_delay_ms` 会让部分假设翻转，返回值里没有对应字段。

Swift 侧不是同一签名。`VoxtralRealtimeStreamSession` 的 `init` 已读参数是 `temperature`、`maxTokens`、`transcriptionDelayMs`。`step` 直接吃样本：

```swift
// 来源: https://raw.githubusercontent.com/Neywa-Labs/mlx-audio-swift/main/Sources/MLXAudioSTT/Models/VoxtralRealtime/VoxtralRealtimeStreamSession.swift
public func step(_ samples: [Float]) -> Delta
```

注释写明吃的是 16 kHz 单声道，返回值是这一次解出的文本。`Delta` 含 `text` 与 `tokenIds`。另有累计属性 `text`。文件头写尾部 partial token 由 `frozenGuardTokens` 守卫，该常数在文件里是 1。没有单独的 `feed`，也没有「改写此前某条 `Delta`」的字段。README 里的 `generateStream(audio:)` 仍是整段数组，与这个 `step` 不是同一个函数。

**存在依据：** 用户首选 Qwen，集成路径是 MLX。偏好不是调用证据。原语 4：输入是有序采样帧；每条假设标明它修订的是哪一条先前假设；更晚的假设可以替换更早假设的文本，替换停在会话里。`018` 第 2.3、2.4 节把修订目标留在适配器外缘，并写明那只是候选包法。原语 9：契约对不上时，适配器不成立，角色空着，不改其余原语的类型。

**现在是否需要补证：** 不需要把 vLLM 的整段覆盖再读一遍，当成 MLX 返回值已经回写。那条拒绝非 vLLM 的分支，`018` 已经核对。不需要调用 REA。Swift 与 Python 不是同一套 `feed` 参数，这一件源码已经对上，不必再把 README 的整段示例当成 Swift `step` 不存在。需要补的是：若要说某一份返回值满足原语 4 的修订，得有字段或对照，表明下一次输出改了上一次已经交出的字符，并标明改的是哪一条。在此之前，适配器自己持有上一条标识，仍然只是候选包法。

## 3. 快照槽位名没有落到任一已读 MLX 入口

**未知：** 原语 6 的槽位是名字到文本的映射。哪一个名字进入 Qwen 的系统提示或 `context` 字符串，哪一个名字能够进入 Voxtral，两份源码都没有槽位表。

已读落点只有这些。`018`：`mlx-audio` 的 Qwen3-ASR，`generate` 有 `system_prompt` 与 `hotwords`，`stream_transcribe` 只有 `system_prompt`；空 hotwords 时系统提示原样返回。`moona3k` 的 README 写 `context` 是空格分隔的领域词，注入系统提示；省略时系统提示为空，并写流式的 `init_streaming` 也能带这份字符串。Voxtral Python 的 `create_streaming_session`，`018` 读到的参数没有 `context`、`system_prompt`、`hotwords`。Swift `VoxtralRealtimeStreamSession.init` 这次读到的参数同样没有这些名字。

**存在依据：** 原语 4 与原语 5 的输入都含同一份上下文快照。原语 6：同一份快照可以同时交给两个流；快照不被某一个模型私有；槽位不是某一个模型的提示词格式；来源读不到时槽位缺席，快照仍然成立。用户要求的接口不依赖某一家实现的内部结构。`018` 开放问题 4 写明源码没有槽位表。

**现在是否需要补证：** 不需要新写一套提示词格式来充当这张表。空快照时，已读材料允许 Qwen 侧系统提示为空、Voxtral 侧构造参数里本来就没有快照，两条都能继续出文本。原语 4 的空快照条款不依赖槽位名。非空快照要改变 Voxtral `step` 的输出：已读参数列表里没有入口，不需要在同一份 `init` 里再找同名参数。哪些槽位名拼进 Qwen 的 `context` 或 `system_prompt`，仍然未知。未知时不把某一槽位写成已经接上。

## 4. 替换文本比原假设错误更少，还没有落到某一档 Qwen

**未知：** 哪一档 Qwen 文本权重，在什么测试集上，会使替换文本的错误少于识别假设原文。`016` 已发表的反例是 ASR-EC：Bai 等 2023 的 Qwen、参数量 7B，零样本、三样本与多步提示的 CER 都高于同表 ASR 基线。那不是 Qwen2.5，也不是 Qwen3。RAIR 正文写的是 “Qwen2.5”，没有 7B、14B 或 72B。粤语标点表的最高名字是 “Qwen-2.5-72b”，测试集是 HKCanCor，论文没有写量化、硬件或生成速度。这次检索 Qwen 文本权重上的口语顺滑、填充词、disfluency 与已发表 F1，没有落到一份点名 Qwen 文本检查点的分数表。因此生成结果与原文不同，不是优化已经成立。

**存在依据：** 用户原话要性能，首选 Qwen，用来做语音输入的文本优化。原语 5：输入是文本与上下文，没有采样帧；输出是「不替换」或「替换为这段文本」；「不替换」是正常输出，不是失败。`016` 第 5 节与第 7 节第 1 条把候选停在文本口的形状。`018` 第 3 节写明比较全文发生在适配器里，不在模型返回值里；第 6 节的反论写明，若把任何差异都交给呈现去替换，会和 ASR-EC 的已发表结果相撞。

**现在是否需要补证：** 不需要把 2023 年那张 CER 上升表再测一遍，用来证明每一档 Qwen 都不应当替换。那是另一套提示、另一档权重。需要补的是：某一档、某一测试集上的分数，才可以把该档的差异交给呈现去替换。没有这份分数时，改写停在输出类型，恒等改写仍然满足原语 5 的核对句。口语清理那次检索是空结果，不把空结果写成零分。`016` 里 Apple Silicon 上的 tok/s 是生成速度，不是这张纠错表，不并到一起。

## 检索日志

| # | 动作 | 地址或查询 | 结果 |
| --- | --- | --- | --- |
| 1 | 本地阅读 | `014`、`007` 原语 4、5、6、9、`012` | 本轮范围是开源权重与 MLX；宿主与豆包契约不重开 |
| 2 | 本地阅读 | `018` 第 2、6、7 节 | 修订指针在适配器外缘；Apple Silicon 有序帧毫秒未记；jinja 当时未打开 |
| 3 | 本地阅读 | `015` 第 4、5 节 | 论文 TTFT 与 2 秒块 WER 不是 Apple Silicon；第三方 MLX 表当时未打开 |
| 4 | 本地阅读 | `016` 第 5、7 节 | ASR-EC 反例、RAIR 无参数档；口语清理 F1 为空 |
| 5 | 本地阅读 | `017` 开放问题 | `qwen3_asr` 不进文本工厂；音频数组没有接到 ASR 注册表 |
| 6 | 网页检索 | mlx-audio Qwen3-ASR Apple Silicon latency | 命中 `moona3k/mlx-qwen3-asr` 与 Soniqo 页；只采用随后打开的 README |
| 7 | 网页检索 | Voxtral-Mini-4B-Realtime-2602-4bit benchmark | 命中模型卡；FLEURS 仍标 from upstream |
| 8 | 网页检索 | mlx-audio-swift VoxtralRealtime feed | README 示例仍是整段 `generateStream`；`feed_forward` 是权重名，不是会话入口 |
| 9 | 网页检索 | Qwen 文本 disfluency 或口语顺滑 F1 | 命中粤语标点与非 Qwen 的不流利检测；没有 Qwen 文本检查点分数表 |
| 10 | 网页检索 | site:github.com/Jacob-Zhou/simple-csc mlx | Tavily 无结果。空结果 |
| 11 | 页面读取 | `https://raw.githubusercontent.com/Jacob-Zhou/simple-csc/main/README.md` | 依赖列表是 torch 与 transformers；已读段没有 MLX。不单列缺口 |
| 12 | 页面读取 | `moona3k/mlx-qwen3-asr` README，偏移约 8000 与 20000 | M4 Pro 整段时延表；流式 primary error 无毫秒；`context` 是领域词字符串 |
| 13 | 页面读取 | 同仓库 `mlx_qwen3_asr/streaming.py` | `feed_audio` 返回 `StreamingState`；有 `stable_text`；无修订指针 |
| 14 | 页面读取 | `mlx-community/Voxtral-Mini-4B-Realtime-2602-4bit` README 原文 | 用法含 `transcription_delay_ms`；基准标题仍是 from upstream |
| 15 | 页面读取 | `mlx-audio` `docs/guides/streaming-stt.md` | 协议写明 `step` 为 append-only；点名 Nemotron 与 Voxtral |
| 16 | 页面读取 | Swift `VoxtralRealtime.swift` 与 `VoxtralRealtimeStreamSession.swift` | `step` 吃 16 kHz 样本并返回 `Delta`；`init` 无 context |
| 17 | 页面读取 | `mlx-community/Qwen3-4B-Instruct-2507-4bit` 的 `chat_template.jinja` | 无 `enable_thinking`；回放历史时可写出 think 标签 |
| 18 | 页面读取 | `https://epidemiology.tech/.../qwen3-asr-mlx-runtime-for-realtime-transcription-on-mac` | HTTP 403。空结果，不采用 |
| 19 | 目录列举 | GitHub contents `MLXAudioSTT/Models/VoxtralRealtime` | 八个文件，含 `VoxtralRealtimeStreamSession.swift` |
| 20 | 本地阅读 | `GUIDE` 4.5 | 缺口只记未知、依据、现在是否需要补证 |

第 9、10、18 条没有给出可写入正文的新分数或新调用序列。第 6 条的 Soniqo 页没有打开，不记数字。

<!-- gap-complete -->
