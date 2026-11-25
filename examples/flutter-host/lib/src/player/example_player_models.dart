import 'package:material_ui/material_ui.dart';
import 'package:vesper_player/vesper_player.dart';

import '../shell/example_host_strings.dart';

export '../shell/example_host_navigation.dart'
    show ExampleHostTab, ExampleLibraryCollectionId;

enum ExamplePlayerSheet { menu, quality, audio, subtitle, speed }

sealed class ExamplePlaybackOrigin {
  const ExamplePlaybackOrigin();
}

final class ExampleQueuePlaybackOrigin extends ExamplePlaybackOrigin {
  const ExampleQueuePlaybackOrigin(this.itemId);

  final String itemId;
}

final class ExampleDolbyAdHocPlaybackOrigin extends ExamplePlaybackOrigin {
  const ExampleDolbyAdHocPlaybackOrigin(this.presetId);

  final String presetId;
}

enum ExampleHostLogSeverity { info, warning, error }

final class ExampleHostLogEntry {
  const ExampleHostLogEntry({
    required this.id,
    required this.atMillis,
    required this.severity,
    required this.title,
    this.detail,
  });

  final int id;
  final int atMillis;
  final ExampleHostLogSeverity severity;
  final String title;
  final String? detail;
}

enum ExampleThemeMode { system, light, dark }

extension ExampleThemeModeLabels on ExampleThemeMode {
  String title([ExampleHostStrings strings = ExampleHostStrings.zh]) {
    return strings[switch (this) {
      ExampleThemeMode.system => ExampleHostText.themeSystem,
      ExampleThemeMode.light => ExampleHostText.themeLight,
      ExampleThemeMode.dark => ExampleHostText.themeDark,
    }];
  }
}

enum ExampleResilienceProfile { balanced, streaming, resilient, lowLatency }

enum ExampleSourceNormalizerSetting {
  disabled,
  diagnosticsOnly,
  preflightOnly,
  preferNormalized,
  requireNormalized,
}

extension ExampleSourceNormalizerSettingLabels
    on ExampleSourceNormalizerSetting {
  String title([ExampleHostStrings strings = ExampleHostStrings.zh]) {
    return switch (this) {
      ExampleSourceNormalizerSetting.disabled =>
        strings[ExampleHostText.normalizerOff],
      ExampleSourceNormalizerSetting.diagnosticsOnly => 'Diagnostics',
      ExampleSourceNormalizerSetting.preflightOnly => 'Preflight',
      ExampleSourceNormalizerSetting.preferNormalized => 'Prefer normalized',
      ExampleSourceNormalizerSetting.requireNormalized => 'Require normalized',
    };
  }

  String subtitle([ExampleHostStrings strings = ExampleHostStrings.zh]) {
    return strings[switch (this) {
      ExampleSourceNormalizerSetting.disabled =>
        ExampleHostText.normalizerDisabledSubtitle,
      ExampleSourceNormalizerSetting.diagnosticsOnly =>
        ExampleHostText.normalizerDiagnosticsSubtitle,
      ExampleSourceNormalizerSetting.preflightOnly =>
        ExampleHostText.normalizerPreflightSubtitle,
      ExampleSourceNormalizerSetting.preferNormalized =>
        ExampleHostText.normalizerPreferSubtitle,
      ExampleSourceNormalizerSetting.requireNormalized =>
        ExampleHostText.normalizerRequireSubtitle,
    }];
  }

  VesperSourceNormalizerMode get mode {
    return switch (this) {
      ExampleSourceNormalizerSetting.disabled =>
        VesperSourceNormalizerMode.disabled,
      ExampleSourceNormalizerSetting.diagnosticsOnly =>
        VesperSourceNormalizerMode.diagnosticsOnly,
      ExampleSourceNormalizerSetting.preflightOnly =>
        VesperSourceNormalizerMode.preflightOnly,
      ExampleSourceNormalizerSetting.preferNormalized =>
        VesperSourceNormalizerMode.preferNormalized,
      ExampleSourceNormalizerSetting.requireNormalized =>
        VesperSourceNormalizerMode.requireNormalized,
    };
  }
}

