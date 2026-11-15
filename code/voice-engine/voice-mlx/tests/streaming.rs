//! 流式出字的实测：按 1 秒一块送 4 秒音频，记每一步的墙钟与 token，最后对齐定稿。
//!
//! 这条不是对照闸，默认不跑（`#[ignore]`）：它打印数并断言「定稿与整段重算一致」。
//! 跑法：
//!
//! ```text
//! export VOICE_MODEL_DIR=.../Qwen3-ASR-0.6B-4bit
//! export VOICE_METALLIB=.../mlx.metallib
//! cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
//!   --test streaming -- --ignored --nocapture --test-threads=1
//! ```
//!
//! 链接参数见 `encoder_parity.rs` 文件头。这里是 `dev` 档案，与产品归档同档。

#![cfg(feature = "metal")]

use std::path::PathBuf;
use std::time::Instant;

use voice_mel::MelFrontend;
use voice_mlx::{encode_mel, AudioTower, Engine, MlxBackend, Stream, MAX_NEW_TOKENS, PROMPT_LANGUAGE};
use voice_parity::{asset_root, read_pcm_i16};
use voice_tensor::{DType, MEL_BINS};

/// 一步的边界：输入的编码步是 2.0 秒。
const STEP_SAMPLES: usize = 32_000;
/// 一块送多少样本。探针是 1 秒一块送进来的。
const BLOCK_SAMPLES: usize = 16_000;

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

fn probe() -> Vec<i16> {
    let bytes = std::fs::read(asset_root().join("probe_pcm.bin")).expect("probe_pcm.bin");
    read_pcm_i16(&bytes)
}

fn repeat(pcm: &[i16], times: usize) -> Vec<i16> {
    let mut out = Vec::with_capacity(pcm.len() * times);
    for _ in 0..times {
        out.extend_from_slice(pcm);
    }
    out
}

struct Step {
    /// 送完这一块之后的累计样本数。
    samples: usize,
    /// 这一步返回时的墙钟，从开始送样本算起。
    at: f64,
    step: voice_mlx::StreamStep,
}

/// 按 1 秒一块送，每满 2 秒跑一步。返回每一步、定稿的墙钟与定稿结果。
///
/// `unfixed` 是允许下一步改写的尾巴长度：给 5 是产品口径，给 1 会让提示里出现
/// 已固定的文本（那一段每步都重算，但它在提示里的位置不变）。
fn stream_blocks(
    engine: &mut Engine,
    ops: &mut MlxBackend,
    pcm: &[i16],
    unfixed: usize,
) -> (Vec<Step>, f64, voice_mlx::Transcription) {
    let start = Instant::now();
    let mut stream = Stream::new(unfixed);
    let mut steps = Vec::new();
    let mut fed = 0usize;
    while fed < pcm.len() {
        let count = BLOCK_SAMPLES.min(pcm.len() - fed);
        stream.push(&pcm[fed..fed + count]);
        fed += count;
        if fed % STEP_SAMPLES == 0 && fed < pcm.len() {
            let step = engine
                .stream_step(ops, &mut stream, MAX_NEW_TOKENS, Some(PROMPT_LANGUAGE), "")
                .expect("非定稿步");
            steps.push(Step {
                samples: fed,
                at: start.elapsed().as_secs_f64(),
                step,
            });
        }
    }
    // 定稿：整段重算，与改造前同一条路径。这里量的是它自己的墙钟。
    let clock = Instant::now();
    let final_pass = engine.transcribe(ops, pcm).expect("定稿");
    let finish = clock.elapsed().as_secs_f64();
    println!(
        "定稿 {finish:.3} s：{} 个 token，文本 {:?}，{:?}（{} 个音频 token）",
        final_pass.tokens.len(),
        final_pass.text,
        final_pass.stopped_by,
        final_pass.audio_tokens
    );
    (steps, finish, final_pass)
}

