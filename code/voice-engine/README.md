# voice-engine

Rust 语音引擎的 Cargo 工作区。键盘组合仍由 librime 完成。

`voice-tensor` 定义张量运算特质、形状、数据类型，以及 Qwen3-ASR 0.6B 的配置常量。识别图只通过这些运算调用后端。`FakeBackend` 不计算数值，只记录每次调用的形状，供 `voice-asr` 的测试使用。

`voice-tensor` 不依赖 `mlx-rs`。

`voice-asr` 持有编码器、文本塔与流式状态。测试用 `FakeBackend` 驱动一次 `feed` 和 `finish`，不启动 GPU。

`voice-ffi` 是静态库，导出 `voice-ffi/include/voice_engine.h` 里的 C 符号。引擎线程收到 `feed` 与 `finish` 后调用 `voice-asr`。打开 `metal` 时 `voice-mlx` 链接 MLX，链接参数写在 `BUILD.txt`。

`voice-mlx` 的 `AudioTower` / `encode_mel` 从 `weights.safetensors` 读出 `audio_tower.*` 的真实权重（卷积与 LayerNorm 是 f16，线性层是 8 位分组权重，组大小 64），按 042 第 4 节跑完音频塔。三项中间量与 `code/voice-oracle/assets` 的对照在 `voice-mlx/tests/encoder_parity.rs`，差值记在 `dev-docs/notes/053-research-encoder-parity.md`。

`voice-parity` 只做测量：用 std 读参考资产、逐元素报差值，不链接 MLX，也不被产品 crate 依赖。

在本目录执行：

```text
cargo test --workspace                     # 不编 MLX
cargo test -p voice-mlx --features metal --target aarch64-apple-darwin   # 编码器三闸，链接参数见 BUILD.txt
```
