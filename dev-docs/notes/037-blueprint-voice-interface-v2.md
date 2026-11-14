未确认蓝图

# 037-blueprint-voice-interface-v2：按住说话、feed_audio 与定稿原文的开发参照

本文是实现用的参照：架构、模块、接口、关键实现决定。`033` 记下的五项拍板直接写进接口。`034` 写明的控制器行为与 `035` 写明的收尾函数直接写进顺序。`031` 仍是未确认蓝图，本文不改它。`036` 没有落地文件。交给本文的异议清单是空的，没有条目被吸收。

用户原话：「我决定以你的方案、决策和思路进行开发。仍未确认蓝图，请自行调研填补空白未知，不再交由我来拍板。请帮我构建 workflow 进行详细工程分析，落地文档，基于调研结果帮我规划一份实施方案。我的目标是：在这个 Rime 输入法客户端里实现上述语音接口。」

下文 `033`、`034`、`035` 指 `dev-docs/notes/` 下同号笔记。Squirrel 基线是 `dev-docs/refs/code/squirrel` 提交 `b7970a93b05ae1ca1bcd97888fd6aa3cd0458d3e`，librime 基线是 `dev-docs/refs/code/librime` 提交 `bb56876890ba60f446fa2640c244b1794b965eda`，行号按 `034`。识别函数在已安装的 `mlx-qwen3-asr==0.4.4` 的 `mlx_qwen3_asr/streaming.py`。本次没有调用 REA。

当前方案里的新选择就是这一份实现的做法，不再分成一组请人挑选的做法。

## 架构

语音接口是一枚语音会话上的组合，不是从采样到上屏的总函数，也不是 Type4Me 的进程或 `type4me://toggle`。键盘事件仍走现行 `handle` → `processKey` → `rimeAPI.process_key`。语音文本不编码成按键，不调用 `set_input`，不调用 `clear_composition`。librime 的键盘路径不改。交付不调用 Type4Me 的 `inject` 或 `injectTracked`，不合成粘贴。

控制器上只有一份 `preedit: String`（`SquirrelInputController.swift:16`）。因此当前方案同时只开放一枚语音会话。第二下按住不再另开一枚。librime 的 `RimeSessionId` 与这枚语音会话不是同一个标识，键盘组合仍留在 librime 里。

### 实现顺序

1. 按下绑定的修饰键。`handle` 在调用 `processKey` 之前取走这一次 `.flagsChanged`（以及 `keyCode` 就是这枚修饰键的 `.keyDown`），返回真。不调用 `processKey`，不写字段。
2. 会话打开：新的语音会话标识，记下当时的 `client` 与 `bundleIdentifier`，交付许可为允许，收束未请求。识别记录空，改写记录未到。
3. 上下文只读汇集。只有 `hotwords` 投影成一段空格分隔字符串。`host_app`、`field_role`、`cursor_before`、`corrections` 缺席。
4. 识别侧执行 `init_streaming`。`model` 传入 `moona3k/mlx-qwen3-asr-0.6b-4bit`。热词字符串在场则传入 `context`，缺席则不传。`chunk_size_sec=2.0`，`max_context_sec=30.0`，`sample_rate=16000`，`language="Chinese"`。其余参数用该函数的默认值。包默认的 `model` 是 `Qwen/Qwen3-ASR-0.6B`（`config.py` 的 `DEFAULT_MODEL_ID`），当前方案不传这个默认值。
5. 采集交出 PCM16 小端、16000 Hz、单声道的有序帧。帧按到达送进 `feed_audio`。不把一句话攒成一次调用。
6. `feed_audio` 返回的 `state.text` 是整段。与会话里上一条部分文本逐字不同时，识别记录换成这一条部分文本。呈现写一条预编辑意图，字符串就是这份整段。交付调用 `show(preedit:selRange:caretPos:)`。不写提交意图。不满一块时 `feed_audio` 不解码，`state.text` 不变，不另造部分文本。
7. 按住期间，其他键不取走，仍进入现行 `processKey` 与 `rimeUpdate`。`034`：活会话上这次 `rimeUpdate` 会用 librime 的组合文本盖过控制器里的语音缓存；不在组合中时，这份文本是空串。语音会话里的部分文本不跟着缓存改。语音路径不在 `rimeUpdate` 之后把字符串写回去。
8. 松开同一枚修饰键。这一次事件同样取走，不进入 `processKey`。会话标成收束已请求，仍开放。采集停止。
9. 调用一次 `finish_streaming`。不把空 PCM 送进 `feed_audio` 当作收尾。定稿原文是返回状态的 `state.text`，整段，不是相对上一条的增量，也不是 `stable_text`。
10. 定稿原文为空：不写提交意图。若控制器 `preedit` 仍等于本会话最后一次 `show` 的字符串且非空，再 `show` 一次空串，只把这份语音缓存清掉。不调用 `commit(string:)`。
11. 定稿原文非空：改写适配器立刻写出一条改写记录。当前方案的提示缺席，不调用模型。判定为不替换，原文等于定稿原文，返回串缺席。
12. 改写记录到达后，呈现写一条提交意图。字符串是定稿原文，不是改写返回串。交付先按下面「提交」的规则调用 `commit(string:)`。会话在这次意图应用或拒绝之后关闭。

