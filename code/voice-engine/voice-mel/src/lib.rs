//! 128 维对数梅尔谱，与 Qwen3-ASR 的 `WhisperFeatureExtractor` 同口径。
//!
//! 参数：16 kHz、`n_fft = 400`、`hop = 160`、128 个频点、`fmin = 0`、`fmax = 8000`、
//! 周期 Hann 窗、居中反射补边。滤波器组与窗直接来自参考实现的资产文件，
//! 不在这里重新推导 Slaney 公式——公式一旦和参考差一点，权重就对不上。
//!
//! 三步整形与参考实现一致：功率谱取 `log10` 并截到 `1e-10`；丢掉最后一帧；
//! 先减去「全局最大值减 8」，再 `(x + 4) / 4`。
//!
//! # 400 点实数 DFT 怎么走 vDSP
//!
//! macOS 上用 Accelerate 的 `vDSP_DFT_zrop`（打包实数 DFT）。400 点不能直接交给它：
//! vDSP 只实现长度 `2^n` 与 `f·2^n`（`f` 取 3、5、15，实数版要求 `n ≥ 4`），
//! 400 = 5²·2⁴ 与 200 = 5²·2³ 都不在其中，`vDSP_DFT_zrop_CreateSetup(400)` 返回空指针。
//! 不补零到 512 也不补到 480：补零之后 vDSP 算的是 `2πk/512` 或 `2πk/480` 上的
//! 离散时间傅里叶变换采样，而参考算的是 `2πk/400`——频点错位，得靠插值往回挪，
//! 插值误差比现在 1e-3 的闸还大。所以按 400 = 5 × 80 做一级时分抽取：
//!
//! ```text
//! X[k] = Σ_{r<5} W_400^{r k} · A_r[k mod 80]
//! A_r[m] = Σ_{m<80} x[5m + r] · W_80^{m k}       （五条 80 点实数 DFT）
//! ```
//!
//! 80 = 5 · 2⁴ 是 vDSP 支持的长度，五条支路各算一次 80 点实数 DFT，再乘旋转因子
//! 相加（201 个频点 × 5 项，f64 累加）。频点仍是 `2πk/400`，与参考同一网格，
//! 因此不需要插值，也不必改容差。
//!
//! 非 macOS 目标退化成逐点直接 DFT：这条路径只保证能编译、能算对，不追求速度。
//!
//! # 增量接口
//!
//! 一次按住里每 2 秒要出一段梅尔。参考实现在整形之前先减「全局最大值减 8」，
//! 那个最大值是全段的函数——这是唯一一处「要看到未来/整段」的地方，但它只作用在
//! **整形**这一步，作用方式是逐元素的单调截断。所以增量的切法是把两件事分开：
//!
//! - **原始帧**（幂谱投到滤波器之后、`log10` 与截断之前）：一帧只取决于它自己那
//!   400 个样本。窗一旦完全落在已收样本里，这 400 个样本就再也不会变，值可以缓存。
//! - **整形**：`(max(log10(x), 全局最大 log10 - 8) + 4) / 4` 逐元素、按当前前缀的
//!   那个 max 套用。这个 max 是增量维护的（已定帧的最大值单调不减，末尾那一帧
//!   每次按新的右端反射重算 128 个值），所以取值时套用的 max 与整段重算的 max 相同。
//!
//! 由此每步的实际开销分两块，都与已收音频长度无关：
//!
//! - 新样本的帧：`新样本数 / 160` 次 400 点 DFT 加滤波器组；
//! - 要交出去的那一段的整形：只算被编码的那几列（≤ 800 帧），不碰历史。
//!
//! 唯一随长度增长的是缓存本身（每帧 128 个 f32）。
//!
//! 逐帧等价的理由在 [`MelStream::push`] 上写着：三件事各自都是精确的，合起来
//! 与「整段重算再整形」逐位相同（`incremental_matches_whole_utterance_bitwise`
//! 直接断言这一点）。

use std::time::Instant;

pub const SAMPLE_RATE: usize = 16_000;
pub const FFT_SIZE: usize = 400;
pub const HOP: usize = 160;
pub const MEL_BINS: usize = 128;
pub const FREQ_BINS: usize = FFT_SIZE / 2 + 1;

/// 400 = `BRANCHES` × `SUB_FFT`，一级时分抽取的支路数与支路长度。
const BRANCHES: usize = 5;
/// 支路长度 80 = 5 · 2⁴，vDSP 的实数 DFT 支持这一档。
const SUB_FFT: usize = FFT_SIZE / BRANCHES;
/// 一条 80 点实数 DFT 名义上的频点数：0..=40（40 是奈奎斯特）。
const SUB_BINS: usize = SUB_FFT / 2 + 1;

