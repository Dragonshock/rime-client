import Foundation

public enum HotwordString {
    /// 在场的热词去掉两端空白、丢掉空白项，再用单个空格拼成一段。缺席保持 nil，不写成空字符串。
    public static func project(_ words: [String]?) -> String? {
        guard let words else { return nil }
        let pieces = words.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
        guard !pieces.isEmpty else { return nil }
        return pieces.joined(separator: " ")
    }
}
