# 035-code-analysis-feed-audio-final-pass：松开后的定稿是 finish_streaming

本文只回答松开后「再定稿一次」对应哪一个函数、它吃剩余缓冲还是空缓冲、返回的文本是整段还是增量、与说话中的 `feed_audio` 返回值是不是同一种整段文本。模型档沿用已拍板的 Qwen3-ASR 0.6B 4-bit，本文不改档。`031` 仍是未确认蓝图，本文不改它。A、D、G、J、L 五项不重新打开。

本次没有喂入 PCM，没有新的转写字符串。下面的数据流来自函数体。

## 1. 对应的函数

松开后的那一次是 `mlx_qwen3_asr.streaming.finish_streaming`。已安装包 `mlx-qwen3-asr==0.4.4` 里，定义在 `streaming.py` 第 339–360 行。本次打开的 GitHub `main` raw（`https://raw.githubusercontent.com/moona3k/mlx-qwen3-asr/main/mlx_qwen3_asr/streaming.py`）中，同名函数体与这 22 行一致。整文件校验和没有做。

`Session.finish_streaming` 在同包 `session.py` 第 205–210 行。它只把本会话已加载的模型传进上面的函数，返回值仍是 `StreamingState`。包入口 `__init__.py` 的 `__all__` 没有 `finish_streaming` 这个名字。CLI 从 `streaming` 模块直接导入。

函数文档写：用任何待处理的尾部音频，把窗口解码完，从而结束会话。`StreamingState.finalization_mode` 的注释写 `accuracy` 与 `latency` 两种都在 finish 时解码待处理尾巴。`enable_tail_refine` 的注释写 finish 时的窗口重解码已经盖过从前单独的 tail refine。这两个参数不另外分出一条收尾函数。

`feed_audio` 不是这一次。它在第 294–336 行。空 PCM（清洗后 `x.size == 0`）直接把原 `state` 返回。缓冲未满 `chunk_size_samples` 时只把采样留在 `state.buffer`，不调用 `_decode_and_update`。

## 2. 解码吃剩余缓冲；空缓冲不解码

```python
# 来源: https://raw.githubusercontent.com/moona3k/mlx-qwen3-asr/main/mlx_qwen3_asr/streaming.py 的 finish_streaming；已安装 0.4.4 同文件第 344–354 行与此一致
    state._pre_finalize_text = state.text
    if len(state.buffer) == 0:
        state._finalization_delta_chars = 0
        if len(state.audio_accum) > 0:
            state.stable_text = state.text
        return state
    tail = state.buffer
    state.buffer = np.array([], dtype=np.float32)
    _extend_window(state, tail, model)
    _decode_and_update(state, model)
```

这个函数没有 PCM 参数。它读的是 `state.buffer`。该字段的数据类注释是尚未处理的待处理采样。

说话中的 `feed_audio` 把新采样拼进 `buffer`，只有长度达到一块才切出去解码，切剩的留在 `buffer`。默认一块是 `chunk_size_sec=2.0` 乘 `sample_rate`，在 16000 Hz 下为 32000 个样本。未满这一块的采样就停在 `state.buffer`。

缓冲长度大于 0 时，`finish_streaming` 把这块剩余缓冲整段取走，随即把 `buffer` 换成空数组，再 `_extend_window` 与 `_decode_and_update`。送进解码的是剩余采样，不是空数组。长度是否满一块，这个分支不检查。

缓冲长度等于 0 时，不调用 `_decode_and_update`。`_finalization_delta_chars` 写成 0。仅当 `audio_accum` 非空时，把 `stable_text` 设成当时的 `state.text`，然后返回。这是空缓冲返回，不是再解码一次。`021` 把 `finish_streaming` 写成一律把 `stable_text` 设成当时的 `text`。函数体比那句窄：空缓冲且 `audio_accum` 为空时，不改 `stable_text`。

`024` 把尾部 23983 个样本留在文件里，没有送入 `state.buffer`。随后的 `finish_streaming` 走的就是空缓冲返回。那次墙钟不是尾块解码。精确微秒本文不新测；`024` 的措辞是大约几十微秒，这里不另写一个整数。

解码分支在上面摘录之后还有三步，然后返回 `state`：`chunk_id` 加 1，`stable_text` 设成当时的 `state.text`，`_finalization_delta_chars` 设成结束文本长度减去进入函数时的长度。

## 3. 返回的是整段，不是增量

返回类型是 `StreamingState`，不是一段新切片，也不是那个长度差。

解码分支调用的 `_decode_and_update` 在第 495–502 行。它重解的是整个 `audio_accum`，写出 `window_text`，再执行 `state.text = _join_texts(state.committed_text, window_text, state.language)`。`_join_texts` 在第 504–512 行：没有已提交文本就返回窗口文本；两边都有时，`is_no_space_language` 为真则直接拼接，否则中间加一个空格。`text` 的数据类注释是当前最好的转写，即已提交窗口加上活动窗口。这是会话当前的整段，不是相对上一次 `text` 的增量。

`_finalization_delta_chars` 是两个长度的差，类型是整数。公式允许负数。剩余缓冲非空时这个差是多少，本次没有样本，记为未知。

`_extend_window` 从第 381 行起。只有 `audio_accum` 加上这次尾巴会超过 `max_context_samples` 时，才先提交旧窗口。`init_streaming` 的默认 `max_context_sec` 是 30.0。不超过这个上限时，尾巴只拼进当前窗口，然后上面那一次 `_decode_and_update` 重解整窗。超过时，同一次 `finish_streaming` 里会先按静音切点多解码一次再提交，然后解新窗口。返回给调用方的仍然是拼好的整段 `state.text`。超过 30 秒的样本本次没有。

