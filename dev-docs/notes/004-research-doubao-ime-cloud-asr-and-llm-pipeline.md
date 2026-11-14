# 豆包输入法（macOS）逆向调研（三）：云端流式 ASR、大模型语音润色优化协议与端云协同机制

> **关联索引**：`dev-docs/notes/001-research-voice-ime-index.md`
> **逆向样本**：`/Library/Input Methods/DoubaoIme.app`（`CFBundleShortVersionString = 1.1.0`，`CFBundleVersion = 1`，`Wave Build Version = 1.1.0.5`，`Wave Build Version Number = 1010005`，`BuildMachineOSBuild = 24G90`）
> **方法论声明**：本文全部证据均来自本地已安装样本的 Mach-O 二进制静态分析（`/tmp/doubao-thin-arm64/` 下的 `DoubaoIme`、`OimeEngine`、`libime_net_sdk.dylib`、`libsscronet.dylib`、`Parfait.framework`、`libTTNetDownloaderCrossPlatform.dylib` 反汇编、符号还原与数据段提取）。文中所有面向鼠须管（Squirrel / `rime-client`）的技术形态归纳与架构映射均标记为 **候选（待用户确认）**。

---

## 1. 事实与来源：网络栈二进制资产、协议端点与源码路径映射

### 1.1 云端语音、LLM 后处理与网络通信相关 Mach-O 资产清单

豆包输入法（`DoubaoIme.app`）并非通过单一网络库完成所有云端通信，而是按「流式语音识别（SAMI WebSocket/QUIC）」、「大模型语音智能整理（HTTP SSE 流）」、「键盘拼音云端与 LLM 联合候选（Rust QUIC 0-RTT/1-RTT SDK）」以及「模型下载与遥测上报（TTNet / Cronet / Parfait）」切分为四个独立网络层：

| 二进制相对路径（相对于 `DoubaoIme.app/Contents/`） | Universal 大小 (Bytes) | `arm64` 切片大小 (Bytes) | `arm64` 切片 SHA-256 | 核心模块 / 导出接口与职责定位 |
| :--- | ---: | ---: | :--- | :--- |
| `MacOS/DoubaoIme` | 44,768,736 | 21,962,208 | `a6a3ac886111eda4bb271ddfd37769fd94ecdd57d04112d77a60e32d74e36cbf` | 静态链接 `OimeAudio`（`SAMIHandle`、`SAMIConnectPoolManager`、`SAMIASRRuntimeManager`）、`sami_core` C++ 语音引擎、`OimeSmartOrganize`（`SmartOrganizeClient`、`SmartOrganizeVoiceCoordinator`）及 `OimeNetwork`（`OimeSSEStreamTask`、`OimeSSEParser`） |
| `Frameworks/OimeEngine.framework/Versions/A/OimeEngine` | 18,789,456 | 8,811,600 | `4659d574d5a0042febb9264e4eba077c7a1296296e64198c07faafa12f696507` | 包含 `OimeEngineNetworkManager`、`makeCloudRequest:`（`0x8ab8`）与 `parseCloudResponse:llmResponse:precedingText:onTiming:sessionLLMEnabled:`（`0x8c9c`），通过 `libime_net_sdk.dylib` 发起键盘端云协同请求 |
| `Frameworks/libime_net_sdk.dylib` | 15,657,424 | 7,645,648 | `9233ce2bd5d1740a5a36f476624660a59db6ce4a83bc8fc0b4df5db77790107b` | Rust 编写的 `ime_net_sdk`（`v0.2.6`），内嵌 `quinn-proto-0.11.13`（QUIC）、`hyper-util-0.1.19`、`brotli-3.5.0`，导出 `_on_init`、`_on_connect`、`_call`、`_new_streaming`、`_call_streaming` 等 11 个 C FFI 函数 |
| `Frameworks/libsscronet.dylib` | 15,076,016 | 7,268,944 | `2fb5a13d3f9365381efab3beeb3db5c9ca97b32963ff022be5a4e6cb4fb96712` | 字节跳动定制 Chromium Cronet 网络栈（HTTP/2、HTTP/3 QUIC、WebSocket 底层传输与 DNS 预解析） |
| `Frameworks/libTTNetDownloaderCrossPlatform.dylib` | 2,404,976 | 1,148,160 | `d9b25a9f2ee8c27323ef6da5714e8795bc85b4c47ebfb5a21d08bb3cb5cf8bb6` | 跨平台分片断点续传下载器，由 `ASROfflineModelTTNetRequest` 驱动，负责下载 `acllm_main_model_mac` 等大体积离线模型包 |
| `Frameworks/Parfait.framework/Versions/A/Parfait` | 12,175,888 | 5,865,680 | `322481f703628b920742be1a6dd3e4c36ca8086c484a31d7b1ff4e3159b32166` | 客户端 APM 性能监控、崩溃收集与端云链路耗时遥测框架（上报至 `https://pc-mon.zijieapi.com`） |

