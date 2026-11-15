//! `weights.safetensors` 的读取。
//!
//! 权重文件是「8 字节小端表头长度 + JSON 表头 + 裸数据」的老格式。本模块只
//! 解表头、按名字定位区间、读出稠密张量与分组权重，不解释层结构。
//!
//! 两座塔的分组位数不同：`quantization_config.json` 写的是文本 4 位、音频
//! `audio_tower_bits` 8 位，组大小都是 64。位数由调用方传入，本模块不猜。
//!
//! 装载分三步，一步一次，不按张量重来：
//!
//! 1. `open` 读表头，之后每张量只按 `data_offsets` 定位；
//! 2. 每张量把裸字节读进一段对齐的主机缓冲，直接交给 [`Array::from_raw_data`]
//!    建常量数组——文件里是 f16 / u32 就按 f16 / u32 收，不先在主机上摊成
//!    `Vec<f32>`；只有目标位宽与文件位宽不同时才挂一个 `as_dtype`；
//! 3. [`SafeWeights::finish`] 一次 `eval` 把攒下的数组一起送到设备。

use std::ffi::c_void;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use mlx_rs::ops::{DequantizeOptions, QuantizationMode};
use mlx_rs::{ops, transforms, Array, Dtype};
use voice_tensor::{Conv2d, DType, LayerNorm, Linear};

use super::{from_mlx, MlxError, MlxTensor};

/// 分段计时的四格：表头、字节读取、主机侧转换、MLX 侧求值与上传。
///
/// 只在 `cfg(test)` 下存在——产品归档里没有任何计时调用，`phase!` 直接退化成
/// 被包住的表达式本身。口径见 `notes/065`。
#[cfg(test)]
pub(crate) mod clock {
    use std::time::Instant;

    /// 一格一秒，累加。`bytes` 是 `read_range` 读过的总字节数。
    #[derive(Default, Clone, Copy)]
    pub(crate) struct Phases {
        pub(crate) header: f64,
        pub(crate) read: f64,
        pub(crate) convert: f64,
        pub(crate) eval: f64,
        pub(crate) bytes: u64,
        pub(crate) reads: u64,
        pub(crate) evals: u64,
    }

    /// 四个格子。
    #[derive(Clone, Copy)]
    pub(crate) enum Slot {
        Header,
        Read,
        Convert,
        Eval,
    }

    impl Phases {
        pub(crate) fn add(&mut self, slot: Slot, seconds: f64) {
            match slot {
                Slot::Header => self.header += seconds,
                Slot::Read => self.read += seconds,
                Slot::Convert => self.convert += seconds,
                Slot::Eval => {
                    self.eval += seconds;
                    self.evals += 1;
                }
            }
        }

        /// 四项加起来（只给测试自己对账用，不当一个数报）。
        pub(crate) fn total(&self) -> f64 {
            self.header + self.read + self.convert + self.eval
        }

        /// 相对上一次记账的增量。
        pub(crate) fn since(&self, before: &Phases) -> Phases {
            Phases {
                header: self.header - before.header,
                read: self.read - before.read,
                convert: self.convert - before.convert,
                eval: self.eval - before.eval,
                bytes: self.bytes.saturating_sub(before.bytes),
                reads: self.reads.saturating_sub(before.reads),
                evals: self.evals.saturating_sub(before.evals),
            }
        }
    }

    /// 开始计时。
    pub(crate) fn tick(enabled: bool) -> Option<Instant> {
        enabled.then(Instant::now)
    }
}

/// 把一段算式记进某一格。产品构建里就是算式本身。
#[cfg(test)]
macro_rules! phase {
    ($self:ident, $slot:expr, $body:expr) => {{
        let at = clock::tick($self.phases.is_some());
        let out = $body;
        if let (Some(at), Some(phases)) = (at, $self.phases.as_mut()) {
            phases.add($slot, at.elapsed().as_secs_f64());
        }
        out
    }};
}

#[cfg(not(test))]
macro_rules! phase {
    ($self:ident, $slot:expr, $body:expr) => {
        $body
    };
}

/// 主机侧的一段字节，起点按 8 字节对齐。
///
/// [`Array::from_raw_data`] 底下是 `mlx_array_set_data`，它按元素类型去读这段内存
/// （f16、u32、u64 都走这条），`Vec<u8>` 只保证 1 字节对齐。这里用 `Vec<u64>`
/// 撑住起点，长度仍按字节记。
struct Bytes {
    words: Vec<u64>,
    len: usize,
}

