//! 编译特性决定会话用哪一个张量后端。
//!
//! `metal` 打开时，引擎线程用 `voice-mlx` 的 GPU stream。
//! 关闭时仍用 `voice-tensor` 的假后端，不链接 MLX。

#[cfg(feature = "metal")]
use voice_mlx::MlxBackend;
#[cfg(not(feature = "metal"))]
use voice_tensor::FakeBackend;

#[cfg(feature = "metal")]
pub(crate) type Active = MlxBackend;
#[cfg(not(feature = "metal"))]
pub(crate) type Active = FakeBackend;

/// 引擎线程启动时调用。模型目录已经在 C ABI 里确认能打开。
///
/// 打开 `metal` 时这一处就把常驻的两座塔、分词器与梅尔前端打开：先设好 metallib
/// 与 GPU stream，再读 513 MB 权重。打不开权重就当没准备好，会话不会拿到半份实例。
pub(crate) fn boot(model_dir: &str, metallib: Option<&str>) -> bool {
    let _ = std::fs::read_dir(model_dir);
    #[cfg(feature = "metal")]
    {
        let Some(path) = metallib else {
            return false;
        };
        if MlxBackend::prepare(path).is_err() {
            return false;
        }
        return crate::decoder::boot_resident(model_dir);
    }
    #[cfg(not(feature = "metal"))]
    {
        if let Some(path) = metallib {
            let _ = std::fs::metadata(path);
        }
        crate::decoder::boot_resident(model_dir)
    }
}

pub(crate) fn new_backend() -> Result<Active, ()> {
    #[cfg(feature = "metal")]
    {
        MlxBackend::on_this_thread().map_err(|_| ())
    }
    #[cfg(not(feature = "metal"))]
    {
        Ok(FakeBackend::new())
    }
}
