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
        // 被拒之后这个会话已经关闭，不再往旧会话上写预编辑。
        XCTAssertTrue(host.shows.isEmpty)
        XCTAssertTrue(host.commits.isEmpty)

        // 再按一下是新会话，部分文本从这里开始正常上屏。
        _ = gate.consume(
            VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true))
        )
        gate.receive(
            RecognitionReply(sessionID: 2, text: "甲乙", decoded: true, kind: "partial", ok: true, message: nil)
        )
        let shown = expectation(description: "shown")
        DispatchQueue.main.async { shown.fulfill() }
        wait(for: [shown], timeout: 1)
        XCTAssertEqual(host.shows, ["甲乙"])
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

    func testRejectedFinalLogsTheReasonAndLetsTheNextPressStartAgain() {
        var lines: [String] = []
        VoiceLog.sink = { lines.append($0) }
        defer { VoiceLog.sink = nil }

        let host = FakeHost()
        let transport = FakeTransport()
        let capture = FakeCapture()
        let gate = VoiceHandleGate(
            configuredKey: ModifierKeycode.rightOption,
            hotwords: nil,
            host: host,
            transport: transport,
            capture: capture,
            callbackQueue: .main
        )
        XCTAssertTrue(
            gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)))
        )
        _ = gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: false)))
        gate.receive(RecognitionReply(
            sessionID: 1,
            text: nil,
            decoded: false,
            kind: nil,
            ok: false,
            message: "当前解码器只能给一个 token（词表下标 5，词片 \"\u{0120}the\"），不是可提交文本"
        ))
        let rejected = expectation(description: "rejected")
        DispatchQueue.main.async { rejected.fulfill() }
        wait(for: [rejected], timeout: 1)
        XCTAssertEqual(
            lines,
            ["recognition rejected 当前解码器只能给一个 token（词表下标 5，词片 \"\u{0120}the\"），不是可提交文本"]
        )
        XCTAssertTrue(host.commits.isEmpty)
        XCTAssertTrue(host.shows.isEmpty)

        // 第二下必须重新开始，否则一次失败就把语音路卡死。
        XCTAssertTrue(
            gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)))
        )
        XCTAssertEqual(capture.started, 2)
        XCTAssertEqual(transport.lines.filter { $0.contains("\"cmd\":\"start\"") }.count, 2)
    }

    func testJsonReplyFromTheEngineTakesTheSamePath() {
        let host = FakeHost()
        let gate = VoiceHandleGate(
            configuredKey: ModifierKeycode.rightOption,
            hotwords: nil,
            host: host,
            transport: FakeTransport(),
            capture: FakeCapture(),
            callbackQueue: .main
        )
        _ = gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)))
        // 部分结果同样是 decoded:true，否则预编辑不会被写出去。
        let partialLine = "{\"decoded\":true,\"kind\":\"partial\",\"ok\":true,\"session\":1,\"text\":\"今天\"}"
        guard let partial = RecognitionReplyParser.parse(partialLine) else {
            return XCTFail("部分结果那一行应可解析")
        }
        gate.receive(partial)
        let shown = expectation(description: "shown")
        DispatchQueue.main.async { shown.fulfill() }
        wait(for: [shown], timeout: 1)
        XCTAssertEqual(host.shows, ["今天"])

        _ = gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: false)))
        let line = "{\"decoded\":true,\"kind\":\"final\",\"ok\":true,\"session\":1,\"text\":\"今天开会。\"}"
        guard let reply = RecognitionReplyParser.parse(line) else {
            return XCTFail("引擎写出的那一行应可解析")
        }
        gate.receive(reply)
        let done = expectation(description: "done")
        DispatchQueue.main.async { done.fulfill() }
        wait(for: [done], timeout: 1)
        XCTAssertEqual(host.commits, ["今天开会。"])
    }
}