impl Bytes {
    fn zeroed(len: usize) -> Self {
        Self {
            words: vec![0u64; len.div_ceil(8)],
            len,
        }
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY：`words` 有 `len.div_ceil(8) * 8 >= len` 个字节，`u64` 转 `u8`
        // 只放宽对齐要求；借用跟着 `&mut self` 走。
        unsafe { std::slice::from_raw_parts_mut(self.words.as_mut_ptr() as *mut u8, self.len) }
    }

    fn as_ptr(&self) -> *const c_void {
        self.words.as_ptr() as *const c_void
    }
}

/// 已打开的权重文件。
pub(crate) struct SafeWeights {
    file: File,
    data_base: u64,
    header: String,
    /// 已经建好、还没求值的数组。`finish` 之后清空。
    pending: Vec<Array>,
    /// 分段计时的四个格子。产品路径上永远是 `None`（`open` 传 `None`）。
    #[cfg(test)]
    phases: Option<clock::Phases>,
}

/// 表头里一条张量的位置。
struct TensorLoc {
    dtype: String,
    shape: Vec<usize>,
    start: u64,
    end: u64,
}

impl SafeWeights {
    /// 打开 `model_dir/weights.safetensors`。
    pub(crate) fn open(model_dir: &str) -> Result<Self, MlxError> {
        let path = Path::new(model_dir).join("weights.safetensors");
        if !path.is_file() {
            return Err(MlxError::new("打不开权重"));
        }
        let mut file = File::open(&path).map_err(|_| MlxError::new("打不开权重"))?;
        let mut len_buf = [0u8; 8];
        file.read_exact(&mut len_buf)
            .map_err(|_| MlxError::new("打不开权重"))?;
        let header_len = u64::from_le_bytes(len_buf);
        if header_len > 64 * 1024 * 1024 {
            return Err(MlxError::new("权重头过大"));
        }
        let mut header_bytes = vec![0u8; header_len as usize];
        file.read_exact(&mut header_bytes)
            .map_err(|_| MlxError::new("打不开权重"))?;
        let header =
            String::from_utf8(header_bytes).map_err(|_| MlxError::new("权重头不是文本"))?;
        Ok(Self {
            file,
            data_base: 8 + header_len,
            header,
            pending: Vec::new(),
            #[cfg(test)]
            phases: None,
        })
    }

    /// 同上，但打开四格计时。只给 `tests` 里的分段用例用。
    ///
    /// 表头那一格是整个 `open` 的耗时（`File::open` + 读 8 字节长度 + 读表头 + 解成
    /// 文本），因为表头之外 `open` 什么也不做。
    #[cfg(test)]
    pub(crate) fn open_counting(model_dir: &str) -> Result<Self, MlxError> {
        let at = std::time::Instant::now();
        let mut weights = Self::open(model_dir)?;
        let mut phases = clock::Phases::default();
        phases.add(clock::Slot::Header, at.elapsed().as_secs_f64());
        phases.bytes += weights.data_base;
        weights.phases = Some(phases);
        Ok(weights)
    }

    /// 四格计时到此刻为止的数。只在打开计时时非空。
    #[cfg(test)]
    pub(crate) fn phases(&self) -> Option<clock::Phases> {
        self.phases
    }

