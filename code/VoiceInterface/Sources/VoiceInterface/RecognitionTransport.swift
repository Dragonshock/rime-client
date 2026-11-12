import Foundation

public protocol RecognitionTransport: AnyObject {
    func send(_ command: RecognitionCommand)
    func stop()
}

public final class PythonRecognitionTransport: RecognitionTransport {
    private let process = Process()
    private let input = Pipe()
    private let output = Pipe()
    private let queue = DispatchQueue(label: "voice.recognition.transport")
    private var buffer = Data()
    private let onReply: (RecognitionReply) -> Void
    private var stopped = false

    public init(pythonURL: URL, workerURL: URL, onReply: @escaping (RecognitionReply) -> Void) {
        self.onReply = onReply
        process.executableURL = pythonURL
        process.arguments = [workerURL.path]
        process.standardInput = input
        process.standardOutput = output
        let errorPipe = Pipe()
        process.standardError = errorPipe
        errorPipe.fileHandleForReading.readabilityHandler = { handle in
            let data = handle.availableData
            guard !data.isEmpty, let text = String(data: data, encoding: .utf8) else { return }
            VoiceLog.write(text)
        }
        output.fileHandleForReading.readabilityHandler = { [weak self] handle in
            self?.consume(handle.availableData)
        }
        do {
            VoiceLog.write("launch \(pythonURL.path) \(workerURL.path)")
            try process.run()
        } catch {
            VoiceLog.write("launch failed \(error)")
        }
    }

    public func send(_ command: RecognitionCommand) {
        queue.async { [input] in
            guard let data = (command.jsonLine() + "\n").data(using: .utf8) else { return }
            input.fileHandleForWriting.write(data)
        }
    }

    public func stop() {
        queue.async { [weak self] in
            guard let self, !self.stopped else { return }
            self.stopped = true
            if let data = (RecognitionCommand.stop.jsonLine() + "\n").data(using: .utf8) {
                self.input.fileHandleForWriting.write(data)
            }
            self.output.fileHandleForReading.readabilityHandler = nil
        }
    }

    private func consume(_ data: Data) {
        guard !data.isEmpty else { return }
        buffer.append(data)
        while let newline = buffer.firstIndex(of: 10) {
            let line = buffer[..<newline]
            buffer.removeSubrange(...newline)
            guard let text = String(data: line, encoding: .utf8),
                  let reply = RecognitionReplyParser.parse(text)
            else { continue }
            onReply(reply)
        }
    }
}