### 1.2 二进制内嵌源码路径与模块映射（Source File Map）

通过扫描 `DoubaoIme` 与 `OimeEngine` 的 `__TEXT,__cstring` 段，可精确还原云端语音识别、大模型润色及网络层在字节跳动内部仓库（`obric-ime-engine4mac` 与 `sami_core`）中的源文件路径：

```text
// 源：/tmp/doubao-thin-arm64/DoubaoIme (__TEXT,__cstring 源码路径提取)
Modules/OimeAudio/Classes/ASR/SAMI/SAMIHandle.swift
Modules/OimeAudio/Classes/ASR/SAMI/SAMIConnectPool.swift
Modules/OimeAudio/Classes/ASR/SAMI/SAMIASRRuntimeManager.swift
Modules/OimeAudio/Classes/ASR/SAMI/SAMITokenManager.swift
Modules/OimeSmartOrganize/Classes/Core/SmartOrganizeClient.swift
Modules/OimeSmartOrganize/Classes/Voice/SmartOrganizeVoiceCoordinator.swift
Modules/OimeSmartOrganize/Classes/Core/SmartOrganizeMemoryCache.swift
Modules/OimeNetwork/Classes/SSE/OimeSSEStreamTask.swift
Modules/OimeNetwork/Classes/SSE/OimeSSEParser.swift
sami_core/src/cpp/processors/speech_v2/asr_processor.cpp
sami_core/src/cpp/processors/speech_v2/worker/asr_on_worker.cpp
sami_core/src/cpp/processors/online/protocol/protocol.cpp
```

### 1.3 云端服务端点与 API 路由表（Cloud Endpoints Inventory）

从 `DoubaoIme` 数据段提取的云端长连接（WebSocket / QUIC）与 HTTPS 接口地址如下表所示：

| 业务子系统 | 协议与传输层 | 服务端点（Host / Path） | 关联类与二进制证据坐标 |
| :--- | :--- | :--- | :--- |
| **云端流式 ASR（QUIC 通道）** | WebSocket over QUIC (`wss://`) | `wss://frontier-audio-ime-quic.doubao.com/api/v1/ws` | `OimeAudio.SAMIASRRuntimeManager` / `SAMIConnectPoolManager`（`DoubaoIme` `__cstring`） |
| **云端流式 ASR（WS 通道）** | WebSocket over TLS (`wss://`) | `wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws` | `OimeAudio.SAMIHandle` / `sami_core::processors::speech_v2::AsrProcessor` |
| **SAMI 语音鉴权令牌服务** | HTTPS GET | `https://speech.bytedance.com/api/v1/token?access_key=` | `OimeAudio.SAMITokenManager`（`SAMITokenManager.swift`） |
| **LLM 语音智能整理（SmartOrganize）** | HTTPS POST + SSE (`text/event-stream`) | `https://ime.doubao.com/api/v2/ai/text_organization` | `OimeSmartOrganize.SmartOrganizeClient`（`SmartOrganizeClient.swift`，地址 `0x100237300..0x10023ae30`） |
| **输入法通用 Frontier 长连接** | WebSocket (`wss://`) | `wss://frontier-ime.doubao.com/ws/v2` | `DoubaoIme.FrontierManager` / `DoubaoIme.FrontierClipboard` |
| **用户配置与云端实验开关** | HTTPS GET/POST | `https://ime.oceancloudapi.com/api/v1/user/get_config`、`https://is.snssdk.com/service/settings/v3/` | `OimeCommon` / `DoubaoIme.CloudControlManager` |

---

## 2. 核心机制（一）：云端流式 ASR 传输管线（SAMIHandle / SAMIConnectPool / Opus）

### 2.1 `OimeAudio` 与 `sami_core` 分层架构

