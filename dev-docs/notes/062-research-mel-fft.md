# 062-research-mel-fft：梅尔前端换成 vDSP 实数 FFT，梅尔 0.272 → 0.050 秒，定稿 0.42 → 0.19 秒

测量日 2026-11-14。对象是 `code/voice-engine/voice-mel`（`log_mel_f32` 的每帧变换），
外加 `voice-parity` 里两处跟着改口径的注释与 `BUILD.txt` 的记录。探针仍是
`code/voice-oracle/assets/probe_pcm.bin`（2.00 秒、32000 样本、200 帧），模型目录与
metallib 取自已安装包 `~/Library/Input Methods/RimeClient.app/Contents/Resources`
（`Models/Qwen3-ASR-0.6B-4bit` 与 `mlx.metallib`）。模板与权重的算术一个数没动。

## 0. 结论速览

| 项 | 改前 | 改后 |
| --- | --- | --- |
| `MelFrontend::log_mel`（200 帧，dev 档案） | **0.272 秒**（复测 0.275） | **0.050 秒** |
| 定稿墙钟（`bench2` 暖，同进程第 2、3 次） | 0.423–0.434，中位 **0.424** 秒 | 0.190–0.194，中位 **0.193** 秒 |
| 定稿墙钟（`bench2` 冷，进程内第 1 次） | 0.451–0.519 秒 | 0.232 / 0.239 / 0.491 秒 |
| 梅尔与参考的最大绝对差 | 3.2138824e-4（下标 17189） | **6.866455e-5**（下标 436） |
| 梅尔闸余量（容差 1e-3） | 3.11 倍 | **14.56 倍** |
| `voice_engine_new`（含常驻权重） | 4.802–4.824 秒 | 4.813–5.296 秒（首次运行偏高，属噪声） |
| token / 文本 | `[106287, 1773, 151645]` / `嗯。` | 同左 |
| `cargo test --workspace --offline` | 49 通过、4 忽略、0 失败 | **51 通过**、4 忽略、0 失败 |
| 打开 metal 的三套闸 | 13 通过、0 失败 | 13 通过、0 失败，实测差值逐位相同 |

一句话：**每帧 400 点复 DFT 换成 vDSP 的五条 80 点实数 DFT 加一次合并**，梅尔快了 5.4 倍，
与参考的差反而小了 4.7 倍（不是靠放宽容差），定稿从 0.42 秒降到 0.19 秒。

## 1. 算法选择：为什么是「五条 80 点」而不是补零到 512

### 1.1 vDSP 支持哪些长度

`Accelerate/vecLib` 的 `vDSP.h` 把 `vDSP_DFT_zrop_CreateSetup`（实数，打包）的实现范围
写死在一段注释里：`Length = 2**n`，或 `Length = f * 2**n`，其中 `f` 取 3、5、15，
实数版要求 `n >= 4`（复数版 `vDSP_DFT_zop` 是 `n >= 3`）。400 与 200 都不在这个集合里：

```
400 = 5² · 2⁴      f 得取 25
200 = 5² · 2³      f 得取 25
```

实测也是这样（`/tmp/voice-abi/dftprobe2.c`，一次 `clang -framework Accelerate`）：

```
zrop(400)=0x0 zop(400)=0x0 zop(200)=0x0      # 三个都是空指针
zrop(80) 编出来了：Or[0]=-7.89805 Oi[0]=1.46707 Or[1]=-4.14545 Oi[1]=0.235911
naive DC=-3.94902 Nyquist=0.733536 bin1=(-2.07273, 0.117955)
```

注意后两行：vDSP 实数前向变换的直流与奈奎斯特正好是朴素 DFT 的 **2 倍**，这就是
`vDSP.h` 里那个 `C`（前向 `C = 2`，反向 `C = 1`），代码里按 0.5 折掉。

所以「把 400 点直接交给 vDSP」这条路不存在，只剩两种改法：

| 方案 | 频点 | 能不能不改容差 |
| --- | --- | --- |
| 补零到 512（或 480）做实数 FFT | 落在 `2πk/512`、`2πk/480` | **不能**：参考算的是 `2πk/400`，得插值往回挪 |
| 400 = 5 × 80，五条 80 点实数 FFT 再合并 | 仍是 `2πk/400` | 能：与参考同一网格 |

