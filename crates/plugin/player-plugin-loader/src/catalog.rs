//! Metadata-only plugin catalog importing and indexing.
//!
//! This module is deliberately separate from [`crate::PluginRegistry`].  It
//! validates manifest-owned values and artifact bytes, but it never opens a
//! dynamic library, creates a WASM instance, or retains a runtime owner.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use player_plugin::{PluginCatalog, PluginCatalogError, PluginCatalogRecord};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Upper bound for one streamed digest check.  Catalog import must remain
/// bounded even when a package points at an unexpectedly large file.
pub const MAX_PLUGIN_CATALOG_IMPORT_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const DIGEST_BUFFER_BYTES: usize = 64 * 1024;

/// A structured failure produced before a catalog candidate is committed.
#[derive(Debug, Error)]
pub enum PluginCatalogImportError {
    #[error(transparent)]
    Catalog(PluginCatalogError),
    #[error(
        "catalog import rejected duplicate identity `{identity}` from `{first_path}` and `{duplicate_path}`"
    )]
    DuplicateIdentity {
        identity: String,
        first_path: String,
        duplicate_path: String,
    },
    #[error("failed to read plugin artifact `{path}`: {source}")]
    ReadArtifact {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("plugin artifact `{path}` is not a regular file")]
    ArtifactNotFile { path: String },
    #[error(
        "plugin artifact `{path}` is {actual_bytes} bytes; maximum allowed for catalog import is {maximum_bytes}"
    )]
    ArtifactTooLarge {
        path: String,
        actual_bytes: u64,
        maximum_bytes: u64,
    },
    #[error("stale plugin artifact digest for `{path}`: declared {expected}, actual {actual}")]
    StaleDigest {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("catalog import path `{path}` is not valid: {message}")]
    InvalidPath { path: String, message: String },
}

impl From<PluginCatalogError> for PluginCatalogImportError {
    fn from(error: PluginCatalogError) -> Self {
        match error {
            PluginCatalogError::DuplicateIdentity {
                identity,
                first_path,
                duplicate_path,
            } => Self::DuplicateIdentity {
                identity,
                first_path,
                duplicate_path,
            },
            other => Self::Catalog(other),
        }
    }
}

/// A read-only, deterministic index over a validated catalog.
///
/// The index stores only offsets into the value-owned [`PluginCatalog`].  No
/// file descriptor, library handle, worker, queue, or media buffer can enter
/// this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCatalogIndex {
    catalog: PluginCatalog,
    identity_index: BTreeMap<String, usize>,
    plugin_index: BTreeMap<String, Vec<usize>>,
}

impl Default for PluginCatalogIndex {
    fn default() -> Self {
        Self {
            catalog: PluginCatalog::empty(),
            identity_index: BTreeMap::new(),
            plugin_index: BTreeMap::new(),
        }
    }
}

impl PluginCatalogIndex {
    /// Builds an index from a validated catalog without touching artifact paths.
    pub fn from_catalog(catalog: PluginCatalog) -> Result<Self, PluginCatalogImportError> {
        let mut identity_index = BTreeMap::new();
        let mut plugin_index: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, record) in catalog.records().iter().enumerate() {
            let canonical_identity = record.canonical_identity_key();
            if let Some(first_index) = identity_index.insert(canonical_identity, index) {
                let first_path = catalog.records()[first_index].artifact_path().to_owned();
                return Err(PluginCatalogImportError::DuplicateIdentity {
                    identity: record.identity_key(),
                    first_path,
                    duplicate_path: record.artifact_path().to_owned(),
                });
            }
            plugin_index
                .entry(record.descriptor().plugin_id.clone())
                .or_default()
                .push(index);
        }
        Ok(Self {
            catalog,
            identity_index,
            plugin_index,
        })
    }

    pub fn catalog(&self) -> &PluginCatalog {
        &self.catalog
    }

    pub fn records(&self) -> &[PluginCatalogRecord] {
        self.catalog.records()
    }

    pub fn len(&self) -> usize {
        self.catalog.len()
    }

    pub fn is_empty(&self) -> bool {
        self.catalog.is_empty()
    }

    pub fn fingerprint(&self) -> &str {
        self.catalog.fingerprint()
    }

    pub fn get(&self, identity: &str) -> Option<&PluginCatalogRecord> {
        self.identity_index
            .get(identity)
            .and_then(|index| self.catalog.records().get(*index))
    }

    pub fn find(&self, plugin_id: &str) -> impl Iterator<Item = &PluginCatalogRecord> {
        self.plugin_index
            .get(plugin_id)
            .into_iter()
            .flat_map(|indices| indices.iter())
            .filter_map(|index| self.catalog.records().get(*index))
    }
}

/// Transactional importer for catalog records.
///
/// Each mutation builds a complete candidate catalog first.  If validation or
/// digest verification fails, the existing index remains byte-for-byte
/// unchanged and no runtime owner has been created.
#[derive(Debug, Clone, Default)]
pub struct PluginCatalogImporter {
    index: PluginCatalogIndex,
}

