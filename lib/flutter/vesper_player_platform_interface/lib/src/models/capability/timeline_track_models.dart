part of '../../models.dart';

final class VesperSeekableRange {
  const VesperSeekableRange({required this.startMs, required this.endMs});

  factory VesperSeekableRange.fromMap(Map<Object?, Object?> map) {
    return VesperSeekableRange(
      startMs: _decodeInt(map, 'startMs') ?? 0,
      endMs: _decodeInt(map, 'endMs') ?? 0,
    );
  }

  final int startMs;
  final int endMs;

  Map<String, Object?> toMap() {
    return <String, Object?>{'startMs': startMs, 'endMs': endMs};
  }
}

final class VesperTimeline {
  const VesperTimeline({
    required this.kind,
    required this.isSeekable,
    required this.positionMs,
    this.seekableRange,
    this.liveEdgeMs,
    this.durationMs,
  });

  const VesperTimeline.initial()
      : kind = VesperTimelineKind.vod,
        isSeekable = false,
        positionMs = 0,
        seekableRange = null,
        liveEdgeMs = null,
        durationMs = null;

  factory VesperTimeline.fromMap(Map<Object?, Object?> map) {
    final rawRange = map['seekableRange'];
    final seekableRange = _rawMap(rawRange);
    return VesperTimeline(
      kind: _decodeEnum(
        VesperTimelineKind.values,
        map['kind'],
        VesperTimelineKind.vod,
      ),
      isSeekable: _decodeBool(map, 'isSeekable'),
      seekableRange: seekableRange != null
          ? VesperSeekableRange.fromMap(seekableRange)
          : null,
      liveEdgeMs: _decodeInt(map, 'liveEdgeMs'),
      positionMs: _decodeInt(map, 'positionMs') ?? 0,
      durationMs: _decodeInt(map, 'durationMs'),
    );
  }

  /// Decodes a timeline returned by the timeline-only platform operation.
  ///
  /// Unlike legacy full snapshots, a malformed or forward-version sample is
  /// rejected instead of being silently converted to the initial VOD value.
  factory VesperTimeline.fromSampleMap(Map<Object?, Object?> map) {
    final rawKind = map['kind'];
    VesperTimelineKind? kind;
    if (rawKind is String) {
      for (final value in VesperTimelineKind.values) {
        if (value.name == rawKind) {
          kind = value;
          break;
        }
      }
    }
    if (kind == null) {
      throw FormatException('Unknown timeline kind: $rawKind');
    }

    final rawSeekable = map['isSeekable'];
    if (rawSeekable is! bool) {
      throw const FormatException('Timeline isSeekable is missing or invalid.');
    }

    final positionMs = _strictTimelineInt(map['positionMs'], 'positionMs');
    final durationMs =
        _strictOptionalTimelineInt(map['durationMs'], 'durationMs');
    final liveEdgeMs =
        _strictOptionalTimelineInt(map['liveEdgeMs'], 'liveEdgeMs');
    final rawRange = map['seekableRange'];
    VesperSeekableRange? seekableRange;
    if (rawRange != null) {
      if (rawRange is! Map) {
        throw const FormatException('Timeline seekableRange is invalid.');
      }
      final range = Map<Object?, Object?>.from(rawRange);
      seekableRange = VesperSeekableRange(
        startMs: _strictTimelineInt(range['startMs'], 'seekableRange.startMs'),
        endMs: _strictTimelineInt(range['endMs'], 'seekableRange.endMs'),
      );
    }

    return VesperTimeline(
      kind: kind,
      isSeekable: rawSeekable,
      seekableRange: seekableRange,
      liveEdgeMs: liveEdgeMs,
      positionMs: positionMs,
      durationMs: durationMs,
    );
  }

  final VesperTimelineKind kind;
  final bool isSeekable;
  final VesperSeekableRange? seekableRange;
  final int? liveEdgeMs;
  final int positionMs;
  final int? durationMs;

