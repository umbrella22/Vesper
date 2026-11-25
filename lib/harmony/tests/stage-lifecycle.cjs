'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const ts = require(process.env.VESPER_HARMONY_TYPESCRIPT);
const { createLoader } = require('./support/load.cjs');
global.Observed = value => value;
global.XComponentController = class { getXComponentSurfaceId() { return 'surface'; } };
const load = createLoader(name => {
  if (name === '@vesper/player-kit') return { VesperPlayerError: class extends Error {} };
  throw new Error('Unexpected binding import: ' + name);
});
const { VesperPlayerBinding } = load(path.resolve(__dirname, '../vesper-player-kit-arkui/src/main/ets/Binding.ets'));
const { stageQuality } = load(path.resolve(__dirname, '../vesper-player-kit-arkui-ui/src/main/ets/Presentation.ets'));
// Execute the component's methods without substituting a second implementation
// of its playback subscription, timer eligibility, or quality labels.
const source = fs.readFileSync(path.resolve(__dirname, '../vesper-player-kit-arkui-ui/src/main/ets/Stage.ets'), 'utf8')
  .replace('export struct VesperPlayerStage', 'export class VesperPlayerStage');
const syntax = ts.createSourceFile('Stage.ts', source, ts.ScriptTarget.ES2020, true);
const component = syntax.statements.find(node => ts.isClassDeclaration(node) && node.name.text === 'VesperPlayerStage');
const names = ['mayHide', 'aboutToAppear', 'aboutToDisappear', 'presentationChanged', 'scheduleHide', 'qualityText'];
const methods = component.members.filter(node => ts.isMethodDeclaration(node) && names.includes(node.name.getText(syntax)));
assert.equal(methods.length, names.length);
const compiled = ts.transpileModule('class StageLifecycle {' + methods.map(node => node.getText(syntax)).join('\n') + '}', {
  compilerOptions: { target: ts.ScriptTarget.ES2020 },
}).outputText;
const screenReaderListeners = new Set();
const StageLifecycle = vm.runInNewContext(compiled + '\nStageLifecycle', { accessibility: {
  isOpenAccessibilitySync: () => false, isScreenReaderOpenSync: () => false,
  on: (_, listener) => screenReaderListeners.add(listener), off: (_, listener) => screenReaderListeners.delete(listener),
}, stageQuality, $r: name => name });
const bitrate = { kind: 'video', mode: 'bitrate', bitrate: 350000 };
const fixed = { requested: bitrate, effective: bitrate, effectiveBitrate: 350000, outcome: 'confirmed' };
const qualitySnapshot = { width: 320, height: 180, availableBitrates: [350000, 650000],
  tracks: [{ kind: 'video', id: 1, width: 160, height: 90, bitrate: 350000 }],
  selection: fixed, trackSelections: { video: fixed } };
