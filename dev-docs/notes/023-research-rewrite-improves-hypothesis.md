# 023-research-rewrite-improves-hypothesis：替换文本是否更正确

本文只核对 `019` 第 4 条：哪一档 Qwen 文本检查点，在哪一个测试集上，替换文本的错误少于识别假设原文。没有这一对数字的档，保持未知。不把 2023 年 Qwen-7B 的 CER 上升写成每一档都不该替换，也不把单独一行的下降写成某一档已经更好。豆包输入法只作机制对照：替换前有单独判定，不替换是正常结果。

`007` 原语 5 的输出是「不替换」或「替换为这段文本」。「不替换」是正常输出，不是失败。`018` 第 3 节写明全文比较发生在适配器里。本文不改这两句的类型。

## 1. 豆包对照：判定与替换不是同一次返回

`004` 第 3.2 节：`SmartOrganizeVoiceCoordinator` 产出 `SmartOrganizeVoiceDecision`，其中有 `rewriteDecision`。该节写明，并非所有识别结果都要改写；若判定无需改写或改写置信度不足，则直接定稿原始 ASR 文本。同一节写，SSE 结束帧内容为空时抛出 `scene.completed.empty_content`，并保留原始 ASR 文本。这是替换前的单独判定。不替换在这条路径上是正常结果。`004` 没有给出任一 Qwen 检查点的 CER、标点 F1 或口语清理分数。

`002` 第 4.3 节的就地替换是另一件事：润色结果准备覆盖前，校验宿主光标是否仍在原文本末尾；焦点已转移或用户已改字则走 `smartTargetRejected` / `failedToSmartOriginal`。那是宿主目标还在不在，不是「替换文本错误更少」的分数。

`003` 的 `W4A8ImeAsrQwen3Transformer` 位于离线识别解码。该笔记没有文本检查点在某一测试集上相对识别假设的 CER。不把这个算子名当成改写权重量到的成绩。

## 2. 事实：已在 `016` 记下、本轮不外推

三组测试集、指标、名字粒度都不同。`016` 已把数字留在各自的表里。本轮重读了该节，不把三组合并，也不用其中一组填另一档。

| `016` 的位置 | 论文里的名字 | 本轮沿用的边界 |
| --- | --- | --- |
| 第 5.1 节，HKCanCor，标点 P/R/F1 | `Qwen-2.5-72b` | 没有对应到 Instruct 或 Base。指标是标点类别的 F1，不是替换文本相对识别假设的 CER |
| 第 5.2 节，ASR-EC，CER | Bai 等 2023 的 Qwen，参数量 7B | 不是 Qwen2.5，也不是 Qwen3。零样本、三样本、多步提示的 CER 高于同表 ASR 基线。只约束这一档、这一套提示 |
| 第 5.3 节，Aishell-1 等 | 正文写 “Qwen2.5” | 没有 7B、14B 或 72B。分数不能安到某一档 |
| 第 5.4 节 | 口语清理 | 当时检索没有点名 Qwen 文本检查点的 F1。空结果不是零分 |

`016` 第 4 节的 tok/s 是生成速度。不与下面任一 CER 相除。

## 3. 事实：点名 `Qwen2.5-3B-Instruct` 的一次文本纠错

来源：*Selective Gating for LLM-Based Chinese ASR Error Correction*，Journal of Big Data and Computing，Vol. 4 No. 4，2026。本次打开的是 `http://www.stemmpress.com/uploadfile/202611/0b5e94c14382a55.pdf`（本地抽取文本）。模型字符串是 `Qwen2.5-3B-Instruct`，引用 Qwen2.5 技术报告。PDF 没有写出 `Qwen/Qwen2.5-3B-Instruct` 这一仓库路径。部署写的是本地 4-bit NF4，解码是确定性贪心。这不是 `016` 里 MLX 4-bit 的 group size，两套量化不并表。

识别侧是 Whisper-small，语言固定中文，temperature 为 0，`condition_on_previous_text` 关闭。纠错输入是 1-best 假设，不是 N-best。提示要求只改明确的识别错误，不润色文风，不新增信息，不删除已有信息，不确定内容保持原样。另有一条启发式：中文输入不含拉丁字母、而输出新引入拉丁字母时，退回原始 ASR 假设。论文把它写成管线中的启发式。表中的 Always LLM 行写的是直接接受全部 Qwen 输出。本文不把这两句合成「该行已经含这条回退」或「该行不含」。

