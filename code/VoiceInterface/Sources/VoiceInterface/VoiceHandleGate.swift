import Foundation

/// 插在输入法 `handle` 进入 `processKey` 之前。返回 true 表示这次事件已取走。
public final class VoiceHandleGate {
    private let lock = NSLock()
    private var machine: VoiceMachine
    private let transport: RecognitionTransport
    private let capture: PCMSource
    private weak var host: VoiceFieldHost?
    private let callbackQueue: DispatchQueue
    /// 看门狗窗口：采集开始之后这么久没有松手、也没有任何识别回复，就当这一路死了。
    private let watchdogSeconds: TimeInterval
    /// 看门狗的计时不走主线程。主线程卡住正是它要治的一种情况，计时器不能跟着一起卡。
    private let watchdogQueue: DispatchQueue
    /// 每起一次表就加一，到点的那一枚先比对号再看会话：旧表不会误停新会话。
    private var watchdogGeneration: UInt64 = 0
    /// 同一句拒绝只写一次。换一句再写。
    private var loggedRejection: String?

    public init(
        configuredKey: UInt16?,
        hotwords: [String]?,
        host: VoiceFieldHost,
        transport: RecognitionTransport,
        capture: PCMSource,
        numbers: VoiceSessionNumbers = .shared,
        callbackQueue: DispatchQueue = .main,
        watchdogSeconds: TimeInterval = VoiceHandleGate.defaultWatchdogSeconds,
        watchdogQueue: DispatchQueue = DispatchQueue(label: "rime.voice.watchdog")
    ) {
        self.machine = VoiceMachine(configuredKey: configuredKey, hotwords: hotwords, numbers: numbers)
        self.host = host
        self.transport = transport
        self.capture = capture
        self.callbackQueue = callbackQueue
        self.watchdogSeconds = watchdogSeconds
        self.watchdogQueue = watchdogQueue
    }

    /// 扬声器松开的那一下通常在两秒内，一句长话也很少超过三十秒。六十秒是四倍余量：
    /// 正常按住不会误停，真断了也就多录一分钟。
    public static let defaultWatchdogSeconds: TimeInterval = 60

    public func consume(_ event: VoiceKeyEvent) -> Bool {
        lock.lock()
        guard let host else {
            lock.unlock()
            return false
        }
        let snapshot = host.voiceSnapshot()
        machine.noteClient(snapshot)
        let (consumed, effects) = machine.handle(event, host: snapshot)
        lock.unlock()
        guard consumed else { return false }
        perform(effects)
        return true
    }

    public func push(_ frame: AudioFrame) {
        guard frame.encoding == "pcm_s16le", frame.sampleRate == 16_000, frame.channelCount == 1 else { return }
        lock.lock()
        let sessionID = machine.session?.id
        let open = machine.session?.life == .open && machine.session?.closeRequested == false
        lock.unlock()
        guard let sessionID, open else { return }
        transport.send(.feed(sessionID: sessionID, pcm: frame.bytes))
    }

    public func receive(_ reply: RecognitionReply) {
        callbackQueue.async { [weak self] in
            self?.applyReply(reply)
        }
    }

    /// 从外部取消当前会话。采集是这里停的：不看有没有收到过松手事件。
    /// 没有开着的会话时不再停采集、也不再写日志，所以可以重复调用。
    /// 返回 true 表示这次确实关掉了一枚开着的会话。
    @discardableResult
    public func abandon(reason: VoiceAbandonReason) -> Bool {
        lock.lock()
        let sessionID: UInt64? = machine.session.flatMap { $0.life == .open ? $0.id : nil }
        _ = machine.abandon(reason: reason)
        lock.unlock()
        // 会话都关掉了，留着的那枚表也没事可做。
        disarmWatchdog()
        guard let sessionID else { return false }
        // 先停采集再写日志：日志那一行意味着麦克风已经不往里报了。
        // 机器返回的效果在这里故意不执行：取消不碰文本框，一条 show / commit 也不发。
        capture.stop()
        // 取消不发 `finish`，引擎侧那一枚会话本来就没人收场：让它把解码器与状态释放掉，
        // 号也就能再用。老引擎没有这个符号时这一步什么都不发，不报错。
        transport.send(.discard(sessionID: sessionID))
        VoiceLog.write("voice session \(sessionID) abandoned \(reason.label)")
        return true
    }

    /// 只读：当前会话的生命期。取消之后是 `.closed`，下一次按下才换新的。
    public var sessionLife: VoiceLife? {
        lock.lock()
        defer { lock.unlock() }
        return machine.session?.life
    }