#[test]
#[ignore = "只打印流式实测，不是对照闸"]
fn partials_every_two_seconds_and_the_final_still_matches() {
    let metal = metallib();
    let dir = model_dir();
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let mut ops = MlxBackend::on_this_thread().expect("stream 在本线程");

    let clock = Instant::now();
    let mut engine = Engine::open(dir.to_str().expect("模型目录"), DType::F16).expect("权重");
    println!("引擎 open {:.3} s", clock.elapsed().as_secs_f64());

    let probe = probe();
    let pcm = repeat(&probe, 2);
    println!(
        "输入 {:.2} 秒（{} 个样本，探针重复 2 次），按 {} 个样本一块送",
        pcm.len() as f64 / 16_000.0,
        pcm.len(),
        BLOCK_SAMPLES
    );

    let clock = Instant::now();
    let baseline = engine.transcribe(&mut ops, &pcm).expect("整段重算");
    let baseline_seconds = clock.elapsed().as_secs_f64();
    println!(
        "改造前的形态（整段重算，无部分结果）{baseline_seconds:.3} s：tokens {:?}，文本 {:?}",
        baseline.tokens, baseline.text
    );

    let (steps, finish, final_pass) = stream_blocks(&mut engine, &mut ops, &pcm, 5);
    for step in &steps {
        let inner = step.step.mel_seconds
            + step.step.mel_shape_seconds
            + step.step.encode_seconds
            + step.step.decode_seconds;
        println!(
            "部分 {:.2} 秒处（墙钟 {:.3} s，步内 {:.3} s = 梅尔 {:.3} + 整形 {:.3} + 编码 {:.3} + 解码 {:.3}）\
音频 token {}（前缀 {}，本步冻结 {}，活动窗 {} 帧）提示 {}（重算 {}，沿用 {}，固定 {}）新 {} 个 → {:?} {:?}",
            step.samples as f64 / 16_000.0,
            step.at,
            inner,
            step.step.mel_seconds,
            step.step.mel_shape_seconds,
            step.step.encode_seconds,
            step.step.decode_seconds,
            step.step.audio_tokens,
            step.step.encoder_prefix_tokens,
            step.step.frozen_tokens,
            step.step.active_frames,
            step.step.prompt_tokens,
            step.step.prefill_tokens,
            step.step.reused_tokens,
            step.step.stable_tokens,
            step.step.new_tokens,
            step.step.text,
            step.step.tokens
        );
    }

    // 定稿走一次性那条路，因此与改造前的整段重算逐位相同。
    assert!(!steps.is_empty(), "4 秒音频至少要出一次部分结果");
    assert!(
        steps.iter().all(|step| !step.step.text.is_empty()),
        "每一部分结果都应有文本"
    );
    assert_eq!(
        final_pass.tokens, baseline.tokens,
        "定稿 token 与整段重算必须一致"
    );
    assert_eq!(final_pass.text, baseline.text, "定稿文本与整段重算必须一致");
    assert!(finish > 0.0);

    // 8 秒以上：前缀真的被复用了。第一个 8 秒窗冻结之后，后面的步只编活动窗。
    let long = repeat(&probe, 7);
    let clock = Instant::now();
    let long_baseline = engine.transcribe(&mut ops, &long).expect("整段重算");
    println!(
        "14 秒整段重算 {:.3} s：tokens {:?}，文本 {:?}",
        clock.elapsed().as_secs_f64(),
        long_baseline.tokens,
        long_baseline.text
    );
    let (long_steps, _, long_final) = stream_blocks(&mut engine, &mut ops, &long, 5);
    for step in &long_steps {
        println!(
            "14 秒部分 {:.2} 秒处（墙钟 {:.3} s，步内 {:.3} s = 梅尔 {:.3} + 整形 {:.3} + 编码 {:.3} + 解码 {:.3}）\
预填充 {:.3} s（重算 {} 个位置，沿用 {} 个，提示 {}）\
音频 token {}（前缀 {}，本步冻结 {}，活动窗 {} 帧）新 {} 个 → {:?}",
            step.samples as f64 / 16_000.0,
            step.at,
            step.step.mel_seconds + step.step.mel_shape_seconds
                + step.step.encode_seconds + step.step.decode_seconds,
            step.step.mel_seconds,
            step.step.mel_shape_seconds,
            step.step.encode_seconds,
            step.step.decode_seconds,
            step.step.prefill_seconds,
            step.step.prefill_tokens,
            step.step.reused_tokens,
            step.step.prompt_tokens,
            step.step.audio_tokens,
            step.step.encoder_prefix_tokens,
            step.step.frozen_tokens,
            step.step.active_frames,
            step.step.new_tokens,
            step.step.text
        );
    }
    assert!(
        long_steps.iter().any(|step| step.step.encoder_prefix_tokens > 0
            && step.step.frozen_tokens == 0
            && step.step.active_frames > 0),
        "8 秒之后的前缀应当被复用：这一步只编活动窗"
    );

    // 每一步的 token 与「这段音频整段重算」逐位相同：跨步复用键值只改变算了多少，
    // 不改变算出来的东西。
    for step in &long_steps {
        let prefix = &long[..step.samples];
        let one_pass = engine.transcribe(&mut ops, prefix).expect("整段重算");
        println!(
            "  {:.2} 秒这一段的 token：复用 {:?} / 整段 {:?}（提示 {}，重算 {}）",
            step.samples as f64 / 16_000.0,
            step.step.tokens,
            one_pass.tokens,
            step.step.prompt_tokens,
            step.step.prefill_tokens
        );
        assert_eq!(
            step.step.tokens, one_pass.tokens,
            "{:.2} 秒处的 token 与整段重算不一致",
            step.samples as f64 / 16_000.0
        );
        assert_eq!(
            step.step.text, one_pass.text,
            "{:.2} 秒处的文本与整段重算不一致",
            step.samples as f64 / 16_000.0
        );
    }

    // 预填充只算「活动窗 + 固定尾巴 + 已固定文本」：提示变长时重算的位置不该跟着长。
    let first = &long_steps[0].step;
    let last = &long_steps[long_steps.len() - 1].step;
    println!(
        "预填充位置：第一步 {}（提示 {}），第六步 {}（提示 {}），沿用 {} 个",
        first.prefill_tokens, first.prompt_tokens, last.prefill_tokens, last.prompt_tokens,
        last.reused_tokens
    );
    assert!(
        last.prefill_tokens < last.prompt_tokens,
        "提示 {} 个位置，这一步应当只重算其中一部分，实际重算 {}",
        last.prompt_tokens,
        last.prefill_tokens
    );
    assert!(
        last.reused_tokens >= first.prompt_tokens,
        "第六步沿用的位置数 {} 至少应当覆盖第一步的整条提示 {}",
        last.reused_tokens,
        first.prompt_tokens
    );
    assert_eq!(
        last.prompt_tokens - last.prefill_tokens,
        last.reused_tokens,
        "提示长度 = 沿用 + 重算"
    );
    assert_eq!(
        long_final.tokens, long_baseline.tokens,
        "14 秒音频的定稿也要与整段重算一致"
    );

    // 增量梅尔在 14 秒这个长度上与整段重算逐位相同（已定帧不重算、末尾帧按新右端重算、
    // 整形按当前前缀的 max 套用这三件事各自精确）。
    let frontend = MelFrontend::new();
    let whole = frontend.log_mel(&long);
    let mut incremental = voice_mel::MelStream::new();
    let mut settled = 0usize;
    for block in long.chunks(BLOCK_SAMPLES) {
        settled += incremental.push_i16(&frontend, block);
    }
    assert_eq!(incremental.frames(), whole.len() / MEL_BINS);
    // 224 000 个样本落在 hop 网格的第 1400 帧上（`224000/160` 正好整除），
    // 这一帧的窗跨过末端，所以它一直是未定的那一帧；其余 1399 帧已定。
    assert_eq!(settled, 1_399);
    assert_eq!(incremental.settled_frames(), 1_399);
    assert_eq!(incremental.settled_frames() + 1, incremental.frames());
    assert_eq!(
        incremental.shaped_block(),
        whole,
        "增量梅尔应与整段重算逐位相同"
    );

    // 每步的梅尔只算新样本：六个步的梅尔墙钟不该随已收音频长度长。
    let first_mel = long_steps[0].step.mel_seconds;
    let last_mel = long_steps[long_steps.len() - 1].step.mel_seconds;
    println!(
        "梅尔：第一步 {first_mel:.4} s（2.00 秒音频），第六步 {last_mel:.4} s（12.00 秒音频），比值 {:.2}",
        last_mel / first_mel
    );
    assert!(
        last_mel < 4.0 * first_mel + 0.004,
        "梅尔墙钟不应当随已收音频长度长：第一步 {first_mel:.4} s、第六步 {last_mel:.4} s"
    );
}