采集不读改写结果。识别适配器不调用 `show` 或 `commit`。改写适配器不读 PCM，不读部分文本，不持有 `client`。呈现不发明优化文本。

`feed_audio` 凑满一块时可能增加 `_text_updates` 与 `_rewrite_events`。`035`：这两个计数不是拍板 D 的改写记录，`finish_streaming` 也不写它们。当前方案不读取这两个计数。

### 谁碰得到谁

| 记录或端口 | 读 | 写 | 碰不到 |
| --- | --- | --- | --- |
| 会话 | 打开时的 `client` 与 bundle id；此后的识别记录与改写记录 | 寿命、许可、收束、最后一次 `show` 的字符串、两条记录 | librime 的 `input_`；`inject` |
| 触发 | 绑定修饰键的按下与松开 | 打开，或收束已请求 | 采样帧、文本、字段 |
| 采集 | 会话开着、设备是否交出样本 | PCM16 帧，或无帧并结束 | 热词、识别文本、`show` |
| 上下文 | 热词来源在打开时的只读值 | 一段热词字符串，或该字符串缺席 | 另外四槽的拼接；模型权重 |
| 识别适配器 | 帧、热词字符串或然缺席、会话标识 | 部分文本或定稿原文，都是整段 `state.text` | 改写返回值、宿主字段 |
| 改写适配器 | 定稿原文、会话标识 | 一条改写记录 | PCM、部分文本、`show`、`commit` |
| 呈现 | 识别记录、改写记录、许可 | 预编辑意图或提交意图 | 采样帧、屏幕上的标记 |
| 交付 | 一条意图、控制器 `preedit`、`client` 是否还在 | `show` 或 `commit`，或拒绝 | `process_key`、`inject`、librime 组合 |

### 提交不读取屏幕

`034` 写明三件与提交有关的事：

- `commit(string:)` 的 `insertText` 使用参数 `string`，不读取控制器 `preedit` 里的旧文本。调用后把 `preedit` 写成 `""`。
- `preedit` 非空时，`force_marked_text_for_direct_commit` 那次 `setMarkedText` 不进入。该分支还要求选项为真且参数非空。
- 已有标记时，`setMarkedText` 与 `insertText` 的屏幕结果，源码未写。`replacementRange` 都是 `.empty`（`NSNotFound`，长度 0）。这个范围在已有标记时替换哪一段，源码未写。

当前方案因此不把「标记范围会被换成定稿」当作提交完成的条件，也不从屏幕或从被按键盖过的缓存读回字符串。提交完成指：在交付仍允许时，`insertText` 以定稿原文被调用了一次。

调用顺序：

- 控制器 `preedit` 仍等于本会话最后一次写下的语音字符串，且该字符串非空：先 `show(preedit: "", selRange: NSRange(location: 0, length: 0), caretPos: 0)`，再 `commit(string: 定稿原文)`。先清空是为了让 `commit` 看见 `preedit.isEmpty`。空串这次 `show` 的屏幕结果，源码未写；方案不根据它再补一次 `commit`，也不改字符串。
- 控制器 `preedit` 已经不是这份语音字符串：不再 `show` 空串，避免把键盘路径刚刚写入的缓存再清成语音侧的空标记。仍然 `commit(string: 定稿原文)`。`insertText` 作用到当时标记上的屏幕结果，源码未写。
- 控制器 `preedit` 已是空串：直接 `commit(string: 定稿原文)`。这是 `commit` 在缓存为空时的那条调用。

