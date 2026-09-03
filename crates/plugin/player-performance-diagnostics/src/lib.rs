#![deny(unsafe_code)]

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use player_plugin::{
    BenchmarkEvent, BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, BenchmarkSinkReport,
    BenchmarkSinkStatus, Plugin, PluginBuildError, PluginDiagnostic, PluginDiagnosticSeverity,
    PluginMeasurement,
};

const PLUGIN_ID: &str = "io.github.umbrella22.vesper.performance-diagnostics";
const INSTANCE_ID: &str = "io.github.umbrella22.vesper.performance-diagnostics.benchmark";
const PLUGIN_NAME: &str = "Vesper Performance Diagnostics";
const MAX_SAMPLES_PER_COHORT: usize = 2_048;
const NANOS_PER_MILLISECOND: u64 = 1_000_000;
const STALL_THRESHOLD_NS: u64 = 500 * NANOS_PER_MILLISECOND;

const FRAME_SAMPLE: &str = "performance_frame_sample";
const OVERLAY_TRANSITION: &str = "performance_overlay_transition";
const SESSION_CONTEXT: &str = "performance_session_context";
const MARKER: &str = "performance_marker";
const BUFFERING_START: &str = "performance_playback_buffering_start";
const BUFFERING_END: &str = "performance_playback_buffering_end";
const DROPPED_VIDEO_FRAMES: &str = "dropped_video_frames";
const PLAYBACK_STALLED: &str = "playback_stalled";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CohortKind {
    OverlayInactive,
    OverlayActive,
    Transition,
    Excluded,
}

impl CohortKind {
    const ALL: [Self; 4] = [
        Self::OverlayInactive,
        Self::OverlayActive,
        Self::Transition,
        Self::Excluded,
    ];

    fn wire_name(self) -> &'static str {
        match self {
            Self::OverlayInactive => "overlayInactive",
            Self::OverlayActive => "overlayActive",
            Self::Transition => "transition",
            Self::Excluded => "excluded",
        }
    }
}

#[derive(Debug, Default)]
struct FrameCohort {
    total_samples: u64,
    retained_samples: VecDeque<u64>,
    jank_count: u64,
    severe_jank_count: u64,
}

impl FrameCohort {
    fn push(&mut self, load_ns: u64, budget_ns: u64) {
        self.total_samples = self.total_samples.saturating_add(1);
        if load_ns > budget_ns {
            self.jank_count = self.jank_count.saturating_add(1);
        }
        if load_ns > budget_ns.saturating_mul(2) {
            self.severe_jank_count = self.severe_jank_count.saturating_add(1);
        }
        if self.retained_samples.len() == MAX_SAMPLES_PER_COHORT {
            self.retained_samples.pop_front();
        }
        self.retained_samples.push_back(load_ns);
    }

    fn percentile(&self, percentile: f64) -> u64 {
        if self.retained_samples.is_empty() {
            return 0;
        }
        let mut sorted = self.retained_samples.iter().copied().collect::<Vec<_>>();
        sorted.sort_unstable();
        let index = ((sorted.len() - 1) as f64 * percentile).ceil() as usize;
        sorted[index.min(sorted.len() - 1)]
    }

    fn minimum(&self) -> u64 {
        self.retained_samples.iter().copied().min().unwrap_or(0)
    }

    fn maximum(&self) -> u64 {
        self.retained_samples.iter().copied().max().unwrap_or(0)
    }

    fn jank_ratio(&self) -> f64 {
        if self.total_samples == 0 {
            0.0
        } else {
            self.jank_count as f64 / self.total_samples as f64
        }
    }

    fn severe_jank_ratio(&self) -> f64 {
        if self.total_samples == 0 {
            0.0
        } else {
            self.severe_jank_count as f64 / self.total_samples as f64
        }
    }
}

