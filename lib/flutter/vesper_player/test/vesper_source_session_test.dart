import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  late VesperPlayerPlatform previous;
  late _SourcePlatform platform;
  final descriptor = VesperPlayerSource.remote(
      uri: 'https://media.test/video.mp4', label: 'Fixture');

  setUp(() {
    previous = VesperPlayerPlatform.instance;
    platform = _SourcePlatform();
    VesperPlayerPlatform.instance = platform;
  });
  tearDown(() => VesperPlayerPlatform.instance = previous);

  test('registration and preload need no player and deduplicate active work',
      () async {
    final session = await VesperSourceSession.create();
    final handle = await session.register(descriptor);
    final pending = handle.preload();
    final duplicate = handle.preload();
    expect(identical(pending, duplicate), isTrue);
    final task = await pending;
    expect(await handle.preload(), same(task));
    expect(platform.preloadCalls, 1);
    expect(platform.waitCalls, 1);
    expect(platform.playerCalls, 0);
    platform.complete('completed');
    expect((await task.completion).isTerminal, isTrue);
    expect(task.snapshot.reuse, VesperSourcePreloadReuse.playbackReusable);
    await session.dispose();
  });

  test('registration completing after disposal is released and never exposed',
      () async {
    platform.registration = Completer<VesperSourceReference>();
    final session = await VesperSourceSession.create();
    final registration = session.register(descriptor);
    final failed = expectLater(registration, throwsStateError);
    await session.dispose();
    platform.registration!.complete(platform.reference);
    await failed;
    expect(platform.releases, 1);
    expect(session.isClosed, isTrue);
  });

  test('source disposal is idempotent and cancels a retained preload waiter',
      () async {
    final session = await VesperSourceSession.create();
    final handle = await session.register(descriptor);
    final task = await handle.preload();
    await handle.dispose();
    await handle.dispose();
    expect(platform.releases, 1);
    expect((await task.completion).status, VesperSourcePreloadStatus.cancelled);
    expect(() => handle.preload(), throwsStateError);
    await session.dispose();
    await session.dispose();
    expect(platform.disposals, 1);
  });

  test('stale refresh cannot overwrite a terminal result', () async {
    final session = await VesperSourceSession.create();
    final handle = await session.register(descriptor);
    final task = await handle.preload();
    final refresh = task.refresh();
    platform.complete('completed');
    await task.completion;
    platform.refresh.complete(platform.observation('running'));
    expect((await refresh).status, VesperSourcePreloadStatus.completed);
    expect(task.snapshot.status, VesperSourcePreloadStatus.completed);
    await session.dispose();
  });

  test('pending registrations count against the source cap', () async {
    platform.registration = Completer<VesperSourceReference>();
    final session = await VesperSourceSession.create(
        configuration: const VesperSourceSessionConfiguration(maxSources: 1));
    final first = session.register(descriptor);
    await expectLater(session.register(descriptor), throwsStateError);
    platform.registration!.complete(platform.reference);
    await first;
    await session.invalidate();
    expect(platform.invalidations, 1);
    expect(session.isClosed, isTrue);
  });
}

final class _SourcePlatform extends VesperPlayerPlatform {
  final reference =
      const VesperSourceReference(sessionId: 'session', sourceId: 'source');
  Completer<VesperSourceReference>? registration;
  final terminal = Completer<VesperSourcePreloadSnapshot>();
  final refresh = Completer<VesperSourcePreloadSnapshot>();
  int preloadCalls = 0,
      waitCalls = 0,
      playerCalls = 0,
      releases = 0,
      disposals = 0,
      invalidations = 0;

  VesperSourcePreloadSnapshot observation(String status) =>
      VesperSourcePreloadSnapshot(
          taskId: 'task',
          source: reference,
          rawStatus: status,
          rawGoal: 'dashSegmentBaseStartup',
          rawReuse: 'playbackReusable');

  void complete(String status) {
    if (!terminal.isCompleted) terminal.complete(observation(status));
  }

  @override
  Future<String> createSourceSession(
          VesperSourceSessionConfiguration configuration) async =>
      'session';

  @override
  Future<VesperSourceReference> registerSource(
          String sessionId, VesperPlayerSource source,
          {int? expiresAtEpochMs}) async =>
      registration == null ? reference : registration!.future;

  @override
  Future<VesperSourcePreloadSnapshot> preloadSource(
      VesperSourceReference source, VesperSourcePreloadOptions options) async {
    preloadCalls++;
    return observation('queued');
  }

  @override
  Future<VesperSourcePreloadSnapshot> awaitSourcePreload(
      String sessionId, String taskId) {
    waitCalls++;
    return terminal.future;
  }

  @override
  Future<VesperSourcePreloadSnapshot> sourcePreloadSnapshot(
          String sessionId, String taskId) =>
      refresh.future;

  @override
  Future<void> releaseSource(VesperSourceReference source) async {
    releases++;
    complete('cancelled');
  }

  @override
  Future<void> disposeSourceSession(String sessionId) async {
    disposals++;
    complete('cancelled');
  }

  @override
  Future<void> invalidateSourceSession(String sessionId) async {
    invalidations++;
    complete('cancelled');
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
