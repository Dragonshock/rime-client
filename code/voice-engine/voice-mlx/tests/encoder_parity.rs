//! 编码器三个闸的实对照：`conv_stem_out`、`encoder_layer0_out`、`encoder_proj_out`。
//!
//! 参考资产由 `code/voice-oracle/dump_reference.py` 导出，参考激活是 float16。
//! 这里把参考的 `mel`（f32，128×200）当输入送进 `voice-mlx` 的音频塔，
//! 中间量与参考逐元素比。先对形状，再对数值。
//!
//! 参考的梅尔是 f32 送进编码器之前才转 f16，所以这里也按 f32 收、按 f16 算。
//!
//! 跑法（三条链接参数缺一不可，见 `BUILD.txt`）：测试二进制要真链接 Metal，
//! MLX 的静态库引用 `Accelerate` 的 BNNS 与 cblas，还引用了当前 SDK 只在
//! arm64e 段导出的几个 Metal/Foundation 弱符号。
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
//! cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline
//! ```

#![cfg(feature = "metal")]

use std::path::PathBuf;

use voice_mlx::{
    encode_mel, encode_mel_with, AudioTower, EncoderOptions, MlxBackend, PositionDenominator,
};
use voice_parity::{
    asset_root,
    tolerance::{
        Margin, CONV_STEM_UNITS, ENCODER_LAYER0_UNITS, ENCODER_PROJ_UNITS,
    },
    Reference,
};
use voice_tensor::DType;

// 容差口径只在 `voice_parity::tolerance` 定义一处：
//
//   容差 = 参考张量的绝对值上界 × 2^-11 × 允许的单位数
//
// 参考是 f16，`2^-11` 是「元素按自身量级折算的半个 f16 间距」的上界；单位数每闸
// 各一个常量，取多少与实测多少都写在 `voice_parity::tolerance` 的注释里。
// 这里不再写任何绝对容差。

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

/// 参考里某件张量的绝对值上界。容差以它为尺度。
fn abs_max(values: &[f32]) -> f32 {
    values
        .iter()
        .fold(0.0f32, |acc, value| acc.max(value.abs()))
}

/// 三个闸的形状先对上，再比数值。
#[test]
fn encoder_matches_reference() {
    let reference = reference();
    let mel = reference.tensor("mel").expect("mel 存在");
    assert_eq!(mel.shape, vec![128, 200], "梅尔应是 128×200");
    let frames = mel.shape[1];

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
    let mut backend = MlxBackend::on_this_thread().expect("stream 在本线程");
    let tower = AudioTower::open(dir.to_str().expect("模型目录"), DType::F16).expect("音频塔权重");
    assert_eq!(tower.layer_count(), 18, "音频塔应是 18 层");

    let out = encode_mel(&mut backend, &tower, &mel.values, frames, DType::F16).expect("前向");

    // --- 先对形状 ---
    println!(
        "窗口边界：{:?}（共 {} 个窗）",
        out.cu_seqlens,
        out.cu_seqlens.len() - 1
    );
    assert_eq!(out.tokens, 26, "音频 token 数");
    assert_eq!(
        out.stem_chunk0_shape(),
        [16, 13, 480],
        "卷积茎第 0 块的形状"
    );
    assert_eq!(out.layer0_shape(), [1, 26, 896], "第 0 层输出的形状");
    assert_eq!(out.proj_shape(), [26, 1024], "投影输出的形状");
    assert_eq!(out.stem_chunk0.len(), 16 * 13 * 480);
    assert_eq!(out.layer0.len(), 26 * 896);
    assert_eq!(out.proj.len(), 26 * 1024);

    // --- 再对数值 ---
    let stem = reference.tensor("conv_stem_out").expect("存在");
    let layer0 = reference.tensor("encoder_layer0_out").expect("存在");
    let proj = reference.tensor("encoder_proj_out").expect("存在");

    let stem_tol = Margin::new(
        abs_max(&stem.values),
        CONV_STEM_UNITS,
        worst(&out.stem_chunk0, &stem.values),
    );
    let layer0_tol = Margin::new(
        abs_max(&layer0.values),
        ENCODER_LAYER0_UNITS,
        worst(&out.layer0, &layer0.values),
    );
    let proj_tol = Margin::new(
        abs_max(&proj.values),
        ENCODER_PROJ_UNITS,
        worst(&out.proj, &proj.values),
    );

    let stem_report = reference.compare("conv_stem_out", &out.stem_chunk0, stem_tol.tolerance());
    let layer0_report =
        reference.compare("encoder_layer0_out", &out.layer0, layer0_tol.tolerance());
    let proj_report = reference.compare("encoder_proj_out", &out.proj, proj_tol.tolerance());

    println!("参考绝对值上界、容差与余量（口径：上界 × 2^-11 × 单位数）：");
    for (name, margin) in [
        ("conv_stem_out", stem_tol),
        ("encoder_layer0_out", layer0_tol),
        ("encoder_proj_out", proj_tol),
    ] {
        println!("  {name:19} 上界 {:e}，{margin}", margin.reference_abs_max);
    }
    println!("{stem_report}");
    println!("{layer0_report}");
    println!("{proj_report}");

    assert!(
        stem_report.within_tolerance(),
        "卷积茎超出容差 {:e}：\n{stem_report}",
        stem_tol.tolerance()
    );
    assert!(
        layer0_report.within_tolerance(),
        "第 0 层超出容差 {:e}：\n{layer0_report}",
        layer0_tol.tolerance()
    );
    assert!(
        proj_report.within_tolerance(),
        "末尾投影超出容差 {:e}：\n{proj_report}",
        proj_tol.tolerance()
    );
    // 余量记在报告里：差值与容差各自通过 `compare` 算过一遍，这里要求两边同源，
    // 免得打印的数与断言的数走两个口径。
    assert_eq!(stem_report.max_abs_diff, Some(stem_tol.diff));
    assert_eq!(layer0_report.max_abs_diff, Some(layer0_tol.diff));
    assert_eq!(proj_report.max_abs_diff, Some(proj_tol.diff));
    assert!(stem_tol.within() && layer0_tol.within() && proj_tol.within());
}

