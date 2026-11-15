//! 容差口径：整个仓库只在这里定义一次，别处只引用，不再各写各的。
//!
//! 数值闸的容差一律写成同一个式子：
//!
//! ```text
//! 容差 = 参考张量的绝对值上界 × 2^-11 × 允许的单位数
//! ```
//!
//! 三段的依据：
//!
//! 1. **参考是 f16**（`manifest.json` 的 `model_dtype`，`dump_reference.py` 用
//!    `astype(float16)` 落盘）。f16 有 10 位尾数，`[2^e, 2^(e+1))` 区间里的相邻值
//!    间距是 `2^(e-10)`；对量级为 `m` 的元素，间距落在 `[m·2^-10, m·2^-9)` 之间。
//!    取间距的一半作为一次存储的舍入上界，就是 `m·2^-11`。所以 `2^-11`（4.8828125e-4）
//!    是「一个元素按其自身量级折算的半个 f16 间距」的上界。
//! 2. **尺度取参考张量的绝对值上界，不逐元素取。** 逐元素的半间距要每个元素各算一遍，
//!    报告里就没法用一个数说清；而且元素的量级总落在 `[上界/2, 上界]`，用上界折算
//!    只会把容差放宽不到 2 倍，不会把「差半个间距」和「差几层」混为一谈。
//!    这就是「上界 × 2^-11 × 倍数」里前两段的来历。
//! 3. **倍数（单位数）按层数与实测取。** 每一层往隐层里存一次 f16，就叠一次不超过
//!    1 个单位的舍入；残差是加法、舍入有正有负，所以不是「层数就是倍数」，而是
//!    按实测折出来的单位数往上取一档整值。各闸取多少、实测多少，写在下面每个常量
//!    的注释里；060 记了当次的逐闸实测差值与余量。
//!
//! 两条不许做的事：
//!
//! - 不为了让失败变成功去放宽单位数。差超了就是超了，先查算术。
//! - 不把不同闸的容差合成一个数。每闸的参考张量、层数、求和长度都不同。
//!
//! 梅尔那一项是唯一的例外：参考是 numpy 的 f32 实数 FFT，`voice-mel` 是 vDSP 的
//! 五条 80 点实数 DFT 再合并（见 notes/062），两边求和顺序不同，差值与 f16 存储
//! 无关，所以它用绝对容差 [`MEL_TOLERANCE`]，并在注释里写明出处。

use std::fmt;

/// 一个 f16 单位：`2^-11 ≈ 4.8828125e-4`。
///
/// 含义是「元素按其自身量级折算的半个 f16 间距」的上界，见模块头的第 1 条。
pub const F16_UNIT: f32 = 1.0 / 2048.0;

/// 音频塔卷积茎（`conv_stem_out`）：三层卷积 + 三次 GELU 的 f16 存储。
/// 实测差 0（060 复测仍为 0），给 4 个单位。
pub const CONV_STEM_UNITS: f32 = 4.0;

/// 音频塔第 0 层（`encoder_layer0_out`）：一次双向注意力 + 两层前馈。
/// 实测 1.2 个单位（3.90625e-3 对 6.6328125 的上界），给 4 个单位。
pub const ENCODER_LAYER0_UNITS: f32 = 4.0;

/// 音频塔末尾投影（`encoder_proj_out`）：18 层全走完，每层叠一次 f16 舍入。
/// 实测 19.2 个单位（1.1855885e-3 对 1.2658691e-1 的上界），给 32 个单位。
/// 32 是给「18 层每层最多 1 个单位」再留一档整值，不是按实测贴上去的；
/// 实测已接近这个数，所以这一闸的余量只有 1.67 倍。
pub const ENCODER_PROJ_UNITS: f32 = 32.0;

/// 解码器预填充 logits 首 64 维（`prefill_logits_64`）：28 层文本塔 + 末尾 RMSNorm
/// + `lm_head` 的 1024 项点积。实测 7.6 个单位（本仓编码器注入）/ 6.1 个单位
/// （注入参考 `encoder_proj_out`），给 8 个单位。
///
/// 没有按「28 层各一次」的 28 个单位取：残差是加法，舍入有正有负，不是线性累加；
/// 28 个单位会让闸失去意义（当前实测 7.6 个单位，28 个单位等于 3.7 倍余量）。
pub const PREFILL_LOGITS_UNITS: f32 = 8.0;

/// 梅尔前端（`mel`）：绝对容差 1e-3，不折算 f16 单位。
///
/// 参考的梅尔走 numpy 的 f32 实数 FFT，`voice-mel` 走 vDSP 的五条 80 点实数 DFT
/// 再合并，两边逐帧求和顺序不同，差值与 f16 存储无关。这个数来自 050 那次 128×50
/// 的对照：直接 DFT 时最大绝对差 3.6e-4 左右，取 1e-3 留两倍多余量。
///
/// 062 换了 FFT 之后同一份 128×200 资产复测：3.2138824e-4（直接 DFT）→
/// 6.866455e-5（vDSP），余量从 3.11 倍变成 14.56 倍。容差没有跟着改：这一项是
/// 「与参考不同算术」的绝对容差，实测差变小只是余量变大，不是放宽松紧的理由。
pub const MEL_TOLERANCE: f32 = 1e-3;

/// 按「参考绝对值上界 × 一个 f16 单位 × 倍数」算容差。全仓唯一的容差式子。
pub fn f16_tolerance(reference_abs_max: f32, units: f32) -> f32 {
    reference_abs_max * F16_UNIT * units
}

