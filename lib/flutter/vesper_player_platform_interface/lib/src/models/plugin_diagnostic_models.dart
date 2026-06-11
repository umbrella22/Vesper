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

enum VesperPluginCapabilityKind { decoder, frameProcessor, sourceNormalizer }

enum VesperPluginParticipation {
  unknown,
  available,
  selected,
  participated,
  bypassed,
  fallback,
}
