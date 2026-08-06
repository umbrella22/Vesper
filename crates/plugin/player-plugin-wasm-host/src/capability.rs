use std::sync::Mutex;

use player_plugin::{
    BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, BenchmarkSinkReport,
    BenchmarkSinkStatus, PipelineEvent, PipelineEventHook, PipelineEventHookError,
    PipelineEventHookOutcome,
};

use crate::{
    WasmBenchmarkSinkSession, WasmPipelineEventHookSession, WasmPluginHostError,
    WasmPluginLogLevel, WasmPluginLogRecord, WasmPluginRuntime,
};

/// Thread-safe typed capability backed by one stateful WASM EventHook instance.
#[derive(Debug)]
pub struct WasmPipelineEventHookAdapter {
    session: Mutex<WasmPipelineEventHookSession>,
}

impl WasmPipelineEventHookAdapter {
    pub fn from_component_bytes(
        runtime: &WasmPluginRuntime,
        bytes: &[u8],
    ) -> Result<Self, WasmPluginHostError> {
        Ok(Self {
            session: Mutex::new(WasmPipelineEventHookSession::from_component_bytes(
                runtime, bytes,
            )?),
        })
    }
}

impl PipelineEventHook for WasmPipelineEventHookAdapter {
    fn on_event(
        &self,
        event: &PipelineEvent,
    ) -> Result<PipelineEventHookOutcome, PipelineEventHookError> {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = session.on_event(event).map_err(map_event_hook_error);
        let logs = session.take_logs();
        drop(session);
        emit_logs(logs);
        result
    }
}

/// Thread-safe typed capability backed by one stateful WASM BenchmarkSink instance.
#[derive(Debug)]
pub struct WasmBenchmarkSinkAdapter {
    name: String,
    session: Mutex<WasmBenchmarkSinkSession>,
}

impl WasmBenchmarkSinkAdapter {
    pub fn from_component_bytes(
        name: impl Into<String>,
        runtime: &WasmPluginRuntime,
        bytes: &[u8],
    ) -> Result<Self, WasmPluginHostError> {
        Ok(Self {
            name: name.into(),
            session: Mutex::new(WasmBenchmarkSinkSession::from_component_bytes(
                runtime, bytes,
            )?),
        })
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
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = session.on_event_batch(batch).map_err(map_benchmark_error);
        let logs = session.take_logs();
        drop(session);
        emit_logs(logs);
        result
    }

    fn flush(&self) -> Result<BenchmarkSinkReport, BenchmarkSinkError> {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = session.flush().map_err(map_benchmark_error);
        let logs = session.take_logs();
        drop(session);
        emit_logs(logs);
        result
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
