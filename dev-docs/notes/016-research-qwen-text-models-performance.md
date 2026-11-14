# 016-research-qwen-text-models-performance：开源文本模型与 Qwen 各档性能

本文只回答一件事：能把一段已经识别出来的文本，在上下文上改写成另一段文本的开源模型有哪些。正文以 Qwen 系列为主，按可核对的代际与参数档列出权重名、磁盘体积、量化，以及 Apple Silicon 或 MLX 上已发表的内存与生成速度，并记录与标点、口语清理、纠错有关的已发表结果。

识别流、语音编码器和声学前端不在本文。MLX 的加载、量化工具和流式调用序列不在本文，那是 `017`。`014` 把本文的问题收成改写流一侧。`007` 原语 5 的输入是文本与上下文，输出是「不替换」或一段替换文本。下面的模型事实都对着这个输入输出形状，不把某一档写成已经接上端口的实现。

豆包输入法只作机制对照：`003` 记录样本里有算子名 `W4A8ImeAsrQwen3Transformer`，它位于识别解码一侧。本文讨论的是识别文本已经生成之后的文本模型，不把该算子的结构当成文本改写权重的规格。

检索日是 2026-11-12。Hugging Face `tree/main` 的字节数会随默认分支变动；下文的字节只对这一天读到的 `main` 成立。

## 1. 什么样的模型在机制上能做这件事

改写的输入是一段文本，加上会话当时的上下文。输出是另一段文本，或者调用方判定「不替换」。能做这件事的开源模型，机制上是解码器式因果语言模型（causal LM），并且带有指令模板：

1. 指令微调权重接受 `chat template`。调用方把上下文和识别文本放进消息，模型自回归生成新文本。生成结果可以与原文相同，也可以不同。模型自己不返回「不替换」这个枚举；枚举要由调用方比较原文与生成结果。这是接口形状，不是已经在输入法句子上测过的行为。
2. 同一家族的 Base 权重没有这个聊天口。它可以给编辑候选打语言模型分数，搜索出一个改正句。`simple-csc` 走的是这条路，见第 5.3 节。它不是「把提示词生成另一段话」。
3. 带思考模式的指令权重会先生成思考 token，再生成回答。若调用方把整段生成当作替换文本，思考内容会进入替换结果。Qwen3 博客给出的开关是 `enable_thinking`，以及用户文本里的 `/think`、`/no_think`（https://qwenlm.github.io/blog/qwen3 ）。思考 token 计入生成长度，因而计入延迟。本文没有读到在短句改写上测量这段开销的数字。

```python
# 来源: https://qwenlm.github.io/blog/qwen3 的 apply_chat_template 片段，已删去加载与解码
text = tokenizer.apply_chat_template(
    [{"role": "user", "content": prompt}],
    tokenize=False,
    add_generation_prompt=True,
    enable_thinking=False,
)
```

这段只说明官方示例能关掉思考开关。它不保证输出只改标点，也不保证输出等于原文。

因此，候选是：Qwen2.5 的 Instruct 权重、Qwen3 混合思考权重在 non-thinking 开关下、`Qwen3-4B-Instruct-2507` 这种模型卡写明不再生成 `<think>` 块的权重、以及 Qwen3.5 / 3.6 / 3.8 模型卡写明的 Instruct（non-thinking）模式，在输入类型上都落在改写流的文本口上。这不是「某一档已经够用」的测量结论。

## 2. 代际与权重名

下表只收本文打开过发布说明或模型卡的代际。参数量一列是发布说明里的名字，不是 safetensors 计数；两者的差别见第 3 节。

| 代际 | 发布说明中的日期与出处 | 本文核对过的文本口权重名 | 发布说明中的参数档 |
| --- | --- | --- | --- |
| Qwen2.5 | 2024-09-19，https://qwenlm.github.io/blog/qwen2.5 | `Qwen/Qwen2.5-{0.5,1.5,3,7,14,32,72}B-Instruct` | 密集 0.5B、1.5B、3B、7B、14B、32B、72B。博客写支持 128K 上下文、生成最长 8K，29 种以上语言 |
| Qwen3 | 2025-04-29，https://qwenlm.github.io/blog/qwen3 | `Qwen/Qwen3-0.6B`、`1.7B`、`4B`、`8B`、`14B`、`32B`、`30B-A3B`、`235B-A22B`；另有 `Qwen/Qwen3-4B-Instruct-2507`、`Qwen/Qwen3-30B-A3B-Instruct-2507` | 密集六档加 MoE 两档。混合思考。博客写 119 种语言与方言，预训练约 36T token |
| Qwen3-Next | 2025-09-11，https://github.com/QwenLM/Qwen3.8/blob/main/README.md 的 News | 新闻点名 `Qwen3-Next-80B-A3B` | 80B 总参数、3B 激活。本文没有读它的文件树，故无体积 |
| Qwen3.5 | 2026-02-16 起，同上 README | `Qwen/Qwen3.5-0.8B`、`2B`、`4B`、`9B`、`27B`、`35B-A3B`、`122B-A10B`、`397B-A17B` | 见第 3.3 节。模型卡类型是带视觉编码器的因果语言模型 |
| Qwen3.6 | 2026-04-16、2026-04-22，同上 README | `Qwen/Qwen3.6-35B-A3B`、`Qwen/Qwen3.6-27B` | 27B 密集；35B 总参数、3B 激活。同样带视觉编码器 |
| Qwen3.8 | 2026-08-12、2026-08-14，同上 README | `Qwen/Qwen3.8-27B`；新闻另有 `Qwen3.8-2.4T-A95B` | 27B 的体积已读。2.4T 的文件树本次没有读 |

