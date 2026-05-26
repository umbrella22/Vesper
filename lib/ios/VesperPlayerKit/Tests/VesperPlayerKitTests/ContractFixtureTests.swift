import Foundation
import XCTest
@testable import VesperPlayerKit

final class ContractFixtureTests: XCTestCase {
    func testSharedPlayerErrorContractKeepsStableFields() throws {
        let payload = try contractMap("player_error")
        let error = VesperPlayerError(
            message: try XCTUnwrap(payload["message"] as? String),
            code: VesperPlayerErrorCode(rawValue: try XCTUnwrap(payload["code"] as? String)) ?? .backendFailure,
            category: VesperPlayerErrorCategory(rawValue: try XCTUnwrap(payload["category"] as? String)) ?? .platform,
            retriable: try XCTUnwrap(payload["retriable"] as? Bool)
        )

        XCTAssertEqual(error.message, "fixture unsupported capability")
        XCTAssertEqual(error.code, .unsupported)
        XCTAssertEqual(error.category, .capability)
        XCTAssertFalse(error.retriable)
        let details = try XCTUnwrap(payload["details"] as? [String: Any])
        XCTAssertEqual(details["operation"] as? String, "setAbrPolicy")
    }

    func testSharedDownloadTaskContractKeepsStableFields() throws {
        let payload = try contractMap("download_task_snapshot")
        let sourcePayload = try XCTUnwrap(payload["source"] as? [String: Any])
        let playerSourcePayload = try XCTUnwrap(sourcePayload["source"] as? [String: Any])
        let profilePayload = try XCTUnwrap(payload["profile"] as? [String: Any])
        let progressPayload = try XCTUnwrap(payload["progress"] as? [String: Any])
        let assetIndexPayload = try XCTUnwrap(payload["assetIndex"] as? [String: Any])

        let source = VesperPlayerSource(
            uri: try XCTUnwrap(playerSourcePayload["uri"] as? String),
            label: try XCTUnwrap(playerSourcePayload["label"] as? String),
            kind: VesperPlayerSourceKind(rawValue: try XCTUnwrap(playerSourcePayload["kind"] as? String)) ?? .remote,
            protocol: VesperPlayerSourceProtocol(rawValue: try XCTUnwrap(playerSourcePayload["protocol"] as? String)) ?? .unknown
        )
        let task = VesperDownloadTaskSnapshot(
            taskId: UInt64(try XCTUnwrap(payload["taskId"] as? Int)),
            assetId: try XCTUnwrap(payload["assetId"] as? String),
            source: VesperDownloadSource(
                source: source,
                contentFormat: VesperDownloadContentFormat.wireName(
                    try XCTUnwrap(sourcePayload["contentFormat"] as? String)
                )
            ),
            profile: VesperDownloadProfile(
                selectedTrackIds: try XCTUnwrap(profilePayload["selectedTrackIds"] as? [String]),
                targetOutputFormat: VesperDownloadOutputFormat.wireName(
                    try XCTUnwrap(profilePayload["targetOutputFormat"] as? String)
                ),
                targetDirectory: URL(fileURLWithPath: try XCTUnwrap(profilePayload["targetDirectory"] as? String))
            ),
            state: VesperDownloadState.wireName(try XCTUnwrap(payload["state"] as? String)),
            progress: VesperDownloadProgressSnapshot(
                receivedBytes: UInt64(try XCTUnwrap(progressPayload["receivedBytes"] as? Int)),
                totalBytes: UInt64(try XCTUnwrap(progressPayload["totalBytes"] as? Int)),
                receivedSegments: UInt32(try XCTUnwrap(progressPayload["receivedSegments"] as? Int)),
                totalSegments: UInt32(try XCTUnwrap(progressPayload["totalSegments"] as? Int))
            ),
            assetIndex: VesperDownloadAssetIndex(
                contentFormat: VesperDownloadContentFormat.wireName(
                    try XCTUnwrap(assetIndexPayload["contentFormat"] as? String)
                ),
                totalSizeBytes: UInt64(try XCTUnwrap(assetIndexPayload["totalSizeBytes"] as? Int))
            )
        )

        XCTAssertEqual(task.taskId, 42)
        XCTAssertEqual(task.assetId, "asset-contract")
        XCTAssertEqual(task.source.source.protocol, .dash)
        XCTAssertEqual(task.source.contentFormat, .dashSegments)
        XCTAssertEqual(task.profile.targetOutputFormat, .mp4)
        XCTAssertEqual(task.profile.selectedTrackIds, ["video:1080p", "audio:ja"])
        XCTAssertEqual(task.state, .downloading)
        XCTAssertEqual(task.progress.receivedBytes, 2048)
        XCTAssertEqual(task.assetIndex.totalSizeBytes, 4096)
        XCTAssertNil(task.error)
    }

