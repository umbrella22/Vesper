import 'models.dart';

enum VesperPlaybackSequenceMode { finite, replenishable }

enum VesperPlaybackSequenceMediaKind { vod, live, liveDvr }

enum VesperPlaybackSequenceSourceState {
  unresolved,
  resolving,
  resolved,
  expired,
  failed,
}

enum VesperPlaybackSequenceDirection { previous, next }

final class VesperPlaybackSequenceConfiguration {
  const VesperPlaybackSequenceConfiguration({
    required this.sequenceId,
    this.mode = VesperPlaybackSequenceMode.finite,
    this.historyLimit = 16,
    this.forwardWindow = 1,
    this.refillThreshold = 1,
    this.maxItems = 512,
    this.maxPendingRequests = 32,
    this.maxEvents = 512,
    this.requestTimeoutMs = 15000,
    this.sourceExpiryLeadMs = 15000,
    this.maxSourceRegistryEntries = 1024,
  })  : assert(sequenceId != ''),
        assert(historyLimit >= 0),
        assert(forwardWindow >= 0),
        assert(refillThreshold >= 0),
        assert(maxItems > 0 && maxItems <= 512),
        assert(maxPendingRequests > 0 && maxPendingRequests <= 512),
        assert(maxEvents > 0 && maxEvents <= 1024),
        assert(maxSourceRegistryEntries >= maxItems &&
            maxSourceRegistryEntries <= 4096);

  final String sequenceId;
  final VesperPlaybackSequenceMode mode;
  final int historyLimit;
  final int forwardWindow;
  final int refillThreshold;
  final int maxItems;
  final int maxPendingRequests;
  final int maxEvents;
  final int requestTimeoutMs;
  final int sourceExpiryLeadMs;
  final int maxSourceRegistryEntries;

  Map<String, Object?> toMap() => <String, Object?>{
        'sequenceId': sequenceId,
        'mode': mode.name,
        'historyLimit': historyLimit,
        'forwardWindow': forwardWindow,
        'refillThreshold': refillThreshold,
        'maxItems': maxItems,
        'maxPendingRequests': maxPendingRequests,
        'maxEvents': maxEvents,
        'requestTimeoutMs': requestTimeoutMs,
        'sourceExpiryLeadMs': sourceExpiryLeadMs,
        'maxSourceRegistryEntries': maxSourceRegistryEntries,
      };
}

final class VesperPlaybackSequenceContentIdentity {
  const VesperPlaybackSequenceContentIdentity({
    required this.providerNamespace,
    required this.value,
  })  : assert(providerNamespace != ''),
        assert(value != '');

  factory VesperPlaybackSequenceContentIdentity.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperPlaybackSequenceContentIdentity(
      providerNamespace: map['providerNamespace'] as String? ?? '',
      value: map['value'] as String? ?? map['contentIdentity'] as String? ?? '',
    );
  }

  final String providerNamespace;
  final String value;

  Map<String, Object?> toMap() => <String, Object?>{
        'providerNamespace': providerNamespace,
        'value': value,
      };
}

final class VesperPlaybackSequenceCacheIdentity {
  const VesperPlaybackSequenceCacheIdentity({
    required this.providerNamespace,
    required this.contentIdentity,
    required this.renditionIdentity,
    required this.resourceIdentity,
    required this.accessPartition,
    required this.sourceRevision,
  })  : assert(providerNamespace != ''),
        assert(contentIdentity != ''),
        assert(renditionIdentity != ''),
        assert(resourceIdentity != ''),
        assert(accessPartition != ''),
        assert(sourceRevision > 0);

  factory VesperPlaybackSequenceCacheIdentity.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperPlaybackSequenceCacheIdentity(
      providerNamespace: map['providerNamespace'] as String? ?? '',
      contentIdentity: map['contentIdentity'] as String? ?? '',
      renditionIdentity: map['renditionIdentity'] as String? ?? '',
      resourceIdentity: map['resourceIdentity'] as String? ?? '',
      accessPartition: map['accessPartition'] as String? ?? '',
      sourceRevision: _asInt(map['sourceRevision']),
    );
  }

  final String providerNamespace;
  final String contentIdentity;
  final String renditionIdentity;
  final String resourceIdentity;
  final String accessPartition;
  final int sourceRevision;

  Map<String, Object?> toMap() => <String, Object?>{
        'providerNamespace': providerNamespace,
        'contentIdentity': contentIdentity,
        'renditionIdentity': renditionIdentity,
        'resourceIdentity': resourceIdentity,
        'accessPartition': accessPartition,
        'sourceRevision': sourceRevision,
      };
}

