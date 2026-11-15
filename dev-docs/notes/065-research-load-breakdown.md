# 065-research-load-breakdown：4.8 秒里 3.73 秒是主机侧摊开，不是「eval 太多次」

测量日 2026-11-15 上午。改动只在 `code/voice-engine/voice-mlx/src/backend/`：
`weights.rs`（装载路径重写）、`text_decoder.rs`（多一个 `load`，`open_with` 收尾多一次
`finish`）、`audio_tower.rs`（同上）、`transcribe.rs`（`Engine::open` 两塔共用一份权重）。
`dev-docs/refs` 没动，librime 没动，`inject` 没调，层数没少一层，位宽没降一档，容差一个
数没改。

## 0. 结论速览

| 项 | 改前 | 现在 |
| --- | --- | --- |
| `engine_new`（六次中位，同一探针、同一模型目录、同一份 metallib） | **4.841 s** | **0.742 s** |
| 表头读（`SafeWeights::open`） | 0.0005 s | 0.0013 s |
| 全部张量的字节读取（1227 次、537 490 432 字节） | 0.391 s | 0.313 s |
| 主机侧转换（f16/f32/u32 解码 + 建数组） | **3.731 s** | 0.067 s |
| MLX 侧 eval 与上传 | 0.491 s（722 次） | 0.117 s（1 次） |
| 四项之外（`locate` 搜键名、建层结构） | 0.172 s | 0.157 s |
| 两塔装载（四项之和 + 上面那一行） | 4.785 s | 0.654 s（0.537 装载 + 末尾一次 eval 0.117） |
| `cold finish`（改前三次 / 现在六次） | 0.403–0.464 s | 0.222–0.263 s |
| `warm finish` | 0.199–0.206 s | 0.187–0.193 s |
| token | `[106287, 1773, 151645]` | `[106287, 1773, 151645]` |
| 文本 | `嗯。` | `嗯。` |
| 解码器主闸首 64 维最大差 | 7.8125e-2（容差 8.203125e-2，余量 1.05 倍） | 逐位相同 |
| `cargo test --workspace --offline` | 53 通过、4 忽略、0 失败 | 53 通过、4 忽略、0 失败 |
| 打开 metal 的三套闸 + 流式两条 | 13 通过 + 2 通过 | 13 通过 + 2 通过（差值逐位相同） |
| 峰值 maximum resident set size | 629 637 120 字节 | 723 402 752 字节 |

一句话：**4.8 秒里 3.73 秒花在「把 f16 字节逐元素解码成 `Vec<f32>`、再拷进 MLX、再
`as_dtype` 回 f16」上，跟 eval 次数关系不大。** 把这段改成「按文件里的位宽直接建数组、
末尾一次 eval」之后，装载（含末尾那次 eval）从 4.785 秒落到 0.654 秒；eval 那一项也从
722 次同步的 0.491 秒变成一次 0.117 秒。数值一个比特没变。

## 1. 「4.8–5.7 秒」这个带子，我这次量到的是哪一段

任务给的基线是 5.104 秒（六次中位）。为了让「前后」可比，改前改后各跑六次，用的是
**同一个 `bench3.c`、同一个探针、同一个模型目录、同一份 metallib**，差别只在链的归档：

- 改前：`/tmp/voice-abi/bench3-baseline`（15 Nov 06:27 链接），链的是**改动前那四个文件**
  （`weights.rs` / `text_decoder.rs` / `audio_tower.rs` / `transcribe.rs`）编出来的
  `target/aarch64-apple-darwin/debug/libvoice_ffi.a`；六次 `engine_new`：
  5.300 / 4.859 / 5.052 / 4.822 / 4.799 / 4.755 → 中位 **4.841 秒**。
- 改后：`/tmp/voice-abi/bench3-new`（15 Nov 06:34 链接），链的是这次的归档
  （285 260 432 字节，15 Nov 06:33:51，晚于改动后全部源文件的最后修改时间，`cargo build`
  复查为「无改动」）；六次 `engine_new`：1.191 / 0.746 / 0.742 / 0.733 / 0.742 / 0.741 →
  中位 **0.742 秒**。

  1.191 那一发是六次里的第一次（页缓存与 MLX 都凉），其余五发落在 0.733–0.746。

`/tmp/voice-abi/bench3` 已重编成当前源码链出的那份（与 `bench3-new` 同一份字节）；改前那份
留作 `bench3-baseline`，两者只是链的归档不同。

