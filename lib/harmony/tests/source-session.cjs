/* Host contract regressions; the ArkTS compiler and simulator remain separate gates. */
'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { Worker } = require('node:worker_threads');
const { randomUUID } = require('node:crypto');
const { createLoader } = require('./support/load.cjs');
const native = require(process.env.VESPER_HARMONY_HOST_MODULE);
let failOperation;
let failPhysicalRelease = false;
const rtspSessions = new Set();
const rtspPlaylist = 'http://127.0.0.1:12345/preview/index.m3u8';
const proxy = {
  create: configuration => {
    if (JSON.parse(configuration).kind !== 'rtsp') return native.create(configuration);
    const handle = randomUUID();
    rtspSessions.add(handle);
    return handle;
  },
  call: (handle, command) => {
    if (rtspSessions.has(handle)) {
      const op = JSON.parse(command).op;
      assert.ok(['open', 'status'].includes(op));
      return JSON.stringify({ ok: true, value: op === 'open' ? true : { phase: 'ready', playlistUri: rtspPlaylist } });
    }
    if (failOperation !== undefined && JSON.parse(command).op === failOperation) {
      failOperation = undefined;
      throw Object.assign(new Error('Injected capacity failure'), { code: '3' });
    }
    return native.call(handle, command);
  },
  release: handle => {
    if (rtspSessions.delete(handle)) return;
    if (failPhysicalRelease) {
      failPhysicalRelease = false;
      throw Object.assign(new Error('Injected release failure'), { code: '4' });
    }
    native.release(handle);
  },
};
let createPlayer;
let createMediaSource = () => ({ setMediaResourceLoaderDelegate() {} });
let openFile = async () => ({ fd: 7 });
let closedFiles = 0;
let networkAllocations = 0;
let networkCloses = 0;
let fetchResource;
const fakeRcp = {
  Request: class { constructor(uri, method, headers) { this.uri = uri; this.method = method; this.headers = headers; } },
  createSession() {
    networkAllocations++;
    return { fetch: request => fetchResource(request), cancel() {}, close() { networkCloses++; } };
  },
};
function fakePlayer() {
  const callbacks = new Map();
  return {
    state: 'idle', duration: 12000, width: 320, height: 180, releases: 0,
    _videoScaleType: 0,
    get videoScaleType() { return this._videoScaleType; },
    set videoScaleType(value) {
      assert.ok(['prepared', 'playing', 'paused', 'completed'].includes(this.state), 'video scaling requires native preparation');
      this._videoScaleType = value; this.scaleConfiguredAt = this.state;
    },
    on(name, callback) { callbacks.set(name, callback); },
    emit(name, ...values) { callbacks.get(name)?.(...values); },
    set fdSrc(value) { this.state = 'initialized'; this.emit('stateChange', this.state); },
    async setMediaSource(source, strategy) { this.strategy = strategy; this.state = 'initialized'; this.emit('stateChange', this.state); },
    async prepare() { this.state = 'prepared'; this.emit('stateChange', this.state); },
    async play() { this.state = 'playing'; this.emit('stateChange', this.state); },
    async pause() { this.state = 'paused'; this.emit('stateChange', this.state); },
    async release() { this.releases++; if (this.failRelease) { this.failRelease = false; throw new Error('injected'); } },
    async getTrackDescription() { return [{ track_type: 0, track_index: 0 }]; },
    async getSelectedTracks() { return [0]; },
    async getSeekableTimeRanges() { return []; },
    seekToDefaultPosition() { throw new Error('A non-seekable native source must never receive this call'); },
    setVolume() {},
  };
}
const load = createLoader(name => {
    if (name === 'libvesper_player_harmony_node.so') return { default: proxy };
    if (name === '@kit.BasicServicesKit') return { deviceInfo: { sdkApiVersion: 26 }, systemDateTime: { TimeType: { STARTUP: 0 }, getUptime: () => performance.now() } };
    if (name === '@kit.MediaKit') return { media: { createAVPlayer: () => createPlayer(), createMediaSourceWithUrl: () => createMediaSource(), VideoScaleType: { VIDEO_SCALE_TYPE_SCALED_ASPECT: 2 }, SeekMode: { SEEK_CLOSEST: 0, SEEK_PREV_SYNC: 1 }, LoadingRequestError: { LOADING_ERROR_SUCCESS: 0 }, BufferingInfoType: { BUFFERING_START: 1, BUFFERING_END: 2, BUFFERING_PERCENT: 3, CACHED_DURATION: 4 } } };
    if (name === '@kit.AudioKit') return { audio: { InterruptMode: { INDEPENDENT_MODE: 1 },
      InterruptHint: { INTERRUPT_HINT_PAUSE: 2, INTERRUPT_HINT_STOP: 3, INTERRUPT_HINT_RESUME: 1,
        INTERRUPT_HINT_DUCK: 4, INTERRUPT_HINT_UNDUCK: 5, INTERRUPT_HINT_MUTE: 6, INTERRUPT_HINT_UNMUTE: 7 }, InterruptForceType: { INTERRUPT_SHARE: 1 } } };
    if (name === '@kit.CoreFileKit') return { fileIo: { OpenMode: { READ_ONLY: 0 }, open: () => openFile(), stat: async () => ({ size: 100 }), close: async () => { closedFiles++; } } };
    if (name === '@kit.RemoteCommunicationKit') return { rcp: fakeRcp };
    if (name === '@kit.NetworkKit') return require('./support/network.cjs');
    if (name === '@kit.CryptoArchitectureKit') return { cryptoFramework: {} };
    if (name === '@kit.ArkTS') return { url: { URL }, util: { generateRandomUUID: randomUUID, TextDecoder: { create: () => ({ decodeWithStream: value => Buffer.from(value).toString('utf8') }) } } };
    throw new Error('Unexpected host test import: ' + name);
});

