import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  late _SequenceFakePlatform platform;
  late VesperPlayerPlatform previousPlatform;

  setUp(() {
    previousPlatform = VesperPlayerPlatform.instance;
    platform = _SequenceFakePlatform();
    VesperPlayerPlatform.instance = platform;
  });

  tearDown(() async {
    VesperPlayerPlatform.instance = previousPlatform;
    await platform.close();
  });

  test('duplicate refill notifications produce one provider call and append',
      () async {
    platform.pending = _itemsRequestPending();
    final load = Completer<VesperPlaybackSequencePage>();
    var loadCalls = 0;
    final controller = await VesperPlayerController.create();
    final sequence = await controller.attachPlaybackSequence(
      configuration: const VesperPlaybackSequenceConfiguration(
        sequenceId: 'feed',
      ),
      provider: _Provider(
        loadItems: (request) {
          loadCalls += 1;
          return load.future;
        },
      ),
    );

    await Future<void>.delayed(Duration.zero);
    expect(loadCalls, 1);

    platform.emitPendingRequestEvent();
    await sequence.resync();
    await Future<void>.delayed(Duration.zero);
    expect(loadCalls, 1);

    load.complete(
      VesperPlaybackSequencePage(
        items: <VesperPlaybackSequenceItem>[_item('b')],
        endReached: true,
      ),
    );
    await Future<void>.delayed(Duration.zero);
    await Future<void>.delayed(Duration.zero);
    expect(loadCalls, 1);
    expect(platform.appendCommands, 1);
    expect(
        sequence.snapshot.items.map((item) => item.itemId), <String>['a', 'b']);

    await sequence.dispose();
    await controller.dispose();
  });

  test('duplicate source notifications produce one resolution and submit',
      () async {
    platform.pending = _sourceRequestPending();
    final resolve = Completer<VesperResolvedSource>();
    var resolveCalls = 0;
    final controller = await VesperPlayerController.create();
    final sequence = await controller.attachPlaybackSequence(
      configuration: const VesperPlaybackSequenceConfiguration(
        sequenceId: 'feed',
      ),
      provider: _Provider(
        resolveSource: (request) {
          resolveCalls += 1;
          return resolve.future;
        },
      ),
    );

    await Future<void>.delayed(Duration.zero);
    expect(resolveCalls, 1);
    platform.emitPendingRequestEvent();
    await sequence.resync();
    await Future<void>.delayed(Duration.zero);
    expect(resolveCalls, 1);

    resolve.complete(_resolvedSource());
    await Future<void>.delayed(Duration.zero);
    await Future<void>.delayed(Duration.zero);
    expect(resolveCalls, 1);
    expect(platform.resolveCommands, 1);
    expect(sequence.snapshot.pendingRequests, isEmpty);

    await sequence.dispose();
    await controller.dispose();
  });
}

final class _Provider implements VesperPlaybackSequenceProvider {
  _Provider({
    Future<VesperPlaybackSequencePage> Function(VesperItemsRequested request)?
        loadItems,
    Future<VesperResolvedSource> Function(
      VesperSourceResolutionRequired request,
    )? resolveSource,
  })  : _loadItems = loadItems,
        _resolveSource = resolveSource;

  final Future<VesperPlaybackSequencePage> Function(
      VesperItemsRequested request)? _loadItems;
  final Future<VesperResolvedSource> Function(
    VesperSourceResolutionRequired request,
  )? _resolveSource;

  @override
  Future<VesperPlaybackSequencePage> loadItems(VesperItemsRequested request) {
    final callback = _loadItems;
    if (callback == null) {
      return Future.error(StateError('unexpected item refill'));
    }
    return callback(request);
  }

  @override
  Future<VesperResolvedSource> resolveSource(
    VesperSourceResolutionRequired request,
  ) {
    final callback = _resolveSource;
    if (callback == null) {
      return Future.error(StateError('unexpected source resolution'));
    }
    return callback(request);
  }
}

final class _SequenceFakePlatform extends VesperPlayerPlatform {
  final StreamController<VesperPlayerEvent> _playerEvents =
      StreamController<VesperPlayerEvent>.broadcast();
  final StreamController<VesperPlaybackSequenceEvent> _sequenceEvents =
      StreamController<VesperPlaybackSequenceEvent>.broadcast();

  VesperPlaybackSequenceSnapshot? pending;
  int appendCommands = 0;
  int resolveCommands = 0;

  Future<void> close() async {
    await _playerEvents.close();
    await _sequenceEvents.close();
  }

