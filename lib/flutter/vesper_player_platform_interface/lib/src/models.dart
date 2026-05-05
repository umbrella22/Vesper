import 'dart:math' as math;

enum VesperPlayerSourceKind { local, remote }

enum VesperPlayerSourceProtocol {
  unknown,
  file,
  content,
  progressive,
  hls,
  dash,
}

enum VesperPlaybackState { ready, playing, paused, finished }

enum VesperTimelineKind { vod, live, liveDvr }

enum VesperPlayerBackendFamily {
  unknown,
  androidHostKit,
  iosHostKit,
  macosFfi,
  softwareFallback,
  fakeDemo,
}

enum VesperPlayerRenderSurfaceKind { auto, textureView, surfaceView }

enum VesperBackgroundPlaybackMode { disabled, continueAudio }

enum VesperSystemPlaybackPermissionStatus { notRequired, granted, denied }

enum VesperExternalPlaybackRouteKind { none, airPlay, cast }

enum VesperMediaTrackKind { video, audio, subtitle }

enum VesperTrackSelectionMode { auto, disabled, track }

enum VesperAbrMode { auto, constrained, fixedTrack }

enum VesperFixedTrackStatus { pending, locked, fallback }

final class VesperVideoVariantObservation {
  const VesperVideoVariantObservation({
    this.bitRate,
    this.width,
    this.height,
  });

  factory VesperVideoVariantObservation.fromMap(Map<Object?, Object?> map) {
    return VesperVideoVariantObservation(
      bitRate: _decodeInt(map, 'bitRate'),
      width: _decodeInt(map, 'width'),
      height: _decodeInt(map, 'height'),
    );
  }

  final int? bitRate;
  final int? width;
  final int? height;

  bool get hasSignal => bitRate != null || (width != null && height != null);

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'bitRate': bitRate,
      'width': width,
      'height': height,
    };
  }
}

final class VesperSystemPlaybackMetadata {
  const VesperSystemPlaybackMetadata({
    required this.title,
    this.artist,
    this.albumTitle,
    this.artworkUri,
    this.contentUri,
    this.durationMs,
    this.isLive = false,
  });

  factory VesperSystemPlaybackMetadata.fromMap(Map<Object?, Object?> map) {
    return VesperSystemPlaybackMetadata(
      title: map['title'] as String? ?? '',
      artist: map['artist'] as String?,
      albumTitle: map['albumTitle'] as String?,
      artworkUri: map['artworkUri'] as String?,
      contentUri: map['contentUri'] as String?,
      durationMs: _decodeInt(map, 'durationMs'),
      isLive: _decodeBool(map, 'isLive'),
    );
  }

  final String title;
  final String? artist;
  final String? albumTitle;
  final String? artworkUri;
  final String? contentUri;
  final int? durationMs;
  final bool isLive;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'title': title,
      'artist': artist,
      'albumTitle': albumTitle,
      'artworkUri': artworkUri,
      'contentUri': contentUri,
      'durationMs': durationMs,
      'isLive': isLive,
    };
  }
}

final class VesperSystemPlaybackConfiguration {
  const VesperSystemPlaybackConfiguration({
    this.enabled = true,
    this.backgroundMode = VesperBackgroundPlaybackMode.continueAudio,
    this.showSystemControls = true,
    this.showSeekActions = true,
    this.metadata,
  });

  factory VesperSystemPlaybackConfiguration.fromMap(
    Map<Object?, Object?> map,
  ) {
    final rawMetadata = _rawMap(map['metadata']);
    return VesperSystemPlaybackConfiguration(
      enabled: _decodeBool(map, 'enabled', fallback: true),
      backgroundMode: _decodeEnum(
        VesperBackgroundPlaybackMode.values,
        map['backgroundMode'],
        VesperBackgroundPlaybackMode.continueAudio,
      ),
      showSystemControls: _decodeBool(
        map,
        'showSystemControls',
        fallback: true,
      ),
      showSeekActions: _decodeBool(map, 'showSeekActions', fallback: true),
      metadata: rawMetadata == null
          ? null
          : VesperSystemPlaybackMetadata.fromMap(rawMetadata),
    );
  }

  final bool enabled;
  final VesperBackgroundPlaybackMode backgroundMode;
  final bool showSystemControls;
  final bool showSeekActions;
  final VesperSystemPlaybackMetadata? metadata;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'enabled': enabled,
      'backgroundMode': backgroundMode.name,
      'showSystemControls': showSystemControls,
      'showSeekActions': showSeekActions,
      'metadata': metadata?.toMap(),
    };
  }
}

