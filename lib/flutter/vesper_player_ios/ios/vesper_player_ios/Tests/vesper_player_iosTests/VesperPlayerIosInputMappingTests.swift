import Flutter
import Foundation
import VesperPlayerKit
import XCTest
@testable import vesper_player_ios

@MainActor
final class VesperPlayerIosInputMappingTests: XCTestCase {
    private let resource: [String: Any] = ["resourceId": "manifest", "uri": "https://fixture.test/index.mpd"]
    private let segment: [String: Any] = ["segmentId": "segment", "uri": "https://fixture.test/1.m4s"]
    private let stream: [String: Any] = ["streamId": "stream"]

    func testExpectedCatalogRevisionAcceptsExactIntegers() throws {
        for revision: Int64 in [0, 1, 2, 9_007_199_254_740_993, Int64.max - 1, Int64.max] {
            for value: Any in [revision, NSNumber(value: revision)] {
                XCTAssertEqual(try ["expectedCatalogRevision": value].toExpectedCatalogRevision(), revision)
            }
        }
        XCTAssertEqual(
            try ["expectedCatalogRevision": NSNumber(value: UInt64(Int64.max))].toExpectedCatalogRevision(),
            Int64.max
        )
    }

    func testExpectedCatalogRevisionRejectsBooleanFractionalAndOutOfRangeValues() {
        let invalidValues: [Any] = [
            true, false, NSNumber(value: true), NSNumber(value: false), "1", [Any](),
            -1, -0.25, 0.5, Double.leastNonzeroMagnitude, Double.nan,
            Double.infinity, -Double.infinity, NSNumber(value: UInt64(Int64.max) + 1),
            NSNumber(value: UInt64.max), Double(Int64.max), Double(Int64.max).nextUp,
        ]
        for value in invalidValues {
            XCTAssertThrowsError(try ["expectedCatalogRevision": value].toExpectedCatalogRevision(), "\(value)") { error in
                guard let error = error as? PluginError, case let .invalidAbrPolicy(message) = error else {
                    return XCTFail("invalid revisions must retain the ABR input error")
                }
                XCTAssertEqual(message, "expectedCatalogRevision must be a non-negative integer.")
            }
        }
    }

    func testExpectedCatalogRevisionKeepsOptionalAndIntegralFloatingPointBehavior() throws {
        XCTAssertNil(try [String: Any]().toExpectedCatalogRevision())
        XCTAssertNil(try ["expectedCatalogRevision": NSNull()].toExpectedCatalogRevision())
        for value in [0.0, 1.0, 2.0, 9_007_199_254_740_992.0, Double(Int64.max).nextDown] {
            XCTAssertEqual(
                try ["expectedCatalogRevision": NSNumber(value: value)].toExpectedCatalogRevision(),
                Int64(exactly: value)
            )
        }
    }

    func testExpectedCatalogRevisionSurvivesFlutterMethodCodec() throws {
        let codec = FlutterStandardMethodCodec.sharedInstance()
        for revision: Int64 in [0, 1, 2, 9_007_199_254_740_993, Int64.max] {
            let call = FlutterMethodCall(methodName: "setAbrPolicy", arguments: [
                "policy": ["mode": "auto"], "expectedCatalogRevision": NSNumber(value: revision),
            ])
            let decoded = codec.decodeMethodCall(codec.encode(call))
            let arguments = try XCTUnwrap(decoded.arguments as? [String: Any])
            XCTAssertEqual(try arguments.toExpectedCatalogRevision(), revision)
        }
        for value in [true, false] {
            let call = FlutterMethodCall(methodName: "setAbrPolicy", arguments: ["expectedCatalogRevision": value])
            let decoded = codec.decodeMethodCall(codec.encode(call))
            let arguments = try XCTUnwrap(decoded.arguments as? [String: Any])
            XCTAssertThrowsError(try arguments.toExpectedCatalogRevision())
        }
    }

