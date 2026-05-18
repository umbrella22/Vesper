import 'dart:async';

import 'models.dart';

const Object _vesperDownloadUnset = Object();

enum VesperDownloadContentFormat {
  hlsSegments,
  dashSegments,
  flvSegments,
  singleFile,
  unknown,
}

enum VesperDownloadOutputFormat {
  mp4,
  mkv,
  original,
}

enum VesperDownloadState {
  queued,
  preparing,
  downloading,
  paused,
  completed,
  failed,
  removed,
}

enum VesperDownloadStaleResourcePhase {
  prepare,
  download,
}

enum VesperDownloadPublicCollection {
  downloads,
  movies,
}

final class VesperDownloadConfiguration {
  const VesperDownloadConfiguration({
    this.autoStart = true,
    this.runPostProcessorsOnCompletion = true,
    this.resumePartialDownloads = true,
    this.restoreTasksOnStartup = true,
    this.baseDirectory,
    this.pluginLibraryPaths = const <String>[],
    this.rangeChunkBytes,
    this.minProgressBytes = 512 * 1024,
    this.minProgressIntervalMs = 250,
  });

  factory VesperDownloadConfiguration.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawPluginLibraryPaths = normalized['pluginLibraryPaths'];
    return VesperDownloadConfiguration(
      autoStart: normalized['autoStart'] as bool? ?? true,
      runPostProcessorsOnCompletion:
          normalized['runPostProcessorsOnCompletion'] as bool? ?? true,
      resumePartialDownloads:
          normalized['resumePartialDownloads'] as bool? ?? true,
      restoreTasksOnStartup:
          normalized['restoreTasksOnStartup'] as bool? ?? true,
      baseDirectory: normalized['baseDirectory'] as String?,
      rangeChunkBytes: _decodeInt(normalized['rangeChunkBytes']),
      minProgressBytes:
          _decodeInt(normalized['minProgressBytes']) ?? 512 * 1024,
      minProgressIntervalMs:
          _decodeInt(normalized['minProgressIntervalMs']) ?? 250,
      pluginLibraryPaths: switch (rawPluginLibraryPaths) {
        final List<dynamic> values => values
            .map((value) => value?.toString() ?? '')
            .where((value) => value.isNotEmpty)
            .toList(growable: false),
        _ => const <String>[],
      },
    );
  }

  final bool autoStart;
  final bool runPostProcessorsOnCompletion;
  final bool resumePartialDownloads;
  final bool restoreTasksOnStartup;
  final String? baseDirectory;
  final List<String> pluginLibraryPaths;
  final int? rangeChunkBytes;
  final int minProgressBytes;
  final int minProgressIntervalMs;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'autoStart': autoStart,
      'runPostProcessorsOnCompletion': runPostProcessorsOnCompletion,
      'resumePartialDownloads': resumePartialDownloads,
      'restoreTasksOnStartup': restoreTasksOnStartup,
      'baseDirectory': baseDirectory,
      'pluginLibraryPaths': pluginLibraryPaths,
      'rangeChunkBytes': rangeChunkBytes,
      'minProgressBytes': minProgressBytes,
      'minProgressIntervalMs': minProgressIntervalMs,
    };
  }
}

typedef VesperDownloadStaleResourcePlanRecoveryCallback
    = FutureOr<VesperDownloadRecoveredTaskPlan?> Function(
  VesperDownloadTaskSnapshot task,
  VesperDownloadStaleResource staleResource,
);

final class VesperDownloadSource {
  const VesperDownloadSource({
    required this.source,
    required this.contentFormat,
    this.manifestUri,
  });

  factory VesperDownloadSource.fromSource({
    required VesperPlayerSource source,
    VesperDownloadContentFormat? contentFormat,
    String? manifestUri,
  }) {
    return VesperDownloadSource(
      source: source,
      contentFormat: contentFormat ?? _inferContentFormat(source.protocol),
      manifestUri: manifestUri,
    );
  }

  factory VesperDownloadSource.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperDownloadSource(
      source: VesperPlayerSource.fromMap(vesperDecodeMap(normalized['source'])),
      contentFormat: _decodeContentFormat(normalized['contentFormat']),
      manifestUri: normalized['manifestUri'] as String?,
    );
  }

  final VesperPlayerSource source;
  final VesperDownloadContentFormat contentFormat;
  final String? manifestUri;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'source': source.toMap(),
      'contentFormat': contentFormat.name,
      'manifestUri': manifestUri,
    };
  }

  static VesperDownloadContentFormat _inferContentFormat(
    VesperPlayerSourceProtocol protocol,
  ) {
    return switch (protocol) {
      VesperPlayerSourceProtocol.hls => VesperDownloadContentFormat.hlsSegments,
      VesperPlayerSourceProtocol.dash =>
        VesperDownloadContentFormat.dashSegments,
      VesperPlayerSourceProtocol.file ||
      VesperPlayerSourceProtocol.content ||
      VesperPlayerSourceProtocol.progressive =>
        VesperDownloadContentFormat.singleFile,
      VesperPlayerSourceProtocol.unknown => VesperDownloadContentFormat.unknown,
    };
  }
}

