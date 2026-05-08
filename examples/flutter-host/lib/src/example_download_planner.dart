import 'dart:io';

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
  final downloadSource = _downloadSourceForExample(source);
  final targetDirectory = await _exampleDownloadTargetDirectory(assetId);
  final targetOutputFormat = switch (downloadSource.contentFormat) {
    VesperDownloadContentFormat.hlsSegments ||
    VesperDownloadContentFormat.dashSegments ||
    VesperDownloadContentFormat.flvSegments => VesperDownloadOutputFormat.mp4,
    VesperDownloadContentFormat.singleFile ||
    VesperDownloadContentFormat.unknown => null,
  };

  return ExamplePreparedDownloadTask(
    source: downloadSource,
    profile: VesperDownloadProfile(
      targetOutputFormat: targetOutputFormat,
      targetDirectory: targetDirectory.path,
    ),
    assetIndex: const VesperDownloadAssetIndex(),
  );
}

VesperDownloadSource _downloadSourceForExample(VesperPlayerSource source) {
  final inferred = VesperDownloadSource.fromSource(source: source);
  if (inferred.contentFormat != VesperDownloadContentFormat.singleFile ||
      source.kind != VesperPlayerSourceKind.remote ||
      !_hasPathExtension(source.uri, 'flv')) {
    return inferred;
  }

  return VesperDownloadSource.fromSource(
    source: source,
    contentFormat: VesperDownloadContentFormat.flvSegments,
    manifestUri: source.uri,
  );
}

bool _hasPathExtension(String uri, String extension) {
  final parsedUri = Uri.tryParse(uri);
  final path = parsedUri?.path ?? uri.split('#').first.split('?').first;
  return path.toLowerCase().endsWith('.${extension.toLowerCase()}');
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

const Set<String> _genericManifestFileNames = <String>{
  'master.m3u8',
  'playlist.m3u8',
  'index.m3u8',
  'prog_index.m3u8',
  'manifest.mpd',
  'stream.mpd',
};
