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
/// receive opaque source handles. Native code owns source identity, leases,
/// request fencing and revisions.
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
    _processPending(initialSnapshot);
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

  /// Updates list metadata without starting or replacing playback.
  Future<void> replace(List<VesperPlaybackSequenceItem> items) async {
    await _execute(<String, Object?>{
      'type': 'replace',
      'items': items.map((item) => item.toMap()).toList(growable: false),
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

  Future<void> remove(String itemId) async {
    await _execute(<String, Object?>{'type': 'remove', 'itemId': itemId});
  }

  /// Resolves when the requested item is ready with the explicit initial state.
  Future<VesperSourceActivation> activate(
    String itemId, {
    VesperSourceActivationOptions options =
        const VesperSourceActivationOptions(),
  }) async {
    final result =
        await _navigate('activate', itemId: itemId, options: options);
    if (result == null) throw StateError('Activation did not return a source.');
    return result;
  }

  /// Returns null at a boundary, including while a page request is pending.
  Future<VesperSourceActivation?> next({
    VesperSourceActivationOptions options =
        const VesperSourceActivationOptions(),
  }) =>
      _navigate('next', options: options);

  Future<VesperSourceActivation?> previous({
    VesperSourceActivationOptions options =
        const VesperSourceActivationOptions(),
  }) =>
      _navigate('previous', options: options);

  Future<VesperSourceActivation?> _navigate(
    String type, {
    String? itemId,
    required VesperSourceActivationOptions options,
  }) async {
    final response = await _execute(<String, Object?>{
      'type': type,
      if (itemId != null) 'itemId': itemId,
      'options': options.toMap(),
    });
    final activation = response['activation'];
    return activation == null
        ? null
        : VesperSourceActivation.fromMap(vesperDecodeMap(activation));
  }

  Future<void> resync() async {
    _ensureActive();
    final value = await _platform.playbackSequenceSnapshot(
      configuration.sequenceId,
    );
    _publishSnapshot(value);
    _processPending(value);
  }

  Future<void> submitResolvedSource({
    required VesperSourceResolutionRequired request,
    required VesperSourceReference source,
  }) async {
    await _execute(<String, Object?>{
      'type': 'submitResolvedSource',
      'source': <String, Object?>{
        'sessionGeneration': request.sessionGeneration,
        'requestId': request.requestId,
        'resolutionAttemptId': request.resolutionAttemptId,
        'itemId': request.itemId,
        'expectedSourceRevision': request.expectedSourceRevision,
        'source': source.toMap(),
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

  Future<Map<String, Object?>> _execute(Map<String, Object?> command) async {
    _ensureActive();
    final response = await _platform.executePlaybackSequenceCommand(
      configuration.sequenceId,
      command,
    );
    _ensureActive();
    final value = await _platform.playbackSequenceSnapshot(
      configuration.sequenceId,
    );
    _publishSnapshot(value);
    _processPending(value);
    return response;
  }

  void _onEvent(VesperPlaybackSequenceEvent event) {
    if (_disposed) return;
    _eventsController.add(event);
    if (event is VesperPlaybackSequenceSnapshotEvent) {
      _publishSnapshot(event.snapshot);
      _processPending(event.snapshot);
    } else if (event is VesperPlaybackSequenceItemsRequestedEvent) {
      unawaited(_resolveItems(event.request));
    } else if (event is VesperPlaybackSequenceSourceResolutionRequiredEvent) {
      unawaited(_resolveSource(event.request));
    }
  }

  void _processPending(VesperPlaybackSequenceSnapshot value) {
    if (_disposed) return;
    for (final raw in value.pendingRequests) {
      final request =
          raw['request'] is Map ? vesperDecodeMap(raw['request']) : raw;
      final type = request['type'];
      if (type == 'itemsRequested') {
        unawaited(_resolveItems(VesperItemsRequested.fromMap(request)));
      } else if (type == 'sourceResolutionRequired') {
        unawaited(
            _resolveSource(VesperSourceResolutionRequired.fromMap(request)));
      }
    }
  }

  Future<void> _resolveItems(VesperItemsRequested request) async {
    final adapter = provider;
    if (adapter == null) return;
    final key = 'items:${request.sessionGeneration}:${request.requestId}';
    if (_inFlightProviderRequests.length >= configuration.maxPendingRequests ||
        !_inFlightProviderRequests.add(key)) {
      return;
    }
    try {
      final page = await adapter
          .loadItems(request)
          .timeout(_providerTimeout(request.deadline));
      if (_disposed || request.sessionGeneration != snapshot.sessionGeneration) {
        return;
      }
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
      await _failProviderRequest(
          request.sessionGeneration, request.requestId, 'provider_failed');
    } finally {
      _inFlightProviderRequests.remove(key);
    }
  }

  Future<void> _resolveSource(VesperSourceResolutionRequired request) async {
    final adapter = provider;
    if (adapter == null) return;
    final key =
        'source:${request.sessionGeneration}:${request.requestId}:${request.resolutionAttemptId}';
    if (_inFlightProviderRequests.length >= configuration.maxPendingRequests ||
        !_inFlightProviderRequests.add(key)) {
      return;
    }
    try {
      final resolved = await adapter
          .resolveSource(request)
          .timeout(_providerTimeout(request.deadline));
      if (_disposed || request.sessionGeneration != snapshot.sessionGeneration) {
        return;
      }
      await submitResolvedSource(request: request, source: resolved);
    } catch (_) {
      await _failProviderRequest(request.sessionGeneration, request.requestId,
          'source_resolution_failed');
    } finally {
      _inFlightProviderRequests.remove(key);
    }
  }

  Duration _providerTimeout(int remainingMs) => Duration(
      milliseconds:
          remainingMs > 0 && remainingMs < configuration.requestTimeoutMs
              ? remainingMs
              : configuration.requestTimeoutMs);

  Future<void> _failProviderRequest(
      int generation, int requestId, String reason) async {
    if (_disposed || generation != snapshot.sessionGeneration) return;
    try {
      await _execute(<String, Object?>{
        'type': 'failRequest',
        'sessionGeneration': generation,
        'requestId': requestId,
        'reasonCode': reason,
      });
    } catch (_) {
      // The native request may have timed out or been superseded while the
      // provider awaited. Its authoritative snapshot retains those outcomes.
      if (!_disposed) {
        try {
          await resync();
        } catch (_) {/* Native disposal can race resync. */}
      }
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
