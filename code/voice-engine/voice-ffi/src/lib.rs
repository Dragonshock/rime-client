//! C ABI。Swift 只通过 `voice_engine.h` 里的符号把 PCM 送进来。
//!
//! 引擎线程收到 `feed` 与 `finish` 后调用 `voice-asr`。
//! 张量留在那根线程上。编译打开 `metal` 时本箱改用 `voice-mlx`，关闭时仍用假后端。
//! 键盘组合不在这里。

mod backend;
mod decoder;
mod engine;
mod json_line;

use std::ffi::{CStr, OsStr};
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use engine::{Callback, ReplySink, TextFn, Worker};

/// 模型目录能读时才启动引擎线程。打不开则返回空指针。
///
/// # Safety
/// `model_dir` 与 `metallib` 要么为空，要么指向以 NUL 结尾的字节串。
#[no_mangle]
pub unsafe extern "C" fn voice_engine_new(
    model_dir: *const c_char,
    metallib: *const c_char,
) -> *mut c_void {
    let Some(dir) = cstr_path(model_dir) else {
        return std::ptr::null_mut();
    };
    if !model_dir_opens(&dir) {
        return std::ptr::null_mut();
    }
    let metal = cstr_path(metallib);
    #[cfg(feature = "metal")]
    {
        let Some(path) = metal.as_deref() else {
            return std::ptr::null_mut();
        };
        if !Path::new(path).is_file() {
            return std::ptr::null_mut();
        }
    }
    match Worker::start(dir, metal) {
        Ok(worker) => Box::into_raw(Box::new(worker)) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

/// 停掉引擎线程并释放。空指针无事发生。
///
/// # Safety
/// `engine` 必须是 `voice_engine_new` 的返回值，且只释放一次。
#[no_mangle]
pub unsafe extern "C" fn voice_engine_free(engine: *mut c_void) {
    if engine.is_null() {
        return;
    }
    drop(Box::from_raw(engine as *mut Worker));
}

/// 按下时开一次会话。热词指针为空或空串时不写入热词。
///
/// # Safety
/// `engine` 必须仍有效。`hotwords_or_null` 要么为空，要么以 NUL 结尾。
#[no_mangle]
pub unsafe extern "C" fn voice_session_begin(
    engine: *mut c_void,
    session: u64,
    hotwords_or_null: *const c_char,
) -> c_int {
    let Some(worker) = worker_ref(engine) else {
        return -1;
    };
    worker.begin(session, cstr_text(hotwords_or_null))
}

/// 把一段 16 kHz 单声道 `i16` 推进有界队列。调用线程不碰张量。
///
/// # Safety
/// `pcm` 在 `n > 0` 时必须指向至少 `n` 个 `int16_t`，本次调用返回前保持有效。
#[no_mangle]
pub unsafe extern "C" fn voice_session_feed(
    engine: *mut c_void,
    session: u64,
    pcm: *const i16,
    n: usize,
) -> c_int {
    let Some(worker) = worker_ref(engine) else {
        return -1;
    };
    if pcm.is_null() && n != 0 {
        return -1;
    }
    let samples = if n == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(pcm, n).to_vec()
    };
    worker.feed(session, samples)
}

/// 松开。引擎线程收齐后调用 `voice-asr` 的 `finish`。
///
/// # Safety
/// `engine` 必须仍有效。
#[no_mangle]
pub unsafe extern "C" fn voice_session_finish(engine: *mut c_void, session: u64) -> c_int {
    let Some(worker) = worker_ref(engine) else {
        return -1;
    };
    worker.finish(session)
}

/// 设置整段文本的回调。函数指针为空时清掉回调。
///
/// # Safety
/// `engine` 必须仍有效。`ctx` 必须活到引擎释放，或先把回调清掉。
#[no_mangle]
pub unsafe extern "C" fn voice_engine_set_callback(
    engine: *mut c_void,
    function: Option<TextFn>,
    ctx: *mut c_void,
) {
    let Some(worker) = worker_ref(engine) else {
        return;
    };
    let callback = function.map(|function| Callback { function, ctx });
    worker.set_callback(callback);
}

/// 设置一行 JSON 的回复口。函数指针为空时清掉。
///
/// 装上之后文本事件只走这个口，不再调 `voice_text_fn`；两个口不会重复发。
/// 定稿失败时也在这里发一行 `ok:false`，`message` 是原因。
///
/// # Safety
/// `engine` 必须仍有效。`ctx` 必须活到引擎释放，或先把回复口清掉。
#[no_mangle]
pub unsafe extern "C" fn voice_engine_set_reply_sink(
    engine: *mut c_void,
    function: Option<engine::JsonFn>,
    ctx: *mut c_void,
) {
    let Some(worker) = worker_ref(engine) else {
        return;
    };
    worker.set_reply_sink(function.map(|function| ReplySink { function, ctx }));
}

/// 写出上一次 `voice_session_finish` 失败的原因。
///
/// 返回写入的字节数（不含结尾 NUL）；没有失败返回 0；指针为空或缓冲区不够时返回 -1。
///
/// # Safety
/// `engine` 必须仍有效。`utf8` 必须指向至少 `n` 字节可写内存。
#[no_mangle]
pub unsafe extern "C" fn voice_engine_last_error(
    engine: *mut c_void,
    utf8: *mut c_char,
    n: usize,
) -> c_int {
    let Some(worker) = worker_ref(engine) else {
        return -1;
    };
    let Some(reason) = worker.last_failure() else {
        return 0;
    };
    let bytes = reason.as_bytes();
    if utf8.is_null() || n <= bytes.len() {
        return -1;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, utf8, bytes.len());
    *utf8.add(bytes.len()) = 0;
    bytes.len() as c_int
}

fn worker_ref<'a>(engine: *mut c_void) -> Option<&'a Worker> {
    if engine.is_null() {
        None
    } else {
        Some(unsafe { &*(engine as *const Worker) })
    }
}

fn cstr_path(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe { CStr::from_ptr(ptr) }.to_bytes();
    if bytes.is_empty() {
        return None;
    }
    Some(OsStr::from_bytes(bytes).to_string_lossy().into_owned())
}

fn cstr_text(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(ptr) }.to_string_lossy();
    if text.is_empty() {
        None
    } else {
        Some(text.into_owned())
    }
}

