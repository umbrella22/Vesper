/* System-service ownership contracts, independent of simulator service availability. */
'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('./support/load.cjs');
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
const tick = () => new Promise(r => setImmediate(r));
let create;
const load = createLoader(name => {
  if (name === '@kit.AVSessionKit') return { avSession: { createAVSession: () => create(), PlaybackState: {} } };
  if (name === '@kit.BackgroundTasksKit') return { backgroundTaskManager: {} };
  if (name === '@kit.AbilityKit') return { wantAgent: { getWantAgent: async () => ({}), OperationType: {}, WantAgentFlags: {} } };
  if (name === '@kit.BasicServicesKit') return { systemDateTime: { TimeType: {}, getUptime: () => performance.now() } };
  throw new Error(name);
});
const { VesperSystemSession } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/SystemSession.ets'));
function player(timeline = 'unknown') {
  const value = { timeline, state: 'prepared', playbackRate: 1, positionMs: 0, volume: 1, muted: false };
  return { value, hooks: new Set(), registerCleanup(fn) { this.hooks.add(fn); return () => this.hooks.delete(fn); },
    subscribe(fn) { this.listener = fn; fn(value); return () => { this.listener = undefined; }; }, snapshot() { return value; },
    async play() {}, async pause() {}, async stop() {}, async seek() {}, async setPlaybackRate() {} };
}
function nativeSession() {
  return { listeners: new Map(), destroys: 0, on(name, fn) { this.listeners.set(name, fn); }, off(name) { this.listeners.delete(name); },
    async setLaunchAbility() {}, async activate() {}, async deactivate() {}, async destroy() { this.destroys++; },
    async setAVMetadata() {}, async setAVPlaybackState() {} };
}
const configuration = { bundleName: 'dev.test', abilityName: 'EntryAbility', title: 'Test' };
async function main() {
  const p = player(); const native = nativeSession(); create = async () => native;
  const session = await VesperSystemSession.create({}, p, configuration); await tick();
  assert.equal(native.listeners.has('seek'), false);
  p.value.timeline = 'vod'; p.listener(p.value); await tick(); assert.equal(native.listeners.has('seek'), true);
  p.value.timeline = 'live'; p.listener(p.value); await tick(); assert.equal(native.listeners.has('seek'), false);
  const oldListener = p.listener; const replacementPlayer = player();
  replacementPlayer.value.sourceId = 'replacement'; session.selectController(replacementPlayer);
  oldListener({ ...p.value, sourceId: 'stale' }); await tick();
  assert.equal(session.controller, replacementPlayer);
  assert.equal(session.pending, undefined, 'a copied old subscription must not enqueue stale metadata');
  const late = deferred(); let destroys = 0;
  native.getController = () => late.promise;
  const command = session.sendCommand('play');
  let closed = false; const close = session.close().then(() => { closed = true; }); await tick();
  assert.equal(closed, false); assert.equal(native.destroys, 0);
  late.resolve({ async sendControlCommand() { assert.fail('Closing command must not run'); }, async destroy() { destroys++; } });
  await assert.rejects(command, error => error.code === 'closed'); await close;
  assert.equal(destroys, 1); assert.equal(p.hooks.size, 0); assert.equal(replacementPlayer.hooks.size, 0);
  const retryNative = nativeSession(); create = async () => retryNative;
  const retry = await VesperSystemSession.create({}, p, configuration);
  let attempts = 0;
  retryNative.getController = async () => ({ async sendControlCommand() {}, async destroy() { if (++attempts === 1) throw new Error('destruction failed'); } });
  await assert.rejects(retry.sendCommand('pause'));
  await retry.close(); assert.equal(attempts, 2, 'Failed destruction retains native command ownership');
  const broken = nativeSession(); broken.activate = async () => { throw new Error('activate failed'); };
  broken.destroy = async () => { if (++broken.destroys === 1) throw new Error('destroy failed'); };
  create = async () => broken; await assert.rejects(VesperSystemSession.create({}, p, configuration)); await tick();
  const recovered = nativeSession(); create = async () => recovered;
  const replacement = await VesperSystemSession.create({}, p, configuration);
  assert.equal(broken.destroys, 2); await replacement.close();
  const pendingPlayer = player(); const optionalNative = nativeSession(); const metadata = []; const states = [];
  function checkDuration(value) {
    if (Object.hasOwn(value, 'duration') && value.duration === undefined) {
      const error = new Error('Native optional duration must be absent'); error.code = 401; throw error;
    }
  }
  optionalNative.setAVMetadata = async value => { checkDuration(value); metadata.push(value); };
  optionalNative.setAVPlaybackState = async value => { checkDuration(value); states.push(value); };
  create = async () => optionalNative;
  const optional = await VesperSystemSession.create({}, pendingPlayer, configuration); await tick();
  assert.equal(optional.snapshot().error, undefined, 'Unknown duration must remain publishable during activation');
  assert.equal(Object.hasOwn(metadata[0], 'duration'), false);
  assert.equal(Object.hasOwn(states[0], 'duration'), false);
  pendingPlayer.value.durationMs = 12000; pendingPlayer.listener(pendingPlayer.value); await tick();
  assert.equal(metadata.at(-1).duration, 12000); assert.equal(states.at(-1).duration, 12000);
  await optional.close();
  {
    const p = player('vod'); Object.assign(p.value, { sessionId: 'library', sourceId: 'one', playbackEpoch: 'e1' });
    const n = nativeSession(); const metadata = []; let hold; let pausePublication = false; let plays = 0;
    p.play = async () => { plays++; };
    n.setAVMetadata = async value => { metadata.push({ ...value }); if (pausePublication) { pausePublication = false; hold = deferred(); await hold.promise; } };
    create = async () => n;
    const s = await VesperSystemSession.create({}, p, configuration); await tick();
    const identity = { sessionId: 'library', sourceId: 'one', playbackEpoch: 'e1' };
    const value = { title: 'First', artist: 'Artist', artworkUri: 'https://example.test/one.png' };
    s.updateMetadata(identity, value); value.title = 'Mutated'; await tick();
    assert.equal(metadata.at(-1).title, 'First'); assert.equal(metadata.at(-1).artist, 'Artist');
    assert.equal(metadata.at(-1).mediaImage, 'https://example.test/one.png');
    assert.equal(s.snapshot().metadataSourceId, 'one'); assert.equal(s.snapshot().metadataState, 'published');
    pausePublication = true; s.updateMetadata(identity, { title: 'Slow old title', artworkUri: 'https://example.test/old.png' }); await tick();
    s.updateMetadata(identity, { title: 'Latest title' }); hold.resolve(); await tick(); await new Promise(r => setTimeout(r, 5));
    assert.equal(metadata.at(-1).title, 'Latest title'); assert.equal(metadata.at(-1).mediaImage, '');
    s.updateMetadata(identity); await tick(); assert.equal(metadata.at(-1).title, 'Test'); assert.equal(metadata.at(-1).artist, '');
    pausePublication = true; s.updateMetadata(identity, { title: 'Old source' }); await tick();
    Object.assign(p.value, { sourceId: 'two', playbackEpoch: 'e2' }); p.listener(p.value);
    assert.throws(() => s.updateMetadata(identity, { title: 'Late host artwork' }), error => error.code === 'stale_epoch');
    hold.resolve(); await tick(); await new Promise(r => setTimeout(r, 5));
    assert.equal(metadata.at(-1).assetId, 'library:two'); assert.equal(metadata.at(-1).title, 'Test');
    const capturedPlay = n.listeners.get('play');
    s.setControls({ play: false, seek: false, setSpeed: false }); await tick();
    assert.equal(n.listeners.has('play'), false); assert.equal(n.listeners.has('seek'), false);
    assert.deepEqual(s.snapshot().supportedControls, ['pause', 'stop']); capturedPlay(); assert.equal(plays, 0);
    await assert.rejects(s.sendCommand('play'), error => error.code === 'unsupported');
    assert.throws(() => s.setControls({ cast: true }), error => error.code === 'unsupported');
    s.setControls({}); await tick(); assert.equal(n.listeners.has('seek'), true);
    p.value.timeline = 'live'; p.listener(p.value); await tick(); assert.equal(n.listeners.has('seek'), false);
    const second = { sessionId: 'library', sourceId: 'two', playbackEpoch: 'e2' };
    pausePublication = true; s.updateMetadata(second, { title: 'Closing title' }); await tick();
    const closing = s.close(); assert.throws(() => s.updateMetadata(second, { title: 'Too late' }), error => error.code === 'closed');
    hold.resolve(); await closing; assert.equal(s.snapshot().state, 'closed');
  }
  for (const hostFirst of [true, false]) {
    const p = player('vod'); Object.assign(p.value, { sessionId: 'library', sourceId: 'one', playbackEpoch: '1' });
    const listeners = new Set();
    p.subscribe = listener => { listeners.add(listener); listener({ ...p.value }); return () => listeners.delete(listener); };
    const publish = () => { for (const listener of listeners) listener({ ...p.value }); };
    const n = nativeSession(); const metadata = []; let hold; let delayNext = false;
    n.setAVMetadata = async value => { metadata.push({ ...value }); if (delayNext) { delayNext = false; hold = deferred(); await hold.promise; } };
    create = async () => n;
    let s; let metadataKey = '';
    const update = snapshot => {
      if (!s) return;
      const title = 'Video ' + snapshot.sourceId;
      const key = snapshot.sessionId + ':' + snapshot.sourceId + ':' + snapshot.playbackEpoch + ':' + title;
      if (key === metadataKey) return;
      s.updateMetadata(snapshot, { title }); metadataKey = key;
    };
    let unsubscribeHost;
    if (hostFirst) unsubscribeHost = p.subscribe(update);
    s = await VesperSystemSession.create({}, p, configuration);
    if (!hostFirst) unsubscribeHost = p.subscribe(update);
    update(p.value); await tick(); await tick();
    assert.equal(metadata.at(-1).title, 'Video one');
    Object.assign(p.value, { sourceId: 'two', playbackEpoch: '2' }); publish();
    await tick(); await tick(); update(p.value); await tick();
    assert.equal(metadata.at(-1).assetId, 'library:two');
    assert.equal(metadata.at(-1).title, 'Video two', 'source metadata must survive either observer order and host deduplication');
    assert.equal(s.snapshot().metadataState, 'published');
    s.updateMetadata(p.value); publish(); await tick();
    assert.equal(metadata.at(-1).title, 'Test', 'an explicit clear must survive either observer order');
    delayNext = true; s.updateMetadata(p.value, { title: 'Slow old artwork', artworkUri: 'https://example.test/old.png' }); await tick();
    Object.assign(p.value, { sourceId: 'three', playbackEpoch: '3' }); publish(); hold.resolve();
    await tick(); await new Promise(resolve => setTimeout(resolve, 5));
    assert.equal(metadata.at(-1).title, 'Video three'); assert.equal(metadata.at(-1).mediaImage, '');
    assert.throws(() => s.updateMetadata({ ...p.value, playbackEpoch: '2' }, { title: 'Late artwork' }), error => error.code === 'stale_epoch');
    unsubscribeHost(); p.value.playbackEpoch = '4'; publish(); await tick();
    assert.equal(metadata.at(-1).title, 'Test', 'an override belongs to its accepted epoch');
    const other = player('vod'); Object.assign(other.value, { sessionId: 'other', sourceId: 'one', playbackEpoch: '1' });
    s.selectController(other); await tick(); assert.equal(metadata.at(-1).assetId, 'other:one'); assert.equal(metadata.at(-1).title, 'Test');
    await s.close();
  }
  console.log(JSON.stringify({ status: 'PASS', suite: 'system-session', contracts: ['timeline commands', 'late controller cleanup', 'close joins physical commands', 'destroy retry', 'failed constructor recovery', 'unknown duration omitted at native boundary', 'metadata observer order', 'host metadata deduplication', 'epoch-scoped override', 'late artwork after source change'] }));
}
main().catch(error => { console.error(error); process.exitCode = 1; });
