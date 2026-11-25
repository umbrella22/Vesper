import { url } from '@kit.ArkTS';
import { VesperDownloadSnapshot, VesperPlayerError, VesperPlayerSource } from '@vesper/player-kit';

export interface ExamplePalette {
  top: ResourceColor;
  bottom: ResourceColor;
  surface: ResourceColor;
  field: ResourceColor;
  title: ResourceColor;
  secondary: ResourceColor;
  accent: ResourceColor;
  onAccent: ResourceColor;
  accentSoft: ResourceColor;
  line: ResourceColor;
  danger: ResourceColor;
}

/**
 * Shell colors come from the base/dark resource qualifiers; the application color mode
 * (system, light, or dark) selects the set, so the flag only keeps call sites explicit.
 */
export function examplePalette(dark: boolean): ExamplePalette {
  return {
    top: $r('app.color.bg'), bottom: $r('app.color.bg'),
    surface: $r('app.color.surface'), field: $r('app.color.surface_2'),
    title: $r('app.color.ink'), secondary: $r('app.color.ink_2'),
    accent: dark ? $r('app.color.accent') : $r('app.color.accent_ink'), onAccent: $r('app.color.on_accent'),
    accentSoft: $r('app.color.accent_soft'), line: $r('app.color.line'),
    danger: $r('app.color.err')
  };
}

export interface ExampleLogEntry {
  id: number;
  time: string;
  title: string;
  detail: string;
  failed: boolean;
}

export const EXAMPLE_QUEUE_CAPACITY: number = 12;
export const EXAMPLE_LOG_CAPACITY: number = 80;
export const EXAMPLE_IMPORT_BYTES: number = 1024 * 1024 * 1024;
export const EXAMPLE_HLS_URL: string =
  'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8';
export const EXAMPLE_DASH_URL: string = 'https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd';
export const EXAMPLE_MP4_URL: string = 'https://media.w3.org/2010/05/sintel/trailer.mp4';

/** Infer from the URL path only; signed query parameters never change the media type. */
export function remoteSource(input: string, protocol: string = 'auto'): VesperPlayerSource {
  const uri = input.trim();
  let parsed: url.URL;
  try { parsed = new url.URL(uri); }
  catch (_) { throw new VesperPlayerError('invalid_argument', 'Enter a complete HTTP or HTTPS media URL.'); }
  if (!['http:', 'https:'].includes(parsed.protocol) || parsed.hostname.length === 0) {
    throw new VesperPlayerError('unsupported', 'Use an HTTP or HTTPS media URL.');
  }
  if (parsed.username.length > 0 || parsed.password.length > 0) {
    throw new VesperPlayerError('unsupported', 'URLs with embedded credentials are not supported by this example.');
  }
  if (!['auto', 'progressive', 'hls', 'dash'].includes(protocol)) {
    throw new VesperPlayerError('invalid_argument', 'Choose MP4, HLS, DASH, or automatic media detection.');
  }
  const path = parsed.pathname.toLowerCase();
  const selected = protocol === 'auto' ? path.endsWith('.m3u8') ? 'hls' : path.endsWith('.mpd') ? 'dash' : 'progressive' : protocol;
  return { uri: uri, kind: 'remote', protocol: selected, label: `Stream from ${parsed.hostname}` };
}

export function formatTime(milliseconds: number | undefined): string {
  if (milliseconds === undefined || !Number.isFinite(milliseconds) || milliseconds < 0) { return '--:--'; }
  const seconds = Math.floor(milliseconds / 1000);
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${(seconds % 60).toString().padStart(2, '0')}`;
}

export function formatBytes(bytes: string | number | undefined): string {
  const value = bytes === undefined ? Number.NaN : Number(bytes);
  if (!Number.isSafeInteger(value) || value < 0) { return 'Unknown size'; }
  if (value < 1024) { return `${value} B`; }
  if (value < 1024 * 1024) { return `${(value / 1024).toFixed(1)} KB`; }
  return `${(value / (1024 * 1024)).toFixed(1)} MB`;
}

export function downloadPercent(snapshot: VesperDownloadSnapshot): number | undefined {
  const received = Number(snapshot.receivedBytes);
  const total = snapshot.totalBytes === undefined ? Number.NaN : Number(snapshot.totalBytes);
  if (!Number.isSafeInteger(received) || !Number.isSafeInteger(total) || received < 0 || total <= 0 || received > total) { return undefined; }
  return received * 100 / total;
}

export function appendLog(entries: ExampleLogEntry[], entry: ExampleLogEntry): ExampleLogEntry[] {
  return [entry].concat(entries).slice(0, EXAMPLE_LOG_CAPACITY);
}

export function downloadCanPause(status: string): boolean {
  return ['Queued', 'Preparing', 'Downloading'].includes(status);
}
