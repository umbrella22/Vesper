part of '../models.dart';

enum VesperRuntimeWarningDomain { frameProcessor, capability }

enum VesperFrameProcessorWarningKind {
  slow,
  deadlineMissed,
  backpressure,
  bypassActivated,
  lateOutputDropped,
  outputDropped,
  disabled,
  recovered,
  unsupported,
}

enum VesperFrameProcessorPolicyAction {
  continuePlayback,
  bypassOriginalFrame,
  dropOutput,
  disableProcessor,
  failPlayback,
  diagnosticsOnly,
}

final class VesperFrameProcessorWarning {
  const VesperFrameProcessorWarning({
    required this.kind,
    required this.pluginName,
    required this.processorIndex,
    required this.policyAction,
    this.frameId,
    this.framePtsUs,
    this.frameDurationUs,
    this.inputHandleKind,
    this.outputHandleKind,
    this.queueDepth,
    this.inFlightFrames,
    this.queueWaitUs,
    this.processTimeUs,
    this.submitToReadyUs,
    this.presentDeadlineUs,
    this.deadlineOverrunUs,
    this.consecutiveMissCount,
    this.message,
  });

  factory VesperFrameProcessorWarning.fromMap(Map<Object?, Object?> map) {
    return VesperFrameProcessorWarning(
      kind: _decodeEnum(
        VesperFrameProcessorWarningKind.values,
        map['kind'],
        VesperFrameProcessorWarningKind.unsupported,
      ),
      pluginName: map['pluginName'] as String? ?? '',
      processorIndex: _decodeInt(map, 'processorIndex') ?? 0,
      frameId: _decodeInt(map, 'frameId'),
      framePtsUs: _decodeInt(map, 'framePtsUs'),
      frameDurationUs: _decodeInt(map, 'frameDurationUs'),
      inputHandleKind: map['inputHandleKind'] as String?,
      outputHandleKind: map['outputHandleKind'] as String?,
      queueDepth: _decodeInt(map, 'queueDepth'),
      inFlightFrames: _decodeInt(map, 'inFlightFrames'),
      queueWaitUs: _decodeInt(map, 'queueWaitUs'),
      processTimeUs: _decodeInt(map, 'processTimeUs'),
      submitToReadyUs: _decodeInt(map, 'submitToReadyUs'),
      presentDeadlineUs: _decodeInt(map, 'presentDeadlineUs'),
      deadlineOverrunUs: _decodeInt(map, 'deadlineOverrunUs'),
      consecutiveMissCount: _decodeInt(map, 'consecutiveMissCount'),
      policyAction: _decodeEnum(
        VesperFrameProcessorPolicyAction.values,
        map['policyAction'],
        VesperFrameProcessorPolicyAction.continuePlayback,
      ),
      message: map['message'] as String?,
    );
  }

  final VesperFrameProcessorWarningKind kind;
  final String pluginName;
  final int processorIndex;
  final int? frameId;
  final int? framePtsUs;
  final int? frameDurationUs;
  final String? inputHandleKind;
  final String? outputHandleKind;
  final int? queueDepth;
  final int? inFlightFrames;
  final int? queueWaitUs;
  final int? processTimeUs;
  final int? submitToReadyUs;
  final int? presentDeadlineUs;
  final int? deadlineOverrunUs;
  final int? consecutiveMissCount;
  final VesperFrameProcessorPolicyAction policyAction;
  final String? message;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'kind': kind.name,
      'pluginName': pluginName,
      'processorIndex': processorIndex,
      if (frameId != null) 'frameId': frameId,
      if (framePtsUs != null) 'framePtsUs': framePtsUs,
      if (frameDurationUs != null) 'frameDurationUs': frameDurationUs,
      if (inputHandleKind != null) 'inputHandleKind': inputHandleKind,
      if (outputHandleKind != null) 'outputHandleKind': outputHandleKind,
      if (queueDepth != null) 'queueDepth': queueDepth,
      if (inFlightFrames != null) 'inFlightFrames': inFlightFrames,
      if (queueWaitUs != null) 'queueWaitUs': queueWaitUs,
      if (processTimeUs != null) 'processTimeUs': processTimeUs,
      if (submitToReadyUs != null) 'submitToReadyUs': submitToReadyUs,
      if (presentDeadlineUs != null) 'presentDeadlineUs': presentDeadlineUs,
      if (deadlineOverrunUs != null) 'deadlineOverrunUs': deadlineOverrunUs,
      if (consecutiveMissCount != null)
        'consecutiveMissCount': consecutiveMissCount,
      'policyAction': policyAction.name,
      if (message != null) 'message': message,
    };
  }
}

