part of '../../models.dart';

final class VesperPerformanceDiagnosticsConfiguration {
  const VesperPerformanceDiagnosticsConfiguration({
    this.includeRawEvents = false,
    this.maxRawEvents = 256,
  });

  factory VesperPerformanceDiagnosticsConfiguration.fromMap(
    Map<Object?, Object?> map,
  ) {
    final rawMaxRawEvents = map['maxRawEvents'];
    final maxRawEvents = rawMaxRawEvents == null
        ? 256
        : _decodePerformanceInteger(rawMaxRawEvents, 'maxRawEvents');
    if (maxRawEvents < 0 || maxRawEvents > 2048) {
      throw const FormatException('maxRawEvents must be between 0 and 2048.');
    }
    return VesperPerformanceDiagnosticsConfiguration(
      includeRawEvents: map['includeRawEvents'] as bool? ?? false,
      maxRawEvents: maxRawEvents,
    );
  }

  final bool includeRawEvents;
  final int maxRawEvents;

  Map<String, Object?> toMap() => <String, Object?>{
        'includeRawEvents': includeRawEvents,
        'maxRawEvents': maxRawEvents,
      };
}

final class VesperPerformanceSampleClass {
  const VesperPerformanceSampleClass(this.rawValue);

  static const steady = VesperPerformanceSampleClass('steady');
  static const transition = VesperPerformanceSampleClass('transition');
  static const excluded = VesperPerformanceSampleClass('excluded');

  final String rawValue;

  @override
  bool operator ==(Object other) =>
      other is VesperPerformanceSampleClass && other.rawValue == rawValue;

  @override
  int get hashCode => rawValue.hashCode;
}

final class VesperPerformanceProbe {
  const VesperPerformanceProbe(this.rawValue);

  static const flutterFrameTiming =
      VesperPerformanceProbe('flutterFrameTiming');
  static const androidFrameMetrics =
      VesperPerformanceProbe('androidFrameMetrics');
  static const iosDisplayLink = VesperPerformanceProbe('iosDisplayLink');

  final String rawValue;

  @override
  bool operator ==(Object other) =>
      other is VesperPerformanceProbe && other.rawValue == rawValue;

  @override
  int get hashCode => rawValue.hashCode;
}

final class VesperPerformanceDiagnosisKind {
  const VesperPerformanceDiagnosisKind(this.rawValue);

  static const insufficientEvidence =
      VesperPerformanceDiagnosisKind('insufficientEvidence');
  static const noSignificantPressure =
      VesperPerformanceDiagnosisKind('noSignificantPressure');
  static const overlayCorrelatedUiPressure =
      VesperPerformanceDiagnosisKind('overlayCorrelatedUiPressure');
  static const hostUiPressureUncorrelated =
      VesperPerformanceDiagnosisKind('hostUiPressureUncorrelated');
  static const playbackPressure =
      VesperPerformanceDiagnosisKind('playbackPressure');
  static const mixedPressure = VesperPerformanceDiagnosisKind('mixedPressure');

  final String rawValue;

  @override
  bool operator ==(Object other) =>
      other is VesperPerformanceDiagnosisKind && other.rawValue == rawValue;

  @override
  int get hashCode => rawValue.hashCode;
}

final class VesperPerformanceConfidence {
  const VesperPerformanceConfidence(this.rawValue);

  static const low = VesperPerformanceConfidence('low');
  static const medium = VesperPerformanceConfidence('medium');
  static const high = VesperPerformanceConfidence('high');

  final String rawValue;

  @override
  bool operator ==(Object other) =>
      other is VesperPerformanceConfidence && other.rawValue == rawValue;

  @override
  int get hashCode => rawValue.hashCode;
}

final class VesperPerformanceDiagnosticSeverity {
  const VesperPerformanceDiagnosticSeverity(this.rawValue);

