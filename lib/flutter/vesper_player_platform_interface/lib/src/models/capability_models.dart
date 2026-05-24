part of '../models.dart';

final class VesperPlayerCapabilities {
  const VesperPlayerCapabilities({
    this.supportsLocalFiles = false,
    this.supportsRemoteUrls = false,
    this.supportsHls = false,
    this.supportsDash = false,
    this.supportsDashStaticVod = false,
    this.supportsDashDynamicLive = false,
    this.supportsDashManifestTrackCatalog = false,
    this.supportsDashTextTracks = false,
    this.supportsTrackCatalog = false,
    this.supportsTrackSelection = false,
    this.supportsVideoTrackSelection = false,
    this.supportsAudioTrackSelection = false,
    this.supportsSubtitleTrackSelection = false,
    this.supportsAbrPolicy = false,
    this.supportsAbrConstrained = false,
    this.supportsAbrFixedTrack = false,
    this.supportsExactAbrFixedTrack = false,
    this.supportsAbrMaxBitRate = false,
    this.supportsAbrMaxResolution = false,
    this.supportsResiliencePolicy = false,
    this.supportsHolePunch = false,
    this.supportsPlaybackRate = false,
    this.supportsLiveEdgeSeeking = false,
    this.isExperimental = false,
    this.supportedPlaybackRates = const <double>[],
  });

  const VesperPlayerCapabilities.unsupported()
      : supportsLocalFiles = false,
        supportsRemoteUrls = false,
        supportsHls = false,
        supportsDash = false,
        supportsDashStaticVod = false,
        supportsDashDynamicLive = false,
        supportsDashManifestTrackCatalog = false,
        supportsDashTextTracks = false,
        supportsTrackCatalog = false,
        supportsTrackSelection = false,
        supportsVideoTrackSelection = false,
        supportsAudioTrackSelection = false,
        supportsSubtitleTrackSelection = false,
        supportsAbrPolicy = false,
        supportsAbrConstrained = false,
        supportsAbrFixedTrack = false,
        supportsExactAbrFixedTrack = false,
        supportsAbrMaxBitRate = false,
        supportsAbrMaxResolution = false,
        supportsResiliencePolicy = false,
        supportsHolePunch = false,
        supportsPlaybackRate = false,
        supportsLiveEdgeSeeking = false,
        isExperimental = false,
        supportedPlaybackRates = const <double>[];

