part of '../models.dart';

enum VesperPluginTransport {
  native,
  wasm,
  unknown,
}

/// Explicit selection of one plugin transport and capability instance.
final class VesperPluginReference {
  factory VesperPluginReference({
    required String pluginId,
    String? capabilityInstanceId,
    required VesperPluginTransport transport,
    String? transportRawValue,
  }) {
    if (!_isValidPluginIdentity(pluginId)) {
      throw const FormatException(
        'pluginId must be a valid reverse-DNS identity',
      );
    }
    if (capabilityInstanceId != null &&
        !_isValidPluginIdentity(capabilityInstanceId)) {
      throw const FormatException(
        'capabilityInstanceId must be a valid reverse-DNS identity',
      );
    }
    if (transport == VesperPluginTransport.unknown) {
      if (transportRawValue == null || transportRawValue.isEmpty) {
        throw const FormatException(
          'transportRawValue is required for an unknown transport',
        );
      }
    } else if (transportRawValue != null) {
      throw const FormatException(
        'transportRawValue is only valid for an unknown transport',
      );
    }
    return VesperPluginReference._(
      pluginId: pluginId,
      capabilityInstanceId: capabilityInstanceId,
      transport: transport,
      transportRawValue: transportRawValue,
    );
  }

  factory VesperPluginReference.fromMap(Map<Object?, Object?> map) {
    final pluginId = map['pluginId'];
    final capabilityInstanceId = map['capabilityInstanceId'];
    final transportRawValue = map['transport'];
    if (pluginId is! String ||
        (capabilityInstanceId != null && capabilityInstanceId is! String) ||
        transportRawValue is! String ||
        transportRawValue.isEmpty) {
      throw const FormatException('invalid plugin reference');
    }
    final transport = switch (transportRawValue) {
      'native' => VesperPluginTransport.native,
      'wasm' => VesperPluginTransport.wasm,
      _ => VesperPluginTransport.unknown,
    };
    return VesperPluginReference(
      pluginId: pluginId,
      capabilityInstanceId: capabilityInstanceId as String?,
      transport: transport,
      transportRawValue:
          transport == VesperPluginTransport.unknown ? transportRawValue : null,
    );
  }

  const VesperPluginReference._({
    required this.pluginId,
    required this.capabilityInstanceId,
    required this.transport,
    required this.transportRawValue,
  });

  final String pluginId;
  final String? capabilityInstanceId;
  final VesperPluginTransport transport;
  final String? transportRawValue;

  String get transportWireValue => transportRawValue ?? transport.name;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'pluginId': pluginId,
      if (capabilityInstanceId != null)
        'capabilityInstanceId': capabilityInstanceId,
      'transport': transportWireValue,
    };
  }

  @override
  bool operator ==(Object other) {
    return other is VesperPluginReference &&
        other.pluginId == pluginId &&
        other.capabilityInstanceId == capabilityInstanceId &&
        other.transport == transport &&
        other.transportRawValue == transportRawValue;
  }

  @override
  int get hashCode => Object.hash(
        pluginId,
        capabilityInstanceId,
        transport,
        transportRawValue,
      );
}

/// Canonical references for plugins distributed with Vesper mobile host kits.
abstract final class VesperBundledPluginReferences {
  static final sourceNormalizerFfmpeg = VesperPluginReference(
    pluginId: 'io.github.umbrella22.vesper.source-normalizer-ffmpeg',
    transport: VesperPluginTransport.native,
  );

  static final remuxFfmpeg = VesperPluginReference(
    pluginId: 'io.github.umbrella22.vesper.remux-ffmpeg',
    transport: VesperPluginTransport.native,
  );

  static final decoderMediaCodec = VesperPluginReference(
    pluginId: 'io.github.umbrella22.vesper.decoder-mediacodec',
    transport: VesperPluginTransport.native,
  );

  static final decoderVideoToolbox = VesperPluginReference(
    pluginId: 'io.github.umbrella22.vesper.decoder-videotoolbox',
    transport: VesperPluginTransport.native,
  );

  static final frameProcessorDiagnostic = VesperPluginReference(
    pluginId: 'dev.vesper.frame-processor-diagnostic',
    transport: VesperPluginTransport.native,
  );
}

/// Native playback EventHook plugins selected for one player instance.
final class VesperPipelineEventHookConfiguration {
  const VesperPipelineEventHookConfiguration({
    this.pluginReferences = const <VesperPluginReference>[],
  });

