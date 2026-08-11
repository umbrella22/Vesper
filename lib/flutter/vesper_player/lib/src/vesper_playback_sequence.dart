import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

import 'vesper_player_controller.dart';

extension VesperPlayerControllerSequenceExtension on VesperPlayerController {
  Future<VesperPlaybackSequence> attachPlaybackSequence({
    VesperPlaybackSequenceConfiguration configuration =
        const VesperPlaybackSequenceConfiguration(sequenceId: 'sequence'),
    VesperPlaybackSequenceProvider? provider,
  }) =>
      VesperPlaybackSequence.attach(
        this,
        configuration: configuration,
        provider: provider,
      );
}

/// The provider-side asynchronous facade for a native playback sequence.
///
/// The provider owns pagination and signed source resolution. Native and Rust
/// only receive opaque content/cache metadata and the host-local source
/// registry entry required to activate a source.
final class VesperPlaybackSequence {
  VesperPlaybackSequence._({
    required this.controller,
    required this.configuration,
    required VesperPlayerPlatform platform,
    required VesperPlaybackSequenceSnapshot initialSnapshot,
    this.provider,
  })  : _platform = platform,
        snapshotListenable = ValueNotifier<VesperPlaybackSequenceSnapshot>(
          initialSnapshot,
        ) {
    _eventsController.add(
      VesperPlaybackSequenceSnapshotEvent(
        sequenceId: configuration.sequenceId,
        sessionGeneration: initialSnapshot.sessionGeneration,
        snapshot: initialSnapshot,
      ),
    );
    _subscription = _platform
        .playbackSequenceEventsFor(configuration.sequenceId)
        .listen(_onEvent);
    unawaited(_processPending(initialSnapshot));
  }

  static Future<VesperPlaybackSequence> attach(
    VesperPlayerController controller, {
    VesperPlaybackSequenceConfiguration configuration =
        const VesperPlaybackSequenceConfiguration(sequenceId: 'sequence'),
    VesperPlaybackSequenceProvider? provider,
  }) async {
    final initial = await controller.platformForSequence.createPlaybackSequence(
      controller.playerId,
      configuration,
    );
    return VesperPlaybackSequence._(
      controller: controller,
      configuration: configuration,
      platform: controller.platformForSequence,
      initialSnapshot: initial,
      provider: provider,
    );
  }

  final VesperPlayerController controller;
  final VesperPlaybackSequenceConfiguration configuration;
  final VesperPlaybackSequenceProvider? provider;
  final VesperPlayerPlatform _platform;
  final ValueNotifier<VesperPlaybackSequenceSnapshot> snapshotListenable;
  final StreamController<VesperPlaybackSequenceEvent> _eventsController =
      StreamController<VesperPlaybackSequenceEvent>.broadcast();

  StreamSubscription<VesperPlaybackSequenceEvent>? _subscription;
  final Set<String> _inFlightProviderRequests = <String>{};
  bool _disposed = false;

  VesperPlaybackSequenceSnapshot get snapshot => snapshotListenable.value;

  Stream<VesperPlaybackSequenceEvent> get events => _eventsController.stream;

  Future<void> replace(
    List<VesperPlaybackSequenceItem> items, {
    String? activeItemId,
  }) async {
    await _execute(<String, Object?>{
      'type': 'replace',
      'items': items.map((item) => item.toMap()).toList(growable: false),
      'activeItemId':
          activeItemId ?? (items.isEmpty ? null : items.first.itemId),
    });
  }

  Future<void> append({
    required int sessionGeneration,
    required int requestId,
    String? anchorItemId,
    required List<VesperPlaybackSequenceItem> items,
    bool endReached = false,
  }) async {
    await _execute(<String, Object?>{
      'type': 'append',
      'sessionGeneration': sessionGeneration,
      'requestId': requestId,
      'anchorItemId': anchorItemId,
      'items': items.map((item) => item.toMap()).toList(growable: false),
      'endReached': endReached,
    });
  }

  Future<void> prepend({
    required int sessionGeneration,
    required int requestId,
    String? anchorItemId,
    required List<VesperPlaybackSequenceItem> items,
    bool endReached = false,
  }) async {
    await _execute(<String, Object?>{
      'type': 'prepend',
      'sessionGeneration': sessionGeneration,
      'requestId': requestId,
      'anchorItemId': anchorItemId,
      'items': items.map((item) => item.toMap()).toList(growable: false),
      'endReached': endReached,
    });
  }

  Future<void> remove(String itemId) => _execute(<String, Object?>{
        'type': 'remove',
        'itemId': itemId,
      });

  Future<void> setActive(String itemId) => _execute(<String, Object?>{
        'type': 'setActive',
        'itemId': itemId,
      });

  Future<void> next() => _execute(const <String, Object?>{'type': 'next'});

  Future<void> previous() =>
      _execute(const <String, Object?>{'type': 'previous'});

