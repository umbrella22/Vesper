part of '../models.dart';

enum VesperRuntimeWarningDomain { frameProcessor, capability, unknown }

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
    this.kindRawValue,
    this.policyActionRawValue,
  });

  factory VesperFrameProcessorWarning.fromMap(Map<Object?, Object?> map) {
    final kindRawValue = map['kind'] as String?;
    final policyActionRawValue = map['policyAction'] as String?;
    return VesperFrameProcessorWarning(
      kind: _decodeEnum(
        VesperFrameProcessorWarningKind.values,
        kindRawValue,
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
        policyActionRawValue,
        VesperFrameProcessorPolicyAction.continuePlayback,
      ),
      message: map['message'] as String?,
      kindRawValue: kindRawValue,
      policyActionRawValue: policyActionRawValue,
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
  final String? kindRawValue;
  final String? policyActionRawValue;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'kind': kindRawValue ?? kind.name,
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
      'policyAction': policyActionRawValue ?? policyAction.name,
      if (message != null) 'message': message,
    };
  }
}

final class VesperRuntimeWarning {
  const VesperRuntimeWarning.frameProcessor(this.frameProcessor)
      : domain = VesperRuntimeWarningDomain.frameProcessor,
        capability = null,
        domainRawValue = null,
        rawPayload = const <String, Object?>{};

  const VesperRuntimeWarning.capability(this.capability)
      : domain = VesperRuntimeWarningDomain.capability,
        frameProcessor = null,
        domainRawValue = null,
        rawPayload = const <String, Object?>{};

  const VesperRuntimeWarning.unknown({
    required this.domainRawValue,
    required this.rawPayload,
  })  : domain = VesperRuntimeWarningDomain.unknown,
        frameProcessor = null,
        capability = null;

  factory VesperRuntimeWarning.fromMap(Map<Object?, Object?> map) {
    final domainRawValue = map['domain'] as String?;
    final domain = _decodeRuntimeWarningDomain(domainRawValue);
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
      VesperRuntimeWarningDomain.unknown => VesperRuntimeWarning.unknown(
          domainRawValue: domainRawValue,
          rawPayload: Map<String, Object?>.unmodifiable(vesperDecodeMap(map)),
        ),
    };
  }

  final VesperRuntimeWarningDomain domain;
  final VesperFrameProcessorWarning? frameProcessor;
  final VesperCapabilityWarning? capability;
  final String? domainRawValue;
  final Map<String, Object?> rawPayload;

  Map<String, Object?> toMap() {
    if (domain == VesperRuntimeWarningDomain.unknown) {
      return <String, Object?>{
        ...rawPayload,
        'domain': domainRawValue ?? domain.name,
      };
    }
    return <String, Object?>{
      'domain': domain.name,
      if (frameProcessor != null) 'frameProcessor': frameProcessor!.toMap(),
      if (capability != null) 'capability': capability!.toMap(),
    };
  }
}

VesperRuntimeWarningDomain _decodeRuntimeWarningDomain(String? raw) {
  final known = _decodeEnumOrNull(VesperRuntimeWarningDomain.values, raw);
  if (known != null) {
    return known;
  }
  return raw == null
      ? VesperRuntimeWarningDomain.frameProcessor
      : VesperRuntimeWarningDomain.unknown;
}
