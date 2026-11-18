import { common } from '@kit.AbilityKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { util } from '@kit.ArkTS';
import { cryptoFramework } from '@kit.CryptoArchitectureKit';
import { ResourceInfo } from './StartupCache';
import { directory, atomicText, boundedText, regularFile, fileHash, writeAll } from './FileStore';
import { VesperPlayerError } from './Types';

export interface VesperDiskCacheConfiguration { partition: string; maximumBytes?: number; lifetimeMs?: number; }
interface DiskEntry {
  key: string; resource: string; offset: number; size: number; checksum: string;
  totalBytes: number; contentType: string; etag?: string; expires: number; used: number;
}
interface DiskIndex { schemaVersion: number; entries: DiskEntry[]; }

/** Optional disk bytes have independent budgets and opaque registration/access partitions. */
export class VesperDiskCache {
  private static owners: Set<string> = new Set();
  private readonly root: string;
  private readonly maximum: number;
  private readonly lifetime: number;
  private readonly namespace: string = util.generateRandomUUID();
  private scopeId: number = 0;
  private entries: DiskEntry[] = [];
  private tail: Promise<void> = Promise.resolve();
  private pending: number = 0;
  private closing: boolean = false;
  private closeWork?: Promise<void>;
  private hits: number = 0;
  private corruptions: number = 0;
  private orphan?: string;
  private orphanBytes: number = 0;
  private constructor(context: common.Context, configuration: VesperDiskCacheConfiguration) {
    if (!/^[A-Za-z0-9_-]{1,64}$/.test(configuration.partition)) { throw new VesperPlayerError('invalid_argument', 'Disk cache partition must be an opaque identifier'); }
    this.root = `${context.cacheDir}/vesper-cache-${configuration.partition}`;
    this.maximum = configuration.maximumBytes ?? 32 * 1024 * 1024; this.lifetime = configuration.lifetimeMs ?? 300000;
    if (!Number.isSafeInteger(this.maximum) || this.maximum < 65536 || this.maximum > 256 * 1024 * 1024
      || !Number.isInteger(this.lifetime) || this.lifetime < 1 || this.lifetime > 86400000) {
      throw new VesperPlayerError('invalid_argument', 'Disk cache budget or lifetime is invalid');
    }
  }
  static async open(context: common.Context, configuration: VesperDiskCacheConfiguration): Promise<VesperDiskCache> {
    const cache = new VesperDiskCache(context, configuration);
    if (VesperDiskCache.owners.has(cache.root) || VesperDiskCache.owners.size >= 8) { throw new VesperPlayerError('in_use', 'Disk cache directory already has an owner'); }
    VesperDiskCache.owners.add(cache.root);
    try {
      await directory(cache.root);
      const index = `${cache.root}/index.json`;
      if (await fs.access(index)) {
        try {
          const stored = JSON.parse(await boundedText(index, 128 * 1024)) as DiskIndex;
          if (stored.schemaVersion !== 1 || !Array.isArray(stored.entries) || stored.entries.length > 128) { throw new Error('Invalid index'); }
          for (const entry of stored.entries) {
            if (!/^[a-f0-9]{64}$/.test(entry.key) || !/^[a-f0-9]{64}$/.test(entry.resource) || !/^[a-f0-9]{64}$/.test(entry.checksum)
              || !Number.isSafeInteger(entry.offset) || entry.offset < 0 || !Number.isSafeInteger(entry.size) || entry.size < 1 || entry.size > 1024 * 1024
              || !Number.isSafeInteger(entry.totalBytes) || entry.offset + entry.size > entry.totalBytes
              || !Number.isFinite(entry.expires) || entry.expires > Date.now() + 86400000 || entry.contentType.length > 256
              || (entry.etag?.length ?? 0) > 512 || cache.entries.some((item: DiskEntry) => item.key === entry.key)) { throw new Error('Invalid entry'); }
            cache.entries.push(entry);
          }
        } catch (_) { cache.entries = []; cache.corruptions++; }
      }
      const names = await fs.listFile(cache.root, { recursion: false, listNum: 1025 });
      if (names.length > 1024) { throw new VesperPlayerError('capacity', 'Disk cache directory exceeds its recovery scan limit'); }
      for (const name of names) {
        if (name === 'index.json') { continue; }
        if (name === 'index.json.tmp' || /^[a-f0-9]{64}\.(?:bin|part)$/.test(name)) {
          if (!cache.entries.some((entry: DiskEntry) => name === `${entry.key}.bin`)) { await fs.unlink(`${cache.root}/${name}`); }
        } else { throw new VesperPlayerError('invalid_file', 'Disk cache directory contains an unowned file'); }
      }
      await cache.prune(0); await cache.persist(); return cache;
    } catch (error) { VesperDiskCache.owners.delete(cache.root); throw error as Error; }
  }
  /** @internal A new registration never aliases another access context, even after process restart. */
  newScope(): string {
    if (this.closing || this.scopeId >= Number.MAX_SAFE_INTEGER) { throw new VesperPlayerError('closed', 'Disk cache is closing'); }
    return `${this.namespace}:${++this.scopeId}`;
  }
  private async hash(text: string): Promise<string> {
    const hash = cryptoFramework.createMd('SHA256'); await hash.update({ data: new util.TextEncoder().encodeInto(text) });
    return Array.from((await hash.digest()).data).map((value: number) => value.toString(16).padStart(2, '0')).join('');
  }
  private io<T>(action: () => Promise<T>): Promise<T> {
    if (this.closing || this.pending >= 8) { return Promise.reject(new VesperPlayerError(this.closing ? 'closed' : 'capacity', 'Disk cache I/O is unavailable')); }
    this.pending++; const work = this.tail.then(action);
    this.tail = work.then(() => { this.pending--; }, () => { this.pending--; }); return work;
  }
  private path(entry: DiskEntry): string { return `${this.root}/${entry.key}.bin`; }
  private async erase(entry: DiskEntry): Promise<void> {
    if (await fs.access(this.path(entry))) { await fs.unlink(this.path(entry)); }
    this.entries = this.entries.filter((item: DiskEntry) => item !== entry);
  }
  private async prune(incoming: number): Promise<void> {
    for (const entry of this.entries.slice()) { if (entry.expires <= Date.now()) { await this.erase(entry); } }
    this.entries.sort((a: DiskEntry, b: DiskEntry) => a.used - b.used);
    while (this.entries.length > 0 && (this.entries.length >= 128 || this.entries.reduce((sum: number, entry: DiskEntry) => sum + entry.size, 0) + incoming > this.maximum)) {
      await this.erase(this.entries[0]);
    }
  }
  private persist(): Promise<void> { const index: DiskIndex = { schemaVersion: 1, entries: this.entries }; return atomicText(`${this.root}/index.json`, JSON.stringify(index)); }
  metadata(scope: string, uri: string): Promise<ResourceInfo | undefined> {
    return this.io(async (): Promise<ResourceInfo | undefined> => {
      await this.prune(0); const resource = await this.hash(`${scope}\n${uri}`);
      const entry = this.entries.find((item: DiskEntry) => item.resource === resource);
      return entry === undefined ? undefined : { totalBytes: entry.totalBytes, contentType: entry.contentType, finalUrl: uri, etag: entry.etag };
    });
  }
  read(scope: string, uri: string, offset: number, length: number): Promise<ArrayBuffer | undefined> {
    return this.io(async (): Promise<ArrayBuffer | undefined> => {
      await this.prune(0); const resource = await this.hash(`${scope}\n${uri}`);
      const entry = this.entries.find((item: DiskEntry) => item.resource === resource && item.offset <= offset && item.offset + item.size > offset);
      if (entry === undefined) { return undefined; }
      // Serial I/O is the read lease: eviction and clear cannot run until close.
      try {
        if (await regularFile(this.path(entry), entry.size) !== entry.size || await fileHash(this.path(entry), entry.size) !== entry.checksum) { throw new Error('Corrupt cache bytes'); }
        const file = await fs.open(this.path(entry), fs.OpenMode.READ_ONLY | fs.OpenMode.NOFOLLOW);
        try {
          const buffer = new ArrayBuffer(Math.min(length, entry.size - (offset - entry.offset)));
          const count = await fs.read(file.fd, buffer, { offset: offset - entry.offset, length: buffer.byteLength });
          if (count !== buffer.byteLength) { throw new Error('Short cache read'); }
          entry.used = Date.now(); this.hits += count; return buffer;
        } finally { await fs.close(file); }
      } catch (_) { this.corruptions++; await this.erase(entry); await this.persist(); return undefined; }
    });
  }
  put(scope: string, uri: string, offset: number, bytes: ArrayBuffer, info: ResourceInfo): Promise<void> {
    // Redirect base URLs remain in memory; signed locations are never persisted.
    if (info.finalUrl !== uri || /mpegurl|dash\+xml/i.test(info.contentType) || /\.(?:m3u8|mpd)(?:[?#]|$)/i.test(uri)
      || bytes.byteLength < 1 || bytes.byteLength > 1024 * 1024 || bytes.byteLength > this.maximum) { return Promise.resolve(); }
    return this.io(async (): Promise<void> => {
      if (this.orphan !== undefined) { throw new VesperPlayerError('storage_cleanup_required', 'A failed cache write retains accounted bytes until cleanup'); }
      const resource = await this.hash(`${scope}\n${uri}`); const key = await this.hash(`${resource}:${offset}`);
      for (const entry of this.entries.slice()) {
        if (entry.resource === resource && (entry.totalBytes !== info.totalBytes || entry.etag !== info.etag)) { await this.erase(entry); }
        else if (entry.key === key) { await this.erase(entry); }
      }
      await this.prune(bytes.byteLength);
      const part = `${this.root}/${key}.part`; let checksum: string;
      try {
        const output = await fs.open(part, fs.OpenMode.CREATE | fs.OpenMode.TRUNC | fs.OpenMode.WRITE_ONLY | fs.OpenMode.NOFOLLOW);
        try { await writeAll(output.fd, bytes); await fs.fsync(output.fd); } finally { await fs.close(output); }
        checksum = await fileHash(part, bytes.byteLength); await fs.rename(part, `${this.root}/${key}.bin`);
      } catch (error) {
        try { if (await fs.access(part)) { await fs.unlink(part); } }
        catch (_) { this.orphan = part; this.orphanBytes = bytes.byteLength; }
        throw error as Error;
      }
      this.entries.push({ key: key, resource: resource, offset: offset, size: bytes.byteLength, checksum: checksum,
        totalBytes: info.totalBytes, contentType: info.contentType, etag: info.etag, expires: Date.now() + this.lifetime, used: Date.now() });
      await this.persist();
    });
  }
  private async cleanOrphan(): Promise<void> {
    if (this.orphan !== undefined) { if (await fs.access(this.orphan)) { await fs.unlink(this.orphan); } this.orphan = undefined; this.orphanBytes = 0; }
  }
  clear(): Promise<void> { return this.io(async (): Promise<void> => { await this.cleanOrphan(); for (const entry of this.entries.slice()) { await this.erase(entry); } await this.persist(); }); }
  inventory(): string { return JSON.stringify({ entries: this.entries.length, residentBytes: this.entries.reduce((sum: number, entry: DiskEntry) => sum + entry.size, 0),
    maximumBytes: this.maximum, orphanBytes: this.orphanBytes, pending: this.pending, hitBytes: this.hits, corruptions: this.corruptions }); }
  close(): Promise<void> {
    if (this.closeWork !== undefined) { return this.closeWork; } this.closing = true;
    const work = this.tail.then(async (): Promise<void> => { await this.cleanOrphan(); await this.persist(); VesperDiskCache.owners.delete(this.root); });
    this.closeWork = work; work.catch(() => { this.closeWork = undefined; }); return work;
  }
}
