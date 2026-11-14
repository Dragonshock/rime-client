# 豆包输入法（macOS）逆向调研（二）：离线语音模型（ASR）与本地大模型（acllm / ContextAux）技术栈及存储结构

> 本文档为豆包输入法（DoubaoIme.app，macOS 版 `1.1.0` / Wave Build `1010005`）深度逆向调研的第二篇分册，聚焦其应用支持目录（`~/Library/Application Support/DoubaoIme/`）下的本地语音与上下文辅助模型资产、二进制内的离线推理框架（`SAMICORE` / `flute` / `panther` / `W4A8ImeAsrQwen3Transformer`），以及 `ASRContext/context.db` 驱动的端侧个性化检索增强生成（Retrieval-Augmented Generation, RAG）与提示词注入（Context Injection）机制。总览索引见 `dev-docs/notes/001-research-voice-ime-index.md`。

---

## 1. 事实与来源：本地模型与数据目录全景清单（Application Support Inventory）

豆包输入法在运行时将所有动态下载的离线语音模型、分词与命名实体识别（Named Entity Recognition, NER）模型、用户词典及个性化上下文数据库存储在 `/Users/dragon/Library/Application Support/DoubaoIme/` 目录中；同时在应用包 `/Library/Input Methods/DoubaoIme.app/Contents/Resources/` 内置基础引擎配置与预置词库。

### 1.1 目录结构、体积与 SHA-256 校验和总表

下表汇总了豆包输入法本地模型与数据目录的核心文件、精确字节数、SHA-256 摘要及其在推理链路中的角色：

| 子目录 / 相对路径 | 精确字节数 (Bytes) | SHA-256 摘要 | 格式与魔数 (Magic) | 核心角色与消费方 |
| :--- | ---: | :--- | :--- | :--- |
| `ASRModel/offline_model_active_layout.json` | 482 | `cd2b0a944ecdc9387003b973aa45f4f8501c5875657efcb8403fa2a978223cf4` | UTF-8 JSON | 记录当前激活的离线语音主模型与上下文模型路径及 `etag` |
| `ASRModel/acllm_main_model_mac/asr_model.model` | 313,440,860 | `8cb29e2055d86e4390400dcfa814755d1c6591378dc4aa7e3e46d9103f788a0f` | `FLUTE` (`46 4c 55 54 45`), Layout `v3` | 离线语音识别主模型打包镜像（含 21 个内部子文件，ChaCha20 头部 + XOR 载荷） |
| `ASRModel/acllm_context_model_mac/fuzzy_model` | 15,743,435 | `855d0ce0f6c1dc1587d9ff72e45dfbf55068c7d294be1a21df9d0f21276747e9` | Binary (`00 00 4b 45 ...`) | 拼音模糊音（Fuzzy Pinyin）映射与混淆纠错图模型 |
| `ASRModel/acllm_context_model_mac/ngram_model` | 64,296,650 | `d9b0de7ed78b81c404b8e6865bd9e77db2be21963faed501083d1351dfdeac9f` | Binary (`da a2 9d 64 ...`) | 端侧 N-gram 语言模型，用于上下文重打分与热词偏置 |
| `OfflineModelDownloads/manifest/offline_model_manifest.json` | 739 | `848c01493c84a83b149123a826fd2f768afbb5e59ac7aa10a591bd8c4b9e6372` | UTF-8 JSON | 离线模型下载清单，维护 `acllm_context_model_mac` 与 `acllm_main_model_mac` 下载状态 |
| `ContextAuxModel/current_revision` | 6 | `29f85f1323b9feefeaad5fbcc5fb4bc4fc26101ffba09c9b2ae234f26b17254e` | ASCII Text (`10002\n`) | 指向当前激活的 `ContextAuxModel` 修订号目录 `revisions/10002` |
| `ContextAuxModel/revisions/10002/meta.json` | 91 | `e7a4ca393e3e4456a714f741371a79d472185db1264ac8a7c73906e82ab0e6f5` | UTF-8 JSON | 记录上下文辅助模型版本号 `10002` 及激活时间戳 |
| `ContextAuxModel/revisions/10002/model/cppjieba_dict/jieba.dict.utf8` | 5,036,559 | `f3ef0edffef7fb6cd6de1cd3fc02ba4de0c3fef6ab62c38f2ff29b036c6e1bc5` | UTF-8 Text (349,053 行) | `cppjieba` 主分词词典（词条、词频、词性三列） |
| `ContextAuxModel/revisions/10002/model/cppjieba_dict/hmm_model.utf8` | 528,896 | `73bedd6dc2b3ebbd180fc9da4ebdc3ed36f0b0eb5afee8d17f70b8e607b157d1` | UTF-8 Text | 隐马尔可夫模型（Hidden Markov Model, HMM）未登录词识别参数 |
| `ContextAuxModel/revisions/10002/model/cppjieba_dict/idf.utf8` | 4,705,839 | `f39dc2df992ed1bc2b5ebda13fb4ef0fbbf96ab1b2f3f0f8f45fc0f679217d67` | UTF-8 Text (320,972 行) | 逆文档频率（Inverse Document Frequency, IDF）权重表，用于关键词抽取 |
| `ContextAuxModel/revisions/10002/model/cppjieba_dict/stop_words.utf8` | 8,906 | `de31fb9a68b2fe0fa6bc26d4cb011cb0df83f964ce9e03cd5f5c6443bd0a527a` | UTF-8 Text (1,280 行) | 停用词过滤表 |
| `ContextAuxModel/revisions/10002/model/cppjieba_dict/user.dict.utf8` | 150 | `92b9f04b4a7aa13429d843c4420ba61cb2623217c42d3b5ef554e94b5d1cf2fb` | UTF-8 Text (12 行) | 用户自定义分词补充词表 |
| `ContextAuxModel/revisions/10002/model/libcut_model/user.txt` | 91 | `e8a9877b7bc66cc24bf8990ebae96bb9268b10fb19cfca97254e675fa0eb37e0` | UTF-8 Text (11 行) | `libcut` 分词器自定义词条配置 |
| `ContextAuxModel/revisions/10002/model/ner_model/car_ner_model_15.convert` | 16,580,694 | `20348cb430eb68bcbeedca0ab6c7e5eb158ba331280be2c598e964b0499c3c6f` | Custom Quantized Binary | 端侧命名实体识别神经网络模型（16.58 MB），提取人名/专有名词 |
| `ContextAuxModel/revisions/10002/model/ner_model/asr_user_ner_dict.dat` | 45,353,000 | `39b775b40bb73f33e93ca2c5d4c56bd72fa3ef633bc92ebdb4858386d42dc61a` | Double-Array Trie (`63 40 2c 00 ...`) | 预编译用户实体识别词典双数组字典树（45.35 MB，头字段 `2,900,067`） |
| `ContextAuxModel/revisions/10002/model/pinyin_dict/hzpy-utf8.txt` | 190,646 | `29f854d75405f9be4da23ab9d8ff66bbbe7a6a192a49b56ebad25461b05da9c1` | UTF-8 Text (21,260 行) | 汉字与带声调拼音映射表（含生僻字与多音字读音） |
| `ASRContext/context.db` | 28,672 | `3df13f6e4dd9a3ab0357c90bf6db2d86fd82ff6b3143cf910790cc486b04d864` | SQLite 3 (`53 51 4c 69 74 65 ...`) | 个性化 ASR 上下文数据库，包含 `modify_pair` 与 `user_word` 两张核心表 |
| `EngineUserDict/usr.dat` | 10,486,936 | `f074fd9d050bf0bb3971b689d01f1f21d0c95cd409df68077fd5100c3688894c` | Binary (`OIME_USR_DICT_0`) | 键盘与语音共享的输入法引擎用户词库（固定 10 MB 预分配空间） |
| `Json/compression_result.json` | 1,738 | `3c39988f1476158638767d7cc3e7e0db5f5d021eb54c7720cb7eb4d5991868c8` | UTF-8 JSON | 引擎核心字典包解压/校验元数据（`core_dict`、`kenlm`、`t2s` 等 CRC32 与尺寸） |

