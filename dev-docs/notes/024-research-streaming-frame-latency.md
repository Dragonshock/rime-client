# 024-research-streaming-frame-latency：本机到达帧到可修订假设的时延

本文只回答 `019` 第 1 条：在 MLX 上，一块仍在到达的采样，要多久才返回一份可以改掉已经交出的假设文本，以及同一次测量的内存。豆包输入法只作机制对照。数字只来自这次进程或这次打开的文件。整段进、整段出的时延这次没有测，不写进下面的到达块表。

事实与推论分开。推论放在文末，标成候选。

## 1. 这次进程的条件

测量开始于 2026-11-12 03:52:04 HKT，进程结束于 03:52:43 HKT。

机器是这次 `system_profiler SPHardwareDataType` 与 `sw_vers` 读到的本机，不是仓库 README 里的 M4 Pro：

| 项 | 读数 |
| --- | --- |
| 型号 | MacBook Pro，Model Identifier `Mac14,10` |
| 芯片 | Apple M2 Pro |
| CPU | 12 核，其中 8 个性能核、4 个能效核 |
| 内存 | 16 GB |
| 系统 | macOS 27.2.1，Build `26B131`；内核 Darwin 27.2.0 |

测量前，系统自带的 `python3` 是 3.13.16，`import mlx` 为 `ModuleNotFoundError`，`~/.cache/huggingface` 不存在。安装没有写入用户主目录。虚拟环境在 `/tmp/gap024/venv`，解释器是 CPython 3.12.14。`uv pip` 装上的包是 `mlx-qwen3-asr==0.4.4` 与 `mlx==0.32.4`。权重缓存是 `HF_HOME=/tmp/gap024/hf`。进程里只加载了下面这一份 0.6B 权重，没有加载 1.7B，也没有加载文本模型。

权重仓库是 `moona3k/mlx-qwen3-asr-0.6b-4bit`，解析到快照 `4c59c533f95c84afb796655e814709034f826f04`。同目录 `quantization_config.json` 全文是 `bits` 4、`group_size` 64、`audio_tower_bits` 8。`config.json` 的 `model_type` 是 `qwen3_asr`，没有 `quantization_config` 键。加载后再数带 `bits` 属性的模块：前缀 `lm_head` 有 1 个 4-bit、组大小 64；前缀 `model` 有 197 个 4-bit、组大小 64；前缀 `audio_tower` 有 111 个 8-bit、组大小 64。没有 `bits` 属性的模块没有计入。`weights.safetensors` 这次 `stat` 为 537626967 字节。README 自述的下载体积是 517 MB，那是另一句，不与这个字节数并成一格。

加载调用是 `_load_model_with_resolved_path(..., dtype=mx.float16)`。这一步的墙钟是 25.463470 秒，里面包含控制台显示的 6 个文件下载。缓存命中后的纯加载秒数这次没有分开记，记为未知。

音频由 `say -v Tingting` 读出下面这句，再经 `afconvert` 变成 16 kHz、单声道、16-bit WAV：

> 下面是一段用来测量到达帧的普通话。今天上午我们在公园见面，先讨论预算，再确认下周的行程。如果下雨就改到室内会议室。下午把记录发给同事，并约好明天继续。

`afinfo` 与 `wave` 一致：279983 帧，时长 279983 / 16000 = 17.4989375 秒。

协议用已安装包里的 `init_streaming` / `feed_audio`，不是 `transcribe()`。显式传入的参数是模型路径、`chunk_size_sec=2.0`、`max_context_sec=30.0`、`sample_rate=16000`、`language="Chinese"`。其余沿用函数默认：`unfixed_chunk_num=2`、`unfixed_token_num=5`、`reuse_window_prefix=True`、`endpointing_mode="fixed"`、`commit_at_silence=True`。已安装 `tokenizer.py` 把 `chinese` 映射为 `Chinese`。块长按该函数是 `int(2.0 * 16000) = 32000` 个样本。循环在缓冲凑满一块时调用一次 `feed_audio`。8 次之后剩余 23983 个样本（1.4989375 秒）留在文件里，没有送入 `state.buffer`。`max_context` 是 30 秒，已读的 `_extend_window` 只在窗口将超过 `max_context_samples` 时才提交窗口；这次累积最多 16 秒，走不到那条分支。

计时包住 `feed_audio` 返回之后的 `mx.synchronize()`。先跑 1 次不计入中位的预热会话，再 `mx.metal.reset_peak_memory()`，然后 10 次独立会话。中位用 Python `statistics.median`：10 个数取第 5、第 6 个的平均。

```python
# 来源: 已安装 mlx-qwen3-asr 0.4.4 的 mlx_qwen3_asr/streaming.py，feed_audio
        prev_text = state.text
        _decode_and_update(state, model)
        if state.text != prev_text:
            state._text_updates += 1
        if prev_text and state.text and not state.text.startswith(prev_text):
            state._rewrite_events += 1
```