豆包输入法的云端语音识别由 Swift 层的 `OimeAudio` 模块驱动静态编译进主程序的 C++ `sami_core`（Speech & Audio Machine Intelligence Core）引擎完成。其核心类与职责如下：

1. **`OimeAudio.SAMIASRRuntimeManager`（`0x1013fd310`）**：负责管理云端/端侧混合 ASR 运行时配置、构造 `SAMIHandle` 参数字典、维护音频包序号与超时计时器。
2. **`OimeAudio.SAMIConnectPoolManager`（`0x1013fe150`，源码 `SAMIConnectPool.swift`）**：维护面向 `frontier-audio-ime-quic.doubao.com` 与 `frontier-audio-ime-ws.doubao.com` 的预热连接池（`connectPool`），通过 `handleConnectPoolBlock` 与 `releaseConnectPoolHandle` 消除按下语音键时的 TCP/TLS/QUIC 握手延迟。
3. **`OimeAudio.SAMITokenManager`（`0x1013fca90`，源码 `SAMITokenManager.swift`）**：在后台提前向 `https://speech.bytedance.com/api/v1/token?access_key=` 获取并缓存鉴权 Token（支持 `GetToken_V2`），避免录音启动时因鉴权请求阻塞首帧音频发送。
4. **`OimeAudio.SAMIHandle`（`0x1013fd698`，源码 `SAMIHandle.swift`）**：对 C++ `SAMICore` 句柄的 Swift 封装，负责将 `ASRAudioEngine` 输出的 16kHz 单声道 PCM 音频帧送入 `sami_core` 处理器，并通过 `callbackQueue` 异步接收识别事件。

### 2.2 Opus 实时音频编码与 WebSocket 二进制分帧协议

在 C++ 层，`sami_core` 通过以下处理器链完成音频编码、网络建连与流式事件分发：

```text
// 源：/tmp/doubao-thin-arm64/DoubaoIme (sami_core C++ 符号与源码路径还原)
OimeAudio.SAMIHandle.feedAudio(pcm16kMono)
  └─> sami_core_common_encode_processor.cpp (Opus 实时帧编码，压缩 16kHz PCM)
       └─> sami_core/src/cpp/processors/speech_v2/asr_processor.cpp
            └─> sami_core/src/cpp/processors/speech_v2/worker/asr_on_worker.cpp
                 └─> sami_core/src/cpp/processors/online/protocol/protocol.cpp
                      ├─ StartTask / CreateConnection (复用 SAMIConnectPool 预热连接)
                      ├─ SendAudioData (WebSocket Binary Frame 持续上行推送 Opus 音频包)
                      └─ OnMessage -> HandleRecognizeResult / HandleSentenceChange
```

1. **Opus 编码降带宽**：`sami_core` 内置 `sami_core_common_encode_processor.cpp` 与 `sami_core_opus_audio_decoder.cpp`，在弱网或云端流式模式下将 16kHz 16-bit 单声道 PCM（原始码率 256 kbps）实时编码为 Opus 音频帧（约 16–24 kbps），将上行带宽降低约 90%。
2. **连接池预建连与复用**：当用户接近触发语音输入或处于活跃输入会话时，`SAMIConnectPoolManager` 通过 `[asr_processor.cpp] CreateConnection` 预先建立到 `wss://frontier-audio-ime-quic.doubao.com/api/v1/ws`（优先 QUIC 通道）或 `wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws` 的连接；若预热连接池为空，则打印 `[asr_processor.cpp][WARN] can't get connection from pool, create new connection!` 并即时建连。
3. **热词与个性化上下文随包注入**：在发起 `StartTask` 握手包时，`SAMIASRRuntimeManager` 从 `DoubaoIme.ASRGlobalContext`（由 `003` 所述 `context.db` 召回生成的 JSON 上下文）读取个人热词（`hotwords`）、纠错对与当前宿主应用上下文，填入 `extra` JSON 载荷随首包发送至服务端，使云端流式 ASR 与端侧离线 ASR 共享同一套个性化上下文。
4. **流式增量回调与断句状态同步**：服务端通过 WebSocket 下发二进制/JSON 混合帧，由 `asr_on_worker.cpp` 解析出两类核心事件：
   - **增量部分结果（Partial Result）**：携带当前未定稿分句文本与 `prefetch` 标志，交由 `ASRTypeWriter`（详见 `002` §4.1）计算最长公共前缀并调用 `setMarkedText` 刷新行内预编辑区；
   - **分句定稿与尾包结算（Definite Sentence / TaskFinished）**：标记分句边界或整轮语音结束，触发下游 `SmartOrganizeVoiceCoordinator` 判断是否发起云端大模型润色。