语料是 Mozilla Common Voice Scripted Speech 26.0 的 Chinese (China)（zh-CN）。论文写官方页有 852,010 条语音、239.59 小时已校验语音；本地下载的 `validated.tsv` 有 190,017 条。开发集 100 条；剔除这些音频后，以 `random_state = 2026` 再抽 1,000 条作独立测试集。论文写两集在 `audio_path` 上无重叠，但没有核验说话人或转写文本是否不相交，因此不声称说话人不相交。

评分前把繁体转为简体、小写，并去掉空格和常见中英文标点。CER 是 \((S+D+I)/N\)。Macro CER 是句级 CER 的算术平均，Corpus CER 是先累计编辑再除。短句多时 Macro CER 对少量短句敏感。标点已在评分前去掉，所以下面的格子不是标点恢复分数。

开发集与测试集分开列表。论文写开发集上的差距只用于选择策略，不解释成稳定的性能提高。

开发集（100 条）：

| 方法 | Macro CER | Corpus CER | 接受条数 | 变好 | 变差 |
| --- | --- | --- | --- | --- | --- |
| Whisper | 38.67% | 22.18% | 0 | — | — |
| Always LLM | 42.94% | 27.45% | 100 | 8 | 41 |
| Edit Gate | 38.71% | 22.26% | 3 | 1 | 2 |
| Confidence Gate | 38.30% | 22.34% | 31 | 3 | 5 |
| Joint Gate | 38.60% | 22.09% | 15 | 1 | 0 |

正文另写这 100 条里 51 条不变（8+51+41=100）。Confidence Gate 的 Macro CER 低于 Whisper，Corpus CER 高于 Whisper。论文写它在 Corpus CER 上没有超过 Whisper。

独立测试集（1,000 条，阈值冻结后只评一次）：

| 方法 | Macro CER | Corpus CER | 接受条数 | 变好 | 不变 | 变差 |
| --- | --- | --- | --- | --- | --- | --- |
| Whisper | 34.75% | 24.50% | 0 | — | — | — |
| Always LLM | 44.75% | 30.56% | 1000 | 72 | 459 | 469 |
| Confidence Gate | 39.26% | 25.28% | 243 | 16 | 907 | 77 |
| Final Joint Gate | 34.79% | 24.53% | 91 | 0 | 995 | 5 |

直接接受全部替换时，两项 CER 都高于 Whisper。Final Joint Gate 接受 91 条，论文写这是测试集的 9.10%。这 91 条里没有一条句级 CER 变好；5 条变差。门控后的 Macro CER 34.79% 与 Corpus CER 24.53% 都略高于 Whisper 的 34.75% 与 24.50%。论文写没有做显著性检验，因此不把 34.75% 与 34.79% 的差解释成有统计意义。论文自己的句子是：减少有害修改主要来自拒绝大部分替换，不应描述成纠错准确率的提高。

开发集上变差的 41 条由单人归类：语义替换 14（34.15%），内容增补 9（21.95%），专名损坏 7（17.07%），信息删除 5（12.20%），幻觉 3（7.32%），不必要改写 3（7.32%）。没有标注者间一致率。这是错误类型计数，不是另一档模型的 CER，不并进上表。

这篇只覆盖名字 `Qwen2.5-3B-Instruct`、4-bit NF4、贪心、1-best、上述抽样。它不覆盖 Qwen2.5 的其他参数档，也不覆盖 Qwen3。

## 4. 事实：点了 72B 与 1.5B、但没写 Instruct 或 Base

来源：Yan Jiang 等，*Game-Oriented ASR Error Correction via RAG-Enhanced LLM*，arXiv:2509.23630v1，本次打开 `https://arxiv.org/html/2509.23630v1`。任务是中文游戏语音纠错。测试集是《Arena Breakout》玩家实况语音：3,872 条，2.58 小时，来源列写 Player。训练 32,754 条、22.16 小时，来源 TTS 与 Player；验证 3,579 条、1.98 小时，来源 Player。不是 Aishell，也不是 Common Voice。