final class VesperDownloadProfile {
  const VesperDownloadProfile({
    this.variantId,
    this.preferredAudioLanguage,
    this.preferredSubtitleLanguage,
    this.selectedTrackIds = const <String>[],
    this.targetOutputFormat,
    this.targetDirectory,
    this.allowMeteredNetwork = false,
  });

  factory VesperDownloadProfile.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawSelectedTrackIds = normalized['selectedTrackIds'];
    return VesperDownloadProfile(
      variantId: normalized['variantId'] as String?,
      preferredAudioLanguage: normalized['preferredAudioLanguage'] as String?,
      preferredSubtitleLanguage:
          normalized['preferredSubtitleLanguage'] as String?,
      selectedTrackIds: switch (rawSelectedTrackIds) {
        final List<dynamic> values => values
            .map((value) => value?.toString() ?? '')
            .where((value) => value.isNotEmpty)
            .toList(growable: false),
        _ => const <String>[],
      },
      targetOutputFormat: _decodeOutputFormat(
        normalized['targetOutputFormat'],
      ),
      targetDirectory: normalized['targetDirectory'] as String?,
      allowMeteredNetwork: normalized['allowMeteredNetwork'] as bool? ?? false,
    );
  }

  final String? variantId;
  final String? preferredAudioLanguage;
  final String? preferredSubtitleLanguage;
  final List<String> selectedTrackIds;
  final VesperDownloadOutputFormat? targetOutputFormat;
  final String? targetDirectory;
  final bool allowMeteredNetwork;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'variantId': variantId,
      'preferredAudioLanguage': preferredAudioLanguage,
      'preferredSubtitleLanguage': preferredSubtitleLanguage,
      'selectedTrackIds': selectedTrackIds,
      'targetOutputFormat': targetOutputFormat?.name,
      'targetDirectory': targetDirectory,
      'allowMeteredNetwork': allowMeteredNetwork,
    };
  }
}

final class VesperDownloadByteRange {
  const VesperDownloadByteRange({
    required this.offset,
    required this.length,
  });

  factory VesperDownloadByteRange.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperDownloadByteRange(
      offset: _decodeInt(normalized['offset']) ?? 0,
      length: _decodeInt(normalized['length']) ?? 0,
    );
  }

  final int offset;
  final int length;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'offset': offset,
      'length': length,
    };
  }
}

final class VesperDownloadResourceRecord {
  const VesperDownloadResourceRecord({
    required this.resourceId,
    required this.uri,
    this.relativePath,
    this.byteRange,
    this.generatedText,
    this.sizeBytes,
    this.etag,
    this.checksum,
  });

  factory VesperDownloadResourceRecord.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperDownloadResourceRecord(
      resourceId: normalized['resourceId'] as String? ?? '',
      uri: normalized['uri'] as String? ?? '',
      relativePath: normalized['relativePath'] as String?,
      byteRange: normalized['byteRange'] == null
          ? null
          : VesperDownloadByteRange.fromMap(
              vesperDecodeMap(normalized['byteRange']),
            ),
      generatedText: normalized['generatedText'] as String?,
      sizeBytes: _decodeInt(normalized['sizeBytes']),
      etag: normalized['etag'] as String?,
      checksum: normalized['checksum'] as String?,
    );
  }

  final String resourceId;
  final String uri;
  final String? relativePath;
  final VesperDownloadByteRange? byteRange;
  final String? generatedText;
  final int? sizeBytes;
  final String? etag;
  final String? checksum;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'resourceId': resourceId,
      'uri': uri,
      'relativePath': relativePath,
      'byteRange': byteRange?.toMap(),
      'generatedText': generatedText,
      'sizeBytes': sizeBytes,
      'etag': etag,
      'checksum': checksum,
    };
  }
}

final class VesperDownloadSegmentRecord {
  const VesperDownloadSegmentRecord({
    required this.segmentId,
    required this.uri,
    this.relativePath,
    this.sequence,
    this.byteRange,
    this.sizeBytes,
    this.checksum,
  });

