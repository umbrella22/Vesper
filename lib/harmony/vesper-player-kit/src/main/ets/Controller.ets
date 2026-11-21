import { media } from '@kit.MediaKit';
import { audio } from '@kit.AudioKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { VesperSourceHandle, VesperSourceLease } from './SourceSession';
import { VesperPlayerError, VesperPlayerSnapshot, VesperPlayerEvent, VesperTrack, VesperExternalSubtitleSource, VesperExternalSubtitleState,
  VesperTrackSelection, VesperSourceActivationOptions, VesperSourceActivationResult } from './Types';
import { OperationScope, VesperCancellationToken, monotonicMs, nativeError } from './Operation';
import { NativeResourceLoader, NetworkResourceProvider, FileResourceProvider, ResourceProvider } from './ResourceLoader';
import { NativeResourceServer } from './ResourceServer';
import { RtspSession } from './RtspSession';
import { Transport } from './Transport';
import { AssetPins } from './FileStore';
import { boundedText } from './FileStore';
import { util } from '@kit.ArkTS';
import { SubtitleCue, parseSubtitles, subtitleAt } from './Subtitles';
import { loadDashText } from './DashText';
import { DashLiveResourceProvider, DashPlaybackPreparation, prepareDashPlayback } from './DashLive';
import { VesperPlaybackPolicy, VesperPlaybackPolicyResult, VesperNativeBufferingPreferences, resolvePolicy, bufferingStrategy } from './Policy';
import { common } from '@kit.AbilityKit';
import { Platform, VesperHarmonyCapabilities, VesperTrackConstraints, VesperTimeRange, VesperTimeRanges } from './Platform';
import { VesperPlaybackView, resolvePlaybackView } from './PlaybackView';
import { VesperTrackPreferences, VesperTrackPreferenceResult, validateTrackPreferences, resolveTrackPreferences, resolveExternalSubtitleDefault } from './TrackPreferences';
import { validateTrackConstraints, nativeTrackConstraints } from './TrackConstraints';
import { VesperPlaybackDiagnostics, playbackInfoObservation, playbackMetricsObservation } from './Diagnostics';
import { DiagnosticHistory, VesperDiagnosticReport, VesperDiagnosticReportOptions } from './DiagnosticReport';

class PlaybackInstance {
  readonly lease: VesperSourceLease;
  readonly epoch: string;
  readonly activationId: string;
  readonly started: number = monotonicMs();
  readonly scope: OperationScope;
  player?: media.AVPlayer;
  mediaSource?: media.MediaSource;
  file?: fs.File;
  loader?: NativeResourceLoader | NativeResourceServer;
  provider?: NetworkResourceProvider;
  live?: DashLiveResourceProvider;
  rtsp?: RtspSession;
  rtspHealthTimer?: number;
  releaseAsset?: () => void;
  subtitles: fs.File[] = [];
  subtitlePaths: string[] = [];
  eventTracks: media.MediaDescription[] = [];
  confirmed: Set<number> = new Set();
  sidecars: SubtitleCue[][] = [];
  sidecarLanguages: string[] = [];
  sidecarSources: (VesperExternalSubtitleSource | undefined)[] = [];
  subtitleCatalogReady: boolean = false;
  embeddedText: boolean = false;
  selectedNativeSubtitles: number[] = [];
  selectedSidecar: number = -1;
  nativeTextEnabled: boolean = true;
  deferredAudio?: VesperTrackSelection;
  subtitleTimer?: number;
  subtitleEnd: number = 0;
  closed: boolean = false;
  releaseWork?: Promise<void>;
  pending: Promise<void> = Promise.resolve();
  refreshWork?: Promise<void>;
  seekRevision: number = 0;
  rateRevision: number = 0;
  bitrateRevision: number = 0;
  commands: number = 0;
  buffering: VesperNativeBufferingPreferences = {};
  resumeIntent?: number;
  duck: number = 1;
  systemMuted: boolean = false;
  tail: Promise<void> = Promise.resolve();
  constructor(lease: VesperSourceLease, epoch: string, scope: OperationScope) {
    this.lease = lease; this.epoch = epoch; this.activationId = epoch; this.scope = scope;
  }
}

function numeric(description: media.MediaDescription, key: string, fallback: number): number {
  const value = description[key]; return typeof value === 'number' ? value : fallback;
}
function textual(description: media.MediaDescription, key: string): string {
  const value = description[key]; return typeof value === 'string' ? value : '';
}

/** Native-first AVPlayer facade. All public operations run on the owning ArkTS context. */
export class VesperPlayerController {
  private static physicalPlayers: number = 0;
  private static identity: number = 0;
  private current?: PlaybackInstance;
  private surface: string = '';
  private disposed: boolean = false;
  private owner?: string;
  private transport: Transport = new Transport();
  private context?: common.Context;
  private listeners: Set<(snapshot: VesperPlayerSnapshot) => void> = new Set();
  private events: VesperPlayerEvent[] = [];
  private diagnosticHistory: DiagnosticHistory = new DiagnosticHistory();
  private cleanup: Set<Promise<void>> = new Set();
  private retired: Set<PlaybackInstance> = new Set();
  private commandScopes: Set<OperationScope> = new Set();
  private commandPending: Map<OperationScope, Promise<void>> = new Map();
  private cleanupHooks: Set<() => Promise<void>> = new Set();
  private disposeWork?: Promise<void>;
  private releaseBarrier: Promise<void> = Promise.resolve();
  private allowRelease: () => void = (): void => {};
  private intentRevision: number = 0;
  private preferences: VesperTrackPreferences = {};
  private buffering: VesperNativeBufferingPreferences = {};
  private value: VesperPlayerSnapshot = {
    disposed: false, state: 'idle', positionMs: 0, timeline: 'unknown', playbackRate: 1,
    volume: 1, muted: false, playWhenReady: false, firstFrame: false, buffering: false, width: 0, height: 0,
    subtitle: '', subtitleState: { catalogState: 'empty', selectionState: 'system', externalTracks: [] },
    tracks: [], selection: { confirmedTrackIds: [] }, availableBitrates: [],
    cacheBytes: 0, loaderBytes: 0, activeResources: 0, releasedPlayers: 0,
    retainedNativePlayers: 0, listenerFailures: 0, droppedEvents: 0, audioOutputEvidence: 'unknown'
  };
  constructor(context?: common.Context) { this.context = context; }
  capabilities(): VesperHarmonyCapabilities { return Platform.capabilities(); }
  /** Bounded local feedback report; excludes media locators, headers, labels and error messages. */
  diagnosticReport(options: VesperDiagnosticReportOptions = {}): VesperDiagnosticReport {
    return this.diagnosticHistory.report(this.snapshot(), this.current?.lease.descriptor.protocol ?? 'unknown', monotonicMs(),
      this.transport.diagnosticRequests(), this.transport.droppedDiagnosticRequests, options);
  }
  exportDiagnosticReport(options: VesperDiagnosticReportOptions = {}): string { return JSON.stringify(this.diagnosticReport(options)); }
  playbackView(): VesperPlaybackView {
    return resolvePlaybackView(this.snapshot(), this.current?.lease.descriptor.protocol ?? 'unknown');
  }
  setPlaybackPolicy(policy: VesperPlaybackPolicy): Promise<VesperPlaybackPolicyResult> {
    try {
      this.check(); const resolved = resolvePolicy(policy, this.transport.configuredAttempts(), this.buffering);
      this.transport.setDefaultAttempts(resolved.attempts); this.buffering = resolved.buffering;
      this.value.policy = resolved.result; this.publish('policyChanged');
      return Promise.resolve(JSON.parse(JSON.stringify(resolved.result)) as VesperPlaybackPolicyResult);
    } catch (error) { return Promise.reject(error); }
  }
  /** Samples the current epoch. Loopback throughput and upstream transport totals are distinct. */
  refreshDiagnostics(): Promise<VesperPlaybackDiagnostics> {
    let result: VesperPlaybackDiagnostics = { playbackEpoch: '', sampledAtMonotonicMs: 0,
      buffer: { status: 'unavailable' }, metrics: { status: 'unavailable', unrecognizedKeys: [] },
      positionStatus: 'unavailable', presentationTimestampStatus: 'unavailable', timeRangesStatus: 'unavailable',
      nativeDownloadScope: 'unknown', upstreamRequestsSinceControllerCreation: 0,
      upstreamBytesSinceControllerCreation: 0, decoderExecutionEvidence: 'unknown' };
    return this.command(async (entry, player, scope): Promise<void> => {
      result.playbackEpoch = entry.epoch; result.sampledAtMonotonicMs = monotonicMs();
      result.nativeDownloadScope = entry.rtsp !== undefined || entry.loader instanceof NativeResourceServer ? 'loopback' :
        entry.lease.descriptor.protocol === 'file' ? 'local-file' : 'resource-loader';
      if (['prepared', 'playing', 'paused'].includes(player.state)) {
        try { playbackInfoObservation(await this.owned(entry, player.getPlaybackInfo(), scope), result); }
        catch (error) { scope.check(); result.buffer = { status: 'unavailable', errorCode: nativeError(error, 'playback info').code }; }
      }
      if (['prepared', 'playing', 'paused', 'completed', 'stopped'].includes(player.state)) {
        try { result.metrics = playbackMetricsObservation(await this.owned(entry, player.getPlaybackStatisticMetrics(), scope)); }
        catch (error) { scope.check(); result.metrics = { status: 'unavailable', errorCode: nativeError(error, 'playback metrics').code, unrecognizedKeys: [] }; }
      }
      if (['prepared', 'playing', 'paused', 'completed'].includes(player.state)) {
        try { result.nativePositionMs = player.getPlaybackPosition(); result.positionStatus = 'observed'; }
        catch (error) { scope.check(); }
      }
      // Querying PTS in prepared emits a native error callback and can fail later commands.
      if (['playing', 'paused', 'completed'].includes(player.state)) {
        try { result.presentationTimestampUs = player.getCurrentPresentationTimestamp(); result.presentationTimestampStatus = 'observed'; }
        catch (error) { scope.check(); }
      }
      if (['prepared', 'playing', 'paused', 'completed'].includes(player.state)) {
        try {
          const loaded = Platform.ranges(await this.owned(entry, player.getLoadedTimeRanges(), scope));
          const seekable = Platform.ranges(await this.owned(entry, player.getSeekableTimeRanges(), scope));
          this.value.timeRanges = { loaded: loaded, seekable: seekable }; result.timeRangesStatus = 'observed';
        } catch (error) { scope.check(); }
      }
      if (!this.accepts(entry)) { throw new VesperPlayerError('stale_epoch', 'Diagnostic sample belongs to a retired playback'); }
      result.upstreamRequestsSinceControllerCreation = this.transport.requests;
      result.upstreamBytesSinceControllerCreation = this.transport.bytesReceived;
      this.value.diagnostics = result;
      if (this.value.nativeBuffering?.playbackEpoch === entry.epoch) { this.value.nativeBuffering.observed = result.buffer; }
      this.publish('diagnosticsChanged');
    }).then((): VesperPlaybackDiagnostics => JSON.parse(JSON.stringify(result)) as VesperPlaybackDiagnostics);
  }
  /** Owned system and presentation resources close before the physical player. */
  registerCleanup(cleanup: () => Promise<void>): () => void {
    this.check();
    if (this.cleanupHooks.size >= 8) { throw new VesperPlayerError('capacity', 'Controller cleanup-owner capacity reached'); }
    this.cleanupHooks.add(cleanup); return (): void => { this.cleanupHooks.delete(cleanup); };
  }

