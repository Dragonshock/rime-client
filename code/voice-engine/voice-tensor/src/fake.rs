use crate::dtype::DType;
use crate::error::TensorError;
use crate::ops::{
    check_conv_input, check_last_dim, check_linear, reduced_shape, require_float, require_numeric,
    require_same_dtype, AxisSlice, Conv2d, KvCache, LayerNorm, Linear, PadWidth, RmsNorm, Storage,
    TensorMeta, TensorOps, CONV_KERNEL, CONV_PAD, CONV_STRIDE,
};
use crate::shape::{
    axis_index, broadcast_all_for, broadcast_shape_for, format_dims, infer_reshape, replace_last,
    Shape,
};

/// 只记录形状的张量。不保存数值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakeTensor {
    shape: Shape,
    dtype: DType,
    storage: Storage,
}

impl FakeTensor {
    /// 稠密句柄，不记入调用记录。
    pub fn dense(shape: &[usize], dtype: DType) -> Self {
        Self {
            shape: Shape::new(shape.to_vec()),
            dtype,
            storage: Storage::Dense,
        }
    }

    /// 分组存放的权重句柄。形状是反量化后的逻辑形状，最后一维必须能被组大小整除。
    pub fn grouped(shape: &[usize], dtype: DType, group_size: usize) -> Result<Self, TensorError> {
        Self::grouped_with_bits(shape, dtype, group_size, crate::config::TEXT_GROUP_BITS)
    }

    /// 指定每组位数的分组权重句柄。音频塔是 8 位，文本塔是 4 位。
    pub fn grouped_with_bits(
        shape: &[usize],
        dtype: DType,
        group_size: usize,
        bits: usize,
    ) -> Result<Self, TensorError> {
        require_float("grouped", dtype)?;
        let last = shape
            .last()
            .copied()
            .ok_or_else(|| TensorError::new("grouped", "分组权重至少要有一维"))?;
        if group_size == 0 || !last.is_multiple_of(group_size) {
            return Err(TensorError::new(
                "grouped",
                format!("最后一维 {last} 不能按组大小 {group_size} 划分"),
            ));
        }
        if !crate::ops::supported_group_bits(bits) {
            return Err(TensorError::new(
                "grouped",
                format!("分组位数 {bits} 不支持"),
            ));
        }
        Ok(Self {
            shape: Shape::new(shape.to_vec()),
            dtype,
            storage: Storage::Grouped { group_size, bits },
        })
    }
}

impl TensorMeta for FakeTensor {
    fn dims(&self) -> &[usize] {
        self.shape.dims()
    }

    fn dtype(&self) -> DType {
        self.dtype
    }

    fn storage(&self) -> Storage {
        self.storage
    }
}

