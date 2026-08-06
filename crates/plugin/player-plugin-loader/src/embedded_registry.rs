use std::collections::{BTreeSet, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use player_plugin::{PluginReference, PluginReferenceError, PluginTransport};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
use zip::{CompressionMethod, ZipArchive, result::ZipError};

use crate::{NativePluginArtifact, PluginInterfaceState, PluginRegistry, PluginRegistryBuildError};

pub const MAX_EMBEDDED_PLUGIN_REGISTRY_BYTES: usize = 1024 * 1024;
pub const MAX_EMBEDDED_PLUGIN_REGISTRY_SET_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_EMBEDDED_PLUGIN_REGISTRY_FRAGMENTS: usize = 256;
pub const MAX_EMBEDDED_PLUGIN_ARTIFACTS: usize = 256;
pub const MAX_EMBEDDED_PLUGIN_CAPABILITIES_PER_ARTIFACT: usize = 64;
pub const MAX_EMBEDDED_PLUGIN_ARCHIVE_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_ANDROID_PACKAGE_PATHS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedPluginRegistry {
    schema_version: u32,
    target: String,
    architecture: String,
    minimum_os: Option<String>,
    artifacts: Vec<EmbeddedPluginArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedPluginArtifact {
    plugin_id: String,
    transport: PluginTransport,
    locator: EmbeddedPluginLocator,
    integrity: EmbeddedPluginIntegrity,
    package: EmbeddedPluginPackage,
    capabilities: Vec<EmbeddedPluginCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum EmbeddedPluginLocator {
    AndroidNativeLibrary {
        name: String,
    },
    AppleFramework {
        name: String,
        bundle_identifier: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum EmbeddedPluginIntegrity {
    Sha256 {
        digest: String,
    },
    AppleCodeSignature {
        validation: EmbeddedAppleCodeSignatureValidation,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EmbeddedAppleCodeSignatureValidation {
    SameTeamAsHostOrSimulatorAdHoc,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedPluginPackage {
    version: String,
    publisher: String,
    descriptor_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedPluginCapability {
    interface_id: String,
    instance_id: String,
    interface_major: u16,
    interface_minor: u16,
}

#[derive(Debug, Error)]
pub enum EmbeddedPluginRegistryError {
    #[error("embedded plugin registry is {actual_bytes} bytes; maximum is {maximum_bytes} bytes")]
    OversizedRegistry {
        actual_bytes: usize,
        maximum_bytes: usize,
    },
    #[error("invalid embedded plugin registry JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported embedded plugin registry schema version {0}")]
    SchemaVersion(u32),
    #[error("embedded plugin registry contains {actual} fragments; maximum is {maximum}")]
    TooManyFragments { actual: usize, maximum: usize },
    #[error(
        "embedded plugin registry fragments total {actual_bytes} bytes; maximum is {maximum_bytes} bytes"
    )]
    OversizedRegistrySet {
        actual_bytes: usize,
        maximum_bytes: usize,
    },
    #[error("embedded plugin registry target `{actual}` does not match `{expected}`")]
    TargetMismatch { expected: String, actual: String },
    #[error("embedded plugin registry architecture `{actual}` does not match `{expected}`")]
    ArchitectureMismatch { expected: String, actual: String },
    #[error("embedded plugin registry fragment minimum OS {actual:?} does not match {expected:?}")]
    MinimumOsMismatch {
        expected: Option<String>,
        actual: Option<String>,
    },
    #[error("invalid embedded plugin registry field `{field}`: {message}")]
    InvalidField { field: String, message: String },
    #[error("embedded plugin registry contains {actual} artifacts; maximum is {maximum}")]
    TooManyArtifacts { actual: usize, maximum: usize },
    #[error("duplicate embedded plugin identity `{0}`")]
    DuplicatePluginId(String),
    #[error("duplicate embedded plugin locator `{0}`")]
    DuplicateLocator(String),
    #[error("duplicate embedded plugin capability `{plugin_id}:{interface_id}:{instance_id}`")]
    DuplicateCapability {
        plugin_id: String,
        interface_id: String,
        instance_id: String,
    },
    #[error("embedded plugin `{plugin_id}` uses unsupported mobile transport {transport:?}")]
    UnsupportedTransport {
        plugin_id: String,
        transport: PluginTransport,
    },
    #[error("embedded plugin registry does not contain referenced plugin `{0}`")]
    UnknownPluginReference(String),
    #[error("failed to resolve embedded plugin `{plugin_id}`: {message}")]
    ResolveArtifact { plugin_id: String, message: String },
    #[error("embedded plugin artifact `{path}` is not a regular file")]
    ArtifactNotFile { path: String },
    #[error(
        "embedded plugin artifact `{path}` is {actual_bytes} bytes; maximum is {maximum_bytes} bytes"
    )]
    OversizedArtifact {
        path: String,
        actual_bytes: u64,
        maximum_bytes: u64,
    },
    #[error("Android package `{path}` contains duplicate native library entry `{entry}`")]
    DuplicateAndroidPackageEntry { path: String, entry: String },
    #[error("failed to read embedded plugin artifact `{path}`: {source}")]
    ReadArtifact {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error(
        "embedded plugin artifact `{path}` checksum mismatch: expected {expected}, actual {actual}"
    )]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("embedded plugin `{plugin_id}` requires platform integrity verification")]
    PlatformIntegrityVerificationRequired { plugin_id: String },
    #[error("platform integrity verification failed for embedded plugin `{plugin_id}`: {message}")]
    PlatformIntegrityVerification { plugin_id: String, message: String },
    #[error(transparent)]
    Load(#[from] PluginRegistryBuildError),
    #[error("embedded plugin `{plugin_id}` has unavailable interface `{instance_id}`: {message}")]
    UnavailableInterface {
        plugin_id: String,
        instance_id: String,
        message: String,
    },
    #[error("embedded plugin `{plugin_id}` capability metadata does not match its Root ABI")]
    CapabilityMismatch { plugin_id: String },
}

impl EmbeddedPluginRegistry {
    pub fn parse(
        json: &[u8],
        expected_target: &str,
        expected_architecture: &str,
    ) -> Result<Self, EmbeddedPluginRegistryError> {
        if json.len() > MAX_EMBEDDED_PLUGIN_REGISTRY_BYTES {
            return Err(EmbeddedPluginRegistryError::OversizedRegistry {
                actual_bytes: json.len(),
                maximum_bytes: MAX_EMBEDDED_PLUGIN_REGISTRY_BYTES,
            });
        }
        let registry: Self = serde_json::from_slice(json)?;
        registry.validate(expected_target, expected_architecture)?;
        Ok(registry)
    }

    /// Parses and combines independently packaged registry fragments.
    ///
    /// Mobile package managers merge plugin artifacts from multiple
    /// dependencies. Each dependency owns one fragment; this method applies
    /// the same limits and duplicate checks to the combined registry before
    /// any dynamic library is loaded.
    pub fn parse_fragments<'a>(
        fragments: impl IntoIterator<Item = &'a [u8]>,
        expected_target: &str,
        expected_architecture: &str,
    ) -> Result<Self, EmbeddedPluginRegistryError> {
        let mut combined = Self {
            schema_version: 1,
            target: expected_target.to_owned(),
            architecture: expected_architecture.to_owned(),
            minimum_os: None,
            artifacts: Vec::new(),
        };
        let mut fragment_count = 0_usize;
        let mut total_bytes = 0_usize;
        let mut expected_minimum_os: Option<Option<String>> = None;

        for json in fragments {
            fragment_count = fragment_count.saturating_add(1);
            if fragment_count > MAX_EMBEDDED_PLUGIN_REGISTRY_FRAGMENTS {
                return Err(EmbeddedPluginRegistryError::TooManyFragments {
                    actual: fragment_count,
                    maximum: MAX_EMBEDDED_PLUGIN_REGISTRY_FRAGMENTS,
                });
            }
            total_bytes = total_bytes.saturating_add(json.len());
            if total_bytes > MAX_EMBEDDED_PLUGIN_REGISTRY_SET_BYTES {
                return Err(EmbeddedPluginRegistryError::OversizedRegistrySet {
                    actual_bytes: total_bytes,
                    maximum_bytes: MAX_EMBEDDED_PLUGIN_REGISTRY_SET_BYTES,
                });
            }

            let fragment = Self::parse(json, expected_target, expected_architecture)?;
            if !fragment.artifacts.is_empty() {
                match expected_minimum_os.as_ref() {
                    Some(expected) if expected != &fragment.minimum_os => {
                        return Err(EmbeddedPluginRegistryError::MinimumOsMismatch {
                            expected: expected.clone(),
                            actual: fragment.minimum_os,
                        });
                    }
                    None => {
                        expected_minimum_os = Some(fragment.minimum_os.clone());
                        combined.minimum_os = fragment.minimum_os.clone();
                    }
                    Some(_) => {}
                }
            }
            combined.artifacts.extend(fragment.artifacts);
            if combined.artifacts.len() > MAX_EMBEDDED_PLUGIN_ARTIFACTS {
                return Err(EmbeddedPluginRegistryError::TooManyArtifacts {
                    actual: combined.artifacts.len(),
                    maximum: MAX_EMBEDDED_PLUGIN_ARTIFACTS,
                });
            }
        }

        combined.validate(expected_target, expected_architecture)?;
        Ok(combined)
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn architecture(&self) -> &str {
        &self.architecture
    }

    pub fn minimum_os(&self) -> Option<&str> {
        self.minimum_os.as_deref()
    }

    pub fn artifacts(&self) -> &[EmbeddedPluginArtifact] {
        &self.artifacts
    }

    pub fn load_native(
        &self,
        mut resolve: impl FnMut(&EmbeddedPluginLocator) -> Result<PathBuf, String>,
    ) -> Result<PluginRegistry, EmbeddedPluginRegistryError> {
        self.load_native_artifacts(self.artifacts.iter(), &mut resolve, None)
    }

    /// Loads every artifact after the host verifies platform-owned integrity.
    pub fn load_native_with_platform_integrity(
        &self,
        mut resolve: impl FnMut(&EmbeddedPluginLocator) -> Result<PathBuf, String>,
        mut verify_platform_integrity: impl FnMut(&Path, &EmbeddedPluginArtifact) -> Result<(), String>,
    ) -> Result<PluginRegistry, EmbeddedPluginRegistryError> {
        self.load_native_artifacts(
            self.artifacts.iter(),
            &mut resolve,
            Some(&mut verify_platform_integrity),
        )
    }

    /// Loads only artifacts named by explicit runtime references.
    ///
    /// Merely packaging a native plugin must not execute its entry point or
    /// affect the no-plugin mobile baseline. Multiple capability references
    /// to the same plugin load that artifact exactly once.
    pub fn load_native_selected<'a>(
        &self,
        references: impl IntoIterator<Item = &'a PluginReference>,
        mut resolve: impl FnMut(&EmbeddedPluginLocator) -> Result<PathBuf, String>,
    ) -> Result<PluginRegistry, EmbeddedPluginRegistryError> {
        let artifacts = self.select_native_artifacts(references)?;
        self.load_native_artifacts(artifacts, &mut resolve, None)
    }

    /// Loads explicitly selected artifacts after the host verifies
    /// platform-owned integrity such as an Apple code signature.
    pub fn load_native_selected_with_platform_integrity<'a>(
        &self,
        references: impl IntoIterator<Item = &'a PluginReference>,
        mut resolve: impl FnMut(&EmbeddedPluginLocator) -> Result<PathBuf, String>,
        mut verify_platform_integrity: impl FnMut(&Path, &EmbeddedPluginArtifact) -> Result<(), String>,
    ) -> Result<PluginRegistry, EmbeddedPluginRegistryError> {
        let artifacts = self.select_native_artifacts(references)?;
        self.load_native_artifacts(
            artifacts,
            &mut resolve,
            Some(&mut verify_platform_integrity),
        )
    }

    /// Resolves explicit Native references to packaged artifact metadata
    /// without loading executable code.
    ///
    /// Multiple capability references to one plugin produce one artifact in
    /// registry order. Unknown plugin identities and non-Native transports
    /// fail before a host inspects any artifact path.
    pub fn select_native_artifacts<'a, 'reference>(
        &'a self,
        references: impl IntoIterator<Item = &'reference PluginReference>,
    ) -> Result<Vec<&'a EmbeddedPluginArtifact>, EmbeddedPluginRegistryError> {
        let selected_plugin_ids = self.selected_plugin_ids(references)?;
        Ok(self
            .artifacts
            .iter()
            .filter(|artifact| selected_plugin_ids.contains(artifact.plugin_id.as_str()))
            .collect())
    }

    fn load_native_artifacts<'a>(
        &'a self,
        artifacts: impl IntoIterator<Item = &'a EmbeddedPluginArtifact>,
        resolve: &mut impl FnMut(&EmbeddedPluginLocator) -> Result<PathBuf, String>,
        mut verify_platform_integrity: Option<
            &mut dyn FnMut(&Path, &EmbeddedPluginArtifact) -> Result<(), String>,
        >,
    ) -> Result<PluginRegistry, EmbeddedPluginRegistryError> {
        let artifacts = artifacts.into_iter().collect::<Vec<_>>();
        let mut native_artifacts = Vec::with_capacity(artifacts.len());
        for artifact in &artifacts {
            let path = resolve(&artifact.locator).map_err(|message| {
                EmbeddedPluginRegistryError::ResolveArtifact {
                    plugin_id: artifact.plugin_id.clone(),
                    message,
                }
            })?;
            match &artifact.integrity {
                EmbeddedPluginIntegrity::Sha256 { digest } => verify_sha256(&path, digest)?,
                EmbeddedPluginIntegrity::AppleCodeSignature { .. } => {
                    let Some(verifier) = verify_platform_integrity.as_deref_mut() else {
                        return Err(
                            EmbeddedPluginRegistryError::PlatformIntegrityVerificationRequired {
                                plugin_id: artifact.plugin_id.clone(),
                            },
                        );
                    };
                    verifier(&path, artifact).map_err(|message| {
                        EmbeddedPluginRegistryError::PlatformIntegrityVerification {
                            plugin_id: artifact.plugin_id.clone(),
                            message,
                        }
                    })?;
                }
            }
            native_artifacts.push(
                NativePluginArtifact::new(&artifact.plugin_id, path)
                    .map_err(|error| invalid_identity("plugin_id", &artifact.plugin_id, error))?,
            );
        }
        let registry = PluginRegistry::load_native_artifacts(native_artifacts)?;
        self.verify_loaded_capabilities(&registry, artifacts.iter().copied())?;
        Ok(registry)
    }

    fn selected_plugin_ids<'a>(
        &self,
        references: impl IntoIterator<Item = &'a PluginReference>,
    ) -> Result<HashSet<String>, EmbeddedPluginRegistryError> {
        let mut selected_plugin_ids = HashSet::new();
        for reference in references {
            if reference.transport() != PluginTransport::Native {
                return Err(EmbeddedPluginRegistryError::UnsupportedTransport {
                    plugin_id: reference.plugin_id().to_owned(),
                    transport: reference.transport(),
                });
            }
            selected_plugin_ids.insert(reference.plugin_id().to_owned());
        }

        for plugin_id in &selected_plugin_ids {
            if !self
                .artifacts
                .iter()
                .any(|artifact| artifact.plugin_id == *plugin_id)
            {
                return Err(EmbeddedPluginRegistryError::UnknownPluginReference(
                    plugin_id.clone(),
                ));
            }
        }
        Ok(selected_plugin_ids)
    }

    fn validate(
        &self,
        expected_target: &str,
        expected_architecture: &str,
    ) -> Result<(), EmbeddedPluginRegistryError> {
        if self.schema_version != 1 {
            return Err(EmbeddedPluginRegistryError::SchemaVersion(
                self.schema_version,
            ));
        }
        if self.target != expected_target {
            return Err(EmbeddedPluginRegistryError::TargetMismatch {
                expected: expected_target.to_owned(),
                actual: self.target.clone(),
            });
        }
        if self.architecture != expected_architecture {
            return Err(EmbeddedPluginRegistryError::ArchitectureMismatch {
                expected: expected_architecture.to_owned(),
                actual: self.architecture.clone(),
            });
        }
        validate_text("target", &self.target, 128)?;
        validate_text("architecture", &self.architecture, 64)?;
        if !self.artifacts.is_empty() && self.minimum_os.is_none() {
            return Err(EmbeddedPluginRegistryError::InvalidField {
                field: "minimum_os".to_owned(),
                message: "is required when artifacts are present".to_owned(),
            });
        }
        if let Some(minimum_os) = self.minimum_os.as_deref() {
            validate_text("minimum_os", minimum_os, 64)?;
        }
        if self.artifacts.len() > MAX_EMBEDDED_PLUGIN_ARTIFACTS {
            return Err(EmbeddedPluginRegistryError::TooManyArtifacts {
                actual: self.artifacts.len(),
                maximum: MAX_EMBEDDED_PLUGIN_ARTIFACTS,
            });
        }

        let mut plugin_ids = HashSet::with_capacity(self.artifacts.len());
        let mut locators = HashSet::with_capacity(self.artifacts.len());
        for artifact in &self.artifacts {
            PluginReference::new(&artifact.plugin_id, None, PluginTransport::Native)
                .map_err(|error| invalid_identity("plugin_id", &artifact.plugin_id, error))?;
            if artifact.transport != PluginTransport::Native {
                return Err(EmbeddedPluginRegistryError::UnsupportedTransport {
                    plugin_id: artifact.plugin_id.clone(),
                    transport: artifact.transport,
                });
            }
            validate_locator(&self.target, &artifact.locator)?;
            validate_integrity(&self.target, &artifact.integrity)?;
            validate_package(&artifact.package)?;
            validate_capabilities(&artifact.plugin_id, &artifact.capabilities)?;

            if !plugin_ids.insert(artifact.plugin_id.clone()) {
                return Err(EmbeddedPluginRegistryError::DuplicatePluginId(
                    artifact.plugin_id.clone(),
                ));
            }
            if !locators.insert(artifact.locator.clone()) {
                return Err(EmbeddedPluginRegistryError::DuplicateLocator(
                    artifact.locator.label(),
                ));
            }
        }
        Ok(())
    }

    fn verify_loaded_capabilities<'a>(
        &'a self,
        registry: &PluginRegistry,
        artifacts: impl IntoIterator<Item = &'a EmbeddedPluginArtifact>,
    ) -> Result<(), EmbeddedPluginRegistryError> {
        for artifact in artifacts {
            let mut actual = BTreeSet::new();
            for registered in registry
                .registered_interfaces()
                .iter()
                .filter(|registered| registered.plugin_id == artifact.plugin_id)
            {
                if registered.interface.state != PluginInterfaceState::Available {
                    return Err(EmbeddedPluginRegistryError::UnavailableInterface {
                        plugin_id: artifact.plugin_id.clone(),
                        instance_id: registered.interface.metadata.instance_id.clone(),
                        message: "interface is unavailable for the host ABI".to_owned(),
                    });
                }
                actual.insert((
                    Uuid::from_bytes(registered.interface.metadata.interface_id),
                    registered.interface.metadata.instance_id.clone(),
                    registered.interface.metadata.major,
                    registered.interface.metadata.minor,
                ));
            }
            let declared = artifact
                .capabilities
                .iter()
                .map(EmbeddedPluginCapability::identity)
                .collect::<Result<BTreeSet<_>, _>>()?;
            if actual != declared {
                return Err(EmbeddedPluginRegistryError::CapabilityMismatch {
                    plugin_id: artifact.plugin_id.clone(),
                });
            }
        }
        Ok(())
    }
}

