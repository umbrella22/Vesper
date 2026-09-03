# Vesper Validation Contract

Report exactly what was run, what was skipped, and which evidence is only a
simulation or archive check. A protocol enum or capability probe is not proof
that a host can play the media; public claims need a host route and regression
evidence.

The current workspace MSRV is Rust 1.98. Toolchain, cache, and device results
bind to the checkout in which they were captured.

Also separate committed baseline behavior from uncommitted worktree candidates.
A test file in the checkout is not execution evidence, and a successful local
artifact stage is not published-consumer evidence.

## Rust And Plugin Checks

```sh
cargo check --workspace
cargo test -p player-plugin-abi -p player-plugin -p player-plugin-loader
cargo test -p player-plugin-package -p player-plugin-wasm-host -p player-cli
./scripts/vesper ffi generate
./scripts/vesper ffi verify
```

For native plugins, also run the relevant `plugin inspect`/`plugin check` path
and a real dynamic-library smoke when the artifact exists. For WASM, use
`wasm32-wasip2` components and verify that no unsupported WASI imports are
present.

For the rewritten runtime, include focused coverage for
`PluginCatalogImporter`/`PluginCatalogIndex` atomic import and digest checks,
`PluginResolver` deterministic ordering and typed conflict/cycle errors,
`PluginPlan` canonical fingerprint and projection rejection, and
`PluginRuntime`/`PluginScope` lifecycle, slot, generation, and quarantine
behavior. Add `AudioProcessor` loader/fixture tests for finite-positive rate
validation, `PreservePitch`/`FollowRate` capability checks, queue
backpressure, flush/close, and preservation of PCM PTS/discontinuity markers.
Run WASM host tests with `PluginInvocationWorkload::Observer` or `Offline` and
assert that `RealtimeMedia` is rejected before artifact lookup.

Online dependency resolution may refresh caches once. On the first
network-related failure, preserve the original error and rerun the identical
command offline from project-local caches; do not repeat online retries. For
Android, keep `GRADLE_USER_HOME` inside the project and use a cached Gradle
distribution.

## Android

First locate a cached executable:

```sh
find <android-project>/.gradle/wrapper/dists -path '*/bin/gradle' -type f -perm -111
```

Invoke that executable directly with `-p <project>`, set a project-local
`GRADLE_USER_HOME`, and follow the online-first/offline-fallback rule above. CI
may use its provisioned Gradle; local work must not download a wrapper
distribution.

## iOS And Flutter

- iOS host-kit changes require the matching XCTest destination and should
  distinguish Simulator/archive evidence from physical-device evidence.
- Build the FFI/XCFramework before testing consumers that require it.
- Flutter package changes require `dart analyze --format=machine` plus package
  tests; report separately if `flutter analyze` emits unusable LSP output.
- Mobile plugin release claims require signed arm64 device/Simulator artifacts,
  registry verification, and explicit device installation evidence.

## Boundary Regression Shapes

Prefer focused tests for stale tokens, wrong-session release, poisoned state,
queue overflow, timeout fallback, unknown values, path traversal, checksum or
signature mismatch, cancellation, and constructor/registration cleanup. Then
add one cross-language or host integration check when the boundary crosses
Rust, FFI, JNI, Swift, Dart, or Gradle.

Plugin runtime regressions also cover stale plan/catalog fingerprints,
ambiguous provider identity, prewarm attempts to commit active authority,
transport/workload policy rejection, scope owner timeout/quarantine, and
AudioProcessor output that mutates host-owned timing metadata.

## Feature-Specific Evidence Gates

- Performance Diagnostics: run the Rust aggregator/ABI tests, bounded lifecycle
  and unknown-value host tests, Flutter batching/model tests, Android host-kit
  checks, and iOS Simulator XCTest. Inspect Release APK/IPA outputs when a host
  claims exclusion. Stable Android acceptance additionally requires a physical
  Debug/Profile `FrameMetrics` capture; Flutter or iOS evidence does not replace
  it. Compare reports only across compatible platform, probe, device, content,
  frame-budget, and refresh-rate conditions.

- Timeline-only sampling: focused Dart state-isolation tests first, then Android
  144 Hz and iPhone ProMotion physical-device Profile/Release evidence. Android
  measurements do not establish iOS behavior.
- Subtitle selection: JVM delayed-readiness/readback tests, real Media3 external
  WebVTT instrumentation, then consumer-device first-selection and repeated
  source-switch regression. Instrumentation source that was not run is not a
  device result.
- Dropped-frame benchmark: helper/recorder tests plus Android host checks, then
  at least 30 seconds of physical-device Profile/Release playback correlated
  with Flutter/system frame evidence. No Media3 callback is not proof of no UI
  jank.
- Optional FFmpeg Gradle input: execute the real build task from a checkout where
  the generated runtime directory is initially absent, and cover both normal
  and skip-runtime modes.
- Optional plugin distribution: verify package contents, then build a clean
  external consumer from hosted coordinates/remote SwiftPM and inspect the final
  signed bundle on device.

## Current Rewrite Evidence Snapshot

The 2026-08-27 checkout evidence records Rust/Cargo 1.98.0. Native
AudioProcessor, FrameProcessor, and WASM observer/offline samples build, load,
check, package deterministically, and verify. Local formatting, workspace check,
strict all-target Clippy, serial workspace tests, contract boundary/verify, FFI,
iOS bridge, documentation, and diff gates passed for that fingerprint.

Android host checks passed with Gradle 9.7.1 on an arm64-v8a NX733J
(`320246872103`) running Android 16/API 36. Dolby clear and CENC playback passed
on the physical device. The optional Shaka Widevine route reached secure decode
and first-frame playback before `proxy.uat.widevine.com:443` timed out; that
external license endpoint remains blocked and is not evidence of a resolved
Shaka gate.

The signed iOS optional-plugin device route has release evidence. FairPlay
protected playback is conditionally waived because no public credential-free
fixture is available; this waiver does not become positive FairPlay evidence.
Silent physical audio/A-V checks cover callback, generation, close, and bounded
drift, but not subjective audible DSP quality.

The rewrite remains `implemented_unverified`, not universal migration or full
feature parity. Open external observations are subjective audible DSP quality,
audible end-to-end A/V synchronization, and playback through a discoverable
Cast/DLNA/AirPlay receiver. Revalidate this snapshot after any related source,
toolchain, artifact, host, or device change.

## Release Caveat

Native signatures prove source/integrity, not sandboxing. `inspect` and `check`
worker processes provide tool crash isolation only. Never claim runtime sandbox
or device playback from package verification alone.

Evidence levels remain separate: source implementation; local package/catalog,
signature, and install verification; clean external consumer build from hosted
coordinates or remote SwiftPM; signed application execution on a supported
physical device or receiver. Subjective DSP quality, physical A/V observation,
DRM, and Cast/DLNA/AirPlay playback require the last level and are not inferred
from Rust or archive tests.