/// 参考实现的 `mel_filters.npz` 里的 `mel_128`，行优先 `(128, 201)`。
static MEL_FILTERS: &[u8] = include_bytes!("../assets/mel_filters_128.bin");
/// 参考实现的 `np.hanning(401)[:-1]`，长度 400。
static HANN: &[u8] = include_bytes!("../assets/hann_400.bin");

/// 预先算好的滤波器组、窗与旋转因子。每一帧做五条 80 点实数 DFT 再合并。
pub struct MelFrontend {
    window: Vec<f32>,
    filters: Vec<f32>,
    /// `W_400^{r k}`，`r` 取 1..=4、`k` 取 0..201，交错存 `(re, im)`。
    /// 头部 `r = 0` 的因子恒为 1，不存。
    twiddle: Vec<f64>,
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
        let mut twiddle = vec![0.0f64; (BRANCHES - 1) * FREQ_BINS * 2];
        for branch in 1..BRANCHES {
            for k in 0..FREQ_BINS {
                let angle = -2.0 * std::f64::consts::PI * (branch * k) as f64 / FFT_SIZE as f64;
                let at = ((branch - 1) * FREQ_BINS + k) * 2;
                twiddle[at] = angle.cos();
                twiddle[at + 1] = angle.sin();
            }
        }
        Self {
            window,
            filters,
            twiddle,
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
        let mut power = [0.0f32; FREQ_BINS];
        let mut raw = [0.0f32; MEL_BINS];
        // 一次 setup 管整段：200 帧里建 200 次 setup 才是浪费。
        let setup = SubDft::new(SUB_FFT);
        for index in 0..kept {
            let start = index * HOP;
            for n in 0..FFT_SIZE {
                frame[n] = padded[start + n] * self.window[n];
            }
            self.power_frame(&setup, &frame, &mut power);
            project(&self.filters, &power, &mut raw);
            for bin in 0..MEL_BINS {
                out[bin * kept + index] = raw[bin];
            }
        }
        shape_log(&mut out);
        out
    }

    /// 一帧 400 个已加窗样本的功率谱，201 个频点。
    ///
    /// macOS：五条 80 点 vDSP 实数 DFT 加旋转因子合并，见模块头。
    #[cfg(target_os = "macos")]
    fn power_frame(&self, setup: &SubDft, frame: &[f32], power: &mut [f32; FREQ_BINS]) {
        // 五条支路的 80 点实数 DFT：0..=40 是名义频点，40 是奈奎斯特。
        let mut branch_re = [[0.0f64; SUB_BINS]; BRANCHES];
        let mut branch_im = [[0.0f64; SUB_BINS]; BRANCHES];
        let mut input_re = Align16([0.0f32; SUB_FFT / 2]);
        let mut input_im = Align16([0.0f32; SUB_FFT / 2]);
        let mut output_re = Align16([0.0f32; SUB_FFT / 2]);
        let mut output_im = Align16([0.0f32; SUB_FFT / 2]);

        for branch in 0..BRANCHES {
            // 打包实数输入：偶数下标进实部数组，奇数下标进虚部数组。
            for j in 0..SUB_FFT / 2 {
                input_re.0[j] = frame[BRANCHES * (2 * j) + branch];
                input_im.0[j] = frame[BRANCHES * (2 * j + 1) + branch];
            }
            setup.execute(&input_re.0, &input_im.0, &mut output_re.0, &mut output_im.0);
            // 打包输出：直流在 re[0]，奈奎斯特在 im[0]，其余 `re[k] + i·im[k]`。
            // vDSP 的实数前向变换带 2 倍增益（文档里的 C），这里折掉，
            // 之后的合并与直接按 DFT 定义算的结果同一个尺度。
            branch_re[branch][0] = 0.5 * output_re.0[0] as f64;
            branch_im[branch][0] = 0.0;
            branch_re[branch][SUB_FFT / 2] = 0.5 * output_im.0[0] as f64;
            branch_im[branch][SUB_FFT / 2] = 0.0;
            for k in 1..SUB_FFT / 2 {
                branch_re[branch][k] = 0.5 * output_re.0[k] as f64;
                branch_im[branch][k] = 0.5 * output_im.0[k] as f64;
            }
        }

        for k in 0..FREQ_BINS {
            // `A_r[k mod 80]`；落到 41..79 的用共轭补，因为支路输入是实数。
            let m = k % SUB_FFT;
            let (m, conjugate) = if m <= SUB_FFT / 2 {
                (m, false)
            } else {
                (SUB_FFT - m, true)
            };
            // 合并用 f64 累加：五个支路各带一次 f32 舍入，合并顺序与参考不同。
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            for branch in 0..BRANCHES {
                let real = branch_re[branch][m];
                let imag = if conjugate {
                    -branch_im[branch][m]
                } else {
                    branch_im[branch][m]
                };
                if branch == 0 {
                    re += real;
                    im += imag;
                } else {
                    let at = ((branch - 1) * FREQ_BINS + k) * 2;
                    let wr = self.twiddle[at];
                    let wi = self.twiddle[at + 1];
                    re += real * wr - imag * wi;
                    im += real * wi + imag * wr;
                }
            }
            // 参考实现用功率谱（幅值平方）。
            power[k] = (re * re + im * im) as f32;
        }
    }