Qwen3 博客的结构表（同一 URL）：

| 权重名 | 层数 | Q / KV heads | 词嵌入绑定 | 博客表内的上下文 |
| --- | --- | --- | --- | --- |
| Qwen3-0.6B | 28 | 16 / 8 | 是 | 32K |
| Qwen3-1.7B | 28 | 16 / 8 | 是 | 32K |
| Qwen3-4B | 36 | 32 / 8 | 是 | 32K |
| Qwen3-8B | 36 | 32 / 8 | 否 | 128K |
| Qwen3-14B | 40 | 40 / 8 | 否 | 128K |
| Qwen3-32B | 64 | 64 / 8 | 否 | 128K |
| Qwen3-30B-A3B | 48 | 32 / 4 | 专家 128 个中激活 8 个 | 128K |
| Qwen3-235B-A22B | 94 | 64 / 4 | 专家 128 个中激活 8 个 | 128K |

模型卡与这张表并不逐格相同，以打开过的卡片为准：

- `Qwen/Qwen3-0.6B` 的 README 写 Type 为 Causal Language Models，Number of Parameters 为 0.6B，Context Length 为 32,768。
- `Qwen/Qwen3-4B` 的页面写总参数 4.0B、非嵌入参数 3.6B，上下文原生 32,768，YaRN 下 131,072。
- `Qwen/Qwen3-8B` 的 README 写参数 8.2B，上下文原生 32,768，YaRN 下 131,072。博客表的「128K」没有区分原生长度与 YaRN。两处并列，不把其中一处改写成另一处。
- `Qwen/Qwen3-4B-Instruct-2507` 的 README 写参数 4.0B，上下文原生 262,144，并且写明只支持 non-thinking，输出中不生成 `<think>` 块，也不再要求传入 `enable_thinking=False`。权重名中的 2507 不是本文读到的日历日期；该 README 片段没有写发布日。
- `Qwen/Qwen2.5-7B-Instruct` 的 README 写参数 7.61B，上下文满 131,072，生成 8192 token。与 Qwen2.5 博客的系列描述一致。

Qwen3.5 及以后的模型卡把类型写成 Causal Language Model with Vision Encoder。`Qwen/Qwen3.5-4B` 与 `Qwen/Qwen3.5-35B-A3B` 写原生上下文 262,144，并可延伸到 1,010,000。`Qwen/Qwen3.5-0.8B` 写原生 262,144，并写明默认是 non-thinking。`Qwen/Qwen3.8-27B` 写参数 27B，原生 262,144，延伸上限在该卡上写成 1,000,000。这些检查点的 safetensors 是整包，包含视觉侧权重。本文不拆编码器结构。文本改写可以不喂图像，但磁盘体积不是「纯文本切片」的体积。

`mlx-community/Qwen3.5-4B-4bit` 的 `config.json` 把文本侧 `model_type` 写成 `qwen3_5_text`，`num_hidden_layers` 为 32，`hidden_size` 为 2560，`vocab_size` 为 248320，`max_position_embeddings` 为 262144，并列出 `linear_attention` 与 `full_attention` 交替。`vision_config.depth` 为 24。同文件 `language_model_only` 不在这份社区配置里；官方 `Qwen/Qwen3.6-27B` 与 `Qwen/Qwen3.8-27B` 的 `config.json` 写着 `"language_model_only": false`。

## 3. 磁盘体积与量化

体积是 Hugging Face `GET /api/models/{repo}/tree/main` 返回的 `*.safetensors` 大小之和，不含 `tokenizer.json` 与 README。GiB 按 1024³ 换算，保留三位小数。`safetensors.parameters` 来自同一天的 `GET /api/models/{repo}`，BF16 计数字节约等于参数个数乘 2，与文件大小差一个文件头。

### 3.1 Qwen2.5 Instruct，BF16

| 仓库 | API 的 BF16 参数个数 | safetensors 字节 | GiB |
| --- | --- | --- | --- |
| `Qwen/Qwen2.5-0.5B-Instruct` | 494,032,768 | 988,097,824 | 0.920 |
| `Qwen/Qwen2.5-1.5B-Instruct` | 1,543,714,304 | 3,087,467,144 | 2.876 |
| `Qwen/Qwen2.5-3B-Instruct` | 3,085,938,688 | 6,171,926,992 | 5.748 |
| `Qwen/Qwen2.5-7B-Instruct` | 7,615,616,512 | 15,231,271,888 | 14.185 |
| `Qwen/Qwen2.5-14B-Instruct` | 14,770,033,664 | 29,540,134,000 | 27.511 |
| `Qwen/Qwen2.5-32B-Instruct` | 32,763,876,352 | 65,527,841,856 | 61.027 |
| `Qwen/Qwen2.5-72B-Instruct` | 72,706,203,648 | 145,412,519,312 | 135.427 |

7.61B 与模型卡「7.61B」一致。0.5B 这个名字对应的计数是 0.494B，不是恰好 5×10⁸。

