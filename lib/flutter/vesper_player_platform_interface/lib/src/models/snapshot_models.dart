part of '../models.dart';

final class VesperPlayerSnapshot {
  const VesperPlayerSnapshot({
    required this.title,
    required this.subtitle,
    required this.sourceLabel,
    required this.playbackState,
    required this.playbackRate,
    required this.isBuffering,
    required this.isInterrupted,
    required this.hasVideoSurface,
    required this.timeline,
    this.viewport,
    this.viewportHint = const VesperViewportHint.hidden(),
    this.backendFamily = VesperPlayerBackendFamily.unknown,
    this.capabilities = const VesperPlayerCapabilities.unsupported(),
    this.trackCatalog = const VesperTrackCatalog(),
    this.trackSelection = const VesperTrackSelectionSnapshot(),
    this.effectiveVideoTrackId,
    this.videoVariantObservation,
    this.fixedTrackStatus,
    this.resiliencePolicy = const VesperPlaybackResiliencePolicy(),
    this.pluginDiagnostics = const <VesperPluginDiagnostic>[],
    this.lastError,
    this.subtitleState = VesperSubtitleState.empty,
  });

  const VesperPlayerSnapshot.initial()
      : title = 'Vesper',
        subtitle = 'Player is not initialized.',
        sourceLabel = '',
        playbackState = VesperPlaybackState.ready,
        playbackRate = 1.0,
        isBuffering = false,
        isInterrupted = false,
        hasVideoSurface = false,
        timeline = const VesperTimeline.initial(),
        viewport = null,
        viewportHint = const VesperViewportHint.hidden(),
        backendFamily = VesperPlayerBackendFamily.unknown,
        capabilities = const VesperPlayerCapabilities.unsupported(),
        trackCatalog = const VesperTrackCatalog(),
        trackSelection = const VesperTrackSelectionSnapshot(),
        effectiveVideoTrackId = null,
        videoVariantObservation = null,
        fixedTrackStatus = null,
        resiliencePolicy = const VesperPlaybackResiliencePolicy(),
        pluginDiagnostics = const <VesperPluginDiagnostic>[],
        lastError = null,
        subtitleState = VesperSubtitleState.empty;

