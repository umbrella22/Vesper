export interface VesperBrightnessAdapter { current(): number; set(value: number): Promise<void>; }
export interface VesperStageEnvironment { brightness?: VesperBrightnessAdapter; }
export interface VesperStageInsets { top: number; right: number; bottom: number; left: number; }
export interface VesperGestureStart {
  x: number; width: number; height: number; positionMs: number; durationMs?: number;
  volume: number; brightness?: number; timeline: string;
}
export interface VesperGestureIntent { kind: string; value: number; }

/** Seek previews commit on release; continuous controls may apply through a bounded pump. */
export function gestureIntent(start: VesperGestureStart, dx: number, dy: number, axis: string): VesperGestureIntent | undefined {
  if (![start.x, start.width, start.height, dx, dy].every((value: number) => Number.isFinite(value))
    || start.width <= 0 || start.height <= 0) { return undefined; }
  if (axis === 'horizontal') {
    if (start.timeline !== 'vod' || start.durationMs === undefined || start.durationMs <= 0) { return undefined; }
    return { kind: 'seek', value: Math.round(Math.max(0, Math.min(start.durationMs, start.positionMs + dx / start.width * Math.min(start.durationMs, 120000)))) };
  }
  const brightness = start.x < start.width / 2;
  const initial = brightness ? start.brightness : start.volume;
  if (initial === undefined || !Number.isFinite(initial)) { return undefined; }
  return { kind: brightness ? 'brightness' : 'volume', value: Math.max(0, Math.min(1, initial - dy / start.height)) };
}
