# 074-research-multichunk-fix：多块路径的验收与六段真实中文语音

测量日：2026-11-15。参考实现仍是 `code/voice-worker/.venv` 里的 `mlx_qwen3_asr 0.4.4`（只作对照标准）。
被验收的是 `code/voice-engine` 里那条「PCM → 梅尔 → 音频塔 → 预填充 → 逐 token 贪心 → 分词器」的路，
驱动是 `/tmp/voice-abi/bench7`（这次用 `bench.c` 重编）。

## 1. 结论

1. **多块路径的缺陷在音频注入口，不在网络里。** `073` 把那一层定在「喂进去多少」，这次换回读整段的驱动
   （`bench.c` → `bench7`）复验：3 个整块（300 帧）与 4 个整块（400 帧）的文本、音频 token 数、
   逐层差值全部重现 `073` 的表，没有一个元素超差。**引擎的网络侧一字未改**，也不该改：
   批量卷积的块维、`(块, 频, 时, 通道) → (块·时, 通道·频)` 的通道重排、每块从位置 0 重排的正弦表、
   块拼接，四处在 300 / 400 帧上都是逐位相同或同一档余量。
2. **六段真实中文语音逐字一致。** s1、s1_2.0、s1_3.0、s1_4.0、s2、s3 六段的文本与参考逐字相同，
   一次喂完与 4096 一块流式两条喂法给出同一串字（第 3 节、第 4 节）。
3. **200 帧的老闸不变差，300 / 400 帧与六段真实语音变成实闸。** 老闸的数值与 `060` / `073` 逐条相同；
   新加的是断言而不是打印：`multichunk_divergence` 三条由「只打印」改成「不过就失败」，
   另加一条六段验收（`multichunk_acceptance`）。容差一个数没动，期望值没有放宽。

## 2. 修的是哪一层：音频注入口

### 2.1 量具：只读前 32000 个采样的那一份源码

`073` 第 3 节查清 `/tmp/voice-abi/bench7` 实际是 `bench3.c` 编出来的，它的输入边界是：

```c
int16_t pcm[32000];
size_t n = fread(pcm, sizeof(int16_t), 32000, f);   // 后面的音频一律不读
```

32000 个采样 = 2.000 s = 200 个梅尔帧 = 2 个整块 = 26 个音频 token。这一次把 `bench7` 按 `bench.c`
重编（`bench.c` 按 `ftell` 拿长度、`malloc` 整份读进来，并真的读 `argv[4]` 这个块长），
同一批音频、同一份模型目录、同一份 `mlx.metallib`，两种驱动的差别只在读了多少字节：

| 音频 | 旧 `bench7`（`bench3.c`，只读 32000 采样） | 重编 `bench7`（`bench.c`，读整段） |
| --- | --- | --- |
| `s1_3.0.pcm`（3.00 s） | 今天下午三点开会。 | 今天下午三点开会，请提前。 |
| `s1_4.0.pcm`（4.00 s） | 今天下午三点开会。 | 今天下午三点开会，请提前十分钟到会。 |
| `s1.pcm`（4.61 s） | 今天下午三点开会。 | 今天下午三点开会，请提前十分钟到会议室。 |

旧驱动在三段上都只说出「今天下午三点开会。」，与剩下多少音频无关——这正是 `071` 记的
「3 个整块起只输出前 2 块文本」。它不是引擎的行为。

### 2.2 引擎这一侧的注入口：整段 PCM 一路都是完整的

`code/` 里对应的注入口有四段，这次逐段读过，没有任何一处按长度截断：

