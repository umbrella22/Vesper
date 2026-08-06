use super::*;
use std::collections::BTreeMap;

use player_plugin::{MAX_PLUGIN_DIAGNOSTICS, MAX_PLUGIN_MEASUREMENTS, PluginReference};

pub struct BenchmarkSinkPluginSession {
    sinks: Vec<Arc<dyn BenchmarkSink>>,
    references: Vec<PluginReference>,
}

impl std::fmt::Debug for BenchmarkSinkPluginSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BenchmarkSinkPluginSession")
            .field("sink_count", &self.sinks.len())
            .field("references", &self.references)
            .finish()
    }
}

impl BenchmarkSinkPluginSession {
    pub fn from_registry(
        registry: &PluginRegistry,
        references: impl IntoIterator<Item = PluginReference>,
    ) -> Result<Self, PluginSelectionError> {
        let mut sinks = Vec::new();
        let mut resolved_references = Vec::new();
        for reference in references {
            let resolved = registry.resolve_benchmark_sink(&reference)?;
            resolved_references.push(resolved.reference().clone());
            sinks.push(resolved.capability());
        }
        Ok(Self {
            sinks,
            references: resolved_references,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.sinks.is_empty()
    }

    pub fn references(&self) -> &[PluginReference] {
        &self.references
    }

    pub fn on_event_batch_json(
        &self,
        batch_json: &str,
    ) -> Result<BenchmarkSinkReport, BenchmarkSinkError> {
        let batch = serde_json::from_str::<BenchmarkEventBatch>(batch_json).map_err(|error| {
            BenchmarkSinkError::PayloadCodec(format!(
                "decode benchmark event batch payload failed: {error}"
            ))
        })?;
        Ok(self.on_event_batch(&batch))
    }

    pub fn on_event_batch_report_json(
        &self,
        batch_json: &str,
    ) -> Result<String, BenchmarkSinkError> {
        serde_json::to_string(&self.on_event_batch_json(batch_json)?).map_err(|error| {
            BenchmarkSinkError::PayloadCodec(format!(
                "encode benchmark sink status failed: {error}"
            ))
        })
    }

    pub fn on_event_batch(&self, batch: &BenchmarkEventBatch) -> BenchmarkSinkReport {
        let mut report = BenchmarkSinkReport::default();
        if let Err(error) = batch.validate() {
            report.dropped_events = batch.events.len() as u64;
            report
                .diagnostics
                .push(sink_error_diagnostic("host", &error));
            return report;
        }
        for sink in &self.sinks {
            match sink.on_event_batch(batch) {
                Ok(status) => {
                    report.accepted_events += status.accepted_events;
                }
                Err(error) => {
                    report.dropped_events += batch.events.len() as u64;
                    push_diagnostic(&mut report, sink_error_diagnostic(sink.name(), &error));
                }
            }
        }
        report
    }

    pub fn flush(&self) -> BenchmarkSinkReport {
        let mut report = BenchmarkSinkReport::default();
        for sink in &self.sinks {
            match sink.flush() {
                Ok(sink_report) => {
                    merge_report(&mut report, sink_report);
                }
                Err(error) => {
                    push_diagnostic(&mut report, sink_error_diagnostic(sink.name(), &error));
                }
            }
        }
        report
    }

    pub fn flush_json(&self) -> Result<String, BenchmarkSinkError> {
        serde_json::to_string(&self.flush()).map_err(|error| {
            BenchmarkSinkError::PayloadCodec(format!(
                "encode benchmark sink report failed: {error}"
            ))
        })
    }
}

fn merge_report(report: &mut BenchmarkSinkReport, incoming: BenchmarkSinkReport) {
    report.accepted_events = report
        .accepted_events
        .saturating_add(incoming.accepted_events);
    report.dropped_events = report
        .dropped_events
        .saturating_add(incoming.dropped_events);
    extend_bounded(
        &mut report.measurements,
        incoming.measurements,
        MAX_PLUGIN_MEASUREMENTS,
    );
    extend_bounded(
        &mut report.threshold_violations,
        incoming.threshold_violations,
        MAX_PLUGIN_MEASUREMENTS,
    );
    for diagnostic in incoming.diagnostics {
        push_diagnostic(report, diagnostic);
    }
}

fn extend_bounded<T>(target: &mut Vec<T>, incoming: Vec<T>, limit: usize) {
    let remaining = limit.saturating_sub(target.len());
    target.extend(incoming.into_iter().take(remaining));
}

fn push_diagnostic(report: &mut BenchmarkSinkReport, diagnostic: PluginDiagnostic) {
    if report.diagnostics.len() < MAX_PLUGIN_DIAGNOSTICS {
        report.diagnostics.push(diagnostic);
    }
}

fn sink_error_diagnostic(name: &str, error: &BenchmarkSinkError) -> PluginDiagnostic {
    let code = match error {
        BenchmarkSinkError::PayloadCodec(_) => "benchmark.payload_codec",
        BenchmarkSinkError::AbiViolation(_) => "benchmark.abi_violation",
        BenchmarkSinkError::SinkFailed(_) => "benchmark.sink_failed",
        BenchmarkSinkError::ProtocolViolation(_) => "benchmark.protocol_violation",
    };
    PluginDiagnostic {
        code: code.to_owned(),
        severity: PluginDiagnosticSeverity::Error,
        message: "benchmark sink operation failed".to_owned(),
        attributes: BTreeMap::from([("sink".to_owned(), name.to_owned())]),
    }
}
