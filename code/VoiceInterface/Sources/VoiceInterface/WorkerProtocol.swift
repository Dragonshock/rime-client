import Foundation

public enum RecognitionCommand: Equatable, Sendable {
    case start(sessionID: UInt64, context: String?)
    case feed(sessionID: UInt64, pcm: Data)
    case finish(sessionID: UInt64)
    case stop

    public func jsonLine() -> String {
        let object: [String: Any]
        switch self {
        case .start(let sessionID, let context):
            var payload: [String: Any] = [
                "cmd": "start",
                "session": sessionID,
                "model": "moona3k/mlx-qwen3-asr-0.6b-4bit",
                "chunk_size_sec": 2.0,
                "max_context_sec": 30.0,
                "sample_rate": 16000,
                "language": "Chinese",
            ]
            if let context {
                payload["context"] = context
            }
            object = payload
        case .feed(let sessionID, let pcm):
            object = [
                "cmd": "feed",
                "session": sessionID,
                "pcm_b64": pcm.base64EncodedString(),
            ]
        case .finish(let sessionID):
            object = ["cmd": "finish", "session": sessionID]
        case .stop:
            object = ["cmd": "stop"]
        }
        let data = try! JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
        return String(data: data, encoding: .utf8)!
    }
}

public struct RecognitionReply: Equatable, Sendable {
    public var sessionID: UInt64?
    public var text: String?
    public var decoded: Bool
    public var kind: String?
    public var ok: Bool
    public var message: String?

    public init(sessionID: UInt64?, text: String?, decoded: Bool, kind: String?, ok: Bool, message: String?) {
        self.sessionID = sessionID
        self.text = text
        self.decoded = decoded
        self.kind = kind
        self.ok = ok
        self.message = message
    }
}

public enum RecognitionReplyParser {
    public static func parse(_ line: String) -> RecognitionReply? {
        guard let data = line.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return nil }
        let session = object["session"] as? UInt64 ?? (object["session"] as? Int).map(UInt64.init)
        return RecognitionReply(
            sessionID: session,
            text: object["text"] as? String,
            decoded: object["decoded"] as? Bool ?? false,
            kind: object["kind"] as? String,
            ok: object["ok"] as? Bool ?? false,
            message: object["message"] as? String
        )
    }
}