许可证只记录打开过的文件标题，不作可用性判断。Qwen2.5 博客写：除 3B 与 72B 外，开源权重为 Apache 2.0。本次打开的文件：`Qwen/Qwen2.5-7B-Instruct` 的 LICENSE 以 Apache License Version 2.0 开头；`Qwen/Qwen2.5-3B-Instruct` 的标题是 Qwen RESEARCH LICENSE AGREEMENT，日期写 September 19, 2024；`Qwen/Qwen2.5-72B-Instruct` 的标题是 Qwen LICENSE AGREEMENT，日期同样写 September 19, 2024。0.5B、1.5B、14B、32B 的 LICENSE 本次没有打开，不从博客句子外推成已逐份核对。

### 3.2 Qwen3，BF16 与 MLX 4-bit

BF16：

| 仓库 | API 的 BF16 参数个数 | safetensors 字节 | GiB |
| --- | --- | --- | --- |
| `Qwen/Qwen3-0.6B` | 751,632,384 | 1,503,300,328 | 1.400 |
| `Qwen/Qwen3-1.7B` | 2,031,739,904 | 4,063,515,592 | 3.784 |
| `Qwen/Qwen3-4B` | 4,022,468,096 | 8,044,982,000 | 7.492 |
| `Qwen/Qwen3-4B-Instruct-2507` | 4,022,468,096 | 8,044,982,000 | 7.492 |
| `Qwen/Qwen3-8B` | 8,190,735,360 | 16,381,516,776 | 15.256 |
| `Qwen/Qwen3-14B` | 14,768,307,200 | 29,536,665,640 | 27.508 |
| `Qwen/Qwen3-32B` | 32,762,123,264 | 65,524,328,560 | 61.024 |
| `Qwen/Qwen3-30B-A3B` | 30,532,122,624 | 61,066,575,648 | 56.872 |
| `Qwen/Qwen3-30B-A3B-Instruct-2507` | 30,532,122,624 | 61,066,575,656 | 56.872 |
| `Qwen/Qwen3-235B-A22B` | 本次 API 行未单列；文件树已读 | 470,191,875,096 | 437.888 |

`Qwen3-0.6B` 模型卡写 0.6B，safetensors 计数是 751,632,384。`Qwen3-8B` 模型卡写 8.2B，计数是 8,190,735,360。`Qwen3-4B` 模型卡写 4.0B 与非嵌入 3.6B，计数是 4,022,468,096。三处都是卡片口径与张量计数口径不同，本文不合并。

官方 MLX 4-bit 仓库（组织 `Qwen`，不是 `mlx-community`）：

| 仓库 | safetensors 字节 | GiB | 已打开的量化字段 |
| --- | --- | --- | --- |
| `Qwen/Qwen3-0.6B-MLX-4bit` | 316,825,742 | 0.295 | 未打开 config |
| `Qwen/Qwen3-1.7B-MLX-4bit` | 914,316,100 | 0.852 | 未打开 config |
| `Qwen/Qwen3-4B-MLX-4bit` | 2,137,326,367 | 1.990 | `bits` 4，`group_size` 128 |
| `Qwen/Qwen3-8B-MLX-4bit` | 4,351,884,216 | 4.053 | 未打开 config |
| `Qwen/Qwen3-14B-MLX-4bit` | 7,846,402,136 | 7.307 | 未打开 config |
| `Qwen/Qwen3-32B-MLX-4bit` | 17,406,055,438 | 16.211 | 未打开 config |
| `Qwen/Qwen3-30B-A3B-MLX-4bit` | 16,220,657,512 | 15.106 | 未打开 config |

`Qwen/Qwen3-4B-MLX-4bit` 的 `config.json` 同时写 `architectures` 为 `Qwen3ForCausalLM`，`hidden_size` 2560，`num_hidden_layers` 36，`vocab_size` 151936，`tie_word_embeddings` true，`max_position_embeddings` 65536。模型卡写的原生上下文是 32,768。65536 是这份转换配置里的字段，不是模型卡原文。

`mlx-community` 的同名 4-bit 体积不同，不能和上表当成同一份量化：

| 仓库 | safetensors 字节 | GiB | 已打开的量化字段 |
| --- | --- | --- | --- |
| `mlx-community/Qwen3-0.6B-4bit` | 335,450,584 | 0.312 | 未打开 |
| `mlx-community/Qwen3-1.7B-4bit` | 968,080,210 | 0.902 | 未打开 |
| `mlx-community/Qwen3-4B-4bit` | 2,263,022,529 | 2.108 | `bits` 4，`group_size` 64 |
| `mlx-community/Qwen3-8B-4bit` | 4,607,835,174 | 4.291 | 未打开 |
| `mlx-community/Qwen3-14B-4bit` | 8,307,898,514 | 7.737 | 未打开 |
| `mlx-community/Qwen3-30B-A3B-4bit` | 17,174,780,668 | 15.995 | 未打开 |
| `mlx-community/Qwen3-4B-Instruct-2507-4bit` | 与 4B 社区包同量级；API 曾返回参数字典，文件树本次未单独加总 | — | 未打开 |

社区 `Qwen3-4B-4bit` 的 `max_position_embeddings` 是 40960，又与官方 MLX 配置的 65536 不同。4-bit 这个名字底下至少有 group size 64 与 128 两套。下文凡是论文只写 “4-bit for MLX” 而没有仓库名的速度，都不能指回其中一套。

