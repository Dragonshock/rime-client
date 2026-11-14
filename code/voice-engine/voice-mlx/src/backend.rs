//! 只在 `metal` 特性下编译。运算走 `mlx-rs` 0.34 的 GPU stream。

mod audio_tower;
mod text_decoder;
mod transcribe;
mod weights;

pub use audio_tower::{
    encode_mel, encode_mel_with, AudioTower, EncoderOptions, EncoderOutput, PositionDenominator,
};
pub use text_decoder::{
    DecoderOptions, Generation, PrefillLogits, QuantPolicy, RopeMode, StopReason, TextDecoder, EOS,
    IM_END, LM_HEAD_BLOCK, MAX_NEW_TOKENS,
};
pub use transcribe::{
    build_prompt, prompt_tokens, transcribe, transcribe_with, transcribe_with_opened, Engine,
    Opened, Transcription, PROMPT_LANGUAGE,
};

use std::cell::RefCell;
use std::fmt;

use mlx_rs::ops::indexing::take_axis;
use mlx_rs::ops::{DequantizeOptions, QuantizationMode, QuantizedMatmulOptions};
use mlx_rs::{fast, ops, random, with_stream, Array, Device, Dtype, Stream};
use voice_tensor::{
    AxisSlice, Conv2d, DType, KvCache, LayerNorm, Linear, PadWidth, RmsNorm, Storage, TensorMeta,
    TensorOps,
};

thread_local! {
    static GPU: RefCell<Option<Stream>> = RefCell::new(None);
}

/// MLX 调用失败。
#[derive(Debug)]
pub struct MlxError {
    message: String,
}

impl MlxError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for MlxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MlxError {}

fn from_mlx(error: impl fmt::Display) -> MlxError {
    MlxError::new(error.to_string())
}

/// 引擎线程上的一个 MLX 张量。逻辑形状由本类型保存。
#[derive(Clone)]
pub struct MlxTensor {
    array: Array,
    scales: Option<Array>,
    biases: Option<Array>,
    dims: Vec<usize>,
    dtype: DType,
    storage: Storage,
}

impl MlxTensor {
    fn dense(array: Array, dtype: DType) -> Self {
        let dims = array.shape().iter().map(|dim| *dim as usize).collect();
        Self {
            array,
            scales: None,
            biases: None,
            dims,
            dtype,
            storage: Storage::Dense,
        }
    }

    /// 分组权重。`packed`、`scales`、`biases` 三块分开存放，求值时按组还原。
    ///
    /// 三块都留在设备上，不预先拼成 `dtype` 的稠密表：量化矩阵乘直接吃这三块，
    /// 只有在需要取若干行或需要 `dtype` 不是 f16 时才把那一小部分还原。
    pub fn grouped(
        packed: Array,
        scales: Array,
        biases: Option<Array>,
        logical: &[usize],
        dtype: DType,
        group_size: usize,
        bits: usize,
    ) -> Self {
        Self {
            array: packed,
            scales: Some(scales),
            biases,
            dims: logical.to_vec(),
            dtype,
            storage: Storage::Grouped { group_size, bits },
        }
    }
}

impl TensorMeta for MlxTensor {
    fn dims(&self) -> &[usize] {
        &self.dims
    }

    fn dtype(&self) -> DType {
        self.dtype
    }

    fn storage(&self) -> Storage {
        self.storage
    }
}

/// 这根线程的 GPU stream。由 [`MlxBackend::prepare`] 创建。
pub struct MlxBackend {
    stream: Stream,
}

impl MlxBackend {
    /// 在引擎线程上设置 metallib，并创建 GPU stream。必须先于任何张量。
    pub fn prepare(metallib: &str) -> Result<(), MlxError> {
        if !std::path::Path::new(metallib).is_file() {
            return Err(MlxError::new(format!("metallib 不是文件：{metallib}")));
        }
        mlx_rs::metal::set_metallib_path(metallib).map_err(from_mlx)?;
        let device = Device::gpu();
        let stream = Stream::new_with_device(&device);
        with_stream(&stream, || {
            let probe = ops::zeros::<f32>(&[1]).map_err(from_mlx)?;
            probe.eval().map_err(from_mlx)?;
            Ok::<(), MlxError>(())
        })?;
        GPU.with(|slot| *slot.borrow_mut() = Some(stream));
        Ok(())
    }