  factory VesperPlayerCapabilities.fromMap(Map<Object?, Object?> map) {
    final rawRates = map['supportedPlaybackRates'];
    final rawSupportsTrackSelection = _decodeOptionalBool(
      map,
      'supportsTrackSelection',
    );
    final supportsVideoTrackSelection =
        _decodeOptionalBool(map, 'supportsVideoTrackSelection') ?? false;
    final supportsAudioTrackSelection =
        _decodeOptionalBool(map, 'supportsAudioTrackSelection') ?? false;
    final supportsSubtitleTrackSelection =
        _decodeOptionalBool(map, 'supportsSubtitleTrackSelection') ?? false;
    final supportsTrackSelection = rawSupportsTrackSelection == true ||
        supportsVideoTrackSelection ||
        supportsAudioTrackSelection ||
        supportsSubtitleTrackSelection;

    final rawSupportsAbrPolicy = _decodeOptionalBool(map, 'supportsAbrPolicy');
    final supportsAbrConstrained =
        _decodeOptionalBool(map, 'supportsAbrConstrained') ?? false;
    final supportsAbrFixedTrack =
        _decodeOptionalBool(map, 'supportsAbrFixedTrack') ?? false;
    final supportsAbrPolicy = rawSupportsAbrPolicy == true ||
        supportsAbrConstrained ||
        supportsAbrFixedTrack;
    final supportsAbrMaxBitRate =
        _decodeOptionalBool(map, 'supportsAbrMaxBitRate') ?? false;
    final supportsAbrMaxResolution =
        _decodeOptionalBool(map, 'supportsAbrMaxResolution') ?? false;
    final supportsDashStaticVod =
        _decodeOptionalBool(map, 'supportsDashStaticVod') ?? false;
    final supportsDashDynamicLive =
        _decodeOptionalBool(map, 'supportsDashDynamicLive') ?? false;
    final supportsDashManifestTrackCatalog =
        _decodeOptionalBool(map, 'supportsDashManifestTrackCatalog') ?? false;
    final supportsDashTextTracks =
        _decodeOptionalBool(map, 'supportsDashTextTracks') ?? false;
    final supportsDash = _decodeBool(map, 'supportsDash') ||
        supportsDashStaticVod ||
        supportsDashDynamicLive ||
        supportsDashManifestTrackCatalog ||
        supportsDashTextTracks;

    return VesperPlayerCapabilities(
      supportsLocalFiles: _decodeBool(map, 'supportsLocalFiles'),
      supportsRemoteUrls: _decodeBool(map, 'supportsRemoteUrls'),
      supportsHls: _decodeBool(map, 'supportsHls'),
      supportsDash: supportsDash,
      supportsDashStaticVod: supportsDashStaticVod,
      supportsDashDynamicLive: supportsDashDynamicLive,
      supportsDashManifestTrackCatalog: supportsDashManifestTrackCatalog,
      supportsDashTextTracks: supportsDashTextTracks,
      supportsTrackCatalog: _decodeBool(map, 'supportsTrackCatalog'),
      supportsTrackSelection: supportsTrackSelection,
      supportsVideoTrackSelection: supportsVideoTrackSelection,
      supportsAudioTrackSelection: supportsAudioTrackSelection,
      supportsSubtitleTrackSelection: supportsSubtitleTrackSelection,
      supportsAbrPolicy: supportsAbrPolicy,
      supportsAbrConstrained: supportsAbrConstrained,
      supportsAbrFixedTrack: supportsAbrFixedTrack,
      supportsExactAbrFixedTrack:
          _decodeOptionalBool(map, 'supportsExactAbrFixedTrack') ?? false,
      supportsAbrMaxBitRate: supportsAbrMaxBitRate,
      supportsAbrMaxResolution: supportsAbrMaxResolution,
      supportsResiliencePolicy: _decodeBool(map, 'supportsResiliencePolicy'),
      supportsHolePunch: _decodeBool(map, 'supportsHolePunch'),
      supportsPlaybackRate: _decodeBool(map, 'supportsPlaybackRate'),
      supportsLiveEdgeSeeking: _decodeBool(map, 'supportsLiveEdgeSeeking'),
      isExperimental: _decodeBool(map, 'isExperimental'),
      supportedPlaybackRates: rawRates is Iterable
          ? rawRates
              .map((value) => value is num ? value.toDouble() : null)
              .whereType<double>()
              .toList(growable: false)
          : const <double>[],
    );
  }

  final bool supportsLocalFiles;
  final bool supportsRemoteUrls;
  final bool supportsHls;
  final bool supportsDash;
  final bool supportsDashStaticVod;
  final bool supportsDashDynamicLive;
  final bool supportsDashManifestTrackCatalog;
  final bool supportsDashTextTracks;
  final bool supportsTrackCatalog;
  final bool supportsTrackSelection;
  final bool supportsVideoTrackSelection;
  final bool supportsAudioTrackSelection;
  final bool supportsSubtitleTrackSelection;
  final bool supportsAbrPolicy;
  final bool supportsAbrConstrained;
  final bool supportsAbrFixedTrack;
  final bool supportsExactAbrFixedTrack;
  final bool supportsAbrMaxBitRate;
  final bool supportsAbrMaxResolution;
  final bool supportsResiliencePolicy;
  final bool supportsHolePunch;
  final bool supportsPlaybackRate;
  final bool supportsLiveEdgeSeeking;
  final bool isExperimental;
  final List<double> supportedPlaybackRates;

  bool supportsTrackSelectionFor(VesperMediaTrackKind kind) {
    return switch (kind) {
      VesperMediaTrackKind.video => supportsVideoTrackSelection,
      VesperMediaTrackKind.audio => supportsAudioTrackSelection,
      VesperMediaTrackKind.subtitle => supportsSubtitleTrackSelection,
    };
  }

