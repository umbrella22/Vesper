import { VesperPlayerError } from './Types';

export interface SubtitleCue { startMs: number; endMs: number; text: string; }
function timestamp(value: string): number {
  const match = value.match(/^(?:(\d{1,3}):)?(\d{2}):(\d{2})[.,](\d{3})$/);
  if (match === null || Number(match[2]) > 59 || Number(match[3]) > 59) {
    throw new VesperPlayerError('invalid_subtitle', 'Subtitle timestamp is invalid');
  }
  return ((Number(match[1] ?? 0) * 60 + Number(match[2])) * 60 + Number(match[3])) * 1000 + Number(match[4]);
}

/** Bounded plain-text SRT/WebVTT sidecars leave audio/video execution in AVPlayer. */
export function parseSubtitles(text: string, mime: string): SubtitleCue[] {
  if (text.length > 256 * 1024 || !['text/vtt', 'application/x-subrip', 'application/srt'].includes(mime)) {
    throw new VesperPlayerError('unsupported', 'External subtitles require SRT or WebVTT within 256 KiB');
  }
  const normalized = text.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n');
  if (mime === 'text/vtt' && !/^WEBVTT(?:\n|\s)/.test(normalized)) {
    throw new VesperPlayerError('invalid_subtitle', 'WebVTT header is missing');
  }
  const cues: SubtitleCue[] = [];
  for (const block of normalized.split(/\n[ \t]*\n/)) {
    const lines = block.split('\n');
    if (/^(WEBVTT|NOTE|STYLE|REGION)(?:\s|$)/.test(lines[0])) { continue; }
    const index = lines[0].includes('-->') ? 0 : 1;
    const timing = lines[index]?.match(/^(\S+)\s+-->\s+(\S+)(?:\s+.*)?$/);
    if (timing === undefined || timing === null) {
      if (block.trim().length > 0) { throw new VesperPlayerError('invalid_subtitle', 'Subtitle cue timing is missing'); }
      continue;
    }
    const start = timestamp(timing[1]); const end = timestamp(timing[2]);
    const body = lines.slice(index + 1).join('\n').replace(/\n+$/, '');
    if (end <= start || body.length > 8192 || cues.length >= 4096) {
      throw new VesperPlayerError('invalid_subtitle', 'Subtitle cue duration, text, or count exceeds its contract');
    }
    cues.push({ startMs: start, endMs: end, text: body });
  }
  return cues.sort((a: SubtitleCue, b: SubtitleCue) => a.startMs - b.startMs);
}

export function subtitleAt(cues: SubtitleCue[], positionMs: number): string {
  const active: string[] = [];
  for (const cue of cues) {
    if (cue.startMs > positionMs) { break; }
    if (positionMs < cue.endMs) { active.push(cue.text); if (active.length === 8) { break; } }
  }
  return active.join('\n').substring(0, 16384);
}
