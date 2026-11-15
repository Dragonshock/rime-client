# 068-research-incremental-mel：梅尔只算新样本，14 秒探针的六步从 0.189…1.994 秒降到 0.179…1.205 秒

测量日 2026-11-15。对象是 `code/voice-engine/voice-mel`（新增增量接口）与
`code/voice-engine/voice-mlx`（`backend/stream.rs` 改成每步只对新样本求梅尔）。
口径是 `dev` 档案（`BUILD.txt` 里装包那一档），模型目录与 `mlx.metallib` 取自已安装包
`~/Library/Input Methods/RimeClient.app/Contents/Resources`，探针是
`code/voice-oracle/assets/probe_pcm.bin`（2.00 秒、32000 样本、200 帧，重复 7 次 = 14 秒）。
定稿那条路（`MelFrontend::log_mel*` → 编码器 → 预填充 → 贪心）一个算子没动，
`dev-docs/refs`、`inject`、librime 都没动，容差一个数没改。

## 0. 结论速览

| 项 | 改前 | 改后 |
| --- | ---: | ---: |
| 14 秒探针六步墙钟（累计，第 1/6 步） | 0.189 / 1.994 秒 | **0.179 / 1.205 秒** |
| 14 秒探针六步墙钟（逐步增量） | 0.194 / 0.253 / 0.325 / 0.408 / 0.420 / 0.503 | **0.179 / 0.187 / 0.200 / 0.215 / 0.205 / 0.219** |
| 单步梅尔（2 秒 → 12 秒） | 0.050 → 约 0.30 秒（线性） | **0.034 → 0.035 秒（平）** |
| 单步整形（只算要编码的那几列） | 含在上面那 0.050 秒里 | **0.000–0.002 秒** |
| 增量与整段重算的梅尔差 | — | **0**（逐位相同） |
| 梅尔与参考的最大绝对差 | 6.866455e-5 | **6.866455e-5**（容差 1e-3，余量 14.56 倍） |
| token / 文本 | `[106287, 1773, 151645]` / 「嗯。」 | 同左 |
| 暖定稿（2 秒探针，`bench2`） | 0.193–0.207 秒 | 0.168–0.213 秒 |
| `engine_new` | 0.137 秒 | 0.125–0.269 秒 |
| `cargo test --workspace --offline` | 54 通过、4 忽略 | **61 通过**、4 忽略、0 失败 |

一句话：**梅尔从「每步整段重算」变成「每步只算新样本 + 只整形要交出去的那几列」**，
单步梅尔开销与已收音频长度无关；14 秒探针六步合计 2.103 → 1.205 秒（少 43%）。
步墙钟仍随长度略涨，涨的那部分不在梅尔上，见第 6 节。

## 1. 增量接口

### 1.1 `voice-mel::MelStream`

```text
MelStream::new()
  push(&frontend, samples: &[f32]) -> usize   // 新样本，返回这一步新定下的帧数
  push_i16(&frontend, pcm: &[i16]) -> usize   // 同一件事的 i16 入口（/32768 与 log_mel 同式）
  samples() -> usize                          // 累计样本数
  frames() -> usize                           // 可输出帧数，与 frame_count 同口径
  settled_frames() -> usize                   // 已定帧数（值不再变）
  samples_to_next_frame() -> usize            // 还差多少样本才多出一个已定帧
  prefix_max() -> f32 / log_floor() -> f32    // 整形用的 max 与 max−8
  raw_frame(i) -> &[f32] / raw_block()        // 整形前的原始值（帧优先 / bin 优先）
  shaped_columns(start, end) -> Vec<f32>      // 取几列整形后的值，bin 优先 (128, end-start)
  shaped_block() -> Vec<f32>                  // 整段，形状与 log_mel_f32 相同
  frames_seconds() -> f64                     // 新帧的 DFT 加滤波器组墙钟累计
```

内部三块状态：