    func testSetAbrPolicyAcceptsCodecIntegerRevisionsAndRejectsBooleans() throws {
        let plugin = VesperPlayerIosPlugin()
        var created: Any?
        plugin.handle(FlutterMethodCall(methodName: "createPlayer", arguments: ["keepScreenOnDuringPlayback": false])) {
            created = $0
        }
        let playerId = try XCTUnwrap((created as? [String: Any])?["playerId"] as? String)
        defer {
            plugin.handle(FlutterMethodCall(methodName: "disposePlayer", arguments: ["playerId": playerId])) { _ in }
        }
        let codec = FlutterStandardMethodCodec.sharedInstance()
        for value: Any in [NSNumber(value: 0), NSNumber(value: 1), NSNumber(value: 2), true, false] {
            let call = FlutterMethodCall(methodName: "setAbrPolicy", arguments: [
                "playerId": playerId, "policy": ["mode": "auto"], "expectedCatalogRevision": value,
            ])
            let decoded = codec.decodeMethodCall(codec.encode(call))
            var replies = 0
            plugin.handle(decoded) { result in
                replies += 1
                if type(of: value) == Bool.self {
                    XCTAssertEqual((result as? FlutterError)?.code, "vesper_operation_failed")
                    XCTAssertEqual(
                        (result as? FlutterError)?.message,
                        "expectedCatalogRevision must be a non-negative integer."
                    )
                } else {
                    XCTAssertNil(result, "integer revisions must reach the native controller: \(value)")
                }
            }
            XCTAssertEqual(replies, 1)
        }
    }

    func testMalformedGraphCollectionsRejectWholePlanWithExistingManifest() {
        for (key, record) in [("resources", resource), ("segments", segment), ("streams", stream)] {
            for bad: Any in [NSNull(), 7, "not a record"] {
                var graph: [String: Any] = ["resources": [resource]]
                graph[key] = [record, bad]
                XCTAssertThrowsError(try graph.toDownloadAssetIndex(), "\(key): \(bad)")
            }
            for bad: Any in [7, "not a list", ["record": record]] {
                XCTAssertThrowsError(try [key: bad].toDownloadAssetIndex())
            }
        }
    }

    func testGraphReferencesAndIdentitiesRejectWrongTypesWithoutShortening() {
        for key in ["resourceIds", "segmentIds"] {
            for bad: Any in [["valid", NSNull()], ["valid", 7], "not a list"] {
                var entry = stream
                entry[key] = bad
                XCTAssertThrowsError(try ["streams": [entry]].toDownloadAssetIndex())
            }
        }
        for bad: Any in [["valid", NSNull()], ["valid", 7], "not a list"] {
            XCTAssertThrowsError(try ["selectedTrackIds": bad].toDownloadProfile())
        }
        XCTAssertThrowsError(try ["resourceId": 7, "uri": "https://fixture.test"].toDownloadResourceRecord())
        XCTAssertThrowsError(try ["segmentId": 7, "uri": "https://fixture.test"].toDownloadSegmentRecord())
        XCTAssertThrowsError(try ["streamId": 7].toDownloadAssetStream())
        XCTAssertThrowsError(try ["streamId": "s", "metadata": ["key": 7]].toDownloadAssetStream())
        XCTAssertThrowsError(try ["streamId": "s", "metadata": [7: "value"]].toDownloadAssetStream())
        XCTAssertThrowsError(try ["streamId": "s", "kind": "future"].toDownloadAssetStream())
        XCTAssertThrowsError(try ["targetOutputFormat": "future"].toDownloadProfile())
    }

