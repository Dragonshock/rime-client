# 051-research-oracle-dump：参考实现的逐层量已导出

测量日：2026-11-14。对象是 `code/voice-oracle/dump_reference.py` 与它写出的
`code/voice-oracle/assets/`。参考实现是 `code/voice-worker/.venv` 里的
`mlx_qwen3_asr` 0.4.4，模型目录是
`~/.cache/huggingface/hub/models--moona3k--mlx-qwen3-asr-0.6b-4bit/snapshots/4c59c533f95c84afb796655e814709034f826f04`。
参考只作对照标准，不进安装包，也没有任何产品代码路径调用它。

## 0. 结论速览

- 六项要的量全部拿到，`manifest.json` 的 `unavailable` 是空数组。拿得到的原因是三处抓取点都在参考实现的正常调用链上：卷积茎是 `AudioEncoder._apply_conv_stem` 方法，第 0 层是 `layers[0]` 这个模块，预填充 logits 由 `Qwen3ASRModel.prefill` 直接返回。
- 每项都是 f32 小端 `.bin`，键名、形状、dtype、来源函数名写在 `manifest.json` 的 `tensors` 里。
- 两次连续导出逐字节相同（六个张量 md5 全等）；`--wav`、`--pcm` 两条入口与内置探针走同一段代码，三者的六个资产逐字节相同。
- 梅尔导出路径与 050 已入库的 `code/voice-engine/voice-mel/assets/reference_mel.bin` 逐元素差为 0.0，说明这里的梅尔和 050 对照用的那份是同一支管线算出来的。
- 贪心前 3 个 token 是 `[106287, 1773, 151645]`，参考自带的 `generate_with_info` 会把末尾 EOS 剥掉，回 `[106287, 1773]`，两者前两位一致。

## 1. 探针输入

不带参数时脚本自己造输入，不读磁盘、不依赖随机数：

| 项 | 值 |
| --- | --- |
| 采样率 | 16000 Hz |
| 声道 | 1 |
| 采样格式 | i16 小端 |
| 长度 | 32000 采样 = 2.000 s |
| 公式 | `sample = (i * 37) % 2001 - 1000` |
| 落盘 | `assets/probe_pcm.bin`（裸 i16 小端，64000 字节，不带 wav 头） |
| 转浮点 | `pcm.astype(f32) / 32768`，与 `mlx_qwen3_asr.load_audio` 的整数 PCM 路径一致 |

读回校验过：`probe_pcm.bin` 的 32000 个采样与公式逐点相等，取值范围 -1000…1000。

## 2. 导出了什么、每个量的形状

2 秒音频 → 200 个梅尔帧 → 2 个 100 帧块 → 26 个编码器 token；提示 44 个 token（`language Chinese`，上下文为空）。

| key | 形状 | dtype | 来源函数名 | 说明 |
| --- | --- | --- | --- | --- |
| `mel` | `(128, 200)` | f32 | `mlx_qwen3_asr.audio.log_mel_spectrogram` | 转换前的 f32 对数梅尔谱 |
| `conv_stem_out` | `(16, 13, 480)` | f32 | `AudioEncoder._apply_conv_stem` | 首次调用的第 0 块，布局是参考的 NHWC（频域 16、时域 13、480 通道）；整次调用的输出形状记在 `full_call_output_shape`，本次是 `[2, 16, 13, 480]` |
| `encoder_layer0_out` | `(1, 26, 896)` | f32 | `AudioEncoderLayer.__call__`（`layers[0]`） | 第 0 层含残差的输出，送进第 1 层 |
| `encoder_proj_out` | `(26, 1024)` | f32 | `AudioEncoder._encode_single` 末尾 `ln_post` → `gelu(proj1)` → `proj2` | 编码器末尾投影，也就是注入文本嵌入的那份音频特征 |
| `prefill_logits_64` | `(64,)` | f32 | `Qwen3ASRModel.prefill` → `lm_head[:, -1:, :64]` | 最后一个提示位置的 logits 前 64 维（全词表 151936 维） |
| `greedy_tokens_3` | `(3,)` | f32 | `generate._sample`（temperature 0 走 `argmax`）＋ `Qwen3ASRModel.prefill`/`step` | 贪心解码前 3 个 token，用 f32 存整数：`106287`、`1773`、`151645` |

