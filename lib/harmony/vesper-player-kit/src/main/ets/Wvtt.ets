import { util } from '@kit.ArkTS';
import { VesperPlayerError } from './Types';
import { SubtitleCue } from './Subtitles';

interface Box { kind: string; start: number; payload: number; end: number; }
/** A bounded text-only ISO/IEC 14496-30 adapter; audio/video remain native. */
export function parseWvtt(buffer: ArrayBuffer): SubtitleCue[] {
  if (buffer.byteLength > 256 * 1024) { throw new VesperPlayerError('capacity', 'WebVTT track exceeds 256 KiB'); }
  const view = new DataView(buffer); const bytes = new Uint8Array(buffer);
  const invalid = (): VesperPlayerError => new VesperPlayerError('unsupported', 'WebVTT requires one finite unencrypted fragmented wvtt track');
  const u32 = (offset: number): number => { if (offset < 0 || offset + 4 > buffer.byteLength) { throw invalid(); } return view.getUint32(offset); };
  const u64 = (offset: number): number => {
    const value = u32(offset) * 4294967296 + u32(offset + 4); if (!Number.isSafeInteger(value)) { throw invalid(); } return value;
  };
  const boxes = (start: number, end: number): Box[] => {
    const result: Box[] = [];
    while (start < end) {
      if (start + 8 > end || result.length >= 4096) { throw invalid(); }
      const size = u32(start); if (size < 8 || size > end - start) { throw invalid(); }
      const kind = String.fromCharCode(bytes[start + 4], bytes[start + 5], bytes[start + 6], bytes[start + 7]);
      result.push({ kind: kind, start: start, payload: start + 8, end: start + size }); start += size;
    }
    return result;
  };
  const child = (parent: Box, kind: string, skip: number = 0): Box => {
    const result = boxes(parent.payload + skip, parent.end).filter((box: Box) => box.kind === kind);
    if (result.length !== 1) { throw invalid(); } return result[0];
  };
  const top = boxes(0, buffer.byteLength); const moovs = top.filter((box: Box) => box.kind === 'moov');
  if (moovs.length !== 1) { throw invalid(); }
  const moov = moovs[0]; const trak = child(moov, 'trak'); const mdia = child(trak, 'mdia');
  if (boxes(trak.payload, trak.end).some((box: Box) => box.kind === 'edts')) { throw invalid(); }
  const mdhd = child(mdia, 'mdhd'); const tkhd = child(trak, 'tkhd');
  const timescale = u32(mdhd.payload + (bytes[mdhd.payload] === 1 ? 20 : 12));
  const trackId = u32(tkhd.payload + (bytes[tkhd.payload] === 1 ? 20 : 12));
  const stsd = child(child(child(mdia, 'minf'), 'stbl'), 'stsd');
  if (timescale < 1 || u32(stsd.payload + 4) !== 1 || child(stsd, 'wvtt', 8).end !== stsd.end) { throw invalid(); }
  const trex = child(child(moov, 'mvex'), 'trex');
  if (u32(trex.payload + 4) !== trackId) { throw invalid(); }
  const cues: SubtitleCue[] = []; let samples = 0; let fragments = 0;
  for (const moof of top.filter((box: Box) => box.kind === 'moof')) {
    if (++fragments > 512) { throw invalid(); }
    const traf = child(moof, 'traf'); const tfhd = child(traf, 'tfhd'); const tfdt = child(traf, 'tfdt');
    if (u32(tfhd.payload + 4) !== trackId) { throw invalid(); }
    const flags = u32(tfhd.payload) & 0xffffff; let cursor = tfhd.payload + 8;
    let base = moof.start;
    if (flags & 1) { base = u64(cursor); cursor += 8; }
    if (flags & 2) { cursor += 4; }
    let duration = u32(trex.payload + 12); let size = u32(trex.payload + 16);
    if (flags & 8) { duration = u32(cursor); cursor += 4; }
    if (flags & 16) { size = u32(cursor); cursor += 4; }
    if (flags & 32) { cursor += 4; }
    if (cursor !== tfhd.end) { throw invalid(); }
    let time = bytes[tfdt.payload] === 1 ? u64(tfdt.payload + 4) : u32(tfdt.payload + 4);
    let dataOffset = moof.end + 8;
    const runs = boxes(traf.payload, traf.end).filter((box: Box) => box.kind === 'trun');
    if (runs.length < 1) { throw invalid(); }
    for (const trun of runs) {
      const runFlags = u32(trun.payload) & 0xffffff; const count = u32(trun.payload + 4); cursor = trun.payload + 8;
      if (runFlags & 1) { dataOffset = base + view.getInt32(cursor); cursor += 4; }
      if (runFlags & 4) { cursor += 4; }
      if (samples + count > 4096) { throw invalid(); } samples += count;
      for (let index = 0; index < count; index++) {
        const sampleDuration = runFlags & 0x100 ? u32(cursor) : duration; if (runFlags & 0x100) { cursor += 4; }
        const sampleSize = runFlags & 0x200 ? u32(cursor) : size; if (runFlags & 0x200) { cursor += 4; }
        if (runFlags & 0x400) { cursor += 4; }
        const composition = runFlags & 0x800 ? bytes[trun.payload] === 1 ? view.getInt32(cursor) : u32(cursor) : 0;
        if (runFlags & 0x800) { cursor += 4; }
        if (sampleDuration < 1 || sampleSize < 8 || !top.some((box: Box) => box.kind === 'mdat' && dataOffset >= box.payload && dataOffset + sampleSize <= box.end)) { throw invalid(); }
        const startMs = (time + composition) * 1000 / timescale; const endMs = startMs + sampleDuration * 1000 / timescale;
        if (!Number.isSafeInteger(time + sampleDuration) || startMs < 0 || !Number.isFinite(endMs)) { throw invalid(); }
        for (const cue of boxes(dataOffset, dataOffset + sampleSize)) {
          if (cue.kind === 'vtte') { continue; } if (cue.kind !== 'vttc') { throw invalid(); }
          const payload = child(cue, 'payl'); if (payload.end - payload.payload > 8192 || cues.length >= 4096) { throw invalid(); }
          const text = util.TextDecoder.create('utf-8', { fatal: true }).decodeWithStream(bytes.slice(payload.payload, payload.end));
          cues.push({ startMs: startMs, endMs: endMs, text: text });
        }
        time += sampleDuration; dataOffset += sampleSize;
      }
      if (cursor !== trun.end) { throw invalid(); }
    }
  }
  if (fragments < 1) { throw invalid(); } return cues.sort((a: SubtitleCue, b: SubtitleCue) => a.startMs - b.startMs);
}
