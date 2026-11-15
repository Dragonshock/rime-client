# MAP

> ⚠️ **进入前必读：`dev-docs/GUIDE.md`（行为准则）。**
> 本 MAP 只是**状态索引 + refs 目录 + notes 目录**，告诉你"现在在哪、有什么"。
> **怎么行事（能做什么、不能做什么、各类文档怎么写）全部在 GUIDE 里。**
> 只读本 MAP 不读 GUIDE = 只知道状态，不知道规矩。
>
> **编写语言：本文件全部内容一律使用中文。** GUIDE.md 为英文规则书；
> 本 MAP 的区名为领域术语，保持中文。

> **做到最好。只有不值得做，没有工作量太大。延后等于 bug。**
> 本句有三条同等重要的反向约束——①轴限定 ②用户延后是指令 ③过度设计比不产出更糟。
> **完整定义只在 GUIDE §1（进入必读）**；本处仅为提名，两处不一致时以 GUIDE 为准。

> 最后更新：2026-11-15
> 演进规则：想法变了**写新文档显式推翻旧文档**，旧文档原样保留（agent 改旧文档往往改不准）。
> 推翻只针对分析/设计内容；**「已确立」区只能由用户指令变更，任何文档（无论新旧）不得覆盖。**

## 当前状态
（一句话定位「现在在哪」。**单值区、改写式**——区别于下方「目录目标」「code-root」「用户延后」的追加式。
⚠️ **本区不吸收其他区的内容**：目标与优化轴归「目录目标」，目录/git 拓扑归 code-root，
用户钉死的方向归「已确立」，用户说先不做的归「用户延后」，refs 定位归「refs 索引」。本区只写一句定位）
<!-- 格式：一行纯文本，不带列表符号。禁止出现优先级分级词（见顶部与 GUIDE §1）。 -->
换窗口语音失效的两处改动（进程一份的会话号、取消时丢掉引擎里那一枚、同号 begin 改成替换）已验收并装进 `~/Library/Input Methods/RimeClient.app`，真机上切换窗口那一步仍未覆盖。

## 目录目标
（这个目录**为什么存在、要达成什么**。当前状态说"现在在哪"，本区说"要去哪"。
**目标一定存在**——即使调研最后什么都不产出，目录目标依然在。
阶段：初始通常是**调研**（第一要务）；产出是第二要务，**只有用户能宣告转入产出阶段并给出产出目标**。
**读法：直接看本区最后一行**，上面各行是目标演进的历史记录，**agent 不得删改；用户指令更正除外**）
<!-- 格式（两行一条，原话独立成续行，主行才能「一眼看到最后一行」）：
     - 目标：<一句话> ｜ 阶段：<调研 / 产出> ｜ 轴：<用户点名的优化轴> @ YYYY-MM-DD
       原话：「<用户原话>」
     · 轴 = 用户点名要做到最好的那个维度（性能 / 兼容 / 最简 / 最快跑起来 / …）；未点名填「未指定」
     · 原话必须是用户的原始措辞，不得用 agent 的转述代替（同「已确立」的确认原话纪律）
     · 阶段由用户宣告；agent 不得自行判定"调研结束了"并追加产出目标
     · ⚠️ 冲突是常态不是异常——中途推翻天然与已记录目标冲突。冲突处理流程与
       「细化 vs 冲突」的判别**定义在 GUIDE §6「目录目标」**，本注释不复述；先读 GUIDE 再动本区。
     · 槽位值须自带行首「- 」，格式如上；不要再补一个。 -->
- 目标：重构一个类似 Squirrel 的 Rime 客户端 ｜ 阶段：调研 ｜ 轴：未指定 @ 2026-11-11
  原话：「如果我想重构一个 Rime 客户端，类似 Squirrel 这样的」
- 目标：在不影响 Squirrel 现有架构与 Rime 键盘输入的前提下，提供接入离线本地模型与云端大模型的语音输入优化接口 ｜ 阶段：调研 ｜ 轴：非侵入式兼容现有 Squirrel 架构与键盘输入 @ 2026-11-11
  原话：「重构 squirrel 的动机是提供一个接口用来接入语音输入的功能，类似豆包输入法可以通过离线本地模型和云端大模型的方式，优化语音输入。最好是在不影响 Squirrel 的现有架构与代码的情况下，提供这样一个接口让我可以调用大模型来优化语音输入。」
- 目标：提供语音输入接口，使用豆包输入法的离线本地模型与云端大模型接口优化语音输入 ｜ 阶段：调研 ｜ 轴：非侵入式兼容现有 Squirrel 架构与键盘输入 @ 2026-11-12
  原话：「我决定以你的方案、决策和思路进行开发。」「我的目标是：提供一个接口用来接入语音输入的功能，使用豆包输入法的离线本地模型和云端大模型接口来优化语音输入。」「这个东西非常通用，我认为你可以把它开发完毕。如果有任何阻力，你可以用逆向工程的方法研究一下竞品，看看竞品是怎么实现的。」
- 目标：语音输入接口的优化绑定改为开源模型，首选 Qwen 系列，集成路径研究 MLX；豆包输入法只作机制参考 ｜ 阶段：调研 ｜ 轴：未指定 @ 2026-11-12
  原话：「确认，构建 workflow 进行详细工程分析，调研有什么开源模型可用，性能如何，首选 qwen 系列，并研究如何集成，集成路径研究 MLX。豆包输入法只作机制参考，开始调研填补空白。如果有任何阻力，调度 REA 工具用逆向工程的方法研究一下竞品，看看竞品是怎么实现的。」
- 目标：Rime 客户端提供语音输入接口，语音功能机制参考 Type4Me；识别复用其契约，交付不接剪贴板粘贴 ｜ 阶段：调研 ｜ 轴：未指定 @ 2026-11-12
  原话：「我决定以你的方案、决策和思路进行开发。」「我的目标是：这个 Rime 输入法客户端提供一个接口用来接入语音输入的功能。」「开发的这个语音输入法，功能机制参考 Type4Me 这个项目。」「这个东西非常通用，我认为你可以把它开发完毕。如果有任何阻力，你可以调用 REA 工具，通过逆向工程的方法研究一下竞品，先看看 Type4Me 是怎么实现的。」
- 目标：在这个 Rime 输入法客户端里实现上述语音接口 ｜ 阶段：产出 ｜ 轴：未指定 @ 2026-11-12
  原话：「我决定以你的方案、决策和思路进行开发。仍未确认蓝图，请自行调研填补空白未知，不再交由我来拍板。」「我的目标是：在这个 Rime 输入法客户端里实现上述语音接口。」
- 目标：按住右 Option 说话会走到引擎，松开后提交定稿 ｜ 阶段：产出 ｜ 轴：未指定 @ 2026-11-13
  原话：「我决定以你的方案、决策和思路进行开发。」「我的目标是：按住右 Option 说话，会走到引擎，松开后提交定稿。」
- 目标：语音转文字尽量没有延迟；调研可用开源模型与集成方式，首选 Qwen 系列 ｜ 阶段：调研 ｜ 轴：延迟 @ 2026-11-14
  原话：「现在语音可以转化为文字上屏，但是响应速度很慢。」「我希望语音转化为文字尽可做到没有延迟，调研有什么开源模型可用，性能如何，首选 qwen 系列，研究如何集成」

## 进入提醒
（容易踩的坑、推荐阅读顺序、术语特殊含义、来源存疑的已确立项。
**本区排在各内容区之前，因为它是读序路由器**——先看它，再决定下面怎么读）
<!-- 格式：- <一句话提醒>
     例：先读 001 再读 003，因为 003 显式推翻了 001 的结论
     例："XX"在本项目中指 YYY，不是通常意义上的 ZZZ
     例：已确立区第 N 条缺确认原话，来源存疑，用前与用户核对 -->
