# Changelog

## Unreleased

## 0.5.0 - 2026-08-29

### Breaking Changes

- Rebuilt the unreleased plugin runtime around typed catalog records,
  dependency resolution, invocation plans, bounded runtime scopes, and explicit
  transport/workload policy. Plugin projects must regenerate metadata and
  packages against the 0.5 SDK; compatibility aliases for the prior draft
  contract are not retained.
- Raised the workspace MSRV and release/tooling pins from Rust 1.97 to Rust
  1.98. Generated plugin templates and diagnostic fixtures use the same minimum
  version.
- Advanced first-party plugin versions to `0.5.0` and their host SDK range to
  `>=0.5.0, <0.6.0`.

### Added

- Added a Native `AudioProcessor` ABI, safe Rust author API, checked loader
  session, diagnostic plugin, host-owned timing enforcement, and distinct
  preserve-pitch and follow-rate playback processing.
- Added language-neutral plugin catalog schemas, typed provisions and
  requirements, deterministic dependency/cycle diagnostics, resolver policy,
  participation projection, cancellation, quarantine, and bounded shutdown.
- Added first-party Native AudioProcessor and WASM observer diagnostic projects
  that pass build, inspect, check, package, signature, and repeatability gates.
- Added typed plugin playback route, fallback target, and fallback reason
  diagnostics to the Flutter platform contract while preserving unknown wire
  values.

### Changed

- Upgraded the independent Android library, Compose host, wrapper tooling, and
  their CI Gradle paths to Gradle 9.7.1. Flutter CI and release validation now
  use Flutter 3.47.1 while `flutter build` remains on Gradle 9.3.1, the newest
  version accepted by Flutter 3.47.1's AGP 9.1 compatibility check.
- Tightened audio-master clock, playback-rate, generation fencing, drain, and
  A/V admission behavior across FFmpeg, CPAL, desktop runtime, and plugin
  processing paths.
- Made native-frame diagnostics report `participated` only after a frame is
  actually presented instead of when a route merely starts.

### Fixed

- Made windowed Flutter and Compose timeline scrubbing track the current pointer
  position and measured control width through forward, reverse, and
  direction-changing drags.
- Scoped Android DLNA description reuse and in-flight fetch coalescing to the
  active network binding, and stopped discovery work after a socket binding
  failure.
- Allowed the iOS subtitle device verifier to drive Flutter tests over either
  USB or local-network connections, matching its physical-device preflight.
- Extended release version updates and verification to standalone first-party
  plugin Cargo manifests and lockfiles, including generic `Unreleased`
  changelog freezing.

## 0.4.3 - 2026-08-17

### Added

- Added hosted Android SourceNormalizer and post-download remux AARs. Maven
  Central now publishes seven same-version coordinates, and each optional
  FFmpeg-backed plugin depends on the core kit plus the single shared FFmpeg
  runtime instead of bundling duplicate `libav*` libraries.
- Added `VesperPlayerSourceNormalizerFfmpeg` and
  `VesperPlayerRemuxFfmpeg` capability-level products to the remote Swift
  package. Each product closes over only its plugin and the three matching
  FFmpeg component frameworks.
- Added the `vesper_player_remux_ffmpeg` optional Flutter package and included
  both optional native dependency packages in tag-driven pub.dev publication.

### Changed

- Stable and prerelease tags now publish matching Maven, SwiftPM, and pub.dev
  versions. Prereleases retain a numeric Cargo and platform bundle version while
  using the full `-rc.N` version for public package coordinates.
- Flutter iOS optional packages resolve remote capability products and no
  longer copy a repository-local optional `Artifacts/` directory into pub
  staging.

### Fixed

- Expanded release verification to require the remux plugin, independent
  SourceNormalizer/remux/runtime/relay profile receipts, complete Maven POM
  closure, and the expected plugin registries and ELF payloads in Android sample
  APKs.
- iOS `VesperPlayerKit` release sources now mark the private
  `VesperPlayerKitBridgeShim` import as implementation-only. Release validation
  also checks Swift ABI JSON so a private shim cannot become an unresolved
  dependency for remote SwiftPM consumers.

This release candidate is intended for external consumer and physical-device
acceptance. Archive and hosted dependency checks do not by themselves establish
successful playback on a device.

## 0.4.2 - 2026-08-16

### Fixed

- Added `vesper-player-kit-external-playback` and its transitive
  `vesper-player-kit-ffmpeg-runtime` dependency to the stable Maven Central
  publication set, including same-version POM closure validation and a hosted
  Android application build after publication.
- Changed the published `vesper_player_ios` Swift package manifest to resolve
  `VesperPlayerKit` from the remote binary package, removed monorepo-local path
  discovery, and removed the nonexistent remote `VesperPlayerFFI` product
  dependency.
