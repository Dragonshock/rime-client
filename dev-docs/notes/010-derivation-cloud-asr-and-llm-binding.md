# 010-derivation-cloud-asr-and-llm-binding：云端语音识别与大模型优化绑定

本文只推导一件事：云端绑定要满足的数据契约。三块契约是流式识别的输入输出、定稿之后文本优化的输入输出、上下文快照进入请求的位置。倾向一律是候选。

闭源样本只用来核对两件事：私有长连接上已经出现过的字段形状，以及鉴权材料坐在哪个类、哪条字面量旁边。样本不是可调用契约。打开官方进程内部的令牌、复刻其签名、或把本进程伪装成官方输入法去打私有主机，不在本文范围内。公开接口盖不住的能力写成缺口。

用户点名的云端制品在这里拆成两面，因为探测看到的是两套主机、两套调用方凭证，而不是一把钥匙打通「听」和「改」：

- 豆包语音（Doubao Speech）开放的流式语音识别（streaming ASR）。
- 火山方舟（Volcengine Ark）开放的豆包大模型对话（chat）接口。

输入法自己的 Frontier 长连接与 `text_organization` 路由是第三条线。它说明样本里识别与改写确实分开出现过，不因此成为绑定的目标主机。

## 1. 推理链

### 1.1 为何是「两套公开契约、一条私有线只作形状」

007 把绑定定义成：角色仍是识别流或改写流，制品只经过适配器进入；契约未知时角色空着，不改其余原语的类型。所以云端绑定要先回答「调用方自己的凭证能不能碰到同一类输入输出」，再回答「样本私有线上多出来的字段算不算这种类型」。

探测把三条线分开了。

流式识别的公开面在 `wss://openspeech.bytedance.com/api/v3/sauc/bigmodel`、`.../bigmodel_nostream`、`.../bigmodel_async`。建连头是调用方在语音控制台拿到的 `X-Api-Key`；旧控制台则是调用方自己的 `X-Api-App-Key` 与 `X-Api-Access-Key`。另有 `X-Api-Resource-Id` 选择模型资源，`X-Api-Request-Id` 由调用方填任务标识。请求吃的是音频格式与有序音频包，响应给出可被更晚的音频包改写的 `result.text`，并用 `utterances[].definite` 标出分句已经定稿。这与原语 4 的「帧进、可修订假设出」同型。凭证主体是调用方，不是输入法进程。

定稿后的文本优化，公开面是方舟对话。官方示例的请求头是 `Authorization: Bearer` 加上调用方自己的 `ARK_API_KEY`；请求体是 `model` 与 `messages`；非流式响应是 `choices[].message.content`。流式文档写明对话接口在请求体里置 `stream` 为真，客户端按 `choices[].index` 聚合 `delta`，用 `finish_reason` 判断这一轮结束。数据面域名在地域说明里写作 `ark.cn-beijing.volces.com`。这与原语 5 的「文本进、文本出」同型，但没有样本里的改写决策枚举。凭证主体仍是调用方，而且与语音控制台那一套不是同一个头、不是同一台主机。

私有线的主机是另外两个名字：`wss://frontier-audio-ime-quic.doubao.com/api/v1/ws` 与 `wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws`。令牌字面量指向 `https://speech.bytedance.com` 上的 `/api/v1/token?access_key=`。整理文本的路径字面量是 `https://ime.doubao.com` 上的 `/api/v2/ai/text_organization`。对 `text_organization` 与 `ime.doubao.com` 的公开检索没有落到这份路径的开放说明上。因此「同一能力」只在前两套公开契约上被调用方凭证盖住；私有主机上的连接、决策字段和进程内加密校验没有被这两套凭证盖住。

### 1.2 舍弃的焊法

**焊法 1：把私有 Frontier 当成识别流的实现。** 样本确有 `StartTask`、`SendAudioData`、`FinishTask`、`TaskFinished`，也确有与音频长连接字面量贴在一起的 `speech_opus`、`x-tt-token`。这些是观察到的名字。令牌与 `samiAppKey` 坐在官方进程的 `OimeAudio.SAMITokenManager` 里。007 的绑定不变量写明：适配器只依赖可调用契约；材料只在官方进程内部时，输出是「该制品尚无可调用契约」，不是一条绕过路径。本文沿用这条不变量，不把私有主机收成候选端点。

