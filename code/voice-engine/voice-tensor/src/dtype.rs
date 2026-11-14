/// 计算用的数据类型。
///
/// 这里不使用 MLX 的类型名。分组存放的权重在求值前反量化到其中的浮点类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DType {
    /// 32 位浮点。
    F32,
    /// 16 位浮点。
    F16,
    /// 32 位有符号整数，用于下标。
    I32,
    /// 16 位有符号整数，用于原始 PCM。
    I16,
    /// 布尔，用于 `where` 的条件。
    Bool,
}

impl DType {
    /// 是否为浮点。
    pub const fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F16)
    }

    /// 是否可以做加减和归约。
    pub const fn is_numeric(self) -> bool {
        !matches!(self, Self::Bool)
    }
}

impl std::fmt::Display for DType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::F32 => "f32",
            Self::F16 => "f16",
            Self::I32 => "i32",
            Self::I16 => "i16",
            Self::Bool => "bool",
        })
    }
}