- refs/code/squirrel（HEAD b7970a9）与 refs/code/librime（HEAD bb56876）均为浅克隆只读参考源码，勿与 code-root 混用
- 本机 /Library/Input Methods/DoubaoIme.app（v1.1.0 / build 1010005）与 ~/Library/Application Support/DoubaoIme/ 为逆向分析只读外部样本
- 阅读 notes 时先读 001 全局索引与 005 Squirrel 源码分析，再按 002 → 003 → 004 → 006 顺序阅读
- 阅读本次语音接口文档时先读 007，再读 008～012，最后读 013；013 是未确认蓝图，其绑定决定写于目标改写之前
- 2026-11-12 起豆包输入法只作机制参考；先读 014 至 019，再按 020 → 026；026 只记仍然未知的问题
- refs/code/type4me 是浅克隆且 sparse checkout，HEAD 2835334，只读，勿与 code-root 混用
- 项目代码在 code/，不要写进 refs/code
- 接 Type4Me 时先读 027，再读 028；028 是候选对应，不是已确认接法
- 五项绑定以 033 为准；031 其余部分仍是未确认蓝图
- 024 的块中位只属于这次 M2 Pro、0.6B 4-bit、`feed_audio`；其余到达帧毫秒是未知，不借用这张表
- 读 029、030、031、032 时按这个顺序；031 文首是未确认蓝图，其交付不调用 inject，032 只记仍然未知的四处
- 读 034、035、037、038 时按这个顺序；037 文首是未确认蓝图，五项仍以 033 为准，交付不调用 inject；038 没有仍会挡住写代码的缺口；036 没有落地文件
- 读上屏对照时读 039。029 说部分文本不调用 inject，039 把这句话收窄到录音中的 transcript；流中断且当时 displayText 非空时会 inject
- 换机器装包先读 064：权重与 metallib 由 code/scripts/fetch-model.sh 取到 code/assets（不进 git、逐个核 sha256），打包与装包各一条脚本；Intel 与 x86_64 没有产物也没有数字
- `/tmp/voice-abi` 的驱动有好几份源码：`bench3.c` 编出的那份只 `fread` 前 32000 个采样（200 帧），会把长音频说成「只出前两块」；要比多块就用 `bench.c` 编、它真的读整段与 `argv[4]`（详见 073）

## 已确立
（用户**明确指令钉下的大方向**。稀有、用户专属。每条必须附：**确认原话** + 确认日期）
<!-- 格式：- <大方向>（确认原话：「……」｜确认：用户 @ YYYY-MM-DD） -->
<!-- 准入规则见 GUIDE §6「已确立」。agent 默认不写本区；确认原话缺失 = 来源存疑，不当作铁律。 -->
- 将 rime/squirrel 与 rime/librime 克隆至 dev-docs/refs/code/ 作为参考基线（确认原话：「三条候选全部确认。」「4. 同意，将 rime/squirrel 和 rime/librime 克隆到 dev-docs/refs/code/ 下作为分析材料」｜确认：用户 @ 2026-11-11）
- 架构设计须处理 IMKTextInput 同步 IPC 的隔离策略（确认原话：「三条候选全部确认。」｜确认：用户 @ 2026-11-11）
- 在不影响 Squirrel 现有架构、代码与配置兼容的前提下，提供接入离线本地模型与云端大模型的语音输入优化接口（确认原话：「三条候选全部确认。」「最好是在不影响 Squirrel 的现有架构与代码的情况下，提供这样一个接口让我可以调用大模型来优化语音输入。」｜确认：用户 @ 2026-11-11）
- 不触碰 Rime 输入法引擎本身，保持原有键盘输入方式完整可用（确认原话：「不要碰 Rime 输入法本身，让我依旧可以通过键盘来输入。」｜确认：用户 @ 2026-11-11）
- 语音输入接口的优化绑定改为开源模型，首选 Qwen 系列，集成路径研究 MLX；豆包输入法只作机制参考（确认原话：「确认，构建 workflow 进行详细工程分析，调研有什么开源模型可用，性能如何，首选 qwen 系列，并研究如何集成，集成路径研究 MLX。豆包输入法只作机制参考，开始调研填补空白。如果有任何阻力，调度 REA 工具用逆向工程的方法研究一下竞品，看看竞品是怎么实现的。」｜确认：用户 @ 2026-11-12）
- Rime 客户端提供语音输入接口；语音功能机制参考 Type4Me；识别复用其 PCM、热词与定稿契约；交付走预编辑与提交，不接剪贴板粘贴上屏（确认原话：「我决定以你的方案、决策和思路进行开发。」「开发的这个语音输入法，功能机制参考 Type4Me 这个项目。」｜确认：用户 @ 2026-11-12）
- 识别只用 feed_audio（Qwen3-ASR 0.6B 4-bit）：说话中的整段文本作部分文本，松开后再定稿一次；部分文本写成预编辑，定稿且改写记录到达后提交定稿原文；改写结果不上屏；只把热词拼成空格分隔字符串送给识别；按住说话，松开即收束（确认原话：「A D G J L」｜确认：用户 @ 2026-11-12）
- 在这个 Rime 输入法客户端里实现上述语音接口；方案中尚未填上的空白由调研补齐，不再逐项交给用户拍板（确认原话：「我的目标是：在这个 Rime 输入法客户端里实现上述语音接口。」「请自行调研填补空白未知，不再交由我来拍板。」｜确认：用户 @ 2026-11-12）
- 按住右 Option 说话会走到语音引擎，松开后提交定稿；安装包补齐引擎、metallib 与 0.6B 权重（确认原话：「我的目标是：按住右 Option 说话，会走到引擎，松开后提交定稿。」｜确认：用户 @ 2026-11-13）
- 语音转文字以「尽量没有延迟」为优化轴；模型首选 Qwen 系列（确认原话：「我希望语音转化为文字尽可做到没有延迟，调研有什么开源模型可用，性能如何，首选 qwen 系列，研究如何集成」｜确认：用户 @ 2026-11-14）

