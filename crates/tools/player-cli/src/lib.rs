#![deny(unsafe_code)]

pub mod ios_bridge_shim;

mod plugin_registry_fragment;

pub use player_plugin_package::{
    CanonicalPluginDescriptor, InstalledPluginActivation, InstalledPluginRecord,
    MAX_PLUGIN_PACKAGE_BYTES, MAX_PLUGIN_PACKAGE_ENTRIES, MAX_PLUGIN_PACKAGE_ENTRY_BYTES,
    MAX_PLUGIN_TRUST_STORE_BYTES, PLUGIN_PACKAGE_CHECKSUMS_PATH, PLUGIN_PACKAGE_MANIFEST_PATH,
    PLUGIN_PACKAGE_SIGNATURE_PATH, PluginArtifactCapability, PluginArtifactFormat,
    PluginArtifactSource, PluginArtifactTransport, PluginCapabilityDescriptor,
    PluginCompatibilityDescriptor, PluginCompatibilityError, PluginDescriptor,
    PluginDescriptorError, PluginHostTarget, PluginIdentityDescriptor, PluginInstallationReport,
    PluginPackageArtifact, PluginPackageBuildReport, PluginPackageError, PluginPackageFileKind,
    PluginPackageFileSource, PluginPackageGenerator, PluginPackageManifest,
    PluginPackageVerification, PluginProjectManifest, PluginProjectManifestError, PluginPublicKey,
    PluginRedistributionDescriptor, PluginRuntimeDependencySource, PluginRuntimeLinkage,
    PluginSigningKey, PluginStability, PluginTrustStore, TrustedKeyStatus,
    VerifiedInstalledArtifact, VerifiedInstalledPluginCatalog, VerifiedPluginPackage,
    build_signed_plugin_package, install_verified_plugin_package, list_installed_plugins,
    uninstall_plugin, verify_installed_plugin_catalog, verify_signed_plugin_package,
};
pub use plugin_registry_fragment::{
    EmbeddedRegistryFragment, EmbeddedRegistryFragmentError, EmbeddedRegistryTarget,
};