async function main() {
  const { VesperPlayerError } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/Types.ets'));
  assert.deepEqual(JSON.parse(JSON.stringify(new VesperPlayerError('native_failure', 'Native operation failed', 5400102))),
    { name: 'VesperPlayerError', message: 'Native operation failed', code: 'native_failure', nativeCode: 5400102 });
  const { VesperSourceSession } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/SourceSession.ets'));
  const { OperationScope, VesperCancellationToken } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/Operation.ets'));
  const cancellation = new VesperCancellationToken();
  cancellation.observe(() => { throw new Error('Injected observer failure'); });
  const scope = new OperationScope(1000, cancellation);
  scope.onFailure(() => { throw new Error('Injected native cancellation failure'); });
  const pending = scope.wait(new Promise(() => {}));
  cancellation.cancel();
  await assert.rejects(pending, error => error.code === 'cancelled');
  assert.equal(cancellation.observerFailures, 1);
  assert.equal(scope.callbackFailures, 1);
  scope.close();
  const session = new VesperSourceSession({ maxRegisteredSources: 1 });
  const source = { uri: 'https://example.test/video.mp4', label: 'Test', kind: 'remote', protocol: 'progressive', headers: { Authorization: 'initial' } };
  const handle = session.register(source);
  source.headers.Authorization = 'changed';
  const lease = handle.acquireLease();
  assert.equal(lease.descriptor.headers.Authorization, 'initial');
  failOperation = 'close';
  assert.throws(() => session.close(), error => error.code === '3');
  assert.equal(session.snapshot().closed, false);
  session.close();
  assert.equal(session.snapshot().closed, true);
  lease.validateActivation();
  failOperation = 'releaseLease';
  assert.throws(() => lease.release(), error => error.code === '3');
  assert.equal(session.snapshot().leases, 1);
  failPhysicalRelease = true;
  assert.throws(() => lease.release(), error => error.code === '4');
  assert.equal(session.snapshot().leases, 0);
  lease.release();
  lease.release();
  assert.equal(session.snapshot().leases, 0);
  assert.equal(session.snapshot().capacity, 1);
  const revoked = new VesperSourceSession();
  revoked.invalidate();
  assert.equal(revoked.snapshot().invalidated, true);
  const { VesperPlayerController } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/Controller.ets'));
  let sourceReference;
  createMediaSource = () => { const source = { setMediaResourceLoaderDelegate() {} }; sourceReference = new WeakRef(source); return source; };
  const retainedSourceSession = new VesperSourceSession();
  const retainedSourceHandle = retainedSourceSession.register({ uri: 'https://example.test/video.mp4', kind: 'remote', protocol: 'progressive', label: 'Retained native loader' });
  const retainedSourcePlayer = fakePlayer(); createPlayer = async () => retainedSourcePlayer;
  let initializeSource;
  retainedSourcePlayer.setMediaSource = function () {
    initializeSource = () => { this.state = 'initialized'; this.emit('stateChange', this.state); };
  };
  const retainedSourceController = new VesperPlayerController();
  const retainedSourceActivation = retainedSourceController.activate(retainedSourceHandle);
  retainedSourceActivation.catch(() => {});
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(retainedSourcePlayer.state, 'idle', 'Void setMediaSource must wait for the native initialized state');
  initializeSource();
  await retainedSourceActivation;
  assert.equal(retainedSourcePlayer.videoScaleType, 2, 'native presentation must preserve the video aspect ratio');
  assert.equal(retainedSourcePlayer.scaleConfiguredAt, 'prepared');
  retainedSourcePlayer.emit('durationUpdate', 4200);
  assert.equal(retainedSourceController.snapshot().durationMs, 4200, 'VOD must accept duration updates');
  assert.equal(retainedSourceController.snapshot().timeline, 'vod');
  await new Promise(resolve => setImmediate(resolve)); global.gc();
  assert.notEqual(sourceReference.deref(), undefined, 'MediaSource owns native loader callbacks and must survive activation GC');
  let releaseRetainedSource;
  retainedSourcePlayer.release = () => new Promise(resolve => { releaseRetainedSource = resolve; });
  const retainedSourceDisposal = retainedSourceController.dispose();
  await new Promise(resolve => setImmediate(resolve)); global.gc();
  assert.notEqual(sourceReference.deref(), undefined, 'MediaSource must remain alive until physical AVPlayer release finishes');
  releaseRetainedSource(); await retainedSourceDisposal;
  for (let attempt = 0; attempt < 10; attempt++) {
    await new Promise(resolve => setImmediate(resolve)); global.gc();
    if (sourceReference.deref() === undefined) break;
  }
  assert.equal(sourceReference.deref(), undefined, 'physical release must clear the MediaSource ownership');
  retainedSourceHandle.close(); retainedSourceSession.close();
  createMediaSource = () => ({ setMediaResourceLoaderDelegate() {} });
  const adaptiveSources = new VesperSourceSession();
  const adaptiveHandle = adaptiveSources.register({ uri: 'https://example.test/master.m3u8', kind: 'remote', protocol: 'hls', label: 'Native adaptive transport' });
  const adaptivePlayer = fakePlayer(); createPlayer = async () => adaptivePlayer;
  const adaptiveController = new VesperPlayerController(); await adaptiveController.activate(adaptiveHandle);
  assert.deepEqual(adaptivePlayer.strategy, { preferredBufferDuration: 2 }, 'native HLS prefetch must use the bounded buffer hint');
  assert.equal(JSON.parse(adaptiveController.snapshot().resourceLoaderInfo).route, 'native-http');
  const firstBuffering = adaptiveController.snapshot().nativeBuffering;
  assert.equal(firstBuffering.status, 'accepted');
  assert.deepEqual(firstBuffering.submitted, { preferredBufferDurationMs: 2000 });
  const bufferRequest = { preferredBufferDurationMs: 10000, preferredBufferDurationForPlayingMs: 500 };
  const bufferPolicy = await adaptiveController.setPlaybackPolicy({ buffering: bufferRequest, retry: { maxAttempts: 2 } });
  assert.ok(bufferPolicy.parameters.filter(item => item.parameter.startsWith('buffering.')).every(item => item.status === 'deferred' && item.effective === undefined));
  bufferRequest.preferredBufferDurationMs = 20000;
  assert.deepEqual(adaptiveController.snapshot().nativeBuffering, firstBuffering, 'mid-playback policy must not claim it changed the active native buffer');
  assert.deepEqual(adaptivePlayer.strategy, { preferredBufferDuration: 2 });
  for (const buffering of [{ preferredBufferDurationMs: 500 }, { preferredBufferDurationMs: 21000 },
    { preferredBufferDurationMs: 1500 }, { preferredBufferDurationForPlayingMs: 21000 },
    { preferredBufferDurationMs: 2000, preferredBufferDurationForPlayingMs: 3000 }]) {
    await assert.rejects(adaptiveController.setPlaybackPolicy({ buffering, retry: { maxAttempts: 3 } }), error => error.code === 'invalid_argument');
  }
  assert.equal(adaptiveController.transport.configuredAttempts(), 2, 'invalid native buffer preferences must preserve the entire preceding policy');
  const tunedPlayer = fakePlayer(); createPlayer = async () => tunedPlayer;
  await adaptiveController.activate(adaptiveHandle);
  assert.deepEqual(tunedPlayer.strategy, { preferredBufferDuration: 10, preferredBufferDurationForPlaying: 0.5 }, 'next activation preserves the fractional playback threshold');
  assert.notEqual(adaptiveController.snapshot().nativeBuffering.playbackEpoch, firstBuffering.playbackEpoch);
  assert.deepEqual(adaptiveController.snapshot().nativeBuffering.submitted, { preferredBufferDurationMs: 10000, preferredBufferDurationForPlayingMs: 500 });
  await adaptiveController.setPlaybackPolicy({ buffering: {} });
  const resetPlayer = fakePlayer(); createPlayer = async () => resetPlayer;
  await adaptiveController.activate(adaptiveHandle);
  assert.deepEqual(resetPlayer.strategy, { preferredBufferDuration: 2 }, 'empty buffering resets native preferences for the next activation');
  await adaptiveController.dispose(); adaptiveHandle.close(); adaptiveSources.close();
  assert.equal(adaptiveController.snapshot().retainedNativePlayers, 0);

  // The host binary can disable RTSP registration. Exercise activation with
  // an accepted lease and a stub loopback bridge in either feature build.
  const rtsp = { acquireLease: () => ({
    descriptor: { uri: 'rtsp://example.test/clock', kind: 'remote', protocol: 'rtsp', label: 'RTSP' },
    sourceId: 'rtsp', sessionId: 'rtsp-contract', validateActivation() {}, release() {},
  }) };
  const rtspController = new VesperPlayerController();
  try {
    const defaultPlayer = fakePlayer(); createPlayer = async () => defaultPlayer;
    const priorNetworkAllocations = networkAllocations;
    await rtspController.activate(rtsp);
    assert.deepEqual(defaultPlayer.strategy, { preferredBufferDuration: 2 });
    assert.equal(rtspController.snapshot().durationMs, undefined);
    defaultPlayer.emit('durationUpdate', 4200);
    assert.equal(rtspController.snapshot().durationMs, undefined, 'An RTSP playlist window is not a media duration');
    assert.equal(rtspController.snapshot().timeline, 'live');
    assert.equal(rtspController.diagnosticReport().protocol, 'rtsp');
    const baseline = rtspController.snapshot().nativeBuffering;
    assert.equal(baseline.status, 'accepted');
    assert.deepEqual(baseline.submitted, { preferredBufferDurationMs: 2000 });

    await rtspController.setPlaybackPolicy({ buffering: { preferredBufferDurationMs: 6000, preferredBufferDurationForPlayingMs: 500 } });
    const explicitPlayer = fakePlayer(); createPlayer = async () => explicitPlayer;
    await rtspController.activate(rtsp);
    assert.deepEqual(explicitPlayer.strategy, { preferredBufferDuration: 6, preferredBufferDurationForPlaying: 0.5 });
    const current = rtspController.snapshot().nativeBuffering;
    assert.notEqual(current.playbackEpoch, baseline.playbackEpoch);
    assert.equal(current.playbackEpoch, rtspController.snapshot().playbackEpoch);
    assert.deepEqual(current.submitted, { preferredBufferDurationMs: 6000, preferredBufferDurationForPlayingMs: 500 });
    assert.equal(networkAllocations, priorNetworkAllocations, 'The RTSP loopback is passed directly to MediaSource');
  } finally {
    await rtspController.dispose();
  }
  assert.equal(rtspSessions.size, 0);

  const playbackSources = new VesperSourceSession();
  const local = playbackSources.register({ uri: '/sandbox/media.mp4', kind: 'local', protocol: 'file', label: 'Local' });
  const initialLease = local.acquireLease();
  local.close();
  const retainedLease = initialLease.retain();
  initialLease.release(); retainedLease.validateActivation(); retainedLease.release();
  const playable = playbackSources.register({ uri: '/sandbox/media.mp4', kind: 'local', protocol: 'file', label: 'Local' });
  const creations = [];
  createPlayer = () => new Promise(resolve => creations.push(resolve));
  const controller = new VesperPlayerController();
  const first = controller.activate(playable).then(() => 'unexpected', error => error.code);
  const { AssetPins } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/FileStore.ets'));
  assert.equal(AssetPins.contains('/sandbox'), true, 'accepted local activation pins its asset before native creation');
  await new Promise(resolve => setImmediate(resolve));
  const second = controller.activate(playable);
  await new Promise(resolve => setImmediate(resolve));
  const latePlayer = fakePlayer(); const activePlayer = fakePlayer();
  creations[0](latePlayer); creations[1](activePlayer);
  assert.equal(await first, 'superseded'); await second;
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(latePlayer.releases, 1, 'late creation must physically release');
  const epoch = controller.snapshot().playbackEpoch;
  latePlayer.emit('timeUpdate', 9999);
  assert.equal(controller.snapshot().positionMs, 0, 'stale callback must not update current epoch');
  assert.equal(controller.snapshot().playbackEpoch, epoch);
  activePlayer.failRelease = true;
  await assert.rejects(controller.dispose());
  await controller.dispose();
  assert.equal(activePlayer.releases, 2, 'native cleanup failure must be retryable');
  assert.equal(playbackSources.snapshot().leases, 0);
  const preceding = fakePlayer(); const replacement = fakePlayer();
  let finishPrecedingRelease;
  preceding.release = async function () { this.releases++; await new Promise(resolve => { finishPrecedingRelease = resolve; }); };
  createPlayer = async () => preceding;
  const handoff = new VesperPlayerController(); handoff.attachSurface('one-native-Surface');
  await handoff.activate(playable);
  createPlayer = async () => replacement;
  const replacing = handoff.activate(playable);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(preceding.releases, 1);
  assert.equal(replacement.state, 'idle', 'replacement must not initialize or prepare while its predecessor owns the Surface');
  assert.equal(replacement.surfaceId, undefined, 'replacement must not attach the shared Surface before physical release');
  finishPrecedingRelease(); await replacing;
  assert.equal(replacement.surfaceId, 'one-native-Surface');
  replacement.seek = function (position) { this.emit('seekDone', position - 50); };
  handoff.drainEvents(); await handoff.seek(500);
  const seekEvents = handoff.drainEvents();
  assert.equal(seekEvents.find(event => event.type === 'seekRequested').positionMs, 500);
  assert.equal(seekEvents.find(event => event.type === 'seekDone').positionMs, 450);
  assert.equal(handoff.snapshot().positionMs, 450, 'seek completion preserves the native observed position');
  await handoff.dispose(); assert.equal(playbackSources.snapshot().leases, 0);
  const delayedOpenPlayer = fakePlayer(); createPlayer = async () => delayedOpenPlayer;
  let finishOpen;
  openFile = () => new Promise(resolve => { finishOpen = resolve; });
  const delayed = new VesperPlayerController();
  const activation = delayed.activate(playable).then(() => 'unexpected', error => error.code);
  await new Promise(resolve => setImmediate(resolve));
  const beforeClose = closedFiles;
  const disposal = delayed.dispose();
  assert.equal(await activation, 'disposed');
  assert.equal(playbackSources.snapshot().leases, 1, 'pending fd open retains its source lease');
  finishOpen({ fd: 8 }); await disposal;
  assert.equal(closedFiles, beforeClose + 1, 'late fd must close after physical release');
  assert.equal(playbackSources.snapshot().leases, 0);
  playbackSources.close();
  const bounded = new VesperSourceSession({ maxRegisteredSources: 1 });
  for (let index = 0; index < 100; index++) bounded.register(source).close();
  assert.equal(bounded.caches.size, 0, 'closed cache scopes must be reclaimed');
  bounded.close();
  const reentrantSources = new VesperSourceSession();
  const reentrantHandle = reentrantSources.register({ uri: '/sandbox/media.mp4', kind: 'local', protocol: 'file', label: 'Local' });
  let resolveReentrant;
  createPlayer = () => new Promise(resolve => { resolveReentrant = resolve; });
  const reentrant = new VesperPlayerController();
  let reentrantDisposal;
  reentrant.subscribe(snapshot => { if (snapshot.state === 'initializing') reentrantDisposal = reentrant.dispose(); });
  const reentrantActivation = reentrant.activate(reentrantHandle).then(() => 'unexpected', error => error.code);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(reentrantSources.snapshot().leases, 1);
  const reentrantPlayer = fakePlayer(); resolveReentrant(reentrantPlayer);
  assert.equal(await reentrantActivation, 'disposed'); await reentrantDisposal;
  assert.equal(reentrantPlayer.releases, 1, 'subscriber reentrancy must retain physical creation ownership');
  reentrantSources.close();
  const { Transport } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/Transport.ets'));
  const { NativeResourceLoader } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/ResourceLoader.ets'));
  const intervals = []; const intervalHeaders = []; let finishInterval; let deliverInterval;
  let finishedIntervals = 0;
  const intervalDone = new Promise(resolve => { finishInterval = resolve; });
  const intervalDelivered = new Promise(resolve => { deliverInterval = resolve; });
  const finiteLoader = new NativeResourceLoader({ metadata: async () => ({ totalBytes: 20, contentType: 'video/mp4', finalUrl: 'https://example.test/media.mp4' }),
    read: async (_, range) => new Uint8Array(range.end - range.start + 1).buffer }, error => { throw error; });
  const finiteDelegate = finiteLoader.delegate();
  const finiteId = finiteDelegate.open({ url: 'https://example.test/media.mp4', respondHeader(id, header) { intervalHeaders.push(header); },
    respondData(id, offset, bytes) { intervals.push([id, offset, bytes.byteLength]); deliverInterval(); return bytes.byteLength; },
    finishLoading(id, status) { assert.equal(id, finiteId); assert.equal(status, 0); finishedIntervals++; finishInterval(); } });
  finiteDelegate.read(finiteId, 5, 3);
  await intervalDelivered;
  assert.equal(finishedIntervals, 0, 'an interior range must not falsely mark the whole resource as EOF');
  finiteDelegate.read(finiteId, 17, 3);
  await Promise.race([intervalDone, new Promise((_, reject) => setTimeout(() => reject(new Error('Finite loading interval did not complete')), 1000))]);
  assert.deepEqual(intervals, [[finiteId, 5, 3], [finiteId, 17, 3]], 'finite SegmentBase reads retain absolute byte offsets');
  assert.deepEqual(intervalHeaders.map(header => [header['content-length'], header['Content-Range']]),
    [['3', 'bytes 5-7/20'], ['3', 'bytes 17-19/20']], 'native response headers describe the actual interval and resource size');
  finiteLoader.dispose(); assert.equal(finiteLoader.activeResources(), 0);
  const invalidTransport = new Transport();
  const beforeAllocate = networkAllocations;
  await assert.rejects(invalidTransport.get('not a URL', { headers: {}, maxBytes: 1 }));
  assert.equal(networkAllocations, beforeAllocate, 'invalid URL must not allocate a native network session');
  const network = new Transport(); const interval = { headers: {}, maxBytes: 3, range: { start: 2, end: 4 }, allowFullResponse: true };
  fetchResource = async request => { request.destination(Uint8Array.from([0, 1, 2, 3, 4, 5]).buffer); return { statusCode: 200, headers: { 'content-length': '6', etag: '"one"' } }; };
  assert.deepEqual(Array.from(new Uint8Array((await network.get('https://example.test/media', interval)).bytes)), [2, 3, 4]);
  await assert.rejects(network.get('https://example.test/media', { ...interval, validator: '"old"' }), error => error.code === 'resource_changed');
  fetchResource = async () => ({ statusCode: 416, headers: {} });
  await assert.rejects(network.get('https://example.test/media', interval), error => error.code === 'http_416');
  let attempts = 0;
  fetchResource = async request => { if (++attempts < 3) return { statusCode: 503, headers: {} }; request.destination(Uint8Array.from([2, 3, 4]).buffer); return { statusCode: 206, headers: { 'content-range': 'bytes 2-4/6' } }; };
  await network.get('https://example.test/media', { ...interval, attempts: 3 }); assert.equal(attempts, 3);
  fetchResource = async request => { request.destination(Uint8Array.from([2, 3, 4]).buffer); return { statusCode: 206, headers: { 'content-range': 'bytes 2-4/6', etag: '"new"' } }; };
  const { NetworkResourceProvider } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/ResourceLoader.ets'));
  const cacheDouble = { metadata: () => ({ totalBytes: 6, etag: '"old"', contentType: 'video/mp4' }), read: () => undefined,
    diskMetadata: async () => undefined, diskRead: async () => undefined, diskWrite: async () => {} };
  const provider = new NetworkResourceProvider(network, cacheDouble, {}, 'https://example.test/media');
  const readToken = new VesperCancellationToken(); await provider.metadata('https://example.test/media', readToken);
  await assert.rejects(provider.read('https://example.test/media', { start: 2, end: 4 }, readToken), error => error.code === 'resource_changed');
  cacheDouble.metadata = () => ({ totalBytes: 6, etag: '"old"', contentType: 'application/vnd.apple.mpegurl' });
  const mutable = new NetworkResourceProvider(network, cacheDouble, {}, 'https://example.test/playlist');
  await mutable.metadata('https://example.test/playlist', readToken);
  fetchResource = async request => { assert.equal(request.headers['If-Range'], undefined); request.destination(Uint8Array.from([2, 3, 4]).buffer); return { statusCode: 206, headers: { 'content-range': 'bytes 2-4/6', etag: '"new"' } }; };
  await mutable.read('https://example.test/playlist', { start: 2, end: 4 }, readToken);
  const { loadDashText } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/DashText.ets'));
  const audioMpd = Buffer.from('<MPD type="static" mediaPresentationDuration="PT12S"><Period><AdaptationSet contentType="audio" mimeType="audio/mp4" lang="en"><Representation id="audio" bandwidth="128000" codecs="mp4a.40.2"><BaseURL>en.mp4</BaseURL><SegmentBase indexRange="100-199"><Initialization range="0-99"/></SegmentBase></Representation></AdaptationSet></Period></MPD>');
  const audioManifestScope = new OperationScope(1000);
  const audioManifestUri = 'https://example.test/dash.mpd';
  assert.deepEqual(await loadDashText({ metadata: async () => ({ totalBytes: audioMpd.byteLength, finalUrl: audioManifestUri }),
    read: async uri => { assert.equal(uri, audioManifestUri, 'The text reader must leave audio resources to AVPlayer'); return Uint8Array.from(audioMpd).buffer; } },
    audioManifestUri, audioManifestScope), []);
  audioManifestScope.close();
  const preloadSources = new VesperSourceSession({ preload: { workers: 1, pendingCapacity: 1 } });
  const warmOne = preloadSources.register(source); const warmTwo = preloadSources.register(source);
  let exitFetch;
  fetchResource = () => new Promise(resolve => { exitFetch = resolve; });
  const activeWarmup = warmOne.preload(); const queuedWarmup = warmTwo.preload();
  queuedWarmup.cancel(); await assert.rejects(queuedWarmup.completion, error => error.code === 'cancelled');
  assert.equal(JSON.parse(preloadSources.preloadInventory()).pending, 0);
  assert.equal(preloadSources.snapshot().leases, 1, 'queued cancellation releases only its unstarted lease');
  activeWarmup.cancel(); await assert.rejects(activeWarmup.completion, error => error.code === 'cancelled');
  assert.equal(JSON.parse(preloadSources.preloadInventory()).active, 1, 'logical cancellation retains the physical worker');
  assert.equal(preloadSources.snapshot().leases, 1);
  preloadSources.close();
  const beforeNetworkClose = networkCloses;
  exitFetch({ statusCode: 206, headers: {} });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(networkCloses, beforeNetworkClose + 1);
  assert.equal(preloadSources.snapshot().leases, 0);
  const { VesperPlaybackSequence } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/Sequence.ets'));
  const sequenceSources = new VesperSourceSession();
  const one = sequenceSources.register({ uri: '/one.mp4', kind: 'local', protocol: 'file', label: 'One' });
  const two = sequenceSources.register({ uri: '/two.mp4', kind: 'local', protocol: 'file', label: 'Two' });
  const sequence = new VesperPlaybackSequence({ sequenceId: 'host-sequence' });
  sequence.replace([
    { itemId: 'one', providerNamespace: 'test', contentIdentity: 'one', mediaKind: 'vod', source: one },
    { itemId: 'two', providerNamespace: 'test', contentIdentity: 'two', mediaKind: 'vod', source: two },
  ], 'one');
  one.close(); two.close();
  assert.equal(sequenceSources.snapshot().leases, 2);
  createPlayer = async () => fakePlayer(); openFile = async () => ({ fd: 9 });
  const sequencedController = new VesperPlayerController(); sequence.attach(sequencedController);
  assert.equal(sequencedController.snapshot().state, 'idle', 'membership must not start playback');
  await sequence.activate(); const firstSequenceEpoch = sequencedController.snapshot().playbackEpoch;
  await assert.rejects(sequencedController.activate(one), error => error.code === 'sequence_owned');
  sequence.next();
  assert.equal(sequencedController.snapshot().playbackEpoch, firstSequenceEpoch, 'navigation must not auto-activate');
  await sequence.activate();
  assert.notEqual(sequencedController.snapshot().playbackEpoch, firstSequenceEpoch);
  sequence.close();
  assert.equal(sequenceSources.snapshot().leases, 1, 'sequence close preserves the independent active player lease');
  await sequencedController.dispose(); sequenceSources.close();
  const expirySources = new VesperSourceSession();
  const expiring = expirySources.register({ uri: '/expiry.mp4', kind: 'local', protocol: 'file', label: 'Expiry' }, Date.now() + 100);
  const refreshed = expirySources.register({ uri: '/refreshed.mp4', kind: 'local', protocol: 'file', label: 'Refreshed' });
  const expirySequence = new VesperPlaybackSequence({ sequenceId: 'expiry', sourceExpiryLeadMs: 0 });
  expirySequence.replace([{ itemId: 'expiry', providerNamespace: 'test', contentIdentity: 'expiry', mediaKind: 'vod', source: expiring }], 'expiry');
  await new Promise(resolve => setTimeout(resolve, 130));
  let resolutions = 0;
  await expirySequence.resolvePending(async () => { resolutions++; return { source: refreshed, sourceRevision: '2' }; });
  assert.equal(resolutions, 1, 'expired source must enter normal resolver refresh');
  assert.equal(expirySequence.snapshot().items[0].item.sourceState.sourceRevision, '2');
  expirySequence.close(); expiring.close(); refreshed.close(); expirySources.close();
  const orderedSources = new VesperSourceSession();
  const orderedSource = orderedSources.register({ uri: '/order.mp4', kind: 'local', protocol: 'file', label: 'Order' });
  const orderedPlayer = fakePlayer(); createPlayer = async () => orderedPlayer;
  const ordered = new VesperPlayerController(); await ordered.activate(orderedSource);
  const immediatePause = orderedPlayer.pause;
  let deliverPaused; let pauseSettled = false;
  orderedPlayer.pause = async () => {
    orderedPlayer.state = 'paused';
    deliverPaused = () => orderedPlayer.emit('stateChange', 'paused');
  };
  const pausingWithDelayedCallback = ordered.pause().then(() => { pauseSettled = true; });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(pauseSettled, false, 'native paused state must not resolve before the observable snapshot callback');
  deliverPaused(); await pausingWithDelayedCallback;
  assert.equal(ordered.snapshot().state, 'paused');
  orderedPlayer.pause = immediatePause;
  await ordered.play();
  let filter = {}; let filterCalls = 0;
  orderedPlayer.setTrackSelectionFilter = async value => { filterCalls++; filter = structuredClone(value); };
  orderedPlayer.getTrackSelectionFilter = async () => ({ ...filter, minVideoBitrate: 0 });
  const filterRequest = { maxVideoResolution: { width: 1280, height: 720 }, minVideoResolution: { width: 160, height: 90 },
    preferredAudioLanguages: ['en-US', 'zh-Hant'], preferredSubtitleLanguages: [] };
  const filterWork = ordered.setTrackConstraints(filterRequest);
  filterRequest.maxVideoResolution.width = 9999; filterRequest.preferredAudioLanguages[0] = 'fr';
  const readback = await filterWork;
  assert.deepEqual(readback.maxVideoResolution, { width: 1280, height: 720 });
  assert.deepEqual(readback.preferredAudioLanguages, ['en-US', 'zh-Hant']);
  assert.equal(readback.minVideoBitrate, 0, 'native zero sentinel must survive the API boundary');
  const acceptedFilter = ordered.snapshot().trackConstraints;
  assert.equal(acceptedFilter.status, 'accepted');
  assert.equal(acceptedFilter.playbackEpoch, ordered.snapshot().playbackEpoch);
  readback.maxVideoResolution.width = 2; readback.preferredAudioLanguages.push('de');
  assert.deepEqual(ordered.snapshot().trackConstraints, acceptedFilter, 'returned arrays and resolution must not alias retained diagnostics');
  for (const invalid of [{ maxVideoBitrate: NaN }, { maxVideoFrameRate: Infinity }, { maxVideoResolution: { width: 0, height: 720 } },
    { minVideoResolution: { width: 1280, height: 720 }, maxVideoResolution: { width: 640, height: 360 } },
    { preferredAudioLanguages: Array(9).fill('en') }, { preferredSubtitleLanguages: ['en_US'] }]) {
    await assert.rejects(ordered.setTrackConstraints(invalid), error => error.code === 'invalid_argument');
  }
  assert.equal(filterCalls, 1, 'invalid filter input must never reach Media Kit');
  orderedPlayer.setTrackSelectionFilter = async () => { throw Object.assign(new Error('Rejected'), { code: 5400102 }); };
  await assert.rejects(ordered.setTrackConstraints({}), error => error.nativeCode === 5400102);
  assert.equal(ordered.snapshot().trackConstraints.status, 'failed');
  assert.equal(ordered.snapshot().state, 'playing', 'a rejected filter must preserve healthy playback');
  orderedPlayer.emit('bufferingUpdate', 1, 0); assert.equal(ordered.snapshot().buffering, true);
  orderedPlayer.emit('bufferingUpdate', 3, 35); assert.equal(ordered.snapshot().bufferingPercent, 35);
  orderedPlayer.emit('bufferingUpdate', 4, 2400); assert.equal(ordered.snapshot().cachedDurationMs, 2400);
  orderedPlayer.emit('bufferingUpdate', 999, 123);
  assert.deepEqual(ordered.snapshot().lastBufferingUpdate, { nativeType: 999, value: 123 });
  assert.equal(ordered.drainEvents().at(-1).type, 'bufferingUnknown');
  orderedPlayer.emit('bufferingUpdate', 2, 0); assert.equal(ordered.snapshot().buffering, false);
  const policy = await ordered.setPlaybackPolicy({ buffering: { minBufferMs: 1000 }, retry: { maxAttempts: 3 }, decoder: 'hardwareOnly' });
  assert.equal(policy.parameters.find(item => item.parameter === 'buffering.minBufferMs').status, 'unsupported');
  assert.equal(policy.parameters.find(item => item.parameter === 'decoder').effective, 'system');
  assert.equal(policy.parameters.find(item => item.parameter === 'decoder').status, 'unsupported');
  assert.equal(policy.parameters.find(item => item.parameter === 'retry.maxAttempts').effective, 3);
  fetchResource = async () => ({ statusCode: 503, headers: {} });
  const policyRequests = ordered.transport.requests;
  await assert.rejects(ordered.transport.get('https://example.test/policy', { headers: {}, maxBytes: 1 }), error => error.code === 'http_503');
  assert.equal(ordered.transport.requests - policyRequests, 3, 'applied transport policy executes exactly three attempts');
  await assert.rejects(ordered.setPlaybackPolicy({ retry: { maxAttempts: 2 }, buffering: { minBufferMs: -1 } }), error => error.code === 'invalid_argument');
  assert.equal(ordered.transport.configuredAttempts(), 3, 'invalid combined policy must not partly apply');
  await ordered.setPlaybackPolicy({ retry: { maxAttempts: 1 }, decoder: 'system' });
  ordered.current.sidecars = [[{ startMs: 0, endMs: 1000, text: 'Embedded' }]];
  ordered.current.sidecarLanguages = ['en']; ordered.current.embeddedText = true;
  orderedPlayer.getTrackDescription = async () => [{ track_type: 0, track_index: 0 }, { track_type: 2, track_index: 2, codec_mime: 'text/vtt' }];
  orderedPlayer.getSelectedTracks = async () => [0, 2];
  orderedPlayer.deselectTrack = async () => { throw new Error('Native text deselection is outside its audio/video contract'); };
  await ordered.refreshTracks(ordered.current);
  await ordered.selectTrack({ kind: 'subtitle', mode: 'track', trackId: -2 });
  assert.equal(ordered.snapshot().subtitle, 'Embedded');
  orderedPlayer.seek = position => { orderedPlayer.emit('seekDone', position - 25); };
  await ordered.seek(1200);
  assert.equal(ordered.snapshot().subtitle, '', 'a paused seek must clear an expired sidecar without a timeUpdate callback');
  ordered.drainEvents();
  const pausedSeekSnapshots = [];
  const stopPausedSeekSnapshots = ordered.subscribe(snapshot => pausedSeekSnapshots.push(snapshot));
  await ordered.seek(600);
  assert.equal(ordered.snapshot().positionMs, 575);
  assert.equal(ordered.snapshot().subtitle, 'Embedded', 'a paused seek must restore the cue at the native completed position');
  assert.equal(pausedSeekSnapshots.at(-1).subtitle, 'Embedded', 'binding observers receive the restored cue');
  stopPausedSeekSnapshots();
  await ordered.selectTrack({ kind: 'subtitle', mode: 'off' }); assert.equal(ordered.snapshot().subtitle, '');
  orderedPlayer.emit('subtitleUpdate', { text: 'late native caption', startTime: 0, duration: 1000 });
  assert.equal(ordered.snapshot().subtitle, '', 'a delayed native callback must not resurrect disabled captions');
  ordered.current.embeddedText = false;
  orderedPlayer.emit('subtitleUpdate', { text: 'late ordinary caption', startTime: 0, duration: 1000 });
  assert.equal(ordered.snapshot().subtitle, '', 'off also fences the ordinary native text route');
  await assert.rejects(ordered.seekToDefaultPosition(), error => error.code === 'unsupported');
  assert.equal(ordered.snapshot().error, undefined, 'non-DVR rejection must leave native playback healthy');
  orderedPlayer.emit('audioInterrupt', { eventType: 1, forceType: 1, hintType: 2 });
  await new Promise(resolve => setImmediate(resolve)); assert.equal(ordered.snapshot().state, 'paused');
  await ordered.pause(); orderedPlayer.emit('audioInterrupt', { eventType: 2, forceType: 1, hintType: 1 });
  await new Promise(resolve => setImmediate(resolve)); assert.equal(ordered.snapshot().state, 'paused', 'User pause cancels interruption resume');
  await ordered.play(); orderedPlayer.emit('audioInterrupt', { eventType: 1, forceType: 1, hintType: 2 });
  await new Promise(resolve => setImmediate(resolve)); orderedPlayer.emit('audioInterrupt', { eventType: 2, forceType: 1, hintType: 1 });
  await new Promise(resolve => setImmediate(resolve)); assert.equal(ordered.snapshot().state, 'playing');
  let cleanupDone; const cleanup = new Promise(resolve => { cleanupDone = resolve; });
  ordered.registerCleanup(() => cleanup); const orderedDisposal = ordered.dispose();
  await new Promise(resolve => setImmediate(resolve)); assert.equal(orderedPlayer.releases, 0);
  cleanupDone(); await orderedDisposal; assert.equal(orderedPlayer.releases, 1);
  orderedSource.close(); orderedSources.close();
  const preferenceSources = new VesperSourceSession();
  const preferenceSource = preferenceSources.register({ uri: '/preferences.mp4', kind: 'local', protocol: 'file', label: 'Preferences' });
  const preferencePlayer = fakePlayer(); const selections = []; let selectedAudio = 0;
  preferencePlayer.getTrackDescription = async () => [
    { track_type: 0, track_index: 0, language: 'zh-CN' }, { track_type: 0, track_index: 1, language: 'en-US' }];
  preferencePlayer.getSelectedTracks = async () => [selectedAudio];
  preferencePlayer.selectTrack = async id => { selections.push(id); selectedAudio = id; preferencePlayer.emit('trackChange', id, true); };
  createPlayer = async () => preferencePlayer;
  const preferred = new VesperPlayerController(); const configured = { audioLanguages: ['en'], subtitleMode: 'off' };
  assert.equal((await preferred.setTrackPreferences(configured)).audio.status, 'deferred'); configured.audioLanguages[0] = 'zh';
  await preferred.activate(preferenceSource); assert.deepEqual(selections, [1], 'copied preferences apply once after the catalog becomes ready');
  assert.equal(preferred.snapshot().trackPreferences.audio.status, 'applied');
  await preferred.selectTrack({ kind: 'audio', mode: 'track', trackId: 0 });
  assert.deepEqual(preferred.snapshot().tracks.filter(track => track.kind === 'audio' && track.selected).map(track => track.id), [0],
    'a confirmed new audio track must retire the preceding native callback hint');
  preferencePlayer.emit('trackInfoUpdate', [{ track_type: 0, track_index: 3, language: 'en' }]);
  await new Promise(setImmediate); assert.deepEqual(selections, [1, 0], 'catalog updates cannot override a later explicit selection');
  preferencePlayer.selectTrack = async () => { throw new Error('selection rejected'); };
  await assert.rejects(preferred.selectTrack({ kind: 'audio', mode: 'track', trackId: 1 }), error => error.code === 'native_failure');
  assert.equal(preferred.snapshot().trackSelections.audio.effective.trackId, 0, 'failed selection retains the previously confirmed choice');
  await assert.rejects(preferred.selectTrack({ kind: 'audio', mode: 'track', trackId: 999 }), error => error.code === 'invalid_argument');
  let finishSelection, beganSelection;
  const selectionStarted = new Promise(resolve => { beganSelection = resolve; });
  preferencePlayer.selectTrack = () => new Promise(resolve => { finishSelection = resolve; beganSelection(); });
  const staleSelection = preferred.selectTrack({ kind: 'audio', mode: 'track', trackId: 1 }).then(() => 'unexpected', error => error.code);
  await selectionStarted; const freshPlayer = fakePlayer(); createPlayer = async () => freshPlayer;
  const freshActivation = preferred.activate(preferenceSource); finishSelection(); await staleSelection; await freshActivation;
  assert.equal(preferred.snapshot().trackSelections.audio, undefined, 'a late selection result cannot populate the replacement epoch');
  await preferred.dispose();
  const pausedAudioPlayer = fakePlayer(); let pausedAudio = 0; const pausedAudioSelections = []; let pausedAudioPlays = 0;
  pausedAudioPlayer.getTrackDescription = async () => [
    { track_type: 0, track_index: 0, language: 'spa' }, { track_type: 0, track_index: 1, language: 'eng' }];
  pausedAudioPlayer.getSelectedTracks = async () => [pausedAudio];
  pausedAudioPlayer.selectTrack = async id => {
    assert.equal(pausedAudioPlayer.state, 'playing', 'paused native reads cannot finish an adaptive audio switch');
    pausedAudioSelections.push(id); pausedAudio = id;
  };
  pausedAudioPlayer.play = async () => { pausedAudioPlays++; pausedAudioPlayer.state = 'playing'; pausedAudioPlayer.emit('stateChange', 'playing'); };
  pausedAudioPlayer.seek = () => { throw new Error('A paused track choice cannot seek or resume playback'); };
  createPlayer = async () => pausedAudioPlayer;
  const pausedAudioController = new VesperPlayerController(); await pausedAudioController.activate(preferenceSource);
  await pausedAudioController.play(); await pausedAudioController.pause(); pausedAudioPlayer.emit('timeUpdate', 2836);
  const pausedAudioEpoch = pausedAudioController.snapshot().playbackEpoch;
  const pausedAudioPlaysBeforeSelection = pausedAudioPlays;
  await pausedAudioController.selectTrack({ kind: 'audio', mode: 'track', trackId: 1 });
  const pausedAudioSnapshot = pausedAudioController.snapshot();
  assert.equal(pausedAudioSnapshot.trackSelections.audio.outcome, 'pending');
  assert.equal(pausedAudioSnapshot.tracks.find(track => track.id === 1).selected, false);
  assert.deepEqual(pausedAudioSelections, []);
  assert.equal(pausedAudioSnapshot.positionMs, 2836); assert.equal(pausedAudioSnapshot.state, 'paused');
  assert.equal(pausedAudioSnapshot.playbackEpoch, pausedAudioEpoch); assert.equal(pausedAudioPlays, pausedAudioPlaysBeforeSelection, 'track selection cannot resume paused playback');
  await pausedAudioController.selectTrack({ kind: 'audio', mode: 'track', trackId: 0 });
  assert.equal(pausedAudioController.snapshot().trackSelections.audio.outcome, 'confirmed', 'choosing the current track cancels a deferred choice');
  await pausedAudioController.selectTrack({ kind: 'audio', mode: 'track', trackId: 1 });
  await pausedAudioController.play();
  assert.deepEqual(pausedAudioSelections, [1]);
  assert.equal(pausedAudioController.snapshot().trackSelections.audio.outcome, 'confirmed');
  assert.equal(pausedAudioController.snapshot().tracks.find(track => track.id === 1).selected, true);
  assert.equal(pausedAudioController.snapshot().positionMs, 2836); assert.equal(pausedAudioController.snapshot().playbackEpoch, pausedAudioEpoch);
  await pausedAudioController.pause(); await pausedAudioController.selectTrack({ kind: 'audio', mode: 'track', trackId: 0 });
  createPlayer = async () => fakePlayer(); await pausedAudioController.activate(preferenceSource);
  assert.deepEqual(pausedAudioSelections, [1], 'a deferred intent cannot cross a replacement epoch');
  assert.equal(pausedAudioController.snapshot().trackSelections.audio, undefined);
  await pausedAudioController.dispose();
  const preparing = fakePlayer(); let finishPrepare, beganPrepare;
  const prepareStarted = new Promise(resolve => { beganPrepare = resolve; });
  preparing.prepare = () => new Promise(resolve => { beganPrepare(); finishPrepare = () => { preparing.state = 'prepared'; preparing.emit('stateChange', 'prepared'); resolve(); }; });
  let preparationPlays = 0; preparing.play = async () => { preparationPlays++; };
  createPlayer = async () => preparing; const pausedPreparation = new VesperPlayerController();
  const preparingActivation = pausedPreparation.activate(preferenceSource); await prepareStarted;
  const intentBefore = pausedPreparation.snapshot().intentRevision; await pausedPreparation.pause();
  assert.ok(pausedPreparation.snapshot().intentRevision > intentBefore); assert.equal(pausedPreparation.snapshot().playWhenReady, false);
  finishPrepare(); await preparingActivation;
  assert.equal(preparationPlays, 0, 'pause during preparation must prevent activation autoplay'); await pausedPreparation.dispose();
  preferenceSource.close(); preferenceSources.close();
  {
    const subtitles = new VesperSourceSession();
    const descriptor = { kind: 'remote', protocol: 'progressive', uri: 'https://example.test/subtitle-video.mp4', label: 'Subtitle contracts',
      externalSubtitles: [
        { id: 'forced-en', uri: 'https://example.test/forced.vtt', mimeType: 'text/vtt', label: 'Forced English', language: 'en', isForced: true },
        { id: 'default-ja', uri: 'https://example.test/default.vtt', mimeType: 'text/vtt', label: 'Japanese', language: 'jpn', isDefault: true },
        { id: 'bad-parse', uri: 'https://example.test/invalid.vtt', mimeType: 'text/vtt', label: 'Broken syntax' },
        { id: 'bad-load', uri: 'https://example.test/missing.vtt', mimeType: 'text/vtt', label: 'Missing' }
      ] };
    const handle = subtitles.register(descriptor);
    const player = fakePlayer(); player.getTrackDescription = async () => [{ track_type: 0, track_index: 0, language: 'eng' }];
    createPlayer = async () => player;
    fetchResource = async request => {
      const uri = request.uri;
      if (uri.endsWith('/missing.vtt')) return { statusCode: 404, headers: {} };
      const text = uri.endsWith('/invalid.vtt') ? 'invalid text' : 'WEBVTT\n\n00:00.000 --> 00:02.000\nCaption\n\n';
      request.destination(Uint8Array.from(Buffer.from(text)).buffer);
      return { statusCode: 200, headers: { 'content-type': 'text/vtt', 'content-length': String(Buffer.byteLength(text)) } };
    };
    const controller = new VesperPlayerController(); const snapshots = [];
    const unsubscribe = controller.subscribe(value => snapshots.push(value));
    await controller.activate(handle, { playWhenReady: false });
    const snapshot = controller.snapshot();
    assert.equal(snapshot.error, undefined, 'sidecar failures do not stop audio/video activation');
    assert.ok(snapshots.some(value => value.subtitleState.catalogState === 'loading'));
    assert.equal(snapshot.subtitleState.catalogState, 'partial');
    assert.equal(snapshot.subtitleState.selectionState, 'selected');
    assert.equal(snapshot.subtitleState.selectedStableId, 'default-ja');
    assert.equal(snapshot.tracks.find(track => track.stableId === 'default-ja').label, 'Japanese');
    assert.equal(snapshot.subtitleState.externalTracks.find(track => track.id === 'bad-parse').error.stage, 'parse');
    assert.equal(snapshot.subtitleState.externalTracks.find(track => track.id === 'bad-load').error.stage, 'load');
    const failedTrack = snapshot.tracks.find(track => track.stableId === 'bad-parse');
    await assert.rejects(controller.selectTrack({ kind: 'subtitle', mode: 'track', trackId: failedTrack.id }), error => error.code === 'unsupported');
    assert.equal(controller.snapshot().subtitleState.selectionState, 'failed');
    assert.equal(controller.snapshot().subtitleState.selectionError.stage, 'select');
    await controller.selectTrack({ kind: 'subtitle', mode: 'off' });
    assert.equal(controller.snapshot().subtitleState.selectionState, 'off');
    assert.equal(controller.snapshot().subtitleState.selectedStableId, undefined);
    assert.equal(controller.snapshot().subtitleState.selectionError, undefined);
    const noCaptions = subtitles.register({ ...descriptor, externalSubtitles: [] });
    await controller.activate(noCaptions, { playWhenReady: false });
    assert.equal(controller.snapshot().subtitleState.catalogState, 'empty');
    assert.deepEqual(controller.snapshot().subtitleState.externalTracks, []);
    assert.equal(controller.snapshot().subtitleState.catalogError, undefined);
    unsubscribe(); await controller.dispose(); handle.close(); noCaptions.close(); subtitles.close();
  }
  const { parseSubtitles, subtitleAt } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/Subtitles.ets'));
  const cues = parseSubtitles('WEBVTT\n\n00:00.100 --> 00:01.000\nFirst\n\n00:01.500 --> 00:02.000\nSecond\n', 'text/vtt');
  assert.equal(subtitleAt(cues, 500), 'First'); assert.equal(subtitleAt(cues, 1000), '');
  assert.equal(subtitleAt(cues, 1600), 'Second'); assert.equal(subtitleAt(cues, 50), '', 'seek must clear a previous cue');
  assert.throws(() => parseSubtitles('1\n00:00:02,000 --> 00:00:01,000\nBad', 'application/x-subrip'), error => error.code === 'invalid_subtitle');
  const { parseWvtt } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/Wvtt.ets'));
  const wvtt = fs.readFileSync(process.env.VESPER_WVTT_FIXTURE);
  const wvttBytes = wvtt.buffer.slice(wvtt.byteOffset, wvtt.byteOffset + wvtt.byteLength);
  const embedded = parseWvtt(wvttBytes);
  assert.equal(subtitleAt(embedded, 100), 'Vesper DASH WebVTT');
  assert.equal(subtitleAt(embedded, 8000), ''); assert.equal(subtitleAt(embedded, 50), '');
  assert.throws(() => parseWvtt(wvttBytes.slice(0, -1)), error => error.code === 'unsupported');
  // A truncated final run still has complete box headers but lacks its signed field.
  for (const flags of [0x000001, 0x01000800]) {
    const run = wvtt.indexOf('trun') - 4;
    const truncated = Buffer.from(wvtt.subarray(0, run + 16));
    for (const kind of ['moof', 'traf', 'trun']) {
      const start = truncated.indexOf(kind) - 4;
      truncated.writeUInt32BE(truncated.length - start, start);
    }
    truncated.writeUInt32BE(flags, run + 8);
    truncated.writeUInt32BE(1, run + 12);
    const data = truncated.buffer.slice(truncated.byteOffset, truncated.byteOffset + truncated.byteLength);
    assert.throws(() => parseWvtt(data), error => error.code === 'unsupported');
  }
  // A valid empty movie edit delays the media presentation by one second.
  // The preview rejects edit lists until it maps movie time to media time.
  const edit = Buffer.alloc(48);
  edit.writeUInt32BE(48, 0); edit.write('edts', 4); edit.writeUInt32BE(40, 8); edit.write('elst', 12);
  [0, 2, 1000, 0xffffffff, 0x10000, 12000, 0, 0x10000].forEach((value, index) => edit.writeUInt32BE(value, 16 + index * 4));
  const trak = wvtt.indexOf('trak') - 4; const moov = wvtt.indexOf('moov') - 4;
  const editedText = Buffer.concat([wvtt.subarray(0, trak + 8), edit, wvtt.subarray(trak + 8)]);
  editedText.writeUInt32BE(wvtt.readUInt32BE(trak) + edit.length, trak);
  editedText.writeUInt32BE(wvtt.readUInt32BE(moov) + edit.length, moov);
  assert.throws(() => parseWvtt(editedText.buffer.slice(editedText.byteOffset, editedText.byteOffset + editedText.byteLength)), error => error.code === 'unsupported');
  const oversized = Buffer.from(wvtt); oversized.writeUInt32BE(5000, oversized.indexOf('trun') + 8);
  assert.throws(() => parseWvtt(oversized.buffer.slice(oversized.byteOffset, oversized.byteOffset + oversized.byteLength)), error => error.code === 'unsupported');
  assert.throws(() => native.call(1, '{}'));
  const owned = native.create('{"kind":"sources"}');
  native.release(owned);
  assert.throws(() => native.call(owned, '{"op":"snapshot"}'));
  // One Worker teardown checks that native ownership is scoped to its environment.
  const worker = new Worker(`
    const {parentPort,workerData}=require('node:worker_threads');
    const api=require(workerData);
    for(let i=0;i<64;i++)api.create('{"kind":"sources"}');
    parentPort.postMessage('ready');
    setInterval(()=>{},1000);
  `, { eval: true, workerData: process.env.VESPER_HARMONY_HOST_MODULE });
  await new Promise((resolve, reject) => { worker.once('message', resolve); worker.once('error', reject); });
  await worker.terminate();
  const handles = Array.from({ length: 64 }, () => native.create('{"kind":"sources"}'));
  assert.throws(() => native.create('{"kind":"sources"}'), error => error.code === '3' || error.code === 'capacity');
  for (const handle of handles) native.release(handle);
  process.stdout.write(JSON.stringify({ status: 'PASS', scenarios: ['immutable descriptor', 'cleanup retry after capacity failure', 'physical cleanup retry', 'retained lease after close', 'revocation snapshot', 'stale handle', 'MediaSource survival across GC and physical release', 'serialized Surface ownership', 'native completed seek position', 'finite Range headers and EOF', 'late native creation', 'stale epoch callback', 'retry native release', 'late fd open cleanup', 'bounded full-response Range fallback', 'If-Range resource change', 'HTTP 416', 'bounded 503 retry', 'cached metadata version fence', 'mutable playlist validator', 'explicit sequence activation', 'interruption intent', 'non-DVR rejection', 'subtitle cue timing', 'cleanup barrier', 'worker environment cleanup', 'session capacity restored', 'one-time language preference before catalog', 'explicit selection survives catalog update', 'failed selection preserves effective choice', 'stale track and delayed epoch rejection', 'pause during prepare prevents autoplay'] }) + '\n');
}
main().catch(error => { process.stderr.write(error.stack + '\n'); process.exitCode = 1; });