enum VesperCapabilityWarningReason { hdrNativeFrameUnsupported }

final class VesperCapabilityWarning {
  const VesperCapabilityWarning({
    required this.reason,
    required this.recommendedPlaybackPath,
    required this.hdrKind,
    this.likelyHdrCapabilityIssue = false,
    this.confidence,
    this.errorCode,
    this.capabilityFailureCause,
    this.capabilityFailureAxis,
    this.appProbeConvergence,
    this.hdrMetadata,
    this.diagnostics = const <String, Object?>{},
    this.message,
  });

  factory VesperCapabilityWarning.fromMap(Map<Object?, Object?> map) {
    final diagnostics = <String, Object?>{};
    for (final entry in map.entries) {
      final key = entry.key;
      if (key is! String || _capabilityWarningCoreKeys.contains(key)) {
        continue;
      }
      diagnostics[key] = entry.value;
    }
    final explicitHdrMetadata = _rawMap(map['hdrMetadata']);
    final hdrKind = _decodeEnum(
      VesperPlaybackCapabilityHdrKind.values,
      map['hdrKind'],
      VesperPlaybackCapabilityHdrKind.unknown,
    );
    return VesperCapabilityWarning(
      reason: _decodeEnum(
        VesperCapabilityWarningReason.values,
        map['reason'],
        VesperCapabilityWarningReason.hdrNativeFrameUnsupported,
      ),
      recommendedPlaybackPath: _decodeEnum(
        VesperRecommendedPlaybackPath.values,
        map['recommendedPlaybackPath'],
        VesperRecommendedPlaybackPath.systemPlayer,
      ),
      hdrKind: hdrKind,
      likelyHdrCapabilityIssue: _decodeBool(
        map,
        'likelyHdrCapabilityIssue',
      ),
      confidence: map['confidence'] as String?,
      errorCode: map['errorCode'] as String?,
      capabilityFailureCause: map['capabilityFailureCause'] as String?,
      capabilityFailureAxis: map['capabilityFailureAxis'] as String?,
      appProbeConvergence: VesperAppProbeConvergence.tryFromMap(map),
      hdrMetadata: explicitHdrMetadata != null
          ? VesperHdrMetadata.fromMap(explicitHdrMetadata)
          : VesperHdrMetadata.fromDiagnostics(map, hdrKind: hdrKind),
      diagnostics: Map<String, Object?>.unmodifiable(diagnostics),
      message: map['message'] as String?,
    );
  }

  final VesperCapabilityWarningReason reason;
  final VesperRecommendedPlaybackPath recommendedPlaybackPath;
  final VesperPlaybackCapabilityHdrKind hdrKind;
  final bool likelyHdrCapabilityIssue;
  final String? confidence;
  final String? errorCode;
  final String? capabilityFailureCause;
  final String? capabilityFailureAxis;
  final VesperAppProbeConvergence? appProbeConvergence;
  final VesperHdrMetadata? hdrMetadata;
  final Map<String, Object?> diagnostics;
  final String? message;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'reason': reason.name,
      'recommendedPlaybackPath': recommendedPlaybackPath.name,
      'hdrKind': hdrKind.name,
      if (likelyHdrCapabilityIssue)
        'likelyHdrCapabilityIssue': likelyHdrCapabilityIssue,
      if (confidence != null) 'confidence': confidence,
      if (errorCode != null) 'errorCode': errorCode,
      if (capabilityFailureCause != null)
        'capabilityFailureCause': capabilityFailureCause,
      if (capabilityFailureAxis != null)
        'capabilityFailureAxis': capabilityFailureAxis,
      if (appProbeConvergence != null) ...appProbeConvergence!.toMap(),
      if (hdrMetadata != null) 'hdrMetadata': hdrMetadata?.toMap(),
      ...diagnostics,
      if (message != null) 'message': message,
    };
  }
}

