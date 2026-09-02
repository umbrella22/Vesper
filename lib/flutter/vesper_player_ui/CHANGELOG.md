# Changelog

## Unreleased

### Added

- Added `contentOverlay`, `landscapeControlBarLeading`, `onNavigateBack`, and
  `keepControlsVisible` Stage integration points.

### Changed

- Host content overlays are clipped, repaint-isolated, non-interactive, below
  Stage controls, and hidden during Picture in Picture presentation.

## 0.5.1 - 2026-09-01

## 0.5.0 - 2026-08-29

### Fixed

- Made windowed timeline scrubbing follow forward, reverse, and
  direction-changing pointer movement, including the final release position.

## 0.4.3-rc.1 - 2026-08-17

- Prepared package metadata for the 0.4.3-rc.1 release.

## 0.4.2 - 2026-08-16

- Prepared package metadata for the 0.4.2 release.

## 0.4.1 - 2026-08-14

### Breaking Changes

- The optional Flutter UI package now requires Flutter 3.44.0 or newer.

### Changed

- Material widgets are imported through the official `material_ui` package.
- The `material_ui` dependency now uses the stable 1.0 release.
- `VesperPlayerStage` now accepts `pictureInPicturePresentation` so hosts can
  hide custom chrome while system Picture in Picture owns playback controls.

## 0.3.0 - 2026-05-18

- Prepared package metadata for the 0.3.0 release.

## 0.2.0 - 2026-05-13

### Breaking Changes

- Default visible stage labels are English. Applications that need localized
  stage text should pass `VesperPlayerStageStrings`; the built-in
  `VesperPlayerStageStrings.zhHans()` constructor provides Simplified Chinese
  labels.
