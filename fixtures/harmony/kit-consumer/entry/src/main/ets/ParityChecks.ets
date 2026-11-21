import { common } from '@kit.AbilityKit';
import { avSession } from '@kit.AVSessionKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { VesperPlayerController, VesperSourceSession, VesperExternalSubtitleSource,
  VesperSystemSession, VesperSystemMediaIdentity, VesperPlayerError } from '@vesper/player-kit';

interface CaptionFile { path: string; text: string; }

async function until(ready: () => boolean, label: string): Promise<void> {
  const deadline = Date.now() + 15000;
  while (!ready()) {
    if (Date.now() >= deadline) { throw new Error('Parity observation deadline: ' + label); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 50); });
  }
}

/** Uses native audio tags, the sidecar adapter and the ability's real AVSession. */
export async function runParityChecks(context: common.UIAbilityContext, surface: string): Promise<string> {
  const sources = new VesperSourceSession(); const controller = new VesperPlayerController(context); controller.attachSurface(surface);
  const results: string[] = []; let system: VesperSystemSession | undefined; let stage = 'audio-aliases';
  let publishHostMetadata = false; let metadataKey = '';
  const unsubscribe = controller.subscribe((snapshot): void => {
    if (!publishHostMetadata || system === undefined || snapshot.sessionId === undefined || snapshot.sourceId === undefined || snapshot.playbackEpoch === undefined) { return; }
    const key = snapshot.sessionId + ':' + snapshot.sourceId + ':' + snapshot.playbackEpoch;
    if (key === metadataKey) { return; }
    system.updateMetadata({ sessionId: snapshot.sessionId, sourceId: snapshot.sourceId, playbackEpoch: snapshot.playbackEpoch },
      { title: 'Queue source ' + snapshot.sourceId });
    metadataKey = key;
  });
  const local = context.filesDir + '/parity-caption.vtt';
  const malformed = context.filesDir + '/parity-malformed.vtt';
  try {
    const files: CaptionFile[] = [{ path: local, text: 'WEBVTT\n\n00:00.000 --> 00:11.900\nStable external caption\n\n' },
      { path: malformed, text: 'not a WebVTT document' }];
    for (const item of files) {
      const output = await fs.open(item.path, fs.OpenMode.CREATE | fs.OpenMode.TRUNC | fs.OpenMode.WRITE_ONLY);
      try { await fs.write(output.fd, item.text); } finally { await fs.close(output); }
    }
    const audio = sources.register({ kind: 'remote', protocol: 'progressive', uri: 'http://127.0.0.1:18765/tracks.mp4', label: 'Alias tracks' });
    try {
      await controller.activate(audio);
      for (const language of ['ja', 'en']) {
        const preference = await controller.setTrackPreferences({ audioLanguages: [language] });
        await until(() => controller.snapshot().trackSelections?.audio?.outcome === 'confirmed', language);
        const selected = controller.snapshot().tracks.find((track) => track.kind === 'audio' && track.selected);
        if (selected?.language !== (language === 'ja' ? 'jpn' : 'eng') || preference.audio.status !== 'applied') {
          throw new Error('Native ISO 639 audio preference did not select ' + language);
        }
        results.push(JSON.stringify({ preference: preference, selected: selected }));
      }
    } finally { audio.close(); }
    await controller.setTrackPreferences({});
    stage = 'subtitle-identity-and-errors';
    const captions: VesperExternalSubtitleSource[] = [
      { id: 'forced-en', uri: local, mimeType: 'text/vtt', language: 'en', label: 'Forced English', isForced: true },
      { id: 'default-ja', uri: local, mimeType: 'text/vtt', language: 'ja', label: 'Default Japanese', isDefault: true },
      { id: 'parse-error', uri: malformed, mimeType: 'text/vtt', label: 'Invalid subtitle' },
      { id: 'load-error', uri: context.filesDir + '/parity-missing.vtt', mimeType: 'text/vtt', label: 'Missing subtitle' }
    ];
    const subtitles = sources.register({ kind: 'remote', protocol: 'progressive', uri: 'http://127.0.0.1:18765/tracks.mp4',
      label: 'Subtitle state', externalSubtitles: captions });
    try {
      await controller.activate(subtitles, { playWhenReady: false });
      const initial = controller.snapshot(); const state = initial.subtitleState;
      if (state?.catalogState !== 'partial' || state.selectedStableId !== 'default-ja' || state.selectionState !== 'selected'
        || !state.externalTracks.some((track) => track.id === 'parse-error' && track.error?.stage === 'parse')
        || !state.externalTracks.some((track) => track.id === 'load-error' && track.error?.stage === 'load')) {
        throw new Error('External default identity or separate failure stages were lost');
      }
      const unavailable = initial.tracks.find((track) => track.stableId === 'parse-error');
      if (unavailable === undefined || unavailable.selectionSupported !== false) { throw new Error('Failed sidecar must remain unavailable in the catalog'); }
      let rejected = false;
      try { await controller.selectTrack({ kind: 'subtitle', mode: 'track', trackId: unavailable.id }); }
      catch (_) { rejected = true; }
      if (!rejected || controller.snapshot().subtitleState?.selectionError?.stage !== 'select') { throw new Error('Subtitle selection failure was not exposed'); }
      await controller.selectTrack({ kind: 'subtitle', mode: 'off' });
      if (controller.snapshot().subtitleState?.selectionState !== 'off' || controller.snapshot().subtitle !== '') { throw new Error('Subtitle Off retained state or text'); }
      results.push(JSON.stringify({ externalCatalog: initial, off: controller.snapshot().subtitleState }));
    } finally { subtitles.close(); }
    stage = 'forced-default';
    const forced = sources.register({ kind: 'remote', protocol: 'progressive', uri: 'http://127.0.0.1:18765/tracks.mp4',
      label: 'Forced caption', externalSubtitles: [captions[0]] });
    try {
      await controller.activate(forced, { playWhenReady: false });
      if (controller.snapshot().subtitleState?.selectedStableId !== 'forced-en') { throw new Error('Forced text did not follow native eng audio'); }
      results.push(JSON.stringify({ forced: controller.snapshot().subtitleState }));
    } finally { forced.close(); }
    stage = 'source-reset-and-metadata';
    const clean = sources.register({ kind: 'remote', protocol: 'progressive', uri: 'http://127.0.0.1:18765/probe.mp4', label: 'Clean source' });
    try { await controller.activate(clean, { playWhenReady: false }); } finally { clean.close(); }
    const cleanState = controller.snapshot().subtitleState;
    if (cleanState?.catalogState !== 'empty' || cleanState.externalTracks.length !== 0 || cleanState.selectionError !== undefined) { throw new Error('Source replacement retained subtitle failures'); }
    system = await VesperSystemSession.create(context, controller, { bundleName: 'dev.vesper.harmony.kitconsumer',
      abilityName: 'EntryAbility', title: 'Fallback title' });
    const playback = controller.snapshot();
    const identity: VesperSystemMediaIdentity = { sessionId: playback.sessionId!, sourceId: playback.sourceId!, playbackEpoch: playback.playbackEpoch! };
    system.updateMetadata(identity, { title: 'Current media', artist: 'Vesper fixture' });
    await until(() => system?.snapshot().metadataState === 'published' || system?.snapshot().metadataState === 'failed', 'metadata');
    if (system.snapshot().metadataState !== 'published') { throw new Error(JSON.stringify(system.snapshot())); }
    {
      const native = await avSession.getAVSession(context); const observer = await native.getController();
      try {
        const published = await observer.getAVMetadata();
        if (published.title !== 'Current media' || published.artist !== 'Vesper fixture') { throw new Error('Native metadata differs from the current media'); }
        system.updateMetadata(identity);
        await until(() => system?.snapshot().metadataState === 'cleared', 'metadata clear');
        const cleared = await observer.getAVMetadata();
        if (cleared.title !== 'Fallback title' || (cleared.artist ?? '') !== '' || (cleared.mediaImage ?? '') !== '') { throw new Error('Native metadata retained old artist or artwork'); }
        system.setControls({ play: false, seek: false, setSpeed: false });
        await until(() => system?.snapshot().supportedControls?.length === 2, 'controls');
        const commands = await observer.getValidCommands();
        if (commands.includes('play') || commands.includes('seek') || commands.includes('setSpeed') || !commands.includes('pause')) { throw new Error('Disabled native system commands remained registered'); }
        results.push(JSON.stringify({ nativeMetadata: published, nativeCleared: cleared, nativeCommands: commands }));
      } finally { await observer.destroy(); }
    }
    stage = 'stale-metadata';
    publishHostMetadata = true;
    const replacement = sources.register({ kind: 'remote', protocol: 'progressive', uri: 'http://127.0.0.1:18765/portrait.mp4', label: 'New source' });
    try { await controller.activate(replacement, { playWhenReady: false }); } finally { replacement.close(); }
    let stale = '';
    try { system.updateMetadata(identity, { title: 'Late old title' }); } catch (error) { stale = (error as VesperPlayerError).code; }
    if (stale !== 'stale_epoch') { throw new Error('Late host metadata crossed source identity'); }
    const current = controller.snapshot();
    await until(() => system?.snapshot().metadataState === 'published' && system?.snapshot().metadataSourceId === current.sourceId, 'ordered host metadata');
    {
      const native = await avSession.getAVSession(context); const observer = await native.getController();
      try {
        const published = await observer.getAVMetadata();
        if (published.title !== 'Queue source ' + current.sourceId) { throw new Error('The later system observer cleared valid host metadata'); }
        results.push(JSON.stringify({ orderedHostMetadata: published, hostObserverRegisteredFirst: true, hostDeduplication: true }));
      } finally { await observer.destroy(); }
    }
    results.push(JSON.stringify({ sourceReset: cleanState, staleMetadata: stale, system: system.snapshot() }));
    return JSON.stringify(results);
  } catch (error) { throw new Error(JSON.stringify({ stage: stage, error: `${error}`, observations: results, playback: controller.snapshot(), system: system?.snapshot() })); }
  finally { unsubscribe(); await system?.close(); await controller.dispose(); sources.close(); }
}