final class VesperPlaybackSequencePreloadProfile {
  const VesperPlaybackSequencePreloadProfile({
    this.expectedMemoryBytes = 0,
    this.expectedDiskBytes = 0,
    this.ttlMs,
    this.warmupWindowMs,
  });

  final int expectedMemoryBytes;
  final int expectedDiskBytes;
  final int? ttlMs;
  final int? warmupWindowMs;

  Map<String, Object?> toMap() => <String, Object?>{
        'expectedMemoryBytes': expectedMemoryBytes,
        'expectedDiskBytes': expectedDiskBytes,
        'ttlMs': ttlMs,
        'warmupWindowMs': warmupWindowMs,
      };
}

final class VesperPlaybackSequenceItem {
  const VesperPlaybackSequenceItem({
    required this.itemId,
    required this.contentIdentity,
    this.mediaKind = VesperPlaybackSequenceMediaKind.vod,
    this.source,
    this.cacheIdentity,
    this.sourceRevision = 0,
    this.expiresAtEpochMs,
    this.providerMetadataRef,
    this.preloadProfile = const VesperPlaybackSequencePreloadProfile(),
  })  : assert(itemId != ''),
        assert((source == null) == (cacheIdentity == null)),
        assert(source == null || sourceRevision > 0);

  factory VesperPlaybackSequenceItem.fromMap(Map<Object?, Object?> map) {
    final rawSource = map['source'];
    final rawCache = map['cacheIdentity'];
    final rawProfile = vesperDecodeMap(map['preloadProfile']);
    return VesperPlaybackSequenceItem(
      itemId: map['itemId'] as String? ?? '',
      contentIdentity: VesperPlaybackSequenceContentIdentity(
        providerNamespace: map['providerNamespace'] as String? ?? '',
        value: map['contentIdentity'] as String? ?? '',
      ),
      mediaKind: _decodeEnum(
        VesperPlaybackSequenceMediaKind.values,
        map['mediaKind'],
        VesperPlaybackSequenceMediaKind.vod,
      ),
      source: rawSource is Map
          ? VesperPlayerSource.fromMap(Map<Object?, Object?>.from(rawSource))
          : null,
      cacheIdentity: rawCache is Map
          ? VesperPlaybackSequenceCacheIdentity.fromMap(
              Map<Object?, Object?>.from(rawCache),
            )
          : null,
      sourceRevision: _asInt(map['sourceRevision']),
      expiresAtEpochMs: _asNullableInt(map['expiresAtEpochMs']),
      providerMetadataRef: map['providerMetadataRef'] as String?,
      preloadProfile: VesperPlaybackSequencePreloadProfile(
        expectedMemoryBytes: _asInt(rawProfile['expectedMemoryBytes']),
        expectedDiskBytes: _asInt(rawProfile['expectedDiskBytes']),
        ttlMs: _asNullableInt(rawProfile['ttlMs']),
        warmupWindowMs: _asNullableInt(rawProfile['warmupWindowMs']),
      ),
    );
  }

  final String itemId;
  final VesperPlaybackSequenceContentIdentity contentIdentity;
  final VesperPlaybackSequenceMediaKind mediaKind;
  final VesperPlayerSource? source;
  final VesperPlaybackSequenceCacheIdentity? cacheIdentity;
  final int sourceRevision;
  final int? expiresAtEpochMs;
  final String? providerMetadataRef;
  final VesperPlaybackSequencePreloadProfile preloadProfile;

  Map<String, Object?> toMap() => <String, Object?>{
        'itemId': itemId,
        'providerNamespace': contentIdentity.providerNamespace,
        'contentIdentity': contentIdentity.value,
        'mediaKind': mediaKind.name,
        if (source != null) 'source': source!.toMap(),
        if (cacheIdentity != null) 'cacheIdentity': cacheIdentity!.toMap(),
        'sourceRevision': sourceRevision,
        'expiresAtEpochMs': expiresAtEpochMs,
        'providerMetadataRef': providerMetadataRef,
        'preloadProfile': preloadProfile.toMap(),
      };
}

final class VesperPlaybackSequenceItemState {
  const VesperPlaybackSequenceItemState({
    required this.itemId,
    required this.index,
    required this.isActive,
    required this.mediaKind,
    required this.sourceState,
    required this.sourceRevision,
    this.sourceReference,
  });

