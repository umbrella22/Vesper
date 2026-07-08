part of '../download_models.dart';

final class VesperDownloadStaleResource {
  const VesperDownloadStaleResource({
    required this.taskId,
    this.resourceId,
    this.segmentId,
    this.uri,
    this.phase = VesperDownloadStaleResourcePhase.prepare,
    this.phaseRawValue,
    this.statusCode,
    this.receivedBytes = 0,
    required this.message,
  });

  factory VesperDownloadStaleResource.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawPhase = normalized['phase'];
    final phase = _decodeStaleResourcePhase(rawPhase);
    return VesperDownloadStaleResource(
      taskId: _decodeInt(normalized['taskId']) ?? 0,
      resourceId: normalized['resourceId'] as String?,
      segmentId: normalized['segmentId'] as String?,
      uri: normalized['uri'] as String?,
      phase: phase,
      phaseRawValue: _unknownEnumRawValue(
        rawPhase,
        isUnknown: rawPhase is String &&
            !VesperDownloadStaleResourcePhase.values.any(
              (value) => value.name == rawPhase,
            ),
      ),
      statusCode: _decodeInt(normalized['statusCode']),
      receivedBytes: _decodeInt(normalized['receivedBytes']) ?? 0,
      message: normalized['message'] as String? ?? '',
    );
  }

  final int taskId;
  final String? resourceId;
  final String? segmentId;
  final String? uri;
  final VesperDownloadStaleResourcePhase phase;
  final String? phaseRawValue;
  final int? statusCode;
  final int receivedBytes;
  final String message;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'taskId': taskId,
      'resourceId': resourceId,
      'segmentId': segmentId,
      'uri': uri,
      'phase': phaseRawValue ?? phase.name,
      'statusCode': statusCode,
      'receivedBytes': receivedBytes,
      'message': message,
    };
  }
}

final class VesperDownloadRecoveredTaskPlan {
  const VesperDownloadRecoveredTaskPlan({
    required this.source,
    required this.profile,
    required this.assetIndex,
  });

  factory VesperDownloadRecoveredTaskPlan.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperDownloadRecoveredTaskPlan(
      source:
          VesperDownloadSource.fromMap(vesperDecodeMap(normalized['source'])),
      profile: VesperDownloadProfile.fromMap(
        vesperDecodeMap(normalized['profile']),
      ),
      assetIndex: VesperDownloadAssetIndex.fromMap(
        vesperDecodeMap(normalized['assetIndex']),
      ),
    );
  }

  final VesperDownloadSource source;
  final VesperDownloadProfile profile;
  final VesperDownloadAssetIndex assetIndex;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'source': source.toMap(),
      'profile': profile.toMap(),
      'assetIndex': assetIndex.toMap(),
    };
  }
}

final class VesperDownloadProgressSnapshot {
  const VesperDownloadProgressSnapshot({
    this.receivedBytes = 0,
    this.totalBytes,
    this.receivedSegments = 0,
    this.totalSegments,
  });

  factory VesperDownloadProgressSnapshot.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperDownloadProgressSnapshot(
      receivedBytes: _decodeInt(normalized['receivedBytes']) ?? 0,
      totalBytes: _decodeInt(normalized['totalBytes']),
      receivedSegments: _decodeInt(normalized['receivedSegments']) ?? 0,
      totalSegments: _decodeInt(normalized['totalSegments']),
    );
  }

  final int receivedBytes;
  final int? totalBytes;
  final int receivedSegments;
  final int? totalSegments;

  double? get completionRatio {
    final total = totalBytes;
    if (total == null || total <= 0) {
      return null;
    }
    return receivedBytes / total;
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'receivedBytes': receivedBytes,
      'totalBytes': totalBytes,
      'receivedSegments': receivedSegments,
      'totalSegments': totalSegments,
    };
  }
}

final class VesperDownloadError {
  const VesperDownloadError({
    required this.code,
    required this.category,
    required this.retriable,
    required this.message,
    this.codeRawValue,
    this.categoryRawValue,
  });

  factory VesperDownloadError.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawCode = normalized['code'];
    final rawCategory = normalized['category'];
    final codeRawValue = rawCode is String ? rawCode : null;
    final categoryRawValue = rawCategory is String ? rawCategory : null;
    return VesperDownloadError(
      code: _decodeRequiredDownloadEnum(
        VesperPlayerErrorCode.values,
        rawCode,
        'code',
      ),
      category: _decodeRequiredDownloadEnum(
        VesperPlayerErrorCategory.values,
        rawCategory,
        'category',
      ),
      retriable: normalized['retriable'] as bool? ?? false,
      message: normalized['message'] as String? ?? 'Unknown download error.',
      codeRawValue: codeRawValue,
      categoryRawValue: categoryRawValue,
    );
  }

  final VesperPlayerErrorCode code;
  final VesperPlayerErrorCategory category;
  final bool retriable;
  final String message;
  final String? codeRawValue;
  final String? categoryRawValue;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'code': codeRawValue ?? code.name,
      'category': categoryRawValue ?? category.name,
      'retriable': retriable,
      'message': message,
    };
  }
}

T _decodeRequiredDownloadEnum<T extends Enum>(
  Iterable<T> values,
  Object? raw,
  String key,
) {
  if (raw is! String) {
    throw FormatException('Expected $key to be a string.');
  }
  for (final value in values) {
    if (value.name == raw) {
      return value;
    }
  }
  // Fall back to the `unknown` variant instead of throwing, so that
  // forward-compatible native enum additions do not crash the event stream.
  for (final value in values) {
    if (value.name == 'unknown') {
      return value;
    }
  }
  throw FormatException('Unknown $key: $raw.');
}

