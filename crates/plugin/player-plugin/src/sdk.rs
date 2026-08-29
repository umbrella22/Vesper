//! Safe author-facing construction and generated native adapters.

#![deny(unsafe_code)]

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use player_plugin_abi::export::{
    ExportFailure, ExportInterface as RawExportInterface, ExportInterfaceKind, ExportInvocation,
    ExportOperation, ExportPlugin as RawExportPlugin, ExportProgress,
};
use player_plugin_abi::{
    VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES, VESPER_MAX_PLUGIN_ID_BYTES,
    VESPER_MAX_PLUGIN_NAME_BYTES, status,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::plugin_reference::is_reverse_dns;
use crate::{
    BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, MAX_PIPELINE_EVENT_INPUT_BYTES,
    PipelineEvent, PipelineEventHook, PipelineEventHookError, PostDownloadProcessor,
    ProcessorError, ProcessorProgress,
};

mod session;
mod session_capabilities;

/// Capability families that can be exported by a native Rust plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginCapability {
    PostDownloadProcessor,
    PipelineEventHook,
    BenchmarkSink,
    NativeDecoder,
    FrameProcessor,
    AudioProcessor,
    SourceNormalizerPacket,
    SourceNormalizerResource,
}

impl From<ExportInterfaceKind> for PluginCapability {
    fn from(value: ExportInterfaceKind) -> Self {
        match value {
            ExportInterfaceKind::PostDownloadProcessor => Self::PostDownloadProcessor,
            ExportInterfaceKind::PipelineEventHook => Self::PipelineEventHook,
            ExportInterfaceKind::BenchmarkSink => Self::BenchmarkSink,
            ExportInterfaceKind::NativeDecoder => Self::NativeDecoder,
            ExportInterfaceKind::FrameProcessor => Self::FrameProcessor,
            ExportInterfaceKind::AudioProcessor => Self::AudioProcessor,
            ExportInterfaceKind::SourceNormalizerPacket => Self::SourceNormalizerPacket,
            ExportInterfaceKind::SourceNormalizerResource => Self::SourceNormalizerResource,
        }
    }
}

/// Errors reported while constructing one exported plugin root.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PluginBuildError {
    #[error("plugin_id must be a valid reverse-DNS identity")]
    InvalidPluginId,
    #[error("capability instance id must be a valid reverse-DNS identity")]
    InvalidCapabilityInstanceId,
    #[error("plugin_name must contain between 1 and {VESPER_MAX_PLUGIN_NAME_BYTES} UTF-8 bytes")]
    InvalidPluginName,
    #[error("plugin must expose at least one capability interface")]
    NoInterfaces,
    #[error("duplicate {capability:?} capability instance `{instance_id}`")]
    DuplicateInterface {
        capability: PluginCapability,
        instance_id: String,
    },
}

/// Safe builder used by Rust plugin authors.
pub struct PluginBuilder {
    plugin_id: String,
    plugin_name: String,
    interfaces: Vec<Arc<dyn RawExportInterface>>,
    interface_keys: HashSet<(ExportInterfaceKind, String)>,
}

impl PluginBuilder {
    pub fn new(
        plugin_id: impl Into<String>,
        plugin_name: impl Into<String>,
    ) -> Result<Self, PluginBuildError> {
        let plugin_id = plugin_id.into();
        if !is_reverse_dns(&plugin_id, VESPER_MAX_PLUGIN_ID_BYTES) {
            return Err(PluginBuildError::InvalidPluginId);
        }
        let plugin_name = plugin_name.into();
        if plugin_name.is_empty() || plugin_name.len() > VESPER_MAX_PLUGIN_NAME_BYTES {
            return Err(PluginBuildError::InvalidPluginName);
        }
        Ok(Self {
            plugin_id,
            plugin_name,
            interfaces: Vec::new(),
            interface_keys: HashSet::new(),
        })
    }

    pub fn with_post_download_processor<P>(
        self,
        instance_id: impl Into<String>,
        processor: P,
    ) -> Result<Self, PluginBuildError>
    where
        P: PostDownloadProcessor + 'static,
    {
        let instance_id = instance_id.into();
        self.with_interface(
            ExportInterfaceKind::PostDownloadProcessor,
            instance_id.clone(),
            Arc::new(PostDownloadAdapter {
                instance_id,
                processor,
            }),
        )
    }

    pub fn with_pipeline_event_hook<H>(
        self,
        instance_id: impl Into<String>,
        hook: H,
    ) -> Result<Self, PluginBuildError>
    where
        H: PipelineEventHook + 'static,
    {
        let instance_id = instance_id.into();
        self.with_interface(
            ExportInterfaceKind::PipelineEventHook,
            instance_id.clone(),
            Arc::new(PipelineEventHookAdapter { instance_id, hook }),
        )
    }

