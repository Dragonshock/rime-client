import Darwin
import Foundation

/// 进程内识别入口。调用 `voice_engine.h` 里的 C 符号。
///
/// 静态库未链入进程时返回「引擎未链入」。不启动解释器进程。
/// 回复走一行 JSON，由 `RecognitionReplyParser` 解析，与解释器那条路同一个形状。
/// 定稿失败时把引擎给出的原因读出来，交给 `VoiceHandleGate` 写进 `voice.log`。
public final class VoiceEngine: RecognitionTransport {
    public static let notLinkedMessage = "引擎未链入"
    /// 引擎替换掉同号旧会话时那一行回复的 `kind`。它不是回包，只进日志。
    static let replacedKind = "replaced"

    /// 未链入或模型目录打不开时的原因。引擎已建立则为 nil。
    public let unavailableReason: String?

    private let symbols: VoiceSymbols?
    private var engine: UnsafeMutableRawPointer?
    private var callback: UnsafeMutableRawPointer?
    private let onReply: (RecognitionReply) -> Void

    /// 符号不在进程里时返回「引擎未链入」，否则返回 nil。
    public static func linkStatus() -> String? {
        VoiceSymbols.load() == nil ? notLinkedMessage : nil
    }

    /// `Bundle.main` 里的 0.6B 目录。资源缺失时仍返回包内应有的路径。
    public static func bundledModelDirectory() -> String {
        if let url = Bundle.main.url(
            forResource: "Qwen3-ASR-0.6B-4bit",
            withExtension: nil,
            subdirectory: "Models"
        ) {
            return url.path
        }
        let base = Bundle.main.resourceURL ?? Bundle.main.bundleURL
        return base
            .appendingPathComponent("Models", isDirectory: true)
            .appendingPathComponent("Qwen3-ASR-0.6B-4bit", isDirectory: true)
            .path
    }

    /// `Bundle.main` 里的 metallib。文件缺失时仍返回包内应有的路径。
    public static func bundledMetallib() -> String {
        if let url = Bundle.main.url(forResource: "mlx", withExtension: "metallib") {
            return url.path
        }
        let base = Bundle.main.resourceURL ?? Bundle.main.bundleURL
        return base.appendingPathComponent("mlx.metallib").path
    }

    public init(
        modelDirectory: String,
        metallibPath: String,
        onReply: @escaping (RecognitionReply) -> Void
    ) {
        self.onReply = onReply
        guard let symbols = VoiceSymbols.load() else {
            self.symbols = nil
            self.engine = nil
            self.callback = nil
            self.unavailableReason = Self.notLinkedMessage
            return
        }
        self.symbols = symbols
        let box = CallbackBox(onReply: onReply)
        let opaque = Unmanaged.passRetained(box).toOpaque()
        let created: UnsafeMutableRawPointer? = modelDirectory.withCString { model in
            if metallibPath.isEmpty {
                return symbols.create(model, nil)
            }
            return metallibPath.withCString { metal in
                symbols.create(model, metal)
            }
        }
        guard let created else {
            self.engine = nil
            self.callback = nil
            self.unavailableReason = "模型目录打不开"
            _ = Unmanaged<CallbackBox>.fromOpaque(opaque).takeRetainedValue()
            return
        }
        if let setReplySink = symbols.setReplySink {
            setReplySink(created, voiceReplyTrampoline, opaque)
        } else {
            symbols.setCallback(created, voiceTextTrampoline, opaque)
        }
        self.engine = created
        self.callback = opaque
        self.unavailableReason = nil
    }

    deinit {
        if let engine, let symbols {
            symbols.destroy(engine)
        }
        if let callback {
            _ = Unmanaged<CallbackBox>.fromOpaque(callback).takeRetainedValue()
        }
    }

