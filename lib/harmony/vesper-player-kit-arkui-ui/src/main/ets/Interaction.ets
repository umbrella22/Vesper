/** Coalesce continuous input to one physical command and one latest value. */
export class StageValuePump {
  private pending?: number;
  private running: boolean = false;
  private generation: number = 0;
  private apply: (value: number) => Promise<void>;
  private failed: (error: Error) => void;
  constructor(apply: (value: number) => Promise<void>, failed: (error: Error) => void) {
    this.apply = apply; this.failed = failed;
  }
  submit(value: number): void { this.pending = value; this.pump(); }
  cancel(): void { this.pending = undefined; this.generation++; }
  private pump(): void {
    if (this.running || this.pending === undefined) { return; }
    const value = this.pending; const generation = this.generation;
    this.pending = undefined; this.running = true;
    this.apply(value).catch((error: Error): void => { if (generation === this.generation) { this.failed(error); } })
      .finally((): void => { this.running = false; this.pump(); });
  }
}

/** Temporary rates restore the captured value after release, including release during apply. */
export class StageTemporaryRate {
  private previous?: number;
  private epoch: string = '';
  private active: boolean = false;
  private work: Promise<void> = Promise.resolve();
  private set: (rate: number) => Promise<void>;
  private currentEpoch: () => string;
  private failed: (error: Error) => void;
  constructor(set: (rate: number) => Promise<void>, currentEpoch: () => string, failed: (error: Error) => void) {
    this.set = set; this.currentEpoch = currentEpoch; this.failed = failed;
  }
  begin(rate: number): boolean {
    if (this.active || this.previous !== undefined || !Number.isFinite(rate)) { return false; }
    this.previous = rate; this.epoch = this.currentEpoch(); this.active = true;
    this.work = this.set(2).catch((error: Error): void => { this.failed(error); });
    return true;
  }
  end(): Promise<void> {
    if (!this.active) { return this.work; }
    this.active = false; const previous = this.previous; const epoch = this.epoch;
    this.work = this.work.then(async (): Promise<void> => {
      if (previous !== undefined && epoch === this.currentEpoch()) { await this.set(previous); }
    }).catch((error: Error): void => { this.failed(error); }).finally((): void => { this.previous = undefined; });
    return this.work;
  }
}

export function stageGestureAxis(dx: number, dy: number): string {
  if (Math.abs(dx) > Math.abs(dy) * 1.15) { return 'horizontal'; }
  if (Math.abs(dy) > Math.abs(dx) * 1.15) { return 'vertical'; }
  return '';
}