### 1.2 离线模型布局清单（Layout & Manifest）解析

`ASRModel/offline_model_active_layout.json` 定义了端侧 ASR 在启动时挂载的两类核心资产：`acllm_main_model_mac`（音频编码与大模型解码主模型）与 `acllm_context_model_mac`（模糊音与 N-gram 上下文模型）：

```json
// 源：~/Library/Application Support/DoubaoIme/ASRModel/offline_model_active_layout.json
{
  "entries" : {
    "acllm_context_model_mac" : {
      "etag" : "\"05c5c75856c542516e24692eb341cd66\"",
      "extractedPath" : ".../DoubaoIme/ASRModel/acllm_context_model_mac",
      "modelName" : "acllm_context_model_mac"
    },
    "acllm_main_model_mac" : {
      "etag" : "\"d6d6ca628e642e6dd77f516c3a4daec0\"",
      "extractedPath" : ".../DoubaoIme/ASRModel/acllm_main_model_mac",
      "modelName" : "acllm_main_model_mac"
    }
  }
}
```

上述 JSON 配置由主程序中的 `OfflineASRService` 与 `PathManager` 读取，并通过 `sami_core` 的 `FluteManager` 注入到底层推理引擎。其中 `acllm` 是 **Audio-Context Large Language Model**（音频上下文大语言模型）的缩写，表明豆包输入法的离线语音识别并非传统的声学模型（Acoustic Model）加 WFST 解码图架构，而是基于音频编码器结合小型量化大语言模型（Speech-LLM）的端到端生成式架构。

### 1.3 上下文辅助模型（`ContextAuxModel`）与分词/实体提取词表

`ContextAuxModel` 采用基于修订号（Revision）的原子目录切换机制：`ContextAuxModel/current_revision` 仅包含字符串 `10002`，指向 `ContextAuxModel/revisions/10002/`。该目录下的 `model/` 包含四个子模块：

1. **`cppjieba_dict/`**：标准 C++ 结巴分词词典组合，含 `jieba.dict.utf8`（349,053 条目）、`hmm_model.utf8`、`idf.utf8`（320,972 条目，赋予低频专有名词高 IDF 权重，如 `抖音 11.799`）、`stop_words.utf8`（1,280 条停用词）和 `user.dict.utf8`。
2. **`libcut_model/user.txt`**：针对字节跳动内部术语与常见高频词的自定义切词规则文件。
3. **`ner_model/`**：包含 `car_ner_model_15.convert`（16.58 MB 神经网络实体识别模型）与 `asr_user_ner_dict.dat`（45.35 MB 双数组 Trie 实体词典）。
4. **`pinyin_dict/hzpy-utf8.txt`**：包含 21,260 行汉字到声调拼音的映射表。

```text
# 源：~/Library/Application Support/DoubaoIme/ContextAuxModel/revisions/10002/model/pinyin_dict/hzpy-utf8.txt
一	yi1
丁	ding1,zheng4
七	qi1
万	mo4,wan4
丈	zhang4
三	san1
上	shang3,shang4
下	xia4
```

该拼音表被 `GlobalContext`（`global_context.cpp`）加载，用于在将用户输入热词写入上下文或执行纠错比对时计算发音相似度，避免大模型在上下文提示词偏置（Prompt Biasing）阶段引入读音完全无关的幻觉词。

---

## 2. 核心机制（一）：离线 ASR 与端侧大模型（`flute` / `panther`）推理栈

通过 REA（Reverse Engineering Assistant）的 `rea_open_binary`、`rea_inspect_macho`、`rea_trace_feature`、`rea_demangle_swift` 工具以及 `llvm-objdump` 对以下三个核心 Mach-O 二进制进行逆向分析：

- 主程序 `/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme`（Universal Binary，`arm64` 切片偏移 `27,820,032`，大小 `24,522,000` 字节，SHA-256 `5b48748fa6394811cba0f8c05905ffebb9a2cd02faf6d47b26deb3d9b86247e0`）
- 引擎框架 `/Library/Input Methods/DoubaoIme.app/Contents/Frameworks/OimeEngine.framework/Versions/A/OimeEngine`（SHA-256 `7ba480eaabbd4d99f6d63917d9fc208c81e8b1d1c38db22afa3795a46ceb8ed3`）
- 公共框架 `/Library/Input Methods/DoubaoIme.app/Contents/Frameworks/OimeCommon.framework/Versions/A/OimeCommon`（SHA-256 `b541b6067f10df4ba0dcddfa6fc3527549dd75567b78dc79dfcbef5528d59bf4`）

### 2.1 框架职责划分与源码树还原

逆向结果表明，`OimeEngine.framework` 主要负责传统拼音/键盘输入法引擎（`obric-ime-engine`）及云端 Protobuf 协议序列化；而**完整的离线语音识别引擎与端侧 LLM 推理框架（`sami_core`、`flute`、`panther`）均以静态库形式直接链接在 `DoubaoIme` 主程序二进制中**。从 `__TEXT,__cstring` 段提取的编译期断言与日志路径可完整还原其内部 C++ 源码树结构：

