# 070-research-step-scaling-result：增量梅尔与提示键值复用之后的验收，装载与定稿逐项复测，剩下的是贪心

测量日 2026-11-15。这一篇做四件事：把 068（增量梅尔）与 069（提示头与已结束各窗的行
只预填充一次）之后的**三套数值闸与整个工作区的测试**跑一遍、**重编归档与 Swift 产物**、
把 `engine_new` / 暖定稿 / 14 秒探针每步墙钟**与 065 / 066 / 067 并列**量一遍、
**重打包装回 `~/Library/Input Methods/RimeClient.app` 并验签名与包内资产**。

这一篇没有改代码：`code/` 一个算子没动，`dev-docs/refs` 没动，librime 没动，`inject` 没调，
容差一个数没改。`code/Client`、`code/VoiceInterface`、`code/voice-oracle` 也没动。

## 0. 结论速览

| 项 | 068/069 之前 | 现在 | 依据 |
| --- | ---: | ---: | --- |
| 14 秒探针六步合计（第 1…6 步墙钟） | 2.103 秒（0.194…0.503 增量） | **1.130 秒**（0.172 / 0.363 / 0.561 / 0.769 / 0.945 / 1.130，四次中位） | §3.2 |
| 第一步 → 第六步的增量 | 0.194 → 0.503 秒（2.6 倍） | **0.172 → 0.185 秒**（1.08 倍） | §3.3 |
| 每步梅尔 | 0.050 → 0.300 秒（随已收音频线性） | **0.035 → 0.037 秒**（第六步/第一步 0.98–1.04） | §3.3 |
| 12 秒处预填充 | 整条提示 174 个位置、0.073 秒 | **重算 61 个位置、0.041 秒** | §3.3、§3.4 |
| `voice_engine_new` | 0.137 秒（067 九次中位） | **0.134 秒**（六次中位；0.128–0.270） | §2.1 |
| 暖定稿（2 秒探针） | 0.193–0.200 秒（067） | **0.178 秒**（三次中位；0.173–0.212） | §2.2 |
| 冷定稿（进程起第一次） | 1.024 秒（067 六次中位） | 0.823 秒（六次中位；0.801–1.018） | §2.2 |
| 按下→出字（2 秒说话） | 2.401–2.513 秒（066） | 2.229–2.291 秒（装好的包，两次） | §2.2 |
| token / 文本 | `[106287, 1773, 151645]` / 「嗯。」 | 同左（每步、定稿、30 秒探针逐位比过） | §4 |
| 增量梅尔与整段重算的差 | 6.866455e-5（容差 1e-3） | **6.866455e-5**，逐长度差 0 | §4.1 |
| 复用与整段预填充的 logits 差 | 7.8125e-2（余量 1.02 倍） | 7.8125e-2（逐位不变） | §4.2 |
| `cargo test --workspace --offline` | 61 通过、4 忽略 | **61 通过、4 忽略、0 失败** | §1.1 |
| 三套 metal 闸 | 3 / 6 / 4 全过 | **3 / 6 / 4 全过**，差值逐位同 067 | §1.2 |
| 装好的包 | 15 Nov 06:59 的产物 | 15 Nov 07:57 重打重装，`codesign --verify --deep --strict` exit 0 | §5 |

一句话：**068 与 069 把「每步随已收音频线性长」这件事按住了——梅尔从 0.050…0.300 变成
平的 0.035…0.037，预填充从 0.108 变成 0.043 且在 30 秒处仍是 0.045…0.060，
六步合计 2.103 → 1.130 秒。剩下的涨幅在编码（随活动窗宽 0.030…0.053）与解码里的两次贪心
前向（约 0.070 秒/步、42 毫秒/token）——后者是现在一步里最大的一块，也是预填充之后下一处
该动的地方。**

## 1. 三套闸与整个工作区

### 1.1 `cargo test --workspace --offline`（不打开 metal）

```
voice-asr 10、voice-ffi 13、voice-mel 12、voice-parity 7+4（另有 4 条 #[ignore]）、
voice-tensor shapes 6、voice-tokenizer 4+5
→ 61 通过、4 忽略、0 失败，exit 0
```

