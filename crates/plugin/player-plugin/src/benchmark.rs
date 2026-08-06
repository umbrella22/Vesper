use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    MAX_PLUGIN_DIAGNOSTICS, MAX_PLUGIN_EVENT_ID_BYTES, MAX_PLUGIN_EVENT_NAME_BYTES,
    MAX_PLUGIN_MEASUREMENTS, MAX_PLUGIN_PLATFORM_BYTES, MAX_PLUGIN_PROTOCOL_BYTES,
    MAX_PLUGIN_THREAD_BYTES, PluginDiagnostic, PluginMeasurement, PluginProtocolViolation,
    protocol::{validate_attributes, validate_optional_text, validate_text},
};

pub const MAX_BENCHMARK_BATCH_EVENTS: usize = 512;
pub const MAX_BENCHMARK_THRESHOLD_VIOLATIONS: usize = 128;

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

impl BenchmarkEvent {
    pub fn validate(&self) -> Result<(), BenchmarkSinkError> {
        validate_text("benchmark.run_id", &self.run_id, MAX_PLUGIN_EVENT_ID_BYTES)?;
        validate_text(
            "benchmark.session_id",
            &self.session_id,
            MAX_PLUGIN_EVENT_ID_BYTES,
        )?;
        validate_text(
            "benchmark.platform",
            &self.platform,
            MAX_PLUGIN_PLATFORM_BYTES,
        )?;
        validate_optional_text(
            "benchmark.source_protocol",
            self.source_protocol.as_deref(),
            MAX_PLUGIN_PROTOCOL_BYTES,
        )?;
        validate_text(
            "benchmark.event_name",
            &self.event_name,
            MAX_PLUGIN_EVENT_NAME_BYTES,
        )?;
        validate_optional_text(
            "benchmark.thread",
            self.thread.as_deref(),
            MAX_PLUGIN_THREAD_BYTES,
        )?;
        validate_attributes(&self.attributes)?;
        Ok(())
    }
}

/// Batch payload sent from the host to a benchmark sink plugin.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkEventBatch {
    pub events: Vec<BenchmarkEvent>,
}

impl BenchmarkEventBatch {
    pub fn validate(&self) -> Result<(), BenchmarkSinkError> {
        if self.events.len() > MAX_BENCHMARK_BATCH_EVENTS {
            return Err(BenchmarkSinkError::ProtocolViolation(format!(
                "benchmark batch exceeds the {MAX_BENCHMARK_BATCH_EVENTS}-event protocol limit"
            )));
        }
        for event in &self.events {
            event.validate()?;
        }
        Ok(())
    }
}

/// Lightweight acknowledgement returned after a sink receives one event batch.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkSinkStatus {
    pub accepted_events: u64,
}

