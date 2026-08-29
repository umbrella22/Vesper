use crate::{
    MAX_PLUGIN_RUNTIME_OWNER_REGISTRATIONS, MAX_PLUGIN_RUNTIME_SCOPE_REGISTRATIONS,
    MAX_PLUGIN_SCOPE_CHILDREN, MAX_PLUGIN_SCOPE_DEPTH, MAX_PLUGIN_SCOPE_OWNERS, PluginPlan,
    PluginPlanError, PluginRegistry, PluginResolutionError, PluginResolver, PluginResolverPolicy,
    PluginRuntime, PluginScopeError, PluginScopeKind, PluginScopeQuarantineReason,
    PluginScopeResource, PluginScopeState, PluginSelectionError,
};
use player_plugin::{
    PLUGIN_CATALOG_MIGRATION_VERSION, PLUGIN_CATALOG_SCHEMA_VERSION,
    PluginActivePlaybackCorrelation, PluginArtifactCapability, PluginArtifactDescriptor,
    PluginArtifactFormat, PluginArtifactTransport, PluginCatalog, PluginCatalogRecord,
    PluginCatalogSource, PluginInvocationWorkload, PluginNextPrewarmCorrelation,
    PluginPlaybackAuthority, PluginPlaybackError, PluginPlaybackRole, PluginProvision,
    PluginReference, PluginRequirement, PluginResourcePolicy, PluginSessionCorrelation,
    PluginTransport,
};

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn assert_wasm_realtime_rejection(error: PluginSelectionError) {
    assert!(matches!(
        error,
        PluginSelectionError::InvocationPolicyRejected(rejection)
            if rejection.workload() == PluginInvocationWorkload::RealtimeMedia
                && rejection.transport() == PluginTransport::Wasm
    ));
}

#[test]
fn plugin_registry_rejects_wasm_realtime_workloads_before_artifact_lookup() {
    let registry = PluginRegistry::default();
    let reference =
        PluginReference::new("dev.vesper.transport-policy", None, PluginTransport::Wasm)
            .expect("wasm reference");

    assert_wasm_realtime_rejection(
        registry
            .resolve_native_decoder(&reference)
            .expect_err("WASM decoder policy rejection"),
    );
    assert_wasm_realtime_rejection(
        registry
            .resolve_frame_processor(&reference)
            .expect_err("WASM frame processor policy rejection"),
    );
    assert_wasm_realtime_rejection(
        registry
            .resolve_source_packet(&reference)
            .expect_err("WASM packet normalizer policy rejection"),
    );
    assert_wasm_realtime_rejection(
        registry
            .resolve_source_resource(&reference)
            .expect_err("WASM resource normalizer policy rejection"),
    );

    assert!(matches!(
        registry.resolve_pipeline_event_hook(&reference),
        Err(PluginSelectionError::PluginNotFound { .. })
    ));
    assert!(matches!(
        registry.resolve_benchmark_sink(&reference),
        Err(PluginSelectionError::PluginNotFound { .. })
    ));
}

fn requirement(service: &str, requirement: &str) -> PluginRequirement {
    PluginRequirement {
        service: service.to_owned(),
        requirement: requirement.to_owned(),
    }
}

fn record(
    plugin_id: &str,
    plugin_version: &str,
    provides: &[(&str, &str)],
    requires: &[(&str, &str)],
) -> PluginCatalogRecord {
    let descriptor = PluginArtifactDescriptor {
        schema_version: PLUGIN_CATALOG_SCHEMA_VERSION,
        plugin_id: plugin_id.to_owned(),
        version: plugin_version.to_owned(),
        publisher: "dev.vesper.resolver.publisher".to_owned(),
        transport: PluginArtifactTransport::Native,
        target: "aarch64-apple-darwin".to_owned(),
        format: PluginArtifactFormat::Dylib,
        architecture: "arm64".to_owned(),
        abi_major: 1,
        abi_minor_min: 0,
        abi_minor_max: 2,
        capabilities: vec![PluginArtifactCapability {
            interface_id: "e9479dbc-42d2-575e-b39e-a24bc512fbc7".to_owned(),
            instance_id: format!("{plugin_id}.primary"),
        }],
        requires: requires
            .iter()
            .map(|(service, version)| requirement(service, version))
            .collect(),
        provides: provides
            .iter()
            .map(|(service, version)| PluginProvision {
                service: (*service).to_owned(),
                version: (*version).to_owned(),
            })
            .collect(),
        runtime_dependencies: Vec::new(),
        resource_policy: PluginResourcePolicy::default(),
        migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
    };
    PluginCatalogRecord::new(
        descriptor,
        format!("/virtual/{plugin_id}-{plugin_version}.dylib"),
        DIGEST,
        PluginCatalogSource::Development,
    )
    .expect("resolver fixture")
}

fn policy() -> PluginResolverPolicy {
    PluginResolverPolicy::new(
        PluginArtifactTransport::Native,
        "aarch64-apple-darwin",
        "arm64",
        1,
        1,
    )
    .expect("resolver policy")
}

#[test]
fn resolver_is_deterministic_across_catalog_order_and_stable_identity_ties() {
    let alpha = record(
        "dev.vesper.resolver.alpha",
        "1.0.0",
        &[("dev.vesper.service.effect", "2.0.0")],
        &[],
    );
    let beta = record(
        "dev.vesper.resolver.beta",
        "1.0.0",
        &[("dev.vesper.service.effect", "2.0.0")],
        &[],
    );
    let root = [requirement("dev.vesper.service.effect", ">=1.0.0, <3.0.0")];

    let forward = PluginCatalog::from_records([beta.clone(), alpha.clone()]).expect("catalog");
    let reverse = PluginCatalog::from_records([alpha, beta]).expect("catalog");
    let first = PluginResolver::new(&forward, policy())
        .resolve(&root)
        .expect("resolution");
    let second = PluginResolver::new(&reverse, policy())
        .resolve(&root)
        .expect("resolution");

    assert_eq!(first, second);
    assert_eq!(first.providers().len(), 1);
    assert_eq!(
        first.providers()[0].artifact().descriptor().plugin_id,
        "dev.vesper.resolver.alpha"
    );
}

