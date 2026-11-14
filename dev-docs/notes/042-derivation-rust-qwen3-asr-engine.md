# 042-derivation-rust-qwen3-asr-engine：全量设计

`041` 只划了语言边界，没有把 Qwen3-ASR 写成可实现的算子图。本文替换那份瘦设计。候选。分发物里不出现 Python。

键盘组合仍由已经钉住的 librime 完成。那条约束不动。除此之外，语音从采样到文字的全部状态与运算都在 Rust 里。Swift 只保留 macOS 输入法套件要求的那一层：按键、麦克风设备、`setMarkedText`、`insertText`。

## 1. 为什么是这一个形状

Python 解释器、虚拟环境和编译机上的源码路径都不是安装包的一部分。识别若依赖它们，包就不能换一台电脑运行。把同一段 Python 收成子进程，仍然要把解释器装进包里，包的形态没有变。

Qwen3-ASR 的前向是固定的一层层线性、卷积、归一化、注意力和门控。这些运算在 MLX 的 C API 里已经有对应实现，`mlx-rs` 0.34 把该 C API 包成 Rust，并链接 Metal（当前绑定所对的 MLX 版本是 0.32.2）。因此「用 Rust 重建这张图」是把每一层写成对张量运算的调用，不是先发明一套新的数组库。MLX 后续增加的量化或 Metal 内核，只替换张量运算的实现。

`mlx-rs` 0.34 起，`Array` 不是 `Send`。图的求值必须留在创建它的那根线程和那条 MLX stream 上。引擎因此是输入法进程内的一个线程，不是第二个操作系统进程，也不是可以随便 `spawn` 的任务。

## 2. 箱子

| 箱子 | 依赖 | 持有 |
| --- | --- | --- |
| `voice-tensor` | 无 | 张量运算特质，以及形状、数据类型。不出现 MLX 的类型名 |
| `voice-mlx` | `mlx-rs` | 该特质在 Metal 上的实现。只这一箱 import `mlx` |
| `voice-asr` | `voice-tensor` | 梅尔、AuT 编码器、Qwen3 解码器、流式状态、贪心解码 |
| `voice-ffi` | `voice-asr` + `voice-mlx` | C ABI。把 PCM 与回调送进引擎线程 |
| Swift 壳 | 输入法套件、音频 | 调用 C ABI，把整段文字写成预编辑或提交 |

`voice-asr` 的测试用一个记录调用的假后端，不启动 GPU。数值对照只在 `voice-mlx` 里做：同一组权重、同一段波形，Rust 图与已经发表的 MLX 前向在 logits 上对齐。

## 3. 张量运算特质

图允许调用的运算只有这些。特质里没有「跑一段 Python」也没有「加载某个脚本」。

- 创建：零、范围、正态、从 `i16` PCM 提升到 `f32`
- 形状：reshape、transpose、slice、pad、concatenate、where、cumsum、gather
- 元素：加、乘、平方根倒数、指数、正弦、余弦、clip、GELU、SiLU
- 线性：`Linear { in, out, bias }`，权重形状 `(out, in)`
- 卷积：`Conv2d { in_ch, out_ch, kernel: 3, stride: 2, pad: 1 }`，输入 NHWC
- 归一化：`LayerNorm(d)`、`RmsNorm(d, eps)`
- 注意力：缩放点积，允许加法 mask；解码器侧允许写入并读取 KV 缓存
- 归约：沿轴求和、最大、最小。仅用于长度与注入下标，不用于每步解码的数值热路径

4-bit 权重是线性层的一种存储：组大小 64，权重与缩放分开存放。特质的 `Linear` 在求值时反量化到计算 dtype。图的层代码不出现「4-bit」这三个字。

## 4. 0.6B 的具体图

配置取自 Qwen3-ASR 0.6B 的音频塔与文本塔。1.7B 只替换配置，层种类相同。

音频塔：

