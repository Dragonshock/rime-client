# 017-research-mlx-qwen-integration：MLX 与 MLX-Swift 加载并流式运行模型

本文件只回答 014 分给 017 的问题：MLX、mlx-lm、mlx-swift-lm 如何加载 Qwen 权重、如何量化、如何逐段取出 token，以及这些数组和线程落在哪。015 负责识别流模型名单，本文件不改写那份名单，也不把语音权重说成已经走通下面的文本调用序列。

源码快照（`main`，本次读取）：

| 仓库 | commit | 日期 |
|---|---|---|
| [ml-explore/mlx](https://github.com/ml-explore/mlx) | `fc344c986c41` | 2026-11-06 |
| [ml-explore/mlx-lm](https://github.com/ml-explore/mlx-lm) | `56230fa02b8f` | 2026-11-05 |
| [ml-explore/mlx-swift](https://github.com/ml-explore/mlx-swift) | `a557dd6024af` | 2026-11-09 |
| [ml-explore/mlx-swift-lm](https://github.com/ml-explore/mlx-swift-lm) | `635290754327` | 2026-11-11 |

文档站页面标题是 MLX 0.32.4。四个仓库根目录 `LICENSE` 的第一行都是 `MIT License`。mlx 与 mlx-lm 的版权行是 `Copyright © 2023 Apple Inc.`；mlx-swift 是 `Copyright (c) 2023 ml-explore`；mlx-swift-lm 是 `Copyright (c) 2024 ml-explore`。

本次没有调用 REA。加载、量化和流式生成的调用序列在上述公开源码里。

## 三层各自做什么

MLX 是数组运行时。统一内存文档写明：Apple silicon 上 CPU 与 GPU 直接访问同一内存池；建数组时不指定位置；指定的是运算所在的 stream，而不是把数组搬到另一块内存。依赖由调度器插入。来源：[Unified Memory](https://ml-explore.github.io/mlx/build/html/usage/unified_memory.html)。运算默认跑在 `mx.default_stream(mx.default_device())`；`stream=` 也可以是 `Device`。来源：[Using Streams](https://ml-explore.github.io/mlx/build/html/usage/using_streams.html)。求值是惰性的，`mx.eval` 或读取标量才会把图跑完。来源：[Lazy Evaluation](https://ml-explore.github.io/mlx/build/html/usage/lazy_evaluation.html)。

mlx-lm 是 Python 包，入口是 `load`、`generate`、`stream_generate`、`convert`。来源：[mlx-lm README](https://github.com/ml-explore/mlx-lm/blob/56230fa02b8f/README.md) 与 `mlx_lm/__init__.py`。它按 `config.json` 的 `model_type` 选 `mlx_lm.models.<model_type>` 里的 `Model` / `ModelArgs`。

mlx-swift 提供 Swift 侧的 `MLXArray`、`Stream`、`asyncEval`（`import MLX`）。mlx-swift-lm 在其上放模型工厂和生成循环。文本模型工厂是 `LLMModelFactory`，容器是 `ModelContainer`，生成事件是 `Generation`。视觉模型是另一套 `VLMModelFactory`，不在文本工厂的类型表里。

`mlx_lm.utils.can_run_metal()` 的条件是默认设备为 `mx.gpu` 且 `mx.metal.is_available()`。来源：`mlx_lm/utils.py`。

## Python：权重如何被选中并加载

`load(path_or_hf_repo)` 先 `_download`，再 `load_model`，再 `load_tokenizer`。`lazy=False` 时 `load_model` 末尾执行 `mx.eval(model.parameters())`，参数在返回前落到内存里；`convert` 调用 `load(..., lazy=True)`，量化前不强制整模求值。

`load_model` 的顺序：

1. `load_config` 读 `config.json`。
2. 收集 `model*.safetensors`。没有且 `strict=True` 时抛 `FileNotFoundError`。
3. 每个文件 `mx.load`，合并成权重字典。文档站的惰性说明与 Swift 加载注释一致：safetensors 在求值前可以只是图上的权重引用。
4. `_get_classes`：`model_type = MODEL_REMAPPING.get(model_type, model_type)`，然后 `importlib.import_module(f"mlx_lm.models.{model_type}")`，取 `arch.Model` 与 `arch.ModelArgs`。导入失败变成 `ValueError: Model type {model_type} not supported.`
5. `Model(ModelArgs.from_dict(config))`。若模型有 `sanitize(weights)`，先改键名再装权重。
6. 若 config 里已有量化描述，先 `nn.quantize` 把对应层换成量化层，再 `load_weights`。见下一节。
7. `model.eval()`，非 lazy 时 `mx.eval(model.parameters())`。

```python
# 来源: mlx-lm mlx_lm/utils.py _get_classes，commit 56230fa02b8f
model_type = MODEL_REMAPPING.get(model_type, model_type)
arch = importlib.import_module(f"mlx_lm.models.{model_type}")
return arch.Model, arch.ModelArgs
```

Qwen 与这次读到的模块对应关系如下。`MODEL_REMAPPING` 里和 Qwen 有关的一条是 `"qwen2_5_vl": "qwen2_vl"`。没有把 `qwen3_asr`、`qwen3_8` 或 `whisper` 映到现有模块。

| `model_type` | 本次 `mlx_lm/models/` 文件 | 前向吃什么 |
|---|---|---|
| `qwen2` | `qwen2.py` 的 `Model` | `inputs` token，或 `input_embeddings` |
| `qwen3` | `qwen3.py` 的 `Model` | 同上 |
| `qwen3_moe` | `qwen3_moe.py` | 文本模块，本次未逐行读前向 |
| `qwen3_next` | `qwen3_next.py` | 文本模块，本次未逐行读前向 |
| `qwen3_5` / `qwen3_5_moe` | `qwen3_5.py` 的 `Model` | 只把 token 或 `input_embeddings` 交给 `language_model` |
| `qwen2_vl` / `qwen3_vl` 等 | 同名 `*_vl.py` | 视觉语言模型文件，不是下面的 ASR 入口 |

`qwen2.py` 的 `Model.__call__(inputs, cache=None, input_embeddings=None)` 走 `Qwen2Model`：没有 `input_embeddings` 时 `embed_tokens(inputs)`，每层把 cache 传给 attention，最后用 `lm_head` 或绑权的 `embed_tokens.as_linear` 得到 logits。`sanitize` 在 `tie_word_embeddings` 时丢掉 `lm_head.weight`，并丢掉键名里含 `self_attn.rotary_emb.inv_freq` 的张量。

`qwen3_5.py` 的 `Model.sanitize` 跳过 `vision_tower` 与 `model.visual` 前缀，再把语言塔键名改到 `language_model`。`__call__` 不再接收像素或波形。因此一份带 `vision_config` 的 Qwen3.5 checkpoint 可以被这段文本 `load` 读进语言塔；视觉张量被丢掉，并不等于这段调用已经做了视觉或语音前向。

抽查的 checkpoint（Hugging Face `config.json`）：

- `mlx-community/Qwen2.5-1.5B-Instruct-4bit`：`model_type` 为 `qwen2`，`architectures` 为 `Qwen2ForCausalLM`，`quantization` 为 `{"group_size": 64, "bits": 4}`，没有 `mode` 键。
- `mlx-community/Qwen3.5-2B-4bit`：`model_type` 为 `qwen3_5`，同文件还有 `text_config`、`vision_config`、`image_token_id`、`video_token_id`。
- `mlx-community/Qwen3-ASR-0.6B-8bit`：`model_type` 为 `qwen3_asr`，`architectures` 为 `Qwen3ASRForConditionalGeneration`。见文末边界。

公开生成示例先 `tokenizer.apply_chat_template(..., add_generation_prompt=True)`，再把得到的字符串交给 `generate` / `stream_generate`。来源：[mlx-lm README](https://github.com/ml-explore/mlx-lm/blob/56230fa02b8f/README.md)、[Qwen MLX-LM 页](https://qwen.readthedocs.io/en/v2.5/run_locally/mlx-lm.html)。`stream_generate` 若收到字符串，会 `tokenizer.encode` 成 `mx.array`，模型看到的是 token id。

`load_tokenizer` 再下载 tokenizer 侧文件，并按模型选择流式 detokenizer。`StreamingDetokenizer.add_token` 吃一个 token id；`last_segment` 返回上次访问之后新增的可读片段，注释写明通常是整词，所以某次片段可以还不是完整解码。`finalize()` 之后 `detokenizer.text` 才对齐 `tokenizer.decode(detokenizer.tokens)`。来源：`mlx_lm/tokenizer_utils.py`。

同文件还有 `sharded_load` 与 `pipeline_load`，使用 `mx.distributed`。单进程路径是 `load`，不是这两支。

## 量化的两个时刻

时刻 A，转换时写出新的量化权重。`convert(..., quantize=True)` 以 `lazy=True` 加载，再调用 `quantize_model`。`defaults_for_mode` 在调用方没给 group size / bits 时：

| `mode` | group size | bits |
|---|---|---|
| `affine` | 64 | 4 |
| `mxfp4` | 32 | 4 |
| `nvfp4` | 16 | 4 |
| `mxfp8` | 32 | 8 |

命令行 `mlx_lm.convert -q` 走这条路径。来源：README 与 `mlx_lm/convert.py`、`quantize_model`。

`quantize_model` 用 `nn.quantize(model, group_size, bits, mode=mode, class_predicate=...)`。谓词跳过没有 `to_quantized` 的模块，也跳过最后一维不能被 group size 整除的权重。结果写入 `config["quantization"]`，并复制到 `config["quantization_config"]`。`mlx.nn.quantize` 的文档签名是 `quantize(model, group_size=None, bits=None, mode="affine", ...)`，默认 mode 为 `"affine"`。来源：[mlx.nn.quantize](https://ml-explore.github.io/mlx/build/html/python/_autosummary/mlx.nn.quantize.html)。`mlx.core.quantize` 页列出的 mode 名为 `affine`、`mxfp4`、`mxfp8`、`nvfp4`。来源：[mlx.core.quantize](https://ml-explore.github.io/mlx/build/html/python/_autosummary/mlx.core.quantize.html)。

时刻 B，加载已经量化过的 checkpoint。`load_model` 若看到 `config["quantization"]`，按其中的 `group_size`、`bits`、`mode`（缺省 `"affine"`）调用 `nn.quantize`。谓词只保留权重里存在 `{path}.scales` 的层，并用 `infer_quant_config` 从已打包权重的形状回推 bits 与 group size。因此 Qwen2.5 4-bit 文件里没有 `mode` 时，加载侧用 affine。`quantization_config.quant_method` 为 `awq` 或 `gptq` 时，先 `_transform_awq_weights` 再量化；这次读到的分支只接受 4 bit。`bitnet`、`mxfp4`、`mxfp8`、`compressed-tensors`、`modelopt` 的 `NVFP4` 各有分支。这是装载已有打包格式，不是再次把 fp16 权重量化一遍。

KV cache 量化是第三处，且默认不发生。`generate_step` 的 `kv_bits` 默认 `None`。非 `None` 时，`maybe_quantize_kv_cache` 在 cache 的 `offset >= quantized_kv_start` 之后把该层换成量化 cache。`DEFAULT_QUANTIZED_KV_START = 5000`，`kv_group_size` 默认 64。来源：`mlx_lm/generate.py`。

Swift 侧对应两处。加载已量化权重：`loadWeights` 读 safetensors 后 `applyCheckpoint`；若 config 带 `perLayerQuantization`，则 `quantize(model:)`，且只处理权重里有 `{path}.scales` 的层，然后 `module.update`。来源：`Libraries/MLXLMCommon/Load.swift`。`BaseConfiguration` 从 `config.json` 的 `quantization` 解码 `group_size`、`bits`、`mode`，mode 缺省为 affine。来源：`Libraries/MLXLMCommon/BaseConfiguration.swift`。

写出新量化文件：`ModelConversion.convert` 要求源目录是 safetensors，且源 config 还没有 `quantization` / `quantization_config`；已量化源会得到 `ModelConversionError` 文案 “Re-quantizing is not supported by this conversion helper.”。量化实现调用 `MLX.quantized(...)`，再把 `quantization` 与 `quantization_config` 写回 `config.json`。来源：`Libraries/MLXLMCommon/ModelConversion.swift`。

Swift 生成参数里的 KV 量化默认值与 Python 不同。`GenerateParameters.kvBits` 默认是 `nil`（不量化）。`quantizedKVStart` 的初始化默认值是 `0`，不是 5000。来源：`Libraries/MLXLMCommon/Evaluate.swift` 的 `GenerateParameters.init`。

## Python：如何把 token 流出来

`stream_generate` 返回 `Generator[GenerationResponse, None, None]`。`generate` 只是把其中的 `response.text` 拼成完整字符串。

```python
# 来源: mlx-lm mlx_lm/generate.py，commit 56230fa02b8f
generation_stream = mx.new_thread_local_stream(mx.default_device())

for n, (token, logprobs, from_draft) in enumerate(token_generator):
    if token in tokenizer.eos_token_ids:
        break
    detokenizer.add_token(token)
    yield GenerationResponse(
        text=detokenizer.last_segment,
        token=token,
        ...
    )
```

一次 yield 的字段：`text`（可为空字符串）、`token`（int）、`logprobs`、`from_draft`、`prompt_tokens`、`prompt_tps`、`generation_tokens`、`generation_tps`、`peak_memory`（`mx.get_peak_memory() / 1e9`，单位 GB）、`finish_reason`。循环中的 yield 其 `finish_reason` 为 `None`。遇到 EOS 时先 `break`，该 EOS 不会先 `add_token`；循环后再 `detokenizer.finalize()`，并再 yield 一次，`finish_reason` 为 `"stop"` 或 `"length"`。因此宿主若只累加中间 `text`，最后一次 finalize 的尾巴在最后一条响应里。

`generate_step` 在 `with mx.stream(stream)` 里做前向。prefill 每步最多 `prefill_step_size` 个 token，默认 `DEFAULT_PREFILL_STEP_SIZE = 2048`，每步之后 `mx.eval([c.state for c in prompt_cache])`。解码一步取最后一个位置的 logits，采样，再把 cache state 留在这条 stream 上。投机解码走 `speculative_generate_step`，`draft_model` 为 `None` 时不进入。

整段生成包在 `wired_limit(model, [stream])` 里。进入时 `maybe_set_recommended_wired_limit()` 读 `mx.device_info()["max_recommended_working_set_size"]`，有该字段则 `mx.set_wired_limit`。退出前 `mx.synchronize(stream)`，再把 limit 设回旧值。模型字节数超过推荐工作集的 0.9 倍时打印警告，并指向 README 的 Large Models：该行为需要 macOS 15.0 或更高；可用 `sudo sysctl iogpu.wired_limit_mb=N`。来源：`mlx_lm/generate.py` 的 `wired_limit` 与 README “Large Models”。这是进程级 wired limit，不是把权重复制到一块独立的 GPU 显存。

调用方线程就是跑这个生成器的线程。`generation_stream` 是默认设备上的 thread-local MLX stream，不是另起的操作系统线程名。GPU 命令进这条 stream；CPU 侧只在读 `token`、detokenize 和计时。

## Swift：类型与调用顺序

文本路径上的类型，均在 mlx-swift-lm `635290754327`：

| 类型 | 文件 | 角色 |
|---|---|---|
| `LLMTypeRegistry` | `Libraries/MLXLLM/LLMModelFactory.swift` | `model_type` 字符串到模型构造器 |
| `LLMRegistry` | 同上 | 一批写好的 `ModelConfiguration`，含多条 Qwen id |
| `LLMModelFactory` | 同上 | `loadContainer` / `_load` |
| `ModelContext` | `Libraries/MLXLMCommon/ModelFactory.swift` | `configuration`、`model`、`processor`、`tokenizer`；构造时 `model.train(false)` |
| `ModelContainer` | `Libraries/MLXLMCommon/ModelContainer.swift` | 外面的可 `Sendable` 外壳 |
| `UserInput` / `LMInput` | `UserInput.swift`、生成 API | 准备前 / 准备后的输入 |
| `GenerateParameters` | `Evaluate.swift` | `maxTokens`、`temperature`、`prefill`、`kvBits` 等 |
| `Generation` | `Evaluate.swift` | `.chunk(String)`、`.reasoning`、`.info`、`.toolCall` 等 |
| `TokenGeneration` | `Evaluate.swift` | `.token(Int)` 与 `.info` |
| `TokenIterator` | `Evaluate.swift` | prefill 与逐步 `next()` |

`LLMTypeRegistry.shared` 里本次读到的 Qwen 键：`qwen2` → `Qwen2Model`，`qwen3` → `createQwen3CompatibleModel`，`qwen3_moe`，`qwen3_next`，`qwen3_5` → `Qwen35Model`，`qwen3_5_moe`，`qwen3_5_text` → `Qwen35TextModel`，以及 `qwen3_8` / `qwen3_8_moe` / `qwen3_8_text`（构造器与 3.5 相同）。没有 `qwen3_asr`，没有 `whisper`。同一份 Swift 树的 `mlx_lm/models` 对照：Python `56230fa02b8f` 的 `mlx_lm/models/` 没有 `qwen3_5_text.py`、`qwen3_8.py`。两边的类型表不是同一份。

`LLMRegistry` 里写死的 id 包括 `mlx-community/Qwen2.5-7B-Instruct-4bit`、`mlx-community/Qwen2.5-1.5B-Instruct-4bit`、`mlx-community/Qwen3-0.6B-4bit`、`mlx-community/Qwen3-4B-4bit`、`mlx-community/Qwen3-8B-4bit`、`mlx-community/Qwen3-30B-A3B-4bit`、`mlx-community/Qwen3.5-2B-4bit`、`mlx-community/Qwen3.6-27B-4bit`、`mlx-community/Qwen3.8-27B-4bit`、`mlx-community/Qwen3.8-27B-8bit`。未注册的 id 仍可用 `ModelConfiguration(id:)` 加载，只要 `model_type` 在类型表里。

`_load` 的顺序：读 `config.json` 解出 `BaseConfiguration.modelType`；`typeRegistry.createModel`；若目录里有 `generation_config.json` 则用它覆盖 EOS；`async let` 加载 tokenizer 的同时 `await loadWeights`；再用 tokenizer 做 `ModelContext`。权重文件选择写在 `Load.swift`：先用 `model.safetensors.index.json` 里真实存在的分片，否则 `model*.safetensors`，再退到 `weight*.safetensors`。注释写明这与 `mlx_lm.utils.load_model` 的 `model*.safetensors` 约定对齐，并避免沿用未量化源仓库里已经不存在的分片名。

```swift
// 来源: mlx-swift-lm Libraries/MLXLMCommon/ModelContainer.swift
let lmInput = try await modelContainer.prepare(input: userInput)
let stream = try await modelContainer.generate(
    input: lmInput, parameters: parameters)
for await generation in stream {
    switch generation {
    case .chunk(let text): print(text)
    case .info(let info): print(info.tokensPerSecond)
    default: break
    }
}
```

`prepare` 把 `UserInput` 交给 `processor.prepare`，得到 `LMInput`。`generate` 先按输入需要的 `ModelComponent` 做按需加载，再在 `SerialAccessContainer.read` 里调用 `MLXLMCommon.generate`。后者构造 `TokenIterator`（内部 `prepare` 做 prefill），再 `generateTask` 返回 `AsyncStream<Generation>`。流里的 `.chunk` 是解码后的字符串，不是 token id。要 id 用 `generateTokens`，事件是 `TokenGeneration.token(Int)`。

`TokenIterator.next()` 先 `step` 算出下一个 token 并 `asyncEval` 该 token 与各层 cache state，然后对上一步的 token 调用 `.item(Int.self)` 把标量读回 Swift。注释写明：不把 cache 放进同一次 `asyncEval` 的话，函数式更新会把每一步的中间数组留着。每 256 个 token 调用 `MLX.Memory.clearCache()`。来源：`Evaluate.swift`。

`Qwen2Model.callAsFunction(_ inputs: MLXArray, cache: [KVCache]?)` 与 Python 一样：embedding 或 `lm_head`，不接收音频缓冲。来源：`Libraries/MLXLLM/Models/Qwen2.swift`。文件头写明移植自 `mlx_lm/models/qwen2.py`。`Libraries/MLXLLM/Models/Qwen35.swift` 的文件头写明移植自 `mlx_lm/models/qwen3_5.py`；`Qwen35Model.excludedCheckpointNamespaces` 指向 `Qwen35CheckpointPolicy.excludedTargetNamespaces`。同仓库 `Libraries/MLXVLM/Models/Qwen35.swift` 是另一份移植，文件头写的是 `mlx-vlm` 的 `qwen3_5`，类名也是 `Qwen35`，但是 VLM 目标。

`ModelFactoryRegistry` 的 trampoline 顺序先 `MLXVLM.TrampolineModelFactory`，后 `MLXLLM.TrampolineModelFactory`。自由函数 `loadModelContainer(from:using:id:)` 会按这个顺序试。直接调用 `LLMModelFactory.shared.loadContainer` 只用文本注册表。`VLMTypeRegistry` 同样登记了 `qwen3_5`、`qwen3_5_moe`、`qwen3_8`、`qwen3_8_moe`，另外还有 `qwen2_vl`、`qwen2_5_vl`、`qwen3_vl`、`qwen3_vl_moe`。文本注册表没有这些 `*_vl` 键。

## 内存与线程落在哪

数组：权重和激活是 MLX 数组，住在统一内存里。Python 侧加载后的参数图由 `mx.eval(model.parameters())` 物化。Swift 侧 `Load.swift` 注释写明：safetensors 加载是惰性的，按文件内字节范围拆开，用并发 `eval` 重叠读取与分配；物化之后权重仍在统一内存。设备差别是 stream，不是第二份拷贝。来源：统一内存文档与 `Load.swift` 头部注释。

Python 生成线程：调用 `stream_generate` 的那条线程跑循环；计算图提交到 `generation_stream`（默认设备上的 thread-local stream）。wired limit 在该 `with` 块内改变进程的 `mx.set_wired_limit`，块结束前同步这条 stream。

Swift 互斥：`ModelContainer` 持有 `SerialAccessContainer`。实现注释写明，普通 `actor` 不能保证整个 `async` 调用期间独占，所以用 `AsyncMutex`（自身是 `actor`）把 `read` / `update` 包成临界区。`ModelContainer` 的注释要求闭包返回前对 `MLXArray` 做 eval，因为 `MLXArray` 不是 `Sendable`。来源：`Utilities/SerialAccessContainer.swift`、`ModelContainer.swift`。

Swift 生成线程：`generate` 的注释写明，独占访问只覆盖 prefill 那一次进入模型；返回流之后权重可以被并发读，因为它们已经 eval。解码循环在 `generateLoopTask` 里。`executionStream = StreamOrDevice.default.stream`。循环体交给 `GenerationWorker`。该类型是 `actor`，executor 是 `DispatchSerialQueue(label: "ml-explore.mlx-swift-lm.generation")`，注释写明把阻塞的 MLX 工作移出 Swift 协作线程池。循环结束 `executionStream.synchronize()`。来源：`Evaluate.swift`、`Utilities/GenerationWorker.swift`。

取消：`for await` 提前结束会 `task.cancel()`。同一段注释写明，流被提前掐断后，模型、参数和 KV cache 上的计算还会再跑一小段（注释原文 “typically a few ms”）。`next()` 在调用 `asyncEval` 之前检查 `Task.isCancelled`，注释给出的失败名是 `kIOGPUCommandBufferCallbackErrorBackgroundExecutionNotPermitted`。这是源码注释，不是本机复测。

`wiredMemoryTicket` 只在调用方传入非 nil 值时包住 worker。注释写明它只在支持 wired memory 控制的 GPU 上生效，版本写的是 macOS 15 / iOS 18 / tvOS 18 或更新。不传 ticket 时这条 Swift 路径不调用对应的 limit。Python 的 `stream_generate` 则总会进入 `wired_limit`。

`UserInput.Audio.asMLXArray` 用 AVFoundation 把文件解码成 32-bit float PCM，或直接返回已有 `MLXArray`。来源：`UserInput+Audio.swift`。它停在数组，不构造 `qwen2` / `qwen3` 模型，也不调用 `stream_generate`。

## 语音模型不能走这段调用序列

014 把识别流模型留给 015。下面是机制边界，不是 015 的名单。

`mlx_lm.load` → `stream_generate`，以及 `LLMModelFactory.loadContainer` → `prepare` → `generate`，选类的键都是 `config.json` 的 `model_type`，前向输入是 token id（或文本模型的 `input_embeddings`）。本次 `mlx_lm/models/` 目录 139 个文件名里，没有文件名包含 `whisper`、`audio`、`asr`。`LLMTypeRegistry` 与 `VLMTypeRegistry` 的键里没有 `qwen3_asr` 或 `whisper`。对 `qwen3_asr` 调用 `_get_classes` 会在 `import mlx_lm.models.qwen3_asr` 失败时抛出 “Model type qwen3_asr not supported.” Swift 文本工厂会 `ModelFactoryError.unsupportedModelType`。

`mlx-community/Qwen3-ASR-0.6B-8bit` 的 config 虽然带 `quantization.bits = 8`、`group_size = 64`、`mode = affine`，打包格式和文本量化相同，架构键仍然是 `qwen3_asr`。量化字段不能把它送进 `Qwen2Model` 或 `Qwen3Model`。

Qwen3-ASR 的公开加载入口在另一个包：

```python
# 来源: Blaizzy/mlx-audio mlx_audio/stt/models/qwen3_asr/README.md
from mlx_audio.stt import load

model = load("mlx-community/Qwen3-ASR-0.6B-8bit")
result = model.generate("audio.wav", language="English")
```

Swift 侧公开示例是 `Qwen3ASRModel.fromPretrained`，流式是 `generateStream`，事件为 `.token` / `.result` / `.info`，模块是 `MLXAudioSTT`，不是 `MLXLLM`。来源：[mlx-audio-swift Qwen3ASR README](https://github.com/Blaizzy/mlx-audio-swift/blob/main/Sources/MLXAudioSTT/Models/Qwen3ASR/README.md)。

因此：Qwen3-ASR、该 README 表里的 Whisper，以及任何 `model_type` 不在 `mlx_lm.models` 或 `LLMTypeRegistry` 里的识别权重，都不能由本节的 `load` / `loadContainer` / `stream_generate` / `ModelContainer.generate` 加载。它们可以同样使用 MLX 数组运行时，那是另一个包的调用序列。本文件不把这些模型写成已经跑在 mlx-lm 或 `LLMModelFactory` 上。

`VLMModelFactory` 能加载 `qwen2_vl`、`qwen2_5_vl`、`qwen3_vl` 与带视觉配置的 `qwen3_5`。那条路径的处理器名是 `Qwen2VLProcessor`、`Qwen3VLProcessor` 等，输入侧是图或视频，不是 015 要的识别流。`qwen3_5.py` 文本 `Model.__call__` 也不吃波形。

## 开放问题

- 015 的具体模型 id 不在本文件的读取范围内。上面的边界只覆盖已经对上源码的入口：`model_type` 不在两份注册表里就不能走文本/视觉工厂。某个 015 模型若另行实现了与 `LLMModel` 相同的前向，本文件没有看到这样的注册。
- `qwen3_5` 同时出现在 `LLMTypeRegistry` 与 `VLMTypeRegistry`。自由函数先试 VLM trampoline。`LLMModelFactory.shared` 固定走文本表。带 `vision_config` 的 `mlx-community/Qwen3.5-2B-4bit` 在 Python 文本 `load` 里会丢掉 `vision_tower` 与 `model.visual`。Swift 两条 `Qwen35` 类是否对同一目录读入同一组张量，本次没有跑加载。
- Python `models/` 没有 `qwen3_8` 与 `qwen3_5_text` 模块，Swift `LLMTypeRegistry` 有这些键。同一 checkpoint 在两个包上的 `model_type` 是否都能实例化，取决于该 config 的字符串，而不是仓库名字。
- `UserInput.Audio.asMLXArray` 能得到 PCM `MLXArray`。本次读到的 LLM/VLM 类型表没有把这个数组接到 ASR 模型。哪个 `UserInputProcessor` 会读它，本次没有逐个处理器读完。
- Python `quantized_kv_start` 默认 5000，Swift `GenerateParameters.quantizedKVStart` 默认 0。两者都要 `kv_bits` / `kvBits` 非空才会量化 cache。默认参数并排使用时，cache 量化起点不一致。
- `stream_generate` 在 EOS 上先跳出再 `finalize`。Swift `.chunk` 由 detokenizer/handler 拼字符串。两段实现是否对同一个 Qwen chat 模板吐出相同的片段边界，本次没有对拍。
- wired limit 与后台 `asyncEval` 的注释描述了 macOS 版本和一种 GPU 错误名。本文件没有在本机改 `sysctl` 或复测。

## 检索日志

| # | 查询或 URL | 结果 |
|---|---|---|
| 1 | 本地 `dev-docs/notes/014-research-open-model-mlx-index.md` | 读到 017 的范围：只写加载、量化、流式与边界 |
| 2 | 检索 `mlx-lm load Qwen model generate stream` | 命中 Qwen 文档 MLX-LM 页、Hugging Face MLX 页、mlx-lm README |
| 3 | 检索 `mlx-swift-lm LLMModelFactory loadContainer Qwen` | 命中 `ModelContainer.swift`、`LLMModelFactory.swift`、model-container 说明 |
| 4 | 检索 `mlx-lm utils.py load stream_generate quantize` | 命中 `utils.py`、`generate.py` 的函数名 |
| 5 | `https://raw.githubusercontent.com/ml-explore/mlx-lm/main/README.md` | 读到 `load` / `stream_generate` / `convert -q` / wired limit |
| 6 | GitHub API `mlx-lm/contents/mlx_lm/models`，139 个文件名过滤 `whisper`/`audio`/`asr` | 空结果；Qwen 文件名见正文表 |
| 7 | 下载并阅读 `mlx_lm/utils.py`（`load`、`load_model`、`quantize_model`、`_get_classes`） | 得到加载与量化顺序 |
| 8 | 下载并阅读 `mlx_lm/generate.py` 的 `stream_generate`、`generate_step`、`wired_limit` | 得到 yield 字段与 stream |
| 9 | 下载并阅读 `mlx_lm/convert.py` 的 `convert` | 量化发生在 lazy load 之后 |
| 10 | 下载 `qwen2.py`、`qwen3.py`、`qwen3_5.py` | 前向参数是 token；3.5 的 sanitize 丢视觉键 |
| 11 | `https://ml-explore.github.io/mlx/build/html/usage/unified_memory.html` | 读到统一内存与 “specify the device” 段落 |
| 12 | `https://ml-explore.github.io/mlx/build/html/usage/using_streams.html` | 读到默认 stream 与 `stream=` |
| 13 | `https://ml-explore.github.io/mlx/build/html/python/nn/_autosummary/mlx.nn.quantize.html` | 404，文件不在该路径 |
| 14 | 检索 `mlx.nn.quantize group_size bits mode` | 命中正确路径 `python/_autosummary/mlx.nn.quantize.html` 与 `mlx.core.quantize` |
| 15 | 下载 `LLMModelFactory.swift` | 读到 Qwen 类型表与 `_load` |
| 16 | 下载 `ModelContainer.swift`、`Evaluate.swift` 的 `generate` 与 `TokenIterator.next` | 读到 `AsyncStream<Generation>` 与 `.item(Int.self)` |
| 17 | 下载 `Load.swift`、`BaseConfiguration.swift`、`ModelConversion.swift` | 读到 safetensors 选择、加载时 quantize、转换时禁止再量化 |
| 18 | 下载 `SerialAccessContainer.swift`、`GenerationWorker.swift` | 读到 `AsyncMutex` 与 `DispatchSerialQueue` 标签 |
| 19 | 下载 `VLMModelFactory.swift` 类型表，并在全文搜索 `audio`/`whisper`/`asr` | 类型表无这些键；搜索空结果 |
| 20 | 下载 `UserInput+Audio.swift` | 只看到 PCM 解码成 `MLXArray` |
| 21 | `https://huggingface.co/mlx-community/Qwen2.5-1.5B-Instruct-4bit/raw/main/config.json` | `model_type=qwen2`，量化 4 bit、group 64 |
| 22 | `https://huggingface.co/mlx-community/Qwen3.5-2B-4bit/raw/main/config.json` | `model_type=qwen3_5`，含 `vision_config` |
| 23 | `https://huggingface.co/mlx-community/Qwen3-ASR-0.6B-8bit/raw/main/config.json` | `model_type=qwen3_asr` |
| 24 | `https://raw.githubusercontent.com/Blaizzy/mlx-audio/main/mlx_audio/stt/models/qwen3_asr/README.md` | 入口是 `mlx_audio.stt.load`，不是 `mlx_lm.load` |
| 25 | `https://raw.githubusercontent.com/Blaizzy/mlx-audio-swift/main/Sources/MLXAudioSTT/Models/Qwen3ASR/README.md` | 入口是 `Qwen3ASRModel.fromPretrained` / `generateStream` |
| 26 | 四个仓库 `LICENSE` 与 `commits/main` | MIT 首行与上表 commit |
| 27 | `mlx_lm/tokenizer_utils.py` 的 `StreamingDetokenizer` | `last_segment` 可短于完整 decode，`finalize` 后才对齐 |
| 28 | `https://ml-explore.github.io/mlx/build/html/usage/lazy_evaluation.html`（检索摘要加文档站交叉） | `mx.eval` 才物化；与 `load_model` 的 `mx.eval(model.parameters())` 一致 |

<!-- research-complete -->
