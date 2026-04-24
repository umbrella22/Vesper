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
    static var tabPlayer: String { string("example.tab.player") }
    static var tabDownloads: String { string("example.tab.downloads") }

    static var sourcesTitle: String { string("example.sources.title") }
    static var sourcesSubtitle: String { string("example.sources.subtitle") }
    static var pickVideo: String { string("example.sources.pick_video") }
    static var pickFromPhotos: String { string("example.sources.pick_from_photos") }
    static var useHlsDemo: String { string("example.sources.use_hls_demo") }
    static var useDashDemo: String { string("example.sources.use_dash_demo") }
    static var dashDemoUnavailableNote: String { string("example.sources.dash_demo_unavailable_note") }
    static var themeTitle: String { string("example.sources.theme_title") }
    static var remoteUrlPlaceholder: String { string("example.sources.remote_url_placeholder") }
    static var openRemoteUrl: String { string("example.sources.open_remote_url") }
    static var playlistTitle: String { string("example.playlist.title") }
    static var playlistSubtitle: String { string("example.playlist.subtitle") }
    static var playlistEmpty: String { string("example.playlist.empty") }
    static var playlistStatusCurrent: String { string("example.playlist.status_current") }
    static var playlistStatusVisible: String { string("example.playlist.status_visible") }
    static var playlistStatusNearVisible: String { string("example.playlist.status_near_visible") }
    static var playlistStatusPrefetch: String { string("example.playlist.status_prefetch") }
    static var playlistStatusHidden: String { string("example.playlist.status_hidden") }
    static var downloadHeaderSubtitle: String { string("example.download.header_subtitle") }
    static var downloadCreateTitle: String { string("example.download.create_title") }
    static var downloadCreateSubtitle: String { string("example.download.create_subtitle") }
    static var downloadHlsDemo: String { string("example.download.hls_demo") }
    static var downloadDashDemo: String { string("example.download.dash_demo") }
    static var downloadRemoteUrl: String { string("example.download.remote_url") }
    static var downloadCreateRemoteTask: String { string("example.download.create_remote_task") }
    static var downloadTasksTitle: String { string("example.download.tasks_title") }
    static var downloadTasksSubtitle: String { string("example.download.tasks_subtitle") }
    static var downloadEmpty: String { string("example.download.empty") }
    static var downloadStateQueued: String { string("example.download.state_queued") }
    static var downloadStatePreparing: String { string("example.download.state_preparing") }
    static var downloadStateDownloading: String { string("example.download.state_downloading") }
    static var downloadStatePaused: String { string("example.download.state_paused") }
    static var downloadStateCompleted: String { string("example.download.state_completed") }
    static var downloadStateFailed: String { string("example.download.state_failed") }
    static var downloadStateRemoved: String { string("example.download.state_removed") }
    static var downloadActionStart: String { string("example.download.action_start") }
    static var downloadActionPause: String { string("example.download.action_pause") }
    static var downloadActionResume: String { string("example.download.action_resume") }
    static var downloadSaveToPhotos: String { string("example.download.save_to_photos") }
    static var downloadRemoveTask: String { string("example.download.remove_task") }
    static var downloadProgressUnknown: String { string("example.download.progress_unknown") }
    static var downloadCreateTaskFailed: String { string("example.download.create_task_failed") }
    static var downloadActionFailed: String { string("example.download.action_failed") }
    static var downloadPendingTaskDetails: String { string("example.download.pending_task_details") }
    static var downloadExporting: String { string("example.download.exporting") }
    static var downloadExportPluginReady: String { string("example.download.export_plugin_ready") }
    static var downloadExportPluginMissing: String { string("example.download.export_plugin_missing") }
    static var downloadSaveToPhotosTitle: String { string("example.download.save_to_photos_title") }
    static var downloadSaveToPhotosSuccess: String { string("example.download.save_to_photos_success") }
    static var downloadSaveToPhotosMissingOutput: String { string("example.download.save_to_photos_missing_output") }
    static var downloadSaveToPhotosFailedUnknown: String { string("example.download.save_to_photos_failed_unknown") }
    static func downloadTaskMeta(_ assetId: String, _ state: String) -> String {
        string("example.download.task_meta", assetId, state)
    }
    static func downloadProgressBytes(_ received: String, _ total: String) -> String {
        string("example.download.progress_bytes", received, total)
    }
    static func downloadCompletedPath(_ path: String) -> String {
        string("example.download.completed_path", path)
    }
    static func downloadPendingSourceUri(_ path: String) -> String {
        string("example.download.pending_source_uri", path)
    }
    static func downloadErrorMessage(_ message: String) -> String {
        string("example.download.error_message", message)
    }
    static func downloadSaveToPhotosFailed(_ reason: String) -> String {
        string("example.download.save_to_photos_failed", reason)
    }
    static func downloadExportProgress(_ value: Int) -> String {
        string("example.download.export_progress", value)
    }

    static var resilienceTitle: String { string("example.resilience.title") }
    static var resilienceSubtitle: String { string("example.resilience.subtitle") }
    static var resilienceApplying: String { string("example.resilience.applying") }
    static var resilienceFactBuffering: String { string("example.resilience.fact_buffering") }
    static var resilienceFactRetry: String { string("example.resilience.fact_retry") }
    static var resilienceFactCache: String { string("example.resilience.fact_cache") }
    static var resilienceBalanced: String { string("example.resilience.balanced") }
    static var resilienceBalancedSubtitle: String { string("example.resilience.balanced_subtitle") }
    static var resilienceStreaming: String { string("example.resilience.streaming") }
    static var resilienceStreamingSubtitle: String { string("example.resilience.streaming_subtitle") }
    static var resilienceResilient: String { string("example.resilience.resilient") }
    static var resilienceResilientSubtitle: String { string("example.resilience.resilient_subtitle") }
    static var resilienceLowLatency: String { string("example.resilience.low_latency") }
    static var resilienceLowLatencySubtitle: String { string("example.resilience.low_latency_subtitle") }
    static var resiliencePresetDefault: String { string("example.resilience.preset_default") }
    static var resiliencePresetBalanced: String { string("example.resilience.preset_balanced") }
    static var resiliencePresetStreaming: String { string("example.resilience.preset_streaming") }
    static var resiliencePresetResilient: String { string("example.resilience.preset_resilient") }
    static var resiliencePresetLowLatency: String { string("example.resilience.preset_low_latency") }
    static var resiliencePresetDisabled: String { string("example.resilience.preset_disabled") }
    static var resilienceBackoffFixed: String { string("example.resilience.backoff_fixed") }
    static var resilienceBackoffLinear: String { string("example.resilience.backoff_linear") }
    static var resilienceBackoffExponential: String { string("example.resilience.backoff_exponential") }
    static var resilienceWindowDefault: String { string("example.resilience.window_default") }
    static func resilienceWindowRange(_ minMs: Int64, _ maxMs: Int64) -> String {
        string("example.resilience.window_range", minMs, maxMs)
    }
    static func resilienceRetryAttempts(_ value: Int) -> String {
        string("example.resilience.retry_attempts", value)
    }
    static var resilienceRetryUnlimited: String { string("example.resilience.retry_unlimited") }
    static func resilienceRetryValue(_ attempts: String, _ backoff: String) -> String {
        string("example.resilience.retry_value", attempts, backoff)
    }
    static func resilienceCacheValue(_ preset: String, _ memory: String, _ disk: String) -> String {
        string("example.resilience.cache_value", preset, memory, disk)
    }

    static var preparingSelectedVideo: String { string("example.message.preparing_selected_video") }
    static var preparingVideoFromPhotos: String { string("example.message.preparing_video_from_photos") }
    static var invalidRemoteUrl: String { string("example.message.invalid_remote_url") }
    static var dashNotSupportedOnIos: String { string("example.message.dash_not_supported_on_ios") }
    static var photoLibraryAccessRequired: String { string("example.message.photo_library_required") }
    static var photoLibraryAddAccessRequired: String { string("example.message.photo_library_add_required") }
    static var unknownPhotoAuthorizationState: String { string("example.message.unknown_photo_authorization_state") }
    static var failedToLoadSelectedVideoFromPhotos: String { string("example.message.failed_to_load_selected_video_from_photos") }
    static func failedToLoadSelectedLocalVideo(_ reason: String) -> String {
        string("example.message.failed_to_load_selected_local_video", reason)
    }
    static func failedToLoadSelectedPhotoVideo(_ reason: String) -> String {
        string("example.message.failed_to_load_selected_photo_video", reason)
    }

    static var hlsDemoLabel: String { string("example.source.hls_demo_label") }
    static var dashDemoLabel: String { string("example.source.dash_demo_label") }
    static var customRemoteUrlLabel: String { string("example.source.custom_remote_url_label") }

    static var qualityButtonCapped: String { string("example.quality.button_capped") }
    static var qualityButtonPinned: String { string("example.quality.button_pinned") }
    static var qualityButtonLocking: String { string("example.quality.button_locking") }
    static var qualityStatusPending: String { string("example.quality.status_pending") }
    static var qualityStatusLocked: String { string("example.quality.status_locked") }
    static var qualityStatusFallback: String { string("example.quality.status_fallback") }
    static var qualityStatusRequested: String { string("example.quality.status_requested") }
    static var qualityNoVideoTracks: String { string("example.quality.no_video_tracks") }
    static var qualityFixedTrackUnavailable: String { string("example.quality.fixed_track_unavailable") }
    static var qualityRuntimeNoticeTitle: String { string("example.quality.runtime_notice_title") }
    static func qualityAutoSubtitleWithEffective(_ effective: String) -> String {
        string("example.quality.auto_subtitle_effective", effective)
    }
    static func qualityAutoSubtitleWithObservation(_ observation: String) -> String {
        string("example.quality.auto_subtitle_observation", observation)
    }
    static func qualityFixedSubtitlePending(_ requested: String) -> String {
        string("example.quality.fixed_subtitle_pending", requested)
    }
    static func qualityFixedSubtitleLocked(_ requested: String) -> String {
        string("example.quality.fixed_subtitle_locked", requested)
    }
    static func qualityFixedSubtitleFallback(_ requested: String, _ effective: String) -> String {
        string("example.quality.fixed_subtitle_fallback", requested, effective)
    }
    static func qualityFixedSubtitleObservation(_ observation: String) -> String {
        string("example.quality.fixed_subtitle_observation", observation)
    }

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
            return playbackTools
        case .quality:
            return quality
        case .audio:
            return audio
        case .subtitle:
            return subtitles
        case .speed:
            return playbackSpeed
        }
    }

    static func sheetSubtitle(_ sheet: ExamplePlayerSheet) -> String {
        switch sheet {
        case .menu:
            return string("example.sheet.menu.subtitle")
        case .quality:
            return string("example.sheet.quality.subtitle")
        case .audio:
            return string("example.sheet.audio.subtitle")
        case .subtitle:
            return string("example.sheet.subtitle.subtitle")
        case .speed:
            return string("example.sheet.speed.subtitle")
        }
    }

    static var qualityAutoSubtitle: String { string("example.sheet.quality.auto_subtitle") }
    static var audioAutoSubtitle: String { string("example.sheet.audio.auto_subtitle") }
    static var subtitleOffSubtitle: String { string("example.sheet.subtitle.off_subtitle") }
    static var subtitleAutoSubtitle: String { string("example.sheet.subtitle.auto_subtitle") }
    static var speedCurrentlyActive: String { string("example.sheet.speed.currently_active") }
    static var speedApplyImmediately: String { string("example.sheet.speed.apply_immediately") }
}