`commit` 不调用 `clear_composition`。松开不会拆掉 librime 里的 `input_`。下一次进入键盘链的键仍由 `rimeUpdate` 按 librime 的组合去 `show`。

`commitComposition` 不承担语音提交。`034`：它提交的是 `get_input` 返回的 `input_`，语音 `show` 不写 `input_`。`deactivateServer` 会走到这里。输入源被切走时，语音交付不再调用 `show` 或 `commit`。

## 模块

目录边界不是进程边界。不另设总控包。顺序留在客户端的 `handle` 与识别回调的组合处。

- `voice-interface/session/`：语音会话标识、寿命、许可、收束、最后一次语音 `show` 的字符串、识别记录、改写记录
- `voice-interface/trigger/`：绑定修饰键的按下与松开
- `voice-interface/capture/`：PCM16 帧
- `voice-interface/context/`：热词字符串或然缺席
- `voice-interface/recognition/`：`init_streaming`、`feed_audio`、`finish_streaming`
- `voice-interface/rewrite/`：定稿原文进，改写记录出
- `voice-interface/presentation/`：预编辑意图与提交意图
- `voice-interface/delivery/`：`show` 与 `commit`，或拒绝

识别库是 CPython 包。`handle` 在返回之前不调用 `feed_audio` 或 `finish_streaming`。当前方案把这三个函数放在输入法进程派生的一个本地辅助进程里：Swift 控制器没有这个包的链接入口，已测调用在 `mlx-qwen3-asr==0.4.4`。辅助进程只接受初始化、PCM 帧和一次收尾，只送回整段 `state.text`。它不提供 `inject`，不持有 `client`。主进程持有会话并调用 `show` 与 `commit`。

类型怎么放：

- 会话标识与两条记录只定义在 `session/`。`recognition/` 与 `rewrite/` 返回记录，不定义寿命。
- 帧的类型在 `capture/`。`recognition/` 依赖帧。`capture/` 不依赖识别文本。
- 热词字符串由 `context/` 投影。`recognition/` 只接收这段字符串或然缺席，不引用五槽。
- 意图的类型在 `presentation/`。`delivery/` 依赖意图。`presentation/` 不调用 `show`。
- `trigger/` 只写开闭。`session/` 不依赖 `trigger/`。

### 四词测量

测量对象是上面八个目录。测量不是改结构的指令。

清晰：触发、采集、上下文互不引用。识别与改写只产出记录。呈现只读记录，交付只读意图并调用 `show` 或 `commit`。代价是一次按住的先后要在组合处才看全。

内聚：`recognition/` 一起变的理由只有「帧与热词字符串变成整段 `state.text`」。辅助进程跟这三个函数同一理由，不另拆目录。`rewrite/` 一起变的理由只有「定稿原文变成不替换或全文不同」。`delivery/` 里没有 PCM。

组合：换掉辅助进程的启动方式时，帧的字段与 `state.text` 的含义不动。改写保持不替换时，识别记录的字段不动。交付不拉取 `inject`。代价是没有一个目录要被两个互不相关的调用方整份取走；共享的是会话标识与记录。

优雅：拆开的点是会话寿命、开闭、帧、热词字符串、整段文本、改写判定、意图种类、宿主是否还接受 `client`。部分文本与定稿不各占一个目录，它们是同一条识别记录的两种种类。不把键盘路径放进这个布局。

## 接口

### 触发

绑定是客户端自有配置里的一枚修饰键，键码属于 `SquirrelKeycode.modifierKeycodes`（`MacOSKeyCodes.swift` 里该集合含左右 Shift、Control、Option、Command、Caps Lock、Function）。不写入 librime 方案。配置里没有这枚键时，`handle` 不取走修饰键，语音会话不打开。本文不写一个具体键码：源码没有给这个客户端指定数字。

取走的规则：