/// 从 `(128, frames)` 行优先的梅尔里切出连续的一段 `[start, end)`。
fn mel_columns(mel: &[f32], frames: usize, start: usize, end: usize) -> Vec<f32> {
    let width = end - start;
    let mut out = vec![0.0f32; MEL_BINS * width];
    for bin in 0..MEL_BINS {
        out[bin * width..(bin + 1) * width]
            .copy_from_slice(&mel[bin * frames + start..bin * frames + end]);
    }
    out
}

#[test]
#[ignore = "只打印切窗与整段编码的差，不是对照闸"]
fn cutting_a_window_stays_within_the_f16_spread_of_one_pass() {
    // 12 秒音频一次编完（1200 帧，两个窗：800 + 400）与切开逐窗编，是同一条算术：
    // 整段那条用块对角加法 mask，窗与窗之间 exp = 0；切开那条干脆按窗切片。差别只可能
    // 来自 f16 归约次序（批长不同、核的 tiling 不同），量级与 encoder_parity 的投影差
    // 同档。这里的数是流式前缀能不能复用的直接依据：≤ 8 秒时两条路是同一个张量、
    // 同一次调用（逐位相同）；> 8 秒时差在 1e-4 这一档，部分结果的文本因此可能与
    // 定稿差一两个字，定稿以整段一次编码为准。
    let metal = metallib();
    let dir = model_dir();
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let mut ops = MlxBackend::on_this_thread().expect("stream 在本线程");
    let tower = AudioTower::open(dir.to_str().expect("模型目录"), DType::F16).expect("音频塔权重");

    let pcm = repeat(&probe(), 6);
    let mel = MelFrontend::new().log_mel(&pcm);
    let frames = mel.len() / MEL_BINS;
    assert_eq!(frames, 1200);

    let whole = encode_mel(&mut ops, &tower, &mel, frames, DType::F16).expect("整段");
    let first = encode_mel(
        &mut ops,
        &tower,
        &mel_columns(&mel, frames, 0, 800),
        800,
        DType::F16,
    )
    .expect("第 0 窗");
    let second = encode_mel(
        &mut ops,
        &tower,
        &mel_columns(&mel, frames, 800, 1200),
        400,
        DType::F16,
    )
    .expect("第 1 窗");

    assert_eq!(whole.tokens, 156);
    assert_eq!(first.tokens + second.tokens, 156);
    let mut worst = 0.0f32;
    for (index, value) in whole.proj.iter().enumerate() {
        let cut = if index < first.proj.len() {
            first.proj[index]
        } else {
            second.proj[index - first.proj.len()]
        };
        worst = worst.max((value - cut).abs());
    }
    println!("1200 帧：整段 {} 个 token，切开 {}+{} 个 token，投影最大绝对差 {worst:e}",
        whole.tokens, first.tokens, second.tokens);
    assert!(
        worst < 1e-3,
        "切窗重编与整段一次编码的差应留在 f16 的归约误差里，实测差 {worst:e}"
    );
}

