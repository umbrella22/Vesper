part of '../../models.dart';

const _maxPipelineEventHookReports = 1024;
const _maxPipelineEventHookMeasurements = 128;
const _maxPipelineEventHookDiagnostics = 64;
const _maxPipelineEventHookAttributes = 32;
const _maxPipelineEventHookAttributeKeyBytes = 64;
const _maxPipelineEventHookAttributeValueBytes = 256;
const _maxPipelineEventHookMessageBytes = 256;

enum VesperPipelineEventHookResultStatus {
  accepted,
  rejected,
  error,
  unknown,
}

enum VesperPipelineEventHookErrorCode {
  invalidInput,
  payloadCodec,
  abiViolation,
  rejected,
  failed,
  protocolViolation,
  unknown,
}

enum VesperPipelineEventHookDiagnosticSeverity {
  info,
  warning,
  error,
  unknown,
}

final class VesperPipelineEventHookMeasurement {
  const VesperPipelineEventHookMeasurement({
    required this.name,
    required this.value,
    required this.unit,
    this.attributes = const <String, String>{},
  });

  factory VesperPipelineEventHookMeasurement.fromMap(
    Map<Object?, Object?> map,
  ) {
    final normalized = vesperDecodeMap(map);
    final value = _decodeDouble(normalized, 'value');
    return VesperPipelineEventHookMeasurement(
      name: normalized['name'] as String? ?? '',
      value: value != null && value.isFinite ? value : 0,
      unit: normalized['unit'] as String? ?? '',
      attributes: _decodeStringMap(normalized['attributes']),
    );
  }

  final String name;
  final double value;
  final String unit;
  final Map<String, String> attributes;

  Map<String, Object?> toMap() => <String, Object?>{
        'name': name,
        'value': value,
        'unit': unit,
        'attributes': attributes,
      };
}

final class VesperPipelineEventHookDiagnostic {
  const VesperPipelineEventHookDiagnostic({
    required this.code,
    required this.severity,
    required this.message,
    this.severityRawValue,
    this.attributes = const <String, String>{},
  });

  factory VesperPipelineEventHookDiagnostic.fromMap(
    Map<Object?, Object?> map,
  ) {
    final normalized = vesperDecodeMap(map);
    final rawSeverity = normalized['severity'];
    return VesperPipelineEventHookDiagnostic(
      code: normalized['code'] as String? ?? '',
      severity: _decodePipelineEventHookSeverity(rawSeverity),
      message: normalized['message'] as String? ?? '',
      severityRawValue: rawSeverity is String ? rawSeverity : null,
      attributes: _decodeStringMap(normalized['attributes']),
    );
  }

  final String code;
  final VesperPipelineEventHookDiagnosticSeverity severity;
  final String message;
  final String? severityRawValue;
  final Map<String, String> attributes;

  String get severityWireValue => severityRawValue ?? severity.name;

  Map<String, Object?> toMap() => <String, Object?>{
        'code': code,
        'severity': severityWireValue,
        'message': message,
        'attributes': attributes,
      };
}

final class VesperPipelineEventHookOutcome {
  const VesperPipelineEventHookOutcome({
    required this.accepted,
    this.measurements = const <VesperPipelineEventHookMeasurement>[],
    this.diagnostics = const <VesperPipelineEventHookDiagnostic>[],
  });

  factory VesperPipelineEventHookOutcome.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperPipelineEventHookOutcome(
      accepted: normalized['accepted'] is bool
          ? normalized['accepted']! as bool
          : false,
      measurements: _decodePipelineEventHookList(
        normalized['measurements'],
        VesperPipelineEventHookMeasurement.fromMap,
      ),
      diagnostics: _decodePipelineEventHookList(
        normalized['diagnostics'],
        VesperPipelineEventHookDiagnostic.fromMap,
      ),
    );
  }

  final bool accepted;
  final List<VesperPipelineEventHookMeasurement> measurements;
  final List<VesperPipelineEventHookDiagnostic> diagnostics;

  Map<String, Object?> toMap() => <String, Object?>{
        'accepted': accepted,
        'measurements': measurements.map((value) => value.toMap()).toList(),
        'diagnostics': diagnostics.map((value) => value.toMap()).toList(),
      };
}

