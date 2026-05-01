use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One high-resolution benchmark event emitted by a host playback session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkEvent {
    pub run_id: String,
    pub session_id: String,
    pub platform: String,
    pub source_protocol: Option<String>,
    pub event_name: String,
    pub timestamp_ns: u64,
    pub elapsed_ns: u64,
    pub thread: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

/// Batch payload sent from the host to a benchmark sink plugin.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkEventBatch {
    pub events: Vec<BenchmarkEvent>,
}

/// Lightweight acknowledgement returned after a sink receives one event batch.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkSinkStatus {
    pub accepted_events: u64,
}

/// Final report returned by a benchmark sink when the host flushes a run.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkSinkReport {
    pub accepted_events: u64,
    pub dropped_events: u64,
    pub plugin_errors: Vec<String>,
}

/// Error payload shared by benchmark sink plugins and host-side adapters.
#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "code", content = "message")]
pub enum BenchmarkSinkError {
    #[error("payload codec error: {0}")]
    PayloadCodec(String),
    #[error("plugin ABI violation: {0}")]
    AbiViolation(String),
    #[error("sink failed: {0}")]
    SinkFailed(String),
}

pub trait BenchmarkSink: Send + Sync {
    fn name(&self) -> &str;

    fn on_event_batch(
        &self,
        batch: &BenchmarkEventBatch,
    ) -> Result<BenchmarkSinkStatus, BenchmarkSinkError>;

    fn flush(&self) -> Result<BenchmarkSinkReport, BenchmarkSinkError> {
        Ok(BenchmarkSinkReport::default())
    }
}