impl EmbeddedPluginArtifact {
    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub const fn transport(&self) -> PluginTransport {
        self.transport
    }

    pub fn locator(&self) -> &EmbeddedPluginLocator {
        &self.locator
    }

    pub fn integrity(&self) -> &EmbeddedPluginIntegrity {
        &self.integrity
    }

    pub fn package(&self) -> &EmbeddedPluginPackage {
        &self.package
    }

    pub fn capabilities(&self) -> &[EmbeddedPluginCapability] {
        &self.capabilities
    }
}

impl EmbeddedPluginLocator {
    pub fn name(&self) -> &str {
        match self {
            Self::AndroidNativeLibrary { name } | Self::AppleFramework { name, .. } => name,
        }
    }

    pub fn apple_bundle_identifier(&self) -> Option<&str> {
        match self {
            Self::AppleFramework {
                bundle_identifier, ..
            } => Some(bundle_identifier),
            Self::AndroidNativeLibrary { .. } => None,
        }
    }

    fn label(&self) -> String {
        match self {
            Self::AndroidNativeLibrary { name } => format!("android-native-library:{name}"),
            Self::AppleFramework {
                name,
                bundle_identifier,
            } => format!("apple-framework:{name}:{bundle_identifier}"),
        }
    }
}

impl EmbeddedPluginIntegrity {
    pub fn sha256_digest(&self) -> Option<&str> {
        match self {
            Self::Sha256 { digest } => Some(digest),
            Self::AppleCodeSignature { .. } => None,
        }
    }

