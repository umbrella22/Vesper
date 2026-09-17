# vesper_player_ui

Optional Flutter UI controls and player stage built on top of `vesper_player`.

This package provides ready-made widgets that consume a `VesperPlayerController`
so apps can adopt a polished player surface without re-implementing controls,
gestures, fullscreen, or bottom sheets.

## Status

Experimental. The widgets and APIs are not yet frozen and may change between
minor releases. Pin the version explicitly when consuming.

## What's Included

Exported from `package:vesper_player_ui/vesper_player_ui.dart`:

- `VesperPlayerStage` — opinionated player stage with controls overlay,
  gestures (double-tap play / pause, drag scrub), fullscreen toggle, and sheet entry
  points. Hosts can pass `topBarPrimaryAction` and `topBarSecondaryAction` for
  Cast, AirPlay, DLNA, or custom menu buttons that should follow the stage
  overlay. `contentOverlay`, `expandedControlBarLeading`, and
  `onNavigateBack` provide content, control-row, and navigation extension points
- Stage helpers: bottom-sheet entry types, formatting helpers
- Stage models: presentation-layer DTOs consumed by `VesperPlayerStage`
- Stage device controls: brightness / volume gesture wiring helpers
- `VesperAirPlayRouteButton` — iOS `AVRoutePickerView` wrapper bound to the
  active `VesperPlayerController`
- `VesperAirPlayRouteIconButton` — stage-sized AirPlay route picker wrapper for
  `VesperPlayerStage.topBarPrimaryAction`

## Installation

Use the hosted packages for normal application integration:

```yaml
dependencies:
  vesper_player: 0.6.0
  vesper_player_ui: 0.6.0
```

Repository development first runs
`./scripts/vesper flutter local-overrides`. External hosts that intentionally
consume a source checkout must configure the root-level federated package
overrides described in the
[`vesper_player` installation guide](../vesper_player/README.md#installation):

```yaml
dependencies:
  vesper_player:
    path: path/to/rust-player-sdk/lib/flutter/vesper_player
  vesper_player_ui:
    path: path/to/rust-player-sdk/lib/flutter/vesper_player_ui
```

`vesper_player_ui` depends on `vesper_player`. Apps that build their own UI
can depend on `vesper_player` directly and skip this package.

`VesperPlayerStage` keeps decorative full-stage overlays non-interactive, so
empty video-space gestures continue to work while controls are visible. Only
the actual buttons, sheet entries, and timeline receive pointer events.

`contentOverlay` renders above the player view and below Stage gestures and
controls. The Stage wraps it in `IgnorePointer` and `RepaintBoundary`, clips it
to the player area, and removes it from Picture in Picture presentation. Hosts
remain responsible for bounding the overlay's parsing, layout, cache, and paint
cost.

Use `onContentTap` to hit-test visual content after the Stage confirms a single
tap. The synchronous callback receives logical pixels from the top-left of the
complete Stage / `contentOverlay` canvas, including letterboxing. Return `true`
to consume the tap without changing controls or playback; return `false` to
retain normal control visibility and auto-hide behavior. A null callback keeps
the existing behavior. Hosts can start asynchronous work after accepting a hit:

```dart
onContentTap: (position) {
  if (snapshot.playbackState != VesperPlaybackState.paused ||
      sheetOpen || pictureInPictureActive) {
    return false;
  }
  final content = hitTestVisibleContent(position);
  if (content == null) return false;
  openContentMenu(content);
  return true;
},
```

Buttons and the timeline retain priority. While controls are visible, the
bottom control area (74 logical pixels in compact layout, 112 in expanded layout) is
excluded from Stage gestures, including blank space between controls. Full
canvas coordinates do not imply that this reserved area is interactive.
Double taps, drags, long presses, cancellations, Picture in Picture, and
disposed stages never dispatch a content tap. Hosts own state gating, current
content identity checks, and hit testing from the topmost visible item down.

The content layer remains excluded from pointer input and accessibility.
Existing Stage accessibility taps keep their control visibility action; their
synthetic center position is not offered to `onContentTap`. Provide a separate
focusable content list or action entry point. Test host interactions with the
complete Stage rather than only the visual overlay.

`expandedControlBarLeading` is inserted directly after the expanded play
button. `null` adds no spacing. A host can pass fixed-size content or a direct
`Expanded`/`Flexible` child. Controls that are unavailable should pass `null`
so the remaining built-in controls retain their order.

`onNavigateBack` controls whether the top-left back action exists. Supply
`navigateBackSemanticLabel` for the current mode. Set `keepControlsVisible`
while a host input or drawer is active; changing it back to `false` restarts the
normal auto-hide interval.

`VesperAirPlayRouteButton` is an iOS-only route picker. It renders an empty box
on non-iOS platforms so shared control rows can keep a stable layout.

Use `VesperAirPlayRouteIconButton` inside a stage top-bar action slot when the
AirPlay picker should hide and show with the player controls.

Set `VesperPlayerStage.pictureInPicturePresentation` while handing playback to
system Picture in Picture. In that mode the stage renders only the video
surface and disables custom overlay gestures so the platform PiP UI owns all
visible playback controls.

`VesperPlayerStage` uses English labels by default. Apps can replace only the
stage copy without rebuilding the stage controls:

```dart
VesperPlayerStage(
  controller: controller,
  snapshot: snapshot,
  controlLayout: VesperStageControlLayout.compact,
  isFullscreen: isFullscreen,
  contentOverlay: const HostContentOverlay(),
  expandedControlBarLeading: const HostExpandedControls(),
  onNavigateBack: exitCurrentPresentation,
  navigateBackSemanticLabel: 'Exit fullscreen',
  keepControlsVisible: activeDrawer != null || composerHasFocus,
  strings: const VesperPlayerStageStrings.zhHans(),
  onOpenSheet: onOpenSheet,
  onToggleFullscreen: onToggleFullscreen,
)
```

## Portrait playback and migration

Version 0.6.0 separates `controlLayout` from `isFullscreen` and renames the
expanded control-row slot. See the [video presentation contract and migration
guide](../vesper_player_platform_interface/doc/video-presentation.md) for
per-view picture geometry, coordinate mapping, gestures, and PiP.

## Minimum Requirements

- Dart SDK 3.6.0+
- Flutter 3.47.2+

## Related Packages

- `vesper_player` — main API surface
- `vesper_player_platform_interface` — shared DTOs
