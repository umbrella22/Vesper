#![deny(unsafe_code)]

pub mod ios_bridge_shim;

mod plugin_descriptor;
mod plugin_project;
mod plugin_registry_fragment;

pub use player_plugin::{
    CanonicalPluginArtifactDescriptor, PLUGIN_CATALOG_MIGRATION_VERSION,
    PLUGIN_CATALOG_SCHEMA_VERSION, PluginArtifactCapability, PluginArtifactDescriptor,
    PluginProvision, PluginRequirement,
};
pub use plugin_descriptor::{
    CanonicalPluginDescriptor, PluginCapabilityDescriptor, PluginCompatibilityDescriptor,
    PluginCompatibilityError, PluginDescriptor, PluginDescriptorError, PluginIdentityDescriptor,
    PluginRedistributionDescriptor, PluginStability,
};
pub use plugin_project::{
    PluginArtifactFormat, PluginArtifactSource, PluginArtifactTransport, PluginPackageFileKind,
    PluginPackageFileSource, PluginProjectManifest, PluginProjectManifestError,
    PluginRuntimeDependencySource, PluginRuntimeLinkage,
};

pub use plugin_registry_fragment::{
    EmbeddedRegistryFragment, EmbeddedRegistryFragmentError, EmbeddedRegistryTarget,
};
