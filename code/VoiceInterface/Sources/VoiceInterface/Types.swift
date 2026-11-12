import Foundation

public struct AudioFrame: Equatable, Sendable {
    public var encoding: String
    public var sampleRate: Int
    public var channelCount: Int
    public var bytes: Data

    public init(encoding: String, sampleRate: Int, channelCount: Int, bytes: Data) {
        self.encoding = encoding
        self.sampleRate = sampleRate
        self.channelCount = channelCount
        self.bytes = bytes
    }

    public static func pcm16Mono16k(_ bytes: Data) -> AudioFrame {
        AudioFrame(encoding: "pcm_s16le", sampleRate: 16_000, channelCount: 1, bytes: bytes)
    }
}

public enum RecognitionKind: Equatable, Sendable {
    case partial
    case finalText
}

public struct RecognitionRecord: Equatable, Sendable {
    public var sessionID: UInt64
    public var kind: RecognitionKind
    public var text: String

    public init(sessionID: UInt64, kind: RecognitionKind, text: String) {
        self.sessionID = sessionID
        self.kind = kind
        self.text = text
    }
}

public enum RewriteVerdict: Equatable, Sendable {
    case notArrived
    case unchanged
    case different
}

public struct RewriteRecord: Equatable, Sendable {
    public var sessionID: UInt64
    public var original: String
    public var returned: String?
    public var verdict: RewriteVerdict

    public init(sessionID: UInt64, original: String, returned: String?, verdict: RewriteVerdict) {
        self.sessionID = sessionID
        self.original = original
        self.returned = returned
        self.verdict = verdict
    }
}

public enum IntentKind: Equatable, Sendable {
    case preedit
    case commit
}

public struct VoiceIntent: Equatable, Sendable {
    public var sessionID: UInt64
    public var text: String
    public var kind: IntentKind

    public init(sessionID: UInt64, text: String, kind: IntentKind) {
        self.sessionID = sessionID
        self.text = text
        self.kind = kind
    }
}

public enum DeliveryCall: Equatable, Sendable {
    case show(text: String, caretUTF16: Int)
    case commit(String)
}

public struct HostSnapshot: Equatable, Sendable {
    public var clientMissing: Bool
    public var bundleIdentifier: String?
    public var preedit: String

    public init(clientMissing: Bool, bundleIdentifier: String?, preedit: String) {
        self.clientMissing = clientMissing
        self.bundleIdentifier = bundleIdentifier
        self.preedit = preedit
    }
}
