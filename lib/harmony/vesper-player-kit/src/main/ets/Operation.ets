import { systemDateTime } from '@kit.BasicServicesKit';
import { VesperPlayerError } from './Types';

export function monotonicMs(): number { return systemDateTime.getUptime(systemDateTime.TimeType.STARTUP); }

export class VesperCancellationToken {
  private callbacks: Set<() => void> = new Set();
  private cancelled: boolean = false;
  observerFailures: number = 0;
  get isCancelled(): boolean { return this.cancelled; }
  cancel(): void {
    if (this.cancelled) { return; }
    this.cancelled = true;
    const callbacks = Array.from(this.callbacks);
    this.callbacks.clear();
    for (const callback of callbacks) { try { callback(); } catch (_) { this.observerFailures++; } }
  }
  /** @internal */
  observe(callback: () => void): () => void {
    if (this.cancelled) { callback(); return (): void => {}; }
    if (this.callbacks.size >= 64) { throw new VesperPlayerError('capacity', 'Cancellation observer capacity reached'); }
    this.callbacks.add(callback);
    return (): void => { this.callbacks.delete(callback); };
  }
}

/** One operation deadline and one terminal cancellation reason for all its waits. */
export class OperationScope {
  private timer: number;
  private failure?: VesperPlayerError;
  private callbacks: Set<(error: VesperPlayerError) => void> = new Set();
  private unsubscribe: () => void = (): void => {};
  callbackFailures: number = 0;
  constructor(timeoutMs: number, token?: VesperCancellationToken) {
    if (!Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 60000) {
      throw new VesperPlayerError('invalid_argument', 'Timeout must be between 1 and 60000 milliseconds');
    }
    this.timer = setTimeout(() => { this.fail(new VesperPlayerError('timeout', 'Operation deadline exceeded')); }, timeoutMs);
    if (token !== undefined) {
      try { this.unsubscribe = token.observe(() => { this.fail(new VesperPlayerError('cancelled', 'Operation was cancelled')); }); }
      catch (error) { clearTimeout(this.timer); throw nativeError(error, 'cancellation registration'); }
    }
  }
  check(): void { if (this.failure !== undefined) { throw this.failure; } }
  fail(error: VesperPlayerError): void {
    if (this.failure !== undefined) { return; }
    this.failure = error;
    const callbacks = Array.from(this.callbacks);
    this.callbacks.clear();
    for (const callback of callbacks) { try { callback(error); } catch (_) { this.callbackFailures++; } }
  }
  onFailure(callback: (error: VesperPlayerError) => void): () => void {
    if (this.failure !== undefined) { callback(this.failure); return (): void => {}; }
    if (this.callbacks.size >= 64) { throw new VesperPlayerError('capacity', 'Operation waiter capacity reached'); }
    this.callbacks.add(callback);
    return (): void => { this.callbacks.delete(callback); };
  }
  wait<T>(work: Promise<T>): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const remove = this.onFailure((error: VesperPlayerError): void => { reject(error); });
      work.then((value: T) => { remove(); if (this.failure === undefined) { resolve(value); } },
        (error: Error) => { remove(); reject(error); });
    });
  }
  until(ready: () => boolean): Promise<void> {
    try { this.check(); if (ready()) { return Promise.resolve(); } }
    catch (error) { return Promise.reject(error); }
    let timer: number = -1;
    const work = new Promise<void>((resolve, reject) => {
      timer = setInterval(() => {
        try { if (ready()) { clearInterval(timer); resolve(); } }
        catch (error) { clearInterval(timer); reject(error); }
      }, 20);
    });
    return this.wait(work).finally(() => { clearInterval(timer); });
  }
  close(): void { clearTimeout(this.timer); this.unsubscribe(); this.callbacks.clear(); }
}

interface RawNativeError { code?: number; }
export function nativeError(error: Object, phase: string): VesperPlayerError {
  if (error instanceof VesperPlayerError) { return error; }
  const raw = error as RawNativeError;
  return new VesperPlayerError('native_failure', `Native operation failed during ${phase}`, raw.code);
}
