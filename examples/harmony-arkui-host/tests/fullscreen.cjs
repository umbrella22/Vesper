'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('../../../lib/harmony/tests/support/load.cjs');
const load = createLoader(name => {
  if (name === '@kit.ArkUI') return { window: { Orientation: { LANDSCAPE: 1 } } };
  throw new Error('Unexpected import ' + name);
});
const { ExampleFullscreen } = load(path.resolve(__dirname, '../entry/src/main/ets/model/ExampleFullscreen.ets'));
function native() {
  return { layout: false, orientation: 2, calls: [],
    getWindowProperties() { return { isLayoutFullScreen: this.layout }; }, getPreferredOrientation() { return this.orientation; },
    async setWindowLayoutFullScreen(value) { this.calls.push(['layout', value]); this.layout = value; },
    async setPreferredOrientation(value) { this.calls.push(['orientation', value]); this.orientation = value; } };
}
async function main() {
  const view = native(); const policy = new ExampleFullscreen(view);
  await policy.set(true, true); assert.equal(view.layout, true); assert.equal(view.orientation, 1);
  await policy.set(false, true); assert.equal(view.layout, false); assert.equal(view.orientation, 2);
  view.calls = []; await policy.set(true, false);
  assert.ok(!view.calls.some(call => call[0] === 'orientation'), 'portrait media must retain the host orientation');
  await policy.close(); await policy.close(); assert.equal(view.layout, false); assert.equal(view.orientation, 2);
  await assert.rejects(policy.set(true, true), /closed/);
  const broken = native(); const layout = broken.setWindowLayoutFullScreen;
  broken.setWindowLayoutFullScreen = async function(value) { if (value) throw new Error('layout failed'); return layout.call(this, value); };
  const rollback = new ExampleFullscreen(broken);
  await assert.rejects(rollback.set(true, true), /layout failed/);
  assert.equal(broken.orientation, 2, 'partial entry failure must restore the previous orientation');
  assert.equal(broken.layout, false); await rollback.close();
  const slow = native(); let finish;
  const orientation = slow.setPreferredOrientation;
  slow.setPreferredOrientation = async function(value) {
    if (value === 1) await new Promise(resolve => { finish = resolve; });
    return orientation.call(this, value);
  };
  const racing = new ExampleFullscreen(slow); const entry = racing.set(true, true); let closed = false;
  const close = racing.close().then(() => { closed = true; });
  await new Promise(setImmediate); assert.equal(closed, false);
  finish(); await entry; await close;
  assert.equal(slow.layout, false); assert.equal(slow.orientation, 2, 'page close must restore after the last native write');
  const retry = native(); const restore = retry.setPreferredOrientation; let fail = true;
  retry.setPreferredOrientation = async function(value) { if (value === 2 && fail) throw new Error('restore failed'); return restore.call(this, value); };
  const owner = new ExampleFullscreen(retry); await owner.set(true, true);
  await assert.rejects(owner.close(), /restore failed/); fail = false; await owner.close();
  assert.equal(retry.orientation, 2); assert.equal(retry.layout, false);
  console.log('Example fullscreen orientation, rollback and page-close ordering: PASS');
}
main().catch(error => { console.error(error); process.exitCode = 1; });
