'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');
const { createLoader } = require('./support/load.cjs');
const descriptors = new Map(); let maximumRead = 0; let cancelOnRead;
const fileIo = {
  OpenMode: { READ_ONLY: 0, NOFOLLOW: 131072 }, lstat: file => fs.lstat(file),
  async open(file) { const handle = await fs.open(file, 'r'); descriptors.set(handle.fd, handle); return { fd: handle.fd }; },
  async close(file) { await descriptors.get(file.fd).close(); descriptors.delete(file.fd); },
  stat: fd => descriptors.get(fd).stat(),
  async read(fd, buffer) { maximumRead = Math.max(maximumRead, buffer.byteLength); cancelOnRead?.(); return (await descriptors.get(fd).read(new Uint8Array(buffer))).bytesRead; },
  async write(fd, buffer) { return (await descriptors.get(fd).write(new Uint8Array(buffer).subarray(0, 10007))).bytesWritten; },
};
const load = createLoader(name => {
  if (name === '@kit.CoreFileKit') return { fileIo };
  if (name === '@kit.ArkTS') return { util: { TextEncoder: class { encodeInto(value) { return new Uint8Array(Buffer.from(value)); } } } };
  if (name === '@kit.BasicServicesKit') return { systemDateTime: { TimeType: { STARTUP: 0 }, getUptime: () => performance.now() } };
  if (name === '@kit.CryptoArchitectureKit') return { cryptoFramework: { createMd() { const hash = crypto.createHash('sha256'); return {
    async update(value) { hash.update(value.data); }, async digest() { return { data: new Uint8Array(hash.digest()) }; } }; } } };
  throw new Error(name);
});
const kit = path.resolve(__dirname, '../vesper-player-kit/src/main/ets');
const { copyVerifiedFile, writeStoredZip, exportOutputBytes } = load(path.join(kit, 'ExportStream.ets'));
const { OperationScope, VesperCancellationToken } = load(path.join(kit, 'Operation.ets'));
async function output(file, work) { const handle = await fs.open(file, 'w'); const fd = handle.fd; descriptors.set(fd, handle); try { return await work(fd); } finally { await handle.close(); descriptors.delete(fd); } }
async function main() {
  const root = await fs.mkdtemp(path.resolve(__dirname, '../../../target/harmony-export-stream-'));
  const bytes = Buffer.alloc(33 * 1024 * 1024 + 31, 42); await fs.writeFile(path.join(root, 'movie.mp4'), bytes);
  const checksum = crypto.createHash('sha256').update(bytes).digest('hex');
  const scope = new OperationScope(60000);
  try {
    await output(path.join(root, 'original.mp4'), fd => copyVerifiedFile(path.join(root, 'movie.mp4'), fd, bytes.length, checksum, scope));
    assert.equal(crypto.createHash('sha256').update(await fs.readFile(path.join(root, 'original.mp4'))).digest('hex'), checksum);
    const files = [{ path: 'movie.mp4', size: bytes.length, checksum }];
    await output(path.join(root, 'offline.zip'), fd => writeStoredZip(root, fd, 'dash', 'movie.mp4', files, scope));
    assert.equal((await fs.stat(path.join(root, 'offline.zip'))).size, exportOutputBytes('dash', 'movie.mp4', files));
    const checked = execFileSync('python3', ['-c', 'import zipfile,sys,hashlib,json\nz=zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None; assert hashlib.sha256(z.read("movie.mp4")).hexdigest()==sys.argv[2]; assert json.loads(z.read("vesper-offline.json"))["entry"]=="movie.mp4"; print("verified")', path.join(root, 'offline.zip'), checksum], { encoding: 'utf8' });
    assert.match(checked, /verified/); assert.ok(maximumRead <= 65536, 'large originals and ZIP members use bounded read buffers');
    await assert.rejects(output(path.join(root, 'corrupt.mp4'), fd => copyVerifiedFile(path.join(root, 'movie.mp4'), fd, bytes.length, '0'.repeat(64), scope)), error => error.code === 'offline_incomplete');
    const token = new VesperCancellationToken(); const cancelled = new OperationScope(60000, token); cancelOnRead = () => token.cancel();
    try { await assert.rejects(output(path.join(root, 'cancelled.mp4'), fd => copyVerifiedFile(path.join(root, 'movie.mp4'), fd, bytes.length, checksum, cancelled)), error => error.code === 'cancelled'); }
    finally { cancelled.close(); cancelOnRead = undefined; }
    assert.equal(descriptors.size, 0); console.log(JSON.stringify({ status: 'PASS', suite: 'streamed-original-and-zip', maximumRead }));
  } finally { scope.close(); await fs.rm(root, { recursive: true }); }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