  static const info = VesperPerformanceDiagnosticSeverity('info');
  static const warning = VesperPerformanceDiagnosticSeverity('warning');
  static const error = VesperPerformanceDiagnosticSeverity('error');

  final String rawValue;

  @override
  bool operator ==(Object other) =>
      other is VesperPerformanceDiagnosticSeverity &&
      other.rawValue == rawValue;

  @override
  int get hashCode => rawValue.hashCode;
}

final class VesperPerformanceOverlayState {
  const VesperPerformanceOverlayState({
    required this.active,
    this.sampleClass = VesperPerformanceSampleClass.steady,
    this.loadedBasicItemCount,
    this.loadedAdvancedItemCount,
    this.advancedEffectsActive = false,
  });

  factory VesperPerformanceOverlayState.fromMap(Map<Object?, Object?> map) =>
      VesperPerformanceOverlayState(
        active: map['active'] as bool? ?? false,
        sampleClass: VesperPerformanceSampleClass(
          map['sampleClass'] as String? ?? 'steady',
        ),
        loadedBasicItemCount: map['loadedBasicItemCount'] as int?,
        loadedAdvancedItemCount: map['loadedAdvancedItemCount'] as int?,
        advancedEffectsActive: map['advancedEffectsActive'] as bool? ?? false,
      );

  final bool active;
  final VesperPerformanceSampleClass sampleClass;
  final int? loadedBasicItemCount;
  final int? loadedAdvancedItemCount;
  final bool advancedEffectsActive;

  Map<String, Object?> toMap() => <String, Object?>{
        'active': active,
        'sampleClass': sampleClass.rawValue,
        if (loadedBasicItemCount != null)
          'loadedBasicItemCount': loadedBasicItemCount,
        if (loadedAdvancedItemCount != null)
          'loadedAdvancedItemCount': loadedAdvancedItemCount,
        'advancedEffectsActive': advancedEffectsActive,
      };
}

final class VesperPerformanceFrameSample {
  const VesperPerformanceFrameSample({
    required this.loadNs,
    required this.budgetNs,
    required this.overlayState,
  })  : assert(loadNs >= 0),
        assert(budgetNs > 0);

  final int loadNs;
  final int budgetNs;
  final VesperPerformanceOverlayState overlayState;

  Map<String, Object?> toMap() => <String, Object?>{
        'loadNs': loadNs,
        'budgetNs': budgetNs,
        'overlayState': overlayState.toMap(),
      };
}

final class VesperPerformanceFrameCohort {
  const VesperPerformanceFrameCohort({
    required this.sampleCount,
    required this.jankCount,
    required this.severeJankCount,
    required this.jankRatio,
    required this.severeJankRatio,
    required this.minLoadNs,
    required this.p50LoadNs,
    required this.p95LoadNs,
    required this.maxLoadNs,
  });

  factory VesperPerformanceFrameCohort.fromMap(Map<Object?, Object?> map) {
    final sampleCount = _decodePerformanceNonnegativeInteger(
      map['sampleCount'],
      'sampleCount',
    );
    final jankCount = _decodePerformanceNonnegativeInteger(
      map['jankCount'],
      'jankCount',
    );
    final severeJankCount = _decodePerformanceNonnegativeInteger(
      map['severeJankCount'],
      'severeJankCount',
    );
    final minLoadNs = _decodePerformanceNonnegativeInteger(
      map['minLoadNs'],
      'minLoadNs',
    );
    final p50LoadNs = _decodePerformanceNonnegativeInteger(
      map['p50LoadNs'],
      'p50LoadNs',
    );
    final p95LoadNs = _decodePerformanceNonnegativeInteger(
      map['p95LoadNs'],
      'p95LoadNs',
    );
    final maxLoadNs = _decodePerformanceNonnegativeInteger(
      map['maxLoadNs'],
      'maxLoadNs',
    );
    if (severeJankCount > jankCount ||
        jankCount > sampleCount ||
        minLoadNs > p50LoadNs ||
        p50LoadNs > p95LoadNs ||
        p95LoadNs > maxLoadNs) {
      throw const FormatException(
        'Performance cohort values violate schema v1 ordering.',
      );
    }
    return VesperPerformanceFrameCohort(
      sampleCount: sampleCount,
      jankCount: jankCount,
      severeJankCount: severeJankCount,
      jankRatio: _decodePerformanceRatio(map['jankRatio'], 'jankRatio'),
      severeJankRatio: _decodePerformanceRatio(
        map['severeJankRatio'],
        'severeJankRatio',
      ),
      minLoadNs: minLoadNs,
      p50LoadNs: p50LoadNs,
      p95LoadNs: p95LoadNs,
      maxLoadNs: maxLoadNs,
    );
  }