## 候选结论
（agent 调研/分析/推论产出、**待用户确认**的结论。agent 无权自行升级，不得在文档里写成已定）
<!-- 格式：- <候选结论>（源：notes/NNN-xxx.md｜待用户确认） -->
<!-- 准入规则见 GUIDE §6「候选结论」。用户说「先按这个方向走」≠ 确认某条候选。 -->
- 将 SquirrelInputController 保持为轻量级快捷键状态机与 IMKTextInput 上屏执行器，将离线 ASR 与 LLM 推理交由独立进程或可插拔后端承载（源：notes/002-research-doubao-ime-architecture-and-audio-pipeline.md｜待用户确认）
- 在 deactivateServer 生命周期中引入基于时间戳与录音会话状态的防误断保护窗口，防止系统权限弹窗或状态栏交互中断活动语音会话（源：notes/002-research-doubao-ime-architecture-and-audio-pipeline.md｜待用户确认）
- 采用预热音频采集缓存（SystemCaptureCache）与合盖状态感知机制复用 AVCaptureSession，消除按下语音键时的音频硬件冷启动延迟（源：notes/002-research-doubao-ime-architecture-and-audio-pipeline.md｜待用户确认）
- 在流式语音识别期间通过最长公共前缀（Common Prefix）打字机算法配合 setMarkedText 渲染行内增量预编辑文本，通过带事务 ID 的结算状态机与剪贴板回退通道完成最终上屏（源：notes/002-research-doubao-ime-architecture-and-audio-pipeline.md｜待用户确认）
- 使用 SQLite 维护用户历史纠错对（modify_pair）与个人高频热词（user_word），通过图消解规则与三段式配额召回构造多槽位结构化上下文提示词（源：notes/003-research-doubao-ime-local-models-and-inference.md｜待用户确认）
- 结合分词、逆文档频率（IDF）、命名实体识别与汉字拼音映射表从日常打字输入中静默提取个人专有名词热词，实现键盘输入与语音识别词库共享（源：notes/003-research-doubao-ime-local-models-and-inference.md｜待用户确认）
- 采用预热 WebSocket 连接池与流式音频分帧传输云端 ASR，并在尾点定稿后通过 HTTP SSE 流式接口（附带改写决策门控、内存缓存与一键撤销记录 UndoRecord）执行大模型语音润色（源：notes/004-research-doubao-ime-cloud-asr-and-llm-pipeline.md｜待用户确认）
- 构建统一的端云个性化上下文提供器（同时向云端接口与本地模型注入光标前文、宿主应用标识与个人热词），并在云端连接超时或离线时自动切换至本地模型（源：notes/004-research-doubao-ime-cloud-asr-and-llm-pipeline.md｜待用户确认）
- 在不修改 librime 的前提下，复用 Squirrel 现有五处非侵入式扩展接缝（SquirrelApplicationDelegate 服务挂载、handle(_:client:) 热键分流、show(preedit:...) 与 commit(string:) 行内上屏、SquirrelPanel 多版本候选展示、deactivateServer 会话守卫与只读状态提取）（源：notes/005-code-analysis-squirrel-index-and-imk-lifecycle.md｜待用户确认）
- 由输入法主 Bundle 唯一持有麦克风 TCC 权限声明与 audio-input 硬化运行时权利，并通过 NSDistributedNotificationCenter 响应独立配置面板的授权委托（源：notes/006-research-macos-ime-voice-llm-integration-mechanisms.md｜待用户确认）
- 采用 IMKInputController 本地修饰键掩码（NX_DEVICE*KEYMASK 区分左右修饰键）与按需 CGEventTap 双轨快捷键捕获架构（源：notes/006-research-macos-ime-voice-llm-integration-mechanisms.md｜待用户确认）
- 本地离线 ASR 集成 sherpa-onnx C API（流式 Zipformer 与整句 SenseVoiceSmall），本地 LLM 润色采用独立后台守护进程运行 llama.cpp 或复用 Ollama（搭载 Qwen2.5-1.5B 或 Qwen3-1.7B 并锁定前缀 KV 缓存）（源：notes/006-research-macos-ime-voice-llm-integration-mechanisms.md｜待用户确认）
- 语音优化接口是九条原语在同一会话上的组合，不另设总函数，也不另设命名预设；点名的离线模型与云端大模型接口只经适配器进入，可调用契约核对之前角色配对保持可换，不按制品所在地预先配对（源：notes/007-derivation-voice-input-primitives.md｜待用户确认）
- 呈现与交付的一种候选配对是：同一会话的部分假设只产生预编辑意图，改写流返回之后才产生一次提交意图；改写输出为「不替换」时，该次提交使用识别假设的定稿文本，且只在会话允许交付时执行。这不是原语 7 的不变量（源：notes/007-derivation-voice-input-primitives.md｜待用户确认）
- 会话把触发「收束已请求」与「会话已关闭」分成两位；字段标识偏离快照只把交付许可改为拒绝，不销毁假设，不把写入改道到新字段，也不在旧字段上补清理；失焦未经触发消费则会话不因此关闭（源：notes/008-derivation-voice-session-commit-and-keyboard-coexistence.md｜待用户确认）
- 部分假设只产生预编辑事务；改写结果未到时不发出提交意图，也不编造优化文本；收束之后至多一笔提交，提交成功后会话进入已关闭（源：notes/008-derivation-voice-session-commit-and-keyboard-coexistence.md｜待用户确认）
- 不修改键盘链，不把语音文本灌进组合；自有预编辑区间仍在时，未取走的键盘事件先停在键盘链之外，由语音事务结束该区间后再原样进入；同一字段至多一枚会话处于允许交付（源：notes/008-derivation-voice-session-commit-and-keyboard-coexistence.md｜待用户确认）
- 本地运行时适配器对外只做三送一收：主模型路径与上下文辅助目录、槽位快照、带结束标记的 PCM16 块；收回带部分或定稿种类的文本假设。角色候选是识别流。执行点留在官方进程内，进程外实例不成立；不打开容器、不送出密钥、不伪装输入法进程来填这个空位（源：notes/009-derivation-doubao-local-model-runtime.md｜待用户确认）
- 云端绑定的一种候选是两个适配器：豆包语音公开流式识别承担识别，方舟公开对话在 finish_reason 之后把定稿文本映射为「不替换」或「替换文本」；私有 Frontier、令牌获取与 text_organization 不进这对适配器，缺口上角色为空，不改成绕过（源：notes/010-derivation-cloud-asr-and-llm-binding.md｜待用户确认）
- 会话打开时只读汇集一份上下文快照，槽为 host_app、field_role、cursor_before、有序 hotwords、有序 corrections，任一槽可缺席；本地槽位与云端优化请求只做同一顺序的投影，不重排，不把缺席写成空字符串；改写结果不写回快照或来源（源：notes/011-derivation-voice-context-memory.md｜待用户确认）
- 当前汇编不采用「自有预编辑仍在时把未取走事件停在键盘链之外」：未取走的键盘事件仍原样进入键盘链，不修改键盘链，不把语音文本编码成按键（源：notes/013-blueprint-voice-interface-v1.md｜待用户确认）
- 当前汇编不采用「同一字段至多一枚允许交付」与「同一会话至多一笔成功提交且随即关闭」：同一字段可以有多枚会话同时允许交付；同一会话可以先后成功多笔提交；呈现序列甲与序列乙都合法；提交成功不把会话迁到已关闭（源：notes/013-blueprint-voice-interface-v1.md｜待用户确认）
- 当前汇编不采用把点名云端制品拆成公开流式识别与公开对话：等不到可调用契约时角色空着，不改等另一套公开产品。两枚点名绑定此时都是空角色；核对用的替身不打开容器、不连接私有主机、不装成官方输入法进程（源：notes/013-blueprint-voice-interface-v1.md｜待用户确认）
- 绑定按制品声称的输入类型接角色，不按是否在本机换角色。识别流在 MLX 上消耗已经成形的波形、并把快照中的文本放进系统提示时，候选制品是 mlx-audio 的 Qwen3-ASR。识别流在帧到达时调用 feed 再 step 时，已读的 Qwen3-ASR 源码没有这个函数，候选制品是 Voxtral Realtime 的 VoxtralStreamingSession。两行互相替换时，原语 4 与原语 5 的对外类型不变。Qwen 文本权重填不上识别流，Qwen3-ASR 填不上改写流，Voxtral 不兼任改写流。用户对 Qwen 的偏好不把 Voxtral 的这一档输入写成 Qwen 已经具备（源：notes/018-derivation-open-model-mlx-binding.md｜待用户确认）
- 改写流的候选制品是 mlx-lm 或 mlx-swift-lm 能按 qwen2、qwen3 或 qwen3_5 加载的 Qwen 指令权重。「不替换」由适配器比较全文得到，不由模型返回。替换文本只取模型卡写明不再生成思考块的指令权重的最终回答。候选停在输出类型，不把字符串差异写成优化已经成立（源：notes/018-derivation-open-model-mlx-binding.md｜待用户确认）
- 制品只给出追加片段时，候选包法是把上一条文本接上该片段当作新假设的文本，修订目标仍指向上一条。映射发生在适配器，不证明模型会改掉已经交出的字（源：notes/018-derivation-open-model-mlx-binding.md｜待用户确认）
- Voxtral 的 done 在 EOS、达到 max_tokens 或关闭后把尾巴解完时置位，可以候选地映射成原语 4 的「分句已定稿」；映射发生在适配器，函数本身不接收会话（源：notes/018-derivation-open-model-mlx-binding.md｜待用户确认）
- Qwen2.5 的 Instruct 权重、Qwen3 混合思考权重在 non-thinking 开关下、模型卡写明不再生成思考块的 Qwen3-4B-Instruct-2507，以及 Qwen3.5、Qwen3.6、Qwen3.8 模型卡写明的 Instruct 模式，在输入类型上落在改写流的文本口。候选只到文本口的形状符合原语 5，不到生成文本应当替换识别文本（源：notes/016-research-qwen-text-models-performance.md｜待用户确认）
- 已经显示出去的字要不要改、改的是屏幕上的哪一段，放在拿到文本之后做。moona3k 的 `feed_audio` 交出新的整段 `state.text`，比较材料与豆包交给 `ASRTypeWriter` 的部分结果同一类，都是整段字符串，不是修订指针。mlx-audio 的 Qwen3-ASR 与 Voxtral 的 `step` 只交出新切片，切片不指向此前任何一条。调用方另存的上一条标识仍然只在调用方（源：notes/021-research-hypothesis-revision-pointer.md｜待用户确认）
- 五槽里只有 `hotwords` 在已读 Qwen 入口有落点，且不是五槽对象：同名列表只在 mlx-audio Python 的 `generate`；官方包、moona 与 Swift 是 `context` 字符串，Hugging Face 模型卡是 `prompt`。`host_app`、`field_role`、`cursor_before`、`corrections` 在已打开入口没有形参；Voxtral 构造参数没有这些名字。不把没有读到的四槽写进 `context` 或 `prompt`，也不新写提示词模板；拼接方式保持未知（源：notes/022-research-context-slot-injection.md｜待用户确认）
- 替换是否更少错误只在写明检查点与测试集的行内记录方向。Common Voice zh-CN 抽样 1,000 条上，直接接受 `Qwen2.5-3B-Instruct`（4-bit NF4，贪心，1-best）的输出时，Macro CER 与 Corpus CER 都高于同表 Whisper。Arena Breakout 表 III 的 `Qwen-72B w/o SFT` 与 `Qwen-1.5B w/ SFT` 的 CER 低于该文最佳单路识别，但 Instruct 或 Base 与仓库路径未写明，提示也不同。其余档相对识别假设的替换 CER：未知。「不替换」仍是正常输出，不把两套口径的差异交给呈现去替换（源：notes/023-research-rewrite-improves-hypothesis.md｜待用户确认）
- Apple M2 Pro、16 GB、`mlx-qwen3-asr` 0.4.4、`moona3k/mlx-qwen3-asr-0.6b-4bit`（解码器 4-bit、组 64，音频塔 8-bit、组 64）、2.0 秒 `feed_audio`、预热后 10 次下，块序 1 至 7 各有一份中位，回答到达中的帧到一份能改写上一份 `text` 的假设；七个中位不收成一个数。第 0 次计时会话里 `stable_text` 只变长，不能当成整段假设都被改写。`rewrite_events` 不是修订标识。0.6B 8-bit 与 1.7B 这次没有加载，整段 `transcribe` 未测，记为未知。尾部未送入，收尾解码未知（源：notes/024-research-streaming-frame-latency.md｜待用户确认）
- 整段波形仍接 mlx-audio 的 Qwen3-ASR，五槽里只有 `hotwords` 对得上 `List[str]`；Swift 的 `context` 字符串单独成行。帧还在到达时，追加切片仍是 Voxtral 的 `feed` / `step`，整段新文本补上 moona3k 的 `feed_audio`；两行不互挑，也不串成前后两级。修订次数不是修订指针。时延只引用该篇记下的 M2 Pro、0.6B 4-bit、`feed_audio` 块序 1 至 7 的中位，其余路径的到达帧毫秒为未知。改写仍停在输出类型，不从两张不同口径的 CER 表挑权重。豆包的公共前缀和留下原文只在适配器外面，不进入制品（源：notes/025-derivation-gap-fill-binding.md｜待用户确认）
- Type4Me 本地识别是 SenseVoice 流式部分结果加本机 Qwen3-ASR MLX WebSocket 定稿；热词进入 transcribe 的 context 字符串；文本优化的本机实现是 llama.cpp 或外部 HTTP，不是 mlx-lm（源：notes/027-research-type4me-pipeline.md｜待用户确认）
- Type4Me 上屏是剪贴板加合成 Command-V，识别与注入代码在可执行目标内，包产品不导出这两层（源：notes/027-research-type4me-pipeline.md｜待用户确认）
- 候选接法只复用 Type4Me 的 PCM16、16 kHz、热词字符串与定稿文本契约，交付仍走预编辑与提交，不调用 TextInjectionEngine.inject（源：notes/028-derivation-type4me-seam.md｜待用户确认）
- Type4Me 热键链在 `RecognitionSession.finishTextOutput` 里、识别定稿与 `LLMClient.process` 之后调用 `injectTracked`（`RecognitionSession.swift:2700`）或 `inject`（`:2709`）；部分文本不调用 `inject`（源：notes/029-code-analysis-type4me-session-orchestration.md｜待用户确认）
- 语音接口按部分文本、再定稿、再对定稿全文改写来实现；交付记录只有预编辑与提交，不调用 `TextInjectionEngine.inject` 或 `injectTracked`（源：notes/030-derivation-type4me-voice-implementation.md｜待用户确认）
- 没有点名仓库路径上的 MLX 纠错分数时，改写全文不同只留在改写记录，预编辑与提交的字符串取识别定稿原文（源：notes/030-derivation-type4me-voice-implementation.md｜待用户确认）
- 部分文本与定稿是同一条识别记录的两种种类，不把 SenseVoice 与 Qwen3 串成前后两级（源：notes/030-derivation-type4me-voice-implementation.md｜待用户确认）
- 未确认蓝图的交付只消费预编辑或提交意图，不调用 `inject`、`injectTracked` 或剪贴板合成按键；异议清单没有条目被吸收（源：notes/031-blueprint-type4me-voice-interface-v1.md｜待用户确认）
- 语音复用 show 之后，普通键的 handle（95-116）与 processKey（402-435）不读控制器 preedit，也不读标记文本；返回后 rimeUpdate 用 librime 组合文本（未组合则为空串）盖过该缓存并再 setMarkedText。get_commit 成功时 insertText 的是 librime 提交串并把缓存清空。insertText 与 setMarkedText 不共用这份缓存作载荷。commitComposition 提交的是 get_input 的原始编码 input_，不是控制器 preedit。字段失焦是否进入该函数，以及这些 IMK 调用在已有标记文本上的屏幕结果，源码未写出（源：notes/034-code-analysis-squirrel-marked-text-and-keys.md｜待用户确认）
- 松开后的定稿是 finish_streaming（session.py 只转发）。缓冲非空时吃掉 state.buffer 里未满块的剩余采样再解码；空缓冲不解码。返回的 state.text 是已提交窗口加活动窗口的整段，与说话中 feed_audio 的 state.text 同一种，不是增量（源：notes/035-code-analysis-feed-audio-final-pass.md｜待用户确认）
- 未确认蓝图把按住说话、feed_audio 的整段部分文本、松开后 finish_streaming 的整段定稿原文、预编辑后提交定稿原文、改写记录不上屏、只送热词写成一份实现；交付只调用 show 与 commit，不调用 inject 或 injectTracked；异议清单没有条目被吸收（源：notes/037-blueprint-voice-interface-v2.md｜待用户确认）
- 按 037 写代码时没有仍会挡住的未知；037 标成源码未写的屏幕结果、替换范围、失焦入口、尾块字符差、收尾时延，以及替换文本是否错误更少，都不进入这些调用（源：notes/038-gap-voice-implementation-remaining.md｜待用户确认）
- 这次 voice.log 的未上屏发生在识别回复被拒绝，voiceShow/voiceCommit 未被调用；不推翻 show/commit 时，修改落在 voicePythonURL 所选解释器（源：notes/039-code-analysis-type4me-text-commit.md｜待用户确认）
- Type4Me 外部应用上的最终文字是 NSPasteboard 写入后合成 Command-V，自己的文本框是 NSTextView.insertText，不是 IMK 的 setMarkedText/insertText。改用该通道需要另行确认（源：notes/039-code-analysis-type4me-text-commit.md｜待用户确认）
- Type4Me 录音中的 transcript 只进悬浮条；流中断且当时 displayText 非空时 injectRecoveryPartial 会调用 inject。恢复成功后的整段在 finishRecovery 里不再次调用 inject（源：notes/039-code-analysis-type4me-text-commit.md｜待用户确认）
- 语音识别是进程内的 Rust 算子图，MLX 只提供卷积、注意力与解码运算；安装包携带 0.6B 4-bit 权重与 metallib，不携带 Python（源：notes/041-derivation-rust-mlx-voice-engine.md｜待用户确认）
- 0.6B 音频塔为 18 层、d_model 896、三层 stride-2 卷积茎；文本塔为 28 层 Qwen3、隐层 1024、16/8 头；流式状态与该图同在 Rust 线程内（源：notes/042-derivation-rust-qwen3-asr-engine.md｜待用户确认）