fn model_dir_opens(dir: &str) -> bool {
    let path = Path::new(dir);
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    meta.is_dir() && std::fs::read_dir(path).is_ok()
}

#[cfg(test)]
mod tests {
    /// 头文件里的九个声明。
    const ABI_DECLARATIONS: [&str; 9] = [
        "voice_text_fn",
        "voice_reply_fn",
        "voice_engine_new",
        "voice_engine_free",
        "voice_session_begin",
        "voice_session_feed",
        "voice_session_finish",
        "voice_engine_set_callback",
        "voice_engine_set_reply_sink",
    ];

    use super::*;
    use std::ffi::CString;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;
    use voice_asr::{DecodeText, DecodeView, PrefixPart, StepOutput};

    struct Record {
        thread_name: Option<String>,
        flags: Vec<bool>,
        saw_hotword: bool,
    }

    struct RecordingDecoder {
        record: Arc<Mutex<Record>>,
    }

    impl DecodeText for RecordingDecoder {
        fn decode_text(&mut self, view: &DecodeView<'_>) -> StepOutput {
            let mut record = self.record.lock().unwrap();
            record.thread_name = thread::current().name().map(str::to_string);
            record.flags.push(view.is_final);
            record.saw_hotword |= view
                .prefix
                .iter()
                .any(|part| matches!(part, PrefixPart::System(text) if text == "专名"));
            let text = if view.is_final { "定稿" } else { "部分" };
            StepOutput {
                text: text.to_string(),
                tokens: vec![1, 2, 3, 4, 5, 6, 7],
            }
        }
    }