逐箱与 069 记的同一组数。

### 1.2 打开 metal 的三套闸（`--test-threads=1`）

```
encoder_parity      3 passed
  conv_stem_out       实测差 0e0（0.0 个单位），容差 3.955841e-3（4 个单位）
  encoder_layer0_out  实测差 3.90625e-3（1.2 个单位），容差 1.2954712e-2（4 个单位），余量 3.32 倍
  encoder_proj_out    实测差 1.1855885e-3（19.2 个单位），容差 1.9779205e-3（32 个单位），余量 1.67 倍
decoder_parity      6 passed
  decoder_prefill_matches_reference     7.8125e-2（容差 8.203125e-2，余量 1.05 倍），argmax 106287
  decoder_with_reference_audio_features 6.25e-2（余量 1.31 倍），argmax 106287
  quantized_paths_are_compared_group_by_group 两组都走量化：7.8125e-2
  另三条（权重装载、嵌入行、错误选项放大）通过
end_to_end_parity   4 passed
  prompt_matches_reference      44 个 id 逐位相同
  greedy_sequence_and_text_match_reference  [106287, 1773, 151645] / 「嗯。」
  cached_step_matches_a_fresh_prefill      缓存第二步 1773 = 重算的 argmax 1773
  empty_audio_gives_empty_result
```

五个数与 067 / 068 / 069 记的逐位相同（0 / 3.90625e-3 / 1.1855885e-3 / 7.8125e-2 /
6.25e-2），argmax 全 106287。容差没有被改宽，也没有测试被跳过或标 `#[ignore]` 新躲开。

## 2. 装载与定稿：与 065 / 066 / 067 并列

### 2.1 一张表

| 项 | 065（改前） | 066（分两段） | 067（验收） | 本次（068 + 069 之后） |
| --- | ---: | ---: | ---: | ---: |
| `voice_engine_new` | 4.841 秒（六次中位） | 0.756 秒（十二次中位，只装第一段） | 0.137 秒（九次中位） | **0.134 秒**（六次中位；0.128–0.270） |
| 暖定稿（2 秒探针） | 0.199–0.206 秒 | 0.202–0.211 秒 | 0.200 / 0.193 秒 | **0.178 秒**（三次中位；0.173–0.212） |
| 冷定稿（进程起第一次） | 0.222–0.263 秒 | 0.97–1.02 秒（第二段的代价） | 1.024 / 0.972 秒 | 0.823 秒（六次中位；0.801–1.018） |
| 按下→出字（2 秒说话，4096 一块实时送） | — | 2.268–2.376 秒 | 2.401–2.513 秒 | 2.229–2.291 秒（装好的包，两次） |
| 松手→出字（同上） | — | 0.2 秒档 | — | 0.198–0.260 秒 |
| token / 文本 | `[106287, 1773, 151645]` / 嗯。 | 同左 | 同左 | 同左 |
| 解码器主闸首 64 维最大差 | 4.7e-2 档 | 7.8125e-2 | 逐位不变 | 逐位不变（7.8125e-2） |

口径与 067 相同：`dev` 档案、M2 Pro / 16 GB、探针 `code/voice-oracle/assets/probe_pcm.bin`
（2.00 秒、32 000 样本）、metallib 是包里那份 183 870 464 字节（`sha256 d523d602…c722`，
与 `code/assets/mlx.metallib` 同一份哈希），模型目录用装好的包里的
`Models/Qwen3-ASR-0.6B-4bit`。`engine_new` 与两次定稿跑的是 `/tmp/voice-abi/bench7-step`
（源 `bench3.c`，链本次重编的归档）；暖定稿另用 `bench2-step`（源 `bench2.c`，3 次 × 2 发）；
按键那一档用 `bench6-step`（源 `bench6.c`，`press` 模式）。

### 2.2 逐次实测

