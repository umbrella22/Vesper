@preconcurrency import AVFoundation
import Foundation
import UIKit
internal import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func makePlayerItem(for source: VesperPlayerSource, url: URL) throws -> AVPlayerItem {
        // Live streaming protocols (RTMP/RTSP/FLV) are not supported by AVPlayer
        // on iOS. Supporting them would require a software demux/remux pipeline
        // (FFmpeg), which conflicts with the native-first boundary. Reject
        // explicitly with a capability error instead of letting AVURLAsset
        // silently fail to load.
        switch source.protocol {
        case .rtmp, .rtsp, .flv:
            throw VesperPlayerError(
                message: "iOS does not support \(source.protocol.rawValue.uppercased()) live streams; use HLS live instead.",
                code: .unsupported,
                category: .capability,
                retriable: false,
                details: [
                    "reason": "liveProtocolUnsupportedOnIos",
                    "route": "direct",
                    "protocol": source.protocol.rawValue,
                ]
            )
        case .unknown, .file, .content, .progressive, .hls, .dash:
            break
        }

        if isVesperSourceNormalizerURL(url) {
            currentDashSession = nil
            dashResourceLoaderDelegate = nil
            fairPlayDrmCoordinator?.close()
            fairPlayDrmCoordinator = nil
            fairPlayDrmCoordinatorId = nil
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
            let assetOptions = source.headers.isEmpty
                ? nil
                : [vesperAVURLAssetHTTPHeaderFieldsKey: source.headers]
            let asset = AVURLAsset(url: url, options: assetOptions)
            try configureFairPlayIfNeeded(for: source, asset: asset)
            return AVPlayerItem(asset: asset)
        }

        fairPlayDrmCoordinator?.close()
        fairPlayDrmCoordinator = nil
        fairPlayDrmCoordinatorId = nil
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
        let loaderDelegate = VesperDashResourceLoaderDelegate(
            session: session,
            subtitleResourceFailureHandler: { [weak self, session] in
                self?.reportDashSubtitleResourceFailure(
                    session: session,
                    source: source
                )
            }
        )
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

    func reportDashSubtitleResourceFailure(
        session: VesperDashSession,
        source: VesperPlayerSource
    ) {
        guard currentDashSession === session, currentSource == source else {
            iosHostLog("ignored stale DASH subtitle resource failure")
            return
        }
        reportSubtitleFailure(
            code: "subtitle_resource_load_failed",
            phase: .resource,
            retriable: true,
            message: "DASH subtitle resource loading failed"
        )
    }

    func configureFairPlayIfNeeded(for source: VesperPlayerSource, asset: AVURLAsset) throws {
        guard let drmConfiguration = source.drmConfiguration,
              drmConfiguration.keySystem.caseInsensitiveCompare("fairPlay") == .orderedSame
        else {
            fairPlayDrmCoordinator?.close()
            fairPlayDrmCoordinator = nil
            fairPlayDrmCoordinatorId = nil
            return
        }

        let coordinatorId = UUID()
        let coordinator = try VesperFairPlayDrmCoordinator.make(source: source) { [weak self] error in
            Task { @MainActor in
                guard let self,
                      self.currentSource == source,
                      self.fairPlayDrmCoordinatorId == coordinatorId
                else {
                    return
                }
                self.handlePlaybackFailure(
                    error: error,
                    fallbackMessage: error.localizedDescription
                )
            }
        }
        fairPlayDrmCoordinator?.close()
        fairPlayDrmCoordinator = coordinator
        fairPlayDrmCoordinatorId = coordinatorId
        coordinator.attach(to: asset)
        iosHostLog("configured FairPlay DRM content key session source=\(source.uri)")
        recordBenchmark("fairplay_drm_configured")
    }
}