- Made Flutter package publication wait for the matching Maven Central
  coordinates and remote Swift package tag before acquiring Dart publication
  credentials and uploading packages.

SourceNormalizer, offline MP4 remux, and the remaining experimental mobile
plugin artifacts stay outside the default hosted dependency closure in this
release.

## 0.4.1 - 2026-08-14

### Migration

0.4 requires a coordinated host, FFI binding, and plugin upgrade. Follow the
[0.3 to 0.4 migration guide](lib/MIGRATION-0.3-TO-0.4.md) before replacing any
runtime artifact.

### Breaking Changes

- Replaced the unreleased `io.github.ikaros` reverse-DNS root with
  `io.github.umbrella22` across Android packages and JNI entry points, Flutter
  plugin packages and channels, Apple bundle identifiers, and first-party
  plugin identities. No compatibility aliases are retained; update imports,
  manifest entries, custom channel names, and `VesperPluginReference` values,
  then rebuild native artifacts as one coordinated upgrade.
- Raised the Rust workspace MSRV and dedicated CI check from 1.94 to 1.97.
- Consolidated the native plugin ABI under the `vesper_plugin_entry` root and
  typed interface query contract. The loader accepts only this contract.
- Public plugin authorship is Rust Native or Rust WASM only. C/C++ author SDKs,
  mobile WASM, and plugin access to protected media are outside the plugin
  product boundary.
- Android `setSubtitleTrackSelection` is now suspending and iOS is now
  `async throws`; both complete after native confirmation.
- External subtitle source declarations now use `VesperExternalSubtitleSource`
  and `externalSubtitles`. The old type and source property remain deprecated
  aliases.
- C and iOS FFI consumers must regenerate and recompile bindings for the
  expanded `PlayerFfiError.details_json` field. C consumers must also rebuild
  for the expanded `PlayerFfiTrack` and `PlayerFfiTrackCatalog` layouts;
  replacing only the static library is not ABI-compatible.
- Removed public raw `pluginLibraryPaths` configuration from Android, iOS, and
  Flutter download and benchmark APIs. Mobile hosts now select build-time
  embedded plugins through explicit `VesperPluginReference` values.

### Added

- Added the safe Rust plugin SDK, Native and WASM scaffolds, stable interface
  identifiers, `PluginReference`, deterministic `.vesper-plugin` packaging,
  Ed25519 publisher signatures, trust-store key rotation, and install checks.
- Added the Rust `vesper plugin` CLI for scaffold, build, inspect, check,
  package, verify, install, uninstall, list, and key management workflows.
- Added the Wasmtime Component host for bounded EventHook and BenchmarkSink
  plugins with memory/fuel/deadline limits, queue caps, structured logs, and
  quarantine after traps or timeouts.
- Added a mobile subtitle baseline across Android, iOS, and Flutter: external
  SRT, WebVTT, and SSA/ASS attachment, track selection, visibility, and bounded
  font scaling. Android renders Media3 cues in the native surface host; iOS uses
  a bounded UTF-8 parser and native overlay while AVPlayer text style rules
  cover embedded subtitles.
- Added explicit RTMP, RTSP, and HTTP-FLV protocol DTO values across Rust, C,
  Android, iOS, and Flutter boundaries. Unsupported host routes fail with
  capability errors instead of falling through silently.
- Added canonical subtitle catalog/selection state, requested/confirmed/effective
  selection snapshots, structured cross-platform subtitle errors, DASH/HLS
  metadata propagation, and source-local external subtitle ids.
- Added per-track support status, bounded support diagnostics, catalog revisions,
  and playback-path identifiers across the Rust, FFI, native host, and Flutter
  contracts.
- Added expected catalog revision checks and structured fixed-track rejection
  details for explicit ABR selection.
- Added tagged iOS release artifacts for the three FFmpeg component frameworks
  and four optional plugin frameworks. The release workflow requires a generated
  FFmpeg compliance bundle and exactly one corresponding source archive before
  those XCFrameworks can be published, and binds that archive to the SHA-256
  recorded in each framework's build metadata.
- Added a physical-device optional-plugin verifier that materializes a retained,
  sanitized snapshot of the verified Release archives into an isolated Xcode
  project. It records both the original Release ZIP and tested ZIP SHA-256 values
  in `verified-release-inputs.json` and preserves the exact XCResult bundle.

### Changed

- Updated the published Android host-kit baseline to AGP 9.1-compatible Kotlin
  2.2.10 while the native Compose and Flutter consumer hosts use Kotlin
  2.4.10, plus Media3 1.11.0,
  kotlinx.coroutines 1.11.0, AndroidX Activity 1.13.0, AppCompat 1.8.0,
  Fragment 1.9.0, Lifecycle 2.10.0, Compose BOM 2026.06.01, OkHttp 5.4.0,
  and `material_ui` 1.0.0.
