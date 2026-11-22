// SCKAudioCapture.swift
//
// Thin Swift bridge between Rust and ScreenCaptureKit for app-specific and
// system-wide audio capture (macOS 13+). Compiled by build.rs into a static
// library via direct `swiftc` (no SwiftPM, to avoid CLT/Xcode tool churn).
//
// Lifecycle: create → start_{app,system} → (audio callbacks fire) → stop →
// destroy. The handle is a retained `SCKAudioCapture` instance opaque to Rust.

import CoreAudioTypes
import CoreMedia
import Foundation
import ScreenCaptureKit

// Mirrors `crate::audio::sck_capture::ResultCode`.
private let RESULT_OK: Int32 = 0
private let RESULT_OS_VERSION: Int32 = 1
private let RESULT_PERMISSION_DENIED: Int32 = 2
private let RESULT_APP_NOT_FOUND: Int32 = 3
private let RESULT_STREAM_ERROR: Int32 = 4
private let RESULT_INTERNAL: Int32 = 5

public typealias AudioSampleCallback = @convention(c) (
    UnsafeMutableRawPointer?,   // user_data (Rust-side ring producer pointer)
    UnsafePointer<Float>?,      // interleaved f32 samples; valid only inside the call
    Int32,                      // number of frames
    Int32                       // channel count
) -> Void

@available(macOS 13.0, *)
private enum FilterMode {
    case system(excludeCurrentApp: Bool)
    case application(bundleId: String)
}

@available(macOS 13.0, *)
private final class Capture: NSObject, SCStreamOutput, SCStreamDelegate {
    private let queue = DispatchQueue(
        label: "com.horuse.splitwave.sck",
        qos: .userInitiated
    )
    private var stream: SCStream?
    private var callback: AudioSampleCallback?
    private var userData: UnsafeMutableRawPointer?

    func start(
        filterMode: FilterMode,
        sampleRate: Int,
        channels: Int,
        callback: @escaping AudioSampleCallback,
        userData: UnsafeMutableRawPointer?,
        completion: @escaping (Int32) -> Void
    ) {
        self.callback = callback
        self.userData = userData

        SCShareableContent.getExcludingDesktopWindows(true, onScreenWindowsOnly: false) { content, error in
            guard let content = content else {
                completion(RESULT_PERMISSION_DENIED)
                return
            }
            guard let display = content.displays.first else {
                completion(RESULT_STREAM_ERROR)
                return
            }

            let filter: SCContentFilter
            switch filterMode {
            case .system(let excludeCurrentApp):
                let myPid = ProcessInfo.processInfo.processIdentifier
                let excluded: [SCRunningApplication] = excludeCurrentApp
                    ? content.applications.filter { $0.processID == myPid }
                    : []
                filter = SCContentFilter(
                    display: display,
                    excludingApplications: excluded,
                    exceptingWindows: []
                )
            case .application(let bundleId):
                guard content.applications.contains(where: { $0.bundleIdentifier == bundleId }) else {
                    completion(RESULT_APP_NOT_FOUND)
                    return
                }
                // Per-app audio on macOS 13: SCContentFilter scopes audio by
                // the captured *content*. The `including:[app]` form only
                // matches apps that have a visible window on the chosen display,
                // so Electron/web apps with hidden or off-screen windows (e.g.
                // YouTube Music when minimised to dock) deliver no audio at all.
                //
                // The robust pattern is to exclude every OTHER application —
                // that way the captured content covers the full display *minus*
                // every other app's audio, leaving only the target's. This
                // works even when the target has no visible window.
                let others = content.applications.filter {
                    $0.bundleIdentifier != bundleId
                }
                filter = SCContentFilter(
                    display: display,
                    excludingApplications: others,
                    exceptingWindows: []
                )
            }

            let config = SCStreamConfiguration()
            config.capturesAudio = true
            config.sampleRate = sampleRate
            config.channelCount = channels
            // Audio-only capture: tiniest frame interval so SCK doesn't waste
            // work producing video frames.
            config.minimumFrameInterval = CMTime(value: 1, timescale: 1)

            let stream = SCStream(filter: filter, configuration: config, delegate: self)
            do {
                try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: self.queue)
            } catch {
                completion(RESULT_STREAM_ERROR)
                return
            }
            stream.startCapture { error in
                if error != nil {
                    completion(RESULT_STREAM_ERROR)
                } else {
                    self.stream = stream
                    completion(RESULT_OK)
                }
            }
        }
    }

    func stop(completion: @escaping () -> Void) {
        guard let stream = self.stream else {
            completion()
            return
        }
        self.stream = nil
        stream.stopCapture { _ in
            self.callback = nil
            self.userData = nil
            completion()
        }
    }

    // MARK: SCStreamOutput

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio else { return }
        guard sampleBuffer.isValid else { return }
        guard let cb = self.callback else { return }

        // Probe the buffer-list size needed (typical SCK delivery is interleaved
        // f32, but we don't assume — we handle planar too in case future SDK
        // versions change it).
        var sizeNeeded = 0
        let probe = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
            sampleBuffer,
            bufferListSizeNeededOut: &sizeNeeded,
            bufferListOut: nil,
            bufferListSize: 0,
            blockBufferAllocator: nil,
            blockBufferMemoryAllocator: nil,
            flags: 0,
            blockBufferOut: nil
        )
        guard probe == noErr, sizeNeeded > 0 else { return }

        let listPtr = UnsafeMutablePointer<UInt8>.allocate(capacity: sizeNeeded)
        defer { listPtr.deallocate() }
        let listMut = UnsafeMutableRawPointer(listPtr).assumingMemoryBound(to: AudioBufferList.self)
        var blockOut: CMBlockBuffer?
        let status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
            sampleBuffer,
            bufferListSizeNeededOut: nil,
            bufferListOut: listMut,
            bufferListSize: sizeNeeded,
            blockBufferAllocator: nil,
            blockBufferMemoryAllocator: nil,
            flags: 0,
            blockBufferOut: &blockOut
        )
        guard status == noErr else { return }
        _ = blockOut  // keep alive through the rest of this scope

        guard let formatDesc = CMSampleBufferGetFormatDescription(sampleBuffer),
              let asbd = formatDesc.audioStreamBasicDescription else { return }
        let isFloat = (asbd.mFormatFlags & kAudioFormatFlagIsFloat) != 0
        guard isFloat, asbd.mBitsPerChannel == 32 else { return }
        let isInterleaved = (asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved) == 0
        let channelCount = Int(asbd.mChannelsPerFrame)
        let frames = Int(CMSampleBufferGetNumSamples(sampleBuffer))
        guard frames > 0, channelCount > 0 else { return }

        let numBuffers = Int(listMut.pointee.mNumberBuffers)
        let buffersPtr = withUnsafeMutablePointer(to: &listMut.pointee.mBuffers) {
            UnsafeMutableBufferPointer<AudioBuffer>(start: $0, count: numBuffers)
        }

        if isInterleaved {
            guard let raw = buffersPtr[0].mData else { return }
            let samples = raw.assumingMemoryBound(to: Float.self)
            cb(self.userData, samples, Int32(frames), Int32(channelCount))
        } else {
            // Planar — interleave into a scratch buffer before dispatching.
            var scratch = [Float](repeating: 0, count: frames * channelCount)
            scratch.withUnsafeMutableBufferPointer { dst in
                for ch in 0..<channelCount where ch < numBuffers {
                    guard let raw = buffersPtr[ch].mData else { continue }
                    let src = raw.assumingMemoryBound(to: Float.self)
                    for i in 0..<frames {
                        dst[i * channelCount + ch] = src[i]
                    }
                }
            }
            scratch.withUnsafeBufferPointer { ptr in
                cb(self.userData, ptr.baseAddress, Int32(frames), Int32(channelCount))
            }
        }
    }

    // MARK: SCStreamDelegate

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        // SCK terminated the stream itself (permission revoked, system sleep,
        // etc.). Rust side stops receiving samples; user re-activates to retry.
        _ = error
    }
}