for (const [locale, neutral, locked, automatic, switching] of [
  ['base', 'Quality', 'Locked', 'Auto', 'Switching…'],
  ['zh_CN', '画质', '锁定', '自动', '正在切换…'],
]) {
  const strings = new Map(JSON.parse(fs.readFileSync(path.resolve(__dirname,
    `../vesper-player-kit-arkui-ui/src/main/resources/${locale}/element/string.json`), 'utf8'))
    .string.map(entry => ['app.string.' + entry.name, entry.value]));
  const probe = new StageLifecycle(); probe.text = resource => strings.get(resource);
  const label = value => { probe.binding = { snapshot: value }; return probe.qualityText(); };
  assert.equal(label({ ...qualitySnapshot, availableBitrates: [], selection: {}, trackSelections: {} }), neutral + ' · 180p',
    'system output must not imply an explicit fixed selection');
  assert.equal(label({ ...qualitySnapshot, selection: {}, trackSelections: {} }), automatic + ' · 180p');
  assert.equal(label(qualitySnapshot), locked + ' · 350 kbps', 'effective bitrate must not lock the old output dimensions');
  assert.equal(label({ ...qualitySnapshot, width: 160, height: 90 }), locked + ' · 350 kbps',
    'a later output size does not change the unit of the fixed policy');
  const pending = { ...fixed, outcome: 'pending' };
  assert.equal(label({ ...qualitySnapshot, selection: pending, trackSelections: { video: pending } }), switching);
  const failed = { ...fixed, requested: { ...bitrate, bitrate: 650000 }, outcome: 'failed' };
  assert.equal(label({ ...qualitySnapshot, selection: failed, trackSelections: { video: failed } }), locked + ' · 350 kbps',
    'a failed request retains the previously effective fixed bitrate');
  const track = { kind: 'video', mode: 'track', trackId: 1 };
  const tracked = { requested: track, effective: track, outcome: 'confirmed' };
  assert.equal(label({ ...qualitySnapshot, selection: tracked, trackSelections: { video: tracked } }), switching);
  assert.equal(label({ ...qualitySnapshot, width: 160, height: 90, selection: tracked, trackSelections: { video: tracked } }),
    locked + ' · 90p', 'fixed resolution requires matching observed output');
}
const listeners = new Set();
let snapshot = { state: 'prepared', disposed: false, playbackEpoch: '1' };
const controller = {
  snapshot: () => snapshot,
  subscribe(listener) { listeners.add(listener); listener(snapshot); return () => listeners.delete(listener); },
  attachSurface() {}, detachSurface() {},
};
function publish(value) { snapshot = value; for (const listener of listeners) listener(value); }
(async () => {
  const binding = new Proxy(new VesperPlayerBinding(controller), {});
  const stage = new StageLifecycle();
  let armed = false;
  Object.assign(stage, { binding, interactionEnabled: true, lastState: '', lastEpoch: undefined,
    focused: false, assistive: false, dragging: false, holding: false, visible: () => true,
    settingsOpen: () => false, isLocked: () => false, currentPane: () => undefined,
    refreshRanges() {}, updatePresentation() {}, debugEvent() {}, reportError(error) { throw error; },
    cancelInteraction() { armed = false; }, hideTimer: { arm(value) { armed = value; } },
  });
  stage.aboutToAppear();
  // Surface attachment rebinds the observed proxy after the Stage subscribes.
  // The Stage therefore receives each native event before binding.snapshot.
  binding.attach();
  publish({ ...snapshot, state: 'playing' });
  assert.equal(armed, true, 'the first playing event must arm hiding even when the binding still holds prepared');
  publish({ ...snapshot, state: 'paused' });
  assert.equal(armed, false, 'pause must cancel hiding even when the binding still holds playing');
  publish({ ...snapshot, state: 'playing' });
  assert.equal(armed, true);
  stage.focused = true; stage.scheduleHide(); assert.equal(armed, false, 'keyboard focus keeps controls visible');
  stage.focused = false; stage.assistive = true; stage.scheduleHide(); assert.equal(armed, false, 'screen reader interaction keeps controls visible');
  stage.assistive = false; stage.scheduleHide(); assert.equal(armed, true);
  stage.interactionEnabled = false; stage.presentationChanged();
  assert.equal(armed, false, 'leaving the mounted Stage cancels hiding');
  stage.interactionEnabled = true; stage.presentationChanged();
  assert.equal(armed, true, 'returning to a playing Stage rearms hiding without another playback event');
  publish({ ...snapshot, state: 'paused' });
  stage.interactionEnabled = false; stage.presentationChanged();
  stage.interactionEnabled = true; stage.presentationChanged();
  assert.equal(armed, false, 'returning to a paused Stage keeps controls visible');
  stage.aboutToDisappear(); assert.equal(armed, false); assert.equal(screenReaderListeners.size, 0);
  await binding.close(); assert.equal(listeners.size, 0);
  process.stdout.write(JSON.stringify({ status: 'PASS', suite: 'stage-lifecycle', contracts: ['system/auto/fixed quality labels', 'effective bitrate before/after output dimensions', 'failed request preserves effective bitrate', 'fixed track requires observed dimensions', 'Surface rebind ordering', 'first playing timer', 'pause cancels timer', 'focus and screen reader guards', 'returning presentation rearms timer', 'paused presentation stays visible', 'disappear cleanup'] }) + '\n');
})().catch(error => { process.stderr.write(String(error.stack || error) + '\n'); process.exitCode = 1; });
