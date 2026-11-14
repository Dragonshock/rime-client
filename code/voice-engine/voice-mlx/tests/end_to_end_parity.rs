//! 端到端对照：一段 `probe_pcm.bin` 走完梅尔 → 编码器 → 预填充 →
//! 逐 token 贪心 → 分词器，与 `greedy_tokens_3` 和 `manifest.greedy.text_from_tokens` 比。
//!
//! 三项对照：
//!
//! 1. 提示：按参考模板拼出的 44 个 id 与 `manifest.prompt.tokens` 逐位相同；
//! 2. token 序列：`voice_mlx::transcribe` 的贪心结果与 `greedy_tokens_3` 逐位相同，
//!    闸用 `voice_parity::compare_tokens`，不过时把实际与期望两串一起打出来；
//! 3. 文本：去掉特殊 token 字面量之后与参考 `text_from_tokens` 去标记的结果相同。
//!
//! 前两项是实算的，没有硬编码实际值。
//!
//! 跑法（链接参数见 `encoder_parity.rs` 文件头，三条缺一不可）：
//!
//! ```text
//! export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
//! export LIBRARY_PATH="$SDKROOT/usr/lib"
//! export MLX_SYS_SYSTEM_SDK=1
//! export RUSTFLAGS="-C link-arg=-framework -C link-arg=Accelerate \
//!   -C link-arg=-Wl,-U,_MTLTensorDomain -C link-arg=-Wl,-U,_MTLLogStateErrorDomain \
//!   -C link-arg=-Wl,-U,_NSDeviceCertificationiPhonePerformanceGaming \
//!   -C link-arg=-Wl,-U,_NSProcessInfoPerformanceProfileDidChangeNotification \
//!   -C link-arg=-Wl,-U,_NSProcessPerformanceProfileDefault \
//!   -C link-arg=-Wl,-U,_NSProcessPerformanceProfileSustained \
//!   -C link-arg=-Wl,-U,___fmaxf16 -C link-arg=-Wl,-U,___fminf16"
//! cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
//!   --test end_to_end_parity -- --nocapture
//! ```

#![cfg(feature = "metal")]

use std::path::PathBuf;

use voice_mlx::{prompt_tokens, transcribe, MlxBackend, StopReason, PROMPT_LANGUAGE};
use voice_parity::{asset_root, compare_tokens, read_pcm_i16, Reference};

/// 参考那次对照的语言与上下文（`manifest.prompt`）。
const CONTEXT: &str = "";

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

fn reference() -> Reference {
    Reference::load().unwrap_or_else(|error| {
        panic!(
            "载入参考资产失败：{error}（目录：{}）",
            asset_root().display()
        )
    })
}

fn prepare() -> (PathBuf, Reference) {
    let metal = metallib();
    assert!(
        metal.is_file(),
        "metallib 不在 {}；可用 VOICE_METALLIB 覆盖",
        metal.display()
    );
    let dir = model_dir();
    assert!(
        dir.join("weights.safetensors").is_file(),
        "权重不在 {}；可用 VOICE_MODEL_DIR 覆盖",
        dir.display()
    );
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    (dir, reference())
}

/// 参考 `manifest.greedy.text_from_tokens` 里的特殊 token 字面量。
/// 产品路径交出去的文本应当没有它们。
fn strip_special_literals(text: &str) -> String {
    let mut out = text.to_string();
    for literal in ["<|im_end|>", "<|endoftext|>", "<asr_text>", "<|audio_start|>", "<|audio_end|>"] {
        out = out.replace(literal, "");
    }
    out
}

/// 参考的音频 token 数（`manifest.audio_frames.n_audio_tokens`）。
fn reference_audio_tokens(reference: &Reference) -> usize {
    reference
        .audio_frames()
        .and_then(|value| value.get("n_audio_tokens"))
        .and_then(|value| value.as_usize())
        .expect("manifest.audio_frames.n_audio_tokens")
}

/// 第一步：分词器按参考模板拼出的提示与参考提示逐位相同。
///
/// 这一项只用到分词器与模型目录，不跑数值前向；放在这里是因为它决定后面
/// 预填充吃的到底是哪个提示。
#[test]
fn prompt_matches_reference() {
    let (dir, reference) = prepare();
    let audio_tokens = reference_audio_tokens(&reference);
    let actual = prompt_tokens(
        dir.to_str().expect("模型目录"),
        audio_tokens,
        Some(PROMPT_LANGUAGE),
        CONTEXT,
    )
    .expect("拼提示");
    let expected = reference.prompt_tokens();
    let report = compare_tokens("prompt.tokens", &expected, &actual);
    println!("{report}");
    assert!(report.matches(), "{report}");
    assert_eq!(actual.len(), 44, "提示应为 44 个 token");
    assert_eq!(
        actual.iter().filter(|id| **id == 151_676).count(),
        audio_tokens,
        "音频占位个数应与音频 token 数一致"
    );
}