final class VesperPipelineEventHookError {
  const VesperPipelineEventHookError({
    required this.code,
    required this.message,
    this.codeRawValue,
  });

  factory VesperPipelineEventHookError.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawCode = normalized['code'];
    return VesperPipelineEventHookError(
      code: _decodePipelineEventHookErrorCode(rawCode),
      message: normalized['message'] as String? ?? '',
      codeRawValue: rawCode is String ? rawCode : null,
    );
  }

  final VesperPipelineEventHookErrorCode code;
  final String message;
  final String? codeRawValue;

  String get codeWireValue => codeRawValue ?? code.name;

  Map<String, Object?> toMap() => <String, Object?>{
        'code': codeWireValue,
        'message': message,
      };
}

final class VesperPipelineEventHookResult {
  const VesperPipelineEventHookResult({
    required this.status,
    this.statusRawValue,
    this.outcome,
    this.error,
  });

  factory VesperPipelineEventHookResult.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawStatus = normalized['status'];
    final rawOutcome = _rawMap(normalized['outcome']);
    final rawError = _rawMap(normalized['error']);
    return VesperPipelineEventHookResult(
      status: _decodePipelineEventHookStatus(rawStatus),
      statusRawValue: rawStatus is String ? rawStatus : null,
      outcome: rawOutcome == null
          ? null
          : VesperPipelineEventHookOutcome.fromMap(rawOutcome),
      error: rawError == null
          ? null
          : VesperPipelineEventHookError.fromMap(rawError),
    );
  }

  final VesperPipelineEventHookResultStatus status;
  final String? statusRawValue;
  final VesperPipelineEventHookOutcome? outcome;
  final VesperPipelineEventHookError? error;

  String get statusWireValue => statusRawValue ?? status.name;

  Map<String, Object?> toMap() => <String, Object?>{
        'status': statusWireValue,
        'outcome': outcome?.toMap(),
        'error': error?.toMap(),
      };
}

final class VesperPipelineEventHookReport {
  const VesperPipelineEventHookReport({
    required this.pluginId,
    required this.capabilityInstanceId,
    required this.transport,
    required this.runId,
    required this.sessionId,
    required this.eventName,
    required this.result,
    this.transportRawValue,
  });

  factory VesperPipelineEventHookReport.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawTransport = normalized['transport'];
    return VesperPipelineEventHookReport(
      pluginId: normalized['pluginId'] as String? ?? '',
      capabilityInstanceId: normalized['capabilityInstanceId'] as String?,
      transport: _decodePluginTransport(rawTransport),
      transportRawValue: rawTransport is String ? rawTransport : null,
      runId: normalized['runId'] as String? ?? '',
      sessionId: normalized['sessionId'] as String? ?? '',
      eventName: normalized['eventName'] as String? ?? '',
      result: VesperPipelineEventHookResult.fromMap(
        vesperDecodeMap(normalized['result']),
      ),
    );
  }

  final String pluginId;
  final String? capabilityInstanceId;
  final VesperPluginTransport transport;
  final String? transportRawValue;
  final String runId;
  final String sessionId;
  final String eventName;
  final VesperPipelineEventHookResult result;

  String get transportWireValue => transportRawValue ?? transport.name;

  /// A validated reference when the host supplied a valid plugin identity.
  VesperPluginReference? get pluginReference {
    try {
      return VesperPluginReference(
        pluginId: pluginId,
        capabilityInstanceId: capabilityInstanceId,
        transport: transport,
        transportRawValue: transport == VesperPluginTransport.unknown
            ? transportWireValue
            : null,
      );
    } on FormatException {
      return null;
    }
  }

  Map<String, Object?> toMap() => <String, Object?>{
        'pluginId': pluginId,
        if (capabilityInstanceId != null)
          'capabilityInstanceId': capabilityInstanceId,
        'transport': transportWireValue,
        'runId': runId,
        'sessionId': sessionId,
        'eventName': eventName,
        'result': result.toMap(),
      };
}

