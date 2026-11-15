//! 真实中文语音的六段验收闸：整段 PCM 一次读进来，文本必须与参考逐字相同。
//!
//! 资产是 `code/voice-oracle/assets/zh/`（071 导出）的六件裸小端 i16、16 kHz 单声道：
//!
//! | 文件 | 时长 | 参考文本 |
//! | --- | ---: | --- |
//! | `s1.pcm` | 4.61 s | 今天下午三点开会，请提前十分钟到会议室。 |
//! | `s1_2.0.pcm` | 2.00 s | 今天下午三点开会。 |
//! | `s1_3.0.pcm` | 3.00 s | 今天下午三点开会，请提前。 |
//! | `s1_4.0.pcm` | 4.00 s | 今天下午三点开会，请提前十分钟到会。 |
//! | `s2.pcm` | 4.55 s | 幫我把這段文字翻譯成英文，然後發到郵箱。 |
//! | `s3.pcm` | 5.09 s | 下周的行程已经确认了吗？如果没有，我再联系一下。 |
//!
//! 三件事在这里同时成立：
//!
//! 1. **文件是整段读进来的。** 期望的梅尔帧数由 `MelFrontend` 对同一份 PCM 现算，
//!    与识别结果自己报的 `mel_frames` 比。少读一段（例如探针只 `fread` 前 32000 个
//!    采样）就会先在这条上显形，不必等文本变短。
//! 2. **音频 token 数按参考的块公式算。** 整块 100 帧给 13 个，尾块给
//!    `ceil(尾帧 / 8)` 个——这是参考 `_get_feat_extract_output_lengths` 的式子，
//!    不是按本仓输出回填的。
//! 3. **文本逐字相同，停在 `<|im_end|>`。** 六段里 2.0 s 是两块，3.0 / 4.0 s 是
//!    三块 / 四块，`s1` / `s2` / `s3` 都是整块加尾块；「只覆盖前两块」那种截断会在
//!    3.0 s 及以上直接撞死在这一条。`s2` 两边都给繁体，那是模型自己的选择。
//!
//! 权重的取法与 `end_to_end_parity.rs` 相同：`VOICE_MODEL_DIR` 覆盖，否则用
//! HuggingFace 缓存里的快照；metallib 用 `VOICE_METALLIB` 覆盖，否则用装包里的那份。
//!
//! 跑法（三条链接参数见 `encoder_parity.rs` 文件头，缺一不可）：
//!
//! ```text
//! export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
//! export LIBRARY_PATH="$SDKROOT/usr/lib"
//! export MLX_SYS_SYSTEM_SDK=1
//! export RUSTFLAGS="-C link-arg=-framework -C link-arg=Accelerate ..."
//! cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
//!   --test multichunk_acceptance -- --nocapture --test-threads=1
//! ```

#![cfg(feature = "metal")]

use std::path::PathBuf;

use voice_mel::MelFrontend;
use voice_mlx::{Engine, MlxBackend, StopReason};
use voice_parity::{asset_root, read_pcm_i16};
use voice_tensor::{DType, CHUNK_FRAMES, MEL_BINS, TOKENS_PER_CHUNK};

/// 六段资产与参考文本。参考文本取自 071 那次对照（`mlx_qwen3_asr 0.4.4`），
/// 不按本仓输出回填。
const CLIPS: [(&str, &str); 6] = [
    ("s1", "今天下午三点开会，请提前十分钟到会议室。"),
    ("s1_2.0", "今天下午三点开会。"),
    ("s1_3.0", "今天下午三点开会，请提前。"),
    ("s1_4.0", "今天下午三点开会，请提前十分钟到会。"),
    ("s2", "幫我把這段文字翻譯成英文，然後發到郵箱。"),
    ("s3", "下周的行程已经确认了吗？如果没有，我再联系一下。"),
];

fn metallib() -> PathBuf {
    if let Some(path) = std::env::var_os("VOICE_METALLIB") {
        return PathBuf::from(path);
    }
    let home = std::env::var("HOME").expect("HOME");
    PathBuf::from(home).join("Library/Input Methods/RimeClient.app/Contents/Resources/mlx.metallib")
}

fn model_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("VOICE_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").expect("HOME");
    let root =
        PathBuf::from(home).join(".cache/huggingface/hub/models--moona3k--mlx-qwen3-asr-0.6b-4bit");
    let rev = std::fs::read_to_string(root.join("refs/main")).expect("快照编号");
    root.join("snapshots").join(rev.trim())
}

/// 参考的音频 token 数：整块 13 个，尾块 `ceil(尾帧 / 8)` 个。
fn expected_tokens(frames: usize) -> usize {
    let full = frames / CHUNK_FRAMES;
    let tail = frames % CHUNK_FRAMES;
    let tail_tokens = if tail == 0 { 0 } else { tail.div_ceil(8) };
    full * TOKENS_PER_CHUNK + tail_tokens
}

/// 一段资产整段读进来。
fn pcm_of(name: &str) -> Vec<i16> {
    let path = asset_root().join("zh").join(format!("{name}.pcm"));
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("读不到 {}：{error}", path.display()));
    let pcm = read_pcm_i16(&bytes);
    assert!(!pcm.is_empty(), "{} 是空的", path.display());
    pcm
}

#[test]
fn six_real_chinese_clips_match_the_reference_text() {
    let metal = metallib();
    assert!(metal.is_file(), "metallib 不在 {}", metal.display());
    let dir = model_dir();
    assert!(
        dir.join("weights.safetensors").is_file(),
        "权重不在 {}；可用 VOICE_MODEL_DIR 覆盖",
        dir.display()
    );
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let mut ops = MlxBackend::on_this_thread().expect("stream 在本线程");
    let frontend = MelFrontend::new();
    let mut engine = Engine::open(dir.to_str().expect("模型目录"), DType::F16).expect("两座塔");

    for (name, want_text) in CLIPS {
        let pcm = pcm_of(name);
        // 期望的梅尔帧数：对同一份 PCM 现算。识别结果报得比这少，就是没吃下整段。
        let frames = frontend.log_mel(&pcm).len() / MEL_BINS;
        let tokens = expected_tokens(frames);
        let result = engine.transcribe(&mut ops, &pcm).expect("识别");
        println!(
            "{name}：样本 {}（{:.2} s），梅尔 {} 帧（现算 {frames}），音频 token {}（参考 {tokens}），提示 {} 个，tokens {:?}，文本 {:?}，停止 {:?}",
            pcm.len(),
            pcm.len() as f64 / 16_000.0,
            result.mel_frames,
            result.audio_tokens,
            result.prompt_tokens,
            result.tokens,
            result.text,
            result.stopped_by
        );
        assert_eq!(
            result.mel_frames, frames,
            "{name}：识别只吃了 {} 帧，整段是 {frames} 帧",
            result.mel_frames
        );
        assert_eq!(
            result.audio_tokens, tokens,
            "{name}：音频 token 数应为 {tokens}（{frames} 帧）"
        );
        assert_eq!(
            result.prompt_tokens,
            result.audio_tokens + 18,
            "{name}：提示长度应是音频占位加 18 个固定 token"
        );
        assert_eq!(result.text, want_text, "{name}：文本应与参考逐字相同");
        assert_eq!(
            result.stopped_by,
            Some(StopReason::ImEnd),
            "{name}：应停在 <|im_end|>"
        );
    }
}
