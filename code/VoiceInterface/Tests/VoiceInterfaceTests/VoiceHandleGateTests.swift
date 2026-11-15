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
    /// 和 EnginePCMSource 的 running 一样：停了就是真的不再往上报帧。
    var isRunning = false
    private var onFrame: ((AudioFrame) -> Void)?
    /// stop 之后还留着的那一枚回调，用来模拟「停之前已经拿到、停之后才跑到」的帧。
    private(set) var staleOnFrame: ((AudioFrame) -> Void)?

    func start(onFrame: @escaping (AudioFrame) -> Void) {
        self.onFrame = onFrame
        self.staleOnFrame = onFrame
        started += 1
        isRunning = true
    }

    func stop() {
        stopped += 1
        isRunning = false
        onFrame = nil
    }

    /// 把一帧音频从采集侧推上去，走的是 gate 在 start 里拿到的那枚回调。
    func deliver(_ frame: AudioFrame) {
        onFrame?(frame)
    }

    func deliverStale(_ frame: AudioFrame) {
        staleOnFrame?(frame)
    }
}

private let oneFrame = AudioFrame.pcm16Mono16k(Data([0x10, 0x00]))

private func feedCount(_ transport: FakeTransport) -> Int {
    transport.lines.filter { $0.contains("\"cmd\":\"feed\"") }.count
}