  factory VesperPipelineEventHookConfiguration.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperPipelineEventHookConfiguration(
      pluginReferences: _decodePluginReferences(map['pluginReferences']),
    );
  }

  final List<VesperPluginReference> pluginReferences;

  bool get hasOverrides => pluginReferences.isNotEmpty;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'pluginReferences':
          pluginReferences.map((reference) => reference.toMap()).toList(),
    };
  }
}

bool _isValidPluginIdentity(String value) {
  if (value.isEmpty ||
      value.length > 255 ||
      !value.codeUnits.every((v) => v <= 0x7f)) {
    return false;
  }
  final segments = value.split('.');
  return segments.length >= 2 && segments.every(_isValidPluginIdentitySegment);
}

bool _isValidPluginIdentitySegment(String segment) {
  if (segment.isEmpty || !RegExp(r'^[a-z]').hasMatch(segment)) {
    return false;
  }
  return RegExp(r'^[a-z][a-z0-9-]*[a-z0-9]$').hasMatch(segment) ||
      RegExp(r'^[a-z]$').hasMatch(segment);
}

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
    this.pluginReferences = const <VesperPluginReference>[],
    this.runtimeProfile,
  });

  factory VesperSourceNormalizerConfiguration.preferBundled({
    String? runtimeProfile,
  }) =>
      VesperSourceNormalizerConfiguration(
        mode: VesperSourceNormalizerMode.preferNormalized,
        pluginReferences: <VesperPluginReference>[
          VesperBundledPluginReferences.sourceNormalizerFfmpeg,
        ],
        runtimeProfile: runtimeProfile,
      );

  factory VesperSourceNormalizerConfiguration.requireBundled({
    String? runtimeProfile,
  }) =>
      VesperSourceNormalizerConfiguration(
        mode: VesperSourceNormalizerMode.requireNormalized,
        pluginReferences: <VesperPluginReference>[
          VesperBundledPluginReferences.sourceNormalizerFfmpeg,
        ],
        runtimeProfile: runtimeProfile,
      );

  factory VesperSourceNormalizerConfiguration.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperSourceNormalizerConfiguration(
      mode: _decodeEnum(
        VesperSourceNormalizerMode.values,
        map['mode'],
        VesperSourceNormalizerMode.disabled,
      ),
      pluginReferences: _decodePluginReferences(map['pluginReferences']),
      runtimeProfile: map['runtimeProfile'] as String?,
    );
  }

  final VesperSourceNormalizerMode mode;
  final List<VesperPluginReference> pluginReferences;
  final String? runtimeProfile;

  bool get hasOverrides =>
      mode != VesperSourceNormalizerMode.disabled ||
      pluginReferences.isNotEmpty ||
      runtimeProfile != null;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'mode': mode.name,
      'pluginReferences':
          pluginReferences.map((reference) => reference.toMap()).toList(),
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
    this.pluginReferences = const <VesperPluginReference>[],
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
      pluginReferences: _decodePluginReferences(map['pluginReferences']),
    );
  }

  final VesperFrameProcessorMode mode;
  final List<VesperPluginReference> pluginReferences;

  bool get hasOverrides =>
      mode != VesperFrameProcessorMode.disabled || pluginReferences.isNotEmpty;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'mode': mode.name,
      'pluginReferences':
          pluginReferences.map((reference) => reference.toMap()).toList(),
    };
  }
}

final class VesperNativeFramePipelineConfiguration {
  const VesperNativeFramePipelineConfiguration({
    this.mode = VesperNativeFramePipelineMode.disabled,
    this.decoderPluginReferences = const <VesperPluginReference>[],
    this.frameProcessorPluginReferences = const <VesperPluginReference>[],
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
      decoderPluginReferences:
          _decodePluginReferences(map['decoderPluginReferences']),
      frameProcessorPluginReferences:
          _decodePluginReferences(map['frameProcessorPluginReferences']),
      maxInFlightFrames: (map['maxInFlightFrames'] as num?)?.toInt(),
    );
  }

  final VesperNativeFramePipelineMode mode;
  final List<VesperPluginReference> decoderPluginReferences;
  final List<VesperPluginReference> frameProcessorPluginReferences;
  final int? maxInFlightFrames;

  bool get hasOverrides =>
      mode != VesperNativeFramePipelineMode.disabled ||
      decoderPluginReferences.isNotEmpty ||
      frameProcessorPluginReferences.isNotEmpty ||
      maxInFlightFrames != null;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'mode': mode.name,
      'decoderPluginReferences': decoderPluginReferences
          .map((reference) => reference.toMap())
          .toList(),
      'frameProcessorPluginReferences': frameProcessorPluginReferences
          .map((reference) => reference.toMap())
          .toList(),
      if (maxInFlightFrames != null) 'maxInFlightFrames': maxInFlightFrames,
    };
  }
}
