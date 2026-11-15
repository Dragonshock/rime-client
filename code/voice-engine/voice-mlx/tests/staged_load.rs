//! 分两段打开：先只装梅尔与音频塔，之后再装文本塔。
//!
//! 三件事分开断言，都是分段带来的新位：
//!
//! 1. 只装第一段时，定稿报「文本塔还没装好」——不返回空串，也不假装成功；
//! 2. 第二段装完之后，token 序列与文本与一次装完（`Engine::open`）逐位相同；
//! 3. 第二段重入是空的（`Ok(false)`）：`begin` 之后与 `finish` 之前各叫一次，
//!    只付一次钱。
//!
//! 顺带把两段的墙钟打出来（`--nocapture` 才有）。数只打，不当闸：闸是上面三条。
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
//!   --test staged_load -- --nocapture --test-threads=1
//! ```

#![cfg(feature = "metal")]

use std::path::PathBuf;
use std::time::Instant;

use voice_mlx::{Engine, MlxBackend};
use voice_parity::{asset_root, compare_tokens, read_pcm_i16, Reference};
use voice_tensor::DType;

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

fn pcm() -> Vec<i16> {
    let bytes = std::fs::read(asset_root().join("probe_pcm.bin")).expect("probe_pcm.bin");
    read_pcm_i16(&bytes)
}

#[test]
fn text_tower_waits_for_the_second_stage() {
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
    MlxBackend::prepare(metal.to_str().expect("metallib 路径")).expect("MLX stream");
    let mut ops = MlxBackend::on_this_thread().expect("stream 在本线程");
    let path = dir.to_str().expect("模型目录");
    let samples = pcm();

    // 第一段：梅尔与音频塔。
    let clock = Instant::now();
    let mut engine = Engine::open_audio(path, DType::F16).expect("第一段");
    let first = clock.elapsed().as_secs_f64();
    assert!(!engine.text_ready(), "第一段之后文本塔不该在位");

    // 文本塔没装好时定稿报错，不返回空串。
    let refused = engine.transcribe(&mut ops, &samples);
    assert!(
        refused.is_err(),
        "文本塔没装好时定稿应报错，实得 {:?}",
        refused.map(|result| result.text)
    );
    let reason = refused.expect_err("应报错").to_string();
    assert!(reason.contains("文本塔"), "报错应点到文本塔：{reason}");
    assert!(!engine.text_ready());

    // 第二段：文本塔。
    let clock = Instant::now();
    assert!(engine.load_text().expect("第二段"));
    let second = clock.elapsed().as_secs_f64();
    assert!(engine.text_ready());
    assert!(!engine.load_text().expect("重入"), "重入应当什么都不做");

    // 分段装完的识别结果。
    let clock = Instant::now();
    let staged = engine.transcribe(&mut ops, &samples).expect("定稿");
    let decode = clock.elapsed().as_secs_f64();

    // 一次装完的同一条路。
    let clock = Instant::now();
    let mut whole = Engine::open(path, DType::F16).expect("一次装完");
    let both = clock.elapsed().as_secs_f64();
    let one_shot = whole
        .transcribe(&mut ops, &samples)
        .expect("一次装完的定稿");

    // 两段里的固定开销分开量：梅尔前端在第一段，分词器在第二段。
    let clock = Instant::now();
    let mel = voice_mel::MelFrontend::new();
    let mel_cost = clock.elapsed().as_secs_f64();
    drop(mel);
    let clock = Instant::now();
    let tokenizer = voice_tokenizer::Tokenizer::load(&dir).expect("分词器");
    let tokenizer_cost = clock.elapsed().as_secs_f64();
    drop(tokenizer);

    println!(
        "第一段（梅尔+分词器+音频塔）{first:.3} s｜第二段（文本塔）{second:.3} s｜两段和 {:.3} s｜一次装完 {both:.3} s",
        first + second
    );
    println!(
        "第一段拆开：梅尔前端 {mel_cost:.3} s，音频塔（含表头与收尾 eval）{:.3} s",
        first - mel_cost
    );
    println!(
        "第二段拆开：分词器 {tokenizer_cost:.3} s，文本塔（含表头与收尾 eval）{:.3} s",
        second - tokenizer_cost
    );
    println!(
        "分段装完的定稿 {decode:.3} s：文本 {:?} tokens {:?}",
        staged.text, staged.tokens
    );

    assert_eq!(
        staged.tokens, one_shot.tokens,
        "分段装完与一次装完的 token 应逐位相同"
    );
    assert_eq!(
        staged.text, one_shot.text,
        "分段装完与一次装完的文本应逐位相同"
    );
    // 基线：参考 manifest 的贪心三位。
    let reference = Reference::load().expect("资产");
    let report = compare_tokens("staged_load_3", &reference.greedy_tokens(), &staged.tokens);
    println!("{report}");
    assert!(report.matches(), "{report}");
    assert_eq!(staged.text, "嗯。");
}
