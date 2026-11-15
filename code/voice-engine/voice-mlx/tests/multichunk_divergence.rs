//! 研究：300 / 400 帧（3 / 4 个整块）上的逐层实对照。
//!
//! 只测量、只打印，不改任何产品口径，也不动 053 的三条闸。这里要回答的是
//! 「缺陷从哪一层开始」：梅尔 → 卷积茎（批量多块）→ 每块的通道重排与 `conv_out`
//! → 每块的正弦位置 → 窗内注意力（第 0 层）→ `ln_post`/`proj1`/`proj2` → 音频 token 数。
//!
//! 参考资产是 `code/voice-oracle/assets/multichunk/`（见 notes/072）：
//! `s1_3.0` 300 帧 39 token、`s1_4.0` 400 帧 52 token，两段都是整块，没有尾块。
//!
//! 为了把「上游」与「下游」分开，编码器那一段喂的是**参考的 mel**（与 053 的闸同一条
//! 做法），所以差值不再含梅尔那一点算术差；梅尔自己另有一段单独比。
//!
//! 跑法同 `encoder_parity.rs` 与 `BUILD.txt`：三条链接参数缺一不可。
//!
//! ```text
//! export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
//! export LIBRARY_PATH="$SDKROOT/usr/lib"
//! export MLX_SYS_SYSTEM_SDK=1
//! export RUSTFLAGS="-C link-arg=-framework -C link-arg=Accelerate ..."
//! cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
//!   --test multichunk_divergence -- --nocapture --test-threads=1
//! ```

#![cfg(feature = "metal")]

use std::path::PathBuf;

use voice_mel::MelFrontend;
use voice_mlx::{encode_mel, transcribe, AudioTower, MlxBackend, StopReason};
use voice_parity::tolerance::{
    f16_tolerance, Margin, CONV_STEM_UNITS, ENCODER_LAYER0_UNITS, ENCODER_PROJ_UNITS,
    MEL_TOLERANCE,
};
use voice_parity::{asset_root, compare_slices, read_pcm_i16, Reference};
use voice_tensor::{DType, CHUNK_FRAMES, D_MODEL, MEL_BINS, PROJECTION_OUT, TOKENS_PER_CHUNK};

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

fn multichunk_root() -> PathBuf {
    asset_root().join("multichunk")
}

/// 一段的参考量：`Reference` 是那一段自己的扁平 `manifest.json`。
fn segment(name: &str) -> Reference {
    Reference::load_from(multichunk_root().join(name))
        .unwrap_or_else(|error| panic!("载入 {name} 参考失败：{error}"))
}

/// 参考落盘的输入 PCM（`input_pcm.bin`，裸小端 i16）。
///
/// `Reference::load_from` 把每个 `.bin` 都按 f32 读，`input_pcm` 是 i16，
/// 所以这一件不经 `Reference`，直接读字节。
fn pcm_of(name: &str) -> Vec<i16> {
    let path = multichunk_root().join(name).join("input_pcm.bin");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("读不到 {}：{error}", path.display()));
    read_pcm_i16(&bytes)
}

fn abs_max(values: &[f32]) -> f32 {
    values.iter().fold(0.0f32, |acc, value| acc.max(value.abs()))
}

/// 把一段报告压成一行：最大绝对差、最大差下标、首个超差下标与个数。
fn line(name: &str, report: &voice_parity::Report) -> String {
    let max = match (report.max_abs_diff, report.max_index) {
        (Some(max), Some(index)) => format!("max|d|={max:e}@{index}"),
        _ => "max|d|=无".to_string(),
    };
    let first = match &report.first_exceed {
        Some(hit) => format!("首个超差={}@{}", hit.diff, hit.index),
        None => "首个超差=无".to_string(),
    };
    format!(
        "{name}：参考 {} 个，实际 {} 个（差 {:+#}）{max}，容差 {:e}，{first}，超差 {} 个",
        report.expected_len,
        report.actual_len,
        report.length_delta,
        report.tolerance,
        report.exceeded_count()
    )
}

fn print_report(label: &str, name: &str, report: &voice_parity::Report) {
    println!("{label}{name}");
    println!("  {}", line(name, report));
}

/// 一层的「差值账」：参考上界、允许单位数、实测最大差 → 容差、单位数、余量。
fn print_margin(label: &str, margin: &Margin, first_exceed: Option<usize>) {
    println!(
        "{label}实测差 {:e}，{}，首个超差下标 {:?}",
        margin.diff,
        margin,
        first_exceed
    );
}

fn first_exceed(report: &voice_parity::Report) -> Option<usize> {
    report.first_exceed.as_ref().map(|hit| hit.index)
}