  snapshot(): VesperPlayerSnapshot {
    const entry = this.current;
    if (entry !== undefined) {
      this.value.cacheBytes = entry.provider?.cacheBytes ?? 0;
      this.value.loaderBytes = entry.loader?.deliveredBytes ?? 0;
      this.value.activeResources = entry.loader?.activeResources() ?? 0;
      this.value.resourceLoaderInfo = entry.loader?.inventory();
      if (entry.provider?.timeline === 'live') { this.value.timeline = 'live'; }
    } else { this.value.activeResources = 0; }
    this.value.retainedNativePlayers = this.retired.size + (entry === undefined ? 0 : 1);
    return JSON.parse(JSON.stringify(this.value)) as VesperPlayerSnapshot;
  }
  subscribe(listener: (snapshot: VesperPlayerSnapshot) => void): () => void {
    this.check();
    if (this.listeners.size >= 64) { throw new VesperPlayerError('capacity', 'Controller listener capacity reached'); }
    this.listeners.add(listener);
    try { listener(this.snapshot()); } catch (_) { this.value.listenerFailures++; }
    return (): void => { this.listeners.delete(listener); };
  }
  drainEvents(): VesperPlayerEvent[] { const result = this.events; this.events = []; return result; }
  private publish(type: string, error?: VesperPlayerError, positionMs?: number): void {
    const event: VesperPlayerEvent = { type: type, atMonotonicMs: monotonicMs(),
      playbackEpoch: this.current?.epoch, nativeState: this.value.state, error: error, positionMs: positionMs };
    if (this.events.length >= 128) { this.events.shift(); this.value.droppedEvents++; }
    this.events.push(event);
    this.diagnosticHistory.record(type, this.value, event.atMonotonicMs, positionMs);
    for (const listener of Array.from(this.listeners)) {
      try { listener(this.snapshot()); } catch (_) { this.value.listenerFailures++; }
    }
  }
  private check(): void { if (this.disposed) { throw new VesperPlayerError('disposed', 'Controller was disposed'); } }
  private accepts(entry: PlaybackInstance): boolean { return !this.disposed && !entry.closed && this.current === entry; }

