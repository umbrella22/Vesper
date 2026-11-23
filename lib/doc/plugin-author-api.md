# Plugin author and host API contracts

Native dynamic libraries export `vesper_native_plugin_entry`.
The C99 and C++17 layout is defined by [`vesper_plugin.h`](../../include/vesper_plugin.h).
The [migration guide](native-plugin-migration.md) covers rebuilding existing plugins.

## Implement and register a capability

- **Rust:** implement the capability in `player_plugin::author` and construct a
  `Plugin` with the corresponding factory. A linked Rust host can use the
  factory directly. A Rust `cdylib` uses `#[player_plugin::export]` on a
  synchronous zero-argument factory that returns `player_plugin::author::Plugin`.
- **C and C++:** implement the vtable from `vesper_plugin.h` and return the plugin
  root from `vesper_native_plugin_entry`. Initialize `struct_size`, advertise only
  implemented interfaces, and declare C++ callbacks `noexcept`.
- **Shaders:** compile the reference Slang filter to the target artifact during
  the plugin build. The runtime receives the compiled shader and the native
  GPU context. A third-party plugin can supply its own shader language and tools.

The [Rust gain scaffold](../../templates/vesper-plugin/rust-native/README.md)
and [C++17 gain scaffold](../../templates/vesper-plugin/cpp-cmake/README.md)
implement the same `AudioProcessor` capability. Both support fixed-size in-place
and split processing. In split processing, input stays read-only and output
uses distinct writable storage. Both preserve the negotiated format, media time,
and playback-rate epoch.

## Frame and packet ownership

A decoder issues a frame lease and supplies its release callback with the frame.
The frame's backing storage stays valid until release, including when the host
holds a frame through seek or decoder close. GPU submissions retire on their
completion signal. A seek invalidates presentation in the previous epoch, while
completion still returns its lease exactly once. A closed session can reject
new calls without invalidating the independent release context for older frames.

A packet normalizer negotiates stream metadata before yielding compressed packet
leases. The host may retry the same packet while a decoder reports backpressure.
A resource normalizer publishes native-player resources under a separate
capability. Each route has its own admission, ownership, and cleanup contract.

## Host observability and platform admission

Playback events and benchmark observations are delivered through host-language
listeners or in-process Rust callbacks.

Mobile hosts admit plugins from installed platform artifacts with explicit
capability references. Android Media3 permits optional fixed-format block DSP
using read-only input and writable output. The iOS and HarmonyOS DirectNative
AVPlayer routes reject custom DSP. Protected playback uses the platform DRM route
and keeps protected packets, frames, and PCM outside the plugin pipeline.

## Verification

Run `./scripts/vesper plugin header-verify` to compare the generated header with
the Rust layout and compile it as C99 and C++17. Build the scaffold outside the
Vesper workspace to verify that it resolves and exports the same ABI independently.
Verify media, device, and GPU behavior on each host that enables the capability.
