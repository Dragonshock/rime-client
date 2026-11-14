//! 128 维对数梅尔谱，与 Qwen3-ASR 的 `WhisperFeatureExtractor` 同口径。
//!
//! 参数：16 kHz、`n_fft = 400`、`hop = 160`、128 个频点、`fmin = 0`、`fmax = 8000`、
//! 周期 Hann 窗、居中反射补边。滤波器组与窗直接来自参考实现的资产文件，
//! 不在这里重新推导 Slaney 公式——公式一旦和参考差一点，权重就对不上。
//!
//! 三步整形与参考实现一致：功率谱取 `log10` 并截到 `1e-10`；丢掉最后一帧；
//! 先减去「全局最大值减 8」，再 `(x + 4) / 4`。

pub const SAMPLE_RATE: usize = 16_000;
pub const FFT_SIZE: usize = 400;
pub const HOP: usize = 160;
pub const MEL_BINS: usize = 128;
pub const FREQ_BINS: usize = FFT_SIZE / 2 + 1;

/// 参考实现的 `mel_filters.npz` 里的 `mel_128`，行优先 `(128, 201)`。
static MEL_FILTERS: &[u8] = include_bytes!("../assets/mel_filters_128.bin");
/// 参考实现的 `np.hanning(401)[:-1]`，长度 400。
static HANN: &[u8] = include_bytes!("../assets/hann_400.bin");

/// 预先算好的滤波器组与窗。每一帧只做一次 400 点 DFT。
pub struct MelFrontend {
    window: Vec<f32>,
    filters: Vec<f32>,
    cos_table: Vec<f32>,
    sin_table: Vec<f32>,
}

impl Default for MelFrontend {
    fn default() -> Self {
        Self::new()
    }
}

impl MelFrontend {
    pub fn new() -> Self {
        let window = decode_f32(HANN);
        let filters = decode_f32(MEL_FILTERS);
        assert_eq!(window.len(), FFT_SIZE, "窗长度应为 400");
        assert_eq!(
            filters.len(),
            MEL_BINS * FREQ_BINS,
            "滤波器组形状应为 128x201"
        );
        let mut cos_table = vec![0.0f32; FREQ_BINS * FFT_SIZE];
        let mut sin_table = vec![0.0f32; FREQ_BINS * FFT_SIZE];
        for freq in 0..FREQ_BINS {
            for n in 0..FFT_SIZE {
                let angle = 2.0 * std::f32::consts::PI * (freq * n) as f32 / FFT_SIZE as f32;
                cos_table[freq * FFT_SIZE + n] = angle.cos();
                sin_table[freq * FFT_SIZE + n] = -angle.sin();
            }
        }
        Self {
            window,
            filters,
            cos_table,
            sin_table,
        }
    }

    /// 一段 16 kHz 单声道样本。返回 `(128, frames)`，行优先，第 0 行是第 0 个频点。
    /// 帧数等于 `样本数 / 160`，与参考实现丢掉最后一帧之后的数量一致。
    pub fn log_mel(&self, pcm: &[i16]) -> Vec<f32> {
        let samples: Vec<f32> = pcm.iter().map(|value| *value as f32 / 32768.0).collect();
        self.log_mel_f32(&samples)
    }

