'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { createLoader } = require('../../../lib/harmony/tests/support/load.cjs');
const load = createLoader(name => { throw new Error(`Unexpected module ${name}`); });
const shell = load(path.resolve(__dirname, '../entry/src/main/ets/model/ExampleShell.ets'));

assert.deepEqual(shell.EXAMPLE_TABS, ['play', 'library', 'downloads', 'lab']);
assert.equal(shell.tabIndex('lab'), 3);
assert.equal(shell.tabIndex('diagnostics'), 0);
assert.equal(shell.tabAt(1), 'library');
for (const index of [-1, 4, 1.5, Number.NaN]) assert.equal(shell.tabAt(index), 'play');
assert.ok(shell.isSheet('settings'));
assert.ok(!shell.isSheet('appearance'));
assert.deepEqual(shell.EXAMPLE_SHEETS, ['open', 'queue', 'tools', 'settings', 'download']);
assert.ok(shell.EXAMPLE_SHEETS.every(name => name.length > 0));
assert.equal(shell.presentSheet('open'), 'open');
for (const name of ['', 'tracks', 'url', 'cast']) assert.equal(shell.presentSheet(name), '');
assert.equal(shell.sheetAfterAction('open', true), '', 'a successful open closes the sheet');
assert.equal(shell.sheetAfterAction('open', false), 'open', 'an invalid URL keeps the sheet open');
assert.equal(shell.sheetAfterAction('download', false), 'download');
assert.equal(shell.sheetAfterAction('download', true), '');
assert.equal(shell.sheetAfterAction('tracks', false), '');

assert.ok(!shell.isWideLayout(839, 1));
assert.ok(shell.isWideLayout(840, 1));
assert.ok(!shell.isWideLayout(1000, 1.5));
assert.ok(shell.isWideLayout(1020, 2));
assert.equal(shell.sidebarWidth(600), 320);
assert.equal(shell.sidebarWidth(1200), 360);
assert.equal(shell.sidebarWidth(2000), 400);

const base = { sheet: '', routes: 0, fullscreen: false, tab: 'play' };
assert.equal(shell.shellBackAction(base), 'system');
assert.equal(shell.shellBackAction({ ...base, tab: 'lab' }), 'homeTab');
assert.equal(shell.shellBackAction({ ...base, tab: 'library', routes: 1 }), 'popRoute');
assert.equal(shell.shellBackAction({ ...base, routes: 1, fullscreen: true }), 'exitFullscreen');
assert.equal(shell.shellBackAction({ ...base, fullscreen: true, settingsPane: 'menu' }), 'stagePane');
assert.equal(shell.shellBackAction({ ...base, sheet: 'open', settingsPane: 'menu' }), 'closeSheet');
assert.equal(shell.parentStagePane('menu'), undefined);
assert.equal(shell.parentStagePane('captionStyle'), 'subtitle');
assert.equal(shell.parentStagePane('subtitle'), 'menu');

const items = [{ id: 'a', builtin: true }, { id: 'b', builtin: false }, { id: 'c', builtin: true }];
assert.deepEqual(shell.collectionMembers(items, 'samples').map(item => item.id), ['a', 'c']);
assert.deepEqual(shell.collectionMembers(items, 'custom').map(item => item.id), ['b']);
assert.deepEqual(shell.collectionMembers(items, 'unknown'), []);
console.log('harmony example shell contracts passed');