**焊法 2：把 `text_organization` 的决策字段收成改写流的类型。** 样本对象上有 `rewriteDecision`、`didReceiveNoRewriteCompleted`，SSE 处理函数旁有 `format.completed` 与 `scene.completed.empty_content`。公开对话接口的完成信号是 `finish_reason` 与一段 `content`，没有这些名字。若把样本枚举写进改写流的输出类型，适配器就会要求每个大模型实现都长得像这条私有路由。原语 5 要的是「不替换」或「替换为这段文本」，两者都保留原假设。类型停在这里。

**焊法 3：用识别请求里的语义顺滑代替大模型改写。** 公开流式识别有 `enable_ddc`（语义顺滑）和 `enable_nonstream`（一句话结束后再识别一遍并替换实时文字）。二者的输入仍是音频包，输出仍是 `result.text` / `utterances`。这是识别假设的修订，符合原语 4「更晚的假设可以替换更早的文本」，不符合原语 5「输入类型是文本与上下文，没有采样帧」。把它们焊进改写流，换掉大模型时识别请求的标志位也会跟着变。

**焊法 4：用键盘云端解析器的 `precedingText` 当作语音整理的线上键。** `precedingText` 出现在 `OimeEngine` 的 `parseCloudAndLLMResponse` 符号里，那是键盘候选与云端结果的融合入口。语音整理对象上看到的是 `query`、`contextID`、`contextBeforeRequest`、`contextAfterRequest`、`hostBundleID`。本次没有在 `/api/v2/ai/text_organization` 旁边解出一份 JSON 键清单，不能把键盘参数名抄成语音请求体。

**焊法 5：一把凭证打两家，或一个请求同时完成听和改。** 语音侧的头是 `X-Api-Key` 或旧版 `X-Api-App-Key` / `X-Api-Access-Key`，主机是 `openspeech.bytedance.com`。大模型侧的头是 `Authorization: Bearer` 与 `ARK_API_KEY`，域名是方舟数据面。样本自己也是先有识别事件、定稿后再走整理客户端。单请求会把原语 4 与原语 5 的输入类型焊回去，也就是 007 舍弃的焊法 A。

**焊法 6：把方舟文档目录里的「音频理解」当成识别流。** 目录里有这一条，与豆包语音的流式识别不是同一篇。本次没有读到它的请求体，不知道它是否接受有序采样帧、是否给出可修订的部分结果。在读到之前，它不担任识别流。

## 2. 与 007 原语的对应关系

| 原语 | 本绑定动的是什么 | 保持不动的是什么 |
| --- | --- | --- |
| 原语 4 识别流 | 哪一份公开流式识别契约把有序帧收成假设；`definite` 充当「分句已定稿」信号 | 帧上自带格式；改写流不接收帧 |
| 原语 5 改写流 | 哪一份公开对话契约在定稿文本上给出另一段文本或与原文相同 | 不持有宿主字段；不写预编辑 |
| 原语 6 上下文 | 快照槽位分别落进识别请求的 `corpus.context`，以及对话的 `messages` 文本 | 快照仍是名字到文本，不是某一家提示词格式；来源读不到则槽位缺席 |
| 原语 9 绑定 | 识别角色接语音开放接口的适配器；改写角色接方舟对话的适配器 | 重新配对不改变原语 4 与原语 5 的类型；私有制品没有可调用契约时角色空着 |
| 原语 1 会话 | 公开请求里的任务标识可以携带与会话的对应关系 | 会话标识仍由会话原语发出；字段变化后的拒绝交付不由云端响应决定 |
| 原语 7 呈现 | 不在本绑定里选择预编辑或提交 | 识别部分结果与改写完成结果都还不是变更意图 |

原语 3 采集不因本绑定改类型。公开识别把格式放在请求的 `audio` 对象里，与「格式写在帧上」相容：适配器读取帧上的格式再填写 `audio`，不从「当前模型」反推格式。

核对句，按 007 的问法：

- 只喂帧、不提供宿主字段时，公开识别响应里是否仍有 `result.text`，且该响应本身不写预编辑？按已读文档，响应体只有识别 JSON。是。
- 对同一条定稿假设把改写实现换成恒等之后，识别请求的音频字段是否不用改？是。恒等改写不调用对话接口。
- 把同一份快照同时送进识别与改写时，是否还要为每个实现各写一份来源？候选映射是各写一份字段装填，来源仍是一份快照。装填代码不属于原语 6 的输出类型。