  factory VesperPlaybackSequenceItemState.fromMap(Map<Object?, Object?> map) {
    final item = vesperDecodeMap(map['item']);
    final state = vesperDecodeMap(item['sourceState']);
    return VesperPlaybackSequenceItemState(
      itemId: item['itemId'] as String? ?? map['itemId'] as String? ?? '',
      index: _asInt(map['index']),
      isActive: map['isActive'] == true,
      mediaKind: item['mediaKind'] as String? ?? 'vod',
      sourceState: state['state'] as String? ?? 'unresolved',
      sourceRevision: _asInt(
        state['sourceRevision'] ?? state['expectedSourceRevision'],
      ),
      sourceReference: state['sourceReference'] as String?,
    );
  }

  final String itemId;
  final int index;
  final bool isActive;
  final String mediaKind;
  final String sourceState;
  final int sourceRevision;
  final String? sourceReference;
}

final class VesperPlaybackSequenceSnapshot {
  const VesperPlaybackSequenceSnapshot({
    required this.sequenceId,
    required this.sessionGeneration,
    required this.activationEpoch,
    required this.items,
    required this.activeItemId,
    required this.pendingRequests,
    required this.requestFailures,
    required this.previousEndReached,
    required this.nextEndReached,
    required this.droppedEvents,
    this.warmupTasks = const <VesperPlaybackSequenceWarmupTask>[],
    this.warmupStats = const VesperPlaybackSequenceWarmupStats(),
  });

  factory VesperPlaybackSequenceSnapshot.fromMap(Map<Object?, Object?> map) {
    final rawItems = map['items'];
    final items = rawItems is Iterable
        ? rawItems
            .whereType<Map>()
            .map((item) => VesperPlaybackSequenceItemState.fromMap(
                  Map<Object?, Object?>.from(item),
                ))
            .toList(growable: false)
        : const <VesperPlaybackSequenceItemState>[];
    return VesperPlaybackSequenceSnapshot(
      sequenceId: map['sequenceId'] as String? ?? '',
      sessionGeneration: _asInt(map['sessionGeneration']),
      activationEpoch: _asInt(map['activationEpoch']),
      items: items,
      activeItemId: map['activeItemId'] as String?,
      pendingRequests: _decodeMapList(map['pendingRequests']),
      requestFailures: _decodeMapList(map['requestFailures']),
      previousEndReached: map['previousEndReached'] == true,
      nextEndReached: map['nextEndReached'] == true,
      droppedEvents: _asInt(map['droppedEvents']),
      warmupTasks: _decodeMapList(map['warmupTasks'])
          .map(VesperPlaybackSequenceWarmupTask.fromMap)
          .toList(growable: false),
      warmupStats: VesperPlaybackSequenceWarmupStats.fromMap(
        vesperDecodeMap(map['warmupStats']),
      ),
    );
  }

  final String sequenceId;
  final int sessionGeneration;
  final int activationEpoch;
  final List<VesperPlaybackSequenceItemState> items;
  final String? activeItemId;
  final List<Map<String, Object?>> pendingRequests;
  final List<Map<String, Object?>> requestFailures;
  final bool previousEndReached;
  final bool nextEndReached;
  final int droppedEvents;
  final List<VesperPlaybackSequenceWarmupTask> warmupTasks;
  final VesperPlaybackSequenceWarmupStats warmupStats;
}

final class VesperPlaybackSequenceWarmupTask {
  const VesperPlaybackSequenceWarmupTask({
    required this.taskId,
    required this.itemId,
    required this.sourceRevision,
    required this.warmupGoal,
    required this.status,
    required this.expectedBytes,
    required this.actualBytes,
    required this.cacheHit,
    required this.cacheEntries,
    required this.cacheBytes,
    required this.evictedEntries,
    required this.reasonCode,
  });

  factory VesperPlaybackSequenceWarmupTask.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperPlaybackSequenceWarmupTask(
      taskId: _asInt(map['taskId']),
      itemId: map['itemId'] as String? ?? '',
      sourceRevision: _asInt(map['sourceRevision']),
      warmupGoal: map['warmupGoal'] as String? ?? 'unknown',
      status: map['status'] as String? ?? 'unknown',
      expectedBytes: _asInt(map['expectedBytes']),
      actualBytes: _asInt(map['actualBytes']),
      cacheHit: map['cacheHit'] as bool?,
      cacheEntries: _asInt(map['cacheEntries']),
      cacheBytes: _asInt(map['cacheBytes']),
      evictedEntries: _asInt(map['evictedEntries']),
      reasonCode: map['reasonCode'] as String?,
    );
  }

  final int taskId;
  final String itemId;
  final int sourceRevision;
  final String warmupGoal;
  final String status;
  final int expectedBytes;
  final int actualBytes;
  final bool? cacheHit;
  final int cacheEntries;
  final int cacheBytes;
  final int evictedEntries;
  final String? reasonCode;
}

