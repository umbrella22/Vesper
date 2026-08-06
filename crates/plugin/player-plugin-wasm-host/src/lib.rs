#![deny(unsafe_code)]

mod benchmark;
mod bindings;
mod capability;
mod event_hook;
mod host_state;
mod runtime;

pub use benchmark::{
    WASM_PLUGIN_BENCHMARK_BATCH_QUEUE_CAPACITY, WASM_PLUGIN_BENCHMARK_REPORT_QUEUE_CAPACITY,
    WasmBenchmarkBatchEnqueueStatus, WasmBenchmarkSinkBatchReport, WasmBenchmarkSinkFlushReport,
    WasmBenchmarkSinkQueue, WasmBenchmarkSinkQueueReportBatch, WasmBenchmarkSinkSession,
};
#[doc(hidden)]
pub use bindings::VESPER_PLUGIN_WIT;
pub use capability::{WasmBenchmarkSinkAdapter, WasmPipelineEventHookAdapter};
pub use event_hook::{
    WASM_PLUGIN_EVENT_QUEUE_CAPACITY, WASM_PLUGIN_EVENT_REPORT_QUEUE_CAPACITY,
    WasmPipelineEventEnqueueStatus, WasmPipelineEventHookQueue, WasmPipelineEventHookReport,
    WasmPipelineEventHookReportBatch, WasmPipelineEventHookSession,
};
pub use host_state::{WasmPluginLogLevel, WasmPluginLogRecord};
pub use runtime::{WasmPluginHostError, WasmPluginRuntime, WasmPluginRuntimeError};

pub const MAX_WASM_PLUGIN_COMPONENT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_WASM_PLUGIN_INPUT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_WASM_PLUGIN_OUTPUT_BYTES: usize = 256 * 1024;
/// Version of the fixed `vesper:plugin` Component Model contract.
pub const WASM_PLUGIN_WIT_INTERFACE_MAJOR: u16 = 1;
/// Minor version of the fixed `vesper:plugin` Component Model contract.
pub const WASM_PLUGIN_WIT_INTERFACE_MINOR: u16 = 0;
pub const WASM_PLUGIN_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
pub const WASM_PLUGIN_MEMORY_COUNT_LIMIT: usize = 1;
pub const WASM_PLUGIN_INSTANCE_LIMIT: usize = 32;
pub const WASM_PLUGIN_TABLE_LIMIT: usize = 16;
pub const WASM_PLUGIN_TABLE_ELEMENT_LIMIT: usize = 16 * 1024;
pub const WASM_PLUGIN_INVOCATION_FUEL: u64 = 10_000_000;
pub const WASM_PLUGIN_HOSTCALL_FUEL: usize = 256 * 1024;
pub const WASM_PLUGIN_EPOCH_TICK_MILLIS: u64 = 5;
pub const WASM_PLUGIN_EVENT_TIMEOUT_MILLIS: u64 = 50;
pub const WASM_PLUGIN_BATCH_TIMEOUT_MILLIS: u64 = 250;
pub const WASM_PLUGIN_FLUSH_TIMEOUT_MILLIS: u64 = 2_000;
