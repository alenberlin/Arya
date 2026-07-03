// Arya system-audio helper.
//
// Captures system audio output via a CoreAudio process tap (macOS 14.2+)
// into a WAV file, out of process so a capture crash can never take down the
// app. Control protocol:
//   args:    --output <wav path> --status <status json path>
//   signals: SIGUSR1 pause, SIGUSR2 resume, SIGTERM finalize + exit
//   status:  one JSON object per line appended to the status file:
//            {"event":"ready"} | {"event":"level","value":0.42}
//            {"event":"error","message":"..."} | {"event":"stopped"}
//
// The WAV header is finalized on clean shutdown; after a crash the parent
// repairs the header from file length (same rule as the mic recorder).

import AVFoundation
import CoreAudio
import Foundation

final class StatusWriter {
    private let handle: FileHandle?

    init(path: String) {
        FileManager.default.createFile(atPath: path, contents: nil)
        handle = FileHandle(forWritingAtPath: path)
    }

    func emit(_ object: [String: Any]) {
        guard let handle,
              let data = try? JSONSerialization.data(withJSONObject: object)
        else { return }
        handle.seekToEndOfFile()
        handle.write(data)
        handle.write(Data("\n".utf8))
    }
}

final class WavFileWriter {
    private let handle: FileHandle
    private var dataBytes: UInt64 = 0
    private let sampleRate: UInt32
    private let channels: UInt16

    init?(path: String, sampleRate: UInt32, channels: UInt16) {
        self.sampleRate = sampleRate
        self.channels = channels
        FileManager.default.createFile(atPath: path, contents: nil)
        guard let handle = FileHandle(forWritingAtPath: path) else { return nil }
        self.handle = handle
        writeHeader(dataLength: 0)
    }

    private func writeHeader(dataLength: UInt32) {
        var header = Data()
        func append(_ string: String) { header.append(Data(string.utf8)) }
        func append32(_ value: UInt32) { withUnsafeBytes(of: value.littleEndian) { header.append(contentsOf: $0) } }
        func append16(_ value: UInt16) { withUnsafeBytes(of: value.littleEndian) { header.append(contentsOf: $0) } }
        let byteRate = sampleRate * UInt32(channels) * 2
        append("RIFF"); append32(36 &+ dataLength); append("WAVE")
        append("fmt "); append32(16); append16(1); append16(channels)
        append32(sampleRate); append32(byteRate); append16(UInt16(channels * 2)); append16(16)
        append("data"); append32(dataLength)
        handle.seek(toFileOffset: 0)
        handle.write(header)
    }

    func write(samples: UnsafeBufferPointer<Float>) {
        var pcm = Data(capacity: samples.count * 2)
        for sample in samples {
            let clamped = max(-1.0, min(1.0, sample))
            var value = Int16(clamped * Float(Int16.max)).littleEndian
            withUnsafeBytes(of: &value) { pcm.append(contentsOf: $0) }
        }
        handle.seekToEndOfFile()
        handle.write(pcm)
        dataBytes &+= UInt64(pcm.count)
    }

    func finalize() {
        writeHeader(dataLength: UInt32(min(dataBytes, UInt64(UInt32.max))))
        try? handle.synchronize()
        try? handle.close()
    }
}

func fail(_ status: StatusWriter, _ message: String) -> Never {
    status.emit(["event": "error", "message": message])
    status.emit(["event": "stopped"])
    exit(1)
}

// --- argument parsing ---------------------------------------------------
var outputPath: String?
var statusPath: String?
var arguments = CommandLine.arguments.dropFirst().makeIterator()
while let argument = arguments.next() {
    switch argument {
    case "--output": outputPath = arguments.next()
    case "--status": statusPath = arguments.next()
    default: break
    }
}
guard let outputPath, let statusPath else {
    FileHandle.standardError.write(Data("usage: --output <wav> --status <json>\n".utf8))
    exit(2)
}
let status = StatusWriter(path: statusPath)

guard #available(macOS 14.2, *) else {
    fail(status, "system audio capture requires macOS 14.2 or later")
}

// --- create the process tap ---------------------------------------------
let tapDescription = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
tapDescription.isPrivate = true
tapDescription.muteBehavior = .unmuted

