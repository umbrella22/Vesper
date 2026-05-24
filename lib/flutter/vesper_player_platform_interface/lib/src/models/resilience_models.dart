part of '../models.dart';

enum VesperBufferingPreset {
  defaultPreset,
  balanced,
  streaming,
  resilient,
  lowLatency,
}

enum VesperRetryBackoff { fixed, linear, exponential }

enum VesperCachePreset { defaultPreset, disabled, streaming, resilient }

enum VesperPlayerErrorCategory {
  input,
  source,
  network,
  decode,
  audioOutput,
  playback,
  capability,
  platform,
}

enum VesperPlayerErrorCode {
  invalidArgument,
  invalidState,
  invalidSource,
  backendFailure,
  audioOutputUnavailable,
  decodeFailure,
  seekFailure,
  unsupported,
  commandChannelClosed,
  eventChannelClosed,
  cancelled,
  timeout,
}