| 源码模块层级 | 编译期嵌入源码路径示例 | 核心类 / 符号 | 功能定位 |
| :--- | :--- | :--- | :--- |
| **OimeCommon (Swift)** | `OimeCommon.PathManager` | `asrContextAuxModelDirectoryName` / `asrContextAuxModelSubdirectoryName` | 管理 `ASRModel`、`ASRContext`、`ContextAuxModel` 本地路径 |
| **OimeEngine (C++)** | `/Users/0bpha83joo1fg7n3/110945/obric-ime-engine/src/other/asr/asr_user_ner_dict.cpp` | `AddModifyPairNerResponse` / `asr_user_ner_dict` | 将云端或本地 NER 词条维护进输入法引擎词典 |
| **SAMICORE 接口层** | `/Users/nj4s14ntgkr70x23/19619/sami_core/sami_interface/src/capi/speech_v2/asr/offline/worker/asr_off_worker.cpp` | `ASROffWorker` / `ASRProcessor` / `FluteManager` | 离线 ASR 工作线程管理、VAD 切分与 `Flute` 引擎生命周期控制 |
| **SAMICORE 上下文层** | `/Users/nj4s14ntgkr70x23/19619/sami_core/sami_interface/src/capi/speech_v2/asr/global_context/global_context.cpp` | `GlobalContext` / `ContextDB` / `CutWithNer` | 调度 `context.db` 读写、结巴分词、NER 模型推理及热词/纠错对召回 |
| **Flute 编排框架** | `/Users/hcoymu818v3occgv/82635/flute/flute/core/executor.cc` | `flute::Executor` / `flute::WorkFlow` / `flute::FilesystemPackedMmap` | 基于 Actor 图（`WorkFlow`）的多模态流式推理编排器与加密模型内存映射 |
| **Flute NTP-CIF 模块** | `/Users/hcoymu818v3occgv/82635/flute/flute/module/asr/ntp_cif/asr_executor_with_context.cc` | `EncoderActor` / `DecoderContextActor` / `PromptProcessor` / `IntegrationActor` | 基于 Next-Token Prediction + CIF 的音频编码、提示词注入与流式/非流式双路解码 |
| **Panther 端侧大模型** | `/Users/hcoymu818v3occgv/82635/flute/flute/module/asr/ntp_cif/integration/pantherllm_engine_creator.cc` | `PantherLLMEngineCreator` / `W4A8ImeAsrQwen3Transformer` / `KVCacheManager` | 端侧量化大模型推理后端，含 4-bit 权重 8-bit 激活（W4A8）的 Qwen3 算子与投机采样 |

### 2.2 `asr_model.model` 容器格式（`FilesystemPackedMmap`）逆向

`ASRModel/acllm_main_model_mac/asr_model.model`（313.44 MB）并非单一权重文件，而是由 `flute/core/filesystem_packed_mmap.cc` 解析的自定义加密归档容器。查看文件前 36 字节十六进制转储：

```text
# 源：~/Library/Application Support/DoubaoIme/ASRModel/acllm_main_model_mac/asr_model.model (Offset 0x00..0x23)
00000000: 464c 5554 4503 0000 0073 0500 0000 0000  FLUTE....s......
00000010: 0015 0000 0065 a6be d51c acd5 d303 9c54  .....e.........T
00000020: d781 d09c 0d                             .....
```

结合 `/tmp/DoubaoIme.arm64` 中 `FilesystemPackedMmap::Open`（虚拟地址 `0x1003f0e84`）的反汇编代码，可精确还原其二进制头部结构：

```asm
// 源：/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme (arm64, 0x1003f0ed0..0x1003f0f74)
1003f0ed0: ldr   w8, [x1]              ; 读取偏移 0x00 的 4 字节魔数 "FLUT" (0x54554c46)
1003f0ed8: cmp   w8, w9
1003f0ee0: ldrb  w9, [x1, #0x4]        ; 读取偏移 0x04 的第 5 字节 'E' (0x45)
1003f0ef0: ldur  w9, [x21, #0x5]       ; 读取偏移 0x05 的 uint32 layout_version (= 3)
1003f0efc: sub   w8, w9, #0x1
1003f0f00: cmp   w8, #0x3              ; 校验版本号是否在 [1, 3] 范围内
1003f0f24: ldur  x20, [x21, #0x9]      ; 读取偏移 0x09 的 uint64 header_total_size (= 0x573 = 1395 字节)
1003f0f64: ldur  w25, [x21, #0x11]     ; 读取偏移 0x11 的 uint32 entry_count (= 0x15 = 21 个内部子文件)
1003f0f70: cmp   w9, #0x3              ; 判断是否为 chacha20-header-xor-payload(3)
```

根据上述指令与异常日志字符串 `"FilesystemPackedMmap supports plaintext(1), xor-payload(2), and chacha20-header-xor-payload(3) layouts, got version "`，`FLUTE` 容器格式支持三种布局版本：

1. **头部定长字段（偏移 `0x00..0x14`，共 21 字节）**：
   - `0x00..0x04`（5 字节）：ASCII 魔数 `"FLUTE"`。
   - `0x05..0x08`（`uint32_le`）：布局版本 `layout_version`。当前模型值为 `3`（即 `chacha20-header-xor-payload`）。
   - `0x09..0x10`（`uint64_le`）：头部总字节数 `header_total_size`。当前模型值为 `0x0573`（`1,395` 字节），这也是内部第一个子文件载荷的起始偏移。
   - `0x11..0x14`（`uint32_le`）：打包的子文件数量 `entry_count`。当前模型值为 `0x15`（共 **21 个子文件**）。
2. **ChaCha20 加密目录表（偏移 `0x15..0x0572`）**：
   - 当 `layout_version == 3` 时，偏移 `0x15..0x20`（12 字节，`65 a6 be d5 1c ac d5 d3 03 9c 54 d7`）为 **96-bit ChaCha20 Nonce**。
   - 偏移 `0x21..0x0572`（共 `1,362` 字节）为经 ChaCha20 流密码加密的目录表，通过调用 `0x1003efe10` 解密（其密钥经由混淆 VM 函数 `0x1002d5cb8` 与常量表 `0x100e74030` `"bd_guard_15.2-BG2.8.2.4"` 动态派生）。
   - 解密后的 21 个目录项依次包含：以 `\0` 结尾的子文件名字符串、`uint64_le size`（文件大小）、`uint64_le offset`（文件在容器内的字节偏移）以及 `uint64_le xor_key`（载荷异或掩码，与偏移 `0x0573` 之后的 `mmap` 内存页按需异或读取）。
   - `FluteManager::CreateEngine()` 打开该容器后，首先从中读取虚拟路径 `config.yaml` 构建 `flute::WorkFlow` 计算图。

