import { monotonicMs } from './Operation';
import { VesperPlayerError } from './Types';
import { VesperDiskCache } from './DiskCache';

export interface ResourceInfo { totalBytes: number; contentType: string; finalUrl: string; etag?: string; }
export interface WarmResource { uri: string; offset: number; bytes: ArrayBuffer; info: ResourceInfo; }
interface CacheEntry { owner: StartupCache; resource: WarmResource; expires: number; used: number; }

/** A session budget is independent of controller disk-cache policy. */
export class CacheBudget {
  readonly maximum: number;
  resident: number = 0;
  reserved: number = 0;
  constructor(maximum: number) {
    if (!Number.isSafeInteger(maximum) || maximum < 0 || maximum > 16 * 1024 * 1024) {
      throw new VesperPlayerError('invalid_argument', 'Session memory budget must be between 0 and 16 MiB');
    }
    this.maximum = maximum;
  }
}

/** One accepted registration owns its own cache scope; credentials are never keys supplied by a host. */
export class StartupCache {
  private static entries: CacheEntry[] = [];
  private static resident: number = 0;
  private static reserved: number = 0;
  private budget: CacheBudget;
  private generation: number = 0;
  private dashStartup: boolean = false;
  private playbackObserved: boolean = false;
  private disk?: VesperDiskCache;
  private diskScope: string;
  constructor(budget: CacheBudget, disk?: VesperDiskCache) { this.budget = budget; this.disk = disk; this.diskScope = disk?.newScope() ?? ''; }
  diskMetadata(uri: string): Promise<ResourceInfo | undefined> { return this.disk?.metadata(`${this.diskScope}:${this.generation}`, uri) ?? Promise.resolve(undefined); }
  diskRead(uri: string, offset: number, length: number): Promise<ArrayBuffer | undefined> { return this.disk?.read(`${this.diskScope}:${this.generation}`, uri, offset, length) ?? Promise.resolve(undefined); }
  diskWrite(uri: string, offset: number, bytes: ArrayBuffer, info: ResourceInfo): Promise<void> { return this.disk?.put(`${this.diskScope}:${this.generation}`, uri, offset, bytes, info) ?? Promise.resolve(); }
  generationId(): number { return this.generation; }
  maximumBytes(): number { return this.budget.maximum; }
  reserve(bytes: number): boolean {
    StartupCache.prune();
    while (this.budget.resident + this.budget.reserved + bytes > this.budget.maximum
      || StartupCache.resident + StartupCache.reserved + bytes > 32 * 1024 * 1024) {
      const sessionFull = this.budget.resident + this.budget.reserved + bytes > this.budget.maximum;
      const candidates = StartupCache.entries.filter((entry: CacheEntry) => !sessionFull || entry.owner.budget === this.budget);
      candidates.sort((a: CacheEntry, b: CacheEntry) => a.used - b.used);
      if (candidates.length === 0) { return false; }
      StartupCache.remove(candidates[0]);
    }
    this.budget.reserved += bytes; StartupCache.reserved += bytes; return true;
  }
  releaseReservation(bytes: number): void { this.budget.reserved -= bytes; StartupCache.reserved -= bytes; }
  setStartupProfile(dash: boolean): void { this.dashStartup = dash; }
  confirmPlayback(): void { if (this.dashStartup) { this.playbackObserved = true; } }
  reuseLevel(): string { return this.playbackObserved ? 'playbackReusable' : 'downloadOnly'; }
  clear(): void {
    this.generation++;
    this.dashStartup = false; this.playbackObserved = false;
    for (const entry of StartupCache.entries.slice()) { if (entry.owner === this) { StartupCache.remove(entry); } }
  }
  private static remove(entry: CacheEntry): void {
    const index = StartupCache.entries.indexOf(entry);
    if (index < 0) { return; }
    StartupCache.entries.splice(index, 1);
    StartupCache.resident -= entry.resource.bytes.byteLength;
    entry.owner.budget.resident -= entry.resource.bytes.byteLength;
  }
  private static prune(): void {
    const now = monotonicMs();
    for (const entry of StartupCache.entries.slice()) { if (entry.expires <= now) { StartupCache.remove(entry); } }
  }
  commit(resources: WarmResource[], generation: number, lifetimeMs: number): void {
    if (generation !== this.generation) { throw new VesperPlayerError('cancelled', 'Cache generation was invalidated'); }
    const size = resources.reduce((total: number, item: WarmResource) => total + item.bytes.byteLength, 0);
    if (resources.length > 64 || size > this.budget.maximum || !Number.isFinite(lifetimeMs) || lifetimeMs <= 0) {
      throw new VesperPlayerError('capacity', 'Startup resources exceed the cache budget or lifetime');
    }
    StartupCache.prune();
    this.clear();
    while (StartupCache.entries.length + resources.length > 64 || StartupCache.resident + StartupCache.reserved + size > 32 * 1024 * 1024
      || this.budget.resident + this.budget.reserved + size > this.budget.maximum) {
      const sessionFull = this.budget.resident + this.budget.reserved + size > this.budget.maximum;
      const candidates = StartupCache.entries.filter((item: CacheEntry) => !sessionFull || item.owner.budget === this.budget);
      candidates.sort((a: CacheEntry, b: CacheEntry) => a.used - b.used);
      if (candidates.length === 0) { throw new VesperPlayerError('capacity', 'Startup cache cannot admit this resource set'); }
      StartupCache.remove(candidates[0]);
    }
    const now = monotonicMs();
    const expires = now + Math.min(lifetimeMs, 30000);
    for (const resource of resources) {
      const entry: CacheEntry = { owner: this, resource: resource, expires: expires, used: now };
      StartupCache.entries.push(entry);
      StartupCache.resident += resource.bytes.byteLength;
      this.budget.resident += resource.bytes.byteLength;
    }
  }
  metadata(uri: string): ResourceInfo | undefined {
    StartupCache.prune();
    const entry = StartupCache.entries.find((item: CacheEntry) => item.owner === this && item.resource.uri === uri);
    return entry?.resource.info;
  }
  read(uri: string, offset: number, maximum: number): ArrayBuffer | undefined {
    StartupCache.prune();
    const entry = StartupCache.entries.find((item: CacheEntry) => item.owner === this && item.resource.uri === uri
      && item.resource.offset <= offset && item.resource.offset + item.resource.bytes.byteLength > offset);
    if (entry === undefined) { return undefined; }
    entry.used = monotonicMs();
    const start = offset - entry.resource.offset;
    return entry.resource.bytes.slice(start, Math.min(start + maximum, entry.resource.bytes.byteLength));
  }
  inventory(): string {
    StartupCache.prune();
    return JSON.stringify({ generation: this.generation, sessionResidentBytes: this.budget.resident,
      sessionReservedBytes: this.budget.reserved, sessionBudgetBytes: this.budget.maximum,
      sharedResidentBytes: StartupCache.resident, sharedReservedBytes: StartupCache.reserved, sharedEntries: StartupCache.entries.length });
  }
}