/// 三段拼出来的提示与整段分词逐个数相同：这是「提示头 + 已结束各窗的行」能按
/// 位置留在缓存里的前提。切点落在 `<|audio_start|>` 与 `<|audio_end|>` 这两个
/// 特殊 token 上，而分词器先把特殊 token 切出来、剩下的文字各自分词，所以切与
/// 不切是同一串 id。
#[test]
fn prompt_splicing_matches_one_pass_tokenization() {
    let dir = model_dir();
    let tokenizer = voice_tokenizer::Tokenizer::load(&dir).expect("分词器");
    for context in ["", "今天天气不错。"] {
        for language in [Some(PROMPT_LANGUAGE), None] {
            for audio in [0usize, 1, 26, 104, 800, 1_200] {
                let parts = voice_mlx::prompt_parts(&tokenizer, language, context);
                let spliced = voice_mlx::splice_prompt(&parts, audio);
                let whole = voice_mlx::build_prompt(&tokenizer, audio, language, context);
                assert_eq!(
                    spliced, whole,
                    "上下文 {context:?}、语言 {language:?}、{audio} 个音频占位：拼接与整段分词不一致"
                );
                assert!(
                    parts.head.iter().all(|id| *id != voice_tensor::AUDIO_PAD),
                    "提示头里不该有音频占位"
                );
            }
        }
    }
}