  bool supportsAbrMode(VesperAbrMode mode) {
    return switch (mode) {
      VesperAbrMode.auto => supportsAbrPolicy,
      VesperAbrMode.constrained => supportsAbrConstrained,
      VesperAbrMode.fixedTrack => supportsAbrFixedTrack,
    };
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'supportsLocalFiles': supportsLocalFiles,
      'supportsRemoteUrls': supportsRemoteUrls,
      'supportsHls': supportsHls,
      'supportsDash': supportsDash,
      'supportsDashStaticVod': supportsDashStaticVod,
      'supportsDashDynamicLive': supportsDashDynamicLive,
      'supportsDashManifestTrackCatalog': supportsDashManifestTrackCatalog,
      'supportsDashTextTracks': supportsDashTextTracks,
      'supportsTrackCatalog': supportsTrackCatalog,
      'supportsTrackSelection': supportsTrackSelection,
      'supportsVideoTrackSelection': supportsVideoTrackSelection,
      'supportsAudioTrackSelection': supportsAudioTrackSelection,
      'supportsSubtitleTrackSelection': supportsSubtitleTrackSelection,
      'supportsAbrPolicy': supportsAbrPolicy,
      'supportsAbrConstrained': supportsAbrConstrained,
      'supportsAbrFixedTrack': supportsAbrFixedTrack,
      'supportsExactAbrFixedTrack': supportsExactAbrFixedTrack,
      'supportsAbrMaxBitRate': supportsAbrMaxBitRate,
      'supportsAbrMaxResolution': supportsAbrMaxResolution,
      'supportsResiliencePolicy': supportsResiliencePolicy,
      'supportsHolePunch': supportsHolePunch,
      'supportsPlaybackRate': supportsPlaybackRate,
      'supportsLiveEdgeSeeking': supportsLiveEdgeSeeking,
      'isExperimental': isExperimental,
      'supportedPlaybackRates': supportedPlaybackRates,
    };
  }
}

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
    };
  }
}

final class VesperTrackCatalog {
  const VesperTrackCatalog({
    this.tracks = const <VesperMediaTrack>[],
    this.adaptiveVideo = false,
    this.adaptiveAudio = false,
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
    );
  }

  final List<VesperMediaTrack> tracks;
  final bool adaptiveVideo;
  final bool adaptiveAudio;

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
    this.abrPolicy = const VesperAbrPolicy.auto(),
  });

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
      abrPolicy: abrPolicy != null
          ? VesperAbrPolicy.fromMap(abrPolicy)
          : const VesperAbrPolicy.auto(),
    );
  }

  final VesperTrackSelection video;
  final VesperTrackSelection audio;
  final VesperTrackSelection subtitle;
  final VesperAbrPolicy abrPolicy;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'video': video.toMap(),
      'audio': audio.toMap(),
      'subtitle': subtitle.toMap(),
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

final class VesperPreloadBudgetPolicy {
  const VesperPreloadBudgetPolicy({
    this.maxConcurrentTasks,
    this.maxMemoryBytes,
    this.maxDiskBytes,
    this.warmupWindowMs,
  });

  factory VesperPreloadBudgetPolicy.fromMap(Map<Object?, Object?> map) {
    return VesperPreloadBudgetPolicy(
      maxConcurrentTasks: _decodeInt(map, 'maxConcurrentTasks'),
      maxMemoryBytes: _decodeInt(map, 'maxMemoryBytes'),
      maxDiskBytes: _decodeInt(map, 'maxDiskBytes'),
      warmupWindowMs: _decodeInt(map, 'warmupWindowMs'),
    );
  }

  final int? maxConcurrentTasks;
  final int? maxMemoryBytes;
  final int? maxDiskBytes;
  final int? warmupWindowMs;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      if (maxConcurrentTasks != null) 'maxConcurrentTasks': maxConcurrentTasks,
      if (maxMemoryBytes != null) 'maxMemoryBytes': maxMemoryBytes,
      if (maxDiskBytes != null) 'maxDiskBytes': maxDiskBytes,
      if (warmupWindowMs != null) 'warmupWindowMs': warmupWindowMs,
    };
  }
}

final class VesperBenchmarkConfiguration {
  const VesperBenchmarkConfiguration({
    this.enabled = false,
    this.maxBufferedEvents = 2048,
    this.includeRawEvents = true,
    this.consoleLogging = false,
    this.pluginLibraryPaths = const <String>[],
  });

  const VesperBenchmarkConfiguration.disabled()
      : enabled = false,
        maxBufferedEvents = 2048,
        includeRawEvents = true,
        consoleLogging = false,
        pluginLibraryPaths = const <String>[];

  factory VesperBenchmarkConfiguration.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperBenchmarkConfiguration(
      enabled: normalized['enabled'] as bool? ?? false,
      maxBufferedEvents: normalized['maxBufferedEvents'] as int? ?? 2048,
      includeRawEvents: normalized['includeRawEvents'] as bool? ?? true,
      consoleLogging: normalized['consoleLogging'] as bool? ?? false,
      pluginLibraryPaths: _decodeStringList(normalized['pluginLibraryPaths']),
    );
  }

  final bool enabled;
  final int maxBufferedEvents;
  final bool includeRawEvents;
  final bool consoleLogging;
  final List<String> pluginLibraryPaths;

  bool get hasOverrides =>
      enabled ||
      maxBufferedEvents != 2048 ||
      !includeRawEvents ||
      consoleLogging ||
      pluginLibraryPaths.isNotEmpty;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'enabled': enabled,
      'maxBufferedEvents': maxBufferedEvents,
      'includeRawEvents': includeRawEvents,
      'consoleLogging': consoleLogging,
      'pluginLibraryPaths': pluginLibraryPaths,
    };
  }
}

