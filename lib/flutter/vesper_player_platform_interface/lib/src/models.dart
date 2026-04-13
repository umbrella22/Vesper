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

enum VesperMediaTrackKind { video, audio, subtitle }

enum VesperTrackSelectionMode { auto, disabled, track }

enum VesperAbrMode { auto, constrained, fixedTrack }

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

final class VesperPlayerSource {
  const VesperPlayerSource({
    required this.uri,
    required this.label,
    required this.kind,
    required this.protocol,
  });

  factory VesperPlayerSource.local({required String uri, String? label}) {
    return VesperPlayerSource(
      uri: uri,
      label: label ?? uri,
      kind: VesperPlayerSourceKind.local,
      protocol: _inferLocalProtocol(uri),
    );
  }

  factory VesperPlayerSource.remote({
    required String uri,
    String? label,
    VesperPlayerSourceProtocol? protocol,
  }) {
    return VesperPlayerSource(
      uri: uri,
      label: label ?? uri,
      kind: VesperPlayerSourceKind.remote,
      protocol: protocol ?? _inferRemoteProtocol(uri),
    );
  }

  factory VesperPlayerSource.hls({required String uri, String? label}) {
    return VesperPlayerSource.remote(
      uri: uri,
      label: label,
      protocol: VesperPlayerSourceProtocol.hls,
    );
  }

  factory VesperPlayerSource.dash({required String uri, String? label}) {
    return VesperPlayerSource.remote(
      uri: uri,
      label: label,
      protocol: VesperPlayerSourceProtocol.dash,
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
    );
  }

  final String uri;
  final String label;
  final VesperPlayerSourceKind kind;
  final VesperPlayerSourceProtocol protocol;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'uri': uri,
      'label': label,
      'kind': kind.name,
      'protocol': protocol.name,
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
    this.supportsTrackCatalog = false,
    this.supportsTrackSelection = false,
    this.supportsAbrPolicy = false,
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
      supportsTrackCatalog = false,
      supportsTrackSelection = false,
      supportsAbrPolicy = false,
      supportsResiliencePolicy = false,
      supportsHolePunch = false,
      supportsPlaybackRate = false,
      supportsLiveEdgeSeeking = false,
      isExperimental = false,
      supportedPlaybackRates = const <double>[];

  factory VesperPlayerCapabilities.fromMap(Map<Object?, Object?> map) {
    final rawRates = map['supportedPlaybackRates'];
    return VesperPlayerCapabilities(
      supportsLocalFiles: _decodeBool(map, 'supportsLocalFiles'),
      supportsRemoteUrls: _decodeBool(map, 'supportsRemoteUrls'),
      supportsHls: _decodeBool(map, 'supportsHls'),
      supportsDash: _decodeBool(map, 'supportsDash'),
      supportsTrackCatalog: _decodeBool(map, 'supportsTrackCatalog'),
      supportsTrackSelection: _decodeBool(map, 'supportsTrackSelection'),
      supportsAbrPolicy: _decodeBool(map, 'supportsAbrPolicy'),
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
  final bool supportsTrackCatalog;
  final bool supportsTrackSelection;
  final bool supportsAbrPolicy;
  final bool supportsResiliencePolicy;
  final bool supportsHolePunch;
  final bool supportsPlaybackRate;
  final bool supportsLiveEdgeSeeking;
  final bool isExperimental;
  final List<double> supportedPlaybackRates;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'supportsLocalFiles': supportsLocalFiles,
      'supportsRemoteUrls': supportsRemoteUrls,
      'supportsHls': supportsHls,
      'supportsDash': supportsDash,
      'supportsTrackCatalog': supportsTrackCatalog,
      'supportsTrackSelection': supportsTrackSelection,
      'supportsAbrPolicy': supportsAbrPolicy,
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
      final clamped = positionMs.clamp(range.startMs, range.endMs);
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
    return (positionMs / total).clamp(0.0, 1.0).toDouble();
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
  }) : mode = VesperAbrMode.constrained,
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
      minBufferMs = 10000,
      maxBufferMs = 30000,
      bufferForPlaybackMs = 1000,
      bufferForPlaybackAfterRebufferMs = 2000;

  const VesperBufferingPolicy.streaming()
    : preset = VesperBufferingPreset.streaming,
      minBufferMs = 12000,
      maxBufferMs = 36000,
      bufferForPlaybackMs = 1200,
      bufferForPlaybackAfterRebufferMs = 2500;

