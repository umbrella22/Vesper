//! Contract tests for the W2.2 catalog importer and W2.5 restart boundary.
//!
//! These tests keep artifact verification transactional and prove that a
//! metadata-only catalog can rebuild its derived index without opening native
//! images or retaining process-live state.

use player_plugin::{
    PLUGIN_CATALOG_MIGRATION_VERSION, PLUGIN_CATALOG_SCHEMA_VERSION, PluginArtifactCapability,
    PluginArtifactDescriptor, PluginArtifactFormat, PluginArtifactTransport, PluginCatalogRecord,
    PluginCatalogSource, PluginResourcePolicy,
};
use player_plugin_loader::{PluginCatalogImportError, PluginCatalogImporter};

fn descriptor_for(plugin_id: &str, version: &str) -> PluginArtifactDescriptor {
    PluginArtifactDescriptor {
        schema_version: PLUGIN_CATALOG_SCHEMA_VERSION,
        plugin_id: plugin_id.to_owned(),
        version: version.to_owned(),
        publisher: "dev.vesper.rewrite-red.publisher".to_owned(),
        transport: PluginArtifactTransport::Native,
        target: "aarch64-apple-darwin".to_owned(),
        format: PluginArtifactFormat::Dylib,
        architecture: "arm64".to_owned(),
        abi_major: 1,
        abi_minor_min: 0,
        abi_minor_max: 0,
        capabilities: vec![PluginArtifactCapability {
            interface_id: "e9479dbc-42d2-575e-b39e-a24bc512fbc7".to_owned(),
            instance_id: format!("{plugin_id}.primary"),
        }],
        requires: Vec::new(),
        provides: Vec::new(),
        runtime_dependencies: Vec::new(),
        resource_policy: PluginResourcePolicy::default(),
        migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
    }
}

fn descriptor() -> PluginArtifactDescriptor {
    descriptor_for("dev.vesper.rewrite-red.index", "1.0.0")
}

fn record(path: String, digest: &str) -> PluginCatalogRecord {
    PluginCatalogRecord::new(
        descriptor(),
        path,
        digest.to_owned(),
        PluginCatalogSource::Development,
    )
    .expect("catalog fixture should be valid")
}

fn record_for(plugin_id: &str, version: &str, path: String, digest: &str) -> PluginCatalogRecord {
    PluginCatalogRecord::new(
        descriptor_for(plugin_id, version),
        path,
        digest.to_owned(),
        PluginCatalogSource::Development,
    )
    .expect("catalog fixture should be valid")
}

#[test]
fn rewrite_red_catalog_import_rejects_stale_digest_without_mutating_index() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("rewrite-red-not-a-library.dylib");
    std::fs::write(&path, b"rewrite-red artifact bytes").expect("artifact bytes");

    let mut importer = PluginCatalogImporter::new();
    let error = importer
        .import_record_at(
            record(
                path.display().to_string(),
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            &path,
        )
        .expect_err("stale artifact digest must be rejected before catalog commit");

    assert!(matches!(
        error,
        PluginCatalogImportError::StaleDigest { .. }
    ));
    assert!(importer.index().is_empty());
}

#[test]
fn catalog_restart_rebuild_preserves_canonical_state_without_artifact_access() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let first_path = directory.path().join("missing-first.dylib");
    let second_path = directory.path().join("missing-second.dylib");
    let third_path = directory.path().join("missing-third.dylib");
    assert!(!first_path.exists());
    assert!(!second_path.exists());
    assert!(!third_path.exists());

    let mut importer = PluginCatalogImporter::new();
    importer
        .import_records([
            record_for(
                "dev.vesper.rewrite-red.index",
                "2.0.0",
                second_path.display().to_string(),
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            record_for(
                "dev.vesper.rewrite-red.alternate",
                "1.0.0",
                third_path.display().to_string(),
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            ),
            record_for(
                "dev.vesper.rewrite-red.index",
                "1.0.0",
                first_path.display().to_string(),
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
        ])
        .expect("metadata-only catalog import");

    let canonical_json = importer
        .index()
        .catalog()
        .to_json()
        .expect("canonical catalog JSON");
    let fingerprint = importer.index().fingerprint().to_owned();
    let expected_records = importer.index().records().to_vec();
    let identity_lookups = expected_records
        .iter()
        .map(|record| (record.canonical_identity_key(), record.clone()))
        .collect::<Vec<_>>();
    drop(importer);

    let rebuilt = PluginCatalogImporter::import_json(&canonical_json)
        .expect("restart must rebuild the derived index from canonical metadata");
    assert_eq!(rebuilt.fingerprint(), fingerprint);
    assert_eq!(
        rebuilt.catalog().to_json().expect("rebuilt catalog JSON"),
        canonical_json
    );
    assert_eq!(rebuilt.records(), expected_records);
    for (identity, expected) in identity_lookups {
        assert_eq!(rebuilt.get(&identity), Some(&expected));
    }
    assert_eq!(
        rebuilt
            .find("dev.vesper.rewrite-red.index")
            .map(|record| record.descriptor().version.as_str())
            .collect::<Vec<_>>(),
        vec!["1.0.0", "2.0.0"]
    );
    assert_eq!(rebuilt.find("dev.vesper.rewrite-red.alternate").count(), 1);
    assert!(!first_path.exists());
    assert!(!second_path.exists());
    assert!(!third_path.exists());
}
