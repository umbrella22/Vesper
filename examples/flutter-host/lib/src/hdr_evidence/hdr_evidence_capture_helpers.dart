part of 'hdr_evidence_capture.dart';

Map<String, Object?> _probeResult(VesperPlaybackCapabilityProbeResult probe) {
  return <String, Object?>{
    'status': probe.status.name,
    'recommendedPlaybackPath': probe.recommendedPlaybackPath.name,
    'confidence': probe.confidence.name,
    'hdrKind': probe.hdrKind.name,
    'missingCapabilities': probe.missingCapabilities,
    'hdrMetadata': probe.hdrMetadata?.toMap() ?? const <String, Object?>{},
  };
}

Map<String, Object?> _warningResult(VesperCapabilityWarning? warning) {
  if (warning == null) {
    return const <String, Object?>{
      'reason': null,
      'recommendedPlaybackPath': null,
      'hdrKind': null,
      'likelyHdrCapabilityIssue': false,
      'confidence': null,
      'errorCode': null,
      'capabilityFailureCause': null,
      'capabilityFailureAxis': null,
      'hdrMetadata': <String, Object?>{},
      'diagnostics': <String, Object?>{},
      'message': null,
    };
  }
  return <String, Object?>{
    'reason': warning.reason.name,
    'recommendedPlaybackPath': warning.recommendedPlaybackPath.name,
    'hdrKind': warning.hdrKind.name,
    'likelyHdrCapabilityIssue': warning.likelyHdrCapabilityIssue,
    'confidence': warning.confidence,
    'errorCode': warning.errorCode,
    'capabilityFailureCause': warning.capabilityFailureCause,
    'capabilityFailureAxis': warning.capabilityFailureAxis,
    'hdrMetadata': warning.hdrMetadata?.toMap() ?? const <String, Object?>{},
    'diagnostics': warning.diagnostics,
    'message': warning.message,
  };
}

Map<String, Object?> _groupProbeDiagnostics(Map<String, Object?> diagnostics) {
  return <String, Object?>{
    'display': _matchingDiagnostics(diagnostics, <String>[
      'display',
      'avPlayer',
      'requestedFrameRate',
    ]),
    'codecFormat': _matchingDiagnostics(diagnostics, <String>['codecFormat']),
    'asset': _matchingDiagnostics(diagnostics, <String>['asset']),
    'dolbyVision': _matchingDiagnostics(diagnostics, <String>['dolbyVision']),
    'other': _otherDiagnostics(diagnostics, <String>[
      'display',
      'avPlayer',
      'requestedFrameRate',
      'codecFormat',
      'asset',
      'dolbyVision',
    ]),
  };
}

Map<String, Object?> _matchingDiagnostics(
  Map<String, Object?> diagnostics,
  List<String> needles,
) {
  return Map<String, Object?>.fromEntries(
    diagnostics.entries.where(
      (entry) => needles.any((needle) => entry.key.startsWith(needle)),
    ),
  );
}

Map<String, Object?> _otherDiagnostics(
  Map<String, Object?> diagnostics,
  List<String> groupedNeedles,
) {
  return Map<String, Object?>.fromEntries(
    diagnostics.entries.where(
      (entry) => !groupedNeedles.any((needle) => entry.key.startsWith(needle)),
    ),
  );
}

