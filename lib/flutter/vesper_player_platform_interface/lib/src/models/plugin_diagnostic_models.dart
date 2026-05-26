part of '../models.dart';

enum VesperPluginDiagnosticStatus {
  loaded,
  loadFailed,
  unsupportedKind,
  decoderSupported,
  decoderUnsupported,
  frameProcessorSupported,
  frameProcessorUnsupported,
  sourceNormalizerSupported,
  sourceNormalizerUnsupported,
}

enum VesperPluginCapabilityKind { decoder, frameProcessor, sourceNormalizer }

enum VesperPluginParticipation {
  unknown,
  available,
  selected,
  participated,
  bypassed,
}

final class VesperPluginCodecCapability {
  const VesperPluginCodecCapability({
    required this.mediaKind,
    required this.codec,
  });

  factory VesperPluginCodecCapability.fromMap(Map<Object?, Object?> map) {
    return VesperPluginCodecCapability(
      mediaKind: map['mediaKind'] as String? ?? '',
      codec: map['codec'] as String? ?? '',
    );
  }

  final String mediaKind;
  final String codec;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'mediaKind': mediaKind,
      'codec': codec,
    };
  }
}

final class VesperPluginDecoderCapabilitySummary {
  const VesperPluginDecoderCapabilitySummary({
    this.codecs = const <VesperPluginCodecCapability>[],
    this.legacyCodecs = const <String>[],
    this.supportsNativeFrameOutput = false,
    this.supportsHardwareDecode = false,
    this.supportsCpuVideoFrames = false,
    this.supportsAudioFrames = false,
    this.supportsGpuHandles = false,
    this.supportsFlush = false,
    this.supportsDrain = false,
    this.maxSessions,
  });

  factory VesperPluginDecoderCapabilitySummary.fromMap(
    Map<Object?, Object?> map,
  ) {
    final rawCodecs = map['codecs'];
    return VesperPluginDecoderCapabilitySummary(
      codecs: rawCodecs is Iterable
          ? rawCodecs
              .map(_rawMap)
              .whereType<Map<Object?, Object?>>()
              .map(VesperPluginCodecCapability.fromMap)
              .toList(growable: false)
          : const <VesperPluginCodecCapability>[],
      legacyCodecs: _decodeStringList(map['legacyCodecs']),
      supportsNativeFrameOutput: _decodeBool(map, 'supportsNativeFrameOutput'),
      supportsHardwareDecode: _decodeBool(map, 'supportsHardwareDecode'),
      supportsCpuVideoFrames: _decodeBool(map, 'supportsCpuVideoFrames'),
      supportsAudioFrames: _decodeBool(map, 'supportsAudioFrames'),
      supportsGpuHandles: _decodeBool(map, 'supportsGpuHandles'),
      supportsFlush: _decodeBool(map, 'supportsFlush'),
      supportsDrain: _decodeBool(map, 'supportsDrain'),
      maxSessions: _decodeInt(map, 'maxSessions'),
    );
  }

  final List<VesperPluginCodecCapability> codecs;
  final List<String> legacyCodecs;
  final bool supportsNativeFrameOutput;
  final bool supportsHardwareDecode;
  final bool supportsCpuVideoFrames;
  final bool supportsAudioFrames;
  final bool supportsGpuHandles;
  final bool supportsFlush;
  final bool supportsDrain;
  final int? maxSessions;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'codecs': codecs.map((codec) => codec.toMap()).toList(growable: false),
      'legacyCodecs': legacyCodecs,
      'supportsNativeFrameOutput': supportsNativeFrameOutput,
      'supportsHardwareDecode': supportsHardwareDecode,
      'supportsCpuVideoFrames': supportsCpuVideoFrames,
      'supportsAudioFrames': supportsAudioFrames,
      'supportsGpuHandles': supportsGpuHandles,
      'supportsFlush': supportsFlush,
      'supportsDrain': supportsDrain,
      if (maxSessions != null) 'maxSessions': maxSessions,
    };
  }
}

final class VesperPluginFrameProcessorCapabilitySummary {
  const VesperPluginFrameProcessorCapabilitySummary({
    this.acceptedInputHandleKinds = const <String>[],
    this.outputHandleKinds = const <String>[],
    this.supportsVideoFrames = false,
    this.supportsInPlacePassthrough = false,
    this.preservesDimensions = false,
    this.mayChangeDimensions = false,
    this.preservesColorMetadata = false,
    this.preservesHdrMetadata = false,
    this.supportsFlush = false,
    this.maxSessions,
    this.maxInFlightFrames,
  });