var tapID = AudioObjectID(kAudioObjectUnknown)
var err = AudioHardwareCreateProcessTap(tapDescription, &tapID)
guard err == noErr, tapID != kAudioObjectUnknown else {
    fail(status, "AudioHardwareCreateProcessTap failed (\(err)) - system audio permission may be missing")
}

// Read the tap's stream format so the writer matches reality.
var format = AudioStreamBasicDescription()
var formatSize = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
var formatAddress = AudioObjectPropertyAddress(
    mSelector: kAudioTapPropertyFormat,
    mScope: kAudioObjectPropertyScopeGlobal,
    mElement: kAudioObjectPropertyElementMain
)
err = AudioObjectGetPropertyData(tapID, &formatAddress, 0, nil, &formatSize, &format)
guard err == noErr else {
    AudioHardwareDestroyProcessTap(tapID)
    fail(status, "failed to read tap format (\(err))")
}
let sampleRate = UInt32(format.mSampleRate)
let channels = UInt16(max(1, format.mChannelsPerFrame))

// --- aggregate device wrapping the tap ------------------------------------
let aggregateUID = UUID().uuidString
let description: [String: Any] = [
    kAudioAggregateDeviceUIDKey: aggregateUID,
    kAudioAggregateDeviceNameKey: "Arya system tap",
    kAudioAggregateDeviceIsPrivateKey: true,
    kAudioAggregateDeviceTapAutoStartKey: true,
    kAudioAggregateDeviceTapListKey: [
        [kAudioSubTapUIDKey: tapDescription.uuid.uuidString]
    ],
]
var aggregateID = AudioObjectID(kAudioObjectUnknown)
err = AudioHardwareCreateAggregateDevice(description as CFDictionary, &aggregateID)
guard err == noErr, aggregateID != kAudioObjectUnknown else {
    AudioHardwareDestroyProcessTap(tapID)
    fail(status, "AudioHardwareCreateAggregateDevice failed (\(err))")
}

guard let writer = WavFileWriter(path: outputPath, sampleRate: sampleRate, channels: channels) else {
    fail(status, "cannot open output wav at \(outputPath)")
}

// --- IO proc: interleave tap buffers into the wav -------------------------
var paused = false
var levelCounter = 0
var procID: AudioDeviceIOProcID?
err = AudioDeviceCreateIOProcIDWithBlock(&procID, aggregateID, nil) { _, inInputData, _, _, _ in
    if paused { return }
    let bufferList = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: inInputData))
    for buffer in bufferList {
        guard let data = buffer.mData else { continue }
        let count = Int(buffer.mDataByteSize) / MemoryLayout<Float>.size
        let samples = UnsafeBufferPointer(start: data.assumingMemoryBound(to: Float.self), count: count)
        writer.write(samples: samples)
        levelCounter += 1
        if levelCounter % 25 == 0 {
            var sum: Float = 0
            for sample in samples { sum += sample * sample }
            let rms = count > 0 ? (sum / Float(count)).squareRoot() : 0
            status.emit(["event": "level", "value": Double(min(1.0, rms))])
        }
    }
}
guard err == noErr, let procID else {
    fail(status, "AudioDeviceCreateIOProcIDWithBlock failed (\(err))")
}
err = AudioDeviceStart(aggregateID, procID)
guard err == noErr else {
    fail(status, "AudioDeviceStart failed (\(err))")
}

status.emit(["event": "ready", "sampleRate": Int(sampleRate), "channels": Int(channels)])

// --- signal handling ------------------------------------------------------
let pauseSource = DispatchSource.makeSignalSource(signal: SIGUSR1, queue: .main)
signal(SIGUSR1, SIG_IGN)
pauseSource.setEventHandler { paused = true }
pauseSource.resume()

let resumeSource = DispatchSource.makeSignalSource(signal: SIGUSR2, queue: .main)
signal(SIGUSR2, SIG_IGN)
resumeSource.setEventHandler { paused = false }
resumeSource.resume()

let termSource = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
signal(SIGTERM, SIG_IGN)
termSource.setEventHandler {
    AudioDeviceStop(aggregateID, procID)
    AudioDeviceDestroyIOProcID(aggregateID, procID)
    AudioHardwareDestroyAggregateDevice(aggregateID)
    AudioHardwareDestroyProcessTap(tapID)
    writer.finalize()
    status.emit(["event": "stopped"])
    exit(0)
}
termSource.resume()

RunLoop.main.run()
