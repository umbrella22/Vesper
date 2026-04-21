import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('live dvr timeline helpers fall back to seekable window end', () {
    const timeline = VesperTimeline(
      kind: VesperTimelineKind.liveDvr,
      isSeekable: true,
      seekableRange: VesperSeekableRange(startMs: 30000, endMs: 120000),
      liveEdgeMs: null,
      positionMs: 90000,
      durationMs: null,
    );

    expect(timeline.goLivePositionMs, 120000);
    expect(timeline.liveOffsetMs, 30000);
    expect(timeline.displayedRatio, closeTo(2 / 3, 0.0001));
    expect(timeline.positionForRatio(1.5), 120000);
  });

  test('timeline helpers clamp positions and live edge tolerance', () {
    const timeline = VesperTimeline(
      kind: VesperTimelineKind.liveDvr,
      isSeekable: true,
      seekableRange: VesperSeekableRange(startMs: 30000, endMs: 120000),
      liveEdgeMs: 120000,
      positionMs: 118800,
      durationMs: null,
    );

    expect(timeline.clampedPosition(10000), 30000);
    expect(timeline.clampedPosition(150000), 120000);
    expect(timeline.positionForRatio(-0.25), 30000);
    expect(timeline.positionForRatio(0.5), 75000);
    expect(timeline.isAtLiveEdge(), isTrue);
    expect(timeline.isAtLiveEdge(toleranceMs: 500), isFalse);
  });

  test('live dvr helpers clamp stale positions after window shrink', () {
    const timeline = VesperTimeline(
      kind: VesperTimelineKind.liveDvr,
      isSeekable: true,
      seekableRange: VesperSeekableRange(startMs: 60000, endMs: 100000),
      liveEdgeMs: 100000,
      positionMs: 120000,
      durationMs: null,
    );

    expect(timeline.clampedPosition(timeline.positionMs), 100000);
    expect(timeline.liveOffsetMs, 0);
    expect(timeline.displayedRatio, 1.0);
    expect(timeline.isAtLiveEdge(), isTrue);
  });

  test('vod timeline helpers fall back to duration bounds', () {
    const timeline = VesperTimeline(
      kind: VesperTimelineKind.vod,
      isSeekable: true,
      seekableRange: null,
      liveEdgeMs: null,
      positionMs: 50000,
      durationMs: 200000,
    );

    expect(timeline.goLivePositionMs, isNull);
    expect(timeline.liveOffsetMs, isNull);
    expect(timeline.clampedPosition(-100), 0);
    expect(timeline.clampedPosition(250000), 200000);
    expect(timeline.positionForRatio(0.25), 50000);
    expect(timeline.displayedRatio, 0.25);
    expect(timeline.isAtLiveEdge(), isFalse);
  });

  test(
      'legacy coarse capability maps stay conservative for fine-grained fields',
      () {
    final capabilities = VesperPlayerCapabilities.fromMap(<Object?, Object?>{
      'supportsTrackSelection': true,
      'supportsAbrPolicy': true,
    });

    expect(capabilities.supportsTrackSelection, isTrue);
    expect(capabilities.supportsVideoTrackSelection, isFalse);
    expect(capabilities.supportsAudioTrackSelection, isFalse);
    expect(capabilities.supportsSubtitleTrackSelection, isFalse);
    expect(capabilities.supportsAbrPolicy, isTrue);
    expect(capabilities.supportsAbrConstrained, isFalse);
    expect(capabilities.supportsAbrFixedTrack, isFalse);
    expect(capabilities.supportsAbrMaxBitRate, isFalse);
    expect(capabilities.supportsAbrMaxResolution, isFalse);
    expect(
      capabilities.supportsTrackSelectionFor(VesperMediaTrackKind.audio),
      isFalse,
    );
    expect(capabilities.supportsAbrMode(VesperAbrMode.auto), isTrue);
    expect(capabilities.supportsAbrMode(VesperAbrMode.fixedTrack), isFalse);
  });

  test('capabilities decode partial iOS ABR and track-selection support', () {
    final capabilities = VesperPlayerCapabilities.fromMap(<Object?, Object?>{
      'supportsTrackSelection': true,
      'supportsVideoTrackSelection': false,
      'supportsAudioTrackSelection': true,
      'supportsSubtitleTrackSelection': true,
      'supportsAbrPolicy': true,
      'supportsAbrConstrained': true,
      'supportsAbrFixedTrack': false,
      'supportsAbrMaxBitRate': true,
      'supportsAbrMaxResolution': true,
    });

    expect(capabilities.supportsTrackSelection, isTrue);
    expect(capabilities.supportsVideoTrackSelection, isFalse);
    expect(capabilities.supportsAudioTrackSelection, isTrue);
    expect(capabilities.supportsSubtitleTrackSelection, isTrue);
    expect(
      capabilities.supportsTrackSelectionFor(VesperMediaTrackKind.video),
      isFalse,
    );
    expect(
      capabilities.supportsTrackSelectionFor(VesperMediaTrackKind.subtitle),
      isTrue,
    );
    expect(capabilities.supportsAbrPolicy, isTrue);
    expect(capabilities.supportsAbrConstrained, isTrue);
    expect(capabilities.supportsAbrFixedTrack, isFalse);
    expect(capabilities.supportsAbrMode(VesperAbrMode.constrained), isTrue);
    expect(capabilities.supportsAbrMode(VesperAbrMode.fixedTrack), isFalse);
    expect(capabilities.toMap()['supportsAbrFixedTrack'], isFalse);
  });

  test(
    'default retry policy keeps fallback getters but omits channel overrides',
    () {
      const policy = VesperRetryPolicy();

      expect(policy.maxAttempts, 3);
      expect(policy.baseDelayMs, 1000);
      expect(policy.maxDelayMs, 5000);
      expect(policy.backoff, VesperRetryBackoff.linear);
      expect(policy.toMap(), <String, Object?>{
        'baseDelayMs': null,
        'maxDelayMs': null,
        'backoff': null,
      });
    },
  );

  test('retry policy can encode explicit unlimited retries', () {
    const policy = VesperRetryPolicy(maxAttempts: null);

    expect(policy.maxAttempts, isNull);
    expect(policy.toMap(), <String, Object?>{
      'maxAttempts': null,
      'baseDelayMs': null,
      'maxDelayMs': null,
      'backoff': null,
    });
  });

  test('retry policy fromMap keeps explicit overrides only', () {
    final policy = VesperRetryPolicy.fromMap(<Object?, Object?>{
      'maxAttempts': 6,
      'maxDelayMs': 8000,
      'backoff': 'exponential',
    });

    expect(policy.maxAttempts, 6);
    expect(policy.baseDelayMs, 1000);
    expect(policy.maxDelayMs, 8000);
    expect(policy.backoff, VesperRetryBackoff.exponential);
    expect(policy.toMap(), <String, Object?>{
      'maxAttempts': 6,
      'baseDelayMs': null,
      'maxDelayMs': 8000,
      'backoff': 'exponential',
    });
  });

  test('retry policy fromMap preserves explicit unlimited retries', () {
    final policy = VesperRetryPolicy.fromMap(<Object?, Object?>{
      'maxAttempts': null,
      'baseDelayMs': 1500,
    });

    expect(policy.maxAttempts, isNull);
    expect(policy.baseDelayMs, 1500);
    expect(policy.toMap(), <String, Object?>{
      'maxAttempts': null,
      'baseDelayMs': 1500,
      'maxDelayMs': null,
      'backoff': null,
    });
  });

  test('buffering preset constructors only serialize preset names', () {
    expect(const VesperBufferingPolicy.resilient().toMap(), <String, Object?>{
      'preset': 'resilient',
      'minBufferMs': null,
      'maxBufferMs': null,
      'bufferForPlaybackMs': null,
      'bufferForPlaybackAfterRebufferMs': null,
    });
  });

  test('cache preset constructors only serialize preset names', () {
    expect(const VesperCachePolicy.streaming().toMap(), <String, Object?>{
      'preset': 'streaming',
      'maxMemoryBytes': null,
      'maxDiskBytes': null,
    });
  });

  test(
      'resilience preset serialization keeps shared values out of buffering/cache',
      () {
    expect(
      const VesperPlaybackResiliencePolicy.resilient().toMap(),
      <String, Object?>{
        'buffering': <String, Object?>{
          'preset': 'resilient',
          'minBufferMs': null,
          'maxBufferMs': null,
          'bufferForPlaybackMs': null,
          'bufferForPlaybackAfterRebufferMs': null,
        },
        'retry': <String, Object?>{
          'maxAttempts': 6,
          'baseDelayMs': 1000,
          'maxDelayMs': 8000,
          'backoff': 'exponential',
        },
        'cache': <String, Object?>{
          'preset': 'resilient',
          'maxMemoryBytes': null,
          'maxDiskBytes': null,
        },
      },
    );
  });

  test('track preference policy serializes sparse overrides only', () {
    const policy = VesperTrackPreferencePolicy(
      preferredAudioLanguage: 'ja',
      selectSubtitlesByDefault: true,
      subtitleSelection: VesperTrackSelection.track('subtitle:zh-Hans'),
      abrPolicy: VesperAbrPolicy.constrained(maxBitRate: 3500000),
    );

    expect(policy.toMap(), <String, Object?>{
      'preferredAudioLanguage': 'ja',
      'selectSubtitlesByDefault': true,
      'subtitleSelection': <String, Object?>{
        'mode': 'track',
        'trackId': 'subtitle:zh-Hans',
      },
      'abrPolicy': <String, Object?>{
        'mode': 'constrained',
        'trackId': null,
        'maxBitRate': 3500000,
        'maxWidth': null,
        'maxHeight': null,
      },
    });
  });

  test('track preference policy fromMap restores explicit values', () {
    final policy = VesperTrackPreferencePolicy.fromMap(<Object?, Object?>{
      'preferredSubtitleLanguage': 'en-US',
      'selectUndeterminedSubtitleLanguage': true,
      'audioSelection': <Object?, Object?>{
        'mode': 'track',
        'trackId': 'audio:ja-main',
      },
    });

    expect(policy.preferredSubtitleLanguage, 'en-US');
    expect(policy.selectUndeterminedSubtitleLanguage, isTrue);
    expect(policy.audioSelection.mode, VesperTrackSelectionMode.track);
    expect(policy.audioSelection.trackId, 'audio:ja-main');
    expect(policy.subtitleSelection.mode, VesperTrackSelectionMode.disabled);
    expect(policy.abrPolicy.mode, VesperAbrMode.auto);
  });

  test('preload budget policy serializes sparse overrides only', () {
    const policy = VesperPreloadBudgetPolicy(
      maxConcurrentTasks: 2,
      maxDiskBytes: 268435456,
    );

    expect(policy.toMap(), <String, Object?>{
      'maxConcurrentTasks': 2,
      'maxDiskBytes': 268435456,
    });
  });

  test('preload budget policy fromMap restores explicit values', () {
    final policy = VesperPreloadBudgetPolicy.fromMap(<Object?, Object?>{
      'maxMemoryBytes': 67108864,
      'warmupWindowMs': 30000,
    });

    expect(policy.maxConcurrentTasks, isNull);
    expect(policy.maxMemoryBytes, 67108864);
    expect(policy.maxDiskBytes, isNull);
    expect(policy.warmupWindowMs, 30000);
  });

  test('viewport hint classification follows visible near prefetch bands', () {
    const visibleViewport = VesperPlayerViewport(
      left: 0,
      top: 100,
      width: 200,
      height: 120,
    );
    const nearViewport = VesperPlayerViewport(
      left: 0,
      top: 860,
      width: 200,
      height: 120,
    );
    const prefetchViewport = VesperPlayerViewport(
      left: 0,
      top: 1500,
      width: 200,
      height: 120,
    );
    const hiddenViewport = VesperPlayerViewport(
      left: 0,
      top: 2400,
      width: 200,
      height: 120,
    );

    expect(
      visibleViewport.classifyHint(surfaceWidth: 400, surfaceHeight: 800).kind,
      VesperViewportHintKind.visible,
    );
    expect(
      nearViewport.classifyHint(surfaceWidth: 400, surfaceHeight: 800).kind,
      VesperViewportHintKind.nearVisible,
    );
    expect(
      prefetchViewport.classifyHint(surfaceWidth: 400, surfaceHeight: 800).kind,
      VesperViewportHintKind.prefetchOnly,
    );
    expect(
      hiddenViewport.classifyHint(surfaceWidth: 400, surfaceHeight: 800).kind,
      VesperViewportHintKind.hidden,
    );
  });

  test('player snapshot decodes viewport shared semantics', () {
    const viewport = VesperPlayerViewport(
      left: 12,
      top: 34,
      width: 200,
      height: 120,
    );
    const viewportHint = VesperViewportHint(
      kind: VesperViewportHintKind.visible,
      visibleFraction: 0.75,
    );

    final snapshot = VesperPlayerSnapshot.fromMap(<Object?, Object?>{
      'title': 'Demo',
      'subtitle': 'Viewport',
      'sourceLabel': 'feed://demo',
      'playbackState': 'playing',
      'playbackRate': 1.0,
      'isBuffering': false,
      'isInterrupted': false,
      'hasVideoSurface': true,
      'timeline': const VesperTimeline.initial().toMap(),
      'viewport': viewport.toMap(),
      'viewportHint': viewportHint.toMap(),
    });

    expect(snapshot.viewport?.left, 12);
    expect(snapshot.viewport?.height, 120);
    expect(snapshot.viewportHint.kind, VesperViewportHintKind.visible);
    expect(snapshot.viewportHint.visibleFraction, 0.75);
  });

  test('player snapshot decodes host lastError shared semantics', () {
    final snapshot = VesperPlayerSnapshot.fromMap(<Object?, Object?>{
      'title': 'Demo',
      'subtitle': 'Unsupported',
      'sourceLabel': 'feed://demo',
      'playbackState': 'ready',
      'playbackRate': 1.0,
      'isBuffering': false,
      'isInterrupted': false,
      'hasVideoSurface': false,
      'timeline': const VesperTimeline.initial().toMap(),
      'lastError': <Object?, Object?>{
        'message': 'setVideoTrackSelection is not implemented on iOS AVPlayer.',
        'category': 'unsupported',
        'retriable': false,
      },
    });

    expect(
      snapshot.lastError?.message,
      'setVideoTrackSelection is not implemented on iOS AVPlayer.',
    );
    expect(
      snapshot.lastError?.category,
      VesperPlayerErrorCategory.unsupported,
    );
    expect(snapshot.lastError?.retriable, isFalse);
  });

  test('player snapshot decodes resilience policy shared semantics', () {
    final snapshot = VesperPlayerSnapshot.fromMap(<Object?, Object?>{
      'title': 'Demo',
      'subtitle': 'Resilience',
      'sourceLabel': 'feed://demo',
      'playbackState': 'ready',
      'playbackRate': 1.0,
      'isBuffering': false,
      'isInterrupted': false,
      'hasVideoSurface': false,
      'timeline': const VesperTimeline.initial().toMap(),
      'effectiveVideoTrackId': 'video:hls:cavc1:b1500000:w1280:h720:f3000',
      'fixedTrackStatus': 'fallback',
      'resiliencePolicy':
          const VesperPlaybackResiliencePolicy.resilient().toMap(),
    });

    expect(
      snapshot.effectiveVideoTrackId,
      'video:hls:cavc1:b1500000:w1280:h720:f3000',
    );
    expect(snapshot.fixedTrackStatus, VesperFixedTrackStatus.fallback);
    expect(snapshot.resiliencePolicy.buffering.preset,
        VesperBufferingPreset.resilient);
    expect(snapshot.resiliencePolicy.retry.maxAttempts, 6);
    expect(snapshot.resiliencePolicy.cache.preset, VesperCachePreset.resilient);
  });

  test('player snapshot event decodes resilience policy shared semantics', () {
    final event = VesperPlayerEvent.fromMap(<Object?, Object?>{
      'type': 'snapshot',
      'playerId': 'ios-player',
      'snapshot': <Object?, Object?>{
        'title': 'Demo',
        'subtitle': 'Event resilience',
        'sourceLabel': 'feed://demo',
        'playbackState': 'playing',
        'playbackRate': 1.0,
        'isBuffering': false,
        'isInterrupted': false,
        'hasVideoSurface': true,
        'timeline': const VesperTimeline.initial().toMap(),
        'effectiveVideoTrackId': 'video:hls:cavc1:b2500000:w1920:h1080:f2997',
        'fixedTrackStatus': 'locked',
        'resiliencePolicy':
            const VesperPlaybackResiliencePolicy.streaming().toMap(),
      },
    });

    expect(event, isA<VesperPlayerSnapshotEvent>());
    expect(event.playerId, 'ios-player');
    final snapshot = (event as VesperPlayerSnapshotEvent).snapshot;
    expect(snapshot.playbackState, VesperPlaybackState.playing);
    expect(
      snapshot.effectiveVideoTrackId,
      'video:hls:cavc1:b2500000:w1920:h1080:f2997',
    );
    expect(snapshot.fixedTrackStatus, VesperFixedTrackStatus.locked);
    expect(snapshot.resiliencePolicy.buffering.preset,
        VesperBufferingPreset.streaming);
    expect(snapshot.resiliencePolicy.cache.preset, VesperCachePreset.streaming);
  });
}
