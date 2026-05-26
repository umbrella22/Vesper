@preconcurrency import AVFoundation
import Foundation
import UniformTypeIdentifiers

final class VesperSourceNormalizerResourceSession {
    let id = UUID().uuidString
    let resource: VesperSourceNormalizerResourceOpenResult
    let playbackURL: URL

    private let rootDirectory: URL
    private let primaryURL: URL
    private let readBufferBytes: Int

    init(resource: VesperSourceNormalizerResourceOpenResult) throws {
        self.resource = resource
        primaryURL = URL(fileURLWithPath: resource.primaryResourcePath)
        rootDirectory = primaryURL.deletingLastPathComponent()
        let bufferValue = resource.cachePolicy["sessionReadBufferBytes"] as? NSNumber
        readBufferBytes = max(
            vesperLocalResourceMinReadBufferBytes,
            min(
                bufferValue?.intValue ?? vesperLocalResourceDefaultReadBufferBytes,
                vesperLocalResourceMaxReadBufferBytes
            )
        )
        let lastPathComponent = resource.outputRoute == "hlsShortWindow" ? "index.m3u8" : "primary"
        guard let url = URL(string: "vesper-normalized://session/\(id)/\(lastPathComponent)") else {
            throw NSError(
                domain: "io.github.ikaros.vesper.player.source-normalizer",
                code: -1,
                userInfo: [NSLocalizedDescriptionKey: "Failed to create normalized playback URL."]
            )
        }
        playbackURL = url
    }

    func localURL(for url: URL) -> URL? {
        guard url.scheme == "vesper-normalized" else {
            return nil
        }
        let components = url.pathComponents.filter { $0 != "/" }
        guard components.first == id, components.count >= 2 else {
            return nil
        }
        let resourcePath = components.dropFirst().joined(separator: "/")
        if resourcePath == "primary" || resourcePath == "index.m3u8" {
            return primaryURL
        }
        guard !resourcePath.contains("..") else {
            return nil
        }
        let candidate = rootDirectory.appendingPathComponent(resourcePath, isDirectory: false)
        guard vesperLocalResourceIsContained(candidate, in: rootDirectory) else {
            return nil
        }
        return candidate
    }

    func contentType(for localURL: URL) -> String {
        let name = localURL.lastPathComponent.lowercased()
        if name.hasSuffix(".m3u8") {
            return "public.m3u-playlist"
        }
        if name.hasSuffix(".ts") {
            return "video/mp2t"
        }
        if name.hasSuffix(".mp4") || name.hasSuffix(".m4s") {
            return "public.mpeg-4"
        }
        if let type = UTType(filenameExtension: localURL.pathExtension) {
            return type.identifier
        }
        return "public.data"
    }

    func localResourceBody(for localURL: URL) throws -> VesperLocalResourceBody {
        let length = try vesperLocalResourceFileSize(localURL)
        let growingPolicy: VesperGrowingFileReadPolicy? =
            isGrowingPrimary(localURL)
                ? VesperGrowingFileReadPolicy(timeoutSeconds: 2.0, pollSeconds: 0.025)
                : nil
        return .file(
            url: localURL,
            offset: 0,
            length: length,
            contentType: contentType(for: localURL),
            removeAfterServing: false,
            growingPolicy: growingPolicy
        )
    }

    var readPolicy: VesperLocalResourceReadPolicy {
        VesperLocalResourceReadPolicy(
            bufferBytes: readBufferBytes,
            outOfRangeBehavior: .finishEmpty
        )
    }

    func isGrowingPrimary(_ localURL: URL) -> Bool {
        resource.outputRoute == "fmp4LocalStream" &&
            localURL.standardizedFileURL == primaryURL.standardizedFileURL
    }
}

final class VesperSourceNormalizerResourceLoaderDelegate:
    NSObject,
    AVAssetResourceLoaderDelegate,
    @unchecked Sendable
{
    let resourceLoadingQueue: DispatchQueue
    private let session: VesperSourceNormalizerResourceSession
    private var tasks: [ObjectIdentifier: Task<Void, Never>] = [:]

    init(session: VesperSourceNormalizerResourceSession) {
        self.session = session
        resourceLoadingQueue = DispatchQueue(
            label: "io.github.ikaros.vesper.player.source-normalizer.resource-loader.\(session.id)"
        )
        super.init()
    }

    func resourceLoader(
        _ resourceLoader: AVAssetResourceLoader,
        shouldWaitForLoadingOfRequestedResource loadingRequest: AVAssetResourceLoadingRequest
    ) -> Bool {
        guard
            let requestURL = loadingRequest.request.url,
            let localURL = session.localURL(for: requestURL)
        else {
            return false
        }

        let requestId = ObjectIdentifier(loadingRequest)
        let task = Task { [weak self, session, loadingRequest] in
            do {
                try Task.checkCancellation()
                let body = try session.localResourceBody(for: localURL)
                await self?.finish(
                    loadingRequest,
                    requestId: requestId,
                    body: body,
                    readPolicy: session.readPolicy
                )
            } catch {
                await self?.finish(loadingRequest, requestId: requestId, error: error)
            }
        }
        tasks[requestId] = task
        return true
    }

    func resourceLoader(
        _ resourceLoader: AVAssetResourceLoader,
        didCancel loadingRequest: AVAssetResourceLoadingRequest
    ) {
        let requestId = ObjectIdentifier(loadingRequest)
        tasks.removeValue(forKey: requestId)?.cancel()
    }

    @MainActor
    private func finish(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        requestId: ObjectIdentifier,
        body: VesperLocalResourceBody,
        readPolicy: VesperLocalResourceReadPolicy
    ) {
        resourceLoadingQueue.async { [weak self] in
            self?.tasks.removeValue(forKey: requestId)
            VesperLocalResourceResponder.finish(
                loadingRequest,
                body: body,
                readPolicy: readPolicy
            )
        }
    }

    @MainActor
    private func finish(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        requestId: ObjectIdentifier,
        error: Error
    ) {
        resourceLoadingQueue.async { [weak self] in
            self?.tasks.removeValue(forKey: requestId)
            VesperLocalResourceResponder.finish(loadingRequest, error: error)
        }
    }
}

func isVesperSourceNormalizerURL(_ url: URL) -> Bool {
    url.scheme == "vesper-normalized"
}