    func testAwkwardValidIdsAndOrderingRemainUnchanged() throws {
        let ids = [" 中文 / audio:#?% ", "001", "e\u{301}", "é"]
        let graph: [String: Any] = [
            "resources": ids.map { ["resourceId": $0, "uri": "https://fixture.test"] },
            "segments": ids.map { ["segmentId": $0, "uri": "https://fixture.test"] },
            "streams": [["streamId": ids[0], "resourceIds": ids, "segmentIds": ids]],
        ]
        let decoded = try graph.toDownloadAssetIndex()
        XCTAssertEqual(decoded.resources.map(\.resourceId), ids)
        XCTAssertEqual(decoded.segments.map(\.segmentId), ids)
        XCTAssertEqual(decoded.streams.first?.streamId, ids[0])
        XCTAssertEqual(decoded.streams.first?.resourceIds, ids)
        XCTAssertEqual(decoded.streams.first?.segmentIds, ids)
        XCTAssertEqual(try ["selectedTrackIds": ids].toDownloadProfile().selectedTrackIds, ids)
        XCTAssertEqual(try ["mode": "track", "trackId": ids[0]].toTrackSelection().trackId, ids[0])
        XCTAssertEqual(try ["mode": "fixedTrack", "trackId": ids[0]].toAbrPolicy().trackId, ids[0])
    }

    func testOptionalListsKeepAbsentNullAndEmptyDefaults() throws {
        for map: [String: Any] in [[:], ["resources": NSNull(), "segments": NSNull(), "streams": NSNull()],
            ["resources": [], "segments": [], "streams": []]] {
            let decoded = try map.toDownloadAssetIndex()
            XCTAssertTrue(decoded.resources.isEmpty)
            XCTAssertTrue(decoded.segments.isEmpty)
            XCTAssertTrue(decoded.streams.isEmpty)
        }
        for value: Any in [NSNull(), [String]()] {
            let decoded = try ["streamId": "s", "resourceIds": value, "segmentIds": value].toDownloadAssetStream()
            XCTAssertTrue(decoded.resourceIds.isEmpty)
            XCTAssertTrue(decoded.segmentIds.isEmpty)
            XCTAssertTrue(try ["selectedTrackIds": value].toDownloadProfile().selectedTrackIds.isEmpty)
            XCTAssertTrue(try toVesperPluginReferences(value).isEmpty)
        }
    }

    func testClosedTokensRejectUnknownStringsAndWrongTypes() {
        for bad: Any in ["future-token", 7, true, [String](), [String: Any]()] {
            let modes: [String: Any] = ["mode": bad]
            let cases: [() throws -> Any] = [
                { try modes.toTrackSelection() }, { try modes.toAbrPolicy() },
                { try modes.toSourceNormalizerConfiguration() }, { try modes.toFrameProcessorConfiguration() },
                { try modes.toNativeFramePipelineConfiguration() },
                { try ["backgroundMode": bad].toSystemPlaybackConfiguration() },
                { try ["kind": bad].toSystemPlaybackControlButton() },
                { try ["preset": bad].toBufferingPolicy() }, { try ["preset": bad].toCachePolicy() },
                { try ["backoff": bad].toRetryPolicy() },
            ]
            for decode in cases { XCTAssertThrowsError(try decode(), "\(bad)") }
        }
    }

    func testOptionalModesDefaultsAndRequiredModesStayValid() throws {
        for map: [String: Any] in [[:], ["mode": NSNull()], ["mode": "auto"]] {
            XCTAssertEqual(try map.toTrackSelection().mode, .auto)
            XCTAssertEqual(try map.toAbrPolicy().mode, .auto)
        }
        for map: [String: Any] in [[:], ["mode": NSNull()], ["mode": "disabled"]] {
            XCTAssertEqual(try map.toSourceNormalizerConfiguration().mode, .disabled)
            XCTAssertEqual(try map.toFrameProcessorConfiguration().mode, .disabled)
            XCTAssertEqual(try map.toNativeFramePipelineConfiguration().mode, .disabled)
        }
        XCTAssertEqual(try ["mode": "requireNormalized"].toSourceNormalizerConfiguration().mode, .requireNormalized)
        XCTAssertEqual(try ["mode": "requireNativeFrame"].toNativeFramePipelineConfiguration().mode, .requireNativeFrame)
        for map: [String: Any] in [[:], ["preset": NSNull()], ["preset": "default"], ["preset": "defaultPreset"]] {
            XCTAssertEqual(try map.toBufferingPolicy().preset, .default)
            XCTAssertEqual(try map.toCachePolicy().preset, .default)
        }
        for map: [String: Any] in [[:], ["backoff": NSNull()]] {
            XCTAssertEqual(try map.toRetryPolicy().backoff, .linear)
        }
        for map: [String: Any] in [[:], ["backgroundMode": NSNull()], ["backgroundMode": "continueAudio"]] {
            XCTAssertEqual(try map.toSystemPlaybackConfiguration().backgroundMode, .continueAudio)
        }
    }