enum ExampleSheetNoteTone { info, warm }

final class ExampleSheetNoticeModel {
  const ExampleSheetNoticeModel({
    this.title,
    required this.message,
    this.tone = ExampleSheetNoteTone.info,
  });

  final String? title;
  final String message;
  final ExampleSheetNoteTone tone;
}

extension ExampleResilienceProfileLabels on ExampleResilienceProfile {
  String get title {
    return switch (this) {
      ExampleResilienceProfile.balanced => 'Balanced',
      ExampleResilienceProfile.streaming => 'Streaming',
      ExampleResilienceProfile.resilient => 'Resilient',
      ExampleResilienceProfile.lowLatency => 'Low Latency',
    };
  }

  String subtitle([ExampleHostStrings strings = ExampleHostStrings.zh]) {
    return strings[switch (this) {
      ExampleResilienceProfile.balanced =>
        ExampleHostText.resilienceBalancedSubtitle,
      ExampleResilienceProfile.streaming =>
        ExampleHostText.resilienceStreamingSubtitle,
      ExampleResilienceProfile.resilient =>
        ExampleHostText.resilienceResilientSubtitle,
      ExampleResilienceProfile.lowLatency =>
        ExampleHostText.resilienceLowLatencySubtitle,
    }];
  }

  VesperPlaybackResiliencePolicy get policy {
    return switch (this) {
      ExampleResilienceProfile.balanced =>
        const VesperPlaybackResiliencePolicy.balanced(),
      ExampleResilienceProfile.streaming =>
        const VesperPlaybackResiliencePolicy.streaming(),
      ExampleResilienceProfile.resilient =>
        const VesperPlaybackResiliencePolicy.resilient(),
      ExampleResilienceProfile.lowLatency =>
        const VesperPlaybackResiliencePolicy.lowLatency(),
    };
  }

  static ExampleResilienceProfile? fromPolicy(
    VesperPlaybackResiliencePolicy policy,
  ) {
    for (final profile in ExampleResilienceProfile.values) {
      final candidate = profile.policy;
      if (_matchesBufferingPolicy(candidate.buffering, policy.buffering) &&
          _matchesRetryPolicy(candidate.retry, policy.retry) &&
          _matchesCachePolicy(candidate.cache, policy.cache)) {
        return profile;
      }
    }
    return null;
  }

  static bool _matchesBufferingPolicy(
    VesperBufferingPolicy lhs,
    VesperBufferingPolicy rhs,
  ) {
    return lhs.preset == rhs.preset &&
        lhs.minBufferMs == rhs.minBufferMs &&
        lhs.maxBufferMs == rhs.maxBufferMs &&
        lhs.bufferForPlaybackMs == rhs.bufferForPlaybackMs &&
        lhs.bufferForPlaybackAfterRebufferMs ==
            rhs.bufferForPlaybackAfterRebufferMs;
  }

  static bool _matchesRetryPolicy(
    VesperRetryPolicy lhs,
    VesperRetryPolicy rhs,
  ) {
    return lhs.maxAttempts == rhs.maxAttempts &&
        lhs.baseDelayMs == rhs.baseDelayMs &&
        lhs.maxDelayMs == rhs.maxDelayMs &&
        lhs.backoff == rhs.backoff;
  }

  static bool _matchesCachePolicy(
    VesperCachePolicy lhs,
    VesperCachePolicy rhs,
  ) {
    return lhs.preset == rhs.preset &&
        lhs.maxMemoryBytes == rhs.maxMemoryBytes &&
        lhs.maxDiskBytes == rhs.maxDiskBytes;
  }
}

final class ExampleHostPalette {
  const ExampleHostPalette({
    required this.pageTop,
    required this.pageBottom,
    required this.sectionBackground,
    required this.sectionStroke,
    required this.title,
    required this.body,
    required this.fieldBackground,
    required this.fieldText,
    required this.primaryAction,
    required this.onPrimaryAction,
    required this.accent,
    required this.onAccent,
    required this.accentInk,
    required this.surfaceVariant,
    required this.outline,
    required this.error,
    required this.stage,
    required this.stageForeground,
  });