    /// 非 macOS 的退路：逐点直接 DFT，慢但算法同一份定义。产品路径不走这里。
    #[cfg(not(target_os = "macos"))]
    fn power_frame(&self, _setup: &SubDft, frame: &[f32], power: &mut [f32; FREQ_BINS]) {
        for k in 0..FREQ_BINS {
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            for n in 0..FFT_SIZE {
                let angle = -2.0 * std::f64::consts::PI * (k * n) as f64 / FFT_SIZE as f64;
                let value = frame[n] as f64;
                re += value * angle.cos();
                im += value * angle.sin();
            }
            power[k] = (re * re + im * im) as f32;
        }
    }

    /// 旋转因子表，测试与非 macOS 路径用来核对合并式。
    #[cfg(test)]
    fn twiddle_at(&self, branch: usize, k: usize) -> (f64, f64) {
        assert!(branch >= 1 && branch < BRANCHES);
        let at = ((branch - 1) * FREQ_BINS + k) * 2;
        (self.twiddle[at], self.twiddle[at + 1])
    }
}

/// 一帧整形前的原始值：幂谱投到 128 个滤波器之后、`log10` 与截断之前。
pub type RawFrame = [f32; MEL_BINS];

/// 增量梅尔：只给新样本，交出新算出的**整形前**帧。
///
/// 形状与一次性那条路完全一致：帧网格仍是 `i · 160`、窗宽 400、首端与末端都是
/// 反射补边。它仅仅是不再每步从第一个样本重算一遍。
///
/// 使用方式（一步 = 一段新样本）：
///
/// ```text
/// let mut stream = MelStream::new();
/// stream.push(&frontend, 新样本);            // 只有这一段新样本进 FFT
/// let columns = stream.shaped_columns(0, 800); // 取窗时按当前前缀的 max 整形
/// ```
pub struct MelStream {
    /// 下一帧窗起点起的样本，长度 ≤ 400 + 159。位置 `tail[0]` 的绝对样本下标是
    /// `tail_base`；首端反射落在 `-200..-1`，值已经按参考的反射顺序摆好。
    tail: Vec<f32>,
    /// `tail[0]` 的绝对样本下标。
    tail_base: isize,
    /// 已收到的样本数。
    received: usize,
    /// 已定帧的整形前原始值，帧优先（每帧 `MEL_BINS` 个）。
    /// 已定帧的尾巴可能多一帧「未定」帧：窗跨过当前的右端，要按右端反射现算。
    raw: Vec<f32>,
    /// 已定帧数：窗完全落在已收样本里的帧，值从此不再变。
    settled: usize,
    /// 已定帧里最大的原始值。单调不减。
    settled_max: f32,
    /// 当前前缀（含未定尾帧）里最大的原始值，整形用的就是这个。
    prefix_max: f32,
    /// 新帧的 DFT 加滤波器组墙钟累计。
    frames_seconds: f64,
}

impl Default for MelStream {
    fn default() -> Self {
        Self::new()
    }
}

impl MelStream {
    pub fn new() -> Self {
        Self {
            tail: Vec::new(),
            tail_base: 0,
            received: 0,
            raw: Vec::new(),
            settled: 0,
            settled_max: f32::NEG_INFINITY,
            prefix_max: f32::NEG_INFINITY,
            frames_seconds: 0.0,
        }
    }

