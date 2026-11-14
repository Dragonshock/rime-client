# 018-derivation-open-model-mlx-binding：开源权重与 MLX 调用序列对识别流、改写流、绑定的对应

本文只做一件事：把已经核对过的模型与 MLX 调用序列，对应到 `007` 原语 4（识别流）、原语 5（改写流）、原语 9（绑定）。用户偏好 Qwen。偏好不是调用证据。某一档输入在已读的 Qwen MLX 函数里不存在时，写另一件开源制品，标明候选，并写舍弃了什么。

原语 4 的对外输入仍是有序采样帧、上下文快照、会话标识；对外输出仍是带修订目标的文本假设。原语 5 的对外输入仍是一条假设文本和同一份快照；对外输出仍是「不替换」或替换文本，两者都留下原假设文本。MLX 的数组、流和容器类型只出现在适配器内部。豆包输入法只作机制对照，不作为绑定。不重开 `013`。

`015`、`016`、`017` 已经记下的错误率、tok/s 与加载顺序，本篇不并成新表，也不补没有来源的时延。

## 1. 对应时用到的契约

原语 4：输入没有麦克风设备，也没有宿主写入句柄。更晚的假设可以替换更早假设的文本，替换停在会话里。快照为空仍要产出假设。输出可以另带「分句已定稿」，供触发消费；输出不是提交文本。

原语 5：输入没有采样帧。改写实现换成恒等（始终「不替换」）时，识别流不用改。模型身份不出现在输出里。

原语 9：输入是角色、当时可用的制品、该制品声称能接受的输入类型。输出是满足原语 4 或原语 5 的适配器。一件制品可以因为实际输入类型只能担任其中一个角色。重新配对不改变这两条的输入输出类型。契约对不上时，角色空着，不改其余原语的类型。

`017` 已经核对：`mlx_lm.load` 与 `LLMModelFactory` 按 `config.json` 的 `model_type` 选文本模块，前向吃 token。`qwen3_asr` 不在那两份类型表里。语音权重走另一个包。本篇沿用这个边界，不把识别权重说成已经走通文本生成循环。

## 2. 识别流

识别流对外只有一条类型。已读到的 MLX 函数有两套，输入形状不同。它们是同一角色的两个候选配对，不是叠成一条流水线。

### 2.1 Qwen3-ASR：整段音频进，文本出

已读入口在 `mlx-audio`，不在 `mlx-lm`。README 列出的权重名包括 `mlx-community/Qwen3-ASR-0.6B-8bit` 与 `mlx-community/Qwen3-ASR-1.7B-8bit`。Swift README 另外列出这两档的 bf16、8bit、6bit、4bit。本篇不在这些名字里选一档。`015` 的 WER 表没有 Apple Silicon 时延，不能用来把某一档写成已经配对的识别实现。

```python
# 来源: https://raw.githubusercontent.com/Blaizzy/mlx-audio/main/mlx_audio/stt/models/qwen3_asr/README.md
from mlx_audio.stt import load
model = load("mlx-community/Qwen3-ASR-0.6B-8bit")
result = model.generate("audio.wav", language="English")
for event in model.stream_transcribe("audio.wav", language="English"):
    piece = event.text
```

这段 README 的两个调用都先要一条音频路径。其后的源码边界是：

- `generate` 的签名还接受 `mx.array`、`np.ndarray`，以及二者的列表。列表逐条串行。`stream=True` 时签名写明不接受列表。`stream=True` 的生成器本体本篇没有逐行读完。
- `stream_transcribe` 若收到路径，先 `load_audio` 成整段，再按 `chunk_duration`（默认 1200 秒）切开。已读循环对每个 token 取 `StreamingDetokenizer.last_segment`，yield 的 `text` 是这次新增片段，不是「指向先前假设」的字段。块结束时再 yield 一次 `text=""`，`is_final` 只在最后一块为真，并带上这块音频的起止时间。这是「给定的整段已经切完」，不是会话在说话过程中发出的定稿信号。
- `generate` 有 `system_prompt` 与 `hotwords`。`merge_hotwords` 在 `hotwords` 为空时原样返回 `system_prompt`；非空时用逗号拼进系统提示。`stream_transcribe` 的签名只有 `system_prompt`，没有 `hotwords`。
- `_build_prompt` 把 `system_prompt` 放进 `<|im_start|>system`。`language` 非空时，助手侧前缀是 `language {语言名}<asr_text>`。`sample_rate` 属性返回 16000。
- 返回对象在 README 里是 `STTOutput.text` 与 `segments`，或流式事件的 `text`。没有会话标识，没有修订目标。

