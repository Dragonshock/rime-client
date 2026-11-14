//! 分段计时：打开一次，逐段跑一次探针，把每一段的墙钟打出来。
//!
//! 这条不是对照闸，默认不跑（`#[ignore]`）：它只打印数，断言只保证每一段真的
//! 跑出了东西。跑法：
//!
//! ```text
//! cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
//!   --test phase_timing -- --ignored --nocapture --test-threads=1
//! ```
//!
//! 链接参数见 `encoder_parity.rs` 文件头。注意这里是 `dev` 档案（与本仓产品归档
//! 同档），梅尔前端与采样转换都在主机上，优化关闭时的绝对值比发布档案高。

#![cfg(feature = "metal")]

use std::path::PathBuf;
use std::time::Instant;

use voice_mlx::{encode_mel, AudioTower, MlxBackend, QuantPolicy, TextDecoder};
use voice_mel::MelFrontend;
use voice_parity::{asset_root, read_pcm_i16, Reference};
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

#[test]
#[ignore = "只打印分段墙钟，不是对照闸"]
fn phases_of_one_transcription() {
    let metal = metallib();
    let dir = model_dir();
    let bytes = std::fs::read(asset_root().join("probe_pcm.bin")).expect("probe_pcm.bin");
    let pcm = read_pcm_i16(&bytes);
    let reference = Reference::load().expect("资产");
    let tokens = reference.prompt_tokens();

    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let mut ops = MlxBackend::on_this_thread().expect("stream 在本线程");

    let clock = Instant::now();
    let tower = AudioTower::open(dir.to_str().expect("模型目录"), DType::F16).expect("音频塔权重");
    let tower_open = clock.elapsed().as_secs_f64();
    // 口径可以用环境变量换：`VOICE_QUANT_POLICY=dequantized` 是 057 的算术
    // （七个投影先拼 f16 再乘），用来把「量化矩阵乘」这一段单独量出来。
    let policy = match std::env::var("VOICE_QUANT_POLICY").as_deref() {
        Ok("dequantized") => QuantPolicy::dequantized(),
        _ => QuantPolicy::default(),
    };
    println!("口径：{policy:?}");
    let clock = Instant::now();
    let mut decoder = TextDecoder::open_with(dir.to_str().expect("模型目录"), DType::F16, policy)
        .expect("解码器权重");
    let decoder_open = clock.elapsed().as_secs_f64();

    let mel_front = MelFrontend::new();
    let clock = Instant::now();
    let mel = mel_front.log_mel(&pcm);
    let mel_frames = mel.len() / 128;
    let mel_time = clock.elapsed().as_secs_f64();

    let clock = Instant::now();
    let encoded = encode_mel(&mut ops, &tower, &mel, mel_frames, DType::F16).expect("编码器前向");
    let encode_time = clock.elapsed().as_secs_f64();

    let clock = Instant::now();
    let logits = decoder
        .prefill(&mut ops, &tokens, &encoded.proj, 8)
        .expect("预填充");
    let prefill_time = clock.elapsed().as_secs_f64();

    let clock = Instant::now();
    let generation = decoder
        .generate(&mut ops, &tokens, &encoded.proj, 8)
        .expect("贪心解码");
    let generate_time = clock.elapsed().as_secs_f64();

    println!("音频塔 open {tower_open:.3} s");
    println!("解码器 open {decoder_open:.3} s");
    println!("梅尔 {mel_time:.3} s（{mel_frames} 帧）");
    println!("编码器 {encode_time:.3} s（{} 个音频 token）", encoded.tokens);
    println!("预填充 {prefill_time:.3} s（{} 个提示 token，argmax {}）", logits.tokens, logits.argmax);
    println!("贪心 {generate_time:.3} s（{} 个新 token）", generation.tokens.len());

    assert_eq!(encoded.tokens, 26, "编码器应吐 26 个音频 token");
    assert_eq!(logits.argmax, 106287, "预填充的 argmax 应是 106287");
    assert_eq!(generation.tokens, vec![106287, 1773, 151645]);
    assert!(tower_open > 0.0 && decoder_open > 0.0 && mel_time > 0.0);
}