    pub fn with_benchmark_sink<S>(
        self,
        instance_id: impl Into<String>,
        sink: S,
    ) -> Result<Self, PluginBuildError>
    where
        S: BenchmarkSink + 'static,
    {
        let instance_id = instance_id.into();
        self.with_interface(
            ExportInterfaceKind::BenchmarkSink,
            instance_id.clone(),
            Arc::new(BenchmarkSinkAdapter { instance_id, sink }),
        )
    }

    pub fn with_native_decoder<F>(
        self,
        instance_id: impl Into<String>,
        factory: F,
    ) -> Result<Self, PluginBuildError>
    where
        F: crate::NativeDecoderPluginFactory + 'static,
    {
        let instance_id = instance_id.into();
        self.with_interface(
            ExportInterfaceKind::NativeDecoder,
            instance_id.clone(),
            Arc::new(session_capabilities::NativeDecoderAdapter::new(
                instance_id,
                factory,
            )),
        )
    }

    pub fn with_frame_processor<F>(
        self,
        instance_id: impl Into<String>,
        factory: F,
    ) -> Result<Self, PluginBuildError>
    where
        F: crate::FrameProcessorPluginFactory + 'static,
    {
        let instance_id = instance_id.into();
        self.with_interface(
            ExportInterfaceKind::FrameProcessor,
            instance_id.clone(),
            Arc::new(session_capabilities::FrameProcessorAdapter::new(
                instance_id,
                factory,
            )),
        )
    }

    pub fn with_audio_processor<F>(
        self,
        instance_id: impl Into<String>,
        factory: F,
    ) -> Result<Self, PluginBuildError>
    where
        F: crate::AudioProcessorPluginFactory + 'static,
    {
        let instance_id = instance_id.into();
        self.with_interface(
            ExportInterfaceKind::AudioProcessor,
            instance_id.clone(),
            Arc::new(session_capabilities::AudioProcessorAdapter::new(
                instance_id,
                factory,
            )),
        )
    }

    pub fn with_source_normalizer_packet<F>(
        self,
        instance_id: impl Into<String>,
        factory: F,
    ) -> Result<Self, PluginBuildError>
    where
        F: crate::SourceNormalizerPacketPluginFactory + 'static,
    {
        let instance_id = instance_id.into();
        self.with_interface(
            ExportInterfaceKind::SourceNormalizerPacket,
            instance_id.clone(),
            Arc::new(session_capabilities::SourceNormalizerPacketAdapter::new(
                instance_id,
                factory,
            )),
        )
    }

    pub fn with_source_normalizer_resource<F>(
        self,
        instance_id: impl Into<String>,
        factory: F,
    ) -> Result<Self, PluginBuildError>
    where
        F: crate::SourceNormalizerResourcePluginFactory + 'static,
    {
        let instance_id = instance_id.into();
        self.with_interface(
            ExportInterfaceKind::SourceNormalizerResource,
            instance_id.clone(),
            Arc::new(session_capabilities::SourceNormalizerResourceAdapter::new(
                instance_id,
                factory,
            )),
        )
    }

    pub fn build(self) -> Result<Plugin, PluginBuildError> {
        if self.interfaces.is_empty() {
            return Err(PluginBuildError::NoInterfaces);
        }
        Ok(Plugin {
            plugin_id: self.plugin_id,
            plugin_name: self.plugin_name,
            interfaces: self.interfaces,
        })
    }

    fn with_interface(
        mut self,
        kind: ExportInterfaceKind,
        instance_id: String,
        interface: Arc<dyn RawExportInterface>,
    ) -> Result<Self, PluginBuildError> {
        if !is_reverse_dns(&instance_id, VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES) {
            return Err(PluginBuildError::InvalidCapabilityInstanceId);
        }
        if !self.interface_keys.insert((kind, instance_id.clone())) {
            return Err(PluginBuildError::DuplicateInterface {
                capability: kind.into(),
                instance_id,
            });
        }
        self.interfaces.push(interface);
        Ok(self)
    }
}

/// Fully validated plugin definition returned by an exported factory.
pub struct Plugin {
    plugin_id: String,
    plugin_name: String,
    interfaces: Vec<Arc<dyn RawExportInterface>>,
}

impl Plugin {
    pub fn builder(
        plugin_id: impl Into<String>,
        plugin_name: impl Into<String>,
    ) -> Result<PluginBuilder, PluginBuildError> {
        PluginBuilder::new(plugin_id, plugin_name)
    }