// MARK: C ABI

@_cdecl("ba_sck_create")
public func ba_sck_create() -> OpaquePointer? {
    if #available(macOS 13.0, *) {
        let inst = Capture()
        return OpaquePointer(Unmanaged.passRetained(inst).toOpaque())
    }
    return nil
}

@_cdecl("ba_sck_destroy")
public func ba_sck_destroy(_ handle: OpaquePointer) {
    if #available(macOS 13.0, *) {
        Unmanaged<Capture>.fromOpaque(UnsafeRawPointer(handle)).release()
    }
}

@_cdecl("ba_sck_start_app")
public func ba_sck_start_app(
    _ handle: OpaquePointer,
    _ bundleIdC: UnsafePointer<CChar>,
    _ sampleRate: Int32,
    _ channels: Int32,
    _ callback: @escaping AudioSampleCallback,
    _ userData: UnsafeMutableRawPointer?
) -> Int32 {
    if #available(macOS 13.0, *) {
        let inst = Unmanaged<Capture>.fromOpaque(UnsafeRawPointer(handle)).takeUnretainedValue()
        let bundleId = String(cString: bundleIdC)
        let sem = DispatchSemaphore(value: 0)
        var result: Int32 = RESULT_INTERNAL
        inst.start(
            filterMode: .application(bundleId: bundleId),
            sampleRate: Int(sampleRate),
            channels: Int(channels),
            callback: callback,
            userData: userData
        ) { code in
            result = code
            sem.signal()
        }
        _ = sem.wait(timeout: .now() + .seconds(10))
        return result
    }
    return RESULT_OS_VERSION
}

@_cdecl("ba_sck_start_system")
public func ba_sck_start_system(
    _ handle: OpaquePointer,
    _ excludeCurrentApp: Int32,
    _ sampleRate: Int32,
    _ channels: Int32,
    _ callback: @escaping AudioSampleCallback,
    _ userData: UnsafeMutableRawPointer?
) -> Int32 {
    if #available(macOS 13.0, *) {
        let inst = Unmanaged<Capture>.fromOpaque(UnsafeRawPointer(handle)).takeUnretainedValue()
        let sem = DispatchSemaphore(value: 0)
        var result: Int32 = RESULT_INTERNAL
        inst.start(
            filterMode: .system(excludeCurrentApp: excludeCurrentApp != 0),
            sampleRate: Int(sampleRate),
            channels: Int(channels),
            callback: callback,
            userData: userData
        ) { code in
            result = code
            sem.signal()
        }
        _ = sem.wait(timeout: .now() + .seconds(10))
        return result
    }
    return RESULT_OS_VERSION
}

/// Returns 0 on clean stop (no further callbacks will fire), 1 on timeout
/// (Swift's queue may still drain pending sample buffers — Rust caller must
/// not free user_data in this case).
@_cdecl("ba_sck_stop")
public func ba_sck_stop(_ handle: OpaquePointer) -> Int32 {
    if #available(macOS 13.0, *) {
        let inst = Unmanaged<Capture>.fromOpaque(UnsafeRawPointer(handle)).takeUnretainedValue()
        let sem = DispatchSemaphore(value: 0)
        inst.stop {
            sem.signal()
        }
        let result = sem.wait(timeout: .now() + .seconds(5))
        return result == .success ? 0 : 1
    }
    return 0
}
