use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use player_cli::{PluginArtifactTransport, PluginProjectManifest};
use player_plugin_abi::{
    BENCHMARK_SINK_INTERFACE_ID, FRAME_PROCESSOR_INTERFACE_ID, NATIVE_DECODER_INTERFACE_ID,
    PIPELINE_EVENT_HOOK_INTERFACE_ID, POST_DOWNLOAD_PROCESSOR_INTERFACE_ID,
    SOURCE_NORMALIZER_PACKET_INTERFACE_ID, SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID,
    VesperInterfaceId,
};
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

use crate::plugin_scaffold_assets::{
    APACHE_2_LICENSE, CANONICAL_WIT, NATIVE_CARGO_TEMPLATE, NATIVE_README_TEMPLATE,
    NATIVE_SOURCE_TEMPLATE, WASM_CARGO_TEMPLATE, WASM_README_TEMPLATE, WASM_SOURCE_TEMPLATE,
};

const VESPER_SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginScaffoldCapability {
    PostDownload,
    EventHook,
    Benchmark,
    Decoder,
    FrameProcessor,
    SourceNormalizerPacket,
    SourceNormalizerResource,
}

impl PluginScaffoldCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PostDownload => "post-download",
            Self::EventHook => "event-hook",
            Self::Benchmark => "benchmark",
            Self::Decoder => "decoder",
            Self::FrameProcessor => "frame-processor",
            Self::SourceNormalizerPacket => "source-normalizer-packet",
            Self::SourceNormalizerResource => "source-normalizer-resource",
        }
    }

    const fn interface_id(self) -> VesperInterfaceId {
        match self {
            Self::PostDownload => POST_DOWNLOAD_PROCESSOR_INTERFACE_ID,
            Self::EventHook => PIPELINE_EVENT_HOOK_INTERFACE_ID,
            Self::Benchmark => BENCHMARK_SINK_INTERFACE_ID,
            Self::Decoder => NATIVE_DECODER_INTERFACE_ID,
            Self::FrameProcessor => FRAME_PROCESSOR_INTERFACE_ID,
            Self::SourceNormalizerPacket => SOURCE_NORMALIZER_PACKET_INTERFACE_ID,
            Self::SourceNormalizerResource => SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID,
        }
    }

    const fn instance_suffix(self) -> &'static str {
        match self {
            Self::PostDownload => "post-download",
            Self::EventHook => "event-hook",
            Self::Benchmark => "benchmark",
            Self::Decoder => "decoder",
            Self::FrameProcessor => "frame-processor",
            Self::SourceNormalizerPacket => "source-normalizer-packet",
            Self::SourceNormalizerResource => "source-normalizer-resource",
        }
    }

    const fn stability(self) -> &'static str {
        match self {
            Self::PostDownload | Self::EventHook | Self::Benchmark => "stable",
            Self::Decoder
            | Self::FrameProcessor
            | Self::SourceNormalizerPacket
            | Self::SourceNormalizerResource => "experimental",
        }
    }

    const fn sort_key(self) -> u8 {
        match self {
            Self::PostDownload => 0,
            Self::EventHook => 1,
            Self::Benchmark => 2,
            Self::Decoder => 3,
            Self::FrameProcessor => 4,
            Self::SourceNormalizerPacket => 5,
            Self::SourceNormalizerResource => 6,
        }
    }

    const fn supported_by_wasm(self) -> bool {
        matches!(self, Self::EventHook | Self::Benchmark)
    }
}

#[derive(Debug)]
pub struct PluginScaffoldRequest {
    pub directory: PathBuf,
    pub plugin_id: String,
    pub plugin_name: Option<String>,
    pub publisher: String,
    pub license: String,
    pub transport: PluginArtifactTransport,
    pub capabilities: Vec<PluginScaffoldCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginScaffoldReport {
    pub directory: PathBuf,
    pub plugin_id: String,
    pub crate_name: String,
    pub transport: PluginArtifactTransport,
    pub capabilities: Vec<PluginScaffoldCapability>,
    pub manifest: PathBuf,
}

#[derive(Debug, Error)]
pub enum PluginScaffoldError {
    #[error("invalid plugin scaffold: {0}")]
    Invalid(String),
    #[error("plugin scaffold is incompatible with the selected transport: {0}")]
    Compatibility(String),
    #[error("plugin scaffold template is invalid: {0}")]
    Template(String),
    #[error("failed to {action} '{}': {source}", path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl PluginScaffoldError {
    pub const fn is_compatibility(&self) -> bool {
        matches!(self, Self::Compatibility(_))
    }
}

pub fn create_plugin_scaffold(
    request: PluginScaffoldRequest,
) -> Result<PluginScaffoldReport, PluginScaffoldError> {
    if request.directory.exists() {
        return Err(PluginScaffoldError::Invalid(format!(
            "destination '{}' already exists; refusing to replace it",
            request.directory.display()
        )));
    }
    let parent = request
        .directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(PluginScaffoldError::Invalid(format!(
            "destination parent '{}' is not a directory",
            parent.display()
        )));
    }

