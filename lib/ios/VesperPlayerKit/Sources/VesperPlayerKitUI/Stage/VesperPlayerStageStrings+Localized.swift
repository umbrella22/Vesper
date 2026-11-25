import Foundation
import SwiftUI

private final class VesperPlayerStageStringsBundleToken {}

public extension VesperPlayerStageStrings {
    /// SDK-provided text for the current system language.
    ///
    /// Resolution follows standard bundle localization, so the Stage matches the
    /// language the host app is running in. English is the fallback.
    static func localized() -> VesperPlayerStageStrings {
        VesperPlayerStageStringsBundle.systemDefault
    }

    /// SDK-provided text for an explicit locale, such as `Locale(identifier: "zh-Hans")`.
    /// Unsupported languages fall back to English.
    static func localized(locale: Locale) -> VesperPlayerStageStrings {
        load(from: VesperPlayerStageStringsBundle.bundle(for: locale))
    }

    fileprivate static func load(from bundle: Bundle) -> VesperPlayerStageStrings {
        func text(_ key: String) -> String {
            bundle.localizedString(forKey: key, value: key, table: "VesperPlayerStage")
        }
        return VesperPlayerStageStrings(
            auto: text("vesper.stage.auto"),
            quality: text("vesper.stage.quality"),
            qualityCapped: text("vesper.stage.quality_capped"),
            qualityPinned: text("vesper.stage.quality_pinned"),
            qualityLocking: text("vesper.stage.quality_locking"),
            videoOnDemand: text("vesper.stage.video_on_demand"),
            liveStream: text("vesper.stage.live_stream"),
            liveWithDvrWindow: text("vesper.stage.live_with_dvr_window"),
            goLive: text("vesper.stage.go_live"),
            live: text("vesper.stage.live"),
            liveBehindFormat: text("vesper.stage.live_behind"),
            liveEdgeFormat: text("vesper.stage.live_edge"),
            buffering: text("vesper.stage.buffering"),
            play: text("vesper.stage.play"),
            pause: text("vesper.stage.pause"),
            playbackPosition: text("vesper.stage.playback_position"),
            fullscreen: text("vesper.stage.fullscreen"),
            exitFullscreen: text("vesper.stage.exit_fullscreen"),
            navigateBack: text("vesper.stage.navigate_back"),
            more: text("vesper.stage.more")
        )
    }
}

enum VesperPlayerStageStringsBundle {
    static let resources: Bundle = {
        #if SWIFT_PACKAGE
        return .module
        #else
        return Bundle(for: VesperPlayerStageStringsBundleToken.self)
        #endif
    }()

    /// Cached because iOS relaunches apps when the system language changes.
    static let systemDefault = VesperPlayerStageStrings.load(from: resources)

    static func bundle(for locale: Locale) -> Bundle {
        let available = resources.localizations.filter { $0 != "Base" }
        let preferred = Bundle.preferredLocalizations(
            from: available,
            forPreferences: [locale.identifier(.bcp47)]
        )
        let fallback = resources.developmentLocalization ?? "en"
        for language in preferred + [fallback, "en"] {
            if let path = resources.path(forResource: language, ofType: "lproj"),
               let bundle = Bundle(path: path) {
                return bundle
            }
        }
        return resources
    }
}

private struct VesperPlayerStageStringsKey: EnvironmentKey {
    static var defaultValue: VesperPlayerStageStrings { VesperPlayerStageStringsBundle.systemDefault }
}

public extension EnvironmentValues {
    /// Active Stage text for standalone controls and host content inside a Stage.
    var vesperPlayerStageStrings: VesperPlayerStageStrings {
        get { self[VesperPlayerStageStringsKey.self] }
        set { self[VesperPlayerStageStringsKey.self] = newValue }
    }
}

public extension View {
    /// Overrides Stage text for a subtree, including Stages created without an
    /// explicit `strings` argument and standalone controls.
    @MainActor func vesperPlayerStageStrings(_ strings: VesperPlayerStageStrings) -> some View {
        environment(\.vesperPlayerStageStrings, strings)
    }
}