## 3. 数据契约或状态迁移

### 3.1 流式识别：输入、输出、定稿信号

公开实时识别文档把服务描述为：说话过程中持续返回增量结果，中间结果可能被更晚到达的音频更新，一句话结束后返回确定的分句结果。与原语 4 的可修订假设同向。

建连输入（调用方凭证，不是样本令牌）：

- `X-Api-Key`：语音控制台的 API Key。文档同时保留旧控制台的 `X-Api-App-Key` 与 `X-Api-Access-Key`。
- `X-Api-Resource-Id`：模型资源。已读到的取值包括豆包流式语音识别模型 2.0 的 `volc.seedasr.sauc.duration`、`volc.seedasr.sauc.concurrent`，以及模型 1.0 的 `volc.bigasr.sauc.duration`、`volc.bigasr.sauc.concurrent`。
- `X-Api-Request-Id`：调用方生成的任务标识。文档推荐随机 UUID。它不是 007 的会话标识；适配器可以建立对应，类型上仍是两件事。

音频输入：

- `audio.format` 取值包括 `wav`、`mp3`、`ogg`、`pcm`。
- `audio.rate` 默认 16000，`audio.bits` 默认 16，`audio.channel` 默认 1。
- 同一页的示例载荷另含 `codec` 为 `raw`。正文的 `audio` 字段表没有把 `codec` 单独列成一节。旧版流式文档写过 `codec` 为 `raw` 或 `opus`。本页示例没有使用 `opus`。因此「pcm / 默认 16 kHz / 16-bit / 单声道」是已读到的输入形状；`opus` 是否仍被 `bigmodel_async` 接受，未知。

请求侧与「假设如何修订」有关、但仍然属于识别输入的标志：

- `request.model_name` 目前文档只写 `bigmodel`。
- `enable_itn`、`enable_punc` 改变结果的书面形态，输入仍是音频。
- `enable_ddc` 在识别结果里删改停顿词与重复词，输入仍是音频。
- `enable_nonstream` 在静音达到 `end_window_size`（默认 800 ms）后，对这一小段再识别一次，用新结果替换实时文字。这是识别流内部的第二次假设，不是改写流。
- `result_type` 取值包括 `full`（全量）与 `single`（不再带回先前分句）。适配器若要表达「修订的是哪一条先前假设」，需要知道服务端这一次返回的是全量还是增量。候选是让适配器向原语 4 始终输出「当前假设全文」，由 `result_type` 的差异留在适配器内消化。

输出形状，取自同一篇文档的输出示例，删去分词列表的重复项：

```text
// 来源：火山引擎文档《实时语音识别》输出示例
// https://docs.volcengine.com/docs/doubao-voice/bidirectional-streaming-automatic-speech-recognition-websocket
{
  "is_last_package": true,
  "payload_msg": {
    "result": {
      "text": "您好，我是您的 AI 助手。",
      "utterances": [
        { "definite": true, "text": "您好，我是您的 AI 助手。" }
      ]
    }
  }
}
```

这段示例的代价与边界：它证明响应里同时有整段 `text`、分句 `text` 和 `definite`，以及末包标志 `is_last_package`。它不证明每一次中间包都带齐这些键；文档正文说中间结果会随更晚到达的音频更新，示例本身是末包。`words`、`start_time`、`end_time`、`additions.log_id` 也在完整示例里出现过。原语 4 的假设类型只要文本、它修订的是哪一条、以及是否定稿。时间戳与日志号不是假设类型的一部分；适配器可以丢弃，不因此改识别流的输出类型。

候选状态，只描述数据上已经能区分的迁移：

1. 尚无 `result.text`：有帧在途，没有假设。
2. 已有 `result.text`，相关分句的 `definite` 不为真：这是一条可被更晚响应替换的部分假设。替换留在会话内。
3. 某条 `utterances[].definite` 为真：该条分句文本可以作为原语 5 的输入。更晚的识别响应仍可能带来别的分句；这一条定稿文本是否还会被同一条 WebSocket 再改，文档对「确定的分句结果」的用语是确定，本次没有用真实音频核对会不会再变。未知记在第 5 节。
4. `is_last_package` 为真：这一轮识别不再期待新的假设。它不是提交，也不是改写完成。

### 3.2 定稿后的文本优化：输入、输出

