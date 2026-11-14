import AppKit
import Foundation

let values = CommandLine.arguments.dropFirst().prefix(4).compactMap(Double.init)
guard values.count == 4, values.allSatisfy({ $0.isFinite }), values[2] > 0, values[3] > 0 else {
    exit(2)
}
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let primaryTop = NSScreen.screens[0].frame.maxY
let width = CommandLine.arguments.contains("--partial") ? values[2] / 2 : values[2]
let window = NSWindow(
    contentRect: NSRect(x: values[0], y: primaryTop - values[1] - values[3], width: width, height: values[3]),
    styleMask: .borderless,
    backing: .buffered,
    defer: false
)
window.title = "Hongguo independent visibility test cover"
window.isOpaque = !CommandLine.arguments.contains("--nonopaque")
window.backgroundColor = CommandLine.arguments.contains("--translucent") ? NSColor.black.withAlphaComponent(0.99) : .black
window.hasShadow = false
window.level = .floating
window.orderFrontRegardless()
Timer.scheduledTimer(withTimeInterval: 3, repeats: false) { _ in
    window.orderOut(nil)
    app.terminate(nil)
}
app.run()