/// 整段预填充的墙钟（改前那条路）：同一条提示一次送进 28 层，每步一个数。
///
/// 与流式那条测试在同一个归档、同一台机器上跑，两边一比就是这次省下来的预填充。
/// 提示长度取流式十五步的实际值（44…408），第 7 步以后每步多两个 8 秒窗前缀。
#[test]
#[ignore = "只打印整段预填充的墙钟，不是对照闸"]
fn full_prefill_cost_by_prompt_length() {
    let metal = metallib();
    let dir = model_dir();
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let mut ops = MlxBackend::on_this_thread().expect("stream 在本线程");
    let mut engine = Engine::open(dir.to_str().expect("模型目录"), DType::F16).expect("权重");

    let probe = probe();
    let pcm = repeat(&probe, 15);
    let frontend = MelFrontend::new();
    println!("整段预填充（每步一次，三次取中位）");
    for step in 1..=15usize {
        let prefix = &pcm[..step * STEP_SAMPLES];
        let mel = frontend.log_mel(prefix);
        let frames = mel.len() / MEL_BINS;
        let encoded = {
            let tower = engine.tower();
            encode_mel(&mut ops, tower, &mel, frames, DType::F16).expect("编码")
        };
        let prompt = voice_mlx::build_prompt(
            engine.tokenizer().expect("分词器"),
            encoded.tokens,
            Some(PROMPT_LANGUAGE),
            "",
        );
        let mut times: Vec<f64> = Vec::new();
        for _ in 0..3 {
            let clock = Instant::now();
            let decoder = engine.decoder().expect("文本塔");
            decoder
                .prefill(&mut ops, &prompt, &encoded.proj, 1)
                .expect("预填充");
            times.push(clock.elapsed().as_secs_f64());
        }
        times.sort_by(|left, right| left.partial_cmp(right).expect("墙钟"));
        println!(
            "  {:.2} 秒音频（{} 帧，{} 个音频 token）提示 {} 个位置：{:.3} s",
            prefix.len() as f64 / 16_000.0,
            frames,
            encoded.tokens,
            prompt.len(),
            times[1]
        );
    }
}

