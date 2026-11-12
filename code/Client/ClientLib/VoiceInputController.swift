import Foundation
import VoiceInterface

/// 仓库 code/voice-worker/worker.py。
public enum VoiceClientLocation {
    public static var workerScript: URL {
        var url = URL(fileURLWithPath: #filePath)
        url.deleteLastPathComponent()
        url.deleteLastPathComponent()
        url.deleteLastPathComponent()
        return url.appendingPathComponent("voice-worker").appendingPathComponent("worker.py")
    }
}

/// 不依赖 librime 的控制器接入。handle 在 processKey 之前调用 consume。
public final class VoiceInputController: VoiceFieldHost {
    /// 右 Option。换成别的修饰键码即换绑定。
    public static let defaultConfiguredKey: UInt16 = 0x3D

    public let configuredKey: UInt16
    public var voiceClientMissing: Bool
    public var voiceBundleIdentifier: String?
    public private(set) var voicePreedit: String

    private var gate: VoiceHandleGate?
    private let processKey: (VoiceKeyEvent) -> Bool
    private let showPreedit: (String, NSRange, Int) -> Void
    private let commitString: (String) -> Void

    public init(
        configuredKey: UInt16 = VoiceInputController.defaultConfiguredKey,
        hotwords: [String]? = nil,
        clientMissing: Bool = false,
        bundleIdentifier: String? = nil,
        transport: RecognitionTransport? = nil,
        capture: PCMSource? = nil,
        workerURL: URL = VoiceClientLocation.workerScript,
        pythonURL: URL = URL(fileURLWithPath: "/usr/bin/python3"),
        processKey: @escaping (VoiceKeyEvent) -> Bool = { _ in false },
        showPreedit: @escaping (String, NSRange, Int) -> Void = { _, _, _ in },
        commitString: @escaping (String) -> Void = { _ in }
    ) {
        self.configuredKey = configuredKey
        self.voiceClientMissing = clientMissing
        self.voiceBundleIdentifier = bundleIdentifier
        self.voicePreedit = ""
        self.processKey = processKey
        self.showPreedit = showPreedit
        self.commitString = commitString

        let bridge = ReplyBridge()
        let resolvedTransport = transport ?? LazyWorkerTransport(
            pythonURL: pythonURL,
            workerURL: workerURL,
            bridge: bridge
        )
        let resolvedCapture = capture ?? EnginePCMSource()
        self.gate = VoiceHandleGate(
            configuredKey: configuredKey,
            hotwords: hotwords,
            host: self,
            transport: resolvedTransport,
            capture: resolvedCapture
        )
        bridge.onReply = { [weak self] reply in
            self?.gate?.receive(reply)
        }
    }

    /// 先交给语音门。返回真则不再调用 processKey。
    public func handle(_ event: VoiceKeyEvent) -> Bool {
        if gate?.consume(event) == true {
            return true
        }
        return processKey(event)
    }

    public func voiceShow(text: String, caretUTF16: Int) {
        show(preedit: text, selRange: NSRange(location: 0, length: 0), caretPos: caretUTF16)
    }

    public func voiceCommit(_ text: String) {
        commit(string: text)
    }

    public func show(preedit: String, selRange: NSRange, caretPos: Int) {
        voicePreedit = preedit
        showPreedit(preedit, selRange, caretPos)
    }

    public func commit(string: String) {
        commitString(string)
        voicePreedit = ""
    }
}

private final class ReplyBridge {
    var onReply: ((RecognitionReply) -> Void)?
}

private final class LazyWorkerTransport: RecognitionTransport {
    private let pythonURL: URL
    private let workerURL: URL
    private let bridge: ReplyBridge
    private var inner: PythonRecognitionTransport?

    init(pythonURL: URL, workerURL: URL, bridge: ReplyBridge) {
        self.pythonURL = pythonURL
        self.workerURL = workerURL
        self.bridge = bridge
    }

    func send(_ command: RecognitionCommand) {
        if inner == nil {
            let bridge = bridge
            inner = PythonRecognitionTransport(pythonURL: pythonURL, workerURL: workerURL) { reply in
                bridge.onReply?(reply)
            }
        }
        inner?.send(command)
    }

    func stop() {
        inner?.stop()
    }
}
