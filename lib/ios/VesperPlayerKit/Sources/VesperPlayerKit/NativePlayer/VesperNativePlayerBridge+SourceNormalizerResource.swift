@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKitBridgeShim

extension VesperNativePlayerBridge {
    func openSourceNormalizerResourceIfNeeded(
        for source: VesperPlayerSource,
        sourceLoadEpoch: UInt64
    ) async -> VesperSourceNormalizerResourceOpenResult? {
        closeCurrentSourceNormalizerResource()
        guard sourceNormalizerConfiguration.mode == .preferNormalized ||
            sourceNormalizerConfiguration.mode == .requireNormalized
        else {
            return nil
        }
        guard source.drmConfiguration == nil else {
            iosHostLog("source normalizer resource bypassed for DRM source; route=direct")
            return nil
        }

        let outputRoot = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-source-normalizer", isDirectory: true)
        let configuration = sourceNormalizerConfiguration
        let forceNormalized = sourceNormalizerConfiguration.mode == .requireNormalized
        let outcome = await Task.detached(priority: .utility) {
            VesperMobileSourceNormalizerResource.open(
                source: source,
                configuration: configuration,
                outputRoot: outputRoot,
                forceNormalized: forceNormalized
            )
        }.value
        guard isCurrentSourceLoad(sourceLoadEpoch, source: source) else {
            if let resource = outcome.resource {
                VesperMobileSourceNormalizerResource.dispose(handle: resource.handle)
            }
            return nil
        }
        if !outcome.diagnostics.isEmpty {
            currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(outcome.diagnostics)
        }
        let resource = outcome.resource
        guard let resource else {
            if sourceNormalizerConfiguration.mode == .requireNormalized {
                reportCommandError(
                    code: .backendFailure,
                    category: .source,
                    message: "SourceNormalizer requireNormalized failed to open a normalized resource"
                )
            } else {
                let bypassReason = sourceNormalizerBypassReason(from: outcome.diagnostics)
                    ?? "sourceNormalizerResourceBypassed"
                iosHostLog(
                    "source normalizer resource bypassed; route=native fallbackReason=\(bypassReason)"
                )
            }
            return nil
        }

        currentSourceNormalizerResource = resource
        if !resource.diagnostics.isEmpty {
            let enrichedDiagnostics = resource.diagnostics.map { diagnostic in
                var enriched = diagnostic
                enriched["outputRoute"] = resource.outputRoute
                enriched["selectedProfile"] = resource.selectedProfile
                enriched["contentType"] = resource.primaryContentType
                enriched["primaryResource"] = resource.primaryResourcePath
                enriched["cachePolicy"] = resource.cachePolicy
                enriched["route"] = resource.route ?? resource.outputRoute
                if let cacheQuota = resource.cacheQuota {
                    enriched["cacheQuota"] = cacheQuota
                }
                if let fallbackReason = resource.fallbackReason {
                    enriched["fallbackReason"] = fallbackReason
                }
                enriched["participation"] = "participated"
                return enriched
            }
            currentPluginDiagnostics = pluginDiagnosticsWithNativeFramePipeline(enrichedDiagnostics)
        }
        iosHostLog(
            "source normalizer resource selected route=\(resource.outputRoute) path=\(resource.primaryResourcePath)"
        )
        return resource
    }

    func sourceNormalizerBypassReason(
        from diagnostics: [[String: Any]]
    ) -> String? {
        let messages = diagnostics.compactMap { $0["message"] as? String }
        if messages.contains(where: { $0.contains("HdrResourceMetadataNotPreserved") }) {
            return "sourceNormalizerResourceBypassedForHdr"
        }
        return messages.first
    }

    func makeSourceNormalizerResourceSession(
        for resource: VesperSourceNormalizerResourceOpenResult?
    ) -> VesperSourceNormalizerResourceSession? {
        guard let resource else {
            sourceNormalizerResourceSession = nil
            sourceNormalizerResourceLoaderDelegate = nil
            return nil
        }
        do {
            let session = try VesperSourceNormalizerResourceSession(resource: resource)
            sourceNormalizerResourceSession = session
            return session
        } catch {
            iosHostLog("source normalizer resource loader setup failed: \(error.localizedDescription)")
            if sourceNormalizerConfiguration.mode == .requireNormalized {
                reportCommandError(
                    code: .backendFailure,
                    category: .source,
                    message: error.localizedDescription
                )
            }
            return nil
        }
    }

    func closeCurrentSourceNormalizerResource() {
        guard let resource = currentSourceNormalizerResource else {
            return
        }
        currentSourceNormalizerResource = nil
        sourceNormalizerResourceSession = nil
        sourceNormalizerResourceLoaderDelegate = nil
        VesperMobileSourceNormalizerResource.dispose(handle: resource.handle)
    }

    func normalizedPlaybackSource(
        original: VesperPlayerSource,
        resource: VesperSourceNormalizerResourceOpenResult?
    ) -> VesperPlayerSource {
        guard let resource else {
            return original
        }
        let playbackProtocol: VesperPlayerSourceProtocol
        switch resource.outputRoute {
        case "hlsShortWindow":
            playbackProtocol = .hls
        case "fmp4LocalStream":
            playbackProtocol = .progressive
        default:
            return original
        }
        return VesperPlayerSource(
            uri: sourceNormalizerResourceSession?.playbackURL.absoluteString
                ?? resource.playbackURL?.absoluteString
                ?? original.uri,
            label: original.label,
            kind: .local,
            protocol: playbackProtocol
        )
    }
}
