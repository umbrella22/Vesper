@preconcurrency import AVFoundation
import XCTest
@testable import VesperPlayerKit

/// Contract tests for the iOS subtitle state model and wire shape.
///
/// These tests cover the behavior that does not require a live
/// `AVPlayerItem` / legible media selection group. The end-to-end
/// `loadTrackCatalogState` AV legible-group behavior is exercised by the
/// existing `VesperDashBridgeHlsBuilderTests` and depends on a real
/// `vesper-dash://` AVAsset; the cases here isolate the model helpers and
/// the Flutter wire mapping so regressions surface without a simulator
/// device boot.
final class VesperNativeSubtitleStateTests: XCTestCase {
    func testHlsSubtitleInspectorPreservesAdvertisedMetadataAndStableIdentity() throws {
        let first = try VesperHlsSubtitleManifestInspector.parse(sampleHlsSubtitleMaster)
        let reordered = try VesperHlsSubtitleManifestInspector.parse(
            sampleHlsSubtitleMasterReordered
        )
        let rotatedUris = try VesperHlsSubtitleManifestInspector.parse(
            sampleHlsSubtitleMaster
                .replacingOccurrences(
                    of: "sub-en.m3u8",
                    with: "sub-en.m3u8?token=rotated"
                )
                .replacingOccurrences(
                    of: "sub-en-forced.m3u8",
                    with: "sub-en-forced.m3u8?token=rotated"
                )
        )
        let refreshedMetadata = try VesperHlsSubtitleManifestInspector.parse(
            sampleHlsSubtitleMaster
                .replacingOccurrences(of: "LANGUAGE=\"en\"", with: "LANGUAGE=\"en-US\"")
                .replacingOccurrences(of: "FORCED=YES", with: "FORCED=NO")
        )

        XCTAssertTrue(first.isMasterPlaylist)
        XCTAssertEqual(first.advertisedTrackCount, 2)
        XCTAssertEqual(first.renditions[0].name, "English")
        XCTAssertEqual(first.renditions[0].language, "en")
        XCTAssertTrue(first.renditions[0].isDefault)
        XCTAssertTrue(first.renditions[1].isForced)
        XCTAssertEqual(
            Set(first.renditions.map(\.id)),
            Set(reordered.renditions.map(\.id))
        )
        XCTAssertEqual(
            Dictionary(uniqueKeysWithValues: first.renditions.map { ($0.name, $0.id) }),
            Dictionary(uniqueKeysWithValues: rotatedUris.renditions.map { ($0.name, $0.id) })
        )
        XCTAssertEqual(
            Dictionary(uniqueKeysWithValues: first.renditions.map { ($0.name, $0.id) }),
            Dictionary(uniqueKeysWithValues: refreshedMetadata.renditions.map { ($0.name, $0.id) })
        )
    }

