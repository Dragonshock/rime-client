# 022-research-context-slot-injection：五槽名字进入已有入口的落点

本文只回答 `019` 第 3 条。快照五槽是 `host_app`、`field_role`、`cursor_before`、`hotwords`、`corrections`。问的是：这些名字里，哪一个在已发表材料或本次打开的源码里，有进入 Qwen3-ASR 或 Voxtral 入口的落点。不新写提示词模板，不把没有读到的形参补上。豆包输入法只作机制对照。

`011` 第 3.1 节把这五槽定义成一次汇集的快照：名字到文本。`hotwords` 是有序词文本，`corrections` 是有序对 `(src_text, dst_text)`，另外三槽各是一段文本。缺席是「没有这个槽」，不是空字符串。本文不改这份形状，只核对公开入口有没有同名或同结构的形参。

## 1. 豆包对照：热词、修正对、前文是分开的处理函数

`003` 第 2.4 节记下 `flute::asr::ntp_cif::PromptProcessor` 的槽，与本文五槽相关的是四条分开的处理函数，不是一个系统字符串：

| 样本内部槽 | 处理函数 | `003` 写的数据 |
| --- | --- | --- |
| `HOTWORDS` | `ProcessHotwords` | `user_word` 与用户词典召回的专有名词 |
| `USER_MODIFY` | `ProcessUserModify` | `modify_pair` 的 `src_text -> dst_text` |
| `CUR_CONTEXT` | `ProcessCursorContext` | 输入框光标前后已有文本 |
| `BOT_NAME` / `BOT_DESC` | `ProcessBotName` / `ProcessBotDesc` | 助手身份或当前应用宿主场景描述 |

`011` 第 3.3 节把快照投影到这些内部槽：`hotwords` 对 `HOTWORDS`，`corrections` 对 `USER_MODIFY`，`cursor_before` 对 `CUR_CONTEXT`，`host_app` 对 `BOT_DESC`。同一节写明 `field_role` 没有同名的 `PromptProcessor` 槽。这些名字在官方进程内部。本文不把这张投影抄到 Qwen 或 Voxtral 的形参上。

`004` 第 2.2 节第 3 点是另一条路径：云端 `StartTask` 的 `extra` 里同时放了个人热词、纠错对和当前宿主应用上下文。那是一份 JSON 载荷，不是上面四个处理函数。两条豆包路径都说明：热词、修正对、前文在样本里有各自的名字。它们都不是本文打开的公开函数的形参名。

`002` 里的「槽位」是协议 witness table 的方法槽计数，不是上下文文本槽。对该问题没有可引用的注入形参。

## 2. 本次打开的入口各自接受什么文本

不同仓库的 `context` 不是同一个类型。下面按文件分开记，不并成一张跨仓库对照表。

### 2.1 论文没有五槽名字

打开 `https://arxiv.org/html/2601.21337v1`。SFT 段写：该阶段使用 non-speech data、streaming-enhancement data 和 context biasing data；并把模型训练成只做 ASR、不跟随提示里的自然语言指令。下一段写：模型学习把 system prompt 里的 context tokens 当作 background knowledge，以便得到定制的识别结果。

对这份 HTML 做不区分大小写的子串计数：`hotword`、`cursor`、`correction`、`preceding`、`host_app`、`bundle` 都是 0。`system prompt` 与 `context biasing` 各 1。论文没有给出 context 字符串的分隔符，也没有词表、修正对、光标前文、应用标识或输入框角色的字段名。

### 2.2 官方 `qwen_asr`：一个叫 `context` 的字符串

打开 `QwenLM/Qwen3-ASR` 的 `qwen_asr/inference/qwen3_asr.py`。`transcribe` 的文本形参是 `context: Union[str, List[str]] = ""`。文档写：标量会按 batch 广播；列表长度要和音频条数一致。`List[str]` 在这里是「每条音频一段上下文」，不是一条音频上的热词列表。

系统角色直接放这段字符串，没有再包一层槽名：

```python
# 来源: https://raw.githubusercontent.com/QwenLM/Qwen3-ASR/main/qwen_asr/inference/qwen3_asr.py
def _build_messages(self, context: str, audio_payload: Any) -> List[Dict[str, Any]]:
    return [
        {"role": "system", "content": context or ""},
        {"role": "user", "content": [{"type": "audio", "audio": audio_payload}]},
    ]
```