  factory VesperDownloadSegmentRecord.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperDownloadSegmentRecord(
      segmentId: normalized['segmentId'] as String? ?? '',
      uri: normalized['uri'] as String? ?? '',
      relativePath: normalized['relativePath'] as String?,
      sequence: _decodeInt(normalized['sequence']),
      byteRange: normalized['byteRange'] == null
          ? null
          : VesperDownloadByteRange.fromMap(
              vesperDecodeMap(normalized['byteRange']),
            ),
      sizeBytes: _decodeInt(normalized['sizeBytes']),
      checksum: normalized['checksum'] as String?,
    );
  }

  final String segmentId;
  final String uri;
  final String? relativePath;
  final int? sequence;
  final VesperDownloadByteRange? byteRange;
  final int? sizeBytes;
  final String? checksum;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'segmentId': segmentId,
      'uri': uri,
      'relativePath': relativePath,
      'sequence': sequence,
      'byteRange': byteRange?.toMap(),
      'sizeBytes': sizeBytes,
      'checksum': checksum,
    };
  }
}

enum VesperDownloadStreamKind {
  combined,
  video,
  audio,
  secondaryAudio,
  subtitle,
  auxiliary,
}

final class VesperDownloadAssetStream {
  const VesperDownloadAssetStream({
    required this.streamId,
    this.kind = VesperDownloadStreamKind.combined,
    this.language,
    this.codec,
    this.label,
    this.qualityRank,
    this.resourceIds = const <String>[],
    this.segmentIds = const <String>[],
    this.metadata = const <String, String>{},
  });

  factory VesperDownloadAssetStream.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperDownloadAssetStream(
      streamId: normalized['streamId'] as String? ?? '',
      kind: _decodeStreamKind(normalized['kind']),
      language: normalized['language'] as String?,
      codec: normalized['codec'] as String?,
      label: normalized['label'] as String?,
      qualityRank: _decodeInt(normalized['qualityRank']),
      resourceIds: _decodeStringList(normalized['resourceIds']),
      segmentIds: _decodeStringList(normalized['segmentIds']),
      metadata: _decodeStringMap(normalized['metadata']),
    );
  }

  final String streamId;
  final VesperDownloadStreamKind kind;
  final String? language;
  final String? codec;
  final String? label;
  final int? qualityRank;
  final List<String> resourceIds;
  final List<String> segmentIds;
  final Map<String, String> metadata;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'streamId': streamId,
      'kind': kind.name,
      'language': language,
      'codec': codec,
      'label': label,
      'qualityRank': qualityRank,
      'resourceIds': resourceIds,
      'segmentIds': segmentIds,
      'metadata': metadata,
    };
  }
}

final class VesperDownloadAssetIndex {
  const VesperDownloadAssetIndex({
    this.contentFormat = VesperDownloadContentFormat.unknown,
    this.version,
    this.etag,
    this.checksum,
    this.totalSizeBytes,
    this.resources = const <VesperDownloadResourceRecord>[],
    this.segments = const <VesperDownloadSegmentRecord>[],
    this.streams = const <VesperDownloadAssetStream>[],
    this.completedPath,
  });

  factory VesperDownloadAssetIndex.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawResources = normalized['resources'];
    final rawSegments = normalized['segments'];
    final rawStreams = normalized['streams'];
    return VesperDownloadAssetIndex(
      contentFormat: _decodeContentFormat(normalized['contentFormat']),
      version: normalized['version'] as String?,
      etag: normalized['etag'] as String?,
      checksum: normalized['checksum'] as String?,
      totalSizeBytes: _decodeInt(normalized['totalSizeBytes']),
      resources: switch (rawResources) {
        final List<dynamic> values => values
            .whereType<Map>()
            .map(
              (value) => VesperDownloadResourceRecord.fromMap(
                Map<Object?, Object?>.from(value),
              ),
            )
            .toList(growable: false),
        _ => const <VesperDownloadResourceRecord>[],
      },
      segments: switch (rawSegments) {
        final List<dynamic> values => values
            .whereType<Map>()
            .map(
              (value) => VesperDownloadSegmentRecord.fromMap(
                Map<Object?, Object?>.from(value),
              ),
            )
            .toList(growable: false),
        _ => const <VesperDownloadSegmentRecord>[],
      },
      streams: switch (rawStreams) {
        final List<dynamic> values => values
            .whereType<Map>()
            .map(
              (value) => VesperDownloadAssetStream.fromMap(
                Map<Object?, Object?>.from(value),
              ),
            )
            .toList(growable: false),
        _ => const <VesperDownloadAssetStream>[],
      },
      completedPath: normalized['completedPath'] as String?,
    );
  }

  final VesperDownloadContentFormat contentFormat;
  final String? version;
  final String? etag;
  final String? checksum;
  final int? totalSizeBytes;
  final List<VesperDownloadResourceRecord> resources;
  final List<VesperDownloadSegmentRecord> segments;
  final List<VesperDownloadAssetStream> streams;
  final String? completedPath;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'contentFormat': contentFormat.name,
      'version': version,
      'etag': etag,
      'checksum': checksum,
      'totalSizeBytes': totalSizeBytes,
      'resources': resources.map((value) => value.toMap()).toList(),
      'segments': segments.map((value) => value.toMap()).toList(),
      'streams': streams.map((value) => value.toMap()).toList(),
      'completedPath': completedPath,
    };
  }
}