| 位置 | 做什么 | 会不会丢样本 |
| --- | --- | --- |
| `voice-ffi/src/lib.rs:90` `voice_session_feed` | 把 `n` 个 `i16` 整份复制成 `Vec` 交给引擎线程 | 不会，长度就是调用方给的长度 |
| `voice-ffi/src/engine.rs:242` `Worker::feed` | 进有界队列（64 条），满时返回 1 而不是丢包 | 不会，`feed` 的返回值是可见的 |
| `voice-asr/src/session.rs:156` `Session::feed` | 全部样本进 `pending`，每 200 个 hop（32000 采样）派一步；`finish`（`:181`）把余下的 `pending` 再派一步 | 不会，`pending` 只增不减地被派完 |
| `voice-ffi/src/decoder.rs:299` `WeightDecoder::decode_text` | 每步把 `view.samples` 追加进 `self.pcm`，定稿时把**整段** `self.pcm` 交给常驻实例 | 不会，定稿不看块，只看整段 |

定稿那一侧（`voice-mlx/src/backend/transcribe.rs:379` `transcribe_with_opened`）对整段 PCM 一次
`log_mel`（`:390`），再整段进音频塔，所以「分几块喂」只影响说话中的部分步，不影响定稿。
六段验收里新加的第一条断言就是这个：识别报的 `mel_frames` 必须等于对同一份 PCM 现算的帧数，
少读一段会先在这里显形。

## 3. 六段真实中文语音逐条对照

驱动：`/tmp/voice-abi/bench7`（`bench.c` 重编）。模型目录与 metallib 取自装包：
`~/Library/Input Methods/RimeClient.app/Contents/Resources`。
「一次喂完」把块长传成比文件长度大的数（整份只发一次 `feed`）；「4096」按 4096 采样一块发。

| 音频 | 时长 | 期望（参考） | 一次喂完 | 4096 一块 | 一致 |
| --- | ---: | --- | --- | --- | :---: |
| `s1.pcm` | 4.61 s | 今天下午三点开会，请提前十分钟到会议室。 | 今天下午三点开会，请提前十分钟到会议室。 | 同左 | 是 |
| `s1_2.0.pcm` | 2.00 s | 今天下午三点开会。 | 今天下午三点开会。 | 同左 | 是 |
| `s1_3.0.pcm` | 3.00 s | 今天下午三点开会，请提前。 | 今天下午三点开会，请提前。 | 同左 | 是 |
| `s1_4.0.pcm` | 4.00 s | 今天下午三点开会，请提前十分钟到会。 | 今天下午三点开会，请提前十分钟到会。 | 同左 | 是 |
| `s2.pcm` | 4.55 s | 幫我把這段文字翻譯成英文，然後發到郵箱。 | 幫我把這段文字翻譯成英文，然後發到郵箱。 | 同左 | 是 |
| `s3.pcm` | 5.09 s | 下周的行程已经确认了吗？如果没有，我再联系一下。 | 下周的行程已经确认了吗？如果没有，我再联系一下。 | 同左 | 是 |

六段在仓内也走同一条路（`multichunk_acceptance`，常驻实例只开一次权重）：

| 音频 | 样本 | 梅尔帧（现算） | 音频 token（参考公式） | 提示 | 停止 |
| --- | ---: | ---: | ---: | ---: | --- |
| `s1` | 73700 | 460（460） | 60（60） | 78 | `<|im_end|>` |
| `s1_2.0` | 32000 | 200（200） | 26（26） | 44 | `<|im_end|>` |
| `s1_3.0` | 48000 | 300（300） | 39（39） | 57 | `<|im_end|>` |
| `s1_4.0` | 64000 | 400（400） | 52（52） | 70 | `<|im_end|>` |
| `s2` | 72878 | 455（455） | 59（59） | 77 | `<|im_end|>` |
| `s3` | 81429 | 508（508） | 66（66） | 84 | `<|im_end|>` |

「参考公式」是参考实现的 `_get_feat_extract_output_lengths`：整块 13 个、尾块 `ceil(尾帧/8)` 个，
不是按本仓输出回填的。`s2` 两边都给繁体，是模型自己的选择，没有改。

