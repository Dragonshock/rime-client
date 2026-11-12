import XCTest
@testable import VoiceInterface

final class VoiceMachineTests: XCTestCase {
    private func host(preedit: String = "", bundle: String? = "com.example.notes") -> HostSnapshot {
        HostSnapshot(clientMissing: false, bundleIdentifier: bundle, preedit: preedit)
    }

    private func press(_ machine: inout VoiceMachine, host: HostSnapshot) -> (Bool, [VoiceEffect]) {
        machine.handle(
            VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)),
            host: host
        )
    }

    func testPressWithoutClientDoesNotOpen() {
        var machine = VoiceMachine(configuredKey: ModifierKeycode.rightOption)
        let (consumed, effects) = machine.handle(
            VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: true)),
            host: HostSnapshot(clientMissing: true, bundleIdentifier: nil, preedit: "")
        )
        XCTAssertTrue(consumed)
        XCTAssertTrue(effects.isEmpty)
        XCTAssertNil(machine.session)
    }

    func testSecondPressDoesNotOpenAnotherSession() {
        var machine = VoiceMachine(configuredKey: ModifierKeycode.rightOption, hotwords: ["鼠须管"])
        let first = press(&machine, host: host())
        XCTAssertEqual(first.1, [.startCapture(1, context: "鼠须管")])
        let second = press(&machine, host: host())
        XCTAssertTrue(second.0)
        XCTAssertTrue(second.1.isEmpty)
        XCTAssertEqual(machine.session?.id, 1)
    }

    func testOtherKeyIsNotTaken() {
        var machine = VoiceMachine(configuredKey: ModifierKeycode.rightOption)
        let (consumed, _) = machine.handle(
            VoiceKeyEvent(keyCode: 0, phase: .keyDown),
            host: host()
        )
        XCTAssertFalse(consumed)
    }

    func testPartialShowsAndDoesNotCommit() {
        var machine = VoiceMachine(configuredKey: ModifierKeycode.rightOption)
        _ = press(&machine, host: host())
        let effects = machine.acceptPartial(sessionID: 1, text: "下面是一段", host: host())
        XCTAssertEqual(effects, [.show(sessionID: 1, text: "下面是一段", caretUTF16: "下面是一段".utf16.count)])
        XCTAssertNil(effects.first { if case .commit = $0 { return true }; return false })
    }

    func testEmptyFinalClearsMatchingCacheWithoutCommit() {
        var machine = VoiceMachine(configuredKey: ModifierKeycode.rightOption)
        _ = press(&machine, host: host())
        _ = machine.acceptPartial(sessionID: 1, text: "临时", host: host())
        let release = machine.handle(
            VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: false)),
            host: host(preedit: "临时")
        )
        XCTAssertEqual(release.1, [.stopCapture(1)])
        let effects = machine.acceptFinal(sessionID: 1, text: "", host: host(preedit: "临时"))
        XCTAssertEqual(effects, [.show(sessionID: 1, text: "", caretUTF16: 0)])
        XCTAssertEqual(machine.session?.life, .closed)
    }

    func testFinalCommitsOriginalAfterClearingVoiceCache() {
        var machine = VoiceMachine(configuredKey: ModifierKeycode.rightOption)
        _ = press(&machine, host: host())
        _ = machine.acceptPartial(sessionID: 1, text: "今天开会", host: host())
        _ = machine.handle(
            VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: false)),
            host: host(preedit: "今天开会")
        )
        let effects = machine.acceptFinal(sessionID: 1, text: "今天开会。", host: host(preedit: "今天开会"))
        XCTAssertEqual(
            effects,
            [
                .show(sessionID: 1, text: "", caretUTF16: 0),
                .commit(sessionID: 1, text: "今天开会。"),
            ]
        )
        XCTAssertEqual(machine.session?.rewrite?.verdict, .unchanged)
        XCTAssertNil(machine.session?.rewrite?.returned)
    }

    func testFinalDoesNotClearKeyboardCache() {
        var machine = VoiceMachine(configuredKey: ModifierKeycode.rightOption)
        _ = press(&machine, host: host())
        _ = machine.acceptPartial(sessionID: 1, text: "语音", host: host())
        _ = machine.handle(
            VoiceKeyEvent(keyCode: ModifierKeycode.rightOption, phase: .flagsChanged(pressed: false)),
            host: host(preedit: "ni")
        )
        let effects = machine.acceptFinal(sessionID: 1, text: "语音定稿", host: host(preedit: "ni"))
        XCTAssertEqual(effects, [.commit(sessionID: 1, text: "语音定稿")])
    }

    func testAbsentHotwordsStayNil() {
        XCTAssertNil(HotwordString.project(nil))
        XCTAssertNil(HotwordString.project([" ", ""]))
        XCTAssertEqual(HotwordString.project([" 甲 ", "乙"]), "甲 乙")
    }

    func testMismatchedPartialIsIgnored() {
        var machine = VoiceMachine(configuredKey: ModifierKeycode.rightOption)
        _ = press(&machine, host: host())
        let effects = machine.acceptPartial(sessionID: 9, text: "错会话", host: host())
        XCTAssertTrue(effects.isEmpty)
        XCTAssertNil(machine.session?.recognition)
    }
}