  final int sampleCount;
  final int jankCount;
  final int severeJankCount;
  final double jankRatio;
  final double severeJankRatio;
  final int minLoadNs;
  final int p50LoadNs;
  final int p95LoadNs;
  final int maxLoadNs;

  double get minLoadMs => minLoadNs / 1000000;
  double get p50LoadMs => p50LoadNs / 1000000;
  double get p95LoadMs => p95LoadNs / 1000000;
  double get maxLoadMs => maxLoadNs / 1000000;

  Map<String, Object?> toMap() => <String, Object?>{
        'sampleCount': sampleCount,
        'jankCount': jankCount,
        'severeJankCount': severeJankCount,
        'jankRatio': jankRatio,
        'severeJankRatio': severeJankRatio,
        'minLoadNs': minLoadNs,
        'p50LoadNs': p50LoadNs,
        'p95LoadNs': p95LoadNs,
        'maxLoadNs': maxLoadNs,
      };
}

final class VesperPerformancePlaybackSummary {
  const VesperPerformancePlaybackSummary({
    required this.activeDurationNs,
    required this.droppedVideoFrames,
    required this.bufferingCount,
    required this.bufferingDurationNs,
    required this.stallCount,
  });

  factory VesperPerformancePlaybackSummary.fromMap(
    Map<Object?, Object?> map,
  ) =>
      VesperPerformancePlaybackSummary(
        activeDurationNs: _decodePerformanceNonnegativeInteger(
          map['activeDurationNs'],
          'activeDurationNs',
        ),
        droppedVideoFrames: _decodePerformanceNonnegativeInteger(
          map['droppedVideoFrames'],
          'droppedVideoFrames',
        ),
        bufferingCount: _decodePerformanceNonnegativeInteger(
          map['bufferingCount'],
          'bufferingCount',
        ),
        bufferingDurationNs: _decodePerformanceNonnegativeInteger(
          map['bufferingDurationNs'],
          'bufferingDurationNs',
        ),
        stallCount: _decodePerformanceNonnegativeInteger(
          map['stallCount'],
          'stallCount',
        ),
      );

  final int activeDurationNs;
  final int droppedVideoFrames;
  final int bufferingCount;
  final int bufferingDurationNs;
  final int stallCount;

  double get activeDurationMs => activeDurationNs / 1000000;
  double get bufferingDurationMs => bufferingDurationNs / 1000000;

  Map<String, Object?> toMap() => <String, Object?>{
        'activeDurationNs': activeDurationNs,
        'droppedVideoFrames': droppedVideoFrames,
        'bufferingCount': bufferingCount,
        'bufferingDurationNs': bufferingDurationNs,
        'stallCount': stallCount,
      };
}

final class VesperPerformanceDiagnosis {
  const VesperPerformanceDiagnosis({
    required this.kind,
    required this.confidence,
    this.evidenceCodes = const <String>[],
  });

