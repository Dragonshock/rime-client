# 009-derivation-doubao-local-model-runtime：豆包离线本地模型运行时契约

本文只回答一件事：语音接口的本地运行时适配器（local runtime adapter），若要把用户点名的豆包离线模型接到 007 的识别流上，向它送什么、从它收什么。倾向是候选。样本是笔记 `dev-docs/notes/003-research-doubao-ime-local-models-and-inference.md` 所记的同一套官方进程，加上本机只读目录 `/Library/Input Methods/DoubaoIme.app` 与 `~/Library/Application Support/DoubaoIme/`。

003 写调研当时的资产布局。本机通用二进制 SHA-256 仍是 `5b48748fa6394811cba0f8c05905ffebb9a2cd02faf6d47b26deb3d9b86247e0`，与 003 所记一致，版本字符串仍是 `1.1.0` / Wave Build `1010005`。应用支持目录里的模型文件已经换成另一套路径和体积。下文凡是与 003 不一致处，以本次只读探测为准，并写明 003 的哪一条不再覆盖当前文件。

容器怎么打开、权重怎么解出，留在官方进程内部。本文不记录容器密钥、nonce、密钥调度、解密后的权重，也不记录从官方客户端取出鉴权材料的做法。逆向只核对可调用的数据契约、类名与调用链。

## 1. 推理链

### 1.1 为何是「进程内参数契约」，不是「把模型文件交给外部引擎」

007 的绑定原语把离线制品收成适配器：对外满足识别流或改写流的输入输出，对内接制品；契约未知时角色空着。003 已经说明推理栈静态链接在 `DoubaoIme` 主程序里，模型文件是 `FLUTE` 打包容器。本次要决定的是：适配器的送入和收回，能不能落在进程边界之外。

本机主程序 arm64 切片的全局导出只有 ` __mh_execute_header`。对 `DoubaoIme` 同包内的 `libDoubaoImeDylib.dylib`、`libime_net_sdk.dylib`、`libsscronet.dylib`、`libMetaSecML.dylib`、`libapplogrs.dylib`、`libTTNetDownloaderCrossPlatform.dylib` 做全局符号过滤，没有 `SAMICoreCreateHandleByIdentify`、`FluteManager`、`AsrOffWorker`。这些名字只出现在主程序的字符串和未导出代码里。因此「链接一个库、调用一个打开函数」这条外部入口，在当前样本上不成立。

同时，主程序字符串把一条完整的进程内顺序留了下来，而且不是死字符串：格式串 `stream partial: {}, non-stream final: {}` 在 `__text` 里被取址后进入一次调用。打开、送音、槽位、部分结果、定稿结果都有对应的日志函数名。适配器能核对的，是这条顺序上的参数名字和结果种类；能执行它的代码在 `DoubaoIme` 进程内部。

所以本文的方向是：把适配器写成「路径 + 槽位快照 + PCM 块进，可修订文本假设出」的数据契约，并把「谁来执行」写成缺口，而不是写成一条绕过官方进程的打开步骤。

### 1.2 舍弃的焊法

**舍弃把 `asr_model.model` 当成外部推理引擎的权重文件。** 当前文件 `ASRModel/versions/1_8/asr_model.model` 前 5 字节是 ASCII `FLUTE`，紧随的小端 `uint32` 布局版本是 3，偏移 `0x09` 的小端 `uint64` 头部总长是 1143，偏移 `0x11` 的小端 `uint32` 子文件数是 20，文件总长 824,999,405，与 `offline_model_manifest.json` 里 `acllm_main_model_mac` / `version` `1.8` 的 `sourceFileSize` 一致。版本 3 在 `FilesystemPackedMmap` 的日志里与「header 加密的打包布局」列在一起。目录项和权重不在这 21 个明文字段里。003 对另一份 313,440,860 字节、21 个子文件、头部 1395 字节的文件所做的目录还原，不覆盖这一份。外部引擎没有可核对的加载入口。

