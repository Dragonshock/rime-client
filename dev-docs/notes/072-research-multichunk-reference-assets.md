# 072-research-multichunk-reference-assets：多块音频的参考资产

测量日：2026-11-15。参考实现是 `code/voice-worker/.venv` 里的 `mlx_qwen3_asr 0.4.4`（只作对照标准）。
本轮只做一件事：把 071 里那两个「3 / 4 个整块」的切片也导成逐层参考量，供后续多块对照使用。
旧的 200 帧资产（`code/voice-oracle/assets/*.bin` 与根 `manifest.json`）一个字节都没动。

## 1. 导出脚本的扩展

`code/voice-oracle/dump_reference.py` 新增一条路径：

```sh
code/voice-worker/.venv/bin/python code/voice-oracle/dump_reference.py --multichunk
```

- 默认对 `assets/zh/s1_3.0.pcm`、`assets/zh/s1_4.0.pcm` 各导一套量（`--segments` 可改段名，`--segments-dir` 可改音频目录）。
- 每段写到 `assets/multichunk/<段名>/`，各自带一份扁平的 `manifest.json`（key、形状、dtype、字节数、来源函数、说明）。
- 另在 `assets/multichunk/manifest.json` 写一份汇总：两段的 `tensors`（含 `key`/`shape`/`dtype`/`source`）、`audio_frames`、`greedy`，以及 `audio_tokens` 的实测值。
- 模型只加载一次，两段共用（`dump(..., runtime=...)`）。
- 卷积茎在这条路径上落**整批**（第 0 维是整块号），因为多块之间的差别正是要看的东西；默认探针/pcm 路径仍是老样子（只落第 0 块 `(16, 13, 480)`），manifest 里用 `conv_stem_layout` 标出是 `all_chunks` 还是 `chunk0_only`。
- 抓卷积茎改成「每次 `_apply_conv_stem` 调用都留一份」，`all_stem_call_shapes` 记录本段发生过几次调用及其形状（这两段都只有一次调用，没有尾块）。

## 2. 导了什么、形状

两段的梅尔帧数都是整块数（300 / 400），没有尾块。

| 项 | s1_3.0 形状 | s1_4.0 形状 | dtype | 来源 |
| --- | --- | --- | --- | --- |
| `mel` | (128, 300) | (128, 400) | float32 | `mlx_qwen3_asr.audio.log_mel_spectrogram` |
| `conv_stem_out` | (3, 16, 13, 480) | (4, 16, 13, 480) | float32 | `AudioEncoder._apply_conv_stem`（整批，第 0 维是块号） |
| `encoder_layer0_out` | (1, 39, 896) | (1, 52, 896) | float32 | `AudioEncoderLayer.__call__`（`layers[0]`，含残差） |
| `encoder_proj_out` | (39, 1024) | (52, 1024) | float32 | `AudioEncoder._encode_single` → `ln_post`/`gelu(proj1)`/`proj2` |
| `prefill_logits_64` | (64,) | (64,) | float32 | `Qwen3ASRModel.prefill` → `lm_head[:, -1:, :64]` |
| `greedy_tokens_3` | (3,) | (3,) | float32（存整数） | `generate._sample`(argmax) + `prefill`/`step` |
| `input_pcm` | (48000,) | (64000,) | int16 | 输入音频原样落盘，便于复算 |

`conv_stem_out` 取 `[0]` 就是旧 200 帧资产里的 `(16, 13, 480)`（同一次调用的第 0 块），所以新老资产可以按块对照。
`encoder_layer0_out` 的 `covers_full_sequence` 两项都是 `true`：39 / 52 个 token 都装得下同一个注意力窗口（见 §4），第 0 层只被调用一次，mask 为 `None`。

## 3. 音频 token 数（实测）

`bench`/manifest 实测：

| 切片 | 采样 | 时长 | 梅尔帧 | 整块数 | 音频 token 数 | 参考文本（本轮复核） |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| s1_2.0（已有，仅复核） | 32000 | 2.000 s | 200 | 2 | 26 | 今天下午三点开会。 |
| s1_3.0 | 48000 | 3.000 s | 300 | 3 | **39** | 今天下午三点开会，请提前。 |
| s1_4.0 | 64000 | 4.000 s | 400 | 4 | **52** | 今天下午三点开会，请提前十分钟到会。 |

即 **audioTokens300 = 39，audioTokens400 = 52**，每整块 13 个 token，与 `_get_feat_extract_output_lengths` 的公式一致。
三个 token 数对应的参考文本也与任务里的期望逐字相同，说明这两份资产确实来自正确的音频。

两段的贪心前 3 个 token 都是 `[100644, 102172, 112055]`（解出「今天下午三点」），与参考 `generate_with_info` 的前 3 个 token 一致（manifest 里 `matches_reference_generate: true`）。

## 4. 两条会影响对照方式的事实

**(a) 单窗口，不能拿窗口边界解释截断。** 配置是 `n_window = 50`（整块 100 梅尔帧）、`n_window_infer = 800`（注意力窗口 800 帧 → 13 × 8 = 104 个 token）。39 与 52 都远小于 104，所以这两段整段就是一个注意力窗口，`_create_windowed_mask` 返回 `None`，第 0 层输出覆盖全部 token。也就是说 3.0 s / 4.0 s 被截断与前缀窗口、尾块、窗口拼接都无关。

