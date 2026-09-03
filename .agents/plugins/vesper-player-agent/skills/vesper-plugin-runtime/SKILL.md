---
name: vesper-plugin-runtime
description: Use when changing Vesper plugin catalog import, deterministic resolution, immutable plans, runtime scopes, playback slot correlation, invocation policy, or plugin participation diagnostics.
metadata:
  short-description: Vesper plugin runtime lifecycle
---

# Vesper Plugin Runtime

## Load First

- `../../references/knowledge-map.md`
- `../../references/repository-memory.md`
- `../../references/plugin-contract.md`
- `../../references/plugin-runtime-contract.md`
- The checkout's root `AGENTS.md` and current plugin/runtime tests
- For ABI loading: `$vesper-plugin-workflow`
- For mobile routes: `$vesper-mobile-plugin-playback` and `$vesper-mobile-flutter-hosts`
- For command selection: `$vesper-validation-playbook`

## Runtime Order

Implement and review the phases in this order:

1. Import metadata into `PluginCatalogIndex` with schema, identity, path, and
   digest validation. Keep `PluginCatalogImporter` atomic.
2. Resolve `PluginRequirement` values through `PluginResolverPolicy` and
   `PluginResolver`. Filter transport, target, architecture, ABI, semver, and
   priorities before any executable load.
3. Create an immutable `PluginPlan` from the resolution. Bind the catalog
   fingerprint, policy, requirements, selected providers, dependency-first
   artifacts, and plan fingerprint.
4. Activate `PluginRuntime` only from that plan. Register executable owners in a
   hierarchical `PluginScope`; keep live handles out of catalog and plan types.
5. Attach playback work using one active slot or one next-prewarm slot. Check
   plan, session, item, source, and playback generations at every authority
   boundary.
6. Settle children and owners with one total deadline. Record failed, cancelled,
   and quarantined outcomes without claiming that quarantine released a resource.

Do not combine import, resolution, loading, selection, and participation into a
single `load_plugin` success flag.

## Catalog And Plan Rules

- `PluginCatalogRecord` is metadata plus provenance and digest only.
- `PluginCatalogIndex` may index records by canonical identity and plugin ID,
  but it never opens a library or creates a WASM instance.
- Failed single or batch imports leave the prior index unchanged.
- Resolver policy is host-owned and serialized into the plan; plugin metadata
  cannot override host transport, target, ABI, or workload rules.
- `PluginPlan::from_json` must reject stale catalog fingerprints, tampered plan
  fingerprints, noncanonical ordering, and projections that no longer match the
  resolver.
- A changed catalog or host policy starts a new plan/runtime generation. Mutating
  a running plan invalidates correlation evidence.

## Scope And Playback Rules

Use `PluginScopeKind::Root`, `Player`, `Playback`, `NextPrewarm`, `Operation`,
and `Worker` with the state machine `Created -> Starting -> Running ->
Draining -> {Closed, Failed, Cancelled, Quarantined}`. Enforce child, owner,
depth, and runtime registration limits.

`PluginActivePlaybackCorrelation` and `PluginNextPrewarmCorrelation` carry the
plan fingerprint plus session/item/source generations. The active slot alone
may commit `MasterClock`, `VideoSurface`, `AudioSink`, or `Participation`.
Prewarm may load or prepare bounded state but cannot publish active authority.
Reject stale attachment tokens and generation mismatches. Promotion validates
the next item before settling the old active scope; replacement settles obsolete
prewarm before installing a new active scope.

Owner disposers run outside locks under a shared deadline. Panic, timeout,
worker failure, or disposer failure produces a bounded quarantine record. Close,
cancel, and fail remain idempotent and preserve the first meaningful terminal
diagnostic.

## Transport And Workload

Use explicit `PluginTransport::Native` or `PluginTransport::Wasm`; never invent
an `Auto` mode or infer transport from `plugin_kind`. Apply
`PluginInvocationPolicy::standard()` with:

- `RealtimeMedia`: Native only under an explicit host route;
- `Observer`: Native or bounded WASM;
- `Offline`: Native or bounded WASM.

Return `PluginInvocationPolicyError` before artifact lookup when the pair is
forbidden. Do not silently fall back to another transport. WASM currently
exposes only `PipelineEventHook` and `BenchmarkSink` and has no media-byte,
PCM, frame, filesystem, network, DRM, clock, process, or random-access input.

## Audio And Frame Placement

Use the Native `AudioProcessor` path for realtime PCM. Validate finite positive
rates and the requested `AudioPitchMode` against each processor's capabilities
before opening. Keep `AudioProcessorChain` queue capacity and backpressure
observable. Preserve host-owned `pts_us` and discontinuity markers across every
processor; reject returned PCM that changes them. The host, not the processor,
controls clock and A/V timing.

Use `FrameProcessor`, `NativeDecoder`, and packet/resource `SourceNormalizer`
only in their explicit experimental lanes. A system-player Android Media3 or
iOS AVPlayer route does not become a frame or PCM route merely because a plugin
is installed. SourceNormalizer participation requires successful consumption
of its normalized resource; diagnostics or preflight alone do not count.

## Diagnostics

Record distinct stages for catalog import, resolution, plan creation, load,
interface validation, selection, open, participation, bypass, fallback,
rejection, failure, and quarantine. Include plugin identity, transport,
interface instance, workload, plan/session/item/source/playback generations,
and a bounded redacted reason. Keep `plugin_kind` as a diagnostic label only.

## Validation

Run the narrow runtime and loader suites first:

```sh
cargo test -p player-plugin -p player-plugin-loader -p player-plugin-package
cargo test -p player-plugin-wasm-host
```

Cover catalog atomicity and digest mismatch, deterministic resolver ordering,
version conflict and dependency-cycle errors, plan fingerprint/projection
rejection, stale attachment and generation rejection, prewarm authority
rejection, scope timeout/quarantine, and transport-workload policy errors.
For a Native audio change add the `AudioProcessor` loader/fixture tests and
PCM PTS/discontinuity assertions. For host-facing routes use
`$vesper-validation-playbook`; package verification or source tests do not
replace clean consumer or physical-device evidence.

Always finish documentation or skill-only edits with:

```sh
git diff --check
```
