import Foundation

/// 插在输入法 `handle` 进入 `processKey` 之前。返回 true 表示这次事件已取走。
public final class VoiceHandleGate {
    private let lock = NSLock()
    private var machine: VoiceMachine
    private let transport: RecognitionTransport
    private let capture: PCMSource
    private weak var host: VoiceFieldHost?
    private let callbackQueue: DispatchQueue
    /// 同一句拒绝只写一次。换一句再写。
    private var loggedRejection: String?

    public init(
        configuredKey: UInt16?,
        hotwords: [String]?,
        host: VoiceFieldHost,
        transport: RecognitionTransport,
        capture: PCMSource,
        callbackQueue: DispatchQueue = .main
    ) {
        self.machine = VoiceMachine(configuredKey: configuredKey, hotwords: hotwords)
        self.host = host
        self.transport = transport
        self.capture = capture
        self.callbackQueue = callbackQueue
    }

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

    private func perform(_ effects: [VoiceEffect]) {
        for effect in effects {
            switch effect {
            case .startCapture(let sessionID, let context):
                transport.send(.start(sessionID: sessionID, context: context))
                capture.start { [weak self] frame in
                    self?.push(frame)
                }
            case .stopCapture(let sessionID):
                capture.stop()
                transport.send(.finish(sessionID: sessionID))
            case .show(_, let text, let caret):
                host?.voiceShow(text: text, caretUTF16: caret)
            case .commit(_, let text):
                host?.voiceCommit(text)
            }
        }
    }

    private func applyReply(_ reply: RecognitionReply) {
        guard let host else { return }
        guard reply.ok, let sessionID = reply.sessionID else {
            let message = reply.message ?? ""
            if loggedRejection != message {
                loggedRejection = message
                VoiceLog.write("recognition rejected \(message)")
            }
            return
        }
        lock.lock()
        let snapshot = host.voiceSnapshot()
        machine.noteClient(snapshot)
        let effects: [VoiceEffect]
        if reply.kind == "final" {
            effects = machine.acceptFinal(sessionID: sessionID, text: reply.text ?? "", host: snapshot)
        } else if reply.decoded, let text = reply.text {
            effects = machine.acceptPartial(sessionID: sessionID, text: text, host: snapshot)
        } else {
            effects = []
        }
        lock.unlock()
        perform(effects)
    }
}