我自己量到的 4.841 与任务给的 5.104 差 5%，都在同一条波动带里；下面所有「前后」都用
这一对（4.841 → 0.742）。**两次实测数字与所用二进制来源如上，改后的数不是拿改前的归档
混出来的。**

## 2. 四段分开：先量，再改

量具是 `weights.rs` 里一条新的单元测试
（`backend::weights::tests::load_phases_are_reported_one_by_one`，`#[ignore]`），它把
`AudioTower::load` 与 `TextDecoder::load` 挂在同一份 `SafeWeights` 上，逐段记账。计时只在
`cfg(test)` 下存在：产品的静态归档里没有 `Instant`，`phase!` 宏不打开时直接退化成被包住的
表达式本身（`weights.rs` 文件头有说明）。

四段的口径：

| 段 | 从哪到哪 |
| --- | --- |
| 表头 | `SafeWeights::open` 整段：`File::open` + 读 8 字节长度 + 读 JSON 表头 + 解成文本 |
| 字节读取 | 每次 `read_range` 的 `seek` + 读裸字节（按 `data_offsets`） |
| 主机侧转换 | 读到 eval 之间的一切：f16/f32/u32 解码、建常量数组、挂 `as_dtype` / `dequantize` |
| 求值与上传 | 每次 `eval` 调用 |

`locate`（在 136 535 字节的表头里搜 `"key"` 再切 JSON）与建层结构（`Linear::new` 等）不算在
四格内，单独作「四项之外」列出。四格加起来不等于装载墙钟，这正是要分开报的原因。

改前那一次测量就已经把两塔挂在同一份 `SafeWeights` 上了（否则量不出「两塔」的分段），
而改前的产品路径是开两次文件；差的只是一次表头读（约 0.5 ms）与一次 `File::open`，
对下面这四段的数没有影响（每张量的读取、转换、eval 次数都一样）。

### 2.1 改前（两塔装载 4.785 秒）

```
表头 0.0005 s（136535 字节）
音频塔：墙钟 1.744 s｜表头 0.0000｜字节读取 0.134（523 次、202118144 字节）｜转换 1.362｜求值与上传 0.203（412 次）
文本塔：墙钟 3.041 s｜表头 0.0000｜字节读取 0.257（704 次、335372288 字节）｜转换 2.369｜求值与上传 0.288（310 次）
两塔合计：墙钟 4.785 s｜表头 0.0000｜字节读取 0.391（1227 次、537490432 字节）｜转换 3.731｜求值与上传 0.491（722 次）
整个装载四格和 4.613 s，其中表头 0.0005，四项之外（locate/建层）0.172 s
```

四项分开看：

| 段 | 秒 | 占装载 |
| --- | --- | --- |
| 表头 | 0.0005 | 0.0 % |
| 字节读取 | 0.391（1.37 GB/s，页缓存暖） | 8.2 % |
| 主机侧转换 | **3.731** | **78.0 %** |
| eval 与上传 | 0.491（722 次，每次约 0.7 ms） | 10.3 % |
| 四项之外 | 0.172 | 3.6 % |

### 2.2 改后（两塔装载 0.537 秒 + 一次 eval 0.117 秒）

```
表头 0.0013 s（136535 字节）
音频塔：墙钟 0.177 s｜表头 0.0000｜字节读取 0.122（523 次、202118144 字节）｜转换 0.023｜求值与上传 0.000（0 次）
文本塔：墙钟 0.360 s｜表头 0.0000｜字节读取 0.191（704 次、335372288 字节）｜转换 0.044｜求值与上传 0.000（0 次）
一次 eval：墙钟 0.117 s｜求值与上传 0.117（1 次）
两塔合计：墙钟 0.537 s｜字节读取 0.313（1227 次、537490432 字节）｜转换 0.067｜求值与上传 0.000（0 次）
整个装载四格和 0.499 s，其中表头 0.0013，四项之外（locate/建层）0.157 s
```

四格之外还有「四项之外」0.157 秒，所以装载墙钟是

```
0.0013（表头）+ 0.313（读取）+ 0.067（转换）+ 0.117（一次 eval）+ 0.157（locate/建层）= 0.654 s
```

（上面测试输出里的「两塔合计 0.537 s」是两塔 `load` 本身，末尾那次 `eval` 单列；改前
每张量自己 `eval`，所以 4.785 秒里已经含着 eval。口径对齐后按 4.785 → 0.654 比。）

