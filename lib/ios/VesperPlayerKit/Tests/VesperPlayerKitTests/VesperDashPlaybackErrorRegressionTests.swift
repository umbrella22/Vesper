import AVFoundation
import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperDashPlaybackErrorRegressionTests: XCTestCase {
    func testUnsupportedDASHEnumsKeepCapabilityAndOriginalReason() {
        let bridge = VesperNativePlayerBridge()
        defer { bridge.dispose() }
        for cause: VesperDashBridgeError in [
            .unsupportedManifest("DASH ContentProtection/DRM is not supported on iOS"),
            .unsupportedManifest("multi-period DASH is not supported"),
            .unsupportedMp4("unsupported SIDX reference type"),
        ] {
            let result = bridge.classifyPlaybackFailure(cause, fallbackMessage: "AVPlayer failed")
            XCTAssertEqual(result.code, .unsupported)
            XCTAssertEqual(result.category, .capability)
            XCTAssertFalse(result.retriable)
            XCTAssertEqual(result.message, cause.errorDescription)
        }
    }

    func testMalformedDASHEnumsRemainSourceFailures() {
        let bridge = VesperNativePlayerBridge()
        defer { bridge.dispose() }
        for cause: VesperDashBridgeError in [.invalidManifest("malformed XML"), .invalidMp4("truncated SIDX")] {
            let result = bridge.classifyPlaybackFailure(cause, fallbackMessage: "AVPlayer failed")
            XCTAssertEqual(result.code, .invalidSource)
            XCTAssertEqual(result.category, .source)
            XCTAssertFalse(result.retriable)
            XCTAssertEqual(result.message, cause.errorDescription)
        }
    }

    func testDASHNetworkEnumRetainsHTTPSBoundaryMessage() {
        let source = VesperPlayerSource.dash(url: URL(string: "http://127.0.0.1:9/manifest.mpd")!, label: "insecure")
        let bridge = VesperNativePlayerBridge(initialSource: source)
        defer { bridge.dispose() }
        let cause = VesperDashBridgeError.network(vesperDashATSFailureMessage)
        let result = bridge.classifyPlaybackFailure(cause, fallbackMessage: "enum error 5")
        XCTAssertEqual(result.category, .network)
        XCTAssertEqual(result.code, .backendFailure)
        XCTAssertFalse(result.retriable, "The SDK never retries a source it rejects for insecure HTTP")
        XCTAssertEqual(result.message, cause.errorDescription)
    }

    func testUnrelatedMissingFileIsNotUnsupportedDASH() {
        let bridge = VesperNativePlayerBridge()
        defer { bridge.dispose() }
        let missing = NSError(domain: NSURLErrorDomain, code: NSURLErrorFileDoesNotExist,
                              userInfo: [NSLocalizedDescriptionKey: "file does not exist"])
        let result = bridge.classifyPlaybackFailure(missing, fallbackMessage: "missing")
        XCTAssertEqual(result.code, .invalidSource)
        XCTAssertEqual(result.category, .source)
        XCTAssertFalse(result.retriable)
    }

    func testExistingLocalNegativeMPDsReachParserWithoutURLSessionFileLookup() async throws {
        for (name, text) in [
            ("encrypted", sampleMpd.replacingOccurrences(of: "<Period id=\"p0\">",
                with: "<Period id=\"p0\"><ContentProtection schemeIdUri=\"urn:mpeg:dash:mp4protection:2011\"/>")),
            ("multi-period", sampleMpd.replacingOccurrences(of: "</MPD>", with: "<Period id=\"p1\"/></MPD>")),
        ] {
            let file = FileManager.default.temporaryDirectory.appendingPathComponent("\(UUID().uuidString)-\(name).mpd")
            try Data(text.utf8).write(to: file)
            defer { try? FileManager.default.removeItem(at: file) }
            XCTAssertTrue(FileManager.default.fileExists(atPath: file.path))
            let (bytes, effectiveURL) = try await VesperDashNetworkClient().manifestData(for: file)
            XCTAssertEqual(bytes, Data(text.utf8))
            XCTAssertEqual(effectiveURL, file)
            let session = makeTestDashSession(sourceURL: file)
            do {
                _ = try await session.masterPlaylistData()
                XCTFail("An unsupported local MPD must not produce a playable master")
            } catch let cause as VesperDashBridgeError {
                guard case .unsupportedManifest = cause else { return XCTFail("Wrong parser failure: \(cause)") }
                let bridge = VesperNativePlayerBridge()
                defer { bridge.dispose() }
                let result = bridge.classifyPlaybackFailure(cause, fallbackMessage: "URL not found")
                XCTAssertEqual(result.code, .unsupported)
                XCTAssertEqual(result.category, .capability)
                XCTAssertEqual(result.message, cause.errorDescription)
            }
        }
    }

    func testLocalNegativeSourceHandlesUseDefaultStartupClientAndKeepParserCause() async throws {
        for text in [
            sampleMpd.replacingOccurrences(of: "<Period id=\"p0\">",
                with: "<Period id=\"p0\"><ContentProtection schemeIdUri=\"urn:mpeg:dash:mp4protection:2011\"/>"),
            sampleMpd.replacingOccurrences(of: "</MPD>", with: "<Period id=\"p1\"/></MPD>"),
        ] {
            let file = FileManager.default.temporaryDirectory.appendingPathComponent("\(UUID().uuidString).mpd")
            try Data(text.utf8).write(to: file)
            defer { try? FileManager.default.removeItem(at: file) }
            let owner = try VesperSourceSession()
            let handle = try owner.register(.init(uri: file.absoluteString, label: "negative", kind: .local, protocol: .dash))
            let lease = try handle.acquire()
            let source = try lease.sourceForActivation()
            XCTAssertNotNil(source.dashStartupScope)
            let bridge = VesperNativePlayerBridge(initialSource: source)
            defer { bridge.dispose(); lease.release(); owner.close() }
            _ = try bridge.makePlayerItem(for: source, url: file)
            let session = try XCTUnwrap(bridge.currentDashSession)
            let client = await session.networkClient
            XCTAssertTrue(client is VesperDashStartupNetworkClient)
            let (bytes, finalURL) = try await client.manifestData(for: file)
            XCTAssertEqual(bytes, Data(text.utf8))
            XCTAssertEqual(finalURL, file)
            do {
                _ = try await session.masterPlaylistData()
                XCTFail("A scoped source handle must retain the unsupported parser failure")
            } catch let cause as VesperDashBridgeError {
                guard case .unsupportedManifest = cause else { return XCTFail("Wrong scoped parser failure: \(cause)") }
                let result = bridge.classifyPlaybackFailure(cause, fallbackMessage: "URL not found")
                XCTAssertEqual(result.code, .unsupported)
                XCTAssertEqual(result.category, .capability)
                XCTAssertEqual(result.message, cause.errorDescription)
            }
        }
    }

    func testPublicActivationOfLocalNegativeHandlesRetainsUnsupportedCauseAndReason() async throws {
        for cause: VesperDashBridgeError in [
            .unsupportedManifest("DASH ContentProtection/DRM is not supported on iOS"),
            .unsupportedManifest("unsupported MPD: multi-period DASH is not supported"),
        ] {
            let isEncrypted = cause.errorDescription == VesperDashBridgeError.unsupportedManifest(
                "DASH ContentProtection/DRM is not supported on iOS").errorDescription
            let text = isEncrypted
                ? sampleMpd.replacingOccurrences(of: "<Period id=\"p0\">",
                    with: "<Period id=\"p0\"><ContentProtection schemeIdUri=\"urn:mpeg:dash:mp4protection:2011\"/>")
                : sampleMpd.replacingOccurrences(of: "</MPD>", with: "<Period id=\"p1\"/></MPD>")
            let file = FileManager.default.temporaryDirectory.appendingPathComponent("\(UUID().uuidString).mpd")
            try Data(text.utf8).write(to: file)
            defer { try? FileManager.default.removeItem(at: file) }
            let owner = try VesperSourceSession()
            let handle = try owner.register(.init(uri: file.absoluteString, label: "negative", kind: .local, protocol: .dash))
            let bridge = VesperNativePlayerBridge()
            let controller = VesperPlayerController(bridge)
            defer { controller.dispose(); owner.close() }
            do {
                _ = try await controller.activate(handle, options: .init(playWhenReady: false, timeoutMs: 5_000))
                XCTFail("Unsupported MPD activation succeeded")
            } catch let actual as VesperPlayerError {
                XCTAssertEqual(actual.code, .unsupported)
                XCTAssertEqual(actual.category, .capability)
                XCTAssertFalse(actual.retriable)
                XCTAssertEqual(actual.message, cause.errorDescription)
                XCTAssertEqual(actual.details["dashErrorKind"], "unsupportedManifest")
                XCTAssertEqual(actual.details["dashFailurePhase"], "resourceLoader")
                XCTAssertNotNil(actual.details["dashSessionId"])
                XCTAssertNotNil(actual.details["dashSourceLoadEpoch"])
            }
        }
    }

    func testCurrentResourceCauseSurvivesAVPlayerReplacementAndUnderlyingWrapper() throws {
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("\(UUID().uuidString).mpd")
        let source = VesperPlayerSource(uri: file.absoluteString, label: "negative", kind: .local, protocol: .dash)
        let bridge = VesperNativePlayerBridge(initialSource: source)
        defer { bridge.dispose() }
        bridge.sourceLoadEpoch = 8
        let item = try bridge.makePlayerItem(for: source, url: file)
        bridge.player = AVPlayer(playerItem: item)
        let session = try XCTUnwrap(bridge.currentDashSession)
        let loader = try XCTUnwrap(bridge.dashResourceLoaderDelegate)
        let request = NSObject()
        let cause = VesperDashBridgeError.unsupportedManifest("multi-period DASH is not supported")
        let resource = try XCTUnwrap(loader.playbackFailures.record(error: cause,
            requestID: ObjectIdentifier(request), requestURL: session.masterPlaylistURL))
        let replacement = NSError(domain: NSURLErrorDomain, code: NSURLErrorFileDoesNotExist,
            userInfo: [NSLocalizedDescriptionKey: "The requested URL was not found on this server."])
        let result = bridge.resolvedPlaybackFailure(error: replacement, fallbackMessage: "AVPlayer failed")
        XCTAssertEqual(result.code, .unsupported)
        XCTAssertEqual(result.category, .capability)
        XCTAssertFalse(result.retriable)
        XCTAssertEqual(result.message, cause.errorDescription)
        XCTAssertEqual(result.details["dashErrorKind"], "unsupportedManifest")
        XCTAssertEqual(result.details["dashSourceLoadEpoch"], "8")
        XCTAssertEqual(result.details["nativeErrorDomain"], NSURLErrorDomain)
        XCTAssertEqual(result.details["nativeErrorCode"], "\(NSURLErrorFileDoesNotExist)")
        let wrapped = NSError(domain: AVFoundationErrorDomain, code: AVError.unknown.rawValue,
            userInfo: [NSUnderlyingErrorKey: resource])
        XCTAssertEqual(bridge.classifyPlaybackFailure(wrapped, fallbackMessage: "wrapped").message, cause.errorDescription)
        XCTAssertEqual(bridge.classifyPlaybackFailure(wrapped, fallbackMessage: "wrapped").code, .unsupported)
        let cancellation = NSError(domain: NSURLErrorDomain, code: NSURLErrorCancelled)
        XCTAssertEqual((bridge.playbackFailurePreservingDashCause(cancellation) as? NSError)?.code, NSURLErrorCancelled)
        let commandTimeout = VesperPlayerError(message: "command timed out", code: .timeout,
            category: .source, retriable: false)
        XCTAssertEqual(bridge.resolvedPlaybackFailure(error: commandTimeout, fallbackMessage: "timeout").code, .timeout)

        // Re-activation of the same URI is a different load generation.
        bridge.sourceLoadEpoch += 1
        XCTAssertNil(bridge.currentDashResourceFailure(item: item))
        XCTAssertEqual(bridge.resolvedPlaybackFailure(error: replacement, fallbackMessage: "stale").code, .invalidSource)
        XCTAssertNotEqual(bridge.classifyPlaybackFailure(wrapped, fallbackMessage: "stale").code, .unsupported)
        bridge.sourceLoadEpoch = 8
        XCTAssertNil(bridge.currentDashResourceFailure(item: AVPlayerItem(url: file)))
        let next = try bridge.makePlayerItem(for: source, url: file)
        bridge.player?.replaceCurrentItem(with: next)
        XCTAssertNil(bridge.currentDashResourceFailure(item: next))
        XCTAssertNotEqual(bridge.classifyPlaybackFailure(wrapped, fallbackMessage: "old session").code, .unsupported)
        bridge.currentSource = .remoteUrl(URL(string: "https://fixture.test/control.mp4")!)
        XCTAssertNil(bridge.currentDashResourceFailure(item: next))
    }

    func testResourceStoreExcludesCancelledAndSubtitleFailures() throws {
        let session = VesperDashSession(sourceURL: URL(fileURLWithPath: "/tmp/negative.mpd"))
        let store = VesperDashPlaybackFailureStore(session: session, sourceLoadEpoch: 3)
        let request = NSObject()
        let id = ObjectIdentifier(request)
        let url = session.masterPlaylistURL
        let cause = VesperDashBridgeError.unsupportedManifest("multi-period DASH is not supported")
        XCTAssertNil(store.record(error: CancellationError(), requestID: id, requestURL: url))
        XCTAssertNil(store.record(error: NSError(domain: NSURLErrorDomain, code: NSURLErrorCancelled),
                                  requestID: id, requestURL: url))
        XCTAssertNil(store.record(error: cause, requestID: id, requestURL: url, isCancelled: true))
        XCTAssertNil(store.record(error: cause, requestID: id, requestURL: url, isSubtitle: true))
        let subtitle = VesperDashBridgeError.subtitle(.init(code: "malformed", phase: "segment",
            trackId: "sub", retriable: false, message: "malformed subtitle"))
        XCTAssertNil(store.record(error: subtitle, requestID: id, requestURL: url))
        XCTAssertNil(store.failure(sessionID: session.id, sourceLoadEpoch: 3, itemURL: url))
        XCTAssertNotNil(store.record(error: cause, requestID: id, requestURL: url))
        XCTAssertNil(store.failure(sessionID: UUID().uuidString, sourceLoadEpoch: 3, itemURL: url))
        XCTAssertNil(store.failure(sessionID: session.id, sourceLoadEpoch: 4, itemURL: url))
        XCTAssertNil(store.failure(sessionID: session.id, sourceLoadEpoch: 3, itemURL: session.mediaPlaylistURL(for: "v")))
        store.cancel(requestID: id)
        XCTAssertNil(store.failure(sessionID: session.id, sourceLoadEpoch: 3, itemURL: url))
    }

    func testResourceWrappingKeepsRealNetworkAndMissingFileClassification() throws {
        let file = URL(fileURLWithPath: "/tmp/missing-\(UUID().uuidString).mpd")
        let source = VesperPlayerSource(uri: file.absoluteString, label: "local", kind: .local, protocol: .dash)
        let bridge = VesperNativePlayerBridge(initialSource: source)
        defer { bridge.dispose() }
        _ = try bridge.makePlayerItem(for: source, url: file)
        let session = try XCTUnwrap(bridge.currentDashSession)
        for (cause, code, category, retry): (NSError, VesperPlayerErrorCode, VesperPlayerErrorCategory, Bool) in [
            (NSError(domain: NSURLErrorDomain, code: NSURLErrorFileDoesNotExist), .invalidSource, .source, false),
            (NSError(domain: NSURLErrorDomain, code: NSURLErrorTimedOut), .backendFailure, .network, true),
        ] {
            let resource = VesperDashResourceFailure(cause: cause, sessionID: session.id,
                sourceLoadEpoch: bridge.sourceLoadEpoch, requestURL: session.masterPlaylistURL)
            let result = bridge.classifyPlaybackFailure(resource, fallbackMessage: "generic AV failure")
            XCTAssertEqual(result.code, code)
            XCTAssertEqual(result.category, category)
            XCTAssertEqual(result.retriable, retry)
            XCTAssertEqual(result.message, cause.localizedDescription)
        }
    }

    func testPublicInsecureSourceHandleKeepsHTTPSBoundaryAndDoesNotRetry() async throws {
        let url = URL(string: "http://127.0.0.1:9/manifest.mpd")!
        let owner = try VesperSourceSession()
        let handle = try owner.register(.dash(url: url, label: "insecure"))
        let bridge = VesperNativePlayerBridge()
        let controller = VesperPlayerController(bridge)
        defer { controller.dispose(); owner.close() }
        do {
            _ = try await controller.activate(handle, options: .init(playWhenReady: false, timeoutMs: 5_000))
            XCTFail("Insecure HTTP activation succeeded")
        } catch let actual as VesperPlayerError {
            let original = VesperDashBridgeError.network(
                "\(vesperDashATSFailureMessage) URL: \(diagnosticURLDescription(url.absoluteString))")
            XCTAssertEqual(actual.code, .backendFailure)
            XCTAssertEqual(actual.category, .network)
            XCTAssertFalse(actual.retriable)
            XCTAssertEqual(actual.message, original.errorDescription)
            XCTAssertEqual(actual.details["dashErrorKind"], "network")
            XCTAssertEqual(actual.details["dashFailurePhase"], "resourceLoader")
        }
    }
}
