@preconcurrency import AVFoundation
import Foundation
import UIKit
import VesperPlayerKit

struct SourceLifecycleSmokeError: Codable {
    let stage: String
    let code: String
}

struct SourceLifecycleOriginSnapshot: Codable {
    struct MediaRequest: Codable, Equatable {
        let range: String?
        let headersMatched: Bool
    }
    let mediaRequests: [MediaRequest]
    let manifestRequests: Int
}

struct SourceLifecyclePlaybackResult: Codable {
    var passed = false
    var hardwareAvcDecodeSupported = false
    var preloadStatus: String?
    var preloadReasonCode: String?
    var preloadReuse: String?
    var preloadBytes: UInt64?
    var preloadCacheHit: Bool?
    var manifestDeletedAfterPreload = false
    var activationId: String?
    var sessionId: String?
    var sourceId: String?
    var playbackEpoch: UInt64?
    var requestedStartPositionMs: Int64 = 250
    var requestedRate: Float = 1.25
    var pausedPositionMs: Double?
    var pausedPositionAfterWaitMs: Double?
    var pausedPlayerRate: Float?
    var configuredPlayerRate: Float?
    var playingPlayerRate: Float?
    var pausedStartVerified = false
    var rateVerified = false
    var firstFrame = false
    var firstFrameEpoch: UInt64?
    var positionSeconds: Double = 0
    var originVerification = "not_requested"
    var warmOriginRequests: Int?
    var formalOriginRequests: Int?
    var baselineOrigin: SourceLifecycleOriginSnapshot?
    var afterPreloadOrigin: SourceLifecycleOriginSnapshot?
    var afterPlaybackOrigin: SourceLifecycleOriginSnapshot?
    var errors: [SourceLifecycleSmokeError] = []
}

private enum SourceLifecycleSmokeFailure: String, Error {
    case invalidMediaURL, invalidStatsURL, preloadNotCompleted, preloadNotReusable
    case missingPlayer, invalidActivationIdentity, pausedStartMismatch, rateMismatch, firstFrameEpochMismatch
    case playbackTimeout, invalidStatsResponse, statsTooLarge, invalidStatsHistory
    case incorrectWarmRanges, repeatedWarmRange, missingLaterRange, requestHeadersMismatch, unexpectedManifestRequest
}

let sourceLifecycleDefaultMediaURL = URL(string:
    "https://raw.githubusercontent.com/umbrella22/Vesper/v0.7.0/fixtures/media/dash-startup-video.mp4")!