| 字段 | 内容 |
| --- | --- |
| `tail` / `tail_base` | 下一帧窗起点起的样本，长度 ≤ 400 + 159；首端反射落在 `-200..-1` |
| `raw` | 已定帧的整形前原始值，帧优先，每帧 128 个 f32；末尾最多多一帧未定帧 |
| `settled_max` / `prefix_max` | 已定帧的最大原始值（单调不减）与含未定帧的当前前缀最大值 |

「上次剩下的不足一个 hop 的尾巴」在实现里就是 `tail` 里还没进任何已定帧的那一段：
每推进一次之后 `samples() - ((settled-1)·160 + 200) < 160`，`samples_to_next_frame()`
是它的补数（落在 0..160，第一帧之前是「等首端反射凑齐 201 个样本」）。
测试 `leftover_tail_is_shorter_than_one_hop` 逐块核这条不等式，并核「再收 `wait` 个
就多一帧、少收一个就不多」。

`MelFrontend::log_mel` / `log_mel_f32` 原样保留：定稿走那条，它仍是整段一次算完的
参考形状实现，也是增量那条路的对照物。

### 1.2 `stream.rs` 的改法

```text
改前：let mel = opened.mel.log_mel(&stream.pcm);          // 每步整段重算
      ...
      let patch = mel_columns(&mel, mel_frames, start, end);

改后：stream.mel.push_i16(opened.mel, &fresh);            // 只算这一步的新样本
      let mel_frames = stream.mel.frames();
      ...
      let patch = stream.mel.shaped_columns(start, end);  // 只整形这几列
```

`Stream` 不再留整段 `pcm`，只留 `pending`（还没进梅尔的新样本）与 `MelStream`。
`mel_columns` 这个切列函数随之删掉：切片与整形现在是同一个动作（`shaped_columns`），
少一次整段 bin 优先数组的搬运。`StreamStep` 加了 `mel_shape_seconds`，
`mel_seconds` 的含义收窄成「这一步新样本的 DFT 加滤波器组」，
`VOICE_STREAM_LOG=1` 那一行把它打出来，两者分开才能看清哪一块随长度长。

## 2. 等价性论证：为什么「缓存整形前的帧 + 按当前 max 整形」与逐帧重算相同

### 2.1 一帧只取决于它自己的 400 个样本

参考的帧网格是 `i·160`，窗宽 400，窗 `i` 覆盖 `padded[i·160 .. i·160+399]`，
而 `padded[p]` 对应样本 `p-200`。因此帧 `i` 只用到样本
`i·160-200 ..= i·160+199`：

- **已定帧**的条件是这 400 个样本全部已收到（`i·160+200 ≤ 样本数`）。之后新样本
  不会进这个窗，值从此不变——这是能不能缓存的分界线。
- 窗跨到负下标只有 `i = 0`（起点 −200）与 `i = 1`（起点 −40），它们用到的是首端反射
  `samples[200], samples[199], …, samples[1]`，只取决于前 201 个样本，也是固定的。
  参考的 `reflect_pad` 里左边那段就是这个顺序（端点不重复），代码里的
  `pad_head_once` 一次摆好，之后按绝对下标直接取，等价于「先补边再取窗」。

### 2.2 只有末尾那一帧会变

帧数 `kept = 样本数 / 160`，其中跨过接收末端的那一帧（窗的最后一个样本还没收到）
用的是右端反射 `samples[len-2], samples[len-3], …`，它的值与 `len` 一起变。
设 `len = 160q + r`（`0 ≤ r < 160`）：已定帧数是 `q-1`（`r < 40`）或 `q`（`r ≥ 40`），
而可输出帧数是 `q`，所以「未定帧」最多一帧，就是最后那一帧。`push` 每步把这一帧的
128 个值按当前 `len` 重算一遍（`fill_window` 的 `mirror = 2(len-1) - index` 分支），
代价一帧，与前缀长度无关。

两条测试分别管两件事：

- `provisional_frame_tracks_the_right_reflection` 在 201..8000 的**每一个长度**上比一次
  整段（增量给的 `shaped_block` 对 `log_mel_f32`），逐位相同。这一条覆盖首端反射的时机
  与每一处右端反射的重算。
