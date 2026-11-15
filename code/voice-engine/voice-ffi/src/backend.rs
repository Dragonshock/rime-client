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
/// 打开 `metal` 时这一处只装第一段：先设好 metallib 与 GPU stream，再读音频塔
/// 那 202 MB（523 张张量）与梅尔前端的表。文本塔（335 MB、704 张张量）不在
/// 这里：它留到 `voice_session_begin` 之后再装，见 [`load_text_deferred`]。
/// 打不开音频塔就当没准备好，会话不会拿到半份实例。
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

/// `voice_session_begin` 之后那一步：把文本塔装上。
///
/// 引擎线程上跑，调用方不等它：`begin` 的回已经在队列里回过了，用户接着说话，
/// 音频排在有界队列里等这一步跑完。已经装好就回 `Ok(false)`，不读盘。
///
/// 装不上不在这里失败：`finish` 那次会再叫一次，并在真装不上时报原因。
pub(crate) fn load_text_deferred() -> Result<bool, String> {
    #[cfg(test)]
    DEFERRED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    crate::decoder::load_text_resident()
}

/// 第二段被叫了几次。只在测试里存在：产品路径不留计数器。
///
/// 两条时序断言靠它：「`begin` 之后真的叫过一次」与「`finish` 之前又叫过一次」。
#[cfg(test)]
pub(crate) static DEFERRED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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