- `.flagsChanged` 里，变化含这枚修饰键：整次事件取走，不调用 `processKey`，不调用 `rimeUpdate`。按下打开会话并开始采集。松开把收束标成已请求并停止采集，然后走 `finish_streaming`。同一事件里若还夹着别的修饰变化，也不拆开送进 `processKey`。
- `.keyDown` 的 `keyCode` 就是这枚修饰键：取走，不调用 `processKey`。
- 其余 `.keyDown` 与其余 `.flagsChanged`：不取走。含 `.command` 的 `.keyDown` 仍按现行 `97-99` 行直接跳出。
- 已有一枚未关闭的语音会话时，再一次按下不另开。松开只收束这一枚。

按下不写字段。松开本身不写字段。写字段的是后面的预编辑意图与提交意图。

### 会话

语音会话标识与 `RimeSessionId` 不是同一枚。打开成功时换一枚新标识。对不上的文本不写入这一份记录，也不调用 `show` 或 `commit`。

| 字段 | 含义 |
| --- | --- |
| 会话标识 | 不透明。识别记录、改写记录、意图都带它 |
| 客户 | 打开时的 `client`。交付时为 nil 则拒绝 |
| 应用 | 打开时的 `bundleIdentifier`。交付时不一致则拒绝 |
| 寿命 | 未建立、开放、已关闭 |
| 收束已请求 | 是或否 |
| 交付许可 | 允许或拒绝 |
| 最后显示 | 无，或本会话最后一次 `show` 的字符串 |
| 识别记录 | 空、一条部分文本，或一条定稿原文 |
| 改写记录 | 未到、不替换，或全文不同 |

| 当前 | 输入 | 迁到 |
| --- | --- | --- |
| 未建立 | 按下，且当时有 `client` | 开放，允许，收束未请求 |
| 未建立 | 按下时没有 `client` | 保持未建立 |
| 开放，允许 | 部分文本，标识相同 | 仍开放。识别记录换成这一条。可写预编辑意图 |
| 开放，允许 | 松开 | 仍开放，收束已请求。尚未写提交 |
| 开放，收束已请求 | `finish_streaming` 给出非空定稿 | 仍开放。识别记录换成定稿。改写仍未到。不写提交 |
| 开放，收束已请求 | 定稿为空 | 不写提交。按实现顺序第 10 步清语音缓存。然后可关闭 |
| 开放，识别已是非空定稿 | 改写记录到达，标识相同 | 仍开放。写出提交意图 |
| 开放 | `client` 为 nil，或 bundle id 偏离 | 仍开放，拒绝交付。不改写新客户，也不再 `show` / `commit` |
| 开放，收束已请求 | 提交意图已应用或已拒绝，且没有未发出的意图 | 已关闭 |
| 已关闭 | 迟到的文本或意图 | 保持已关闭 |

同一应用里换了一个输入框、但 `client` 仍是同一对象时，源码没有第二枚字段标识。当前方案不另造字段号，仍向这个 `client` 提交。字段失焦时 IMK 调用 `commitComposition` 还是 `deactivateServer`，`034` 记为源码未写。失焦不单独变成「提交定稿原文」。

空定稿是一条正常的识别记录。它不产生提交意图。

### 采集与帧

每一帧：

| 字段 | 取值 |
| --- | --- |
| 编码 | PCM16，小端 |
| 采样率 | 16000 |
| 声道 | 1 |
| 字节 | 这一段样本，顺序与采集交出的顺序相同 |

这三个数写在帧上。块长不是帧的字段。使用权不成立或设备没有样本时，采集无帧并结束。识别适配器不把这件事写成一段文本。

整数 PCM 交给 `feed_audio` 时，由该包的 `_sanitize_stream_pcm` 转成单声道 float32。主进程不另写一套缩放。

### 识别

对外一条记录：种类、文本、会话标识。种类是部分或定稿。文本是整段。没有修订指针，没有 `confirmedSegments`。

调用的是 `mlx_qwen3_asr.streaming` 的模块函数，不从包的 `__all__` 找名字。`035`：`__all__` 里没有 `feed_audio`，也没有 `finish_streaming`。`Session.feed_audio` 与 `Session.finish_streaming` 只是把已加载模型转进这两个函数。当前方案调用模块函数，或调用 `Session` 的同名转发，二者只留一种进程内入口：辅助进程里各调用一次对应函数，不串行两次收尾。

