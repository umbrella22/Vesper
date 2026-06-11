part of '../../models.dart';

final class VesperPlaybackCapabilityProbeResult {
  const VesperPlaybackCapabilityProbeResult({
    required this.status,
    required this.codecFamily,
    required this.systemPlaybackSupported,
    required this.hardwareDecodeSupported,
    required this.sdkManagedNativeFrameSupported,
    required this.recommendedPlaybackPath,
    required this.outputFormat,
    required this.hdrKind,
    required this.dolbyVisionMode,
    required this.confidence,
    this.hdrMetadata,
    this.missingCapabilities = const <String>[],
    this.diagnostics = const <String, Object?>{},
  });

  factory VesperPlaybackCapabilityProbeResult.fromMap(
    Map<Object?, Object?> map,
  ) {
    final hdrKind = _decodeEnum(
      VesperPlaybackCapabilityHdrKind.values,
      map['hdrKind'],
      VesperPlaybackCapabilityHdrKind.none,
    );
    final dolbyVisionMode = _decodeEnum(
      VesperPlaybackCapabilityDolbyVisionMode.values,
      map['dolbyVisionMode'],
      VesperPlaybackCapabilityDolbyVisionMode.none,
    );
    final diagnostics = vesperDecodeMap(map['diagnostics']);
    final explicitHdrMetadata = _rawMap(map['hdrMetadata']);
    return VesperPlaybackCapabilityProbeResult(
      status: _decodeEnum(
        VesperPlaybackCapabilityProbeStatus.values,
        map['status'],
        VesperPlaybackCapabilityProbeStatus.unknown,
      ),
      codecFamily: _decodeEnum(
        VesperPlaybackCodecFamily.values,
        map['codecFamily'],
        VesperPlaybackCodecFamily.unknown,
      ),
      systemPlaybackSupported: _decodeBool(map, 'systemPlaybackSupported'),
      hardwareDecodeSupported: _decodeBool(map, 'hardwareDecodeSupported'),
      sdkManagedNativeFrameSupported:
          _decodeBool(map, 'sdkManagedNativeFrameSupported'),
      recommendedPlaybackPath: _decodeEnum(
        VesperRecommendedPlaybackPath.values,
        map['recommendedPlaybackPath'],
        VesperRecommendedPlaybackPath.systemPlayer,
      ),
      outputFormat: _decodeEnum(
        VesperPlaybackCapabilityOutputFormat.values,
        map['outputFormat'],
        VesperPlaybackCapabilityOutputFormat.unknown,
      ),
      hdrKind: hdrKind,
      dolbyVisionMode: dolbyVisionMode,
      confidence: _decodeEnum(
        VesperPlaybackCapabilityConfidence.values,
        map['confidence'],
        VesperPlaybackCapabilityConfidence.codecOnly,
      ),
      hdrMetadata: explicitHdrMetadata != null
          ? VesperHdrMetadata.fromMap(explicitHdrMetadata)
          : VesperHdrMetadata.fromDiagnostics(
              diagnostics,
              hdrKind: hdrKind,
              dolbyVisionMode: dolbyVisionMode,
            ),
      missingCapabilities: _decodeStringList(map['missingCapabilities']),
      diagnostics: diagnostics,
    );
  }

  final VesperPlaybackCapabilityProbeStatus status;
  final VesperPlaybackCodecFamily codecFamily;
  final bool systemPlaybackSupported;
  final bool hardwareDecodeSupported;
  final bool sdkManagedNativeFrameSupported;
  final VesperRecommendedPlaybackPath recommendedPlaybackPath;
  final VesperPlaybackCapabilityOutputFormat outputFormat;
  final VesperPlaybackCapabilityHdrKind hdrKind;
  final VesperPlaybackCapabilityDolbyVisionMode dolbyVisionMode;
  final VesperPlaybackCapabilityConfidence confidence;
  final VesperHdrMetadata? hdrMetadata;
  final List<String> missingCapabilities;
  final Map<String, Object?> diagnostics;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'status': status.name,
      'codecFamily': codecFamily.name,
      'systemPlaybackSupported': systemPlaybackSupported,
      'hardwareDecodeSupported': hardwareDecodeSupported,
      'sdkManagedNativeFrameSupported': sdkManagedNativeFrameSupported,
      'recommendedPlaybackPath': recommendedPlaybackPath.name,
      'outputFormat': outputFormat.name,
      'hdrKind': hdrKind.name,
      'dolbyVisionMode': dolbyVisionMode.name,
      'confidence': confidence.name,
      if (hdrMetadata != null) 'hdrMetadata': hdrMetadata?.toMap(),
      'missingCapabilities': missingCapabilities,
      'diagnostics': diagnostics,
    };
  }
}

VesperPlaybackCapabilityHdrKind? _effectiveHdrKind(
  Map<Object?, Object?> diagnostics,
  VesperPlaybackCapabilityHdrKind? fallback,
) {
  final raw = _firstHdrString(diagnostics, <String>[
    'assetVideoMetadataHdrKind',
    'hdrKind',
  ]);
  if (raw != null) {
    return _hdrKind(raw);
  }
  if (fallback == VesperPlaybackCapabilityHdrKind.none ||
      fallback == VesperPlaybackCapabilityHdrKind.unknown) {
    return null;
  }
  return fallback;
}

VesperPlaybackCapabilityHdrKind? _hdrKind(Object? raw) {
  if (raw is VesperPlaybackCapabilityHdrKind) {
    return raw;
  }
  if (raw is String) {
    for (final value in VesperPlaybackCapabilityHdrKind.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return null;
}

VesperPlaybackCapabilityDolbyVisionMode? _hdrDolbyVisionMode(Object? raw) {
  if (raw is VesperPlaybackCapabilityDolbyVisionMode) {
    return raw;
  }
  if (raw is String) {
    for (final value in VesperPlaybackCapabilityDolbyVisionMode.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return null;
}

String? _firstHdrString(Map<Object?, Object?> map, List<String> keys) {
  for (final key in keys) {
    final value = _hdrString(map[key]);
    if (value != null) {
      return value;
    }
  }
  return null;
}

int? _firstHdrInt(Map<Object?, Object?> map, List<String> keys) {
  for (final key in keys) {
    final value = _hdrInt(map[key]);
    if (value != null) {
      return value;
    }
  }
  return null;
}

String? _hdrString(Object? raw) {
  if (raw is String && raw.isNotEmpty) {
    return raw;
  }
  return null;
}

bool? _hdrBool(Object? raw) {
  if (raw is bool) {
    return raw;
  }
  if (raw is String) {
    if (raw == 'true') {
      return true;
    }
    if (raw == 'false') {
      return false;
    }
  }
  return null;
}

int? _hdrInt(Object? raw) {
  if (raw is int) {
    return raw;
  }
  if (raw is double && raw.isFinite) {
    return raw.round();
  }
  if (raw is String) {
    return int.tryParse(raw);
  }
  return null;
}

double? _hdrDouble(Object? raw) {
  if (raw is double && raw.isFinite) {
    return raw;
  }
  if (raw is int) {
    return raw.toDouble();
  }
  if (raw is String) {
    return double.tryParse(raw);
  }
  return null;
}
