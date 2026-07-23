# Current Progress Checklist

- Status: Active
- Last verified: 2026-06-10
- Canonical path: `CURRENT-CHECKLIST.md`
- Supersedes: none
- Superseded by: none

This checklist is synchronized from the current source tree. It is a short
root-level status view; detailed implementation notes stay under `devnotes/`.

## Confirmed

- [x] Shared Rust contracts remain centered on `player-model`,
  `player-runtime`, and `player-ffi`.
- [x] Timeline semantics cover VOD, live, and LiveDvr with seekable range and
  live-edge state.
- [x] Track catalog, track selection, ABR policy, retry policy, cache policy,
  resilience metrics, preload, download, and playlist are implemented shared
  concepts, not planning-only items.
- [x] Android host APIs remain centered around `VesperPlayerController`,
  `VesperPlayerSource`, `VesperTrackSelection`, `VesperDownloadManager`, and
  `VesperPlayerSurface`.
- [x] iOS host APIs remain centered around `VesperPlayerController`,
  `VesperPlayerSource`, `PlayerSurfaceContainer`, and `VesperDownloadManager`.
- [x] Flutter Android and iOS packages call the native host kits through
  platform channels. The mobile Flutter packages do not call JNI or C FFI
  directly.
- [x] Flutter currently ships Android and iOS packages only; desktop Flutter
  packages are intentionally removed for now.
- [x] Android release packaging is arm64-only.
- [x] iOS release packaging is arm64-only for device and Apple Silicon
  Simulator slices.
- [x] SDK-managed offline download covers VOD HLS, static DASH, FLV planning,
  task restore where host kits own persistence, validated resume, per-resource
  restart when resume ranges are ignored, stale-resource failures, and optional
  MP4 remux export. Desktop FFmpeg HLS does not implement ABR switching yet.
- [x] The desktop download service is shared by macOS, Windows, and Linux host
  surfaces instead of living only inside `basic-player`.
- [x] Desktop preload currently has shared policy/planner integration only; the
  `player-host-desktop` bridge records warmup/cancel commands through a noop
  executor and does not perform real media warmup yet.
- [x] Plugin loading uses the checked-wrapper pattern:
  raw ABI table -> validated API wrapper -> trait object.
- [x] Decoder plugins now use the current native-frame decoder ABI version with
  typed native requirements. The exported API table type is still named
  `VesperDecoderPluginApiV2`, while the expected ABI version is
  `VESPER_DECODER_PLUGIN_ABI_VERSION_V3`.
- [x] FrameProcessor v1 has landed as an internal plugin family with ABI,
  diagnostic plugin, loader support, macOS chain integration, runtime warning
  DTOs, FFI / Flutter warning models, and DirectNative unsupported diagnostics.
- [x] SourceNormalizer has landed as a desktop-first internal plugin family with
  runtime profiles, core detector/config crate, packet-stream v2 ABI,
  diagnostic plugin, FFmpeg packet plugin, and macOS/basic-player opt-in
  routing.
- [x] `PlayerRuntimeStartup.plugin_diagnostics` carries startup plugin
  diagnostics. FFI and Flutter preserve decoder and frame-processor capability
  summaries; SourceNormalizer currently reports through startup diagnostics but
  does not expose a public Flutter capability union.
- [x] `basic-player` distinguishes plugin availability from plugin participation
  in startup summaries.
- [x] The desktop `wgpu` software-render shader path remains SDR-oriented and
  calibrated around Rec.709 limited range.

## Still Open

- [ ] Keep hardening macOS native-frame playback around seek, flush, EOF,
  dispose, source switching, long HLS playback, presenter failures, plugin
  session failures, unsupported codecs, and fallback automation.
- [ ] Keep Windows native-frame on the D3D11 presenter / decoder roadmap until a
  FrameProcessor chain is implemented and validated; unsupported configuration
  should continue to produce explicit startup diagnostics.
- [ ] Keep Linux on the FFmpeg software path until loader-backed plugin
  diagnostics, native-frame presentation, and FrameProcessor support are
  implemented; plugin path configuration should continue to report unsupported
  startup diagnostics.
- [ ] Keep SourceNormalizer desktop-first until packet-stream ownership,
  fallback depth, profile validation, and diagnostics are stable enough for a
  wider surface.
- [ ] Keep FrameProcessor internal-only until desktop native-frame behavior is
  stable enough to justify public API design.
- [ ] Reintroduce Flutter desktop packages only after desktop backend contracts
  and Flutter desktop integration settle.
- [ ] Continue Live / LiveDvr real-experience validation across Android, iOS,
  and Flutter hosts.
- [x] The 0.4 subtitle implementation separates catalog and selection state,
  publishes requested, confirmed, and effective subtitle selection, preserves
  opaque source-stable ids, carries structured errors through native and Flutter
  boundaries, and supports embedded plus external SRT / WebVTT / SSA sources.
  Android selection waits for Media3 track callbacks; iOS selection waits for
  `currentMediaSelection` convergence. Shared fixtures and host unit tests cover
  partial resource failure, duplicate identity/default rejection, source epochs,
  superseded commands, and unknown error values.
- [ ] Complete the 0.4 subtitle device gate. Required evidence includes the
  Controller-to-native-to-snapshot chain on Android and iOS, Flutter integration,
  reorder/refresh/restore identity, manual/auto/disabled selection, timeout and
  source-switch cancellation, and local WebVTT cue delivery.
  - [x] iOS evidence: On 2026-07-23, the complete subtitle gate passed on an
    arm64 iPhone 16 Pro running iOS 27.0 beta and an Apple Silicon Simulator.
    Simulator and physical-device XCTest, Flutter positive rendering, and
    timeout/source-change/supersede lifecycle coverage passed. JSON and PNG
    evidence confirmed visible `Subtitle B` in a window-attached, nonzero overlay.
  - [ ] Android evidence: Run the equivalent complete gate on an arm64 Android
    device before closing the cross-platform gate.
- [ ] Continue release validation for tag-derived version metadata, GitHub
  binary artifacts, and future pub.dev publishing.
- [ ] Pass the canonical iOS `verify-release --scope complete` gate in the
  tagged Xcode 16+ Release job. The local core gate already rebuilds the public
  module from textual interfaces and links all four core distribution paths.

## Deferred

- [ ] DRM support.
- [ ] OS-managed background-transfer services inside the SDK layer.
- [ ] Mobile visual polish and gesture refinements that do not affect SDK
  contracts.
- [ ] Full Source -> Demux -> Decode -> Render plugin graph.
- [ ] Android software fallback inside the main Android crate.

## Verification Entrypoints

```sh
cargo check --workspace
./scripts/vesper ffi verify
./scripts/vesper android aar
./scripts/vesper ios kit-xcframework
./scripts/vesper ios verify-release /path/to/ios-release --scope core
./scripts/vesper desktop verify-remux
cargo test -p player-plugin -p player-plugin-loader -p player-runtime
```
