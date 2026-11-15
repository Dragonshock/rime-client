import XCTest
@testable import VoiceInterface

/// 这组用例走的是**默认**编号源（进程级共享的那一枚），不注入自己的计数器：
/// 要证的正是「换一扇门（新的 `IMKInputController`）之后不会把 1 再发一遍」。
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
    private var onFrame: ((AudioFrame) -> Void)?

    func start(onFrame: @escaping (AudioFrame) -> Void) {
        self.onFrame = onFrame
        started += 1
    }

    func stop() {
        stopped += 1
        onFrame = nil
    }
}

private let press = VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true))
private let releaseKey = VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: false))

private struct Rig {
    let gate: VoiceHandleGate
    let host: FakeHost
    let transport: FakeTransport
    let capture: FakeCapture
}

final class VoiceSessionNumberTests: XCTestCase {
    /// 一扇新的门 = 一个 `IMKInputController` 实例。这里不传 `numbers`，用默认那枚共享计数器。
    private func rig(callbackQueue: DispatchQueue? = nil) -> Rig {
        let host = FakeHost()
        let transport = FakeTransport()
        let capture = FakeCapture()
        let gate: VoiceHandleGate
        if let callbackQueue {
            gate = VoiceHandleGate(
                configuredKey: ModifierKeycode.rightOption,
                hotwords: nil,
                host: host,
                transport: transport,
                capture: capture,
                callbackQueue: callbackQueue
            )
        } else {
            gate = VoiceHandleGate(
                configuredKey: ModifierKeycode.rightOption,
                hotwords: nil,
                host: host,
                transport: transport,
                capture: capture
            )
        }
        return Rig(gate: gate, host: host, transport: transport, capture: capture)
    }

    /// 两枚门交替打开：号必须是新的，日志里能逐行对上。
    func testTwoGatesAlternatingNeverReuseASessionNumber() {
        var lines: [String] = []
        VoiceLog.sink = { lines.append($0) }
        defer { VoiceLog.sink = nil }

        let first = rig()
        let second = rig()

        XCTAssertTrue(first.gate.consume(press))
        let firstNumber = first.gate.sessionID ?? 0
        XCTAssertTrue(first.gate.abandon(reason: .deactivate))

        XCTAssertTrue(second.gate.consume(press))
        let secondNumber = second.gate.sessionID ?? 0
        XCTAssertTrue(second.gate.abandon(reason: .deactivate))

        // 第一枚门再开一次，它自己上一次用过的号也不能再用。
        XCTAssertTrue(first.gate.consume(press))
        let thirdNumber = first.gate.sessionID ?? 0

        XCTAssertGreaterThan(firstNumber, 0)
        XCTAssertNotEqual(firstNumber, secondNumber)
        XCTAssertNotEqual(secondNumber, thirdNumber)
        XCTAssertNotEqual(firstNumber, thirdNumber)
        XCTAssertLessThan(firstNumber, secondNumber)
        XCTAssertLessThan(secondNumber, thirdNumber)

        XCTAssertEqual(
            lines,
            [
                "voice session \(firstNumber) begin bundle=com.example.notes",
                "voice session \(firstNumber) abandoned deactivate",
                "voice session \(secondNumber) begin bundle=com.example.notes",
                "voice session \(secondNumber) abandoned deactivate",
                "voice session \(thirdNumber) begin bundle=com.example.notes",
            ]
        )
    }

