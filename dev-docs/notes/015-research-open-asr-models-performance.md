# 015-research-open-asr-models-performance：开源语音识别模型与已发表性能

本篇只回答一件事：哪些已发表、可核对的开源模型能把采样帧变成文本，以及它们公开的体积、流式或整句、时延和中文效果。识别流的输入输出形状以 `007` 原语 4 为对照：输入是有序采样帧（可带上下文快照），输出是同一会话上的文本假设，更晚的结果可以改更早的文本。本篇不写文本改写模型，不写 MLX 的加载与调用序列。那两件分别是 `016` 与 `017`。

豆包输入法不在本篇拆解。下面表格里的 Doubao-ASR 只是他人论文中的闭源对照列。公开模型卡和论文足够核对权重与数字，本篇没有调用 REA。

数字只写打开过的页面。没有写在来源里的硬件、量化或时延记为未知。不同论文的错误率分表，不排进同一张对照表。中文列有的来源标 WER，有的标 CER；表头用来源原文，不把 WER 改写成 CER。

## 1. 怎样区分整句转写和可修订假设

来源里的「streaming」不是同一种输出。

| 来源里的行为 | 和原语 4 的关系 | 本篇见到的例子 |
|---|---|---|
| 整段或整句音频进模型，返回一次文本 | 能产生文本假设；同一次调用内部没有「更晚的假设改前文」 | Whisper 官方命令行；SenseVoice-Small 的非自回归一次前向；Qwen2-Audio 对一条音频生成回复 |
| 音频按块进入，未结束时就有部分文本，且来源写明近期文本尚未固定 | 部分文本可以被后来的块改写。这与「可修订」同向 | Qwen3-ASR 论文：2 秒一块、5 token 回退、最后四块不固定 |
| 按块解码并带 cache，示例只展示每块文本或 `is_final` | 存在部分文本。已输出的字会不会被改，来源没写 | Paraformer-zh-streaming；sherpa-onnx 的 Zipformer 示例 JSON |
| 自回归地把回复一个 token 一个 token 打出来，或编码器按块吃音频 | 输出在时间上是流式的。这不等于识别假设回改 | Qwen2.5-Omni、Qwen3-Omni 的 README 对 streaming response 的描述 |

候选，不是绑定：原语 4 要的是「标明修订了哪一条先前假设」的序列。上面第三、四行的模型能产出随时间变化的文本，但已读页面都没有给出假设标识或修订指针。缺的是接口字段，不是又一篇论文。

## 2. Qwen 系列里有没有语音识别或音频模型

有。专用识别权重、音频语言模型、全能模型是三条线，不能互相代替参数量。

| 线 | 公开权重名 | 角色 | 许可证字段 |
|---|---|---|---|
| 专用 ASR | `Qwen/Qwen3-ASR-1.7B`，`Qwen/Qwen3-ASR-0.6B` | 语言识别加转写。论文写训练成只做 ASR、不跟随自然语言指令 | Hugging Face `cardData.license` = `apache-2.0` |
| 同时发布、但不是识别 | `Qwen/Qwen3-ForcedAligner-0.6B` | 已有文本与语音对齐，预测时间戳。输入里已经有转写 | 同上，模型卡与 ASR 同一家族，Apache-2.0 |
| 音频语言模型 | `Qwen/Qwen-Audio`，`Qwen/Qwen-Audio-Chat`；`Qwen/Qwen2-Audio-7B`，`Qwen/Qwen2-Audio-7B-Instruct` | 音频进、文本出。可以转写，也可以按指令做翻译、描述、对话 | Qwen2-Audio 两条均为 `apache-2.0`。本篇打开的 `Qwen-Audio-Chat` API 没有返回 license 字段 |
| 全能模型，含 ASR 提示 | `Qwen/Qwen2.5-Omni-3B`，`Qwen/Qwen2.5-Omni-7B`；`Qwen/Qwen3-Omni-30B-A3B-Instruct` | 文本、图像、音频、视频进；文本与语音出。模型卡有单独的 ASR 提示 | 三条的 `cardData.license` 都是 `other` |
| 闭源接口名，不是开源权重 | Qwen3-ASR-Flash、Qwen3-Omni-Flash | 出现在论文或 README 的对照列 | 不适用 |

检索摘要里还有 `Qwen/Qwen3-ASR-1.7B-hf` 与 `Qwen/Qwen3-ASR-0.6B-hf`。本篇没有打开这两个仓库的文件列表，不记第二套参数量。

Qwen3-ASR 论文写：两支 ASR 从 Qwen3-Omni 后训练而来；AuT 编码器在 Omni 预训练阶段之前单独预训练。因此 Omni 权重量到的是全能模型，ASR 权重量到的是后训练后的识别模型。

### 2.1 Qwen3-ASR：权重、体积、量化、流式

论文与模型卡：