    let mut capabilities = request.capabilities;
    if capabilities.is_empty() {
        return Err(PluginScaffoldError::Invalid(
            "at least one --capability is required".to_owned(),
        ));
    }
    let mut seen = HashSet::with_capacity(capabilities.len());
    if let Some(duplicate) = capabilities
        .iter()
        .copied()
        .find(|capability| !seen.insert(*capability))
    {
        return Err(PluginScaffoldError::Invalid(format!(
            "capability '{}' is declared more than once",
            duplicate.as_str()
        )));
    }
    capabilities.sort_by_key(|capability| capability.sort_key());
    if request.transport == PluginArtifactTransport::Wasm
        && let Some(capability) = capabilities
            .iter()
            .copied()
            .find(|capability| !capability.supported_by_wasm())
    {
        return Err(PluginScaffoldError::Compatibility(format!(
            "WASM plugins cannot implement capability '{}'",
            capability.as_str()
        )));
    }

    let crate_component = request
        .plugin_id
        .rsplit('.')
        .next()
        .filter(|component| !component.is_empty())
        .ok_or_else(|| PluginScaffoldError::Invalid("plugin id is empty".to_owned()))?;
    let crate_name = format!("vesper-plugin-{crate_component}");
    let library_name = crate_name.replace('-', "_");
    let plugin_name = request
        .plugin_name
        .unwrap_or_else(|| display_name(crate_component));
    let (target, architecture, artifact_file) = artifact_target(request.transport, &library_name)?;
    let artifact_source = format!("dist/{artifact_file}");
    let artifact_archive_path = format!("artifacts/{target}/{artifact_file}");

    let manifest = render_manifest(
        &request.plugin_id,
        &plugin_name,
        &request.publisher,
        &request.license,
        request.transport,
        &capabilities,
        &target,
        &architecture,
        &artifact_source,
        &artifact_archive_path,
    );
    PluginProjectManifest::from_toml(&manifest).map_err(|error| {
        PluginScaffoldError::Invalid(format!("generated manifest is invalid: {error}"))
    })?;

    let cargo = render(
        match request.transport {
            PluginArtifactTransport::Native => NATIVE_CARGO_TEMPLATE,
            PluginArtifactTransport::Wasm => WASM_CARGO_TEMPLATE,
        },
        &[
            ("{{CRATE_NAME}}", &toml_string(&crate_name)),
            ("{{LICENSE}}", &toml_string(&request.license)),
            ("{{VESPER_SDK_VERSION}}", VESPER_SDK_VERSION),
        ],
    )?;
    let source = match request.transport {
        PluginArtifactTransport::Native => {
            render_native_source(&request.plugin_id, &plugin_name, &capabilities)?
        }
        PluginArtifactTransport::Wasm => render_wasm_source(&capabilities)?,
    };
    let readme = render(
        match request.transport {
            PluginArtifactTransport::Native => NATIVE_README_TEMPLATE,
            PluginArtifactTransport::Wasm => WASM_README_TEMPLATE,
        },
        &[
            ("{{PLUGIN_NAME}}", &plugin_name),
            ("{{ARTIFACT_SOURCE}}", &artifact_source),
        ],
    )?;