  const VesperBufferingPolicy.resilient()
    : preset = VesperBufferingPreset.resilient,
      minBufferMs = 20000,
      maxBufferMs = 50000,
      bufferForPlaybackMs = 1500,
      bufferForPlaybackAfterRebufferMs = 3000;

  const VesperBufferingPolicy.lowLatency()
    : preset = VesperBufferingPreset.lowLatency,
      minBufferMs = 4000,
      maxBufferMs = 12000,
      bufferForPlaybackMs = 500,
      bufferForPlaybackAfterRebufferMs = 1000;

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
    this.maxAttempts = 3,
    this.baseDelayMs = 1000,
    this.maxDelayMs = 5000,
    this.backoff = VesperRetryBackoff.linear,
  });

  const VesperRetryPolicy.aggressive()
    : maxAttempts = 2,
      baseDelayMs = 500,
      maxDelayMs = 2000,
      backoff = VesperRetryBackoff.fixed;

  const VesperRetryPolicy.resilient()
    : maxAttempts = 6,
      baseDelayMs = 1000,
      maxDelayMs = 8000,
      backoff = VesperRetryBackoff.exponential;

  factory VesperRetryPolicy.fromMap(Map<Object?, Object?> map) {
    return VesperRetryPolicy(
      maxAttempts: _decodeInt(map, 'maxAttempts'),
      baseDelayMs: _decodeInt(map, 'baseDelayMs') ?? 1000,
      maxDelayMs: _decodeInt(map, 'maxDelayMs') ?? 5000,
      backoff: _decodeEnum(
        VesperRetryBackoff.values,
        map['backoff'],
        VesperRetryBackoff.linear,
      ),
    );
  }

  final int? maxAttempts;
  final int baseDelayMs;
  final int maxDelayMs;
  final VesperRetryBackoff backoff;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'maxAttempts': maxAttempts,
      'baseDelayMs': baseDelayMs,
      'maxDelayMs': maxDelayMs,
      'backoff': backoff.name,
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
      maxMemoryBytes = 0,
      maxDiskBytes = 0;

  const VesperCachePolicy.streaming()
    : preset = VesperCachePreset.streaming,
      maxMemoryBytes = 8 * 1024 * 1024,
      maxDiskBytes = 128 * 1024 * 1024;

  const VesperCachePolicy.resilient()
    : preset = VesperCachePreset.resilient,
      maxMemoryBytes = 16 * 1024 * 1024,
      maxDiskBytes = 384 * 1024 * 1024;

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

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'left': left,
      'top': top,
      'width': width,
      'height': height,
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
    this.backendFamily = VesperPlayerBackendFamily.unknown,
    this.capabilities = const VesperPlayerCapabilities.unsupported(),
    this.trackCatalog = const VesperTrackCatalog(),
    this.trackSelection = const VesperTrackSelectionSnapshot(),
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
      backendFamily = VesperPlayerBackendFamily.unknown,
      capabilities = const VesperPlayerCapabilities.unsupported(),
      trackCatalog = const VesperTrackCatalog(),
      trackSelection = const VesperTrackSelectionSnapshot(),
      lastError = null;

  factory VesperPlayerSnapshot.fromMap(Map<Object?, Object?> map) {
    final rawTimeline = map['timeline'];
    final rawCapabilities = map['capabilities'];
    final rawTrackCatalog = map['trackCatalog'];
    final rawTrackSelection = map['trackSelection'];
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
  final VesperPlayerBackendFamily backendFamily;
  final VesperPlayerCapabilities capabilities;
  final VesperTrackCatalog trackCatalog;
  final VesperTrackSelectionSnapshot trackSelection;
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
    VesperPlayerBackendFamily? backendFamily,
    VesperPlayerCapabilities? capabilities,
    VesperTrackCatalog? trackCatalog,
    VesperTrackSelectionSnapshot? trackSelection,
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
      backendFamily: backendFamily ?? this.backendFamily,
      capabilities: capabilities ?? this.capabilities,
      trackCatalog: trackCatalog ?? this.trackCatalog,
      trackSelection: trackSelection ?? this.trackSelection,
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
      'backendFamily': backendFamily.name,
      'capabilities': capabilities.toMap(),
      'trackCatalog': trackCatalog.toMap(),
      'trackSelection': trackSelection.toMap(),
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
