import { common } from '@kit.AbilityKit';
import { fileIo as fs, fileUri, picker } from '@kit.CoreFileKit';
import { util } from '@kit.ArkTS';
import { AssetPins, assetFile, directory, atomicText, boundedText, fileHash, regularFile } from './FileStore';
import { VesperPlayerError } from './Types';
import { VesperCancellationToken, OperationScope } from './Operation';
import { copyVerifiedFile, writeStoredZip, exportOutputBytes, EXPORT_MAXIMUM_BYTES, EXPORT_DISK_BUDGET } from './ExportStream';

export interface VesperExportFile { path: string; size: number; checksum?: string; }
export interface VesperExportOptions { cancellation?: VesperCancellationToken; timeoutMs?: number; }
interface ExportRetention { schemaVersion: number; retainUntilEpochMs: number; reservedBytes?: number; }

/** An immutable temporary output owns its own lifetime after the download manager closes. */
export class VesperExportArtifact {
  private static active: Set<VesperExportArtifact> = new Set();
  private static recovering: Map<string, Promise<void>> = new Map();
  private static preparations: number = 0;
  private static admissions: Map<string, Promise<void>> = new Map();
  readonly path: string;
  readonly fileName: string;
  readonly mimeType: string;
  private readonly root: string;
  private readers: number = 0;
  private retainUntil: number = 0;
  private released: boolean = false;
  private releaseWork?: Promise<void>;
  private preparing: boolean = true;
  private output?: fs.File;
  private checksum: string = '';
  private readonly reservedBytes: number;
  private constructor(root: string, fileName: string, mimeType: string, reservedBytes: number) {
    this.root = root; this.fileName = fileName; this.path = `${root}/${fileName}`; this.mimeType = mimeType;
    this.reservedBytes = reservedBytes;
  }
  /** @internal Only verified, finite asset members are admitted. No remux is performed. */
  static async create(root: string, exportRoot: string, format: string, entry: string, files: VesperExportFile[], options: VesperExportOptions = {}): Promise<VesperExportArtifact> {
    if (!['progressive', 'hls', 'dash'].includes(format) || format === 'progressive' && (files.length !== 1 || files[0].path !== entry)) {
      throw new VesperPlayerError('unsupported', 'Export requires an original finite file or offline HLS/DASH bundle');
    }
    const size = exportOutputBytes(format, entry, files);
    if (VesperExportArtifact.active.size >= 8 || VesperExportArtifact.preparations >= 2 || files.length < 1 || files.length > 512 ||
      files.some((file: VesperExportFile): boolean => !Number.isSafeInteger(file.size) || file.size < 1) || !Number.isSafeInteger(size) || size > EXPORT_MAXIMUM_BYTES) {
      throw new VesperPlayerError('capacity', 'Export allows two preparations, eight temporary assets and 256 MiB per original or ZIP');
    }
    const scope = new OperationScope(options.timeoutMs ?? 60000, options.cancellation);
    const parent = `${exportRoot}/exports`;
    const destination = `${parent}/${Date.now()}-${util.generateRandomUUID()}`;
    const artifact = new VesperExportArtifact(destination, format === 'progressive' ? entry.split('/').pop() ?? 'media.bin' : 'vesper-offline.zip',
      format === 'progressive' ? 'application/octet-stream' : 'application/zip', size);
    VesperExportArtifact.active.add(artifact); VesperExportArtifact.preparations++;
    const physical = VesperExportArtifact.prepare(artifact, root, exportRoot, format, entry, files, scope);
    return scope.wait(physical);
  }
  private static async prepare(artifact: VesperExportArtifact, root: string, exportRoot: string, format: string, entry: string,
    files: VesperExportFile[], scope: OperationScope): Promise<VesperExportArtifact> {
    let releasePin: () => void = (): void => {};
    try {
      scope.check();
      releasePin = AssetPins.acquire(`${root}/${entry}`);
      await VesperExportArtifact.reserve(artifact, exportRoot, scope);
      artifact.output = await fs.open(`${artifact.path}.part`, fs.OpenMode.CREATE | fs.OpenMode.TRUNC | fs.OpenMode.WRITE_ONLY | fs.OpenMode.NOFOLLOW);
      try {
        if (format === 'progressive') {
          const file = files[0];
          if (file.checksum === undefined) { throw new VesperPlayerError('offline_incomplete', 'Export requires a verified asset'); }
          await copyVerifiedFile(await assetFile(root, entry), artifact.output.fd, file.size, file.checksum, scope);
        } else { await writeStoredZip(root, artifact.output.fd, format, entry, files, scope); }
        scope.check(); await fs.fsync(artifact.output.fd);
      } finally { await fs.close(artifact.output); artifact.output = undefined; }
      scope.check(); await fs.rename(`${artifact.path}.part`, artifact.path);
      if (await regularFile(artifact.path, EXPORT_MAXIMUM_BYTES) !== artifact.reservedBytes) { throw new VesperPlayerError('file_changed', 'Export output did not match its reserved size'); }
      artifact.checksum = await fileHash(artifact.path, EXPORT_MAXIMUM_BYTES); scope.check(); artifact.preparing = false;
      return artifact;
    } catch (error) {
      artifact.preparing = false;
      try { await artifact.release(); } catch (_) { /* Failed cleanup remains reachable in the bounded owner set. */ }
      throw error as Error;
    } finally { releasePin(); VesperExportArtifact.preparations--; scope.close(); }
  }
  private static reserve(artifact: VesperExportArtifact, exportRoot: string, scope: OperationScope): Promise<void> {
    const previous = VesperExportArtifact.admissions.get(exportRoot) ?? Promise.resolve();
    const work = previous.then(async (): Promise<void> => {
      const parent = `${exportRoot}/exports`; scope.check(); await directory(parent); await VesperExportArtifact.recover(exportRoot);
      const names = await fs.listFile(parent, { recursion: false, listNum: 9 });
      if (names.length >= 8) { throw new VesperPlayerError('export_cleanup_required', 'Release earlier temporary exports before creating more'); }
      let reserved = artifact.reservedBytes;
      for (const name of names) {
        scope.check();
        if (!/^\d{1,16}-[a-f0-9-]{36}$/.test(name)) { throw new VesperPlayerError('export_cleanup_required', 'Unknown temporary export inventory requires cleanup'); }
        let bytes = EXPORT_MAXIMUM_BYTES;
        try {
          const retained = JSON.parse(await boundedText(`${parent}/${name}/retention.json`, 1024)) as ExportRetention;
          if (retained.schemaVersion === 1 && Number.isSafeInteger(retained.reservedBytes) && (retained.reservedBytes ?? 0) >= 0 && (retained.reservedBytes ?? 0) <= EXPORT_MAXIMUM_BYTES) {
            bytes = retained.reservedBytes ?? EXPORT_MAXIMUM_BYTES;
          }
        } catch (_) { /* Unknown crash-era reservations conservatively retain the per-export maximum. */ }
        let physicalBytes = 0;
        const members = await fs.listFile(`${parent}/${name}`, { recursion: false, listNum: 5 });
        if (members.length > 4) { throw new VesperPlayerError('export_cleanup_required', 'Temporary export members exceed their inventory bound'); }
        for (const member of members) {
          if (member === 'retention.json' || member === 'retention.json.tmp') { continue; }
          physicalBytes += await regularFile(`${parent}/${name}/${member}`, EXPORT_MAXIMUM_BYTES);
        }
        reserved += Math.max(bytes, physicalBytes);
      }
      if (reserved > EXPORT_DISK_BUDGET) { throw new VesperPlayerError('export_cleanup_required', 'Temporary exports exceed the 512 MiB directory budget'); }
      scope.check(); await directory(artifact.root); await artifact.persistRetention();
    });
    const tail = work.then((): void => {}, (): void => {}); VesperExportArtifact.admissions.set(exportRoot, tail);
    tail.then((): void => { if (VesperExportArtifact.admissions.get(exportRoot) === tail) { VesperExportArtifact.admissions.delete(exportRoot); } });
    return work;
  }
  /** Retries temporary cleanup after failed preparation; active share leases remain protected. */
  static async cleanupTemporary(context?: common.Context): Promise<void> {
    for (const artifact of Array.from(VesperExportArtifact.active)) { await artifact.release(); }
    if (context !== undefined) { await VesperExportArtifact.recover(`${context.filesDir}/vesper-downloads`); }
  }
  /** @internal Recover only expired SDK-owned directories; current owners remain protected. */
  static recover(exportRoot: string): Promise<void> {
    const existing = VesperExportArtifact.recovering.get(exportRoot); if (existing !== undefined) { return existing; }
    if (VesperExportArtifact.recovering.size >= 8) { return Promise.reject(new VesperPlayerError('capacity', 'Export recovery capacity reached')); }
    const work = Promise.resolve().then(async (): Promise<void> => {
      const parent = `${exportRoot}/exports`; if (!await fs.access(parent)) { return; } await directory(parent);
      const names = await fs.listFile(parent, { recursion: false, listNum: 9 });
      if (names.length > 8) { throw new VesperPlayerError('export_cleanup_required', 'Temporary export inventory exceeds its owned capacity'); }
      for (const name of names) {
        if (!/^\d{1,16}-[a-f0-9-]{36}$/.test(name)) { continue; }
        const path = `${parent}/${name}`;
        if (Array.from(VesperExportArtifact.active).some((artifact: VesperExportArtifact) => artifact.root === path)) { continue; }
        const created = Number(name.substring(0, name.indexOf('-')));
        let until = created + 600000;
        const stat = await fs.lstat(path); if (!stat.isDirectory() || stat.isSymbolicLink()) { continue; }
        if (await fs.access(`${path}/retention.json`)) {
          try {
            const record = JSON.parse(await boundedText(`${path}/retention.json`, 1024)) as ExportRetention;
            if (record.schemaVersion !== 1 || !Number.isSafeInteger(record.retainUntilEpochMs) || record.retainUntilEpochMs < created) { continue; }
            until = Math.max(until, record.retainUntilEpochMs);
          } catch (_) { /* A malformed record still receives the ten-minute crash retention. */ }
        }
        if (Number.isSafeInteger(until) && Date.now() >= until) { await fs.rmdir(path); }
      }
    });
    VesperExportArtifact.recovering.set(exportRoot, work);
    work.finally(() => { VesperExportArtifact.recovering.delete(exportRoot); }).catch(() => {}); return work;
  }
  private persistRetention(): Promise<void> {
    return atomicText(`${this.root}/retention.json`, JSON.stringify({ schemaVersion: 1, retainUntilEpochMs: Math.max(this.retainUntil, Date.now() + 600000), reservedBytes: this.reservedBytes }));
  }
  private check(): void { if (this.released || this.releaseWork !== undefined) { throw new VesperPlayerError('closed', 'Export artifact was released'); } }
  /** The host owns cancellation at the system picker; an empty selection means user cancellation. */
  async save(context: common.UIAbilityContext, options: VesperExportOptions = {}): Promise<boolean> {
    this.check(); this.readers++;
    try {
      const selected = await new picker.DocumentViewPicker(context).save({ newFileNames: [this.fileName] });
      if (selected.length === 0) { return false; }
      if (selected.length !== 1) { throw new VesperPlayerError('invalid_destination', 'Save requires one document picker output'); }
      // The provider URI may name a file that does not exist until open creates it.
      // Only open accepts that URI; subsequent operations use the owned descriptor.
      const output = await fs.open(selected[0], fs.OpenMode.CREATE | fs.OpenMode.READ_WRITE);
      try {
        if ((await fs.stat(output.fd)).size !== 0) {
          throw new VesperPlayerError('destination_not_empty', 'Save requires an empty document picker output');
        }
        try {
          const scope = new OperationScope(options.timeoutMs ?? 60000, options.cancellation);
          // Keep this reader and its descriptors until the physical copy exits.
          // Cancellation is observed between bounded chunks, including after a late I/O completion.
          try { await copyVerifiedFile(this.path, output.fd, this.reservedBytes, this.checksum, scope); }
          finally { scope.close(); }
          if ((await fs.stat(output.fd)).size !== (await fs.stat(this.path)).size) {
            throw new VesperPlayerError('save_incomplete', 'The document provider did not preserve the complete export');
          }
          await fs.fsync(output.fd);
        } catch (error) {
          // URI deletion is not a FileIO contract. Restore only the empty output
          // admitted above; never truncate a pre-existing nonempty user file.
          try { await fs.truncate(output.fd, 0); await fs.fsync(output.fd); }
          catch (_) { throw new VesperPlayerError('save_cleanup_required', 'The document provider could not clear its failed output'); }
          throw error as Error;
        }
      } finally { await fs.close(output); }
      return true;
    } finally { this.readers--; }
  }
  /** System sharing is loaded on demand so core-only OpenHarmony consumers can load without Share Kit. */
  async share(context: common.UIAbilityContext): Promise<void> {
    this.check();
    if (!canIUse('SystemCapability.Collaboration.SystemShare')) { throw new VesperPlayerError('unsupported', 'System Share Kit is unavailable on this device'); }
    this.readers++;
    try {
      // The local adapter preserves the SDK's static kit alias transformation
      // while keeping Share Kit out of the core module's eager load path.
      const kit = await import('./SystemShare').catch(() => {
        throw new VesperPlayerError('unsupported', 'System Share Kit is unavailable on this device');
      });
      const share = kit.createShare(fileUri.getUriFromPath(this.path), this.fileName);
      // Closing the chooser does not prove the recipient finished reading.
      // Preserve the URI for ten minutes after handing it to the system.
      this.retainUntil = Math.max(this.retainUntil, Date.now() + 600000);
      await this.persistRetention();
      await share.show(context, {});
    } finally { this.readers--; }
  }
  release(): Promise<void> {
    if (this.released) { return Promise.resolve(); }
    if (this.releaseWork !== undefined) { return this.releaseWork; }
    if (this.preparing || this.readers > 0 || Date.now() < this.retainUntil) { return Promise.reject(new VesperPlayerError('in_use', 'A preparation or save/share reader retains this temporary artifact')); }
    const work = Promise.resolve().then(async (): Promise<void> => {
      if (this.output !== undefined) { await fs.close(this.output); this.output = undefined; }
      if (await fs.access(this.root)) { await fs.rmdir(this.root); }
      this.released = true; VesperExportArtifact.active.delete(this);
    });
    this.releaseWork = work; work.catch(() => { this.releaseWork = undefined; }); return work;
  }
}