**舍弃把 003 的上下文模型路径当成当前送入路径。** 003 写的是 `ASRModel/acllm_context_model_mac/{fuzzy_model,ngram_model}` 和 `ContextAuxModel/revisions/10002/`。本机 `offline_model_active_layout.json`（210 字节，`schemaVersion` 1）写的是：

- `mainModelActiveRelativePath` = `ASRModel/versions/1_8/asr_model.model`
- `contextAuxActiveRelativePath` = `ContextAuxModel/versions/1_1/context_aux_model`

`OfflineModelDownloads/acllm_main_model_mac` 与 `acllm_context_model_mac` 两个目录存在且为空。`fuzzy_model`、`ngram_model` 两个文件不在支持目录里。二进制仍保留键名 `fuzzy_model_path`、`ngram_model_path`，那是 `GlobalContext` 初始化参数名，不是本机当前文件存在的证据。

**舍弃把 `OimeEngine` 的音频核函数当成离线语音入口。** `OimeEngine` 导出 `GetAudioFbankKernel`、`InitAudioFbankParam`，以及 `oime_engine::pinyin` 下的 `AsrNgramDict`、`AsrUserNerDict`、`AsrCharErrorDict`，还有 ObjC 类 `ASRGlobalContext`。这些符号落在键盘拼音引擎。`flute::asr::ntp_cif`、`FluteAsr`、`AsrOffWorker` 的字符串在主程序，不在这份导出表里。把 fbank 核函数接到适配器上，送进去的就不是 007 所说的识别流。

**舍弃把 `ASROfflineModelIPCClient` 当成送音通道。** 该 Swift 类型在 `OimeOfflineModel/.../ASROfflineModelIPCClient.swift`。相邻日志是安装、快照、下载会话、`offline_model_manifest.json`、解压 `context_aux_model.zip`。没有与 `SendAudioData` 或 `ProcessAudio` 同串的参数。IPC 在这里搬的是模型文件是否到位，不是采样帧。

**舍弃把在线结果字段焊成离线结果字段。** 同一主程序里还有 `sami_core_ws_asr_context.cpp` 和字符串 `is_interim`。离线工人自己的结果键是 `is_offline_result`、`is_vad_finished`，并另有日志 `online has final result, no need use offline` 与 `offline final result return will wait online result`。两套结果在官方会话里会互相等待或互相跳过。适配器若只描述离线制品，不能把在线字段的名字当成离线回调的字段。

**舍弃单函数「给出模型路径就返回提交文本」。** 007 已经舍弃焊法 A。本次日志把打开（`CreateFromFile`）、会话开始（`AsrOffWorker::Start`）、送音（`SendAudioData`）、部分文本与定稿文本分成先后不同的记录，支持该舍弃，不另立一个总函数。

## 2. 与 007 原语的对应关系

对应只说明这条制品碰到哪一维、哪一维它碰不到。不把官方类名升成接口名。

| 007 原语 | 本机观察到的对应物 | 对应到哪一步为止 |
| --- | --- | --- |
| 会话 | `SAMIHandle` 的 `logID`、`isASRStarted_`、`finishASRCalledAt`；离线侧字符串 `task_id`、`SessionFinished` | 官方进程用这些记号区分一次识别。它们与宿主字段标识是否为同一字段：未知 |
| 触发 | `OimeCommon` 的快捷键通知与 `ASRShortcutModifierFlags` | 不进入模型运行时。开闭条件仍按 007，不从模型日志里读 |
| 采集 | `DoubaoIme.ASRAudioEngine` 把 `CMSampleBuffer` 转成 `AVAudioPCMBuffer`，日志 `audio_sent` | 这是官方采集。适配器停在「已经带格式的帧」，不接 `AVAudioEngine` |
| 识别流 | `AsrOffWorker` 吃音频与 `context_info`，经 `FluteAsr::ProcessAudio` 产出可被非流式覆盖的流式文本 | 对应成立到「帧与快照进、假设出」。执行点在进程内 |
| 改写流 | `PromptProcessor` 在同一次解码里处理 `USER_MODIFY` 与 `HOTWORDS` | 没有观察到脱离 `fillAudio` 的纯文本入口。制品可以担任识别流；不能据此认为它另外暴露了改写流 |
| 上下文 | `BuildContextInfo` 的键，与 `PromptProcessor` 的槽位名 | 适配器交出的是槽位快照。`context.db` 的加解密留在 `GlobalContext` |
| 呈现 / 交付 | `generateASRResult`、`sendASRResultToKeyboard`；`OimeCommon.ASRPunctuationTextProcessor` | 这两处已经离开运行时，进入键盘侧文本。适配器不把它们收成模型的返回值 |
| 绑定 | 无进程外导出；布局 JSON 只给出路径 | 绑定的输出是「字段名已核对，进程外实例不成立」 |

