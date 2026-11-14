# 057-research-resident-weights：权重与梅尔前端常驻之后，同进程第二次定稿 0.46 秒

测量日：2026-11-14。对象是 `code/voice-engine/voice-mlx` 的 `Engine`（常驻实例）、
`TextDecoder` 的层与词嵌入表、`code/voice-engine/voice-ffi` 的引擎线程（`Worker::start`
与 `backend::boot`）。基线来自 `056`：同一探针 `finish` 12.534 秒，`voice_engine_new`
0.035 秒。探针仍是 `code/voice-oracle/assets/probe_pcm.bin`（2.00 秒、32000 样本），
模型与 metallib 取自已安装包
`~/Library/Input Methods/RimeClient.app/Contents/Resources`。

## 0. 结论速览

| 项 | 基线（056） | 现在 | 说明 |
| --- | ---: | ---: | --- |
| `voice_engine_new` | 0.035 s | **4.908 s** | 权重与梅尔前端在这里打开一次 |
| `finish` 第一次（冷） | 12.534 s | **0.807 s** | 启动后第一次定稿 |
| `finish` 第二次（暖） | 12.534 s | **0.466 s** | 同进程再按一次，不再读盘、不再反量化 |
| 冷起整条路（new + 第一次 finish） | 12.569 s | 5.715 s | 读盘与反量化从每次定稿挪到启动一次 |
| token 序列 | `[106287, 1773, 151645]` | `[106287, 1773, 151645]` | 逐位相同 |
| 文本 | `嗯。` | `嗯。` | 相同 |

两次 `finish` 的口径：同一个进程、同一台引擎、同一段探针，先跑会话 1（冷），
紧接着跑会话 2 与 3（暖）。三个数分别来自 `/tmp/voice-abi/bench2`，它对
`libvoice_ffi.a` 直接链接，与 `bench` 同一份计时方式（`CLOCK_MONOTONIC`）。

预热之后三轮重复：`engine_new` 4.822–4.908 s、冷 `finish` 0.490–0.807 s、暖 `finish`
0.462–0.469 s。冷 `finish` 那一位在 0.49–1.20 s 之间浮动，第一次读 513 MB 之后
操作系统页缓存与 GPU 侧的页都不是热的；暖的那一位稳定在 0.46 s 上下。

## 1. 改动点

改动只在 `code/voice-engine` 里，`code/voice-ffi` 走同一条路径。Python 没有进产品
路径，`inject` 与 librime 没有动。

| 位置 | 改动 |
| --- | --- |
| `voice-ffi/src/backend.rs` | `boot` 在 `MlxBackend::prepare` 之后调用 `decoder::boot_resident`，把常驻实例打开；打不开就当引擎没准备好 |
| `voice-ffi/src/decoder.rs` | 引擎线程上 `thread_local!` 的 `RESIDENT` 槽持有 `voice_mlx::Engine`；`WeightDecoder` 只借用它，不再自己开权重 |
| `voice-ffi/src/engine.rs` | `WeightDecoder::new()` 不再带模型目录：权重不在会话里打开 |
| `voice-mlx/src/backend/transcribe.rs` | 新增 `Engine`（梅尔前端 + 音频塔 + 文本解码器 + 分词器）与 `Opened`/`transcribe_with_opened`；`transcribe`/`transcribe_with` 签名保留，内部现开现用，供对照 |
| `voice-mlx/src/backend/text_decoder.rs` | `TextDecoder::open` 一次装好 28 层、末尾 RMSNorm 与词嵌入表；`run_layers`/`final_norm`/`embed_*`/`head_logits` 只借用 |
| `voice-mlx/src/backend/weights.rs` | 新增 `affine_table`：整张分组权重按行分块反量化成常驻表；删掉按需回文件取行的 `affine_rows`/`read_rows` |
| `voice-mlx/src/backend.rs` | 新增 `MlxBackend::take_rows`：从常驻表按行取子表 |

三处「不再发生」的固定开销：

