'use strict';
const assert = require('node:assert/strict');
const http = require('node:http');
const net = require('node:net');
const path = require('node:path');
const { randomUUID } = require('node:crypto');
const { createLoader } = require('./support/load.cjs');
const native = require(process.env.VESPER_HARMONY_HOST_MODULE);
const load = createLoader(name => {
  if (name === '@kit.NetworkKit') return require('./support/network.cjs');
  if (name === '@kit.ArkTS') return { url: { URL }, util: { generateRandomUUID: randomUUID,
    TextEncoder: class { encodeInto(value) { return new TextEncoder().encode(value); } },
    TextDecoder: { create: () => ({ decodeWithStream: value => new TextDecoder('utf-8', { fatal: true }).decode(value) }) } } };
  if (name === '@kit.BasicServicesKit') return { systemDateTime: { TimeType: { STARTUP: 0 }, getUptime: () => performance.now() } };
  if (name === 'libvesper_player_harmony_node.so') return { default: native };
  if (name === '@kit.MediaKit') return { media: {} };
  if (name === '@kit.CoreFileKit') return { fileIo: {} };
  if (name === '@kit.RemoteCommunicationKit') return { rcp: {} };
  throw new Error('Unexpected import ' + name);
});
const { NativeResourceServer, resourceResponseRange } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/ResourceServer.ets'));
const rawRequest = (uri, headers = {}, method = 'GET') => new Promise((resolve, reject) => {
  const request = http.request(uri, { method, headers }, response => {
    const chunks = []; response.on('data', chunk => chunks.push(chunk));
    response.on('end', () => resolve({ status: response.statusCode, headers: response.headers, body: Buffer.concat(chunks) }));
    response.on('error', reject);
  }); request.on('error', reject); request.end();
});

