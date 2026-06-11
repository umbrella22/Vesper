part of '../../models.dart';

final class VesperHdrChromaticityPoint {
  const VesperHdrChromaticityPoint({required this.x, required this.y});

  factory VesperHdrChromaticityPoint.fromMap(Map<Object?, Object?> map) {
    return VesperHdrChromaticityPoint(
      x: _hdrDouble(map['x']) ?? 0,
      y: _hdrDouble(map['y']) ?? 0,
    );
  }

  static VesperHdrChromaticityPoint? fromDiagnostic(Object? raw) {
    final map = _rawMap(raw);
    if (map != null) {
      return VesperHdrChromaticityPoint.fromMap(map);
    }
    final value = raw as String?;
    if (value == null) {
      return null;
    }
    final parts = value.split(',');
    if (parts.length != 2) {
      return null;
    }
    final x = double.tryParse(parts[0].trim());
    final y = double.tryParse(parts[1].trim());
    if (x == null || y == null) {
      return null;
    }
    return VesperHdrChromaticityPoint(x: x, y: y);
  }

  final double x;
  final double y;

  Map<String, Object?> toMap() {
    return <String, Object?>{'x': x, 'y': y};
  }
}

final class VesperHdrMetadata {
  const VesperHdrMetadata({
    this.hdrKind,
    this.dolbyVisionMode,
    this.probe,
    this.codec,
    this.sampleMimeType,
    this.colorPrimaries,
    this.colorSpace,
    this.colorRange,
    this.transferFunction,
    this.yCbCrMatrix,
    this.alternativeTransferCharacteristics,
    this.lumaBitDepth,
    this.chromaBitDepth,
    this.hdrStaticInfoPresent,
    this.hdrStaticInfoByteLength,
    this.hdrStaticInfoParseError,
    this.maxContentLightLevelNits,
    this.maxFrameAverageLightLevelNits,
    this.masteringDisplayColorVolumePresent,
    this.masteringDisplayColorVolumeByteLength,
    this.masteringDisplayColorVolumeParseError,
    this.masteringDisplayPrimary0,
    this.masteringDisplayPrimary1,
    this.masteringDisplayPrimary2,
    this.masteringDisplayWhitePoint,
    this.masteringDisplayMaxLuminanceNits,
    this.masteringDisplayMinLuminanceNits,
    this.dolbyVisionCodec,
    this.dolbyVisionProfile,
    this.dolbyVisionLevel,
    this.dolbyVisionCompatibility,
    this.dolbyVisionProfileFamily,
    this.dolbyVisionBaseLayer,
    this.dolbyVisionFallbackTarget,
    this.dolbyVisionBaseLayerEvidence,
    this.dolbyVisionBaseLayerTransferFunction,
  });

