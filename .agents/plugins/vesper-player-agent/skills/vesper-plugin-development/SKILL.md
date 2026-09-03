---
name: vesper-plugin-development
description: Use when implementing a Vesper plugin from a product requirement, choosing whether a feature should be a plugin, designing a new plugin kind, extending an existing plugin family, wiring plugin ABI and loader support, adding runtime or host integration, publishing optional Android/iOS/desktop artifacts, or planning plugin diagnostics, documentation, and validation.
metadata:
  short-description: End-to-end Vesper plugin development
---

# Vesper Plugin Development

## Load First

- `../../references/knowledge-map.md`
- `../../references/repository-memory.md`
- `../../references/plugin-contract.md`
- `../../references/plugin-runtime-contract.md` for catalog, resolver, plan,
  scope, workload, and participation semantics
- `../../references/platform-hosts.md`
- The checkout's root `AGENTS.md` and package READMEs when present
- For ABI details: `$vesper-plugin-workflow`
- For mobile host surfaces: `$vesper-mobile-flutter-hosts`
- For mobile playback consumption: `$vesper-mobile-plugin-playback`
- For FFmpeg-backed artifacts: `$vesper-ffmpeg-packaging`
- For validation routing: `$vesper-validation-playbook`

## Start From the Requirement

Before writing ABI or packaging code, classify the request:

- What behavior should become optional or replaceable?
- Does it need third-party native code, platform APIs, or heavy dependencies?
- Is the feature host-safe, or does it sit inside demux, decode, frame, render,
  network, storage, or release-sensitive code?
- Can failure fall back cleanly without corrupting playback state?
- Which platforms must load it, diagnose it, and distribute it?

Classify the lifecycle phase separately. Metadata belongs in the catalog;
provider choice belongs in the resolver; a selected set belongs in an immutable
plan; executable owners belong in a runtime scope; playback use belongs in a
correlated active slot. Do not expose a live handle through an earlier phase.

Use a plugin only when the boundary reduces coupling, isolates optional
dependencies, or lets hosts opt into capabilities. Do not turn ordinary runtime
configuration into a plugin just because it is configurable.

## Choose the Extension Point

Prefer high-level, host-safe plugin points first:

- source normalization or source resolving
- post-download or remux processing
- frame processing after a decoder/native-frame boundary exists
- decoder selection where native frame ownership is explicit
- Native AudioProcessor where an SDK-managed PCM route owns the timing boundary
- diagnostics or capability probing

Be conservative with low-level boundaries. Demux, decode, frame ownership,
rendering, and AV/MediaCodec handles must have a concrete ownership, fallback,
latency, memory, and diagnostics story before becoming public plugin APIs.

If a plugin family already exists, extend it instead of adding a parallel kind.
If the new behavior needs different lifecycle, data ownership, or participation
semantics, make a new plugin kind rather than overloading decoder-centric or
remux-centric structures.

## Contract Checklist

Define these before implementation:

- root identity, transport (`Native` or `Wasm`), interface ID, and major/minor
  version
- catalog descriptor fields, artifact digest/provenance, requirements,
  provisions, runtime dependencies, and resource limits
- resolver policy, immutable plan fingerprint, selected providers, and
  dependency-first order
- capability summary and unsupported reasons
- configuration model and default mode
- lifecycle: load, probe, open, poll/read/process, flush/seek, close/cancel
- ownership for every handle, buffer, callback, and native object
- stale handle, stale packet lease, callback-token mismatch, and partial-open
  cleanup behavior
- capacity and timeout policy for queues, caches, event batches, retry loops,
  packet-skip loops, and pending output
- fallback behavior for load failure, capability mismatch, runtime errors, and
  timeout
- participation rules: when the plugin was merely available, selected,
  preflighted, bypassed, or actually used in playback
- active versus next-prewarm slot, correlation generations, and which authority
  the plugin may commit
- release artifact shape and whether it is optional
- runtime dependency boundary, especially FFmpeg or platform frameworks
- public docs, notices, examples, and validation commands

The supported author languages are Rust Native and Rust WASM. C and C++ author
SDKs are outside the product surface, while internal C-compatible ABI and
platform bridge dependencies remain available to the host implementation.
WASM author plugins are limited to EventHook and BenchmarkSink and cannot access
media bytes, files, network, DRM, process state, environment, clock, or random.

Keep diagnostics stable enough for Rust tests, platform host kits, Flutter DTOs,
example panels, logs, and release verification.

## Implementation Order

Follow the repo's checked-wrapper pattern:

1. Add or extend the raw C-compatible ABI table in `player-plugin-abi`.
2. Expose only safe author-facing Rust traits and export macros from
   `player-plugin`; ordinary plugin source must not contain `unsafe` or
   `extern "C"`.
3. Add loader parsing, version checks, and `CheckedXxxApi` construction in
   `player-plugin-loader`.
4. Add a fixture or diagnostic plugin when practical.
5. Add runtime/platform model types for configuration, capability, diagnostics,
   and participation.
