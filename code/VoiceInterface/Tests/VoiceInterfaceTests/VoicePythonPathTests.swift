import XCTest
@testable import VoiceInterface

final class VoicePythonPathTests: XCTestCase {
    func testDropsThreeDirectoriesThenAppendsFourComponents() {
        let source = URL(fileURLWithPath: "/repo/code/Client/sources/SquirrelInputController.swift")
        let python = VoicePythonPath.interpreter(sourceFile: source)
        XCTAssertEqual(python.path, "/repo/code/voice-worker/.venv/bin/python3")
        XCTAssertEqual(
            Array(python.pathComponents.suffix(4)),
            ["voice-worker", ".venv", "bin", "python3"]
        )
        XCTAssertFalse(python.path.contains("/Client/voice-worker"))
        XCTAssertFalse(python.path.contains("%2F"))
        XCTAssertNotEqual(python.path, "/usr/bin/python3")
    }
}
