import { NativeSession } from './NativeSession';
import { VesperPlayerSource, VesperPlayerError } from './Types';
import { CacheBudget, StartupCache } from './StartupCache';
import { VesperDiskCache } from './DiskCache';
import { Preloader, VesperPreloadTask, VesperPreloadOptions, VesperPreloadConfiguration } from './Preload';

interface Registration { sourceId: string; sourceRevision: string; }
interface Acquisition { sourceId: string; leaseId: string; descriptor: VesperPlayerSource; expiresAtEpochMs?: number; }
interface SourceInventory { closed: boolean; invalidated: boolean; registrations: number; leases: number; capacity: number; }

export interface VesperSourceSessionConfiguration {
  maxRegisteredSources?: number; memoryBudgetBytes?: number; preload?: VesperPreloadConfiguration; diskCache?: VesperDiskCache;
}

/** Immutable registration. Identity is independent of URI and playback epoch. */
export class VesperSourceHandle {
  readonly sourceId: string;
  readonly sessionId: string;
  private owner: VesperSourceSession;
  constructor(owner: VesperSourceSession, registration: Registration) {
    this.owner = owner; this.sourceId = registration.sourceId; this.sessionId = owner.sessionId;
  }
  close(): void { this.owner.closeSource(this.sourceId, false); }
  invalidate(): void { this.owner.closeSource(this.sourceId, true); }
  preload(options: VesperPreloadOptions = {}): VesperPreloadTask { return this.owner.preload(this, options); }
  /** @internal Acquires a separate lifetime for native playback or a sequence. */
  acquireLease(): VesperSourceLease { return this.owner.acquire(this.sourceId); }
}

/** @internal Retains a descriptor until physical native resource cleanup finishes. */
export class VesperSourceLease {
  readonly sourceId: string;
  readonly sessionId: string;
  readonly descriptor: VesperPlayerSource;
  readonly expiresAtEpochMs?: number;
  private owner: VesperSourceSession;
  private identity: string;
  private released: boolean = false;
  constructor(owner: VesperSourceSession, value: Acquisition) {
    this.owner = owner; this.identity = value.leaseId; this.descriptor = value.descriptor;
    this.expiresAtEpochMs = value.expiresAtEpochMs ?? undefined;
    this.sourceId = value.sourceId; this.sessionId = owner.sessionId;
  }
  validateActivation(): void {
    if (this.released) { throw new VesperPlayerError('stale_lease', 'Source lease was released'); }
    this.owner.validate(this.identity);
  }
  release(): void {
    if (this.released) { return; }
    this.owner.releaseLease(this.identity); this.released = true;
  }
  /** @internal Retains an accepted lease even after ordinary registration close. */
  retain(): VesperSourceLease {
    this.validateActivation(); return this.owner.retain(this.identity);
  }
  /** @internal */
  startupCache(): StartupCache { return this.owner.startupCache(this.sourceId); }
  /** @internal */
  preload(options: VesperPreloadOptions): VesperPreloadTask { return this.owner.preloadRetained(this.retain(), options); }
}

