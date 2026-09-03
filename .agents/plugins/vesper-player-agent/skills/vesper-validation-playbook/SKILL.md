---
name: vesper-validation-playbook
description: Use when choosing or running Vesper validation commands, cargo checks, Gradle checks, Flutter analyze/test, xcodebuild, FFI header generation, Android AAR builds, iOS XCFramework builds, release checks, or CI-equivalent verification.
metadata:
  short-description: Vesper verification command routing
---

# Vesper Validation Playbook

## Load First

- `../../references/knowledge-map.md`
- `../../references/repository-memory.md`
- `../../references/validation-contract.md`
- `../../references/plugin-runtime-contract.md` when plugin catalog, plan,
  scope, workload, or participation state is involved
- The checkout's root `AGENTS.md`, especially Android Gradle resolution rules,
  when present.
- The specific package README, workflow, or script before running broad checks.

## Command Selection

Validate the touched surface first, then one integration path if behavior crosses
layers.

Rust shared changes:

```sh
cargo check --workspace
cargo test -p player-runtime -p player-download -p player-preload -p player-playlist
```

FFI changes:

```sh
./scripts/vesper ffi generate
./scripts/vesper ffi verify
./scripts/vesper ffi c-host-smoke
```

Android Rust bridge changes:

```sh
cargo check -p player-platform-android -p player-jni-android
```

Android host kit changes:

```sh
cd lib/android
VESPER_GRADLE_BIN="$(find .gradle/wrapper/dists \
  -path '*/bin/gradle' -type f -perm -111 -print -quit)"
test -n "$VESPER_GRADLE_BIN"
GRADLE_USER_HOME="$PWD/.gradle/gradle-user-home" \
"$VESPER_GRADLE_BIN" \
  -p . \
  :vesper-player-kit:checkPublicApiSurface \
  :vesper-player-kit:check
```

The first dependency-resolution attempt may use the network to refresh the
project-local cache. If that attempt fails for a network reason, record the
original error and rerun the identical task once with `--offline`; do not repeat
online retries. This permission does not allow `gradlew` to download a Gradle
distribution: invoke the already cached `bin/gradle` directly.

Flutter package changes:

```sh
cd lib/flutter/<package>
dart analyze --format=machine
flutter test
```

`flutter analyze` may be unavailable when the local analysis server truncates
its LSP JSON output. Report that tool failure separately and retain
`dart analyze --format=machine` as the source analyzer evidence.

Plugin ABI, package, or WASM host changes:

```sh
cargo check -p player-plugin-abi -p player-plugin -p player-plugin-loader -p player-plugin-package -p player-plugin-wasm-host -p player-cli
cargo test -p player-plugin-abi -p player-plugin -p player-plugin-loader -p player-plugin-package -p player-plugin-wasm-host -p player-cli
./scripts/vesper plugin check <project>/vesper-plugin.toml \
  --artifact <artifact> --transport native
./scripts/vesper plugin package <project>/vesper-plugin.toml \
  --signing-key <publisher-key.json> --output <project>.vesper-plugin
./scripts/vesper plugin verify <project>.vesper-plugin --trust-store <trust-store.json>
```

The Native SDK surface is tested with ordinary Rust author crates. The WASM
surface is tested with `wasm32-wasip2` Component fixtures and the Wasmtime host;
the supported WASM capabilities are EventHook and BenchmarkSink only.

Runtime-focused plugin validation must also exercise catalog importer/index
atomicity, resolver determinism and typed conflict/cycle errors, immutable plan
fingerprint/projection checks, scope settlement and quarantine, active versus
next-prewarm authority, and stale correlation generations. For Native
AudioProcessor, cover finite-positive playback-rate policy,
`PreservePitch`/`FollowRate`, bounded queue backpressure, flush/close, and
preserved PCM PTS/discontinuity. Assert that WASM `RealtimeMedia` requests are
rejected before artifact lookup.

Package checks cover deterministic re-packaging, checksum/signature mismatch,
publisher key rotation, target ambiguity, archive limits, path traversal,
duplicate paths, symlinks, and atomic staging. Native signature verification
does not constitute sandbox validation.

Flutter host integration:

```sh
cd examples/flutter-host
flutter analyze
flutter test
flutter build apk --debug
flutter build ios --debug --no-codesign
```

iOS host kit changes:

Generate the Xcode project from the checked-in manifest, list the destinations,
then replace `<SIMULATOR_ID>` with an installed arm64 Simulator ID:

