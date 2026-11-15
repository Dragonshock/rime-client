# 063-research-streaming-partials：说话中每满 2 秒回一条部分结果，定稿仍与改造前逐字相同

测量日 2026-11-14。对象是 `code/voice-engine`（`voice-mlx` 新增
`backend/stream.rs` 与 `Engine::stream_step`；`voice-ffi` 的 `WeightDecoder` 在非定稿步
走流式路径、松开后排队的部分步跳过；`voice-asr` 的 `Session::apply` 不下发空的部分结果）
与 `code/Client`（只核了不需要改，见第 3 节）。口径是 `dev` 档案，与装包同档；
模型目录与 `mlx.metallib` 取自已安装包的 `Contents/Resources`。`dev-docs/refs`、
`inject`、librime 都没动，Python 没有进产品路径，容差一个数没改。

## 0. 结论速览

| 项 | 改造前 | 现在 |
| --- | ---: | ---: |
| 4 秒语音的回调条数 | 1（只有 `final`） | 2（`partial` + `final`） |
| 4 秒语音第一次出字 | 4.62 秒（松开后定稿） | **2.40 秒**（音频第 2.0 秒后 0.40 秒） |
| 4 秒语音 `finish` 墙钟（三次中位） | 0.608 s | 0.638 s |
| 10 秒语音的回调条数 | 1 | **5**（4 条 `partial` + 1 条 `final`） |
| 10 秒语音四条 `partial` 的时刻 | — | 2.384 / 4.606 / 6.789 / 8.790 s |
| 10 秒语音 `finish` 墙钟 | 1.405–1.555 s | 1.406 s |
| 2 秒探针（基线音频）`finish` 墙钟 | 0.232–0.242 s | 0.229–0.244 s |
| 探针定稿 token | `[106287, 1773, 151645]` | `[106287, 1773, 151645]` |
| 4 秒 / 10 秒语音的定稿文本 | 见第 4、5 节 | 逐字相同 |
| `cargo test --workspace --offline` | 51 通过、4 忽略 | 53 通过、4 忽略 |

4 秒音频按每 1 秒一块送，编码步是 2.0 秒，所以音频里只有一个 2 秒边界落在松开之前；
松开本身正好落在第二个 2 秒边界上，那一步被跳过（见第 2.3 节），因此 4 秒只有一条
`partial`。10 秒那条把四个 `partial` 都留下了，文本逐步变长。

## 1. 引擎侧：042 第 5 节的表怎么落到代码里

新增 `code/voice-engine/voice-mlx/src/backend/stream.rs`。`Stream` 只带逐次变化的
东西，权重仍在常驻的 `Engine` 里：

| 状态 | 内容 |
| --- | --- |
| `pcm` | 这一次按住的全部样本 |
| `prefix` | 已结束各窗的 1024 维投影输出，行优先 `(token, 1024)` |
| `encoded_frames` | 已经进 `prefix` 的梅尔帧数 |
| `tokens` | 上一步的整段假设，含停止符 |
| `unfixed` | 允许下一步改写的尾巴长度，取 042 表的 5 |

`Engine::stream_step`（实现在 `stream_step_with_opened`）一步做四件事：

1. **整段重算梅尔**。不能只算新样本：参考在 `(x + 4) / 4` 之前先减「全局最大值减 8」，
   那个最大值要看整段；末尾的帧还要用到右端的反射补边。代价是线性于历史长度：
   2 秒 0.051 s、4 秒 0.103 s、6 秒 0.157 s、8 秒 0.201 s（第 5 节的实测）。
2. **满 800 个梅尔帧就编一段并接进 `prefix`**。窗边界与一次性编码那条路完全一致：
   800 = 8 × 100（块长），位置表本来每块从 0 起算，窗与窗之间互不看。两条路是同一条
   算术（整段那条用块对角加法 mask，被 mask 的项 `exp = 0`），但**不是逐位相同**：
   1200 帧一次编完（156 个 token）与切成 800 + 400 两段编（104 + 52 个 token）实测
   投影最大绝对差 **1.0691956e-4**，来自 f16 的归约次序（批长不同、核的 tiling 不同），
   与 `encoder_parity` 的投影差同一档。**8 秒以内两条路是同一次调用、同一个张量**，
   因此逐位相同——这就是 2 秒、4 秒、8 秒这些长度上定稿一定与改造前一致的原因。