/// 第二步与第三步：贪心 token 序列与文本。
///
/// 一次 `transcribe` 里跑完梅尔、编码器、预填充与逐 token 循环；序列不通过时
/// 报告里两串都在，方便看是从第几位开始分叉。
#[test]
fn greedy_sequence_and_text_match_reference() {
    let (dir, reference) = prepare();
    let bytes = std::fs::read(asset_root().join("probe_pcm.bin")).expect("probe_pcm.bin");
    let pcm = read_pcm_i16(&bytes);
    assert_eq!(pcm.len(), 32_000, "探针应是 32000 个采样");

    let started = std::time::Instant::now();
    let result = transcribe(dir.to_str().expect("模型目录"), &pcm).expect("端到端识别");
    let elapsed = started.elapsed();
    println!(
        "梅尔帧 {}、音频 token {}、提示 {}、新生成 {}（{:?}），墙钟 {:?}",
        result.mel_frames,
        result.audio_tokens,
        result.prompt_tokens,
        result.tokens.len(),
        result.stopped_by,
        elapsed
    );

    let expected = reference.greedy_tokens();
    let report = compare_tokens("greedy_tokens_3", &expected, &result.tokens);
    println!("{report}");
    assert!(report.matches(), "{report}");
    assert_eq!(
        result.stopped_by,
        Some(StopReason::ImEnd),
        "参考第三位是 151645，贪心应停在 im_end"
    );
    assert_eq!(result.mel_frames, 200, "参考梅尔是 200 帧");
    assert_eq!(
        result.audio_tokens,
        reference_audio_tokens(&reference),
        "编码器吐出的音频 token 数应与参考一致"
    );
    assert_eq!(result.prompt_tokens, reference.prompt_tokens().len());

    let reference_text = reference
        .manifest()
        .get("greedy")
        .and_then(|value| value.get("text_from_tokens"))
        .and_then(|value| value.as_str())
        .expect("manifest.greedy.text_from_tokens");
    let expected_text = strip_special_literals(reference_text);
    println!(
        "参考文本 {:?}（去标记后 {:?}），实际文本 {:?}",
        reference_text, expected_text, result.text
    );
    assert_eq!(result.text, expected_text, "去标记后的文本应相同");
    assert!(
        !result.text.contains("<|") && !result.text.contains("<asr_text>"),
        "提交文本里不应出现特殊 token 字面量：{:?}",
        result.text
    );
}

/// 空音频：不跑前向，回空串与空 token。
#[test]
fn empty_audio_gives_empty_result() {
    let (dir, _reference) = prepare();
    let result = transcribe(dir.to_str().expect("模型目录"), &[]).expect("空音频");
    assert_eq!(result.tokens, Vec::<u32>::new());
    assert_eq!(result.text, "");
    assert_eq!(result.mel_frames, 0);
}

/// KV 缓存那一步与「把新 token 拼回提示重算一遍」应当给出同一个 argmax。
///
/// 这条只碰解码器：音频特征直接用参考的 `encoder_proj_out`，把编码器的差排除。
/// 它挡的是位置编号或注意力遮罩写错——单步没有因果遮罩、位置从提示长度继续，
/// 这两处一旦写错，缓存路径与重算路径就会分叉。
#[test]
fn cached_step_matches_a_fresh_prefill() {
    let (dir, reference) = prepare();
    let prompt = reference.prompt_tokens();
    let audio = reference
        .tensor("encoder_proj_out")
        .expect("encoder_proj_out")
        .values
        .clone();
    let mut decoder =
        voice_mlx::TextDecoder::open(dir.to_str().expect("模型目录"), voice_tensor::DType::F16)
            .expect("解码器权重");
    let mut backend = MlxBackend::on_this_thread().expect("stream 在本线程");

    let one = decoder
        .generate(&mut backend, &prompt, &audio, 1)
        .expect("先生成一个 token");
    assert_eq!(one.tokens.len(), 1);
    let mut extended = prompt.clone();
    extended.extend_from_slice(&one.tokens);
    let fresh = decoder
        .prefill(&mut backend, &extended, &audio, 1)
        .expect("重算扩展提示");

    let two = decoder
        .generate(&mut backend, &prompt, &audio, 2)
        .expect("生成两个 token");
    println!(
        "缓存第二步 {}，重算扩展提示的 argmax {}",
        two.tokens[1], fresh.argmax
    );
    assert_eq!(
        two.tokens[1], fresh.argmax as u32,
        "缓存路径的第二个 token 应与重算扩展提示的 argmax 相同"
    );
}
