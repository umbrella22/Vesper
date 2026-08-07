use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use player_plugin::{PluginReference, PluginTransport};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::{
    PluginArtifactFormat, PluginArtifactTransport, PluginCapabilityDescriptor,
    PluginCompatibilityDescriptor, PluginDescriptor, PluginDescriptorError,
    PluginIdentityDescriptor, PluginProjectManifest, PluginProjectManifestError,
    PluginRedistributionDescriptor, PluginRuntimeDependencySource,
};

pub const PLUGIN_PACKAGE_MANIFEST_PATH: &str = "manifest.json";
pub const PLUGIN_PACKAGE_CHECKSUMS_PATH: &str = "SHA256SUMS";
pub const PLUGIN_PACKAGE_SIGNATURE_PATH: &str = "signature.json";
pub const MAX_PLUGIN_PACKAGE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_PLUGIN_PACKAGE_ENTRY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_PLUGIN_PACKAGE_ENTRIES: usize = 256;
pub const MAX_PLUGIN_TRUST_STORE_BYTES: u64 = 1024 * 1024;
const MAX_SMALL_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_COMPRESSION_RATIO: u64 = 100;
const SIGNING_KEY_SCHEMA_VERSION: u32 = 1;
const TRUST_STORE_SCHEMA_VERSION: u32 = 1;
const PACKAGE_MANIFEST_SCHEMA_VERSION: u32 = 1;
const SIGNATURE_SCHEMA_VERSION: u32 = 1;
const SIGNATURE_ALGORITHM: &str = "ed25519";
const SIGNATURE_DOMAIN: &[u8] = b"vesper-plugin-signature\0";
pub(crate) const INSTALL_MARKER_PATH: &str = ".vesper-package-sha256";
const CATALOG_LOCK_PATH: &str = ".vesper-catalog.lock";
const UNIX_FILE_TYPE_MASK: u32 = 0o170000;
const UNIX_REGULAR_FILE: u32 = 0o100000;
const ARTIFACT_FILE_MODE: u32 = 0o755;
const PACKAGE_METADATA_FILE_MODE: u32 = 0o644;
const MAX_INSTALLED_PLUGIN_IDENTITIES: usize = 1024;
const MAX_INSTALLED_VERSIONS_PER_PLUGIN: usize = 256;
const RUST_WASM_COMPONENT_TARGET: &str = "wasm32-wasip2";
const RUST_WASM_COMPONENT_ARCHITECTURE: &str = "wasm32";

