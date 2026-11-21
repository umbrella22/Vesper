import { NativeSession } from './NativeSession';
import { OperationScope, nativeError } from './Operation';
import { VesperPlayerError } from './Types';

interface RtspStatus {
  phase: string;
  playlistUri?: string;
  code?: string;
  message?: string;
}

/** Owns one Rust loopback route for the lifetime of its AVPlayer. */
export class RtspSession {
  private readonly bridge: NativeSession;

  constructor(uri: string) {
    this.bridge = new NativeSession(JSON.stringify({ kind: 'rtsp' }));
    try {
      this.bridge.invoke<boolean>(JSON.stringify({ op: 'open', uri: uri }));
    } catch (error) {
      this.bridge.close();
      throw nativeError(error, 'RTSP bridge open');
    }
  }

  async playlist(scope: OperationScope): Promise<string> {
    let uri: string | undefined;
    await scope.until((): boolean => {
      uri = this.readyPlaylist();
      return uri !== undefined;
    });
    if (uri === undefined) {
      throw new VesperPlayerError('rtsp_invalid_response', 'RTSP bridge did not return a playlist.');
    }
    return uri;
  }

  checkHealth(): void { this.readyPlaylist(); }
  close(): void { this.bridge.close(); }

  private readyPlaylist(): string | undefined {
    const value = this.bridge.invoke<RtspStatus>('{"op":"status"}');
    if (value.phase === 'connecting' || value.phase === 'reconnecting') { return undefined; }
    if (value.phase === 'ready' && value.playlistUri !== undefined) { return value.playlistUri; }
    throw new VesperPlayerError(value.code ?? 'rtsp_invalid_response', value.message ?? 'RTSP bridge failed.');
  }
}
