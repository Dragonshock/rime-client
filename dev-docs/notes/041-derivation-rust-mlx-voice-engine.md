# 041-derivation-rust-mlx-voice-engine：Rust 持有 Qwen3-ASR 算子图

本文是一份候选设计。它要的是一套可以放进单个输入法包、换一台 Mac 仍能启动的语音引擎。分发单位里没有解释器，没有虚拟环境，也没有指向编译机源码树的路径。

键盘组合仍由现有的 C++ 输入法引擎负责。这是已经钉下的约束，不是这张算子图的一部分。输入法进程与系统之间的界面仍是输入法套件：按键进来，预编辑与提交出去。

## 1. 语言边界

三件事的变化原因不同，所以不放进同一种语言。

| 边界 | 持有者 | 只做的事 |
| --- | --- | --- |
| 系统界面 | Swift | 输入法会话、麦克风、包路径、把文字交给 `setMarkedText` / `insertText` |
| 语音引擎 | Rust | 采样帧、流式状态、算子图、权重映射、热词上下文 |
| 键盘组合 | 现有 C++ 引擎 | 按键到候选。语音文本不进入这条路径 |

Swift 不实现编码器或解码器。Rust 不实现输入法控制器。两边用一条 C ABI 相连：Rust 编成输入法进程内的静态库，由一个专用线程持有张量运行时。张量对象不越过这根线程。Swift 把 PCM 与控制消息送进信道，文字回调回到主队列。

不另起 Python 进程，也不把音频编成 Base64。进程内信道的音频是长度前缀的原始字节。

## 2. 张量后端与算子图

Qwen3-ASR 是一张算子图，不是某个脚本。图只依赖下面这一组运算：卷积、矩阵乘、归一化、旋转位置编码、分组查询注意力、SwiGLU、以及词嵌入的查表。MLX 是这组运算在 Apple 芯片上的后端。`mlx-rs` 0.34 通过 MLX 的 C API 提供这些运算，并链接 Metal。后端升级是替换这组运算的实现，不是改输入法会话。

图的数据流：

1. 16 kHz 单声道波形。
2. 128 维梅尔谱。与 Qwen3-ASR 所用的 Whisper 特征提取一致：`hop_length` 160。
3. 音频编码器。卷积茎做 8 倍下采样，得到 12.5 Hz 的帧，再经窗口注意力的变换层。0.6B 的编码器约 1.80 亿参数、隐层 896。1.7B 的编码器约 3.00 亿参数、隐层 1024。
4. 线性投影，把编码器隐层对到解码器隐层。
5. 解码器是对应尺寸的 Qwen3。上下文、语言标签和已经稳定的前缀，作为解码器的前缀 token。热词是这段前缀里的文本，不是另一套网络。

权重是 safetensors，放在包内资源目录，由引擎映射进内存。包内同时放入这份图需要的 `mlx.metallib`。首次按下不应再去下载解释器或虚拟环境。着色器缓存可以留在用户缓存目录，那是运行时缓存，不是分发物。

分发的识别尺寸是 0.6B 4-bit 权重。1.7B 是同一张图的另一组权重，引擎按配置文件选择，不另写一套会话。

## 3. 流式状态

一次按住对应一份状态，放在 Rust 里。

| 字段 | 含义 |
| --- | --- |
| 波形尾 | 尚未凑满一个编码步的样本 |
| 编码器缓存 | 已经提交的音频前缀 |
| 解码器 KV 缓存 | 稳定前缀的键值 |
| 未固定后缀 | 最近若干 token。下一次解码可以改写它们 |
| 已公布文本 | 上一轮交给 Swift 的整段字符串 |

编码步长是 2.0 秒的音频增量，与现有流式协议同一量级。未固定后缀沿用「回退 5 个 token、最近若干块不固定」这一规则，使已经显示的句尾可以被改写。松开时把波形尾全部送进编码器，再解到句尾，得到定稿整段。定稿与部分结果都是整段文本，不是增量片段。

Swift 只看到两种回调：部分整段、定稿整段。它不读取 KV 缓存，也不读取未固定 token。

## 4. Swift 与 Rust 的 ABI

```c
/* 候选 ABI。字符串由 Rust 分配，Swift 复制后调用 release。 */
typedef void (*voice_text_fn)(uint64_t session, const char *text, int is_final, void *ctx);

void *voice_engine_new(const char *model_dir);
void voice_engine_free(void *engine);
int voice_session_begin(void *engine, uint64_t session, const char *hotwords_or_null);
int voice_session_feed(void *engine, uint64_t session, const int16_t *pcm, size_t n);
int voice_session_finish(void *engine, uint64_t session);
void voice_engine_set_callback(void *engine, voice_text_fn fn, void *ctx);
void voice_string_release(char *text);
```

`hotwords_or_null` 为 NULL 表示热词缺席，不传空字符串。`feed` 可以在凑满编码步之前返回而不回调。`finish` 必须产生一次 `is_final = 1` 的回调；定稿为空串时也回调，由 Swift 决定不提交。

Swift 在按键按下时 `begin`，在音频回调里 `feed`，在松开时 `finish`。回调里的部分文本调用 `setMarkedText`，定稿非空时调用 `insertText`。这两次调用仍在主队列。

## 5. 包布局

```text
RimeClient.app
  Contents/MacOS/Squirrel
  Contents/Frameworks/librime 及其依赖
  Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/
  Contents/Resources/mlx.metallib
  Contents/Resources/worker 不存在
```

安装包不包含 Python，不包含虚拟环境。引擎通过 `Bundle` 资源路径打开模型目录。

## 6. 与现状的距离

现状把识别放在 Python 进程里，解释器路径由编译机的源文件路径拼出来。那条路径换电脑即失效，也使包内的 `worker.py` 无法独立运行。本设计删掉该进程和该路径。

输入法控制器、预编辑、提交、以及不把语音文本送进键盘组合，这些保持不变。变的是控制器调用的识别实现：从 JSON 行上的 Python，换成进程内的 Rust 算子图。

<!-- derivation-complete -->