  factory VesperHdrMetadata.fromMap(Map<Object?, Object?> map) {
    return VesperHdrMetadata(
      hdrKind: _hdrKind(map['hdrKind']),
      dolbyVisionMode: _hdrDolbyVisionMode(map['dolbyVisionMode']),
      probe: _hdrString(map['probe']),
      codec: _hdrString(map['codec']),
      sampleMimeType: _hdrString(map['sampleMimeType']),
      colorPrimaries: _hdrString(map['colorPrimaries']),
      colorSpace: _hdrString(map['colorSpace']),
      colorRange: _hdrString(map['colorRange']),
      transferFunction: _hdrString(map['transferFunction']),
      yCbCrMatrix: _hdrString(map['yCbCrMatrix']),
      alternativeTransferCharacteristics:
          _hdrString(map['alternativeTransferCharacteristics']),
      lumaBitDepth: _hdrInt(map['lumaBitDepth']),
      chromaBitDepth: _hdrInt(map['chromaBitDepth']),
      hdrStaticInfoPresent: _hdrBool(map['hdrStaticInfoPresent']),
      hdrStaticInfoByteLength: _hdrInt(map['hdrStaticInfoByteLength']),
      hdrStaticInfoParseError: _hdrString(map['hdrStaticInfoParseError']),
      maxContentLightLevelNits: _hdrInt(map['maxContentLightLevelNits']),
      maxFrameAverageLightLevelNits:
          _hdrInt(map['maxFrameAverageLightLevelNits']),
      masteringDisplayColorVolumePresent:
          _hdrBool(map['masteringDisplayColorVolumePresent']),
      masteringDisplayColorVolumeByteLength:
          _hdrInt(map['masteringDisplayColorVolumeByteLength']),
      masteringDisplayColorVolumeParseError:
          _hdrString(map['masteringDisplayColorVolumeParseError']),
      masteringDisplayPrimary0: VesperHdrChromaticityPoint.fromDiagnostic(
        map['masteringDisplayPrimary0'],
      ),
      masteringDisplayPrimary1: VesperHdrChromaticityPoint.fromDiagnostic(
        map['masteringDisplayPrimary1'],
      ),
      masteringDisplayPrimary2: VesperHdrChromaticityPoint.fromDiagnostic(
        map['masteringDisplayPrimary2'],
      ),
      masteringDisplayWhitePoint: VesperHdrChromaticityPoint.fromDiagnostic(
        map['masteringDisplayWhitePoint'],
      ),
      masteringDisplayMaxLuminanceNits:
          _hdrDouble(map['masteringDisplayMaxLuminanceNits']),
      masteringDisplayMinLuminanceNits:
          _hdrDouble(map['masteringDisplayMinLuminanceNits']),
      dolbyVisionCodec: _hdrString(map['dolbyVisionCodec']),
      dolbyVisionProfile: _hdrInt(map['dolbyVisionProfile']),
      dolbyVisionLevel: _hdrInt(map['dolbyVisionLevel']),
      dolbyVisionCompatibility: _hdrString(map['dolbyVisionCompatibility']),
      dolbyVisionProfileFamily: _hdrString(map['dolbyVisionProfileFamily']),
      dolbyVisionBaseLayer: _hdrString(map['dolbyVisionBaseLayer']),
      dolbyVisionFallbackTarget: _hdrString(map['dolbyVisionFallbackTarget']),
      dolbyVisionBaseLayerEvidence:
          _hdrString(map['dolbyVisionBaseLayerEvidence']),
      dolbyVisionBaseLayerTransferFunction:
          _hdrString(map['dolbyVisionBaseLayerTransferFunction']),
    );
  }