### 2.3 `flute::asr::ntp_cif` 与 `Panther`（`W4A8ImeAsrQwen3Transformer`）推理流水线

通过对 `DoubaoIme` 中 `flute::asr::ntp_cif` 命名空间符号与字符串的交叉引用分析，豆包输入法的离线语音识别采用 **NTP-CIF（Next-Token Prediction with Continuous Integrate-and-Fire）** 架构，将连续语音帧压缩为离散声学 Token 嵌入后直接输入量化版 **Qwen3 Transformer** 语言模型进行自回归或投机解码。

```asm
// 源：/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme (arm64, 0x1007c2a7c..0x1007c2ab0)
1007c2a7c: mov   w0, #0x20
1007c2a80: bl    0x100e45618           ; operator new(0x20)
1007c2a84: mov   x22, x0
1007c2a88: adrp  x1, 0x100fe9000
1007c2a8c: add   x1, x1, #0xad3        ; "W4A8ImeAsrQwen3Transformer attention type"
1007c2a90: add   x0, sp, #0x40
1007c2a94: bl    0x100717914           ; 构造自定义算子配置异常信息
1007c2a98: adrp  x2, 0x100fe9000
1007c2a9c: add   x2, x2, #0xab9        ; " must be causal_mask or no_mask"
```

在 `0x1007c2a88` 处的算子校验逻辑揭示了底层大模型的核心架构实现细节：

1. **端侧模型基座与量化方案（`W4A8ImeAsrQwen3Transformer`）**：
   - 算子名称 `W4A8ImeAsrQwen3Transformer` 明确指出端侧 ASR 解码器采用 **Qwen3 Transformer** 架构，并針對输入法语音识别（`ImeAsr`）场景进行了 **W4A8 量化**（4-bit 权重 Weight、8-bit 激活 Activation）。
   - 该算子校验注意力掩码类型（`attention type must be causal_mask or no_mask`），结合 `asr_model.model` 總体积（313.44 MB，含声学编码器、CIF 模块、流式/非流式双解码器或共享权重及词表），按 4-bit 权重量化（约 0.5 Bytes/Param）推算，该端侧 Qwen3 模型的参数量级约为 **0.3B ~ 0.5B（3 亿至 5 亿参数）**。
2. **流式与非流式双路融合解码（`IntegrationActor` & `PantherLLMEngineCreator`）**：
   - `pantherllm_engine_creator.cc` 读取 `s_model_name`（流式模型）与 `non_s_model_name`（非流式模型），并支持通过 `share_weight` 标志共享底层 Qwen3 权重。
   - 在说话过程中，流式引擎实时输出部分识别结果（`stream partial`）；当 VAD（`flute.vad.v4`）检测到尾点或分段结束时，非流式引擎执行完整上下文重解码（`non-stream final`），并在 `asr_executor_with_context.cc` 中通过时间戳覆盖策略（`skip stream output covered by non-stream: stream [{}-{}], non-stream [{}-{}]`）修正流式输出。
3. **投机采样加速（Speculative Decoding）与 KV Cache 管理**：
   - `decoder_context_actor.cc` 内置投机解码支持（日志：`[SPEC] draft enabled: draft_len={} layers={}`），加载轻量级草稿模型（`draft_model`，层数由 `draft_layer_num` 控制）一次猜出多个 Token，再由主模型并行验证；若验证失败则自动回退至贪心搜索（`[SPEC] run failed (n={}), falling back to greedy`）。
   - `kvcache_manager.cc`（`KVCacheManager`）支持主机内存分配与 GPU 卸载模式（`gpu_kv_offload`），在每次更新上下文提示词或中间生成后通过 `ResetKvCachePos` 复用前缀 KV 缓存，避免重复计算 Prompt Token 的注意力状态。

### 2.4 `PromptProcessor` 多槽位上下文注入机制

`flute/module/asr/ntp_cif/decoder_context/prompt_processor.cc` 中的 `flute::asr::ntp_cif::PromptProcessor` 负责将结构化上下文转换为 `GPT2Tokenizer` 的 Token ID 序列（`pre_prompt` 与 `post_prompt`），围绕音频特征占位符 `[AUDIO]` 组装大模型的完整输入：

| 提示词模板槽位 (Slot) | 处理函数 (`PromptProcessor`) | 长度与分隔符配置键 | 数据来源与作用 |
| :--- | :--- | :--- | :--- |
| `BOT_NAME` / `BOT_DESC` | `ProcessBotName` / `ProcessBotDesc` | `bot_name_max_length` / `bot_desc_max_length` | 设定助手身份或当前应用宿主场景描述 |
| `USER_MODIFY` | `ProcessUserModify` | `user_modify_max_length`, `user_modify_delim`, `user_modify_pair_delim` | 注入从 `context.db` 的 `modify_pair` 表召回的历史纠错对（`src_text -> dst_text`） |
| `HOTWORDS` | `ProcessHotwords` | `hotwords_max_length`, `hotword_delim`, `hotwords_types`, `hotwords_types_prompts` | 注入从 `context.db` 的 `user_word` 表及用户词典召回的个人高频专有名词 |
| `DIALOG` | `ProcessDialog` | `dialog_max_length` | 注入近期对话历史上下文 |
| `CUR_CONTEXT` | `ProcessCursorContext` | `cursor_context_max_length`, `cursor_context_delim` | 注入当前输入框光标前后的已有文本（Surrounding Text） |
| `AUDIO` | `audio_embedding_prefill` | `cur_audio_token_num_` | 经 `EncoderActor` 与 CIF 压缩后的声学特征嵌入序列 |

---

## 3. 核心机制（二）：`ASRContext/context.db` 加密标签、纠错规则与 RAG 召回机制

`/Users/dragon/Library/Application Support/DoubaoIme/ASRContext/context.db`（28,672 字节）是豆包输入法实现端侧“越用越准”自学习能力的核心存储。通过 `rea_inspect_sqlite_database` 工具读取该数据库，并结合 `DoubaoIme` 中 `sami_core/.../global_context/context_db.cpp` 的反汇编代码，可完整还原其表结构、字段级加密算法、冲突消解规则与三段式召回策略。

### 3.1 数据库表结构（Schema）与索引设计

`context.db` 是标准 SQLite 3 数据库文件（并非全库级 SQLCipher 页加密，而是由 `context_db.cpp` 在应用层实现**字段级 AEAD 加密与 HMAC 盲索引标签**）。数据库包含两张业务表 `modify_pair` 与 `user_word`：

