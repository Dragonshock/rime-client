# 056-research-latency-baseline-and-model-options：先量时延，再谈换模型

测量日：2026-11-14。对象是已安装包 `~/Library/Input Methods/RimeClient.app` 与 `code/voice-engine` 的 metal 归档。
用户要求：语音转文字尽量没有延迟，调研有哪些开源模型、性能如何，首选 Qwen，研究如何集成。

## 1. 实测：时延不在模型上

用一个直接链 `libvoice_ffi.a` 的 C 程序（`/tmp/voice-abi/bench.c`，不在 `code/` 内）逐段计时，喂 `code/voice-oracle/assets/probe_pcm.bin`（2.00 秒、32000 样本、一个整块）：

| 阶段 | 墙钟 |
| --- | ---: |
| `voice_engine_new`（建 GPU stream、检查 metallib） | 0.035 s |
| `voice_session_begin` | 0.003 s |
| `voice_session_feed`（32000 样本） | 0.000 s |
| `voice_session_finish` | **12.534 s** |
| 合计 | 12.572 s |

同一份代码在 2 秒音频上的实时倍率（RTF）约 **6.3**。用户说的「很慢」与这个数一致：说两秒，等十几秒。

原因的代码位置很直接：

```rust
// 源：code/voice-engine/voice-mlx/src/backend/transcribe.rs:140,153
let tower = AudioTower::open(model_dir, dtype)?;
...
let mut decoder = TextDecoder::open(model_dir, dtype)?;
```

`transcribe_with` 在**每一次调用**里重新打开 513MB 的 `weights.safetensors`，把 8-bit / 4-bit 分组权重反量化成 f16 再交给 MLX。这个动作不随音频变长而摊薄，所以每按一次右 Option 都要付一遍。`feed` 之所以是 0.000 秒，是因为它只把样本排进队列，真正的计算全压在 `finish`。

因此：**换模型不会解决这件事**。把模型换成小一倍，这十几秒里绝大部分仍然花在重复读盘与反量化上。

## 2. 该改的是常驻，不是模型

四处改动按收益排序：

1. **权重常驻。** `AudioTower` 与 `TextDecoder` 在 `voice_engine_new` 时各打开一次，之后所有会话共用。这一条预计把 `finish` 从 12.5 秒降到秒级以下，是唯一决定性的改动。
2. **保留 4-bit 打包权重，用 MLX 的量化矩阵乘**，不要先反量化成 f16 再算。反量化把 513MB 摊成约 1.5GB 的 f16 常驻，且每次都要算一遍。
3. **`lm_head` 只算需要的行。** 词表 151936 × 1024，f32 下是 622MB；每步解码都全量算会单独吃掉一秒以上。按块取 argmax（现有的 8192 行分块已经是这个思路）应当常驻成量化表。
4. **`MelFrontend::new()` 提到引擎创建时。** 现在每次 `transcribe` 都重建滤波器组与旋转因子表；那是 201×400 的三角表，重建一次不贵，但没有理由每次都建。

流式部分另算：现在的 `feed` 只排队、2 秒块满才整段重来，句尾还会整段重解码。真正的「边说边出字」要把已完成的编码窗留在 `encoder_prefix`，每个新块只编码活动窗并增量解码。这件事在 `042` 第 5 节已经写进设计，代码里还没实现。

## 3. 模型选项

事实来自 `015`、`016`、`024`、`026`，这里只收与延迟有关的结论。

| 候选 | 流式原生 | 与现在这套的关系 | 延迟特征 |
| --- | --- | --- | --- |
| Qwen3-ASR 0.6B（现在用的） | 论文与 vLLM 后端支持；MLX 上需要自己做块与窗 | 已打通，权重与本仓库图形都对齐过 | 修完常驻之后，2 秒块的成本主要是编码 26 个 token 与解码若干 token |
| Qwen3-ASR 0.6B 8-bit | 同上 | 换仓库路径即可 | 权重更大、反量化更贵，常驻之后差别在精度与内存 |
| Qwen3-ASR 1.7B | 同上 | 同图换配置 | 更慢，用于离线整段更合适 |
| sherpa-onnx 的流式 Zipformer / SenseVoice | 是，按帧出字 | 另一套运行时（C API），不经 MLX | 首字明显更早，但中文准确率与 Qwen3-ASR 不是一个量级；`015` 记了各自的测试集，不能并排比 |
| Voxtral Realtime | 是，`feed`/`step` | 已在 `018` 作为「帧还在到达」那行的候选 | 流式友好，但构造参数里没有热词与上下文 |

结论：**首选仍是 Qwen3-ASR 0.6B**。它的延迟问题出在我们这边（重复加载），不在模型；换成流式原生的 sherpa-onnx 能更早出字，但要接受另一套运行时与另一档准确率，而且会推翻已拍板的「只走 feed_audio」那一条。

## 4. 集成路径

分两步，互不阻塞：

1. **常驻改造**（不动模型、不动协议）：引擎持有一份 `AudioTower` + `TextDecoder` + `MelFrontend` + 量化 `lm_head`；`transcribe` 只做 梅尔 → 编码 → 预填充 → 循环。出口是同一个探针的 `finish` 从 12.5 秒降到 1 秒以内，且 `probe` 的 token 序列仍是 `[106287, 1773, 151645]`。
2. **流式出字**（不动模型）：把已完成的编码窗缓存起来，每满 2 秒只编码活动窗、增量解码，把结果作为 `kind:"partial"` 交给 Swift 的 `show(preedit:)`。出口是说话过程中屏幕上持续出现文字，而 `finish` 只补最后一段。

第一条不做完，第二条的收益会被重复加载吃掉，所以顺序不能反。

## 检索

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | 读解码入口 | `voice-mlx/src/backend/transcribe.rs` | 每次调用重建 tower 与 decoder |
| 2 | 计时探针 | `/tmp/voice-abi/bench`（新归档） | load 0.035s、finish 12.534s |
| 3 | 同探针文本 | 同上 | `嗯。`，与参考一致 |
| 4 | 读模型列表 | `notes/015` | Qwen3-ASR 0.6B/1.7B、Whisper、Paraformer、SenseVoice、Zipformer |
| 5 | 读文本塔速度 | `notes/016` | M4 Max 4-bit：0.6B 356 tok/s、4B 129 tok/s |
| 6 | 读到达块时延 | `notes/024` | 0.6B 4-bit 块中位 0.100–0.140 s |
| 7 | 读流式缺口 | `notes/026` | 修订指针与其余路径时延仍未知 |
| 8 | 读设计里的流式节 | `notes/042` 第 5 节 | 已完成窗缓存、活动窗重算 |

<!-- research-complete -->