`init_streaming_state` 的文本形参同样是 `context: str = ""`，文档写 “Context string.”。该文件检索不到 `hotwords`、`system_prompt`、`host_app`、`field_role`、`cursor_before`、`corrections`。

`examples/example_qwen3_asr_transformers.py` 与 `example_qwen3_asr_vllm.py` 的调用是 `context=["", "交易 停滞", ""]`，和三条音频对齐。`example_qwen3_asr_vllm_streaming.py` 里的 `init_streaming_state` 只写了 `unfixed_chunk_num=2`、`unfixed_token_num=5`、`chunk_size_sec=2.0`，这次调用没有传 `context`。

同仓库 `README.md` 本次下载全文 60408 字符，`context` 与 `hotword` 的出现次数都是 0。README 没有这段 API 的说明；说明在上面的函数文档和示例里。

### 2.3 Hugging Face `Qwen/Qwen3-ASR-1.7B-hf`：形参名是 `prompt`

打开该模型卡的 “Context / hotwords” 一节。原文写：可以经由 `prompt` 传入自由文本，例子是 domain-specific vocabulary、names 或 background information。示例调用是 `prompt="Vocabulary: Quilter, apostle, gospel."`。同页聊天模板把同一句放进 `{"role": "system", ...}`，注释写 “Context/hotwords as system message”。

这一节把 Context 和 hotwords 写在同一个标题里。形参名不是 `hotwords`，也没有 `corrections`、`cursor_before`、`host_app`、`field_role`。`Vocabulary:` 出现在调用方写入的那句示例字符串里面。本节没有写库会自动加上这个前缀。

这是 Transformers 模型卡上的 `apply_transcription_request`，不是第 2.2 节 `qwen_asr.Qwen3ASRModel.transcribe` 的 `context` 形参。两个入口不要并成一个签名。

### 2.4 `mlx-audio` Python：`generate` 有 `hotwords`，`stream_transcribe` 没有

打开 `Blaizzy/mlx-audio` 的 `mlx_audio/stt/models/qwen3_asr/qwen3_asr.py` 与 `mlx_audio/stt/utils.py`。

`generate` 的文本形参是 `system_prompt: str | None = None` 和 `hotwords: Optional[List[str]] = None`。文档写 hotwords 是 vocabulary/hotword 列表，折进 `system_prompt`。`stream_transcribe` 的签名只有 `system_prompt`，没有 `hotwords`。

空列表不改原字符串；非空则用逗号拼到系统提示后面：

```python
# 来源: https://raw.githubusercontent.com/Blaizzy/mlx-audio/main/mlx_audio/stt/utils.py
def merge_hotwords(base: Optional[str], hotwords: Optional[List[str]]) -> Optional[str]:
    if not hotwords:
        return base
    terms = [str(t).strip() for t in hotwords if t is not None and str(t).strip()]
    if not terms:
        return base
    joined = ", ".join(terms)
    return f"{base}\n{joined}" if base else joined
```

`generate` 先调用 `merge_hotwords`。`stream=True` 时，把合并后的 `system_prompt` 传给 `stream_transcribe`。因此热词列表能进流式路径，只发生在先调用 `generate` 的时候；直接调用 `stream_transcribe` 没有 `hotwords` 这个名字。

`_build_prompt` 在 `system_prompt` 非空时写成 `f"{system_prompt}\n"`，放进 `<|im_start|>system` 与 `<|im_end|>` 之间。该文件检索不到 `host_app`、`field_role`、`cursor_before`、`corrections`。

同文件另有整数形参 `repetition_context_size`（`generate` 默认 100）。它不是快照。Swift 侧同名用途见第 2.5 节，默认值不同，不放进同一格。

### 2.5 `mlx-audio` Swift：`context: String`，没有 `hotwords`

打开 `Blaizzy/mlx-audio-swift` 的 `Sources/MLXAudioSTT/Models/Qwen3ASR/Qwen3ASR.swift`。长签名是：

