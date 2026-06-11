part of '../models.dart';

final class VesperPlayerCapabilities {
  const VesperPlayerCapabilities({
    this.supportsLocalFiles = false,
    this.supportsRemoteUrls = false,
    this.supportsHls = false,
    this.supportsDash = false,
    this.supportsDashStaticVod = false,
    this.supportsDashDynamicLive = false,
    this.supportsDashManifestTrackCatalog = false,
    this.supportsDashTextTracks = false,
    this.supportsTrackCatalog = false,
    this.supportsTrackSelection = false,
    this.supportsVideoTrackSelection = false,
    this.supportsAudioTrackSelection = false,
    this.supportsSubtitleTrackSelection = false,
    this.supportsAbrPolicy = false,
    this.supportsAbrConstrained = false,
    this.supportsAbrFixedTrack = false,
    this.supportsExactAbrFixedTrack = false,
    this.supportsAbrMaxBitRate = false,
    this.supportsAbrMaxResolution = false,
    this.supportsResiliencePolicy = false,
    this.supportsHolePunch = false,
    this.supportsPlaybackRate = false,
    this.supportsLiveEdgeSeeking = false,
    this.isExperimental = false,
    this.supportedPlaybackRates = const <double>[],
  });

  const VesperPlayerCapabilities.unsupported()
      : supportsLocalFiles = false,
        supportsRemoteUrls = false,
        supportsHls = false,
        supportsDash = false,
        supportsDashStaticVod = false,
        supportsDashDynamicLive = false,
        supportsDashManifestTrackCatalog = false,
        supportsDashTextTracks = false,
        supportsTrackCatalog = false,
        supportsTrackSelection = false,
        supportsVideoTrackSelection = false,
        supportsAudioTrackSelection = false,
        supportsSubtitleTrackSelection = false,
        supportsAbrPolicy = false,
        supportsAbrConstrained = false,
        supportsAbrFixedTrack = false,
        supportsExactAbrFixedTrack = false,
        supportsAbrMaxBitRate = false,
        supportsAbrMaxResolution = false,
        supportsResiliencePolicy = false,
        supportsHolePunch = false,
        supportsPlaybackRate = false,
        supportsLiveEdgeSeeking = false,
        isExperimental = false,
        supportedPlaybackRates = const <double>[];

