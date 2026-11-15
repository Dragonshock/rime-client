//! 参考资产对照的测试入口。
//!
//! 不依赖 mlx-rs：读数走 std 文件 IO，比较走 `voice_parity::compare_slices`
//! 与 `voice_parity::compare_tokens`。
//! 梅尔前端（`voice-mel`）那一项是真对照。编码器、解码器、预填充 logits 与
//! 贪心 token 序列的实对照在 `voice-mlx/tests/encoder_parity.rs`、
//! `decoder_parity.rs` 与 `end_to_end_parity.rs`：那三项要真跑 MLX，而本箱子按
//! 052 的边界不链接 mlx-rs，所以只留 `#[ignore]` 的入口打印参考形状与那边记下的
//! 实测值。token 闸 `compare_tokens` 本身在这里有真测试。
//!
//! 跑法：
//!
//! ```text
//! cargo test -p voice-parity --offline
//! cargo test -p voice-parity --offline -- --ignored
//! ```

use std::fs;

use voice_mel::MelFrontend;
use voice_parity::{
    asset_root, compare_tokens, read_f32, read_pcm_i16, tolerance::MEL_TOLERANCE, Reference,
};

/// 梅尔容差：参考是 numpy 的 f32 实数 FFT，`voice-mel` 是 vDSP 的五条 80 点实数
/// DFT 再合并（见 notes/062），求和顺序不同，差值与 f16 存储无关，所以这一条用
/// 绝对容差。数在 `voice_parity::tolerance::MEL_TOLERANCE` 定义（全仓容差口径
/// 只有那一处），050 的 128×50 对照给它留了两倍多余量，062 实测差 6.866455e-5。
const MEL_ABS_TOLERANCE: f32 = MEL_TOLERANCE;

fn reference() -> Reference {
    Reference::load().unwrap_or_else(|error| {
        panic!("载入参考资产失败：{error}（目录：{}）", asset_root().display())
    })
}

/// 直接读资产文件，不经 manifest：给「清单说 f32 小端」这条口径做交叉核对。
fn read_asset(file: &str) -> Vec<u8> {
    let path = asset_root().join(file);
    fs::read(&path).unwrap_or_else(|error| panic!("读不到 {}：{error}", path.display()))
}

// ---------------------------------------------------------------------------
// 已实现：梅尔前端
// ---------------------------------------------------------------------------

/// `voice-mel` 对 `probe_pcm.bin` 的输出，与参考 `mel.bin`（128×200）逐元素比。
///
/// 输入是 32000 个 i16 采样（2 秒），参考帧数 200。参考梅尔是编码器吃之前的
/// f32 值，因此这里不需要先做 fp16 截断。
#[test]
fn mel_matches_reference() {
    let reference = reference();
    let pcm = read_pcm_i16(&read_asset("probe_pcm.bin"));
    assert_eq!(pcm.len(), 32_000, "探针应是 32000 个采样");

    let frontend = MelFrontend::new();
    let actual = frontend.log_mel(&pcm);
    let report = reference.compare("mel", &actual, MEL_ABS_TOLERANCE);

    println!("{report}");
    println!("实测最大绝对差：{:?}", report.max_abs_diff);
    if let Some(diff) = report.max_abs_diff {
        // 梅尔不走 f16 折算（见 tolerance 模块头），余量只是容差除以实测差。
        let room = if diff == 0.0 {
            "无穷（差为 0）".to_string()
        } else {
            format!("{:.2} 倍", MEL_ABS_TOLERANCE / diff)
        };
        println!(
            "梅尔：实测差 {diff:e}，容差 {MEL_ABS_TOLERANCE:e}，余量 {room}"
        );
    }

    assert_eq!(
        report.length_delta, 0,
        "梅尔帧数应与参考一致：参考 {} 个元素，实际 {}",
        report.expected_len, report.actual_len
    );
    assert!(
        report.within_tolerance(),
        "梅尔超出容差 {MEL_ABS_TOLERANCE:e}：\n{report}"
    );
}