- Updated Flutter CI and publication workflows to Flutter 3.47.0 while keeping
  the public package compatibility floor at Flutter 3.44.0.
- Moved the repository Codex marketplace and maintainer plugins under
  `.agents/plugins`, reserving the root `plugins/` directory for Vesper runtime
  plugin projects.
- Isolated Gradle distributions, service homes, and Android build locks under
  their Android projects; Vesper CLI commands no longer create repository-root
  `.gradle/` state.
- Expanded the Stage UI integration note into `lib/README.md`, the canonical
  Android, iOS, and Flutter package guide, and corrected the iOS temporary-rate
  callback and Flutter mobile-only examples.
- HTTP URLs ending in `.flv` remain progressive VOD sources unless callers use
  the explicit `flvLive` source factory.
- Native-frame plugin breakers now enforce queue/in-flight load decisions and
  reset consecutive failure counters after every successful adapter call.
- Subtitle selection restore, source refresh, and stale callback handling now
  pass through bounded source-epoch and command-id coordination.
- Fixed-track commands now revalidate the current catalog and platform support
  before changing ABR state; iOS retains best-effort variant pinning when exact
  support cannot be established.
- Subtitle selection now resolves stable track identities and confirms the
  applied selection within one bounded readiness/readback deadline.
- Native and Flutter iOS hosts now consume seven direct products from the
  `VesperPlayerOptionalPlugins` Swift package, one for each FFmpeg component or
  plugin framework, and embed and sign them as top-level siblings.
  The flat-dylib and legacy umbrella-runtime embedding paths were removed.
- Removed product-level plugin generation labels from the public entry symbol,
  loader types, WIT package, schemas, mobile registry paths, templates, and
  diagnostics. ABI and interface major/minor fields remain the compatibility
  contract.

### Fixed

- iOS native-frame initialization now reports its pending surface state before
  asynchronous plugin diagnostics, preventing surface attachment from racing
  command readiness.
- Android instrumentation JNI staging now stays inside the host-kit module when
  Flutter redirects Gradle build directories, preserving the Rust CLI output
  boundary. The Android examples also serialize FFmpeg relay generation after
  remux and SourceNormalizer plugin tasks that share the same profile output.
- Android external-playback themes now scope `android:isLightTheme` to API 29+
  resources, and the Flutter package no longer contributes a self-referential
  route-theme alias during resource merging.
- Mobile source selection and seek commands now have bounded, generation-fenced
  native completion semantics across Android, iOS, and Flutter. Pausing during
  an in-flight source load cancels pending autoplay and preserves a paused
  snapshot even at timeline position zero. Obsolete commands fail only their
  originating call without replacing the current playback error.

- Redacted credentials, query parameters, and fragments from iOS playback
  diagnostics, including current-source details, lifecycle and retry logs, HDR
  evidence, DASH network errors, and AVPlayer error-log evidence.
- Redacted iOS foreground-download diagnostic URLs while preserving complete
  stale-resource URIs for recovery callbacks and retried requests.
- Preserved structured subtitle error details across Rust, C FFI, Android
  JNI/Kotlin, and Flutter boundaries, including malformed JSON payloads and
  unknown enum values.
- Preserved structured fixed-track rejection details, including catalog
  revisions and capability evidence, across the C FFI, Android, iOS, and
  Flutter boundaries.
- Isolated obsolete iOS subtitle selection transactions so source changes,
  disposal, and superseding commands cannot restore stale backend state,
  overwrite newer selection state, or publish stale player errors.
- Prevented obsolete Android subtitle selection failures from overwriting newer
  Flutter session errors or emitting stale player error events while preserving
  the original MethodChannel error for the superseded command.
- Core iOS framework archives now keep `VesperPlayerKitBridgeShim` internal,
  use one canonical XCFramework, omit AppleDouble metadata, and pass an isolated
  textual-interface import and link smoke before Release upload.
- Hardened optional iOS release validation to preserve whitespace in Mach-O
  dependency locators, reject undeclared dynamic dependencies and extra
  assets/slices, verify FFmpeg compliance file contents against their sources,
  force FFmpeg source rebuilds, cross-check device and Simulator metadata, and
  remove retired GitHub Release assets during same-tag reruns.
- Aligned Android CI and release jobs with the Gradle 9.6.0 wrapper, corrected
  the Flutter Android minimum requirement to 3.44.0, and synchronized Android
  source-build toolchain documentation.
- Preserved usable subtitle catalogs after partial external resource failures
  while reporting all-track failures and duplicate identity/default conflicts.