- `Qwen3-ASR-1.7B`：Qwen3-1.7B，加投影层，加 AuT 编码器。编码器 300M 参数、hidden size 1024。
- `Qwen3-ASR-0.6B`：Qwen3-0.6B，加投影层，加 AuT。编码器 180M 参数、hidden size 896。
- AuT 对 128 维 Fbank 做 8 倍下采样，得到 12.5 Hz 的音频 token。动态 flash attention 窗口从 1 秒到 8 秒，同一结构既走短块流式，也走长音频离线。
- 表 1：离线与流式都支持；单次音频最长 1200 秒；音频类型包括说话、唱腔、带伴奏的歌曲。语言为 30 种加上 22 种汉语方言。
- 模型卡：流式推理目前只在 vLLM 后端；流式不做 batch，也不返回时间戳。transformers 路径的示例是 `transcribe()` 整段转写。流式 demo 把浏览器麦克风重采样到 16,000 Hz，再连续送 PCM 块。
- `preprocessor_config.json` 使用 `WhisperFeatureExtractor`，`feature_size` 128，`hop_length` 160，`n_samples` 480000。该 JSON 没有 `sampling_rate` 键。16 kHz 的直接出处是流式 demo 的模型卡文字，不是这个 JSON。

发布名里的 0.6B / 1.7B 是语言模型骨干的称呼。Hugging Face safetensors 统计的是整包参数，二者不要相加，也不要互相替换。

| 权重 | safetensors | `usedStorage` 字节 | 官方张量类型 |
|---|---|---|---|
| `Qwen/Qwen3-ASR-1.7B` | BF16 2,349,217,408 | 4,699,896,190 | 模型卡 Tensor type：BF16 |
| `Qwen/Qwen3-ASR-0.6B` | BF16 938,008,576 | 1,876,091,704 | 同为 BF16 发布 |

官方仓库文件列表里没有 int4 / int8 权重。模型卡写推理用 `torch.bfloat16`，FlashAttention 2 只在 float16 或 bfloat16 下可用。Hugging Face 模型树显示存在社区量化；本篇没有逐个打开那些仓库，不把它们写成官方量化。

流式协议里和「可修订」直接相关的句子在论文 4.5 节：评测使用 2 秒一块、5 token 回退，并保持最后四块不固定（keeping the last four chunks unfixed）。候选：不固定的块意味着近期文本还可以变，这和原语 4 的修订同向；论文没有定义假设标识。

效率表是另一套协议，不要和 2 秒块的 WER 当成同一次实验。论文 2.4 节：vLLM v0.14.0，打开 CUDA Graph，bfloat16；离线用 vLLM offline batch，在线用 vLLM Serve 的多并发。输入大约 2 分钟。原文写 “a single typical compute resource”，没有点名 GPU。

下表只摘并发 1 和并发 128 的在线异步行。单位沿用论文：RTF 为转写时间除以音频时长，Throughput 为每秒处理的音频秒数。

| 模型 | 并发 | TTFT 平均 | TTFT p95 | 在线 RTF | 在线吞吐 |
|---|---|---|---|---|---|
| Qwen3-ASR-0.6B | 1 | 92 ms | 105 ms | 0.00940 | 106.38 |
| Qwen3-ASR-0.6B | 128 | 3210 ms | 6195 ms | 0.06400 | 2000.00 |
| Qwen3-ASR-1.7B | 1 | 102 ms | 113 ms | 0.01483 | 67.43 |
| Qwen3-ASR-1.7B | 128 | 3392 ms | 6227 ms | 0.10496 | 1219.51 |

来源：<https://arxiv.org/html/2601.21337v1> 表 2。0.6B 在并发 128 时「1 秒处理 2000 秒音频」是这条吞吐，不是单路麦克风的端到端时延。单路并发 1 的 TTFT 是 92 ms 与 102 ms，对象仍是约 2 分钟的请求，不是 2 秒流式块。

Apple Silicon、MLX 上的时延：这篇论文和模型卡都没有。第三方 MLX 仓库的自测表本篇没有打开，不抄。

### 2.2 Qwen3-ASR 已发表的中文效果

模型卡评测协议：全部模型 `dtype=torch.bfloat16`，`max_new_tokens=1024`，vLLM，greedy，不指定语言。硬件未写。表头是 WER ↓。下面只列中文与方言行；英文与多语种全表在同一模型卡。

公开集，Qwen3-ASR 两列。Whisper-large-v3 列属于同一张表、同一协议，可以放在这张表里，不能拿去和第 3 节 FunAudioLLM 的 Whisper 数字混用。

| 测试集 | Qwen3-ASR-0.6B | Qwen3-ASR-1.7B | 同表 Whisper-large-v3 |
|---|---|---|---|
| WenetSpeech net \| meeting | 5.97 \| 6.88 | 4.97 \| 5.88 | 9.86 \| 19.11 |
| AISHELL-2-test | 3.15 | 2.71 | 5.06 |
| SpeechIO | 3.44 | 2.88 | 7.56 |
| Fleurs-zh | 2.88 | 2.41 | 4.09 |
| Common Voice zh | 6.89 | 5.35 | 12.91 |
| KeSpeech | 7.08 | 5.10 | 28.79 |
| Fleurs-yue | 5.79 | 3.98 | 9.18 |
| Common Voice yue | 9.50 | 7.57 | 16.23 |
| Common Voice zh-tw | 5.59 | 3.77 | 7.84 |
| WenetSpeech-Yue short \| long | 7.54 \| 9.92 | 5.82 \| 8.85 | 32.26 \| 46.64 |
| WenetSpeech-Chuan easy \| hard | 13.92 \| 24.45 | 11.99 \| 21.63 | 14.35 \| 26.80 |