    /// 接一段 16 kHz 单声道 f32 样本（已归一到 ±1），返回这一步新定下的帧数。
    ///
    /// 等价于「整段重算」的三条理由：
    /// 1. **已定帧的窗不再变**。帧 `i` 的窗是样本 `i·160-200 ..= i·160+199`，
    ///    已定的条件是这 400 个样本全部已收到；之后新样本不会进这个窗。
    /// 2. **首端反射的时机一样**。参考的 `reflect_pad` 在左边补 `samples[200], …, samples[1]`，
    ///    只用到前 201 个样本；这段一旦收齐就固定，所以补进去之后与一次性补边同形。
    ///    在那之前 `frame_count` 本来就是 0（`samples <= 200`），没有帧要算。
    /// 3. **未定的那一帧每次按新右端重算**。最后输出的帧可能跨过接收末端，
    ///    它的值与参考的右端反射 `samples[len-2], samples[len-3], …` 一起变；
    ///    这一步把那 128 个值重算一遍，代价是一帧，与前缀长度无关。
    pub fn push(&mut self, frontend: &MelFrontend, samples: &[f32]) -> usize {
        if samples.is_empty() {
            return 0;
        }
        // 上一轮的未定尾帧作废：右端反射变了。
        self.raw.truncate(self.settled * MEL_BINS);
        self.tail.extend_from_slice(samples);
        self.received += samples.len();
        self.pad_head_once();

        let clock = Instant::now();
        let mut computed = 0usize;
        let setup = SubDft::new(SUB_FFT);
        let mut window = [0.0f32; FFT_SIZE];
        let mut power = [0.0f32; FREQ_BINS];
        // 已定帧：窗的最后一个样本已经收到。
        while self.window_fits(self.settled) {
            let frame = self.raw_frame_now(frontend, &setup, self.settled, &mut window, &mut power);
            self.raw.extend_from_slice(&frame);
            self.settled += 1;
            computed += 1;
            let local = raw_max(&frame);
            if local > self.settled_max {
                self.settled_max = local;
            }
        }
        // 尾巴只要留到下一帧窗的起点：更早的样本不再进任何未来的窗。
        // 首端那 200 个反射值在 `tail_base` 还是 0 时就位（那时还没有帧）。
        let next_start = self.frame_start(self.settled);
        if next_start > self.tail_base {
            let shift = (next_start - self.tail_base) as usize;
            self.tail.drain(..shift);
            self.tail_base = next_start;
        }
        // 未定尾帧：按当前的接收末端重算一帧，它的值随长度变。
        let frames = self.frames();
        let mut provisional_max = f32::NEG_INFINITY;
        if frames > self.settled {
            let frame = self.raw_frame_now(frontend, &setup, self.settled, &mut window, &mut power);
            provisional_max = raw_max(&frame);
            self.raw.extend_from_slice(&frame);
        }
        self.frames_seconds += clock.elapsed().as_secs_f64();
        self.prefix_max = self.settled_max.max(provisional_max);
        debug_assert_eq!(self.raw.len(), frames * MEL_BINS, "缓存的帧数与形状对齐");
        computed
    }

    /// `i16` 样本的入口，归一化与 [`MelFrontend::log_mel`] 同一条算式。
    pub fn push_i16(&mut self, frontend: &MelFrontend, pcm: &[i16]) -> usize {
        let samples: Vec<f32> = pcm.iter().map(|value| *value as f32 / 32768.0).collect();
        self.push(frontend, &samples)
    }

    /// 已收到的样本数。
    pub fn samples(&self) -> usize {
        self.received
    }

    /// 现在能输出的帧数，与 [`frame_count`] 同口径。
    pub fn frames(&self) -> usize {
        frame_count(self.received)
    }

    /// 已经定下、值不再变的帧数。`frames() - settled_frames()` 是 0 或 1。
    pub fn settled_frames(&self) -> usize {
        self.settled
    }

    /// 还差多少样本才多出一个已定帧。它与「上次剩下的不足一个 hop 的尾巴」互补：
    /// 尾巴是 `samples() - ((settled-1)·160 + 200) < 160`，等的是 `160` 减它，
    /// 因此这个数落在 `0..=160`（第 0 帧之前是「等首端反射凑齐 201 个样本」）。
    pub fn samples_to_next_frame(&self) -> usize {
        if self.settled == 0 {
            // 第 0 帧的窗在样本 1..=200 上取首端反射，所以要先收到 201 个。
            return (FFT_SIZE / 2 + 1).saturating_sub(self.received);
        }
        (self.settled * HOP + FFT_SIZE / 2).saturating_sub(self.received)
    }

    /// 当前前缀里最大的原始值（整形要用它的 `log10`）。
    pub fn prefix_max(&self) -> f32 {
        self.prefix_max
    }

    /// 参考实现的截断下限：`max(log10) - 8`。
    pub fn log_floor(&self) -> f32 {
        self.prefix_max.max(1e-10).log10() - 8.0
    }

    /// 新帧的 DFT 与滤波器组墙钟累计。
    pub fn frames_seconds(&self) -> f64 {
        self.frames_seconds
    }

    /// 第 `frame` 帧的整形前原始值（含未定尾帧）。
    pub fn raw_frame(&self, frame: usize) -> &[f32] {
        let base = frame * MEL_BINS;
        &self.raw[base..base + MEL_BINS]
    }