    let staging = tempfile::Builder::new()
        .prefix(".vesper-plugin-new-")
        .tempdir_in(parent)
        .map_err(|source| PluginScaffoldError::Io {
            action: "create scaffold staging directory",
            path: parent.to_owned(),
            source,
        })?;
    write_scaffold_file(staging.path(), "Cargo.toml", cargo.as_bytes())?;
    write_scaffold_file(staging.path(), "src/lib.rs", source.as_bytes())?;
    write_scaffold_file(staging.path(), "vesper-plugin.toml", manifest.as_bytes())?;
    write_scaffold_file(staging.path(), "README.md", readme.as_bytes())?;
    write_scaffold_file(
        staging.path(),
        "LICENSE",
        license_contents(&request.license).as_bytes(),
    )?;
    write_scaffold_file(
        staging.path(),
        "NOTICE",
        format!("{plugin_name}\nCopyright (c) plugin contributors\n").as_bytes(),
    )?;
    if request.transport == PluginArtifactTransport::Wasm {
        write_scaffold_file(staging.path(), "wit/plugin.wit", CANONICAL_WIT)?;
    }
    fs::rename(staging.path(), &request.directory).map_err(|source| PluginScaffoldError::Io {
        action: "publish scaffold directory",
        path: request.directory.clone(),
        source,
    })?;

    Ok(PluginScaffoldReport {
        manifest: request.directory.join("vesper-plugin.toml"),
        directory: request.directory,
        plugin_id: request.plugin_id,
        crate_name,
        transport: request.transport,
        capabilities,
    })
}