| 段 | 秒 | 相对改前 |
| --- | --- | --- |
| 表头 | 0.0013 | +0.0008 |
| 字节读取 | 0.313 | −0.078（同 1227 次、同字节数；主机缓冲从 `Vec<u8>` 换成 8 字节对齐的 `Vec<u64>`，读法不变，这 0.078 秒在重复测量的波动内） |
| 主机侧转换 | 0.067 | **−3.664** |
| eval 与上传 | 0.117（1 次） | −0.374 |
| 四项之外 | 0.157 | −0.015 |
| 装载墙钟（含末尾那次 eval） | **0.654** | **−4.131** |

现在最大的一块变成字节读取（0.313 秒，537 MB 过一次 `read`），其次「四项之外」的
0.157 秒（1227 次 `locate`，每次都在表头里做一次子串搜索），再次是那一次 eval 的
0.117 秒。这三块都没改：本节的目标是把 4.8 秒的构成拆开、把最大的那块（转换）拿掉。

## 3. 改了什么

### 3.1 按文件里的位宽直接建数组：省掉的不是一次拷贝，是三次

改前一张 f16 稠密张量要走四步：

1. `read_range` 读进 `Vec<u8>`（f16 字节）；
2. `f16_le` 逐元素解码成 `Vec<f32>` —— 内存流量从 2 字节/元素涨到 4 字节/元素，
   而且是**逐元素函数调用 + 位运算**（dev 档案，没有内联也没有向量化）；
3. `Array::from_slice::<f32>` 把这份 `Vec<f32>` 拷进 MLX 的常量缓冲；
4. `as_dtype(Float16)` 再挂一个 GPU 逐元素转换节点，`eval` 时才真跑。

现在同一张张量只有两步：读进一段 8 字节对齐的缓冲，`Array::from_raw_data` 直接按
`Dtype::Float16` 建数组（底下 `mlx_array_set_data` 按元素类型拷一次）。

| 省掉的 | 是什么 |
| --- | --- |
| ① 逐元素 f16→f32 解码 | 一次 2 倍内存流量的往返 + 每元素一次 `f16_to_f32`；`weights.rs` 里这条函数连同它的单测一起删了，装载路径上不再有主机侧位宽解码 |
| ② `from_slice` 的 f32→MLX 拷贝 | 拷贝的量从「元素数 × 4 字节」降到「元素数 × 2 字节」，且不再需要那块 `Vec<f32>` |
| ③ `as_dtype` 的 GPU 逐元素转换 | 文件是 f16、目标也是 f16 时**不挂任何节点**；只有文件位宽与目标位宽真不一样（F32 文件要 f16）时才挂 `as_dtype` |

f16 → f32 → f16 本来就是一个精确往返（f16 的每个值都精确落在 f32 里，f32→f16 再取整回
原值），所以省掉这三步**不改一个比特**。分组权重同理：打包值 `U32` 直接按 `Uint32` 建
数组（以前是 `u32_le` 逐元素拼一个 `Vec<u32>`），缩放与偏置 `F16` 直接按 `Float16` 建数组。

### 3.2 先建所有 Array，末尾一次 eval

`SafeWeights` 多一个 `pending: Vec<Array>`：`dense_as` / `grouped_tensor` / `dequant_as`
一路只建图（`dequantize`、`as_dtype` 都只挂节点），`finish()` 用
`mlx_rs::transforms::eval` 把它们一次求完并清空。改前是每张量一次 `eval`（722 次设备同步，
0.491 秒）；现在一次（0.117 秒）。

`pending` 里放的是和常驻张量同一份数组的句柄（`Array` 是引用计数），不多占内存；
`finish` 之后立刻清空。

### 3.3 两座塔共用一份 `SafeWeights`

`Engine::open` 以前是 `AudioTower::open` + `TextDecoder::open`，各开一次文件、各求一遍值
（音频塔 412 次、文本塔 310 次）。现在一份 `SafeWeights` 走完两塔再 `finish()` 一次：文件
只开一次，`eval` 只有一次（上表「一次 eval 0.117 秒」）。`AudioTower::open` / `TextDecoder::open_with` /
`TextDecoder::open` 三个单独的口子保留原签名，内部各自 `finish()`。

### 3.4 没动的

