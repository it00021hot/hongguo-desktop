import Foundation
import AVFoundation
import CoreMedia
import CoreText
import CoreGraphics
import UniformTypeIdentifiers

// 自研测试样片：25fps 1280x720 H.264 + 440Hz 正弦 AAC，分片间隔 1s。
// 画面是「帧号 + 秒 + 移动竖条 + 进度条」，肉眼即可判断卡顿/跳帧。
let args = CommandLine.arguments
guard args.count >= 2 else { fputs("usage: make-av-spike-sample <out.mp4>\n", stderr); exit(2) }
let outURL = URL(fileURLWithPath: args[1])
try? FileManager.default.removeItem(at: outURL)
try? FileManager.default.createDirectory(at: outURL.deletingLastPathComponent(), withIntermediateDirectories: true)

let width = 1280
let height = 720
let fps: Int32 = 25
let durationSeconds = 20
let totalFrames = Int(fps) * durationSeconds
let sampleRate = 48000.0
let audioFramesPerVideoFrame = 1920  // 48000 / 25

// 用 AVAssetWriter 的分段代理模式产出标准 fMP4（init 段 = ftyp+moov，
// 媒体段 = moof+mdat）；movieFragmentInterval 在本机 macOS 27 上不生效
//（仍写成单 moov），代理模式是 Apple 明确支持的 CMAF 路径。
let writer = try AVAssetWriter(contentType: .mpeg4Movie)
writer.outputFileTypeProfile = .mpeg4AppleHLS
writer.preferredOutputSegmentInterval = CMTime(value: 1, timescale: 1)
writer.initialSegmentStartTime = .zero

final class SegmentCollector: NSObject, AVAssetWriterDelegate {
    let handle: FileHandle
    var initBytes = 0
    var mediaSegments = 0
    init(url: URL) {
        FileManager.default.createFile(atPath: url.path, contents: nil)
        handle = try! FileHandle(forWritingTo: url)
    }
    func assetWriter(_ writer: AVAssetWriter, didOutputSegmentData segmentData: Data, segmentType: AVAssetSegmentType) {
        handle.write(segmentData)
        if segmentType == .initialization {
            initBytes = segmentData.count
        } else {
            mediaSegments += 1
        }
    }
}
let collector = SegmentCollector(url: outURL)
writer.delegate = collector

let videoSettings: [String: Any] = [
    AVVideoCodecKey: AVVideoCodecType.h264,
    AVVideoWidthKey: width,
    AVVideoHeightKey: height,
    AVVideoCompressionPropertiesKey: [
        AVVideoAverageBitRateKey: 3_000_000,
        AVVideoProfileLevelKey: AVVideoProfileLevelH264MainAutoLevel,
        AVVideoMaxKeyFrameIntervalKey: Int(fps) * 2,
    ],
]
let vinput = AVAssetWriterInput(mediaType: .video, outputSettings: videoSettings)
vinput.expectsMediaDataInRealTime = false
let adaptor = AVAssetWriterInputPixelBufferAdaptor(
    assetWriterInput: vinput,
    sourcePixelBufferAttributes: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        kCVPixelBufferWidthKey as String: width,
        kCVPixelBufferHeightKey as String: height,
    ]
)
guard writer.canAdd(vinput) else { fatalError("cannot add video input") }
writer.add(vinput)

let audioSettings: [String: Any] = [
    AVFormatIDKey: kAudioFormatMPEG4AAC,
    AVSampleRateKey: sampleRate,
    AVNumberOfChannelsKey: 2,
    AVEncoderBitRateKey: 128_000,
]
let ainput = AVAssetWriterInput(mediaType: .audio, outputSettings: audioSettings)
ainput.expectsMediaDataInRealTime = false
guard writer.canAdd(ainput) else { fatalError("cannot add audio input") }
writer.add(ainput)

guard writer.startWriting() else { fatalError("startWriting failed: \(String(describing: writer.error))") }
writer.startSession(atSourceTime: .zero)