输入类型按原语 5：一条已经是文本的定稿假设、一份上下文快照、会话标识。没有采样帧，因此不把音频字段放进对话请求。

公开对话已读到的形状：

- 头：`Authorization: Bearer` 与调用方的 `ARK_API_KEY`。
- 体：`model`，以及 `messages` 数组。示例里的元素带 `role` 与 `content`。`role` 示例值见过 `system`、`user`、`assistant`。
- 非流式响应：`choices[].message.role`、`choices[].message.content`、`finish_reason`。示例里的 `finish_reason` 为 `stop`。
- 流式：请求体 `stream` 为真；按 `choices[].index` 聚合 `delta`；用 `finish_reason` 判断结束。

本次抓取官方 Base URL 页时，正文被导航截断，没有在官方正文里重新读到完整路径。地域说明写明华北 2（北京）的大模型调用域名是 `ark.cn-beijing.volces.com`。路径字符串不在本文写成已经核对的事实。

样本私有整理线已观察到的对象字段，不是公开契约：

```text
// 来源：/tmp/doubao-thin-arm64/classes-dump.txt
// 类 OimeSmartOrganize.SmartOrganizeClient.RequestContext 的实例变量名
requestID, generationID, query, contextID, spacingOptions,
rewriteDecision, didReceiveNoRewriteCompleted, encryptionValidated
```

这段的代价与边界：它证明官方进程里一次整理请求在对象上区分了「待整理的 query」「上下文标识」「改写决策」「是否收到不改写完成」「加密是否已校验」。它不是线上 JSON 模式，也不是适配器要复刻的头。`encryptionValidated` 以及旁边的状态字面量 `encryption_initialization_failed`、`encryption_validation_failed` 说明校验发生在官方进程内部。校验材料如何生成，本文不写。该边界是缺口：公开对话接口不提供这条私有整理路由，适配器不通过补齐进程内材料去调用它。

同一进程里，SSE 处理函数的字面量挨在一起：`handleDelta(payload:requestID:)`、`handleCompleted(payload:requestID:)`、`format.completed`、`scene.completed.empty_content`，以及 `missing_completed`、`delta_after_result`、`transport_closed_before_completed`。它们说明样本把增量与完成分成两次回调，空完成有单独的场景名。公开对话没有这些场景名。

候选输出映射，只为对齐原语 5，不是协议里已有的字段：

- 适配器等到 `finish_reason` 出现，才向会话交出一次改写输出。流式 `delta` 留在适配器内。理由来自原语 7：改写结果尚未到达时，呈现不发明优化文本。若把每个 `delta` 当成一次「替换为这段文本」，呈现就会在完成前拿到半段优化。这一挡法是候选；原语 5 的类型本身没有禁止多次替换文本。
- 交出时保留输入中的定稿原文。返回文本与定稿逐字相同，候选把它看成「不替换」。返回文本不同，候选把它看成「替换为这段文本」。
- 逐字相同并不是对话协议里的「不改写」信号。模型可以返回解释、前后缀、或空 `content`。空内容在样本里有 `scene.completed.empty_content` 这条私有场景名；公开接口没有同名场景。空 `content` 是否算「不替换」，未知，不把它写成错误类型。原语 5 规定「不替换」是正常输出。

`enable_ddc` 与 `enable_nonstream` 产生的文本变化停在识别假设上。改写流看到的「原假设」是识别流已经交出的那条定稿，不再把语义顺滑单拆成一次改写。

### 3.3 上下文如何进入请求

原语 6 的输出是槽位快照：名字到文本。同一份快照可以同时交给两个流。下面是候选装填，槽位缺席就不写该字段，不换成空错误。

进入公开流式识别的位置是 `request.corpus`，其中 `context` 在文档中是一段再序列化的 JSON 字符串，不是自由提示词。已读到的内部形状：

- `hotwords[].word`：直传热词。
- `context_type`：文档写目前仅 `dialog_ctx`。
- `context_data[].text`：历史对话文本；可带 `speaker`。
- 另有控制台词表标识 `boosting_table_id` / `boosting_table_name`、`correct_table_id` / `correct_table_name`，以及正则替换词表标识。这些标识指向控制台里预先放好的表，不是会话当场的一条纠错对。

文档给出的边界：直传热词与词表热词合计超过 100 tokens 时，按传入顺序从前向后截断，只保留前 100 tokens 内的内容。这是该请求字段的长度边界，不是另一套流量设施。热词被文档写成提示，不保证逐词命中。