  factory VesperPluginFrameProcessorCapabilitySummary.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperPluginFrameProcessorCapabilitySummary(
      acceptedInputHandleKinds:
          _decodeStringList(map['acceptedInputHandleKinds']),
      outputHandleKinds: _decodeStringList(map['outputHandleKinds']),
      supportsVideoFrames: _decodeBool(map, 'supportsVideoFrames'),
      supportsInPlacePassthrough:
          _decodeBool(map, 'supportsInPlacePassthrough'),
      preservesDimensions: _decodeBool(map, 'preservesDimensions'),
      mayChangeDimensions: _decodeBool(map, 'mayChangeDimensions'),
      preservesColorMetadata: _decodeBool(map, 'preservesColorMetadata'),
      preservesHdrMetadata: _decodeBool(map, 'preservesHdrMetadata'),
      supportsFlush: _decodeBool(map, 'supportsFlush'),
      maxSessions: _decodeInt(map, 'maxSessions'),
      maxInFlightFrames: _decodeInt(map, 'maxInFlightFrames'),
    );
  }

  final List<String> acceptedInputHandleKinds;
  final List<String> outputHandleKinds;
  final bool supportsVideoFrames;
  final bool supportsInPlacePassthrough;
  final bool preservesDimensions;
  final bool mayChangeDimensions;
  final bool preservesColorMetadata;
  final bool preservesHdrMetadata;
  final bool supportsFlush;
  final int? maxSessions;
  final int? maxInFlightFrames;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'acceptedInputHandleKinds': acceptedInputHandleKinds,
      'outputHandleKinds': outputHandleKinds,
      'supportsVideoFrames': supportsVideoFrames,
      'supportsInPlacePassthrough': supportsInPlacePassthrough,
      'preservesDimensions': preservesDimensions,
      'mayChangeDimensions': mayChangeDimensions,
      'preservesColorMetadata': preservesColorMetadata,
      'preservesHdrMetadata': preservesHdrMetadata,
      'supportsFlush': supportsFlush,
      if (maxSessions != null) 'maxSessions': maxSessions,
      if (maxInFlightFrames != null) 'maxInFlightFrames': maxInFlightFrames,
    };
  }
}

final class VesperPluginSourceNormalizerCapabilitySummary {
  const VesperPluginSourceNormalizerCapabilitySummary({
    this.supportedRuntimeProfiles = const <String>[],
    this.supportedOutputRoutes = const <String>[],
    this.maxLevel = '',
    this.mediaKinds = const <String>[],
    this.codecs = const <String>[],
    this.bitstreamFormats = const <String>[],
    this.supportsSeek = false,
    this.supportsFlush = false,
    this.supportsGrowingResources = false,
    this.supportsRangeReads = false,
    this.supportsCancel = false,
    this.contentTypes = const <String>[],
    this.requiredLibraries = const <String>[],
    this.requiredDemuxers = const <String>[],
    this.requiredMuxers = const <String>[],
    this.requiredProtocols = const <String>[],
    this.requiredParsers = const <String>[],
    this.requiredBitstreamFilters = const <String>[],
    this.requiredTls,
    this.requiresNetwork = false,
    this.sessionReadBufferBytes,
    this.manifestSnapshotBytes,
    this.sessionDiskSoftCapBytes,
    this.globalDiskSoftCapBytes,
    this.maxSessions,
  });