原语 4 的核对句（只喂帧、不写预编辑）在官方进程里没有被本次探测执行。日志表明工人自己不持有宿主字段：它打印 `asr engine final result` 和 `prompt_str`，把上屏留给 `sendASRResultToKeyboard`。这是结构上的分离，不是一次运行测量。

原语 5 的核对句（换成恒等改写、识别流不动）对本制品不成立为一次独立调用。纠错对和热词是解码提示的槽，不是解码完成之后的另一次文本调用。007 允许一件制品只担任其中一个角色。候选是：这件制品担任识别流；改写若要独立替换，不把 `PromptProcessor` 的槽位函数当成改写流的入口。

## 3. 数据契约与状态迁移

### 3.1 进程内调用链

下列名字来自 arm64 切片的日志字符串与源码路径字符串。符号表已剥离，除特别注明外没有函数体证明。顺序按日志所描述的前后关系排列，不是一份可链接的头文件。

1. 路径。`OimeCommon.PathManager` 提供 `getASRModelVersionFileURL(versionKey:)`、`getASRContextAuxModelVersionDirectoryURL(versionKey:)`、`getASRContextDBFileURL()`。`ASROfflineModelActiveLayoutStore` 读写 `offline_model_active_layout.json` 的 `mainModelActiveRelativePath` 与 `contextAuxActiveRelativePath`。Swift 侧加载日志是 `SAMIASRRuntimeManager.loadOfflineASRModelIfNeeded()`，失败日志是 `offline model unavailable: model path is nil`，以及 `asr model path is empty will use online asr`。
2. 上下文库与辅助目录。`SAMIGlobalContextManager` 在 `context_aux_model_path` 或 `dbPath` 不可用时跳过初始化。`GlobalContext` 的参数键包括 `db_path`、`cut_mode`，以及相对子目录 `/cppjieba_dict`、`/libcut_model`、`/ner_model`、`/pinyin_dict`，另有 `fuzzy_model_path`、`ngram_model_path`。`SetExecutorMode` 的日志写明要在装载离线模型之前调用。`enable_global_context` 出现在 `AsrOffWorker::Init` 附近。
3. 打开主模型。`FluteManager::CreateFromFile(model_path)`：打开失败、文件长度为 0、`FluteEngineLoadResource` 失败各自有日志；成功路径上的资源名字符串是 `config.yaml`，随后是 `FluteEngineInitialize` 与 `CreateEngine`。`FluteAsr` 初始化打印 `asr_model_path` 和返回码。`AsrOffWorker::Init` 要求 `asr_params` 是 JSON 对象。
4. 会话开始。`AsrOffWorker::Start` 打印 `FluteAsr Start, context info`。`BuildContextInfo` 处理 `context_info`；日志含「base64 decode failed」和「discarded」。解码是否每次都发生，未知。能从相邻键名读到的字段见 3.3。
5. 送音。Swift 侧 `SAMIHandle.sendAudioData(_:dataType:frameSize:)`，并累计 `totalAudioBytesSent`。C++ 侧函数名 `SendAudioData`，日志为 `offline send audio size: %d, cnt: %d, final: %d`、`offline asr receive audio length`、`finish audio`。`FluteAsr::ProcessAudio` 之后，`flute.asr.ntp_cif.IntegrationActor` 打印 `fillAudio`；失败则有 `fillAudio failed`。空上下文时的另一条日志是 `context is empty (call fillAudio/fillToken first)`。`EnhancePcm16Volume` 与 `SetAgcGainDb` 表明这条离线增益作用在 16-bit PCM 上。`SAMIHandle` 初始化串还带 `agcEnabled` 与 `agcGainDB`。
6. 部分结果与定稿。`IntegrationActor` 用同一条格式串同时打出流式部分文本和非流式定稿文本。覆盖规则的日志是 `skip stream output covered by non-stream`，并带两侧时间区间。`AsrOffWorker::Run` 在引擎指针为空时直接记录；否则解析结果。解析键包含 `is_offline_result`、`is_vad_finished`、`prompt_str`、`tail_vad_state`。空文本且未结束：`text is empty interim result, not callback`。空文本且 `vad_finished`：允许作为定稿回调。`Finish` 若发生在定稿和 `SessionFinished` 之前，工人等待。没有待定稿时：`use last result to return final result`。
7. 卸下。`unloadOfflineASRModel(reason:)` 调用 `UnloadOfflineAsrModel`。`AsrOffWorker::Destroy` 会 join 其线程。`FluteManager::Release` 在仍有借出的 executor 时拒绝。`GlobalContext` 关闭失败时，生命周期日志选择保持已装载状态。

