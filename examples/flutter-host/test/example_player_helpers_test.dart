import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_host/src/example_player_helpers.dart';
import 'package:flutter_host/src/example_player_models.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  test('live dvr acceptance source is hls and exposed in examples', () {
    final source = flutterLiveDvrAcceptanceSource();

    expect(source.uri, flutterLiveDvrAcceptanceUrl);
    expect(source.protocol, VesperPlayerSourceProtocol.hls);
    expect(
      exampleSources.any(
        (candidate) =>
            candidate.uri == flutterLiveDvrAcceptanceUrl &&
            candidate.protocol == VesperPlayerSourceProtocol.hls,
      ),
      isTrue,
    );
  });

  test('go live falls back to seekable end for live dvr', () {
    const timeline = VesperTimeline(
      kind: VesperTimelineKind.liveDvr,
      isSeekable: true,
      seekableRange: VesperSeekableRange(startMs: 10000, endMs: 60000),
      liveEdgeMs: null,
      positionMs: 55000,
      durationMs: 60000,
    );

    expect(liveButtonLabel(timeline), '直播 -00:05');
    expect(timelineSummary(timeline, null), '00:45 / 00:50');
  });

  test('live edge tolerance keeps live badge active', () {
    const timeline = VesperTimeline(
      kind: VesperTimelineKind.live,
      isSeekable: false,
      positionMs: 119100,
      liveEdgeMs: 120000,
      durationMs: null,
    );

    expect(liveButtonLabel(timeline), '直播');
    expect(timelineSummary(timeline, null), '直播 • 实时点 02:00');
  });

  test('pending ratio is clamped to seekable range', () {
    const timeline = VesperTimeline(
      kind: VesperTimelineKind.liveDvr,
      isSeekable: true,
      seekableRange: VesperSeekableRange(startMs: 30000, endMs: 90000),
      positionMs: 48000,
      liveEdgeMs: 90000,
      durationMs: 90000,
    );

    expect(timelineSummary(timeline, 1.4), '01:00 / 01:00');
  });

  test('window shrink clamps stale position before rendering', () {
    const timeline = VesperTimeline(
      kind: VesperTimelineKind.liveDvr,
      isSeekable: true,
      seekableRange: VesperSeekableRange(startMs: 40000, endMs: 70000),
      positionMs: 82000,
      liveEdgeMs: null,
      durationMs: 120000,
    );

    expect(liveButtonLabel(timeline), '直播');
    expect(timelineSummary(timeline, null), '00:30 / 00:30');
  });

  test(
    'quality capability notice explains best-effort fixed track on iOS-like backends',
    () {
      const capabilities = VesperPlayerCapabilities(
        supportsTrackCatalog: true,
        supportsTrackSelection: true,
        supportsVideoTrackSelection: false,
        supportsAudioTrackSelection: true,
        supportsSubtitleTrackSelection: true,
        supportsAbrPolicy: true,
        supportsAbrConstrained: true,
        supportsAbrFixedTrack: true,
      );

      expect(
        qualityCapabilityNotice(capabilities),
        '当前平台按 HLS variant 做 best-effort 固定画质，不保证精确切到单一视频轨。',
      );
    },
  );

  test(
    'quality capability notice stays hidden when exact video track selection is available',
    () {
      const capabilities = VesperPlayerCapabilities(
        supportsTrackCatalog: true,
        supportsTrackSelection: true,
        supportsVideoTrackSelection: true,
        supportsAudioTrackSelection: true,
        supportsSubtitleTrackSelection: true,
        supportsAbrPolicy: true,
        supportsAbrConstrained: true,
        supportsAbrFixedTrack: true,
      );

      expect(qualityCapabilityNotice(capabilities), isNull);
    },
  );

  test('quality runtime notice highlights fixed-track fallback recovery', () {
    final notice = qualityRuntimeNotice(
      const VesperPlayerSnapshot.initial().copyWith(
        trackSelection: const VesperTrackSelectionSnapshot(
          abrPolicy: VesperAbrPolicy.constrained(maxHeight: 720),
        ),
        lastError: const VesperPlayerError(
          message:
              '恢复的 iOS fixedTrack 目标 720p 长时间未收敛；当前已回退为不高于 720p 的 constrained ABR，播放器实际仍在渲染 480p。',
          category: VesperPlayerErrorCategory.playback,
          retriable: false,
        ),
      ),
    );

    expect(notice, isNotNull);
    expect(notice?.title, '已回退为受限自动');
    expect(notice?.tone, ExampleSheetNoteTone.warm);
  });

  test('quality runtime notice highlights active fixed-track mismatch', () {
    final notice = qualityRuntimeNotice(
      const VesperPlayerSnapshot.initial().copyWith(
        trackSelection: const VesperTrackSelectionSnapshot(
          abrPolicy: VesperAbrPolicy.fixedTrack(
            'video:hls:cavc1:b1500000:w1280:h720:f3000',
          ),
        ),
        fixedTrackStatus: VesperFixedTrackStatus.fallback,
        lastError: const VesperPlayerError(
          message: 'Best-effort iOS fixedTrack 720p is still rendering 480p.',
          category: VesperPlayerErrorCategory.playback,
          retriable: false,
        ),
      ),
    );

    expect(notice, isNotNull);
    expect(notice?.title, '锁定画质仍未收敛');
    expect(notice?.tone, ExampleSheetNoteTone.warm);
  });

  test('quality runtime notice hides unrelated playback errors', () {
    expect(
      qualityRuntimeNotice(
        const VesperPlayerSnapshot.initial().copyWith(
          trackSelection: const VesperTrackSelectionSnapshot(
            abrPolicy: VesperAbrPolicy.auto(),
          ),
          lastError: const VesperPlayerError(
            message: 'Network timed out.',
            category: VesperPlayerErrorCategory.playback,
            retriable: true,
          ),
        ),
      ),
      isNull,
    );
  });

  test('quality button label shows effective variant during auto abr', () {
    const trackCatalog = VesperTrackCatalog(
      tracks: <VesperMediaTrack>[
        VesperMediaTrack(
          id: 'video:hls:cavc1:b1500000:w1280:h720:f3000',
          kind: VesperMediaTrackKind.video,
          height: 720,
          bitRate: 1500000,
        ),
      ],
      adaptiveVideo: true,
    );
    const trackSelection = VesperTrackSelectionSnapshot(
      abrPolicy: VesperAbrPolicy.auto(),
    );

    expect(
      qualityButtonLabel(
        trackCatalog,
        trackSelection,
        'video:hls:cavc1:b1500000:w1280:h720:f3000',
        null,
      ),
      '自动 · 720p',
    );
  });

  test('quality button label shows pending fixed-track lock state', () {
    const trackCatalog = VesperTrackCatalog(
      tracks: <VesperMediaTrack>[
        VesperMediaTrack(
          id: 'video:hls:cavc1:b854000:w854:h480:f3000',
          kind: VesperMediaTrackKind.video,
          height: 480,
          bitRate: 854000,
        ),
        VesperMediaTrack(
          id: 'video:hls:cavc1:b1500000:w1280:h720:f3000',
          kind: VesperMediaTrackKind.video,
          height: 720,
          bitRate: 1500000,
        ),
      ],
      adaptiveVideo: true,
    );
    const trackSelection = VesperTrackSelectionSnapshot(
      abrPolicy: VesperAbrPolicy.fixedTrack(
        'video:hls:cavc1:b1500000:w1280:h720:f3000',
      ),
    );

    expect(
      qualityButtonLabel(
        trackCatalog,
        trackSelection,
        'video:hls:cavc1:b854000:w854:h480:f3000',
        VesperFixedTrackStatus.pending,
      ),
      '锁定中 · 720p',
    );
  });

  test(
    'quality auto row subtitle describes constrained abr and effective track',
    () {
      const trackCatalog = VesperTrackCatalog(
        tracks: <VesperMediaTrack>[
          VesperMediaTrack(
            id: 'video:hls:cavc1:b1500000:w1280:h720:f3000',
            kind: VesperMediaTrackKind.video,
            height: 720,
            bitRate: 1500000,
          ),
        ],
        adaptiveVideo: true,
      );
      const trackSelection = VesperTrackSelectionSnapshot(
        abrPolicy: VesperAbrPolicy.constrained(maxHeight: 720),
      );

      expect(qualityAutoRowTitle(trackSelection.abrPolicy), '自动');
      expect(qualityAutoRowBadgeLabel(trackSelection.abrPolicy), '受限');
      expect(
        qualityAutoRowSubtitle(
          trackCatalog,
          trackSelection,
          'video:hls:cavc1:b1500000:w1280:h720:f3000',
          null,
          null,
        ),
        '当前在最高 720p约束内自动调整画质。点按恢复完全自动。当前实际档位：720p。',
      );
    },
  );

  test(
    'quality auto row subtitle describes pending fixed-track observation',
    () {
      const trackCatalog = VesperTrackCatalog(
        tracks: <VesperMediaTrack>[
          VesperMediaTrack(
            id: 'video:hls:cavc1:b854000:w854:h480:f3000',
            kind: VesperMediaTrackKind.video,
            height: 480,
            bitRate: 854000,
          ),
          VesperMediaTrack(
            id: 'video:hls:cavc1:b1500000:w1280:h720:f3000',
            kind: VesperMediaTrackKind.video,
            height: 720,
            bitRate: 1500000,
          ),
        ],
        adaptiveVideo: true,
      );
      const trackSelection = VesperTrackSelectionSnapshot(
        abrPolicy: VesperAbrPolicy.fixedTrack(
          'video:hls:cavc1:b1500000:w1280:h720:f3000',
        ),
      );

      expect(
        qualityAutoRowSubtitle(
          trackCatalog,
          trackSelection,
          null,
          VesperFixedTrackStatus.pending,
          null,
        ),
        '当前请求锁定到720p，正在等待播放器确认实际档位。点按切回自动。',
      );
    },
  );

  test('quality auto row subtitle describes fixed-track fallback status', () {
    const trackCatalog = VesperTrackCatalog(
      tracks: <VesperMediaTrack>[
        VesperMediaTrack(
          id: 'video:hls:cavc1:b854000:w854:h480:f3000',
          kind: VesperMediaTrackKind.video,
          height: 480,
          bitRate: 854000,
        ),
        VesperMediaTrack(
          id: 'video:hls:cavc1:b1500000:w1280:h720:f3000',
          kind: VesperMediaTrackKind.video,
          height: 720,
          bitRate: 1500000,
        ),
      ],
      adaptiveVideo: true,
    );
    const trackSelection = VesperTrackSelectionSnapshot(
      abrPolicy: VesperAbrPolicy.fixedTrack(
        'video:hls:cavc1:b1500000:w1280:h720:f3000',
      ),
    );

    expect(
      qualityAutoRowSubtitle(
        trackCatalog,
        trackSelection,
        'video:hls:cavc1:b854000:w854:h480:f3000',
        VesperFixedTrackStatus.fallback,
        null,
      ),
      '当前请求锁定到720p，播放器实际仍在480p。点按切回自动。',
    );
  });

  test(
    'quality auto row subtitle surfaces raw observation when track is unresolved',
    () {
      const trackCatalog = VesperTrackCatalog(
        tracks: <VesperMediaTrack>[
          VesperMediaTrack(
            id: 'video:hls:cavc1:b1500000:w1280:h720:f3000',
            kind: VesperMediaTrackKind.video,
            height: 720,
            width: 1280,
            bitRate: 1500000,
          ),
        ],
        adaptiveVideo: true,
      );
      const trackSelection = VesperTrackSelectionSnapshot(
        abrPolicy: VesperAbrPolicy.fixedTrack(
          'video:hls:cavc1:b1500000:w1280:h720:f3000',
        ),
      );

      expect(
        qualityAutoRowSubtitle(
          trackCatalog,
          trackSelection,
          null,
          VesperFixedTrackStatus.pending,
          const VesperVideoVariantObservation(
            bitRate: 1420000,
            width: 1280,
            height: 720,
          ),
        ),
        '当前请求锁定到720p，正在等待播放器确认实际档位。点按切回自动。当前观测：1280×720 · 1.4 Mbps。',
      );
    },
  );

  test(
    'quality option badges separate requested and effective fixed tracks',
    () {
      expect(
        qualityOptionBadgeLabel(
          'video:hls:cavc1:b1500000:w1280:h720:f3000',
          trackCatalog: const VesperTrackCatalog(
            tracks: <VesperMediaTrack>[
              VesperMediaTrack(
                id: 'video:hls:cavc1:b854000:w854:h480:f3000',
                kind: VesperMediaTrackKind.video,
                height: 480,
                bitRate: 854000,
              ),
              VesperMediaTrack(
                id: 'video:hls:cavc1:b1500000:w1280:h720:f3000',
                kind: VesperMediaTrackKind.video,
                height: 720,
                bitRate: 1500000,
              ),
            ],
            adaptiveVideo: true,
          ),
          trackSelection: const VesperTrackSelectionSnapshot(
            abrPolicy: VesperAbrPolicy.fixedTrack(
              'video:hls:cavc1:b1500000:w1280:h720:f3000',
            ),
          ),
          effectiveVideoTrackId: 'video:hls:cavc1:b854000:w854:h480:f3000',
          fixedTrackStatus: VesperFixedTrackStatus.fallback,
        ),
        '锁定中',
      );
      expect(
        qualityOptionBadgeLabel(
          'video:hls:cavc1:b854000:w854:h480:f3000',
          trackCatalog: const VesperTrackCatalog(
            tracks: <VesperMediaTrack>[
              VesperMediaTrack(
                id: 'video:hls:cavc1:b854000:w854:h480:f3000',
                kind: VesperMediaTrackKind.video,
                height: 480,
                bitRate: 854000,
              ),
              VesperMediaTrack(
                id: 'video:hls:cavc1:b1500000:w1280:h720:f3000',
                kind: VesperMediaTrackKind.video,
                height: 720,
                bitRate: 1500000,
              ),
            ],
            adaptiveVideo: true,
          ),
          trackSelection: const VesperTrackSelectionSnapshot(
            abrPolicy: VesperAbrPolicy.fixedTrack(
              'video:hls:cavc1:b1500000:w1280:h720:f3000',
            ),
          ),
          effectiveVideoTrackId: 'video:hls:cavc1:b854000:w854:h480:f3000',
          fixedTrackStatus: VesperFixedTrackStatus.fallback,
        ),
        '实际',
      );
    },
  );

  test('quality option subtitle explains pending fixed-track rows', () {
    const trackCatalog = VesperTrackCatalog(
      tracks: <VesperMediaTrack>[
        VesperMediaTrack(
          id: 'video:hls:cavc1:b854000:w854:h480:f3000',
          kind: VesperMediaTrackKind.video,
          height: 480,
          bitRate: 854000,
        ),
        VesperMediaTrack(
          id: 'video:hls:cavc1:b1500000:w1280:h720:f3000',
          kind: VesperMediaTrackKind.video,
          height: 720,
          bitRate: 1500000,
        ),
      ],
      adaptiveVideo: true,
    );
    const requestedTrack = VesperMediaTrack(
      id: 'video:hls:cavc1:b1500000:w1280:h720:f3000',
      kind: VesperMediaTrackKind.video,
      height: 720,
      bitRate: 1500000,
    );
    const effectiveTrack = VesperMediaTrack(
      id: 'video:hls:cavc1:b854000:w854:h480:f3000',
      kind: VesperMediaTrackKind.video,
      height: 480,
      bitRate: 854000,
    );

    expect(
      qualityOptionSubtitle(
        requestedTrack,
        const VesperTrackSelectionSnapshot(
          abrPolicy: VesperAbrPolicy.fixedTrack(
            'video:hls:cavc1:b1500000:w1280:h720:f3000',
          ),
        ),
        null,
        VesperFixedTrackStatus.pending,
        trackCatalog: trackCatalog,
      ),
      '1.5 Mbps · 正在等待播放器确认这个画质',
    );
    expect(
      qualityOptionSubtitle(
        effectiveTrack,
        const VesperTrackSelectionSnapshot(
          abrPolicy: VesperAbrPolicy.fixedTrack(
            'video:hls:cavc1:b1500000:w1280:h720:f3000',
          ),
        ),
        'video:hls:cavc1:b854000:w854:h480:f3000',
        VesperFixedTrackStatus.fallback,
        trackCatalog: trackCatalog,
      ),
      '854 kbps · 播放器当前仍在这个档位',
    );
  });

  test('resilience profile can be resolved from snapshot policy', () {
    final profile = ExampleResilienceProfileLabels.fromPolicy(
      const VesperPlaybackResiliencePolicy.resilient(),
    );

    expect(profile, ExampleResilienceProfile.resilient);
  });

  test('custom resilience policy does not pretend to match a preset', () {
    final profile = ExampleResilienceProfileLabels.fromPolicy(
      const VesperPlaybackResiliencePolicy(
        buffering: VesperBufferingPolicy.streaming(),
        retry: VesperRetryPolicy(maxAttempts: 9),
        cache: VesperCachePolicy.streaming(),
      ),
    );

    expect(profile, isNull);
  });
}
