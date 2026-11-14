//! 引擎线程。音频侧只把样本推进有界队列，出队后才调用 `voice-asr`。
//!
//! 两次下发有两个口：C 函数指针（`voice_text_fn`）与一行 JSON（`voice_reply_fn`）。
//! JSON 口装上之后优先用它，文本指针不再被调用；只装文本指针时行为与旧版相同。
//! 定稿失败时两边都带上同一条原因，并把原因留在 `last_error` 里供调用线程同步读取。

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
#[cfg(test)]
use std::time::Duration;

use voice_asr::{DecodeText, Session, TranscriptEvent, TranscriptSink};

use crate::backend::{self, Active};
#[cfg(not(feature = "metal"))]
use crate::decoder::UnlinkedDecoder;

/// 音频队列长度。满了以后 `feed` 不再接受，避免堵在音频线程上。
const PCM_QUEUE_BOUND: usize = 64;

/// `voice_text_fn` 的函数指针。上下文指针由 Swift 持有。
pub(crate) type TextFn = unsafe extern "C" fn(
    session: u64,
    utf8: *const std::os::raw::c_char,
    is_final: i32,
    ctx: *mut std::os::raw::c_void,
);

/// `voice_reply_fn` 的函数指针。收到一行不带换行的 JSON。
pub(crate) type JsonFn =
    unsafe extern "C" fn(json_line: *const std::os::raw::c_char, ctx: *mut std::os::raw::c_void);

/// 当前回调。指针按值传到引擎线程，指针对应的对象仍由调用方保管。
#[derive(Clone, Copy)]
pub(crate) struct Callback {
    pub function: TextFn,
    pub ctx: *mut std::os::raw::c_void,
}

/// 当前的 JSON 口。
#[derive(Clone, Copy)]
pub(crate) struct ReplySink {
    pub function: JsonFn,
    pub ctx: *mut std::os::raw::c_void,
}

/// 两个下发的槽位。JSON 口在位时不走文本指针，一条事件只发一次。
#[derive(Clone, Copy, Default)]
pub(crate) struct Sinks {
    pub text: Option<Callback>,
    pub reply: Option<ReplySink>,
}

// 回调在引擎线程执行。指针本身可以跨线程复制，对象的线程安全由 Swift 侧保证。
unsafe impl Send for Callback {}
unsafe impl Sync for Callback {}
unsafe impl Send for ReplySink {}
unsafe impl Sync for ReplySink {}

enum Command {
    Begin {
        session: u64,
        hotwords: Option<String>,
        reply: mpsc::Sender<i32>,
    },
    Feed {
        session: u64,
        pcm: Vec<i16>,
    },
    Finish {
        session: u64,
        reply: mpsc::Sender<i32>,
    },
    Shutdown,
}

struct SessionSink {
    session: u64,
    sinks: Arc<Mutex<Sinks>>,
}

impl TranscriptSink for SessionSink {
    fn on_transcript(&mut self, event: &TranscriptEvent) {
        let sinks = *lock(&self.sinks);
        let text = event.text.replace('\0', "");
        let kind = if event.is_final { "final" } else { "partial" };
        if let Some(reply) = sinks.reply {
            let line = crate::json_line::reply_line(self.session, &text, kind);
            emit_json(reply, &line);
            return;
        }
        let Some(callback) = sinks.text else {
            return;
        };
        let c_text = std::ffi::CString::new(text)
            .unwrap_or_else(|_| std::ffi::CString::new("").expect("空串"));
        let flag = if event.is_final { 1 } else { 0 };
        unsafe {
            (callback.function)(self.session, c_text.as_ptr(), flag, callback.ctx);
        }
    }
}

/// 定稿失败时的一行。文本口没有位置放原因，所以只走 JSON 口；
/// 没有 JSON 口时靠 `voice_engine_last_error` 同步读原因。
fn emit_failure(sinks: &Arc<Mutex<Sinks>>, session: u64, reason: &str) {
    let sinks = *lock(sinks);
    let Some(reply) = sinks.reply else {
        return;
    };
    let line = crate::json_line::failure_line(session, reason);
    emit_json(reply, &line);
}

