use crate::dtype::DType;
use crate::error::TensorError;
use crate::shape::{axis_index, checked_numel, conv2d_spatial_out, format_dims};

/// 张量对外暴露的形状、数据类型和存放方式。
pub trait TensorMeta {
    /// 各维长度。
    fn dims(&self) -> &[usize];

    /// 数据类型。
    fn dtype(&self) -> DType;

    /// 存放方式。分组权重的形状仍是反量化后的逻辑形状。
    fn storage(&self) -> Storage;

    /// 秩。
    fn rank(&self) -> usize {
        self.dims().len()
    }

    /// 元素个数。
    fn numel(&self) -> Result<usize, TensorError> {
        checked_numel("numel", self.dims())
    }
}

/// 权重的存放方式。层代码只看见逻辑形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Storage {
    /// 已经是计算用的数据类型。
    Dense,
    /// 按组存放。权重与缩放分开保存，求值时反量化到张量自己的数据类型。
    Grouped {
        /// 组大小。0.6B 使用 64。
        group_size: usize,
        /// 每组元素的位数。文本塔是 4，音频塔是 8。
        bits: usize,
    },
}

/// 半开区间 `[start, end)`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisSlice {
    /// 起点，含。
    pub start: usize,
    /// 终点，不含。
    pub end: usize,
}

impl AxisSlice {
    /// 构造一个半开区间。
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// 覆盖整轴。
    pub const fn full(len: usize) -> Self {
        Self { start: 0, end: len }
    }
}

/// 某一轴前后补零的宽度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadWidth {
    /// 该轴前面补的个数。
    pub before: usize,
    /// 该轴后面补的个数。
    pub after: usize,
}

impl PadWidth {
    /// 前后都补。
    pub const fn new(before: usize, after: usize) -> Self {
        Self { before, after }
    }

    /// 不补。
    pub const fn none() -> Self {
        Self {
            before: 0,
            after: 0,
        }
    }
}

/// 线性层。权重逻辑形状是 `(output, input)`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Linear<T> {
    input: usize,
    output: usize,
    weight: T,
    bias: Option<T>,
}

impl<T: TensorMeta> Linear<T> {
    /// 由权重和可选偏置构造。分组权重在求值时反量化。
    pub fn new(weight: T, bias: Option<T>) -> Result<Self, TensorError> {
        let (input, output) = check_linear(&weight, bias.as_ref())?;
        Ok(Self {
            input,
            output,
            weight,
            bias,
        })
    }
}

impl<T> Linear<T> {
    /// 输入维。
    pub fn input(&self) -> usize {
        self.input
    }

    /// 输出维。
    pub fn output(&self) -> usize {
        self.output
    }

    /// 权重。
    pub fn weight(&self) -> &T {
        &self.weight
    }

    /// 偏置。
    pub fn bias(&self) -> Option<&T> {
        self.bias.as_ref()
    }

    /// 是否带偏置。
    pub fn has_bias(&self) -> bool {
        self.bias.is_some()
    }
}

/// 二维卷积。输入是 NHWC，权重形状是 `(out_ch, kernel, kernel, in_ch)`。
///
/// 本图里的卷积核是 3，步长是 2，填充是 1。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conv2d<T> {
    in_ch: usize,
    out_ch: usize,
    kernel: usize,
    stride: usize,
    pad: usize,
    weight: T,
    bias: Option<T>,
}

impl<T: TensorMeta> Conv2d<T> {
    /// 核、步长和填充固定为 3、2、1。
    pub fn new(weight: T, bias: Option<T>) -> Result<Self, TensorError> {
        let (in_ch, out_ch) = check_conv_weight(&weight, bias.as_ref())?;
        Ok(Self {
            in_ch,
            out_ch,
            kernel: CONV_KERNEL,
            stride: CONV_STRIDE,
            pad: CONV_PAD,
            weight,
            bias,
        })
    }
}

impl<T> Conv2d<T> {
    /// 输入通道。
    pub fn in_ch(&self) -> usize {
        self.in_ch
    }