    fn invalid() -> Self {
        Self {
            plugin_id: String::new(),
            plugin_name: String::new(),
            interfaces: Vec::new(),
        }
    }
}

impl RawExportPlugin for Plugin {
    fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    fn plugin_name(&self) -> &str {
        &self.plugin_name
    }

    fn interfaces(&self) -> Vec<Arc<dyn RawExportInterface>> {
        self.interfaces.clone()
    }
}

#[doc(hidden)]
pub trait PluginFactoryResult {
    fn into_plugin(self) -> Option<Plugin>;
}

impl PluginFactoryResult for Plugin {
    fn into_plugin(self) -> Option<Plugin> {
        Some(self)
    }
}

impl<E> PluginFactoryResult for Result<Plugin, E> {
    fn into_plugin(self) -> Option<Plugin> {
        self.ok()
    }
}

pub(crate) fn export_plugin<R>(factory: fn() -> R) -> *const player_plugin_abi::VesperPluginRoot
where
    R: PluginFactoryResult,
{
    player_plugin_abi::export::export_plugin(move || {
        factory().into_plugin().unwrap_or_else(Plugin::invalid)
    })
}

impl ProcessorProgress for ExportProgress<'_> {
    fn on_progress(&self, ratio: f32) {
        ExportProgress::on_progress(self, f64::from(ratio));
    }

    fn is_cancelled(&self) -> bool {
        ExportProgress::is_cancelled(self)
    }
}

struct PostDownloadAdapter<P> {
    instance_id: String,
    processor: P,
}

impl<P> RawExportInterface for PostDownloadAdapter<P>
where
    P: PostDownloadProcessor,
{
    fn kind(&self) -> ExportInterfaceKind {
        ExportInterfaceKind::PostDownloadProcessor
    }

    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn invoke(&self, operation: ExportOperation<'_>) -> Result<ExportInvocation, ExportFailure> {
        match operation {
            ExportOperation::Capabilities => json_invocation(&self.processor.capabilities()),
            ExportOperation::PostDownloadProcess {
                input_json,
                output_path,
                progress,
                assemble,
            } => {
                let input =
                    decode::<crate::CompletedDownloadInfo>(input_json).map_err(|error| {
                        failure(
                            status::INVALID_ARGUMENT,
                            &ProcessorError::PayloadCodec(error),
                        )
                    })?;
                let output_path = std::str::from_utf8(output_path).map_err(|error| {
                    failure(
                        status::INVALID_ARGUMENT,
                        &ProcessorError::OutputPath(error.to_string()),
                    )
                })?;
                let result = if assemble {
                    self.processor
                        .assemble(&input, Path::new(output_path), &progress)
                } else {
                    self.processor
                        .process(&input, Path::new(output_path), &progress)
                };
                match result {
                    Ok(output) => json_invocation(&output),
                    Err(error) => {
                        let status = match error {
                            ProcessorError::Cancelled => status::CANCELLED,
                            ProcessorError::AbiViolation(_) => status::ABI_VIOLATION,
                            _ => status::FAILURE,
                        };
                        Err(failure(status, &error))
                    }
                }
            }
            _ => Err(unexpected_operation("post-download processor")),
        }
    }
}

struct PipelineEventHookAdapter<H> {
    instance_id: String,
    hook: H,
}

impl<H> RawExportInterface for PipelineEventHookAdapter<H>
where
    H: PipelineEventHook,
{
    fn kind(&self) -> ExportInterfaceKind {
        ExportInterfaceKind::PipelineEventHook
    }

    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn invoke(&self, operation: ExportOperation<'_>) -> Result<ExportInvocation, ExportFailure> {
        let ExportOperation::PipelineEvent { event_json } = operation else {
            return Err(unexpected_operation("pipeline event hook"));
        };
        if event_json.len() > MAX_PIPELINE_EVENT_INPUT_BYTES {
            return Err(failure(
                status::INVALID_ARGUMENT,
                &PipelineEventHookError::ProtocolViolation(format!(
                    "pipeline event input exceeds the {MAX_PIPELINE_EVENT_INPUT_BYTES}-byte transport limit"
                )),
            ));
        }
        let event = decode::<PipelineEvent>(event_json).map_err(|error| {
            failure(
                status::INVALID_ARGUMENT,
                &PipelineEventHookError::PayloadCodec(error),
            )
        })?;
        event
            .validate()
            .map_err(|error| failure(status::INVALID_ARGUMENT, &error))?;
        let outcome = match self.hook.on_event(&event) {
            Ok(outcome) => outcome,
            Err(error) => {
                if let Err(protocol_error) = error.validate_author_failure() {
                    return Err(failure(status::ABI_VIOLATION, &protocol_error));
                }
                return Err(failure(status::FAILURE, &error));
            }
        };
        outcome
            .validate()
            .map_err(|error| failure(status::ABI_VIOLATION, &error))?;
        json_invocation(&outcome)
    }
}