    pub const fn apple_code_signature_validation(
        &self,
    ) -> Option<EmbeddedAppleCodeSignatureValidation> {
        match self {
            Self::AppleCodeSignature { validation } => Some(*validation),
            Self::Sha256 { .. } => None,
        }
    }
}

impl EmbeddedPluginPackage {
    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn publisher(&self) -> &str {
        &self.publisher
    }

    pub fn descriptor_sha256(&self) -> &str {
        &self.descriptor_sha256
    }
}

impl EmbeddedPluginCapability {
    pub fn interface_id(&self) -> &str {
        &self.interface_id
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub const fn interface_major(&self) -> u16 {
        self.interface_major
    }

    pub const fn interface_minor(&self) -> u16 {
        self.interface_minor
    }

    fn identity(&self) -> Result<(Uuid, String, u16, u16), EmbeddedPluginRegistryError> {
        Ok((
            parse_canonical_uuid("interface_id", &self.interface_id)?,
            self.instance_id.clone(),
            self.interface_major,
            self.interface_minor,
        ))
    }
}

fn validate_locator(
    target: &str,
    locator: &EmbeddedPluginLocator,
) -> Result<(), EmbeddedPluginRegistryError> {
    let target_matches = match locator {
        EmbeddedPluginLocator::AndroidNativeLibrary { .. } => target.contains("android"),
        EmbeddedPluginLocator::AppleFramework { .. } => target.contains("apple-ios"),
    };
    if !target_matches {
        return Err(EmbeddedPluginRegistryError::InvalidField {
            field: "locator.kind".to_owned(),
            message: format!("{} is incompatible with target `{target}`", locator.label()),
        });
    }
    let name = locator.name();
    if name.is_empty() || name.len() > 128 || !name.is_ascii() {
        return Err(EmbeddedPluginRegistryError::InvalidField {
            field: "locator.name".to_owned(),
            message: "must be 1 to 128 ASCII bytes".to_owned(),
        });
    }
    let mut bytes = name.bytes();
    if !bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        return Err(EmbeddedPluginRegistryError::InvalidField {
            field: "locator.name".to_owned(),
            message: "contains unsupported characters".to_owned(),
        });
    }
    if let EmbeddedPluginLocator::AppleFramework {
        bundle_identifier, ..
    } = locator
    {
        PluginReference::new(bundle_identifier, None, PluginTransport::Native).map_err(
            |error| invalid_identity("locator.bundle_identifier", bundle_identifier, error),
        )?;
    }
    Ok(())
}

