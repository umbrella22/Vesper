/// Resource limits for a source session independent of any player.
final class VesperSourceSessionConfiguration {
  const VesperSourceSessionConfiguration({
    this.maxSources = 128,
    this.maxConcurrentPreloads = 2,
    this.maxPendingPreloads = 4,
    this.maxMemoryBytes = 8 * 1024 * 1024,
  });

  final int maxSources;
  final int maxConcurrentPreloads;
  final int maxPendingPreloads;
  final int maxMemoryBytes;

  Map<String, Object?> toMap() {
    if (maxSources < 1 ||
        maxSources > 512 ||
        maxConcurrentPreloads < 1 ||
        maxConcurrentPreloads > 4 ||
        maxPendingPreloads < 0 ||
        maxPendingPreloads > 32 ||
        maxMemoryBytes < 0 ||
        maxMemoryBytes > 16 * 1024 * 1024) {
      throw ArgumentError(
          'Source session limits are outside the supported bounds.');
    }
    return <String, Object?>{
      'maxSources': maxSources,
      'maxConcurrentPreloads': maxConcurrentPreloads,
      'maxPendingPreloads': maxPendingPreloads,
      'maxMemoryBytes': maxMemoryBytes,
    };
  }
}

/// An opaque native registration. A reference never contains source credentials.
class VesperSourceReference {
  const VesperSourceReference({
    required this.sessionId,
    required this.sourceId,
    this.expiresAtEpochMs,
  });

  factory VesperSourceReference.fromMap(Map<Object?, Object?> map) =>
      VesperSourceReference(
        sessionId: _requiredString(map, 'sessionId'),
        sourceId: _requiredString(map, 'sourceId'),
        expiresAtEpochMs: _optionalInt(map, 'expiresAtEpochMs'),
      );

  final String sessionId;
  final String sourceId;
  final int? expiresAtEpochMs;

  Map<String, Object?> toMap() => <String, Object?>{
        'sessionId': sessionId,
        'sourceId': sourceId,
        if (expiresAtEpochMs != null) 'expiresAtEpochMs': expiresAtEpochMs,
      };

  @override
  bool operator ==(Object other) =>
      other is VesperSourceReference &&
      other.sessionId == sessionId &&
      other.sourceId == sourceId;

  @override
  int get hashCode => Object.hash(sessionId, sourceId);
}

enum VesperSourcePreloadStatus {
  queued,
  running,
  completed,
  failed,
  unsupported,
  cancelled,
  unknown,
}

enum VesperSourcePreloadGoal {
  progressiveRange,
  dashSegmentBaseStartup,
  unsupported,
  unknown,
}

/// Whether the native playback path can consume the retained preload bytes.
enum VesperSourcePreloadReuse { playbackReusable, downloadOnly, none, unknown }

final class VesperSourcePreloadOptions {
  const VesperSourcePreloadOptions({
    this.maximumBytes = 8 * 1024 * 1024,
    this.timeout = const Duration(seconds: 5),
  });

  final int maximumBytes;
  final Duration timeout;

  Map<String, Object?> toMap() {
    if (maximumBytes < 1 || maximumBytes > 16 * 1024 * 1024) {
      throw ArgumentError.value(
          maximumBytes, 'maximumBytes', 'Must be between 1 byte and 16 MiB.');
    }
    if (timeout.inMilliseconds < 1 || timeout.inMilliseconds > 60000) {
      throw ArgumentError.value(
          timeout, 'timeout', 'Must be between 1 ms and 60 s.');
    }
    return <String, Object?>{
      'maximumBytes': maximumBytes,
      'timeoutMs': timeout.inMilliseconds
    };
  }
}

/// A retained preload observation. Completion does not establish a playback hit.
final class VesperSourcePreloadSnapshot {
  const VesperSourcePreloadSnapshot({
    required this.taskId,
    required this.source,
    required this.rawStatus,
    required this.rawGoal,
    required this.rawReuse,
    this.actualBytes = 0,
    this.cacheHit,
    this.reasonCode,
  });