    /// 输出通道。
    pub fn out_ch(&self) -> usize {
        self.out_ch
    }

    /// 核大小。
    pub fn kernel(&self) -> usize {
        self.kernel
    }

    /// 步长。
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// 填充。
    pub fn pad(&self) -> usize {
        self.pad
    }

    /// 权重。
    pub fn weight(&self) -> &T {
        &self.weight
    }

    /// 偏置。
    pub fn bias(&self) -> Option<&T> {
        self.bias.as_ref()
    }
}

/// 层归一化，作用在最后一维。
#[derive(Clone, Debug, PartialEq)]
pub struct LayerNorm<T> {
    dim: usize,
    eps: f32,
    weight: T,
    bias: Option<T>,
}

impl<T: TensorMeta> LayerNorm<T> {
    /// `eps` 必须是有限正数。
    pub fn new(weight: T, bias: Option<T>, eps: f32) -> Result<Self, TensorError> {
        let dim = check_norm("layer_norm", &weight, bias.as_ref(), eps)?;
        Ok(Self {
            dim,
            eps,
            weight,
            bias,
        })
    }
}

impl<T> LayerNorm<T> {
    /// 归一化宽度。
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// 稳定项。
    pub fn eps(&self) -> f32 {
        self.eps
    }

    /// 缩放。
    pub fn weight(&self) -> &T {
        &self.weight
    }

    /// 偏置。
    pub fn bias(&self) -> Option<&T> {
        self.bias.as_ref()
    }
}

/// 均方根归一化，作用在最后一维。文本塔不带偏置。
#[derive(Clone, Debug, PartialEq)]
pub struct RmsNorm<T> {
    dim: usize,
    eps: f32,
    weight: T,
}

impl<T: TensorMeta> RmsNorm<T> {
    /// `eps` 必须是有限正数。
    pub fn new(weight: T, eps: f32) -> Result<Self, TensorError> {
        let dim = check_norm("rms_norm", &weight, None, eps)?;
        Ok(Self { dim, eps, weight })
    }
}

impl<T> RmsNorm<T> {
    /// 归一化宽度。
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// 稳定项。
    pub fn eps(&self) -> f32 {
        self.eps
    }

    /// 缩放。
    pub fn weight(&self) -> &T {
        &self.weight
    }
}

/// 解码器的一层键值缓存。序列轴是形状 `(B, H, T, D)` 的第 2 轴。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvCache<T> {
    key: Option<T>,
    value: Option<T>,
}

impl<T> Default for KvCache<T> {
    fn default() -> Self {
        Self {
            key: None,
            value: None,
        }
    }
}

impl<T> KvCache<T> {
    /// 空缓存。
    pub fn new() -> Self {
        Self::default()
    }

    /// 还没有写入。
    pub fn is_empty(&self) -> bool {
        self.key.is_none()
    }

    /// 已写入的键。
    pub fn key(&self) -> Option<&T> {
        self.key.as_ref()
    }

    /// 已写入的值。
    pub fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// 替换这一层的键和值。调用方先沿时间轴拼接。
    pub fn store(&mut self, key: T, value: T) {
        self.key = Some(key);
        self.value = Some(value);
    }

    pub(crate) fn pair_mut(&mut self) -> (&mut Option<T>, &mut Option<T>) {
        (&mut self.key, &mut self.value)
    }
}

/// 分组量化支持的位数。MLX 的反量化只认这几个。
pub(crate) const fn supported_group_bits(bits: usize) -> bool {
    matches!(bits, 2 | 3 | 4 | 5 | 6 | 8)
}

/// 卷积核固定值。
pub const CONV_KERNEL: usize = 3;
/// 卷积步长固定值。
pub const CONV_STRIDE: usize = 2;
/// 卷积填充固定值。
pub const CONV_PAD: usize = 1;