来源：<https://huggingface.co/Qwen/Qwen3-ASR-1.7B> Evaluation，与论文 4.2 节同一套数。

流式与离线对比只发表了三套公开集，不是上面整张中文表。协议：2 秒块、5 token 回退、最后四块不固定。表头仍是 WER ↓。Librispeech 两格是 clean \| other。

| 模型 | 模式 | Librispeech clean \| other | Fleurs-en | Fleurs-zh | 四格平均 |
|---|---|---|---|---|---|
| Qwen3-ASR-1.7B | 离线 | 1.63 \| 3.38 | 3.35 | 2.41 | 2.69 |
| Qwen3-ASR-1.7B | 流式 | 1.95 \| 4.51 | 4.02 | 2.84 | 3.33 |
| Qwen3-ASR-0.6B | 离线 | 2.11 \| 4.55 | 4.39 | 2.88 | 3.48 |
| Qwen3-ASR-0.6B | 流式 | 2.54 \| 6.27 | 5.38 | 3.40 | 4.40 |

来源：论文 4.5 节表 8，模型卡有同一张表。Fleurs-zh 从离线 2.41 / 2.88 变成流式 2.84 / 3.40。硬件未知。

内部集不是公开音频。论文写 Dialog-Chinese Dialects 是 22 种方言的平均，Dialog-Accented English 是 16 种口音的平均。中文相关行：

| 内部集 | 0.6B | 1.7B |
|---|---|---|
| Elders&Kids | 4.48 | 3.81 |
| ExtremeNoise | 17.88 | 16.17 |
| TongueTwister | 4.06 | 2.44 |
| Dialog-Mandarin | 7.06 | 6.54 |
| Dialog-Cantonese | 4.80 | 4.12 |
| Dialog-Chinese Dialects | 18.24 | 15.94 |

来源：同一模型卡 / 论文表 4。

Hugging Face 模型卡的 eval widget 另有一组 Open ASR Leaderboard 数：Mean WER 5.76，RTFx 147.93，AMI WER 10.56，讨论帖 <https://huggingface.co/Qwen/Qwen3-ASR-1.7B/discussions/20>。本篇没有打开榜单评测脚本，硬件未知。这组数不与论文 RTF 或中文 WER 并排。

论文还写：系统提示里的 context token 可以被当作背景知识，用来得到定制转写。这是识别解码时的上下文，不是 `007` 里吃定稿文本的改写流。改写能力不在本篇展开。

### 2.3 Qwen-Audio 与 Qwen2-Audio

权重与体积（Hugging Face API，2026-11-12 本轮读取）：

| 权重 | safetensors | `usedStorage` 字节 | 管道标签 |
|---|---|---|---|
| `Qwen/Qwen2-Audio-7B` | BF16 8,397,094,912 | 16,794,296,368 | `audio-text-to-text` |
| `Qwen/Qwen2-Audio-7B-Instruct` | BF16 8,397,094,912 | 16,887,655,544 | `audio-text-to-text` |
| `Qwen/Qwen-Audio-Chat` | BF16 8,396,222,464 | 16,792,531,608 | API 返回 `text-generation` |

官方 README 写：两个交互模式是 voice chat 与 audio analysis；当前在短于 30 秒的音频上表现较好。示例把整段音频交给 `generate`。README 没有写「边收麦克风边回改前文」的协议。量化：官方发布是 BF16；本篇没有在这两个仓库的文件列表里看到官方 int4 权重。

中文数字来自 <https://github.com/QwenLM/Qwen2-Audio/blob/main/README.md>。README 先给原始训练框架的表，再给转成 Hugging Face 之后有波动的表。表头写的是 WER，包括中文行。硬件未写。下表是原始框架那一版里和中文直接相关的格子。

| 测试集 | Qwen-Audio | Qwen2-Audio | 同表对照 |
|---|---|---|---|
| AISHELL-2 Mic \| iOS \| Android | 3.3 \| 3.1 \| 3.3 | 3.0 \| 3.0 \| 2.9 | Paraformer-large 的 iOS 为 2.9，Mic 与 Android 为「-」 |
| Common Voice 15 en \| zh \| yue \| fr | 未列入该行 | 8.6 \| 6.9 \| 5.9 \| 9.6 | Whisper-large-v3：9.3 \| 12.8 \| 10.9 \| 10.8 |
| Fleurs zh | 未列入该行 | 7.5 | Whisper-large-v3：7.7 |

同一 README 的 Hugging Face 转换后表：Qwen2-Audio 的 AISHELL-2 变为 3.2 \| 3.1 \| 2.9，Common Voice 15 变为 8.7 \| 6.5 \| 5.9 \| 9.6，Fleurs zh 变为 7.0。两套都要保留来源说明，不能并成一个「Qwen2-Audio 的中文错误率」。

FireRedASR 的 README 用另一套中文 CER 表报告了 Qwen-Audio：AISHELL-1 1.30，AISHELL-2 3.10，WenetSpeech net 9.50，meeting 10.87，四集平均 6.19，参数量写作 8.4B。见第 3.6 节。不要和上面的 AISHELL-2 三分项 WER 相减。

