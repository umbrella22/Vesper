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
    return VesperExternalFormatAdaptationConfig(
      enabled: _decodeBool(map, 'enabled'),
      preferredFallback: _decodeEnum(
        VesperExternalFallbackFormat.values,
        map['preferredFallback'],
        VesperExternalFallbackFormat.mpegTs,
      ),
      allowHls: _decodeBool(map, 'allowHls', fallback: true),
      enableRangeCache: _decodeBool(map, 'enableRangeCache', fallback: true),
      allowRemoteDashMediaReferences: _decodeBool(
        map,
        'allowRemoteDashMediaReferences',
      ),
      allowPrivateRemoteDashMediaAddresses: _decodeBool(
        map,
        'allowPrivateRemoteDashMediaAddresses',
      ),
      remoteDashMediaRequestHeaders: _decodeStringSet(
        map['remoteDashMediaRequestHeaders'],
        fallback: _defaultRemoteDashMediaRequestHeaders,
      ),
      debugDiagnostics: _decodeBool(map, 'debugDiagnostics'),
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

const Set<String> _defaultRemoteDashMediaRequestHeaders = <String>{
  'User-Agent',
  'Accept',
  'Accept-Language',
};