槽位的候选落点：

| 快照槽位 | 进入识别请求 | 进入对话请求 |
| --- | --- | --- |
| 个人词 | `corpus.context` 里的 `hotwords[].word` | 放进某条 `messages[].content` 的文本 |
| 光标周围文本 | `context_data[].text`，`context_type` 为 `dialog_ctx` | 放进某条 `messages[].content` 的文本 |
| 宿主应用标识 | 公开识别字段表中没有同名键 | 公开对话没有同名键；只能作为文本出现在 `content` 里 |
| 纠错对 | 已读字段是控制台词表标识，不是当场的 src 到 dst | 放进某条 `messages[].content` 的文本 |

纠错对这条是缺口，不是绕过：本次读到的流式识别文档没有「请求体内直接带若干 `(src, dst)`」的字段。端到端实时语音另一篇文档的示例里出现过 `context.hotwords` 与 `correct_words` 映射，那是另一条接口（实时对话），不是本篇流式识别。不把那张映射借来当成 `bigmodel_async` 的字段。

对话侧没有槽位对象。候选是适配器把缺席以外的槽位写成 `messages` 里的文本，定稿假设单独作为待优化文本放进另一条 `content`，避免和上下文混成不可区分的一段。`model` 的具体型号是调用方凭证下已开通的模型名，本文不指定一个型号。

样本侧，上下文进入私有识别的痕迹是另一套名字，只记形状：`TaskRequest extra`、`hotwords`、`OimeAudio.ASRContext.HotwordContext` 的 `word` / `type` / `frequency`，以及 `modify_pair`、`user_word`。语音整理对象上另有 `contextBeforeRequest`、`contextAfterRequest`、`hostBundleID`、`compositionText`、`originalResult`。私有同步路径字面量见过 `/api/v3/context/ime/user_words` 与 `/api/v3/context/ime/modify_pair/v2`，主机字面量见过 `https://ime.doubao.com`。这些路径没有出现在本次读到的语音开放文档里。调用方的热词直传不等于这些私有同步路由。

### 3.4 私有长连接：只记已观察到的形状与鉴权材料位置

下列名字来自 `DoubaoIme` 的字符串与类转储。不附取值，不附构造方法。

音频长连接字面量两侧紧邻的短字符串包括：`speech_opus`、`proto-version`、`sdk-version`、`x-tt-token`、`x-use-ppe`、`x-tt-env`、`loc_info`、`ime_info`、`user_input`、`asr_input`、`context`、`enable_asr_twopass`、`enable_asr_threepass`、`frequency`。日志格式里反复出现 `event`、`payload`、`seq_id`、`task_id`、`status_code`、`status_text`、`message_id`。发送侧日志名包括 `StartTask`、`SendAudioData`、`FinishTask`，完成侧包括 `TaskFinished`、`onTaskFinished`。

`show_utterances`、`definite`、`utterances` 也在同一二进制的字符串表里。它们紧挨着 `flute/.../asr_executor_with_context.cc`，与本地执行器路径在一起，不与上面的 `wss://frontier-audio-ime-*` 字面量相邻。因此这三枚名字不能当成私有长连接 JSON 已被证实的键。公开识别文档自己使用 `show_utterances` 与 `definite`。两处同名是事实，是否同一条线上的键，未知。

鉴权材料的位置：

- 类 `OimeAudio.SAMITokenManager` 的实例变量名：`samiAppKey`、`fetchTokenUrl`、`token`，另有 `refreshTokenInterval`。
- 类 `OimeAudio.SAMIHandle` 的实例变量名含 `voiceAuthorization`。
- 字符串表中的 URL 模板：`/api/v1/token?access_key=`，主机字符串 `https://speech.bytedance.com`。本文不读取、不记录该查询的取值。
- 整理请求对象上的 `encryptionValidated`，以及 `encryption_validation_failed` 一类状态名，位于 `SmartOrganize` 的处理函数附近。

这些位置说明：私有长连接的可用凭证与整理通道的加密校验都出现在官方进程内部。公开的 `X-Api-Key` 与 `ARK_API_KEY` 是调用方控制台的另一套材料，文档没有写它们可以替换 `x-tt-token` 或 `SAMITokenManager.token` 去访问 `frontier-audio-ime-*` 与 `ime.doubao.com` 的整理路径。该差异记成缺口。