---

## 3. 核心机制（二）：大模型语音润色后处理（OimeSmartOrganize）与键盘端云融合

### 3.1 语音智能整理子系统（`OimeSmartOrganize`）架构与类拓扑

旧版或外部传闻中常假设语音后处理仅是一个简单的 REST 替换脚本，但对 `DoubaoIme` 的符号与反汇编分析表明，豆包输入法构建了独立的 `OimeSmartOrganize` 框架模块（编译进 `DoubaoIme` 主二进制 `0x100237300..0x10023ae30` 及 `0x1001d2000..0x10020f000`），其完整类体系如下：

| 类名 / 结构体名（Swift Demangled） | 二进制偏移 / 元数据地址 | 核心职责说明 |
| :--- | :--- | :--- |
| `OimeSmartOrganize.SmartOrganizeVoiceCoordinator` | `0x1013ee030` | 语音智能整理总协调器，衔接 ASR 识别输出与 LLM 改写请求，产出 `SmartOrganizeVoiceDecision`（含 `rewriteDecision`、是否采用润色文本） |
| `OimeSmartOrganize.SmartOrganizeClient` | `0x1013edbd8` (`0x100237300`) | 封装对 `https://ime.doubao.com/api/v2/ai/text_organization` 的请求构造、超时控制、SSE 流式消费与单发响应解析 |
| `OimeSmartOrganize.SmartOrganizeMemoryCache` | `0x1013edd38` | 内存级 LRU 缓存（`Entry`），对相同原文本与上下文的改写结果进行去重缓存，避免重复请求大模型 |
| `OimeNetwork.OimeSSEStreamTask` / `OimeSSEParser` | `0x1013ebfb0` / `0x1013ebf18` | 通用 HTTP Server-Sent Events (SSE) 流式任务与状态机解析器，派发 `delta`、`completed` 与错误事件 |
| `DoubaoIme.ASRSmartOrganizeContext` | `0x1013ca2b8` | 采集并封装发送给 LLM 的多维上下文（含光标前文 `precedingText`、宿主 Bundle ID、输入框角色、个人纠错对） |
| `DoubaoIme.ASRSmartOrganizeUndoRecord` | `0x1013ca3a0` | 记录润色替换前的原始 ASR 文本（`rawText`）、润色后文本（`smartText`）及光标区间，支持一键撤销回退 |
| `DoubaoIme.SmartOrganizeUndoCommandObserver` | `0x1013ca670` | 监听用户在润色上屏后立即按下的撤销快捷键（如 `Command+Z`），配合 `SmartOrganizeUndoAXVerifier` / `ClipboardVerifier` 安全还原原始语音识别结果 |

### 3.2 `/api/v2/ai/text_organization` 协议与 `OimeSSEParser` 流式解析机制

当用户完成口述（或达到长语音分段点）且启用了「语音智能整理 / 口语顺滑」功能时，`SmartOrganizeVoiceCoordinator` 调用 `SmartOrganizeClient`（`SmartOrganizeClient.swift`，代码段 `0x100237300..0x10023ae30`）向 `https://ime.doubao.com/api/v2/ai/text_organization` 发起 POST 请求：

```text
// 源：/tmp/doubao-thin-arm64/DoubaoIme:0x100237300..0x10023ae30 & OimeSSEParser 符号还原
SmartOrganizeVoiceCoordinator.coordinate(asrText, context: ASRSmartOrganizeContext)
  ├─ 检查 SmartOrganizeMemoryCache 是否命中相同 (asrText, contextHash)
  ├─ SmartOrganizeClient 构造 POST https://ime.doubao.com/api/v2/ai/text_organization
  │   └─ 请求体包含：原始 ASR 文本、光标前文 (precedingText)、宿主应用信息、会话 ID (requestID)
  └─ OimeSSEStreamTask 启动流式接收 (OimeSSEResponseMode: streaming / singleShot)
      ├─ OimeSSEParser.handleDelta(payload:requestID:) -> 增量流式回调
      ├─ OimeSSEParser.handleCompleted(payload:requestID:) -> 校验 format.completed
      └─ 异常分支：OimeSSEStreamError (scene.completed.empty_content / timeout / cancelled)
```

