//! 解码器预填充的实对照：`prefill_logits_64`。
//!
//! 参考资产由 `code/voice-oracle/dump_reference.py` 导出，参考激活是 float16。
//! 提示用 `manifest.prompt.tokens` 这 44 个 id（含 26 个音频占位 `151676`），
//! 占位处换成编码器末尾投影的行。对照两步：
//!
//! 1. 首 64 维逐元素最大差，与 `prefill_logits_64` 比。
//! 2. 全词表 argmax 是否等于参考贪心的第一个 token（`greedy.tokens[0]`）。
//!
//! 两条音频来源都跑：一条用本仓编码器对 `mel` 的输出，一条直接注入参考的
//! `encoder_proj_out`。后者把编码器的差排除在外，差值只属于解码器。
//!
//! 跑法见 `BUILD.txt` 与 `encoder_parity.rs` 文件头（三条链接参数缺一不可）：
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
//!   --test decoder_parity -- --nocapture
//! ```

#![cfg(feature = "metal")]

use std::path::PathBuf;

use voice_mlx::{
    encode_mel, AudioTower, DecoderOptions, MlxBackend, PrefillLogits, QuantPolicy, RopeMode,
    TextDecoder,
};
use voice_parity::{
    asset_root,
    tolerance::{f16_units, Margin, PREFILL_LOGITS_UNITS},
    Reference,
};
use voice_tensor::DType;

// 容差口径只在 `voice_parity::tolerance` 定义一处：
//
//   容差 = 参考张量的绝对值上界 × 2^-11 × 允许的单位数
//
// 这一闸的参考是 f16 logits，上界 21，单位数 8；取多少与实测多少都写在
// `voice_parity::tolerance::PREFILL_LOGITS_UNITS` 的注释里。本文件不写绝对容差。
/// 取多少维 logits，与资产一致。
const HEAD_DIMS: usize = 64;

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

fn abs_max(values: &[f32]) -> f32 {
    values
        .iter()
        .fold(0.0f32, |acc, value| acc.max(value.abs()))
}

/// 起一个 stream 与音频塔、解码器。三次测试共用同一条准备流程。
fn prepare() -> (MlxBackend, AudioTower, TextDecoder, Reference) {
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
    let backend = MlxBackend::on_this_thread().expect("stream 在本线程");
    let path = dir.to_str().expect("模型目录");
    let tower = AudioTower::open(path, DType::F16).expect("音频塔权重");
    let decoder = TextDecoder::open(path, DType::F16).expect("解码器权重");
    (backend, tower, decoder, reference())
}

/// 本仓编码器对参考 `mel` 的输出，行优先展平 `(26, 1024)`。
fn encoder_features(
    backend: &mut MlxBackend,
    tower: &AudioTower,
    reference: &Reference,
) -> Vec<f32> {
    let mel = reference.tensor("mel").expect("mel 存在");
    let out =
        encode_mel(backend, tower, &mel.values, mel.shape[1], DType::F16).expect("编码器前向");
    assert_eq!(out.proj.len(), 26 * 1024, "编码器末尾投影应是 26×1024");
    out.proj
}

fn prompt(reference: &Reference) -> Vec<u32> {
    let tokens = reference.prompt_tokens();
    assert_eq!(tokens.len(), 44, "参考提示是 44 个 token");
    assert_eq!(
        tokens.iter().filter(|id| **id == 151676).count(),
        26,
        "提示里有 26 个音频占位"
    );
    tokens
}

fn worst(actual: &[f32], expected: &[f32]) -> f32 {
    assert_eq!(actual.len(), expected.len(), "两侧元素数应一致");
    actual
        .iter()
        .zip(expected)
        .fold(0.0f32, |acc, (got, want)| acc.max((got - want).abs()))
}

