// CATapCapture.swift
//
// Core Audio process taps (macOS 14.4+) for per-app and system audio capture.
// Replaces the ScreenCaptureKit path on hosts that support it: taps are scoped
// to process objects at the HAL, so no other app's audio can enter the mix, and
// they need only the System Audio Recording TCC permission.
//
// Lifecycle: create -> start_{app,system} -> (IOProc callbacks) -> stop ->
// destroy. Handle is a retained `Tap` instance opaque to Rust.

import AppKit
import AudioToolbox
import CoreAudio
import Darwin
import Foundation

// Mirrors `crate::audio::capture::macos_tap::ResultCode`.
private let RESULT_OK: Int32 = 0
private let RESULT_OS_VERSION: Int32 = 1
private let RESULT_PERMISSION_DENIED: Int32 = 2
private let RESULT_APP_NOT_FOUND: Int32 = 3
private let RESULT_TAP_ERROR: Int32 = 4
private let RESULT_INTERNAL: Int32 = 5

public typealias TapSampleCallback = @convention(c) (
    UnsafeMutableRawPointer?,   // user_data (Rust-side bridge pointer)
    UnsafePointer<Float>?,      // interleaved f32; valid only inside the call
    Int32,                      // frames
    Int32                       // channels
) -> Void

private let systemObject = AudioObjectID(kAudioObjectSystemObject)

private func address(
    _ selector: AudioObjectPropertySelector,
    _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal
) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress(
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain
    )
}

private func readValue<T>(_ objectID: AudioObjectID, _ selector: AudioObjectPropertySelector, _ initial: T) -> T? {
    var addr = address(selector)
    var value = initial
    var size = UInt32(MemoryLayout<T>.size)
    let err = withUnsafeMutablePointer(to: &value) {
        AudioObjectGetPropertyData(objectID, &addr, 0, nil, &size, $0)
    }
    return err == noErr ? value : nil
}

private func readString(_ objectID: AudioObjectID, _ selector: AudioObjectPropertySelector) -> String? {
    var addr = address(selector)
    var value: CFString = "" as CFString
    var size = UInt32(MemoryLayout<CFString>.size)
    let err = withUnsafeMutablePointer(to: &value) {
        AudioObjectGetPropertyData(objectID, &addr, 0, nil, &size, $0)
    }
    guard err == noErr else { return nil }
    let string = value as String
    return string.isEmpty ? nil : string
}

private func readObjectList(_ objectID: AudioObjectID, _ selector: AudioObjectPropertySelector) -> [AudioObjectID] {
    var addr = address(selector)
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(objectID, &addr, 0, nil, &size) == noErr, size > 0 else {
        return []
    }
    var ids = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
    guard AudioObjectGetPropertyData(objectID, &addr, 0, nil, &size, &ids) == noErr else {
        return []
    }
    return ids
}

/// Number of `AudioBuffer`s the device contributes to an IOProc's input list.
/// The aggregate lays out its main sub-device's input buffers before the tap's,
/// so this is the tap's starting index.
private func inputBufferCount(_ deviceID: AudioDeviceID) -> Int {
    var addr = address(kAudioDevicePropertyStreamConfiguration, kAudioObjectPropertyScopeInput)
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(deviceID, &addr, 0, nil, &size) == noErr, size > 0 else {
        return 0
    }
    let raw = UnsafeMutableRawPointer.allocate(byteCount: Int(size), alignment: MemoryLayout<AudioBufferList>.alignment)
    defer { raw.deallocate() }
    guard AudioObjectGetPropertyData(deviceID, &addr, 0, nil, &size, raw) == noErr else { return 0 }
    return Int(raw.assumingMemoryBound(to: AudioBufferList.self).pointee.mNumberBuffers)
}

private func defaultOutputDevice() -> AudioDeviceID? {
    guard let id = readValue(systemObject, kAudioHardwarePropertyDefaultOutputDevice, AudioDeviceID(0)),
          id != AudioDeviceID(kAudioObjectUnknown) else { return nil }
    return id
}

private typealias ResponsibleForPID = @convention(c) (pid_t) -> pid_t

/// `responsibility_get_pid_responsible_for_pid` maps a helper process (browser
/// renderer, XPC service) back to the app that spawned it. Core Audio exposes
/// no equivalent, and helpers carry their own bundle IDs.
private let responsibleForPID: ResponsibleForPID? = {
    guard let handle = dlopen(nil, RTLD_NOW),
          let sym = dlsym(handle, "responsibility_get_pid_responsible_for_pid") else { return nil }
    return unsafeBitCast(sym, to: ResponsibleForPID.self)
}()

