# 053-research-encoder-parity：音频塔三个闸对上，卷积茎逐位相同

测量日：2026-11-14。对象是 `code/voice-engine/voice-mlx` 新加的两块：
`backend/weights.rs`（`weights.safetensors` 的读取）与 `backend/audio_tower.rs`
（音频塔的权重与前向）。参考资产来自 `code/voice-oracle/assets`，参考实现是
`code/voice-worker/.venv` 里的 `mlx_qwen3_asr` 0.4.4，只作对照标准。

## 0. 结论速览

| 闸 | 参考形状 | 实测量 | 最大绝对差 | 参考绝对值上界 | 容差 |
| --- | --- | --- | --- | --- | --- |
| `conv_stem_out` | (16, 13, 480) | 99840 / 99840 个元素 | **0** | 2.0253906 | 3.955841e-3 |
| `encoder_layer0_out` | (1, 26, 896) | 23296 / 23296 | **3.90625e-3** | 6.6328125 | 1.2954712e-2 |
| `encoder_proj_out` | (26, 1024) | 26624 / 26624 | **1.1855885e-3** | 0.12658691 | 1.9779205e-3 |

三项都在容差内，超差元素 0 个。形状先对上：卷积茎第 0 块按参考口径是
`(16, 13, 480)`（频、时、通道），第 0 层是 `(1, 26, 896)`，投影是 `(26, 1024)`。

卷积茎逐位相同，不是「差得很小」：99840 个元素一个都没差。这条说明卷积核布局、
`(频, 时, 通道) → (时, 通道*频)` 的重排、`conv_out` 的权重转置与 8 位反量化都是
对的——错任何一处，这一闸会先炸。

## 1. 参考的 dtype 与资产口径

- `manifest.json` 写 `model_dtype: float16`、`activation_dtype: mlx.core.float16`。
- 参考 `tower(mel.astype(dtype), feature_lens)`：梅尔是 f32 算出来，进编码器前才转 f16。
  卷积、注意力、LayerNorm、GELU 全在 f16 上跑；每层输出再按 f16 钳到
  `65504 - 1000 = 64504`。
- 资产文件本身是 **f32 小端**：`dump_reference.py` 把 f16 激活 `np.asarray(..., dtype=np.float32)`
  写出。f16 的每个值都能被 f32 精确表示，所以对照时的参考侧与实测侧都是
  「f16 值在 f32 里的展开」，比较的是同一层量化下的数。
- 因此这一层不存在「参考 f32、实际 f16」那种不可消的差距：除了求和顺序与
  正弦表在主机上的最后一位，两侧的取值格点一样。

## 2. 权重来源

`weights.safetensors` 是「8 字节表头长度 + JSON 表头 + 裸数据」。
`backend/weights.rs` 只做定位与读出，不解释层结构：

| 名字前缀 | 存放 | 读法 |
| --- | --- | --- |
| `audio_tower.conv2d1/2/3` | 稠密 f16，`(out, 3, 3, in)` | `dense_as`，配 `<name>.bias` |
| `audio_tower.conv_out`、`layers.N.*`、`proj1`、`proj2` | 8 位分组，`U32` 打包 | `affine_as`，键名带 `.scales` 与 `.biases` |
| `audio_tower.*layer_norm`、`ln_post` | 稠密 f16 | `dense_as`，配 eps |

组大小 64、位数由调用方传入：`quantization_config.json` 写 `bits: 4`、
`audio_tower_bits: 8`，两座塔的组大小都是 64。位数不是猜的——`conv_out` 的
打包权重是 `(896, 1920)`、缩放是 `(896, 120)`，`7680 * 8 / 32 = 1920`、
`7680 / 64 = 120`，只有 8 位这一组数能对上。

`voice-tensor` 的 `Storage::Grouped` 原来只带 `group_size`，反量化固定 4 位。
本次给它加了 `bits`（文本 4、音频 8），层代码仍然只说「分组权重」，
不出现位数。`FakeTensor::grouped_with_bits` 是给形状测试用的对应入口。

装载时把权重统一转成激活 dtype（f16）：参考的 `nn.QuantizedLinear` 反量化后也是
f16，若这里留 f32，`matmul` 会把整条链提升成 f32，与参考就不是同一个算术了。

## 3. 前向口径（042 第 4 节）

1. 梅尔 `(128, frames)` 按 f32 收，转 f16 放上设备。
2. 每 100 帧一块：reshape 成 `(128, n, 100)` → transpose `(n, 128, 100)` →
   补通道成 `(n, 128, 100, 1)`；三层 `Conv2d(k=3, s=2, p=1) + GELU`。
