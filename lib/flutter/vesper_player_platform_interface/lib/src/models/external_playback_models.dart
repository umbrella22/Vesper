part of '../models.dart';

enum VesperExternalPlaybackRouteKind { none, airPlay, cast, dlna }

enum VesperExternalProxyPolicy { auto, always, never }

enum VesperExternalFallbackFormat { mpegTs, hls }

enum VesperExternalPlaybackResultStatus {
  success,
  unavailable,
  unsupported,
  failed,
}

enum VesperExternalPlaybackSessionEventKind {
  routeConnected,
  routeDisconnected,
  loaded,
  playing,
  paused,
  stopped,
  suspended,
  discoveryDiagnostic,
  error,
  unknown,
}

final class VesperExternalFormatAdaptationConfig {
  const VesperExternalFormatAdaptationConfig({
    this.enabled = false,
    this.preferredFallback = VesperExternalFallbackFormat.mpegTs,
    this.allowHls = true,
    this.enableRangeCache = true,
    this.allowRemoteDashMediaReferences = false,
    this.allowPrivateRemoteDashMediaAddresses = false,
    this.remoteDashMediaRequestHeaders = _defaultRemoteDashMediaRequestHeaders,
    this.debugDiagnostics = false,
  });

  const VesperExternalFormatAdaptationConfig.disabled()
      : enabled = false,
        preferredFallback = VesperExternalFallbackFormat.mpegTs,
        allowHls = true,
        enableRangeCache = true,
        allowRemoteDashMediaReferences = false,
        allowPrivateRemoteDashMediaAddresses = false,
        remoteDashMediaRequestHeaders = _defaultRemoteDashMediaRequestHeaders,
        debugDiagnostics = false;

  const VesperExternalFormatAdaptationConfig.dlnaRemux({
    this.preferredFallback = VesperExternalFallbackFormat.mpegTs,
    this.allowHls = true,
    this.enableRangeCache = true,
    this.allowRemoteDashMediaReferences = false,
    this.allowPrivateRemoteDashMediaAddresses = false,
    this.remoteDashMediaRequestHeaders = _defaultRemoteDashMediaRequestHeaders,
    this.debugDiagnostics = false,
  }) : enabled = true;

  factory VesperExternalFormatAdaptationConfig.fromMap(
    Map<Object?, Object?> map,
  ) {
    _validateCallerMapKeys(map, 'VesperExternalFormatAdaptationConfig');
    return VesperExternalFormatAdaptationConfig(
      enabled: _decodeExternalConfigurationBool(map, 'enabled'),
      preferredFallback: _decodeCallerEnum(
        VesperExternalFallbackFormat.values,
        map['preferredFallback'],
        VesperExternalFallbackFormat.mpegTs,
        'preferredFallback',
      ),
      allowHls:
          _decodeExternalConfigurationBool(map, 'allowHls', fallback: true),
      enableRangeCache: _decodeExternalConfigurationBool(
          map, 'enableRangeCache',
          fallback: true),
      allowRemoteDashMediaReferences: _decodeExternalConfigurationBool(
        map,
        'allowRemoteDashMediaReferences',
      ),
      allowPrivateRemoteDashMediaAddresses: _decodeExternalConfigurationBool(
        map,
        'allowPrivateRemoteDashMediaAddresses',
      ),
      remoteDashMediaRequestHeaders: _decodeRemoteDashMediaRequestHeaders(
        map['remoteDashMediaRequestHeaders'],
      ),
      debugDiagnostics:
          _decodeExternalConfigurationBool(map, 'debugDiagnostics'),
    );
  }

  final bool enabled;
  final VesperExternalFallbackFormat preferredFallback;
  final bool allowHls;
  final bool enableRangeCache;
  final bool allowRemoteDashMediaReferences;
  final bool allowPrivateRemoteDashMediaAddresses;
  final Set<String> remoteDashMediaRequestHeaders;
  final bool debugDiagnostics;

  bool get hasOverrides =>
      enabled ||
      preferredFallback != VesperExternalFallbackFormat.mpegTs ||
      !allowHls ||
      !enableRangeCache ||
      allowRemoteDashMediaReferences ||
      allowPrivateRemoteDashMediaAddresses ||
      !remoteDashMediaRequestHeaders.containsAll(
        _defaultRemoteDashMediaRequestHeaders,
      ) ||
      remoteDashMediaRequestHeaders.length !=
          _defaultRemoteDashMediaRequestHeaders.length ||
      debugDiagnostics;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'enabled': enabled,
      'preferredFallback': preferredFallback.name,
      'allowHls': allowHls,
      'enableRangeCache': enableRangeCache,
      'allowRemoteDashMediaReferences': allowRemoteDashMediaReferences,
      'allowPrivateRemoteDashMediaAddresses':
          allowPrivateRemoteDashMediaAddresses,
      'remoteDashMediaRequestHeaders':
          remoteDashMediaRequestHeaders.toList(growable: false),
      'debugDiagnostics': debugDiagnostics,
    };
  }
}

bool _decodeExternalConfigurationBool(
  Map<Object?, Object?> map,
  String field, {
  bool fallback = false,
}) {
  final raw = map[field];
  if (raw == null) return fallback;
  if (raw is! bool) throw FormatException('$field must be a boolean');
  return raw;
}

Set<String> _decodeRemoteDashMediaRequestHeaders(Object? raw) {
  if (raw == null) return _defaultRemoteDashMediaRequestHeaders;
  if (raw is! Iterable) {
    throw FormatException('remoteDashMediaRequestHeaders must be an iterable');
  }
  final headers = <String>{};
  for (final (index, value) in raw.indexed) {
    if (value is! String) {
      throw FormatException(
        'remoteDashMediaRequestHeaders[$index] must be a string',
      );
    }
    headers.add(value);
  }
  return headers;
}

const Set<String> _defaultRemoteDashMediaRequestHeaders = <String>{
  'User-Agent',
  'Accept',
  'Accept-Language',
};