  double? get displayedRatio {
    final range = seekableRange;
    if (range != null && range.endMs > range.startMs) {
      final clamped = clampedPosition(positionMs);
      final width = range.endMs - range.startMs;
      if (width <= 0) {
        return null;
      }
      final ratio = (clamped - range.startMs) / width;
      return ratio.clamp(0.0, 1.0).toDouble();
    }
    final total = durationMs;
    if (total == null || total <= 0) {
      return null;
    }
    return (clampedPosition(positionMs) / total).clamp(0.0, 1.0).toDouble();
  }

  int? get goLivePositionMs => switch (kind) {
        VesperTimelineKind.vod => null,
        VesperTimelineKind.live => liveEdgeMs,
        VesperTimelineKind.liveDvr => liveEdgeMs ?? seekableRange?.endMs,
      };

  int? get liveOffsetMs {
    final liveEdge = goLivePositionMs;
    if (liveEdge == null) {
      return null;
    }
    return (liveEdge - clampedPosition(positionMs)).clamp(0, liveEdge);
  }

  int clampedPosition(int positionMs) {
    final range = seekableRange;
    if (range != null && range.endMs >= range.startMs) {
      return positionMs.clamp(range.startMs, range.endMs);
    }

    final total = durationMs;
    if (total == null) {
      return positionMs < 0 ? 0 : positionMs;
    }

    return positionMs.clamp(0, total < 0 ? 0 : total);
  }

  int positionForRatio(double ratio) {
    final normalized = ratio.clamp(0.0, 1.0).toDouble();
    final range = seekableRange;
    if (range != null && range.endMs >= range.startMs) {
      final width = range.endMs - range.startMs;
      return clampedPosition(range.startMs + (width * normalized).toInt());
    }

    return clampedPosition(((durationMs ?? 0) * normalized).toInt());
  }

  bool isAtLiveEdge({int toleranceMs = 1500}) {
    final liveEdge = goLivePositionMs;
    if (liveEdge == null) {
      return false;
    }
    final effectiveTolerance = toleranceMs < 0 ? 0 : toleranceMs;
    return (liveEdge - clampedPosition(positionMs)).abs() <= effectiveTolerance;
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'kind': kind.name,
      'isSeekable': isSeekable,
      'seekableRange': seekableRange?.toMap(),
      'liveEdgeMs': liveEdgeMs,
      'positionMs': positionMs,
      'durationMs': durationMs,
    };
  }
}

int _strictTimelineInt(Object? raw, String field) {
  if (raw is int) {
    return raw;
  }
  throw FormatException('Timeline $field is missing or invalid.');
}

int? _strictOptionalTimelineInt(Object? raw, String field) {
  if (raw == null) {
    return null;
  }
  return _strictTimelineInt(raw, field);
}

enum VesperTrackSupportStatus {
  supported,
  exceedsCapabilities,
  unsupported,
  unknown,
}

enum VesperTrackSupportReason {
  none,
  formatExceedsCapabilities,
  unsupportedType,
  unsupportedSubtype,
  unsupportedDrm,
  routeUnavailable,
  presentationUnavailable,
  runtimeFailure,
  platformUnknown,
  unknown,
}

enum VesperTrackSupportSource {
  runtimeTrackCatalog,
  capabilityProbe,
  runtimeFailure,
  unavailable,
  unknown,
}

final class VesperTrackSupport {
  const VesperTrackSupport({
    this.status = VesperTrackSupportStatus.unknown,
    this.reason = VesperTrackSupportReason.platformUnknown,
    this.source = VesperTrackSupportSource.unavailable,
    this.statusRawValue,
    this.reasonRawValue,
    this.sourceRawValue,
    this.playbackPath,
    this.formatSupportRawValue,
    this.diagnostics = const <String, Object?>{},
  });

