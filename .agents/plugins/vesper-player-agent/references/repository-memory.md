# Vesper Repository Contract

This file is bundled with the plugin as a compact maintainer baseline. It is
not a substitute for current source, manifests, tests, generated artifacts, or
public repository documentation. When those sources exist, verify behavior
against them.

## Current Architecture

- The stable contract is `player-runtime`; platforms can replace the backend.
- `player-ffi` is a bridge model layer, not a place to expose backend internals.
- Android and iOS are native-first through host kits.
- Windows and Linux are FFmpeg-first.
- Software decode fallback remains a fallback direction, not part of the current
  mobile delivery promise.
- Runtime owns shared timeline, snapshot, event, track, ABR, resilience, defaults,
  download, preload, and playlist semantics.
- Platform layers own real player execution, surfaces, audio sessions, route
  changes, interruption handling, notifications, background tasks, permissions,
  DRM, and vendor SDK setup.

## Workspace Boundaries

- Rust crates live under `crates/`; platform wrappers live under `lib/`; host
  applications live under `examples/`.
- `include/player_ffi.h` is generated and must not be edited manually.
- `plugins/` contains Vesper runtime plugin projects. Repository-local Codex
  plugin assets live under `.agents/plugins/` and are separate from runtime
  plugins.
- Public root documentation is limited to `README.md`, `ROADMAP.md`,
  `CURRENT-CHECKLIST.md`, `CHANGELOG.md`, `AGENTS.md`, and notices. Use package
  READMEs for package-specific contracts.
- Repository-local Chinese plans and implementation records belong under
  `devnotes/`. When absorbing a root draft, compare it with current code first,
  rewrite it into cause, actual solution, design rationale, implementation
  evidence, and remaining gates, update the local indexes, and only then remove
  the draft. Do not archive a raw copy and call that absorption.
- Rust uses Edition 2024 with MSRV 1.98. Pure business crates deny unsafe code;
  FFI, plugin ABI, platform, and backend wrappers may use documented unsafe
  blocks only at their system boundaries.

## Product Direction

- Android API 26+ arm64-v8a and iOS 17+ arm64 are the supported mobile floor.
- Android and iOS native player routes define production playback. Desktop
  native-frame, decoder, FrameProcessor, SourceNormalizer, and Flutter desktop
  surfaces remain experimental or optional.
- A capability enum, parser, loader result, or probe does not establish support
  without an implemented host route, explicit unsupported behavior, regression
  coverage, and host/device evidence where relevant.

## Public Surface Boundaries

- Android public APIs stay centered around `VesperPlayerController`,
  `VesperPlayerSource`, and `VesperTrackSelection`.
- iOS public APIs are `@MainActor` and expose `VesperPlayerKit`, not bridge
  internals.
- Flutter public DTOs live in `vesper_player_platform_interface`; platform
  packages must not invent parallel public DTOs.
- Flutter mobile packages call existing host kits through MethodChannel and
  EventChannel; Dart must not call JNI or C FFI directly.
- Examples are standalone hosts and manual regression apps, not SDK logic
  storage.
- Android external playback currently ships as a host-kit/package integration
  surface; treat Cast, DLNA, relay, and relay format adaptation as internal
  responsibilities unless current code exposes a separate distributable module.

## Timeline And Track Contracts

- Periodic Flutter progress polling uses a timeline-only `sampleTimeline` path.
  It updates only timeline state, rejects stale in-flight samples with a
  revision fence, and uses bounded backoff without publishing a playback error.
  Full refresh remains authoritative for source changes, command reconciliation,
  tracks, subtitles, viewport, diagnostics, and errors.
- Track support is structured as status, reason, evidence source, bounded
  diagnostics, and playback path. Preserve unknown wire values and their raw
  representation; lack of evidence is not unsupported.
- Track catalogs carry a monotonic revision. A fixed-track command may carry an
  expected revision, but that token belongs to the command rather than the
  persistent ABR policy. Revalidate the current platform catalog before changing
  playback state.
- Android rejects tracks explicitly reported as exceeding capability or
  unsupported before creating an override. iOS fixed-track remains best-effort
  variant pinning and must not be described as exact track switching.

## Plugin Model

- `player-plugin-abi` owns the raw C-compatible root and typed interface
  tables. `player-plugin` is the `#![deny(unsafe_code)]` author SDK and export
  macro surface; external Rust authors should not write `unsafe` or
  `extern "C"`.
- Native plugins export one `vesper_plugin_entry` root. The root exposes
  identity, version, interface query, byte ownership, and destruction hooks.
- `player-plugin-loader` validates the root and typed interface headers before
  constructing checked wrappers and trait objects. Interface IDs are stable;
  minor revisions append fields and major revisions change compatibility.
- `PluginReference` requires a validated reverse-DNS identity and explicit
  `Native` or `Wasm` transport. Omitting a capability instance is valid only
  when exactly one implementation matches.
- Rust authors use the safe `player-plugin` SDK. Rust WASM components target
  `wasm32-wasip2` and currently cover EventHook and BenchmarkSink with bounded
  structured input/output. They do not receive media bytes, files, network,
  DRM, environment, process, clock, or random access.
- The public authoring surface supports Rust Native and Rust WASM only. There
  is no C or C++ author SDK; C-compatible ABI, iOS bridge FFI, and FFmpeg C
  dependencies remain internal integration boundaries.
