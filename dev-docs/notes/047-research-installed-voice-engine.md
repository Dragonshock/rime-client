# 047-research-installed-voice-engine：已安装包里的语音引擎

测量日：2026-11-13。安装包是 `~/Library/Input Methods/RimeClient.app`。没有调用 inject，没有写剪贴板，没有改 `/Library/Input Methods/Squirrel.app`。识别时延：未知。这次没有真实麦克风语句，也不把引擎创建的进程耗时写成识别时延。

## 1. 事实

### 1.1 包内文件

`Contents/Resources/mlx.metallib` 与 `Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/weights.safetensors` 都在。模型目录里没有符号链接（`find -type l` 计数为 0）。下表字节来自 `stat`，类型都是普通文件。

| 路径（相对 Contents） | 字节 |
| --- | --- |
| `MacOS/Squirrel` | 39616624 |
| `Resources/mlx.metallib` | 183870464 |
| `Resources/Models/Qwen3-ASR-0.6B-4bit/weights.safetensors` | 537626967 |
| `Resources/Models/Qwen3-ASR-0.6B-4bit/config.json` | 6193 |
| `Resources/Models/Qwen3-ASR-0.6B-4bit/quantization_config.json` | 60 |
| `Resources/Models/Qwen3-ASR-0.6B-4bit/tokenizer_config.json` | 12487 |
| `Resources/Models/Qwen3-ASR-0.6B-4bit/vocab.json` | 2776833 |
| `Resources/Models/Qwen3-ASR-0.6B-4bit/merges.txt` | 1671853 |

`file`：主程序是 Mach-O 64-bit executable arm64；`mlx.metallib` 是 MetalLib executable (MacOS), version 1.2.9；`weights.safetensors` 是 data。权重字节与本机 Hugging Face 快照 `models--moona3k--mlx-qwen3-asr-0.6b-4bit` 解开符号链接后的 `weights.safetensors` 相同，都是 537626967。`046` 记过快照条目本身是指向 blob 的符号链接；打进包之后不再是符号链接。

`codesign --verify --deep --strict` 退出码 0。`codesign -dv`：Identifier `im.dragonshock.inputmethod.RimeClient`，Format 为 app bundle、Mach-O thin、arm64，Signature 为 adhoc，TeamIdentifier 为 not set。

主程序字节搜索：`/usr/bin/python3` 出现 0 次。

### 1.2 符号与 Metal

`nm -gU` 在已安装主程序上看到已定义符号（类型 `T`）：

| 符号 | 地址 |
| --- | --- |
| `_voice_engine_free` | `00000001000a5ce8` |
| `_voice_engine_new` | `00000001000a5d5c` |
| `_voice_engine_set_callback` | `00000001000a60e8` |
| `_voice_session_begin` | `00000001000a618c` |
| `_voice_session_feed` | `00000001000a624c` |
| `_voice_session_finish` | `00000001000a639c` |

`otool -L` 有一行：

`/System/Library/Frameworks/Metal.framework/Versions/A/Metal (compatibility version 1.0.0, current version 383.1.0)`

同一份 `otool -L` 还有 Accelerate.framework。这是已安装主程序，不是只存在于调试目录的那份。

### 1.3 一次 `voice_engine_new`

完成的那一次调用不是在已安装主程序进程里下断，而是把 Squirrel 链接所用的同一份归档 `code/voice-engine/target/aarch64-apple-darwin/debug/libvoice_ffi.a` 链进一个临时程序。链接命令退出码 0，不是链接失败。参数是已安装包里的模型目录和 `mlx.metallib`。

调用前该临时程序自己查过：模型路径是目录且 `opendir` 成功，metallib 是普通文件。随后 `voice_engine_new` 返回非空指针，`voice_engine_free` 返回。结果归类：引擎创建成功。不是「模型目录打不开」，也不是「链接失败」。

另一次尝试是用调试器启动已安装主程序并停在 `main`，准备在那个进程里调用同一个符号。`process launch` 在 25 秒内没有停住，函数没有被执行。那个进程随后被停掉。因此已安装 Mach-O 文本段里的 `_voice_engine_new` 这次没有被调用到；已安装主程序上的证据停在 `nm` 的已定义符号和 `otool` 的 Metal 依赖。临时程序调用的是打进该主程序的同一份静态归档，模型目录与 metallib 用的是包内路径。

这次没有调用 `voice_session_begin`、`voice_session_feed` 或 `voice_session_finish`。没有送入麦克风采样。识别时延：未知。

### 1.4 输入源

安装之后执行了已安装主程序的 `--enable-input-source im.dragonshock.inputmethod.RimeClient.Hans`。标准输出是：

`Enable succeeds for input source: im.dragonshock.inputmethod.RimeClient.Hans`

随后用 `TISCreateInputSourceList(nil, true)` 再读（`true` 表示包含未启用的源）：

| 输入源 | localizedName | enabled | selectable |
| --- | --- | --- | --- |
| `im.dragonshock.inputmethod.RimeClient.Hans` | RimeClient | false | true |
| `im.dragonshock.inputmethod.RimeClient.Hant` | RimeClient Traditional | true | true |
| `im.dragonshock.inputmethod.RimeClient` | RimeClient | true | false |