补零那条不是「少算一点」，是**换了一道题的答案**：对 400 个加窗样本补零到 512 之后，
vDSP 给的是同一个离散时间傅里叶变换在 512 个等间隔频率上的采样，而滤波器组要的是 400 个
等间隔频率上的采样。两者错开约 22%，功率谱在梅尔带边沿本来就有陡坡，插值误差比 1e-3 的
闸还大。要么插值要么改容差，两件都不做。

### 1.2 采用的分解

80 = 5 · 2⁴ 是 vDSP 支持的长度，400 = 5 × 80 正好一级时分抽取：

```text
X[k]  = Σ_{r<5} W_400^{r k} · A_r[k mod 80]
A_r[m] = Σ_{m<80} x[5m + r] · W_80^{m k}          （五条 80 点实数 DFT）
```

`x[5m + r]`（`r = 0..4`）是五条抽取支路，下标集合互不相交、并集是 0..399；下标留在 41..79
时用共轭补（支路输入是实数，`A_r[80-m] = conj(A_r[m])`）。

实现落在 `code/voice-engine/voice-mel/src/lib.rs`：

- 每条支路把 80 个样本按打包约定拆成偶/奇两半，`vDSP_DFT_Execute` 一次，输出里
  直流在 `re[0]`、奈奎斯特在 `im[0]`、其余是 `re[k] + i·im[k]`，全部乘 0.5 折掉 vDSP 的 2 倍增益；
- 201 个频点各做 5 项合并（`r = 0` 的旋转因子恒为 1，不查表），累加用 f64；
- 暂存用 `#[repr(align(16))]` 的定长数组：vDSP 文档把 16 字节对齐列为建议，不对齐时
  某些长度会明显变慢；
- 一个 `vDSP_DFT_zrop` setup 管整段音频（200 帧），`Drop` 里释放，不每帧建一次。

非 macOS 目标退化成逐点直接 DFT（原来那份算术，只是不再预存 cos/sin 表）。它不在产品路径上，
留在那里是为了这箱子在没有 Accelerate 的机器上还能编译、还能算对。

## 2. 与参考的差：3.2138824e-4 → 6.866455e-5

同一条闸（`voice-parity/tests/parity.rs::mel_matches_reference`，128×200 逐元素），
同一份资产，只换前端算术：

```
改前 最大绝对差：3.2138824e-4（下标 17189：实际 -8.3754385e-1，参考 -8.3786523e-1）
     梅尔：实测差 3.2138824e-4，容差 1e-3，余量 3.11 倍
改后 最大绝对差：6.866455e-5（下标 436：实际 -1.0573416e0，参考 -1.0572729e0）
     梅尔：实测差 6.866455e-5，容差 1e-3，余量 14.56 倍
```

与 055/058 记的 3.2e-4 同一档（那是直接 DFT 的数），现在是它的 **1/4.68**。

差的变小有个说得通的来源：参考是 numpy 在 f32 上跑的 FFT，改前这一侧是「f32 样本 + f64
精确 DFT」，两边精度不对称，差主要来自参考自己的 f32 蝴蝶运算；改后这一侧也是 f32 蝴蝶
（vDSP），舍入量级更接近，差就掉了一个量级。这是**观察加解释**，不是证明——能证明的只有
「同一个参考、同一条闸，实测差从 3.2e-4 掉到 6.9e-5」。

`tolerance.rs` 里 `MEL_TOLERANCE` 仍是 `1e-3`，一个数没改：实测差变小只是余量变大，
不是把闸放宽松紧的理由。两处注释（`tolerance.rs` 模块头与 `MEL_TOLERANCE` 的文档、
`parity.rs` 里那条闸的注释）改成了「参考是 numpy 的 f32 实数 FFT，这里是 vDSP 的五条
80 点实数 DFT 再合并」，并把这一档实测数写进去。

## 3. 单量梅尔本体

`/tmp/voice-abi/melbench`（cargo 项目，`path` 依赖 `voice-mel`，`profile.dev` 与产品同一档
`opt-level = 0`，因为 `BUILD.txt` 里装包用的是 `cargo build` 而不是 `--release`）。
同一台机器、同一时段、同一份探针，各跑 6 次取第 2..6 次的中位：

