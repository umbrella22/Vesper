part of '../download_models.dart';

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
    final normalized =
        _decodeRequiredDownloadMap(map, 'VesperDownloadResourceRecord');
    return VesperDownloadResourceRecord(
      resourceId:
          _decodeRequiredDownloadString(normalized['resourceId'], 'resourceId'),
      uri: _decodeRequiredDownloadString(normalized['uri'], 'uri'),
      relativePath: normalized['relativePath'] as String?,
      byteRange: normalized['byteRange'] == null
          ? null
          : VesperDownloadByteRange.fromMap(
              _decodeDownloadMap(normalized['byteRange'], 'byteRange'),
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
    final normalized =
        _decodeRequiredDownloadMap(map, 'VesperDownloadSegmentRecord');
    return VesperDownloadSegmentRecord(
      segmentId:
          _decodeRequiredDownloadString(normalized['segmentId'], 'segmentId'),
      uri: _decodeRequiredDownloadString(normalized['uri'], 'uri'),
      relativePath: normalized['relativePath'] as String?,
      sequence: _decodeInt(normalized['sequence']),
      byteRange: normalized['byteRange'] == null
          ? null
          : VesperDownloadByteRange.fromMap(
              _decodeDownloadMap(normalized['byteRange'], 'byteRange'),
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
  unknown,
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
    this.kindRawValue,
    this.language,
    this.codec,
    this.label,
    this.qualityRank,
    this.resourceIds = const <String>[],
    this.segmentIds = const <String>[],
    this.metadata = const <String, String>{},
  });

  factory VesperDownloadAssetStream.fromMap(Map<Object?, Object?> map) {
    final normalized =
        _decodeRequiredDownloadMap(map, 'VesperDownloadAssetStream');
    return VesperDownloadAssetStream(
      streamId:
          _decodeRequiredDownloadString(normalized['streamId'], 'streamId'),
      kind: _decodeStreamKind(normalized['kind']),
      kindRawValue: normalized['kind'] as String?,
      language: normalized['language'] as String?,
      codec: normalized['codec'] as String?,
      label: normalized['label'] as String?,
      qualityRank: _decodeInt(normalized['qualityRank']),
      resourceIds: _decodeStringList(normalized['resourceIds'], 'resourceIds'),
      segmentIds: _decodeStringList(normalized['segmentIds'], 'segmentIds'),
      metadata: _decodeStringMap(normalized['metadata']),
    );
  }

  final String streamId;
  final VesperDownloadStreamKind kind;
  final String? kindRawValue;
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
      'kind': kindRawValue ?? kind.name,
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
    this.contentFormatRawValue,
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
    final normalized =
        _decodeRequiredDownloadMap(map, 'VesperDownloadAssetIndex');
    final rawResources = normalized['resources'];
    final rawSegments = normalized['segments'];
    final rawStreams = normalized['streams'];
    final rawContentFormat = normalized['contentFormat'];
    final contentFormat = _decodeContentFormat(rawContentFormat);
    return VesperDownloadAssetIndex(
      contentFormat: contentFormat,
      contentFormatRawValue: _unknownEnumRawValue(
        rawContentFormat,
        isUnknown: contentFormat == VesperDownloadContentFormat.unknown,
      ),
      version: normalized['version'] as String?,
      etag: normalized['etag'] as String?,
      checksum: normalized['checksum'] as String?,
      totalSizeBytes: _decodeInt(normalized['totalSizeBytes']),
      resources: _decodeDownloadRecordList(
        rawResources,
        'resources',
        VesperDownloadResourceRecord.fromMap,
      ),
      segments: _decodeDownloadRecordList(
        rawSegments,
        'segments',
        VesperDownloadSegmentRecord.fromMap,
      ),
      streams: _decodeDownloadRecordList(
        rawStreams,
        'streams',
        VesperDownloadAssetStream.fromMap,
      ),
      completedPath: normalized['completedPath'] as String?,
    );
  }

  final VesperDownloadContentFormat contentFormat;
  final String? contentFormatRawValue;
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
      'contentFormat': contentFormatRawValue ?? contentFormat.name,
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
