import CoreImage
import CoreVideo
import Foundation
import Metal
@_implementationOnly import VesperPlayerKitBridgeShim

/// Encodes the bundled reference filter into the surface's presentation command.
final class VesperBundledMetalFilter {
    private let handle: UnsafeMutableRawPointer
    private let colorSpace: CGColorSpace
    private var reportedStatus: UInt32?

    init?(device: MTLDevice, queue: MTLCommandQueue, library: URL) {
        guard let colorSpace = CGColorSpace(name: CGColorSpace.extendedLinearSRGB),
              let parameters = try? JSONSerialization.data(withJSONObject: ["metalLibrary": library.path])
        else { return nil }
        let handle = parameters.withUnsafeBytes { bytes in
            vesper_ios_metal_filter_create(
                Unmanaged.passUnretained(device as AnyObject).toOpaque(),
                Unmanaged.passUnretained(queue as AnyObject).toOpaque(),
                bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count
            )
        }
        guard let handle else { return nil }
        self.handle = handle
        self.colorSpace = colorSpace
    }

    deinit {
        vesper_ios_metal_filter_destroy(handle)
    }

    func encode(pixelBuffer: CVPixelBuffer, into command: MTLCommandBuffer) -> CIImage? {
        var result: UnsafeRawPointer?
        let status = vesper_ios_metal_filter_encode(
            handle,
            Unmanaged.passUnretained(command as AnyObject).toOpaque(),
            pixelBuffer, &result
        )
        guard status == 0, let result,
              let texture = Unmanaged<AnyObject>.fromOpaque(result).takeUnretainedValue() as? MTLTexture
        else {
            if reportedStatus != status {
                reportedStatus = status
                iosHostLog("bundled Metal filter returned status \(status)")
            }
            return nil
        }
        // The filter writes Metal texture rows in pixel-buffer order. Core Image
        // interprets a Metal texture with the opposite vertical origin.
        return CIImage(mtlTexture: texture, options: [.colorSpace: colorSpace])?
            .oriented(.downMirrored)
    }
}
