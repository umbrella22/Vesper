@preconcurrency import AVFoundation
import Foundation
import VesperPlayerKitBridgeShim

final class VesperDashResourceLoaderDelegate: NSObject, AVAssetResourceLoaderDelegate {
    let resourceLoadingQueue: DispatchQueue

    private let session: VesperDashSession
    private var tasks: [ObjectIdentifier: Task<Void, Never>] = [:]

    init(session: VesperDashSession) {
        self.session = session
        resourceLoadingQueue = DispatchQueue(
            label: "io.github.ikaros.vesper.player.dash.resource-loader.\(session.id)"
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
                    switch segment {
                    case .initialization:
                        // Init segments are small and AVPlayer normally fetches them once, so
                        // return the raw bytes through the resource loader. This keeps init
                        // delivery visible to benchmark events and avoids relying on local HTTP
                        // behavior for EXT-X-MAP.
                        let initData = try await session.segmentData(
                            renditionId: renditionId,
                            segment: .initialization
                        )
#if DEBUG
                        iosHostLog(
                            "dashResourceInit rendition=\(renditionId) bytes=\(initData.count)"
                        )
#endif
                        // contentType must be a UTI, not a MIME type. fMP4 / ISO BMFF maps to public.mpeg-4.
                        response = .resource(.data(initData, contentType: "public.mpeg-4"))
                    case .media:
                        response = .resource(
                            try await session.segmentResourcePayload(
                                renditionId: renditionId,
                                segment: segment
                            ).localResourceBody
                        )
                    }
                }
                self?.finish(loadingRequest, requestId: requestId, response: response)
            } catch {
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
                    "dashResourceRedirect from=\(loadingRequest.request.url?.absoluteString ?? "nil") to=\(url.absoluteString)"
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