    /// 取出本线程已经准备好的 stream。
    pub fn on_this_thread() -> Result<Self, MlxError> {
        let stream = GPU
            .with(|slot| slot.borrow().clone())
            .ok_or_else(|| MlxError::new("本线程还没有 GPU stream"))?;
        Ok(Self { stream })
    }

    fn on<T>(&self, body: impl FnOnce() -> Result<T, MlxError>) -> Result<T, MlxError> {
        with_stream(&self.stream, body)
    }

    /// 把主机上的 f32 数据放成一个张量，必要时换成 `dtype`。
    ///
    /// 正弦位置表、加法 mask、梅尔谱这三样在参考里都是先在主机上按 f32 算好，
    /// 再转成激活类型。走这条口子，形状与元素个数当场校验。
    pub fn literal(
        &mut self,
        data: &[f32],
        dims: &[usize],
        dtype: DType,
    ) -> Result<MlxTensor, MlxError> {
        let numel = dims
            .iter()
            .try_fold(1usize, |acc, dim| acc.checked_mul(*dim))
            .ok_or_else(|| MlxError::new("形状溢出"))?;
        if numel != data.len() {
            return Err(MlxError::new(format!(
                "元素个数 {} 与形状 {} 不一致",
                data.len(),
                dims.iter()
                    .map(|d| d.to_string())
                    .collect::<Vec<_>>()
                    .join("×")
            )));
        }
        let dims = i32_shape(dims)?;
        self.on(|| {
            let array = Array::from_slice(data, &dims);
            let array = if dtype == DType::F32 {
                array
            } else {
                cast_dtype(array, dtype)?
            };
            Ok(MlxTensor::dense(array, dtype))
        })
    }

    /// 按行取二维张量的子表，形状 `(行数, 宽度)`。
    ///
    /// 行在源张量里各自连续，取出来的元素与整表切片逐位相同，所以常驻一张表
    /// 也能按需拿其中若干行，不必回到权重文件。分组权重按同样的下标取三块，
    /// 取出来的仍是分组权重：不预先还原成稠密表。
    pub fn take_rows(&mut self, input: &MlxTensor, rows: &[usize]) -> Result<MlxTensor, MlxError> {
        if input.dims.len() != 2 {
            return Err(MlxError::new("取行要求二维张量"));
        }
        if rows.is_empty() {
            return Err(MlxError::new("取行的行数为空"));
        }
        let height = input.dims[0];
        let mut index = Vec::with_capacity(rows.len());
        for row in rows {
            if *row >= height {
                return Err(MlxError::new(format!("行下标 {row} 越界")));
            }
            index.push(*row as i32);
        }
        let storage = input.storage;
        self.on(|| {
            let index = Array::from_slice(&index, &[index.len() as i32]);
            match storage {
                Storage::Dense => {
                    let array = take_axis(&input.array, &index, 0).map_err(from_mlx)?;
                    Ok(MlxTensor::dense(array, input.dtype))
                }
                Storage::Grouped { group_size, bits } => {
                    let scales = input
                        .scales
                        .as_ref()
                        .ok_or_else(|| MlxError::new("分组权重缺少缩放"))?;
                    let packed = take_axis(&input.array, &index, 0).map_err(from_mlx)?;
                    let scales = take_axis(scales, &index, 0).map_err(from_mlx)?;
                    let biases = match input.biases.as_ref() {
                        Some(biases) => Some(take_axis(biases, &index, 0).map_err(from_mlx)?),
                        None => None,
                    };
                    Ok(MlxTensor::grouped(
                        packed,
                        scales,
                        biases,
                        &[rows.len(), input.dims[1]],
                        input.dtype,
                        group_size,
                        bits,
                    ))
                }
            }
        })
    }

    /// 把一个分组权重还原成 `dtype` 的稠密张量。
    ///
    /// 只用于小张量：取词嵌入的几十行、或者激活不是 f16 时的退路。整层的
    /// 稠密表不走这里，走 [`TensorOps::linear`] 里的量化矩阵乘。
    pub fn materialize(&mut self, tensor: &MlxTensor) -> Result<MlxTensor, MlxError> {
        let storage = tensor.storage;
        if storage == Storage::Dense {
            return Ok(tensor.clone());
        }
        self.on(|| {
            let array = material_weight(tensor)?;
            Ok(MlxTensor::dense(array, tensor.dtype))
        })
    }

