'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { mock } = require('node:test');
const { createLoader } = require('./support/load.cjs');
const load = createLoader(name => { throw new Error('Unexpected gesture platform import: ' + name); });
const { gestureIntent } = load(path.resolve(__dirname, '../vesper-player-kit-arkui-ui/src/main/ets/Gestures.ets'));
const { stageClock, stageSeekTarget, StageControlTimer } = load(path.resolve(__dirname, '../vesper-player-kit-arkui-ui/src/main/ets/Controls.ets'));
global.Color = { White: '#FFFFFF', Black: '#000000', Transparent: '#00000000' };
const { stageLayout, stageLoadedRanges, stageQuality, stageTracks, stageTrackSelected, stageTrackDetail, stageLanguageBase, stageCaptionGeometry, VesperSubtitleStyle } = load(path.resolve(__dirname, '../vesper-player-kit-arkui-ui/src/main/ets/Presentation.ets'));
for (const options of [{ fontScale: NaN }, { fontScale: 0 }, { bottomInset: -1 }, { edgeRadius: Infinity }, { maxLines: 1.5 }, { maxLines: 9 }]) {
  assert.throws(() => new VesperSubtitleStyle(options), /Caption style/);
}
const readable = new VesperSubtitleStyle({ fontScale: 2, bottomInset: 0, edgeRadius: 0, maxLines: 2 });
assert.equal(readable.fontScale, 2); assert.equal(readable.bottomInset, 0); assert.equal(readable.maxLines, 2);
assert.deepEqual(stageLoadedRanges({ timeline: 'vod', durationMs: 1000, bufferingPercent: 100, cachedDurationMs: 1000 }), []);
assert.deepEqual(stageLoadedRanges({ timeline: 'vod', durationMs: 1000, timeRanges: { loaded: [{ startMs: 300, endMs: 1500 }] } }), [{ startMs: 300, endMs: 1000 }]);
assert.equal(stageLayout(280, 240, 'compact', 48, 8, 110, 56, 100).splitTime, true);
assert.equal(stageLayout(550, 160, 'expanded', 48, 8, 110, 56, 100).compact, true);
assert.equal(stageLayout(230, 500, 'expanded', 48, 8, 110, 56, 100).quality, true,
  'a 230 vp row fits the quality shortcut, two 48 vp buttons and both 8 vp gaps');
assert.equal(stageLayout(230, 500, 'expanded', 48, 8, 110, 56, 145).quality, false,
  'a longer localized quality label moves into More instead of squeezing a button');
assert.equal(stageLayout(284, 224, 'compact', 48, 8, 76, 56, 100).splitTime, false,
  'a 360 vp example with page and Stage insets keeps a single compact transport row');
assert.equal(stageLayout(244, 224, 'compact', 48, 8, 152, 56, 100).splitTime, true,
  'large text on a 320 vp phone gets its own time line and retains the scrubber');
assert.equal(stageLayout(284, 196, 'expanded', 48, 8, 152, 96, 200, 0,
  { headerHeight: 69, timeHeight: 28 }).compact, true, 'actual text height also drives short-window fallback');
assert.equal(stageLayout(364, 224, 'standard', 48, 8, 76, 56, 100).primary, false,
  'a central play cluster cannot crowd a 248 vp inline Stage');
assert.equal(stageLayout(364, 324, 'standard', 48, 8, 76, 56, 100).primary, true);
assert.equal(stageLayout(244, 224, 'expanded', 48, 8, 76, 56, 100, 180).leading, 0,
  'host actions are hidden as a whole instead of leaving clipped touch targets');
assert.equal(stageLayout(284, 224, 'compact', 48, 8, 76, 56, 100, 0,
  { fullscreenTarget: 96 }).splitTime, true, 'each skin button reserves its actual target');
const quality = { height: 360, width: 640, tracks: [{ id: 3, kind: 'video', height: 720, width: 1280 }], availableBitrates: [],
  selection: { requested: { kind: 'video', mode: 'track', trackId: 3 }, effective: { mode: 'track' }, outcome: 'confirmed' } };
assert.equal(stageQuality(quality).status, 'switching', 'native selection confirmation does not prove observed output');
assert.equal(stageQuality({ ...quality, height: 720, width: 1280 }).status, 'observed');
const video = { id: 1, kind: 'video', selected: true, width: 1280, height: 720, bitrate: 2850000, mimeType: 'video/avc' };
const adaptive = { ...quality, tracks: [video], availableBitrates: [2850000], selection: { confirmedTrackIds: [1] } };
assert.equal(stageQuality(adaptive).mode, 'auto');
assert.equal(stageTrackSelected(adaptive, video), false, 'a native current rendition does not turn default Auto into a fixed choice');
assert.equal(stageTrackSelected({ ...adaptive, selection: { effective: { kind: 'video', mode: 'auto' } } }, video), false,
  'explicit Auto and a native rendition cannot both be checked');
