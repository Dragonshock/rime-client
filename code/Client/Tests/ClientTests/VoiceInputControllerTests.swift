import XCTest
import VoiceInterface
@testable import Client

private final class FakeTransport: RecognitionTransport {
    var lines: [String] = []
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

final class VoiceInputControllerTests: XCTestCase {
    func testDefaultKeyIsRightOption() {
        XCTAssertEqual(VoiceInputController.defaultConfiguredKey, 0x3D)
        XCTAssertEqual(VoiceInputController.defaultConfiguredKey, ModifierKeycode.rightOption)
        let controller = VoiceInputController(transport: FakeTransport(), capture: FakeCapture())
        XCTAssertEqual(controller.configuredKey, 0x3D)
        XCTAssertFalse(controller.voiceClientMissing)
    }

    func testHandleConsumesBeforeProcessKey() {
        let capture = FakeCapture()
        var processed: [UInt16] = []
        let controller = VoiceInputController(
            bundleIdentifier: "com.example.notes",
            transport: FakeTransport(),
            capture: capture,
            processKey: { event in
                processed.append(event.keyCode)
                return false
            }
        )
        let taken = controller.handle(
            VoiceKeyEvent(keyCode: 0x3D, phase: .flagsChanged(pressed: true))
        )
        XCTAssertTrue(taken)
        XCTAssertTrue(processed.isEmpty)
        XCTAssertEqual(capture.started, 1)

        let released = controller.handle(
            VoiceKeyEvent(keyCode: 0x3D, phase: .flagsChanged(pressed: false))
        )
        XCTAssertTrue(released)
        XCTAssertEqual(capture.stopped, 1)
        XCTAssertTrue(processed.isEmpty)

        let passed = controller.handle(VoiceKeyEvent(keyCode: 0, phase: .keyDown))
        XCTAssertFalse(passed)
        XCTAssertEqual(processed, [0])
    }

    func testConfiguredKeyCanBeReplaced() {
        let capture = FakeCapture()
        var processed: [UInt16] = []
        let controller = VoiceInputController(
            configuredKey: ModifierKeycode.function,
            transport: FakeTransport(),
            capture: capture,
            processKey: { event in
                processed.append(event.keyCode)
                return false
            }
        )
        XCTAssertFalse(controller.handle(
            VoiceKeyEvent(keyCode: 0x3D, phase: .flagsChanged(pressed: true))
        ))
        XCTAssertEqual(processed, [0x3D])
        XCTAssertEqual(capture.started, 0)
        XCTAssertTrue(controller.handle(
            VoiceKeyEvent(keyCode: ModifierKeycode.function, phase: .flagsChanged(pressed: true))
        ))
        XCTAssertEqual(capture.started, 1)
        XCTAssertEqual(processed, [0x3D])
    }

    func testMissingClientConsumesWithoutCapture() {
        let capture = FakeCapture()
        var processed = 0
        let controller = VoiceInputController(
            clientMissing: true,
            transport: FakeTransport(),
            capture: capture,
            processKey: { _ in
                processed += 1
                return false
            }
        )
        XCTAssertTrue(controller.voiceClientMissing)
        XCTAssertTrue(controller.handle(
            VoiceKeyEvent(keyCode: 0x3D, phase: .flagsChanged(pressed: true))
        ))
        XCTAssertEqual(processed, 0)
        XCTAssertEqual(capture.started, 0)
    }

    func testVoiceShowAndCommitForward() {
        var shown: [(String, NSRange, Int)] = []
        var committed: [String] = []
        let controller = VoiceInputController(
            transport: FakeTransport(),
            capture: FakeCapture(),
            showPreedit: { text, range, caret in
                shown.append((text, range, caret))
            },
            commitString: { committed.append($0) }
        )
        controller.voiceShow(text: "甲乙", caretUTF16: 2)
        XCTAssertEqual(controller.voicePreedit, "甲乙")
        XCTAssertEqual(shown.count, 1)
        XCTAssertEqual(shown[0].0, "甲乙")
        XCTAssertEqual(shown[0].1, NSRange(location: 0, length: 0))
        XCTAssertEqual(shown[0].2, 2)
        controller.voiceCommit("甲乙")
        XCTAssertEqual(committed, ["甲乙"])
        XCTAssertEqual(controller.voicePreedit, "")
    }

    func testWorkerScriptPointsAtRepo() {
        let url = VoiceClientLocation.workerScript
        XCTAssertTrue(url.path.hasSuffix("code/voice-worker/worker.py"))
        XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))
    }

    func testCopiedHandleConsumesBeforeSwitch() throws {
        var repo = VoiceClientLocation.workerScript
        repo.deleteLastPathComponent()
        repo.deleteLastPathComponent()
        repo.deleteLastPathComponent()
        let file = repo
            .appendingPathComponent("code")
            .appendingPathComponent("Client")
            .appendingPathComponent("sources")
            .appendingPathComponent("SquirrelInputController.swift")
        let source = try String(contentsOf: file, encoding: .utf8)
        guard let handle = source.range(of: "override func handle"),
              let switchEvent = source.range(of: "switch event.type", range: handle.upperBound..<source.endIndex)
        else {
            XCTFail("handle 或 switch 缺失")
            return
        }
        let head = source[handle.upperBound..<switchEvent.lowerBound]
        XCTAssertTrue(head.contains("self.client ?="))
        XCTAssertTrue(head.contains(".consume("))
        XCTAssertTrue(head.contains("return true"))
        XCTAssertFalse(head.contains("processKey("))
        XCTAssertFalse(head.contains("rimeUpdate("))
        XCTAssertTrue(source.contains("IMKInputController, VoiceFieldHost"))
        XCTAssertTrue(source.contains("show(preedit: text, selRange:"))
        XCTAssertTrue(source.contains("commit(string: text)"))
        XCTAssertTrue(source.contains("client == nil"))
        XCTAssertTrue(source.contains("EnginePCMSource()"))
        XCTAssertTrue(source.contains("0x3D"))
        XCTAssertTrue(source.contains("VoiceEngine("))
        XCTAssertFalse(source.contains("voicePythonURL"))
        XCTAssertFalse(source.contains("/usr/bin/python3"))
    }
}
