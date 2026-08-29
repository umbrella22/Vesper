//! RED TESTS and canonical-schema checks for the plugin runtime rewrite
//! (W1/W2.3, contracts C-01, C-03, C-12).
//!
//! These tests intentionally FAIL against the current implementation. They
//! capture the target manifest contracts from
//! `devnotes/plugin-runtime-rewrite-development-plan.md`. The ABI migration
//! check remains red until its later wave; the requires/provides checks are
//! canonical-schema checks after W2.3. Do not weaken, skip, or delete them;
//! deterministic red failure shapes are recorded in
//! `devnotes/plugin-runtime-rewrite-execution-ledger.md`.

use player_plugin_package::PluginDescriptor;
use semver::Version;

fn rewrite_red_descriptor_toml() -> String {
    r#"
schema_version = 1

[plugin]
id = "dev.vesper.rewrite-red.fixture"
name = "Rewrite Red Fixture"
version = "1.2.3"
description = "Fixture for plugin runtime rewrite red tests"
license = "Apache-2.0"
publisher = "dev.vesper.rewrite-red.publisher"

[compatibility]
host_sdk = ">=0.4.0, <0.5.0"
abi_major = 1
abi_minor_min = 0
abi_minor_max = 0

[[capabilities]]
interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7"
instance_id = "dev.vesper.rewrite-red.fixture.primary"
interface_major = 1
interface_minor = 0
stability = "stable"
"#
    .to_owned()
}

/// RED TEST (plugin-runtime-rewrite W1, contract C-03).
///
/// Target: the canonical plugin manifest must carry typed requirement
/// declarations (service identity and semver constraints) so a later resolver
/// can build a dependency graph, detect cycles, and diagnose missing or
/// conflicting providers.
///
/// The old failure shape was a generic TOML unknown-field error for the
/// dependency section. The canonical `requires` section is now the typed
/// replacement; graph solving remains outside W2.3.
#[test]
fn rewrite_red_requires_declarations_have_typed_manifest_semantics() {
    let source = format!(
        "{}\n[[requires]]\nservice = \"dev.vesper.rewrite-red.service.time-stretch\"\nrequirement = \">=1.0.0, <2.0.0\"\n",
        rewrite_red_descriptor_toml()
    );

    let descriptor = PluginDescriptor::from_toml(&source)
        .expect("dependency declarations must parse into the canonical manifest schema (C-03)");

    descriptor
        .canonicalize()
        .expect("declared dependencies must canonicalize deterministically (C-03)");
}

/// RED TEST (plugin-runtime-rewrite W1, contracts C-01 and C-12).
///
/// Target: rejecting a legacy or future ABI artifact must be a typed failure
/// that names the rejected artifact identity and points the author at the
/// migration entry (guide identity/version), so the rejection is actionable
/// without reading loader internals.
///
/// Old failure shape: `evaluate_current_host_compatibility` returns
/// `AbiMajorMismatch` whose message carries only the numeric major versions.
/// It names neither the plugin whose manifest was rejected nor any migration
/// entry, so authors cannot map the rejection onto a migration workflow.
#[test]
fn rewrite_red_legacy_abi_rejection_carries_migration_entry_and_identity() {
    let source = rewrite_red_descriptor_toml().replace("abi_major = 1", "abi_major = 2");
    let descriptor =
        PluginDescriptor::from_toml(&source).expect("future ABI descriptor should parse");

    let error = descriptor
        .evaluate_current_host_compatibility(&Version::new(0, 4, 3))
        .expect_err("a future ABI major must be rejected by the current host");

    let message = error.to_string();
    assert!(
        message.to_lowercase().contains("migrat"),
        "ABI rejection must point authors at the migration entry (C-01/C-12), got: {message}"
    );
    assert!(
        message.contains("dev.vesper.rewrite-red.fixture"),
        "ABI rejection must identify the rejected artifact (C-01), got: {message}"
    );
}