`flute.vad.v4` 的 `FeatureExtractorActor`、`EncoderActor`、`DecoderActor`、`WavSplitActor` 与 `flute.asr.ntp_cif` 的 `EncoderActor`、`DecoderContextActor`、`PromptProcessor`、`PantherLLMEngineCreator` 是这条链内部的图节点名。适配器不逐个节点送数据。图的入口在观察到的证据里是 `ProcessAudio` / `fillAudio` 加上 `set_context`。

```
// 源：/tmp/DoubaoIme.arm64 __TEXT,__text，llvm-objdump，地址 0x10061ba70 起
10061ba70: add  x1, x20, #0x218
10061ba74: adrp x0, 0x100fc2000
10061ba78: add  x0, x0, #0x7ca
10061ba7c: add  x19, sp, #0xb8
10061ba80: add  x8, sp, #0xb8
10061ba84: sub  x2, x29, #0x90
10061ba88: bl   0x10061cb98
```

`x0` 被设成文件偏移 `0xfc27ca` 处的格式串 `stream partial: {}, non-stream final: {}`（`__TEXT` 虚拟地址基址 `0x100000000`，该字符串页内偏移 `0x7ca`）。随后的 `bl` 目标在符号表里没有名字。这段只证明该格式串从可执行代码取用，两个花括号所对应的 C++ 类型没有在这 8 条指令里还原。代价是：不能把这条日志当成回调结构体的定义。边界是：流式文本与非流式文本在同一次打印里成对出现，谁覆盖谁还要看前面的 `skip stream output covered by non-stream` 日志，而不是看这次调用的返回寄存器。

### 3.2 打开时送出的路径

适配器在「装载」这一步送出的，是布局 JSON 已经写明的两条相对路径，加上上下文库文件路径。当前样本的值：

| 名字 | 当前样本 |
| --- | --- |
| 主模型文件 | `ASRModel/versions/1_8/asr_model.model`，824,999,405 字节，`FLUTE` 版本 3，子文件数 20 |
| 上下文辅助目录 | `ContextAuxModel/versions/1_1/context_aux_model` |
| 清单 | `ASRContext/offline_model_manifest.json`，`schemaVersion` 2；主模型 `componentVersionKey` `1_8`，辅助包 `1_1`，`targetKind` 分别是 `mainModel` 与 `contextArchive` |
| 上下文库 | `ASRContext/context.db`，SQLite 3。只读打开时 `modify_pair` 与 `user_word` 行数都是 0。同目录有 WAL 文件，本次没有另做逐页解析 |