/// 把实测差折成 f16 单位数：`diff / (上界 × 2^-11)`。报告里用来说「差了几层」。
pub fn f16_units(reference_abs_max: f32, diff: f32) -> f32 {
    diff / (reference_abs_max * F16_UNIT)
}

/// 一次闸的实测账：差值、容差、折成的单位数、余量。
///
/// 闸的打印与断言都用它，免得「差多少、容差多少、还剩多少」散在各处各算一遍。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margin {
    /// 参考张量的绝对值上界，容差的尺度。
    pub reference_abs_max: f32,
    /// 允许的单位数。
    pub allowed_units: f32,
    /// 实测最大绝对差。
    pub diff: f32,
}

impl Margin {
    /// 按「上界、允许单位数、实测差」记一笔账。
    pub fn new(reference_abs_max: f32, allowed_units: f32, diff: f32) -> Self {
        Self {
            reference_abs_max,
            allowed_units,
            diff,
        }
    }

    /// 容差。
    pub fn tolerance(&self) -> f32 {
        f16_tolerance(self.reference_abs_max, self.allowed_units)
    }

    /// 实测差折成的单位数。
    pub fn used_units(&self) -> f32 {
        f16_units(self.reference_abs_max, self.diff)
    }

    /// 余量：容差除以实测差。实测差为 0 时是无穷大（没有可用的比值）。
    pub fn times(&self) -> f32 {
        self.tolerance() / self.diff
    }

    /// 是否在容差内。与 `Report::within_tolerance` 同一口径：差等于容差算过。
    pub fn within(&self) -> bool {
        self.diff <= self.tolerance()
    }
}

impl fmt::Display for Margin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "实测差 {:e}（{:.1} 个单位），容差 {:e}（{} 个单位），余量 ",
            self.diff,
            self.used_units(),
            self.tolerance(),
            self.allowed_units
        )?;
        if self.diff == 0.0 {
            write!(f, "无穷（差为 0）")
        } else {
            write!(f, "{:.2} 倍", self.times())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `2^-11` 的十进制值写死在这里：这个数一改，所有闸的松紧都跟着变。
    #[test]
    fn f16_unit_is_two_to_the_minus_eleven() {
        assert_eq!(F16_UNIT, 2f32.powi(-11));
        assert_eq!(F16_UNIT, 4.882_812_5e-4);
    }

    /// 容差式子只有一条：上界 × 2^-11 × 倍数，逐项可验。
    #[test]
    fn tolerance_is_upper_times_unit_times_units() {
        let upper = 21.0f32;
        assert_eq!(f16_tolerance(upper, 8.0), upper * F16_UNIT * 8.0);
        // 054 记的那个数：上界 21，8 个单位 → 8.203125e-2。
        assert_eq!(f16_tolerance(upper, 8.0), 8.203_125e-2);
        // 倍数翻倍，容差翻倍；上界翻倍，容差也翻倍。
        assert_eq!(
            f16_tolerance(upper, 16.0),
            f16_tolerance(upper, 8.0) * 2.0
        );
        assert_eq!(
            f16_tolerance(upper * 2.0, 8.0),
            f16_tolerance(upper, 8.0) * 2.0
        );
    }

    /// 折单位与算容差互为逆运算，报告里的「几个单位」不是另一个口径。
    #[test]
    fn units_round_trip_with_tolerance() {
        let upper = 6.632_812_5f32;
        for units in [0.5f32, 4.0, 8.0, 32.0] {
            let tolerance = f16_tolerance(upper, units);
            assert_eq!(f16_units(upper, tolerance), units);
        }
        // 060 实测：1.1855885e-3 对 1.2658691e-1 的上界是 19.2 个单位。
        let used = f16_units(1.265_869_1e-1, 1.185_588_5e-3);
        assert!((used - 19.2).abs() < 0.1, "折出来是 {used} 个单位");
    }

    /// 余量 = 容差 / 实测差；差为 0 时不编造比值。
    #[test]
    fn margin_reports_the_remaining_room() {
        let hit = Margin::new(21.0, 8.0, 7.812_5e-2);
        assert_eq!(hit.tolerance(), 8.203_125e-2);
        assert!((hit.times() - 1.05).abs() < 0.01, "余量 {}", hit.times());
        assert!(hit.within(), "{hit}");

        let clean = Margin::new(2.025_390_6, 4.0, 0.0);
        assert_eq!(clean.used_units(), 0.0);
        assert!(clean.within());
        assert!(clean.to_string().contains("无穷"), "{clean}");

        let over = Margin::new(21.0, 8.0, 9.0e-2);
        assert!(!over.within());
        assert!(over.times() < 1.0);
    }

    /// 各闸的倍数都只升不降地用在这里：改一个常量就得改这条。
    #[test]
    fn gate_unit_counts_are_the_documented_ones() {
        assert_eq!(CONV_STEM_UNITS, 4.0);
        assert_eq!(ENCODER_LAYER0_UNITS, 4.0);
        assert_eq!(ENCODER_PROJ_UNITS, 32.0);
        assert_eq!(PREFILL_LOGITS_UNITS, 8.0);
        assert_eq!(MEL_TOLERANCE, 1e-3);
        // 同一副式子下，层数多的闸给的单位数不该比层数少的少。
        assert!(ENCODER_PROJ_UNITS >= ENCODER_LAYER0_UNITS);
        assert!(CONV_STEM_UNITS <= ENCODER_LAYER0_UNITS);
    }
}
