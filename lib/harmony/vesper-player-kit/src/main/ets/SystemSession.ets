import { avSession } from '@kit.AVSessionKit';
import { backgroundTaskManager } from '@kit.BackgroundTasksKit';
import { common, wantAgent, WantAgent } from '@kit.AbilityKit';
import { VesperPlayerController } from './Controller';
import { VesperPlayerSnapshot, VesperPlayerError } from './Types';
import { OperationScope, nativeError } from './Operation';

export interface VesperSystemConfiguration {
  bundleName: string; abilityName: string; title: string; backgroundAudio?: boolean;
  controls?: VesperSystemControls;
}
export interface VesperSystemControls { play?: boolean; pause?: boolean; stop?: boolean; seek?: boolean; setSpeed?: boolean; }
export interface VesperSystemMediaIdentity { sessionId: string; sourceId: string; playbackEpoch: string; }
export interface VesperSystemMediaMetadata { title: string; artist?: string; artworkUri?: string; }
export interface VesperSystemSnapshot {
  state: string; background: string; commands: number; publications: number; error?: VesperPlayerError;
  metadataState?: string; metadataSourceId?: string; supportedControls?: string[];
}
function controls(value: VesperSystemControls = {}): VesperSystemControls {
  if (Object.keys(value).some((key: string): boolean => !['play', 'pause', 'stop', 'seek', 'setSpeed'].includes(key))) {
    throw new VesperPlayerError('unsupported', 'Unknown system control');
  }
  if ([value.play, value.pause, value.stop, value.seek, value.setSpeed].some((value: boolean | undefined): boolean => value !== undefined && typeof value !== 'boolean')) {
    throw new VesperPlayerError('invalid_argument', 'System controls require booleans');
  }
  return { play: value.play ?? true, pause: value.pause ?? true, stop: value.stop ?? true, seek: value.seek ?? true, setSpeed: value.setSpeed ?? true };
}
function sourceKey(snapshot: VesperPlayerSnapshot): string { return `${snapshot.sessionId ?? 'idle'}:${snapshot.sourceId ?? 'idle'}`; }
function playbackKey(snapshot: VesperPlayerSnapshot): string { return `${sourceKey(snapshot)}:${snapshot.playbackEpoch ?? 'idle'}`; }