/// 资产生自身的一致性：形状乘积等于落盘元素数、dtype 写着 float32、没有 NaN/Inf。
#[test]
fn assets_match_manifest() {
    let reference = reference();
    let mut keys: Vec<String> = reference.keys().map(|key| key.to_string()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "conv_stem_out",
            "encoder_layer0_out",
            "encoder_proj_out",
            "greedy_tokens_3",
            "mel",
            "prefill_logits_64",
        ],
        "manifest 的键集合与 051 记的六项一致"
    );

    for key in reference.keys() {
        let tensor = reference.tensor(key).expect("键来自自身");
        assert_eq!(
            tensor.element_count(),
            tensor.values.len(),
            "{key} 的形状 {} 与落盘 {} 个元素不一致",
            tensor.shape_text(),
            tensor.values.len()
        );
        assert_eq!(tensor.dtype, "float32", "{key} 的 dtype");
        let bytes = read_asset(&tensor.file);
        assert_eq!(
            bytes.len(),
            tensor.values.len() * 4,
            "{key} 的字节数应是元素数的 4 倍"
        );
        assert!(
            tensor.values.iter().all(|value| value.is_finite()),
            "{key} 含 NaN 或 Inf"
        );
    }

    // 直接读文件与按 manifest 读，必须得到同一份数。
    let direct = read_f32(&read_asset("mel.bin"));
    let via_manifest = &reference.tensor("mel").expect("mel 存在").values;
    assert_eq!(direct, *via_manifest, "两条读数路径应得到同一份 mel");
}

/// 参考内部的交叉核对：`greedy_tokens_3.bin` 与 `manifest.greedy.tokens` 一致。
/// 这条不涉及 Rust 实现，只保证对照用的两个口径没有被改歪。
#[test]
fn reference_greedy_tokens_are_consistent() {
    let reference = reference();
    let from_bin: Vec<u32> = reference
        .tensor("greedy_tokens_3")
        .expect("greedy_tokens_3 存在")
        .values
        .iter()
        .map(|value| *value as u32)
        .collect();
    assert_eq!(from_bin, reference.greedy_tokens());
    assert_eq!(from_bin, vec![106_287, 1_773, 151_645]);
    assert_eq!(reference.prompt_tokens().len(), 44, "提示应为 44 个 token");
    assert_eq!(reference.sample_rate(), Some(16_000));
}

// ---------------------------------------------------------------------------
// 入口：打开时打印参考形状与实对照记下的值
// ---------------------------------------------------------------------------

/// 编码器入口：三层卷积茎 → 18 层音频塔 → `ln_post`/`proj1`/`proj2`。
///
/// 要对比的三件参考量：
/// - `conv_stem_out`（16, 13, 480）：参考 NHWC 的第 0 块，送进 `conv_out` 前会
///   transpose 成 (13, 480*16)；
/// - `encoder_layer0_out`（1, 26, 896）：第 0 层含残差的输出；
/// - `encoder_proj_out`（26, 1024）：注入文本嵌入的那份音频特征。
///
/// 三项的实对照在 `voice-mlx/tests/encoder_parity.rs`：编码器要真跑 MLX，
/// 而本箱子按 052 的边界不链接 mlx-rs。这里保留入口只打印参考形状与实测值，
/// 免得「差值」变成一句没有出处的断言。
#[test]
#[ignore = "实对照在 voice-mlx/tests/encoder_parity.rs：本箱子不链接 MLX"]
fn encoder_matches_reference() {
    let reference = reference();
    for key in ["conv_stem_out", "encoder_layer0_out", "encoder_proj_out"] {
        let tensor = reference.tensor(key).expect("参考项存在");
        println!(
            "{key}: shape={} dtype={} source={}",
            tensor.shape_text(),
            tensor.dtype,
            tensor.source
        );
    }
    println!(
        "当前差值（2026-11-14，见 053）：conv_stem_out 0、encoder_layer0_out 3.90625e-3、\
         encoder_proj_out 1.1855885e-3；实对照在 voice-mlx/tests/encoder_parity.rs"
    );
    unimplemented!("编码器对照在 voice-mlx 的测试里跑：那边才链接 MLX，本箱子只用 std 读资产");
}

