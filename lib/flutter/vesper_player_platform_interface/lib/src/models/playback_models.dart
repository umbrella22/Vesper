part of '../models.dart';

enum VesperMediaTrackKind { video, audio, subtitle }

enum VesperTrackSelectionMode { auto, disabled, track }

enum VesperAbrMode { auto, constrained, fixedTrack }

enum VesperFixedTrackStatus { pending, locked, fallback }

final class VesperVideoVariantObservation {
  const VesperVideoVariantObservation({
    this.bitRate,
    this.width,
    this.height,
  });

  factory VesperVideoVariantObservation.fromMap(Map<Object?, Object?> map) {
    return VesperVideoVariantObservation(
      bitRate: _decodeInt(map, 'bitRate'),
      width: _decodeInt(map, 'width'),
      height: _decodeInt(map, 'height'),
    );
  }

  final int? bitRate;
  final int? width;
  final int? height;

  bool get hasSignal => bitRate != null || (width != null && height != null);

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'bitRate': bitRate,
      'width': width,
      'height': height,
    };
  }
}

final class VesperSystemPlaybackMetadata {
  const VesperSystemPlaybackMetadata({
    required this.title,
    this.artist,
    this.albumTitle,
    this.artworkUri,
    this.contentUri,
    this.durationMs,
    this.isLive = false,
  });

  factory VesperSystemPlaybackMetadata.fromMap(Map<Object?, Object?> map) {
    return VesperSystemPlaybackMetadata(
      title: map['title'] as String? ?? '',
      artist: map['artist'] as String?,
      albumTitle: map['albumTitle'] as String?,
      artworkUri: map['artworkUri'] as String?,
      contentUri: map['contentUri'] as String?,
      durationMs: _decodeInt(map, 'durationMs'),
      isLive: _decodeBool(map, 'isLive'),
    );
  }

  final String title;
  final String? artist;
  final String? albumTitle;
  final String? artworkUri;
  final String? contentUri;
  final int? durationMs;
  final bool isLive;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'title': title,
      'artist': artist,
      'albumTitle': albumTitle,
      'artworkUri': artworkUri,
      'contentUri': contentUri,
      'durationMs': durationMs,
      'isLive': isLive,
    };
  }
}

final class VesperSystemPlaybackConfiguration {
  const VesperSystemPlaybackConfiguration({
    this.enabled = true,
    this.backgroundMode = VesperBackgroundPlaybackMode.continueAudio,
    this.showSystemControls = true,
    this.showSeekActions = true,
    this.metadata,
    this.controls,
  });

  factory VesperSystemPlaybackConfiguration.fromMap(
    Map<Object?, Object?> map,
  ) {
    final rawMetadata = _rawMap(map['metadata']);
    final rawControls = _rawMap(map['controls']);
    return VesperSystemPlaybackConfiguration(
      enabled: _decodeBool(map, 'enabled', fallback: true),
      backgroundMode: _decodeEnum(
        VesperBackgroundPlaybackMode.values,
        map['backgroundMode'],
        VesperBackgroundPlaybackMode.continueAudio,
      ),
      showSystemControls: _decodeBool(
        map,
        'showSystemControls',
        fallback: true,
      ),
      showSeekActions: _decodeBool(map, 'showSeekActions', fallback: true),
      metadata: rawMetadata == null
          ? null
          : VesperSystemPlaybackMetadata.fromMap(rawMetadata),
      controls: rawControls == null
          ? const VesperSystemPlaybackControls.videoDefault()
          : VesperSystemPlaybackControls.fromMap(rawControls),
    );
  }

  final bool enabled;
  final VesperBackgroundPlaybackMode backgroundMode;
  final bool showSystemControls;
  final bool showSeekActions;
  final VesperSystemPlaybackMetadata? metadata;
  final VesperSystemPlaybackControls? controls;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'enabled': enabled,
      'backgroundMode': backgroundMode.name,
      'showSystemControls': showSystemControls,
      'showSeekActions': showSeekActions,
      'metadata': metadata?.toMap(),
      'controls':
          (controls ?? const VesperSystemPlaybackControls.videoDefault())
              .toMap(showSeekActions: showSeekActions),
    };
  }
}