- `incremental_matches_whole_utterance_bitwise` 按 1000 个样本一块送、每送一块比一次
  `to_bits`；`chunking_does_not_change_the_frames` 再用 1 / 159 / 160 / 161 / 400 / 401 /
  2000 个样本一块送一遍，结果与一次送完逐位相同。这一条覆盖分块边界（含「一块里连出
  多帧」与「一块里连一帧都不够」两种情形）。

### 2.3 全局最大值是唯一的非因果依赖，而它只作用在整形这一步

参考的整形是三步：

```text
logged = log10(max(x, 1e-10))            // 逐元素
floor  = max(logged) - 8                 // 全段一个标量
out    = (max(logged, floor) + 4) / 4    // 逐元素
```

中间那个 `max(logged)` 要看整段，是这一层唯一的「非因果」依赖：新来的音频可能抬高它，
已经算完的帧的整形结果随之改变。它不能像原始帧那样缓存成定值，但它的作用方式很窄：

- 它是一个**标量**，逐元素比较的阈值；
- 映射 `logged → (max(logged, floor) + 4)/4` 逐元素。

所以只要在**取值时**用「当前前缀的 max」套一次，就与「整段重算再整形」是同一串运算。
实现上这个 max 是增量维护的：已定帧的最大值单调不减（`settled_max`），
未定那一帧的最大值每次重算（128 个值），两者取大就是当前前缀的 max。
`shaped_columns` 每次调用现算 `floor = prefix_max.max(1e-10).log10() - 8`，
再对要交出去的那几列做 `log10` 与截断——这就是为什么整形只花 0.000–0.002 秒：
它只碰被编码的那 ≤ 800 列，不碰历史。

注意「先取 max 再 `log10`」与参考「先 `log10` 再取 max」是同一个数：
`log10` 单调、`max(x, 1e-10)` 单调，f32 比较与一次 `log10f` 都不会把次序翻过来。
逐位相同的断言已经把这条也包括进去了。

### 2.4 三维「不重算」各自精确，合起来还是同一个数

| 会变的东西 | 处理 | 精确性 |
| --- | --- | --- |
| 已定帧的 400 个样本 | 缓存原始帧，不再算 | 样本已到齐且不变，重算只可能是同一个结果 |
| 末尾一帧的右端反射 | 每步重算这一帧 | 与 `reflect_pad` 同一条算式 |
| 全段最大 `log10` | 增量维护（已定 + 未定） | max 是精确比较；`log10` 是同一个函数 |
| 整形 | 取值时按当前 max 套用 | 元素级同一个 f32 运算串 |

`matches_reference_mel_incrementally` 用增量那条路对参考资产算了一遍，打印
`6.866455e-5`，与整段那条路的实测差同一个数（第 4 节）。
12 秒处增量与整段重算的最大绝对差是 **0**（`/tmp/voice-abi/melstream` 也打了一遍）。

## 3. 六步墙钟：14 秒探针

同一台机器、同一份归档编法、`voice-mlx/tests/streaming.rs` 的
`partials_every_two_seconds_and_the_final_still_matches`（`--ignored`，1 秒一块送，
每满 2 秒跑一步，六步）。墙钟是测试里那个 `at`：从开始送样本算起的累计值。

```
改前（今天的归档，同一台机器）
14 秒部分 2.00 秒处（墙钟 0.194 s）… 编码 0.032 s
14 秒部分 4.00 秒处（墙钟 0.447 s）… 编码 0.036 s
14 秒部分 6.00 秒处（墙钟 0.772 s）… 编码 0.050 s
14 秒部分 8.00 秒处（墙钟 1.180 s）… 编码 0.069 s
14 秒部分 10.00 秒处（墙钟 1.600 s）… 编码 0.033 s
14 秒部分 12.00 秒处（墙钟 2.103 s）… 编码 0.044 s
```

改后三次（每次六步）：

```
0.179 / 0.365 / 0.573 / 0.791 / 1.004 / 1.223
0.180 / 0.367 / 0.566 / 0.781 / 0.986 / 1.205
0.176 / 0.364 / 0.564 / 0.773 / 0.972 / 1.191
中位 0.179 / 0.366 / 0.566 / 0.781 / 0.986 / 1.205
```