直接调用 `TISRegisterInputSource` 返回 0，`TISEnableInputSource` 对 Hans 返回 0。紧接着新建的列表里 Hans 的 enabled 仍是 false。`defaults read com.apple.HIToolbox AppleEnabledInputSources` 的文本里没有 `RimeClient`。命令打印成功和列表属性仍为 false，两件事同时成立。

### 1.5 官方 Squirrel

`/Library/Input Methods/Squirrel.app/Contents/MacOS/Squirrel` 在替换用户目录里的 RimeClient 之前与之后，mtime、字节和 inode 都是 `1768354365`、`892640`、`33034978`。进程 96157 仍是该路径，没有被停掉。停掉的是用户目录 `RimeClient.app` 里原先的进程，停掉之后才 `ditto`。

## 2. 机制

`voice_engine_new`（`code/voice-engine/voice-ffi/src/lib.rs`）先把模型目录当成可 `read_dir` 的目录。打不开则返回空指针。这次编译打开了 `metal`：metallib 参数必须指向已存在的文件，否则同样返回空指针。两者都通过之后，`Worker::start`（`engine.rs`）在引擎线程上调用 `boot`。`boot`（`backend.rs`）把 metallib 交给 `MlxBackend::prepare`：设置 metallib 路径，在该线程建立 GPU stream，并用一个长度为 1 的零张量做一次求值。准备失败则 `voice_engine_new` 返回空指针。准备成功则返回指向 `Worker` 的非空指针。这次非空返回说明包内目录能打开、包内 metallib 被当成文件、并且这次 `prepare` 没有失败。

打开 `metal` 时，`Worker::start` 为每个会话构造 `WeightDecoder`，而不是 `UnlinkedDecoder`。`WeightDecoder::decode_text` 只在定稿且累积样本非空时调用 `voice_mlx::transcribe_finish`。该函数打开模型目录里的 `weights.safetensors`，经已有的卷积、线性和注意力读取 `audio_tower.conv2d1`、`audio_tower.conv_out`、第 0 层 `q/k/v`、`audio_tower.proj2` 和 `model.embed_tokens`，再取贪心下标。打不开权重或计算失败时返回错误。`WeightDecoder` 把错误记成失败，回调空串。成功的下标经 `vocab.json` 取词片；`tokenizer_config.json` 里的附加 token 会并进同一张表。词片对不上时同样是空串，并且 `voice_session_finish` 走失败返回码。这次没有调用 `finish`，所以上述定稿路径没有在这次测量里跑过。非空的 `voice_engine_new` 不表示已经得到词片。

## 3. 边界

- 引擎创建成功只覆盖这一次 `voice_engine_new` / `voice_engine_free`。它不覆盖麦克风、定稿词片或上屏。
- 调试器没有在已安装主程序里执行该符号。已安装主程序的符号证据是 `nm` 与 `otool`。
- `TISEnableInputSource` 返回 0 没有把 Hans 的 enabled 读数改成 true。
- 官方 `/Library/Input Methods/Squirrel.app` 的主程序 inode 没有变。
- 没有从这次调用写出识别时延。没有真实麦克风语句。

## 4. 仍未知

- 麦克风到文字的识别时延：未知。
- Hans 在启用调用返回 0 之后，列表属性为何仍是 false：未知。

## 检索

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | `cargo build -p voice-ffi --features metal` | `code/voice-engine` | Finished，0.10 秒，退出码 0 |
| 2 | `swift build --product Squirrel` | `code/Client` | Build complete；调试主程序 `nm` 有 `_voice_engine_new`，`otool` 有 Metal.framework |
| 3 | `make-app.sh` | `code/Client/dist/Squirrel.app` | metallib 183870464 字节；`weights.safetensors` 537626967 字节；模型目录无符号链接 |
| 4 | `--quit` 后 `ditto` | 用户目录 `RimeClient.app` | 只停用户目录进程；官方主程序 inode 不变 |
| 5 | `--enable-input-source` | 已安装主程序 | 打印 Enable succeeds；随后列表里 Hans enabled 仍为 false |
| 6 | `stat`、`find -type l`、`file` | 已安装 Resources | 见 1.1 的字节表；符号链接数 0 |
| 7 | `nm -gU`、`otool -L` | 已安装 `Contents/MacOS/Squirrel` | 六个 `voice_*` 为 `T`；Metal.framework 383.1.0 |
| 8 | `codesign --verify --deep --strict` | 已安装包 | 退出码 0；adhoc |
| 9 | 字节搜索 `/usr/bin/python3` | 已安装主程序 | 0 次 |
| 10 | 链接并调用 `voice_engine_new` | 同一份 `libvoice_ffi.a`，参数为包内模型目录与 metallib | 链接退出码 0；目录能打开；返回非空；`voice_engine_free` 返回 |
| 11 | 调试器 `process launch` | 已安装主程序 | 25 秒内未停在 `main`，没有执行调用 |
| 12 | `TISCreateInputSourceList` 与 `TISEnableInputSource` | Hans / Hant / 父源 | Hans enabled=false；Enable 的 OSStatus 为 0 |
| 13 | `defaults read` AppleEnabledInputSources | `com.apple.HIToolbox` | 文本中无 RimeClient |
| 14 | `stat` 前后对照 | `/Library/Input Methods/Squirrel.app` 主程序 | mtime、字节、inode 未变 |

<!-- research-complete -->