  factory VesperPlayerSnapshot.fromMap(Map<Object?, Object?> map) {
    final rawTimeline = map['timeline'];
    final rawCapabilities = map['capabilities'];
    final rawTrackCatalog = map['trackCatalog'];
    final rawTrackSelection = map['trackSelection'];
    final rawEffectiveVideoTrackId = map['effectiveVideoTrackId'];
    final rawVideoVariantObservation = map['videoVariantObservation'];
    final rawFixedTrackStatus = map['fixedTrackStatus'];
    final rawResiliencePolicy = map['resiliencePolicy'];
    final rawPluginDiagnostics = map['pluginDiagnostics'];
    final rawViewport = map['viewport'];
    final rawViewportHint = map['viewportHint'];
    final rawLastError = map['lastError'];
    final rawSubtitleState = map['subtitleState'];
    final timeline = _rawMap(rawTimeline);
    final viewport = _rawMap(rawViewport);
    final viewportHint = _rawMap(rawViewportHint);
    final capabilities = _rawMap(rawCapabilities);
    final trackCatalog = _rawMap(rawTrackCatalog);
    final trackSelection = _rawMap(rawTrackSelection);
    final videoVariantObservation = _rawMap(rawVideoVariantObservation);
    final resiliencePolicy = _rawMap(rawResiliencePolicy);
    final lastError = _rawMap(rawLastError);
    final subtitleStateWire = _rawMap(rawSubtitleState);
    return VesperPlayerSnapshot(
      title: map['title'] as String? ?? 'Vesper',
      subtitle: map['subtitle'] as String? ?? '',
      sourceLabel: map['sourceLabel'] as String? ?? '',
      playbackState: _decodeEnum(
        VesperPlaybackState.values,
        map['playbackState'],
        VesperPlaybackState.ready,
      ),
      playbackRate: _decodeDouble(map, 'playbackRate') ?? 1.0,
      isBuffering: _decodeBool(map, 'isBuffering'),
      isInterrupted: _decodeBool(map, 'isInterrupted'),
      hasVideoSurface: _decodeBool(map, 'hasVideoSurface'),
      timeline: timeline != null
          ? VesperTimeline.fromMap(timeline)
          : const VesperTimeline.initial(),
      viewport:
          viewport != null ? VesperPlayerViewport.fromMap(viewport) : null,
      viewportHint: viewportHint != null
          ? VesperViewportHint.fromMap(viewportHint)
          : const VesperViewportHint.hidden(),
      backendFamily: _decodeEnum(
        VesperPlayerBackendFamily.values,
        map['backendFamily'],
        VesperPlayerBackendFamily.unknown,
      ),
      capabilities: capabilities != null
          ? VesperPlayerCapabilities.fromMap(capabilities)
          : const VesperPlayerCapabilities.unsupported(),
      trackCatalog: trackCatalog != null
          ? VesperTrackCatalog.fromMap(trackCatalog)
          : const VesperTrackCatalog(),
      trackSelection: trackSelection != null
          ? VesperTrackSelectionSnapshot.fromMap(trackSelection)
          : const VesperTrackSelectionSnapshot(),
      effectiveVideoTrackId: rawEffectiveVideoTrackId as String?,
      videoVariantObservation: videoVariantObservation != null
          ? VesperVideoVariantObservation.fromMap(
              videoVariantObservation,
            )
          : null,
      fixedTrackStatus: rawFixedTrackStatus is String
          ? _decodeEnum(
              VesperFixedTrackStatus.values,
              rawFixedTrackStatus,
              VesperFixedTrackStatus.pending,
            )
          : null,
      resiliencePolicy: resiliencePolicy != null
          ? VesperPlaybackResiliencePolicy.fromMap(
              resiliencePolicy,
            )
          : const VesperPlaybackResiliencePolicy(),
      pluginDiagnostics: rawPluginDiagnostics is Iterable
          ? rawPluginDiagnostics
              .map((value) => VesperPluginDiagnostic.fromMap(
                    _rawMap(value) ?? const <Object?, Object?>{},
                  ))
              .toList(growable: false)
          : const <VesperPluginDiagnostic>[],
      lastError:
          lastError != null ? VesperPlayerError.fromMap(lastError) : null,
      subtitleState: subtitleStateWire != null
          ? VesperSubtitleState.fromMap(subtitleStateWire)
          : VesperSubtitleState.empty,
    );
  }

  final String title;
  final String subtitle;
  final String sourceLabel;
  final VesperPlaybackState playbackState;
  final double playbackRate;
  final bool isBuffering;
  final bool isInterrupted;
  final bool hasVideoSurface;
  final VesperTimeline timeline;
  final VesperPlayerViewport? viewport;
  final VesperViewportHint viewportHint;
  final VesperPlayerBackendFamily backendFamily;
  final VesperPlayerCapabilities capabilities;
  final VesperTrackCatalog trackCatalog;
  final VesperTrackSelectionSnapshot trackSelection;
  final String? effectiveVideoTrackId;
  final VesperVideoVariantObservation? videoVariantObservation;
  final VesperFixedTrackStatus? fixedTrackStatus;
  final VesperPlaybackResiliencePolicy resiliencePolicy;
  final List<VesperPluginDiagnostic> pluginDiagnostics;
  final VesperPlayerError? lastError;

  /// Subtitle lifecycle state (loading / ready / failed) shared by iOS and
  /// Android host kits. Older host payloads that omit this field decode
  /// to [VesperSubtitleState.empty] so the snapshot remains decodable.
  final VesperSubtitleState subtitleState;