`manifest.json` 里另外记了这些上下文，方便复算时不猜：模型目录与激活 dtype、梅尔帧数 200、编码器 token 数 26、`encoder_output_lens`、完整提示的 44 个 token id、贪心结果与 `decode` 出来的文本、以及参考 `generate_with_info` 的交叉核对结果。

## 3. 抓取点：每处是怎么拿到的

| 量 | 抓取方式 | 原接口 |
| --- | --- | --- |
| 梅尔 | 直接调 `compute_features` / `log_mel_spectrogram` | 公开函数 |
| 卷积茎 | 临时替换 `model.audio_tower` 实例上的 `_apply_conv_stem`，抓 `out[0]` 后立刻还原 | 私有方法，但正常前向每次都经过它 |
| 第 0 层 | 临时替换 `AudioEncoderLayer.__call__` 类方法，用 `self is layers[0]` 判定，抓完还原 | Python 的 `layer(x)` 走类型上的 `__call__`，替换实例属性拦不住，所以换类方法 |
| 编码器末尾投影 | 直接用 `audio_tower(...)` 的返回值 | 公开返回值 |
| 预填充 logits | 直接调 `model.prefill(...)`，返回 `(1, 1, 151936)`，取 `[0, -1, :64]` | 公开方法 |
| 贪心 3 token | 与 `generate_with_info` 同路：预填充取一次 `argmax`，之后 `model.step` 逐 token | 公开方法＋`generate` 模块里的私有 `_sample` |

两处替换都在 `try/finally` 里还原，进程退出前不会再被调用。若哪天参考实现把 `_apply_conv_stem` 改名或删掉，脚本不会编造数据：它把 `conv_stem_out` 连同原因写进 `unavailable`，其余量照常导出。

## 4. 口径差异（如实记，不含阈值）

这几条不是缺陷，是「参考量的定义」本身，下游对照时必须按同一口径比：

1. **梅尔存 f32，编码器吃 fp16。** 参考里 `log_mel_spectrogram` 输出 f32，送进编码器前 `mel.astype(dtype)` 转成模型 dtype。这里落盘的是转换前的 f32 值，因为 Rust 的 `voice-mel` 是 f32 前端。若要比对「编码器吃到的梅尔」，要自己再做一次 fp16 截断。
2. **激活是 fp16，权重是 4 bit 量化（`bits=4`、`group_size=64`、`audio_tower_bits=8`）。** 导出的所有激活取自这个量化后的图，不是 bf16 原始权重的前向。所以这些资产是「这个包能产出的参考」，不是「理想全精度参考」。
3. **卷积茎只落第 0 块。** 参考一次前向把 `n_full_chunks` 个块批处理，输出 `(n_chunks, 16, 13, 480)`；这里只存 `[0]`，因为块的数值不随块数变化，而整份落盘要 24 MB 以上。整次调用的形状另存在 `full_call_output_shape` 里。
4. **第 0 层在长音频时只落窗口 0。** 窗口化注意力下 `layers[0]` 按窗口分次调用；本次 2 秒音频只有一个窗口，`covers_full_sequence` 为 `true`，形状就是全序列 `(1, 26, 896)`。换成超过 8 秒的音频，这个字段会变 `false`，存的只是第一个窗口的片段，`total_encoder_tokens` 会写全序列 token 数。
5. **贪心 token 用 f32 存整数**，符合「每项都是 f32 小端」这条；读出来当整数用即可，3 个值都远小于 2^24，f32 表示无损。

## 5. 运行方式

```text
code/voice-worker/.venv/bin/python code/voice-oracle/dump_reference.py
code/voice-worker/.venv/bin/python code/voice-oracle/dump_reference.py --wav /tmp/a.wav
code/voice-worker/.venv/bin/python code/voice-oracle/dump_reference.py --pcm /tmp/a.bin --out /tmp/ref
```

- `--wav` 只吃 16 kHz、单声道、16 位未压缩 PCM；不吃别的就报错退出，脚本不做重采样，也不悄悄改采样率。
- `--pcm` 吃裸 i16 小端 16 kHz 单声道。
- `--out` 指定资产目录；给了 `--wav`/`--pcm` 时产物同名，输入 PCM 存成 `input_pcm.bin`，探针那条存成 `probe_pcm.bin`。
- `--model` 显式指定模型目录，否则看 `VOICE_MODEL_DIR`，再否则扫 HF 缓存快照。
- `--dtype` 默认 `float16`（`load_model` 的默认值），可切 `float32` 出全精度参考。
- `--language` 默认 `Chinese`（与 `code/voice-worker/worker.py` 的 `init_streaming` 默认一致），传 `none` 表示提示尾部不写语言。
- `--context` 写系统提示里的上下文串，默认空。

