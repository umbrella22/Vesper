@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func resolvedUrl(for source: VesperPlayerSource) throws -> URL {
        guard let url = URL(string: source.uri) else {
            throw NSError(
                domain: "io.github.ikaros.vesper.host.ios",
                code: -2,
                userInfo: [NSLocalizedDescriptionKey: VesperPlayerI18n.invalidMediaUrl]
            )
        }
        return url
    }
}
