part of '../models.dart';

final class VesperPlayerSource {
  const VesperPlayerSource({
    required this.uri,
    required this.label,
    required this.kind,
    required this.protocol,
    this.headers = const <String, String>{},
    this.drmConfiguration,
    List<VesperExternalSubtitleSource>? externalSubtitles,
    @Deprecated('Use externalSubtitles instead.')
    List<VesperSubtitleSideLoad>? subtitleConfigurations,
  }) : externalSubtitles = externalSubtitles ??
            subtitleConfigurations ??
            const <VesperExternalSubtitleSource>[];

  factory VesperPlayerSource.local({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
    VesperPlayerDrmConfiguration? drmConfiguration,
    List<VesperExternalSubtitleSource> externalSubtitles =
        const <VesperExternalSubtitleSource>[],
  }) {
    return VesperPlayerSource(
      uri: uri,
      label: label ?? uri,
      kind: VesperPlayerSourceKind.local,
      protocol: _inferLocalProtocol(uri),
      headers: headers,
      drmConfiguration: drmConfiguration,
      externalSubtitles: externalSubtitles,
    );
  }

  factory VesperPlayerSource.localDash({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
    VesperPlayerDrmConfiguration? drmConfiguration,
    List<VesperExternalSubtitleSource> externalSubtitles =
        const <VesperExternalSubtitleSource>[],
  }) {
    return VesperPlayerSource(
      uri: uri,
      label: label ?? uri,
      kind: VesperPlayerSourceKind.local,
      protocol: VesperPlayerSourceProtocol.dash,
      headers: headers,
      drmConfiguration: drmConfiguration,
      externalSubtitles: externalSubtitles,
    );
  }

  factory VesperPlayerSource.remote({
    required String uri,
    String? label,
    VesperPlayerSourceProtocol? protocol,
    Map<String, String> headers = const <String, String>{},
    VesperPlayerDrmConfiguration? drmConfiguration,
    List<VesperExternalSubtitleSource> externalSubtitles =
        const <VesperExternalSubtitleSource>[],
  }) {
    return VesperPlayerSource(
      uri: uri,
      label: label ?? uri,
      kind: VesperPlayerSourceKind.remote,
      protocol: protocol ?? _inferRemoteProtocol(uri),
      headers: headers,
      drmConfiguration: drmConfiguration,
      externalSubtitles: externalSubtitles,
    );
  }

  factory VesperPlayerSource.hls({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
    VesperPlayerDrmConfiguration? drmConfiguration,
    List<VesperExternalSubtitleSource> externalSubtitles =
        const <VesperExternalSubtitleSource>[],
  }) {
    return VesperPlayerSource.remote(
      uri: uri,
      label: label,
      protocol: VesperPlayerSourceProtocol.hls,
      headers: headers,
      drmConfiguration: drmConfiguration,
      externalSubtitles: externalSubtitles,
    );
  }

  factory VesperPlayerSource.dash({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
    VesperPlayerDrmConfiguration? drmConfiguration,
    List<VesperExternalSubtitleSource> externalSubtitles =
        const <VesperExternalSubtitleSource>[],
  }) {
    return VesperPlayerSource.remote(
      uri: uri,
      label: label,
      protocol: VesperPlayerSourceProtocol.dash,
      headers: headers,
      drmConfiguration: drmConfiguration,
      externalSubtitles: externalSubtitles,
    );
  }

  /// RTMP / RTMPS live stream.
  ///
  /// On iOS this protocol is not supported by AVPlayer and the host kit will
  /// surface a capability error. On Android it is routed through the Media3
  /// live source once available.
  factory VesperPlayerSource.rtmp({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
    List<VesperExternalSubtitleSource> externalSubtitles =
        const <VesperExternalSubtitleSource>[],
  }) {
    return VesperPlayerSource.remote(
      uri: uri,
      label: label,
      protocol: VesperPlayerSourceProtocol.rtmp,
      headers: headers,
      externalSubtitles: externalSubtitles,
    );
  }

