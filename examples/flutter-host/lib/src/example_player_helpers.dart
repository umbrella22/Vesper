import 'package:vesper_player/vesper_player.dart';

import 'example_player_models.dart';

VesperPlayerSourceProtocol inferProtocol(String uri) {
  final normalized = uri.toLowerCase();
  final withoutQuery = normalized.split('#').first.split('?').first;
  if (withoutQuery.endsWith('.m3u8')) {
    return VesperPlayerSourceProtocol.hls;
  }
  if (withoutQuery.endsWith('.mpd')) {
    return VesperPlayerSourceProtocol.dash;
  }
  return VesperPlayerSourceProtocol.progressive;
}

String normalizeLocalUri(String raw) {
  final trimmed = raw.trim();
  if (trimmed.isEmpty) {
    return trimmed;
  }
  if (trimmed.startsWith('file://') || trimmed.startsWith('content://')) {
    return trimmed;
  }
  if (trimmed.startsWith('/')) {
    return 'file://$trimmed';
  }
  return trimmed;
}

String localSourceLabel(String uri) {
  final normalized = uri.split('?').first;
  final lastSegment = normalized.split('/').last;
  if (lastSegment.isNotEmpty) {
    return lastSegment;
  }
  return '本地视频';
}

String sheetTitle(ExamplePlayerSheet sheet) {
  return switch (sheet) {
    ExamplePlayerSheet.menu => '播放工具',
    ExamplePlayerSheet.quality => '画质',
    ExamplePlayerSheet.audio => '音频',
    ExamplePlayerSheet.subtitle => '字幕',
    ExamplePlayerSheet.speed => '播放速度',
  };
}

String sheetSubtitle(ExamplePlayerSheet sheet) {
  return switch (sheet) {
    ExamplePlayerSheet.menu => '打开音轨、字幕、画质和速度控制，同时避免播放器浮层过于拥挤。',
    ExamplePlayerSheet.quality => '切换自适应视频，或将流固定到某个具体画质轨道。',
    ExamplePlayerSheet.audio => '选择当前流暴露出来的音频节目。',
    ExamplePlayerSheet.subtitle => '选择字幕，或将其关闭。',
    ExamplePlayerSheet.speed => '预览不同倍速下的播放表现。',
  };
}

String stageBadgeText(VesperTimeline timeline) {
  return switch (timeline.kind) {
    VesperTimelineKind.live => '直播流',
    VesperTimelineKind.liveDvr => '带 DVR 窗口的直播',
    VesperTimelineKind.vod => '点播视频',
  };
}

String playlistItemStatusLabel({required int index, required int activeIndex}) {
  if (activeIndex < 0) {
    return '隐藏';
  }
  if (index == activeIndex) {
    return '当前播放';
  }

  final distance = (index - activeIndex).abs();
  if (distance == 1) {
    return '临近可见';
  }
  return '仅预取';
}

String liveButtonLabel(VesperTimeline timeline) {
  final liveEdge = timeline.goLivePositionMs;
  if (liveEdge == null) {
    return '回到直播';
  }
  final behindMs = (liveEdge - timeline.clampedPosition(timeline.positionMs))
      .clamp(0, liveEdge);
  if (behindMs > 1500) {
    return '直播 -${formatMillis(behindMs)}';
  }
  return '直播';
}

String timelineSummary(VesperTimeline timeline, double? pendingSeekRatio) {
  final displayedPosition = pendingSeekRatio == null
      ? timeline.clampedPosition(timeline.positionMs)
      : timeline.positionForRatio(pendingSeekRatio);

  switch (timeline.kind) {
    case VesperTimelineKind.live:
      final liveEdge = timeline.goLivePositionMs;
      if (liveEdge == null) {
        return '直播';
      }
      return '直播 • 实时点 ${formatMillis(liveEdge)}';
    case VesperTimelineKind.liveDvr:
      final liveEdge = timeline.goLivePositionMs ?? timeline.durationMs ?? 0;
      return '${formatMillis(displayedPosition)} / ${formatMillis(liveEdge)}';
    case VesperTimelineKind.vod:
      return '${formatMillis(displayedPosition)} / ${formatMillis(timeline.durationMs ?? 0)}';
  }
}

String qualityButtonLabel(
  VesperTrackCatalog trackCatalog,
  VesperTrackSelectionSnapshot trackSelection,
) {
  final selectedTrack = trackCatalog.videoTracks.firstWhere(
    (track) => track.id == trackSelection.abrPolicy.trackId,
    orElse: () =>
        const VesperMediaTrack(id: '', kind: VesperMediaTrackKind.video),
  );

  return switch (trackSelection.abrPolicy.mode) {
    VesperAbrMode.fixedTrack when selectedTrack.id.isNotEmpty => qualityLabel(
      selectedTrack,
    ),
    VesperAbrMode.fixedTrack => '画质',
    VesperAbrMode.constrained || VesperAbrMode.auto => '自动',
  };
}