#[derive(Debug, Default)]
struct DiagnosticsState {
    accepted_events: u64,
    dropped_events: u64,
    frame_budget_ns: u64,
    probe: Option<String>,
    inactive: FrameCohort,
    active: FrameCohort,
    transition: FrameCohort,
    excluded: FrameCohort,
    overlay_transitions: u64,
    active_playback_ns: u64,
    dropped_video_frames: u64,
    buffering_count: u64,
    buffering_duration_ns: u64,
    stall_count: u64,
    buffering_started_ns: Option<u64>,
    buffering_started_steady: bool,
    steady_buffering_duration_ns: u64,
    steady_stall_duration_ns: u64,
    last_elapsed_ns: u64,
    marker_count: u64,
}

impl DiagnosticsState {
    fn cohort_mut(&mut self, kind: CohortKind) -> &mut FrameCohort {
        match kind {
            CohortKind::OverlayInactive => &mut self.inactive,
            CohortKind::OverlayActive => &mut self.active,
            CohortKind::Transition => &mut self.transition,
            CohortKind::Excluded => &mut self.excluded,
        }
    }

    fn cohort(&self, kind: CohortKind) -> &FrameCohort {
        match kind {
            CohortKind::OverlayInactive => &self.inactive,
            CohortKind::OverlayActive => &self.active,
            CohortKind::Transition => &self.transition,
            CohortKind::Excluded => &self.excluded,
        }
    }

    fn record(&mut self, event: &BenchmarkEvent) -> bool {
        self.last_elapsed_ns = self.last_elapsed_ns.max(event.elapsed_ns);
        match event.event_name.as_str() {
            FRAME_SAMPLE => self.record_frame(event),
            OVERLAY_TRANSITION => {
                if self.buffering_started_ns.is_some()
                    && event.attributes.get("sampleClass").map(String::as_str) != Some("steady")
                {
                    self.buffering_started_steady = false;
                }
                self.overlay_transitions = self.overlay_transitions.saturating_add(1);
                true
            }
            SESSION_CONTEXT => self.record_context(event),
            MARKER => {
                let Some(name) = event.attributes.get("name") else {
                    return false;
                };
                if !is_valid_marker_name(name) || self.marker_count >= 64 {
                    false
                } else {
                    self.marker_count += 1;
                    true
                }
            }
            BUFFERING_START => {
                let Some(steady) = parse_steady_sample_class(event) else {
                    return false;
                };
                if self.buffering_started_ns.is_none() {
                    self.buffering_started_ns = Some(event.elapsed_ns);
                    self.buffering_started_steady = steady;
                    self.buffering_count = self.buffering_count.saturating_add(1);
                }
                true
            }
            BUFFERING_END => {
                let Some(ended_steady) = parse_steady_sample_class(event) else {
                    return false;
                };
                if let Some(started_ns) = self.buffering_started_ns.take() {
                    let duration_ns = event.elapsed_ns.saturating_sub(started_ns);
                    self.buffering_duration_ns =
                        self.buffering_duration_ns.saturating_add(duration_ns);
                    if self.buffering_started_steady && ended_steady {
                        self.steady_buffering_duration_ns = self
                            .steady_buffering_duration_ns
                            .saturating_add(duration_ns);
                    }
                    self.buffering_started_steady = false;
                }
                true
            }
            DROPPED_VIDEO_FRAMES => {
                let Some(count) = parse_u64_attribute(event, "count") else {
                    return false;
                };
                self.dropped_video_frames = self.dropped_video_frames.saturating_add(count);
                true
            }
            PLAYBACK_STALLED => {
                let count = parse_u64_attribute(event, "count").unwrap_or(1);
                let Some(steady) = parse_steady_sample_class(event) else {
                    return false;
                };
                let duration_ns = match event.attributes.get("durationNs") {
                    Some(_) => {
                        let Some(duration_ns) = parse_u64_attribute(event, "durationNs") else {
                            return false;
                        };
                        duration_ns
                    }
                    None => 0,
                };
                self.stall_count = self.stall_count.saturating_add(count);
                if steady {
                    self.steady_stall_duration_ns =
                        self.steady_stall_duration_ns.saturating_add(duration_ns);
                }
                true
            }
            // Lifecycle and first-frame events are accepted as bounded context,
            // but they do not influence the v1 diagnosis directly.
            "initialize_start"
            | "initialize_completed"
            | "source_load_start"
            | "source_load_configured"
            | "first_frame_rendered"
            | "playback_ended"
            | "playback_error" => true,
            _ => false,
        }
    }

