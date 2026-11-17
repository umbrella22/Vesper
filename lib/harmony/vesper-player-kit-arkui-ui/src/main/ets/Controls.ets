/** Presentation helpers keep live and unknown-duration media out of VOD controls. */
export function stageClock(milliseconds: number | undefined): string {
  if (milliseconds === undefined || !Number.isFinite(milliseconds) || milliseconds < 0) { return '--:--'; }
  const seconds = Math.floor(milliseconds / 1000);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor(seconds % 3600 / 60);
  return `${hours > 0 ? `${hours}:` : ''}${minutes.toString().padStart(2, '0')}:${(seconds % 60).toString().padStart(2, '0')}`;
}

export function stageSeekTarget(timeline: string, durationMs: number | undefined, positionMs: number, deltaMs: number): number | undefined {
  if (timeline !== 'vod' || durationMs === undefined || !Number.isFinite(durationMs) || durationMs <= 0 ||
    !Number.isFinite(positionMs) || !Number.isFinite(deltaMs)) { return undefined; }
  return Math.round(Math.max(0, Math.min(durationMs, positionMs + deltaMs)));
}

/** A Stage owns at most one hide timer; pausing, interaction and disposal cancel it. */
export class StageControlTimer {
  private timer: number = -1;
  private hide: () => void;
  constructor(hide: () => void) { this.hide = hide; }
  arm(enabled: boolean): void {
    this.clear();
    if (enabled) { this.timer = setTimeout((): void => { this.timer = -1; this.hide(); }, 3000); }
  }
  clear(): void { if (this.timer !== -1) { clearTimeout(this.timer); this.timer = -1; } }
}