fn validate_integrity(
    target: &str,
    integrity: &EmbeddedPluginIntegrity,
) -> Result<(), EmbeddedPluginRegistryError> {
    match integrity {
        EmbeddedPluginIntegrity::Sha256 { digest } if target.contains("android") => {
            validate_sha256("integrity.digest", digest)
        }
        EmbeddedPluginIntegrity::AppleCodeSignature {
            validation: EmbeddedAppleCodeSignatureValidation::SameTeamAsHostOrSimulatorAdHoc,
        } if target.contains("apple-ios") => Ok(()),
        EmbeddedPluginIntegrity::Sha256 { .. } => Err(EmbeddedPluginRegistryError::InvalidField {
            field: "integrity.kind".to_owned(),
            message: format!("sha256 is incompatible with target `{target}`"),
        }),
        EmbeddedPluginIntegrity::AppleCodeSignature { .. } => {
            Err(EmbeddedPluginRegistryError::InvalidField {
                field: "integrity.kind".to_owned(),
                message: format!("apple-code-signature is incompatible with target `{target}`"),
            })
        }
    }
}

fn validate_package(package: &EmbeddedPluginPackage) -> Result<(), EmbeddedPluginRegistryError> {
    Version::parse(&package.version).map_err(|error| {
        EmbeddedPluginRegistryError::InvalidField {
            field: "package.version".to_owned(),
            message: error.to_string(),
        }
    })?;
    PluginReference::new(&package.publisher, None, PluginTransport::Native)
        .map_err(|error| invalid_identity("package.publisher", &package.publisher, error))?;
    validate_sha256("package.descriptor_sha256", &package.descriptor_sha256)
}

fn validate_capabilities(
    plugin_id: &str,
    capabilities: &[EmbeddedPluginCapability],
) -> Result<(), EmbeddedPluginRegistryError> {
    if capabilities.is_empty() || capabilities.len() > MAX_EMBEDDED_PLUGIN_CAPABILITIES_PER_ARTIFACT
    {
        return Err(EmbeddedPluginRegistryError::InvalidField {
            field: "capabilities".to_owned(),
            message: format!(
                "must contain 1 to {MAX_EMBEDDED_PLUGIN_CAPABILITIES_PER_ARTIFACT} entries"
            ),
        });
    }
    let mut identities = HashSet::with_capacity(capabilities.len());
    for capability in capabilities {
        parse_canonical_uuid("capabilities.interface_id", &capability.interface_id)?;
        PluginReference::new(
            plugin_id,
            Some(capability.instance_id.clone()),
            PluginTransport::Native,
        )
        .map_err(|error| {
            invalid_identity("capabilities.instance_id", &capability.instance_id, error)
        })?;
        if capability.interface_major == 0 {
            return Err(EmbeddedPluginRegistryError::InvalidField {
                field: "capabilities.interface_major".to_owned(),
                message: "must be greater than zero".to_owned(),
            });
        }
        if !identities.insert((&capability.interface_id, &capability.instance_id)) {
            return Err(EmbeddedPluginRegistryError::DuplicateCapability {
                plugin_id: plugin_id.to_owned(),
                interface_id: capability.interface_id.clone(),
                instance_id: capability.instance_id.clone(),
            });
        }
    }
    Ok(())
}

fn validate_text(
    field: &str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), EmbeddedPluginRegistryError> {
    if value.is_empty() || value.len() > maximum_bytes {
        return Err(EmbeddedPluginRegistryError::InvalidField {
            field: field.to_owned(),
            message: format!("must be 1 to {maximum_bytes} bytes"),
        });
    }
    Ok(())
}

fn validate_sha256(field: &str, value: &str) -> Result<(), EmbeddedPluginRegistryError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(EmbeddedPluginRegistryError::InvalidField {
            field: field.to_owned(),
            message: "must be 64 lowercase hexadecimal characters".to_owned(),
        });
    }
    Ok(())
}

fn parse_canonical_uuid(field: &str, value: &str) -> Result<Uuid, EmbeddedPluginRegistryError> {
    let parsed =
        Uuid::parse_str(value).map_err(|error| EmbeddedPluginRegistryError::InvalidField {
            field: field.to_owned(),
            message: error.to_string(),
        })?;
    if parsed.hyphenated().to_string() != value {
        return Err(EmbeddedPluginRegistryError::InvalidField {
            field: field.to_owned(),
            message: "must use canonical lowercase hyphenated UUID form".to_owned(),
        });
    }
    Ok(parsed)
}

fn invalid_identity(
    field: &str,
    value: &str,
    source: PluginReferenceError,
) -> EmbeddedPluginRegistryError {
    EmbeddedPluginRegistryError::InvalidField {
        field: field.to_owned(),
        message: format!("`{value}`: {source}"),
    }
}