    fn record_context(&mut self, event: &BenchmarkEvent) -> bool {
        if let Some(probe) = event.attributes.get("probe") {
            match self.probe.as_deref() {
                None => self.probe = Some(probe.clone()),
                Some(existing) if existing != probe => self.probe = Some("mixed".to_owned()),
                _ => {}
            }
        }
        if let Some(duration_ns) = parse_u64_attribute(event, "activePlaybackNs") {
            self.active_playback_ns = self.active_playback_ns.max(duration_ns);
        }
        true
    }

    fn record_frame(&mut self, event: &BenchmarkEvent) -> bool {
        let Some(load_ns) = parse_u64_attribute(event, "frameLoadNs") else {
            return false;
        };
        let Some(budget_ns) = parse_u64_attribute(event, "frameBudgetNs") else {
            return false;
        };
        if budget_ns == 0 {
            return false;
        }
        let Some(overlay_active) = parse_bool_attribute(event, "overlayActive") else {
            return false;
        };
        let Some(sample_class) = event.attributes.get("sampleClass") else {
            return false;
        };
        let Some(probe) = event.attributes.get("probe") else {
            return false;
        };
        let cohort = match sample_class.as_str() {
            "steady" if overlay_active => CohortKind::OverlayActive,
            "steady" => CohortKind::OverlayInactive,
            "transition" => CohortKind::Transition,
            "excluded" => CohortKind::Excluded,
            _ => return false,
        };
        self.frame_budget_ns = budget_ns;
        match self.probe.as_deref() {
            None => self.probe = Some(probe.clone()),
            Some(existing) if existing != probe => self.probe = Some("mixed".to_owned()),
            _ => {}
        }
        self.cohort_mut(cohort).push(load_ns, budget_ns);
        true
    }

    fn report(&self) -> BenchmarkSinkReport {
        let (buffering_duration_ns, _) = self.buffering_durations();
        let mut measurements = Vec::new();
        for cohort_kind in CohortKind::ALL {
            append_cohort_measurements(&mut measurements, cohort_kind, self.cohort(cohort_kind));
        }
        append_measurement(
            &mut measurements,
            "frame_budget",
            self.frame_budget_ns as f64,
            "ns",
        );
        append_measurement(
            &mut measurements,
            "overlay_transitions",
            self.overlay_transitions as f64,
            "count",
        );
        append_measurement(
            &mut measurements,
            "active_playback_duration",
            self.active_playback_ns as f64,
            "ns",
        );
        append_measurement(
            &mut measurements,
            "dropped_video_frames",
            self.dropped_video_frames as f64,
            "count",
        );
        append_measurement(
            &mut measurements,
            "buffering_count",
            self.buffering_count as f64,
            "count",
        );
        append_measurement(
            &mut measurements,
            "buffering_duration",
            buffering_duration_ns as f64,
            "ns",
        );
        append_measurement(
            &mut measurements,
            "stall_count",
            self.stall_count as f64,
            "count",
        );

        let diagnosis = self.diagnosis();
        let mut attributes = BTreeMap::new();
        attributes.insert("kind".to_owned(), diagnosis.kind.to_owned());
        attributes.insert("confidence".to_owned(), diagnosis.confidence.to_owned());
        attributes.insert(
            "probe".to_owned(),
            self.probe.clone().unwrap_or_else(|| "unknown".to_owned()),
        );
        attributes.insert(
            "evidenceCodes".to_owned(),
            diagnosis.evidence_codes.join(","),
        );

        BenchmarkSinkReport {
            accepted_events: self.accepted_events,
            dropped_events: self.dropped_events,
            measurements,
            threshold_violations: Vec::new(),
            diagnostics: vec![PluginDiagnostic {
                code: "performance.diagnosis".to_owned(),
                severity: if diagnosis.kind == "insufficientEvidence" {
                    PluginDiagnosticSeverity::Warning
                } else {
                    PluginDiagnosticSeverity::Info
                },
                message: "Performance diagnosis reports correlation, not causation.".to_owned(),
                attributes,
            }],
        }
    }

