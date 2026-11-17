'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('../../../../lib/harmony/tests/support/load.cjs');
const load = createLoader(name => { throw new Error(name); });
const { prepareAndPlayNative } = load(path.resolve(__dirname, '../entry/src/main/ets/NativePlayerChecks.ets'));

async function main() {
  const calls = [];
  const player = {
    state: 'initialized',
    async prepare() {
      calls.push('prepare-resolved');
      setTimeout(() => { this.state = 'prepared'; calls.push('state-prepared'); }, 5);
    },
    async play() {
      assert.equal(this.state, 'prepared', 'play must wait for the native state after prepare resolves');
      calls.push('play');
    },
  };
  await prepareAndPlayNative(player, 1000);
  assert.deepEqual(calls, ['prepare-resolved', 'state-prepared', 'play']);
  let plays = 0;
  await assert.rejects(prepareAndPlayNative({ state: 'initialized', async prepare() {}, async play() { plays++; } }, 1),
    /Native prepared-state observation deadline/);
  assert.equal(plays, 0, 'a missing prepared notification must time out without issuing play');
  const failure = new Error('prepare rejected');
  await assert.rejects(prepareAndPlayNative({ state: 'initialized', async prepare() { throw failure; }, async play() { plays++; } }),
    error => error === failure);
  assert.equal(plays, 0);
  console.log(JSON.stringify({ status: 'PASS', suite: 'native-prepare-promise-before-state-and-bounded-wait' }));
}
main().catch(error => { console.error(error); process.exitCode = 1; });
