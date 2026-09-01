# Vesper Player WASM Guest SDK

`vesper-player-plugin-wasm` is the Rust guest runtime for authors of Vesper
WASM Component plugins. It supplies the `wit-bindgen` integration, allocator,
panic behavior, and canonical ABI reallocation required to export the fixed
Vesper Component Model contract from a safe Rust guest.

Use this crate when writing a plugin that runs as `wasm32-wasip2`. It is not a
media player, a Wasmtime host, or a general-purpose WASI plugin framework.

## Supported Workloads

WASM Components can implement only the stable, structured-data capabilities:

- `PipelineEventHook` for observing or responding to bounded pipeline events.
- `BenchmarkSink` for accepting bounded benchmark event batches and returning
  structured reports.

The guest has no filesystem, network, environment, process, clock, random, or
media-byte imports. Native decoding, frame processing, audio processing,
SourceNormalizer, post-download processing, DRM, and mobile WASM execution are
outside this transport contract.

## Start a Component

The CLI creates a project with the matching WIT files and Cargo configuration:

```sh
cargo install vesper-player-cli --locked
vesper plugin new ./analytics \
  --plugin-id dev.example.analytics \
  --publisher dev.example \
  --license Apache-2.0 \
  --transport wasm \
  --capability event-hook
```

An author crate normally uses this dependency shape:

```toml
[dependencies]
player-plugin-wasm = { package = "vesper-player-plugin-wasm", version = "0.5" }
```

Its source generates bindings from the supplied WIT world and exports the
component through this crate:

```rust
#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

mod bindings {
    player_plugin_wasm::generate!({
        path: "wit",
        world: "event-hook-plugin",
        runtime_path: "player_plugin_wasm::rt",
    });
}

struct Component;

// Implement the generated Guest trait for Component.

player_plugin_wasm::export_component!(bindings, Component);
```

`export_component!` accepts the generated bindings module and guest type. It
requires the `wasm32-wasip2` target and rejects the WASM atomics target feature.

## Host Relationship

`vesper-player-plugin-wasm-host` runs generated components under bounded
Wasmtime settings. `vesper-player-plugin-loader` connects verified catalog
records to that host. Use the CLI to build, inspect, check, sign, and package a
component; the guest crate only provides the author-side runtime boundary.

The canonical WIT contract is available in the
[Vesper repository](https://github.com/umbrella22/Vesper/tree/main/wit).
