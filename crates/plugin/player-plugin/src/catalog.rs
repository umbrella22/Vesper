//! Pure plugin artifact and catalog metadata.
//!
//! The catalog layer deliberately contains no executable plugin state.  It is
//! safe to serialize, sort, cache, and rebuild these values before a runtime
//! decides to load an artifact.

use std::collections::HashSet;

use player_plugin_abi::{VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES, VESPER_MAX_PLUGIN_ID_BYTES};
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{PluginReference, PluginReferenceError, PluginTransport};

/// Version of the pure artifact/catalog wire model.
pub const PLUGIN_CATALOG_SCHEMA_VERSION: u32 = 1;
/// Migration guide identity carried by new catalog records.
pub const PLUGIN_CATALOG_MIGRATION_VERSION: &str = "vesper-plugin-runtime-rewrite-v1";
pub const MAX_PLUGIN_ARTIFACT_CAPABILITIES: usize = 64;
pub const MAX_PLUGIN_REQUIREMENTS: usize = 64;
pub const MAX_PLUGIN_PROVISIONS: usize = 64;
pub const MAX_PLUGIN_CATALOG_RECORDS: usize = 1024;
pub const MAX_PLUGIN_CATALOG_DIAGNOSTICS: usize = 64;
pub const MAX_PLUGIN_TARGET_BYTES: usize = 128;
pub const MAX_PLUGIN_ARCHITECTURE_BYTES: usize = 64;
pub const MAX_PLUGIN_ARTIFACT_PATH_BYTES: usize = 4096;
pub const MAX_PLUGIN_CATALOG_SOURCE_BYTES: usize = 256;
pub const MAX_PLUGIN_RUNTIME_DEPENDENCIES: usize = 32;

/// Storage transport used by an artifact package.  This is kept separate from
/// capability selection's [`PluginTransport`] so package provenance cannot be
/// mistaken for workload support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
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

/// Artifact packaging format independent of the loader implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
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

/// A capability exposed by one artifact.  The descriptor owns the interface
/// identity; executable availability is established later by the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginArtifactCapability {
    pub interface_id: String,
    pub instance_id: String,
}

/// A typed provider declaration.  Resolution is intentionally outside the
/// catalog; this value only records the author-owned requirement.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRequirement {
    pub service: String,
    pub requirement: String,
}

/// A typed service/capability provided by an artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginProvision {
    pub service: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginRuntimeLinkage {
    Dynamic,
    Static,
    System,
}

/// A native/runtime dependency declaration preserved as catalog metadata.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRuntimeDependency {
    pub id: String,
    pub version: String,
    pub linkage: PluginRuntimeLinkage,
    pub compatibility_key: String,
}

/// Bounded resource declarations used by a future resolver policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginResourcePolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_queue_depth: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_call_micros: Option<u64>,
}

/// Why a catalog record came from a particular source.  This is provenance,
/// not a transport or capability claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginCatalogSource {
    Package,
    Installed,
    Embedded,
    Development,
}

/// Artifact metadata that can be inspected without opening the artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginArtifactDescriptor {
    pub schema_version: u32,
    pub plugin_id: String,
    pub version: String,
    pub publisher: String,
    pub transport: PluginArtifactTransport,
    pub target: String,
    pub format: PluginArtifactFormat,
    pub architecture: String,
    pub abi_major: u16,
    pub abi_minor_min: u16,
    pub abi_minor_max: u16,
    pub capabilities: Vec<PluginArtifactCapability>,
    #[serde(default)]
    pub requires: Vec<PluginRequirement>,
    #[serde(default)]
    pub provides: Vec<PluginProvision>,
    #[serde(default)]
    pub runtime_dependencies: Vec<PluginRuntimeDependency>,
    #[serde(default)]
    pub resource_policy: PluginResourcePolicy,
    #[serde(default = "default_migration_version")]
    pub migration_version: String,
}

/// A catalog record adds immutable artifact provenance and content identity to
/// [`PluginArtifactDescriptor`].  It intentionally has no live owner field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCatalogRecord {
    pub schema_version: u32,
    pub descriptor: PluginArtifactDescriptor,
    pub artifact_path: String,
    pub artifact_sha256: String,
    pub source: PluginCatalogSource,
    #[serde(default)]
    pub diagnostics: Vec<PluginCatalogDiagnostic>,
}

