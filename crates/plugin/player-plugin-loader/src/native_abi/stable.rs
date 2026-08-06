use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use player_plugin::{
    BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, BenchmarkSinkReport,
    BenchmarkSinkStatus, CompletedDownloadInfo, MAX_PIPELINE_EVENT_INPUT_BYTES, PipelineEvent,
    PipelineEventHook, PipelineEventHookError, PipelineEventHookOutcome, PostDownloadProcessor,
    ProcessorCapabilities, ProcessorError, ProcessorOutput, ProcessorProgress,
};
use player_plugin_abi::{
    VesperBenchmarkSink, VesperByteSlice, VesperJsonOut, VesperPipelineEventHook,
    VesperPostDownloadProcessor, VesperProgressCallbacks, VesperStatus, status,
};

use super::PluginOwner;
use super::runtime::{InterfaceRuntime, JsonCallResult, NativeAbiBoundaryError, borrowed_bytes};

type PostDownloadCall = unsafe extern "C" fn(
    context: *mut c_void,
    input_json: VesperByteSlice,
    output_path: VesperByteSlice,
    progress: *const VesperProgressCallbacks,
    out: *mut VesperJsonOut,
) -> VesperStatus;

#[derive(Debug)]
struct NativeAbiPostDownloadProcessorInner {
    runtime: Arc<InterfaceRuntime>,
    name: String,
    capabilities: ProcessorCapabilities,
    process: PostDownloadCall,
    assemble: PostDownloadCall,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeAbiPostDownloadProcessor {
    inner: Arc<NativeAbiPostDownloadProcessorInner>,
}

impl NativeAbiPostDownloadProcessor {
    pub(super) fn new(
        plugin_id: &str,
        plugin_name: String,
        instance_id: &str,
        owner: Arc<PluginOwner>,
        table: VesperPostDownloadProcessor,
    ) -> Result<Self, NativeAbiBoundaryError> {
        let runtime = Arc::new(InterfaceRuntime::new(
            owner,
            table.header.context,
            plugin_id,
            instance_id,
        )?);
        let capabilities = table.capabilities_json.ok_or_else(|| {
            runtime.contract_violation(
                "construct_wrapper",
                "capabilities_json callback is missing after validation",
            )
        })?;
        let process = table.process_json.ok_or_else(|| {
            runtime.contract_violation(
                "construct_wrapper",
                "process_json callback is missing after validation",
            )
        })?;
        let assemble = table.assemble_json.ok_or_else(|| {
            runtime.contract_violation(
                "construct_wrapper",
                "assemble_json callback is missing after validation",
            )
        })?;
        let capability_result =
            runtime.invoke_json("capabilities_json", &[status::FAILURE], |out| {
                // SAFETY: table validation fixed the callback signature and
                // context; `out` is host-initialized for this synchronous call.
                unsafe { capabilities(runtime.context(), out) }
            })?;
        let capabilities = match capability_result {
            JsonCallResult::Success(payload) => {
                runtime.decode_json::<ProcessorCapabilities>("capabilities_json", &payload)?
            }
            JsonCallResult::Failure {
                status: raw_status,
                payload,
            } => {
                let error = runtime.decode_json::<ProcessorError>("capabilities_json", &payload)?;
                return Err(runtime.reported_failure(
                    "capabilities_json",
                    raw_status,
                    error.to_string(),
                ));
            }
        };
        Ok(Self {
            inner: Arc::new(NativeAbiPostDownloadProcessorInner {
                runtime,
                name: plugin_name,
                capabilities,
                process,
                assemble,
            }),
        })
    }