  factory VesperPluginSourceNormalizerCapabilitySummary.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperPluginSourceNormalizerCapabilitySummary(
      supportedRuntimeProfiles:
          _decodeStringList(map['supportedRuntimeProfiles']),
      supportedOutputRoutes: _decodeStringList(map['supportedOutputRoutes']),
      maxLevel: map['maxLevel'] as String? ?? '',
      mediaKinds: _decodeStringList(map['mediaKinds']),
      codecs: _decodeStringList(map['codecs']),
      bitstreamFormats: _decodeStringList(map['bitstreamFormats']),
      supportsSeek: _decodeBool(map, 'supportsSeek'),
      supportsFlush: _decodeBool(map, 'supportsFlush'),
      supportsGrowingResources:
          _decodeBool(map, 'supportsGrowingResources'),
      supportsRangeReads: _decodeBool(map, 'supportsRangeReads'),
      supportsCancel: _decodeBool(map, 'supportsCancel'),
      contentTypes: _decodeStringList(map['contentTypes']),
      requiredLibraries: _decodeStringList(map['requiredLibraries']),
      requiredDemuxers: _decodeStringList(map['requiredDemuxers']),
      requiredMuxers: _decodeStringList(map['requiredMuxers']),
      requiredProtocols: _decodeStringList(map['requiredProtocols']),
      requiredParsers: _decodeStringList(map['requiredParsers']),
      requiredBitstreamFilters:
          _decodeStringList(map['requiredBitstreamFilters']),
      requiredTls: map['requiredTls'] as String?,
      requiresNetwork: _decodeBool(map, 'requiresNetwork'),
      sessionReadBufferBytes: _decodeInt(map, 'sessionReadBufferBytes'),
      manifestSnapshotBytes: _decodeInt(map, 'manifestSnapshotBytes'),
      sessionDiskSoftCapBytes: _decodeInt(map, 'sessionDiskSoftCapBytes'),
      globalDiskSoftCapBytes: _decodeInt(map, 'globalDiskSoftCapBytes'),
      maxSessions: _decodeInt(map, 'maxSessions'),
    );
  }

  final List<String> supportedRuntimeProfiles;
  final List<String> supportedOutputRoutes;
  final String maxLevel;
  final List<String> mediaKinds;
  final List<String> codecs;
  final List<String> bitstreamFormats;
  final bool supportsSeek;
  final bool supportsFlush;
  final bool supportsGrowingResources;
  final bool supportsRangeReads;
  final bool supportsCancel;
  final List<String> contentTypes;
  final List<String> requiredLibraries;
  final List<String> requiredDemuxers;
  final List<String> requiredMuxers;
  final List<String> requiredProtocols;
  final List<String> requiredParsers;
  final List<String> requiredBitstreamFilters;
  final String? requiredTls;
  final bool requiresNetwork;
  final int? sessionReadBufferBytes;
  final int? manifestSnapshotBytes;
  final int? sessionDiskSoftCapBytes;
  final int? globalDiskSoftCapBytes;
  final int? maxSessions;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'supportedRuntimeProfiles': supportedRuntimeProfiles,
      'supportedOutputRoutes': supportedOutputRoutes,
      'maxLevel': maxLevel,
      'mediaKinds': mediaKinds,
      'codecs': codecs,
      'bitstreamFormats': bitstreamFormats,
      'supportsSeek': supportsSeek,
      'supportsFlush': supportsFlush,
      'supportsGrowingResources': supportsGrowingResources,
      'supportsRangeReads': supportsRangeReads,
      'supportsCancel': supportsCancel,
      'contentTypes': contentTypes,
      'requiredLibraries': requiredLibraries,
      'requiredDemuxers': requiredDemuxers,
      'requiredMuxers': requiredMuxers,
      'requiredProtocols': requiredProtocols,
      'requiredParsers': requiredParsers,
      'requiredBitstreamFilters': requiredBitstreamFilters,
      if (requiredTls != null) 'requiredTls': requiredTls,
      'requiresNetwork': requiresNetwork,
      if (sessionReadBufferBytes != null)
        'sessionReadBufferBytes': sessionReadBufferBytes,
      if (manifestSnapshotBytes != null)
        'manifestSnapshotBytes': manifestSnapshotBytes,
      if (sessionDiskSoftCapBytes != null)
        'sessionDiskSoftCapBytes': sessionDiskSoftCapBytes,
      if (globalDiskSoftCapBytes != null)
        'globalDiskSoftCapBytes': globalDiskSoftCapBytes,
      if (maxSessions != null) 'maxSessions': maxSessions,
    };
  }
}

final class VesperPluginCapability {
  const VesperPluginCapability.decoder(this.decoder)
      : kind = VesperPluginCapabilityKind.decoder,
        frameProcessor = null,
        sourceNormalizer = null;

  const VesperPluginCapability.frameProcessor(this.frameProcessor)
      : kind = VesperPluginCapabilityKind.frameProcessor,
        decoder = null,
        sourceNormalizer = null;

  const VesperPluginCapability.sourceNormalizer(this.sourceNormalizer)
      : kind = VesperPluginCapabilityKind.sourceNormalizer,
        decoder = null,
        frameProcessor = null;