    func testSharedSystemPlaybackContractKeepsStableFields() throws {
        let payload = try contractMap("system_playback_configuration")
        let metadataPayload = try XCTUnwrap(payload["metadata"] as? [String: Any])
        let controlsPayload = try XCTUnwrap(payload["controls"] as? [String: Any])
        let buttonPayloads = try XCTUnwrap(controlsPayload["compactButtons"] as? [[String: Any]])

        let configuration = VesperSystemPlaybackConfiguration(
            enabled: try XCTUnwrap(payload["enabled"] as? Bool),
            backgroundMode: VesperBackgroundPlaybackMode(rawValue: try XCTUnwrap(payload["backgroundMode"] as? String)) ?? .disabled,
            showSystemControls: try XCTUnwrap(payload["showSystemControls"] as? Bool),
            showSeekActions: try XCTUnwrap(payload["showSeekActions"] as? Bool),
            metadata: VesperSystemPlaybackMetadata(
                title: try XCTUnwrap(metadataPayload["title"] as? String),
                artist: metadataPayload["artist"] as? String,
                albumTitle: metadataPayload["albumTitle"] as? String,
                artworkUri: metadataPayload["artworkUri"] as? String,
                contentUri: metadataPayload["contentUri"] as? String,
                durationMs: Int64(try XCTUnwrap(metadataPayload["durationMs"] as? Int)),
                isLive: try XCTUnwrap(metadataPayload["isLive"] as? Bool)
            ),
            controls: VesperSystemPlaybackControls(
                compactButtons: buttonPayloads.map { payload in
                    VesperSystemPlaybackControlButton(
                        kind: VesperSystemPlaybackControlKind(rawValue: payload["kind"] as? String ?? "") ?? .playPause,
                        seekOffsetMs: (payload["seekOffsetMs"] as? Int).map(Int64.init)
                    )
                }
            )
        )

        XCTAssertTrue(configuration.enabled)
        XCTAssertEqual(configuration.backgroundMode, .continueAudio)
        XCTAssertEqual(configuration.metadata?.title, "Contract Episode")
        XCTAssertEqual(configuration.metadata?.isLive, true)
        XCTAssertEqual(configuration.controls.compactButtons.map(\.kind), [.seekBack, .playPause, .seekForward])
        XCTAssertEqual(configuration.controls.seekOffsetMs(for: .seekBack), 10_000)
    }

    func testSharedPluginDiagnosticsContractKeepsStableFields() throws {
        let payload = try contractArray("plugin_diagnostics")

        XCTAssertEqual(payload.count, 3)
        let decoder = try XCTUnwrap(payload.first)
        XCTAssertEqual(decoder["status"] as? String, "decoderSupported")
        XCTAssertEqual(decoder["participation"] as? String, "participated")
        let decoderCapability = try XCTUnwrap(decoder["capability"] as? [String: Any])
        XCTAssertEqual(decoderCapability["kind"] as? String, "decoder")
        let decoderSummary = try XCTUnwrap(decoderCapability["decoder"] as? [String: Any])
        let codecs = try XCTUnwrap(decoderSummary["codecs"] as? [[String: Any]])
        XCTAssertEqual(codecs.first?["codec"] as? String, "h264")

        let processor = try XCTUnwrap(payload.dropFirst().first)
        XCTAssertEqual(processor["status"] as? String, "frameProcessorSupported")
        XCTAssertEqual(processor["participation"] as? String, "available")
        let processorCapability = try XCTUnwrap(processor["capability"] as? [String: Any])
        XCTAssertEqual(processorCapability["kind"] as? String, "frameProcessor")
        let processorSummary = try XCTUnwrap(processorCapability["frameProcessor"] as? [String: Any])
        XCTAssertEqual(processorSummary["maxInFlightFrames"] as? Int, 4)

        let sourceNormalizer = try XCTUnwrap(payload.dropFirst(2).first)
        XCTAssertEqual(sourceNormalizer["status"] as? String, "sourceNormalizerSupported")
        XCTAssertEqual(sourceNormalizer["participation"] as? String, "bypassed")
        let sourceNormalizerCapability = try XCTUnwrap(sourceNormalizer["capability"] as? [String: Any])
        XCTAssertEqual(sourceNormalizerCapability["kind"] as? String, "sourceNormalizer")
        let sourceNormalizerSummary = try XCTUnwrap(sourceNormalizerCapability["sourceNormalizer"] as? [String: Any])
        XCTAssertEqual(sourceNormalizerSummary["supportedRuntimeProfiles"] as? [String], ["generic-fallback"])
        XCTAssertEqual(sourceNormalizerSummary["supportedOutputRoutes"] as? [String], ["packetStream"])
        XCTAssertEqual(sourceNormalizerSummary["requiresNetwork"] as? Bool, false)
    }
}

private func contractMap(_ name: String) throws -> [String: Any] {
    let data = try Data(contentsOf: contractFixtureUrl(name))
    return try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
}

private func contractArray(_ name: String) throws -> [[String: Any]] {
    let data = try Data(contentsOf: contractFixtureUrl(name))
    return try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [[String: Any]])
}

private func contractFixtureUrl(_ name: String) -> URL {
    let url = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .appendingPathComponent("../../../../../fixtures/contracts/\(name).json")
        .standardizedFileURL
    return url
}

private extension VesperDownloadContentFormat {
    static func wireName(_ value: String) -> Self {
        switch value {
        case "hlsSegments": return .hlsSegments
        case "dashSegments": return .dashSegments
        case "flvSegments": return .flvSegments
        case "singleFile": return .singleFile
        default: return .unknown
        }
    }
}

private extension VesperDownloadOutputFormat {
    static func wireName(_ value: String) -> Self? {
        switch value {
        case "mp4": return .mp4
        case "mkv": return .mkv
        case "original": return .original
        default: return nil
        }
    }
}

private extension VesperDownloadState {
    static func wireName(_ value: String) -> Self {
        switch value {
        case "queued": return .queued
        case "preparing": return .preparing
        case "downloading": return .downloading
        case "paused": return .paused
        case "completed": return .completed
        case "failed": return .failed
        case "removed": return .removed
        default: return .queued
        }
    }
}