```text
engine_new（六次，code/assets 与包内资产哈希相同）
0.270 / 0.134 / 0.134 / 0.128 / 0.136 / 0.129 → 中位 0.134 秒
（装包之后再跑三次：0.147 / 0.133 / 0.131）

暖定稿（bench2-step，三个进程 × 每个进程两发；取每个进程内第二、三发的定稿）
0.179 / 0.176 ｜ 0.183 / 0.212 ｜ 0.178 / 0.178
  每个进程取中位：0.178 / 0.198 / 0.178（第二个进程那发的 0.212 是唯一的离群）
  三个进程取中位：0.178 秒
（bench7-step 的同进程第二发，六次：0.201 / 0.204 / 0.174 / 0.182 / 0.177 / 0.173 → 中位 0.180）

冷定稿（bench7-step 的第一次 finish）
1.018 / 1.014 / 0.825 / 0.821 / 0.815 / 0.801 → 中位 0.823 秒
（装包之后：0.837 / 0.828 / 0.846）

press 模式（2 秒说话，bench6-step，装好的包）
0.260 / 0.198 秒（松手→出字）｜ 2.291 / 2.229 秒（按下→出字）｜ 文本「嗯。」
```

三句要写在前面的读法：

1. **`engine_new` 没有回退。** 0.134 与 067 的 0.137、066 的 0.136 同一档，唯一偏高的
   0.270 是这一发踩到冷启动（页缓存与 MLX 都凉），067 记过同样两发（0.254 / 0.286）。
2. **暖定稿变短不能全算给 068 / 069。** 定稿走的是整段重算那条路，068 与 069 一个算子
   没动它；069 自己量的同一份代码是 0.170–0.179 秒，067 是 0.193–0.200 秒，这一档
   0.17–0.21 秒里有 0.03 秒左右的机器抖动（069 复测已经出现「同一份归档两次量到 0.170
   与 0.213」）。这里只报实测，不把差额记成收益。
3. **冷定稿 1.024 → 0.823 秒同理。** 它的分布本来就宽（本次 0.801–1.018，067 是 0.847–1.098
   中位 1.024），两个分布重叠，不当作改动带来的差额。冷定稿 = 第二段装载
   （分词器 + 文本塔，约 0.60–0.66 秒）+ 一次整段解码。

## 3. 14 秒探针：每步墙钟

量具是 `voice-mlx/tests/streaming.rs` 的
`partials_every_two_seconds_and_the_final_still_matches`（`--ignored --nocapture`，
探针重复 7 次 = 14 秒，每 16 000 个样本一块送，每满 2 秒回一条部分结果）。跑四遍取中位。

### 3.1 四遍原始数

```text
第 1 遍  0.179 / 0.369 / 0.570 / 0.785 / 0.963 / 1.151   （合计 1.151）
第 2 遍  0.174 / 0.373 / 0.574 / 0.825 / 1.008 / 1.192   （合计 1.192）
第 3 遍  0.169 / 0.347 / 0.538 / 0.741 / 0.919 / 1.108   （合计 1.108）
第 4 遍  0.169 / 0.357 / 0.552 / 0.752 / 0.926 / 1.106   （合计 1.106）
中位     0.172 / 0.363 / 0.561 / 0.769 / 0.945 / 1.130   （合计 1.130）
```

第 2 遍整条偏高（8 秒那一步 0.825、预填充 0.093，其余三遍是 0.741–0.785 / 0.052–0.058），
是这一发机器偏热，不是某一步退化。四遍的每一步都单调不降，没有回退。

### 3.2 与改前并列

| 步 | 音频 | 改前（068 量） | 068 之后 | 069 之后 | 本次（中位） |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 2.00 秒 | 0.194 | 0.179 | 0.181 | **0.172** |
| 2 | 4.00 秒 | 0.447 | 0.366 | 0.368 | **0.363** |
| 3 | 6.00 秒 | 0.772 | 0.566 | 0.572 | **0.561** |
| 4 | 8.00 秒 | 1.180 | 0.781 | 0.785 | **0.769** |
| 5 | 10.00 秒 | 1.600 | 0.986 | 0.966 | **0.945** |
| 6 | 12.00 秒 | 2.103 | 1.205 | 1.158 | **1.130** |
| 合计 | — | 2.103 | 1.205 | 1.158 | **1.130** |