async function main() {
  for (const [input, expected] of [[undefined, [0, 19]], ['bytes=5-', [5, 19]], ['bytes=-5', [15, 19]], ['bytes=3-99', [3, 19]]]) {
    const result = resourceResponseRange(input, 20); assert.deepEqual([result.start, result.end], expected);
  }
  for (const input of ['bytes=20-', 'bytes=-0', 'bytes=5-3', 'bytes=0-1,3-4', 'bytes=9007199254740992-']) assert.equal(resourceResponseRange(input, 20), undefined);
  const media = Buffer.from('0123456789abcdefghijklmnopqrstuv');
  const manifest = Buffer.from('#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100,CODECS="avc1.4d401e,mp4a.40.2"\nhttps://cdn.test/a/low.m3u8?token=x%2Fy\n#EXT-X-STREAM-INF:BANDWIDTH=200,CODECS="avc1.4d401e,ec-3"\nbad.m3u8\n');
  const calls = []; const failures = [];
  const provider = {
    async metadata(uri) { calls.push(['metadata', uri]); return { totalBytes: uri.endsWith('master.m3u8') ? manifest.length : media.length,
      contentType: uri.endsWith('master.m3u8') ? 'application/vnd.apple.mpegurl' : 'video/mp4', finalUrl: uri }; },
    async read(uri, range, token) { calls.push(['read', uri, range]); assert.equal(token.isCancelled, false);
      return Uint8Array.from((uri.endsWith('master.m3u8') ? manifest : media).subarray(range.start, range.end + 1)).buffer; },
  };
  const server = new NativeResourceServer(provider, 'hls', error => failures.push(error));
  const uri = await server.start('https://media.test/a/master.m3u8');
  const headers = server.headers();
  assert.equal((await rawRequest(new URL('/a/master.m3u8', uri))).status, 403, 'an unrelated local client must not use the accepted source context');
  assert.equal(calls.length, 0, 'unauthenticated requests must not fetch data');
  const response = await rawRequest(uri, headers);
  assert.equal(response.status, 200);
  assert.equal(Number(response.headers['content-length']), response.body.length);
  const text = response.body.toString(); assert.ok(!text.includes('ec-3') && !text.includes('bad.m3u8'));
  const child = text.split('\n').find(line => line.startsWith('http://127.0.0.1:'));
  assert.ok(child.endsWith('/a/low.m3u8?token=x%2Fy'));
  const data = await rawRequest(new URL('../file.mp4?x=a%2Fb', child), { ...headers, Range: 'bytes=4-9' });
  assert.equal(data.status, 206); assert.equal(data.headers['content-range'], 'bytes 4-9/32'); assert.equal(data.body.toString(), '456789');
  assert.ok(calls.some(call => call[1] === 'https://cdn.test/file.mp4?x=a%2Fb'));
  const doubledPath = 'https://cdn.test//nested/file.mp4?x=a%2Fb';
  assert.equal((await rawRequest(await server.start(doubledPath), headers)).status, 200);
  assert.ok(calls.some(call => call[1] === doubledPath), 'a double-slash resource path must retain its accepted origin');
  assert.ok(!calls.some(call => call[1].startsWith('https://nested/')));
  const fileUri = await server.start('https://cdn.test/file.mp4');
  const head = await rawRequest(fileUri, headers, 'HEAD');
  assert.equal(head.status, 200); assert.equal(head.headers['content-length'], '32'); assert.equal(head.body.length, 0);
  assert.equal((await rawRequest(fileUri, { ...headers, Range: 'bytes=32-' })).status, 416);
  const before = calls.length;
  assert.equal((await rawRequest(fileUri, { ...headers, Host: 'outside.test' })).status, 403);
  assert.equal(calls.length, before);
  const idle = net.connect(new URL(uri).port, '127.0.0.1'); await new Promise(resolve => idle.once('connect', resolve));
  idle.on('error', () => {}); idle.resume();
  const idleClosed = new Promise(resolve => idle.once('close', resolve));
  const closingAt = performance.now();
  await server.dispose(); await idleClosed;
  assert.ok(performance.now() - closingAt < 1000, 'disposal must close idle clients without waiting for their request deadline');
  assert.equal(server.activeResources(), 0); assert.equal(JSON.parse(server.inventory()).listeners, 0);
  assert.deepEqual(failures, []);
  assert.ok(!server.inventory().includes(new URL(uri).pathname.split('/')[1]), 'diagnostics must not contain the playback capability');
  await server.dispose();
  await assert.rejects(rawRequest(uri, headers), error => error.code === 'ECONNREFUSED');
  let finishMetadata; let observedMetadata;
  const metadataStarted = new Promise(resolve => { observedMetadata = resolve; });
  const slow = new NativeResourceServer({ metadata: async () => {
    observedMetadata(); return new Promise(resolve => { finishMetadata = resolve; });
  }, read: provider.read }, 'hls', error => failures.push(error));
  const slowUri = await slow.start('https://media.test/file.mp4');
  const slowRead = rawRequest(slowUri, slow.headers()).catch(() => undefined);
  await metadataStarted;
  let released = false;
  const closing = slow.dispose().then(() => { released = true; });
  await new Promise(resolve => setTimeout(resolve, 20));
  assert.equal(released, false, 'logical cancellation must retain physical provider work');
  finishMetadata({ totalBytes: media.length, contentType: 'video/mp4', finalUrl: 'https://media.test/file.mp4' });
  await closing; await slowRead;
  assert.equal(slow.activeResources(), 0);
  const network = require('./support/network.cjs'); network.rejectClosedClient = true;
  const raced = new NativeResourceServer(provider, 'hls', error => failures.push(error));
  const racedUri = await raced.start('https://media.test/file.mp4');
  assert.equal((await rawRequest(racedUri, raced.headers())).body.toString(), media.toString());
  await new Promise(resolve => setTimeout(resolve, 20));
  assert.equal(raced.activeResources(), 0, 'a physical close event must collect a connection even when close() rejects');
  await raced.dispose(); network.rejectClosedClient = false;
  let unavailable = true; const transientFailures = [];
  const recovering = new NativeResourceServer({ async metadata(uri, token) {
    if (unavailable) { unavailable = false; const error = new Error('transfer timeout'); error.code = 1007900028; throw error; }
    return provider.metadata(uri, token);
  }, read: provider.read }, 'dash', error => transientFailures.push(error));
  const recoveringUri = await recovering.start('https://media.test/file.mp4');
  assert.equal((await rawRequest(recoveringUri)).status, 502);
  assert.equal((await rawRequest(recoveringUri)).status, 200);
  assert.deepEqual(transientFailures, [], 'a retried HTTP transfer must not mark continuing native playback as failed');
  assert.equal(JSON.parse(recovering.inventory()).requestErrors['native_failure:1007900028'], 1);
  await recovering.dispose();
  const rollingUri = 'https://media.test/rolling.m3u8';
  const rolling = Buffer.from('#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:999\n#EXTINF:2.0,\nsegment999.ts\n');
  let snapshots = 0;
  const consistent = new NativeResourceServer({
    async metadata(uri) { return { totalBytes: 12, contentType: 'application/vnd.apple.mpegurl', finalUrl: uri }; },
    async snapshot(uri) { snapshots++; return { info: { totalBytes: rolling.length, contentType: 'application/vnd.apple.mpegurl', finalUrl: uri }, bytes: Uint8Array.from(rolling).buffer }; },
    async read() { throw new Error('A mutable manifest must not combine separate length and Range versions'); },
  }, 'hls', error => failures.push(error));
  const coherent = await rawRequest(await consistent.start(rollingUri));
  assert.equal(coherent.status, 200); assert.match(coherent.body.toString(), /MEDIA-SEQUENCE:999/);
  assert.equal(Number(coherent.headers['content-length']), coherent.body.length); assert.equal(snapshots, 1);
  await consistent.dispose();
  console.log('Harmony native HTTP resource, codec projection, Range and closure contracts: PASS');
}
main().catch(error => { console.error(error); process.exitCode = 1; });
