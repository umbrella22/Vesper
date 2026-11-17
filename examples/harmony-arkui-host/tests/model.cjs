'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('../../../lib/harmony/tests/support/load.cjs');
const strings = Object.fromEntries(require('../entry/src/main/resources/base/element/string.json').string.map(item => [item.name, item.value]));
global.Observed = value => value;
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
const flush = async () => { for (let i = 0; i < 20; i++) await new Promise(setImmediate); };
class PlayerError extends Error { constructor(code, message) { super(message); this.code = code; } }
let managerWork, pickerWork, pickerOptions, exportWork, cacheWork, pipWork, activated = [], removed = [], deleted = [], files = new Set(), managers = [], preloads = [], pips = [], closeOrder = [];
let recoveries = [], refreshes = [], resumes = [], exportPreparations = 0;
let copiedReport = '';
const snapshot = { state: 'idle', positionMs: 0, firstFrame: false, tracks: [], selection: {} };
class Controller {
  snapshot() { return snapshot; }
  subscribe(callback) { callback(snapshot); return () => {}; }
  capabilities() { return {}; }
  diagnosticReport(options) { this.diagnosticOptions = options; return { fixtureId: options.fixtureId, state: 'captured' }; }
  async setPlaybackPolicy(policy) { this.policy = policy; return { parameters: [] }; }
  async setTrackPreferences(preferences) { this.preferences = preferences; }
}
class Binding {
  controller = new Controller(); closes = 0;
  surfaceId() { return 'stable-surface'; }
  async close() { this.closes++; closeOrder.push('player'); }
}
class Sources {
  handles = []; closed = false;
  register(source) { const handle = { source, closes: 0, close() { this.closes++; }, preload() {
    const done = deferred(); const task = { completion: done.promise, result: { status: 'running', receivedBytes: 32, reuseLevel: 'none' },
      snapshot() { return this.result; }, cancel() { this.cancelled = true; this.result = { status: 'cancelled', receivedBytes: 32, reuseLevel: 'none' }; done.resolve(); },
      finish() { if (!this.cancelled) this.result = { status: 'completed', receivedBytes: 128, reuseLevel: 'playbackReusable' }; done.resolve(); } };
    preloads.push(task); return task;
  } }; this.handles.push(handle); return handle; }
  close() { this.closed = true; }
}
class Sequence {
  constructor(configuration) {
    if (process.env.VESPER_HARMONY_HOST_MODULE) {
      this.native = require(process.env.VESPER_HARMONY_HOST_MODULE);
      this.nativeHandle = this.native.create(JSON.stringify({ kind: 'sequence', configuration }));
    }
  }
  attach() {}
  replace(items, selected) {
    if (this.nativeHandle) {
      const wire = items.map(({ itemId, providerNamespace, contentIdentity, mediaKind }) => ({ itemId, providerNamespace, contentIdentity, mediaKind }));
      const response = JSON.parse(this.native.call(this.nativeHandle, JSON.stringify({ op: 'command', command: { type: 'replace', items: wire, activeItemId: selected } })));
      if (!response.ok) throw new PlayerError(response.error.code, response.error.message);
    }
    this.items = items; this.selected = selected;
  }
  jump(id) { this.selected = id; }
  async activate() {
    const item = this.items.find(item => item.itemId === this.selected); activated.push(item.source.source.uri);
    if (item.source.source.uri.includes('fail.test')) throw new PlayerError('network', 'Cannot open https://fail.test/video?token=secret-token Authorization: secret-header');
  }
  close() { if (this.nativeHandle) { this.native.release(this.nativeHandle); this.nativeHandle = undefined; } }
}
const makeManager = () => {
  const value = { closes: 0, tasks: [], subscribe(callback) { this.listener = callback; return () => {}; }, snapshots() { return this.tasks; },
    publish(task) { this.tasks = [task]; this.listener(task); },
    async close() { this.closes++; }, async remove(id) { removed.push(id); }, async offlineSource() { return { kind: 'local', protocol: 'file', uri: '/offline/movie.mp4', label: 'Offline' }; },
    async refreshSource(id, handle) { refreshes.push({ id, handle }); }, resume(id) { resumes.push(id); },
    async exportAsset() { exportPreparations++; return exportWork.promise; } };
  managers.push(value); return value;
};
const fs = { OpenMode: { CREATE: 1, TRUNC: 2, WRITE_ONLY: 4, READ_ONLY: 8 },
  async access(path) { return files.has(path); }, async mkdir(path) { files.add(path); },
  async listFile() { return ['import-123-4.mp4', 'movie.mp4', 'dash']; },
  async unlink(path) { files.delete(path); deleted.push(path); }, async open() { return { fd: 1 }; },
  async write(_, bytes) { return bytes.byteLength; }, async close() {}, async stat() { return { size: 4 }; },
  async copyFile(_, path) { files.add(path); } };
