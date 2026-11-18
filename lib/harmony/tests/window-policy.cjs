'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('./support/load.cjs');
const load = createLoader(name => {
  if (name === '@vesper/player-kit') return { VesperPlayerError: class extends Error { constructor(code, message) { super(message); this.code = code; } } };
  throw new Error('Unexpected window import: ' + name);
});
const { VesperWindowPolicy } = load(path.resolve(__dirname, '../vesper-player-kit-arkui/src/main/ets/WindowOwnership.ets'));
function adapter(id, keep = false) {
  const state = { keepScreenOn: keep, brightness: -1 }; const writes = [];
  return { identity: id, state, writes, read: () => ({ ...state }),
    async setKeepScreenOn(value) { writes.push(['keep', value]); state.keepScreenOn = value; },
    async setBrightness(value) { writes.push(['brightness', value]); state.brightness = value; } };
}
async function main() {
  const native = adapter('two-views'); const first = new VesperWindowPolicy(native); const second = new VesperWindowPolicy(native);
  await first.update(true, true); await second.update(true, true); assert.equal(native.state.keepScreenOn, true);
  await first.update(false, true); assert.equal(native.state.keepScreenOn, true, 'hiding one view cannot cancel another view');
  await second.update(true, false); assert.equal(native.state.keepScreenOn, false, 'paused video releases the SDK request');
  await first.update(true, false); await first.setBrightness(0.8); await second.setBrightness(0.3);
  assert.equal(native.state.brightness, 0.3); await second.close(); assert.equal(native.state.brightness, 0.8);
  await first.close(); assert.equal(native.state.brightness, -1, 'final release restores automatic system brightness');
  const host = adapter('host', true); const policy = new VesperWindowPolicy(host);
  await policy.update(true, true); await policy.update(false, false); await policy.close();
  assert.equal(host.state.keepScreenOn, true, 'a host baseline must survive playback and disposal');
  const changes = adapter('host-change'); const lease = new VesperWindowPolicy(changes);
  await lease.update(true, true); await lease.setHostKeepScreenOn(true); await lease.close(); assert.equal(changes.state.keepScreenOn, true);
  const delayed = adapter('late-write'); let complete; let active = 0; let maximum = 0;
  delayed.setKeepScreenOn = value => { active++; maximum = Math.max(maximum, active); return new Promise(resolve => { complete = () => { active--; delayed.state.keepScreenOn = value; resolve(); }; }); };
  const late = new VesperWindowPolicy(delayed); const playing = late.update(true, true); const closing = late.close();
  complete(); await new Promise(resolve => setImmediate(resolve)); complete(); await Promise.all([playing, closing]);
  assert.equal(maximum, 1); assert.equal(delayed.state.keepScreenOn, false, 'late physical completion is followed by restoration');
  const failing = adapter('failure'); let fail = true;
  failing.setBrightness = async value => { if (fail) throw new Error('window rejected brightness'); failing.state.brightness = value; };
  const retry = new VesperWindowPolicy(failing); await assert.rejects(retry.setBrightness(0.6), /rejected/);
  fail = false; await retry.close(); assert.equal(failing.state.brightness, -1);
  const rejectedKeep = adapter('same-value-retry'); let keepCalls = 0;
  rejectedKeep.setKeepScreenOn = async value => { if (++keepCalls === 1) throw new Error('keep failed'); rejectedKeep.state.keepScreenOn = value; };
  const same = new VesperWindowPolicy(rejectedKeep); await assert.rejects(same.update(true, true), /keep failed/);
  await same.update(true, true); assert.equal(rejectedKeep.state.keepScreenOn, true, 'unchanged intent retries a failed native write');
  await same.close(); assert.equal(rejectedKeep.state.keepScreenOn, false);
  const mutated = adapter('mutate-then-reject'); let rejectKeep = true; let rejectBrightness = true;
  mutated.setKeepScreenOn = async value => { mutated.state.keepScreenOn = value; if (rejectKeep) throw new Error('late keep failure'); };
  mutated.setBrightness = async value => { mutated.state.brightness = value; if (rejectBrightness) throw new Error('late brightness failure'); };
  const uncertain = new VesperWindowPolicy(mutated); await assert.rejects(uncertain.update(true, true), /late keep/);
  rejectKeep = false; await assert.rejects(uncertain.setBrightness(0.7), /late brightness/); rejectBrightness = false;
  await uncertain.close(); assert.deepEqual(mutated.state, { keepScreenOn: false, brightness: -1 }, 'failed writes cannot overwrite the host baseline');
  await assert.rejects(uncertain.setHostKeepScreenOn(true), error => error.code === 'closed');
  assert.throws(() => new VesperWindowPolicy({ identity: 'constructor-failure', read() { throw new Error('window closed'); } }), /window closed/);
  const recovered = new VesperWindowPolicy(adapter('constructor-failure')); await recovered.close();
  process.stdout.write(JSON.stringify({ status: 'PASS', suite: 'window-policy-ownership' }) + '\n');
}
main().catch(error => { process.stderr.write(String(error.stack || error) + '\n'); process.exitCode = 1; });