- Fixed iOS local audio-only playback waiting for a video frame and separated
  explicit automatic subtitle selection from the startup subtitle-enable gate.

## 0.3.1 - 2026-06-09

### Breaking Changes

- Rust: `player-runtime` no longer exports the unused lightweight async controller
  types (`Player`, `PlayerHandle`, `PlayerConfig`, `PlaybackCommand`, and
  `PlayerEvent`) and no longer depends on Tokio.

## 0.3.0 - 2026-05-18

### Breaking Changes

- FFmpeg mobile builds now use `./scripts/vesper ffmpeg --platform android|ios|all --profile <name>` as the public CLI. The old public `android ffmpeg`, `android ffmpeg-runtime`, and `apple ffmpeg` commands were removed from `scripts/vesper`.
- Android Cast, DLNA, relay, and relay FFmpeg split modules were consolidated into `vesper-player-kit-external-playback` with public APIs under `io.github.ikaros.vesper.player.android.external`.

### Added

- Added `scripts/ffmpeg-profiles.toml` with `base`, `download-remux`, `relay-remux`, and `default` FFmpeg profiles, including inheritance, platform overrides, overlays, validation, and stable profile hashes.
- Added Android release staging for `VesperPlayerKitComposeUi`, `VesperPlayerKitExternalPlayback`, and `VesperPlayerKitFfmpegRuntime` AARs.
- Added optional iOS `VesperPlayerFfmpegRuntime.xcframework.zip` and `VesperPlayerRemuxFfmpegPlugin.xcframework.zip` staging so FFmpeg-backed remux support stays out of the core `VesperPlayerKit.xcframework`.

### Changed

- `download-remux`, `relay-remux`, and `default` profiles validate local-only remux builds with network and OpenSSL disabled by default.
- Flutter external playback on Android now calls the consolidated Kotlin external playback facade while preserving the Dart API.

## 0.2.0 - 2026-05-13

### Breaking Changes

- Android: JNI, bridge, and `Native*` payload types are no longer public API. Use `VesperPlayerController`, `VesperPlayerSource`, `VesperTrackSelection`, `VesperVideoSurfaceKind`, and the download/preload facades instead.
- Android: DLNA and relay artifacts no longer set global `android:usesCleartextTraffic="true"`. Host apps that use local-network HTTP playback must opt in explicitly in their own manifest or network security configuration.
- Android: `vesper-player-kit-compose` now provides only controller binding and surface attachment. Visual styling such as rounded corners, background, and outline belongs in `vesper-player-kit-compose-ui` or host UI.
- Flutter: external playback DTOs moved to `vesper_player_platform_interface`; `vesper_player_external_playback` no longer owns parallel public DTO definitions.
- Rust: `player-model` is now a DTO-only crate. The Tokio actor/controller types moved to `player-runtime`.
- Rust: `DownloadTaskSnapshot.asset_index` is now shared as `Arc<DownloadAssetIndex>` so polling manager snapshots no longer deep-clones resource and segment lists.
- Rust/plugin ABI: decoder plugins now use ABI v3 and typed native device context payloads such as `DecoderNativeDeviceContext::D3D11Device { device_ptr }`. The old generic `{ kind, handle }` native context payload is not accepted.
- iOS FFI: `player-ffi-ios` error codes and categories now align with the desktop FFI error taxonomy.

### Fixed

- Added panic containment to iOS FFI entry points, macOS AVFoundation callbacks, and plugin-loader progress callbacks.
- Added panic containment to decoder and remux plugin entry points and ABI callbacks so plugin panics are mapped to ABI error payloads.
- Replaced production panic paths in Windows probing, timeline ratio seeking, FFmpeg timestamp/audio conversion, and JNI signature helpers.
- Reworked `player-audio-cpal` around an `rtrb` SPSC audio ring so the CPAL callback no longer locks the shared timeline or allocates output chunks.
- Added shared iOS `AVAudioSession` ownership, interruption handling, route-change pause behavior, and `isInterrupted` state propagation.
- Restricted Android relay default binding to a Wi-Fi LAN address and fail fast when no LAN address is available.
- Hardened Android DLNA discovery so stale description fetches cannot publish routes after discovery stops, and SSDP NOTIFY falls back when port 1900 is already bound.
- Clamped download progress updates to known byte and segment totals, and forced paused in-flight preparation tasks to re-run preparation before resuming.
- Added a backend-level FFmpeg `MasterClock` abstraction with audio-as-master selection for desktop A/V synchronization.
- Added Android host-kit API surface verification to block bridge, JNI, and `Native*` implementation types from re-entering the public API.
- Preserved Flutter viewport state across app lifecycle transitions.
