part of '../models.dart';

final class VesperPlayerError {
  const VesperPlayerError({
    required this.message,
    required this.code,
    required this.category,
    required this.retriable,
    this.details = const <String, Object?>{},
    this.hdrCapabilityEvidence,
    this.codeRawValue,
    this.categoryRawValue,
  });

  factory VesperPlayerError.fromMap(Map<Object?, Object?> map) {
    final details = _decodeObjectMap(map['details']);
    final rawCode = map['code'];
    final rawCategory = map['category'];
    final codeRawValue = rawCode is String ? rawCode : null;
    final categoryRawValue = rawCategory is String ? rawCategory : null;
    return VesperPlayerError(
      message: map['message'] as String? ?? 'Unknown Vesper player error.',
      code: _decodeRequiredEnum(
        VesperPlayerErrorCode.values,
        rawCode,
        'code',
      ),
      category: _decodeRequiredEnum(
        VesperPlayerErrorCategory.values,
        rawCategory,
        'category',
      ),
      retriable: _decodeBool(map, 'retriable'),
      details: details,
      hdrCapabilityEvidence:
          VesperHdrCapabilityEvidence.tryFromDetails(details),
      codeRawValue: codeRawValue,
      categoryRawValue: categoryRawValue,
    );
  }

  final String message;
  final VesperPlayerErrorCode code;
  final VesperPlayerErrorCategory category;
  final bool retriable;
  final Map<String, Object?> details;
  final VesperHdrCapabilityEvidence? hdrCapabilityEvidence;
  final String? codeRawValue;
  final String? categoryRawValue;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'message': message,
      'code': codeRawValue ?? code.name,
      'category': categoryRawValue ?? category.name,
      'retriable': retriable,
      'details': details,
    };
  }
}

final class VesperHdrCapabilityEvidence {
  const VesperHdrCapabilityEvidence({
    required this.likelyHdrCapabilityIssue,
    required this.hdrKind,
    required this.recommendedPlaybackPath,
    this.confidence,
    this.errorCode,
    this.capabilityFailureCause,
    this.capabilityFailureAxis,
    this.hdrMetadata,
    this.diagnostics = const <String, Object?>{},
  });

  factory VesperHdrCapabilityEvidence.fromDetails(
    Map<String, Object?> details,
  ) {
    final likelyHdrCapabilityIssue =
        _decodeFlexibleBool(details['likelyHdrCapabilityIssue']);
    final hdrKind = _decodeEnum(
      VesperPlaybackCapabilityHdrKind.values,
      details['hdrKind'],
      VesperPlaybackCapabilityHdrKind.unknown,
    );
    final explicitHdrMetadata = _rawMap(details['hdrMetadata']);
    final hdrMetadata = explicitHdrMetadata != null
        ? VesperHdrMetadata.fromMap(explicitHdrMetadata)
        : VesperHdrMetadata.fromDiagnostics(details, hdrKind: hdrKind);
    return VesperHdrCapabilityEvidence(
      likelyHdrCapabilityIssue: likelyHdrCapabilityIssue,
      hdrKind: hdrKind,
      recommendedPlaybackPath: _decodeEnum(
        VesperRecommendedPlaybackPath.values,
        details['recommendedPlaybackPath'],
        VesperRecommendedPlaybackPath.systemPlayer,
      ),
      confidence: details['confidence'] as String?,
      errorCode: details['errorCode'] as String?,
      capabilityFailureCause: details['capabilityFailureCause'] as String?,
      capabilityFailureAxis: details['capabilityFailureAxis'] as String?,
      hdrMetadata: hdrMetadata,
      diagnostics: Map<String, Object?>.unmodifiable(
        details.map((key, value) => MapEntry(key.toString(), value))
          ..removeWhere(
              (key, _) => _hdrCapabilityEvidenceCoreKeys.contains(key)),
      ),
    );
  }

  static VesperHdrCapabilityEvidence? tryFromDetails(
    Map<String, Object?> details,
  ) {
    if (_decodeFlexibleBool(details['likelyHdrCapabilityIssue']) != true &&
        details['hdrKind'] == null &&
        details['hdrMetadata'] == null) {
      return null;
    }
    return VesperHdrCapabilityEvidence.fromDetails(details);
  }

  final bool likelyHdrCapabilityIssue;
  final VesperPlaybackCapabilityHdrKind hdrKind;
  final VesperRecommendedPlaybackPath recommendedPlaybackPath;
  final String? confidence;
  final String? errorCode;
  final String? capabilityFailureCause;
  final String? capabilityFailureAxis;
  final VesperHdrMetadata? hdrMetadata;
  final Map<String, Object?> diagnostics;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      if (likelyHdrCapabilityIssue)
        'likelyHdrCapabilityIssue': likelyHdrCapabilityIssue,
      'hdrKind': hdrKind.name,
      'recommendedPlaybackPath': recommendedPlaybackPath.name,
      if (confidence != null) 'confidence': confidence,
      if (errorCode != null) 'errorCode': errorCode,
      if (capabilityFailureCause != null)
        'capabilityFailureCause': capabilityFailureCause,
      if (capabilityFailureAxis != null)
        'capabilityFailureAxis': capabilityFailureAxis,
      if (hdrMetadata != null) 'hdrMetadata': hdrMetadata?.toMap(),
      ...diagnostics,
    };
  }
}

const Set<String> _hdrCapabilityEvidenceCoreKeys = <String>{
  'likelyHdrCapabilityIssue',
  'hdrKind',
  'recommendedPlaybackPath',
  'confidence',
  'errorCode',
  'capabilityFailureCause',
  'capabilityFailureAxis',
  'hdrMetadata',
};

bool _decodeFlexibleBool(Object? raw) {
  if (raw is bool) {
    return raw;
  }
  if (raw is String) {
    return raw == 'true';
  }
  return false;
}
