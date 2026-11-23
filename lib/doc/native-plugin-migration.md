# Native plugin migration

Vesper 0.11 changes the native plugin ABI. Applications upgrading from the
0.10 series must replace their packaged native plugin binaries.

Native libraries export `vesper_native_plugin_entry` and return a
`VesperPluginRoot` from [the generated header](../../include/vesper_plugin.h).
Dynamic loaders resolve only that symbol. Both dynamic and linked registration
check `VESPER_PLUGIN_ABI_MAJOR` and the root's `struct_size`. Plugins built against
earlier headers must be rebuilt with the current author API and repackaged before
deployment.

| Authoring route | Build contract |
| --- | --- |
| Rust | Implement a factory in `player_plugin::author` and apply `#[player_plugin::export]` to a synchronous function returning `Plugin`. |
| C or C++ | Include `vesper_plugin.h`, initialize the root and vtable sizes, and export `vesper_native_plugin_entry`. C++ callbacks use `noexcept`. |
| Linked Rust | Register the factory directly through the host's Rust registration API. |

The [author contract](plugin-author-api.md) describes capabilities, buffer
ownership, and platform admission. `vesper plugin check` validates the native
artifact against its manifest. Mobile applications package native plugins at
build time and admit them through the installed platform artifact.

Replace FrameProcessor plugins with the `VideoFilter` interface for GPU
filtering. Replace EventHook integrations with host-language listeners or
in-process Rust callbacks for playback events and benchmark samples. Remove
references to the standalone performance diagnostics packages from application
manifests and use the host kit callbacks.

FFmpeg-backed plugins retain the applicable FFmpeg redistribution requirements.