const load = createLoader(name => {
  if (name === '@kit.AbilityKit') return { ConfigurationConstant: { ColorMode: { COLOR_MODE_DARK: 0, COLOR_MODE_LIGHT: 1, COLOR_MODE_NOT_SET: -1 } } };
  if (name === '@kit.CoreFileKit') return { fileIo: fs, picker: { DocumentViewPicker: class { select(options) { pickerOptions = options; return pickerWork.promise; } } } };
  if (name === '@kit.ArkData') return { preferences: { async getPreferences() { return { async get() { return 'system'; } }; } } };
  if (name === '@kit.BasicServicesKit') return { pasteboard: { MIMETYPE_TEXT_PLAIN: 'text/plain', createData: (_, text) => text,
    getSystemPasteboard: () => ({ async setData(text) { copiedReport = text; } }) } };
  if (name === '@kit.ArkUI') return { PiPWindow: { PiPState: { ABOUT_TO_START: 1, STARTED: 2, ABOUT_TO_STOP: 3 } } };
  if (name === '@kit.ArkTS') return { url: { URL } };
  if (name === '@vesper/player-kit-arkui') return { VesperPlayerBinding: Binding, playbackFeedback: error => ({ code: error.code ?? 'unknown', category: error.code === 'in_use' ? 'inUse' : 'playback' }),
    VesperPictureInPicture: { async create() {
      if (pipWork) return pipWork.promise;
      const pip = { autoStart: false, starts: 0, closes: 0,
        async setAutoStart(value) { this.autoStart = value; }, async start() { this.starts++; }, async close() { this.closes++; },
        snapshot() { return { state: 'ready', autoStart: this.autoStart }; } };
      pips.push(pip); return pip;
    } } };
  if (name === '@vesper/player-kit') return { VesperPlayerController: Controller, VesperPlayerError: PlayerError,
    VesperDiskCache: { async open() { if (cacheWork) return cacheWork.promise;
      return { async close() { closeOrder.push('cache'); }, inventory() { return '{"entries":0,"residentBytes":0,"hitBytes":0}'; }, async clear() {} }; } },
    VesperSourceSession: Sources, VesperPlaybackSequence: Sequence,
    VesperPlaybackRecovery: class {
      constructor(controller, activate, options) { this.controller = controller; this.activate = activate; this.options = options; recoveries.push(this); }
      snapshot() { return { state: 'watching' }; } close() { this.closed = true; }
    },
    VesperCancellationToken: class { cancel() {} },
    VesperDownloadManager: { open() { return managerWork.promise; } },
    VesperSystemSession: { async create() { return { snapshot() { return { state: 'active' }; }, async close() {} }; } } };
  throw new Error(`Unexpected module ${name}`);
});
const { ExampleModel } = load(path.resolve(__dirname, '../entry/src/main/ets/model/ExampleModel.ets'));
const context = { filesDir: '/private', resourceManager: { getStringByNameSync(name) { assert.ok(strings[name], 'Missing string: ' + name); return strings[name]; }, async getRawFileContent() { return new Uint8Array([1, 2, 3, 4]); } },
  getApplicationContext() { return { setColorMode() {} }; } };