struct BenchmarkSinkAdapter<S> {
    instance_id: String,
    sink: S,
}

impl<S> RawExportInterface for BenchmarkSinkAdapter<S>
where
    S: BenchmarkSink,
{
    fn kind(&self) -> ExportInterfaceKind {
        ExportInterfaceKind::BenchmarkSink
    }

    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn invoke(&self, operation: ExportOperation<'_>) -> Result<ExportInvocation, ExportFailure> {
        match operation {
            ExportOperation::BenchmarkBatch { batch_json } => {
                let batch = decode::<BenchmarkEventBatch>(batch_json).map_err(|error| {
                    failure(
                        status::INVALID_ARGUMENT,
                        &BenchmarkSinkError::PayloadCodec(error),
                    )
                })?;
                batch
                    .validate()
                    .map_err(|error| failure(status::INVALID_ARGUMENT, &error))?;
                let sink_status = self
                    .sink
                    .on_event_batch(&batch)
                    .map_err(|error| failure(status::FAILURE, &error))?;
                sink_status
                    .validate_for_batch(batch.events.len())
                    .map_err(|error| failure(status::ABI_VIOLATION, &error))?;
                json_invocation(&sink_status)
            }
            ExportOperation::BenchmarkFlush => {
                let report = self
                    .sink
                    .flush()
                    .map_err(|error| failure(status::FAILURE, &error))?;
                report
                    .validate()
                    .map_err(|error| failure(status::ABI_VIOLATION, &error))?;
                json_invocation(&report)
            }
            _ => Err(unexpected_operation("benchmark sink")),
        }
    }
}

fn decode<T>(bytes: &[u8]) -> Result<T, String>
where
    T: DeserializeOwned,
{
    serde_json::from_slice(bytes).map_err(|error| error.to_string())
}

fn json_invocation<T>(value: &T) -> Result<ExportInvocation, ExportFailure>
where
    T: Serialize,
{
    encode(value).map(ExportInvocation::Json)
}

fn encode<T>(value: &T) -> Result<Vec<u8>, ExportFailure>
where
    T: Serialize,
{
    serde_json::to_vec(value).map_err(|error| {
        ExportFailure::with_status(status::ABI_VIOLATION, error.to_string().into_bytes())
    })
}

fn failure<T>(failure_status: u32, value: &T) -> ExportFailure
where
    T: Serialize,
{
    let payload = serde_json::to_vec(value).unwrap_or_else(|error| error.to_string().into_bytes());
    ExportFailure::with_status(failure_status, payload)
}

