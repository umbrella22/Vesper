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
  String? effectiveVideoTrackId,
  VesperFixedTrackStatus? fixedTrackStatus,
) {
  final requestedTrack = requestedFixedVideoTrack(trackCatalog, trackSelection);
  final effectiveTrack = effectiveVideoTrack(
    trackCatalog,
    effectiveVideoTrackId,
  );
  final resolvedFixedTrackStatus = currentFixedTrackStatus(
    trackCatalog,
    trackSelection,
    effectiveVideoTrackId,
    fixedTrackStatus,
  );

  return switch (trackSelection.abrPolicy.mode) {
    VesperAbrMode.fixedTrack
        when requestedTrack != null &&
            resolvedFixedTrackStatus == VesperFixedTrackStatus.pending =>
      '锁定中 · ${qualityLabel(requestedTrack)}',
    VesperAbrMode.fixedTrack
        when requestedTrack != null &&
            resolvedFixedTrackStatus == VesperFixedTrackStatus.fallback =>
      '锁定中 · ${qualityLabel(requestedTrack)}',
    VesperAbrMode.fixedTrack when requestedTrack != null =>
      '锁定 · ${qualityLabel(requestedTrack)}',
    VesperAbrMode.fixedTrack => '画质',
    VesperAbrMode.constrained || VesperAbrMode.auto
        when effectiveTrack != null =>
      '自动 · ${qualityLabel(effectiveTrack)}',
    VesperAbrMode.constrained || VesperAbrMode.auto => '自动',
  };
}

VesperMediaTrack? effectiveVideoTrack(
  VesperTrackCatalog trackCatalog,
  String? effectiveVideoTrackId,
) {
  return firstWhereOrNull<VesperMediaTrack>(
    trackCatalog.videoTracks,
    (track) => track.id == effectiveVideoTrackId,
  );
}

VesperMediaTrack? requestedFixedVideoTrack(
  VesperTrackCatalog trackCatalog,
  VesperTrackSelectionSnapshot trackSelection,
) {
  if (trackSelection.abrPolicy.mode != VesperAbrMode.fixedTrack) {
    return null;
  }
  return firstWhereOrNull<VesperMediaTrack>(
    trackCatalog.videoTracks,
    (track) => track.id == trackSelection.abrPolicy.trackId,
  );
}

VesperFixedTrackStatus? currentFixedTrackStatus(
  VesperTrackCatalog trackCatalog,
  VesperTrackSelectionSnapshot trackSelection,
  String? effectiveVideoTrackId,
  VesperFixedTrackStatus? fixedTrackStatus,
) {
  if (trackSelection.abrPolicy.mode != VesperAbrMode.fixedTrack) {
    return null;
  }
  if (fixedTrackStatus != null) {
    return fixedTrackStatus;
  }
  final requestedTrack = requestedFixedVideoTrack(trackCatalog, trackSelection);
  if (requestedTrack == null || effectiveVideoTrackId == null) {
    return VesperFixedTrackStatus.pending;
  }
  if (effectiveVideoTrackId == requestedTrack.id) {
    return VesperFixedTrackStatus.locked;
  }
  return VesperFixedTrackStatus.fallback;
}

String qualityAutoRowTitle(VesperAbrPolicy _) {
  return '自动';
}

String? qualityAutoRowBadgeLabel(VesperAbrPolicy abrPolicy) {
  return switch (abrPolicy.mode) {
    VesperAbrMode.constrained => '受限',
    _ => null,
  };
}

String qualityAutoRowSubtitle(
  VesperTrackCatalog trackCatalog,
  VesperTrackSelectionSnapshot trackSelection,
  String? effectiveVideoTrackId,
  VesperFixedTrackStatus? fixedTrackStatus,
  VesperVideoVariantObservation? videoVariantObservation,
) {
  final abrPolicy = trackSelection.abrPolicy;
  final effectiveTrack = effectiveVideoTrack(
    trackCatalog,
    effectiveVideoTrackId,
  );
  final requestedTrack = requestedFixedVideoTrack(trackCatalog, trackSelection);
  final resolvedFixedTrackStatus = currentFixedTrackStatus(
    trackCatalog,
    trackSelection,
    effectiveVideoTrackId,
    fixedTrackStatus,
  );
  final constraintSummary = abrConstraintSummary(abrPolicy);
  final observationSummary = videoVariantObservationSummary(
    videoVariantObservation,
  );

  final lead = switch (abrPolicy.mode) {
    VesperAbrMode.auto => '让播放器自动调整画质。',
    VesperAbrMode.constrained when constraintSummary != null =>
      '当前在$constraintSummary约束内自动调整画质。点按恢复完全自动。',
    VesperAbrMode.constrained => '当前在受限 ABR 策略内自动调整画质。点按恢复完全自动。',
    VesperAbrMode.fixedTrack
        when requestedTrack != null &&
            resolvedFixedTrackStatus == VesperFixedTrackStatus.fallback &&
            effectiveTrack != null =>
      '当前请求锁定到${qualityLabel(requestedTrack)}，播放器实际仍在${qualityLabel(effectiveTrack)}。点按切回自动。',
    VesperAbrMode.fixedTrack
        when requestedTrack != null &&
            resolvedFixedTrackStatus == VesperFixedTrackStatus.pending =>
      '当前请求锁定到${qualityLabel(requestedTrack)}，正在等待播放器确认实际档位。点按切回自动。',
    VesperAbrMode.fixedTrack
        when requestedTrack != null && effectiveTrack != null =>
      '当前已锁定到${qualityLabel(requestedTrack)}。点按切回自动。',
    VesperAbrMode.fixedTrack when requestedTrack != null =>
      '当前请求锁定到${qualityLabel(requestedTrack)}。点按切回自动。',
    VesperAbrMode.fixedTrack => '切回让播放器自动调整画质。',
  };
  if (abrPolicy.mode == VesperAbrMode.fixedTrack) {
    if (observationSummary == null) {
      return lead;
    }
    return '$lead当前观测：$observationSummary。';
  }
  if (effectiveTrack == null) {
    if (observationSummary == null) {
      return lead;
    }
    return '$lead当前观测：$observationSummary。';
  }
  if (observationSummary == null) {
    return '$lead当前实际档位：${qualityLabel(effectiveTrack)}。';
  }
  return '$lead当前实际档位：${qualityLabel(effectiveTrack)}。当前观测：$observationSummary。';
}

