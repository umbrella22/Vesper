import 'package:flutter/widgets.dart';

import 'example_host_strings_en.dart';
import 'example_host_strings_zh.dart';

/// Keys for host-owned shell copy. Stage copy belongs to `vesper_player_ui`.
enum ExampleHostText {
  appTitle,
  tabPlay,
  tabLibrary,
  tabDownloads,
  tabLab,
  settings,
  settingsAppearance,
  settingsAbout,
  themeSystem,
  themeLight,
  themeDark,
  back,
  close,
  undo,
  openMedia,
  openMediaSubtitle,
  openMediaPickLocal,
  openMediaPickLocalSubtitle,
  openMediaPickLocalUnavailable,
  openMediaStreamUrl,
  openMediaStreamUrlHint,
  openMediaOpenUrl,
  openMediaQuickSamples,
  openMediaFailed,
  sampleHlsTitle,
  sampleHlsSubtitle,
  sampleDashTitle,
  sampleDashSubtitle,
  sampleLiveDvrTitle,
  sampleLiveDvrSubtitle,
  dashUnavailable,
  urlErrorEmpty,
  urlErrorInvalid,
  urlErrorDashUnsupported,
  nowPlaying,
  nothingPlaying,
  quickPictureInPicture,
  quickCast,
  quickTracks,
  upNext,
  manageQueue,
  queueEmptyTitle,
  queueEmptyBody,
  goToLibrary,
  castSheetTitle,
  libraryCollections,
  librarySubtitle,
  collectionVodTitle,
  collectionVodSummary,
  collectionLiveTitle,
  collectionLiveSummary,
  collectionAdaptiveTitle,
  collectionAdaptiveSummary,
  collectionDolbyTitle,
  collectionDolbySummary,
  libraryAddedToQueue,
  downloadsTasks,
  downloadsNew,
  downloadsNewSubtitle,
  downloadsEmptyTitle,
  downloadsEmptyBody,
  downloadsScopeNote,
  downloadRemoved,
  labSubtitle,
  labSystemPlayback,
  busyApplyingPolicy,
  busySwitchingPlugin,
  // Common.
  cancel,
  open,
  none,
  auto,
  notSelected,
  detailLine,
  listSeparator,
  // Player tool sheet.
  sheetMenuTitle,
  sheetMenuSubtitle,
  sheetQualityTitle,
  sheetQualitySubtitle,
  sheetAudioTitle,
  sheetAudioSubtitle,
  sheetSubtitleTitle,
  sheetSubtitleSubtitle,
  sheetSpeedTitle,
  sheetSpeedSubtitle,
  sheetNoAdaptiveVideo,
  sheetNoVideoTracks,
  sheetNoFixedTrack,
  sheetAudioAutoSubtitle,
  sheetNoAudioTracks,
  sheetSubtitleOff,
  sheetSubtitleOffSubtitle,
  sheetSubtitleAutoSubtitle,
  sheetNoSubtitleTracks,
  sheetSpeedActive,
  sheetSpeedApply,
  // Quality and track labels.
  qualityLimited,
  qualityLockingValue,
  qualityLockedValue,
  qualityAutoValue,
  qualityLeadAuto,
  qualityLeadConstrainedWithin,
  qualityLeadConstrained,
  qualityLeadFallback,
  qualityLeadPending,
  qualityLeadLocked,
  qualityLeadRequested,
  qualityLeadSwitchToAuto,
  qualityObserved,
  qualityEffective,
  qualityStateLocked,
  qualityStatePending,
  qualityStateRequested,
  qualityStateStillHere,
  qualityBadgeCurrent,
  qualityBadgeWaiting,
  qualityBadgeLocking,
  qualityBadgeActual,
  abrMaxHeight,
  abrMaxWidth,
  abrMaxBitRate,
  qualityBestEffortNotice,
  qualityFellBackConstrained,
  qualityFellBackAuto,
  qualityLockNotConverged,
  qualityLockNotice,
  subtitleOffValue,
  subtitleAutoValue,
  trackVideoFallback,
  trackVideoVariantFallback,
  trackAudioFallback,
  trackAudioChannels,
  trackAudioProgramFallback,
  trackSubtitleFallback,
  trackSubtitleForced,
  trackSubtitleDefault,
  trackSubtitleOptionFallback,
  playlistStatusHidden,
  playlistStatusPlaying,
  playlistStatusNearby,
  playlistStatusPrefetch,
  liveGoLive,
  liveBehind,
  live,
  liveEdgeSummary,
  localVideo,
  localPathLabel,
  // Sources, plugin and resilience settings.
  sourceLabelHls,
  sourceLabelDash,
  sourceLabelLiveDvr,
  sourceLabelCustomRemote,
  normalizerOff,
  normalizerDisabledSubtitle,
  normalizerDiagnosticsSubtitle,
  normalizerPreflightSubtitle,
  normalizerPreferSubtitle,
  normalizerRequireSubtitle,
  resilienceBalancedSubtitle,
  resilienceStreamingSubtitle,
  resilienceResilientSubtitle,
  resilienceLowLatencySubtitle,
  // Lab sections and queue panel.
  loadingController,
  controllerInitFailed,
  sessionOverviewTitle,
  sessionOverviewSubtitle,
  originQueue,
  originDolby,
  eventLogTitle,
  eventLogSubtitle,
  eventLogEmpty,
  pluginDiagnosticsTitle,
  pluginDiagnosticsSubtitle,
  pluginNoNormalizerDiagnostics,
  pluginNoFrameProcessorDiagnostics,
  hdrEvidenceCapturing,
  hdrEvidenceCapture,
  resilienceTitle,
  resilienceSubtitle,
  resilienceRetryValue,
  recentErrorTitle,
  recentErrorSubtitle,
  recentErrorDolbyP5,
  recentErrorWidevine,
  recentErrorFairPlay,
  recentErrorCode,
  systemPlaybackSubtitle,
  pipEnable,
  notificationPermission,
  rescanDlna,
  pipActive,
  pipEntering,
  pipOff,
  pipUnavailable,
  pipRequest,
  dolbyTitle,
  dolbySubtitle,
  playNow,
  addToQueue,
  queuePanelSubtitle,
  queuePanelTitle,
  queueNoSources,
  sourceSectionTitle,
  sourceSectionSubtitle,
  itemCountOne,
  itemCountOther,
  // Downloads.
  downloadHeaderSubtitle,
  downloadCreateTitle,
  downloadCreateSubtitle,
  downloadTasksTitle,
  downloadTasksSubtitle,
  downloadTasksEmpty,
  downloadExportInstalled,
  downloadExportMissing,
  downloadHlsDemo,
  downloadDashDemo,
  downloadUrlLabel,
  downloadCreateRemote,
  downloadPlanning,
  downloadRemoteAddress,
  downloadExporting,
  downloadExportingShort,
  downloadOutputPath,
  downloadError,
  downloadSaveToGallery,
  downloadRemove,
  downloadStateQueued,
  downloadStatePreparing,
  downloadStateDownloading,
  downloadStatePaused,
  downloadStateCompleted,
  downloadStateFailed,
  downloadStateRemoved,
  downloadStateUnknown,
  downloadActionStart,
  downloadActionPause,
  downloadActionResume,
  downloadProgressUnknown,
  downloadPrepareFailed,
  downloadCreateFailed,
  downloadUrlEmpty,
  downloadActionFailed,
  downloadOutputMissing,
  downloadSavedToGallery,
  downloadGalleryUnsupported,
  downloadGalleryFailed,
  // Host messages (SnackBar and inline status).
  externalUnavailable,
  externalRescanning,
  externalDevice,
  externalConnected,
  externalDisconnected,
  externalLoadedMedia,
  externalPlaying,
  externalPaused,
  externalStopped,
  externalSuspended,
  externalError,
  externalConnecting,
  externalLoadedRoute,
  externalDiagnostic,
  dolbyDashUnsupported,
  dolbyQueueUnavailable,
  dolbyPluginsKeptOff,
  dolbyPluginsTurnedOff,
  normalizerSwitchFailed,
  hdrEvidenceNeedsSource,
  hdrEvidenceWritten,
  hdrEvidenceFailed,
  hdrEvidenceConfirm,
  hdrEvidenceStart,
  // Host event log entries shown in the Lab tab.
  logDownloadCreateFailed,
  logExternalUnavailable,
  logExternalDisconnected,
  logExternalError,
  logExternalUnknownEvent,
  logSourceSelected,
  logDolbySelected,
  logDolbyQueued,
  logPluginsKeptOff,
  logPluginsKeptOffDetail,
  logPluginModeChanged,
  logResilienceChanged,
  logHdrEvidenceResult,
  logDolbyDirectDetail,
}