final class VesperExternalPlaybackRouteSnapshot {
  const VesperExternalPlaybackRouteSnapshot({
    this.kind = VesperExternalPlaybackRouteKind.none,
    this.routeId,
    this.routeName,
    this.active = false,
    this.available = false,
  });

  factory VesperExternalPlaybackRouteSnapshot.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperExternalPlaybackRouteSnapshot(
      kind: _decodeEnum(
        VesperExternalPlaybackRouteKind.values,
        map['kind'],
        VesperExternalPlaybackRouteKind.none,
      ),
      routeId: map['routeId'] as String?,
      routeName: map['routeName'] as String?,
      active: _decodeBool(map, 'active'),
      available: _decodeBool(map, 'available'),
    );
  }

  final VesperExternalPlaybackRouteKind kind;
  final String? routeId;
  final String? routeName;
  final bool active;
  final bool available;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'kind': kind.name,
      'routeId': routeId,
      'routeName': routeName,
      'active': active,
      'available': available,
    };
  }
}

final class VesperExternalPlaybackAvailability {
  const VesperExternalPlaybackAvailability({
    this.airPlayAvailable = false,
    this.castAvailable = false,
    this.activeRoute = const VesperExternalPlaybackRouteSnapshot(),
  });

  factory VesperExternalPlaybackAvailability.fromMap(
    Map<Object?, Object?> map,
  ) {
    final rawRoute = _rawMap(map['activeRoute']);
    return VesperExternalPlaybackAvailability(
      airPlayAvailable: _decodeBool(map, 'airPlayAvailable'),
      castAvailable: _decodeBool(map, 'castAvailable'),
      activeRoute: rawRoute == null
          ? const VesperExternalPlaybackRouteSnapshot()
          : VesperExternalPlaybackRouteSnapshot.fromMap(rawRoute),
    );
  }

  final bool airPlayAvailable;
  final bool castAvailable;
  final VesperExternalPlaybackRouteSnapshot activeRoute;

  bool get hasAvailableRoute => airPlayAvailable || castAvailable;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'airPlayAvailable': airPlayAvailable,
      'castAvailable': castAvailable,
      'activeRoute': activeRoute.toMap(),
    };
  }
}

final class VesperRoutePickerConfiguration {
  const VesperRoutePickerConfiguration({
    this.prioritizesVideoDevices = true,
  });

  factory VesperRoutePickerConfiguration.fromMap(Map<Object?, Object?> map) {
    return VesperRoutePickerConfiguration(
      prioritizesVideoDevices: _decodeBool(
        map,
        'prioritizesVideoDevices',
        fallback: true,
      ),
    );
  }

  final bool prioritizesVideoDevices;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'prioritizesVideoDevices': prioritizesVideoDevices,
    };
  }
}

enum VesperBufferingPreset {
  defaultPreset,
  balanced,
  streaming,
  resilient,
  lowLatency,
}

enum VesperRetryBackoff { fixed, linear, exponential }

enum VesperCachePreset { defaultPreset, disabled, streaming, resilient }

enum VesperPlayerErrorCategory {
  input,
  source,
  network,
  decode,
  audioOutput,
  playback,
  capability,
  platform,
  unsupported,
}

T _decodeEnum<T extends Enum>(Iterable<T> values, Object? raw, T fallback) {
  if (raw is! String) {
    return fallback;
  }
  for (final value in values) {
    if (value.name == raw) {
      return value;
    }
  }
  return fallback;
}

bool _decodeBool(
  Map<Object?, Object?> map,
  String key, {
  bool fallback = false,
}) {
  final raw = map[key];
  return raw is bool ? raw : fallback;
}

bool? _decodeOptionalBool(Map<Object?, Object?> map, String key) {
  final raw = map[key];
  return raw is bool ? raw : null;
}

int? _decodeInt(Map<Object?, Object?> map, String key) {
  final raw = map[key];
  return raw is int ? raw : null;
}

double? _decodeDouble(Map<Object?, Object?> map, String key) {
  final raw = map[key];
  if (raw is double) {
    return raw;
  }
  if (raw is int) {
    return raw.toDouble();
  }
  return null;
}

Map<String, Object?> _stringKeyedMap(Map<Object?, Object?> source) {
  return source.map((key, value) => MapEntry(key.toString(), value));
}

