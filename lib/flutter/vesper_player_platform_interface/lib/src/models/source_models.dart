part of '../models.dart';

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

  factory VesperPlayerSource.localDash({
    required String uri,
    String? label,
    Map<String, String> headers = const <String, String>{},
  }) {
    return VesperPlayerSource(
      uri: uri,
      label: label ?? uri,
      kind: VesperPlayerSourceKind.local,
      protocol: VesperPlayerSourceProtocol.dash,
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