  void emitPendingRequestEvent() {
    final snapshot = pending;
    if (snapshot == null || snapshot.pendingRequests.isEmpty) return;
    final raw = snapshot.pendingRequests.single['request'];
    if (raw is! Map) return;
    _sequenceEvents.add(
      VesperPlaybackSequenceEvent.fromMap(
        Map<Object?, Object?>.from(raw),
      ),
    );
  }

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
    VesperNativeFramePipelineConfiguration nativeFramePipelineConfiguration =
        const VesperNativeFramePipelineConfiguration(),
    VesperPipelineEventHookConfiguration pipelineEventHookConfiguration =
        const VesperPipelineEventHookConfiguration(),
  }) async {
    return const VesperPlatformCreateResult(
      playerId: 'test-player',
      snapshot: VesperPlayerSnapshot.initial(),
    );
  }

  @override
  Stream<VesperPlayerEvent> eventsFor(String playerId) => _playerEvents.stream;

  @override
  Future<void> initialize(String playerId) async {}

  @override
  Future<void> dispose(String playerId) async {}

  @override
  Future<void> selectSource(String playerId, VesperPlayerSource source) async {}

  @override
  Future<void> play(String playerId) async {}

  @override
  Future<void> pause(String playerId) async {}

  @override
  Future<void> togglePause(String playerId) async {}

  @override
  Future<void> stop(String playerId) async {}

  @override
  Future<void> seekBy(String playerId, int deltaMs) async {}

  @override
  Future<void> seekToRatio(String playerId, double ratio) async {}

  @override
  Future<void> seekToLiveEdge(String playerId) async {}

  @override
  Future<void> setPlaybackRate(String playerId, double rate) async {}

  @override
  Future<void> setVideoTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) async {}

  @override
  Future<void> setAudioTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) async {}

  @override
  Future<void> setSubtitleTrackSelection(
    String playerId,
    VesperTrackSelection selection,
  ) async {}

  @override
  Future<void> setAbrPolicy(
    String playerId,
    VesperAbrPolicy policy, {
    int? expectedCatalogRevision,
  }) async {}

  @override
  Future<void> setResiliencePolicy(
    String playerId,
    VesperPlaybackResiliencePolicy policy,
  ) async {}

  @override
  Future<void> updateViewport(
    String playerId,
    VesperPlayerViewport viewport,
  ) async {}

  @override
  Future<void> clearViewport(String playerId) async {}

  @override
  Future<VesperPlatformDownloadCreateResult> createDownloadManager({
    VesperDownloadConfiguration configuration =
        const VesperDownloadConfiguration(),
    VesperDownloadStaleResourcePlanRecoveryCallback? staleResourceRecovery,
  }) async {
    return const VesperPlatformDownloadCreateResult(
      downloadId: 'download',
      snapshot: VesperDownloadSnapshot.initial(),
    );
  }

  @override
  Stream<VesperDownloadManagerEvent> downloadEventsFor(String downloadId) =>
      const Stream<VesperDownloadManagerEvent>.empty();

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
  }) async =>
      null;

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
  Future<void> shareDownloadTask(
    String downloadId,
    int taskId, {
    String? fileName,
    String? mimeType,
  }) async {}

  @override
  Future<String?> saveDownloadTask(
    String downloadId,
    int taskId, {
    String? fileName,
    VesperDownloadPublicCollection collection =
        VesperDownloadPublicCollection.downloads,
  }) async =>
      null;

  @override
  Future<VesperPlaybackSequenceSnapshot> createPlaybackSequence(
    String playerId,
    VesperPlaybackSequenceConfiguration configuration,
  ) async {
    pending ??= _emptySnapshot(configuration.sequenceId);
    return pending!;
  }

  @override
  Stream<VesperPlaybackSequenceEvent> playbackSequenceEventsFor(
    String sequenceId,
  ) =>
      _sequenceEvents.stream.where((event) => event.sequenceId == sequenceId);

  @override
  Future<Map<String, Object?>> executePlaybackSequenceCommand(
    String sequenceId,
    Map<String, Object?> command,
  ) async {
    final snapshot = pending ?? _emptySnapshot(sequenceId);
    final type = command['type'];
    final envelope =
        command['source'] is Map ? vesperDecodeMap(command['source']) : command;
    final requestId = (envelope['requestId'] as num?)?.toInt();
    if (type == 'append') {
      appendCommands += 1;
      final additions = (command['items'] as Iterable?)
              ?.whereType<Map>()
              .map((item) => VesperPlaybackSequenceItem.fromMap(
                    Map<Object?, Object?>.from(item),
                  ))
              .toList(growable: false) ??
          const <VesperPlaybackSequenceItem>[];
      pending = _withItems(
        snapshot,
        <VesperPlaybackSequenceItemState>[
          ...snapshot.items,
          ...additions.map((item) => _itemState(item.itemId)),
        ],
        pendingRequests: snapshot.pendingRequests
            .where((raw) =>
                vesperDecodeMap(raw['request'])['requestId'] != requestId)
            .toList(growable: false),
      );
    } else if (type == 'submitResolvedSource') {
      resolveCommands += 1;
      pending = _withItems(
        snapshot,
        snapshot.items,
        pendingRequests: snapshot.pendingRequests
            .where((raw) =>
                vesperDecodeMap(raw['request'])['requestId'] != requestId)
            .toList(growable: false),
      );
    }
    return const <String, Object?>{};
  }

  @override
  Future<VesperPlaybackSequenceSnapshot> playbackSequenceSnapshot(
    String sequenceId,
  ) async =>
      pending ?? _emptySnapshot(sequenceId);

  @override
  Future<void> disposePlaybackSequence(String sequenceId) async {}
}