  factory VesperTrackSupport.fromMap(Map<Object?, Object?> map) {
    final rawStatus = map['status'];
    final rawReason = map['reason'];
    final rawSource = map['source'];
    final status = _trackSupportStatusFromWire(rawStatus);
    final reason = _trackSupportReasonFromWire(rawReason);
    final source = _trackSupportSourceFromWire(rawSource);
    return VesperTrackSupport(
      status: status,
      reason: reason,
      source: source,
      statusRawValue: _trackSupportRawValue(
        map['statusRawValue'],
        rawStatus,
        status == VesperTrackSupportStatus.unknown,
      ),
      reasonRawValue: _trackSupportRawValue(
        map['reasonRawValue'],
        rawReason,
        reason == VesperTrackSupportReason.unknown,
      ),
      sourceRawValue: _trackSupportRawValue(
        map['sourceRawValue'],
        rawSource,
        source == VesperTrackSupportSource.unknown,
      ),
      playbackPath: map['playbackPath'] as String?,
      formatSupportRawValue: map['formatSupportRawValue'] as String?,
      diagnostics: _decodeObjectMap(map['diagnostics']),
    );
  }

  final VesperTrackSupportStatus status;
  final VesperTrackSupportReason reason;
  final VesperTrackSupportSource source;
  final String? statusRawValue;
  final String? reasonRawValue;
  final String? sourceRawValue;
  final String? playbackPath;
  final String? formatSupportRawValue;
  final Map<String, Object?> diagnostics;

  bool get canAttemptExplicitSelection {
    return status == VesperTrackSupportStatus.supported ||
        status == VesperTrackSupportStatus.unknown;
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'status': status.name,
      'reason': reason.name,
      'source': source.name,
      'statusRawValue': statusRawValue,
      'reasonRawValue': reasonRawValue,
      'sourceRawValue': sourceRawValue,
      'playbackPath': playbackPath,
      'formatSupportRawValue': formatSupportRawValue,
      'diagnostics': diagnostics,
    };
  }
}

VesperTrackSupportStatus _trackSupportStatusFromWire(Object? raw) {
  if (raw is String) {
    for (final value in VesperTrackSupportStatus.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperTrackSupportStatus.unknown;
}

VesperTrackSupportReason _trackSupportReasonFromWire(Object? raw) {
  if (raw == null) {
    return VesperTrackSupportReason.platformUnknown;
  }
  if (raw is String) {
    for (final value in VesperTrackSupportReason.values) {
      if (value.name == raw) {
        return value;
      }
    }
    return VesperTrackSupportReason.unknown;
  }
  return VesperTrackSupportReason.platformUnknown;
}

VesperTrackSupportSource _trackSupportSourceFromWire(Object? raw) {
  if (raw == null) {
    return VesperTrackSupportSource.unavailable;
  }
  if (raw is String) {
    for (final value in VesperTrackSupportSource.values) {
      if (value.name == raw) {
        return value;
      }
    }
    return VesperTrackSupportSource.unknown;
  }
  return VesperTrackSupportSource.unavailable;
}

String? _trackSupportRawValue(Object? explicit, Object? wire, bool unknown) {
  if (explicit is String && explicit.isNotEmpty) {
    return explicit;
  }
  if (unknown && wire is String && wire.isNotEmpty) {
    return wire;
  }
  return null;
}

final class VesperMediaTrack {
  const VesperMediaTrack({
    required this.id,
    required this.kind,
    this.label,
    this.language,
    this.codec,
    this.bitRate,
    this.width,
    this.height,
    this.frameRate,
    this.channels,
    this.sampleRate,
    this.isDefault = false,
    this.isForced = false,
    this.support = const VesperTrackSupport(),
  });

  factory VesperMediaTrack.fromMap(Map<Object?, Object?> map) {
    return VesperMediaTrack(
      id: map['id'] as String? ?? '',
      kind: _decodeEnum(
        VesperMediaTrackKind.values,
        map['kind'],
        VesperMediaTrackKind.video,
      ),
      label: map['label'] as String?,
      language: map['language'] as String?,
      codec: map['codec'] as String?,
      bitRate: _decodeInt(map, 'bitRate'),
      width: _decodeInt(map, 'width'),
      height: _decodeInt(map, 'height'),
      frameRate: _decodeDouble(map, 'frameRate'),
      channels: _decodeInt(map, 'channels'),
      sampleRate: _decodeInt(map, 'sampleRate'),
      isDefault: _decodeBool(map, 'isDefault'),
      isForced: _decodeBool(map, 'isForced'),
      support: _rawMap(map['support']) != null
          ? VesperTrackSupport.fromMap(_rawMap(map['support'])!)
          : const VesperTrackSupport(),
    );
  }

  final String id;
  final VesperMediaTrackKind kind;
  final String? label;
  final String? language;
  final String? codec;
  final int? bitRate;
  final int? width;
  final int? height;
  final double? frameRate;
  final int? channels;
  final int? sampleRate;
  final bool isDefault;
  final bool isForced;
  final VesperTrackSupport support;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'id': id,
      'kind': kind.name,
      'label': label,
      'language': language,
      'codec': codec,
      'bitRate': bitRate,
      'width': width,
      'height': height,
      'frameRate': frameRate,
      'channels': channels,
      'sampleRate': sampleRate,
      'isDefault': isDefault,
      'isForced': isForced,
      'support': support.toMap(),
    };
  }
}

