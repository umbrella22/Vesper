import { common, ConfigurationConstant } from '@kit.AbilityKit';
import { fileIo as fs, picker } from '@kit.CoreFileKit';
import { preferences } from '@kit.ArkData';
import { pasteboard } from '@kit.BasicServicesKit';
import { PiPWindow } from '@kit.ArkUI';
import { util } from '@kit.ArkTS';
import {
  VesperCancellationToken, VesperDownloadManager, VesperDownloadSnapshot, VesperExportArtifact,
  VesperPlaybackSequence, VesperPlayerController, VesperPlayerError, VesperPlayerSnapshot,
  VesperPlayerSource, VesperSourceHandle, VesperSourceSession, VesperSystemSession, VesperSequenceItem,
  VesperDiskCache, VesperPreloadTask, VesperPlaybackPolicy, VesperPlaybackRecovery,
  queryVideoDecoderCapability, VesperVideoDecoderCapability, VESPER_HARMONY_SOURCE_SHA256
} from '@vesper/player-kit';
import { VesperPictureInPicture, VesperPlayerBinding, playbackFeedback } from '@vesper/player-kit-arkui';
import {
  appendLog, ExampleLogEntry, EXAMPLE_HLS_URL, EXAMPLE_DASH_URL, EXAMPLE_IMPORT_BYTES,
  EXAMPLE_MP4_URL, EXAMPLE_QUEUE_CAPACITY, remoteSource
} from './ExampleSupport';
import { ExampleDiagnosticResult, runDiagnosticScenario } from './DiagnosticScenario';

export interface ExampleQueueItem {
  id: string;
  title: string;
  detail: string;
  source: VesperPlayerSource;
  handle: VesperSourceHandle;
  builtin: boolean;
  downloadTaskId?: string;
}

/** Stable row identity with an independently observed task snapshot. */
@Observed
export class ExampleDownloadItem {
  readonly taskId: string;
  snapshot: VesperDownloadSnapshot;
  constructor(snapshot: VesperDownloadSnapshot) { this.taskId = snapshot.taskId; this.snapshot = snapshot; }
}

const BUNDLED_FILES: string[] = ['movie.mp4', 'dash/manifest.mpd', 'dash/video.mp4', 'dash/audio.mp4'];
interface ExampleCacheInventory { entries: number; residentBytes: number; maximumBytes: number; hitBytes: number; }

/** App-owned orchestration; all playback and download work uses the public HAR APIs. */
@Observed
export class ExampleModel {
  readonly binding: VesperPlayerBinding = new VesperPlayerBinding(new VesperPlayerController(), {
    onDisappear: 'keep', releaseController: true
  });
  snapshot: VesperPlayerSnapshot = this.binding.controller.snapshot();
  tab: string = 'play';
  theme: string = 'system';
  layout: string = 'compact';
  fullscreenLayout: string = 'expanded';
  accented: boolean = false;
  address: string = EXAMPLE_HLS_URL;
  protocol: string = 'auto';
  downloadAddress: string = EXAMPLE_MP4_URL;
  downloadProtocol: string = 'auto';
  queue: ExampleQueueItem[] = [];
  selectedId: string = '';
  nowPlayingTitle: string = '';
  downloads: ExampleDownloadItem[] = [];
  logs: ExampleLogEntry[] = [];
  diagnosticScenario: string = 'progress';
  diagnosticRuntimeKind: string = 'unknown';
  diagnosticAudioObservation: string = 'not-assessed';
  diagnosticResult?: ExampleDiagnosticResult;
  private diagnosticResultEpoch?: string;
  message: string = '';
  messageFailed: boolean = false;
  busy: boolean = true;
  ready: boolean = false;
  pip?: VesperPictureInPicture;
  pipState: string = 'Off';
  systemState: string = 'Not active';
  systemEnabled: boolean = true;
  pipActive: boolean = false;
  pipAutoStart: boolean = false;
  policyProfile: string = 'system';
  recoveryEnabled: boolean = false;
  recoveryState: string = 'off';
  preferenceLanguage: string = 'system';
  subtitlePreference: string = 'system';
  preloadStatus: string = 'Not started';
  preloadItemId: string = '';
  preloadBytes: number = 0;
  preloadReuse: string = 'none';
  preloadReason: string = '';
  cacheEntries: number = 0;
  cacheBytes: number = 0;
  cacheHits: number = 0;
  cacheMaximum: number = 32 * 1024 * 1024;
  decoderCapability?: VesperVideoDecoderCapability;
  private diagnosticPending: boolean = false;
  private lastDiagnosticSample: number = 0;
  private cache?: VesperDiskCache;
  private preload?: VesperPreloadTask;
  private monitorTimer: number = -1;
  private context?: common.UIAbilityContext;
  private sources: VesperSourceSession = new VesperSourceSession();
  private sequence: VesperPlaybackSequence = new VesperPlaybackSequence({ sequenceId: 'harmony-example',
    maxItems: EXAMPLE_QUEUE_CAPACITY, historyLimit: EXAMPLE_QUEUE_CAPACITY - 1 });
  private manager?: VesperDownloadManager;
  private system?: VesperSystemSession;
  private systemMetadataKey: string = '';
  private recovery?: VesperPlaybackRecovery;
  private settings?: preferences.Preferences;
  private exports: Map<string, VesperExportArtifact> = new Map();
  private unsubscribe: () => void = (): void => {};
  private unsubscribeDownloads: () => void = (): void => {};
  private nextId: number = 0;
  private logId: number = 0;
  private closed: boolean = false;
  private initialization?: Promise<void>;
  private closing?: Promise<void>;
  private cancellation: VesperCancellationToken = new VesperCancellationToken();
  private lastState: string = '';
  private lastError: string = '';
  private lastRate: number = 1;
  private lastSelection: string = '';
  private importFiles: string[] = [];
  private importedBytes: number = 0;
  private exportCleanupTimer: number = -1;