enum ExampleHostLanguage { en, zh }

/// Resolved host strings for one language.
final class ExampleHostStrings {
  const ExampleHostStrings._(this.language, this._values);

  static const ExampleHostStrings en = ExampleHostStrings._(
    ExampleHostLanguage.en,
    exampleHostStringsEn,
  );
  static const ExampleHostStrings zh = ExampleHostStrings._(
    ExampleHostLanguage.zh,
    exampleHostStringsZh,
  );

  final ExampleHostLanguage language;
  final Map<ExampleHostText, String> _values;

  factory ExampleHostStrings.forLocale(Locale? locale) {
    return locale?.languageCode == 'zh' ? zh : en;
  }

  /// Returns the copy for [key], falling back to English and then the key.
  String operator [](ExampleHostText key) {
    return _values[key] ?? exampleHostStringsEn[key] ?? key.name;
  }

  /// Returns the copy for [key] with `{name}` placeholders replaced by [args].
  String format(ExampleHostText key, Map<String, Object?> args) {
    var value = this[key];
    args.forEach((name, arg) {
      value = value.replaceAll('{$name}', '${arg ?? ''}');
    });
    return value;
  }

  /// Item count with the correct plural form for the active language.
  String itemCount(int count) {
    return format(
      count == 1
          ? ExampleHostText.itemCountOne
          : ExampleHostText.itemCountOther,
      <String, Object?>{'count': count},
    );
  }

  static ExampleHostStrings of(BuildContext context) {
    final scope = context
        .dependOnInheritedWidgetOfExactType<ExampleHostStringsScope>();
    return scope?.strings ?? en;
  }
}

class ExampleHostStringsScope extends InheritedWidget {
  const ExampleHostStringsScope({
    super.key,
    required this.strings,
    required super.child,
  });

  final ExampleHostStrings strings;

  @override
  bool updateShouldNotify(ExampleHostStringsScope oldWidget) {
    return !identical(strings, oldWidget.strings);
  }
}