/// 假后端记下来的一次调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordedCall {
    /// 全零。
    Zeros {
        /// 形状。
        shape: Shape,
        /// 数据类型。
        dtype: DType,
    },
    /// 范围。
    Arange {
        /// 起点。
        start: i64,
        /// 终点，不含。
        stop: i64,
        /// 步长。
        step: i64,
        /// 数据类型。
        dtype: DType,
        /// 形状。
        shape: Shape,
    },
    /// 正态。
    Normal {
        /// 形状。
        shape: Shape,
        /// 数据类型。
        dtype: DType,
    },
    /// PCM 提升。
    PcmToF32 {
        /// 样本数。
        samples: usize,
    },
    /// 改变形状。
    Reshape {
        /// 输入形状。
        input: Shape,
        /// 输出形状。
        output: Shape,
    },
    /// 转置。
    Transpose {
        /// 输入形状。
        input: Shape,
        /// 轴顺序。
        axes: Vec<usize>,
        /// 输出形状。
        output: Shape,
    },
    /// 切片。
    Slice {
        /// 输入形状。
        input: Shape,
        /// 输出形状。
        output: Shape,
    },
    /// 补零。
    Pad {
        /// 输入形状。
        input: Shape,
        /// 输出形状。
        output: Shape,
    },
    /// 拼接。
    Concatenate {
        /// 各段形状。
        inputs: Vec<Shape>,
        /// 拼接轴。
        axis: usize,
        /// 输出形状。
        output: Shape,
    },
    /// 条件选择。
    Where {
        /// 输出形状。
        output: Shape,
    },
    /// 累积和。
    Cumsum {
        /// 输入形状。
        input: Shape,
        /// 轴。
        axis: usize,
    },
    /// 收集。
    Gather {
        /// 输入形状。
        input: Shape,
        /// 下标形状。
        index: Shape,
        /// 轴。
        axis: usize,
        /// 输出形状。
        output: Shape,
    },
    /// 加。
    Add {
        /// 左操作数。
        lhs: Shape,
        /// 右操作数。
        rhs: Shape,
        /// 输出形状。
        output: Shape,
    },
    /// 乘。
    Mul {
        /// 左操作数。
        lhs: Shape,
        /// 右操作数。
        rhs: Shape,
        /// 输出形状。
        output: Shape,
    },
    /// 一元浮点运算。
    Unary {
        /// 运算名。
        op: UnaryOp,
        /// 输入形状。
        input: Shape,
    },
    /// 裁剪。
    Clip {
        /// 输入形状。
        input: Shape,
    },
    /// 线性层。
    Linear {
        /// 输入形状。
        input: Shape,
        /// 权重逻辑形状。
        weight: Shape,
        /// 是否带偏置。
        bias: bool,
        /// 权重是否分组存放。
        grouped: bool,
        /// 输出形状。
        output: Shape,
    },
    /// 卷积。
    Conv2d {
        /// 输入形状。
        input: Shape,
        /// 输入通道。
        in_ch: usize,
        /// 输出通道。
        out_ch: usize,
        /// 核大小。
        kernel: usize,
        /// 步长。
        stride: usize,
        /// 填充。
        pad: usize,
        /// 输出形状。
        output: Shape,
    },
    /// 层归一化。
    LayerNorm {
        /// 输入形状。
        input: Shape,
        /// 宽度。
        dim: usize,
    },
    /// 均方根归一化。
    RmsNorm {
        /// 输入形状。
        input: Shape,
        /// 宽度。
        dim: usize,
    },
    /// 缩放点积注意力。
    Attention {
        /// 查询形状。
        query: Shape,
        /// 键形状。
        key: Shape,
        /// 值形状。
        value: Shape,
        /// 加法 mask 的形状。
        mask: Option<Shape>,
        /// 输出形状。
        output: Shape,
    },
    /// 写入 KV 缓存。
    KvWrite {
        /// 本次键的形状。
        key: Shape,
        /// 本次值的形状。
        value: Shape,
        /// 写入后的时间长度。
        cached_time: usize,
    },
    /// 读出 KV 缓存。
    KvRead {
        /// 键形状。
        key: Shape,
        /// 值形状。
        value: Shape,
    },
    /// 归约。
    Reduce {
        /// 归约种类。
        op: ReduceOp,
        /// 输入形状。
        input: Shape,
        /// 轴。
        axis: usize,
        /// 输出形状。
        output: Shape,
    },
}

/// 形状不变的一元运算。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// 平方根倒数。
    Rsqrt,
    /// 指数。
    Exp,
    /// 正弦。
    Sin,
    /// 余弦。
    Cos,
    /// GELU。
    Gelu,
    /// SiLU。
    Silu,
}

/// 沿轴归约。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReduceOp {
    /// 求和。
    Sum,
    /// 最大。
    Max,
    /// 最小。
    Min,
}

/// 一次线性层调用的形状。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinearShape {
    /// 输入。
    pub input: Shape,
    /// 权重。
    pub weight: Shape,
    /// 是否带偏置。
    pub bias: bool,
    /// 是否分组存放。
    pub grouped: bool,
    /// 输出。
    pub output: Shape,
}

/// 一次卷积调用的形状。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvShape {
    /// 输入。
    pub input: Shape,
    /// 输入通道。
    pub in_ch: usize,
    /// 输出通道。
    pub out_ch: usize,
    /// 核大小。
    pub kernel: usize,
    /// 步长。
    pub stride: usize,
    /// 填充。
    pub pad: usize,
    /// 输出。
    pub output: Shape,
}

