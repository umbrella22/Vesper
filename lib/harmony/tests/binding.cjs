'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('./support/load.cjs');
global.Observed = value => value;
global.XComponentController = class { getXComponentSurfaceId() { return 'surface'; } };
const load = createLoader(name => {
  if (name === '@vesper/player-kit') return { VesperPlayerError: class extends Error {} };
  throw new Error('Unexpected binding import: ' + name);
});
const { VesperPlayerBinding } = load(path.resolve(__dirname, '../vesper-player-kit-arkui/src/main/ets/Binding.ets'));
const { playbackFeedback } = load(path.resolve(__dirname, '../vesper-player-kit-arkui/src/main/ets/Feedback.ets'));
const listeners = new Set();
let snapshot = { state: 'idle', disposed: false };
const controller = {
  snapshot: () => snapshot,
  subscribe(listener) { listeners.add(listener); listener(snapshot); return () => listeners.delete(listener); },
  attachSurface() {}, detachSurface() {}, async dispose() {},
};
function publish(value) { snapshot = value; for (const listener of listeners) listener(value); }
const turn = () => new Promise(resolve => setImmediate(resolve));
async function main() {
  let observedUpdates = 0;
  // ArkUI installs its observed proxy after construction. A constructor callback
  // captures the raw instance and cannot notify that proxy.
  const binding = new Proxy(new VesperPlayerBinding(controller), {
    set(target, key, value) { if (key === 'snapshot') observedUpdates++; return Reflect.set(target, key, value); },
  });
  binding.attach(); const initial = observedUpdates;
  const second = new VesperPlayerBinding(controller);
  assert.notEqual(binding.surfaceIdentity, second.surfaceIdentity, 'multiple video views require distinct XComponent identities');
  await second.close();
  publish({ state: 'playing', disposed: false, playbackEpoch: '1' });
  assert.ok(observedUpdates > initial, 'native callbacks must update the observed binding after Surface attachment');
  binding.report(Promise.reject(new Error('old command'))); await turn(); assert.match(binding.error, /old command/);
  publish({ state: 'initializing', disposed: false, playbackEpoch: '2' });
  assert.equal(binding.error, '', 'a new activation must clear an earlier command error');
  assert.equal(binding.feedback, undefined);
  for (const [code, category, action] of [['timeout', 'network', 'retry'], ['http_403', 'expired', 'refreshSource'],
    ['http_404', 'source', 'changeSource'], ['unsupported', 'unsupported', 'changeSource'],
    ['capacity', 'capacity', 'manageStorage'], ['cancelled', 'cancelled', 'dismiss']]) {
    const failure = Object.assign(new Error('https://private/?token=secret'), { code });
    const feedback = playbackFeedback(failure);
    assert.equal(feedback.category, category); assert.equal(feedback.action, action);
    assert.equal(feedback.retryable, action === 'retry');
    assert.ok(!JSON.stringify(feedback).includes('secret'), 'presentation feedback cannot expose the native message or URL');
  }
  assert.equal(playbackFeedback(Object.assign(new Error(), { nativeCode: 5411004 })).retryable, true);
  let rejectOld;
  binding.report(new Promise((_, reject) => { rejectOld = reject; }));
  binding.report(Promise.resolve()); await turn(); rejectOld(new Error('late failure')); await turn();
  assert.equal(binding.error, '', 'a late failure must not overwrite a newer successful command');
  await binding.close(); assert.equal(listeners.size, 0, 'closing must unsubscribe the observed callback');
  process.stdout.write(JSON.stringify({ status: 'PASS', suite: 'arkui-binding', contracts: ['observed native callbacks', 'activation error recovery', 'late command fencing', 'unsubscribe'] }) + '\n');
}
main().catch(error => { process.stderr.write(String(error.stack || error) + '\n'); process.exitCode = 1; });