var asbd = AudioStreamBasicDescription(
    mSampleRate: sampleRate,
    mFormatID: kAudioFormatLinearPCM,
    mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
    mBytesPerPacket: 8,
    mFramesPerPacket: 1,
    mBytesPerFrame: 8,
    mChannelsPerFrame: 2,
    mBitsPerChannel: 32,
    mReserved: 0
)
var audioFormat: CMAudioFormatDescription?
let fmtStatus = CMAudioFormatDescriptionCreate(
    allocator: kCFAllocatorDefault, asbd: &asbd, layoutSize: 0, layout: nil,
    magicCookieSize: 0, magicCookie: nil, extensions: nil, formatDescriptionOut: &audioFormat
)
guard fmtStatus == noErr, let audioFormat else { fatalError("audio format failed \(fmtStatus)") }

let colorSpace = CGColorSpaceCreateDeviceRGB()
var phase = 0.0
let twoPi = Double.pi * 2.0

func makeAudioSample(frameIndex: Int) -> CMSampleBuffer? {
    let bytesPerFrame = 8
    let byteCount = audioFramesPerVideoFrame * bytesPerFrame
    var block: CMBlockBuffer?
    guard CMBlockBufferCreateWithMemoryBlock(
        allocator: kCFAllocatorDefault, memoryBlock: nil, blockLength: byteCount,
        blockAllocator: kCFAllocatorDefault, customBlockSource: nil, offsetToData: 0,
        dataLength: byteCount, flags: 0, blockBufferOut: &block
    ) == noErr, let block else { return nil }
    var samples = [Float](repeating: 0, count: audioFramesPerVideoFrame * 2)
    let inc = twoPi * 440.0 / sampleRate
    for i in 0..<audioFramesPerVideoFrame {
        let v = Float(sin(phase) * 0.25)
        phase += inc
        if phase > twoPi { phase -= twoPi }
        samples[i * 2] = v
        samples[i * 2 + 1] = v
    }
    let ok = samples.withUnsafeBytes { raw -> Bool in
        CMBlockBufferReplaceDataBytes(
            with: raw.baseAddress!, blockBuffer: block,
            offsetIntoDestination: 0, dataLength: byteCount
        ) == noErr
    }
    guard ok else { return nil }
    var timing = CMSampleTimingInfo(
        duration: CMTime(value: 1, timescale: fps),
        presentationTimeStamp: CMTime(value: CMTimeValue(frameIndex), timescale: fps),
        decodeTimeStamp: .invalid
    )
    var size = bytesPerFrame
    var sample: CMSampleBuffer?
    let st = CMSampleBufferCreateReady(
        allocator: kCFAllocatorDefault, dataBuffer: block, formatDescription: audioFormat,
        sampleCount: audioFramesPerVideoFrame, sampleTimingEntryCount: 1,
        sampleTimingArray: &timing, sampleSizeEntryCount: 1, sampleSizeArray: &size,
        sampleBufferOut: &sample
    )
    return st == noErr ? sample : nil
}