    /// 求值并把一个浮点张量取回主机，按行优先展平成 f32。
    ///
    /// 分组权重先还原成 `dtype` 再取回；调用方拿到的行数与调用前一致。
    pub fn to_f32(&mut self, tensor: &MlxTensor) -> Result<Vec<f32>, MlxError> {
        let tensor = if tensor.storage == Storage::Dense {
            tensor.clone()
        } else {
            self.materialize(tensor)?
        };
        tensor.array.eval().map_err(from_mlx)?;
        let array = if tensor.dtype == DType::F32 {
            tensor.array.clone()
        } else {
            tensor.array.as_dtype(Dtype::Float32).map_err(from_mlx)?
        };
        array.eval().map_err(from_mlx)?;
        let values = array
            .try_as_slice::<f32>()
            .map_err(|error| MlxError::new(error.to_string()))?;
        Ok(values.to_vec())
    }
}

fn i32_shape(shape: &[usize]) -> Result<Vec<i32>, MlxError> {
    shape
        .iter()
        .map(|dim| i32::try_from(*dim).map_err(|_| MlxError::new(format!("维度 {dim} 超出 i32"))))
        .collect()
}

fn zeros_of(shape: &[usize], dtype: DType) -> Result<Array, MlxError> {
    let shape = i32_shape(shape)?;
    let mlx = match dtype {
        DType::F32 => Dtype::Float32,
        DType::F16 => Dtype::Float16,
        DType::I32 => Dtype::Int32,
        DType::I16 => Dtype::Int16,
        DType::Bool => Dtype::Bool,
    };
    ops::zeros_dtype(&shape, mlx).map_err(from_mlx)
}

fn cast_dtype(array: Array, dtype: DType) -> Result<Array, MlxError> {
    let mlx = cast_of(dtype);
    array.as_dtype(mlx).map_err(from_mlx)
}

/// 工程内部的 `DType` → `mlx` 的类型。
fn cast_of(dtype: DType) -> Dtype {
    match dtype {
        DType::F32 => Dtype::Float32,
        DType::F16 => Dtype::Float16,
        DType::I32 => Dtype::Int32,
        DType::I16 => Dtype::Int16,
        DType::Bool => Dtype::Bool,
    }
}

fn infer_reshape(numel: usize, shape: &[isize]) -> Result<Vec<i32>, MlxError> {
    let mut inferred = None;
    let mut known = 1usize;
    for (index, dim) in shape.iter().copied().enumerate() {
        if dim == -1 {
            if inferred.is_some() {
                return Err(MlxError::new("reshape 只能有一个 -1"));
            }
            inferred = Some(index);
        } else if dim < 0 {
            return Err(MlxError::new(format!("reshape 维度 {dim} 非法")));
        } else {
            known = known
                .checked_mul(dim as usize)
                .ok_or_else(|| MlxError::new("reshape 形状溢出"))?;
        }
    }
    if known == 0 || !numel.is_multiple_of(known) {
        return Err(MlxError::new("reshape 不能整除元素个数"));
    }
    let mut out = Vec::with_capacity(shape.len());
    for dim in shape {
        if *dim == -1 {
            out.push(i32::try_from(numel / known).map_err(|_| MlxError::new("reshape 超出 i32"))?);
        } else {
            out.push(*dim as i32);
        }
    }
    Ok(out)
}

/// 分组权重还原成稠密表。
///
/// 缩放与偏置都是 f16 存的（`quantization_config.json` 只写了位数与组大小，
/// 类型由权重表头给出），还原出来的类型就是它们各自的类型；调用方再按需转换。
fn material_weight(tensor: &MlxTensor) -> Result<Array, MlxError> {
    match tensor.storage {
        Storage::Dense => Ok(tensor.array.clone()),
        Storage::Grouped { group_size, bits } => {
            let Some(scales) = tensor.scales.as_ref() else {
                return Err(MlxError::new("分组权重缺少缩放"));
            };
            ops::dequantize(
                &tensor.array,
                scales,
                tensor.biases.as_ref(),
                DequantizeOptions {
                    mode: QuantizationMode::Affine,
                    group_size: Some(i32::try_from(group_size).unwrap_or(64)),
                    bits: Some(i32::try_from(bits).unwrap_or(4)),
                },
            )
            .map_err(from_mlx)
        }
    }
}

