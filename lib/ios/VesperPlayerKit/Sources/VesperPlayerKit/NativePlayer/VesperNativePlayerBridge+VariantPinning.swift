@preconcurrency import AVFoundation
import Foundation
import UIKit
internal import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func applyDefaultPlaybackRate(_ rate: Float, to player: AVPlayer) {
        player.defaultRate = rate
    }

    func applyVideoVariantPin(_ pin: LoadedVideoVariantPin?, to item: AVPlayerItem) {
        desiredVideoVariantPin = pin
        applyEffectiveVideoVariantPin(pin, to: item)
    }

    func applyDashStartupAbrLimitIfNeeded(
        for source: VesperPlayerSource,
        to item: AVPlayerItem
    ) {
        guard source.protocol == .dash else {
            dashStartupAbrLimitPin = nil
            dashStartupAbrLimitAppliedAtNs = nil
            return
        }

        dashStartupAbrLimitPin = LoadedVideoVariantPin(
            peakBitRate: Self.dashStartupAbrPeakBitRate,
            maxWidth: Self.dashStartupAbrMaxWidth,
            maxHeight: Self.dashStartupAbrMaxHeight
        )
        dashStartupAbrLimitAppliedAtNs = DispatchTime.now().uptimeNanoseconds
        applyEffectiveVideoVariantPin(desiredVideoVariantPin, to: item)
        recordBenchmark(
            "dash_startup_abr_limit_applied",
            attributes: [
                "maxBitRate": "\(Int(Self.dashStartupAbrPeakBitRate))",
                "maxWidth": "\(Self.dashStartupAbrMaxWidth)",
                "maxHeight": "\(Self.dashStartupAbrMaxHeight)",
                "playbackEpoch": "\(currentPlaybackEpoch())",
            ]
        )
        iosHostLog(
            "dashStartupAbrLimit applied maxBitRate=\(Int(Self.dashStartupAbrPeakBitRate)) maxWidth=\(Self.dashStartupAbrMaxWidth) maxHeight=\(Self.dashStartupAbrMaxHeight)"
        )
    }

    func releaseDashStartupAbrLimitIfNeeded(reason: String, item: AVPlayerItem?) {
        guard dashStartupAbrLimitPin != nil else {
            return
        }
        dashStartupAbrLimitPin = nil
        let appliedAtNs = dashStartupAbrLimitAppliedAtNs
        dashStartupAbrLimitAppliedAtNs = nil
        if let item = item ?? player?.currentItem {
            applyEffectiveVideoVariantPin(desiredVideoVariantPin, to: item)
        }

        var attributes = [
            "reason": reason,
            "playbackEpoch": "\(currentPlaybackEpoch())",
        ]
        if let appliedAtNs {
            let now = DispatchTime.now().uptimeNanoseconds
            attributes["elapsedNs"] = "\(now >= appliedAtNs ? now - appliedAtNs : 0)"
        }
        recordBenchmark(
            "dash_startup_abr_limit_released",
            attributes: attributes
        )
        iosHostLog("dashStartupAbrLimit released reason=\(reason)")
    }

    func applyEffectiveVideoVariantPin(
        _ pin: LoadedVideoVariantPin?,
        to item: AVPlayerItem
    ) {
        let effectivePin = combinedVideoVariantPin(pin, dashStartupAbrLimitPin)
        item.preferredPeakBitRate = effectivePin?.peakBitRate ?? 0
        if let maxWidth = effectivePin?.maxWidth, let maxHeight = effectivePin?.maxHeight {
            item.preferredMaximumResolution = CGSize(
                width: CGFloat(maxWidth),
                height: CGFloat(maxHeight)
            )
        } else {
            item.preferredMaximumResolution = .zero
        }
    }

    func combinedVideoVariantPin(
        _ desiredPin: LoadedVideoVariantPin?,
        _ temporaryPin: LoadedVideoVariantPin?
    ) -> LoadedVideoVariantPin? {
        guard let desiredPin else {
            return temporaryPin
        }
        guard let temporaryPin else {
            return desiredPin
        }
        return LoadedVideoVariantPin(
            peakBitRate: minimumOptional(desiredPin.peakBitRate, temporaryPin.peakBitRate),
            maxWidth: minimumOptional(desiredPin.maxWidth, temporaryPin.maxWidth),
            maxHeight: minimumOptional(desiredPin.maxHeight, temporaryPin.maxHeight)
        )
    }
}
