'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { randomUUID } = require('node:crypto');
const { createLoader } = require('./support/load.cjs');
const native = require(process.env.VESPER_HARMONY_HOST_MODULE);
let now = 0;
const load = createLoader(name => {
  if (name === '@kit.ArkTS') return { url: { URL }, util: { generateRandomUUID: randomUUID,
    TextEncoder: class { encodeInto(value) { return new TextEncoder().encode(value); } },
    TextDecoder: { create: () => ({ decodeWithStream: value => new TextDecoder('utf-8', { fatal: true }).decode(value) }) } } };
  if (name === '@kit.BasicServicesKit') return { systemDateTime: { TimeType: { STARTUP: 0 }, getUptime: () => now } };
  if (name === 'libvesper_player_harmony_node.so') return { default: native };
  if (['@kit.MediaKit', '@kit.CoreFileKit', '@kit.RemoteCommunicationKit'].includes(name)) return {};
  throw new Error('Unexpected import ' + name);
});
const { prepareDashPlayback } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/DashLive.ets'));
const { VesperCancellationToken, OperationScope } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/Operation.ets'));
const { VesperPlayerError } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/Types.ets'));
function mpd(first) {
  const sets = [['video', 'avc1.42c01e'], ['audio', 'mp4a.40.2']].map(([kind, codec]) =>
    `<AdaptationSet contentType="${kind}" mimeType="${kind}/mp4"><SegmentTemplate timescale="1000" startNumber="${first + 41}" initialization="${kind}/init.mp4" media="${kind}/$Time$.m4s"><SegmentTimeline><S t="${first * 2000}" d="2000" r="5"/></SegmentTimeline></SegmentTemplate><Representation id="${kind}" codecs="${codec}" bandwidth="100000"/></AdaptationSet>`).join('');
  return `<MPD type="dynamic" minimumUpdatePeriod="PT2S"><Period id="live">${sets}</Period></MPD>`;
}
async function main() {
  const manifestUri = 'https://origin.test/live.mpd'; let first = 0; let snapshots = 0; let failure; let release;
  const upstream = {
    async snapshot(uri) {
      assert.equal(uri, manifestUri); snapshots++;
      if (release) await new Promise(resolve => { release.resolve = resolve; });
      if (failure) throw failure;
      const bytes = Buffer.from(mpd(first));
      return { info: { finalUrl: uri, totalBytes: bytes.length, contentType: 'application/dash+xml' }, bytes: Uint8Array.from(bytes).buffer };
    },
    async metadata(uri) { return { finalUrl: uri, totalBytes: 4, contentType: 'video/mp4' }; },
    async read(uri, range) { assert.match(uri, /^https:\/\/origin\.test\/(?:video|audio)\//); return Uint8Array.from([0, 1, 2, 3].slice(range.start, range.end + 1)).buffer; },
  };
  const scope = new OperationScope(10000);
  await assert.rejects(prepareDashPlayback(upstream, manifestUri, scope), error => error.code === 'unsupported');
  snapshots = 0;
  const prepared = await prepareDashPlayback(upstream, manifestUri, scope, true); scope.close();
  const live = prepared.live; assert.ok(live);
  const token = new VesperCancellationToken();
  const initial = await live.snapshot(live.nativeUrl, 65536, token);
  const master = Buffer.from(initial.bytes).toString(); assert.match(master, /audio\.m3u8/);
  const videoUri = master.split('\n').find(line => line.startsWith('https://') && line.endsWith('video.m3u8'));
  const video = Buffer.from((await live.snapshot(videoUri, 65536, token)).bytes).toString();
  const oldSegment = video.split('\n').find(line => line.endsWith('/segment-0.m4s'));
  const pin = new VesperCancellationToken(); await live.metadata(oldSegment, pin);
  for (first = 1; first <= 20; first++) {
    now += 2001;
    const [a, b] = await Promise.all([live.snapshot(videoUri, 65536, token), live.snapshot(videoUri, 65536, token)]);
    assert.deepEqual(a.bytes, b.bytes); assert.match(Buffer.from(a.bytes).toString(), new RegExp(`MEDIA-SEQUENCE:${first + 41}`));
  }
  assert.equal(snapshots, 21, 'Concurrent video/audio requests share one MPD refresh');
  assert.deepEqual(Array.from(new Uint8Array(await live.read(oldSegment, { start: 0, end: 3 }, pin))), [0, 1, 2, 3], 'An active response pins old resource identity across eviction');
  pin.cancel(); now += 2001; await live.snapshot(videoUri, 65536, token);
  await assert.rejects(live.metadata(oldSegment, new VesperCancellationToken()), error => error.code === 'http_410');
  assert.ok(JSON.parse(live.inventory()).retainedResources < 40, 'Resource retention must not grow with refresh count');
  failure = new VesperPlayerError('http_503', 'fixture outage'); now += 2001;
  await live.snapshot(videoUri, 65536, token);
  assert.equal(JSON.parse(live.inventory()).refreshFailures, 1);
  failure = undefined; first++; now += 2001; await live.snapshot(videoUri, 65536, token);
  release = {}; now += 2001;
  const pending = live.snapshot(videoUri, 65536, token); await new Promise(resolve => setImmediate(resolve));
  let closed = false; const closing = live.dispose().then(() => { closed = true; });
  await new Promise(resolve => setImmediate(resolve)); assert.equal(closed, false, 'Physical refresh work must settle before bridge release');
  release.resolve(); await assert.rejects(pending, error => error.code === 'cancelled'); await closing;
  assert.equal(JSON.parse(live.inventory()).retainedResources, 0); assert.equal(JSON.parse(live.inventory()).activePins, 0);
  await assert.rejects(live.metadata(oldSegment, token), error => error.code === 'cancelled');
  console.log('Harmony dynamic DASH refresh, stable identity, eviction, outage and close contracts: PASS');
}
main().catch(error => { console.error(error); process.exitCode = 1; });