Librispeech（WER，原始框架）：Qwen2-Audio dev-clean / dev-other / test-clean / test-other = 1.3 / 3.4 / 1.6 / 3.6；Qwen-Audio = 1.8 / 4.0 / 2.0 / 4.2。转换后 Qwen2-Audio 变为 1.7 / 3.6 / 1.7 / 4.0。

### 2.4 Qwen2.5-Omni

README：<https://github.com/QwenLM/Qwen2.5-Omni/blob/main/README.md>。模型同时吃文本、图像、音频、视频，并以流式方式生成文本和语音。技术报告摘要写音频与视觉编码器按块处理，以便流式输入（<https://arxiv.org/abs/2503.20215>）。ASR 表没有标注这些格子是流式解码还是整段解码。已输出的转写会不会随下一块音频回改：未知。

| 权重 | safetensors 合计 | 其中 | `usedStorage` 字节 |
|---|---|---|---|
| `Qwen/Qwen2.5-Omni-3B` | 5,537,120,672 | F32 449,051,296 + BF16 5,088,069,376 | 11,984,344,622 |
| `Qwen/Qwen2.5-Omni-7B` | 10,732,225,440 | F32 449,051,296 + BF16 10,283,174,144 | 22,374,542,606 |

名字里的 3B / 7B 不是 safetensors 合计。

官方仓库另有 Thinker 的 GPTQ-Int4 与 AWQ。README 写这是为了降低 GPU 显存，并列出 RTX 3080 / 4080 / 5070 这一类设备。量化对照表把 WenetSpeech test-net 标成 WER：

| 评测 | 指标 | 7B | GPTQ-Int4 | AWQ |
|---|---|---|---|---|
| LibriSpeech test-other | WER | 3.4 | 3.71 | 3.91 |
| WenetSpeech test-net | WER | 5.9 | 6.62 | 6.31 |

同页还有一张显存表，对象是 15 / 30 / 60 秒视频，不是纯语音。BF16、15 秒视频写 31.11 GB；GPTQ-Int4 写 11.64 GB。模型行名写作 Qwen-Omni-7B。不把这张表当成 ASR 时延。

ASR 主表在 “Audio -> Text” 下，中文相关格子如下。大段表头没有逐格写 CER 或 WER。后文把 test-net 的 5.9 标成 WER，与 7B 的 5.9 对得上 test-net 这一格。

| 测试集 | Omni-3B | Omni-7B |
|---|---|---|
| Common Voice 15 en \| zh \| yue \| fr | 9.1 \| 6.0 \| 11.6 \| 9.6 | 7.6 \| 5.2 \| 7.3 \| 7.5 |
| Fleurs zh \| en | 3.2 \| 5.4 | 3.0 \| 4.1 |
| WenetSpeech test-net \| test-meeting | 6.3 \| 8.1 | 5.9 \| 7.7 |

同表 Qwen2-Audio 的 Common Voice 15 为 8.6 \| 6.9 \| 5.9 \| 9.6，Fleurs zh 为 7.5，与 Qwen2-Audio 自己 README 的原始框架表一致；Qwen2-Audio 没有出现在这张 WenetSpeech 行里。硬件未写。

同一仓库的 Qwen3-Omni README 又给了 Qwen2.5-Omni 一行 WenetSpeech 5.91 \| 7.65、Fleurs-zh 2.54、CV15-zh 5.13。Fleurs-zh 的 2.54 与本表 3.0 对不上，所以两张表保持分开。

### 2.5 Qwen3-Omni

README：<https://github.com/QwenLM/Qwen3-Omni/blob/main/README.md>。`Qwen/Qwen3-Omni-30B-A3B-Instruct` 的 safetensors 为 BF16 35,259,818,545，`usedStorage` 70,523,299,202 字节。架构文字：MoE Thinker–Talker，AuT 预训练，多码本；实时音视频交互，文本或语音可以马上出来。ASR 提示表把中文转写写成「请将这段中文语音转换为纯文本。」评测说明：Instruct 使用 greedy、不采样。硬件未写。

下表只摘该 README 里 `Qwen3-Omni-30B-A3B-Instruct` 这一列，表头为 EN & ZH ASR (wer)。Flash 列是接口名，不放进开源权重行。

| 测试集 | Qwen3-Omni-30B-A3B-Instruct |
|---|---|
| WenetSpeech net \| meeting | 4.69 \| 5.89 |
| Librispeech clean \| other | 1.22 \| 2.48 |
| CV15-zh | 4.31 |
| Fleurs-zh | 2.20 |

ASR 用的是哪一种流式块、前文是否不固定：这张表没有写。Qwen3-ASR 论文写明后训练把模型收成不跟随自然语言指令的 ASR。Omni Instruct 仍按提示工作，同一权重还能做语音生成和翻译。候选：把它接到只消费采样帧的识别流时，输出文本是否「只是转写」取决于提示；这是模型卡与论文已写的机制差异，不是本篇的绑定。

### 2.6 不担任识别流的近邻

`Qwen/Qwen3-ForcedAligner-0.6B` 的输入是语音加上已经写好的文本，输出是时间戳。论文：非自回归，11 种语言，最长 300 秒，帧 80 ms。它不从采样帧单独产生可修订转写。速度在论文表 2，PyTorch + FlashAttention + bfloat16；并发 1 的离线 RTF 为 0.00889。GPU 型号同样未点名。