Map<Object?, Object?>? _rawMap(Object? raw) {
  if (raw is Map<Object?, Object?>) {
    return raw;
  }
  if (raw is Map) {
    return Map<Object?, Object?>.from(raw);
  }
  return null;
}

Map<String, String> _decodeStringMap(Object? raw) {
  final map = _rawMap(raw);
  if (map == null || map.isEmpty) {
    return const <String, String>{};
  }

  final decoded = <String, String>{};
  for (final entry in map.entries) {
    final key = entry.key;
    final value = entry.value;
    if (key is String && value is String) {
      decoded[key] = value;
    }
  }
  return decoded;
}

List<String> _decodeStringList(Object? raw) {
  if (raw is! Iterable) {
    return const <String>[];
  }
  return raw
      .map((value) => value?.toString() ?? '')
      .where((value) => value.isNotEmpty)
      .toList(growable: false);
}

const Object _vesperRetryMaxAttemptsUnset = Object();

final class VesperPlayerSource {
  const VesperPlayerSource({
    required this.uri,
    required this.label,
    required this.kind,
    required this.protocol,
    this.headers = const <String, String>{},
  });

  factory VesperPlayerSource.local({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
  }) {
    return VesperPlayerSource(
      uri: uri,
      label: label ?? uri,
      kind: VesperPlayerSourceKind.local,
      protocol: _inferLocalProtocol(uri),
      headers: headers,
    );
  }

  factory VesperPlayerSource.remote({
    required String uri,
    String? label,
    VesperPlayerSourceProtocol? protocol,
    Map<String, String> headers = const <String, String>{},
  }) {
    return VesperPlayerSource(
      uri: uri,
      label: label ?? uri,
      kind: VesperPlayerSourceKind.remote,
      protocol: protocol ?? _inferRemoteProtocol(uri),
      headers: headers,
    );
  }

  factory VesperPlayerSource.hls({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
  }) {
    return VesperPlayerSource.remote(
      uri: uri,
      label: label,
      protocol: VesperPlayerSourceProtocol.hls,
      headers: headers,
    );
  }

  factory VesperPlayerSource.dash({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
  }) {
    return VesperPlayerSource.remote(
      uri: uri,
      label: label,
      protocol: VesperPlayerSourceProtocol.dash,
      headers: headers,
    );
  }

  factory VesperPlayerSource.fromMap(Map<Object?, Object?> map) {
    final uri = map['uri'] as String? ?? '';
    return VesperPlayerSource(
      uri: uri,
      label: map['label'] as String? ?? uri,
      kind: _decodeEnum(
        VesperPlayerSourceKind.values,
        map['kind'],
        uri.startsWith('http://') || uri.startsWith('https://')
            ? VesperPlayerSourceKind.remote
            : VesperPlayerSourceKind.local,
      ),
      protocol: _decodeEnum(
        VesperPlayerSourceProtocol.values,
        map['protocol'],
        VesperPlayerSourceProtocol.unknown,
      ),
      headers: _decodeStringMap(map['headers']),
    );
  }