    /// 同一枚门连开两次（上一次定稿关掉之后再开）：号也必须换。
    func testSameGateReopeningNeverReusesASessionNumber() {
        var lines: [String] = []
        VoiceLog.sink = { lines.append($0) }
        defer { VoiceLog.sink = nil }

        let r = rig(callbackQueue: .main)

        XCTAssertTrue(r.gate.consume(press))
        let firstNumber = r.gate.sessionID ?? 0
        XCTAssertTrue(r.gate.consume(releaseKey))
        r.gate.receive(
            RecognitionReply(sessionID: firstNumber, text: "定稿", decoded: true, kind: "final", ok: true, message: nil)
        )
        let done = expectation(description: "定稿落地")
        DispatchQueue.main.async { done.fulfill() }
        wait(for: [done], timeout: 1)
        XCTAssertEqual(r.gate.sessionLife, .closed)
        XCTAssertEqual(r.host.commits, ["定稿"])

        XCTAssertTrue(r.gate.consume(press))
        let secondNumber = r.gate.sessionID ?? 0
        XCTAssertGreaterThan(secondNumber, firstNumber)
        XCTAssertEqual(r.capture.started, 2)

        XCTAssertEqual(
            lines,
            [
                "voice session \(firstNumber) begin bundle=com.example.notes",
                "voice session \(firstNumber) final",
                "voice session \(secondNumber) begin bundle=com.example.notes",
            ]
        )
    }

    /// 不经过门、直接开两枚机器（默认编号源）：号同样不重复。
    func testTwoMachinesWithTheDefaultSourceTakeDistinctNumbers() {
        var first = VoiceMachine(configuredKey: ModifierKeycode.rightOption)
        var second = VoiceMachine(configuredKey: ModifierKeycode.rightOption)
        guard
            let firstNumber = open(&first),
            let secondNumber = open(&second)
        else {
            return XCTFail("两枚机器都该开出会话")
        }
        XCTAssertLessThan(firstNumber, secondNumber)
    }

    /// 换窗口之后连着来三枚新的门（三个 `IMKInputController` 实例），各自按下：
    /// 三个号互不相同、严格递增，每一枚都按自己的号发 `start`，没有一条被拒。
    /// 引擎那一侧「三次都接受」由 `voice-ffi` 的
    /// `three_fresh_sessions_begin_in_a_row_are_all_accepted` 与 `/tmp/voice-abi` 的
    /// C 探针证；这里证的是号不会重复发。
    func testThreeFreshGatesTakeThreeDistinctNumbers() {
        var lines: [String] = []
        VoiceLog.sink = { lines.append($0) }
        defer { VoiceLog.sink = nil }

        var numbers: [UInt64] = []
        for _ in 0..<3 {
            let r = rig()
            XCTAssertTrue(r.gate.consume(press))
            guard let number = r.gate.sessionID else { return XCTFail("新门应当开出会话") }
            numbers.append(number)
            XCTAssertTrue(
                r.transport.lines.contains { $0.contains("\"cmd\":\"start\"") && $0.contains("\"session\":\(number)") },
                "第 \(numbers.count) 枚门应当按自己的号 \(number) 发 start：\(r.transport.lines)"
            )
        }

        XCTAssertEqual(numbers.count, 3)
        for (earlier, later) in zip(numbers, numbers.dropFirst()) {
            XCTAssertLessThan(earlier, later, "三枚号必须严格递增：\(numbers)")
        }
        XCTAssertEqual(Set(numbers).count, 3, "三枚号必须互不相同：\(numbers)")
        XCTAssertEqual(
            lines,
            numbers.map { "voice session \($0) begin bundle=com.example.notes" },
            "日志逐行按号对上，没有 rejected 行"
        )
    }

    /// 注入的计数器照它自己的起点发号，接着往下发。
    func testInjectedNumbersContinueWhereTheSourceIsTold() {
        let numbers = VoiceSessionNumbers(startingAt: 40)
        var first = VoiceMachine(configuredKey: ModifierKeycode.rightOption, numbers: numbers)
        var second = VoiceMachine(configuredKey: ModifierKeycode.rightOption, numbers: numbers)
        XCTAssertEqual(open(&first), 41)
        XCTAssertEqual(open(&second), 42)
    }

    private func open(_ machine: inout VoiceMachine) -> UInt64? {
        _ = machine.handle(
            press,
            host: HostSnapshot(clientMissing: false, bundleIdentifier: "com.example.notes", preedit: "")
        )
        return machine.session?.id
    }
}