## 3. 其他已发表且可核对的开源识别家族

本节每张错误率表只来自一个页面。Whisper、Paraformer、SenseVoice、Zipformer 都有公开论文或模型卡。Fun-ASR、FireRedASR、GLM-ASR-Nano 是同一时期能核对到权重页的另外三支。

### 3.1 Whisper

官方模型表：<https://github.com/openai/whisper/blob/main/README.md>。相对速度是在 A100 上转写英文，相对 `large`。VRAM 是 README 的约数。

| 尺寸 | 参数量 | 多语权重名 | README 的 VRAM | 相对速度 |
|---|---|---|---|---|
| tiny | 39 M | `tiny` | ~1 GB | ~10x |
| base | 74 M | `base` | ~1 GB | ~7x |
| small | 244 M | `small` | ~2 GB | ~4x |
| medium | 769 M | `medium` | ~5 GB | ~2x |
| large | 1550 M | `large` | ~10 GB | 1x |
| turbo | 809 M | `turbo` | ~6 GB | ~8x |

README 写 `turbo` 是 `large-v3` 的优化版，更快，精度损失小；`turbo` 不训练翻译任务。多语种识别仍用 tiny / base / small / medium / large 这条线。`large-v3` 与 `large-v2` 的分语言 WER 或 CER 在 README 里是一张图，本篇不从图里读数。

Hugging Face API：

| 仓库 | safetensors | 许可证字段 | `usedStorage` 字节 |
|---|---|---|---|
| `openai/whisper-large-v3` | F16 1,543,490,560 | `apache-2.0` | 31,658,039,844 |
| `openai/whisper-large-v3-turbo` | F16 808,878,080 | `mit` | 5,096,208,915 |
| `openai/whisper-small` | F32 241,734,912 | `apache-2.0` | 6,206,629,867 |

`whisper-large-v3` 的 `usedStorage` 远大于一份 F16 参数的体积，仓库里有多份格式。不把 31 GB 当成一次加载的模型大小。README 的 large VRAM 约 10 GB 才是官方给出的显存约数，且没有按 large-v3 单独改写。

官方 README 的用法是对音频文件做转写，没有描述边说边改前文。中文错误率不用 2022 年原论文的图代替 large-v3。large-v3 的中文数只出现在别人的对照表里，见 2.2、3.2、3.6，各自协议不同。

### 3.2 Paraformer 与 SenseVoice：同一篇 FunAudioLLM 报告

来源：<https://arxiv.org/html/2407.04051v3> 表 6 与表 7。中文、粤语、日语、韩语用 CER，其余语言用 WER；文本归一化对齐 Whisper 的做法，中文另加一套归一化。效率在一台 A800 上、batch size 1。自回归模型（Whisper-S、Whisper-L-V3、SenseVoice-L）beam size 5。这是「转写一段已经完整的音频要多久」，不是部分假设的修订时延。

表 6，错误率：

| 测试集 | Whisper-S | Whisper-L-V3 | SenseVoice-S | SenseVoice-L | Paraformer-zh |
|---|---|---|---|---|---|
| AISHELL-1 test | 10.04 | 5.14 | 2.96 | 2.09 | 1.95 |
| AISHELL-2 test_ios | 8.78 | 4.96 | 3.80 | 3.04 | 2.85 |
| WenetSpeech test_meeting | 25.62 | 18.87 | 7.44 | 6.73 | 6.97 |
| WenetSpeech test_net | 16.66 | 10.48 | 7.84 | 6.01 | 6.74 |
| Common Voice zh-CN | 19.60 | 12.55 | 10.78 | 7.68 | 10.30 |
| Common Voice yue | 38.97 | 10.41 | 7.09 | 6.78 | - |

表 7，结构与时延：

| 模型 | 结构 | 参数量 | 语言 | RTF | 10 秒音频耗时 |
|---|---|---|---|---|---|
| Whisper-S | 自回归 | 224 M | 50+ | 0.042 | 518 ms |
| Whisper-L-V3 | 自回归 | 1550 M | 50+ | 0.111 | 1281 ms |
| Paraformer-zh | 非自回归 | 220 M | zh | 0.009 | 100 ms |
| SenseVoice-S | 非自回归 | 234 M | zh, yue, en, ja, ko | 0.007 | 70 ms |
| SenseVoice-L | 自回归 | 1587 M | 50+ | 0.110 | 1623 ms |

论文正文还写 SenseVoice-Small 的识别时延低于 80 ms，并称 SenseVoice 与 CosyVoice 已在 ModelScope 和 Hugging Face 开源。本篇打开的仓库是 `FunAudioLLM/SenseVoiceSmall`：`pipeline_tag` 为 automatic-speech-recognition，`cardData.license` 为 `other`，`license_name` 为 `model-license`，链接指向 FunASR 的 `MODEL_LICENSE`。SenseVoice-Large 的仓库路径本篇没有确认。CosyVoice 是语音生成，不列入识别表。

SenseVoice-Small 是非自回归端到端，一次前向给出转写，还可以带情感和音频事件标签。论文没有写部分文本回改。

Paraformer 的开源权重分成离线和流式两份，不要把表 6 的错误率安到流式权重上。