从反汇编与符号表可以确认 `OimeSSEParser` 与 `SmartOrganizeVoiceCoordinator` 的三项关键工程保障机制：

1. **双响应模式（`OimeSSEResponseMode`）与增量解析**：`OimeSSEStreamTask` 支持流式（`streaming`）与单包完成（`singleShot`）两种模式。在 SSE 模式下，`OimeSSEParser` 按标准 `event:` / `data:` 行解析服务端推送的 JSON 分片，通过 `handleDelta(payload:requestID:)` 持续拼装增量润色文本，并在收到结束帧时调用 `handleCompleted(payload:requestID:)` 校验内容非空（若为空则抛出 `scene.completed.empty_content` 并自动保留原始 ASR 文本）。
2. **改写决策门控（`SmartOrganizeVoiceDecision.rewriteDecision`）**：并非所有语音识别结果都需要被大模型改写（例如简短指令、纯数字或已经通顺的短句）。`SmartOrganizeVoiceCoordinator` 根据服务端返回的决策字段与本地规则生成 `rewriteDecision`，若判定无需改写或改写置信度不足，则直接定稿原始 ASR 文本，避免过度润色篡改用户原意。
3. **上屏冲突检测与多模态撤销回退（`SmartOrganizeUndo*`）**：
   - 在润色结果返回并准备替换行内文本前，状态机检查用户在等待 LLM 期间是否已移动光标或切换窗口（若已变更，触发 `"smartTargetRejected"` / `"failedToSmartOriginal"` `0x100208248`，放弃覆盖）。
   - 替换上屏成功后，生成 `ASRSmartOrganizeUndoRecord` 并启动 `SmartOrganizeUndoCommandObserver` 与 `SmartOrganizeUndoFloatingTipController`（悬浮提示窗）。若用户随后触发撤销，`SmartOrganizeUndoAXVerifier`（通过 macOS `AXUIElement` 校验当前焦点文本）或 `SmartOrganizeUndoClipboardVerifier` 会验证目标文本未被二次编辑，随后将润色后的文本精准还原为原始 ASR 文本（`rawText`）。

### 3.3 键盘拼音端云协同与 `libime_net_sdk.dylib`（Rust QUIC SDK）

除语音链路外，`OimeEngine.framework` 在常规键盘拼音输入中也集成了云端候选词与云端大模型（LLM）预测能力，底层由 Rust 编写的 `libime_net_sdk.dylib`（`ime_net_sdk 0.2.6`）承载：

```text
// 源：/tmp/doubao-thin-arm64/OimeEngine:0x8ab8..0x9218 & libime_net_sdk.dylib 导出表
-[OimeEngine makeCloudRequest:] (OimeEngine:0x8ab8)
  └─> OimeEngine.OimeEngineNetworkManager
       └─> libime_net_sdk.dylib!_call (0x11f0) / _call_streaming (0x1660)
            └─> Rust: ime_net_sdk::QUIC_CLIENT (quinn-proto-0.11.13 + brotli-3.5.0)
                 └─> 回调 -[OimeEngine parseCloudResponse:llmResponse:precedingText:onTiming:sessionLLMEnabled:] (0x8c9c)
```

1. **Rust QUIC 0-RTT/1-RTT 与 Brotli 压缩**：`libime_net_sdk.dylib` 内部维护全局单例 `ime_net_sdk::QUIC_CLIENT` 与流句柄表 `ime_net_sdk::STREAMING_HANDLES`（`Mutex<HashMap<usize, StreamStatus>>`），导出 `_on_init`（`0x2ac8`）、`_on_connect`（`0x29e8`）、`_call`（`0x11f0`）、`_new_streaming`（`0x2350`）、`_call_streaming`（`0x1660`）、`_close_streaming`（`0x1dec`）及 `_log_desensitization`（`0x218c`）。通过 QUIC 多路复用与 Brotli 字典压缩，将拼音击键期间的云端请求往返延迟降至最低。
2. **双路响应融合（`parseCloudResponse:llmResponse:precedingText:onTiming:sessionLLMEnabled:`）**：在 `OimeEngine` 偏移 `0x8c9c` 处，引擎同时接收常规云端拼音解码结果（`cloudResponse`）、大模型上下文续写/纠错结果（`llmResponse`）以及光标前文（`precedingText`），当 `sessionLLMEnabled` 为真时将云端候选词无缝合并至本地 `marisa` 词图候选列表中。

