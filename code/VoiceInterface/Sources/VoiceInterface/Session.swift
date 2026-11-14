import Foundation

public enum VoiceLife: Equatable, Sendable {
    case absent
    case open
    case closed
}

public struct VoiceSession: Equatable, Sendable {
    public var id: UInt64
    public var bundleIdentifier: String?
    public var life: VoiceLife
    public var closeRequested: Bool
    public var deliveryAllowed: Bool
    public var lastShown: String?
    public var recognition: RecognitionRecord?
    public var rewrite: RewriteRecord?

    public static func opened(id: UInt64, bundleIdentifier: String?) -> VoiceSession {
        VoiceSession(
            id: id,
            bundleIdentifier: bundleIdentifier,
            life: .open,
            closeRequested: false,
            deliveryAllowed: true,
            lastShown: nil,
            recognition: nil,
            rewrite: nil
        )
    }
}

public enum VoiceEffect: Equatable, Sendable {
    case startCapture(UInt64, context: String?)
    case stopCapture(UInt64)
    case show(sessionID: UInt64, text: String, caretUTF16: Int)
    case commit(sessionID: UInt64, text: String)
}

public struct VoiceMachine {
    public private(set) var session: VoiceSession?
    public private(set) var nextID: UInt64
    public var configuredKey: UInt16?
    public var hotwords: [String]?

    public init(configuredKey: UInt16? = nil, hotwords: [String]? = nil) {
        self.session = nil
        self.nextID = 1
        self.configuredKey = configuredKey
        self.hotwords = hotwords
    }

    public mutating func handle(
        _ event: VoiceKeyEvent,
        host: HostSnapshot
    ) -> (consumed: Bool, effects: [VoiceEffect]) {
        switch VoiceTrigger.decide(configuredKey: configuredKey, event: event) {
        case .pass:
            return (false, [])
        case .takePress:
            return (true, press(host: host))
        case .takeRelease:
            return (true, release())
        }
    }

    public mutating func noteClient(_ host: HostSnapshot) {
        guard var current = session, current.life == .open else { return }
        if host.clientMissing || host.bundleIdentifier != current.bundleIdentifier {
            current.deliveryAllowed = false
            session = current
        }
    }

    public mutating func acceptPartial(sessionID: UInt64, text: String, host: HostSnapshot) -> [VoiceEffect] {
        guard var current = session, current.life == .open, current.id == sessionID, !current.closeRequested else {
            return []
        }
        if let previous = current.recognition, previous.kind == .partial, previous.text == text {
            return []
        }
        current.recognition = RecognitionRecord(sessionID: sessionID, kind: .partial, text: text)
        session = current
        return deliver(VoiceIntent(sessionID: sessionID, text: text, kind: .preedit), host: host)
    }

    public mutating func acceptFinal(sessionID: UInt64, text: String, host: HostSnapshot) -> [VoiceEffect] {
        guard var current = session, current.life == .open, current.id == sessionID, current.closeRequested else {
            return []
        }
        current.recognition = RecognitionRecord(sessionID: sessionID, kind: .finalText, text: text)
        if text.isEmpty {
            session = current
            var effects: [VoiceEffect] = []
            if shouldClearVoiceCache(current, host: host) {
                current.lastShown = nil
                session = current
                effects.append(.show(sessionID: sessionID, text: "", caretUTF16: 0))
            }
            current.life = .closed
            session = current
            return effects
        }
        current.rewrite = RewriteAdapter.record(sessionID: sessionID, finalText: text)
        session = current
        let effects = deliver(VoiceIntent(sessionID: sessionID, text: text, kind: .commit), host: host)
        if var done = session, done.id == sessionID {
            done.life = .closed
            session = done
        }
        return effects
    }

    /// 引擎拒绝了这一次。会话就此关闭，下一次按下才能开新会话；
    /// 已经写出去的预编辑在仍是自己那一串时才清掉。文本不提交。
    public mutating func abandon(sessionID: UInt64, host: HostSnapshot) -> [VoiceEffect] {
        guard var current = session, current.life == .open, current.id == sessionID else {
            return []
        }
        var effects: [VoiceEffect] = []
        if shouldClearVoiceCache(current, host: host) {
            current.lastShown = nil
            effects.append(.show(sessionID: sessionID, text: "", caretUTF16: 0))
        }
        current.life = .closed
        session = current
        return effects
    }

    private func shouldClearVoiceCache(_ current: VoiceSession, host: HostSnapshot) -> Bool {
        guard current.deliveryAllowed, !host.clientMissing, host.bundleIdentifier == current.bundleIdentifier else {
            return false
        }
        guard let shown = current.lastShown, !shown.isEmpty else { return false }
        return host.preedit == shown
    }

    private mutating func press(host: HostSnapshot) -> [VoiceEffect] {
        if let current = session, current.life == .open { return [] }
        guard !host.clientMissing else { return [] }
        let id = nextID
        nextID += 1
        session = .opened(id: id, bundleIdentifier: host.bundleIdentifier)
        return [.startCapture(id, context: HotwordString.project(hotwords))]
    }

    private mutating func release() -> [VoiceEffect] {
        guard var current = session, current.life == .open, !current.closeRequested else { return [] }
        current.closeRequested = true
        session = current
        return [.stopCapture(current.id)]
    }

    private mutating func deliver(_ intent: VoiceIntent, host: HostSnapshot) -> [VoiceEffect] {
        guard var current = session else { return [] }
        guard intent.sessionID == current.id, current.life == .open else { return [] }
        guard current.deliveryAllowed, !host.clientMissing, host.bundleIdentifier == current.bundleIdentifier else {
            return []
        }
        switch intent.kind {
        case .preedit:
            guard current.rewrite?.returned == nil else { return [] }
            guard let recognition = current.recognition, recognition.text == intent.text else { return [] }
            current.lastShown = intent.text
            session = current
            return [.show(sessionID: current.id, text: intent.text, caretUTF16: intent.text.utf16.count)]
        case .commit:
            guard let recognition = current.recognition,
                  recognition.kind == .finalText,
                  recognition.text == intent.text
            else { return [] }
            guard let rewrite = current.rewrite, rewrite.verdict != .notArrived else { return [] }
            guard intent.text == rewrite.original else { return [] }
            let shown = current.lastShown
            current.lastShown = nil
            session = current
            if let shown, !shown.isEmpty, host.preedit == shown {
                return [
                    .show(sessionID: current.id, text: "", caretUTF16: 0),
                    .commit(sessionID: current.id, text: intent.text),
                ]
            }
            return [.commit(sessionID: current.id, text: intent.text)]
        }
    }
}
