import { VesperPlayerError } from '@vesper/player-kit';

/** Message keys carry no source URL, credentials, or native error message. */
export interface VesperOperationFeedback {
  code: string;
  nativeCode?: number;
  category: string;
  messageKey: string;
  action: string;
  retryable: boolean;
}

export function playbackFeedback(error: Error): VesperOperationFeedback {
  const native = error as VesperPlayerError;
  const code = typeof native.code === 'string' ? native.code : 'unknown';
  let category = 'playback'; let action = 'changeSource'; let retryable = false;
  if (['cancelled', 'canceled', 'superseded', 'stale_epoch', 'detached', 'stopped', 'disposed', 'closed'].includes(code)) {
    category = 'cancelled'; action = 'dismiss';
  } else if (['http_401', 'http_403', 'expired', 'source_expired', 'resource_changed'].includes(code) || native.nativeCode === 5411005) {
    category = 'expired'; action = 'refreshSource';
  } else if (code === 'unsupported' || [5400106, 5411011].includes(native.nativeCode ?? 0)) {
    category = 'unsupported';
  } else if (['invalid_argument', 'invalid_source', 'invalid_url', 'redirect', 'http_404', 'http_410'].includes(code) || native.nativeCode === 5411007) {
    category = 'source';
  } else if (['capacity', 'export_cleanup_required', 'disk_full'].includes(code)) {
    category = 'capacity'; action = 'manageStorage';
  } else if (code === 'in_use') {
    category = 'inUse'; action = 'dismiss';
  } else if (['timeout', 'network_failure', 'http_408', 'http_429', 'http_500', 'http_502', 'http_503', 'http_504'].includes(code) ||
    [5400104, 5411001, 5411002, 5411003, 5411004].includes(native.nativeCode ?? 0)) {
    category = 'network'; action = 'retry'; retryable = true;
  } else if (code === 'not_ready') {
    category = 'notReady'; action = 'dismiss';
  }
  return { code: code, nativeCode: native.nativeCode, category: category,
    messageKey: `vesper_error_${category}`, action: action, retryable: retryable };
}
