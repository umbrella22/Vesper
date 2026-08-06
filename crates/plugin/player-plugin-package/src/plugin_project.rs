use std::collections::HashSet;
use std::path::{Path, PathBuf};

use player_plugin::{PluginReference, PluginTransport};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

use crate::{
    PluginCapabilityDescriptor, PluginCompatibilityDescriptor, PluginDescriptor,
    PluginDescriptorError, PluginIdentityDescriptor, PluginRedistributionDescriptor,
};

pub(crate) const MAX_ARTIFACTS: usize = 32;
const MAX_PACKAGE_FILES: usize = 128;
pub(crate) const MAX_RUNTIME_DEPENDENCIES: usize = 32;
pub(crate) const MAX_ARCHIVE_PATH_BYTES: usize = 512;
pub(crate) const MAX_TARGET_BYTES: usize = 128;
pub(crate) const MAX_ARCHITECTURE_BYTES: usize = 64;
pub(crate) const MAX_MINIMUM_OS_BYTES: usize = 64;
pub(crate) const MAX_RUNTIME_VALUE_BYTES: usize = 256;

/// Complete author-owned source manifest read from vesper-plugin.toml.
///
/// Descriptor and capability fields remain the single source for runtime
/// registry metadata. Artifact hashes are generated from the declared source
/// files and are never accepted from author input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginProjectManifest {
    descriptor: PluginDescriptor,
    artifacts: Vec<PluginArtifactSource>,
    package_files: Vec<PluginPackageFileSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginProjectManifestWire {
    schema_version: u32,
    plugin: PluginIdentityDescriptor,
    compatibility: PluginCompatibilityDescriptor,
    capabilities: Vec<PluginCapabilityDescriptor>,
    #[serde(default)]
    redistribution: Vec<PluginRedistributionDescriptor>,
    #[serde(default)]
    artifacts: Vec<PluginArtifactSource>,
    #[serde(default)]
    package_files: Vec<PluginPackageFileSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginArtifactTransport {
    Native,
    Wasm,
}

impl PluginArtifactTransport {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Wasm => "wasm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginArtifactFormat {
    Dylib,
    Aar,
    Xcframework,
    WasmComponent,
}

impl PluginArtifactFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dylib => "dylib",
            Self::Aar => "aar",
            Self::Xcframework => "xcframework",
            Self::WasmComponent => "wasm-component",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginRuntimeLinkage {
    Dynamic,
    Static,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRuntimeDependencySource {
    pub id: String,
    pub version: String,
    pub linkage: PluginRuntimeLinkage,
    pub compatibility_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginArtifactCapability {
    pub interface_id: String,
    pub instance_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginArtifactSource {
    pub transport: PluginArtifactTransport,
    pub target: String,
    pub format: PluginArtifactFormat,
    pub source: PathBuf,
    pub path: String,
    pub architecture: String,
    pub capabilities: Vec<PluginArtifactCapability>,
    #[serde(default)]
    pub minimum_os: Option<String>,
    #[serde(default)]
    pub runtime_dependencies: Vec<PluginRuntimeDependencySource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginPackageFileKind {
    License,
    Notice,
    RuntimeMetadata,
    Redistribution,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPackageFileSource {
    pub source: PathBuf,
    pub path: String,
    pub kind: PluginPackageFileKind,
}

#[derive(Debug, Error)]
pub enum PluginProjectManifestError {
    #[error("invalid vesper-plugin.toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error(transparent)]
    Descriptor(#[from] PluginDescriptorError),
    #[error("invalid plugin project field '{field}': {message}")]
    InvalidField { field: String, message: String },
    #[error("duplicate package path '{path}'")]
    DuplicatePackagePath { path: String },
    #[error("package file path '{path}' conflicts with '{conflicting_path}'")]
    ConflictingPackagePath {
        path: String,
        conflicting_path: String,
    },
    #[error(
        "ambiguous artifact target for transport '{transport}', target '{target}', and architecture '{architecture}'"
    )]
    AmbiguousArtifactTarget {
        transport: &'static str,
        target: String,
        architecture: String,
    },
}

impl PluginProjectManifest {
    pub fn from_toml(source: &str) -> Result<Self, PluginProjectManifestError> {
        let wire: PluginProjectManifestWire = toml::from_str(source)?;
        let descriptor = PluginDescriptor {
            schema_version: wire.schema_version,
            plugin: wire.plugin,
            compatibility: wire.compatibility,
            capabilities: wire.capabilities,
            redistribution: wire.redistribution,
        };
        descriptor.validate()?;
        let project = Self {
            descriptor,
            artifacts: wire.artifacts,
            package_files: wire.package_files,
        };
        project.validate_declared_inputs()?;
        Ok(project)
    }

    pub fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    pub fn artifacts(&self) -> &[PluginArtifactSource] {
        &self.artifacts
    }

    pub fn package_files(&self) -> &[PluginPackageFileSource] {
        &self.package_files
    }

    pub fn validate_package_inputs(&self) -> Result<(), PluginProjectManifestError> {
        self.validate_declared_inputs()?;
        if self.artifacts.is_empty() {
            return project_invalid("artifacts", "must contain at least one artifact");
        }
        if !self
            .package_files
            .iter()
            .any(|file| file.kind == PluginPackageFileKind::License)
        {
            return project_invalid("package_files", "must contain at least one license file");
        }
        if !self
            .package_files
            .iter()
            .any(|file| file.kind == PluginPackageFileKind::Notice)
        {
            return project_invalid("package_files", "must contain at least one notice file");
        }
        Ok(())
    }

    fn validate_declared_inputs(&self) -> Result<(), PluginProjectManifestError> {
        if self.artifacts.len() > MAX_ARTIFACTS {
            return project_invalid(
                "artifacts",
                format!("must contain at most {MAX_ARTIFACTS} entries"),
            );
        }
        if self.package_files.len() > MAX_PACKAGE_FILES {
            return project_invalid(
                "package_files",
                format!("must contain at most {MAX_PACKAGE_FILES} entries"),
            );
        }

        let mut package_paths =
            HashSet::with_capacity(self.artifacts.len() + self.package_files.len() + 4);
        for reserved_path in [
            crate::plugin_package::PLUGIN_PACKAGE_MANIFEST_PATH,
            crate::plugin_package::PLUGIN_PACKAGE_CHECKSUMS_PATH,
            crate::plugin_package::PLUGIN_PACKAGE_SIGNATURE_PATH,
            crate::plugin_package::INSTALL_MARKER_PATH,
        ] {
            package_paths.insert(normalized_package_path(reserved_path));
        }
        let mut selectors = HashSet::with_capacity(self.artifacts.len());
        let descriptor_capabilities = self
            .descriptor
            .capabilities
            .iter()
            .map(|capability| {
                (
                    capability.interface_id.as_str(),
                    capability.instance_id.as_str(),
                )
            })
            .collect::<HashSet<_>>();
        let mut covered_capabilities = HashSet::with_capacity(descriptor_capabilities.len());
        for artifact in &self.artifacts {
            validate_local_source("artifacts.source", &artifact.source)?;
            validate_archive_path("artifacts.path", &artifact.path)?;
            insert_archive_file_path(&mut package_paths, &artifact.path)?;
            validate_text("artifacts.target", &artifact.target, MAX_TARGET_BYTES)?;
            validate_text(
                "artifacts.architecture",
                &artifact.architecture,
                MAX_ARCHITECTURE_BYTES,
            )?;
            if let Some(minimum_os) = artifact.minimum_os.as_deref() {
                validate_text("artifacts.minimum_os", minimum_os, MAX_MINIMUM_OS_BYTES)?;
            }
            match (artifact.transport, artifact.format) {
                (PluginArtifactTransport::Wasm, PluginArtifactFormat::WasmComponent)
                | (PluginArtifactTransport::Native, PluginArtifactFormat::Dylib)
                | (PluginArtifactTransport::Native, PluginArtifactFormat::Aar)
                | (PluginArtifactTransport::Native, PluginArtifactFormat::Xcframework) => {}
                _ => {
                    return project_invalid(
                        "artifacts.format",
                        format!(
                            "format '{}' is incompatible with transport '{}'",
                            artifact.format.as_str(),
                            artifact.transport.as_str()
                        ),
                    );
                }
            }
            let selector = (
                artifact.transport,
                artifact.target.clone(),
                artifact.architecture.clone(),
            );
            if !selectors.insert(selector) {
                return Err(PluginProjectManifestError::AmbiguousArtifactTarget {
                    transport: artifact.transport.as_str(),
                    target: artifact.target.clone(),
                    architecture: artifact.architecture.clone(),
                });
            }
            if artifact.capabilities.is_empty()
                || artifact.capabilities.len() > descriptor_capabilities.len()
            {
                return project_invalid(
                    "artifacts.capabilities",
                    format!(
                        "must contain 1 to {} descriptor capability references",
                        descriptor_capabilities.len()
                    ),
                );
            }
            let mut artifact_capabilities = HashSet::with_capacity(artifact.capabilities.len());
            for capability in &artifact.capabilities {
                let key = (
                    capability.interface_id.as_str(),
                    capability.instance_id.as_str(),
                );
                if !descriptor_capabilities.contains(&key) {
                    return project_invalid(
                        "artifacts.capabilities",
                        format!(
                            "capability '{}:{}' is not declared by the plugin descriptor",
                            capability.interface_id, capability.instance_id
                        ),
                    );
                }
                if !artifact_capabilities.insert(key) {
                    return project_invalid(
                        "artifacts.capabilities",
                        format!(
                            "duplicate capability '{}:{}'",
                            capability.interface_id, capability.instance_id
                        ),
                    );
                }
                covered_capabilities.insert(key);
            }
            if artifact.runtime_dependencies.len() > MAX_RUNTIME_DEPENDENCIES {
                return project_invalid(
                    "artifacts.runtime_dependencies",
                    format!("must contain at most {MAX_RUNTIME_DEPENDENCIES} entries"),
                );
            }
            let mut runtime_ids = HashSet::with_capacity(artifact.runtime_dependencies.len());
            for dependency in &artifact.runtime_dependencies {
                validate_identity("artifacts.runtime_dependencies.id", &dependency.id)?;
                validate_text(
                    "artifacts.runtime_dependencies.version",
                    &dependency.version,
                    MAX_RUNTIME_VALUE_BYTES,
                )?;
                validate_text(
                    "artifacts.runtime_dependencies.compatibility_key",
                    &dependency.compatibility_key,
                    MAX_RUNTIME_VALUE_BYTES,
                )?;
                if !runtime_ids.insert(dependency.id.as_str()) {
                    return project_invalid(
                        "artifacts.runtime_dependencies",
                        format!("duplicate runtime dependency '{}'", dependency.id),
                    );
                }
            }
        }
        if !self.artifacts.is_empty() && covered_capabilities != descriptor_capabilities {
            return project_invalid(
                "artifacts.capabilities",
                "every descriptor capability must be provided by at least one artifact",
            );
        }
        for file in &self.package_files {
            validate_local_source("package_files.source", &file.source)?;
            validate_archive_path("package_files.path", &file.path)?;
            let required_prefix = match file.kind {
                PluginPackageFileKind::License => "licenses/",
                PluginPackageFileKind::Notice => "notices/",
                PluginPackageFileKind::RuntimeMetadata => "runtime/",
                PluginPackageFileKind::Redistribution => "redistribution/",
            };
            if !file.path.starts_with(required_prefix) {
                return project_invalid(
                    "package_files.path",
                    format!(
                        "kind requires an archive path below '{}'",
                        required_prefix.trim_end_matches('/')
                    ),
                );
            }
            insert_archive_file_path(&mut package_paths, &file.path)?;
        }
        Ok(())
    }
}

pub(crate) fn validate_archive_path(
    field: &str,
    value: &str,
) -> Result<(), PluginProjectManifestError> {
    if value.is_empty()
        || value.len() > MAX_ARCHIVE_PATH_BYTES
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains('\\')
        || value.contains(':')
        || value.chars().any(char::is_control)
        || value
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return project_invalid(
            field,
            "must be a bounded relative archive file path without dot segments or backslashes",
        );
    }
    Ok(())
}

pub(crate) fn normalized_package_path(value: &str) -> String {
    value.nfc().case_fold().nfc().collect()
}

pub(crate) fn insert_archive_file_path(
    paths: &mut HashSet<String>,
    value: &str,
) -> Result<(), PluginProjectManifestError> {
    let normalized = normalized_package_path(value);
    if paths.contains(&normalized) {
        return Err(PluginProjectManifestError::DuplicatePackagePath {
            path: value.to_owned(),
        });
    }
    if let Some(conflicting_path) = paths.iter().find(|existing| {
        is_archive_path_ancestor(existing, &normalized)
            || is_archive_path_ancestor(&normalized, existing)
    }) {
        return Err(PluginProjectManifestError::ConflictingPackagePath {
            path: value.to_owned(),
            conflicting_path: conflicting_path.clone(),
        });
    }
    paths.insert(normalized);
    Ok(())
}

fn is_archive_path_ancestor(candidate: &str, path: &str) -> bool {
    path.strip_prefix(candidate)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

fn validate_local_source(field: &str, value: &Path) -> Result<(), PluginProjectManifestError> {
    if value.as_os_str().is_empty() {
        return project_invalid(field, "must not be empty");
    }
    Ok(())
}

fn validate_identity(field: &str, value: &str) -> Result<(), PluginProjectManifestError> {
    PluginReference::new(value, None, PluginTransport::Native)
        .map(|_| ())
        .map_err(|error| PluginProjectManifestError::InvalidField {
            field: field.to_owned(),
            message: error.to_string(),
        })
}

fn validate_text(
    field: &str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), PluginProjectManifestError> {
    if value.is_empty() || value.len() > maximum_bytes {
        return project_invalid(
            field,
            format!("must contain 1 to {maximum_bytes} UTF-8 bytes"),
        );
    }
    Ok(())
}

fn project_invalid<T>(
    field: &str,
    message: impl Into<String>,
) -> Result<T, PluginProjectManifestError> {
    Err(PluginProjectManifestError::InvalidField {
        field: field.to_owned(),
        message: message.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_toml(extra: &str) -> String {
        format!(
            r#"
schema_version = 1

[plugin]
id = "dev.vesper.fixture"
name = "Fixture"
version = "1.2.3"
description = "Fixture plugin"
license = "Apache-2.0"
publisher = "dev.vesper.publisher"

[compatibility]
host_sdk = ">=0.4.0, <0.5.0"
abi_major = 1
abi_minor_min = 0
abi_minor_max = 0

[[capabilities]]
interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7"
instance_id = "dev.vesper.fixture.post-download"
interface_major = 1
interface_minor = 0
stability = "stable"

{extra}
"#
        )
    }

    #[test]
    fn project_keeps_descriptor_metadata_authoritative_for_packaging() {
        let project = PluginProjectManifest::from_toml(&project_toml(
            r#"
[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "target/plugin with spaces.dylib"
path = "artifacts/aarch64-apple-darwin/plugin with spaces.dylib"
architecture = "arm64"
capabilities = [{ interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.fixture.post-download" }]

[[package_files]]
source = "LICENSE"
path = "licenses/LICENSE"
kind = "license"

[[package_files]]
source = "NOTICE"
path = "notices/NOTICE"
kind = "notice"
"#,
        ))
        .expect("valid project");

        project
            .validate_package_inputs()
            .expect("complete package inputs");
        assert_eq!(project.descriptor().plugin.id, "dev.vesper.fixture");
        assert_eq!(project.artifacts().len(), 1);
    }

    #[test]
    fn project_rejects_traversal_and_ambiguous_target_selection() {
        let traversal = project_toml(
            r#"
[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "plugin.dylib"
path = "../plugin.dylib"
architecture = "arm64"
capabilities = [{ interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.fixture.post-download" }]
"#,
        );
        assert!(matches!(
            PluginProjectManifest::from_toml(&traversal),
            Err(PluginProjectManifestError::InvalidField { ref field, .. })
                if field == "artifacts.path"
        ));

        let ambiguous = project_toml(
            r#"
[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "first.dylib"
path = "artifacts/first.dylib"
architecture = "arm64"
capabilities = [{ interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.fixture.post-download" }]

[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "xcframework"
source = "second.zip"
path = "artifacts/second.zip"
architecture = "arm64"
capabilities = [{ interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.fixture.post-download" }]
"#,
        );
        assert!(matches!(
            PluginProjectManifest::from_toml(&ambiguous),
            Err(PluginProjectManifestError::AmbiguousArtifactTarget { .. })
        ));
    }

    #[test]
    fn project_rejects_invalid_artifact_capability_ownership_and_coverage() {
        let empty = project_toml(
            r#"
[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "plugin.dylib"
path = "artifacts/plugin.dylib"
architecture = "arm64"
capabilities = []
"#,
        );
        assert!(matches!(
            PluginProjectManifest::from_toml(&empty),
            Err(PluginProjectManifestError::InvalidField { ref field, ref message })
                if field == "artifacts.capabilities" && message.contains("must contain 1")
        ));

        let unowned = project_toml(
            r#"
[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "plugin.dylib"
path = "artifacts/plugin.dylib"
architecture = "arm64"
capabilities = [{ interface_id = "c7a69475-79b2-5b5e-a477-08844a5da5d1", instance_id = "dev.vesper.fixture.event-hook" }]
"#,
        );
        assert!(matches!(
            PluginProjectManifest::from_toml(&unowned),
            Err(PluginProjectManifestError::InvalidField { ref field, ref message })
                if field == "artifacts.capabilities"
                    && message.contains("is not declared by the plugin descriptor")
        ));

        let duplicate = project_toml(
            r#"
[[capabilities]]
interface_id = "c7a69475-79b2-5b5e-a477-08844a5da5d1"
instance_id = "dev.vesper.fixture.event-hook"
interface_major = 1
interface_minor = 0
stability = "stable"

[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "plugin.dylib"
path = "artifacts/plugin.dylib"
architecture = "arm64"
capabilities = [
  { interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.fixture.post-download" },
  { interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.fixture.post-download" },
]
"#,
        );
        assert!(matches!(
            PluginProjectManifest::from_toml(&duplicate),
            Err(PluginProjectManifestError::InvalidField { ref field, ref message })
                if field == "artifacts.capabilities" && message.contains("duplicate capability")
        ));

        let uncovered = project_toml(
            r#"
[[capabilities]]
interface_id = "c7a69475-79b2-5b5e-a477-08844a5da5d1"
instance_id = "dev.vesper.fixture.event-hook"
interface_major = 1
interface_minor = 0
stability = "stable"

[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "plugin.dylib"
path = "artifacts/plugin.dylib"
architecture = "arm64"
capabilities = [{ interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.fixture.post-download" }]
"#,
        );
        assert!(matches!(
            PluginProjectManifest::from_toml(&uncovered),
            Err(PluginProjectManifestError::InvalidField { ref field, ref message })
                if field == "artifacts.capabilities"
                    && message.contains("every descriptor capability")
        ));
    }

    #[test]
    fn project_rejects_file_and_directory_path_conflicts() {
        let conflict = project_toml(
            r#"
[[package_files]]
source = "node.json"
path = "runtime/node"
kind = "runtime-metadata"

[[package_files]]
source = "data.json"
path = "runtime/node/data"
kind = "runtime-metadata"
"#,
        );
        assert!(matches!(
            PluginProjectManifest::from_toml(&conflict),
            Err(PluginProjectManifestError::ConflictingPackagePath {
                ref path,
                ref conflicting_path,
            }) if path == "runtime/node/data" && conflicting_path == "runtime/node"
        ));

        let reserved_ancestor = project_toml(
            r#"
[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "plugin.dylib"
path = "manifest.json/payload"
architecture = "arm64"
capabilities = [{ interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.fixture.post-download" }]
"#,
        );
        assert!(matches!(
            PluginProjectManifest::from_toml(&reserved_ancestor),
            Err(PluginProjectManifestError::ConflictingPackagePath { .. })
        ));
    }

    #[test]
    fn project_rejects_unicode_normalization_and_casefold_path_collisions() {
        let collision = project_toml(
            r#"
[[package_files]]
source = "composed.txt"
path = "licenses/É.txt"
kind = "license"

[[package_files]]
source = "decomposed.txt"
path = "licenses/É.txt"
kind = "license"
"#,
        );
        assert!(matches!(
            PluginProjectManifest::from_toml(&collision),
            Err(PluginProjectManifestError::DuplicatePackagePath { .. })
        ));

        let full_casefold_collision = project_toml(
            r#"
[[package_files]]
source = "eszett.txt"
path = "licenses/ß.txt"
kind = "license"

[[package_files]]
source = "double-s.txt"
path = "licenses/ss.txt"
kind = "license"
"#,
        );
        assert!(matches!(
            PluginProjectManifest::from_toml(&full_casefold_collision),
            Err(PluginProjectManifestError::DuplicatePackagePath { .. })
        ));
    }
}
