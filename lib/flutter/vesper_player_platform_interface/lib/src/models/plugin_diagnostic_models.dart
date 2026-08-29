part of '../models.dart';

enum VesperPluginDiagnosticStatus {
  loaded,
  loadFailed,
  unsupportedKind,
  decoderSupported,
  decoderUnsupported,
  frameProcessorSupported,
  frameProcessorUnsupported,
  sourceNormalizerSupported,
  sourceNormalizerUnsupported,
}

enum VesperPluginCapabilityKind {
  decoder,
  frameProcessor,
  sourceNormalizer,
  unknown,
}

enum VesperPluginParticipation {
  unknown,
  available,
  selected,
  participated,
  bypassed,
  fallback,
}

/// Stable playback routes reported by plugin diagnostics.
enum VesperPluginPlaybackRoute {
  systemPlayer,
  sdkManagedNativeFrame,
  softwareDecoder,
  unknown,
}
