# 021-research-hypothesis-revision-pointer：下一次输出会不会改掉已经交出的字

本文只填 `019` 第 2 条。问题是：下一次输出会不会改掉已经交出的字符；若会，返回值里有没有「改的是哪一条」。对照对象是 `002` §4.1 的 `ASRTypeWriter` 公共前缀。豆包只作机制对照。

适配器自己记住上一条假设的标识，是调用方的包装。下面三份返回值都没有这个字段。本文不把那层包装写成模型已经返回修订指针。`018` 第 2.4 节把「适配器持有上一条」标成候选包法；这里只核对返回值里实际有什么。

本次没有跑音频，没有一条实测转写。下面凡是「会改字」，都是读函数体得到的数据流，不是一次转写对照。本次没有调用 REA。

## 1. 事实：moona3k `feed_audio` 交回整段状态，尾部按 token 回退

打开的全文是 `https://raw.githubusercontent.com/moona3k/mlx-qwen3-asr/main/mlx_qwen3_asr/streaming.py`。文件头写：每来一块，就把累积窗口重新编码，先前文本减去最后 `unfixed_token_num` 个 token，再把剩余 token 当前缀；并写 partial results are stable。同文件常量 `UNFIXED_CHUNK_NUM = 2`、`UNFIXED_TOKEN_NUM = 5`。

`feed_audio(pcm, state, model=None)` 的返回类型是 `StreamingState`，不是一段增量，也不是假设标识。空 PCM 直接把原 `state` 返回。凑满一块之后，先记下 `prev_text = state.text`，再 `_decode_and_update`。`state.text` 的数据类注释是当前整段：已提交窗口文本拼上活动窗口文本。

回退发生在 `_decode_window`。活动窗口的块计数 `_window_chunk_id` 小于 `unfixed_chunk_num` 时，前缀 token 列表是空的，这一次整段窗口文本不带旧文本前缀。否则调用 `_rollback_prefix_ids`，丢掉末尾 `unfixed_token_num + _commit_extra_rollback` 个 id；若切点落在替换字符 U+FFFD 上，再往前退，直到解码结果不再含这个字符。新生成的 id 接在这条前缀后面，写回 `_window_raw_ids`，再经 `parse_asr_output` 得到窗口文本。`state.text` 由 `_join_texts(committed_text, window_text, language)` 拼成。有空格的语言在两段之间加一个空格，`is_no_space_language` 为真时不加。

因此，调用方若把上一次的 `state.text` 整段已经交出，下一次仍是一整段新的 `state.text`。被回退的那些 token 不在强制前缀里，下一次解码可以换成别的 id，对应字符就可以不同于上一次整段的尾部。窗口开头的 `unfixed_chunk_num` 块（默认 2）连前缀都不加，活动窗口文本可以整段重写。文件头的 “partial results are stable” 指的是「前缀被强制、只生成新尾」这套做法。本次没有测量，不能把这句话写成某一条音频上前缀字符没变。

改字被记成次数，不记成「哪一条」：

```python
# 来源: https://raw.githubusercontent.com/moona3k/mlx-qwen3-asr/main/mlx_qwen3_asr/streaming.py 的 feed_audio
        if state.text != prev_text:
            state._text_updates += 1
        if prev_text and state.text and not state.text.startswith(prev_text):
            state._rewrite_events += 1
```

`not startswith` 在尾部被回退重写时就会成立，并不区分「只动了末尾五个 token」和「更早的字符也变了」。`_rewrite_events` 是会话上的整数。`streaming_metrics` 把它和 `rewrite_rate`（`rewrite_events / text_updates`，无更新时为 0.0）放进诊断字典，键还有 `chunks_processed`、`text_chars`、`stable_chars`、`partial_stability`、`finalization_delta_chars`、`commit_silence_events`。没有假设标识，没有被改字符串的下标，没有编辑区间。