final class VesperPlaybackSequenceWarmupStats {
  const VesperPlaybackSequenceWarmupStats({
    this.started = 0,
    this.completed = 0,
    this.cancelled = 0,
    this.failed = 0,
    this.unsupported = 0,
    this.cacheHits = 0,
    this.cacheMisses = 0,
    this.expectedBytes = 0,
    this.actualBytes = 0,
    this.evictedEntries = 0,
  });

  factory VesperPlaybackSequenceWarmupStats.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperPlaybackSequenceWarmupStats(
      started: _asInt(map['started']),
      completed: _asInt(map['completed']),
      cancelled: _asInt(map['cancelled']),
      failed: _asInt(map['failed']),
      unsupported: _asInt(map['unsupported']),
      cacheHits: _asInt(map['cacheHits']),
      cacheMisses: _asInt(map['cacheMisses']),
      expectedBytes: _asInt(map['expectedBytes']),
      actualBytes: _asInt(map['actualBytes']),
      evictedEntries: _asInt(map['evictedEntries']),
    );
  }

  final int started;
  final int completed;
  final int cancelled;
  final int failed;
  final int unsupported;
  final int cacheHits;
  final int cacheMisses;
  final int expectedBytes;
  final int actualBytes;
  final int evictedEntries;
}

final class VesperItemsRequested {
  const VesperItemsRequested({
    required this.sequenceId,
    required this.sessionGeneration,
    required this.requestId,
    required this.direction,
    required this.anchorItemId,
    required this.maxCount,
    required this.deadline,
  });

  factory VesperItemsRequested.fromMap(Map<Object?, Object?> map) {
    return VesperItemsRequested(
      sequenceId: map['sequenceId'] as String? ?? '',
      sessionGeneration: _asInt(map['sessionGeneration']),
      requestId: _asInt(map['requestId']),
      direction: _decodeEnum(
        VesperPlaybackSequenceDirection.values,
        map['direction'],
        VesperPlaybackSequenceDirection.next,
      ),
      anchorItemId: map['anchorItemId'] as String?,
      maxCount: _asInt(map['maxCount']),
      deadline: _asInt(map['deadline'] ?? map['deadlineRemainingMs']),
    );
  }

  final String sequenceId;
  final int sessionGeneration;
  final int requestId;
  final VesperPlaybackSequenceDirection direction;
  final String? anchorItemId;
  final int maxCount;
  final int deadline;
}

final class VesperSourceResolutionRequired {
  const VesperSourceResolutionRequired({
    required this.sequenceId,
    required this.sessionGeneration,
    required this.requestId,
    required this.resolutionAttemptId,
    required this.itemId,
    required this.expectedSourceRevision,
    required this.reason,
    required this.deadline,
  });

  factory VesperSourceResolutionRequired.fromMap(Map<Object?, Object?> map) {
    return VesperSourceResolutionRequired(
      sequenceId: map['sequenceId'] as String? ?? '',
      sessionGeneration: _asInt(map['sessionGeneration']),
      requestId: _asInt(map['requestId']),
      resolutionAttemptId: _asInt(map['resolutionAttemptId']),
      itemId: map['itemId'] as String? ?? '',
      expectedSourceRevision: _asInt(map['expectedSourceRevision']),
      reason: map['reason'] as String? ?? 'unresolved',
      deadline: _asInt(map['deadline'] ?? map['deadlineRemainingMs']),
    );
  }

  final String sequenceId;
  final int sessionGeneration;
  final int requestId;
  final int resolutionAttemptId;
  final String itemId;
  final int expectedSourceRevision;
  final String reason;
  final int deadline;
}

final class VesperPlaybackSequencePage {
  const VesperPlaybackSequencePage({
    this.items = const <VesperPlaybackSequenceItem>[],
    this.endReached = false,
  });

  final List<VesperPlaybackSequenceItem> items;
  final bool endReached;
}

