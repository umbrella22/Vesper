'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('./support/load.cjs');
const load = createLoader(name => { throw new Error('Unexpected platform import: ' + name); });
const { resolvePlaybackView } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/PlaybackView.ets'));
const snapshot = { state: 'playing', timeline: 'vod', sourceId: 'source', playbackEpoch: 'epoch', durationMs: 12000,
  buffering: true, bufferingPercent: 100, cachedDurationMs: 5000, tracks: [], availableBitrates: [] };
const view = resolvePlaybackView(snapshot, 'mp4');
assert.equal(view.seek.status, 'supported');
assert.equal(view.ranges, undefined, '100 percent buffering is not a loaded time range');
assert.equal(view.cachedDurationMs, 5000, 'a duration does not identify where the loaded interval begins');
assert.equal(view.timeRanges.status, 'supported');
assert.equal(view.playbackEpoch, 'epoch');
for (const timeline of ['live', 'unknown', 'futureNativeTimeline']) {
  const live = resolvePlaybackView({ ...snapshot, timeline }, 'hls');
  assert.equal(live.seek.status, 'unsupported'); assert.equal(live.rate.status, 'unsupported');
  assert.equal(live.timeline, timeline, 'unknown timeline values remain visible to diagnostics');
}
assert.equal(resolvePlaybackView({ ...snapshot, durationMs: undefined }, 'mp4').seek.reason, 'durationUnknown');
const initializing = resolvePlaybackView({ ...snapshot, state: 'initializing' }, 'hls');
assert.equal(initializing.seek.status, 'temporarilyUnavailable');
assert.equal(initializing.timeRanges.status, 'temporarilyUnavailable');
assert.equal(resolvePlaybackView(snapshot, 'mp4').trackConstraints.reason, 'filterApplicabilityUnverified');
assert.equal(resolvePlaybackView(snapshot, 'hls').trackConstraints.status, 'supported');
const ranges = { loaded: [{ startMs: 1000, endMs: 3000 }], seekable: [] };
assert.deepEqual(resolvePlaybackView({ ...snapshot, timeRanges: ranges }, 'hls').ranges, ranges);
process.stdout.write(JSON.stringify({ status: 'PASS', suite: 'epoch-playback-view' }) + '\n');
