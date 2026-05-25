import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  late VesperPlayerPlatform previousPlatform;
  late _FakeVesperPlatform platform;

  setUp(() {
    previousPlatform = VesperPlayerPlatform.instance;
    platform = _FakeVesperPlatform();
    VesperPlayerPlatform.instance = platform;
  });

  tearDown(() async {
    await platform.close();
    VesperPlayerPlatform.instance = previousPlatform;
  });

  test('applies compact task patches to the local snapshot map', () async {
    final initialTask = _downloadTask(
      taskId: 7,
      state: VesperDownloadState.downloading,
      receivedBytes: 0,
    );
    platform.initialSnapshot =
        VesperDownloadSnapshot(tasks: <VesperDownloadTaskSnapshot>[
      initialTask,
    ]);

    final manager = await VesperDownloadManager.create();
    addTearDown(manager.dispose);

    final snapshots = <VesperDownloadSnapshot>[];
    final subscription = manager.snapshots.listen(snapshots.add);
    addTearDown(subscription.cancel);

    platform.emit(
      const VesperDownloadTaskUpdatedEvent(
        downloadId: _FakeVesperPlatform.downloadId,
        progressPatch: VesperDownloadTaskProgressPatch(
          taskId: 7,
          progress: VesperDownloadProgressSnapshot(
            receivedBytes: 512,
            totalBytes: 1024,
            receivedSegments: 1,
            totalSegments: 2,
          ),
        ),
      ),
    );
    await _flushEvents();

    expect(manager.task(7)?.progress.receivedBytes, 512);
    expect(manager.task(7)?.assetIndex.resources,
        initialTask.assetIndex.resources);

    platform.emit(
      const VesperDownloadTaskUpdatedEvent(
        downloadId: _FakeVesperPlatform.downloadId,
        patch: VesperDownloadTaskStatePatch(
          taskId: 7,
          state: VesperDownloadState.completed,
          progress: VesperDownloadProgressSnapshot(
            receivedBytes: 1024,
            totalBytes: 1024,
            receivedSegments: 2,
            totalSegments: 2,
          ),
          completedPath: '/tmp/movie.mp4',
        ),
      ),
    );
    await _flushEvents();

    expect(manager.task(7)?.state, VesperDownloadState.completed);
    expect(manager.task(7)?.assetIndex.completedPath, '/tmp/movie.mp4');

    platform.emit(
      const VesperDownloadTaskRemovedEvent(
        downloadId: _FakeVesperPlatform.downloadId,
        taskId: 7,
      ),
    );
    await _flushEvents();

    expect(manager.task(7), isNull);
    expect(snapshots.last.tasks, isEmpty);
  });

  test('forwards share and save requests through the platform API', () async {
    final manager = await VesperDownloadManager.create();
    addTearDown(manager.dispose);

    await manager.shareTaskOutput(
      9,
      fileName: 'clip.mp4',
      mimeType: 'video/mp4',
    );
    final savedUri = await manager.saveTaskOutput(
      9,
      fileName: 'clip.mp4',
      collection: VesperDownloadPublicCollection.movies,
    );

    expect(savedUri, 'content://vesper-downloads/clip.mp4');
    expect(platform.sharedTaskId, 9);
    expect(platform.sharedFileName, 'clip.mp4');
    expect(platform.sharedMimeType, 'video/mp4');
    expect(platform.savedTaskId, 9);
    expect(platform.savedFileName, 'clip.mp4');
    expect(platform.savedCollection, VesperDownloadPublicCollection.movies);
  });

  test('player controller exposes startup plugin diagnostics', () async {
    platform.playerPluginDiagnostics = <VesperPluginDiagnostic>[
      const VesperPluginDiagnostic(
        path: '/tmp/player-decoder-fixture.dylib',
        pluginName: 'fixture-decoder',
        pluginKind: 'decoder',
        status: VesperPluginDiagnosticStatus.decoderSupported,
        capability: VesperPluginCapability.decoder(
          VesperPluginDecoderCapabilitySummary(
            codecs: <VesperPluginCodecCapability>[
              VesperPluginCodecCapability(mediaKind: 'Video', codec: 'h264'),
            ],
            legacyCodecs: <String>['Video:h264'],
            supportsNativeFrameOutput: true,
            supportsHardwareDecode: true,
            supportsGpuHandles: true,
            supportsFlush: true,
            supportsDrain: true,
            maxSessions: 1,
          ),
        ),
      ),
    ];

    final controller = await VesperPlayerController.create();
    addTearDown(controller.dispose);

    expect(controller.pluginDiagnostics, hasLength(1));
    final diagnostic = controller.pluginDiagnostics.single;
    expect(diagnostic.status, VesperPluginDiagnosticStatus.decoderSupported);
    expect(diagnostic.capability?.kind, VesperPluginCapabilityKind.decoder);
    expect(diagnostic.capability?.decoder?.codecs.single.codec, 'h264');
  });

  test('player controller preserves unsupported platform error details',
      () async {
    platform.playError = VesperUnsupportedError(
      'unsupported operation',
      'vesper_operation_failed',
      <String, Object?>{
        'code': 'unsupported',
        'category': 'capability',
        'nativeDetail': 'surface-missing',
      },
    );
    final reportedErrors = <FlutterErrorDetails>[];
    final previousOnError = FlutterError.onError;
    FlutterError.onError = reportedErrors.add;
    addTearDown(() {
      FlutterError.onError = previousOnError;
    });

    final controller = await VesperPlayerController.create();
    addTearDown(controller.dispose);

    await expectLater(
      controller.play(),
      throwsA(isA<VesperUnsupportedError>()),
    );

    final error = controller.snapshot.lastError;
    expect(error?.code, VesperPlayerErrorCode.unsupported);
    expect(error?.details['platformCode'], 'vesper_operation_failed');
    expect(error?.details['nativeDetail'], 'surface-missing');
    expect(reportedErrors.single.exception, isA<VesperUnsupportedError>());
  });
}