- Native signatures establish source and integrity only; native plugins are
  trusted code. `inspect` and `check` worker processes provide tool failure
  isolation, not a runtime sandbox.
- Plugin execution is phase-separated: `PluginCatalog` and
  `PluginCatalogIndex` hold metadata and digest provenance; `PluginResolver`
  produces deterministic provider choices; `PluginPlan` stores a canonical,
  fingerprinted immutable projection; `PluginRuntime` activates executable
  owners under hierarchical `PluginScope` values. Catalogs and plans never
  contain live handles, workers, queues, callbacks, or media bytes.
- Scope kinds include `Root`, `Player`, `Playback`, `NextPrewarm`, `Operation`,
  and `Worker`; states include `Created`, `Starting`, `Running`, `Draining`,
  `Closed`, `Failed`, `Cancelled`, and `Quarantined`. Child/owner counts,
  nesting, and runtime registrations are bounded. Settlement uses one total
  deadline and records panic, timeout, worker, and disposer failures as
  quarantine diagnostics.
- Playback has one active slot and one next-item prewarm slot. Plan, session,
  item, source, and playback generations fence attachments. Only the active
  slot can commit `MasterClock`, `VideoSurface`, `AudioSink`, or
  `Participation`; prewarm cannot exercise active authority.
- `PluginInvocationPolicy` separates `Native`/`Wasm` transport from
  `RealtimeMedia`, `Observer`, and `Offline` workload. Standard policy rejects
  WASM for realtime media and never silently switches transports.
- Native `AudioProcessor` is a realtime PCM extension with bounded
  `AudioProcessorChain`, finite-positive playback rates, and
  `AudioPitchMode::{PreservePitch, FollowRate}`. Processors return plugin-owned
  PCM while preserving host-owned PTS and discontinuity; the host retains clock
  and A/V timing. Native audio processing is not an Android Media3 or iOS
  AVPlayer direct-route contract.
- `FrameProcessor`, `NativeDecoder`, and `SourceNormalizer` packet/resource
  families remain experimental and require an explicit SDK-managed frame or
  normalized-resource route plus participation evidence.
- Prefer host-safe or runtime-safe plugin points for new extension ideas unless
  the repo already has a proven low-level ABI for that domain. Do not freeze a
  demux, decode, or render provider boundary merely because a feature request
  says "plugin".

## FFmpeg and Remux

- `scripts/ffmpeg-profiles.toml` is the profile source of truth.
- Android packages one shared FFmpeg runtime AAR:
  `vesper-player-kit-ffmpeg-runtime`.
- Relay remux and download remux must not each bundle `libav*`.
- FFmpeg network support is not a default path; relay/download should fetch
  through host/platform code and feed FFmpeg via file, pipe, or local input.
- Any change to FFmpeg flags, external libs, bundled payload, or remux packaging
  must update notices and relevant README/changelog material before release.

## Defensive Programming Judgment

- Keep guards at thread lifecycle, release ordering, generation token, FFI/JNI,
  FFmpeg, AVFoundation, Media3, Flutter channel, socket, XML, manifest, URI, and
  device capability boundaries.
- Remove or consolidate duplicated checks inside one trusted layer, broad
  exception swallowing, and dead branches after checked wrappers.
- Library Rust code should use `Result` and `?`, not `unwrap()` or `expect()`.
- Recover poisoned mutex values with `unwrap_or_else(|e| e.into_inner())` unless
  the boundary should report a backend failure.

## Review-Derived Guardrails

- Treat lifecycle, resource, and diagnostic review findings as reusable patterns,
  not isolated bugs.
- FFI, JNI, plugin ABI, Swift bridge, Kotlin bridge, Flutter channel, and
  platform callback boundaries should define ownership, failure outputs, stale
  handle or lease behavior, and panic/exception mapping before implementation.
- Queues, caches, registries, retry loops, packet-skip loops, event batches, and
  pending-frame lists should be bounded or have a documented proof that they
  cannot grow or spin unbounded.
- Do not hold locks or registries across blocking I/O, socket operations,
  executor shutdown, platform callbacks, or long JNI/FFmpeg/plugin calls.
- Async-to-sync bridges need timeout, cancellation, and fallback or error
  behavior.
- Cross-language warning, diagnostic, capability, and enum decoding should keep
  or report unknown raw values instead of silently mapping them away.
- Regression tests for review fixes should capture the failure shape: stale
  handles, poisoned locks, queue caps, timeout fallback, invalid input, unknown
  enum values, cancellation races, or constructor-failure cleanup.

## Active Risk Themes

- Panic must not unwind across C ABI, plugin ABI, JNI callbacks, or platform C
  callbacks.
- Audio callbacks must avoid locks and allocation in hot paths.
- Android libraries must not leak raw JNI/bridge APIs or globally relax host app
  cleartext policy.
- iOS must handle audio session interruption, route changes, and shared session
  ownership carefully.
- Flutter channel registration and event stream lifecycles must work with
  multiple controller instances.
- Download/preload/playlist high-frequency snapshots should avoid cloning large
  indexes in tight loops.
- A committed implementation, a worktree-only candidate, an unexecuted
  instrumentation test, archive verification, and physical-device acceptance
  are different evidence levels. Report them separately.
