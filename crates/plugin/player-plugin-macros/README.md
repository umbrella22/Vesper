# Vesper Player Plugin Macros

`vesper-player-plugin-macros` provides the procedural macro that exports a
Rust Native plugin factory through Vesper's fixed `vesper_plugin_entry` symbol.

Most plugin authors should not depend on this crate directly. The public
`vesper-player-plugin` SDK re-exports the macro as `#[player_plugin::export]`
and supplies the safe `Plugin` builder used by the factory.

```rust
use player_plugin::{Plugin, PluginBuildError};

#[player_plugin::export]
fn plugin() -> Result<Plugin, PluginBuildError> {
    Plugin::builder("dev.example.plugin", "Example Plugin")?
        // Add one or more capability implementations here.
        .build()
}
```

The factory must be synchronous, non-generic, and take no arguments. It may
return `Plugin` directly or `Result<Plugin, E>`. The generated export delegates
to the safe SDK's ABI adapter, so an author crate can deny unsafe code.

This crate does not define plugin capabilities, package plugins, load shared
libraries, or validate publisher trust. Those responsibilities belong to
`vesper-player-plugin`, `vesper-player-plugin-package`, and
`vesper-player-plugin-loader`.

See the [Native plugin template](https://github.com/umbrella22/Vesper/tree/main/templates/vesper-plugin/native)
for a complete author project.
