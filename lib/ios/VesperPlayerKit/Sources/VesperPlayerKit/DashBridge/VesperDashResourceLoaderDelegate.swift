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
                    // Both initialization and media segments route through
                    // `segmentResourcePayload(...).localResourceBody`, which
                    // applies `dashSegmentContentType` + `avResourceContentType`.
                    // This ensures subtitle init/media segments receive
                    // `public.webvtt` rather than the hardcoded
                    // `public.mpeg-4` previously applied to init only.
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
            case let .redirect(url):
                var request = URLRequest(url: url)
                request.cachePolicy = .returnCacheDataElseLoad
                loadingRequest.redirect = request
#if DEBUG
                iosHostLog(
                    "dashResourceRedirect from=\(diagnosticURLDescription(loadingRequest.request.url?.absoluteString)) to=\(diagnosticURLDescription(url.absoluteString))"
                )
#endif
                loadingRequest.response = HTTPURLResponse(
                    url: loadingRequest.request.url ?? url,
                    statusCode: 302,
                    httpVersion: nil,
                    headerFields: ["Location": url.absoluteString]
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
