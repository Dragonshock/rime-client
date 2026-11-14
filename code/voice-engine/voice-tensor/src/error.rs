use std::fmt;

/// 张量运算的形状或参数错误。失败的调用不记入假后端。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorError {
    op: &'static str,
    message: String,
}

impl TensorError {
    pub(crate) fn new(op: &'static str, message: impl Into<String>) -> Self {
        Self {
            op,
            message: message.into(),
        }
    }

    /// 运算名。
    pub fn op(&self) -> &'static str {
        self.op
    }
}

impl fmt::Display for TensorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}：{}", self.op, self.message)
    }
}

impl std::error::Error for TensorError {}
