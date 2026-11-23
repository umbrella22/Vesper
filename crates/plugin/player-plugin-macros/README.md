# Vesper Player Plugin Macros

`vesper-player-plugin-macros` generates the `vesper_native_plugin_entry` export for
Rust plugins. The `vesper-player-plugin` SDK re-exports it as
`#[player_plugin::export]`.

The annotated function is synchronous, non-generic, takes zero arguments,
and returns `player_plugin::author::Plugin`. The generated entry delegates root
validation and panic containment to the SDK adapter. Author crates can use
`#![deny(unsafe_code)]`.

The [Rust gain scaffold](../../../templates/vesper-plugin/rust-native/README.md)
contains a complete author project. The [author API](../../../lib/doc/plugin-author-api.md)
describes ownership and platform admission.