final class VesperBufferingPolicy {
  const VesperBufferingPolicy({
    this.preset = VesperBufferingPreset.defaultPreset,
    this.minBufferMs,
    this.maxBufferMs,
    this.bufferForPlaybackMs,
    this.bufferForPlaybackAfterRebufferMs,
  });

  const VesperBufferingPolicy.balanced()
      : preset = VesperBufferingPreset.balanced,
        minBufferMs = null,
        maxBufferMs = null,
        bufferForPlaybackMs = null,
        bufferForPlaybackAfterRebufferMs = null;

  const VesperBufferingPolicy.streaming()
      : preset = VesperBufferingPreset.streaming,
        minBufferMs = null,
        maxBufferMs = null,
        bufferForPlaybackMs = null,
        bufferForPlaybackAfterRebufferMs = null;

  const VesperBufferingPolicy.resilient()
      : preset = VesperBufferingPreset.resilient,
        minBufferMs = null,
        maxBufferMs = null,
        bufferForPlaybackMs = null,
        bufferForPlaybackAfterRebufferMs = null;

  const VesperBufferingPolicy.lowLatency()
      : preset = VesperBufferingPreset.lowLatency,
        minBufferMs = null,
        maxBufferMs = null,
        bufferForPlaybackMs = null,
        bufferForPlaybackAfterRebufferMs = null;

  factory VesperBufferingPolicy.fromMap(Map<Object?, Object?> map) {
    return VesperBufferingPolicy(
      preset: _decodeEnum(
        VesperBufferingPreset.values,
        map['preset'],
        VesperBufferingPreset.defaultPreset,
      ),
      minBufferMs: _decodeInt(map, 'minBufferMs'),
      maxBufferMs: _decodeInt(map, 'maxBufferMs'),
      bufferForPlaybackMs: _decodeInt(map, 'bufferForPlaybackMs'),
      bufferForPlaybackAfterRebufferMs: _decodeInt(
        map,
        'bufferForPlaybackAfterRebufferMs',
      ),
    );
  }

  final VesperBufferingPreset preset;
  final int? minBufferMs;
  final int? maxBufferMs;
  final int? bufferForPlaybackMs;
  final int? bufferForPlaybackAfterRebufferMs;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'preset': preset.name,
      'minBufferMs': minBufferMs,
      'maxBufferMs': maxBufferMs,
      'bufferForPlaybackMs': bufferForPlaybackMs,
      'bufferForPlaybackAfterRebufferMs': bufferForPlaybackAfterRebufferMs,
    };
  }
}

final class VesperRetryPolicy {
  const VesperRetryPolicy({
    Object? maxAttempts = _vesperRetryMaxAttemptsUnset,
    int? baseDelayMs,
    int? maxDelayMs,
    VesperRetryBackoff? backoff,
  })  : _maxAttempts = maxAttempts,
        _baseDelayMs = baseDelayMs,
        _maxDelayMs = maxDelayMs,
        _backoff = backoff;

  const VesperRetryPolicy.aggressive()
      : _maxAttempts = 2,
        _baseDelayMs = 500,
        _maxDelayMs = 2000,
        _backoff = VesperRetryBackoff.fixed;

  const VesperRetryPolicy.resilient()
      : _maxAttempts = 6,
        _baseDelayMs = 1000,
        _maxDelayMs = 8000,
        _backoff = VesperRetryBackoff.exponential;

  factory VesperRetryPolicy.fromMap(Map<Object?, Object?> map) {
    final rawMaxAttempts = map['maxAttempts'];
    final maxAttempts = switch (rawMaxAttempts) {
      int value => value,
      null when map.containsKey('maxAttempts') => null,
      _ => _vesperRetryMaxAttemptsUnset,
    };
    return VesperRetryPolicy(
      maxAttempts: maxAttempts,
      baseDelayMs: _decodeInt(map, 'baseDelayMs'),
      maxDelayMs: _decodeInt(map, 'maxDelayMs'),
      backoff: switch (map['backoff']) {
        'fixed' => VesperRetryBackoff.fixed,
        'linear' => VesperRetryBackoff.linear,
        'exponential' => VesperRetryBackoff.exponential,
        _ => null,
      },
    );
  }