String qualityOptionSubtitle(
  VesperMediaTrack track,
  VesperTrackSelectionSnapshot trackSelection,
  String? effectiveVideoTrackId,
  VesperFixedTrackStatus? fixedTrackStatus, {
  required VesperTrackCatalog trackCatalog,
}) {
  final base = qualitySubtitle(track);
  final isFixedTrack =
      trackSelection.abrPolicy.mode == VesperAbrMode.fixedTrack;
  final isRequestedTrack =
      isFixedTrack && trackSelection.abrPolicy.trackId == track.id;
  final isEffectiveTrack = effectiveVideoTrackId == track.id;
  final resolvedFixedTrackStatus = currentFixedTrackStatus(
    trackCatalog,
    trackSelection,
    effectiveVideoTrackId,
    fixedTrackStatus,
  );

  final stateDescription = switch ((isRequestedTrack, isEffectiveTrack)) {
    (true, true) => '当前已锁定到这个画质',
    (true, false)
        when resolvedFixedTrackStatus == VesperFixedTrackStatus.pending =>
      '正在等待播放器确认这个画质',
    (true, false) => '已请求锁定到这个画质',
    (false, true) when isFixedTrack => '播放器当前仍在这个档位',
    _ => null,
  };
  if (stateDescription == null) {
    return base;
  }
  return '$base · $stateDescription';
}

String? qualityOptionBadgeLabel(
  String trackId, {
  required VesperTrackCatalog trackCatalog,
  required VesperTrackSelectionSnapshot trackSelection,
  required String? effectiveVideoTrackId,
  VesperFixedTrackStatus? fixedTrackStatus,
}) {
  final isFixedTrack =
      trackSelection.abrPolicy.mode == VesperAbrMode.fixedTrack;
  final requestedTrackId = trackSelection.abrPolicy.trackId;
  final resolvedFixedTrackStatus = currentFixedTrackStatus(
    trackCatalog,
    trackSelection,
    effectiveVideoTrackId,
    fixedTrackStatus,
  );

  if (isFixedTrack && trackId == requestedTrackId) {
    if (resolvedFixedTrackStatus == VesperFixedTrackStatus.locked) {
      return '当前';
    }
    if (resolvedFixedTrackStatus == VesperFixedTrackStatus.pending) {
      return '等待';
    }
    return '锁定中';
  }

  if (isFixedTrack &&
      resolvedFixedTrackStatus == VesperFixedTrackStatus.fallback &&
      effectiveVideoTrackId == trackId) {
    return '实际';
  }

  if (trackId == effectiveVideoTrackId) {
    return '当前';
  }
  return null;
}

String? abrConstraintSummary(VesperAbrPolicy abrPolicy) {
  final constraints = <String>[
    if (abrPolicy.maxHeight case final height?) '最高 ${height}p',
    if (abrPolicy.maxWidth case final width?) '最大宽度 $width',
    if (abrPolicy.maxBitRate case final bitRate?)
      '最高 ${formatBitRate(bitRate)}',
  ];
  if (constraints.isEmpty) {
    return null;
  }
  return constraints.join('，');
}

String? videoVariantObservationSummary(
  VesperVideoVariantObservation? observation,
) {
  if (observation == null || !observation.hasSignal) {
    return null;
  }
  final parts = <String>[
    if (observation.width case final width?)
      if (observation.height case final height?) '$width×$height',
    if (observation.bitRate case final bitRate?) formatBitRate(bitRate),
  ];
  if (parts.isEmpty) {
    return null;
  }
  return parts.join(' · ');
}

String? qualityCapabilityNotice(VesperPlayerCapabilities capabilities) {
  final supportsFixedTrackAbr = capabilities.supportsAbrMode(
    VesperAbrMode.fixedTrack,
  );
  final supportsExactVideoTrackSelection = capabilities
      .supportsTrackSelectionFor(VesperMediaTrackKind.video);
  if (supportsFixedTrackAbr && !supportsExactVideoTrackSelection) {
    return '当前平台按 HLS variant 做 best-effort 固定画质，不保证精确切到单一视频轨。';
  }
  return null;
}

ExampleSheetNoticeModel? qualityRuntimeNotice(VesperPlayerSnapshot snapshot) {
  final lastError = snapshot.lastError;
  if (lastError == null ||
      lastError.category != VesperPlayerErrorCategory.playback) {
    return null;
  }
  if (!lastError.message.contains('fixedTrack')) {
    return null;
  }

  final title = switch (snapshot.trackSelection.abrPolicy.mode) {
    VesperAbrMode.constrained => '已回退为受限自动',
    VesperAbrMode.auto => '已回退为自动画质',
    VesperAbrMode.fixedTrack
        when snapshot.fixedTrackStatus == VesperFixedTrackStatus.fallback =>
      '锁定画质仍未收敛',
    VesperAbrMode.fixedTrack => '画质锁定提示',
  };

  return ExampleSheetNoticeModel(
    title: title,
    message: lastError.message,
    tone: ExampleSheetNoteTone.warm,
  );
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
