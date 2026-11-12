import Foundation

public enum VoiceKeyPhase: Equatable, Sendable {
    case flagsChanged(pressed: Bool)
    case keyDown
}

public struct VoiceKeyEvent: Equatable, Sendable {
    public var keyCode: UInt16
    public var phase: VoiceKeyPhase
    public var commandDown: Bool

    public init(keyCode: UInt16, phase: VoiceKeyPhase, commandDown: Bool = false) {
        self.keyCode = keyCode
        self.phase = phase
        self.commandDown = commandDown
    }
}

public enum VoiceKeyDecision: Equatable, Sendable {
    case takePress
    case takeRelease
    case pass
}

public enum ModifierKeycode {
    public static let shift: UInt16 = 0x38
    public static let rightShift: UInt16 = 0x3C
    public static let capsLock: UInt16 = 0x39
    public static let control: UInt16 = 0x3B
    public static let rightControl: UInt16 = 0x3E
    public static let option: UInt16 = 0x3A
    public static let rightOption: UInt16 = 0x3D
    public static let command: UInt16 = 0x37
    public static let rightCommand: UInt16 = 0x36
    public static let function: UInt16 = 0x3F

    public static let all: Set<UInt16> = [
        shift, rightShift, capsLock, control, rightControl,
        option, rightOption, command, rightCommand, function,
    ]
}

public enum VoiceTrigger {
    public static func decide(configuredKey: UInt16?, event: VoiceKeyEvent) -> VoiceKeyDecision {
        guard let configuredKey, ModifierKeycode.all.contains(configuredKey) else { return .pass }
        switch event.phase {
        case .flagsChanged(let pressed):
            guard event.keyCode == configuredKey else { return .pass }
            return pressed ? .takePress : .takeRelease
        case .keyDown:
            if event.commandDown && event.keyCode != configuredKey { return .pass }
            guard event.keyCode == configuredKey else { return .pass }
            return .takePress
        }
    }
}
