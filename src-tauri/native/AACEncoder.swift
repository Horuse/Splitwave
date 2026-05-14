// AACEncoder.swift
//
// Streaming AAC-LC encoder writing to M4A files via AVAudioFile. Accepts
// interleaved float32 PCM via the C ABI; AVAudioFile + CoreAudio handle the
// encode + container muxing internally.

import AVFoundation
import Foundation

private final class AACEncoderState {
    let file: AVAudioFile
    let format: AVAudioFormat

    init(file: AVAudioFile, format: AVAudioFormat) {
        self.file = file
        self.format = format
    }
}

@_cdecl("ba_aac_create")
public func ba_aac_create(
    _ pathC: UnsafePointer<CChar>,
    _ sampleRate: Int32,
    _ channels: Int32,
    _ bitrate: Int32
) -> OpaquePointer? {
    let path = String(cString: pathC)
    let url = URL(fileURLWithPath: path)

    let settings: [String: Any] = [
        AVFormatIDKey: kAudioFormatMPEG4AAC,
        AVSampleRateKey: Int(sampleRate),
        AVNumberOfChannelsKey: Int(channels),
        AVEncoderBitRateKey: Int(bitrate)
    ]

    guard let format = AVAudioFormat(
        commonFormat: .pcmFormatFloat32,
        sampleRate: Double(sampleRate),
        channels: AVAudioChannelCount(channels),
        interleaved: true
    ) else { return nil }

    do {
        let file = try AVAudioFile(
            forWriting: url,
            settings: settings,
            commonFormat: .pcmFormatFloat32,
            interleaved: true
        )
        let state = AACEncoderState(file: file, format: format)
        return OpaquePointer(Unmanaged.passRetained(state).toOpaque())
    } catch {
        return nil
    }
}

@_cdecl("ba_aac_write")
public func ba_aac_write(
    _ handle: OpaquePointer,
    _ samples: UnsafePointer<Float>,
    _ frames: Int32
) -> Int32 {
    let state = Unmanaged<AACEncoderState>.fromOpaque(UnsafeRawPointer(handle))
        .takeUnretainedValue()
    let frameCount = AVAudioFrameCount(frames)
    guard let buffer = AVAudioPCMBuffer(pcmFormat: state.format, frameCapacity: frameCount)
    else { return -1 }
    buffer.frameLength = frameCount

    let total = Int(frames) * Int(state.format.channelCount)
    if let dst = buffer.floatChannelData?.pointee {
        dst.update(from: samples, count: total)
    } else {
        return -2
    }

    do {
        try state.file.write(from: buffer)
        return 0
    } catch {
        return -3
    }
}

@_cdecl("ba_aac_destroy")
public func ba_aac_destroy(_ handle: OpaquePointer) {
    Unmanaged<AACEncoderState>.fromOpaque(UnsafeRawPointer(handle)).release()
}
