'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('../../../lib/harmony/tests/support/load.cjs');
class PlayerError extends Error { constructor(code, message) { super(message); this.code = code; } }
const load = createLoader(name => { if (name === '@vesper/player-kit') return { VesperPlayerError: PlayerError }; throw new Error(name); });
const { runDiagnosticScenario } = load(path.resolve(__dirname, '../entry/src/main/ets/model/DiagnosticScenario.ets'));
(async () => {
  const controller = { async play() {}, async refreshDiagnostics() {}, snapshot: () => ({ tracks: [], timeline: 'live' }) };
  assert.deepEqual(await runDiagnosticScenario(controller, 'alternate-audio', () => {}), { scenario: 'alternate-audio', outcome: 'unsupported', errorCode: 'unsupported' });
  assert.equal((await runDiagnosticScenario(controller, 'tail-completion', () => {})).outcome, 'unsupported');
  let closed = false;
  controller.refreshDiagnostics = async () => { closed = true; };
  await assert.rejects(runDiagnosticScenario(controller, 'progress', () => { if (closed) throw new PlayerError('closed', 'closed'); }), error => error.code === 'closed');
  closed = false; controller.play = async () => { throw new PlayerError('https://private.test/token', 'private'); };
  assert.equal((await runDiagnosticScenario(controller, 'progress', () => {})).errorCode, 'unknown');
  console.log(JSON.stringify({ status: 'PASS', suite: 'diagnostic-scenario-admission-and-page-close' }));
})().catch(error => { console.error(error); process.exitCode = 1; });