final class VesperAppProbeConvergence {
  const VesperAppProbeConvergence({
    required this.status,
    required this.recommendedPlaybackPath,
    required this.confidence,
    required this.hdrKind,
    required this.dolbyVisionMode,
    this.missingCapabilities = const <String>[],
    this.sourceUri,
    this.sourceProtocol,
    this.sourceMatchesRuntime,
    this.sourceMatchBasis,
    this.runtimeRecommendedPathMatches,
    this.runtimeHdrKindMatches,
    this.runtimeDolbyVisionModeMatches,
    this.runtimeSystemPlayerRecommendationConfirmed,
    this.runtimeHdrKindPresent,
    this.runtimeDolbyVisionModePresent,
    this.displayHdrSupported,
    this.displayFrameRateSupported,
    this.codecFormatSupported,
    this.codecFormatMissingCapability,
    this.codecFormatSampleMimeType,
    this.codecFormatCodecs,
    this.codecFormatWidth,
    this.codecFormatHeight,
    this.codecFormatFrameRate,
  });

  factory VesperAppProbeConvergence.fromMap(Map<Object?, Object?> map) {
    return VesperAppProbeConvergence(
      status: _decodeEnum(
        VesperPlaybackCapabilityProbeStatus.values,
        map['appProbeStatus'],
        VesperPlaybackCapabilityProbeStatus.unknown,
      ),
      recommendedPlaybackPath: _decodeEnum(
        VesperRecommendedPlaybackPath.values,
        map['appProbeRecommendedPlaybackPath'],
        VesperRecommendedPlaybackPath.systemPlayer,
      ),
      confidence: _decodeEnum(
        VesperPlaybackCapabilityConfidence.values,
        map['appProbeConfidence'],
        VesperPlaybackCapabilityConfidence.codecOnly,
      ),
      hdrKind: _decodeEnum(
        VesperPlaybackCapabilityHdrKind.values,
        map['appProbeHdrKind'],
        VesperPlaybackCapabilityHdrKind.unknown,
      ),
      dolbyVisionMode: _decodeEnum(
        VesperPlaybackCapabilityDolbyVisionMode.values,
        map['appProbeDolbyVisionMode'],
        VesperPlaybackCapabilityDolbyVisionMode.none,
      ),
      missingCapabilities:
          _decodeCapabilityList(map['appProbeMissingCapabilities']),
      sourceUri: map['appProbeSourceUri'] as String?,
      sourceProtocol: _decodeEnumOrNull(
        VesperPlayerSourceProtocol.values,
        map['appProbeSourceProtocol'],
      ),
      sourceMatchesRuntime:
          _decodeOptionalFlexibleBool(map['appProbeSourceMatchesRuntime']),
      sourceMatchBasis: map['appProbeSourceMatchBasis'] as String?,
      runtimeRecommendedPathMatches: _decodeOptionalFlexibleBool(
        map['appProbeRuntimeRecommendedPathMatches'],
      ),
      runtimeHdrKindMatches: _decodeOptionalFlexibleBool(
        map['appProbeRuntimeHdrKindMatches'],
      ),
      runtimeDolbyVisionModeMatches: _decodeOptionalFlexibleBool(
        map['appProbeRuntimeDolbyVisionModeMatches'],
      ),
      runtimeSystemPlayerRecommendationConfirmed: _decodeOptionalFlexibleBool(
        map['appProbeRuntimeSystemPlayerRecommendationConfirmed'],
      ),
      runtimeHdrKindPresent:
          _decodeOptionalFlexibleBool(map['appProbeRuntimeHdrKindPresent']),
      runtimeDolbyVisionModePresent: _decodeOptionalFlexibleBool(
        map['appProbeRuntimeDolbyVisionModePresent'],
      ),
      displayHdrSupported:
          _decodeOptionalFlexibleBool(map['appProbeDisplayHdrSupported']),
      displayFrameRateSupported: _decodeOptionalFlexibleBool(
        map['appProbeDisplayFrameRateSupported'],
      ),
      codecFormatSupported:
          _decodeOptionalFlexibleBool(map['appProbeCodecFormatSupported']),
      codecFormatMissingCapability:
          map['appProbeCodecFormatMissingCapability'] as String?,
      codecFormatSampleMimeType:
          map['appProbeCodecFormatSampleMimeType'] as String?,
      codecFormatCodecs: map['appProbeCodecFormatCodecs'] as String?,
      codecFormatWidth: _decodeFlexibleInt(map['appProbeCodecFormatWidth']),
      codecFormatHeight: _decodeFlexibleInt(map['appProbeCodecFormatHeight']),
      codecFormatFrameRate:
          _decodeFlexibleDouble(map['appProbeCodecFormatFrameRate']),
    );
  }