impl PluginCatalogImporter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_catalog(catalog: PluginCatalog) -> Result<Self, PluginCatalogImportError> {
        Ok(Self {
            index: PluginCatalogIndex::from_catalog(catalog)?,
        })
    }

    /// Creates an immutable index in one step from catalog records.
    pub fn import(
        records: impl IntoIterator<Item = PluginCatalogRecord>,
    ) -> Result<PluginCatalogIndex, PluginCatalogImportError> {
        let catalog =
            PluginCatalog::from_records(records).map_err(PluginCatalogImportError::from)?;
        PluginCatalogIndex::from_catalog(catalog)
    }

    /// Creates an immutable index from a canonical catalog JSON snapshot.
    pub fn import_json(bytes: &[u8]) -> Result<PluginCatalogIndex, PluginCatalogImportError> {
        let catalog = PluginCatalog::from_json(bytes).map_err(PluginCatalogImportError::from)?;
        PluginCatalogIndex::from_catalog(catalog)
    }

    pub fn index(&self) -> &PluginCatalogIndex {
        &self.index
    }

    pub fn into_index(self) -> PluginCatalogIndex {
        self.index
    }

    /// Imports metadata without reading the artifact path.
    pub fn import_record(
        &mut self,
        record: PluginCatalogRecord,
    ) -> Result<(), PluginCatalogImportError> {
        self.commit_records(std::iter::once(record))
    }

    /// Imports a batch atomically.  A single invalid or duplicate record
    /// leaves the previous index untouched.
    pub fn import_records(
        &mut self,
        records: impl IntoIterator<Item = PluginCatalogRecord>,
    ) -> Result<(), PluginCatalogImportError> {
        self.commit_records(records)
    }

    pub fn import_json_into(&mut self, bytes: &[u8]) -> Result<(), PluginCatalogImportError> {
        let catalog = PluginCatalog::from_json(bytes).map_err(PluginCatalogImportError::from)?;
        self.commit_records(catalog.records().iter().cloned())
    }

    /// Verifies one artifact's bytes and commits its metadata only after the
    /// digest matches.  The file is streamed and never loaded as a runtime.
    pub fn import_record_at(
        &mut self,
        record: PluginCatalogRecord,
        path: impl AsRef<Path>,
    ) -> Result<(), PluginCatalogImportError> {
        let path = path.as_ref();
        record.validate().map_err(PluginCatalogImportError::from)?;
        verify_artifact_digest(&record, path)?;
        self.import_record(record)
    }

    /// Resolves a record path against a package/install root, verifies its
    /// digest, and commits it atomically. Absolute installed paths are kept.
    pub fn import_record_from_root(
        &mut self,
        record: PluginCatalogRecord,
        root: impl AsRef<Path>,
    ) -> Result<(), PluginCatalogImportError> {
        let path = resolve_record_path(&record, root.as_ref())?;
        self.import_record_at(record, path)
    }

    pub fn import_records_from_root(
        &mut self,
        records: impl IntoIterator<Item = PluginCatalogRecord>,
        root: impl AsRef<Path>,
    ) -> Result<(), PluginCatalogImportError> {
        let root = root.as_ref();
        let records = records.into_iter().collect::<Vec<_>>();
        for record in &records {
            record.validate().map_err(PluginCatalogImportError::from)?;
            let path = resolve_record_path(record, root)?;
            verify_artifact_digest(record, &path)?;
        }
        self.commit_records(records)
    }

    fn commit_records(
        &mut self,
        records: impl IntoIterator<Item = PluginCatalogRecord>,
    ) -> Result<(), PluginCatalogImportError> {
        let mut candidate = self.index.records().to_vec();
        candidate.extend(records);
        let catalog =
            PluginCatalog::from_records(candidate).map_err(PluginCatalogImportError::from)?;
        let index = PluginCatalogIndex::from_catalog(catalog)?;
        self.index = index;
        Ok(())
    }
}

fn resolve_record_path(
    record: &PluginCatalogRecord,
    root: &Path,
) -> Result<PathBuf, PluginCatalogImportError> {
    if root.as_os_str().is_empty() {
        return Err(PluginCatalogImportError::InvalidPath {
            path: record.artifact_path().to_owned(),
            message: "root path must not be empty".to_owned(),
        });
    }
    let path = Path::new(record.artifact_path());
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(root.join(path))
    }
}

