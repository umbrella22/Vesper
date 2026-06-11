import 'package:vesper_player/vesper_player.dart';

import 'stage_models.dart';

String stageBadgeText(
  VesperTimeline timeline, {
  VesperPlayerStageStrings strings = const VesperPlayerStageStrings(),
}) {
  return switch (timeline.kind) {
    VesperTimelineKind.live => strings.liveTimelineBadge,
    VesperTimelineKind.liveDvr => strings.liveDvrTimelineBadge,
    VesperTimelineKind.vod => strings.vodTimelineBadge,
  };
}

String liveButtonLabel(
  VesperTimeline timeline, {
  VesperPlayerStageStrings strings = const VesperPlayerStageStrings(),
}) {
  final liveEdge = timeline.goLivePositionMs;
  if (liveEdge == null) {
    return strings.goLive;
  }
  final behindMs = (liveEdge - timeline.clampedPosition(timeline.positionMs))
      .clamp(0, 1 << 62);
  if (behindMs <= 1500) {
    return strings.live;
  }
  return '${strings.liveBehindPrefix}${formatMillis(behindMs)}';
}

String timelineSummary(
  VesperTimeline timeline,
  double? pendingSeekRatio, {
  VesperPlayerStageStrings strings = const VesperPlayerStageStrings(),
}) {
  final displayedPosition = pendingSeekRatio == null
      ? timeline.clampedPosition(timeline.positionMs)
      : timeline.positionForRatio(pendingSeekRatio);

  switch (timeline.kind) {
    case VesperTimelineKind.live:
      final liveEdge = timeline.goLivePositionMs;
      if (liveEdge == null) {
        return strings.live;
      }
      return '${strings.liveEdge} ${formatMillis(liveEdge)}';
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
  double? pendingSeekRatio, {
  VesperPlayerStageStrings strings = const VesperPlayerStageStrings(),
}) {
  final displayedPosition = pendingSeekRatio == null
      ? timeline.clampedPosition(timeline.positionMs)
      : timeline.positionForRatio(pendingSeekRatio);

  switch (timeline.kind) {
    case VesperTimelineKind.live:
      return strings.live;
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
  VesperPlayerStageStrings strings = const VesperPlayerStageStrings(),
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
      '${strings.locking}${strings.qualitySeparator}${qualityLabel(requestedTrack)}',
    VesperAbrMode.fixedTrack
        when requestedTrack != null &&
            resolvedFixedTrackStatus == VesperFixedTrackStatus.fallback =>
      '${strings.locking}${strings.qualitySeparator}${qualityLabel(requestedTrack)}',
    VesperAbrMode.fixedTrack when requestedTrack != null =>
      '${strings.pinned}${strings.qualitySeparator}${qualityLabel(requestedTrack)}',
    VesperAbrMode.fixedTrack => strings.quality,
    VesperAbrMode.constrained ||
    VesperAbrMode.auto when effectiveTrack != null =>
      '${strings.auto}${strings.qualitySeparator}${qualityLabel(effectiveTrack)}',
    VesperAbrMode.constrained || VesperAbrMode.auto => strings.auto,
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