    /// 取 `[start, end)` 这几帧的整形后值，行优先 `(MEL_BINS, end - start)`，
    /// 与 [`MelFrontend::log_mel_f32`] 的行列顺序相同。
    ///
    /// 整形只算被取走的这几列，因此开销只跟窗宽有关，与已经收了多少音频无关。
    /// 套用的下限是当前前缀的 `max(log10) - 8`，也就是「整段重算」在同一时刻会
    /// 用的那一个数。
    pub fn shaped_columns(&self, start: usize, end: usize) -> Vec<f32> {
        assert!(start <= end, "区间反了：{start}..{end}");
        assert!(end <= self.frames(), "只输出到 {} 帧", self.frames());
        let width = end - start;
        let floor = self.log_floor();
        let mut out = vec![0.0f32; MEL_BINS * width];
        for frame in start..end {
            let base = frame * MEL_BINS;
            for bin in 0..MEL_BINS {
                out[bin * width + frame - start] = shape_one(self.raw[base + bin], floor);
            }
        }
        out
    }

    /// 整段的整形前原始值，行优先 `(MEL_BINS, frames)`。论证与测试用。
    pub fn raw_block(&self) -> Vec<f32> {
        let frames = self.frames();
        let mut out = vec![0.0f32; MEL_BINS * frames];
        for frame in 0..frames {
            let base = frame * MEL_BINS;
            for bin in 0..MEL_BINS {
                out[bin * frames + frame] = self.raw[base + bin];
            }
        }
        out
    }

    /// 整段的整形后值，行优先 `(MEL_BINS, frames)`，与 `log_mel_f32` 同一个形状。
    pub fn shaped_block(&self) -> Vec<f32> {
        self.shaped_columns(0, self.frames())
    }

    /// 首端反射只做一次：`padded[0..200] = samples[200], samples[199], …, samples[1]`。
    /// 与 `reflect_pad` 的左端同一条算式（端点不重复）。
    fn pad_head_once(&mut self) {
        if self.tail_base != 0 || self.received <= FFT_SIZE / 2 {
            return;
        }
        let pad = FFT_SIZE / 2;
        let mut head = Vec::with_capacity(pad + self.tail.len());
        for index in (1..=pad).rev() {
            head.push(self.tail[index]);
        }
        head.extend_from_slice(&self.tail);
        self.tail = head;
        self.tail_base = -(pad as isize);
    }

    /// 帧 `frame` 的窗在绝对样本下标上的起点（可为负：首端反射）。
    fn frame_start(&self, frame: usize) -> isize {
        frame as isize * HOP as isize - (FFT_SIZE as isize / 2)
    }

    /// 窗的 400 个样本是不是全部已收到。
    ///
    /// 窗跨到负下标（第 0、1 帧）时还要等首端反射就位：反射值取 `samples[1..=200]`，
    /// 所以那两帧要等 `received > 200`。这与 `frame_count` 的门一致（那里 `<= 200` 回 0）。
    fn window_fits(&self, frame: usize) -> bool {
        let start = self.frame_start(frame);
        start + FFT_SIZE as isize <= self.received as isize
            && (start >= 0 || self.received > FFT_SIZE / 2)
    }

    /// 现算一帧：取窗（跨右端时按当前接收长度做右端反射）→ 加窗 → 功率谱 → 滤波器组。
    fn raw_frame_now(
        &self,
        frontend: &MelFrontend,
        setup: &SubDft,
        frame: usize,
        window: &mut [f32; FFT_SIZE],
        power: &mut [f32; FREQ_BINS],
    ) -> RawFrame {
        self.fill_window(frame, window);
        for n in 0..FFT_SIZE {
            window[n] *= frontend.window[n];
        }
        frontend.power_frame(setup, window, power);
        let mut raw = [0.0f32; MEL_BINS];
        project(&frontend.filters, power, &mut raw);
        raw
    }

    /// 把第 `frame` 帧的 400 个样本摆进 `out`。
    ///
    /// 落在已收样本里的直接取；越过接收末端的按右端反射取
    /// （`padded[len+200+k] = samples[len-2-k]`，与 `reflect_pad` 同一条算式）。
    fn fill_window(&self, frame: usize, out: &mut [f32; FFT_SIZE]) {
        let start = self.frame_start(frame);
        let last = self.received as isize - 1;
        for (n, value) in out.iter_mut().enumerate() {
            let index = start + n as isize;
            let sample = if index <= last {
                self.tail[(index - self.tail_base) as usize]
            } else {
                let mirror = 2 * last - index;
                self.tail[(mirror - self.tail_base) as usize]
            };
            *value = sample;
        }
    }
}