辅助目录里实际有的只有三组明文材料：`cppjieba_dict/`（`jieba.dict.utf8`、`hmm_model.utf8`、`user.dict.utf8`）、`ner_model/`（`meta.json` 列出 6 个 `*.convert`，魔数以 `#mdl#2#` 开头）、`pinyin_dict/hzpy-utf8.txt`（26851 行，首行形状是「汉字,声母韵母拆开的拼音,码点,声调标记」，与 003 所引的制表符加 `yi1` 不同）。003 所记的 `idf.utf8`、`stop_words.utf8`、独立的 `libcut_model/` 目录、`asr_user_ner_dict.dat` 大词典，不在这个辅助目录里。`EngineUserDict/asr_user_ner_dict.dat` 只有 90,184 字节，魔数也不是 003 所记的双数组头。它是不是 `GlobalContext` 的 NER 词典：未知。

`config.yaml` 这个名字出现在 `CreateFromFile` 的日志邻域。它是容器打开之后引擎要找的虚拟资源名。文件本体不在支持目录的明文侧。

### 3.3 会话开始时送出的上下文槽

`BuildContextInfo` 邻域的键，和 `PromptProcessor` 的槽，是两层，不要并成一层。

工人在解码之前能看到的键名有：`cursor_position`、`left_text`、`right_text`、`current_chat`、`lr_context`、`bot_id`、`today_pair`、`history_pair`、`original_word`、`modify_word`、`modify_pair_info`、`hotwords`。召回打点还使用 `user_words_text`、`modify_pair_text`、`recall_user_words`、`recall_modify_pair`。

这些键进入 `flute::asr::ntp_cif::PromptProcessor` 之后，日志函数把它们收成槽位：

| 槽位 | 处理日志 | 长度与分隔键（值未知） |
| --- | --- | --- |
| `BOT_NAME` | `ProcessBotName` | `bot_name_max_length` |
| `BOT_DESC` | `ProcessBotDesc` | `bot_desc_max_length` |
| `USER_MODIFY` | `ProcessUserModify` | `user_modify_max_length`、`user_modify_delim`、`user_modify_pair_delim` |
| `DIALOG` | `ProcessDialog` | `dialog_max_length` |
| `HOTWORDS` | `ProcessHotwords` | `hotwords_max_length`、`hotword_delim`、`hotwords_types`、`hotwords_types_prompts` |
| `CUR_CONTEXT` | `ProcessCursorContext` | `cursor_context_max_length`、`cursor_context_delim` |

另外有 `prompt_template`、`prompt_template_map`、`template_delim`、`max_prompt_num`，以及 `UpdatePromptTokens` 所区分的 `pre_prompt` 与 `post_prompt`。模板正文和各 `max_length` 的数值没有出现在明文字符串里，候选位置是容器内的 `config.yaml`。适配器送出的是槽位名到文本的快照，不是某一版模板串。

`left_text` / `right_text` 与槽位 `CUR_CONTEXT` 的拼接方式未知。`today_pair` / `history_pair` 与槽位 `USER_MODIFY` 的拼接方式未知。007 的上下文原语只要求快照；这里的未知是「快照键如何落进槽」，不是快照本身缺席。

`context.db` 的建表语句就在主程序 `context_db.cpp` 的字符串里，与本机空库的 DDL 一致：`modify_pair` 含 `src_text`、`dst_text`、`src_text_enc`、`dst_text_enc`、`src_text_tag`、`dst_text_tag`、`pair_tag`、`key_version`、`use_count`、`recall_count`；`user_word` 含 `word`、`word_enc`、`word_tag`、`key_version`、`use_count`。`SetEncryptionSecret` 与 `context_db_device_secret` 的日志表明加密材料由 `GlobalContext` 持有。适配器不读这些列，也不写这个文件。个人词和纠错对若要进入识别，走槽位快照。

### 3.4 会话中送出的音频

能核对的形状：

