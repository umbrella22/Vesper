/* Exercise the real controller queue with native mutations that outlive cancellation. */
'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('./support/load.cjs');
const load = createLoader(name => {
  if (name === 'libvesper_player_harmony_node.so') return { default: { create() { throw new Error('Native bridge must not run'); } } };
  if (name === '@kit.BasicServicesKit') return { deviceInfo: { sdkApiVersion: 26 }, systemDateTime: { TimeType: { STARTUP: 0 }, getUptime: () => performance.now() } };
  if (name === '@kit.MediaKit') return { media: {} };
  if (name === '@kit.AudioKit') return { audio: {} };
  if (name === '@kit.CoreFileKit') return { fileIo: {} };
  if (name === '@kit.RemoteCommunicationKit') return { rcp: {} };
  if (name === '@kit.NetworkKit') return { socket: {} };
  if (name === '@kit.CryptoArchitectureKit') return { cryptoFramework: {} };
  if (name === '@kit.ArkTS') return { url: { URL }, util: {} };
  throw new Error('Unexpected import: ' + name);
});
const base = path.resolve(__dirname, '../vesper-player-kit/src/main/ets');
const { VesperPlayerController } = load(path.join(base, 'Controller.ets'));
const { OperationScope, VesperCancellationToken } = load(path.join(base, 'Operation.ets'));
const tick = () => new Promise(resolve => setImmediate(resolve));
const result = promise => promise.then(() => 'completed', error => error.code);
function fixture(kind = 'selection') {
  const controller = new VesperPlayerController(); const callbacks = new Map(); const calls = [];
  let selected = 0; let filter = {}; let settle; let releases = 0; let leaseReleases = 0;
  const mutate = (value, apply) => {
    calls.push(value);
    if (calls.length === 1) return new Promise((resolve, reject) => { settle = error => {
      if (error) { reject(error); } else { apply(); resolve(); }
    }; });
    apply(); return Promise.resolve();
  };
  const native = { state: 'playing', on(name, callback) { callbacks.set(name, callback); },
    async getTrackDescription() { return [0, 1, 2].map(id => ({ track_type: 0, track_index: id, language: ['und', 'eng', 'jpn'][id], codec_mime: 'audio/aac' })); },
    async getSelectedTracks() { return [selected]; },
    selectTrack(id) { return mutate(id, () => { selected = id; callbacks.get('trackChange')(id, true); }); },
    setTrackSelectionFilter(value) { return mutate(value.maxVideoBitrate, () => { filter = value; }); },
    async getTrackSelectionFilter() { return filter; }, async release() { releases++; } };
  const scope = new OperationScope(5000);
  const entry = { player: native, scope, epoch: 'one', activationId: 'one', closed: false, commands: 0,
    tail: Promise.resolve(), pending: Promise.resolve(), lease: { descriptor: { protocol: 'progressive' }, release() { leaseReleases++; } },
    eventTracks: [], confirmed: new Set(), sidecars: [], sidecarLanguages: [], sidecarSources: [],
    subtitles: [], subtitlePaths: [], selectedNativeSubtitles: [], selectedSidecar: -1, nativeTextEnabled: true, subtitleCatalogReady: true };
  VesperPlayerController.physicalPlayers++;
  controller.current = entry;
  Object.assign(controller.value, { state: 'playing', timeline: 'vod', sourceId: 'source', sessionId: 'library',
    playbackEpoch: 'one', playWhenReady: true, firstFrame: true, trackSelections: {} });
  controller.installCallbacks(entry, native);
  return { controller, entry, native, calls, settle: error => settle(error),
    releases: () => releases, leaseReleases: () => leaseReleases,
    async ready() { await controller.refreshTracks(entry); },
    command(value, token, timeout) { return kind === 'filter'
      ? controller.setTrackConstraints({ maxVideoBitrate: value }, token, timeout)
      : controller.selectTrack({ kind: 'audio', mode: 'track', trackId: value }, token, timeout); },
    async close() { await controller.dispose(); scope.close(); } };
}
async function main() {
  {
    const f = fixture(); let metricCalls = 0;
    const capabilities = f.controller.capabilities();
    assert.equal(capabilities.runtimeApi, 26);
    assert.equal(capabilities.timeRanges, true);
    assert.equal(capabilities.advancedTrackConstraints, true);
    f.native.getPlaybackInfo = async () => ({ buffer_duration: 2.5, download_rate: 64000 });
    f.native.getPlaybackStatisticMetrics = async () => { metricCalls++; return { prepare_duration: 300, stalling_count: 1, total_stalling_time: 400 }; };
    f.native.getPlaybackPosition = () => 1200;
    f.native.getCurrentPresentationTimestamp = () => 2450000;
    f.native.getLoadedTimeRanges = async () => [{ min: 0, max: 5000 }];
    f.native.getSeekableTimeRanges = async () => [{ min: 0, max: 12000 }];
    const sample = await f.controller.refreshDiagnostics();
    assert.equal(sample.playbackEpoch, 'one'); assert.equal(sample.buffer.durationMs, 2500);
    assert.equal(sample.decoderExecutionEvidence, 'unknown'); assert.equal(sample.nativeDownloadScope, 'resource-loader');
    assert.equal(metricCalls, 1);
    assert.equal(sample.metrics.status, 'observed');
    assert.equal(f.controller.snapshot().diagnostics.playbackEpoch, 'one');
    const report = f.controller.diagnosticReport({ fixtureId: 'test-fixture' });
    assert.equal(report.latest.ptsUs, 2450000);
    assert.ok(Number.isFinite(report.latest.ptsSampledAtMonotonicMs));
    assert.equal(sample.presentationTimestampStatus, 'observed');
    assert.equal(report.latest.seekable.length, 1);
    f.controller.drainEvents(); assert.ok(f.controller.diagnosticReport().timeline.length > 0);
    await f.close(); assert.equal(f.controller.diagnosticReport().latest.state, 'released');
  }
  for (const state of ['initialized', 'prepared', 'completed']) {
    const f = fixture(); f.native.state = state; f.controller.value.state = state;
    let ptsCalls = 0, infoCalls = 0;
    const notPermitted = operation => {
      f.controller.failEntry(f.entry, Object.assign(new Error(operation), { code: 'native_failure', nativeCode: 5400102 }));
      throw new Error(operation);
    };
    f.native.getPlaybackInfo = async () => {
      infoCalls++; if (state !== 'prepared') notPermitted('getPlaybackInfo outside prepared/playing/paused');
      return { buffer_duration: 0 };
    };
    f.native.getPlaybackStatisticMetrics = async () => ({});
    f.native.getPlaybackPosition = () => 0;
    f.native.getCurrentPresentationTimestamp = () => {
      ptsCalls++; if (state !== 'completed') notPermitted('getCurrentPresentationTimestamp before playing');
      return 12000000;
    };
    f.native.getLoadedTimeRanges = async () => [];
    f.native.getSeekableTimeRanges = async () => [];
    const sample = await f.controller.refreshDiagnostics();
    assert.equal(ptsCalls, state === 'completed' ? 1 : 0);
    assert.equal(infoCalls, state === 'prepared' ? 1 : 0);
    assert.equal(sample.positionStatus, state === 'initialized' ? 'unavailable' : 'observed');
    assert.equal(sample.presentationTimestampStatus, state === 'completed' ? 'observed' : 'unavailable');
    assert.equal(f.controller.snapshot().error, undefined, 'diagnostic sampling must not poison playback through an invalid native query');
    assert.equal(f.controller.diagnosticReport().latest.ptsUs, state === 'completed' ? 12000000 : undefined);
    if (state === 'prepared') {
      f.native.play = async () => { f.native.state = 'playing'; };
      await f.controller.play();
    }
    await f.close();
  }
  {
    const f = fixture(); let complete;
    f.native.getPlaybackInfo = () => new Promise(resolve => { complete = resolve; });
    const sampling = result(f.controller.refreshDiagnostics()); await tick();
    const stopping = f.controller.stop(); await tick();
    complete({ buffer_duration: 10 }); await stopping;
    assert.equal(await sampling, 'stopped');
    assert.equal(f.controller.snapshot().diagnostics, undefined, 'a retired epoch must not publish its delayed sample');
    await f.close();
  }
  for (const kind of ['selection', 'filter']) {
    const f = fixture(kind); await f.ready(); const token = new VesperCancellationToken();
    const old = result(f.command(1, token)); await tick(); assert.deepEqual(f.calls, [1]);
    token.cancel(); assert.equal(await old, 'cancelled');
    const latest = f.command(2); await tick();
    assert.deepEqual(f.calls, [1], 'a cancelled caller must retain the submitted native serialization slot');
    assert.equal(f.entry.commands, 2, 'physical and queued commands still consume bounded capacity');
    f.settle(); await latest; await tick();
    assert.deepEqual(f.calls, [1, 2]);
    const snapshot = f.controller.snapshot();
    if (kind === 'selection') {
      assert.equal(snapshot.tracks.find(track => track.selected).id, 2);
      assert.equal(snapshot.trackSelections.audio.effective.trackId, 2);
      assert.equal(snapshot.trackSelections.audio.outcome, 'confirmed');
    } else { assert.equal(snapshot.trackConstraints.accepted.maxVideoBitrate, 2); }
    assert.equal(f.entry.commands, 0); await f.close(); assert.equal(f.leaseReleases(), 1);
  }
  {
    const f = fixture(); await f.ready(); const token = new VesperCancellationToken();
    const old = result(f.command(1, token)); await tick(); token.cancel(); assert.equal(await old, 'cancelled');
    const latest = f.command(2); await tick(); assert.deepEqual(f.calls, [1]);
    f.settle(new Error('late native failure')); await latest;
    assert.equal(f.controller.snapshot().trackSelections.audio.effective.trackId, 2);
    await f.close();
  }
  {
    const f = fixture(); await f.ready();
    assert.equal(await result(f.command(1, undefined, 10)), 'timeout');
    assert.equal(await result(f.command(2, undefined, 10)), 'timeout');
    assert.deepEqual(f.calls, [1], 'public deadlines must not submit work through a pending native mutation');
    f.settle(); await f.entry.tail; await f.close();
  }
  {
    const f = fixture(); await f.ready(); const token = new VesperCancellationToken();
    const commands = [result(f.command(1, token))]; await tick(); token.cancel(); await commands[0];
    for (let count = 1; count < 16; count++) commands.push(result(f.command(2)));
    assert.equal(await result(f.command(2)), 'capacity'); assert.equal(f.entry.commands, 16);
    const stopping = f.controller.stop(); await tick();
    assert.equal(f.releases(), 0); assert.deepEqual(f.calls, [1]);
    f.settle(); await stopping; await Promise.all(commands); await f.entry.tail;
    assert.equal(f.controller.snapshot().state, 'stopped'); assert.deepEqual(f.calls, [1]);
    assert.equal(f.releases(), 1); await f.close();
  }
  {
    const f = fixture(); await f.ready(); const token = new VesperCancellationToken();
    const old = result(f.command(1, token)); await tick(); token.cancel(); await old;
    const queued = result(f.command(2)); const closing = f.controller.dispose(); await tick();
    assert.equal(f.releases(), 0); assert.deepEqual(f.calls, [1]);
    f.settle(); assert.equal(await queued, 'disposed'); await closing;
    assert.equal(f.releases(), 1); assert.equal(f.leaseReleases(), 1);
    assert.equal(f.controller.snapshot().state, 'released'); f.entry.scope.close();
  }
  console.log(JSON.stringify({ status: 'PASS', suite: 'controller-commands', contracts: [
    'cancelled native selection barrier', 'cancelled native filter barrier', 'late native rejection recovery',
    'public deadlines', 'physical capacity retained', 'stop rejects queued stale work', 'dispose joins physical ownership' ] }));
}
main().catch(error => { console.error(error); process.exitCode = 1; });
