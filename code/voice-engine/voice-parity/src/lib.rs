//! 参考资产对照箱子。
//!
//! 读 `code/voice-oracle/assets` 下由 `dump_reference.py` 导出的 `.bin` 与
//! `manifest.json`，给出「最大绝对差 / 首个超差下标 / 两侧数值」的报告。
//!
//! 三条边界：
//!
//! 1. 只用 std 读文件与比数，不接 `voice-mlx` 或 mlx-rs 的测试钩子——对照要能
//!    在后端还没把权重送进各层的时候就跑起来。
//! 2. 资产只作对照标准，不进安装包；本箱子不是产品路径，也不被产品 crate 依赖。
//! 3. 报告只陈述差值，不在比较里放「模糊通过」。超差就是超差，调用方自己决定
//!    是否断言失败。
//!
//! 资产位置默认 `../../voice-oracle/assets`（相对本箱子），可用环境变量
//! `VOICE_ORACLE_ASSETS` 指向别处。
//!
//! 数值闸的容差口径（参考是 f16，容差 = 上界 × 2^-11 × 倍数）只在
//! [`tolerance`] 里写一次，编码器、解码器与梅尔三条闸都引用那里。

mod json;
pub mod tolerance;

pub use tolerance::{f16_tolerance, f16_units, Margin, F16_UNIT};

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use json::Json;

/// 默认资产目录：本箱子往上是 `voice-engine`，再往上是 `code`。
pub const DEFAULT_ASSET_SUBDIR: &str = "../../voice-oracle/assets";

/// 一件参考张量：`manifest.json` 里 `tensors` 的一条，加上落盘的数值。
#[derive(Debug, Clone)]
pub struct Tensor {
    /// manifest 里的键名，例如 `mel`、`encoder_proj_out`。
    pub key: String,
    /// 文件名，例如 `mel.bin`。
    pub file: String,
    /// 参考记下的形状，`mel` 是 `[128, 200]`。
    pub shape: Vec<usize>,
    /// 参考记下的 dtype，导出脚本一律写 `float32`。
    pub dtype: String,
    /// 参考实现里产出这份量的函数名。
    pub source: String,
    /// manifest 里附带的说明；没有就是空串。
    pub note: String,
    /// 裸小端 f32，按行优先展平。
    pub values: Vec<f32>,
}

impl Tensor {
    /// 元素个数。与 `values.len()` 不一致时说明资产被截断。
    pub fn element_count(&self) -> usize {
        self.shape.iter().product()
    }

    /// 形状的短写法，报告里用。
    pub fn shape_text(&self) -> String {
        format_shape(&self.shape)
    }
}

/// 对照报告。一次比较的全部结论都在这里，调用方只读不猜。
#[derive(Debug, Clone)]
pub struct Report {
    /// 被比较的量，通常是 manifest 的键名。
    pub name: String,
    /// 本次比较使用的容差。
    pub tolerance: f32,
    /// 参考值元素数。
    pub expected_len: usize,
    /// 实际值元素数。
    pub actual_len: usize,
    /// 参考记下的形状；实际侧没有形状可对，只有长度。
    pub expected_shape: Vec<usize>,
    /// 最大绝对差。长度不等或空数组时为 `None`。
    pub max_abs_diff: Option<f32>,
    /// 最大绝对差所在下标。
    pub max_index: Option<usize>,
    /// 最大绝对差处的实际值。
    pub max_actual: Option<f32>,
    /// 最大绝对差处的参考值。
    pub max_expected: Option<f32>,
    /// 首个超差位置（含下标与两侧数值）；全部在容差内时为 `None`。
    pub first_exceed: Option<Mismatch>,
    /// 长度不等时记下差额（实际减参考）。
    pub length_delta: i64,
    /// 超差元素个数。
    pub exceeded: usize,
}

/// 一处超差：下标、实际值、参考值、差的绝对值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mismatch {
    pub index: usize,
    pub actual: f32,
    pub expected: f32,
    pub diff: f32,
}