fn verify_artifact_digest(
    record: &PluginCatalogRecord,
    path: &Path,
) -> Result<(), PluginCatalogImportError> {
    let path_string = path.display().to_string();
    let metadata =
        std::fs::metadata(path).map_err(|source| PluginCatalogImportError::ReadArtifact {
            path: path_string.clone(),
            source,
        })?;
    if !metadata.is_file() {
        return Err(PluginCatalogImportError::ArtifactNotFile { path: path_string });
    }
    if metadata.len() > MAX_PLUGIN_CATALOG_IMPORT_ARTIFACT_BYTES {
        return Err(PluginCatalogImportError::ArtifactTooLarge {
            path: path_string,
            actual_bytes: metadata.len(),
            maximum_bytes: MAX_PLUGIN_CATALOG_IMPORT_ARTIFACT_BYTES,
        });
    }

    let mut file = File::open(path).map_err(|source| PluginCatalogImportError::ReadArtifact {
        path: path.display().to_string(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; DIGEST_BUFFER_BYTES];
    let mut total_read = 0_u64;
    loop {
        let read =
            file.read(&mut buffer)
                .map_err(|source| PluginCatalogImportError::ReadArtifact {
                    path: path.display().to_string(),
                    source,
                })?;
        if read == 0 {
            break;
        }
        total_read = total_read.saturating_add(read as u64);
        if total_read > MAX_PLUGIN_CATALOG_IMPORT_ARTIFACT_BYTES {
            return Err(PluginCatalogImportError::ArtifactTooLarge {
                path: path.display().to_string(),
                actual_bytes: total_read,
                maximum_bytes: MAX_PLUGIN_CATALOG_IMPORT_ARTIFACT_BYTES,
            });
        }
        hasher.update(&buffer[..read]);
    }
    let actual = hex::encode(hasher.finalize());
    if actual != record.artifact_sha256() {
        return Err(PluginCatalogImportError::StaleDigest {
            path: path.display().to_string(),
            expected: record.artifact_sha256().to_owned(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_plugin::{
        PLUGIN_CATALOG_MIGRATION_VERSION, PLUGIN_CATALOG_SCHEMA_VERSION, PluginArtifactCapability,
        PluginArtifactDescriptor, PluginArtifactFormat, PluginArtifactTransport,
        PluginCatalogSource, PluginResourcePolicy,
    };

    fn descriptor(plugin_id: &str, instance_id: &str) -> PluginArtifactDescriptor {
        PluginArtifactDescriptor {
            schema_version: PLUGIN_CATALOG_SCHEMA_VERSION,
            plugin_id: plugin_id.to_owned(),
            version: "1.0.0".to_owned(),
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
                instance_id: instance_id.to_owned(),
            }],
            requires: Vec::new(),
            provides: Vec::new(),
            runtime_dependencies: Vec::new(),
            resource_policy: PluginResourcePolicy::default(),
            migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
        }
    }

    fn record(plugin_id: &str, path: &str) -> PluginCatalogRecord {
        record_with_digest(
            plugin_id,
            path,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
    }

    fn record_with_digest(plugin_id: &str, path: &str, digest: &str) -> PluginCatalogRecord {
        PluginCatalogRecord::new(
            descriptor(plugin_id, &format!("{plugin_id}.primary")),
            path,
            digest,
            PluginCatalogSource::Development,
        )
        .expect("fixture record")
    }

    #[test]
    fn failed_duplicate_import_keeps_the_previous_index() {
        let mut importer = PluginCatalogImporter::new();
        importer
            .import_record(record("dev.vesper.one", "/tmp/one"))
            .expect("first record");
        let before = importer.index().fingerprint().to_owned();
        let error = importer
            .import_record(record("dev.vesper.one", "/tmp/duplicate"))
            .expect_err("duplicate identity");
        assert!(matches!(
            error,
            PluginCatalogImportError::DuplicateIdentity { .. }
        ));
        assert_eq!(before, importer.index().fingerprint());
        assert_eq!(importer.index().len(), 1);
    }

    #[test]
    fn json_import_rebuilds_an_immutable_index() {
        let record = record("dev.vesper.one", "/tmp/one");
        let catalog = PluginCatalog::from_records([record]).expect("catalog");
        let bytes = catalog.to_json().expect("json");
        let index = PluginCatalogImporter::import_json(&bytes).expect("index");
        assert_eq!(index.fingerprint(), catalog.fingerprint());
        assert_eq!(index.find("dev.vesper.one").count(), 1);
    }

    #[test]
    fn verified_artifact_import_commits_only_after_digest_match() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("fixture.bin");
        let bytes = b"catalog importer fixture";
        std::fs::write(&path, bytes).expect("artifact bytes");
        let digest = hex::encode(Sha256::digest(bytes));
        let mut importer = PluginCatalogImporter::new();

        importer
            .import_record_at(
                record_with_digest("dev.vesper.one", &path.to_string_lossy(), &digest),
                &path,
            )
            .expect("matching digest");
        let before = importer.index().fingerprint().to_owned();
        let error = importer
            .import_record_at(
                PluginCatalogRecord::new(
                    descriptor("dev.vesper.two", "dev.vesper.two.primary"),
                    path.to_string_lossy(),
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    PluginCatalogSource::Development,
                )
                .expect("stale record"),
                &path,
            )
            .expect_err("stale digest");
        assert!(matches!(
            error,
            PluginCatalogImportError::StaleDigest { .. }
        ));
        assert_eq!(before, importer.index().fingerprint());
        assert_eq!(importer.index().len(), 1);
    }
}
