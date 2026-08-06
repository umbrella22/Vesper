use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use player_plugin_loader::EmbeddedPluginRegistry;
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{CanonicalPluginDescriptor, PluginDescriptorError};

const HASH_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddedRegistryTarget {
    AndroidNativeLibrary {
        target: String,
        architecture: String,
        minimum_os: String,
        library_name: String,
        artifact_path: PathBuf,
    },
    AppleFramework {
        target: String,
        architecture: String,
        minimum_os: String,
        framework_name: String,
        bundle_identifier: String,
    },
}

impl EmbeddedRegistryTarget {
    fn target(&self) -> &str {
        match self {
            Self::AndroidNativeLibrary { target, .. } | Self::AppleFramework { target, .. } => {
                target
            }
        }
    }

    fn architecture(&self) -> &str {
        match self {
            Self::AndroidNativeLibrary { architecture, .. }
            | Self::AppleFramework { architecture, .. } => architecture,
        }
    }

    fn minimum_os(&self) -> &str {
        match self {
            Self::AndroidNativeLibrary { minimum_os, .. }
            | Self::AppleFramework { minimum_os, .. } => minimum_os,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedRegistryFragment {
    file_name: String,
    json: Vec<u8>,
}

impl EmbeddedRegistryFragment {
    pub fn generate(
        descriptor: &CanonicalPluginDescriptor,
        target: &EmbeddedRegistryTarget,
    ) -> Result<Self, EmbeddedRegistryFragmentError> {
        descriptor.descriptor().validate()?;
        let plugin = &descriptor.descriptor().plugin;
        let (locator, integrity) = match target {
            EmbeddedRegistryTarget::AndroidNativeLibrary {
                library_name,
                artifact_path,
                ..
            } => (
                FragmentArtifactLocator::AndroidNativeLibrary {
                    name: library_name.clone(),
                },
                FragmentIntegrity::Sha256 {
                    digest: sha256_file(artifact_path)?,
                },
            ),
            EmbeddedRegistryTarget::AppleFramework {
                framework_name,
                bundle_identifier,
                ..
            } => (
                FragmentArtifactLocator::AppleFramework {
                    name: framework_name.clone(),
                    bundle_identifier: bundle_identifier.clone(),
                },
                FragmentIntegrity::AppleCodeSignature {
                    validation: AppleCodeSignatureValidation::SameTeamAsHostOrSimulatorAdHoc,
                },
            ),
        };
        let artifact = FragmentArtifact {
            plugin_id: plugin.id.clone(),
            transport: FragmentTransport::Native,
            locator,
            integrity,
            package: FragmentPackage {
                version: plugin.version.clone(),
                publisher: plugin.publisher.clone(),
                descriptor_sha256: descriptor.sha256().to_owned(),
            },
            capabilities: descriptor
                .descriptor()
                .capabilities
                .iter()
                .map(FragmentCapability::from)
                .collect(),
        };
        let wire = FragmentRegistry {
            schema_version: 1,
            target: target.target().to_owned(),
            architecture: target.architecture().to_owned(),
            minimum_os: target.minimum_os().to_owned(),
            artifacts: vec![artifact],
        };
        let json = serde_json::to_vec(&wire)?;
        EmbeddedPluginRegistry::parse(&json, target.target(), target.architecture())?;
        Ok(Self {
            file_name: format!("{}.json", plugin.id),
            json,
        })
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Returns the exact canonical JSON bytes that must be embedded in the host artifact.
    pub fn canonical_json(&self) -> &[u8] {
        &self.json
    }
}

#[derive(Debug, Error)]
pub enum EmbeddedRegistryFragmentError {
    #[error(transparent)]
    Descriptor(#[from] PluginDescriptorError),
    #[error("embedded registry Android artifact `{path}` is not a regular file")]
    ArtifactNotFile { path: String },
    #[error("failed to read embedded registry Android artifact `{path}`: {source}")]
    ReadArtifact {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to serialize embedded registry fragment: {0}")]
    Json(#[from] serde_json::Error),
    #[error("generated embedded registry fragment is invalid: {0}")]
    Registry(#[from] player_plugin_loader::EmbeddedPluginRegistryError),
}

#[derive(Serialize)]
struct FragmentRegistry {
    schema_version: u32,
    target: String,
    architecture: String,
    minimum_os: String,
    artifacts: Vec<FragmentArtifact>,
}

#[derive(Serialize)]
struct FragmentArtifact {
    plugin_id: String,
    transport: FragmentTransport,
    locator: FragmentArtifactLocator,
    integrity: FragmentIntegrity,
    package: FragmentPackage,
    capabilities: Vec<FragmentCapability>,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum FragmentTransport {
    Native,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum FragmentArtifactLocator {
    AndroidNativeLibrary {
        name: String,
    },
    AppleFramework {
        name: String,
        bundle_identifier: String,
    },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum FragmentIntegrity {
    Sha256 {
        digest: String,
    },
    AppleCodeSignature {
        validation: AppleCodeSignatureValidation,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum AppleCodeSignatureValidation {
    SameTeamAsHostOrSimulatorAdHoc,
}

#[derive(Serialize)]
struct FragmentPackage {
    version: String,
    publisher: String,
    descriptor_sha256: String,
}

#[derive(Serialize)]
struct FragmentCapability {
    interface_id: String,
    instance_id: String,
    interface_major: u16,
    interface_minor: u16,
}

impl From<&crate::PluginCapabilityDescriptor> for FragmentCapability {
    fn from(capability: &crate::PluginCapabilityDescriptor) -> Self {
        Self {
            interface_id: capability.interface_id.clone(),
            instance_id: capability.instance_id.clone(),
            interface_major: capability.interface_major,
            interface_minor: capability.interface_minor,
        }
    }
}

fn sha256_file(path: &Path) -> Result<String, EmbeddedRegistryFragmentError> {
    if !path.is_file() {
        return Err(EmbeddedRegistryFragmentError::ArtifactNotFile {
            path: path.display().to_string(),
        });
    }
    let mut file =
        File::open(path).map_err(|source| EmbeddedRegistryFragmentError::ReadArtifact {
            path: path.display().to_string(),
            source,
        })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; HASH_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer).map_err(|source| {
            EmbeddedRegistryFragmentError::ReadArtifact {
                path: path.display().to_string(),
                source,
            }
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::PluginDescriptor;

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

    fn descriptor() -> CanonicalPluginDescriptor {
        PluginDescriptor::from_toml(
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
"#,
        )
        .expect("valid descriptor")
        .canonicalize()
        .expect("canonical descriptor")
    }

    fn temporary_artifact() -> PathBuf {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "vesper-registry-fragment-{}-{id}.so",
            std::process::id()
        ));
        fs::write(&path, b"fixture artifact bytes").expect("write artifact");
        path
    }

    #[test]
    fn android_fragment_hashes_the_runtime_artifact_and_round_trips() {
        let descriptor = descriptor();
        let artifact_path = temporary_artifact();
        let target = EmbeddedRegistryTarget::AndroidNativeLibrary {
            target: "aarch64-linux-android".to_owned(),
            architecture: "arm64-v8a".to_owned(),
            minimum_os: "26".to_owned(),
            library_name: "vesper_fixture".to_owned(),
            artifact_path: artifact_path.clone(),
        };

        let fragment = EmbeddedRegistryFragment::generate(&descriptor, &target)
            .expect("Android registry fragment");
        let value: serde_json::Value =
            serde_json::from_slice(fragment.canonical_json()).expect("registry JSON");

        assert_eq!(fragment.file_name(), "dev.vesper.fixture.json");
        assert_eq!(
            value["artifacts"][0]["package"]["descriptor_sha256"],
            descriptor.sha256()
        );
        assert_eq!(
            value["artifacts"][0]["integrity"]["digest"],
            format!("{:x}", Sha256::digest(b"fixture artifact bytes"))
        );
        let _ = fs::remove_file(artifact_path);
    }

    #[test]
    fn apple_fragment_uses_bundle_identity_and_code_signature_policy() {
        let descriptor = descriptor();
        let target = EmbeddedRegistryTarget::AppleFramework {
            target: "aarch64-apple-ios".to_owned(),
            architecture: "arm64".to_owned(),
            minimum_os: "17.0".to_owned(),
            framework_name: "VesperPluginFixture".to_owned(),
            bundle_identifier: "dev.vesper.plugin-fixture".to_owned(),
        };

        let fragment = EmbeddedRegistryFragment::generate(&descriptor, &target)
            .expect("Apple registry fragment");
        let value: serde_json::Value =
            serde_json::from_slice(fragment.canonical_json()).expect("registry JSON");

        assert_eq!(
            value["artifacts"][0]["locator"]["bundle_identifier"],
            "dev.vesper.plugin-fixture"
        );
        assert_eq!(
            value["artifacts"][0]["integrity"]["validation"],
            "same-team-as-host-or-simulator-ad-hoc"
        );
    }

    #[test]
    fn missing_android_artifact_fails_before_writing_a_fragment() {
        let descriptor = descriptor();
        let target = EmbeddedRegistryTarget::AndroidNativeLibrary {
            target: "aarch64-linux-android".to_owned(),
            architecture: "arm64-v8a".to_owned(),
            minimum_os: "26".to_owned(),
            library_name: "vesper_fixture".to_owned(),
            artifact_path: PathBuf::from("/definitely/missing/vesper_fixture.so"),
        };

        assert!(matches!(
            EmbeddedRegistryFragment::generate(&descriptor, &target),
            Err(EmbeddedRegistryFragmentError::ArtifactNotFile { .. })
        ));
    }
}