String audioButtonLabel(
  VesperTrackCatalog trackCatalog,
  VesperTrackSelectionSnapshot trackSelection,
) {
  final selectedTrack = firstWhereOrNull<VesperMediaTrack>(
    trackCatalog.audioTracks,
    (track) => track.id == trackSelection.audio.trackId,
  );

  return switch (trackSelection.audio.mode) {
    VesperTrackSelectionMode.track when selectedTrack != null => audioLabel(
      selectedTrack,
    ),
    _ => '音频',
  };
}

String subtitleButtonLabel(
  VesperTrackCatalog trackCatalog,
  VesperTrackSelectionSnapshot trackSelection,
) {
  final selectedTrack = firstWhereOrNull<VesperMediaTrack>(
    trackCatalog.subtitleTracks,
    (track) => track.id == trackSelection.subtitle.trackId,
  );

  return switch (trackSelection.subtitle.mode) {
    VesperTrackSelectionMode.disabled => '字幕关',
    VesperTrackSelectionMode.track when selectedTrack != null => subtitleLabel(
      selectedTrack,
    ),
    VesperTrackSelectionMode.track => '字幕',
    VesperTrackSelectionMode.auto => '字幕自动',
  };
}

String qualityLabel(VesperMediaTrack track) {
  if (track.height != null) {
    return '${track.height}p';
  }
  if (track.width != null && track.height != null) {
    return '${track.width}×${track.height}';
  }
  if (track.label case final label?) {
    return label;
  }
  return '视频轨';
}

String qualitySubtitle(VesperMediaTrack track) {
  final values = <String?>[
    track.codec,
    if (track.bitRate case final bitRate?) formatBitRate(bitRate),
  ].whereType<String>().toList(growable: false);
  if (values.isEmpty) {
    return '固定视频变体';
  }
  return values.join(' • ');
}

String audioLabel(VesperMediaTrack track) {
  if (track.label case final label?) {
    return label;
  }
  if (track.language case final language?) {
    return language.toUpperCase();
  }
  return '音轨';
}

String audioSubtitle(VesperMediaTrack track) {
  final values = <String?>[
    track.language?.toUpperCase(),
    if (track.channels case final channels?) '$channels 声道',
    if (track.sampleRate case final sampleRate?) '${sampleRate ~/ 1000} kHz',
    track.codec,
  ].whereType<String>().toList(growable: false);
  if (values.isEmpty) {
    return '音频节目';
  }
  return values.join(' • ');
}

String subtitleLabel(VesperMediaTrack track) {
  if (track.label case final label?) {
    return label;
  }
  if (track.language case final language?) {
    return language.toUpperCase();
  }
  return '字幕轨';
}

String subtitleSubtitle(VesperMediaTrack track) {
  final values = <String>[
    if (track.language case final language?) language.toUpperCase(),
    if (track.isForced) '强制',
    if (track.isDefault) '默认',
  ];
  if (values.isEmpty) {
    return '字幕选项';
  }
  return values.join(' • ');
}

String speedBadge(double rate) => '${formatRate(rate)}x';

String formatBitRate(int value) {
  if (value >= 1000000) {
    return '${(value / 1000000).toStringAsFixed(1)} Mbps';
  }
  if (value >= 1000) {
    return '${(value / 1000).toStringAsFixed(0)} kbps';
  }
  return '$value bps';
}

String formatRate(double value) {
  if ((value - value.roundToDouble()).abs() < 0.001) {
    return value.toStringAsFixed(0);
  }
  if ((value * 10 - (value * 10).roundToDouble()).abs() < 0.001) {
    return value.toStringAsFixed(1);
  }
  return value.toStringAsFixed(2);
}

String formatMillis(int value) {
  final totalSeconds = value ~/ 1000;
  final minutes = totalSeconds ~/ 60;
  final seconds = totalSeconds % 60;
  return '${minutes.toString().padLeft(2, '0')}:${seconds.toString().padLeft(2, '0')}';
}

String bufferWindowLabel(VesperBufferingPolicy policy) {
  final min = policy.minBufferMs;
  final max = policy.maxBufferMs;
  if (min == null || max == null) {
    return 'default';
  }
  return '$min-$max ms';
}

String formatBytes(int? value) {
  if (value == null) {
    return 'default';
  }
  if (value == 0) {
    return '0 B';
  }
  if (value >= 1024 * 1024 * 1024) {
    return '${(value / (1024 * 1024 * 1024)).toStringAsFixed(1)} GB';
  }
  if (value >= 1024 * 1024) {
    return '${(value / (1024 * 1024)).toStringAsFixed(0)} MB';
  }
  if (value >= 1024) {
    return '${(value / 1024).toStringAsFixed(0)} KB';
  }
  return '$value B';
}

String formatDownloadBytes(int? value) {
  if (value == null || value <= 0) {
    return '-';
  }
  if (value >= 1024 * 1024 * 1024) {
    return '${(value / (1024 * 1024 * 1024)).toStringAsFixed(1)} GB';
  }
  if (value >= 1024 * 1024) {
    return '${(value / (1024 * 1024)).toStringAsFixed(1)} MB';
  }
  if (value >= 1024) {
    return '${(value / 1024).toStringAsFixed(0)} KB';
  }
  return '$value B';
}

T? firstWhereOrNull<T>(Iterable<T> values, bool Function(T value) test) {
  for (final value in values) {
    if (test(value)) {
      return value;
    }
  }
  return null;
}