  attachSurface(surfaceId: string): void {
    this.check();
    if (surfaceId.length === 0) { throw new VesperPlayerError('invalid_argument', 'Surface identity is empty'); }
    const player = this.current?.player;
    if (player !== undefined && player.state !== 'idle') {
      try { player.surfaceId = surfaceId; } catch (error) { throw nativeError(error, 'Surface attachment'); }
    }
    this.surface = surfaceId;
  }
  detachSurface(): void {
    this.check();
    const player = this.current?.player;
    if (player !== undefined && player.state !== 'idle') {
      try { player.surfaceId = ''; } catch (error) { throw nativeError(error, 'Surface detachment'); }
    }
    this.surface = '';
  }
  async activate(handle: VesperSourceHandle, options: VesperSourceActivationOptions = {},
    cancellation?: VesperCancellationToken): Promise<VesperSourceActivationResult> {
    this.check();
    if (this.owner !== undefined) { throw new VesperPlayerError('sequence_owned', 'Detach the sequence before direct activation'); }
    return this.activateLease(handle.acquireLease(), options, cancellation);
  }
  /** @internal Sequence ownership and accepted leases remain opaque to applications. */
  claimSequence(identity: string): void {
    this.check();
    if (this.owner !== undefined && this.owner !== identity) { throw new VesperPlayerError('sequence_owned', 'Controller already has a sequence'); }
    this.owner = identity;
  }
  /** @internal */
  detachSequence(identity: string): void {
    if (this.owner !== identity) { return; }
    this.owner = undefined; this.cancelPending(new VesperPlayerError('detached', 'Sequence was detached'));
  }
  /** @internal */
  activateSequence(identity: string, lease: VesperSourceLease, options: VesperSourceActivationOptions,
    cancellation?: VesperCancellationToken): Promise<VesperSourceActivationResult> {
    this.check();
    if (this.owner !== identity) { lease.release(); return Promise.reject(new VesperPlayerError('detached', 'Sequence is detached')); }
    return this.activateLease(lease, options, cancellation);
  }
  private async activateLease(lease: VesperSourceLease, options: VesperSourceActivationOptions,
    cancellation?: VesperCancellationToken): Promise<VesperSourceActivationResult> {
    let scope: OperationScope;
    let createdScope: OperationScope | undefined;
    let releaseAsset: (() => void) | undefined;
    const position = options.startPositionMs ?? 0;
    const rate = options.playbackRate ?? 1;
    try {
      if (!Number.isSafeInteger(position) || position < 0 || !Number.isFinite(rate) || rate < 0.25 || rate > 4) {
        throw new VesperPlayerError('invalid_argument', 'Activation position or playback rate is invalid');
      }
      lease.validateActivation();
      createdScope = new OperationScope(options.timeoutMs ?? 30000, cancellation); createdScope.check(); scope = createdScope;
      if (lease.descriptor.kind === 'local') { releaseAsset = AssetPins.acquire(lease.descriptor.uri); }
    } catch (error) { createdScope?.close(); releaseAsset?.(); lease.release(); throw nativeError(error, 'activation validation'); }
    if (VesperPlayerController.physicalPlayers >= 4 || VesperPlayerController.identity >= Number.MAX_SAFE_INTEGER) {
      scope.close(); releaseAsset?.(); lease.release(); throw new VesperPlayerError('capacity', 'Physical player capacity reached');
    }
    this.cancelPending(new VesperPlayerError('superseded', 'A newer activation replaced this operation'));
    const previous = this.current;
    const entry = new PlaybackInstance(lease, `${++VesperPlayerController.identity}`, scope);
    entry.buffering = { preferredBufferDurationMs: this.buffering.preferredBufferDurationMs,
      preferredBufferDurationForPlayingMs: this.buffering.preferredBufferDurationForPlayingMs };
    entry.releaseAsset = releaseAsset;
    VesperPlayerController.physicalPlayers++;
    this.current = entry;
    const previousRelease = previous === undefined ? Promise.resolve() : this.retire(previous);
    this.value.sourceId = lease.sourceId; this.value.sessionId = lease.sessionId;
    this.value.activationId = entry.activationId; this.value.playbackEpoch = entry.epoch;
    this.value.state = 'initializing'; this.value.error = undefined; this.value.firstFrame = false;
    this.value.firstFrameMs = undefined; this.value.positionMs = 0; this.value.durationMs = undefined;
    this.value.buffering = false; this.value.bufferingPercent = undefined; this.value.cachedDurationMs = undefined; this.value.lastBufferingUpdate = undefined;
    this.value.timeRanges = undefined; this.value.trackConstraints = undefined;
    this.value.nativeBuffering = { playbackEpoch: entry.epoch, requested: entry.buffering, status: 'pending', reason: 'Source initialization pending' };
    this.value.diagnostics = undefined;
    this.value.playbackRate = 1; this.value.width = 0; this.value.height = 0;
    this.value.timeline = 'unknown'; this.value.subtitle = ''; this.value.tracks = [];
    this.value.subtitleState = { playbackEpoch: entry.epoch, catalogState: 'loading', selectionState: 'system',
      externalTracks: (lease.descriptor.externalSubtitles ?? []).map((source: VesperExternalSubtitleSource): VesperExternalSubtitleState => ({ id: source.id, state: 'loading' })) };
    this.value.selection = { confirmedTrackIds: [] }; this.value.trackSelections = {}; this.value.trackPreferences = undefined; this.value.availableBitrates = [];
    this.value.cacheBytes = 0; this.value.loaderBytes = 0; this.value.activeResources = 0;
    this.value.resourceLoaderInfo = undefined;
    this.value.playWhenReady = options.playWhenReady ?? true;
    this.value.intentRevision = ++this.intentRevision;
    // Creation may outlive logical cancellation. It still owns one physical slot.
    const creation = Promise.resolve().then(() => media.createAVPlayer()).then((player: media.AVPlayer): media.AVPlayer => {
      entry.player = player; return player;
    });
    const ready = this.owned(entry, creation, scope);
    this.publish('activationStarted');
    let phase = 'native creation';
    try {
      const player = await ready;
      // A replacement may be created early, but must not prepare or attach the
      // same Surface while the preceding native instance still owns it.
      phase = 'previous native release'; await this.owned(entry, previousRelease, scope);
      scope.check();
      this.installCallbacks(entry, player);
      phase = 'source initialization';
      const descriptor = lease.descriptor;
      if (descriptor.kind === 'local' && descriptor.protocol === 'file') {
        const file = await this.owned(entry, fs.open(descriptor.uri, fs.OpenMode.READ_ONLY).then((opened: fs.File): fs.File => {
          entry.file = opened; return opened;
        }), scope);
        const stat = await this.owned(entry, fs.stat(file.fd), scope);
        scope.check(); player.fdSrc = { fd: file.fd, offset: 0, length: stat.size };
        this.value.nativeBuffering = { playbackEpoch: entry.epoch, requested: entry.buffering, status: 'unsupported',
          reason: 'Local fdSrc does not accept a MediaSource playback strategy' };
      } else if (descriptor.protocol === 'rtsp') {
        if (descriptor.kind !== 'remote' || (descriptor.headers !== undefined && Object.keys(descriptor.headers).length > 0)) {
          throw new VesperPlayerError('rtsp_unsupported_input', 'RTSP requires a remote source without HTTP headers.');
        }
        const session = new RtspSession(descriptor.uri);
        entry.rtsp = session;
        const uri = await this.owned(entry, session.playlist(scope), scope);
        entry.mediaSource = media.createMediaSourceWithUrl(uri, {});
      } else {
        const local = descriptor.kind === 'local' ? new FileResourceProvider(descriptor.uri) : undefined;
        const network = local === undefined ? new NetworkResourceProvider(this.transport, lease.startupCache(), descriptor.headers ?? {}, descriptor.uri) : undefined;
        entry.provider = network;
        let provider: ResourceProvider | undefined = local ?? network;
        if (provider === undefined) { throw new VesperPlayerError('invalid_source', 'Resource provider is unavailable'); }
        let sourceUri = local?.nativeUrl ?? descriptor.uri;
        if (descriptor.protocol === 'dash') {
          const prepared = await this.owned(entry, prepareDashPlayback(provider, sourceUri, scope).then((value: DashPlaybackPreparation): DashPlaybackPreparation => {
            entry.live = value.live; return value;
          }), scope);
          if (prepared.live !== undefined) {
            if (network === undefined) { throw new VesperPlayerError('unsupported', 'Dynamic DASH requires a remote source'); }
            provider = prepared.live; sourceUri = prepared.live.nativeUrl; network.timeline = 'live';
          } else {
            const textTracks = await this.owned(entry, loadDashText(provider, sourceUri, scope, prepared.snapshot), scope);
            entry.embeddedText = textTracks.length > 0;
            for (const track of textTracks) { entry.sidecars.push(track.cues); entry.sidecarLanguages.push(track.language); entry.sidecarSources.push(undefined); }
          }
          // Playback cache evidence starts when AVPlayer receives the loader.
          if (network !== undefined) { network.cacheBytes = 0; }
        }
        let sourceHeaders = descriptor.headers ?? {};
        let delegate: NativeResourceLoader | undefined;
        if (network !== undefined && ['hls', 'dash'].includes(descriptor.protocol)) {
          const server = new NativeResourceServer(provider, descriptor.protocol, (error: VesperPlayerError): void => { this.failEntry(entry, error); });
          entry.loader = server;
          sourceUri = await this.owned(entry, server.start(sourceUri), scope); sourceHeaders = server.headers();
        } else {
          delegate = new NativeResourceLoader(provider, (error: VesperPlayerError): void => { this.failEntry(entry, error); }); entry.loader = delegate;
        }
        const source = media.createMediaSourceWithUrl(sourceUri, sourceHeaders);
        // MediaSource's finalizer clears its loader callbacks even while the
        // native player still needs them. Keep it through physical release.
        entry.mediaSource = source;
        if (delegate !== undefined) { source.setMediaResourceLoaderDelegate(delegate.delegate()); }
      }
      let strategy: media.PlaybackStrategy | undefined;
      if (entry.mediaSource !== undefined) {
        // Remote HLS defaults to a two-second preference. Explicit preferences
        // are frozen for this activation and never masquerade as strict limits.
        const remoteHls = descriptor.kind === 'remote' &&
          (entry.rtsp !== undefined || descriptor.protocol === 'hls' || entry.live !== undefined);
        strategy = bufferingStrategy(entry.buffering, remoteHls);
        // AVPlayer can return void on API 26. Native state confirms readiness.
        const sourceWork = player.setMediaSource(entry.mediaSource, strategy);
        await this.owned(entry, Promise.resolve(sourceWork), scope);
      }
      await scope.until(() => player.state === 'initialized');
      scope.check();
      if (entry.mediaSource !== undefined) {
        this.value.nativeBuffering = { playbackEpoch: entry.epoch, requested: entry.buffering, status: strategy === undefined ? 'systemDefault' : 'accepted',
          submitted: strategy === undefined ? undefined : { preferredBufferDurationMs: strategy.preferredBufferDuration === undefined ? undefined : strategy.preferredBufferDuration * 1000,
            preferredBufferDurationForPlayingMs: strategy.preferredBufferDurationForPlaying === undefined ? undefined : strategy.preferredBufferDurationForPlaying * 1000 },
          reason: 'MediaSource initialized; submitted hints have no native effective-threshold readback' };
      }
      phase = 'Surface attachment';
      if (this.surface.length > 0) { player.surfaceId = this.surface; }
      phase = 'external subtitles';
      await this.owned(entry, this.externalSubtitles(entry, player), scope);
      phase = 'native prepare';
      await this.owned(entry, player.prepare(), scope);
      await scope.until(() => player.state === 'prepared');
      phase = 'native playback configuration';
      player.videoScaleType = media.VideoScaleType.VIDEO_SCALE_TYPE_SCALED_ASPECT;
      player.audioInterruptMode = audio.InterruptMode.INDEPENDENT_MODE;
      this.value.durationMs = entry.rtsp === undefined && player.duration > 0 ? player.duration : undefined;
      this.value.timeline = entry.rtsp !== undefined || entry.live !== undefined ? 'live' : player.duration > 0 ? 'vod' : 'unknown';
      this.value.width = player.width; this.value.height = player.height;
      await this.owned(entry, this.refreshTracks(entry), scope);
      await this.applyPreferencesEntry(entry, player, scope);
      if (position > 0) { await this.seekEntry(entry, player, position, scope); }
      if (rate !== 1) { await this.rateEntry(entry, player, rate, scope); }
      player.setVolume(this.value.muted ? 0 : this.value.volume);
      if (this.value.playWhenReady) {
        phase = 'native play';
        await this.owned(entry, player.play(), scope); await scope.until(() => player.state === 'playing');
      }
      scope.check(); lease.validateActivation();
      if (!this.accepts(entry)) { throw new VesperPlayerError('superseded', 'Activation is no longer current'); }
      if (entry.rtsp !== undefined) {
        entry.rtspHealthTimer = setInterval((): void => {
          if (!this.accepts(entry)) { return; }
          try { entry.rtsp?.checkHealth(); }
          catch (error) {
            if (entry.rtspHealthTimer !== undefined) { clearInterval(entry.rtspHealthTimer); entry.rtspHealthTimer = undefined; }
            this.failEntry(entry, nativeError(error, 'RTSP bridge status'));
          }
        }, 500);
      }
      scope.close(); this.publish('activationCommitted');
      return { sourceId: lease.sourceId, sessionId: lease.sessionId, activationId: entry.activationId, playbackEpoch: entry.epoch };
    } catch (error) {
      const failure = nativeError(error, phase);
      if (this.current === entry) { this.value.error = failure; this.value.state = 'error'; this.publish('activationFailed', failure); this.current = undefined; }
      // Wait for creation to exit before physical release. A pending file open is
      // separately retained by the instance until it has attached its descriptor.
      scope.close(); this.retire(entry);
      throw failure;
    }
  }
  private async externalSubtitles(entry: PlaybackInstance, player: media.AVPlayer): Promise<void> {
    const subtitles = entry.lease.descriptor.externalSubtitles ?? [];
    const token = new VesperCancellationToken(); const remove = entry.scope.onFailure(() => { token.cancel(); });
    try {
      for (const source of subtitles) {
        entry.scope.check(); const index = entry.sidecars.length;
        entry.sidecars.push([]); entry.sidecarLanguages.push(source.language ?? ''); entry.sidecarSources.push(source);
        const state = this.value.subtitleState?.externalTracks.find((item) => item.id === source.id);
        let stage = 'load';
        try {
          let text: string;
          if (!source.uri.startsWith('/')) {
            const response = await this.transport.get(source.uri, { headers: source.headers ?? {}, maxBytes: 256 * 1024, cancellation: token });
            entry.scope.check(); stage = 'parse';
            text = util.TextDecoder.create('utf-8', { fatal: true }).decodeWithStream(new Uint8Array(response.bytes));
          } else { text = await boundedText(source.uri, 256 * 1024); stage = 'parse'; }
          entry.scope.check();
          if (!this.accepts(entry)) { throw new VesperPlayerError('stale_epoch', 'Subtitle source was replaced'); }
          entry.sidecars[index] = parseSubtitles(text, source.mimeType);
          if (state !== undefined) { state.state = 'ready'; }
        } catch (error) {
          entry.scope.check();
          if (!this.accepts(entry)) { throw new VesperPlayerError('stale_epoch', 'Subtitle source was replaced'); }
          if (state !== undefined) { state.state = 'failed'; state.error = { stage: stage, code: nativeError(error, 'external subtitle').code, sourceId: source.id }; }
        }
        this.updateSubtitleState(entry); this.publish('subtitleCatalogChanged');
      }
    } finally { remove(); }
  }
  private updateSubtitleState(entry: PlaybackInstance): void {
    const state = this.value.subtitleState;
    if (!this.accepts(entry) || state === undefined || state.playbackEpoch !== entry.epoch) { return; }
    const failed = state.externalTracks.filter((item) => item.state === 'failed');
    const available = this.value.tracks.some((track: VesperTrack): boolean => track.kind === 'subtitle' && track.selectionSupported !== false);
    state.catalogState = !entry.subtitleCatalogReady ? 'loading' : failed.length > 0 ? (available ? 'partial' : 'failed') : available ? 'ready' : 'empty';
    state.catalogError = failed[0]?.error;
    const selection = this.value.trackSelections?.subtitle;
    const selected = this.value.tracks.find((track: VesperTrack): boolean => track.kind === 'subtitle' && track.selected);
    state.selectedTrackId = selected?.id; state.selectedStableId = selected?.stableId;
    state.selectionState = selection?.outcome === 'pending' ? 'selecting' : selection?.outcome === 'failed' ? 'failed' :
      selection?.effective?.mode === 'off' ? 'off' : selected !== undefined ? 'selected' : 'system';
    state.selectionError = selection?.outcome === 'failed' ? { stage: 'select', code: selection.errorCode ?? 'native_failure' } : undefined;
  }
  private updateSubtitleTimer(entry: PlaybackInstance): void {
    const active = this.accepts(entry) && entry.selectedSidecar >= 0 && this.value.state === 'playing';
    if (active && entry.subtitleTimer === undefined) {
      entry.subtitleTimer = setInterval(() => { if (this.accepts(entry)) { this.updateSubtitle(entry); } }, 100);
    } else if (!active && entry.subtitleTimer !== undefined) { clearInterval(entry.subtitleTimer); entry.subtitleTimer = undefined; }
  }
  private updateSubtitle(entry: PlaybackInstance): void {
    const text = entry.selectedSidecar >= 0 ? subtitleAt(entry.sidecars[entry.selectedSidecar], this.value.positionMs)
      : entry.subtitleEnd > 0 && this.value.positionMs >= entry.subtitleEnd ? '' : this.value.subtitle;
    if (text !== this.value.subtitle) { this.value.subtitle = text; this.publish('subtitle'); }
  }
  setTrackConstraints(constraints: VesperTrackConstraints, cancellation?: VesperCancellationToken, timeoutMs: number = 10000): Promise<VesperTrackConstraints> {
    let copy: VesperTrackConstraints;
    try { copy = validateTrackConstraints(constraints); } catch (error) { return Promise.reject(error); }
    this.advanceIntent();
    let effective: VesperTrackConstraints = {};
    return this.command(async (entry, player, scope): Promise<void> => {
      this.value.trackConstraints = { playbackEpoch: entry.epoch, requested: copy, status: 'requested' };
      this.publish('constraintsRequested');
      try {
        await this.owned(entry, player.setTrackSelectionFilter(copy), scope);
        effective = nativeTrackConstraints(await this.owned(entry, player.getTrackSelectionFilter(), scope));
        this.value.trackConstraints = { playbackEpoch: entry.epoch, requested: copy, accepted: effective, status: 'accepted' };
      } catch (error) {
        if (this.accepts(entry)) {
          this.value.trackConstraints = { playbackEpoch: entry.epoch, requested: copy, status: 'failed', errorCode: nativeError(error, 'track filter').code };
          this.publish('constraintsFailed');
        }
        throw nativeError(error, 'track filter');
      }
      this.publish('constraintsConfirmed');
    }, cancellation, timeoutMs).then(() => JSON.parse(JSON.stringify(effective)) as VesperTrackConstraints);
  }
  timeRanges(): Promise<VesperTimeRanges> {
    let ranges: VesperTimeRanges = { loaded: [], seekable: [] };
    return this.command(async (entry, player, scope): Promise<void> => {
      const loaded = Platform.ranges(await this.owned(entry, player.getLoadedTimeRanges(), scope));
      const seekable = Platform.ranges(await this.owned(entry, player.getSeekableTimeRanges(), scope));
      ranges = { loaded: loaded, seekable: seekable };
      this.value.timeRanges = ranges; this.publish('timeRangesChanged');
    }).then(() => ranges);
  }
  seekToDefaultPosition(): Promise<void> {
    return this.command(async (entry, player, scope): Promise<void> => {
      if (this.value.timeline === 'live') {
        throw new VesperPlayerError('unsupported', 'Live default-position seeking is not supported on the resource-loader route');
      }
      const ranges = Platform.ranges(await this.owned(entry, player.getSeekableTimeRanges(), scope));
      if (!ranges.some((range: VesperTimeRange) => range.endMs > range.startMs)) {
        throw new VesperPlayerError('unsupported', 'The current native source has no seekable access point');
      }
      const revision = entry.seekRevision; player.seekToDefaultPosition(); await scope.until(() => entry.seekRevision !== revision);
    });
  }
  private owned<T>(entry: PlaybackInstance, physical: Promise<T>, scope: OperationScope): Promise<T> {
    const settled = physical.then(() => {}, () => {});
    entry.pending = Promise.all([entry.pending, settled]).then(() => {});
    const command = this.commandPending.get(scope);
    if (command !== undefined) { this.commandPending.set(scope, Promise.all([command, settled]).then(() => {})); }
    return scope.wait(physical);
  }
  private installCallbacks(entry: PlaybackInstance, player: media.AVPlayer): void {
    player.on('endOfStream', (): void => { if (this.accepts(entry)) { this.publish('endOfStream'); } });
    player.on('audioInterrupt', (event: audio.InterruptEvent): void => { this.interrupt(entry, event); });
    player.on('audioOutputDeviceChangeWithInfo', (info: audio.AudioStreamDeviceChangeInfo): void => {
      if (!this.accepts(entry)) { return; } this.value.audioRouteReason = info.changeReason;
      if (info.changeReason === audio.AudioStreamDeviceChangeReason.REASON_OLD_DEVICE_UNAVAILABLE) {
        this.pause().catch((error: Error) => { this.failEntry(entry, nativeError(error, 'audio route pause')); });
      }
      this.publish('audioRouteChanged');
    });
    player.on('stateChange', (state: media.AVPlayerState): void => {
      if (!this.accepts(entry)) { return; } this.value.state = state;
      this.updateSubtitleTimer(entry);
      if (['completed', 'stopped', 'released', 'error'].includes(state)) { this.value.buffering = false; }
      this.publish('stateChanged');
    });
    player.on('bufferingUpdate', (type: media.BufferingInfoType, value: number): void => {
      if (!this.accepts(entry)) { return; }
      this.value.lastBufferingUpdate = { nativeType: type, value: value };
      if (type === media.BufferingInfoType.BUFFERING_START) { this.value.buffering = true; }
      else if (type === media.BufferingInfoType.BUFFERING_END) { this.value.buffering = false; }
      else if (type === media.BufferingInfoType.BUFFERING_PERCENT && Number.isFinite(value) && value >= 0 && value <= 100) { this.value.bufferingPercent = value; }
      else if (type === media.BufferingInfoType.CACHED_DURATION && Number.isFinite(value) && value >= 0) { this.value.cachedDurationMs = value; }
      else { this.publish('bufferingUnknown'); return; }
      this.publish('bufferingChanged');
    });
    player.on('error', (error): void => { this.failEntry(entry, nativeError(error, 'AVPlayer callback')); });
    player.on('timeUpdate', (position: number): void => {
      if (!this.accepts(entry)) { return; } this.value.positionMs = Math.max(0, position); this.updateSubtitle(entry); this.publish('position');
    });
    player.on('durationUpdate', (duration: number): void => {
      if (!this.accepts(entry) || entry.lease.descriptor.protocol === 'rtsp') { return; }
      this.value.durationMs = duration > 0 ? duration : undefined;
    });
    player.on('videoSizeChange', (width: number, height: number): void => {
      if (!this.accepts(entry)) { return; } this.value.width = width; this.value.height = height; this.publish('videoSizeChanged');
    });
    player.on('startRenderFrame', (): void => {
      if (!this.accepts(entry)) { return; } this.value.firstFrame = true;
      this.value.firstFrameMs = monotonicMs() - entry.started; this.publish('firstFrame');
      if ((entry.provider?.cacheBytes ?? 0) > 0) { entry.lease.startupCache().confirmPlayback(); }
    });
    player.on('seekDone', (position: number): void => {
      if (!this.accepts(entry)) { return; }
      entry.seekRevision++;
      if (Number.isFinite(position) && position >= 0) { this.value.positionMs = position; }
      this.value.subtitle = '';
      this.updateSubtitle(entry);
      this.publish('seekDone', undefined, position);
    });
    player.on('playbackRateDone', (rate: number): void => {
      if (this.accepts(entry)) { entry.rateRevision++; this.value.playbackRate = rate; this.publish('rateChanged'); }
    });
    player.on('availableBitrates', (values: number[]): void => {
      if (this.accepts(entry)) { this.value.availableBitrates = values.slice(0, 256); }
    });
    player.on('bitrateDone', (value: number): void => {
      if (this.accepts(entry)) {
        entry.bitrateRevision++; this.value.selection.effectiveBitrate = value;
        if (this.value.trackSelections?.video !== undefined) { this.value.trackSelections.video.effectiveBitrate = value; }
        this.publish('selectionChanged');
      }
    });
    player.on('subtitleUpdate', (value: media.SubtitleInfo): void => {
      if (this.accepts(entry) && entry.nativeTextEnabled && !entry.embeddedText && entry.selectedSidecar < 0) {
        entry.subtitleEnd = (value.startTime ?? this.value.positionMs) + (value.duration ?? 0);
        this.value.subtitle = (value.text ?? '').substring(0, 16384); this.publish('subtitle');
      }
    });
    player.on('trackChange', (index: number, selected: boolean): void => {
      if (!this.accepts(entry)) { return; }
      const kind = this.value.tracks.find((track: VesperTrack): boolean => track.id === index)?.kind;
      if (selected && (kind === 'audio' || kind === 'video')) {
        for (const previous of entry.confirmed) {
          if (this.value.tracks.some((track: VesperTrack): boolean => track.id === previous && track.kind === kind)) { entry.confirmed.delete(previous); }
        }
      }
      if (selected) { entry.confirmed.add(index); } else { entry.confirmed.delete(index); }
      if (['prepared', 'playing', 'paused'].includes(player.state)) {
        this.refreshTracks(entry).catch((error: Error) => { this.failEntry(entry, nativeError(error, 'track refresh')); });
      }
    });
    player.on('trackInfoUpdate', (tracks: media.MediaDescription[]): void => {
      if (this.accepts(entry)) { entry.eventTracks = tracks.slice(0, 256); }
      if (this.accepts(entry) && ['prepared', 'playing', 'paused'].includes(player.state)) {
        this.refreshTracks(entry).then(() => { if (this.accepts(entry)) { this.publish('tracksChanged'); } },
          (error: Error) => { this.failEntry(entry, nativeError(error, 'track catalog update')); });
      }
    });
  }
  private failEntry(entry: PlaybackInstance, error: VesperPlayerError): void {
    if (!this.accepts(entry)) { return; }
    this.value.error = error; entry.scope.fail(error);
    for (const scope of this.commandScopes) { scope.fail(error); }
    this.publish('error', error);
  }
  private applyVolume(entry: PlaybackInstance, player: media.AVPlayer): void {
    player.setVolume(this.value.muted || entry.systemMuted ? 0 : this.value.volume * entry.duck);
  }
  private interrupt(entry: PlaybackInstance, event: audio.InterruptEvent): void {
    if (!this.accepts(entry)) { return; }
    this.value.interruption = `${event.eventType}:${event.forceType}:${event.hintType}`; this.publish('audioInterruption');
    const hint = event.hintType;
    if (hint === audio.InterruptHint.INTERRUPT_HINT_PAUSE || hint === audio.InterruptHint.INTERRUPT_HINT_STOP) {
      entry.resumeIntent = hint === audio.InterruptHint.INTERRUPT_HINT_PAUSE && this.value.playWhenReady ? this.intentRevision : undefined;
      this.command(async (current, player, scope): Promise<void> => {
        if (player.state === 'playing') { await this.owned(current, player.pause(), scope); }
      }).catch((error: Error) => { this.failEntry(entry, nativeError(error, 'interruption pause')); });
    } else if (hint === audio.InterruptHint.INTERRUPT_HINT_RESUME) {
      const intent = entry.resumeIntent; entry.resumeIntent = undefined;
      if (intent !== undefined && intent === this.intentRevision && this.value.playWhenReady) {
        this.command(async (current, player, scope): Promise<void> => {
          if (intent !== this.intentRevision || !this.value.playWhenReady) { return; }
          await this.owned(current, player.play(), scope);
        }).catch((error: Error) => { this.failEntry(entry, nativeError(error, 'interruption resume')); });
      }
    } else if (event.forceType === audio.InterruptForceType.INTERRUPT_SHARE) {
      if (hint === audio.InterruptHint.INTERRUPT_HINT_DUCK) { entry.duck = 0.2; }
      else if (hint === audio.InterruptHint.INTERRUPT_HINT_UNDUCK) { entry.duck = 1; }
      else if (hint === audio.InterruptHint.INTERRUPT_HINT_MUTE) { entry.systemMuted = true; }
      else if (hint === audio.InterruptHint.INTERRUPT_HINT_UNMUTE) { entry.systemMuted = false; }
      else { return; }
      try { if (entry.player !== undefined) { this.applyVolume(entry, entry.player); } } catch (error) { this.failEntry(entry, nativeError(error, 'interruption volume')); }
    }
  }
  private refreshTracks(entry: PlaybackInstance): Promise<void> {
    if (entry.refreshWork !== undefined) { return entry.refreshWork; }
    const work = this.readTracks(entry).catch((error: Error): void => {
      const state = this.value.subtitleState;
      if (this.accepts(entry) && state !== undefined) {
        state.catalogState = 'failed'; state.catalogError = { stage: 'catalog', code: nativeError(error, 'subtitle catalog').code };
        this.publish('subtitleCatalogFailed');
      }
      throw error;
    });
    entry.refreshWork = work;
    entry.pending = Promise.all([entry.pending, work.then(() => {}, () => {})]).then(() => {});
    work.finally(() => { entry.refreshWork = undefined; }).catch(() => {});
    return work;
  }
  private async readTracks(entry: PlaybackInstance): Promise<void> {
    const player = entry.player;
    if (player === undefined || !['prepared', 'playing', 'paused'].includes(player.state)) { return; }
    const nativeDescriptions = await player.getTrackDescription();
    const byId: Map<number, media.MediaDescription> = new Map();
    for (const description of nativeDescriptions.concat(entry.eventTracks)) { byId.set(numeric(description, 'track_index', -1), description); }
    const descriptions = Array.from(byId.values());
    if (descriptions.length > 256) { throw new VesperPlayerError('capacity', 'Native track inventory exceeds capacity'); }
    const selected = await player.getSelectedTracks();
    for (const index of entry.confirmed) {
      if (selected.includes(index)) { entry.confirmed.delete(index); continue; }
      const description = byId.get(index);
      const type = description === undefined ? -1 : numeric(description, 'track_type', -1);
      if (type === 0 || type === 1) {
        for (let position = selected.length - 1; position >= 0; position--) {
          const previous = byId.get(selected[position]);
          if (previous !== undefined && numeric(previous, 'track_type', -1) === type) { selected.splice(position, 1); }
        }
      }
      selected.push(index);
    }
    if (!this.accepts(entry)) { return; }
    this.value.tracks = descriptions.map((description: media.MediaDescription): VesperTrack => {
      const type = numeric(description, 'track_type', -1);
      const id = numeric(description, 'track_index', -1);
      const selectionSupported = [0, 1, 2].includes(type);
      const width = numeric(description, 'width', 0); const height = numeric(description, 'height', 0); const bitrate = numeric(description, 'bitrate', 0);
      return { id: id, nativeType: type, kind: type === 0 ? 'audio' : type === 1 ? 'video' : type === 2 ? 'subtitle' : `unknown:${type}`,
        mimeType: textual(description, 'codec_mime'), language: textual(description, 'language'), selected: selected.includes(id),
        width: width > 0 ? width : undefined, height: height > 0 ? height : undefined, bitrate: bitrate > 0 ? bitrate : undefined,
        selectionSupported: selectionSupported,
        selectionReason: selectionSupported ? undefined : `Unknown native track type ${type}` };
    });
    entry.selectedNativeSubtitles = this.value.tracks.filter((track: VesperTrack) => track.kind === 'subtitle' && track.selected).map((track: VesperTrack) => track.id);
    if (entry.embeddedText) { this.value.tracks = this.value.tracks.filter((track: VesperTrack) => track.kind !== 'subtitle'); }
    this.value.selection.confirmedTrackIds = selected.filter((id: number) => !entry.embeddedText || !entry.selectedNativeSubtitles.includes(id));
    for (let index = 0; index < entry.sidecars.length; index++) {
      const id = -2 - index;
      const active = entry.selectedSidecar === index;
      const source = entry.sidecarSources[index];
      const failed = source !== undefined && this.value.subtitleState?.externalTracks.some((item) => item.id === source.id && item.state === 'failed');
      this.value.tracks.push({ id: id, kind: 'subtitle', nativeType: 2, mimeType: source?.mimeType ?? 'text/vtt', language: entry.sidecarLanguages[index],
        stableId: source?.id, label: source?.label, external: source !== undefined, isDefault: source?.isDefault, isForced: source?.isForced,
        selected: active, selectionSupported: !failed, selectionReason: failed ? 'External subtitle loading failed' : undefined });
      if (active) { this.value.selection.confirmedTrackIds.push(id); }
    }
    entry.subtitleCatalogReady = true; this.updateSubtitleState(entry);
  }
  private requireEntry(): PlaybackInstance {
    this.check(); const entry = this.current;
    if (entry === undefined || entry.player === undefined || entry.closed) { throw new VesperPlayerError('not_ready', 'No active player'); }
    return entry;
  }
  private advanceIntent(): void {
    if (this.disposed) { return; }
    this.value.intentRevision = ++this.intentRevision; this.publish('playbackIntent');
  }
  private command(work: (entry: PlaybackInstance, player: media.AVPlayer, scope: OperationScope) => Promise<void>,
    cancellation?: VesperCancellationToken, timeoutMs: number = 10000): Promise<void> {
    let entry: PlaybackInstance;
    let scope: OperationScope;
    try { entry = this.requireEntry(); scope = new OperationScope(timeoutMs, cancellation); } catch (error) { return Promise.reject(error); }
    if (entry.commands >= 16) { scope.close(); return Promise.reject(new VesperPlayerError('capacity', 'Controller command queue is full')); }
    this.commandScopes.add(scope); this.commandPending.set(scope, Promise.resolve()); entry.commands++;
    const physical = entry.tail.then(async (): Promise<void> => {
      try {
        scope.check(); if (!this.accepts(entry) || entry.player === undefined) { throw new VesperPlayerError('stale_epoch', 'Playback epoch is no longer current'); }
        await work(entry, entry.player, scope);
      } finally {
        // Cancellation ends the public wait, not a submitted native mutation.
        // Keep its serialization slot and bounded capacity until it settles.
        // Replacement/close retire this player and retain its physical ownership.
        const pending = this.commandPending.get(scope); if (pending !== undefined) { await pending; }
      }
    });
    entry.tail = physical.then(() => {}, () => {});
    physical.finally(() => { scope.close(); this.commandScopes.delete(scope); this.commandPending.delete(scope); entry.commands--; }).catch(() => {});
    return scope.wait(physical).catch((error: Error) => { throw nativeError(error, 'playback command'); });
  }
  play(): Promise<void> {
    if (this.disposed) { return Promise.reject(new VesperPlayerError('disposed', 'Controller was disposed')); }
    this.value.intentRevision = ++this.intentRevision; this.value.playWhenReady = true; this.publish('playbackIntent');
    return this.command(async (entry, player, scope): Promise<void> => {
    await this.owned(entry, player.play(), scope); await scope.until(() => player.state === 'playing');
    const deferred = entry.deferredAudio;
    if (deferred !== undefined) { entry.deferredAudio = undefined; await this.selectEntry(entry, player, scope, deferred); }
  }); }
  pause(): Promise<void> {
    if (this.disposed) { return Promise.reject(new VesperPlayerError('disposed', 'Controller was disposed')); }
    this.value.intentRevision = ++this.intentRevision; this.value.playWhenReady = false; this.publish('playbackIntent');
    return this.command(async (entry, player, scope): Promise<void> => {
    const requestedPause = player.state === 'playing';
    if (requestedPause) { await this.owned(entry, player.pause(), scope); }
    // Native state can precede the asynchronous callback. Resolve only once
    // subscribers can observe the pause, including an already-paused native player.
    if (requestedPause || player.state === 'paused') { await scope.until(() => player.state === 'paused' && this.value.state === 'paused'); }
  }); }
  seek(positionMs: number): Promise<void> { return this.command(async (entry, player, scope): Promise<void> => {
    await this.seekEntry(entry, player, positionMs, scope);
  }); }
  private async seekEntry(entry: PlaybackInstance, player: media.AVPlayer, positionMs: number, scope: OperationScope): Promise<void> {
    if (!Number.isSafeInteger(positionMs) || positionMs < 0) { throw new VesperPlayerError('invalid_argument', 'Seek position is invalid'); }
    if (this.snapshot().timeline !== 'vod') { throw new VesperPlayerError('unsupported', 'Live seeking requires a verified seekable range'); }
    const revision = entry.seekRevision;
    const target = Math.min(positionMs, player.duration); this.publish('seekRequested', undefined, target);
    player.seek(target, media.SeekMode.SEEK_CLOSEST);
    await scope.until(() => entry.seekRevision !== revision);
  }
  setPlaybackRate(rate: number): Promise<void> { return this.command(async (entry, player, scope): Promise<void> => { await this.rateEntry(entry, player, rate, scope); }); }
  private async rateEntry(entry: PlaybackInstance, player: media.AVPlayer, rate: number, scope: OperationScope): Promise<void> {
    if (!Number.isFinite(rate) || rate < 0.25 || rate > 4) { throw new VesperPlayerError('invalid_argument', 'Playback rate must be between 0.25 and 4'); }
    if (this.snapshot().timeline !== 'vod') { throw new VesperPlayerError('unsupported', 'Numeric playback rate is limited to VOD'); }
    const revision = entry.rateRevision; player.setPlaybackRate(rate);
    await scope.until(() => entry.rateRevision !== revision);
  }
  setVolume(volume: number): Promise<void> { return this.command(async (entry, player, scope): Promise<void> => {
    if (!Number.isFinite(volume) || volume < 0 || volume > 1) { throw new VesperPlayerError('invalid_argument', 'Volume must be between 0 and 1'); }
    this.value.volume = volume; this.applyVolume(entry, player); this.publish('volume');
  }); }
  setMuted(muted: boolean): Promise<void> { return this.command(async (entry, player, scope): Promise<void> => {
    this.value.muted = muted; this.applyVolume(entry, player); this.publish('volume');
  }); }
  /** Preferences run once at activation, or once now when a prepared catalog already exists. */
  setTrackPreferences(preferences: VesperTrackPreferences): Promise<VesperTrackPreferenceResult> {
    try { this.check(); this.preferences = validateTrackPreferences(preferences); }
    catch (error) { return Promise.reject(error); }
    this.advanceIntent();
    if (!['prepared', 'playing', 'paused'].includes(this.value.state) || this.current === undefined) {
      const result = resolveTrackPreferences([], this.preferences, this.value.playbackEpoch);
      result.audio = { status: 'deferred', reason: 'catalogNotReady' }; result.subtitle = { status: 'deferred', reason: 'catalogNotReady' };
      this.value.trackPreferences = result; this.publish('trackPreferences'); return Promise.resolve(result);
    }
    let result: VesperTrackPreferenceResult | undefined;
    return this.command(async (entry, player, scope): Promise<void> => { result = await this.applyPreferencesEntry(entry, player, scope); })
      .then((): VesperTrackPreferenceResult => result as VesperTrackPreferenceResult);
  }
  private async applyPreferencesEntry(entry: PlaybackInstance, player: media.AVPlayer, scope: OperationScope): Promise<VesperTrackPreferenceResult> {
    const result = resolveTrackPreferences(this.value.tracks, this.preferences, entry.epoch);
    for (const choice of [result.audio, result.subtitle]) {
      if (choice.selection === undefined) { continue; }
      try {
        await this.selectEntry(entry, player, scope, choice.selection); scope.check();
        choice.status = choice.selection.kind === 'audio' && entry.deferredAudio !== undefined ? 'deferred' : 'applied';
      }
      catch (error) {
        scope.check(); if (!this.accepts(entry)) { throw new VesperPlayerError('stale_epoch', 'Track preference epoch was replaced'); }
        choice.status = 'failed'; choice.reason = nativeError(error, 'track preferences').code;
      }
    }
    if ((this.preferences.subtitleMode ?? 'system') === 'system') {
      const automatic = resolveExternalSubtitleDefault(this.value.tracks);
      if (automatic.selection !== undefined) {
        try { await this.selectEntry(entry, player, scope, automatic.selection); scope.check(); automatic.status = 'applied'; }
        catch (error) { scope.check(); automatic.status = 'failed'; automatic.reason = nativeError(error, 'default external subtitle').code; }
        result.subtitle = automatic;
      }
    }
    scope.check();
    if (!this.accepts(entry)) { throw new VesperPlayerError('stale_epoch', 'Track preference epoch was replaced'); }
    this.value.trackPreferences = result; this.publish('trackPreferences');
    return JSON.parse(JSON.stringify(result)) as VesperTrackPreferenceResult;
  }
  selectTrack(selection: VesperTrackSelection, cancellation?: VesperCancellationToken, timeoutMs: number = 10000): Promise<void> {
    const copy: VesperTrackSelection = { kind: selection.kind, mode: selection.mode, trackId: selection.trackId, bitrate: selection.bitrate };
    this.advanceIntent();
    return this.command((entry, player, scope): Promise<void> => this.selectEntry(entry, player, scope, copy), cancellation, timeoutMs);
  }
  private async selectEntry(entry: PlaybackInstance, player: media.AVPlayer, scope: OperationScope, selection: VesperTrackSelection): Promise<void> {
    if (selection.kind === 'audio') { entry.deferredAudio = undefined; }
    const previous = selection.kind === 'video' ? this.value.trackSelections?.video : selection.kind === 'audio' ? this.value.trackSelections?.audio : this.value.trackSelections?.subtitle;
    this.value.selection = { confirmedTrackIds: this.value.selection.confirmedTrackIds.slice(),
      effective: previous?.effective,
      effectiveBitrate: this.value.trackSelections?.video?.effectiveBitrate ?? this.value.selection.effectiveBitrate };
    if (this.value.trackSelections === undefined) { this.value.trackSelections = {}; }
    if (selection.kind === 'video') { this.value.trackSelections.video = this.value.selection; }
    else if (selection.kind === 'audio') { this.value.trackSelections.audio = this.value.selection; }
    else if (selection.kind === 'subtitle') { this.value.trackSelections.subtitle = this.value.selection; }
    this.value.selection.requested = JSON.parse(JSON.stringify(selection)) as VesperTrackSelection;
    this.value.selection.outcome = 'pending'; this.value.selection.errorCode = undefined;
    this.updateSubtitleState(entry); this.publish('selectionRequested');
    try {
    if (!['audio', 'video', 'subtitle'].includes(selection.kind)) { throw new VesperPlayerError('unsupported', 'Unknown track kind'); }
    if (selection.mode === 'track') {
      const track = this.value.tracks.find((item: VesperTrack) => item.id === selection.trackId && item.kind === selection.kind);
      if (track === undefined) { throw new VesperPlayerError('invalid_argument', 'Track does not belong to the current epoch'); }
      if (track.selectionSupported === false) { throw new VesperPlayerError('unsupported', track.selectionReason ?? 'Track selection is unavailable'); }
      if (selection.kind === 'audio' && !track.selected && player.state === 'paused' && this.value.timeline === 'vod') {
        entry.deferredAudio = selection; this.publish('selectionDeferred'); return;
      }
      if (selection.kind === 'subtitle' && track.id <= -2) {
        // Embedded wvtt presentation uses the bounded text adapter. AVPlayer's
        // audio/video deselection API does not control this text route.
        if (!entry.embeddedText) {
          for (const id of entry.selectedNativeSubtitles) { await this.owned(entry, player.deselectTrack(id), scope); entry.confirmed.delete(id); }
        }
        entry.nativeTextEnabled = false; entry.selectedSidecar = -2 - track.id; this.updateSubtitle(entry);
      } else {
        if (selection.kind === 'subtitle') { entry.nativeTextEnabled = true; entry.selectedSidecar = -1; this.value.subtitle = ''; }
        if (selection.kind !== 'audio' || !track.selected) { await this.owned(entry, player.selectTrack(track.id), scope); }
      }
    } else if (selection.kind === 'subtitle' && selection.mode === 'off') {
      entry.nativeTextEnabled = false; entry.selectedSidecar = -1;
      for (const track of this.value.tracks) { if (track.kind === 'subtitle' && track.id >= 0 && track.selected) { await this.owned(entry, player.deselectTrack(track.id), scope); } }
      this.value.subtitle = '';
    } else if (selection.kind === 'video' && (selection.mode === 'auto' || selection.mode === 'bitrate')) {
      if (this.value.availableBitrates.length === 0) { throw new VesperPlayerError('unsupported', 'This stream has no native bitrate selection'); }
      const bitrate = selection.mode === 'auto' ? 0 : selection.bitrate;
      if (bitrate === undefined || !Number.isSafeInteger(bitrate) || bitrate < 0) { throw new VesperPlayerError('invalid_argument', 'Bitrate is invalid'); }
      const revision = entry.bitrateRevision; player.setBitrate(bitrate); await scope.until(() => entry.bitrateRevision !== revision);
    } else { throw new VesperPlayerError('unsupported', 'Track selection mode is unsupported'); }
    this.updateSubtitleTimer(entry);
    await scope.wait(this.refreshTracks(entry)); scope.check(); this.publish('selectionChanged');
    if (selection.mode === 'track' && selection.trackId !== undefined) {
      const selected = (): boolean => this.value.tracks.some((track: VesperTrack): boolean =>
        track.kind === selection.kind && track.id === selection.trackId && track.selected);
      if (!selected()) { await scope.until(selected); await this.refreshTracks(entry); }
    }
    scope.check(); if (!this.accepts(entry)) { throw new VesperPlayerError('stale_epoch', 'Track selection epoch was replaced'); }
    this.value.selection.outcome = 'confirmed'; this.value.selection.effective = this.value.selection.requested;
    this.updateSubtitleState(entry); this.publish('selectionConfirmed');
    } catch (error) {
      if (this.accepts(entry)) {
        this.value.selection.outcome = 'failed'; this.value.selection.errorCode = nativeError(error, 'track selection').code;
        this.updateSubtitleState(entry); this.publish('selectionFailed');
      }
      throw nativeError(error, 'track selection');
    }
  }
  private cancelPending(reason: VesperPlayerError): void {
    this.current?.scope.fail(reason);
    for (const scope of this.commandScopes) { scope.fail(reason); }
  }
  private retire(entry: PlaybackInstance): Promise<void> {
    if (entry.releaseWork !== undefined) { return entry.releaseWork; }
    entry.closed = true;
    entry.live?.cancel();
    if (entry.rtspHealthTimer !== undefined) { clearInterval(entry.rtspHealthTimer); entry.rtspHealthTimer = undefined; }
    entry.deferredAudio = undefined;
    if (entry.subtitleTimer !== undefined) { clearInterval(entry.subtitleTimer); entry.subtitleTimer = undefined; }
    this.retired.add(entry);
    const work = entry.pending.then(async (): Promise<void> => {
      await this.releaseBarrier;
      if (entry.player !== undefined) { await entry.player.release(); entry.player = undefined; }
      entry.mediaSource = undefined;
      entry.rtsp?.close();
      entry.rtsp = undefined;
      await entry.loader?.dispose();
      await entry.live?.dispose();
      if (entry.file !== undefined) { await fs.close(entry.file); entry.file = undefined; }
      while (entry.subtitles.length > 0) { const file = entry.subtitles[0]; await fs.close(file); entry.subtitles.shift(); }
      while (entry.subtitlePaths.length > 0) { const path = entry.subtitlePaths[0]; if (await fs.access(path)) { await fs.unlink(path); } entry.subtitlePaths.shift(); }
      entry.releaseAsset?.(); entry.releaseAsset = undefined;
      entry.lease.release(); VesperPlayerController.physicalPlayers--; this.value.releasedPlayers++;
    });
    entry.releaseWork = work; this.cleanup.add(work);
    work.then(() => { this.cleanup.delete(work); this.retired.delete(entry); }, (error: Error) => {
      // Retain failed native resources and their slot; a retry must never close
      // an fd still borrowed by a native player that failed to release.
      this.cleanup.delete(work); entry.releaseWork = undefined;
      this.value.error = nativeError(error, 'physical release'); this.publish('cleanupFailed', this.value.error);
    });
    return work;
  }
  async stop(): Promise<void> {
    this.check(); this.value.intentRevision = ++this.intentRevision;
    this.cancelPending(new VesperPlayerError('stopped', 'Playback was stopped'));
    const entry = this.current; this.current = undefined; this.value.playWhenReady = false;
    this.value.state = 'stopped'; this.value.subtitle = ''; this.publish('stopped');
    if (entry !== undefined) { const scope = new OperationScope(10000); try { await scope.wait(this.retire(entry)); } finally { scope.close(); } }
  }
  dispose(): Promise<void> {
    if (!this.disposed) {
      this.cancelPending(new VesperPlayerError('disposed', 'Controller was disposed'));
      this.disposed = true; this.value.disposed = true; this.value.playWhenReady = false;
      this.releaseBarrier = new Promise<void>((resolve) => { this.allowRelease = resolve; });
      this.value.state = 'releasing'; this.publish('disposed'); this.listeners.clear();
    }
    if (this.disposeWork === undefined) {
      const work = Promise.resolve().then(async (): Promise<void> => {
        for (const hook of Array.from(this.cleanupHooks)) { await hook(); this.cleanupHooks.delete(hook); }
        this.allowRelease();
        const current = this.current; this.current = undefined;
        if (current !== undefined) { this.retire(current); }
        for (const entry of this.retired) { this.retire(entry); }
        await Promise.all(Array.from(this.cleanup));
        this.surface = ''; this.owner = undefined; this.value.state = 'released';
      });
      this.disposeWork = work; work.catch(() => { this.disposeWork = undefined; });
    }
    const scope = new OperationScope(10000);
    return scope.wait(this.disposeWork).finally(() => { scope.close(); });
  }
}