3. **活动窗每步重编**。它是唯一会随新样本改写的一段，也是「句尾可以改」的来源。
4. **增量解码**。提示 = 模板（含 `audio_pad` 占位）+ 已固定的文本；只有未固定的
   5 个 token 加上新冒出来的那几个要重算。

`prefix` 的复用是真的：14 秒那条（第 5.2 节）在 10 秒与 12 秒两步里 `prefix` 已经是
104 个 token，`本步冻结 0`，编码只跑活动窗的 200 / 400 帧，编码墙钟 0.032 / 0.046 s；
如果每步重编整段，同样这两步要编 1000 / 1200 帧。

### 1.1 跨步没有复用 KV 缓存，理由是位置对不上

042 第 5 节写「复用 KV 缓存」，这里必须写清哪一半做到了：

- **步内**复用：`generate` 预填充一次，之后每步只喂 1 个 token，键值走 KV。
- **跨步不复用**：提示里的音频占位个数每步都变（26 → 52 → 78 → 104），
  `<|audio_pad|>` 之后的每一个位置整体平移，RoPE 位置随之改变，上一步算出的键值
  对不上新位置。整段音频是提示里的一段连续占位，没有位置不动的写法。
- 跨步真正复用的是**编码器前缀**与**已固定的文本**：后者写进提示重新预填充
  （预填充是批量矩阵乘，比逐 token 生成便宜得多），只有未固定的尾巴重新生成。

所以「复用 KV」在这一版是步内复用 + 跨步复用前缀与文本，不假装跨步键值还能用。

## 2. 端口侧：不是定稿时也回调

### 2.1 `partial` 的形状

`voice-ffi/src/decoder.rs` 的 `WeightDecoder::decode_text` 分两条：

- `view.is_final == false` → `decode_partial`：把这一步的样本推进会话的 `Stream`，
  借用常驻实例跑 `Engine::stream_step`。
- `view.is_final == true` → 原来那条一次性路径，整段 PCM 重算。

协议一个字没改：`voice-ffi/src/json_line.rs` 早就支持 `kind:"partial"`，
`SessionSink` 按 `event.is_final` 选 `final` / `partial`，`decoded` 恒为真。实测的一行：

```
{"decoded":true,"kind":"partial","ok":true,"session":1,"text":"今天天气不错，我们。"}
```

`decoded` 为真是对的：这条回复确实带着一段解出来的文本。

### 2.2 部分步失败不当成识别失败

说话中某一步出错（没 GPU stream、编码器报错等）时 `decode_partial` 只回空串，
不写 `failure`、不置 `weight_failed`：定稿会把整段重算一遍，中间一步没算出文本
不等于这一次识别失败。原因只在设了 `VOICE_STREAM_LOG=1` 时打到 stderr。

### 2.3 松开之后不再跑排队的部分步

`voice_session_finish` 一进来就把 `Worker` 里那一位 `finishing`（`Arc<AtomicBool>`）
置真；排在 `Finish` 前面的 `Feed` 出队时看见这一位，就跳过那一步的部分识别。理由：
定稿会把整段重算一遍，那一步的结果一定被丢掉，白跑只会把定稿拖后。

这一步是量出来的，不是猜的：不跳过时，4 秒语音的 `finish` 是 **1.14 秒**（先等上一次
部分步跑完 0.56 秒，再跑定稿 0.58 秒），2 秒探针是 **0.43 秒**（0.24 + 0.19）；
跳过之后回到 **0.62–0.65 秒** 与 **0.23–0.24 秒**，与改造前的 0.61 秒、0.23 秒同一档。
正在跑的那一步不能撤回，所以松开与部分步真正撞上时仍会等它跑完；2 秒探针那次
（松开正好落在唯一的 2 秒边界上）落在 0.23 秒，因为那一步出队时就看见这一位、
没有开始跑。

### 2.4 空的部分结果不下发