Swift README 的顺序是 `loadAudioArray(from: audioURL)`，再 `Qwen3ASRModel.fromPretrained`，再 `generate(audio:language:)` 或 `generateStream`。`generateStream` 的事件是 `.token` / `.result` / `.info`。示例把 `.token` 当文本片段打印。音频来自文件 URL 读出的整段数组，不是一边到达一边 `feed`。

和原语 4 对得上的，是「波形进、文本出」，以及系统提示这一个文本槽可以带走快照里的某些字符串。对不上的，是有序帧在到达过程中被推入、会话标识、修订目标。这三件留在适配器外缘：适配器可以先把帧攒成该函数要的数组，再把返回文本包成假设。攒到多长、由谁决定这块数组已经齐，源码没有给出会话字段。这是候选包法，不是权重自己的输出。

`Qwen3-ForcedAligner` 的 `generate` 同时要音频和已经写好的文本，返回词级时间。它不从帧单独产生假设，也不返回「不替换」或替换文本。两个角色都舍弃这件制品。

### 2.2 同一权重族里，可修订文本出现在 vLLM，不在上面的 MLX 函数

官方 `Qwen3ASRModel.init_streaming_state` 在 `backend != "vllm"` 时抛出 “Streaming ASR is supported only for vLLM backend”。`streaming_transcribe` 接受任意长度的 16 kHz 单声道 PCM，攒满 `chunk_size_sec` 后把迄今整段音频再送进模型，并覆盖 `state.text`。前 `unfixed_chunk_num` 块不用上文做前缀；其后从已解码 token 回退 `unfixed_token_num` 个再当前缀。`context` 进入系统提示。`finish_streaming_transcribe` 把尾巴再解码一次，同样覆盖 `state.text`。示例文件写明流式不做 forced aligner。

```python
# 来源: QwenLM/Qwen3-ASR qwen_asr/inference/qwen3_asr.py 的 init_streaming_state
if self.backend != "vllm":
    raise ValueError(
        "Streaming ASR is supported only for vLLM backend (backend='vllm')."
    )
state = self.init_streaming_state(
    context=context, language=language,
    unfixed_chunk_num=2, unfixed_token_num=5, chunk_size_sec=2.0)
self.streaming_transcribe(pcm16k, state)
```

`state.text` 是这一块解码之后的最新整段文本，不是追加片段，也不是假设标识。回退 token 再当前缀，是「前文还可以变」的机制。论文 4.5 节的评测协议（2 秒一块、5 token 回退、最后四块不固定）与这里的参数是同一族描述；`015` 已记，硬件未点名。

这条序列舍弃作为 MLX 绑定：函数自己拒绝非 vLLM 后端。它只说明同一套 Qwen3-ASR 权重在另一条运行时上怎样覆盖 `state.text`。不把 `mlx-audio` 的 `last_segment` 说成已经实现了这段回退。

### 2.3 Qwen 的 MLX 源码填不上的输入：到达中的帧

`mlx_audio/stt/streaming.py` 的 `StreamingSession` 约定：`feed(samples)` 只排队，不做推理；`step` 返回 append-only 文本片段；`close` 之后还要继续 `step` 直到 `done`。指南写 Nemotron 与 Voxtral Realtime 实现了这个协议。在 `mlx_audio/stt/models/qwen3_asr/qwen3_asr.py` 里检索 `create_streaming_session`，空结果。

因此，原语 4 里「帧还在到达」这一档输入，已读的 Qwen3-ASR MLX 函数填不上。候选改到另一件开源制品：`mlx-community/Voxtral-Mini-4B-Realtime-2602-4bit`（模型卡写由 `mistralai/Voxtral-Mini-4B-Realtime-2602` 转成 MLX，`model_type` 为 `voxtral_realtime`）。同页还列出 fp16 变体；Swift README 另有 6bit。本篇不在量化档之间选择。模型卡把 License 写成 Apache 2.0。这是该页的字段，不是本篇打开过的 LICENSE 全文。