  final Color pageTop;
  final Color pageBottom;
  final Color sectionBackground;
  final Color sectionStroke;
  final Color title;
  final Color body;
  final Color fieldBackground;
  final Color fieldText;

  /// Neutral filled-button color (prototype `--ink`). The amber [accent] is
  /// reserved for the open-media FAB and the selected navigation indicator so
  /// each screen keeps at most two prominent accents.
  final Color primaryAction;
  final Color onPrimaryAction;
  final Color accent;
  final Color onAccent;
  final Color accentInk;
  final Color surfaceVariant;
  final Color outline;
  final Color error;
  final Color stage;
  final Color stageForeground;
}

/// Tokens taken from `Vesper示例-Android-可交互原型.html` (`:root` and
/// `html[data-theme="dark"]`).
ExampleHostPalette exampleHostPalette(bool useDarkTheme) {
  if (useDarkTheme) {
    return const ExampleHostPalette(
      pageTop: Color(0xFF0E1017),
      pageBottom: Color(0xFF0E1017),
      sectionBackground: Color(0xFF171A23),
      sectionStroke: Color(0x1AECEAE4),
      title: Color(0xFFECEAE4),
      body: Color(0xFF9B9EAA),
      fieldBackground: Color(0xFF20242F),
      fieldText: Color(0xFFECEAE4),
      primaryAction: Color(0xFFECEAE4),
      onPrimaryAction: Color(0xFF0E1017),
      accent: Color(0xFFE8A93A),
      onAccent: Color(0xFF1B1406),
      accentInk: Color(0xFFF0BC5E),
      surfaceVariant: Color(0xFF20242F),
      outline: Color(0xFF6E7280),
      error: Color(0xFFD2463C),
      stage: Color(0xFF0B0D16),
      stageForeground: Color(0xFFF4F2EC),
    );
  }

  return const ExampleHostPalette(
    pageTop: Color(0xFFF5F3EE),
    pageBottom: Color(0xFFF5F3EE),
    sectionBackground: Color(0xFFFFFFFF),
    sectionStroke: Color(0x1A17181E),
    title: Color(0xFF17181E),
    body: Color(0xFF5C5F6B),
    fieldBackground: Color(0xFFECE9E2),
    fieldText: Color(0xFF17181E),
    primaryAction: Color(0xFF17181E),
    onPrimaryAction: Color(0xFFF5F3EE),
    accent: Color(0xFFE8A93A),
    onAccent: Color(0xFF1B1406),
    accentInk: Color(0xFF8C5B00),
    surfaceVariant: Color(0xFFECE9E2),
    outline: Color(0xFF8A8D98),
    error: Color(0xFFD2463C),
    stage: Color(0xFF0B0D16),
    stageForeground: Color(0xFFF4F2EC),
  );
}

const String flutterHlsDemoUrl =
    'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8';

const String flutterDashDemoUrl =
    'https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd';
const String flutterLiveDvrAcceptanceUrl =
    'https://demo.unified-streaming.com/k8s/live/scte35.isml/.m3u8';
const String flutterHlsPlaylistItemId = 'hls-demo';
const String flutterDashPlaylistItemId = 'dash-demo';
const String flutterLiveDvrPlaylistItemId = 'live-dvr-acceptance';
const String flutterRemotePlaylistItemId = 'custom-remote';
const String flutterLocalPlaylistItemId = 'local-file';
const int exampleHostLogCapacity = 80;
const String _dolbyPlaylistItemPrefix = 'dolby-';

String flutterDolbyPlaylistItemId(String presetId) {
  return '$_dolbyPlaylistItemPrefix$presetId';
}

String? flutterDolbyPresetIdFromPlaylistItemId(String itemId) {
  if (!itemId.startsWith(_dolbyPlaylistItemPrefix)) {
    return null;
  }
  final presetId = itemId.substring(_dolbyPlaylistItemPrefix.length);
  return presetId.isEmpty ? null : presetId;
}

