import SwiftUI
import UIKit
import XCTest
import VesperPlayerKit
@testable import VesperPlayerKitUI

@MainActor
final class VesperStageStringsTests: XCTestCase {
    func testEnglishDefaults() {
        let strings = VesperPlayerStageStrings.localized(locale: Locale(identifier: "en"))
        XCTAssertEqual(strings.videoOnDemand, "Video on demand")
        XCTAssertEqual(strings.buffering, "Buffering")
        XCTAssertEqual(strings.play, "Play")
        XCTAssertEqual(strings.liveBehind("00:05"), "LIVE -00:05")
        XCTAssertEqual(stageBadgeText(timeline(.vod), strings: strings), "Video on demand")
    }

    func testSimplifiedChineseDefaults() {
        for identifier in ["zh-Hans", "zh-Hans-CN", "zh_CN"] {
            let strings = VesperPlayerStageStrings.localized(locale: Locale(identifier: identifier))
            XCTAssertEqual(strings.videoOnDemand, "点播视频", identifier)
            XCTAssertEqual(strings.buffering, "缓冲中", identifier)
            XCTAssertEqual(strings.pause, "暂停", identifier)
            XCTAssertEqual(strings.liveEdge("01:00"), "直播 · 实时点 01:00", identifier)
            XCTAssertEqual(stageBadgeText(timeline(.live), strings: strings), "直播流", identifier)
        }
    }

    func testUnsupportedLanguageFallsBackToEnglish() {
        let strings = VesperPlayerStageStrings.localized(locale: Locale(identifier: "ja"))
        XCTAssertEqual(strings, .localized(locale: Locale(identifier: "en")))
    }

    func testEveryKeyResolvesInShippedLocales() {
        for identifier in ["en", "zh-Hans"] {
            let strings = VesperPlayerStageStrings.localized(locale: Locale(identifier: identifier))
            let values = Mirror(reflecting: strings).children.compactMap { $0.value as? String }
            XCTAssertEqual(values.count, 20, identifier)
            for value in values {
                XCTAssertFalse(value.hasPrefix("vesper.stage."), "\(identifier) unresolved: \(value)")
            }
        }
    }

    func testHostOverrideReachesHelpersAndFormats() {
        var strings = VesperPlayerStageStrings.localized(locale: Locale(identifier: "en"))
        strings.videoOnDemand = "VOD"
        strings.liveBehindFormat = "Behind %@ (100%)"
        XCTAssertEqual(stageBadgeText(timeline(.vod), strings: strings), "VOD")
        XCTAssertEqual(strings.liveBehind("00:03"), "Behind 00:03 (100%)")
        strings.liveEdgeFormat = "Edge"
        XCTAssertEqual(strings.liveEdge("00:03"), "Edge")
    }

    func testStageScopesExplicitAndEnvironmentStrings() {
        var custom = VesperPlayerStageStrings.localized(locale: Locale(identifier: "en"))
        custom.videoOnDemand = "Custom badge"
        var scoped = custom
        scoped.videoOnDemand = "Scoped badge"
        let english = VesperPlayerStageStrings.localized(locale: Locale(identifier: "en"))
        let cases: [(String, VesperPlayerStageStrings?, VesperPlayerStageStrings?, String)] = [
            ("explicit", custom, nil, "Custom badge"),
            ("environment", nil, scoped, "Scoped badge"),
            ("explicit wins", custom, scoped, "Custom badge"),
        ]
        for (name, explicit, environment, expected) in cases {
            let probe = StringsProbe()
            var view = AnyView(stage(strings: explicit, probe: probe))
            if let environment { view = AnyView(view.vesperPlayerStageStrings(environment)) }
            mountAndRender(view)
            XCTAssertEqual(probe.observed?.videoOnDemand, expected, name)
        }
        let probe = StringsProbe()
        mountAndRender(AnyView(stage(strings: nil, probe: probe)))
        XCTAssertEqual(probe.observed, .localized(), "default follows system language")
        XCTAssertNotEqual(english.videoOnDemand, custom.videoOnDemand)
    }

    private func timeline(_ kind: TimelineKindUi) -> TimelineUiState {
        TimelineUiState(kind: kind, isSeekable: kind != .live, seekableRange: nil,
                        liveEdgeMs: nil, positionMs: 0, durationMs: 60_000)
    }

    private func stage(strings: VesperPlayerStageStrings?, probe: StringsProbe) -> VesperPlayerStage {
        VesperPlayerStage(
            surface: AnyView(Color.black),
            uiState: PlayerHostUiState(
                title: "Test", subtitle: "", sourceLabel: "Source",
                playbackState: .paused, playbackRate: 1, isBuffering: true,
                isInterrupted: false, timeline: timeline(.vod)
            ),
            trackCatalog: .empty,
            trackSelection: VesperTrackSelectionSnapshot(),
            effectiveVideoTrackId: nil,
            fixedTrackStatus: nil,
            controlsVisible: .constant(true),
            pendingSeekRatio: .constant(nil),
            controlLayout: .expanded,
            isFullscreen: false,
            onSeekBy: { _ in }, onTogglePause: {}, onSeekToRatio: { _ in },
            onSeekToLiveEdge: {}, onToggleFullscreen: {}, onOpenSheet: { _ in },
            contentOverlay: AnyView(StringsProbeView(probe: probe)),
            strings: strings
        )
    }

    private func mountAndRender(_ view: AnyView) {
        let host = UIHostingController(rootView: view)
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 600, height: 340))
        window.rootViewController = host
        window.isHidden = false
        defer { window.isHidden = true }
        host.view.layoutIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        host.view.layoutIfNeeded()
    }
}

@MainActor
private final class StringsProbe {
    var observed: VesperPlayerStageStrings?
}

@MainActor
private struct StringsProbeView: View {
    @Environment(\.vesperPlayerStageStrings) private var strings
    let probe: StringsProbe
    var body: some View {
        probe.observed = strings
        return Color.clear
    }
}
