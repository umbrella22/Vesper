import Foundation

private final class ExampleI18nBundleToken {}

enum ExampleI18n {
    private static let bundle = Bundle(for: ExampleI18nBundleToken.self)

    private static func string(_ key: String, _ args: CVarArg...) -> String {
        let format = bundle.localizedString(forKey: key, value: key, table: "Localizable")
        guard !args.isEmpty else { return format }
        return String(format: format, locale: Locale.current, arguments: args)
    }

    static var auto: String { string("example.common.auto") }
    static var off: String { string("example.common.off") }
    static var audio: String { string("example.common.audio") }
    static var subtitles: String { string("example.common.subtitles") }
    static var quality: String { string("example.common.quality") }
    static var playbackSpeed: String { string("example.common.playback_speed") }
    static var playbackTools: String { string("example.common.playback_tools") }

    static var themeSystem: String { string("example.theme.system") }
    static var themeLight: String { string("example.theme.light") }
    static var themeDark: String { string("example.theme.dark") }

    static var sourcesTitle: String { string("example.sources.title") }
    static var sourcesSubtitle: String { string("example.sources.subtitle") }
    static var pickFromPhotos: String { string("example.sources.pick_from_photos") }
    static var useHlsDemo: String { string("example.sources.use_hls_demo") }
    static var themeTitle: String { string("example.sources.theme_title") }
    static var remoteUrlPlaceholder: String { string("example.sources.remote_url_placeholder") }
    static var openRemoteUrl: String { string("example.sources.open_remote_url") }

    static var preparingVideoFromPhotos: String { string("example.message.preparing_video_from_photos") }
    static var invalidRemoteUrl: String { string("example.message.invalid_remote_url") }
    static var photoLibraryAccessRequired: String { string("example.message.photo_library_required") }
    static var unknownPhotoAuthorizationState: String { string("example.message.unknown_photo_authorization_state") }
    static var failedToLoadSelectedVideoFromPhotos: String { string("example.message.failed_to_load_selected_video_from_photos") }
    static func failedToLoadSelectedPhotoVideo(_ reason: String) -> String {
        string("example.message.failed_to_load_selected_photo_video", reason)
    }

    static var hlsDemoLabel: String { string("example.source.hls_demo_label") }
    static var customRemoteUrlLabel: String { string("example.source.custom_remote_url_label") }

    static var qualityButtonCapped: String { string("example.quality.button_capped") }
    static var qualityButtonPinned: String { string("example.quality.button_pinned") }

    static var captionsOff: String { string("example.subtitle.cc_off") }
    static var captionsAuto: String { string("example.subtitle.cc_auto") }

    static var stageVideoOnDemand: String { string("example.stage.video_on_demand") }
    static var stageLiveStream: String { string("example.stage.live_stream") }
    static var stageLiveWithDvrWindow: String { string("example.stage.live_with_dvr_window") }
    static var goLive: String { string("example.stage.go_live") }
    static var live: String { string("example.stage.live") }
    static func liveBehind(_ time: String) -> String {
        string("example.stage.live_behind", time)
    }
    static func liveEdge(_ time: String) -> String {
        string("example.stage.live_edge", time)
    }
    static var buffering: String { string("example.stage.buffering") }

    static var audioTrack: String { string("example.track.audio_track") }
    static func audioChannels(_ value: Int) -> String {
        string("example.track.audio_channels", value)
    }
    static func audioSampleRateKhz(_ value: Int) -> String {
        string("example.track.audio_sample_rate_khz", value)
    }
    static var audioProgram: String { string("example.track.audio_program") }
    static var subtitleTrack: String { string("example.track.subtitle_track") }
    static var subtitleForced: String { string("example.track.subtitle_forced") }
    static var subtitleDefault: String { string("example.track.subtitle_default") }
    static var subtitleOption: String { string("example.track.subtitle_option") }

    static func bitRateMbps(_ value: Double) -> String {
        string("example.unit.bitrate_mbps", value)
    }
    static func bitRateKbps(_ value: Double) -> String {
        string("example.unit.bitrate_kbps", value)
    }
    static func bitRateBps(_ value: Int64) -> String {
        string("example.unit.bitrate_bps", value)
    }
    static func playbackRate(_ value: Double) -> String {
        string("example.unit.playback_rate", value)
    }

    static var abrPresetDataSaverTitle: String { string("example.abr.data_saver.title") }
    static var abrPresetDataSaverSubtitle: String { string("example.abr.data_saver.subtitle") }
    static var abrPresetBalancedTitle: String { string("example.abr.balanced.title") }
    static var abrPresetBalancedSubtitle: String { string("example.abr.balanced.subtitle") }
    static var abrPresetHighTitle: String { string("example.abr.high.title") }
    static var abrPresetHighSubtitle: String { string("example.abr.high.subtitle") }

    static func sheetTitle(_ sheet: ExamplePlayerSheet) -> String {
        switch sheet {
        case .menu:
            playbackTools
        case .quality:
            quality
        case .audio:
            audio
        case .subtitle:
            subtitles
        case .speed:
            playbackSpeed
        }
    }

    static func sheetSubtitle(_ sheet: ExamplePlayerSheet) -> String {
        switch sheet {
        case .menu:
            string("example.sheet.menu.subtitle")
        case .quality:
            string("example.sheet.quality.subtitle")
        case .audio:
            string("example.sheet.audio.subtitle")
        case .subtitle:
            string("example.sheet.subtitle.subtitle")
        case .speed:
            string("example.sheet.speed.subtitle")
        }
    }

    static var qualityAutoSubtitle: String { string("example.sheet.quality.auto_subtitle") }
    static var audioAutoSubtitle: String { string("example.sheet.audio.auto_subtitle") }
    static var subtitleOffSubtitle: String { string("example.sheet.subtitle.off_subtitle") }
    static var subtitleAutoSubtitle: String { string("example.sheet.subtitle.auto_subtitle") }
    static var speedCurrentlyActive: String { string("example.sheet.speed.currently_active") }
    static var speedApplyImmediately: String { string("example.sheet.speed.apply_immediately") }
}