- 一块字节，日志分别记录长度、块计数、以及 `final` 是否为结束块。
- 样本宽度按 `EnhancePcm16Volume` 这个函数名，是 16-bit PCM。`sendAudioData` 还带一个 `dataType` 和一个 `frameSize`，二者的取值集合没有在字符串里出现。
- 增益是装载或句柄上的 `agcEnabled` / `agcGainDB`，不是每一块的文本字段。非法增益会被拒绝并留下范围日志，具体上下界的立即数本次没有从指令里取出。

采样率、声道数、交错与否，没有在离线送音路径上被核对成常量。全库搜索字面量 `16000`，命中的是证书时间字段和 silk 解码器断言（`Fs == 16000` 等），不是 `AsrOffWorker` 的日志。003 与 007 提到的 16 kHz、单声道，仍是此前样本的内部观察，不是本次离线送音常量。适配器把格式写在帧上；数值留空，见未知。

`DoraAudio` 的 `pcm_data` 与 `doraSendPCMData` 是另一条耳机采集路径，和 `ASRAudioEngine` 并列出现。适配器不把耳机私有帧格式收成模型契约。

### 3.5 收回的假设

一次收回至少要能区分下面几种，它们在日志里是不同的句子：

| 种类 | 日志依据 | 对适配器的含义 |
| --- | --- | --- |
| 流式部分文本 | `stream partial` | 同一会话上可被更晚结果替换的假设 |
| 非流式定稿文本 | `non-stream final`；可按时间区间盖过流式输出 | 会话内的定稿假设，不是宿主提交 |
| 被盖过的流式输出 | `skip stream output covered by non-stream` | 更早的部分假设不再作为当前文本 |
| 空的中间结果 | `text is empty interim result, not callback` | 不产生假设 |
| 空且 VAD 已结束 | `vad_finished=true, allow callback for final result` | 产生一条空的定稿信号，而不是把「没有字」报成识别失败 |
| 没有新定稿 | `use last result to return final result` | 定稿文本沿用上一条已有结果 |
| 在线侧已有定稿 | `online has final result, no need use offline` | 官方工人可以不把离线定稿送出去 |
| 离线定稿等待在线 | `offline final result return will wait online result` | 官方工人可以把离线定稿扣住 |

`is_vad_finished` 可作为「分句已定稿」信号的候选来源，供 007 的触发消费。`SessionFinished` 是工人运行结束，不等同于某一条假设的文本。二者都不是提交文本。

部分假设如何修订前一条（整段替换，还是按公共前缀），日志没有写。007 原语 7 把这件事留给呈现。运行时契约只保证：更晚的非流式文本可以宣布一段流式时间区间作废。

### 3.6 状态迁移（候选）

状态名是对日志的归纳，不是官方枚举。

```
未装载
  loadOfflineASRModelIfNeeded
    路径为空 → 保持未装载（官方侧改走在线，该分支不属于本适配器的成功返回）
    CreateFromFile / FluteAsr Init 返回失败 → 保持未装载
    成功 → 已装载
已装载
  Start(context_info) → 会话中
  unload → 未装载（GlobalContext 关闭失败时，官方日志选择留在已装载）
会话中
  SendAudioData(final=0) → 会话中，可伴随零条或多条部分假设
  SendAudioData(final=1) 或 Finish → 等待定稿
等待定稿
  解析到 is_vad_finished，或 SessionFinished 且采用上一条结果 → 定稿假设，会话结束
  Stop / Destroy → 已装载或未装载（Destroy 会 join 工人线程；与 unload 的先后未见调用图）
```

在线结果插入「等待定稿」的行为是官方工人内部的分支。适配器的状态图不把该分支画成自己的状态：它只记录「定稿可能被官方会话扣住或丢弃」这一事实。没有测量到一个公开参数可以关掉这个等待。

## 4. 当前倾向（候选）

候选倾向如下。

本地运行时适配器对外只做三送一收。

