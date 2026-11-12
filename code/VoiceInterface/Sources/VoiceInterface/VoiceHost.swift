import Foundation

/// 输入法控制器交给语音交付的那一面。实现只调用 `show` 与 `commit`。
public protocol VoiceFieldHost: AnyObject {
    var voiceClientMissing: Bool { get }
    var voiceBundleIdentifier: String? { get }
    var voicePreedit: String { get }
    func voiceShow(text: String, caretUTF16: Int)
    func voiceCommit(_ text: String)
}

public extension VoiceFieldHost {
    func voiceSnapshot() -> HostSnapshot {
        HostSnapshot(
            clientMissing: voiceClientMissing,
            bundleIdentifier: voiceBundleIdentifier,
            preedit: voicePreedit
        )
    }
}
