part of '../models.dart';

/// Minimal subtitle styling shared by the stable mobile host kits.
///
/// Per-cue typography, animation, and layout remain platform- or
/// content-specific concerns.
final class VesperSubtitleStyle {
  const VesperSubtitleStyle({
    this.fontScale = 1.0,
    this.visible = true,
  }) : assert(
          fontScale >= 0.5 && fontScale <= 3.0,
          'fontScale must be between 0.5 and 3.0.',
        );

  factory VesperSubtitleStyle.fromMap(Map<Object?, Object?> map) {
    final fontScale = _decodeDouble(map, 'fontScale') ?? 1.0;
    if (!fontScale.isFinite || fontScale < 0.5 || fontScale > 3.0) {
      throw ArgumentError.value(
        fontScale,
        'fontScale',
        'must be finite and between 0.5 and 3.0',
      );
    }
    return VesperSubtitleStyle(
      fontScale: fontScale,
      visible: _decodeOptionalBool(map, 'visible') ?? true,
    );
  }

  /// Text scale factor relative to the platform default. `1.0` keeps the
  /// platform default.
  final double fontScale;

  /// Whether subtitle rendering is visible.
  final bool visible;

  /// Convenience for disabling subtitles while keeping the scale.
  VesperSubtitleStyle copyWith({double? fontScale, bool? visible}) {
    return VesperSubtitleStyle(
      fontScale: fontScale ?? this.fontScale,
      visible: visible ?? this.visible,
    );
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'fontScale': fontScale,
      'visible': visible,
    };
  }

  @override
  bool operator ==(Object other) {
    if (identical(this, other)) return true;
    return other is VesperSubtitleStyle &&
        other.fontScale == fontScale &&
        other.visible == visible;
  }

  @override
  int get hashCode => Object.hash(fontScale, visible);

  @override
  String toString() =>
      'VesperSubtitleStyle(fontScale: $fontScale, visible: $visible)';
}

/// An external subtitle track to attach to a [VesperPlayerSource].
///
/// Platform implementations forward the URI, MIME type and optional
/// language/label to the native player (Media3 `SubtitleConfiguration` on
/// Android; a custom renderer on iOS where AVPlayer does not natively consume
/// standalone SRT files).
final class VesperExternalSubtitleSource {
  const VesperExternalSubtitleSource({
    required this.id,
    required this.uri,
    this.mimeType = mimeSubrip,
    this.language,
    this.label,
    this.headers = const <String, String>{},
    this.isDefault = false,
    this.isForced = false,
  })  : assert(id != '', 'id must not be empty.'),
        assert(uri != '', 'uri must not be empty.'),
        assert(mimeType != '', 'mimeType must not be empty.');

  factory VesperExternalSubtitleSource.fromMap(Map<Object?, Object?> map) {
    final id = map['id'] as String? ?? '';
    if (id.trim().isEmpty) {
      throw ArgumentError.value(id, 'id', 'must not be blank.');
    }
    final uri = map['uri'] as String? ?? '';
    if (uri.trim().isEmpty) {
      throw ArgumentError.value(uri, 'uri', 'must not be blank.');
    }
    final mimeType = map['mimeType'] as String? ?? mimeSubrip;
    if (mimeType.trim().isEmpty) {
      throw ArgumentError.value(mimeType, 'mimeType', 'must not be blank.');
    }
    return VesperExternalSubtitleSource(
      id: id,
      uri: uri,
      mimeType: mimeType,
      language: map['language'] as String?,
      label: map['label'] as String?,
      headers: _decodeStringMap(map['headers']),
      isDefault: _decodeBool(map, 'isDefault'),
      isForced: _decodeBool(map, 'isForced'),
    );
  }

  /// Stable source-local identifier used for selection and diagnostics.
  ///
  /// The id must be non-empty and unique within its containing source.
  final String id;

  /// Subtitle file URI (local `file://`, `content://`, or remote `https://`).
  final String uri;

  /// Subtitle codec MIME type.
  final String mimeType;

  /// Optional BCP-47 language tag for track selection.
  final String? language;

  /// Optional human-readable label.
  final String? label;

  /// Optional request headers used when loading this subtitle resource.
  final Map<String, String> headers;

  /// Whether the host should prefer this source for automatic selection.
  final bool isDefault;

  /// Whether this source should only be selected when explicitly requested.
  final bool isForced;

  /// MIME type for SRT subtitles.
  static const mimeSubrip = 'application/x-subrip';

  /// MIME type for WebVTT subtitles.
  static const mimeWebvtt = 'text/vtt';

  /// MIME type for SSA/ASS subtitles.
  static const mimeSsa = 'text/x-ssa';

  Map<String, Object?> toMap() {
    if (id.trim().isEmpty) {
      throw ArgumentError.value(id, 'id', 'must not be blank.');
    }
    if (uri.trim().isEmpty) {
      throw ArgumentError.value(uri, 'uri', 'must not be blank.');
    }
    if (mimeType.trim().isEmpty) {
      throw ArgumentError.value(mimeType, 'mimeType', 'must not be blank.');
    }
    return <String, Object?>{
      'id': id,
      'uri': uri,
      'mimeType': mimeType,
      if (language != null) 'language': language,
      if (label != null) 'label': label,
      'headers': headers,
      'isDefault': isDefault,
      'isForced': isForced,
    };
  }

  @override
  bool operator ==(Object other) {
    if (identical(this, other)) return true;
    return other is VesperExternalSubtitleSource &&
        other.id == id &&
        other.uri == uri &&
        other.mimeType == mimeType &&
        other.language == language &&
        other.label == label &&
        _mapEquals(other.headers, headers) &&
        other.isDefault == isDefault &&
        other.isForced == isForced;
  }

  @override
  int get hashCode => Object.hash(
        id,
        uri,
        mimeType,
        language,
        label,
        Object.hashAll(
          (headers.keys.toList()..sort())
              .map((key) => Object.hash(key, headers[key])),
        ),
        isDefault,
        isForced,
      );

  @override
  String toString() =>
      'VesperExternalSubtitleSource(uri: $uri, mimeType: $mimeType, '
      'language: $language, label: $label, id: $id, '
      'isDefault: $isDefault, isForced: $isForced)';
}

bool _mapEquals(Map<String, String> left, Map<String, String> right) {
  if (left.length != right.length) {
    return false;
  }
  for (final entry in left.entries) {
    if (right[entry.key] != entry.value) {
      return false;
    }
  }
  return true;
}

/// Legacy name for [VesperExternalSubtitleSource].
///
/// This typedef keeps existing source declarations and MIME constants source
/// compatible while making the external-subtitle terminology explicit in the
/// canonical API.
@Deprecated('Use VesperExternalSubtitleSource instead.')
typedef VesperSubtitleSideLoad = VesperExternalSubtitleSource;