const List<ExampleSource> exampleSources = <ExampleSource>[
  ExampleSource(
    titleKey: ExampleHostText.sampleHlsTitle,
    subtitleKey: ExampleHostText.sampleHlsSubtitle,
    uri: flutterHlsDemoUrl,
    protocol: VesperPlayerSourceProtocol.hls,
  ),
  ExampleSource(
    titleKey: ExampleHostText.sampleDashTitle,
    subtitleKey: ExampleHostText.sampleDashSubtitle,
    uri: flutterDashDemoUrl,
    protocol: VesperPlayerSourceProtocol.dash,
  ),
  ExampleSource(
    titleKey: ExampleHostText.sampleLiveDvrTitle,
    subtitleKey: ExampleHostText.sampleLiveDvrSubtitle,
    uri: flutterLiveDvrAcceptanceUrl,
    protocol: VesperPlayerSourceProtocol.hls,
  ),
];

final class ExampleSource {
  const ExampleSource({
    required this.titleKey,
    required this.subtitleKey,
    required this.uri,
    required this.protocol,
  });

  final ExampleHostText titleKey;
  final ExampleHostText subtitleKey;
  final String uri;
  final VesperPlayerSourceProtocol protocol;

  String title([ExampleHostStrings strings = ExampleHostStrings.zh]) {
    return strings[titleKey];
  }

  String subtitle([ExampleHostStrings strings = ExampleHostStrings.zh]) {
    return strings[subtitleKey];
  }

  VesperPlayerSource toPlayerSource([
    ExampleHostStrings strings = ExampleHostStrings.zh,
  ]) {
    return VesperPlayerSource.remote(
      uri: uri,
      label: title(strings),
      protocol: protocol,
    );
  }
}

final class ExamplePlaylistItemViewData {
  const ExamplePlaylistItemViewData({
    required this.itemId,
    required this.label,
    required this.status,
    required this.isActive,
  });

  final String itemId;
  final String label;
  final String status;
  final bool isActive;
}

List<String> enqueuePlaylistItem(List<String> playlistItemIds, String itemId) {
  return <String>[
    ...playlistItemIds.where((existingItemId) => existingItemId != itemId),
    itemId,
  ];
}

List<ExampleHostLogEntry> appendExampleHostLogEntry(
  List<ExampleHostLogEntry> entries,
  ExampleHostLogEntry entry, {
  int capacity = exampleHostLogCapacity,
}) {
  if (capacity <= 0) {
    return const <ExampleHostLogEntry>[];
  }
  return <ExampleHostLogEntry>[entry, ...entries].take(capacity).toList();
}

bool shouldAdvancePlaylistOnFinished({
  required ExamplePlaybackOrigin? origin,
  required String? activeItemId,
}) {
  return origin is ExampleQueuePlaybackOrigin &&
      activeItemId != null &&
      origin.itemId == activeItemId;
}

String? nextPlaylistItemIdOnFinished({
  required List<String> playlistItemIds,
  required String? activeItemId,
}) {
  if (activeItemId == null) {
    return null;
  }
  final activeIndex = playlistItemIds.indexOf(activeItemId);
  if (activeIndex < 0 || activeIndex + 1 >= playlistItemIds.length) {
    return null;
  }
  return playlistItemIds[activeIndex + 1];
}

VesperPlayerSource flutterHlsDemoSource([
  ExampleHostStrings strings = ExampleHostStrings.zh,
]) {
  return VesperPlayerSource.hls(
    uri: flutterHlsDemoUrl,
    label: strings[ExampleHostText.sourceLabelHls],
  );
}

VesperPlayerSource flutterDashDemoSource([
  ExampleHostStrings strings = ExampleHostStrings.zh,
]) {
  return VesperPlayerSource.dash(
    uri: flutterDashDemoUrl,
    label: strings[ExampleHostText.sourceLabelDash],
  );
}

VesperPlayerSource flutterLiveDvrAcceptanceSource([
  ExampleHostStrings strings = ExampleHostStrings.zh,
]) {
  return VesperPlayerSource.hls(
    uri: flutterLiveDvrAcceptanceUrl,
    label: strings[ExampleHostText.sourceLabelLiveDvr],
  );
}