/// 一边转主 runloop（好让 `receive` 派回主队列的回包真被应用），一边把时间走过去。
private func spin(seconds: TimeInterval) {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        RunLoop.main.run(until: Date().addingTimeInterval(0.002))
    }
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

    func testAbandonStopsCaptureAndSendsNoText() {
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
        XCTAssertTrue(capture.isRunning)
        capture.deliver(oneFrame)
        XCTAssertEqual(feedCount(transport), 1)

        gate.receive(RecognitionReply(sessionID: 1, text: "甲乙", decoded: true, kind: "partial", ok: true, message: nil))
        let shown = expectation(description: "shown")
        DispatchQueue.main.async { shown.fulfill() }
        wait(for: [shown], timeout: 1)
        XCTAssertEqual(host.shows, ["甲乙"])

        XCTAssertTrue(gate.abandon(reason: .deactivate))
        // 采集真的停了：采集源不再运行，后面推上来的帧一帧也进不了传输。
        XCTAssertEqual(capture.stopped, 1)
        XCTAssertFalse(capture.isRunning)
        XCTAssertEqual(gate.sessionLife, .closed)
        capture.deliver(oneFrame)
        capture.deliverStale(oneFrame)
        XCTAssertEqual(feedCount(transport), 1)

        // 迟到的部分/定稿都不写也不交。
        gate.receive(RecognitionReply(sessionID: 1, text: "甲乙丙", decoded: true, kind: "partial", ok: true, message: nil))
        gate.receive(RecognitionReply(sessionID: 1, text: "甲乙丙。", decoded: true, kind: "final", ok: true, message: nil))
        let done = expectation(description: "late replies applied")
        DispatchQueue.main.async { done.fulfill() }
        wait(for: [done], timeout: 1)
        XCTAssertEqual(host.shows, ["甲乙"])
        XCTAssertTrue(host.commits.isEmpty)
        XCTAssertEqual(lines, ["voice session 1 abandoned deactivate"])
    }

    func testSecondAbandonDoesNothing() {
        var lines: [String] = []
        VoiceLog.sink = { lines.append($0) }
        defer { VoiceLog.sink = nil }

        let host = FakeHost()
        let capture = FakeCapture()
        let gate = VoiceHandleGate(
            configuredKey: ModifierKeycode.rightOption,
            hotwords: nil,
            host: host,
            transport: FakeTransport(),
            capture: capture
        )
        _ = gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)))
        XCTAssertTrue(gate.abandon(reason: .deactivate))
        XCTAssertFalse(gate.abandon(reason: .watchdog))
        XCTAssertEqual(capture.stopped, 1)
        XCTAssertEqual(lines.count, 1)
        XCTAssertEqual(gate.sessionLife, .closed)

        // 会话已经关掉，下一次按下是新的采集。
        XCTAssertTrue(
            gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)))
        )
        XCTAssertEqual(capture.started, 2)
        XCTAssertTrue(capture.isRunning)
        XCTAssertEqual(gate.sessionLife, .open)
    }

    func testAbandonAfterReleaseStillCommitsNothing() {
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
        _ = gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)))
        _ = gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: false)))
        XCTAssertEqual(capture.stopped, 1)
        XCTAssertFalse(capture.isRunning)
        // 松手之后会话还开着（等定稿），这期间取消同样不交任何文本。
        XCTAssertEqual(gate.sessionLife, .open)
        XCTAssertTrue(gate.abandon(reason: .watchdog))
        XCTAssertFalse(capture.isRunning)
        XCTAssertEqual(gate.sessionLife, .closed)

        gate.receive(RecognitionReply(sessionID: 1, text: "临时定稿", decoded: true, kind: "final", ok: true, message: nil))
        let done = expectation(description: "final after abandon applied")
        DispatchQueue.main.async { done.fulfill() }
        wait(for: [done], timeout: 1)
        XCTAssertTrue(host.commits.isEmpty)
        XCTAssertTrue(host.shows.isEmpty)

        // 下一次按下重新开采集，旧的取消不把语音路卡死。
        XCTAssertTrue(
            gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)))
        )
        XCTAssertEqual(capture.started, 2)
        XCTAssertTrue(capture.isRunning)
    }

    // MARK: - 看门狗

    func testWatchdogStopsCaptureAndCommitsNothing() {
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
            callbackQueue: .main,
            watchdogSeconds: 0.05
        )
        XCTAssertTrue(
            gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)))
        )
        XCTAssertTrue(capture.isRunning)
        capture.deliver(oneFrame)
        XCTAssertEqual(feedCount(transport), 1)

        // 既不松手也不回话：窗口过完自己停。等四个窗口，留足调度余量。
        spin(seconds: 0.2)
        XCTAssertEqual(capture.stopped, 1)
        XCTAssertFalse(capture.isRunning)
        XCTAssertEqual(gate.sessionLife, .closed)
        XCTAssertEqual(lines, ["voice session 1 abandoned watchdog"])
        // 看门狗与取消同路：一条文本都不往文本框里走。
        XCTAssertTrue(host.commits.isEmpty)
        XCTAssertTrue(host.shows.isEmpty)

        // 停掉的采集一帧也不再上报（新的与停之前残留的回调各试一次）。
        capture.deliver(oneFrame)
        capture.deliverStale(oneFrame)
        XCTAssertEqual(feedCount(transport), 1)

        // 已在路上的定稿也交不出去。
        gate.receive(RecognitionReply(sessionID: 1, text: "甲乙丙。", decoded: true, kind: "final", ok: true, message: nil))
        spin(seconds: 0.05)
        XCTAssertTrue(host.commits.isEmpty)
        XCTAssertTrue(host.shows.isEmpty)
    }

    func testWatchdogDoesNotFireWhenTheKeyIsReleasedInTime() {
        var lines: [String] = []
        VoiceLog.sink = { lines.append($0) }
        defer { VoiceLog.sink = nil }

        let host = FakeHost()
        let capture = FakeCapture()
        let gate = VoiceHandleGate(
            configuredKey: ModifierKeycode.rightOption,
            hotwords: nil,
            host: host,
            transport: FakeTransport(),
            capture: capture,
            callbackQueue: .main,
            watchdogSeconds: 0.05
        )
        XCTAssertTrue(
            gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)))
        )
        XCTAssertTrue(
            gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: false)))
        )
        XCTAssertEqual(capture.stopped, 1)

        // 等到窗口过完：已经松手的会话不能再被停一次，也不能被看门狗关掉。
        spin(seconds: 0.2)
        XCTAssertEqual(capture.stopped, 1)
        XCTAssertEqual(gate.sessionLife, .open)
        XCTAssertEqual(lines, [])

        // 正常路径不受影响：定稿照样提交。
        gate.receive(RecognitionReply(sessionID: 1, text: "定稿", decoded: true, kind: "final", ok: true, message: nil))
        spin(seconds: 0.05)
        XCTAssertEqual(host.commits, ["定稿"])
        XCTAssertEqual(gate.sessionLife, .closed)
        XCTAssertEqual(lines, [])
    }

    func testWatchdogWindowRestartsWhileTheEngineKeepsReplying() {
        var lines: [String] = []
        VoiceLog.sink = { lines.append($0) }
        defer { VoiceLog.sink = nil }

        let host = FakeHost()
        let capture = FakeCapture()
        let gate = VoiceHandleGate(
            configuredKey: ModifierKeycode.rightOption,
            hotwords: nil,
            host: host,
            transport: FakeTransport(),
            capture: capture,
            callbackQueue: .main,
            watchdogSeconds: 0.1
        )
        XCTAssertTrue(
            gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)))
        )
        // 按住期间一直在出部分结果：总时长超过一个窗口，但每次回话都把窗口重新起算。
        let partials = ["一", "一二", "一二三", "一二三四", "一二三四五", "一二三四五六", "一二三四五六七"]
        for partial in partials {
            gate.receive(
                RecognitionReply(sessionID: 1, text: partial, decoded: true, kind: "partial", ok: true, message: nil)
            )
            spin(seconds: 0.02)
        }
        XCTAssertEqual(capture.stopped, 0)
        XCTAssertTrue(capture.isRunning)
        XCTAssertEqual(gate.sessionLife, .open)
        XCTAssertTrue(host.commits.isEmpty)

        // 松手之后照旧：停采集、定稿提交。
        XCTAssertTrue(
            gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: false)))
        )
        gate.receive(RecognitionReply(sessionID: 1, text: "定稿", decoded: true, kind: "final", ok: true, message: nil))
        spin(seconds: 0.2)
        XCTAssertEqual(capture.stopped, 1)
        XCTAssertEqual(host.commits, ["定稿"])
        XCTAssertEqual(lines, [])
    }

    func testWatchdogLeavesTheNextSessionAlone() {
        var lines: [String] = []
        VoiceLog.sink = { lines.append($0) }
        defer { VoiceLog.sink = nil }

        let host = FakeHost()
        let capture = FakeCapture()
        let gate = VoiceHandleGate(
            configuredKey: ModifierKeycode.rightOption,
            hotwords: nil,
            host: host,
            transport: FakeTransport(),
            capture: capture,
            callbackQueue: .main,
            watchdogSeconds: 0.05
        )
        XCTAssertTrue(
            gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)))
        )
        spin(seconds: 0.2)
        XCTAssertEqual(capture.stopped, 1)
        XCTAssertEqual(gate.sessionLife, .closed)

        // 被看门狗停掉之后语音路不卡死：下一次按下是新的采集。
        XCTAssertTrue(
            gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)))
        )
        XCTAssertEqual(capture.started, 2)
        XCTAssertTrue(capture.isRunning)
        XCTAssertEqual(gate.sessionLife, .open)

        // 第二枚会话当场松手：旧表早已作废，新的也已撤掉。
        XCTAssertTrue(
            gate.consume(VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: false)))
        )
        XCTAssertEqual(capture.stopped, 2)
        spin(seconds: 0.2)
        XCTAssertEqual(capture.stopped, 2)
        XCTAssertEqual(gate.sessionLife, .open)
        XCTAssertEqual(lines, ["voice session 1 abandoned watchdog"])
    }
}