  static VesperAppProbeConvergence? tryFromMap(Map<Object?, Object?> map) {
    if (map['appProbeStatus'] == null &&
        map['appProbeRecommendedPlaybackPath'] == null &&
        map['appProbeHdrKind'] == null) {
      return null;
    }
    return VesperAppProbeConvergence.fromMap(map);
  }

  final VesperPlaybackCapabilityProbeStatus status;
  final VesperRecommendedPlaybackPath recommendedPlaybackPath;
  final VesperPlaybackCapabilityConfidence confidence;
  final VesperPlaybackCapabilityHdrKind hdrKind;
  final VesperPlaybackCapabilityDolbyVisionMode dolbyVisionMode;
  final List<String> missingCapabilities;
  final String? sourceUri;
  final VesperPlayerSourceProtocol? sourceProtocol;
  final bool? sourceMatchesRuntime;
  final String? sourceMatchBasis;
  final bool? runtimeRecommendedPathMatches;
  final bool? runtimeHdrKindMatches;
  final bool? runtimeDolbyVisionModeMatches;
  final bool? runtimeSystemPlayerRecommendationConfirmed;
  final bool? runtimeHdrKindPresent;
  final bool? runtimeDolbyVisionModePresent;
  final bool? displayHdrSupported;
  final bool? displayFrameRateSupported;
  final bool? codecFormatSupported;
  final String? codecFormatMissingCapability;
  final String? codecFormatSampleMimeType;
  final String? codecFormatCodecs;
  final int? codecFormatWidth;
  final int? codecFormatHeight;
  final double? codecFormatFrameRate;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'appProbeStatus': status.name,
      'appProbeRecommendedPlaybackPath': recommendedPlaybackPath.name,
      'appProbeConfidence': confidence.name,
      'appProbeHdrKind': hdrKind.name,
      'appProbeDolbyVisionMode': dolbyVisionMode.name,
      if (missingCapabilities.isNotEmpty)
        'appProbeMissingCapabilities': missingCapabilities,
      if (sourceUri != null) 'appProbeSourceUri': sourceUri,
      if (sourceProtocol != null)
        'appProbeSourceProtocol': sourceProtocol?.name,
      if (sourceMatchesRuntime != null)
        'appProbeSourceMatchesRuntime': sourceMatchesRuntime,
      if (sourceMatchBasis != null)
        'appProbeSourceMatchBasis': sourceMatchBasis,
      if (runtimeRecommendedPathMatches != null)
        'appProbeRuntimeRecommendedPathMatches': runtimeRecommendedPathMatches,
      if (runtimeHdrKindMatches != null)
        'appProbeRuntimeHdrKindMatches': runtimeHdrKindMatches,
      if (runtimeDolbyVisionModeMatches != null)
        'appProbeRuntimeDolbyVisionModeMatches': runtimeDolbyVisionModeMatches,
      if (runtimeSystemPlayerRecommendationConfirmed != null)
        'appProbeRuntimeSystemPlayerRecommendationConfirmed':
            runtimeSystemPlayerRecommendationConfirmed,
      if (runtimeHdrKindPresent != null)
        'appProbeRuntimeHdrKindPresent': runtimeHdrKindPresent,
      if (runtimeDolbyVisionModePresent != null)
        'appProbeRuntimeDolbyVisionModePresent': runtimeDolbyVisionModePresent,
      if (displayHdrSupported != null)
        'appProbeDisplayHdrSupported': displayHdrSupported,
      if (displayFrameRateSupported != null)
        'appProbeDisplayFrameRateSupported': displayFrameRateSupported,
      if (codecFormatSupported != null)
        'appProbeCodecFormatSupported': codecFormatSupported,
      if (codecFormatMissingCapability != null)
        'appProbeCodecFormatMissingCapability': codecFormatMissingCapability,
      if (codecFormatSampleMimeType != null)
        'appProbeCodecFormatSampleMimeType': codecFormatSampleMimeType,
      if (codecFormatCodecs != null)
        'appProbeCodecFormatCodecs': codecFormatCodecs,
      if (codecFormatWidth != null)
        'appProbeCodecFormatWidth': codecFormatWidth,
      if (codecFormatHeight != null)
        'appProbeCodecFormatHeight': codecFormatHeight,
      if (codecFormatFrameRate != null)
        'appProbeCodecFormatFrameRate': codecFormatFrameRate,
    };
  }
}

