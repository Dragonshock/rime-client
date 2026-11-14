use crate::error::TensorError;
use std::fmt;

/// 张量的形状。维度从左到右，与调用处写出的顺序一致。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shape(Vec<usize>);

impl Shape {
    /// 用给定的各维长度构造形状。
    pub fn new(dims: impl Into<Vec<usize>>) -> Self {
        Self(dims.into())
    }

    /// 各维长度。
    pub fn dims(&self) -> &[usize] {
        &self.0
    }

    /// 秩。
    pub fn rank(&self) -> usize {
        self.0.len()
    }

    /// 元素个数。溢出时返回错误。
    pub fn numel(&self) -> Result<usize, TensorError> {
        checked_numel("shape", &self.0)
    }
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_dims(f, &self.0)
    }
}

impl From<Vec<usize>> for Shape {
    fn from(value: Vec<usize>) -> Self {
        Self(value)
    }
}

impl From<&[usize]> for Shape {
    fn from(value: &[usize]) -> Self {
        Self(value.to_vec())
    }
}

impl<const N: usize> From<[usize; N]> for Shape {
    fn from(value: [usize; N]) -> Self {
        Self(value.to_vec())
    }
}

pub(crate) fn checked_numel(op: &'static str, dims: &[usize]) -> Result<usize, TensorError> {
    let mut n = 1usize;
    for &dim in dims {
        n = n.checked_mul(dim).ok_or_else(|| {
            TensorError::new(op, format!("元素个数溢出，形状是 {}", format_dims(dims)))
        })?;
    }
    Ok(n)
}

/// 右对齐广播。某一侧缺失的维度视为 1。
pub fn broadcast_shape(lhs: &[usize], rhs: &[usize]) -> Result<Shape, TensorError> {
    broadcast_shape_for("broadcast", lhs, rhs)
}

/// 把多个形状依次广播到同一个形状。
pub fn broadcast_all(shapes: &[&[usize]]) -> Result<Shape, TensorError> {
    broadcast_all_for("broadcast", shapes)
}

pub(crate) fn broadcast_shape_for(
    op: &'static str,
    lhs: &[usize],
    rhs: &[usize],
) -> Result<Shape, TensorError> {
    let rank = lhs.len().max(rhs.len());
    let mut out = Vec::with_capacity(rank);
    for index in 0..rank {
        let left = dim_from_right(lhs, rank, index);
        let right = dim_from_right(rhs, rank, index);
        if left == right || left == 1 || right == 1 {
            out.push(left.max(right));
        } else {
            return Err(TensorError::new(
                op,
                format!("无法广播 {} 与 {}", format_dims(lhs), format_dims(rhs)),
            ));
        }
    }
    Ok(Shape(out))
}

pub(crate) fn broadcast_all_for(
    op: &'static str,
    shapes: &[&[usize]],
) -> Result<Shape, TensorError> {
    let mut acc = Shape::new(Vec::new());
    for shape in shapes {
        acc = broadcast_shape_for(op, acc.dims(), shape)?;
    }
    Ok(acc)
}

/// 卷积输出的空间长度：`(len + 2 * pad - kernel) / stride + 1`。
pub fn conv2d_spatial_out(
    len: usize,
    kernel: usize,
    stride: usize,
    pad: usize,
) -> Result<usize, TensorError> {
    if kernel == 0 || stride == 0 {
        return Err(TensorError::new("conv2d", "核大小与步长必须大于 0"));
    }
    let padded = len
        .checked_add(pad.saturating_mul(2))
        .ok_or_else(|| TensorError::new("conv2d", "补齐后的空间长度溢出"))?;
    if padded < kernel {
        return Err(TensorError::new(
            "conv2d",
            format!("空间长度 {len} 在核 {kernel}、边 {pad} 下无法卷积"),
        ));
    }
    Ok((padded - kernel) / stride + 1)
}

pub(crate) fn replace_last(dims: &[usize], last: usize) -> Result<Vec<usize>, TensorError> {
    if dims.is_empty() {
        return Err(TensorError::new("linear", "输入至少要有一维"));
    }
    let mut out = dims.to_vec();
    let index = out.len() - 1;
    out[index] = last;
    Ok(out)
}

pub(crate) fn infer_reshape(
    op: &'static str,
    numel: usize,
    shape: &[isize],
) -> Result<Vec<usize>, TensorError> {
    if shape.is_empty() {
        if numel == 1 {
            return Ok(Vec::new());
        }
        return Err(TensorError::new(
            op,
            format!("标量只能容纳 1 个元素，实际有 {numel}"),
        ));
    }
    let mut inferred = None;
    let mut product = 1usize;
    for (index, &dim) in shape.iter().enumerate() {
        if dim == -1 {
            if inferred.is_some() {
                return Err(TensorError::new(op, "最多只能有一个 -1"));
            }
            inferred = Some(index);
        } else if dim < 0 {
            return Err(TensorError::new(op, format!("维度 {dim} 不是正数或 -1")));
        } else {
            product = product
                .checked_mul(dim as usize)
                .ok_or_else(|| TensorError::new(op, "目标形状的元素个数溢出"))?;
        }
    }
    let mut out: Vec<usize> = shape
        .iter()
        .map(|&dim| if dim < 0 { 0 } else { dim as usize })
        .collect();
    if let Some(index) = inferred {
        if product == 0 {
            return Err(TensorError::new(op, "有 0 维时无法推断 -1"));
        }
        if !numel.is_multiple_of(product) {
            return Err(TensorError::new(
                op,
                format!("元素个数 {numel} 不能被 {product} 整除"),
            ));
        }
        out[index] = numel / product;
    } else if product != numel {
        return Err(TensorError::new(
            op,
            format!("元素个数不一致：输入 {numel}，目标 {}", format_dims(&out)),
        ));
    }
    Ok(out)
}

pub(crate) fn axis_index(op: &'static str, axis: usize, rank: usize) -> Result<usize, TensorError> {
    if axis >= rank {
        return Err(TensorError::new(op, format!("轴 {axis} 超出秩 {rank}")));
    }
    Ok(axis)
}

pub(crate) fn format_dims(dims: &[usize]) -> String {
    let mut text = String::from("[");
    for (index, dim) in dims.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        text.push_str(&dim.to_string());
    }
    text.push(']');
    text
}

fn dim_from_right(dims: &[usize], rank: usize, index: usize) -> usize {
    if index < rank - dims.len() {
        1
    } else {
        dims[index - (rank - dims.len())]
    }
}

fn write_dims(f: &mut fmt::Formatter<'_>, dims: &[usize]) -> fmt::Result {
    write!(f, "{}", format_dims(dims))
}
