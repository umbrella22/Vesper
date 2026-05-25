# Changelog

## Unreleased

### Breaking Changes

- The optional Flutter UI package now requires Flutter 3.44.0 or newer.

### Changed

- Material widgets are imported through the official `material_ui` package.

## 0.3.0 - 2026-05-18

- Prepared package metadata for the 0.3.0 release.

## 0.2.0 - 2026-05-13

### Breaking Changes

- Default visible stage labels are English. Applications that need localized
  stage text should pass `VesperPlayerStageStrings`; the built-in
  `VesperPlayerStageStrings.zhHans()` constructor provides Simplified Chinese
  labels.