逐步增量（相邻两步之差，也就是每一步自己花的时间）：

| 步 | 音频 | 改前 | 改后（中位） |
| --- | ---: | ---: | ---: |
| 1 | 2.00 秒 | 0.194 | **0.179** |
| 2 | 4.00 秒 | 0.253 | **0.187** |
| 3 | 6.00 秒 | 0.325 | **0.200** |
| 4 | 8.00 秒 | 0.408 | **0.215** |
| 5 | 10.00 秒 | 0.420 | **0.205** |
| 6 | 12.00 秒 | 0.503 | **0.219** |
| 合计 | — | 2.103 | **1.205** |

任务里给的那组基线是 0.189 / 0.438 / 0.745 / 1.112 / 1.521 / 1.994，与今天这台的
0.194 / 0.447 / 0.772 / 1.180 / 1.600 / 2.103 同一条线（差在噪声与机器状态）。
改后同一位置的数是 0.179 / 0.366 / 0.566 / 0.781 / 0.986 / 1.205。

### 3.1 梅尔那一段已经不随长度长

改后一步的分项（第一次那轮，`步内 = 梅尔 + 整形 + 编码 + 解码`）：

| 步 | 音频 | 步内 | 梅尔 | 整形 | 编码 | 解码 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 2.00 秒 | 0.178 | **0.035** | 0.000 | 0.033 | 0.111 |
| 2 | 4.00 秒 | 0.186 | **0.035** | 0.001 | 0.036 | 0.114 |
| 3 | 6.00 秒 | 0.207 | **0.034** | 0.001 | 0.043 | 0.128 |
| 4 | 8.00 秒 | 0.217 | **0.035** | 0.002 | 0.049 | 0.131 |
| 5 | 10.00 秒 | 0.212 | **0.035** | 0.000 | 0.033 | 0.144 |
| 6 | 12.00 秒 | 0.216 | **0.035** | 0.001 | 0.035 | 0.145 |

六步的梅尔是 0.0343–0.0350 秒，第六步与第一步的比值 0.97–1.01（三次运行）。
改前这一步是 `0.049 → 0.103 → 0.157 → 0.201`（063 第 5 节按 2/4/6/8 秒实测，
每 2 秒音频加 0.05 秒，10 秒与 12 秒落在同一条线上约 0.25 / 0.30 秒）。

单独的台架 `/tmp/voice-abi/melstream`（同一探针，六步各推 32000 个样本，
每步三次取中位，`opt-level = 0`，不碰 Metal）把两条路放在同一个进程里比：

```
步 | 音频   | 帧数 | 老路整段梅尔 | 增量新样本 | 增量整形 | 增量合计
1  |  2.00s |  200 | 0.0338 s     | 0.0337 s   | 0.0005 s | 0.0342 s（101%）
2  |  4.00s |  400 | 0.0677 s     | 0.0340 s   | 0.0009 s | 0.0350 s（52%）
3  |  6.00s |  600 | 0.1018 s     | 0.0342 s   | 0.0014 s | 0.0356 s（35%）
4  |  8.00s |  800 | 0.1364 s     | 0.0343 s   | 0.0019 s | 0.0363 s（27%）
5  | 10.00s | 1000 | 0.1691 s     | 0.0340 s   | 0.0005 s | 0.0345 s（20%）
6  | 12.00s | 1200 | 0.2038 s     | 0.0347 s   | 0.0009 s | 0.0357 s（17%）
六步合计：老路 0.7126 s，增量 0.2112 s（30%）
```

第一行是 101%：2 秒处两条路算的是同一批 200 帧，增量只省掉「整形整段」与一次
`pcm` 的整段 `Vec` 拷贝，本来就没多少可省——这也是这件事的判据：省的应该正比于历史，
不是一次性省下一笔。台架里整形只有 0.0005–0.0019 秒，随窗宽（≤ 800 帧）变，与长度无关；
第 4 步 0.0019 秒是那一步要整 800 帧（冻结第一个窗），第 5、6 步只要整 200 / 400 帧。