    fn temp_dir() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "voice-ffi-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("时钟")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("临时目录");
        path
    }

    #[test]
    fn header_lists_nine_declarations() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("include/voice_engine.h");
        let text = std::fs::read_to_string(&path).expect("voice_engine.h");
        assert_eq!(ABI_DECLARATIONS.len(), 9);
        for name in ABI_DECLARATIONS {
            assert!(text.contains(name), "头文件缺少 {name}");
        }
        assert!(
            text.contains("voice_engine_last_error"),
            "头文件缺少 last_error"
        );
        let headers = std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("include"))
            .expect("include")
            .map(|entry| entry.expect("目录项").file_name())
            .collect::<Vec<_>>();
        assert_eq!(headers, vec![std::ffi::OsString::from("voice_engine.h")]);
    }

    #[test]
    fn missing_model_dir_returns_null() {
        let missing = CString::new("/no/such/voice-ffi/model").unwrap();
        let engine = unsafe { voice_engine_new(missing.as_ptr(), std::ptr::null()) };
        assert!(engine.is_null());
        assert!(unsafe { voice_engine_new(std::ptr::null(), std::ptr::null()) }.is_null());
        assert_eq!(
            unsafe { voice_session_begin(std::ptr::null_mut(), 1, std::ptr::null()) },
            -1
        );
        assert_eq!(
            unsafe { voice_session_feed(std::ptr::null_mut(), 1, std::ptr::null(), 0) },
            -1
        );
        assert_eq!(unsafe { voice_session_finish(std::ptr::null_mut(), 1) }, -1);
        assert_eq!(
            unsafe { voice_engine_last_error(std::ptr::null_mut(), std::ptr::null_mut(), 0) },
            -1
        );
        unsafe { voice_engine_free(std::ptr::null_mut()) };
    }

    #[test]
    fn engine_thread_feeds_and_finishes_through_voice_asr() {
        let dir = temp_dir();
        let record = Arc::new(Mutex::new(Record {
            thread_name: None,
            flags: Vec::new(),
            saw_hotword: false,
        }));
        let shared = Arc::clone(&record);
        let dir_text = dir.to_string_lossy().into_owned();
        let worker = Worker::spawn(dir_text, None, move || RecordingDecoder {
            record: Arc::clone(&shared),
        })
        .expect("线程");
        let caller = thread::current().name().map(str::to_string);
        assert_eq!(worker.begin(7, Some("专名".to_string())), 0);
        assert_eq!(worker.begin(7, None), -1);
        assert_eq!(worker.feed(7, vec![0; 31_999]), 0);
        assert_eq!(worker.feed(7, vec![1]), 0);
        let saw_partial = engine::wait_until(Duration::from_secs(5), || {
            record.lock().unwrap().flags.len() == 1
        });
        assert!(saw_partial, "满 32000 个样本应让引擎线程调用 voice-asr");
        {
            let record = record.lock().unwrap();
            assert_eq!(record.flags, vec![false]);
            assert_eq!(record.thread_name.as_deref(), Some("voice-engine"));
            assert_ne!(record.thread_name, caller);
            assert!(record.saw_hotword);
        }
        assert_eq!(worker.finish(7), 0);
        assert_eq!(worker.finish(7), -1);
        let record = record.lock().unwrap();
        assert_eq!(record.flags, vec![false, true]);
        drop(record);
        worker.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct Event {
        session: u64,
        text: String,
        is_final: i32,
        thread_name: Option<String>,
    }

    unsafe extern "C" fn collect_text(
        session: u64,
        utf8: *const c_char,
        is_final: i32,
        ctx: *mut c_void,
    ) {
        let slot = &*(ctx as *const Mutex<Vec<Event>>);
        let text = if utf8.is_null() {
            String::new()
        } else {
            CStr::from_ptr(utf8).to_string_lossy().into_owned()
        };
        slot.lock().unwrap().push(Event {
            session,
            text,
            is_final,
            thread_name: thread::current().name().map(str::to_string),
        });
    }

    #[test]
    fn c_abi_empty_finish_callbacks_empty_final() {
        let dir = temp_dir();
        let dir_c = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
        let metal = CString::new("mlx.metallib").unwrap();
        let engine = unsafe { voice_engine_new(dir_c.as_ptr(), metal.as_ptr()) };
        assert!(!engine.is_null());
        let events = Mutex::new(Vec::<Event>::new());
        unsafe {
            voice_engine_set_callback(
                engine,
                Some(collect_text),
                &events as *const Mutex<Vec<Event>> as *mut c_void,
            );
        }
        let hotwords = CString::new("").unwrap();
        assert_eq!(
            unsafe { voice_session_begin(engine, 3, hotwords.as_ptr()) },
            0
        );
        assert_eq!(unsafe { voice_session_finish(engine, 3) }, 0);
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session, 3);
        assert_eq!(events[0].text, "");
        assert_eq!(events[0].is_final, 1);
        assert_eq!(events[0].thread_name.as_deref(), Some("voice-engine"));
        drop(events);
        unsafe { voice_engine_free(engine) };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn c_abi_feed_then_finish_runs_on_engine_thread() {
        let dir = temp_dir();
        let dir_c = CString::new(dir.to_str().unwrap()).unwrap();
        let engine = unsafe { voice_engine_new(dir_c.as_ptr(), std::ptr::null()) };
        assert!(!engine.is_null());
        let events = Mutex::new(Vec::<Event>::new());
        unsafe {
            voice_engine_set_callback(
                engine,
                Some(collect_text),
                &events as *const Mutex<Vec<Event>> as *mut c_void,
            );
        }
        let hotwords = CString::new("专名").unwrap();
        assert_eq!(
            unsafe { voice_session_begin(engine, 9, hotwords.as_ptr()) },
            0
        );
        let pcm = vec![0i16; 32_000];
        assert_eq!(
            unsafe { voice_session_feed(engine, 9, pcm.as_ptr(), pcm.len()) },
            0
        );
        assert_eq!(
            unsafe { voice_session_feed(engine, 9, std::ptr::null(), 0) },
            0
        );
        assert_eq!(unsafe { voice_session_finish(engine, 9) }, 0);
        let events = events.lock().unwrap();
        assert!(events.iter().any(|event| event.is_final == 1));
        assert!(events.iter().all(|event| event.text.is_empty()));
        assert!(events
            .iter()
            .all(|event| event.thread_name.as_deref() == Some("voice-engine")));
        assert!(events.iter().all(|event| event.session == 9));
        drop(events);
        unsafe { voice_engine_free(engine) };
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 只报失败、不写文本的解码器。定稿原因走 `failure_reason`。
    struct FailingDecoder;

    impl DecodeText for FailingDecoder {
        fn decode_text(&mut self, _view: &DecodeView<'_>) -> StepOutput {
            StepOutput {
                text: String::new(),
                tokens: Vec::new(),
            }
        }

        fn supplies_numeric_text(&self) -> bool {
            true
        }

        fn weight_failed(&self) -> bool {
            true
        }

        fn failure_reason(&self) -> Option<String> {
            Some("当前解码器只能给一个 token，不是可提交文本".to_string())
        }
    }

    unsafe extern "C" fn collect_json(json_line: *const c_char, ctx: *mut c_void) {
        let slot = &*(ctx as *const Mutex<Vec<String>>);
        let line = if json_line.is_null() {
            String::new()
        } else {
            CStr::from_ptr(json_line).to_string_lossy().into_owned()
        };
        slot.lock().unwrap().push(line);
    }

    #[test]
    fn finish_failure_reason_reaches_the_json_sink_and_last_error() {
        let dir = temp_dir();
        let dir_text = dir.to_string_lossy().into_owned();
        let worker = Worker::spawn(dir_text, None, || FailingDecoder).expect("线程");
        let lines = Mutex::new(Vec::<String>::new());
        worker.set_reply_sink(Some(engine::ReplySink {
            function: collect_json,
            ctx: &lines as *const Mutex<Vec<String>> as *mut c_void,
        }));
        assert_eq!(worker.begin(5, None), 0);
        assert_eq!(worker.feed(5, vec![0; 16_000]), 0);
        assert_eq!(worker.finish(5), -1);
        assert_eq!(
            worker.last_failure().as_deref(),
            Some("当前解码器只能给一个 token，不是可提交文本")
        );
        {
            let lines = lines.lock().unwrap();
            assert!(
                lines.iter().any(|line| line.contains("\"ok\":false")
                    && line.contains("不是可提交文本")
                    && line.contains("\"session\":5")),
                "{lines:?}"
            );
        }
        let raw = Box::into_raw(Box::new(worker)) as *mut c_void;
        let mut buffer = vec![0 as c_char; 256];
        let written = unsafe { voice_engine_last_error(raw, buffer.as_mut_ptr(), buffer.len()) };
        assert!(written > 0);
        assert_eq!(
            unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_str().unwrap(),
            "当前解码器只能给一个 token，不是可提交文本"
        );
        assert_eq!(
            unsafe { voice_engine_last_error(raw, std::ptr::null_mut(), 0) },
            -1
        );
        let mut small = [0 as c_char; 2];
        assert_eq!(
            unsafe { voice_engine_last_error(raw, small.as_mut_ptr(), small.len()) },
            -1
        );
        unsafe { voice_engine_free(raw) };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_sink_replaces_the_text_callback() {
        let dir = temp_dir();
        let dir_text = dir.to_string_lossy().into_owned();
        let worker = Worker::spawn(dir_text, None, || FailingDecoder).expect("线程");
        let texts = Mutex::new(Vec::<Event>::new());
        let lines = Mutex::new(Vec::<String>::new());
        worker.set_callback(Some(Callback {
            function: collect_text,
            ctx: &texts as *const Mutex<Vec<Event>> as *mut c_void,
        }));
        worker.set_reply_sink(Some(engine::ReplySink {
            function: collect_json,
            ctx: &lines as *const Mutex<Vec<String>> as *mut c_void,
        }));
        assert_eq!(worker.begin(11, None), 0);
        assert_eq!(worker.feed(11, vec![0; 16_000]), 0);
        assert_eq!(worker.finish(11), -1);
        assert!(
            texts.lock().unwrap().is_empty(),
            "JSON 口在位时不再调文本指针"
        );
        let lines = lines.lock().unwrap();
        assert!(lines.iter().all(|line| line.contains("\"session\":11")));
        assert!(lines.iter().any(|line| line.contains("\"kind\":\"final\"")));
        assert!(lines.iter().any(|line| line.contains("\"decoded\":true")));
    }
}