private func processPID(_ processObject: AudioObjectID) -> pid_t? {
    readValue(processObject, kAudioProcessPropertyPID, pid_t(0))
}

private func isRunningOutput(_ processObject: AudioObjectID) -> Bool {
    (readValue(processObject, kAudioProcessPropertyIsRunningOutput, UInt32(0)) ?? 0) != 0
}

/// Process objects whose audio belongs to `bundleID`. Membership is the union
/// of three checks, all always evaluated: exact bundle ID, helper bundle ID
/// prefixed by the target (`com.google.Chrome.helper`), and responsible-PID
/// ownership (covers helpers with unrelated IDs, e.g. `com.apple.WebKit.GPU`).
@available(macOS 14.4, *)
private func candidateProcesses(forBundleID bundleID: String) -> [AudioObjectID] {
    let appPIDs = Set(
        NSRunningApplication.runningApplications(withBundleIdentifier: bundleID)
            .map { $0.processIdentifier }
    )
    let helperPrefix = bundleID + "."

    return readObjectList(systemObject, kAudioHardwarePropertyProcessObjectList).filter { object in
        if let id = readString(object, kAudioProcessPropertyBundleID) {
            if id == bundleID || id.hasPrefix(helperPrefix) { return true }
        }
        guard let pid = processPID(object) else { return false }
        if appPIDs.contains(pid) { return true }
        guard let responsible = responsibleForPID?(pid), responsible != pid else { return false }
        return appPIDs.contains(responsible)
    }
}

@available(macOS 14.4, *)
private func ownProcessObject() -> AudioObjectID? {
    var pid = getpid()
    var addr = address(kAudioHardwarePropertyTranslatePIDToProcessObject)
    var object = AudioObjectID(kAudioObjectUnknown)
    var size = UInt32(MemoryLayout<AudioObjectID>.size)
    let err = AudioObjectGetPropertyData(
        systemObject,
        &addr,
        UInt32(MemoryLayout<pid_t>.size),
        &pid,
        &size,
        &object
    )
    guard err == noErr, object != AudioObjectID(kAudioObjectUnknown) else { return nil }
    return object
}

@available(macOS 14.4, *)
private enum TapMode {
    case system(excludeCurrentApp: Bool)
    case application(bundleID: String)
}

@available(macOS 14.4, *)
private final class Tap {
    private let queue = DispatchQueue(label: "com.horuse.splitwave.catap", qos: .userInteractive)
    /// The property listener must not share the IOProc queue: teardown blocks
    /// in `AudioDeviceStop` until that queue drains, and a listener waiting on
    /// `lock` there would deadlock against `stop()`.
    private let controlQueue = DispatchQueue(label: "com.horuse.splitwave.catap.control")
    /// Serialises start/stop/rebuild. Never taken on the IOProc queue, so
    /// `deliver` cannot block graph teardown.
    private let lock = NSLock()

    private var tapID = AudioObjectID(kAudioObjectUnknown)
    private var aggregateID = AudioDeviceID(0)
    private var ioProcID: AudioDeviceIOProcID?
    private var listenerBlock: AudioObjectPropertyListenerBlock?

    private var callback: TapSampleCallback?
    private var userData: UnsafeMutableRawPointer?
    private var mode: TapMode = .system(excludeCurrentApp: true)
    /// Every process object belonging to the target app.
    private var candidates: [AudioObjectID] = []
    /// The subset actually rendering output, which is what the tap is built
    /// from. Including an idle helper silently kills the whole tap: the
    /// aggregate is created without error but its IOProc never fires.
    private var active: Set<AudioObjectID> = []
    private var pollTimer: DispatchSourceTimer?

    private var tapChannels = 0
    private var tapBufferIndex = 0
    private var sampleRate = 0.0
    /// Preallocated so the IOProc never allocates while de-interleaving.
    private var scratch = [Float]()

    func start(
        mode: TapMode,
        callback: @escaping TapSampleCallback,
        userData: UnsafeMutableRawPointer?
    ) -> Int32 {
        lock.lock()
        defer { lock.unlock() }

        self.mode = mode
        self.callback = callback
        self.userData = userData

        let code = build()
        guard code == RESULT_OK else { return code }
        observeProcesses()
        return RESULT_OK
    }

    func stop() {
        lock.lock()
        defer { lock.unlock() }
        removeProcessObservers()
        teardown()
        callback = nil
        userData = nil
    }