  factory VesperSourcePreloadSnapshot.fromMap(Map<Object?, Object?> map) =>
      VesperSourcePreloadSnapshot(
        taskId: _requiredString(map, 'taskId'),
        source: VesperSourceReference.fromMap(map),
        rawStatus: _requiredString(map, 'status'),
        rawGoal: _requiredString(map, 'goal'),
        rawReuse: _requiredString(map, 'reuse'),
        actualBytes: _optionalInt(map, 'actualBytes') ?? 0,
        cacheHit: map['cacheHit'] as bool?,
        reasonCode: map['reasonCode'] as String?,
      );

  final String taskId;
  final VesperSourceReference source;
  final String rawStatus;
  final String rawGoal;
  final String rawReuse;
  final int actualBytes;
  final bool? cacheHit;
  final String? reasonCode;

  VesperSourcePreloadStatus get status => _decodeEnum(
      VesperSourcePreloadStatus.values,
      rawStatus,
      VesperSourcePreloadStatus.unknown);
  VesperSourcePreloadGoal get goal => _decodeEnum(
      VesperSourcePreloadGoal.values, rawGoal, VesperSourcePreloadGoal.unknown);
  VesperSourcePreloadReuse get reuse => _decodeEnum(
      VesperSourcePreloadReuse.values,
      rawReuse,
      VesperSourcePreloadReuse.unknown);
  bool get isTerminal => switch (status) {
        VesperSourcePreloadStatus.completed ||
        VesperSourcePreloadStatus.failed ||
        VesperSourcePreloadStatus.unsupported ||
        VesperSourcePreloadStatus.cancelled =>
          true,
        _ => false,
      };
}

/// Explicit initial state applied by a source activation.
final class VesperSourceActivationOptions {
  const VesperSourceActivationOptions({
    this.playWhenReady = true,
    this.startPosition = Duration.zero,
    this.playbackRate = 1,
    this.timeout = const Duration(seconds: 30),
  });

  final bool playWhenReady;
  final Duration startPosition;
  final double playbackRate;
  final Duration timeout;

  Map<String, Object?> toMap() {
    if (startPosition.isNegative ||
        !playbackRate.isFinite ||
        playbackRate <= 0 ||
        timeout.inMilliseconds < 1 ||
        timeout.inMilliseconds > 60000) {
      throw ArgumentError('Invalid source activation options.');
    }
    return <String, Object?>{
      'playWhenReady': playWhenReady,
      'startPositionMs': startPosition.inMilliseconds,
      'playbackRate': playbackRate,
      'timeoutMs': timeout.inMilliseconds,
    };
  }
}

/// Correlates a completed source activation with native playback observations.
final class VesperSourceActivation {
  const VesperSourceActivation({
    required this.activationId,
    required this.source,
    required this.playbackEpoch,
  });

  factory VesperSourceActivation.fromMap(Map<Object?, Object?> map) =>
      VesperSourceActivation(
        activationId: _requiredString(map, 'activationId'),
        source: VesperSourceReference.fromMap(map),
        playbackEpoch: _requiredPositiveInt(map, 'playbackEpoch'),
      );

  final String activationId;
  final VesperSourceReference source;
  final int playbackEpoch;
}

T _decodeEnum<T extends Enum>(List<T> values, String raw, T unknown) {
  for (final value in values) {
    if (value.name == raw) return value;
  }
  return unknown;
}

String _requiredString(Map<Object?, Object?> map, String key) {
  final value = map[key];
  if (value is! String || value.isEmpty) {
    throw FormatException('Missing source lifecycle field: $key');
  }
  return value;
}

int? _optionalInt(Map<Object?, Object?> map, String key) {
  final value = map[key];
  if (value == null) return null;
  if (value is! int || value < 0) {
    throw FormatException('Invalid source lifecycle integer: $key');
  }
  return value;
}

int _requiredPositiveInt(Map<Object?, Object?> map, String key) {
  final value = _optionalInt(map, key);
  if (value == null || value == 0) {
    throw FormatException('Missing source lifecycle epoch: $key');
  }
  return value;
}