  VesperPlayerSnapshot copyWith({
    String? title,
    String? subtitle,
    String? sourceLabel,
    VesperPlaybackState? playbackState,
    double? playbackRate,
    bool? isBuffering,
    bool? isInterrupted,
    bool? hasVideoSurface,
    VesperTimeline? timeline,
    VesperPlayerViewport? viewport,
    VesperViewportHint? viewportHint,
    VesperPlayerBackendFamily? backendFamily,
    VesperPlayerCapabilities? capabilities,
    VesperTrackCatalog? trackCatalog,
    VesperTrackSelectionSnapshot? trackSelection,
    String? effectiveVideoTrackId,
    bool clearEffectiveVideoTrackId = false,
    VesperVideoVariantObservation? videoVariantObservation,
    bool clearVideoVariantObservation = false,
    VesperFixedTrackStatus? fixedTrackStatus,
    bool clearFixedTrackStatus = false,
    VesperPlaybackResiliencePolicy? resiliencePolicy,
    List<VesperPluginDiagnostic>? pluginDiagnostics,
    VesperPlayerError? lastError,
    bool clearLastError = false,
    VesperSubtitleState? subtitleState,
  }) {
    return VesperPlayerSnapshot(
      title: title ?? this.title,
      subtitle: subtitle ?? this.subtitle,
      sourceLabel: sourceLabel ?? this.sourceLabel,
      playbackState: playbackState ?? this.playbackState,
      playbackRate: playbackRate ?? this.playbackRate,
      isBuffering: isBuffering ?? this.isBuffering,
      isInterrupted: isInterrupted ?? this.isInterrupted,
      hasVideoSurface: hasVideoSurface ?? this.hasVideoSurface,
      timeline: timeline ?? this.timeline,
      viewport: viewport ?? this.viewport,
      viewportHint: viewportHint ?? this.viewportHint,
      backendFamily: backendFamily ?? this.backendFamily,
      capabilities: capabilities ?? this.capabilities,
      trackCatalog: trackCatalog ?? this.trackCatalog,
      trackSelection: trackSelection ?? this.trackSelection,
      effectiveVideoTrackId: clearEffectiveVideoTrackId
          ? null
          : (effectiveVideoTrackId ?? this.effectiveVideoTrackId),
      videoVariantObservation: clearVideoVariantObservation
          ? null
          : (videoVariantObservation ?? this.videoVariantObservation),
      fixedTrackStatus: clearFixedTrackStatus
          ? null
          : (fixedTrackStatus ?? this.fixedTrackStatus),
      resiliencePolicy: resiliencePolicy ?? this.resiliencePolicy,
      pluginDiagnostics: pluginDiagnostics ?? this.pluginDiagnostics,
      lastError: clearLastError ? null : (lastError ?? this.lastError),
      subtitleState: subtitleState ?? this.subtitleState,
    );
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'title': title,
      'subtitle': subtitle,
      'sourceLabel': sourceLabel,
      'playbackState': playbackState.name,
      'playbackRate': playbackRate,
      'isBuffering': isBuffering,
      'isInterrupted': isInterrupted,
      'hasVideoSurface': hasVideoSurface,
      'timeline': timeline.toMap(),
      'viewport': viewport?.toMap(),
      'viewportHint': viewportHint.toMap(),
      'backendFamily': backendFamily.name,
      'capabilities': capabilities.toMap(),
      'trackCatalog': trackCatalog.toMap(),
      'trackSelection': trackSelection.toMap(),
      'effectiveVideoTrackId': effectiveVideoTrackId,
      'videoVariantObservation': videoVariantObservation?.toMap(),
      'fixedTrackStatus': fixedTrackStatus?.name,
      'resiliencePolicy': resiliencePolicy.toMap(),
      'pluginDiagnostics':
          pluginDiagnostics.map((diagnostic) => diagnostic.toMap()).toList(),
      'lastError': lastError?.toMap(),
      'subtitleState': subtitleState.toMap(),
    };
  }
}