Qwen2.5 的 MLX 4-bit 本次只读了 `mlx-community` 文件树，没有打开它们的 `config.json`，所以只有仓库名上的 4bit，没有 group size：

| 仓库 | safetensors 字节 | GiB |
| --- | --- | --- |
| `mlx-community/Qwen2.5-0.5B-Instruct-4bit` | 278,064,920 | 0.259 |
| `mlx-community/Qwen2.5-1.5B-Instruct-4bit` | 868,628,559 | 0.809 |
| `mlx-community/Qwen2.5-3B-Instruct-4bit` | 1,736,293,090 | 1.617 |
| `mlx-community/Qwen2.5-7B-Instruct-4bit` | 4,284,346,255 | 3.990 |
| `mlx-community/Qwen2.5-14B-Instruct-4bit` | 8,309,494,233 | 7.739 |
| `mlx-community/Qwen2.5-32B-Instruct-4bit` | 18,431,478,459 | 17.166 |

Qwen3 博客写列出的密集与 MoE 权重为 Apache 2.0。本次打开 LICENSE 且以 Apache License Version 2.0 开头的，只有 `Qwen/Qwen3-4B`。其余 Qwen3 档的许可证文件没有打开。

### 3.3 Qwen3.5、Qwen3.6、Qwen3.8

BF16 整包，含视觉侧：

| 仓库 | 模型卡或 README 中的参数 | safetensors 字节 | GiB |
| --- | --- | --- | --- |
| `Qwen/Qwen3.5-0.8B` | 0.8B | 1,746,942,600 | 1.627 |
| `Qwen/Qwen3.5-2B` | 文件树已读；参数句子本次未从该卡摘出 | 4,548,221,488 | 4.236 |
| `Qwen/Qwen3.5-4B` | 4B | 9,319,828,096 | 8.680 |
| `Qwen/Qwen3.5-9B` | 文件树已读；参数句子本次未从该卡摘出 | 19,306,310,880 | 17.980 |
| `Qwen/Qwen3.5-27B` | 文件树已读 | 55,563,022,432 | 51.746 |
| `Qwen/Qwen3.5-35B-A3B` | 总共 35B，激活 3B | 71,903,878,016 | 66.965 |
| `Qwen/Qwen3.5-122B-A10B` | 名字即 122B 总参数、10B 激活 | 250,173,250,864 | 232.995 |
| `Qwen/Qwen3.5-397B-A17B` | 名字即 397B 总参数、17B 激活 | 806,796,241,352 | 751.378 |
| `Qwen/Qwen3.6-27B` | 27B | 55,563,006,400 | 51.746 |
| `Qwen/Qwen3.6-35B-A3B` | 名字即 35B 总参数、3B 激活 | 71,903,776,776 | 66.965 |
| `Qwen/Qwen3.8-27B` | 27B | 55,563,006,776 | 51.746 |

API 还给出少量 F32 张量：`Qwen3.5-4B` 为 BF16 4,659,861,248 加 F32 3,840。F32 个数相对 BF16 可以忽略，不单独换算成体积。

`mlx-community` 4-bit。只打开了 `Qwen3.5-4B-4bit` 的 config：`bits` 4，`group_size` 64，`mode` 为 `affine`。其余档只读了文件大小：

| 仓库 | safetensors 字节 | GiB |
| --- | --- | --- |
| `mlx-community/Qwen3.5-0.8B-4bit` | 625,229,487 | 0.582 |
| `mlx-community/Qwen3.5-2B-4bit` | 1,722,271,785 | 1.604 |
| `mlx-community/Qwen3.5-4B-4bit` | 3,034,300,695 | 2.826 |
| `mlx-community/Qwen3.5-9B-4bit` | 5,950,221,072 | 5.541 |
| `mlx-community/Qwen3.5-27B-4bit` | 16,054,546,159 | 14.952 |
| `mlx-community/Qwen3.5-35B-A3B-4bit` | 20,391,679,439 | 18.991 |

4-bit 之后 4B 仍有 2.826 GiB，大于纯文本 `Qwen3-4B` 官方 MLX 4-bit 的 1.990 GiB。候选解释是视觉侧仍有较高精度张量；本文没有做逐张量分解，所以这只是候选，不是已分开的体积。

本次打开且以 Apache License Version 2.0 开头的 LICENSE：`Qwen/Qwen3.5-4B`、`Qwen/Qwen3.6-27B`、`Qwen/Qwen3.8-27B`。GitHub 仓库 `QwenLM/Qwen3.8` 的页面也标 Apache-2.0，那是代码仓库标记，不等于每一份权重文件都已打开。

官方路径 `Qwen/Qwen3.5-*-MLX-*` 本次没有请求。没有读到，就不写成「不存在」。

## 4. Apple Silicon 与 MLX 上已发表的速度和内存

三组数字的提示长度、量化、硬件都不相同。下面分成三张表，不排成一张无注的速度榜。

### 4.1 M4 Max，4-bit，吞吐表

出处：Wayner Barrios, *Native LLM and MLLM Inference at Scale on Apple Silicon*, arXiv:2601.19139v2，https://arxiv.org/html/2601.19139v2 ，Table 1。硬件是 Apple M4 Max、128GB 统一内存。GGUF 侧为 Q4_K_M，MLX 侧论文只写 4-bit，没有仓库名，也没有 group size。表题没有给出提示长度和生成长度。单位是 tok/s。

