import 'dart:convert';
import 'dart:io';
import 'dart:math' as math;

import 'package:vesper_player/vesper_player.dart';

final class ExamplePreparedDownloadTask {
  const ExamplePreparedDownloadTask({
    required this.source,
    required this.profile,
    required this.assetIndex,
  });

  final VesperDownloadSource source;
  final VesperDownloadProfile profile;
  final VesperDownloadAssetIndex assetIndex;
}

String exampleDraftDownloadLabelFromSource(VesperPlayerSource source) {
  final normalizedLabel = source.label.trim();
  if (normalizedLabel.isNotEmpty) {
    return normalizedLabel;
  }
  return exampleDraftDownloadLabelFromUri(source.uri);
}

String exampleDraftDownloadLabelFromUri(String uri) {
  final parsedUri = Uri.tryParse(uri);
  final segments = parsedUri?.pathSegments.where((value) => value.isNotEmpty);
  final fileName = segments?.isEmpty ?? true ? null : segments!.last;
  final parentDirectory = segments == null || segments.length < 2
      ? null
      : segments.elementAt(segments.length - 2);
  final lowercasedFileName = fileName?.toLowerCase();
  final rawCandidate =
      switch ((fileName, parentDirectory, lowercasedFileName)) {
        (null, _, _) => parsedUri?.host,
        (_, final parent?, final normalized?)
            when _genericManifestFileNames.contains(normalized) =>
          parent,
        (final name?, _, _) when name.contains('.') => name.substring(
          0,
          name.lastIndexOf('.'),
        ),
        (final name?, _, _) => name,
      } ??
      parsedUri?.host ??
      uri;
  final cleaned = rawCandidate.replaceAll('_', ' ').replaceAll('-', ' ').trim();
  return cleaned.isEmpty ? uri : cleaned;
}

Future<ExamplePreparedDownloadTask> prepareExampleDownloadTask({
  required String assetId,
  required VesperPlayerSource source,
}) async {
  switch (source.protocol) {
    case VesperPlayerSourceProtocol.hls:
      return _prepareHlsDownloadTask(assetId: assetId, source: source);
    case VesperPlayerSourceProtocol.dash:
      return _prepareDashDownloadTask(assetId: assetId, source: source);
    case VesperPlayerSourceProtocol.progressive:
    case VesperPlayerSourceProtocol.file:
    case VesperPlayerSourceProtocol.content:
    case VesperPlayerSourceProtocol.unknown:
      return ExamplePreparedDownloadTask(
        source: VesperDownloadSource.fromSource(source: source),
        profile: const VesperDownloadProfile(),
        assetIndex: const VesperDownloadAssetIndex(),
      );
  }
}

