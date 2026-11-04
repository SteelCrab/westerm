// Draws the background of the westerm disk image: an arrow from the app to
// the Applications folder and one line of instructions. Finder shows the
// window at 640 × 400 points; this writes that size and a 2x copy.
//
//   swift macos/make-dmg-background.swift <out.png> <out@2x.png>
//
// The icon centers in macos/build-dmg.sh (160, 190 and 480, 190) match the
// arrow here.

import AppKit

let arguments = CommandLine.arguments
guard arguments.count == 3 else {
    FileHandle.standardError.write("usage: make-dmg-background.swift <out.png> <out@2x.png>\n".data(using: .utf8)!)
    exit(2)
}

let width = 640.0
let height = 400.0

func render(scale: Double, to path: String) {
    let pixelsWide = Int(width * scale)
    let pixelsHigh = Int(height * scale)
    let rgb = CGColorSpace(name: CGColorSpace.sRGB)!
    let context = CGContext(data: nil, width: pixelsWide, height: pixelsHigh, bitsPerComponent: 8,
                            bytesPerRow: pixelsWide * 4, space: rgb,
                            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    // Points, with the origin at the top left like Finder's icon positions.
    context.scaleBy(x: scale, y: scale)
    context.translateBy(x: 0, y: height)
    context.scaleBy(x: 1, y: -1)

    // A light gray vertical gradient. Finder draws the icon names over it in
    // dark text, in light and dark mode alike.
    let gradient = CGGradient(colorsSpace: rgb, colors: [
        CGColor(srgbRed: 0.80, green: 0.82, blue: 0.86, alpha: 1),
        CGColor(srgbRed: 0.66, green: 0.69, blue: 0.75, alpha: 1),
    ] as CFArray, locations: [0, 1])!
    context.drawLinearGradient(gradient, start: CGPoint(x: 0, y: 0), end: CGPoint(x: 0, y: height), options: [])

    // The arrow between the two icons (centers at x 160 and 480, y 190).
    let arrowColor = CGColor(srgbRed: 0.20, green: 0.24, blue: 0.32, alpha: 0.75)
    let y = 190.0
    let start = 248.0
    let end = 392.0
    let head = 14.0
    context.setStrokeColor(arrowColor)
    context.setLineWidth(6)
    context.setLineCap(.round)
    context.setLineJoin(.round)
    context.move(to: CGPoint(x: start, y: y))
    context.addLine(to: CGPoint(x: end - 2, y: y))
    context.strokePath()
    context.move(to: CGPoint(x: end - head, y: y - head))
    context.addLine(to: CGPoint(x: end, y: y))
    context.addLine(to: CGPoint(x: end - head, y: y + head))
    context.strokePath()

    // The instructions, centered near the bottom.
    let graphics = NSGraphicsContext(cgContext: context, flipped: true)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = graphics
    let style = NSMutableParagraphStyle()
    style.alignment = .center
    let text = NSAttributedString(string: "Drag westerm to Applications to install", attributes: [
        .font: NSFont.systemFont(ofSize: 15, weight: .medium),
        .foregroundColor: NSColor(srgbRed: 0.13, green: 0.16, blue: 0.22, alpha: 1),
        .paragraphStyle: style,
    ])
    text.draw(in: CGRect(x: 0, y: 318, width: width, height: 24))
    NSGraphicsContext.restoreGraphicsState()

    let url = URL(fileURLWithPath: path)
    guard let destination = CGImageDestinationCreateWithURL(url as CFURL, "public.png" as CFString, 1, nil) else {
        FileHandle.standardError.write("cannot write \(path)\n".data(using: .utf8)!)
        exit(1)
    }
    // 72 dpi per point, so Finder shows both files at 640 × 400 points.
    let dpi = 72 * scale
    CGImageDestinationAddImage(destination, context.makeImage()!, [
        kCGImagePropertyDPIWidth: dpi,
        kCGImagePropertyDPIHeight: dpi,
    ] as CFDictionary)
    guard CGImageDestinationFinalize(destination) else { exit(1) }
}

render(scale: 1, to: arguments[1])
render(scale: 2, to: arguments[2])
