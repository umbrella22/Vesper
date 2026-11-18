import { systemShare } from '@kit.ShareKit';
import { VesperPlayerError } from './Types';

/** Loaded only when the host explicitly requests system sharing. */
export function createShare(uri: string, title: string): systemShare.ShareController {
  try {
    const data = new systemShare.SharedData({ utd: 'general.file', uri: uri, title: title });
    return new systemShare.ShareController(data);
  } catch (error) {
    if (error instanceof TypeError) { throw new VesperPlayerError('unsupported', 'System Share Kit is unavailable on this device'); }
    throw error as Error;
  }
}
