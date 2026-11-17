'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('./support/load.cjs');
const load = createLoader(name => { if (name === '@kit.MediaKit') return { media: {} }; throw new Error(name); });
const kit = path.resolve(__dirname, '../vesper-player-kit/src/main/ets');
const { playbackInfoObservation, playbackMetricsObservation } = load(path.join(kit, 'Diagnostics.ets'));
const { VesperBufferingPresets, resolvePolicy, bufferingStrategy } = load(path.join(kit, 'Policy.ets'));
const result = {};
playbackInfoObservation({ buffer_duration: 1.25, download_rate: 8000, average_download_rate: 4000, server_ip_address: 'private' }, result);
assert.deepEqual(result.buffer, { status: 'observed', durationMs: 1250 });
assert.equal(result.nativeDownloadRateBitsPerSecond, 8000);
assert.equal(JSON.stringify(result).includes('private'), false);
playbackInfoObservation({ buffer_duration: -1, download_rate: NaN }, result);
assert.deepEqual(result.buffer, { status: 'unavailable' }); assert.equal(result.nativeDownloadRateBitsPerSecond, undefined);
const metrics = playbackMetricsObservation({ prepare_duration: 300, stalling_count: 2, total_stalling_time: 700,
  total_playback_time: -1, future_metric: 42 });
assert.equal(metrics.prepareDurationMs, 300); assert.equal(metrics.stallCount, 2); assert.equal(metrics.stallDurationMs, 700);
assert.equal(metrics.playingDurationMs, undefined); assert.deepEqual(metrics.unrecognizedKeys, ['future_metric']);
for (const preset of [VesperBufferingPresets.fastStart(), VesperBufferingPresets.balanced(), VesperBufferingPresets.resilient()]) {
  const resolved = resolvePolicy({ buffering: preset }, 1);
  assert.equal(resolved.result.parameters.every(parameter => parameter.status === 'deferred'), true);
  const strategy = bufferingStrategy(resolved.buffering, true);
  assert.equal(strategy.preferredBufferDuration, preset.preferredBufferDurationMs / 1000);
  assert.equal(strategy.thresholdForAutoQuickPlay, undefined);
}
console.log(JSON.stringify({ status: 'PASS', suite: 'native-buffer-observation-and-presets' }));