`generate(audio:maxTokens:temperature:context:language:chunkDuration:minChunkDuration:repetitionPenalty:repetitionContextSize:)`

其中 `context: String = ""`。`generateStream` 同样有 `context: String = ""`。`buildPromptText` 在 `context` 为空时系统内容为空字符串，非空时把该字符串原样放进 `<|im_start|>system` 与 `<|im_end|>` 之间，没有另加前缀。

接受 `STTGenerateParameters` 的那个 `generate(audio:generationParameters:)` 调用长签名时，把 `context` 写成 `""`。本次没有打开 `STTGenerateParameters` 的定义文件：`Sources/MLXAudioSTT/STTGenerateParameters.swift` 返回 HTTP 404。因此只记录这条重载传入了空字符串，不记录该结构体有哪些字段。

该 Swift 文件检索不到 `hotword`、`systemPrompt`、`host_app`、`field_role`、`cursor_before`。`repetitionContextSize` 默认 32，函数体把它用作已生成 token 的后缀长度。这是整数窗口，不是文本槽。

### 2.6 `moona3k/mlx-qwen3-asr`：`context` 被说明成领域词字符串

打开该仓库 `README.md`、`mlx_qwen3_asr/streaming.py`、`mlx_qwen3_asr/tokenizer.py`。

README 的 “Domain vocabulary context” 写：`context` 是领域词或短语组成的字符串，注入系统提示；格式是空格分隔；省略时系统提示为空。示例是 `context="EBITDA non-GAAP FX hedging"`，以及 `init_streaming(context="EBITDA non-GAAP FX hedging")`。批量一节写 `context` 可以是每条音频一段字符串的列表。这与第 2.2 节官方 `List[str]` 的批量含义相同，与第 2.4 节 `hotwords: List[str]` 的「一条调用里多个词」不是同一个列表。

`init_streaming` 的文本形参是 `context: str = ""`，写入 `StreamingState.context`。`tokenizer.build_prompt_tokens` 的文档写 context 是领域词字符串，空字符串为默认；函数在 `context` 非空时把 `encode(context)` 放进 system 回合，不另加槽名。同文件 `build_followup_prompt_tokens` 的文档写：`context` 在 follow-up 回合目前不用，仅为了 API 一致而保留。`streaming.py` 两处调用的是 `build_prompt_tokens`，传入 `context=state.context`，不是 `build_followup_prompt_tokens`。

README 另有一句：`stable_text` is monotonic by design: corrections that would shorten already-stable prefix text are intentionally not applied。这里的 corrections 是「不要缩短已经稳定的前缀」，不是快照槽 `corrections`。

`init_streaming` 还有 `max_context_sec: float = 30.0`。函数把它乘 `sample_rate` 得到 `max_context_samples`，截的是音频样本。这不是文本槽。

这三个文件的已读片段没有 `hotwords` 形参，也没有五槽里除上述 corrections 一词以外的名字。该 corrections 已在上一段排除。

### 2.7 Voxtral：构造参数里没有这些名字

打开 `Blaizzy/mlx-audio` 的 `mlx_audio/stt/models/voxtral_realtime/voxtral_realtime.py` 与 `streaming.py`。`Neywa-Labs/mlx-audio` 上同路径的 `streaming.py` 与 Blaizzy 这份 `diff -q` 无差异。`mlx_audio/stt/models/voxtral_realtime/voxtral.py` 返回 HTTP 404；模型文件名是 `voxtral_realtime.py`。

`create_streaming_session` 的关键字参数只有：

```python
# 来源: https://raw.githubusercontent.com/Blaizzy/mlx-audio/main/mlx_audio/stt/models/voxtral_realtime/voxtral_realtime.py
def create_streaming_session(
    self,
    *,
    max_tokens: int = 4096,
    temperature: float = 0.0,
    transcription_delay_ms: Optional[int] = None,
) -> StreamingSession:
```

`VoxtralStreamingSession.__init__` 接收 `model`、`max_tokens`、`temperature`、`transcription_delay_ms`。没有 `context`、`system_prompt`、`hotwords`，也没有五槽名字。

`generate` 的关键字参数是 `max_tokens`、`temperature`、`verbose`、`stream`、`transcription_delay_ms`，另加 `**kwargs`。该文件里 `kwargs` 只出现这一次，函数体没有读取它。没有读到的名字不因为 `**kwargs` 而算作落点。

