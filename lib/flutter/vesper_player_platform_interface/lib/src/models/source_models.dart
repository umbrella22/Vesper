part of '../models.dart';

final class VesperPlayerSource {
  const VesperPlayerSource({
    required this.uri,
    required this.label,
    required this.kind,
    required this.protocol,
    this.headers = const <String, String>{},
    this.drmConfiguration,
  });

  factory VesperPlayerSource.local({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
    VesperPlayerDrmConfiguration? drmConfiguration,
  }) {
    return VesperPlayerSource(
      uri: uri,
      label: label ?? uri,
      kind: VesperPlayerSourceKind.local,
      protocol: _inferLocalProtocol(uri),
      headers: headers,
      drmConfiguration: drmConfiguration,
    );
  }

  factory VesperPlayerSource.localDash({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
    VesperPlayerDrmConfiguration? drmConfiguration,
  }) {
    return VesperPlayerSource(
      uri: uri,
      label: label ?? uri,
      kind: VesperPlayerSourceKind.local,
      protocol: VesperPlayerSourceProtocol.dash,
      headers: headers,
      drmConfiguration: drmConfiguration,
    );
  }

  factory VesperPlayerSource.remote({
    required String uri,
    String? label,
    VesperPlayerSourceProtocol? protocol,
    Map<String, String> headers = const <String, String>{},
    VesperPlayerDrmConfiguration? drmConfiguration,
  }) {
    return VesperPlayerSource(
      uri: uri,
      label: label ?? uri,
      kind: VesperPlayerSourceKind.remote,
      protocol: protocol ?? _inferRemoteProtocol(uri),
      headers: headers,
      drmConfiguration: drmConfiguration,
    );
  }

  factory VesperPlayerSource.hls({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
    VesperPlayerDrmConfiguration? drmConfiguration,
  }) {
    return VesperPlayerSource.remote(
      uri: uri,
      label: label,
      protocol: VesperPlayerSourceProtocol.hls,
      headers: headers,
      drmConfiguration: drmConfiguration,
    );
  }

  factory VesperPlayerSource.dash({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
    VesperPlayerDrmConfiguration? drmConfiguration,
  }) {
    return VesperPlayerSource.remote(
      uri: uri,
      label: label,
      protocol: VesperPlayerSourceProtocol.dash,
      headers: headers,
      drmConfiguration: drmConfiguration,
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
      drmConfiguration: VesperPlayerDrmConfiguration.tryFromMap(
        _rawMap(map['drmConfiguration']),
      ),
    );
  }

  final String uri;
  final String label;
  final VesperPlayerSourceKind kind;
  final VesperPlayerSourceProtocol protocol;
  final Map<String, String> headers;
  final VesperPlayerDrmConfiguration? drmConfiguration;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'uri': uri,
      'label': label,
      'kind': kind.name,
      'protocol': protocol.name,
      'headers': headers,
      if (drmConfiguration != null)
        'drmConfiguration': drmConfiguration?.toMap(),
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

final class VesperPlayerDrmConfiguration {
  const VesperPlayerDrmConfiguration({
    required this.keySystem,
    required this.licenseUri,
    this.licenseHeaders = const <String, String>{},
    this.fairPlayCertificateUri,
    this.fairPlayCertificateBase64,
    this.multiSession = false,
  });

  factory VesperPlayerDrmConfiguration.fromMap(Map<Object?, Object?> map) {
    return VesperPlayerDrmConfiguration(
      keySystem: map['keySystem'] as String? ?? '',
      licenseUri: map['licenseUri'] as String? ?? '',
      licenseHeaders: _decodeStringMap(map['licenseHeaders']),
      fairPlayCertificateUri: map['fairPlayCertificateUri'] as String?,
      fairPlayCertificateBase64: map['fairPlayCertificateBase64'] as String?,
      multiSession: _decodeBool(map, 'multiSession'),
    );
  }

  static VesperPlayerDrmConfiguration? tryFromMap(
    Map<Object?, Object?>? map,
  ) {
    if (map == null || map.isEmpty) {
      return null;
    }
    return VesperPlayerDrmConfiguration.fromMap(map);
  }

  final String keySystem;
  final String licenseUri;
  final Map<String, String> licenseHeaders;
  final String? fairPlayCertificateUri;
  final String? fairPlayCertificateBase64;
  final bool multiSession;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'keySystem': keySystem,
      'licenseUri': licenseUri,
      'licenseHeaders': licenseHeaders,
      if (fairPlayCertificateUri != null)
        'fairPlayCertificateUri': fairPlayCertificateUri,
      if (fairPlayCertificateBase64 != null)
        'fairPlayCertificateBase64': fairPlayCertificateBase64,
      'multiSession': multiSession,
    };
  }
}
