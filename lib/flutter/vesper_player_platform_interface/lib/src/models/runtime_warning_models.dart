part of '../models.dart';

enum VesperRuntimeWarningDomain { frameProcessor }

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

final class VesperRuntimeWarning {
  const VesperRuntimeWarning.frameProcessor(this.frameProcessor)
      : domain = VesperRuntimeWarningDomain.frameProcessor;

  factory VesperRuntimeWarning.fromMap(Map<Object?, Object?> map) {
    final domain = _decodeEnum(
      VesperRuntimeWarningDomain.values,
      map['domain'],
      VesperRuntimeWarningDomain.frameProcessor,
    );
    final rawFrameProcessor = _rawMap(map['frameProcessor']);
    return switch (domain) {
      VesperRuntimeWarningDomain.frameProcessor =>
        VesperRuntimeWarning.frameProcessor(
          VesperFrameProcessorWarning.fromMap(
            rawFrameProcessor ?? const <Object?, Object?>{},
          ),
        ),
    };
  }

  final VesperRuntimeWarningDomain domain;
  final VesperFrameProcessorWarning frameProcessor;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'domain': domain.name,
      'frameProcessor': frameProcessor.toMap(),
    };
  }
}