| 仓库 | 模型卡写法 | `usedStorage` 字节 | 许可证字段 |
|---|---|---|---|
| `funasr/paraformer-zh` | 非自回归，220 M，中英，16 kHz，训练数据 60,000 小时以上；非流式，可带时间戳 | 880,919,754 | `apache-2.0` |
| `funasr/paraformer-zh-streaming` | Paraformer-Streaming，220 M，中英，流式，16 kHz | 880,893,613 | `apache-2.0` |

流式块的模型卡示例：

```python
# 来源：https://huggingface.co/funasr/paraformer-zh-streaming 的 Quick Start
chunk_size = [0, 10, 5]  # [lookback, chunk, lookahead]，单位是 60 ms 帧
result = model.generate(
    input="audio.wav",
    chunk_size=chunk_size,
    encoder_chunk_look_back=4,
    decoder_chunk_look_back=1,
)
```

10 帧乘 60 ms 等于 600 ms 的中心块。示例对整段 wav 调用 `generate`，并带 cache 参数的说明在 FunASR 文档的分块循环里。已打印的字会不会被下一块改掉：该示例没有写。流式权重自己的 AISHELL / WenetSpeech 错误率：本篇打开的页面里没有。

### 3.3 Zipformer

来源：<https://github.com/k2-fsa/icefall/blob/master/egs/wenetspeech/ASR/RESULTS.md>，WenetSpeech 字级训练，L 子集。指标是 CER，三格顺序为 dev、test-net、test-meeting。解码硬件未写。`chunk-size` 与 `left-context-frames` 没有在该文件里换成毫秒。

流式 Zipformer，参数量约 76 M，epoch 12：

| 设置 | greedy | modified beam search |
|---|---|---|
| chunk-size 16，left-context-frames 256 | 8.45 / 9.89 / 16.46 | 8.21 / 9.77 / 16.07 |
| chunk-size 32，left-context-frames 256 | 8.0 / 9.0 / 15.11 | 7.84 / 8.94 / 14.92 |

同文件的非流式 Zipformer，同样约 76 M，epoch 12：greedy 7.36 / 7.65 / 12.43，beam 7.32 / 7.61 / 12.35。非流式与流式是不同训练（`causal` 0 与 1），不是同一个 checkpoint 的两种开关。

预训练权重页：流式 <https://huggingface.co/pkufool/icefall-asr-zipformer-streaming-wenetspeech-20230615>，许可证字段 `apache-2.0`。该仓库 `usedStorage` 为 13,231,936,531 字节，含训练产物，不能当成一份 76 M 推理权重的体积。int8 推理体积：未知。

更早的 Pruned Transducer 5 流式行也在同一 RESULTS：modified beam search 的 DEV / TEST-NET / TEST-MEETING 为 8.53 / 9.95 / 15.81（epoch 7，avg 1）。那是另一支 transducer，不是上面的 Zipformer 76 M。

sherpa-onnx 文档把 Zipformer transducer 标成 online 模型，示例 JSON 含 `is_final: false` 和一段部分文本（<https://k2-fsa.github.io/sherpa/onnx/pretrained_models/online-transducer/zipformer-transducer-models.html>）。部分假设存在。`is_final` 之前的字是否还会改：示例没有写。同页出现过 `sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23` 对单条 wav 的 RTF，没有硬件，本篇不把这条 RTF 记成性能。

### 3.4 Fun-ASR-Nano

权重页：`FunAudioLLM/Fun-ASR-Nano-2512` 与 `FunAudioLLM/Fun-ASR-MLT-Nano-2512`。两者 `cardData.license` 都是 `apache-2.0`，`usedStorage` 分别为 3,957,628,670 与 3,957,569,803 字节。safetensors 参数合计字段为空。仓库内有 `Qwen3-0.6B` 目录，说明这支是带语言模型的 ASR，不是第 2 节的 Qwen3-ASR 权重。

GitHub README（<https://github.com/QwenAudio/Fun-ASR>）写：Nano-2512 覆盖中文、英文、日文以及汉语方言和口音；MLT-Nano-2512 是 800 M、31 种语言的另一份 checkpoint。流式示例调用 `streaming_generate`，`chunk_ms = 720`，打印 `fixed_text`。`fixed_text` 表示有一段被标成已固定的文本。未固定部分是否回改：该 README 片段没有写。分项 CER 在检索摘要里表格错位，本篇没有从对齐后的模型卡拉出逐格数字，所以这里不写 CER。

### 3.5 FireRedASR

来源：<https://github.com/FireRedTeam/FireRedASR/blob/master/README.md>。中文 CER，英文 WER。硬件与 beam 未在该表旁写明。

| 模型 | 参数量 | AISHELL-1 | AISHELL-2 | WenetSpeech net | WenetSpeech meeting | 四集平均 |
|---|---|---|---|---|---|---|
| FireRedASR-LLM | 8.3 B | 0.76 | 2.15 | 4.60 | 4.67 | 3.05 |
| FireRedASR-AED | 1.1 B | 0.55 | 2.52 | 4.88 | 4.76 | 3.18 |
| Qwen-Audio | 8.4 B | 1.30 | 3.10 | 9.50 | 10.87 | 6.19 |
| SenseVoice-L | 1.6 B | 2.09 | 3.04 | 6.01 | 6.73 | 4.47 |
| Whisper-Large-v3 | 1.6 B | 5.14 | 4.96 | 10.48 | 18.87 | 9.86 |
| Paraformer-Large | 0.2 B | 1.68 | 2.85 | 6.74 | 6.97 | 4.56 |