```python
# 来源: Neywa-Labs/mlx-audio mlx_audio/stt/models/voxtral_realtime/streaming.py 类注释
sess = model.create_streaming_session(temperature=0.0)
sess.feed(samples_np)
deltas = sess.step(max_decode_tokens=4)
sess.close()
```

已读实现的边界：

- `create_streaming_session` 的参数是 `max_tokens`、`temperature`、`transcription_delay_ms`。没有 `context`、`system_prompt`、`hotwords`，也没有会话标识。
- `feed` 把样本收成一维 `float32` 放进队列。`input_sample_rate` 取 `audio_encoding_args.sampling_rate`；配置默认值是 16000。
- `step` 返回 `list[str]`。协议文档写这是 append-only 片段。已读的 `_decode_some` 把 `StreamingDetokenizer.last_segment` 放进这个列表，没有修订指针。
- 类注释另写：较小的 `transcription_delay_ms` 会让 partial hypotheses 更常翻转。配置默认 480。注释里的翻转没有对应到返回值里的一个字段。已读循环只追加片段。
- `done` 在 EOS、达到 `max_tokens`，或关闭后把尾巴解完时置位。这可以候选地映射成原语 4 的「分句已定稿」。映射发生在适配器，函数本身不接收会话。

舍弃了什么：

- 舍弃把 Qwen3-ASR 的 `stream_transcribe` 当作 `feed`。它先装入整段音频，且源文件没有 `create_streaming_session`。
- 舍弃把第 2.2 节的 vLLM 函数当作 MLX 路径。
- 指南同时点名 Nemotron。本篇没有读它的 `streaming.py`，所以不把它写成已核对的调用序列。这不是两件制品的分数比较。
- Swift 的 Voxtral README 示例仍是 `loadAudioArray` 之后的 `generate` / `generateStream`，事件同样是 `.token` / `.result` / `.info`。发布说明里有 incremental streaming 的提交标题。本篇没有打开那份 Swift `feed` 源码，不把 Python 的 `feed` 写成 Swift 已经同一签名。

和原语 4 对得上的，是到达中的单声道 PCM 可以推进一次解码并吐出文本片段。对不上的，是非空快照没有参数可进，修订目标仍要适配器另编，返回类型按协议是追加而不是覆盖前文。

模型卡有一节 “Benchmarks (from upstream)”，FLEURS 上按 delay 列出 WER，其中 ZH 一列。该节没有写这些格子是不是 int4 MLX 文件测出来的，已读段落也没有写硬件。本篇不把那些格子抄进来当作这件 MLX 权重的成绩，也不拿去和 `015` 的 Qwen3-ASR 表比。

### 2.4 修订目标与会话标识

两套 MLX 返回值都没有「这条假设修订哪一条」。适配器若要满足原语 4，只能自己持有本会话上一条假设的标识与文本：

- 制品给出整段最新文本时（第 2.2 节的 `state.text`，该路径不进入 MLX 绑定），新假设的文本用这份整段，修订目标指向上一条。
- 制品只给出追加片段时（`stream_transcribe` 的 `last_segment`，Voxtral 的 `step`），候选是把「上一条文本接上片段」当作新假设的文本，修订目标仍指向上一条。

这是适配器侧的对应。它不证明模型会改掉已经吐出的字。快照为空时，Qwen 的系统提示可为空字符串，Voxtral 的会话构造本来就没有快照参数，两条都能继续吐文本。非空快照只有 Qwen 的 `system_prompt` / `hotwords` 有落点；Voxtral 这条没有。槽位是名字到文本的映射，不是系统提示。哪些槽位拼进 `system_prompt`，两份源码都没有槽位表。

16000 这个数是上述制品写明的采样率，不是原语 3 的不变量。帧上的格式与这个数不一致时，转换发生在适配器内部。

## 3. 改写流

改写流吃文本。`017` 已核对的文本调用序列是：`mlx_lm.load` 或 `LLMModelFactory.loadContainer`，再按聊天模板得到 token，再 `stream_generate` 或 `ModelContainer.generate`。`model_type` 为 `qwen2`、`qwen3`、`qwen3_5` 的模块，前向参数是 token 或 `input_embeddings`，不接收波形。

```python
# 来源: https://qwenlm.github.io/blog/qwen3 的 apply_chat_template 片段
text = tokenizer.apply_chat_template(
    messages, tokenize=False,
    add_generation_prompt=True, enable_thinking=False)
```