func makeFrame(index: Int) -> CVPixelBuffer? {
    guard let pool = adaptor.pixelBufferPool else { return nil }
    var pb: CVPixelBuffer?
    guard CVPixelBufferPoolCreatePixelBuffer(kCFAllocatorDefault, pool, &pb) == kCVReturnSuccess,
          let pb else { return nil }
    CVPixelBufferLockBaseAddress(pb, [])
    defer { CVPixelBufferUnlockBaseAddress(pb, []) }
    guard let ctx = CGContext(
        data: CVPixelBufferGetBaseAddress(pb), width: width, height: height,
        bitsPerComponent: 8, bytesPerRow: CVPixelBufferGetBytesPerRow(pb),
        space: colorSpace,
        bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue
    ) else { return nil }
    let t = Double(index) / Double(fps)
    // 底色每秒换一次，移动竖条 5 秒扫过全屏。
    let tSec = Double(index) / Double(fps)
    let phaseColor = tSec.truncatingRemainder(dividingBy: 4.0) / 4.0
    let bg = CGColor(red: 0.10 + 0.25 * phaseColor, green: 0.18, blue: 0.30 - 0.15 * phaseColor, alpha: 1.0)
    ctx.setFillColor(bg)
    ctx.fill(CGRect(x: 0, y: 0, width: CGFloat(width), height: CGFloat(height)))
    let barX = CGFloat(t.truncatingRemainder(dividingBy: 5.0) / 5.0) * CGFloat(width)
    ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 0.9))
    ctx.fill(CGRect(x: barX - 4, y: 0, width: 8, height: CGFloat(height)))

    let fontKey = NSAttributedString.Key(kCTFontAttributeName as String)
    let colorKey = NSAttributedString.Key(kCTForegroundColorAttributeName as String)
    let attrs: [NSAttributedString.Key: Any] = [
        fontKey: CTFontCreateWithName("Menlo-Bold" as CFString, 120, nil),
        colorKey: CGColor(red: 1, green: 1, blue: 1, alpha: 1),
    ]
    let text = "\(index)" as NSString
    let line = CTLineCreateWithAttributedString(NSAttributedString(string: text as String, attributes: attrs))
    ctx.textPosition = CGPoint(x: 60, y: 300)
    CTLineDraw(line, ctx)

    let sub: [NSAttributedString.Key: Any] = [
        fontKey: CTFontCreateWithName("Menlo" as CFString, 44, nil),
        colorKey: CGColor(red: 1, green: 1, blue: 1, alpha: 1),
    ]
    let subLine = CTLineCreateWithAttributedString(NSAttributedString(
        string: String(format: "t=%.2fs  frame %d / %d", t, index, totalFrames), attributes: sub))
    ctx.textPosition = CGPoint(x: 62, y: 230)
    CTLineDraw(subLine, ctx)

    // 进度条 + 每秒刻度
    ctx.setFillColor(CGColor(red: 0, green: 0, blue: 0, alpha: 0.45))
    ctx.fill(CGRect(x: 0, y: 0, width: CGFloat(width), height: 40))
    ctx.setFillColor(CGColor(red: 0.2, green: 0.9, blue: 0.4, alpha: 1))
    ctx.fill(CGRect(x: 0, y: 0, width: CGFloat(width) * CGFloat(index) / CGFloat(totalFrames), height: 40))
    for s in 0...durationSeconds {
        let x = CGFloat(width) * CGFloat(s) / CGFloat(durationSeconds)
        ctx.setFillColor(CGColor(red: 1, green: 0.85, blue: 0.1, alpha: 1))
        ctx.fill(CGRect(x: x, y: 0, width: 2, height: 40))
    }
    return pb
}

for i in 0..<totalFrames {
    while !vinput.isReadyForMoreMediaData { usleep(500) }
    guard let pb = makeFrame(index: i) else { fatalError("pixel buffer \(i)") }
    let pts = CMTime(value: CMTimeValue(i), timescale: fps)
    guard adaptor.append(pb, withPresentationTime: pts) else {
        fatalError("append video \(i): \(String(describing: writer.error))")
    }
    while !ainput.isReadyForMoreMediaData { usleep(500) }
    guard let audio = makeAudioSample(frameIndex: i), ainput.append(audio) else {
        fatalError("append audio \(i): \(String(describing: writer.error))")
    }
}
vinput.markAsFinished()
ainput.markAsFinished()
let sem = DispatchSemaphore(value: 0)
writer.finishWriting { sem.signal() }
sem.wait()
guard writer.status == .completed else {
    fatalError("writer failed: \(String(describing: writer.error))")
}
print("init=\(collector.initBytes) bytes, \(collector.mediaSegments) media segments")
collector.handle.closeFile()
print("wrote \(outURL.path) \(try! FileManager.default.attributesOfItem(atPath: outURL.path)[.size]!) bytes")