## 4. 两条喂法（一次喂完 / 4096 一块流式）是否相同

**定稿相同**：第 3 节两列逐字相同，六段都对。两条喂法的差别只在 `feed` 的次数，定稿都从整段 PCM 重算
（见 2.2 的表），所以这一条是构造上就该成立，实测也成立。

**说话中的部分步也吃到了第 3、4 块**：`bench7` 一次喂完时中间那几步会被跳过（`finish` 先把 `finishing`
置真，排在队列里的 `feed` 于是不再跑部分步），所以另编了一个给每块之间留 0.4 秒的驱动
（`bench8`，与 `bench.c` 只差一句 `usleep`），让部分步真的跑起来：

| 音频 | 第 1 条部分结果（2 秒处，2 个整块） | 第 2 条部分结果（4 秒处，4 个整块） | 定稿 |
| --- | --- | --- | --- |
| `s1`（4.61 s） | 今天下午三点开会。 | 今天下午三点开会，请提前十分钟到会。 | 今天下午三点开会，请提前十分钟到会议室。 |
| `s3`（5.09 s） | 下周的行程已经确认了。 | 下周的行程已经确认了吗？如果没有，我再。 | 下周的行程已经确认了吗？如果没有，我再联系一下。 |

第 2 条部分结果用的是第 3、4 块的声音（s1 的「请提前十分钟到会」只可能来自 4 秒那一段），
所以流式那一条也没有停在前 2 块。部分步的尾巴允许被下一步改写，与定稿不逐字相同是既有口径
（`notes/063`、`069`）。

**跨过 8 秒（多窗）时两条喂法也相同。** 把 `s1.pcm` 接三遍（13.82 s；按第 3 节那张表的同一套口径推，约 1381 个梅尔帧、180 个音频 token、
两个注意力窗 `[0, 104]` / `[104, 180]`），一次喂完与 4096 一块给的是同一串字：

```text
今天下午三点开会，请提前十分钟到会议室。今天下午三点开会，请提前十分钟到会议室。今天下午三点开会，请提前十分钟到会议室。
```

三段话都出来了，所以 800 帧之后的第二个窗也在用声音，不是只编了第一个窗。这一段没有逐窗的参考文本
（见第 7 节第 3 条），它证明的是「整段被吃完」与「两条喂法一致」，不是逐窗数值对参考。

## 5. 逐层差值：300 / 400 帧与 073 逐条相同

口径不变：容差 = 参考张量的绝对值上界 × 2^-11 × 允许单位数（`voice_parity::tolerance`），
卷积茎 4、第 0 层 4、投影 32，梅尔用绝对容差 `1e-3`。这一跑是把 073 的表重跑一遍，数值逐条相同。

### 300 帧（`s1_3.0`，3 个整块，39 个音频 token）

| 层 | 元素数 | max\|d\| | @下标 | 容差 | 折成 f16 单位 | 首个超差 | 超差数 |
| --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| 梅尔 | 38400 | 1.629591e-4 | 22326 | 1e-3 | — | 无 | 0 |
| 卷积茎第 0 块（批量） | 99840 | 0 | 0 | 7.7552795e-3 | 0.0 | 无 | 0 |
| 卷积茎第 1 块（单独编） | 99840 | 0 | 0 | 7.411957e-3 | 0.0 | 无 | 0 |
| 卷积茎第 2 块（单独编） | 99840 | 0 | 0 | 7.484436e-3 | 0.0 | 无 | 0 |
| 第 0 层 | 34944 | 1.953125e-3 | 15486 | 1.8432617e-2 | 0.4 | 无 | 0 |
| 投影 | 39936 | 2.193749e-4 | 28871 | 1.6241074e-3 | 4.3 | 无 | 0 |
| 投影 token 26..39（最后一块） | 13312 | 2.193749e-4 | 2247 | 1.6241074e-3 | 4.3 | 无 | 0 |