同页方言与英文：FireRedASR-LLM 的 KeSpeech 3.56，LibriSpeech test-clean / test-other 1.73 / 3.67；AED 为 4.48、1.93、4.44。

`FireRedTeam/FireRedASR-AED-L` 的 `usedStorage` 为 4,678,849,421 字节，许可证 `apache-2.0`。LLM 版 README 写还要另放 `Qwen/Qwen2-7B-Instruct`。流式增量假设：该 README 的用法是整段转写，没有块协议。

这张表的 Paraformer AISHELL-1 是 1.68，FunAudioLLM 表 6 的 Paraformer-zh 是 1.95。SenseVoice-L 与 Whisper-large-v3 的若干格子则与 FunAudioLLM 表 6 相同。相同与不相同都保留，不合并成一个「Paraformer 的 CER」。

### 3.6 GLM-ASR-Nano-2512

仓库 `zai-org/GLM-ASR-Nano-2512`。模型卡正文写 1.5 B 参数，许可证 `mit`。safetensors 统计是 BF16 2,257,843,200，`usedStorage` 9,040,649,552 字节。1.5 B 与 2.26 B 同时存在，本篇不把它们收成一个数。

模型卡写平均错误率 4.10，并说在 Wenet Meeting、AISHELL-1 等中文集上有优势。分项是图片，本篇不从图里抄数。硬件未知。示例是整段 `generate`，流式回改协议未知。Qwen3-ASR 论文的多语种表把 GLM-ASR-Nano-2512 当作对照，那是另一套平均 WER，不放到这句 4.10 旁边。

## 4. 和识别流端口对应的事实

只记录输入输出上已读到的事实。哪一份权重接到识别流，是 `018` 的事。

- 采样率写在帧上，不由权重名隐含。本篇能直接引用的是：Qwen3-ASR 流式 demo 16,000 Hz；Paraformer 两份模型卡 16 kHz；Qwen3-ASR 的特征是 128 维 Fbank。Qwen2-Audio 示例把音频重采样到 `feature_extractor.sampling_rate`，本篇没有再打开那个 JSON 核对具体整数。
- 能在音频尚未结束时给出部分文本，并且来源写了「近期块不固定」的，是 Qwen3-ASR 的 vLLM 流式路径。离线 `transcribe()` 一次返回整段文本。
- Paraformer 用两份权重区分流式和离线。Zipformer 的流式与非流式在 icefall 里是两次训练。SenseVoice-Small 与 Whisper 官方路径是整段或文件级转写。
- Qwen-Audio、Qwen2-Audio、两代 Omni 都能从音频得到文本，也都能按提示做转写以外的事。Qwen3-ASR 论文写自己被训练成不跟随这类指令。
- 时间戳不是识别假设。Qwen3-ASR 的时间戳走 ForcedAligner，且流式路径不返回时间戳。

## 5. 开放问题

1. Qwen3-ASR 效率表的 “single typical compute resource” 是哪一种 GPU：未知。因此 92 ms TTFT 不能写成某张显卡上的数字。
2. 2 秒流式块的用户侧端到端时延（从一块 PCM 到假设文本）：论文给了 WER 协议，没有给毫秒。
3. Paraformer-zh-streaming、Zipformer 流式解码里，已经显示的字是否回改：未知。
4. SenseVoice-Large 的权重仓库路径：论文说已开源，本篇只确认了 SenseVoiceSmall。
5. Fun-ASR-Nano 的逐测试集 CER，以及 GLM-ASR 图片里的分项：本篇未形成可引用的格子。
6. GLM 模型卡 1.5 B 与 safetensors 2.26 B 各指哪一部分：未知。
7. Qwen2.5-Omni 自己的 Fleurs zh 3.0 与 Qwen3-Omni README 里 Qwen2.5-Omni 的 Fleurs-zh 2.54 为何不同：未知。两边硬件都未写。
8. 上述模型在 Apple Silicon 上的时延与内存：官方 ASR 表里没有。MLX 上的加载方式不在本篇。
9. sherpa-onnx 目录还列出 Conformer transducer、LSTM transducer、CTC、TeleSpeech、Moonshine、WeNet 等运行时包装。除本篇已打开的 Zipformer 页外，那些页的指标没有逐页核对。

## 6. 检索日志

