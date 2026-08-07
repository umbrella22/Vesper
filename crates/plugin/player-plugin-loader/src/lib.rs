#![warn(clippy::undocumented_unsafe_blocks)]
#![allow(
    clippy::result_large_err,
    reason = "loader error enums are public APIs; boxing their source variants would be breaking"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use player_plugin::{
    BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, BenchmarkSinkReport,
    DecoderCapabilities, DecoderCodecCapability, DecoderMediaKind, DecoderNativeRequirements,
    FrameProcessorCapabilities, FrameProcessorPluginFactory, NativeDecoderPluginFactory,
    NativeFramePipelineProfile, NativeHandleKind, PipelineEventHook, PluginDiagnostic,
    PluginDiagnosticSeverity, PostDownloadProcessor, SourceNormalizerPacketCapabilities,
    SourceNormalizerPacketPluginFactory, SourceNormalizerResourceCapabilities,
    SourceNormalizerResourcePluginFactory,
};
use thiserror::Error;

mod benchmark;
mod diagnostics;
mod embedded_registry;
mod native_abi;
mod native_library;
mod registry;
#[cfg(feature = "wasm")]
mod wasm;

pub use benchmark::BenchmarkSinkPluginSession;
pub use diagnostics::{
    DecoderPluginCapabilitySummary, DecoderPluginCodecSummary, DecoderPluginMatchRequest,
    FrameProcessorPluginCapabilitySummary, PluginCapabilityKind, PluginCapabilitySummary,
    PluginDiagnosticRecord, PluginDiagnosticStatus, SourceNormalizerPacketPluginCapabilitySummary,
    SourceNormalizerResourcePluginCapabilitySummary,
};
pub use embedded_registry::{
    EmbeddedAppleCodeSignatureValidation, EmbeddedPluginArtifact, EmbeddedPluginCapability,
    EmbeddedPluginIntegrity, EmbeddedPluginLocator, EmbeddedPluginPackage, EmbeddedPluginRegistry,
    EmbeddedPluginRegistryError, MAX_ANDROID_PACKAGE_PATHS,
    MAX_EMBEDDED_PLUGIN_ARCHIVE_ARTIFACT_BYTES, MAX_EMBEDDED_PLUGIN_ARTIFACTS,
    MAX_EMBEDDED_PLUGIN_CAPABILITIES_PER_ARTIFACT, MAX_EMBEDDED_PLUGIN_REGISTRY_BYTES,
    MAX_EMBEDDED_PLUGIN_REGISTRY_FRAGMENTS, MAX_EMBEDDED_PLUGIN_REGISTRY_SET_BYTES,
    resolve_android_native_library,
};
pub use native_abi::{
    LoadedNativePlugin, NativePluginContractError, PluginContractDiagnosticKind,
    PluginInterfaceDiagnostic, PluginInterfaceMetadata, PluginInterfaceRecord,
    PluginInterfaceState, PluginSelectionError,
};
pub use native_library::PluginLoadError;
#[cfg(feature = "wasm")]
pub use player_plugin_wasm_host::{
    WASM_PLUGIN_WIT_INTERFACE_MAJOR, WASM_PLUGIN_WIT_INTERFACE_MINOR,
};
pub use registry::{
    NativePluginArtifact, PluginRegistry, PluginRegistryBuildError, PluginRegistryReport,
    RegisteredPluginInterface, ResolvedPluginCapability,
};
#[cfg(feature = "wasm")]
pub use wasm::{
    WasmPluginArtifact, WasmPluginArtifactError, WasmPluginInterfaceDeclaration,
    WasmPluginLoadError,
};

pub(crate) use native_library::LibraryHolder;
#[cfg(feature = "wasm")]
pub(crate) use wasm::LoadedWasmPlugin;

#[cfg(test)]
mod tests;