pub struct PluginSigningKey {
    publisher: String,
    key_id: String,
    key: SigningKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPublicKey {
    publisher: String,
    key_id: String,
    public_key: [u8; 32],
    status: TrustedKeyStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SigningKeyWire {
    schema_version: u32,
    algorithm: String,
    publisher: String,
    key_id: String,
    public_key: String,
    secret_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustStoreWire {
    schema_version: u32,
    publishers: BTreeMap<String, Vec<TrustedKeyWire>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedKeyWire {
    algorithm: String,
    key_id: String,
    public_key: String,
    status: TrustedKeyStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustedKeyStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTrustStore {
    publishers: BTreeMap<String, Vec<PluginPublicKey>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPackageManifest {
    pub schema_version: u32,
    pub plugin: PluginIdentityDescriptor,
    pub compatibility: PluginCompatibilityDescriptor,
    pub capabilities: Vec<PluginCapabilityDescriptor>,
    pub artifacts: Vec<PluginPackageArtifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redistribution: Vec<PluginRedistributionDescriptor>,
    pub generated_by: PluginPackageGenerator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPackageArtifact {
    pub transport: PluginArtifactTransport,
    pub target: String,
    pub format: PluginArtifactFormat,
    pub path: String,
    pub architecture: String,
    pub capabilities: Vec<crate::PluginArtifactCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_os: Option<String>,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_dependencies: Vec<PluginRuntimeDependencySource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPackageGenerator {
    pub vesper: String,
    pub sdk: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginPackageSignature {
    schema_version: u32,
    algorithm: String,
    publisher: String,
    key_id: String,
    signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginPackageBuildReport {
    pub package_path: PathBuf,
    pub plugin_id: String,
    pub publisher: String,
    pub key_id: String,
    pub artifact_count: usize,
    pub package_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginPackageVerification {
    pub package_path: PathBuf,
    pub plugin_id: String,
    pub version: String,
    pub publisher: String,
    pub key_id: String,
    pub artifact_count: usize,
    pub package_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginInstallationReport {
    pub plugin_id: String,
    pub version: String,
    pub install_path: PathBuf,
    pub package_sha256: String,
    pub already_installed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledPluginRecord {
    pub plugin_id: String,
    pub version: String,
    pub install_path: PathBuf,
    pub package_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPluginActivation {
    plugin_id: String,
    version: String,
}

impl InstalledPluginActivation {
    pub fn new(
        plugin_id: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, PluginPackageError> {
        let plugin_id = plugin_id.into();
        let version = version.into();
        validate_reverse_dns_identifier(&plugin_id).map_err(PluginPackageError::InvalidPackage)?;
        Version::parse(&version).map_err(|error| {
            PluginPackageError::InvalidPackage(format!(
                "invalid activation version '{version}': {error}"
            ))
        })?;
        Ok(Self { plugin_id, version })
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn version(&self) -> &str {
        &self.version
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHostTarget {
    host_sdk: Version,
    target: String,
    architecture: String,
}

impl PluginHostTarget {
    pub fn new(
        host_sdk: Version,
        target: impl Into<String>,
        architecture: impl Into<String>,
    ) -> Result<Self, PluginPackageError> {
        let target = target.into();
        let architecture = architecture.into();
        validate_package_text(
            "host target",
            &target,
            crate::plugin_project::MAX_TARGET_BYTES,
        )?;
        validate_package_text(
            "host architecture",
            &architecture,
            crate::plugin_project::MAX_ARCHITECTURE_BYTES,
        )?;
        Ok(Self {
            host_sdk,
            target,
            architecture,
        })
    }

    pub fn host_sdk(&self) -> &Version {
        &self.host_sdk
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn architecture(&self) -> &str {
        &self.architecture
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedInstalledArtifact {
    plugin_id: String,
    version: String,
    transport: PluginArtifactTransport,
    target: String,
    format: PluginArtifactFormat,
    architecture: String,
    capabilities: Vec<PluginCapabilityDescriptor>,
    minimum_os: Option<String>,
    runtime_dependencies: Vec<PluginRuntimeDependencySource>,
    installed_path: PathBuf,
    sha256: String,
    snapshot: Arc<tempfile::NamedTempFile>,
}

impl VerifiedInstalledArtifact {
    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub const fn transport(&self) -> PluginArtifactTransport {
        self.transport
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub const fn format(&self) -> PluginArtifactFormat {
        self.format
    }

    pub fn architecture(&self) -> &str {
        &self.architecture
    }

    pub fn capabilities(&self) -> &[PluginCapabilityDescriptor] {
        &self.capabilities
    }

    pub fn minimum_os(&self) -> Option<&str> {
        self.minimum_os.as_deref()
    }

    pub fn runtime_dependencies(&self) -> &[PluginRuntimeDependencySource] {
        &self.runtime_dependencies
    }

    pub fn installed_path(&self) -> &Path {
        &self.installed_path
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Process-private immutable snapshot used by checked loaders.
    pub fn snapshot_path(&self) -> &Path {
        self.snapshot.path()
    }

    pub fn read_snapshot(&self, maximum_bytes: usize) -> Result<Vec<u8>, PluginPackageError> {
        let metadata =
            self.snapshot
                .as_file()
                .metadata()
                .map_err(|source| PluginPackageError::Io {
                    operation: "inspect verified artifact snapshot",
                    path: self.snapshot.path().display().to_string(),
                    source,
                })?;
        if metadata.len() > maximum_bytes as u64 {
            return Err(PluginPackageError::InvalidPackage(format!(
                "verified artifact snapshot '{}' exceeds {maximum_bytes} bytes",
                self.snapshot.path().display()
            )));
        }
        let mut file =
            self.snapshot
                .as_file()
                .try_clone()
                .map_err(|source| PluginPackageError::Io {
                    operation: "clone verified artifact snapshot",
                    path: self.snapshot.path().display().to_string(),
                    source,
                })?;
        file.seek(SeekFrom::Start(0))
            .map_err(|source| PluginPackageError::Io {
                operation: "rewind verified artifact snapshot",
                path: self.snapshot.path().display().to_string(),
                source,
            })?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut bytes)
            .map_err(|source| PluginPackageError::Io {
                operation: "read verified artifact snapshot",
                path: self.snapshot.path().display().to_string(),
                source,
            })?;
        Ok(bytes)
    }
}

#[derive(Debug)]
pub struct VerifiedInstalledPluginCatalog {
    _catalog_lock: Option<PluginCatalogLock>,
    artifacts: Vec<VerifiedInstalledArtifact>,
}

impl VerifiedInstalledPluginCatalog {
    pub fn artifacts(&self) -> &[VerifiedInstalledArtifact] {
        &self.artifacts
    }
}

#[derive(Debug)]
pub struct VerifiedPluginPackage {
    package_file: Arc<File>,
    manifest: PluginPackageManifest,
    verification: PluginPackageVerification,
    entries: Vec<VerifiedPackageEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VerifiedPackageEntry {
    path: String,
    size: u64,
    mode: u32,
    sha256: String,
}

#[derive(Debug)]
struct PositionedFile {
    file: Arc<File>,
    position: u64,
}

#[derive(Debug)]
struct PluginCatalogLock {
    _file: File,
}

#[derive(Debug)]
struct EmptyIdentityDirectoryRollback {
    path: PathBuf,
    armed: bool,
}

impl EmptyIdentityDirectoryRollback {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for EmptyIdentityDirectoryRollback {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir(&self.path);
        }
    }
}

impl PluginCatalogLock {
    fn acquire(install_root: &Path) -> Result<Self, PluginPackageError> {
        Self::acquire_with_mode(install_root, false)
    }

    fn acquire_shared(install_root: &Path) -> Result<Self, PluginPackageError> {
        Self::acquire_with_mode(install_root, true)
    }

    fn acquire_with_mode(install_root: &Path, shared: bool) -> Result<Self, PluginPackageError> {
        let lock_path = install_root.join(CATALOG_LOCK_PATH);
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => {
                file.sync_all().map_err(|source| PluginPackageError::Io {
                    operation: "sync plugin install catalog lock",
                    path: lock_path.display().to_string(),
                    source,
                })?;
                sync_directory(install_root)?;
                file
            }
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                let metadata =
                    fs::symlink_metadata(&lock_path).map_err(|source| PluginPackageError::Io {
                        operation: "inspect plugin install catalog lock",
                        path: lock_path.display().to_string(),
                        source,
                    })?;
                if !metadata.file_type().is_file() {
                    return Err(PluginPackageError::InvalidPackage(format!(
                        "plugin install catalog lock '{}' is not a regular file",
                        lock_path.display()
                    )));
                }
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&lock_path)
                    .map_err(|source| PluginPackageError::Io {
                        operation: "open plugin install catalog lock",
                        path: lock_path.display().to_string(),
                        source,
                    })?
            }
            Err(source) => {
                return Err(PluginPackageError::Io {
                    operation: "create plugin install catalog lock",
                    path: lock_path.display().to_string(),
                    source,
                });
            }
        };
        let lock_result = if shared {
            file.try_lock_shared()
        } else {
            file.try_lock()
        };
        match lock_result {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(PluginPackageError::CatalogBusy {
                path: install_root.display().to_string(),
            }),
            Err(TryLockError::Error(source)) => Err(PluginPackageError::Io {
                operation: "lock plugin install catalog",
                path: lock_path.display().to_string(),
                source,
            }),
        }
    }
}

impl PositionedFile {
    fn new(file: Arc<File>) -> Self {
        Self { file, position: 0 }
    }
}

impl Read for PositionedFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = positioned_read(&self.file, buffer, self.position)?;
        self.position = self
            .position
            .checked_add(read as u64)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file offset overflow"))?;
        Ok(read)
    }
}

impl Seek for PositionedFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let next = match position {
            SeekFrom::Start(position) => position,
            SeekFrom::End(delta) => checked_seek_position(self.file.metadata()?.len(), delta)?,
            SeekFrom::Current(delta) => checked_seek_position(self.position, delta)?,
        };
        self.position = next;
        Ok(next)
    }
}

fn checked_seek_position(base: u64, delta: i64) -> io::Result<u64> {
    let position = i128::from(base) + i128::from(delta);
    u64::try_from(position)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid file seek"))
}

#[cfg(unix)]
fn positioned_read(file: &File, buffer: &mut [u8], position: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;

    file.read_at(buffer, position)
}

#[cfg(windows)]
fn positioned_read(file: &File, buffer: &mut [u8], position: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;

    file.seek_read(buffer, position)
}

#[derive(Debug, Error)]
pub enum PluginPackageError {
    #[error(transparent)]
    Project(#[from] PluginProjectManifestError),
    #[error(transparent)]
    Descriptor(#[from] PluginDescriptorError),
    #[error("failed to {operation} '{path}': {source}")]
    Io {
        operation: &'static str,
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("plugin install catalog '{path}' is busy")]
    CatalogBusy { path: String },
    #[error("invalid signing key: {0}")]
    InvalidSigningKey(String),
    #[error("invalid trust store: {0}")]
    InvalidTrustStore(String),
    #[error("invalid plugin package: {0}")]
    InvalidPackage(String),
    #[error("plugin package signature is not trusted or valid")]
    InvalidSignature,
    #[error("installed plugin '{plugin_id}' has no version '{version}'")]
    InstalledVersionNotFound { plugin_id: String, version: String },
    #[error("installed plugin '{plugin_id}' has ambiguous versions {versions:?}")]
    AmbiguousInstalledVersions {
        plugin_id: String,
        versions: Vec<String>,
    },
    #[error(
        "installed plugin '{plugin_id}' has no {transport} artifact for target '{target}' architecture '{architecture}'"
    )]
    InstalledArtifactNotFound {
        plugin_id: String,
        transport: &'static str,
        target: String,
        architecture: String,
    },
    #[error("installed plugin compatibility check failed: {0}")]
    Compatibility(String),
    #[error("failed to encode plugin package JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("failed to process plugin package ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
}

impl PluginSigningKey {
    pub fn generate(publisher: impl Into<String>) -> Result<Self, PluginPackageError> {
        let publisher = publisher.into();
        validate_reverse_dns_identifier(&publisher)
            .map_err(PluginPackageError::InvalidSigningKey)?;
        let mut secret_key = [0_u8; 32];
        getrandom::fill(&mut secret_key).map_err(|error| {
            PluginPackageError::InvalidSigningKey(format!(
                "failed to obtain system randomness for signing key: {error}"
            ))
        })?;
        let key = SigningKey::from_bytes(&secret_key);
        let key_id = key_id(&key.verifying_key().to_bytes());
        Ok(Self {
            publisher,
            key_id,
            key,
        })
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, PluginPackageError> {
        let wire: SigningKeyWire = serde_json::from_slice(bytes)
            .map_err(|error| PluginPackageError::InvalidSigningKey(error.to_string()))?;
        if wire.schema_version != SIGNING_KEY_SCHEMA_VERSION {
            return Err(PluginPackageError::InvalidSigningKey(format!(
                "expected schema version {SIGNING_KEY_SCHEMA_VERSION}"
            )));
        }
        if wire.algorithm != SIGNATURE_ALGORITHM {
            return Err(PluginPackageError::InvalidSigningKey(
                "algorithm must be ed25519".to_owned(),
            ));
        }
        validate_reverse_dns_identifier(&wire.publisher)
            .map_err(PluginPackageError::InvalidSigningKey)?;
        let secret_key =
            decode_hex::<32>(&wire.secret_key).map_err(PluginPackageError::InvalidSigningKey)?;
        let public_key =
            decode_hex::<32>(&wire.public_key).map_err(PluginPackageError::InvalidSigningKey)?;
        let key = SigningKey::from_bytes(&secret_key);
        if key.verifying_key().to_bytes() != public_key {
            return Err(PluginPackageError::InvalidSigningKey(
                "public key does not match the secret key".to_owned(),
            ));
        }
        let expected_key_id = key_id(&public_key);
        if wire.key_id != expected_key_id {
            return Err(PluginPackageError::InvalidSigningKey(
                "key_id does not match the public key".to_owned(),
            ));
        }
        Ok(Self {
            publisher: wire.publisher,
            key_id: expected_key_id,
            key,
        })
    }

    pub fn to_json(&self) -> Result<Vec<u8>, PluginPackageError> {
        let wire = SigningKeyWire {
            schema_version: SIGNING_KEY_SCHEMA_VERSION,
            algorithm: SIGNATURE_ALGORITHM.to_owned(),
            publisher: self.publisher.clone(),
            key_id: self.key_id.clone(),
            public_key: encode_hex(&self.key.verifying_key().to_bytes()),
            secret_key: encode_hex(&self.key.to_bytes()),
        };
        Ok(serde_json::to_vec(&wire)?)
    }

    pub fn publisher(&self) -> &str {
        &self.publisher
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn public_key(&self) -> PluginPublicKey {
        PluginPublicKey {
            publisher: self.publisher.clone(),
            key_id: self.key_id.clone(),
            public_key: self.key.verifying_key().to_bytes(),
            status: TrustedKeyStatus::Active,
        }
    }
}

impl PluginPublicKey {
    pub fn publisher(&self) -> &str {
        &self.publisher
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub const fn status(&self) -> TrustedKeyStatus {
        self.status
    }
}

impl PluginTrustStore {
    pub fn empty() -> Self {
        Self {
            publishers: BTreeMap::new(),
        }
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, PluginPackageError> {
        let wire: TrustStoreWire = serde_json::from_slice(bytes)
            .map_err(|error| PluginPackageError::InvalidTrustStore(error.to_string()))?;
        if wire.schema_version != TRUST_STORE_SCHEMA_VERSION {
            return Err(PluginPackageError::InvalidTrustStore(format!(
                "expected schema version {TRUST_STORE_SCHEMA_VERSION}"
            )));
        }
        let mut store = Self::empty();
        for (publisher, keys) in wire.publishers {
            validate_reverse_dns_identifier(&publisher)
                .map_err(PluginPackageError::InvalidTrustStore)?;
            if keys.is_empty() || keys.len() > 16 {
                return Err(PluginPackageError::InvalidTrustStore(format!(
                    "publisher '{publisher}' must contain 1 to 16 keys"
                )));
            }
            for key in keys {
                if key.algorithm != SIGNATURE_ALGORITHM {
                    return Err(PluginPackageError::InvalidTrustStore(
                        "algorithm must be ed25519".to_owned(),
                    ));
                }
                let public_key = decode_hex::<32>(&key.public_key)
                    .map_err(PluginPackageError::InvalidTrustStore)?;
                let expected_key_id = key_id(&public_key);
                if key.key_id != expected_key_id {
                    return Err(PluginPackageError::InvalidTrustStore(format!(
                        "key_id does not match a public key for publisher '{publisher}'"
                    )));
                }
                store.insert(PluginPublicKey {
                    publisher: publisher.clone(),
                    key_id: expected_key_id,
                    public_key,
                    status: key.status,
                })?;
            }
        }
        Ok(store)
    }

    pub fn from_file(path: &Path) -> Result<Self, PluginPackageError> {
        let bytes = read_bounded_file(path, MAX_PLUGIN_TRUST_STORE_BYTES, "plugin trust store")?;
        Self::from_json(&bytes)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, PluginPackageError> {
        let publishers = self
            .publishers
            .iter()
            .map(|(publisher, keys)| {
                (
                    publisher.clone(),
                    keys.iter()
                        .map(|key| TrustedKeyWire {
                            algorithm: SIGNATURE_ALGORITHM.to_owned(),
                            key_id: key.key_id.clone(),
                            public_key: encode_hex(&key.public_key),
                            status: key.status,
                        })
                        .collect(),
                )
            })
            .collect();
        Ok(serde_json::to_vec(&TrustStoreWire {
            schema_version: TRUST_STORE_SCHEMA_VERSION,
            publishers,
        })?)
    }

    pub fn insert(&mut self, key: PluginPublicKey) -> Result<(), PluginPackageError> {
        validate_reverse_dns_identifier(&key.publisher)
            .map_err(PluginPackageError::InvalidTrustStore)?;
        if key.key_id != key_id(&key.public_key) {
            return Err(PluginPackageError::InvalidTrustStore(
                "key_id does not match the public key".to_owned(),
            ));
        }
        let keys = self.publishers.entry(key.publisher.clone()).or_default();
        if let Some(existing) = keys.iter().find(|existing| existing.key_id == key.key_id) {
            if existing.public_key == key.public_key {
                return Ok(());
            }
            return Err(PluginPackageError::InvalidTrustStore(format!(
                "duplicate key_id '{}' has different key bytes",
                key.key_id
            )));
        }
        if keys.len() >= 16 {
            return Err(PluginPackageError::InvalidTrustStore(format!(
                "publisher '{}' already has 16 keys",
                key.publisher
            )));
        }
        keys.push(key);
        keys.sort_by(|left, right| left.key_id.cmp(&right.key_id));
        Ok(())
    }

    pub fn revoke(
        &mut self,
        publisher: &str,
        requested_key_id: &str,
    ) -> Result<(), PluginPackageError> {
        let key = self
            .publishers
            .get_mut(publisher)
            .and_then(|keys| keys.iter_mut().find(|key| key.key_id == requested_key_id))
            .ok_or_else(|| {
                PluginPackageError::InvalidTrustStore(format!(
                    "publisher '{publisher}' has no key '{requested_key_id}'"
                ))
            })?;
        key.status = TrustedKeyStatus::Revoked;
        Ok(())
    }

    fn verifying_key(
        &self,
        publisher: &str,
        requested_key_id: &str,
    ) -> Result<VerifyingKey, PluginPackageError> {
        let key = self
            .publishers
            .get(publisher)
            .and_then(|keys| {
                keys.iter().find(|key| {
                    key.key_id == requested_key_id && key.status == TrustedKeyStatus::Active
                })
            })
            .ok_or(PluginPackageError::InvalidSignature)?;
        VerifyingKey::from_bytes(&key.public_key).map_err(|_| PluginPackageError::InvalidSignature)
    }
}

enum PreparedEntryData {
    Bytes(Vec<u8>),
    Snapshot { file: File, source_path: PathBuf },
}

struct PreparedEntry {
    path: String,
    data: PreparedEntryData,
    sha256: String,
    size: u64,
    mode: u32,
}

pub fn build_signed_plugin_package(
    project: &PluginProjectManifest,
    base_directory: &Path,
    signing_key: &PluginSigningKey,
    output: &Path,
) -> Result<PluginPackageBuildReport, PluginPackageError> {
    project.validate_package_inputs()?;
    let descriptor = project.descriptor().canonicalize()?.descriptor().clone();
    if signing_key.publisher() != descriptor.plugin.publisher {
        return Err(PluginPackageError::InvalidSigningKey(format!(
            "key publisher '{}' does not match manifest publisher '{}'",
            signing_key.publisher(),
            descriptor.plugin.publisher
        )));
    }

    let mut entries =
        Vec::with_capacity(project.artifacts().len() + project.package_files().len() + 3);
    let mut artifacts = Vec::with_capacity(project.artifacts().len());
    let reserved_paths = [
        PLUGIN_PACKAGE_MANIFEST_PATH,
        PLUGIN_PACKAGE_CHECKSUMS_PATH,
        PLUGIN_PACKAGE_SIGNATURE_PATH,
        INSTALL_MARKER_PATH,
    ]
    .into_iter()
    .map(crate::plugin_project::normalized_package_path)
    .collect::<HashSet<_>>();

    for artifact in project.artifacts() {
        if reserved_paths.contains(&crate::plugin_project::normalized_package_path(
            &artifact.path,
        )) {
            return Err(PluginPackageError::InvalidPackage(format!(
                "artifact path '{}' is reserved",
                artifact.path
            )));
        }
        let prepared = prepare_file_entry(
            base_directory,
            &artifact.source,
            &artifact.path,
            ARTIFACT_FILE_MODE,
        )?;
        let mut runtime_dependencies = artifact.runtime_dependencies.clone();
        runtime_dependencies.sort_by(|left, right| left.id.cmp(&right.id));
        artifacts.push(PluginPackageArtifact {
            transport: artifact.transport,
            target: artifact.target.clone(),
            format: artifact.format,
            path: artifact.path.clone(),
            architecture: artifact.architecture.clone(),
            capabilities: artifact.capabilities.clone(),
            minimum_os: artifact.minimum_os.clone(),
            sha256: prepared.sha256.clone(),
            runtime_dependencies,
        });
        entries.push(prepared);
    }
    for file in project.package_files() {
        if reserved_paths.contains(&crate::plugin_project::normalized_package_path(&file.path)) {
            return Err(PluginPackageError::InvalidPackage(format!(
                "package file path '{}' is reserved",
                file.path
            )));
        }
        entries.push(prepare_file_entry(
            base_directory,
            &file.source,
            &file.path,
            PACKAGE_METADATA_FILE_MODE,
        )?);
    }

    artifacts.sort_by(|left, right| {
        (
            left.transport.as_str(),
            &left.target,
            &left.architecture,
            &left.path,
        )
            .cmp(&(
                right.transport.as_str(),
                &right.target,
                &right.architecture,
                &right.path,
            ))
    });
    let mut manifest = PluginPackageManifest {
        schema_version: PACKAGE_MANIFEST_SCHEMA_VERSION,
        plugin: descriptor.plugin,
        compatibility: descriptor.compatibility,
        capabilities: descriptor.capabilities,
        artifacts,
        redistribution: descriptor.redistribution,
        generated_by: PluginPackageGenerator {
            vesper: env!("CARGO_PKG_VERSION").to_owned(),
            sdk: env!("CARGO_PKG_VERSION").to_owned(),
        },
    };
    let manifest_bytes = canonical_manifest_bytes(&mut manifest)?;
    entries.push(prepared_bytes_entry(
        PLUGIN_PACKAGE_MANIFEST_PATH,
        manifest_bytes,
        PACKAGE_METADATA_FILE_MODE,
    )?);
    entries.sort_by(|left, right| left.path.cmp(&right.path));

    let checksums = entries
        .iter()
        .map(|entry| (entry.path.clone(), entry.sha256.clone()))
        .collect::<BTreeMap<_, _>>();
    let checksums_bytes = canonical_checksums(&checksums);
    let signature = signing_key.key.sign(&signature_message(&checksums_bytes));
    let signature_bytes = serde_json::to_vec(&PluginPackageSignature {
        schema_version: SIGNATURE_SCHEMA_VERSION,
        algorithm: SIGNATURE_ALGORITHM.to_owned(),
        publisher: signing_key.publisher.clone(),
        key_id: signing_key.key_id.clone(),
        signature: encode_hex(&signature.to_bytes()),
    })?;
    entries.push(prepared_bytes_entry(
        PLUGIN_PACKAGE_CHECKSUMS_PATH,
        checksums_bytes,
        PACKAGE_METADATA_FILE_MODE,
    )?);
    entries.push(prepared_bytes_entry(
        PLUGIN_PACKAGE_SIGNATURE_PATH,
        signature_bytes,
        PACKAGE_METADATA_FILE_MODE,
    )?);
    entries.sort_by(|left, right| left.path.cmp(&right.path));

    write_package_atomically(output, &entries)?;
    let package_sha256 = sha256_file(output)?;
    Ok(PluginPackageBuildReport {
        package_path: output.to_path_buf(),
        plugin_id: manifest.plugin.id,
        publisher: manifest.plugin.publisher,
        key_id: signing_key.key_id.clone(),
        artifact_count: manifest.artifacts.len(),
        package_sha256,
    })
}

fn canonical_manifest_bytes(
    manifest: &mut PluginPackageManifest,
) -> Result<Vec<u8>, PluginPackageError> {
    if manifest.schema_version != PACKAGE_MANIFEST_SCHEMA_VERSION {
        return Err(PluginPackageError::InvalidPackage(format!(
            "manifest schema_version must be {PACKAGE_MANIFEST_SCHEMA_VERSION}"
        )));
    }
    let descriptor = PluginDescriptor {
        schema_version: manifest.schema_version,
        plugin: manifest.plugin.clone(),
        compatibility: manifest.compatibility.clone(),
        capabilities: manifest.capabilities.clone(),
        redistribution: manifest.redistribution.clone(),
    }
    .canonicalize()?
    .descriptor()
    .clone();
    manifest.plugin = descriptor.plugin;
    manifest.compatibility = descriptor.compatibility;
    manifest.capabilities = descriptor.capabilities;
    manifest.redistribution = descriptor.redistribution;

    if manifest.artifacts.is_empty()
        || manifest.artifacts.len() > crate::plugin_project::MAX_ARTIFACTS
    {
        return Err(PluginPackageError::InvalidPackage(format!(
            "manifest artifacts must contain 1 to {} entries",
            crate::plugin_project::MAX_ARTIFACTS
        )));
    }
    let mut paths = HashSet::with_capacity(manifest.artifacts.len());
    let mut selectors = HashSet::with_capacity(manifest.artifacts.len());
    let descriptor_capabilities = manifest
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
    for artifact in &mut manifest.artifacts {
        crate::plugin_project::validate_archive_path("artifacts.path", &artifact.path)?;
        crate::plugin_project::insert_archive_file_path(&mut paths, &artifact.path)?;
        validate_package_text(
            "artifacts.target",
            &artifact.target,
            crate::plugin_project::MAX_TARGET_BYTES,
        )?;
        validate_package_text(
            "artifacts.architecture",
            &artifact.architecture,
            crate::plugin_project::MAX_ARCHITECTURE_BYTES,
        )?;
        if let Some(minimum_os) = artifact.minimum_os.as_deref() {
            validate_package_text(
                "artifacts.minimum_os",
                minimum_os,
                crate::plugin_project::MAX_MINIMUM_OS_BYTES,
            )?;
        }
        match (artifact.transport, artifact.format) {
            (PluginArtifactTransport::Wasm, PluginArtifactFormat::WasmComponent)
            | (PluginArtifactTransport::Native, PluginArtifactFormat::Dylib)
            | (PluginArtifactTransport::Native, PluginArtifactFormat::Aar)
            | (PluginArtifactTransport::Native, PluginArtifactFormat::Xcframework) => {}
            _ => {
                return Err(PluginPackageError::InvalidPackage(format!(
                    "artifact format '{}' is incompatible with transport '{}'",
                    artifact.format.as_str(),
                    artifact.transport.as_str()
                )));
            }
        }
        let selector = (
            artifact.transport,
            artifact.target.clone(),
            artifact.architecture.clone(),
        );
        if !selectors.insert(selector) {
            return Err(PluginPackageError::InvalidPackage(format!(
                "ambiguous artifact target '{}:{}:{}'",
                artifact.transport.as_str(),
                artifact.target,
                artifact.architecture
            )));
        }
        if artifact.capabilities.is_empty()
            || artifact.capabilities.len() > descriptor_capabilities.len()
        {
            return Err(PluginPackageError::InvalidPackage(format!(
                "artifact capabilities must contain 1 to {} descriptor capability references",
                descriptor_capabilities.len()
            )));
        }
        artifact.capabilities.sort_by(|left, right| {
            (&left.interface_id, &left.instance_id).cmp(&(&right.interface_id, &right.instance_id))
        });
        let mut artifact_capabilities = HashSet::with_capacity(artifact.capabilities.len());
        for capability in &artifact.capabilities {
            let key = (
                capability.interface_id.as_str(),
                capability.instance_id.as_str(),
            );
            if !descriptor_capabilities.contains(&key) || !artifact_capabilities.insert(key) {
                return Err(PluginPackageError::InvalidPackage(format!(
                    "artifact capability '{}:{}' is absent from the descriptor or duplicated",
                    capability.interface_id, capability.instance_id
                )));
            }
            covered_capabilities.insert(key);
        }
        validate_sha256(&artifact.sha256)?;
        if artifact.runtime_dependencies.len() > crate::plugin_project::MAX_RUNTIME_DEPENDENCIES {
            return Err(PluginPackageError::InvalidPackage(format!(
                "artifacts.runtime_dependencies must contain at most {} entries",
                crate::plugin_project::MAX_RUNTIME_DEPENDENCIES
            )));
        }
        artifact
            .runtime_dependencies
            .sort_by(|left, right| left.id.cmp(&right.id));
        let mut dependency_ids = HashSet::with_capacity(artifact.runtime_dependencies.len());
        for dependency in &artifact.runtime_dependencies {
            validate_reverse_dns_identifier(&dependency.id)
                .map_err(PluginPackageError::InvalidPackage)?;
            validate_package_text(
                "artifacts.runtime_dependencies.version",
                &dependency.version,
                crate::plugin_project::MAX_RUNTIME_VALUE_BYTES,
            )?;
            validate_package_text(
                "artifacts.runtime_dependencies.compatibility_key",
                &dependency.compatibility_key,
                crate::plugin_project::MAX_RUNTIME_VALUE_BYTES,
            )?;
            if !dependency_ids.insert(dependency.id.as_str()) {
                return Err(PluginPackageError::InvalidPackage(format!(
                    "invalid or duplicate runtime dependency '{}'",
                    dependency.id
                )));
            }
        }
    }
    if covered_capabilities != descriptor_capabilities {
        return Err(PluginPackageError::InvalidPackage(
            "every descriptor capability must be provided by at least one artifact".to_owned(),
        ));
    }
    manifest.artifacts.sort_by(|left, right| {
        (
            left.transport.as_str(),
            &left.target,
            &left.architecture,
            &left.path,
        )
            .cmp(&(
                right.transport.as_str(),
                &right.target,
                &right.architecture,
                &right.path,
            ))
    });
    if manifest.generated_by.vesper.is_empty() || manifest.generated_by.sdk.is_empty() {
        return Err(PluginPackageError::InvalidPackage(
            "manifest generated_by versions must not be empty".to_owned(),
        ));
    }
    Ok(serde_json::to_vec(manifest)?)
}

fn prepare_file_entry(
    base_directory: &Path,
    source: &Path,
    package_path: &str,
    mode: u32,
) -> Result<PreparedEntry, PluginPackageError> {
    let resolved = if source.is_absolute() {
        source.to_path_buf()
    } else {
        base_directory.join(source)
    };
    let metadata = fs::symlink_metadata(&resolved).map_err(|source| PluginPackageError::Io {
        operation: "inspect package input",
        path: resolved.display().to_string(),
        source,
    })?;
    if !metadata.file_type().is_file() {
        return Err(PluginPackageError::InvalidPackage(format!(
            "package input '{}' is not a regular non-symlink file",
            resolved.display()
        )));
    }
    if metadata.len() > MAX_PLUGIN_PACKAGE_ENTRY_BYTES {
        return Err(PluginPackageError::InvalidPackage(format!(
            "package input '{}' exceeds {MAX_PLUGIN_PACKAGE_ENTRY_BYTES} bytes",
            resolved.display()
        )));
    }
    let mut input = File::open(&resolved).map_err(|source| PluginPackageError::Io {
        operation: "open package input",
        path: resolved.display().to_string(),
        source,
    })?;
    let opened_metadata = input.metadata().map_err(|source| PluginPackageError::Io {
        operation: "inspect opened package input",
        path: resolved.display().to_string(),
        source,
    })?;
    if !opened_metadata.file_type().is_file() {
        return Err(PluginPackageError::InvalidPackage(format!(
            "package input '{}' did not open as a regular file",
            resolved.display()
        )));
    }
    let mut snapshot = tempfile::tempfile().map_err(|source| PluginPackageError::Io {
        operation: "create package input snapshot",
        path: resolved.display().to_string(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|source| PluginPackageError::Io {
                operation: "read package input snapshot",
                path: resolved.display().to_string(),
                source,
            })?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(u64::try_from(read).map_err(|_| {
                PluginPackageError::InvalidPackage("package input size overflow".to_owned())
            })?)
            .ok_or_else(|| {
                PluginPackageError::InvalidPackage("package input size overflow".to_owned())
            })?;
        if size > MAX_PLUGIN_PACKAGE_ENTRY_BYTES {
            return Err(PluginPackageError::InvalidPackage(format!(
                "package input '{}' exceeds {MAX_PLUGIN_PACKAGE_ENTRY_BYTES} bytes",
                resolved.display()
            )));
        }
        snapshot
            .write_all(&buffer[..read])
            .map_err(|source| PluginPackageError::Io {
                operation: "write package input snapshot",
                path: resolved.display().to_string(),
                source,
            })?;
        hasher.update(&buffer[..read]);
    }
    snapshot
        .seek(SeekFrom::Start(0))
        .map_err(|source| PluginPackageError::Io {
            operation: "rewind package input snapshot",
            path: resolved.display().to_string(),
            source,
        })?;
    Ok(PreparedEntry {
        path: package_path.to_owned(),
        sha256: hex::encode(hasher.finalize()),
        size,
        data: PreparedEntryData::Snapshot {
            file: snapshot,
            source_path: resolved,
        },
        mode,
    })
}

fn prepared_bytes_entry(
    path: &str,
    bytes: Vec<u8>,
    mode: u32,
) -> Result<PreparedEntry, PluginPackageError> {
    let size = u64::try_from(bytes.len()).map_err(|_| {
        PluginPackageError::InvalidPackage(format!("metadata entry '{path}' is too large"))
    })?;
    if size > MAX_SMALL_METADATA_BYTES {
        return Err(PluginPackageError::InvalidPackage(format!(
            "metadata entry '{path}' exceeds {MAX_SMALL_METADATA_BYTES} bytes"
        )));
    }
    Ok(PreparedEntry {
        path: path.to_owned(),
        sha256: hex::encode(Sha256::digest(&bytes)),
        size,
        data: PreparedEntryData::Bytes(bytes),
        mode,
    })
}

fn canonical_checksums(checksums: &BTreeMap<String, String>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (path, checksum) in checksums {
        bytes.extend_from_slice(checksum.as_bytes());
        bytes.extend_from_slice(b"  ");
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(b'\n');
    }
    bytes
}

fn signature_message(checksums: &[u8]) -> Vec<u8> {
    let mut message = SIGNATURE_DOMAIN.to_vec();
    message.extend_from_slice(checksums);
    message
}

fn write_package_atomically(
    output: &Path,
    entries: &[PreparedEntry],
) -> Result<(), PluginPackageError> {
    validate_prepared_package_size(entries)?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(PluginPackageError::InvalidPackage(format!(
            "output directory '{}' is not a directory",
            parent.display()
        )));
    }
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|source| PluginPackageError::Io {
            operation: "create package staging file",
            path: parent.display().to_string(),
            source,
        })?;
    {
        let mut archive = ZipWriter::new(temporary.as_file_mut());
        for entry in entries {
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Stored)
                .last_modified_time(DateTime::default())
                .unix_permissions(entry.mode)
                .large_file(entry.size >= u32::MAX as u64);
            archive.start_file(&entry.path, options)?;
            match &entry.data {
                PreparedEntryData::Bytes(bytes) => {
                    archive
                        .write_all(bytes)
                        .map_err(|source| PluginPackageError::Io {
                            operation: "write package metadata",
                            path: entry.path.clone(),
                            source,
                        })?
                }
                PreparedEntryData::Snapshot { file, source_path } => {
                    let mut input = file.try_clone().map_err(|source| PluginPackageError::Io {
                        operation: "clone package input snapshot",
                        path: source_path.display().to_string(),
                        source,
                    })?;
                    input
                        .seek(SeekFrom::Start(0))
                        .map_err(|source| PluginPackageError::Io {
                            operation: "rewind package input snapshot",
                            path: source_path.display().to_string(),
                            source,
                        })?;
                    let copied = io::copy(&mut input, &mut archive).map_err(|source| {
                        PluginPackageError::Io {
                            operation: "write package input",
                            path: source_path.display().to_string(),
                            source,
                        }
                    })?;
                    if copied != entry.size {
                        return Err(PluginPackageError::InvalidPackage(format!(
                            "package input snapshot '{}' changed while writing",
                            source_path.display()
                        )));
                    }
                }
            }
        }
        archive.finish()?;
    }
    let staged_size = temporary
        .as_file()
        .metadata()
        .map_err(|source| PluginPackageError::Io {
            operation: "inspect package staging file",
            path: output.display().to_string(),
            source,
        })?
        .len();
    if staged_size > MAX_PLUGIN_PACKAGE_BYTES {
        return Err(PluginPackageError::InvalidPackage(format!(
            "generated package exceeds {MAX_PLUGIN_PACKAGE_BYTES} bytes"
        )));
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|source| PluginPackageError::Io {
            operation: "sync package staging file",
            path: output.display().to_string(),
            source,
        })?;
    temporary
        .persist(output)
        .map_err(|error| PluginPackageError::Io {
            operation: "atomically replace package",
            path: output.display().to_string(),
            source: error.error,
        })?;
    Ok(())
}

fn validate_prepared_package_size(entries: &[PreparedEntry]) -> Result<(), PluginPackageError> {
    if entries.len() > MAX_PLUGIN_PACKAGE_ENTRIES {
        return Err(PluginPackageError::InvalidPackage(format!(
            "package exceeds the {MAX_PLUGIN_PACKAGE_ENTRIES}-entry limit"
        )));
    }
    let total = entries.iter().try_fold(0_u64, |total, entry| {
        total.checked_add(entry.size).ok_or_else(|| {
            PluginPackageError::InvalidPackage("aggregate package input size overflow".to_owned())
        })
    })?;
    if total > MAX_PLUGIN_PACKAGE_BYTES {
        return Err(PluginPackageError::InvalidPackage(format!(
            "aggregate package input exceeds {MAX_PLUGIN_PACKAGE_BYTES} bytes"
        )));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, PluginPackageError> {
    let file = File::open(path).map_err(|source| PluginPackageError::Io {
        operation: "open file for hashing",
        path: path.display().to_string(),
        source,
    })?;
    sha256_open_file(&file, path)
}

fn sha256_open_file(file: &File, path: &Path) -> Result<String, PluginPackageError> {
    let mut file = file.try_clone().map_err(|source| PluginPackageError::Io {
        operation: "clone file handle for hashing",
        path: path.display().to_string(),
        source,
    })?;
    file.seek(SeekFrom::Start(0))
        .map_err(|source| PluginPackageError::Io {
            operation: "rewind file for hashing",
            path: path.display().to_string(),
            source,
        })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| PluginPackageError::Io {
                operation: "read file for hashing",
                path: path.display().to_string(),
                source,
            })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn validate_sha256(value: &str) -> Result<(), PluginPackageError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(PluginPackageError::InvalidPackage(
            "SHA-256 values must contain 64 lowercase hexadecimal characters".to_owned(),
        ));
    }
    Ok(())
}

fn validate_package_text(
    field: &str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), PluginPackageError> {
    if value.is_empty() || value.len() > maximum_bytes {
        return Err(PluginPackageError::InvalidPackage(format!(
            "{field} must contain 1 to {maximum_bytes} UTF-8 bytes"
        )));
    }
    Ok(())
}

impl VerifiedPluginPackage {
    pub fn manifest(&self) -> &PluginPackageManifest {
        &self.manifest
    }

    pub fn verification(&self) -> &PluginPackageVerification {
        &self.verification
    }
}

pub fn verify_signed_plugin_package(
    package_path: &Path,
    trust_store: &PluginTrustStore,
) -> Result<VerifiedPluginPackage, PluginPackageError> {
    let package_metadata =
        fs::symlink_metadata(package_path).map_err(|source| PluginPackageError::Io {
            operation: "inspect plugin package",
            path: package_path.display().to_string(),
            source,
        })?;
    if !package_metadata.file_type().is_file() {
        return Err(PluginPackageError::InvalidPackage(format!(
            "'{}' is not a regular non-symlink package file",
            package_path.display()
        )));
    }
    if package_metadata.len() > MAX_PLUGIN_PACKAGE_BYTES {
        return Err(PluginPackageError::InvalidPackage(format!(
            "package exceeds {MAX_PLUGIN_PACKAGE_BYTES} bytes"
        )));
    }
    let package_file = File::open(package_path).map_err(|source| PluginPackageError::Io {
        operation: "open plugin package",
        path: package_path.display().to_string(),
        source,
    })?;
    let archive_file = package_file
        .try_clone()
        .map_err(|source| PluginPackageError::Io {
            operation: "clone verified plugin package handle",
            path: package_path.display().to_string(),
            source,
        })?;
    let mut archive = ZipArchive::new(archive_file)?;
    if archive.len() < 4 || archive.len() > MAX_PLUGIN_PACKAGE_ENTRIES {
        return Err(PluginPackageError::InvalidPackage(format!(
            "archive must contain 4 to {MAX_PLUGIN_PACKAGE_ENTRIES} entries"
        )));
    }

    let mut normalized_paths = HashSet::with_capacity(archive.len());
    let mut archive_paths = HashSet::with_capacity(archive.len());
    let mut verified_entries = Vec::with_capacity(archive.len());
    let mut total_size = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let path = entry.name().to_owned();
        crate::plugin_project::validate_archive_path("archive entry", &path)?;
        if !entry.is_file() || entry.encrypted() {
            return Err(PluginPackageError::InvalidPackage(format!(
                "archive entry '{path}' must be an unencrypted regular file"
            )));
        }
        let unix_mode = entry.unix_mode().ok_or_else(|| {
            PluginPackageError::InvalidPackage(format!(
                "archive entry '{path}' is missing canonical Unix file metadata"
            ))
        })?;
        if unix_mode & UNIX_FILE_TYPE_MASK != UNIX_REGULAR_FILE {
            return Err(PluginPackageError::InvalidPackage(format!(
                "archive entry '{path}' has an unsupported Unix file type"
            )));
        }
        crate::plugin_project::insert_archive_file_path(&mut normalized_paths, &path)
            .map_err(|error| PluginPackageError::InvalidPackage(error.to_string()))?;
        archive_paths.insert(path.clone());
        if entry.size() > MAX_PLUGIN_PACKAGE_ENTRY_BYTES {
            return Err(PluginPackageError::InvalidPackage(format!(
                "archive entry '{path}' exceeds {MAX_PLUGIN_PACKAGE_ENTRY_BYTES} bytes"
            )));
        }
        total_size = total_size.checked_add(entry.size()).ok_or_else(|| {
            PluginPackageError::InvalidPackage("archive size sum overflowed".to_owned())
        })?;
        if total_size > MAX_PLUGIN_PACKAGE_BYTES {
            return Err(PluginPackageError::InvalidPackage(format!(
                "archive expands beyond {MAX_PLUGIN_PACKAGE_BYTES} bytes"
            )));
        }
        if entry.size() > MAX_SMALL_METADATA_BYTES
            && (entry.compressed_size() == 0
                || entry
                    .compressed_size()
                    .saturating_mul(MAX_COMPRESSION_RATIO)
                    < entry.size())
        {
            return Err(PluginPackageError::InvalidPackage(format!(
                "archive entry '{path}' exceeds the {MAX_COMPRESSION_RATIO}:1 compression ratio limit"
            )));
        }
        if matches!(
            path.as_str(),
            PLUGIN_PACKAGE_MANIFEST_PATH
                | PLUGIN_PACKAGE_CHECKSUMS_PATH
                | PLUGIN_PACKAGE_SIGNATURE_PATH
        ) && entry.size() > MAX_SMALL_METADATA_BYTES
        {
            return Err(PluginPackageError::InvalidPackage(format!(
                "metadata entry '{path}' exceeds {MAX_SMALL_METADATA_BYTES} bytes"
            )));
        }
        verified_entries.push(VerifiedPackageEntry {
            path,
            size: entry.size(),
            mode: unix_mode & 0o777,
            sha256: String::new(),
        });
    }

    for required in [
        PLUGIN_PACKAGE_MANIFEST_PATH,
        PLUGIN_PACKAGE_CHECKSUMS_PATH,
        PLUGIN_PACKAGE_SIGNATURE_PATH,
    ] {
        if !archive_paths.contains(required) {
            return Err(PluginPackageError::InvalidPackage(format!(
                "archive is missing required entry '{required}'"
            )));
        }
    }
    if archive_paths.contains(INSTALL_MARKER_PATH) {
        return Err(PluginPackageError::InvalidPackage(format!(
            "archive entry '{INSTALL_MARKER_PATH}' is reserved for installation metadata"
        )));
    }

    let checksums_bytes = read_bounded_zip_entry(
        &mut archive,
        PLUGIN_PACKAGE_CHECKSUMS_PATH,
        MAX_SMALL_METADATA_BYTES,
    )?;
    let checksums = parse_canonical_checksums(&checksums_bytes)?;
    let expected_checksum_paths = archive_paths
        .iter()
        .filter(|path| {
            path.as_str() != PLUGIN_PACKAGE_CHECKSUMS_PATH
                && path.as_str() != PLUGIN_PACKAGE_SIGNATURE_PATH
        })
        .cloned()
        .collect::<HashSet<_>>();
    let actual_checksum_paths = checksums.keys().cloned().collect::<HashSet<_>>();
    if expected_checksum_paths != actual_checksum_paths {
        return Err(PluginPackageError::InvalidPackage(
            "SHA256SUMS must cover every manifest and payload entry exactly once".to_owned(),
        ));
    }

    let manifest_bytes = read_bounded_zip_entry(
        &mut archive,
        PLUGIN_PACKAGE_MANIFEST_PATH,
        MAX_SMALL_METADATA_BYTES,
    )?;
    let mut manifest: PluginPackageManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| PluginPackageError::InvalidPackage(error.to_string()))?;
    let canonical_manifest = canonical_manifest_bytes(&mut manifest)?;
    if canonical_manifest != manifest_bytes {
        return Err(PluginPackageError::InvalidPackage(
            "manifest.json is not canonical JSON".to_owned(),
        ));
    }
    let artifact_paths = manifest
        .artifacts
        .iter()
        .map(|artifact| artifact.path.as_str())
        .collect::<HashSet<_>>();
    for entry in &verified_entries {
        let expected_mode = if artifact_paths.contains(entry.path.as_str()) {
            ARTIFACT_FILE_MODE
        } else {
            PACKAGE_METADATA_FILE_MODE
        };
        if entry.mode != expected_mode {
            return Err(PluginPackageError::InvalidPackage(format!(
                "archive entry '{}' has non-canonical permissions {:o}; expected {:o}",
                entry.path, entry.mode, expected_mode
            )));
        }
    }
    for artifact in &manifest.artifacts {
        let checksum = checksums.get(&artifact.path).ok_or_else(|| {
            PluginPackageError::InvalidPackage(format!(
                "artifact '{}' is absent from SHA256SUMS",
                artifact.path
            ))
        })?;
        if checksum != &artifact.sha256 {
            return Err(PluginPackageError::InvalidPackage(format!(
                "artifact '{}' hash disagrees with SHA256SUMS",
                artifact.path
            )));
        }
    }
    if !archive_paths
        .iter()
        .any(|path| path.starts_with("licenses/"))
        || !archive_paths
            .iter()
            .any(|path| path.starts_with("notices/"))
    {
        return Err(PluginPackageError::InvalidPackage(
            "archive must contain license and notice entries".to_owned(),
        ));
    }

    let signature_bytes = read_bounded_zip_entry(
        &mut archive,
        PLUGIN_PACKAGE_SIGNATURE_PATH,
        MAX_SMALL_METADATA_BYTES,
    )?;
    let signature: PluginPackageSignature = serde_json::from_slice(&signature_bytes)
        .map_err(|error| PluginPackageError::InvalidPackage(error.to_string()))?;
    if serde_json::to_vec(&signature)? != signature_bytes {
        return Err(PluginPackageError::InvalidPackage(
            "signature.json is not canonical JSON".to_owned(),
        ));
    }
    if signature.schema_version != SIGNATURE_SCHEMA_VERSION
        || signature.algorithm != SIGNATURE_ALGORITHM
    {
        return Err(PluginPackageError::InvalidSignature);
    }
    if signature.publisher != manifest.plugin.publisher {
        return Err(PluginPackageError::InvalidSignature);
    }
    validate_sha256(&signature.key_id)?;
    let signature_value =
        decode_hex::<64>(&signature.signature).map_err(|_| PluginPackageError::InvalidSignature)?;
    let verifying_key = trust_store.verifying_key(&signature.publisher, &signature.key_id)?;
    verifying_key
        .verify(
            &signature_message(&checksums_bytes),
            &Signature::from_bytes(&signature_value),
        )
        .map_err(|_| PluginPackageError::InvalidSignature)?;

    for entry in &mut verified_entries {
        let actual = sha256_zip_entry(&mut archive, &entry.path)?;
        if let Some(expected) = checksums.get(&entry.path)
            && &actual != expected
        {
            return Err(PluginPackageError::InvalidPackage(format!(
                "checksum mismatch for archive entry '{}'",
                entry.path
            )));
        }
        entry.sha256 = actual;
    }

    verified_entries.sort_by(|left, right| left.path.cmp(&right.path));
    let package_sha256 = sha256_open_file(&package_file, package_path)?;
    let verification = PluginPackageVerification {
        package_path: package_path.to_path_buf(),
        plugin_id: manifest.plugin.id.clone(),
        version: manifest.plugin.version.clone(),
        publisher: manifest.plugin.publisher.clone(),
        key_id: signature.key_id,
        artifact_count: manifest.artifacts.len(),
        package_sha256,
    };
    Ok(VerifiedPluginPackage {
        package_file: Arc::new(package_file),
        manifest,
        verification,
        entries: verified_entries,
    })
}

pub fn install_verified_plugin_package(
    verified: &VerifiedPluginPackage,
    install_root: &Path,
) -> Result<PluginInstallationReport, PluginPackageError> {
    ensure_directory(install_root, "plugin install root")?;
    let _catalog_lock = PluginCatalogLock::acquire(install_root)?;
    let plugin_root = install_root.join(&verified.manifest.plugin.id);
    let mut identity_rollback = None;
    if plugin_root.exists() {
        require_existing_directory(&plugin_root, "plugin install identity directory")?;
    } else {
        ensure_plugin_identity_capacity(install_root)?;
        fs::create_dir(&plugin_root).map_err(|source| PluginPackageError::Io {
            operation: "create plugin install identity directory",
            path: plugin_root.display().to_string(),
            source,
        })?;
        identity_rollback = Some(EmptyIdentityDirectoryRollback::new(plugin_root.clone()));
        sync_directory(install_root)?;
    }
    let target = plugin_root.join(&verified.manifest.plugin.version);
    if target.exists() {
        require_existing_directory(&target, "existing plugin installation")?;
        let marker = read_bounded_file(
            &target.join(INSTALL_MARKER_PATH),
            65,
            "installed package marker",
        )?;
        let marker = std::str::from_utf8(&marker)
            .map_err(|_| {
                PluginPackageError::InvalidPackage(
                    "installed package marker is not UTF-8".to_owned(),
                )
            })?
            .trim_end_matches('\n');
        if marker == verified.verification.package_sha256 {
            return Ok(PluginInstallationReport {
                plugin_id: verified.manifest.plugin.id.clone(),
                version: verified.manifest.plugin.version.clone(),
                install_path: target.clone(),
                package_sha256: verified.verification.package_sha256.clone(),
                already_installed: true,
            });
        }
        return Err(PluginPackageError::InvalidPackage(format!(
            "plugin '{}' version '{}' is already installed from a different package",
            verified.manifest.plugin.id, verified.manifest.plugin.version
        )));
    }
    ensure_plugin_version_capacity(&plugin_root, &verified.manifest.plugin.id)?;

    let staging = tempfile::Builder::new()
        .prefix(".vesper-staging-")
        .tempdir_in(&plugin_root)
        .map_err(|source| PluginPackageError::Io {
            operation: "create plugin install staging directory",
            path: plugin_root.display().to_string(),
            source,
        })?;
    extract_verified_entries(verified, staging.path())?;
    let marker_path = staging.path().join(INSTALL_MARKER_PATH);
    let mut marker = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker_path)
        .map_err(|source| PluginPackageError::Io {
            operation: "create installed package marker",
            path: marker_path.display().to_string(),
            source,
        })?;
    marker
        .write_all(verified.verification.package_sha256.as_bytes())
        .and_then(|()| marker.write_all(b"\n"))
        .and_then(|()| marker.sync_all())
        .map_err(|source| PluginPackageError::Io {
            operation: "write installed package marker",
            path: marker_path.display().to_string(),
            source,
        })?;
    sync_directory(staging.path())?;
    fs::rename(staging.path(), &target).map_err(|source| PluginPackageError::Io {
        operation: "atomically promote plugin installation",
        path: target.display().to_string(),
        source,
    })?;
    if let Some(rollback) = &mut identity_rollback {
        rollback.disarm();
    }
    sync_directory(&plugin_root)?;
    sync_directory(install_root)?;
    Ok(PluginInstallationReport {
        plugin_id: verified.manifest.plugin.id.clone(),
        version: verified.manifest.plugin.version.clone(),
        install_path: target,
        package_sha256: verified.verification.package_sha256.clone(),
        already_installed: false,
    })
}

pub fn list_installed_plugins(
    install_root: &Path,
) -> Result<Vec<InstalledPluginRecord>, PluginPackageError> {
    if !install_root.exists() {
        return Ok(Vec::new());
    }
    require_existing_directory(install_root, "plugin install root")?;
    let _catalog_lock = PluginCatalogLock::acquire_shared(install_root)?;
    let mut records = Vec::new();
    let mut identity_entry_count = 0_usize;
    for plugin_entry in read_directory(install_root)? {
        let plugin_entry = read_directory_entry(plugin_entry, install_root)?;
        if plugin_entry.file_name() == OsStr::new(CATALOG_LOCK_PATH) {
            continue;
        }
        identity_entry_count += 1;
        if identity_entry_count > MAX_INSTALLED_PLUGIN_IDENTITIES {
            return Err(PluginPackageError::InvalidPackage(format!(
                "plugin install root exceeds {MAX_INSTALLED_PLUGIN_IDENTITIES} entries"
            )));
        }
        let plugin_path = plugin_entry.path();
        if !plugin_entry
            .file_type()
            .map_err(|source| PluginPackageError::Io {
                operation: "inspect installed plugin identity",
                path: plugin_path.display().to_string(),
                source,
            })?
            .is_dir()
        {
            continue;
        }
        let plugin_id = plugin_entry.file_name().to_string_lossy().into_owned();
        if validate_reverse_dns_identifier(&plugin_id).is_err() {
            continue;
        }
        let mut version_entry_count = 0_usize;
        for version_entry in read_directory(&plugin_path)? {
            version_entry_count += 1;
            if version_entry_count > MAX_INSTALLED_VERSIONS_PER_PLUGIN {
                return Err(PluginPackageError::InvalidPackage(format!(
                    "installed plugin '{plugin_id}' exceeds {MAX_INSTALLED_VERSIONS_PER_PLUGIN} version entries"
                )));
            }
            let version_entry = read_directory_entry(version_entry, &plugin_path)?;
            let version_path = version_entry.path();
            if !version_entry
                .file_type()
                .map_err(|source| PluginPackageError::Io {
                    operation: "inspect installed plugin version",
                    path: version_path.display().to_string(),
                    source,
                })?
                .is_dir()
            {
                continue;
            }
            let version = version_entry.file_name().to_string_lossy().into_owned();
            if Version::parse(&version).is_err() {
                continue;
            }
            let marker = read_bounded_file(
                &version_path.join(INSTALL_MARKER_PATH),
                65,
                "package marker",
            )?;
            let package_sha256 = std::str::from_utf8(&marker)
                .map_err(|_| {
                    PluginPackageError::InvalidPackage(format!(
                        "installed marker for '{plugin_id}' version '{version}' is not UTF-8"
                    ))
                })?
                .trim_end_matches('\n')
                .to_owned();
            validate_sha256(&package_sha256)?;
            records.push(InstalledPluginRecord {
                plugin_id: plugin_id.clone(),
                version,
                install_path: version_path,
                package_sha256,
            });
        }
    }
    records.sort_by(|left, right| {
        (&left.plugin_id, &left.version).cmp(&(&right.plugin_id, &right.version))
    });
    Ok(records)
}

struct VerifiedInstalledVersion {
    manifest: PluginPackageManifest,
    snapshots: BTreeMap<String, Arc<tempfile::NamedTempFile>>,
}

struct InstalledFileLayout {
    files: BTreeMap<String, PathBuf>,
    directories: BTreeSet<String>,
}

pub fn verify_installed_plugin_catalog(
    install_root: &Path,
    trust_store: &PluginTrustStore,
    host: &PluginHostTarget,
    references: &[PluginReference],
    activations: &[InstalledPluginActivation],
) -> Result<VerifiedInstalledPluginCatalog, PluginPackageError> {
    let mut requested = BTreeMap::<String, Vec<&PluginReference>>::new();
    for reference in references {
        requested
            .entry(reference.plugin_id().to_owned())
            .or_default()
            .push(reference);
    }
    if requested.is_empty() {
        if activations.is_empty() {
            return Ok(VerifiedInstalledPluginCatalog {
                _catalog_lock: None,
                artifacts: Vec::new(),
            });
        }
        return Err(PluginPackageError::InvalidPackage(
            "installed plugin activations require at least one explicit PluginReference".to_owned(),
        ));
    }

    require_existing_directory(install_root, "plugin install root")?;
    let catalog_lock = PluginCatalogLock::acquire_shared(install_root)?;
    let mut activation_versions = BTreeMap::new();
    for activation in activations {
        if !requested.contains_key(activation.plugin_id()) {
            return Err(PluginPackageError::InvalidPackage(format!(
                "activation for unrequested plugin '{}' is not allowed",
                activation.plugin_id()
            )));
        }
        if activation_versions
            .insert(
                activation.plugin_id().to_owned(),
                activation.version().to_owned(),
            )
            .is_some()
        {
            return Err(PluginPackageError::InvalidPackage(format!(
                "duplicate activation for plugin '{}'",
                activation.plugin_id()
            )));
        }
    }

    let mut artifacts = Vec::new();
    for (plugin_id, plugin_references) in requested {
        let plugin_root = install_root.join(&plugin_id);
        require_existing_directory(&plugin_root, "installed plugin identity")?;
        let version = select_installed_version(
            &plugin_root,
            &plugin_id,
            activation_versions.get(&plugin_id).map(String::as_str),
        )?;
        let version_root = plugin_root.join(&version);
        let verified = verify_installed_version(&version_root, &plugin_id, &version, trust_store)?;
        let descriptor = PluginDescriptor {
            schema_version: verified.manifest.schema_version,
            plugin: verified.manifest.plugin.clone(),
            compatibility: verified.manifest.compatibility.clone(),
            capabilities: verified.manifest.capabilities.clone(),
            redistribution: verified.manifest.redistribution.clone(),
        };
        descriptor
            .evaluate_current_host_compatibility(host.host_sdk())
            .map_err(|error| PluginPackageError::Compatibility(error.to_string()))?;

        let mut requested_transports = HashSet::new();
        for reference in &plugin_references {
            requested_transports.insert(reference.transport());
        }
        for requested_transport in requested_transports {
            let transport = artifact_transport(requested_transport);
            let expected_format = match transport {
                PluginArtifactTransport::Native => PluginArtifactFormat::Dylib,
                PluginArtifactTransport::Wasm => PluginArtifactFormat::WasmComponent,
            };
            let (artifact_target, artifact_architecture) = match transport {
                PluginArtifactTransport::Native => (host.target(), host.architecture()),
                PluginArtifactTransport::Wasm => {
                    (RUST_WASM_COMPONENT_TARGET, RUST_WASM_COMPONENT_ARCHITECTURE)
                }
            };
            let matching = verified
                .manifest
                .artifacts
                .iter()
                .filter(|artifact| {
                    artifact.transport == transport
                        && artifact.format == expected_format
                        && artifact.target == artifact_target
                        && artifact.architecture == artifact_architecture
                })
                .collect::<Vec<_>>();
            let artifact = match matching.as_slice() {
                [artifact] => *artifact,
                [] => {
                    return Err(PluginPackageError::InstalledArtifactNotFound {
                        plugin_id: plugin_id.clone(),
                        transport: transport.as_str(),
                        target: artifact_target.to_owned(),
                        architecture: artifact_architecture.to_owned(),
                    });
                }
                _ => {
                    return Err(PluginPackageError::InvalidPackage(format!(
                        "installed plugin '{plugin_id}' has ambiguous {} artifacts for target '{}:{}'",
                        transport.as_str(),
                        artifact_target,
                        artifact_architecture
                    )));
                }
            };
            for reference in plugin_references
                .iter()
                .filter(|reference| reference.transport() == requested_transport)
            {
                if let Some(instance_id) = reference.capability_instance_id()
                    && !artifact
                        .capabilities
                        .iter()
                        .any(|capability| capability.instance_id == instance_id)
                {
                    return Err(PluginPackageError::InvalidPackage(format!(
                        "installed artifact '{}' does not declare requested capability instance '{instance_id}'",
                        artifact.path
                    )));
                }
            }
            let snapshot = verified.snapshots.get(&artifact.path).ok_or_else(|| {
                PluginPackageError::InvalidPackage(format!(
                    "installed artifact '{}' has no verified snapshot",
                    artifact.path
                ))
            })?;
            let capabilities = artifact
                .capabilities
                .iter()
                .map(|reference| {
                    verified
                        .manifest
                        .capabilities
                        .iter()
                        .find(|capability| {
                            capability.interface_id == reference.interface_id
                                && capability.instance_id == reference.instance_id
                        })
                        .cloned()
                        .ok_or_else(|| {
                            PluginPackageError::InvalidPackage(format!(
                                "installed artifact capability '{}:{}' is absent from the descriptor",
                                reference.interface_id, reference.instance_id
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            artifacts.push(VerifiedInstalledArtifact {
                plugin_id: plugin_id.clone(),
                version: version.clone(),
                transport: artifact.transport,
                target: artifact.target.clone(),
                format: artifact.format,
                architecture: artifact.architecture.clone(),
                capabilities,
                minimum_os: artifact.minimum_os.clone(),
                runtime_dependencies: artifact.runtime_dependencies.clone(),
                installed_path: version_root.join(&artifact.path),
                sha256: artifact.sha256.clone(),
                snapshot: Arc::clone(snapshot),
            });
        }
    }
    artifacts.sort_by(|left, right| {
        (
            &left.plugin_id,
            left.transport.as_str(),
            &left.target,
            &left.architecture,
        )
            .cmp(&(
                &right.plugin_id,
                right.transport.as_str(),
                &right.target,
                &right.architecture,
            ))
    });
    Ok(VerifiedInstalledPluginCatalog {
        _catalog_lock: Some(catalog_lock),
        artifacts,
    })
}

fn select_installed_version(
    plugin_root: &Path,
    plugin_id: &str,
    activation: Option<&str>,
) -> Result<String, PluginPackageError> {
    let mut versions = Vec::new();
    for entry in read_directory(plugin_root)? {
        if versions.len() >= MAX_INSTALLED_VERSIONS_PER_PLUGIN {
            return Err(PluginPackageError::InvalidPackage(format!(
                "installed plugin '{plugin_id}' exceeds {MAX_INSTALLED_VERSIONS_PER_PLUGIN} version entries"
            )));
        }
        let entry = read_directory_entry(entry, plugin_root)?;
        let path = entry.path();
        if !entry
            .file_type()
            .map_err(|source| PluginPackageError::Io {
                operation: "inspect installed plugin version",
                path: path.display().to_string(),
                source,
            })?
            .is_dir()
        {
            return Err(PluginPackageError::InvalidPackage(format!(
                "installed plugin '{plugin_id}' contains a non-directory version entry '{}'",
                path.display()
            )));
        }
        let version = entry.file_name().into_string().map_err(|_| {
            PluginPackageError::InvalidPackage(format!(
                "installed plugin '{plugin_id}' contains a non-UTF-8 version"
            ))
        })?;
        Version::parse(&version).map_err(|error| {
            PluginPackageError::InvalidPackage(format!(
                "installed plugin '{plugin_id}' contains invalid version '{version}': {error}"
            ))
        })?;
        versions.push(version);
    }
    versions.sort();
    if let Some(activation) = activation {
        if versions.iter().any(|version| version == activation) {
            return Ok(activation.to_owned());
        }
        return Err(PluginPackageError::InstalledVersionNotFound {
            plugin_id: plugin_id.to_owned(),
            version: activation.to_owned(),
        });
    }
    match versions.as_slice() {
        [version] => Ok(version.clone()),
        [] => Err(PluginPackageError::InstalledVersionNotFound {
            plugin_id: plugin_id.to_owned(),
            version: "<any>".to_owned(),
        }),
        _ => Err(PluginPackageError::AmbiguousInstalledVersions {
            plugin_id: plugin_id.to_owned(),
            versions,
        }),
    }
}

fn verify_installed_version(
    version_root: &Path,
    expected_plugin_id: &str,
    expected_version: &str,
    trust_store: &PluginTrustStore,
) -> Result<VerifiedInstalledVersion, PluginPackageError> {
    require_existing_directory(version_root, "installed plugin version")?;
    let layout = collect_installed_file_layout(version_root)?;
    let manifest_bytes = read_bounded_file(
        &version_root.join(PLUGIN_PACKAGE_MANIFEST_PATH),
        MAX_SMALL_METADATA_BYTES,
        "installed plugin manifest",
    )?;
    let mut manifest: PluginPackageManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| PluginPackageError::InvalidPackage(error.to_string()))?;
    if canonical_manifest_bytes(&mut manifest)? != manifest_bytes {
        return Err(PluginPackageError::InvalidPackage(
            "installed manifest.json is not canonical JSON".to_owned(),
        ));
    }
    if manifest.plugin.id != expected_plugin_id || manifest.plugin.version != expected_version {
        return Err(PluginPackageError::InvalidPackage(format!(
            "installed manifest identity '{}:{}' does not match directory '{}:{}'",
            manifest.plugin.id, manifest.plugin.version, expected_plugin_id, expected_version
        )));
    }

    let checksums_bytes = read_bounded_file(
        &version_root.join(PLUGIN_PACKAGE_CHECKSUMS_PATH),
        MAX_SMALL_METADATA_BYTES,
        "installed plugin checksums",
    )?;
    let checksums = parse_canonical_checksums(&checksums_bytes)?;
    let signature_bytes = read_bounded_file(
        &version_root.join(PLUGIN_PACKAGE_SIGNATURE_PATH),
        MAX_SMALL_METADATA_BYTES,
        "installed plugin signature",
    )?;
    let signature: PluginPackageSignature = serde_json::from_slice(&signature_bytes)
        .map_err(|error| PluginPackageError::InvalidPackage(error.to_string()))?;
    if serde_json::to_vec(&signature)? != signature_bytes
        || signature.schema_version != SIGNATURE_SCHEMA_VERSION
        || signature.algorithm != SIGNATURE_ALGORITHM
        || signature.publisher != manifest.plugin.publisher
    {
        return Err(PluginPackageError::InvalidSignature);
    }
    validate_sha256(&signature.key_id)?;
    let signature_value =
        decode_hex::<64>(&signature.signature).map_err(|_| PluginPackageError::InvalidSignature)?;
    trust_store
        .verifying_key(&signature.publisher, &signature.key_id)?
        .verify(
            &signature_message(&checksums_bytes),
            &Signature::from_bytes(&signature_value),
        )
        .map_err(|_| PluginPackageError::InvalidSignature)?;

    let marker = read_bounded_file(
        &version_root.join(INSTALL_MARKER_PATH),
        65,
        "installed package marker",
    )?;
    if marker.len() != 65 || marker.last() != Some(&b'\n') {
        return Err(PluginPackageError::InvalidPackage(
            "installed package marker must be one SHA-256 followed by a newline".to_owned(),
        ));
    }
    let marker_hash = std::str::from_utf8(&marker[..64]).map_err(|_| {
        PluginPackageError::InvalidPackage("installed package marker is not UTF-8".to_owned())
    })?;
    validate_sha256(marker_hash)?;

    let mut expected_files = checksums.keys().cloned().collect::<BTreeSet<_>>();
    expected_files.insert(PLUGIN_PACKAGE_CHECKSUMS_PATH.to_owned());
    expected_files.insert(PLUGIN_PACKAGE_SIGNATURE_PATH.to_owned());
    expected_files.insert(INSTALL_MARKER_PATH.to_owned());
    let actual_files = layout.files.keys().cloned().collect::<BTreeSet<_>>();
    if actual_files != expected_files {
        return Err(PluginPackageError::InvalidPackage(
            "installed plugin files do not exactly match SHA256SUMS and installation metadata"
                .to_owned(),
        ));
    }
    let expected_directories = installed_parent_directories(&expected_files);
    if layout.directories != expected_directories {
        return Err(PluginPackageError::InvalidPackage(
            "installed plugin contains unexpected or missing package directories".to_owned(),
        ));
    }
    if !checksums.contains_key(PLUGIN_PACKAGE_MANIFEST_PATH)
        || !checksums.keys().any(|path| path.starts_with("licenses/"))
        || !checksums.keys().any(|path| path.starts_with("notices/"))
    {
        return Err(PluginPackageError::InvalidPackage(
            "installed plugin must contain checksummed manifest, license, and notice files"
                .to_owned(),
        ));
    }

    let artifact_paths = manifest
        .artifacts
        .iter()
        .map(|artifact| artifact.path.as_str())
        .collect::<HashSet<_>>();
    for artifact in &manifest.artifacts {
        if checksums.get(&artifact.path) != Some(&artifact.sha256) {
            return Err(PluginPackageError::InvalidPackage(format!(
                "installed artifact '{}' hash disagrees with SHA256SUMS",
                artifact.path
            )));
        }
    }
    verify_installed_permissions(&layout.files, &artifact_paths)?;

    let mut snapshots = BTreeMap::new();
    for (relative_path, expected_hash) in &checksums {
        let installed_path = layout.files.get(relative_path).ok_or_else(|| {
            PluginPackageError::InvalidPackage(format!(
                "installed plugin is missing checksummed file '{relative_path}'"
            ))
        })?;
        let snapshot_required = artifact_paths.contains(relative_path.as_str());
        let (actual_hash, snapshot) =
            hash_and_snapshot_installed_file(installed_path, snapshot_required)?;
        if &actual_hash != expected_hash {
            return Err(PluginPackageError::InvalidPackage(format!(
                "checksum mismatch for installed file '{relative_path}'"
            )));
        }
        if let Some(snapshot) = snapshot {
            snapshots.insert(relative_path.clone(), Arc::new(snapshot));
        }
    }
    Ok(VerifiedInstalledVersion {
        manifest,
        snapshots,
    })
}

fn collect_installed_file_layout(
    version_root: &Path,
) -> Result<InstalledFileLayout, PluginPackageError> {
    let maximum_directories = MAX_PLUGIN_PACKAGE_ENTRIES
        .saturating_mul(crate::plugin_project::MAX_ARCHIVE_PATH_BYTES / 2);
    let mut files = BTreeMap::new();
    let mut directories = BTreeSet::new();
    let mut normalized_paths = HashSet::new();
    let mut pending = vec![(version_root.to_path_buf(), String::new())];
    let mut total_size = 0_u64;
    while let Some((directory, relative_directory)) = pending.pop() {
        for entry in read_directory(&directory)? {
            let entry = read_directory_entry(entry, &directory)?;
            let name = entry.file_name().into_string().map_err(|_| {
                PluginPackageError::InvalidPackage(format!(
                    "installed plugin path below '{}' is not UTF-8",
                    version_root.display()
                ))
            })?;
            let relative_path = if relative_directory.is_empty() {
                name
            } else {
                format!("{relative_directory}/{name}")
            };
            crate::plugin_project::validate_archive_path("installed plugin path", &relative_path)?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|source| PluginPackageError::Io {
                operation: "inspect installed plugin entry",
                path: path.display().to_string(),
                source,
            })?;
            if file_type.is_dir() {
                if !directories.insert(relative_path.clone())
                    || directories.len() > maximum_directories
                {
                    return Err(PluginPackageError::InvalidPackage(
                        "installed plugin directory layout exceeds package limits".to_owned(),
                    ));
                }
                pending.push((path, relative_path));
                continue;
            }
            if !file_type.is_file() {
                return Err(PluginPackageError::InvalidPackage(format!(
                    "installed plugin entry '{}' is not a regular non-symlink file",
                    path.display()
                )));
            }
            crate::plugin_project::insert_archive_file_path(&mut normalized_paths, &relative_path)?;
            if files.len() > MAX_PLUGIN_PACKAGE_ENTRIES {
                return Err(PluginPackageError::InvalidPackage(format!(
                    "installed plugin exceeds {} files",
                    MAX_PLUGIN_PACKAGE_ENTRIES + 1
                )));
            }
            let metadata = entry.metadata().map_err(|source| PluginPackageError::Io {
                operation: "inspect installed plugin file",
                path: path.display().to_string(),
                source,
            })?;
            if metadata.len() > MAX_PLUGIN_PACKAGE_ENTRY_BYTES {
                return Err(PluginPackageError::InvalidPackage(format!(
                    "installed plugin file '{}' exceeds {MAX_PLUGIN_PACKAGE_ENTRY_BYTES} bytes",
                    path.display()
                )));
            }
            total_size = total_size.checked_add(metadata.len()).ok_or_else(|| {
                PluginPackageError::InvalidPackage(
                    "installed plugin aggregate file size overflowed".to_owned(),
                )
            })?;
            if total_size > MAX_PLUGIN_PACKAGE_BYTES {
                return Err(PluginPackageError::InvalidPackage(format!(
                    "installed plugin exceeds {MAX_PLUGIN_PACKAGE_BYTES} aggregate bytes"
                )));
            }
            files.insert(relative_path, path);
        }
    }
    Ok(InstalledFileLayout { files, directories })
}

fn installed_parent_directories(files: &BTreeSet<String>) -> BTreeSet<String> {
    let mut directories = BTreeSet::new();
    for file in files {
        let mut parent = Path::new(file).parent();
        while let Some(path) = parent {
            if path.as_os_str().is_empty() {
                break;
            }
            directories.insert(path.to_string_lossy().into_owned());
            parent = path.parent();
        }
    }
    directories
}

#[cfg(unix)]
fn verify_installed_permissions(
    files: &BTreeMap<String, PathBuf>,
    artifact_paths: &HashSet<&str>,
) -> Result<(), PluginPackageError> {
    use std::os::unix::fs::PermissionsExt;

    for (relative_path, path) in files {
        if relative_path == INSTALL_MARKER_PATH {
            continue;
        }
        let actual = fs::metadata(path)
            .map_err(|source| PluginPackageError::Io {
                operation: "inspect installed plugin permissions",
                path: path.display().to_string(),
                source,
            })?
            .permissions()
            .mode()
            & 0o777;
        let expected = if artifact_paths.contains(relative_path.as_str()) {
            ARTIFACT_FILE_MODE
        } else {
            PACKAGE_METADATA_FILE_MODE
        };
        if actual != expected {
            return Err(PluginPackageError::InvalidPackage(format!(
                "installed file '{relative_path}' has permissions {actual:o}; expected {expected:o}"
            )));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_installed_permissions(
    _files: &BTreeMap<String, PathBuf>,
    _artifact_paths: &HashSet<&str>,
) -> Result<(), PluginPackageError> {
    Ok(())
}

fn hash_and_snapshot_installed_file(
    path: &Path,
    snapshot_required: bool,
) -> Result<(String, Option<tempfile::NamedTempFile>), PluginPackageError> {
    let mut input = File::open(path).map_err(|source| PluginPackageError::Io {
        operation: "open installed plugin file",
        path: path.display().to_string(),
        source,
    })?;
    let metadata = input.metadata().map_err(|source| PluginPackageError::Io {
        operation: "inspect opened installed plugin file",
        path: path.display().to_string(),
        source,
    })?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_PLUGIN_PACKAGE_ENTRY_BYTES {
        return Err(PluginPackageError::InvalidPackage(format!(
            "installed plugin file '{}' did not open as a bounded regular file",
            path.display()
        )));
    }
    let suffix = path
        .extension()
        .and_then(OsStr::to_str)
        .map(|extension| format!(".{extension}"))
        .unwrap_or_default();
    let mut snapshot = if snapshot_required {
        Some(
            tempfile::Builder::new()
                .prefix("vesper-verified-plugin-")
                .suffix(&suffix)
                .tempfile()
                .map_err(|source| PluginPackageError::Io {
                    operation: "create verified plugin snapshot",
                    path: path.display().to_string(),
                    source,
                })?,
        )
    } else {
        None
    };
    let mut hasher = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|source| PluginPackageError::Io {
                operation: "read installed plugin file",
                path: path.display().to_string(),
                source,
            })?;
        if read == 0 {
            break;
        }
        copied = copied.checked_add(read as u64).ok_or_else(|| {
            PluginPackageError::InvalidPackage(
                "installed plugin file size overflowed while hashing".to_owned(),
            )
        })?;
        if copied > metadata.len() || copied > MAX_PLUGIN_PACKAGE_ENTRY_BYTES {
            return Err(PluginPackageError::InvalidPackage(format!(
                "installed plugin file '{}' changed size while hashing",
                path.display()
            )));
        }
        hasher.update(&buffer[..read]);
        if let Some(snapshot) = snapshot.as_mut() {
            snapshot
                .as_file_mut()
                .write_all(&buffer[..read])
                .map_err(|source| PluginPackageError::Io {
                    operation: "write verified plugin snapshot",
                    path: path.display().to_string(),
                    source,
                })?;
        }
    }
    if copied != metadata.len() {
        return Err(PluginPackageError::InvalidPackage(format!(
            "installed plugin file '{}' changed size while hashing",
            path.display()
        )));
    }
    if let Some(snapshot) = snapshot.as_mut() {
        #[cfg(unix)]
        snapshot
            .as_file()
            .set_permissions(fs::Permissions::from_mode(ARTIFACT_FILE_MODE))
            .map_err(|source| PluginPackageError::Io {
                operation: "set verified plugin snapshot permissions",
                path: snapshot.path().display().to_string(),
                source,
            })?;
        snapshot
            .as_file_mut()
            .seek(SeekFrom::Start(0))
            .map_err(|source| PluginPackageError::Io {
                operation: "rewind verified plugin snapshot",
                path: snapshot.path().display().to_string(),
                source,
            })?;
        snapshot
            .as_file()
            .sync_all()
            .map_err(|source| PluginPackageError::Io {
                operation: "sync verified plugin snapshot",
                path: snapshot.path().display().to_string(),
                source,
            })?;
    }
    Ok((hex::encode(hasher.finalize()), snapshot))
}

const fn artifact_transport(transport: PluginTransport) -> PluginArtifactTransport {
    match transport {
        PluginTransport::Native => PluginArtifactTransport::Native,
        PluginTransport::Wasm => PluginArtifactTransport::Wasm,
    }
}

pub fn uninstall_plugin(
    install_root: &Path,
    plugin_id: &str,
    version: &str,
) -> Result<bool, PluginPackageError> {
    validate_reverse_dns_identifier(plugin_id).map_err(PluginPackageError::InvalidPackage)?;
    Version::parse(version).map_err(|error| {
        PluginPackageError::InvalidPackage(format!("invalid plugin version: {error}"))
    })?;
    if !install_root.exists() {
        return Ok(false);
    }
    require_existing_directory(install_root, "plugin install root")?;
    let _catalog_lock = PluginCatalogLock::acquire(install_root)?;
    let plugin_root = install_root.join(plugin_id);
    if !plugin_root.exists() {
        return Ok(false);
    }
    require_existing_directory(&plugin_root, "plugin install identity directory")?;
    let target = plugin_root.join(version);
    if !target.exists() {
        return Ok(false);
    }
    require_existing_directory(&target, "plugin uninstall target")?;
    let marker_path = target.join(INSTALL_MARKER_PATH);
    let marker = read_bounded_file(&marker_path, 65, "installed package marker")?;
    let marker = std::str::from_utf8(&marker).map_err(|_| {
        PluginPackageError::InvalidPackage("installed package marker is not UTF-8".to_owned())
    })?;
    validate_sha256(marker.trim_end_matches('\n'))?;
    fs::remove_dir_all(&target).map_err(|source| PluginPackageError::Io {
        operation: "remove installed plugin version",
        path: target.display().to_string(),
        source,
    })?;
    sync_directory(&plugin_root)?;
    if let Some(plugin_root) = target.parent()
        && fs::read_dir(plugin_root)
            .map_err(|source| PluginPackageError::Io {
                operation: "inspect plugin identity directory",
                path: plugin_root.display().to_string(),
                source,
            })?
            .next()
            .is_none()
    {
        fs::remove_dir(plugin_root).map_err(|source| PluginPackageError::Io {
            operation: "remove empty plugin identity directory",
            path: plugin_root.display().to_string(),
            source,
        })?;
    }
    sync_directory(install_root)?;
    Ok(true)
}

fn extract_verified_entries(
    verified: &VerifiedPluginPackage,
    destination: &Path,
) -> Result<(), PluginPackageError> {
    let reader = PositionedFile::new(Arc::clone(&verified.package_file));
    let mut archive = ZipArchive::new(reader)?;
    for metadata in &verified.entries {
        let output = destination.join(Path::new(&metadata.path));
        let parent = output.parent().ok_or_else(|| {
            PluginPackageError::InvalidPackage(format!(
                "archive entry '{}' has no parent directory",
                metadata.path
            ))
        })?;
        fs::create_dir_all(parent).map_err(|source| PluginPackageError::Io {
            operation: "create plugin install directory",
            path: parent.display().to_string(),
            source,
        })?;
        let mut input = archive.by_name(&metadata.path)?;
        let mut output_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
            .map_err(|source| PluginPackageError::Io {
                operation: "create installed package entry",
                path: output.display().to_string(),
                source,
            })?;
        let mut copied = 0_u64;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = input
                .read(&mut buffer)
                .map_err(|source| PluginPackageError::Io {
                    operation: "read verified package entry",
                    path: metadata.path.clone(),
                    source,
                })?;
            if read == 0 {
                break;
            }
            copied = copied.checked_add(read as u64).ok_or_else(|| {
                PluginPackageError::InvalidPackage(format!(
                    "archive entry '{}' size overflowed during extraction",
                    metadata.path
                ))
            })?;
            if copied > metadata.size {
                return Err(PluginPackageError::InvalidPackage(format!(
                    "archive entry '{}' grew after verification",
                    metadata.path
                )));
            }
            output_file
                .write_all(&buffer[..read])
                .map_err(|source| PluginPackageError::Io {
                    operation: "extract verified package entry",
                    path: metadata.path.clone(),
                    source,
                })?;
            hasher.update(&buffer[..read]);
        }
        if copied != metadata.size {
            return Err(PluginPackageError::InvalidPackage(format!(
                "archive entry '{}' changed size during extraction",
                metadata.path
            )));
        }
        let extracted_sha256 = hex::encode(hasher.finalize());
        if extracted_sha256 != metadata.sha256 {
            return Err(PluginPackageError::InvalidPackage(format!(
                "archive entry '{}' changed after verification",
                metadata.path
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output_file
                .set_permissions(fs::Permissions::from_mode(metadata.mode))
                .map_err(|source| PluginPackageError::Io {
                    operation: "set installed package entry permissions",
                    path: output.display().to_string(),
                    source,
                })?;
        }
        output_file
            .sync_all()
            .map_err(|source| PluginPackageError::Io {
                operation: "sync installed package entry",
                path: output.display().to_string(),
                source,
            })?;
    }
    sync_extracted_directories(&verified.entries, destination)?;
    Ok(())
}

fn sync_extracted_directories(
    entries: &[VerifiedPackageEntry],
    destination: &Path,
) -> Result<(), PluginPackageError> {
    let mut directories = BTreeSet::new();
    for entry in entries {
        let mut parent = Path::new(&entry.path).parent();
        while let Some(relative) = parent {
            if relative.as_os_str().is_empty() {
                break;
            }
            directories.insert(destination.join(relative));
            parent = relative.parent();
        }
    }
    let mut directories = directories.into_iter().collect::<Vec<_>>();
    directories.sort_by(|left, right| {
        right
            .components()
            .count()
            .cmp(&left.components().count())
            .then_with(|| left.cmp(right))
    });
    for directory in directories {
        sync_directory(&directory)?;
    }
    Ok(())
}

fn ensure_directory(path: &Path, label: &str) -> Result<(), PluginPackageError> {
    if !path.exists() {
        fs::create_dir_all(path).map_err(|source| PluginPackageError::Io {
            operation: "create directory",
            path: path.display().to_string(),
            source,
        })?;
    }
    require_existing_directory(path, label)
}

fn require_existing_directory(path: &Path, label: &str) -> Result<(), PluginPackageError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| PluginPackageError::Io {
        operation: "inspect directory",
        path: path.display().to_string(),
        source,
    })?;
    if !metadata.file_type().is_dir() {
        return Err(PluginPackageError::InvalidPackage(format!(
            "{label} '{}' is not a regular directory",
            path.display()
        )));
    }
    Ok(())
}

fn read_directory(path: &Path) -> Result<fs::ReadDir, PluginPackageError> {
    fs::read_dir(path).map_err(|source| PluginPackageError::Io {
        operation: "read install directory",
        path: path.display().to_string(),
        source,
    })
}

fn read_directory_entry(
    entry: Result<fs::DirEntry, io::Error>,
    parent: &Path,
) -> Result<fs::DirEntry, PluginPackageError> {
    entry.map_err(|source| PluginPackageError::Io {
        operation: "read install directory entry",
        path: parent.display().to_string(),
        source,
    })
}

fn ensure_plugin_identity_capacity(path: &Path) -> Result<(), PluginPackageError> {
    ensure_directory_entry_capacity(
        path,
        MAX_INSTALLED_PLUGIN_IDENTITIES,
        "plugin install root",
        Some(OsStr::new(CATALOG_LOCK_PATH)),
    )
}

fn ensure_plugin_version_capacity(path: &Path, plugin_id: &str) -> Result<(), PluginPackageError> {
    ensure_directory_entry_capacity(
        path,
        MAX_INSTALLED_VERSIONS_PER_PLUGIN,
        &format!("installed plugin '{plugin_id}'"),
        None,
    )
}

fn ensure_directory_entry_capacity(
    path: &Path,
    maximum_entries: usize,
    label: &str,
    ignored_entry_name: Option<&OsStr>,
) -> Result<(), PluginPackageError> {
    let mut entry_count = 0_usize;
    for entry in read_directory(path)? {
        let entry = read_directory_entry(entry, path)?;
        if ignored_entry_name.is_some_and(|ignored| entry.file_name() == ignored) {
            continue;
        }
        entry_count += 1;
        if entry_count >= maximum_entries {
            return Err(PluginPackageError::InvalidPackage(format!(
                "{label} has reached its {maximum_entries}-entry installation limit"
            )));
        }
    }
    Ok(())
}

fn read_bounded_file(
    path: &Path,
    maximum_bytes: u64,
    label: &str,
) -> Result<Vec<u8>, PluginPackageError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| PluginPackageError::Io {
        operation: "inspect installed metadata",
        path: path.display().to_string(),
        source,
    })?;
    if !metadata.file_type().is_file() || metadata.len() > maximum_bytes {
        return Err(PluginPackageError::InvalidPackage(format!(
            "{label} '{}' is not a bounded regular file",
            path.display()
        )));
    }
    let file = File::open(path).map_err(|source| PluginPackageError::Io {
        operation: "open installed metadata",
        path: path.display().to_string(),
        source,
    })?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(maximum_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| PluginPackageError::Io {
            operation: "read installed metadata",
            path: path.display().to_string(),
            source,
        })?;
    if bytes.len() as u64 > maximum_bytes {
        return Err(PluginPackageError::InvalidPackage(format!(
            "{label} '{}' exceeds {maximum_bytes} bytes",
            path.display()
        )));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), PluginPackageError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| PluginPackageError::Io {
            operation: "sync directory",
            path: path.display().to_string(),
            source,
        })
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), PluginPackageError> {
    Ok(())
}

fn read_bounded_zip_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
    maximum_bytes: u64,
) -> Result<Vec<u8>, PluginPackageError> {
    let entry = archive.by_name(path)?;
    let mut bytes =
        Vec::with_capacity(usize::try_from(entry.size().min(maximum_bytes)).unwrap_or_default());
    entry
        .take(maximum_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| PluginPackageError::Io {
            operation: "read package metadata",
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > maximum_bytes {
        return Err(PluginPackageError::InvalidPackage(format!(
            "archive entry '{path}' exceeds {maximum_bytes} bytes"
        )));
    }
    Ok(bytes)
}

fn parse_canonical_checksums(bytes: &[u8]) -> Result<BTreeMap<String, String>, PluginPackageError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| PluginPackageError::InvalidPackage("SHA256SUMS is not UTF-8".to_owned()))?;
    if source.is_empty() || !source.ends_with('\n') {
        return Err(PluginPackageError::InvalidPackage(
            "SHA256SUMS must be non-empty and newline terminated".to_owned(),
        ));
    }
    let mut checksums = BTreeMap::new();
    for line in source.lines() {
        let Some((checksum, path)) = line.split_once("  ") else {
            return Err(PluginPackageError::InvalidPackage(
                "SHA256SUMS contains a malformed line".to_owned(),
            ));
        };
        validate_sha256(checksum)?;
        crate::plugin_project::validate_archive_path("SHA256SUMS path", path)?;
        if matches!(
            path,
            PLUGIN_PACKAGE_CHECKSUMS_PATH | PLUGIN_PACKAGE_SIGNATURE_PATH
        ) {
            return Err(PluginPackageError::InvalidPackage(
                "SHA256SUMS must not include itself or signature.json".to_owned(),
            ));
        }
        if checksums
            .insert(path.to_owned(), checksum.to_owned())
            .is_some()
        {
            return Err(PluginPackageError::InvalidPackage(format!(
                "SHA256SUMS repeats path '{path}'"
            )));
        }
    }
    if canonical_checksums(&checksums) != bytes {
        return Err(PluginPackageError::InvalidPackage(
            "SHA256SUMS is not canonically sorted".to_owned(),
        ));
    }
    Ok(checksums)
}

fn sha256_zip_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
) -> Result<String, PluginPackageError> {
    let mut entry = archive.by_name(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = entry
            .read(&mut buffer)
            .map_err(|source| PluginPackageError::Io {
                operation: "hash package entry",
                path: path.to_owned(),
                source,
            })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn validate_reverse_dns_identifier(value: &str) -> Result<(), String> {
    PluginReference::new(value, None, PluginTransport::Native)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn key_id(public_key: &[u8; 32]) -> String {
    hex::encode(Sha256::digest(public_key))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_hex<const N: usize>(value: &str) -> Result<[u8; N], String> {
    if value.len() != N * 2 {
        return Err(format!(
            "expected {} lowercase hexadecimal characters",
            N * 2
        ));
    }
    let mut decoded = [0_u8; N];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        decoded[index] = (decode_hex_nibble(pair[0])? << 4) | decode_hex_nibble(pair[1])?;
    }
    Ok(decoded)
}

fn decode_hex_nibble(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err("hexadecimal values must use lowercase ASCII".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> PluginProjectManifest {
        project_with_id("dev.vesper.fixture")
    }

    fn project_with_id(plugin_id: &str) -> PluginProjectManifest {
        let source = r#"
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

[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "fixture plugin.dylib"
path = "artifacts/aarch64-apple-darwin/fixture plugin.dylib"
architecture = "arm64"
minimum_os = "13.0"
capabilities = [{ interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.fixture.post-download" }]

[[package_files]]
source = "LICENSE"
path = "licenses/LICENSE"
kind = "license"

[[package_files]]
source = "NOTICE"
path = "notices/NOTICE"
kind = "notice"
"#
        .replace("dev.vesper.fixture", plugin_id);
        PluginProjectManifest::from_toml(&source).expect("valid package project")
    }

    fn write_inputs(directory: &Path) {
        fs::write(directory.join("fixture plugin.dylib"), b"fixture artifact")
            .expect("write artifact");
        fs::write(directory.join("LICENSE"), b"Apache-2.0\n").expect("write license");
        fs::write(directory.join("NOTICE"), b"Fixture notice\n").expect("write notice");
    }

    #[test]
    fn prepared_file_entry_keeps_the_bytes_that_were_hashed() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        let source = directory.path().join("artifact.bin");
        fs::write(&source, b"original artifact").expect("write original artifact");
        let entry = prepare_file_entry(
            directory.path(),
            Path::new("artifact.bin"),
            "artifacts/artifact.bin",
            ARTIFACT_FILE_MODE,
        )
        .expect("prepare package entry");
        fs::write(&source, b"replacement artifact").expect("replace source artifact");

        let package = directory.path().join("snapshot.zip");
        write_package_atomically(&package, &[entry]).expect("write package from snapshot");
        let mut archive =
            ZipArchive::new(File::open(&package).expect("open package")).expect("read package");
        let mut bytes = Vec::new();
        archive
            .by_name("artifacts/artifact.bin")
            .expect("artifact entry")
            .read_to_end(&mut bytes)
            .expect("read artifact entry");
        assert_eq!(bytes, b"original artifact");
    }

    #[test]
    fn package_writer_rejects_aggregate_entry_size_over_verifier_limit() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        let package = directory.path().join("oversized.zip");
        let entries = [PreparedEntry {
            path: "artifacts/oversized.bin".to_owned(),
            data: PreparedEntryData::Bytes(Vec::new()),
            sha256: hex::encode(Sha256::digest([])),
            size: MAX_PLUGIN_PACKAGE_BYTES + 1,
            mode: ARTIFACT_FILE_MODE,
        }];

        assert!(matches!(
            write_package_atomically(&package, &entries),
            Err(PluginPackageError::InvalidPackage(message))
                if message.contains("aggregate package input exceeds")
        ));
        assert!(!package.exists());
    }

    fn verified_fixture_package(directory: &Path) -> VerifiedPluginPackage {
        verified_fixture_package_with_id(directory, "dev.vesper.fixture")
    }

    fn verified_fixture_package_with_id(
        directory: &Path,
        plugin_id: &str,
    ) -> VerifiedPluginPackage {
        write_inputs(directory);
        let project = project_with_id(plugin_id);
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let package_path = directory.join("fixture.vesper-plugin");
        build_signed_plugin_package(&project, directory, &key, &package_path)
            .expect("signed package");
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");
        verify_signed_plugin_package(&package_path, &trust).expect("verified package")
    }

    fn rewrite_with_tampered_artifact(source: &Path, output: &Path) {
        let input = File::open(source).expect("open source package");
        let mut archive = ZipArchive::new(input).expect("read source package");
        let output_file = File::create(output).expect("create tampered package");
        let mut writer = ZipWriter::new(output_file);
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("source entry");
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read source entry");
            if name.ends_with("fixture plugin.dylib") {
                bytes.push(0xff);
            }
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Stored)
                .last_modified_time(DateTime::default())
                .unix_permissions(entry.unix_mode().unwrap_or(0o644));
            writer
                .start_file(name, options)
                .expect("start copied entry");
            writer.write_all(&bytes).expect("write copied entry");
        }
        writer.finish().expect("finish tampered package");
    }

    fn rewrite_with_conflicting_archive_path(source: &Path, output: &Path) {
        let input = File::open(source).expect("open source package");
        let mut archive = ZipArchive::new(input).expect("read source package");
        let output_file = File::create(output).expect("create conflicting package");
        let mut writer = ZipWriter::new(output_file);
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("source entry");
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Stored)
                .last_modified_time(DateTime::default())
                .unix_permissions(entry.unix_mode().unwrap_or(PACKAGE_METADATA_FILE_MODE));
            writer
                .start_file(entry.name(), options)
                .expect("start copied entry");
            io::copy(&mut entry, &mut writer).expect("copy source entry");
        }
        writer
            .start_file(
                "licenses/LICENSE/detail",
                SimpleFileOptions::default()
                    .compression_method(CompressionMethod::Stored)
                    .last_modified_time(DateTime::default())
                    .unix_permissions(PACKAGE_METADATA_FILE_MODE),
            )
            .expect("start conflicting entry");
        writer.write_all(b"conflict").expect("write conflict");
        writer.finish().expect("finish conflicting package");
    }

    fn rewrite_central_directory_mode(
        source: &Path,
        output: &Path,
        target_path: &str,
        unix_mode: u32,
    ) {
        const CENTRAL_DIRECTORY_HEADER: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];

        let mut bytes = fs::read(source).expect("read package for mode rewrite");
        let mut cursor = 0_usize;
        let mut found = false;
        while cursor + 46 <= bytes.len() {
            if bytes[cursor..cursor + 4] != CENTRAL_DIRECTORY_HEADER {
                cursor += 1;
                continue;
            }
            let name_length = u16::from_le_bytes([bytes[cursor + 28], bytes[cursor + 29]]) as usize;
            let extra_length =
                u16::from_le_bytes([bytes[cursor + 30], bytes[cursor + 31]]) as usize;
            let comment_length =
                u16::from_le_bytes([bytes[cursor + 32], bytes[cursor + 33]]) as usize;
            let entry_length = 46 + name_length + extra_length + comment_length;
            assert!(cursor + entry_length <= bytes.len());
            if bytes[cursor + 46..cursor + 46 + name_length] == *target_path.as_bytes() {
                bytes[cursor + 5] = 3;
                bytes[cursor + 38..cursor + 42].copy_from_slice(&(unix_mode << 16).to_le_bytes());
                found = true;
                break;
            }
            cursor += entry_length;
        }
        assert!(found, "central directory entry must exist");
        fs::write(output, bytes).expect("write package with rewritten mode");
    }

    #[test]
    fn signed_package_is_deterministic_and_rejects_payload_tampering() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        write_inputs(directory.path());
        let project = project();
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let first = directory.path().join("first.vesper-plugin");
        let second = directory.path().join("second.vesper-plugin");
        build_signed_plugin_package(&project, directory.path(), &key, &first)
            .expect("first package");
        build_signed_plugin_package(&project, directory.path(), &key, &second)
            .expect("second package");
        assert_eq!(
            fs::read(&first).expect("first bytes"),
            fs::read(&second).expect("second bytes")
        );

        let mut trust = PluginTrustStore::empty();
        trust
            .insert(key.public_key())
            .expect("trusted publisher key");
        let verified =
            verify_signed_plugin_package(&first, &trust).expect("verified signed package");
        assert_eq!(verified.manifest().plugin.id, "dev.vesper.fixture");
        assert_eq!(verified.manifest().artifacts.len(), 1);

        let tampered = directory.path().join("tampered.vesper-plugin");
        rewrite_with_tampered_artifact(&first, &tampered);
        assert!(matches!(
            verify_signed_plugin_package(&tampered, &trust),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("checksum mismatch")
        ));
    }

    #[test]
    fn canonical_checksums_round_trip_paths_with_repeated_spaces() {
        let checksums =
            BTreeMap::from([("artifacts/plugin  debug.dylib".to_owned(), "a".repeat(64))]);
        let encoded = canonical_checksums(&checksums);

        assert_eq!(
            parse_canonical_checksums(&encoded).expect("canonical checksum list"),
            checksums
        );
    }

    #[test]
    fn package_manifest_validation_matches_published_artifact_limits() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        let verified = verified_fixture_package(directory.path());
        let manifest = verified.manifest().clone();

        let mut empty_target = manifest.clone();
        empty_target.artifacts[0].target.clear();
        assert!(matches!(
            canonical_manifest_bytes(&mut empty_target),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("artifacts.target")
        ));

        let mut empty_architecture = manifest.clone();
        empty_architecture.artifacts[0].architecture.clear();
        assert!(matches!(
            canonical_manifest_bytes(&mut empty_architecture),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("artifacts.architecture")
        ));

        let mut empty_minimum_os = manifest.clone();
        empty_minimum_os.artifacts[0].minimum_os = Some(String::new());
        assert!(matches!(
            canonical_manifest_bytes(&mut empty_minimum_os),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("artifacts.minimum_os")
        ));

        let mut excessive_dependencies = manifest;
        excessive_dependencies.artifacts[0].runtime_dependencies = (0
            ..=crate::plugin_project::MAX_RUNTIME_DEPENDENCIES)
            .map(|index| PluginRuntimeDependencySource {
                id: format!("dev.vesper.runtime.dep{index}"),
                version: "1".to_owned(),
                linkage: crate::PluginRuntimeLinkage::Dynamic,
                compatibility_key: "baseline".to_owned(),
            })
            .collect();
        assert!(matches!(
            canonical_manifest_bytes(&mut excessive_dependencies),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("runtime_dependencies")
        ));
    }

    #[test]
    fn verification_rejects_special_files_and_non_canonical_permissions() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        write_inputs(directory.path());
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let package_path = directory.path().join("fixture.vesper-plugin");
        build_signed_plugin_package(&project(), directory.path(), &key, &package_path)
            .expect("signed package");
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");

        let special_path = directory.path().join("special-file.vesper-plugin");
        rewrite_central_directory_mode(
            &package_path,
            &special_path,
            PLUGIN_PACKAGE_MANIFEST_PATH,
            0o010644,
        );
        assert!(matches!(
            verify_signed_plugin_package(&special_path, &trust),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("unsupported Unix file type")
        ));

        let permissive_path = directory.path().join("permissive.vesper-plugin");
        rewrite_central_directory_mode(
            &package_path,
            &permissive_path,
            "artifacts/aarch64-apple-darwin/fixture plugin.dylib",
            UNIX_REGULAR_FILE | 0o777,
        );
        assert!(matches!(
            verify_signed_plugin_package(&permissive_path, &trust),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("non-canonical permissions")
        ));
    }

    #[test]
    fn verification_rejects_file_and_directory_archive_path_conflicts() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        write_inputs(directory.path());
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let package_path = directory.path().join("fixture.vesper-plugin");
        let conflicting_path = directory.path().join("conflicting.vesper-plugin");
        build_signed_plugin_package(&project(), directory.path(), &key, &package_path)
            .expect("signed package");
        rewrite_with_conflicting_archive_path(&package_path, &conflicting_path);
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");

        assert!(matches!(
            verify_signed_plugin_package(&conflicting_path, &trust),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("conflicts with")
        ));
    }

    #[test]
    fn trust_store_supports_overlap_and_explicit_key_revocation() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        write_inputs(directory.path());
        let project = project();
        let old_key = PluginSigningKey::generate("dev.vesper.publisher").expect("old key");
        let new_key = PluginSigningKey::generate("dev.vesper.publisher").expect("new key");
        let old_package = directory.path().join("old.vesper-plugin");
        let new_package = directory.path().join("new.vesper-plugin");
        build_signed_plugin_package(&project, directory.path(), &old_key, &old_package)
            .expect("old-key package");
        build_signed_plugin_package(&project, directory.path(), &new_key, &new_package)
            .expect("new-key package");

        let mut trust = PluginTrustStore::empty();
        trust.insert(old_key.public_key()).expect("insert old key");
        trust.insert(new_key.public_key()).expect("insert new key");
        let encoded = trust.to_json().expect("trust store JSON");
        let mut trust = PluginTrustStore::from_json(&encoded).expect("decode trust store");
        verify_signed_plugin_package(&old_package, &trust).expect("old key during overlap");
        verify_signed_plugin_package(&new_package, &trust).expect("new key during overlap");

        trust
            .revoke(old_key.publisher(), old_key.key_id())
            .expect("revoke old key");
        assert!(matches!(
            verify_signed_plugin_package(&old_package, &trust),
            Err(PluginPackageError::InvalidSignature)
        ));
        verify_signed_plugin_package(&new_package, &trust).expect("new key remains active");

        let empty = PluginTrustStore::empty();
        assert!(matches!(
            verify_signed_plugin_package(&new_package, &empty),
            Err(PluginPackageError::InvalidSignature)
        ));
    }

    #[test]
    fn signing_key_round_trip_rejects_publisher_mismatch() {
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let encoded = key.to_json().expect("signing key JSON");
        let decoded = PluginSigningKey::from_json(&encoded).expect("decode signing key");
        assert_eq!(decoded.key_id(), key.key_id());

        let directory = tempfile::tempdir().expect("temporary package directory");
        write_inputs(directory.path());
        let wrong_key =
            PluginSigningKey::generate("dev.vesper.other-publisher").expect("wrong publisher key");
        assert!(matches!(
            build_signed_plugin_package(
                &project(),
                directory.path(),
                &wrong_key,
                &directory.path().join("rejected.vesper-plugin")
            ),
            Err(PluginPackageError::InvalidSigningKey(ref message))
                if message.contains("does not match")
        ));
    }

    #[test]
    fn verified_package_install_is_atomic_idempotent_and_removable() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        write_inputs(directory.path());
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let package_path = directory.path().join("fixture.vesper-plugin");
        build_signed_plugin_package(&project(), directory.path(), &key, &package_path)
            .expect("signed package");
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");
        let verified =
            verify_signed_plugin_package(&package_path, &trust).expect("verified package");
        let install_root = directory.path().join("installed");

        let first =
            install_verified_plugin_package(&verified, &install_root).expect("first installation");
        assert!(!first.already_installed);
        let installed_path = Path::new(&first.install_path);
        assert_eq!(
            fs::read(installed_path.join("artifacts/aarch64-apple-darwin/fixture plugin.dylib"))
                .expect("installed artifact"),
            b"fixture artifact"
        );
        assert!(installed_path.join(PLUGIN_PACKAGE_MANIFEST_PATH).is_file());
        assert!(installed_path.join(PLUGIN_PACKAGE_CHECKSUMS_PATH).is_file());
        assert!(installed_path.join(PLUGIN_PACKAGE_SIGNATURE_PATH).is_file());

        let second = install_verified_plugin_package(&verified, &install_root)
            .expect("idempotent installation");
        assert!(second.already_installed);
        assert_eq!(second.install_path, first.install_path);
        assert_eq!(second.package_sha256, first.package_sha256);

        assert_eq!(
            list_installed_plugins(&install_root).expect("installed plugin list"),
            vec![InstalledPluginRecord {
                plugin_id: "dev.vesper.fixture".to_owned(),
                version: "1.2.3".to_owned(),
                install_path: first.install_path.clone(),
                package_sha256: first.package_sha256.clone(),
            }]
        );
        assert!(
            uninstall_plugin(&install_root, "dev.vesper.fixture", "1.2.3")
                .expect("remove installed plugin")
        );
        assert!(
            !uninstall_plugin(&install_root, "dev.vesper.fixture", "1.2.3")
                .expect("missing installation is idempotent")
        );
        assert!(
            list_installed_plugins(&install_root)
                .expect("empty installed plugin list")
                .is_empty()
        );
    }

    #[test]
    fn installed_catalog_reverifies_and_snapshots_only_explicit_references() {
        let directory = tempfile::tempdir().expect("temporary installed catalog");
        write_inputs(directory.path());
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let package_path = directory.path().join("fixture.vesper-plugin");
        build_signed_plugin_package(&project(), directory.path(), &key, &package_path)
            .expect("signed package");
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");
        let verified =
            verify_signed_plugin_package(&package_path, &trust).expect("verified package");
        let install_root = directory.path().join("installed");
        let installation = install_verified_plugin_package(&verified, &install_root)
            .expect("verified installation");
        let reference = PluginReference::new(
            "dev.vesper.fixture",
            Some("dev.vesper.fixture.post-download".to_owned()),
            PluginTransport::Native,
        )
        .expect("plugin reference");
        let host = PluginHostTarget::new(Version::new(0, 4, 0), "aarch64-apple-darwin", "arm64")
            .expect("host target");

        let catalog = verify_installed_plugin_catalog(
            &install_root,
            &trust,
            &host,
            std::slice::from_ref(&reference),
            &[],
        )
        .expect("verified installed catalog");
        let [artifact] = catalog.artifacts() else {
            panic!("expected exactly one verified artifact");
        };
        assert_eq!(artifact.plugin_id(), "dev.vesper.fixture");
        assert_eq!(artifact.version(), "1.2.3");
        assert_ne!(artifact.snapshot_path(), artifact.installed_path());
        assert_eq!(
            artifact.read_snapshot(1024).expect("snapshot bytes"),
            b"fixture artifact"
        );

        fs::write(
            artifact.installed_path(),
            b"mutated after catalog verification",
        )
        .expect("mutate host-owned installation for regression test");
        assert_eq!(
            artifact
                .read_snapshot(1024)
                .expect("immutable snapshot bytes"),
            b"fixture artifact"
        );
        assert_eq!(installation.plugin_id, "dev.vesper.fixture");
    }

    #[test]
    fn installed_catalog_rejects_payload_tampering() {
        let directory = tempfile::tempdir().expect("temporary tampered catalog");
        write_inputs(directory.path());
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let package_path = directory.path().join("fixture.vesper-plugin");
        build_signed_plugin_package(&project(), directory.path(), &key, &package_path)
            .expect("signed package");
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");
        let verified =
            verify_signed_plugin_package(&package_path, &trust).expect("verified package");
        let install_root = directory.path().join("installed");
        let installation = install_verified_plugin_package(&verified, &install_root)
            .expect("verified installation");
        fs::write(
            installation
                .install_path
                .join("artifacts/aarch64-apple-darwin/fixture plugin.dylib"),
            b"tampered artifact",
        )
        .expect("tamper installed artifact");
        let reference = PluginReference::new("dev.vesper.fixture", None, PluginTransport::Native)
            .expect("plugin reference");
        let host = PluginHostTarget::new(Version::new(0, 4, 0), "aarch64-apple-darwin", "arm64")
            .expect("host target");

        assert!(matches!(
            verify_installed_plugin_catalog(
                &install_root,
                &trust,
                &host,
                &[reference],
                &[],
            ),
            Err(PluginPackageError::InvalidPackage(message))
                if message.contains("checksum mismatch")
        ));
    }

    #[test]
    fn installed_catalog_rejects_extra_files_and_non_regular_entries() {
        let directory = tempfile::tempdir().expect("temporary installed catalog layout");
        write_inputs(directory.path());
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let package_path = directory.path().join("fixture.vesper-plugin");
        build_signed_plugin_package(&project(), directory.path(), &key, &package_path)
            .expect("signed package");
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");
        let verified =
            verify_signed_plugin_package(&package_path, &trust).expect("verified package");
        let install_root = directory.path().join("installed");
        let installation = install_verified_plugin_package(&verified, &install_root)
            .expect("verified installation");
        let reference = PluginReference::new("dev.vesper.fixture", None, PluginTransport::Native)
            .expect("plugin reference");
        let host = PluginHostTarget::new(Version::new(0, 4, 0), "aarch64-apple-darwin", "arm64")
            .expect("host target");

        let extra_file = installation.install_path.join("unexpected.txt");
        fs::write(&extra_file, b"not checksummed").expect("write extra installed file");
        assert!(matches!(
            verify_installed_plugin_catalog(
                &install_root,
                &trust,
                &host,
                std::slice::from_ref(&reference),
                &[],
            ),
            Err(PluginPackageError::InvalidPackage(message))
                if message.contains("files do not exactly match")
        ));
        fs::remove_file(&extra_file).expect("remove extra installed file");

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            symlink(
                installation.install_path.join("licenses/LICENSE"),
                installation.install_path.join("unexpected-link"),
            )
            .expect("create installed symlink");
            assert!(matches!(
                verify_installed_plugin_catalog(
                    &install_root,
                    &trust,
                    &host,
                    &[reference],
                    &[],
                ),
                Err(PluginPackageError::InvalidPackage(message))
                    if message.contains("not a regular non-symlink file")
            ));
        }
    }

    #[test]
    fn installed_catalog_selects_mixed_transports_without_fallback() {
        let directory = tempfile::tempdir().expect("temporary mixed plugin catalog");
        fs::write(
            directory.path().join("fixture plugin.dylib"),
            b"native artifact",
        )
        .expect("write Native artifact");
        fs::write(directory.path().join("fixture.wasm"), b"WASM artifact")
            .expect("write WASM artifact");
        fs::write(directory.path().join("LICENSE"), b"Apache-2.0\n").expect("write license");
        fs::write(directory.path().join("NOTICE"), b"Fixture notice\n").expect("write notice");
        let project = PluginProjectManifest::from_toml(
            r#"
schema_version = 1

[plugin]
id = "dev.vesper.mixed"
name = "Mixed Fixture"
version = "1.2.3"
description = "Mixed Native and WASM fixture"
license = "Apache-2.0"
publisher = "dev.vesper.publisher"

[compatibility]
host_sdk = ">=0.4.0, <0.5.0"
abi_major = 1
abi_minor_min = 0
abi_minor_max = 0

[[capabilities]]
interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7"
instance_id = "dev.vesper.mixed.post-download"
interface_major = 1
interface_minor = 0
stability = "stable"

[[capabilities]]
interface_id = "c7a69475-79b2-5b5e-a477-08844a5da5d1"
instance_id = "dev.vesper.mixed.event-hook"
interface_major = 1
interface_minor = 0
stability = "stable"

[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "fixture plugin.dylib"
path = "artifacts/aarch64-apple-darwin/fixture plugin.dylib"
architecture = "arm64"
capabilities = [{ interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.mixed.post-download" }]

[[artifacts]]
transport = "wasm"
target = "wasm32-wasip2"
format = "wasm-component"
source = "fixture.wasm"
path = "artifacts/wasm32-wasip2/fixture.wasm"
architecture = "wasm32"
capabilities = [{ interface_id = "c7a69475-79b2-5b5e-a477-08844a5da5d1", instance_id = "dev.vesper.mixed.event-hook" }]

[[package_files]]
source = "LICENSE"
path = "licenses/LICENSE"
kind = "license"

[[package_files]]
source = "NOTICE"
path = "notices/NOTICE"
kind = "notice"
"#,
        )
        .expect("mixed plugin project");
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let package_path = directory.path().join("mixed.vesper-plugin");
        build_signed_plugin_package(&project, directory.path(), &key, &package_path)
            .expect("signed mixed package");
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");
        let verified =
            verify_signed_plugin_package(&package_path, &trust).expect("verified mixed package");
        let install_root = directory.path().join("installed");
        install_verified_plugin_package(&verified, &install_root).expect("installed mixed package");
        let native_reference = PluginReference::new(
            "dev.vesper.mixed",
            Some("dev.vesper.mixed.post-download".to_owned()),
            PluginTransport::Native,
        )
        .expect("Native reference");
        let wasm_reference = PluginReference::new(
            "dev.vesper.mixed",
            Some("dev.vesper.mixed.event-hook".to_owned()),
            PluginTransport::Wasm,
        )
        .expect("WASM reference");
        let host = PluginHostTarget::new(Version::new(0, 4, 0), "aarch64-apple-darwin", "arm64")
            .expect("host target");
        let catalog = verify_installed_plugin_catalog(
            &install_root,
            &trust,
            &host,
            &[native_reference.clone(), wasm_reference],
            &[],
        )
        .expect("verified mixed catalog");
        assert_eq!(catalog.artifacts().len(), 2);
        assert!(catalog.artifacts().iter().any(|artifact| {
            artifact.transport() == PluginArtifactTransport::Native
                && artifact.target() == "aarch64-apple-darwin"
                && artifact.capabilities().len() == 1
                && artifact.capabilities()[0].instance_id == "dev.vesper.mixed.post-download"
        }));
        assert!(catalog.artifacts().iter().any(|artifact| {
            artifact.transport() == PluginArtifactTransport::Wasm
                && artifact.target() == RUST_WASM_COMPONENT_TARGET
                && artifact.capabilities().len() == 1
                && artifact.capabilities()[0].instance_id == "dev.vesper.mixed.event-hook"
        }));

        let unsupported_host =
            PluginHostTarget::new(Version::new(0, 4, 0), "x86_64-unknown-linux-gnu", "x86_64")
                .expect("unsupported host target");
        assert!(matches!(
            verify_installed_plugin_catalog(
                &install_root,
                &trust,
                &unsupported_host,
                &[native_reference],
                &[],
            ),
            Err(PluginPackageError::InstalledArtifactNotFound { transport, .. })
                if transport == "native"
        ));
    }

    #[test]
    fn installed_catalog_requires_version_activation_and_rechecks_revocation() {
        let directory = tempfile::tempdir().expect("temporary activated catalog");
        write_inputs(directory.path());
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let key_id = key.key_id().to_owned();
        let package_path = directory.path().join("fixture.vesper-plugin");
        build_signed_plugin_package(&project(), directory.path(), &key, &package_path)
            .expect("signed package");
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");
        let verified =
            verify_signed_plugin_package(&package_path, &trust).expect("verified package");
        let install_root = directory.path().join("installed");
        install_verified_plugin_package(&verified, &install_root).expect("verified installation");
        fs::create_dir(install_root.join("dev.vesper.fixture").join("1.2.4"))
            .expect("create second version candidate");
        let reference = PluginReference::new("dev.vesper.fixture", None, PluginTransport::Native)
            .expect("plugin reference");
        let host = PluginHostTarget::new(Version::new(0, 4, 0), "aarch64-apple-darwin", "arm64")
            .expect("host target");
        assert!(matches!(
            verify_installed_plugin_catalog(
                &install_root,
                &trust,
                &host,
                std::slice::from_ref(&reference),
                &[],
            ),
            Err(PluginPackageError::AmbiguousInstalledVersions { .. })
        ));

        let activation = InstalledPluginActivation::new("dev.vesper.fixture", "1.2.3")
            .expect("version activation");
        let catalog = verify_installed_plugin_catalog(
            &install_root,
            &trust,
            &host,
            std::slice::from_ref(&reference),
            std::slice::from_ref(&activation),
        )
        .expect("activated catalog");
        assert_eq!(catalog.artifacts().len(), 1);
        drop(catalog);

        trust
            .revoke("dev.vesper.publisher", &key_id)
            .expect("revoke signing key");
        assert!(matches!(
            verify_installed_plugin_catalog(
                &install_root,
                &trust,
                &host,
                &[reference],
                &[activation],
            ),
            Err(PluginPackageError::InvalidSignature)
        ));
    }

    #[test]
    fn verified_package_supports_independent_concurrent_install_reads() {
        use std::sync::Arc;

        let directory = tempfile::tempdir().expect("temporary package directory");
        let verified = Arc::new(verified_fixture_package(directory.path()));
        let first_verified = Arc::clone(&verified);
        let first_root = directory.path().join("first-install-root");
        let first = std::thread::spawn(move || {
            install_verified_plugin_package(&first_verified, &first_root)
        });
        let second_verified = Arc::clone(&verified);
        let second_root = directory.path().join("second-install-root");
        let second = std::thread::spawn(move || {
            install_verified_plugin_package(&second_verified, &second_root)
        });

        assert!(
            !first
                .join()
                .expect("first install thread")
                .expect("first install")
                .already_installed
        );
        assert!(
            !second
                .join()
                .expect("second install thread")
                .expect("second install")
                .already_installed
        );
    }

    #[test]
    fn installed_catalog_enumeration_is_bounded() {
        let directory = tempfile::tempdir().expect("temporary install root");
        fs::write(directory.path().join(CATALOG_LOCK_PATH), b"").expect("catalog lock");
        for index in 0..MAX_INSTALLED_PLUGIN_IDENTITIES {
            fs::write(directory.path().join(format!("entry-{index}")), b"").expect("catalog entry");
        }

        assert!(
            list_installed_plugins(directory.path())
                .expect("catalog lock does not consume identity capacity")
                .is_empty()
        );
        fs::write(directory.path().join("overflow-entry"), b"").expect("overflow entry");
        assert!(matches!(
            list_installed_plugins(directory.path()),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("plugin install root exceeds")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn installation_report_paths_preserve_non_utf8_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let install_path = PathBuf::from(OsString::from_vec(b"install-\xff".to_vec()));
        let report = PluginInstallationReport {
            plugin_id: "dev.vesper.fixture".to_owned(),
            version: "1.2.3".to_owned(),
            install_path: install_path.clone(),
            package_sha256: "0".repeat(64),
            already_installed: false,
        };

        assert_eq!(report.install_path, install_path);
    }

    #[test]
    fn install_rejects_a_new_identity_when_the_catalog_is_full() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        let verified = verified_fixture_package(directory.path());
        let install_root = directory.path().join("installed-identities");
        fs::create_dir(&install_root).expect("install root");
        for index in 0..MAX_INSTALLED_PLUGIN_IDENTITIES {
            fs::write(install_root.join(format!("entry-{index}")), b"").expect("catalog entry");
        }

        assert!(matches!(
            install_verified_plugin_package(&verified, &install_root),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("1024-entry installation limit")
        ));
        assert!(!install_root.join("dev.vesper.fixture").exists());
    }

    #[test]
    fn catalog_lock_contention_is_nonblocking_and_prevents_mutation() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        let verified = verified_fixture_package(directory.path());
        let install_root = directory.path().join("locked-install-root");
        ensure_directory(&install_root, "test install root").expect("install root");
        let held_lock = PluginCatalogLock::acquire(&install_root).expect("held catalog lock");

        assert!(matches!(
            install_verified_plugin_package(&verified, &install_root),
            Err(PluginPackageError::CatalogBusy { .. })
        ));
        assert!(matches!(
            list_installed_plugins(&install_root),
            Err(PluginPackageError::CatalogBusy { .. })
        ));
        assert!(!install_root.join("dev.vesper.fixture").exists());

        drop(held_lock);
        install_verified_plugin_package(&verified, &install_root)
            .expect("installation after unlock");
        let held_lock = PluginCatalogLock::acquire(&install_root).expect("held catalog lock");
        assert!(matches!(
            uninstall_plugin(&install_root, "dev.vesper.fixture", "1.2.3"),
            Err(PluginPackageError::CatalogBusy { .. })
        ));
        assert!(install_root.join("dev.vesper.fixture/1.2.3").is_dir());
        drop(held_lock);
    }

    #[test]
    fn concurrent_installs_cannot_exceed_identity_capacity() {
        use std::sync::{Arc, Barrier};

        let directory = tempfile::tempdir().expect("temporary package directory");
        let first_source = directory.path().join("first-source");
        let second_source = directory.path().join("second-source");
        fs::create_dir(&first_source).expect("first package source");
        fs::create_dir(&second_source).expect("second package source");
        let first_verified = Arc::new(verified_fixture_package_with_id(
            &first_source,
            "dev.vesper.concurrent-first",
        ));
        let second_verified = Arc::new(verified_fixture_package_with_id(
            &second_source,
            "dev.vesper.concurrent-second",
        ));
        let install_root = directory.path().join("concurrent-install-root");
        fs::create_dir(&install_root).expect("install root");
        for index in 0..MAX_INSTALLED_PLUGIN_IDENTITIES - 1 {
            fs::write(install_root.join(format!("entry-{index}")), b"").expect("catalog entry");
        }

        let barrier = Arc::new(Barrier::new(3));
        let first_barrier = Arc::clone(&barrier);
        let first_root = install_root.clone();
        let first = std::thread::spawn(move || {
            first_barrier.wait();
            install_verified_plugin_package(&first_verified, &first_root)
        });
        let second_barrier = Arc::clone(&barrier);
        let second_root = install_root.clone();
        let second = std::thread::spawn(move || {
            second_barrier.wait();
            install_verified_plugin_package(&second_verified, &second_root)
        });
        barrier.wait();

        let results = [
            first.join().expect("first install thread"),
            second.join().expect("second install thread"),
        ];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert!(
            results
                .iter()
                .filter_map(|result| result.as_ref().err())
                .all(|error| match error {
                    PluginPackageError::CatalogBusy { .. } => true,
                    PluginPackageError::InvalidPackage(message) => {
                        message.contains("1024-entry installation limit")
                    }
                    _ => false,
                })
        );

        let catalog_entries = read_directory(&install_root)
            .expect("installed catalog")
            .map(|entry| read_directory_entry(entry, &install_root).expect("catalog entry"))
            .filter(|entry| entry.file_name() != OsStr::new(CATALOG_LOCK_PATH))
            .count();
        assert_eq!(catalog_entries, MAX_INSTALLED_PLUGIN_IDENTITIES);
        let installed_candidates = [
            install_root.join("dev.vesper.concurrent-first"),
            install_root.join("dev.vesper.concurrent-second"),
        ];
        assert_eq!(
            installed_candidates
                .iter()
                .filter(|candidate| candidate.is_dir())
                .count(),
            1
        );
        for candidate in installed_candidates
            .iter()
            .filter(|candidate| candidate.is_dir())
        {
            assert!(candidate.join("1.2.3").is_dir());
            assert!(
                read_directory(candidate)
                    .expect("installed identity")
                    .map(|entry| entry.expect("identity entry").file_name())
                    .all(|name| !name.to_string_lossy().starts_with(".vesper-staging-"))
            );
        }
    }

    #[test]
    fn install_rejects_a_new_version_when_the_identity_catalog_is_full() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        let verified = verified_fixture_package(directory.path());
        let install_root = directory.path().join("installed-versions");
        let plugin_root = install_root.join("dev.vesper.fixture");
        fs::create_dir_all(&plugin_root).expect("plugin identity directory");
        for index in 0..MAX_INSTALLED_VERSIONS_PER_PLUGIN {
            fs::write(plugin_root.join(format!("entry-{index}")), b"")
                .expect("version catalog entry");
        }

        assert!(matches!(
            install_verified_plugin_package(&verified, &install_root),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("256-entry installation limit")
        ));
        assert!(!plugin_root.join("1.2.3").exists());
    }

    #[test]
    fn install_rejects_a_conflicting_package_for_the_same_version() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        write_inputs(directory.path());
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let first_path = directory.path().join("first.vesper-plugin");
        let second_path = directory.path().join("second.vesper-plugin");
        build_signed_plugin_package(&project(), directory.path(), &key, &first_path)
            .expect("first signed package");
        fs::write(
            directory.path().join("fixture plugin.dylib"),
            b"different fixture artifact",
        )
        .expect("replace artifact input");
        build_signed_plugin_package(&project(), directory.path(), &key, &second_path)
            .expect("second signed package");
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");
        let first = verify_signed_plugin_package(&first_path, &trust).expect("first verified");
        let second = verify_signed_plugin_package(&second_path, &trust).expect("second verified");
        let install_root = directory.path().join("installed");
        install_verified_plugin_package(&first, &install_root).expect("first installation");

        assert!(matches!(
            install_verified_plugin_package(&second, &install_root),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("already installed from a different package")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn install_remains_bound_to_the_file_that_was_verified() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        write_inputs(directory.path());
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let package_path = directory.path().join("fixture.vesper-plugin");
        let replacement_path = directory.path().join("replacement.vesper-plugin");
        build_signed_plugin_package(&project(), directory.path(), &key, &package_path)
            .expect("original signed package");
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");
        let verified =
            verify_signed_plugin_package(&package_path, &trust).expect("verified package");

        fs::write(
            directory.path().join("fixture plugin.dylib"),
            b"different fixture artifact",
        )
        .expect("replace artifact input");
        build_signed_plugin_package(&project(), directory.path(), &key, &replacement_path)
            .expect("replacement signed package");
        fs::rename(&replacement_path, &package_path).expect("replace verified package path");

        let report = install_verified_plugin_package(
            &verified,
            &directory.path().join("installed-from-pinned-handle"),
        )
        .expect("install pinned verified package");
        assert_eq!(
            fs::read(
                Path::new(&report.install_path)
                    .join("artifacts/aarch64-apple-darwin/fixture plugin.dylib")
            )
            .expect("installed artifact"),
            b"fixture artifact"
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_rejects_in_place_package_mutation_after_verification() {
        let directory = tempfile::tempdir().expect("temporary package directory");
        write_inputs(directory.path());
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let package_path = directory.path().join("fixture.vesper-plugin");
        let replacement_path = directory.path().join("replacement.vesper-plugin");
        build_signed_plugin_package(&project(), directory.path(), &key, &package_path)
            .expect("original signed package");
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");
        let verified =
            verify_signed_plugin_package(&package_path, &trust).expect("verified package");

        fs::write(
            directory.path().join("fixture plugin.dylib"),
            b"fixture artifacX",
        )
        .expect("mutate artifact input");
        build_signed_plugin_package(&project(), directory.path(), &key, &replacement_path)
            .expect("replacement signed package");
        fs::write(
            &package_path,
            fs::read(&replacement_path).expect("replacement package bytes"),
        )
        .expect("mutate verified package in place");

        let install_root = directory.path().join("rejected-install");
        assert!(matches!(
            install_verified_plugin_package(&verified, &install_root),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("changed after verification")
        ));
        assert!(!install_root.join("dev.vesper.fixture").exists());
        assert_eq!(
            read_directory(&install_root)
                .expect("failed install root")
                .map(|entry| entry.expect("install root entry").file_name())
                .collect::<Vec<_>>(),
            vec![OsStr::new(CATALOG_LOCK_PATH).to_os_string()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_and_uninstall_reject_symlinked_install_boundaries() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temporary package directory");
        write_inputs(directory.path());
        let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
        let package_path = directory.path().join("fixture.vesper-plugin");
        build_signed_plugin_package(&project(), directory.path(), &key, &package_path)
            .expect("signed package");
        let mut trust = PluginTrustStore::empty();
        trust.insert(key.public_key()).expect("trusted key");
        let verified =
            verify_signed_plugin_package(&package_path, &trust).expect("verified package");
        let real_root = directory.path().join("real-install-root");
        fs::create_dir(&real_root).expect("real install root");
        let linked_root = directory.path().join("linked-install-root");
        symlink(&real_root, &linked_root).expect("install root symlink");

        assert!(matches!(
            install_verified_plugin_package(&verified, &linked_root),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("not a regular directory")
        ));
        assert!(matches!(
            uninstall_plugin(&linked_root, "dev.vesper.fixture", "1.2.3"),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("not a regular directory")
        ));

        let install_root = directory.path().join("installed");
        let plugin_root = install_root.join("dev.vesper.fixture");
        fs::create_dir_all(&plugin_root).expect("plugin identity directory");
        symlink(&real_root, plugin_root.join("1.2.3")).expect("version target symlink");
        assert!(matches!(
            install_verified_plugin_package(&verified, &install_root),
            Err(PluginPackageError::InvalidPackage(ref message))
                if message.contains("not a regular directory")
        ));
    }
}