  factory VesperPerformanceDiagnosis.fromMap(Map<Object?, Object?> map) =>
      VesperPerformanceDiagnosis(
        kind: VesperPerformanceDiagnosisKind(
          _decodePerformanceNonemptyString(map['kind'], 'diagnosis.kind'),
        ),
        confidence: VesperPerformanceConfidence(
          _decodePerformanceNonemptyString(
            map['confidence'],
            'diagnosis.confidence',
          ),
        ),
        evidenceCodes: _decodePerformanceStringList(
          map['evidenceCodes'],
          'diagnosis.evidenceCodes',
          requireNonemptyValues: true,
        ),
      );

  final VesperPerformanceDiagnosisKind kind;
  final VesperPerformanceConfidence confidence;
  final List<String> evidenceCodes;

  String get kindRawValue => kind.rawValue;
  String get confidenceRawValue => confidence.rawValue;

  Map<String, Object?> toMap() => <String, Object?>{
        'kind': kind.rawValue,
        'confidence': confidence.rawValue,
        'evidenceCodes': evidenceCodes,
      };
}

final class VesperPerformanceDiagnostic {
  const VesperPerformanceDiagnostic({
    required this.code,
    required this.severity,
    required this.message,
    this.attributes = const <String, String>{},
  });

  factory VesperPerformanceDiagnostic.fromMap(Map<Object?, Object?> map) =>
      VesperPerformanceDiagnostic(
        code: _decodePerformanceNonemptyString(
          map['code'],
          'diagnostic.code',
        ),
        severity: VesperPerformanceDiagnosticSeverity(
          _decodePerformanceNonemptyString(
            map['severity'],
            'diagnostic.severity',
          ),
        ),
        message: _decodePerformanceString(
          map['message'],
          'diagnostic.message',
        ),
        attributes: Map<String, String>.unmodifiable(
          _decodePerformanceStringMap(
            map['attributes'],
            'diagnostic.attributes',
          ),
        ),
      );

  final String code;
  final VesperPerformanceDiagnosticSeverity severity;
  final String message;
  final Map<String, String> attributes;

  String get severityRawValue => severity.rawValue;

  Map<String, Object?> toMap() => <String, Object?>{
        'code': code,
        'severity': severity.rawValue,
        'message': message,
        'attributes': attributes,
      };
}

final class VesperPerformanceDiagnosticsReport {
  const VesperPerformanceDiagnosticsReport({
    required this.schemaVersion,
    required this.runId,
    required this.sessionId,
    required this.platform,
    required this.probe,
    required this.durationNs,
    required this.frameBudgetNs,
    required this.cohorts,
    required this.playback,
    required this.diagnosis,
    required this.acceptedEvents,
    required this.droppedEvents,
    required this.rawEventsDropped,
    required this.diagnostics,
    required this.rawEvents,
    this.extensions = const <String, Object?>{},
  });