    /// 把攒下来的数组一次求值。
    ///
    /// 装载时一路只建图：`Array::from_raw_data` 只把字节拷进 MLX 自己的常量缓冲，
    /// `as_dtype` 与 `dequantize` 只挂节点。到这里一次 `eval` 把这些节点一起跑完，
    /// 一次设备同步，不再每张量一次。
    pub(crate) fn finish(&mut self) -> Result<(), MlxError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        phase!(self, clock::Slot::Eval, {
            let out = transforms::eval(self.pending.iter()).map_err(from_mlx);
            self.pending.clear();
            out
        })
    }

    /// 表头里有没有这个名字。
    pub(crate) fn has(&self, key: &str) -> bool {
        self.header.contains(&format!("\"{key}\""))
    }

    /// 读稠密张量并转成 `dtype`。卷积、LayerNorm、偏置都走这条。
    pub(crate) fn dense_as(&mut self, key: &str, dtype: DType) -> Result<MlxTensor, MlxError> {
        let loc = self.locate(key)?;
        let bytes = self.read_range(&loc)?;
        let array = self.array_of_bytes(&bytes, &loc, key, dtype)?;
        Ok(MlxTensor::dense(array, dtype))
    }

    /// 卷积层：`<name>.weight` 与 `<name>.bias`，权重形状是 `(out, 3, 3, in)`。
    pub(crate) fn conv2d(
        &mut self,
        name: &str,
        dtype: DType,
    ) -> Result<Conv2d<MlxTensor>, MlxError> {
        let weight = self.dense_as(&format!("{name}.weight"), dtype)?;
        let bias = self.dense_as(&format!("{name}.bias"), dtype)?;
        Conv2d::new(weight, Some(bias)).map_err(|err| MlxError::new(err.to_string()))
    }

    /// 线性层，权重按 `bits` 位分组反量化到 `dtype`。
    ///
    /// 分组权重的键名带 `.scales` 与 `.biases`；`.bias` 是另一回事——那是线性层
    /// 自己的偏置，可能没有。
    pub(crate) fn affine_as(
        &mut self,
        name: &str,
        group_size: usize,
        bits: usize,
        dtype: DType,
    ) -> Result<Linear<MlxTensor>, MlxError> {
        let weight = self.dequant_as(
            &format!("{name}.weight"),
            &format!("{name}.scales"),
            &format!("{name}.biases"),
            group_size,
            bits,
            dtype,
        )?;
        let bias_name = format!("{name}.bias");
        let bias = if self.has(&bias_name) {
            Some(self.dense_as(&bias_name, dtype)?)
        } else {
            None
        };
        Linear::new(weight, bias).map_err(|err| MlxError::new(err.to_string()))
    }

    /// 线性层，权重保持分组形状（打包值 + 缩放 + 偏置），不预先反量化。
    ///
    /// 这样前向就能直接走量化矩阵乘：整层的 f16 稠密表从不产生，文件里有多少
    /// 打包字节就读多少。反量化的口子见 `affine_as`，只给不经量化矩阵乘的
    /// 路径（例如激活不是 f16）与归因对照用。
    pub(crate) fn affine_packed(
        &mut self,
        name: &str,
        group_size: usize,
        bits: usize,
        dtype: DType,
    ) -> Result<Linear<MlxTensor>, MlxError> {
        let weight = self.grouped_tensor(&format!("{name}.weight"), group_size, bits, dtype)?;
        let bias_name = format!("{name}.bias");
        let bias = if self.has(&bias_name) {
            Some(self.dense_as(&bias_name, dtype)?)
        } else {
            None
        };
        Linear::new(weight, bias).map_err(|err| MlxError::new(err.to_string()))
    }

    /// 整张分组权重（例如词嵌入表）原样常驻：三块都留在设备上，一元素不还原。
    ///
    /// 词嵌入与 `lm_head` 是同一张 151936×1024 的表（`tie_word_embeddings`）。
    /// 以前的做法是先把整张表反量化成 f16（311 MB）再按行取；现在常驻的就是
    /// 文件里那三块（约 78 + 5 + 5 MB），取几十行时才还原那几十行。
    pub(crate) fn affine_table_packed(
        &mut self,
        name: &str,
        group_size: usize,
        bits: usize,
        dtype: DType,
    ) -> Result<MlxTensor, MlxError> {
        self.grouped_tensor(&format!("{name}.weight"), group_size, bits, dtype)
    }

    /// 把 `key` 这一张分组权重的三块读上设备，逻辑形状按 `(输出, 输入)` 记。
    ///
    /// 打包宽度与组大小必须对得上：一行 `输入` 个数在 {bits} 位下占
    /// `输入 * bits / 32` 个 u32，缩放与偏置各占 `输入 / 组大小` 个。
    fn grouped_tensor(
        &mut self,
        key: &str,
        group_size: usize,
        bits: usize,
        dtype: DType,
    ) -> Result<MlxTensor, MlxError> {
        if group_size == 0 || !voice_tensor::GROUP_SIZE.is_multiple_of(group_size) {
            return Err(MlxError::new(format!("组大小 {group_size} 不是 64 的因子")));
        }
        if !matches!(bits, 2 | 3 | 4 | 5 | 6 | 8) {
            return Err(MlxError::new(format!("分组位数 {bits} 不受支持")));
        }
        let stem = key
            .strip_suffix(".weight")
            .ok_or_else(|| MlxError::new(format!("{key} 不是权重键")))?;
        let packed_loc = self.locate(key)?;
        let scale_loc = self.locate(&format!("{stem}.scales"))?;
        let bias_loc = self.locate(&format!("{stem}.biases"))?;
        if packed_loc.dtype != "U32" {
            return Err(MlxError::new(format!("{key} 不是分组权重")));
        }
        if packed_loc.shape.len() != 2 || scale_loc.shape.len() != 2 || bias_loc.shape.len() != 2 {
            return Err(MlxError::new(format!("{key} 不是二维分组权重")));
        }
        let rows = packed_loc.shape[0];
        if rows == 0 {
            return Err(MlxError::new(format!("{key} 有 0 行")));
        }
        let width = packed_loc.shape[1]
            .checked_mul(32 / bits)
            .ok_or_else(|| MlxError::new(format!("{key} 的输入维溢出")))?;
        if !width.is_multiple_of(group_size) {
            return Err(MlxError::new(format!(
                "{key} 的输入维 {width} 不能按组大小 {group_size} 划分"
            )));
        }
        let groups = width / group_size;
        if scale_loc.shape != [rows, groups] || bias_loc.shape != [rows, groups] {
            return Err(MlxError::new(format!("{key} 的缩放或偏置形状不对")));
        }

        let packed_bytes = self.read_range(&packed_loc)?;
        let scale_bytes = self.read_range(&scale_loc)?;
        let bias_bytes = self.read_range(&bias_loc)?;
        let packed_array = phase!(
            self,
            clock::Slot::Convert,
            array_of(&packed_bytes, &packed_loc.shape, Dtype::Uint32)
        )?;
        let scale_array =
            self.array_of_bytes(&scale_bytes, &scale_loc, &format!("{stem}.scales"), dtype)?;
        let bias_array =
            self.array_of_bytes(&bias_bytes, &bias_loc, &format!("{stem}.biases"), dtype)?;
        drop((packed_bytes, scale_bytes, bias_bytes));
        self.pending
            .extend([packed_array.clone(), scale_array.clone(), bias_array.clone()]);
        Ok(MlxTensor::grouped(
            packed_array,
            scale_array,
            Some(bias_array),
            &[rows, width],
            dtype,
            group_size,
            bits,
        ))
    }

    /// 层归一化：`<name>.weight` 与 `<name>.bias`，eps 由调用方给。
    pub(crate) fn layer_norm(
        &mut self,
        name: &str,
        eps: f32,
        dtype: DType,
    ) -> Result<LayerNorm<MlxTensor>, MlxError> {
        let weight = self.dense_as(&format!("{name}.weight"), dtype)?;
        let bias = self.dense_as(&format!("{name}.bias"), dtype)?;
        LayerNorm::new(weight, Some(bias), eps).map_err(|err| MlxError::new(err.to_string()))
    }

    /// 一张浮点张量：按文件里的位宽建常量数组，再按目标位宽挂 `as_dtype`。
    ///
    /// 文件是 f16、目标也是 f16 时不挂任何节点——这就是省掉的那次主机侧摊开：
    /// 以前 f16 字节要先逐元素解码成 `Vec<f32>`（内存流量翻倍），再拷进 MLX，
    /// 再 `as_dtype` 回 f16；现在字节直接进数组，一步到位。
    fn array_of_bytes(
        &mut self,
        bytes: &Bytes,
        loc: &TensorLoc,
        key: &str,
        dtype: DType,
    ) -> Result<Array, MlxError> {
        let source = match loc.dtype.as_str() {
            "F16" => Dtype::Float16,
            "F32" => Dtype::Float32,
            other => return Err(MlxError::new(format!("{key} 的数据类型 {other} 不是浮点"))),
        };
        let target = mlx_dtype(dtype)?;
        let array = phase!(
            self,
            clock::Slot::Convert,
            array_of(bytes, &loc.shape, source)
        )?;
        if source == target {
            return Ok(array);
        }
        phase!(
            self,
            clock::Slot::Convert,
            array.as_dtype(target).map_err(from_mlx)
        )
    }

    /// 分组权重反量化到 `dtype`。
    fn dequant_as(
        &mut self,
        weight: &str,
        scales: &str,
        biases: &str,
        group_size: usize,
        bits: usize,
        dtype: DType,
    ) -> Result<MlxTensor, MlxError> {
        if group_size == 0 || !voice_tensor::GROUP_SIZE.is_multiple_of(group_size) {
            return Err(MlxError::new(format!("组大小 {group_size} 不是 64 的因子")));
        }
        let packed_loc = self.locate(weight)?;
        let scale_loc = self.locate(scales)?;
        let bias_loc = self.locate(biases)?;
        if packed_loc.dtype != "U32" {
            return Err(MlxError::new(format!("{weight} 不是分组权重")));
        }
        let packed_bytes = self.read_range(&packed_loc)?;
        let scale_bytes = self.read_range(&scale_loc)?;
        let bias_bytes = self.read_range(&bias_loc)?;
        let packed_array = phase!(
            self,
            clock::Slot::Convert,
            array_of(&packed_bytes, &packed_loc.shape, Dtype::Uint32)
        )?;
        let scale_array = self.array_of_bytes(&scale_bytes, &scale_loc, scales, dtype)?;
        let bias_array = self.array_of_bytes(&bias_bytes, &bias_loc, biases, dtype)?;
        drop((packed_bytes, scale_bytes, bias_bytes));

        let bits_i32 =
            i32::try_from(bits).map_err(|_| MlxError::new(format!("分组位数 {bits} 超出 i32")))?;
        let restored = phase!(self, clock::Slot::Convert, {
            let restored = ops::dequantize(
                &packed_array,
                &scale_array,
                Some(&bias_array),
                DequantizeOptions {
                    mode: QuantizationMode::Affine,
                    group_size: Some(i32::try_from(group_size).unwrap_or(64)),
                    bits: Some(bits_i32),
                },
            )
            .map_err(from_mlx)?;
            restored.as_dtype(mlx_dtype(dtype)?).map_err(from_mlx)
        })?;
        let dims: Vec<usize> = restored
            .shape()
            .iter()
            .map(|dim| usize::try_from(*dim).unwrap_or(0))
            .collect();
        if dims.iter().any(|dim| *dim == 0) {
            return Err(MlxError::new(format!("{weight} 反量化形状无效")));
        }
        self.pending.push(restored.clone());
        Ok(MlxTensor::dense(restored, dtype))
    }

    fn locate(&self, key: &str) -> Result<TensorLoc, MlxError> {
        let object = object_for(&self.header, key)?;
        let dtype = json_string(object, "dtype")?;
        let shape = json_ints(object, "shape")?;
        let offsets = json_ints(object, "data_offsets")?;
        if offsets.len() != 2 || offsets[1] < offsets[0] {
            return Err(MlxError::new(format!("{key} 的数据区间无效")));
        }
        Ok(TensorLoc {
            dtype,
            shape,
            start: offsets[0] as u64,
            end: offsets[1] as u64,
        })
    }

    fn read_range(&mut self, loc: &TensorLoc) -> Result<Bytes, MlxError> {
        let len = loc
            .end
            .checked_sub(loc.start)
            .ok_or_else(|| MlxError::new("数据区间回绕"))?;
        if len > 1024 * 1024 * 1024 {
            return Err(MlxError::new("单个权重超过 1GB"));
        }
        phase!(self, clock::Slot::Read, {
            #[cfg(test)]
            if let Some(phases) = self.phases.as_mut() {
                phases.bytes += len;
                phases.reads += 1;
            }
            self.file
                .seek(SeekFrom::Start(self.data_base + loc.start))
                .map_err(|_| MlxError::new("读权重失败"))?;
            let mut buf = Bytes::zeroed(len as usize);
            self.file
                .read_exact(buf.as_mut_slice())
                .map_err(|_| MlxError::new("读权重失败"))?;
            Ok(buf)
        })
    }
}

