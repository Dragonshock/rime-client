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
/// 返回的时候只装好了第一段（梅尔前端与音频塔）：文本塔留到 `voice_session_begin`
/// 之后那一步，跟用户说话重叠。需要「两段都装完」的才叫得动：那一次 `finish`
/// 会等文本塔，不会因为还没装好而给空串。
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
/// 引擎线程回完这个调用之后才开始装文本塔（第二段），所以这里不等那 335 MB；
/// 接下来的 `feed` 排在有界队列里，等那一步跑完。
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
/// 文本塔若还没装好（按下就松手），这一步先等它装完再定稿。
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

/// 丢掉引擎侧的一枚会话：解码器与状态一起释放，号随即可以再用。
///
/// 取消路径（失焦、看门狗）只停采集、不发 `finish`，引擎表里那一枚会话原本会一直
/// 挂着占号占内存。这一句是那个收场的手：没有那枚会话也回 0，取消本来就允许重复。
///
/// # Safety
/// `engine` 必须仍有效（或为空指针，此时返回 -1）。
#[no_mangle]
pub unsafe extern "C" fn voice_session_discard(engine: *mut c_void, session: u64) -> c_int {
    let Some(worker) = worker_ref(engine) else {
        return -1;
    };
    worker.discard(session)
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
    /// 头文件里的十个声明。
    const ABI_DECLARATIONS: [&str; 10] = [
        "voice_text_fn",
        "voice_reply_fn",
        "voice_engine_new",
        "voice_engine_free",
        "voice_session_begin",
        "voice_session_feed",
        "voice_session_finish",
        "voice_session_discard",
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
    fn header_lists_ten_declarations() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("include/voice_engine.h");
        let text = std::fs::read_to_string(&path).expect("voice_engine.h");
        assert_eq!(ABI_DECLARATIONS.len(), 10);
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
            unsafe { voice_session_discard(std::ptr::null_mut(), 1) },
            -1
        );
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
        let worker = Worker::spawn(
            dir_text,
            None,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            move |_| RecordingDecoder {
                record: Arc::clone(&shared),
            },
        )
        .expect("线程");
        let caller = thread::current().name().map(str::to_string);
        // 同号 `begin` 的语义（替换旧会话，不再拒绝）由下面
        // `same_number_begin_replaces_the_old_session` 专门证；这条只管
        // `feed` / `finish` 走完 `voice-asr`。
        assert_eq!(worker.begin(7, Some("专名".to_string())), 0);
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

    /// 每枚解码器带一个标签与自己的热词：替换之后到底是哪一枚在收 `feed` / `finish`。
    /// `dropped` 数的是「这枚解码器什么时候被真正释放」。
    struct TaggedDecoder {
        tag: &'static str,
        hotword: &'static str,
        steps: Arc<Mutex<Vec<(&'static str, bool, bool)>>>,
        dropped: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Drop for TaggedDecoder {
        fn drop(&mut self) {
            self.dropped
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl DecodeText for TaggedDecoder {
        fn decode_text(&mut self, view: &DecodeView<'_>) -> StepOutput {
            let saw_hotword = view
                .prefix
                .iter()
                .any(|part| matches!(part, PrefixPart::System(text) if text == self.hotword));
            self.steps
                .lock()
                .unwrap()
                .push((self.tag, view.is_final, saw_hotword));
            let text = if view.is_final { "定稿" } else { "部分" };
            StepOutput {
                text: text.to_string(),
                tokens: vec![1, 2, 3],
            }
        }
    }

    #[test]
    fn same_number_begin_replaces_the_old_session() {
        let dir = temp_dir();
        let steps = Arc::new(Mutex::new(Vec::<(&'static str, bool, bool)>::new()));
        let dropped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let shared_steps = Arc::clone(&steps);
        let shared_dropped = Arc::clone(&dropped);
        // 工厂每次被叫就换一对标签：第一枚是建来又要被替换掉的旧会话，第二枚是新的。
        let mut warm = true;
        let worker = Worker::spawn(
            dir.to_string_lossy().into_owned(),
            None,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            move |_| {
                let here = Arc::clone(&shared_steps);
                let gone = Arc::clone(&shared_dropped);
                let (tag, hotword) = if warm {
                    warm = false;
                    ("旧", "旧热词")
                } else {
                    ("新", "新热词")
                };
                TaggedDecoder {
                    tag,
                    hotword,
                    steps: here,
                    dropped: gone,
                }
            },
        )
        .expect("线程");
        let lines = Mutex::new(Vec::<String>::new());
        worker.set_reply_sink(Some(engine::ReplySink {
            function: collect_json,
            ctx: &lines as *const Mutex<Vec<String>> as *mut c_void,
        }));

        assert_eq!(worker.begin(4, Some("旧热词".to_string())), 0);
        // 同号再来一次：接受，并把那枚旧会话丢掉。
        assert_eq!(worker.begin(4, Some("新热词".to_string())), 0);
        assert_eq!(
            dropped.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "旧会话的解码器应当在替换时就被释放"
        );

        assert_eq!(worker.feed(4, vec![0; 32_000]), 0);
        assert_eq!(worker.finish(4), 0);
        assert_eq!(worker.finish(4), -1);

        {
            let steps = steps.lock().unwrap();
            // 两步都只落在新会话上，而且用的是新的热词。
            assert_eq!(*steps, vec![("新", false, true), ("新", true, true)]);
            assert!(!steps.iter().any(|(tag, _, _)| *tag == "旧"));
        }
        {
            let lines = lines.lock().unwrap();
            assert!(
                lines
                    .iter()
                    .any(|line| line.contains("\"kind\":\"replaced\"")
                        && line.contains("\"session\":4")),
                "替换应留一行带号的日志：{lines:?}"
            );
        }
        assert_eq!(
            dropped.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "定稿之后两枚解码器都已释放"
        );
        worker.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 换窗口之后连着三枚新的 `VoiceMachine`：真实路上号由共享计数器依次发 1、2、3。
    /// 三枚会话各自立起来、各自 `feed` / `finish`，引擎三次都回 0——没有一条 -1。
    /// 这是「三枚新门各自 begin」在引擎侧的那一半；号不重复那半在
    /// `VoiceInterfaceTests.testThreeFreshGatesTakeThreeDistinctNumbers`。
    #[test]
    fn three_fresh_sessions_begin_in_a_row_are_all_accepted() {
        use std::sync::atomic::Ordering;

        const TAGS: [&str; 3] = ["一", "二", "三"];

        let dir = temp_dir();
        let steps = Arc::new(Mutex::new(Vec::<(&'static str, bool, bool)>::new()));
        let dropped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let shared_steps = Arc::clone(&steps);
        let shared_dropped = Arc::clone(&dropped);
        // 工厂按被叫的次数取标签：三枚会话各有一枚带自己热词的解码器。
        let mut next = 0usize;
        let worker = Worker::spawn(
            dir.to_string_lossy().into_owned(),
            None,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            move |_| {
                let tag = TAGS[next];
                next += 1;
                TaggedDecoder {
                    tag,
                    hotword: tag,
                    steps: Arc::clone(&shared_steps),
                    dropped: Arc::clone(&shared_dropped),
                }
            },
        )
        .expect("线程");

        let mut begins = Vec::new();
        for (index, tag) in TAGS.iter().enumerate() {
            begins.push(worker.begin(index as u64 + 1, Some(tag.to_string())));
        }
        assert_eq!(
            begins,
            vec![0, 0, 0],
            "三枚新门各按自己的号 begin，引擎三次都该接受：{begins:?}"
        );

        for session in 1..=3u64 {
            assert_eq!(worker.feed(session, vec![0; 32_000]), 0);
        }
        assert!(
            engine::wait_until(Duration::from_secs(10), || {
                steps.lock().unwrap().len() == 3
            }),
            "三枚会话都该各自跑出一步：{:?}",
            steps.lock().unwrap()
        );
        for session in 1..=3u64 {
            assert_eq!(
                worker.finish(session),
                0,
                "会话 {session} 应当还在表里，能定稿"
            );
        }

        {
            let steps = steps.lock().unwrap();
            assert_eq!(steps.len(), 6, "三枚各两步：{steps:?}");
            for tag in TAGS {
                let mine: Vec<_> = steps.iter().filter(|(step_tag, _, _)| *step_tag == tag).collect();
                assert_eq!(mine.len(), 2, "{tag} 该有一中一定稿：{steps:?}");
                assert_eq!(
                    mine.iter().map(|(_, final_, seen)| (*final_, *seen)).collect::<Vec<_>>(),
                    vec![(false, true), (true, true)],
                    "{tag} 的两步都该带自己的热词"
                );
            }
        }
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            3,
            "定稿之后三枚解码器都已释放"
        );
        worker.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discard_drops_the_session_so_the_same_number_begins_again() {
        let dir = temp_dir();
        let steps = Arc::new(Mutex::new(Vec::<(&'static str, bool, bool)>::new()));
        let dropped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let shared_steps = Arc::clone(&steps);
        let shared_dropped = Arc::clone(&dropped);
        let mut warm = true;
        let worker = Worker::spawn(
            dir.to_string_lossy().into_owned(),
            None,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            move |_| {
                let (tag, hotword) = if warm {
                    warm = false;
                    ("旧", "旧热词")
                } else {
                    ("新", "新热词")
                };
                TaggedDecoder {
                    tag,
                    hotword,
                    steps: Arc::clone(&shared_steps),
                    dropped: Arc::clone(&shared_dropped),
                }
            },
        )
        .expect("线程");

        assert_eq!(worker.begin(6, Some("旧热词".to_string())), 0);
        assert_eq!(worker.feed(6, vec![0; 32_000]), 0);
        assert!(engine::wait_until(Duration::from_secs(5), || {
            !steps.lock().unwrap().is_empty()
        }));

        // 取消那条路：只丢会话，不请引擎定稿。
        assert_eq!(worker.discard(6), 0);
        assert_eq!(
            dropped.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "丢掉的会话应连同解码器一起释放"
        );
        assert_eq!(worker.finish(6), -1, "丢掉之后这一枚已经不在表里");
        assert_eq!(worker.discard(6), 0, "重复丢弃仍是 0，不是一次失败");

        // 号回来了：同号还能开，而且 `feed` / `finish` 走的是新会话。
        assert_eq!(worker.begin(6, Some("新热词".to_string())), 0);
        assert_eq!(worker.feed(6, vec![0; 32_000]), 0);
        assert_eq!(worker.finish(6), 0);
        let steps = steps.lock().unwrap();
        assert_eq!(
            *steps,
            vec![("旧", false, true), ("新", false, true), ("新", true, true)],
            "旧会话只留下它那一步，丢掉的之后不再参与"
        );
        worker.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 第二段装载的两个时刻：`begin` 之后叫一次（与说话重叠），`finish` 之前再叫
    /// 一次（极短语音也不会拿到空串）。这条不依赖 `metal`：数的是引擎线程上那一步
    /// 被叫了几次，不是它真读了多少字节。
    #[test]
    fn the_second_stage_runs_after_begin_and_again_before_finish() {
        use std::sync::atomic::Ordering;

        /// 不说话的解码器：这条只数第二段被叫了几次。
        struct SilentDecoder;

        impl DecodeText for SilentDecoder {
            fn decode_text(&mut self, _view: &DecodeView<'_>) -> StepOutput {
                StepOutput {
                    text: String::new(),
                    tokens: Vec::new(),
                }
            }
        }

        let dir = temp_dir();
        let before = backend::DEFERRED.load(Ordering::SeqCst);
        let worker = Worker::spawn(
            dir.to_string_lossy().into_owned(),
            None,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            |_| SilentDecoder,
        )
        .expect("线程");
        assert!(worker.begin(11, None) == 0, "会话应当立起来");
        // `begin` 回来之后，引擎线程自己把那一步跑了。
        let after_begin = engine::wait_until(Duration::from_secs(5), || {
            backend::DEFERRED.load(Ordering::SeqCst) > before
        });
        assert!(
            after_begin,
            "`begin` 之后应当叫过一次第二段，实得 {}",
            backend::DEFERRED.load(Ordering::SeqCst) - before
        );
        let after_stage = backend::DEFERRED.load(Ordering::SeqCst);
        assert_eq!(worker.finish(11), 0);
        assert!(
            backend::DEFERRED.load(Ordering::SeqCst) > after_stage,
            "`finish` 之前应当再确认一次第二段"
        );
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

    /// 记录每一步看到的 `finishing` 位。
    struct FlagDecoder {
        flags: Arc<Mutex<Vec<bool>>>,
        finishing: Arc<std::sync::atomic::AtomicBool>,
    }

    impl voice_asr::DecodeText for FlagDecoder {
        fn decode_text(&mut self, view: &voice_asr::DecodeView<'_>) -> voice_asr::StepOutput {
            self.flags
                .lock()
                .unwrap()
                .push(self.finishing.load(std::sync::atomic::Ordering::SeqCst));
            let text = if view.is_final { "终" } else { "半" };
            voice_asr::StepOutput {
                text: text.to_string(),
                tokens: vec![1],
            }
        }
    }

    #[test]
    fn the_decoder_factory_receives_the_finishing_flag() {
        // 松开时引擎线程靠这一位跳过说话中的部分步。这里只核这一位确实交到了
        // 会话的解码器手上，并且 finish 之后回到假。
        let dir = temp_dir();
        let dir_text = dir.to_string_lossy().into_owned();
        let seen = Arc::new(Mutex::new(Vec::<bool>::new()));
        let shared = Arc::clone(&seen);
        let worker = Worker::spawn(
            dir_text,
            None,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            move |finishing| {
                let here = Arc::clone(&shared);
                let flag = Arc::clone(finishing);
                FlagDecoder {
                    flags: here,
                    finishing: flag,
                }
            },
        )
        .expect("线程");
        assert_eq!(worker.begin(3, None), 0);
        assert_eq!(worker.feed(3, vec![0; 32_000]), 0);
        assert!(
            engine::wait_until(Duration::from_secs(5), || !seen.lock().unwrap().is_empty()),
            "满 32000 个样本应让引擎线程走一次非定稿步"
        );
        assert_eq!(seen.lock().unwrap().first().copied(), Some(false));
        assert_eq!(worker.finish(3), 0);
        assert!(
            !worker.flag_value_for_test(),
            "finish 之后这一位应回到假，下一次按住才能继续出部分结果"
        );
    }

    #[test]
    fn finish_failure_reason_reaches_the_json_sink_and_last_error() {
        let dir = temp_dir();
        let dir_text = dir.to_string_lossy().into_owned();
        let worker = Worker::spawn(
            dir_text,
            None,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            |_| FailingDecoder,
        )
        .expect("线程");
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
        let worker = Worker::spawn(
            dir_text,
            None,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            |_| FailingDecoder,
        )
        .expect("线程");
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