/// Bounded, redacted provenance diagnostic kept with a catalog record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCatalogDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginArtifactDescriptorWire {
    schema_version: u32,
    plugin_id: String,
    version: String,
    publisher: String,
    transport: PluginArtifactTransport,
    target: String,
    format: PluginArtifactFormat,
    architecture: String,
    abi_major: u16,
    abi_minor_min: u16,
    abi_minor_max: u16,
    capabilities: Vec<PluginArtifactCapability>,
    #[serde(default)]
    requires: Vec<PluginRequirement>,
    #[serde(default)]
    provides: Vec<PluginProvision>,
    #[serde(default)]
    runtime_dependencies: Vec<PluginRuntimeDependency>,
    #[serde(default)]
    resource_policy: PluginResourcePolicy,
    #[serde(default = "default_migration_version")]
    migration_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginCatalogRecordWire {
    schema_version: u32,
    descriptor: PluginArtifactDescriptor,
    artifact_path: String,
    artifact_sha256: String,
    source: PluginCatalogSource,
    #[serde(default)]
    diagnostics: Vec<PluginCatalogDiagnostic>,
}

impl<'de> Deserialize<'de> for PluginArtifactDescriptor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = PluginArtifactDescriptorWire::deserialize(deserializer)?;
        let descriptor = Self {
            schema_version: wire.schema_version,
            plugin_id: wire.plugin_id,
            version: wire.version,
            publisher: wire.publisher,
            transport: wire.transport,
            target: wire.target,
            format: wire.format,
            architecture: wire.architecture,
            abi_major: wire.abi_major,
            abi_minor_min: wire.abi_minor_min,
            abi_minor_max: wire.abi_minor_max,
            capabilities: wire.capabilities,
            requires: wire.requires,
            provides: wire.provides,
            runtime_dependencies: wire.runtime_dependencies,
            resource_policy: wire.resource_policy,
            migration_version: wire.migration_version,
        };
        descriptor.validate().map_err(serde::de::Error::custom)?;
        Ok(descriptor)
    }
}

impl<'de> Deserialize<'de> for PluginCatalogRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = PluginCatalogRecordWire::deserialize(deserializer)?;
        let record = Self {
            schema_version: wire.schema_version,
            descriptor: wire.descriptor,
            artifact_path: wire.artifact_path,
            artifact_sha256: wire.artifact_sha256,
            source: wire.source,
            diagnostics: wire.diagnostics,
        };
        record.validate().map_err(serde::de::Error::custom)?;
        Ok(record)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalPluginArtifactDescriptor {
    descriptor: PluginArtifactDescriptor,
    json: Vec<u8>,
    sha256: String,
}

impl CanonicalPluginArtifactDescriptor {
    pub fn descriptor(&self) -> &PluginArtifactDescriptor {
        &self.descriptor
    }