  factory VesperPlayerCapabilities.fromMap(Map<Object?, Object?> map) {
    final rawRates = map['supportedPlaybackRates'];
    final rawSupportsTrackSelection = _decodeOptionalBool(
      map,
      'supportsTrackSelection',
    );
    final supportsVideoTrackSelection =
        _decodeOptionalBool(map, 'supportsVideoTrackSelection') ?? false;
    final supportsAudioTrackSelection =
        _decodeOptionalBool(map, 'supportsAudioTrackSelection') ?? false;
    final supportsSubtitleTrackSelection =
        _decodeOptionalBool(map, 'supportsSubtitleTrackSelection') ?? false;
    final supportsTrackSelection = rawSupportsTrackSelection == true ||
        supportsVideoTrackSelection ||
        supportsAudioTrackSelection ||
        supportsSubtitleTrackSelection;

    final rawSupportsAbrPolicy = _decodeOptionalBool(map, 'supportsAbrPolicy');
    final supportsAbrConstrained =
        _decodeOptionalBool(map, 'supportsAbrConstrained') ?? false;
    final supportsAbrFixedTrack =
        _decodeOptionalBool(map, 'supportsAbrFixedTrack') ?? false;
    final supportsAbrPolicy = rawSupportsAbrPolicy == true ||
        supportsAbrConstrained ||
        supportsAbrFixedTrack;
    final supportsAbrMaxBitRate =
        _decodeOptionalBool(map, 'supportsAbrMaxBitRate') ?? false;
    final supportsAbrMaxResolution =
        _decodeOptionalBool(map, 'supportsAbrMaxResolution') ?? false;
    final supportsDashStaticVod =
        _decodeOptionalBool(map, 'supportsDashStaticVod') ?? false;
    final supportsDashDynamicLive =
        _decodeOptionalBool(map, 'supportsDashDynamicLive') ?? false;
    final supportsDashManifestTrackCatalog =
        _decodeOptionalBool(map, 'supportsDashManifestTrackCatalog') ?? false;
    final supportsDashTextTracks =
        _decodeOptionalBool(map, 'supportsDashTextTracks') ?? false;
    final supportsDash = _decodeBool(map, 'supportsDash') ||
        supportsDashStaticVod ||
        supportsDashDynamicLive ||
        supportsDashManifestTrackCatalog ||
        supportsDashTextTracks;

    return VesperPlayerCapabilities(
      supportsLocalFiles: _decodeBool(map, 'supportsLocalFiles'),
      supportsRemoteUrls: _decodeBool(map, 'supportsRemoteUrls'),
      supportsHls: _decodeBool(map, 'supportsHls'),
      supportsDash: supportsDash,
      supportsDashStaticVod: supportsDashStaticVod,
      supportsDashDynamicLive: supportsDashDynamicLive,
      supportsDashManifestTrackCatalog: supportsDashManifestTrackCatalog,
      supportsDashTextTracks: supportsDashTextTracks,
      supportsTrackCatalog: _decodeBool(map, 'supportsTrackCatalog'),
      supportsTrackSelection: supportsTrackSelection,
      supportsVideoTrackSelection: supportsVideoTrackSelection,
      supportsAudioTrackSelection: supportsAudioTrackSelection,
      supportsSubtitleTrackSelection: supportsSubtitleTrackSelection,
      supportsAbrPolicy: supportsAbrPolicy,
      supportsAbrConstrained: supportsAbrConstrained,
      supportsAbrFixedTrack: supportsAbrFixedTrack,
      supportsExactAbrFixedTrack:
          _decodeOptionalBool(map, 'supportsExactAbrFixedTrack') ?? false,
      supportsAbrMaxBitRate: supportsAbrMaxBitRate,
      supportsAbrMaxResolution: supportsAbrMaxResolution,
      supportsResiliencePolicy: _decodeBool(map, 'supportsResiliencePolicy'),
      supportsHolePunch: _decodeBool(map, 'supportsHolePunch'),
      supportsPlaybackRate: _decodeBool(map, 'supportsPlaybackRate'),
      supportsLiveEdgeSeeking: _decodeBool(map, 'supportsLiveEdgeSeeking'),
      isExperimental: _decodeBool(map, 'isExperimental'),
      supportedPlaybackRates: rawRates is Iterable
          ? rawRates
              .map((value) => value is num ? value.toDouble() : null)
              .whereType<double>()
              .toList(growable: false)
          : const <double>[],
    );
  }

  final bool supportsLocalFiles;
  final bool supportsRemoteUrls;
  final bool supportsHls;
  final bool supportsDash;
  final bool supportsDashStaticVod;
  final bool supportsDashDynamicLive;
  final bool supportsDashManifestTrackCatalog;
  final bool supportsDashTextTracks;
  final bool supportsTrackCatalog;
  final bool supportsTrackSelection;
  final bool supportsVideoTrackSelection;
  final bool supportsAudioTrackSelection;
  final bool supportsSubtitleTrackSelection;
  final bool supportsAbrPolicy;
  final bool supportsAbrConstrained;
  final bool supportsAbrFixedTrack;
  final bool supportsExactAbrFixedTrack;
  final bool supportsAbrMaxBitRate;
  final bool supportsAbrMaxResolution;
  final bool supportsResiliencePolicy;
  final bool supportsHolePunch;
  final bool supportsPlaybackRate;
  final bool supportsLiveEdgeSeeking;
  final bool isExperimental;
  final List<double> supportedPlaybackRates;

  bool supportsTrackSelectionFor(VesperMediaTrackKind kind) {
    return switch (kind) {
      VesperMediaTrackKind.video => supportsVideoTrackSelection,
      VesperMediaTrackKind.audio => supportsAudioTrackSelection,
      VesperMediaTrackKind.subtitle => supportsSubtitleTrackSelection,
    };
  }

