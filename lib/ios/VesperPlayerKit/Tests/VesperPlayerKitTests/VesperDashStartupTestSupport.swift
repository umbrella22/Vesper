import Foundation
@testable import VesperPlayerKit

actor DashStartupFixtureTransport: VesperDashStartupTransport {
    private(set) var requests = 0
    var failAt: Int?
    let video: Data
    let manifest: Data
    init(mediaURL: String = "https://fixture.test/video.mp4", failAt: Int? = nil) throws {
        self.failAt = failAt
        let bundle = Bundle(for: DashStartupFixtureBundle.self)
        guard let url = bundle.url(forResource: "dash-startup-video", withExtension: "mp4") else {
            throw VesperDashStartupError.invalidResponse
        }
        video = try Data(contentsOf: url)
        manifest = Data("""
        <MPD type="static" mediaPresentationDuration="PT6S"><Period id="p0">
        <AdaptationSet id="v" contentType="video" mimeType="video/mp4">
        <Representation id="v1" bandwidth="50000" codecs="avc1.42c00a" width="64" height="64">
        <BaseURL>\(mediaURL)</BaseURL><SegmentBase indexRange="771-846"><Initialization range="0-770"/></SegmentBase>
        </Representation></AdaptationSet></Period></MPD>
        """.utf8)
    }
    func fetch(_ resource: VesperDashStartupResource, headers: [String: String], maximumBytes: Int) async throws -> VesperDashStartupBytes {
        requests += 1
        if requests == failAt { throw URLError(.networkConnectionLost) }
        let data: Data
        if resource.url.path.hasSuffix(".mpd") { data = manifest }
        else if let range = resource.range { data = video.subdata(in: Int(range.start)..<(Int(range.end) + 1)) }
        else { data = video }
        guard data.count <= maximumBytes else { throw VesperDashStartupError.budgetExceeded }
        return .init(resource: resource, data: data, finalURL: resource.url)
    }
}

private final class DashStartupFixtureBundle: NSObject {}