- 装载送出：布局 JSON 中的主模型相对路径、上下文辅助目录相对路径。不送容器内部的子文件，不送密钥。
- 会话开始送出：一份槽位快照。键使用 3.3 的两层名字，值是文本；读不到的来源让对应键缺席。快照不是 `context.db` 的行，也不是某个模型的模板串。
- 会话中送出：有序的 16-bit PCM 块，每块带长度，最后一块带结束标记。采样率与声道写在帧的格式字段上；本样本没有核对出这两个数。
- 收回：同一会话上的假设序列。每条假设是文本，并带种类（部分或定稿）。定稿可以是空文本。更晚的定稿可以使一段部分假设作废。收回物不带宿主写入句柄。

这件制品的角色候选是识别流（007 原语 4）。`USER_MODIFY` 与 `HOTWORDS` 作为识别输入上的上下文槽进入，不另设一次文本改写调用。若绑定要把改写流接到别的制品，识别流这边的帧与假设类型不变。

执行该契约的 `SAMICore*`、`FluteManager`、`AsrOffWorker` 都静态留在 `DoubaoIme` 进程内，当前样本没有进程外导出。绑定原语的输出因此是：字段名和状态迁移已经能逐条核对，进程外的适配器实例不成立。这个缺口保持为缺口。适配器不用打开容器、不连接官方私有云、不伪装输入法进程来填这个缺口。

反论：只把路径和 PCM 写成契约，看起来像一份不能运行的说明书。正是如此。007 写明契约未知时角色空着，其余原语用替身核对。把说明书补成「如何在进程外把 FLUTE 装进一个自建推理器」，就会把本次没有的入口写成入口。本文不补。

另一反论：官方会话会用在线定稿压过离线定稿，所以离线模型并不是稳定的识别流。日志确实允许这种压过。那是官方 `SAMIHandle` 把两件制品排进同一次会话的策略，不是离线工人输入类型的一部分。适配器若只绑定离线制品，就不应继承这个等待；如何在官方进程里关掉等待，本次没有证据，因此也不把「关掉等待」写进契约。

## 5. 仍然未知的点

1. 16-bit 之外的帧格式数值：采样率、声道数、是否交错、`dataType` 的取值、`frameSize` 的单位。离线路径上没有核对到 16000 这个常量。
2. `context_info` 的 JSON 层级，以及 base64 是每次封装还是只在某条失败日志里出现。键名有了，嵌套没有。
3. `left_text` 与 `right_text` 如何变成 `CUR_CONTEXT`；`today_pair` 与 `history_pair` 如何变成 `USER_MODIFY`。分隔符键名有了，分隔符字符没有。
4. `prompt_template` 正文、各 `max_length`、`hotwords_types_prompts` 的具体字符串。它们不在支持目录的明文文件里。
5. 部分假设的修订粒度，以及 Swift 回调 `generateASRResult` 的字段全集。`is_interim` 属于另一条在线路径，是否也会出现在离线回调上：未知。
6. `task_id` 或 `logID` 与宿主字段标识是否有对应。本次没有把一次真实语音跑进进程。
7. 在线定稿等待的时长和开关参数。只看到「会等」和「可以不用离线」两条日志。
8. 当前 `1_1` 辅助包缺少 `fuzzy_model`、`ngram_model`、`libcut_model/` 时，`GlobalContext` 初始化是否仍然返回成功。字符串要求这些键，文件不在目录里，运行返回码未测。
9. `EngineUserDict/*.dat` 与 `context.db` 的 `user_word` 是否为同一来源。本机 `user_word` 表为 0 行。
10. `CreateFromFile` 到 `fillAudio` 的函数体调用图。Ghidra 在 330 秒分析期限内没有完成导入，符号表又已剥离。上面的链是日志顺序，不是完整控制流图。
11. `SAMIHandle.init` 那条长参数表里 `networkConfiguration` 的字段。其中涉及在线会话的材料不在本文展开，也没有被读取。

## 6. 探测日志

