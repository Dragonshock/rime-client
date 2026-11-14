use std::fmt;

use voice_tensor::TensorError;

/// 图跑不起来时的错误。后端错误与形状错误分开存放。
#[derive(Debug)]
pub enum EngineError<E> {
    /// 张量后端返回的错误。
    Backend(E),
    /// 线性层、卷积或归一化的参数对不上。
    Graph(TensorError),
}

impl<E: fmt::Display> fmt::Display for EngineError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Backend(error) => write!(f, "张量后端：{error}"),
            EngineError::Graph(error) => write!(f, "识别图：{error}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for EngineError<E> {}

pub(crate) fn backend<T, E>(result: Result<T, E>) -> Result<T, EngineError<E>> {
    result.map_err(EngineError::Backend)
}

pub(crate) fn graph<T, E>(result: Result<T, TensorError>) -> Result<T, EngineError<E>> {
    result.map_err(EngineError::Graph)
}
