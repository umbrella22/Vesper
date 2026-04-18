import 'models.dart';

enum VesperDownloadContentFormat {
  hlsSegments,
  dashSegments,
  singleFile,
  unknown,
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

final class VesperDownloadConfiguration {
  const VesperDownloadConfiguration({
    this.autoStart = true,
    this.runPostProcessorsOnCompletion = true,
    this.baseDirectory,
    this.pluginLibraryPaths = const <String>[],
  });

  factory VesperDownloadConfiguration.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawPluginLibraryPaths = normalized['pluginLibraryPaths'];
    return VesperDownloadConfiguration(
      autoStart: normalized['autoStart'] as bool? ?? true,
      runPostProcessorsOnCompletion:
          normalized['runPostProcessorsOnCompletion'] as bool? ?? true,
      baseDirectory: normalized['baseDirectory'] as String?,
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
  final String? baseDirectory;
  final List<String> pluginLibraryPaths;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'autoStart': autoStart,
      'runPostProcessorsOnCompletion': runPostProcessorsOnCompletion,
      'baseDirectory': baseDirectory,
      'pluginLibraryPaths': pluginLibraryPaths,
    };
  }
}

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
      targetDirectory: normalized['targetDirectory'] as String?,
      allowMeteredNetwork: normalized['allowMeteredNetwork'] as bool? ?? false,
    );
  }

  final String? variantId;
  final String? preferredAudioLanguage;
  final String? preferredSubtitleLanguage;
  final List<String> selectedTrackIds;
  final String? targetDirectory;
  final bool allowMeteredNetwork;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'variantId': variantId,
      'preferredAudioLanguage': preferredAudioLanguage,
      'preferredSubtitleLanguage': preferredSubtitleLanguage,
      'selectedTrackIds': selectedTrackIds,
      'targetDirectory': targetDirectory,
      'allowMeteredNetwork': allowMeteredNetwork,
    };
  }
}

final class VesperDownloadResourceRecord {
  const VesperDownloadResourceRecord({
    required this.resourceId,
    required this.uri,
    this.relativePath,
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
      sizeBytes: _decodeInt(normalized['sizeBytes']),
      etag: normalized['etag'] as String?,
      checksum: normalized['checksum'] as String?,
    );
  }

  final String resourceId;
  final String uri;
  final String? relativePath;
  final int? sizeBytes;
  final String? etag;
  final String? checksum;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'resourceId': resourceId,
      'uri': uri,
      'relativePath': relativePath,
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
      sizeBytes: _decodeInt(normalized['sizeBytes']),
      checksum: normalized['checksum'] as String?,
    );
  }

  final String segmentId;
  final String uri;
  final String? relativePath;
  final int? sequence;
  final int? sizeBytes;
  final String? checksum;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'segmentId': segmentId,
      'uri': uri,
      'relativePath': relativePath,
      'sequence': sequence,
      'sizeBytes': sizeBytes,
      'checksum': checksum,
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
    this.completedPath,
  });

  factory VesperDownloadAssetIndex.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawResources = normalized['resources'];
    final rawSegments = normalized['segments'];
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
      'completedPath': completedPath,
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
    required this.codeOrdinal,
    required this.categoryOrdinal,
    required this.retriable,
    required this.message,
  });

  factory VesperDownloadError.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperDownloadError(
      codeOrdinal: _decodeInt(normalized['codeOrdinal']) ?? 0,
      categoryOrdinal: _decodeInt(normalized['categoryOrdinal']) ?? 0,
      retriable: normalized['retriable'] as bool? ?? false,
      message: normalized['message'] as String? ?? 'Unknown download error.',
    );
  }

  final int codeOrdinal;
  final int categoryOrdinal;
  final bool retriable;
  final String message;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'codeOrdinal': codeOrdinal,
      'categoryOrdinal': categoryOrdinal,
      'retriable': retriable,
      'message': message,
    };
  }
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

int? _decodeInt(Object? raw) {
  return switch (raw) {
    final int value => value,
    _ => null,
  };
}
