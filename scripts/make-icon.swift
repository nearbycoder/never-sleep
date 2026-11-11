#!/usr/bin/env swift
// Build all native macOS icon sizes from the committed master artwork.
// Run from the project root: xcrun swift scripts/make-icon.swift target/app-icon
import CoreGraphics
import CoreImage
import Foundation
import ImageIO
import UniformTypeIdentifiers

guard CommandLine.arguments.count == 2 else {
    fputs("Usage: make-icon.swift OUTPUT_DIRECTORY\n", stderr)
    exit(2)
}

func failure(_ message: String) -> NSError {
    NSError(domain: "NeverSleep.Icon", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
}

func makeIconset() throws {
    let artwork = URL(fileURLWithPath: "assets/app-icon.png")
    guard let source = CGImageSourceCreateWithURL(artwork as CFURL, nil),
          let type = CGImageSourceGetType(source),
          type as String == UTType.png.identifier,
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
        throw failure("Cannot read assets/app-icon.png. Run from the project root with the master PNG in place.")
    }
    guard image.width == image.height, image.width >= 1024 else {
        throw failure("assets/app-icon.png must be square and at least 1024 × 1024 pixels; found \(image.width) × \(image.height).")
    }

    let output = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
    let iconset = output.appendingPathComponent("NeverSleep.iconset", isDirectory: true)
    try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)

    let colorSpace = CGColorSpace(name: CGColorSpace.sRGB)!
    let context = CIContext(options: [
        .workingColorSpace: colorSpace,
        .outputColorSpace: colorSpace,
        .cacheIntermediates: false,
    ])
    let master = CIImage(cgImage: image)
    // Render each size directly from the master with Lanczos filtering. The
    // artwork already contains its safe area; preserve its full extent and alpha.
    var sizes: [Int: Data] = [:]
    for size in [16, 32, 128, 256, 512] {
        for scale in [1, 2] {
            let pixels = size * scale
            let suffix = scale == 2 ? "@2x" : ""
            let destination = iconset.appendingPathComponent("icon_\(size)x\(size)\(suffix).png")
            if sizes[pixels] == nil {
                let resized = master.applyingFilter("CILanczosScaleTransform", parameters: [
                    kCIInputScaleKey: Double(pixels) / Double(image.width),
                    kCIInputAspectRatioKey: 1.0,
                ])
                guard let rendered = context.createCGImage(
                    resized,
                    from: CGRect(x: 0, y: 0, width: pixels, height: pixels),
                    format: .RGBA8,
                    colorSpace: colorSpace
                ) else {
                    throw failure("Could not render the \(pixels)-pixel icon.")
                }
                let png = NSMutableData()
                guard let encoder = CGImageDestinationCreateWithData(png, UTType.png.identifier as CFString, 1, nil) else {
                    throw failure("Could not encode the \(pixels)-pixel icon.")
                }
                CGImageDestinationAddImage(encoder, rendered, nil)
                guard CGImageDestinationFinalize(encoder) else {
                    throw failure("Could not finish the \(pixels)-pixel PNG.")
                }
                sizes[pixels] = png as Data
            }
            try sizes[pixels]!.write(to: destination, options: .atomic)
        }
    }
    print("Created \(iconset.path) from \(artwork.path)")
}

do {
    try makeIconset()
} catch {
    fputs("Icon generation failed: \(error.localizedDescription)\n", stderr)
    exit(1)
}