Future<ExamplePreparedDownloadTask> _prepareHlsDownloadTask({
  required String assetId,
  required VesperPlayerSource source,
}) async {
  final manifestUri = Uri.parse(source.uri);
  final manifestText = await _fetchRemoteText(manifestUri);
  final targetDirectory = await _exampleDownloadTargetDirectory(assetId);
  final resourceRecords = <String, VesperDownloadResourceRecord>{};
  final segmentRecords = <String, VesperDownloadSegmentRecord>{};

  void addResource(Uri uri) {
    final relativePath = _relativePathForRemoteUri(uri);
    resourceRecords.putIfAbsent(
      relativePath,
      () => VesperDownloadResourceRecord(
        resourceId: relativePath,
        uri: uri.toString(),
        relativePath: relativePath,
      ),
    );
  }

  void addSegment(Uri uri, int? sequence) {
    final relativePath = _relativePathForRemoteUri(uri);
    segmentRecords.putIfAbsent(
      relativePath,
      () => VesperDownloadSegmentRecord(
        segmentId: relativePath,
        uri: uri.toString(),
        relativePath: relativePath,
        sequence: sequence,
      ),
    );
  }

  void addPlaylistEntry(_HlsPlaylistEntry entry) {
    switch (entry.kind) {
      case _HlsPlaylistEntryKind.resource:
        addResource(entry.uri);
      case _HlsPlaylistEntryKind.segment:
        addSegment(entry.uri, entry.sequence);
    }
  }

  addResource(manifestUri);

  String? primaryPlaylistText;
  final parsedMaster = _parseHlsMasterManifest(manifestText, manifestUri);
  if (parsedMaster != null) {
    addResource(parsedMaster.variantPlaylistUri);
    if (parsedMaster.audioPlaylistUri case final audioPlaylistUri?) {
      addResource(audioPlaylistUri);
    }

    final videoPlaylistText = await _fetchRemoteText(
      parsedMaster.variantPlaylistUri,
    );
    primaryPlaylistText = videoPlaylistText;
    for (final entry in _parseHlsMediaPlaylist(
      videoPlaylistText,
      parsedMaster.variantPlaylistUri,
    )) {
      addPlaylistEntry(entry);
    }

    if (parsedMaster.audioPlaylistUri case final audioPlaylistUri?) {
      final audioPlaylistText = await _fetchRemoteText(audioPlaylistUri);
      for (final entry in _parseHlsMediaPlaylist(
        audioPlaylistText,
        audioPlaylistUri,
      )) {
        addPlaylistEntry(entry);
      }
    }
  } else {
    primaryPlaylistText = manifestText;
    for (final entry in _parseHlsMediaPlaylist(manifestText, manifestUri)) {
      addPlaylistEntry(entry);
    }
  }

  final preparedLabel = _resolvePreparedHlsLabel(
    originalSource: source,
    manifestUri: manifestUri,
    manifestText: manifestText,
    primaryPlaylistText: primaryPlaylistText,
  );

  return ExamplePreparedDownloadTask(
    source: VesperDownloadSource.fromSource(
      source: VesperPlayerSource.remote(
        uri: manifestUri.toString(),
        label: preparedLabel,
        protocol: VesperPlayerSourceProtocol.hls,
      ),
      contentFormat: VesperDownloadContentFormat.hlsSegments,
      manifestUri: manifestUri.toString(),
    ),
    profile: VesperDownloadProfile(targetDirectory: targetDirectory.path),
    assetIndex: VesperDownloadAssetIndex(
      contentFormat: VesperDownloadContentFormat.hlsSegments,
      resources: resourceRecords.values.toList(growable: false),
      segments: segmentRecords.values.toList(growable: false),
    ),
  );
}