/// 图可以调用的张量运算。
///
/// 创建：零、范围、正态、把 `i16` PCM 提升到 `f32`。
/// 形状：reshape、transpose、slice、pad、concatenate、where、cumsum、gather。
/// 元素：加、乘、平方根倒数、指数、正弦、余弦、clip、GELU、SiLU。
/// 线性、卷积、层归一化、均方根归一化、带可选加法 mask 的缩放点积注意力。
/// 解码器可以写入并读取 KV 缓存。
/// 归约：沿轴求和、最大、最小。
pub trait TensorOps {
    /// 后端自己的张量句柄。
    type Tensor: TensorMeta + Clone;
    /// 后端自己的错误。
    type Error: std::error::Error + Send + Sync + 'static;

    /// 全零。
    fn zeros(&mut self, shape: &[usize], dtype: DType) -> Result<Self::Tensor, Self::Error>;

    /// 半开整数范围，再按 `dtype` 存放。
    fn arange(
        &mut self,
        start: i64,
        stop: i64,
        step: i64,
        dtype: DType,
    ) -> Result<Self::Tensor, Self::Error>;

    /// 正态随机数。假后端只保留形状。
    fn normal(
        &mut self,
        shape: &[usize],
        mean: f32,
        std: f32,
        dtype: DType,
    ) -> Result<Self::Tensor, Self::Error>;

    /// 把 `i16` PCM 提升成 `f32`，形状是 `[样本数]`。
    fn pcm_to_f32(&mut self, pcm: &[i16]) -> Result<Self::Tensor, Self::Error>;

    /// 改变形状。最多一个维度可以写成 `-1`。
    fn reshape(
        &mut self,
        input: &Self::Tensor,
        shape: &[isize],
    ) -> Result<Self::Tensor, Self::Error>;

    /// 按 `axes` 重排维度。
    fn transpose(
        &mut self,
        input: &Self::Tensor,
        axes: &[usize],
    ) -> Result<Self::Tensor, Self::Error>;

    /// 每一轴一个半开区间。
    fn slice(
        &mut self,
        input: &Self::Tensor,
        ranges: &[AxisSlice],
    ) -> Result<Self::Tensor, Self::Error>;

    /// 每一轴前后补零。
    fn pad(&mut self, input: &Self::Tensor, pads: &[PadWidth])
        -> Result<Self::Tensor, Self::Error>;

    /// 沿一轴拼接。
    fn concatenate(
        &mut self,
        inputs: &[&Self::Tensor],
        axis: usize,
    ) -> Result<Self::Tensor, Self::Error>;

    /// 按条件选择。条件是布尔张量。
    fn where_op(
        &mut self,
        cond: &Self::Tensor,
        when_true: &Self::Tensor,
        when_false: &Self::Tensor,
    ) -> Result<Self::Tensor, Self::Error>;

    /// 沿轴累积和，形状不变。
    fn cumsum(&mut self, input: &Self::Tensor, axis: usize) -> Result<Self::Tensor, Self::Error>;

    /// 沿轴收集。该轴被下标的形状替换。
    fn gather(
        &mut self,
        input: &Self::Tensor,
        index: &Self::Tensor,
        axis: usize,
    ) -> Result<Self::Tensor, Self::Error>;

    /// 逐元素加，形状按广播对齐。
    fn add(&mut self, lhs: &Self::Tensor, rhs: &Self::Tensor) -> Result<Self::Tensor, Self::Error>;

    /// 逐元素乘，形状按广播对齐。
    fn mul(&mut self, lhs: &Self::Tensor, rhs: &Self::Tensor) -> Result<Self::Tensor, Self::Error>;