for (const outcome of ['confirmed', 'pending', 'failed']) {
  const snapshot = { ...adaptive, trackSelections: { video: { effective: { kind: 'video', mode: 'track', trackId: 1 },
    requested: { kind: 'video', mode: 'auto' }, outcome } } };
  assert.equal(stageTrackSelected(snapshot, { ...video, selected: false }), true, 'the effective fixed choice survives ' + outcome + ' requests');
}
assert.equal(stageTrackSelected({ ...adaptive, selection: { effective: { kind: 'video', mode: 'track', trackId: 2 } } }, video), false);
assert.equal(stageTrackSelected({ ...adaptive, selection: { effective: { kind: 'video', mode: 'bitrate', bitrate: 2850000 } } }, video), false);
assert.equal(stageTrackSelected({ ...adaptive, availableBitrates: [] }, video), true, 'a single system track retains its native selection');
assert.equal(stageTrackSelected(adaptive, { ...video, kind: 'audio' }), true, 'audio selection remains native');
assert.equal(stageTrackSelected(adaptive, { ...video, kind: 'subtitle' }), true, 'subtitle selection remains native');
const ladder = { ...adaptive, tracks: [video, { ...video, id: 2, height: 180 }, { ...video, id: 3, bitrate: 4300000 },
  { ...video, id: 4, height: 1080 }, { ...video, id: 5, kind: 'audio' }] };
assert.deepEqual(stageTracks(ladder, 'video').map(track => track.id), [4, 3, 1, 2], 'quality sorts by resolution and bitrate');
assert.deepEqual(ladder.tracks.map(track => track.id), [1, 2, 3, 4, 5], 'presentation sorting cannot mutate the controller snapshot');
assert.equal(stageTrackDetail(ladder, video), '1280 × 720 · H.264 · 2850 kbps');
assert.equal(stageTrackDetail(adaptive, video), '1280 × 720 · H.264');
for (const [native, display] of [['eng', 'en'], ['EN-us', 'en'], ['spa', 'es'], ['fra', 'fr'], ['fre', 'fr'], ['deu', 'de'], ['ger', 'de'], ['chi', 'zh'], ['zho-Hant', 'zh']]) {
  assert.equal(stageLanguageBase(native), display, 'native ISO language aliases retain a stable display name');
}
assert.equal(stageLanguageBase('und'), 'und'); assert.equal(stageLanguageBase('el'), 'el');
const safe = { top: 0, bottom: 12, left: 0, right: 0 };
const caption = stageCaptionGeometry(600, 600, 1920, 1080, safe, 100, 16);
assert.ok(caption.bottom > 116, 'letterboxed captions stay inside the actual fitted content');
assert.equal(stageCaptionGeometry(600, 300, 1080, 1920, safe, 0, 16).width, 130);
const tightCaption = stageCaptionGeometry(320, 180, 1920, 1080, safe, 72, 8, 48);
assert.ok(tightCaption.maxHeight >= 0 && tightCaption.bottom + tightCaption.maxHeight <= 180 - 48 - 8,
  'long captions remain below the measured header and above the measured controls');
assert.equal(stageCaptionGeometry(320, 100, 1920, 1080, safe, 72, 8, 48).maxHeight, 0,
  'a Stage with no caption space must suppress text rather than overlap controls');
assert.equal(stageSeekTarget('live', 12000, 4000, 10000), undefined);
assert.equal(stageSeekTarget('vod', undefined, 4000, 10000), undefined);
assert.equal(stageSeekTarget('vod', Infinity, 4000, 10000), undefined);
assert.equal(stageSeekTarget('vod', 12000, NaN, 10000), undefined);
assert.equal(stageSeekTarget('vod', 12000, 4000, -10000), 0);
assert.equal(stageSeekTarget('vod', 12000, 4000, 10000), 12000);
assert.equal(stageClock(undefined), '--:--');
assert.equal(stageClock(3599000), '59:59');
assert.equal(stageClock(3601000), '1:00:01');
mock.timers.enable({ apis: ['setTimeout'] });
try {
  let hidden = 0;
  const timer = new StageControlTimer(() => { hidden++; });
  timer.arm(true); mock.timers.tick(2999); assert.equal(hidden, 0);
  timer.arm(true); mock.timers.tick(1); assert.equal(hidden, 0, 'interaction replaces the prior hide deadline');
  mock.timers.tick(2999); assert.equal(hidden, 1);
  timer.arm(true); timer.arm(false); mock.timers.tick(5000); assert.equal(hidden, 1, 'paused or blocked controls stay visible');
  timer.arm(true); timer.clear(); mock.timers.tick(5000); assert.equal(hidden, 1, 'disposal cancels pending UI updates');
} finally { mock.timers.reset(); }
global.Color = { White: '#ffffff', Black: '#000000' };
global.$r = name => ({ id: name });
const { VesperPlayerStageSkin, VesperStageMetrics, VesperPlayerStageIcons, VesperStageIconRole,
  VesperStageButtonStyle, VesperStageButtonVariant } = load(path.resolve(__dirname, '../vesper-player-kit-arkui-ui/src/main/ets/Skin.ets'));