```
改前 round 0..5：0.273 0.271 0.272 0.272 0.274 0.274  中位 0.272 秒
     （把 HEAD 那版 lib.rs 换回来重编复测：中位 0.275 秒）
改后 round 0..5：0.050 0.050 0.051 0.049 0.051 0.049  中位 0.050 秒
```

运行期分段（`voice-mlx/tests/phase_timing.rs`，`#[ignore]`，`--test-threads=1`）：

```
音频塔 open 1.676 s
解码器 open 2.775 s
梅尔 0.049 s（200 帧）          # 改前 0.27 s
编码器 0.134 s（26 个音频 token）
预填充 0.070 s（44 个提示 token，argmax 106287）
贪心 0.119 s（3 个新 token）
```

剩下的 0.050 秒里，**0.043 秒是滤波器组**：128 × 201 的逐点乘加，每帧一遍，这次没有动它
（`melbench` 里把同一段循环单独拎出来量：200 帧 0.042–0.043 秒，两次都测过）。FFT 加合并
合计约 0.007 秒。也就是说这一刀之后，梅尔的瓶颈从变换挪到了滤波器组。

## 4. 定稿墙钟前后

同一个 `bench2`（`/tmp/voice-abi/bench2.c`：进程内建引擎一次，然后 cold / warm / warm 三次定稿），
改前改后各自重编 `libvoice_ffi.a`、各自用 `/tmp/voice-abi/link.sh` 重新链，同一天同一台机器：

```
改前（bench2-before）
  engine_new 4.802 s   cold 0.519   warm 0.434   warm 0.426
  engine_new 4.804 s   cold 0.456   warm 0.424   warm 0.430
  engine_new 4.824 s   cold 0.451   warm 0.423   warm 0.424

改后（bench2-new）
  engine_new 5.296 s   cold 0.491   warm 0.233   warm 0.203   # 重链后第一次，页缓存冷
  engine_new 4.825 s   cold 0.239   warm 0.194   warm 0.192
  engine_new 4.813 s   cold 0.232   warm 0.194   warm 0.190
  engine_new 5.675 s   cold 0.466   warm 0.204   warm 0.198   # rustfmt 之后重编再加跑一次
```

- 暖定稿：中位 **0.424 → 0.193 秒**（改后四次运行八个暖数：0.233 0.203 / 0.194 0.192 /
  0.194 0.190 / 0.204 0.198；首跑那一对偏高），少了约 0.23 秒，与「梅尔少 0.222 秒」对得上。
- 冷定稿（进程内第一次）：0.451–0.519 → 0.232–0.491 秒。第一次那一档（0.491）是重链后
  首跑，与 `engine_new` 一起偏高，第 2、3 次的 0.232 / 0.239 才是稳定值。
- `engine_new` 没变（4.81–4.83，除首跑 5.30）：梅尔前端建表本来就是微秒级。
- 三次运行的回复都是 `{"decoded":true,"kind":"final","ok":true,"text":"嗯。"}`；
  token 序列由 `end_to_end_parity::greedy_sequence_and_text_match_reference` 卡住，仍是
  `[106287, 1773, 151645]`。

## 5. 逐层闸的实测

`cargo test --workspace --offline`：exit 0，**51 通过**（`voice-mel` 从 3 条变 5 条）、
4 忽略、0 失败。新增的两条：`branch_twiddles_are_the_400_point_roots`（旋转因子表与
`W_400^{rk}` 的定义一致、五条支路的下标不重不漏）、`single_tone_peaks_at_its_bin`
（单帧单频正弦，功率谱起在 25 号频点、旁瓣低于主峰的 1%）。这两条不依赖参考资产，
能独立抓住支路顺序或旋转因子接错。

打开 metal 的三套对照（13 通过、0 失败），实测差值与 060 逐位相同：