impl Report {
    /// 是否落在容差内。长度不等直接算不在容差内——不把形状差异吞掉。
    pub fn within_tolerance(&self) -> bool {
        self.first_exceed.is_none() && self.length_delta == 0 && self.max_abs_diff.is_some()
    }

    /// 超差元素个数（逐个比，不看容差聚合）。
    pub fn exceeded_count(&self) -> usize {
        self.exceeded
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "对照量：{}（参考形状 {}）", self.name, format_shape(&self.expected_shape))?;
        writeln!(
            f,
            "元素数：参考 {}，实际 {}{}",
            self.expected_len,
            self.actual_len,
            if self.length_delta == 0 {
                String::new()
            } else {
                format!("（差 {:+#}）", self.length_delta)
            }
        )?;
        match (self.max_abs_diff, self.max_index, self.max_actual, self.max_expected) {
            (Some(max), Some(index), Some(actual), Some(expected)) => {
                writeln!(
                    f,
                    "最大绝对差：{max:e}（下标 {index}：实际 {actual:e}，参考 {expected:e}）"
                )?;
            }
            _ => writeln!(f, "最大绝对差：无（一侧为空，无法逐元素比）")?,
        }
        match &self.first_exceed {
            Some(hit) => writeln!(
                f,
                "首个超差：下标 {}，实际 {:e}，参考 {:e}，差 {:e}，容差 {:e}（共 {} 个元素超差）",
                hit.index, hit.actual, hit.expected, hit.diff, self.tolerance, self.exceeded
            )?,
            None if self.length_delta == 0 => writeln!(
                f,
                "首个超差：无（全部在容差 {:e} 内）",
                self.tolerance
            )?,
            None => writeln!(
                f,
                "首个超差：无（可比元素都在容差 {:e} 内，但两边长度差 {:+#}，没有比完）",
                self.tolerance, self.length_delta
            )?,
        }
        Ok(())
    }
}

/// 参考资产集合。加载一次，之后按名字取数。
#[derive(Debug, Clone)]
pub struct Reference {
    root: PathBuf,
    manifest: Json,
    tensors: BTreeMap<String, Tensor>,
}

impl Reference {
    /// 从默认位置或 `VOICE_ORACLE_ASSETS` 载入。
    pub fn load() -> Result<Self, String> {
        Self::load_from(asset_root())
    }