Future<ExamplePreparedDownloadTask> _prepareDashDownloadTask({
  required String assetId,
  required VesperPlayerSource source,
}) async {
  final manifestUri = Uri.parse(source.uri);
  final manifestText = await _fetchRemoteText(manifestUri);
  final targetDirectory = await _exampleDownloadTargetDirectory(assetId);
  final resourceRecords = <String, VesperDownloadResourceRecord>{};
  final segmentRecords = <String, VesperDownloadSegmentRecord>{};

  void addResource(Uri uri) {
    final relativePath = _relativePathForRemoteUri(uri);
    resourceRecords.putIfAbsent(
      relativePath,
      () => VesperDownloadResourceRecord(
        resourceId: relativePath,
        uri: uri.toString(),
        relativePath: relativePath,
      ),
    );
  }

  void addSegment(Uri uri, int sequence) {
    final relativePath = _relativePathForRemoteUri(uri);
    segmentRecords.putIfAbsent(
      relativePath,
      () => VesperDownloadSegmentRecord(
        segmentId: relativePath,
        uri: uri.toString(),
        relativePath: relativePath,
        sequence: sequence,
      ),
    );
  }

  addResource(manifestUri);

  final presentationDurationSeconds = _parseDashPresentationDuration(
    manifestText,
  );
  final adaptationSets = _dashAdaptationSetPattern.allMatches(manifestText);
  var nextSequence = 0;
  for (final adaptationMatch in adaptationSets) {
    final adaptationAttributes = _parseXmlAttributes(
      adaptationMatch.group(1) ?? '',
    );
    final adaptationBody = adaptationMatch.group(2) ?? '';
    final mimeType = adaptationAttributes['mimeType'] ?? '';
    if (!mimeType.startsWith('video/') && !mimeType.startsWith('audio/')) {
      continue;
    }

    final representationMatch = _dashRepresentationPattern.firstMatch(
      adaptationBody,
    );
    if (representationMatch == null) {
      continue;
    }
    final representationAttributes = _parseXmlAttributes(
      representationMatch.group(1) ?? representationMatch.group(3) ?? '',
    );
    final representationId = representationAttributes['id'];
    if (representationId == null || representationId.isEmpty) {
      continue;
    }

    final templateMatch =
        _dashSegmentTemplatePattern.firstMatch(
          representationMatch.group(2) ?? '',
        ) ??
        _dashSegmentTemplatePattern.firstMatch(adaptationBody);
    if (templateMatch == null) {
      continue;
    }
    final templateAttributes = _parseXmlAttributes(
      templateMatch.group(1) ?? '',
    );
    final initializationTemplate = templateAttributes['initialization'];
    final mediaTemplate = templateAttributes['media'];
    final startNumber =
        int.tryParse(templateAttributes['startNumber'] ?? '') ?? 1;
    final timescale = int.tryParse(templateAttributes['timescale'] ?? '') ?? 1;
    final duration = int.tryParse(templateAttributes['duration'] ?? '');
    if (initializationTemplate == null ||
        mediaTemplate == null ||
        duration == null ||
        duration <= 0) {
      continue;
    }

    final segmentCount = presentationDurationSeconds == null
        ? 1
        : math.max(
            1,
            (presentationDurationSeconds * timescale / duration).ceil(),
          );

    addResource(
      manifestUri.resolve(
        initializationTemplate.replaceAll(
          r'$RepresentationID$',
          representationId,
        ),
      ),
    );

    for (var index = 0; index < segmentCount; index += 1) {
      final segmentNumber = startNumber + index;
      addSegment(
        manifestUri.resolve(
          mediaTemplate
              .replaceAll(r'$RepresentationID$', representationId)
              .replaceAll(r'$Number$', '$segmentNumber'),
        ),
        nextSequence,
      );
      nextSequence += 1;
    }
  }

  final preparedLabel = _resolvePreparedDashLabel(
    originalSource: source,
    manifestUri: manifestUri,
    manifestText: manifestText,
  );

  return ExamplePreparedDownloadTask(
    source: VesperDownloadSource.fromSource(
      source: VesperPlayerSource.remote(
        uri: manifestUri.toString(),
        label: preparedLabel,
        protocol: VesperPlayerSourceProtocol.dash,
      ),
      contentFormat: VesperDownloadContentFormat.dashSegments,
      manifestUri: manifestUri.toString(),
    ),
    profile: VesperDownloadProfile(targetDirectory: targetDirectory.path),
    assetIndex: VesperDownloadAssetIndex(
      contentFormat: VesperDownloadContentFormat.dashSegments,
      resources: resourceRecords.values.toList(growable: false),
      segments: segmentRecords.values.toList(growable: false),
    ),
  );
}

String _resolvePreparedHlsLabel({
  required VesperPlayerSource originalSource,
  required Uri manifestUri,
  required String manifestText,
  required String? primaryPlaylistText,
}) {
  final draftLabel = exampleDraftDownloadLabelFromUri(manifestUri.toString());
  final originalLabel = originalSource.label.trim();
  if (originalLabel.isNotEmpty && originalLabel != draftLabel) {
    return originalLabel;
  }
  return _extractHlsManifestTitle(manifestText) ??
      (primaryPlaylistText == null
          ? null
          : _extractHlsManifestTitle(primaryPlaylistText)) ??
      draftLabel;
}

String _resolvePreparedDashLabel({
  required VesperPlayerSource originalSource,
  required Uri manifestUri,
  required String manifestText,
}) {
  final draftLabel = exampleDraftDownloadLabelFromUri(manifestUri.toString());
  final originalLabel = originalSource.label.trim();
  if (originalLabel.isNotEmpty && originalLabel != draftLabel) {
    return originalLabel;
  }
  return _extractDashManifestTitle(manifestText) ?? draftLabel;
}

String? _extractHlsManifestTitle(String manifestText) {
  for (final line in const LineSplitter().convert(manifestText)) {
    final trimmed = line.trim();
    if (!trimmed.toUpperCase().startsWith('#EXT-X-SESSION-DATA')) {
      continue;
    }
    final attributes = _parseAttributeList(
      trimmed.split(':').skip(1).join(':'),
    );
    final dataId = (attributes['DATA-ID'] ?? '').toLowerCase();
    final value = (attributes['VALUE'] ?? '').trim();
    if (dataId.contains('title') && value.isNotEmpty) {
      return value;
    }
  }
  return null;
}