| 论文中的模型名 | vllm-mlx | vllm-metal | mlx-lm | llama.cpp |
| --- | --- | --- | --- | --- |
| Qwen3-0.6B | 525.5 | 365.8 | 356.2 | 281.5 |
| Qwen3-4B | 159.0 | 137.3 | 128.9 | 118.2 |
| Qwen3-8B | 93.3 | 87.1 | 79.9 | 76.9 |
| Qwen3-30B-A3B | 109.7 | 110.3 | 107.4 | 89.9 |
| Llama-3.2-1B | 461.9 | 350.9 | 347.1 | 331.3 |
| Llama-3.2-3B | 203.6 | 174.3 | 167.5 | 155.8 |
| Gemma 3-4B | 152.5 | 117.0 | 105.4 | 123.2 |
| Nemotron-30B-A3B | 121.8 | 论文写无此项 | 101.6 | 85.1 |

同一篇论文的并发段又写：vllm-mlx 上 Qwen3-0.6B 从单请求 441 tok/s 到 16 并发 1642 tok/s。441 与 Table 1 的 525.5 不是同一个测量，本文不取平均。Table 7 是另一项：Qwen3-4B、512 token 共享前缀，无缓存 TTFT 245 ms，前缀命中 42 ms。这是首 token 时间，不是解码 tok/s。

这篇论文的正文没有给出上表文本模型的峰值内存。视频表里的内存属于视觉模型，不抄进文本模型表。

### 4.2 Mac Studio，Qwen3.8-27B，BF16

出处：Apple Machine Learning Research，*Local and Distributed Large LLM Inference with MLX and RDMA on Mac Studio*，2026-10-15，https://machinelearning.apple.com/research/mlx-distributed-inference 。模型是 `Qwen/Qwen3.8-27B`，原生精度 BF16，评测精度也是 BF16。命令形式是 `mlx_lm.benchmark --model Qwen/Qwen3.8-27B -p 8192 -g 128 -b 1`。解码速度按上下文之后的 128 个生成 token 计算。机器是 Mac Studio，单机内存叙述为 512GB。M5 Ultra 带宽文中写 1.2 TB/s，M3 Ultra 写 819 GB/s。

解码 tok/s（Table 3）：

| 上下文 | 单台 M3 Ultra | 单台 M5 Ultra | 两台 M5 流水线 | 两台 M5 张量并行 | 四台 M5 张量并行 |
| --- | --- | --- | --- | --- | --- |
| 8,192 | 12.46 | 19.69 | 19.44 | 33.27 | 50.89 |
| 65,536 | 11.54 | 17.87 | 18.04 | 30.84 | 47.45 |

TTFT，单位秒（Table 2）：

| 上下文 | 单台 M3 Ultra | 单台 M5 Ultra | 两台 M5 流水线 | 两台 M5 张量并行 | 四台 M5 张量并行 |
| --- | --- | --- | --- | --- | --- |
| 8,192 | 18.2 | 4.1 | 4.1 | 3.1 | 1.7 |
| 65,536 | 199.4 | 43.7 | 24.5 | 29.4 | 15.6 |

内存：文中写 Qwen 3.8 27B 与 Mistral Medium 3.5 128B 在 65,536 上下文下分别占用至多 68GB 与 158GB RAM。68GB 是这一配置下的占用上限叙述，不是 4-bit 小模型的占用，也不能当成 8,192 上下文的数字。

这组速度比第 4.1 节低一个数量级，因为精度是 BF16、模型是 27B、上下文是 8K 或 65K，而且解码只统计 128 token。不能与 Qwen3-4B 的 128.9 tok/s 比高低。

### 4.3 另一则单次测量

https://github.com/waybarrios/vllm-mlx/discussions/637 写的是 MacBook Pro M5 Max 128GB、`mlx-community/Qwen3-8B-4bit`、mlx-lm 投机解码 `num_draft_tokens=2`。同一组约 3.6K 提示 token、约 2K 完成 token 的提示上，投机解码一侧 p50 TTFT 1.55 s，prefill 2368 tok/s，每请求解码 107.6 tok/s；服务进程 RSS 峰值约 5.4 GB，swap 为 0。这是讨论帖里的一次对照，不是第 4.1 节那篇论文的表，也没有覆盖思考模式开关。5.4 GB 是进程 RSS，不是权重文件大小；该仓库权重文件是 4.291 GiB。

本次没有读到与第 4.1 节同一硬件、同一量化写法的 Qwen2.5 或 Qwen3.5 小模型 tok/s。没有来源的速度不写入。

## 5. 标点、口语清理、纠错

三组实验的测试集、指标和模型档都不同。数字只留在各自的表里。

### 5.1 粤语标点：HKCanCor

King Yiu Suen, Rudolf Chow, Albert Y. S. Lam, *Cantonese Punctuation Restoration using LLM Annotated Data*, Interspeech 2025，https://www.isca-archive.org/interspeech_2025/suen25_interspeech.pdf ，Table 1。测试集是 Hong Kong Cantonese Corpus（HKCanCor）。Overall 是全部分标点类的 micro-average。论文写 Qwen-2.5-72b 在逗号、句号、问号三项的 F1 都是该表最高，并随后用它给粤语 YouTube 转写打标点。论文没有写量化、硬件或生成速度。这不是普通话输入法测试集。

P / R / F1：

