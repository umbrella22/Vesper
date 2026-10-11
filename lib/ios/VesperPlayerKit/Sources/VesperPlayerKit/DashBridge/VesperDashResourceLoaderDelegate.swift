@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

func isVesperDashCancellation(_ error: Error) -> Bool {
    if error is CancellationError { return true }
    let native = error as NSError
    return native.domain == NSURLErrorDomain && native.code == NSURLErrorCancelled
}

// AVFoundation can replace a custom resource failure with a URL-loading error.
// Keep the typed cause on its resource-loader owner, and in the NSError wrapper
// when AVFoundation preserves the underlying error chain.
final class VesperDashResourceFailure: NSError, @unchecked Sendable {
    let cause: Error
    let sessionID: String
    let sourceLoadEpoch: UInt64
    let requestURL: URL

    init(cause: Error, sessionID: String, sourceLoadEpoch: UInt64, requestURL: URL) {
        self.cause = cause
        self.sessionID = sessionID
        self.sourceLoadEpoch = sourceLoadEpoch
        self.requestURL = requestURL
        let native = cause as NSError
        super.init(domain: "io.github.umbrella22.vesper.dash.resource", code: 1, userInfo: [
            NSLocalizedDescriptionKey: (cause as? LocalizedError)?.errorDescription ?? native.localizedDescription,
            NSUnderlyingErrorKey: native,
        ])
    }

    required init?(coder: NSCoder) { return nil }
}

final class VesperDashPlaybackFailureStore: @unchecked Sendable {
    private let lock = NSLock()
    private let sessionID: String
    private let sourceLoadEpoch: UInt64
    private let masterPlaylistURL: URL
    private var recorded: (requestID: ObjectIdentifier, failure: VesperDashResourceFailure)?

    init(session: VesperDashSession, sourceLoadEpoch: UInt64) {
        sessionID = session.id
        self.sourceLoadEpoch = sourceLoadEpoch
        masterPlaylistURL = session.masterPlaylistURL
    }

    func record(error: Error, requestID: ObjectIdentifier, requestURL: URL,
                isSubtitle: Bool = false, isCancelled: Bool = false) -> VesperDashResourceFailure? {
        guard !isSubtitle, !isCancelled, !isVesperDashCancellation(error) else { return nil }
        if let dash = error as? VesperDashBridgeError, case .subtitle = dash { return nil }
        let failure = VesperDashResourceFailure(cause: error, sessionID: sessionID,
            sourceLoadEpoch: sourceLoadEpoch, requestURL: requestURL)
        lock.lock()
        defer { lock.unlock() }
        if recorded == nil { recorded = (requestID, failure) }
        return failure
    }

    func cancel(requestID: ObjectIdentifier) {
        lock.lock()
        defer { lock.unlock() }
        if recorded?.requestID == requestID { recorded = nil }
    }

    func failure(sessionID: String, sourceLoadEpoch: UInt64, itemURL: URL) -> VesperDashResourceFailure? {
        guard self.sessionID == sessionID, self.sourceLoadEpoch == sourceLoadEpoch,
              masterPlaylistURL == itemURL else { return nil }
        lock.lock()
        defer { lock.unlock() }
        return recorded?.failure
    }
}

final class VesperDashResourceLoaderDelegate: NSObject, AVAssetResourceLoaderDelegate {
    typealias SubtitleResourceFailureHandler = @MainActor @Sendable (String) -> Void

    let resourceLoadingQueue: DispatchQueue
    let playbackFailures: VesperDashPlaybackFailureStore

    private let session: VesperDashSession
    private let subtitleResourceFailureHandler: SubtitleResourceFailureHandler?
    private var tasks: [ObjectIdentifier: Task<Void, Never>] = [:]

    init(
        session: VesperDashSession,
        sourceLoadEpoch: UInt64 = 0,
        subtitleResourceFailureHandler: SubtitleResourceFailureHandler? = nil
    ) {
        self.session = session
        playbackFailures = VesperDashPlaybackFailureStore(session: session, sourceLoadEpoch: sourceLoadEpoch)
        self.subtitleResourceFailureHandler = subtitleResourceFailureHandler
        resourceLoadingQueue = DispatchQueue(
            label: "io.github.umbrella22.vesper.player.dash.resource-loader.\(session.id)"
        )
        super.init()
    }