  current(): ExampleQueueItem | undefined { return this.queue.find((item: ExampleQueueItem) => item.id === this.selectedId); }
  text(name: string): string { return this.context?.resourceManager.getStringByNameSync(name) ?? ''; }
  title(): string { return this.current()?.title ?? this.text('example_welcome'); }
  private check(): void { if (this.closed) { throw new VesperPlayerError('closed', 'The example has closed.'); } }
  private hostContext(): common.UIAbilityContext {
    this.check();
    if (this.context === undefined) { throw new VesperPlayerError('not_ready', 'The example is still getting ready.'); }
    return this.context;
  }
  private downloadManager(): VesperDownloadManager {
    this.check();
    if (this.manager === undefined) { throw new VesperPlayerError('not_ready', 'Downloads are still getting ready.'); }
    return this.manager;
  }
  start(context: common.UIAbilityContext): Promise<void> {
    if (this.closed) { return Promise.reject(new VesperPlayerError('closed', 'The example has closed.')); }
    if (this.initialization !== undefined) { return this.initialization; }
    this.context = context;
    this.initialization = this.initialize(context);
    return this.initialization;
  }
  private async initialize(context: common.UIAbilityContext): Promise<void> {
    try {
      this.unsubscribe = this.binding.controller.subscribe((snapshot: VesperPlayerSnapshot): void => {
        if (this.closed) { return; }
        if (snapshot.playbackEpoch !== this.snapshot.playbackEpoch) {
          this.diagnosticResult = undefined; this.diagnosticAudioObservation = 'not-assessed';
        }
        this.snapshot = snapshot;
        this.updateSystemMetadata(snapshot);
        if (snapshot.state !== this.lastState) { this.log('Playback', snapshot.state); this.lastState = snapshot.state; }
        const error = snapshot.error?.code ?? '';
        if (error.length > 0 && error !== this.lastError) { this.log('Playback error', error, true); }
        this.lastError = error;
        if (snapshot.playbackRate !== this.lastRate) { this.log('Playback rate', `${snapshot.playbackRate}×`); this.lastRate = snapshot.playbackRate; }
        const selection = JSON.stringify(snapshot.selection.requested ?? {}) + (snapshot.selection.outcome ?? '');
        if (selection !== this.lastSelection && snapshot.selection.requested !== undefined) {
          this.log('Track selection', `${snapshot.selection.requested.kind} · ${snapshot.selection.outcome ?? 'pending'}`);
        }
        this.lastSelection = selection;
      });
      const cache = await VesperDiskCache.open(context, { partition: 'example', maximumBytes: this.cacheMaximum });
      if (this.closed) { await cache.close(); return; }
      this.cache = cache; this.sources.close();
      this.sources = new VesperSourceSession({ maxRegisteredSources: EXAMPLE_QUEUE_CAPACITY, memoryBudgetBytes: 8 * 1024 * 1024,
        preload: { workers: 2, pendingCapacity: 4 }, diskCache: cache });
      const root = `${context.filesDir}/example-media`;
      if (!await fs.access(root)) { await fs.mkdir(root); }
      if (!await fs.access(`${root}/dash`)) { await fs.mkdir(`${root}/dash`); }
      // Imports belong to one app lifetime; recover abandoned copies after process death.
      const abandoned = await fs.listFile(root, { recursion: false, listNum: 32 });
      for (const name of abandoned) {
        this.check();
        if (/^import-[0-9]+-[0-9]+\.mp4$/.test(name)) { await fs.unlink(`${root}/${name}`); }
      }
      for (const name of BUNDLED_FILES) {
        this.check();
        const bytes = await context.resourceManager.getRawFileContent(`demos/${name}`);
        const output = await fs.open(`${root}/${name}`, fs.OpenMode.CREATE | fs.OpenMode.TRUNC | fs.OpenMode.WRITE_ONLY);
        try { await this.writeAll(output.fd, bytes.buffer as ArrayBuffer); } finally { await fs.close(output); }
      }
      this.check();
      this.sequence.attach(this.binding.controller);
      this.addSource({ kind: 'local', protocol: 'file', uri: `${root}/movie.mp4`, label: this.text('example_studio_sample') }, this.text('example_studio_detail'), undefined, true);
      this.addSource({ kind: 'local', protocol: 'dash', uri: `${root}/dash/manifest.mpd`, label: this.text('example_dash_sample') }, this.text('example_dash_detail'), undefined, true);
      const stream = remoteSource(EXAMPLE_HLS_URL); stream.label = this.text('example_hls_title');
      this.addSource(stream, this.text('example_hls_detail'), undefined, true);
      const dash = remoteSource(EXAMPLE_DASH_URL); dash.label = this.text('example_public_dash_title');
      this.addSource(dash, this.text('example_public_dash_detail'), undefined, true);
      const manager = await VesperDownloadManager.open(context);
      if (this.closed) { await manager.close(); return; }
      this.manager = manager;
      this.unsubscribeDownloads = manager.subscribe((): void => { if (!this.closed) { this.refreshDownloads(); } });
      this.refreshDownloads();
      try {
        this.settings = await preferences.getPreferences(context, { name: 'vesper-example' });
        this.check();
        const stored = await this.settings.get('theme', 'system');
        this.check();
        if (typeof stored === 'string' && ['system', 'light', 'dark'].includes(stored)) { this.applyTheme(stored); }
      } catch (_) { if (!this.closed) { this.log('Preferences', 'Using the default appearance'); } }
      this.check();
      for (let attempt = 0; attempt < 50 && this.binding.surfaceId().length === 0; attempt++) {
        await new Promise<void>((resolve) => { setTimeout(resolve, 20); }); this.check();
      }
      const initial = this.current();
      if (initial !== undefined && this.binding.surfaceId().length > 0) { await this.activate(initial, false); }
      this.check(); this.ready = true; this.message = '';
      this.monitorTimer = setInterval((): void => { this.refreshOperations(); }, 500);
    } catch (error) {
      if (!this.closed) { this.failure('Setup', error as Error); }
    } finally { if (!this.closed) { this.busy = false; } }
  }
  private async writeAll(fd: number, bytes: ArrayBuffer): Promise<void> {
    let offset = 0;
    while (offset < bytes.byteLength) {
      this.check(); const written = await fs.write(fd, bytes.slice(offset));
      if (written <= 0 || written > bytes.byteLength - offset) { throw new VesperPlayerError('file_write', 'The video could not be copied completely.'); }
      offset += written;
    }
  }
  private addSource(source: VesperPlayerSource, detail: string, taskId?: string, builtin: boolean = false): ExampleQueueItem {
    this.check();
    if (this.queue.length >= EXAMPLE_QUEUE_CAPACITY) { throw new VesperPlayerError('capacity', 'The queue holds twelve videos. Remove a video before adding another.'); }
    const handle = this.sources.register(source);
    const item: ExampleQueueItem = { id: `video-${++this.nextId}`, title: source.label, detail: detail, builtin: builtin,
      source: source, handle: handle, downloadTaskId: taskId };
    const next = this.queue.concat([item]);
    try { this.replaceQueue(next, this.selectedId || item.id); }
    catch (error) { handle.close(); throw error as Error; }
    return item;
  }
  private replaceQueue(items: ExampleQueueItem[], active: string): void {
    this.sequence.replace(items.map((item: ExampleQueueItem): VesperSequenceItem => ({ itemId: item.id, providerNamespace: 'example',
      contentIdentity: item.id, mediaKind: 'vod', source: item.handle })), active || undefined);
    this.queue = items; this.selectedId = active; this.nowPlayingTitle = this.title();
  }
  private async activate(item: ExampleQueueItem, playWhenReady: boolean = true): Promise<void> {
    this.check();
    if (this.binding.surfaceId().length === 0) { throw new VesperPlayerError('not_ready', 'The video window is not ready yet. Try again in a moment.'); }
    this.sequence.jump(item.id); this.selectedId = item.id; this.nowPlayingTitle = item.title; this.tab = 'play';
    await this.sequence.activate({ playWhenReady: playWhenReady }, this.cancellation);
    this.check();
    await this.ensureSystem();
    this.message = '';
  }
  private async ensureSystem(): Promise<void> {
    if (this.system === undefined && this.systemEnabled) {
      const system = await VesperSystemSession.create(this.hostContext(), this.binding.controller,
        { bundleName: 'dev.vesper.harmony.example', abilityName: 'EntryAbility', title: 'Vesper Player', backgroundAudio: true });
      if (this.closed) { await system.close(); return; }
      this.system = system;
      this.systemMetadataKey = '';
    }
    this.updateSystemMetadata(this.binding.controller.snapshot());
    this.systemState = this.system?.snapshot().state ?? 'Off';
  }
  private updateSystemMetadata(snapshot: VesperPlayerSnapshot): void {
    if (this.system === undefined || snapshot.sessionId === undefined || snapshot.sourceId === undefined || snapshot.playbackEpoch === undefined) { return; }
    const item = this.queue.find((item: ExampleQueueItem): boolean => item.handle.sourceId === snapshot.sourceId && item.handle.sessionId === snapshot.sessionId);
    if (item === undefined) { return; }
    const key = snapshot.sessionId + ':' + snapshot.sourceId + ':' + snapshot.playbackEpoch + ':' + item.title;
    if (key === this.systemMetadataKey) { return; }
    this.system.updateMetadata({ sessionId: snapshot.sessionId, sourceId: snapshot.sourceId, playbackEpoch: snapshot.playbackEpoch }, { title: item.title });
    this.systemMetadataKey = key;
  }
  play(item: ExampleQueueItem): void { this.run('Open video', async (): Promise<void> => { await this.activate(item); }); }
  retry(): void { const item = this.current(); if (item !== undefined) { this.play(item); } }
  next(direction: number): void {
    const index = this.queue.findIndex((item: ExampleQueueItem) => item.id === this.selectedId) + direction;
    if (index >= 0 && index < this.queue.length) { this.play(this.queue[index]); }
  }
  openRemote(playNow: boolean): void {
    this.run(playNow ? 'Open stream' : 'Add to queue', async (): Promise<void> => {
      const source = remoteSource(this.address, this.protocol);
      source.label = this.text('example_remote_video');
      const item = this.addSource(source, source.protocol.toUpperCase());
      if (playNow) { await this.activate(item); } else { this.notify('example_added'); }
    });
  }
  removeQueueItem(item: ExampleQueueItem): void {
    if (item.builtin || item.id === this.selectedId) { return; }
    this.run('Remove video', async (): Promise<void> => {
      this.replaceQueue(this.queue.filter((entry: ExampleQueueItem) => entry.id !== item.id), this.selectedId);
      item.handle.close(); this.notify('example_removed_queue');
    });
  }
  playSample(protocol: string): void {
    const item = this.queue.find((entry: ExampleQueueItem): boolean => entry.builtin && entry.source.kind === 'remote' && entry.source.protocol === protocol);
    if (item !== undefined) { this.play(item); }
  }
  queuePreview(): ExampleQueueItem[] {
    const active = this.queue.findIndex((item: ExampleQueueItem): boolean => item.id === this.selectedId);
    const start = Math.max(0, Math.min(active - 1, this.queue.length - 5));
    return this.queue.slice(start, start + 5);
  }
  queueState(item: ExampleQueueItem): string {
    if (item.id === this.selectedId) { return this.snapshot.state; }
    if (item.id === this.preloadItemId) { return `Preload: ${this.preloadStatus}`; }
    return 'Ready in queue';
  }
  preloadNext(): void {
    const index = this.queue.findIndex((item: ExampleQueueItem): boolean => item.id === this.selectedId);
    const item = this.queue[index + 1];
    if (item !== undefined) { this.preloadItem(item); } else { this.notify('example_preload_last'); }
  }
  preloadItem(item: ExampleQueueItem): void {
    this.run('Preload video', async (): Promise<void> => {
      if (this.preload !== undefined && ['queued', 'running'].includes(this.preload.snapshot().status)) {
        throw new VesperPlayerError('in_use', 'Cancel the current preload before starting another.');
      }
      const task = item.handle.preload({ maxBytes: 8 * 1024 * 1024, timeoutMs: 10000, lifetimeMs: 30000 });
      this.preload = task; this.preloadItemId = item.id; this.refreshOperations();
      task.completion.then((): void => { if (!this.closed && this.preload === task) { this.refreshOperations(); this.log('Preload', this.preloadStatus); } },
        (error: Error): void => { if (!this.closed && this.preload === task) { this.refreshOperations(); this.log('Preload', playbackFeedback(error).code, true); } });
    });
  }
  cancelPreload(): void { this.preload?.cancel(); this.refreshOperations(); }
  clearCache(): void {
    this.run('Clear cache', async (): Promise<void> => {
      await this.cache?.clear(); this.check(); this.refreshOperations();
      this.notify('example_cache_cleared');
    });
  }
  applyPolicy(profile: string): void {
    this.run('Playback policy', async (): Promise<void> => {
      const policy: VesperPlaybackPolicy = { decoder: 'system', retry: { maxAttempts: profile === 'resilient' ? 3 : profile === 'responsive' ? 1 : 2 } };
      policy.buffering = profile === 'system' ? {} : { preferredBufferDurationMs: profile === 'resilient' ? 20000 : 10000,
        preferredBufferDurationForPlayingMs: profile === 'resilient' ? 2000 : 500 };
      await this.binding.controller.setPlaybackPolicy(policy); this.check(); this.policyProfile = profile;
      this.notify('example_policy_evaluated');
    });
  }
  setRecoveryEnabled(value: boolean): void {
    this.run('Playback recovery', async (): Promise<void> => {
      this.recovery?.close(); this.recovery = undefined;
      if (value) {
        this.recovery = new VesperPlaybackRecovery(this.binding.controller,
          (options, token) => this.sequence.activate(options, token), { maxAttempts: 2, deadlineMs: 30000, backoffMs: 500 });
      }
      this.recoveryEnabled = value; this.recoveryState = this.recovery?.snapshot().state ?? 'off';
    });
  }
  setTrackPreferences(language: string, subtitleMode: string): void {
    this.run('Track preferences', async (): Promise<void> => {
      await this.binding.controller.setTrackPreferences({ audioLanguages: language === 'system' ? [] : [language],
        subtitleLanguages: language === 'system' ? [] : [language], subtitleMode: subtitleMode });
      this.check(); this.preferenceLanguage = language; this.subtitlePreference = subtitleMode;
    });
  }
  setSystemEnabled(enabled: boolean): void {
    this.run('System playback', async (): Promise<void> => {
      if (!enabled) { await this.system?.close(); this.system = undefined; }
      this.check(); this.systemEnabled = enabled; await this.ensureSystem(); this.refreshOperations();
    });
  }
  private refreshOperations(): void {
    this.recoveryState = this.recovery?.snapshot().state ?? 'off';
    if (this.closed) { return; }
    if (this.tab === 'diagnostics' && !this.diagnosticPending && Date.now() - this.lastDiagnosticSample >= 2000
      && ['prepared', 'playing', 'paused'].includes(this.snapshot.state)) {
      this.diagnosticPending = true; this.lastDiagnosticSample = Date.now();
      try {
        const video = this.snapshot.tracks.find((track): boolean => track.kind === 'video' && track.selected);
        const mime = video?.mimeType;
        this.decoderCapability = mime !== undefined && ['video/avc', 'video/hevc', 'video/x-vnd.on2.vp8', 'video/x-vnd.on2.vp9', 'video/av01'].includes(mime)
          ? queryVideoDecoderCapability({ mimeType: mime,
            width: (video?.width ?? 0) > 0 && (video?.height ?? 0) > 0 ? video?.width : undefined,
            height: (video?.width ?? 0) > 0 && (video?.height ?? 0) > 0 ? video?.height : undefined }) : undefined;
      }
      catch (error) { this.log('Decoder preflight', `${error}`, true); }
      this.binding.controller.refreshDiagnostics().catch((error: Error): void => {
        if (!this.closed && !['stopped', 'stale_epoch', 'cancelled'].some((code: string): boolean => `${error}`.includes(code))) {
          this.log('Playback diagnostics', `${error}`, true);
        }
      }).finally((): void => { this.diagnosticPending = false; });
    }
    const preload = this.preload?.snapshot();
    if (preload !== undefined) { this.preloadStatus = preload.status; this.preloadBytes = preload.receivedBytes;
      this.preloadReuse = preload.reuseLevel; this.preloadReason = preload.reason ?? ''; }
    if (this.cache !== undefined) {
      const cache = JSON.parse(this.cache.inventory()) as ExampleCacheInventory;
      this.cacheEntries = cache.entries; this.cacheBytes = cache.residentBytes; this.cacheHits = cache.hitBytes;
    }
    const system = this.system?.snapshot();
    this.systemState = system === undefined ? 'Off' : `${system.state} · background ${system.background}${system.error ? ` · ${system.error.code}` : ''}`;
    const pip = this.pip?.snapshot(); this.pipState = pip?.state ?? 'Off';
    this.pipActive = pip !== undefined && [PiPWindow.PiPState.ABOUT_TO_START, PiPWindow.PiPState.STARTED, PiPWindow.PiPState.ABOUT_TO_STOP].some((state: number): boolean => pip.state === `native:${state}`);
  }
  pickVideo(): void {
    this.run('Choose video', async (): Promise<void> => {
      const context = this.hostContext();
      if (this.queue.length >= EXAMPLE_QUEUE_CAPACITY) { throw new VesperPlayerError('capacity', 'Remove a queued video before importing another.'); }
      const selected = await new picker.DocumentViewPicker(context).select({ maxSelectNumber: 1, fileSuffixFilters: ['Video|.mp4,.m4v,.mov'] });
      this.check();
      if (selected.length === 0) { this.notify('example_pick_cancelled'); return; }
      const input = await fs.open(selected[0], fs.OpenMode.READ_ONLY);
      const path = `${context.filesDir}/example-media/import-${Date.now()}-${this.nextId}.mp4`;
      let accepted = false;
      try {
        const size = (await fs.stat(input.fd)).size;
        if (!Number.isSafeInteger(size) || size < 1 || size > EXAMPLE_IMPORT_BYTES - this.importedBytes) {
          throw new VesperPlayerError('capacity', 'Imported videos share a 1 GiB budget in this example.');
        }
        this.check();
        await fs.copyFile(input.fd, path);
        this.check();
        if ((await fs.stat(path)).size !== size) { throw new VesperPlayerError('file_changed', 'The selected video changed while being copied.'); }
        const item = this.addSource({ kind: 'local', protocol: 'file', uri: path, label: this.text('example_imported') }, this.text('example_imported_detail'));
        this.importFiles.push(path); this.importedBytes += size; accepted = true;
        await this.activate(item);
      } finally {
        await fs.close(input);
        if (!accepted && await fs.access(path)) { await fs.unlink(path); }
      }
    });
  }
  createDownload(): void {
    this.run('Create download', async (): Promise<void> => {
      const source = remoteSource(this.downloadAddress, this.downloadProtocol);
      source.label = this.text('example_downloaded_video');
      const handle = this.sources.register(source);
      try {
        const task = await this.downloadManager().create(handle, `${source.protocol.toUpperCase()} video ${Date.now()}`);
        this.check(); this.downloadManager().start(task.taskId); this.refreshDownloads(); this.notify('example_download_started');
      } finally { handle.close(); }
    });
  }
  pauseDownload(task: VesperDownloadSnapshot): void {
    this.run('Pause download', async (): Promise<void> => {
      await this.downloadManager().pause(task.taskId); await this.downloadManager().wait(task.taskId);
      this.refreshDownloads(); this.notify('example_download_paused');
    });
  }
  resumeDownload(task: VesperDownloadSnapshot): void {
    this.run('Resume download', async (): Promise<void> => {
      const manager = this.downloadManager();
      if (task.resumeState === 'source-refresh-required' || task.reason === 'source_refresh_required' || task.status === 'Failed') {
        const handle = this.sources.register(remoteSource(this.downloadAddress, this.downloadProtocol));
        try { await manager.refreshSource(task.taskId, handle); } finally { handle.close(); }
      }
      this.check(); manager.resume(task.taskId); this.refreshDownloads(); this.notify('example_download_resumed');
    });
  }
  playDownload(task: VesperDownloadSnapshot): void {
    this.run('Play offline', async (): Promise<void> => {
      const existing = this.queue.find((item: ExampleQueueItem) => item.downloadTaskId === task.taskId);
      if (existing !== undefined) { await this.activate(existing); return; }
      const source = await this.downloadManager().offlineSource(task.taskId);
      const item = this.addSource(source, this.text('example_imported_detail'), task.taskId);
      await this.activate(item);
    });
  }
  canRemoveDownload(taskId: string): boolean { return this.current()?.downloadTaskId !== taskId; }
  removeDownload(task: VesperDownloadSnapshot): void {
    this.run('Delete download', async (): Promise<void> => {
      if (!this.canRemoveDownload(task.taskId)) { throw new VesperPlayerError('in_use', 'Play another video before deleting this download.'); }
      const queued = this.queue.filter((item: ExampleQueueItem) => item.downloadTaskId === task.taskId);
      this.replaceQueue(this.queue.filter((item: ExampleQueueItem) => item.downloadTaskId !== task.taskId), this.selectedId);
      for (const item of queued) { item.handle.close(); }
      await this.downloadManager().remove(task.taskId); await this.releaseExports(task.taskId);
      this.refreshDownloads(); this.notify('example_download_deleted');
    });
  }
  exportDownload(task: VesperDownloadSnapshot, sharing: boolean): void {
    this.run(sharing ? 'Share download' : 'Save download', async (): Promise<void> => {
      if (task.exportEligibility?.available === false) { throw new VesperPlayerError('capacity', 'This task is not eligible for export'); }
      await this.releaseExports(); this.check();
      let artifact = this.exports.get(task.taskId);
      if (artifact === undefined) {
        artifact = await this.downloadManager().exportAsset(task.taskId);
        if (this.closed) { await artifact.release(); return; }
        this.exports.set(task.taskId, artifact);
      }
      this.check();
      if (sharing) { await artifact.share(this.hostContext()); this.notify('example_share_opened'); }
      else {
        try { const saved = await artifact.save(this.hostContext()); this.notify(saved ? 'example_saved' : 'example_save_cancelled'); }
        finally {
          try { await artifact.release(); this.exports.delete(task.taskId); }
          catch (_) { /* A prior share can still own its documented ten-minute lease. */ }
        }
      }
    });
  }
  private async releaseExports(taskId?: string): Promise<void> {
    for (const entry of this.exports) {
      if (taskId !== undefined && taskId !== entry[0]) { continue; }
      try { await entry[1].release(); this.exports.delete(entry[0]); }
      catch (_) { /* Keep the bounded owner reachable while a system reader retains it. */ }
    }
  }
  private retainExportCleanup(attempt: number = 0): void {
    if (this.exports.size === 0 || this.exportCleanupTimer !== -1 || attempt >= 3) { return; }
    // A closed page retains only its export owners until the ten-minute share lease expires.
    this.exportCleanupTimer = setTimeout((): void => {
      this.exportCleanupTimer = -1;
      this.releaseExports().then((): void => { this.retainExportCleanup(attempt + 1); });
    }, attempt === 0 ? 600001 : 60000);
  }
  private refreshDownloads(): void {
    const next: ExampleDownloadItem[] = [];
    for (const snapshot of this.manager?.snapshots() ?? []) {
      if (snapshot.status === 'Removed') { continue; }
      const item = this.downloads.find((value: ExampleDownloadItem) => value.taskId === snapshot.taskId) ?? new ExampleDownloadItem(snapshot);
      item.snapshot = snapshot; next.push(item);
    }
    this.downloads = next;
  }
  private async ensurePiP(): Promise<VesperPictureInPicture> {
    this.check();
    if (this.pip === undefined) {
      const pip = await VesperPictureInPicture.create(this.hostContext(), this.binding);
      if (this.closed) { await pip.close(); this.check(); }
      this.pip = pip;
    }
    return this.pip;
  }
  pictureInPicture(): void {
    this.run('Picture in Picture', async (): Promise<void> => {
      const pip = await this.ensurePiP();
      await pip.setAutoStart(this.pipAutoStart); await pip.start(); this.check(); this.refreshOperations();
      this.notify('example_pip_enabled');
    });
  }
  setPiPAutoStart(value: boolean): void {
    this.run('Automatic Picture in Picture', async (): Promise<void> => {
      const pip = value ? await this.ensurePiP() : this.pip;
      if (pip !== undefined) { await pip.setAutoStart(value); }
      this.check(); this.pipAutoStart = value; this.refreshOperations();
    });
  }
  setTheme(value: string): void {
    this.run('Appearance', async (): Promise<void> => {
      this.applyTheme(value);
      if (this.settings !== undefined) { await this.settings.put('theme', value); await this.settings.flush(); }
    });
  }
  private applyTheme(value: string): void {
    if (!['system', 'light', 'dark'].includes(value)) { throw new VesperPlayerError('invalid_argument', 'Unknown appearance.'); }
    const mode = value === 'dark' ? ConfigurationConstant.ColorMode.COLOR_MODE_DARK : value === 'light' ? ConfigurationConstant.ColorMode.COLOR_MODE_LIGHT : ConfigurationConstant.ColorMode.COLOR_MODE_NOT_SET;
    this.hostContext().getApplicationContext().setColorMode(mode); this.theme = value;
  }
  copyDiagnostics(): void {
    this.run('Copy diagnostics', async (): Promise<void> => {
      const report = this.diagnosticReportText();
      await pasteboard.getSystemPasteboard().setData(pasteboard.createData(pasteboard.MIMETYPE_TEXT_PLAIN, report));
      this.notify('example_diagnostics_copied');
    });
  }
  private diagnosticReportText(): string {
    const item = this.current();
    const player = this.binding.controller.diagnosticReport({ scenario: this.diagnosticScenario,
      fixtureId: item?.builtin ? item.source.uri === EXAMPLE_HLS_URL ? 'apple-bipbop-adv-ts' :
        item.source.uri === EXAMPLE_DASH_URL ? 'envivio-dash3' : item.source.protocol === 'dash' ? 'bundled-dash' : 'bundled-h264-aac' : 'user-selected', applicationVersion: '0.9.1',
      applicationBuildIdentity: VESPER_HARMONY_SOURCE_SHA256, runtimeKind: this.diagnosticRuntimeKind,
      audibleOutputObservation: this.diagnosticAudioObservation });
    const result = this.diagnosticResultEpoch === this.binding.controller.snapshot().playbackEpoch
      && this.diagnosticResult?.scenario === this.diagnosticScenario ? this.diagnosticResult : undefined;
    return JSON.stringify({ player: player, scenarioResult: result });
  }
  runDiagnostics(): void {
    this.run('Run diagnostics', async (): Promise<void> => {
      this.diagnosticResult = undefined;
      this.diagnosticResultEpoch = this.binding.controller.snapshot().playbackEpoch;
      this.diagnosticResult = await runDiagnosticScenario(this.binding.controller, this.diagnosticScenario, (): void => { this.check(); });
      this.notify('example_diag_scenario_finished');
    });
  }
  saveDiagnostics(): void {
    this.run('Save diagnostics', async (): Promise<void> => {
      const report = this.diagnosticReportText();
      const selected = await new picker.DocumentViewPicker(this.hostContext()).save({ newFileNames: ['vesper-diagnostics.json'] });
      this.check();
      if (selected.length === 0) { this.notify('example_save_cancelled'); return; }
      if (selected.length !== 1) { throw new VesperPlayerError('invalid_destination', 'Report saving requires one document picker output'); }
      const output = await fs.open(selected[0], fs.OpenMode.CREATE | fs.OpenMode.READ_WRITE);
      try {
        if ((await fs.stat(output.fd)).size !== 0) { throw new VesperPlayerError('destination_not_empty', 'Report saving requires an empty document picker output'); }
        await this.writeAll(output.fd, new util.TextEncoder().encodeInto(report).buffer as ArrayBuffer);
      }
      finally { await fs.close(output); }
      this.notify('example_saved');
    });
  }
  clearLogs(): void { this.logs = []; this.notify('example_history_cleared'); }
  private notify(key: string): void { if (!this.closed) { this.message = this.text(key); this.messageFailed = false; } }
  private failure(action: string, error: Error): void {
    const feedback = playbackFeedback(error);
    if (feedback.category === 'cancelled') { this.message = ''; this.messageFailed = false; return; }
    const suffix = feedback.category === 'inUse' ? 'in_use' : feedback.category === 'notReady' ? 'not_ready' : feedback.category;
    this.message = this.text('example_error_' + suffix); this.messageFailed = true;
    this.log(action, feedback.code, true);
  }
  private log(title: string, detail: string, failed: boolean = false): void {
    if (this.closed) { return; }
    const now = new Date();
    const time = [now.getHours(), now.getMinutes(), now.getSeconds()].map((value: number) => value.toString().padStart(2, '0')).join(':');
    this.logs = appendLog(this.logs, { id: ++this.logId, time: time, title: title, detail: detail, failed: failed });
  }
  private run(action: string, work: () => Promise<void>): void {
    if (this.closed || this.busy || !this.ready) { return; }
    this.busy = true; this.message = ''; this.messageFailed = false;
    work().catch((error: Error): void => { if (!this.closed) { this.failure(action, error); } })
      .finally((): void => { if (!this.closed) { this.busy = false; } });
  }
  close(): Promise<void> {
    if (this.closing !== undefined) { return this.closing; }
    this.closed = true; this.cancellation.cancel(); this.unsubscribe(); this.unsubscribeDownloads();
    if (this.monitorTimer !== -1) { clearInterval(this.monitorTimer); this.monitorTimer = -1; }
    this.preload?.cancel(); this.recovery?.close();
    this.sequence.close(); this.sources.close();
    const work = this.binding.close(true).then(async (): Promise<void> => {
      if (this.manager !== undefined) { await this.manager.close(); }
      await this.cache?.close();
      await this.releaseExports(); this.retainExportCleanup();
      for (const path of this.importFiles) { if (await fs.access(path)) { await fs.unlink(path); } }
      this.importFiles = [];
    });
    this.closing = work; work.catch(() => { this.closing = undefined; }); return work;
  }
}
