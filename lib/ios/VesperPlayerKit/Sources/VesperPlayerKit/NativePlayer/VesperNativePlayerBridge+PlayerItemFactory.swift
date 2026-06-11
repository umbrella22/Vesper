@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func makePlayerItem(for source: VesperPlayerSource, url: URL) -> AVPlayerItem {
        if isVesperSourceNormalizerURL(url) {
            currentDashSession = nil
            dashResourceLoaderDelegate = nil
            guard let session = sourceNormalizerResourceSession else {
                return AVPlayerItem(url: url)
            }
            let loaderDelegate = VesperSourceNormalizerResourceLoaderDelegate(session: session)
            let asset = AVURLAsset(url: url)
            asset.resourceLoader.setDelegate(
                loaderDelegate,
                queue: loaderDelegate.resourceLoadingQueue
            )
            sourceNormalizerResourceLoaderDelegate = loaderDelegate
            iosHostLog("configured SourceNormalizer resource loader url=\(url.absoluteString)")
            recordBenchmark("source_normalizer_resource_loader_configured")
            return AVPlayerItem(asset: asset)
        }

        guard source.protocol == .dash else {
            currentDashSession = nil
            dashResourceLoaderDelegate = nil
            sourceNormalizerResourceLoaderDelegate = nil
            guard !source.headers.isEmpty else {
                return AVPlayerItem(url: url)
            }
            let asset = AVURLAsset(
                url: url,
                options: [vesperAVURLAssetHTTPHeaderFieldsKey: source.headers]
            )
            return AVPlayerItem(asset: asset)
        }

        let dashBenchmarkEventRecorder: VesperDashSession.BenchmarkEventRecorder?
        if benchmarkRecorder.isEnabled {
            dashBenchmarkEventRecorder = { [weak self] eventName, attributes in
                self?.recordBenchmark(eventName, attributes: attributes)
            }
        } else {
            dashBenchmarkEventRecorder = nil
        }
        let session = VesperDashSession(
            sourceURL: url,
            headers: source.headers,
            benchmarkEventRecorder: dashBenchmarkEventRecorder
        )
        let loaderDelegate = VesperDashResourceLoaderDelegate(session: session)
        let asset = source.headers.isEmpty
            ? AVURLAsset(url: session.masterPlaylistURL)
            : AVURLAsset(
                url: session.masterPlaylistURL,
                options: [vesperAVURLAssetHTTPHeaderFieldsKey: source.headers]
            )
        asset.resourceLoader.setDelegate(
            loaderDelegate,
            queue: loaderDelegate.resourceLoadingQueue
        )
        currentDashSession = session
        dashResourceLoaderDelegate = loaderDelegate
        sourceNormalizerResourceLoaderDelegate = nil
        iosHostLog("configured DASH bridge master=\(session.masterPlaylistURL.absoluteString)")
        recordBenchmark("dash_bridge_configured")
        return AVPlayerItem(asset: asset)
    }
}