    fn call(
        &self,
        operation: &'static str,
        callback: PostDownloadCall,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        let input_json = serde_json::to_vec(input).map_err(|error| {
            ProcessorError::PayloadCodec(format!("serialize post-download input failed: {error}"))
        })?;
        let output_path = output_path.to_str().ok_or_else(|| {
            ProcessorError::OutputPath("output path is not valid UTF-8".to_owned())
        })?;
        let progress_adapter = ProgressAdapter {
            progress,
            violated: AtomicBool::new(false),
        };
        let progress_callbacks = VesperProgressCallbacks {
            struct_size: std::mem::size_of::<VesperProgressCallbacks>() as u32,
            reserved: 0,
            context: std::ptr::from_ref(&progress_adapter).cast_mut().cast(),
            on_progress: Some(progress_on_progress),
            is_cancelled: Some(progress_is_cancelled),
        };
        let result = self.inner.runtime.invoke_json(
            operation,
            &[status::FAILURE, status::CANCELLED],
            |out| {
                // SAFETY: callback, context, borrowed JSON/path bytes, progress
                // adapter, and output all remain live for this synchronous call.
                unsafe {
                    callback(
                        self.inner.runtime.context(),
                        borrowed_bytes(&input_json),
                        borrowed_bytes(output_path.as_bytes()),
                        &progress_callbacks,
                        out,
                    )
                }
            },
        );
        if progress_adapter.violated.load(Ordering::Acquire) {
            return Err(map_processor_boundary(self.inner.runtime.contract_violation(
                operation,
                "plugin used an invalid progress callback value or the host progress callback panicked",
            )));
        }
        match result.map_err(map_processor_boundary)? {
            JsonCallResult::Success(payload) => self
                .inner
                .runtime
                .decode_json::<ProcessorOutput>(operation, &payload)
                .map_err(map_processor_boundary),
            JsonCallResult::Failure {
                status: raw_status,
                payload,
            } => {
                let error = self
                    .inner
                    .runtime
                    .decode_json::<ProcessorError>(operation, &payload)
                    .map_err(map_processor_boundary)?;
                let status_matches = (raw_status == status::CANCELLED
                    && error == ProcessorError::Cancelled)
                    || (raw_status == status::FAILURE
                        && error != ProcessorError::Cancelled
                        && !matches!(&error, ProcessorError::AbiViolation(_)));
                if !status_matches {
                    return Err(map_processor_boundary(
                        self.inner.runtime.contract_violation(
                            operation,
                            format!(
                                "status {raw_status} is inconsistent with processor error `{error}`"
                            ),
                        ),
                    ));
                }
                Err(error)
            }
        }
    }
}

impl PostDownloadProcessor for NativeAbiPostDownloadProcessor {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn supported_input_formats(&self) -> &[player_plugin::ContentFormatKind] {
        &self.inner.capabilities.supported_input_formats
    }

    fn capabilities(&self) -> ProcessorCapabilities {
        self.inner.capabilities.clone()
    }

    fn process(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        self.call(
            "process_json",
            self.inner.process,
            input,
            output_path,
            progress,
        )
    }

    fn assemble(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        self.call(
            "assemble_json",
            self.inner.assemble,
            input,
            output_path,
            progress,
        )
    }
}

struct ProgressAdapter<'a> {
    progress: &'a dyn ProcessorProgress,
    violated: AtomicBool,
}

unsafe extern "C" fn progress_on_progress(context: *mut c_void, ratio: f64) -> VesperStatus {
    let Some(context) = std::ptr::NonNull::new(context.cast::<ProgressAdapter<'_>>()) else {
        return status::INVALID_ARGUMENT;
    };
    // SAFETY: the context points to a stack adapter that remains live for the
    // enclosing synchronous plugin call and may be shared by scoped workers.
    let adapter = unsafe { context.as_ref() };
    if !ratio.is_finite() || !(0.0..=1.0).contains(&ratio) {
        adapter.violated.store(true, Ordering::Release);
        return status::ABI_VIOLATION;
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        adapter.progress.on_progress(ratio as f32);
    })) {
        Ok(()) => status::OK,
        Err(_) => {
            adapter.violated.store(true, Ordering::Release);
            status::PANIC
        }
    }
}