| 模型名（论文原文） | 逗号 | 句号 | 问号 | Overall |
| --- | --- | --- | --- | --- |
| GPT-3.5-turbo | 44.0 / 63.0 / 51.9 | 90.6 / 89.4 / 90.0 | 70.6 / 59.3 / 64.4 | 65.5 / 75.5 / 70.1 |
| GPT-4o-mini | 42.7 / 80.4 / 55.8 | 93.2 / 85.0 / 88.9 | 57.6 / 74.0 / 64.8 | 60.9 / 81.9 / 69.9 |
| Llama-3.3-70b | 39.4 / 74.3 / 51.5 | 96.1 / 70.0 / 81.0 | 41.6 / 86.2 / 56.1 | 54.7 / 73.5 / 62.7 |
| Qwen-2.5-72b | 46.3 / 72.7 / 56.6 | 93.8 / 91.4 / 92.6 | 72.8 / 71.7 / 72.3 | 67.1 / 81.6 / 73.6 |
| Yi-1.5-34B | 43.8 / 38.0 / 40.7 | 93.7 / 52.2 / 67.1 | 35.5 / 80.7 / 49.3 | 56.9 / 49.5 / 52.9 |

论文没有把 Qwen-2.5-72b 对应到 `Qwen/Qwen2.5-72B-Instruct` 或 Base。名字停在「Qwen-2.5-72b」。

### 5.2 中文 ASR 纠错：提示使 CER 上升

Victor Junqiu Wei 等，*ASR-EC Benchmark: Evaluating Large Language Models on Chinese ASR Error Correction*，EMNLP 2025 Industry，https://aclanthology.org/2025.emnlp-industry.110.pdf ，Table 4。模型是 Bai 等 2023 的 Qwen，参数量 7B，文本口。不是 Qwen2.5，也不是 Qwen3。指标是 CER，越低越好。列顺序是 ASR-EC A 的短句、长句、混合，然后 ASR-EC B 的短句、长句、混合。

| 设置 | Qwen 的六列 CER | 同列 ASR 基线 |
| --- | --- | --- |
| ASR 基线 | — | 12.83，11.73，12.42，13.44，6.92，8.11 |
| Zero-Shot | 27.21，25.58，25.84，26.04，21.30，22.15 | 同上 |
| Three-Shot | 21.36，24.30，23.74，20.44，20.59，20.56 | 同上 |
| Multi-step | 26.73，27.26，27.12，25.88，22.68，23.27 | 同上 |
| LoRA 微调 | 13.45，14.16，14.59，12.58，7.08，8.48 | 同上 |

零样本、三样本和多步提示下，Qwen-7B 的 CER 都高于 ASR 基线。论文写：提示式纠错会去改本已正确的句子，从而引入新错误。LoRA 文本微调后，ASR-EC A 三列仍高于基线；ASR-EC B 只有短句 12.58 低于基线 13.44，长句与混合仍高于基线。

同表还有 Multimodal 行，Qwen 六列为 9.01、5.64、6.07、9.96、5.22、6.16，其中 A 混合 6.07 低于基线 12.42。该行使用音频。本文不写编码器，也不把这一行当成文本改写口的结果。

这条已发表结果直接约束第 1 节：指令口在机制上能生成另一段文本，不等于生成结果的 CER 低于识别原文。

### 5.3 拼写与 ASR N-best：Qwen2.5 的零样本

Junhong Liang, Yu Zhou, *RAIR: Retrieval-Augmented Iterative Refinement for Chinese Spelling Correction*, arXiv:2504.18938v2，https://arxiv.org/html/2504.18938v2 。正文把基线写成 “Qwen2.5”，引用 Team 等 2025，没有写 7B、14B 或 72B。下表的分数不能安到某一档上。

Aishell-1 上相对 ASR Top-1 的 CER（Table 3）。CERR 的百分数与相对 Top-1 的变化一致：Top-1 CER 5.84，Qwen2.5 直接纠正 5.17，相对下降 11.5%。负的 CERR 表示 CER 下降。

| 方法 | CER | 相对 Top-1 的 CERR |
| --- | --- | --- |
| Top-1（ASR） | 5.84 | — |
| MacBERT | 5.06 | -13.4 |
| Qwen2.5 直接纠正 | 5.17 | -11.5 |
| Qwen2.5 + RAIR | 4.37 | -25.2 |
| GPT-3.5 直接纠正 | 9.84 | +68.5 |
| DeepSeek-V3 + RAIR | 4.15 | -28.9 |

直接纠正时，Qwen2.5 的 CER 低于 GPT-3.5，也低于 Top-1，但高于 MacBERT 的 5.06。加上论文自己的检索迭代之后降到 4.37。Table 5 的消融用同一组 5.17 与 4.37，数据集写的是 ChineseHP/Aishell-1。

ECSpell 与 LEMON 的句子级 F1（Table 2）。列是 law、med、odw、car、cot、enc、gam、mec、new、nov。零样本，不是微调。

| 方法 | 十列 F1 |
| --- | --- |
| Qwen2.5 | 46.9，23.5，54.4，26.6，33.5，34.7，20.8，24.3，34.6，24.2 |
| Qwen2.5 + RAIR | 68.0，40.5，73.0，38.8，50.1，49.4，27.1，47.9，45.2，34.1 |
| DeepSeek-V3 | 82.7，67.6，90.2，56.2，65.6，60.6，40.9，68.7，68.8，57.7 |

