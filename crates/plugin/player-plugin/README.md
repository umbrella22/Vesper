# Vesper Player Plugin SDK

`vesper-player-plugin` exposes safe Rust types for implementing native plugin ABI media
capabilities. The public C and C++ contract is
[`vesper_plugin.h`](../../../include/vesper_plugin.h).

## Rust author entry

Implement an `AudioProcessorFactory` and its processor from `player_plugin::author`.
A statically linked host can register the factory directly. A `cdylib` returns
`player_plugin::author::Plugin` from a synchronous zero-argument function annotated
with `#[player_plugin::export]`. The macro emits `vesper_native_plugin_entry` and
allows the author crate to use `#![deny(unsafe_code)]`.

The [Rust gain scaffold](../../../templates/vesper-plugin/rust-native/README.md)
contains a complete example. The equivalent
[C++ scaffold](../../../templates/vesper-plugin/cpp-cmake/README.md) uses the
same vtable and buffer contract. Use the matching workspace SDK path when
building those scaffolds against a local candidate.

## Audio buffers and distribution

A host with writable PCM can process it in place. Fixed-block processing also
accepts separate read-only input and writable output views. Streaming processors
return consumption counts and drain pending output after end of input.

Mobile hosts load bundled native artifacts from their platform package. The host
checks the plugin identity and capability records at admission. Native artifacts
execute in the host process. Playback events and benchmark observations use
ordinary host callbacks.

See the [author API](../../../lib/doc/plugin-author-api.md) and
[migration guide](../../../lib/doc/native-plugin-migration.md) for platform-specific
capabilities and the version cutover.
