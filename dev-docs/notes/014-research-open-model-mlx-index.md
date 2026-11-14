# 014-research-open-model-mlx-index：开源模型与 MLX 集成调研索引

本轮填补的空白是：语音接口的识别与文本优化改用开源模型，首选 Qwen 系列，集成路径研究 MLX。豆包输入法只作机制参考，不再当作优化绑定。

目录目标最后一行（2026-11-12）：语音输入接口的优化绑定改为开源模型，首选 Qwen 系列，集成路径研究 MLX；豆包输入法只作机制参考。阶段仍是调研。

接口形状仍以 `007` 的九条原语为准。`013` 是未确认蓝图，其中「点名制品角色空着」写于目标改写之前，不作为本轮绑定的前提。

## 预分配

| 编号 | 路径 | 类型 | 只回答 |
|---|---|---|---|
| 014 | `dev-docs/notes/014-research-open-model-mlx-index.md` | research | 本轮切分与阅读顺序 |
| 015 | `dev-docs/notes/015-research-open-asr-models-performance.md` | research | 能担任识别流的开源语音模型有哪些，已发表的体积、时延、中文效果、是否流式 |
| 016 | `dev-docs/notes/016-research-qwen-text-models-performance.md` | research | 能担任改写流的开源文本模型有哪些，Qwen 系列各档的体积、内存、生成速度与文本优化能力 |
| 017 | `dev-docs/notes/017-research-mlx-qwen-integration.md` | research | MLX 与 MLX-Swift 如何加载、量化并流式运行上述模型，调用序列与边界 |
| 018 | `dev-docs/notes/018-derivation-open-model-mlx-binding.md` | derivation | 上述事实如何对应识别流与改写流端口；当前倾向标为候选 |
| 019 | `dev-docs/notes/019-gap-open-model-mlx.md` | gap | 仍未知、且能追溯到本轮目标或 007 原语的缺口 |

015 与 016 分开，因为识别吃采样帧、改写吃文本，性能口径不同。017 只写集成机制，不另选一套模型。018 在 015～017 之后写，不把未发表的数字补成事实。

阅读顺序：014 → 015 → 016 → 017 → 018 → 019。

<!-- research-complete -->
