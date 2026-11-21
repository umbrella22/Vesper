import { common } from '@kit.AbilityKit';
import { VesperPlayerController, VesperPlayerError, VesperSourceSession } from '@vesper/player-kit';

export async function runRtspChecks(context: common.UIAbilityContext, surface: string, enabled: boolean): Promise<string> {
  const sources = new VesperSourceSession();
  const controller = new VesperPlayerController(context);
  controller.attachSurface(surface);
  const source = sources.register({ kind: 'remote', protocol: 'rtsp', uri: 'rtsp://127.0.0.1:18554/clock', label: 'RTSP fixture' });
  try {
    try {
      await controller.activate(source, { timeoutMs: 20000 });
    } catch (error) {
      if (!enabled && error instanceof VesperPlayerError && error.code === 'rtsp_bridge_not_bundled') {
        return JSON.stringify({ code: error.code });
      }
      throw new Error(`RTSP activation failed: ${error}`);
    }
    if (!enabled) { throw new Error('Activation accepted RTSP with the feature disabled.'); }
    const deadline = Date.now() + 12000;
    let firstPosition: number | undefined;
    while (Date.now() < deadline) {
      const snapshot = controller.snapshot();
      if (snapshot.error !== undefined) { throw new Error(`RTSP playback failed: ${snapshot.error.code}`); }
      if (snapshot.firstFrame && snapshot.state === 'playing') {
        firstPosition ??= snapshot.positionMs;
        if (snapshot.positionMs - firstPosition >= 1500) {
          if (snapshot.timeline !== 'live' || snapshot.durationMs !== undefined) { throw new Error('Expected RTSP live timeline.'); }
          return JSON.stringify({ firstFrameMs: snapshot.firstFrameMs, progressMs: snapshot.positionMs - firstPosition,
            width: snapshot.width, height: snapshot.height, timeline: snapshot.timeline, api: controller.capabilities().runtimeApi });
        }
      }
      await new Promise<void>((resolve) => { setTimeout(resolve, 100); });
    }
    throw new Error('The first frame and advancing playback position were not both observed.');
  } finally {
    source.close();
    await controller.dispose();
    sources.close();
  }
}
