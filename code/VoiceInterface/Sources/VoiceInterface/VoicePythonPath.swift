import Foundation

/// 源文件在 `code/Client/sources/`。删掉文件名、sources、Client 三段后，
/// 依次接上 voice-worker、.venv、bin、python3。每一段单独 append。
public enum VoicePythonPath {
    public static func interpreter(sourceFile: URL) -> URL {
        var url = sourceFile
        url.deleteLastPathComponent()
        url.deleteLastPathComponent()
        url.deleteLastPathComponent()
        return url
            .appendingPathComponent("voice-worker")
            .appendingPathComponent(".venv")
            .appendingPathComponent("bin")
            .appendingPathComponent("python3")
    }
}