  final Object? _maxAttempts;
  final int? _baseDelayMs;
  final int? _maxDelayMs;
  final VesperRetryBackoff? _backoff;

  int? get maxAttempts => switch (_maxAttempts) {
        _vesperRetryMaxAttemptsUnset => 3,
        int value => value,
        null => null,
        _ => 3,
      };

  bool get hasMaxAttemptsOverride =>
      !identical(_maxAttempts, _vesperRetryMaxAttemptsUnset);

  int get baseDelayMs => _baseDelayMs ?? 1000;
  int get maxDelayMs => _maxDelayMs ?? 5000;
  VesperRetryBackoff get backoff => _backoff ?? VesperRetryBackoff.linear;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      if (hasMaxAttemptsOverride) 'maxAttempts': _maxAttempts as int?,
      'baseDelayMs': _baseDelayMs,
      'maxDelayMs': _maxDelayMs,
      'backoff': _backoff?.name,
    };
  }
}

final class VesperCachePolicy {
  const VesperCachePolicy({
    this.preset = VesperCachePreset.defaultPreset,
    this.maxMemoryBytes,
    this.maxDiskBytes,
  });

  const VesperCachePolicy.disabled()
      : preset = VesperCachePreset.disabled,
        maxMemoryBytes = null,
        maxDiskBytes = null;

  const VesperCachePolicy.streaming()
      : preset = VesperCachePreset.streaming,
        maxMemoryBytes = null,
        maxDiskBytes = null;

  const VesperCachePolicy.resilient()
      : preset = VesperCachePreset.resilient,
        maxMemoryBytes = null,
        maxDiskBytes = null;

  factory VesperCachePolicy.fromMap(Map<Object?, Object?> map) {
    return VesperCachePolicy(
      preset: _decodeEnum(
        VesperCachePreset.values,
        map['preset'],
        VesperCachePreset.defaultPreset,
      ),
      maxMemoryBytes: _decodeInt(map, 'maxMemoryBytes'),
      maxDiskBytes: _decodeInt(map, 'maxDiskBytes'),
    );
  }

  final VesperCachePreset preset;
  final int? maxMemoryBytes;
  final int? maxDiskBytes;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'preset': preset.name,
      'maxMemoryBytes': maxMemoryBytes,
      'maxDiskBytes': maxDiskBytes,
    };
  }
}

final class VesperPlaybackResiliencePolicy {
  const VesperPlaybackResiliencePolicy({
    this.buffering = const VesperBufferingPolicy(),
    this.retry = const VesperRetryPolicy(),
    this.cache = const VesperCachePolicy(),
  });

  const VesperPlaybackResiliencePolicy.balanced()
      : buffering = const VesperBufferingPolicy.balanced(),
        retry = const VesperRetryPolicy(),
        cache = const VesperCachePolicy.streaming();

  const VesperPlaybackResiliencePolicy.streaming()
      : buffering = const VesperBufferingPolicy.streaming(),
        retry = const VesperRetryPolicy(),
        cache = const VesperCachePolicy.streaming();

  const VesperPlaybackResiliencePolicy.resilient()
      : buffering = const VesperBufferingPolicy.resilient(),
        retry = const VesperRetryPolicy.resilient(),
        cache = const VesperCachePolicy.resilient();

  const VesperPlaybackResiliencePolicy.lowLatency()
      : buffering = const VesperBufferingPolicy.lowLatency(),
        retry = const VesperRetryPolicy.aggressive(),
        cache = const VesperCachePolicy.disabled();

  factory VesperPlaybackResiliencePolicy.fromMap(Map<Object?, Object?> map) {
    final rawBuffering = map['buffering'];
    final rawRetry = map['retry'];
    final rawCache = map['cache'];
    final buffering = _rawMap(rawBuffering);
    final retry = _rawMap(rawRetry);
    final cache = _rawMap(rawCache);
    return VesperPlaybackResiliencePolicy(
      buffering: buffering != null
          ? VesperBufferingPolicy.fromMap(buffering)
          : const VesperBufferingPolicy(),
      retry: retry != null
          ? VesperRetryPolicy.fromMap(retry)
          : const VesperRetryPolicy(),
      cache: cache != null
          ? VesperCachePolicy.fromMap(cache)
          : const VesperCachePolicy(),
    );
  }

  final VesperBufferingPolicy buffering;
  final VesperRetryPolicy retry;
  final VesperCachePolicy cache;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'buffering': buffering.toMap(),
      'retry': retry.toMap(),
      'cache': cache.toMap(),
    };
  }
}

