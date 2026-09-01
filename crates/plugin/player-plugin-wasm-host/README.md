# Vesper Player WASM Host

`vesper-player-plugin-wasm-host` runs Vesper WASM Component plugins in a
bounded Wasmtime host. It is for desktop/tooling host authors who need to turn
verified component bytes into the transport-neutral `PipelineEventHook` or
`BenchmarkSink` traits from `vesper-player-plugin`.

## What It Provides

- `WasmPluginRuntime` configures the Component Model engine, fuel accounting,
  epoch interruption, and the allowed Vesper host imports.
- `WasmPipelineEventHookAdapter` and `WasmBenchmarkSinkAdapter` expose a
  thread-safe typed capability from component bytes.
- Session and queue APIs provide bounded event/batch queues, report draining,
  flush, close, structured log collection, and explicit timeout behavior.
- Component size, memory, table, instance, input, output, queue, and execution
  limits protect the host boundary. A trap, timeout, or invalid guest protocol
  result quarantines the affected session.

```rust
use player_plugin_wasm_host::{
    WasmPipelineEventHookAdapter, WasmPluginRuntime,
};

let runtime = WasmPluginRuntime::new()?;
let hook = WasmPipelineEventHookAdapter::from_component_bytes(&runtime, &component_bytes)?;
```

The host accepts only the fixed `vesper:plugin` WIT contract. Components may
receive bounded structured event or benchmark data and emit structured results
and logs. They do not receive filesystem, network, environment, process,
clock, random, media bytes, FFmpeg handles, or DRM material.

## Scope and Deployment

WASM transport currently serves desktop and tooling workflows. Mobile hosts
embed verified Native plugin artifacts at build time and reject the WASM
transport.

This crate does not discover packages, verify signatures, resolve dependencies,
or make deployment policy decisions. Use `vesper-player-plugin-package` for
signed package and trust-store verification, and
`vesper-player-plugin-loader` with its `wasm` feature to connect verified
catalog entries to this host.

The canonical WIT contract is maintained in the
[Vesper repository](https://github.com/umbrella22/Vesper/tree/main/wit).