/// 权重来源本身：`audio_tower.*` 的分组权重、卷积与 LayerNorm 都能读出来。
#[test]
fn audio_tower_weights_load_from_safetensors() {
    let metal = metallib();
    assert!(metal.is_file(), "metallib 不在 {}", metal.display());
    let dir = model_dir();
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let tower = AudioTower::open(dir.to_str().expect("模型目录"), DType::F16).expect("音频塔权重");
    assert_eq!(tower.layer_count(), 18);
}

/// 归因：把位置表分母、位置编号、窗宽各换一次，看差值从哪一步变大。
///
/// 这条不是「换口径把失败改成功」，反过来：它证明当前对上的口径不是碰上的。
/// 三项都换掉之后差值至少变大一档，说明对照能分辨这三种取法。
#[test]
fn wrong_encoder_options_make_the_difference_bigger() {
    let reference = reference();
    let mel = reference.tensor("mel").expect("mel 存在");
    let frames = mel.shape[1];
    let layer0_ref = &reference.tensor("encoder_layer0_out").expect("存在").values;
    let proj_ref = &reference.tensor("encoder_proj_out").expect("存在").values;

    let metal = metallib();
    assert!(metal.is_file(), "metallib 不在 {}", metal.display());
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let mut backend = MlxBackend::on_this_thread().expect("stream 在本线程");
    let dir = model_dir();
    let tower = AudioTower::open(dir.to_str().expect("模型目录"), DType::F16).expect("音频塔权重");

    let mut run = |options: EncoderOptions| {
        let out = encode_mel_with(
            &mut backend,
            &tower,
            &mel.values,
            frames,
            DType::F16,
            &options,
        )
        .expect("前向");
        let layer0 = worst(&out.layer0, layer0_ref);
        let proj = worst(&out.proj, proj_ref);
        (layer0, proj)
    };

    let exact = run(EncoderOptions::default());
    let half = run(EncoderOptions {
        position_denominator: PositionDenominator::HalfDim,
        ..EncoderOptions::default()
    });
    let global_positions = run(EncoderOptions {
        restart_positions_each_chunk: false,
        ..EncoderOptions::default()
    });
    let narrow_window = run(EncoderOptions {
        window_frames: 100,
        ..EncoderOptions::default()
    });

    for (name, (layer0, proj)) in [
        ("默认（half-1、每块从 0、窗 800 帧）", exact),
        ("分母用 half", half),
        ("位置跨块连续编号", global_positions),
        ("窗宽改成 100 帧", narrow_window),
    ] {
        println!("{name}：第 0 层最大差 {layer0:e}，末尾投影最大差 {proj:e}");
    }

    assert!(
        exact.0 < half.0 && exact.1 < half.1,
        "换掉位置表分母应让差值变大：默认 {:?}，分母 half {:?}",
        exact,
        half
    );
    assert!(
        exact.0 < global_positions.0 && exact.1 < global_positions.1,
        "换掉每块从 0 的编号应让差值变大：默认 {:?}，连续编号 {:?}",
        exact,
        global_positions
    );
    assert!(
        exact.0 <= narrow_window.0,
        "把窗宽改成 100 帧不该变小：默认 {:?}，窄窗 {:?}",
        exact,
        narrow_window
    );
}

fn worst(actual: &[f32], expected: &[f32]) -> f32 {
    assert_eq!(actual.len(), expected.len(), "两侧元素数应一致");
    actual
        .iter()
        .zip(expected)
        .fold(0.0f32, |acc, (got, want)| acc.max((got - want).abs()))
}
