# 046-derivation-bundle-voice-assets

已安装的输入法包缺三样语音分发物：`Contents/Resources/mlx.metallib`、`Contents/Resources/Models/Qwen3-ASR-0.6B-4bit`，以及带已定义 `voice_engine_*` 符号的主程序。`mlx.metallib` 已经在 `code/voice-engine/target` 的 `mlx-sys` 构建输出里。0.6B 4-bit 权重在本机 Hugging Face 缓存，快照条目是指向 blob 的符号链接。`voice-ffi` 的 `UnlinkedDecoder` 在没有数值时返回空串。空定稿按现有规则不调用 `commit`。识别时延：未知。

这次没有改 `/Library/Input Methods/Squirrel.app`，没有调用 inject，没有写剪贴板，也没有把 `/usr/bin/python3` 写回脚本。

## 1. 安装包里没有的三样

两份包的主程序时间戳都是 2026-11-13 00:53:05：

| 包 | 主程序 |
| --- | --- |
| `/Users/dragon/Library/Input Methods/RimeClient.app` | `Contents/MacOS/Squirrel` |
| `code/Client/dist/Squirrel.app` | `Contents/MacOS/Squirrel` |

两边的 `Contents/Resources` 都只有 `worker.py`、`rime.pdf`、`entitlements.plist`、`en.lproj`、`zh-Hans.lproj`。没有 `mlx.metallib`。没有 `Models`。没有 `Qwen3-ASR-0.6B-4bit`。

`nm -gU` 与 `nm -gu` 在已安装主程序上都没有 `voice_engine`。二进制里仍有供查找的 C 字符串 `voice_engine_new`、`voice_engine_free`、`voice_engine_set_callback`。那是字符串，不是已定义符号。`044` 记过同一情况。`code/Client/dist/Squirrel.app` 的 `nm -gU` 同样没有 `voice_engine`。

`/Library/Input Methods/Squirrel.app` 的 Resources 下也没有这份 `mlx.metallib`，也没有 Qwen3 目录。本文不覆盖那个包。

`VoiceEngine.bundledMetallib()` 在资源缺失时仍返回包内应有的 `mlx.metallib`。`VoiceEngine.bundledModelDirectory()` 在资源缺失时仍返回包内应有的 `Models/Qwen3-ASR-0.6B-4bit`。路径存在与文件存在不是同一件事。

## 2. 调试主程序已有符号，但还没打进包

`code/Client/.build/out/Products/Debug/Squirrel` 是 Mach-O 64-bit executable arm64，时间 2026-11-13 02:09:40，比上面两份包新。`nm -gU` 有已定义符号：

| 符号 | 类型 |
| --- | --- |
| `_voice_engine_free` | T |
| `_voice_engine_new` | T |
| `_voice_engine_set_callback` | T |
| `_voice_session_begin` | T |
| `_voice_session_feed` | T |
| `_voice_session_finish` | T |

`otool -L` 列出 `Metal.framework` 与 `Accelerate.framework`。同一份二进制里还有 `voice_mlx` 的符号。`make-app.sh` 的主程序来源就是这份调试产物。当前 dist 与已安装包是更早一次打包，没有换成它，所以安装包里的主程序仍然没有这些已定义符号。

## 3. metallib

普通文件，不是符号链接：

`code/voice-engine/target/aarch64-apple-darwin/debug/build/mlx-sys-6cd75aa1123af3a9/out/build/lib/mlx.metallib`

183870464 字节。`code/voice-engine/target` 下只有这一份 `mlx.metallib`。`BUILD.txt` 写的是同一路径。改脚本之前，`make-app.sh` 不复制它。

## 4. 本机权重快照

仓库：`/Users/dragon/.cache/huggingface/hub/models--moona3k--mlx-qwen3-asr-0.6b-4bit`。`refs/main` 是 `4c59c533f95c84afb796655e814709034f826f04`。快照目录：

`snapshots/4c59c533f95c84afb796655e814709034f826f04`

六个条目都是指向 `../../blobs/` 的符号链接。`lstat` 只有几十字节，那是链接本身。解开之后的字节：