unsafe extern "C" fn progress_is_cancelled(
    context: *mut c_void,
    out_cancelled: *mut u32,
) -> VesperStatus {
    let Some(context) = std::ptr::NonNull::new(context.cast::<ProgressAdapter<'_>>()) else {
        return status::INVALID_ARGUMENT;
    };
    // SAFETY: same scoped callback context contract as `progress_on_progress`.
    let adapter = unsafe { context.as_ref() };
    if out_cancelled.is_null() {
        adapter.violated.store(true, Ordering::Release);
        return status::INVALID_ARGUMENT;
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        adapter.progress.is_cancelled()
    })) {
        Ok(cancelled) => {
            // SAFETY: the plugin supplied a non-null output borrowed for this
            // callback and the ABI fixes its representation to 0 or 1.
            unsafe { *out_cancelled = u32::from(cancelled) };
            status::OK
        }
        Err(_) => {
            adapter.violated.store(true, Ordering::Release);
            // SAFETY: same validated output pointer as the success branch.
            unsafe { *out_cancelled = 1 };
            status::PANIC
        }
    }
}

fn map_processor_boundary(error: NativeAbiBoundaryError) -> ProcessorError {
    ProcessorError::AbiViolation(error.to_string())
}

#[derive(Debug)]
struct NativeAbiPipelineEventHookInner {
    runtime: Arc<InterfaceRuntime>,
    on_event: player_plugin_abi::VesperJsonCallFn,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeAbiPipelineEventHook {
    inner: Arc<NativeAbiPipelineEventHookInner>,
}

impl NativeAbiPipelineEventHook {
    pub(super) fn new(
        plugin_id: &str,
        instance_id: &str,
        owner: Arc<PluginOwner>,
        table: VesperPipelineEventHook,
    ) -> Result<Self, NativeAbiBoundaryError> {
        let runtime = Arc::new(InterfaceRuntime::new(
            owner,
            table.header.context,
            plugin_id,
            instance_id,
        )?);
        let on_event = table.on_event_json.ok_or_else(|| {
            runtime.contract_violation(
                "construct_wrapper",
                "on_event_json callback is missing after validation",
            )
        })?;
        Ok(Self {
            inner: Arc::new(NativeAbiPipelineEventHookInner { runtime, on_event }),
        })
    }
}

impl PipelineEventHook for NativeAbiPipelineEventHook {
    fn on_event(
        &self,
        event: &PipelineEvent,
    ) -> Result<PipelineEventHookOutcome, PipelineEventHookError> {
        event.validate()?;
        let input = serde_json::to_vec(event).map_err(|error| {
            PipelineEventHookError::PayloadCodec(format!(
                "serialize pipeline event failed: {error}"
            ))
        })?;
        if input.len() > MAX_PIPELINE_EVENT_INPUT_BYTES {
            return Err(PipelineEventHookError::ProtocolViolation(format!(
                "pipeline event input exceeds the {MAX_PIPELINE_EVENT_INPUT_BYTES}-byte transport limit"
            )));
        }
        let result = self
            .inner
            .runtime
            .invoke_json("on_event_json", &[status::FAILURE], |out| {
                // SAFETY: callback/context are validated and input/output are
                // borrowed for this synchronous call only.
                unsafe {
                    (self.inner.on_event)(self.inner.runtime.context(), borrowed_bytes(&input), out)
                }
            })
            .map_err(map_hook_boundary)?;
        match result {
            JsonCallResult::Success(payload) => {
                let outcome = self
                    .inner
                    .runtime
                    .decode_json::<PipelineEventHookOutcome>("on_event_json", &payload)
                    .map_err(map_hook_boundary)?;
                if let Err(error) = outcome.validate() {
                    return Err(map_hook_boundary(
                        self.inner
                            .runtime
                            .contract_violation("on_event_json", error.to_string()),
                    ));
                }
                Ok(outcome)
            }
            JsonCallResult::Failure { payload, .. } => {
                let error = self
                    .inner
                    .runtime
                    .decode_json::<PipelineEventHookError>("on_event_json", &payload)
                    .map_err(map_hook_boundary)?;
                if let Err(protocol_error) = error.validate_author_failure() {
                    return Err(map_hook_boundary(
                        self.inner
                            .runtime
                            .contract_violation("on_event_json", protocol_error.to_string()),
                    ));
                }
                Err(error)
            }
        }
    }
}

fn map_hook_boundary(error: NativeAbiBoundaryError) -> PipelineEventHookError {
    PipelineEventHookError::AbiViolation(error.to_string())
}

#[derive(Debug)]
struct NativeAbiBenchmarkSinkInner {
    runtime: Arc<InterfaceRuntime>,
    name: String,
    on_event_batch: player_plugin_abi::VesperJsonCallFn,
    flush: Option<player_plugin_abi::VesperGetJsonFn>,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeAbiBenchmarkSink {
    inner: Arc<NativeAbiBenchmarkSinkInner>,
}

impl NativeAbiBenchmarkSink {
    pub(super) fn new(
        plugin_id: &str,
        plugin_name: String,
        instance_id: &str,
        owner: Arc<PluginOwner>,
        table: VesperBenchmarkSink,
    ) -> Result<Self, NativeAbiBoundaryError> {
        let runtime = Arc::new(InterfaceRuntime::new(
            owner,
            table.header.context,
            plugin_id,
            instance_id,
        )?);
        let on_event_batch = table.on_event_batch_json.ok_or_else(|| {
            runtime.contract_violation(
                "construct_wrapper",
                "on_event_batch_json callback is missing after validation",
            )
        })?;
        Ok(Self {
            inner: Arc::new(NativeAbiBenchmarkSinkInner {
                runtime,
                name: plugin_name,
                on_event_batch,
                flush: table.flush_json,
            }),
        })
    }