  /// RTSP / RTSPS live stream.
  ///
  /// On iOS this protocol is not supported by AVPlayer and the host kit will
  /// surface a capability error.
  factory VesperPlayerSource.rtsp({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
    List<VesperExternalSubtitleSource> externalSubtitles =
        const <VesperExternalSubtitleSource>[],
  }) {
    return VesperPlayerSource.remote(
      uri: uri,
      label: label,
      protocol: VesperPlayerSourceProtocol.rtsp,
      headers: headers,
      externalSubtitles: externalSubtitles,
    );
  }

  /// HTTP-FLV live stream.
  ///
  /// On iOS this protocol is not supported by AVPlayer and the host kit will
  /// surface a capability error. On Android it is routed through the Media3
  /// FLV source.
  factory VesperPlayerSource.flvLive({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
    List<VesperExternalSubtitleSource> externalSubtitles =
        const <VesperExternalSubtitleSource>[],
  }) {
    return VesperPlayerSource.remote(
      uri: uri,
      label: label,
      protocol: VesperPlayerSourceProtocol.flv,
      headers: headers,
      externalSubtitles: externalSubtitles,
    );
  }

  factory VesperPlayerSource.fromMap(Map<Object?, Object?> map) {
    final uri = map['uri'] as String? ?? '';
    final externalSubtitles = _decodeExternalSubtitles(
      map['externalSubtitles'] ?? map['subtitleConfigurations'],
    );
    _validateExternalSubtitleIds(externalSubtitles);
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
      externalSubtitles: externalSubtitles,
    );
  }

  final String uri;
  final String label;
  final VesperPlayerSourceKind kind;
  final VesperPlayerSourceProtocol protocol;
  final Map<String, String> headers;
  final VesperPlayerDrmConfiguration? drmConfiguration;

  /// Optional external subtitle tracks (SRT/ASS/WebVTT URIs).
  final List<VesperExternalSubtitleSource> externalSubtitles;

  /// Legacy alias for [externalSubtitles].
  @Deprecated('Use externalSubtitles instead.')
  List<VesperExternalSubtitleSource> get subtitleConfigurations =>
      externalSubtitles;

  Map<String, Object?> toMap() {
    _validateExternalSubtitleIds(externalSubtitles);
    return <String, Object?>{
      'uri': uri,
      'label': label,
      'kind': kind.name,
      'protocol': protocol.name,
      'headers': headers,
      if (drmConfiguration != null)
        'drmConfiguration': drmConfiguration?.toMap(),
      if (externalSubtitles.isNotEmpty)
        'externalSubtitles': externalSubtitles
            .map((VesperExternalSubtitleSource source) => source.toMap())
            .toList(),
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
    if (normalized.startsWith('rtmp://') || normalized.startsWith('rtmps://')) {
      return VesperPlayerSourceProtocol.rtmp;
    }
    if (normalized.startsWith('rtsp://') || normalized.startsWith('rtsps://')) {
      return VesperPlayerSourceProtocol.rtsp;
    }
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

List<VesperExternalSubtitleSource> _decodeExternalSubtitles(
  Object? raw,
) {
  if (raw == null) {
    return const <VesperExternalSubtitleSource>[];
  }
  if (raw is! List) {
    throw ArgumentError.value(
      raw,
      'externalSubtitles',
      'must be a list of subtitle maps.',
    );
  }
  return raw.indexed.map((entry) {
    final (index, value) = entry;
    if (value is! Map) {
      throw ArgumentError.value(
        value,
        'externalSubtitles[$index]',
        'must be a subtitle map.',
      );
    }
    return VesperExternalSubtitleSource.fromMap(
      Map<Object?, Object?>.from(value),
    );
  }).toList(growable: false);
}

bool _hasUniqueExternalSubtitleIds(
  Iterable<VesperExternalSubtitleSource> subtitles,
) {
  final ids = <String>{};
  for (final subtitle in subtitles) {
    if (subtitle.id.trim().isEmpty) {
      return false;
    }
    if (!ids.add(subtitle.id)) {
      return false;
    }
  }
  return true;
}

void _validateExternalSubtitleIds(
  Iterable<VesperExternalSubtitleSource> subtitles,
) {
  if (!_hasUniqueExternalSubtitleIds(subtitles)) {
    throw ArgumentError.value(
      subtitles,
      'externalSubtitles',
      'ids must be non-empty and unique within a source.',
    );
  }
}