impl BenchmarkSinkStatus {
    pub fn validate_for_batch(&self, batch_event_count: usize) -> Result<(), BenchmarkSinkError> {
        let batch_event_count = u64::try_from(batch_event_count).map_err(|_| {
            BenchmarkSinkError::ProtocolViolation(
                "benchmark batch size cannot be represented by the protocol".to_owned(),
            )
        })?;
        if self.accepted_events > batch_event_count {
            return Err(BenchmarkSinkError::ProtocolViolation(format!(
                "benchmark sink accepted {} events from a {batch_event_count}-event batch",
                self.accepted_events
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkThresholdViolation {
    pub measurement: String,
    pub actual: f64,
    pub threshold: f64,
    pub comparison: String,
}

/// Final report returned by a benchmark sink when the host flushes a run.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkSinkReport {
    pub accepted_events: u64,
    pub dropped_events: u64,
    #[serde(default)]
    pub measurements: Vec<PluginMeasurement>,
    #[serde(default)]
    pub threshold_violations: Vec<BenchmarkThresholdViolation>,
    #[serde(default)]
    pub diagnostics: Vec<PluginDiagnostic>,
}

impl BenchmarkSinkReport {
    pub fn validate(&self) -> Result<(), BenchmarkSinkError> {
        if self.measurements.len() > MAX_PLUGIN_MEASUREMENTS {
            return Err(BenchmarkSinkError::ProtocolViolation(format!(
                "benchmark report exceeds the {MAX_PLUGIN_MEASUREMENTS}-measurement protocol limit"
            )));
        }
        if self.diagnostics.len() > MAX_PLUGIN_DIAGNOSTICS {
            return Err(BenchmarkSinkError::ProtocolViolation(format!(
                "benchmark report exceeds the {MAX_PLUGIN_DIAGNOSTICS}-diagnostic protocol limit"
            )));
        }
        if self.threshold_violations.len() > MAX_BENCHMARK_THRESHOLD_VIOLATIONS {
            return Err(BenchmarkSinkError::ProtocolViolation(format!(
                "benchmark report exceeds the {MAX_BENCHMARK_THRESHOLD_VIOLATIONS}-threshold-violation protocol limit"
            )));
        }
        for measurement in &self.measurements {
            measurement.validate()?;
        }
        for diagnostic in &self.diagnostics {
            diagnostic.validate()?;
        }
        for violation in &self.threshold_violations {
            validate_text(
                "threshold_violation.measurement",
                &violation.measurement,
                MAX_PLUGIN_EVENT_NAME_BYTES,
            )?;
            validate_text(
                "threshold_violation.comparison",
                &violation.comparison,
                MAX_PLUGIN_EVENT_NAME_BYTES,
            )?;
            if !violation.actual.is_finite() || !violation.threshold.is_finite() {
                return Err(BenchmarkSinkError::ProtocolViolation(format!(
                    "threshold violation `{}` contains a non-finite value",
                    violation.measurement
                )));
            }
        }
        Ok(())
    }
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
    #[error("sink protocol violation: {0}")]
    ProtocolViolation(String),
}

impl From<PluginProtocolViolation> for BenchmarkSinkError {
    fn from(value: PluginProtocolViolation) -> Self {
        Self::ProtocolViolation(value.to_string())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_limit_is_part_of_the_protocol() {
        let event = BenchmarkEvent {
            run_id: "run".to_owned(),
            session_id: "session".to_owned(),
            platform: "test".to_owned(),
            source_protocol: None,
            event_name: "tick".to_owned(),
            timestamp_ns: 0,
            elapsed_ns: 0,
            thread: None,
            attributes: BTreeMap::new(),
        };
        let batch = BenchmarkEventBatch {
            events: vec![event; MAX_BENCHMARK_BATCH_EVENTS + 1],
        };
        assert!(matches!(
            batch.validate(),
            Err(BenchmarkSinkError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn sink_status_cannot_accept_more_events_than_the_input_batch() {
        let status = BenchmarkSinkStatus { accepted_events: 2 };
        assert!(matches!(
            status.validate_for_batch(1),
            Err(BenchmarkSinkError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn benchmark_events_validate_all_transport_text_fields() {
        let event = BenchmarkEvent {
            run_id: String::new(),
            session_id: "session".to_owned(),
            platform: "test".to_owned(),
            source_protocol: None,
            event_name: "tick".to_owned(),
            timestamp_ns: 0,
            elapsed_ns: 0,
            thread: None,
            attributes: BTreeMap::new(),
        };

        assert!(matches!(
            event.validate(),
            Err(BenchmarkSinkError::ProtocolViolation(message))
                if message.contains("benchmark.run_id")
        ));
    }

    #[test]
    fn benchmark_reports_bound_threshold_violations() {
        let violation = BenchmarkThresholdViolation {
            measurement: "latency".to_owned(),
            actual: 2.0,
            threshold: 1.0,
            comparison: "greater-than".to_owned(),
        };
        let report = BenchmarkSinkReport {
            threshold_violations: vec![violation; MAX_BENCHMARK_THRESHOLD_VIOLATIONS + 1],
            ..BenchmarkSinkReport::default()
        };

        assert!(matches!(
            report.validate(),
            Err(BenchmarkSinkError::ProtocolViolation(message))
                if message.contains("threshold-violation")
        ));
    }
}