  Future<void> resync() async {
    _ensureActive();
    final value = await _platform.playbackSequenceSnapshot(
      configuration.sequenceId,
    );
    _publishSnapshot(value);
    await _processPending(value);
  }

  Future<void> submitResolvedSource({
    required VesperSourceResolutionRequired request,
    required VesperResolvedSource resolved,
  }) async {
    await _execute(<String, Object?>{
      'type': 'submitResolvedSource',
      'source': <String, Object?>{
        'sessionGeneration': request.sessionGeneration,
        'requestId': request.requestId,
        'resolutionAttemptId': request.resolutionAttemptId,
        'itemId': resolved.itemId,
        'expectedSourceRevision': resolved.expectedSourceRevision,
        'sourceRevision': resolved.sourceRevision,
        'source': resolved.source.toMap(),
        'cacheIdentity': resolved.cacheIdentity.toMap(),
        'expiresAtEpochMs': resolved.expiresAtEpochMs,
      },
    });
  }

  Future<void> dispose() async {
    if (_disposed) return;
    _disposed = true;
    Object? failure;
    StackTrace? stack;
    try {
      await _platform.disposePlaybackSequence(configuration.sequenceId);
    } catch (error, trace) {
      failure = error;
      stack = trace;
    }
    await _subscription?.cancel();
    await _eventsController.close();
    snapshotListenable.dispose();
    if (failure != null) Error.throwWithStackTrace(failure, stack!);
  }

  Future<void> _execute(Map<String, Object?> command) async {
    _ensureActive();
    await _platform.executePlaybackSequenceCommand(
      configuration.sequenceId,
      command,
    );
    final value = await _platform.playbackSequenceSnapshot(
      configuration.sequenceId,
    );
    _publishSnapshot(value);
    await _processPending(value);
  }

  void _onEvent(VesperPlaybackSequenceEvent event) {
    if (_disposed) return;
    _eventsController.add(event);
    if (event is VesperPlaybackSequenceSnapshotEvent) {
      _publishSnapshot(event.snapshot);
      unawaited(_processPending(event.snapshot));
    } else if (event is VesperPlaybackSequenceItemsRequestedEvent) {
      unawaited(_resolveItems(event.request));
    } else if (event is VesperPlaybackSequenceSourceResolutionRequiredEvent) {
      unawaited(_resolveSource(event.request));
    }
  }

  Future<void> _processPending(VesperPlaybackSequenceSnapshot value) async {
    for (final raw in value.pendingRequests) {
      final request =
          raw['request'] is Map ? vesperDecodeMap(raw['request']) : raw;
      final type = request['type'];
      if (type == 'itemsRequested') {
        await _resolveItems(VesperItemsRequested.fromMap(request));
      } else if (type == 'sourceResolutionRequired') {
        await _resolveSource(VesperSourceResolutionRequired.fromMap(request));
      }
    }
  }

  Future<void> _resolveItems(VesperItemsRequested request) async {
    final adapter = provider;
    if (adapter == null) return;
    final key = 'items:${request.sessionGeneration}:${request.requestId}';
    if (!_inFlightProviderRequests.add(key)) return;
    try {
      final page = await adapter.loadItems(request);
      final command = <String, Object?>{
        'type': request.direction == VesperPlaybackSequenceDirection.next
            ? 'append'
            : 'prepend',
        'sessionGeneration': request.sessionGeneration,
        'requestId': request.requestId,
        'anchorItemId': request.anchorItemId,
        'items': page.items.map((item) => item.toMap()).toList(growable: false),
        'endReached': page.endReached,
      };
      await _execute(command);
    } catch (_) {
      await _execute(<String, Object?>{
        'type': 'failRequest',
        'sessionGeneration': request.sessionGeneration,
        'requestId': request.requestId,
        'reasonCode': 'provider_failed',
      });
    } finally {
      _inFlightProviderRequests.remove(key);
    }
  }

  Future<void> _resolveSource(VesperSourceResolutionRequired request) async {
    final adapter = provider;
    if (adapter == null) return;
    final key =
        'source:${request.sessionGeneration}:${request.requestId}:${request.resolutionAttemptId}';
    if (!_inFlightProviderRequests.add(key)) return;
    try {
      final resolved = await adapter.resolveSource(request);
      await submitResolvedSource(request: request, resolved: resolved);
    } catch (_) {
      await _execute(<String, Object?>{
        'type': 'failRequest',
        'sessionGeneration': request.sessionGeneration,
        'requestId': request.requestId,
        'reasonCode': 'source_resolution_failed',
      });
    } finally {
      _inFlightProviderRequests.remove(key);
    }
  }

  void _publishSnapshot(VesperPlaybackSequenceSnapshot value) {
    if (_disposed) return;
    snapshotListenable.value = value;
  }

  void _ensureActive() {
    if (_disposed) {
      throw StateError('VesperPlaybackSequence has already been disposed.');
    }
  }
}
