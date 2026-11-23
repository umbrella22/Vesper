#![warn(clippy::undocumented_unsafe_blocks)]
#![allow(
    clippy::result_large_err,
    reason = "loader error enums are public APIs; boxing their source variants would be breaking"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use player_plugin::{
    DecoderCapabilities, DecoderCodecCapability, DecoderMediaKind, DecoderNativeRequirements,
    FrameProcessorCapabilities, FrameProcessorPluginFactory, NativeDecoderPluginFactory,
    NativeFramePipelineProfile, NativeHandleKind, PostDownloadProcessor,
    SourceNormalizerPacketCapabilities, SourceNormalizerPacketPluginFactory,
    SourceNormalizerResourceCapabilities, SourceNormalizerResourcePluginFactory,
};
use thiserror::Error;

mod capability_ids;
mod catalog;
mod diagnostics;
mod embedded_registry;
mod native_abi;

/// Validated native plugin loading and direct Rust registration.
pub mod native;
mod native_library;
mod registry;

pub use catalog::{
    MAX_PLUGIN_CATALOG_IMPORT_ARTIFACT_BYTES, PluginCatalogImportError, PluginCatalogImporter,
    PluginCatalogIndex,
};
pub use diagnostics::{
    DecoderPluginCapabilitySummary, DecoderPluginCodecSummary, DecoderPluginMatchRequest,
    FrameProcessorPluginCapabilitySummary, PluginCapabilityAvailability, PluginCapabilityKind,
    PluginCapabilitySummary, PluginDiagnosticRecord, PluginDiagnosticStatus,
    SourceNormalizerPacketPluginCapabilitySummary, SourceNormalizerResourcePluginCapabilitySummary,
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
    LoadedNativePlugin, PluginContractDiagnosticKind, PluginInterfaceDiagnostic,
    PluginInterfaceMetadata, PluginInterfaceRecord, PluginInterfaceState, PluginSelectionError,
};
pub use native_library::PluginLoadError;
pub use player_plugin::{
    CanonicalPluginArtifactDescriptor, MAX_PLUGIN_PROVISIONS, MAX_PLUGIN_REQUIREMENTS,
    PLUGIN_CATALOG_MIGRATION_VERSION, PLUGIN_CATALOG_SCHEMA_VERSION, PluginArtifactCapability,
    PluginArtifactDescriptor, PluginArtifactFormat, PluginArtifactTransport, PluginCatalog,
    PluginCatalogDiagnostic, PluginCatalogError, PluginCatalogRecord, PluginCatalogSource,
    PluginInvocationPolicy, PluginInvocationPolicyError, PluginInvocationWorkload, PluginProvision,
    PluginRequirement,
};
pub use registry::{
    NativePluginArtifact, PluginRegistry, PluginRegistryBuildError, PluginRegistryReport,
    RegisteredPluginInterface, ResolvedPluginCapability,
};

#[cfg(test)]
mod tests;
