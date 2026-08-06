import 'dart:async';

import 'models.dart';

part 'download/asset_models.dart';
part 'download/task_models.dart';
part 'download/download_decode_helpers.dart';

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
  unknown,
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

List<VesperPluginReference> _decodeDownloadPluginReferences(Object? value) {
  if (value == null) {
    return const <VesperPluginReference>[];
  }
  if (value is! List) {
    throw const FormatException('plugin references must be a list');
  }
  return List<VesperPluginReference>.unmodifiable(
    value.map((entry) {
      if (entry is! Map) {
        throw const FormatException('invalid plugin reference');
      }
      return VesperPluginReference.fromMap(Map<Object?, Object?>.from(entry));
    }),
  );
}

final class VesperDownloadConfiguration {
  const VesperDownloadConfiguration({
    this.autoStart = true,
    this.runPostProcessorsOnCompletion = true,
    this.resumePartialDownloads = true,
    this.restoreTasksOnStartup = true,
    this.baseDirectory,
    this.postDownloadPluginReferences = const <VesperPluginReference>[],
    this.eventHookPluginReferences = const <VesperPluginReference>[],
    this.rangeChunkBytes,
    this.minProgressBytes = 512 * 1024,
    this.minProgressIntervalMs = 250,
  });

  factory VesperDownloadConfiguration.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
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
      postDownloadPluginReferences: _decodeDownloadPluginReferences(
        normalized['postDownloadPluginReferences'],
      ),
      eventHookPluginReferences: _decodeDownloadPluginReferences(
        normalized['eventHookPluginReferences'],
      ),
    );
  }

  final bool autoStart;
  final bool runPostProcessorsOnCompletion;
  final bool resumePartialDownloads;
  final bool restoreTasksOnStartup;
  final String? baseDirectory;
  final List<VesperPluginReference> postDownloadPluginReferences;
  final List<VesperPluginReference> eventHookPluginReferences;
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
      'postDownloadPluginReferences': postDownloadPluginReferences
          .map((reference) => reference.toMap())
          .toList(growable: false),
      'eventHookPluginReferences': eventHookPluginReferences
          .map((reference) => reference.toMap())
          .toList(growable: false),
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
    this.contentFormatRawValue,
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
    final rawContentFormat = normalized['contentFormat'];
    final contentFormat = _decodeContentFormat(rawContentFormat);
    return VesperDownloadSource(
      source: VesperPlayerSource.fromMap(vesperDecodeMap(normalized['source'])),
      contentFormat: contentFormat,
      contentFormatRawValue: _unknownEnumRawValue(
        rawContentFormat,
        isUnknown: contentFormat == VesperDownloadContentFormat.unknown,
      ),
      manifestUri: normalized['manifestUri'] as String?,
    );
  }

  final VesperPlayerSource source;
  final VesperDownloadContentFormat contentFormat;
  final String? contentFormatRawValue;
  final String? manifestUri;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'source': source.toMap(),
      'contentFormat': contentFormatRawValue ?? contentFormat.name,
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
      VesperPlayerSourceProtocol.rtmp ||
      VesperPlayerSourceProtocol.rtsp ||
      VesperPlayerSourceProtocol.flv ||
      VesperPlayerSourceProtocol.unknown =>
        VesperDownloadContentFormat.unknown,
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
    this.targetOutputFormatRawValue,
    this.targetDirectory,
    this.allowMeteredNetwork = false,
  });

  factory VesperDownloadProfile.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    final rawSelectedTrackIds = normalized['selectedTrackIds'];
    final rawTargetOutputFormat = normalized['targetOutputFormat'];
    final targetOutputFormat = _decodeOutputFormat(rawTargetOutputFormat);
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
      targetOutputFormat: targetOutputFormat,
      targetOutputFormatRawValue: _unknownEnumRawValue(
        rawTargetOutputFormat,
        isUnknown: targetOutputFormat == null,
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
  final String? targetOutputFormatRawValue;
  final String? targetDirectory;
  final bool allowMeteredNetwork;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'variantId': variantId,
      'preferredAudioLanguage': preferredAudioLanguage,
      'preferredSubtitleLanguage': preferredSubtitleLanguage,
      'selectedTrackIds': selectedTrackIds,
      'targetOutputFormat':
          targetOutputFormatRawValue ?? targetOutputFormat?.name,
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