Future<void> _flushEvents() async {
  await Future<void>.delayed(Duration.zero);
}

VesperDownloadTaskSnapshot _downloadTask({
  required int taskId,
  required VesperDownloadState state,
  required int receivedBytes,
}) {
  return VesperDownloadTaskSnapshot(
    taskId: taskId,
    assetId: 'asset-$taskId',
    source: VesperDownloadSource.fromSource(
      source: VesperPlayerSource.hls(
        uri: 'https://example.com/master.m3u8',
        label: 'HLS demo',
      ),
      manifestUri: 'https://example.com/master.m3u8',
    ),
    profile: const VesperDownloadProfile(),
    state: state,
    progress: VesperDownloadProgressSnapshot(
      receivedBytes: receivedBytes,
      totalBytes: 1024,
      receivedSegments: receivedBytes == 0 ? 0 : 1,
      totalSegments: 2,
    ),
    assetIndex: const VesperDownloadAssetIndex(
      contentFormat: VesperDownloadContentFormat.hlsSegments,
      totalSizeBytes: 1024,
      resources: <VesperDownloadResourceRecord>[
        VesperDownloadResourceRecord(
          resourceId: 'manifest',
          uri: 'file:///tmp/.generated/manifest.m3u8',
          relativePath: 'manifest.m3u8',
          sizeBytes: 256,
        ),
      ],
      segments: <VesperDownloadSegmentRecord>[
        VesperDownloadSegmentRecord(
          segmentId: 'seg-1',
          uri: 'https://example.com/seg-1.ts',
          relativePath: 'seg-1.ts',
          sequence: 1,
          sizeBytes: 768,
        ),
      ],
    ),
  );
}

final class _FakeVesperPlatform extends VesperPlayerPlatform {
  static const String downloadId = 'download-session';

  final StreamController<VesperDownloadManagerEvent> _downloadEvents =
      StreamController<VesperDownloadManagerEvent>.broadcast();

  VesperDownloadSnapshot initialSnapshot =
      const VesperDownloadSnapshot.initial();
  List<VesperPluginDiagnostic> playerPluginDiagnostics =
      const <VesperPluginDiagnostic>[];
  Object? playError;
  int? sharedTaskId;
  String? sharedFileName;
  String? sharedMimeType;
  int? savedTaskId;
  String? savedFileName;
  VesperDownloadPublicCollection? savedCollection;

  void emit(VesperDownloadManagerEvent event) {
    _downloadEvents.add(event);
  }

  Future<void> close() async {
    await _downloadEvents.close();
  }

  @override
  Future<VesperPlatformDownloadCreateResult> createDownloadManager({
    VesperDownloadConfiguration configuration =
        const VesperDownloadConfiguration(),
    VesperDownloadStaleResourcePlanRecoveryCallback? staleResourceRecovery,
  }) async {
    return VesperPlatformDownloadCreateResult(
      downloadId: downloadId,
      snapshot: initialSnapshot,
    );
  }

  @override
  Stream<VesperDownloadManagerEvent> downloadEventsFor(String downloadId) {
    return _downloadEvents.stream
        .where((event) => event.downloadId == downloadId);
  }

  @override
  Future<void> shareDownloadTask(
    String downloadId,
    int taskId, {
    String? fileName,
    String? mimeType,
  }) async {
    sharedTaskId = taskId;
    sharedFileName = fileName;
    sharedMimeType = mimeType;
  }

