# Vesper Plugin Contract

This card is the bundled reference for the current Vesper plugin platform.
The implementation and tests in the checkout remain authoritative.

## Native entry and capabilities

- Rust plugins use `player_plugin::author` and `#[player_plugin::export]`.
  C99 and C++17 plugins use the generated `include/vesper_plugin.h`.
- `vesper_native_plugin_entry` returns a `VesperPluginRoot`. The loader
  validates the ABI major and root size, then queries size-tagged vtables by
  stable interface ID. The generated header defines the current IDs.
- Native interfaces include video decoding, GPU video filtering, audio
  processing, packet and resource normalization, and post-download processing.
  Playback events and benchmark samples use host callbacks.
- Native signatures validate source and integrity. The host executes trusted
  native callbacks in process.

## Session and lease lifetime

- Native sessions use exclusive access. Factories may support concurrent calls.
- Borrowed packet data stays valid until its lease is returned. Frame leases
  can outlive the decoder session while GPU work holds the release context.
- The final reference to a root invokes its optional `destroy_owner` callback.
  Dynamic code remains mapped for process lifetime.
- The loader checks status, sizes and progress at the ABI boundary. Preserve
  unknown status and capability values in diagnostics.

## Runtime Selection Boundary

The runtime phases are documented in `plugin-runtime-contract.md`:

1. `PluginCatalogImporter` validates metadata and artifact digests into a
   `PluginCatalogIndex` without opening executable code.
2. `PluginResolver` applies host transport, target, architecture, ABI, semver,
   and priority policy to produce a deterministic `PluginResolution`.
3. `PluginPlan` stores the canonical catalog projection, selected providers,
   dependency-first artifacts, and catalog/plan fingerprints.
4. `PluginRuntime` activates the immutable plan under hierarchical scopes and
   only then creates checked loader capabilities.

Catalog records and plans contain no live owners, handles, workers, queues,
callbacks, or media. A changed catalog or host policy requires a new plan and
runtime generation.

## Artifact admission

`PluginReference` binds plugin identity and capability selection. Artifact
admission requires the native transport. Metadata-only catalog operations
inspect records without loading executable code. `vesper plugin check` validates
one concrete native artifact against the declared contract.

`AudioProcessorFactory` opens an `AudioProcessor` with a negotiated PCM format.
Fixed blocks borrow exclusive PCM or separate read-only input and writable
output. Buffered processing reports partial consumption and returns one rate
epoch per output block. The host supplies media time and schedules output.
Media3 can use optional fixed-block DSP. AVPlayer DirectNative routes reject
custom DSP.

## Mobile And Security Non-Goals

- Android and iOS production playback remains Media3 and AVPlayer.
- Mobile plugins are build-time embedded; runtime code download is not allowed.
- Plugins never receive protected media, Widevine/FairPlay material, headers,
  tokens, or raw sensitive URLs.
- `inspect` and `check` worker processes provide tool-level crash isolation,
  not a runtime sandbox.