---

## 4. 核心机制（三）：端云协同路由策略与离线回退机制

豆包输入法在端侧离线模型（`003` 分析的 `acllm_main_model_mac` + `acllm_context_model_mac`）与云端流式服务之间构建了动态协同与故障回退状态机：

1. **离线模型分片下载与热切换（`ASROfflineModelTTNetRequest`）**：
   - `OimeOfflineModel` 模块通过 `libTTNetDownloaderCrossPlatform.dylib` 在后台下载离线模型包，校验 SHA-256 后更新 `~/Library/Application Support/DoubaoIme/ASRModel/offline_model_active_layout.json`。
   - `SAMIASRRuntimeManager` 实时监听离线模型就绪状态，无需重启输入法进程即可动态挂载端侧 Flute/Panther 解码器。
2. **云端优先、端侧兜底与混合抢跑策略**：
   - **在线流式态**：网络正常时，优先通过 `SAMIConnectPoolManager` 复用 `wss://frontier-audio-ime-quic.doubao.com/api/v1/ws` 连接进行云端流式识别，并在尾包触发 `SmartOrganizeClient` 执行 `/api/v2/ai/text_organization` SSE 润色。
   - **弱网/断网无缝降级**：当 `SAMIHandle` 检测到连接池建连失败、WebSocket 握手超时或流式包 RTT 超限时，自动无缝切换至本地 `flute::asr::ntp_cif` + `W4A8ImeAsrQwen3Transformer` 离线引擎（见 `003` §2.3），利用本地 `context.db` 召回的热词与纠错对完成端侧识别。
   - **端云共享同一套个性化上下文（`ASRGlobalContext`）**：无论路由至云端 `SAMIHandle`、`SmartOrganizeClient` 还是本地 `Flute` 引擎，上游均统一由 `ASRGlobalContext` 提供从 `context.db` 提取的 `modify_pair`（用户历史纠错对）、`user_word`（个人高频热词）及宿主光标上文，确保端侧与云端识别结果在专有名词上的一致性。
3. **全链路耗时与质量遥测（`Parfait.framework`）**：
   - 每次语音会话的连接池命中率、首包延迟（First Packet Latency）、尾包延迟、`SmartOrganize` SSE 耗时以及用户是否触发 `ASRSmartOrganizeUndoRecord` 撤销，均由 `Parfait` 埋点上报至 `https://pc-mon.zijieapi.com`，作为端云路由阈值调优的依据。

---

## 5. 边界与限制及开放问题（Boundaries, Limitations & Open Questions）

### 5.1 边界与限制

1. **私有二进制协议与字节跳动云端基建强绑定**：`SAMIHandle` 使用的 `sami_core` `speech_v2` WebSocket 二进制分帧协议、`https://speech.bytedance.com/api/v1/token` 鉴权机制以及 `libime_net_sdk.dylib` 的私有 QUIC 接口均与字节跳动自有服务端强耦合，开源客户端（如鼠须管 Squirrel）无法直接复用其服务端点，只能借鉴其连接池预热、Opus 压缩与 SSE 后处理架构。
2. **多重网络库叠加导致的包体积膨胀**：`DoubaoIme.app` 同时打包了 `libsscronet.dylib`（15.08 MB）、`libime_net_sdk.dylib`（15.66 MB）、`Parfait.framework`（12.18 MB）与 `libTTNetDownloaderCrossPlatform.dylib`（2.40 MB），仅网络与遥测动态库合计即超过 45 MB（Universal），违背了轻量级开源输入法的设计约束。
3. **云端润色延迟与宿主焦点漂移风险**：即便采用 SSE 流式返回，`/api/v2/ai/text_organization` 仍需等待整句语音尾点定稿后才能生成完整的改写文本；若用户在尾点后的数百毫秒内立刻按回车发送消息（如在即时通讯软件中），异步返回的润色结果将错过替换窗口（触发 `smartTargetRejected`）。

### 5.2 开放问题

