import native from 'libvesper_player_harmony_node.so';
import { VesperPlayerError } from './Types';

interface BridgeError { code: string; message: string; }
interface Envelope<T> { ok: boolean; value?: T; error?: BridgeError; }

/** Internal synchronous bridge for finite CPU work; no platform I/O runs here. */
export class NativeSession {
  readonly identity: string;
  private closed: boolean = false;
  constructor(configurationJson: string) { this.identity = native.create(configurationJson); }
  invoke<T>(commandJson: string): T {
    if (this.closed) { throw new VesperPlayerError('closed', 'Session is closed'); }
    const envelope = JSON.parse(native.call(this.identity, commandJson)) as Envelope<T>;
    if (!envelope.ok || envelope.value === undefined) {
      throw new VesperPlayerError(envelope.error?.code ?? 'invalid_response',
        envelope.error?.message ?? 'Native bridge response is invalid');
    }
    return envelope.value;
  }
  close(): void {
    if (this.closed) { return; }
    native.release(this.identity);
    this.closed = true;
  }
}
