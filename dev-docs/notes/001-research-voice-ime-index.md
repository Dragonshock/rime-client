# 001-research-voice-ime-index：语音增强型 Rime 客户端首轮调研全局索引

## 1. 一句话定位

本索引定义「在不破坏 Squirrel 现有架构与 `librime` 键盘输入功能的前提下，提供离线本地模型与云端大模型语音输入优化接口」首轮调研的全局目标表、闭源基线与开源源码定位、以及预分配文档编号映射。

## 2. 调研目标与预分配编号表（Target Table）

本轮调研按机制边界拆分为 1 篇全局索引（本文档）、4 篇专项调研文档（`research-`）与 1 篇源码分析文档（`code-analysis-`），预分配编号 `001`～`006`：

| 预分配编号 | 文档路径 | 文档类型 | 调研目标（Target） | 源码/产物性质 | 本地路径 / 外部定位 | 解决的核心问题 |
|---|---|---|---|---|---|---|
| `001` | `dev-docs/notes/001-research-voice-ime-index.md` | `research-`（全局索引） | 首轮调研全局导航与编号预分配 | 索引文档 | `dev-docs/notes/001-research-voice-ime-index.md` | 确立首轮调研的子域边界、目标清单与预分配编号 |
| `002` | `dev-docs/notes/002-research-doubao-ime-architecture-and-audio-pipeline.md` | `research-` | 豆包输入法（`DoubaoIme.app` v1.1.0 / build 1010005）包结构、组件拓扑、音频采集与 IMK 上屏机制 | 闭源基线（未发现公开源码，基于本地 Mach-O 产物与 REA 逆向） | `/Library/Input Methods/DoubaoIme.app`（主程序 SHA-256: `5b48748fa6394811cba0f8c05905ffebb9a2cd02faf6d47b26deb3d9b86247e0`） | 逆向弄清 `DoubaoImeInputController`、`OimeEngine.framework`、`OimeCommon.framework` 如何组织进程、拦截快捷键、采集麦克风音频并将结果注入 `IMKTextInput` |
| `003` | `dev-docs/notes/003-research-doubao-ime-local-models-and-inference.md` | `research-` | 豆包输入法离线语音识别（`ASRModel`）与本地上下文大模型（`acllm` / `ContextAuxModel` / `ASRContext`）技术栈 | 闭源基线（未发现公开源码，基于本地模型产物、SQLite 数据库与 REA 逆向） | `/Users/dragon/Library/Application Support/DoubaoIme/`（含 `ASRModel/`、`ASRContext/context.db`、`ContextAuxModel/`、`OfflineModelDownloads/acllm_*`） | 逆向弄清豆包输入法离线 ASR 模型与本地 LLM（`acllm_main_model_mac`、`acllm_context_model_mac`）的格式、推理运行时、以及 `context.db`（`modify_pair` / `user_word`）如何做个人词库与纠错对召回 |
| `004` | `dev-docs/notes/004-research-doubao-ime-cloud-asr-and-llm-pipeline.md` | `research-` | 豆包输入法云端流式 ASR、大模型语音润色优化协议与端云协同机制 | 闭源基线（未发现公开源码，基于本地网络 SDK 产物与 REA 逆向） | `/Library/Input Methods/DoubaoIme.app/Contents/Frameworks/Parfait.framework`、`libime_net_sdk.dylib`、`libsscronet.dylib` | 逆向弄清豆包输入法通过什么网络栈（TTNet/Cronet/Parfait/WebSocket/gRPC）传输音频流、如何触发云端大模型对语音识别结果进行语境优化与纠错、以及离线与云端如何协同切换 |
| `005` | `dev-docs/notes/005-code-analysis-squirrel-index-and-imk-lifecycle.md` | `code-analysis-` | Squirrel（`rime/squirrel`）与 `librime` C API 源码索引、IMK 生命周期及非侵入式语音扩展边界分析 | 开源参考源码（本地浅克隆） | `dev-docs/refs/code/squirrel`（HEAD `b7970a9`）、`dev-docs/refs/code/librime`（HEAD `bb56876`） | 建立 Squirrel 与 `librime` 的模块/符号/调用图索引，精确定位 `SquirrelInputController`、`SquirrelPanel`、`RimeWrapper` 中可在不改动 `librime` 且不影响现有键盘输入的前提下挂载语音输入与 LLM 后处理接口的非侵入式扩展切入点 |
| `006` | `dev-docs/notes/006-research-macos-ime-voice-llm-integration-mechanisms.md` | `research-` | macOS InputMethodKit 语音采集权限边界、Apple Silicon 本地 ASR/LLM 推理引擎与云端流式后处理集成机制 | 公开技术规范与开源方案 | Apple Developer 文档、`whisper.cpp` / `sherpa-onnx` / `mlx` / `llama.cpp`、macOS TCC / `AVAudioEngine` | 弄清 macOS 输入法后台进程（`LSBackgroundOnly`）下的麦克风 TCC 权限、音频会话管理、同步 IPC 防阻塞机制，以及开源离线 ASR + 本地/云端 LLM 接入 Squirrel 的技术选型边界 |

## 3. 核心参考目录骨架

```
# 源：本地文件系统实测（ls / git rev-parse 验证）
dev-docs/
├── refs/
│   └── code/
│       ├── squirrel/                      # rime/squirrel 浅克隆 (HEAD: b7970a9)
│       │   └── sources/                   # Squirrel 核心 Swift 源码 (SquirrelApp, SquirrelInputController, SquirrelPanel 等)
│       └── librime/                       # rime/librime 浅克隆 (HEAD: bb56876)
│           └── src/rime_api.h             # librime 对外 C API 契约定义
└── notes/
    ├── 001-research-voice-ime-index.md
    ├── 002-research-doubao-ime-architecture-and-audio-pipeline.md
    ├── 003-research-doubao-ime-local-models-and-inference.md
    ├── 004-research-doubao-ime-cloud-asr-and-llm-pipeline.md
    ├── 005-code-analysis-squirrel-index-and-imk-lifecycle.md
    └── 006-research-macos-ime-voice-llm-integration-mechanisms.md

# 外部本地逆向分析对象（只读，位于系统与用户目录）：
/Library/Input Methods/DoubaoIme.app/      # 豆包输入法 macOS 客户端 (v1.1.0 / Wave Build 1010005)
~/Library/Application Support/DoubaoIme/   # 豆包输入法运行时数据、离线模型与上下文数据库
```

## 4. 推荐阅读顺序

1. 先读 **`005-code-analysis-squirrel-index-and-imk-lifecycle.md`**：掌握 Squirrel 的现有架构、`IMKInputController` 到 `librime` 的调用链，以及「不碰 Rime 核心、保持键盘输入不变」条件下的非侵入式扩展接缝。
2. 按序读 **`002` → `003` → `004`**：完整还原豆包输入法在 macOS 上的「客户端事件与音频采集 → 离线 ASR 与本地 `acllm` 大模型 + `context.db` 个性化召回 → 云端流式 ASR 与大模型后处理润色」三层实现机制。
3. 最后读 **`006-research-macos-ime-voice-llm-integration-mechanisms.md`**：对照豆包输入法的闭源实现与 macOS 平台约束，审视接入 Squirrel 时的本地模型与云端大模型接口方案。

<!-- research-complete -->