## 4. 与参考的差，以及 token / 文本

同一条闸、同一份资产，只换前端取值方式：

```
整段那条路（voice-parity，2 秒探针 128×200）：
  梅尔：实测差 6.866455e-5，容差 1e-3，余量 14.56 倍
增量那条路（voice-mel，0.5 秒参考资产 128×50）：
  增量梅尔：实测差 6.866455e-5（下标 136），容差 1e-3
增量与整段重算：
  12 秒处最大绝对差 0（201..8000 每个长度都逐位相同）
```

`6.866455e-5` 与 062/067 记的是同一个数，不是「换了一条路之后恰好也过闸」：
参考的整形用到的那个标量在增量这边是同一个数（第 2.3 节），原始帧是同一串 f32 运算，
所以两条路给的是同一批值。容差 `MEL_TOLERANCE = 1e-3` 一个数没改。

token 与文本：

| 项 | 基线 | 现在 |
| --- | --- | --- |
| 探针定稿 token | `[106287, 1773, 151645]` | 同左（`end_to_end_parity` 逐位断言） |
| 探针定稿文本 | 「嗯。」 | 同左 |
| 14 秒音频定稿 | `[106287, 1773, 151645]` / 「嗯。」 | 同左（`streaming.rs` 断言与整段重算一致） |
| 六步的部分结果 | 「嗯。」 | 同左（六步各 3 个新 token） |
| 8 秒以上前缀复用 | 104 个 token、本步冻结 0 | 同左（第 4、5、6 步的前缀 104） |

解码器的三套闸（`encoder_parity` 3、`decoder_parity` 6、`end_to_end_parity` 4）全过，
差值与 067 逐位相同：卷积茎 0、第 0 层 3.90625e-3、末尾投影 1.1855885e-3、
主闸 7.8125e-2、注入参考特征 6.25e-2，argmax 全 106287。

### 4.1 端口那边的一步（4 秒语音，`bench6` 的 press 模式，按 4096 个样本一块实时送）

`VOICE_STREAM_LOG=1` 打出来的那一步（产品路径：worker 线程、Metal 同时在收样本，
所以编码与解码比台架里的数大）：

```
stream partial mel=0.037s(200帧,新样本) 整形=0.000s encode=0.222s(活动窗200帧,冻结0个token)
  decode=0.315s(提示44个,固定0个,新7个) 音频token=26 前缀token=0 文本="今天天气不错，我们。"
```

回调与文本与 063 记的一致：第 3.004 秒出 `partial`「今天天气不错，我们。」，
松手 0.589 秒后出 `final`「今天天气不错，我们下午去公园散步吧。」
（063 §5 里 4 秒那一档的 `partial` / `final` 逐字相同）。
`mel=0.037s` 比台架里的 0.034 略高，是同一段音频在 worker 线程上、Metal 正忙时的数；
两者都不含任何按历史长度累加的成分。

## 5. 复现

```text
cd /Users/dragon/repo/tools/rime-client/code/voice-engine
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"; export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
cargo test --workspace --offline                          # 61 通过、4 忽略
cargo test -p voice-parity --offline --test parity mel_matches_reference -- --nocapture
                                                          # 打印 6.866455e-5
cargo test -p voice-mel --offline -- --nocapture           # 打印增量那条路的 6.866455e-5

# 台架：六步梅尔（老路 vs 增量）
cd /tmp/voice-abi/melstream && cargo build --offline --target aarch64-apple-darwin \
  && ./target/aarch64-apple-darwin/debug/melstream

# 流式六步墙钟（要 metal，链接参数见 encoder_parity.rs 文件头）
export RUSTFLAGS="-C link-arg=-framework -C link-arg=Accelerate -C link-arg=-Wl,-U,_MTLTensorDomain ..."
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --test streaming -- --ignored --nocapture --test-threads=1

# 归档与 2 秒探针的定稿
cargo build -p voice-ffi --features metal --target aarch64-apple-darwin
cd /tmp/voice-abi && xcrun clang -O2 bench2.c \
  /Users/dragon/repo/tools/rime-client/code/voice-engine/target/aarch64-apple-darwin/debug/libvoice_ffi.a \
  -o bench2-mel8 -liconv -lc++ -lobjc -framework Foundation -framework Metal \
  -framework Accelerate -lSystem -lc -lm
RES="$HOME/Library/Input Methods/RimeClient.app/Contents/Resources"
./bench2-mel8 "$RES/Models/Qwen3-ASR-0.6B-4bit" "$RES/mlx.metallib" \
  /Users/dragon/repo/tools/rime-client/code/voice-oracle/assets/probe_pcm.bin
```

