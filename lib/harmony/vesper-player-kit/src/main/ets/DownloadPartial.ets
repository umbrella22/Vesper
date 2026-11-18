import { fileIo as fs } from '@kit.CoreFileKit';
import { cryptoFramework } from '@kit.CryptoArchitectureKit';
import { util } from '@kit.ArkTS';
import { regularFile } from './FileStore';
import { VesperPlayerError } from './Types';

/** Digests describe committed 64 KiB chunks; URLs and access headers never enter this record. */
export interface DownloadPartial {
  partialBytes?: number; partialHashes?: string[]; resourceFingerprint?: string; resourceLength?: number; etag?: string;
}
export async function downloadDigest(bytes: Uint8Array): Promise<string> {
  const hash = cryptoFramework.createMd('SHA256'); await hash.update({ data: bytes }); const digest = await hash.digest();
  return Array.from(digest.data).map((value: number): string => value.toString(16).padStart(2, '0')).join('');
}
export function downloadResourceFingerprint(uri: string): Promise<string> { return downloadDigest(new util.TextEncoder().encodeInto(uri)); }
export function clearDownloadPartial(file: DownloadPartial): void {
  file.partialBytes = undefined; file.partialHashes = undefined; file.etag = undefined;
  file.resourceFingerprint = undefined; file.resourceLength = undefined;
}
export function validDownloadPartial(file: DownloadPartial, size: number): boolean {
  const count = file.partialBytes ?? 0; const hashes = file.partialHashes ?? [];
  return Number.isSafeInteger(count) && count >= 0 && count <= size && hashes.length === Math.ceil(count / 65536) && hashes.length <= 16384 &&
    hashes.every((hash: string): boolean => /^[a-f0-9]{64}$/.test(hash)) &&
    (count === 0 || typeof file.etag === 'string' && file.etag.length <= 1024 && !file.etag.startsWith('W/') &&
      /^[a-f0-9]{64}$/.test(file.resourceFingerprint ?? '') && Number.isSafeInteger(file.resourceLength) && (file.resourceLength ?? 0) >= size);
}
/** A crash may leave extra bytes after the last checkpoint. Only the hashed prefix may be reused. */
export async function verifiedDownloadPrefix(path: string, file: DownloadPartial, size: number, guard: () => void): Promise<number> {
  if (!validDownloadPartial(file, size) || (file.partialBytes ?? 0) === 0 || !await fs.access(path)) { return 0; }
  const count = file.partialBytes ?? 0;
  if (await regularFile(path, size) < count) { return 0; }
  const input = await fs.open(path, fs.OpenMode.READ_ONLY | fs.OpenMode.NOFOLLOW);
  try {
    let verified = 0;
    for (const hash of file.partialHashes ?? []) {
      guard(); const bytes = new ArrayBuffer(Math.min(65536, count - verified)); let offset = 0;
      while (offset < bytes.byteLength) {
        guard(); const chunk = new ArrayBuffer(bytes.byteLength - offset); const read = await fs.read(input.fd, chunk);
        if (!Number.isInteger(read) || read < 1 || read > chunk.byteLength) { return 0; }
        new Uint8Array(bytes).set(new Uint8Array(chunk.slice(0, read)), offset); offset += read;
      }
      if (await downloadDigest(new Uint8Array(bytes)) !== hash) { return 0; }
      verified += bytes.byteLength;
    }
    guard(); return verified;
  } finally { await fs.close(input); }
}
export async function writeDownloadChunk(fd: number, bytes: ArrayBuffer, offset: number): Promise<void> {
  let written = 0;
  while (written < bytes.byteLength) {
    const count = await fs.write(fd, bytes.slice(written), { offset: offset + written });
    if (!Number.isSafeInteger(count) || count < 1 || count > bytes.byteLength - written) { throw new VesperPlayerError('file_write', 'Partial download write made no progress'); }
    written += count;
  }
}