  final String uri;
  final String label;
  final VesperPlayerSourceKind kind;
  final VesperPlayerSourceProtocol protocol;
  final Map<String, String> headers;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'uri': uri,
      'label': label,
      'kind': kind.name,
      'protocol': protocol.name,
      'headers': headers,
    };
  }

  static VesperPlayerSourceProtocol _inferLocalProtocol(String uri) {
    final normalized = uri.toLowerCase();
    if (normalized.startsWith('content://')) {
      return VesperPlayerSourceProtocol.content;
    }
    if (normalized.startsWith('file://')) {
      return VesperPlayerSourceProtocol.file;
    }
    return VesperPlayerSourceProtocol.unknown;
  }

  static VesperPlayerSourceProtocol _inferRemoteProtocol(String uri) {
    final normalized = uri.toLowerCase();
    final withoutQuery = normalized.split('#').first.split('?').first;
    if (withoutQuery.endsWith('.m3u8')) {
      return VesperPlayerSourceProtocol.hls;
    }
    if (withoutQuery.endsWith('.mpd')) {
      return VesperPlayerSourceProtocol.dash;
    }
    if (normalized.startsWith('http://') || normalized.startsWith('https://')) {
      return VesperPlayerSourceProtocol.progressive;
    }
    return VesperPlayerSourceProtocol.unknown;
  }
}

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
    return VesperTimeline(
      kind: _decodeEnum(
        VesperTimelineKind.values,
        map['kind'],
        VesperTimelineKind.vod,
      ),
      isSeekable: _decodeBool(map, 'isSeekable'),
      seekableRange: _rawMap(rawRange) != null
          ? VesperSeekableRange.fromMap(_rawMap(rawRange)!)
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
    return VesperTrackSelectionSnapshot(
      video: _rawMap(rawVideo) != null
          ? VesperTrackSelection.fromMap(_rawMap(rawVideo)!)
          : const VesperTrackSelection.auto(),
      audio: _rawMap(rawAudio) != null
          ? VesperTrackSelection.fromMap(_rawMap(rawAudio)!)
          : const VesperTrackSelection.auto(),
      subtitle: _rawMap(rawSubtitle) != null
          ? VesperTrackSelection.fromMap(_rawMap(rawSubtitle)!)
          : const VesperTrackSelection.disabled(),
      abrPolicy: _rawMap(rawAbr) != null
          ? VesperAbrPolicy.fromMap(_rawMap(rawAbr)!)
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
    return VesperTrackPreferencePolicy(
      preferredAudioLanguage: map['preferredAudioLanguage'] as String?,
      preferredSubtitleLanguage: map['preferredSubtitleLanguage'] as String?,
      selectSubtitlesByDefault: _decodeBool(map, 'selectSubtitlesByDefault'),
      selectUndeterminedSubtitleLanguage: _decodeBool(
        map,
        'selectUndeterminedSubtitleLanguage',
      ),
      audioSelection: _rawMap(rawAudioSelection) != null
          ? VesperTrackSelection.fromMap(_rawMap(rawAudioSelection)!)
          : const VesperTrackSelection.auto(),
      subtitleSelection: _rawMap(rawSubtitleSelection) != null
          ? VesperTrackSelection.fromMap(_rawMap(rawSubtitleSelection)!)
          : const VesperTrackSelection.disabled(),
      abrPolicy: _rawMap(rawAbrPolicy) != null
          ? VesperAbrPolicy.fromMap(_rawMap(rawAbrPolicy)!)
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
    return VesperRetryPolicy(
      maxAttempts: map.containsKey('maxAttempts')
          ? switch (rawMaxAttempts) {
              int value => value,
              null => null,
              _ => _vesperRetryMaxAttemptsUnset,
            }
          : _vesperRetryMaxAttemptsUnset,
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
    return VesperPlaybackResiliencePolicy(
      buffering: _rawMap(rawBuffering) != null
          ? VesperBufferingPolicy.fromMap(_rawMap(rawBuffering)!)
          : const VesperBufferingPolicy(),
      retry: _rawMap(rawRetry) != null
          ? VesperRetryPolicy.fromMap(_rawMap(rawRetry)!)
          : const VesperRetryPolicy(),
      cache: _rawMap(rawCache) != null
          ? VesperCachePolicy.fromMap(_rawMap(rawCache)!)
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

final class VesperPlayerViewport {
  const VesperPlayerViewport({
    required this.left,
    required this.top,
    required this.width,
    required this.height,
  });

  factory VesperPlayerViewport.fromMap(Map<Object?, Object?> map) {
    return VesperPlayerViewport(
      left: _decodeDouble(map, 'left') ?? 0,
      top: _decodeDouble(map, 'top') ?? 0,
      width: _decodeDouble(map, 'width') ?? 0,
      height: _decodeDouble(map, 'height') ?? 0,
    );
  }

  final double left;
  final double top;
  final double width;
  final double height;

  bool get isEmpty => width <= 0 || height <= 0;

  VesperViewportHint classifyHint({
    required double surfaceWidth,
    required double surfaceHeight,
  }) {
    if (isEmpty || surfaceWidth <= 0 || surfaceHeight <= 0) {
      return const VesperViewportHint.hidden();
    }

    final right = left + width;
    final bottom = top + height;
    final visibleWidth = _overlapExtent(left, right, 0, surfaceWidth);
    final visibleHeight = _overlapExtent(top, bottom, 0, surfaceHeight);
    final visibleArea = visibleWidth * visibleHeight;
    final totalArea = width * height;
    final visibleFraction =
        totalArea <= 0 ? 0.0 : _clampUnit(visibleArea / totalArea);

    if (visibleArea > 0) {
      return VesperViewportHint(
        kind: VesperViewportHintKind.visible,
        visibleFraction: visibleFraction,
      );
    }

    final dx = _axisGap(left, right, 0, surfaceWidth);
    final dy = _axisGap(top, bottom, 0, surfaceHeight);
    final edgeDistance = math.sqrt(dx * dx + dy * dy);
    final reference = math.max(surfaceWidth, surfaceHeight);
    final kind = edgeDistance <= reference * 0.5
        ? VesperViewportHintKind.nearVisible
        : edgeDistance <= reference * 1.5
            ? VesperViewportHintKind.prefetchOnly
            : VesperViewportHintKind.hidden;

    return VesperViewportHint(kind: kind, visibleFraction: 0);
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'left': left,
      'top': top,
      'width': width,
      'height': height,
    };
  }
}

enum VesperViewportHintKind { visible, nearVisible, prefetchOnly, hidden }

final class VesperViewportHint {
  const VesperViewportHint({
    required this.kind,
    this.visibleFraction = 0,
  });

  const VesperViewportHint.hidden()
      : kind = VesperViewportHintKind.hidden,
        visibleFraction = 0;

  factory VesperViewportHint.fromMap(Map<Object?, Object?> map) {
    return VesperViewportHint(
      kind: _decodeEnum(
        VesperViewportHintKind.values,
        map['kind'],
        VesperViewportHintKind.hidden,
      ),
      visibleFraction: _clampUnit(_decodeDouble(map, 'visibleFraction') ?? 0),
    );
  }

  final VesperViewportHintKind kind;
  final double visibleFraction;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'kind': kind.name,
      'visibleFraction': visibleFraction,
    };
  }
}

final class VesperPlayerError {
  const VesperPlayerError({
    required this.message,
    this.code,
    this.category = VesperPlayerErrorCategory.platform,
    this.retriable = false,
  });

  factory VesperPlayerError.unsupported([String? message]) {
    return VesperPlayerError(
      message: message ?? 'Vesper player is not supported on this platform.',
      category: VesperPlayerErrorCategory.unsupported,
    );
  }

  factory VesperPlayerError.fromMap(Map<Object?, Object?> map) {
    return VesperPlayerError(
      message: map['message'] as String? ?? 'Unknown Vesper player error.',
      code: map['code'] as String?,
      category: _decodeEnum(
        VesperPlayerErrorCategory.values,
        map['category'],
        VesperPlayerErrorCategory.platform,
      ),
      retriable: _decodeBool(map, 'retriable'),
    );
  }

  final String message;
  final String? code;
  final VesperPlayerErrorCategory category;
  final bool retriable;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'message': message,
      'code': code,
      'category': category.name,
      'retriable': retriable,
    };
  }
}