final class VesperTrackCatalog {
  const VesperTrackCatalog({
    this.tracks = const <VesperMediaTrack>[],
    this.adaptiveVideo = false,
    this.adaptiveAudio = false,
    this.catalogRevision = 0,
    this.playbackPath,
  });

  factory VesperTrackCatalog.fromMap(Map<Object?, Object?> map) {
    final rawTracks = map['tracks'];
    return VesperTrackCatalog(
      tracks: rawTracks is Iterable
          ? rawTracks
              .whereType<Map<Object?, Object?>>()
              .map(VesperMediaTrack.fromMap)
              .toList(growable: false)
          : const <VesperMediaTrack>[],
      adaptiveVideo: _decodeBool(map, 'adaptiveVideo'),
      adaptiveAudio: _decodeBool(map, 'adaptiveAudio'),
      catalogRevision: _decodeInt(map, 'catalogRevision') ?? 0,
      playbackPath: map['playbackPath'] as String?,
    );
  }

  final List<VesperMediaTrack> tracks;
  final bool adaptiveVideo;
  final bool adaptiveAudio;
  final int catalogRevision;
  final String? playbackPath;

  List<VesperMediaTrack> get videoTracks {
    return tracks
        .where((track) => track.kind == VesperMediaTrackKind.video)
        .toList();
  }

  List<VesperMediaTrack> get audioTracks {
    return tracks
        .where((track) => track.kind == VesperMediaTrackKind.audio)
        .toList();
  }

  List<VesperMediaTrack> get subtitleTracks {
    return tracks
        .where((track) => track.kind == VesperMediaTrackKind.subtitle)
        .toList();
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'tracks': tracks.map((track) => track.toMap()).toList(growable: false),
      'adaptiveVideo': adaptiveVideo,
      'adaptiveAudio': adaptiveAudio,
      'catalogRevision': catalogRevision,
      'playbackPath': playbackPath,
    };
  }
}

final class VesperTrackSelection {
  const VesperTrackSelection({required this.mode, this.trackId});

  const VesperTrackSelection.auto()
      : mode = VesperTrackSelectionMode.auto,
        trackId = null;

  const VesperTrackSelection.disabled()
      : mode = VesperTrackSelectionMode.disabled,
        trackId = null;

  const VesperTrackSelection.track(String this.trackId)
      : mode = VesperTrackSelectionMode.track;

  factory VesperTrackSelection.fromMap(Map<Object?, Object?> map) {
    return VesperTrackSelection(
      mode: _decodeEnum(
        VesperTrackSelectionMode.values,
        map['mode'],
        VesperTrackSelectionMode.auto,
      ),
      trackId: map['trackId'] as String?,
    );
  }

