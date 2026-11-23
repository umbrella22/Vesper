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
2. The plugin exports `vesper_native_plugin_entry` returning a `VesperPluginRoot`.
3. The loader validates the root and queries typed interfaces by stable
   interface ID; `plugin_kind` is diagnostic classification only and never
   selects an ABI table.
4. `player_plugin_loader::native` exposes the checked capability.
5. The hot path holds a checked wrapper or trait object, not repeated `Option`
   function pointer checks.

When the change is runtime-facing, follow the separate catalog -> resolver ->
plan -> scope phases from `plugin-runtime-contract.md`. Never let a loader call
mutate a catalog or plan, and never use a live owner as catalog metadata.

Do not scatter "missing required function" checks across runtime calls. Validate
once at load/open time, then rely on the checked wrapper.

Rust plugins use `player_plugin::author` and `#[player_plugin::export]`.
C99 and C++17 plugins use the generated `include/vesper_plugin.h`.
Playback events and benchmark observations use host-language callbacks.

## Extension Point Judgment

Prefer host-safe and runtime-safe extension points for new feature requests:

- source resolver
- event listener
- subtitle discovery or sidecar loading
- post-download processor
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
- Keep the root ABI major and `struct_size` validation. Extend tables by
  appending fields and preserve existing callback signatures and ownership.
- Plugin-driven loops that skip packets, poll callbacks, retry reads, or drain
  events must have bounded iterations or timeout-backed diagnostics.
- Loader-side plugin calls should treat panic or ABI violation as a poisoned
  plugin instance unless the ABI explicitly proves reuse is safe.
- Plugin activation requires a native artifact implementing the declared
  capability. The SDK reports unsupported transports as typed failures.

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
cargo check -p vesper-player-plugin-abi -p vesper-player-plugin -p vesper-player-plugin-loader
cargo test -p vesper-player-plugin-abi -p vesper-player-plugin -p vesper-player-plugin-loader
./scripts/vesper plugin header-verify
./scripts/vesper desktop verify-decoder-videotoolbox loader
```

For remux plugins, also run the matching FFmpeg packaging and no-remux baseline
checks from `$vesper-ffmpeg-packaging`.