`init_streaming` 的显式参数：

| 参数 | 当前方案 |
| --- | --- |
| `model` | `moona3k/mlx-qwen3-asr-0.6b-4bit` |
| `context` | 热词字符串；缺席则不传 |
| `chunk_size_sec` | `2.0` |
| `max_context_sec` | `30.0` |
| `sample_rate` | `16000` |
| `language` | `"Chinese"` |

`chunk_size_samples` 由函数写成 `int(chunk_size_sec * sample_rate)`。在这组参数下是 32000。`035` 写的就是这个数。`unfixed_chunk_num` 默认 2，`unfixed_token_num` 默认 5，`endpointing_mode` 默认 `"fixed"`，`finalization_mode` 默认 `"accuracy"`。当前方案不改这些默认值。`035`：`accuracy` 与 `latency` 都在 `finish_streaming` 里处理待处理尾巴，不另有一条收尾函数。

说话中：

- 输入是新的 PCM 与已经 `init_streaming` 的 `state`。
- 缓冲长度达到一块才解码。空 PCM 时 `feed_audio` 把原 `state` 返回，当前方案不发送空 PCM。
- 输出文本是返回的 `state.text`。这是已提交窗口拼上活动窗口的整段。不取 `stable_text`，不取增量。
- 部分文本可以整段替换上一条部分文本。替换留在会话里。

松开后：

- 调用一次 `finish_streaming(state)`。没有 PCM 参数。它读的是 `state.buffer`。
- `buffer` 长度大于 0：取走剩余样本，清空 `buffer`，再解码。不要求这块凑满 32000。定稿原文是解码后的 `state.text`。
- `buffer` 长度等于 0：不再解码。定稿原文是进入函数时的 `state.text`。`audio_accum` 非空时函数把 `stable_text` 设成这份 `text`；当前方案仍读 `text`，不读 `stable_text`。
- 按住长到超过 `max_context_sec`（30.0）时，同一次 `finish_streaming` 内部可能先提交旧窗口再解新窗口。返回给会话的仍是拼好的整段 `state.text`。超过 30 秒的样本，`035` 没有。内部那一次提交解码的时延：未知。
- 剩余缓冲非空时，定稿整段与上一条部分文本差几个字符：未知。

不装 SenseVoice。不把两个识别器串成前后两级。不走 Type4Me 的 `POST /transcribe`，不订阅它的 `/ws`。

辅助进程送回主进程的只有种类、整段文本、会话标识。`_rewrite_events` 不越过这个边界。

### 热词字符串

快照可以仍有五个名字。进入识别的只有 `hotwords`。

- `hotwords` 缺席：热词字符串缺席。不写成 `""`。`init_streaming` 不传 `context`。函数自己把缺省写成 `StreamingState.context == ""`（`context=context or ""`）。端口上的缺席与函数内部的空串不是同一层。
- `hotwords` 在场：去掉每条两端空白，丢掉空白项，用单个空格拼成一段。这段传入 `context`。拼法与 `dev-docs/refs/code/type4me/Sources/qwen3-asr-server/server.py:388-391` 的 `" ".join(words)` 相同，作用对象只有这个槽。
- `host_app`、`field_role`、`cursor_before`、`corrections` 不进入 `context`，也不进入改写提示。不把缺席写成空字符串再拼进去。
- 帧的字节里不放热词。

`022` 写明 moona 的 `context` 是一段领域词字符串，不是五槽对象。当前方案不再给另外四槽补分隔符。

### 改写记录

输入是定稿原文与会话标识。不是部分文本，不是 PCM。提示缺席。缺席不是空字符串；当前方案没有一段提示可传。

输出：

| 字段 | 含义 |
| --- | --- |
| 会话标识 | 与定稿那条相同 |
| 原文 | 定稿原文，原样留下 |
| 返回串 | 缺席。当前方案不调用模型 |
| 判定 | 不替换 |