层数、算术语序、量化位数（文本 4 位、音频 8 位）、组大小 64、`QuantPolicy` 的两组开关、
`lm_head` 的分块 argmax、容差口径、梅尔前端、分词器，一行没动。没有跳过任何一层，没有把
任何一层降精度。

## 4. mlx-rs 0.34 能做什么、这次用了什么、没用什么

- **`Array::from_raw_data`（用了）**：从一段字节直接建数组，就是 3.1 里省掉那三次的那条路。
  底下是 `mlx_array_set_data`，按元素类型从我们给的指针拷一次进 MLX 自己的常量缓冲。
  这条要求指针按元素类型对齐，所以 `weights.rs` 里那段主机缓冲用 `Vec<u64>` 撑起点（8 字节
  对齐）而不是 `Vec<u8>`（1 字节对齐）。
- **mmap（没有）**：mlx-rs 0.34 没有包装 mmap，`Array` 也没有「接管一块外部缓冲」的公开
  构造函数。
- **`mlx_array_new_data_managed_payload`（没有用）**：这一条在 `mlx-sys` 的绑定里（`mlx-c`
  的 C 接口），能把一块外部缓冲连同一个释放回调交给 MLX，**免掉 3.1 里剩下的那一次拷贝**。
  用它的代价是给 `voice-mlx` 直接加一条 `mlx-sys` 依赖，并且那块缓冲要按「权重活多久它就
  活多久」钉在主机侧（当前是读一张张量就放一张，峰值只有一张的量）。为了不到 0.15 秒的
  第四次拷贝，这次没有走；要走的条件写在上面两句里。

## 5. 前后六次逐次

| 序 | 改前 `engine_new` | 改后 `engine_new` |
| --- | --- | --- |
| 1 | 5.300 | 1.191（进程冷、页缓存冷） |
| 2 | 4.859 | 0.746 |
| 3 | 5.052 | 0.742 |
| 4 | 4.822 | 0.733 |
| 5 | 4.799 | 0.742 |
| 6 | 4.755 | 0.741 |
| 中位 | **4.841** | **0.742** |

改后六次的 `cold finish` 0.222–0.263、`warm finish` 0.187–0.193；改前三次的
`cold finish` 0.403–0.464、`warm finish` 0.199–0.206。定稿没有变慢，文本仍是 `嗯。`。

RSS 的代价：`maximum resident set size` 629 637 120 → 723 402 752 字节（+94 MB，+14.9 %）。
原因是一次 eval 之前，那些「已经拷进 MLX 的 CPU 常量」要一起等到末尾才送设备；改前每张量
求完值就把主机侧那份放开。峰值只涨了 94 MB 而不是一整份 537 MB，是因为 MLX 常量缓冲与
后面设备侧那份在统一内存上共用页。

## 6. 复跑

```text
# 四段墙钟（链接参数见 BUILD.txt）
cd code/voice-engine
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
export RUSTFLAGS="-C link-arg=-framework -C link-arg=Accelerate \
  -C link-arg=-Wl,-U,_MTLTensorDomain -C link-arg=-Wl,-U,_MTLLogStateErrorDomain \
  -C link-arg=-Wl,-U,_NSDeviceCertificationiPhonePerformanceGaming \
  -C link-arg=-Wl,-U,_NSProcessInfoPerformanceProfileDidChangeNotification \
  -C link-arg=-Wl,-U,_NSProcessPerformanceProfileDefault \
  -C link-arg=-Wl,-U,_NSProcessPerformanceProfileSustained \
  -C link-arg=-Wl,-U,___fmaxf16 -C link-arg=-Wl,-U,___fminf16"
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
  --lib load_phases -- --ignored --nocapture --test-threads=1

# engine_new（归档与新链接的 bench3；link.sh 里的 -liconv 要 SDKROOT 与 LIBRARY_PATH）
cargo build -p voice-ffi --features metal --target aarch64-apple-darwin
bash /tmp/voice-abi/link.sh /tmp/voice-abi/bench3.c /tmp/voice-abi/bench3
/tmp/voice-abi/bench3 \
  code/assets/qwen3-asr-0.6b-4bit \
  "$HOME/Library/Input Methods/RimeClient.app/Contents/Resources/mlx.metallib" \
  code/voice-oracle/assets/probe_pcm.bin
```

`bench3` 会自己等 6 秒让人把 `weights.safetensors` 挪走，再跑 cold / warm 两次定稿；
两次都出 `嗯。`，与改前一致。

<!-- research-complete -->