同文件默认 `reuse_window_prefix=True`。README 的 Streaming 节写明：完整注意力窗上的编码器输出和解码器 KV 会跨块复用。因此下表是这条默认路径的墙钟，不是「每块都把整段窗口从零重算」的墙钟。

## 2. 到达中的块：已有上一份文本之后

下表每一行都是同一次条件：M2 Pro、16 GB、上述 4-bit/8-bit 权重、2.0 秒一块、language 传入 Chinese、预热之后的 10 次。块序从 0 起。块序 k 的 `feed_audio` 吃到的是第 k 块刚到达的 2.0 秒，返回时文件里还有未送入的采样。10 次的 `streaming_metrics` 都是 `text_updates=8`、`rewrite_events=7`。块序 0 没有上一份文本，7 次改写落在块序 1 至 7。窗长随块序增加，所以七个中位分列，不收成一个数。

| 块序 | 该块结束时已送入 | 次数 | 中位（秒） | 最小（秒） | 最大（秒） |
| --- | --- | --- | --- | --- | --- |
| 1 | 4.0 秒 | 10 | 0.100019 | 0.098852 | 0.103165 |
| 2 | 6.0 秒 | 10 | 0.113075 | 0.112377 | 0.114901 |
| 3 | 8.0 秒 | 10 | 0.115692 | 0.115111 | 0.117534 |
| 4 | 10.0 秒 | 10 | 0.140109 | 0.138917 | 0.142307 |
| 5 | 12.0 秒 | 10 | 0.110996 | 0.109887 | 0.112240 |
| 6 | 14.0 秒 | 10 | 0.119115 | 0.117680 | 0.121229 |
| 7 | 16.0 秒 | 10 | 0.127331 | 0.125818 | 0.127921 |

第 0 次计时会话里，块序 1 返回的文本不再以上一块全文为前缀。上一块是「下面是一段用来测量。」，这一块是「下面是一段用来测量到达正的普通话。经」。句号被拿掉，并补上了后文。块序 4 改到了更早的标点：上一份作「见面，先讨论预算。」，这一份作「见面。先讨论预算，再确认下周的行程。」。逗号换成了句号。识别把口播里的「帧」写成了「正」；这里不计算错误率。

`stable_text` 不是同一列。源码在 `chunk_id < unfixed_chunk_num`（默认 2）时不更新它。第 0 次计时会话里，块序 0 和 1 的 `stable_text` 仍是空字符串；从块序 2 起它是 `text` 的一个更短前缀，并且一次比一次长。这次保存下来的只有第 0 次计时会话的八份字符串，不能把「只变长」写成其余 9 次的观察。

返回值仍是整份 `StreamingState`。`rewrite_events` 是计数。没有「改的是哪一条假设」这种标识。`019` 第 2 条要的那个字段，这次调用没有出现。

## 3. 第一份假设与预热，分开记

块序 0 返回第一份 `text`，当时没有可改的上一份。10 次中位 0.064944 秒，最小 0.063924 秒，最大 0.066072 秒。它仍是 2.0 秒到达块，但不是上一节的改写行。

预热会话不进上两节。它的八块墙钟依次是 1.858804、0.116143、0.155544、0.115539、0.227112、0.111271、0.118015、0.126137 秒。第一块比后来的块序 0 中位大约长 1.8 秒。这次没有把多出来的时间拆成着色器编译或其他名目。

## 4. 内存

两套读数来自不同接口，不相加。

Metal 峰值在预热之后清零，10 次计时跑完再读。调用的是 `mx.metal.get_peak_memory`，运行时提示它是 `mx.get_peak_memory` 的旧名。

| 接口 | 字节 |
| --- | --- |
| `mx.metal.get_peak_memory` | 825819094 |
| `mx.metal.get_active_memory`（进程末） | 545453458 |
| `mx.metal.get_cache_memory`（进程末） | 20990536 |

进程峰值来自 `resource.getrusage(RUSAGE_SELF).ru_maxrss`。这次打开的 `man 2 getrusage` 写明该字段单位是字节。它覆盖整个进程，含下载、加载和预热，不是上表那个清零之后的 Metal 峰值。

| 接口 | 字节 |
| --- | --- |
| `ru_maxrss` | 1007730688 |

同一条测量命令前后，`sysctl -n vm.swapusage` 都是 `total = 3072.00M  used = 1831.69M  free = 1240.31M`。交换区占用没有在这条命令的前后读数之间上升。开始之前交换区已经用了 1831.69M，所以这不是一张空闲 16 GB 上的读数。

## 5. 不放进第 1 条的数字

尾部 23983 个样本没有送入缓冲。随后的 `finish_streaming` 看到空缓冲就返回，10 次墙钟大约是几十微秒。那不是尾块解码。收尾解码的时延记为未知。

`transcribe()` 整段进、整段出这次没有跑。本机上的整段时延记为未知。

`mlx-qwen3-asr` README 的 “Performance on Apple Silicon” 仍是另一张表：Apple M4 Pro（48 GB）、macOS 26、v0.4.0、10 次中位、整段片段。其中 0.6B 4-bit 对大约 2.5 秒片段写 0.09 秒，对 10 秒片段写 0.17 秒。那不是 M2 Pro，也不是 `feed_audio`。不与第 2 节并表。同页流式段落的 primary error 仍没有毫秒。