/// 一次注意力调用的形状。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttentionShape {
    /// 查询。
    pub query: Shape,
    /// 键。
    pub key: Shape,
    /// 值。
    pub value: Shape,
    /// 加法 mask。
    pub mask: Option<Shape>,
    /// 输出。
    pub output: Shape,
}

/// 记录调用形状的假后端。不启动 GPU，也不计算数值。
#[derive(Debug, Default)]
pub struct FakeBackend {
    calls: Vec<RecordedCall>,
}

impl FakeBackend {
    /// 空的调用记录。
    pub fn new() -> Self {
        Self::default()
    }

    /// 全部调用，按发生顺序排列。失败的调用不在其中。
    pub fn calls(&self) -> &[RecordedCall] {
        &self.calls
    }

    /// 清空调用记录。
    pub fn clear(&mut self) {
        self.calls.clear();
    }

    /// 线性层的形状记录。
    pub fn linear_shapes(&self) -> Vec<LinearShape> {
        self.calls
            .iter()
            .filter_map(|call| match call {
                RecordedCall::Linear {
                    input,
                    weight,
                    bias,
                    grouped,
                    output,
                } => Some(LinearShape {
                    input: input.clone(),
                    weight: weight.clone(),
                    bias: *bias,
                    grouped: *grouped,
                    output: output.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// 卷积的形状记录。
    pub fn conv_shapes(&self) -> Vec<ConvShape> {
        self.calls
            .iter()
            .filter_map(|call| match call {
                RecordedCall::Conv2d {
                    input,
                    in_ch,
                    out_ch,
                    kernel,
                    stride,
                    pad,
                    output,
                } => Some(ConvShape {
                    input: input.clone(),
                    in_ch: *in_ch,
                    out_ch: *out_ch,
                    kernel: *kernel,
                    stride: *stride,
                    pad: *pad,
                    output: output.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// 注意力的形状记录。
    pub fn attention_shapes(&self) -> Vec<AttentionShape> {
        self.calls
            .iter()
            .filter_map(|call| match call {
                RecordedCall::Attention {
                    query,
                    key,
                    value,
                    mask,
                    output,
                } => Some(AttentionShape {
                    query: query.clone(),
                    key: key.clone(),
                    value: value.clone(),
                    mask: mask.clone(),
                    output: output.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    fn push(&mut self, call: RecordedCall) {
        self.calls.push(call);
    }

    fn dense_out(&self, shape: Vec<usize>, dtype: DType) -> FakeTensor {
        FakeTensor::dense(&shape, dtype)
    }

    fn unary(&mut self, op: UnaryOp, input: &FakeTensor) -> Result<FakeTensor, TensorError> {
        require_float(op.name(), input.dtype)?;
        let shape = input.shape.clone();
        self.push(RecordedCall::Unary { op, input: shape });
        Ok(input.clone())
    }
}

impl TensorOps for FakeBackend {
    type Tensor = FakeTensor;
    type Error = TensorError;

    fn zeros(&mut self, shape: &[usize], dtype: DType) -> Result<Self::Tensor, Self::Error> {
        let tensor = FakeTensor::dense(shape, dtype);
        self.push(RecordedCall::Zeros {
            shape: tensor.shape.clone(),
            dtype,
        });
        Ok(tensor)
    }

    fn arange(
        &mut self,
        start: i64,
        stop: i64,
        step: i64,
        dtype: DType,
    ) -> Result<Self::Tensor, Self::Error> {
        require_numeric("arange", dtype)?;
        let len = arange_len(start, stop, step)?;
        let tensor = FakeTensor::dense(&[len], dtype);
        self.push(RecordedCall::Arange {
            start,
            stop,
            step,
            dtype,
            shape: tensor.shape.clone(),
        });
        Ok(tensor)
    }

    fn normal(
        &mut self,
        shape: &[usize],
        mean: f32,
        std: f32,
        dtype: DType,
    ) -> Result<Self::Tensor, Self::Error> {
        require_float("normal", dtype)?;
        if !mean.is_finite() || !std.is_finite() || std < 0.0 {
            return Err(TensorError::new(
                "normal",
                "均值必须有限，标准差必须是有限非负数",
            ));
        }
        let tensor = FakeTensor::dense(shape, dtype);
        self.push(RecordedCall::Normal {
            shape: tensor.shape.clone(),
            dtype,
        });
        Ok(tensor)
    }

    fn pcm_to_f32(&mut self, pcm: &[i16]) -> Result<Self::Tensor, Self::Error> {
        let tensor = FakeTensor::dense(&[pcm.len()], DType::F32);
        self.push(RecordedCall::PcmToF32 { samples: pcm.len() });
        Ok(tensor)
    }

    fn reshape(
        &mut self,
        input: &Self::Tensor,
        shape: &[isize],
    ) -> Result<Self::Tensor, Self::Error> {
        let numel = input.numel()?;
        let dims = infer_reshape("reshape", numel, shape)?;
        let output = self.dense_out(dims, input.dtype);
        self.push(RecordedCall::Reshape {
            input: input.shape.clone(),
            output: output.shape.clone(),
        });
        Ok(output)
    }

    fn transpose(
        &mut self,
        input: &Self::Tensor,
        axes: &[usize],
    ) -> Result<Self::Tensor, Self::Error> {
        if axes.len() != input.rank() {
            return Err(TensorError::new(
                "transpose",
                format!("轴个数 {} 与秩 {} 不一致", axes.len(), input.rank()),
            ));
        }
        let mut seen = vec![false; input.rank()];
        let mut dims = vec![0; input.rank()];
        for (out_axis, &src) in axes.iter().enumerate() {
            let src = axis_index("transpose", src, input.rank())?;
            if seen[src] {
                return Err(TensorError::new("transpose", format!("轴 {src} 重复")));
            }
            seen[src] = true;
            dims[out_axis] = input.dims()[src];
        }
        let output = self.dense_out(dims, input.dtype);
        self.push(RecordedCall::Transpose {
            input: input.shape.clone(),
            axes: axes.to_vec(),
            output: output.shape.clone(),
        });
        Ok(output)
    }

    fn slice(
        &mut self,
        input: &Self::Tensor,
        ranges: &[AxisSlice],
    ) -> Result<Self::Tensor, Self::Error> {
        if ranges.len() != input.rank() {
            return Err(TensorError::new(
                "slice",
                format!("区间个数 {} 与秩 {} 不一致", ranges.len(), input.rank()),
            ));
        }
        let mut dims = Vec::with_capacity(input.rank());
        for (axis, range) in ranges.iter().enumerate() {
            let len = input.dims()[axis];
            if range.start > range.end || range.end > len {
                return Err(TensorError::new(
                    "slice",
                    format!(
                        "轴 {axis} 的区间 [{}, {}) 超出长度 {len}",
                        range.start, range.end
                    ),
                ));
            }
            dims.push(range.end - range.start);
        }
        let output = self.dense_out(dims, input.dtype);
        self.push(RecordedCall::Slice {
            input: input.shape.clone(),
            output: output.shape.clone(),
        });
        Ok(output)
    }

    fn pad(
        &mut self,
        input: &Self::Tensor,
        pads: &[PadWidth],
    ) -> Result<Self::Tensor, Self::Error> {
        if pads.len() != input.rank() {
            return Err(TensorError::new(
                "pad",
                format!("填充个数 {} 与秩 {} 不一致", pads.len(), input.rank()),
            ));
        }
        let mut dims = Vec::with_capacity(input.rank());
        for (axis, (len, pad)) in input.dims().iter().zip(pads).enumerate() {
            let next = len
                .checked_add(pad.before)
                .and_then(|value| value.checked_add(pad.after))
                .ok_or_else(|| TensorError::new("pad", format!("轴 {axis} 填充后溢出")))?;
            dims.push(next);
        }
        let output = self.dense_out(dims, input.dtype);
        self.push(RecordedCall::Pad {
            input: input.shape.clone(),
            output: output.shape.clone(),
        });
        Ok(output)
    }

    fn concatenate(
        &mut self,
        inputs: &[&Self::Tensor],
        axis: usize,
    ) -> Result<Self::Tensor, Self::Error> {
        let first = inputs
            .first()
            .copied()
            .ok_or_else(|| TensorError::new("concatenate", "至少要有一段"))?;
        let axis = axis_index("concatenate", axis, first.rank())?;
        let mut dims = first.dims().to_vec();
        dims[axis] = 0;
        let mut recorded = Vec::with_capacity(inputs.len());
        for input in inputs {
            require_same_dtype("concatenate", first.dtype, input.dtype())?;
            if input.rank() != first.rank() {
                return Err(TensorError::new("concatenate", "各段的秩必须相同"));
            }
            for (index, (&got, &expect)) in input.dims().iter().zip(first.dims()).enumerate() {
                if index != axis && got != expect {
                    return Err(TensorError::new(
                        "concatenate",
                        format!(
                            "轴 {index} 的长度 {got} 与 {} 不一致",
                            format_dims(first.dims())
                        ),
                    ));
                }
            }
            dims[axis] = dims[axis]
                .checked_add(input.dims()[axis])
                .ok_or_else(|| TensorError::new("concatenate", "拼接长度溢出"))?;
            recorded.push(input.shape.clone());
        }
        let output = self.dense_out(dims, first.dtype);
        self.push(RecordedCall::Concatenate {
            inputs: recorded,
            axis,
            output: output.shape.clone(),
        });
        Ok(output)
    }

    fn where_op(
        &mut self,
        cond: &Self::Tensor,
        when_true: &Self::Tensor,
        when_false: &Self::Tensor,
    ) -> Result<Self::Tensor, Self::Error> {
        if cond.dtype != DType::Bool {
            return Err(TensorError::new(
                "where",
                format!("条件必须是 bool，实际是 {}", cond.dtype),
            ));
        }
        require_same_dtype("where", when_true.dtype, when_false.dtype())?;
        let shape =
            broadcast_all_for("where", &[cond.dims(), when_true.dims(), when_false.dims()])?;
        let output = self.dense_out(shape.dims().to_vec(), when_true.dtype);
        self.push(RecordedCall::Where {
            output: output.shape.clone(),
        });
        Ok(output)
    }

    fn cumsum(&mut self, input: &Self::Tensor, axis: usize) -> Result<Self::Tensor, Self::Error> {
        require_numeric("cumsum", input.dtype)?;
        let axis = axis_index("cumsum", axis, input.rank())?;
        self.push(RecordedCall::Cumsum {
            input: input.shape.clone(),
            axis,
        });
        Ok(input.clone())
    }

    fn gather(
        &mut self,
        input: &Self::Tensor,
        index: &Self::Tensor,
        axis: usize,
    ) -> Result<Self::Tensor, Self::Error> {
        if index.dtype != DType::I32 {
            return Err(TensorError::new(
                "gather",
                format!("下标必须是 i32，实际是 {}", index.dtype),
            ));
        }
        let axis = axis_index("gather", axis, input.rank())?;
        let mut dims = Vec::with_capacity(input.rank() + index.rank() - 1);
        dims.extend_from_slice(&input.dims()[..axis]);
        dims.extend_from_slice(index.dims());
        dims.extend_from_slice(&input.dims()[axis + 1..]);
        let output = self.dense_out(dims, input.dtype);
        self.push(RecordedCall::Gather {
            input: input.shape.clone(),
            index: index.shape.clone(),
            axis,
            output: output.shape.clone(),
        });
        Ok(output)
    }

    fn add(&mut self, lhs: &Self::Tensor, rhs: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.binary("add", lhs, rhs, |lhs, rhs, output| RecordedCall::Add {
            lhs,
            rhs,
            output,
        })
    }

    fn mul(&mut self, lhs: &Self::Tensor, rhs: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.binary("mul", lhs, rhs, |lhs, rhs, output| RecordedCall::Mul {
            lhs,
            rhs,
            output,
        })
    }

    fn rsqrt(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.unary(UnaryOp::Rsqrt, input)
    }

    fn exp(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.unary(UnaryOp::Exp, input)
    }

    fn sin(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.unary(UnaryOp::Sin, input)
    }

    fn cos(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.unary(UnaryOp::Cos, input)
    }

    fn clip(
        &mut self,
        input: &Self::Tensor,
        min: f32,
        max: f32,
    ) -> Result<Self::Tensor, Self::Error> {
        require_float("clip", input.dtype)?;
        if !min.is_finite() || !max.is_finite() || min > max {
            return Err(TensorError::new(
                "clip",
                "下界和上界必须有限，且下界不能大于上界",
            ));
        }
        self.push(RecordedCall::Clip {
            input: input.shape.clone(),
        });
        Ok(input.clone())
    }

    fn gelu(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.unary(UnaryOp::Gelu, input)
    }

    fn silu(&mut self, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error> {
        self.unary(UnaryOp::Silu, input)
    }

    fn linear(
        &mut self,
        input: &Self::Tensor,
        layer: &Linear<Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error> {
        let (in_features, out_features) = check_linear(layer.weight(), layer.bias())?;
        require_same_dtype("linear", input.dtype, layer.weight().dtype())?;
        if input.dims().last().copied() != Some(in_features) {
            return Err(TensorError::new(
                "linear",
                format!(
                    "输入末维是 {}，权重的输入维是 {in_features}",
                    input
                        .dims()
                        .last()
                        .map(|dim| dim.to_string())
                        .unwrap_or_else(|| "缺失".to_string())
                ),
            ));
        }
        let dims = replace_last(input.dims(), out_features)?;
        let output = self.dense_out(dims, input.dtype);
        self.push(RecordedCall::Linear {
            input: input.shape.clone(),
            weight: layer.weight().shape.clone(),
            bias: layer.has_bias(),
            grouped: matches!(layer.weight().storage(), Storage::Grouped { .. }),
            output: output.shape.clone(),
        });
        Ok(output)
    }

    fn conv2d(
        &mut self,
        input: &Self::Tensor,
        layer: &Conv2d<Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error> {
        require_same_dtype("conv2d", input.dtype, layer.weight().dtype())?;
        if layer.kernel() != CONV_KERNEL || layer.stride() != CONV_STRIDE || layer.pad() != CONV_PAD
        {
            return Err(TensorError::new(
                "conv2d",
                format!("只接受核 {CONV_KERNEL}、步长 {CONV_STRIDE}、填充 {CONV_PAD}"),
            ));
        }
        let (out_h, out_w) = check_conv_input(input.dims(), layer.in_ch())?;
        let output = self.dense_out(
            vec![input.dims()[0], out_h, out_w, layer.out_ch()],
            input.dtype,
        );
        self.push(RecordedCall::Conv2d {
            input: input.shape.clone(),
            in_ch: layer.in_ch(),
            out_ch: layer.out_ch(),
            kernel: layer.kernel(),
            stride: layer.stride(),
            pad: layer.pad(),
            output: output.shape.clone(),
        });
        Ok(output)
    }

    fn layer_norm(
        &mut self,
        input: &Self::Tensor,
        norm: &LayerNorm<Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error> {
        self.norm(
            "layer_norm",
            input,
            norm.dim(),
            norm.weight().dtype(),
            |input, dim| RecordedCall::LayerNorm { input, dim },
        )
    }

    fn rms_norm(
        &mut self,
        input: &Self::Tensor,
        norm: &RmsNorm<Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error> {
        self.norm(
            "rms_norm",
            input,
            norm.dim(),
            norm.weight().dtype(),
            |input, dim| RecordedCall::RmsNorm { input, dim },
        )
    }

    fn attention(
        &mut self,
        query: &Self::Tensor,
        key: &Self::Tensor,
        value: &Self::Tensor,
        mask: Option<&Self::Tensor>,
    ) -> Result<Self::Tensor, Self::Error> {
        require_float("attention", query.dtype)?;
        require_same_dtype("attention", query.dtype, key.dtype())?;
        require_same_dtype("attention", query.dtype, value.dtype())?;
        let (batch, query_heads, query_len, query_dim) = attention_rank("查询", query.dims())?;
        let (key_batch, key_heads, key_len, key_dim) = attention_rank("键", key.dims())?;
        let (value_batch, value_heads, value_len, value_dim) = attention_rank("值", value.dims())?;
        if batch != key_batch || batch != value_batch {
            return Err(TensorError::new(
                "attention",
                "查询、键、值的批大小必须相同",
            ));
        }
        if query_dim != key_dim {
            return Err(TensorError::new(
                "attention",
                format!("查询头维 {query_dim} 与键头维 {key_dim} 不一致"),
            ));
        }
        if key_heads != value_heads || key_len != value_len {
            return Err(TensorError::new(
                "attention",
                "键和值的头数与时间长度必须相同",
            ));
        }
        if key_heads == 0 || query_heads % key_heads != 0 {
            return Err(TensorError::new(
                "attention",
                format!("查询头 {query_heads} 不能被键值头 {key_heads} 整除"),
            ));
        }
        let score = [batch, query_heads, query_len, key_len];
        if let Some(mask) = mask {
            require_same_dtype("attention", query.dtype, mask.dtype())?;
            let _ = broadcast_shape_for("attention", &score, mask.dims())?;
        }
        let output = self.dense_out(vec![batch, query_heads, query_len, value_dim], query.dtype);
        self.push(RecordedCall::Attention {
            query: query.shape.clone(),
            key: key.shape.clone(),
            value: value.shape.clone(),
            mask: mask.map(|tensor| tensor.shape.clone()),
            output: output.shape.clone(),
        });
        Ok(output)
    }

    fn kv_write(
        &mut self,
        cache: &mut KvCache<Self::Tensor>,
        key: &Self::Tensor,
        value: &Self::Tensor,
    ) -> Result<(), Self::Error> {
        require_float("kv_write", key.dtype)?;
        require_same_dtype("kv_write", key.dtype, value.dtype())?;
        let key_rank = attention_rank("键", key.dims())?;
        let value_rank = attention_rank("值", value.dims())?;
        if key_rank != value_rank {
            return Err(TensorError::new("kv_write", "键和值的形状必须相同"));
        }
        let (slot_key, slot_value) = cache.pair_mut();
        let cached = match (slot_key.clone(), slot_value.clone()) {
            (None, None) => key.clone(),
            (Some(prev_key), Some(prev_value)) => {
                if prev_key.dtype != key.dtype || prev_value.dims() != prev_key.dims() {
                    return Err(TensorError::new("kv_write", "缓存里的键和值不完整"));
                }
                if prev_key.dims()[0] != key.dims()[0]
                    || prev_key.dims()[1] != key.dims()[1]
                    || prev_key.dims()[3] != key.dims()[3]
                {
                    return Err(TensorError::new(
                        "kv_write",
                        "缓存与新键的批、头或头维不一致",
                    ));
                }
                let mut dims = key.dims().to_vec();
                dims[2] = prev_key.dims()[2]
                    .checked_add(key.dims()[2])
                    .ok_or_else(|| TensorError::new("kv_write", "时间轴长度溢出"))?;
                FakeTensor::dense(&dims, key.dtype)
            }
            _ => return Err(TensorError::new("kv_write", "缓存里的键和值不完整")),
        };
        let cached_time = cached.dims()[2];
        self.push(RecordedCall::KvWrite {
            key: key.shape.clone(),
            value: value.shape.clone(),
            cached_time,
        });
        *slot_key = Some(cached.clone());
        *slot_value = Some(FakeTensor::dense(cached.dims(), value.dtype));
        Ok(())
    }

    fn kv_read(
        &mut self,
        cache: &KvCache<Self::Tensor>,
    ) -> Result<(Self::Tensor, Self::Tensor), Self::Error> {
        let key = cache
            .key()
            .cloned()
            .ok_or_else(|| TensorError::new("kv_read", "缓存是空的"))?;
        let value = cache
            .value()
            .cloned()
            .ok_or_else(|| TensorError::new("kv_read", "缓存是空的"))?;
        self.push(RecordedCall::KvRead {
            key: key.shape.clone(),
            value: value.shape.clone(),
        });
        Ok((key, value))
    }

    fn sum(
        &mut self,
        input: &Self::Tensor,
        axis: usize,
        keep_dim: bool,
    ) -> Result<Self::Tensor, Self::Error> {
        self.reduce(ReduceOp::Sum, input, axis, keep_dim)
    }

    fn max(
        &mut self,
        input: &Self::Tensor,
        axis: usize,
        keep_dim: bool,
    ) -> Result<Self::Tensor, Self::Error> {
        self.reduce(ReduceOp::Max, input, axis, keep_dim)
    }

    fn min(
        &mut self,
        input: &Self::Tensor,
        axis: usize,
        keep_dim: bool,
    ) -> Result<Self::Tensor, Self::Error> {
        self.reduce(ReduceOp::Min, input, axis, keep_dim)
    }
}

impl FakeBackend {
    fn binary(
        &mut self,
        op: &'static str,
        lhs: &FakeTensor,
        rhs: &FakeTensor,
        record: impl FnOnce(Shape, Shape, Shape) -> RecordedCall,
    ) -> Result<FakeTensor, TensorError> {
        require_numeric(op, lhs.dtype)?;
        require_same_dtype(op, lhs.dtype, rhs.dtype)?;
        let shape = broadcast_shape_for(op, lhs.dims(), rhs.dims())?;
        let output = self.dense_out(shape.dims().to_vec(), lhs.dtype);
        self.push(record(
            lhs.shape.clone(),
            rhs.shape.clone(),
            output.shape.clone(),
        ));
        Ok(output)
    }

    fn norm(
        &mut self,
        op: &'static str,
        input: &FakeTensor,
        dim: usize,
        weight_dtype: DType,
        record: impl FnOnce(Shape, usize) -> RecordedCall,
    ) -> Result<FakeTensor, TensorError> {
        require_same_dtype(op, input.dtype, weight_dtype)?;
        check_last_dim(op, input.dims(), dim)?;
        self.push(record(input.shape.clone(), dim));
        Ok(input.clone())
    }

    fn reduce(
        &mut self,
        op: ReduceOp,
        input: &FakeTensor,
        axis: usize,
        keep_dim: bool,
    ) -> Result<FakeTensor, TensorError> {
        require_numeric(op.name(), input.dtype)?;
        let dims = reduced_shape(op.name(), input.dims(), axis, keep_dim)?;
        let output = self.dense_out(dims, input.dtype);
        self.push(RecordedCall::Reduce {
            op,
            input: input.shape.clone(),
            axis,
            output: output.shape.clone(),
        });
        Ok(output)
    }
}

impl UnaryOp {
    fn name(self) -> &'static str {
        match self {
            Self::Rsqrt => "rsqrt",
            Self::Exp => "exp",
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Gelu => "gelu",
            Self::Silu => "silu",
        }
    }
}

impl ReduceOp {
    fn name(self) -> &'static str {
        match self {
            Self::Sum => "sum",
            Self::Max => "max",
            Self::Min => "min",
        }
    }
}

fn attention_rank(
    label: &str,
    dims: &[usize],
) -> Result<(usize, usize, usize, usize), TensorError> {
    if dims.len() != 4 {
        return Err(TensorError::new(
            "attention",
            format!("{label}必须是 (B, H, L, D)，实际 {}", format_dims(dims)),
        ));
    }
    Ok((dims[0], dims[1], dims[2], dims[3]))
}

fn arange_len(start: i64, stop: i64, step: i64) -> Result<usize, TensorError> {
    if step == 0 {
        return Err(TensorError::new("arange", "步长不能为 0"));
    }
    let start = i128::from(start);
    let stop = i128::from(stop);
    let step = i128::from(step);
    let len = if step > 0 && start < stop {
        (stop - start + step - 1) / step
    } else if step < 0 && start > stop {
        (start - stop - step - 1) / -step
    } else {
        0
    };
    usize::try_from(len).map_err(|_| TensorError::new("arange", "范围长度溢出"))
}
