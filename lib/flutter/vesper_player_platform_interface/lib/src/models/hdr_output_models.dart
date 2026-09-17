part of '../models.dart';

/// The observed dynamic range of the current display output, not source HDR
/// metadata, decoder support, display capability, or a requested render mode.
enum VesperHdrOutputState { unknown, sdr, hdr }

/// The observed output format, when the platform can identify it independently
/// of the source format. Confirmed HDR may still have an unknown format.
enum VesperHdrOutputFormat { unknown, hdr10, hlg, dolbyVision }

/// Evidence about the current player's active display path.
///
/// Platforms without a reliable output observation report [state] as unknown.
/// Missing output snapshots have the same meaning. SDR also requires positive
/// output evidence; absent metadata must never be interpreted as SDR.
///
/// An observing platform must provide [playerId], [sourceRevision],
/// [outputGeneration] and [evidence] for confirmed states. It advances the output
/// generation and publishes unknown before a source, effective track, player,
/// Surface, display or picture-in-picture path change can invalidate evidence.
/// Async observations must carry the generation captured when observation began;
/// the producer rejects stale results even when track or display IDs recur.
/// Capability probes, including candidate-source probes, cannot populate this
/// model. Identity fields may be absent while observation is unavailable.
final class VesperHdrOutputSnapshot {
  const VesperHdrOutputSnapshot({
    this.state = VesperHdrOutputState.unknown,
    this.format = VesperHdrOutputFormat.unknown,
    this.playerId,
    this.sourceRevision,
    this.outputGeneration,
    this.effectiveVideoTrackId,
    this.catalogRevision,
    this.displayId,
    this.evidence,
    this.reason,
    this.stateRawValue,
    this.formatRawValue,
  });

  factory VesperHdrOutputSnapshot.fromMap(Map<Object?, Object?> map) {
    final stateRawValue = map['state'] as String?;
    final formatRawValue = map['format'] as String?;
    final state = _decodeEnum(
      VesperHdrOutputState.values,
      stateRawValue,
      VesperHdrOutputState.unknown,
    );
    final playerId = map['playerId'] as String?;
    final sourceRevision = _decodeInt(map, 'sourceRevision');
    final outputGeneration = _decodeInt(map, 'outputGeneration');
    final evidence = map['evidence'] as String?;
    final incompleteEvidence = state != VesperHdrOutputState.unknown &&
        (playerId == null ||
            playerId.trim().isEmpty ||
            sourceRevision == null ||
            sourceRevision < 0 ||
            outputGeneration == null ||
            outputGeneration < 0 ||
            evidence == null ||
            evidence.trim().isEmpty);
    return VesperHdrOutputSnapshot(
      state: incompleteEvidence ? VesperHdrOutputState.unknown : state,
      format: incompleteEvidence
          ? VesperHdrOutputFormat.unknown
          : _decodeEnum(
              VesperHdrOutputFormat.values,
              formatRawValue,
              VesperHdrOutputFormat.unknown,
            ),
      playerId: playerId,
      sourceRevision: sourceRevision,
      outputGeneration: outputGeneration,
      effectiveVideoTrackId: map['effectiveVideoTrackId'] as String?,
      catalogRevision: _decodeInt(map, 'catalogRevision'),
      displayId: map['displayId'] as String?,
      evidence: incompleteEvidence ? null : evidence,
      reason: map['reason'] as String? ??
          (incompleteEvidence ? 'incompleteOutputEvidence' : null),
      stateRawValue: map['stateRawValue'] as String? ?? stateRawValue,
      formatRawValue: map['formatRawValue'] as String? ?? formatRawValue,
    );
  }

  final VesperHdrOutputState state;
  final VesperHdrOutputFormat format;
  final String? playerId;

  /// A session-local source generation, never a URL or a content label.
  final int? sourceRevision;

  /// A monotonically increasing observation generation within [playerId].
  /// This is independent of the track catalog's revision and track identity.
  final int? outputGeneration;
  final String? effectiveVideoTrackId;
  final int? catalogRevision;
  final String? displayId;

  /// Identifies the platform observation point that confirmed current output.
  final String? evidence;

  /// Explains unavailable or invalidated evidence, for example
  /// `outputObservationUnavailable`. Unknown future reasons are preserved.
  final String? reason;
  final String? stateRawValue;
  final String? formatRawValue;

  Map<String, Object?> toMap() {
    final knownState =
        VesperHdrOutputState.values.any((value) => value.name == stateRawValue);
    final knownFormat = VesperHdrOutputFormat.values
        .any((value) => value.name == formatRawValue);
    return <String, Object?>{
      'state': knownState ? state.name : (stateRawValue ?? state.name),
      'format': knownFormat ? format.name : (formatRawValue ?? format.name),
      if (knownState && stateRawValue != state.name)
        'stateRawValue': stateRawValue,
      if (knownFormat && formatRawValue != format.name)
        'formatRawValue': formatRawValue,
      if (playerId != null) 'playerId': playerId,
      if (sourceRevision != null) 'sourceRevision': sourceRevision,
      if (outputGeneration != null) 'outputGeneration': outputGeneration,
      if (effectiveVideoTrackId != null)
        'effectiveVideoTrackId': effectiveVideoTrackId,
      if (catalogRevision != null) 'catalogRevision': catalogRevision,
      if (displayId != null) 'displayId': displayId,
      if (evidence != null) 'evidence': evidence,
      if (reason != null) 'reason': reason,
    };
  }
}