  final VesperTrackSelectionMode mode;
  final String? trackId;

  Map<String, Object?> toMap() {
    return <String, Object?>{'mode': mode.name, 'trackId': trackId};
  }
}

final class VesperAbrPolicy {
  const VesperAbrPolicy({
    required this.mode,
    this.trackId,
    this.maxBitRate,
    this.maxWidth,
    this.maxHeight,
  });

  const VesperAbrPolicy.auto()
      : mode = VesperAbrMode.auto,
        trackId = null,
        maxBitRate = null,
        maxWidth = null,
        maxHeight = null;

  const VesperAbrPolicy.constrained({
    this.maxBitRate,
    this.maxWidth,
    this.maxHeight,
  })  : mode = VesperAbrMode.constrained,
        trackId = null;

  const VesperAbrPolicy.fixedTrack(String this.trackId)
      : mode = VesperAbrMode.fixedTrack,
        maxBitRate = null,
        maxWidth = null,
        maxHeight = null;

  factory VesperAbrPolicy.fromMap(Map<Object?, Object?> map) {
    return VesperAbrPolicy(
      mode: _decodeEnum(VesperAbrMode.values, map['mode'], VesperAbrMode.auto),
      trackId: map['trackId'] as String?,
      maxBitRate: _decodeInt(map, 'maxBitRate'),
      maxWidth: _decodeInt(map, 'maxWidth'),
      maxHeight: _decodeInt(map, 'maxHeight'),
    );
  }

  final VesperAbrMode mode;
  final String? trackId;
  final int? maxBitRate;
  final int? maxWidth;
  final int? maxHeight;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'mode': mode.name,
      'trackId': trackId,
      'maxBitRate': maxBitRate,
      'maxWidth': maxWidth,
      'maxHeight': maxHeight,
    };
  }
}

final class VesperTrackSelectionSnapshot {
  const VesperTrackSelectionSnapshot({
    this.video = const VesperTrackSelection.auto(),
    this.audio = const VesperTrackSelection.auto(),
    this.subtitle = const VesperTrackSelection.disabled(),
    VesperTrackSelection? confirmedSubtitle,
    this.effectiveSubtitleTrackId,
    this.abrPolicy = const VesperAbrPolicy.auto(),
  }) : confirmedSubtitle = confirmedSubtitle ?? subtitle;

  factory VesperTrackSelectionSnapshot.fromMap(Map<Object?, Object?> map) {
    final rawVideo = map['video'];
    final rawAudio = map['audio'];
    final rawSubtitle = map['subtitle'];
    final rawAbr = map['abrPolicy'];
    final video = _rawMap(rawVideo);
    final audio = _rawMap(rawAudio);
    final subtitle = _rawMap(rawSubtitle);
    final abrPolicy = _rawMap(rawAbr);
    return VesperTrackSelectionSnapshot(
      video: video != null
          ? VesperTrackSelection.fromMap(video)
          : const VesperTrackSelection.auto(),
      audio: audio != null
          ? VesperTrackSelection.fromMap(audio)
          : const VesperTrackSelection.auto(),
      subtitle: subtitle != null
          ? VesperTrackSelection.fromMap(subtitle)
          : const VesperTrackSelection.disabled(),
      confirmedSubtitle: _rawMap(map['confirmedSubtitle']) != null
          ? VesperTrackSelection.fromMap(
              _rawMap(map['confirmedSubtitle'])!,
            )
          : null,
      effectiveSubtitleTrackId: map['effectiveSubtitleTrackId'] as String?,
      abrPolicy: abrPolicy != null
          ? VesperAbrPolicy.fromMap(abrPolicy)
          : const VesperAbrPolicy.auto(),
    );
  }