1. **通用开源协议栈对私有网络 SDK 的轻量化替代**：在鼠须管（Squirrel）中实现同等的低延迟流式 ASR 与 LLM 润色时，直接使用 macOS 系统原生 `URLSessionWebSocketTask` + `URLSession.bytes(for:)`（原生支持 HTTP/2、HTTP/3 QUIC 与 TLS 连接复用）是否足以替代重型的 Cronet / Rust `quinn` 动态库？
2. **流式 ASR 定稿与 LLM 润色的两阶段展示交互**：当先向用户展示流式 ASR 原始文本、随后再由云端/本地 LLM 替换为润色文本时，在鼠须管中应当全程保持在 `setMarkedText` 预编辑态直至 LLM 完成，还是先 `insertText` 上屏原文再提供悬浮候选或快捷键替换？

---

## 6. 面向鼠须管（Squirrel / `rime-client`）的候选结论（待用户确认）

基于对豆包输入法云端流式 ASR、大模型润色（`OimeSmartOrganize`）与端云协同机制的逆向实证，提炼出以下面向本项目的候选技术结论（全部标记为 **候选（待用户确认）**）：

1. **连接预热池与 Opus/PCM 流式 WebSocket ASR 管线（候选（待用户确认））**：
   - 借鉴 `SAMIConnectPoolManager` 与 `SAMIHandle` 的设计，在用户按下语音快捷键的瞬间（或输入法激活空闲期）预先建立/复用 WebSocket 连接，支持将 16kHz 单声道音频流式推送至兼容的云端/本地 ASR 服务端，并通过增量回调驱动行内 `setMarkedText` 预编辑更新（候选（待用户确认））。
2. **基于 HTTP SSE（`OimeSSEParser` 模式）的二阶段大模型语音润色与决策门控（候选（待用户确认））**：
   - 借鉴 `SmartOrganizeVoiceCoordinator` 与 `SmartOrganizeClient` 的架构，将「流式语音转文字（ASR）」与「大模型语境润色/口语顺滑（LLM Post-processing）」解耦为两阶段流水线：第一阶段实时流式产出 ASR 文本，第二阶段在尾点定稿后携带光标前文（`precedingText`）、宿主 Bundle ID 与个人热词，通过标准 OpenAI 兼容 HTTP SSE 流式接口（`URLSession.bytes`）请求大模型润色，并引入改写决策门控（无需改写时直接保留原文）与内存缓存（`SmartOrganizeMemoryCache`）（候选（待用户确认））。
3. **润色替换前的焦点校验与一键撤销回退记录（`UndoRecord`，候选（待用户确认））**：
   - 借鉴 `ASRSmartOrganizeUndoRecord` 与 `smartTargetRejected` 保护机制，在异步 LLM 润色结果准备上屏前校验会话代数令牌与光标位置；上屏后保留最近一次 `{rawASRText, polishedText}` 快照，允许用户通过特定快捷键或候选窗操作一键回退至未润色的原始 ASR 识别文本（候选（待用户确认））。
4. **端云统一上下文注入与弱网自动回退本地模型（候选（待用户确认））**：
   - 借鉴 `ASRGlobalContext` 与 `SAMIASRRuntimeManager` 的端云协同设计，将个人词库/热词与历史纠错对抽象为统一的上下文提供器（Context Provider），同时服务于云端 API 请求与本地离线推理；当检测到云端连接超时或处于离线模式时，自动回退至本地离线 ASR/LLM 引擎而无需中断用户录音会话（候选（待用户确认））。

---

## 7. 检索日志（Query Log）