/** Registers sources without allocating a player, Surface, decoder or audio session. */
export class VesperSourceSession {
  readonly sessionId: string;
  private bridge: NativeSession;
  private closed: boolean = false;
  private leases: Map<string, string> = new Map();
  private closedSources: Set<string> = new Set();
  private bridgeReleased: boolean = false;
  private invalidated: boolean = false;
  private capacity: number;
  private caches: Map<string, StartupCache> = new Map();
  private budget: CacheBudget;
  private preloader: Preloader;
  private disk?: VesperDiskCache;
  constructor(configuration: VesperSourceSessionConfiguration = {}) {
    const maximum = configuration.maxRegisteredSources ?? 128;
    this.capacity = maximum;
    this.budget = new CacheBudget(configuration.memoryBudgetBytes ?? 8 * 1024 * 1024);
    this.disk = configuration.diskCache;
    this.preloader = new Preloader(configuration.preload ?? {});
    this.bridge = new NativeSession(JSON.stringify({ kind: 'sources', maxSources: maximum }));
    this.sessionId = this.bridge.identity;
  }
  register(source: VesperPlayerSource, expiresAtEpochMs?: number): VesperSourceHandle {
    if (this.closed) { throw new VesperPlayerError('closed', 'Source session is closed'); }
    const cache = new StartupCache(this.budget, this.disk);
    const value = this.bridge.invoke<Registration>(JSON.stringify({ op: 'register', source: source, expiresAtEpochMs: expiresAtEpochMs }));
    this.caches.set(value.sourceId, cache);
    return new VesperSourceHandle(this, value);
  }
  preload(handle: VesperSourceHandle, options: VesperPreloadOptions = {}): VesperPreloadTask {
    if (this.closed || handle.sessionId !== this.sessionId) { throw new VesperPlayerError('closed', 'Source is not owned by this open session'); }
    const lease = handle.acquireLease();
    try { return this.preloader.submit(lease, options); }
    catch (error) { lease.release(); throw error as Error; }
  }
  preloadInventory(): string { return this.preloader.inventory(); }
  /** @internal */
  preloadRetained(lease: VesperSourceLease, options: VesperPreloadOptions): VesperPreloadTask {
    try { return this.preloader.submit(lease, options); }
    catch (error) { lease.release(); throw error as Error; }
  }
  close(): void {
    if (this.bridgeReleased) { return; }
    if (!this.closed) { this.bridge.invoke<Object>('{"op":"close"}'); this.closed = true; }
    this.preloader.close();
    for (const id of this.caches.keys()) { this.closedSources.add(id); }
    this.clearCaches();
    this.releaseIfUnused();
  }
  invalidate(): void {
    if (this.bridgeReleased) { return; }
    this.bridge.invoke<Object>('{"op":"invalidate"}');
    this.closed = true; this.invalidated = true;
    this.preloader.close();
    for (const id of this.caches.keys()) { this.closedSources.add(id); }
    this.clearCaches();
    this.releaseIfUnused();
  }
  snapshot(): SourceInventory {
    if (this.bridgeReleased) { return { closed: true, invalidated: this.invalidated, registrations: 0, leases: 0, capacity: this.capacity }; }
    return this.bridge.invoke<SourceInventory>('{"op":"snapshot"}');
  }
  /** @internal */
  acquire(sourceId: string): VesperSourceLease {
    if (this.closed) { throw new VesperPlayerError('closed', 'Source session is closed'); }
    const result = this.bridge.invoke<Acquisition>(JSON.stringify({ op: 'acquire', sourceId: sourceId }));
    this.leases.set(result.leaseId, result.sourceId);
    return new VesperSourceLease(this, result);
  }
  /** @internal */
  retain(leaseId: string): VesperSourceLease {
    const value = this.bridge.invoke<Acquisition>(JSON.stringify({ op: 'retainLease', leaseId: leaseId }));
    this.leases.set(value.leaseId, value.sourceId); return new VesperSourceLease(this, value);
  }
  /** @internal */
  startupCache(sourceId: string): StartupCache {
    const cache = this.caches.get(sourceId);
    if (cache === undefined) { throw new VesperPlayerError('stale_source', 'Source cache scope was released'); }
    return cache;
  }
  /** @internal */
  closeSource(sourceId: string, invalidate: boolean): void {
    if (this.bridgeReleased) { return; }
    this.bridge.invoke<Object>(JSON.stringify({ op: invalidate ? 'invalidateSource' : 'closeSource', sourceId: sourceId }));
    this.caches.get(sourceId)?.clear();
    this.preloader.cancelSource(sourceId);
    this.closedSources.add(sourceId); this.collectCaches();
  }
  /** @internal */
  validate(leaseId: string): void { this.bridge.invoke<Object>(JSON.stringify({ op: 'validateLease', leaseId: leaseId })); }
  /** @internal */
  releaseLease(leaseId: string): void {
    if (this.leases.has(leaseId)) {
      this.bridge.invoke<Object>(JSON.stringify({ op: 'releaseLease', leaseId: leaseId }));
      this.leases.delete(leaseId);
      this.collectCaches();
    }
    this.releaseIfUnused();
  }
  private releaseIfUnused(): void {
    this.collectCaches();
    if (this.closed && this.leases.size === 0 && !this.bridgeReleased) {
      this.bridge.close(); this.bridgeReleased = true; this.caches.clear();
    }
  }
  private collectCaches(): void {
    for (const sourceId of this.closedSources) {
      let retained = false;
      for (const value of this.leases.values()) { if (value === sourceId) { retained = true; break; } }
      if (!retained) { this.caches.get(sourceId)?.clear(); this.caches.delete(sourceId); this.closedSources.delete(sourceId); }
    }
  }
  private clearCaches(): void { for (const cache of this.caches.values()) { cache.clear(); } }
}