这段只说明官方示例能把思考开关关掉，再交给生成。它不返回「不替换」。生成结果是一段文本，思考开关关掉也不保证这段文本等于原假设，也不保证只改标点。

适配器侧的对应是：原假设文本留在适配器里；生成结束后与原文比较。相同则输出「不替换」；不同则输出「替换为这段文本」。两条输出都还能读到原假设。比较不在模型返回值里。`stream_generate` 的中间 `text` 可以是不完整片段，`finish_reason` 尚未置位时不能把片段当成替换文本。Swift 的 `.chunk` 同样是片段；`.info` 不是替换结果。这些类型不出适配器。

`016` 已记一条反例：ASR-EC 上，Bai 等 2023 的 Qwen、参数量 7B，零样本、三样本与多步提示的 CER 都高于同表 ASR 基线。因此「字符串不同」不是「应当替换」的测量结论。候选只到输出类型能被填上。恒等改写（始终「不替换」）不需要调用这个生成循环，原语 5 的核对句仍然成立。

思考 token 若被当成替换文本，会进入宿主可见的字符串。`016` 记：`Qwen/Qwen3-4B-Instruct-2507` 的 README 写明不生成 `<think>` 块；原版 `Qwen3-4B` 的默认示例是 `enable_thinking=True`。检索到 mlx-swift-lm issue 154 的摘要：社区包 `mlx-community/Qwen3-4B-Instruct-2507-4bit` 的 jinja 模板不一定包含 `enable_thinking` 分支。本篇没有打开那份 jinja 对拍。候选是：替换文本只取模型卡写明不再生成思考块的指令权重的最终回答；摘要所述的模板缺口未对拍之前，不把 Swift 的 `additionalContext` 写成已经关掉思考。

`016` 的体积与 tok/s 分属不同硬件、不同量化、不同提示长度。本篇不据此选择 0.6B、4B 或 27B。调用序列对已注册的 `model_type` 是同一形状。`qwen3_5` 的文本 `load` 会丢掉视觉键；磁盘上的整包不是纯文本切片。这不改变改写流的对外类型，只说明适配器内部装入的字节含视觉侧。

舍弃把下列制品当成改写流：

- Qwen3-ASR 的 `generate`：签名要求音频。`context` 在官方 `transcribe` 里与音频一起进系统角色，不是「只有一条假设文本」。
- ForcedAligner：输出是时间戳。
- `simple-csc`：仓库说明走的是 Base 权重的分数与束搜索，示例加载 `Qwen/Qwen2.5-1.5B`，不是 `apply_chat_template` 再 `stream_generate`。本篇没有读到它的 MLX 调用序列。
- Qwen2-Audio 与 Omni：`015` 记它们的已核对入口吃音频，并且跟随指令。`017` 的文本类型表没有把它们接成改写流。

## 4. 绑定

适配器对外没有 `MLXArray`、`ModelContainer`、`Generation`、`StreamingSession`、`STTOutput`。这些留在内部。对外只有原语 4 或原语 5。

| 角色 | 已核对的制品与调用 | 制品实际接受的输入 | 适配器补上的、制品没有的字段 |
| --- | --- | --- | --- |
| 识别流，波形已经成段，快照文本要进系统提示 | `mlx_audio.stt.load` 的 Qwen3-ASR，`generate` / `stream_transcribe`；Swift 为 `Qwen3ASRModel.fromPretrained` 后的 `generate` / `generateStream` | 路径、`mx.array` 或 `np.ndarray`；Swift 示例是文件读出的数组。`generate` 另有 `system_prompt` 与 `hotwords` | 会话标识；修订目标；帧格式到 16000 的转换；把快照收成系统提示 |
| 识别流，帧还在到达 | Python `VoxtralStreamingSession.feed` / `step` / `close` | 单声道 float32 PCM，采样率取配置，默认 16000 | 会话标识；修订目标。非空快照没有参数 |
| 改写流 | `mlx_lm` 的 `load` + 聊天模板 + `stream_generate`，或 `LLMModelFactory.loadContainer` + `generate` | token 化之后的文本消息 | 「不替换」或替换文本；原假设文本的保留；思考块不进入替换文本 |

候选配对是按这张表的输入类型换角色，不是按「在本机」换角色。Qwen 文本权重填不上识别流，因为已读前向不吃波形。Qwen3-ASR 填不上改写流，因为已读 `generate` 要音频。Voxtral 的 `feed` 填的是 Qwen3-ASR MLX 源码里没有的那一档识别输入；它不因此兼任改写流，构造参数里没有假设文本。