`015` 记下的论文单路 TTFT（约 2 分钟请求上的 92 ms 与 102 ms）硬件未点名。不与第 2 节并表。

## 6. 豆包只作机制对照

`002` 第 4.1 节：部分结果会改局部字词；`ASRTypeWriter` 用公共前缀对齐已经展示的字，再挪动后缀。`004`：云端增量结果带着未定稿分句和 `prefetch`，再交给这个打字机。`003`：说话过程中流式引擎给出部分结果；VAD 尾点之后，非流式引擎重解码，并按时间戳盖住被覆盖的流式输出。

这次 MLX 读数与那三条机制同向的只有一件事：音频还没送完，新的一份 `text` 已经改了上一份里的字，而不必等到文件结束。对不上的也有一件事：豆包笔记里的定稿重解码绑在尾点；这次改写发生在默认 2.0 秒的 `feed_audio` 返回时。豆包那几篇没有给出可与第 2 节相减的本机毫秒，这里不补。

## 7. 候选

候选：第 2 节就是 `019` 第 1 条要的那一类测量。芯片、量化、块长、次数和 `feed_audio` 写在同一次里；返回的 `text` 在下一次到达块里不再以上一份全文为前缀；测量时文件尾部仍未送入。

候选：能被这次读数证明会变的，是 `text` 相对上一份不再构成前缀的那一段。`stable_text` 在第 0 次计时会话里只变长。README 写稳定前缀按设计不缩短。这次没有证明已经放进 `stable_text` 的字会被改掉。

候选：`rewrite_events` 不能充当 `019` 第 2 条的修订标识。它不指出改的是哪一条。

候选：这张表不覆盖 0.6B 的 8-bit，也不覆盖 1.7B。那些权重这次没有加载。不用第 2 节的中位去比较它们。

## 检索日志

| # | 动作 | 地址或查询 | 结果 |
| --- | --- | --- | --- |
| 1 | 本地阅读 | `020`、`019` 全文 | 第 1 条要的是到达帧、芯片、量化、块长；M4 Pro 整段表不能填这一格 |
| 2 | 本地检索 | `015`、`014`、`017` 中的时延与 MLX | 论文 TTFT 与 2 秒块 WER 不是 Apple Silicon；第三方 MLX 整段表当时未当作本机测量 |
| 3 | 本地阅读 | `002` §4.1、`003` 流式/非流式双路、`004` 增量部分结果 | 豆包是部分结果加尾点重解码；笔记里没有可与本次相减的毫秒 |
| 4 | 本机命令 | `system_profiler`、`sw_vers`、`python3 -c import mlx`、`~/.cache/huggingface` | M2 Pro、16 GB；mlx 未安装；Hugging Face 缓存目录不存在 |
| 5 | 网页检索 | moona3k mlx-qwen3-asr 0.6B 4bit feed_audio | 命中 `mlx-qwen3-asr` 与 `moona3k/mlx-qwen3-asr-0.6b-4bit`；未采用未打开的 DeepWiki 数字 |
| 6 | 页面读取 | `https://raw.githubusercontent.com/moona3k/mlx-qwen3-asr/main/mlx_qwen3_asr/streaming.py` | `feed_audio` 按块重解窗口；`UNFIXED_CHUNK_NUM=2`、`UNFIXED_TOKEN_NUM=5` |
| 7 | 页面读取 | 同仓库 `pyproject.toml`、`load_models.py`、`__init__.py` | 依赖 `mlx>=0.18`；`load_model` 走 `snapshot_download`；包入口不导出 `feed_audio` |
| 8 | 页面读取 | 第一次拉取 README | 管道写失败，得到 0 字节。空结果 |
| 9 | 页面读取 | 同仓库 README，Streaming 与 Performance 两节 | 默认 `feed_audio` 示例；M4 Pro 整段表；`stable_text` 按设计不缩短 |
| 10 | 本机安装 | `/tmp/gap024/venv` 安装 `mlx-qwen3-asr==0.4.4` | 成功。实际装上 `mlx==0.32.4`。未装入用户环境 |
| 11 | 本机测量 | `/tmp/gap024/bench.py`，模型 `moona3k/mlx-qwen3-asr-0.6b-4bit` | 10 次到达块中位见第 2 节；`rewrite_events` 每次为 7；尾块解码未发生 |
| 12 | 打开文件 | 快照内 `quantization_config.json`、`config.json`，以及 `weights.safetensors` 的 `stat` | 4-bit / 组 64 / 音频塔 8-bit；权重 537626967 字节 |
| 13 | 打开手册 | `man 2 getrusage` 的 `ru_maxrss` | 单位是字节 |
| 14 | 对照 | 已安装包的 `streaming.py` 与 GitHub 上读到的 `feed_audio` | 改写计数与块循环一致；计时用的是已安装的 0.4.4 |

第 8 条没有新数字。本次没有调用 REA。

<!-- research-complete -->
