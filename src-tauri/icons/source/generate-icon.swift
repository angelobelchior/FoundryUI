import AppKit
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

let size: CGFloat = 1024
let colorSpace = CGColorSpaceCreateDeviceRGB()
let bitmapInfo = CGImageAlphaInfo.premultipliedLast.rawValue

guard
    let context = CGContext(
        data: nil,
        width: Int(size),
        height: Int(size),
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: colorSpace,
        bitmapInfo: bitmapInfo
    )
else {
    fatalError("could not create the bitmap context")
}

context.translateBy(x: 0, y: size)
context.scaleBy(x: 1, y: -1)
context.setShouldAntialias(true)
context.interpolationQuality = .high

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    let red = CGFloat((hex >> 16) & 0xFF) / 255
    let green = CGFloat((hex >> 8) & 0xFF) / 255
    let blue = CGFloat(hex & 0xFF) / 255

    return CGColor(colorSpace: colorSpace, components: [red, green, blue, alpha])!
}

func gradient(_ stops: [(UInt32, CGFloat)], _ locations: [CGFloat]) -> CGGradient {
    let colors = stops.map { color($0.0, $0.1) } as CFArray

    return CGGradient(colorsSpace: colorSpace, colors: colors, locations: locations)!
}

let plateRect = CGRect(x: 100, y: 100, width: 824, height: 824)
let platePath = CGPath(roundedRect: plateRect, cornerWidth: 185, cornerHeight: 185, transform: nil)

context.saveGState()
context.setShadow(offset: CGSize(width: 0, height: 14), blur: 30, color: color(0x000000, 0.38))
context.addPath(platePath)
context.setFillColor(color(0x000000))
context.fillPath()
context.restoreGState()

let plateGradient = gradient([(0x34343B, 1), (0x1A1A1E, 1)], [0, 1])

context.saveGState()
context.addPath(platePath)
context.clip()
context.drawLinearGradient(
    plateGradient,
    start: CGPoint(x: 512, y: 100),
    end: CGPoint(x: 512, y: 924),
    options: []
)
context.restoreGState()

let topLight = gradient([(0xFFFFFF, 0.10), (0xFFFFFF, 0)], [0, 1])

context.saveGState()
context.addPath(platePath)
context.clip()
context.drawRadialGradient(
    topLight,
    startCenter: CGPoint(x: 512, y: 120),
    startRadius: 0,
    endCenter: CGPoint(x: 512, y: 120),
    endRadius: 520,
    options: []
)
context.restoreGState()

let forgeGlow = gradient([(0xFF9F0A, 0.14), (0xFF9F0A, 0)], [0, 1])

context.saveGState()
context.addPath(platePath)
context.clip()
context.drawRadialGradient(
    forgeGlow,
    startCenter: CGPoint(x: 380, y: 700),
    startRadius: 0,
    endCenter: CGPoint(x: 380, y: 700),
    endRadius: 340,
    options: []
)
context.restoreGState()

context.saveGState()
context.addPath(platePath)
context.setStrokeColor(color(0xFFFFFF, 0.10))
context.setLineWidth(1.5)
context.strokePath()
context.restoreGState()

let fPath = CGMutablePath()

fPath.addRoundedRect(
    in: CGRect(x: 307, y: 277, width: 108, height: 470),
    cornerWidth: 54,
    cornerHeight: 54
)
fPath.addRoundedRect(
    in: CGRect(x: 307, y: 277, width: 410, height: 108),
    cornerWidth: 20,
    cornerHeight: 20
)
fPath.addRoundedRect(
    in: CGRect(x: 307, y: 497, width: 296, height: 102),
    cornerWidth: 20,
    cornerHeight: 20
)

context.saveGState()
context.setShadow(offset: CGSize(width: 0, height: 8), blur: 18, color: color(0x000000, 0.28))
context.addPath(fPath)
context.setFillColor(color(0x000000))
context.fillPath()
context.restoreGState()

let fGradient = gradient([(0xFFFFFF, 1), (0xDDDDE5, 1)], [0, 1])

context.saveGState()
context.addPath(fPath)
context.clip()
context.drawLinearGradient(
    fGradient,
    start: CGPoint(x: 512, y: 277),
    end: CGPoint(x: 512, y: 747),
    options: []
)
context.restoreGState()

let fSheen = gradient([(0xFFFFFF, 0.28), (0xFFFFFF, 0)], [0, 1])

context.saveGState()
context.addPath(fPath)
context.clip()
context.drawLinearGradient(
    fSheen,
    start: CGPoint(x: 512, y: 277),
    end: CGPoint(x: 512, y: 500),
    options: []
)
context.restoreGState()

let hotGradient = gradient(
    [(0xFFE6BC, 0), (0xFFB340, 1), (0xFF8F00, 1)],
    [0, 0.45, 1]
)

context.saveGState()
context.addPath(fPath)
context.clip()
context.drawLinearGradient(
    hotGradient,
    start: CGPoint(x: 512, y: 686),
    end: CGPoint(x: 512, y: 747),
    options: []
)
context.restoreGState()

let hotCore = gradient([(0xFFF3CE, 0.75), (0xFFE08A, 0)], [0, 1])

context.saveGState()
context.addPath(fPath)
context.clip()
context.drawRadialGradient(
    hotCore,
    startCenter: CGPoint(x: 361, y: 740),
    startRadius: 0,
    endCenter: CGPoint(x: 361, y: 740),
    endRadius: 46,
    options: []
)
context.restoreGState()

guard let image = context.makeImage() else {
    fatalError("could not render the image")
}

let output = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? "foundryui-1024.png")

guard
    let destination = CGImageDestinationCreateWithURL(
        output as CFURL,
        UTType.png.identifier as CFString,
        1,
        nil
    )
else {
    fatalError("could not create the image destination")
}

CGImageDestinationAddImage(destination, image, nil)

guard CGImageDestinationFinalize(destination) else {
    fatalError("could not write the image")
}