    /// 平方根倒数。
    fn rsqrt(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error>;

    /// 指数。
    fn exp(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error>;

    /// 正弦。
    fn sin(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error>;

    /// 余弦。
    fn cos(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error>;

    /// 裁剪到闭区间。
    fn clip(
        &mut self,
        input: &Self::Tensor,
        min: f32,
        max: f32,
    ) -> Result<Self::Tensor, Self::Error>;

    /// GELU。
    fn gelu(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error>;

    /// SiLU。
    fn silu(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error>;

    /// 线性层。分组权重由后端在求值时反量化。
    fn linear(
        &mut self,
        input: &Self::Tensor,
        layer: &Linear<Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error>;

    /// NHWC 卷积。
    fn conv2d(
        &mut self,
        input: &Self::Tensor,
        layer: &Conv2d<Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error>;

    /// 层归一化。
    fn layer_norm(
        &mut self,
        input: &Self::Tensor,
        norm: &LayerNorm<Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error>;

    /// 均方根归一化。
    fn rms_norm(
        &mut self,
        input: &Self::Tensor,
        norm: &RmsNorm<Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error>;

    /// 缩放点积注意力。`mask` 是可广播的加法 mask。
    ///
    /// 查询、键、值的形状是 `(B, H, L, D)`。键值头可以少于查询头，但必须整除。
    fn attention(
        &mut self,
        query: &Self::Tensor,
        key: &Self::Tensor,
        value: &Self::Tensor,
        mask: Option<&Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error>;

    /// 把键和值沿时间轴接到缓存后面。
    fn kv_write(
        &mut self,
        cache: &mut KvCache<Self::Tensor>,
        key: &Self::Tensor,
        value: &Self::Tensor,
    ) -> Result<(), Self::Error>;

    /// 读出当前缓存。空缓存是错误。
    fn kv_read(
        &mut self,
        cache: &KvCache<Self::Tensor>,
    ) -> Result<(Self::Tensor, Self::Tensor), Self::Error>;

    /// 沿轴求和。
    fn sum(
        &mut self,
        input: &Self::Tensor,
        axis: usize,
        keep_dim: bool,
    ) -> Result<Self::Tensor, Self::Error>;

    /// 沿轴取最大。
    fn max(
        &mut self,
        input: &Self::Tensor,
        axis: usize,
        keep_dim: bool,
    ) -> Result<Self::Tensor, Self::Error>;

    /// 沿轴取最小。
    fn min(
        &mut self,
        input: &Self::Tensor,
        axis: usize,
        keep_dim: bool,
    ) -> Result<Self::Tensor, Self::Error>;
}

pub(crate) fn check_linear<T: TensorMeta>(
    weight: &T,
    bias: Option<&T>,
) -> Result<(usize, usize), TensorError> {
    require_float("linear", weight.dtype())?;
    if weight.rank() != 2 {
        return Err(TensorError::new(
            "linear",
            format!(
                "权重必须是二维 (output, input)，实际 {}",
                format_dims(weight.dims())
            ),
        ));
    }
    let output = weight.dims()[0];
    let input = weight.dims()[1];
    if let Storage::Grouped { group_size, bits } = weight.storage() {
        if group_size == 0 || !input.is_multiple_of(group_size) {
            return Err(TensorError::new(
                "linear",
                format!("输入维 {input} 不能按组大小 {group_size} 划分"),
            ));
        }
        if !crate::ops::supported_group_bits(bits) {
            return Err(TensorError::new(
                "linear",
                format!("分组位数 {bits} 不支持"),
            ));
        }
    }
    if let Some(bias) = bias {
        require_same_dtype("linear", weight.dtype(), bias.dtype())?;
        if bias.dims() != [output] {
            return Err(TensorError::new(
                "linear",
                format!("偏置形状应为 [{output}]，实际 {}", format_dims(bias.dims())),
            ));
        }
    }
    Ok((input, output))
}

pub(crate) fn check_conv_weight<T: TensorMeta>(
    weight: &T,
    bias: Option<&T>,
) -> Result<(usize, usize), TensorError> {
    require_float("conv2d", weight.dtype())?;
    if weight.rank() != 4 {
        return Err(TensorError::new(
            "conv2d",
            format!(
                "权重必须是 (out_ch, kernel, kernel, in_ch)，实际 {}",
                format_dims(weight.dims())
            ),
        ));
    }
    let out_ch = weight.dims()[0];
    let kernel_h = weight.dims()[1];
    let kernel_w = weight.dims()[2];
    let in_ch = weight.dims()[3];
    if kernel_h != CONV_KERNEL || kernel_w != CONV_KERNEL {
        return Err(TensorError::new(
            "conv2d",
            format!("核必须是 {CONV_KERNEL}，实际 {kernel_h}×{kernel_w}"),
        ));
    }
    if let Some(bias) = bias {
        require_same_dtype("conv2d", weight.dtype(), bias.dtype())?;
        if bias.dims() != [out_ch] {
            return Err(TensorError::new(
                "conv2d",
                format!("偏置形状应为 [{out_ch}]，实际 {}", format_dims(bias.dims())),
            ));
        }
    }
    Ok((in_ch, out_ch))
}

pub(crate) fn check_conv_input(
    dims: &[usize],
    layer_in: usize,
) -> Result<(usize, usize), TensorError> {
    if dims.len() != 4 {
        return Err(TensorError::new(
            "conv2d",
            format!("输入必须是 NHWC 四维，实际 {}", format_dims(dims)),
        ));
    }
    if dims[3] != layer_in {
        return Err(TensorError::new(
            "conv2d",
            format!("输入通道 {} 与权重输入通道 {layer_in} 不一致", dims[3]),
        ));
    }
    let out_h = conv2d_spatial_out(dims[1], CONV_KERNEL, CONV_STRIDE, CONV_PAD)?;
    let out_w = conv2d_spatial_out(dims[2], CONV_KERNEL, CONV_STRIDE, CONV_PAD)?;
    Ok((out_h, out_w))
}

fn check_norm<T: TensorMeta>(
    op: &'static str,
    weight: &T,
    bias: Option<&T>,
    eps: f32,
) -> Result<usize, TensorError> {
    require_float(op, weight.dtype())?;
    if !(eps.is_finite() && eps > 0.0) {
        return Err(TensorError::new(op, "eps 必须是有限正数"));
    }
    if weight.rank() != 1 {
        return Err(TensorError::new(
            op,
            format!("缩放必须是一维，实际 {}", format_dims(weight.dims())),
        ));
    }
    let dim = weight.dims()[0];
    if let Some(bias) = bias {
        require_same_dtype(op, weight.dtype(), bias.dtype())?;
        if bias.dims() != [dim] {
            return Err(TensorError::new(
                op,
                format!("偏置形状应为 [{dim}]，实际 {}", format_dims(bias.dims())),
            ));
        }
    }
    Ok(dim)
}

pub(crate) fn require_float(op: &'static str, dtype: DType) -> Result<(), TensorError> {
    if dtype.is_float() {
        Ok(())
    } else {
        Err(TensorError::new(op, format!("需要浮点，实际是 {dtype}")))
    }
}

pub(crate) fn require_numeric(op: &'static str, dtype: DType) -> Result<(), TensorError> {
    if dtype.is_numeric() {
        Ok(())
    } else {
        Err(TensorError::new(
            op,
            format!("需要数值类型，实际是 {dtype}"),
        ))
    }
}

pub(crate) fn require_same_dtype(
    op: &'static str,
    lhs: DType,
    rhs: DType,
) -> Result<(), TensorError> {
    if lhs == rhs {
        Ok(())
    } else {
        Err(TensorError::new(
            op,
            format!("数据类型不一致：{lhs} 与 {rhs}"),
        ))
    }
}

pub(crate) fn check_last_dim(
    op: &'static str,
    dims: &[usize],
    expected: usize,
) -> Result<(), TensorError> {
    match dims.last() {
        Some(&last) if last == expected => Ok(()),
        Some(&last) => Err(TensorError::new(
            op,
            format!("最后一维是 {last}，应为 {expected}"),
        )),
        None => Err(TensorError::new(op, "输入至少要有一维")),
    }
}

pub(crate) fn reduced_shape(
    op: &'static str,
    dims: &[usize],
    axis: usize,
    keep_dim: bool,
) -> Result<Vec<usize>, TensorError> {
    let axis = axis_index(op, axis, dims.len())?;
    let mut out = dims.to_vec();
    if keep_dim {
        out[axis] = 1;
    } else {
        out.remove(axis);
    }
    Ok(out)
}