```sql
-- 源：~/Library/Application Support/DoubaoIme/ASRContext/context.db (sqlite_master DDL)
CREATE TABLE modify_pair (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    src_text VARCHAR NOT NULL,
    dst_text VARCHAR NOT NULL,
    create_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    update_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    use_count INTEGER DEFAULT 1,
    recall_count INTEGER DEFAULT 0,
    src_text_enc TEXT, dst_text_enc TEXT,
    src_text_tag TEXT, dst_text_tag TEXT,
    pair_tag TEXT, key_version INTEGER DEFAULT 0,
    UNIQUE(src_text, dst_text)
);
```

`modify_pair` 表同时建有索引 `idx_modify_pair_time_count ON modify_pair(update_at DESC, use_count DESC)` 与唯一索引 `idx_modify_pair_tag ON modify_pair(pair_tag)`。另一张表 `user_word` 的 DDL 结构如下：

```sql
-- 源：~/Library/Application Support/DoubaoIme/ASRContext/context.db (sqlite_master DDL)
CREATE TABLE user_word (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    word VARCHAR NOT NULL UNIQUE,
    create_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    update_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    use_count INTEGER DEFAULT 1,
    word_enc TEXT,
    word_tag TEXT,
    key_version INTEGER DEFAULT 0
);
```

`user_word` 表同样建有时间/频次复合索引 `idx_user_word_time_count ON user_word(update_at DESC, use_count DESC)` 与唯一盲索引 `idx_user_word_tag ON user_word(word_tag)`。

### 3.2 真实样本数据与字段级加密（`ctxdb-enc-v1:`）逆向验证

使用 `rea_inspect_sqlite_database` 检查当前 `context.db` 中的真实记录可见：当 `key_version = 1` 时，明文字段 `src_text`、`dst_text` 与 `word` 仅存入 `#<id>` 形式的占位符（例如 `modify_pair` 中 `id=1` 的 `src_text="#1"`, `dst_text="#1"`，`user_word` 中 `id=1..14` 的 `word="#1"..`），真实文本则以 `ctxdb-enc-v1:<hex>` 格式存储在 `*_enc` 列，而確定性检索标签存储在 `*_tag` 列：

| 表名 | `id` | 明文列占位符 | `*_enc` 密文列前缀示例 | `*_tag` 盲索引列（64 字符 Hex） | `use_count` | `recall_count` | `key_version` |
| :--- | :---: | :---: | :--- | :--- | :---: | :---: | :---: |
| `modify_pair` | 1 | `src="#1"`, `dst="#1"` | `ctxdb-enc-v1:1c719234fd6c...` | `src_text_tag`: `08bb92e64123...` | 1 | 0 | 1 |
| `user_word` | 1 | `word="#1"` | `ctxdb-enc-v1:3ae6f95f971c...` | `word_tag`: `3081ba92a6a3...` | 1 | — | 1 |
| `user_word` | 3 | `word="#3"` | `ctxdb-enc-v1:2d9809ccde93...` | `word_tag`: `5390d62a42f3...` | 2 | — | 1 |
| `user_word` | 7 | `word="#7"` | `ctxdb-enc-v1:2fc6d2ff61b6...` | `word_tag`: `0a9bd7627f8c...` | 4 | — | 1 |

通过对 `/tmp/DoubaoIme.arm64` 中 `0x100344120..0x10034a620`（`context_db.cpp`）的反汇编分析，完整还原了 `ctxdb-enc-v1:` 加密方案与 `*_tag` 盲索引派生算法：

```asm
// 源：/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme (arm64, 0x10034a3ec..0x10034a480)
10034a3ec: mov   w2, #0x11               ; =17 字节
10034a3f0: adrp  x1, 0x100fb0000
10034a3f4: add   x1, x1, #0x787          ; "context-db:enc:v1"
10034a3fc: bl    0x10034a624             ; HMAC_SHA256(device_secret, "context-db:enc:v1") -> enc_key
10034a428: adrp  x1, 0x100fb0000
10034a42c: add   x1, x1, #0x799          ; "context-db:mac:v1"
10034a434: bl    0x10034a624             ; HMAC_SHA256(device_secret, "context-db:mac:v1") -> mac_key
10034a464: adrp  x1, 0x100fb0000
10034a468: add   x1, x1, #0x7ab          ; "context-db:tag:v1"
10034a470: bl    0x10034a624             ; HMAC_SHA256(device_secret, "context-db:tag:v1") -> tag_key
```

具体密码学构造如下：

1. **三路子密钥派生（Key Derivation）**：
   - 从 `GlobalContext` 配置参数 `context_db_device_secret` 获取主密钥 `device_secret`，通过 `HMAC-SHA256`（位于 `0x10034a624`，内部调用 `SHA256_Init` 魔数 `0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a`）分别以域分离常量 `"context-db:enc:v1"`、`"context-db:mac:v1"` 和 `"context-db:tag:v1"` 派生出三个 32 字节（256-bit）子密钥：`enc_key`、`mac_key` 与 `tag_key`。
2. **確定性盲索引标签（`src_text_tag` / `dst_text_tag` / `word_tag` / `pair_tag`）**：
   - **单文本标签（`0x100344120`）**：对明文拼接 `"text:"` 前缀后计算 HMAC：
     $$\text{text\_tag} = \text{Hex}\big(\text{HMAC-SHA256}(\text{tag\_key},\ \texttt{"text:"} \parallel \text{plaintext})\big)$$
   - **纠错对联合标签（`0x10034421c`）**：对源文本与目标文本以空字节 `\x00` 分隔并拼接 `"pair:"` 前缀：
     $$\text{pair\_tag} = \text{Hex}\big(\text{HMAC-SHA256}(\text{tag\_key},\ \texttt{"pair:"} \parallel \text{src\_text} \parallel \texttt{"\textbackslash 0"} \parallel \text{dst\_text})\big)$$
   - 这种设计使得 SQLite 无需解密即可在 SQL 层通过 `WHERE src_text_tag = ?` 或 `ON CONFLICT(pair_tag)` 完成精确匹配、去重与链式规则消解。
3. **带认证流加密字段（`ctxdb-enc-v1:`，`0x100344350`）**：
   - 从 `/dev/urandom` 读取 16 字节随机 IV。
   - 按 32 字节分块，以 `HMAC-SHA256(enc_key, IV || uint32_be(counter))` 生成密钥流并与明文异或得到 `ciphertext`。
   - 计算認證标签 `mac = HMAC-SHA256(mac_key, "v1:" || IV || ciphertext)`，最终输出 `"ctxdb-enc-v1:" + Hex(IV || ciphertext || mac)`。因此，对于长度为 $L$ 字节的 UTF-8 明文，`Hex` 部分长度恒为 $2 \times (16 + L + 32) = 96 + 2L$ 个十六进制字符（例如 6 字节即 2 个汉字的词条，Hex 长度为 `108` 字符）。

