import { fileIo as fs } from '@kit.CoreFileKit';
import { cryptoFramework } from '@kit.CryptoArchitectureKit';
import { util } from '@kit.ArkTS';
import { VesperPlayerError } from './Types';

/** Sandbox-relative names are validated again when loading persisted metadata. */
export function relativeFile(value: string): string {
  if (value.length < 1 || value.length > 512 || value.includes('\\') || value.includes('%')
    || value.split('/').some((part: string) => !/^[A-Za-z0-9_.-]+$/.test(part) || part === '.' || part === '..')) {
    throw new VesperPlayerError('invalid_path', 'Asset path is not a safe relative file');
  }
  return value;
}

export async function directory(path: string): Promise<void> {
  try { await fs.mkdir(path, true); } catch (_) {
    const stat = await fs.lstat(path);
    if (!stat.isDirectory() || stat.isSymbolicLink()) { throw new VesperPlayerError('invalid_path', 'Asset directory is not a directory'); }
  }
  const stat = await fs.lstat(path);
  if (!stat.isDirectory() || stat.isSymbolicLink()) { throw new VesperPlayerError('invalid_path', 'Asset directory is not a directory'); }
}

export async function regularFile(path: string, maximum: number): Promise<number> {
  const stat = await fs.lstat(path);
  if (!stat.isFile() || stat.isSymbolicLink() || !Number.isSafeInteger(stat.size) || stat.size < 0 || stat.size > maximum) {
    throw new VesperPlayerError('invalid_file', 'Asset file type or size is invalid');
  }
  return stat.size;
}

export async function assetFile(root: string, relative: string, createParents: boolean = false): Promise<string> {
  const parts = relativeFile(relative).split('/');
  let current = root;
  let stat = await fs.lstat(current);
  if (!stat.isDirectory() || stat.isSymbolicLink()) { throw new VesperPlayerError('invalid_path', 'Asset root is invalid'); }
  for (let index = 0; index < parts.length - 1; index++) {
    current = `${current}/${parts[index]}`;
    if (createParents) { await directory(current); }
    stat = await fs.lstat(current);
    if (!stat.isDirectory() || stat.isSymbolicLink()) { throw new VesperPlayerError('invalid_path', 'Asset parent is invalid'); }
  }
  return `${root}/${relative}`;
}

export async function writeAll(fd: number, data: ArrayBuffer): Promise<void> {
  let offset = 0;
  while (offset < data.byteLength) {
    const written = await fs.write(fd, data.slice(offset));
    if (!Number.isSafeInteger(written) || written < 1 || written > data.byteLength - offset) {
      throw new VesperPlayerError('file_write', 'Asset write made no valid progress');
    }
    offset += written;
  }
}

export async function atomicText(path: string, text: string): Promise<void> {
  const file = await fs.open(`${path}.tmp`, fs.OpenMode.CREATE | fs.OpenMode.TRUNC | fs.OpenMode.WRITE_ONLY | fs.OpenMode.NOFOLLOW);
  try { await writeAll(file.fd, new util.TextEncoder().encodeInto(text).buffer as ArrayBuffer); await fs.fsync(file.fd); }
  finally { await fs.close(file); }
  await fs.rename(`${path}.tmp`, path);
}

export async function boundedText(path: string, maximum: number): Promise<string> {
  await regularFile(path, maximum); return fs.readText(path, { length: maximum });
}

export async function fileHash(path: string, maximum: number): Promise<string> {
  const size = await regularFile(path, maximum);
  const hash = cryptoFramework.createMd('SHA256');
  const file = await fs.open(path, fs.OpenMode.READ_ONLY | fs.OpenMode.NOFOLLOW);
  try {
    let remaining = size;
    while (remaining > 0) {
      const data = new ArrayBuffer(Math.min(65536, remaining));
      const read = await fs.read(file.fd, data);
      if (read < 1 || read > data.byteLength) { throw new VesperPlayerError('file_read', 'Asset changed or ended during verification'); }
      await hash.update({ data: new Uint8Array(data.slice(0, read)) }); remaining -= read;
    }
    const digest = await hash.digest();
    return Array.from(digest.data).map((value: number) => value.toString(16).padStart(2, '0')).join('');
  } finally { await fs.close(file); }
}

/** Active playback pins outlive the manager that supplied an offline source. */
export class AssetPins {
  private static active: Map<string, number> = new Map();
  private static deleting: Set<string> = new Set();
  static acquire(path: string): () => void {
    for (const root of AssetPins.deleting) {
      if (path.startsWith(`${root}/`)) { throw new VesperPlayerError('in_use', 'Offline asset is being removed'); }
    }
    if (AssetPins.active.size >= 64 && !AssetPins.active.has(path)) { throw new VesperPlayerError('capacity', 'Local asset lease capacity reached'); }
    AssetPins.active.set(path, (AssetPins.active.get(path) ?? 0) + 1);
    let released = false;
    return (): void => {
      if (released) { return; } released = true;
      const count = AssetPins.active.get(path) ?? 0;
      if (count <= 1) { AssetPins.active.delete(path); } else { AssetPins.active.set(path, count - 1); }
    };
  }
  static contains(root: string): boolean {
    for (const path of AssetPins.active.keys()) { if (path.startsWith(`${root}/`)) { return true; } }
    return false;
  }
  static beginDelete(root: string): () => void {
    if (AssetPins.deleting.size >= 32 || AssetPins.deleting.has(root) || AssetPins.contains(root)) {
      throw new VesperPlayerError('in_use', 'An active playback lease retains this offline asset');
    }
    AssetPins.deleting.add(root);
    return (): void => { AssetPins.deleting.delete(root); };
  }
}
