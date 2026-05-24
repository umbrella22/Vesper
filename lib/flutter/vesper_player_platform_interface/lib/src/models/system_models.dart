part of '../models.dart';

enum VesperBackgroundPlaybackMode { disabled, continueAudio }

enum VesperSystemPlaybackPermissionStatus { notRequired, granted, denied }

enum VesperSystemPlaybackControlKind {
  playPause,
  seekBack,
  seekForward,
}

const int _defaultSystemPlaybackSeekOffsetMs = 10000;
const int _minSystemPlaybackSeekOffsetMs = 1000;
const int _maxSystemPlaybackSeekOffsetMs = 60000;

final class VesperSystemPlaybackControlButton {
  const VesperSystemPlaybackControlButton.playPause()
      : kind = VesperSystemPlaybackControlKind.playPause,
        seekOffsetMs = null;

  const VesperSystemPlaybackControlButton.seekBack([
    this.seekOffsetMs = _defaultSystemPlaybackSeekOffsetMs,
  ]) : kind = VesperSystemPlaybackControlKind.seekBack;

  const VesperSystemPlaybackControlButton.seekForward([
    this.seekOffsetMs = _defaultSystemPlaybackSeekOffsetMs,
  ]) : kind = VesperSystemPlaybackControlKind.seekForward;

  factory VesperSystemPlaybackControlButton.fromMap(
    Map<Object?, Object?> map,
  ) {
    final kind = _decodeEnum(
      VesperSystemPlaybackControlKind.values,
      map['kind'],
      VesperSystemPlaybackControlKind.playPause,
    );
    final seekOffsetMs = _decodeInt(map, 'seekOffsetMs');
    return switch (kind) {
      VesperSystemPlaybackControlKind.seekBack =>
        VesperSystemPlaybackControlButton.seekBack(seekOffsetMs),
      VesperSystemPlaybackControlKind.seekForward =>
        VesperSystemPlaybackControlButton.seekForward(seekOffsetMs),
      VesperSystemPlaybackControlKind.playPause =>
        const VesperSystemPlaybackControlButton.playPause(),
    };
  }

  final VesperSystemPlaybackControlKind kind;
  final int? seekOffsetMs;

  VesperSystemPlaybackControlButton normalized() {
    return switch (kind) {
      VesperSystemPlaybackControlKind.seekBack =>
        VesperSystemPlaybackControlButton.seekBack(_normalizedSeekOffsetMs),
      VesperSystemPlaybackControlKind.seekForward =>
        VesperSystemPlaybackControlButton.seekForward(_normalizedSeekOffsetMs),
      VesperSystemPlaybackControlKind.playPause =>
        const VesperSystemPlaybackControlButton.playPause(),
    };
  }

  Map<String, Object?> toMap() {
    final normalized = this.normalized();
    return <String, Object?>{
      'kind': normalized.kind.name,
      if (normalized.seekOffsetMs != null)
        'seekOffsetMs': normalized.seekOffsetMs,
    };
  }

  int get _normalizedSeekOffsetMs => math.min(
        math.max(
          seekOffsetMs ?? _defaultSystemPlaybackSeekOffsetMs,
          _minSystemPlaybackSeekOffsetMs,
        ),
        _maxSystemPlaybackSeekOffsetMs,
      );
}

final class VesperSystemPlaybackControls {
  const VesperSystemPlaybackControls({
    this.compactButtons = const <VesperSystemPlaybackControlButton>[
      VesperSystemPlaybackControlButton.seekBack(),
      VesperSystemPlaybackControlButton.playPause(),
      VesperSystemPlaybackControlButton.seekForward(),
    ],
  });

  const VesperSystemPlaybackControls.videoDefault()
      : compactButtons = const <VesperSystemPlaybackControlButton>[
          VesperSystemPlaybackControlButton.seekBack(),
          VesperSystemPlaybackControlButton.playPause(),
          VesperSystemPlaybackControlButton.seekForward(),
        ];

  factory VesperSystemPlaybackControls.fromMap(Map<Object?, Object?> map) {
    final rawButtons = map['compactButtons'];
    final buttons = rawButtons is Iterable
        ? rawButtons
            .map(_rawMap)
            .whereType<Map<Object?, Object?>>()
            .map(VesperSystemPlaybackControlButton.fromMap)
            .toList(growable: false)
        : const <VesperSystemPlaybackControlButton>[];
    return VesperSystemPlaybackControls(compactButtons: buttons).normalized();
  }

  final List<VesperSystemPlaybackControlButton> compactButtons;

  VesperSystemPlaybackControls normalized({bool showSeekActions = true}) {
    var buttons = compactButtons
        .take(3)
        .map((button) => button.normalized())
        .toList(growable: true);

    if (buttons.isEmpty) {
      buttons = const VesperSystemPlaybackControls.videoDefault()
          .compactButtons
          .map((button) => button.normalized())
          .toList(growable: true);
    }
    if (buttons.length == 3 &&
        buttons[1].kind != VesperSystemPlaybackControlKind.playPause) {
      buttons[1] = const VesperSystemPlaybackControlButton.playPause();
    }
    if (buttons.every(
      (button) => button.kind != VesperSystemPlaybackControlKind.playPause,
    )) {
      buttons = const VesperSystemPlaybackControls.videoDefault()
          .compactButtons
          .map((button) => button.normalized())
          .toList(growable: true);
    }
    if (!showSeekActions) {
      buttons.removeWhere(
        (button) =>
            button.kind == VesperSystemPlaybackControlKind.seekBack ||
            button.kind == VesperSystemPlaybackControlKind.seekForward,
      );
      if (buttons.isEmpty) {
        buttons.add(const VesperSystemPlaybackControlButton.playPause());
      }
    }

    return VesperSystemPlaybackControls(
      compactButtons: List.unmodifiable(buttons),
    );
  }

  Map<String, Object?> toMap({bool showSeekActions = true}) {
    final normalized = this.normalized(showSeekActions: showSeekActions);
    return <String, Object?>{
      'compactButtons': normalized.compactButtons
          .map((button) => button.toMap())
          .toList(growable: false),
    };
  }
}