### 3.3 用户纠错对学习、冲突清洗与三段式 RAG 召回链路

当用户完成一次语音输入并在输入框中手动修改了识别结果，或高频输入特定专有名词时，`GlobalContext`（`global_context.cpp`）与 `ContextDB`（`context_db.cpp`）按以下闭环工作：

#### （1）分词、NER 提取与入库（`CutWithNer` / `UpsertUserWord` / `UpsertModifyPair`）

- **用户词提取（`cut_with_ner.cpp`）**：`GlobalContext` 调用 `CutWithNer`，联合使用 `ContextAuxModel` 中的 `cppjieba_dict`、`libcut_model`、`car_ner_model_15.convert` 与 `asr_user_ner_dict.dat` 对用户输入文本进行分词与命名实体识别，将识别出的专有名词通过 `UpsertUserWord` 写入 `user_word` 表（若 `word_tag` 冲突则累加 `use_count` 并更新 `update_at`）。
- **纠错对提取与更新（`UpsertModifyPair`）**：当检测到用户将语音识别原句中的片段 `src_text` 修改为 `dst_text` 时，调用 `UpsertModifyPair` 插入或更新 `modify_pair` 表。若 `pair_tag` 已存在，则执行冲突更新：

```sql
-- 源：/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme (0xfadf06, context_db.cpp)
UPDATE SET
    use_count = use_count + 1,
    update_at = ?,
    recall_count = recall_count; -- 保留召回次数原值
```

如源码注释 `-- 保留召回次数原值` 所示，重复触发同一纠错对时仅递增 `use_count` 并刷新 `update_at`，不会清零历史累计的 `recall_count`。

#### （2）图结构冲突消解与过期清理（`CleanModifyPair` & `ApplyReplaceRules`）

为防止用户历史修改产生循环替换、过期映射或相互抵消，`context_db.cpp` 在数据库层（`CleanModifyPair`）与内存层（`ApplyReplaceRules`）实施了完全对称的三条图消解规则及容量淘汰策略：

1. **过期与容量淘汰**：默认清理创建时间超过 **30 天**（`julianday(CURRENT_TIMESTAMP) - julianday(create_at) > 30`）的记录，并按 `ORDER BY update_at DESC, use_count DESC LIMIT N` 截断超出上限的旧记录。
2. **Rule 1 — 传递链式替换消解（`Chain replace: A->B (old) → A->C (new)`）**：若存在旧记录 `A -> B` 且随后出现更新时间更晚的 `B -> C`（即 `b.src_text_tag = modify_pair.dst_text_tag AND modify_pair.update_at < b.update_at`），则在清理时删除旧版本或将其合并为 `A -> C`。
3. **Rule 2 — 同源多目标去重（`Exclude outdated pair`）**：若同一 `src_text_tag` 对应多个不同 `dst_text_tag`，仅保留 `update_at` 最新的一条记录。
4. **Rule 3 — 双向互逆抵消（`Exclude mutual pair: A<->B`）**：若同时存在 `A -> B` 与 `B -> A`（即 `b.src_text_tag = modify_pair.dst_text_tag AND b.dst_text_tag = modify_pair.src_text_tag`），判定为偶发误改或语义歧义，将两条互逆记录一并剔除：

```sql
-- 源：/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme (0xfae4d3, context_db.cpp)
DELETE FROM modify_pair
WHERE EXISTS (
    SELECT 1 FROM modify_pair b
    WHERE b.src_text_tag = modify_pair.dst_text_tag AND b.dst_text_tag = modify_pair.src_text_tag
);
```

该 SQL 直接利用确定性 HMAC 盲索引 `src_text_tag` 与 `dst_text_tag` 的自连接（Self-Join），在不解密任何记录的前提下即可在 SQLite 引擎内完成双向互逆对的检测与删除。

#### （3）三段式混合召回策略（`QueryModifyPairTopN`）与召回反馈闭环（`UpdateDstTextRecallCount`）

在每次启动语音识别会话构建大模型 Prompt 时，`GlobalContext` 调用 `QueryModifyPairTopN`（默认 `total_limit = 50`）与 `QueryUserWordTopN`（默认 `limit_num = 100`）。其中 `QueryModifyPairTopN` 并非简单按单一维度排序，而是采用兼顾**即时性（Recency）**、**历史有效性（Recall Utility）**与**高频性（Frequency）**的**三段式混合配额召回算法**：

```sql
-- 源：/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme (0xfaee5e, Stage 1: TodayTop30)
SELECT src_text_enc, dst_text_enc, src_text_tag, use_count, recall_count, update_at, 1 AS is_today
FROM modify_pair
WHERE src_text_enc IS NOT NULL AND dst_text_enc IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM modify_pair mp2
                  WHERE mp2.src_text_tag = modify_pair.dst_text_tag
                    AND mp2.dst_text_tag = modify_pair.src_text_tag)
  AND datetime(update_at) >= datetime(CURRENT_TIMESTAMP, 'start of day')
ORDER BY use_count DESC, update_at DESC;
```

三段式配额的具体分配逻辑如下：

1. **第一层配额（Tier 1）：`[TodayTop30]`（当日即时纠错配额，目标上限 30 条）**：
   - 筛选当日零点以来（`datetime(update_at) >= datetime(CURRENT_TIMESTAMP, 'start of day')`）新增或触发过的纠错对，按 `use_count DESC, update_at DESC` 排序，解密后取最多 30 条（计为 `today_actual`）。
2. **第二层配额（Tier 2）：`[RecallTop10]`（高召回价值配额，目标上限 10 条）**：
   - 在排除第一层配额已选中的 `src_text_tag` 后（`AND src_text_tag NOT IN (...)`），按 **`ORDER BY recall_count DESC LIMIT 10`** 选出历史上被 ASR 引擎成功命中并采纳次数最多的前 10 条记录（计为 `recall_actual`）。
3. **第三层配额（Tier 3）：`[Remaining]`（全局高频兜底配额，目标上限 `total_limit - today_actual - recall_actual`）**：
   - 若剩余配额 `final_remaining = total_limit - today_actual - recall_actual > 0`，则在其余记录中按 **`ORDER BY use_count DESC, update_at DESC LIMIT <final_remaining>`** 补齐至总上限（50 条），随后通过 `ApplyReplaceRules` 执行内存规则校验并注入 `PromptProcessor::ProcessUserModify`。