final class VesperPipelineEventHookReportBatch {
  const VesperPipelineEventHookReportBatch({
    this.reports = const <VesperPipelineEventHookReport>[],
    this.droppedEvents = 0,
    this.droppedReports = 0,
    this.dispatcherError,
  });

  factory VesperPipelineEventHookReportBatch.fromMap(
    Map<Object?, Object?> map,
  ) {
    final normalized = vesperDecodeMap(map);
    final rawReports = normalized['reports'];
    if (rawReports is! Iterable) {
      return VesperPipelineEventHookReportBatch(
        droppedEvents: _decodeNonNegativeInt(normalized['droppedEvents']),
        droppedReports: _decodeNonNegativeInt(normalized['droppedReports']),
        dispatcherError: normalized['dispatcherError'] as String? ??
            'invalid pipeline EventHook report batch',
      );
    }
    if (rawReports.length > _maxPipelineEventHookReports) {
      return const VesperPipelineEventHookReportBatch(
        dispatcherError:
            'pipeline EventHook report batch exceeds the 1024-report limit',
      );
    }
    try {
      final reports = <VesperPipelineEventHookReport>[];
      for (final rawReport in rawReports) {
        final reportMap = _requirePipelineEventHookMap(
          rawReport,
          'pipeline EventHook report entry',
        );
        _validatePipelineEventHookReport(reportMap);
        reports.add(VesperPipelineEventHookReport.fromMap(reportMap));
      }
      final droppedEvents =
          _decodePipelineEventHookCounter(normalized['droppedEvents']);
      final droppedReports =
          _decodePipelineEventHookCounter(normalized['droppedReports']);
      final dispatcherError = normalized['dispatcherError'];
      if (dispatcherError != null && dispatcherError is! String) {
        throw const FormatException(
          'pipeline EventHook dispatcherError was not a string',
        );
      }
      return VesperPipelineEventHookReportBatch(
        reports: reports,
        droppedEvents: droppedEvents,
        droppedReports: droppedReports,
        dispatcherError: dispatcherError as String?,
      );
    } on FormatException catch (error) {
      final rawDispatcherError = normalized['dispatcherError'];
      return VesperPipelineEventHookReportBatch(
        dispatcherError:
            rawDispatcherError is String ? rawDispatcherError : error.message,
      );
    }
  }

  final List<VesperPipelineEventHookReport> reports;
  final int droppedEvents;
  final int droppedReports;
  final String? dispatcherError;

  bool get isEmpty =>
      reports.isEmpty &&
      droppedEvents == 0 &&
      droppedReports == 0 &&
      dispatcherError == null;

  Map<String, Object?> toMap() => <String, Object?>{
        'reports': reports.map((value) => value.toMap()).toList(),
        'droppedEvents': droppedEvents,
        'droppedReports': droppedReports,
        'dispatcherError': dispatcherError,
      };
}

List<T> _decodePipelineEventHookList<T>(
  Object? raw,
  T Function(Map<Object?, Object?> map) decoder,
) {
  if (raw is! Iterable) {
    return <T>[];
  }
  return raw
      .map(_rawMap)
      .whereType<Map<Object?, Object?>>()
      .map(decoder)
      .toList(growable: false);
}

Map<Object?, Object?> _requirePipelineEventHookMap(
  Object? raw,
  String field,
) {
  final map = _rawMap(raw);
  if (map == null) {
    throw FormatException('$field was not an object');
  }
  return map;
}