    pub fn log_mel_f32(&self, samples: &[f32]) -> Vec<f32> {
        let kept = frame_count(samples.len());
        if kept == 0 {
            return Vec::new();
        }
        let padded = reflect_pad(samples, FFT_SIZE / 2);
        let mut out = vec![0.0f32; MEL_BINS * kept];
        let mut frame = vec![0.0f32; FFT_SIZE];
        let mut power = vec![0.0f32; FREQ_BINS];
        for index in 0..kept {
            let start = index * HOP;
            for n in 0..FFT_SIZE {
                frame[n] = padded[start + n] * self.window[n];
            }
            // 一帧一次 DFT，随后投到 128 个滤波器上。
            for freq in 0..FREQ_BINS {
                let base = freq * FFT_SIZE;
                // 用 f64 累加：参考实现走 FFT，求和顺序不同，f32 累加会多出 1e-4 级误差。
                let mut re = 0.0f64;
                let mut im = 0.0f64;
                for n in 0..FFT_SIZE {
                    let value = frame[n] as f64;
                    re += value * self.cos_table[base + n] as f64;
                    im += value * self.sin_table[base + n] as f64;
                }
                // 参考实现用功率谱（幅值平方）。
                power[freq] = (re * re + im * im) as f32;
            }
            for bin in 0..MEL_BINS {
                let row = bin * FREQ_BINS;
                let mut sum = 0.0f32;
                for freq in 0..FREQ_BINS {
                    let weight = self.filters[row + freq];
                    if weight != 0.0 {
                        sum += power[freq] * weight;
                    }
                }
                out[bin * kept + index] = sum;
            }
        }
        shape_log(&mut out);
        out
    }
}

/// STFT 帧数，含被保留的最后修剪。
pub fn frame_count(samples: usize) -> usize {
    if samples <= FFT_SIZE / 2 {
        return 0;
    }
    // 补边后共 `samples / HOP + 1` 帧，参考实现丢掉最后一帧。
    samples / HOP
}

fn decode_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// 与 `numpy.pad(x, pad, mode="reflect")` 同义：端点不重复。
fn reflect_pad(samples: &[f32], pad: usize) -> Vec<f32> {
    let len = samples.len();
    let mut out = Vec::with_capacity(len + pad * 2);
    // numpy 的 reflect 左边是 x[pad..=1] 倒序。
    for index in (1..=pad).rev() {
        let source = index.min(len - 1);
        out.push(samples[source]);
    }
    out.extend_from_slice(samples);
    for index in 1..=pad {
        let source = len.saturating_sub(1 + index);
        out.push(samples[source]);
    }
    out
}

/// 参考实现的三步整形：相对最大值截断 8，再按 `(x + 4) / 4` 归一。
fn shape_log(values: &mut [f32]) {
    let mut max = f32::NEG_INFINITY;
    for value in values.iter_mut() {
        let logged = value.max(1e-10).log10();
        *value = logged;
        if logged > max {
            max = logged;
        }
    }
    let floor = max - 8.0;
    for value in values.iter_mut() {
        *value = ((*value).max(floor) + 4.0) / 4.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static REFERENCE_PCM: &[u8] = include_bytes!("../assets/reference_pcm.bin");
    static REFERENCE_MEL: &[u8] = include_bytes!("../assets/reference_mel.bin");

    #[test]
    fn frame_count_matches_reference() {
        assert_eq!(frame_count(8_000), 50);
        assert_eq!(frame_count(16_000), 100);
        assert_eq!(frame_count(800), 5);
    }

    #[test]
    /// 与参考实现逐元素对照。参考走 FFT，这里走直接 DFT，求和顺序不同，
    /// 因此门槛是 1e-3 而不是逐位相同。
    fn matches_reference_mel_within_1e_3() {
        let pcm: Vec<i16> = REFERENCE_PCM
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let expected = decode_f32(REFERENCE_MEL);
        let frontend = MelFrontend::new();
        let actual = frontend.log_mel(&pcm);
        assert_eq!(
            actual.len(),
            expected.len(),
            "形状应与参考一致：{} 对 {}",
            actual.len(),
            expected.len()
        );
        let mut worst = 0.0f32;
        for (index, (a, b)) in actual.iter().zip(expected.iter()).enumerate() {
            let diff = (a - b).abs();
            if diff > worst {
                worst = diff;
            }
            assert!(diff < 1e-3, "第 {index} 个元素差 {diff}：{a} 对 {b}");
        }
        assert!(worst >= 0.0);
    }

    #[test]
    fn silence_is_finite_and_flat() {
        let frontend = MelFrontend::new();
        let mel = frontend.log_mel(&vec![0i16; 3_200]);
        assert!(mel.iter().all(|value| value.is_finite()));
        let min = mel.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = mel.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(max - min < 1e-3, "静音应是平的，实际 {min}..{max}");
    }
}
