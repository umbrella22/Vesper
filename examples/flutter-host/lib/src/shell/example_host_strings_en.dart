import 'example_host_strings.dart';

/// English host shell copy. Keys must stay in sync with the zh table.
const Map<ExampleHostText, String>
exampleHostStringsEn = <ExampleHostText, String>{
  ExampleHostText.appTitle: 'Vesper',
  ExampleHostText.tabPlay: 'Play',
  ExampleHostText.tabLibrary: 'Library',
  ExampleHostText.tabDownloads: 'Downloads',
  ExampleHostText.tabLab: 'Lab',
  ExampleHostText.settings: 'Settings',
  ExampleHostText.settingsAppearance: 'Appearance',
  ExampleHostText.settingsAbout: 'Vesper Player SDK example host · Flutter',
  ExampleHostText.themeSystem: 'System',
  ExampleHostText.themeLight: 'Light',
  ExampleHostText.themeDark: 'Dark',
  ExampleHostText.back: 'Back',
  ExampleHostText.close: 'Close',
  ExampleHostText.undo: 'Undo',
  ExampleHostText.openMedia: 'Open media',
  ExampleHostText.openMediaSubtitle:
      'Local files, stream URLs, or quick samples',
  ExampleHostText.openMediaPickLocal: 'Choose a video',
  ExampleHostText.openMediaPickLocalSubtitle: 'System picker, videos only',
  ExampleHostText.openMediaPickLocalUnavailable:
      'This host does not support local files.',
  ExampleHostText.openMediaStreamUrl: 'Stream URL',
  ExampleHostText.openMediaStreamUrlHint:
      'Enter an address that starts with http:// or https://.',
  ExampleHostText.openMediaOpenUrl: 'Open URL',
  ExampleHostText.openMediaQuickSamples: 'Quick samples',
  ExampleHostText.openMediaFailed: 'Could not open this media.',
  ExampleHostText.sampleHlsTitle: 'HLS demo',
  ExampleHostText.sampleHlsSubtitle: 'Apple BipBop, adaptive bitrate',
  ExampleHostText.sampleDashTitle: 'DASH demo',
  ExampleHostText.sampleDashSubtitle: 'Envivio, multi-rendition manifest',
  ExampleHostText.sampleLiveDvrTitle: 'Live DVR acceptance',
  ExampleHostText.sampleLiveDvrSubtitle:
      'Unified SCTE-35 public HLS for Go Live and DVR slider checks',
  ExampleHostText.dashUnavailable:
      'This platform host does not support the DASH demo yet.',
  ExampleHostText.urlErrorEmpty: 'Enter a stream URL.',
  ExampleHostText.urlErrorInvalid:
      'The URL must start with http:// or https://.',
  ExampleHostText.urlErrorDashUnsupported:
      'This platform host does not support DASH streams yet.',
  ExampleHostText.nowPlaying: 'Now playing',
  ExampleHostText.nothingPlaying: 'Nothing playing',
  ExampleHostText.quickPictureInPicture: 'Picture in Picture',
  ExampleHostText.quickCast: 'Cast',
  ExampleHostText.quickTracks: 'Audio & subtitles',
  ExampleHostText.upNext: 'Up next',
  ExampleHostText.manageQueue: 'Manage queue',
  ExampleHostText.queueEmptyTitle: 'The queue is empty',
  ExampleHostText.queueEmptyBody:
      'Pick a stream from the library or open media to add it here.',
  ExampleHostText.goToLibrary: 'Go to library',
  ExampleHostText.castSheetTitle: 'System playback & cast',
  ExampleHostText.libraryCollections: 'Collections',
  ExampleHostText.librarySubtitle:
      'Sample sources grouped by scenario. Open a collection, then tap an '
      'item to queue and play it.',
  ExampleHostText.collectionVodTitle: 'Basic VOD',
  ExampleHostText.collectionVodSummary:
      'HLS on demand for startup and seeking checks.',
  ExampleHostText.collectionLiveTitle: 'Live / DVR',
  ExampleHostText.collectionLiveSummary:
      'Live edge, DVR window, and SCTE-35 acceptance stream.',
  ExampleHostText.collectionAdaptiveTitle: 'Adaptive',
  ExampleHostText.collectionAdaptiveSummary:
      'Multi-bitrate HLS and DASH for ABR switching.',
  ExampleHostText.collectionDolbyTitle: 'HDR & Dolby',
  ExampleHostText.collectionDolbySummary:
      'Dolby Vision acceptance presets filtered by DRM, profile, and fps. '
      'Direct native playback only.',
  ExampleHostText.libraryAddedToQueue: 'Added to the queue',
  ExampleHostText.downloadsTasks: 'Tasks',
  ExampleHostText.downloadsNew: 'New download',
  ExampleHostText.downloadsNewSubtitle:
      'Use the HLS / DASH demos for quick regression, or paste a remote URL.',
  ExampleHostText.downloadsEmptyTitle: 'No download tasks yet',
  ExampleHostText.downloadsEmptyBody:
      'Create a foreground download from a demo or a remote URL.',
  ExampleHostText.downloadsScopeNote:
      'This example covers foreground execution and task lifecycle only. '
      'It does not promise background resume, offline DRM, or packaged '
      'HLS/DASH offline playback.',
  ExampleHostText.downloadRemoved: 'Download removed',
  ExampleHostText.labSubtitle:
      'Diagnostics for playback routes, plugins, and resilience policies.',
  ExampleHostText.labSystemPlayback: 'System playback',
  ExampleHostText.busyApplyingPolicy: 'Applying policy',
  ExampleHostText.busySwitchingPlugin: 'Switching plugins',
  ExampleHostText.cancel: 'Cancel',
  ExampleHostText.open: 'Open',
  ExampleHostText.none: 'None',
  ExampleHostText.auto: 'Auto',
  ExampleHostText.notSelected: 'Not selected',
  ExampleHostText.detailLine: '{label}: {value}',
  ExampleHostText.listSeparator: ', ',
  ExampleHostText.sheetMenuTitle: 'Playback tools',
  ExampleHostText.sheetMenuSubtitle:
      'Audio, subtitle, quality, and speed controls without crowding the '
      'player overlay.',
  ExampleHostText.sheetQualityTitle: 'Quality',
  ExampleHostText.sheetQualitySubtitle:
      'Switch adaptive video, or pin the stream to a specific quality track.',
  ExampleHostText.sheetAudioTitle: 'Audio',
  ExampleHostText.sheetAudioSubtitle:
      'Choose an audio program exposed by the current stream.',
  ExampleHostText.sheetSubtitleTitle: 'Subtitles',
  ExampleHostText.sheetSubtitleSubtitle: 'Choose subtitles or turn them off.',
  ExampleHostText.sheetSpeedTitle: 'Playback speed',
  ExampleHostText.sheetSpeedSubtitle: 'Preview playback at different speeds.',
  ExampleHostText.sheetNoAdaptiveVideo:
      'This route does not expose adaptive video switching.',
  ExampleHostText.sheetNoVideoTracks:
      'This media does not expose selectable video tracks.',
  ExampleHostText.sheetNoFixedTrack:
      'This platform supports auto or constrained ABR only, not fixed '
      'video tracks.',
  ExampleHostText.sheetAudioAutoSubtitle:
      'Use the player default audio selection.',
  ExampleHostText.sheetNoAudioTracks:
      'This media does not expose selectable audio programs.',
  ExampleHostText.sheetSubtitleOff: 'Off',
  ExampleHostText.sheetSubtitleOffSubtitle: 'Hide subtitle rendering.',
  ExampleHostText.sheetSubtitleAutoSubtitle:
      'Use the stream default subtitle behavior.',
  ExampleHostText.sheetNoSubtitleTracks:
      'This media does not expose selectable subtitle tracks.',
  ExampleHostText.sheetSpeedActive: 'Currently active.',
  ExampleHostText.sheetSpeedApply: 'Apply this speed now.',
  ExampleHostText.qualityLimited: 'Limited',
  ExampleHostText.qualityLockingValue: 'Locking · {quality}',
  ExampleHostText.qualityLockedValue: 'Locked · {quality}',
  ExampleHostText.qualityAutoValue: 'Auto · {quality}',
  ExampleHostText.qualityLeadAuto: 'Let the player adjust quality.',
  ExampleHostText.qualityLeadConstrainedWithin:
      'Adjusting quality within {constraints}. Tap to restore full auto.',
  ExampleHostText.qualityLeadConstrained:
      'Adjusting quality within a constrained ABR policy. Tap to restore '
      'full auto.',
  ExampleHostText.qualityLeadFallback:
      'Requested a lock to {requested}; the player is still on {effective}. '
      'Tap to switch back to auto.',
  ExampleHostText.qualityLeadPending:
      'Requested a lock to {requested}; waiting for the player to confirm. '
      'Tap to switch back to auto.',
  ExampleHostText.qualityLeadLocked:
      'Locked to {requested}. Tap to switch back to auto.',
  ExampleHostText.qualityLeadRequested:
      'Requested a lock to {requested}. Tap to switch back to auto.',
  ExampleHostText.qualityLeadSwitchToAuto: 'Switch back to automatic quality.',
  ExampleHostText.qualityObserved: ' Observed: {value}.',
  ExampleHostText.qualityEffective: ' Effective: {value}.',
  ExampleHostText.qualityStateLocked: 'Locked to this quality',
  ExampleHostText.qualityStatePending:
      'Waiting for the player to confirm this quality',
  ExampleHostText.qualityStateRequested: 'Lock requested for this quality',
  ExampleHostText.qualityStateStillHere: 'The player is still on this level',
  ExampleHostText.qualityBadgeCurrent: 'Current',
  ExampleHostText.qualityBadgeWaiting: 'Waiting',
  ExampleHostText.qualityBadgeLocking: 'Locking',
  ExampleHostText.qualityBadgeActual: 'Actual',
  ExampleHostText.abrMaxHeight: 'max {value}p',
  ExampleHostText.abrMaxWidth: 'max width {value}',
  ExampleHostText.abrMaxBitRate: 'max {value}',
  ExampleHostText.qualityBestEffortNotice:
      'This platform pins quality per HLS variant on a best-effort basis '
      'and cannot guarantee an exact single video track.',
  ExampleHostText.qualityFellBackConstrained: 'Fell back to constrained auto',
  ExampleHostText.qualityFellBackAuto: 'Fell back to auto quality',
  ExampleHostText.qualityLockNotConverged: 'Quality lock has not converged',
  ExampleHostText.qualityLockNotice: 'Quality lock notice',
  ExampleHostText.subtitleOffValue: 'Subtitles off',
  ExampleHostText.subtitleAutoValue: 'Subtitles auto',
  ExampleHostText.trackVideoFallback: 'Video track',
  ExampleHostText.trackVideoVariantFallback: 'Fixed video variant',
  ExampleHostText.trackAudioFallback: 'Audio track',
  ExampleHostText.trackAudioChannels: '{count} ch',
  ExampleHostText.trackAudioProgramFallback: 'Audio program',
  ExampleHostText.trackSubtitleFallback: 'Subtitle track',
  ExampleHostText.trackSubtitleForced: 'Forced',
  ExampleHostText.trackSubtitleDefault: 'Default',
  ExampleHostText.trackSubtitleOptionFallback: 'Subtitle option',
  ExampleHostText.playlistStatusHidden: 'Hidden',
  ExampleHostText.playlistStatusPlaying: 'Playing',
  ExampleHostText.playlistStatusNearby: 'Nearby',
  ExampleHostText.playlistStatusPrefetch: 'Prefetch only',
  ExampleHostText.liveGoLive: 'Go live',
  ExampleHostText.liveBehind: 'Live -{behind}',
  ExampleHostText.live: 'Live',
  ExampleHostText.liveEdgeSummary: 'Live • edge {edge}',
  ExampleHostText.localVideo: 'Local video',
  ExampleHostText.localPathLabel: 'Local path or URI',
  ExampleHostText.sourceLabelHls: 'HLS demo (BipBop)',
  ExampleHostText.sourceLabelDash: 'DASH demo (Envivio)',
  ExampleHostText.sourceLabelLiveDvr: 'Live DVR acceptance (Unified SCTE-35)',
  ExampleHostText.sourceLabelCustomRemote: 'Custom remote stream',
  ExampleHostText.normalizerOff: 'SourceNormalizer off',
  ExampleHostText.normalizerDisabledSubtitle:
      'Does not load a SourceNormalizer plugin for playback probing.',
  ExampleHostText.normalizerDiagnosticsSubtitle:
      'Loads the SourceNormalizer plugin and reports capability only; no '
      'packet session is opened.',
  ExampleHostText.normalizerPreflightSubtitle:
      'Loads the SourceNormalizer plugin, opens and closes a packet session '
      'for the current source, then plays the original source.',
  ExampleHostText.normalizerPreferSubtitle:
      'Tries disk-backed fMP4/HLS normalized playback and falls back to the '
      'original source on failure.',
  ExampleHostText.normalizerRequireSubtitle:
      'Requires disk-backed normalized playback and shows the error on '
      'failure.',
  ExampleHostText.resilienceBalancedSubtitle:
      'General default for most remote streams',
  ExampleHostText.resilienceStreamingSubtitle:
      'Larger buffer window, favors stable playback',
  ExampleHostText.resilienceResilientSubtitle:
      'More retries and cache, for weak networks',
  ExampleHostText.resilienceLowLatencySubtitle:
      'Shorter buffer and cache, favors latency',
  ExampleHostText.loadingController: 'Initializing Vesper Flutter Host...',
  ExampleHostText.controllerInitFailed: 'Controller initialization failed',
  ExampleHostText.sessionOverviewTitle: 'Session overview',
  ExampleHostText.sessionOverviewSubtitle:
      'Current playback route, source, queue origin, and host diagnostics '
      'mode.',
  ExampleHostText.originQueue: 'Queue: {id}',
  ExampleHostText.originDolby: 'Dolby ad hoc: {id}',
  ExampleHostText.eventLogTitle: 'Event log',
  ExampleHostText.eventLogSubtitle:
      'Records host actions in this example app only; does not read Logcat '
      'or native logs.',
  ExampleHostText.eventLogEmpty: 'No host events yet.',
  ExampleHostText.pluginDiagnosticsTitle: 'Plugin diagnostics',
  ExampleHostText.pluginDiagnosticsSubtitle:
      'SourceNormalizer can move from diagnostics/preflight to the '
      'normalized playback route; FrameProcessor records debug capability '
      'diagnostics only.',
  ExampleHostText.pluginNoNormalizerDiagnostics:
      'No SourceNormalizer diagnostics yet.',
  ExampleHostText.pluginNoFrameProcessorDiagnostics:
      'No FrameProcessor debug diagnostics yet.',
  ExampleHostText.hdrEvidenceCapturing: 'Capturing HDR evidence',
  ExampleHostText.hdrEvidenceCapture: 'Capture HDR evidence',
  ExampleHostText.resilienceTitle: 'Resilience policy',
  ExampleHostText.resilienceSubtitle:
      'Demonstrates the resilience policy Flutter API. Switching a profile '
      'applies it to the player and keeps the current media and position '
      'when possible.',
  ExampleHostText.resilienceRetryValue: '{count} attempts · {backoff}',
  ExampleHostText.recentErrorTitle: 'Recent error',
  ExampleHostText.recentErrorSubtitle:
      'Platform errors reach both the snapshot and the event stream.',
  ExampleHostText.recentErrorDolbyP5:
      'This device does not support this Dolby Vision P5 / Widevine '
      'combination.',
  ExampleHostText.recentErrorWidevine:
      'Widevine license or provisioning request failed after {count} '
      'attempts.',
  ExampleHostText.recentErrorFairPlay:
      'FairPlay license or certificate request failed after {count} '
      'attempts.',
  ExampleHostText.recentErrorCode: 'Error code',
  ExampleHostText.systemPlaybackSubtitle:
      'Host integration for background audio, lock screen controls, '
      'AirPlay, Android Cast, and DLNA.',
  ExampleHostText.pipEnable: 'Enable Picture in Picture',
  ExampleHostText.notificationPermission: 'Notification permission: {status}',
  ExampleHostText.rescanDlna: 'Rescan DLNA',
  ExampleHostText.pipActive: 'Picture in Picture active',
  ExampleHostText.pipEntering: 'Entering Picture in Picture',
  ExampleHostText.pipOff: 'Picture in Picture off',
  ExampleHostText.pipUnavailable:
      'Picture in Picture is unavailable for this playback',
  ExampleHostText.pipRequest: 'Picture in Picture',
  ExampleHostText.dolbyTitle: 'Dolby acceptance',
  ExampleHostText.dolbySubtitle:
      'Public Dolby Vision Online Delivery Kit signals, direct native '
      'playback only; never routed through downloads, preload, external '
      'playback, or plugins.',
  ExampleHostText.playNow: 'Play now',
  ExampleHostText.addToQueue: 'Add to queue',
  ExampleHostText.queuePanelSubtitle:
      'Only sources queued for continuous playback appear here. Dolby '
      'presets stay in the library unless added manually.',
  ExampleHostText.queuePanelTitle: 'Up next',
  ExampleHostText.queueNoSources: 'No sources in the queue yet.',
  ExampleHostText.sourceSectionTitle: 'Media sources',
  ExampleHostText.sourceSectionSubtitle:
      'Switch between local files, HLS, DASH, and custom remote URLs with '
      'these demo actions.',
  ExampleHostText.itemCountOne: '1 item',
  ExampleHostText.itemCountOther: '{count} items',
  ExampleHostText.downloadHeaderSubtitle:
      'This page is for download session regression: it verifies the shared '
      'download state machine and host bridge, not a full offline product '
      'flow.',
  ExampleHostText.downloadCreateTitle: 'Create download task',
  ExampleHostText.downloadCreateSubtitle:
      'Use the HLS and DASH demo buttons for quick regression, or paste a '
      'remote URL to create a foreground download task.',
  ExampleHostText.downloadTasksTitle: 'Tasks',
  ExampleHostText.downloadTasksSubtitle:
      'This example stays at foreground execution and task lifecycle '
      'regression; it does not claim background resume, offline DRM, or '
      'fully packaged HLS/DASH product capabilities.',
  ExampleHostText.downloadTasksEmpty: 'No download tasks yet.',
  ExampleHostText.downloadExportInstalled: 'MP4 export library: installed',
  ExampleHostText.downloadExportMissing: 'MP4 export library not installed.',
  ExampleHostText.downloadHlsDemo: 'Download HLS demo',
  ExampleHostText.downloadDashDemo: 'Download DASH demo',
  ExampleHostText.downloadUrlLabel: 'Download URL',
  ExampleHostText.downloadCreateRemote: 'Create remote task',
  ExampleHostText.downloadPlanning:
      'Reading the remote manifest and building a download plan…',
  ExampleHostText.downloadRemoteAddress: 'Remote URL: {uri}',
  ExampleHostText.downloadExporting: 'Exporting MP4… {percent}%',
  ExampleHostText.downloadExportingShort: 'Exporting MP4',
  ExampleHostText.downloadOutputPath: 'Output: {path}',
  ExampleHostText.downloadError: 'Error: {message}',
  ExampleHostText.downloadSaveToGallery: 'Save to gallery',
  ExampleHostText.downloadRemove: 'Remove',
  ExampleHostText.downloadStateQueued: 'Queued',
  ExampleHostText.downloadStatePreparing: 'Preparing',
  ExampleHostText.downloadStateDownloading: 'Downloading',
  ExampleHostText.downloadStatePaused: 'Paused',
  ExampleHostText.downloadStateCompleted: 'Completed',
  ExampleHostText.downloadStateFailed: 'Failed',
  ExampleHostText.downloadStateRemoved: 'Removed',
  ExampleHostText.downloadStateUnknown: 'Unknown state',
  ExampleHostText.downloadActionStart: 'Start',
  ExampleHostText.downloadActionPause: 'Pause',
  ExampleHostText.downloadActionResume: 'Resume',
  ExampleHostText.downloadProgressUnknown: 'Progress unknown',
  ExampleHostText.downloadPrepareFailed:
      'Could not prepare the download task: {error}',
  ExampleHostText.downloadCreateFailed: 'Could not create the download task.',
  ExampleHostText.downloadUrlEmpty: 'Enter a download URL.',
  ExampleHostText.downloadActionFailed: 'Download task action failed.',
  ExampleHostText.downloadOutputMissing:
      'The completed task has no output file.',
  ExampleHostText.downloadSavedToGallery: 'Saved to the system gallery.',
  ExampleHostText.downloadGalleryUnsupported:
      'This host does not support gallery export yet.',
  ExampleHostText.downloadGalleryFailed:
      'Could not save to the system gallery.',
  ExampleHostText.externalUnavailable:
      'External playback is not available right now.',
  ExampleHostText.externalRescanning: 'Rescanning DLNA devices.',
  ExampleHostText.externalDevice: 'device',
  ExampleHostText.externalConnected: 'External playback connected: {route}',
  ExampleHostText.externalDisconnected:
      'External playback disconnected; local playback resumed.',
  ExampleHostText.externalLoadedMedia: 'External playback media loaded.',
  ExampleHostText.externalPlaying: 'External playback resumed.',
  ExampleHostText.externalPaused: 'External playback paused.',
  ExampleHostText.externalStopped: 'External playback stopped.',
  ExampleHostText.externalSuspended: 'External playback connection suspended.',
  ExampleHostText.externalError: 'External playback error.',
  ExampleHostText.externalConnecting: 'Connecting external playback: {route}',
  ExampleHostText.externalLoadedRoute: 'External playback loaded: {route}',
  ExampleHostText.externalDiagnostic: 'External playback diagnostic event.',
  ExampleHostText.dolbyDashUnsupported:
      'This platform host does not support DASH Dolby acceptance streams yet.',
  ExampleHostText.dolbyQueueUnavailable:
      'This Dolby preset cannot be queued yet.',
  ExampleHostText.dolbyPluginsKeptOff:
      'Dolby Vision acceptance streams need direct native playback; plugins '
      'stay off.',
  ExampleHostText.dolbyPluginsTurnedOff:
      'Dolby Vision acceptance switched back to direct native playback; '
      'plugin selection is off.',
  ExampleHostText.normalizerSwitchFailed:
      'Could not switch the SourceNormalizer setting: {error}',
  ExampleHostText.hdrEvidenceNeedsSource:
      'Select a local file or remote URL before capturing this HDR '
      'evidence sample.',
  ExampleHostText.hdrEvidenceWritten: 'HDR evidence bundle written: {path}',
  ExampleHostText.hdrEvidenceFailed: 'HDR evidence capture failed: {error}',
  ExampleHostText.hdrEvidenceConfirm: 'Confirm {label} evidence metadata',
  ExampleHostText.hdrEvidenceStart: 'Start capture',
  ExampleHostText.logDownloadCreateFailed: 'Download task creation failed',
  ExampleHostText.logExternalUnavailable: 'External playback unavailable',
  ExampleHostText.logExternalDisconnected: 'External playback disconnected',
  ExampleHostText.logExternalError: 'External playback error',
  ExampleHostText.logExternalUnknownEvent: 'Unknown external playback event',
  ExampleHostText.logSourceSelected: 'Source selected',
  ExampleHostText.logDolbySelected: 'Dolby preset selected',
  ExampleHostText.logDolbyQueued: 'Dolby preset queued',
  ExampleHostText.logPluginsKeptOff: 'Plugin mode kept off',
  ExampleHostText.logPluginsKeptOffDetail:
      'Dolby Vision acceptance streams bypass SourceNormalizer, FFmpeg, and '
      'FrameProcessor.',
  ExampleHostText.logPluginModeChanged: 'Plugin mode changed',
  ExampleHostText.logResilienceChanged: 'Resilience profile changed',
  ExampleHostText.logHdrEvidenceResult: 'HDR evidence result',
  ExampleHostText.logDolbyDirectDetail:
      'SourceNormalizer, FFmpeg, and FrameProcessor are off.',
};