    pub fn json(&self) -> &[u8] {
        &self.json
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn fingerprint(&self) -> &str {
        self.sha256()
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PluginCatalogError {
    #[error("invalid plugin catalog field `{field}`: {message}")]
    InvalidField { field: String, message: String },
    #[error("duplicate artifact capability `{interface_id}:{instance_id}`")]
    DuplicateCapability {
        interface_id: String,
        instance_id: String,
    },
    #[error("duplicate catalog identity `{identity}` from `{first_path}` and `{duplicate_path}`")]
    DuplicateIdentity {
        identity: String,
        first_path: String,
        duplicate_path: String,
    },
    #[error("artifact digest for `{path}` is not valid SHA-256")]
    InvalidDigest { path: String },
    #[error("catalog contains more than {limit} records")]
    TooManyRecords { limit: usize },
    #[error("catalog contains more than {limit} diagnostics")]
    TooManyDiagnostics { limit: usize },
    #[error("failed to serialize canonical plugin catalog metadata: {0}")]
    Json(String),
    #[error(transparent)]
    Reference(#[from] PluginReferenceError),
}

impl PluginArtifactDescriptor {
    /// Constructs and validates a descriptor from the stable artifact fields.
    /// Optional dependency/resource declarations can be added before calling
    /// [`Self::canonicalize`].
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        plugin_id: impl Into<String>,
        version: impl Into<String>,
        publisher: impl Into<String>,
        transport: PluginArtifactTransport,
        target: impl Into<String>,
        format: PluginArtifactFormat,
        architecture: impl Into<String>,
        abi_major: u16,
        abi_minor_min: u16,
        abi_minor_max: u16,
        capabilities: Vec<PluginArtifactCapability>,
    ) -> Result<Self, PluginCatalogError> {
        let descriptor = Self {
            schema_version: PLUGIN_CATALOG_SCHEMA_VERSION,
            plugin_id: plugin_id.into(),
            version: version.into(),
            publisher: publisher.into(),
            transport,
            target: target.into(),
            format,
            architecture: architecture.into(),
            abi_major,
            abi_minor_min,
            abi_minor_max,
            capabilities,
            requires: Vec::new(),
            provides: Vec::new(),
            runtime_dependencies: Vec::new(),
            resource_policy: PluginResourcePolicy::default(),
            migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    /// Decodes and validates one descriptor from canonical JSON input.
    pub fn from_json(bytes: &[u8]) -> Result<Self, PluginCatalogError> {
        let descriptor: Self = serde_json::from_slice(bytes)
            .map_err(|error| PluginCatalogError::Json(error.to_string()))?;
        descriptor.validate()?;
        Ok(descriptor)
    }

    /// Encodes the validated descriptor in deterministic JSON form.
    pub fn to_json(&self) -> Result<Vec<u8>, PluginCatalogError> {
        Ok(self.canonicalize()?.json().to_vec())
    }

    /// Validates the descriptor without loading or probing an artifact.
    pub fn validate(&self) -> Result<(), PluginCatalogError> {
        if self.schema_version != PLUGIN_CATALOG_SCHEMA_VERSION {
            return invalid(
                "schema_version",
                format!(
                    "expected {PLUGIN_CATALOG_SCHEMA_VERSION}, got {}",
                    self.schema_version
                ),
            );
        }
        validate_reverse_dns("plugin_id", &self.plugin_id, VESPER_MAX_PLUGIN_ID_BYTES)?;
        validate_reverse_dns("publisher", &self.publisher, VESPER_MAX_PLUGIN_ID_BYTES)?;
        Version::parse(&self.version).map_err(|error| field_error("version", error.to_string()))?;
        validate_text("target", &self.target, MAX_PLUGIN_TARGET_BYTES)?;
        validate_text(
            "architecture",
            &self.architecture,
            MAX_PLUGIN_ARCHITECTURE_BYTES,
        )?;
        if self.abi_major == 0 {
            return invalid("abi_major", "must be greater than zero");
        }
        if self.abi_minor_min > self.abi_minor_max {
            return invalid("abi_minor_min", "must not exceed abi_minor_max");
        }
        if !format_matches_transport(self.transport, self.format) {
            return invalid(
                "format",
                format!(
                    "format '{}' is incompatible with transport '{}'",
                    self.format.as_str(),
                    transport_name(self.transport)
                ),
            );
        }
        if self.capabilities.is_empty()
            || self.capabilities.len() > MAX_PLUGIN_ARTIFACT_CAPABILITIES
        {
            return invalid(
                "capabilities",
                format!("must contain 1 to {MAX_PLUGIN_ARTIFACT_CAPABILITIES} entries"),
            );
        }
        let mut identities = HashSet::with_capacity(self.capabilities.len());
        for capability in &self.capabilities {
            let interface_id = Uuid::parse_str(&capability.interface_id)
                .map_err(|error| field_error("capabilities.interface_id", error.to_string()))?;
            if interface_id.hyphenated().to_string() != capability.interface_id {
                return invalid(
                    "capabilities.interface_id",
                    "must use canonical lowercase hyphenated UUID form",
                );
            }
            validate_reverse_dns(
                "capabilities.instance_id",
                &capability.instance_id,
                VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES,
            )?;
            if !identities.insert((&capability.interface_id, &capability.instance_id)) {
                return Err(PluginCatalogError::DuplicateCapability {
                    interface_id: capability.interface_id.clone(),
                    instance_id: capability.instance_id.clone(),
                });
            }
        }
        validate_dependency_declarations(&self.requires, "requires")?;
        validate_provisions(&self.provides)?;
        validate_runtime_dependencies(&self.runtime_dependencies)?;
        validate_text(
            "migration_version",
            &self.migration_version,
            MAX_PLUGIN_CATALOG_SOURCE_BYTES,
        )?;
        validate_resource_policy(&self.resource_policy)?;
        Ok(())
    }

    /// Produces stable JSON and a SHA-256 identity for catalog metadata.
    pub fn canonicalize(&self) -> Result<CanonicalPluginArtifactDescriptor, PluginCatalogError> {
        self.validate()?;
        let mut descriptor = self.clone();
        descriptor.capabilities.sort();
        descriptor.requires.sort();
        descriptor.provides.sort();
        descriptor.runtime_dependencies.sort();
        let json = serde_json::to_vec(&descriptor)
            .map_err(|error| PluginCatalogError::Json(error.to_string()))?;
        let sha256 = hex::encode(Sha256::digest(&json));
        Ok(CanonicalPluginArtifactDescriptor {
            descriptor,
            json,
            sha256,
        })
    }

    pub fn fingerprint(&self) -> Result<String, PluginCatalogError> {
        Ok(self.canonicalize()?.sha256().to_owned())
    }

    pub fn plugin_reference(
        &self,
        capability_instance_id: Option<String>,
    ) -> Result<PluginReference, PluginCatalogError> {
        self.validate()?;
        Ok(PluginReference::new(
            self.plugin_id.clone(),
            capability_instance_id,
            match self.transport {
                PluginArtifactTransport::Native => PluginTransport::Native,
                PluginArtifactTransport::Wasm => PluginTransport::Wasm,
            },
        )?)
    }
}

impl PluginCatalogRecord {
    pub fn from_descriptor(
        descriptor: PluginArtifactDescriptor,
        artifact_path: impl Into<String>,
        artifact_sha256: impl Into<String>,
        source: PluginCatalogSource,
    ) -> Result<Self, PluginCatalogError> {
        Self::new(descriptor, artifact_path, artifact_sha256, source)
    }

    pub fn new(
        descriptor: PluginArtifactDescriptor,
        artifact_path: impl Into<String>,
        artifact_sha256: impl Into<String>,
        source: PluginCatalogSource,
    ) -> Result<Self, PluginCatalogError> {
        let record = Self {
            schema_version: PLUGIN_CATALOG_SCHEMA_VERSION,
            descriptor,
            artifact_path: artifact_path.into(),
            artifact_sha256: artifact_sha256.into(),
            source,
            diagnostics: Vec::new(),
        };
        record.validate()?;
        Ok(record)
    }

    /// Decodes and validates one catalog record from JSON input.
    pub fn from_json(bytes: &[u8]) -> Result<Self, PluginCatalogError> {
        let record: Self = serde_json::from_slice(bytes)
            .map_err(|error| PluginCatalogError::Json(error.to_string()))?;
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), PluginCatalogError> {
        if self.schema_version != PLUGIN_CATALOG_SCHEMA_VERSION {
            return invalid(
                "schema_version",
                format!(
                    "expected {PLUGIN_CATALOG_SCHEMA_VERSION}, got {}",
                    self.schema_version
                ),
            );
        }
        self.descriptor.validate()?;
        validate_text(
            "artifact_path",
            &self.artifact_path,
            MAX_PLUGIN_ARTIFACT_PATH_BYTES,
        )?;
        if self.artifact_path.contains('\0') {
            return invalid("artifact_path", "must not contain NUL bytes");
        }
        if self.source == PluginCatalogSource::Package && !is_safe_package_path(&self.artifact_path)
        {
            return invalid(
                "artifact_path",
                "package catalog paths must be relative and must not contain traversal segments",
            );
        }
        if !is_sha256(&self.artifact_sha256) {
            return Err(PluginCatalogError::InvalidDigest {
                path: self.artifact_path.clone(),
            });
        }
        if self.diagnostics.len() > MAX_PLUGIN_CATALOG_DIAGNOSTICS {
            return Err(PluginCatalogError::TooManyDiagnostics {
                limit: MAX_PLUGIN_CATALOG_DIAGNOSTICS,
            });
        }
        for diagnostic in &self.diagnostics {
            validate_text("diagnostics.code", &diagnostic.code, 128)?;
            validate_text("diagnostics.message", &diagnostic.message, 512)?;
        }
        Ok(())
    }

    pub fn descriptor(&self) -> &PluginArtifactDescriptor {
        &self.descriptor
    }

    pub fn artifact_path(&self) -> &str {
        &self.artifact_path
    }

    pub fn artifact_sha256(&self) -> &str {
        &self.artifact_sha256
    }

    pub fn identity_key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}",
            transport_name(self.descriptor.transport),
            self.descriptor.plugin_id,
            self.descriptor.version,
            self.descriptor.target,
            self.descriptor.architecture,
            self.descriptor.format.as_str()
        )
    }

    /// Returns an unambiguous key for sorting and indexing catalog records.
    ///
    /// The human-readable [`Self::identity_key`] is retained for diagnostics;
    /// this key length-prefixes every component so opaque target labels may
    /// contain the diagnostic separator without colliding.
    pub fn canonical_identity_key(&self) -> String {
        [
            transport_name(self.descriptor.transport),
            &self.descriptor.plugin_id,
            &self.descriptor.version,
            &self.descriptor.target,
            &self.descriptor.architecture,
            self.descriptor.format.as_str(),
        ]
        .into_iter()
        .map(|component| format!("{}:{component}", component.len()))
        .collect::<Vec<_>>()
        .join("|")
    }

    pub fn canonicalize(&self) -> Result<Vec<u8>, PluginCatalogError> {
        self.validate()?;
        let mut record = self.clone();
        record.descriptor = record.descriptor.canonicalize()?.descriptor().clone();
        record
            .diagnostics
            .sort_by(|left, right| (&left.code, &left.message).cmp(&(&right.code, &right.message)));
        serde_json::to_vec(&record).map_err(|error| PluginCatalogError::Json(error.to_string()))
    }

    pub fn fingerprint(&self) -> Result<String, PluginCatalogError> {
        Ok(hex::encode(Sha256::digest(self.canonicalize()?)))
    }
}

/// Immutable catalog projection.  Construction validates and sorts records;
/// querying it cannot load or instantiate a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCatalog {
    records: Vec<PluginCatalogRecord>,
    fingerprint: String,
}

impl Serialize for PluginCatalog {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.records.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PluginCatalog {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let records = Vec::<PluginCatalogRecord>::deserialize(deserializer)?;
        Self::from_records(records).map_err(serde::de::Error::custom)
    }
}

impl PluginCatalog {
    pub fn new(
        records: impl IntoIterator<Item = PluginCatalogRecord>,
    ) -> Result<Self, PluginCatalogError> {
        Self::from_records(records)
    }