「改前」是 068 记的今日同机复测（同口径、同探针；任务原始记录是
0.189 / 0.438 / 0.745 / 1.112 / 1.521 / 1.994，量法相同但机器状态不同）。
2.103 → 1.130 秒是 **少 46%**；0.172 与 0.185 之间是这一步现在全部的涨幅。

### 3.3 逐步增量与分项

| 步 | 音频 | 逐步增量（本次） | 改前的逐步增量 | 步内 | 梅尔 | 整形 | 编码 | 解码（含预填充） | 其中预填充 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 2.00 秒 | 0.172 | 0.194 | 0.169 | 0.035 | 0.000 | 0.030 | 0.104 | 0.037 |
| 2 | 4.00 秒 | 0.191 | 0.253 | 0.187 | 0.035 | 0.001 | 0.035 | 0.116 | 0.044 |
| 3 | 6.00 秒 | 0.198 | 0.325 | 0.194 | 0.036 | 0.001 | 0.042 | 0.115 | 0.047 |
| 4 | 8.00 秒 | 0.208 | 0.408 | 0.199 | 0.034 | 0.002 | 0.045 | 0.118 | 0.052 |
| 5 | 10.00 秒 | 0.176 | 0.420 | 0.172 | 0.036 | 0.000 | 0.030 | 0.105 | 0.039 |
| 6 | 12.00 秒 | 0.185 | 0.503 | 0.178 | 0.036 | 0.001 | 0.034 | 0.107 | 0.041 |

（分项取自第 4 遍——它的六步墙钟与四次中位最接近；各栏四舍五入到毫秒，相加等于步内
（第 5 步那一行四舍五入后差 1 毫秒）。第 3 遍的同项与它最多差 9 毫秒（第 2 步的解码 0.107
对 0.116），其余差在 0–6 毫秒。）

两处按住了：

- **梅尔**：0.035 → 0.036 秒，六步平。改前是 0.050 → 0.300 秒。测试自己打印的
  「第一步 0.0346 秒 / 第六步 0.0348 秒，比值 1.01」与第 3 遍的 0.98、第 4 遍的 1.03
  同一档。
- **预填充**：0.037 → 0.041 秒（第 4 步新冻窗那一发是 0.052）。改前 0.042 → 0.073 秒，
  且随提示长度一直涨。

还剩两处涨：编码 0.030 → 0.045 秒（随活动窗宽 200 → 800 帧，与已收音频无关，
第五步回到 200 帧就掉回 0.030），解码 0.104 → 0.118 秒（其中预填充只占 0.037–0.052，
剩下 0.065–0.075 是两次贪心前向，约 42 毫秒/token，见 067）。

### 3.4 30 秒探针（同一次运行）

`thirty_second_probe_keeps_the_prefill_flat`（探针重复 15 次，14 个部分步）：

| 步 | 音频 | 提示位置 | 重算 | 沿用 | 预填充 | 同长度整段一次预填充 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 2 秒 | 44 | 44 | 0 | 0.048 | 0.038 |
| 4 | 8 秒 | 122 | 113 | 9 | 0.104 | 0.053 |
| 5 | 10 秒 | 148 | 35 | 113 | 0.050 | 0.061 |
| 8 | 16 秒 | 226 | 113 | 113 | 0.058 | 0.086 |
| 9 | 18 秒 | 252 | 35 | 217 | 0.047 | 0.084 |
| 12 | 24 秒 | 330 | 113 | 217 | 0.060 | 0.107 |
| 13 | 26 秒 | 356 | 35 | 321 | 0.059 | 0.117 |
| 14 | 28 秒 | 382 | 61 | 321 | 0.045 | 0.116 |

（「整段」那一列来自同一次运行的 `full_prefill_cost_by_prompt_length`：44 → 0.038、
174 → 0.067、382 → 0.116 秒，三次中位。）重算的位置数在 35 / 61 / 87 / 113 上循环，
提示从 44 涨到 382 个位置，预填充一直在 0.042–0.060（第 4 步那一发 0.104 是新冻窗
整窗重算 + 这一发机器偏热，见 §3.1）。三十秒处一步省 0.07 秒左右。