6. Wire FFI/JNI/Swift/Dart DTOs only after the Rust shape is stable.
7. Add host-kit integration behind disabled or explicit opt-in defaults.
8. Add examples that show diagnostics and fallback state honestly.
9. Add packaging and release staging only after the runtime boundary is clear.
10. Update README, package docs, changelog, and notices when public behavior or
   redistributed dependencies change.

For a runtime-facing extension, insert catalog import, resolver, immutable plan,
and scope activation before executable loading. Runtime activation consumes a
verified plan and does not re-resolve or silently choose another transport in
the hot path.

Validate once at load or open time. Hot paths should use checked wrappers or
trait objects, not repeated optional-function checks.

## Diagnostics Vocabulary

For every plugin path or artifact, make it possible to answer:

- Was the plugin path found?
- Did the dynamic loader open it?
- Which root identity, typed interface IDs, and ABI versions did it advertise?
- Which capability or profile was selected?
- Why was it rejected, bypassed, or treated as diagnostics-only?
- Did it participate in playback, or was the original path used?
- What fallback did the host choose?

Do not collapse "plugin loaded", "capability available", "preflight passed",
and "participated in playback" into one success state.

`plugin_kind` may remain in diagnostics as a domain classification, but it is
not an ABI discriminator and must not select a table or transport.

For realtime media, `PluginInvocationPolicy` rejects WASM before artifact
lookup. Native AudioProcessor supports bounded PCM processing with finite
positive rate and `PreservePitch`/`FollowRate`; it must preserve host-owned PTS
and discontinuity. Android Media3 and iOS AVPlayer DirectNative do not consume
plugin PCM merely because the capability is installed.

## Cross-Language Propagation

When a plugin surface becomes host-facing, keep paired boundaries synchronized:

- Rust model and runtime diagnostics
- FFI structs and generated headers
- JNI bridge and Android Kotlin public/internal DTOs
- Swift host-kit DTOs and resource ownership
- Flutter platform-interface models and method-channel mapping
- examples and tests

Do not expose raw JNI, raw C ABI, `dlopen`, Media3, AVFoundation, or native
handle details as public Flutter or host-kit API unless the product requirement
explicitly needs that low-level contract.

## Packaging Rules

Optional plugin artifacts should be independently installable and diagnosable.
Keep the main host kit usable without optional payloads.

For FFmpeg-backed plugins:

- use the shared FFmpeg profile system
- keep runtime libraries in the shared runtime artifact
- put only the plugin glue binary in the feature plugin artifact
- write and compare profile hashes
- update `THIRD_PARTY_NOTICES.md` and relevant README files
- never hide duplicate runtime libraries with packaging conflict workarounds

For non-FFmpeg plugins, still record capability metadata and dependency
expectations so hosts can explain missing or unsupported behavior.

Keep four evidence levels distinct:

1. a module or Swift product exists in source;
2. local staging and artifact verification succeed;
3. hosted coordinates or remote SwiftPM build in a clean external consumer;
4. the signed final application bundle installs and executes on a supported
   device.

Do not describe level 1 or 2 as published distribution. For iOS, inspect the
actual products rather than inferring an aggregate product from the package
name. For Android, check whether optional publications are enabled rather than
assuming every Gradle module is released.

## Testing Strategy

Start with the smallest layer that owns the risk:

- ABI table and checked-wrapper tests for plugin contract changes
- loader tests for missing functions, duplicate or unknown interface IDs,
  incompatible versions, and capability summaries
- fixture plugin tests for lifecycle and fallback behavior
- Rust runtime tests for diagnostics and participation semantics
- FFI/JNI/Swift/Dart model tests for wire-shape changes
- platform host tests for opt-in loading and fallback
- packaging checks for artifact contents and dependency boundaries
- catalog, plan fingerprint, scope settlement, generation fencing, and
  prewarm-authority checks
- example smoke tests when the user-facing flow changes

Use negative tests deliberately. A good plugin change proves both the success
path and why unsupported plugins do not break normal playback.

For review-driven fixes, add a regression test for the exact failure shape when
practical: stale handles, poisoned locks, bounded queue eviction, timeout
fallback, invalid external input, unknown enum values, cancellation races, or
constructor-failure cleanup.

## Validation Commands

Choose from these based on the touched surfaces:

```sh
cargo check -p player-plugin-abi -p player-plugin -p player-plugin-loader
cargo test -p player-plugin-abi -p player-plugin -p player-plugin-loader
```

For runtime or feature crates:

```sh
cargo test -p <feature-crate> -p <diagnostic-or-fixture-plugin>
```

For FFI shape changes:

```sh
./scripts/vesper ffi generate
./scripts/vesper ffi verify
```

For Android, iOS, or Flutter host changes, use
`$vesper-validation-playbook`. Resolve the current checkout's cached Gradle
executable and an available Simulator at run time; do not pin a cache directory
or device identifier in a reusable skill.

Always finish with:

```sh
git diff --check
```