fn unexpected_operation(interface: &str) -> ExportFailure {
    ExportFailure::with_status(
        status::ABI_VIOLATION,
        format!("unexpected operation for {interface}").into_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PipelineEventHookOutcome, PluginDiagnostic};

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
            attributes: Default::default(),
            diagnostic: None,
        }
    }

    struct Hook;

    impl PipelineEventHook for Hook {
        fn on_event(
            &self,
            _event: &PipelineEvent,
        ) -> Result<PipelineEventHookOutcome, PipelineEventHookError> {
            Ok(PipelineEventHookOutcome::accepted())
        }
    }

    #[test]
    fn builder_validates_identity_and_duplicate_interfaces() {
        assert!(matches!(
            PluginBuilder::new("invalid", "Fixture"),
            Err(PluginBuildError::InvalidPluginId)
        ));
        let builder = PluginBuilder::new("dev.vesper.fixture", "Fixture")
            .and_then(|builder| builder.with_pipeline_event_hook("dev.vesper.fixture.hook", Hook))
            .expect("first interface");
        assert!(matches!(
            builder.with_pipeline_event_hook("dev.vesper.fixture.hook", Hook),
            Err(PluginBuildError::DuplicateInterface { .. })
        ));
    }

    #[test]
    fn hook_adapter_round_trips_typed_json() {
        let plugin = PluginBuilder::new("dev.vesper.fixture", "Fixture")
            .and_then(|builder| builder.with_pipeline_event_hook("dev.vesper.fixture.hook", Hook))
            .and_then(PluginBuilder::build)
            .expect("plugin");
        let event = pipeline_event("download.task.completed");
        let input = serde_json::to_vec(&event).expect("serialize event");
        let invocation = plugin.interfaces[0]
            .invoke(ExportOperation::PipelineEvent { event_json: &input })
            .expect("invoke hook");
        let ExportInvocation::Json(output) = invocation else {
            panic!("expected JSON output");
        };
        let outcome: PipelineEventHookOutcome =
            serde_json::from_slice(&output).expect("decode outcome");
        assert!(outcome.accepted);
        assert!(outcome.diagnostics.is_empty());
    }

    #[test]
    fn hook_adapter_rejects_protocol_violations() {
        struct InvalidHook;

        impl PipelineEventHook for InvalidHook {
            fn on_event(
                &self,
                _event: &PipelineEvent,
            ) -> Result<PipelineEventHookOutcome, PipelineEventHookError> {
                Ok(PipelineEventHookOutcome {
                    accepted: true,
                    measurements: Vec::new(),
                    diagnostics: vec![PluginDiagnostic {
                        code: "x".repeat(300),
                        severity: crate::PluginDiagnosticSeverity::Error,
                        message: "invalid".to_owned(),
                        attributes: Default::default(),
                    }],
                })
            }
        }

        let plugin = PluginBuilder::new("dev.vesper.fixture", "Fixture")
            .and_then(|builder| {
                builder.with_pipeline_event_hook("dev.vesper.fixture.hook", InvalidHook)
            })
            .and_then(PluginBuilder::build)
            .expect("plugin");
        let input = serde_json::to_vec(&pipeline_event("download.task.completed"))
            .expect("serialize event");
        assert!(
            plugin.interfaces[0]
                .invoke(ExportOperation::PipelineEvent { event_json: &input })
                .is_err()
        );
    }

    #[test]
    fn hook_adapter_rejects_oversized_input_before_invoking_author_code() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountingHook(Arc<AtomicUsize>);

        impl PipelineEventHook for CountingHook {
            fn on_event(
                &self,
                _event: &PipelineEvent,
            ) -> Result<PipelineEventHookOutcome, PipelineEventHookError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(PipelineEventHookOutcome::accepted())
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let plugin = PluginBuilder::new("dev.vesper.fixture", "Fixture")
            .and_then(|builder| {
                builder.with_pipeline_event_hook(
                    "dev.vesper.fixture.hook",
                    CountingHook(calls.clone()),
                )
            })
            .and_then(PluginBuilder::build)
            .expect("plugin");
        let input = vec![b' '; MAX_PIPELINE_EVENT_INPUT_BYTES + 1];
        let error = plugin.interfaces[0]
            .invoke(ExportOperation::PipelineEvent { event_json: &input })
            .expect_err("oversized input must be rejected");

        assert_eq!(error.status(), status::INVALID_ARGUMENT);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn hook_adapter_accepts_unknown_event_names_and_ignores_appended_json_fields() {
        let plugin = PluginBuilder::new("dev.vesper.fixture", "Fixture")
            .and_then(|builder| builder.with_pipeline_event_hook("dev.vesper.fixture.hook", Hook))
            .and_then(PluginBuilder::build)
            .expect("plugin");
        let mut value = serde_json::to_value(pipeline_event("vendor.future.event"))
            .expect("serialize event value");
        value.as_object_mut().expect("event object").insert(
            "futureField".to_owned(),
            serde_json::json!({ "nested": true }),
        );
        let input = serde_json::to_vec(&value).expect("serialize extended event");

        assert!(
            plugin.interfaces[0]
                .invoke(ExportOperation::PipelineEvent { event_json: &input })
                .is_ok()
        );
    }

    #[test]
    fn hook_adapter_marks_host_owned_author_errors_as_abi_violations() {
        struct InvalidErrorHook;

        impl PipelineEventHook for InvalidErrorHook {
            fn on_event(
                &self,
                _event: &PipelineEvent,
            ) -> Result<PipelineEventHookOutcome, PipelineEventHookError> {
                Err(PipelineEventHookError::AbiViolation(
                    "forged by author code".to_owned(),
                ))
            }
        }

        let plugin = PluginBuilder::new("dev.vesper.fixture", "Fixture")
            .and_then(|builder| {
                builder.with_pipeline_event_hook("dev.vesper.fixture.hook", InvalidErrorHook)
            })
            .and_then(PluginBuilder::build)
            .expect("plugin");
        let input = serde_json::to_vec(&pipeline_event("download.task.completed"))
            .expect("serialize event");
        let error = plugin.interfaces[0]
            .invoke(ExportOperation::PipelineEvent { event_json: &input })
            .expect_err("host-owned error kind must fail the ABI contract");

        assert_eq!(error.status(), status::ABI_VIOLATION);
    }
}