    public func send(_ command: RecognitionCommand) {
        if let unavailableReason {
            // 丢弃不是一次识别请求，不回包：引擎没起来这件事已经由 `begin` 那一行说过了，
            // 取消时再补一行 `rejected` 只会把日志读歪。
            if case .discard = command { return }
            onReply(RecognitionReply(
                sessionID: sessionID(of: command),
                text: nil,
                decoded: false,
                kind: nil,
                ok: false,
                message: unavailableReason
            ))
            return
        }
        switch command {
        case .start(let sessionID, let context):
            let code = begin(session: sessionID, hotwords: context)
            if code != 0 {
                reject(sessionID, lastErrorReason() ?? "会话未被接受")
            }
        case .feed(let sessionID, let pcm):
            let code = feed(session: sessionID, pcm: pcm)
            if code != 0 {
                VoiceLog.write("feed 未被接受")
            }
        case .finish(let sessionID):
            let code = finish(session: sessionID)
            if code != 0 {
                reject(sessionID, lastErrorReason() ?? "定稿未被接受")
            }
        case .discard(let sessionID):
            discard(session: sessionID)
        case .stop:
            stop()
        }
    }

    /// 不释放引擎。按下与松开仍走 begin 与 finish。
    public func stop() {}

    private func begin(session: UInt64, hotwords: String?) -> Int32 {
        guard let symbols, let engine else { return -1 }
        if let hotwords {
            return hotwords.withCString { symbols.begin(engine, session, $0) }
        }
        return symbols.begin(engine, session, nil)
    }

    private func feed(session: UInt64, pcm: Data) -> Int32 {
        guard let symbols, let engine else { return -1 }
        let count = pcm.count / MemoryLayout<Int16>.size
        if count == 0 {
            return symbols.feed(engine, session, nil, 0)
        }
        return pcm.withUnsafeBytes { raw in
            guard let address = raw.bindMemory(to: Int16.self).baseAddress else { return -1 }
            return symbols.feed(engine, session, address, count)
        }
    }

    private func finish(session: UInt64) -> Int32 {
        guard let symbols, let engine else { return -1 }
        return symbols.finish(engine, session)
    }

    /// 取消时让引擎也把那一枚会话丢掉（解码器与状态一起释放，号随即可以再用）。
    ///
    /// 老归档里没有 `voice_session_discard` 这个符号：那就一条不发，也不报错——
    /// 那时的行为就是现状（引擎表里留一枚挂着不用的会话）。
    private func discard(session: UInt64) {
        guard let symbols, let engine, let discard = symbols.discard else { return }
        _ = discard(engine, session)
    }

    private func reject(_ sessionID: UInt64, _ message: String) {
        onReply(RecognitionReply(
            sessionID: sessionID,
            text: nil,
            decoded: false,
            kind: nil,
            ok: false,
            message: message
        ))
    }

    /// 引擎线程给出的一行原因。没有原因时返回 nil，调用方用固定文案。
    private func lastErrorReason() -> String? {
        guard let symbols, let engine, let lastError = symbols.lastError else { return nil }
        var buffer = [CChar](repeating: 0, count: 1024)
        let written = buffer.withUnsafeMutableBufferPointer { pointer in
            lastError(engine, pointer.baseAddress, pointer.count)
        }
        guard written > 0, Int(written) < buffer.count else { return nil }
        let bytes = buffer[..<Int(written)].map { UInt8(bitPattern: $0) }
        return String(decoding: bytes, as: UTF8.self)
    }

    private func sessionID(of command: RecognitionCommand) -> UInt64? {
        switch command {
        case .start(let sessionID, _), .feed(let sessionID, _), .finish(let sessionID), .discard(let sessionID):
            return sessionID
        case .stop:
            return nil
        }
    }
}

private final class CallbackBox {
    let onReply: (RecognitionReply) -> Void

    init(onReply: @escaping (RecognitionReply) -> Void) {
        self.onReply = onReply
    }
}

private let voiceTextTrampoline: VoiceSymbols.TextFn = { session, utf8, isFinal, raw in
    guard let raw else { return }
    let box = Unmanaged<CallbackBox>.fromOpaque(raw).takeUnretainedValue()
    let text = utf8.map { String(cString: $0) } ?? ""
    box.onReply(RecognitionReply(
        sessionID: session,
        text: text,
        decoded: true,
        kind: isFinal != 0 ? "final" : "partial",
        ok: true,
        message: nil
    ))
}