  factory VesperPluginCapability.fromMap(Map<Object?, Object?> map) {
    final kind = _decodeEnum(
      VesperPluginCapabilityKind.values,
      map['kind'],
      VesperPluginCapabilityKind.decoder,
    );
    return switch (kind) {
      VesperPluginCapabilityKind.decoder => VesperPluginCapability.decoder(
          VesperPluginDecoderCapabilitySummary.fromMap(
            _rawMap(map['decoder']) ?? const <Object?, Object?>{},
          ),
        ),
      VesperPluginCapabilityKind.frameProcessor =>
        VesperPluginCapability.frameProcessor(
          VesperPluginFrameProcessorCapabilitySummary.fromMap(
            _rawMap(map['frameProcessor']) ?? const <Object?, Object?>{},
          ),
        ),
      VesperPluginCapabilityKind.sourceNormalizer =>
        VesperPluginCapability.sourceNormalizer(
          VesperPluginSourceNormalizerCapabilitySummary.fromMap(
            _rawMap(map['sourceNormalizer']) ?? const <Object?, Object?>{},
          ),
        ),
    };
  }

  final VesperPluginCapabilityKind kind;
  final VesperPluginDecoderCapabilitySummary? decoder;
  final VesperPluginFrameProcessorCapabilitySummary? frameProcessor;
  final VesperPluginSourceNormalizerCapabilitySummary? sourceNormalizer;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'kind': kind.name,
      if (decoder != null) 'decoder': decoder!.toMap(),
      if (frameProcessor != null) 'frameProcessor': frameProcessor!.toMap(),
      if (sourceNormalizer != null)
        'sourceNormalizer': sourceNormalizer!.toMap(),
    };
  }
}

final class VesperPluginDiagnostic {
  const VesperPluginDiagnostic({
    required this.path,
    required this.status,
    this.pluginName,
    this.pluginKind,
    this.message,
    this.capability,
    this.participation = VesperPluginParticipation.unknown,
    this.extra = const <String, Object?>{},
  });

  factory VesperPluginDiagnostic.fromMap(Map<Object?, Object?> map) {
    final rawCapability = _rawMap(map['capability']);
    final knownKeys = <Object?>{
      'path',
      'pluginName',
      'pluginKind',
      'status',
      'message',
      'capability',
      'participation',
    };
    return VesperPluginDiagnostic(
      path: map['path'] as String? ?? '',
      pluginName: map['pluginName'] as String?,
      pluginKind: map['pluginKind'] as String?,
      status: _decodeEnum(
        VesperPluginDiagnosticStatus.values,
        map['status'],
        VesperPluginDiagnosticStatus.unsupportedKind,
      ),
      message: map['message'] as String?,
      capability: rawCapability == null
          ? null
          : VesperPluginCapability.fromMap(rawCapability),
      participation: _decodeEnum(
        VesperPluginParticipation.values,
        map['participation'],
        VesperPluginParticipation.unknown,
      ),
      extra: <String, Object?>{
        for (final entry in map.entries)
          if (entry.key is String && !knownKeys.contains(entry.key))
            entry.key! as String: entry.value,
      },
    );
  }

  final String path;
  final String? pluginName;
  final String? pluginKind;
  final VesperPluginDiagnosticStatus status;
  final String? message;
  final VesperPluginCapability? capability;
  final VesperPluginParticipation participation;
  final Map<String, Object?> extra;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'path': path,
      if (pluginName != null) 'pluginName': pluginName,
      if (pluginKind != null) 'pluginKind': pluginKind,
      'status': status.name,
      if (message != null) 'message': message,
      if (capability != null) 'capability': capability!.toMap(),
      if (participation != VesperPluginParticipation.unknown)
        'participation': participation.name,
      ...extra,
    };
  }
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

T _decodeRequiredEnum<T extends Enum>(
  Iterable<T> values,
  Object? raw,
  String key,
) {
  if (raw is! String) {
    throw FormatException(
      'Expected enum field `$key` to be a string, got ${raw.runtimeType}.',
    );
  }
  for (final value in values) {
    if (value.name == raw) {
      return value;
    }
  }
  throw FormatException(
    'Unknown enum value `$raw` for field `$key`. Expected one of '
    '${values.map((value) => value.name).join(', ')}.',
  );
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

Map<String, Object?> _toStringKeyedMap(Map<Object?, Object?> source) {
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

Map<String, Object?> _decodeObjectMap(Object? raw) {
  final map = _rawMap(raw);
  if (map == null || map.isEmpty) {
    return const <String, Object?>{};
  }
  return _toStringKeyedMap(map);
}

Set<String> _decodeStringSet(
  Object? raw, {
  Set<String> fallback = const <String>{},
}) {
  if (raw is! Iterable) {
    return fallback;
  }
  final decoded =
      raw.whereType<String>().where((value) => value.isNotEmpty).toSet();
  return decoded.isEmpty ? fallback : decoded;
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