    func testByteRangeAndRetryCountsRejectBooleanFractionalAndWrongValues() throws {
        for map: [String: Any] in [[:], ["offset": NSNull(), "length": NSNull()]] {
            let range = try map.toDownloadByteRange()
            XCTAssertEqual(range.offset, 0)
            XCTAssertEqual(range.length, 0)
        }
        for key in ["offset", "length"] {
            for bad: Any in [true, false, NSNumber(value: true), "7", 1.0, Float(1), 1.25, -1,
                Double.infinity, Double.nan, [Any]()] {
                XCTAssertThrowsError(try [key: bad].toDownloadByteRange(), "\(key): \(bad)")
            }
        }
        XCTAssertEqual(try ["offset": NSNumber(value: UInt64.max)].toDownloadByteRange().offset, UInt64.max)
        XCTAssertEqual(try ["length": NSNumber(value: 1), "offset": 2].toDownloadByteRange().length, 1)
        XCTAssertEqual(try ["length": NSNumber(value: 1), "offset": 2].toDownloadByteRange().offset, 2)
        XCTAssertEqual(try [String: Any]().toRetryPolicy().maxAttempts, 3)
        XCTAssertNil(try ["maxAttempts": NSNull()].toRetryPolicy().maxAttempts)
        for attempts in [0, -1, 6, Int.max] {
            XCTAssertEqual(try ["maxAttempts": NSNumber(value: attempts)].toRetryPolicy().maxAttempts, attempts)
        }
        for bad: Any in [true, false, NSNumber(value: true), "6", 1.0, Float(1), 1.25, [Any]()] {
            XCTAssertThrowsError(try ["maxAttempts": bad].toRetryPolicy())
        }
    }

    func testSupportedClosedTokensRemainAccepted() throws {
        for token in ["balanced", "streaming", "resilient", "lowLatency"] {
            _ = try ["preset": token].toBufferingPolicy()
        }
        for token in ["disabled", "streaming", "resilient"] {
            _ = try ["preset": token].toCachePolicy()
        }
        for token in ["fixed", "linear", "exponential"] {
            _ = try ["backoff": token].toRetryPolicy()
        }
        for token in ["disabled", "continueAudio"] {
            _ = try ["backgroundMode": token].toSystemPlaybackConfiguration()
        }
        for token in ["playPause", "seekBack", "seekForward"] {
            _ = try ["kind": token].toSystemPlaybackControlButton()
        }
        for token in ["diagnosticsOnly", "preflightOnly", "preferNormalized", "requireNormalized"] {
            _ = try ["mode": token].toSourceNormalizerConfiguration()
        }
        _ = try ["mode": "diagnosticsOnly"].toFrameProcessorConfiguration()
        for token in ["diagnosticsOnly", "preferNativeFrame", "requireNativeFrame"] {
            _ = try ["mode": token].toNativeFramePipelineConfiguration()
        }
        _ = try ["mode": "constrained"].toAbrPolicy()
        _ = try ["mode": "disabled"].toTrackSelection()
    }