窗边界 `[0, 39]`（1 个窗，39 远小于 104）；文本 今天下午三点开会，请提前。，停在 `<|im_end|>`。

### 400 帧（`s1_4.0`，4 个整块，52 个音频 token）

| 层 | 元素数 | max\|d\| | @下标 | 容差 | 折成 f16 单位 | 首个超差 | 超差数 |
| --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| 梅尔 | 51200 | 1.629591e-4 | 29726 | 1e-3 | — | 无 | 0 |
| 卷积茎第 0 块（批量） | 99840 | 0 | 0 | 7.7552795e-3 | 0.0 | 无 | 0 |
| 卷积茎第 1 块（单独编） | 99840 | 0 | 0 | 7.411957e-3 | 0.0 | 无 | 0 |
| 卷积茎第 2 块（单独编） | 99840 | 0 | 0 | 7.484436e-3 | 0.0 | 无 | 0 |
| 卷积茎第 3 块（单独编） | 99840 | 0 | 0 | 8.102417e-3 | 0.0 | 无 | 0 |
| 第 0 层 | 46592 | 1.953125e-3 | 21456 | 1.8447876e-2 | 0.4 | 无 | 0 |
| 投影 | 53248 | 3.8785487e-4 | 28817 | 1.7175674e-3 | 7.2 | 无 | 4.43 倍余量 |
| 投影 token 39..52（最后一块） | 13312 | 1.9018352e-4 | 7227 | 1.7175674e-3 | 3.5 | 无 | 0 |

窗边界 `[0, 52]`；文本 今天下午三点开会，请提前十分钟到会。，停在 `<|im_end|>`。

四块单独编与整批编的第 k 块逐位相同（`max|d| = 0`），所以块维、通道重排、每块位置表三处没有偏差；
最后一块那 13 行与其它块同一档，所以「第 3 / 第 4 块」也没有跳出来。

## 6. 闸

### 6.1 200 帧的老闸（不变差）

| 闸 | 结果 | 与 060 / 073 对照 |
| --- | --- | --- |
| `--test encoder_parity` | 3 通过 | 卷积茎 0、第 0 层 3.90625e-3（容差 1.2954712e-2）、投影 1.1855885e-3（19.2 单位，容差 1.9779205e-3，余量 1.67 倍）——同一组数 |
| `--test decoder_parity` | 6 通过 | 主闸仍 7.8125e-2 对 8.203125e-2（7.6 单位，余量 1.05 倍） |
| `--test end_to_end_parity` | 4 通过 | 梅尔 200 帧、音频 token 26、提示 44、新生成 3 个、停在 `ImEnd`，token 与文本仍逐位对参考 |
| `cargo test --workspace --offline` | 61 通过、4 忽略、0 失败 | 打开 metal 的用例不在其中（`voice-mlx` 四个对照文件在关闭 metal 时 0 条） |

### 6.2 300 / 400 帧与六段真实语音的新闸

| 闸 | 内容 |
| --- | --- |
| `--test multichunk_divergence`（3 条） | 本轮由「只打印」改成「不过就失败」：梅尔帧数与容差、卷积茎四块逐块、第 0 层（整段与每 13 行）、投影（整段与每 13 行）、音频 token 39 / 52、窗边界 `[0, 39]` / `[0, 52]`、文本与停止符 |
| `--test multichunk_acceptance`（1 条，本轮新增） | 六段真实中文语音：`mel_frames` 等于对同一份 PCM 现算的帧数、音频 token 数按参考公式、提示长度 = 音频占位 + 18、文本逐字、停在 `<|im_end|>` |

容差取自 `voice-parity/src/tolerance.rs`，本轮没有新增常量、没有改数值。新闸的期望文本按 `071` / `073`
记的参考结果写死，不按本仓输出回填。

## 7. 仍剩什么