final class VesperPlayerSnapshot {
  const VesperPlayerSnapshot({
    required this.title,
    required this.subtitle,
    required this.sourceLabel,
    required this.playbackState,
    required this.playbackRate,
    required this.isBuffering,
    required this.isInterrupted,
    required this.hasVideoSurface,
    required this.timeline,
    this.viewport,
    this.viewportHint = const VesperViewportHint.hidden(),
    this.backendFamily = VesperPlayerBackendFamily.unknown,
    this.capabilities = const VesperPlayerCapabilities.unsupported(),
    this.trackCatalog = const VesperTrackCatalog(),
    this.trackSelection = const VesperTrackSelectionSnapshot(),
    this.effectiveVideoTrackId,
    this.videoVariantObservation,
    this.fixedTrackStatus,
    this.resiliencePolicy = const VesperPlaybackResiliencePolicy(),
    this.lastError,
  });

  const VesperPlayerSnapshot.initial()
      : title = 'Vesper',
        subtitle = 'Player is not initialized.',
        sourceLabel = '',
        playbackState = VesperPlaybackState.ready,
        playbackRate = 1.0,
        isBuffering = false,
        isInterrupted = false,
        hasVideoSurface = false,
        timeline = const VesperTimeline.initial(),
        viewport = null,
        viewportHint = const VesperViewportHint.hidden(),
        backendFamily = VesperPlayerBackendFamily.unknown,
        capabilities = const VesperPlayerCapabilities.unsupported(),
        trackCatalog = const VesperTrackCatalog(),
        trackSelection = const VesperTrackSelectionSnapshot(),
        effectiveVideoTrackId = null,
        videoVariantObservation = null,
        fixedTrackStatus = null,
        resiliencePolicy = const VesperPlaybackResiliencePolicy(),
        lastError = null;