    var format: (rate: Double, channels: Int) {
        lock.lock()
        defer { lock.unlock() }
        return (sampleRate, tapChannels)
    }

    // MARK: Graph

    private func targetCandidates() -> [AudioObjectID] {
        switch mode {
        case .system:
            return []
        case .application(let bundleID):
            return candidateProcesses(forBundleID: bundleID)
        }
    }

    private func description(for processes: [AudioObjectID]) -> CATapDescription {
        let desc: CATapDescription
        switch mode {
        case .system(let excludeCurrentApp):
            // Empty exclusion list means the whole system mix.
            let excluded = excludeCurrentApp ? [ownProcessObject()].compactMap { $0 } : []
            desc = CATapDescription(stereoGlobalTapButExcludeProcesses: excluded)
        case .application:
            desc = CATapDescription(stereoMixdownOfProcesses: processes)
        }
        desc.uuid = UUID()
        desc.name = "Splitwave Capture"
        desc.isPrivate = true
        // Leave the process's own output audible; the initialiser already set
        // isExclusive to match the chosen mode and flipping it inverts the
        // process list into silence.
        desc.muteBehavior = .unmuted
        return desc
    }

    private func build() -> Int32 {
        candidates = targetCandidates()
        if case .application = mode, candidates.isEmpty {
            return RESULT_APP_NOT_FOUND
        }
        // An app that is running but silent yields an empty set; the tap is
        // still valid and starts delivering once a process begins output.
        active = Set(candidates.filter(isRunningOutput))

        let desc = description(for: Array(active))
        var tap = AudioObjectID(kAudioObjectUnknown)
        let tapErr = AudioHardwareCreateProcessTap(desc, &tap)
        guard tapErr == noErr, tap != AudioObjectID(kAudioObjectUnknown) else {
            return tapErr == kAudioHardwareIllegalOperationError
                ? RESULT_PERMISSION_DENIED
                : RESULT_TAP_ERROR
        }
        tapID = tap

        guard let asbd = readValue(tapID, kAudioTapPropertyFormat, AudioStreamBasicDescription()),
              asbd.mChannelsPerFrame > 0,
              asbd.mSampleRate > 0,
              (asbd.mFormatFlags & kAudioFormatFlagIsFloat) != 0,
              asbd.mBitsPerChannel == 32
        else {
            teardown()
            return RESULT_TAP_ERROR
        }
        tapChannels = Int(asbd.mChannelsPerFrame)
        sampleRate = asbd.mSampleRate

        // The tap rides on a real output device; a tap-only aggregate yields
        // silence.
        guard let outputDevice = defaultOutputDevice(),
              let outputUID = readString(outputDevice, kAudioDevicePropertyDeviceUID)
        else {
            teardown()
            return RESULT_TAP_ERROR
        }
        tapBufferIndex = inputBufferCount(outputDevice)

        let aggregateUID = "com.horuse.splitwave.tap.\(UUID().uuidString)"
        let spec: [String: Any] = [
            kAudioAggregateDeviceNameKey: "Splitwave Tap",
            kAudioAggregateDeviceUIDKey: aggregateUID,
            kAudioAggregateDeviceMainSubDeviceKey: outputUID,
            kAudioAggregateDeviceIsPrivateKey: true,
            kAudioAggregateDeviceIsStackedKey: false,
            kAudioAggregateDeviceTapAutoStartKey: true,
            kAudioAggregateDeviceSubDeviceListKey: [[kAudioSubDeviceUIDKey: outputUID]],
            kAudioAggregateDeviceTapListKey: [[
                kAudioSubTapUIDKey: desc.uuid.uuidString,
                kAudioSubTapDriftCompensationKey: true,
            ]],
        ]
        var aggregate = AudioDeviceID(0)
        guard AudioHardwareCreateAggregateDevice(spec as CFDictionary, &aggregate) == noErr,
              aggregate != AudioDeviceID(0)
        else {
            teardown()
            return RESULT_TAP_ERROR
        }
        aggregateID = aggregate

        let maxFrames = Int(readValue(aggregateID, kAudioDevicePropertyBufferFrameSize, UInt32(0)) ?? 4096)
        scratch = [Float](repeating: 0, count: max(maxFrames, 4096) * tapChannels)

        var procID: AudioDeviceIOProcID?
        let procErr = AudioDeviceCreateIOProcIDWithBlock(&procID, aggregateID, queue) {
            [weak self] _, inInputData, _, _, _ in
            self?.deliver(inInputData)
        }
        guard procErr == noErr, let procID else {
            teardown()
            return RESULT_TAP_ERROR
        }
        ioProcID = procID

        guard AudioDeviceStart(aggregateID, procID) == noErr else {
            teardown()
            return RESULT_TAP_ERROR
        }
        return RESULT_OK
    }