实测耗时：单次导出约 1.2 秒（含加载 4 bit 权重与分词器）。

## 6. 自检

| 检查 | 做法 | 结果 |
| --- | --- | --- |
| 探针符合公式 | 读回 `probe_pcm.bin` 与 `(i * 37) % 2001 - 1000` 逐点比 | 32000 点全等 |
| 确定性 | 连跑两次，比六个 `.bin` 的 md5 | 全等 |
| 三条入口等价 | `--wav`（由探针造的 wav）、`--pcm`（探针裸 PCM）、无参数三条各导一次，`cmp` 六个资产 | 全部逐字节相同 |
| 梅尔管线与 050 一致 | 用 050 的 `reference_pcm.bin`（8000 采样）跑参考 `log_mel_spectrogram`，与 `reference_mel.bin` 逐元素比 | 最大差 0.0，形状 `(128, 50)` |
| 尾部块路径 | 用 1.37 s（21920 采样、137 帧）的 wav 导一次 | 137 帧 → 1 个满块 + 1 个尾块 → 18 token，形状随之变成 `(1, 18, 896)` 与 `(18, 1024)`，不报错 |
| 值是否健康 | 逐项看 min/max 与 NaN/Inf | 无 NaN 无 Inf；`encoder_proj_out` 在 ±0.13 量级，`encoder_layer0_out` 在 -2.9…+6.6 量级，`prefill_logits_64` 在 11.6…21.0 量级 |
| 贪心交叉核对 | 自走 `argmax` 三次 vs 参考 `generate_with_info(max_new_tokens=3)` | 前两位一致；参考剥掉末尾 `151645`（EOS），`matches_reference_generate` 为 `true` |

`prefill_logits_64` 只是 64 维切片，不能用来判断「下一 token 是哪个」——第一个 token 是 `106287`，它的下标远在 64 之外，要靠全词表 `argmax` 才看得到。这条切片只用于比对这些维度的数值。

## 7. 边界

- 这些资产只作对照，不进安装包，`code/voice-engine` 的产品路径不读它们；`code/voice-worker/.venv` 与 Python 同样不进安装包。
- 本脚本没有改 `dev-docs/refs`，也没有改 `code/voice-engine` 与 `code/voice-worker` 的任何源文件。
- 参考量到手不等于 Rust 侧已经对上。`voice-mlx/src/backend/finish.rs` 目前仍是那条只读几个层、只解一个 token 的形状骨架（050 里记过），逐层对照要等后端把权重真正送进各层之后再做。

## 检索

| # | 动作 | 地址 | 结果 |
| --- | --- | --- | --- |
| 1 | 读参考前向结构 | `mlx_qwen3_asr/model.py`、`encoder.py` | `prefill`/`step` 返回 logits；`_encode_single` 是编码器全流程 |
| 2 | 读梅尔前端 | `mlx_qwen3_asr/audio.py` | `n_fft=400`、`hop=160`、功率谱、丢最后一帧 |
| 3 | 读整数 PCM 归一 | `audio.py::_normalize_integer_pcm` | i16 除以 32768 |
| 4 | 读提示构造 | `mlx_qwen3_asr/tokenizer.py::build_prompt_tokens` | 44 token（含 26 个 `<\|audio_pad\|>`） |
| 5 | 读贪心路径 | `generate.py::_sample`、`_build_decode_positions`、`_finalize_generation_result` | temperature 0 走 argmax；末尾 EOS 被剥掉 |
| 6 | 读量化配置 | `quantization_config.json` | `bits=4`、`group_size=64`、`audio_tower_bits=8` |
| 7 | 写导出脚本 | `code/voice-oracle/dump_reference.py` | 六项全拿到，`unavailable` 为空 |
| 8 | 核对梅尔口径 | 050 的 `reference_pcm.bin` / `reference_mel.bin` | 最大差 0.0 |
| 9 | 跑三种入口 | 探针 / `--wav` / `--pcm` | 六个资产逐字节相同 |

<!-- research-complete -->
