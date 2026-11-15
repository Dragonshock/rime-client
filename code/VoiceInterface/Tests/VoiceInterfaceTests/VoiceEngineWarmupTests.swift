import XCTest
@testable import VoiceInterface

final class VoiceEngineWarmupTests: XCTestCase {
    func testIdleWarmupHasNoEngineAndSaysSo() {
        let warmup = VoiceEngineWarmup()
        XCTAssertEqual(warmup.status, .idle)
        XCTAssertNil(warmup.engineSeconds)
        XCTAssertNil(warmup.engineOrWait(0.05))
        XCTAssertNil(warmup.engineIfReady())
        XCTAssertEqual(warmup.summary, "预热未开始")
    }

    /// 预热只做一次：第二次 start 返回 false，不建第二个引擎。
    func testStartIsOnlyTakenOnce() {
        let warmup = VoiceEngineWarmup()
        let queue = DispatchQueue(label: "test.warmup.once")
        XCTAssertTrue(warmup.start(modelDirectory: "/no/such/voice/model", metallibPath: "", queue: queue))
        XCTAssertFalse(warmup.start(modelDirectory: "/no/such/voice/model", metallibPath: "", queue: queue))
        _ = warmup.waitUntilBuilt()
    }

    /// 这台机器上单测没链入静态库，所以预热会失败；失败只留一行原因，不抛。
    func testFailedWarmupKeepsReasonAndStopsWaiting() {
        let warmup = VoiceEngineWarmup()
        let queue = DispatchQueue(label: "test.warmup.failed")
        XCTAssertTrue(warmup.start(modelDirectory: "/no/such/voice/model", metallibPath: "", queue: queue))
        let engine = warmup.waitUntilBuilt()
        XCTAssertEqual(engine?.unavailableReason, VoiceEngine.notLinkedMessage)
        XCTAssertEqual(warmup.status, .failed(VoiceEngine.notLinkedMessage))
        XCTAssertEqual(warmup.status.failureReason, VoiceEngine.notLinkedMessage)
        XCTAssertTrue(warmup.summary.contains("预热失败"))
        XCTAssertTrue(warmup.summary.contains(VoiceEngine.notLinkedMessage))
        XCTAssertNil(warmup.engineIfReady())

        // 失败的那个引擎也会被交出去：它自己回一行原因，按键不会无声挂住。
        var replies: [RecognitionReply] = []
        warmup.route { replies.append($0) }
        let transport = VoiceEnginePreheatedTransport(warmup: warmup, budget: 0.05) { replies.append($0) }
        transport.send(.start(sessionID: 9, context: nil))
        XCTAssertEqual(replies.count, 1)
        XCTAssertEqual(replies.first?.sessionID, 9)
        XCTAssertEqual(replies.first?.ok, false)
        XCTAssertEqual(replies.first?.message, VoiceEngine.notLinkedMessage)
    }

    /// 预热没跑完时按键有上限地等：到点发一行拒绝，`.feed` 一次也不等。
    func testStartRejectsAfterBudgetWhileWarming() {
        let warmup = VoiceEngineWarmup()
        let queue = DispatchQueue(label: "test.warmup.suspended")
        queue.suspend()
        XCTAssertTrue(warmup.start(modelDirectory: "/no/such/voice/model", metallibPath: "", queue: queue))
        XCTAssertEqual(warmup.status, .warming)
        XCTAssertNil(warmup.engineIfReady())

        var replies: [RecognitionReply] = []
        let transport = VoiceEnginePreheatedTransport(warmup: warmup, budget: 0.05) { replies.append($0) }
        let clock = Date()
        transport.send(.start(sessionID: 3, context: nil))
        let waited = Date().timeIntervalSince(clock)
        XCTAssertGreaterThanOrEqual(waited, 0.04)
        XCTAssertLessThan(waited, 0.5)
        XCTAssertEqual(replies.count, 1)
        XCTAssertEqual(replies.first?.ok, false)
        XCTAssertEqual(replies.first?.sessionID, 3)
        XCTAssertTrue(replies.first?.message?.contains("引擎未就绪") ?? false)
        XCTAssertTrue(replies.first?.message?.contains("预热中") ?? false)

        // 还没好之前 feed 与 finish 立刻返回，不排队也不等。
        let feedClock = Date()
        transport.send(.feed(sessionID: 3, pcm: Data([0, 0, 0, 0])))
        transport.send(.finish(sessionID: 3))
        XCTAssertLessThan(Date().timeIntervalSince(feedClock), 0.01)
        XCTAssertEqual(replies.count, 1)

        queue.resume()
        _ = warmup.waitUntilBuilt()
        XCTAssertEqual(warmup.status, .failed(VoiceEngine.notLinkedMessage))
    }

    /// 预热完成之后，同一个壳直接把命令交给那个引擎，不再等。
    func testReadyWarmupForwardsWithoutWaiting() {
        let warmup = VoiceEngineWarmup()
        let queue = DispatchQueue(label: "test.warmup.ready")
        XCTAssertTrue(warmup.start(modelDirectory: "/no/such/voice/model", metallibPath: "", queue: queue))
        guard let engine = warmup.waitUntilBuilt(), engine.unavailableReason == nil else {
            // 单测没链入静态库时到不了这里，链路完整时才走下面。
            XCTAssertEqual(warmup.status, .failed(VoiceEngine.notLinkedMessage))
            return
        }
        XCTAssertNotNil(warmup.engineIfReady())
        let clock = Date()
        XCTAssertNotNil(warmup.engineOrWait(0.05))
        XCTAssertLessThan(Date().timeIntervalSince(clock), 0.01)
        XCTAssertEqual(warmup.status, .ready)
        guard let seconds = warmup.engineSeconds else {
            XCTFail("预热完成时应当留下墙钟")
            return
        }
        XCTAssertGreaterThan(seconds, 0)
        XCTAssertTrue(warmup.summary.hasPrefix("预热完成"))
    }

    /// 落点可以在预热之后换，引擎线程给的回复走最后装的那个闭包。
    func testRouteCanBeReplaced() {
        let warmup = VoiceEngineWarmup()
        var first: [RecognitionReply] = []
        var second: [RecognitionReply] = []
        warmup.route { first.append($0) }
        warmup.route { second.append($0) }
        let engine = warmup.engineOrWait(0)
        XCTAssertNil(engine)
        // 没引擎时两个闭包都不该被调；这里只确认 route 不崩、可重复装。
        XCTAssertTrue(first.isEmpty)
        XCTAssertTrue(second.isEmpty)
    }
}