1. `weights.safetensors`（537,626,967 字节）只读一次，读完之后文件句柄就关掉；
2. 8-bit 的音频塔与 4-bit 的 28 层文本塔只反量化一次；
3. `MelFrontend::new()`（201×400 滤波器组与 400×201 的 cos/sin 表）只建一次。

常驻的东西与逐次变化的东西分开了：`Engine` 只带不随会话变的四件，会话仍带 PCM、
提示、KV 缓存与统计。`TextDecoder` 里分块取 `lm_head` 的口径没有变，只是行的来源
从文件换成了常驻表；`take_rows` 取出来的行与整表切片逐位相同，所以 logits 与
基线一致。

## 2. 两次实测

`/tmp/voice-abi/bench2`（同 `bench` 的计时方式，加了两轮循环）：

```
engine_new 4.908 s（含常驻权重）
reply {"decoded":true,"kind":"final","ok":true,"session":1,"text":"嗯。"}
cold finish 0.807 s status=0
reply {"decoded":true,"kind":"final","ok":true,"session":2,"text":"嗯。"}
warm finish 0.466 s status=0
reply {"decoded":true,"kind":"final","ok":true,"session":3,"text":"嗯。"}
warm finish 0.463 s status=0
```

同一份归档下 `/usr/bin/time -l` 的峰值：`maximum resident set size` 323 MB、
`peak memory footprint` 2.92 GB。基线同一探针是 314 MB / 2.07 GB：常驻把 f16 的
28 层（约 820 MB）与词嵌入表（151936×1024 f16，约 311 MB）留在了内存里，峰值多出
约 0.85 GB。这是常驻的代价，与精度无关：两者都是同一份 f16 数值。

## 3. 不读盘的直接验证

`/tmp/voice-abi/bench3` 把模型目录复制到 `/tmp/voice-resident-model`，启动后写一个
标记文件并等 6 秒；脚本看到标记就把 `weights.safetensors` 挪走，然后才跑两次定稿：

```
== 权重文件已移走（537626967 字节） ==
engine_new 5.369 s
reply {"decoded":true,"kind":"final","ok":true,"session":1,"text":"嗯。"}
cold finish 1.196 s status=0 error=[]
reply {"decoded":true,"kind":"final","ok":true,"session":2,"text":"嗯。"}
warm finish 0.463 s status=0 error=[]
```

权重文件已经不在，两次定稿照常出「嗯。」。这条闸证明第二次及以后既不读盘也不
反量化；只把加载挪到别处、暗地里还读文件的做法过不了这一条。

## 4. token 与文本

`voice-mlx` 的端到端对照（`cargo test -p voice-mlx --features metal --test
end_to_end_parity`，`--test-threads=2`）在改动后的归档上仍是四条全过：

```
梅尔帧 200、音频 token 26、提示 44、新生成 3（Some(ImEnd)）
期望（3 个）：[106287, 1773, 151645]
实际（3 个）：[106287, 1773, 151645]
参考文本 "嗯。<|im_end|>"（去标记后 "嗯。"），实际文本 "嗯。"
```

逐层数值也复核了一遍：`--test decoder_parity --test-threads=1` 五条全过，预填充
logits 前 64 维最大差 3.125e-2（容差 8.203125e-2），全词表 argmax 106287，与 054
记的同一条闸一致。常驻没有改变任何一步的算法，只改变了实例什么时候打开。

`voice-ffi` 的三条口也各跑了一次（`/tmp/voice-abi/check2`）：空音频定稿回空串且
`status=0`；带热词「专名」的探针回「嗯。」；同进程第二次探针回「嗯。」，
`voice_engine_last_error` 为空。

## 5. 已知边界

1. **对照测试要限并发。** 四条端到端测试各持一份常驻实例，四线程并行时峰值超过
   本机 16 GB，进程会被 MLX 掐掉（SIGTRAP）。`--test-threads=2` 或 1 全过。
2. **启动那 4.9 秒落在 `voice_engine_new` 上。** 这一次读盘与反量化没有消失，只是
   从每次定稿挪到了引擎线程起来的那一刻；真正让它消失的是把打包权重与量化矩阵乘
   接进前向（056 第 2 节第 2 条），那一条不在本次改动里。

<!-- research-complete -->
