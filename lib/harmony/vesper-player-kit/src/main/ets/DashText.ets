import { util } from '@kit.ArkTS';
import { NativeSession } from './NativeSession';
import { ResourceProvider, ResourceSnapshot } from './ResourceLoader';
import { OperationScope, VesperCancellationToken } from './Operation';
import { VesperPlayerError } from './Types';
import { SubtitleCue } from './Subtitles';
import { parseWvtt } from './Wvtt';
import { ByteRange } from './Transport';

interface SegmentBase { initialization: ByteRange; indexRange: ByteRange; }
interface Representation { baseURL: string; codecs: string; segmentBase?: SegmentBase | null; }
interface Adaptation { kind: string; language?: string; representations: Representation[]; }
interface Period { adaptationSets: Adaptation[]; }
interface Manifest { type: string; periods: Period[]; }
export interface DashTextTrack { cues: SubtitleCue[]; language: string; }

/** Reads only bounded text renditions; the original MPD and AVPlayer route are preserved. */
export async function loadDashText(provider: ResourceProvider, uri: string, scope: OperationScope, initial?: ResourceSnapshot): Promise<DashTextTrack[]> {
  const token = new VesperCancellationToken(); const remove = scope.onFailure(() => { token.cancel(); });
  const bridge = new NativeSession('{"kind":"dash"}');
  let manifestUri = uri;
  const read = async (resource: string): Promise<ArrayBuffer> => {
    if (resource === uri && initial !== undefined) { manifestUri = initial.info.finalUrl; return initial.bytes; }
    if (provider.snapshot !== undefined) {
      const snapshot = await provider.snapshot(resource, 256 * 1024, token); scope.check();
      if (resource === uri) { manifestUri = snapshot.info.finalUrl; }
      return snapshot.bytes;
    }
    scope.check(); const info = await provider.metadata(resource, token); scope.check();
    if (resource === uri) { manifestUri = info.finalUrl; }
    if (info.totalBytes < 1 || info.totalBytes > 256 * 1024) { throw new VesperPlayerError('unsupported', 'DASH text/manifest exceeds 256 KiB'); }
    const output = new Uint8Array(info.totalBytes); let offset = 0;
    while (offset < output.byteLength) {
      const bytes = await provider.read(resource, { start: offset, end: Math.min(output.byteLength, offset + 65536) - 1 }, token);
      scope.check(); if (bytes.byteLength < 1 || bytes.byteLength > output.byteLength - offset) { throw new VesperPlayerError('invalid_response', 'DASH text ended unexpectedly'); }
      output.set(new Uint8Array(bytes), offset); offset += bytes.byteLength;
    }
    return output.buffer as ArrayBuffer;
  };
  try {
    const bytes = await read(uri);
    const mpd = util.TextDecoder.create('utf-8', { fatal: true }).decodeWithStream(new Uint8Array(bytes));
    const manifest = bridge.invoke<Manifest>(JSON.stringify({ operation: 'parse_manifest', mpd: mpd, manifestUrl: manifestUri }));
    if (manifest.type !== 'static' || manifest.periods.length !== 1) { throw new VesperPlayerError('unsupported', 'DASH playback profile requires one static Period'); }
    const tracks: DashTextTrack[] = [];
    for (const adaptation of manifest.periods[0].adaptationSets) {
      if (adaptation.kind !== 'subtitle') { continue; }
      for (const representation of adaptation.representations) {
        if (tracks.length >= 16 || representation.codecs !== 'wvtt' || representation.segmentBase === undefined || representation.segmentBase === null) {
          throw new VesperPlayerError('unsupported', 'Embedded DASH captions require at most sixteen SegmentBase wvtt renditions');
        }
        tracks.push({ cues: parseWvtt(await read(representation.baseURL)), language: adaptation.language ?? '' });
      }
    }
    return tracks;
  } finally { remove(); token.cancel(); bridge.close(); }
}