    fn diagnosis(&self) -> Diagnosis<'static> {
        let inactive = &self.inactive;
        let active = &self.active;
        if inactive.total_samples < 120 || active.total_samples < 120 {
            return Diagnosis {
                kind: "insufficientEvidence",
                confidence: "low",
                evidence_codes: vec!["steady_cohorts_below_120"],
            };
        }

        let budget = self.frame_budget_ns;
        let inactive_jank = inactive.jank_ratio();
        let active_jank = active.jank_ratio();
        let jank_delta = active_jank - inactive_jank;
        let relative_increase = if inactive_jank == 0.0 {
            if active_jank > 0.0 {
                f64::INFINITY
            } else {
                1.0
            }
        } else {
            active_jank / inactive_jank
        };
        let p95_delta = active
            .percentile(0.95)
            .saturating_sub(inactive.percentile(0.95));
        let overlay_correlated =
            (jank_delta >= 0.05 && relative_increase >= 1.5) || p95_delta >= budget / 2;
        let ui_pressure = active_jank >= 0.05
            || inactive_jank >= 0.05
            || active.percentile(0.95) > budget
            || inactive.percentile(0.95) > budget;

        let active_minutes = self.active_playback_ns as f64 / 60_000_000_000.0;
        let dropped_threshold = (active_minutes * 5.0).ceil().max(3.0) as u64;
        let (_, steady_buffering_duration_ns) = self.buffering_durations();
        let playback_pressure = steady_buffering_duration_ns >= STALL_THRESHOLD_NS
            || self.steady_stall_duration_ns >= STALL_THRESHOLD_NS
            || self.dropped_video_frames >= dropped_threshold;

        let overlay_correlated_ui_pressure = overlay_correlated && ui_pressure;
        let kind = match (
            playback_pressure,
            overlay_correlated_ui_pressure,
            ui_pressure,
        ) {
            (true, true, _) => "mixedPressure",
            (true, false, _) => "playbackPressure",
            (false, true, _) => "overlayCorrelatedUiPressure",
            (false, false, true) => "hostUiPressureUncorrelated",
            (false, false, false) => "noSignificantPressure",
        };
        let minimum_samples = inactive.total_samples.min(active.total_samples);
        let confidence = if minimum_samples >= 600 && self.overlay_transitions >= 2 {
            "high"
        } else if minimum_samples >= 300 {
            "medium"
        } else {
            "low"
        };
        let mut evidence_codes = Vec::new();
        if overlay_correlated_ui_pressure {
            evidence_codes.push("overlay_steady_cohort_delta");
        }
        if playback_pressure {
            evidence_codes.push("native_playback_pressure");
        }
        if ui_pressure && !overlay_correlated {
            evidence_codes.push("ui_pressure_not_overlay_correlated");
        }
        if evidence_codes.is_empty() {
            evidence_codes.push("thresholds_not_exceeded");
        }
        Diagnosis {
            kind,
            confidence,
            evidence_codes,
        }
    }

    fn buffering_durations(&self) -> (u64, u64) {
        let open_duration_ns = self
            .buffering_started_ns
            .map(|started_ns| self.last_elapsed_ns.saturating_sub(started_ns))
            .unwrap_or(0);
        let total = self.buffering_duration_ns.saturating_add(open_duration_ns);
        let steady =
            self.steady_buffering_duration_ns
                .saturating_add(if self.buffering_started_steady {
                    open_duration_ns
                } else {
                    0
                });
        (total, steady)
    }
}