/// 比一次预填充 logits：差值、容差与余量都由 `voice_parity::tolerance` 算。
fn compare_logits(reference: &Reference, logits: &PrefillLogits) -> Margin {
    let expected = reference.tensor("prefill_logits_64").expect("存在");
    assert_eq!(expected.values.len(), HEAD_DIMS);
    assert_eq!(logits.head.len(), HEAD_DIMS);
    let margin = Margin::new(
        abs_max(&expected.values),
        PREFILL_LOGITS_UNITS,
        worst(&logits.head, &expected.values),
    );
    let report = reference.compare("prefill_logits_64", &logits.head, margin.tolerance());
    println!("{margin}");
    println!("{report}");
    println!(
        "全词表 argmax：{}（logit {:e}）；参考贪心第一个 token：{}",
        logits.argmax,
        logits.argmax_value,
        reference.greedy_tokens().first().copied().unwrap_or(0)
    );
    // 打印的差值与断言的差值同源：两边都是同一对切片、同一个 fold。
    assert_eq!(report.max_abs_diff, Some(margin.diff));
    margin
}

/// 词嵌入表按行取的一致性：同一行只取它自己、和它在 16 行块里、
/// 和它在 64 行块里，三种取法给同一组数。
///
/// 这条支撑两件事：`lm_head` 分块算 logits 与整表算是同一组值；占位处的词嵌入
/// 行不读也不影响结果。
#[test]
fn embedding_rows_do_not_depend_on_the_block() {
    let (mut backend, _tower, mut decoder, _reference) = prepare();
    let alone = decoder.embedding_rows(&mut backend, &[7]).expect("单行");
    let in_sixteen = decoder
        .embedding_rows(&mut backend, &(0..16).collect::<Vec<_>>())
        .expect("16 行块");
    let in_sixty_four = decoder
        .embedding_rows(&mut backend, &(0..64).collect::<Vec<_>>())
        .expect("64 行块");
    assert_eq!(alone.len(), 1024);
    assert_eq!(&in_sixteen[7 * 1024..8 * 1024], &alone[..]);
    assert_eq!(&in_sixty_four[7 * 1024..8 * 1024], &alone[..]);

    // 分两段读与一次读整块，拼出来的数一致。
    let split_a = decoder
        .embedding_rows(&mut backend, &(0..32).collect::<Vec<_>>())
        .expect("前 32 行");
    let split_b = decoder
        .embedding_rows(&mut backend, &(32..64).collect::<Vec<_>>())
        .expect("后 32 行");
    let mut joined = split_a;
    joined.extend_from_slice(&split_b);
    assert_eq!(joined, in_sixty_four, "两段读拼起来应与整块读一致");
}

/// 主闸：本仓编码器输出注入 + 44 token 提示，对 `prefill_logits_64` 与贪心首 token。
#[test]
fn decoder_prefill_matches_reference() {
    let (mut backend, tower, mut decoder, reference) = prepare();
    let tokens = prompt(&reference);
    let audio = encoder_features(&mut backend, &tower, &reference);

    let logits = decoder
        .prefill(&mut backend, &tokens, &audio, HEAD_DIMS)
        .expect("预填充");
    assert_eq!(logits.tokens, 44);
    assert_eq!(logits.vocab, 151936);
    assert_eq!(logits.head.len(), HEAD_DIMS);

    let margin = compare_logits(&reference, &logits);
    assert!(
        margin.within(),
        "首 {HEAD_DIMS} 维最大差 {:e} 超出容差 {:e}",
        margin.diff,
        margin.tolerance()
    );
    let first = reference.greedy_tokens()[0];
    assert_eq!(
        logits.argmax as u32, first,
        "全词表 argmax 应与参考贪心第一个 token 一致"
    );

    // 两条归因断言：主闸对上的口径必须能被这两处错误辨出来。
    // 方向与「放宽口径让失败变成功」相反——换成错的必须让差值变大。
    let expected = &reference.tensor("prefill_logits_64").expect("存在").values;
    let upper = abs_max(expected);
    let mut wrong_diffs = Vec::new();
    for (name, options) in [
        (
            "RoPE 只乘 sin",
            DecoderOptions {
                rope: RopeMode::SinOnly,
                ..DecoderOptions::default()
            },
        ),
        (
            "去掉每头 q/k 归一化",
            DecoderOptions {
                qk_norm: false,
                ..DecoderOptions::default()
            },
        ),
    ] {
        let wrong = decoder
            .prefill_with(&mut backend, &tokens, &audio, HEAD_DIMS, &options)
            .expect("错口径预填充");
        let diff = worst(&wrong.head, expected);
        println!(
            "{name}：首 {HEAD_DIMS} 维最大差 {diff:e}（{:.1} 个单位），argmax {}",
            f16_units(upper, diff),
            wrong.argmax
        );
        assert!(
            margin.diff < diff,
            "{name} 必须让差值变大：默认 {:e}，换错之后 {:e}",
            margin.diff,
            diff
        );
        // 「变差一点」不够：换错之后要直接落到容差外，否则这条归因就分辨不出对错。
        assert!(
            diff > margin.tolerance(),
            "{name} 换错之后仍在容差 {:e} 内（实测 {:e}），主闸分辨不出这个错",
            margin.tolerance(),
            diff
        );
        wrong_diffs.push(diff);
    }
    assert_eq!(wrong_diffs.len(), 2, "两条归因断言都要跑过");
}