#[test]
fn resolver_applies_semver_then_explicit_priority() {
    let old_preferred = record(
        "dev.vesper.resolver.preferred",
        "1.0.0",
        &[("dev.vesper.service.effect", "1.5.0")],
        &[],
    );
    let current = record(
        "dev.vesper.resolver.current",
        "1.0.0",
        &[("dev.vesper.service.effect", "2.0.0")],
        &[],
    );
    let catalog = PluginCatalog::from_records([old_preferred, current]).expect("catalog");
    let mut old_provider_policy = policy();
    old_provider_policy
        .set_plugin_priority("dev.vesper.resolver.preferred", 100)
        .expect("priority");

    let resolution = PluginResolver::new(&catalog, old_provider_policy)
        .resolve(&[requirement("dev.vesper.service.effect", ">=1.0.0, <3.0.0")])
        .expect("resolution");
    assert_eq!(
        resolution.providers()[0].artifact().descriptor().plugin_id,
        "dev.vesper.resolver.current"
    );

    let priority_candidate = record(
        "dev.vesper.resolver.priority",
        "1.0.0",
        &[("dev.vesper.service.priority", "2.0.0")],
        &[],
    );
    let newer_plugin = record(
        "dev.vesper.resolver.newer-plugin",
        "3.0.0",
        &[("dev.vesper.service.priority", "2.0.0")],
        &[],
    );
    let priority_catalog =
        PluginCatalog::from_records([newer_plugin, priority_candidate]).expect("catalog");
    let priority_requirement = [requirement("dev.vesper.service.priority", ">=2.0.0")];

    let default_resolution = PluginResolver::new(&priority_catalog, policy())
        .resolve(&priority_requirement)
        .expect("default resolution");
    assert_eq!(
        default_resolution.providers()[0]
            .artifact()
            .descriptor()
            .plugin_id,
        "dev.vesper.resolver.newer-plugin"
    );

    let mut priority_policy = policy();
    priority_policy
        .set_plugin_priority("dev.vesper.resolver.priority", 1)
        .expect("priority");
    let priority_resolution = PluginResolver::new(&priority_catalog, priority_policy)
        .resolve(&priority_requirement)
        .expect("priority resolution");
    assert_eq!(
        priority_resolution.providers()[0]
            .artifact()
            .descriptor()
            .plugin_id,
        "dev.vesper.resolver.priority"
    );
}