3. `(n, 16, 13, 480)` → transpose `(n, 13, 480, 16)` → reshape `(n*13, 7680)`。
   通道在前：`480 * 16 = 7680`。
4. `conv_out`（`7680 → 896`，无偏置）。
5. 每块从位置 0 加正弦表：`log(10000) / (half_dim - 1)`，`half_dim = 448`，
   拼 `[sin | cos]`，f32 算表、转 f16 再加。
6. 窗：`tokens_per_window = 13 * (800 / 100) = 104`。本次 26 个 token，
   边界 `[0, 26]`，只有 1 个窗，mask 为 None。
7. 18 层 `x = x + Attn(LN(x)); x = x + FC2(GELU(FC1(LN(x))))`，双向、
   无 RoPE、无因果遮罩，f16 时每层输出钳到 64504。
8. `ln_post`（eps 1e-5）→ `proj1` + GELU → `proj2`（`896 → 1024`）。

尾部块按官方的做法：块内先补到 100 帧再卷积，再切回有效 token
（`(tail + 7) / 8`）。本次探针 200 帧正好两块，走不到尾块。

## 4. 容差怎么定的

单位取 f16 的机器精度 `2^-11 = 4.8828125e-4`（1.0 附近的格点间距）。
容差一律写成

```text
容差 = 参考该张量的绝对值上界 × 2^-11 × 乘数
```

乘数不是拿结果凑出来的，是「这一闸要做多少次 f16 舍入」的量级：

| 闸 | 乘数 | 依据 | 实测折算 |
| --- | --- | --- | --- |
| 卷积茎 | 4 | 3 层卷积 + 3 次 GELU，每次一个 f16 存储 | 0 个单位（逐位相同） |
| 第 0 层 | 4 | 1 次双向注意力求和 + 2 层前馈 + 残差 | 1.21 个单位（恰好 1 个 ULP） |
| 末尾投影 | 32 | 18 层，每层都往隐层叠一次 f16 舍入，长度 26 的注意力 | 19.2 个单位 |

实测差值与上界的对应：

- 第 0 层最大差 `3.90625e-3` 出现在下标 1697（实际 `6.1367188`，参考 `6.1328125`）。
  `[4, 8)` 区间里 f16 的格点间距正是 `2^-8 = 3.90625e-3`，所以这就是**一个 ULP**。
- 末尾投影最大差 `1.1855885e-3` 出现在下标 20688（实际 `-1.4312938e-3`，
  参考 `-2.6168823e-3`）。该处数值很小，绝对差折到本张量上界
  `0.12658691` 上是 19.2 个单位——18 层累积在这个量级，不是某一步接错。
- 卷积茎 0。

容差没有为了「把失败改成成功」放宽：这一轮第一次跑就是这三个数，
乘数按上面的量级从 8/16/64 收到 4/4/32 之后仍然全过，且余量最紧的一项
（末尾投影）还有 1.67 倍。

## 5. 归因：换掉口径差值从哪一步变大

`wrong_encoder_options_make_the_difference_bigger` 用 `EncoderOptions` 把三条口径
各换一次，同一段梅尔、同一份权重重跑。这条测试的作用是证明「对上」不是碰上的：

| 口径 | 第 0 层最大差 | 末尾投影最大差 |
| --- | --- | --- |
| 默认（分母 half-1、每块从位置 0、窗 800 帧） | 3.90625e-3 | 1.1855885e-3 |
| 分母用 `half_dim`（差一位） | 3.7109375e-2 | 1.3971468e-3 |
| 位置跨块连续编号（块 1 从 13 起） | **5.947998** | **0.12468285** |
| 窗宽改成 100 帧（26 个 token 切成 2 个窗） | 3.7109375e-2 | 8.285414e-2 |

- 位置表分母错一位：第 0 层差一个数量级。位置公式是错的能被认出来。
- 位置编号不按块重启：第 0 层差到 5.9，是量级级别的错。块内从 0 起这一条是硬的。
- 窗宽改小：等于让两块互相看不见，第 0 层与投影都明显变大。这条也顺带把
  「多窗 + 加法 mask」那条路径跑通了（本次参考只有 1 个窗，正常路径走不到它）。

## 6. 命令与实测

```text
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
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline
```

```text
running 3 tests   (tests/encoder_parity.rs)
test audio_tower_weights_load_from_safetensors ... ok
test encoder_matches_reference ... ok
test wrong_encoder_options_make_the_difference_bigger ... ok
test result: ok. 3 passed
```