/// Uses only public source registration and activation. The optional counting origin
/// distinguishes preload completion from actual reuse by a new AVPlayer session.
@MainActor
func runSourceLifecyclePlaybackScenario(
    mediaURL: URL = sourceLifecycleDefaultMediaURL,
    statsURL: URL? = nil
) async -> SourceLifecyclePlaybackResult {
    var result = SourceLifecyclePlaybackResult()
    result.hardwareAvcDecodeSupported = VesperCodecSupport.hardwareDecodeSupported(for: "avc1.42c00a")
    result.originVerification = statsURL == nil ? "not_requested" : "pending"
    var stage = "configuration"
    let headers = ["X-Vesper-Fixture": "source-lifecycle-080"]
    let manifestURL = FileManager.default.temporaryDirectory
        .appendingPathComponent("source-lifecycle-\(UUID().uuidString).mpd")
    var sourceSession: VesperSourceSession?
    var controller: VesperPlayerController?
    var smokeWindow: UIWindow?
    defer {
        controller?.dispose()
        sourceSession?.close()
        smokeWindow?.isHidden = true
        try? FileManager.default.removeItem(at: manifestURL)
    }
    do {
        guard mediaURL.scheme?.lowercased() == "https", mediaURL.host != nil,
              mediaURL.user == nil, mediaURL.password == nil else { throw SourceLifecycleSmokeFailure.invalidMediaURL }
        if let statsURL {
            guard statsURL.scheme?.lowercased() == "https", statsURL.host != nil,
                  statsURL.user == nil, statsURL.password == nil else { throw SourceLifecycleSmokeFailure.invalidStatsURL }
            stage = "baseline_stats"
            result.baselineOrigin = try await sourceLifecycleReadStats(statsURL, headers: headers)
        }

        stage = "manifest"
        // The committed six-second fixture has one SIDX and three independent video fragments.
        let escapedMediaURL = mediaURL.absoluteString.replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;").replacingOccurrences(of: ">", with: "&gt;")
        let manifest = Data("""
        <MPD type="static" mediaPresentationDuration="PT6S"><Period id="p0">
        <AdaptationSet id="v" contentType="video" mimeType="video/mp4">
        <Representation id="v1" bandwidth="50000" codecs="avc1.42c00a" width="64" height="64">
        <BaseURL>\(escapedMediaURL)</BaseURL><SegmentBase indexRange="771-846"><Initialization range="0-770"/></SegmentBase>
        </Representation></AdaptationSet></Period></MPD>
        """.utf8)
        try manifest.write(to: manifestURL, options: .atomic)
        let session = try VesperSourceSession()
        sourceSession = session
        let descriptor = VesperPlayerSource(uri: manifestURL.absoluteString, label: "Source lifecycle fixture",
                                            kind: .local, protocol: .dash, headers: headers)
        let handle = try session.register(descriptor)
        result.sessionId = handle.sessionId
        result.sourceId = handle.id

        stage = "preload"
        let preload = try handle.preload(options: .init(timeoutMs: 15_000))
        let preloadResult = await preload.result
        result.preloadStatus = preloadResult.status.rawValue
        result.preloadReasonCode = preloadResult.reasonCode
        result.preloadReuse = preloadResult.capability.rawValue
        result.preloadBytes = preloadResult.actualBytes
        result.preloadCacheHit = preloadResult.cacheHit
        guard preloadResult.status == .completed else { throw SourceLifecycleSmokeFailure.preloadNotCompleted }
        guard preloadResult.capability == .playbackReusable else { throw SourceLifecycleSmokeFailure.preloadNotReusable }
        try FileManager.default.removeItem(at: manifestURL)
        result.manifestDeletedAfterPreload = !FileManager.default.fileExists(atPath: manifestURL.path)

        if let statsURL {
            stage = "preload_stats"
            result.afterPreloadOrigin = try await sourceLifecycleReadStats(statsURL, headers: headers)
            result.warmOriginRequests = try sourceLifecycleWarmRequests(
                baseline: result.baselineOrigin!, afterPreload: result.afterPreloadOrigin!)
        }

        stage = "activation"
        let playerController = VesperPlayerControllerFactory.makeDefault(resiliencePolicy: .streaming(),
                                                                         keepScreenOnDuringPlayback: false)
        controller = playerController
        let surface = PlayerSurfaceView(frame: CGRect(x: 0, y: 0, width: 192, height: 192))
        let window: UIWindow
        if let scene = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).first {
            window = UIWindow(windowScene: scene)
        } else { window = UIWindow(frame: surface.bounds) }
        smokeWindow = window
        let root = UIViewController()
        root.view.backgroundColor = .black
        window.rootViewController = root
        root.view.addSubview(surface)
        window.makeKeyAndVisible()
        playerController.attachSurfaceHost(surface)
        let activation = try await playerController.activate(handle, options: .init(
            playWhenReady: false, startPositionMs: result.requestedStartPositionMs,
            playbackRate: result.requestedRate, timeoutMs: 15_000))
        result.activationId = activation.activationId
        result.playbackEpoch = activation.playbackEpoch
        guard !activation.activationId.isEmpty, activation.sessionId == handle.sessionId,
              activation.sourceId == handle.id, activation.playbackEpoch > 0 else {
            throw SourceLifecycleSmokeFailure.invalidActivationIdentity
        }
        guard let layer = surface.pictureInPicturePlayerLayer, let player = layer.player else {
            throw SourceLifecycleSmokeFailure.missingPlayer
        }
        playerController.refresh()
        result.pausedPositionMs = sourceLifecycleFinitePosition(player) * 1_000
        result.pausedPlayerRate = player.rate
        result.configuredPlayerRate = player.defaultRate
        try await Task.sleep(for: .milliseconds(350))
        result.pausedPositionAfterWaitMs = sourceLifecycleFinitePosition(player) * 1_000
        result.pausedStartVerified = abs((result.pausedPositionMs ?? -1) - Double(result.requestedStartPositionMs)) <= 150
            && abs((result.pausedPositionAfterWaitMs ?? -1) - (result.pausedPositionMs ?? -1)) <= 50
            && player.rate == 0 && result.pausedPlayerRate == 0
        result.rateVerified = abs(player.defaultRate - result.requestedRate) < 0.01
        guard result.pausedStartVerified else { throw SourceLifecycleSmokeFailure.pausedStartMismatch }
        guard result.rateVerified else { throw SourceLifecycleSmokeFailure.rateMismatch }

        stage = "playback"
        playerController.play()
        let deadline = ContinuousClock.now.advanced(by: .seconds(20))
        while ContinuousClock.now < deadline {
            try Task.checkCancellation()
            playerController.refresh()
            surface.layoutIfNeeded()
            result.firstFrame = layer.isReadyForDisplay
            result.firstFrameEpoch = playerController.playbackDiagnostics?.firstFrame?.playbackEpoch
            result.positionSeconds = sourceLifecycleFinitePosition(player)
            if player.rate > 0 { result.playingPlayerRate = player.rate }
            if result.firstFrame && result.firstFrameEpoch == activation.playbackEpoch && result.positionSeconds > 3 { break }
            try await Task.sleep(for: .milliseconds(100))
        }
        playerController.pause()
        guard result.firstFrame && result.positionSeconds > 3 else { throw SourceLifecycleSmokeFailure.playbackTimeout }
        guard result.firstFrameEpoch == activation.playbackEpoch else { throw SourceLifecycleSmokeFailure.firstFrameEpochMismatch }
        result.rateVerified = result.rateVerified && abs((result.playingPlayerRate ?? 0) - result.requestedRate) < 0.01
        guard result.rateVerified else { throw SourceLifecycleSmokeFailure.rateMismatch }

        if let statsURL {
            stage = "playback_stats"
            result.afterPlaybackOrigin = try await sourceLifecycleReadStats(statsURL, headers: headers)
            result.formalOriginRequests = try sourceLifecycleFormalRequests(
                baseline: result.baselineOrigin!, afterPreload: result.afterPreloadOrigin!,
                afterPlayback: result.afterPlaybackOrigin!)
            result.originVerification = "verified"
        }
        result.passed = true
    } catch {
        let code = sourceLifecycleErrorCode(error)
        result.errors.append(.init(stage: stage, code: code))
        if statsURL != nil && result.originVerification != "verified" { result.originVerification = "failed" }
        if let statsURL, result.baselineOrigin != nil, result.afterPlaybackOrigin == nil {
            // Preserve counting-origin evidence even when preload or player startup fails.
            do {
                let final = try await sourceLifecycleReadStats(statsURL, headers: headers)
                result.afterPlaybackOrigin = final
                if let afterPreload = result.afterPreloadOrigin,
                   final.mediaRequests.starts(with: afterPreload.mediaRequests) {
                    result.formalOriginRequests = final.mediaRequests.count - afterPreload.mediaRequests.count
                }
            } catch {
                result.errors.append(.init(stage: "failure_stats", code: "stats_unavailable"))
            }
        }
    }
    return result
}