  bool supportsAbrMode(VesperAbrMode mode) {
    return switch (mode) {
      VesperAbrMode.auto => supportsAbrPolicy,
      VesperAbrMode.constrained => supportsAbrConstrained,
      VesperAbrMode.fixedTrack => supportsAbrFixedTrack,
    };
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'supportsLocalFiles': supportsLocalFiles,
      'supportsRemoteUrls': supportsRemoteUrls,
      'supportsHls': supportsHls,
      'supportsDash': supportsDash,
      'supportsDashStaticVod': supportsDashStaticVod,
      'supportsDashDynamicLive': supportsDashDynamicLive,
      'supportsDashManifestTrackCatalog': supportsDashManifestTrackCatalog,
      'supportsDashTextTracks': supportsDashTextTracks,
      'supportsTrackCatalog': supportsTrackCatalog,
      'supportsTrackSelection': supportsTrackSelection,
      'supportsVideoTrackSelection': supportsVideoTrackSelection,
      'supportsAudioTrackSelection': supportsAudioTrackSelection,
      'supportsSubtitleTrackSelection': supportsSubtitleTrackSelection,
      'supportsAbrPolicy': supportsAbrPolicy,
      'supportsAbrConstrained': supportsAbrConstrained,
      'supportsAbrFixedTrack': supportsAbrFixedTrack,
      'supportsExactAbrFixedTrack': supportsExactAbrFixedTrack,
      'supportsAbrMaxBitRate': supportsAbrMaxBitRate,
      'supportsAbrMaxResolution': supportsAbrMaxResolution,
      'supportsResiliencePolicy': supportsResiliencePolicy,
      'supportsHolePunch': supportsHolePunch,
      'supportsPlaybackRate': supportsPlaybackRate,
      'supportsLiveEdgeSeeking': supportsLiveEdgeSeeking,
      'isExperimental': isExperimental,
      'supportedPlaybackRates': supportedPlaybackRates,
    };
  }
}

final class VesperPlaybackCapabilityProbeRequest {
  const VesperPlaybackCapabilityProbeRequest({
    this.source,
    this.codec,
    this.width,
    this.height,
    this.frameRate,
    this.requiresNativeFrame = false,
    this.sourceNormalizerConfiguration =
        const VesperSourceNormalizerConfiguration(),
    this.frameProcessorConfiguration =
        const VesperFrameProcessorConfiguration(),
    this.nativeFramePipelineConfiguration =
        const VesperNativeFramePipelineConfiguration(),
  });

  factory VesperPlaybackCapabilityProbeRequest.fromMap(
    Map<Object?, Object?> map,
  ) {
    final rawSource = vesperDecodeMap(map['source']);
    return VesperPlaybackCapabilityProbeRequest(
      source: rawSource.isEmpty ? null : VesperPlayerSource.fromMap(rawSource),
      codec: map['codec'] as String?,
      width: _decodeInt(map, 'width'),
      height: _decodeInt(map, 'height'),
      frameRate: _decodeDouble(map, 'frameRate'),
      requiresNativeFrame: _decodeBool(map, 'requiresNativeFrame'),
      sourceNormalizerConfiguration:
          VesperSourceNormalizerConfiguration.fromMap(
        vesperDecodeMap(map['sourceNormalizer']),
      ),
      frameProcessorConfiguration: VesperFrameProcessorConfiguration.fromMap(
        vesperDecodeMap(map['frameProcessor']),
      ),
      nativeFramePipelineConfiguration:
          VesperNativeFramePipelineConfiguration.fromMap(
        vesperDecodeMap(map['nativeFramePipeline']),
      ),
    );
  }

  final VesperPlayerSource? source;
  final String? codec;
  final int? width;
  final int? height;
  final double? frameRate;
  final bool requiresNativeFrame;
  final VesperSourceNormalizerConfiguration sourceNormalizerConfiguration;
  final VesperFrameProcessorConfiguration frameProcessorConfiguration;
  final VesperNativeFramePipelineConfiguration nativeFramePipelineConfiguration;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'source': source?.toMap(),
      'codec': codec,
      'width': width,
      'height': height,
      'frameRate': frameRate,
      'requiresNativeFrame': requiresNativeFrame,
      if (sourceNormalizerConfiguration.hasOverrides)
        'sourceNormalizer': sourceNormalizerConfiguration.toMap(),
      if (frameProcessorConfiguration.hasOverrides)
        'frameProcessor': frameProcessorConfiguration.toMap(),
      if (nativeFramePipelineConfiguration.hasOverrides)
        'nativeFramePipeline': nativeFramePipelineConfiguration.toMap(),
    };
  }
}