/// 段的时间维切出来的每一块：`(段名, 块号)` → 参考里的 `conv_stem_out[k]`。
fn stem_of_chunk(reference: &Reference, chunk: usize, time: usize) -> (Vec<f32>, Vec<usize>) {
    let stem = reference.tensor("conv_stem_out").expect("conv_stem_out 存在");
    let per_chunk = 16 * time * 480;
    let start = chunk * per_chunk;
    let values = stem.values[start..start + per_chunk].to_vec();
    let mut shape = stem.shape.clone();
    shape.remove(0);
    (values, shape)
}

/// 1) 梅尔：应完全一致（同一份 `pcm`，同一条整形）。
///
/// 参考的 `mel` 是 f32，本仓的 `log_mel` 也是 f32；两边算术不同（numpy 实数 FFT
/// 对 vDSP 五条 80 点 DFT），容差按 `voice_parity::tolerance::MEL_TOLERANCE`。
#[test]
fn mel_matches_reference_on_three_and_four_chunks() {
    let frontend = MelFrontend::new();
    for name in ["s1_3.0", "s1_4.0"] {
        let reference = segment(name);
        let pcm = pcm_of(name);
        let mel = frontend.log_mel(&pcm);
        let frames = mel.len() / MEL_BINS;
        let expected = reference.tensor("mel").expect("mel 存在");
        assert_eq!(
            frames, expected.shape[1],
            "{name}：本仓梅尔 {frames} 帧，参考 {} 帧",
            expected.shape[1]
        );
        let report = compare_slices("mel", &expected.values, &expected.shape, &mel, MEL_TOLERANCE);
        print_report(&format!("{name} 梅尔：{frames} 帧（参考 {} 帧）\n  ", expected.shape[1]), "mel", &report);
        assert!(
            report.within_tolerance(),
            "{name} 的梅尔不该超差：{}",
            line("mel", &report)
        );
    }
}