## 4. 数值：与基线一致，差多少

### 4.1 增量梅尔

```
voice-mel  matches_reference_mel_incrementally
  增量梅尔：实测差 6.866455e-5（下标 136），容差 1e-3   → 余量 14.56 倍

voice-mel  provisional_frame_tracks_the_right_reflection
  201..=8000 每个样本长度：增量结果 == 整段重算，逐位相同（差 0）
```

与 062 / 067 / 068 记的是同一个数（6.866455e-5）。容差仍是 `voice_parity::MEL_TOLERANCE = 1e-3`，
没有放宽。log 整形那个全局最大值（参考的 max−8 截断）在流里按「当前前缀的 max」套用，
上面那条逐长度的断言就是它的证据。

### 4.2 复用的键值与整段预填充

```
voice-mlx  reused_key_values_match_one_pass_prefill
  整段预填充：argmax 106287（2.759375e1）
  复用 + 重算：argmax 106287（2.759375e1）
  前 64 维 logits 最大绝对差 7.8125e-2
  上界 2.0328125e1，容差 7.940674e-2（8 个单位），折 7.8708687 个单位，余量 1.02 倍
```

与 069 逐位相同。**余量 1.02 倍照旧写在这里**：这是把「复用」与「整段一次预填充」放在
同一条提示上比的内部闸，不是与参考比的那道闸；贴着 argmax 的两个 token 仍有可能被换掉。

### 4.3 token 与文本

| 项 | 基线 | 本次 |
| --- | --- | --- |
| 探针定稿（`end_to_end_parity`） | `[106287, 1773, 151645]` / 「嗯。」 | 同左 |
| 14 秒六步的部分 token | `[106287, 1773, 151645]`（每步） | 同左，且每步与「这一段音频整段重算」逐位相同（测试逐条断言并打印） |
| 14 秒定稿 | `[106287, 1773, 151645]` / 「嗯。」 | 同左 |
| 30 秒十四步 | `[106287, 1773, 151645]` | 同左 |
| 已固定文本那一路（`unfixed = 1`） | 「嗯。」 | 同左（提示里带 1 个固定 token，重算 62 / 88 / 114 / 36 / 62 / 88） |
| 装好的包实跑（`bench7-step`，九次） | 「嗯。」 | 同左 |
| 装好的包 press 模式（`bench6-step`，两次） | 「嗯。」 | 同左 |
| 切窗重编 vs 整段编码（1200 帧） | — | 投影最大绝对差 1.0691956e-4（闸 1e-3） |

## 5. 重编归档、重编 Swift 产物、重打包、重装

### 5.1 归档

删掉旧产物再编（`SDKROOT`、`LIBRARY_PATH`、`MLX_SYS_SYSTEM_SDK=1`、
`--target aarch64-apple-darwin`，`cargo build -p voice-ffi --features metal`）：

```
rm -f target/aarch64-apple-darwin/debug/libvoice_ffi.a
→ Compiling voice-ffi v0.1.0 … Finished `dev` profile in 1.30s
→ target/aarch64-apple-darwin/debug/libvoice_ffi.a  285 750 480 字节  15 Nov 07:54
```

（只有 `voice-ffi` 重编，MLX 与 `voice-mlx` 的中间产物在缓存里；这不是全量重编，
只是把旧归档删掉后重新产出，编出来的算术仍来自当前源码。）

### 5.2 Swift 产物

```
rm -rf code/Client/.build
cd code/Client && swift build --product Squirrel   → Build complete! (10.21 秒)
→ .build/out/Products/Debug/Squirrel  40 697 680 字节  15 Nov 07:54
```

链接只有 `ld: warning: object file … was built for newer 'macOS' version (27.2.1)` 这一种
（MLX 目标文件按 27.2 编、部署目标 14.0），与 066 / 067 记的同一条，不影响产物。

二进制里的符号（`nm`）：