链接参数为什么要写这么长：MLX 的静态库引用了 `Accelerate` 的 BNNS 与 `cblas`
（`-framework Accelerate`），还引用了当前 SDK 只在 arm64e 段导出的一组
Metal/Foundation 弱符号与两个 `_Float16` 内建（`-Wl,-U,<符号>`，留到运行时解析）。
不写这些，链接直接失败；写成参考那次编译时的弱引用，结果一样。细节记在
`code/voice-engine/BUILD.txt`。

不打开 `metal` 时：

```text
cargo test --workspace --offline     # exit 0，不编 MLX
cargo test -p voice-parity --offline # 3 通过 / 4 忽略
```

`voice-parity` 里那个 `encoder_matches_reference` 仍然是 `#[ignore]`，原因改成了
「实对照在 `voice-mlx/tests/encoder_parity.rs`：本箱子不链接 MLX」。它打印参考形状
与这次记下的三个数，不再声称「无从计算」。

## 7. 输入用的是哪一份梅尔

编码器吃的是**参考的 `mel`**（`mel.bin` 的 f32 值，转 f16），不是 `voice-mel` 的输出。
理由：三个闸是参考用它自己的梅尔产出的，换一份梅尔就是换输入，差值不再属于编码器。
`voice-mel` 与参考的梅尔另有闸：052 记的 128×200 上最大绝对差 `3.2138824e-4`。
把 `voice-mel` 接进来会得到一条端到端的数，但那条数与这三个闸不可比，所以没有报。

## 8. 仍未对上的项

- **第 1 到 17 层没有单独的参考量**。参考资产只抓了 `layers[0]`，所以中间层只能
  通过末尾投影间接约束。要逐层报差值，得往 `dump_reference.py` 里补层输出。
- **多窗路径没有参考资产**。探针 26 个 token 只有一个窗（`[0, 26]`）；窗数 >= 20 时
  按窗切开算的那条分支目前只有形状上的自洽，没有参考数值。
- **尾块路径没有参考资产**。本次 200 帧正好两块 100 帧，`(tail + 7) / 8` 与
  「先补到块长再卷积」那段代码没有被这段音频执行到。
- **解码器、logits、贪心 token 三项仍未对上**（052 的四项里还有三项）。
- **编码器还没有接进产品路径**。`voice-mlx::transcribe_finish` 仍是 64 个采样均值
  那条形状骨架，`voice-asr` 的图仍用全零权重。这次对上的是 `AudioTower` /
  `encode_mel` 这条真实的数值路径，不是 `finish` 的返回值。

## 检索

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | 读参考的编码器实现 | `code/voice-worker/.venv/.../mlx_qwen3_asr/encoder.py` | 逐块卷积、通道优先重排、每块位置从 0、窗 800 帧、f16 钳位 |
| 2 | 读注意力实现作为缩放口径 | 同目录 `attention.py` | `1/sqrt(head_dim)`，走 `mx.fast.scaled_dot_product_attention` |
| 3 | 读量化配置 | 模型目录 `quantization_config.json` | `bits: 4`、`group_size: 64`、`audio_tower_bits: 8` |
| 4 | 核对权重键与打包形状 | 模型目录 `weights.safetensors` 表头 | `conv_out` 打包 `(896,1920)`、缩放 `(896,120)`，只有 8 位能对上 |
| 5 | 读参考资产口径 | `code/voice-oracle/assets/manifest.json` | 资产是 f32，激活是 f16，卷积茎只落第 0 块 |
| 6 | 写权重读取 | `voice-mlx/src/backend/weights.rs` | 稠密/分组两条路，位数由调用方给 |
| 7 | 写音频塔与前向 | `voice-mlx/src/backend/audio_tower.rs` | 18 层装载 + 042 第 4 节前向 + 三个中间量 |
| 8 | 给 `Storage::Grouped` 加位数 | `voice-tensor/src/ops.rs`、`src/fake.rs`、`src/config.rs` | 文本 4 位、音频 8 位 |
| 9 | 写三闸对照 | `voice-mlx/tests/encoder_parity.rs` | 0 / 3.90625e-3 / 1.1855885e-3 |
| 10 | 跑归因口径 | 同上第二条测试 | 分母 half 0.0371、跨块编号 5.95、窗 100 帧 0.0829 |
| 11 | 跑全工作区 | `cargo test --workspace --offline` | exit 0 |

<!-- research-complete -->