private func sourceLifecycleErrorCode(_ error: Error) -> String {
    if let failure = error as? SourceLifecycleSmokeFailure { return failure.rawValue }
    if let failure = error as? VesperSourceActivationError { return failure.rawValue }
    if let failure = error as? VesperSourceSessionError { return failure.rawValue }
    if let failure = error as? VesperPlayerError { return "\(failure.category.rawValue):\(failure.code.rawValue)" }
    let failure = error as NSError
    return "\(failure.domain):\(failure.code)"
}

private func sourceLifecycleFinitePosition(_ player: AVPlayer) -> Double {
    let position = player.currentTime().seconds
    return position.isFinite ? position : 0
}

private func sourceLifecycleReadStats(_ url: URL, headers: [String: String]) async throws -> SourceLifecycleOriginSnapshot {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.urlCache = nil
    configuration.timeoutIntervalForRequest = 10
    configuration.timeoutIntervalForResource = 10
    let session = URLSession(configuration: configuration)
    defer { session.invalidateAndCancel() }
    var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData)
    headers.forEach { request.setValue($1, forHTTPHeaderField: $0) }
    let (bytes, response) = try await session.bytes(for: request)
    guard let response = response as? HTTPURLResponse, response.statusCode == 200 else {
        throw SourceLifecycleSmokeFailure.invalidStatsResponse
    }
    var data = Data()
    for try await byte in bytes {
        guard data.count < 64 * 1024 else { throw SourceLifecycleSmokeFailure.statsTooLarge }
        data.append(byte)
    }
    let value = try JSONDecoder().decode(SourceLifecycleOriginSnapshot.self, from: data)
    guard value.manifestRequests >= 0, value.mediaRequests.count <= 256 else { throw SourceLifecycleSmokeFailure.statsTooLarge }
    return value
}

