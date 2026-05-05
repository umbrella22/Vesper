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
  gestures (double-tap seek, drag scrub), fullscreen toggle, and sheet entry
  points
- Stage helpers: bottom-sheet entry types, formatting helpers
- Stage models: presentation-layer DTOs consumed by `VesperPlayerStage`
- Stage device controls: brightness / volume gesture wiring helpers

## Installation

The Flutter packages are source-distributed from this repository and currently
set `publish_to: none`. In a host app, use path or git dependencies until the
package family is published:

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

## Minimum Requirements

- Dart SDK 3.6.0+
- Flutter 3.41.0+

## Related Packages

- `vesper_player` — main API surface
- `vesper_player_platform_interface` — shared DTOs