`voice-asr/src/session.rs` 的 `apply` 加了一条：非定稿步的文本若是空串，直接返回，
不改 `published`、不回调。空串在部分步里表示「这一步没有新文本」，不是「整段变空」——
按原来的写法它会发一条 `text:""` 的 `partial`，把用户眼前的预编辑清掉，还会把
`lastShown` 变成空串，让「只清自己那一串」的判断失效。定稿的空串照旧下发。

## 3. Swift 侧：确认不用改

`partial` 这条路本来就在：`VoiceHandleGate.receive` → `VoiceMachine.acceptPartial`
→ `.show` 效果 → `SquirrelInputController.voiceShow(text:caretUTF16:)`
→ `show(preedit:selRange:caretPos:)`。交付语义没动：定稿仍走 `.commit` →
`voiceCommit` → `insertText`，部分结果只写 `setMarkedText`。

两层各有一次「同文不重发」：`Session::apply` 只在文本变化时回调，`VoiceMachine.acceptPartial`
再挡一次。这两条都没改。

`code/VoiceInterface` 27 条、`code/Client` 9 条测试全通过；`swift build --product Squirrel`
exit 0，产物 `code/Client/.build/out/Products/Debug/Squirrel` 40597808 字节（16:38）。
安装包没有重装（见第 7 节）。

## 4. 实测：4 秒语音，按 1 秒一块送

音频是 `say -v "Flo (中文（中國大陸）)"` 生成的
「今天天气不错，我们下午去公园散步吧。」（3.95 秒，补零到 4.00 秒，64 000 个样本），
放在 `/tmp/voice-abi/speech4.bin`，没有进仓。计时程序是新写的
`/tmp/voice-abi/bench5.c`：`voice_session_begin` 之后按 16 000 个样本一块送，
每块之间睡 1 秒（音频时间与墙钟对齐），每条回复回调打上相对第一次 `feed` 的时刻。
链接用 `/tmp/voice-abi/link.sh`，归档是 `cargo build -p voice-ffi --features metal` 的
`dev` 档产物。

「改造前」一栏用的是同一个 `bench5.c` 链到改造前归档的产物（`bench5-before`，
改造前源码重编 `libvoice_ffi.a` 后重链），因此两边只差本次这一刀。

| 时刻（相对第一次 `feed`） | 改造前 | 现在 |
| --- | --- | --- |
| 回调 1 | — | `partial` 2.394 s「今天天气不错，我们。」 |
| 回调 2 | `final` 4.621 s | `final` 4.658 s |
| `finish` 墙钟 | 0.608 s | 0.638 s |

三次（三次是三个新进程）：

```
改造前：final at=4.621 / 4.624 / 4.628   finish 0.608 / 0.608 / 0.610 s
现在：  partial at=2.394 / 2.408 / 2.412，final at=4.642 / 4.658 / 4.663
        finish 0.622 / 0.638 / 0.649 s
```

唯一那条 `partial` 的步内分解（`VOICE_STREAM_LOG=1` 的一项，新进程第一次跑）：

```
stream partial mel=0.051s(200帧) encode=0.075s(活动窗200帧,冻结0个token)
               decode=0.265s(提示44个,固定0个,新7个) 音频token=26 前缀token=0
               文本="今天天气不错，我们。"
               tokens=[100644, 104307, 100832, 3837, 97639, 1773, 151645]
```

即 0.391 秒：梅尔 0.051 + 编码 0.075 + 预填充与 7 个 token 的贪心 0.265。回调落在
音频第 2.0 秒之后 0.40 秒。第二个 2 秒边界正好是松开的时刻，那一步被跳过（第 2.3 节），
所以 4 秒只有一条 `partial`；若不跳过，它会在 4.58 秒回一条与定稿同文的 `partial`，
而定稿要等这一步跑完、到 5.15 秒才出（第 2.3 节量的 1.14 秒就是这一段）。

## 5. 10 秒语音：四个 `partial` 逐步变长

音频是同一声音生成的
「今天天气不错，我们下午去公园散步吧。晚上回来的时候顺便买一点水果和牛奶，明天早上做早餐用。」
（9.54 秒，补零到 10.00 秒），`/tmp/voice-abi/speech12.bin`。

