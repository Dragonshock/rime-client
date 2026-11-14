import XCTest
@testable import VoiceInterface

final class VoiceEngineTests: XCTestCase {
    func testMissingStaticLibraryReturnsNotLinked() {
        XCTAssertEqual(VoiceEngine.linkStatus(), VoiceEngine.notLinkedMessage)
        XCTAssertEqual(VoiceEngine.linkStatus(), "引擎未链入")

        var replies: [RecognitionReply] = []
        let engine = VoiceEngine(
            modelDirectory: "/no/such/voice/model",
            metallibPath: "/no/such/mlx.metallib"
        ) { replies.append($0) }
        XCTAssertEqual(engine.unavailableReason, "引擎未链入")
        engine.send(.start(sessionID: 4, context: "专名"))
        engine.send(.feed(sessionID: 4, pcm: Data([0, 0])))
        engine.send(.finish(sessionID: 4))
        XCTAssertEqual(replies.count, 3)
        XCTAssertTrue(replies.allSatisfy { $0.ok == false && $0.message == "引擎未链入" })
        engine.stop()
    }

    func testEngineSourceDoesNotLaunchInterpreter() throws {
        var url = URL(fileURLWithPath: #filePath)
        url.deleteLastPathComponent()
        url.deleteLastPathComponent()
        url.deleteLastPathComponent()
        let sourceURL = url
            .appendingPathComponent("Sources")
            .appendingPathComponent("VoiceInterface")
            .appendingPathComponent("VoiceEngine.swift")
        let text = try String(contentsOf: sourceURL, encoding: .utf8)
        XCTAssertFalse(text.contains("/usr/bin/python3"))
        XCTAssertFalse(text.contains("Process("))
        XCTAssertTrue(text.contains("voice_engine_new"))
        XCTAssertTrue(text.contains("引擎未链入"))
        XCTAssertTrue(text.contains("voice_engine_set_reply_sink"))
        XCTAssertTrue(text.contains("voice_engine_last_error"))
        XCTAssertTrue(text.contains("RecognitionReplyParser.parse"))
    }

    /// voice-ffi 写出的两种行。键与解析器读的六个一一对应。
    func testEngineReplyLinesParseWithTheSharedParser() {
        let final = "{\"decoded\":true,\"kind\":\"final\",\"ok\":true,\"session\":7,\"text\":\"你好\"}"
        let parsed = RecognitionReplyParser.parse(final)
        XCTAssertEqual(parsed?.sessionID, 7)
        XCTAssertEqual(parsed?.text, "你好")
        XCTAssertEqual(parsed?.kind, "final")
        XCTAssertEqual(parsed?.ok, true)
        XCTAssertEqual(parsed?.decoded, true)
        XCTAssertNil(parsed?.message)

        let failure = "{\"decoded\":false,\"message\":\"只有一 token\",\"ok\":false,\"session\":7,\"text\":\"\"}"
        let rejected = RecognitionReplyParser.parse(failure)
        XCTAssertEqual(rejected?.sessionID, 7)
        XCTAssertEqual(rejected?.ok, false)
        XCTAssertEqual(rejected?.message, "只有一 token")
        XCTAssertEqual(rejected?.text, "")
    }
}
