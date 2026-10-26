// Writes a small H.264 .mp4 of the given length, for the end-to-end tests:
//   swift make-sample.swift <output.mp4> <seconds>
// A plain grey picture whose shade drifts, at 5 frames a second -- enough for
// VLC to play, seek and report a clock, and only a few hundred kilobytes.
import AVFoundation
import CoreVideo

let args = CommandLine.arguments
guard args.count == 3, let seconds = Int(args[2]), seconds > 0 else {
    FileHandle.standardError.write("usage: make-sample.swift <out.mp4> <seconds>\n".data(using: .utf8)!)
    exit(2)
}
let url = URL(fileURLWithPath: args[1])
try? FileManager.default.removeItem(at: url)

let width = 320, height = 180, fps: Int32 = 5
let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
    AVVideoCodecKey: AVVideoCodecType.h264,
    AVVideoWidthKey: width,
    AVVideoHeightKey: height,
])
input.expectsMediaDataInRealTime = false
let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
    kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
    kCVPixelBufferWidthKey as String: width,
    kCVPixelBufferHeightKey as String: height,
])
writer.add(input)
writer.startWriting()
writer.startSession(atSourceTime: .zero)

let frames = seconds * Int(fps)
for frame in 0...frames {
    while !input.isReadyForMoreMediaData { usleep(1000) }
    var buffer: CVPixelBuffer?
    CVPixelBufferPoolCreatePixelBuffer(nil, adaptor.pixelBufferPool!, &buffer)
    guard let buffer else { exit(1) }
    CVPixelBufferLockBaseAddress(buffer, [])
    let shade = UInt8(40 + (frame * 3) % 180)
    memset(CVPixelBufferGetBaseAddress(buffer), Int32(shade),
           CVPixelBufferGetBytesPerRow(buffer) * height)
    CVPixelBufferUnlockBaseAddress(buffer, [])
    adaptor.append(buffer, withPresentationTime: CMTime(value: CMTimeValue(frame), timescale: fps))
}
input.markAsFinished()
let done = DispatchSemaphore(value: 0)
writer.finishWriting { done.signal() }
done.wait()
guard writer.status == .completed else {
    FileHandle.standardError.write("failed: \(String(describing: writer.error))\n".data(using: .utf8)!)
    exit(1)
}