/** One explicitly selected controller publishes one AVSession per UIAbility. */
export class VesperSystemSession {
  private static owners: Map<string, VesperSystemSession> = new Map();
  private readonly key: string;
  private readonly context: common.UIAbilityContext;
  private readonly configuration: VesperSystemConfiguration;
  private controller: VesperPlayerController;
  private session?: avSession.AVSession;
  private agent?: WantAgent;
  private unsubscribe: () => void = (): void => {};
  private creation: Promise<void> = Promise.resolve();
  private updateWork?: Promise<void>;
  private pending?: VesperPlayerSnapshot;
  private closing: boolean = false;
  private closeWork?: Promise<void>;
  private background: boolean = false;
  private backgroundFailed: boolean = false;
  private controls: VesperSystemControls;
  private registered: VesperSystemControls = {};
  private metadata?: VesperSystemMediaMetadata;
  private metadataRevision: number = 0;
  private publishedMetadataRevision: number = -1;
  private source: string = '';
  private commandWork: Set<Promise<void>> = new Set();
  private commandControllers: Set<avSession.AVSessionController> = new Set();
  private removeCleanup: () => void = (): void => {};
  private asset: string = '';
  private metadataDuration?: number;
  private selection: number = 0;
  private value: VesperSystemSnapshot = { state: 'creating', background: 'inactive', commands: 0, publications: 0 };
  private constructor(context: common.UIAbilityContext, controller: VesperPlayerController, configuration: VesperSystemConfiguration) {
    this.context = context; this.controller = controller;
    this.configuration = JSON.parse(JSON.stringify(configuration)) as VesperSystemConfiguration;
    this.controls = controls(configuration.controls);
    this.key = `${configuration.bundleName}/${configuration.abilityName}`;
  }
  static async create(context: common.UIAbilityContext, controller: VesperPlayerController,
    configuration: VesperSystemConfiguration): Promise<VesperSystemSession> {
    if (!/^[A-Za-z0-9_.]+$/.test(configuration.bundleName) || !/^[A-Za-z0-9_.]+$/.test(configuration.abilityName)
      || configuration.title.length < 1 || configuration.title.length > 512) { throw new VesperPlayerError('invalid_argument', 'System session configuration is invalid'); }
    const value = new VesperSystemSession(context, controller, configuration);
    const previous = VesperSystemSession.owners.get(value.key);
    // A failed constructor retains a reachable cleanup owner. A later create
    // can retry that cleanup before attempting a new platform registration.
    if (previous?.closing) { await previous.close(); }
    if (VesperSystemSession.owners.has(value.key) || VesperSystemSession.owners.size >= 8) { throw new VesperPlayerError('in_use', 'This ability already owns a system session'); }
    VesperSystemSession.owners.set(value.key, value);
    const scope = new OperationScope(10000);
    try { value.removeCleanup = controller.registerCleanup(() => value.close()); }
    catch (error) { VesperSystemSession.owners.delete(value.key); scope.close(); throw nativeError(error, 'system cleanup registration'); }
    value.creation = value.initialize();
    try { await scope.wait(value.creation); value.selectController(controller); return value; }
    catch (error) { value.close().catch(() => {}); throw nativeError(error, 'system session creation'); }
    finally { scope.close(); }
  }
  private async initialize(): Promise<void> {
    const session = await avSession.createAVSession(this.context, 'Vesper', 'video'); this.session = session;
    if (this.closing) { return; }
    const info: wantAgent.WantAgentInfo = {
      wants: [{ bundleName: this.configuration.bundleName, abilityName: this.configuration.abilityName }],
      actionType: wantAgent.OperationType.START_ABILITY, requestCode: 1,
      actionFlags: [wantAgent.WantAgentFlags.UPDATE_PRESENT_FLAG]
    };
    this.agent = await wantAgent.getWantAgent(info);
    if (this.closing) { return; }
    await session.setLaunchAbility(this.agent); await session.activate(); this.value.state = 'active';
  }
  selectController(controller: VesperPlayerController): void {
    if (this.closing) { throw new VesperPlayerError('closed', 'System session is closing'); }
    if (controller !== this.controller) {
      const remove = controller.registerCleanup(() => this.close());
      this.removeCleanup(); this.removeCleanup = remove;
    }
    this.unsubscribe(); this.controller = controller; this.asset = ''; this.source = '';
    this.metadata = undefined; this.metadataRevision++;
    const selection = ++this.selection;
    this.unsubscribe = controller.subscribe((snapshot: VesperPlayerSnapshot): void => {
      if (snapshot.disposed || selection !== this.selection || this.controller !== controller) { return; }
      if (this.source !== playbackKey(snapshot)) {
        this.source = playbackKey(snapshot); this.metadata = undefined; this.metadataRevision++;
      }
      this.pending = snapshot; this.flush();
    });
  }
  /** Capture identity before asynchronous host work; stale source/epoch updates are rejected. Undefined clears overrides. */
  updateMetadata(identity: VesperSystemMediaIdentity, metadata?: VesperSystemMediaMetadata): void {
    const snapshot = this.controller.snapshot();
    if (this.closing) { throw new VesperPlayerError('closed', 'System session is closing'); }
    if (snapshot.disposed || identity.sessionId !== snapshot.sessionId || identity.sourceId !== snapshot.sourceId || identity.playbackEpoch !== snapshot.playbackEpoch) {
      throw new VesperPlayerError('stale_epoch', 'System metadata no longer belongs to the selected playback');
    }
    if (metadata !== undefined) {
      if (typeof metadata.title !== 'string' || metadata.title.length < 1 || metadata.title.length > 512 ||
        metadata.artist !== undefined && (typeof metadata.artist !== 'string' || metadata.artist.length > 512) ||
        metadata.artworkUri !== undefined && (typeof metadata.artworkUri !== 'string' || metadata.artworkUri.length < 1 || metadata.artworkUri.length > 8192 || /[\u0000-\u001F\u007F]/.test(metadata.artworkUri))) {
        throw new VesperPlayerError('invalid_argument', 'System metadata is invalid');
      }
      this.metadata = { title: metadata.title, artist: metadata.artist, artworkUri: metadata.artworkUri };
    } else { this.metadata = undefined; }
    // A host observer may publish before our observer in the same notification.
    // Adopt the validated identity now so that observer preserves its new value.
    this.source = playbackKey(snapshot);
    this.metadataRevision++; this.value.metadataState = 'pending'; this.pending = snapshot; this.flush();
  }
  setControls(value: VesperSystemControls): void {
    if (this.closing) { throw new VesperPlayerError('closed', 'System session is closing'); }
    this.controls = controls(value); this.pending = this.controller.snapshot(); this.flush();
  }
  private updateControls(session: avSession.AVSession, snapshot: VesperPlayerSnapshot): void {
    const desired: VesperSystemControls = { play: this.controls.play, pause: this.controls.pause, stop: this.controls.stop,
      seek: this.controls.seek && snapshot.timeline === 'vod', setSpeed: this.controls.setSpeed && snapshot.timeline === 'vod' };
    if (desired.play !== this.registered.play) { if (desired.play) { session.on('play', () => { this.dispatch('play'); }); } else { session.off('play'); } }
    if (desired.pause !== this.registered.pause) { if (desired.pause) { session.on('pause', () => { this.dispatch('pause'); }); } else { session.off('pause'); } }
    if (desired.stop !== this.registered.stop) { if (desired.stop) { session.on('stop', () => { this.dispatch('stop'); }); } else { session.off('stop'); } }
    if (desired.seek !== this.registered.seek) { if (desired.seek) { session.on('seek', (position: number) => { this.dispatch('seek', position); }); } else { session.off('seek'); } }
    if (desired.setSpeed !== this.registered.setSpeed) { if (desired.setSpeed) { session.on('setSpeed', (rate: number) => { this.dispatch('rate', rate); }); } else { session.off('setSpeed'); } }
    this.registered = desired;
    const supported: string[] = [];
    if (desired.play) { supported.push('play'); } if (desired.pause) { supported.push('pause'); } if (desired.stop) { supported.push('stop'); }
    if (desired.seek) { supported.push('seek'); } if (desired.setSpeed) { supported.push('setSpeed'); }
    this.value.supportedControls = supported;
  }
  private dispatch(operation: string, number?: number): void {
    if (this.closing) { return; }
    const enabled = operation === 'play' ? this.controls.play : operation === 'pause' ? this.controls.pause :
      operation === 'stop' ? this.controls.stop : operation === 'seek' ? this.controls.seek : operation === 'rate' ? this.controls.setSpeed : false;
    if (!enabled) { this.value.error = new VesperPlayerError('unsupported', 'System control is disabled'); return; }
    this.value.commands++;
    let work: Promise<void>;
    if (operation === 'play') { work = this.controller.play(); }
    else if (operation === 'pause') { work = this.controller.pause(); }
    else if (operation === 'stop') { work = this.controller.stop(); }
    else if (operation === 'seek' && number !== undefined && this.controller.snapshot().timeline === 'vod') { work = this.controller.seek(number); }
    else if (operation === 'rate' && number !== undefined && this.controller.snapshot().timeline === 'vod') { work = this.controller.setPlaybackRate(number); }
    else { this.value.error = new VesperPlayerError('unsupported', 'System command is unavailable for this timeline'); return; }
    work.catch((error: Error) => { this.value.error = nativeError(error, 'system command'); });
  }
  private flush(): void {
    if (this.closing || this.session === undefined || this.updateWork !== undefined) { return; }
    const revision = this.metadataRevision; const selection = this.selection;
    const work = this.publishPending(); this.updateWork = work;
    work.catch((error: Error) => {
      if (!this.closing && revision === this.metadataRevision && selection === this.selection) {
        this.value.error = nativeError(error, 'system publication'); this.value.metadataState = 'failed'; this.pending = undefined;
      }
    })
      .finally(() => { this.updateWork = undefined; if (this.pending !== undefined && !this.closing) { setTimeout(() => { this.flush(); }, 0); } });
  }
  private async publishPending(): Promise<void> {
    const session = this.session; if (session === undefined) { return; }
    for (let count = 0; count < 64 && !this.closing; count++) {
      const snapshot = this.pending; this.pending = undefined; if (snapshot === undefined) { return; }
      const selection = this.selection;
      this.updateControls(session, snapshot);
      const asset = sourceKey(snapshot); const revision = this.metadataRevision;
      const current = (): boolean => selection === this.selection && !this.closing && sourceKey(this.controller.snapshot()) === asset &&
        this.controller.snapshot().playbackEpoch === snapshot.playbackEpoch && revision === this.metadataRevision;
      if (asset !== this.asset || snapshot.durationMs !== this.metadataDuration || revision !== this.publishedMetadataRevision) {
        const metadata: avSession.AVMetadata = { assetId: asset, title: this.metadata?.title ?? this.configuration.title,
          artist: this.metadata?.artist ?? '', mediaImage: this.metadata?.artworkUri ?? '' };
        if (snapshot.durationMs !== undefined) { metadata.duration = snapshot.durationMs; }
        await session.setAVMetadata(metadata);
        if (!current()) { continue; }
        this.asset = asset; this.metadataDuration = snapshot.durationMs;
        this.publishedMetadataRevision = revision; this.value.metadataSourceId = snapshot.sourceId;
        this.value.metadataState = this.metadata === undefined ? 'cleared' : 'published';
      }
      if (!current()) { continue; }
      const state = snapshot.state === 'playing' ? avSession.PlaybackState.PLAYBACK_STATE_PLAY
        : snapshot.state === 'paused' || snapshot.state === 'prepared' ? avSession.PlaybackState.PLAYBACK_STATE_PAUSE
        : snapshot.state === 'completed' ? avSession.PlaybackState.PLAYBACK_STATE_COMPLETED
        : snapshot.state === 'error' ? avSession.PlaybackState.PLAYBACK_STATE_ERROR
        : snapshot.state === 'stopped' || snapshot.disposed ? avSession.PlaybackState.PLAYBACK_STATE_STOP
        : avSession.PlaybackState.PLAYBACK_STATE_PREPARE;
      const playback: avSession.AVPlaybackState = { state: state, speed: snapshot.playbackRate,
        position: { elapsedTime: snapshot.positionMs, updateTime: Date.now() },
        volume: snapshot.volume, maxVolume: 1, muted: snapshot.muted };
      if (snapshot.durationMs !== undefined) { playback.duration = snapshot.durationMs; }
      await session.setAVPlaybackState(playback);
      if (!current()) { continue; }
      this.value.publications++;
      if (this.configuration.backgroundAudio && !this.backgroundFailed) {
        try {
          if (snapshot.state === 'playing' && !this.background && this.agent !== undefined && !this.closing) {
            await backgroundTaskManager.startBackgroundRunning(this.context, backgroundTaskManager.BackgroundMode.AUDIO_PLAYBACK, this.agent);
            this.background = true; this.value.background = 'active';
          } else if (snapshot.state !== 'playing' && this.background) {
            await backgroundTaskManager.stopBackgroundRunning(this.context); this.background = false; this.value.background = 'inactive';
          }
        } catch (error) { this.backgroundFailed = true; this.value.background = 'failed'; this.value.error = nativeError(error, 'background audio'); }
      }
    }
  }
  snapshot(): VesperSystemSnapshot { return JSON.parse(JSON.stringify(this.value)) as VesperSystemSnapshot; }
  /** Sends through the platform AVSessionController, useful for host command integration tests. */
  async sendCommand(command: string): Promise<void> {
    if (this.closing || this.session === undefined || !['play', 'pause', 'stop'].includes(command)) { throw new VesperPlayerError('unsupported', 'System transport command is unavailable'); }
    if (command === 'play' && !this.controls.play || command === 'pause' && !this.controls.pause || command === 'stop' && !this.controls.stop) {
      throw new VesperPlayerError('unsupported', 'System transport command is disabled');
    }
    if (this.commandWork.size + this.commandControllers.size >= 8) { throw new VesperPlayerError('capacity', 'System command capacity reached'); }
    const session = this.session; const scope = new OperationScope(10000);
    // Install ownership and destruction before the public deadline race.
    const physical = session.getController().then(async (controller: avSession.AVSessionController): Promise<void> => {
      this.commandControllers.add(controller);
      try {
        scope.check(); if (this.closing) { throw new VesperPlayerError('closed', 'System session is closing'); }
        await controller.sendControlCommand({ command: command as avSession.AVControlCommandType });
      } finally { await controller.destroy(); this.commandControllers.delete(controller); }
    });
    this.commandWork.add(physical);
    physical.finally(() => { this.commandWork.delete(physical); }).catch(() => {});
    try {
      await scope.wait(physical);
    } finally { scope.close(); }
  }
  close(): Promise<void> {
    if (this.closeWork !== undefined) { return this.closeWork; }
    this.closing = true; this.value.state = 'closing'; this.unsubscribe(); this.pending = undefined;
    this.metadata = undefined; this.metadataRevision++;
    const physical = this.cleanup();
    const scope = new OperationScope(10000);
    const work = scope.wait(physical).finally(() => { scope.close(); });
    this.closeWork = work;
    // Physical cleanup remains owned after a caller deadline. Only its actual
    // rejection allows a retry; a logical timeout must not duplicate destroy.
    physical.catch(() => { this.closeWork = undefined; }); return work;
  }
  private async cleanup(): Promise<void> {
    await this.creation.catch(() => {}); await this.updateWork?.catch(() => {});
    await Promise.all(Array.from(this.commandWork).map((work: Promise<void>) => work.catch(() => {})));
    let failure: VesperPlayerError | undefined;
    for (const controller of Array.from(this.commandControllers)) {
      try { await controller.destroy(); this.commandControllers.delete(controller); }
      catch (error) { failure = nativeError(error, 'system command controller destruction'); }
    }
    if (this.background) {
      try { await backgroundTaskManager.stopBackgroundRunning(this.context); this.background = false; this.value.background = 'inactive'; }
      catch (error) { failure = nativeError(error, 'background audio release'); }
    }
    const session = this.session;
    if (session !== undefined) {
      try { session.off('play'); session.off('pause'); session.off('stop'); session.off('seek'); session.off('setSpeed'); } catch (_) {}
      try { await session.deactivate(); } catch (error) { this.value.error = nativeError(error, 'system deactivation'); }
      try { await session.destroy(); this.session = undefined; }
      catch (error) { failure = nativeError(error, 'system destruction'); }
    }
    if (failure !== undefined) { this.value.error = failure; throw failure; }
    this.removeCleanup();
    this.agent = undefined; this.value.state = 'closed';
    if (VesperSystemSession.owners.get(this.key) === this) { VesperSystemSession.owners.delete(this.key); }
  }
}