/// 引擎线程给出的一行 JSON。解析仍是 `RecognitionReplyParser` 那一处。
private let voiceReplyTrampoline: VoiceSymbols.ReplyFn = { line, raw in
    guard let raw, let line else { return }
    let box = Unmanaged<CallbackBox>.fromOpaque(raw).takeUnretainedValue()
    guard let reply = RecognitionReplyParser.parse(String(cString: line)) else { return }
    // 引擎把同号的旧会话替换掉时的一行。它不是回包，不派给门：只进日志，
    // 号在前，与 begin / abandoned / final / rejected 四种行对齐。
    if reply.kind == VoiceEngine.replacedKind {
        let number = reply.sessionID.map(String.init) ?? "-"
        VoiceLog.write("voice session \(number) replaced \(reply.message ?? "-")")
        return
    }
    box.onReply(reply)
}

private struct VoiceSymbols {
    typealias Create = @convention(c) (
        UnsafePointer<CChar>?,
        UnsafePointer<CChar>?
    ) -> UnsafeMutableRawPointer?
    typealias Destroy = @convention(c) (UnsafeMutableRawPointer?) -> Void
    typealias Begin = @convention(c) (
        UnsafeMutableRawPointer?,
        UInt64,
        UnsafePointer<CChar>?
    ) -> Int32
    typealias Feed = @convention(c) (
        UnsafeMutableRawPointer?,
        UInt64,
        UnsafePointer<Int16>?,
        Int
    ) -> Int32
    typealias Finish = @convention(c) (UnsafeMutableRawPointer?, UInt64) -> Int32
    /// 老归档没有这一枚。没有就退回现状：取消时只停采集，不告诉引擎。
    typealias Discard = @convention(c) (UnsafeMutableRawPointer?, UInt64) -> Int32
    typealias TextFn = @convention(c) (
        UInt64,
        UnsafePointer<CChar>?,
        Int32,
        UnsafeMutableRawPointer?
    ) -> Void
    typealias ReplyFn = @convention(c) (
        UnsafePointer<CChar>?,
        UnsafeMutableRawPointer?
    ) -> Void
    typealias SetCallback = @convention(c) (
        UnsafeMutableRawPointer?,
        TextFn?,
        UnsafeMutableRawPointer?
    ) -> Void
    typealias SetReplySink = @convention(c) (
        UnsafeMutableRawPointer?,
        ReplyFn?,
        UnsafeMutableRawPointer?
    ) -> Void
    typealias LastError = @convention(c) (
        UnsafeMutableRawPointer?,
        UnsafeMutablePointer<CChar>?,
        Int
    ) -> Int32

    var create: Create
    var destroy: Destroy
    var begin: Begin
    var feed: Feed
    var finish: Finish
    var discard: Discard?
    var setCallback: SetCallback
    /// 旧归档没有这一对。缺了就退回文本指针，形状仍由本侧补齐。
    var setReplySink: SetReplySink?
    var lastError: LastError?

    static func load() -> VoiceSymbols? {
        guard let create: Create = symbol("voice_engine_new"),
              let destroy: Destroy = symbol("voice_engine_free"),
              let begin: Begin = symbol("voice_session_begin"),
              let feed: Feed = symbol("voice_session_feed"),
              let finish: Finish = symbol("voice_session_finish"),
              let setCallback: SetCallback = symbol("voice_engine_set_callback")
        else { return nil }
        return VoiceSymbols(
            create: create,
            destroy: destroy,
            begin: begin,
            feed: feed,
            finish: finish,
            discard: symbol("voice_session_discard"),
            setCallback: setCallback,
            setReplySink: symbol("voice_engine_set_reply_sink"),
            lastError: symbol("voice_engine_last_error")
        )
    }
}

/// `RTLD_DEFAULT`。只查找已经链进本进程的符号。
private func symbol<T>(_ name: String) -> T? {
    guard let pointer = name.withCString({ dlsym(UnsafeMutableRawPointer(bitPattern: -2), $0) }) else {
        return nil
    }
    return unsafeBitCast(pointer, to: T.self)
}