    /// Order matters: a tap destroyed before its aggregate leaves the aggregate
    /// stranded in the HAL and the next create fails.
    private func teardown() {
        if aggregateID != 0, let procID = ioProcID {
            AudioDeviceStop(aggregateID, procID)
            AudioDeviceDestroyIOProcID(aggregateID, procID)
        }
        ioProcID = nil
        if aggregateID != 0 {
            AudioHardwareDestroyAggregateDevice(aggregateID)
            aggregateID = 0
        }
        if tapID != AudioObjectID(kAudioObjectUnknown) {
            AudioHardwareDestroyProcessTap(tapID)
            tapID = AudioObjectID(kAudioObjectUnknown)
        }
    }

    // MARK: Process list

    /// A tap's process list is fixed at creation, so the graph must be rebuilt
    /// when the app opens a new audio process (extra browser tab) and when one
    /// of its processes starts or stops rendering output.
    ///
    /// The process list posts change notifications, but `IsRunningOutput` was
    /// observed never to post one, so a long-lived process going from paused to
    /// playing would be missed. Both are therefore re-evaluated on a timer;
    /// these are cheap property reads off the IO queue.
    private func observeProcesses() {
        guard case .application = mode else { return }
        var addr = address(kAudioHardwarePropertyProcessObjectList)
        let block: AudioObjectPropertyListenerBlock = { [weak self] _, _ in
            self?.rebuildIfTargetChanged()
        }
        listenerBlock = block
        AudioObjectAddPropertyListenerBlock(systemObject, &addr, controlQueue, block)

        let timer = DispatchSource.makeTimerSource(queue: controlQueue)
        timer.schedule(deadline: .now() + 1.0, repeating: 1.0)
        timer.setEventHandler { [weak self] in self?.rebuildIfTargetChanged() }
        timer.resume()
        pollTimer = timer
    }

    private func removeProcessObservers() {
        pollTimer?.cancel()
        pollTimer = nil
        guard let block = listenerBlock else { return }
        var addr = address(kAudioHardwarePropertyProcessObjectList)
        AudioObjectRemovePropertyListenerBlock(systemObject, &addr, controlQueue, block)
        listenerBlock = nil
    }

    private func rebuildIfTargetChanged() {
        lock.lock()
        defer { lock.unlock() }
        guard callback != nil else { return }
        let newCandidates = targetCandidates()
        // An empty candidate set means the app quit; keep the current tap so a
        // restart of the app resumes into a later rebuild.
        guard !newCandidates.isEmpty else { return }
        let newActive = Set(newCandidates.filter(isRunningOutput))
        guard newActive != active || Set(newCandidates) != Set(candidates) else { return }
        // `teardown` returns only once the IOProc queue has drained, so the
        // rebuilt graph never races a delivery.
        teardown()
        _ = build()
    }

    // MARK: IO

    private func deliver(_ inInputData: UnsafePointer<AudioBufferList>) {
        guard let callback else { return }
        let list = UnsafeMutableAudioBufferListPointer(
            UnsafeMutablePointer(mutating: inInputData)
        )
        guard tapBufferIndex < list.count else { return }

        let first = list[tapBufferIndex]
        guard let firstData = first.mData, first.mDataByteSize > 0 else { return }

        if Int(first.mNumberChannels) == tapChannels {
            let frames = Int(first.mDataByteSize) / (MemoryLayout<Float>.size * tapChannels)
            guard frames > 0 else { return }
            callback(userData, firstData.assumingMemoryBound(to: Float.self), Int32(frames), Int32(tapChannels))
            return
        }

        // Planar: one buffer per channel starting at tapBufferIndex.
        guard tapBufferIndex + tapChannels <= list.count else { return }
        let frames = Int(first.mDataByteSize) / MemoryLayout<Float>.size
        guard frames > 0, frames * tapChannels <= scratch.count else { return }
        scratch.withUnsafeMutableBufferPointer { dst in
            for channel in 0..<tapChannels {
                guard let src = list[tapBufferIndex + channel].mData?.assumingMemoryBound(to: Float.self) else {
                    continue
                }
                for frame in 0..<frames {
                    dst[frame * tapChannels + channel] = src[frame]
                }
            }
            callback(userData, dst.baseAddress, Int32(frames), Int32(tapChannels))
        }
    }
}