String? _extractDashManifestTitle(String manifestText) {
  final match = _dashTitlePattern.firstMatch(manifestText);
  return match?.group(1)?.trim().isEmpty ?? true
      ? null
      : match?.group(1)?.trim();
}

_HlsMasterSelection? _parseHlsMasterManifest(
  String manifestText,
  Uri manifestUri,
) {
  final audioPlaylists = <String, List<Uri>>{};
  final variants = <(int, Uri, String?)>[];
  int? pendingVariantBandwidth;
  String? pendingAudioGroupId;

  for (final rawLine in const LineSplitter().convert(manifestText)) {
    final line = rawLine.trim();
    if (line.toUpperCase().startsWith('#EXT-X-MEDIA')) {
      final attributes = _parseAttributeList(line.split(':').skip(1).join(':'));
      final groupId = attributes['GROUP-ID'];
      final uriValue = attributes['URI'];
      if (attributes['TYPE'] == 'AUDIO' &&
          groupId != null &&
          uriValue != null) {
        audioPlaylists[groupId] = <Uri>[
          ...?audioPlaylists[groupId],
          manifestUri.resolve(uriValue),
        ];
      }
      continue;
    }
    if (line.toUpperCase().startsWith('#EXT-X-STREAM-INF')) {
      final attributes = _parseAttributeList(line.split(':').skip(1).join(':'));
      pendingVariantBandwidth = int.tryParse(attributes['BANDWIDTH'] ?? '');
      pendingAudioGroupId = attributes['AUDIO'];
      continue;
    }
    if (pendingVariantBandwidth != null &&
        line.isNotEmpty &&
        !line.startsWith('#')) {
      variants.add((
        pendingVariantBandwidth,
        manifestUri.resolve(line),
        pendingAudioGroupId,
      ));
      pendingVariantBandwidth = null;
      pendingAudioGroupId = null;
    }
  }

  if (variants.isEmpty) {
    return null;
  }
  final selectedVariant = variants.first;
  return _HlsMasterSelection(
    variantPlaylistUri: selectedVariant.$2,
    audioPlaylistUri: selectedVariant.$3 == null
        ? null
        : audioPlaylists[selectedVariant.$3!]?.first,
  );
}

List<_HlsPlaylistEntry> _parseHlsMediaPlaylist(
  String playlistText,
  Uri playlistUri,
) {
  final entries = <_HlsPlaylistEntry>[];
  var nextSequence = 0;

  for (final rawLine in const LineSplitter().convert(playlistText)) {
    final line = rawLine.trim();
    if (line.toUpperCase().startsWith('#EXT-X-MEDIA-SEQUENCE')) {
      nextSequence =
          int.tryParse(line.split(':').skip(1).join(':')) ?? nextSequence;
      continue;
    }
    if (line.toUpperCase().startsWith('#EXT-X-KEY') ||
        line.toUpperCase().startsWith('#EXT-X-MAP')) {
      final attributes = _parseAttributeList(line.split(':').skip(1).join(':'));
      final uriValue = attributes['URI'];
      if (uriValue == null) {
        continue;
      }
      entries.add(
        _HlsPlaylistEntry(
          kind: _HlsPlaylistEntryKind.resource,
          uri: playlistUri.resolve(uriValue),
          sequence: null,
        ),
      );
      continue;
    }
    if (line.isNotEmpty && !line.startsWith('#')) {
      entries.add(
        _HlsPlaylistEntry(
          kind: _HlsPlaylistEntryKind.segment,
          uri: playlistUri.resolve(line),
          sequence: nextSequence,
        ),
      );
      nextSequence += 1;
    }
  }

  return entries;
}

Map<String, String> _parseAttributeList(String line) {
  final result = <String, String>{};
  for (final match in _attributePattern.allMatches(line)) {
    result[match.group(1)!] = (match.group(3) ?? match.group(2) ?? '').trim();
  }
  return result;
}

