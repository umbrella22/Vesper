import native from 'libvesper_player_harmony_node.so';
import { VesperPlayerError } from './Types';

export interface VesperVideoDecoderQuery { mimeType: string; width?: number; height?: number; profile?: number; level?: number; }
export interface VesperDecoderIntegerRange { min: number; max: number; }
export interface VesperVideoDecoderCapability {
  mimeType: string; status: string; reason?: string; name?: string; hardware?: boolean;
  sizeSupported?: boolean; profileAndLevelSupported?: boolean; profiles?: number[];
  widthRange?: VesperDecoderIntegerRange; heightRange?: VesperDecoderIntegerRange; frameRateRange?: VesperDecoderIntegerRange;
  decoderExecutionEvidence: string;
}

/** Device preflight only. AVPlayer decoder selection and execution remain unobservable. */
export function queryVideoDecoderCapability(query: VesperVideoDecoderQuery): VesperVideoDecoderCapability {
  const paired = (first: number | undefined, second: number | undefined, minimum: number): boolean => {
    if (first === undefined && second === undefined) { return true; }
    return first !== undefined && second !== undefined && Number.isInteger(first) && Number.isInteger(second)
      && first >= minimum && second >= minimum && first <= 2147483647 && second <= 2147483647;
  };
  if (!paired(query.width, query.height, 1) || !paired(query.profile, query.level, 0)) {
    throw new VesperPlayerError('invalid_argument', 'Decoder capability dimensions and profile/level require paired int32 values');
  }
  return native.queryVideoDecoder(query.mimeType, query.width ?? 0, query.height ?? 0, query.profile ?? -1, query.level ?? -1) as VesperVideoDecoderCapability;
}
