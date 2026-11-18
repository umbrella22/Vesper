'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('./support/load.cjs');
const load = createLoader(name => {
  if (name === '@kit.BasicServicesKit') return { deviceInfo: { sdkApiVersion: 26, osFullName: 'test-runtime', displayVersion: '26', productModel: 'test-model', deviceType: 'phone' }, systemDateTime: { TimeType: { STARTUP: 0 }, getUptime: () => performance.now() } };
  if (name === '@kit.ArkTS') return { url: { URL } };
  if (name === '@kit.RemoteCommunicationKit') return { rcp: { Request: class {}, createSession: () => ({
    async fetch(request) { request.destination(new Uint8Array([1, 2]).buffer); return { statusCode: 206, headers: { 'content-range': 'bytes 0-1/100', 'content-length': '2' } }; }, cancel() {}, close() {} }) } };
  throw new Error(name);
});
const { DiagnosticHistory } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/DiagnosticReport.ets'));
const history = new DiagnosticHistory();
const snapshot = { state: 'playing', playbackEpoch: '1', positionMs: 50, buffering: false, firstFrame: true,
  tracks: [{ id: 7, kind: 'audio', mimeType: 'audio/aac', language: 'eng', selected: true, label: 'private-label' }],
  error: { code: 'native_failure', nativeCode: 5400102, message: 'https://private.test/media?token=private-token /private/path Authorization: private-header' },
  subtitle: 'private-subtitle', sourceId: 'private-source', resourceLoaderInfo: '/private/resource' };
for (let i = 0; i < 256; i++) history.record('position', snapshot, i * 10);
let report = history.report(snapshot, 'hls', 3000, [], 0, { scenario: 'hls-tail', fixtureId: 'apple-bipbop', runtimeKind: 'simulator', audibleOutputObservation: 'silent' });
assert.equal(report.timeline.length, 3);
assert.equal(report.latest.nativeCode, 5400102);
assert.equal(report.runtimeKindEvidence, 'host-reported');
assert.equal(report.observationEvidence, 'human-reported');
assert.equal(report.actualDecoder, 'unknown');
assert.equal(JSON.stringify(report).includes('private'), false);
for (let i = 0; i < 150; i++) history.record('seekRequested', snapshot, 4000 + i, i);
report = history.report(snapshot, 'dash', 5000, [], 0);
assert.equal(report.timeline.length, 128); assert.equal(report.droppedMoments, 25);
report.timeline[0].selectedTrackIds.push(900);
assert.equal(history.report(snapshot, 'dash', 5000, [], 0).timeline[0].selectedTrackIds.includes(900), false);
assert.throws(() => history.report(snapshot, 'hls', 0, [], 0, { fixtureId: 'https://private.test/token' }), error => error.code === 'invalid_argument');
const { Transport } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/Transport.ets'));
(async () => {
  const transport = new Transport();
  for (let i = 0; i < 40; i++) await transport.get('https://private.test/media?token=private-token', {
    headers: { Authorization: 'private-header' }, range: { start: 0, end: 1 }, maxBytes: 2 });
  const requests = transport.diagnosticRequests();
  assert.equal(requests.length, 32); assert.equal(transport.droppedDiagnosticRequests, 8);
  assert.equal(requests[0].requestId, 9); assert.equal(requests[0].status, 206);
  assert.equal(requests[0].rangeEnd, 1); assert.equal(requests[0].receivedBytes, 2);
  assert.equal(JSON.stringify(requests).includes('private'), false);
  requests[0].status = 999; assert.equal(transport.diagnosticRequests()[0].status, 206);
  console.log(JSON.stringify({ status: 'PASS', suite: 'bounded-private-diagnostic-report' }));
})().catch(error => { console.error(error); process.exitCode = 1; });