  factory VesperPlayerSnapshot.fromMap(Map<Object?, Object?> map) {
    final rawTimeline = map['timeline'];
    final rawCapabilities = map['capabilities'];
    final rawTrackCatalog = map['trackCatalog'];
    final rawTrackSelection = map['trackSelection'];
    final rawEffectiveVideoTrackId = map['effectiveVideoTrackId'];
    final rawVideoVariantObservation = map['videoVariantObservation'];
    final rawFixedTrackStatus = map['fixedTrackStatus'];
    final rawResiliencePolicy = map['resiliencePolicy'];
    final rawViewport = map['viewport'];
    final rawViewportHint = map['viewportHint'];
    final rawLastError = map['lastError'];
    return VesperPlayerSnapshot(
      title: map['title'] as String? ?? 'Vesper',
      subtitle: map['subtitle'] as String? ?? '',
      sourceLabel: map['sourceLabel'] as String? ?? '',
      playbackState: _decodeEnum(
        VesperPlaybackState.values,
        map['playbackState'],
        VesperPlaybackState.ready,
      ),
      playbackRate: _decodeDouble(map, 'playbackRate') ?? 1.0,
      isBuffering: _decodeBool(map, 'isBuffering'),
      isInterrupted: _decodeBool(map, 'isInterrupted'),
      hasVideoSurface: _decodeBool(map, 'hasVideoSurface'),
      timeline: _rawMap(rawTimeline) != null
          ? VesperTimeline.fromMap(_rawMap(rawTimeline)!)
          : const VesperTimeline.initial(),
      viewport: _rawMap(rawViewport) != null
          ? VesperPlayerViewport.fromMap(_rawMap(rawViewport)!)
          : null,
      viewportHint: _rawMap(rawViewportHint) != null
          ? VesperViewportHint.fromMap(_rawMap(rawViewportHint)!)
          : const VesperViewportHint.hidden(),
      backendFamily: _decodeEnum(
        VesperPlayerBackendFamily.values,
        map['backendFamily'],
        VesperPlayerBackendFamily.unknown,
      ),
      capabilities: _rawMap(rawCapabilities) != null
          ? VesperPlayerCapabilities.fromMap(_rawMap(rawCapabilities)!)
          : const VesperPlayerCapabilities.unsupported(),
      trackCatalog: _rawMap(rawTrackCatalog) != null
          ? VesperTrackCatalog.fromMap(_rawMap(rawTrackCatalog)!)
          : const VesperTrackCatalog(),
      trackSelection: _rawMap(rawTrackSelection) != null
          ? VesperTrackSelectionSnapshot.fromMap(_rawMap(rawTrackSelection)!)
          : const VesperTrackSelectionSnapshot(),
      effectiveVideoTrackId: rawEffectiveVideoTrackId as String?,
      videoVariantObservation: _rawMap(rawVideoVariantObservation) != null
          ? VesperVideoVariantObservation.fromMap(
              _rawMap(rawVideoVariantObservation)!,
            )
          : null,
      fixedTrackStatus: rawFixedTrackStatus is String
          ? _decodeEnum(
              VesperFixedTrackStatus.values,
              rawFixedTrackStatus,
              VesperFixedTrackStatus.pending,
            )
          : null,
      resiliencePolicy: _rawMap(rawResiliencePolicy) != null
          ? VesperPlaybackResiliencePolicy.fromMap(
              _rawMap(rawResiliencePolicy)!,
            )
          : const VesperPlaybackResiliencePolicy(),
      lastError: _rawMap(rawLastError) != null
          ? VesperPlayerError.fromMap(_rawMap(rawLastError)!)
          : null,
    );
  }

  final String title;
  final String subtitle;
  final String sourceLabel;
  final VesperPlaybackState playbackState;
  final double playbackRate;
  final bool isBuffering;
  final bool isInterrupted;
  final bool hasVideoSurface;
  final VesperTimeline timeline;
  final VesperPlayerViewport? viewport;
  final VesperViewportHint viewportHint;
  final VesperPlayerBackendFamily backendFamily;
  final VesperPlayerCapabilities capabilities;
  final VesperTrackCatalog trackCatalog;
  final VesperTrackSelectionSnapshot trackSelection;
  final String? effectiveVideoTrackId;
  final VesperVideoVariantObservation? videoVariantObservation;
  final VesperFixedTrackStatus? fixedTrackStatus;
  final VesperPlaybackResiliencePolicy resiliencePolicy;
  final VesperPlayerError? lastError;