| 时刻 | 一条 | 文本 |
| ---: | --- | --- |
| 2.384 s | `partial` | 今天天气不错，我们。 |
| 4.606 s | `partial` | 今天天气不错，我们下午去公园散步吧。 |
| 6.789 s | `partial` | 今天天气不错，我们下午去公园散步吧。晚上回来的时候，顺便买一。 |
| 8.790 s | `partial` | …顺便买一点水果和牛奶。明天早。 |
| 11.483 s | `final` | 今天天气不错，我们下午去公园散步吧。晚上回来的时候，顺便买一点水果和牛奶。明天早上早早早用。 |

改造前：只有一条 `final`，11.474 / 11.592 秒，`finish` 1.405 / 1.555 秒；文本与上表
最后一行逐字相同。

每一步的分解（`VOICE_STREAM_LOG=1`，同一次运行）：

| 音频位置 | 梅尔 | 编码 | 解码（提示 / 已固定 / 新） | 音频 token | 前缀 token |
| ---: | ---: | ---: | --- | ---: | ---: |
| 2 s | 0.051 s（200 帧） | 0.060 s（活动窗 200 帧，冻结 0） | 0.260 s（44 / 0 / 7） | 26 | 0 |
| 4 s | 0.103 s（400 帧） | 0.066 s（活动窗 400 帧，冻结 0） | 0.403 s（71 / 1 / 11） | 52 | 0 |
| 6 s | 0.157 s（600 帧） | 0.072 s（活动窗 600 帧，冻结 0） | 0.511 s（102 / 6 / 14） | 78 | 0 |
| 8 s | 0.201 s（800 帧） | 0.080 s（活动窗 0 帧，冻结 104） | 0.449 s（136 / 14 / 12） | 104 | 104 |
| 10 s | — | — | — | — | —（与松开重合，跳过） |

8 秒那一步把第一个 8 秒窗冻结进 `prefix`：活动窗为 0 帧，`prefix` 从此是 104 个
token。10 秒那一步被跳过，所以这次没有「前缀复用且活动窗非零」的行；那一行在下面
的 14 秒对照里。

### 5.1 逐层的闸与 token

`voice-mlx/tests/end_to_end_parity.rs` 的 `greedy_sequence_and_text_match_reference`
仍是 `[106287, 1773, 151645]` 与「嗯。」，逐位不变；编码器 3 条、解码器 6 条、端到端
4 条全过（`--test-threads=1`）。流式路径没有另开闸，它复用同一条 `encode_mel` /
`TextDecoder` / 分词器，没有第二套算术。

### 5.2 编码器前缀复用的直接证据（14 秒，`voice-mlx/tests/streaming.rs`）

`--ignored` 的实测把探针重复 7 次（14 秒）按 1 秒一块送，六步：

```
部分 2.00 秒 音频token 26（前缀0，冻结0，活动窗200帧） 编码 0.033 s
部分 4.00 秒 音频token 52（前缀0，冻结0，活动窗400帧） 编码 0.035 s
部分 6.00 秒 音频token 78（前缀0，冻结0，活动窗600帧） 编码 0.045 s
部分 8.00 秒 音频token 104（前缀104，冻结104，活动窗0帧）编码 0.054 s
部分 10.00 秒 音频token 130（前缀104，冻结0，活动窗200帧）编码 0.032 s
部分 12.00 秒 音频token 156（前缀104，冻结0，活动窗400帧）编码 0.046 s
```

10 秒与 12 秒两步的 `prefix` 已经是 104 个 token 且本步冻结 0，编码只跑活动窗；
音频 token 总数 130 = 104 + 26、156 = 104 + 52，行数对得上。同一条测试里
「整段重算」与「定稿」的 token 与文本逐位相同（4 秒与 14 秒两个长度都断言了）。

## 6. 验收：定稿与基线一致

| 音频 | 基线 token / 文本 | 现在定稿 |
| --- | --- | --- |
| 2 秒探针 | `[106287, 1773, 151645]` / 「嗯。」 | 相同（`end_to_end_parity` 对探针逐位断言；`streaming.rs` 断言定稿与整段重算相同，并打出 `[106287, 1773, 151645]`） |
| 4 秒语音 | 「今天天气不错，我们下午去公园散步吧。」 | 相同（`bench5-before` 与 `bench5` 的 `final` 行逐字比对） |
| 10 秒语音 | 「…明天早上早早早用。」 | 相同（同上） |

