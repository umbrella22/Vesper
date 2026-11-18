'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('../../../lib/harmony/tests/support/load.cjs');
class PlayerError extends Error {
  constructor(code, message) { super(message); this.code = code; }
}
const load = createLoader(name => {
  if (name === '@kit.ArkTS') return { url: { URL } };
  if (name === '@vesper/player-kit') return { VesperPlayerError: PlayerError };
  throw new Error(`Unexpected module ${name}`);
});
const support = load(path.resolve(__dirname, '../entry/src/main/ets/model/ExampleSupport.ets'));
const source = support.remoteSource('  https://example.test/movie.m3u8?token=a&format=.mpd  ');
assert.equal(source.protocol, 'hls');
assert.equal(source.uri, 'https://example.test/movie.m3u8?token=a&format=.mpd');
assert.equal(source.label, 'Stream from example.test');
assert.equal(support.remoteSource('https://example.test/api?file=movie.m3u8').protocol, 'progressive');
assert.equal(support.remoteSource('https://example.test/api?token=secret', 'dash').protocol, 'dash');
for (const input of ['ftp://example.test/movie.mp4', 'file:///private/movie.mp4', 'not a URL', 'https://user:secret@example.test/movie']) {
  assert.throws(() => support.remoteSource(input), error => !error.message.includes('secret') && ['invalid_argument', 'unsupported'].includes(error.code));
}
assert.equal(support.downloadPercent({ receivedBytes: '50', totalBytes: '100' }), 50);
for (const snapshot of [{ receivedBytes: '50' }, { receivedBytes: '10', totalBytes: '0' }, { receivedBytes: '101', totalBytes: '100' }]) {
  assert.equal(support.downloadPercent(snapshot), undefined);
}
let entries = [];
for (let id = 0; id < 100; id++) entries = support.appendLog(entries, { id, title: 'test' });
assert.equal(entries.length, 80);
assert.equal(entries[0].id, 99);
assert.equal(entries[79].id, 20);
assert.equal(support.formatTime(undefined), '--:--');
assert.equal(support.formatTime(125000), '2:05');
console.log(JSON.stringify({ status: 'PASS', contracts: ['signed URL path detection', 'explicit protocol', 'credential-free errors and labels', 'unknown download progress', 'bounded diagnostic history', 'timeline formatting'] }));