    func resourceLoader(
        _ resourceLoader: AVAssetResourceLoader,
        shouldWaitForLoadingOfRequestedResource loadingRequest: AVAssetResourceLoadingRequest
    ) -> Bool {
        guard
            let url = loadingRequest.request.url,
            let route = session.route(for: url)
        else {
            return false
        }

        let requestId = ObjectIdentifier(loadingRequest)
        let task = Task { [weak self, session, loadingRequest] in
            do {
                let response: VesperDashResourceResponse
                switch route {
                case .master:
                    response = .resource(
                        .data(
                            try await session.masterPlaylistData(),
                            contentType: "public.m3u-playlist"
                        )
                    )
                case let .media(renditionId):
                    response = .resource(
                        .data(
                            try await session.mediaPlaylistData(renditionId: renditionId),
                            contentType: "public.m3u-playlist"
                        )
                    )
                case let .segment(renditionId, segment):
                    // AVFoundation requires DASH-derived fMP4 resources that
                    // use a custom scheme to redirect to a network URL. A
                    // byte response, and a redirect to file://, both fail on
                    // physical devices with CoreMediaErrorDomain -12881
                    // ("custom url not redirect"). WebVTT remains a byte
                    // response so the custom route can preserve its MIME
                    // classification.
                    if try await session.isSubtitleRendition(renditionId: renditionId) {
                        let payload = try await session.segmentResourcePayload(
                            renditionId: renditionId,
                            segment: segment
                        )
#if DEBUG
                        if segment == .initialization {
                            iosHostLog(
                                "dashResourceInit rendition=\(renditionId) bytes=\(payload.size)"
                            )
                        }
#endif
                        response = .resource(payload.localResourceBody)
                    } else {
                        response = .redirect(
                            try await session.segmentRedirectRequest(
                                renditionId: renditionId,
                                segment: segment
                            )
                        )
                    }
                }
                self?.finish(loadingRequest, requestId: requestId, response: response)
            } catch {
                let subtitleRenditionId: String?
                switch route {
                case let .media(renditionId), let .segment(renditionId, _):
                    subtitleRenditionId = renditionId
                case .master:
                    subtitleRenditionId = nil
                }
                let isSubtitle = if let subtitleRenditionId {
                    await session.isSubtitleRenditionSnapshot(renditionId: subtitleRenditionId)
                } else { false }
                self?.finish(loadingRequest, requestId: requestId, error: error,
                             subtitleRenditionId: isSubtitle ? subtitleRenditionId : nil)
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
        if let task = tasks.removeValue(forKey: requestId) {
            task.cancel()
            playbackFailures.cancel(requestID: requestId)
        }
    }

    private func finish(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        requestId: ObjectIdentifier,
        response: VesperDashResourceResponse
    ) {
        resourceLoadingQueue.async { [weak self] in
            guard let self else { return }
            guard let task = self.tasks.removeValue(forKey: requestId),
                  !task.isCancelled, !loadingRequest.isCancelled else { return }

            switch response {
            case let .resource(body):
                VesperLocalResourceResponder.finish(loadingRequest, body: body)
            case var .redirect(request):
                guard let redirectURL = request.url else {
                    let error = VesperDashBridgeError.network("DASH segment redirect request is missing its URL")
                    let failure = self.playbackFailures.record(error: error, requestID: requestId,
                        requestURL: loadingRequest.request.url ?? self.session.masterPlaylistURL)
                    VesperLocalResourceResponder.finish(loadingRequest, error: (failure as NSError?) ?? error as NSError)
                    return
                }
                request.cachePolicy = .returnCacheDataElseLoad
                loadingRequest.redirect = request
#if DEBUG
                iosHostLog(
                    "dashResourceRedirect from=\(diagnosticURLDescription(loadingRequest.request.url?.absoluteString)) to=\(diagnosticURLDescription(redirectURL.absoluteString))"
                )
#endif
                loadingRequest.response = HTTPURLResponse(
                    url: loadingRequest.request.url ?? redirectURL,
                    statusCode: 302,
                    httpVersion: nil,
                    headerFields: ["Location": redirectURL.absoluteString]
                )
                loadingRequest.finishLoading()
            }
        }
    }

    private func finish(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        requestId: ObjectIdentifier,
        error: Error,
        subtitleRenditionId: String? = nil
    ) {
        resourceLoadingQueue.async { [weak self] in
            guard let self, let task = self.tasks.removeValue(forKey: requestId),
                  !task.isCancelled, !loadingRequest.isCancelled else { return }
            if let subtitleRenditionId, !isVesperDashCancellation(error),
               let handler = self.subtitleResourceFailureHandler {
                Task { @MainActor in handler(subtitleRenditionId) }
            }
            let failure = self.playbackFailures.record(error: error, requestID: requestId,
                requestURL: loadingRequest.request.url ?? self.session.masterPlaylistURL,
                isSubtitle: subtitleRenditionId != nil)
            VesperLocalResourceResponder.finish(loadingRequest, error: (failure as NSError?) ?? error as NSError)
        }
    }
}
