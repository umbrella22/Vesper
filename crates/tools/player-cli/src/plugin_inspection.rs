use std::collections::BTreeMap;

use player_cli::{
    PLUGIN_CATALOG_MIGRATION_VERSION, PLUGIN_CATALOG_SCHEMA_VERSION, PluginArtifactDescriptor,
    PluginArtifactTransport, PluginDescriptor, PluginProjectManifest, PluginProvision,
    PluginRequirement,
};
use player_plugin_abi as abi;
use player_plugin_loader::native::{DEFAULT_AUDIO_CALL_BUDGET, LoadError, NativePlugin};
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
        report.compatibility_failure("plugin.transport", "WASM plugins have been retired");
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
    inspect_loaded_native_plugin(descriptor, artifact, &mut report);
    report
}

#[allow(
    unsafe_code,
    reason = "the dedicated plugin worker is the native module boundary"
)]
fn inspect_loaded_native_plugin(
    descriptor: &PluginDescriptor,
    artifact: &std::path::Path,
    report: &mut PluginInspectionReport,
) {
    // SAFETY: the operator explicitly selected the native library. This runs
    // inside the bounded inspection worker, with no media or player session.
    let plugin = match unsafe { NativePlugin::load(artifact) } {
        Ok(plugin) => plugin,
        Err(error @ (LoadError::Entry(_) | LoadError::UnsupportedHost)) => {
            report.compatibility_failure("runtime.load", error.to_string());
            return;
        }
        Err(error) => {
            report.conformance_failure("runtime.load", error.to_string());
            return;
        }
    };
    report.conformant = Some(true);
    report.runtime_plugin_id = Some(plugin.id().to_owned());
    report.runtime_plugin_name = Some(plugin.name().to_owned());
    report.passed(
        "runtime.load",
        "the native plugin ABI native root loaded successfully",
    );
    if plugin.id() != descriptor.plugin.id {
        report.conformance_failure(
            "runtime.identity",
            format!(
                "manifest plugin '{}' does not match runtime plugin '{}'",
                descriptor.plugin.id,
                plugin.id()
            ),
        );
        return;
    }
    report.passed(
        "runtime.identity",
        "the runtime plugin identity matches the manifest",
    );

    let mut declared = BTreeMap::new();
    for capability in &descriptor.capabilities {
        let Ok(uuid) = Uuid::parse_str(&capability.interface_id) else {
            report.conformance_failure("runtime.interface-set", "invalid manifest interface UUID");
            continue;
        };
        if declared.insert(uuid, capability).is_some() {
            report.conformance_failure(
                "runtime.interface-set",
                format!("native plugin ABI exposes one table per interface UUID, but {uuid} is declared more than once"),
            );
        }
    }
    for &id in plugin.interface_ids() {
        let uuid = Uuid::from_bytes(id.bytes);
        let declared_interface = declared.remove(&uuid);
        if declared_interface.is_none() {
            report.conformance_failure(
                "runtime.interface-set",
                format!("native interface {uuid} is missing from the manifest"),
            );
        }
        let state = match plugin.inspect_interface(id) {
            Ok(true) => PluginRuntimeInterfaceState::Available,
            Ok(false) => PluginRuntimeInterfaceState::Unknown,
            Err(error) => {
                report.conformance_failure(format!("runtime.interface.{uuid}"), error.to_string());
                PluginRuntimeInterfaceState::Unavailable
            }
        };
        if report.operation == PluginInspectionOperation::Check
            && state == PluginRuntimeInterfaceState::Available
            && id == abi::VESPER_AUDIO_PROCESSOR_ID
        {
            let check_id = format!("native.call.{uuid}");
            match exercise_native_fixed_block(&plugin) {
                Ok(true) => report.passed(
                    check_id,
                    "audio processor completed a split-buffer PCM block and flushed",
                ),
                Ok(false) => report.warning(
                    check_id,
                    "buffered-only audio processor requires a playback fixture",
                ),
                Err(error) => report.conformance_failure(check_id, error.to_string()),
            }
        }
        report.interfaces.push(PluginRuntimeInterfaceReport {
            interface_id: uuid.to_string(),
            instance_id: declared_interface.map_or_else(String::new, |cap| cap.instance_id.clone()),
            major: abi::VESPER_PLUGIN_ABI_MAJOR,
            minor: plugin.abi_minor(),
            state,
        });
    }
    for uuid in declared.keys() {
        report.conformance_failure(
            "runtime.interface-set",
            format!("declared interface {uuid} is missing from the native root"),
        );
    }
}

fn exercise_native_fixed_block(plugin: &NativePlugin) -> player_plugin::author::PluginResult<bool> {
    use player_plugin::author::{AudioBlock, AudioBufferMut, AudioBufferView, AudioTime};
    use std::num::NonZeroU16;

    let factory = plugin.audio_processor(DEFAULT_AUDIO_CALL_BUDGET)?;
    if !factory.capabilities().fixed_block {
        return Ok(false);
    }
    let channels = NonZeroU16::new(2).ok_or(player_plugin::author::PluginError::Contract(
        "invalid probe channel count",
    ))?;
    let format = abi::VesperAudioFormat {
        struct_size: std::mem::size_of::<abi::VesperAudioFormat>() as u32,
        sample_format: abi::VESPER_SAMPLE_F32_INTERLEAVED,
        format_id: 1,
        sample_rate: 48_000,
        channels: channels.get(),
        max_block_frames: 480,
        ..Default::default()
    };
    const PROBE_EPOCH: u64 = 1;
    let mut session = factory.open(format, b"{}")?.processor;
    session.set_playback_policy(1.0, abi::VESPER_PITCH_PRESERVE, PROBE_EPOCH)?;
    let input = [0.25; 960];
    let mut output = [0.0; 960];
    session.process_block(AudioBlock::Split {
        input: AudioBufferView::interleaved(
            channels,
            &input,
            AudioTime {
                pts_us: 0,
                rate_epoch: PROBE_EPOCH,
            },
        )?,
        output: AudioBufferMut::interleaved(channels, &mut output)?,
    })?;
    session.flush()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use player_cli::{
        PluginCapabilityDescriptor, PluginCompatibilityDescriptor, PluginIdentityDescriptor,
        PluginStability,
    };

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
                host_sdk: format!("={}", env!("CARGO_PKG_VERSION")),
                abi_major: abi::VESPER_PLUGIN_ABI_MAJOR,
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
}