**(b) 同一份梅尔，编码 39 个 token 与只编码前 26 个 token，前 26 个 token 的结果就不一样。**
拿 300 帧那份梅尔，比较「整段编码」与「只编码前 200 帧」（`audio_tower.encode_frames(mel[:, :200])`）：

- 前者 `(39, 1024)`，后者 `(26, 1024)`。
- 前 26 个 token 上 `max|d| = 0.0935`，而该量级自身的 `max|v| = 0.0881`——差值和数值同量级。
- 末 token（第 25 个，第 1 块的最后一格）差得最多（0.0935），块内越靠前差得越小（第 0 个 token 0.0111）。

原因是同一窗口内后一块的 token 会进注意力分母。推论是：如果引擎只跑了前 2 块，那么 `encoder_proj_out` 的前 26 个 token 就已经对不上参考，**不必等解码**就能把缺陷定位到编码器之前/之内；`conv_stem_out` 的第 0 块则应该仍然吻合（它只依赖第 0 块的 100 帧梅尔）。

## 5. 跨长度复用梅尔前缀时的两个坑（实测）

`s1_3.0.pcm` 就是 `s1.pcm` 的前 48000 采样，`s1_4.0.pcm` 就是前 64000 采样（逐采样相同）。但把两段梅尔的前 300 帧相减，`max|d| = 0.0757`，38400 个点里有 7687 个超 1e-6。原因有两条，都是参考实现本身的性质：

1. **全局 8 dB 钳位**：`log_mel_spectrogram` 里是 `log_spec = max(log_spec, max(log_spec) - 8.0)`，`max` 取的是整段音频的全局最大值。300 帧那份归一化后全局最大 1.4255，400 帧那份 1.4345，钳位底因此差 0.009，所有被钳住的点都跟着变。7687 个差异点里 98.3% 至少有一侧正处在钳位底上。
2. **末帧的反射填充**：STFT 两端各补 `nperseg // 2 = 200` 采样，补法依赖音频在哪里结束。所以 300 帧那份的第 299 帧（末帧）与 400 帧那份的同下标帧不同，恰好 128 个点（一帧 128 个梅尔格）差值 0.0757；前 299 帧里没有任何「非钳位」的差异点。

实践含义：拿不同的切片长度互相对照时，不能假设梅尔前缀可复用；要在同一份梅尔上比。§4(b) 的实验就是这么做的。

## 6. 旧的 200 帧资产没动

- 导出前后对 `assets/*.bin` 与 `assets/manifest.json` 做 `shasum -a 256`，7 个文件全部 `OK`（同一份校验清单）。
- 还跑了一遍默认路径 `dump_reference.py --out /tmp/probe-recheck`，7 个文件与库里的旧资产**逐字节相同**（`cmp` 全部通过），说明抓取逻辑的改动没有改变数值。
- 因此 encoder 3 / decoder 6 / end_to_end 4 这几条闸的输入（`assets` 根下的 200 帧资产）与之前完全一样，闸的结果不会因本轮改动变化。

另：`code/.gitignore` 里原有 `assets/` 一条会把 `voice-oracle/assets` 下的新资产一并挡掉，加了一行 `!voice-oracle/assets/`，只放行 `voice-oracle` 这个对照资产目录；模型权重、`mlx.metallib`、`.venv`、构建目录仍然被忽略。

## 7. 复现

```sh
# 多块参考资产（3 / 4 个整块）
code/voice-worker/.venv/bin/python code/voice-oracle/dump_reference.py --multichunk

# 只导一段、导到别处
code/voice-worker/.venv/bin/python code/voice-oracle/dump_reference.py \
    --multichunk --segments s1_4.0 --out /tmp/mc

# 老的 200 帧探针路径
code/voice-worker/.venv/bin/python code/voice-oracle/dump_reference.py
```

## 检索

| # | 动作 | 对象 | 结果 |
| --- | --- | --- | --- |
| 1 | 读参考 `AudioEncoder._encode_single` | `encoder.py` | 整块 100 帧、每块独立过卷积茎、按块给位置编码、窗口化注意力（`n_window_infer = 800` 帧） |
| 2 | 扩脚本 | `dump_reference.py --multichunk` | 每段 6 项 + 输入 PCM，各自 manifest，另写汇总 manifest |
| 3 | 导出并打印 token 数 | `assets/zh/s1_3.0`、`s1_4.0` | 梅尔 300 / 400 帧 → token 39 / 52 |
| 4 | 复核参考文本 | 同上（`generate_with_info`） | 与任务期望逐字相同，资产对应正确音频 |
| 5 | 同一份 300 帧梅尔：整体 vs 只前 200 帧 | `audio_tower.encode_frames` | 前 26 token `max|d| = 0.0935`（值量级 0.0881），缺陷在解码前就可见 |
| 6 | 跨长度比梅尔前缀 | `mel.bin` 两份 | 7687/38400 点不同：98.3% 来自全局 8 dB 钳位，余下 128 点全在末帧（反射填充） |
| 7 | 旧资产校验 | `assets/*.bin`、`manifest.json` | sha256 全部 OK，默认路径重跑逐字节相同 |

<!-- research-complete -->