基线列表里的字符串是 `Qwen2.5-72B` 与 `Qwen2.5-1.5B`。表 III 的行名是 `Qwen-72B w/o SFT` 与 `Qwen-1.5B w/ SFT`。HTML 没有写出 Instruct、Base，也没有 `Qwen/Qwen2.5-72B-Instruct` 这类仓库路径。实现节写 GO-AEC 的基座是 `Qwen2.5-1.5B`，实验在 NVIDIA A100 上进行，学习率 \(1\times10^{-5}\)，批大小 32。没有单独写出 72B 的量化或另一台机器。

提示协议在两条基线之间并不相同，本文不把它说成同一种改写：

- `Qwen2.5-72B`：无领域微调，用与框架相同的提示生成纠正。框架推理节写，多个 ASR 服务先产生 N-best，再与游戏背景一起构成提示；检索命中时把术语条目补进提示。
- `Qwen2.5-1.5B`：在游戏 ASR 输出与参考转写上微调。该条写明使用与框架相同的提示，但不接入 RAG。列表里「The same LLM」紧挨着另一个参数量，与上一行的 72B 不是同一个括号。本文保留这个并置，不把它改写成单一仓库。

表 III，CER 与 SER，越低越好。对照行是该文「最佳单路 ASR、无后处理」：

| 表中的行 | CER | SER |
| --- | --- | --- |
| Vanilla ASR Output | 9.87% | 46.51% |
| Qwen-72B w/o SFT | 7.07% | 34.68% |
| Qwen-1.5B w/ SFT | 6.79% | 24.66% |

这两行的 CER、SER 都低于同表的 Vanilla ASR。论文没有给这两行单独的相对下降百分比。摘要里的 CER 下降 6.22 个点对应的是 GO-AEC 的 3.65% 相对 9.87%，不是 72B 这一行。

表 IV 是同一指标下的消融，行名不是检查点名。`GO-AEC w/o SFT` 的 CER 是 13.66%，SER 是 50.10%，都高于 Vanilla 的 9.87% 与 46.51%。正文写这一行是去掉领域微调、仍保留 RAG 与 N-best 的预训练模型。实现节把框架基座写成 `Qwen2.5-1.5B`。表 IV 没有把行名印成 `Qwen2.5-1.5B`。因此 13.66% 不记成「`Qwen2.5-1.5B` 零样本 CER」。

## 5. 事实：点了 Qwen3 尺寸，但没有识别假设的 CER

来源：Junjie An 等，*Retrieval-Augmented Self-Taught Reasoning Model with Adaptive Chain-of-Thought for ASR Named Entity Correction*，arXiv:2602.12287v1，本次打开 `https://arxiv.org/html/2602.12287v1`。纠正模块用 Qwen3 系列的 0.6B 与 8B，引用 Qwen3 技术报告。HTML 没有写出 `Qwen/Qwen3-8B` 或 `Qwen/Qwen3-0.6B`。微调在 8 张 40GB NVIDIA H20 上。nothinking / thinking 行被写成预训练 Qwen3。

测试集是 AISHELL-1，另有从 AISHELL-1 抽出的 Homophone 115 条。假设来自与 DANCER 相同的、在 AISHELL-1 上训练的 Conformer。表 2 的对照行是 DANCER 及其 NER 替换，不是未纠错的识别假设。表中没有识别假设原文的 CER。

因此，下面这些格子不能回答「替换是否比原假设错误更少」。AISHELL-1 上预训练行的总 CER 只说明它相对 DANCER 的位置：`Qwen3 (8B)-nothinking` 为 4.27，`Qwen3 (8B)-thinking` 为 4.24，DANCER 为 4.29；`Qwen3 (0.6B)-nothinking` 为 4.69，`Qwen3 (0.6B)-thinking` 为 4.27。SFT 与 RASTAR 行是微调后的系统，不是未改权重的检查点。Homophone 同样没有识别假设行，格子不转写到这里，避免与 AISHELL-1 并成一张「已经更好」的表。

识别假设 CER：未知。0.6B 与 8B 是否使替换文本错误更少：未知。

## 6. 事实：口语清理与标点，打开后仍对不上这一问

口语清理的已发表 F1，本轮检索仍然没有落到一份点名 Qwen 文本检查点的分数表。空结果不写成零分。