// MARK: C ABI

/// 1 when process taps are available on this host.
@_cdecl("ba_tap_available")
public func ba_tap_available() -> Int32 {
    if #available(macOS 14.4, *) { return 1 }
    return 0
}

/// Sample rate of a process tap's own format. This deliberately does not read
/// the default output device's nominal rate: aggregate/tap creation can expose
/// a different conversion format, and the Rust graph needs the rate the tap
/// will actually deliver before it wires source resamplers.
@_cdecl("ba_tap_default_rate")
public func ba_tap_default_rate() -> Double {
    if #available(macOS 14.4, *) {
        let desc = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
        desc.uuid = UUID()
        desc.name = "Splitwave Format Probe"
        desc.isPrivate = true
        desc.muteBehavior = .unmuted

        var tap = AudioObjectID(kAudioObjectUnknown)
        guard AudioHardwareCreateProcessTap(desc, &tap) == noErr,
              tap != AudioObjectID(kAudioObjectUnknown)
        else { return 0 }
        defer { AudioHardwareDestroyProcessTap(tap) }

        guard let asbd = readValue(tap, kAudioTapPropertyFormat, AudioStreamBasicDescription()),
              asbd.mSampleRate > 0
        else { return 0 }
        return asbd.mSampleRate
    }
    return 0
}

@_cdecl("ba_tap_create")
public func ba_tap_create() -> OpaquePointer? {
    if #available(macOS 14.4, *) {
        return OpaquePointer(Unmanaged.passRetained(Tap()).toOpaque())
    }
    return nil
}

@_cdecl("ba_tap_destroy")
public func ba_tap_destroy(_ handle: OpaquePointer) {
    if #available(macOS 14.4, *) {
        Unmanaged<Tap>.fromOpaque(UnsafeRawPointer(handle)).release()
    }
}

@_cdecl("ba_tap_start_app")
public func ba_tap_start_app(
    _ handle: OpaquePointer,
    _ bundleIDC: UnsafePointer<CChar>,
    _ callback: @escaping TapSampleCallback,
    _ userData: UnsafeMutableRawPointer?
) -> Int32 {
    if #available(macOS 14.4, *) {
        let tap = Unmanaged<Tap>.fromOpaque(UnsafeRawPointer(handle)).takeUnretainedValue()
        return tap.start(
            mode: .application(bundleID: String(cString: bundleIDC)),
            callback: callback,
            userData: userData
        )
    }
    return RESULT_OS_VERSION
}

@_cdecl("ba_tap_start_system")
public func ba_tap_start_system(
    _ handle: OpaquePointer,
    _ excludeCurrentApp: Int32,
    _ callback: @escaping TapSampleCallback,
    _ userData: UnsafeMutableRawPointer?
) -> Int32 {
    if #available(macOS 14.4, *) {
        let tap = Unmanaged<Tap>.fromOpaque(UnsafeRawPointer(handle)).takeUnretainedValue()
        return tap.start(
            mode: .system(excludeCurrentApp: excludeCurrentApp != 0),
            callback: callback,
            userData: userData
        )
    }
    return RESULT_OS_VERSION
}

/// Writes the running tap's format. Valid only after a successful start.
@_cdecl("ba_tap_format")
public func ba_tap_format(
    _ handle: OpaquePointer,
    _ sampleRateOut: UnsafeMutablePointer<Double>,
    _ channelsOut: UnsafeMutablePointer<Int32>
) -> Int32 {
    if #available(macOS 14.4, *) {
        let tap = Unmanaged<Tap>.fromOpaque(UnsafeRawPointer(handle)).takeUnretainedValue()
        let format = tap.format
        guard format.channels > 0, format.rate > 0 else { return RESULT_INTERNAL }
        sampleRateOut.pointee = format.rate
        channelsOut.pointee = Int32(format.channels)
        return RESULT_OK
    }
    return RESULT_OS_VERSION
}

/// Blocks until the IOProc is torn down; no further callbacks fire after it
/// returns, so Rust may free user_data.
@_cdecl("ba_tap_stop")
public func ba_tap_stop(_ handle: OpaquePointer) {
    if #available(macOS 14.4, *) {
        Unmanaged<Tap>.fromOpaque(UnsafeRawPointer(handle)).takeUnretainedValue().stop()
    }
}
