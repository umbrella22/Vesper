@preconcurrency import AVFoundation
import Foundation
import UIKit
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func makePlayerItem(
        for source: VesperPlayerSource,
        url: URL,
        sourceEpoch: UInt64? = nil
    ) throws -> AVPlayerItem {
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
            iosHostLog(
                "configured SourceNormalizer resource loader url=\(diagnosticURLDescription(url.absoluteString))"
            )
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
            subtitleResourceFailureHandler: { [weak self, session] renditionId in
                self?.reportDashSubtitleResourceFailure(
                    session: session,
                    source: source,
                    trackId: renditionId,
                    sourceEpoch: sourceEpoch
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
        iosHostLog(
            "configured DASH bridge master=\(diagnosticURLDescription(session.masterPlaylistURL.absoluteString))"
        )
        recordBenchmark("dash_bridge_configured")
        return AVPlayerItem(asset: asset)
    }

    func reportDashSubtitleResourceFailure(
        session: VesperDashSession,
        source: VesperPlayerSource,
        trackId: String? = nil,
        sourceEpoch: UInt64? = nil
    ) {
        guard currentDashSession === session,
              currentSource == source,
              sourceEpoch.map({ $0 == subtitleSourceEpoch }) ?? true
        else {
            iosHostLog("ignored stale DASH subtitle resource failure")
            return
        }
        let catalogTrackId = trackId.map { renditionId in
            renditionId.hasPrefix("subtitle:dash:")
                ? renditionId
                : "subtitle:dash:\(renditionId)"
        }
        let didRecordFailure: Bool
        if let catalogTrackId, !catalogTrackId.isEmpty {
            didRecordFailure = failedSubtitleTrackIds.insert(catalogTrackId).inserted
            subtitleOptionsByTrackId.removeValue(forKey: catalogTrackId)
            if didRecordFailure {
                publishTrackCatalog(VesperTrackCatalog(
                    tracks: publishedTrackCatalog.tracks.filter { $0.id != catalogTrackId },
                    adaptiveVideo: publishedTrackCatalog.adaptiveVideo,
                    adaptiveAudio: publishedTrackCatalog.adaptiveAudio
                ))
            }
            if publishedEffectiveSubtitleTrackId == catalogTrackId {
                if let item = player?.currentItem, let subtitleGroup {
                    item.select(nil, in: subtitleGroup)
                }
                _ = subtitleOverlayRenderer.select(trackId: nil)
                publishedEffectiveSubtitleTrackId = nil
                publishedTrackSelection = VesperTrackSelectionSnapshot(
                    video: publishedTrackSelection.video,
                    audio: publishedTrackSelection.audio,
                    subtitle: publishedTrackSelection.subtitle,
                    confirmedSubtitle: publishedTrackSelection.confirmedSubtitle,
                    effectiveSubtitleTrackId: nil,
                    abrPolicy: publishedTrackSelection.abrPolicy
                )
            }
        } else {
            didRecordFailure = true
        }
        reportSubtitleFailure(
            code: "subtitle_resource_failed",
            phase: .resource,
            trackId: catalogTrackId,
            retriable: true,
            message: "DASH subtitle resource loading failed"
        )
        guard didRecordFailure else { return }
        let remainingSelectableCount = max(
            0,
            publishedSubtitleState.selectableTrackCount - 1
        )
        let catalogError = publishedSubtitleState.catalogError
        publishedSubtitleState = VesperSubtitleState(
            catalogState: remainingSelectableCount > 0 ? .ready : .failed,
            selectionState: publishedSubtitleState.selectionState,
            advertisedTrackCount: publishedSubtitleState.advertisedTrackCount,
            selectableTrackCount: remainingSelectableCount,
            catalogError: catalogError,
            selectionError: publishedSubtitleState.selectionError
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
        iosHostLog(
            "configured FairPlay DRM content key session source=\(diagnosticURLDescription(source.uri))"
        )
        recordBenchmark("fairplay_drm_configured")
    }
}