    func testSystemControlsRejectEntireMalformedCollectionAndKeepValidOrder() throws {
        for bad: Any in [NSNull(), 7, "not a button"] {
            XCTAssertThrowsError(try ["compactButtons": [["kind": "playPause"], bad]].toSystemPlaybackControls())
        }
        for bad: Any in [7, "not a list", ["kind": "playPause"]] {
            XCTAssertThrowsError(try ["compactButtons": bad].toSystemPlaybackControls())
        }
        for map: [String: Any] in [[:], ["compactButtons": NSNull()], ["compactButtons": []]] {
            XCTAssertTrue(try map.toSystemPlaybackControls().compactButtons.isEmpty)
        }
        let controls = try ["compactButtons": [["kind": "seekForward", "seekOffsetMs": 12_345],
            ["kind": "playPause"], ["kind": "seekBack", "seekOffsetMs": 2_345]]].toSystemPlaybackControls()
        XCTAssertEqual(controls.compactButtons.map(\.kind), [.seekForward, .playPause, .seekBack])
        XCTAssertEqual(controls.compactButtons[0].seekOffsetMs, 12_345)
        XCTAssertThrowsError(try ["controls": "not a map"].toSystemPlaybackConfiguration())
    }

    func testPluginReferencesRejectMalformedOptionalIdentityAndKeepUnknownTransport() throws {
        let reference: [String: Any] = ["pluginId": "io.github.example.plugin-with-dash", "transport": "future-sandbox"]
        for bad: Any in [7, true, [Any]()] {
            var entry = reference
            entry["capabilityInstanceId"] = bad
            XCTAssertThrowsError(try toVesperPluginReferences([entry]))
        }
        let decoded = try XCTUnwrap(toVesperPluginReferences([reference]).first)
        XCTAssertEqual(decoded.pluginId, reference["pluginId"] as? String)
        XCTAssertEqual(decoded.transport.rawValue, "future-sandbox")
    }

    func testSubtitleInputErrorsKeepCanonicalDetails() {
        for bad: Any in ["future", 7, NSNull()] {
            XCTAssertThrowsError(try ["mode": bad].toTrackSelection(isSubtitle: true)) { error in
                let subtitle = error as? PluginSubtitleError
                XCTAssertEqual(subtitle?.code, "subtitle_selection_invalid")
                XCTAssertEqual(subtitle?.phase, "selection")
                XCTAssertEqual(subtitle?.retriable, false)
            }
        }
    }

    func testInvalidCreationReturnsExistingChannelErrorWithoutRegisteringSession() {
        for key in ["sourceNormalizer", "frameProcessor", "nativeFramePipeline", "trackPreferencePolicy", "resiliencePolicy"] {
            for bad: Any in ["not a map", ["mode": "future"]] {
                let value: Any = if key == "trackPreferencePolicy", bad is [String: Any] {
                    ["audioSelection": bad]
                } else if key == "resiliencePolicy", bad is [String: Any] {
                    ["retry": ["maxAttempts": true]]
                } else { bad }
                let plugin = VesperPlayerIosPlugin()
                var calls = 0
                plugin.handle(FlutterMethodCall(methodName: "createPlayer", arguments: [key: value])) { value in
                    calls += 1
                    let error = value as? FlutterError
                    XCTAssertEqual(error?.code, "vesper_create_failed")
                    XCTAssertNotNil(error?.message)
                }
                XCTAssertEqual(calls, 1)
                XCTAssertTrue(plugin.sessions.isEmpty)
            }
        }
    }

    func testProbeRejectsMalformedConfigurationInsteadOfDefaulting() {
        let plugin = VesperPlayerIosPlugin()
        for key in ["sourceNormalizer", "frameProcessor", "nativeFramePipeline"] {
            var calls = 0
            plugin.handle(FlutterMethodCall(methodName: "probePlaybackCapability", arguments: [key: ["mode": "future"]])) { value in
                calls += 1
                XCTAssertEqual((value as? FlutterError)?.code, "invalid_probe_request")
            }
            XCTAssertEqual(calls, 1)
        }
    }
}