fn emit_json(reply: ReplySink, line: &str) {
    let Ok(c_line) = std::ffi::CString::new(line) else {
        return;
    };
    unsafe {
        (reply.function)(c_line.as_ptr(), reply.ctx);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// 进程内的一台引擎。张量对象只活在引擎线程上。
pub(crate) struct Worker {
    tx: SyncSender<Command>,
    join: Mutex<Option<JoinHandle<()>>>,
    sinks: Arc<Mutex<Sinks>>,
    last_error: Arc<Mutex<Option<String>>>,
}

impl Worker {
    /// 启动引擎线程。模型目录必须已经能打开。
    ///
    /// 打开 `metal` 时，权重与梅尔前端在引擎线程起来的时候打开一次，之后的会话
    /// 只借用同一份实例。
    pub(crate) fn start(model_dir: String, metallib: Option<String>) -> std::io::Result<Self> {
        #[cfg(feature = "metal")]
        {
            return Self::spawn(model_dir, metallib, crate::decoder::WeightDecoder::new);
        }
        #[cfg(not(feature = "metal"))]
        {
            Self::spawn(model_dir, metallib, || UnlinkedDecoder)
        }
    }

    /// `factory` 在引擎线程上为每个新会话造一个解码器。
    pub(crate) fn spawn<H, F>(
        model_dir: String,
        metallib: Option<String>,
        factory: F,
    ) -> std::io::Result<Self>
    where
        H: DecodeText + 'static,
        F: FnMut() -> H + Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel(PCM_QUEUE_BOUND);
        let sinks = Arc::new(Mutex::new(Sinks::default()));
        let last_error = Arc::new(Mutex::new(None));
        let shared_sinks = Arc::clone(&sinks);
        let shared_error = Arc::clone(&last_error);
        let (ready_tx, ready_rx) = mpsc::channel();
        let join = thread::Builder::new()
            .name("voice-engine".to_string())
            .spawn(move || {
                let ready = backend::boot(&model_dir, metallib.as_deref());
                if ready_tx.send(ready).is_err() {
                    return;
                }
                if !ready {
                    return;
                }
                thread_main(rx, shared_sinks, shared_error, factory);
            })?;
        match ready_rx.recv() {
            Ok(true) => Ok(Self {
                tx,
                join: Mutex::new(Some(join)),
                sinks,
                last_error,
            }),
            _ => {
                let _ = join.join();
                Err(std::io::Error::other("引擎线程没有准备好"))
            }
        }
    }

    pub(crate) fn set_callback(&self, callback: Option<Callback>) {
        lock(&self.sinks).text = callback;
    }

    pub(crate) fn set_reply_sink(&self, sink: Option<ReplySink>) {
        lock(&self.sinks).reply = sink;
    }

    /// 最近一次 `finish` 失败的原因。成功过或从未失败时为 `None`。
    pub(crate) fn last_failure(&self) -> Option<String> {
        lock(&self.last_error).clone()
    }

    /// 等到引擎线程把会话立起来。0 表示已接受。
    pub(crate) fn begin(&self, session: u64, hotwords: Option<String>) -> i32 {
        let (reply_tx, reply_rx) = mpsc::channel();
        if self
            .tx
            .send(Command::Begin {
                session,
                hotwords,
                reply: reply_tx,
            })
            .is_err()
        {
            return -1;
        }
        reply_rx.recv().unwrap_or(-1)
    }

    /// 只入队。队列满或引擎已停时返回非 0，不在调用线程上跑识别。
    pub(crate) fn feed(&self, session: u64, pcm: Vec<i16>) -> i32 {
        match self.tx.try_send(Command::Feed { session, pcm }) {
            Ok(()) => 0,
            Err(TrySendError::Full(_)) => 1,
            Err(TrySendError::Disconnected(_)) => -1,
        }
    }

    /// 等到引擎线程调用 `voice-asr` 的 `finish`。0 表示已接受。
    pub(crate) fn finish(&self, session: u64) -> i32 {
        let (reply_tx, reply_rx) = mpsc::channel();
        if self
            .tx
            .send(Command::Finish {
                session,
                reply: reply_tx,
            })
            .is_err()
        {
            return -1;
        }
        reply_rx.recv().unwrap_or(-1)
    }

    pub(crate) fn shutdown(&self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(handle) = lock(&self.join).take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn thread_main<H, F>(
    rx: Receiver<Command>,
    sinks: Arc<Mutex<Sinks>>,
    last_error: Arc<Mutex<Option<String>>>,
    mut factory: F,
) where
    H: DecodeText,
    F: FnMut() -> H,
{
    let mut sessions: HashMap<u64, Session<Active, H, SessionSink>> = HashMap::new();
    loop {
        let command = match rx.recv() {
            Ok(command) => command,
            Err(_) => break,
        };
        if matches!(command, Command::Shutdown) {
            break;
        }
        let sinks = &sinks;
        let last_error = &last_error;
        let factory = &mut factory;
        let _ = panic::catch_unwind(AssertUnwindSafe(|| {
            drive(&mut sessions, command, sinks, last_error, factory);
        }));
    }
}

/// 出队后的一次调用。`feed` 与 `finish` 都进 `voice-asr`。
fn drive<H, F>(
    sessions: &mut HashMap<u64, Session<Active, H, SessionSink>>,
    command: Command,
    sinks: &Arc<Mutex<Sinks>>,
    last_error: &Arc<Mutex<Option<String>>>,
    factory: &mut F,
) where
    H: DecodeText,
    F: FnMut() -> H,
{
    match command {
        Command::Begin {
            session,
            hotwords,
            reply,
        } => {
            if sessions.contains_key(&session) {
                let _ = reply.send(-1);
                return;
            }
            let sink = SessionSink {
                session,
                sinks: Arc::clone(sinks),
            };
            let Ok(ops) = backend::new_backend() else {
                let _ = reply.send(-1);
                return;
            };
            match Session::new(hotwords.as_deref(), ops, factory(), sink) {
                Ok(created) => {
                    sessions.insert(session, created);
                    let _ = reply.send(0);
                }
                Err(_) => {
                    let _ = reply.send(-1);
                }
            }
        }
        Command::Feed { session, pcm } => {
            if let Some(slot) = sessions.get_mut(&session) {
                let _ = slot.feed(&pcm);
            }
        }
        Command::Finish { session, reply } => {
            let Some(mut slot) = sessions.remove(&session) else {
                *lock(last_error) = Some("这个会话不在引擎线程上".to_string());
                let _ = reply.send(-1);
                return;
            };
            let reason = match slot.finish() {
                Ok(()) => slot.decoder().failure_reason(),
                Err(error) => Some(format!("定稿步失败：{error}")),
            };
            if let Some(reason) = &reason {
                emit_failure(sinks, session, reason);
            }
            *lock(last_error) = reason.clone();
            let _ = reply.send(if reason.is_none() { 0 } else { -1 });
        }
        Command::Shutdown => {}
    }
}

/// 测试里等回调。
#[cfg(test)]
pub(crate) fn wait_until(timeout: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if ready() {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    ready()
}