String _requirePipelineEventHookString(
  Map<Object?, Object?> map,
  String field, {
  int? maxBytes,
}) {
  final value = map[field];
  if (value is! String || value.isEmpty) {
    throw FormatException('pipeline EventHook $field was missing or empty');
  }
  if (maxBytes != null && utf8.encode(value).length > maxBytes) {
    throw FormatException(
      'pipeline EventHook $field exceeds the $maxBytes-byte limit',
    );
  }
  return value;
}

void _validatePipelineEventHookOptionalString(
  Map<Object?, Object?> map,
  String field,
) {
  final value = map[field];
  if (value != null && value is! String) {
    throw FormatException('pipeline EventHook $field was not a string');
  }
}

Map<Object?, Object?>? _optionalPipelineEventHookMap(
  Map<Object?, Object?> map,
  String field,
) {
  final value = map[field];
  if (value == null) {
    return null;
  }
  return _requirePipelineEventHookMap(value, 'pipeline EventHook $field');
}

Iterable<Object?> _optionalPipelineEventHookList(
  Map<Object?, Object?> map,
  String field,
) {
  final value = map[field];
  if (value == null) {
    return const <Object?>[];
  }
  if (value is! Iterable) {
    throw FormatException('pipeline EventHook $field was not an array');
  }
  return value.cast<Object?>();
}

void _validatePipelineEventHookAttributes(Object? raw) {
  if (raw == null) {
    return;
  }
  final map =
      _requirePipelineEventHookMap(raw, 'pipeline EventHook attributes');
  if (map.length > _maxPipelineEventHookAttributes) {
    throw const FormatException(
      'pipeline EventHook attributes exceed the 32-entry limit',
    );
  }
  for (final entry in map.entries) {
    final key = entry.key;
    final value = entry.value;
    if (key is! String ||
        key.isEmpty ||
        utf8.encode(key).length > _maxPipelineEventHookAttributeKeyBytes) {
      throw const FormatException(
        'pipeline EventHook attribute key exceeded the 64-byte limit',
      );
    }
    if (value is! String ||
        value.isEmpty ||
        utf8.encode(value).length > _maxPipelineEventHookAttributeValueBytes) {
      throw const FormatException(
        'pipeline EventHook attribute value exceeded the 256-byte limit',
      );
    }
  }
}

void _validatePipelineEventHookMeasurement(Object? raw) {
  final map = _requirePipelineEventHookMap(
    raw,
    'pipeline EventHook measurement',
  );
  _requirePipelineEventHookString(
    map,
    'name',
    maxBytes: _maxPipelineEventHookAttributeKeyBytes,
  );
  final value = map['value'];
  if (value is! num || !value.toDouble().isFinite) {
    throw const FormatException(
      'pipeline EventHook measurement value was not finite',
    );
  }
  _requirePipelineEventHookString(
    map,
    'unit',
    maxBytes: _maxPipelineEventHookAttributeKeyBytes,
  );
  _validatePipelineEventHookAttributes(map['attributes']);
}

void _validatePipelineEventHookDiagnostic(Object? raw) {
  final map = _requirePipelineEventHookMap(
    raw,
    'pipeline EventHook diagnostic',
  );
  _requirePipelineEventHookString(
    map,
    'code',
    maxBytes: _maxPipelineEventHookAttributeKeyBytes,
  );
  _requirePipelineEventHookString(map, 'severity');
  _requirePipelineEventHookString(
    map,
    'message',
    maxBytes: _maxPipelineEventHookMessageBytes,
  );
  _validatePipelineEventHookAttributes(map['attributes']);
}

