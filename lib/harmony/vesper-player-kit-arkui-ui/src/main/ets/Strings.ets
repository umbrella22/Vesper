import { UIContext } from '@kit.ArkUI';
import { VesperOperationFeedback } from '@vesper/player-kit-arkui';

export function stageText(context: UIContext, resource: Resource, value?: number): string {
  const manager = context.getHostContext()?.resourceManager;
  if (manager === undefined) { return ''; }
  return value === undefined ? manager.getStringSync(resource.id) : manager.getStringSync(resource.id, value);
}
export function feedbackResource(feedback: VesperOperationFeedback): Resource {
  switch (feedback.category) {
    case 'network': return $r('app.string.vesper_error_network');
    case 'unsupported': return $r('app.string.vesper_error_unsupported');
    case 'expired': return $r('app.string.vesper_error_expired');
    case 'source': return $r('app.string.vesper_error_source');
    case 'capacity': return $r('app.string.vesper_error_capacity');
    case 'inUse': return $r('app.string.vesper_error_in_use');
    case 'notReady': return $r('app.string.vesper_error_not_ready');
    case 'cancelled': return $r('app.string.vesper_error_cancelled');
    default: return $r('app.string.vesper_error_playback');
  }
}