/// 归因：把音频换成参考自己的 `encoder_proj_out`，差值只属于解码器。
#[test]
fn decoder_with_reference_audio_features() {
    let (mut backend, _tower, mut decoder, reference) = prepare();
    let tokens = prompt(&reference);
    let audio = reference
        .tensor("encoder_proj_out")
        .expect("存在")
        .values
        .clone();

    let logits = decoder
        .prefill(&mut backend, &tokens, &audio, HEAD_DIMS)
        .expect("预填充");
    let margin = compare_logits(&reference, &logits);
    assert!(
        margin.within(),
        "注入参考音频特征后最大差 {:e} 超出容差 {:e}",
        margin.diff,
        margin.tolerance()
    );
    assert_eq!(logits.argmax as u32, reference.greedy_tokens()[0]);
}

/// 归因：换掉 RoPE 与 q/k 归一化各一次，看差值是否变大。
///
/// 这条不是「换口径把失败改成功」，反过来：它证明当前对上的口径不是碰上的。
#[test]
fn wrong_decoder_options_make_the_difference_bigger() {
    let (mut backend, _tower, mut decoder, reference) = prepare();
    let tokens = prompt(&reference);
    let audio = reference
        .tensor("encoder_proj_out")
        .expect("存在")
        .values
        .clone();
    let expected = reference
        .tensor("prefill_logits_64")
        .expect("存在")
        .values
        .clone();

    let mut run = |backend: &mut MlxBackend, options: DecoderOptions| {
        let logits = decoder
            .prefill_with(backend, &tokens, &audio, HEAD_DIMS, &options)
            .expect("预填充");
        (worst(&logits.head, &expected), logits.argmax)
    };

    let exact = run(&mut backend, DecoderOptions::default());
    let sin_only = run(
        &mut backend,
        DecoderOptions {
            rope: RopeMode::SinOnly,
            ..DecoderOptions::default()
        },
    );
    let no_norm = run(
        &mut backend,
        DecoderOptions {
            qk_norm: false,
            ..DecoderOptions::default()
        },
    );

    for (name, (diff, argmax)) in [
        ("默认（rotate_half + q/k 归一化）", exact),
        ("RoPE 只乘 sin", sin_only),
        ("不做 q/k 归一化", no_norm),
    ] {
        println!(
            "{name}：首 {HEAD_DIMS} 维最大差 {diff:e}（{:.1} 个单位），argmax {argmax}",
            f16_units(abs_max(&expected), diff)
        );
    }

    assert!(
        exact.0 < sin_only.0,
        "换成只乘 sin 应让差值变大：默认 {:?}，只乘 sin {:?}",
        exact,
        sin_only
    );
    assert!(
        exact.0 < no_norm.0,
        "不做 q/k 归一化应让差值变大：默认 {:?}，无归一化 {:?}",
        exact,
        no_norm
    );

    // 除了「更差」，还要「差到容差外」：否则归因与主闸各说各话。
    let margin = Margin::new(abs_max(&expected), PREFILL_LOGITS_UNITS, exact.0);
    assert!(margin.within(), "默认口径应在容差内：{margin}");
    for (name, (diff, _argmax)) in [("RoPE 只乘 sin", sin_only), ("不做 q/k 归一化", no_norm)] {
        assert!(
            diff > margin.tolerance(),
            "{name} 换错之后仍在容差 {:e} 内（实测 {:e}）",
            margin.tolerance(),
            diff
        );
    }
}

