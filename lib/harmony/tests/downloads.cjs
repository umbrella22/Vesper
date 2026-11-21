'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const flags = require('node:fs').constants;
const path = require('node:path');
const crypto = require('node:crypto');
const { createLoader } = require('./support/load.cjs');
const native = require(process.env.VESPER_HARMONY_HOST_MODULE);
let failDiscardPlan = false;
const bridge = {
  create: native.create,
  call(handle, command) {
    if (failDiscardPlan && JSON.parse(command).op === 'discardPlan') {
      failDiscardPlan = false;
      throw Object.assign(new Error('Injected discard failure'), { code: '3' });
    }
    return native.call(handle, command);
  },
  release: native.release,
};
const descriptors = new Map();
const descriptorNames = new Map();
const providerUris = new Map();
let deferRemove, deferCheckpoint, failSync, pickerSelection = [], failedCopyTarget;
let writes = 0;
let shareModule;
global.canIUse = () => true;
const fileIo = {
  OpenMode: { READ_ONLY: 0, WRITE_ONLY: 1, READ_WRITE: 2, CREATE: 64, TRUNC: 512, NOFOLLOW: 131072 },
  async open(name, mode) {
    if (deferCheckpoint && name.endsWith('checkpoint.json.tmp')) { const hook = deferCheckpoint; deferCheckpoint = undefined; await hook(); }
    const target = providerUris.get(name) ?? name;
    const nativeMode = (mode & 2 ? flags.O_RDWR : mode & 1 ? flags.O_WRONLY : flags.O_RDONLY)
      | (mode & 64 ? flags.O_CREAT : 0) | (mode & 512 ? flags.O_TRUNC : 0) | (mode & 131072 ? flags.O_NOFOLLOW : 0);
    const handle = await fs.open(target, nativeMode);
    descriptors.set(handle.fd, handle); descriptorNames.set(handle.fd, target); return { fd: handle.fd };
  },
  async close(file) { const fd = typeof file === 'number' ? file : file.fd; const handle = descriptors.get(fd); await handle.close(); descriptors.delete(fd); descriptorNames.delete(fd); },
  async read(fd, buffer, options) { return (await descriptors.get(fd).read(new Uint8Array(buffer), 0, options?.length ?? buffer.byteLength, options?.offset ?? null)).bytesRead; },
  async write(fd, buffer, options) {
    if (descriptorNames.get(fd) === failedCopyTarget) { await descriptors.get(fd).writeFile('partial'); throw new Error('Injected partial copy'); }
    writes++; return (await descriptors.get(fd).write(new Uint8Array(buffer).subarray(0, 17), 0, Math.min(17, buffer.byteLength), options?.offset ?? null)).bytesWritten;
  },
  lstat: name => fs.lstat(name), stat: name => typeof name === 'number' ? descriptors.get(name).stat() : fs.stat(name),
  mkdir: (name, recursive) => fs.mkdir(name, { recursive }),
  async fsync(fd) { if (failSync) { failSync = false; throw new Error('Injected fsync failure'); } await descriptors.get(fd).sync(); },
  rename: (a, b) => fs.rename(a, b),
  unlink: name => fs.unlink(name),
  async copyFile(source, destination) {
    const target = typeof destination === 'number' ? descriptorNames.get(destination) : destination;
    if (target === failedCopyTarget) {
      if (typeof destination === 'number') { await descriptors.get(destination).writeFile('partial'); }
      else { await fs.writeFile(target, 'partial'); }
      throw new Error('Injected partial copy');
    }
    if (typeof destination === 'number') { await descriptors.get(destination).writeFile(await fs.readFile(source)); }
    else { await fs.copyFile(source, destination); }
  },
  truncate: (fd, size) => descriptors.get(fd).truncate(size),
  listFile: name => fs.readdir(name),
  async access(name) { try { await fs.access(name); return true; } catch { return false; } },
  async readText(name) { return fs.readFile(name, 'utf8'); },
  async rmdir(name) { if (deferRemove) { const hook = deferRemove; deferRemove = undefined; await hook(); } await fs.rm(name, { recursive: true }); },
};
const bytes = Buffer.alloc(70001, 42);
let fetches = 0;
let resourceBytes = bytes, resourceEtag = '"stable"', beforeFetch;
const requests = [];
const resources = new Map();
let concurrentFetches = 0, maximumConcurrentFetches = 0;
const load = createLoader(name => {
  if (name === 'libvesper_player_harmony_node.so') return { default: bridge };
  if (name === '@kit.CoreFileKit') return { fileIo, fileUri: { getUriFromPath: value => 'file://' + value }, picker: { DocumentViewPicker: class { async save() { return pickerSelection; } } } };
  if (name === '@kit.ShareKit') return { get systemShare() { return shareModule; } };
  if (name === '@kit.ArkTS') return { url: { URL }, util: { generateRandomUUID: () => crypto.randomUUID(), TextEncoder: class { encodeInto(value) { return new Uint8Array(Buffer.from(value)); } },
    TextDecoder: { create: () => ({ decodeWithStream: value => Buffer.from(value).toString('utf8') }) } } };
  if (name === '@kit.BasicServicesKit') return { systemDateTime: { TimeType: { STARTUP: 0 }, getUptime: () => performance.now() } };
  if (name === '@kit.CryptoArchitectureKit') return { cryptoFramework: { createMd() {
    const hash = crypto.createHash('sha256'); return { async update(blob) { hash.update(blob.data); }, async digest() { return { data: new Uint8Array(hash.digest()) }; } };
  } } };
  if (name === '@kit.RemoteCommunicationKit') return { rcp: {
    Request: class { constructor(uri, method, headers) { this.uri = uri; this.headers = headers; } },
    createSession: () => ({ cancel() {}, close() {}, async fetch(request) {
      fetches++; requests.push({ uri: request.uri, ...request.headers });
      if (beforeFetch) await beforeFetch(request);
      const payload = resources.get(request.uri) ?? resourceBytes;
      if (resources.size > 0) {
        concurrentFetches++; maximumConcurrentFetches = Math.max(maximumConcurrentFetches, concurrentFetches);
        await new Promise(resolve => setImmediate(resolve)); concurrentFetches--;
      }
      const match = request.headers.Range?.match(/bytes=(\d+)-(\d+)/);
      if (!match) {
        request.destination(new Uint8Array(payload).buffer);
        return { statusCode: 200, headers: { 'content-type': 'application/dash+xml', 'content-length': `${payload.length}`, etag: resourceEtag } };
      }
      const start = +match[1], end = +match[2];
      request.destination(new Uint8Array(payload.subarray(start, end + 1)).buffer);
      return { statusCode: 206, headers: { 'content-type': 'video/mp4', 'content-range': `bytes ${start}-${end}/${payload.length}`, etag: resourceEtag } };
    } }),
  } };
  throw new Error('Unexpected import ' + name);
});
const kit = path.resolve(__dirname, '../vesper-player-kit/src/main/ets');
const { VesperDownloadManager } = load(path.join(kit, 'Downloads.ets'));
const { VesperSourceSession } = load(path.join(kit, 'SourceSession.ets'));
const { AssetPins } = load(path.join(kit, 'FileStore.ets'));
const turn = () => new Promise(resolve => setImmediate(resolve));
async function main() {
  const root = await fs.mkdtemp(path.resolve(__dirname, '../../../target/harmony-download-contract-'));
  const context = { filesDir: root }; const checkpoint = path.join(root, 'vesper-downloads/checkpoint.json');
  const sources = new VesperSourceSession();
  const source = sources.register({ kind: 'remote', protocol: 'progressive', label: 'Private', uri: 'https://example.test/media.mp4?token=secret', headers: { Authorization: 'secret' } });
  let manager = await VesperDownloadManager.open(context);
  failDiscardPlan = true;
  await assert.rejects(manager.create(source, ' invalid-asset-id'), error => error.code === '3');
  assert.equal(failDiscardPlan, false, 'Creation failed after the plan was reserved');
  assert.equal(sources.snapshot().leases, 0, 'Failed discard must still release the source lease');
  const created = await manager.create(source, 'asset'); manager.start(created.taskId);
  assert.equal((await manager.wait(created.taskId)).status, 'Completed');
  const offline = await manager.offlineSource(created.taskId);
  const artifact = await manager.exportAsset(created.taskId);
  assert.equal((await fs.readFile(artifact.path)).length, bytes.length);
  assert.equal(artifact.path.startsWith(path.dirname(offline.uri) + '/'), false, 'temporary export must survive removal of its original asset');
  const providerUri = 'file://docs/storage/Users/currentUser/Download/media.mp4';
  const providerOutput = path.join(root, 'provider-output'); providerUris.set(providerUri, providerOutput); pickerSelection = [providerUri];
  assert.equal(await artifact.save(context), true, 'a new provider output must be created through its URI and copied by descriptor');
  assert.deepEqual(await fs.readFile(providerOutput), bytes);
  assert.equal(descriptors.size, 0, 'saving must close the provider descriptor');
  pickerSelection = []; assert.equal(await artifact.save(context), false);
  pickerSelection = [providerUri]; failedCopyTarget = providerOutput; await fs.writeFile(providerOutput, '');
  await assert.rejects(artifact.save(context), /Injected partial copy/);
  assert.equal((await fs.stat(providerOutput)).size, 0, 'failed provider output must return to its admitted empty state');
  await fs.writeFile(providerOutput, 'preserve');
  await assert.rejects(artifact.save(context), error => error.code === 'destination_not_empty');
  assert.equal(await fs.readFile(providerOutput, 'utf8'), 'preserve');
  assert.equal(descriptors.size, 0, 'save failures must close the provider descriptor');
  const { VesperExportArtifact } = load(path.join(kit, 'Exports.ets'));
  const { exportCapacityIssue, EXPORT_MAXIMUM_BYTES } = load(path.join(kit, 'ExportStream.ets'));
  assert.equal(exportCapacityIssue([{ path: 'media.mp4', size: EXPORT_MAXIMUM_BYTES }], EXPORT_MAXIMUM_BYTES), undefined);
  assert.equal(exportCapacityIssue([{ path: 'media.mp4', size: EXPORT_MAXIMUM_BYTES + 1 }], EXPORT_MAXIMUM_BYTES + 1), 'size-limit');
  const staleExport = path.join(root, 'vesper-downloads/exports', `${Date.now() - 700000}-${crypto.randomUUID()}`);
  await fs.mkdir(staleExport); await fs.writeFile(path.join(staleExport, 'partial'), 'old');
  const retainedExport = path.join(root, 'vesper-downloads/exports', `${Date.now() - 700000}-${crypto.randomUUID()}`);
  await fs.mkdir(retainedExport); await fs.writeFile(path.join(retainedExport, 'retention.json'), JSON.stringify({ schemaVersion: 1, retainUntilEpochMs: Date.now() + 600000 }));
  await VesperExportArtifact.recover(path.join(root, 'vesper-downloads'));
  assert.equal(await fileIo.access(staleExport), false); assert.equal(await fileIo.access(retainedExport), true);
  assert.equal(await fileIo.access(artifact.path), true, 'recovery must retain current owners'); await fs.rm(retainedExport, { recursive: true });
  await assert.rejects(artifact.share(context), error => error.code === 'unsupported', 'an unavailable native share module must report unsupported');
  let sharedRecord;
  shareModule = { SharedData: class { constructor(record) { sharedRecord = record; } }, ShareController: class { async show() {} } };
  await artifact.share(context);
  assert.equal(sharedRecord.uri, 'file://' + artifact.path, 'the lazy share adapter must receive the owned URI');
  await assert.rejects(artifact.release(), error => error.code === 'in_use', 'chooser dismissal must retain recipient access');
  const now = Date.now; const afterRetention = now() + 600001;
  try { Date.now = () => afterRetention; await artifact.release(); } finally { Date.now = now; }
  const budgetRoot = path.join(root, 'budget'); const exportParent = path.join(budgetRoot, 'exports');
  await fs.mkdir(exportParent, { recursive: true });
  for (const reservedBytes of [256 * 1024 * 1024, 256 * 1024 * 1024 - bytes.length]) {
    const retained = path.join(exportParent, `${Date.now()}-${crypto.randomUUID()}`); await fs.mkdir(retained);
    await fs.writeFile(path.join(retained, 'retention.json'), JSON.stringify({ schemaVersion: 1, reservedBytes, retainUntilEpochMs: Date.now() + 600000 }));
  }
  const members = [{ path: path.basename(offline.uri), size: bytes.length, checksum: crypto.createHash('sha256').update(bytes).digest('hex') }];
  const admissions = await Promise.allSettled(Array.from({ length: 2 }, () => VesperExportArtifact.create(path.dirname(offline.uri), budgetRoot, 'progressive', members[0].path, members)));
  assert.equal(admissions.filter(item => item.status === 'fulfilled').length, 1, 'concurrent exports cannot each spend the same remaining disk reservation');
  assert.equal(admissions.find(item => item.status === 'rejected').reason.code, 'export_cleanup_required');
  for (const item of admissions) if (item.status === 'fulfilled') { assert.equal((await fs.stat(item.value.path)).size, bytes.length); await item.value.release(); }
  assert.equal((await fs.readFile(offline.uri)).length, bytes.length);
  assert.ok(writes > 100, 'partial writes must make bounded positive progress');
  assert.equal((await fs.readFile(checkpoint, 'utf8')).includes('secret'), false);
  await manager.close();
  const record = JSON.parse(await fs.readFile(checkpoint, 'utf8'));
  record.tasks[0].entry = 'orphan.mp4'; await fs.writeFile(checkpoint, JSON.stringify(record));
  const quarantined = await VesperDownloadManager.open(context);
  assert.equal(quarantined.snapshots()[0].status, 'Removed'); assert.equal(JSON.parse(quarantined.inventory()).quarantinedRecords, 1); await quarantined.close();
  record.tasks[0].entry = path.basename(offline.uri); await fs.writeFile(checkpoint, JSON.stringify(record));
  manager = await VesperDownloadManager.open(context);
  let releaseWrite, beganWrite;
  const writeStarted = new Promise(resolve => { beganWrite = resolve; });
  deferCheckpoint = async () => { beganWrite(); await new Promise(resolve => { releaseWrite = resolve; }); };
  const pauses = Array.from({ length: 64 }, () => manager.pause(created.taskId).then(() => 'ok', error => error.code));
  await writeStarted;
  assert.ok(manager.pendingMutations <= 8); assert.ok(manager.pendingCheckpoints <= 8);
  releaseWrite(); const outcomes = await Promise.all(pauses);
  assert.ok(outcomes.includes('capacity'));
  const unpin = AssetPins.acquire(offline.uri);
  await assert.rejects(manager.remove(created.taskId), error => error.code === 'in_use'); unpin();
  let releaseRemove, beganRemove;
  const removeStarted = new Promise(resolve => { beganRemove = resolve; });
  deferRemove = async () => { beganRemove(); await new Promise(resolve => { releaseRemove = resolve; }); };
  const removing = manager.remove(created.taskId); await removeStarted;
  assert.throws(() => AssetPins.acquire(offline.uri), error => error.code === 'in_use');
  let closed = false; const closing = manager.close().then(() => { closed = true; }); await turn();
  assert.equal(closed, false, 'close must retain the root through physical removal');
  await assert.rejects(VesperDownloadManager.open(context), error => error.code === 'in_use');
  releaseRemove(); await removing; await closing;
  manager = await VesperDownloadManager.open(context);
  assert.equal(manager.snapshots()[0].status, 'Removed');
  await assert.rejects(manager.offlineSource(created.taskId), error => error.code === 'offline_incomplete');
  await manager.close(); source.close(); sources.close();
  // Pause after one committed chunk, reopen the manager, and prove actual byte reuse.
  resourceBytes = crypto.randomBytes(196609); const resumeContext = { filesDir: path.join(root, 'resume') };
  const resumeSources = new VesperSourceSession(); const longTitle = 'Long media title '.repeat(32);
  const resumeSource = resumeSources.register({ kind: 'remote', protocol: 'progressive', label: longTitle,
    uri: 'https://example.test/resume.mp4?signature=private', headers: { Authorization: 'private' } });
  let resumeManager = await VesperDownloadManager.open(resumeContext);
  async function pausedTask(assetId) {
    const task = await resumeManager.create(resumeSource, assetId); let blocked, release;
    const reached = new Promise(resolve => { blocked = resolve; });
    beforeFetch = async request => {
      if (request.headers.Range.startsWith('bytes=65536-')) {
        beforeFetch = undefined; blocked(); await new Promise(resolve => { release = resolve; });
      }
    };
    resumeManager.start(task.taskId); await reached;
    const durable = JSON.parse(await fs.readFile(path.join(resumeContext.filesDir, 'vesper-downloads/checkpoint.json'), 'utf8'));
    const committed = durable.tasks.find(item => item.task.taskId === task.taskId).files[0];
    assert.equal(committed.partialBytes, 65536, 'the first completed chunk must survive a crash before the next request completes');
    assert.equal(committed.partialHashes.length, 1);
    await resumeManager.pause(task.taskId); release();
    const paused = await resumeManager.wait(task.taskId);
    assert.equal(paused.status, 'Paused'); assert.equal(paused.reusableBytes, '65536');
    assert.equal(paused.exportEligibility.available, false); assert.equal(paused.exportEligibility.reason, 'not-completed');
    return task;
  }
  const resumedTask = await pausedTask('resume'); await resumeManager.close();
  const resumeCheckpoint = path.join(resumeContext.filesDir, 'vesper-downloads/checkpoint.json');
  const resumeRecord = JSON.parse(await fs.readFile(resumeCheckpoint, 'utf8'));
  assert.equal(JSON.stringify(resumeRecord).includes('private'), false, 'neither locator nor access headers persist');
  const member = resumeRecord.tasks[0].files[0];
  const partialPath = path.join(resumeContext.filesDir, `vesper-downloads/asset-${resumedTask.taskId}`, member.path + '.part');
  await fs.appendFile(partialPath, Buffer.alloc(123, 99));
  resumeManager = await VesperDownloadManager.open(resumeContext);
  assert.equal(resumeManager.snapshots()[0].title, longTitle, 'a valid long source label must survive checkpoint restore');
  assert.equal(resumeManager.snapshots()[0].resumeState, 'source-refresh-required');
  await resumeManager.refreshSource(resumedTask.taskId, resumeSource); const reuseStart = requests.length;
  resumeManager.resume(resumedTask.taskId); const continued = await resumeManager.wait(resumedTask.taskId);
  assert.equal(continued.status, 'Completed'); assert.equal(continued.exportEligibility.available, true);
  const continuedRequests = requests.slice(reuseStart);
  assert.equal(continuedRequests[0].Range, 'bytes=0-0'); assert.equal(continuedRequests[0]['If-Range'], resourceEtag);
  assert.equal(continuedRequests[1].Range, 'bytes=65536-131071'); assert.equal(continuedRequests[1]['If-Range'], resourceEtag);
  const continuedSource = await resumeManager.offlineSource(resumedTask.taskId);
  assert.deepEqual(await fs.readFile(continuedSource.uri), resourceBytes, 'crash suffix must be discarded before continuing');
  const corruptTask = await pausedTask('corrupt');
  const corruptRecord = JSON.parse(await fs.readFile(resumeCheckpoint, 'utf8')).tasks.find(task => task.task.taskId === corruptTask.taskId);
  const corruptPath = path.join(resumeContext.filesDir, `vesper-downloads/asset-${corruptTask.taskId}`, corruptRecord.files[0].path + '.part');
  await fs.writeFile(corruptPath, Buffer.alloc(65536, 0)); const corruptStart = requests.length;
  resumeManager.resume(corruptTask.taskId); assert.equal((await resumeManager.wait(corruptTask.taskId)).status, 'Completed');
  assert.equal(requests.slice(corruptStart)[1].Range, 'bytes=0-65535', 'a corrupt prefix restarts instead of mixing bytes');
  const changedTask = await pausedTask('changed'); resourceEtag = '"replacement"';
  resumeManager.resume(changedTask.taskId); const changed = await resumeManager.wait(changedTask.taskId);
  assert.equal(changed.status, 'Failed'); assert.equal(changed.reason, 'resource_changed');
  assert.equal(changed.resumeState, 'source-refresh-required'); assert.equal(changed.reusableBytes, '0');
  await resumeManager.refreshSource(changedTask.taskId, resumeSource); resumeManager.resume(changedTask.taskId);
  assert.equal((await resumeManager.wait(changedTask.taskId)).status, 'Completed', 'explicit refresh resets a failed native task');
  await resumeManager.close(); resumeSource.close(); resumeSources.close(); resourceBytes = bytes; resourceEtag = '"stable"';
  const templateContext = { filesDir: path.join(root, 'templates') };
  const templateSources = new VesperSourceSession();
  const templateUri = 'https://example.test/template/manifest.mpd';
  const mpd = (seconds, timeline = false) => Buffer.from(`<MPD type="static" mediaPresentationDuration="PT${seconds}S"><Period>${['video', 'audio'].map((kind, index) =>
    `<AdaptationSet mimeType="${kind}/mp4" lang="${index ? 'eng' : 'und'}"><Representation id="${kind}" bandwidth="1000" codecs="${index ? 'mp4a.40.2' : 'avc1.42c01e'}"><SegmentTemplate timescale="1000" startNumber="7" ${timeline ? 'presentationTimeOffset="9000"' : 'duration="6000"'} initialization="${kind}-init.mp4" media="${kind}-$${timeline ? 'Time' : 'Number%05d'}$.m4s">${timeline ? '<SegmentTimeline><S t="9000" d="6000" r="-1"/></SegmentTimeline>' : ''}</SegmentTemplate></Representation></AdaptationSet>`).join('')}</Period></MPD>`);
  resources.set(templateUri, mpd(7200));
  resourceBytes = Buffer.alloc(17, 42);
  const templateSource = templateSources.register({ kind: 'remote', protocol: 'dash', label: 'Finite templates', uri: templateUri });
  let templates = await VesperDownloadManager.open(templateContext);
  const long = await templates.create(templateSource, 'two-hours');
  const templateCheckpoint = path.join(templateContext.filesDir, 'vesper-downloads/checkpoint.json');
  let stored = JSON.parse(await fs.readFile(templateCheckpoint, 'utf8'));
  assert.equal(stored.tasks[0].files.length, 2403, 'two hours requires both 1200-fragment streams, init files, and MPD');
  assert.equal(long.totalSegments, 2400);
  assert.equal(maximumConcurrentFetches, 4, 'long-asset discovery must use the bounded four-request batch');
  await templates.close(); templates = await VesperDownloadManager.open(templateContext);
  assert.equal(templates.snapshots()[0].resumeState, 'source-refresh-required', 'long-asset admission and restart use the same file quota');
  await templates.remove(long.taskId);
  resources.set(templateUri, mpd(12, true));
  for (const kind of ['video', 'audio']) {
    resources.set(`https://example.test/template/${kind}-init.mp4`, Buffer.alloc(17, kind === 'video' ? 1 : 2));
    for (const time of [9000, 15000]) resources.set(`https://example.test/template/${kind}-${time}.m4s`, Buffer.alloc(17, time / 1000));
  }
  const finite = await templates.create(templateSource, 'finite-timeline');
  templates.start(finite.taskId); assert.equal((await templates.wait(finite.taskId)).status, 'Completed');
  const finiteSource = await templates.offlineSource(finite.taskId);
  const localMpd = await fs.readFile(finiteSource.uri, 'utf8');
  assert.match(localMpd, /presentationTimeOffset="9000"/); assert.match(localMpd, /SegmentTimeline/);
  assert.equal(localMpd.includes('https://'), false);
  await templates.close(); resources.clear();
  beforeFetch = async () => { throw new Error('Origin disconnected'); };
  templates = await VesperDownloadManager.open(templateContext);
  assert.equal(templates.snapshots().find(task => task.taskId === finite.taskId).status, 'Completed');
  assert.equal((await templates.offlineSource(finite.taskId)).uri, finiteSource.uri);
  const templatePin = AssetPins.acquire(finiteSource.uri);
  await assert.rejects(templates.remove(finite.taskId), error => error.code === 'in_use'); templatePin();
  await templates.remove(finite.taskId); await templates.close(); templateSource.close(); templateSources.close();
  // Restore verified files through the public manager to exercise the export policy
  // without issuing hundreds of network requests and download checkpoints.
  const exportContext = { filesDir: path.join(root, 'export-boundary') };
  const exportRoot = path.join(exportContext.filesDir, 'vesper-downloads');
  const boundaryTasks = [];
  for (const count of [512, 513]) {
    const taskId = String(count);
    const assetRoot = path.join(exportRoot, 'asset-' + taskId);
    const entry = 'manifest.mpd';
    const manifest = Buffer.from('<MPD type="static"/>');
    const segment = Buffer.from('sample');
    const manifestHash = crypto.createHash('sha256').update(manifest).digest('hex');
    const segmentHash = crypto.createHash('sha256').update(segment).digest('hex');
    await fs.mkdir(assetRoot, { recursive: true });
    await fs.writeFile(path.join(assetRoot, entry), manifest);
    const files = [{ path: entry, size: manifest.length, checksum: manifestHash, generated: true, segment: false }];
    for (let index = 1; index < count; index++) {
      const name = `video-${index}.m4s`;
      await fs.writeFile(path.join(assetRoot, name), segment);
      files.push({ path: name, size: segment.length, checksum: segmentHash, generated: false, segment: true });
    }
    const size = String(manifest.length + (count - 1) * segment.length);
    boundaryTasks.push({ task: { taskId, assetId: `export-${count}`, status: 'Completed', receivedBytes: size, totalBytes: size }, format: 'dash', entry, files });
  }
  await fs.writeFile(path.join(exportRoot, 'checkpoint.json'), JSON.stringify({ schemaVersion: 1, tasks: boundaryTasks }));
  const exports = await VesperDownloadManager.open(exportContext);
  try {
    const accepted = exports.snapshots().find(task => task.taskId === '512');
    assert.equal(accepted.status, 'Completed');
    assert.equal(accepted.exportEligibility.available, true);
    assert.equal(accepted.exportEligibility.reason, 'eligible');
    const zip = await exports.exportAsset(accepted.taskId);
    assert.equal((await fs.stat(zip.path)).size, accepted.exportEligibility.outputBytes);
    await zip.release();
    const rejected = exports.snapshots().find(task => task.taskId === '513');
    assert.equal(rejected.status, 'Completed');
    assert.equal(rejected.exportEligibility.available, false);
    assert.equal(rejected.exportEligibility.reason, 'file-count-limit');
    await assert.rejects(exports.exportAsset(rejected.taskId), error => error.code === 'capacity');
  } finally { await exports.close(); }
  beforeFetch = undefined; resourceBytes = bytes;
  const { VesperDiskCache } = load(path.join(kit, 'DiskCache.ets'));
  const disk = await VesperDiskCache.open({ cacheDir: root }, { partition: 'account-a', maximumBytes: 65536 });
  const scope = disk.newScope(); const other = disk.newScope(); const uri = 'https://private.test/media?token=secret';
  const info = { totalBytes: 90000, finalUrl: uri, contentType: 'video/mp4', etag: '"v1"' };
  const chunk = new Uint8Array(40000).fill(19).buffer;
  failSync = true; await assert.rejects(disk.put(scope, uri, 0, chunk, info));
  assert.equal((await fs.readdir(path.join(root, 'vesper-cache-account-a'))).some(name => name.endsWith('.part')), false);
  await disk.put(scope, uri, 0, chunk, info);
  assert.equal(new Uint8Array(await disk.read(scope, uri, 10, 20))[0], 19);
  assert.equal(await disk.read(other, uri, 10, 20), undefined, 'registration/access partitions must not alias');
  const diskRoot = path.join(root, 'vesper-cache-account-a');
  let index = JSON.parse(await fs.readFile(path.join(diskRoot, 'index.json'), 'utf8'));
  assert.equal(JSON.stringify(index).includes('secret'), false);
  await disk.put(scope, 'https://private.test/live', 0, chunk, { ...info, finalUrl: 'https://private.test/live', contentType: 'application/vnd.apple.mpegurl' });
  assert.equal(await disk.metadata(scope, 'https://private.test/live'), undefined, 'mutable manifests never enter the disk byte cache');
  await fs.writeFile(path.join(diskRoot, index.entries[0].key + '.bin'), 'corrupt');
  assert.equal(await disk.read(scope, uri, 0, 20), undefined); assert.equal(JSON.parse(disk.inventory()).corruptions, 1);
  await disk.put(scope, uri, 0, chunk, info); await disk.put(scope, uri, 40000, chunk, info);
  assert.equal(await disk.read(scope, uri, 0, 20), undefined, 'capacity evicts least recently used bytes');
  assert.equal(JSON.parse(disk.inventory()).residentBytes, 40000);
  await disk.clear(); assert.equal(await disk.metadata(scope, uri), undefined); await disk.close();
  assert.equal(descriptors.size, 0); await fs.rm(root, { recursive: true });
  console.log(JSON.stringify({ status: 'PASS', scope: 'download-host-contracts', scenarios: ['partial-write', 'plan-discard-releases-lease', 'export-file-count-boundaries', 'export-byte-boundaries', 'credential-free-checkpoint',
    'entry-membership', 'bounded-mutations', 'exclusive-asset-removal', 'close-physical-barrier', 'removed-restoration',
    'picker-provider-uri', 'save-cancel', 'save-partial-rollback', 'preserve-user-file', 'lazy-share', 'share-retention',
    'serialized-export-disk-admission', 'verified-byte-resume-after-restart', 'crash-suffix-truncation', 'corrupt-prefix-restart',
    'If-Range-etag-change-rejection', 'failed-task-source-refresh', 'long-user-title-preserved',
    'disk-partition', 'disk-corruption', 'disk-budget-lru', 'disk-clear'], fetches }));
}
main().catch(error => { console.error(error); process.exitCode = 1; });
