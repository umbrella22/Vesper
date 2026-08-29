use std::time::Duration;

use player_plugin::{
    BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, BenchmarkSinkReport,
    BenchmarkSinkStatus, PipelineEvent, PipelineEventHook, PipelineEventHookError,
    PipelineEventHookOutcome,
};

use crate::{
    WASM_PLUGIN_BENCHMARK_RESULT_WAIT_MILLIS, WASM_PLUGIN_EVENT_RESULT_WAIT_MILLIS,
    WASM_PLUGIN_FLUSH_TIMEOUT_MILLIS, WasmBenchmarkSinkQueue, WasmPipelineEventHookQueue,
    WasmPluginHostError, WasmPluginLogLevel, WasmPluginLogRecord, WasmPluginRuntime,
};

/// Thread-safe typed capability backed by one stateful WASM EventHook instance.
#[derive(Debug)]
pub struct WasmPipelineEventHookAdapter {
    queue: WasmPipelineEventHookQueue,
}

impl WasmPipelineEventHookAdapter {
    pub fn from_component_bytes(
        runtime: &WasmPluginRuntime,
        bytes: &[u8],
    ) -> Result<Self, WasmPluginHostError> {
        Ok(Self {
            queue: WasmPipelineEventHookQueue::from_component_bytes(runtime, bytes)?,
        })
    }

    pub fn close(&self, timeout: Duration) -> Result<(), WasmPluginHostError> {
        self.queue.close(timeout)
    }
}

impl PipelineEventHook for WasmPipelineEventHookAdapter {
    fn on_event(
        &self,
        event: &PipelineEvent,
    ) -> Result<PipelineEventHookOutcome, PipelineEventHookError> {
        let report = self
            .queue
            .invoke(
                event.clone(),
                Duration::from_millis(WASM_PLUGIN_EVENT_RESULT_WAIT_MILLIS),
            )
            .map_err(map_event_hook_error)?;
        emit_logs(report.logs);
        report.result.map_err(map_event_hook_error)
    }
}

/// Thread-safe typed capability backed by one stateful WASM BenchmarkSink instance.
#[derive(Debug)]
pub struct WasmBenchmarkSinkAdapter {
    name: String,
    queue: WasmBenchmarkSinkQueue,
}

impl WasmBenchmarkSinkAdapter {
    pub fn from_component_bytes(
        name: impl Into<String>,
        runtime: &WasmPluginRuntime,
        bytes: &[u8],
    ) -> Result<Self, WasmPluginHostError> {
        Ok(Self {
            name: name.into(),
            queue: WasmBenchmarkSinkQueue::from_component_bytes(runtime, bytes)?,
        })
    }

    pub fn close(&self, timeout: Duration) -> Result<BenchmarkSinkReport, WasmPluginHostError> {
        let report = self.queue.close(timeout)?;
        emit_logs(report.logs);
        report.result
    }
}

impl BenchmarkSink for WasmBenchmarkSinkAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn on_event_batch(
        &self,
        batch: &BenchmarkEventBatch,
    ) -> Result<BenchmarkSinkStatus, BenchmarkSinkError> {
        batch.validate()?;
        let report = self
            .queue
            .submit(
                batch.clone(),
                Duration::from_millis(WASM_PLUGIN_BENCHMARK_RESULT_WAIT_MILLIS),
            )
            .map_err(map_benchmark_error)?;
        emit_logs(report.logs);
        report.result.map_err(map_benchmark_error)
    }

    fn flush(&self) -> Result<BenchmarkSinkReport, BenchmarkSinkError> {
        let report = self
            .queue
            .flush(Duration::from_millis(WASM_PLUGIN_FLUSH_TIMEOUT_MILLIS))
            .map_err(map_benchmark_error)?;
        emit_logs(report.logs);
        report.result.map_err(map_benchmark_error)
    }
}

fn map_event_hook_error(error: WasmPluginHostError) -> PipelineEventHookError {
    match error {
        WasmPluginHostError::InvalidInput(message) => PipelineEventHookError::InvalidInput(message),
        WasmPluginHostError::Rejected(message) => PipelineEventHookError::Rejected(message),
        WasmPluginHostError::PluginFailed(message) => PipelineEventHookError::Failed(message),
        WasmPluginHostError::ProtocolViolation(message) => {
            PipelineEventHookError::ProtocolViolation(message)
        }
        other => PipelineEventHookError::AbiViolation(other.to_string()),
    }
}

fn map_benchmark_error(error: WasmPluginHostError) -> BenchmarkSinkError {
    match error {
        WasmPluginHostError::InvalidInput(message)
        | WasmPluginHostError::Rejected(message)
        | WasmPluginHostError::PluginFailed(message) => BenchmarkSinkError::SinkFailed(message),
        WasmPluginHostError::ProtocolViolation(message) => {
            BenchmarkSinkError::ProtocolViolation(message)
        }
        other => BenchmarkSinkError::AbiViolation(other.to_string()),
    }
}

fn emit_logs(logs: Vec<WasmPluginLogRecord>) {
    for log in logs {
        match log.level {
            WasmPluginLogLevel::Trace => {
                tracing::trace!(code = %log.code, message = %log.message, "WASM plugin log")
            }
            WasmPluginLogLevel::Debug => {
                tracing::debug!(code = %log.code, message = %log.message, "WASM plugin log")
            }
            WasmPluginLogLevel::Info => {
                tracing::info!(code = %log.code, message = %log.message, "WASM plugin log")
            }
            WasmPluginLogLevel::Warn => {
                tracing::warn!(code = %log.code, message = %log.message, "WASM plugin log")
            }
            WasmPluginLogLevel::Error => {
                tracing::error!(code = %log.code, message = %log.message, "WASM plugin log")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_author_invalid_input_remains_an_author_failure() {
        assert_eq!(
            map_benchmark_error(WasmPluginHostError::InvalidInput(
                "guest rejected the batch".to_owned(),
            )),
            BenchmarkSinkError::SinkFailed("guest rejected the batch".to_owned())
        );
    }
}