    pub fn from_records(
        records: impl IntoIterator<Item = PluginCatalogRecord>,
    ) -> Result<Self, PluginCatalogError> {
        let mut normalized = Vec::new();
        for record in records {
            if normalized.len() >= MAX_PLUGIN_CATALOG_RECORDS {
                return Err(PluginCatalogError::TooManyRecords {
                    limit: MAX_PLUGIN_CATALOG_RECORDS,
                });
            }
            record.validate()?;
            let mut record = record;
            record.descriptor = record.descriptor.canonicalize()?.descriptor().clone();
            record.diagnostics.sort_by(|left, right| {
                (&left.code, &left.message).cmp(&(&right.code, &right.message))
            });
            normalized.push(record);
        }
        let mut records = normalized;
        records.sort_by(|left, right| {
            left.identity_key()
                .cmp(&right.identity_key())
                .then_with(|| {
                    left.canonical_identity_key()
                        .cmp(&right.canonical_identity_key())
                })
        });
        for pair in records.windows(2) {
            if pair[0].canonical_identity_key() == pair[1].canonical_identity_key() {
                return Err(PluginCatalogError::DuplicateIdentity {
                    identity: pair[0].identity_key(),
                    first_path: pair[0].artifact_path.clone(),
                    duplicate_path: pair[1].artifact_path.clone(),
                });
            }
        }
        let bytes = serde_json::to_vec(&records)
            .map_err(|error| PluginCatalogError::Json(error.to_string()))?;
        let fingerprint = hex::encode(Sha256::digest(bytes));
        Ok(Self {
            records,
            fingerprint,
        })
    }

