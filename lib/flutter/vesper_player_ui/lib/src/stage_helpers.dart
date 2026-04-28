import 'package:vesper_player/vesper_player.dart';

String stageBadgeText(VesperTimeline timeline) {
  return switch (timeline.kind) {
    VesperTimelineKind.live => '直播流',
    VesperTimelineKind.liveDvr => '带 DVR 窗口的直播',
    VesperTimelineKind.vod => '点播视频',
  };
}

String liveButtonLabel(VesperTimeline timeline) {
  final liveEdge = timeline.goLivePositionMs;
  if (liveEdge == null) {
    return '回到直播';
  }
  final behindMs = (liveEdge - timeline.clampedPosition(timeline.positionMs))
      .clamp(0, 1 << 62);
  if (behindMs <= 1500) {
    return '直播';
  }
  return '直播 -${formatMillis(behindMs)}';
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
      final windowPosition = (displayedPosition - rangeStart).clamp(
        0,
        liveEdge - rangeStart,
      );
      final windowEnd = (liveEdge - rangeStart).clamp(0, 1 << 62);
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
      final windowPosition = (displayedPosition - rangeStart).clamp(
        0,
        liveEdge - rangeStart,
      );
      final windowEnd = (liveEdge - rangeStart).clamp(0, 1 << 62);
      return '${formatMillis(windowPosition)}/${formatMillis(windowEnd)}';
    case VesperTimelineKind.vod:
      return '${formatMillis(displayedPosition)}/${formatMillis(timeline.durationMs ?? 0)}';
  }
}

String qualityButtonLabel(
  VesperTrackCatalog trackCatalog,
  VesperTrackSelectionSnapshot trackSelection, {
  String? effectiveVideoTrackId,
  VesperFixedTrackStatus? fixedTrackStatus,
}) {
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
  for (final track in trackCatalog.videoTracks) {
    if (track.id == effectiveVideoTrackId) {
      return track;
    }
  }
  return null;
}

VesperMediaTrack? requestedFixedVideoTrack(
  VesperTrackCatalog trackCatalog,
  VesperTrackSelectionSnapshot trackSelection,
) {
  if (trackSelection.abrPolicy.mode != VesperAbrMode.fixedTrack) {
    return null;
  }
  for (final track in trackCatalog.videoTracks) {
    if (track.id == trackSelection.abrPolicy.trackId) {
      return track;
    }
  }
  return null;
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
  if (requestedTrack == null) {
    return null;
  }
  if (effectiveVideoTrackId == null) {
    return VesperFixedTrackStatus.pending;
  }
  if (effectiveVideoTrackId == requestedTrack.id) {
    return VesperFixedTrackStatus.locked;
  }
  return VesperFixedTrackStatus.fallback;
}

String qualityLabel(VesperMediaTrack track) {
  if (track.height != null) {
    return '${track.height}p';
  }
  if (track.width != null) {
    return '${track.width}w';
  }
  if (track.bitRate != null) {
    return formatBitRate(track.bitRate!);
  }
  return track.label ?? track.id;
}

String speedBadge(double rate) => '${formatRate(rate)}x';

String formatBitRate(int value) {
  if (value >= 1000000) {
    return '${(value / 1000000).toStringAsFixed(1)} Mbps';
  }
  if (value >= 1000) {
    return '${(value / 1000).toStringAsFixed(0)} Kbps';
  }
  return '$value bps';
}

String formatRate(double value) {
  return value.toStringAsFixed(1).replaceFirst(RegExp(r'\.0$'), '.0');
}

String formatMillis(int value) {
  final safeValue = value < 0 ? 0 : value;
  final totalSeconds = safeValue ~/ 1000;
  final minutes = totalSeconds ~/ 60;
  final seconds = totalSeconds % 60;
  return '${minutes.toString().padLeft(2, '0')}:${seconds.toString().padLeft(2, '0')}';
}