  @override
  Future<String?> saveDownloadTask(
    String downloadId,
    int taskId, {
    String? fileName,
    VesperDownloadPublicCollection collection =
        VesperDownloadPublicCollection.downloads,
  }) async {
    savedTaskId = taskId;
    savedFileName = fileName;
    savedCollection = collection;
    return 'content://vesper-downloads/${fileName ?? taskId}';
  }

  @override
  Future<void> refreshDownloadManager(String downloadId) async {}

  @override
  Future<void> disposeDownloadManager(String downloadId) async {}

  @override
  Future<int?> createDownloadTask(
    String downloadId, {
    required String assetId,
    required VesperDownloadSource source,
    VesperDownloadProfile profile = const VesperDownloadProfile(),
    VesperDownloadAssetIndex assetIndex = const VesperDownloadAssetIndex(),
  }) async {
    return null;
  }

  @override
  Future<bool> startDownloadTask(String downloadId, int taskId) async => true;

  @override
  Future<bool> pauseDownloadTask(String downloadId, int taskId) async => true;

  @override
  Future<bool> resumeDownloadTask(String downloadId, int taskId) async => true;

  @override
  Future<bool> removeDownloadTask(String downloadId, int taskId) async => true;

  @override
  Future<void> exportDownloadTask(
    String downloadId,
    int taskId,
    String outputPath,
  ) async {}

  @override
  Future<VesperPlatformCreateResult> createPlayer({
    VesperPlayerSource? initialSource,
    VesperPlayerRenderSurfaceKind renderSurfaceKind =
        VesperPlayerRenderSurfaceKind.auto,
    VesperPlaybackResiliencePolicy resiliencePolicy =
        const VesperPlaybackResiliencePolicy(),
    VesperTrackPreferencePolicy trackPreferencePolicy =
        const VesperTrackPreferencePolicy(),
    VesperPreloadBudgetPolicy preloadBudgetPolicy =
        const VesperPreloadBudgetPolicy(),
    bool keepScreenOnDuringPlayback = true,
    VesperBenchmarkConfiguration benchmarkConfiguration =
        const VesperBenchmarkConfiguration.disabled(),
    VesperSourceNormalizerConfiguration sourceNormalizerConfiguration =
        const VesperSourceNormalizerConfiguration(),
    VesperFrameProcessorConfiguration frameProcessorConfiguration =
        const VesperFrameProcessorConfiguration(),
  }) async {
    return VesperPlatformCreateResult(
      playerId: 'test-player',
      snapshot: const VesperPlayerSnapshot.initial(),
      pluginDiagnostics: playerPluginDiagnostics,
    );
  }

  @override
  Stream<VesperPlayerEvent> eventsFor(String playerId) {
    return const Stream<VesperPlayerEvent>.empty();
  }

  @override
  Future<void> initialize(String playerId) async => throw UnimplementedError();

  @override
  Future<void> dispose(String playerId) async {}

  @override
  Future<void> refreshPlayer(String playerId) async =>
      throw UnimplementedError();

  @override
  Future<void> selectSource(String playerId, VesperPlayerSource source) async =>
      throw UnimplementedError();

  @override
  Future<void> play(String playerId) async {
    final error = playError;
    if (error != null) {
      throw error;
    }
    throw UnimplementedError();
  }

  @override
  Future<void> pause(String playerId) async => throw UnimplementedError();

  @override
  Future<void> togglePause(String playerId) async => throw UnimplementedError();

  @override
  Future<void> stop(String playerId) async => throw UnimplementedError();

  @override
  Future<void> seekBy(String playerId, int deltaMs) async =>
      throw UnimplementedError();

  @override
  Future<void> seekToRatio(String playerId, double ratio) async =>
      throw UnimplementedError();

  @override
  Future<void> seekToLiveEdge(String playerId) async =>
      throw UnimplementedError();

  @override
  Future<void> setPlaybackRate(String playerId, double rate) async =>
      throw UnimplementedError();

  @override
  Future<void> setVideoTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) async =>
      throw UnimplementedError();

  @override
  Future<void> setAudioTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) async =>
      throw UnimplementedError();

  @override
  Future<void> setSubtitleTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) async =>
      throw UnimplementedError();

  @override
  Future<void> setAbrPolicy(String playerId, VesperAbrPolicy policy) async =>
      throw UnimplementedError();

  @override
  Future<void> setResiliencePolicy(
    String playerId,
    VesperPlaybackResiliencePolicy policy,
  ) async =>
      throw UnimplementedError();

  @override
  Future<void> updateViewport(
    String playerId,
    VesperPlayerViewport viewport,
  ) async =>
      throw UnimplementedError();

  @override
  Future<void> clearViewport(String playerId) async =>
      throw UnimplementedError();
}