    fn decode_failure(
        &self,
        operation: &'static str,
        payload: &[u8],
    ) -> Result<BenchmarkSinkError, BenchmarkSinkError> {
        self.inner
            .runtime
            .decode_json::<BenchmarkSinkError>(operation, payload)
            .map_err(map_benchmark_boundary)
    }
}

impl BenchmarkSink for NativeAbiBenchmarkSink {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn on_event_batch(
        &self,
        batch: &BenchmarkEventBatch,
    ) -> Result<BenchmarkSinkStatus, BenchmarkSinkError> {
        batch.validate()?;
        let input = serde_json::to_vec(batch).map_err(|error| {
            BenchmarkSinkError::PayloadCodec(format!(
                "serialize benchmark event batch failed: {error}"
            ))
        })?;
        let result = self
            .inner
            .runtime
            .invoke_json("on_event_batch_json", &[status::FAILURE], |out| {
                // SAFETY: callback/context are validated and input/output are
                // borrowed for this synchronous call only.
                unsafe {
                    (self.inner.on_event_batch)(
                        self.inner.runtime.context(),
                        borrowed_bytes(&input),
                        out,
                    )
                }
            })
            .map_err(map_benchmark_boundary)?;
        match result {
            JsonCallResult::Success(payload) => {
                let sink_status = self
                    .inner
                    .runtime
                    .decode_json::<BenchmarkSinkStatus>("on_event_batch_json", &payload)
                    .map_err(map_benchmark_boundary)?;
                if let Err(error) = sink_status.validate_for_batch(batch.events.len()) {
                    return Err(map_benchmark_boundary(
                        self.inner
                            .runtime
                            .contract_violation("on_event_batch_json", error.to_string()),
                    ));
                }
                Ok(sink_status)
            }
            JsonCallResult::Failure { payload, .. } => self
                .decode_failure("on_event_batch_json", &payload)
                .and_then(Err),
        }
    }

