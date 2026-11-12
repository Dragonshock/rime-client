import Foundation

public enum RewriteAdapter {
    /// 提示缺席时不调用模型。返回串缺席，判定为不替换。原文留下定稿。
    public static func record(sessionID: UInt64, finalText: String) -> RewriteRecord {
        RewriteRecord(sessionID: sessionID, original: finalText, returned: nil, verdict: .unchanged)
    }
}