/// 归因：七个线性层走量化矩阵乘，与先拼 f16 再乘，差在哪里、差多少。
///
/// 四个口径只差「哪一组投影走量化矩阵乘」，其余（提示、音频特征、RoPE、
/// `lm_head` 分块）完全一样。每个口径都对同一份参考资产求首 64 维最大差，
/// 折成 054 记的 f16 单位，于是能看出偏差是哪一组带来的。
#[test]
fn quantized_paths_are_compared_group_by_group() {
    let (mut backend, tower, _decoder, reference) = prepare();
    let tokens = prompt(&reference);
    let audio = encoder_features(&mut backend, &tower, &reference);
    let expected = reference
        .tensor("prefill_logits_64")
        .expect("存在")
        .values
        .clone();
    let upper = abs_max(&expected);
    let tolerance = voice_parity::tolerance::f16_tolerance(upper, PREFILL_LOGITS_UNITS);
    let path = model_dir();
    let path = path.to_str().expect("模型目录");

    let mut measured: Vec<(&str, f32, usize)> = Vec::new();
    for (name, policy) in [
        ("两组都走量化矩阵乘（产品口径）", QuantPolicy::default()),
        (
            "只走注意力（MLP 先拼 f16）",
            QuantPolicy {
                attention: true,
                mlp: false,
            },
        ),
        (
            "只走 MLP（注意力先拼 f16）",
            QuantPolicy {
                attention: false,
                mlp: true,
            },
        ),
        ("一组都不走（全部先拼 f16，057 的做法）", QuantPolicy::dequantized()),
    ] {
        let mut decoder =
            TextDecoder::open_with(path, DType::F16, policy).expect("解码器权重");
        assert_eq!(decoder.policy(), policy);
        let logits = decoder
            .prefill(&mut backend, &tokens, &audio, HEAD_DIMS)
            .expect("预填充");
        let diff = logits
            .head
            .iter()
            .zip(&expected)
            .fold(0.0f32, |acc, (got, want)| acc.max((got - want).abs()));
        println!(
            "{name}：首 {HEAD_DIMS} 维最大差 {diff:e}（{:.1} 个单位），argmax {}",
            f16_units(upper, diff),
            logits.argmax
        );
        assert!(
            diff <= tolerance,
            "{name} 的最大差 {diff:e} 超出容差 {tolerance:e}"
        );
        assert_eq!(
            logits.argmax as u32,
            reference.greedy_tokens()[0],
            "{name} 的首个贪心 token 应与参考一致"
        );
        measured.push((name, diff, logits.argmax));
    }
    println!(
        "容差 {tolerance:e}（{PREFILL_LOGITS_UNITS} 个单位，上界 {upper:e} × 2^-11 × {PREFILL_LOGITS_UNITS}）"
    );
    assert_eq!(measured.len(), 4);
}

/// 权重来源本身：`model.*` 的 4 位分组线性层、词嵌入表与两个 RMSNorm 都能读出来。
#[test]
fn text_decoder_weights_load_from_safetensors() {
    let metal = metallib();
    assert!(metal.is_file(), "metallib 不在 {}", metal.display());
    let dir = model_dir();
    MlxBackend::prepare(metal.to_str().expect("路径")).expect("MLX stream");
    let decoder =
        TextDecoder::open(dir.to_str().expect("模型目录"), DType::F16).expect("解码器权重");
    assert_eq!(decoder.layer_count(), 28, "文本塔是 28 层");
}