1. **装好的包已重编重装（第 9 节），真机上还没有按过一次。** 六段验收走的是 `/tmp/voice-abi/bench7`；
   「装好的包在真机上按键是否也读整段」要看一次真实按键，麦克风那一段（Squirrel 侧采集与
   `voice_session_feed` 的调用方）不在本轮的量程里。
2. **`s2` 两边都给繁体**：模型选择，与参考一致，没有改。
3. **超过 8 秒（800 帧）的多窗那一支没有逐窗的真实语音参考。** 六段最长 5.09 s，只有 1 个注意力窗；
   13.82 s 那一段（第 4 节）只证明了整段被吃完、两条喂法一致，没有参考文本逐窗比。
   `windows >= 20`（2080 token，约 160 秒）那条按窗切开的支路这次一次都没走到；`2..20` 个窗的
   加法遮罩支路只走到 2 个窗、且只做了自一致对照。
4. **说话中的部分步与定稿不逐字相同**（第 4 节的 s1 第 2 条部分结果给「到会」、定稿给「到会议室」）：
   尾巴 5 个 token 允许下一步改写，这是既有口径，不是本轮引入的差。
5. `/tmp` 下的量具不入库：`bench.c`（整段）、`bench8.c`（留间隔版）与编出来的二进制只在
   `/tmp/voice-abi/`，复现命令见第 8 节。

## 8. 复现

```text
# 1) 归档（带 SDKROOT、LIBRARY_PATH、MLX_SYS_SYSTEM_SDK=1 与 --target aarch64-apple-darwin）
cd /Users/dragon/repo/tools/rime-client/code/voice-engine
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"; export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
cargo build -p voice-ffi --features metal --target aarch64-apple-darwin --offline

# 2) 量具：bench.c 编成 bench7（读整段，argv[4] 是块长）
cd /tmp/voice-abi
xcrun clang -O2 bench.c \
  /Users/dragon/repo/tools/rime-client/code/voice-engine/target/aarch64-apple-darwin/debug/libvoice_ffi.a \
  -liconv -lc++ -lobjc -framework Foundation -framework Metal -framework Accelerate \
  -lSystem -lc -lm -o bench7
RES="$HOME/Library/Input Methods/RimeClient.app/Contents/Resources"
Z=/Users/dragon/repo/tools/rime-client/code/voice-oracle/assets/zh
for f in s1 s1_2.0 s1_3.0 s1_4.0 s2 s3; do
  ./bench7 "$RES/Models/Qwen3-ASR-0.6B-4bit" "$RES/mlx.metallib" "$Z/$f.pcm" 4096   # 流式
  ./bench7 "$RES/Models/Qwen3-ASR-0.6B-4bit" "$RES/mlx.metallib" "$Z/$f.pcm" 100000000  # 一次喂完
done

# 3) 四道 metal 闸（链接参数见 encoder_parity.rs 文件头）
export RUSTFLAGS="-C link-arg=-framework -C link-arg=Accelerate \
  -C link-arg=-Wl,-U,_MTLTensorDomain -C link-arg=-Wl,-U,_MTLLogStateErrorDomain \
  -C link-arg=-Wl,-U,_NSDeviceCertificationiPhonePerformanceGaming \
  -C link-arg=-Wl,-U,_NSProcessInfoPerformanceProfileDidChangeNotification \
  -C link-arg=-Wl,-U,_NSProcessPerformanceProfileDefault \
  -C link-arg=-Wl,-U,_NSProcessPerformanceProfileSustained \
  -C link-arg=-Wl,-U,___fmaxf16 -C link-arg=-Wl,-U,___fminf16"
for t in encoder_parity decoder_parity end_to_end_parity multichunk_divergence multichunk_acceptance; do
  cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
    --test "$t" -- --nocapture --test-threads=1
done
cargo test --workspace --offline
```

## 9. 重编、重打包、重装之后的复验