一次会话选定一行识别制品和一行改写制品。把 Qwen3-ASR 与 Voxtral 串成前后两级识别，已读源码里没有这种组合，本篇不把它写成配对。0.6B 与 1.7B、各量化档，都不在本篇选定；它们共享各自那一行的调用形状。

绑定不打开官方进程里的容器，不取得鉴权材料。这里的制品是公开仓库上的权重与公开包里的函数，适配器只调用这些函数。

## 5. 豆包只作机制对照

`016` 引用 `003`：闭源样本里有算子名 `W4A8ImeAsrQwen3Transformer`，位于识别解码一侧。该名字不是本篇任一公开仓库 id，不进入上表。样本怎样组帧、怎样改字，只说明「识别与改写可以分开」这件事在 `007` 里已经用过；不把样本的内部调用写成 MLX 序列。公开源码与模型卡已经给出本节的函数，本篇没有调用 REA。

## 6. 当前倾向（候选）

候选倾向如下。

会话建立时，绑定按制品声称的输入类型接角色。识别流若要在 MLX 上消耗一条已经成形的波形，并把快照中的文本放进系统提示，候选制品是 `mlx-audio` 的 Qwen3-ASR。识别流若要在帧到达时调用 `feed` 再 `step`，已读的 Qwen3-ASR 源码没有这个函数，候选制品换成 Voxtral Realtime 的 `VoxtralStreamingSession`。改写流的候选制品是 `mlx-lm` 或 mlx-swift-lm 能按 `qwen2` / `qwen3` / `qwen3_5` 加载的 Qwen 指令权重；「不替换」由适配器比较全文得到，不由模型返回。两行识别候选互相替换时，原语 4 与原语 5 的对外类型不变。用户对 Qwen 的偏好只解释第一行与改写行为什么先核对 Qwen 仓库；它不把 Voxtral 那一档输入写成 Qwen 已经具备。

反论：Voxtral 的协议写明 `step` 是追加片段，类注释又写短 delay 时部分假设会翻转。若产品语义是「已经显示的字会被后来的音频改掉」，已读到的覆盖整段 `state.text` 的实现在 vLLM 上，并且拒绝非 vLLM。适配器把追加片段包成「新假设替换上一条」之后，并没有证据表明模型还会改前文。这条包法因此只是候选。

另一反论：ASR-EC 上提示式 Qwen-7B 的 CER 高于识别原文。改写候选若把任何差异都交给呈现去替换，会和这条已发表结果相撞。候选停在输出类型，不把差异写成优化已经成立。

## 7. 开放问题

1. Swift 版 Voxtral 的 incremental 提交是否提供与 Python `feed` 相同的参数：README 示例没有；源码未读。
2. Nemotron 的 `StreamingSession` 实现未读。不把它与 Voxtral 比错误率。
3. Qwen3-ASR `generate(..., stream=True)` 的 yield 形状未逐行读。已读的流式函数是 `stream_transcribe`。
4. 快照槽位名与 `system_prompt` / 聊天消息的对应：源码没有槽位表。
5. MLX 追加片段被适配器包成修订假设之后，模型是否还会改已吐出的字：返回值里没有这种回写。
6. mlx-swift-lm issue 154 的摘要说社区 jinja 可能没有 `enable_thinking`。jinja 文件未打开。
7. Qwen3-ASR 与 Voxtral 在 Apple Silicon 上、针对有序帧的端到端毫秒：本篇打开的论文与 MLX 源码都没有这份测量。Voxtral 卡上的 upstream FLEURS 表不记成 int4 MLX 成绩。
8. 同一进程里识别适配器与改写适配器是否交错使用同一条 MLX stream：`017` 分别描述了文本生成的 stream，本篇没有做交错实验。

## 8. 检索日志