/// 复用键值那一截与整段一次预填充的差：同一条提示（12 秒音频，174 个位置），
/// 先整段跑一次，再「留 113 个位置（提示头 + 第一个 8 秒窗的 104 行）重算 61 个」
/// 跑一次，比最后一个位置的 logits 前 64 维与 argmax。
///
/// 差只可能来自 f16 的归约次序：查询之间互不影响，遮罩是同一串值（分段那条按
/// 绝对位置拼，与整段的因果遮罩逐元素相同），键值里被复用的那些行前面没有别的东西。
#[test]
#[ignore = "只打印复用与整段预填充的差，不是对照闸"]
fn reused_key_values_match_one_pass_prefill() {
    use voice_parity::tolerance::{Margin, PREFILL_LOGITS_UNITS};
    use voice_tensor::{HIDDEN, KvCache, TEXT_LAYERS};

    let metal = metallib();
    let dir = model_dir();
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let mut ops = MlxBackend::on_this_thread().expect("stream 在本线程");
    let mut engine = Engine::open(dir.to_str().expect("模型目录"), DType::F16).expect("权重");

    let pcm = repeat(&probe(), 6);
    let mel = MelFrontend::new().log_mel(&pcm);
    let frames = mel.len() / MEL_BINS;
    let encoded = {
        let tower = engine.tower();
        encode_mel(&mut ops, tower, &mel, frames, DType::F16).expect("编码")
    };
    let prompt = voice_mlx::build_prompt(
        engine.tokenizer().expect("分词器"),
        encoded.tokens,
        Some(PROMPT_LANGUAGE),
        "",
    );
    let head = voice_mlx::prompt_parts(
        engine.tokenizer().expect("分词器"),
        Some(PROMPT_LANGUAGE),
        "",
    )
    .head
    .len();
    // 留提示头与第一个 8 秒窗的 104 行，重算活动窗的 52 行加固定尾巴。
    let keep = head + 104;
    assert_eq!(head, 9, "提示头的 token 数");
    assert_eq!(prompt.len(), 174, "12 秒的提示长度");
    assert_eq!(encoded.tokens, 156);

    let decoder = engine.decoder().expect("文本塔");
    let whole = decoder
        .prefill(&mut ops, &prompt, &encoded.proj, 64)
        .expect("整段预填充");

    let mut cache: Vec<KvCache<voice_mlx::MlxTensor>> =
        (0..TEXT_LAYERS).map(|_| KvCache::new()).collect();
    let first = decoder
        .prefill_chunk(
            &mut ops,
            &mut cache,
            &prompt[..keep],
            &encoded.proj[..(keep - head) * HIDDEN],
            0,
            64,
            &voice_mlx::DecoderOptions::default(),
        )
        .expect("前一段");
    let second = decoder
        .prefill_chunk(
            &mut ops,
            &mut cache,
            &prompt[keep..],
            &encoded.proj[(keep - head) * HIDDEN..],
            keep,
            64,
            &voice_mlx::DecoderOptions::default(),
        )
        .expect("后一段");

    let mut worst = 0.0f32;
    let mut reference_max = 0.0f32;
    for (left, right) in whole.head.iter().zip(second.head.iter()) {
        worst = worst.max((left - right).abs());
        reference_max = reference_max.max(left.abs());
    }
    println!(
        "整段预填充：argmax {}（{:e}），前一段（留在缓存里的 {} 个位置）argmax {}（{:e}）",
        whole.argmax, whole.argmax_value, keep, first.argmax, first.argmax_value
    );
    println!(
        "复用 + 重算：argmax {}（{:e}），logits 前 64 维最大绝对差 {worst:e}",
        second.argmax, second.argmax_value
    );
    assert_eq!(second.argmax, whole.argmax, "argmax 必须相同");
    // 容差按仓库唯一的式子算：参考绝对值上界 × 2^-11 × 单位数，单位数取
    // `PREFILL_LOGITS_UNITS`（28 层文本塔 + lm_head 那一道闸的口径）。
    let margin = Margin::new(reference_max, PREFILL_LOGITS_UNITS, worst);
    println!(
        "同一条提示：整段预填充 vs 复用 + 重算，前 64 维 logits 最大绝对差 {:e}\
（上界 {:e}，容差 {:e}，折 {} 个单位，余量 {:.2} 倍）",
        margin.diff,
        margin.reference_abs_max,
        margin.tolerance(),
        margin.used_units(),
        margin.times()
    );
    assert!(
        margin.diff <= margin.tolerance(),
        "复用与整段预填充的 logits 差应在 f16 归约误差里：实测 {:e} > 容差 {:e}",
        margin.diff,
        margin.tolerance()
    );
}