/// RED TEST (plugin-runtime-rewrite W2.3, contract C-03).
///
/// The author manifest and generated descriptor must expose the same
/// requires/provides arrays. Their canonical JSON is the input to the later
/// resolver, so declaration order cannot change the golden representation.
#[test]
fn rewrite_red_requires_provides_have_canonical_golden_shape() {
    let source = format!(
        "{}\n[[requires]]\nservice = \"dev.vesper.rewrite-red.service.time-stretch\"\nrequirement = \">=1.0.0, <2.0.0\"\n\n[[provides]]\nservice = \"dev.vesper.rewrite-red.service.time-stretch\"\nversion = \"1.4.0\"\n",
        rewrite_red_descriptor_toml()
    );

    let descriptor = PluginDescriptor::from_toml(&source)
        .expect("requires/provides must parse in the canonical author schema (C-03)");
    let canonical = descriptor
        .canonicalize()
        .expect("requires/provides must have deterministic canonical JSON (C-03)");
    let json: serde_json::Value =
        serde_json::from_slice(canonical.json()).expect("canonical descriptor JSON");
    assert_eq!(
        json["requires"][0]["service"],
        "dev.vesper.rewrite-red.service.time-stretch"
    );
    assert_eq!(json["requires"][0]["requirement"], ">=1.0.0, <2.0.0");
    assert_eq!(
        json["provides"][0]["service"],
        "dev.vesper.rewrite-red.service.time-stretch"
    );
    assert_eq!(json["provides"][0]["version"], "1.4.0");
}

/// RED TEST (plugin-runtime-rewrite W2.3, contract C-03).
///
/// Schema validation must reject duplicate services and malformed semver
/// ranges before a catalog record is produced. The dependency graph itself
/// is deliberately not solved in W2.3.
#[test]
fn rewrite_red_requires_reject_duplicate_and_invalid_semver() {
    let duplicate = format!(
        "{}\n[[requires]]\nservice = \"dev.vesper.rewrite-red.service.time-stretch\"\nrequirement = \">=1.0.0\"\n\n[[requires]]\nservice = \"dev.vesper.rewrite-red.service.time-stretch\"\nrequirement = \"<2.0.0\"\n",
        rewrite_red_descriptor_toml()
    );
    let duplicate_error = PluginDescriptor::from_toml(&duplicate)
        .expect_err("duplicate requirement services must be rejected");
    assert!(duplicate_error.to_string().contains("duplicate"));

    let invalid = format!(
        "{}\n[[requires]]\nservice = \"dev.vesper.rewrite-red.service.time-stretch\"\nrequirement = \"not-semver\"\n",
        rewrite_red_descriptor_toml()
    );
    let invalid_error = PluginDescriptor::from_toml(&invalid)
        .expect_err("invalid requirement semver must be rejected");
    assert!(invalid_error.to_string().contains("semver"));
}

/// RED TEST (plugin-runtime-rewrite W2.3, contract C-03).
///
/// Unknown declaration fields must remain a visible schema failure. W2.3
/// does not silently drop an extension that a later schema version might
/// understand.
#[test]
fn rewrite_red_unknown_requirement_field_is_not_silently_dropped() {
    let source = format!(
        "{}\n[[requires]]\nservice = \"dev.vesper.rewrite-red.service.time-stretch\"\nrequirement = \">=1.0.0\"\nunknown_predicate = \"platform\"\n",
        rewrite_red_descriptor_toml()
    );
    let error = PluginDescriptor::from_toml(&source)
        .expect_err("unknown requirement fields must be rejected or versioned explicitly");
    let message = error.to_string();
    assert!(
        message.contains("unknown field") && message.contains("unknown_predicate"),
        "unknown requirement field must remain observable, got: {message}"
    );
}

/// RED TEST (plugin-runtime-rewrite W2.3, contract C-03).
///
/// A declaration that forms a future dependency cycle is still valid catalog
/// input. Cycle detection belongs to W3's resolver; W2.3 must preserve the
/// edge in canonical metadata without attempting to instantiate a plugin.
#[test]
fn rewrite_red_cycle_declarations_are_preserved_for_the_resolver() {
    let source = format!(
        "{}\n[[requires]]\nservice = \"dev.vesper.rewrite-red.service.time-stretch\"\nrequirement = \">=1.0.0\"\n\n[[provides]]\nservice = \"dev.vesper.rewrite-red.service.time-stretch\"\nversion = \"1.4.0\"\n",
        rewrite_red_descriptor_toml()
    );
    let descriptor = PluginDescriptor::from_toml(&source)
        .expect("cycle edges are valid metadata and must reach the resolver");
    let canonical = descriptor
        .canonicalize()
        .expect("cycle metadata must canonicalize without a solver");
    assert!(
        canonical
            .json()
            .windows(b"requires".len())
            .any(|window| { window == b"requires" })
    );
}