| 闸 | 实测差 | 容差 | 余量 |
| --- | --- | --- | --- |
| `conv_stem_out` | 0 | 3.955841e-3（4 个单位） | 无穷（差为 0） |
| `encoder_layer0_out` | 3.90625e-3 | 1.2954712e-2（4 个单位） | 3.32 倍 |
| `encoder_proj_out` | 1.1855885e-3 | 1.9779205e-3（32 个单位） | 1.67 倍 |
| `prefill_logits_64`（主闸） | 7.8125e-2 | 8.203125e-2（8 个单位） | 1.05 倍 |
| `prefill_logits_64`（注入参考音频特征） | 6.25e-2 | 8.203125e-2（8 个单位） | 1.31 倍 |
| `mel` | 3.2138824e-4 → **6.866455e-5** | 1e-3（绝对容差） | 3.11 → **14.56 倍** |

解码器的两条归因断言（`RopeMode::SinOnly` 首 64 维最大差 2.5214844e1、`qk_norm: false`
2.1436523e1，都在容差外）照旧成立，全词表 argmax 仍是 106287。

## 6. 改了哪些文件

| 位置 | 改动 |
| --- | --- |
| `code/voice-engine/voice-mel/src/lib.rs` | 删掉 `cos_table`/`sin_table`（201×400×2 个 f32）与每帧 201 次 400 点复 DFT；新增 `twiddle`（4×201 个 f64 复旋转因子）、`SubDft`（`vDSP_DFT_zrop` setup 的封装，`Drop` 释放）、`power_frame`（五条 80 点实数 DFT + 合并）、`Align16` 暂存；模块头写清分解与为什么不用补零；测试 3 条变 5 条 |
| `code/voice-engine/voice-parity/src/tolerance.rs` | 只改注释：梅尔那一项的算术描述换成 vDSP 分解，补上 062 的两个实测数；`MEL_TOLERANCE = 1e-3` 未改 |
| `code/voice-engine/voice-parity/tests/parity.rs` | 只改注释：同上 |
| `code/voice-engine/BUILD.txt` | 追加「梅尔前端换 vDSP 实数 FFT」一节：链接要求（`-framework Accelerate` 本来就带）、测试 51 通过、`bench2` 前后三次、梅尔 0.272 → 0.050 秒 |

`code/Client`、`code/VoiceInterface`、`code/voice-oracle`、`dev-docs/refs` 都没有动；
没有按键测量，也没有调用 `inject`（这次改的是定稿里的主机侧前端，与按键路径无关）。

## 7. 局限

- 非 macOS 的退化路径（逐点直接 DFT）只保证编译与算对，没有第二台机器验证过；
  那条路径在 dev 档案下与改前同速。
- 一段音频建一次 setup、200 帧复用它。若将来改成「每帧单独调」（例如流式出梅尔），
  setup 的建/销开销会进到每帧，得重新量。
- 采样转换仍是 `Vec<f32>` 一次性分配（`log_mel` 里那条 `map`），2 秒探针上可忽略，
  但它与 `reflect_pad` 的两次分配都还在热路径上。
- 滤波器组 0.043 秒这一块原样保留：矩阵乘没有走 BLAS，仍是带零权重跳过的逐点累加。
- 端到端测试那次打印的墙钟 5.1 秒含两座塔的 `open`，与 `bench2` 的 `finish` 不是一回事。

## 8. 复现

```text
cd /tmp/voice-abi/melbench && cargo build --offline --target aarch64-apple-darwin \
  && ./target/aarch64-apple-darwin/debug/melbench          # 梅尔本体，6 次取中位

cd /Users/dragon/repo/tools/rime-client/code/voice-engine
cargo test --workspace --offline                           # 51 通过、4 忽略
cargo test -p voice-parity --offline --test parity mel_matches_reference -- --nocapture
                                                           # 打印 6.866455e-5

export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
export LIBRARY_PATH="$SDKROOT/usr/lib"
export MLX_SYS_SYSTEM_SDK=1
cargo build -p voice-ffi --features metal --target aarch64-apple-darwin
cd /tmp/voice-abi && bash link.sh bench2.c bench2-new
RES="$HOME/Library/Input Methods/RimeClient.app/Contents/Resources"
./bench2-new "$RES/Models/Qwen3-ASR-0.6B-4bit" "$RES/mlx.metallib" \
  /Users/dragon/repo/tools/rime-client/code/voice-oracle/assets/probe_pcm.bin
```

打开 metal 的三套对照要带 `RUSTFLAGS`（见 `BUILD.txt`「音频塔的真实数值与三个闸」一节），
并且 `--test-threads=1`。

<!-- research-complete -->
