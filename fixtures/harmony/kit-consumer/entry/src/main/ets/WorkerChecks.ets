import { worker, MessageEvents, ErrorEvent } from '@kit.ArkTS';
import { VesperSourceSession } from '@vesper/player-kit';

function closeWorkerEnvironment(): Promise<void> {
  return new Promise<void>((resolve, reject): void => {
    const instance = new worker.ThreadWorker('entry/ets/workers/SessionWorker.ets', { name: 'vesper-session-cleanup' });
    let settled = false;
    let retained = false;
    let failure: string = '';
    const deadline = setTimeout((): void => {
      if (settled) { return; }
      settled = true; instance.terminate();
      reject(new Error('Worker environment cleanup exceeded 10 seconds'));
    }, 10000);
    instance.onmessage = (event: MessageEvents): void => {
      const message = event.data as string;
      if (message === 'retained-64') { retained = true; }
      else { failure = message; }
    };
    instance.onerror = (error: ErrorEvent): void => { failure = error.message; instance.terminate(); };
    instance.onexit = (code: number): void => {
      clearTimeout(deadline);
      if (settled) { return; }
      settled = true;
      if (code === 0 && retained && failure.length === 0) { resolve(); }
      else { reject(new Error(`Worker exit ${code}, retained=${retained}, ${failure}`)); }
    };
    try { instance.postMessage('start'); }
    catch (error) {
      clearTimeout(deadline); settled = true; instance.terminate();
      reject(new Error(`Cannot start session Worker: ${error}`));
    }
  });
}

/** The Worker must return all 64 process-wide native slots when its VM closes. */
export async function runWorkerCleanupChecks(): Promise<string[]> {
  await closeWorkerEnvironment();
  const sessions: VesperSourceSession[] = [];
  try {
    for (let index = 0; index < 64; index++) { sessions.push(new VesperSourceSession()); }
    return ['Worker retained 64 native sessions until environment exit', 'Main environment recovered all 64 native slots'];
  } finally {
    for (const session of sessions) { session.close(); }
  }
}