/// 2) 从卷积茎到投影：喂参考的 mel，逐层报差值。
///
/// 顺带量出本仓在这两段上吐几个音频 token——一处被截，后面的层与 token 数就全跟着错。
#[test]
fn encoder_layers_on_three_and_four_chunks() {
    let metal = metallib();
    assert!(metal.is_file(), "metallib 不在 {}", metal.display());
    let dir = model_dir();
    assert!(
        dir.join("weights.safetensors").is_file(),
        "权重不在 {}；可用 VOICE_MODEL_DIR 覆盖",
        dir.display()
    );
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let mut backend = MlxBackend::on_this_thread().expect("stream 在本线程");
    let tower = AudioTower::open(dir.to_str().expect("模型目录"), DType::F16).expect("音频塔权重");

    for name in ["s1_3.0", "s1_4.0"] {
        let reference = segment(name);
        let mel = reference.tensor("mel").expect("mel 存在");
        let frames = mel.shape[1];
        let n_full = frames / CHUNK_FRAMES;
        println!("\n=== {name}：{frames} 帧 = {n_full} 个整块，参考 audio_tokens={TOKENS_PER_CHUNK}×{n_full}={}", TOKENS_PER_CHUNK * n_full);

        let out = encode_mel(&mut backend, &tower, &mel.values, frames, DType::F16).expect("前向");
        println!(
            "本仓：音频 token {} 个（参考 {} 个，差 {:+#}），窗边界 {:?}",
            out.tokens,
            TOKENS_PER_CHUNK * n_full,
            out.tokens as i64 - (TOKENS_PER_CHUNK * n_full) as i64,
            out.cu_seqlens
        );
        // 新闸（300 / 400 帧）：音频 token 数与参考逐块相同，一个不多一个不少。
        // 这一条正是「只覆盖前两块」会撞上的地方——被截的编码器吐不出 39 / 52 个。
        assert_eq!(
            out.tokens,
            TOKENS_PER_CHUNK * n_full,
            "{name}：音频 token 数应与参考的 {} 相同",
            TOKENS_PER_CHUNK * n_full
        );
        // 300 / 400 帧都只有 1 个窗（远小于 800 帧），所以窗边界是整段一个。
        assert_eq!(
            out.cu_seqlens,
            vec![0, out.tokens],
            "{name}：{frames} 帧应只有一个注意力窗"
        );

        // --- 卷积茎，第 0 块的批量输出 ---
        let stem = reference.tensor("conv_stem_out").expect("conv_stem_out 存在");
        let stem_shape = stem.shape.clone();
        let per_chunk = 16 * stem_shape[2] * stem_shape[3];
        assert_eq!(stem_shape[0], n_full, "{name}：参考的块数与帧数一致");
        println!("卷积茎：参考整批 {:?}，本仓只落第 0 块 {:?}", stem_shape, out.stem_chunk0_shape());
        let mut chunk_shape = stem_shape.clone();
        chunk_shape.remove(0);
        let report = compare_slices(
            "conv_stem_out[0]",
            &stem.values[..per_chunk],
            &chunk_shape,
            &out.stem_chunk0,
            f16_tolerance(abs_max(&stem.values), CONV_STEM_UNITS),
        );
        print_report("  批量第 0 块：", "conv_stem_out[0]", &report);
        print_margin("    ", &Margin::new(abs_max(&stem.values), CONV_STEM_UNITS, report.max_abs_diff.unwrap_or(0.0)), first_exceed(&report));
        assert!(
            report.within_tolerance(),
            "{name} 批量第 0 块的卷积茎不该超差：{}",
            line("conv_stem_out[0]", &report)
        );

        // --- 每块单独跑一遍：批量里看不见的第 1、2、3 块 ---
        // 卷积茎按块独立（每块 100 帧、位置从 0 重排），所以把这一块的 100 帧
        // 单独送进编码器，它的第 0 块卷积茎就是整批里的第 k 块。
        for chunk in 0..n_full {
            let start = chunk * CHUNK_FRAMES;
            let piece = piece_of(mel, start, CHUNK_FRAMES);
            let alone = encode_mel(&mut backend, &tower, &piece, CHUNK_FRAMES, DType::F16)
                .expect("单块前向");
            assert_eq!(alone.tokens, TOKENS_PER_CHUNK, "{name} 第 {chunk} 块单独编应有 13 个 token");
            let (expected, expected_shape) = stem_of_chunk(&reference, chunk, stem_shape[2]);
            let report = compare_slices(
                &format!("conv_stem_out[{chunk}]"),
                &expected,
                &expected_shape,
                &alone.stem_chunk0,
                f16_tolerance(abs_max(&expected), CONV_STEM_UNITS),
            );
            print_report(&format!("  第 {chunk} 块（单独编）："), &format!("conv_stem_out[{chunk}]"), &report);
            print_margin(
                "    ",
                &Margin::new(abs_max(&expected), CONV_STEM_UNITS, report.max_abs_diff.unwrap_or(0.0)),
                first_exceed(&report),
            );
            // 批量多块里的第 k 块必须与单独编这一块逐位同档：块维、通道重排、
            // 每块位置表三件事都在这一条里。
            assert!(
                report.within_tolerance(),
                "{name} 第 {chunk} 块（单独编）的卷积茎不该超差：{}",
                line(&format!("conv_stem_out[{chunk}]"), &report)
            );
        }

        // --- 第 0 层（含残差）：窗内注意力之前的位置表与之后的注意力都在这一层里 ---
        let layer0 = reference.tensor("encoder_layer0_out").expect("encoder_layer0_out 存在");
        let report = compare_slices(
            "encoder_layer0_out",
            &layer0.values,
            &layer0.shape,
            &out.layer0,
            f16_tolerance(abs_max(&layer0.values), ENCODER_LAYER0_UNITS),
        );
        print_report("  第 0 层：", "encoder_layer0_out", &report);
        print_margin(
            "    ",
            &Margin::new(abs_max(&layer0.values), ENCODER_LAYER0_UNITS, report.max_abs_diff.unwrap_or(0.0)),
            first_exceed(&report),
        );
        assert!(
            report.within_tolerance(),
            "{name} 的第 0 层不该超差：{}",
            line("encoder_layer0_out", &report)
        );

        // --- 每块那 13 行单看：位置表按块重排、窗内注意力之后的行都在这里 ---
        for chunk in 0..n_full {
            let rows = TOKENS_PER_CHUNK;
            let at = chunk * rows;
            let layer0_rows = &layer0.values[at * D_MODEL..(at + rows) * D_MODEL];
            let ours_rows = &out.layer0[at * D_MODEL..(at + rows) * D_MODEL];
            let report = compare_slices(
                &format!("encoder_layer0_out[{at}..{}]", at + rows),
                layer0_rows,
                &[1, rows, D_MODEL],
                ours_rows,
                f16_tolerance(abs_max(&layer0.values), ENCODER_LAYER0_UNITS),
            );
            print_report(
                &format!("    第 0 层 token {at}..{}：", at + rows),
                &format!("encoder_layer0_out[{at}..{}]", at + rows),
                &report,
            );
            assert!(
                report.within_tolerance(),
                "{name} 第 0 层的 token {at}..{} 不该超差：{}",
                at + rows,
                line(&format!("encoder_layer0_out[{at}..{}]", at + rows), &report)
            );
        }

        // --- 投影 ---
        let proj = reference.tensor("encoder_proj_out").expect("encoder_proj_out 存在");
        let report = compare_slices(
            "encoder_proj_out",
            &proj.values,
            &proj.shape,
            &out.proj,
            f16_tolerance(abs_max(&proj.values), ENCODER_PROJ_UNITS),
        );
        print_report("  投影：", "encoder_proj_out", &report);
        print_margin(
            "    ",
            &Margin::new(abs_max(&proj.values), ENCODER_PROJ_UNITS, report.max_abs_diff.unwrap_or(0.0)),
            first_exceed(&report),
        );
        assert!(
            report.within_tolerance(),
            "{name} 的末尾投影不该超差：{}",
            line("encoder_proj_out", &report)
        );
        for chunk in 0..n_full {
            let rows = TOKENS_PER_CHUNK;
            let at = chunk * rows;
            let want = &proj.values[at * PROJECTION_OUT..(at + rows) * PROJECTION_OUT];
            let got = &out.proj[at * PROJECTION_OUT..(at + rows) * PROJECTION_OUT];
            let report = compare_slices(
                &format!("encoder_proj_out[{at}..{}]", at + rows),
                want,
                &[rows, PROJECTION_OUT],
                got,
                f16_tolerance(abs_max(&proj.values), ENCODER_PROJ_UNITS),
            );
            print_report(
                &format!("    投影 token {at}..{}：", at + rows),
                &format!("encoder_proj_out[{at}..{}]", at + rows),
                &report,
            );
            assert!(
                report.within_tolerance(),
                "{name} 投影的 token {at}..{} 不该超差：{}",
                at + rows,
                line(&format!("encoder_proj_out[{at}..{}]", at + rows), &report)
            );
        }
    }
}