预填充用的 prompt 是音频占位，不是系统文本：

```python
# 来源: https://raw.githubusercontent.com/Blaizzy/mlx-audio/main/mlx_audio/stt/models/voxtral_realtime/streaming.py
prompt_ids = [self.model.config.bos_token_id] + [
    self.model.config.streaming_pad_token_id
] * (self._n_left + self._n_delay)
```

`streaming.py` 里名为 `context` 的局部变量出现在卷积缓存：它是样本数组的拼接，不是字符串。类注释里的 prompt length 是 `1 + n_left + n_delay`，随 `transcription_delay_ms` 变化。本次没有打开 `config.py`，不记 `n_left` 的数值。

Swift `Neywa-Labs/mlx-audio-swift` 的 `VoxtralRealtimeStreamSession.init` 参数是 `model`、`temperature`（默认 0.0）、`maxTokens`（默认 4096）、`transcriptionDelayMs`（默认 nil）。该文件检索不到 `hotword`、`systemPrompt`、`context:`、`host_app`。

Voxtral 构造参数里没有的名字，保持没有。

## 3. 五槽哪一个有落点

事实只认本次打开的论文、模型卡和源码里的名字与类型。

`hotwords` 有落点，但落点不是五槽对象，而且各入口的名字不一样。

- 同名形参只出现在 `mlx-audio` Python 的 `generate`：`hotwords: Optional[List[str]]`。空则 `system_prompt` 原样返回；非空则用 `", "` 拼进 `system_prompt`。`stream_transcribe` 自身没有这个形参。
- 官方 `qwen_asr`、moona3k、`mlx-audio` Swift 的文本形参叫 `context`，类型是一段字符串（官方与 moona 的列表是按音频条数给的，每条仍是一段字符串）。moona README 把这段字符串说明成空格分隔的领域词。官方示例值是 `"交易 停滞"`。
- `Qwen/Qwen3-ASR-1.7B-hf` 模型卡用标题 “Context / hotwords” 描述自由文本，形参名是 `prompt`。
- 论文没有 `hotword` 这个词。它写的是 system prompt 中的 context tokens，以及名为 context biasing data 的训练数据。
- Voxtral 的会话构造参数没有 `hotwords`，也没有文本 `context`。

`corrections` 没有落点。已打开的论文与上述源码都没有修正对、`src_text`、`dst_text` 形参。豆包的 `USER_MODIFY` 是另一套进程内槽名。模型卡里的 background information 是自由文本的举例，没有成对字段。moona README 里的 corrections 一词指稳定前缀不被缩短，不是这个槽。

`cursor_before` 没有落点。已打开材料没有 cursor、preceding 或光标前文形参。豆包的 `CUR_CONTEXT` 由 `ProcessCursorContext` 单独处理。Qwen 侧已读文档把 `context` / `prompt` 说明成领域词、名字或背景文本，没有写成光标之前的字段文本。

`host_app` 没有落点。已打开的公开入口没有应用标识或 Bundle ID 形参。豆包侧是 `BOT_DESC`，以及 `004` 所记 `extra` 里的宿主应用上下文。

`field_role` 没有落点。已打开的公开入口没有这个名字。`011` 已记豆包 `PromptProcessor` 也没有同名槽；本次源码没有给出另一套名字。

## 4. 推论

以下是候选，不是槽位表。

五槽里，只有 `hotwords` 能在已读材料里找到「词文本进入识别系统文本」的痕迹。同名函数只有 `mlx-audio` Python 的 `generate`。论文、官方包、moona、Swift、HF 模型卡用的是 `context` 或 `prompt`，类型是一整段字符串，不是有序词列表这个结构本身。把快照里的有序词交给哪一个入口、用空格还是逗号，源码没有一张槽位表。未知的拼接方式保持未知，不在本文补一种模板。

另外四槽在这些入口里没有字段。不把它们写进 `context`、`prompt` 或 `system_prompt` 来冒充落点。论文写该 ASR 模型被训练成不跟随提示中的自然语言指令；这句只说明论文没有把指令句定义成槽，不提供分隔符，也不证明任意句子会被当作修正对或光标前文。