const Set<String> _capabilityWarningCoreKeys = <String>{
  'reason',
  'recommendedPlaybackPath',
  'hdrKind',
  'likelyHdrCapabilityIssue',
  'confidence',
  'errorCode',
  'capabilityFailureCause',
  'capabilityFailureAxis',
  ..._appProbeConvergenceKeys,
  'hdrMetadata',
  'message',
};

const Set<String> _appProbeConvergenceKeys = <String>{
  'appProbeStatus',
  'appProbeRecommendedPlaybackPath',
  'appProbeConfidence',
  'appProbeHdrKind',
  'appProbeDolbyVisionMode',
  'appProbeMissingCapabilities',
  'appProbeSourceUri',
  'appProbeSourceProtocol',
  'appProbeSourceMatchesRuntime',
  'appProbeSourceMatchBasis',
  'appProbeRuntimeRecommendedPathMatches',
  'appProbeRuntimeHdrKindMatches',
  'appProbeRuntimeDolbyVisionModeMatches',
  'appProbeRuntimeSystemPlayerRecommendationConfirmed',
  'appProbeRuntimeHdrKindPresent',
  'appProbeRuntimeDolbyVisionModePresent',
  'appProbeDisplayHdrSupported',
  'appProbeDisplayFrameRateSupported',
  'appProbeCodecFormatSupported',
  'appProbeCodecFormatMissingCapability',
  'appProbeCodecFormatSampleMimeType',
  'appProbeCodecFormatCodecs',
  'appProbeCodecFormatWidth',
  'appProbeCodecFormatHeight',
  'appProbeCodecFormatFrameRate',
};

T? _decodeEnumOrNull<T extends Enum>(Iterable<T> values, Object? raw) {
  if (raw is String) {
    for (final value in values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return null;
}

List<String> _decodeCapabilityList(Object? raw) {
  if (raw is String) {
    return raw
        .split(',')
        .map((value) => value.trim())
        .where((value) => value.isNotEmpty)
        .toList(growable: false);
  }
  return _decodeStringList(raw);
}

bool? _decodeOptionalFlexibleBool(Object? raw) {
  if (raw is bool) {
    return raw;
  }
  if (raw is String) {
    return raw == 'true'
        ? true
        : raw == 'false'
            ? false
            : null;
  }
  return null;
}

int? _decodeFlexibleInt(Object? raw) {
  if (raw is int) {
    return raw;
  }
  if (raw is num) {
    return raw.toInt();
  }
  if (raw is String) {
    return int.tryParse(raw);
  }
  return null;
}

double? _decodeFlexibleDouble(Object? raw) {
  if (raw is num) {
    return raw.toDouble();
  }
  if (raw is String) {
    return double.tryParse(raw);
  }
  return null;
}

final class VesperRuntimeWarning {
  const VesperRuntimeWarning.frameProcessor(this.frameProcessor)
      : domain = VesperRuntimeWarningDomain.frameProcessor,
        capability = null;

  const VesperRuntimeWarning.capability(this.capability)
      : domain = VesperRuntimeWarningDomain.capability,
        frameProcessor = null;

  factory VesperRuntimeWarning.fromMap(Map<Object?, Object?> map) {
    final domain = _decodeEnum(
      VesperRuntimeWarningDomain.values,
      map['domain'],
      VesperRuntimeWarningDomain.frameProcessor,
    );
    final rawFrameProcessor = _rawMap(map['frameProcessor']);
    final rawCapability = _rawMap(map['capability']);
    return switch (domain) {
      VesperRuntimeWarningDomain.frameProcessor =>
        VesperRuntimeWarning.frameProcessor(
          VesperFrameProcessorWarning.fromMap(
            rawFrameProcessor ?? const <Object?, Object?>{},
          ),
        ),
      VesperRuntimeWarningDomain.capability => VesperRuntimeWarning.capability(
          VesperCapabilityWarning.fromMap(
            rawCapability ?? const <Object?, Object?>{},
          ),
        ),
    };
  }

  final VesperRuntimeWarningDomain domain;
  final VesperFrameProcessorWarning? frameProcessor;
  final VesperCapabilityWarning? capability;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'domain': domain.name,
      if (frameProcessor != null) 'frameProcessor': frameProcessor!.toMap(),
      if (capability != null) 'capability': capability!.toMap(),
    };
  }
}