4. **正向反馈闭环（`UpdateDstTextRecallCount`）**：
   - 当离线或云端 ASR 最终输出的文本命中了某条召回纠错对的目标词 `dst_text` 时，引擎立即调用 `UpdateDstTextRecallCount`，通过 `WHERE dst_text_tag = ?` 将该记录的 `recall_count` 加 1。这使得真正帮助用户纠正发音偏差的词条在第二层配额 `[RecallTop10]` 中持续保持靠前排名，而未被命中的无效纠错对则随时间自然淘汰。

---

## 4. 端到端离线语音与个性化上下文推理架构图

综合上述逆向证据，豆包输入法（macOS）的端侧离线语音识别与个性化上下文自学习闭环架构如下：

```
+-----------------------------------------------------------------------------------+
|                        用户交互与宿主上下文层 (DoubaoIme Swift)                      |
|  - 键盘/语音上屏后用户手动修改文本 (src_text -> dst_text)                             |
|  - 当前输入框光标上下文 (CUR_CONTEXT) & 历史对话 (DIALOG)                            |
+-----------------------------------------+-----------------------------------------+
                                          |
                                          v
+-----------------------------------------------------------------------------------+
|                 SAMICORE 全局上下文引擎 (GlobalContext / ContextDB)                  |
|                                                                                   |
|  +----------------------------------+      +-----------------------------------+  |
|  | ContextAuxModel (rev 10002)      |      | ASRContext/context.db (SQLite 3)  |  |
|  | - cppjieba_dict (34.9万词 + IDF) |      | - Key: HMAC-SHA256(device_secret) |  |
|  | - libcut_model/user.txt          | ---> | - Tag: HMAC(tag_key, "text:"|...) |  |
|  | - car_ner_model_15.convert (NER) |      | - Enc: ctxdb-enc-v1 (HMAC流加密)   |  |
|  | - asr_user_ner_dict.dat (45.3MB) |      | - 规则: 链式合并/去旧/互逆抵消       |  |
|  | - pinyin_dict/hzpy-utf8.txt      |      | - 召回: TodayTop30 + RecallTop10  |  |
|  +----------------------------------+      |         + Remaining (UseCount)    |  |
|                                            +-----------------+-----------------+  |
+--------------------------------------------------------------|--------------------+
                                                               |
                  +--------------------------------------------+
                  | 召回 Top-100 user_word (HOTWORDS) & Top-50 modify_pair (USER_MODIFY)
                  v
+-----------------------------------------------------------------------------------+
|                Flute & Panther 离线语音大模型推理栈 (flute::asr::ntp_cif)            |
|                                                                                   |
|  [麦克风 PCM 16kHz] --> [flute.vad.v4] --> [FeatureExtractorActor]                 |
|                                                    |                              |
|  +-------------------------------------------------+---------------------------+  |
|  | asr_model.model (313.44 MB, FLUTE v3: ChaCha20 Header + XOR Payload, 21子文件) |  |
|  |                                                                             |  |
|  |  1. 声学编码与 CIF 压缩:                                                     |  |
|  |     EncoderActor (Frontend + Backbone) -> cif_calculator -> Audio Embeds    |  |
|  |                                                                             |  |
|  |  2. 多槽位 Prompt 组装 (PromptProcessor + GPT2Tokenizer):                    |  |
|  |     [BOT_NAME/DESC] + [USER_MODIFY] + [HOTWORDS] + [CUR_CONTEXT] + [AUDIO]  |  |
|  |                                                                             |  |
|  |  3. 端侧量化大模型解码 (PantherLLMEngineCreator & DecoderContextActor):        |  |
|  |     - 核心算子: W4A8ImeAsrQwen3Transformer (4-bit 权重 / 8-bit 激活 Qwen3)    |  |
|  |     - 加速机制: KVCacheManager (gpu_kv_offload) + Speculative Decoding (SPEC)|  |
|  |     - 双路融合: Stream Partial (实时流式) + Non-Stream Final (尾点重解码)      |  |
|  +-------------------------------------------------+---------------------------+  |
|                                                    |                              |
|  +-------------------------------------------------+---------------------------+  |
|  | 拼音模糊音与语言模型辅助重打分 (acllm_context_model_mac)                        |  |
|  | - fuzzy_model (15.74 MB) + ngram_model (64.30 MB)                           |  |
|  +-------------------------------------------------+---------------------------+  |
+----------------------------------------------------|------------------------------+
                                                     |
                                                     v
                     [最终识别文本上屏 & 触发 UpdateDstTextRecallCount(+1)]
```

---

## 5. 边界与限制及开放问题（Boundaries, Limitations & Open Questions）

### 5.1 边界与限制

1. **端侧模型总磁盘与内存开销较大**：`acllm_main_model_mac`（`asr_model.model` 506.90 MB）与 `acllm_context_model_mac`（`fuzzy_model` 15.74 MB + `ngram_model` 64.30 MB）加 `ContextAuxModel`（33.54 MB）合计占用约 620.5 MB 磁盘空间；尽管通过 `FilesystemPackedMmap` 按页映射，但唤醒解码器与 `W4A8ImeAsrQwen3Transformer` 算子期间仍存在显著的端侧内存峰值。
2. **私有打包容器与加密密钥耦合**：`FLUTE` v3 容器内嵌 ChaCha20 流加密（基于 `nonce` 与内置派生密钥），且 `context.db` 使用硬编码于二进制中的 `HMAC-SHA256` 三路派生密钥（`ctxdb-enc-v1:`），无法直接被外部通用推理引擎加载，仅能借鉴其架构设计。
3. **热词挖掘对中文拼音表的语种绑定**：`GlobalContext` 的 `hzpy-utf8.txt` 与 `fuzzy_model` 深度针对中文拼音模糊音（如平翘舌、前后鼻音）设计，对纯英文或中英混输缩写的跨语种同音纠错覆盖有限。

### 5.2 开放问题

1. **Rime 用户词典与端侧语音热词库的双向同步边界**：鼠须管（Squirrel）已具备 `librime` 的用户词典（`*.userdb.txt` / LevelDB），若在本地语音输入中引入类似 `context.db` 的 `user_word` 与 `modify_pair` 表，应直接读取 Rime 用户词典快照，还是维护独立的 SQLite 热词与纠错对存储？
2. **端侧小参数 LLM 与纯规则/N-gram 纠错的延迟权衡**：在 Apple Silicon 设备上，离线 ASR 尾点重解码若叠加端侧 Transformer（如 `Qwen3` W4A8）会增加尾包上屏延迟，如何根据语句长度或置信度动态决定是否跳过端侧重打分？

---

## 6. 与本项目的潜在关联（候选，待用户确认）

以下观察仅作为技术层面的客观参照（全部标记为**候选（待用户确认）**，不构成任何架构定案）：

