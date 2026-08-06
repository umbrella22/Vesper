#![deny(unsafe_code)]

mod plugin_descriptor;
mod plugin_package;
mod plugin_project;

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
