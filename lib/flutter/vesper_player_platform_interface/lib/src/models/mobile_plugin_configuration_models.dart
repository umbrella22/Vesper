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
