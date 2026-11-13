import CoreGraphics
import Foundation
import ImageIO

enum PixelTestError: Error {
    case invalidArguments
    case imageUnavailable
    case contextUnavailable
}

func emit(_ value: Any) throws {
    let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
}

func window(for pid: Int) throws {
    let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    let owned = windows.filter {
        ($0[kCGWindowOwnerPID as String] as? Int) == pid &&
        ($0[kCGWindowLayer as String] as? Int) == 0
    }
    let visible = owned.filter {
        guard let bounds = $0[kCGWindowBounds as String] as? [String: Any],
              let width = bounds["Width"] as? Double,
              let height = bounds["Height"] as? Double else { return false }
        return width >= 300 && height >= 200
    }
    guard visible.count == 1, let id = visible.first?[kCGWindowNumber as String] as? Int else {
        try emit(["windowID": NSNull(), "count": visible.count])
        return
    }
    try emit(["windowID": id])
}

func analyze(_ path: String, mode: String) throws -> Bool {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { throw PixelTestError.imageUnavailable }
    let width = image.width
    let height = image.height
    let channels = 4
    var pixels = [UInt8](repeating: 0, count: width * height * channels)
    let bitmapInfo = CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue
    try pixels.withUnsafeMutableBytes { bytes in
        guard let context = CGContext(data: bytes.baseAddress, width: width, height: height, bitsPerComponent: 8,
                                      bytesPerRow: width * channels, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: bitmapInfo)
        else { throw PixelTestError.contextUnavailable }
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    }
    let darkThreshold = 32
    let lowChannel = 90
    let highChannel = 150
    var leftCount = 0
    var rightCount = 0
    var green = 0
    var magenta = 0
    var colored = 0
    var buckets = Set<Int>()
    for y in (height * 3 / 10)..<(height * 7 / 10) {
        for x in (width / 5)..<(width * 4 / 5) {
            let offset = (y * width + x) * channels
            let red = Int(pixels[offset])
            let greenChannel = Int(pixels[offset + 1])
            let blue = Int(pixels[offset + 2])
            if max(red, greenChannel, blue) > darkThreshold { colored += 1 }
            buckets.insert((red / 32) * 64 + (greenChannel / 32) * 8 + blue / 32)
            if x < width * 2 / 5 {
                leftCount += 1
                if red < lowChannel && greenChannel > highChannel && blue < lowChannel { green += 1 }
            }
            if x >= width * 3 / 5 {
                rightCount += 1
                if red > highChannel && greenChannel < lowChannel && blue > highChannel { magenta += 1 }
            }
        }
    }
    let sampleCount = (height * 7 / 10 - height * 3 / 10) * (width * 4 / 5 - width / 5)
    let greenRatio = Double(green) / Double(max(1, leftCount))
    let magentaRatio = Double(magenta) / Double(max(1, rightCount))
    let coloredRatio = Double(colored) / Double(max(1, sampleCount))
    let checkerPass = greenRatio >= 0.8 && magentaRatio >= 0.8
    let hevcPass = coloredRatio >= 0.3 && buckets.count >= 8
    let passed = width >= 300 && height >= 200 && (mode == "checker" ? checkerPass : hevcPass)
    try emit(["passed": passed, "mode": mode, "width": width, "height": height,
              "greenRatio": greenRatio, "magentaRatio": magentaRatio, "coloredRatio": coloredRatio, "colorBuckets": buckets.count])
    return passed
}

do {
    let arguments = Array(CommandLine.arguments.dropFirst())
    if arguments.count == 2, arguments[0] == "window", let pid = Int(arguments[1]), pid > 0 {
        try window(for: pid)
    } else if arguments.count == 3, arguments[0] == "analyze", ["checker", "hevc"].contains(arguments[2]) {
        exit(try analyze(arguments[1], mode: arguments[2]) ? 0 : 1)
    } else {
        throw PixelTestError.invalidArguments
    }
} catch {
    FileHandle.standardError.write(Data("macOS pixel test: \(error)\n".utf8))
    exit(2)
}