打开了 MinMo（arXiv:2501.06282v1 的 HTML，`https://arxiv.org/html/2501.06282v1`）里与口语顺滑相邻的一节。SWAB 测试集是中英会议、播客、讲座 ASR 转写的书面改写，测试 100 段。表 15 的行名是 `Qwen2.5-7B`，不是 `Qwen2.5-7B-Instruct`，也没有仓库路径。同句写做了全量微调再与 “Qwen2.5-7B-based model” 比较，没有分开写这一行是否也在 SWAB 上微调。参照是人工书面文本。该行 CER 0.75，BLEU 21.06，ROUGE-L 42.01，BLEURT 64.61；S-Faithful 人工 9.04、LLM 8.06；S-Formal 人工 9.80、LLM 7.63。表中没有未改写 ASR 转写相对同一书面参照的 CER。论文写词汇匹配不一定反映这项任务。这些格子不是「相对识别假设的错误更少」，也不是 F1，不与第 3、4 节并表。

同一 HTML 的标点与逆文本归一化表（表 16）比较的是 MinMo、SenseVoice-L、whisper-large-v3 在 Fleurs 中英子集上的 GPT-4 Turbo 排名分。行里没有 Qwen。不把 MinMo 的排名分记成 Qwen 文本检查点的标点 F1。

`fanolabs/cantonese-punctuation-restoration` 的检索摘要里有一条命令使用 `Qwen/Qwen2.5-7B-Instruct`。摘要没有给出该命令的 P/R/F1。本轮没有打开一份 7B Instruct 的 HKCanCor 分数表。`016` 已记下的仍是论文名 `Qwen-2.5-72b` 的 F1，不是仓库路径，也不是相对识别假设的 CER。

检索里出现过座舱口语理解的意图准确率，以及安全评测里的攻击成功率。前者不是替换文本的 CER，后者的 ASR 不是语音识别。两组数字都不转入正文。

## 7. 仍然未知

下面每一条都没有「该检查点、该测试集、替换文本 CER 低于识别假设 CER」这一对数字。

- Qwen2.5 的 0.5B、7B、14B、32B，无论 Instruct 或 Base：未知。7B 只在第 6 节出现过书面参照上的 CER，且行名没有 Instruct。
- `Qwen2.5-3B-Instruct` 以外的量化（含 MLX 4-bit）在 Common Voice zh-CN 或其他普通话测试集上：未知。第 3 节的 NF4 不外推。
- `Qwen2.5-72B` 与 `Qwen2.5-1.5B` 的 Instruct 或 Base 身份：未知。第 4 节的下降只属于该文表 III 的行、该游戏测试集、该文写下的提示，不属于「这一档已经更好」。
- `Qwen2.5-1.5B` 在单条 1-best、不微调、不接入该文 RAG 时的 CER：未知。13.66% 的行名不是这个检查点。
- Qwen3 的 1.7B、4B、14B、32B，以及 0.6B、8B 相对识别假设：未知。第 5 节没有识别假设 CER。
- Qwen3.5 及以后的文本检查点：未知。
- 点名仓库路径的标点 F1，以及点名 Qwen 文本检查点的口语清理 F1：未知。
- 上述任何一行在 Apple Silicon 或 MLX 上的复测：未知。本轮没有本地测量。

## 8. 推论（候选）

候选：在第 3 节那 1,000 条上，直接把 `Qwen2.5-3B-Instruct`（4-bit NF4，贪心，1-best）的输出当作替换，Macro CER 与 Corpus CER 都高于 Whisper 假设。这只描述这一名字、这一套量化和这一抽样。它不证明其他档也不该替换。

候选：同一测试上，联合门控把接受率压到 9.10% 之后，两项 CER 仍不低于识别假设。机制上，这与 `004` 的 `rewriteDecision` 同向：先判定，不接受就留下原文。它不是豆包的分数，也不把豆包的判定写成已经量过 CER。

候选：第 4 节表 III 里，论文命名的 72B 无领域微调行，以及 1.5B 微调且不接入 RAG 的行，CER 与 SER 低于该文最佳单路识别。提示不是第 3 节那种只看 1-best 的最小修改提示。检查点变体未写明。这两行不构成「72B 档已经应当替换」，也不取消第 3 节的上升。

候选：同一篇的 `GO-AEC w/o SFT` 行高于识别原文。方向依行而变。没有跨档的单一方向。