const firstSkin = new VesperPlayerStageSkin(); const secondSkin = new VesperPlayerStageSkin();
firstSkin.colors.accent = '#35d8ba';
assert.notEqual(firstSkin.colors.accent, secondSkin.colors.accent, 'separate Stages must own independent default palettes');
for (const values of [[43, 8, 22, 20], [44, Infinity, 22, 20], [44, 8, -1, 20], [44, 8, 22, NaN]]) {
  assert.throws(() => new VesperStageMetrics(...values), /Stage metrics/, 'invalid host geometry must be rejected');
}
const icons = new VesperPlayerStageIcons(); const replacement = { id: 'host.play' }; icons.play = replacement;
const customized = new VesperPlayerStageSkin(undefined, undefined, { icons });
assert.equal(customized.icons.resolve(VesperStageIconRole.Play), replacement);
assert.deepEqual(customized.icons.resolve(VesperStageIconRole.Pause), secondSkin.icons.resolve(VesperStageIconRole.Pause),
  'a partial icon override keeps the other native roles');
assert.notEqual(secondSkin.icons.resolve(VesperStageIconRole.Play), replacement, 'one Stage cannot mutate another Stage icon map');
const tinyVisual = new VesperStageButtonStyle(20, 14, 0.2, 0, 44);
const customizedMetrics = new VesperStageMetrics(44, 0, 0, 18, { toolbar: tinyVisual });
assert.equal(customizedMetrics.button(VesperStageButtonVariant.Toolbar), tinyVisual);
assert.ok(customizedMetrics.button(VesperStageButtonVariant.Toolbar).hitTarget >= 44, 'small custom visuals keep a full hit target');
for (const variant of Object.values(VesperStageButtonVariant)) {
  const style = customizedMetrics.button(variant);
  assert.ok(style.hitTarget >= Math.max(44, style.size), 'every role reserves its entire visual and minimum touch target');
}
for (const args of [[38, 22, 1.1], [38, 22, -0.1], [0, 22], [38, Infinity], [38, 22, 0, -1], [38, 22, 0, 0, 43]]) {
  assert.throws(() => new VesperStageButtonStyle(...args), /Stage button styles/);
}
assert.throws(() => new VesperStageMetrics(44, 8, 22, 20, { timelineTrackHeight: NaN }), /Stage metrics/);
assert.throws(() => new VesperStageMetrics(44, 8, 22, 20, { hudIconSize: 0 }), /Stage metrics/);
assert.throws(() => customizedMetrics.button('unknown'), /Unknown Stage button variant/);
assert.throws(() => customized.icons.resolve('unknown'), /Unknown Stage icon role/);
assert.throws(() => new VesperPlayerStageSkin(undefined, undefined, { settingsStyle: 'unknown' }), /Unknown Stage settings style/);
const start = { x: 240, width: 320, height: 180, positionMs: 4000, durationMs: 12000, volume: 0.5, timeline: 'vod' };
assert.equal(gestureIntent({ ...start, timeline: 'live' }, 300, 0, 'horizontal'), undefined, 'live must not acquire a VOD seek');
assert.equal(gestureIntent({ ...start, durationMs: undefined }, 300, 0, 'horizontal'), undefined, 'unknown duration must not acquire a VOD seek');
assert.equal(gestureIntent({ ...start, width: 0 }, 300, 0, 'horizontal'), undefined, 'unmeasured layout cannot produce commands');
assert.equal(gestureIntent(start, Infinity, 0, 'horizontal'), undefined);
assert.equal(gestureIntent(start, -1000, 0, 'horizontal').value, 0, 'seeks cannot pass the start');
assert.equal(gestureIntent(start, 1000, 0, 'horizontal').value, 12000, 'seeks cannot pass duration');
assert.deepEqual(gestureIntent(start, 0, -1000, 'vertical'), { kind: 'volume', value: 1 });
assert.deepEqual(gestureIntent(start, 0, 1000, 'vertical'), { kind: 'volume', value: 0 });
assert.equal(gestureIntent({ ...start, x: 20 }, 0, -90, 'vertical'), undefined, 'missing host brightness never changes volume');
assert.deepEqual(gestureIntent({ ...start, x: 20, brightness: 0.25 }, 0, -90, 'vertical'), { kind: 'brightness', value: 0.75 });
process.stdout.write(JSON.stringify({ status: 'PASS', suite: 'stage-gesture-intent', contracts: ['live and unknown duration rejection', 'layout readiness', 'finite displacement', 'seek boundaries', 'volume boundaries', 'host brightness isolation', 'per-Stage skin isolation', 'skin geometry validation', 'skip admission and clamping', 'hour timeline formatting', 'one replaceable hide deadline', 'pause and disposal cancel hiding'] }) + '\n');
