import 'package:vesper_player/vesper_player.dart';

import 'example_player_models.dart';

part 'example_player_track_helpers.dart';
part 'example_player_format_helpers.dart';

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
      final rangeStart = timeline.seekableRange?.startMs ?? 0;
      final windowPosition = (displayedPosition - rangeStart)
          .clamp(0, liveEdge)
          .toInt();
      final windowEnd = (liveEdge - rangeStart).clamp(0, liveEdge).toInt();
      return '${formatMillis(windowPosition)} / ${formatMillis(windowEnd)}';
    case VesperTimelineKind.vod:
      return '${formatMillis(displayedPosition)} / ${formatMillis(timeline.durationMs ?? 0)}';
  }
}

String compactTimelineSummary(
  VesperTimeline timeline,
  double? pendingSeekRatio,
) {
  final displayedPosition = pendingSeekRatio == null
      ? timeline.clampedPosition(timeline.positionMs)
      : timeline.positionForRatio(pendingSeekRatio);

  switch (timeline.kind) {
    case VesperTimelineKind.live:
      return '直播';
    case VesperTimelineKind.liveDvr:
      final liveEdge = timeline.goLivePositionMs ?? timeline.durationMs ?? 0;
      final rangeStart = timeline.seekableRange?.startMs ?? 0;
      final windowPosition = (displayedPosition - rangeStart)
          .clamp(0, liveEdge)
          .toInt();
      final windowEnd = (liveEdge - rangeStart).clamp(0, liveEdge).toInt();
      return '${formatMillis(windowPosition)}/${formatMillis(windowEnd)}';
    case VesperTimelineKind.vod:
      return '${formatMillis(displayedPosition)}/${formatMillis(timeline.durationMs ?? 0)}';
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