## 进行中
（**所有在轨的活**：探索中的问题、被内部工作阻塞的、拆分的第二块、预算中断的余量、等用户决定的。
每条说明下一步。外部硬依赖阻塞、客观做不了的 → 「不建议」区）
<!-- 格式：- <事项>（下一步：xxx｜来源：<探索/内部阻塞/拆分②/预算余量/待用户决定>｜阻塞于：<内部阻塞源，无则省略本字段>）
     · 来源 是必填的：拆分② 与 预算余量 是**已承诺的交付余量，只能由完成来关闭**，
       不带标记就会退化成普通探索项而被无声丢弃（GUIDE §6「进行中」）。
     · 「等用户决定」写 下一步：问用户 X —— 待问的事项属于这里，不属于「不建议」。 -->
- 麦克风到文字的识别时延仍未知：这条路径现在能出文本，探针的暖定稿已有 0.178 秒的中位（三次；`dev` 档案、页缓存有内容；冷的那一次要等第二段，0.823 秒六次中位），装好的包上 2 秒说话那一档按下→出字 2.229–2.291 秒，但没有真实麦克风语句（下一步：有麦克风语句后从按下到上屏记一次墙钟｜来源：探索）
- 新冻窗那一步仍整窗重算：14 秒探针第 4 步重算 113 个位置（其它步 35–61），缓存里那 104 行是按活动窗编出来的，窗一满就得按整窗重编（下一步：要省这一步得让编码器对首端因果，落在编码器这一层而不是缓存｜来源：探索）
- 复用与整段预填充的 logits 余量只有 1.02 倍（实测差 7.87 个单位、容差 8 个单位）：六步、30 秒十四步、带已固定文本七步都全对，但贴着 argmax 的两个 token 仍有被换掉的可能（下一步：真实长句里第一次出现差一个 token 时，先看那一步 logits 前两名差多少，再决定要不要把复用边界往前收｜来源：探索）
- 定稿仍整段重算，松手到上屏不借最后一步：暖定稿 0.178 秒（2 秒探针 44 个位置整条预填充）（下一步：量一次长句定稿里整段重算占多少，再决定要不要让 `finish` 接上最后一步的缓存｜来源：探索）
- 一步里的三段串在引擎线程上：12 秒处梅尔 0.035 + 编码 0.030–0.053 + 解码 0.107 秒，编码随活动窗宽（200/400/600/800 帧）走、与已收音频无关（下一步：先看梅尔与编码能不能与解码重叠，再谈并行｜来源：探索）
- 贪心按 token 计费，已经是每步最大的一块：12 秒处一步 0.178 秒里解码 0.107、其中预填充只占 0.041，剩下 0.065–0.075 是两次贪心前向（约 42 毫秒/token），`lm_head` 是对 151 936 行按 5 块 × 32768 行做 argmax（下一步：量一次长句定稿里贪心占多少，再决定动不动 `lm_head` 分块或跳块 argmax｜来源：探索）
- 第二段里分词器 0.393 秒占 2/3：`Tokenizer::load` 给 151 643 条各做一次 `Vec<u8>` 堆分配并建两张 `HashMap`（下一步：改成一遍扫下来的偏移表 + 大缓冲，会动 `voice-tokenizer` 的对外形状｜来源：探索）
- 热词没有进提示：build_prompt 有 `context` 位置（参考塞进 system 段），但 voice-ffi 收下的 hotwords 没有传给 transcribe，这条调用链现在是空字符串（下一步：把 hotwords 映到 system 段并核对参考拼法｜来源：探索）
- 预热窗口现在只要 0.13–0.14 秒（进程起→可接受按键），但这个窗口里的按键仍被拒、本次语音丢失：排队回放没做（下一步：要做排队回放得改 `code/Client` 的预热接线｜来源：探索）
- 多窗那一支仍没有逐窗参考：六段真实语音最长 5.09 秒、只有 1 个注意力窗；13.82 秒那段只证了整段被吃完与两条喂法给同一串字，`windows >= 20`（约 160 秒）按窗切开的支路一次都没走到（下一步：造一段有参考文本的 9 秒以上语音，记窗边界与逐窗文本｜来源：探索）
- 装好的包上没有真实按键过一次：六段验收与逐层闸都走 `/tmp/voice-abi/bench7`，麦克风采集与 Swift 侧 `voice_session_feed` 的调用方不在量程（下一步：按一次右 Option 说一句 4 秒以上的话，看定稿里有没有第一块之后的字｜来源：探索）
- 所有秒数都是 `dev` 档案：`release` 的读盘、反量化与梅尔都还没有量（下一步：用 `release` 重编归档并重测同一探针｜来源：探索）
- 参考的逐层资产只有音频塔 layers[0]：文本塔 28 层与 argmax 处的 logit 都没有参考，只能经末尾 logits 与 token 序列间接约束（下一步：要给逐层差值就得改 voice-oracle 的导出脚本补隐层与全词表 logits｜来源：探索）
- voice-parity 里四个 `#[ignore]` 入口不跑数值：编码器、解码器、logits、token 序列的实对照在 voice-mlx 的三套测试里，本箱子只用 std 读资产（下一步：若要在这箱子里也跑，就得让它链接 mlx-rs；当前边界是 052 定的｜来源：探索）
- 回调是否真的到达过一次仍未知：voice.log 的两行 audio engine started 也能由两个 IMKInputController 各按一次解释（下一步：在 receive 里记一行会话号与 kind，再看一次日志｜来源：探索）
- 看门狗按「最后一次回话」起算：松手丢了而引擎一直回非空部分结果时窗口被一次次续上，麦克风不会停；纯静音不会续窗，空串部分结果不下发（下一步：先量一次真实长句与连续说话的中位时长，再决定要不要另加一条按住时长上限｜来源：探索）
- 麦克风授权回调与取消交错时采集会被重新起起来：`EnginePCMSource.start` 的 `granted` 分支不看 `running`，会话已被取消之后仍会 `startEngine`，此时表已撤、`push` 只丢帧（下一步：授权回调里先看 `running` 再 `startEngine`，并补一条走假授权的用例｜来源：探索）
- 多枚输入控制器各自一枚门与采集：`voiceGate` 与 `EnginePCMSource` 都是每个实例一份，取消只作用于被系统调到的那个实例；号已改成全进程唯一并写进 `voice.log`，两枚门同时说话不再互撞号，但两个采集源并行的内存与时延没有量过（下一步：在 voice.log 里同时记控制器实例与门会话号，确认实际有几枚实例在活着｜来源：探索）
- 换窗口之后第一句在真机上没按过：号不重复由 Swift 用例证、三枚号各自 begin 引擎都回 0 由引擎线程用例与 `/tmp/voice-abi/session_probe` 证，但输入源还没启用（`AppleEnabledInputSources` 里没有 `im.dragonshock.inputmethod.RimeClient`），没有真实按键也没有麦克风里的语音（下一步：启用这条输入源，在窗口 A 与窗口 B 各按住右 Option 说一句，看 voice.log 有没有 `rejected 会话未被接受` 或 `replaced`｜来源：探索）
- Hans 在 TISEnableInputSource 返回 0 之后，输入源列表里的 enabled 仍是 false，HIToolbox 的已启用列表里也没有这条源（下一步：核对注册之后这条源为什么没有写成启用｜来源：探索）
- 只有一份数值内核（MLX/Metal/arm64）：`voice-tensor` 的 `TensorOps` 第二份实现只有不做数值的 `FakeBackend`，Intel 与 x86_64 从未编过、没有数字（下一步：换后端要凑齐 `TensorOps` 的运算清单并重跑梅尔与编码器、解码器三道闸，容差不改｜来源：探索）
- 「换一台机器」只在同一台机上换目录验证：`fetch-model.sh`、`make-app.sh`、`install-app.sh` 三条都跑通，但 `LIBRIME_PREFIX` 与带 metal 的 `cargo build` 仍是构建机前置（下一步：找一台没装过本仓库的机器，从取资产跑到装包记一次墙钟｜来源：探索）