### 3.5 一次会话上的候选迁移

迁移只连接上面三块契约，不引入新的角色：

1. 会话打开后，绑定把识别角色接到语音开放接口的适配器，把改写角色接到方舟对话的适配器。任一侧契约不可用时，该角色空着，另一侧的输入输出类型不变。
2. 采集帧进入识别适配器。适配器按帧上的格式填写 `audio`，按顺序发送音频包，并把 `X-Api-Request-Id` 与会话标识的对应留在适配器内。
3. 每次识别响应更新会话内的假设。`definite` 未真时，不调用对话接口。
4. 一条分句定稿后，改写适配器读取该条文本和当时的快照，发起对话请求。快照槽位缺席则对应文本不出现。
5. 对话在 `finish_reason` 之后给出一次「不替换」或「替换文本」。在此之前，会话里的定稿假设保持原样，供呈现读取。
6. `is_last_package` 结束识别侧的新假设。它不代替改写输出，也不产生提交。

键盘云端的 `libime_net_sdk` 与 `parseCloudAndLLMResponse` 不在这条迁移里。那条调用吃的是键盘云端结果与 `precedingText`，输出进候选融合，不是采样帧的识别假设，也不是定稿语音文本的改写。

## 4. 当前倾向（候选）

候选倾向如下。

云端绑定是两个适配器，不是一条私有长连接。识别适配器的可调用契约是豆包语音公开流式识别：调用方自己的 `X-Api-Key`（或旧版 App Key 与 Access Key）加 `X-Api-Resource-Id`，输入为带格式的音频，输出为 `result.text` 与带 `definite` 的分句。改写适配器的可调用契约是方舟公开对话：调用方自己的 `ARK_API_KEY`，输入为定稿文本与快照装填后的 `messages`，输出在 `finish_reason` 之后映射成原语 5 的「不替换」或「替换文本」，并始终保留原假设。

两套凭证各管各的主机。它们一起盖住「流式听写」和「定稿后用大模型产出另一段文本」这两类输入输出。它们不盖住 Frontier 音频长连接、`/api/v1/token?access_key=` 那条令牌获取、以及 `/api/v2/ai/text_organization` 上的 `rewriteDecision` 与进程内加密校验。后三者保持为缺口，角色在缺口上为空，不改成绕过。

上下文的候选是一份快照、两处装填：个人词与周围文本进入识别的 `corpus.context`；同一份文本进入对话的 `messages`。宿主应用标识与当场纠错对在识别字段表里没有同型键，只在对话文本里有落点，识别侧记成缺口。

`enable_ddc` 与 `enable_nonstream` 留在识别请求内，作为识别假设的修订，不充当改写流。

反论：对话接口的 `delta` 已经是文本，挡到 `finish_reason` 才输出，等于放弃公开接口已经给出的增量。若呈现只把改写增量画进预编辑、且不把增量当成提交，原语 7 的「尚未到达不发明优化文本」仍可能成立。本次没有宿主测量，所以「增量留在适配器内」只是候选挡法，不是原语 5 的不变量。

另一反论：语义顺滑已经会改字，再接一次对话会把同一次口误改两遍，调用方无法从最终字符串看出哪一遍改的。候选仍把两遍分成识别假设与改写输出，是为了换掉对话模型时不必拆识别请求。若将来的契约证明对话模型只是在重复 `enable_ddc`，绑定仍允许改写角色接恒等。恒等满足原语 5。

## 5. 仍然未知的点