同一张表里 DeepSeek-V3 每一列都高于 Qwen2.5。Qwen2.5 对 RAIR 的抬升大于 GPT-3.5 的抬升；论文把这写成观察，不是参数档结论。

字符拆分数据集 CSEC 的 P / R / F1（Table 4）：Qwen2.5 在 News 上是 33.7 / 35.0 / 34.3，在 Social 上是 12.6 / 14.8 / 13.6。BERT 在 News 上的 F1 是 73.1。零样本 Qwen2.5 在这个等长拆分任务上远低于 BERT。

另一条机制，不是聊天改写：https://github.com/Jacob-Zhou/simple-csc 实现两篇训练无关论文，arXiv:2410.04027 与 arXiv:2502.15266。README 写 Base 权重大于 Instruct 或 Chat；v2.0.0 推荐 `Qwen/Qwen2.5-7B` 与 `Qwen/Qwen2.5-14B` 作为速度与效果的折中，示例代码加载的是 `Qwen/Qwen2.5-1.5B`。纠正器用语言模型分数和束搜索做增删改，不是 `apply_chat_template` 生成。README 还写，不用 flash-attn 时 Qwen2 与 Qwen2.5 的纠正器可能停在束搜索里。本文没有读那两篇论文的结果表，所以不转写它们的 F1。README 提到的比赛名次属于整套系统，不记成某一档 Qwen 的分数。

### 5.4 口语清理

检索「Qwen 文本模型、口语顺滑、填充词、disfluency、已发表 F1」时，命中的是语音模型论文和更早的统计机器翻译论文，不是 Qwen 文本权重的分数表。

*FormalASR*（arXiv:2605.19266）写明用 DeepSeek-V3.2 把逐字稿改写成书面汉语，步骤包括去掉填充词和不流利、整理标点、改正明显识别错误。随后微调的是 Qwen3-ASR 0.6B 与 1.7B。那是语音模型，本文不展开。可以核对的文本侧事实只有一句：该文的改写器是 DeepSeek-V3.2 的提示，不是一份已点名的 Qwen 文本检查点。

因此，Qwen 文本档上「口语清理」的已发表 F1，本次检索为空。不把粤语标点 F1 或 Aishell CER 改写成口语清理分数。

## 6. 其他家族对照

只保留与第 4.1 节同一篇、同一硬件、同一种「4-bit」写法的模型。论文写的名字没有 Instruct 或 Base。这些名字能否担任改写流，取决于具体权重是不是指令微调；本文没有打开 Llama、Gemma、Nemotron 的模型卡，不补参数字节，也不补许可证。

| 论文中的名字 | 参数档（论文用语） | mlx-lm tok/s | vllm-mlx tok/s | 与 Qwen 同行的关系 |
| --- | --- | --- | --- | --- |
| Llama-3.2-1B | 1B | 347.1 | 461.9 | 同列相比：mlx-lm 347.1 低于 Qwen3-0.6B 的 356.2；vllm-mlx 461.9 低于 525.5 |
| Llama-3.2-3B | 3B | 167.5 | 203.6 | 同列相比：mlx-lm 167.5 高于 Qwen3-4B 的 128.9；vllm-mlx 203.6 高于 159.0 |
| Gemma 3-4B | 4B | 105.4 | 152.5 | 同列相比：mlx-lm 105.4 低于 Qwen3-4B 的 128.9；vllm-mlx 152.5 低于 159.0；llama.cpp 列 Gemma 123.2 高于 Qwen3-4B 的 118.2 |
| Nemotron-30B-A3B | 30B 总参数、3B 激活 | 101.6 | 121.8 | 与 Qwen3-30B-A3B 激活规模同级。同列：mlx-lm 101.6 低于 107.4；vllm-mlx 121.8 高于 109.7 |

这张表的口径就是第 4.1 节，不能拿去比第 5 节的 F1 或 CER。

## 7. 边界与仍未核对的点

1. 指令口能生成另一段文本，是机制。Qwen-7B（2023）在 ASR-EC 上的提示纠正提高了 CER。这个反例已经发表。候选只到「文本口的形状符合原语 5」，不到「生成文本应该替换识别文本」。
2. RAIR 的 “Qwen2.5” 没有参数档。不能把 Aishell-1 的 CER 5.17 安到 1.5B 或 7B 上。
3. 粤语标点最高的是论文中的 Qwen-2.5-72b。72B 的 BF16 文件是 135.427 GiB。本文没有读到这一档在 Apple Silicon 上的 tok/s。
4. 思考模式的额外 token 没有短句测量。`Qwen3-4B-Instruct-2507` 的模型卡写明不生成思考块；原版 `Qwen3-4B` 的默认示例是 `enable_thinking=True`。两者磁盘同为 7.492 GiB，行为开关不同。
5. Qwen3.5 及以后的 safetensors 含视觉权重，`language_model_only` 在已打开的 3.6 与 3.8 配置里为 false。4-bit 社区包大于同名纯文本 Qwen3 包。视觉张量占多少，本次没有逐张量相加。
6. 4-bit 至少有 group size 64 与 128。速度论文没有仓库名时，不能对应到第 3 节某一行的字节数。
7. 第 4.1 节没有提示长度。第 4.2 节的提示长度是 8192 或 65536，生成 128 token，精度是 BF16。两组 tok/s 不能相除。
8. Qwen3-Next-80B-A3B 与 Qwen3.8-2.4T-A95B 只在发布新闻里出现。文件树未读，没有体积，也没有 Apple Silicon 速度。
9. `simple-csc` 两篇论文的分数表未读。口语清理的 Qwen 文本 F1 未检索到。
10. 官方 Qwen3.5 的 MLX 仓库路径本次没有请求。`mlx-community` 的存在不说明官方仓库是否另有一份。