void _validatePipelineEventHookOutcome(Map<Object?, Object?> map) {
  if (map['accepted'] is! bool) {
    throw const FormatException(
      'pipeline EventHook outcome accepted field was not a boolean',
    );
  }
  final measurements = _optionalPipelineEventHookList(map, 'measurements');
  if (measurements.length > _maxPipelineEventHookMeasurements) {
    throw const FormatException(
      'pipeline EventHook outcome exceeds the 128-measurement limit',
    );
  }
  for (final measurement in measurements) {
    _validatePipelineEventHookMeasurement(measurement);
  }
  final diagnostics = _optionalPipelineEventHookList(map, 'diagnostics');
  if (diagnostics.length > _maxPipelineEventHookDiagnostics) {
    throw const FormatException(
      'pipeline EventHook outcome exceeds the 64-diagnostic limit',
    );
  }
  for (final diagnostic in diagnostics) {
    _validatePipelineEventHookDiagnostic(diagnostic);
  }
}

void _validatePipelineEventHookError(Map<Object?, Object?> map) {
  _requirePipelineEventHookString(map, 'code');
  _requirePipelineEventHookString(
    map,
    'message',
    maxBytes: _maxPipelineEventHookMessageBytes,
  );
}

void _validatePipelineEventHookReport(Map<Object?, Object?> map) {
  _requirePipelineEventHookString(map, 'pluginId');
  _validatePipelineEventHookOptionalString(map, 'capabilityInstanceId');
  _requirePipelineEventHookString(map, 'transport');
  _requirePipelineEventHookString(map, 'runId');
  _requirePipelineEventHookString(map, 'sessionId');
  _requirePipelineEventHookString(map, 'eventName');
  final result = _requirePipelineEventHookMap(
    map['result'],
    'pipeline EventHook report result',
  );
  _requirePipelineEventHookString(result, 'status');
  final outcome = _optionalPipelineEventHookMap(result, 'outcome');
  if (outcome != null) {
    _validatePipelineEventHookOutcome(outcome);
  }
  final error = _optionalPipelineEventHookMap(result, 'error');
  if (error != null) {
    _validatePipelineEventHookError(error);
  }
}

int _decodePipelineEventHookCounter(Object? raw) {
  if (raw == null) {
    return 0;
  }
  if (raw is int && raw >= 0) {
    return raw;
  }
  throw const FormatException(
    'pipeline EventHook counter was not a non-negative integer',
  );
}

VesperPipelineEventHookResultStatus _decodePipelineEventHookStatus(
    Object? raw) {
  return switch (raw) {
    'accepted' => VesperPipelineEventHookResultStatus.accepted,
    'rejected' => VesperPipelineEventHookResultStatus.rejected,
    'error' => VesperPipelineEventHookResultStatus.error,
    _ => VesperPipelineEventHookResultStatus.unknown,
  };
}

VesperPipelineEventHookErrorCode _decodePipelineEventHookErrorCode(
    Object? raw) {
  return switch (raw) {
    'invalidInput' => VesperPipelineEventHookErrorCode.invalidInput,
    'payloadCodec' => VesperPipelineEventHookErrorCode.payloadCodec,
    'abiViolation' => VesperPipelineEventHookErrorCode.abiViolation,
    'rejected' => VesperPipelineEventHookErrorCode.rejected,
    'failed' => VesperPipelineEventHookErrorCode.failed,
    'protocolViolation' => VesperPipelineEventHookErrorCode.protocolViolation,
    _ => VesperPipelineEventHookErrorCode.unknown,
  };
}

VesperPipelineEventHookDiagnosticSeverity _decodePipelineEventHookSeverity(
  Object? raw,
) {
  return switch (raw) {
    'info' => VesperPipelineEventHookDiagnosticSeverity.info,
    'warning' => VesperPipelineEventHookDiagnosticSeverity.warning,
    'error' => VesperPipelineEventHookDiagnosticSeverity.error,
    _ => VesperPipelineEventHookDiagnosticSeverity.unknown,
  };
}

int _decodeNonNegativeInt(Object? raw) {
  return raw is int && raw >= 0 ? raw : 0;
}

VesperPluginTransport _decodePluginTransport(Object? raw) {
  return switch (raw) {
    'native' => VesperPluginTransport.native,
    'wasm' => VesperPluginTransport.wasm,
    _ => VesperPluginTransport.unknown,
  };
}