## 不建议
（**唯一合法住民 = 存在具体外部硬依赖、当前客观做不了的事**。每条必须四字段齐全。
不是工作量的出口，也不是停车场——工作量大但做得了的，进「进行中」）
<!-- 格式：- <任务>（不建议理由：<具体外部依赖，非工作量>｜不做的代价：xxx｜激活条件：<该依赖如何解除>｜源：notes/NNN-xxx.md） -->
<!-- 准入规则见 GUIDE §6「不建议」。缺「不做的代价」的条目记为「未写明」并与用户补齐，不因缺字段而丢弃。
     本区是 GUIDE §1 工作量禁令的**两个授权例外之一**（另一个是「用户延后」）。 -->

## 用户排除
（用户**明确指示不做**的路径。**唯一准入依据 = 用户指示**）
<!-- 格式：- <被排除的方案>（原因：xxx｜准入依据：用户指示 @ YYYY-MM-DD｜源：notes/NNN-xxx.md） -->
<!-- 准入规则见 GUIDE §6「用户排除」。agent 无权以自己的判断把条目放进本区。 -->
- 修改 Rime 输入法核心引擎（librime）本身或破坏原有键盘输入路径的方案（原因：用户要求不触碰 Rime 输入法本身并保持原有键盘输入｜准入依据：用户指示 @ 2026-11-11｜源：notes/001-research-voice-ime-index.md）