struct Diagnosis<'a> {
    kind: &'a str,
    confidence: &'a str,
    evidence_codes: Vec<&'a str>,
}

#[derive(Debug, Default)]
pub struct PerformanceDiagnosticsSink {
    state: Mutex<DiagnosticsState>,
}

impl BenchmarkSink for PerformanceDiagnosticsSink {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn on_event_batch(
        &self,
        batch: &BenchmarkEventBatch,
    ) -> Result<BenchmarkSinkStatus, BenchmarkSinkError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut accepted = 0_u64;
        for event in &batch.events {
            if state.record(event) {
                state.accepted_events = state.accepted_events.saturating_add(1);
                accepted = accepted.saturating_add(1);
            } else {
                state.dropped_events = state.dropped_events.saturating_add(1);
            }
        }
        Ok(BenchmarkSinkStatus {
            accepted_events: accepted,
        })
    }

    fn flush(&self) -> Result<BenchmarkSinkReport, BenchmarkSinkError> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(state.report())
    }
}

fn parse_u64_attribute(event: &BenchmarkEvent, key: &str) -> Option<u64> {
    event.attributes.get(key)?.parse().ok()
}

fn parse_bool_attribute(event: &BenchmarkEvent, key: &str) -> Option<bool> {
    match event.attributes.get(key)?.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_steady_sample_class(event: &BenchmarkEvent) -> Option<bool> {
    match event.attributes.get("sampleClass")?.as_str() {
        "steady" => Some(true),
        "transition" | "excluded" => Some(false),
        _ => None,
    }
}

fn is_valid_marker_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };
    bytes.len() <= 64
        && (first.is_ascii_alphabetic() || *first == b'_')
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'.' | b'-'))
}

fn append_cohort_measurements(
    output: &mut Vec<PluginMeasurement>,
    kind: CohortKind,
    cohort: &FrameCohort,
) {
    let attributes = BTreeMap::from([("cohort".to_owned(), kind.wire_name().to_owned())]);
    for (name, value, unit) in [
        ("frame_sample_count", cohort.total_samples as f64, "count"),
        ("frame_jank_count", cohort.jank_count as f64, "count"),
        (
            "frame_severe_jank_count",
            cohort.severe_jank_count as f64,
            "count",
        ),
        ("frame_jank_ratio", cohort.jank_ratio(), "ratio"),
        (
            "frame_severe_jank_ratio",
            cohort.severe_jank_ratio(),
            "ratio",
        ),
        ("frame_load_min", cohort.minimum() as f64, "ns"),
        ("frame_load_p50", cohort.percentile(0.50) as f64, "ns"),
        ("frame_load_p95", cohort.percentile(0.95) as f64, "ns"),
        ("frame_load_max", cohort.maximum() as f64, "ns"),
    ] {
        output.push(PluginMeasurement {
            name: name.to_owned(),
            value,
            unit: unit.to_owned(),
            attributes: attributes.clone(),
        });
    }
}

fn append_measurement(output: &mut Vec<PluginMeasurement>, name: &str, value: f64, unit: &str) {
    output.push(PluginMeasurement {
        name: name.to_owned(),
        value,
        unit: unit.to_owned(),
        attributes: BTreeMap::new(),
    });
}

