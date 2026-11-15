import Foundation

/// 进程启动时把语音引擎建好，第一次按键不再掏五秒去读权重。
///
/// 三件事写死在这里：
/// 1. `start` 只在启动序列里调一次，跑在后台队列上；失败只写一行 `voice.log`，
///    不抛、不崩、不挡窗口服务器。
/// 2. 按键那一侧不自己建引擎，只用 `VoiceEnginePreheatedTransport` 包一层。
/// 3. 预热没跑完时按键有上限地等（`keyPressBudget`），到点就发一行拒绝并收尾，
///    不无声挂住主线程，也不会把引擎建第二遍（权重会变成两份）。
///
/// 回复落点只有一个：最后 `route` 进来的那个闭包。
public final class VoiceEngineWarmup {
    public enum Status: Equatable {
        /// 还没调 `start`。非 App 启动路径（单测、其它宿主）会停在这里。
        case idle
        /// 后台正在建。
        case warming
        case ready
        /// 建了但引擎给不出原因，把原因留下。
        case failed(String)
    }

    public static let shared = VoiceEngineWarmup()

    /// 按键在预热未完成时最多等这么久。到点就拒，不把主线程挂住。
    public static let keyPressBudget: TimeInterval = 2.0

    private let condition = NSCondition()
    private var state: Status = .idle
    /// 建完之后一直留着，失败也是（失败的那个引擎自己会回一行原因）。
    private var built: VoiceEngine?
    private var buildSeconds: TimeInterval?
    private var sink: ((RecognitionReply) -> Void)?

    init() {}

    public var status: Status {
        condition.lock()
        defer { condition.unlock() }
        return state
    }

    /// `voice_engine_new` 的墙钟。没建完是 nil。
    public var engineSeconds: TimeInterval? {
        condition.lock()
        defer { condition.unlock() }
        return buildSeconds
    }

    /// 一行字。日志与拒绝原因共用，免得两处口径不一。
    public var summary: String {
        condition.lock()
        defer { condition.unlock() }
        return Self.describe(state, seconds: buildSeconds)
    }

    /// 装回复落点。预热之前、之中、之后都能调，换了就换了。
    public func route(_ sink: ((RecognitionReply) -> Void)?) {
        condition.lock()
        self.sink = sink
        condition.unlock()
    }

    /// 后台建。已经建过或正在建时返回 false，不重复建。
    @discardableResult
    public func start(
        modelDirectory: String,
        metallibPath: String,
        queue: DispatchQueue = DispatchQueue(label: "voice.engine.warmup", qos: .userInitiated)
    ) -> Bool {
        condition.lock()
        guard state == .idle else {
            condition.unlock()
            return false
        }
        state = .warming
        condition.unlock()

        queue.async { [self] in
            let clock = Date()
            let engine = VoiceEngine(
                modelDirectory: modelDirectory,
                metallibPath: metallibPath,
                onReply: { [weak self] reply in
                    self?.forward(reply)
                }
            )
            let seconds = Date().timeIntervalSince(clock)

            condition.lock()
            built = engine
            buildSeconds = seconds
            if let reason = engine.unavailableReason {
                state = .failed(reason)
            } else {
                state = .ready
            }
            let line = Self.describe(state, seconds: seconds)
            condition.broadcast()
            condition.unlock()

            VoiceLog.write("预热 \(line)")
        }
        return true
    }

    /// 有上限地等引擎。等到就返回（失败的那个也返回，它自己会回原因）；
    /// 还没开始、或到点还没建完，返回 nil。
    public func engineOrWait(_ budget: TimeInterval) -> VoiceEngine? {
        let deadline = Date().addingTimeInterval(max(0, budget))
        condition.lock()
        defer { condition.unlock() }
        while built == nil {
            if state == .idle {
                return nil
            }
            if !condition.wait(until: deadline) {
                return nil
            }
        }
        return built
    }

    /// 不等。只有预热完成才给，预热中与预热失败都给 nil。
    public func engineIfReady() -> VoiceEngine? {
        condition.lock()
        defer { condition.unlock() }
        return state == .ready ? built : nil
    }

    /// 一直等到建完，给单测与探针用（产品路径不用它）。
    public func waitUntilBuilt() -> VoiceEngine? {
        condition.lock()
        defer { condition.unlock() }
        while built == nil {
            if state == .idle {
                return nil
            }
            condition.wait()
        }
        return built
    }

    private func forward(_ reply: RecognitionReply) {
        condition.lock()
        let sink = self.sink
        condition.unlock()
        sink?(reply)
    }

    static func describe(_ status: Status, seconds: TimeInterval?) -> String {
        switch status {
        case .idle:
            return "预热未开始"
        case .warming:
            return "预热中"
        case .ready:
            guard let seconds else { return "预热完成" }
            return String(format: "预热完成 引擎 %.2f 秒", seconds)
        case .failed(let reason):
            guard let seconds else { return "预热失败（\(reason)）" }
            return String(format: "预热失败（%@）引擎 %.2f 秒", reason, seconds)
        }
    }
}

/// 预热引擎的传输壳。按键到达时预热可能还没跑完。
///
/// - `.start`：按 `budget` 有上限地等。等到了就交出去；没等到就发一行拒绝，
///   让 `VoiceHandleGate` 那边把会话收尾——有声地失败，不无声挂住。
/// - `.feed` / `.finish` / `.stop`：一次也不等。引擎没好之前直接丢掉，
///   那次会话已经由那行拒绝收尾了。
public final class VoiceEnginePreheatedTransport: RecognitionTransport {
    private let warmup: VoiceEngineWarmup
    private let budget: TimeInterval
    private let onReply: (RecognitionReply) -> Void

    public init(
        warmup: VoiceEngineWarmup = VoiceEngineWarmup.shared,
        budget: TimeInterval = VoiceEngineWarmup.keyPressBudget,
        onReply: @escaping (RecognitionReply) -> Void
    ) {
        self.warmup = warmup
        self.budget = budget
        self.onReply = onReply
    }

    public func send(_ command: RecognitionCommand) {
        switch command {
        case .start(let sessionID, _):
            let clock = Date()
            guard let engine = warmup.engineOrWait(budget) else {
                let waited = Date().timeIntervalSince(clock)
                let message = String(
                    format: "引擎未就绪（等了 %.2f 秒，%@）",
                    waited,
                    warmup.summary
                )
                VoiceLog.write("首键拒绝 \(message)")
                onReply(RecognitionReply(
                    sessionID: sessionID,
                    text: nil,
                    decoded: false,
                    kind: nil,
                    ok: false,
                    message: message
                ))
                return
            }
            let waited = Date().timeIntervalSince(clock)
            if waited >= 0.01 {
                VoiceLog.write(String(format: "首键等了 %.2f 秒，%@", waited, warmup.summary))
            }
            engine.send(command)
        case .feed, .finish, .stop:
            warmup.engineIfReady()?.send(command)
        }
    }

    public func stop() {
        warmup.engineIfReady()?.stop()
    }
}

public extension VoiceEngineWarmup.Status {
    /// 预热失败时给出引擎的原因，其它状态为 nil。
    var failureReason: String? {
        if case .failed(let reason) = self {
            return reason
        }
        return nil
    }
}
