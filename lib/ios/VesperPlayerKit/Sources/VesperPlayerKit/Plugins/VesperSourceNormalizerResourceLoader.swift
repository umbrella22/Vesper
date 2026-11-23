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
                domain: "io.github.umbrella22.vesper.player.source-normalizer",
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

/// Accessed only on the loader's serial queue. A removed request cannot finish.
struct VesperSourceNormalizerPendingRequests {
    private var tasks: [ObjectIdentifier: Task<Void, Never>] = [:]
    private var invalidated = false
    // One item shares this budget across its range and segment requests.
    private let maximumPendingRequests = 16

    mutating func start(_ id: ObjectIdentifier, task: () -> Task<Void, Never>) -> Bool {
        guard !invalidated, tasks.count < maximumPendingRequests, tasks[id] == nil else {
            return false
        }
        tasks[id] = task()
        return true
    }

    mutating func cancel(_ id: ObjectIdentifier) {
        tasks.removeValue(forKey: id)?.cancel()
    }

    mutating func claimCompletion(_ id: ObjectIdentifier) -> Bool {
        guard let task = tasks.removeValue(forKey: id) else { return false }
        return !task.isCancelled
    }

    mutating func invalidate() {
        invalidated = true
        for task in tasks.values {
            task.cancel()
        }
        tasks.removeAll()
    }
}

final class VesperSourceNormalizerResourceLoaderDelegate:
    NSObject,
    AVAssetResourceLoaderDelegate,
    @unchecked Sendable
{
    let resourceLoadingQueue: DispatchQueue
    private let session: VesperSourceNormalizerResourceSession
    private var requests = VesperSourceNormalizerPendingRequests()

    init(session: VesperSourceNormalizerResourceSession) {
        self.session = session
        resourceLoadingQueue = DispatchQueue(
            label: "io.github.umbrella22.vesper.player.source-normalizer.resource-loader.\(session.id)"
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
        return requests.start(requestId) {
            Task { [weak self, session, loadingRequest] in
                do {
                    try Task.checkCancellation()
                    let body = try session.localResourceBody(for: localURL)
                    self?.finish(
                        loadingRequest,
                        requestId: requestId,
                        body: body,
                        readPolicy: session.readPolicy
                    )
                } catch {
                    self?.finish(loadingRequest, requestId: requestId, error: error)
                }
            }
        }
    }

    func resourceLoader(
        _ resourceLoader: AVAssetResourceLoader,
        didCancel loadingRequest: AVAssetResourceLoadingRequest
    ) {
        let requestId = ObjectIdentifier(loadingRequest)
        requests.cancel(requestId)
    }

    /// Retires all requests before the normalizer removes its backing files.
    func invalidate() async {
        await withCheckedContinuation { continuation in
            resourceLoadingQueue.async {
                self.requests.invalidate()
                continuation.resume()
            }
        }
    }

    private func finish(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        requestId: ObjectIdentifier,
        body: VesperLocalResourceBody,
        readPolicy: VesperLocalResourceReadPolicy
    ) {
        resourceLoadingQueue.async { [weak self] in
            guard self?.claimCompletion(loadingRequest, requestId: requestId) == true else { return }
            VesperLocalResourceResponder.finish(
                loadingRequest,
                body: body,
                readPolicy: readPolicy
            )
        }
    }

    private func finish(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        requestId: ObjectIdentifier,
        error: Error
    ) {
        resourceLoadingQueue.async { [weak self] in
            guard self?.claimCompletion(loadingRequest, requestId: requestId) == true else { return }
            VesperLocalResourceResponder.finish(loadingRequest, error: error)
        }
    }

    private func claimCompletion(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        requestId: ObjectIdentifier
    ) -> Bool {
        requests.claimCompletion(requestId) && !loadingRequest.isCancelled
    }
}

func isVesperSourceNormalizerURL(_ url: URL) -> Bool {
    url.scheme == "vesper-normalized"
}
