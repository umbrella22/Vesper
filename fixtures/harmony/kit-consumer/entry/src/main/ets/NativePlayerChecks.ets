import { media } from '@kit.MediaKit';

/** Native Promise completion can precede the corresponding state notification. */
export async function prepareAndPlayNative(player: media.AVPlayer, timeoutMs: number = 15000): Promise<void> {
  await player.prepare();
  const deadline = Date.now() + timeoutMs;
  while (player.state !== 'prepared') {
    if (Date.now() >= deadline) { throw new Error('Native prepared-state observation deadline'); }
    await new Promise<void>((resolve) => { setTimeout(resolve, 50); });
  }
  await player.play();
}
