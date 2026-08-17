# Changelog

## 0.4.3-rc.1 - 2026-08-17

### Changed

- Android now resolves the hosted SourceNormalizer AAR with its same-version
  core and shared FFmpeg runtime closure.
- iOS now resolves the exact remote
  `VesperPlayerSourceNormalizerFfmpeg` capability product; pub staging no
  longer copies local optional XCFrameworks.

## 0.4.2 - 2026-08-16

- Prepared package metadata for the 0.4.2 release.

## 0.4.1 - 2026-08-14

- The Android plugin package and bundled SourceNormalizer plugin identity now
  use the `io.github.umbrella22` reverse-DNS root. Existing serialized plugin
  references using `io.github.ikaros` must be recreated.
- Aligned package and Android plugin metadata with the Vesper 0.4 package
  family.
- Android build tooling now uses Kotlin 2.4.10.
- Changed the iOS SPM package to depend on the canonical
  `VesperPlayerSourceNormalizerFfmpegPlugin` product. Flutter App targets embed
  and sign its framework and the FFmpeg component frameworks as top-level
  siblings.
- Tagged iOS releases now publish the SourceNormalizer framework as part of the
  seven-XCFramework optional set, gated on the FFmpeg compliance archive and
  exact corresponding source archive.

## 0.3.0

- Adds optional native dependency wiring for the FFmpeg SourceNormalizer plugin.