  static VesperHdrMetadata? fromDiagnostics(
    Map<Object?, Object?> diagnostics, {
    VesperPlaybackCapabilityHdrKind? hdrKind,
    VesperPlaybackCapabilityDolbyVisionMode? dolbyVisionMode,
  }) {
    final metadata = VesperHdrMetadata(
      hdrKind: _effectiveHdrKind(diagnostics, hdrKind),
      dolbyVisionMode:
          dolbyVisionMode == VesperPlaybackCapabilityDolbyVisionMode.none
              ? null
              : dolbyVisionMode,
      probe: _firstHdrString(diagnostics, <String>[
        'runtimeFormatHdrMetadataProbe',
        'assetVideoHdrMetadataProbe',
        'assetProbe',
      ]),
      codec: _firstHdrString(diagnostics, <String>[
        'assetVideoCodec',
        'runtimeFormatCodecs',
      ]),
      sampleMimeType: _hdrString(diagnostics['runtimeFormatSampleMimeType']),
      colorPrimaries: _hdrString(diagnostics['assetVideoColorPrimaries']),
      colorSpace: _hdrString(diagnostics['runtimeFormatColorSpace']),
      colorRange: _hdrString(diagnostics['runtimeFormatColorRange']),
      transferFunction: _firstHdrString(diagnostics, <String>[
        'assetVideoTransferFunction',
        'runtimeFormatColorTransfer',
      ]),
      yCbCrMatrix: _hdrString(diagnostics['assetVideoYCbCrMatrix']),
      alternativeTransferCharacteristics: _hdrString(
          diagnostics['assetVideoAlternativeTransferCharacteristics']),
      lumaBitDepth: _hdrInt(diagnostics['runtimeFormatLumaBitDepth']),
      chromaBitDepth: _hdrInt(diagnostics['runtimeFormatChromaBitDepth']),
      hdrStaticInfoPresent:
          _hdrBool(diagnostics['runtimeFormatHdrStaticInfoPresent']),
      hdrStaticInfoByteLength:
          _hdrInt(diagnostics['runtimeFormatHdrStaticInfoByteLength']),
      hdrStaticInfoParseError:
          _hdrString(diagnostics['runtimeFormatHdrStaticInfoParseError']),
      maxContentLightLevelNits: _firstHdrInt(diagnostics, <String>[
        'assetVideoMaxContentLightLevelNits',
        'runtimeFormatMaxContentLightLevelNits',
      ]),
      maxFrameAverageLightLevelNits: _firstHdrInt(diagnostics, <String>[
        'assetVideoMaxFrameAverageLightLevelNits',
        'runtimeFormatMaxFrameAverageLightLevelNits',
      ]),
      masteringDisplayColorVolumePresent:
          _hdrBool(diagnostics['assetVideoMasteringDisplayColorVolumePresent']),
      masteringDisplayColorVolumeByteLength: _hdrInt(
          diagnostics['assetVideoMasteringDisplayColorVolumeByteLength']),
      masteringDisplayColorVolumeParseError: _hdrString(
          diagnostics['assetVideoMasteringDisplayColorVolumeParseError']),
      masteringDisplayPrimary0: VesperHdrChromaticityPoint.fromDiagnostic(
        diagnostics['assetVideoMasteringDisplayPrimary0'],
      ),
      masteringDisplayPrimary1: VesperHdrChromaticityPoint.fromDiagnostic(
        diagnostics['assetVideoMasteringDisplayPrimary1'],
      ),
      masteringDisplayPrimary2: VesperHdrChromaticityPoint.fromDiagnostic(
        diagnostics['assetVideoMasteringDisplayPrimary2'],
      ),
      masteringDisplayWhitePoint: VesperHdrChromaticityPoint.fromDiagnostic(
        diagnostics['assetVideoMasteringDisplayWhitePoint'],
      ),
      masteringDisplayMaxLuminanceNits:
          _hdrDouble(diagnostics['assetVideoMasteringDisplayMaxLuminanceNits']),
      masteringDisplayMinLuminanceNits:
          _hdrDouble(diagnostics['assetVideoMasteringDisplayMinLuminanceNits']),
      dolbyVisionCodec: _hdrString(diagnostics['dolbyVisionCodec']),
      dolbyVisionProfile: _hdrInt(diagnostics['dolbyVisionProfile']),
      dolbyVisionLevel: _hdrInt(diagnostics['dolbyVisionLevel']),
      dolbyVisionCompatibility:
          _hdrString(diagnostics['dolbyVisionCompatibility']),
      dolbyVisionProfileFamily:
          _hdrString(diagnostics['dolbyVisionProfileFamily']),
      dolbyVisionBaseLayer: _hdrString(diagnostics['dolbyVisionBaseLayer']),
      dolbyVisionFallbackTarget:
          _hdrString(diagnostics['dolbyVisionFallbackTarget']),
      dolbyVisionBaseLayerEvidence:
          _hdrString(diagnostics['dolbyVisionBaseLayerEvidence']),
      dolbyVisionBaseLayerTransferFunction:
          _hdrString(diagnostics['dolbyVisionBaseLayerTransferFunction']),
    );
    return metadata.toMap().isEmpty ? null : metadata;
  }

