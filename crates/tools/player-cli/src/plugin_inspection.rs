use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use player_cli::{
    PLUGIN_CATALOG_MIGRATION_VERSION, PLUGIN_CATALOG_SCHEMA_VERSION, PluginArtifactDescriptor,
    PluginArtifactTransport, PluginCapabilityDescriptor, PluginDescriptor, PluginProjectManifest,
    PluginProvision, PluginRequirement,
};
use player_plugin::{
    AssemblyMode, AudioPitchMode, AudioPlaybackPolicy, AudioProcessorCapabilities,
    AudioProcessorError, AudioProcessorSessionConfig, BenchmarkEvent, BenchmarkEventBatch,
    BenchmarkSinkError, CompletedContentFormat, CompletedDownloadInfo, ContentFormatKind,
    DecoderFrameFormat, DecoderPcmFrame, DecoderPcmFrameMetadata, DecoderPcmSampleLayout,
    DownloadMetadata, PipelineEvent, PipelineEventHookError, PluginReference, PluginTransport,
    ProcessorError, ProcessorProgress,
};
use player_plugin_abi::{
    AUDIO_PROCESSOR_INTERFACE_ID, BENCHMARK_SINK_INTERFACE_ID, FRAME_PROCESSOR_INTERFACE_ID,
    NATIVE_DECODER_INTERFACE_ID, PIPELINE_EVENT_HOOK_INTERFACE_ID,
    POST_DOWNLOAD_PROCESSOR_INTERFACE_ID, SOURCE_NORMALIZER_PACKET_INTERFACE_ID,
    SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID, VesperInterfaceId,
};
use player_plugin_loader::{
    LoadedNativePlugin, PluginContractDiagnosticKind, PluginInterfaceState, PluginLoadError,
};
use player_plugin_wasm_host::{
    WASM_PLUGIN_WIT_INTERFACE_MAJOR, WASM_PLUGIN_WIT_INTERFACE_MINOR, WasmBenchmarkSinkSession,
    WasmPipelineEventHookSession, WasmPluginHostError, WasmPluginRuntime, WasmPluginRuntimeError,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PLUGIN_INSPECTION_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginInspectionOperation {
    Inspect,
    Check,
}

impl PluginInspectionOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Check => "check",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginInspectionOutcome {
    Passed,
    CompatibilityFailure,
    ConformanceFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginInspectionCheckStatus {
    Passed,
    Failed,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginInspectionFailureKind {
    Compatibility,
    Conformance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginInspectionCheck {
    pub id: String,
    pub status: PluginInspectionCheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<PluginInspectionFailureKind>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginRuntimeInterfaceState {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginRuntimeInterfaceReport {
    pub interface_id: String,
    pub instance_id: String,
    pub major: u16,
    pub minor: u16,
    pub state: PluginRuntimeInterfaceState,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginWorkerOutputSummary {
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginInspectionReport {
    pub schema_version: u32,
    pub operation: PluginInspectionOperation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<PluginArtifactTransport>,
    pub plugin_id: String,
    pub plugin_version: String,
    pub migration_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_plugin_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_plugin_name: Option<String>,
    pub compatible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conformant: Option<bool>,
    pub outcome: PluginInspectionOutcome,
    pub interfaces: Vec<PluginRuntimeInterfaceReport>,
    pub checks: Vec<PluginInspectionCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_output: Option<PluginWorkerOutputSummary>,
}

/// Canonical metadata that plugin tooling can report without opening an
/// artifact or creating a runtime worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogInspectionReport {
    pub schema_version: u32,
    pub plugin_id: String,
    pub plugin_version: String,
    pub descriptor_sha256: String,
    pub migration_version: String,
    pub requires: Vec<PluginRequirement>,
    pub provides: Vec<PluginProvision>,
    pub artifacts: Vec<PluginArtifactDescriptor>,
}

impl PluginInspectionReport {
    fn new(
        descriptor: &PluginDescriptor,
        operation: PluginInspectionOperation,
        transport: Option<PluginArtifactTransport>,
    ) -> Self {
        Self {
            schema_version: PLUGIN_INSPECTION_REPORT_SCHEMA_VERSION,
            operation,
            transport,
            plugin_id: descriptor.plugin.id.clone(),
            plugin_version: descriptor.plugin.version.clone(),
            migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
            runtime_plugin_id: None,
            runtime_plugin_name: None,
            compatible: true,
            conformant: (operation == PluginInspectionOperation::Check).then_some(true),
            outcome: PluginInspectionOutcome::Passed,
            interfaces: Vec::new(),
            checks: Vec::new(),
            worker_output: None,
        }
    }

    #[cfg(any(unix, windows))]
    pub fn with_worker_output(mut self, worker_output: PluginWorkerOutputSummary) -> Self {
        self.worker_output = Some(worker_output);
        self
    }

    pub const fn outcome(&self) -> PluginInspectionOutcome {
        self.outcome
    }

    fn passed(&mut self, id: impl Into<String>, message: impl Into<String>) {
        self.checks.push(PluginInspectionCheck {
            id: id.into(),
            status: PluginInspectionCheckStatus::Passed,
            failure_kind: None,
            message: message.into(),
        });
    }

    fn warning(&mut self, id: impl Into<String>, message: impl Into<String>) {
        self.checks.push(PluginInspectionCheck {
            id: id.into(),
            status: PluginInspectionCheckStatus::Warning,
            failure_kind: None,
            message: message.into(),
        });
    }

    fn compatibility_failure(&mut self, id: impl Into<String>, message: impl Into<String>) {
        self.compatible = false;
        if self.outcome == PluginInspectionOutcome::Passed {
            self.outcome = PluginInspectionOutcome::CompatibilityFailure;
        }
        self.checks.push(PluginInspectionCheck {
            id: id.into(),
            status: PluginInspectionCheckStatus::Failed,
            failure_kind: Some(PluginInspectionFailureKind::Compatibility),
            message: message.into(),
        });
    }

    fn conformance_failure(&mut self, id: impl Into<String>, message: impl Into<String>) {
        self.conformant = Some(false);
        self.outcome = PluginInspectionOutcome::ConformanceFailure;
        self.checks.push(PluginInspectionCheck {
            id: id.into(),
            status: PluginInspectionCheckStatus::Failed,
            failure_kind: Some(PluginInspectionFailureKind::Conformance),
            message: message.into(),
        });
    }
}

pub fn inspect_project_catalog(
    project: &PluginProjectManifest,
) -> Result<PluginCatalogInspectionReport, String> {
    let descriptor = project
        .descriptor()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut artifacts = project
        .artifact_descriptors()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|artifact| {
            artifact
                .canonicalize()
                .map(|canonical| (canonical.json().to_vec(), canonical.descriptor().clone()))
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    artifacts.sort_by(|left, right| left.0.cmp(&right.0));

    Ok(PluginCatalogInspectionReport {
        schema_version: PLUGIN_CATALOG_SCHEMA_VERSION,
        plugin_id: descriptor.descriptor().plugin.id.clone(),
        plugin_version: descriptor.descriptor().plugin.version.clone(),
        descriptor_sha256: descriptor.sha256().to_owned(),
        migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
        requires: descriptor.descriptor().requires.clone(),
        provides: descriptor.descriptor().provides.clone(),
        artifacts: artifacts
            .into_iter()
            .map(|(_, descriptor)| descriptor)
            .collect(),
    })
}

pub fn inspect_manifest(
    descriptor: &PluginDescriptor,
    operation: PluginInspectionOperation,
    transport: Option<PluginArtifactTransport>,
) -> PluginInspectionReport {
    let mut report = PluginInspectionReport::new(descriptor, operation, transport);
    report.passed("manifest.schema", "the plugin manifest is valid");

    let host_sdk = match Version::parse(env!("CARGO_PKG_VERSION")) {
        Ok(version) => version,
        Err(error) => {
            report.conformance_failure(
                "host.version",
                format!("the host SDK version is invalid: {error}"),
            );
            return report;
        }
    };
    match descriptor.evaluate_current_host_compatibility(&host_sdk) {
        Ok(()) => report.passed(
            "host.compatibility",
            format!("the plugin supports host SDK {host_sdk}"),
        ),
        Err(error) => report.compatibility_failure("host.compatibility", error.to_string()),
    }

    if transport == Some(PluginArtifactTransport::Wasm) {
        validate_wasm_capabilities(descriptor, &mut report);
    }
    report
}

pub fn inspect_native_plugin(
    descriptor: &PluginDescriptor,
    artifact: &std::path::Path,
    operation: PluginInspectionOperation,
) -> PluginInspectionReport {
    let mut report = inspect_manifest(descriptor, operation, Some(PluginArtifactTransport::Native));
    if report.outcome() != PluginInspectionOutcome::Passed {
        return report;
    }

    let loaded = match LoadedNativePlugin::load_development(artifact) {
        Ok(loaded) => loaded,
        Err(error) => {
            classify_native_load_error(&mut report, &error);
            return report;
        }
    };
    report.conformant = Some(true);
    report.runtime_plugin_id = Some(loaded.plugin_id().to_owned());
    report.runtime_plugin_name = Some(loaded.plugin_name().to_owned());
    report.passed("runtime.load", "the native plugin root loaded successfully");

    if loaded.plugin_id() == descriptor.plugin.id {
        report.passed(
            "runtime.identity",
            "the runtime plugin identity matches the manifest",
        );
    } else {
        report.conformance_failure(
            "runtime.identity",
            format!(
                "manifest plugin '{}' does not match runtime plugin '{}'",
                descriptor.plugin.id,
                loaded.plugin_id()
            ),
        );
    }

    for diagnostic in loaded.diagnostics() {
        let id = diagnostic.index.map_or_else(
            || "runtime.interface".to_owned(),
            |index| format!("runtime.interface.{index}"),
        );
        match diagnostic.kind {
            PluginContractDiagnosticKind::Compatibility => {
                report.compatibility_failure(id, diagnostic.message.clone())
            }
            PluginContractDiagnosticKind::ContractViolation => {
                report.conformance_failure(id, diagnostic.message.clone())
            }
        }
    }

    report.interfaces = loaded
        .interfaces()
        .iter()
        .map(|interface| PluginRuntimeInterfaceReport {
            interface_id: Uuid::from_bytes(interface.metadata.interface_id).to_string(),
            instance_id: interface.metadata.instance_id.clone(),
            major: interface.metadata.major,
            minor: interface.metadata.minor,
            state: match interface.state {
                PluginInterfaceState::Available => PluginRuntimeInterfaceState::Available,
                PluginInterfaceState::Unavailable => PluginRuntimeInterfaceState::Unavailable,
                PluginInterfaceState::Unknown => PluginRuntimeInterfaceState::Unknown,
            },
        })
        .collect();
    compare_declared_interfaces(descriptor, &mut report);

    if operation == PluginInspectionOperation::Check {
        for capability in &descriptor.capabilities {
            check_native_capability(&loaded, descriptor, capability, &mut report);
        }
    }
    drop(loaded);
    report
}

pub fn inspect_wasm_plugin(
    descriptor: &PluginDescriptor,
    component_bytes: &[u8],
    operation: PluginInspectionOperation,
) -> Result<PluginInspectionReport, WasmPluginRuntimeError> {
    let mut report = inspect_manifest(descriptor, operation, Some(PluginArtifactTransport::Wasm));
    if report.outcome() != PluginInspectionOutcome::Passed {
        return Ok(report);
    }

    let runtime = WasmPluginRuntime::new()?;
    report.conformant = Some(true);
    let mut seen = BTreeSet::new();
    for capability in &descriptor.capabilities {
        let interface_id = match Uuid::parse_str(&capability.interface_id) {
            Ok(interface_id) => interface_id,
            Err(error) => {
                report.conformance_failure(
                    capability_check_id("wasm.export", capability),
                    format!("invalid capability UUID reached the WASM host: {error}"),
                );
                continue;
            }
        };
        let key = (interface_id, capability.instance_id.clone());
        if !seen.insert(key) {
            continue;
        }

        if interface_id == interface_uuid(PIPELINE_EVENT_HOOK_INTERFACE_ID) {
            match WasmPipelineEventHookSession::from_component_bytes(&runtime, component_bytes) {
                Ok(mut session) => {
                    report
                        .interfaces
                        .push(runtime_interface_from_capability(capability));
                    report.passed(
                        capability_check_id("wasm.export", capability),
                        "the event-hook component export instantiated successfully",
                    );
                    if operation == PluginInspectionOperation::Check {
                        match session.on_event(&synthetic_pipeline_event()) {
                            Ok(_) => report.passed(
                                capability_check_id("wasm.call", capability),
                                "the event-hook export accepted a bounded synthetic call",
                            ),
                            Err(error) => classify_wasm_call_error(
                                &mut report,
                                capability_check_id("wasm.call", capability),
                                error,
                            ),
                        }
                    }
                }
                Err(error) => report.conformance_failure(
                    capability_check_id("wasm.export", capability),
                    error.to_string(),
                ),
            }
        } else if interface_id == interface_uuid(BENCHMARK_SINK_INTERFACE_ID) {
            match WasmBenchmarkSinkSession::from_component_bytes(&runtime, component_bytes) {
                Ok(mut session) => {
                    report
                        .interfaces
                        .push(runtime_interface_from_capability(capability));
                    report.passed(
                        capability_check_id("wasm.export", capability),
                        "the benchmark-sink component export instantiated successfully",
                    );
                    if operation == PluginInspectionOperation::Check {
                        let batch = synthetic_benchmark_batch();
                        match session.on_event_batch(&batch) {
                            Ok(_) => match session.flush() {
                                Ok(_) => report.passed(
                                    capability_check_id("wasm.call", capability),
                                    "the benchmark export accepted a batch and flushed",
                                ),
                                Err(error) => classify_wasm_call_error(
                                    &mut report,
                                    capability_check_id("wasm.flush", capability),
                                    error,
                                ),
                            },
                            Err(error) => classify_wasm_call_error(
                                &mut report,
                                capability_check_id("wasm.call", capability),
                                error,
                            ),
                        }
                    }
                }
                Err(error) => report.conformance_failure(
                    capability_check_id("wasm.export", capability),
                    error.to_string(),
                ),
            }
        }
    }
    Ok(report)
}

fn classify_native_load_error(report: &mut PluginInspectionReport, error: &PluginLoadError) {
    match error {
        PluginLoadError::NativeContract(source)
            if source.diagnostic_kind() == PluginContractDiagnosticKind::Compatibility =>
        {
            report.compatibility_failure("runtime.load", error.to_string());
        }
        PluginLoadError::OpenLibrary { .. }
        | PluginLoadError::ResolveEntrySymbol { .. }
        | PluginLoadError::NativeContract(_) => {
            report.conformance_failure("runtime.load", error.to_string());
        }
    }
}

fn compare_declared_interfaces(descriptor: &PluginDescriptor, report: &mut PluginInspectionReport) {
    let mut declared = BTreeMap::new();
    for capability in &descriptor.capabilities {
        match Uuid::parse_str(&capability.interface_id) {
            Ok(interface_id) => {
                declared.insert(
                    (interface_id, capability.instance_id.as_str()),
                    (capability.interface_major, capability.interface_minor),
                );
            }
            Err(error) => report.conformance_failure(
                "runtime.interface-set",
                format!("invalid manifest capability UUID reached inspection: {error}"),
            ),
        }
    }

    let runtime_interfaces = report.interfaces.clone();
    let mut runtime = BTreeMap::new();
    for interface in &runtime_interfaces {
        let interface_id = match Uuid::parse_str(&interface.interface_id) {
            Ok(interface_id) => interface_id,
            Err(error) => {
                report.conformance_failure(
                    "runtime.interface-set",
                    format!("runtime interface UUID cannot be parsed: {error}"),
                );
                continue;
            }
        };
        runtime.insert(
            (interface_id, interface.instance_id.as_str()),
            (interface.major, interface.minor, &interface.state),
        );
    }

    for ((interface_id, instance_id), (major, minor)) in &declared {
        match runtime.get(&(*interface_id, *instance_id)) {
            None => report.conformance_failure(
                "runtime.interface-set",
                format!(
                    "declared capability {interface_id}:{instance_id} is missing from the native root"
                ),
            ),
            Some((runtime_major, runtime_minor, _))
                if (*runtime_major, *runtime_minor) != (*major, *minor) =>
            {
                report.conformance_failure(
                    "runtime.interface-version",
                    format!(
                        "declared capability {interface_id}:{instance_id} uses {major}.{minor}, but the native root reports {runtime_major}.{runtime_minor}"
                    ),
                );
            }
            Some(_) => {}
        }
    }
    for (interface_id, instance_id) in runtime.keys() {
        if !declared.contains_key(&(*interface_id, *instance_id)) {
            report.conformance_failure(
                "runtime.interface-set",
                format!(
                    "native root capability {interface_id}:{instance_id} is not declared by the manifest"
                ),
            );
        }
    }
    if report.checks.iter().all(|check| {
        check.id != "runtime.interface-set" || check.status != PluginInspectionCheckStatus::Failed
    }) {
        report.passed(
            "runtime.interface-set",
            "the native root interface set matches the manifest",
        );
    }
}

fn check_native_capability(
    loaded: &LoadedNativePlugin,
    descriptor: &PluginDescriptor,
    capability: &PluginCapabilityDescriptor,
    report: &mut PluginInspectionReport,
) {
    let interface_id = match Uuid::parse_str(&capability.interface_id) {
        Ok(interface_id) => interface_id,
        Err(error) => {
            report.conformance_failure(
                capability_check_id("native.call", capability),
                format!("invalid capability UUID reached conformance: {error}"),
            );
            return;
        }
    };
    let reference = match PluginReference::new(
        descriptor.plugin.id.clone(),
        Some(capability.instance_id.clone()),
        PluginTransport::Native,
    ) {
        Ok(reference) => reference,
        Err(error) => {
            report.conformance_failure(
                capability_check_id("native.call", capability),
                error.to_string(),
            );
            return;
        }
    };
    let check_id = capability_check_id("native.call", capability);

    if interface_id == interface_uuid(PIPELINE_EVENT_HOOK_INTERFACE_ID) {
        match loaded.resolve_pipeline_event_hook(&reference) {
            Ok(hook) => match hook.on_event(&synthetic_pipeline_event()) {
                Ok(_) => report.passed(check_id, "event hook completed a bounded synthetic call"),
                Err(
                    PipelineEventHookError::InvalidInput(message)
                    | PipelineEventHookError::Rejected(message)
                    | PipelineEventHookError::Failed(message),
                ) => report.warning(
                    check_id,
                    format!("event hook returned a valid author failure: {message}"),
                ),
                Err(error) => report.conformance_failure(check_id, error.to_string()),
            },
            Err(error) => report.conformance_failure(check_id, error.to_string()),
        }
    } else if interface_id == interface_uuid(BENCHMARK_SINK_INTERFACE_ID) {
        match loaded.resolve_benchmark_sink(&reference) {
            Ok(sink) => {
                let batch = synthetic_benchmark_batch();
                match sink.on_event_batch(&batch) {
                    Ok(_) => match sink.flush() {
                        Ok(_) => {
                            report.passed(check_id, "benchmark sink accepted a batch and flushed")
                        }
                        Err(BenchmarkSinkError::SinkFailed(message)) => report.warning(
                            check_id,
                            format!("benchmark sink returned a valid author failure: {message}"),
                        ),
                        Err(error) => report.conformance_failure(check_id, error.to_string()),
                    },
                    Err(BenchmarkSinkError::SinkFailed(message)) => report.warning(
                        check_id,
                        format!("benchmark sink returned a valid author failure: {message}"),
                    ),
                    Err(error) => report.conformance_failure(check_id, error.to_string()),
                }
            }
            Err(error) => report.conformance_failure(check_id, error.to_string()),
        }
    } else if interface_id == interface_uuid(POST_DOWNLOAD_PROCESSOR_INTERFACE_ID) {
        check_native_post_download(loaded, &reference, check_id, report);
    } else if interface_id == interface_uuid(NATIVE_DECODER_INTERFACE_ID) {
        match loaded.resolve_native_decoder(&reference) {
            Ok(factory) => {
                let _ = factory.name();
                let _ = factory.capabilities();
                let _ = factory.native_requirements();
                report.passed(check_id, "decoder factory metadata is readable");
            }
            Err(error) => report.conformance_failure(check_id, error.to_string()),
        }
    } else if interface_id == interface_uuid(FRAME_PROCESSOR_INTERFACE_ID) {
        match loaded.resolve_frame_processor(&reference) {
            Ok(factory) => {
                let _ = factory.name();
                let _ = factory.capabilities();
                report.passed(check_id, "frame-processor factory metadata is readable");
            }
            Err(error) => report.conformance_failure(check_id, error.to_string()),
        }
    } else if interface_id == interface_uuid(AUDIO_PROCESSOR_INTERFACE_ID) {
        check_native_audio_processor(loaded, &reference, check_id, report);
    } else if interface_id == interface_uuid(SOURCE_NORMALIZER_PACKET_INTERFACE_ID) {
        match loaded.resolve_source_packet(&reference) {
            Ok(factory) => {
                let _ = factory.name();
                let _ = factory.packet_capabilities();
                report.passed(check_id, "packet normalizer factory metadata is readable");
            }
            Err(error) => report.conformance_failure(check_id, error.to_string()),
        }
    } else if interface_id == interface_uuid(SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID) {
        match loaded.resolve_source_resource(&reference) {
            Ok(factory) => {
                let _ = factory.name();
                let _ = factory.resource_capabilities();
                report.passed(check_id, "resource normalizer factory metadata is readable");
            }
            Err(error) => report.conformance_failure(check_id, error.to_string()),
        }
    } else {
        report.compatibility_failure(
            check_id,
            format!("host does not implement conformance checks for interface {interface_id}"),
        );
    }
}

fn check_native_audio_processor(
    loaded: &LoadedNativePlugin,
    reference: &PluginReference,
    check_id: String,
    report: &mut PluginInspectionReport,
) {
    let factory = match loaded.resolve_audio_processor(reference) {
        Ok(factory) => factory,
        Err(error) => {
            report.conformance_failure(check_id, error.to_string());
            return;
        }
    };
    let capabilities = factory.capabilities();
    let Some(format) = audio_check_format(&capabilities) else {
        report.conformance_failure(check_id, "audio processor does not accept F32 or S16 PCM");
        return;
    };
    let policy = audio_check_policy(&capabilities);
    if !capabilities.supports_playback_policy(policy) {
        report.conformance_failure(
            check_id,
            "audio processor does not expose a usable playback policy",
        );
        return;
    }

    let mut metadata = DecoderPcmFrameMetadata::audio(
        "vesper-check-pcm",
        format.clone(),
        48_000,
        1,
        DecoderPcmSampleLayout::Interleaved,
        64,
    );
    metadata.pts_us = Some(1_000);
    metadata.duration_us = Some(1_333);
    metadata.discontinuity = true;
    let bytes_per_sample = if format == DecoderFrameFormat::F32 {
        size_of::<f32>()
    } else {
        size_of::<i16>()
    };
    let frame = DecoderPcmFrame {
        metadata: metadata.clone(),
        data: vec![0; 64 * bytes_per_sample],
    };
    let config = AudioProcessorSessionConfig {
        processor_index: 0,
        input_metadata: metadata,
        playback_policy: policy,
        max_in_flight_frames: Some(1),
    };

    match exercise_native_audio_processor(factory.as_ref(), &config, policy, frame) {
        Ok(()) => report.passed(
            check_id,
            "audio processor completed bounded PCM configure/process/flush/close",
        ),
        Err(AudioProcessorError::AbiViolation(message))
        | Err(AudioProcessorError::PayloadCodec(message)) => {
            report.conformance_failure(check_id, message)
        }
        Err(error) => report.warning(
            check_id,
            format!("audio processor returned a valid author failure: {error}"),
        ),
    }
}

fn audio_check_format(capabilities: &AudioProcessorCapabilities) -> Option<DecoderFrameFormat> {
    [DecoderFrameFormat::F32, DecoderFrameFormat::S16]
        .into_iter()
        .find(|format| capabilities.supports_input_format(format))
}

fn audio_check_policy(capabilities: &AudioProcessorCapabilities) -> AudioPlaybackPolicy {
    let pitch_mode = capabilities
        .pitch_modes
        .first()
        .copied()
        .unwrap_or(AudioPitchMode::FollowRate);
    let normal = AudioPlaybackPolicy {
        playback_rate: 1.0,
        pitch_mode,
    };
    if capabilities.supports_playback_policy(normal) {
        return normal;
    }
    AudioPlaybackPolicy {
        playback_rate: capabilities
            .playback_rate_min
            .or(capabilities.playback_rate_max)
            .unwrap_or(1.0),
        pitch_mode,
    }
}

fn exercise_native_audio_processor(
    factory: &dyn player_plugin::AudioProcessorPluginFactory,
    config: &AudioProcessorSessionConfig,
    policy: AudioPlaybackPolicy,
    frame: DecoderPcmFrame,
) -> Result<(), AudioProcessorError> {
    let mut session = factory.open_session(config)?;
    let operation_result = (|| {
        session.configure(policy)?;
        session.process(frame)?;
        session.flush()
    })();
    let close_result = session.close();
    operation_result.and(close_result)
}

fn check_native_post_download(
    loaded: &LoadedNativePlugin,
    reference: &PluginReference,
    check_id: String,
    report: &mut PluginInspectionReport,
) {
    let processor = match loaded.resolve_post_download(reference) {
        Ok(processor) => processor,
        Err(error) => {
            report.conformance_failure(check_id, error.to_string());
            return;
        }
    };
    let capabilities = processor.capabilities();
    if !capabilities
        .supported_input_formats
        .contains(&ContentFormatKind::SingleFile)
    {
        report.passed(check_id, "post-download capability metadata is readable");
        return;
    }
    let directory = match tempfile::tempdir() {
        Ok(directory) => directory,
        Err(error) => {
            report
                .conformance_failure(check_id, format!("failed to create check fixture: {error}"));
            return;
        }
    };
    let input_path = directory.path().join("input.bin");
    if let Err(error) = fs::write(&input_path, []) {
        report.conformance_failure(check_id, format!("failed to write check fixture: {error}"));
        return;
    }
    let input = CompletedDownloadInfo {
        asset_id: "vesper-check-asset".to_owned(),
        task_id: Some("vesper-check-task".to_owned()),
        content_format: CompletedContentFormat::SingleFile { path: input_path },
        metadata: DownloadMetadata::default(),
        streams: Vec::new(),
        assembly_mode: AssemblyMode::Single,
    };
    let output_path = directory.path().join("output.bin");
    match processor.process(&input, &output_path, &NoopProgress) {
        Ok(_) => report.passed(
            check_id,
            "post-download processor completed a bounded fixture",
        ),
        Err(
            ProcessorError::UnsupportedFormat(_)
            | ProcessorError::UnsupportedDynamicStream { .. }
            | ProcessorError::MuxFailed(_)
            | ProcessorError::OutputPath(_)
            | ProcessorError::Cancelled,
        ) => report.warning(
            check_id,
            "post-download processor returned a valid domain failure for the fixture",
        ),
        Err(error) => report.conformance_failure(check_id, error.to_string()),
    }
}

fn validate_wasm_capabilities(descriptor: &PluginDescriptor, report: &mut PluginInspectionReport) {
    let event_hook = interface_uuid(PIPELINE_EVENT_HOOK_INTERFACE_ID);
    let benchmark_sink = interface_uuid(BENCHMARK_SINK_INTERFACE_ID);
    let mut per_interface = BTreeMap::<Uuid, usize>::new();
    for capability in &descriptor.capabilities {
        let interface_id = match Uuid::parse_str(&capability.interface_id) {
            Ok(interface_id) => interface_id,
            Err(error) => {
                report.conformance_failure(
                    "transport.capability",
                    format!("invalid capability UUID reached transport validation: {error}"),
                );
                continue;
            }
        };
        let supported = interface_id == event_hook || interface_id == benchmark_sink;
        if !supported {
            report.compatibility_failure(
                "transport.capability",
                format!("WASM transport does not support interface {interface_id}"),
            );
        } else if capability.interface_major != WASM_PLUGIN_WIT_INTERFACE_MAJOR
            || capability.interface_minor != WASM_PLUGIN_WIT_INTERFACE_MINOR
        {
            report.compatibility_failure(
                "transport.capability",
                format!(
                    "WASM transport uses fixed WIT interface version {WASM_PLUGIN_WIT_INTERFACE_MAJOR}.{WASM_PLUGIN_WIT_INTERFACE_MINOR}, but interface {interface_id} declares {}.{}",
                    capability.interface_major, capability.interface_minor
                ),
            );
        }
        let count = per_interface.entry(interface_id).or_default();
        *count += 1;
        if *count > 1 {
            report.compatibility_failure(
                "transport.capability",
                format!(
                    "WASM transport supports one component export for interface {interface_id}, but the manifest declares multiple instances"
                ),
            );
        }
    }
    if report.compatible {
        report.passed(
            "transport.capability",
            "all declared capabilities are supported by the WASM transport",
        );
    }
}

fn classify_wasm_call_error(
    report: &mut PluginInspectionReport,
    check_id: String,
    error: WasmPluginHostError,
) {
    match error {
        WasmPluginHostError::InvalidInput(message)
        | WasmPluginHostError::Rejected(message)
        | WasmPluginHostError::PluginFailed(message) => report.warning(
            check_id,
            format!("WASM plugin returned a valid author failure: {message}"),
        ),
        other => report.conformance_failure(check_id, other.to_string()),
    }
}

fn runtime_interface_from_capability(
    capability: &PluginCapabilityDescriptor,
) -> PluginRuntimeInterfaceReport {
    PluginRuntimeInterfaceReport {
        interface_id: capability.interface_id.clone(),
        instance_id: capability.instance_id.clone(),
        major: capability.interface_major,
        minor: capability.interface_minor,
        state: PluginRuntimeInterfaceState::Available,
    }
}

fn capability_check_id(prefix: &str, capability: &PluginCapabilityDescriptor) -> String {
    format!(
        "{prefix}.{}.{}",
        capability.interface_id, capability.instance_id
    )
}

fn interface_uuid(interface_id: VesperInterfaceId) -> Uuid {
    Uuid::from_bytes(interface_id.0)
}

fn synthetic_pipeline_event() -> PipelineEvent {
    PipelineEvent {
        run_id: "vesper-check-run".to_owned(),
        session_id: "vesper-check-session".to_owned(),
        platform: std::env::consts::OS.to_owned(),
        protocol: None,
        event_name: "plugin.conformance".to_owned(),
        timestamp_ns: 0,
        thread: None,
        resource_identity: Some("vesper-check:opaque-resource".to_owned()),
        attributes: BTreeMap::new(),
        diagnostic: None,
    }
}

fn synthetic_benchmark_batch() -> BenchmarkEventBatch {
    BenchmarkEventBatch {
        events: vec![BenchmarkEvent {
            run_id: "vesper-check-run".to_owned(),
            session_id: "vesper-check-session".to_owned(),
            platform: std::env::consts::OS.to_owned(),
            source_protocol: None,
            event_name: "plugin.conformance".to_owned(),
            timestamp_ns: 0,
            elapsed_ns: 0,
            thread: None,
            attributes: BTreeMap::new(),
        }],
    }
}

struct NoopProgress;

impl ProcessorProgress for NoopProgress {
    fn on_progress(&self, _ratio: f32) {}
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use player_cli::{PluginCompatibilityDescriptor, PluginIdentityDescriptor, PluginStability};

    use super::*;

    fn descriptor(interface_id: String) -> PluginDescriptor {
        PluginDescriptor {
            schema_version: 1,
            plugin: PluginIdentityDescriptor {
                id: "dev.vesper.fixture".to_owned(),
                name: "Fixture".to_owned(),
                version: "1.0.0".to_owned(),
                description: "Fixture".to_owned(),
                license: "Apache-2.0".to_owned(),
                publisher: "dev.vesper.publisher".to_owned(),
            },
            compatibility: PluginCompatibilityDescriptor {
                host_sdk: ">=0.5.0, <0.6.0".to_owned(),
                abi_major: 1,
                abi_minor_min: 0,
                abi_minor_max: 0,
            },
            capabilities: vec![PluginCapabilityDescriptor {
                interface_id,
                instance_id: "dev.vesper.fixture.default".to_owned(),
                interface_major: 1,
                interface_minor: 0,
                stability: PluginStability::Stable,
            }],
            requires: Vec::new(),
            provides: Vec::new(),
            redistribution: Vec::new(),
        }
    }

    #[test]
    fn manifest_inspection_keeps_unknown_capability_schema_valid() {
        let descriptor = descriptor("11111111-2222-4333-8444-555555555555".to_owned());
        let report = inspect_manifest(&descriptor, PluginInspectionOperation::Inspect, None);
        assert_eq!(report.outcome, PluginInspectionOutcome::Passed);
        assert!(report.compatible);
    }

    #[test]
    fn wasm_transport_rejects_native_only_capability_as_compatibility() {
        let descriptor = descriptor(interface_uuid(NATIVE_DECODER_INTERFACE_ID).to_string());
        let report = inspect_manifest(
            &descriptor,
            PluginInspectionOperation::Inspect,
            Some(PluginArtifactTransport::Wasm),
        );
        assert_eq!(
            report.outcome,
            PluginInspectionOutcome::CompatibilityFailure
        );
        assert!(!report.compatible);
    }

    #[test]
    fn wasm_transport_rejects_manifest_interface_version_outside_fixed_wit_world() {
        for (major, minor) in [(2, 0), (1, 1)] {
            let mut descriptor =
                descriptor(interface_uuid(PIPELINE_EVENT_HOOK_INTERFACE_ID).to_string());
            descriptor.capabilities[0].interface_major = major;
            descriptor.capabilities[0].interface_minor = minor;

            let report = inspect_manifest(
                &descriptor,
                PluginInspectionOperation::Inspect,
                Some(PluginArtifactTransport::Wasm),
            );

            assert_eq!(
                report.outcome,
                PluginInspectionOutcome::CompatibilityFailure
            );
            assert!(!report.compatible);
            assert!(report.checks.iter().any(|check| {
                check.id == "transport.capability"
                    && check.message.contains("WIT interface version 1.0")
            }));
        }
    }

    struct TestAudioFactory {
        close_calls: Arc<AtomicUsize>,
        fail_process: bool,
    }

    impl player_plugin::AudioProcessorPluginFactory for TestAudioFactory {
        fn name(&self) -> &str {
            "test-audio-processor"
        }

        fn capabilities(&self) -> AudioProcessorCapabilities {
            AudioProcessorCapabilities {
                accepted_formats: vec![DecoderFrameFormat::F32],
                output_format: Some(DecoderFrameFormat::F32),
                supports_flush: true,
                max_in_flight_frames: Some(1),
                playback_rate_min: Some(1.0),
                playback_rate_max: Some(1.0),
                pitch_modes: vec![AudioPitchMode::FollowRate],
            }
        }

        fn open_session(
            &self,
            _config: &AudioProcessorSessionConfig,
        ) -> Result<Box<dyn player_plugin::AudioProcessorSession>, AudioProcessorError> {
            Ok(Box::new(TestAudioSession {
                close_calls: Arc::clone(&self.close_calls),
                fail_process: self.fail_process,
            }))
        }
    }

    struct TestAudioSession {
        close_calls: Arc<AtomicUsize>,
        fail_process: bool,
    }

    impl player_plugin::AudioProcessorSession for TestAudioSession {
        fn name(&self) -> &str {
            "test-audio-session"
        }

        fn capabilities(&self) -> AudioProcessorCapabilities {
            AudioProcessorCapabilities::default()
        }

        fn process(
            &mut self,
            frame: DecoderPcmFrame,
        ) -> Result<DecoderPcmFrame, AudioProcessorError> {
            if self.fail_process {
                return Err(AudioProcessorError::Processor(
                    "expected process failure".to_owned(),
                ));
            }
            Ok(frame)
        }

        fn flush(&mut self) -> Result<(), AudioProcessorError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), AudioProcessorError> {
            self.close_calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    fn test_audio_invocation() -> (
        AudioProcessorSessionConfig,
        AudioPlaybackPolicy,
        DecoderPcmFrame,
    ) {
        let metadata = DecoderPcmFrameMetadata::audio(
            "test-audio-pcm",
            DecoderFrameFormat::F32,
            48_000,
            1,
            DecoderPcmSampleLayout::Interleaved,
            64,
        );
        let policy = AudioPlaybackPolicy {
            playback_rate: 1.0,
            pitch_mode: AudioPitchMode::FollowRate,
        };
        (
            AudioProcessorSessionConfig {
                processor_index: 0,
                input_metadata: metadata.clone(),
                playback_policy: policy,
                max_in_flight_frames: Some(1),
            },
            policy,
            DecoderPcmFrame {
                metadata,
                data: vec![0; 64 * size_of::<f32>()],
            },
        )
    }

    #[test]
    fn audio_conformance_closes_successful_and_failed_sessions() {
        for fail_process in [false, true] {
            let close_calls = Arc::new(AtomicUsize::new(0));
            let factory = TestAudioFactory {
                close_calls: Arc::clone(&close_calls),
                fail_process,
            };
            let (config, policy, frame) = test_audio_invocation();

            let result = exercise_native_audio_processor(&factory, &config, policy, frame);

            assert_eq!(result.is_err(), fail_process);
            assert_eq!(close_calls.load(Ordering::Relaxed), 1);
        }
    }
}
