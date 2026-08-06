use wasmtime::StoreLimits;
use wasmtime::StoreLimitsBuilder;

use crate::bindings::{benchmark_sink, event_hook};
use crate::{
    WASM_PLUGIN_INSTANCE_LIMIT, WASM_PLUGIN_MEMORY_COUNT_LIMIT, WASM_PLUGIN_MEMORY_LIMIT_BYTES,
    WASM_PLUGIN_TABLE_ELEMENT_LIMIT, WASM_PLUGIN_TABLE_LIMIT,
};

const MAX_HOST_LOG_RECORDS_PER_CALL: usize = 64;
const MAX_HOST_LOG_CODE_BYTES: usize = 64;
const MAX_HOST_LOG_MESSAGE_BYTES: usize = 256;
const MAX_HOST_LOG_BYTES_PER_CALL: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPluginLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmPluginLogRecord {
    pub level: WasmPluginLogLevel,
    pub code: String,
    pub message: String,
}

#[derive(Debug)]
pub(crate) struct WasmHostState {
    pub(crate) limits: StoreLimits,
    logs: Vec<WasmPluginLogRecord>,
    log_bytes: usize,
}

impl WasmHostState {
    pub(crate) fn new() -> Self {
        Self {
            limits: StoreLimitsBuilder::new()
                .memory_size(WASM_PLUGIN_MEMORY_LIMIT_BYTES)
                .memories(WASM_PLUGIN_MEMORY_COUNT_LIMIT)
                .instances(WASM_PLUGIN_INSTANCE_LIMIT)
                .tables(WASM_PLUGIN_TABLE_LIMIT)
                .table_elements(WASM_PLUGIN_TABLE_ELEMENT_LIMIT)
                .trap_on_grow_failure(true)
                .build(),
            logs: Vec::new(),
            log_bytes: 0,
        }
    }

    pub(crate) fn begin_call(&mut self) {
        self.logs.clear();
        self.log_bytes = 0;
    }

    pub(crate) fn take_logs(&mut self) -> Vec<WasmPluginLogRecord> {
        std::mem::take(&mut self.logs)
    }

    fn push_log(
        &mut self,
        level: WasmPluginLogLevel,
        code: String,
        message: String,
    ) -> wasmtime::Result<()> {
        if code.is_empty() || code.len() > MAX_HOST_LOG_CODE_BYTES {
            return Err(wasmtime::format_err!(
                "host.log code must contain 1 to {MAX_HOST_LOG_CODE_BYTES} UTF-8 bytes"
            ));
        }
        if message.is_empty() || message.len() > MAX_HOST_LOG_MESSAGE_BYTES {
            return Err(wasmtime::format_err!(
                "host.log message must contain 1 to {MAX_HOST_LOG_MESSAGE_BYTES} UTF-8 bytes"
            ));
        }
        if self.logs.len() >= MAX_HOST_LOG_RECORDS_PER_CALL {
            return Err(wasmtime::format_err!(
                "host.log exceeds the {MAX_HOST_LOG_RECORDS_PER_CALL}-record call limit"
            ));
        }
        let added_bytes = code
            .len()
            .checked_add(message.len())
            .ok_or_else(|| wasmtime::format_err!("host.log byte count overflow"))?;
        let log_bytes = self
            .log_bytes
            .checked_add(added_bytes)
            .ok_or_else(|| wasmtime::format_err!("host.log byte count overflow"))?;
        if log_bytes > MAX_HOST_LOG_BYTES_PER_CALL {
            return Err(wasmtime::format_err!(
                "host.log exceeds the {MAX_HOST_LOG_BYTES_PER_CALL}-byte call limit"
            ));
        }
        self.log_bytes = log_bytes;
        self.logs.push(WasmPluginLogRecord {
            level,
            code,
            message,
        });
        Ok(())
    }
}

impl event_hook::vesper::plugin::host::Host for WasmHostState {
    fn log(
        &mut self,
        level: event_hook::vesper::plugin::host::LogLevel,
        code: String,
        message: String,
    ) -> wasmtime::Result<()> {
        self.push_log(event_log_level(level), code, message)
    }
}

impl event_hook::vesper::plugin::protocol::Host for WasmHostState {}

impl benchmark_sink::vesper::plugin::host::Host for WasmHostState {
    fn log(
        &mut self,
        level: benchmark_sink::vesper::plugin::host::LogLevel,
        code: String,
        message: String,
    ) -> wasmtime::Result<()> {
        self.push_log(benchmark_log_level(level), code, message)
    }
}

impl benchmark_sink::vesper::plugin::protocol::Host for WasmHostState {}

fn event_log_level(level: event_hook::vesper::plugin::host::LogLevel) -> WasmPluginLogLevel {
    match level {
        event_hook::vesper::plugin::host::LogLevel::Trace => WasmPluginLogLevel::Trace,
        event_hook::vesper::plugin::host::LogLevel::Debug => WasmPluginLogLevel::Debug,
        event_hook::vesper::plugin::host::LogLevel::Info => WasmPluginLogLevel::Info,
        event_hook::vesper::plugin::host::LogLevel::Warn => WasmPluginLogLevel::Warn,
        event_hook::vesper::plugin::host::LogLevel::Error => WasmPluginLogLevel::Error,
    }
}

fn benchmark_log_level(
    level: benchmark_sink::vesper::plugin::host::LogLevel,
) -> WasmPluginLogLevel {
    match level {
        benchmark_sink::vesper::plugin::host::LogLevel::Trace => WasmPluginLogLevel::Trace,
        benchmark_sink::vesper::plugin::host::LogLevel::Debug => WasmPluginLogLevel::Debug,
        benchmark_sink::vesper::plugin::host::LogLevel::Info => WasmPluginLogLevel::Info,
        benchmark_sink::vesper::plugin::host::LogLevel::Warn => WasmPluginLogLevel::Warn,
        benchmark_sink::vesper::plugin::host::LogLevel::Error => WasmPluginLogLevel::Error,
    }
}