## 用户延后
（用户明确说「先不做 / 以后再说」的事。含义是**还没想清楚到能做**，不是不做——与「用户排除」不同。
准入依据 = 用户指示。**追加式列表区——每条都持续有效，不是读最后一行**，区别于「目录目标」「code-root」两个单值区。
agent 对本区条目：不设计、不预埋扩展点、不当作 gap 去填，**不得拿「延后等于 bug」反驳用户**。
但 **MAY 问**被延后项对在建部分的接口影响——问不是脑补设计）
<!-- 格式：- <被延后的事>（原话：「……」｜用户 @ YYYY-MM-DD｜与在建部分的耦合：<一句话，或「无」>）
     · 耦合字段只记两样：① 在建部分做了什么假定 ② 该假定波及哪些位置。
       写**属性**（"当前请求不携带身份信息"），不写**机制**（"无鉴权中间件"）——
       写机制等于替被延后项定了实现形态，那是设计不是记录。
       更不得出现"需要 / 应该 / 建议 / 方案"这类处方措辞。
       在建部分尚不存在时（init 常见），本字段填「无」。
     · 「不碰」的范围仅限用户点名的那件事，既不得外扩到相邻工作，
       也不得把明显属于被延后项内部的东西说成「相邻」照做；分不清时默认按被延后项处理，去问。 -->

## code-root
（项目编码所在目录。所有代码必须位于此路径内；也是 git 提交的默认根路径，项目可改。
**本区记录的是约束，不是已观察到的事实**——调研结束前目录结构往往还不存在，
先把用户声明的约束落在这里，日后真正建立目录/仓库时必须遵守。
**读法：直接看本区最后一行**；上面各行是历史，**agent 不得删改；用户指令更正除外**）
<!-- 格式：- 路径：<路径> ｜ 子目录结构：<一句话> ｜ git 根：<单一 / 每 X 各自独立> @ YYYY-MM-DD
     · 路径写用户声明的路径。若用户的既定约定就是把代码放在名为 code-root 的目录里，
       那么 `code-root/` 就是正确答案，照写。
     · 用户未给出的单个字段填「未定」，已给出的字段照写，不要因为一个字段未知就整行放弃
     · 三个字段全未给出时，整行写：- 路径：未确认（待与用户核对）
     · ⚠️ 本区只放**约束**。目录是否已存在、git 是否已建 等观察事实不写进来，
       要记就记进「进入提醒」——把观察塞进约束字段会让本区失去可执行性。
     · 「未确认」占位行被真实路径取代后**可删**（本区唯一可删的例外）
     · 槽位值须自带行首「- 」，格式如上；不要再补一个。 -->
- 路径：~/repo/tools/ ｜ 子目录结构：未定 ｜ git 根：未定
- 路径：~/repo/tools/rime-client ｜ 子目录结构：未定 ｜ git 根：单一（https://github.com/Dragonshock/rime-client.git） @ 2026-11-11
- 路径：~/repo/tools/rime-client ｜ 子目录结构：项目代码在 code/ ｜ git 根：单一（https://github.com/Dragonshock/rime-client.git） @ 2026-11-12

## refs 索引
（`refs/` 下参考材料的目录，按存放位置，每条一句话定位「这是什么」。**新增或克隆参考时必须同步追加**。
  `refs/code/` — 非本项目的代码：第三方 / 恢复 / 竞品源码克隆，以及编码规范、命名、lint。
  `refs/document/` — 非本空间 notes 的文档：第三方 API 文档、协议规格、RFC 摘录。
  **不是 code-root**：本项目代码写在 code-root（约束见上区），不写进 `refs/code/`）
<!-- 格式：- refs/<相对路径> — <一句话：这是什么>
     例：- refs/code/ghidra — NSA 反编译器，P-code / SLEIGH
     例：- refs/document/rfc7540 — HTTP/2 规格
     · 一行一条。浅克隆、HEAD、只读、勿与 code-root 混用 等读序陷阱写进「进入提醒」，不写进本区。
     · `.gitkeep` 不算条目；只有 .gitkeep 的目录 = 空。
     · 本区是目录，不是约束；路径 / 布局 / git 拓扑仍归 code-root。
     · 准入规则见 GUIDE §6「refs 索引」。 -->
