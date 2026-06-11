import AVFoundation
import Foundation
import UIKit

struct VesperIOSSessionProbeEnvironment: Equatable {
    enum DisplayGamut: String, Equatable {
        case p3
        case srgb
        case unspecified
        case unknown

        init(_ gamut: UIDisplayGamut) {
            switch gamut {
            case .P3:
                self = .p3
            case .SRGB:
                self = .srgb
            case .unspecified:
                self = .unspecified
            @unknown default:
                self = .unknown
            }
        }
    }

    let displayGamut: DisplayGamut
    let hdrPlaybackEligible: Bool?
    let maximumFramesPerSecond: Int?
    let nativeWidth: Int?
    let nativeHeight: Int?

    @MainActor
    static func current(
        screen: UIScreen? = nil
    ) -> VesperIOSSessionProbeEnvironment {
        let resolvedScreen = screen ?? UIScreen.main
        let nativeBounds = resolvedScreen.nativeBounds
        let hdrPlaybackEligible: Bool?
        if #available(iOS 11.0, *) {
            hdrPlaybackEligible = AVPlayer.eligibleForHDRPlayback
        } else {
            hdrPlaybackEligible = nil
        }
        return VesperIOSSessionProbeEnvironment(
            displayGamut: DisplayGamut(resolvedScreen.traitCollection.displayGamut),
            hdrPlaybackEligible: hdrPlaybackEligible,
            maximumFramesPerSecond: resolvedScreen.maximumFramesPerSecond,
            nativeWidth: Int(nativeBounds.width.rounded()),
            nativeHeight: Int(nativeBounds.height.rounded())
        )
    }
}

enum VesperIOSSessionProbeProvider {
    @MainActor
    static func currentDisplay() -> VesperPlaybackCapabilityProbe.SessionProbeProvider {
        let environment = VesperIOSSessionProbeEnvironment.current()
        return { request in
            probe(request, environment: environment)
        }
    }

    static func probe(
        _ request: VesperPlaybackCapabilityProbeRequest,
        environment: VesperIOSSessionProbeEnvironment
    ) -> VesperPlaybackCapabilitySessionProbeResult {
        let hdrKind = request.codec.map(VesperPlaybackCapabilityProbe.detectHdrKind) ?? .none
        var supportedHdrKinds: Set<VesperPlaybackCapabilityHdrKind> = []
        if environment.hdrPlaybackEligible == true, hdrKind != .none, hdrKind != .unknown {
            supportedHdrKinds.insert(hdrKind)
        }

        var diagnostics: [String: String] = [
            "sessionProbe": "iosDisplayAndPlayerHdrEligibility",
            displayHdrProbeAvailableKey: "true",
            "displayGamut": environment.displayGamut.rawValue,
            "avPlayerEligibleForHDRPlayback": environment.hdrPlaybackEligible.map(String.init)
                ?? "unknown",
        ]
        if environment.hdrPlaybackEligible == true {
            diagnostics["hdrKindSupportBasis"] = "avPlayerEligibleForHDRPlayback"
        }
        if let maximumFramesPerSecond = environment.maximumFramesPerSecond {
            diagnostics["displayMaximumFramesPerSecond"] = String(maximumFramesPerSecond)
            if let frameRate = request.frameRate, frameRate > Double(maximumFramesPerSecond) + 0.01 {
                diagnostics[displayFrameRateSupportedKey] = "false"
            } else if request.frameRate != nil {
                diagnostics[displayFrameRateSupportedKey] = "true"
            }
        }
        if let nativeWidth = environment.nativeWidth {
            diagnostics["displayNativeWidth"] = String(nativeWidth)
        }
        if let nativeHeight = environment.nativeHeight {
            diagnostics["displayNativeHeight"] = String(nativeHeight)
        }
        if let width = request.width {
            diagnostics["requestedWidth"] = String(width)
        }
        if let height = request.height {
            diagnostics["requestedHeight"] = String(height)
        }
        if let frameRate = request.frameRate {
            diagnostics["requestedFrameRate"] = String(frameRate)
        }

        return VesperPlaybackCapabilitySessionProbeResult(
            supportedHdrKinds: supportedHdrKinds,
            diagnostics: diagnostics
        )
    }
}
