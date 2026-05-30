part of '../models.dart';

enum VesperSourceNormalizerMode {
  disabled,
  diagnosticsOnly,
  preflightOnly,
  preferNormalized,
  requireNormalized,
}

final class VesperSourceNormalizerConfiguration {
  const VesperSourceNormalizerConfiguration({
    this.mode = VesperSourceNormalizerMode.disabled,
    this.pluginLibraryPaths = const <String>[],
    this.runtimeProfile,
  });

  factory VesperSourceNormalizerConfiguration.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperSourceNormalizerConfiguration(
      mode: _decodeEnum(
        VesperSourceNormalizerMode.values,
        map['mode'],
        VesperSourceNormalizerMode.disabled,
      ),
      pluginLibraryPaths: _decodeStringList(map['pluginLibraryPaths']),
      runtimeProfile: map['runtimeProfile'] as String?,
    );
  }

  final VesperSourceNormalizerMode mode;
  final List<String> pluginLibraryPaths;
  final String? runtimeProfile;

  bool get hasOverrides =>
      mode != VesperSourceNormalizerMode.disabled ||
      pluginLibraryPaths.isNotEmpty ||
      runtimeProfile != null;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'mode': mode.name,
      'pluginLibraryPaths': pluginLibraryPaths,
      if (runtimeProfile != null) 'runtimeProfile': runtimeProfile,
    };
  }
}

enum VesperFrameProcessorMode {
  disabled,
  diagnosticsOnly,
}

enum VesperNativeFramePipelineMode {
  disabled,
  diagnosticsOnly,
  preferNativeFrame,
  requireNativeFrame,
}

final class VesperFrameProcessorConfiguration {
  const VesperFrameProcessorConfiguration({
    this.mode = VesperFrameProcessorMode.disabled,
    this.pluginLibraryPaths = const <String>[],
  });

  factory VesperFrameProcessorConfiguration.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperFrameProcessorConfiguration(
      mode: _decodeEnum(
        VesperFrameProcessorMode.values,
        map['mode'],
        VesperFrameProcessorMode.disabled,
      ),
      pluginLibraryPaths: _decodeStringList(map['pluginLibraryPaths']),
    );
  }

  final VesperFrameProcessorMode mode;
  final List<String> pluginLibraryPaths;

  bool get hasOverrides =>
      mode != VesperFrameProcessorMode.disabled ||
      pluginLibraryPaths.isNotEmpty;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'mode': mode.name,
      'pluginLibraryPaths': pluginLibraryPaths,
    };
  }
}

final class VesperNativeFramePipelineConfiguration {
  const VesperNativeFramePipelineConfiguration({
    this.mode = VesperNativeFramePipelineMode.disabled,
    this.decoderPluginLibraryPaths = const <String>[],
    this.frameProcessorPluginLibraryPaths = const <String>[],
    this.maxInFlightFrames,
  });

  factory VesperNativeFramePipelineConfiguration.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperNativeFramePipelineConfiguration(
      mode: _decodeEnum(
        VesperNativeFramePipelineMode.values,
        map['mode'],
        VesperNativeFramePipelineMode.disabled,
      ),
      decoderPluginLibraryPaths:
          _decodeStringList(map['decoderPluginLibraryPaths']),
      frameProcessorPluginLibraryPaths:
          _decodeStringList(map['frameProcessorPluginLibraryPaths']),
      maxInFlightFrames: (map['maxInFlightFrames'] as num?)?.toInt(),
    );
  }

  final VesperNativeFramePipelineMode mode;
  final List<String> decoderPluginLibraryPaths;
  final List<String> frameProcessorPluginLibraryPaths;
  final int? maxInFlightFrames;

  bool get hasOverrides =>
      mode != VesperNativeFramePipelineMode.disabled ||
      decoderPluginLibraryPaths.isNotEmpty ||
      frameProcessorPluginLibraryPaths.isNotEmpty ||
      maxInFlightFrames != null;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'mode': mode.name,
      'decoderPluginLibraryPaths': decoderPluginLibraryPaths,
      'frameProcessorPluginLibraryPaths': frameProcessorPluginLibraryPaths,
      if (maxInFlightFrames != null) 'maxInFlightFrames': maxInFlightFrames,
    };
  }
}