  factory VesperPerformanceDiagnosticsReport.fromMap(
      Map<Object?, Object?> map) {
    const knownKeys = <String>{
      'schemaVersion',
      'runId',
      'sessionId',
      'platform',
      'probe',
      'durationNs',
      'frameBudgetNs',
      'cohorts',
      'playback',
      'diagnosis',
      'acceptedEvents',
      'droppedEvents',
      'rawEventsDropped',
      'diagnostics',
      'rawEvents',
    };
    if (map.keys.any((key) => key is! String)) {
      throw const FormatException(
        'Performance report keys must be strings.',
      );
    }
    final schemaVersion = _decodePerformanceNonnegativeInteger(
      map['schemaVersion'],
      'schemaVersion',
    );
    if (schemaVersion != 1) {
      throw const FormatException(
        'Only performance diagnostics schema v1 is supported.',
      );
    }
    const cohortNames = <String>{
      'overlayInactive',
      'overlayActive',
      'transition',
      'excluded',
    };
    final cohortsMap = _decodePerformanceMap(map['cohorts'], 'cohorts');
    if (cohortsMap.length != cohortNames.length ||
        !cohortNames.every(cohortsMap.containsKey)) {
      throw const FormatException(
        'Performance report must contain all four schema v1 cohorts.',
      );
    }
    final cohorts = Map<String, VesperPerformanceFrameCohort>.unmodifiable(
      cohortsMap.map(
        (key, value) => MapEntry(
          key,
          VesperPerformanceFrameCohort.fromMap(
            _decodePerformanceMap(value, 'cohorts.$key'),
          ),
        ),
      ),
    );
    final frameBudgetNs = _decodePerformanceNonnegativeInteger(
      map['frameBudgetNs'],
      'frameBudgetNs',
    );
    if (frameBudgetNs == 0 &&
        cohorts.values.any((cohort) => cohort.sampleCount > 0)) {
      throw const FormatException(
        'frameBudgetNs must be positive when frame samples exist.',
      );
    }
    final diagnosis = VesperPerformanceDiagnosis.fromMap(
      _decodePerformanceMap(map['diagnosis'], 'diagnosis'),
    );
    final diagnostics = _decodePerformanceList(
      map['diagnostics'],
      'diagnostics',
    )
        .map(
          (value) => VesperPerformanceDiagnostic.fromMap(
            _decodePerformanceMap(value, 'diagnostics[]'),
          ),
        )
        .toList(growable: false);
    final diagnosisDiagnostics = diagnostics
        .where((diagnostic) => diagnostic.code == 'performance.diagnosis')
        .toList(growable: false);
    if (diagnosisDiagnostics.length != 1 ||
        diagnosisDiagnostics.single.attributes['kind'] !=
            diagnosis.kind.rawValue ||
        diagnosisDiagnostics.single.attributes['confidence'] !=
            diagnosis.confidence.rawValue ||
        diagnosisDiagnostics.single.attributes['evidenceCodes'] !=
            diagnosis.evidenceCodes.join(',')) {
      throw const FormatException(
        'Performance diagnosis fields are inconsistent.',
      );
    }
    final rawEvents = _decodePerformanceList(map['rawEvents'], 'rawEvents')
        .map((value) => _decodePerformanceMap(value, 'rawEvents[]'))
        .toList(growable: false);
    return VesperPerformanceDiagnosticsReport(
      schemaVersion: schemaVersion,
      runId: _decodePerformanceNonemptyString(map['runId'], 'runId'),
      sessionId:
          _decodePerformanceNonemptyString(map['sessionId'], 'sessionId'),
      platform: _decodePerformanceNonemptyString(map['platform'], 'platform'),
      probe: VesperPerformanceProbe(
        _decodePerformanceNonemptyString(map['probe'], 'probe'),
      ),
      durationNs: _decodePerformanceNonnegativeInteger(
        map['durationNs'],
        'durationNs',
      ),
      frameBudgetNs: frameBudgetNs,
      cohorts: cohorts,
      playback: VesperPerformancePlaybackSummary.fromMap(
        _decodePerformanceMap(map['playback'], 'playback'),
      ),
      diagnosis: diagnosis,
      acceptedEvents: _decodePerformanceNonnegativeInteger(
        map['acceptedEvents'],
        'acceptedEvents',
      ),
      droppedEvents: _decodePerformanceNonnegativeInteger(
        map['droppedEvents'],
        'droppedEvents',
      ),
      rawEventsDropped: _decodePerformanceNonnegativeInteger(
        map['rawEventsDropped'],
        'rawEventsDropped',
      ),
      diagnostics: diagnostics,
      rawEvents: rawEvents,
      extensions: Map<String, Object?>.unmodifiable(
        map.entries
            .where((entry) =>
                entry.key is String && !knownKeys.contains(entry.key))
            .map((entry) => MapEntry(entry.key! as String, entry.value))
            .toMap(),
      ),
    );
  }