## 检索日志

| # | 方式 | 目标 | 结果 |
| --- | --- | --- | --- |
| 1 | 网页检索 | Qwen3 各档参数与层数 | 命中 https://qwenlm.github.io/blog/qwen3 与 arXiv:2505.09388 的结构表 |
| 2 | 网页检索 | MLX、Apple Silicon、Qwen tok/s | 命中 arXiv:2601.19139 与 Apple MLX 博文线索 |
| 3 | 网页检索 | Qwen 标点恢复、ASR 纠错 | 命中 Interspeech 2025 粤语标点与 ASR-EC；口语顺滑没有 Qwen 文本 F1 |
| 4 | 网页检索 | Qwen2.5 档位与磁盘 | 命中 2024-09-19 博客；Ollama 体积未采用，因量化口径与 safetensors 不同 |
| 5 | 网页检索 | Qwen3.5 发布档位 | 命中 GitHub `QwenLM/Qwen3.8` 的日期与档位 |
| 6 | 网页检索 | `mlx-community/Qwen3-4B-4bit` 文件大小 | 命中模型页，`model.safetensors` 约 2.26 GB；后以 tree API 精确字节为准 |
| 7 | 网页检索 | 中文拼写纠正与 Qwen2.5 | 命中 RAIR 与 `simple-csc` |
| 8 | 网页检索 | Qwen2.5 Instruct 模型卡参数句 | 命中 HF 模型卡，具体句子改由 raw README 读取 |
| 9 | 页面读取 | https://qwenlm.github.io/blog/qwen3 | 读到 Apache 2.0、结构表、`enable_thinking` |
| 10 | 页面读取 | https://qwenlm.github.io/blog/qwen2.5 | 读到七档、3B/72B 许可证例外、128K 与 8K 生成 |
| 11 | 网页检索 | `qwen.ai` 上的 Qwen3.5 博文 URL | 得到 `https://qwen.ai/blog?id=qwen3.5` |
| 12 | 页面读取 | https://qwen.ai/blog?id=qwen3.5 | 空。页面无正文，疑似脚本渲染，未当作数据源 |
| 13 | 页面读取 | https://huggingface.co/Qwen/Qwen3-4B | 读到 4.0B、非嵌入 3.6B、32,768 与 YaRN 131,072 |
| 14 | 页面读取 | https://arxiv.org/html/2601.19139v2 | 读到 Table 1 与前缀缓存 245 ms / 42 ms；文本模型无峰值内存 |
| 15 | HF API | `/api/models/{repo}` 的 `safetensors.parameters` | 读到 Qwen2.5、Qwen3、Qwen3.5、Qwen3.6、Qwen3.8 与 MLX 仓库的参数计数字典。`usedStorage` 与权重文件不一致，弃用 |
| 16 | HF API | `/api/models/{repo}/tree/main` | 读到第 3 节全部 safetensors 字节。Qwen3-Next 与 Qwen3.8-2.4T 不在这批路径里 |
| 17 | 原始文件 | 若干 `LICENSE` 与 `config.json` | 读到第 3 节写明的许可证标题、group size 64/128、`language_model_only: false` |
| 18 | 页面读取 | https://arxiv.org/html/2504.18938v2 开头 | 确认任务含 ASR N-best 与等长拼写；分数表改由本地 HTML 抽取 |
| 19 | 页面读取 | https://machinelearning.apple.com/research/mlx-distributed-inference | 读到 BF16、`mlx_lm.benchmark` 命令、68GB 叙述；表内数字在页面脚本中 |
| 20 | 文件下载 | RAIR HTML、EMNLP 2025 industry PDF、Interspeech 2025 PDF | 三份都非空。PDF 用内容流抽取，不经转写 |
| 21 | 本地解析 | Apple 博文 Table 2 与 Table 3 的两档上下文 | 得到第 4.2 节的 TTFT 与 tok/s |
| 22 | 原始 README | Qwen3-0.6B、8B、4B-Instruct-2507、Qwen3.5-0.8B、4B、35B-A3B、Qwen3.6-27B、Qwen3.8-27B、Qwen2.5-7B-Instruct | 读到参数句、上下文、non-thinking 默认值 |
| 23 | 页面读取 | https://raw.githubusercontent.com/Jacob-Zhou/simple-csc/main/README.md | 读到 Base 优于 Instruct 的仓库说明，以及推荐的 Qwen2.5-7B/14B。论文分数表未读 |
| 24 | 页面读取 | https://raw.githubusercontent.com/QwenLM/Qwen3.8/main/README.md | 读到 2026-02-16 至 2026-08-14 的发布新闻，以及 2025-09-11 的 Qwen3-Next |
| 25 | 本地解析 | RAIR HTML 的 Table 2–4，标点 PDF 的 Table 1，ASR-EC PDF 的 Table 4 | 得到第 5 节全部分数。口语清理查询没有对应的 Qwen 文本分数表，记为空结果 |

<!-- research-complete -->