final class VesperDownloadTaskSnapshot {
  const VesperDownloadTaskSnapshot({
    required this.taskId,
    required this.assetId,
    required this.source,
    required this.profile,
    required this.state,
    required this.progress,
    required this.assetIndex,
    this.stateRawValue,
    this.error,
  });

  factory VesperDownloadTaskSnapshot.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawError = normalized['error'];
    return VesperDownloadTaskSnapshot(
      taskId: _decodeInt(normalized['taskId']) ?? 0,
      assetId: normalized['assetId'] as String? ?? '',
      source:
          VesperDownloadSource.fromMap(vesperDecodeMap(normalized['source'])),
      profile: VesperDownloadProfile.fromMap(
        vesperDecodeMap(normalized['profile']),
      ),
      state: _decodeDownloadState(normalized['state']),
      stateRawValue: normalized['state'] as String?,
      progress: VesperDownloadProgressSnapshot.fromMap(
        vesperDecodeMap(normalized['progress']),
      ),
      assetIndex: VesperDownloadAssetIndex.fromMap(
        vesperDecodeMap(normalized['assetIndex']),
      ),
      error: rawError == null
          ? null
          : VesperDownloadError.fromMap(vesperDecodeMap(rawError)),
    );
  }

  final int taskId;
  final String assetId;
  final VesperDownloadSource source;
  final VesperDownloadProfile profile;
  final VesperDownloadState state;
  final String? stateRawValue;
  final VesperDownloadProgressSnapshot progress;
  final VesperDownloadAssetIndex assetIndex;
  final VesperDownloadError? error;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'taskId': taskId,
      'assetId': assetId,
      'source': source.toMap(),
      'profile': profile.toMap(),
      'state': stateRawValue ?? state.name,
      'progress': progress.toMap(),
      'assetIndex': assetIndex.toMap(),
      'error': error?.toMap(),
    };
  }

  VesperDownloadTaskSnapshot copyWith({
    VesperDownloadState? state,
    String? stateRawValue,
    VesperDownloadProgressSnapshot? progress,
    VesperDownloadAssetIndex? assetIndex,
    Object? error = _vesperDownloadUnset,
  }) {
    return VesperDownloadTaskSnapshot(
      taskId: taskId,
      assetId: assetId,
      source: source,
      profile: profile,
      state: state ?? this.state,
      stateRawValue:
          stateRawValue ?? (state == null ? this.stateRawValue : state.name),
      progress: progress ?? this.progress,
      assetIndex: assetIndex ?? this.assetIndex,
      error: identical(error, _vesperDownloadUnset)
          ? this.error
          : error as VesperDownloadError?,
    );
  }
}

final class VesperDownloadTaskStatePatch {
  const VesperDownloadTaskStatePatch({
    required this.taskId,
    required this.state,
    required this.progress,
    this.stateRawValue,
    this.error,
    this.completedPath,
  });

  factory VesperDownloadTaskStatePatch.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawError = normalized['error'];
    return VesperDownloadTaskStatePatch(
      taskId: _decodeInt(normalized['taskId']) ?? 0,
      state: _decodeDownloadState(normalized['state']),
      stateRawValue: normalized['state'] as String?,
      progress: VesperDownloadProgressSnapshot.fromMap(
        vesperDecodeMap(normalized['progress']),
      ),
      error: rawError == null
          ? null
          : VesperDownloadError.fromMap(vesperDecodeMap(rawError)),
      completedPath: normalized['completedPath'] as String?,
    );
  }

  final int taskId;
  final VesperDownloadState state;
  final String? stateRawValue;
  final VesperDownloadProgressSnapshot progress;
  final VesperDownloadError? error;
  final String? completedPath;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'taskId': taskId,
      'state': stateRawValue ?? state.name,
      'progress': progress.toMap(),
      'error': error?.toMap(),
      'completedPath': completedPath,
    };
  }
}

final class VesperDownloadTaskProgressPatch {
  const VesperDownloadTaskProgressPatch({
    required this.taskId,
    required this.progress,
  });

  factory VesperDownloadTaskProgressPatch.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperDownloadTaskProgressPatch(
      taskId: _decodeInt(normalized['taskId']) ?? 0,
      progress: VesperDownloadProgressSnapshot.fromMap(
        vesperDecodeMap(normalized['progress']),
      ),
    );
  }

  final int taskId;
  final VesperDownloadProgressSnapshot progress;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'taskId': taskId,
      'progress': progress.toMap(),
    };
  }
}

final class VesperDownloadSnapshot {
  const VesperDownloadSnapshot({required this.tasks});

  const VesperDownloadSnapshot.initial()
      : tasks = const <VesperDownloadTaskSnapshot>[];

  factory VesperDownloadSnapshot.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawTasks = normalized['tasks'];
    return VesperDownloadSnapshot(
      tasks: switch (rawTasks) {
        final List<dynamic> values => values
            .whereType<Map>()
            .map(
              (value) => VesperDownloadTaskSnapshot.fromMap(
                Map<Object?, Object?>.from(value),
              ),
            )
            .toList(growable: false),
        _ => const <VesperDownloadTaskSnapshot>[],
      },
    );
  }

  final List<VesperDownloadTaskSnapshot> tasks;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'tasks': tasks.map((value) => value.toMap()).toList(growable: false),
    };
  }
}
