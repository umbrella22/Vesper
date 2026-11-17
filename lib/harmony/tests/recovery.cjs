'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('./support/load.cjs');
const load = createLoader(name => {
  if (name === '@kit.BasicServicesKit') return { systemDateTime: { TimeType: { STARTUP: 0 }, getUptime: () => performance.now() } };
  throw new Error('Unexpected recovery import: ' + name);
});
const { VesperPlaybackRecovery, isTransientPlaybackError } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/Recovery.ets'));
const { captureRecoveryIntent, matchRecoveryTrack } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/RecoveryIntent.ets'));
const turn = () => new Promise(resolve => setTimeout(resolve, 5));
const error = code => Object.assign(new Error('https://private/?token=secret'), { code });
function fixture() {
  const listeners = new Set(); const cleanup = new Set();
  let value = { state: 'playing', sourceId: 's', sessionId: 'session', playbackEpoch: '1', activationId: '1', intentRevision: 1,
    playWhenReady: true, timeline: 'vod', positionMs: 6200, playbackRate: 1.5, firstFrame: true, tracks: [], trackSelections: {} };
  let calls = 0;
  const controller = { snapshot: () => ({ ...value }),
    registerCleanup(hook) { cleanup.add(hook); return () => cleanup.delete(hook); },
    subscribe(listener) { listeners.add(listener); listener(value); return () => listeners.delete(listener); } };
  const publish = patch => { value = { ...value, ...patch }; for (const listener of listeners) listener(value); };
  const activate = async (options, token) => {
    calls++; assert.equal(options.startPositionMs, 6200); assert.equal(options.playbackRate, 1.5);
    publish({ playbackEpoch: String(calls + 1), activationId: String(calls + 1), intentRevision: value.intentRevision + 1,
      firstFrame: false, error: undefined, state: 'playing', positionMs: 6200 });
    return { playbackEpoch: value.playbackEpoch, activationId: value.activationId, sourceId: 's', sessionId: 'session' };
  };
  return { controller, publish, activate, calls: () => calls, cleanup, listeners };
}
async function until(predicate) { for (let i = 0; i < 100; i++) { if (predicate()) return; await turn(); } assert.fail('Recovery did not settle'); }
async function main() {
  {
    const audio = { id: 9, kind: 'audio', language: 'jpn', mimeType: 'audio/mp4a-latm', selected: true };
    const video = { id: 4, kind: 'video', language: '', mimeType: 'video/avc', width: 320, height: 180, bitrate: 1200000 };
    const original = { tracks: [audio, video], trackSelections: {
      audio: { outcome: 'confirmed', effective: { kind: 'audio', mode: 'track', trackId: 9 } },
      video: { outcome: 'confirmed', effective: { kind: 'video', mode: 'track', trackId: 4 } },
      subtitle: { outcome: 'confirmed', effective: { kind: 'subtitle', mode: 'off' } }
    }, trackConstraints: { status: 'accepted', requested: { maxVideoResolution: { width: 320, height: 180 } } } };
    const saved = captureRecoveryIntent(original);
    original.trackConstraints.requested.maxVideoResolution.width = 1;
    assert.equal(saved.constraints.maxVideoResolution.width, 320);
    assert.ok(!JSON.stringify(saved).includes('trackId'), 'native indexes must not survive an epoch');
    assert.equal(matchRecoveryTrack(saved.tracks[1], [{ ...audio, id: 42, language: 'ja' }]).selection.trackId, 42);
    assert.equal(matchRecoveryTrack(saved.tracks[0], [{ ...video, id: 77 }]).selection.trackId, 77);
    assert.equal(matchRecoveryTrack(saved.tracks[0], [{ ...video, id: 4, height: 90 }]).reason, 'noMatchingTrack');
    assert.equal(matchRecoveryTrack(saved.tracks[1], [{ ...audio, id: 2 }, { ...audio, id: 3 }]).reason, 'ambiguousTrack');
    assert.equal(matchRecoveryTrack(saved.tracks[2], []).selection.mode, 'off');
    for (const mode of ['auto', 'bitrate']) {
      assert.equal(matchRecoveryTrack({ kind: 'video', mode, bitrate: mode === 'bitrate' ? 350000 : undefined }, []).selection.mode, mode);
    }
    assert.equal(matchRecoveryTrack({ kind: 'subtitle', mode: 'track', track: { stableId: 'captions-jp', language: 'ja', mimeType: 'text/vtt' } },
      [{ id: -8, kind: 'subtitle', stableId: 'captions-jp', language: 'jpn', mimeType: 'text/vtt' }]).selection.trackId, -8);
    const latest = captureRecoveryIntent({ tracks: [audio], trackSelections: { audio: { outcome: 'pending',
      effective: { kind: 'audio', mode: 'track', trackId: 1 }, requested: { kind: 'audio', mode: 'track', trackId: 9 } } } });
    assert.equal(matchRecoveryTrack(latest.tracks[0], [{ ...audio, id: 40 }]).selection.trackId, 40);
  }
  for (const code of ['http_401', 'http_403', 'http_404', 'unsupported', 'resource_changed', 'capacity', 'cancelled']) assert.equal(isTransientPlaybackError(error(code)), false);
  for (const code of ['timeout', 'network_failure', 'http_503']) assert.equal(isTransientPlaybackError(error(code)), true);
  {
    const f = fixture();
    const audio = { id: 9, kind: 'audio', language: 'jpn', mimeType: 'audio/aac' };
    f.publish({ tracks: [audio], trackSelections: {
      video: { outcome: 'confirmed', effective: { kind: 'video', mode: 'bitrate', bitrate: 350000 } },
      audio: { outcome: 'confirmed', effective: { kind: 'audio', mode: 'track', trackId: 9 } },
      subtitle: { outcome: 'confirmed', effective: { kind: 'subtitle', mode: 'off' } }
    }, trackConstraints: { status: 'accepted', requested: { maxVideoBitrate: 1200000 } } });
    const restored = [];
    const intent = () => f.publish({ intentRevision: f.controller.snapshot().intentRevision + 1 });
    f.controller.setTrackConstraints = async (filter, token, timeout) => {
      assert.ok(timeout <= 1000 && timeout > 0); assert.equal(token.isCancelled, false);
      intent(); restored.push(['filter', filter]);
    };
    f.controller.selectTrack = async (choice, token) => { intent(); assert.equal(token.isCancelled, false); restored.push([choice.kind, choice]); };
    const recovery = new VesperPlaybackRecovery(f.controller, (options, token) => {
      const work = f.activate(options, token);
      f.publish({ tracks: [{ ...audio, id: 55, language: 'ja' }], trackSelections: {}, trackConstraints: undefined });
      return work;
    }, { backoffMs: 0, deadlineMs: 1000 });
    f.publish({ error: error('timeout') }); await until(() => recovery.snapshot().state === 'awaitingFrame');
    assert.deepEqual(restored.map(x => x[0]), ['filter', 'video', 'audio', 'subtitle']);
    assert.equal(restored[2][1].trackId, 55); assert.equal(restored[3][1].mode, 'off');
    f.publish({ firstFrame: true }); await until(() => recovery.snapshot().state === 'recovered');
    assert.equal(recovery.snapshot().restorations.length, 4);
    const copy = recovery.snapshot(); copy.restorations[0].status = 'mutated'; assert.equal(recovery.snapshot().restorations[0].status, 'restored');
    recovery.close();
  }
  {
    const f = fixture();
    f.publish({ tracks: [{ id: 8, kind: 'audio', language: 'eng', mimeType: 'audio/aac' }],
      trackSelections: { audio: { outcome: 'confirmed', effective: { kind: 'audio', mode: 'track', trackId: 8 } } } });
    const recovery = new VesperPlaybackRecovery(f.controller, (options, token) => {
      const work = f.activate(options, token); f.publish({ tracks: [] }); return work;
    }, { backoffMs: 0, deadlineMs: 1000 });
    f.publish({ error: error('timeout') }); await until(() => recovery.snapshot().state === 'awaitingFrame');
    assert.equal(recovery.snapshot().restorations[0].reason, 'noMatchingTrack');
    f.publish({ firstFrame: true }); await until(() => recovery.snapshot().state === 'recoveredPartial'); recovery.close();
  }
  {
    const f = fixture(); let restoring = false; let subtitles = 0;
    f.publish({ trackSelections: { video: { effective: { kind: 'video', mode: 'auto' } }, subtitle: { effective: { kind: 'subtitle', mode: 'off' } } } });
    f.controller.selectTrack = (choice, token) => {
      if (choice.kind === 'subtitle') subtitles++;
      f.publish({ intentRevision: f.controller.snapshot().intentRevision + 1 }); restoring = true;
      return new Promise((_, reject) => token.observe(() => reject(error('cancelled'))));
    };
    const recovery = new VesperPlaybackRecovery(f.controller, f.activate, { backoffMs: 0, deadlineMs: 1000 });
    f.publish({ error: error('timeout') }); await until(() => restoring);
    f.publish({ intentRevision: f.controller.snapshot().intentRevision + 1 });
    await until(() => recovery.snapshot().state === 'cancelled'); await turn();
    assert.equal(subtitles, 0, 'a newer manual intent cancels later restore steps'); recovery.close();
  }
  {
    const f = fixture(); const recovery = new VesperPlaybackRecovery(f.controller, f.activate, { backoffMs: 0, maxAttempts: 2, deadlineMs: 1000 });
    f.publish({ error: error('network_failure') }); await until(() => recovery.snapshot().state === 'awaitingFrame');
    assert.equal(recovery.snapshot().attempts, 1); assert.equal(recovery.snapshot().originalEpoch, '1');
    assert.notEqual(recovery.snapshot().state, 'recovered', 'activation/HTTP success is not a rendered frame');
    f.publish({ firstFrame: true, positionMs: 6200 }); await until(() => recovery.snapshot().state === 'recovered');
    f.publish({ error: error('timeout') }); await until(() => recovery.snapshot().attempts === 2);
    f.publish({ firstFrame: true, error: undefined }); await until(() => recovery.snapshot().state === 'recovered');
    f.publish({ error: error('timeout') }); assert.equal(recovery.snapshot().state, 'exhausted');
    await turn(); assert.equal(f.calls(), 2, 'own recovered epochs cannot reset the attempt budget');
    assert.ok(!JSON.stringify(recovery.snapshot()).includes('secret')); recovery.close(); assert.equal(f.listeners.size, 0); assert.equal(f.cleanup.size, 0);
  }
  for (const action of ['pause', 'source', 'dispose', 'close']) {
    const f = fixture(); const recovery = new VesperPlaybackRecovery(f.controller, f.activate, { backoffMs: 35, deadlineMs: 1000 });
    f.publish({ error: error('timeout') });
    if (action === 'pause') f.publish({ playWhenReady: false, intentRevision: 2 });
    if (action === 'source') f.publish({ sourceId: 'new', playbackEpoch: 'external', intentRevision: 2, error: undefined });
    if (action === 'dispose') f.publish({ disposed: true });
    if (action === 'close') recovery.close();
    await new Promise(resolve => setTimeout(resolve, 60)); assert.equal(f.calls(), 0, action + ' must cancel pending backoff'); recovery.close();
  }
  {
    const f = fixture(); const recovery = new VesperPlaybackRecovery(f.controller, f.activate, { backoffMs: 0, deadlineMs: 20 });
    f.publish({ error: error('timeout') }); await until(() => recovery.snapshot().state === 'exhausted');
    assert.equal(f.calls(), 1, 'a missing first frame must reach the total deadline'); assert.equal(recovery.snapshot().errorCode, 'timeout'); recovery.close();
  }
  {
    const f = fixture(); const recovery = new VesperPlaybackRecovery(f.controller, f.activate);
    f.publish({ error: error('http_403') }); assert.equal(recovery.snapshot().state, 'sourceRefreshRequired');
    f.publish({ error: error('http_404') }); assert.equal(recovery.snapshot().state, 'hostActionRequired'); assert.equal(f.calls(), 0); recovery.close();
  }
  process.stdout.write(JSON.stringify({ status: 'PASS', suite: 'bounded-playback-recovery' }) + '\n');
}
main().catch(error => { process.stderr.write(error.stack + '\n'); process.exitCode = 1; });
