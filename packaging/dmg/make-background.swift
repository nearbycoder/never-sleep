#!/usr/bin/env swift
// Native text and vector artwork only. Finder draws the two real file icons.
import AppKit

guard CommandLine.arguments.count == 2 else {
    fputs("Usage: make-background.swift OUTPUT_DIRECTORY\n", stderr)
    exit(2)
}
let output = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
let width: CGFloat = 660
let height: CGFloat = 420

func color(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ alpha: CGFloat = 1) -> NSColor {
    NSColor(srgbRed: r / 255, green: g / 255, blue: b / 255, alpha: alpha)
}

for scale in [1, 2] {
    let factor = CGFloat(scale)
    guard let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil,
        pixelsWide: Int(width) * scale,
        pixelsHigh: Int(height) * scale,
        bitsPerSample: 8,
        samplesPerPixel: 4,
        hasAlpha: true,
        isPlanar: false,
        colorSpaceName: .deviceRGB,
        bytesPerRow: 0,
        bitsPerPixel: 0
    ), let context = NSGraphicsContext(bitmapImageRep: bitmap) else {
        fatalError("Could not allocate the disk image background")
    }
    bitmap.size = NSSize(width: width, height: height)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    let cg = context.cgContext
    cg.scaleBy(x: factor, y: factor)
    let bounds = NSRect(x: 0, y: 0, width: width, height: height)
    NSGradient(starting: color(252, 251, 249), ending: color(242, 241, 238))!
        .draw(in: bounds, angle: -90)

    func centered(_ string: String, top: CGFloat, size: CGFloat, weight: NSFont.Weight, ink: NSColor) {
        let paragraph = NSMutableParagraphStyle()
        paragraph.alignment = .center
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: size, weight: weight),
            .foregroundColor: ink,
            .paragraphStyle: paragraph,
        ]
        let line = NSAttributedString(string: string, attributes: attributes)
        let lineHeight = ceil(line.size().height) + 2
        line.draw(in: NSRect(x: 28, y: height - top - lineHeight, width: width - 56, height: lineHeight))
    }

    centered("Never Sleep", top: 43, size: 30, weight: .semibold, ink: color(35, 34, 32))
    centered("Drag Never Sleep to Applications to install.", top: 88, size: 14, weight: .regular, ink: color(106, 103, 97))

    // A gentle separator and a single arrow leave the icons as the focal point.
    let divider = NSBezierPath()
    divider.lineWidth = 0.5
    divider.move(to: NSPoint(x: 70, y: height - 131))
    divider.line(to: NSPoint(x: 590, y: height - 131))
    color(222, 220, 215).setStroke()
    divider.stroke()

    let arrow = NSBezierPath()
    arrow.lineWidth = 2.5
    arrow.lineCapStyle = .round
    arrow.lineJoinStyle = .round
    arrow.move(to: NSPoint(x: 308, y: height - 220))
    arrow.line(to: NSPoint(x: 350, y: height - 220))
    arrow.move(to: NSPoint(x: 338, y: height - 208))
    arrow.line(to: NSPoint(x: 350, y: height - 220))
    arrow.line(to: NSPoint(x: 338, y: height - 232))
    color(178, 150, 102).setStroke()
    arrow.stroke()

    centered("Once copied, open Never Sleep from Applications.", top: 350, size: 12, weight: .regular, ink: color(133, 129, 122))
    context.flushGraphics()
    NSGraphicsContext.restoreGraphicsState()
    guard let png = bitmap.representation(using: .png, properties: [:]) else {
        fatalError("Could not encode the disk image background")
    }
    let suffix = scale == 2 ? "@2x" : ""
    try png.write(to: output.appendingPathComponent("background\(suffix).png"), options: .atomic)
}
print("Created Retina Finder background in \(output.path)")
