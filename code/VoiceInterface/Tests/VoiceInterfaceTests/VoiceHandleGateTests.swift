import XCTest
@testable import VoiceInterface

private final class FakeHost: VoiceFieldHost {
    var voiceClientMissing = false
    var voiceBundleIdentifier: String? = "com.example.notes"
    var voicePreedit = ""
    var shows: [String] = []
    var commits: [String] = []

    func voiceShow(text: String, caretUTF16: Int) {
        _ = caretUTF16
        voicePreedit = text
        shows.append(text)
    }

    func voiceCommit(_ text: String) {
        voicePreedit = ""
        commits.append(text)
    }
}

private final class FakeTransport: RecognitionTransport {
    var lines: [String] = []
    var handler: ((RecognitionReply) -> Void)?
    func send(_ command: RecognitionCommand) { lines.append(command.jsonLine()) }
    func stop() {}
}

private final class FakeCapture: PCMSource {
    var started = 0
    var stopped = 0
    func start(onFrame: @escaping (AudioFrame) -> Void) {
        _ = onFrame
        started += 1
    }
    func stop() { stopped += 1 }
}

final class VoiceHandleGateTests: XCTestCase {
    func testPressIsConsumedAndStartsWorkerWithoutCommit() {
        let host = FakeHost()
        let transport = FakeTransport()
        let capture = FakeCapture()
        let gate = VoiceHandleGate(
            configuredKey: ModifierKeycode.rightOption,
            hotwords: ["甲"],
            host: host,
            transport: transport,
            capture: capture,
            callbackQueue: .main
        )
        let consumed = gate.consume(
            VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true))
        )
        XCTAssertTrue(consumed)
        XCTAssertEqual(capture.started, 1)
        XCTAssertTrue(transport.lines[0].contains("\"cmd\":\"start\""))
        XCTAssertTrue(transport.lines[0].contains("甲"))
        XCTAssertTrue(host.commits.isEmpty)

        gate.receive(RecognitionReply(sessionID: 1, text: "甲乙", decoded: true, kind: "partial", ok: true, message: nil))
        let partial = expectation(description: "partial")
        DispatchQueue.main.async { partial.fulfill() }
        wait(for: [partial], timeout: 1)
        XCTAssertEqual(host.shows, ["甲乙"])
        XCTAssertTrue(host.commits.isEmpty)
    }

    func testReleaseFinishesAndCommitsOriginal() {
        let host = FakeHost()
        let transport = FakeTransport()
        let capture = FakeCapture()
        let gate = VoiceHandleGate(
            configuredKey: ModifierKeycode.function,
            hotwords: nil,
            host: host,
            transport: transport,
            capture: capture,
            callbackQueue: .main
        )
        _ = gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.function, phase: .flagsChanged(pressed: true)))
        gate.receive(RecognitionReply(sessionID: 1, text: "部分", decoded: true, kind: "partial", ok: true, message: nil))
        let shown = expectation(description: "shown")
        DispatchQueue.main.async { shown.fulfill() }
        wait(for: [shown], timeout: 1)

        let consumed = gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.function, phase: .flagsChanged(pressed: false)))
        XCTAssertTrue(consumed)
        XCTAssertEqual(capture.stopped, 1)
        XCTAssertTrue(transport.lines.contains { $0.contains("\"cmd\":\"finish\"") })

        gate.receive(RecognitionReply(sessionID: 1, text: "定稿", decoded: true, kind: "final", ok: true, message: nil))
        let done = expectation(description: "done")
        DispatchQueue.main.async { done.fulfill() }
        wait(for: [done], timeout: 1)
        XCTAssertEqual(host.commits, ["定稿"])
        XCTAssertFalse(host.shows.contains { $0 == "定稿" && host.commits.isEmpty })
    }

    func testSameRejectionMessageIsLoggedOnce() {
        var lines: [String] = []
        VoiceLog.sink = { lines.append($0) }
        defer { VoiceLog.sink = nil }

        let host = FakeHost()
        let gate = VoiceHandleGate(
            configuredKey: ModifierKeycode.rightOption,
            hotwords: nil,
            host: host,
            transport: FakeTransport(),
            capture: FakeCapture(),
            callbackQueue: .main
        )
        _ = gate.consume(
            VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true))
        )
        let rejected = RecognitionReply(
            sessionID: 1,
            text: nil,
            decoded: false,
            kind: nil,
            ok: false,
            message: "No module named 'mlx_qwen3_asr'"
        )
        gate.receive(rejected)
        gate.receive(rejected)
        gate.receive(
            RecognitionReply(
                sessionID: 1,
                text: nil,
                decoded: false,
                kind: nil,
                ok: false,
                message: "other failure"
            )
        )
        gate.receive(
            RecognitionReply(sessionID: 1, text: "甲乙", decoded: true, kind: "partial", ok: true, message: nil)
        )
        let done = expectation(description: "replies applied")
        DispatchQueue.main.async { done.fulfill() }
        wait(for: [done], timeout: 1)

        XCTAssertEqual(
            lines,
            [
                "recognition rejected No module named 'mlx_qwen3_asr'",
                "recognition rejected other failure",
            ]
        )
        XCTAssertEqual(host.shows, ["甲乙"])
        XCTAssertTrue(host.commits.isEmpty)
    }

    func testPlainKeyIsNotConsumed() {
        let host = FakeHost()
        let gate = VoiceHandleGate(
            configuredKey: ModifierKeycode.rightOption,
            hotwords: nil,
            host: host,
            transport: FakeTransport(),
            capture: FakeCapture()
        )
        XCTAssertFalse(gate.consume(VoiceKeyEvent(keyCode: 0, phase: .keyDown)))
    }
}