## 6. 没做到的部分

- **步墙钟仍随长度涨一点**（0.179 → 0.219 秒）。涨的不是梅尔：梅尔六步都是 0.035 秒，
  整形 0.000–0.002 秒；涨的是解码（0.111 → 0.145 秒，占步内 60–67%）与编码
  （0.033 → 0.049，随活动窗 200 → 800 帧变，属窗宽而不是历史）。解码那一段是
  「每步重新预填充 44–52 个提示 token」——音频占位个数每步变、RoPE 位置整体平移，
  键值跨步对不上，这次没有解法写进代码。
- **定稿仍整段重算**：`finish` 不借用最后一步的部分结果，暖定稿 0.168–0.213 秒与改前的
  0.193–0.207 秒同一档（定稿走的那条老路一个算子没改）。
- **增量状态占内存**：缓存每帧 128 个 f32，14 秒 1400 帧约 717 KB；换来的是 `Stream`
  不再留整段 `pcm`（14 秒 448 KB）。`voice-ffi` 的 `WeightDecoder` 仍为定稿留着整段
  `pcm`，那一份没动。
- **整形仍是每步一次全量的那几列**：800 帧一窗时 0.0019 秒。它不是 O(历史)，但也不是
  0；要再压就得让编码器直接吃相邻的原始帧、把整形挪进编码前的常量准备里。
- 所有数字都是 `dev` 档案，绝对数是上限，不是发布值。

## 7. 改了哪些文件

| 位置 | 改动 |
| --- | --- |
| `code/voice-engine/voice-mel/src/lib.rs` | 新增 `MelStream`（增量状态、`push` / `push_i16` / `shaped_columns` / `shaped_block` / `raw_block` / `samples_to_next_frame` / `log_floor`）、`RawFrame`、`project`（`log_mel_f32` 与增量共用的滤波器组）、`shape_one`、`raw_max`、`pad_head_once`、`fill_window`、`window_fits`；模块头写清增量切法与「全局最大值是非因果依赖、只作用在整形」；`log_mel_f32` 的算术未变（只把滤波器组那段抽成 `project`）；测试 5 条变 12 条 |
| `code/voice-engine/voice-mlx/src/backend/stream.rs` | `Stream` 换成 `pending` + `MelStream`，不再留整段 `pcm`；每步只推新样本；切列改走 `shaped_columns`（删掉 `mel_columns` 与其测试）；`StreamStep` 加 `mel_shape_seconds`；模块头第 1 条重写 |
| `code/voice-engine/voice-ffi/src/decoder.rs` | `VOICE_STREAM_LOG` 那一行加「新样本」「整形」两项；模块头一句话跟着改 |
| `code/voice-engine/voice-mlx/tests/streaming.rs` | 六步打印加梅尔与整形分项；新增三条断言：14 秒处增量梅尔与整段重算逐位相同（1399 已定 + 1 未定）、第六步梅尔 < 4 倍第一步 + 4 毫秒、已定帧数与帧数差一 |
| `code/voice-engine/BUILD.txt` | 追加「梅尔只算新样本」一节：接口、61 通过、台架六步表、归档与 `bench2` |
| `dev-docs/notes/068-research-incremental-mel.md` | 本篇 |

`code/Client`、`code/VoiceInterface`、`code/voice-oracle`、`dev-docs/refs` 都没有动；
没有按键测量，也没有调用 `inject`。

<!-- research-complete -->