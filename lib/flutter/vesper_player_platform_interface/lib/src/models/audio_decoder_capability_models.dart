part of '../models.dart';

enum VesperAudioDecoderSupport { supported, unsupported, unknown }

final class VesperAudioDecoderCapabilityRequest {
  const VesperAudioDecoderCapabilityRequest(
      {this.sampleMimeType, this.codec, this.channels, this.sampleRate});
  factory VesperAudioDecoderCapabilityRequest.fromMap(
          Map<Object?, Object?> map) =>
      VesperAudioDecoderCapabilityRequest(
          sampleMimeType: map['sampleMimeType'] as String?,
          codec: map['codec'] as String?,
          channels: _decodeInt(map, 'channels'),
          sampleRate: _decodeInt(map, 'sampleRate'));
  final String? sampleMimeType;
  final String? codec;
  final int? channels;
  final int? sampleRate;
  Map<String, Object?> toMap() {
    if ((channels != null && channels! <= 0) ||
        (sampleRate != null && sampleRate! <= 0)) {
      throw ArgumentError(
          'Audio channel count and sample rate must be positive.');
    }
    return <String, Object?>{
      if (sampleMimeType != null) 'sampleMimeType': sampleMimeType,
      if (codec != null) 'codec': codec,
      if (channels != null) 'channels': channels,
      if (sampleRate != null) 'sampleRate': sampleRate,
    };
  }
}

final class VesperAudioDecoderCandidate {
  const VesperAudioDecoderCandidate(
      {required this.name,
      required this.status,
      required this.reason,
      this.hardwareAccelerated,
      this.statusRawValue});
  factory VesperAudioDecoderCandidate.fromMap(Map<Object?, Object?> map) =>
      VesperAudioDecoderCandidate(
          name: map['name'] as String? ?? '',
          hardwareAccelerated: map['hardwareAccelerated'] as bool?,
          status: _decodeEnum(VesperAudioDecoderSupport.values, map['status'],
              VesperAudioDecoderSupport.unknown),
          statusRawValue: map['status'] as String?,
          reason: map['reason'] as String? ?? 'unspecified');
  final String name;
  final bool? hardwareAccelerated;
  final VesperAudioDecoderSupport status;
  final String? statusRawValue;
  final String reason;
  Map<String, Object?> toMap() => <String, Object?>{
        'name': name,
        if (hardwareAccelerated != null)
          'hardwareAccelerated': hardwareAccelerated,
        'status': statusRawValue ?? status.name,
        'reason': reason
      };
}

/// Decoder-format evidence only. This cannot establish container, DRM, output
/// routing or audible-output support, and must not override device quirks.
final class VesperAudioDecoderCapabilityResult {
  const VesperAudioDecoderCapabilityResult(
      {required this.request,
      required this.status,
      required this.reason,
      required this.evidence,
      this.resolvedMimeType,
      this.candidates = const <VesperAudioDecoderCandidate>[],
      this.statusRawValue});
  factory VesperAudioDecoderCapabilityResult.fromMap(
          Map<Object?, Object?> map) =>
      VesperAudioDecoderCapabilityResult(
          request: VesperAudioDecoderCapabilityRequest.fromMap(
              _rawMap(map['request']) ?? const <Object?, Object?>{}),
          status: _decodeEnum(VesperAudioDecoderSupport.values, map['status'],
              VesperAudioDecoderSupport.unknown),
          statusRawValue: map['status'] as String?,
          reason: map['reason'] as String? ?? 'unspecified',
          evidence: map['evidence'] as String? ?? 'unavailable',
          resolvedMimeType: map['resolvedMimeType'] as String?,
          candidates: List<VesperAudioDecoderCandidate>.unmodifiable(
              (map['candidates'] as List? ?? const <Object?>[]).map((value) =>
                  VesperAudioDecoderCandidate.fromMap(
                      Map<Object?, Object?>.from(value as Map)))));
  final VesperAudioDecoderCapabilityRequest request;
  final VesperAudioDecoderSupport status;
  final String? statusRawValue;
  final String reason;
  final String evidence;
  final String? resolvedMimeType;
  final List<VesperAudioDecoderCandidate> candidates;
  Map<String, Object?> toMap() => <String, Object?>{
        'request': request.toMap(),
        'status': statusRawValue ?? status.name,
        'reason': reason,
        'evidence': evidence,
        if (resolvedMimeType != null) 'resolvedMimeType': resolvedMimeType,
        'candidates':
            candidates.map((value) => value.toMap()).toList(growable: false)
      };
}