private let sourceLifecycleWarmRanges: Set<String> = ["bytes=0-770", "bytes=771-846", "bytes=847-11668"]

private func sourceLifecycleWarmRequests(baseline: SourceLifecycleOriginSnapshot,
                                         afterPreload: SourceLifecycleOriginSnapshot) throws -> Int {
    guard afterPreload.mediaRequests.starts(with: baseline.mediaRequests) else { throw SourceLifecycleSmokeFailure.invalidStatsHistory }
    let warm = Array(afterPreload.mediaRequests.dropFirst(baseline.mediaRequests.count))
    guard warm.count == 3, Set(warm.compactMap(\.range)) == sourceLifecycleWarmRanges else {
        throw SourceLifecycleSmokeFailure.incorrectWarmRanges
    }
    guard warm.allSatisfy(\.headersMatched) else { throw SourceLifecycleSmokeFailure.requestHeadersMismatch }
    guard afterPreload.manifestRequests == baseline.manifestRequests else { throw SourceLifecycleSmokeFailure.unexpectedManifestRequest }
    return warm.count
}

private func sourceLifecycleFormalRequests(baseline: SourceLifecycleOriginSnapshot,
                                           afterPreload: SourceLifecycleOriginSnapshot,
                                           afterPlayback: SourceLifecycleOriginSnapshot) throws -> Int {
    guard afterPlayback.mediaRequests.starts(with: afterPreload.mediaRequests) else { throw SourceLifecycleSmokeFailure.invalidStatsHistory }
    let formal = Array(afterPlayback.mediaRequests.dropFirst(afterPreload.mediaRequests.count))
    guard !formal.contains(where: { sourceLifecycleWarmRanges.contains($0.range ?? "") }) else {
        throw SourceLifecycleSmokeFailure.repeatedWarmRange
    }
    // A full-file fetch would contain the warmed prefix too, so it is not reuse evidence.
    guard !formal.isEmpty, formal.allSatisfy({ request in
        guard let range = request.range, range.hasPrefix("bytes="),
              let start = Int(range.dropFirst(6).split(separator: "-").first ?? "") else { return false }
        return start >= 11_669
    }) else { throw SourceLifecycleSmokeFailure.missingLaterRange }
    guard afterPlayback.mediaRequests.dropFirst(baseline.mediaRequests.count).allSatisfy(\.headersMatched) else {
        throw SourceLifecycleSmokeFailure.requestHeadersMismatch
    }
    guard afterPlayback.manifestRequests == baseline.manifestRequests else { throw SourceLifecycleSmokeFailure.unexpectedManifestRequest }
    return formal.count
}
