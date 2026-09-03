# Vesper Agent Knowledge Map

This plugin is self-contained and carries the contract cards used by its skills.

## Source Priority

When the plugin is used inside a Vesper checkout, use this order:

1. Current source, manifests, generated artifacts, and tests. Distinguish the
   committed baseline from uncommitted worktree candidates.
2. The repository root `AGENTS.md` and public root/package documentation when
   those files are present.
3. The bundled reference cards in this directory.

If a bundled card and the current checkout disagree, verify the current code
and report the mismatch.

Do not turn an untracked plan, a test source that has not been run, local
artifact staging, or a worktree-only implementation into a completed capability
claim. Record each as proposal, candidate implementation, executed verification,
or released/consumer evidence.

## Reference Cards

- `repository-memory.md`: runtime architecture, workspace boundaries, public
  API ownership, platform floor, and repository conventions.
- `plugin-contract.md`: native ABI, safe Rust SDK, capability interfaces,
  ownership, package trust, and WASM limits.
- `plugin-runtime-contract.md`: catalog import, deterministic resolution,
  immutable plans, runtime scopes, playback correlation, workload policy, and
  participation evidence.
- `platform-hosts.md`: Android, iOS, Flutter, surfaces, channels, system
  playback, and external-route boundaries.
- `mobile-plugin-contract.md`: SourceNormalizer, Decoder, FrameProcessor,
  AudioProcessor, and mobile participation rules.
- `ffmpeg-contract.md`: FFmpeg profiles, runtime/plugin artifact split,
  licensing, relay remux, and release metadata.
- `defensive-boundaries.md`: lifecycle, FFI, queue, lock, timeout, and
  cross-language diagnostic guardrails.
- `validation-contract.md`: command routing, local Gradle policy, package
  checks, device evidence, and reporting rules.
- `performance-diagnostics.md`: schema v1, platform probes, fixed diagnosis
  thresholds, privacy boundary, guided overlay A/B method, and optional
  artifact evidence.

When a task crosses catalog, resolver, plan, scope, or playback-slot state,
load `plugin-runtime-contract.md` before the family-specific card. The runtime
card defines the phase boundary; ABI, mobile, and FFmpeg cards define the
implementation-specific boundary inside that phase.

## Current Checkout Anchors

Use these public files when they exist, but do not treat their absence as a
reason to load private material:

- `AGENTS.md`
- `README.md`
- `ROADMAP.md`
- `CURRENT-CHECKLIST.md`
- `CHANGELOG.md`
- package READMEs under `lib/`, `examples/`, and `crates/`
- `docs/performance-diagnostics.md` when the checkout exposes the official
  diagnostics session
