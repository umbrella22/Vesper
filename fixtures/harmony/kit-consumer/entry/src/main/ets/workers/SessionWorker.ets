import { worker } from '@kit.ArkTS';
import { VesperSourceSession } from '@vesper/player-kit';

const retained: VesperSourceSession[] = [];
let started: boolean = false;
worker.workerPort.onmessage = (): void => {
  if (started) { return; }
  started = true;
  try {
    for (let index = 0; index < 64; index++) { retained.push(new VesperSourceSession()); }
    worker.workerPort.postMessage('retained-64');
    // Intentionally retain sessions: the Node-API environment hook owns cleanup.
    worker.workerPort.close();
  } catch (error) {
    worker.workerPort.postMessage(`FAIL: ${error}`); worker.workerPort.close();
  }
};