判定只做全文逐字比较：返回串缺席、调用失败、或与原文逐字相同，记成不替换；逐字不同，记成全文不同。全文不同的返回串只留在这条记录里。预编辑意图与提交意图都不读取返回串。当前绑定的返回串缺席，判定为不替换：没有提示，也没有一份点名仓库路径上的数字说明替换文本的错误更少。`033` 写明 G 与 D 是在这件未知仍在时作出的。本文不补这对数字，不补阈值。

恒等的不替换仍是一条改写记录。提交要等它到达。它与定稿回调写在同一次回调里，中间不插入别的文本。

### 预编辑与提交意图

一条意图四个字段：会话标识、字符串、种类、以及「最后显示」用来核对缓存。种类只有预编辑与提交。意图上没有采样帧，没有制品名字，没有键盘事件，没有 `InjectionOutcome`。

Squirrel 的 `show` 与 `commit` 都不接收「替换屏幕上哪一段」的业务范围。两者的 `replacementRange` 都是 `.empty`。意图里因此不带一份要回读的宿主范围。

预编辑：

- 字符串等于当前部分文本，也就是某次 `feed_audio` 的 `state.text`。
- 交付调用 `show(preedit: 字符串, selRange: NSRange(location: 0, length: 0), caretPos: 字符串的 UTF-16 长度)`。`show` 用 `selRange.location` 把此前标成已转换、此后标成未转换；location 为 0 时整段按未转换标记。`selRange.length` 只参与缓存相等判断。
- 调用后会话的「最后显示」改成这份字符串。
- 不调用 `process_key`、`set_input`、`clear_composition`。

提交：

- 只在识别记录已是定稿、且改写记录已经不是「未到」时写出。
- 字符串等于定稿原文。改写判定是不替换或全文不同，字符串都是这份原文。
- 交付按架构一节的三条调用 `commit(string: 定稿原文)`。
- 应用后「最后显示」视为无。控制器 `preedit` 被 `commit` 写成 `""`。

检查按这个顺序。任一项失败：不调用 `show` 或 `commit`，字段保持调用前的控制器缓存。

1. 意图的会话标识等于这一份会话。
2. 寿命是开放。
3. 交付许可是允许。
4. `client` 非 nil，且 bundle id 等于打开时的值。
5. 种类为预编辑时，字符串等于识别记录里的部分文本或定稿原文，且改写返回串不在字符串里。当前顺序里，预编辑只发生在定稿之前，字符串是部分文本。
6. 种类为提交时，字符串等于定稿原文，且改写记录不是未到。

部分文本可以有多次预编辑，零次提交。没有部分文本、只有松开后的定稿时，可以只有一次提交，没有预编辑。不补一段假的部分文本，也不把「录音中」一类占位写进 `show`。

### 交付不调用的函数

交付的宿主调用只有 `show(preedit:selRange:caretPos:)` 与 `commit(string:)`。

不调用 `TextInjectionEngine.inject`，不调用 `injectTracked`，不写剪贴板，不合成 Command-V。不调用 `rimeAPI.process_key`、`set_input`、`clear_composition`、`get_input`。不把语音字符串交给 `commitComposition`。

## 关键实现决定

### 决定 1：按住打开，松开收束

按下取走并打开，松开取走并收束。收束之后才 `finish_streaming`。松开不是再送一块空 PCM。第二下按住不另开会话。键是客户端配置里的一枚修饰键，不进 librime 方案。

依据：`033` 的 L。`handle` 的 `.flagsChanged` 在 `54-91` 行会把修饰键送进 `processKey`；当前方案在那之前取走绑定键，所以这条取走不改 librime。

### 决定 2：部分文本与定稿都是整段 state.text

说话中每次解码后的部分文本是 `feed_audio` 返回的 `state.text`。松开后的定稿原文是 `finish_streaming` 返回的 `state.text`。二者都是整段。不取 `stable_text`，不取 `_finalization_delta_chars`，不把 `_rewrite_events` 当成改写记录。

依据：`033` 的 A。`035` 第 3、4 节。

### 决定 3：只走这一条识别，模型参数用 4-bit 仓库路径

识别只调用 `init_streaming`、`feed_audio`、`finish_streaming`。`model` 是 `moona3k/mlx-qwen3-asr-0.6b-4bit`。不传包默认的 `Qwen/Qwen3-ASR-0.6B`。不装 SenseVoice，不把两个识别器串成两级，不走 `/transcribe` 与 `/ws`。

