# Vesper Player Native Plugin SDK

`vesper-player-plugin` is the safe Rust author SDK for Vesper Native plugins.
Use it to implement a `cdylib` that exports a Vesper plugin root without
writing the raw ABI or unsafe FFI glue yourself.

## Who Uses This Crate

This crate is for Rust developers creating a Native plugin. The usual workflow
starts with `vesper plugin new --transport native`, implements a capability,
then uses the Vesper CLI to build, inspect, check, sign, and package the shared
library.

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
player-plugin = { package = "vesper-player-plugin", version = "0.5" }
```

## Safe Native Export

Implement a capability, add it to `Plugin::builder`, and annotate a
zero-argument factory with `#[player_plugin::export]`:

```rust
use player_plugin::{
    PipelineEvent, PipelineEventHook, PipelineEventHookError,
    PipelineEventHookOutcome, Plugin, PluginBuildError,
};

struct Hook;

impl PipelineEventHook for Hook {
    fn on_event(
        &self,
        _event: &PipelineEvent,
    ) -> Result<PipelineEventHookOutcome, PipelineEventHookError> {
        Ok(PipelineEventHookOutcome::accepted())
    }
}

#[player_plugin::export]
fn plugin() -> Result<Plugin, PluginBuildError> {
    Plugin::builder("dev.example.analytics", "Example Analytics")?
        .with_pipeline_event_hook("dev.example.analytics.hook", Hook)?
        .build()
}
```

The macro emits the fixed `vesper_plugin_entry` export. Plugin and capability
instance IDs are validated as reverse-DNS identities, and the builder rejects
empty or duplicate interface definitions before the root is exported.

## Capability Surface

Stable Native capability families are `PostDownloadProcessor`,
`PipelineEventHook`, and `BenchmarkSink`. `NativeDecoder`, `FrameProcessor`,
`AudioProcessor`, packet `SourceNormalizer`, and resource `SourceNormalizer`
are experimental. A capability declaration alone does not make it available in
an Android, iOS, or Flutter host; each host route requires explicit support.

The crate also exposes bounded protocol DTOs, plugin references, metadata-only
catalog and resolver types, invocation policy, and runtime scope primitives for
host-side integrations that share the Native contract.

## Packaging and Safety Boundary

Use `vesper-player-plugin-package` or the `vesper plugin` CLI to create a
deterministic signed `.vesper-plugin` archive. Hosts should use
`vesper-player-plugin-loader` to validate and load the exported ABI.

Native plugins execute as native code. Package signatures establish publisher
identity and artifact integrity; they do not provide a sandbox. Plugins must
not expect DRM material, and public mobile hosts select build-time embedded
artifacts instead of accepting arbitrary runtime library paths.

The project templates and manifest schemas are in the
[Vesper repository](https://github.com/umbrella22/Vesper/tree/main/templates/vesper-plugin/native).
