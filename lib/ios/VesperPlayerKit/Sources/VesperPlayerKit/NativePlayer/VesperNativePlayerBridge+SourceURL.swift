@preconcurrency import AVFoundation
import Foundation
import UIKit
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func resolvedUrl(for source: VesperPlayerSource) throws -> URL {
        guard let url = URL(string: source.uri) else {
            throw NSError(
                domain: "io.github.umbrella22.vesper.host.ios",
                code: -2,
                userInfo: [NSLocalizedDescriptionKey: VesperPlayerI18n.invalidMediaUrl]
            )
        }
        return url
    }
}