/// 滤波器组：`MEL_BINS × FREQ_BINS` 的逐点乘加，零权重跳过。
fn project(filters: &[f32], power: &[f32; FREQ_BINS], out: &mut RawFrame) {
    for bin in 0..MEL_BINS {
        let row = bin * FREQ_BINS;
        let mut sum = 0.0f32;
        for freq in 0..FREQ_BINS {
            let weight = filters[row + freq];
            if weight != 0.0 {
                sum += power[freq] * weight;
            }
        }
        out[bin] = sum;
    }
}

fn raw_max(frame: &RawFrame) -> f32 {
    let mut max = f32::NEG_INFINITY;
    for value in frame {
        if *value > max {
            max = *value;
        }
    }
    max
}

/// 参考实现的整形：`log10`（抬底 `1e-10`）→ 按 `floor` 截断 → `(x + 4) / 4`。
fn shape_one(raw: f32, floor: f32) -> f32 {
    let logged = raw.max(1e-10).log10();
    (logged.max(floor) + 4.0) / 4.0
}

/// 16 字节对齐的暂存区：vDSP 文档把对齐列为「建议」，不对齐时某些长度会明显变慢。
#[cfg(target_os = "macos")]
#[repr(align(16))]
struct Align16<const N: usize>([f32; N]);

/// 一条 `SUB_FFT` 点实数 DFT 的 setup（vDSP），`Drop` 时释放。
#[cfg(target_os = "macos")]
struct SubDft(*const std::ffi::c_void);

#[cfg(target_os = "macos")]
impl SubDft {
    fn new(length: usize) -> Self {
        debug_assert_eq!(length, SUB_FFT);
        let setup = unsafe {
            vDSP_DFT_zrop_CreateSetup(
                std::ptr::null_mut(),
                length as std::os::raw::c_ulong,
                VDSP_DFT_FORWARD,
            )
        };
        assert!(
            !setup.is_null(),
            "vDSP 没有 {length} 点实数 DFT 的实现（只支持 2^n 与 f·2^n，f 取 3、5、15）"
        );
        Self(setup)
    }