#[allow(clippy::too_many_arguments)]
fn render_manifest(
    plugin_id: &str,
    plugin_name: &str,
    publisher: &str,
    license: &str,
    transport: PluginArtifactTransport,
    capabilities: &[PluginScaffoldCapability],
    target: &str,
    architecture: &str,
    artifact_source: &str,
    artifact_archive_path: &str,
) -> String {
    let host_sdk = plugin_host_sdk_requirement();
    let mut manifest = format!(
        "schema_version = 1\n\n[plugin]\nid = {}\nname = {}\nversion = \"0.1.0\"\ndescription = {}\nlicense = {}\npublisher = {}\n\n[compatibility]\nhost_sdk = {}\nabi_major = 1\nabi_minor_min = 0\nabi_minor_max = 0\n",
        toml_string(plugin_id),
        toml_string(plugin_name),
        toml_string(&format!("{plugin_name} Vesper plugin.")),
        toml_string(license),
        toml_string(publisher),
        toml_string(&host_sdk),
    );
    for capability in capabilities {
        let interface_id = Uuid::from_bytes(capability.interface_id().0);
        let instance_id = format!("{plugin_id}.{}", capability.instance_suffix());
        manifest.push_str(&format!(
            "\n[[capabilities]]\ninterface_id = \"{interface_id}\"\ninstance_id = {}\ninterface_major = 1\ninterface_minor = 0\nstability = \"{}\"\n",
            toml_string(&instance_id),
            capability.stability(),
        ));
    }
    let artifact_capabilities = capabilities
        .iter()
        .map(|capability| {
            let interface_id = Uuid::from_bytes(capability.interface_id().0).to_string();
            let instance_id = format!("{plugin_id}.{}", capability.instance_suffix());
            format!(
                "{{ interface_id = {}, instance_id = {} }}",
                toml_string(&interface_id),
                toml_string(&instance_id)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    manifest.push_str(&format!(
        "\n[[artifacts]]\ntransport = \"{}\"\ntarget = {}\nformat = \"{}\"\nsource = {}\npath = {}\narchitecture = {}\ncapabilities = [{}]\n\n[[package_files]]\nsource = \"LICENSE\"\npath = \"licenses/LICENSE\"\nkind = \"license\"\n\n[[package_files]]\nsource = \"NOTICE\"\npath = \"notices/NOTICE\"\nkind = \"notice\"\n",
        transport.as_str(),
        toml_string(target),
        match transport {
            PluginArtifactTransport::Native => "dylib",
            PluginArtifactTransport::Wasm => "wasm-component",
        },
        toml_string(artifact_source),
        toml_string(artifact_archive_path),
        toml_string(architecture),
        artifact_capabilities,
    ));
    manifest
}

fn plugin_host_sdk_requirement() -> String {
    match semver::Version::parse(VESPER_SDK_VERSION) {
        Ok(version) if version.major == 0 => {
            let upper_minor = version.minor.checked_add(1).unwrap_or(version.minor);
            format!(">={version}, <0.{upper_minor}.0")
        }
        Ok(version) => {
            let upper_major = version.major.checked_add(1).unwrap_or(version.major);
            format!(">={version}, <{upper_major}.0.0")
        }
        Err(_) => format!("={VESPER_SDK_VERSION}"),
    }
}

fn render_native_source(
    plugin_id: &str,
    plugin_name: &str,
    capabilities: &[PluginScaffoldCapability],
) -> Result<String, PluginScaffoldError> {
    let implementations = capabilities
        .iter()
        .map(|capability| native_implementation(*capability))
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut builder_steps = String::new();
    for capability in capabilities {
        builder_steps.push_str(&format!(
            "    let builder = builder.{}({}, {})?;\n",
            native_builder_method(*capability),
            rust_string(&format!("{plugin_id}.{}", capability.instance_suffix())),
            native_type_name(*capability),
        ));
    }
    render(
        NATIVE_SOURCE_TEMPLATE,
        &[
            ("{{PLUGIN_ID}}", &rust_string(plugin_id)),
            ("{{PLUGIN_NAME}}", &rust_string(plugin_name)),
            ("{{IMPLEMENTATIONS}}", &implementations),
            ("{{BUILDER_STEPS}}", &builder_steps),
        ],
    )
}

fn render_wasm_source(
    capabilities: &[PluginScaffoldCapability],
) -> Result<String, PluginScaffoldError> {
    let has_event = capabilities.contains(&PluginScaffoldCapability::EventHook);
    let has_benchmark = capabilities.contains(&PluginScaffoldCapability::Benchmark);
    let world = match (has_event, has_benchmark) {
        (true, false) => "event-hook-plugin",
        (false, true) => "benchmark-sink-plugin",
        (true, true) => "event-and-benchmark-plugin",
        (false, false) => {
            return Err(PluginScaffoldError::Compatibility(
                "WASM scaffold requires EventHook or BenchmarkSink".to_owned(),
            ));
        }
    };
    let mut implementations = Vec::new();
    if has_event {
        implementations.push(
            r#"impl bindings::exports::vesper::plugin::event_hook::Guest for Component {
    fn on_event(
        _event: bindings::vesper::plugin::protocol::PipelineEvent,
    ) -> Result<
        bindings::vesper::plugin::protocol::EventHookOutcome,
        bindings::vesper::plugin::protocol::PluginError,
    > {
        Ok(bindings::vesper::plugin::protocol::EventHookOutcome {
            accepted: true,
            measurements: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}"#,
        );
    }
    if has_benchmark {
        implementations.push(
            r#"impl bindings::exports::vesper::plugin::benchmark_sink::Guest for Component {
    fn on_event_batch(
        batch: bindings::vesper::plugin::protocol::BenchmarkBatch,
    ) -> Result<u64, bindings::vesper::plugin::protocol::PluginError> {
        Ok(u64::try_from(batch.events.len()).unwrap_or(u64::MAX))
    }

    fn flush() -> Result<
        bindings::vesper::plugin::protocol::BenchmarkReport,
        bindings::vesper::plugin::protocol::PluginError,
    > {
        Ok(bindings::vesper::plugin::protocol::BenchmarkReport {
            accepted_events: 0,
            dropped_events: 0,
            measurements: Vec::new(),
            threshold_violations: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}"#,
        );
    }
    render(
        WASM_SOURCE_TEMPLATE,
        &[
            ("{{WIT_WORLD}}", &rust_string(world)),
            ("{{IMPLEMENTATIONS}}", &implementations.join("\n\n")),
        ],
    )
}

fn native_type_name(capability: PluginScaffoldCapability) -> &'static str {
    match capability {
        PluginScaffoldCapability::PostDownload => "PostDownload",
        PluginScaffoldCapability::EventHook => "EventHook",
        PluginScaffoldCapability::Benchmark => "Benchmark",
        PluginScaffoldCapability::Decoder => "DecoderFactory",
        PluginScaffoldCapability::FrameProcessor => "FrameProcessorFactory",
        PluginScaffoldCapability::SourceNormalizerPacket => "PacketNormalizerFactory",
        PluginScaffoldCapability::SourceNormalizerResource => "ResourceNormalizerFactory",
    }
}

fn native_builder_method(capability: PluginScaffoldCapability) -> &'static str {
    match capability {
        PluginScaffoldCapability::PostDownload => "with_post_download_processor",
        PluginScaffoldCapability::EventHook => "with_pipeline_event_hook",
        PluginScaffoldCapability::Benchmark => "with_benchmark_sink",
        PluginScaffoldCapability::Decoder => "with_native_decoder",
        PluginScaffoldCapability::FrameProcessor => "with_frame_processor",
        PluginScaffoldCapability::SourceNormalizerPacket => "with_source_normalizer_packet",
        PluginScaffoldCapability::SourceNormalizerResource => "with_source_normalizer_resource",
    }
}

fn native_implementation(capability: PluginScaffoldCapability) -> &'static str {
    match capability {
        PluginScaffoldCapability::PostDownload => {
            r#"struct PostDownload;

impl player_plugin::PostDownloadProcessor for PostDownload {
    fn name(&self) -> &str {
        "post-download"
    }

    fn supported_input_formats(&self) -> &[player_plugin::ContentFormatKind] {
        &[player_plugin::ContentFormatKind::SingleFile]
    }

    fn process(
        &self,
        _input: &player_plugin::CompletedDownloadInfo,
        _output_path: &std::path::Path,
        _progress: &dyn player_plugin::ProcessorProgress,
    ) -> Result<player_plugin::ProcessorOutput, player_plugin::ProcessorError> {
        Ok(player_plugin::ProcessorOutput::Skipped)
    }
}"#
        }
        PluginScaffoldCapability::EventHook => {
            r#"struct EventHook;

impl player_plugin::PipelineEventHook for EventHook {
    fn on_event(
        &self,
        _event: &player_plugin::PipelineEvent,
    ) -> Result<player_plugin::PipelineEventHookOutcome, player_plugin::PipelineEventHookError> {
        Ok(player_plugin::PipelineEventHookOutcome::accepted())
    }
}"#
        }
        PluginScaffoldCapability::Benchmark => {
            r#"struct Benchmark;

impl player_plugin::BenchmarkSink for Benchmark {
    fn name(&self) -> &str {
        "benchmark"
    }

    fn on_event_batch(
        &self,
        batch: &player_plugin::BenchmarkEventBatch,
    ) -> Result<player_plugin::BenchmarkSinkStatus, player_plugin::BenchmarkSinkError> {
        Ok(player_plugin::BenchmarkSinkStatus {
            accepted_events: u64::try_from(batch.events.len()).unwrap_or(u64::MAX),
        })
    }
}"#
        }
        PluginScaffoldCapability::Decoder => {
            r#"struct DecoderFactory;

impl player_plugin::NativeDecoderPluginFactory for DecoderFactory {
    fn name(&self) -> &str {
        "decoder"
    }

    fn capabilities(&self) -> player_plugin::DecoderCapabilities {
        player_plugin::DecoderCapabilities::default()
    }

    fn open_native_session(
        &self,
        _config: &player_plugin::DecoderSessionConfig,
    ) -> Result<Box<dyn player_plugin::NativeDecoderSession>, player_plugin::DecoderError> {
        Err(player_plugin::DecoderError::UnsupportedCapability {
            capability: "implement decoder session".to_owned(),
        })
    }
}"#
        }
        PluginScaffoldCapability::FrameProcessor => {
            r#"struct FrameProcessorFactory;

impl player_plugin::FrameProcessorPluginFactory for FrameProcessorFactory {
    fn name(&self) -> &str {
        "frame-processor"
    }

    fn capabilities(&self) -> player_plugin::FrameProcessorCapabilities {
        player_plugin::FrameProcessorCapabilities::default()
    }

    fn open_session(
        &self,
        _config: &player_plugin::FrameProcessorSessionConfig,
    ) -> Result<Box<dyn player_plugin::FrameProcessorSession>, player_plugin::FrameProcessorError> {
        Err(player_plugin::FrameProcessorError::internal(
            "implement frame-processor session",
        ))
    }
}"#
        }
        PluginScaffoldCapability::SourceNormalizerPacket => {
            r#"struct PacketNormalizerFactory;

impl player_plugin::SourceNormalizerPacketPluginFactory for PacketNormalizerFactory {
    fn name(&self) -> &str {
        "source-normalizer-packet"
    }

    fn packet_capabilities(&self) -> player_plugin::SourceNormalizerPacketCapabilities {
        player_plugin::SourceNormalizerPacketCapabilities::default()
    }

    fn open_packet_session(
        &self,
        _config: &player_plugin::SourceNormalizerPacketSessionConfig,
    ) -> Result<Box<dyn player_plugin::SourceNormalizerPacketSession>, player_plugin::SourceNormalizerError> {
        Err(player_plugin::SourceNormalizerError::unsupported_operation(
            "implement packet normalizer session",
        ))
    }
}"#
        }
        PluginScaffoldCapability::SourceNormalizerResource => {
            r#"struct ResourceNormalizerFactory;

impl player_plugin::SourceNormalizerResourcePluginFactory for ResourceNormalizerFactory {
    fn name(&self) -> &str {
        "source-normalizer-resource"
    }

    fn resource_capabilities(&self) -> player_plugin::SourceNormalizerResourceCapabilities {
        player_plugin::SourceNormalizerResourceCapabilities::default()
    }

    fn open_resource_session(
        &self,
        _config: &player_plugin::SourceNormalizerResourceSessionConfig,
    ) -> Result<Box<dyn player_plugin::SourceNormalizerResourceSession>, player_plugin::SourceNormalizerError> {
        Err(player_plugin::SourceNormalizerError::unsupported_operation(
            "implement resource normalizer session",
        ))
    }
}"#
        }
    }
}

fn artifact_target(
    transport: PluginArtifactTransport,
    library_name: &str,
) -> Result<(String, String, String), PluginScaffoldError> {
    if transport == PluginArtifactTransport::Wasm {
        return Ok((
            "wasm32-wasip2".to_owned(),
            "wasm32".to_owned(),
            format!("{library_name}.wasm"),
        ));
    }
    let architecture = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        architecture => architecture,
    };
    let (target, file) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => (
            "aarch64-apple-darwin".to_owned(),
            format!("lib{library_name}.dylib"),
        ),
        ("macos", "x86_64") => (
            "x86_64-apple-darwin".to_owned(),
            format!("lib{library_name}.dylib"),
        ),
        ("linux", "aarch64") => (
            "aarch64-unknown-linux-gnu".to_owned(),
            format!("lib{library_name}.so"),
        ),
        ("linux", "x86_64") => (
            "x86_64-unknown-linux-gnu".to_owned(),
            format!("lib{library_name}.so"),
        ),
        ("windows", "aarch64") => (
            "aarch64-pc-windows-msvc".to_owned(),
            format!("{library_name}.dll"),
        ),
        ("windows", "x86_64") => (
            "x86_64-pc-windows-msvc".to_owned(),
            format!("{library_name}.dll"),
        ),
        (os, architecture) => {
            return Err(PluginScaffoldError::Compatibility(format!(
                "no default Native Rust target is defined for {os}/{architecture}"
            )));
        }
    };
    Ok((target, architecture.to_owned(), file))
}

fn display_name(component: &str) -> String {
    component
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().chain(characters).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

fn rust_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"invalid template string\"".to_owned())
}

fn render(template: &str, replacements: &[(&str, &str)]) -> Result<String, PluginScaffoldError> {
    let mut rendered = template.to_owned();
    for (placeholder, value) in replacements {
        rendered = rendered.replace(placeholder, value);
    }
    if rendered.contains("{{") {
        return Err(PluginScaffoldError::Template(
            "one or more template placeholders were not replaced".to_owned(),
        ));
    }
    Ok(rendered)
}

fn write_scaffold_file(
    root: &Path,
    relative_path: &str,
    bytes: &[u8],
) -> Result<(), PluginScaffoldError> {
    let path = root.join(relative_path);
    let parent = path.parent().unwrap_or(root);
    fs::create_dir_all(parent).map_err(|source| PluginScaffoldError::Io {
        action: "create scaffold directory",
        path: parent.to_owned(),
        source,
    })?;
    fs::write(&path, bytes).map_err(|source| PluginScaffoldError::Io {
        action: "write scaffold file",
        path,
        source,
    })
}

fn license_contents(license: &str) -> String {
    if license == "Apache-2.0" {
        APACHE_2_LICENSE.to_owned()
    } else {
        format!(
            "SPDX-License-Identifier: {license}\n\nReplace this file with the complete license text before distribution.\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_scaffold_assets_match_repository_sources() {
        let Some(root) = crate::source_checkout_root() else {
            return;
        };

        for (path, embedded) in [
            (
                "templates/vesper-plugin/native/Cargo.toml.template",
                NATIVE_CARGO_TEMPLATE,
            ),
            (
                "templates/vesper-plugin/native/src/lib.rs.template",
                NATIVE_SOURCE_TEMPLATE,
            ),
            (
                "templates/vesper-plugin/native/README.md.template",
                NATIVE_README_TEMPLATE,
            ),
            (
                "templates/vesper-plugin/wasm/Cargo.toml.template",
                WASM_CARGO_TEMPLATE,
            ),
            (
                "templates/vesper-plugin/wasm/src/lib.rs.template",
                WASM_SOURCE_TEMPLATE,
            ),
            (
                "templates/vesper-plugin/wasm/README.md.template",
                WASM_README_TEMPLATE,
            ),
            ("LICENSE", APACHE_2_LICENSE),
        ] {
            let actual = fs::read_to_string(root.join(path)).expect("read repository asset");
            assert_eq!(actual, embedded, "embedded asset differs from {path}");
        }

        let actual_wit =
            fs::read(root.join("wit/vesper-plugin/plugin.wit")).expect("read canonical WIT");
        let template_wit = fs::read(root.join("templates/vesper-plugin/wasm/wit/plugin.wit"))
            .expect("read template WIT");
        assert_eq!(actual_wit, CANONICAL_WIT);
        assert_eq!(template_wit, CANONICAL_WIT);
    }

    #[test]
    fn native_scaffold_is_created_atomically_without_author_unsafe_code() {
        let parent = tempfile::tempdir().expect("scaffold parent");
        let directory = parent.path().join("native plugin");
        let report = create_plugin_scaffold(PluginScaffoldRequest {
            directory: directory.clone(),
            plugin_id: "dev.vesper.example".to_owned(),
            plugin_name: Some("Example".to_owned()),
            publisher: "dev.vesper".to_owned(),
            license: "Apache-2.0".to_owned(),
            transport: PluginArtifactTransport::Native,
            capabilities: vec![PluginScaffoldCapability::EventHook],
        })
        .expect("native scaffold");
        assert_eq!(report.directory, directory);
        let source = fs::read_to_string(directory.join("src/lib.rs")).expect("source");
        assert!(!source.contains("unsafe {"));
        assert!(!source.contains("extern \"C\""));
        assert!(directory.join("vesper-plugin.toml").is_file());
    }

    #[test]
    fn wasm_scaffold_uses_the_safe_guest_runtime() {
        let parent = tempfile::tempdir().expect("scaffold parent");
        let directory = parent.path().join("WASM plugin");
        create_plugin_scaffold(PluginScaffoldRequest {
            directory: directory.clone(),
            plugin_id: "dev.vesper.example".to_owned(),
            plugin_name: None,
            publisher: "dev.vesper".to_owned(),
            license: "Apache-2.0".to_owned(),
            transport: PluginArtifactTransport::Wasm,
            capabilities: vec![
                PluginScaffoldCapability::EventHook,
                PluginScaffoldCapability::Benchmark,
            ],
        })
        .expect("WASM scaffold");
        let cargo = fs::read_to_string(directory.join("Cargo.toml")).expect("Cargo.toml");
        let source = fs::read_to_string(directory.join("src/lib.rs")).expect("source");
        assert!(cargo.contains(&format!("player-plugin-wasm = \"={VESPER_SDK_VERSION}\"")));
        assert!(!cargo.contains("path ="));
        assert!(source.contains("#![deny(unsafe_code)]"));
        assert!(!source.contains("allow(unsafe_code)"));
        assert!(!source.contains("unsafe {"));
        assert!(!source.contains("unsafe fn"));
        assert!(!source.contains("unsafe extern"));
        assert!(!source.contains("extern \"C\""));
        assert!(source.contains("#![no_std]"));
        assert!(source.contains("player_plugin_wasm::generate!"));
        assert!(source.contains("player_plugin_wasm::export_component!(bindings, Component)"));
    }

    #[test]
    fn wasm_rejects_native_only_capabilities_before_creating_a_directory() {
        let parent = tempfile::tempdir().expect("scaffold parent");
        let directory = parent.path().join("wasm plugin");
        let error = create_plugin_scaffold(PluginScaffoldRequest {
            directory: directory.clone(),
            plugin_id: "dev.vesper.example".to_owned(),
            plugin_name: None,
            publisher: "dev.vesper".to_owned(),
            license: "Apache-2.0".to_owned(),
            transport: PluginArtifactTransport::Wasm,
            capabilities: vec![PluginScaffoldCapability::Decoder],
        })
        .expect_err("decoder is not a WASM capability");
        assert!(error.is_compatibility());
        assert!(!directory.exists());
    }
}
