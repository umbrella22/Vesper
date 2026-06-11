@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func refreshCurrentHdrFailureEvidence(for source: VesperPlayerSource, item: AVPlayerItem) {
        currentHdrFailureEvidence = nil
        let asset = item.asset
        Task { @MainActor [weak self, weak item] in
            let assetProbeResult = await VesperIOSAssetProbeProvider.probe(asset)
            guard let self,
                let item,
                self.player?.currentItem === item,
                self.currentSource == source
            else {
                return
            }
            let baseResult = VesperPlaybackCapabilityProbe.probe(
                VesperPlaybackCapabilityProbeRequest(source: source)
            )
            let result = VesperPlaybackCapabilityProbe.withAssetProbeResult(
                baseResult,
                assetProbeResult: assetProbeResult
            )
            self.updateCurrentHdrFailureEvidence(result, source: source)
        }
    }

    func updateCurrentHdrFailureEvidence(
        _ result: VesperPlaybackCapabilityProbeResult,
        source: VesperPlayerSource
    ) {
        guard currentSource == source else {
            return
        }
        currentHdrFailureEvidence = VesperNativeHdrFailureEvidence(source: source, result: result)
    }
}
