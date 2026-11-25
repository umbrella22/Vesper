part of 'example_player_helpers.dart';

String qualityAutoRowTitle(
  VesperAbrPolicy _, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  return strings[ExampleHostText.auto];
}

String? qualityAutoRowBadgeLabel(
  VesperAbrPolicy abrPolicy, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  return switch (abrPolicy.mode) {
    VesperAbrMode.constrained => strings[ExampleHostText.qualityLimited],
    _ => null,
  };
}

String qualityAutoRowSubtitle(
  VesperTrackCatalog trackCatalog,
  VesperTrackSelectionSnapshot trackSelection,
  String? effectiveVideoTrackId,
  VesperFixedTrackStatus? fixedTrackStatus,
  VesperVideoVariantObservation? videoVariantObservation, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final abrPolicy = trackSelection.abrPolicy;
  final effectiveTrack = effectiveVideoTrack(
    trackCatalog,
    effectiveVideoTrackId,
  );
  final requestedTrack = requestedFixedVideoTrack(trackCatalog, trackSelection);
  final resolvedFixedTrackStatus = currentFixedTrackStatus(
    trackCatalog,
    trackSelection,
    effectiveVideoTrackId,
    fixedTrackStatus,
  );
  final constraintSummary = abrConstraintSummary(abrPolicy, strings: strings);
  final observationSummary = videoVariantObservationSummary(
    videoVariantObservation,
  );

  final lead = switch (abrPolicy.mode) {
    VesperAbrMode.auto => strings[ExampleHostText.qualityLeadAuto],
    VesperAbrMode.constrained when constraintSummary != null => strings.format(
      ExampleHostText.qualityLeadConstrainedWithin,
      <String, Object?>{'constraints': constraintSummary},
    ),
    VesperAbrMode.constrained =>
      strings[ExampleHostText.qualityLeadConstrained],
    VesperAbrMode.fixedTrack
        when requestedTrack != null &&
            resolvedFixedTrackStatus == VesperFixedTrackStatus.fallback &&
            effectiveTrack != null =>
      strings.format(ExampleHostText.qualityLeadFallback, <String, Object?>{
        'requested': qualityLabel(requestedTrack, strings: strings),
        'effective': qualityLabel(effectiveTrack, strings: strings),
      }),
    VesperAbrMode.fixedTrack
        when requestedTrack != null &&
            resolvedFixedTrackStatus == VesperFixedTrackStatus.pending =>
      strings.format(ExampleHostText.qualityLeadPending, <String, Object?>{
        'requested': qualityLabel(requestedTrack, strings: strings),
      }),
    VesperAbrMode.fixedTrack
        when requestedTrack != null && effectiveTrack != null =>
      strings.format(ExampleHostText.qualityLeadLocked, <String, Object?>{
        'requested': qualityLabel(requestedTrack, strings: strings),
      }),
    VesperAbrMode.fixedTrack when requestedTrack != null => strings.format(
      ExampleHostText.qualityLeadRequested,
      <String, Object?>{
        'requested': qualityLabel(requestedTrack, strings: strings),
      },
    ),
    VesperAbrMode.fixedTrack =>
      strings[ExampleHostText.qualityLeadSwitchToAuto],
  };
  final observed = observationSummary == null
      ? ''
      : strings.format(ExampleHostText.qualityObserved, <String, Object?>{
          'value': observationSummary,
        });
  if (abrPolicy.mode == VesperAbrMode.fixedTrack || effectiveTrack == null) {
    return '$lead$observed';
  }
  final effective = strings.format(
    ExampleHostText.qualityEffective,
    <String, Object?>{'value': qualityLabel(effectiveTrack, strings: strings)},
  );
  return '$lead$effective$observed';
}