    fn execute(
        &self,
        input_re: &[f32],
        input_im: &[f32],
        output_re: &mut [f32],
        output_im: &mut [f32],
    ) {
        unsafe {
            vDSP_DFT_Execute(
                self.0,
                input_re.as_ptr(),
                input_im.as_ptr(),
                output_re.as_mut_ptr(),
                output_im.as_mut_ptr(),
            );
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for SubDft {
    fn drop(&mut self) {
        unsafe { vDSP_DFT_DestroySetup(self.0 as *mut std::ffi::c_void) };
    }
}

#[cfg(target_os = "macos")]
const VDSP_DFT_FORWARD: std::os::raw::c_int = 1;

#[cfg(target_os = "macos")]
#[link(name = "Accelerate", kind = "framework")]
extern "C" {
    fn vDSP_DFT_zrop_CreateSetup(
        previous: *mut std::ffi::c_void,
        length: std::os::raw::c_ulong,
        direction: std::os::raw::c_int,
    ) -> *const std::ffi::c_void;
    fn vDSP_DFT_DestroySetup(setup: *mut std::ffi::c_void);
    fn vDSP_DFT_Execute(
        setup: *const std::ffi::c_void,
        input_re: *const f32,
        input_im: *const f32,
        output_re: *mut f32,
        output_im: *mut f32,
    );
}

/// 非 macOS 没有 setup 可建，留一个空壳让签名一致。
#[cfg(not(target_os = "macos"))]
struct SubDft;

#[cfg(not(target_os = "macos"))]
impl SubDft {
    fn new(_length: usize) -> Self {
        Self
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
    /// 与参考实现逐元素对照。参考是 numpy 的 f32 实数 FFT，这里是 vDSP 的
    /// 五条 80 点实数 DFT 再合并，两边求和顺序与精度都不同，
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

    /// 抽取合并式本身：旋转因子表与 `W_400^{rk}` 的定义一致，且五条支路覆盖
    /// 0..400 的每个样本一次（不重不漏）。
    #[test]
    fn branch_twiddles_are_the_400_point_roots() {
        let frontend = MelFrontend::new();
        for branch in 1..BRANCHES {
            for k in [0usize, 1, 7, 40, 200] {
                let (re, im) = frontend.twiddle_at(branch, k);
                let angle = -2.0 * std::f64::consts::PI * (branch * k) as f64 / FFT_SIZE as f64;
                assert!((re - angle.cos()).abs() < 1e-12, "branch {branch} k {k}");
                assert!((im - angle.sin()).abs() < 1e-12, "branch {branch} k {k}");
                assert!(
                    (re * re + im * im - 1.0).abs() < 1e-12,
                    "旋转因子应是单位模"
                );
            }
        }
        // 五个支路的下标集合 `5m + r` 正好是 0..400。
        let mut seen = vec![false; FFT_SIZE];
        for branch in 0..BRANCHES {
            for m in 0..SUB_FFT {
                let index = BRANCHES * m + branch;
                assert!(index < FFT_SIZE);
                assert!(!seen[index], "下标 {index} 被两条支路拿到");
                seen[index] = true;
            }
        }
        assert!(seen.iter().all(|hit| *hit), "有样本没进任何支路");
    }

    /// 单帧单频正弦：功率谱应在对应频点起峰，且其余频点明显更低。
    /// 这条不依赖参考资产，能抓住支路顺序或旋转因子接错。
    #[test]
    fn single_tone_peaks_at_its_bin() {
        let frontend = MelFrontend::new();
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|n| (2.0 * std::f64::consts::PI * 25.0 * n as f64 / FFT_SIZE as f64).sin() as f32)
            .collect();
        let setup = SubDft::new(SUB_FFT);
        let mut frame = [0.0f32; FFT_SIZE];
        frame.copy_from_slice(&samples);
        let mut power = [0.0f32; FREQ_BINS];
        frontend.power_frame(&setup, &frame, &mut power);
        let peak = (0..FREQ_BINS)
            .max_by(|a, b| power[*a].partial_cmp(&power[*b]).expect("有限值"))
            .expect("非空");
        assert_eq!(
            peak, 25,
            "峰应在第 25 个频点，实际 {peak}，功率 {:?}",
            power[peak]
        );
        // 一个整周期的整数倍上，别的频点应是零（舍入范围内）。
        let elsewhere = (0..FREQ_BINS)
            .filter(|k| *k != 25)
            .fold(0.0f32, |worst, k| worst.max(power[k]));
        assert!(
            elsewhere < 1e-2 * power[25],
            "旁瓣过高：{elsewhere} 对 {}",
            power[25]
        );
    }

    /// 增量那条路与整段重算逐位相同：一段样本按 1/16 秒一块送，每送一块比一次。
    ///
    /// 这条是「等价于整段重算」的核心闸。它不靠容差，而是要求同一个数：
    /// 参考的整形只用到一个标量（全段最大 `log10` 减 8），增量那条路把这个
    /// 标量按**当前前缀**增量维护，所以两边逐元素走的是同一串 f32 运算。
    #[test]
    fn incremental_matches_whole_utterance_bitwise() {
        let pcm: Vec<i16> = REFERENCE_PCM
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let frontend = MelFrontend::new();
        let mut stream = MelStream::new();
        let mut fed = 0usize;
        // 一块 1000 个样本：不是 hop 的整数倍，也不是窗宽的整数倍。
        for block in pcm.chunks(1_000) {
            stream.push_i16(&frontend, block);
            fed += block.len();
            let whole = frontend.log_mel(&pcm[..fed]);
            let step = stream.shaped_block();
            assert_eq!(
                step.len(),
                whole.len(),
                "第 {fed} 个样本之后形状不一致：{} 对 {}",
                step.len(),
                whole.len()
            );
            for (index, (a, b)) in step.iter().zip(whole.iter()).enumerate() {
                assert_eq!(
                    a.to_bits(),
                    b.to_bits(),
                    "第 {fed} 个样本之后下标 {index} 不同：{a:?} 对 {b:?}"
                );
            }
        }
        assert_eq!(fed, pcm.len());
        assert_eq!(stream.frames(), frame_count(pcm.len()));
        assert_eq!(stream.settled_frames() + 1, stream.frames());
    }

    /// 按 hop 与按窗宽两种送法都给同一组数：分块边界不影响结果，也不重复补边。
    #[test]
    fn chunking_does_not_change_the_frames() {
        let pcm: Vec<i16> = REFERENCE_PCM
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let frontend = MelFrontend::new();
        let expected = frontend.log_mel(&pcm);
        for block in [1usize, 159, 160, 161, 400, 401, 2_000] {
            let mut stream = MelStream::new();
            for chunk in pcm.chunks(block) {
                stream.push_i16(&frontend, chunk);
            }
            let actual = stream.shaped_block();
            assert_eq!(actual, expected, "按 {block} 个样本一块送时结果不同");
        }
    }

    /// 尾巴的长度与「再收多少才多一帧」：每次推进之后剩下的不足一个 hop。
    #[test]
    fn leftover_tail_is_shorter_than_one_hop() {
        let frontend = MelFrontend::new();
        let mut stream = MelStream::new();
        let mut fed = 0usize;
        for block in [1usize, 200, 159, 40, 161, 320, 999] {
            stream.push(&frontend, &vec![0.0f32; block]);
            fed += block;
            let wait = stream.samples_to_next_frame();
            // 第 0 帧要等首端反射凑齐（201 个样本）；之后「已收但还没进任何已定帧」
            // 的那一段一定短于一个 hop，等的量因此不超过一个 hop。
            if stream.settled_frames() > 0 {
                assert!(
                    wait <= HOP,
                    "第 {fed} 个样本之后还要等 {wait} 个，超过一个 hop"
                );
                let used = (stream.settled_frames() - 1) * HOP + FFT_SIZE / 2;
                assert!(
                    stream.samples() - used < HOP,
                    "第 {fed} 个样本之后剩下的尾巴不短于一个 hop"
                );
            } else {
                assert!(wait <= FFT_SIZE / 2 + 1, "起步不该等 {wait} 个样本");
            }
            // 再收 `wait` 个就一定多一帧，少收一个就一定不多。
            let mut probe = MelStream::new();
            probe.push(&frontend, &vec![0.0f32; fed + wait]);
            let mut short = MelStream::new();
            short.push(&frontend, &vec![0.0f32; fed + wait - 1]);
            assert_eq!(
                probe.settled_frames(),
                stream.settled_frames() + 1,
                "第 {fed} 个样本之后再收 {wait} 个应当多出一帧"
            );
            assert_eq!(
                short.settled_frames(),
                stream.settled_frames(),
                "第 {fed} 个样本之后再收 {} 个不应当多出一帧",
                wait - 1
            );
        }
    }

    /// 未定的尾帧跟着右端反射走：逐长度比一遍，与整段重算逐位相同。
    #[test]
    fn provisional_frame_tracks_the_right_reflection() {
        let frontend = MelFrontend::new();
        let pcm: Vec<i16> = REFERENCE_PCM
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let mut stream = MelStream::new();
        let mut fed = 0usize;
        // 从「第一帧出现」到「最后一个样本」，每个长度都比一次。
        for len in 201..=pcm.len() {
            stream.push_i16(&frontend, &pcm[fed..len]);
            fed = len;
            if frame_count(len) == 0 {
                continue;
            }
            let whole = frontend.log_mel(&pcm[..len]);
            let step = stream.shaped_block();
            assert_eq!(step, whole, "{len} 个样本时结果不同");
        }
    }

    /// 增量那条路与参考实现的差：与整段那条路同一个数（同一份算术）。
    #[test]
    fn matches_reference_mel_incrementally() {
        let pcm: Vec<i16> = REFERENCE_PCM
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let expected = decode_f32(REFERENCE_MEL);
        let frontend = MelFrontend::new();
        let mut stream = MelStream::new();
        for chunk in pcm.chunks(1_000) {
            stream.push_i16(&frontend, chunk);
        }
        let actual = stream.shaped_block();
        assert_eq!(actual.len(), expected.len());
        let mut worst = 0.0f32;
        let mut at = 0usize;
        for (index, (a, b)) in actual.iter().zip(expected.iter()).enumerate() {
            let diff = (a - b).abs();
            if diff > worst {
                worst = diff;
                at = index;
            }
            assert!(diff < 1e-3, "第 {index} 个元素差 {diff}：{a} 对 {b}");
        }
        println!("增量梅尔：实测差 {worst:e}（下标 {at}），容差 1e-3");
    }

    /// 取列只取这一段：行优先 `(128, end - start)`，与整段里那一段相同。
    #[test]
    fn shaped_columns_takes_one_contiguous_window_per_bin() {
        let frontend = MelFrontend::new();
        let pcm: Vec<i16> = REFERENCE_PCM
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let mut stream = MelStream::new();
        stream.push_i16(&frontend, &pcm);
        let frames = stream.frames();
        let block = stream.shaped_block();
        let columns = stream.shaped_columns(7, 33);
        assert_eq!(columns.len(), MEL_BINS * 26);
        for bin in 0..MEL_BINS {
            for frame in 7..33 {
                assert_eq!(
                    columns[bin * 26 + frame - 7],
                    block[bin * frames + frame],
                    "bin {bin} frame {frame}"
                );
            }
        }
    }

    /// 空输入与不足 201 个样本：没有帧，也不越界。
    #[test]
    fn too_few_samples_give_no_frames() {
        let frontend = MelFrontend::new();
        let mut stream = MelStream::new();
        for block in [0usize, 1, 199] {
            stream.push(&frontend, &vec![0.5f32; block]);
        }
        assert_eq!(stream.samples(), 200);
        assert_eq!(stream.frames(), 0);
        assert_eq!(stream.shaped_block().len(), 0);
        assert_eq!(stream.samples_to_next_frame(), 1);
    }
}
