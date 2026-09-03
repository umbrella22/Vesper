# Changelog

## Unreleased

## 0.5.2 - 2026-09-03

## 0.5.1 - 2026-09-01

## 0.5.0 - 2026-08-29

### Fixed

- Remote iOS consumers no longer inherit `VesperPlayerKitBridgeShim` as an
  unresolved Swift module dependency through the binary kit's ABI metadata.

## 0.4.3-rc.1 - 2026-08-17

### Changed

- Optional Flutter native packages now resolve the remote
  `VesperPlayerSourceNormalizerFfmpeg` and `VesperPlayerRemuxFfmpeg` capability
  products instead of requiring app-local optional framework staging.

## 0.4.2 - 2026-08-16

### Fixed

- The Swift package manifest now depends on the remote
  `umbrella22/VesperPlayerKit` package within the compatible `0.4.x` range and
  requests only its exported `VesperPlayerKit` product.
- Removed monorepo-local package discovery and the nonexistent remote
  `VesperPlayerFFI` product dependency. The binary `VesperPlayerKit` product
  already contains the native bridge closure required by Flutter consumers.

## 0.4.1 - 2026-08-14

### Added

- Added MethodChannel mapping for external subtitle configurations and
  `setSubtitleStyle`, backed by the iOS host-kit subtitle overlay.
- Added complete RTMP, RTSP, and HTTP-FLV protocol wire mapping with explicit
  iOS unsupported playback errors.
- Added track support, catalog revision, playback-path, and structured
  fixed-track error mapping for the iOS wire contract.

### Breaking Changes

- MethodChannel, EventChannel, and platform-view identifiers now use the
  `io.github.umbrella22` reverse-DNS root. The Swift package and module remain
  named `VesperPlayerKit`; custom channel integrations must use the new names.
- The iOS Flutter implementation now requires Flutter 3.44.0 or newer.
- Subtitle MethodChannel calls now await the iOS host API and return canonical
  `vesper_subtitle_error` details for validation and convergence failures.

### Changed

- Picture in Picture failures now emit PiP-specific events and method errors
  without publishing generic player errors.
- `setAbrPolicy` now forwards the optional expected catalog revision and keeps
  native fixed-track capability details in the returned platform error.
- `exitPictureInPicture` now lets `AVPictureInPictureControllerDelegate`
  callbacks publish the final inactive state.
- Subtitle snapshots now carry canonical catalog/selection state and
  requested/confirmed/effective selection fields.
- Bundled SourceNormalizer discovery now resolves the signed sibling framework
  embedded by the Flutter App target through the canonical optional-plugin
  SwiftPM package.

### Fixed

- Source selection starts synchronously on the main actor, waits for the same
  host-kit source task, and preserves a following pause command's intent even
  when the replacement source remains at timeline position zero.
- Seek MethodChannel calls now wait for AVPlayer completion. Structured native
  command errors preserve their message and details, normalize obsolete flags
  for Dart, and do not publish stale command failures as current player errors.

## 0.3.0 - 2026-05-18

- Prepared package metadata for the 0.3.0 release.
