@preconcurrency import AVFoundation
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

final class VesperDashResourceLoaderDelegate: NSObject, AVAssetResourceLoaderDelegate {
    typealias SubtitleResourceFailureHandler = @MainActor @Sendable (String) -> Void

    let resourceLoadingQueue: DispatchQueue

    private let session: VesperDashSession
    private let subtitleResourceFailureHandler: SubtitleResourceFailureHandler?
    private var tasks: [ObjectIdentifier: Task<Void, Never>] = [:]

    init(
        session: VesperDashSession,
        subtitleResourceFailureHandler: SubtitleResourceFailureHandler? = nil
    ) {
        self.session = session
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
                    if await session.isSubtitleRendition(renditionId: renditionId) {
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
                if let subtitleRenditionId,
                   await session.isSubtitleRendition(renditionId: subtitleRenditionId),
                   let subtitleResourceFailureHandler = self?.subtitleResourceFailureHandler
                {
                    Task { @MainActor in
                        subtitleResourceFailureHandler(subtitleRenditionId)
                    }
                }
                self?.finish(loadingRequest, requestId: requestId, error: error)
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

    private func finish(
        _ loadingRequest: AVAssetResourceLoadingRequest,
        requestId: ObjectIdentifier,
        response: VesperDashResourceResponse
    ) {
        resourceLoadingQueue.async { [weak self] in
            guard let self else { return }
            self.tasks.removeValue(forKey: requestId)

            switch response {
            case let .resource(body):
                VesperLocalResourceResponder.finish(loadingRequest, body: body)
            case var .redirect(request):
                guard let redirectURL = request.url else {
                    VesperLocalResourceResponder.finish(
                        loadingRequest,
                        error: VesperDashBridgeError.network(
                            "DASH segment redirect request is missing its URL"
                        )
                    )
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
        error: Error
    ) {
        resourceLoadingQueue.async { [weak self] in
            self?.tasks.removeValue(forKey: requestId)
            VesperLocalResourceResponder.finish(loadingRequest, error: error)
        }
    }
}