定稿为什么一定与改造前相同：定稿仍然走 `transcribe_on_resident` → `Engine::transcribe`，
即整段 PCM 重算那条老路，本次没有改它的一个算子。流式状态在定稿时被丢掉，不参与
任何一步。

测试（全部 exit 0）：

```
cd code/voice-engine && cargo test --workspace --offline
  voice-asr 10、voice-ffi 12、voice-mel 5、voice-parity 7+4（4 条 ignore）、
  voice-tensor 6、voice-tokenizer 4+5：53 通过、4 忽略、0 失败

cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test encoder_parity --test decoder_parity --test end_to_end_parity -- --test-threads=1
  encoder 3、decoder 6、end_to_end 4，全过

cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline --lib
  5 条，含新增的 stable_prefix_drops_stop_tokens_and_the_unfixed_tail、
  mel_columns_takes_one_contiguous_window_per_bin
```

新增的测试：`voice-asr` 的 `an_empty_partial_step_does_not_retract_the_published_text`、
`voice-ffi` 的 `the_decoder_factory_receives_the_finishing_flag`、`voice-mlx`
`--lib` 那两条，以及 `--ignored` 的 `voice-mlx/tests/streaming.rs` 两条
（`partials_every_two_seconds_and_the_final_still_matches` 与
`cutting_a_window_stays_within_the_f16_spread_of_one_pass`）。

## 7. 仍然未知与仍然偏慢的地方

- **梅尔每步整段重算。** 0.025 秒／秒音频，8 秒的音频一次部分步要 0.20 秒。
  想省掉它要先解决两件与整段有关的事：`(x + 4) / 4` 之前的「全局最大值减 8」，
  以及末尾两帧的右端反射补边。可以按帧缓存 `log10` 结果、把最大值挪到用时再减，
  末尾两帧在每次新样本到达时重算——这一版没做。
- **每步贪心按 token 计费。** 一个 token 约 0.036 秒：6 秒那一步新生成 14 个 token，
  解码 0.511 秒，是那一步里最大的开销。`unfixed` 是 5，加上这一步新冒出来的文本，
  一步要做 10 个上下的解码步。批量解码或把 5 调小都能压，但都会改变「句尾可改写」
  的范围，没有在这一版里动。
- **定稿仍不复用编码器前缀。** 10 秒以上、已经冻结了窗时，定稿还会从头重编一遍。
  复用会把 1.0691956e-4 那一档差（第 1 节第 2 点）带进定稿，等于把定稿的数值路径换成
  另一条；本次按「定稿不动」处理，所以定稿墙钟与改造前同档（10 秒 1.41 秒），
  定稿文本与改造前逐字相同（第 6 节）。
- **8 秒以上的部分结果可能与定稿差一两个字。** 前缀复用那几步的编码器输入与整段
  一次编码差在 1e-4 这一档，贪心的 argmax 可能因此翻一两个 token。定稿走整段重算，
  以它为准；8 秒以内不存在这一档差。
- **第一次按键仍要付 4.8 秒。** `voice_engine_new` 在这几次新进程里是 4.80–4.82 秒
  （含常驻权重），与 062 的 4.81–5.68 秒同一档，本次没有碰。
- **麦克风到屏幕仍没量。** 全部是文件探针喂 `voice_session_feed`；采集、预编辑上屏、
  松开提交这一段只有 Swift 侧的单元测试，没有真实按键。
- **探针音频对模型只有一句话。** `probe_pcm.bin` 是合成波形，模型对它只回「嗯。」，
  重复拼接仍是同一句，所以 4 秒那条只出一条 `partial`；文本逐步变长的证据用的是
  `say` 生成的语音（第 4、5 节），这两段音频只在 `/tmp`，没有进仓。
- **安装包没有重装。** `~/Library/Input Methods/RimeClient.app` 里仍是 14:57 的产物
  （40499776 字节），与 062 之后的状态相同；本次所有秒数都是在
  `code/voice-engine` 的新归档上量的，装包要另跑 `make-app.sh` 与 `ditto`。

<!-- research-complete -->