```sh
cd lib/ios/VesperPlayerKit
xcodegen generate
xcodebuild -project VesperPlayerKit.xcodeproj \
  -scheme VesperPlayerKit -showdestinations
xcodebuild test -scheme VesperPlayerKit \
  -project VesperPlayerKit.xcodeproj \
  -destination 'id=<SIMULATOR_ID>' \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=YES \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO
```

Build the distributable core framework with the Rust CLI after the FFI
prerequisite is available:

```sh
./scripts/vesper ios kit-xcframework
```

Desktop decoder plugin checks:

```sh
./scripts/vesper desktop verify-decoder-diagnostics debug all
./scripts/vesper desktop verify-decoder-videotoolbox debug loader
```

FFmpeg packaging:

```sh
./scripts/vesper ffmpeg --list-profiles
./scripts/vesper ffmpeg --platform android --profile default --dry-run
./scripts/vesper ffmpeg --platform android --profile default --verify-only
```

## Regression Shape Selection

For review-driven safety fixes, choose at least one narrow test that proves the
old failure shape is gone:

- stale handle, stale generation, stale packet lease, or double-release paths
- poisoned mutex or failed lock recovery behavior
- queue, cache, registry, event-batch, packet-skip, or pending-frame caps
- timeout fallback for async-to-sync bridges, backpressure, or readiness waits
- invalid external protocol input, non-finite media values, or overflow edges
- unknown cross-language enum, warning, diagnostic, or capability values
- cancellation races and rapid subscribe/unsubscribe or start/stop cycles
- constructor, registration, or loopback startup failure cleanup
- stale catalog/plan fingerprints, prewarm authority violations, transport /
  workload policy rejection, scope quarantine, and AudioProcessor timing
  metadata mutation

Prefer a pure helper or focused crate/package test first. Then add one host-facing
integration check when the behavior crosses FFI, JNI, Swift, Dart, or Gradle
package boundaries.

## Specialized Gates

Timeline-only sampling:

- run the focused controller state-isolation test and platform adapter tests;
- then capture Android 144 Hz and iPhone ProMotion physical-device
  Profile/Release evidence separately;
- do not infer device acceptance from implementation, Debug, or the other OS.

Android subtitle selection:

- cover delayed exact target readiness and exact parameters readback in JVM
  tests;
- run actual Media3 external WebVTT instrumentation, including stable identity
  and cue delivery;
- run the consumer's first-selection and repeated source-switch flow on the
  affected device before closing the original report.

Gradle optional FFmpeg inputs:

- execute `buildRelayFfmpegAndroidJni` or an assembling task in a checkout where
  the generated FFmpeg directory is initially absent;
- cover normal and skip-runtime modes plus input/profile changes;
- `tasks`, `help`, and `--dry-run` are not evidence for task input snapshotting.

Dropped-frame diagnostics:

- run helper/recorder and Android host-kit checks;
- use at least 30 seconds of physical-device Profile/Release playback and
  correlate Media3 events with Flutter/system frame data before making a
  high-refresh claim.

Optional plugin publication:

- after package layout verification, build a clean external consumer from
  hosted Android coordinates and remote SwiftPM;
- inspect the final dependency closure and run the signed app on device.

## Android Gradle Rules

For local Android commands:

- Check for cached Gradle with:
  `find <project>/.gradle/wrapper/dists -path '*/bin/gradle' -type f -perm -111`.
- Invoke the discovered `bin/gradle` directly with `-p <project>`.
- Set `GRADLE_USER_HOME=$PWD/.gradle/gradle-user-home` unless a task requires a
  different cache.
- Do not run `gradlew` if it would download Gradle.
- Allow one online dependency/cache refresh with the cached Gradle executable.
  On its first network-related failure, preserve the error and rerun the same
  command once with `--offline` from the project-local cache.

CI may use CI-provisioned `gradle`; local agent work should avoid online wrapper
downloads.

## Release and Packaging

Before release-affecting changes, verify:

- generated FFI header matches source when FFI changes
- Android public API surface when host kit public API changes
- package changelogs for public breaking changes
- `THIRD_PARTY_NOTICES.md` and README when FFmpeg-backed artifacts change
- baseline host kit without FFmpeg payload when remux/runtime packaging changes

## Reporting

Report exactly what was run and what was not run. If a command is skipped because
the cached Gradle distribution, simulator, Flutter SDK, Xcode, FFmpeg prebuilts,
or device matrix is unavailable, say that directly.
State whether each conclusion comes from committed source, uncommitted worktree
code, an executed test, archive verification, remote consumer build, or physical
device evidence.
