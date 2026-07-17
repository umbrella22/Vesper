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

/// A side-loaded external subtitle track to attach to a [VesperPlayerSource].
///
/// Platform implementations forward the URI, MIME type and optional
/// language/label to the native player (Media3 `SubtitleConfiguration` on
/// Android; a custom renderer on iOS where AVPlayer does not natively consume
/// standalone SRT files).
final class VesperSubtitleSideLoad {
  const VesperSubtitleSideLoad({
    required this.uri,
    this.mimeType = mimeSubrip,
    this.language,
    this.label,
  });

  factory VesperSubtitleSideLoad.fromMap(Map<Object?, Object?> map) {
    return VesperSubtitleSideLoad(
      uri: map['uri'] as String? ?? '',
      mimeType: map['mimeType'] as String? ?? mimeSubrip,
      language: map['language'] as String?,
      label: map['label'] as String?,
    );
  }

  /// Subtitle file URI (local `file://`, `content://`, or remote `https://`).
  final String uri;

  /// Subtitle codec MIME type.
  final String mimeType;

  /// Optional BCP-47 language tag for track selection.
  final String? language;

  /// Optional human-readable label.
  final String? label;

  /// MIME type for SRT subtitles.
  static const mimeSubrip = 'application/x-subrip';

  /// MIME type for WebVTT subtitles.
  static const mimeWebvtt = 'text/vtt';

  /// MIME type for SSA/ASS subtitles.
  static const mimeSsa = 'text/x-ssa';

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'uri': uri,
      'mimeType': mimeType,
      if (language != null) 'language': language,
      if (label != null) 'label': label,
    };
  }

  @override
  bool operator ==(Object other) {
    if (identical(this, other)) return true;
    return other is VesperSubtitleSideLoad &&
        other.uri == uri &&
        other.mimeType == mimeType &&
        other.language == language &&
        other.label == label;
  }

  @override
  int get hashCode => Object.hash(uri, mimeType, language, label);

  @override
  String toString() => 'VesperSubtitleSideLoad(uri: $uri, mimeType: $mimeType, '
      'language: $language, label: $label)';
}
