#![deny(unsafe_code)]

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use player_plugin::{
    BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, BenchmarkSinkReport,
    BenchmarkSinkStatus, CompletedDownloadInfo, ContentFormatKind, PipelineEvent,
    PipelineEventHook, PipelineEventHookError, PipelineEventHookOutcome, Plugin, PluginBuildError,
    PostDownloadProcessor, ProcessorError, ProcessorOutput, ProcessorProgress,
};

struct FixturePostDownloadProcessor;

impl PostDownloadProcessor for FixturePostDownloadProcessor {
    fn name(&self) -> &str {
        "fixture-post-download"
    }

    fn supported_input_formats(&self) -> &[ContentFormatKind] {
        &[ContentFormatKind::SingleFile]
    }

    fn process(
        &self,
        _input: &CompletedDownloadInfo,
        _output_path: &Path,
        _progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        Ok(ProcessorOutput::Skipped)
    }
}

struct FixtureEventHook;

impl PipelineEventHook for FixtureEventHook {
    fn on_event(
        &self,
        _event: &PipelineEvent,
    ) -> Result<PipelineEventHookOutcome, PipelineEventHookError> {
        Ok(PipelineEventHookOutcome::accepted())
    }
}

struct FixtureBenchmarkSink;

impl BenchmarkSink for FixtureBenchmarkSink {
    fn name(&self) -> &str {
        "fixture-benchmark"
    }

    fn on_event_batch(
        &self,
        batch: &BenchmarkEventBatch,
    ) -> Result<BenchmarkSinkStatus, BenchmarkSinkError> {
        Ok(BenchmarkSinkStatus {
            accepted_events: batch.events.len() as u64,
        })
    }

    fn flush(&self) -> Result<BenchmarkSinkReport, BenchmarkSinkError> {
        Ok(BenchmarkSinkReport::default())
    }
}

#[player_plugin::export]
fn fixture_plugin() -> Result<Plugin, PluginBuildError> {
    run_worker_fixture_behavior();
    Plugin::builder("dev.vesper.plugin-fixture", "Vesper Plugin Fixture")?
        .with_post_download_processor(
            "dev.vesper.plugin-fixture.post-download",
            FixturePostDownloadProcessor,
        )?
        .with_pipeline_event_hook("dev.vesper.plugin-fixture.event-hook", FixtureEventHook)?
        .with_benchmark_sink("dev.vesper.plugin-fixture.benchmark", FixtureBenchmarkSink)?
        .build()
}

fn run_worker_fixture_behavior() {
    let Ok(behavior) = std::env::var("VESPER_PLUGIN_FIXTURE_WORKER_BEHAVIOR") else {
        return;
    };
    match behavior.as_str() {
        "abort" => std::process::abort(),
        "hang" => {
            write_worker_fixture_ready_pid();
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        "flood" => {
            let bytes = vec![b'x'; 300 * 1024];
            let _ = std::io::stdout().lock().write_all(&bytes);
            let _ = std::io::stderr().lock().write_all(&bytes);
        }
        "spawn-descendant" => spawn_worker_fixture_descendant(),
        "reserve-response" => reserve_worker_response_path(),
        _ => {}
    }
}

#[cfg(unix)]
fn spawn_worker_fixture_descendant() {
    let Ok(child) = std::process::Command::new("/bin/sleep").arg("60").spawn() else {
        return;
    };
    if let Ok(path) = std::env::var("VESPER_PLUGIN_FIXTURE_DESCENDANT_PID_PATH") {
        let _ = std::fs::write(path, child.id().to_string());
    }
}

#[cfg(not(unix))]
fn spawn_worker_fixture_descendant() {}

fn reserve_worker_response_path() {
    let mut arguments = std::env::args_os();
    while let Some(argument) = arguments.next() {
        if argument == "--response"
            && let Some(path) = arguments.next()
        {
            let _ = std::fs::write(path, b"reserved by fixture");
            return;
        }
    }
}

fn write_worker_fixture_ready_pid() {
    if let Ok(path) = std::env::var("VESPER_PLUGIN_FIXTURE_READY_PID_PATH") {
        let _ = std::fs::write(path, std::process::id().to_string());
    }
}