| 项 | 值 |
| --- | --- |
| 梅尔 | 128 维 |
| 层数 | 18 |
| 头 | 14，每头 `896 / 14 = 64` |
| 前馈 | 3584，GELU |
| `d_model` | 896 |
| 投影输出 | 1024，与文本隐层相同 |
| `n_window` | 50，块长 `n_window * 2 = 100` 个梅尔帧 |
| `n_window_infer` | 800 个梅尔帧，即 8 秒一个注意力窗 |
| 下采样隐层 | 480 |
| 归一化 | LayerNorm，注意力与前馈的线性层都带 bias |

卷积茎三层，都是 `Conv2d(k=3, stride=2, pad=1)` 再 GELU：

1. `1 → 480`
2. `480 → 480`
3. `480 → 480`

128 个梅尔频点三次除以 2 得到 16。通道优先重排后，线性层输入宽度是 `480 * 16 = 7680`，输出 `d_model = 896`。满块 100 帧经过三次 `(n - 1) // 2 + 1` 得到 13 个音频 token。不满一块的尾部要按官方方式先 pad 到块长再卷积，再切回有效 token。直接用尾部自己的宽度做卷积，bias 和 GELU 会让最后一个有效 token 偏移。

位置编码是固定正弦表，分母用 `half_dim - 1`，最大位置 1500。每一块从位置 0 重新加。该表是 `f32`，加到隐层之前要转成隐层的 dtype，否则后面的层会被提升成 `f32`。

每一层编码器：

```text
x = x + Attn(LayerNorm(x))
x = x + Linear2(GELU(Linear1(LayerNorm(x))))
```

注意力是双向的，没有 RoPE。窗与窗之间不互看。窗数达到 20 时按窗切开计算；少于 20 时用一块对角的加法 mask。`float16` 时每层输出 clip 到该类型最大值减 1000，避免溢出。

窗后：`LayerNorm`，然后 `GELU(Linear(d_model → d_model))`，再 `Linear(d_model → 1024)`。

文本塔：

| 项 | 值 |
| --- | --- |
| 词表 | 151936 |
| 隐层 | 1024 |
| 中间层 | 3072 |
| 层数 | 28 |
| 查询头 / 键值头 | 16 / 8，头维 128 |
| 激活 | SiLU，SwiGLU |
| RoPE | 三维 MRoPE，`theta = 1_000_000`，位置编号形状 `(B, 3, L)` |
| RMSNorm | `eps = 1e-6`，线性层无 bias |
| 词嵌入 | 与 `lm_head` 绑同一张表 |

特殊 token：`audio_start = 151669`，`audio_pad = 151676`，`audio_end = 151670`。

一次前向：

1. `input_ids` 含音频占位。嵌入查表得到 `(B, L, 1024)`。
2. 若这是预填充，编码器产出 `(B, N, 1024)`。占位位置用累积和下标换成音频向量，其余位置保留文本嵌入。占位个数不得超过 `N`。
3. 解码器带因果 mask 与 KV 缓存。预填充只把最后一个位置的 logits 留下，形状 `(B, 1, V)`。
4. 之后每步只喂一个 token，`step` 的 `input_ids` 形状 `(B, 1)`，位置 `(B, 3, 1)`。

解码是贪心：每步取 logits 的 argmax。结束于结束符，或达到该次音频允许的最大新 token。语言标签按 Qwen3-ASR 的输出样式放在助手前缀里，中文输入使用 `Chinese`。热词文本放在系统前缀。热词缺席时不写这段前缀，也不写入空串。

## 5. 流式状态

编码器的一个注意力窗只依赖该窗的梅尔帧。已经结束的窗，其 1024 维输出可以留着，不重算。活动窗随着新样本增长而重算。这是流式能改写句尾、又不重跑整段历史的原因。

| 状态 | 内容 |
| --- | --- |
| `pcm_tail` | 不足一个梅尔帧的 `i16` 样本 |
| `mel_pending` | 尚未构成完整窗的梅尔帧 |
| `encoder_prefix` | 已结束各窗的 `(token, 1024)` 拼接 |
| `kv` | 28 层解码器缓存，只含稳定前缀 |
| `unfixed` | 最近 5 个 token。下一编码步允许替换它们 |
| `published` | 上一轮交给 Swift 的整段 UTF-8 |