/// 用一段主机字节建一个常量数组。字节数必须与形状对得上。
///
/// `mlx_array_set_data` 会把这段字节拷进 MLX 自己的缓冲，所以调用方在返回之后
/// 就能把 `bytes` 放开；这一次拷贝是「有且只有一次」——以前是「读进 `Vec<u8>`、
/// 摊成 `Vec<f32>`、再拷进 MLX」三次。
fn array_of(bytes: &Bytes, dims: &[usize], dtype: Dtype) -> Result<Array, MlxError> {
    let shape = i32_dims(dims)?;
    let count = numel(dims)?;
    let item = item_size(dtype)?;
    if bytes.len != count * item {
        return Err(MlxError::new("张量字节数与形状不一致"));
    }
    // SAFETY：`bytes` 起点 8 字节对齐，长度已按 `count * item` 对过账，拷贝在
    // `mlx_array_set_data` 里发生、随即返回。
    unsafe { Ok(Array::from_raw_data(bytes.as_ptr(), &shape, dtype)) }
}

fn item_size(dtype: Dtype) -> Result<usize, MlxError> {
    match dtype {
        Dtype::Float16 => Ok(2),
        Dtype::Float32 => Ok(4),
        Dtype::Uint32 => Ok(4),
        other => Err(MlxError::new(format!("权重里没有 {other:?} 这种位宽"))),
    }
}