String qualityOptionSubtitle(
  VesperMediaTrack track,
  VesperTrackSelectionSnapshot trackSelection,
  String? effectiveVideoTrackId,
  VesperFixedTrackStatus? fixedTrackStatus, {
  required VesperTrackCatalog trackCatalog,
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final base = qualitySubtitle(track, strings: strings);
  final isFixedTrack =
      trackSelection.abrPolicy.mode == VesperAbrMode.fixedTrack;
  final isRequestedTrack =
      isFixedTrack && trackSelection.abrPolicy.trackId == track.id;
  final isEffectiveTrack = effectiveVideoTrackId == track.id;
  final resolvedFixedTrackStatus = currentFixedTrackStatus(
    trackCatalog,
    trackSelection,
    effectiveVideoTrackId,
    fixedTrackStatus,
  );

  final stateKey = switch ((isRequestedTrack, isEffectiveTrack)) {
    (true, true) => ExampleHostText.qualityStateLocked,
    (true, false)
        when resolvedFixedTrackStatus == VesperFixedTrackStatus.pending =>
      ExampleHostText.qualityStatePending,
    (true, false) => ExampleHostText.qualityStateRequested,
    (false, true) when isFixedTrack => ExampleHostText.qualityStateStillHere,
    _ => null,
  };
  if (stateKey == null) {
    return base;
  }
  return '$base · ${strings[stateKey]}';
}

String? qualityOptionBadgeLabel(
  String trackId, {
  required VesperTrackCatalog trackCatalog,
  required VesperTrackSelectionSnapshot trackSelection,
  required String? effectiveVideoTrackId,
  VesperFixedTrackStatus? fixedTrackStatus,
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final isFixedTrack =
      trackSelection.abrPolicy.mode == VesperAbrMode.fixedTrack;
  final requestedTrackId = trackSelection.abrPolicy.trackId;
  final resolvedFixedTrackStatus = currentFixedTrackStatus(
    trackCatalog,
    trackSelection,
    effectiveVideoTrackId,
    fixedTrackStatus,
  );

  if (isFixedTrack && trackId == requestedTrackId) {
    if (resolvedFixedTrackStatus == VesperFixedTrackStatus.locked) {
      return strings[ExampleHostText.qualityBadgeCurrent];
    }
    if (resolvedFixedTrackStatus == VesperFixedTrackStatus.pending) {
      return strings[ExampleHostText.qualityBadgeWaiting];
    }
    return strings[ExampleHostText.qualityBadgeLocking];
  }

  if (isFixedTrack &&
      resolvedFixedTrackStatus == VesperFixedTrackStatus.fallback &&
      effectiveVideoTrackId == trackId) {
    return strings[ExampleHostText.qualityBadgeActual];
  }

  if (trackId == effectiveVideoTrackId) {
    return strings[ExampleHostText.qualityBadgeCurrent];
  }
  return null;
}

/// Whether [badgeLabel] marks a fixed-track lock that has not settled yet.
bool isQualityBadgeWarm(
  String? badgeLabel, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  return badgeLabel != null &&
      (badgeLabel == strings[ExampleHostText.qualityBadgeWaiting] ||
          badgeLabel == strings[ExampleHostText.qualityBadgeLocking]);
}

String? abrConstraintSummary(
  VesperAbrPolicy abrPolicy, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final constraints = <String>[
    if (abrPolicy.maxHeight case final height?)
      strings.format(ExampleHostText.abrMaxHeight, <String, Object?>{
        'value': height,
      }),
    if (abrPolicy.maxWidth case final width?)
      strings.format(ExampleHostText.abrMaxWidth, <String, Object?>{
        'value': width,
      }),
    if (abrPolicy.maxBitRate case final bitRate?)
      strings.format(ExampleHostText.abrMaxBitRate, <String, Object?>{
        'value': formatBitRate(bitRate),
      }),
  ];
  if (constraints.isEmpty) {
    return null;
  }
  return constraints.join(strings[ExampleHostText.listSeparator]);
}

String? videoVariantObservationSummary(
  VesperVideoVariantObservation? observation,
) {
  if (observation == null || !observation.hasSignal) {
    return null;
  }
  final parts = <String>[
    if (observation.width case final width?)
      if (observation.height case final height?) '$width×$height',
    if (observation.bitRate case final bitRate?) formatBitRate(bitRate),
  ];
  if (parts.isEmpty) {
    return null;
  }
  return parts.join(' · ');
}

String? qualityCapabilityNotice(
  VesperPlayerCapabilities capabilities, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final supportsFixedTrackAbr = capabilities.supportsAbrMode(
    VesperAbrMode.fixedTrack,
  );
  final supportsExactVideoTrackSelection = capabilities
      .supportsTrackSelectionFor(VesperMediaTrackKind.video);
  if (supportsFixedTrackAbr && !supportsExactVideoTrackSelection) {
    return strings[ExampleHostText.qualityBestEffortNotice];
  }
  return null;
}

ExampleSheetNoticeModel? qualityRuntimeNotice(
  VesperPlayerSnapshot snapshot, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final lastError = snapshot.lastError;
  if (lastError == null ||
      lastError.category != VesperPlayerErrorCategory.playback) {
    return null;
  }
  if (!lastError.message.contains('fixedTrack')) {
    return null;
  }

  final titleKey = switch (snapshot.trackSelection.abrPolicy.mode) {
    VesperAbrMode.constrained => ExampleHostText.qualityFellBackConstrained,
    VesperAbrMode.auto => ExampleHostText.qualityFellBackAuto,
    VesperAbrMode.fixedTrack
        when snapshot.fixedTrackStatus == VesperFixedTrackStatus.fallback =>
      ExampleHostText.qualityLockNotConverged,
    VesperAbrMode.fixedTrack => ExampleHostText.qualityLockNotice,
  };

  return ExampleSheetNoticeModel(
    title: strings[titleKey],
    message: lastError.message,
    tone: ExampleSheetNoteTone.warm,
  );
}

String audioButtonLabel(
  VesperTrackCatalog trackCatalog,
  VesperTrackSelectionSnapshot trackSelection, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final selectedTrack = firstWhereOrNull<VesperMediaTrack>(
    trackCatalog.audioTracks,
    (track) => track.id == trackSelection.audio.trackId,
  );

  return switch (trackSelection.audio.mode) {
    VesperTrackSelectionMode.track when selectedTrack != null => audioLabel(
      selectedTrack,
      strings: strings,
    ),
    _ => strings[ExampleHostText.sheetAudioTitle],
  };
}

String subtitleButtonLabel(
  VesperTrackCatalog trackCatalog,
  VesperTrackSelectionSnapshot trackSelection, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final selectedTrack = firstWhereOrNull<VesperMediaTrack>(
    trackCatalog.subtitleTracks,
    (track) => track.id == trackSelection.subtitle.trackId,
  );

  return switch (trackSelection.subtitle.mode) {
    VesperTrackSelectionMode.disabled =>
      strings[ExampleHostText.subtitleOffValue],
    VesperTrackSelectionMode.track when selectedTrack != null => subtitleLabel(
      selectedTrack,
      strings: strings,
    ),
    VesperTrackSelectionMode.track =>
      strings[ExampleHostText.sheetSubtitleTitle],
    VesperTrackSelectionMode.auto => strings[ExampleHostText.subtitleAutoValue],
  };
}

String qualityLabel(
  VesperMediaTrack track, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  if (track.height != null) {
    return '${track.height}p';
  }
  if (track.label case final label?) {
    return label;
  }
  return strings[ExampleHostText.trackVideoFallback];
}

String qualitySubtitle(
  VesperMediaTrack track, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final values = <String?>[
    track.codec,
    if (track.bitRate case final bitRate?) formatBitRate(bitRate),
  ].whereType<String>().toList(growable: false);
  if (values.isEmpty) {
    return strings[ExampleHostText.trackVideoVariantFallback];
  }
  return values.join(' • ');
}

String audioLabel(
  VesperMediaTrack track, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  if (track.label case final label?) {
    return label;
  }
  if (track.language case final language?) {
    return language.toUpperCase();
  }
  return strings[ExampleHostText.trackAudioFallback];
}

String audioSubtitle(
  VesperMediaTrack track, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final values = <String?>[
    track.language?.toUpperCase(),
    if (track.channels case final channels?)
      strings.format(ExampleHostText.trackAudioChannels, <String, Object?>{
        'count': channels,
      }),
    if (track.sampleRate case final sampleRate?) '${sampleRate ~/ 1000} kHz',
    track.codec,
  ].whereType<String>().toList(growable: false);
  if (values.isEmpty) {
    return strings[ExampleHostText.trackAudioProgramFallback];
  }
  return values.join(' • ');
}

String subtitleLabel(
  VesperMediaTrack track, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  if (track.label case final label?) {
    return label;
  }
  if (track.language case final language?) {
    return language.toUpperCase();
  }
  return strings[ExampleHostText.trackSubtitleFallback];
}

String subtitleSubtitle(
  VesperMediaTrack track, {
  ExampleHostStrings strings = ExampleHostStrings.zh,
}) {
  final values = <String>[
    if (track.language case final language?) language.toUpperCase(),
    if (track.isForced) strings[ExampleHostText.trackSubtitleForced],
    if (track.isDefault) strings[ExampleHostText.trackSubtitleDefault],
  ];
  if (values.isEmpty) {
    return strings[ExampleHostText.trackSubtitleOptionFallback];
  }
  return values.join(' • ');
}