    pub fn empty() -> Self {
        Self {
            records: Vec::new(),
            fingerprint: hex::encode(Sha256::digest(b"[]")),
        }
    }

    /// Decodes a complete catalog snapshot and rebuilds its deterministic
    /// index/fingerprint.  No executable artifact is touched.
    pub fn from_json(bytes: &[u8]) -> Result<Self, PluginCatalogError> {
        let records: Vec<PluginCatalogRecord> = serde_json::from_slice(bytes)
            .map_err(|error| PluginCatalogError::Json(error.to_string()))?;
        Self::from_records(records)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, PluginCatalogError> {
        serde_json::to_vec(&self.records)
            .map_err(|error| PluginCatalogError::Json(error.to_string()))
    }

    pub fn records(&self) -> &[PluginCatalogRecord] {
        &self.records
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn find(&self, plugin_id: &str) -> impl Iterator<Item = &PluginCatalogRecord> {
        self.records
            .iter()
            .filter(move |record| record.descriptor.plugin_id == plugin_id)
    }
}

fn default_migration_version() -> String {
    PLUGIN_CATALOG_MIGRATION_VERSION.to_owned()
}

fn format_matches_transport(
    transport: PluginArtifactTransport,
    format: PluginArtifactFormat,
) -> bool {
    matches!(
        (transport, format),
        (
            PluginArtifactTransport::Wasm,
            PluginArtifactFormat::WasmComponent
        ) | (PluginArtifactTransport::Native, PluginArtifactFormat::Dylib)
            | (PluginArtifactTransport::Native, PluginArtifactFormat::Aar)
            | (
                PluginArtifactTransport::Native,
                PluginArtifactFormat::Xcframework
            )
    )
}

fn transport_name(transport: PluginArtifactTransport) -> &'static str {
    match transport {
        PluginArtifactTransport::Native => "native",
        PluginArtifactTransport::Wasm => "wasm",
    }
}

fn validate_dependency_declarations(
    dependencies: &[PluginRequirement],
    field: &str,
) -> Result<(), PluginCatalogError> {
    if dependencies.len() > MAX_PLUGIN_REQUIREMENTS {
        return invalid(
            field,
            format!("must contain at most {MAX_PLUGIN_REQUIREMENTS} entries"),
        );
    }
    let mut seen = HashSet::with_capacity(dependencies.len());
    for dependency in dependencies {
        validate_reverse_dns(
            &format!("{field}.service"),
            &dependency.service,
            VESPER_MAX_PLUGIN_ID_BYTES,
        )?;
        semver::VersionReq::parse(&dependency.requirement).map_err(|error| {
            field_error(
                &format!("{field}.requirement"),
                format!("invalid semver requirement: {error}"),
            )
        })?;
        if !seen.insert(&dependency.service) {
            return invalid(
                &format!("{field}.service"),
                "must not contain duplicate service identities",
            );
        }
    }
    Ok(())
}

/// Validates author-facing `requires` declarations using the catalog's
/// canonical identity and semver rules.
pub fn validate_plugin_requirements(
    requirements: &[PluginRequirement],
) -> Result<(), PluginCatalogError> {
    validate_dependency_declarations(requirements, "requires")
}

fn validate_provisions(provisions: &[PluginProvision]) -> Result<(), PluginCatalogError> {
    if provisions.len() > MAX_PLUGIN_PROVISIONS {
        return invalid(
            "provides",
            format!("must contain at most {MAX_PLUGIN_PROVISIONS} entries"),
        );
    }
    let mut seen = HashSet::with_capacity(provisions.len());
    for provision in provisions {
        validate_reverse_dns(
            "provides.service",
            &provision.service,
            VESPER_MAX_PLUGIN_ID_BYTES,
        )?;
        Version::parse(&provision.version).map_err(|error| {
            field_error(
                "provides.version",
                format!("invalid semver version: {error}"),
            )
        })?;
        if !seen.insert(&provision.service) {
            return invalid(
                "provides.service",
                "must not contain duplicate service identities",
            );
        }
    }
    Ok(())
}

/// Validates author-facing `provides` declarations using the catalog's
/// canonical identity and semver rules.
pub fn validate_plugin_provisions(
    provisions: &[PluginProvision],
) -> Result<(), PluginCatalogError> {
    validate_provisions(provisions)
}

fn validate_runtime_dependencies(
    dependencies: &[PluginRuntimeDependency],
) -> Result<(), PluginCatalogError> {
    if dependencies.len() > MAX_PLUGIN_RUNTIME_DEPENDENCIES {
        return invalid(
            "runtime_dependencies",
            format!("must contain at most {MAX_PLUGIN_RUNTIME_DEPENDENCIES} entries"),
        );
    }
    let mut seen = HashSet::with_capacity(dependencies.len());
    for dependency in dependencies {
        validate_reverse_dns(
            "runtime_dependencies.id",
            &dependency.id,
            VESPER_MAX_PLUGIN_ID_BYTES,
        )?;
        validate_text(
            "runtime_dependencies.version",
            &dependency.version,
            MAX_PLUGIN_CATALOG_SOURCE_BYTES,
        )?;
        validate_text(
            "runtime_dependencies.compatibility_key",
            &dependency.compatibility_key,
            MAX_PLUGIN_CATALOG_SOURCE_BYTES,
        )?;
        if !seen.insert(&dependency.id) {
            return invalid(
                "runtime_dependencies.id",
                "must not contain duplicate dependency identities",
            );
        }
    }
    Ok(())
}

fn validate_resource_policy(policy: &PluginResourcePolicy) -> Result<(), PluginCatalogError> {
    if policy.max_memory_bytes == Some(0)
        || policy.max_queue_depth == Some(0)
        || policy.max_call_micros == Some(0)
    {
        return invalid("resource_policy", "limits must be greater than zero");
    }
    Ok(())
}

fn validate_reverse_dns(
    field: &str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), PluginCatalogError> {
    PluginReference::new(value.to_owned(), None, PluginTransport::Native)
        .map(|_| ())
        .map_err(|error| {
            field_error(
                field,
                format!("must be a valid reverse-DNS identity: {error}"),
            )
        })?;
    if value.len() > maximum_bytes {
        return invalid(
            field,
            format!("must not exceed {maximum_bytes} UTF-8 bytes"),
        );
    }
    Ok(())
}

fn validate_text(field: &str, value: &str, maximum_bytes: usize) -> Result<(), PluginCatalogError> {
    if value.is_empty() || value.len() > maximum_bytes {
        return invalid(
            field,
            format!("must contain 1 to {maximum_bytes} UTF-8 bytes"),
        );
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn is_safe_package_path(value: &str) -> bool {
    !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains('\\')
        && !value.contains("//")
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn invalid<T>(field: &str, message: impl Into<String>) -> Result<T, PluginCatalogError> {
    Err(field_error(field, message))
}

fn field_error(field: &str, message: impl Into<String>) -> PluginCatalogError {
    PluginCatalogError::InvalidField {
        field: field.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> PluginArtifactDescriptor {
        PluginArtifactDescriptor {
            schema_version: PLUGIN_CATALOG_SCHEMA_VERSION,
            plugin_id: "dev.vesper.catalog-fixture".to_owned(),
            version: "1.2.3".to_owned(),
            publisher: "dev.vesper.publisher".to_owned(),
            transport: PluginArtifactTransport::Native,
            target: "aarch64-apple-darwin".to_owned(),
            format: PluginArtifactFormat::Dylib,
            architecture: "arm64".to_owned(),
            abi_major: 1,
            abi_minor_min: 0,
            abi_minor_max: 0,
            capabilities: vec![PluginArtifactCapability {
                interface_id: "e9479dbc-42d2-575e-b39e-a24bc512fbc7".to_owned(),
                instance_id: "dev.vesper.catalog-fixture.primary".to_owned(),
            }],
            requires: Vec::new(),
            provides: Vec::new(),
            runtime_dependencies: Vec::new(),
            resource_policy: PluginResourcePolicy::default(),
            migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
        }
    }

    #[test]
    fn descriptor_canonicalization_is_order_independent_and_pure() {
        let mut left = descriptor();
        let mut right = descriptor();
        left.capabilities.push(PluginArtifactCapability {
            interface_id: "c7a69475-79b2-5b5e-a477-08844a5da5d1".to_owned(),
            instance_id: "dev.vesper.catalog-fixture.secondary".to_owned(),
        });
        right.capabilities = left.capabilities.iter().cloned().rev().collect();
        assert_eq!(
            left.canonicalize().expect("left canonical").sha256(),
            right.canonicalize().expect("right canonical").sha256()
        );
    }

    #[test]
    fn catalog_rejects_duplicate_identity_without_live_state() {
        let first = PluginCatalogRecord::new(
            descriptor(),
            "artifacts/first.dylib",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            PluginCatalogSource::Package,
        )
        .expect("first record");
        let second = PluginCatalogRecord::new(
            descriptor(),
            "artifacts/second.dylib",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            PluginCatalogSource::Package,
        )
        .expect("second record");
        assert!(matches!(
            PluginCatalog::from_records([first, second]),
            Err(PluginCatalogError::DuplicateIdentity { .. })
        ));
    }

    #[test]
    fn json_entrypoints_validate_metadata_and_rebuild_the_same_fingerprint() {
        let record = PluginCatalogRecord::new(
            descriptor(),
            "artifacts/fixture.dylib",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            PluginCatalogSource::Package,
        )
        .expect("record");
        let catalog = PluginCatalog::from_records([record]).expect("catalog");
        let bytes = catalog.to_json().expect("catalog json");
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json value");
        assert!(
            json[0]["descriptor"]["resource_policy"]
                .as_object()
                .expect("resource policy object")
                .values()
                .all(|value| !value.is_null())
        );
        let rebuilt = PluginCatalog::from_json(&bytes).expect("rebuilt catalog");
        assert_eq!(catalog.fingerprint(), rebuilt.fingerprint());
        let generic = serde_json::from_slice::<PluginCatalog>(&bytes).expect("generic catalog");
        assert_eq!(catalog.fingerprint(), generic.fingerprint());

        let mut invalid = bytes;
        invalid.extend_from_slice(b" ");
        let decoded = PluginCatalog::from_json(&invalid).expect("JSON whitespace is harmless");
        assert_eq!(decoded.fingerprint(), catalog.fingerprint());
    }

    #[test]
    fn package_records_reject_path_traversal_before_catalog_insertion() {
        let error = PluginCatalogRecord::new(
            descriptor(),
            "../escape.dylib",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            PluginCatalogSource::Package,
        )
        .expect_err("package traversal path");
        assert!(
            matches!(error, PluginCatalogError::InvalidField { ref field, .. } if field == "artifact_path")
        );
    }

    #[test]
    fn runtime_dependency_metadata_is_retained_and_validated() {
        let mut descriptor = descriptor();
        descriptor
            .runtime_dependencies
            .push(PluginRuntimeDependency {
                id: "dev.vesper.runtime".to_owned(),
                version: "1.0.0".to_owned(),
                linkage: PluginRuntimeLinkage::Dynamic,
                compatibility_key: "darwin-arm64".to_owned(),
            });
        let json = descriptor.to_json().expect("descriptor json");
        let decoded = PluginArtifactDescriptor::from_json(&json).expect("descriptor");
        assert_eq!(decoded.runtime_dependencies.len(), 1);
        assert_eq!(decoded.runtime_dependencies[0].id, "dev.vesper.runtime");
    }

    #[test]
    fn serde_deserialization_cannot_bypass_descriptor_validation() {
        let mut value = serde_json::to_value(descriptor()).expect("descriptor value");
        value["plugin_id"] = serde_json::json!("Invalid.Plugin");
        let error = serde_json::from_value::<PluginArtifactDescriptor>(value)
            .expect_err("invalid identity");
        assert!(error.to_string().contains("reverse-DNS"));
    }

    #[test]
    fn canonical_identity_key_separates_opaque_colon_fields() {
        let mut left = descriptor();
        left.target = "target:one".to_owned();
        left.architecture = "arch".to_owned();
        let mut right = descriptor();
        right.target = "target".to_owned();
        right.architecture = "one:arch".to_owned();
        let left = PluginCatalogRecord::new(
            left,
            "artifacts/left.dylib",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            PluginCatalogSource::Package,
        )
        .expect("left record");
        let right = PluginCatalogRecord::new(
            right,
            "artifacts/right.dylib",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            PluginCatalogSource::Package,
        )
        .expect("right record");
        assert_eq!(left.identity_key(), right.identity_key());
        assert_ne!(
            left.canonical_identity_key(),
            right.canonical_identity_key()
        );
        let catalog = PluginCatalog::from_records([left, right]).expect("distinct identities");
        assert_eq!(catalog.len(), 2);
    }
}