```
backend10transcribe10transcribe
backend10transcribe11PromptParts
backend10transcribe12build_prompt
backend10transcribe12prompt_parts
backend10transcribe13prompt_tokens
backend10transcribe13splice_prompt
backend10transcribe14load_tokenizer
backend10transcribe15transcribe_with
backend10transcribe19empty_transcription
backend10transcribe22transcribe_with_opened
…（65 条名字里带 backend10transcribe）
```

`nm -gU` 的 `_voice_*` 八条（`voice_engine_new` / `voice_engine_free` /
`voice_engine_set_callback` / `voice_engine_set_reply_sink` / `voice_engine_last_error` /
`voice_session_begin` / `voice_session_feed` / `voice_session_finish`）都在，`-u` 拉住的是同一组。

### 5.3 打包与安装

```
bash code/Client/scripts/make-app.sh     → app: code/Client/dist/Squirrel.app
bash code/scripts/install-app.sh         → 装好了：~/Library/Input Methods/RimeClient.app

停进程：…/RimeClient.app/Contents/MacOS/Squirrel
校验签名：codesign --verify --deep --strict      → 通过（exit 0）
在  Contents/Info.plist                                             2 791 字节
在  Contents/Resources/mlx.metallib                               183 870 464 字节
在  Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/weights.safetensors  537 626 967 字节
在  Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/vocab.json         2 776 833 字节
资产哈希与包内清单一致（voice-assets.sha256）
```

装完之后单独复核一遍：

```
codesign --verify --deep --strict   → exit 0
codesign -dv                          → Signature=adhoc，Identifier=im.dragonshock.inputmethod.RimeClient
包内可执行文件 40 697 984 字节，15 Nov 07:57，nm 里带 backend10transcribe
用包里的资产实跑 bench7-step 三次：engine_new 0.147 / 0.133 / 0.131、暖定稿 0.182 / 0.177 / 0.184，文本全「嗯。」
```

装包过程中输入法进程被停过一次（`install-app.sh` 的固定第一步），没有做按键测量，
也没有调用 `inject`。

## 6. 还剩下的瓶颈

按「一步 0.178 秒（12 秒处）里各占多少」排：

1. **两次贪心前向 0.065–0.075 秒（37–42%）。** 42 毫秒/token，2 个 token；`lm_head` 是对
   151 936 行按 5 块 × 32768 行做 argmax。这一步的大头已经从预填充换成这里，预填充再怎么
   压，上限也只有 0.041 秒。
2. **编码 0.030–0.053 秒/步（17–30%）。** 随活动窗宽走（200 / 400 / 600 / 800 帧），
   与已收音频无关。要再压得动编码器本身或换窗宽策略。
3. **梅尔 0.035 秒/步（20%）。** 已经与已收音频无关，但 200 帧 0.035 秒仍是这一步第二大块；
   它与「编码 + 解码」串在引擎线程上，没有重叠。
4. **新冻窗那一步整窗重算（第 4 步 113 个位置 vs 其它步 35–61）。** 缓存里那 104 行是按
   活动窗编出来的，窗一满就得按整窗重编。要省这一步得让编码器对首端因果，不在缓存这一层。
5. **固定尾巴 9 个位置与已固定文本每步重算。** 它们排在活动窗之后，随活动窗变；
   已固定文本越长这一步越贵。
6. **缓存截断是 gather，每位置 112 KB。** 12 秒约 14 MB、30 秒约 36 MB 的搬运，量到约 4 毫秒，
   算在预填充里。要彻底去掉得让序列轴支持稀疏拼接。
7. **复用与整段的 logits 差 7.87 个单位、余量 1.02 倍。** 本探针六步、30 秒十四步、
   `unfixed = 1` 七步全对；真实长句里第一次出现「复用前后差一个 token」时，先看那一步
   logits 前两名差多少，再决定要不要把复用边界往前收。
8. **定稿仍整段重算：暖定稿 0.178 秒。** 松手到上屏这一段不借最后一步的部分结果，
   梅尔、编码、整条提示预填充都要重来一遍（2 秒探针 = 44 个位置）。
