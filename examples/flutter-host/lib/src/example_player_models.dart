import 'package:flutter/material.dart';
import 'package:vesper_player/vesper_player.dart';

enum ExamplePlayerSheet { menu, quality, audio, subtitle, speed }

enum ExampleThemeMode { system, light, dark }

extension ExampleThemeModeLabels on ExampleThemeMode {
  String get title {
    return switch (this) {
      ExampleThemeMode.system => '跟随系统',
      ExampleThemeMode.light => '浅色',
      ExampleThemeMode.dark => '深色',
    };
  }
}

enum ExampleResilienceProfile { balanced, streaming, resilient, lowLatency }

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

  String get subtitle {
    return switch (this) {
      ExampleResilienceProfile.balanced => '通用默认，适合大多数远程流',
      ExampleResilienceProfile.streaming => '拉大缓冲窗口，优先稳定播放',
      ExampleResilienceProfile.resilient => '高重试与高缓存，适合弱网回放',
      ExampleResilienceProfile.lowLatency => '缩短缓冲和缓存，优先追求时延',
    };
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
  });

  final Color pageTop;
  final Color pageBottom;
  final Color sectionBackground;
  final Color sectionStroke;
  final Color title;
  final Color body;
  final Color fieldBackground;
  final Color fieldText;
  final Color primaryAction;
}

ExampleHostPalette exampleHostPalette(bool useDarkTheme) {
  if (useDarkTheme) {
    return const ExampleHostPalette(
      pageTop: Color(0xFF0C1018),
      pageBottom: Color(0xFF06080D),
      sectionBackground: Color(0x0AFFFFFF),
      sectionStroke: Color(0x0FFFFFFF),
      title: Colors.white,
      body: Color(0xFF94A0B5),
      fieldBackground: Color(0x0FFFFFFF),
      fieldText: Colors.white,
      primaryAction: Color(0xFF2A8BFF),
    );
  }

  return const ExampleHostPalette(
    pageTop: Color(0xFFF8F2EA),
    pageBottom: Color(0xFFF2F4F9),
    sectionBackground: Color(0xDBFFFFFF),
    sectionStroke: Color(0x140B1220),
    title: Color(0xFF101521),
    body: Color(0xFF5C667A),
    fieldBackground: Color(0xFFF6F7FA),
    fieldText: Color(0xFF101521),
    primaryAction: Color(0xFF256DFF),
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

const List<ExampleSource> exampleSources = <ExampleSource>[
  ExampleSource(
    title: 'HLS 演示',
    subtitle: 'Apple BipBop，自适应码率',
    uri: flutterHlsDemoUrl,
    protocol: VesperPlayerSourceProtocol.hls,
  ),
  ExampleSource(
    title: 'DASH 演示',
    subtitle: 'Envivio，多清晰度清单',
    uri: flutterDashDemoUrl,
    protocol: VesperPlayerSourceProtocol.dash,
  ),
  ExampleSource(
    title: 'Live DVR 验收',
    subtitle: 'Unified SCTE-35 公开 HLS，手测 Go Live 与 DVR slider',
    uri: flutterLiveDvrAcceptanceUrl,
    protocol: VesperPlayerSourceProtocol.hls,
  ),
];

final class ExampleSource {
  const ExampleSource({
    required this.title,
    required this.subtitle,
    required this.uri,
    required this.protocol,
  });

  final String title;
  final String subtitle;
  final String uri;
  final VesperPlayerSourceProtocol protocol;

  VesperPlayerSource toPlayerSource() {
    return VesperPlayerSource.remote(
      uri: uri,
      label: title,
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

VesperPlayerSource flutterHlsDemoSource() {
  return VesperPlayerSource.hls(
    uri: flutterHlsDemoUrl,
    label: 'HLS 演示（BipBop）',
  );
}

VesperPlayerSource flutterDashDemoSource() {
  return VesperPlayerSource.dash(
    uri: flutterDashDemoUrl,
    label: 'DASH 演示（Envivio）',
  );
}

VesperPlayerSource flutterLiveDvrAcceptanceSource() {
  return VesperPlayerSource.hls(
    uri: flutterLiveDvrAcceptanceUrl,
    label: 'Live DVR 验收（Unified SCTE-35）',
  );
}
