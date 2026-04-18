import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_host/src/example_player_helpers.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
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
    expect(timelineSummary(timeline, null), '00:55 / 01:00');
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

    expect(timelineSummary(timeline, 1.4), '01:30 / 01:30');
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
    expect(timelineSummary(timeline, null), '01:10 / 01:10');
  });
}