  final VesperTrackSelection video;
  final VesperTrackSelection audio;
  final VesperTrackSelection subtitle;
  final VesperTrackSelection confirmedSubtitle;
  final String? effectiveSubtitleTrackId;
  final VesperAbrPolicy abrPolicy;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'video': video.toMap(),
      'audio': audio.toMap(),
      'subtitle': subtitle.toMap(),
      'confirmedSubtitle': confirmedSubtitle.toMap(),
      'effectiveSubtitleTrackId': effectiveSubtitleTrackId,
      'abrPolicy': abrPolicy.toMap(),
    };
  }
}

final class VesperTrackPreferencePolicy {
  const VesperTrackPreferencePolicy({
    this.preferredAudioLanguage,
    this.preferredSubtitleLanguage,
    this.selectSubtitlesByDefault = false,
    this.selectUndeterminedSubtitleLanguage = false,
    this.audioSelection = const VesperTrackSelection.auto(),
    this.subtitleSelection = const VesperTrackSelection.disabled(),
    this.abrPolicy = const VesperAbrPolicy.auto(),
  });

  factory VesperTrackPreferencePolicy.fromMap(Map<Object?, Object?> map) {
    final rawAudioSelection = map['audioSelection'];
    final rawSubtitleSelection = map['subtitleSelection'];
    final rawAbrPolicy = map['abrPolicy'];
    final audioSelection = _rawMap(rawAudioSelection);
    final subtitleSelection = _rawMap(rawSubtitleSelection);
    final abrPolicy = _rawMap(rawAbrPolicy);
    return VesperTrackPreferencePolicy(
      preferredAudioLanguage: map['preferredAudioLanguage'] as String?,
      preferredSubtitleLanguage: map['preferredSubtitleLanguage'] as String?,
      selectSubtitlesByDefault: _decodeBool(map, 'selectSubtitlesByDefault'),
      selectUndeterminedSubtitleLanguage: _decodeBool(
        map,
        'selectUndeterminedSubtitleLanguage',
      ),
      audioSelection: audioSelection != null
          ? VesperTrackSelection.fromMap(audioSelection)
          : const VesperTrackSelection.auto(),
      subtitleSelection: subtitleSelection != null
          ? VesperTrackSelection.fromMap(subtitleSelection)
          : const VesperTrackSelection.disabled(),
      abrPolicy: abrPolicy != null
          ? VesperAbrPolicy.fromMap(abrPolicy)
          : const VesperAbrPolicy.auto(),
    );
  }

  final String? preferredAudioLanguage;
  final String? preferredSubtitleLanguage;
  final bool selectSubtitlesByDefault;
  final bool selectUndeterminedSubtitleLanguage;
  final VesperTrackSelection audioSelection;
  final VesperTrackSelection subtitleSelection;
  final VesperAbrPolicy abrPolicy;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      if (preferredAudioLanguage != null)
        'preferredAudioLanguage': preferredAudioLanguage,
      if (preferredSubtitleLanguage != null)
        'preferredSubtitleLanguage': preferredSubtitleLanguage,
      if (selectSubtitlesByDefault)
        'selectSubtitlesByDefault': selectSubtitlesByDefault,
      if (selectUndeterminedSubtitleLanguage)
        'selectUndeterminedSubtitleLanguage':
            selectUndeterminedSubtitleLanguage,
      if (audioSelection.mode != VesperTrackSelectionMode.auto ||
          audioSelection.trackId != null)
        'audioSelection': audioSelection.toMap(),
      if (subtitleSelection.mode != VesperTrackSelectionMode.disabled ||
          subtitleSelection.trackId != null)
        'subtitleSelection': subtitleSelection.toMap(),
      if (abrPolicy.mode != VesperAbrMode.auto ||
          abrPolicy.trackId != null ||
          abrPolicy.maxBitRate != null ||
          abrPolicy.maxWidth != null ||
          abrPolicy.maxHeight != null)
        'abrPolicy': abrPolicy.toMap(),
    };
  }
}