    /// 从指定目录载入：先读 `manifest.json`，再按 `tensors[].file` 读 `.bin`。
    pub fn load_from(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = root.as_ref().to_path_buf();
        let manifest_path = root.join("manifest.json");
        let text = fs::read_to_string(&manifest_path).map_err(|error| {
            format!(
                "读不到 {}：{error}。可用 VOICE_ORACLE_ASSETS 指向资产目录",
                manifest_path.display()
            )
        })?;
        let manifest = json::parse(&text)
            .map_err(|error| format!("{} 解析失败：{error}", manifest_path.display()))?;
        let entries = manifest
            .get("tensors")
            .and_then(|value| value.as_array())
            .ok_or_else(|| format!("{} 缺少 tensors 数组", manifest_path.display()))?;

        let mut tensors = BTreeMap::new();
        for entry in entries {
            let key = entry
                .get("key")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "tensors 有一条缺少 key".to_string())?
                .to_string();
            let file = entry
                .get("file")
                .and_then(|value| value.as_str())
                .ok_or_else(|| format!("{key} 缺少 file"))?
                .to_string();
            let shape = entry
                .get("shape")
                .and_then(|value| value.as_array())
                .ok_or_else(|| format!("{key} 缺少 shape"))?
                .iter()
                .map(|dim| dim.as_usize().ok_or_else(|| format!("{key} 的 shape 不是整数")))
                .collect::<Result<Vec<usize>, String>>()?;
            let dtype = entry
                .get("dtype")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            let source = entry
                .get("source")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            let note = entry
                .get("note")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            let path = root.join(&file);
            let bytes = fs::read(&path)
                .map_err(|error| format!("读不到 {}：{error}", path.display()))?;
            if bytes.len() % 4 != 0 {
                return Err(format!("{} 字节数 {} 不是 4 的倍数", path.display(), bytes.len()));
            }
            let values: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();
            tensors.insert(
                key.clone(),
                Tensor {
                    key,
                    file,
                    shape,
                    dtype,
                    source,
                    note,
                    values,
                },
            );
        }
        Ok(Self {
            root,
            manifest,
            tensors,
        })
    }

    /// 资产目录。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// manifest 原始值，供测试打印上下文。
    pub fn manifest(&self) -> &Json {
        &self.manifest
    }

    /// manifest 里的一段标量字符串，例如 `model_dtype`。
    pub fn manifest_str(&self, key: &str) -> Option<&str> {
        self.manifest.get(key).and_then(|value| value.as_str())
    }

    /// 按名字取一件参考张量。
    pub fn tensor(&self, name: &str) -> Option<&Tensor> {
        self.tensors.get(name)
    }

    /// 所有键名，按字典序。
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.tensors.keys().map(|key| key.as_str())
    }

    /// 提示里那串 token id（`prompt.tokens`）。没有就是空。
    pub fn prompt_tokens(&self) -> Vec<u32> {
        self.manifest
            .get("prompt")
            .and_then(|value| value.get("tokens"))
            .and_then(|value| value.as_array())
            .map(|items| items.iter().filter_map(|item| item.as_u32()).collect())
            .unwrap_or_default()
    }

    /// 贪心解码的前 3 个 token（`greedy.tokens`）。没有就是空。
    pub fn greedy_tokens(&self) -> Vec<u32> {
        self.manifest
            .get("greedy")
            .and_then(|value| value.get("tokens"))
            .and_then(|value| value.as_array())
            .map(|items| items.iter().filter_map(|item| item.as_u32()).collect())
            .unwrap_or_default()
    }

    /// 参考记下的采样率。
    pub fn sample_rate(&self) -> Option<usize> {
        self.manifest.get("sample_rate").and_then(|value| value.as_usize())
    }

    /// 音频帧数上下文（`audio_frames`）。
    pub fn audio_frames(&self) -> Option<&Json> {
        self.manifest.get("audio_frames")
    }

    /// 把实际数与同名参考张量逐元素比较。
    ///
    /// `tolerance` 是对「差的绝对值」的上界：`diff > tolerance` 记为超差。
    /// 长度不等时报告里 `length_delta` 非零，`within_tolerance()` 为假。
    pub fn compare(&self, name: &str, actual: &[f32], tolerance: f32) -> Report {
        let expected = match self.tensors.get(name) {
            Some(tensor) => tensor,
            None => panic!(
                "manifest 里没有 {name}；现有键：{}",
                self.keys().collect::<Vec<_>>().join(", ")
            ),
        };
        compare_slices(name, &expected.values, &expected.shape, actual, tolerance)
    }
}

/// 不依赖 manifest 的纯函数比较：给参考切片、参考形状、实际切片。
///
/// 报告里只写差值，不做任何「容差内改判成功」的处理。
pub fn compare_slices(
    name: &str,
    expected: &[f32],
    expected_shape: &[usize],
    actual: &[f32],
    tolerance: f32,
) -> Report {
    let mut max_abs_diff: Option<f32> = None;
    let mut max_index: Option<usize> = None;
    let mut max_actual: Option<f32> = None;
    let mut max_expected: Option<f32> = None;
    let mut first_exceed: Option<Mismatch> = None;
    let mut exceeded = 0usize;

    for index in 0..expected.len().min(actual.len()) {
        let (want, got) = (expected[index], actual[index]);
        let diff = (want - got).abs();
        if max_abs_diff.map_or(true, |worst| diff > worst) {
            max_abs_diff = Some(diff);
            max_index = Some(index);
            max_actual = Some(got);
            max_expected = Some(want);
        }
        if diff > tolerance {
            exceeded += 1;
            if first_exceed.is_none() {
                first_exceed = Some(Mismatch {
                    index,
                    actual: got,
                    expected: want,
                    diff,
                });
            }
        }
    }

    Report {
        name: name.to_string(),
        tolerance,
        expected_len: expected.len(),
        actual_len: actual.len(),
        expected_shape: expected_shape.to_vec(),
        max_abs_diff,
        max_index,
        max_actual,
        max_expected,
        first_exceed,
        length_delta: actual.len() as i64 - expected.len() as i64,
        exceeded,
    }
}