`stable_text` 的数据类注释写 Text considered stable (won't change)。函数体不是公共前缀：

```python
# 来源: 同一 streaming.py 的 _split_stable_unstable
    if len(units) <= unfixed_tokens:
        return prev_stable, new_text
    stable_units = units[:-unfixed_tokens]
    stable = joiner.join(stable_units)
    if len(stable) < len(prev_stable):
        stable = prev_stable
    return stable, unstable
```

有空白时按空白切分，否则按字符切。新稳定串更短才留旧串。新串长度不少于旧串时，直接换成新串，不检查它是不是旧串的前缀。`feed_audio` 在 `chunk_id < unfixed_chunk_num` 时不调用这个切分，`stable_text` 保持调用前的值（初始是空串）；否则用切分结果覆盖。这和 `_window_chunk_id` 不是同一个计数：前者在 `feed_audio` 每处理一块加一，后者在每次 `_decode_window` 加一。提交窗口时的额外解码会加后者，不会在同一次调用里加前者。

`committed_text` 的注释写：已经刷出 `audio_accum` 的窗口文本，never re-decoded。`_commit_window` 把它设成与当时 `window_text` 的拼接，然后清空窗口文本、音频和 `_window_raw_ids`。刷出之前，若开着 `commit_at_silence`（`init_streaming` 默认 True），会先把窗口截到静音切点，按带出音频的时长追加回退再解码一次。追加量是 `_rollback_for_carry`：带出秒数乘 `_COMMIT_ROLLBACK_TOKENS_PER_SEC`（10.0）再向上取整。静音判定用 `_COMMIT_DIP_RATIO = 0.5`。这次额外解码仍然更新整段 `state.text`，仍然没有「改了哪一条」的字段。

`finish_streaming` 把 `stable_text` 设成当时的 `state.text`，并把 `_finalization_delta_chars` 设成结束文本长度减去进入函数时的长度。这是字符数差，不是被修订假设的标识。长度差的公式允许负数；本次没有样本说明它是否出现过。

同文件末尾的 `_append_chunk_text` 注释写：新段若像是对当前文本的整段改写，就替换而不是拼接。本次通读 `feed_audio`、`finish_streaming`、`_decode_and_update`、`_commit_window`，都没有调用它。`https://raw.githubusercontent.com/moona3k/mlx-qwen3-asr/main/mlx_qwen3_asr/__init__.py` 的 `__all__` 也没有这个名字。仓库内其他文件是否引用：检索 `_append_chunk_text` 无结果。不把它写成 `feed_audio` 的返回路径。

`Session.feed_audio`（`session.py`，同一仓库）只是把已加载模型传进 `streaming_mod.feed_audio`，返回类型仍是 `StreamingState`。`Session.init_streaming` 的默认值与模块常量一致：`unfixed_chunk_num=2`、`unfixed_token_num=5`、`chunk_size_sec=2.0`、`max_context_sec=30.0`、`sample_rate=16000`。`Session.transcribe` 返回一次 `TranscriptionResult`。本次没有打开这个数据类的字段表，不把离线结果写成流式修订指针。

`StreamingState` 上与文本有关、本次读到的名字是：`text`、`committed_text`、`window_text`、`stable_text`、`context`、`language`、`_text_updates`、`_rewrite_events`、`_pre_finalize_text`、`_finalization_delta_chars`、`_window_raw_ids`。没有 `hypothesis`、`revision`、`replaces` 这一类名字。

## 2. 事实：mlx-audio 的 Qwen3-ASR 交出新切片，不回写上一条

本次打开 `https://raw.githubusercontent.com/Blaizzy/mlx-audio/main/mlx_audio/stt/models/qwen3_asr/qwen3_asr.py` 的开头。`StreamingResult` 的字段是 `text`、`is_final`、`start_time`、`end_time`、`language`（默认 `"en"`）、`prompt_tokens`、`generation_tokens`。注释写 `text` 是这一次发射的解码文本；`is_final` 表示这一次是不是最终结果；时间戳对文本增量是 `None`，token 计数只在最终结果上设置。没有修订目标，没有上一条的标识。

本机 `/tmp/qwen3_asr_mlx.py` 是 64024 字节，开头与上述 raw 一致。`stream_transcribe` 读自这份全文。它先把路径交给 `load_audio`。`chunk_duration` 默认 1200.0 秒，`min_chunk_duration` 默认 1.0 秒。循环里对每个 token 取 `StreamingDetokenizer.last_segment`，非空才 yield。yield 的 `text` 是这个切片，`is_final=False`，时间为 `None`。块内 `finalize` 后再 yield 一次可能剩余的切片，仍是 `is_final=False`。每块结束再 yield 一次 `text=""`，`is_final` 只在最后一块为真，并带上这块的起止秒和累计 token 数。已读循环不修改先前 yield 出去的对象。

非流式 `generate` 在 `stream=False` 时返回 `STTOutput`。本机 `/tmp/mlx_stt_base.py` 的字段是 `text`、`segments`、`language`、`prompt_tokens`、`generation_tokens`、`total_tokens`、`prompt_tps`、`generation_tps`、`total_time`。`_generate_one` 的返回处把多块文本用空格拼成一个 `text`，`segments` 是每块的文本、语言和起止。这是整段结果，不是「这一条改了上一条」。`stream=True` 时 `_generate_one` 直接转去 `stream_transcribe`。

在本机这份 `qwen3_asr` 全文检索 `create_streaming_session`，无匹配。`mlx_audio/stt/streaming.py`（本机 `/tmp/mlx_stt_streaming.py`，43 行）的协议把 `step` 写成返回 append-only 文本片段。Qwen3-ASR 这份文件不实现该协议。

`last_segment` 打开的是 `https://raw.githubusercontent.com/Blaizzy/mlx-audio/main/mlx_audio/lm/generate.py`。`StreamingDetokenizer` 按 tokenizer 后端二选一，两边的 `last_segment` 都是：取当前累积文本从 `offset` 起到末尾的切片，再把 `offset` 设成这段文本的长度。后端路径把 `DecodeStream.step` 的返回值追加到 `_text`。有界回退路径会重解尚未提交的 token；只有在「前缀加后缀等于整段解码」且两边都不含 U+FFFD 时，才把前缀提交进 `_text`。不完整的 U+FFFD 后缀和（在 `clean_up_tokenization_spaces` 为真时）末尾空格会在 `text` 属性里先拿掉，不进入这一次切片。已经通过上一次 `last_segment` 交出去的那个字符串，这两个属性都不再写回。

Rust `DecodeStream` 的实现本次没有打开。能确定的是 Python 包装只追加 `step` 的返回值，再按 offset 切片。

## 3. 事实：Voxtral 的 `step` 返回 `list[str]`，注释里的翻转没有字段

打开的全文是 `https://raw.githubusercontent.com/Blaizzy/mlx-audio/main/mlx_audio/stt/models/voxtral_realtime/streaming.py`。`VoxtralStreamingSession.step(self, *, max_decode_tokens: int = 4) -> list[str]`。文档字符串写：排空待处理音频，必要时 prefill，再最多解 `max_decode_tokens` 个 token，返回这一次发出的 text deltas。已完成时返回空列表。

类注释写较小的 `transcription_delay_ms`（举例 160、320）会让增量更早出现，WER 上升，并且 partial hypotheses flip more often；较大（举例 640、960）更稳。默认取 `config.transcription_delay_ms`。本机 `/tmp/voxtral_config.py` 里该字段默认 480，注释写 480 ms。同一配置还有 `n_left_pad_tokens = 32`、`sampling_rate` 默认 16000。`create_streaming_session` 不在这个 streaming 文件里；本文件 `VoxtralStreamingSession.__init__` 的参数是 `max_tokens`（默认 4096）、`temperature`（默认 0.0）、`transcription_delay_ms`。没有 `context`、`system_prompt`、`hotwords`，也没有假设标识。

`_decode_some` 建一个局部 `deltas: list[str]`。非 EOS 时 `add_token`，若 `last_segment` 非空就 `append`。EOS、超过 `max_tokens`，或关闭后音频用尽时，`finalize` 再追加一次切片，然后 `_done = True`。已读循环只往 `self.generated` 追加 token，不删除已经追加的 id，也不接收上一次返回的那个列表。翻转若发生在声学条件里，这个返回值没有一项用来指出「哪一条先前字符串被改了」。

同一仓库 `docs/guides/streaming-stt.md` 写协议：`step` 返回 append-only text deltas，可以是空列表；点名实现该协议的是 Nemotron 与 Voxtral Realtime。指南没有修订指针。Nemotron 的 `streaming.py` 本次没有打开，不把它的返回字段写进来。

Swift 侧的 `Delta` 不在本篇点名的三份 Python 入口里。`019` 已记过那份 Swift 文件。本篇没有重开，不用它回答上面的返回值。

## 4. 事实：豆包的可见编辑在识别结果外面做公共前缀

`002` §4.1 记录 `ASRTypeWriter.updateResult(result:block:)`（`0x100bb1a04`，区间 `0x100bb1500..0x100bb3500`）。已展示文本来自 `targetText[..<currentIndex]`。随后调用 `StringProtocol.commonPrefix(with:options:)`（`0x100bb1b98`），用 `String.count`（`0x100bb1bb8`）得到公共前缀长度，再用 `Substring.index(_:offsetBy:limitedBy:)`（`0x100bb1bdc`）把 `currentIndex` 放到公共前缀末尾。`002` 写：已稳定前缀保持不动，只对新增或改写的后缀按固定时间步长逐字回调。该节把这三步标成候选（待用户确认）。本文沿用该节的地址，没有重新反汇编。

`004` §2.2 写增量部分结果携带当前未定稿分句文本和 `prefetch` 标志，再交给 `ASRTypeWriter` 计算最长公共前缀。该节没有写部分结果里有「修订的是哪一条假设」。`002` 里的 `transactionID` 用来丢掉过期打字机回调（日志原因 `staleTypewriterCallback`），比的是回调是否还属于这一次提交事务，不是文本里哪一个区间被改写。

`003` 里名为 revision 的条目是 `ContextAuxModel/current_revision` 指向目录 `revisions/10002`，不是识别假设的标识。同篇图里有 Stream Partial 与 Non-Stream Final 两条字样，没有返回字段表。

公共前缀比较的两边，都是调用方已经拿到的字符串：新来的 `result`，以及屏幕上已经推进到的前缀。识别器交来新的整段未定稿文本；哪几个已经显示的字符还留着，由 `ASRTypeWriter` 在模型外面算。

## 5. 机制

三份 MLX 入口分成两种交法。

第一种是 moona3k。调用方每次拿到的都是会话当前的整段 `state.text`，外加一条被声明为稳定、但实现只按长度决定是否退回旧串的 `stable_text`。下一次整段可以不等于「上一次整段再接几个字」：窗口早期没有文本前缀，其后每次丢掉末尾若干 token 再解码，窗口溢出前还可能为静音切点多退一截。返回值用 `_rewrite_events` 累计「新整段不以旧整段为前缀」的次数。次数回答不了「改的是哪一条假设」，也回答不了「从第几个字符开始不一样」。要知道已经交出的字哪些还留着，调用方得自己拿新旧两段字符串做比较。这和 `ASRTypeWriter` 的位置相同：比较发生在拿到文本之后，不在模型返回的字段里。不同之处是豆包笔记里的比较函数是 `commonPrefix`；moona3k 的 `feed_audio` 只做 `startswith` 计数，`_split_stable_unstable` 只按末尾单位数和长度取舍，两者都不是公共前缀编辑。

第二种是 mlx-audio 的 Qwen3-ASR `stream_transcribe` 与 Voxtral `step`。它们交给调用方的是 detokenizer 自上次 offset 以来的新切片，类型分别是 `StreamingResult` 和 `list[str]`。上一次已经返回的对象不在下一次的参数里，函数也没有把它改掉。Qwen3-ASR 的 `is_final` 标记的是音频块是否最后一块，空 `text` 配上时间戳是块边界，不是「请替换此前某条」。Voxtral 类注释写短延迟时部分假设更常翻转；`step` 的列表里没有与这句话对应的一项。注释不能当成返回字段。把追加切片包装成「新假设修订上一条」，是调用方另外编号；编号不在 `list[str]` 里，也不能从「协议写了 append-only」推出模型内部没有翻转。能从源码说出的只有：翻转没有被编码成「改的是哪一条」。

`019` 把这三份入口记成「仍然没有修订了哪一条」。本次对得上。补上的区分是：moona3k 的下一次整段文本，按回退实现，可以改掉上一次整段里已经包含的尾部字符；另外两份的下一次返回值不改写上一次已经交出的那个对象。三份都仍然没有「改的是哪一条」。

## 6. 边界

- 没有在本机喂入 PCM。尾部五个 token 解码成哪些字符、`stable_text` 在长度不缩短时是否换掉前缀，都没有一条对照字符串。
- `_append_chunk_text` 的替换逻辑在已读调用链上没有执行。检索该符号无结果，不排除仓库其他分支。
- Qwen3-ASR 与 Voxtral 使用的 Rust `DecodeStream` 未打开。Python 侧只追加并切片。
- Nemotron 的 `step` 未打开。Swift `Delta` 未重开。
- `TranscriptionResult` 的字段表未打开。
- `002` 的公共前缀三步在原文中标为候选（待用户确认）。本文没有新的反汇编去改这个标记。

## 7. 仍未知

若要说某一份返回值自己满足「标明修订的是哪一条先前假设」，缺的是字段或等价对照，不是再把适配器记住的上一条标识叫成模型字段。moona3k 在真实音频上，强制前缀所覆盖的字符会不会变，本次没有测量。Voxtral 注释中的 hypothesis flip 会不会让已经打印的追加切片在语义上作废，返回值不记载；要看它是否发生，得另有观测，不能从 `list[str]` 读出来。

## 8. 候选读法

候选：已经显示出去的字要不要改、改的是屏幕上的哪一段，放在拿到文本之后做。moona3k 给出的是新的整段，比较的材料和豆包交给 `ASRTypeWriter` 的部分结果同一类，都是整段字符串，不是修订指针。mlx-audio 这两份给出的是新切片；切片本身没有指向此前任何一条。调用方若另存一条标识，那个标识仍然只存在于调用方。

## 检索日志

| # | 动作 | 地址或查询 | 结果 |
| --- | --- | --- | --- |
| 1 | 本地阅读 | `020-research-gap-fill-index.md` | 021 只回答下一次输出会不会改已交出的字，以及豆包打字机如何对照 |
| 2 | 本地阅读 | `019` 第 2 条 | 三份入口当时被记成没有修订指针；`feed_audio` 返回 `StreamingState` |
| 3 | 本地检索 | `002`、`003`、`004`、`019` 中的 `ASRTypeWriter`、公共前缀、`revision` | `002` §4.1 与 `004` 部分结果命中；`003` 的 revision 是模型目录号 |
| 4 | 本地阅读 | `002` §4.1 至 §4.3 | `commonPrefix` 地址 `0x100bb1b98`；`transactionID` 过滤过期回调 |
| 5 | 本地阅读 | `004` §2.2 | 部分结果是未定稿分句文本加 `prefetch`，再交给打字机 |
| 6 | 本地阅读 | `018` 第 2.2 至 2.4 节 | 修订目标被写成适配器外缘的候选包法 |
| 7 | 本地阅读 | `007` 原语 4 | 假设应标明修订哪一条；本文只核对照，不改原语 |
| 8 | 页面读取 | moona3k `streaming.py` raw 全文 | `feed_audio` 返回状态；`_rewrite_events` 为次数；无假设标识 |
| 9 | 页面读取 | 同仓库 `__init__.py` | `__all__` 无 `_append_chunk_text`，无流式修订类型 |
| 10 | 页面读取 | 同仓库 `session.py` | `Session.feed_audio` 仍返回 `StreamingState` |
| 11 | 网页检索 | `site:github.com/moona3k/mlx-qwen3-asr _append_chunk_text` | Tavily 无结果。空结果 |
| 12 | 本地阅读 | `/tmp/qwen3_asr_mlx.py` 的 `StreamingResult`、`generate`、`stream_transcribe` | yield `last_segment`；无修订字段 |
| 13 | 本地检索 | 该文件 `create_streaming_session` | 无匹配。空结果 |
| 14 | 页面读取 | raw `qwen3_asr.py` 开头约 4500 字 | 与本机文件开头一致；`StreamingResult` 字段同上 |
| 15 | 本地阅读 | `/tmp/mlx_stt_base.py`、`/tmp/mlx_stt_streaming.py` | `STTOutput` 无修订字段；协议写 append-only |
| 16 | 页面读取 | mlx-audio `lm/generate.py` 的 `StreamingDetokenizer` | `last_segment` 是 offset 之后的新切片，然后推进 offset |
| 17 | 页面读取 | `voxtral_realtime/streaming.py` 全文 | `step` 返回 `list[str]`；注释写 hypotheses flip；循环只追加 |
| 18 | 本地阅读 | `/tmp/voxtral_config.py` | `transcription_delay_ms` 默认 480；无假设标识字段 |
| 19 | 网页检索 | mlx-audio `voxtral_realtime.py` `def step` | 命中模型卡与 TTS 文档，不是这份 `streaming.py`；不采用其中的用法数字 |
| 20 | 页面读取 | `docs/guides/streaming-stt.md` | `step` 为 append-only `list[str]`；点名 Nemotron 与 Voxtral |
| 21 | 本地阅读 | `003` 约第 370 行附近的双路字样 | 只有 Stream Partial 与 Non-Stream Final 的图注，无返回字段 |

第 11 条没有给出 `_append_chunk_text` 的其他调用点。第 13 条没有给出 Qwen3-ASR 的 `create_streaming_session`。第 19 条没有打开到 `step` 的源码，源码以第 17 条为准。

<!-- research-complete -->