  final int schemaVersion;
  final String runId;
  final String sessionId;
  final String platform;
  final VesperPerformanceProbe probe;
  final int durationNs;
  final int frameBudgetNs;
  final Map<String, VesperPerformanceFrameCohort> cohorts;
  final VesperPerformancePlaybackSummary playback;
  final VesperPerformanceDiagnosis diagnosis;
  final int acceptedEvents;
  final int droppedEvents;
  final int rawEventsDropped;
  final List<VesperPerformanceDiagnostic> diagnostics;
  final List<Map<String, Object?>> rawEvents;
  final Map<String, Object?> extensions;

  double get frameBudgetMs => frameBudgetNs / 1000000;
  double get durationMs => durationNs / 1000000;

  Map<String, Object?> toMap() => <String, Object?>{
        ...extensions,
        'schemaVersion': schemaVersion,
        'runId': runId,
        'sessionId': sessionId,
        'platform': platform,
        'probe': probe.rawValue,
        'durationNs': durationNs,
        'frameBudgetNs': frameBudgetNs,
        'cohorts': cohorts.map((key, value) => MapEntry(key, value.toMap())),
        'playback': playback.toMap(),
        'diagnosis': diagnosis.toMap(),
        'acceptedEvents': acceptedEvents,
        'droppedEvents': droppedEvents,
        'rawEventsDropped': rawEventsDropped,
        'diagnostics': diagnostics.map((value) => value.toMap()).toList(),
        'rawEvents': rawEvents,
      };

  String toJson() => jsonEncode(toMap());
}

extension<K, V> on Iterable<MapEntry<K, V>> {
  Map<K, V> toMap() => Map<K, V>.fromEntries(this);
}

int _decodePerformanceInteger(Object? value, String field) {
  if (value is! int) {
    throw FormatException('$field must be an integer.');
  }
  return value;
}

int _decodePerformanceNonnegativeInteger(Object? value, String field) {
  final decoded = _decodePerformanceInteger(value, field);
  if (decoded < 0) {
    throw FormatException('$field must be nonnegative.');
  }
  return decoded;
}

double _decodePerformanceRatio(Object? value, String field) {
  if (value is! num || !value.isFinite) {
    throw FormatException('$field must be a finite ratio.');
  }
  final decoded = value.toDouble();
  if (decoded < 0 || decoded > 1) {
    throw FormatException('$field must be between 0 and 1.');
  }
  return decoded;
}

String _decodePerformanceString(Object? value, String field) {
  if (value is! String) {
    throw FormatException('$field must be a string.');
  }
  return value;
}

String _decodePerformanceNonemptyString(Object? value, String field) {
  final decoded = _decodePerformanceString(value, field);
  if (decoded.isEmpty) {
    throw FormatException('$field must not be empty.');
  }
  return decoded;
}

Map<String, Object?> _decodePerformanceMap(Object? value, String field) {
  if (value is! Map || value.keys.any((key) => key is! String)) {
    throw FormatException('$field must be a string-keyed map.');
  }
  return Map<String, Object?>.unmodifiable(
    value.map((key, value) => MapEntry(key as String, value)),
  );
}

Map<String, String> _decodePerformanceStringMap(
  Object? value,
  String field,
) {
  final decoded = _decodePerformanceMap(value, field);
  if (decoded.values.any((value) => value is! String)) {
    throw FormatException('$field values must be strings.');
  }
  return decoded.map((key, value) => MapEntry(key, value! as String));
}

List<Object?> _decodePerformanceList(Object? value, String field) {
  if (value is! List) {
    throw FormatException('$field must be a list.');
  }
  return List<Object?>.unmodifiable(value);
}

List<String> _decodePerformanceStringList(
  Object? value,
  String field, {
  bool requireNonemptyValues = false,
}) {
  final decoded = _decodePerformanceList(value, field);
  if (decoded.any(
    (value) => value is! String || (requireNonemptyValues && value.isEmpty),
  )) {
    throw FormatException('$field must contain valid strings.');
  }
  if (requireNonemptyValues && decoded.isEmpty) {
    throw FormatException('$field must not be empty.');
  }
  return decoded.cast<String>();
}