  final VesperPlaybackCapabilityHdrKind? hdrKind;
  final VesperPlaybackCapabilityDolbyVisionMode? dolbyVisionMode;
  final String? probe;
  final String? codec;
  final String? sampleMimeType;
  final String? colorPrimaries;
  final String? colorSpace;
  final String? colorRange;
  final String? transferFunction;
  final String? yCbCrMatrix;
  final String? alternativeTransferCharacteristics;
  final int? lumaBitDepth;
  final int? chromaBitDepth;
  final bool? hdrStaticInfoPresent;
  final int? hdrStaticInfoByteLength;
  final String? hdrStaticInfoParseError;
  final int? maxContentLightLevelNits;
  final int? maxFrameAverageLightLevelNits;
  final bool? masteringDisplayColorVolumePresent;
  final int? masteringDisplayColorVolumeByteLength;
  final String? masteringDisplayColorVolumeParseError;
  final VesperHdrChromaticityPoint? masteringDisplayPrimary0;
  final VesperHdrChromaticityPoint? masteringDisplayPrimary1;
  final VesperHdrChromaticityPoint? masteringDisplayPrimary2;
  final VesperHdrChromaticityPoint? masteringDisplayWhitePoint;
  final double? masteringDisplayMaxLuminanceNits;
  final double? masteringDisplayMinLuminanceNits;
  final String? dolbyVisionCodec;
  final int? dolbyVisionProfile;
  final int? dolbyVisionLevel;
  final String? dolbyVisionCompatibility;
  final String? dolbyVisionProfileFamily;
  final String? dolbyVisionBaseLayer;
  final String? dolbyVisionFallbackTarget;
  final String? dolbyVisionBaseLayerEvidence;
  final String? dolbyVisionBaseLayerTransferFunction;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      if (hdrKind != null) 'hdrKind': hdrKind?.name,
      if (dolbyVisionMode != null) 'dolbyVisionMode': dolbyVisionMode?.name,
      if (probe != null) 'probe': probe,
      if (codec != null) 'codec': codec,
      if (sampleMimeType != null) 'sampleMimeType': sampleMimeType,
      if (colorPrimaries != null) 'colorPrimaries': colorPrimaries,
      if (colorSpace != null) 'colorSpace': colorSpace,
      if (colorRange != null) 'colorRange': colorRange,
      if (transferFunction != null) 'transferFunction': transferFunction,
      if (yCbCrMatrix != null) 'yCbCrMatrix': yCbCrMatrix,
      if (alternativeTransferCharacteristics != null)
        'alternativeTransferCharacteristics':
            alternativeTransferCharacteristics,
      if (lumaBitDepth != null) 'lumaBitDepth': lumaBitDepth,
      if (chromaBitDepth != null) 'chromaBitDepth': chromaBitDepth,
      if (hdrStaticInfoPresent != null)
        'hdrStaticInfoPresent': hdrStaticInfoPresent,
      if (hdrStaticInfoByteLength != null)
        'hdrStaticInfoByteLength': hdrStaticInfoByteLength,
      if (hdrStaticInfoParseError != null)
        'hdrStaticInfoParseError': hdrStaticInfoParseError,
      if (maxContentLightLevelNits != null)
        'maxContentLightLevelNits': maxContentLightLevelNits,
      if (maxFrameAverageLightLevelNits != null)
        'maxFrameAverageLightLevelNits': maxFrameAverageLightLevelNits,
      if (masteringDisplayColorVolumePresent != null)
        'masteringDisplayColorVolumePresent':
            masteringDisplayColorVolumePresent,
      if (masteringDisplayColorVolumeByteLength != null)
        'masteringDisplayColorVolumeByteLength':
            masteringDisplayColorVolumeByteLength,
      if (masteringDisplayColorVolumeParseError != null)
        'masteringDisplayColorVolumeParseError':
            masteringDisplayColorVolumeParseError,
      if (masteringDisplayPrimary0 != null)
        'masteringDisplayPrimary0': masteringDisplayPrimary0?.toMap(),
      if (masteringDisplayPrimary1 != null)
        'masteringDisplayPrimary1': masteringDisplayPrimary1?.toMap(),
      if (masteringDisplayPrimary2 != null)
        'masteringDisplayPrimary2': masteringDisplayPrimary2?.toMap(),
      if (masteringDisplayWhitePoint != null)
        'masteringDisplayWhitePoint': masteringDisplayWhitePoint?.toMap(),
      if (masteringDisplayMaxLuminanceNits != null)
        'masteringDisplayMaxLuminanceNits': masteringDisplayMaxLuminanceNits,
      if (masteringDisplayMinLuminanceNits != null)
        'masteringDisplayMinLuminanceNits': masteringDisplayMinLuminanceNits,
      if (dolbyVisionCodec != null) 'dolbyVisionCodec': dolbyVisionCodec,
      if (dolbyVisionProfile != null) 'dolbyVisionProfile': dolbyVisionProfile,
      if (dolbyVisionLevel != null) 'dolbyVisionLevel': dolbyVisionLevel,
      if (dolbyVisionCompatibility != null)
        'dolbyVisionCompatibility': dolbyVisionCompatibility,
      if (dolbyVisionProfileFamily != null)
        'dolbyVisionProfileFamily': dolbyVisionProfileFamily,
      if (dolbyVisionBaseLayer != null)
        'dolbyVisionBaseLayer': dolbyVisionBaseLayer,
      if (dolbyVisionFallbackTarget != null)
        'dolbyVisionFallbackTarget': dolbyVisionFallbackTarget,
      if (dolbyVisionBaseLayerEvidence != null)
        'dolbyVisionBaseLayerEvidence': dolbyVisionBaseLayerEvidence,
      if (dolbyVisionBaseLayerTransferFunction != null)
        'dolbyVisionBaseLayerTransferFunction':
            dolbyVisionBaseLayerTransferFunction,
    };
  }
}