- refs/code/squirrel — Rime 官方 macOS 客户端 Squirrel 源码
- refs/code/librime — Rime 输入法核心引擎 librime 源码与 C API 头文件
- refs/code/type4me — macOS 菜单栏语音输入应用 Type4Me 源码

## notes 索引
（所有 notes 的目录，按序号，每条一句话定位「这篇在解决什么」。**新增 notes 时必须同步追加**）
<!-- 格式：- NNN-<类型前缀>-<主题> — <一句话：这篇在解决什么>
     类型前缀见 GUIDE §3：research- | code-analysis- | derivation- | design-(稀有) | gap- | blueprint-
     整名型：NNN-synthesis-<主题> | NNN-crystallization-<主题>（多篇共存，各带主题可区分）
     一个文档一个类型一个职责；揉杂 = 该拆开的信号。 -->
- 001-research-voice-ime-index — 语音增强型 Rime 客户端首轮调研全局索引与预分配编号表
- 002-research-doubao-ime-architecture-and-audio-pipeline — 豆包输入法包结构、进程拓扑、双轨音频采集与 IMK 三级上屏状态机逆向分析
- 003-research-doubao-ime-local-models-and-inference — 豆包输入法离线语音模型、端侧大模型推理栈及加密个人上下文数据库逆向分析
- 004-research-doubao-ime-cloud-asr-and-llm-pipeline — 豆包输入法云端流式语音识别、大模型语音润色 SSE 协议与端云协同机制逆向分析
- 005-code-analysis-squirrel-index-and-imk-lifecycle — Squirrel 与 librime 源码全局索引、IMK 会话生命周期与非侵入式语音扩展接缝分析
- 006-research-macos-ime-voice-llm-integration-mechanisms — macOS 输入法麦克风权限、本地离线推理引擎与云端流式大模型后处理集成机制调研
- 007-derivation-voice-input-primitives — 语音优化输入的九条可替换原语，以及不另设总函数的会话组合
- 008-derivation-voice-session-commit-and-keyboard-coexistence — 语音会话的收束与关闭、预编辑与提交事务，以及与键盘链共存的候选约束
- 009-derivation-doubao-local-model-runtime — 豆包离线模型在官方进程内的送音与假设形状，以及进程外适配器不成立的边界
- 010-derivation-cloud-asr-and-llm-binding — 云端识别与定稿文本优化的候选契约，以及私有长连接不当绑定端点的边界
- 011-derivation-voice-context-memory — 一次语音优化的五槽上下文快照，以及本地槽位与云端请求的同序投影
- 012-gap-voice-interface — 键盘预编辑相遇、提交历史、离线可调用契约与云端绑定四处仍未闭合的缺口
- 013-blueprint-voice-interface-v1 — 未确认蓝图：九条原语在一次会话上的开发参照，点名制品在无契约时为空角色
- 014-research-open-model-mlx-index — 开源模型与 MLX 集成本轮调研的目标切分与预分配编号
- 015-research-open-asr-models-performance — 开源识别模型已发表的体积、时延、中文效果与是否流式，对照可修订文本假设
- 016-research-qwen-text-models-performance — Qwen 文本各档已发表的体积、生成速度与标点、纠错结果，对照改写流的文本口
- 017-research-mlx-qwen-integration — MLX 与 MLX-Swift 加载、量化并流式运行文本 Qwen 的调用序列，以及识别权重不走这段调用
- 018-derivation-open-model-mlx-binding — 已核对的权重与 MLX 调用如何对应识别流、改写流和绑定，倾向标为候选
- 019-gap-open-model-mlx — 开源模型与 MLX 路径上仍未闭合的时延、修订指针、槽位名与替换效果
- 020-research-gap-fill-index — 四处空白的切分：修订指针、上下文槽位、替换是否更正确、到达帧时延
- 021-research-hypothesis-revision-pointer — 下一次输出会不会改掉已经交出的字，以及返回值里有没有标明改的是哪一条
- 022-research-context-slot-injection — 五槽快照里哪些名字能进入已有的 Qwen3-ASR 或 Voxtral 入口
- 023-research-rewrite-improves-hypothesis — 哪一档 Qwen、哪一个测试集证明替换文本的错误少于识别假设
- 024-research-streaming-frame-latency — 本机上到达中的帧到一份可改写上一份文本的假设要多久
- 025-derivation-gap-fill-binding — 021 至 024 的事实如何改写 018 的候选绑定
- 026-gap-after-mechanism-pass — 机制核对之后仍然未知的修订指针、四槽拼法、其余路径时延与改写是否更正确
- 027-research-type4me-pipeline — Type4Me 的本地识别、文本优化与剪贴板上屏机制
- 028-derivation-type4me-seam — Type4Me 哪一截对得上识别流与改写流，上屏为何不一并接入
- 029-code-analysis-type4me-session-orchestration — Type4Me 热键链从快捷键到 inject 的会话编排，部分文本不调用 inject
- 030-derivation-type4me-voice-implementation — 按识别再改写的顺序收成候选实现，交付停在 inject 之前
- 031-blueprint-type4me-voice-interface-v1 — 未确认蓝图：Type4Me 数据顺序上的语音接口开发参照，交付不调用 inject
- 032-gap-type4me-voice-implementation — 实施方案仍然未知的识别入口、四槽拼法、纠错分数与宿主字段相遇
- 033-crystallization-voice-binding-adgjl — 用户拍板的五项绑定：feed_audio、预编辑后提交原文、改写不上屏、只送热词、按住说话
- 034-code-analysis-squirrel-marked-text-and-keys — 语音复用 show 与 commit 之后，普通键、预编辑缓存与 commitComposition 碰到的状态
- 035-code-analysis-feed-audio-final-pass — 松开后的定稿是 finish_streaming，返回整段 state.text
- 037-blueprint-voice-interface-v2 — 未确认蓝图：按住说话、feed_audio 部分文本与松开后定稿原文的语音接口开发参照
- 038-gap-voice-implementation-remaining — 按 037 写代码时没有仍会挡住的未知
- 039-code-analysis-type4me-text-commit — Type4Me 最终文字的上屏 API，以及不改 show/commit 时解释器能改什么
- 040-derivation-voice-commit-fix — 修改方案：识别进程改用带模块的解释器，同一失败只记一次，文字到达后仍走 show/commit
- 041-derivation-rust-mlx-voice-engine — 候选：Rust 进程内持有 Qwen3-ASR 算子图，MLX 为张量后端，安装包不含 Python
- 042-derivation-rust-qwen3-asr-engine — 全量候选：Rust 按层重建 0.6B 编码器与 Qwen3 解码器，安装包内无 Python
- 043-blueprint-rust-voice-engine-v1 — 未确认蓝图：code/voice-engine 的箱子划分与无 Python 的验收
- 044-research-reinstall-usability-and-timing — 重装后的签名、Hans 输入源、VoiceEngine 入口与启动及 feed_32000 墙钟；麦克风到文字时延未知
- 045-derivation-voice-mlx-metal-link — voice-mlx 接 mlx-rs 0.34 的 Metal，voice-ffi 在 metal 开关两侧各用一个后端
- 046-derivation-bundle-voice-assets — 安装包补上 metallib 与解开符号链接的 0.6B 权重，空定稿不提交
- 047-research-installed-voice-engine — 已安装包的 metallib、权重、voice_engine_new 与一次引擎创建；识别时延未知
- 048-research-voice-path-self-check — 按住右 Option 到上屏的逐段自检：第 4 段解码只有单 token，是链上第一处确定断点
- 049-research-voice-path-verification — 修完第 3、4 段之后每步实测：回复同形、失败原因进日志、单 token 不提交、包已重装
- 071-research-real-speech-comparison — 真实中文语音对照：2 秒逐字一致，超过 2 秒被截断在多块批量路径
- 050-research-mel-and-tokenizer-parity — 梅尔前端与分词器已与参考实现逐元素对齐，编码器数值前向仍缺
- 051-research-oracle-dump — 参考实现导出的逐层量与提示词 id，作为重写的验收基线
- 052-research-parity-harness — 逐层比对的工具与四道闸的初始状态
- 053-research-encoder-parity — 编码器与参考逐层对齐，投影差 1.2e-3
- 054-research-decoder-parity — 预填充 logits 差 4.7e-2（3 个 f16 ULP），top-1 一致
- 055-research-end-to-end-parity — 探针走完全链路，token 与文本与参考一致
- 056-research-latency-baseline-and-model-options — 2 秒音频 finish 实测 12.5 秒，瓶颈是每次重读权重，不是模型选择
- 057-research-resident-weights — 权重与梅尔前端在引擎线程上常驻：冷 finish 0.807 秒、同进程暖 finish 0.466 秒，token 与文本不变
- 058-research-quantized-weights — 文本塔七个投影与词嵌入表直接吃打包 4 位权重，冷 finish 0.424–0.486 秒、暖 0.405–0.424 秒
- 059-research-latency-after-residency — 把已有的常驻实例真正装进包：2 秒探针 finish 三次中位 0.443 秒（对 12.534 秒），token 与文本不变，包已重装
- 051-research-oracle-dump — 参考实现导出六件逐层资产与探针，manifest 记形状、dtype 与来源函数
- 052-research-parity-harness — voice-parity 对照箱子：梅尔实测最大差 3.2e-4，编码器/解码器/logits/token 四个空入口
- 053-research-encoder-parity — 音频塔接上真实权重：卷积茎差 0、第 0 层差 3.90625e-3、投影差 1.1855885e-3，解码器仍缺
- 054-research-decoder-parity — 28 层文本塔能跑通：预填充末尾 logits 前 64 维差 4.6875e-2，全词表 argmax 与参考贪心首 token 一致
- 055-research-end-to-end-parity — 一段探针走完梅尔、编码器、预填充、逐 token 贪心与分词器：token 序列 [106287, 1773, 151645] 与文本「嗯。」都对上参考，定稿已接回这条路径
- 060-research-parity-guardrails — 容差口径收到 `voice-parity/src/tolerance.rs` 一处，并加两条可证伪的归因断言；主闸仍 7.8125e-2 对 8.203125e-2
- 061-research-warm-start — 引擎预热挪到启动后：预热完成按键 0.003 秒、定稿 0.455 秒，预热窗口内的按键按预算拒掉
- 062-research-mel-fft — 梅尔每帧换成五条 80 点 vDSP 实数 DFT 合并：0.272 → 0.050 秒，梅尔差 3.2e-4 → 6.9e-5
- 063-research-streaming-partials — 说话中每满 2 秒回一条部分结果：4 秒语音 2.40 秒出字，定稿逐字不变
- 064-research-portability-boundaries — 权重与 metallib 的取法与哈希、make-app.sh 与 Package.swift 参数化、装包脚本，以及 arm64/Metal/Intel 另需内核三件事实与 `TensorOps` 落点
- 065-research-load-breakdown — 装载四段分开量：转换 3.731 秒是大头（读取 0.391、eval 0.491、表头 0.0005）；改成按文件位宽直接建数组、末尾一次 eval 之后 engine_new 六次中位 4.841 → 0.742 秒，token 与文本不变
- 066-research-staged-load — 装载分两段：engine_new 只装音频塔与梅尔（0.754 → 0.135 秒），文本塔与分词器（0.643 秒）在 begin 之后跟说话重叠；进程起→可以接受按键 0.75 → 0.14 秒，2 秒说话按下→出字仍是 2.27 秒，token 与文本不变
- 067-research-load-cut-result — 装载这一刀的验收与结果：四段完整对照（转换 3.731 → 0.077 秒、eval 722 次 0.491 → 1 次 0.140、装载 4.785 → 0.757 秒、engine_new 4.841 → 0.137 秒），三套闸全过，包重打重装且签名通过；还剩下的瓶颈是分词器 0.393 秒、每步整段重算梅尔、跨步不复用键值、贪心每 token 约 42 毫秒
- 068-research-incremental-mel — 梅尔每步只算新样本：14 秒探针六步 0.194…0.503 → 0.179…0.219 秒，整形按当前前缀的 max 套用，增量与整段重算逐位相同（差 0）
- 069-research-prefix-kv-reuse — 提示头与已结束各窗的行只预填充一次：12 秒处重算的位置从 174 降到 61、预填充 0.073 → 0.043 秒，六步合计 1.263 → 1.158 秒（少 8.3%），token 与文本六步全对
- 070-research-step-scaling-result — 068 与 069 之后的验收：三套闸 3/6/4 全过、61 通过，重编归档与 Swift 产物、包重打重装且签名通过；14 秒探针六步 2.103 → 1.130 秒（梅尔 0.035 平、预填充 0.041），engine_new 0.134 秒、暖定稿 0.178 秒，剩下的涨幅在贪心 42 毫秒/token 与编码 0.030–0.053 秒
- 072-research-multichunk-reference-assets — 把 3 / 4 个整块的两段切片导成逐层参考量（梅尔、卷积茎、第 0 层、投影与提示），旧的 200 帧资产一个字节没动
- 073-research-multichunk-first-divergence — 300 / 400 帧逐层对照没有一个元素超差，第一处偏离在驱动：`bench7` 是 `bench3.c` 编的，只读 32000 个采样
- 074-research-multichunk-fix — 换回读整段的驱动：六段真实中文语音逐字对上参考，300 / 400 帧与六段由打印改成断言闸，重编归档与产物、包重装并复验
- 075-derivation-voice-abandon — 从外部取消一条语音会话：为什么取消只停采集不补提交，机器与门的接口形状，状态迁移，以及怎么证明采集真的停了
- 076-research-voice-guards-wiring — 看门狗窗口 60 秒的理由、计时器为什么走自己的串行队列、撤表三处，以及 deactivateServer 与 commitComposition 的两处接线
- 077-research-voice-guards-result — 兜底两项的验收与装包结果：38 + 9 条 Swift 用例、61 条 Rust 用例、六段真实语音与装好的包逐字一致，以及麦克风仍可能长开的九种情况
- 078-derivation-unique-session-ids — 会话号改由进程级共享计数器发：接口与注入点、为什么不是每门自增、`begin` / `abandoned` / `final` / `rejected` 四种带号日志行，以及两门交替与同一门重开不重号的四条用例
- 079-research-engine-replace-semantics — 引擎侧同号 `begin` 从拒绝改成替换（旧会话连同解码器一起释放，日志多一行带号的 `replaced`）、新增 `voice_session_discard` 给取消路径收场，以及受影响的两条引擎用例与 5 条 ABI 用例逐条对照
- 080-research-session-collision-fix — 换窗口之后语音失效的定位与验收：两处改动（进程一份的会话号、取消发 `discard` 与同号 `begin` 替换）、三枚新的门各自 begin 的三层证据（Swift 号不重复、引擎三次回 0、真归档 C 探针一号重开与三号连开全为 0）、重编归档与产物并重打重装且签名通过，以及真机切换窗口那一步没有覆盖的实情
