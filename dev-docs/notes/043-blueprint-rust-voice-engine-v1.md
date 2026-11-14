未确认蓝图

# 043-blueprint-rust-voice-engine-v1：Rust 语音引擎的落地参照

本文把 `042` 收成仓库里的目录、箱子和验收。候选。键盘仍走 librime。文字仍经 `setMarkedText` 与 `insertText`。Python 不进入这条构建。

## 目录

```text
code/voice-engine/
  Cargo.toml
  voice-tensor/
  voice-asr/
  voice-mlx/
  voice-ffi/
```

`voice-tensor` 不依赖 `mlx-rs`。`voice-asr` 只依赖 `voice-tensor`。`voice-mlx` 是唯一允许依赖 `mlx-rs` 的箱子，特性 `metal` 默认打开。`voice-ffi` 把引擎线程和 C 符号连起来。

## 验收

1. `cargo test -p voice-tensor -p voice-asr` 在没有 GPU 的假后端上通过。假后端记录每一次线性、卷积和注意力的形状。
2. 0.6B 配置与 `042` 第 4 节的数字一致：编码器 18 层、`d_model` 896、文本 28 层、隐层 1024、查询头 16、键值头 8。
3. 流式状态在假后端上跑通：喂入 32000 个样本产生一次部分回调；`finish` 产生一次 `is_final = 1`，空输入的定稿是空串。
4. `voice-ffi` 导出 `042` 第 6 节的七个符号。
5. Swift 侧新增一个调用这些符号的类型。解释器路径不再启动进程。`voice-ffi` 静态库还没链上时，这个类型返回「引擎未链入」，不回退到 `/usr/bin/python3`。

`mlx-rs` 若在本机编译失败，`voice-mlx` 的失败日志写进 `code/voice-engine/BUILD.txt`。`voice-asr` 的测试不因此关掉。

<!-- blueprint-complete -->