解码分支不写 `_text_updates`，也不写 `_rewrite_events`。这两个计数只在 `feed_audio` 凑满一块并解码之后增加。

## 4. 与说话中的 feed_audio 是同一种整段

`feed_audio` 的返回类型同样是 `StreamingState`。凑满一块之后，它调用的也是 `_decode_and_update`，`state.text` 同样是已提交文本拼上活动窗口文本。说话中当作部分文本的那一份，和松开后这一次当作定稿的那一份，都是这个字段上的整段字符串。不是增量切片。

调度不同，文本种类相同：

- `feed_audio` 不满一块不解码；空 PCM 原样返回。
- `finish_streaming` 在 `buffer` 非空时解码，不论是否满一块；`buffer` 为空则不解码。
- 说话中，`chunk_id < unfixed_chunk_num`（默认 2）时 `feed_audio` 不更新 `stable_text`；其后用 `_split_stable_unstable` 按末尾 `unfixed_token_num`（默认 5）个单位留下不稳定尾。`finish_streaming` 在解码之后把 `stable_text` 设成整段 `text`。空缓冲且 `audio_accum` 非空时，也把 `stable_text` 设成当时的 `text`，但不重解。
- `feed_audio` 在整段变化时累计 `_text_updates`，并在新整段不以旧整段为前缀时累计 `_rewrite_events`。`finish_streaming` 不写这两个计数。`_rewrite_events` 不是拍板 D 里的改写记录。

CLI 的文件流在 `cli.py` 第 802 行、麦克风在第 710 行：循环 `feed_audio` 之后调用 `finish_streaming`，再把 `TranscriptionResult.text` 设成 `state.text`。麦克风打印时，若这份整段以上一次已经打印的 `stable_text` 为前缀，终端只补打后缀；前缀对不上就整段重打。那是打印，不是函数的返回值。返回值仍是整段 `state.text`。

## 5. 当前方案

松开时调用 `finish_streaming`，或调用 `Session.finish_streaming`。定稿原文取返回状态的 `state.text`。说话中每次 `feed_audio` 返回的 `state.text` 仍是部分文本。

未满一块的采样留在 `state.buffer`。这次 `finish_streaming` 吃掉这块剩余缓冲，再解码一次，得到新的整段 `state.text`。不把空 PCM 送进 `feed_audio` 当作收尾。`buffer` 已经为空时，这次调用不再解码，定稿原文就是进入函数时的那份整段 `state.text`。

识别仍只用这一条 `feed_audio` 加这一次 `finish_streaming`，权重仍是 Qwen3-ASR 0.6B 4-bit。不另装 SenseVoice，不把两个识别器串成两级。预编辑、改写不上屏、热词字符串、按住说话，维持 `033` 已记下的拍板。交付不调用 Type4Me 的 `inject`，不改 librime。

## 6. 边界

- 本次没有喂入 PCM。剩余缓冲非空时，定稿整段与上一份部分文本差几个字符：未知。
- GitHub `main` 与已安装 0.4.4 只核对了 `finish_streaming` 函数体一致，没有整文件校验和。
- 按住超过默认 30 秒时，同一次 `finish_streaming` 内部的提交解码没有样本。
- `024` 的到达块中位不属于这次收尾解码。收尾解码的时延仍是未知。

## 检索日志

| # | 动作 | 地址或查询 | 结果 |
| --- | --- | --- | --- |
| 1 | 本地阅读 | `021-research-hypothesis-revision-pointer.md` | `feed_audio` 返回整段 `StreamingState`；该篇把 `finish_streaming` 写成一律改 `stable_text` |
| 2 | 本地阅读 | `024-research-streaming-frame-latency.md` | 尾部未送入缓冲时，`finish_streaming` 见空缓冲即返回，不是尾块解码 |
| 3 | 本地阅读 | `033-crystallization-voice-binding-adgjl.md` | A 已写松开后再定稿一次；模型档是 Qwen3-ASR 0.6B 4-bit |
| 4 | 本地检索 | `dev-docs` 中的 `feed_audio`、`finish_streaming` | 命中 021、024、025、026、018；018 的收尾函数是 vLLM 的 `finish_streaming_transcribe`，不是本篇这条 |
| 5 | 打开文件 | 已安装 `mlx-qwen3-asr` 0.4.4 的 `streaming.py` 第 294–360、381、495–512 行 | `finish_streaming` 非空则吃 `state.buffer` 再走 `_decode_and_update`；空则返回 |
| 6 | 打开文件 | 同包 `session.py` 第 197–210 行 | `Session.finish_streaming` 只转发到模块函数 |
| 7 | 打开文件 | 同包 `__init__.py` 的 `__all__` | 无 `finish_streaming`，无 `feed_audio` |
| 8 | 打开文件 | 同包 `cli.py` 第 710 行与第 802 行 | 循环 `feed_audio` 之后调用 `finish_streaming`，结果文本取 `state.text` |
| 9 | 页面读取 | GitHub `main` 的 `streaming.py` raw | `finish_streaming` 函数体与 0.4.4 第 339–360 行一致 |
| 10 | 网页检索 | moona3k `finish_streaming` README | 示例是 `feed_audio` 循环后 `finish_streaming`，再读 `state.text`；未另发现收尾函数 |

第 4 条里的 vLLM 函数不进入本篇的定稿调用。第 10 条没有给出与第 9 条冲突的函数体。本次没有调用 REA。

<!-- research-complete -->