1. 方舟对话的完整 URL 路径。本次官方 Base URL 页正文被导航截断，没有重新读到路径；只核对到数据面域名 `ark.cn-beijing.volces.com`、Bearer 头，以及 `messages` / `content` / `finish_reason` / `stream` 的形状。
2. `bigmodel_async` 是否接受 `opus`。示例载荷使用 `codec` 为 `raw`，旧文档写过 `opus`，本页字段表未单独列出 `codec`。
3. `definite` 为真之后，同一条 WebSocket 还会不会改写该分句文本。文档用语是「最终确定的分句结果」，没有用本次探测的音频去核对。
4. 公开对话没有 `rewriteDecision`。返回与原文逐字相同、返回空 `content`、或返回带解释的前后缀时，哪一种算原语 5 的「不替换」，没有协议字段可核对。
5. 流式识别的 `corpus.context` 不能直接放入会话当场的纠错对；控制台只看到词表标识。当场的 `(src, dst)` 若要影响识别，公开字段是哪一个，未知。
6. 宿主应用标识没有公开类型字段。写进 `messages[].content` 之后，模型是否按该标识改变文本，未知。
7. 私有 Frontier 帧的二进制布局没有逐字段解开。已记的是日志名、邻近短字符串和头名字面量。`definite` 那一组键与本地 `flute` 路径相邻，不能算作私有长连接的已证 JSON。
8. `SAMITokenManager.token`、`samiAppKey` 与 `x-tt-token` 的取值未读取。公开语音密钥与方舟密钥能否代替它们访问私有主机，文档没有这样写。按缺口放置，不另做验证。
9. 整理通道的 `encryptionValidated` 依赖的材料只出现在官方进程内。公开对话不覆盖这条路由。
10. 键盘解析器上的 `precedingText` 与语音整理对象上的 `contextBeforeRequest` 是否同一枚线上 JSON 键，未证实。
11. 方舟目录中的「音频理解」请求是否接受有序采样帧并给出部分假设，本次未读该页。

## 6. 探测日志

| # | 工具 | 查询或对象 | 产出 |
| --- | --- | --- | --- |
| 1 | `web_search` | 火山引擎豆包语音流式识别、开放接口、调用方凭证 | 命中实时识别与大模型流式识别文档：主机 `openspeech.bytedance.com`，路径 `/api/v3/sauc/bigmodel` 与 `_nostream`、`_async`；头为调用方的 `X-Api-App-Key` / `X-Api-Access-Key` 或 `X-Api-Key`，加 `X-Api-Resource-Id` |
| 2 | `web_search` | 火山方舟豆包大模型对话 API、`Authorization`、`ARK_API_KEY` | 命中对话 API 与 Base URL 说明：`Authorization: Bearer $ARK_API_KEY`，体含 `model` 与 `messages`，响应含 `choices[].message.content` |
| 3 | `web_fetch` | `https://docs.volcengine.com/docs/doubao-voice/bidirectional-streaming-automatic-speech-recognition-websocket` 前段 | 读到 `X-Api-Key`、`X-Api-Resource-Id` 的模型 1.0/2.0 资源名、`X-Api-Request-Id`，以及 `audio.format/rate/bits/channel` 的默认值 |
| 4 | `web_fetch` | 同一文档偏移 15000 | 读到 `enable_ddc`、`enable_nonstream`、`corpus.context` 的 `hotwords` 与 `dialog_ctx`、100 tokens 截断、以及 `definite` / `is_last_package` 输出示例 |
| 5 | `web_search` | `text_organization`、`ime.doubao.com`、语音智能整理开放接口 | 未命中该私有路径的开放文档；命中的是实时对话、智能体等其他产品页 |
| 6 | `web_search` 与 `web_fetch` | 方舟「流式输出」 | 读到对话接口以 `stream` 为真返回增量，按 `choices[].index` 聚合 `delta`，以 `finish_reason` 结束 |
| 7 | `web_fetch` 与 `web_search` | 方舟 Base URL 页；对话完成体的第三方镜像仅作对照 | 官方页确认域名文档与 Bearer 头的入口；正文路径被导航截断。完成体字段以官方对话示例为准，不采用镜像里的路径 |
| 8 | 本地字符串扫描 | `/tmp/doubao-thin-arm64/DoubaoIme` | 读到两条 `frontier-audio-ime` 的 `wss` 字面量、`/api/v1/token?access_key=`、`/api/v2/ai/text_organization`、`format.completed`、`scene.completed.empty_content`；未抄录任何令牌取值 |
| 9 | 类转储 | `/tmp/doubao-thin-arm64/classes-dump.txt` | 定位 `SAMITokenManager` 的 `samiAppKey` / `fetchTokenUrl` / `token`，`SAMIHandle.voiceAuthorization`，整理请求的 `query` / `rewriteDecision` / `encryptionValidated`，以及 `ASRSmartOrganizeContext` 的 `hostBundleID` 与前后文槽 |
| 10 | 邻近字符串 | 私有 `wss` 字面量附近，以及 `definite` 字面量附近 | 前者旁是 `x-tt-token`、`speech_opus`、`proto-version` 等名字；后者旁是 `flute` 的 `asr_executor_with_context.cc`，故不把 `definite` 记成私有长连接已证字段 |

<!-- derivation-complete -->
