import AVFoundation
import Foundation

public protocol PCMSource: AnyObject {
    func start(onFrame: @escaping (AudioFrame) -> Void)
    func stop()
}

/// 把输入设备转成 16 kHz、单声道、PCM16 小端。转换失败时停止，不把静音报成识别文本。
public final class EnginePCMSource: PCMSource {
    private let engine = AVAudioEngine()
    private var converter: AVAudioConverter?
    private let lock = NSLock()
    private var running = false

    public init() {}

    public func start(onFrame: @escaping (AudioFrame) -> Void) {
        lock.lock()
        if running {
            lock.unlock()
            return
        }
        running = true
        lock.unlock()

        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            startEngine(onFrame: onFrame)
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .audio) { [weak self] granted in
                guard let self else { return }
                if granted {
                    self.startEngine(onFrame: onFrame)
                } else {
                    VoiceLog.write("microphone denied")
                    self.stop()
                }
            }
        default:
            VoiceLog.write("microphone not authorized")
            stop()
        }
    }

    private func startEngine(onFrame: @escaping (AudioFrame) -> Void) {
        let input = engine.inputNode
        let sourceFormat = input.outputFormat(forBus: 0)
        guard let target = AVAudioFormat(
            commonFormat: .pcmFormatInt16,
            sampleRate: 16_000,
            channels: 1,
            interleaved: true
        ), let converter = AVAudioConverter(from: sourceFormat, to: target) else {
            stop()
            return
        }
        self.converter = converter
        input.installTap(onBus: 0, bufferSize: 4096, format: sourceFormat) { [weak self] buffer, _ in
            guard let self, let pcm = self.convert(buffer, converter: converter, target: target), !pcm.isEmpty else {
                return
            }
            onFrame(.pcm16Mono16k(pcm))
        }
        engine.prepare()
        do {
            try engine.start()
            VoiceLog.write("audio engine started")
        } catch {
            VoiceLog.write("audio engine failed \(error)")
            stop()
        }
    }

    public func stop() {
        lock.lock()
        let wasRunning = running
        running = false
        lock.unlock()
        guard wasRunning || engine.isRunning else { return }
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        converter = nil
    }

    private func convert(_ buffer: AVAudioPCMBuffer, converter: AVAudioConverter, target: AVAudioFormat) -> Data? {
        let ratio = target.sampleRate / buffer.format.sampleRate
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + 32
        guard let output = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else { return nil }
        var error: NSError?
        var consumed = false
        converter.convert(to: output, error: &error) { _, status in
            if consumed {
                status.pointee = .noDataNow
                return nil
            }
            consumed = true
            status.pointee = .haveData
            return buffer
        }
        if error != nil || output.frameLength == 0 { return nil }
        let byteCount = Int(output.frameLength) * MemoryLayout<Int16>.size
        guard let address = output.int16ChannelData?.pointee else { return nil }
        return Data(bytes: address, count: byteCount)
    }
}