依据：`033` 的 A。`024` 测量的权重是这个 4-bit 仓库。`config.py` 的 `DEFAULT_MODEL_ID` 是另一条路径。

### 决定 4：部分文本写成预编辑，改写记录到达后提交定稿原文

每一条新的部分文本至多一条预编辑意图，`show` 的字符串就是该整段。定稿写进识别记录之后、改写记录未到时，不写提交。改写记录到达后写一条提交，字符串是定稿原文。定稿不再单独刷一条预编辑：最后一条部分文本已经在预编辑里；若说话短到没有部分文本，就只有这一次提交。

依据：`033` 的 D 与 G。`show` 与 `commit` 的分工在 `034` 第 2 节。

### 决定 5：改写只留记录

提示缺席，不调用模型。记录为不替换，原文是定稿原文，返回串缺席。全文不同的返回串若出现在记录里，也不进入 `show` 或 `commit`。

依据：`033` 的 G。纠错分数仍然没有点名仓库路径上的一对数字，`033` 写明这项未知还在。本文不补阈值。

### 决定 6：热词只有一槽

识别只接收空格分隔的热词字符串，或该字符串缺席。另外四槽缺席，不进 `context`，不进改写提示。缺席在端口上不是 `""`。

依据：`033` 的 J。`server.py:388-391` 的空格连接。`022` 对 `context` 的形状。

### 决定 7：提交字符串不来自屏幕

定稿原文只来自 `finish_streaming` 的 `state.text`。不读控制器 `preedit`，不读宿主字段。缓存仍是本会话最后一次语音字符串时，先 `show` 空串再 `commit`；缓存已被键盘路径盖过时，不 `show` 空串，仍然 `commit` 定稿原文。两种情况都不根据屏幕增删字符。

依据：`034` 第 1、2 节。屏幕结果源码未写，所以不拿它当提交条件。

### 决定 8：未取走的键进入键盘链

除绑定修饰键外，键仍走 `processKey` 与 `rimeUpdate`。语音不在 `rimeUpdate` 之后抢回 `show`。不修改 librime。语音提交不调用 `clear_composition`，librime 的 `input_` 留在原处。

依据：交付不改 librime。`034` 第 1 节写明普通键会盖过语音缓存，以及 `commitComposition` 提交的是 `input_` 而不是控制器 `preedit`。

### 决定 9：解码不在 handle 的返回栈上

`feed_audio` 与 `finish_streaming` 在辅助进程里执行。`handle` 取走按住键之后就返回。部分文本与定稿回到主进程之后才 `show` 或 `commit`。

依据：控制器的 `handle` 在键路径上同步调用 `rimeUpdate`。`024` 的到达块解码不是这一次 `handle` 里能省略的空操作。辅助进程的边界就是决定 2 的整段文本，不增加别的服务。

### 决定 10：空定稿不提交

`state.text` 为空时不调用 `commit`。语音自己留下的非空缓存只用 `show` 空串清掉。清掉之后的屏幕，源码未写。

依据：`035` 的空缓冲返回仍可能是空的 `state.text`。`commit` 对空串仍会 `insertText`，已有标记时的屏幕结果未写，所以空定稿不走 `commit`。

## 源码未写、因而不拿来当条件的结果

下列各项 `034` 或 `035` 已标成源码未写或没有样本。当前方案不把它们写成屏幕会怎样，也不因此改提交字符串。

- 已有标记时 `setMarkedText` 或 `insertText` 之后，宿主屏幕变成什么。
- `replacementRange` 为 `.empty` 时，已有标记被替换的是哪一段。
- `handle` 返回假、事件交给客户应用之后，标记文本变成什么。
- 字段失焦时 IMK 调用的是 `commitComposition` 还是 `deactivateServer`。
- 剩余缓冲非空时，定稿整段与上一条部分文本的字符差。
- `finish_streaming` 真正解码尾巴时的时延。按住超过 30 秒时，内部那一次提交解码的时延。
- 点名仓库路径的 MLX 量化是否让替换文本的错误少于定稿原文。

## 异议

异议清单是 `{"objections":[]}`。没有条目被吸收，也没有条目被改写成另一种做法。

<!-- blueprint-complete -->