| # | 动作 | 地址或查询 | 结果 |
|---|---|---|---|
| 1 | 检索 | Qwen3-ASR huggingface model card streaming latency Chinese | 命中 `Qwen/Qwen3-ASR-1.7B`、论文 2601.21337、流式 WER 表 |
| 2 | 检索 | Qwen2-Audio 与 Qwen2.5-Omni ASR 模型卡 | 命中两代音频模型的 Librispeech / Common Voice / Fleurs / WenetSpeech 表 |
| 3 | 检索 | openai whisper large-v3 turbo 参数量 | 命中尺寸表与 turbo 剪枝描述；第三方 Mac 时延未采用 |
| 4 | 检索 | FunASR Paraformer-zh streaming 220M | 命中离线与流式两份 220 M 权重，以及 600 ms 分块示例 |
| 5 | 检索 | SenseVoiceSmall CER RTF | 命中 FunAudioLLM 论文表；第三方 Mixpeek 页不采用 |
| 6 | 检索 | sherpa-onnx zipformer streaming Chinese | 命中 online Zipformer 文档与 icefall RESULTS 链接；单条 wav 的 RTF 无硬件 |
| 7 | 检索 | Qwen2-Audio-7B-Instruct streaming WenetSpeech | 官方卡是整段音频生成；第三方站的 AISHELL-1 1.4 与后来打开的 README 不符，不采用 |
| 8 | 打开 | <https://huggingface.co/Qwen/Qwen3-ASR-1.7B> | 读到评测协议、中文与方言 WER、流式限制、Apache-2.0、BF16 |
| 9 | 打开 | <https://qwen.ai/blog?id=qwen3asr> | 页面无正文 |
| 10 | 检索 | Qwen3-ASR 文件体积、vLLM 流式 | 命中官方 GitHub 与 vLLM 配方；NVFP4 为第三方量化，不写入官方行 |
| 11 | 检索 | FireRedASR 参数量与 AISHELL CER | 命中 GitHub 表：AED 1.1 B、LLM 8.3 B |
| 12 | 检索 | Qwen3-ASR technical report RTF TTFT AuT | 命中 92 ms、RTF 0.064、编码器 300 M / 180 M；随后用论文 HTML 核对 |
| 13 | 打开 | Hugging Face API `Qwen/Qwen3-ASR-1.7B` | BF16 2,349,217,408；usedStorage 4,699,896,190；license apache-2.0 |
| 14 | 打开 | Hugging Face API `Qwen/Qwen3-ASR-0.6B` | BF16 938,008,576；usedStorage 1,876,091,704 |
| 15 | 检索 | funasr/paraformer-zh 模型卡 | 命中 220 M、16 kHz、60,000 小时、apache-2.0 |
| 16 | 打开 | <https://arxiv.org/html/2601.21337v1> 开头 | 架构、训练、表 1、表 2 全文；GPU 型号未出现 |
| 17 | 打开 | `Qwen3-ASR-1.7B` 的 `preprocessor_config.json` | WhisperFeatureExtractor，128 维，hop 160，n_samples 480000；无 sampling_rate 键 |
| 18 | 打开 | Qwen2-Audio README | 原始框架与 Hugging Face 转换后两套 ASR 表；30 秒以内；无流式回改协议 |
| 19 | 打开 | icefall `egs/wenetspeech/ASR/RESULTS.md` | Zipformer 流式与非流式 CER，约 76 M |
| 20 | 打开 | 论文 HTML 偏移 30000 | 读到 4.5 节：2 秒块、5 token 回退、最后四块不固定，以及表 8 |
| 21 | 打开 | 论文 HTML 偏移 42000 | 落到文末参考文献，没有新的时延表 |
| 22 | 打开 | <https://arxiv.org/html/2407.04051v3> 表 6 与表 7 所在节 | A800、batch 1、CER/WER、SenseVoice 与 Paraformer 时延 |
| 23 | 打开 | Whisper 官方 README 模型表 | A100 英文相对速度与 VRAM；分语言结果是图 |
| 24 | 打开 | Hugging Face API：Qwen2-Audio、Omni、Qwen-Audio-Chat、Whisper、Fun-ASR、Paraformer、Zipformer 仓库、FireRedASR-AED-L、GLM-ASR | 参数量或 usedStorage、许可证字段；Fun-ASR 与 Paraformer 无 safetensors 合计 |
| 25 | 打开 | Qwen2.5-Omni README 的 ASR 与 GPTQ/AWQ 段 | 中文格子、Int4/AWQ 的 test-net WER、视频显存表 |
| 26 | 打开 | GLM-ASR-Nano-2512 README | 正文 1.5 B 与平均 4.10；分项在图片中 |
| 27 | 打开 | FireRedASR README 评测表 | 中文 CER 表与 KeSpeech、LibriSpeech |
| 28 | 打开 | sherpa-onnx Zipformer 文档页首 | 确认 online transducer 目录；指标主体未在返回的导航段里 |
| 29 | 打开 | Qwen3-Omni README 的 ASR 表与提示表 | Instruct 列的 WenetSpeech、CV15-zh、Fleurs-zh；greedy；流式描述与 ASR 表未绑定 |
| 30 | 打开 | Fun-ASR GitHub 检索摘录与 HF API | 800 M 的 MLT-Nano、`chunk_ms=720`、`fixed_text`；逐格 CER 未对齐，不写入 |
| 31 | 本地阅读 | `dev-docs/notes/014-research-open-model-mlx-index.md` | 015 只回答识别流的体积、时延、中文、是否流式 |
| 32 | 本地阅读 | `dev-docs/notes/007-derivation-voice-input-primitives.md` 原语 4 | 用来对照「可修订文本假设」，不另写接口 |

空结果是第 9 条。第 7 条的第三方数字因与随后打开的 README 冲突而弃用。第 21 条没有新表。

<!-- research-complete -->