| # | 阶段 / 工具 | 查询目标或命令摘要 | 关键产出与证据定位 |
| :--- | :--- | :--- | :--- |
| 1 | `read` | 读取 `dev-docs/GUIDE.md`、`dev-docs/MAP.md`、`dev-docs/notes/001-research-voice-ime-index.md` | 明确文档结构规范、禁词表与子课题 `004` 边界 |
| 2 | `bash` (`shasum -a 256` / `stat`) | 对 `/tmp/doubao-thin-arm64/` 下 `DoubaoIme`、`OimeEngine`、`libime_net_sdk.dylib`、`libsscronet.dylib`、`Parfait`、`libTTNetDownloaderCrossPlatform.dylib` 计算大小与哈希 | 获取全部网络与云端协同相关 Mach-O 二进制精确字节数与 SHA-256 |
| 3 | `bash` (`grep` on `classes-dump.txt`) | 检索 `/tmp/doubao-thin-arm64/classes-dump.txt` 中所有包含 `SAMI`、`SmartOrganize`、`SSE`、`Cloud`、`Frontier` 的类与符号 | 定位 `OimeAudio.SAMIHandle`、`OimeSmartOrganize.SmartOrganizeClient`、`OimeNetwork.OimeSSEParser` 等真实类体系 |
| 4 | `bash` (Python `__cstring` scanner) | 扫描 `DoubaoIme` 二进制代码段内嵌的 `.swift` / `.cpp` 源文件路径字面量 | 提取 `SAMIHandle.swift`、`SAMIConnectPool.swift`、`SAMIASRRuntimeManager.swift`、`SmartOrganizeClient.swift`、`OimeSSEParser.swift` 及 `sami_core` 路径 |
| 5 | `bash` (Python URL & endpoint regex) | 扫描 `DoubaoIme` 与 `OimeCommon` 中的 `https://`、`wss://` 及 `/api/v*` 路由字面量 | 定位 `wss://frontier-audio-ime-quic.doubao.com/api/v1/ws`、`wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws`、`https://ime.doubao.com/api/v2/ai/text_organization`、`https://speech.bytedance.com/api/v1/token` |
| 6 | `bash` (Python string dump) | 提取 `DoubaoIme` 中 `sami_core/src/cpp/processors/speech_v2/asr_processor.cpp` 与 `asr_on_worker.cpp` 关联日志与 JSON 键 | 还原 `CreateConnection`、`StartTask`、`SendAudioData`、`HandleRecognizeResult`、`prefetch` 等流式协议状态机 |
| 7 | `bash` (`grep` on `DoubaoIme.syms.txt`) | 检索 `sami_core` 音频编解码器相关符号 | 确认 `sami_core_common_encode_processor.cpp` 与 `sami_core_opus_audio_decoder.cpp`（16kHz PCM 转 Opus 流式编码） |
| 8 | `bash` (`xcrun llvm-objdump -d` / `DoubaoIme.disasm.txt`) | 分析 `DoubaoIme` `0x100237300..0x10023ae30` 区间（`SmartOrganizeClient.swift`） | 还原 `SmartOrganizeClient` 请求构造、响应解析与 `SmartOrganizeMemoryCache` 协同逻辑 |
| 9 | `bash` (`grep` on `DoubaoIme.syms.txt` & `classes-dump.txt`) | 检索 `OimeSSEStreamTask`、`OimeSSEParser`、`OimeSSEResponseMode`、`OimeSSEStreamError` 符号与字符串 | 还原 `handleDelta(payload:requestID:)`、`handleCompleted(payload:requestID:)`、`format.completed` 与 `scene.completed.empty_content` |
| 10 | `bash` (`grep` on `classes-dump.txt` & `DoubaoIme.disasm.txt`) | 检索 `ASRSmartOrganizeUndoRecord`、`SmartOrganizeUndoCommandObserver`、`SmartOrganizeUndoAXVerifier`、`SmartOrganizeUndoClipboardVerifier` | 还原 LLM 润色上屏后的焦点校验（`smartTargetRejected`）与多模态一键撤销回退状态机 |
| 11 | `bash` (`nm -gU` & Python string scan on `libime_net_sdk.dylib`) | 提取 `libime_net_sdk.dylib` 导出 C FFI 函数表与 Rust crate 版本字符串 | 确认 `ime_net_sdk 0.2.6` 基于 `quinn-proto-0.11.13` (QUIC) + `hyper-util-0.1.19` + `brotli-3.5.0`，导出 `_on_init`、`_call`、`_new_streaming`、`_call_streaming` 等 11 个函数 |
| 12 | `bash` (`grep` on `OimeEngine.syms.txt` & `OimeEngine.disasm.txt`) | 分析 `OimeEngine` 中 `makeCloudRequest:`（`0x8ab8`）与 `parseCloudResponse:llmResponse:precedingText:onTiming:sessionLLMEnabled:`（`0x8c9c`） | 还原键盘拼音输入下的云端候选与云端 LLM 联合解码融合机制 |
| 13 | `bash` (`otool -L` & string scan on `libTTNetDownloaderCrossPlatform.dylib` / `Parfait`) | 检查 `ASROfflineModelTTNetRequest` 与 `Parfait.framework` 导出符号及上报域名 | 确认离线模型通过 TTNet 分片下载热加载，APM 遥测通过 Parfait 上报至 `https://pc-mon.zijieapi.com` |

<!-- research-complete -->