/// 从 `(128, frames)` 的梅尔里切出 `[start, start + len)` 这一段。
fn piece_of(mel: &voice_parity::Tensor, start: usize, len: usize) -> Vec<f32> {
    let frames = mel.shape[1];
    let mut out = Vec::with_capacity(MEL_BINS * len);
    for bin in 0..MEL_BINS {
        let base = bin * frames + start;
        out.extend_from_slice(&mel.values[base..base + len]);
    }
    out
}

/// 3) 整条 `transcribe`：本仓在这几段上到底给几个音频 token、什么文本。
///
/// 这一条最贵（每次开一份权重），两组各跑一次。它把编码器的 token 数与
/// 提示里的占位个数、解码出来的文本印在同一行，用来把「token 被截」与
/// 「文本只覆盖前两块」对上。
#[test]
fn transcribe_reports_audio_tokens_and_text() {
    let dir = model_dir();
    assert!(dir.join("weights.safetensors").is_file(), "权重不在 {}", dir.display());
    let metal = metallib();
    assert!(metal.is_file(), "metallib 不在 {}", metal.display());
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let frontend = MelFrontend::new();
    for (name, want_tokens, want_text) in [
        ("s1_3.0", 39, "今天下午三点开会，请提前。"),
        ("s1_4.0", 52, "今天下午三点开会，请提前十分钟到会。"),
    ] {
        let pcm = pcm_of(name);
        let frames = frontend.log_mel(&pcm).len() / MEL_BINS;
        let result = transcribe(dir.to_str().expect("模型目录"), &pcm).expect("识别");
        println!(
            "{name}：样本 {}，梅尔 {} 帧（{frames}），音频 token {}，提示 {} 个，tokens {:?}，文本 {:?}，停止 {:?}",
            pcm.len(),
            result.mel_frames,
            result.audio_tokens,
            result.prompt_tokens,
            result.tokens,
            result.text,
            result.stopped_by
        );
        // 新闸：整条 `transcribe`（本仓自己的梅尔，不经参考）在三块 / 四块上
        // 给出的文本与参考逐字相同，且停在 `<|im_end|>`。文本按 073 第 2 节记的
        // 参考结果写死，不按本仓输出回填。
        assert_eq!(result.mel_frames, frames, "{name}：梅尔帧数应与梅尔前端的帧数一致");
        assert_eq!(result.audio_tokens, want_tokens, "{name}：音频 token 数应为 {want_tokens}");
        assert_eq!(result.text, want_text, "{name}：文本应与参考逐字相同");
        assert_eq!(
            result.stopped_by,
            Some(StopReason::ImEnd),
            "{name}：应停在 <|im_end|>"
        );
    }
}
