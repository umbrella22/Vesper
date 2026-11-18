import { VesperPlayerError } from '@vesper/player-kit';

export interface VesperWindowValues { keepScreenOn: boolean; brightness: number; }
export interface VesperWindowAdapter {
  identity: string;
  read(): VesperWindowValues;
  setKeepScreenOn(value: boolean): Promise<void>;
  setBrightness(value: number): Promise<void>;
}
export interface VesperWindowPolicySnapshot {
  requests: number; baselineKeepScreenOn: boolean; appliedKeepScreenOn?: boolean; pending: boolean;
}
interface WindowWaiter { resolve: () => void; reject: (error: Error) => void; timer: number; }

/** One physical write at a time; latest desired values replace intermediate gesture updates. */
class WindowOwner {
  readonly adapter: VesperWindowAdapter;
  readonly leases: Set<VesperWindowPolicy> = new Set();
  baseline: VesperWindowValues;
  applied?: VesperWindowValues;
  attemptedKeep?: boolean;
  attemptedBrightness?: number;
  failed: boolean = false;
  running: boolean = false;
  dirty: boolean = false;
  revision: number = 0;
  private waiters: Set<WindowWaiter> = new Set();
  private retired: () => void;
  constructor(adapter: VesperWindowAdapter, retired: () => void) {
    this.adapter = adapter; this.baseline = adapter.read(); this.retired = retired;
  }
  request(): Promise<void> {
    if (this.waiters.size >= 16) { return Promise.reject(new VesperPlayerError('capacity', 'Window policy waiter capacity reached')); }
    const work = new Promise<void>((resolve, reject) => {
      const waiter: WindowWaiter = { resolve: resolve, reject: reject, timer: -1 };
      waiter.timer = setTimeout((): void => {
        this.waiters.delete(waiter); reject(new VesperPlayerError('timeout', 'Window policy deadline exceeded'));
      }, 10000);
      this.waiters.add(waiter);
    });
    this.dirty = true; this.pump(); return work;
  }
  private pump(): void {
    if (this.running || !this.dirty) { return; }
    this.running = true; this.dirty = false;
    this.apply().then((): void => {
      this.running = false; this.failed = false;
      if (this.dirty) { this.pump(); }
      else { this.finish(); if (this.leases.size === 0) { this.retired(); } }
    }, (error: Error): void => {
      this.running = false; this.failed = true; this.finish(error);
      if (this.dirty) { this.pump(); }
    });
  }
  private async apply(): Promise<void> {
    const current = this.adapter.read();
    // A native mutation can take effect before rejecting. Its attempted value still
    // belongs to this owner and must never become the host's restoration baseline.
    if (this.applied !== undefined || this.attemptedKeep !== undefined) {
      if (current.keepScreenOn !== this.applied?.keepScreenOn && current.keepScreenOn !== this.attemptedKeep) {
        this.baseline.keepScreenOn = current.keepScreenOn;
      }
    }
    if (this.applied !== undefined || this.attemptedBrightness !== undefined) {
      if (current.brightness !== this.applied?.brightness && current.brightness !== this.attemptedBrightness) {
        this.baseline.brightness = current.brightness;
      }
    }
    let keep = this.baseline.keepScreenOn; let brightness = this.baseline.brightness; let revision = -1;
    for (const lease of Array.from(this.leases)) {
      keep = keep || lease.keep;
      if (lease.visible && lease.brightnessValue !== undefined && lease.brightnessRevision > revision) {
        brightness = lease.brightnessValue; revision = lease.brightnessRevision;
      }
    }
    // Physical completion retains serialization even if a caller's deadline expires.
    if (current.keepScreenOn !== keep) { this.attemptedKeep = keep; await this.adapter.setKeepScreenOn(keep); }
    this.applied = { keepScreenOn: keep, brightness: current.brightness };
    if (current.brightness !== brightness) { this.attemptedBrightness = brightness; await this.adapter.setBrightness(brightness); }
    this.applied = { keepScreenOn: keep, brightness: brightness };
  }
  private finish(error?: Error): void {
    const waiters = Array.from(this.waiters); this.waiters.clear();
    for (const waiter of waiters) { clearTimeout(waiter.timer); if (error !== undefined) { waiter.reject(error); } else { waiter.resolve(); } }
  }
}

/** Leases coordinate a window's host baseline, playback requests and temporary brightness. */
export class VesperWindowPolicy {
  private static owners: Map<string, WindowOwner> = new Map();
  private owner: WindowOwner;
  /** @internal */ keep: boolean = false;
  /** @internal */ visible: boolean = true;
  /** @internal */ brightnessValue?: number;
  /** @internal */ brightnessRevision: number = 0;
  private closed: boolean = false;
  private closeWork?: Promise<void>;
  constructor(adapter: VesperWindowAdapter) {
    let owner = VesperWindowPolicy.owners.get(adapter.identity);
    if (owner === undefined) {
      if (VesperWindowPolicy.owners.size >= 8) { throw new VesperPlayerError('capacity', 'Window policy owner capacity reached'); }
      owner = new WindowOwner(adapter, (): void => { VesperWindowPolicy.owners.delete(adapter.identity); });
      VesperWindowPolicy.owners.set(adapter.identity, owner);
    }
    if (owner.leases.size >= 16) { throw new VesperPlayerError('capacity', 'Window policy lease capacity reached'); }
    this.owner = owner; owner.leases.add(this);
  }
  update(visible: boolean, playingVideo: boolean, keepScreenOnDuringPlayback: boolean = true): Promise<void> {
    if (this.closed) { return Promise.reject(new VesperPlayerError('closed', 'Window policy is closed')); }
    const keep = visible && playingVideo && keepScreenOnDuringPlayback;
    if (visible === this.visible && keep === this.keep && !this.owner.failed) { return Promise.resolve(); }
    this.visible = visible; this.keep = keep; return this.owner.request();
  }
  brightness(): number {
    const value = this.brightnessValue ?? this.owner.adapter.read().brightness;
    return value < 0 ? 0.5 : value;
  }
  setBrightness(value: number): Promise<void> {
    if (this.closed) { return Promise.reject(new VesperPlayerError('closed', 'Window policy is closed')); }
    if (!Number.isFinite(value) || value < 0 || value > 1) { return Promise.reject(new VesperPlayerError('invalid_argument', 'Brightness must be in [0, 1]')); }
    this.brightnessValue = value; this.brightnessRevision = ++this.owner.revision; return this.owner.request();
  }
  /** Hosts changing their own policy while a lease exists use this entrypoint. */
  setHostKeepScreenOn(value: boolean): Promise<void> {
    if (this.closed) { return Promise.reject(new VesperPlayerError('closed', 'Window policy is closed')); }
    this.owner.baseline.keepScreenOn = value; return this.owner.request();
  }
  snapshot(): VesperWindowPolicySnapshot {
    return { requests: Array.from(this.owner.leases).filter((lease: VesperWindowPolicy): boolean => lease.keep).length,
      baselineKeepScreenOn: this.owner.baseline.keepScreenOn, appliedKeepScreenOn: this.owner.applied?.keepScreenOn, pending: this.owner.running };
  }
  close(): Promise<void> {
    if (this.closeWork !== undefined) { return this.closeWork; }
    this.closed = true; this.owner.leases.delete(this);
    this.closeWork = this.owner.request();
    this.closeWork.catch((): void => { this.closeWork = undefined; }); return this.closeWork;
  }
}