final class VesperDownloadStaleResource {
  const VesperDownloadStaleResource({
    required this.taskId,
    this.resourceId,
    this.segmentId,
    this.uri,
    this.phase = VesperDownloadStaleResourcePhase.prepare,
    this.statusCode,
    this.receivedBytes = 0,
    required this.message,
  });

  factory VesperDownloadStaleResource.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperDownloadStaleResource(
      taskId: _decodeInt(normalized['taskId']) ?? 0,
      resourceId: normalized['resourceId'] as String?,
      segmentId: normalized['segmentId'] as String?,
      uri: normalized['uri'] as String?,
      phase: _decodeStaleResourcePhase(normalized['phase']),
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
  final int? statusCode;
  final int receivedBytes;
  final String message;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'taskId': taskId,
      'resourceId': resourceId,
      'segmentId': segmentId,
      'uri': uri,
      'phase': phase.name,
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
  });

  factory VesperDownloadError.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperDownloadError(
      code: _decodeRequiredDownloadEnum(
        VesperPlayerErrorCode.values,
        normalized['code'],
        'code',
      ),
      category: _decodeRequiredDownloadEnum(
        VesperPlayerErrorCategory.values,
        normalized['category'],
        'category',
      ),
      retriable: normalized['retriable'] as bool? ?? false,
      message: normalized['message'] as String? ?? 'Unknown download error.',
    );
  }

  final VesperPlayerErrorCode code;
  final VesperPlayerErrorCategory category;
  final bool retriable;
  final String message;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'code': code.name,
      'category': category.name,
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
  final VesperDownloadProgressSnapshot progress;
  final VesperDownloadAssetIndex assetIndex;
  final VesperDownloadError? error;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'taskId': taskId,
      'assetId': assetId,
      'source': source.toMap(),
      'profile': profile.toMap(),
      'state': state.name,
      'progress': progress.toMap(),
      'assetIndex': assetIndex.toMap(),
      'error': error?.toMap(),
    };
  }

  VesperDownloadTaskSnapshot copyWith({
    VesperDownloadState? state,
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
    this.error,
    this.completedPath,
  });

  factory VesperDownloadTaskStatePatch.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawError = normalized['error'];
    return VesperDownloadTaskStatePatch(
      taskId: _decodeInt(normalized['taskId']) ?? 0,
      state: _decodeDownloadState(normalized['state']),
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
  final VesperDownloadProgressSnapshot progress;
  final VesperDownloadError? error;
  final String? completedPath;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'taskId': taskId,
      'state': state.name,
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

VesperDownloadContentFormat _decodeContentFormat(Object? raw) {
  if (raw is String) {
    for (final value in VesperDownloadContentFormat.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperDownloadContentFormat.unknown;
}

VesperDownloadOutputFormat? _decodeOutputFormat(Object? raw) {
  if (raw is String) {
    for (final value in VesperDownloadOutputFormat.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return null;
}

VesperDownloadStreamKind _decodeStreamKind(Object? raw) {
  if (raw is String) {
    for (final value in VesperDownloadStreamKind.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperDownloadStreamKind.combined;
}

VesperDownloadState _decodeDownloadState(Object? raw) {
  if (raw is String) {
    for (final value in VesperDownloadState.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperDownloadState.queued;
}

VesperDownloadStaleResourcePhase _decodeStaleResourcePhase(Object? raw) {
  if (raw is String) {
    for (final value in VesperDownloadStaleResourcePhase.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperDownloadStaleResourcePhase.prepare;
}

int? _decodeInt(Object? raw) {
  return switch (raw) {
    final int value => value,
    _ => null,
  };
}

List<String> _decodeStringList(Object? raw) {
  return switch (raw) {
    final List<dynamic> values =>
      values.whereType<String>().toList(growable: false),
    _ => const <String>[],
  };
}

Map<String, String> _decodeStringMap(Object? raw) {
  if (raw == null) {
    return const <String, String>{};
  }
  final normalized = vesperDecodeMap(raw);
  return normalized.map(
    (key, value) => MapEntry(key.toString(), value?.toString() ?? ''),
  );
}