空快照不依赖这些名字。官方 `context` 默认 `""`，`_build_messages` 在空字符串时系统内容仍是 `""`。moona README 写省略 `context` 时系统提示为空。Voxtral 的 `create_streaming_session` 与 Swift `init` 本来就没有文本槽，预填充是 bos 加 streaming pad。这三处都能在不接收五槽的情况下继续解出文本。这与 `019` 已写的空快照条款一致，本文不改那句。

## 5. 检索日志

| # | 动作 | 地址或查询 | 结果 |
| --- | --- | --- | --- |
| 1 | 本地阅读 | `020`、`019` 第 3 条 | 本篇只核对五槽能否进入已有 Qwen 或 Voxtral 入口 |
| 2 | 本地阅读 | `011` 第 3.1、3.3 节 | 五槽的值类型；豆包投影名；`field_role` 无同名内部槽 |
| 3 | 本地阅读 | `003` 第 2.4 节 | `HOTWORDS`、`USER_MODIFY`、`CUR_CONTEXT`、`BOT_DESC` 分函数 |
| 4 | 本地阅读 | `004` 第 2.2 节第 3 点、第 3.1 节 | 云端 `extra` 把热词、纠错对、宿主上下文放在同一份载荷 |
| 5 | 本地阅读 | `002` 开头与「槽位」检索 | 方法槽计数，不是文本注入。对该问题为空 |
| 6 | 本地阅读 | `018` 第 2 节、`015` 对论文的引用 | 已有笔记里的形参边界；本文重新打开源码核对 |
| 7 | 网页检索 | Qwen3-ASR paper context hotwords system prompt | 命中论文、`Qwen/Qwen3-ASR-1.7B-hf`、官方仓库。另有 Qwen-Audio-3.0 与 HearInContext 的摘要，未打开正文，不记数字 |
| 8 | 页面读取 | `https://arxiv.org/html/2601.21337v1` | context biasing data；system prompt 的 context tokens。五槽与 hotword 计数为 0 |
| 9 | 页面读取 | `qwen_asr/inference/qwen3_asr.py` | `context` 字符串进 system；无 `hotwords` |
| 10 | 页面读取 | 官方三份 `examples/example_qwen3_asr_*.py` | 批量示例值为 `"交易 停滞"`；流式示例这次调用未传 `context` |
| 11 | 页面读取 | `QwenLM/Qwen3-ASR` `README.md` | `context`、`hotword` 出现次数 0。空结果 |
| 12 | 页面读取 | `https://huggingface.co/Qwen/Qwen3-ASR-1.7B-hf` 的 Context / hotwords 节 | 形参名 `prompt`；示例字符串含 `Vocabulary:` |
| 13 | 页面读取 | `mlx-audio` `qwen3_asr.py`、`stt/utils.py` | `generate` 有 `hotwords`；`merge_hotwords` 用逗号；`stream_transcribe` 无该形参 |
| 14 | 页面读取 | `mlx-audio-swift` `Qwen3ASR.swift` | `context: String`；`STTGenerateParameters` 重载传入 `""`；无 hotword |
| 15 | 页面读取 | `moona3k/mlx-qwen3-asr` README、`streaming.py`、`tokenizer.py` | `context` 为领域词字符串；`max_context_sec` 是音频窗 |
| 16 | 页面读取 | `voxtral_realtime.py`、`streaming.py`；Neywa 同路径 `streaming.py` | 构造参数无文本槽；prefill 为 bos 与 pad。两份 streaming 无差异 |
| 17 | 页面读取 | Swift `VoxtralRealtimeStreamSession.swift` | `init` 为 temperature、maxTokens、transcriptionDelayMs |
| 18 | 页面读取 | `.../voxtral_realtime/voxtral.py` | HTTP 404。空结果 |
| 19 | 页面读取 | `STTGenerateParameters.swift` 猜测路径 | HTTP 404。空结果 |
| 20 | 网页检索 | GitHub code search `STTGenerateParameters` | HTTP 401。空结果 |

第 7 条未打开的第三方论文不作为槽位落点。第 11、18、19、20 条没有新的形参名。

<!-- research-complete -->