fn mlx_dtype(dtype: DType) -> Result<Dtype, MlxError> {
    match dtype {
        DType::F32 => Ok(Dtype::Float32),
        DType::F16 => Ok(Dtype::Float16),
        DType::I32 => Ok(Dtype::Int32),
        DType::I16 => Ok(Dtype::Int16),
        DType::Bool => Ok(Dtype::Bool),
    }
}

fn object_for<'a>(header: &'a str, key: &str) -> Result<&'a str, MlxError> {
    let pattern = format!("\"{key}\"");
    let at = header
        .find(&pattern)
        .ok_or_else(|| MlxError::new(format!("权重里没有 {key}")))?;
    let after = &header[at + pattern.len()..];
    let brace = after
        .find('{')
        .ok_or_else(|| MlxError::new(format!("{key} 没有对象")))?;
    let bytes = after[brace..].as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match *byte {
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&after[brace..brace + index + 1]);
                }
            }
            _ => {}
        }
    }
    Err(MlxError::new(format!("{key} 的对象没有结束")))
}

fn json_string(object: &str, field: &str) -> Result<String, MlxError> {
    let pattern = format!("\"{field}\"");
    let at = object
        .find(&pattern)
        .ok_or_else(|| MlxError::new(format!("缺少 {field}")))?;
    let rest = &object[at + pattern.len()..];
    let quote = rest
        .find('"')
        .ok_or_else(|| MlxError::new(format!("{field} 不是字符串")))?;
    let tail = &rest[quote + 1..];
    let end = tail
        .find('"')
        .ok_or_else(|| MlxError::new(format!("{field} 没有结束引号")))?;
    Ok(tail[..end].to_string())
}