/// Resolves an Android native library without assuming APK extraction.
///
/// Extracted libraries use a regular filesystem path. Otherwise this returns
/// Android's supported `apk!/lib/<abi>/<name>` linker path after confirming
/// that the package contains one uncompressed, bounded entry.
pub fn resolve_android_native_library(
    native_library_dir: &Path,
    package_paths: &[PathBuf],
    architecture: &str,
    library_name: &str,
) -> Result<PathBuf, String> {
    if package_paths.len() > MAX_ANDROID_PACKAGE_PATHS {
        return Err(format!(
            "Android package path count {} exceeds maximum {}",
            package_paths.len(),
            MAX_ANDROID_PACKAGE_PATHS,
        ));
    }
    if !valid_android_path_component(architecture) {
        return Err(format!(
            "invalid Android plugin architecture `{architecture}`"
        ));
    }
    if !valid_android_path_component(library_name) {
        return Err(format!(
            "invalid Android plugin library name `{library_name}`"
        ));
    }

    let file_name = format!("lib{library_name}.so");
    if !native_library_dir.as_os_str().is_empty() {
        let extracted_path = native_library_dir.join(&file_name);
        if extracted_path.is_file() {
            return Ok(extracted_path);
        }
    }

    let entry_name = format!("lib/{architecture}/{file_name}");
    for package_path in package_paths {
        if !package_path.is_file() {
            continue;
        }
        let package_file = File::open(package_path).map_err(|error| {
            format!(
                "failed to open Android package `{}`: {error}",
                package_path.display()
            )
        })?;
        let mut archive = ZipArchive::new(package_file).map_err(|error| {
            format!(
                "failed to inspect Android package `{}`: {error}",
                package_path.display()
            )
        })?;
        let matching_entries = android_central_directory_entry_count(
            package_path,
            archive.central_directory_start(),
            &entry_name,
        )
        .map_err(|error| {
            format!(
                "failed to inspect Android package `{}` central directory: {error}",
                package_path.display()
            )
        })?;
        if matching_entries > 1 {
            return Err(format!(
                "Android package `{}` contains duplicate native library entry `{entry_name}`",
                package_path.display()
            ));
        }
        if matching_entries == 0 {
            continue;
        }
        match archive.by_name(&entry_name) {
            Ok(entry) => {
                if entry.is_dir() {
                    return Err(format!(
                        "Android package entry `{entry_name}` is not a native library"
                    ));
                }
                if entry.size() > MAX_EMBEDDED_PLUGIN_ARCHIVE_ARTIFACT_BYTES {
                    return Err(format!(
                        "Android package entry `{entry_name}` is {} bytes; maximum is {} bytes",
                        entry.size(),
                        MAX_EMBEDDED_PLUGIN_ARCHIVE_ARTIFACT_BYTES,
                    ));
                }
                if entry.compression() != CompressionMethod::Stored {
                    return Err(format!(
                        "Android package entry `{entry_name}` is compressed and cannot be loaded in place"
                    ));
                }
                let package_path = package_path.to_str().ok_or_else(|| {
                    "Android package path must be valid UTF-8 for linker loading".to_owned()
                })?;
                return Ok(PathBuf::from(format!("{package_path}!/{entry_name}")));
            }
            Err(ZipError::FileNotFound) => {}
            Err(error) => {
                return Err(format!(
                    "failed to inspect Android package entry `{entry_name}` in `{}`: {error}",
                    package_path.display(),
                ));
            }
        }
    }

    Err(format!(
        "Android plugin library `{file_name}` was not found as an extracted file or package entry"
    ))
}

fn valid_android_path_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn android_central_directory_entry_count(
    package_path: &Path,
    central_directory_start: u64,
    entry_name: &str,
) -> io::Result<usize> {
    const CENTRAL_DIRECTORY_HEADER: u32 = 0x0201_4b50;
    const CENTRAL_DIRECTORY_DIGITAL_SIGNATURE: u32 = 0x0505_4b50;
    const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
    const ZIP64_END_OF_CENTRAL_DIRECTORY: u32 = 0x0606_4b50;
    const ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR: u32 = 0x0706_4b50;

    let mut reader = File::open(package_path)?;
    reader.seek(SeekFrom::Start(central_directory_start))?;
    let mut matches = 0_usize;
    loop {
        let mut signature = [0_u8; 4];
        reader.read_exact(&mut signature)?;
        match u32::from_le_bytes(signature) {
            CENTRAL_DIRECTORY_HEADER => {
                let mut fixed_fields = [0_u8; 42];
                reader.read_exact(&mut fixed_fields)?;
                let file_name_length =
                    u16::from_le_bytes([fixed_fields[24], fixed_fields[25]]) as usize;
                let extra_field_length =
                    u16::from_le_bytes([fixed_fields[26], fixed_fields[27]]) as u64;
                let comment_length =
                    u16::from_le_bytes([fixed_fields[28], fixed_fields[29]]) as u64;
                let mut file_name = vec![0_u8; file_name_length];
                reader.read_exact(&mut file_name)?;
                if file_name == entry_name.as_bytes() {
                    matches = matches.saturating_add(1);
                    if matches > 1 {
                        return Ok(matches);
                    }
                }
                let trailing_bytes =
                    extra_field_length
                        .checked_add(comment_length)
                        .ok_or_else(|| {
                            io::Error::new(io::ErrorKind::InvalidData, "ZIP entry length overflow")
                        })?;
                reader.seek(SeekFrom::Current(i64::try_from(trailing_bytes).map_err(
                    |_| io::Error::new(io::ErrorKind::InvalidData, "ZIP entry is too large"),
                )?))?;
            }
            CENTRAL_DIRECTORY_DIGITAL_SIGNATURE
            | END_OF_CENTRAL_DIRECTORY
            | ZIP64_END_OF_CENTRAL_DIRECTORY
            | ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR => return Ok(matches),
            signature => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected ZIP central directory signature {signature:#010x}"),
                ));
            }
        }
    }
}