  VesperPlayerSnapshot copyWith({
    String? title,
    String? subtitle,
    String? sourceLabel,
    VesperPlaybackState? playbackState,
    double? playbackRate,
    bool? isBuffering,
    bool? isInterrupted,
    bool? hasVideoSurface,
    VesperTimeline? timeline,
    VesperPlayerViewport? viewport,
    VesperViewportHint? viewportHint,
    VesperPlayerBackendFamily? backendFamily,
    VesperPlayerCapabilities? capabilities,
    VesperTrackCatalog? trackCatalog,
    VesperTrackSelectionSnapshot? trackSelection,
    String? effectiveVideoTrackId,
    bool clearEffectiveVideoTrackId = false,
    VesperVideoVariantObservation? videoVariantObservation,
    bool clearVideoVariantObservation = false,
    VesperFixedTrackStatus? fixedTrackStatus,
    bool clearFixedTrackStatus = false,
    VesperPlaybackResiliencePolicy? resiliencePolicy,
    VesperPlayerError? lastError,
    bool clearLastError = false,
  }) {
    return VesperPlayerSnapshot(
      title: title ?? this.title,
      subtitle: subtitle ?? this.subtitle,
      sourceLabel: sourceLabel ?? this.sourceLabel,
      playbackState: playbackState ?? this.playbackState,
      playbackRate: playbackRate ?? this.playbackRate,
      isBuffering: isBuffering ?? this.isBuffering,
      isInterrupted: isInterrupted ?? this.isInterrupted,
      hasVideoSurface: hasVideoSurface ?? this.hasVideoSurface,
      timeline: timeline ?? this.timeline,
      viewport: viewport ?? this.viewport,
      viewportHint: viewportHint ?? this.viewportHint,
      backendFamily: backendFamily ?? this.backendFamily,
      capabilities: capabilities ?? this.capabilities,
      trackCatalog: trackCatalog ?? this.trackCatalog,
      trackSelection: trackSelection ?? this.trackSelection,
      effectiveVideoTrackId: clearEffectiveVideoTrackId
          ? null
          : (effectiveVideoTrackId ?? this.effectiveVideoTrackId),
      videoVariantObservation: clearVideoVariantObservation
          ? null
          : (videoVariantObservation ?? this.videoVariantObservation),
      fixedTrackStatus: clearFixedTrackStatus
          ? null
          : (fixedTrackStatus ?? this.fixedTrackStatus),
      resiliencePolicy: resiliencePolicy ?? this.resiliencePolicy,
      lastError: clearLastError ? null : (lastError ?? this.lastError),
    );
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'title': title,
      'subtitle': subtitle,
      'sourceLabel': sourceLabel,
      'playbackState': playbackState.name,
      'playbackRate': playbackRate,
      'isBuffering': isBuffering,
      'isInterrupted': isInterrupted,
      'hasVideoSurface': hasVideoSurface,
      'timeline': timeline.toMap(),
      'viewport': viewport?.toMap(),
      'viewportHint': viewportHint.toMap(),
      'backendFamily': backendFamily.name,
      'capabilities': capabilities.toMap(),
      'trackCatalog': trackCatalog.toMap(),
      'trackSelection': trackSelection.toMap(),
      'effectiveVideoTrackId': effectiveVideoTrackId,
      'videoVariantObservation': videoVariantObservation?.toMap(),
      'fixedTrackStatus': fixedTrackStatus?.name,
      'resiliencePolicy': resiliencePolicy.toMap(),
      'lastError': lastError?.toMap(),
    };
  }
}

Map<String, Object?> vesperDecodeMap(Object? raw) {
  final decoded = _rawMap(raw);
  if (decoded != null) {
    return _stringKeyedMap(decoded);
  }
  return <String, Object?>{};
}

double _overlapExtent(
  double startA,
  double endA,
  double startB,
  double endB,
) {
  return math.max(0, math.min(endA, endB) - math.max(startA, startB));
}

double _axisGap(
  double startA,
  double endA,
  double startB,
  double endB,
) {
  if (endA < startB) {
    return startB - endA;
  }
  if (endB < startA) {
    return startA - endB;
  }
  return 0;
}

double _clampUnit(double value) => value.clamp(0.0, 1.0).toDouble();