#[test]
fn resolver_reports_missing_and_version_conflict_as_distinct_typed_failures() {
    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.resolver.old",
        "1.0.0",
        &[("dev.vesper.service.effect", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let resolver = PluginResolver::new(&catalog, policy());

    let missing = resolver
        .resolve(&[requirement("dev.vesper.service.missing", ">=1.0.0")])
        .expect_err("missing provider");
    assert!(matches!(
        missing,
        PluginResolutionError::MissingProvider { ref service, .. }
            if service == "dev.vesper.service.missing"
    ));

    let conflict = resolver
        .resolve(&[requirement("dev.vesper.service.effect", ">=2.0.0")])
        .expect_err("version conflict");
    assert!(matches!(
        conflict,
        PluginResolutionError::VersionConflict { ref service, .. }
            if service == "dev.vesper.service.effect"
    ));
}

#[test]
fn resolver_returns_dependency_first_order_and_rejects_cycles() {
    let dependency = record(
        "dev.vesper.resolver.dependency",
        "1.0.0",
        &[("dev.vesper.service.dependency", "1.0.0")],
        &[],
    );
    let root = record(
        "dev.vesper.resolver.root",
        "1.0.0",
        &[("dev.vesper.service.root", "1.0.0")],
        &[("dev.vesper.service.dependency", ">=1.0.0")],
    );
    let catalog = PluginCatalog::from_records([root, dependency]).expect("catalog");
    let resolution = PluginResolver::new(&catalog, policy())
        .resolve(&[requirement("dev.vesper.service.root", ">=1.0.0")])
        .expect("acyclic resolution");
    assert_eq!(
        resolution
            .artifacts()
            .iter()
            .map(|record| record.descriptor().plugin_id.as_str())
            .collect::<Vec<_>>(),
        vec!["dev.vesper.resolver.dependency", "dev.vesper.resolver.root"]
    );

    let cycle_a = record(
        "dev.vesper.resolver.cycle-a",
        "1.0.0",
        &[("dev.vesper.service.cycle-a", "1.0.0")],
        &[("dev.vesper.service.cycle-b", ">=1.0.0")],
    );
    let cycle_b = record(
        "dev.vesper.resolver.cycle-b",
        "1.0.0",
        &[("dev.vesper.service.cycle-b", "1.0.0")],
        &[("dev.vesper.service.cycle-a", ">=1.0.0")],
    );
    let cyclic = PluginCatalog::from_records([cycle_b, cycle_a]).expect("catalog");
    let error = PluginResolver::new(&cyclic, policy())
        .resolve(&[requirement("dev.vesper.service.cycle-a", ">=1.0.0")])
        .expect_err("dependency cycle");
    assert!(matches!(
        error,
        PluginResolutionError::DependencyCycle { ref artifact_identities }
            if artifact_identities.len() >= 2
    ));
}

#[test]
fn resolver_backtracks_from_a_higher_version_with_unsatisfied_dependencies() {
    let high = record(
        "dev.vesper.resolver.high",
        "2.0.0",
        &[("dev.vesper.service.root", "2.0.0")],
        &[("dev.vesper.service.absent", ">=1.0.0")],
    );
    let fallback = record(
        "dev.vesper.resolver.fallback",
        "1.0.0",
        &[("dev.vesper.service.root", "1.0.0")],
        &[],
    );
    let catalog = PluginCatalog::from_records([fallback, high]).expect("catalog");
    let resolution = PluginResolver::new(&catalog, policy())
        .resolve(&[requirement("dev.vesper.service.root", ">=1.0.0")])
        .expect("lower provider must remain a valid deterministic solution");

    assert_eq!(
        resolution.providers()[0].artifact().descriptor().plugin_id,
        "dev.vesper.resolver.fallback"
    );
}

#[test]
fn resolver_rejects_transitive_version_and_plugin_identity_conflicts() {
    let root = record(
        "dev.vesper.resolver.root-conflict",
        "1.0.0",
        &[("dev.vesper.service.a-root", "1.0.0")],
        &[("dev.vesper.service.shared", "<2.0.0")],
    );
    let shared = record(
        "dev.vesper.resolver.shared",
        "2.0.0",
        &[("dev.vesper.service.shared", "2.0.0")],
        &[],
    );
    let version_catalog = PluginCatalog::from_records([shared, root]).expect("catalog");
    let version_error = PluginResolver::new(&version_catalog, policy())
        .resolve(&[
            requirement("dev.vesper.service.a-root", ">=1.0.0"),
            requirement("dev.vesper.service.shared", ">=2.0.0"),
        ])
        .expect_err("incompatible host and transitive requirements");
    assert!(matches!(
        version_error,
        PluginResolutionError::VersionConflict { ref service, .. }
            if service == "dev.vesper.service.shared"
    ));

    let first = record(
        "dev.vesper.resolver.same-plugin",
        "1.0.0",
        &[("dev.vesper.service.a-first", "1.0.0")],
        &[],
    );
    let second = record(
        "dev.vesper.resolver.same-plugin",
        "2.0.0",
        &[("dev.vesper.service.b-second", "1.0.0")],
        &[],
    );
    let identity_catalog = PluginCatalog::from_records([second, first]).expect("catalog");
    let identity_error = PluginResolver::new(&identity_catalog, policy())
        .resolve(&[
            requirement("dev.vesper.service.a-first", ">=1.0.0"),
            requirement("dev.vesper.service.b-second", ">=1.0.0"),
        ])
        .expect_err("one plugin identity cannot select two artifact versions");
    assert!(matches!(
        identity_error,
        PluginResolutionError::PluginIdentityConflict { ref plugin_id, .. }
            if plugin_id == "dev.vesper.resolver.same-plugin"
    ));
}

#[test]
fn resolver_policy_validation_and_compatibility_filter_are_typed() {
    assert!(matches!(
        PluginResolverPolicy::new(
            PluginArtifactTransport::Native,
            "",
            "arm64",
            1,
            0,
        ),
        Err(PluginResolutionError::InvalidPolicy { ref field, .. }) if field == "target"
    ));
    assert!(matches!(
        PluginResolverPolicy::new(
            PluginArtifactTransport::Native,
            "aarch64-apple-darwin",
            "arm64",
            0,
            0,
        ),
        Err(PluginResolutionError::InvalidPolicy { ref field, .. }) if field == "abi_major"
    ));
    let mut invalid_priority = policy();
    assert!(matches!(
        invalid_priority.set_plugin_priority("invalid", 1),
        Err(PluginResolutionError::InvalidPolicy { ref field, .. })
            if field == "plugin_priorities.plugin_id"
    ));

    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.resolver.policy",
        "1.0.0",
        &[("dev.vesper.service.policy", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let incompatible_policy = PluginResolverPolicy::new(
        PluginArtifactTransport::Native,
        "x86_64-unknown-linux-gnu",
        "x86_64",
        1,
        1,
    )
    .expect("policy");
    let error = PluginResolver::new(&catalog, incompatible_policy)
        .resolve(&[requirement("dev.vesper.service.policy", ">=1.0.0")])
        .expect_err("target mismatch");
    assert!(matches!(
        error,
        PluginResolutionError::MissingProvider {
            catalog_candidates: 1,
            policy_candidates: 0,
            ..
        }
    ));
}

#[test]
fn plugin_plan_is_canonical_round_trippable_and_rejects_noncanonical_order() {
    let dependency = record(
        "dev.vesper.plan.dependency",
        "1.0.0",
        &[("dev.vesper.service.plan-dependency", "1.0.0")],
        &[],
    );
    let root = record(
        "dev.vesper.plan.root",
        "2.0.0",
        &[("dev.vesper.service.plan-root", "2.0.0")],
        &[("dev.vesper.service.plan-dependency", ">=1.0.0")],
    );
    let requirements = [
        requirement("dev.vesper.service.plan-root", ">=2.0.0"),
        requirement("dev.vesper.service.plan-dependency", ">=1.0.0"),
    ];
    let reversed_requirements = [requirements[1].clone(), requirements[0].clone()];
    let forward_catalog =
        PluginCatalog::from_records([root.clone(), dependency.clone()]).expect("catalog");
    let reverse_catalog =
        PluginCatalog::from_records([dependency.clone(), root.clone()]).expect("catalog");

    let first = PluginResolver::new(&forward_catalog, policy())
        .resolve_plan(&requirements)
        .expect("plan");
    let second = PluginResolver::new(&reverse_catalog, policy())
        .resolve_plan(&reversed_requirements)
        .expect("plan");

    assert_eq!(first, second);
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(
        first.to_json().expect("canonical JSON"),
        second.to_json().expect("canonical JSON")
    );
    assert_eq!(
        first
            .artifacts()
            .iter()
            .map(|artifact| artifact.descriptor().plugin_id.as_str())
            .collect::<Vec<_>>(),
        vec!["dev.vesper.plan.dependency", "dev.vesper.plan.root"]
    );

    let json = first.to_json().expect("canonical JSON");
    let rebuilt = PluginPlan::from_json(&json).expect("plan round trip");
    assert_eq!(rebuilt, first);
    assert_eq!(rebuilt.to_json().expect("rebuilt JSON"), json);

    let mut prioritized_policy = policy();
    prioritized_policy
        .set_plugin_priority("dev.vesper.plan.root", 1)
        .expect("priority");
    let policy_changed = PluginResolver::new(&forward_catalog, prioritized_policy)
        .resolve_plan(&requirements)
        .expect("policy-changed plan");
    assert_ne!(policy_changed.fingerprint(), first.fingerprint());

    let unrelated = record(
        "dev.vesper.plan.unrelated",
        "1.0.0",
        &[("dev.vesper.service.plan-unrelated", "1.0.0")],
        &[],
    );
    let expanded_catalog =
        PluginCatalog::from_records([dependency, root, unrelated]).expect("expanded catalog");
    let catalog_changed = PluginResolver::new(&expanded_catalog, policy())
        .resolve_plan(&requirements)
        .expect("catalog-changed plan");
    assert_eq!(catalog_changed.artifacts(), first.artifacts());
    assert_ne!(
        catalog_changed.catalog_fingerprint(),
        first.catalog_fingerprint()
    );
    assert_ne!(catalog_changed.fingerprint(), first.fingerprint());

    let mut fingerprint_tampered =
        serde_json::from_slice::<serde_json::Value>(&json).expect("plan JSON");
    fingerprint_tampered["fingerprint"] = serde_json::Value::String("0".repeat(64));
    let fingerprint_tampered_json =
        serde_json::to_vec(&fingerprint_tampered).expect("tampered JSON");
    assert!(matches!(
        PluginPlan::from_json(&fingerprint_tampered_json),
        Err(PluginPlanError::FingerprintMismatch { .. })
    ));

    let mut catalog_fingerprint_tampered =
        serde_json::from_slice::<serde_json::Value>(&json).expect("plan JSON");
    catalog_fingerprint_tampered["catalog_fingerprint"] = serde_json::Value::String("b".repeat(64));
    let catalog_fingerprint_tampered_json =
        serde_json::to_vec(&catalog_fingerprint_tampered).expect("tampered JSON");
    assert!(matches!(
        PluginPlan::from_json(&catalog_fingerprint_tampered_json),
        Err(PluginPlanError::CatalogFingerprintMismatch { .. })
    ));

    let mut noncanonical = serde_json::from_slice::<serde_json::Value>(&json).expect("plan JSON");
    noncanonical["artifacts"]
        .as_array_mut()
        .expect("artifact array")
        .reverse();
    let noncanonical_json = serde_json::to_vec(&noncanonical).expect("noncanonical JSON");
    assert!(matches!(
        PluginPlan::from_json(&noncanonical_json),
        Err(PluginPlanError::NonCanonical { .. })
    ));
}

#[test]
fn plugin_plan_provider_bound_matches_the_resolution_constraint_domain() {
    let dependency_services = (0..64)
        .map(|index| format!("dev.vesper.service.plan-dependency-{index}"))
        .collect::<Vec<_>>();
    let mut records = dependency_services
        .iter()
        .enumerate()
        .map(|(index, service)| {
            record(
                &format!("dev.vesper.plan.dependency-{index}"),
                "1.0.0",
                &[(service.as_str(), "1.0.0")],
                &[],
            )
        })
        .collect::<Vec<_>>();
    let root_requirements = dependency_services
        .iter()
        .map(|service| (service.as_str(), ">=1.0.0"))
        .collect::<Vec<_>>();
    records.push(record(
        "dev.vesper.plan.large-root",
        "1.0.0",
        &[("dev.vesper.service.plan-large-root", "1.0.0")],
        &root_requirements,
    ));
    let catalog = PluginCatalog::from_records(records).expect("catalog");

    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement("dev.vesper.service.plan-large-root", ">=1.0.0")])
        .expect("plan");
    assert_eq!(plan.providers().len(), 65);

    let json = plan.to_json().expect("plan JSON");
    assert_eq!(PluginPlan::from_json(&json).expect("plan round trip"), plan);
}

#[test]
fn plugin_scope_lifecycle_is_hierarchical_cancelable_and_reverse_disposable() {
    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.root",
        "1.0.0",
        &[("dev.vesper.service.scope", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement("dev.vesper.service.scope", ">=1.0.0")])
        .expect("plan");
    let runtime = PluginRuntime::new(plan);
    let root = runtime.root_scope();
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let root_events = events.clone();
    root.add_disposer(move || root_events.lock().expect("events lock").push("root"))
        .expect("root disposer");
    root.start().expect("start root");
    let player = root
        .create_child(PluginScopeKind::Player)
        .expect("player scope");
    player.start().expect("start player");
    let playback = player
        .create_child(PluginScopeKind::Playback)
        .expect("playback scope");
    let playback_events = events.clone();
    playback
        .add_disposer(move || {
            playback_events
                .lock()
                .expect("events lock")
                .push("playback")
        })
        .expect("playback disposer");
    playback.start().expect("start playback");
    let operation = playback
        .create_child(PluginScopeKind::Operation)
        .expect("operation scope");
    let operation_events = events.clone();
    operation
        .add_disposer(move || {
            operation_events
                .lock()
                .expect("events lock")
                .push("operation")
        })
        .expect("operation disposer");
    let operation_second_events = events.clone();
    operation
        .add_disposer(move || {
            operation_second_events
                .lock()
                .expect("events lock")
                .push("operation-second")
        })
        .expect("second operation disposer");
    operation.start().expect("start operation");

    assert_eq!(root.state(), PluginScopeState::Running);
    assert_eq!(root.cancel().expect("cancel root").disposers_run, 4);
    assert_eq!(root.state(), PluginScopeState::Cancelled);
    assert_eq!(player.state(), PluginScopeState::Closed);
    assert_eq!(playback.state(), PluginScopeState::Closed);
    assert_eq!(operation.state(), PluginScopeState::Closed);
    assert_eq!(
        *events.lock().expect("events lock"),
        vec!["operation-second", "operation", "playback", "root"]
    );
    assert!(matches!(
        root.create_child(PluginScopeKind::Worker),
        Err(PluginScopeError::Terminal { .. })
    ));
    assert_eq!(root.cancel().expect("idempotent cancel").disposers_run, 0);
}

#[test]
fn plugin_runtime_scope_tree_has_one_active_and_one_next_prewarm_slot() {
    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.finite-playback",
        "1.0.0",
        &[("dev.vesper.service.scope-finite-playback", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement(
            "dev.vesper.service.scope-finite-playback",
            ">=1.0.0",
        )])
        .expect("plan");
    let runtime = PluginRuntime::new(plan);
    let root = runtime.root_scope();
    root.start().expect("start root");

    root.create_child(PluginScopeKind::Playback)
        .expect("active playback slot");
    assert!(matches!(
        root.create_child(PluginScopeKind::Playback),
        Err(PluginScopeError::CapacityExceeded {
            resource: PluginScopeResource::ActivePlaybackSlot,
            limit: 1,
        })
    ));

    root.create_child(PluginScopeKind::NextPrewarm)
        .expect("next prewarm slot");
    assert!(matches!(
        root.create_child(PluginScopeKind::NextPrewarm),
        Err(PluginScopeError::CapacityExceeded {
            resource: PluginScopeResource::NextPrewarmSlot,
            limit: 1,
        })
    ));
}

#[test]
fn plugin_runtime_fences_correlated_active_and_next_prewarm_attachments() {
    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.correlated-playback",
        "1.0.0",
        &[("dev.vesper.service.scope-correlated-playback", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement(
            "dev.vesper.service.scope-correlated-playback",
            ">=1.0.0",
        )])
        .expect("plan");
    let runtime = PluginRuntime::new(plan);
    let session =
        PluginSessionCorrelation::new(runtime.plan().fingerprint(), "sequence:primary", 7)
            .expect("session correlation");
    let active_correlation = PluginActivePlaybackCorrelation::new(session.clone(), "item-a", 3, 10)
        .expect("active correlation");
    let active = runtime
        .attach_active_playback(active_correlation)
        .expect("attach active playback");

    let wrong_plan = PluginSessionCorrelation::new("b".repeat(64), "sequence:primary", 7)
        .expect("synthetic plan correlation");
    let wrong_plan_prewarm = PluginNextPrewarmCorrelation::new(wrong_plan, "item-b", 4, 101)
        .expect("wrong-plan prewarm correlation");
    assert!(matches!(
        runtime.attach_next_prewarm(wrong_plan_prewarm),
        Err(PluginPlaybackError::PlanFingerprintMismatch)
    ));

    let stale_session =
        PluginSessionCorrelation::new(runtime.plan().fingerprint(), "sequence:primary", 8)
            .expect("stale session correlation");
    let stale_prewarm = PluginNextPrewarmCorrelation::new(stale_session, "item-b", 4, 101)
        .expect("stale prewarm correlation");
    assert!(matches!(
        runtime.attach_next_prewarm(stale_prewarm),
        Err(PluginPlaybackError::SessionGenerationMismatch {
            expected: 7,
            actual: 8,
        })
    ));

    let next_correlation = PluginNextPrewarmCorrelation::new(session.clone(), "item-b", 4, 101)
        .expect("next prewarm correlation");
    let next = runtime
        .attach_next_prewarm(next_correlation)
        .expect("attach next prewarm");
    assert_eq!(active.role(), PluginPlaybackRole::Active);
    assert_eq!(next.role(), PluginPlaybackRole::NextPrewarm);
    for authority in [
        PluginPlaybackAuthority::MasterClock,
        PluginPlaybackAuthority::VideoSurface,
        PluginPlaybackAuthority::AudioSink,
        PluginPlaybackAuthority::Participation,
    ] {
        assert_eq!(
            runtime.authorize_playback_authority(&next, authority),
            Err(PluginPlaybackError::NextPrewarmCannotCommit { authority })
        );
    }

    let stale_source = PluginActivePlaybackCorrelation::new(session.clone(), "item-b", 5, 11)
        .expect("stale-source active correlation");
    assert!(matches!(
        runtime.promote_next_prewarm(&next, stale_source, std::time::Duration::from_secs(1),),
        Err(PluginPlaybackError::SourceRevisionMismatch {
            expected: 4,
            actual: 5,
        })
    ));
    assert_eq!(next.scope().state(), PluginScopeState::Running);

    let promoted_correlation = PluginActivePlaybackCorrelation::new(session, "item-b", 4, 11)
        .expect("promoted active correlation");
    let transition = runtime
        .promote_next_prewarm(
            &next,
            promoted_correlation,
            std::time::Duration::from_secs(1),
        )
        .expect("promote next prewarm");
    assert_eq!(active.scope().state(), PluginScopeState::Closed);
    assert_eq!(transition.active.role(), PluginPlaybackRole::Active);
    assert_eq!(transition.active.scope().kind(), PluginScopeKind::Playback);
    assert_eq!(
        runtime.authorize_playback_authority(
            &transition.active,
            PluginPlaybackAuthority::MasterClock,
        ),
        Ok(())
    );
    assert_eq!(
        runtime.authorize_playback_authority(&next, PluginPlaybackAuthority::MasterClock),
        Err(PluginPlaybackError::StaleAttachment {
            role: PluginPlaybackRole::NextPrewarm,
        })
    );
    assert_eq!(
        runtime.authorize_playback_authority(&active, PluginPlaybackAuthority::AudioSink),
        Err(PluginPlaybackError::StaleAttachment {
            role: PluginPlaybackRole::Active,
        })
    );
}

#[test]
fn plugin_runtime_active_replacement_cancels_obsolete_next_prewarm() {
    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.replace-playback",
        "1.0.0",
        &[("dev.vesper.service.scope-replace-playback", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement(
            "dev.vesper.service.scope-replace-playback",
            ">=1.0.0",
        )])
        .expect("plan");
    let runtime = PluginRuntime::new(plan);
    let session =
        PluginSessionCorrelation::new(runtime.plan().fingerprint(), "sequence:replace", 9)
            .expect("session correlation");
    let active = runtime
        .attach_active_playback(
            PluginActivePlaybackCorrelation::new(session.clone(), "item-a", 1, 20)
                .expect("active correlation"),
        )
        .expect("attach active playback");
    let next = runtime
        .attach_next_prewarm(
            PluginNextPrewarmCorrelation::new(session.clone(), "item-b", 1, 201)
                .expect("next correlation"),
        )
        .expect("attach next prewarm");

    let transition = runtime
        .replace_active_playback(
            PluginActivePlaybackCorrelation::new(session.clone(), "item-c", 2, 21)
                .expect("replacement correlation"),
            std::time::Duration::from_secs(1),
        )
        .expect("replace active playback");

    assert_eq!(active.scope().state(), PluginScopeState::Closed);
    assert_eq!(next.scope().state(), PluginScopeState::Cancelled);
    assert_eq!(
        transition
            .discarded_next_prewarm
            .as_ref()
            .expect("discarded prewarm report")
            .final_state,
        PluginScopeState::Cancelled
    );
    assert_eq!(transition.active.item_id(), "item-c");
    assert_eq!(
        runtime.authorize_playback_authority(
            &transition.active,
            PluginPlaybackAuthority::Participation,
        ),
        Ok(())
    );
    assert!(matches!(
        runtime.promote_next_prewarm(
            &next,
            PluginActivePlaybackCorrelation::new(session.clone(), "item-b", 1, 22)
                .expect("stale promotion correlation"),
            std::time::Duration::from_secs(1),
        ),
        Err(PluginPlaybackError::StaleAttachment {
            role: PluginPlaybackRole::NextPrewarm,
        })
    ));

    runtime
        .attach_next_prewarm(
            PluginNextPrewarmCorrelation::new(session, "item-d", 1, 202)
                .expect("replacement next correlation"),
        )
        .expect("finite next slot is reusable after cancellation");
}

#[test]
fn plugin_scope_constructor_failure_settles_children_and_disposers_once() {
    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.failure",
        "1.0.0",
        &[("dev.vesper.service.scope-failure", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement("dev.vesper.service.scope-failure", ">=1.0.0")])
        .expect("plan");
    let runtime = PluginRuntime::new(plan);
    let root = runtime.root_scope();
    let child = root
        .create_child(PluginScopeKind::Worker)
        .expect("worker scope");
    let runs = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let disposer_runs = runs.clone();
    child
        .add_disposer(move || {
            disposer_runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })
        .expect("worker disposer");
    let report = child.fail("factory rejected").expect("failure settlement");
    assert_eq!(child.state(), PluginScopeState::Failed);
    assert_eq!(report.disposers_run, 1);
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(child.close().expect("failed scope close").disposers_run, 0);
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn plugin_scope_parent_close_can_retry_after_concurrent_child_close() {
    use std::sync::{Arc, Barrier};
    use std::thread;

    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.concurrent",
        "1.0.0",
        &[("dev.vesper.service.scope-concurrent", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement(
            "dev.vesper.service.scope-concurrent",
            ">=1.0.0",
        )])
        .expect("plan");
    let runtime = PluginRuntime::new(plan);
    let root = runtime.root_scope();
    root.start().expect("start root");
    let child = root
        .create_child(PluginScopeKind::Worker)
        .expect("worker scope");
    child.start().expect("start worker");

    let disposer_started = Arc::new(Barrier::new(2));
    let release_disposer = Arc::new(Barrier::new(2));
    let started_for_disposer = disposer_started.clone();
    let release_for_disposer = release_disposer.clone();
    child
        .add_disposer(move || {
            started_for_disposer.wait();
            release_for_disposer.wait();
        })
        .expect("worker disposer");

    let closing_child = child.clone();
    let child_close =
        thread::spawn(move || closing_child.close_with_timeout(std::time::Duration::from_secs(2)));
    disposer_started.wait();

    assert_eq!(child.add_disposer(|| {}), Err(PluginScopeError::Busy));
    assert!(matches!(
        child.create_child(PluginScopeKind::Operation),
        Err(PluginScopeError::Busy)
    ));
    assert_eq!(root.close(), Err(PluginScopeError::Busy));
    assert_eq!(root.state(), PluginScopeState::Running);

    release_disposer.wait();
    child_close
        .join()
        .expect("child close thread")
        .expect("child close");
    assert_eq!(
        root.close().expect("retry root close").final_state,
        PluginScopeState::Closed
    );
    assert_eq!(root.state(), PluginScopeState::Closed);
}

#[test]
fn plugin_scope_owner_tokens_are_unique_and_capacity_bounded() {
    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.owner-capacity",
        "1.0.0",
        &[("dev.vesper.service.scope-owner-capacity", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement(
            "dev.vesper.service.scope-owner-capacity",
            ">=1.0.0",
        )])
        .expect("plan");
    let runtime = PluginRuntime::new(plan);
    let root = runtime.root_scope();
    let mut tokens = std::collections::BTreeSet::new();

    for _ in 0..MAX_PLUGIN_SCOPE_OWNERS {
        let token = root
            .add_owner_disposer(|| {})
            .expect("owner disposer token");
        assert!(tokens.insert(token));
    }
    assert!(matches!(
        root.add_owner_disposer(|| {}),
        Err(PluginScopeError::CapacityExceeded {
            resource: PluginScopeResource::Owners,
            limit: MAX_PLUGIN_SCOPE_OWNERS,
        })
    ));

    let report = runtime
        .shutdown(std::time::Duration::from_secs(2))
        .expect("runtime shutdown");
    assert_eq!(report.owners_settled, MAX_PLUGIN_SCOPE_OWNERS);
    assert_eq!(report.owners_quarantined, 0);
}

#[test]
fn plugin_scope_child_and_depth_capacity_failures_are_typed() {
    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.tree-capacity",
        "1.0.0",
        &[("dev.vesper.service.scope-tree-capacity", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement(
            "dev.vesper.service.scope-tree-capacity",
            ">=1.0.0",
        )])
        .expect("plan");
    let runtime = PluginRuntime::new(plan.clone());
    let root = runtime.root_scope();

    let mut first_child = None;
    for index in 0..MAX_PLUGIN_SCOPE_CHILDREN {
        let child = root
            .create_child(PluginScopeKind::Operation)
            .expect("bounded child");
        if index == 0 {
            first_child = Some(child);
        }
    }
    assert!(matches!(
        root.create_child(PluginScopeKind::Operation),
        Err(PluginScopeError::CapacityExceeded {
            resource: PluginScopeResource::Children,
            limit: MAX_PLUGIN_SCOPE_CHILDREN,
        })
    ));
    first_child
        .expect("first child")
        .close()
        .expect("close first child");
    root.create_child(PluginScopeKind::Operation)
        .expect("terminal child capacity is reusable");
    runtime
        .shutdown(std::time::Duration::from_secs(2))
        .expect("child-capacity runtime shutdown");

    let depth_runtime = PluginRuntime::new(plan);
    let mut scope = depth_runtime.root_scope();
    for _ in 0..MAX_PLUGIN_SCOPE_DEPTH {
        scope = scope
            .create_child(PluginScopeKind::Operation)
            .expect("bounded depth");
    }
    assert!(matches!(
        scope.create_child(PluginScopeKind::Operation),
        Err(PluginScopeError::CapacityExceeded {
            resource: PluginScopeResource::Depth,
            limit: MAX_PLUGIN_SCOPE_DEPTH,
        })
    ));
    depth_runtime
        .shutdown(std::time::Duration::from_secs(2))
        .expect("depth runtime shutdown");
}

#[test]
fn plugin_scope_runtime_registration_limits_are_lifetime_bounded() {
    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.runtime-capacity",
        "1.0.0",
        &[("dev.vesper.service.scope-runtime-capacity", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement(
            "dev.vesper.service.scope-runtime-capacity",
            ">=1.0.0",
        )])
        .expect("plan");
    let scope_runtime = PluginRuntime::new(plan.clone());
    let root = scope_runtime.root_scope();

    for _ in 1..MAX_PLUGIN_RUNTIME_SCOPE_REGISTRATIONS {
        root.create_child(PluginScopeKind::Operation)
            .expect("runtime scope registration")
            .close()
            .expect("terminal child settlement");
    }
    assert!(matches!(
        root.create_child(PluginScopeKind::Operation),
        Err(PluginScopeError::CapacityExceeded {
            resource: PluginScopeResource::ScopeRegistrations,
            limit: MAX_PLUGIN_RUNTIME_SCOPE_REGISTRATIONS,
        })
    ));

    let owner_runtime = PluginRuntime::new(plan);
    let owner_root = owner_runtime.root_scope();
    for _ in 0..(MAX_PLUGIN_RUNTIME_OWNER_REGISTRATIONS / MAX_PLUGIN_SCOPE_OWNERS) {
        let scope = owner_root
            .create_child(PluginScopeKind::Operation)
            .expect("owner registration scope");
        for _ in 0..MAX_PLUGIN_SCOPE_OWNERS {
            scope
                .add_owner_disposer(|| {})
                .expect("runtime owner registration");
        }
        scope
            .close_with_timeout(std::time::Duration::ZERO)
            .expect("zero-budget owner quarantine");
    }
    let final_scope = owner_root
        .create_child(PluginScopeKind::Operation)
        .expect("final owner registration scope");
    assert!(matches!(
        final_scope.add_owner_disposer(|| {}),
        Err(PluginScopeError::CapacityExceeded {
            resource: PluginScopeResource::OwnerRegistrations,
            limit: MAX_PLUGIN_RUNTIME_OWNER_REGISTRATIONS,
        })
    ));
}

#[test]
fn plugin_scope_deadline_quarantines_panic_and_timeout_without_blocking_sibling() {
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.quarantine",
        "1.0.0",
        &[("dev.vesper.service.scope-quarantine", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement(
            "dev.vesper.service.scope-quarantine",
            ">=1.0.0",
        )])
        .expect("plan");
    let runtime = PluginRuntime::new(plan);
    let root = runtime.root_scope();
    root.start().expect("start root");
    let events = Arc::new(Mutex::new(Vec::new()));

    let sibling = root
        .create_child(PluginScopeKind::Worker)
        .expect("sibling scope");
    let sibling_events = events.clone();
    sibling
        .add_owner_disposer(move || sibling_events.lock().expect("events lock").push("sibling"))
        .expect("sibling owner");

    let slow_child = root
        .create_child(PluginScopeKind::Worker)
        .expect("slow scope");
    let (release_slow, wait_for_release) = mpsc::channel();
    let slow_token = slow_child
        .add_owner_disposer(move || {
            let _ = wait_for_release.recv_timeout(Duration::from_secs(2));
        })
        .expect("slow owner");
    let panic_child = root
        .create_child(PluginScopeKind::Worker)
        .expect("panic scope");
    let panic_token = panic_child
        .add_owner_disposer(|| panic!("synthetic disposer panic"))
        .expect("panic owner");
    let root_events = events.clone();
    root.add_owner_disposer(move || root_events.lock().expect("events lock").push("root"))
        .expect("root owner");

    let started_at = Instant::now();
    let report = runtime
        .shutdown(Duration::from_millis(50))
        .expect("bounded runtime shutdown");
    assert!(started_at.elapsed() < Duration::from_millis(500));
    assert_eq!(report.final_state, PluginScopeState::Quarantined);
    assert_eq!(report.owners_quarantined, 2);
    assert_eq!(report.disposer_panics, 1);
    assert_eq!(report.disposer_timeouts, 1);
    assert!(report.quarantined_owners.iter().any(|entry| {
        entry.owner_token == panic_token && entry.reason == PluginScopeQuarantineReason::Panicked
    }));
    assert!(report.quarantined_owners.iter().any(|entry| {
        entry.owner_token == slow_token && entry.reason == PluginScopeQuarantineReason::TimedOut
    }));
    assert_eq!(
        *events.lock().expect("events lock"),
        vec!["sibling", "root"]
    );
    assert_eq!(root.state(), PluginScopeState::Quarantined);
    release_slow.send(()).expect("release slow disposer");
}

#[test]
fn plugin_scope_expired_deadline_retains_the_quarantined_owner() {
    struct DropProbe(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.quarantine-retain",
        "1.0.0",
        &[("dev.vesper.service.scope-quarantine-retain", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement(
            "dev.vesper.service.scope-quarantine-retain",
            ">=1.0.0",
        )])
        .expect("plan");
    let runtime = PluginRuntime::new(plan);
    let root = runtime.root_scope();
    let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let probe = DropProbe(drops.clone());
    let token = root
        .add_owner_disposer(move || drop(probe))
        .expect("owner token");

    let report = runtime
        .shutdown(std::time::Duration::ZERO)
        .expect("zero-budget shutdown");

    assert_eq!(report.final_state, PluginScopeState::Quarantined);
    assert_eq!(report.disposers_run, 0);
    assert_eq!(report.disposer_timeouts, 1);
    assert!(report.quarantined_owners.iter().any(|entry| {
        entry.owner_token == token && entry.reason == PluginScopeQuarantineReason::TimedOut
    }));
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn plugin_runtime_shutdown_quarantines_an_in_progress_root_close() {
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant};

    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.concurrent-shutdown",
        "1.0.0",
        &[("dev.vesper.service.scope-concurrent-shutdown", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement(
            "dev.vesper.service.scope-concurrent-shutdown",
            ">=1.0.0",
        )])
        .expect("plan");
    let runtime = PluginRuntime::new(plan);
    let root = runtime.root_scope();
    let disposer_started = Arc::new(Barrier::new(2));
    let release_disposer = Arc::new(Barrier::new(2));
    let started_for_disposer = disposer_started.clone();
    let release_for_disposer = release_disposer.clone();
    root.add_owner_disposer(move || {
        started_for_disposer.wait();
        release_for_disposer.wait();
    })
    .expect("root owner");

    let closing_root = root.clone();
    let root_close =
        std::thread::spawn(move || closing_root.close_with_timeout(Duration::from_secs(2)));
    disposer_started.wait();

    let started_at = Instant::now();
    let shutdown_report = runtime
        .shutdown(Duration::from_millis(50))
        .expect("runtime shutdown");
    assert!(started_at.elapsed() < Duration::from_millis(500));
    assert_eq!(shutdown_report.final_state, PluginScopeState::Quarantined);
    assert_eq!(shutdown_report.busy_scopes_quarantined, 1);
    assert_eq!(root.state(), PluginScopeState::Quarantined);

    release_disposer.wait();
    assert_eq!(
        root_close
            .join()
            .expect("root close thread")
            .expect("root close")
            .final_state,
        PluginScopeState::Quarantined
    );
    let final_report = root
        .last_close_report()
        .expect("combined concurrent close report");
    assert_eq!(final_report.busy_scopes_quarantined, 1);
    assert_eq!(final_report.owners_settled, 1);
}

#[test]
fn plugin_runtime_drop_settles_the_root_scope() {
    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.runtime-drop",
        "1.0.0",
        &[("dev.vesper.service.scope-runtime-drop", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement(
            "dev.vesper.service.scope-runtime-drop",
            ">=1.0.0",
        )])
        .expect("plan");
    let runtime = PluginRuntime::new(plan);
    let root = runtime.root_scope();
    let runs = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let disposer_runs = runs.clone();
    root.add_owner_disposer(move || {
        disposer_runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    })
    .expect("root owner");

    drop(runtime);

    assert_eq!(root.state(), PluginScopeState::Closed);
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        root.last_close_report()
            .expect("runtime drop settlement report")
            .owners_settled,
        1
    );
}

#[test]
fn plugin_runtime_rejects_an_attachment_from_an_identical_peer_runtime() {
    let catalog = PluginCatalog::from_records([record(
        "dev.vesper.scope.runtime-local-attachment",
        "1.0.0",
        &[("dev.vesper.service.scope-runtime-local-attachment", "1.0.0")],
        &[],
    )])
    .expect("catalog");
    let plan = PluginResolver::new(&catalog, policy())
        .resolve_plan(&[requirement(
            "dev.vesper.service.scope-runtime-local-attachment",
            ">=1.0.0",
        )])
        .expect("plan");
    let first = PluginRuntime::new(plan.clone());
    let second = PluginRuntime::new(plan);
    let session =
        PluginSessionCorrelation::new(first.plan().fingerprint(), "sequence:runtime-local", 1)
            .expect("session correlation");
    let correlation =
        PluginActivePlaybackCorrelation::new(session, "item-a", 1, 1).expect("active correlation");
    let first_attachment = first
        .attach_active_playback(correlation.clone())
        .expect("first runtime attachment");
    let second_attachment = second
        .attach_active_playback(correlation)
        .expect("second runtime attachment");
    assert_eq!(first_attachment.token(), second_attachment.token());
    assert_eq!(
        second.authorize_playback_authority(
            &first_attachment,
            PluginPlaybackAuthority::Participation,
        ),
        Err(PluginPlaybackError::StaleAttachment {
            role: PluginPlaybackRole::Active,
        })
    );
}