Map<String, Object?> _androidRuntime(Map<String, Object?> details) {
  return <String, Object?>{
    'playbackExceptionErrorCode': details['errorCode'],
    'capabilityFailureCause': details['capabilityFailureCause'],
    'capabilityFailureAxis': details['capabilityFailureAxis'],
    'playbackFailureCauseClass': details['playbackFailureCauseClass'],
    'playbackFailureCauseMessage': details['playbackFailureCauseMessage'],
    'playbackFailureRootCauseClass': details['playbackFailureRootCauseClass'],
    'playbackFailureRootCauseMessage':
        details['playbackFailureRootCauseMessage'],
    'rendererName': details['playbackFailureRendererName'],
    'rendererIndex': details['playbackFailureRendererIndex'],
    'rendererFormatSupport': details['playbackFailureRendererFormatSupport'],
    'rendererFormatSampleMimeType':
        details['playbackFailureRendererFormatSampleMimeType'],
    'rendererFormatCodecs': details['playbackFailureRendererFormatCodecs'],
    'rendererFormatWidth': details['playbackFailureRendererFormatWidth'],
    'rendererFormatHeight': details['playbackFailureRendererFormatHeight'],
    'rendererFormatFrameRate':
        details['playbackFailureRendererFormatFrameRate'],
    'rendererFormatSupported':
        details['playbackFailureRendererFormatSupported'],
    'rendererFormatMimeMatchesRuntime':
        details['playbackFailureRendererFormatMimeMatchesRuntime'],
    'rendererFormatCodecsMatchRuntime':
        details['playbackFailureRendererFormatCodecsMatchRuntime'],
    'rendererFormatSizeMatchesRuntime':
        details['playbackFailureRendererFormatSizeMatchesRuntime'],
    'rendererFormatFrameRateMatchesRuntime':
        details['playbackFailureRendererFormatFrameRateMatchesRuntime'],
    'runtimeSessionProbeStatus': details['runtimeSessionProbeStatus'],
    'runtimeSessionProbeRecommendedPlaybackPath':
        details['runtimeSessionProbeRecommendedPlaybackPath'],
    'runtimeSessionProbeConfidence': details['runtimeSessionProbeConfidence'],
    'runtimeSessionProbeHdrKind': details['runtimeSessionProbeHdrKind'],
    'runtimeSessionProbeDolbyVisionMode':
        details['runtimeSessionProbeDolbyVisionMode'],
    'runtimeSessionProbeMissingCapabilities':
        details['runtimeSessionProbeMissingCapabilities'],
    'runtimeSessionProbeCodecFormatSupported':
        details['runtimeSessionProbeCodecFormatSupported'],
    'runtimeSessionProbeCodecFormatMissingCapability':
        details['runtimeSessionProbeCodecFormatMissingCapability'],
    'runtimeSessionProbeCodecFormatSampleMimeType':
        details['runtimeSessionProbeCodecFormatSampleMimeType'],
    'runtimeSessionProbeCodecFormatCodecs':
        details['runtimeSessionProbeCodecFormatCodecs'],
    'runtimeSessionProbeCodecFormatWidth':
        details['runtimeSessionProbeCodecFormatWidth'],
    'runtimeSessionProbeCodecFormatHeight':
        details['runtimeSessionProbeCodecFormatHeight'],
    'runtimeSessionProbeCodecFormatFrameRate':
        details['runtimeSessionProbeCodecFormatFrameRate'],
    'runtimeSessionProbeDisplayHdrSupported':
        details['runtimeSessionProbeDisplayHdrSupported'],
    'runtimeSessionProbeDisplayFrameRateSupported':
        details['runtimeSessionProbeDisplayFrameRateSupported'],
    'runtimeSessionProbeCodecFormatMimeMatchesRuntime':
        details['runtimeSessionProbeCodecFormatMimeMatchesRuntime'],
    'runtimeSessionProbeCodecFormatCodecsMatchRuntime':
        details['runtimeSessionProbeCodecFormatCodecsMatchRuntime'],
    'runtimeSessionProbeCodecFormatSizeMatchesRuntime':
        details['runtimeSessionProbeCodecFormatSizeMatchesRuntime'],
    'runtimeSessionProbeCodecFormatFrameRateMatchesRuntime':
        details['runtimeSessionProbeCodecFormatFrameRateMatchesRuntime'],
    'rawPayloadKeys': <String, Object?>{
      'playbackFailureRendererFormatSupported':
          details['playbackFailureRendererFormatSupported'],
      'playbackFailureRendererFormatMimeMatchesRuntime':
          details['playbackFailureRendererFormatMimeMatchesRuntime'],
      'playbackFailureRendererFormatCodecsMatchRuntime':
          details['playbackFailureRendererFormatCodecsMatchRuntime'],
      'playbackFailureRendererFormatSizeMatchesRuntime':
          details['playbackFailureRendererFormatSizeMatchesRuntime'],
      'playbackFailureRendererFormatFrameRateMatchesRuntime':
          details['playbackFailureRendererFormatFrameRateMatchesRuntime'],
    },
  };
}

