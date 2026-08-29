#![deny(unsafe_code)]

mod plugin_descriptor;
mod plugin_package;
mod plugin_project;

pub use player_plugin::{
    CanonicalPluginArtifactDescriptor, MAX_PLUGIN_ARCHITECTURE_BYTES,
    MAX_PLUGIN_ARTIFACT_CAPABILITIES, MAX_PLUGIN_ARTIFACT_PATH_BYTES,
    MAX_PLUGIN_CATALOG_DIAGNOSTICS, MAX_PLUGIN_CATALOG_RECORDS, MAX_PLUGIN_CATALOG_SOURCE_BYTES,
    MAX_PLUGIN_PROVISIONS, MAX_PLUGIN_REQUIREMENTS, MAX_PLUGIN_RUNTIME_DEPENDENCIES,
    MAX_PLUGIN_TARGET_BYTES, PLUGIN_CATALOG_MIGRATION_VERSION, PLUGIN_CATALOG_SCHEMA_VERSION,
    PluginArtifactDescriptor, PluginCatalog, PluginCatalogDiagnostic, PluginCatalogError,
    PluginCatalogRecord, PluginCatalogSource, PluginProvision, PluginRequirement,
    PluginResourcePolicy, PluginRuntimeDependency,
};
pub use plugin_descriptor::{
    CanonicalPluginDescriptor, PluginCapabilityDescriptor, PluginCompatibilityDescriptor,
    PluginCompatibilityError, PluginDescriptor, PluginDescriptorError, PluginIdentityDescriptor,
    PluginRedistributionDescriptor, PluginStability,
};
pub use plugin_package::{
    InstalledPluginActivation, InstalledPluginRecord, MAX_PLUGIN_PACKAGE_BYTES,
    MAX_PLUGIN_PACKAGE_ENTRIES, MAX_PLUGIN_PACKAGE_ENTRY_BYTES, MAX_PLUGIN_TRUST_STORE_BYTES,
    PLUGIN_PACKAGE_CHECKSUMS_PATH, PLUGIN_PACKAGE_MANIFEST_PATH, PLUGIN_PACKAGE_SIGNATURE_PATH,
    PluginHostTarget, PluginInstallationReport, PluginPackageArtifact, PluginPackageBuildReport,
    PluginPackageError, PluginPackageGenerator, PluginPackageManifest, PluginPackageVerification,
    PluginPublicKey, PluginSigningKey, PluginTrustStore, TrustedKeyStatus,
    VerifiedInstalledArtifact, VerifiedInstalledPluginCatalog, VerifiedPluginPackage,
    build_signed_plugin_package, install_verified_plugin_package, list_installed_plugins,
    uninstall_plugin, verify_installed_plugin_catalog, verify_signed_plugin_package,
};
pub use plugin_project::{
    PluginArtifactCapability, PluginArtifactFormat, PluginArtifactSource, PluginArtifactTransport,
    PluginPackageFileKind, PluginPackageFileSource, PluginProjectManifest,
    PluginProjectManifestError, PluginRuntimeDependencySource, PluginRuntimeLinkage,
};