final class VesperResolvedSource {
  const VesperResolvedSource({
    required this.itemId,
    required this.expectedSourceRevision,
    required this.sourceRevision,
    required this.source,
    required this.cacheIdentity,
    this.expiresAtEpochMs,
  });

  final String itemId;
  final int expectedSourceRevision;
  final int sourceRevision;
  final VesperPlayerSource source;
  final VesperPlaybackSequenceCacheIdentity cacheIdentity;
  final int? expiresAtEpochMs;
}

abstract interface class VesperPlaybackSequenceProvider {
  Future<VesperPlaybackSequencePage> loadItems(VesperItemsRequested request);

  Future<VesperResolvedSource> resolveSource(
    VesperSourceResolutionRequired request,
  );
}

sealed class VesperPlaybackSequenceEvent {
  const VesperPlaybackSequenceEvent({
    required this.sequenceId,
    required this.sessionGeneration,
  });

  factory VesperPlaybackSequenceEvent.fromMap(Map<Object?, Object?> map) {
    final type = map['type'] as String? ?? '';
    final sequenceId = map['sequenceId'] as String? ?? '';
    final generation = _asInt(map['sessionGeneration']);
    final payload = vesperDecodeMap(map['event']);
    final body = payload.isEmpty ? map : payload;
    final eventType = type == 'event' ? body['type'] as String? ?? type : type;
    switch (eventType) {
      case 'itemsRequested':
        return VesperPlaybackSequenceItemsRequestedEvent(
          sequenceId: sequenceId,
          sessionGeneration: generation,
          request: VesperItemsRequested.fromMap(body),
        );
      case 'sourceResolutionRequired':
        return VesperPlaybackSequenceSourceResolutionRequiredEvent(
          sequenceId: sequenceId,
          sessionGeneration: generation,
          request: VesperSourceResolutionRequired.fromMap(body),
        );
      case 'snapshot':
        return VesperPlaybackSequenceSnapshotEvent(
          sequenceId: sequenceId,
          sessionGeneration: generation,
          snapshot: VesperPlaybackSequenceSnapshot.fromMap(
            vesperDecodeMap(body['snapshot']),
          ),
        );
      default:
        return VesperPlaybackSequenceUnknownEvent(
          sequenceId: sequenceId,
          sessionGeneration: generation,
          type: eventType,
          payload: vesperDecodeMap(map),
        );
    }
  }

  final String sequenceId;
  final int sessionGeneration;
}

final class VesperPlaybackSequenceItemsRequestedEvent
    extends VesperPlaybackSequenceEvent {
  const VesperPlaybackSequenceItemsRequestedEvent({
    required super.sequenceId,
    required super.sessionGeneration,
    required this.request,
  });

  final VesperItemsRequested request;
}

final class VesperPlaybackSequenceSourceResolutionRequiredEvent
    extends VesperPlaybackSequenceEvent {
  const VesperPlaybackSequenceSourceResolutionRequiredEvent({
    required super.sequenceId,
    required super.sessionGeneration,
    required this.request,
  });

  final VesperSourceResolutionRequired request;
}

final class VesperPlaybackSequenceSnapshotEvent
    extends VesperPlaybackSequenceEvent {
  const VesperPlaybackSequenceSnapshotEvent({
    required super.sequenceId,
    required super.sessionGeneration,
    required this.snapshot,
  });

  final VesperPlaybackSequenceSnapshot snapshot;
}

final class VesperPlaybackSequenceUnknownEvent
    extends VesperPlaybackSequenceEvent {
  const VesperPlaybackSequenceUnknownEvent({
    required super.sequenceId,
    required super.sessionGeneration,
    required this.type,
    required this.payload,
  });

  final String type;
  final Map<String, Object?> payload;
}

int _asInt(Object? raw) => raw is num && raw.isFinite ? raw.toInt() : 0;

int? _asNullableInt(Object? raw) =>
    raw is num && raw.isFinite ? raw.toInt() : null;

List<Map<String, Object?>> _decodeMapList(Object? raw) {
  if (raw is! Iterable) return const <Map<String, Object?>>[];
  return raw
      .whereType<Map>()
      .map((value) => vesperDecodeMap(value))
      .toList(growable: false);
}

T _decodeEnum<T extends Enum>(List<T> values, Object? raw, T fallback) {
  if (raw is String) {
    for (final value in values) {
      if (value.name == raw) return value;
    }
  }
  return fallback;
}