fn verify_sha256(path: &Path, expected: &str) -> Result<(), EmbeddedPluginRegistryError> {
    if let Some((package_path, entry_name)) = android_package_entry(path) {
        return verify_android_package_entry_sha256(path, &package_path, entry_name, expected);
    }
    if !path.is_file() {
        return Err(EmbeddedPluginRegistryError::ArtifactNotFile {
            path: path.display().to_string(),
        });
    }
    let mut file =
        File::open(path).map_err(|source| EmbeddedPluginRegistryError::ReadArtifact {
            path: path.display().to_string(),
            source,
        })?;
    let metadata = file
        .metadata()
        .map_err(|source| EmbeddedPluginRegistryError::ReadArtifact {
            path: path.display().to_string(),
            source,
        })?;
    if metadata.len() > MAX_EMBEDDED_PLUGIN_ARCHIVE_ARTIFACT_BYTES {
        return Err(EmbeddedPluginRegistryError::OversizedArtifact {
            path: path.display().to_string(),
            actual_bytes: metadata.len(),
            maximum_bytes: MAX_EMBEDDED_PLUGIN_ARCHIVE_ARTIFACT_BYTES,
        });
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total_bytes = 0_u64;
    loop {
        let read =
            file.read(&mut buffer)
                .map_err(|source| EmbeddedPluginRegistryError::ReadArtifact {
                    path: path.display().to_string(),
                    source,
                })?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes.saturating_add(read as u64);
        if total_bytes > MAX_EMBEDDED_PLUGIN_ARCHIVE_ARTIFACT_BYTES {
            return Err(EmbeddedPluginRegistryError::OversizedArtifact {
                path: path.display().to_string(),
                actual_bytes: total_bytes,
                maximum_bytes: MAX_EMBEDDED_PLUGIN_ARCHIVE_ARTIFACT_BYTES,
            });
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(EmbeddedPluginRegistryError::ChecksumMismatch {
            path: path.display().to_string(),
            expected: expected.to_owned(),
            actual,
        });
    }
    Ok(())
}

fn android_package_entry(path: &Path) -> Option<(PathBuf, &str)> {
    let value = path.to_str()?;
    let (package_path, entry_name) = value.split_once("!/")?;
    let mut components = entry_name.split('/');
    if package_path.is_empty()
        || components.next() != Some("lib")
        || components.any(|component| component.is_empty() || matches!(component, "." | ".."))
        || entry_name.contains('\\')
    {
        return None;
    }
    Some((PathBuf::from(package_path), entry_name))
}

fn verify_android_package_entry_sha256(
    load_path: &Path,
    package_path: &Path,
    entry_name: &str,
    expected: &str,
) -> Result<(), EmbeddedPluginRegistryError> {
    let package_file =
        File::open(package_path).map_err(|source| EmbeddedPluginRegistryError::ReadArtifact {
            path: load_path.display().to_string(),
            source,
        })?;
    let mut archive = ZipArchive::new(package_file).map_err(|error| {
        EmbeddedPluginRegistryError::ReadArtifact {
            path: load_path.display().to_string(),
            source: io::Error::other(error),
        }
    })?;
    let matching_entries = android_central_directory_entry_count(
        package_path,
        archive.central_directory_start(),
        entry_name,
    )
    .map_err(|source| EmbeddedPluginRegistryError::ReadArtifact {
        path: load_path.display().to_string(),
        source,
    })?;
    if matching_entries > 1 {
        return Err(EmbeddedPluginRegistryError::DuplicateAndroidPackageEntry {
            path: package_path.display().to_string(),
            entry: entry_name.to_owned(),
        });
    }
    let mut entry =
        archive
            .by_name(entry_name)
            .map_err(|error| EmbeddedPluginRegistryError::ReadArtifact {
                path: load_path.display().to_string(),
                source: io::Error::other(error),
            })?;
    if entry.is_dir() || entry.compression() != CompressionMethod::Stored {
        return Err(EmbeddedPluginRegistryError::ArtifactNotFile {
            path: load_path.display().to_string(),
        });
    }
    if entry.size() > MAX_EMBEDDED_PLUGIN_ARCHIVE_ARTIFACT_BYTES {
        return Err(EmbeddedPluginRegistryError::OversizedArtifact {
            path: load_path.display().to_string(),
            actual_bytes: entry.size(),
            maximum_bytes: MAX_EMBEDDED_PLUGIN_ARCHIVE_ARTIFACT_BYTES,
        });
    }

    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total_bytes = 0_u64;
    loop {
        let read = entry.read(&mut buffer).map_err(|source| {
            EmbeddedPluginRegistryError::ReadArtifact {
                path: load_path.display().to_string(),
                source,
            }
        })?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes.saturating_add(read as u64);
        if total_bytes > MAX_EMBEDDED_PLUGIN_ARCHIVE_ARTIFACT_BYTES {
            return Err(EmbeddedPluginRegistryError::ArtifactNotFile {
                path: load_path.display().to_string(),
            });
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(EmbeddedPluginRegistryError::ChecksumMismatch {
            path: load_path.display().to_string(),
            expected: expected.to_owned(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};

    #[test]
    fn android_apk_locator_hashes_the_exact_uncompressed_library_entry() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let apk_path = temp.path().join("base.apk");
        let library_bytes = b"packaged native plugin";
        let mut archive = ZipWriter::new(File::create(&apk_path).expect("create package"));
        archive
            .start_file(
                "lib/arm64-v8a/libvesper..fixture.so",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .expect("start library entry");
        archive
            .write_all(library_bytes)
            .expect("write library entry");
        archive.finish().expect("finish package");

        let load_path = resolve_android_native_library(
            &temp.path().join("unextracted"),
            std::slice::from_ref(&apk_path),
            "arm64-v8a",
            "vesper..fixture",
        )
        .expect("resolve package library");
        assert!(
            load_path
                .to_string_lossy()
                .contains("base.apk!/lib/arm64-v8a/")
        );

        let expected = format!("{:x}", Sha256::digest(library_bytes));
        verify_sha256(&load_path, &expected).expect("verify package entry");
        assert!(matches!(
            verify_sha256(&load_path, &"0".repeat(64)),
            Err(EmbeddedPluginRegistryError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn android_artifact_limits_and_duplicate_entries_apply_to_every_storage_form() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let oversized_library = temp.path().join("liboversized.so");
        let file = File::create(&oversized_library).expect("create sparse library");
        file.set_len(MAX_EMBEDDED_PLUGIN_ARCHIVE_ARTIFACT_BYTES + 1)
            .expect("size sparse library");
        assert!(matches!(
            verify_sha256(&oversized_library, &"0".repeat(64)),
            Err(EmbeddedPluginRegistryError::OversizedArtifact { .. })
        ));

        let apk_path = temp.path().join("duplicate.apk");
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, bytes) in [
            ("lib/arm64-v8a/libduplicate-a.so", b"first".as_slice()),
            ("lib/arm64-v8a/libduplicate-b.so", b"second".as_slice()),
        ] {
            archive
                .start_file(name, options)
                .expect("start library entry");
            archive.write_all(bytes).expect("write duplicate entry");
        }
        let mut archive_bytes = archive
            .finish()
            .expect("finish duplicate package")
            .into_inner();
        let original_name = b"lib/arm64-v8a/libduplicate-b.so";
        let duplicate_name = b"lib/arm64-v8a/libduplicate-a.so";
        let mut replacement_count = 0;
        for offset in 0..=archive_bytes.len() - original_name.len() {
            if &archive_bytes[offset..offset + original_name.len()] == original_name {
                archive_bytes[offset..offset + duplicate_name.len()]
                    .copy_from_slice(duplicate_name);
                replacement_count += 1;
            }
        }
        assert_eq!(replacement_count, 2, "local and central ZIP names");
        std::fs::write(&apk_path, archive_bytes).expect("write duplicate package");

        let error = resolve_android_native_library(
            Path::new(""),
            std::slice::from_ref(&apk_path),
            "arm64-v8a",
            "duplicate-a",
        )
        .expect_err("duplicate package entry");
        assert!(error.contains("duplicate native library entry"));
    }

    fn registry_json(artifact: &str) -> Vec<u8> {
        format!(
            r#"{{
                "schema_version": 1,
                "target": "aarch64-linux-android",
                "architecture": "arm64-v8a",
                "minimum_os": "26",
                "artifacts": [{artifact}]
            }}"#
        )
        .into_bytes()
    }

    fn artifact_json(plugin_id: &str, transport: &str, capabilities: &str) -> String {
        format!(
            r#"{{
                "plugin_id": "{plugin_id}",
                "transport": "{transport}",
                "locator": {{
                    "kind": "android-native-library",
                    "name": "vesper_fixture"
                }},
                "integrity": {{
                    "kind": "sha256",
                    "digest": "{digest}"
                }},
                "package": {{
                    "version": "1.2.3",
                    "publisher": "dev.vesper.publisher",
                    "descriptor_sha256": "{digest}"
                }},
                "capabilities": [{capabilities}]
            }}"#,
            digest = "0".repeat(64),
        )
    }

    fn capability_json(instance_id: &str) -> String {
        format!(
            r#"{{
                "interface_id": "e9479dbc-42d2-575e-b39e-a24bc512fbc7",
                "instance_id": "{instance_id}",
                "interface_major": 1,
                "interface_minor": 0
            }}"#
        )
    }

    fn apple_registry_json() -> Vec<u8> {
        let capability = capability_json("dev.vesper.fixture.post-download");
        let artifact = artifact_json("dev.vesper.fixture", "native", &capability);
        String::from_utf8(registry_json(&artifact))
            .expect("fixture JSON")
            .replace("aarch64-linux-android", "aarch64-apple-ios")
            .replace("arm64-v8a", "arm64")
            .replace("\"minimum_os\": \"26\"", "\"minimum_os\": \"17.0\"")
            .replace(
                r#""kind": "android-native-library",
                    "name": "vesper_fixture""#,
                r#""kind": "apple-framework",
                    "name": "VesperPluginFixture",
                    "bundle_identifier": "dev.vesper.plugin-fixture""#,
            )
            .replace(
                r#""kind": "sha256",
                    "digest": "0000000000000000000000000000000000000000000000000000000000000000""#,
                r#""kind": "apple-code-signature",
                    "validation": "same-team-as-host-or-simulator-ad-hoc""#,
            )
            .into_bytes()
    }

    #[test]
    fn registry_parse_preserves_valid_identity_and_target() {
        let capability = capability_json("dev.vesper.fixture.post-download");
        let artifact = artifact_json("dev.vesper.fixture", "native", &capability);
        let registry = EmbeddedPluginRegistry::parse(
            &registry_json(&artifact),
            "aarch64-linux-android",
            "arm64-v8a",
        )
        .expect("valid registry");

        assert_eq!(registry.minimum_os(), Some("26"));
        assert_eq!(registry.artifacts()[0].plugin_id(), "dev.vesper.fixture");
        assert_eq!(registry.artifacts()[0].locator().name(), "vesper_fixture");
    }

    #[test]
    fn registry_requires_minimum_os_only_when_artifacts_are_present() {
        let capability = capability_json("dev.vesper.fixture.post-download");
        let artifact = artifact_json("dev.vesper.fixture", "native", &capability);
        let without_minimum_os = String::from_utf8(registry_json(&artifact))
            .expect("fixture JSON")
            .replace("\n                \"minimum_os\": \"26\",", "");

        let error = EmbeddedPluginRegistry::parse(
            without_minimum_os.as_bytes(),
            "aarch64-linux-android",
            "arm64-v8a",
        )
        .expect_err("artifacts require a minimum OS");
        assert!(matches!(
            error,
            EmbeddedPluginRegistryError::InvalidField { ref field, .. }
                if field == "minimum_os"
        ));

        let empty_without_minimum_os = br#"{
            "schema_version": 1,
            "target": "aarch64-linux-android",
            "architecture": "arm64-v8a",
            "artifacts": []
        }"#;
        let registry = EmbeddedPluginRegistry::parse(
            empty_without_minimum_os,
            "aarch64-linux-android",
            "arm64-v8a",
        )
        .expect("empty no-plugin registry may omit minimum_os");
        assert_eq!(registry.minimum_os(), None);
    }

    #[test]
    fn empty_fragments_do_not_define_combined_minimum_os() {
        let empty = br#"{
            "schema_version": 1,
            "target": "aarch64-linux-android",
            "architecture": "arm64-v8a",
            "artifacts": []
        }"#;
        let capability = capability_json("dev.vesper.fixture.post-download");
        let artifact = artifact_json("dev.vesper.fixture", "native", &capability);
        let populated = registry_json(&artifact);

        let registry = EmbeddedPluginRegistry::parse_fragments(
            [empty.as_slice(), populated.as_slice()],
            "aarch64-linux-android",
            "arm64-v8a",
        )
        .expect("empty fragment must not conflict with populated platform metadata");

        assert_eq!(registry.artifacts().len(), 1);
        assert_eq!(registry.minimum_os(), Some("26"));
    }

    #[test]
    fn public_registry_schema_requires_minimum_os_for_artifacts() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .expect("canonical loader crate path");
        let workspace = manifest_dir.join("../../..");
        // The public schema belongs to the workspace distribution, outside the
        // Rust crate package. Only run this drift check from a source checkout.
        let Ok(workspace_member) = workspace
            .join("crates/plugin/player-plugin-loader")
            .canonicalize()
        else {
            return;
        };
        if manifest_dir != workspace_member {
            return;
        }
        let relative_path = "schemas/vesper-plugin/embedded-registry.schema.json";
        let bytes = std::fs::read(workspace.join(relative_path)).expect("read registry schema");
        let schema: serde_json::Value =
            serde_json::from_slice(&bytes).expect("parse registry schema");
        let conditional = &schema["allOf"][0];

        assert_eq!(
            conditional["if"]["properties"]["artifacts"]["minItems"], 1,
            "{relative_path}"
        );
        assert!(
            conditional["then"]["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|field| field == "minimum_os")),
            "{relative_path}"
        );
    }

    #[test]
    fn registry_rejects_unknown_mobile_transport_without_fallback() {
        let capability = capability_json("dev.vesper.fixture.post-download");
        let artifact = artifact_json("dev.vesper.fixture", "wasm", &capability);
        let error = EmbeddedPluginRegistry::parse(
            &registry_json(&artifact),
            "aarch64-linux-android",
            "arm64-v8a",
        )
        .expect_err("mobile WASM must be rejected");
        assert!(matches!(
            error,
            EmbeddedPluginRegistryError::UnsupportedTransport {
                transport: PluginTransport::Wasm,
                ..
            }
        ));
    }

    #[test]
    fn registry_rejects_duplicate_capability_and_lossy_identity_forms() {
        let capability = capability_json("dev.vesper.fixture.post-download");
        let duplicate_capabilities = format!("{capability},{capability}");
        let duplicate = artifact_json("dev.vesper.fixture", "native", &duplicate_capabilities);
        assert!(matches!(
            EmbeddedPluginRegistry::parse(
                &registry_json(&duplicate),
                "aarch64-linux-android",
                "arm64-v8a",
            ),
            Err(EmbeddedPluginRegistryError::DuplicateCapability { .. })
        ));

        let invalid = artifact_json(" Dev.Vesper.Fixture ", "native", &capability);
        assert!(matches!(
            EmbeddedPluginRegistry::parse(
                &registry_json(&invalid),
                "aarch64-linux-android",
                "arm64-v8a",
            ),
            Err(EmbeddedPluginRegistryError::InvalidField { ref field, .. })
                if field == "plugin_id"
        ));
    }

    #[test]
    fn registry_rejects_target_architecture_and_locator_mismatch() {
        let capability = capability_json("dev.vesper.fixture.post-download");
        let artifact = artifact_json("dev.vesper.fixture", "native", &capability);
        let json = registry_json(&artifact);
        assert!(matches!(
            EmbeddedPluginRegistry::parse(&json, "aarch64-apple-ios", "arm64"),
            Err(EmbeddedPluginRegistryError::TargetMismatch { .. })
        ));
        assert!(matches!(
            EmbeddedPluginRegistry::parse(&json, "aarch64-linux-android", "x86_64",),
            Err(EmbeddedPluginRegistryError::ArchitectureMismatch { .. })
        ));
    }

    #[test]
    fn registry_fragments_merge_without_implicit_identity_changes() {
        let first_capability = capability_json("dev.vesper.fixture.post-download");
        let first_artifact = artifact_json("dev.vesper.fixture", "native", &first_capability);
        let second_capability = capability_json("dev.vesper.other.post-download");
        let second_artifact = artifact_json("dev.vesper.other", "native", &second_capability)
            .replace("vesper_fixture", "vesper_other");
        let first = registry_json(&first_artifact);
        let second = registry_json(&second_artifact);

        let registry = EmbeddedPluginRegistry::parse_fragments(
            [first.as_slice(), second.as_slice()],
            "aarch64-linux-android",
            "arm64-v8a",
        )
        .expect("valid fragments");

        assert_eq!(registry.artifacts().len(), 2);
        assert_eq!(registry.minimum_os(), Some("26"));
        assert_eq!(registry.artifacts()[0].plugin_id(), "dev.vesper.fixture");
        assert_eq!(registry.artifacts()[1].plugin_id(), "dev.vesper.other");
    }

    #[test]
    fn registry_fragments_reject_cross_package_duplicates_and_metadata_drift() {
        let capability = capability_json("dev.vesper.fixture.post-download");
        let artifact = artifact_json("dev.vesper.fixture", "native", &capability);
        let first = registry_json(&artifact);
        let duplicate = registry_json(&artifact);
        assert!(matches!(
            EmbeddedPluginRegistry::parse_fragments(
                [first.as_slice(), duplicate.as_slice()],
                "aarch64-linux-android",
                "arm64-v8a",
            ),
            Err(EmbeddedPluginRegistryError::DuplicatePluginId(ref plugin_id))
                if plugin_id == "dev.vesper.fixture"
        ));

        let different_minimum_os = String::from_utf8(registry_json(&artifact))
            .expect("fixture JSON")
            .replace("\"minimum_os\": \"26\"", "\"minimum_os\": \"27\"")
            .into_bytes();
        assert!(matches!(
            EmbeddedPluginRegistry::parse_fragments(
                [first.as_slice(), different_minimum_os.as_slice()],
                "aarch64-linux-android",
                "arm64-v8a",
            ),
            Err(EmbeddedPluginRegistryError::MinimumOsMismatch { .. })
        ));
    }

    #[test]
    fn empty_registry_fragment_set_is_a_valid_no_plugin_baseline() {
        let fragments: [&[u8]; 0] = [];
        let registry = EmbeddedPluginRegistry::parse_fragments(
            fragments,
            "aarch64-linux-android",
            "arm64-v8a",
        )
        .expect("empty registry");

        assert!(registry.artifacts().is_empty());
        assert_eq!(registry.minimum_os(), None);
    }

    #[test]
    fn runtime_parser_rejects_fields_forbidden_by_the_schema() {
        let capability = capability_json("dev.vesper.fixture.post-download");
        let artifact = artifact_json("dev.vesper.fixture", "native", &capability);
        let json = String::from_utf8(registry_json(&artifact))
            .expect("fixture JSON")
            .replace(
                "\"schema_version\": 1",
                "\"schema_version\": 1, \"unexpected\": true",
            );

        let error =
            EmbeddedPluginRegistry::parse(json.as_bytes(), "aarch64-linux-android", "arm64-v8a")
                .expect_err("unknown root fields must be rejected");

        assert!(matches!(error, EmbeddedPluginRegistryError::Json(_)));
        assert!(error.to_string().contains("unknown field `unexpected`"));
    }

    #[test]
    fn apple_registry_requires_host_code_signature_verification() {
        let registry =
            EmbeddedPluginRegistry::parse(&apple_registry_json(), "aarch64-apple-ios", "arm64")
                .expect("valid Apple registry");
        let artifact = &registry.artifacts()[0];
        assert_eq!(
            artifact.locator().apple_bundle_identifier(),
            Some("dev.vesper.plugin-fixture")
        );
        assert_eq!(
            artifact.integrity().apple_code_signature_validation(),
            Some(EmbeddedAppleCodeSignatureValidation::SameTeamAsHostOrSimulatorAdHoc)
        );

        let executable = std::env::current_exe().expect("test executable");
        let error = registry
            .load_native(|_| Ok(executable.clone()))
            .expect_err("Apple integrity cannot be skipped");
        assert!(matches!(
            error,
            EmbeddedPluginRegistryError::PlatformIntegrityVerificationRequired {
                ref plugin_id
            } if plugin_id == "dev.vesper.fixture"
        ));

        let error = registry
            .load_native_with_platform_integrity(
                |_| Ok(executable.clone()),
                |path, artifact| {
                    assert_eq!(path, executable);
                    assert_eq!(
                        artifact.locator().apple_bundle_identifier(),
                        Some("dev.vesper.plugin-fixture")
                    );
                    Err("code signature team mismatch".to_owned())
                },
            )
            .expect_err("failed code signature verification must stop before dlopen");
        assert!(matches!(
            error,
            EmbeddedPluginRegistryError::PlatformIntegrityVerification { ref message, .. }
                if message == "code signature team mismatch"
        ));
    }

    #[test]
    fn registry_rejects_target_incompatible_integrity() {
        let apple_with_sha256 = String::from_utf8(apple_registry_json())
            .expect("fixture JSON")
            .replace(
                r#""kind": "apple-code-signature",
                    "validation": "same-team-as-host-or-simulator-ad-hoc""#,
                &format!(
                    "\"kind\": \"sha256\",\n                    \"digest\": \"{}\"",
                    "0".repeat(64)
                ),
            );
        assert!(matches!(
            EmbeddedPluginRegistry::parse(
                apple_with_sha256.as_bytes(),
                "aarch64-apple-ios",
                "arm64",
            ),
            Err(EmbeddedPluginRegistryError::InvalidField { ref field, .. })
                if field == "integrity.kind"
        ));
    }

    #[test]
    fn selected_loading_rejects_unlisted_and_mobile_wasm_references_before_resolution() {
        let fragments: [&[u8]; 0] = [];
        let registry = EmbeddedPluginRegistry::parse_fragments(
            fragments,
            "aarch64-linux-android",
            "arm64-v8a",
        )
        .expect("empty registry");
        let missing = PluginReference::new("dev.vesper.missing", None, PluginTransport::Native)
            .expect("valid reference");
        let mut resolver_called = false;
        let error = registry
            .load_native_selected([&missing], |_| {
                resolver_called = true;
                Err("must not resolve".to_owned())
            })
            .expect_err("missing plugin must fail");
        assert!(matches!(
            error,
            EmbeddedPluginRegistryError::UnknownPluginReference(ref plugin_id)
                if plugin_id == "dev.vesper.missing"
        ));
        assert!(!resolver_called);

        let wasm = PluginReference::new("dev.vesper.wasm", None, PluginTransport::Wasm)
            .expect("valid reference");
        let error = registry
            .load_native_selected([&wasm], |_| Err("must not resolve".to_owned()))
            .expect_err("mobile WASM must fail");
        assert!(matches!(
            error,
            EmbeddedPluginRegistryError::UnsupportedTransport {
                transport: PluginTransport::Wasm,
                ..
            }
        ));
    }

    #[test]
    fn metadata_selection_is_explicit_deduplicated_and_does_not_resolve_paths() {
        let capability = capability_json("dev.vesper.fixture.post-download");
        let artifact = artifact_json("dev.vesper.fixture", "native", &capability);
        let registry = EmbeddedPluginRegistry::parse(
            &registry_json(&artifact),
            "aarch64-linux-android",
            "arm64-v8a",
        )
        .expect("valid registry");
        let first = PluginReference::new(
            "dev.vesper.fixture",
            Some("dev.vesper.fixture.post-download".to_owned()),
            PluginTransport::Native,
        )
        .expect("first reference");
        let second = PluginReference::new(
            "dev.vesper.fixture",
            None::<String>,
            PluginTransport::Native,
        )
        .expect("second reference");

        let selected = registry
            .select_native_artifacts([&first, &second])
            .expect("selected metadata");

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].plugin_id(), "dev.vesper.fixture");
        assert_eq!(selected[0].locator().name(), "vesper_fixture");
    }

    #[test]
    fn empty_selection_does_not_resolve_or_load_packaged_artifacts() {
        let capability = capability_json("dev.vesper.fixture.post-download");
        let artifact = artifact_json("dev.vesper.fixture", "native", &capability);
        let registry = EmbeddedPluginRegistry::parse(
            &registry_json(&artifact),
            "aarch64-linux-android",
            "arm64-v8a",
        )
        .expect("valid registry");
        let references: [&PluginReference; 0] = [];
        let mut resolver_called = false;

        let loaded = registry
            .load_native_selected(references, |_| {
                resolver_called = true;
                Err("must not resolve".to_owned())
            })
            .expect("empty selection");

        assert!(!resolver_called);
        assert!(loaded.registered_interfaces().is_empty());
    }
}