/// 30 秒探针（十五步）：预填充重算的位置数封顶在一窗加尾巴，不再随已收音频长。
///
/// 14 秒那条探针只有两步落在「第一个 8 秒窗已经结束」之后，省下来的预填充因此
/// 只出现在最后两步。这里把同一条探针重复 15 次，看第五、六个窗接上去之后
/// 重算的位置数还封不封顶。
#[test]
#[ignore = "只打印 30 秒探针的预填充，不是对照闸"]
fn thirty_second_probe_keeps_the_prefill_flat() {
    let metal = metallib();
    let dir = model_dir();
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let mut ops = MlxBackend::on_this_thread().expect("stream 在本线程");
    let mut engine = Engine::open(dir.to_str().expect("模型目录"), DType::F16).expect("权重");

    let pcm = repeat(&probe(), 15);
    let (steps, _, final_pass) = stream_blocks(&mut engine, &mut ops, &pcm, 5);
    println!("30 秒探针（{} 步），定稿 {:?}", steps.len(), final_pass.tokens);
    for step in &steps {
        println!(
            "  {:.2} 秒：提示 {}（重算 {}，沿用 {}）预填充 {:.3} s → {:?}",
            step.samples as f64 / 16_000.0,
            step.step.prompt_tokens,
            step.step.prefill_tokens,
            step.step.reused_tokens,
            step.step.prefill_seconds,
            step.step.tokens
        );
    }
    let last = &steps[steps.len() - 1].step;
    println!(
        "最后一步：提示 {} 个位置，重算 {} 个，沿用 {} 个（第一个 8 秒窗的 104 行已经留在缓存里）",
        last.prompt_tokens, last.prefill_tokens, last.reused_tokens
    );
    // 最后一块送完不再出步（那一步就是定稿），所以 30 秒音频是十四步。
    assert_eq!(steps.len(), 14, "2 秒一步，30 秒音频出十四步部分结果");
    assert!(
        last.prompt_tokens > 380,
        "30 秒的提示应当有 400 个上下的位置，实际 {}",
        last.prompt_tokens
    );
    assert!(
        last.prefill_tokens < 100,
        "最后一步只该重算活动窗加尾巴，实际 {}",
        last.prefill_tokens
    );
    assert!(
        last.reused_tokens > 300,
        "最后一步应当沿用三个 8 秒窗的前缀（300 个以上位置），实际 {}",
        last.reused_tokens
    );
}

/// 提示里带已固定文本时（`unfixed = 1`），跨步复用键值的 token 仍与整段重算一致。
///
/// 已固定文本排在音频占位之后、固定尾巴之后，它前面的活动窗每步都在变，所以这一段
/// 每步都要重算：这条测试就是把「固定了几个 token」也走到，确认那条路不改变结果。
#[test]
#[ignore = "只打印带已固定文本的流式实测，不是对照闸"]
fn stable_text_still_matches_one_pass_with_reused_key_values() {
    let metal = metallib();
    let dir = model_dir();
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let mut ops = MlxBackend::on_this_thread().expect("stream 在本线程");
    let mut engine = Engine::open(dir.to_str().expect("模型目录"), DType::F16).expect("权重");

    let pcm = repeat(&probe(), 8);
    let (steps, _, final_pass) = stream_blocks(&mut engine, &mut ops, &pcm, 1);
    println!("八秒探针（{} 步，unfixed = 1），定稿 {:?}", steps.len(), final_pass.tokens);
    for step in &steps {
        println!(
            "  {:.2} 秒：提示 {}（重算 {}，沿用 {}），固定 {} 个 → {:?} {:?}",
            step.samples as f64 / 16_000.0,
            step.step.prompt_tokens,
            step.step.prefill_tokens,
            step.step.reused_tokens,
            step.step.stable_tokens,
            step.step.tokens,
            step.step.text
        );
    }
    assert!(
        steps.iter().any(|step| step.step.stable_tokens > 0),
        "unfixed = 1 时提示里应当出现已固定的文本"
    );
    for step in &steps {
        let one_pass = engine
            .transcribe(&mut ops, &pcm[..step.samples])
            .expect("整段重算");
        // 已固定文本与整段重算的起点不同（前者少生成几个 token），只比新生成的
        // 那一段：固定的是上一步的假设前缀，贪心对齐之后剩下的必须一样。
        assert_eq!(
            step.step.tokens.last(),
            one_pass.tokens.last(),
            "{:.2} 秒处最后一个 token 与整段重算不一致",
            step.samples as f64 / 16_000.0
        );
        assert_eq!(
            step.step.text, one_pass.text,
            "{:.2} 秒处的文本与整段重算不一致",
            step.samples as f64 / 16_000.0
        );
    }
}
