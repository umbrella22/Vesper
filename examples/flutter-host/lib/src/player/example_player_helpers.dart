import 'package:vesper_player/vesper_player.dart';

import '../shell/example_host_strings.dart';
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

String localSourceLabel(
  String uri, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final normalized = uri.split('?').first;
  final lastSegment = normalized.split('/').last;
  if (lastSegment.isNotEmpty) {
    return lastSegment;
  }
  return strings[ExampleHostText.localVideo];
}

String sheetTitle(
  ExamplePlayerSheet sheet, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  return strings[switch (sheet) {
    ExamplePlayerSheet.menu => ExampleHostText.sheetMenuTitle,
    ExamplePlayerSheet.quality => ExampleHostText.sheetQualityTitle,
    ExamplePlayerSheet.audio => ExampleHostText.sheetAudioTitle,
    ExamplePlayerSheet.subtitle => ExampleHostText.sheetSubtitleTitle,
    ExamplePlayerSheet.speed => ExampleHostText.sheetSpeedTitle,
  }];
}

String sheetSubtitle(
  ExamplePlayerSheet sheet, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  return strings[switch (sheet) {
    ExamplePlayerSheet.menu => ExampleHostText.sheetMenuSubtitle,
    ExamplePlayerSheet.quality => ExampleHostText.sheetQualitySubtitle,
    ExamplePlayerSheet.audio => ExampleHostText.sheetAudioSubtitle,
    ExamplePlayerSheet.subtitle => ExampleHostText.sheetSubtitleSubtitle,
    ExamplePlayerSheet.speed => ExampleHostText.sheetSpeedSubtitle,
  }];
}

String playlistItemStatusLabel({
  required int index,
  required int activeIndex,
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  if (activeIndex < 0) {
    return strings[ExampleHostText.playlistStatusHidden];
  }
  if (index == activeIndex) {
    return strings[ExampleHostText.playlistStatusPlaying];
  }

  final distance = (index - activeIndex).abs();
  if (distance == 1) {
    return strings[ExampleHostText.playlistStatusNearby];
  }
  return strings[ExampleHostText.playlistStatusPrefetch];
}

String liveButtonLabel(
  VesperTimeline timeline, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final liveEdge = timeline.goLivePositionMs;
  if (liveEdge == null) {
    return strings[ExampleHostText.liveGoLive];
  }
  final behindMs = (liveEdge - timeline.clampedPosition(timeline.positionMs))
      .clamp(0, liveEdge);
  if (behindMs > 1500) {
    return strings.format(ExampleHostText.liveBehind, <String, Object?>{
      'behind': formatMillis(behindMs),
    });
  }
  return strings[ExampleHostText.live];
}

String timelineSummary(
  VesperTimeline timeline,
  double? pendingSeekRatio, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final displayedPosition = pendingSeekRatio == null
      ? timeline.clampedPosition(timeline.positionMs)
      : timeline.positionForRatio(pendingSeekRatio);

  switch (timeline.kind) {
    case VesperTimelineKind.live:
      final liveEdge = timeline.goLivePositionMs;
      if (liveEdge == null) {
        return strings[ExampleHostText.live];
      }
      return strings.format(ExampleHostText.liveEdgeSummary, <String, Object?>{
        'edge': formatMillis(liveEdge),
      });
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
  double? pendingSeekRatio, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final displayedPosition = pendingSeekRatio == null
      ? timeline.clampedPosition(timeline.positionMs)
      : timeline.positionForRatio(pendingSeekRatio);

  switch (timeline.kind) {
    case VesperTimelineKind.live:
      return strings[ExampleHostText.live];
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
  VesperFixedTrackStatus? fixedTrackStatus, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
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
            (resolvedFixedTrackStatus == VesperFixedTrackStatus.pending ||
                resolvedFixedTrackStatus == VesperFixedTrackStatus.fallback) =>
      strings.format(ExampleHostText.qualityLockingValue, <String, Object?>{
        'quality': qualityLabel(requestedTrack),
      }),
    VesperAbrMode.fixedTrack when requestedTrack != null => strings.format(
      ExampleHostText.qualityLockedValue,
      <String, Object?>{'quality': qualityLabel(requestedTrack)},
    ),
    VesperAbrMode.fixedTrack => strings[ExampleHostText.sheetQualityTitle],
    VesperAbrMode.constrained ||
    VesperAbrMode.auto when effectiveTrack != null => strings.format(
      ExampleHostText.qualityAutoValue,
      <String, Object?>{'quality': qualityLabel(effectiveTrack)},
    ),
    VesperAbrMode.constrained ||
    VesperAbrMode.auto => strings[ExampleHostText.auto],
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
