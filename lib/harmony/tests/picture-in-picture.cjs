'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { mock } = require('node:test');
const { createLoader } = require('./support/load.cjs');
const hooks = new Set(); let create; let stopped; let listener; let value = { state: 'playing', playbackEpoch: 'one', width: 320, height: 180 };
const sizes = []; const configurations = [];
const callbacks = new Map();
const native = { on(name, callback) { callbacks.set(name, callback); }, off(name) { callbacks.delete(name); },
  updateContentSize(width, height) { sizes.push([width, height]); },
  updatePiPControlStatus() {}, setAutoStartEnabled() {}, async startPiP() { callbacks.get('stateChange')(2); },
  async stopPiP() { callbacks.get('stateChange')(3); await new Promise(resolve => { stopped = () => { callbacks.get('stateChange')(4); resolve(); }; }); } };
const load = createLoader(name => {
  if (name === '@kit.ArkUI') return { PiPWindow: { isPiPEnabled: () => true, create: config => { configurations.push(config); return create(); }, PiPTemplateType: { VIDEO_PLAY: 0 },
    PiPState: { STARTED: 2, ABOUT_TO_STOP: 3, STOPPED: 4, ERROR: 6 }, PiPControlType: { VIDEO_PLAY_PAUSE: 0 }, PiPControlStatus: { PLAY: 1, PAUSE: 0 } } };
  if (name === '@vesper/player-kit') return { VesperPlayerError: class extends Error { constructor(code, message) { super(message); this.code = code; } } };
  throw new Error(name);
});
const { VesperPictureInPicture } = load(path.resolve(__dirname, '../vesper-player-kit-arkui/src/main/ets/PictureInPicture.ets'));
const binding = { surface: { getXComponentSurfaceRect: () => ({ surfaceWidth: 600, surfaceHeight: 400 }) }, surfaceId: () => 'surface', controller: {
  registerCleanup(fn) { hooks.add(fn); return () => hooks.delete(fn); }, snapshot: () => value,
  subscribe(fn) { listener = fn; fn(value); return () => { listener = undefined; }; } } };
const tick = () => new Promise(resolve => setImmediate(resolve));
async function main() {
  create = () => { throw new Error('synchronous creation failure'); };
  await assert.rejects(VesperPictureInPicture.create({}, binding)); await tick(); assert.equal(hooks.size, 0);
  create = async () => native; const pip = await VesperPictureInPicture.create({}, binding); await pip.start();
  assert.equal(configurations.at(-1).componentController, binding.surface, 'PiP borrows the original Surface');
  assert.equal(configurations.at(-1).contentWidth, 320); assert.equal(configurations.at(-1).contentHeight, 180);
  value = { ...value, width: 180, height: 320 }; listener(value);
  assert.deepEqual(sizes.at(-1), [180, 320]); assert.equal(pip.snapshot().sizeOrigin, 'media');
  const count = sizes.length; listener(value); assert.equal(sizes.length, count, 'position-only publications must not resize PiP');
  value = { ...value, playbackEpoch: 'two', width: 0, height: 0 }; listener(value);
  assert.deepEqual(sizes.at(-1), [600, 400]); assert.equal(pip.snapshot().sizeOrigin, 'surface');
  const update = native.updateContentSize; native.updateContentSize = () => { throw Object.assign(new Error('size rejected'), { code: 401 }); };
  value = { ...value, width: 640, height: 360 }; listener(value);
  assert.equal(pip.snapshot().sizeState, 'failed'); assert.equal(pip.snapshot().sizeErrorCode, 401);
  native.updateContentSize = update; value = { ...value, width: 1280, height: 720 }; listener(value);
  assert.equal(pip.snapshot().sizeState, 'applied'); assert.equal(pip.snapshot().sizeErrorCode, undefined);
  const lateListener = listener;
  let closed = false; const close = pip.close().then(() => { closed = true; }); await tick();
  const beforeLate = sizes.length; lateListener({ ...value, width: 200, height: 100 }); assert.equal(sizes.length, beforeLate);
  assert.equal(closed, false); assert.equal(hooks.size, 1); stopped(); await close; assert.equal(hooks.size, 0);
  mock.timers.enable({ apis: ['setTimeout'] });
  try {
    let finishStart;
    const autoStartChanges = [];
    native.startPiP = () => new Promise(resolve => {
      finishStart = () => { callbacks.get('stateChange')(2); resolve(); };
    });
    native.setAutoStartEnabled = enabled => { autoStartChanges.push(enabled); };
    const delayed = await VesperPictureInPicture.create({}, binding);
    const starting = delayed.start().then(() => 'completed', error => error.code);
    await tick();
    const queued = delayed.setAutoStart(true).then(() => 'completed', error => error.code);
    await tick();
    mock.timers.tick(10000); await tick();
    assert.equal(await starting, 'timeout'); assert.equal(await queued, 'timeout');
    assert.equal(hooks.size, 1, 'logical timeout must retain ownership of the pending native start');
    finishStart(); await tick(); await tick();
    assert.deepEqual(autoStartChanges, [], 'an expired queued transition must not enable native auto-start later');
    await delayed.setAutoStart(false);
    assert.deepEqual(autoStartChanges, [false], 'physical completion must restore transition capacity');
    let released = false;
    const release = delayed.close().then(() => { released = true; }); await tick();
    assert.equal(released, false); assert.equal(hooks.size, 1);
    stopped(); await release; assert.equal(hooks.size, 0);
  } finally { mock.timers.reset(); }
  console.log(JSON.stringify({ status: 'PASS', suite: 'picture-in-picture', contracts: ['synchronous creation failure', 'confirmed stop before cleanup', 'cleanup hook ownership', 'expired queued transition rejection', 'physical ownership after timeout', 'capacity restored after late native completion'] }));
}
main().catch(error => { console.error(error); process.exitCode = 1; });