产品代码一个算子没改，但归档、Swift 产物与装好的包都重做了一遍，再用**装好的包自己的**权重与
metallib 把六段与五道 metal 闸重跑一遍。

### 9.1 产物

| 步 | 做什么 | 结果 |
| --- | --- | --- |
| 归档 | 删 `target/aarch64-apple-darwin/debug/libvoice_ffi.a` 后带 `SDKROOT` / `LIBRARY_PATH` / `MLX_SYS_SYSTEM_SDK=1` 重编 `cargo build -p voice-ffi --features metal --target aarch64-apple-darwin --offline` | exit 0，归档 285 728 424 字节（08:46） |
| Swift | 删 `.build/out/Products/Debug/Squirrel` 后 `swift build --product Squirrel` | `Build complete!`，产物 40 697 680 字节 |
| 符号 | `nm` 查二进制 | `backend10transcribe` 相关符号 65 条；`voice_*` 八个导出入口都在 |
| 打包 | `code/Client/scripts/make-app.sh`（资产先核哈希再动 `dist`） | `dist/Squirrel.app` 重打，签名逐个重做 |
| 装包 | `code/scripts/install-app.sh` | 停旧进程 → ditto → 签名校验 → 核包内资产，四步全过 |

### 9.2 装好的包

```text
codesign --verify --deep --strict "$HOME/Library/Input Methods/RimeClient.app"   # exit 0
Identifier=im.dragonshock.inputmethod.RimeClient  Format=app bundle with Mach-O thin (arm64)
Info.plist entries=19  Sealed Resources version=2 rules=13 files=22  Signature=adhoc
```

包内资产逐件在且非空：`Contents/Info.plist` 2 791 字节、`Contents/Resources/mlx.metallib`
183 870 464 字节、`Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/weights.safetensors`
537 626 967 字节、同目录 `vocab.json` 2 776 833 字节；`voice-assets.sha256` 逐个核过，一致。

### 9.3 装好的包再跑六段真实语音

驱动 `/tmp/voice-abi/bench7` 按 `bench.c` 对着重编后的归档再编一次（命令行见第 8 节），模型目录与
metallib 都取 `~/Library/Input Methods/RimeClient.app/Contents/Resources`。每条音频跑两种喂法：
4096 采样一块，与块长大于文件长度（整份一次 `feed`）。

| 音频 | 期望（参考） | 4096 一块 | 一次喂完 | 一致 |
| --- | --- | --- | --- | :---: |
| `s1.pcm` | 今天下午三点开会，请提前十分钟到会议室。 | 同左 | 同左 | 是 |
| `s1_2.0.pcm` | 今天下午三点开会。 | 同左 | 同左 | 是 |
| `s1_3.0.pcm` | 今天下午三点开会，请提前。 | 同左 | 同左 | 是 |
| `s1_4.0.pcm` | 今天下午三点开会，请提前十分钟到会。 | 同左 | 同左 | 是 |
| `s2.pcm` | 幫我把這段文字翻譯成英文，然後發到郵箱。 | 同左 | 同左 | 是 |
| `s3.pcm` | 下周的行程已经确认了吗？如果没有，我再联系一下。 | 同左 | 同左 | 是 |

十二次运行的定稿 `kind` 都是 `final`、`ok` 都是 `true`、都带 `decoded:true`，文本与第 3 节逐字相同。

### 9.4 五道 metal 闸与全工作区

| 闸 | 结果 |
| --- | --- |
| `--test encoder_parity` | 3 通过 |
| `--test decoder_parity` | 6 通过 |
| `--test end_to_end_parity` | 4 通过 |
| `--test multichunk_divergence` | 3 通过 |
| `--test multichunk_acceptance` | 1 通过 |
| `cargo test --workspace --offline` | 61 通过、4 忽略、0 失败 |

容差与期望值一个数没动（全部取自 `voice-parity/src/tolerance.rs` 与 `code/voice-oracle/assets/`）。

<!-- research-complete -->