候选：Qwen3 0.6B 与 8B 的 AISHELL-1 格子因为缺少识别假设 CER，仍落在第 7 节的未知里。不把相对 DANCER 的 0.02 个点写成原假设已经被改好。

候选：在未知的档上，适配器仍可以输出「不替换」。恒等改写满足原语 5 的核对句。这是类型核对，不是性能排名，也不是某一档已经被否掉。

## 检索日志

| # | 动作 | 地址或查询 | 结果 |
| --- | --- | --- | --- |
| 1 | 本地阅读 | `020`、`019` 第 4 条 | 本篇只回答替换文本是否错误更少；没有分数则保持未知 |
| 2 | 本地阅读 | `004` 第 3.2 节，`002` 第 4.3 节 | 豆包在替换前有 `rewriteDecision`；空内容保留原文；宿主焦点校验不是 CER |
| 3 | 本地阅读 | `003` 篇首与 `W4A8ImeAsrQwen3Transformer` | 算子在识别解码侧。无文本改写 CER |
| 4 | 本地阅读 | `016` 第 5、7 节；`007` 原语 5；`018` 第 3 节 | ASR-EC 只约束 2023 年 7B；RAIR 无参数档；口语清理 F1 当时为空 |
| 5 | 网页检索 | `Qwen2.5-7B-Instruct` ASR error correction CER | 命中门控 PDF 摘要，以及 ASR-EC。7B Instruct 本身无新 CER 表 |
| 6 | 网页检索 | `Qwen3-4B` punctuation restoration F1 | 命中非 Qwen 的标点论文与 HKCanCor 旧文。无 Qwen3-4B 分数。空结果 |
| 7 | 网页检索 | `Qwen2.5-7B` disfluency removal F1 | 命中统计机器翻译年代的不流利去除，以及无关的消融权重。无 Qwen 文本 F1。空结果 |
| 8 | 网页检索 | `Qwen/Qwen2.5-7B` CER ASR correction benchmark | 命中的 “ASR” 是攻击成功率，不是语音识别。不采用 |
| 9 | 页面抓取 | 门控 PDF 的 HTTP 正文 | 返回 application/pdf，抓取器拒绝。空结果 |
| 10 | 网页检索 | `Qwen2.5-3B-Instruct` CER Common Voice | 模型卡与博客无这项 CER。分数只在随后打开的 PDF |
| 11 | 网页检索 | `Qwen/Qwen2.5-7B-Instruct` punctuation F1 HKCanCor | 命中仓库命令行。摘要无 F1。空结果 |
| 12 | 网页检索 | `Qwen2.5-14B` 或 `Qwen2.5-7B` disfluency detection F1 | 无点名检查点的不流利 F1。空结果 |
| 13 | 本地抽取 | `mutool` 抽取门控 PDF | 读到第 3 节两张 CER 表、NF4、1-best、Common Voice 26.0 抽样 |
| 14 | 页面阅读 | `https://arxiv.org/html/2509.23630v1` | 读到 Arena Breakout 表 III、表 IV，以及 72B / 1.5B 的提示差异 |
| 15 | 页面阅读 | `https://arxiv.org/html/2602.12287v1` | 读到 Qwen3 0.6B / 8B 的 AISHELL-1 CER。无识别假设行 |
| 16 | 页面阅读 | `https://arxiv.org/html/2501.06282v1` | 读到 SWAB 表 15 的 `Qwen2.5-7B`。表 16 标点行无 Qwen |
| 17 | 网页检索 | Selective Gating Macro CER 34.75 | 摘要与 PDF 一致。数字以 PDF 抽取为准 |
| 18 | 网页检索 | `Qwen2.5-7B-Instruct` 口语顺滑或 disfluency F1 | 仍无 F1 表。空结果 |
| 19 | 网页检索 | `Qwen2.5-7B-Instruct` punctuation restoration F1 Chinese ASR | 命中的是指令跟随 WER 与视觉模型 F1，不是文本标点恢复。不采用 |
| 20 | 网页检索 | arXiv 上 `Qwen3-4B` 或 `Qwen3-8B` 的文本纠错 CER | 命中意图准确率与命名实体纠正。意图准确率不转入。Qwen3 CER 见第 15 条 |

第 6、7、8、9、11、12、18、19 条没有新的「检查点 + 测试集 + 低于识别假设」分数。

<!-- research-complete -->
