'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const native = require(process.env.VESPER_HARMONY_HOST_MODULE);
const { createLoader } = require('./support/load.cjs');
const load = createLoader(name => { if (name === 'libvesper_player_harmony_node.so') return { default: native }; throw new Error(name); });
const { queryVideoDecoderCapability } = load(path.resolve(__dirname, '../vesper-player-kit/src/main/ets/DecoderCapabilities.ets'));
const capability = queryVideoDecoderCapability({ mimeType: 'video/avc', width: 1920, height: 1080 });
assert.equal(capability.status, 'unavailable'); assert.equal(capability.decoderExecutionEvidence, 'unknown');
assert.equal(capability.hardware, undefined, 'host tests cannot manufacture device capability evidence');
for (const query of [{ mimeType: 'audio/mp4a-latm' }, { mimeType: 'video/avc', width: 1920 },
  { mimeType: 'video/avc', width: 1.5, height: 1080 }, { mimeType: 'video/avc', profile: 0 },
  { mimeType: 'video/avc', width: 2147483648, height: 1080 }]) {
  assert.throws(() => queryVideoDecoderCapability(query), error => ['invalid_argument', 'unsupported'].includes(error.code));
}
assert.throws(() => native.queryVideoDecoder('video/avc', 1.5, 1080, -1, -1), error => error.code === 'invalid_argument');
console.log(JSON.stringify({ status: 'PASS', suite: 'decoder-capability-preflight-boundary' }));