| 名称 | 真实字节 |
| --- | --- |
| `config.json` | 6193 |
| `quantization_config.json` | 60 |
| `tokenizer_config.json` | 12487 |
| `vocab.json` | 2776833 |
| `merges.txt` | 1671853 |
| `weights.safetensors` | 537626967 |

目录内普通文件合计 542094393 字节。权重文件是 `weights.safetensors`，537626967 字节。分词相关文件是 `tokenizer_config.json`、`vocab.json`、`merges.txt`。快照里没有 `tokenizer.json`，也没有 `model.safetensors`。`042` 的包内示意用过那两个名字；本机这份快照的文件名以上表为准。

复制进包必须解开符号链接。包内应是普通文件，不能再指向用户缓存里的 blob。

## 5. 空定稿不调用 commit

`code/voice-engine/voice-ffi/src/decoder.rs` 的 `UnlinkedDecoder` 注释写明：没有 Metal 数值时不编造识别文本，logits 还没有数值后端，整段文本保持空串。`decode_text` 不读 `DecodeView`，返回 `text: String::new()` 和空的 token 列表。

`code/voice-engine/voice-ffi/src/engine.rs` 的 `Worker::start` 把这个解码器交给每个会话。`voice-ffi` 的 `metal` 特性会链入 `voice-mlx`，但 `start` 的工厂仍然是 `UnlinkedDecoder`。

`code/VoiceInterface/Sources/VoiceInterface/Session.swift` 的 `acceptFinal`：文本为空时记下 `finalText`，会话关闭。若本会话还留着非空预编辑，只追加一条空的 `show` 用来清缓存。这条分支不调用 `deliver`，因此不产生 `.commit`。`VoiceHandleGate` 只在收到 `.commit` 时调用 `voiceCommit`。空定稿不会走到 `commit(string:)`。非空定稿才记录改写并 `deliver` 成 `commit`。

识别时延：未知。这次没有从麦克风测到文字的墙钟。空串定稿不是识别耗时。`044` 里假后端 `feed_32000_then_finish` 的 0.02 秒也不拿来充当识别时延。

## 6. 打包脚本

`code/Client/scripts/make-app.sh` 在组装 `Contents/Resources` 时增加两步：

- 从 `code/voice-engine/target` 里 `mlx-sys` 的 `out/build/lib/mlx.metallib` 复制到 `Contents/Resources/mlx.metallib`。
- 从上面的 Hugging Face 快照复制到 `Contents/Resources/Models/Qwen3-ASR-0.6B-4bit`。`cp -RL` 跟随符号链接，写入普通文件。复制后检查 `weights.safetensors`、`config.json`、`tokenizer_config.json`、`vocab.json`、`merges.txt` 都在，并且目录里不再有符号链接。

脚本里收集 dylib 的那段仍调用 `python3`，没有改成 `/usr/bin/python3`。

## 检索

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | 列资源 | 已安装 `RimeClient.app`、`code/Client/dist/Squirrel.app` | 无 `mlx.metallib`，无 `Models` |
| 2 | `nm -gU` / `nm -gu` | 已安装主程序、dist 主程序 | 无 `voice_engine` 符号 |
| 3 | `nm -gU`、`otool -L` | `code/Client/.build/out/Products/Debug/Squirrel` | 六个 `voice_engine_*` 已定义；链接 Metal 与 Accelerate |
| 4 | `find`、`stat` | `code/voice-engine/target` 下 `mlx.metallib` | 一份，183870464 字节 |
| 5 | `ls -la`、`stat -L` | Hugging Face 快照 `4c59c533…` | 六条符号链接；`weights.safetensors` 537626967 字节 |
| 6 | 读源码 | `decoder.rs`、`engine.rs`、`Session.swift`、`VoiceEngine.swift`、`make-app.sh` | 空串解码器；空定稿不 `commit`；脚本原先不复制这两样资源 |
| 7 | 读笔记 | `042`、`044`、`045`，`BUILD.txt` | 包内路径、已安装包缺符号、Metal 构建产物路径 |

<!-- derivation-complete -->