#[player_plugin::export]
fn performance_diagnostics_plugin() -> Result<Plugin, PluginBuildError> {
    Plugin::builder(PLUGIN_ID, PLUGIN_NAME)?
        .with_benchmark_sink(INSTANCE_ID, PerformanceDiagnosticsSink::default())?
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str, elapsed_ns: u64, attributes: &[(&str, &str)]) -> BenchmarkEvent {
        BenchmarkEvent {
            run_id: "run".to_owned(),
            session_id: "session".to_owned(),
            platform: "test".to_owned(),
            source_protocol: None,
            event_name: name.to_owned(),
            timestamp_ns: elapsed_ns,
            elapsed_ns,
            thread: None,
            attributes: attributes
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        }
    }

    fn frame(active: bool, load_ns: u64) -> BenchmarkEvent {
        let active = if active { "true" } else { "false" };
        event(
            FRAME_SAMPLE,
            load_ns,
            &[
                ("frameLoadNs", &load_ns.to_string()),
                ("frameBudgetNs", "16666667"),
                ("overlayActive", active),
                ("sampleClass", "steady"),
                ("probe", "flutterFrameTiming"),
            ],
        )
    }

    fn feed(sink: &PerformanceDiagnosticsSink, events: Vec<BenchmarkEvent>) {
        let status = sink
            .on_event_batch(&BenchmarkEventBatch { events })
            .expect("batch accepted");
        assert!(status.accepted_events > 0);
    }

    fn diagnosis(report: &BenchmarkSinkReport) -> (&str, &str) {
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "performance.diagnosis")
            .expect("diagnosis");
        (
            diagnostic.attributes["kind"].as_str(),
            diagnostic.attributes["confidence"].as_str(),
        )
    }

    #[test]
    fn reports_insufficient_evidence_until_both_cohorts_are_populated() {
        let sink = PerformanceDiagnosticsSink::default();
        feed(&sink, vec![frame(false, 10_000_000); 120]);
        assert_eq!(
            diagnosis(&sink.flush().expect("report")).0,
            "insufficientEvidence"
        );
    }

    #[test]
    fn diagnoses_overlay_correlated_ui_pressure() {
        let sink = PerformanceDiagnosticsSink::default();
        feed(&sink, vec![frame(false, 8_000_000); 300]);
        feed(&sink, vec![frame(true, 22_000_000); 300]);
        feed(
            &sink,
            vec![event(OVERLAY_TRANSITION, 1, &[("sampleClass", "transition")],); 2],
        );
        let report = sink.flush().expect("report");
        assert_eq!(
            diagnosis(&report),
            ("overlayCorrelatedUiPressure", "medium")
        );
        assert!(report.measurements.len() < 128);
        report.validate().expect("valid benchmark report");
    }

    #[test]
    fn does_not_report_pressure_for_a_sub_budget_p95_delta() {
        let sink = PerformanceDiagnosticsSink::default();
        feed(&sink, vec![frame(false, 1_000_000); 120]);
        feed(&sink, vec![frame(true, 10_000_000); 120]);

        let report = sink.flush().expect("report");

        assert_eq!(diagnosis(&report).0, "noSignificantPressure");
        assert_eq!(
            report.diagnostics[0].attributes["evidenceCodes"],
            "thresholds_not_exceeded"
        );
    }

    #[test]
    fn combines_native_playback_and_overlay_pressure() {
        let sink = PerformanceDiagnosticsSink::default();
        feed(&sink, vec![frame(false, 8_000_000); 120]);
        feed(&sink, vec![frame(true, 22_000_000); 120]);
        feed(
            &sink,
            vec![event(DROPPED_VIDEO_FRAMES, 2, &[("count", "3")])],
        );
        assert_eq!(diagnosis(&sink.flush().expect("report")).0, "mixedPressure");
    }

    #[test]
    fn playback_pressure_uses_only_steady_intervals_over_the_threshold() {
        let sink = PerformanceDiagnosticsSink::default();
        feed(&sink, vec![frame(false, 8_000_000); 120]);
        feed(&sink, vec![frame(true, 8_000_000); 120]);
        feed(
            &sink,
            vec![
                event(BUFFERING_START, 1, &[("sampleClass", "excluded")]),
                event(
                    BUFFERING_END,
                    STALL_THRESHOLD_NS + 1,
                    &[("sampleClass", "excluded")],
                ),
                event(
                    PLAYBACK_STALLED,
                    STALL_THRESHOLD_NS + 2,
                    &[
                        ("count", "1"),
                        ("durationNs", "499999999"),
                        ("sampleClass", "steady"),
                    ],
                ),
            ],
        );
        assert_eq!(
            diagnosis(&sink.flush().expect("report")).0,
            "noSignificantPressure"
        );

        feed(
            &sink,
            vec![
                event(
                    BUFFERING_START,
                    STALL_THRESHOLD_NS + 3,
                    &[("sampleClass", "steady")],
                ),
                event(
                    BUFFERING_END,
                    STALL_THRESHOLD_NS * 2 + 3,
                    &[("sampleClass", "steady")],
                ),
            ],
        );
        assert_eq!(
            diagnosis(&sink.flush().expect("report")).0,
            "playbackPressure"
        );
    }

    #[test]
    fn transition_during_buffering_excludes_the_interval_from_diagnosis() {
        let sink = PerformanceDiagnosticsSink::default();
        feed(&sink, vec![frame(false, 8_000_000); 120]);
        feed(&sink, vec![frame(true, 8_000_000); 120]);
        feed(
            &sink,
            vec![
                event(BUFFERING_START, 1, &[("sampleClass", "steady")]),
                event(OVERLAY_TRANSITION, 2, &[("sampleClass", "transition")]),
                event(
                    BUFFERING_END,
                    STALL_THRESHOLD_NS + 2,
                    &[("sampleClass", "steady")],
                ),
            ],
        );

        assert_eq!(
            diagnosis(&sink.flush().expect("report")).0,
            "noSignificantPressure"
        );
    }

    #[test]
    fn preserves_an_unknown_probe_raw_value() {
        let sink = PerformanceDiagnosticsSink::default();
        let frame = event(
            FRAME_SAMPLE,
            1,
            &[
                ("frameLoadNs", "8000000"),
                ("frameBudgetNs", "16666667"),
                ("overlayActive", "false"),
                ("sampleClass", "steady"),
                ("probe", "futureHostProbe"),
            ],
        );

        feed(&sink, vec![frame]);

        let report = sink.flush().expect("report");
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "performance.diagnosis")
            .expect("diagnosis");
        assert_eq!(diagnostic.attributes["probe"], "futureHostProbe");
    }

    #[test]
    fn rejects_markers_outside_the_owned_identifier_contract() {
        let sink = PerformanceDiagnosticsSink::default();
        let status = sink
            .on_event_batch(&BenchmarkEventBatch {
                events: vec![
                    event(MARKER, 1, &[]),
                    event(MARKER, 2, &[("name", "contains whitespace")]),
                    event(MARKER, 3, &[("name", "valid_marker-1.0")]),
                ],
            })
            .expect("bounded marker validation");

        assert_eq!(status.accepted_events, 1);
        let report = sink.flush().expect("report");
        assert_eq!(report.accepted_events, 1);
        assert_eq!(report.dropped_events, 2);
    }

    #[test]
    fn rejects_unknown_or_malformed_events_without_leaking_values() {
        let sink = PerformanceDiagnosticsSink::default();
        let status = sink
            .on_event_batch(&BenchmarkEventBatch {
                events: vec![
                    event(
                        "contains_sensitive_payload",
                        1,
                        &[("url", "https://example.test")],
                    ),
                    event(FRAME_SAMPLE, 2, &[("frameLoadNs", "not-a-number")]),
                ],
            })
            .expect("bounded rejection");
        assert_eq!(status.accepted_events, 0);
        let report = sink.flush().expect("report");
        assert_eq!(report.dropped_events, 2);
        assert!(!format!("{report:?}").contains("example.test"));
    }

    #[test]
    fn retains_only_a_bounded_frame_window() {
        let sink = PerformanceDiagnosticsSink::default();
        feed(&sink, vec![frame(false, 8_000_000); 512]);
        for _ in 0..4 {
            feed(&sink, vec![frame(false, 8_000_000); 512]);
        }
        feed(&sink, vec![frame(false, 9_000_000)]);
        let state = sink
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(state.inactive.total_samples, 2_561);
        assert_eq!(
            state.inactive.retained_samples.len(),
            MAX_SAMPLES_PER_COHORT
        );
    }
}