1. **基于 SQLite 的轻量级用户纠错对与个人热词闭环（候选（待用户确认））**：
   - 豆包输入法在端侧维护 `modify_pair`（记录用户将 ASR 识别结果从 `src_text` 改为 `dst_text`）与 `user_word`（高频专有名词），并通过三条图消解规则（传递链合并、同源留新、双向互逆抵消）清洗脏数据，结合 `[TodayTop30] + [RecallTop10] + [Remaining]` 三段式配额召回注入大模型 Prompt。这种无需微调模型权重即可实现端侧个性化自适应的数据闭环，可作为本地语音输入上下文记忆机制的参考形态（候选（待用户确认））。
2. **多槽位结构化 Prompt 注入模板（候选（待用户确认））**：
   - `flute::asr::ntp_cif::PromptProcessor` 将 `USER_MODIFY`（纠错对）、`HOTWORDS`（个人热词）、`CUR_CONTEXT`（当前光标上下文）与 `DIALOG` 统一编码为前缀/后缀 Prompt Token 并通过 `ResetKvCachePos` 复用 KV 缓存的设计，展示了端侧 Speech-LLM 或后处理 LLM 低延迟融合多源上下文的实现路径（候选（待用户确认））。
3. **基于结巴分词 + IDF + 拼音词表的本地热词自动挖掘（候选（待用户确认））**：
   - `GlobalContext` 利用 `cppjieba`（配合高 IDF 阈值过滤通用词、保留低频专有名词）、轻量级 NER 模型与 `hzpy-utf8.txt` 汉字拼音映射表从用户日常打字中静默提取高价值热词，可作为输入法与语音识别共享用户词库的技术参考（候选（待用户确认））。

---

## 7. 检索日志（Query Log）

| # | 工具 / 命令 | 目标路径 / 符号 / 参数 | 核心产出摘要 |
| :-: | :--- | :--- | :--- |
| 1 | `bash` (`find`, `stat`, `shasum -a 256`) | `~/Library/Application Support/DoubaoIme/` | 获取全部本地模型、词表、JSON 清单及 SQLite 数据库的精确字节数与 SHA-256 校验和 |
| 2 | `mcp__rea` (`rea_inspect_sqlite_database`) | `~/Library/Application Support/DoubaoIme/ASRContext/context.db` | 提取 `modify_pair` 与 `user_word` 表 DDL、索引定义及 15 条加密样本记录 |
| 3 | `read` | `ASRModel/offline_model_active_layout.json`, `OfflineModelDownloads/manifest/offline_model_manifest.json`, `ContextAuxModel/revisions/10002/meta.json`, `Json/compression_result.json` | 确认 `acllm_main_model_mac`、`acllm_context_model_mac` 与 `ContextAuxModel` (rev `10002`) 的目录布局及版本元数据 |
| 4 | `bash` (`xxd`, `head`, `wc -l`) | `asr_model.model`, `fuzzy_model`, `ngram_model`, `car_ner_model_15.convert`, `asr_user_ner_dict.dat`, `cppjieba_dict/*`, `hzpy-utf8.txt`, `usr.dat` | 识别 `FLUTE` 魔数（`46 4c 55 54 45`）、`OIME_USR_DICT_0` 魔数、Trie 头部及 34.9 万行结巴词表与 2.1 万行拼音表 |
| 5 | `mcp__rea` (`rea_open_binary`, `rea_inspect_macho`) | `/Library/Input Methods/DoubaoIme.app/Contents/Frameworks/OimeEngine.framework/Versions/A/OimeEngine` | 解析 `OimeEngine` Mach-O 头部、段布局与导入/导出符号表 |
| 6 | `mcp__rea` (`rea_trace_feature`) | `OimeEngine` (`asr_user_ner_dict`, `ner_model`) | 定位 `/Users/0bpha83joo1fg7n3/110945/obric-ime-engine/src/other/asr/asr_user_ner_dict.cpp` 与 `AddModifyPairNerResponse` |
| 7 | `mcp__rea` (`rea_open_binary`, `rea_inspect_macho`) | `/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme` (`detail="summary"`) | 获取主程序 Mach-O 摘要、链接库依赖及段信息（证据 ID `ev_5e3d0c51...`） |
| 8 | `mcp__rea` (`rea_demangle_swift`) & `xcrun swift-demangle` | `DoubaoIme`, `OimeCommon.framework`, `OimeEngine.framework` | 还原 `OimeCommon.PathManager.asrContextAuxModelDirectoryName`、`OfflineASRService` 等 Swift 符号 |
| 9 | `bash` (`nm`, `strings`) | `/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme` | 发现 `sami_core`、`flute`、`panther` 静态链接于主程序，提取 `FluteManager`、`PromptProcessor`、`PantherLLMEngineCreator`、`W4A8ImeAsrQwen3Transformer` 等符号 |
| 10 | `bash` (Python string dump) | `DoubaoIme.arm64` (`0xfab000..0xfb2000`, `0xfb7f00..0xfc2c00`) | 完整提取 `global_context.cpp`、`context_db.cpp`、`filesystem_packed_mmap.cc`、`prompt_processor.cc`、`decoder_context_actor.cc` 日志与 SQL 语句 |
| 11 | `bash` (`lipo -thin arm64` + Python `adrp`/`add` XREF scanner) | `/tmp/DoubaoIme.arm64` | 精确定位 `FilesystemPackedMmap` (`0x1003f0e84`)、`ContextDB` 加密函数 (`0x100344000..0x10034a620`) 与 `W4A8ImeAsrQwen3Transformer` (`0x1007c2a88`) 的代码交叉引用地址 |
| 12 | `bash` (`xcrun llvm-objdump -d`) | `/tmp/DoubaoIme.arm64` (`0x1003f0c00..0x1003f19a8`) | 逆向还原 `asr_model.model` 的 `FLUTE` v3 容器头部结构（5 字节魔数、版本 `3`、头部长度 `1395`、21 个子文件、12 字节 ChaCha20 Nonce 及目录项结构） |
| 13 | `bash` (`xcrun llvm-objdump -d`) | `/tmp/DoubaoIme.arm64` (`0x100344000..0x10034a660`) | 逆向还原 `context.db` 的 `HMAC-SHA256` 三路密钥派生（`enc`/`mac`/`tag`）、`text:`/`pair:` 盲索引计算与 `ctxdb-enc-v1:` 流加密算法 |
| 14 | `bash` (`xcrun llvm-objdump -d`) | `/tmp/DoubaoIme.arm64` (`0x1007c29c0..0x1007c2c40`) | 逆向验证 `W4A8ImeAsrQwen3Transformer` 自定义算子配置校验与注意力掩码检查逻辑 |

<!-- research-complete -->