    fn flush(&self) -> Result<BenchmarkSinkReport, BenchmarkSinkError> {
        self.inner
            .runtime
            .ensure_healthy("flush_json")
            .map_err(map_benchmark_boundary)?;
        let Some(flush) = self.inner.flush else {
            return Ok(BenchmarkSinkReport::default());
        };
        let result = self
            .inner
            .runtime
            .invoke_json("flush_json", &[status::FAILURE], |out| {
                // SAFETY: callback/context are validated and output is borrowed
                // for this synchronous call only.
                unsafe { flush(self.inner.runtime.context(), out) }
            })
            .map_err(map_benchmark_boundary)?;
        match result {
            JsonCallResult::Success(payload) => {
                let report = self
                    .inner
                    .runtime
                    .decode_json::<BenchmarkSinkReport>("flush_json", &payload)
                    .map_err(map_benchmark_boundary)?;
                if let Err(error) = report.validate() {
                    return Err(map_benchmark_boundary(
                        self.inner
                            .runtime
                            .contract_violation("flush_json", error.to_string()),
                    ));
                }
                Ok(report)
            }
            JsonCallResult::Failure { payload, .. } => {
                self.decode_failure("flush_json", &payload).and_then(Err)
            }
        }
    }
}

fn map_benchmark_boundary(error: NativeAbiBoundaryError) -> BenchmarkSinkError {
    BenchmarkSinkError::AbiViolation(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use player_plugin::{
        AssemblyMode, BenchmarkEvent, ContentFormatKind, DownloadMetadata, Plugin, PluginBuilder,
    };

    use super::*;
    use crate::native_abi::{CheckedInterfaceTable, CheckedPluginRoot};

    const POST_INSTANCE: &str = "dev.vesper.fixture.post-download";
    const HOOK_INSTANCE: &str = "dev.vesper.fixture.event-hook";
    const BENCHMARK_INSTANCE: &str = "dev.vesper.fixture.benchmark";
    const SUPPORTED_FORMATS: &[ContentFormatKind] = &[ContentFormatKind::SingleFile];

    fn pipeline_event(event_name: &str) -> PipelineEvent {
        PipelineEvent {
            run_id: "run-1".to_owned(),
            session_id: "session-1".to_owned(),
            platform: "test".to_owned(),
            protocol: None,
            event_name: event_name.to_owned(),
            timestamp_ns: 1,
            thread: None,
            resource_identity: Some("download-task:1".to_owned()),
            attributes: BTreeMap::new(),
            diagnostic: None,
        }
    }

    struct FixtureProcessor;

    impl PostDownloadProcessor for FixtureProcessor {
        fn name(&self) -> &str {
            "fixture processor"
        }

        fn supported_input_formats(&self) -> &[ContentFormatKind] {
            SUPPORTED_FORMATS
        }

        fn process(
            &self,
            input: &CompletedDownloadInfo,
            _output_path: &Path,
            progress: &dyn ProcessorProgress,
        ) -> Result<ProcessorOutput, ProcessorError> {
            progress.on_progress(0.5);
            if input.asset_id == "cancel" || progress.is_cancelled() {
                Err(ProcessorError::Cancelled)
            } else {
                Ok(ProcessorOutput::Skipped)
            }
        }
    }

    struct FixtureHook;

    impl PipelineEventHook for FixtureHook {
        fn on_event(
            &self,
            event: &PipelineEvent,
        ) -> Result<PipelineEventHookOutcome, PipelineEventHookError> {
            match event.event_name.as_str() {
                "fail" => Err(PipelineEventHookError::Failed("fixture failure".to_owned())),
                "forged-abi-error" => Err(PipelineEventHookError::AbiViolation(
                    "forged by author code".to_owned(),
                )),
                _ => Ok(PipelineEventHookOutcome::accepted()),
            }
        }
    }

    struct FixtureBenchmark;

    impl BenchmarkSink for FixtureBenchmark {
        fn name(&self) -> &str {
            "fixture benchmark"
        }

        fn on_event_batch(
            &self,
            batch: &BenchmarkEventBatch,
        ) -> Result<BenchmarkSinkStatus, BenchmarkSinkError> {
            if batch.events.iter().any(|event| event.event_name == "fail") {
                Err(BenchmarkSinkError::SinkFailed("fixture failure".to_owned()))
            } else {
                Ok(BenchmarkSinkStatus {
                    accepted_events: batch.events.len() as u64,
                })
            }
        }

        fn flush(&self) -> Result<BenchmarkSinkReport, BenchmarkSinkError> {
            Ok(BenchmarkSinkReport {
                accepted_events: 3,
                ..BenchmarkSinkReport::default()
            })
        }
    }

    fn fixture_plugin() -> Plugin {
        PluginBuilder::new("dev.vesper.fixture", "Plugin fixture")
            .expect("builder")
            .with_post_download_processor(POST_INSTANCE, FixtureProcessor)
            .expect("post-download interface")
            .with_pipeline_event_hook(HOOK_INSTANCE, FixtureHook)
            .expect("event-hook interface")
            .with_benchmark_sink(BENCHMARK_INSTANCE, FixtureBenchmark)
            .expect("benchmark interface")
            .build()
            .expect("plugin")
    }

    fn stable_wrappers() -> (
        NativeAbiPostDownloadProcessor,
        NativeAbiPipelineEventHook,
        NativeAbiBenchmarkSink,
    ) {
        let root_ptr = player_plugin::__private::export_plugin(fixture_plugin);
        let root =
            // SAFETY: the generated export returns a live root whose ownership
            // transfers into `CheckedPluginRoot` and its wrapper Arcs.
            unsafe { CheckedPluginRoot::from_raw(root_ptr, None) }.expect("checked root");
        let mut post_download = None;
        let mut hook = None;
        let mut benchmark = None;
        for interface in &root.interfaces {
            match interface.table {
                CheckedInterfaceTable::PostDownload(table) => {
                    post_download = Some(
                        NativeAbiPostDownloadProcessor::new(
                            &root.plugin_id,
                            root.plugin_name.clone(),
                            &interface.descriptor.instance_id,
                            root.owner.clone(),
                            table,
                        )
                        .expect("post-download wrapper"),
                    );
                }
                CheckedInterfaceTable::PipelineEventHook(table) => {
                    hook = Some(
                        NativeAbiPipelineEventHook::new(
                            &root.plugin_id,
                            &interface.descriptor.instance_id,
                            root.owner.clone(),
                            table,
                        )
                        .expect("event-hook wrapper"),
                    );
                }
                CheckedInterfaceTable::BenchmarkSink(table) => {
                    benchmark = Some(
                        NativeAbiBenchmarkSink::new(
                            &root.plugin_id,
                            root.plugin_name.clone(),
                            &interface.descriptor.instance_id,
                            root.owner.clone(),
                            table,
                        )
                        .expect("benchmark wrapper"),
                    );
                }
                _ => {}
            }
        }
        (
            post_download.expect("post-download capability"),
            hook.expect("event-hook capability"),
            benchmark.expect("benchmark capability"),
        )
    }

    fn completed_download(asset_id: &str) -> CompletedDownloadInfo {
        CompletedDownloadInfo {
            asset_id: asset_id.to_owned(),
            task_id: None,
            content_format: player_plugin::CompletedContentFormat::SingleFile {
                path: PathBuf::from("input.mp4"),
            },
            metadata: DownloadMetadata::default(),
            streams: Vec::new(),
            assembly_mode: AssemblyMode::Single,
        }
    }

    fn benchmark_batch(event_name: &str) -> BenchmarkEventBatch {
        BenchmarkEventBatch {
            events: vec![BenchmarkEvent {
                run_id: "run".to_owned(),
                session_id: "session".to_owned(),
                platform: "test".to_owned(),
                source_protocol: None,
                event_name: event_name.to_owned(),
                timestamp_ns: 1,
                elapsed_ns: 1,
                thread: None,
                attributes: BTreeMap::new(),
            }],
        }
    }

    struct NoProgress;

    impl ProcessorProgress for NoProgress {
        fn on_progress(&self, _ratio: f32) {}
    }

    #[test]
    fn generated_stable_interfaces_round_trip_after_root_value_drops() {
        let (processor, hook, benchmark) = stable_wrappers();
        assert_eq!(processor.name(), "Plugin fixture");
        assert_eq!(processor.supported_input_formats(), SUPPORTED_FORMATS);
        assert_eq!(
            processor.process(
                &completed_download("ok"),
                Path::new("output.mp4"),
                &NoProgress,
            ),
            Ok(ProcessorOutput::Skipped)
        );
        assert_eq!(
            hook.on_event(&pipeline_event("ok")),
            Ok(PipelineEventHookOutcome::accepted())
        );
        assert_eq!(
            benchmark.on_event_batch(&benchmark_batch("tick")),
            Ok(BenchmarkSinkStatus { accepted_events: 1 })
        );
        assert_eq!(benchmark.flush().expect("flush").accepted_events, 3);
    }

    #[test]
    fn typed_failures_do_not_poison_stable_interfaces() {
        let (processor, hook, benchmark) = stable_wrappers();
        assert_eq!(
            processor.process(
                &completed_download("cancel"),
                Path::new("output.mp4"),
                &NoProgress,
            ),
            Err(ProcessorError::Cancelled)
        );
        assert_eq!(
            processor.process(
                &completed_download("ok"),
                Path::new("output.mp4"),
                &NoProgress,
            ),
            Ok(ProcessorOutput::Skipped)
        );

        assert!(matches!(
            hook.on_event(&pipeline_event("fail")),
            Err(PipelineEventHookError::Failed(_))
        ));
        assert!(hook.on_event(&pipeline_event("ok")).is_ok());

        assert!(matches!(
            benchmark.on_event_batch(&benchmark_batch("fail")),
            Err(BenchmarkSinkError::SinkFailed(_))
        ));
        assert!(benchmark.on_event_batch(&benchmark_batch("tick")).is_ok());
    }

    #[test]
    fn invalid_pipeline_event_is_rejected_without_poisoning_the_interface() {
        let (_, hook, _) = stable_wrappers();
        let mut event = pipeline_event("vendor.future.event");
        event.platform.clear();

        assert!(matches!(
            hook.on_event(&event),
            Err(PipelineEventHookError::ProtocolViolation(_))
        ));
        assert!(hook.on_event(&pipeline_event("still-usable")).is_ok());
    }

    #[test]
    fn host_owned_author_error_poisons_the_native_interface() {
        let (_, hook, _) = stable_wrappers();

        assert!(matches!(
            hook.on_event(&pipeline_event("forged-abi-error")),
            Err(PipelineEventHookError::AbiViolation(_))
        ));
        assert!(matches!(
            hook.on_event(&pipeline_event("after-poison")),
            Err(PipelineEventHookError::AbiViolation(_))
        ));
    }

    static OWNER_DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct DropCountingHook;

    impl Drop for DropCountingHook {
        fn drop(&mut self) {
            OWNER_DROP_COUNT.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl PipelineEventHook for DropCountingHook {
        fn on_event(
            &self,
            _event: &PipelineEvent,
        ) -> Result<PipelineEventHookOutcome, PipelineEventHookError> {
            Ok(PipelineEventHookOutcome::accepted())
        }
    }

    fn drop_counting_plugin() -> Plugin {
        PluginBuilder::new("dev.vesper.owner-fixture", "Owner fixture")
            .expect("builder")
            .with_pipeline_event_hook("dev.vesper.owner-fixture.hook", DropCountingHook)
            .expect("hook")
            .build()
            .expect("plugin")
    }

    #[test]
    fn last_wrapper_reference_destroys_the_root_owner_once() {
        OWNER_DROP_COUNT.store(0, Ordering::SeqCst);
        let root_ptr = player_plugin::__private::export_plugin(drop_counting_plugin);
        let root =
            // SAFETY: generated root ownership transfers into the checked root.
            unsafe { CheckedPluginRoot::from_raw(root_ptr, None) }.expect("checked root");
        let interface = root.interfaces.first().expect("hook interface");
        let CheckedInterfaceTable::PipelineEventHook(table) = interface.table else {
            panic!("expected hook table");
        };
        let first = NativeAbiPipelineEventHook::new(
            &root.plugin_id,
            &interface.descriptor.instance_id,
            root.owner.clone(),
            table,
        )
        .expect("hook wrapper");
        let second = first.clone();
        drop(root);
        assert_eq!(OWNER_DROP_COUNT.load(Ordering::SeqCst), 0);
        drop(first);
        assert_eq!(OWNER_DROP_COUNT.load(Ordering::SeqCst), 0);
        drop(second);
        assert_eq!(OWNER_DROP_COUNT.load(Ordering::SeqCst), 1);
    }
}