/// token 序列上的一处不一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenMismatch {
    /// 第几个位置。
    pub index: usize,
    /// 实际产出的 token id。
    pub actual: u32,
    /// 参考的 token id。
    pub expected: u32,
}

/// token 序列的对照报告。与数值对照分开：token id 是整数，只有相同与不同，
/// 没有容差可言。
#[derive(Debug, Clone)]
pub struct TokenReport {
    /// 被比较的量，通常是 `greedy_tokens_3`。
    pub name: String,
    /// 参考序列。
    pub expected: Vec<u32>,
    /// 实际序列。
    pub actual: Vec<u32>,
    /// 从左往右逐位比，第一处不同。
    pub first_mismatch: Option<TokenMismatch>,
    /// 开头连续相同的个数。
    pub matched_prefix: usize,
}

impl TokenReport {
    /// 长度相同且每一位都相同。
    pub fn matches(&self) -> bool {
        self.first_mismatch.is_none() && self.expected.len() == self.actual.len()
    }
}

impl fmt::Display for TokenReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "token 序列：{}", self.name)?;
        writeln!(f, "期望（{} 个）：{:?}", self.expected.len(), self.expected)?;
        writeln!(f, "实际（{} 个）：{:?}", self.actual.len(), self.actual)?;
        writeln!(f, "开头相同 {} 个", self.matched_prefix)?;
        match self.first_mismatch {
            Some(hit) => writeln!(
                f,
                "首处不同：下标 {}，实际 {}，期望 {}",
                hit.index, hit.actual, hit.expected
            ),
            None if self.expected.len() == self.actual.len() => {
                writeln!(f, "首处不同：无（逐位一致）")
            }
            None => writeln!(
                f,
                "首处不同：无（可比前缀一致，但长度差 {:+#}，没有比完）",
                self.actual.len() as i64 - self.expected.len() as i64
            ),
        }
    }
}

/// 逐位比较两串 token id。不做「前缀相同就算过」的宽松判定：
/// 长度不等时 `matches()` 为假，报告里写明差了几位。
pub fn compare_tokens(name: &str, expected: &[u32], actual: &[u32]) -> TokenReport {
    let mut first_mismatch = None;
    let mut matched_prefix = 0usize;
    for index in 0..expected.len().min(actual.len()) {
        if expected[index] == actual[index] {
            if first_mismatch.is_none() {
                matched_prefix += 1;
            }
        } else if first_mismatch.is_none() {
            first_mismatch = Some(TokenMismatch {
                index,
                actual: actual[index],
                expected: expected[index],
            });
        }
    }
    TokenReport {
        name: name.to_string(),
        expected: expected.to_vec(),
        actual: actual.to_vec(),
        first_mismatch,
        matched_prefix,
    }
}

/// 裸小端 i16 读成采样。参考的 PCM 资产就是这个格式。
pub fn read_pcm_i16(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect()
}

/// 裸小端 f32 读成向量。用于直接读资产，不经 manifest。
pub fn read_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// 资产目录：`VOICE_ORACLE_ASSETS` 优先，否则相对 `CARGO_MANIFEST_DIR`。
pub fn asset_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("VOICE_ORACLE_ASSETS") {
        return PathBuf::from(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_ASSET_SUBDIR)
}

fn format_shape(shape: &[usize]) -> String {
    let dims: Vec<String> = shape.iter().map(|dim| dim.to_string()).collect();
    format!("({})", dims.join(", "))
}
