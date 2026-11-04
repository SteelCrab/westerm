// Makes the 1024 px app icon from a square source image whose rounded body
// sits on black: finds the body, crops to it, cuts the corners with a
// transparent mask, and places it on Apple's icon grid (an 824 px body with
// a 100 px transparent margin).
//
//   swift macos/make-icon.swift macos/icon-source.jpg macos/AppIcon.png

import AppKit

let arguments = CommandLine.arguments
guard arguments.count == 3 else {
    FileHandle.standardError.write("usage: make-icon.swift <source image> <out.png>\n".data(using: .utf8)!)
    exit(2)
}
guard let source = NSImage(contentsOfFile: arguments[1]),
      let image = source.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    FileHandle.standardError.write("cannot read \(arguments[1])\n".data(using: .utf8)!)
    exit(1)
}

// RGBA pixels, first row at the top.
let width = image.width
let height = image.height
var pixels = [UInt8](repeating: 0, count: width * height * 4)
let rgb = CGColorSpaceCreateDeviceRGB()
pixels.withUnsafeMutableBytes { buffer in
    let context = CGContext(data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8,
                            bytesPerRow: width * 4, space: rgb,
                            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
}

/// The body is dark navy; the margin is black (with a little JPEG noise).
func isBody(_ x: Int, _ y: Int) -> Bool {
    let i = (y * width + x) * 4
    return max(pixels[i], pixels[i + 1], pixels[i + 2]) > 9
}

var minX = width, minY = height, maxX = -1, maxY = -1
for y in 0..<height {
    for x in 0..<width where isBody(x, y) {
        minX = min(minX, x); maxX = max(maxX, x)
        minY = min(minY, y); maxY = max(maxY, y)
    }
}
guard maxX > minX, maxY > minY else {
    FileHandle.standardError.write("no icon body found\n".data(using: .utf8)!)
    exit(1)
}

// Trim the rim: the source's edge fades into the black margin.
let trim = 8
let crop = CGRect(x: minX + trim, y: minY + trim, width: maxX - minX + 1 - 2 * trim, height: maxY - minY + 1 - 2 * trim)
let body = image.cropping(to: crop)!

let canvas = 1024
let bodySize = 824.0
let bodyRect = CGRect(x: 100, y: 100, width: bodySize, height: bodySize)

func render(radius: Double) -> CGContext {
    let context = CGContext(data: nil, width: canvas, height: canvas, bitsPerComponent: 8, bytesPerRow: canvas * 4,
                            space: rgb, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.addPath(CGPath(roundedRect: bodyRect, cornerWidth: radius, cornerHeight: radius, transform: nil))
    context.clip()
    context.interpolationQuality = .high
    context.draw(body, in: bodyRect)
    return context
}

/// Opaque pixels as dark as the black margin in the four corner squares:
/// margin the mask did not cut. (The body's own gradient is dark too, so
/// only the corners count.)
func marginPixels(_ context: CGContext) -> Int {
    let data = context.data!.assumingMemoryBound(to: UInt8.self)
    let corner = 280
    let low = 100 + corner, high = canvas - 100 - corner
    var count = 0
    for y in 100..<(canvas - 100) {
        for x in 100..<(canvas - 100) where (x < low || x >= high) && (y < low || y >= high) {
            let i = (y * canvas + x) * 4
            if data[i + 3] == 255 && max(data[i], data[i + 1], data[i + 2]) < 6 { count += 1 }
        }
    }
    return count
}

// Apple's grid uses a corner radius of about 185 px for an 824 px body. The
// source's corners are not exact circles, so grow the radius while that
// still removes dark pixels. Those left over are JPEG ringing around the
// white strokes, which a larger radius does not change.
var radius = 185.0
var context = render(radius: radius)
var dark = marginPixels(context)
while dark > 0 && radius < 300 {
    let next = render(radius: radius + 4)
    let nextDark = marginPixels(next)
    if nextDark >= dark { break }
    radius += 4
    context = next
    dark = nextDark
}

let output = URL(fileURLWithPath: arguments[2])
guard let destination = CGImageDestinationCreateWithURL(output as CFURL, "public.png" as CFString, 1, nil) else {
    FileHandle.standardError.write("cannot write \(arguments[2])\n".data(using: .utf8)!)
    exit(1)
}
CGImageDestinationAddImage(destination, context.makeImage()!, nil)
guard CGImageDestinationFinalize(destination) else { exit(1) }
print("icon: body \(Int(crop.width)) px, mask radius \(Int(radius)) px → \(arguments[2])")
