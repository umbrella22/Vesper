import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('shared player error contract decodes stable fields and details', () {
    final payload = _readContractMap('player_error.json');
    final error = VesperPlayerError.fromMap(payload);

    expect(error.message, 'fixture unsupported capability');
    expect(error.code, VesperPlayerErrorCode.unsupported);
    expect(error.category, VesperPlayerErrorCategory.capability);
    expect(error.retriable, isFalse);
    expect(error.details['operation'], 'setAbrPolicy');
    expect(error.details['trackId'], 'video:missing');
    expect(error.toMap()['code'], payload['code']);
    expect(error.toMap()['category'], payload['category']);
  });

  test('shared download task contract decodes stable fields', () {
    final payload = _readContractMap('download_task_snapshot.json');
    final task = VesperDownloadTaskSnapshot.fromMap(payload);

    expect(task.taskId, 42);
    expect(task.assetId, 'asset-contract');
    expect(task.source.source.protocol, VesperPlayerSourceProtocol.dash);
    expect(task.source.contentFormat, VesperDownloadContentFormat.dashSegments);
    expect(task.profile.targetOutputFormat, VesperDownloadOutputFormat.mp4);
    expect(task.profile.selectedTrackIds, <String>['video:1080p', 'audio:ja']);
    expect(task.state, VesperDownloadState.downloading);
    expect(task.progress.receivedBytes, 2048);
    expect(task.assetIndex.resources.single.resourceId, 'manifest');
    expect(task.assetIndex.segments.single.byteRange?.offset, 128);
    expect(task.assetIndex.streams.single.metadata['bandwidth'], '2400000');
    expect(task.error, isNull);
    expect(task.toMap()['state'], payload['state']);
  });

  test('shared system playback contract decodes stable fields', () {
    final payload = _readContractMap('system_playback_configuration.json');
    final configuration = VesperSystemPlaybackConfiguration.fromMap(payload);

    expect(configuration.enabled, isTrue);
    expect(configuration.backgroundMode,
        VesperBackgroundPlaybackMode.continueAudio);
    expect(configuration.metadata?.title, 'Contract Episode');
    expect(configuration.metadata?.isLive, isTrue);
    final controls = configuration.controls!;
    expect(
        controls.compactButtons.map((button) => button.kind),
        <VesperSystemPlaybackControlKind>[
          VesperSystemPlaybackControlKind.seekBack,
          VesperSystemPlaybackControlKind.playPause,
          VesperSystemPlaybackControlKind.seekForward,
        ]);
    expect(controls.toMap()['compactButtons'], <Object?>[
      <String, Object?>{'kind': 'seekBack', 'seekOffsetMs': 10000},
      <String, Object?>{'kind': 'playPause'},
      <String, Object?>{'kind': 'seekForward', 'seekOffsetMs': 10000},
    ]);
    expect(configuration.toMap()['backgroundMode'], payload['backgroundMode']);
  });

  test('render surface kind wire names stay stable', () {
    expect(VesperPlayerRenderSurfaceKind.auto.name, 'auto');
    expect(VesperPlayerRenderSurfaceKind.textureView.name, 'textureView');
    expect(VesperPlayerRenderSurfaceKind.surfaceView.name, 'surfaceView');
  });

  test('download DTOs encode FLV byte ranges and target output', () {
    const source = VesperDownloadSource(
      source: VesperPlayerSource(
        uri: 'https://example.com/video.flv',
        label: 'FLV demo',
        kind: VesperPlayerSourceKind.remote,
        protocol: VesperPlayerSourceProtocol.progressive,
      ),
      contentFormat: VesperDownloadContentFormat.flvSegments,
      manifestUri: 'https://example.com/video.flv',
    );
    const profile = VesperDownloadProfile(
      targetOutputFormat: VesperDownloadOutputFormat.mp4,
      targetDirectory: '/tmp/vesper-downloads/demo',
    );
    const assetIndex = VesperDownloadAssetIndex(
      contentFormat: VesperDownloadContentFormat.flvSegments,
      totalSizeBytes: 4096,
      resources: <VesperDownloadResourceRecord>[
        VesperDownloadResourceRecord(
          resourceId: 'flv-concat',
          uri: 'vesper-generated://flv/manifest.ffconcat',
          relativePath: 'manifest.ffconcat',
          generatedText: 'ffconcat version 1.0\n',
        ),
      ],
      segments: <VesperDownloadSegmentRecord>[
        VesperDownloadSegmentRecord(
          segmentId: 'clip-1',
          uri: 'https://example.com/video.flv',
          relativePath: 'clips/clip-00001.flv',
          sequence: 1,
          byteRange: VesperDownloadByteRange(offset: 128, length: 4096),
          sizeBytes: 4096,
        ),
      ],
    );

    final task = VesperDownloadTaskSnapshot.fromMap(
      VesperDownloadTaskSnapshot(
        taskId: 7,
        assetId: 'asset-flv',
        source: source,
        profile: profile,
        state: VesperDownloadState.preparing,
        progress: const VesperDownloadProgressSnapshot(
          totalBytes: 4096,
          totalSegments: 1,
        ),
        assetIndex: assetIndex,
      ).toMap(),
    );

    expect(source.toMap()['contentFormat'], 'flvSegments');
    expect(profile.toMap()['targetOutputFormat'], 'mp4');
    expect(task.assetIndex.totalSizeBytes, 4096);
    expect(task.assetIndex.segments.single.byteRange?.offset, 128);
    expect(task.assetIndex.segments.single.byteRange?.length, 4096);
  });

  test('download error requires typed code and category strings', () {
    final error = VesperDownloadError.fromMap(<Object?, Object?>{
      'code': 'backendFailure',
      'category': 'network',
      'retriable': true,
      'message': 'download failed',
    });

    expect(error.code, VesperPlayerErrorCode.backendFailure);
    expect(error.category, VesperPlayerErrorCategory.network);
    expect(error.retriable, isTrue);
    expect(error.message, 'download failed');
    expect(error.toMap(), <String, Object?>{
      'code': 'backendFailure',
      'category': 'network',
      'retriable': true,
      'message': 'download failed',
    });

    expect(
      () => VesperDownloadError.fromMap(<Object?, Object?>{
        'category': 'network',
        'message': 'missing code',
      }),
      throwsA(isA<FormatException>()),
    );
    expect(
      () => VesperDownloadError.fromMap(<Object?, Object?>{
        'code': 3,
        'category': 'network',
        'message': 'non-string code',
      }),
      throwsA(isA<FormatException>()),
    );
    expect(
      () => VesperDownloadError.fromMap(<Object?, Object?>{
        'code': 'backendFailure',
        'category': 'doesNotExist',
      }),
      throwsA(isA<FormatException>()),
    );
  });

  test('system playback DTOs keep stable defaults and wire names', () {
    const metadata = VesperSystemPlaybackMetadata(
      title: 'Episode 1',
      artist: 'Vesper',
      albumTitle: 'SDK Samples',
      artworkUri: 'https://example.com/artwork.jpg',
      contentUri: 'https://example.com/video.m3u8',
      durationMs: 60000,
      isLive: true,
    );
    const configuration = VesperSystemPlaybackConfiguration(
      metadata: metadata,
    );

    expect(VesperBackgroundPlaybackMode.disabled.name, 'disabled');
    expect(VesperBackgroundPlaybackMode.continueAudio.name, 'continueAudio');
    expect(
        VesperSystemPlaybackPermissionStatus.notRequired.name, 'notRequired');
    expect(configuration.toMap(), <String, Object?>{
      'enabled': true,
      'backgroundMode': 'continueAudio',
      'showSystemControls': true,
      'showSeekActions': true,
      'metadata': metadata.toMap(),
      'controls': const VesperSystemPlaybackControls.videoDefault().toMap(),
    });
    expect(
      VesperSystemPlaybackConfiguration.fromMap(configuration.toMap())
          .metadata
          ?.title,
      'Episode 1',
    );
  });

  test('system playback DTOs decode legacy sparse maps', () {
    final configuration = VesperSystemPlaybackConfiguration.fromMap(
      <Object?, Object?>{
        'metadata': <Object?, Object?>{'title': 'Sparse'},
      },
    );

    expect(configuration.enabled, isTrue);
    expect(configuration.backgroundMode,
        VesperBackgroundPlaybackMode.continueAudio);
    expect(configuration.showSystemControls, isTrue);
    expect(configuration.showSeekActions, isTrue);
    expect(configuration.metadata?.title, 'Sparse');
    expect(configuration.metadata?.isLive, isFalse);
    expect(
      configuration.toMap()['controls'],
      const VesperSystemPlaybackControls.videoDefault().toMap(),
    );
  });

  test('system playback controls normalize seek offsets and disabled seek', () {
    final decoded = VesperSystemPlaybackConfiguration.fromMap(
      <Object?, Object?>{
        'controls': <Object?, Object?>{
          'compactButtons': <Object?>[
            <Object?, Object?>{'kind': 'seekBack', 'seekOffsetMs': 5000},
            <Object?, Object?>{'kind': 'playPause'},
            <Object?, Object?>{'kind': 'seekForward', 'seekOffsetMs': 15000},
          ],
        },
      },
    );

    expect(
      decoded.toMap()['controls'],
      <String, Object?>{
        'compactButtons': <Object?>[
          <String, Object?>{'kind': 'seekBack', 'seekOffsetMs': 5000},
          <String, Object?>{'kind': 'playPause'},
          <String, Object?>{'kind': 'seekForward', 'seekOffsetMs': 15000},
        ],
      },
    );

    const configuration = VesperSystemPlaybackConfiguration(
      controls: VesperSystemPlaybackControls(
        compactButtons: <VesperSystemPlaybackControlButton>[
          VesperSystemPlaybackControlButton.seekBack(500),
          VesperSystemPlaybackControlButton.seekForward(15000),
          VesperSystemPlaybackControlButton.seekForward(90000),
        ],
      ),
    );

    expect(
      configuration.toMap()['controls'],
      <String, Object?>{
        'compactButtons': <Object?>[
          <String, Object?>{'kind': 'seekBack', 'seekOffsetMs': 1000},
          <String, Object?>{'kind': 'playPause'},
          <String, Object?>{'kind': 'seekForward', 'seekOffsetMs': 60000},
        ],
      },
    );

    const seekDisabled = VesperSystemPlaybackConfiguration(
      showSeekActions: false,
      controls: VesperSystemPlaybackControls.videoDefault(),
    );
    expect(
      seekDisabled.toMap()['controls'],
      <String, Object?>{
        'compactButtons': <Object?>[
          <String, Object?>{'kind': 'playPause'},
        ],
      },
    );
  });

  test('external playback DTOs keep stable defaults and wire names', () {
    const route = VesperExternalPlaybackRouteSnapshot(
      kind: VesperExternalPlaybackRouteKind.cast,
      routeId: 'living-room',
      routeName: 'Living Room TV',
      active: true,
      available: true,
    );
    const availability = VesperExternalPlaybackAvailability(
      airPlayAvailable: true,
      castAvailable: true,
      activeRoute: route,
    );
    const picker = VesperRoutePickerConfiguration();
    const adaptation = VesperExternalFormatAdaptationConfig.dlnaRemux(
      preferredFallback: VesperExternalFallbackFormat.hls,
      allowRemoteDashMediaReferences: true,
      remoteDashMediaRequestHeaders: <String>{'User-Agent', 'Referer'},
      debugDiagnostics: true,
    );
    final mediaItem = VesperExternalPlaybackMediaItem(
      sources: <VesperPlayerSource>[
        VesperPlayerSource.dash(
          uri: 'https://example.com/video.mpd',
          label: 'DASH',
        ),
      ],
      metadata: const VesperSystemPlaybackMetadata(title: 'Episode'),
      formatAdaptation: adaptation,
    );

    expect(VesperExternalPlaybackRouteKind.none.name, 'none');
    expect(VesperExternalPlaybackRouteKind.airPlay.name, 'airPlay');
    expect(VesperExternalPlaybackRouteKind.cast.name, 'cast');
    expect(VesperExternalPlaybackRouteKind.dlna.name, 'dlna');
    expect(VesperExternalFallbackFormat.mpegTs.name, 'mpegTs');
    expect(VesperExternalFallbackFormat.hls.name, 'hls');
    expect(availability.hasAvailableRoute, isTrue);
    expect(
      VesperExternalPlaybackAvailability.fromMap(availability.toMap())
          .activeRoute
          .routeName,
      'Living Room TV',
    );
    expect(picker.toMap(), <String, Object?>{
      'prioritizesVideoDevices': true,
    });
    expect(
      VesperRoutePickerConfiguration.fromMap(const <Object?, Object?>{})
          .prioritizesVideoDevices,
      isTrue,
    );
    expect(
      VesperExternalPlaybackMediaItem.fromMap(mediaItem.toMap())
          .formatAdaptation
          .preferredFallback,
      VesperExternalFallbackFormat.hls,
    );
    expect(
      VesperExternalPlaybackMediaItem.fromMap(mediaItem.toMap())
          .formatAdaptation
          .allowRemoteDashMediaReferences,
      isTrue,
    );
    expect(
      VesperExternalPlaybackMediaItem.fromMap(mediaItem.toMap())
          .formatAdaptation
          .remoteDashMediaRequestHeaders,
      <String>{'User-Agent', 'Referer'},
    );
  });

  test('player source preserves request headers in wire map', () {
    final source = VesperPlayerSource.dash(
      uri: 'https://example.com/video.mpd',
      label: 'DASH',
      headers: const <String, String>{
        'Referer': 'https://www.bilibili.com/',
        'User-Agent': 'VesperTest',
      },
    );

    expect(source.toMap()['headers'], <String, String>{
      'Referer': 'https://www.bilibili.com/',
      'User-Agent': 'VesperTest',
    });

    final restored = VesperPlayerSource.fromMap(source.toMap());
    expect(restored.headers, source.headers);
    expect(restored.protocol, VesperPlayerSourceProtocol.dash);
  });

  test('local DASH source keeps local kind and DASH protocol', () {
    final source = VesperPlayerSource.localDash(
      uri: 'content://media/video/demo/manifest.mpd',
      label: 'Local DASH',
    );

    expect(source.kind, VesperPlayerSourceKind.local);
    expect(source.protocol, VesperPlayerSourceProtocol.dash);

    final restored = VesperPlayerSource.fromMap(source.toMap());
    expect(restored.kind, VesperPlayerSourceKind.local);
    expect(restored.protocol, VesperPlayerSourceProtocol.dash);
  });

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
    expect(capabilities.supportsDashStaticVod, isFalse);
    expect(capabilities.supportsDashDynamicLive, isFalse);
    expect(capabilities.supportsDashManifestTrackCatalog, isFalse);
    expect(capabilities.supportsDashTextTracks, isFalse);
    expect(capabilities.supportsExactAbrFixedTrack, isFalse);
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
      'capabilities keep iOS best-effort fixed-track separate from exact video track selection',
      () {
    final capabilities = VesperPlayerCapabilities.fromMap(<Object?, Object?>{
      'supportsTrackCatalog': true,
      'supportsTrackSelection': true,
      'supportsVideoTrackSelection': false,
      'supportsAudioTrackSelection': true,
      'supportsSubtitleTrackSelection': true,
      'supportsAbrPolicy': true,
      'supportsAbrConstrained': true,
      'supportsAbrFixedTrack': true,
      'supportsExactAbrFixedTrack': false,
      'supportsAbrMaxBitRate': true,
      'supportsAbrMaxResolution': true,
    });

    expect(capabilities.supportsTrackCatalog, isTrue);
    expect(capabilities.supportsTrackSelection, isTrue);
    expect(capabilities.supportsVideoTrackSelection, isFalse);
    expect(capabilities.supportsTrackSelectionFor(VesperMediaTrackKind.video),
        isFalse);
    expect(capabilities.supportsAbrPolicy, isTrue);
    expect(capabilities.supportsAbrConstrained, isTrue);
    expect(capabilities.supportsAbrFixedTrack, isTrue);
    expect(capabilities.supportsExactAbrFixedTrack, isFalse);
    expect(capabilities.supportsAbrMode(VesperAbrMode.fixedTrack), isTrue);
    expect(capabilities.supportsAbrMaxBitRate, isTrue);
    expect(capabilities.supportsAbrMaxResolution, isTrue);
  });

  test('capabilities expose DASH sub-capabilities and exact fixed-track', () {
    final capabilities = VesperPlayerCapabilities.fromMap(<Object?, Object?>{
      'supportsDash': true,
      'supportsDashStaticVod': true,
      'supportsDashDynamicLive': true,
      'supportsDashManifestTrackCatalog': true,
      'supportsDashTextTracks': true,
      'supportsAbrFixedTrack': true,
      'supportsExactAbrFixedTrack': true,
    });

    expect(capabilities.supportsDash, isTrue);
    expect(capabilities.supportsDashStaticVod, isTrue);
    expect(capabilities.supportsDashDynamicLive, isTrue);
    expect(capabilities.supportsDashManifestTrackCatalog, isTrue);
    expect(capabilities.supportsDashTextTracks, isTrue);
    expect(capabilities.supportsAbrFixedTrack, isTrue);
    expect(capabilities.supportsExactAbrFixedTrack, isTrue);
    expect(capabilities.toMap()['supportsDashStaticVod'], isTrue);
    expect(capabilities.toMap()['supportsExactAbrFixedTrack'], isTrue);

    final inferredDash = VesperPlayerCapabilities.fromMap(<Object?, Object?>{
      'supportsDashManifestTrackCatalog': true,
    });
    expect(inferredDash.supportsDash, isTrue);
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

  test('benchmark configuration serializes explicit console logging options',
      () {
    const configuration = VesperBenchmarkConfiguration(
      enabled: true,
      maxBufferedEvents: 512,
      includeRawEvents: false,
      consoleLogging: true,
      pluginLibraryPaths: <String>['/tmp/libvesper_benchmark_sink.dylib'],
    );

    expect(configuration.hasOverrides, isTrue);
    expect(configuration.toMap(), <String, Object?>{
      'enabled': true,
      'maxBufferedEvents': 512,
      'includeRawEvents': false,
      'consoleLogging': true,
      'pluginLibraryPaths': <String>['/tmp/libvesper_benchmark_sink.dylib'],
    });

    final restored = VesperBenchmarkConfiguration.fromMap(
      configuration.toMap(),
    );
    expect(restored.enabled, isTrue);
    expect(restored.maxBufferedEvents, 512);
    expect(restored.includeRawEvents, isFalse);
    expect(restored.consoleLogging, isTrue);
    expect(restored.pluginLibraryPaths, <String>[
      '/tmp/libvesper_benchmark_sink.dylib',
    ]);
  });

  test('disabled benchmark configuration has no channel overrides', () {
    const configuration = VesperBenchmarkConfiguration.disabled();

    expect(configuration.hasOverrides, isFalse);
    expect(configuration.toMap(), <String, Object?>{
      'enabled': false,
      'maxBufferedEvents': 2048,
      'includeRawEvents': true,
      'consoleLogging': false,
      'pluginLibraryPaths': <String>[],
    });
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
        'code': 'unsupported',
        'category': 'capability',
        'retriable': false,
      },
    });

    expect(
      snapshot.lastError?.message,
      'setVideoTrackSelection is not implemented on iOS AVPlayer.',
    );
    expect(
      snapshot.lastError?.code,
      VesperPlayerErrorCode.unsupported,
    );
    expect(snapshot.lastError?.category, VesperPlayerErrorCategory.capability);
    expect(snapshot.lastError?.retriable, isFalse);
  });

  test('player error requires typed code and category strings', () {
    final error = VesperPlayerError.fromMap(<Object?, Object?>{
      'message': 'unsupported operation',
      'code': 'unsupported',
      'category': 'capability',
      'retriable': false,
      'details': <Object?, Object?>{
        'platformCode': 'vesper_operation_failed',
        'native': true,
      },
    });

    expect(error.code, VesperPlayerErrorCode.unsupported);
    expect(error.category, VesperPlayerErrorCategory.capability);
    expect(error.message, 'unsupported operation');
    expect(error.details['platformCode'], 'vesper_operation_failed');
    expect(error.details['native'], isTrue);
    expect(error.toMap()['details'], <String, Object?>{
      'platformCode': 'vesper_operation_failed',
      'native': true,
    });

    expect(
      () => VesperPlayerError.fromMap(<Object?, Object?>{
        'message': 'missing code',
        'category': 'platform',
      }),
      throwsA(isA<FormatException>()),
    );
    expect(
      () => VesperPlayerError.fromMap(<Object?, Object?>{
        'message': 'unknown code',
        'code': 'doesNotExist',
        'category': 'platform',
      }),
      throwsA(isA<FormatException>()),
    );
    expect(
      () => VesperPlayerError.fromMap(<Object?, Object?>{
        'message': 'unknown category',
        'code': 'unsupported',
        'category': 'doesNotExist',
      }),
      throwsA(isA<FormatException>()),
    );
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
      'videoVariantObservation': <Object?, Object?>{
        'bitRate': 1420000,
        'width': 1280,
        'height': 720,
      },
      'fixedTrackStatus': 'fallback',
      'resiliencePolicy':
          const VesperPlaybackResiliencePolicy.resilient().toMap(),
    });

    expect(
      snapshot.effectiveVideoTrackId,
      'video:hls:cavc1:b1500000:w1280:h720:f3000',
    );
    expect(snapshot.videoVariantObservation?.bitRate, 1420000);
    expect(snapshot.videoVariantObservation?.width, 1280);
    expect(snapshot.videoVariantObservation?.height, 720);
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
        'videoVariantObservation': <Object?, Object?>{
          'bitRate': 2480000,
          'width': 1920,
          'height': 1080,
        },
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
    expect(snapshot.videoVariantObservation?.bitRate, 2480000);
    expect(snapshot.videoVariantObservation?.width, 1920);
    expect(snapshot.videoVariantObservation?.height, 1080);
    expect(snapshot.fixedTrackStatus, VesperFixedTrackStatus.locked);
    expect(snapshot.resiliencePolicy.buffering.preset,
        VesperBufferingPreset.streaming);
    expect(snapshot.resiliencePolicy.cache.preset, VesperCachePreset.streaming);
  });
}

Map<Object?, Object?> _readContractMap(String name) {
  final decoded = jsonDecode(_contractFile(name).readAsStringSync());
  return Map<Object?, Object?>.from(decoded as Map);
}

File _contractFile(String name) {
  return File('../../../fixtures/contracts/$name');
}