async function ready() {
  managerWork = deferred(); const model = new ExampleModel(); const start = model.start(context);
  managerWork.resolve(makeManager()); await start; assert.equal(model.ready, true); return model;
}
async function main() {
  cacheWork = deferred(); const cacheLate = new ExampleModel(); const cacheStarting = cacheLate.start(context); await flush();
  await cacheLate.close(); let lateCacheClosed = 0; cacheWork.resolve({ async close() { lateCacheClosed++; } }); await cacheStarting;
  assert.equal(lateCacheClosed, 1, 'a cache acquired after page close is still released'); cacheWork = undefined;
  // A resource owner acquired after page close must still be released.
  managerWork = deferred(); const late = new ExampleModel(); const starting = late.start(context); await flush();
  await late.close(); const lateManager = makeManager(); managerWork.resolve(lateManager); await starting;
  assert.equal(lateManager.closes, 1); assert.equal(late.ready, false); await late.close(); assert.equal(late.binding.closes, 1);
  await assert.rejects(late.start(context), error => error.code === 'closed');
  const model = await ready(); assert.ok(deleted.includes('/private/example-media/import-123-4.mp4'));
  assert.equal(model.queue.length, 4);
  model.playSample('dash'); await flush();
  assert.equal(activated.at(-1), 'https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd');
  model.playSample('hls'); await flush(); assert.ok(activated.at(-1).endsWith('/master.m3u8'));
  const bundled = model.queue[1]; model.removeQueueItem(bundled); await flush();
  assert.ok(model.queue.includes(bundled), 'public quick actions must retain the offline fixture in the queue');
  model.setRecoveryEnabled(true); await flush();
  assert.equal(model.recoveryEnabled, true); assert.equal(model.recoveryState, 'watching');
  assert.deepEqual(recoveries.at(-1).options, { maxAttempts: 2, deadlineMs: 30000, backoffMs: 500 });
  model.setRecoveryEnabled(false); await flush();
  assert.equal(recoveries.at(-1).closed, true); assert.equal(model.recoveryState, 'off');
  model.setTrackPreferences('zh', 'preferred'); await flush();
  assert.deepEqual(model.binding.controller.preferences, { audioLanguages: ['zh'], subtitleLanguages: ['zh'], subtitleMode: 'preferred' });
  assert.equal(model.preferenceLanguage, 'zh'); assert.equal(model.subtitlePreference, 'preferred');
  model.downloadAddress = 'https://fixture.test/current.mp4';
  model.resumeDownload({ taskId: 'restored', status: 'Paused', resumeState: 'source-refresh-required' }); await flush();
  assert.equal(refreshes.at(-1).id, 'restored'); assert.equal(refreshes.at(-1).handle.closes, 1);
  assert.equal(resumes.at(-1), 'restored');
  model.resumeDownload({ taskId: 'failed', status: 'Failed' }); await flush();
  assert.equal(refreshes.at(-1).id, 'failed'); assert.equal(resumes.at(-1), 'failed');
  const preparedBefore = exportPreparations;
  model.exportDownload({ taskId: 'too-large', exportEligibility: { available: false } }, false); await flush();
  assert.equal(exportPreparations, preparedBefore, 'ineligible downloads must not allocate an export');
  assert.equal(model.messageFailed, true);
  assert.equal(model.pipAutoStart, false, 'automatic PiP starts disabled until the native owner is configured');
  model.setPiPAutoStart(true); await flush(); const pip = pips.at(-1);
  assert.ok(pip, 'enabling automatic PiP creates its native owner without manual PiP first');
  assert.equal(pip.autoStart, true); assert.equal(pip.starts, 0); assert.equal(model.pipAutoStart, true);
  model.setPiPAutoStart(false); await flush(); assert.equal(pip.autoStart, false);
  const pipCount = pips.length; model.pictureInPicture(); await flush();
  assert.equal(pips.length, pipCount, 'manual and automatic PiP share one owner'); assert.equal(pip.starts, 1);
  model.applyPolicy('resilient'); await flush();
  assert.equal(model.binding.controller.policy.retry.maxAttempts, 3);
  assert.equal(model.policyProfile, 'resilient');
  model.preloadNext(); await flush(); assert.equal(model.preloadStatus, 'running');
  const firstPreload = preloads.at(-1); model.preloadNext(); await flush();
  assert.equal(preloads.at(-1), firstPreload, 'one active demo preload is admitted at a time');
  model.cancelPreload(); assert.equal(firstPreload.cancelled, true); firstPreload.finish(); await flush();
  assert.equal(model.preloadStatus, 'cancelled');
  model.preloadNext(); await flush(); preloads.at(-1).finish(); await flush();
  assert.equal(model.preloadReuse, 'playbackReusable'); assert.equal(model.preloadBytes, 128);
  model.setSystemEnabled(false); await flush(); assert.equal(model.systemState, 'Off');
  model.setSystemEnabled(true); await flush(); assert.equal(model.systemEnabled, true);
  assert.ok(!deleted.includes('/private/example-media/movie.mp4'));
  const rowManager = managers.at(-1); rowManager.publish({ taskId: '7', status: 'Downloading', receivedBytes: '0' });
  const row = model.downloads[0]; rowManager.publish({ taskId: '7', status: 'Paused', receivedBytes: '20' });
  assert.equal(model.downloads[0], row); assert.equal(row.snapshot.status, 'Paused'); assert.equal(row.snapshot.receivedBytes, '20');
  rowManager.publish({ taskId: '7', status: 'Removed' }); assert.equal(model.downloads.length, 0);
  model.address = 'https://fail.test/current.m3u8'; model.openRemote(true); await flush();
  assert.equal(model.messageFailed, true); const failedId = model.selectedId; model.retry(); await flush();
  assert.ok(!JSON.stringify({ message: model.message, logs: model.logs }).includes('secret-'), 'messages and app logs must not expose signed URLs or headers');
  model.copyDiagnostics(); await flush();
  assert.equal(JSON.parse(copiedReport).player.state, 'captured');
  assert.equal(copiedReport.includes('fail.test'), false, 'clipboard reports use the SDK projection rather than source descriptors or raw snapshots');
  assert.equal(model.binding.controller.diagnosticOptions.fixtureId, 'user-selected');
  model.diagnosticResult = { scenario: 'progress', outcome: 'completed' }; model.diagnosticResultEpoch = 'retired';
  model.copyDiagnostics(); await flush();
  assert.equal(JSON.parse(copiedReport).scenarioResult, undefined, 'a previous playback result must not be attached to the current material');
  assert.equal(model.selectedId, failedId); assert.deepEqual(activated.slice(-2), ['https://fail.test/current.m3u8', 'https://fail.test/current.m3u8']);
  const count = model.queue.length; pickerWork = deferred(); model.pickVideo(); pickerWork.resolve([]); await flush();
  assert.equal(model.queue.length, count); assert.match(model.message, /cancelled/);
  // DocumentSelectOptions separates extensions with commas on the platform boundary.
  const suffixes = pickerOptions.fileSuffixFilters.flatMap(filter => filter.split('|').at(-1).split(','));
  for (const suffix of ['.mp4', '.m4v', '.mov']) assert.ok(suffixes.includes(suffix), `Picker must admit ${suffix}`);
  assert.ok(!suffixes.includes('.txt'));
  pickerWork = deferred(); model.pickVideo(); pickerWork.resolve(['datashare://file/movie.mp4']); await flush();
  assert.equal(model.queue.length, count + 1); assert.match(activated.at(-1), /^\/private\/example-media\/import-\d+-\d+\.mp4$/);
  assert.ok(files.has(activated.at(-1)));
  const task = { taskId: '1', status: 'Completed' }; model.playDownload(task); await flush();
  assert.equal(model.canRemoveDownload('1'), false); model.removeDownload(task); await flush();
  assert.equal(removed.length, 0); assert.equal(model.message, strings.example_error_in_use);
  model.play(model.queue[0]); await flush(); model.removeDownload(task); await flush();
  assert.deepEqual(removed, ['1']); assert.ok(!model.queue.some(item => item.downloadTaskId === '1'));
  let releases = 0; exportWork = deferred(); model.exportDownload(task, false);
  exportWork.resolve({ async save() { return false; }, async release() { releases++; } }); await flush();
  assert.equal(releases, 1); assert.match(model.message, /Save cancelled/);
  exportWork = deferred(); model.exportDownload(task, false); await flush(); closeOrder = []; await model.close();
  assert.deepEqual(closeOrder, ['player', 'cache'], 'cache closes after player ownership has settled');
  exportWork.resolve({ async save() { assert.fail('Must not open a picker after close'); }, async release() { releases++; } }); await flush();
  assert.equal(releases, 2);
  const sharing = await ready(); let expired = false, retained = 0, sharedReleases = 0;
  for (let id = 1; id <= 8; id++) {
    exportWork = deferred(); sharing.exportDownload({ taskId: String(id) }, true); await flush();
    retained++;
    exportWork.resolve({ async share() {}, async release() {
      if (!expired) throw new PlayerError('in_use', 'Shared reader');
      retained--; sharedReleases++;
    } }); await flush();
    sharing.removeDownload({ taskId: String(id) }); await flush();
  }
  assert.equal(retained, 8); expired = true;
  exportWork = deferred(); sharing.exportDownload({ taskId: '9' }, false); await flush();
  assert.equal(sharedReleases, 8); assert.equal(retained, 0);
  exportWork.resolve({ async save() { return false; }, async release() {} }); await flush(); await sharing.close();
  const pickLate = await ready(); pickerWork = deferred(); pickLate.pickVideo(); await pickLate.close();
  pickerWork.resolve(['datashare://file/video.mp4']); await flush(); assert.equal(pickLate.queue.length, 4);
  const pipLate = await ready(); pipWork = deferred(); pipLate.setPiPAutoStart(true); await flush(); await pipLate.close();
  let latePiPClosed = 0; pipWork.resolve({ async close() { latePiPClosed++; } }); await flush(); pipWork = undefined;
  assert.equal(latePiPClosed, 1, 'a PiP owner acquired after page close is still released');
  assert.equal(pipLate.pip, undefined); assert.equal(pipLate.pipAutoStart, false);
  console.log(JSON.stringify({ status: 'PASS', contracts: ['late initialization owner cleanup', 'idempotent close and stale start',
    'orphan import recovery', 'retry current failed source', 'picker extension admission and private playback', 'picker cancellation and late result', 'active offline deletion guard',
    'offline queue cleanup', 'cancelled save release', 'late export release', 'expired share owners permit subsequent export', 'stable observed download row updates',
    'late disk owner cleanup', 'player-before-cache close', 'single demo preload and cancellation', 'preload result reuse remains distinct', 'policy and system controls',
    'automatic PiP without manual start', 'one PiP owner for both entry points', 'late PiP owner cleanup', 'opt-in recovery ownership', 'one-time track preference configuration', 'restored and failed source refresh before resume', 'ineligible export allocates nothing', 'user feedback and logs redact raw errors'] }));
}
main().catch(error => { console.error(error); process.exitCode = 1; });