9. **冷定稿 0.82 秒 = 第二段装载 0.60–0.66 + 一次整段解码。** 第二段里分词器 0.393 秒占
   2/3，是「进程刚起、按下就松手」那一档按下→出字的主项。
10. **每步多一次分词**（核「拼接 == 整段分词」，0.1 毫秒这一档），核不过会退回整段预填充。
11. **所有数字都是 `dev` 档案。** `release` 的读盘、反量化、梅尔与解码都还没有量，
    绝对数是上限，不是发布值。

## 7. 复现

```text
cd /Users/dragon/repo/tools/rime-client/code/voice-engine
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"; export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
export RUSTFLAGS="-C link-arg=-framework -C link-arg=Accelerate \
  -C link-arg=-Wl,-U,_MTLTensorDomain -C link-arg=-Wl,-U,_MTLLogStateErrorDomain \
  -C link-arg=-Wl,-U,_NSDeviceCertificationiPhonePerformanceGaming \
  -C link-arg=-Wl,-U,_NSProcessInfoPerformanceProfileDidChangeNotification \
  -C link-arg=-Wl,-U,_NSProcessPerformanceProfileDefault \
  -C link-arg=-Wl,-U,_NSProcessPerformanceProfileSustained \
  -C link-arg=-Wl,-U,___fmaxf16 -C link-arg=-Wl,-U,___fminf16"

cargo test --workspace --offline                      # 61 通过、4 忽略
cargo test -p voice-mel --offline --lib -- --nocapture matches_reference_mel_incrementally
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test encoder_parity  -- --test-threads=1
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test decoder_parity  -- --test-threads=1 --nocapture
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test end_to_end_parity -- --test-threads=1 --nocapture
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test streaming -- --ignored --nocapture --test-threads=1   # 六步 / 30 秒 / 复用差 / 整段预填充
```

重编与装包：

```text
rm -f code/voice-engine/target/aarch64-apple-darwin/debug/libvoice_ffi.a
cargo build -p voice-ffi --features metal --target aarch64-apple-darwin
rm -rf ../Client/.build && ( cd ../Client && swift build --product Squirrel )
bash ../Client/scripts/make-app.sh
bash ../scripts/install-app.sh
codesign --verify --deep --strict "$HOME/Library/Input Methods/RimeClient.app"
```

量具（`/tmp/voice-abi/link.sh <源.c> <输出>` 用的是同一份归档与 067 记的八个 `-U` 符号）：

```text
bash /tmp/voice-abi/link.sh /tmp/voice-abi/bench3.c /tmp/voice-abi/bench7-step   # engine_new / 冷 / 暖
bash /tmp/voice-abi/link.sh /tmp/voice-abi/bench2.c /tmp/voice-abi/bench2-step   # 暖定稿
bash /tmp/voice-abi/link.sh /tmp/voice-abi/bench6.c /tmp/voice-abi/bench6-step   # press 模式
RES="$HOME/Library/Input Methods/RimeClient.app/Contents/Resources"
/tmp/voice-abi/bench7-step "$RES/Models/Qwen3-ASR-0.6B-4bit" "$RES/mlx.metallib" \
  /Users/dragon/repo/tools/rime-client/code/voice-oracle/assets/probe_pcm.bin
```

## 8. 改了哪些文件

| 位置 | 改动 |
| --- | --- |
| `dev-docs/notes/070-research-step-scaling-result.md` | 本篇 |
| `dev-docs/MAP.md` | 「进行中」里梅尔与跨步复用两条按已完成收掉；补上「新冻窗整窗重算」「复用余量 1.02 倍」「定稿仍整段重算」「三段串在引擎线程上」四条；贪心那条换成本次的数；识别时延那条换成本次的数；notes 索引追加 070 |

`code/` 一个字没动：`voice-mel`、`voice-mlx`、`voice-ffi`、`code/Client`、
`code/VoiceInterface`、`code/voice-oracle` 都是 068 / 069 留下的形态；`dev-docs/refs`
没动，librime 没动，`inject` 没调。

<!-- research-complete -->
