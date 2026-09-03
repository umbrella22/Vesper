---
name: vesper-plugin-workflow
description: Use when adding or changing Vesper plugin support, plugin ABI tables, player-plugin, player-plugin-loader, decoder plugins, remux plugins, dynamic loader diagnostics, host-safe extension points, or plugin ownership and safety rules.
metadata:
  short-description: Vesper plugin ABI workflow
---

# Vesper Plugin Workflow

## Load First

- `../../references/knowledge-map.md`
- `../../references/repository-memory.md`
- `../../references/plugin-contract.md`
- `../../references/plugin-runtime-contract.md`
- The current ABI, loader, fixture, and package tests in the checkout
- For FFmpeg remux work: also use `$vesper-ffmpeg-packaging`

## Default Design

Use the native ABI adapter pattern:

1. `player-plugin-abi` owns the raw C-compatible root and typed tables;
   `player-plugin` is the safe Rust author SDK and macro re-export.
2. The plugin exports `vesper_plugin_entry` with one `VesperPluginRoot`.
3. The loader validates the root and queries typed interfaces by stable
   interface ID; `plugin_kind` is diagnostic classification only and never
   selects an ABI table.
4. The loader constructs a checked wrapper, such as
   `CheckedXxxApi::try_from`.
5. The hot path holds a checked wrapper or trait object, not repeated `Option`
   function pointer checks.

When the change is runtime-facing, follow the separate catalog -> resolver ->
plan -> scope phases from `plugin-runtime-contract.md`. Never let a loader call
mutate a catalog or plan, and never use a live owner as catalog metadata.

Do not scatter "missing required function" checks across runtime calls. Validate
once at load/open time, then rely on the checked wrapper.

Authoring is limited to Rust Native and Rust WASM. The safe Rust SDK is the
only public author interface; there is no C or C++ author SDK. Internal C ABI,
iOS bridge FFI, and FFmpeg C dependencies remain host integration details.

WASM components use the Wasmtime Component Model and `wasm32-wasip2`. WASM
supports only `PipelineEventHook` and `BenchmarkSink`, with bounded structured
events and diagnostics. The host grants no filesystem, network, environment,
process, clock, random, DRM, PCM, native-frame, or media-byte access.

## Extension Point Judgment

Prefer host-safe and runtime-safe extension points for new feature requests:

- source resolver
- event listener
- subtitle discovery or sidecar loading
- post-download processor
- pipeline event hook
- Native AudioProcessor for an explicit SDK-managed PCM route

Be cautious with low-level demux, decode, and render plugin boundaries. The repo
has active decoder and remux plugin paths, but new low-level plugin kinds must
prove ownership, diagnostics, fallback, and platform coupling before becoming a
public boundary.

## ABI Safety

- Every `extern "C"` boundary must prevent Rust panic from unwinding across C
  ABI. Use `catch_unwind` patterns that map panic to an error status.
- Define ownership for every pointer, byte buffer, native frame, callback, and
  handle.
- Define what happens after a stale or mismatched handle, packet lease, callback
  token, or generation value is observed. Prefer poisoning or invalidating the
  resource and requiring reopen, seek, or flush over allowing ambiguous retry.
- Release through the same API table that created the resource.
- Opaque native handles need explicit platform invariants and `SAFETY` comments.
- Do not accept integer handles as trusted pointers without a validation story
  such as `QueryInterface`, retain/release pairing, or equivalent ownership.
- Keep the root and typed interface headers stable and versioned. Minor
  versions are append-only; incompatible signatures or ownership require a new
  major interface version.
- Plugin-driven loops that skip packets, poll callbacks, retry reads, or drain
  events must have bounded iterations or timeout-backed diagnostics.
- Loader-side plugin calls should treat panic or ABI violation as a poisoned
  plugin instance unless the ABI explicitly proves reuse is safe.
- `PluginInvocationPolicy` is checked before lookup: standard policy allows
  Native for `RealtimeMedia`, permits bounded Native/WASM `Observer` and
  `Offline`, and rejects WASM realtime media with a typed error. There is no
  silent transport fallback.

## Diagnostics

Plugin diagnostics should help a host answer:

- Which plugin paths were considered?
- Which loaded successfully?
- Which root identity, typed interface IDs, and ABI versions did each artifact
  advertise?
- Which codecs or capabilities are supported?
- Why was a plugin rejected or only used as diagnostics?
- Which fallback path was selected?
- Did a prewarm scope attempt to commit active authority, and was the owner
  quarantined after timeout or panic?

Keep short fallback summaries in host-facing runtime fields. Put verbose loader
or `dlopen` details in structured diagnostics.

## Validation

Pick commands based on touched crates:

```sh
cargo check -p player-plugin-abi -p player-plugin -p player-plugin-loader
cargo test -p player-plugin-abi -p player-plugin -p player-plugin-loader
cargo test -p player-plugin -p player-plugin-loader -p player-plugin-package
./scripts/vesper desktop verify-decoder-diagnostics debug all
./scripts/vesper desktop verify-decoder-videotoolbox debug loader
```

For remux plugins, also run the matching FFmpeg packaging and no-remux baseline
checks from `$vesper-ffmpeg-packaging`.