Map<String, String> _parseXmlAttributes(String line) {
  final result = <String, String>{};
  for (final match in _xmlAttributePattern.allMatches(line)) {
    result[match.group(1)!] = (match.group(2) ?? '').trim();
  }
  return result;
}

double? _parseDashPresentationDuration(String manifestText) {
  final mpdMatch = _dashRootPattern.firstMatch(manifestText);
  if (mpdMatch == null) {
    return null;
  }
  final attributes = _parseXmlAttributes(mpdMatch.group(1) ?? '');
  final value = attributes['mediaPresentationDuration'];
  if (value == null || value.isEmpty) {
    return null;
  }
  final match = _isoDurationPattern.firstMatch(value);
  if (match == null) {
    return null;
  }
  final hours = double.tryParse(match.group(1) ?? '') ?? 0;
  final minutes = double.tryParse(match.group(2) ?? '') ?? 0;
  final seconds = double.tryParse(match.group(3) ?? '') ?? 0;
  return hours * 3600 + minutes * 60 + seconds;
}

String _relativePathForRemoteUri(Uri uri) {
  final path = uri.path.replaceFirst(RegExp(r'^/+'), '');
  if (path.isNotEmpty) {
    return path;
  }
  final fallback = uri.pathSegments.isEmpty ? '' : uri.pathSegments.last;
  return fallback.isEmpty ? 'download.bin' : fallback;
}

Future<String> _fetchRemoteText(Uri uri) async {
  final client = HttpClient();
  try {
    final request = await client.getUrl(uri);
    final response = await request.close();
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw HttpException(
        'HTTP ${response.statusCode} while loading $uri',
        uri: uri,
      );
    }
    return await utf8.decodeStream(response);
  } finally {
    client.close(force: true);
  }
}

Future<Directory> _exampleDownloadTargetDirectory(String assetId) async {
  final directory = Directory(
    '${Directory.systemTemp.path}/vesper-downloads/$assetId',
  );
  if (!await directory.exists()) {
    await directory.create(recursive: true);
  }
  return directory;
}

final class _HlsMasterSelection {
  const _HlsMasterSelection({
    required this.variantPlaylistUri,
    required this.audioPlaylistUri,
  });

  final Uri variantPlaylistUri;
  final Uri? audioPlaylistUri;
}

enum _HlsPlaylistEntryKind { resource, segment }

final class _HlsPlaylistEntry {
  const _HlsPlaylistEntry({
    required this.kind,
    required this.uri,
    required this.sequence,
  });

  final _HlsPlaylistEntryKind kind;
  final Uri uri;
  final int? sequence;
}

final RegExp _attributePattern = RegExp(r'([A-Z0-9-]+)=("([^"]*)"|[^,]*)');
final RegExp _xmlAttributePattern = RegExp(
  r'([A-Za-z_:][A-Za-z0-9_.:-]*)="([^"]*)"',
);
final RegExp _dashRootPattern = RegExp(r'<MPD\b([^>]*)>', caseSensitive: false);
final RegExp _dashAdaptationSetPattern = RegExp(
  r'<AdaptationSet\b([^>]*)>(.*?)</AdaptationSet>',
  caseSensitive: false,
  dotAll: true,
);
final RegExp _dashRepresentationPattern = RegExp(
  r'<Representation\b([^>]*)>(.*?)</Representation>|<Representation\b([^>]*)\s*/>',
  caseSensitive: false,
  dotAll: true,
);
final RegExp _dashSegmentTemplatePattern = RegExp(
  r'<SegmentTemplate\b([^>]*)/?>',
  caseSensitive: false,
  dotAll: true,
);
final RegExp _dashTitlePattern = RegExp(
  r'<Title>(.*?)</Title>',
  caseSensitive: false,
  dotAll: true,
);
final RegExp _isoDurationPattern = RegExp(
  r'PT(?:([0-9.]+)H)?(?:([0-9.]+)M)?(?:([0-9.]+)S)?',
);

const Set<String> _genericManifestFileNames = <String>{
  'master.m3u8',
  'playlist.m3u8',
  'index.m3u8',
  'prog_index.m3u8',
  'manifest.mpd',
  'stream.mpd',
};