输入每增加满 2.0 秒（32000 个 16 kHz 样本）才跑一次编码步。步内：更新活动窗，拼上 `encoder_prefix`，从「已发布文本去掉未固定后缀」处继续解码，得到新的整段。新整段若与 `published` 不同，回调一次，`is_final = 0`。

松开时不再等待 2 秒。余下样本全部变成梅尔，活动窗算完，未固定后缀允许被替换，然后解码到结束符。回调 `is_final = 1`。没有可识别语音时，整段为空串，仍然回调。

30 秒以上的音频按窗提交旧的编码器输出，再解新窗。返回给 Swift 的仍是拼好的整段，不是窗内片段。

## 6. 进程内线程与 ABI

引擎线程启动时创建 MLX 的 GPU stream，并在该线程上加载权重与 `metallib`。Swift 音频回调只把 `i16` 样本推进一个有界队列。引擎线程出队后调用 `feed`。禁止在音频线程上碰 `Array`。

```c
typedef void (*voice_text_fn)(uint64_t session, const char *utf8, int is_final, void *ctx);

void *voice_engine_new(const char *model_dir, const char *metallib);
void voice_engine_free(void *engine);
int voice_session_begin(void *engine, uint64_t session, const char *hotwords_or_null);
int voice_session_feed(void *engine, uint64_t session, const int16_t *pcm, size_t n);
int voice_session_finish(void *engine, uint64_t session);
void voice_engine_set_callback(void *engine, voice_text_fn fn, void *ctx);
```

`model_dir` 与 `metallib` 由 Swift 从 `Bundle.main` 的资源目录传入。返回码 `0` 表示已接受。模型目录打不开时 `voice_engine_new` 返回空指针，Swift 不创建会话。

Swift 在按下时 `begin`，在采集回调里 `feed`，在松开时 `finish`。部分回调在主队列执行 `setMarkedText`。定稿非空时在主队列执行 `insertText`。定稿为空时只清除本会话留下的预编辑，不调用 `insertText`。

## 7. 安装包

```text
RimeClient.app
  Contents/MacOS/Squirrel
  Contents/Frameworks/librime 及其非系统依赖
  Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/
      config.json
      model.safetensors
      tokenizer.json
  Contents/Resources/mlx.metallib
```

`Squirrel` 这个可执行文件静态链接 `voice-ffi`。包内没有 `.py`，没有虚拟环境，没有指向用户主目录或仓库目录的解释器路径。权重文件随包分发。着色器首次编译的缓存可以落在用户缓存目录，丢失后可以由包内 `metallib` 再生成。

1.7B 若放入同一包，是 `Models/Qwen3-ASR-1.7B-4bit/`。会话开始时读配置选择目录。两套目录共用一个引擎类型。

## 8. 构建

Rust 箱子用 `cargo` 构建，打开 `mlx-rs` 的 `metal` 特性，目标是 `aarch64-apple-darwin`。Swift 包用链接搜索路径找到 `libvoice_ffi.a`。头文件只有 `voice_engine.h`。CI 在同一台 Apple 芯片上做两件对照：假后端的形状测试，以及一段固定波形上 Rust logits 与 MLX 参考实现的最大绝对误差。误差门槛在第一次对照时记入该测试，不在本文里写一个没有测量的数。

## 9. 删掉的东西

- `worker.py`、JSON 行、Base64 PCM、`/usr/bin/python3`、`.venv`
- 由 `#filePath` 推导解释器位置
- 在音频线程或主线程上调用 MLX

输入法控制器里的按键取舍、预编辑与提交、以及不把语音文本送进 librime，保留。它们调用的识别从「启动一个 Python」改成 `voice_engine_*`。

<!-- derivation-complete -->