| # | 手段 | 对象 | 产出 |
| --- | --- | --- | --- |
| 1 | 目录遍历与前 16 字节魔数 | `~/Library/Application Support/DoubaoIme/` 下 `ASRModel`、`ContextAuxModel`、`OfflineModelDownloads`、`ASRContext`、`EngineUserDict` | 主模型仅 `versions/1_8/asr_model.model`；辅助模型在 `versions/1_1/context_aux_model`；下载目录为空；`fuzzy_model` 与 `ngram_model` 不存在 |
| 2 | `plutil` 与 `lipo -info` | `DoubaoIme.app` 的 Info.plist、主程序、`OimeEngine`、`OimeCommon` 及同级 dylib | 版本 `1.1.0`，Wave `1010005`，标识 `com.bytedance.inputmethod.doubaoime`；主程序为 x86_64 + arm64 |
| 3 | 只读 JSON、SQLite `sqlite_master`、FLUTE 明文计数字段 | `ASRContext/offline_model_active_layout.json`、`offline_model_manifest.json`、`context.db`、模型文件头 0x00 起的版本与计数 | 布局 schema 1；清单 schema 2；两表 0 行；布局版本 3、头部总长 1143、子文件数 20。未打印头部加密区 |
| 4 | `nm -gU` / `nm -u` | 主程序与同包 dylib、`OimeEngine` | 主程序全局定义仅 `__mh_execute_header`；dylib 无 `SAMICoreCreate` / `FluteManager` / `AsrOffWorker`；`OimeEngine` 的 ASR 符号属于拼音引擎 |
| 5 | `lipo -thin arm64` 后的 C 字符串检索 | `/tmp/DoubaoIme.arm64`，SHA-256 `a6a3ac886111eda4bb271ddfd37769fd94ecdd57d04112d77a60e32d74e36cbf` | 命中 `FluteManager`、`AsrOffWorker`、`PromptProcessor`、`W4A8ImeAsrQwen3Transformer`、`offline final result` 等日志 |
| 6 | 按工人、送音、打开、结果、槽位、Swift 类型分组的字符串检索 | 同一薄切片 | 分开看到 `SendAudioData`、`ProcessAudio`、`fillAudio`、`SessionFinished`、六个槽位名，以及 `ASRAudioEngine`、`SAMIASRRuntimeManager` |
| 7 | 四个文件偏移区间的连续字符串转储 | `0xfa9800`、`0xfb1600`、`0xfc1000`、`0x1210000` 附近 | 还原 `asr_off_worker.cpp`、`flute_asr.cpp`、`prompt_processor.cc`、`SAMIHandle.swift` 的日志顺序与键名 |
| 8 | Mach-O 段解析与 ADRP 扫描，加 `llvm-objdump` 短窗 | 薄切片 `__text` | `SendAudioData` 字符串 15 处代码引用；`stream partial` 格式串 1 处，位于 `0x10061ba74` |
| 9 | `mcp__rea` `rea_open_binary`，先通用二进制，再薄切片 | `/Library/Input Methods/DoubaoIme.app/Contents/MacOS/DoubaoIme` 与 `/tmp/DoubaoIme.arm64` | 通用二进制被拒（Ghidra 不能在导入时选定架构）；薄切片打开成功，SHA 与第 5 项一致 |
| 10 | `mcp__rea` `rea_trace_feature`，查询 `FluteAsr::ProcessAudio` | 已打开的薄切片 | Ghidra 12.1.4 在 330000 ms 内未完成分析，无交叉引用结果。调用链改由第 7、8 项的日志与短窗支持 |
| 11 | `nm` 与 `swift-demangle --compact` | `OimeCommon` 中带 `ASR` 的 Swift 符号 | 得到 `PathManager` 的模型与上下文库 URL、`ASRPunctuationTextProcessor`、`ASRTextDisplayWindowController`。后两者不属于模型运行时 |
| 12 | 字面量 `16000` 的邻域字节 | 薄切片 | 命中证书日期与 silk 断言，未命中 `AsrOffWorker` 送音日志 |

<!-- derivation-complete -->