fn json_ints(object: &str, field: &str) -> Result<Vec<usize>, MlxError> {
    let pattern = format!("\"{field}\"");
    let at = object
        .find(&pattern)
        .ok_or_else(|| MlxError::new(format!("缺少 {field}")))?;
    let rest = &object[at + pattern.len()..];
    let open = rest
        .find('[')
        .ok_or_else(|| MlxError::new(format!("{field} 不是数组")))?;
    let close = rest[open..]
        .find(']')
        .ok_or_else(|| MlxError::new(format!("{field} 没有结束括号")))?;
    let body = &rest[open + 1..open + close];
    if body.trim().is_empty() {
        return Ok(Vec::new());
    }
    body.split(',')
        .map(|part| {
            part.trim()
                .parse::<usize>()
                .map_err(|_| MlxError::new(format!("{field} 含有非整数字")))
        })
        .collect()
}

fn numel(shape: &[usize]) -> Result<usize, MlxError> {
    shape
        .iter()
        .try_fold(1usize, |acc, dim| acc.checked_mul(*dim))
        .ok_or_else(|| MlxError::new("形状溢出"))
}

fn i32_dims(dims: &[usize]) -> Result<Vec<i32>, MlxError> {
    dims.iter()
        .map(|dim| i32::try_from(*dim).map_err(|_| MlxError::new(format!("维度 {dim} 超出 i32"))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_helpers_read_a_tensor_entry() {
        let object = r#"{"dtype":"F16","shape":[480,3,3,1],"data_offsets":[0,2880]}"#;
        assert_eq!(json_string(object, "dtype").unwrap(), "F16");
        assert_eq!(json_ints(object, "shape").unwrap(), vec![480, 3, 3, 1]);
        assert_eq!(json_ints(object, "data_offsets").unwrap(), vec![0, 2880]);
    }

    /// 装载的四段墙钟：表头、字节读取、主机侧转换、MLX 侧求值与上传。
    ///
    /// 这条只打数，不是对照闸，默认不跑。跑法见 `notes/065`：
    ///
    /// ```text
    /// cargo test -p voice-mlx --features metal --target aarch64-apple-darwin --offline \
    ///   --lib load_phases -- --ignored --nocapture --test-threads=1
    /// ```
    ///
    /// 四项分开报，不把和当一个数：装载的总时长里这四项之外还有 `locate`
    /// （在表头里搜键名、解 JSON）与建层结构本身，那部分不算在四格内。
    #[test]
    #[ignore = "只打四段墙钟，不是对照闸"]
    fn load_phases_are_reported_one_by_one() {
        use crate::backend::{AudioTower, MlxBackend, QuantPolicy, TextDecoder};
        use voice_tensor::DType;

        let dir = model_dir();
        MlxBackend::prepare(metallib().to_str().expect("metallib 路径")).expect("MLX stream");
        let dir = dir.to_str().expect("模型目录").to_string();

        let mut weights = SafeWeights::open_counting(&dir).expect("打开权重");
        let header = weights.phases().expect("计时开着");
        println!("表头 {:.4} s（{} 字节）", header.header, header.bytes);

        let at = std::time::Instant::now();
        let tower = AudioTower::load(&mut weights, DType::F16).expect("音频塔");
        let tower_wall = at.elapsed().as_secs_f64();
        let after_tower = weights.phases().expect("计时开着");
        report("音频塔", tower_wall, header, after_tower);

        let at = std::time::Instant::now();
        let decoder =
            TextDecoder::load(&mut weights, DType::F16, QuantPolicy::default()).expect("文本塔");
        let decoder_wall = at.elapsed().as_secs_f64();
        let after_decoder = weights.phases().expect("计时开着");
        report("文本塔", decoder_wall, after_tower, after_decoder);

        let at = std::time::Instant::now();
        weights.finish().expect("一次求值");
        let finish_wall = at.elapsed().as_secs_f64();
        let after_finish = weights.phases().expect("计时开着");
        report("一次 eval", finish_wall, after_decoder, after_finish);

        report("两塔合计", tower_wall + decoder_wall, header, after_decoder);
        println!(
            "张量：音频塔 {} 层、文本塔 {} 层",
            tower.layer_count(),
            decoder.layer_count()
        );
        let all = after_finish.since(&clock::Phases::default());
        println!(
            "整个装载四格和 {:.3} s，其中表头 {:.4}，四项之外（locate/建层）{:.3} s",
            all.total(),
            all.header,
            tower_wall + decoder_wall + finish_wall - (all.total() - all.header)
        );
        assert!(after_finish.reads > 0);
        drop((tower, decoder));
    }

    fn report(label: &str, wall: f64, before: clock::Phases, after: clock::Phases) {
        let d = after.since(&before);
        println!(
            "{label}：墙钟 {:.3} s｜表头 {:.4}｜字节读取 {:.3}（{} 次、{} 字节）｜转换 {:.3}｜求值与上传 {:.3}（{} 次）｜四格和 {:.3}",
            wall,
            d.header,
            d.read,
            d.reads,
            d.bytes,
            d.convert,
            d.eval,
            d.evals,
            d.total()
        );
    }

    /// 模型目录：`VOICE_MODEL_DIR` 优先，否则仓里的资产目录。
    fn model_dir() -> std::path::PathBuf {
        if let Some(dir) = std::env::var_os("VOICE_MODEL_DIR") {
            return std::path::PathBuf::from(dir);
        }
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/qwen3-asr-0.6b-4bit")
            .canonicalize()
            .expect("模型目录")
    }

    /// `mlx.metallib`：`VOICE_METALLIB` 优先，否则用装好的包里的那份。
    fn metallib() -> std::path::PathBuf {
        if let Some(path) = std::env::var_os("VOICE_METALLIB") {
            return std::path::PathBuf::from(path);
        }
        let home = std::env::var("HOME").expect("HOME");
        std::path::PathBuf::from(home)
            .join("Library/Input Methods/RimeClient.app/Contents/Resources/mlx.metallib")
    }
}