    /// 只读：当前（或最近一枚）会话的号。撞号时先看这个，再看日志里的 `begin` / `abandoned`。
    public var sessionID: UInt64? {
        lock.lock()
        defer { lock.unlock() }
        return machine.session?.id
    }

    private func perform(_ effects: [VoiceEffect]) {
        for effect in effects {
            switch effect {
            case .startCapture(let sessionID, let context):
                transport.send(.start(sessionID: sessionID, context: context))
                // 号与来源一起写进去：撞号时「哪个号、哪个客户端」一行就能对上。
                VoiceLog.write("voice session \(sessionID) begin bundle=\(host?.voiceBundleIdentifier ?? "-")")
                capture.start { [weak self] frame in
                    self?.push(frame)
                }
                armWatchdog(sessionID)
            case .stopCapture(let sessionID):
                disarmWatchdog()
                capture.stop()
                transport.send(.finish(sessionID: sessionID))
            case .show(_, let text, let caret):
                host?.voiceShow(text: text, caretUTF16: caret)
            case .commit(_, let text):
                host?.voiceCommit(text)
            }
        }
    }

    /// 起表。每次调用都换一个号，先前那一枚到点时会自己作废。
    private func armWatchdog(_ sessionID: UInt64) {
        guard watchdogSeconds > 0 else { return }
        lock.lock()
        watchdogGeneration &+= 1
        let generation = watchdogGeneration
        lock.unlock()
        watchdogQueue.asyncAfter(deadline: .now() + watchdogSeconds) { [weak self] in
            self?.watchdogDeadline(sessionID: sessionID, generation: generation)
        }
    }

    /// 撤表。松手、会话关掉、引擎回了话都算这一路还活着。
    private func disarmWatchdog() {
        lock.lock()
        watchdogGeneration &+= 1
        lock.unlock()
    }

    private func watchdogDeadline(sessionID: UInt64, generation: UInt64) {
        lock.lock()
        let stillArmed = generation == watchdogGeneration
        // 只有「还开着、还没人松手」的那一枚会话可以被看门狗停掉。
        let silent = machine.session.map { current in
            current.id == sessionID && current.life == .open && !current.closeRequested
        } ?? false
        lock.unlock()
        guard stillArmed, silent else { return }
        abandon(reason: .watchdog)
    }

    /// 引擎回了话：这一路还活着，窗口从这一枚回复重新起算。
    /// 会话已经关掉、或者已经有人松过手时反而撤表：没有采集可停了。
    private func noteReply(_ sessionID: UInt64?) {
        lock.lock()
        let listening = machine.session.map { current in
            current.life == .open && !current.closeRequested && current.id == sessionID
        } ?? false
        lock.unlock()
        if listening, let sessionID {
            armWatchdog(sessionID)
        } else {
            disarmWatchdog()
        }
    }

    /// 带号的日志行。拿不到号时退回不带号的老形状，宁可少一点信息也不写一个假号。
    private func label(sessionID: UInt64?, _ rest: String) -> String {
        guard let sessionID else { return rest }
        return "voice session \(sessionID) \(rest)"
    }

    private func applyReply(_ reply: RecognitionReply) {
        guard let host else { return }
        guard reply.ok, let sessionID = reply.sessionID else {
            let message = reply.message ?? ""
            if loggedRejection != message {
                loggedRejection = message
                // 号在前：撞号时这一行与上一条 `begin` 能一眼对上。
                VoiceLog.write(label(sessionID: reply.sessionID, "rejected \(message)"))
            }
            guard let sessionID = reply.sessionID else { return }
            lock.lock()
            let snapshot = host.voiceSnapshot()
            machine.noteClient(snapshot)
            let effects = machine.abandon(sessionID: sessionID, host: snapshot)
            lock.unlock()
            perform(effects)
            noteReply(sessionID)
            return
        }
        lock.lock()
        let snapshot = host.voiceSnapshot()
        machine.noteClient(snapshot)
        let effects: [VoiceEffect]
        // 定稿只有落到「此刻还开着的那一枚」上才算数；迟到的、号不对的都记成没落到。
        var settledAsFinal = false
        if reply.kind == "final" {
            settledAsFinal = machine.session.map { $0.id == sessionID && $0.life == .open } ?? false
            effects = machine.acceptFinal(sessionID: sessionID, text: reply.text ?? "", host: snapshot)
        } else if reply.decoded, let text = reply.text {
            effects = machine.acceptPartial(sessionID: sessionID, text: text, host: snapshot)
        } else {
            effects = []
        }
        lock.unlock()
        if settledAsFinal {
            VoiceLog.write("voice session \(sessionID) final")
        }
        perform(effects)
        noteReply(sessionID)
    }
}