Map<String, Object?> _iosRuntime(Map<String, Object?> details) {
  return <String, Object?>{
    'avErrorCode': details['avErrorCode'],
    'nsErrorDomain': details['nsErrorDomain'],
    'nsErrorCode': details['nsErrorCode'],
    'iosRuntimeEvidenceSource': details['iosRuntimeEvidenceSource'],
    'iosRuntimeFailureCategory': details['iosRuntimeFailureCategory'],
    'iosRuntimeFailureRetriable': details['iosRuntimeFailureRetriable'],
    'iosRuntimeFailureCode': details['iosRuntimeFailureCode'],
    'capabilityFailureCause': details['capabilityFailureCause'],
    'missingCapabilities': details['missingCapabilities'],
    'sessionProbe': details['sessionProbe'],
    'displayHdrProbeAvailable': details['displayHdrProbeAvailable'],
    'displayHdrSupported': details['displayHdrSupported'],
    'displayGamut': details['displayGamut'],
    'avPlayerEligibleForHDRPlayback': details['avPlayerEligibleForHDRPlayback'],
    'hdrKindSupportBasis': details['hdrKindSupportBasis'],
    'displayFrameRateSupported': details['displayFrameRateSupported'],
    'displayMaximumFramesPerSecond': details['displayMaximumFramesPerSecond'],
    'displayNativeWidth': details['displayNativeWidth'],
    'displayNativeHeight': details['displayNativeHeight'],
    'requestedWidth': details['requestedWidth'],
    'requestedHeight': details['requestedHeight'],
    'requestedFrameRate': details['requestedFrameRate'],
    'avPlayerItemStatusEvidenceSource':
        details['avPlayerItemStatusEvidenceSource'],
    'avPlayerItemStatus': details['avPlayerItemStatus'],
    'avPlayerItemErrorLogEvidenceSource':
        details['avPlayerItemErrorLogEvidenceSource'],
    'avPlayerItemErrorLogEventCount': details['avPlayerItemErrorLogEventCount'],
    'avPlayerItemErrorLogRecentEventCount':
        details['avPlayerItemErrorLogRecentEventCount'],
    'avPlayerItemErrorLogEvents': details['avPlayerItemErrorLogEvents'],
    'avPlayerItemErrorStatusCode': details['avPlayerItemErrorStatusCode'],
    'avPlayerItemErrorDomain': details['avPlayerItemErrorDomain'],
    'avPlayerItemErrorComment': details['avPlayerItemErrorComment'],
  };
}

Object? _jsonValue(Object? value) {
  if (value == null || value is num || value is bool || value is String) {
    return value;
  }
  if (value is Iterable) {
    return value.map(_jsonValue).toList(growable: false);
  }
  if (value is Map) {
    return <String, Object?>{
      for (final entry in value.entries)
        entry.key.toString(): _jsonValue(entry.value),
    };
  }
  return value.toString();
}

Map<String, Object?> _mergeMaps(
  Map<String, Object?> defaults,
  Map<String, Object?> overrides,
) {
  final result = Map<String, Object?>.of(defaults);
  for (final entry in overrides.entries) {
    final base = result[entry.key];
    final override = entry.value;
    if (base is Map<String, Object?> && override is Map) {
      result[entry.key] = _mergeMaps(base, <String, Object?>{
        for (final overrideEntry in override.entries)
          overrideEntry.key.toString(): overrideEntry.value,
      });
    } else {
      result[entry.key] = override;
    }
  }
  return result;
}

int? _intValue(Object? value) {
  if (value is int) {
    return value;
  }
  if (value is num) {
    return value.toInt();
  }
  if (value is String) {
    return int.tryParse(value);
  }
  return null;
}

double? _doubleValue(Object? value) {
  if (value is double) {
    return value;
  }
  if (value is num) {
    return value.toDouble();
  }
  if (value is String) {
    return double.tryParse(value);
  }
  return null;
}

String _playbackOutcome(
  VesperPlayerError? error,
  VesperCapabilityWarning? warning,
) {
  if (error != null) {
    return 'failure';
  }
  if (warning?.recommendedPlaybackPath ==
      VesperRecommendedPlaybackPath.systemPlayer) {
    return 'fallback';
  }
  return 'success';
}

String _sourceKindFor(VesperPlayerSource source) {
  return switch (source.protocol) {
    VesperPlayerSourceProtocol.file ||
    VesperPlayerSourceProtocol.content => 'file',
    VesperPlayerSourceProtocol.hls => 'hls',
    VesperPlayerSourceProtocol.dash => 'progressive',
    VesperPlayerSourceProtocol.progressive ||
    VesperPlayerSourceProtocol.rtmp ||
    VesperPlayerSourceProtocol.rtsp ||
    VesperPlayerSourceProtocol.flv => 'progressive',
    VesperPlayerSourceProtocol.unknown =>
      source.kind == VesperPlayerSourceKind.local ? 'file' : 'progressive',
  };
}

String _manifestKindFor(VesperPlayerSource source) {
  return switch (source.protocol) {
    VesperPlayerSourceProtocol.hls => 'hls',
    VesperPlayerSourceProtocol.dash => 'dash',
    _ => 'none',
  };
}