fn repeat_heads(array: &Array, times: usize) -> Result<Array, MlxError> {
    if times == 1 {
        return Ok(array.clone());
    }
    let shape = array.shape();
    if shape.len() != 4 {
        return Err(MlxError::new("注意力键值必须是四维"));
    }
    let expanded =
        ops::reshape(array, &[shape[0], shape[1], 1, shape[2], shape[3]]).map_err(from_mlx)?;
    let tiled = ops::tile(&expanded, &[1, 1, times as i32, 1, 1]).map_err(from_mlx)?;
    let heads = shape[1]
        .checked_mul(times as i32)
        .ok_or_else(|| MlxError::new("注意力头数溢出"))?;
    ops::reshape(&tiled, &[shape[0], heads, shape[2], shape[3]]).map_err(from_mlx)
}

impl TensorOps for MlxBackend {
    type Tensor = MlxTensor;
    type Error = MlxError;

    fn zeros(&mut self, shape: &[usize], dtype: DType) -> Result<Self::Tensor, Self::Error> {
        self.on(|| Ok(MlxTensor::dense(zeros_of(shape, dtype)?, dtype)))
    }

    fn arange(
        &mut self,
        start: i64,
        stop: i64,
        step: i64,
        dtype: DType,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            if step == 0 {
                return Err(MlxError::new("arange 步长不能为 0"));
            }
            let array = match dtype {
                DType::I32 | DType::I16 | DType::Bool => {
                    ops::arange::<i64, i32>(Some(start), stop, Some(step)).map_err(from_mlx)?
                }
                DType::F32 | DType::F16 => {
                    ops::arange::<i64, f32>(Some(start), stop, Some(step)).map_err(from_mlx)?
                }
            };
            let array = cast_dtype(array, dtype)?;
            Ok(MlxTensor::dense(array, dtype))
        })
    }

    fn normal(
        &mut self,
        shape: &[usize],
        mean: f32,
        std: f32,
        dtype: DType,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            if !dtype.is_float() {
                return Err(MlxError::new("正态需要浮点"));
            }
            let shape = i32_shape(shape)?;
            let array = random::normal::<f32>(Some(shape.as_slice()), Some(mean), Some(std), None)
                .map_err(from_mlx)?;
            Ok(MlxTensor::dense(cast_dtype(array, dtype)?, dtype))
        })
    }

    fn pcm_to_f32(&mut self, pcm: &[i16]) -> Result<Self::Tensor, Self::Error> {
        let widened: Vec<i32> = pcm.iter().copied().map(i32::from).collect();
        self.on(|| {
            let array = Array::from_slice(&widened, &[widened.len() as i32]);
            Ok(MlxTensor::dense(cast_dtype(array, DType::F32)?, DType::F32))
        })
    }

    fn reshape(
        &mut self,
        input: &Self::Tensor,
        shape: &[isize],
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let numel = input
                .dims
                .iter()
                .try_fold(1usize, |acc, dim| acc.checked_mul(*dim));
            let numel = numel.ok_or_else(|| MlxError::new("reshape 元素个数溢出"))?;
            let shape = infer_reshape(numel, shape)?;
            let array = ops::reshape(&input.array, &shape).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn transpose(
        &mut self,
        input: &Self::Tensor,
        axes: &[usize],
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let axes = axes
                .iter()
                .map(|axis| i32::try_from(*axis).map_err(|_| MlxError::new("转置轴超出 i32")))
                .collect::<Result<Vec<_>, _>>()?;
            let array = ops::transpose_axes(&input.array, &axes).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn slice(
        &mut self,
        input: &Self::Tensor,
        ranges: &[AxisSlice],
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            if ranges.len() != input.dims.len() {
                return Err(MlxError::new("slice 的区间个数与秩不一致"));
            }
            let mut array = input.array.clone();
            for (axis, range) in ranges.iter().enumerate() {
                let len = input.dims[axis];
                if range.end < range.start || range.end > len {
                    return Err(MlxError::new("slice 区间越界"));
                }
                if range.start == 0 && range.end == len {
                    continue;
                }
                let index = ops::arange::<i32, i32>(
                    Some(range.start as i32),
                    range.end as i32,
                    None::<i32>,
                )
                .map_err(from_mlx)?;
                array = take_axis(&array, &index, axis as i32).map_err(from_mlx)?;
            }
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn pad(
        &mut self,
        input: &Self::Tensor,
        pads: &[PadWidth],
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            if pads.len() != input.dims.len() {
                return Err(MlxError::new("pad 的宽度个数与秩不一致"));
            }
            let widths: Vec<(i32, i32)> = pads
                .iter()
                .map(|pad| (pad.before as i32, pad.after as i32))
                .collect();
            let array = ops::pad(
                &input.array,
                widths.as_slice(),
                None::<Array>,
                None::<ops::PadMode>,
            )
            .map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn concatenate(
        &mut self,
        inputs: &[&Self::Tensor],
        axis: usize,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let dtype = inputs
                .first()
                .map(|tensor| tensor.dtype)
                .ok_or_else(|| MlxError::new("concatenate 没有输入"))?;
            let refs: Vec<&Array> = inputs.iter().map(|tensor| &tensor.array).collect();
            let array = ops::concatenate(refs.as_slice(), axis as i32).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, dtype))
        })
    }

    fn where_op(
        &mut self,
        cond: &Self::Tensor,
        when_true: &Self::Tensor,
        when_false: &Self::Tensor,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array =
                ops::select(&cond.array, &when_true.array, &when_false.array).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, when_true.dtype))
        })
    }

    fn cumsum(&mut self, input: &Self::Tensor, axis: usize) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array =
                ops::cumsum(&input.array, Some(axis as i32), None, None).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn gather(
        &mut self,
        input: &Self::Tensor,
        index: &Self::Tensor,
        axis: usize,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array = take_axis(&input.array, &index.array, axis as i32).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn add(&mut self, lhs: &Self::Tensor, rhs: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array = ops::add(&lhs.array, &rhs.array).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, lhs.dtype))
        })
    }

    fn mul(&mut self, lhs: &Self::Tensor, rhs: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array = ops::multiply(&lhs.array, &rhs.array).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, lhs.dtype))
        })
    }

    fn rsqrt(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array = ops::rsqrt(&input.array).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn exp(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array = ops::exp(&input.array).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn sin(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array = ops::sin(&input.array).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn cos(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array = ops::cos(&input.array).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn clip(
        &mut self,
        input: &Self::Tensor,
        min: f32,
        max: f32,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array = ops::clip(&input.array, (min, max)).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn gelu(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array = mlx_rs::nn::gelu(&input.array).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn silu(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array = mlx_rs::nn::silu(&input.array).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn linear(
        &mut self,
        input: &Self::Tensor,
        layer: &Linear<Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let weight = layer.weight();
            let storage = weight.storage();
            let mut array = match storage {
                // 分组权重直接走 MLX 的量化矩阵乘：打包值、缩放、偏置三块进核，
                // 不在外面先拼一张 f16 的整表。`transpose: true` 是因为表里的
                // 打包权重按 `(输出, 输入)` 排，与 `Linear` 的逻辑形状一致。
                Storage::Grouped { group_size, bits } if input.dtype == DType::F16 => {
                    let scales = weight
                        .scales
                        .as_ref()
                        .ok_or_else(|| MlxError::new("分组权重缺少缩放"))?;
                    ops::quantized_matmul(
                        &input.array,
                        &weight.array,
                        scales,
                        weight.biases.as_ref(),
                        QuantizedMatmulOptions {
                            mode: QuantizationMode::Affine,
                            group_size: Some(i32::try_from(group_size).unwrap_or(64)),
                            bits: Some(i32::try_from(bits).unwrap_or(4)),
                            transpose: true,
                        },
                    )
                    .map_err(from_mlx)?
                }
                // 激活不是 f16（以及稠密权重）时走通用路：先按组还原再乘。
                // 这条与量化矩阵乘同值，只是把还原显式化了。
                _ => {
                    let weight = material_weight(weight)?;
                    // 激活与权重的浮点类型可能不同（比如 f32 激活读 f16 权重），
                    // 让右乘数跟上激活，不改变每一行的元素。
                    let weight = if weight.dtype() == cast_of(input.dtype) {
                        weight
                    } else {
                        weight.as_dtype(cast_of(input.dtype)).map_err(from_mlx)?
                    };
                    let weight = ops::transpose(&weight).map_err(from_mlx)?;
                    ops::matmul(&input.array, &weight).map_err(from_mlx)?
                }
            };
            if let Some(bias) = layer.bias() {
                array = ops::add(&array, &bias.array).map_err(from_mlx)?;
            }
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn conv2d(
        &mut self,
        input: &Self::Tensor,
        layer: &Conv2d<Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let weight = material_weight(layer.weight())?;
            let mut array = ops::conv2d(
                &input.array,
                &weight,
                Some((layer.stride() as i32, layer.stride() as i32)),
                Some((layer.pad() as i32, layer.pad() as i32)),
                None,
                None,
            )
            .map_err(from_mlx)?;
            if let Some(bias) = layer.bias() {
                array = ops::add(&array, &bias.array).map_err(from_mlx)?;
            }
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn layer_norm(
        &mut self,
        input: &Self::Tensor,
        norm: &LayerNorm<Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let bias = norm.bias().map(|bias| &bias.array);
            let array =
                fast::layer_norm(&input.array, Some(&norm.weight().array), bias, norm.eps())
                    .map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn rms_norm(
        &mut self,
        input: &Self::Tensor,
        norm: &RmsNorm<Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array = fast::rms_norm(&input.array, Some(&norm.weight().array), norm.eps())
                .map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn attention(
        &mut self,
        query: &Self::Tensor,
        key: &Self::Tensor,
        value: &Self::Tensor,
        mask: Option<&Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let q_heads = query.dims.get(1).copied().unwrap_or(0);
            let kv_heads = key.dims.get(1).copied().unwrap_or(0);
            if kv_heads == 0 || !q_heads.is_multiple_of(kv_heads) {
                return Err(MlxError::new("键值头数不能整除查询头数"));
            }
            let head_dim = *query.dims.last().unwrap_or(&0);
            if head_dim == 0 {
                return Err(MlxError::new("注意力头维为 0"));
            }
            let key = repeat_heads(&key.array, q_heads / kv_heads)?;
            let value = repeat_heads(&value.array, q_heads / kv_heads)?;
            let scale = (head_dim as f32).sqrt().recip();
            let array = if let Some(mask) = mask {
                fast::scaled_dot_product_attention(
                    &query.array,
                    &key,
                    &value,
                    scale,
                    &mask.array,
                    None::<&Array>,
                )
            } else {
                fast::scaled_dot_product_attention(
                    &query.array,
                    &key,
                    &value,
                    scale,
                    None::<fast::ScaledDotProductAttentionMask>,
                    None::<&Array>,
                )
            }
            .map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, query.dtype))
        })
    }

    fn kv_write(
        &mut self,
        cache: &mut KvCache<Self::Tensor>,
        key: &Self::Tensor,
        value: &Self::Tensor,
    ) -> Result<(), Self::Error> {
        let key_next = match cache.key() {
            Some(existing) => self.concatenate(&[existing, key], 2)?,
            None => key.clone(),
        };
        let value_next = match cache.value() {
            Some(existing) => self.concatenate(&[existing, value], 2)?,
            None => value.clone(),
        };
        cache.store(key_next, value_next);
        Ok(())
    }

    fn kv_read(
        &mut self,
        cache: &KvCache<Self::Tensor>,
    ) -> Result<(Self::Tensor, Self::Tensor), Self::Error> {
        match (cache.key(), cache.value()) {
            (Some(key), Some(value)) => Ok((key.clone(), value.clone())),
            _ => Err(MlxError::new("KV 缓存是空的")),
        }
    }

    fn sum(
        &mut self,
        input: &Self::Tensor,
        axis: usize,
        keep_dim: bool,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array =
                ops::sum_axis(&input.array, axis as i32, Some(keep_dim)).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn max(
        &mut self,
        input: &Self::Tensor,
        axis: usize,
        keep_dim: bool,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array =
                ops::max_axis(&input.array, axis as i32, Some(keep_dim)).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }

    fn min(
        &mut self,
        input: &Self::Tensor,
        axis: usize,
        keep_dim: bool,
    ) -> Result<Self::Tensor, Self::Error> {
        self.on(|| {
            let array =
                ops::min_axis(&input.array, axis as i32, Some(keep_dim)).map_err(from_mlx)?;
            Ok(MlxTensor::dense(array, input.dtype))
        })
    }
}
