import { fileIo as fs } from '@kit.CoreFileKit';
import { cryptoFramework } from '@kit.CryptoArchitectureKit';
import { util } from '@kit.ArkTS';
import { VesperPlayerError } from './Types';
import { OperationScope } from './Operation';
import { assetFile, regularFile, writeAll } from './FileStore';

export interface ExportMember { path: string; size: number; checksum?: string; }
interface ZipEntry { name: Uint8Array; size: number; crc: number; offset: number; }
export const EXPORT_MAXIMUM_BYTES: number = 256 * 1024 * 1024;
export const EXPORT_DISK_BUDGET: number = 512 * 1024 * 1024;
const CHUNK_BYTES: number = 65536;

function crcUpdate(crc: number, bytes: Uint8Array): number {
  let value = crc;
  for (let index = 0; index < bytes.length; index++) {
    value ^= bytes[index];
    for (let bit = 0; bit < 8; bit++) { value = (value >>> 1) ^ ((value & 1) !== 0 ? 0xEDB88320 : 0); }
  }
  return value >>> 0;
}
function header(signature: number, length: number): DataView {
  const value = new DataView(new ArrayBuffer(length)); value.setUint32(0, signature, true); return value;
}
function encoded(value: string): Uint8Array { return new util.TextEncoder().encodeInto(value); }
export function exportManifest(format: string, entry: string, files: ExportMember[]): string {
  return JSON.stringify({ schemaVersion: 1, format: format, entry: entry, files: files });
}
/** ZIP uses stored entries: its complete disk requirement is known before allocating an output. */
export function exportOutputBytes(format: string, entry: string, files: ExportMember[]): number {
  if (format === 'progressive') { return files.reduce((sum: number, file: ExportMember): number => sum + file.size, 0); }
  let size = 22;
  for (const file of files) { size += file.size + 92 + encoded(file.path).byteLength * 2; }
  size += encoded(exportManifest(format, entry, files)).byteLength + 92 + encoded('vesper-offline.json').byteLength * 2;
  return size;
}

/** The physical operation retains descriptors until each pending read/write has completed. */
export async function copyVerifiedFile(source: string, output: number, size: number, checksum: string, scope: OperationScope): Promise<number> {
  scope.check();
  if (await regularFile(source, size) !== size) { throw new VesperPlayerError('offline_incomplete', 'Export source size changed'); }
  const input = await fs.open(source, fs.OpenMode.READ_ONLY | fs.OpenMode.NOFOLLOW);
  const hash = cryptoFramework.createMd('SHA256'); let crc = 0xFFFFFFFF;
  try {
    if ((await fs.stat(input.fd)).size !== size) { throw new VesperPlayerError('offline_incomplete', 'Export source size changed while opening'); }
    let remaining = size;
    while (remaining > 0) {
      scope.check(); const data = new ArrayBuffer(Math.min(CHUNK_BYTES, remaining));
      const count = await fs.read(input.fd, data); scope.check();
      if (!Number.isInteger(count) || count < 1 || count > data.byteLength) { throw new VesperPlayerError('file_read', 'Export source ended during streaming'); }
      const chunk = new Uint8Array(data.slice(0, count)); await hash.update({ data: chunk }); scope.check();
      crc = crcUpdate(crc, chunk); await writeAll(output, chunk.buffer as ArrayBuffer); remaining -= count;
    }
    const digest = await hash.digest(); scope.check();
    const actual = Array.from(digest.data).map((value: number): string => value.toString(16).padStart(2, '0')).join('');
    if (actual !== checksum || (await fs.stat(input.fd)).size !== size) { throw new VesperPlayerError('offline_incomplete', 'Export source checksum changed'); }
    return (crc ^ 0xFFFFFFFF) >>> 0;
  } finally { await fs.close(input); }
}

/** A bounded ZIP32 store writer avoids native compression's uninterruptible staging copy. */
export async function writeStoredZip(root: string, output: number, format: string, entry: string, files: ExportMember[], scope: OperationScope): Promise<void> {
  const entries: ZipEntry[] = []; let offset = 0;
  for (let index = 0; index <= files.length; index++) {
    scope.check(); const file = files[index];
    const name = encoded(file === undefined ? 'vesper-offline.json' : file.path);
    const inline = file === undefined ? encoded(exportManifest(format, entry, files)) : undefined;
    const size = file?.size ?? inline?.byteLength ?? 0;
    const start = offset; const local = header(0x04034B50, 30);
    local.setUint16(4, 20, true); local.setUint16(6, 0x808, true); local.setUint16(12, 33, true); local.setUint16(26, name.byteLength, true);
    await writeAll(output, local.buffer as ArrayBuffer); await writeAll(output, name.buffer as ArrayBuffer);
    let crc: number;
    if (file !== undefined) {
      if (file.checksum === undefined) { throw new VesperPlayerError('offline_incomplete', 'ZIP requires verified asset members'); }
      crc = await copyVerifiedFile(await assetFile(root, file.path), output, size, file.checksum, scope);
    } else {
      const bytes = inline ?? new Uint8Array(); crc = (crcUpdate(0xFFFFFFFF, bytes) ^ 0xFFFFFFFF) >>> 0;
      await writeAll(output, bytes.buffer as ArrayBuffer);
    }
    const descriptor = header(0x08074B50, 16);
    descriptor.setUint32(4, crc, true); descriptor.setUint32(8, size, true); descriptor.setUint32(12, size, true);
    await writeAll(output, descriptor.buffer as ArrayBuffer);
    entries.push({ name: name, size: size, crc: crc, offset: start }); offset += 30 + name.byteLength + size + 16;
  }
  const centralStart = offset;
  for (const item of entries) {
    scope.check(); const central = header(0x02014B50, 46);
    central.setUint16(4, 20, true); central.setUint16(6, 20, true); central.setUint16(8, 0x808, true); central.setUint16(14, 33, true);
    central.setUint32(16, item.crc, true); central.setUint32(20, item.size, true); central.setUint32(24, item.size, true);
    central.setUint16(28, item.name.byteLength, true); central.setUint32(42, item.offset, true);
    await writeAll(output, central.buffer as ArrayBuffer); await writeAll(output, item.name.buffer as ArrayBuffer); offset += 46 + item.name.byteLength;
  }
  const end = header(0x06054B50, 22); end.setUint16(8, entries.length, true); end.setUint16(10, entries.length, true);
  end.setUint32(12, offset - centralStart, true); end.setUint32(16, centralStart, true);
  await writeAll(output, end.buffer as ArrayBuffer); scope.check();
}