/// 解码器入口：文本塔的逐层前向。
///
/// 参考资产里没有解码器层输出，所以这个入口的可对比面只有两处间接量：
/// `prefill_logits_64`（最后位置的 logits 前 64 维）与 `greedy_tokens_3`。
/// 要做逐层对照，得先补一份解码器层输出资产——这会改导出脚本，不在本箱子范围。
#[test]
#[ignore = "实对照在 voice-mlx/tests/decoder_parity.rs：本箱子不链接 MLX"]
fn decoder_matches_reference() {
    let reference = reference();
    println!(
        "参考资产里与解码器相关的项：prefill_logits_64 shape={}、greedy_tokens_3 shape={}、prompt.tokens 长度={}",
        reference.tensor("prefill_logits_64").expect("存在").shape_text(),
        reference.tensor("greedy_tokens_3").expect("存在").shape_text(),
        reference.prompt_tokens().len()
    );
    println!(
        "当前差值（2026-11-14，见 054）：28 层文本塔能跑通，末尾 logits 前 64 维最大绝对差\
         4.6875e-2（本仓编码器输出注入）/ 3.125e-2（注入参考 encoder_proj_out），\
         全词表 argmax = 106287 与参考贪心第一个 token 一致"
    );
    println!(
        "仍缺的对照面：参考只落了 layers[0] 的音频塔输出，没有文本塔任何一层的隐层，\
         所以 28 层只能经末尾 logits 间接约束"
    );
    unimplemented!("解码器对照在 voice-mlx 的测试里跑：那边才链接 MLX，本箱子只用 std 读资产");
}

/// logits 入口：最后一个提示位置的 `lm_head` 输出前 64 维。
///
/// 参考只落了 64 维（全词表 151936 维），因此这项只能比前 64 维数值，
/// 不能用来判断下一个 token 是哪个。
#[test]
#[ignore = "实对照在 voice-mlx/tests/decoder_parity.rs：本箱子不链接 MLX"]
fn logits_match_reference() {
    let reference = reference();
    let tensor = reference.tensor("prefill_logits_64").expect("存在");
    println!(
        "prefill_logits_64: shape={} dtype={} source={}",
        tensor.shape_text(),
        tensor.dtype,
        tensor.source
    );
    println!(
        "参考前 8 维：{:?}",
        &tensor.values[..8.min(tensor.values.len())]
    );
    println!(
        "当前差值（2026-11-14，见 054）：最大绝对差 4.6875e-2（下标 0，实际 19.75，\
         参考 19.703125），另两个位置各有 3.125e-2；参考绝对值上界 21，\
         容差 8.203125e-2 = 上界 × 2^-11 × 8"
    );
    unimplemented!("预填充 logits 的实对照在 voice-mlx/tests/decoder_parity.rs：本箱子不链接 MLX");
}

/// token 序列入口：贪心解码前 3 个 token 的 id。
///
/// 参考：`[106287, 1773, 151645]`，用 f32 存整数。第三位是 EOS（151645），
/// 参考自带的 `generate_with_info` 会剥掉它，这里按资产原样比。
///
/// 实对照在 `voice-mlx/tests/end_to_end_parity.rs`：token 序列要真跑编码器、
/// 预填充与逐 token 贪心，本箱子按 052 的边界不链接 MLX。那边用本箱子的
/// [`compare_tokens`] 作闸，失败时把实际与期望两串一起打出来。
#[test]
#[ignore = "实对照在 voice-mlx/tests/end_to_end_parity.rs：本箱子不链接 MLX"]
fn greedy_tokens_match_reference() {
    let reference = reference();
    let expected = reference.greedy_tokens();
    println!("greedy_tokens_3 参考：{expected:?}");
    println!(
        "贪心文本：{:?}",
        reference
            .manifest()
            .get("greedy")
            .and_then(|value| value.get("text_from_tokens"))
            .and_then(|value| value.as_str())
            .unwrap_or("")
    );
    println!(
        "本箱只能核对参考自己：`reference_greedy_tokens_are_consistent` 已对上 bin 与 manifest；\
         实际序列由 voice-mlx 那条路径产出"
    );
}

/// token 闸本身的行为：逐位比、长度不等不算过、报告里带两侧序列。
#[test]
fn token_gate_reports_both_sequences() {
    let expected = [106_287u32, 1_773, 151_645];
    let exact = compare_tokens("greedy_tokens_3", &expected, &expected);
    assert!(exact.matches(), "{exact}");
    assert_eq!(exact.matched_prefix, 3);

    let short = compare_tokens("greedy_tokens_3", &expected, &expected[..2]);
    assert!(!short.matches(), "长度不等不能算过");
    assert!(short.first_mismatch.is_none());
    assert_eq!(short.matched_prefix, 2);

    let deviated = compare_tokens("greedy_tokens_3", &expected, &[106_287, 1_773, 151_643]);
    assert!(!deviated.matches());
    let hit = deviated.first_mismatch.expect("第三位不同");
    assert_eq!(hit.index, 2);
    assert_eq!(hit.actual, 151_643);
    assert_eq!(hit.expected, 151_645);
    let text = deviated.to_string();
    assert!(text.contains("期望"), "{text}");
    assert!(text.contains("实际"), "{text}");
}