VesperPlaybackSequenceSnapshot _itemsRequestPending() => _snapshotWithRequest(
      <String, Object?>{
        'type': 'itemsRequested',
        'sequenceId': 'feed',
        'sessionGeneration': 1,
        'requestId': 7,
        'direction': 'next',
        'anchorItemId': 'a',
        'maxCount': 1,
        'deadlineRemainingMs': 1000,
      },
    );

VesperPlaybackSequenceSnapshot _sourceRequestPending() => _snapshotWithRequest(
      <String, Object?>{
        'type': 'sourceResolutionRequired',
        'sequenceId': 'feed',
        'sessionGeneration': 1,
        'requestId': 8,
        'resolutionAttemptId': 3,
        'itemId': 'a',
        'expectedSourceRevision': 0,
        'reason': 'initial',
        'deadlineRemainingMs': 1000,
      },
    );

VesperPlaybackSequenceSnapshot _snapshotWithRequest(
  Map<String, Object?> request,
) =>
    VesperPlaybackSequenceSnapshot.fromMap(<Object?, Object?>{
      'sequenceId': 'feed',
      'sessionGeneration': 1,
      'activationEpoch': 1,
      'items': <Object?>[
        <Object?, Object?>{
          'index': 0,
          'isActive': true,
          'item': <Object?, Object?>{
            'itemId': 'a',
            'mediaKind': 'vod',
            'sourceState': <Object?, Object?>{
              'state': 'unresolved',
              'sourceRevision': 0,
            },
          },
        },
      ],
      'activeItemId': 'a',
      'pendingRequests': <Object?>[
        <Object?, Object?>{'request': request}
      ],
      'requestFailures': const <Object?>[],
      'previousEndReached': false,
      'nextEndReached': false,
      'droppedEvents': 0,
    });

VesperPlaybackSequenceSnapshot _emptySnapshot(String sequenceId) =>
    VesperPlaybackSequenceSnapshot.fromMap(<Object?, Object?>{
      'sequenceId': sequenceId,
      'sessionGeneration': 1,
      'activationEpoch': 1,
      'items': const <Object?>[],
      'pendingRequests': const <Object?>[],
      'requestFailures': const <Object?>[],
      'previousEndReached': false,
      'nextEndReached': false,
      'droppedEvents': 0,
    });

VesperPlaybackSequenceSnapshot _withItems(
  VesperPlaybackSequenceSnapshot snapshot,
  List<VesperPlaybackSequenceItemState> items, {
  required List<Map<String, Object?>> pendingRequests,
}) =>
    VesperPlaybackSequenceSnapshot(
      sequenceId: snapshot.sequenceId,
      sessionGeneration: snapshot.sessionGeneration,
      activationEpoch: snapshot.activationEpoch,
      items: items,
      activeItemId: snapshot.activeItemId,
      pendingRequests: pendingRequests,
      requestFailures: snapshot.requestFailures,
      previousEndReached: snapshot.previousEndReached,
      nextEndReached: snapshot.nextEndReached,
      droppedEvents: snapshot.droppedEvents,
    );

VesperPlaybackSequenceItem _item(String itemId) => VesperPlaybackSequenceItem(
      itemId: itemId,
      contentIdentity: VesperPlaybackSequenceContentIdentity(
        providerNamespace: 'example.provider',
        value: itemId,
      ),
    );

VesperPlaybackSequenceItemState _itemState(String itemId) =>
    VesperPlaybackSequenceItemState(
      itemId: itemId,
      index: itemId == 'a' ? 0 : 1,
      isActive: itemId == 'a',
      mediaKind: 'vod',
      sourceState: 'unresolved',
      sourceRevision: 0,
    );

VesperResolvedSource _resolvedSource() => VesperResolvedSource(
      itemId: 'a',
      expectedSourceRevision: 0,
      sourceRevision: 1,
      source: VesperPlayerSource.remote(uri: 'https://example.com/a.mp4'),
      cacheIdentity: const VesperPlaybackSequenceCacheIdentity(
        providerNamespace: 'example.provider',
        contentIdentity: 'a',
        renditionIdentity: '720p',
        resourceIdentity: 'progressive',
        accessPartition: 'public',
        sourceRevision: 1,
      ),
    );