| # | 动作 | 地址或查询 | 结果 |
| --- | --- | --- | --- |
| 1 | 本地阅读 | `007` 原语 4、5、9；`014` 对 018 的范围 | 输入输出以 007 为准；018 只做对应 |
| 2 | 本地阅读 | `015` 第 2、4、5 节 | Qwen3-ASR 与 vLLM 流式协议、整段与流式的差别；无 Apple Silicon 时延 |
| 3 | 本地阅读 | `016` 第 1、5 节与开放问题 | 文本口形状、思考开关、ASR-EC 反例；口语清理 F1 为空 |
| 4 | 本地阅读 | `017` 全文 | `qwen3_asr` 不进 `mlx_lm` / `LLMModelFactory`；文本 `stream_generate` 的字段 |
| 5 | 网页检索 | mlx-audio Qwen3-ASR generate streaming | 命中 mlx-audio README、HF 8bit 卡；第三方 Mac 运行时文未采用 |
| 6 | 网页检索 | Qwen3-ASR streaming_transcribe unfixed | 命中官方示例、`qwen3_asr.py`、vLLM issue 标题 |
| 7 | 网页检索 | mlx-audio whisper generate | 命中文件级 `generate`；与到达中的帧无关，不作为本篇候选 |
| 8 | 网页检索 | mlx-swift-lm enable_thinking | 命中 issue 154 摘要；jinja 未打开 |
| 9 | 页面读取 | mlx-audio `qwen3_asr/README.md` | `load`、`generate`、`stream_transcribe`、ForcedAligner 要文本 |
| 10 | 页面读取 | mlx-audio-swift `Qwen3ASR/README.md` | `fromPretrained`、`generate`、`generateStream` 吃文件数组 |
| 11 | 页面读取 | `example_qwen3_asr_vllm_streaming.py` | 16 kHz 切片推进 `state.text`；注释写 streaming 仅 vLLM |
| 12 | 页面读取 | 官方 `qwen_asr/inference/qwen3_asr.py` 的 `transcribe`、`init_streaming_state`、`streaming_transcribe`、`finish_streaming_transcribe` | 非 vLLM 抛错；`context` 进系统提示；`state.text` 被覆盖 |
| 13 | 页面读取 | GitHub contents `mlx_audio/stt/models/qwen3_asr` | 文件为 `qwen3_asr.py` 与 `qwen3_forced_aligner.py` |
| 14 | 本地阅读 | 下载的 `qwen3_asr.py`（mlx-audio）`generate`、`stream_transcribe`、`_build_prompt`、`sample_rate` | 先整段加载；`last_segment`；16000；无 `create_streaming_session` |
| 15 | 页面读取 | `mlx_audio/stt/utils.py` 的 `merge_hotwords` | 空 hotwords 为无操作；非空则逗号拼进系统提示 |
| 16 | GitHub code search API | `class StreamingResult` repo 搜索 | HTTP 401，空结果 |
| 17 | 页面读取 | `mlx_audio/stt/streaming.py` 与 `docs/guides/streaming-stt.md` | `feed` / `step` 协议；指南点名 Nemotron 与 Voxtral；`step` 为 append-only |
| 18 | 页面读取 | `voxtral_realtime/streaming.py` 的 `VoxtralStreamingSession` | `feed` 只排队；`step` 返回片段；类注释写短 delay 时假设会翻转 |
| 19 | 页面读取 | `voxtral_realtime.py` 的 `create_streaming_session` 与 `config.py` | 无 context 参数；`sampling_rate` 默认 16000；`transcription_delay_ms` 默认 480 |
| 20 | 网页检索 | mlx-community Voxtral-Mini-4B-Realtime-2602 | 命中 4bit 与 fp16 模型卡 |
| 21 | 页面读取 | `https://huggingface.co/mlx-community/Voxtral-Mini-4B-Realtime-2602-4bit` | 卡上写 int4、13 种语言含中文、License Apache 2.0；FLEURS 表标明 from upstream，未记成 int4 成绩 |
| 22 | 网页检索 | mlx-audio-swift VoxtralRealtime | 命中仓库目录与发布说明标题 |
| 23 | 页面读取 | Swift `VoxtralRealtime/README.md` | 示例仍是整段 `generate` / `generateStream`，没有 `feed` |
| 24 | 网页检索摘要中的 Yahoo 财经页 | Qwen3 MLX 适配新闻标题 | 无调用序列，未采用 |
| 25 | 本地检索 | 下载的 mlx-audio `qwen3_asr.py` 全文 `create_streaming_session` | 空结果 |

第 16 条与第 25 条是空结果。第 5 条的第三方时延、第 21 条的 upstream WER、第 24 条的新闻页不写入正文数字。

<!-- derivation-complete -->
