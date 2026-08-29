//! Regression tests for the plugin runtime rewrite (contracts C-02 and C-05).
//!
//! The original W1 red failure shapes are retained in
//! `devnotes/plugin-runtime-rewrite-execution-ledger.md`; these tests now
//! exercise the separated catalog importer and transport policy.

use player_plugin::{
    PLUGIN_CATALOG_MIGRATION_VERSION, PLUGIN_CATALOG_SCHEMA_VERSION, PluginArtifactCapability,
    PluginArtifactDescriptor, PluginArtifactFormat, PluginArtifactTransport, PluginCatalogRecord,
    PluginCatalogSource, PluginReference, PluginTransport,
};
use player_plugin_loader::{
    PluginCatalogImporter, PluginInvocationWorkload, PluginRegistry, PluginSelectionError,
};

/// Catalog import does not open or load executable artifact bytes (C-02).
///
/// Target: importing an artifact into the catalog layer must not require the
/// artifact to be loadable. Artifact discovery and catalog queries are pure
/// metadata operations; loading plugin code is a runtime concern that happens
/// only after a plan is accepted, and load failures must surface as
/// structured diagnostics rather than aborting catalog import.
///
/// Old failure shape: `PluginRegistry::load_native_artifacts` fuses catalog
/// import with `dlopen`. A non-loadable path makes the whole registry build
/// return `Err(Load)`, so no catalog record can exist without live plugin
/// code being loaded first.
#[test]
fn catalog_import_survives_unloadable_artifact() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let artifact_path = directory.path().join("librewrite_red_not_a_library.so");
    std::fs::write(&artifact_path, b"rewrite-red: not a loadable image")
        .expect("placeholder artifact bytes");

    let descriptor = PluginArtifactDescriptor {
        schema_version: PLUGIN_CATALOG_SCHEMA_VERSION,
        plugin_id: "dev.vesper.rewrite-red.native".to_owned(),
        version: "1.0.0".to_owned(),
        publisher: "dev.vesper.rewrite-red".to_owned(),
        transport: PluginArtifactTransport::Native,
        target: "aarch64-apple-darwin".to_owned(),
        format: PluginArtifactFormat::Dylib,
        architecture: "arm64".to_owned(),
        abi_major: 1,
        abi_minor_min: 0,
        abi_minor_max: 0,
        capabilities: vec![PluginArtifactCapability {
            interface_id: "e9479dbc-42d2-575e-b39e-a24bc512fbc7".to_owned(),
            instance_id: "dev.vesper.rewrite-red.primary".to_owned(),
        }],
        requires: Vec::new(),
        provides: Vec::new(),
        runtime_dependencies: Vec::new(),
        resource_policy: Default::default(),
        migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
    };
    let record = PluginCatalogRecord::new(
        descriptor,
        artifact_path.to_string_lossy(),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        PluginCatalogSource::Development,
    )
    .expect("catalog record should be canonical");

    let mut importer = PluginCatalogImporter::new();
    importer
        .import_record(record)
        .expect("catalog import must succeed without loading the artifact (C-02)");

    let index = importer.index();
    assert_eq!(index.len(), 1);
    assert_eq!(
        index.records()[0].artifact_path(),
        artifact_path.to_string_lossy()
    );
}

/// Realtime media workloads reject the WASM transport by policy (C-05).
///
/// Target: transport selection is a workload policy decision, not a lookup
/// accident. Requesting a realtime media workload (native-frame decode or
/// frame processing) through the Wasm transport must fail with a typed
/// transport-workload policy rejection that is distinguishable from
/// "plugin not loaded", regardless of which artifacts exist.
///
/// Old failure shape: `resolve_native_decoder` and `resolve_frame_processor`
/// look a Wasm-transport reference up in the native plugin map and return the
/// generic `PluginSelectionError::PluginNotFound`, which is indistinguishable
/// from an unknown plugin id. "Wasm may not serve this workload" is not
/// expressible, so a loadable Wasm artifact could be mistaken for realtime
/// support.
#[test]
fn wasm_transport_realtime_workload_is_policy_rejected() {
    let registry = PluginRegistry::default();

    let reference =
        PluginReference::new("dev.vesper.rewrite-red.wasm", None, PluginTransport::Wasm)
            .expect("wasm reference should be canonical");

    let decoder_error = registry
        .resolve_native_decoder(&reference)
        .expect_err("wasm transport must be rejected for realtime decode workloads");
    assert!(matches!(
        decoder_error,
        PluginSelectionError::InvocationPolicyRejected(rejection)
            if rejection.workload() == PluginInvocationWorkload::RealtimeMedia
                && rejection.transport() == PluginTransport::Wasm
    ));

    let processor_error = registry
        .resolve_frame_processor(&reference)
        .expect_err("wasm transport must be rejected for realtime frame workloads");
    assert!(matches!(
        processor_error,
        PluginSelectionError::InvocationPolicyRejected(rejection)
            if rejection.workload() == PluginInvocationWorkload::RealtimeMedia
                && rejection.transport() == PluginTransport::Wasm
    ));
}
