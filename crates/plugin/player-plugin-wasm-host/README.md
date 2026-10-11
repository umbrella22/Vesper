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
  limits protect the host boundary. A guest trap, exhausted execution budget, or
  invalid guest protocol result quarantines the affected session. Caller wait
  expiry does not.

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

## Receipts, Errors, and Settlement

`queue.submit(event_or_batch)` returns a `WasmInvocationTicket` for accepted work.
`ticket.invocation_id()` identifies that work within its queue; consuming
`ticket.wait(timeout)` returns a completion report whose `result` contains the
actual guest result. `submit_and_wait` combines those steps. A wait expiry or
an abandoned ticket does not cancel execution. Eventual reports retain their ID
and enter a bounded sink; newest-report loss is counted independently of settlement.

`flush` captures worker-time FIFO settlement. Benchmark flush additionally calls
the guest and caches a completed report if its caller expired. The next flush
retrieves that original report. `settlement()` returns current accounting;
`close` drains work, releases the guest, and joins the worker within one deadline.
Closed state alone does not prove `worker_joined`. `Drop` requests close and
detaches without certifying completion.

`WasmPluginHostError` distinguishes local validation, guest-declared failures,
execution, worker panic, queue state, caller wait expiry, protocol violations,
and reason-bearing quarantine. `WasmExecutionFailure` preserves the original
typed Wasmtime cause and classifies fuel, deadline, guest trap, host call, or
unknown failures without parsing text. `quarantine_reason()` exposes the retained
session failure. Transport-neutral SDK adapters format these errors; low-level
sessions/queues preserve typed inspection.

Queue inputs are validated once into private immutable checked values. Raw
session APIs still validate external input. Guest results retain complete semantic,
size, count, duplicate-key, and actual-batch acknowledgement validation.

The [plugin author API contract](../../../lib/doc/plugin-author-api.md) defines
the current receipt, counter, diagnostic, and release contracts, including
the 0.9.x source API and the matching package-consumption gate. Canonical
WIT, Native ABI, and mobile DTOs retain their existing wire contracts.

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