    func testHlsSubtitleInspectorKeepsDefaultsScopedToEachGroup() throws {
        let snapshot = try VesperHlsSubtitleManifestInspector.parse(
            #"""
            #EXTM3U
            #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs-main",NAME="English",LANGUAGE="en",DEFAULT=YES,FORCED=NO,URI="main-en.m3u8"
            #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs-commentary",NAME="English",LANGUAGE="en",DEFAULT=YES,FORCED=NO,URI="commentary-en.m3u8"
            #EXT-X-STREAM-INF:BANDWIDTH=800000,SUBTITLES="subs-main"
            video.m3u8
            """#
        )

        XCTAssertEqual(snapshot.advertisedTrackCount, 2)
        XCTAssertEqual(Set(snapshot.renditions.map(\.groupId)), ["subs-main", "subs-commentary"])
        XCTAssertEqual(Set(snapshot.renditions.map(\.id)).count, 2)
    }

    @MainActor
    func testHlsDescriptorResolverIgnoresUnselectedGroupWithOverlappingMetadata() {
        let english = VesperNativePlayerBridge.VesperHlsSubtitleMatchKey(
            language: "en",
            label: "english",
            isForced: false
        )
        let chinese = VesperNativePlayerBridge.VesperHlsSubtitleMatchKey(
            language: "zh",
            label: "chinese",
            isForced: false
        )
        let descriptors = [
            VesperNativePlayerBridge.VesperHlsSubtitleDescriptorIdentity(
                id: "main-en",
                groupId: "main",
                key: english
            ),
            VesperNativePlayerBridge.VesperHlsSubtitleDescriptorIdentity(
                id: "main-zh",
                groupId: "main",
                key: chinese
            ),
            VesperNativePlayerBridge.VesperHlsSubtitleDescriptorIdentity(
                id: "commentary-en",
                groupId: "commentary",
                key: english
            ),
        ]
        let bridge = VesperNativePlayerBridge()

        let result = bridge.resolveHlsSubtitleDescriptorGroup(
            optionKeys: [english, chinese],
            descriptors: descriptors
        )

        XCTAssertEqual(result, .unique(["main-en", "main-zh"]))
    }

    @MainActor
    func testHlsDescriptorResolverRejectsTiedGroupsAsAmbiguous() {
        let english = VesperNativePlayerBridge.VesperHlsSubtitleMatchKey(
            language: "en",
            label: "english",
            isForced: false
        )
        let descriptors = [
            VesperNativePlayerBridge.VesperHlsSubtitleDescriptorIdentity(
                id: "main-en",
                groupId: "main",
                key: english
            ),
            VesperNativePlayerBridge.VesperHlsSubtitleDescriptorIdentity(
                id: "commentary-en",
                groupId: "commentary",
                key: english
            ),
        ]
        let bridge = VesperNativePlayerBridge()

        let result = bridge.resolveHlsSubtitleDescriptorGroup(
            optionKeys: [english],
            descriptors: descriptors
        )

        XCTAssertEqual(result, .ambiguous)
    }

    func testHlsSubtitleInspectorRejectsDuplicateDefaultWithinGroup() {
        XCTAssertThrowsError(
            try VesperHlsSubtitleManifestInspector.parse(
                sampleHlsSubtitleMaster.replacingOccurrences(
                    of: "DEFAULT=NO,AUTOSELECT=YES,FORCED=YES",
                    with: "DEFAULT=YES,AUTOSELECT=YES,FORCED=YES"
                )
            )
        ) { error in
            guard case VesperHlsSubtitleManifestError.duplicateDefault = error else {
                return XCTFail("unexpected error: \(error)")
            }
        }
    }

    func testHlsSubtitleInspectorDoesNotInferAdvertisedCountFromMediaPlaylist() throws {
        let snapshot = try VesperHlsSubtitleManifestInspector.parse(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\nsegment.ts\n"
        )
        XCTAssertFalse(snapshot.isMasterPlaylist)
        XCTAssertEqual(snapshot.advertisedTrackCount, 0)
        XCTAssertTrue(snapshot.renditions.isEmpty)
    }

    func testSourceConvenienceFactoriesPreserveExternalSubtitles() throws {
        let subtitle = VesperExternalSubtitleSource(
            id: "external-en",
            uri: "https://example.com/subtitle.vtt",
            mimeType: VesperExternalSubtitleSource.mimeWebVtt
        )
        let expected = [subtitle]
        let localURL = try XCTUnwrap(URL(string: "file:///tmp/video.mp4"))
        let remoteURL = try XCTUnwrap(URL(string: "https://example.com/video.mp4"))
        let hlsURL = try XCTUnwrap(URL(string: "https://example.com/master.m3u8"))
        let dashURL = try XCTUnwrap(URL(string: "https://example.com/manifest.mpd"))
        let sources = [
            VesperPlayerSource.localFile(url: localURL, externalSubtitles: expected),
            VesperPlayerSource.remoteUrl(remoteURL, externalSubtitles: expected),
            VesperPlayerSource.hls(url: hlsURL, externalSubtitles: expected),
            VesperPlayerSource.dash(url: dashURL, externalSubtitles: expected),
        ]

        for source in sources {
            XCTAssertEqual(source.externalSubtitles, expected)
        }
    }

    @MainActor
    func testResilienceRestoreWaitsForExternalSubtitlePreparation() async {
        let externalTrack = VesperExternalSubtitleSource(
            id: "external-en",
            uri: "file:///tmp/external-en.vtt",
            mimeType: VesperExternalSubtitleSource.mimeWebVtt,
            language: "en",
            label: "English"
        )
        let source = VesperPlayerSource(
            uri: "file:///tmp/video.mp4",
            label: "Video",
            kind: .local,
            protocol: .file,
            externalSubtitles: [externalTrack]
        )
        let bridge = VesperNativePlayerBridge(initialSource: source)
        bridge.subtitleOverlayLoadTask = Task { @MainActor in }
        defer {
            bridge.subtitleOverlayLoadTask?.cancel()
            bridge.subtitleOverlayLoadTask = nil
        }
        let preserved = PreservedPlaybackState(
            positionMs: 0,
            restorePosition: false,
            seekToLiveEdge: false,
            playbackRate: 1,
            playbackState: .ready,
            shouldResumePlayback: false,
            audioSelection: .auto(),
            subtitleSelection: .track(externalTrack.id),
            abrPolicy: .auto()
        )

        let stillPending = await bridge.restoreTrackSelectionsIfNeeded(
            preserved,
            item: AVPlayerItem(url: URL(fileURLWithPath: "/tmp/video.mp4"))
        )

        XCTAssertTrue(stillPending)
        XCTAssertNil(bridge.publishedSubtitleState.selectionError)
    }

    @MainActor
    func testExplicitExternalSubtitleSelectionWaitsForLoadingCatalog() async throws {
        let subtitleURL =
            FileManager.default.temporaryDirectory
                .appendingPathComponent("vesper-subtitle-\(UUID().uuidString).vtt")
        try "WEBVTT\n\n00:00.000 --> 00:01.000\nHello\n".write(
            to: subtitleURL,
            atomically: true,
            encoding: .utf8
        )
        defer { try? FileManager.default.removeItem(at: subtitleURL) }

        let externalTrack = VesperExternalSubtitleSource(
            id: "external-en",
            uri: subtitleURL.absoluteString,
            mimeType: VesperExternalSubtitleSource.mimeWebVtt,
            language: "en",
            label: "English",
            isDefault: true
        )
        let source = VesperPlayerSource(
            uri: "file:///tmp/video.mp4",
            label: "Video",
            kind: .local,
            protocol: .file,
            externalSubtitles: [externalTrack]
        )
        let bridge = VesperNativePlayerBridge(initialSource: source)
        let item = AVPlayerItem(url: URL(fileURLWithPath: "/tmp/video.mp4"))
        bridge.player = AVPlayer(playerItem: item)
        bridge.publishedSubtitleState = .loading(advertisedTrackCount: 1)

        let selectionTask = Task { @MainActor in
            try await bridge.setSubtitleTrackSelection(.track(externalTrack.id))
        }
        defer { selectionTask.cancel() }
        await Task.yield()
        await Task.yield()

        XCTAssertEqual(bridge.publishedSubtitleState.selectionState, .applying)

        let prepared = try await bridge.subtitleOverlayRenderer.prepare([externalTrack])
        bridge.subtitleOverlayRenderer.install(prepared)
        bridge.publishedTrackCatalog = VesperTrackCatalog(
            tracks: [
                VesperMediaTrack(
                    id: externalTrack.id,
                    kind: .subtitle,
                    language: externalTrack.language,
                    isDefault: true
                ),
            ]
        )
        bridge.publishedSubtitleState = .ready(advertisedTrackCount: 1, selectableTrackCount: 1)

        try await selectionTask.value

        XCTAssertEqual(bridge.publishedRequestedSubtitleSelection, .track(externalTrack.id))
        XCTAssertEqual(bridge.publishedConfirmedSubtitleSelection, .track(externalTrack.id))
        XCTAssertEqual(bridge.publishedEffectiveSubtitleTrackId, externalTrack.id)
        XCTAssertEqual(bridge.publishedSubtitleState.selectionState, .confirmed)
    }

    @MainActor
    func testSubtitleSelectionTimeoutDoesNotCommitLateCatalogResult() async throws {
        let subtitleURL = try writeSubtitleFixture(text: "Late subtitle")
        defer { try? FileManager.default.removeItem(at: subtitleURL) }
        let track = externalSubtitle(id: "timeout-track", url: subtitleURL)
        let (bridge, _) = makeLoadingSubtitleBridge(
            tracks: [track],
            waitPolicy: VesperSubtitleSelectionWaitPolicy(
                timeout: .milliseconds(30),
                pollInterval: .milliseconds(1)
            )
        )

        let selectionTask = Task { @MainActor in
            try await bridge.setSubtitleTrackSelection(.track(track.id))
        }
        let error = try await subtitleCommandError(from: selectionTask)

        XCTAssertEqual(error.code, "subtitle_selection_timeout")
        XCTAssertEqual(error.commandId, 1)
        XCTAssertEqual(error.sourceEpoch, 0)
        XCTAssertEqual(bridge.publishedSubtitleState.selectionState, .failed)

        let prepared = try await bridge.subtitleOverlayRenderer.prepare([track])
        bridge.subtitleOverlayRenderer.install(prepared)
        bridge.publishedTrackCatalog = subtitleCatalog(tracks: [track])
        bridge.publishedSubtitleState = .ready(
            advertisedTrackCount: 1,
            selectableTrackCount: 1
        )
        try await ContinuousClock().sleep(for: .milliseconds(10))

        XCTAssertEqual(bridge.publishedConfirmedSubtitleSelection, .disabled())
        XCTAssertNil(bridge.publishedEffectiveSubtitleTrackId)
        XCTAssertNil(bridge.pendingSubtitleSelection)
    }

    @MainActor
    func testSubtitleSelectionSourceChangeDoesNotOverwriteNewSourceState() async throws {
        let oldTrack = externalSubtitle(
            id: "old-track",
            url: URL(fileURLWithPath: "/tmp/old-track.vtt")
        )
        let (bridge, _) = makeLoadingSubtitleBridge(
            tracks: [oldTrack],
            waitPolicy: VesperSubtitleSelectionWaitPolicy(
                timeout: .milliseconds(500),
                pollInterval: .milliseconds(1)
            )
        )
        let selectionTask = Task { @MainActor in
            try await bridge.setSubtitleTrackSelection(.track(oldTrack.id))
        }
        try await waitForPendingSubtitleSelection(bridge)

        let newTrackId = "new-source-track"
        let newSelection = VesperTrackSelection.track(newTrackId)
        let newSource = VesperPlayerSource.localFile(
            url: URL(fileURLWithPath: "/tmp/new-source.mp4"),
            label: "New Source"
        )
        bridge.advanceSubtitleSourceEpoch()
        bridge.currentSource = newSource
        bridge.player = AVPlayer(
            playerItem: AVPlayerItem(url: URL(fileURLWithPath: "/tmp/new-source.mp4"))
        )
        bridge.publishedRequestedSubtitleSelection = newSelection
        bridge.publishedConfirmedSubtitleSelection = newSelection
        bridge.publishedEffectiveSubtitleTrackId = newTrackId
        bridge.publishedTrackSelection = VesperTrackSelectionSnapshot(
            subtitle: newSelection,
            confirmedSubtitle: newSelection,
            effectiveSubtitleTrackId: newTrackId
        )
        bridge.publishedSubtitleState = VesperSubtitleState(
            catalogState: .ready,
            selectionState: .confirmed,
            advertisedTrackCount: 1,
            selectableTrackCount: 1,
            catalogError: nil,
            selectionError: nil
        )

        let error = try await subtitleCommandError(from: selectionTask)

        XCTAssertEqual(error.code, "subtitle_source_changed")
        XCTAssertEqual(error.commandId, 1)
        XCTAssertEqual(error.sourceEpoch, 0)
        XCTAssertEqual(bridge.publishedConfirmedSubtitleSelection, newSelection)
        XCTAssertEqual(bridge.publishedEffectiveSubtitleTrackId, newTrackId)
        XCTAssertEqual(bridge.publishedSubtitleState.selectionState, .confirmed)
        XCTAssertNil(bridge.publishedSubtitleState.selectionError)
        XCTAssertNil(bridge.publishedLastError)
    }

    @MainActor
    func testSubtitleSelectionDisposeCancelsWithoutPublishingFailure() async throws {
        let track = externalSubtitle(
            id: "dispose-track",
            url: URL(fileURLWithPath: "/tmp/dispose-track.vtt")
        )
        let (bridge, _) = makeLoadingSubtitleBridge(
            tracks: [track],
            waitPolicy: VesperSubtitleSelectionWaitPolicy(
                timeout: .milliseconds(500),
                pollInterval: .milliseconds(1)
            )
        )
        let selectionTask = Task { @MainActor in
            try await bridge.setSubtitleTrackSelection(.track(track.id))
        }
        try await waitForPendingSubtitleSelection(bridge)

        bridge.dispose()
        let error = try await subtitleCommandError(from: selectionTask)

        XCTAssertEqual(error.code, "subtitle_selection_cancelled")
        XCTAssertEqual(error.commandId, 1)
        XCTAssertEqual(error.sourceEpoch, 0)
        XCTAssertNil(bridge.currentSource)
        XCTAssertNil(bridge.publishedSubtitleState.selectionError)
        XCTAssertNil(bridge.publishedLastError)
    }

    @MainActor
    func testNewSubtitleSelectionSupersedesPendingCommandAndCommitsLatest() async throws {
        let firstURL = try writeSubtitleFixture(text: "Subtitle A")
        let secondURL = try writeSubtitleFixture(text: "Subtitle B")
        defer {
            try? FileManager.default.removeItem(at: firstURL)
            try? FileManager.default.removeItem(at: secondURL)
        }
        let firstTrack = externalSubtitle(id: "track-a", url: firstURL)
        let secondTrack = externalSubtitle(id: "track-b", url: secondURL)
        let tracks = [firstTrack, secondTrack]
        let (bridge, _) = makeLoadingSubtitleBridge(
            tracks: tracks,
            waitPolicy: VesperSubtitleSelectionWaitPolicy(
                timeout: .milliseconds(500),
                pollInterval: .milliseconds(1)
            )
        )
        let prepared = try await bridge.subtitleOverlayRenderer.prepare(tracks)
        bridge.subtitleOverlayRenderer.install(prepared)
        let firstTask = Task { @MainActor in
            try await bridge.setSubtitleTrackSelection(.track(firstTrack.id))
        }
        try await waitForPendingSubtitleSelection(bridge)

        let secondTask = Task { @MainActor in
            try await bridge.setSubtitleTrackSelection(.track(secondTrack.id))
        }
        try await waitForPendingSubtitleSelection(bridge, expectedCommandId: 2)
        bridge.publishedTrackCatalog = subtitleCatalog(tracks: tracks)
        bridge.publishedSubtitleState = .ready(
            advertisedTrackCount: tracks.count,
            selectableTrackCount: tracks.count
        )
        try await secondTask.value
        let firstError = try await subtitleCommandError(from: firstTask)

        XCTAssertEqual(firstError.code, "subtitle_selection_superseded")
        XCTAssertEqual(firstError.commandId, 1)
        XCTAssertEqual(firstError.sourceEpoch, 0)
        XCTAssertEqual(bridge.nextSubtitleCommandId, 2)
        XCTAssertEqual(bridge.publishedRequestedSubtitleSelection, .track(secondTrack.id))
        XCTAssertEqual(bridge.publishedConfirmedSubtitleSelection, .track(secondTrack.id))
        XCTAssertEqual(bridge.publishedEffectiveSubtitleTrackId, secondTrack.id)
        XCTAssertEqual(bridge.publishedSubtitleState.selectionState, .confirmed)
        XCTAssertNil(bridge.publishedSubtitleState.selectionError)
        XCTAssertNil(bridge.publishedLastError)
    }

    @MainActor
    func testResilienceItemResetDoesNotPublishTheOldItemAsEffective() {
        let trackId = "stable-subtitle-id"
        let confirmed = VesperTrackSelection.track(trackId)
        let bridge = VesperNativePlayerBridge()
        bridge.publishedConfirmedSubtitleSelection = confirmed
        bridge.publishedEffectiveSubtitleTrackId = trackId
        bridge.publishedTrackSelection = VesperTrackSelectionSnapshot(
            subtitle: confirmed,
            confirmedSubtitle: confirmed,
            effectiveSubtitleTrackId: trackId
        )
        let preserved = PreservedPlaybackState(
            positionMs: 0,
            restorePosition: false,
            seekToLiveEdge: false,
            playbackRate: 1,
            playbackState: .ready,
            shouldResumePlayback: false,
            audioSelection: .auto(),
            subtitleSelection: confirmed,
            abrPolicy: .auto()
        )
        bridge.pendingResilienceRestore = PendingResilienceRestore(
            sourceUri: "file:///tmp/video.mp4",
            state: preserved
        )

        bridge.resetTrackState()

        XCTAssertEqual(bridge.publishedConfirmedSubtitleSelection, confirmed)
        XCTAssertEqual(bridge.publishedTrackSelection.confirmedSubtitle, confirmed)
        XCTAssertNil(bridge.publishedEffectiveSubtitleTrackId)
        XCTAssertNil(bridge.publishedTrackSelection.effectiveSubtitleTrackId)
    }

    func testEmbeddedSubtitleIdentityIsStableAcrossMetadataOrdering() {
        let first = stableEmbeddedSubtitleTrackId(
            language: "zh-Hans",
            label: "Commentary: main / descriptive",
            isForced: false,
            codecValues: [0x77767474, 0x73756274],
            characteristics: ["public.accessibility", "public.main-program-content"]
        )
        let reordered = stableEmbeddedSubtitleTrackId(
            language: "zh-Hans",
            label: "Commentary: main / descriptive",
            isForced: false,
            codecValues: [0x73756274, 0x77767474],
            characteristics: ["public.main-program-content", "public.accessibility"]
        )

        XCTAssertEqual(first, reordered)
        XCTAssertTrue(first.hasPrefix("subtitle:av:"))
    }

    @MainActor
    private func makeLoadingSubtitleBridge(
        tracks: [VesperExternalSubtitleSource],
        waitPolicy: VesperSubtitleSelectionWaitPolicy
    ) -> (VesperNativePlayerBridge, AVPlayerItem) {
        let source = VesperPlayerSource(
            uri: "file:///tmp/subtitle-transaction-video.mp4",
            label: "Subtitle Transaction",
            kind: .local,
            protocol: .file,
            externalSubtitles: tracks
        )
        let bridge = VesperNativePlayerBridge(
            initialSource: source,
            subtitleSelectionWaitPolicy: waitPolicy
        )
        let item = AVPlayerItem(
            url: URL(fileURLWithPath: "/tmp/subtitle-transaction-video.mp4")
        )
        bridge.player = AVPlayer(playerItem: item)
        bridge.publishedSubtitleState = .loading(advertisedTrackCount: tracks.count)
        return (bridge, item)
    }

    @MainActor
    private func waitForPendingSubtitleSelection(
        _ bridge: VesperNativePlayerBridge,
        expectedCommandId: UInt64? = nil
    ) async throws {
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: .milliseconds(250))
        while clock.now < deadline {
            if let pending = bridge.pendingSubtitleSelection,
               expectedCommandId == nil || pending.commandId == expectedCommandId {
                return
            }
            try await clock.sleep(for: .milliseconds(1))
        }
        let pending = try XCTUnwrap(bridge.pendingSubtitleSelection)
        XCTAssertEqual(pending.commandId, expectedCommandId)
    }

    @MainActor
    private func subtitleCommandError(
        from task: Task<Void, Error>
    ) async throws -> VesperSubtitleSelectionCommandError {
        do {
            try await task.value
        } catch let error as VesperSubtitleSelectionCommandError {
            return error
        }
        XCTFail("Expected a subtitle selection command error")
        throw NSError(domain: "VesperNativeSubtitleStateTests", code: 1)
    }

    private func writeSubtitleFixture(text: String) throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("vesper-subtitle-\(UUID().uuidString).vtt")
        try "WEBVTT\n\n00:00.000 --> 00:05.000\n\(text)\n".write(
            to: url,
            atomically: true,
            encoding: .utf8
        )
        return url
    }

    private func externalSubtitle(
        id: String,
        url: URL
    ) -> VesperExternalSubtitleSource {
        VesperExternalSubtitleSource(
            id: id,
            uri: url.absoluteString,
            mimeType: VesperExternalSubtitleSource.mimeWebVtt,
            language: "en",
            label: id
        )
    }

    private func subtitleCatalog(
        tracks: [VesperExternalSubtitleSource]
    ) -> VesperTrackCatalog {
        VesperTrackCatalog(
            tracks: tracks.map { track in
                VesperMediaTrack(
                    id: track.id,
                    kind: .subtitle,
                    language: track.language,
                    isDefault: track.isDefault,
                    isForced: track.isForced
                )
            }
        )
    }

    func testEmbeddedSubtitleIdentityPreservesSemanticFieldBoundaries() {
        let labelContainsDelimiter = stableEmbeddedSubtitleTrackId(
            language: "en",
            label: "main:forced",
            isForced: false,
            codecValues: [],
            characteristics: []
        )
        let forcedTrack = stableEmbeddedSubtitleTrackId(
            language: "en",
            label: "main",
            isForced: true,
            codecValues: [],
            characteristics: []
        )
        let missingLanguage = stableEmbeddedSubtitleTrackId(
            language: nil,
            label: "main",
            isForced: false,
            codecValues: [],
            characteristics: []
        )
        let emptyLanguage = stableEmbeddedSubtitleTrackId(
            language: "",
            label: "main",
            isForced: false,
            codecValues: [],
            characteristics: []
        )

        XCTAssertNotEqual(labelContainsDelimiter, forcedTrack)
        XCTAssertNotEqual(missingLanguage, emptyLanguage)
    }

    func testSubtitleStateUnavailableHasZeroCounts() {
        let state = VesperSubtitleState.unavailable()
        XCTAssertEqual(state.status, .unavailable)
        XCTAssertEqual(state.advertisedTrackCount, 0)
        XCTAssertEqual(state.selectableTrackCount, 0)
        XCTAssertNil(state.error)
    }

    func testSubtitleStateLoadingPreservesAdvertisedCount() {
        let state = VesperSubtitleState.loading(advertisedTrackCount: 2)
        XCTAssertEqual(state.status, .loading)
        XCTAssertEqual(state.advertisedTrackCount, 2)
        XCTAssertEqual(state.selectableTrackCount, 0)
        XCTAssertNil(state.error)
    }

    func testSubtitleStateReadyCarriesSelectableCount() {
        let state = VesperSubtitleState.ready(advertisedTrackCount: 2, selectableTrackCount: 2)
        XCTAssertEqual(state.status, .ready)
        XCTAssertEqual(state.advertisedTrackCount, 2)
        XCTAssertEqual(state.selectableTrackCount, 2)
        XCTAssertNil(state.error)
    }

    func testSubtitleStateFailedPreservesAdvertisedCountAndCarriesStructuredError() throws {
        let state = VesperSubtitleState.failed(
            advertisedTrackCount: 3,
            code: "subtitle_platform_track_unavailable",
            phase: .discovery,
            message: "no legible group"
        )
        XCTAssertEqual(state.status, .failed)
        XCTAssertEqual(state.advertisedTrackCount, 3)
        XCTAssertEqual(state.selectableTrackCount, 0)
        let error = try XCTUnwrap(state.error)
        XCTAssertEqual(error.code, "subtitle_platform_track_unavailable")
        XCTAssertEqual(error.phase, .discovery)
        XCTAssertNil(error.trackId)
        XCTAssertFalse(error.retriable)
    }

    func testSubtitleStateFailedCarriesTrackIdForSelectionFailures() throws {
        let state = VesperSubtitleState.failed(
            advertisedTrackCount: 1,
            code: "subtitle_track_not_found",
            phase: .selection,
            trackId: "subtitle:dash:sub-en",
            message: "missing"
        )
        XCTAssertEqual(state.catalogState, .ready)
        XCTAssertEqual(state.selectionState, .failed)
        let error = try XCTUnwrap(state.selectionError)
        XCTAssertEqual(error.trackId, "subtitle:dash:sub-en")
        XCTAssertEqual(error.phase, .selection)
    }

    func testCatalogFailureReplacementPreservesSelectionFailure() throws {
        let selectionFailure = VesperSubtitleError(
            code: "subtitle_selection_timeout",
            phase: .selection,
            trackId: "opaque-track-id",
            retriable: true,
            message: "timed out",
            commandId: 9,
            sourceEpoch: 3
        )
        let current = VesperSubtitleState(
            catalogState: .ready,
            selectionState: .failed,
            advertisedTrackCount: 2,
            selectableTrackCount: 2,
            catalogError: nil,
            selectionError: selectionFailure
        )
        let catalogFailure = VesperSubtitleState.failed(
            advertisedTrackCount: 2,
            code: "subtitle_manifest_parse_failed",
            phase: .manifest,
            message: "manifest failed"
        )

        let merged = current.replacingCatalog(with: catalogFailure)

        XCTAssertEqual(merged.catalogState, .failed)
        XCTAssertEqual(merged.catalogError?.code, "subtitle_manifest_parse_failed")
        XCTAssertEqual(merged.selectionState, .failed)
        XCTAssertEqual(merged.selectionError, selectionFailure)
    }

    @MainActor
    func testReportSubtitleFailurePublishesBothLastErrorAndSubtitleState() throws {
        let bridge = VesperNativePlayerBridge()
        bridge.publishedSubtitleState = .ready(advertisedTrackCount: 2, selectableTrackCount: 2)

        bridge.reportSubtitleFailure(
            code: "subtitle_track_not_found",
            phase: .selection,
            trackId: "subtitle:dash:sub-zh",
            message: "track not in catalog"
        )

        XCTAssertEqual(bridge.publishedSubtitleState.catalogState, .ready)
        XCTAssertEqual(bridge.publishedSubtitleState.selectionState, .failed)
        // Advertised count must be preserved across the failure transition
        // so a future ready state can still show "2 of 2 subtitles".
        XCTAssertEqual(bridge.publishedSubtitleState.advertisedTrackCount, 2)
        XCTAssertEqual(bridge.publishedSubtitleState.selectableTrackCount, 2)
        let subtitleError = try XCTUnwrap(bridge.publishedSubtitleState.selectionError)
        XCTAssertEqual(subtitleError.code, "subtitle_track_not_found")
        XCTAssertEqual(subtitleError.phase, .selection)
        XCTAssertEqual(subtitleError.trackId, "subtitle:dash:sub-zh")

        // The existing generic `lastError` channel must also carry the
        // structured subtitle phase/code details so Flutter consumers that
        // have not migrated to subtitleState can still observe the failure.
        let lastError = try XCTUnwrap(bridge.publishedLastError)
        XCTAssertEqual(lastError.code, .invalidState)
        XCTAssertEqual(lastError.category, .capability)
        XCTAssertEqual(lastError.details["phase"], "selection")
        XCTAssertEqual(lastError.details["code"], "subtitle_track_not_found")
        XCTAssertEqual(lastError.details["trackId"], "subtitle:dash:sub-zh")
    }

    @MainActor
    func testStaleDashResourceFailureDoesNotOverwriteCurrentSourceState() {
        let oldSource = VesperPlayerSource.dash(
            url: URL(string: "https://example.test/old.mpd")!
        )
        let currentSource = VesperPlayerSource.dash(
            url: URL(string: "https://example.test/current.mpd")!
        )
        let oldSession = VesperDashSession(sourceURL: URL(string: oldSource.uri)!)
        let currentSession = VesperDashSession(sourceURL: URL(string: currentSource.uri)!)
        let bridge = VesperNativePlayerBridge(initialSource: currentSource)
        bridge.currentDashSession = currentSession
        bridge.publishedSubtitleState = .ready(
            advertisedTrackCount: 1,
            selectableTrackCount: 1
        )

        bridge.reportDashSubtitleResourceFailure(
            session: oldSession,
            source: oldSource
        )

        XCTAssertEqual(bridge.publishedSubtitleState.status, .ready)
        XCTAssertNil(bridge.publishedSubtitleState.error)

        bridge.reportDashSubtitleResourceFailure(
            session: currentSession,
            source: currentSource,
            trackId: "subtitle:dash:sub-en"
        )
        XCTAssertEqual(bridge.publishedSubtitleState.status, .failed)
        XCTAssertEqual(
            bridge.publishedSubtitleState.error?.code,
            "subtitle_resource_failed"
        )
    }

    @MainActor
    func testDashResourceFailureKeepsRemainingSubtitleSelectable() {
        let source = VesperPlayerSource.dash(
            url: URL(string: "https://example.test/current.mpd")!
        )
        let session = VesperDashSession(sourceURL: URL(string: source.uri)!)
        let bridge = VesperNativePlayerBridge(initialSource: source)
        bridge.currentDashSession = session
        bridge.publishedTrackCatalog = VesperTrackCatalog(
            tracks: [
                VesperMediaTrack(id: "subtitle:dash:sub-en", kind: .subtitle),
                VesperMediaTrack(id: "subtitle:dash:sub-ja", kind: .subtitle),
            ]
        )
        bridge.publishedSubtitleState = .ready(
            advertisedTrackCount: 2,
            selectableTrackCount: 2
        )

        bridge.reportDashSubtitleResourceFailure(
            session: session,
            source: source,
            trackId: "sub-en"
        )
        bridge.reportDashSubtitleResourceFailure(
            session: session,
            source: source,
            trackId: "sub-en"
        )

        XCTAssertEqual(bridge.publishedSubtitleState.catalogState, .ready)
        XCTAssertEqual(bridge.publishedSubtitleState.advertisedTrackCount, 2)
        XCTAssertEqual(bridge.publishedSubtitleState.selectableTrackCount, 1)
        XCTAssertEqual(
            bridge.publishedSubtitleState.catalogError?.code,
            "subtitle_resource_failed"
        )
        XCTAssertEqual(
            bridge.publishedTrackCatalog.subtitleTracks.map(\.id),
            ["subtitle:dash:sub-ja"]
        )
    }

    @MainActor
    func testClearSubtitleFailureRevertsToReadyWhenSelectableTracksExist() {
        let bridge = VesperNativePlayerBridge()
        bridge.publishedSubtitleState = VesperSubtitleState(
            catalogState: .ready,
            selectionState: .failed,
            advertisedTrackCount: 2,
            selectableTrackCount: 1,
            selectionError: VesperSubtitleError(
                code: "subtitle_track_not_found",
                phase: .selection,
                trackId: nil,
                retriable: false,
                message: "previous failure"
            )
        )

        bridge.clearSubtitleFailure()

        XCTAssertEqual(bridge.publishedSubtitleState.catalogState, .ready)
        XCTAssertEqual(bridge.publishedSubtitleState.selectionState, .idle)
        XCTAssertEqual(bridge.publishedSubtitleState.advertisedTrackCount, 2)
        XCTAssertEqual(bridge.publishedSubtitleState.selectableTrackCount, 1)
        XCTAssertNil(bridge.publishedSubtitleState.error)
    }

    @MainActor
    func testClearSubtitleFailureDoesNotEraseCatalogFailure() {
        let bridge = VesperNativePlayerBridge()
        bridge.publishedSubtitleState = .failed(
            advertisedTrackCount: 1,
            code: "subtitle_platform_track_unavailable",
            phase: .discovery,
            message: "no group"
        )

        bridge.clearSubtitleFailure()

        XCTAssertEqual(bridge.publishedSubtitleState.catalogState, .failed)
        XCTAssertEqual(bridge.publishedSubtitleState.selectionState, .idle)
        XCTAssertEqual(bridge.publishedSubtitleState.advertisedTrackCount, 1)
        XCTAssertEqual(
            bridge.publishedSubtitleState.catalogError?.code,
            "subtitle_platform_track_unavailable"
        )
        XCTAssertNil(bridge.publishedSubtitleState.selectionError)
    }

    @MainActor
    func testControllerSubtitleStateWireMapMatchesFlutterContract() throws {
        let bridge = VesperNativePlayerBridge()
        bridge.publishedSubtitleState = .failed(
            advertisedTrackCount: 2,
            code: "subtitle_track_not_found",
            phase: .selection,
            trackId: "subtitle:dash:sub-en",
            message: "missing"
        )
        let controller = VesperPlayerController(bridge)

        let map = controller.subtitleStateWireMap()

        XCTAssertEqual(map["catalogState"] as? String, "ready")
        XCTAssertEqual(map["selectionState"] as? String, "failed")
        // The legacy status is catalog-derived; the legacy error alias still
        // exposes the selection failure for old clients.
        XCTAssertEqual(map["status"] as? String, "ready")
        XCTAssertEqual(map["advertisedTrackCount"] as? Int, 2)
        XCTAssertEqual(map["selectableTrackCount"] as? Int, 0)
        XCTAssertTrue(map["catalogError"] is NSNull)
        let errorMap = try XCTUnwrap(map["selectionError"] as? [String: Any])
        XCTAssertEqual(errorMap["code"] as? String, "subtitle_track_not_found")
        XCTAssertEqual(errorMap["phase"] as? String, "selection")
        XCTAssertEqual(errorMap["trackId"] as? String, "subtitle:dash:sub-en")
        XCTAssertEqual(errorMap["retriable"] as? Bool, false)
        XCTAssertEqual(errorMap["message"] as? String, "missing")
        XCTAssertEqual(
            (map["error"] as? [String: Any])?["code"] as? String,
            "subtitle_track_not_found"
        )
    }

    @MainActor
    func testControllerSubtitleStateWireMapEmitsNullErrorWhenReady() {
        let bridge = VesperNativePlayerBridge()
        bridge.publishedSubtitleState = .ready(advertisedTrackCount: 1, selectableTrackCount: 1)
        let controller = VesperPlayerController(bridge)

        let map = controller.subtitleStateWireMap()

        XCTAssertEqual(map["status"] as? String, "ready")
        XCTAssertTrue(map["error"] is NSNull)
    }

    // MARK: - setSubtitleTrackSelection throws

    /// When the AV legible group is missing for a `.track(id)` request, the
    /// bridge must throw
    /// `.platformTrackUnavailable(trackId:)` so the iOS Flutter plugin's
    /// `handleSessionCommand` catch converts it to a `FlutterError` and
    /// the Dart `Future<void>` actually fails.
    ///
    /// This test verifies the error type's payload contract directly
    /// because constructing a real `AVPlayerItem` + legible group in a
    /// unit test is not feasible. The end-to-end plugin path is exercised
    /// by the integration test suite.
    func testPlatformTrackUnavailableErrorCarriesStructuredCode() {
        let error = VesperSubtitleSelectionError.platformTrackUnavailable(
            trackId: "subtitle:dash:sub-en"
        )
        XCTAssertEqual(error.subtitleCode, "subtitle_platform_track_unavailable")
        XCTAssertEqual(error.subtitleTrackId, "subtitle:dash:sub-en")
        XCTAssertEqual(
            error.errorDescription,
            "No legible media selection group is available. trackId=subtitle:dash:sub-en"
        )
    }

    /// `.platformTrackUnavailable` without a trackId produces the correct
    /// message shape (used by `.auto` paths where no id is in flight).
    func testPlatformTrackUnavailableErrorWithoutTrackId() {
        let error = VesperSubtitleSelectionError.platformTrackUnavailable(trackId: nil)
        XCTAssertEqual(error.subtitleCode, "subtitle_platform_track_unavailable")
        XCTAssertNil(error.subtitleTrackId)
        XCTAssertEqual(
            error.errorDescription,
            "No legible media selection group is available."
        )
    }

    /// A `.track(id)` for an id not in `subtitleOptionsByTrackId` must
    /// throw `.trackNotFound(trackId:)` carrying the offending id.
    func testSetSubtitleTrackSelectionThrowsForUnknownTrackId() {
        let error = VesperSubtitleSelectionError.trackNotFound(trackId: "subtitle:dash:sub-zh")
        XCTAssertEqual(error.subtitleCode, "subtitle_track_not_found")
        XCTAssertEqual(error.subtitleTrackId, "subtitle:dash:sub-zh")
        XCTAssertEqual(
            error.errorDescription,
            "Subtitle trackId=subtitle:dash:sub-zh is not in the current catalog."
        )
    }

    /// `.autoCandidateUnavailable` carries the matching subtitle_* code.
    func testAutoCandidateUnavailableErrorCarriesStructuredCode() {
        let error = VesperSubtitleSelectionError.autoCandidateUnavailable
        XCTAssertEqual(error.subtitleCode, "subtitle_auto_candidate_unavailable")
        XCTAssertNil(error.subtitleTrackId)
        XCTAssertEqual(
            error.errorDescription,
            "No subtitle candidate is available for automatic selection."
        )
    }

    func testAutomaticSubtitleResolverPrefersNativeDefaultOverNonDefaultExternal() {
        let nativeDefault = VesperMediaTrack(
            id: "native-default",
            kind: .subtitle,
            language: "ja",
            isDefault: true
        )
        let externalNonDefault = VesperMediaTrack(
            id: "external-non-default",
            kind: .subtitle,
            language: "en"
        )

        let selected = resolveAutomaticSubtitleTrackId(
            tracks: [externalNonDefault, nativeDefault],
            preferredLanguage: nil,
            selectUndeterminedLanguage: false,
            allowDefaultCandidate: true
        )

        XCTAssertEqual(selected, nativeDefault.id)
    }

    func testAutomaticSubtitleResolverAppliesLanguageBeforeBackendOrDefault() {
        let nativeDefault = VesperMediaTrack(
            id: "native-default",
            kind: .subtitle,
            language: "ja",
            isDefault: true
        )
        let externalEnglish = VesperMediaTrack(
            id: "external-english",
            kind: .subtitle,
            language: "en-US"
        )

        let selected = resolveAutomaticSubtitleTrackId(
            tracks: [nativeDefault, externalEnglish],
            preferredLanguage: "en",
            selectUndeterminedLanguage: false,
            allowDefaultCandidate: true
        )

        XCTAssertEqual(selected, externalEnglish.id)
    }

    func testAutomaticSubtitleResolverIsStableAcrossCandidateOrdering() {
        let first = VesperMediaTrack(
            id: "track-b",
            kind: .subtitle,
            language: "en"
        )
        let preferredDefault = VesperMediaTrack(
            id: "track-a",
            kind: .subtitle,
            language: "en",
            isDefault: true
        )

        let forward = resolveAutomaticSubtitleTrackId(
            tracks: [first, preferredDefault],
            preferredLanguage: "en",
            selectUndeterminedLanguage: false,
            allowDefaultCandidate: false
        )
        let reversed = resolveAutomaticSubtitleTrackId(
            tracks: [preferredDefault, first],
            preferredLanguage: "en",
            selectUndeterminedLanguage: false,
            allowDefaultCandidate: false
        )

        XCTAssertEqual(forward, preferredDefault.id)
        XCTAssertEqual(reversed, preferredDefault.id)
    }

    func testAutomaticSubtitleDefaultCandidateRespectsSelectionOrigin() {
        let defaultTrack = VesperMediaTrack(
            id: "external-default",
            kind: .subtitle,
            language: "zh",
            isDefault: true
        )

        func resolvedTrackId(
            origin: SubtitleSelectionOrigin,
            startupPolicySelectsSubtitlesByDefault: Bool
        ) -> String? {
            resolveAutomaticSubtitleTrackId(
                tracks: [defaultTrack],
                preferredLanguage: nil,
                selectUndeterminedLanguage: false,
                allowDefaultCandidate: automaticSubtitleSelectionAllowsDefaultCandidate(
                    origin: origin,
                    startupPolicySelectsSubtitlesByDefault:
                        startupPolicySelectsSubtitlesByDefault
                )
            )
        }

        XCTAssertNil(
            resolvedTrackId(
                origin: .defaultPolicy,
                startupPolicySelectsSubtitlesByDefault: false
            )
        )
        XCTAssertEqual(
            resolvedTrackId(
                origin: .defaultPolicy,
                startupPolicySelectsSubtitlesByDefault: true
            ),
            defaultTrack.id
        )
        for origin in [
            SubtitleSelectionOrigin.explicit,
            .resilienceRestore,
            .visibilityRestore,
        ] {
            XCTAssertEqual(
                resolvedTrackId(
                    origin: origin,
                    startupPolicySelectsSubtitlesByDefault: false
                ),
                defaultTrack.id
            )
        }
    }

    /// `.selectionDidNotConverge(trackId:)` carries the matching code.
    func testSelectionDidNotConvergeErrorCarriesStructuredCode() {
        let error = VesperSubtitleSelectionError.selectionDidNotConverge(trackId: "subtitle:dash:sub-en")
        XCTAssertEqual(error.subtitleCode, "subtitle_selection_mismatch")
        XCTAssertEqual(error.subtitleTrackId, "subtitle:dash:sub-en")
        XCTAssertEqual(
            error.errorDescription,
            "AVPlayer did not converge on the requested subtitle option. trackId=subtitle:dash:sub-en"
        )
    }

    func testSelectionCommandErrorPreservesTransactionIdentity() {
        let error = VesperSubtitleSelectionCommandError(
            failure: .selectionTimedOut(trackId: "opaque-track-id"),
            commandId: 42,
            sourceEpoch: 7
        )

        XCTAssertEqual(error.code, "subtitle_selection_timeout")
        XCTAssertEqual(error.trackId, "opaque-track-id")
        XCTAssertTrue(error.retriable)
        XCTAssertEqual(error.commandId, 42)
        XCTAssertEqual(error.sourceEpoch, 7)
    }

    func testInternalSelectionOriginsCannotSupersedeExplicitIntent() {
        XCTAssertFalse(
            SubtitleSelectionOrigin.defaultPolicy.canSupersede(.explicit)
        )
        XCTAssertFalse(
            SubtitleSelectionOrigin.resilienceRestore.canSupersede(.explicit)
        )
        XCTAssertFalse(
            SubtitleSelectionOrigin.visibilityRestore.canSupersede(.explicit)
        )
        XCTAssertTrue(
            SubtitleSelectionOrigin.explicit.canSupersede(.resilienceRestore)
        )
        XCTAssertTrue(
            SubtitleSelectionOrigin.resilienceRestore.canSupersede(.defaultPolicy)
        )
    }
}

private let sampleHlsSubtitleMaster = #"""
#EXTM3U
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",NAME="English",LANGUAGE="en",DEFAULT=YES,AUTOSELECT=YES,FORCED=NO,URI="sub-en.m3u8"
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",NAME="English Forced",LANGUAGE="en",DEFAULT=NO,AUTOSELECT=YES,FORCED=YES,URI="sub-en-forced.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=800000,SUBTITLES="subs"
video.m3u8
"""#

private let sampleHlsSubtitleMasterReordered = #"""
#EXTM3U
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",NAME="English Forced",LANGUAGE="en",DEFAULT=NO,AUTOSELECT=YES,FORCED=YES,URI="sub-en-forced.m3u8"
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",NAME="English",LANGUAGE="en",DEFAULT=YES,AUTOSELECT=YES,FORCED=NO,URI="sub-en.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=800000,SUBTITLES="subs"
video.m3u8
"""#
