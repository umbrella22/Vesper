---
name: vesper-architecture-memory
description: Use when changing Vesper Player SDK architecture, shared Rust runtime capabilities, DTOs, repository boundaries, public API shape, examples, or deciding whether behavior belongs in runtime, FFI, platform host kits, Flutter packages, or examples.
metadata:
  short-description: Vesper runtime and boundary memory
---

# Vesper Architecture Memory

## Load First

- `../../references/knowledge-map.md`
- `../../references/repository-memory.md`
- `../../references/plugin-runtime-contract.md` for plugin catalog, plan, scope,
  and playback-slot placement
- `../../references/platform-hosts.md` for host-kit or Flutter placement
- The checkout's root `AGENTS.md` and public package documentation when present

For substantial architecture work, verify the bundled baseline against the
current source and public status files before editing.

## Placement Rules

- Cross-platform semantics belong in Rust shared layers: models, state machines,
  queueing, budget, defaults, source identity, cache keys, snapshots, events,
  timeline, track, ABR, resilience, download, preload, playlist.
- Platform execution belongs in platform layers: real player engines, surfaces,
  audio sessions, route changes, permissions, background task handles,
  notifications, system controls, DRM, and vendor SDK setup.
- FFI bridges stable host semantics; it should not expose internal registries or
  backend implementation details.
- Flutter public API should stay in `vesper_player_platform_interface`; platform
  packages serialize and adapt, not invent new public DTO families.
- Examples should only be host apps and regression surfaces. If multiple hosts
  need behavior, move it to runtime, platform, host kit, or Flutter package.

## Runtime Contract

Keep host-facing behavior expressed as:

- `controller`
- `source`
- `snapshot`
- `event`
- `timeline`
- `track`
- `surface`
- `system playback`
- `external route`

Timeline semantics are explicit:

- `Vod`: normal progress bar.
- `Live`: live semantic first, not duration guessing.
- `LiveDvr`: progress represents the DVR window, with `Go Live` and distance to
  live edge.

The default seek interaction is commit-on-release unless a task explicitly adds
preview or scrub behavior.

Plugin runtime placement follows a fixed phase boundary:

- `PluginCatalogImporter` and `PluginCatalogIndex` validate metadata and
  digests without executable I/O.
- `PluginResolver` applies host transport, target, architecture, ABI, semver,
  and priority policy.
- `PluginPlan` is a canonical immutable projection with catalog and plan
  fingerprints.
- `PluginRuntime` activates the plan under hierarchical `PluginScope` values.

Playback has one active and one next-prewarm slot. Correlations fence plan,
session, item, source, and playback generations; only active playback may
commit clock, surface, audio, or participation authority.

## Shared Defaults

Runtime is the source of truth for shared defaults. Platform override is valid
only when a system API, platform security rule, lifecycle rule, or lossy API
mapping requires it. Name the override as a platform override and keep the reason
near the executor boundary.

## Compatibility

Breaking changes are acceptable in this repo when they remove leaked internals,
old ABI fallbacks, or stale DTO aliases. Do not keep a deprecated shim by default
if the shim preserves a wrong public boundary.

When changing public API, update the package changelog or README that owns the
public surface. Keep root Markdown within the root allowlist.

## Plan Absorption

When asked to absorb scattered plans into `devnotes/`, do not move or preserve
them verbatim by default:

1. Resolve the committed baseline and inspect overlapping worktree changes.
2. Classify every proposal as implemented in baseline, worktree-only candidate,
   still open, rejected, or superseded.
3. Write the canonical note around the original cause, actual source-backed
   solution, why that solution owns the invariant, implementation evidence, and
   remaining validation/consumer gates.
4. Update the `devnotes` indexes and supersession metadata.
5. Remove the root draft and raw duplicate only after every useful constraint or
   unresolved item has a canonical home.

Do not turn a test source into executed evidence, a worktree patch into a
released capability, or Android measurements into iOS acceptance.

## Timeline And Track Semantics

- Periodic Flutter progress work should use timeline-only sampling, update only
  timeline state, reject stale results by revision, and retain full refresh for
  authoritative non-timeline reconciliation.
- Track support keeps status/reason/source/playback path and unknown raw values.
  Catalog revision is a command precondition, not persistent ABR configuration.
  Revalidate current platform tracks before applying fixed selection.

Native `AudioProcessor` belongs to an SDK-managed realtime PCM route, not to
the Android Media3 or iOS AVPlayer DirectNative path. Its playback-rate and
pitch policy is shared Rust semantics; host scheduling, PTS, discontinuity, and
A/V timing remain outside the processor.

## Review Questions

- Is this a shared semantic rule or platform execution detail?
- Will this force Android, iOS, Flutter, and desktop to duplicate logic?
- Does this leak raw JNI, C ABI, Media3, AVFoundation, FFmpeg, or Flutter render
  surface details into public API?
- Does this belong in a reusable package instead of an example host?
- Does the change need FFI header generation, public API surface checks, or
  package changelog updates?
