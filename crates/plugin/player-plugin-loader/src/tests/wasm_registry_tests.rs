use std::collections::BTreeMap;
#[cfg(feature = "installed-catalog")]
use std::fs;
use std::io::Write;
use std::path::Path;

use player_plugin::{
    BenchmarkEvent, BenchmarkEventBatch, PipelineEvent, PluginArtifactTransport, PluginCatalog,
    PluginReference, PluginResolver, PluginResolverPolicy, PluginRuntime, PluginScopeKind,
    PluginScopeState, PluginTransport,
};
use player_plugin_abi::{BENCHMARK_SINK_INTERFACE_ID, PIPELINE_EVENT_HOOK_INTERFACE_ID};
use player_plugin_wasm_host::{
    VESPER_PLUGIN_WIT, WASM_PLUGIN_WIT_INTERFACE_MAJOR, WASM_PLUGIN_WIT_INTERFACE_MINOR,
    WasmPluginHostError,
};
use tempfile::NamedTempFile;

#[cfg(feature = "installed-catalog")]
use player_plugin_package::{
    PluginHostTarget, PluginProjectManifest, PluginSigningKey, PluginTrustStore,
    build_signed_plugin_package, install_verified_plugin_package, verify_installed_plugin_catalog,
    verify_signed_plugin_package,
};

use crate::{
    PluginRegistry, PluginRegistryBuildError, PluginSelectionError, WasmPluginArtifact,
    WasmPluginArtifactError, WasmPluginInterfaceDeclaration, WasmPluginLoadError,
};

const COMBINED_CORE_WAT: &str = r#"
(module
  (import "vesper:plugin/host" "log"
    (func $log (param i32 i32 i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (global $accepted (mut i64) (i64.const 0))

  (func $realloc
    (param $old-ptr i32) (param $old-size i32) (param $align i32) (param $new-size i32)
    (result i32)
    (local $result i32)
    local.get $new-size
    i32.eqz
    if
      i32.const 0
      return
    end
    global.get $heap
    local.get $align
    i32.const 1
    i32.sub
    i32.add
    i32.const 0
    local.get $align
    i32.sub
    i32.and
    local.tee $result
    local.get $new-size
    i32.add
    global.set $heap
    local.get $result)
  (export "cabi_realloc" (func $realloc))

  (func $on-event (param i32) (result i32)
    i32.const 0
    i32.const 0
    i32.store8
    i32.const 4
    i32.const 1
    i32.store8
    i32.const 8
    i32.const 0
    i32.store
    i32.const 12
    i32.const 0
    i32.store
    i32.const 16
    i32.const 0
    i32.store
    i32.const 20
    i32.const 0
    i32.store
    i32.const 0)
  (export "vesper:plugin/event-hook#on-event" (func $on-event))
  (func $post-on-event (param i32))
  (export "cabi_post_vesper:plugin/event-hook#on-event" (func $post-on-event))

  (func $on-event-batch (param i32) (param $events-len i32) (result i32)
    global.get $accepted
    local.get $events-len
    i64.extend_i32_u
    i64.add
    global.set $accepted
    i32.const 32
    i32.const 0
    i32.store8
    i32.const 40
    local.get $events-len
    i64.extend_i32_u
    i64.store
    i32.const 32)
  (export "vesper:plugin/benchmark-sink#on-event-batch" (func $on-event-batch))
  (func $post-on-event-batch (param i32))
  (export "cabi_post_vesper:plugin/benchmark-sink#on-event-batch"
    (func $post-on-event-batch))

  (func $flush (result i32)
    i32.const 64
    i32.const 0
    i32.store8
    i32.const 72
    global.get $accepted
    i64.store
    i32.const 80
    i64.const 0
    i64.store
    i32.const 88
    i64.const 0
    i64.store
    i32.const 96
    i64.const 0
    i64.store
    i32.const 104
    i64.const 0
    i64.store
    i32.const 64)
  (export "vesper:plugin/benchmark-sink#flush" (func $flush))
  (func $post-flush (param i32))
  (export "cabi_post_vesper:plugin/benchmark-sink#flush" (func $post-flush)))
"#;

const EVENT_ONLY_CORE_WAT: &str = r#"
(module
  (import "vesper:plugin/host" "log"
    (func $log (param i32 i32 i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (func $realloc
    (param i32) (param i32) (param $align i32) (param $new-size i32)
    (result i32)
    (local $result i32)
    local.get $new-size
    i32.eqz
    if
      i32.const 0
      return
    end
    global.get $heap
    local.get $align
    i32.const 1
    i32.sub
    i32.add
    i32.const 0
    local.get $align
    i32.sub
    i32.and
    local.tee $result
    local.get $new-size
    i32.add
    global.set $heap
    local.get $result)
  (export "cabi_realloc" (func $realloc))
  (func $on-event (param i32) (result i32)
    i32.const 0
    i32.const 0
    i32.store8
    i32.const 4
    i32.const 1
    i32.store8
    i32.const 8
    i32.const 0
    i32.store
    i32.const 12
    i32.const 0
    i32.store
    i32.const 16
    i32.const 0
    i32.store
    i32.const 20
    i32.const 0
    i32.store
    i32.const 0)
  (export "vesper:plugin/event-hook#on-event" (func $on-event))
  (func $post-on-event (param i32))
  (export "cabi_post_vesper:plugin/event-hook#on-event" (func $post-on-event)))
"#;

#[test]
fn rewrite_w4_wasm_observer_and_offline_resolution_require_a_scope() {
    let component = write_component(component("event-and-benchmark-plugin", COMBINED_CORE_WAT));
    let registry = PluginRegistry::load_wasm_artifacts([artifact(
        "dev.vesper.scope-required",
        component.path(),
        [
            declaration(
                PIPELINE_EVENT_HOOK_INTERFACE_ID.0,
                "dev.vesper.scope-required.event",
            ),
            declaration(
                BENCHMARK_SINK_INTERFACE_ID.0,
                "dev.vesper.scope-required.benchmark",
            ),
        ],
    )])
    .expect("WASM registry");
    let reference = reference("dev.vesper.scope-required", None, PluginTransport::Wasm);

    let hook_error = registry
        .resolve_pipeline_event_hook(&reference)
        .expect_err("WASM observer resolution must require a lifecycle scope");
    assert!(hook_error.to_string().contains("scope"));

    let benchmark_error = registry
        .resolve_benchmark_sink(&reference)
        .expect_err("WASM offline resolution must require a lifecycle scope");
    assert!(benchmark_error.to_string().contains("scope"));
}

#[test]
fn wasm_registry_resolves_and_executes_both_typed_capabilities() {
    let component = write_component(component("event-and-benchmark-plugin", COMBINED_CORE_WAT));
    let artifact = artifact(
        "dev.vesper.wasm-fixture",
        component.path(),
        [
            declaration(
                PIPELINE_EVENT_HOOK_INTERFACE_ID.0,
                "dev.vesper.wasm-fixture.event",
            ),
            declaration(
                BENCHMARK_SINK_INTERFACE_ID.0,
                "dev.vesper.wasm-fixture.benchmark",
            ),
        ],
    );
    let registry = PluginRegistry::load_wasm_artifacts([artifact]).expect("WASM registry");
    let lifecycle = empty_plugin_runtime();
    let scope = lifecycle.root_scope();

    assert_eq!(registry.registered_interfaces().len(), 2);
    assert!(
        registry
            .registered_interfaces()
            .iter()
            .all(|interface| interface.transport == PluginTransport::Wasm)
    );
    assert_eq!(registry.pipeline_event_hook_references().unwrap().len(), 1);
    assert_eq!(registry.benchmark_sink_references().unwrap().len(), 1);

    let implicit = reference("dev.vesper.wasm-fixture", None, PluginTransport::Wasm);
    let hook = registry
        .resolve_pipeline_event_hook_in_scope(&implicit, &scope)
        .expect("unique WASM EventHook");
    assert_eq!(
        hook.reference().capability_instance_id(),
        Some("dev.vesper.wasm-fixture.event")
    );
    assert!(
        hook.capability()
            .on_event(&pipeline_event())
            .expect("WASM EventHook outcome")
            .accepted
    );

    let benchmark = registry
        .resolve_benchmark_sink_in_scope(
            &reference(
                "dev.vesper.wasm-fixture",
                Some("dev.vesper.wasm-fixture.benchmark"),
                PluginTransport::Wasm,
            ),
            &scope,
        )
        .expect("explicit WASM BenchmarkSink");
    assert_eq!(
        benchmark
            .capability()
            .on_event_batch(&benchmark_batch())
            .expect("WASM benchmark batch")
            .accepted_events,
        1
    );
    assert_eq!(
        benchmark
            .capability()
            .flush()
            .expect("WASM benchmark report")
            .accepted_events,
        1
    );
    let close = lifecycle
        .shutdown(std::time::Duration::from_secs(1))
        .expect("WASM lifecycle close");
    assert_eq!(close.owners_settled, 2);
}

#[test]
fn wasm_registry_requires_an_instance_when_multiple_implementations_match() {
    let component = write_component(component("event-hook-plugin", EVENT_ONLY_CORE_WAT));
    let artifact = artifact(
        "dev.vesper.multi-hook",
        component.path(),
        [
            declaration(
                PIPELINE_EVENT_HOOK_INTERFACE_ID.0,
                "dev.vesper.multi-hook.primary",
            ),
            declaration(
                PIPELINE_EVENT_HOOK_INTERFACE_ID.0,
                "dev.vesper.multi-hook.secondary",
            ),
        ],
    );
    let registry = PluginRegistry::load_wasm_artifacts([artifact]).expect("WASM registry");
    let lifecycle = empty_plugin_runtime();
    let scope = lifecycle.root_scope();

    let implicit = reference("dev.vesper.multi-hook", None, PluginTransport::Wasm);
    assert_eq!(
        registry.resolve_pipeline_event_hook(&implicit).unwrap_err(),
        PluginSelectionError::Ambiguous {
            plugin_id: "dev.vesper.multi-hook".to_owned(),
            interface: "PipelineEventHook",
            count: 2,
        }
    );
    let explicit = reference(
        "dev.vesper.multi-hook",
        Some("dev.vesper.multi-hook.secondary"),
        PluginTransport::Wasm,
    );
    assert!(
        registry
            .resolve_pipeline_event_hook_in_scope(&explicit, &scope)
            .expect("explicit secondary hook")
            .capability()
            .on_event(&pipeline_event())
            .expect("secondary hook outcome")
            .accepted
    );
    lifecycle
        .shutdown(std::time::Duration::from_secs(1))
        .expect("WASM lifecycle close");
}

#[test]
fn rewrite_w4_scoped_wasm_workers_close_once_and_isolate_siblings() {
    let component = write_component(component("event-and-benchmark-plugin", COMBINED_CORE_WAT));
    let registry = PluginRegistry::load_wasm_artifacts([artifact(
        "dev.vesper.scoped-workers",
        component.path(),
        [
            declaration(
                PIPELINE_EVENT_HOOK_INTERFACE_ID.0,
                "dev.vesper.scoped-workers.event",
            ),
            declaration(
                BENCHMARK_SINK_INTERFACE_ID.0,
                "dev.vesper.scoped-workers.benchmark",
            ),
        ],
    )])
    .expect("WASM registry");
    let lifecycle = empty_plugin_runtime();
    let root = lifecycle.root_scope();
    let hook_scope = root
        .create_child(PluginScopeKind::Operation)
        .expect("hook scope");
    let benchmark_scope = root
        .create_child(PluginScopeKind::Operation)
        .expect("benchmark scope");
    let hook = registry
        .resolve_pipeline_event_hook_in_scope(
            &reference(
                "dev.vesper.scoped-workers",
                Some("dev.vesper.scoped-workers.event"),
                PluginTransport::Wasm,
            ),
            &hook_scope,
        )
        .expect("scoped EventHook");
    let benchmark = registry
        .resolve_benchmark_sink_in_scope(
            &reference(
                "dev.vesper.scoped-workers",
                Some("dev.vesper.scoped-workers.benchmark"),
                PluginTransport::Wasm,
            ),
            &benchmark_scope,
        )
        .expect("scoped BenchmarkSink");

    assert!(
        hook.capability()
            .on_event(&pipeline_event())
            .expect("hook before close")
            .accepted
    );
    assert_eq!(
        benchmark
            .capability()
            .on_event_batch(&benchmark_batch())
            .expect("benchmark before sibling close")
            .accepted_events,
        1
    );

    let hook_close = hook_scope
        .close_with_timeout(std::time::Duration::from_secs(1))
        .expect("hook scope close");
    assert_eq!(hook_close.final_state, PluginScopeState::Closed);
    assert_eq!(hook_close.owners_settled, 1);
    assert!(hook.capability().on_event(&pipeline_event()).is_err());
    assert_eq!(
        benchmark
            .capability()
            .on_event_batch(&benchmark_batch())
            .expect("benchmark sibling remains live")
            .accepted_events,
        1
    );

    let benchmark_close = benchmark_scope
        .close_with_timeout(std::time::Duration::from_secs(1))
        .expect("benchmark scope close");
    assert_eq!(benchmark_close.final_state, PluginScopeState::Closed);
    assert_eq!(benchmark_close.owners_settled, 1);
    assert!(
        benchmark
            .capability()
            .on_event_batch(&benchmark_batch())
            .is_err()
    );
    lifecycle
        .shutdown(std::time::Duration::from_secs(1))
        .expect("root scope close");
}

#[test]
fn wasm_registry_preserves_identity_and_transport_failures_without_fallback() {
    let component = write_component(component("event-hook-plugin", EVENT_ONLY_CORE_WAT));
    let registry = PluginRegistry::load_wasm_artifacts([artifact(
        "dev.vesper.identity",
        component.path(),
        [declaration(
            PIPELINE_EVENT_HOOK_INTERFACE_ID.0,
            "dev.vesper.identity.event",
        )],
    )])
    .expect("WASM registry");

    let missing_instance = reference(
        "dev.vesper.identity",
        Some("dev.vesper.identity.missing"),
        PluginTransport::Wasm,
    );
    assert!(matches!(
        registry.resolve_pipeline_event_hook(&missing_instance),
        Err(PluginSelectionError::InstanceNotFound { ref instance_id, .. })
            if instance_id == "dev.vesper.identity.missing"
    ));

    let wrong_plugin = reference("dev.vesper.other", None, PluginTransport::Wasm);
    assert_eq!(
        registry
            .resolve_pipeline_event_hook(&wrong_plugin)
            .unwrap_err(),
        PluginSelectionError::PluginNotFound {
            plugin_id: "dev.vesper.other".to_owned(),
            transport: PluginTransport::Wasm,
        }
    );

    let native = reference("dev.vesper.identity", None, PluginTransport::Native);
    assert_eq!(
        registry.resolve_pipeline_event_hook(&native).unwrap_err(),
        PluginSelectionError::PluginNotFound {
            plugin_id: "dev.vesper.identity".to_owned(),
            transport: PluginTransport::Native,
        }
    );
}

#[cfg(feature = "installed-catalog")]
#[test]
fn signed_installed_wasm_catalog_loads_and_executes_through_the_registry() {
    let directory = tempfile::tempdir().expect("temporary installed WASM catalog");
    let component_bytes = component("event-hook-plugin", EVENT_ONLY_CORE_WAT);
    fs::write(directory.path().join("event-hook.wasm"), component_bytes)
        .expect("write component artifact");
    fs::write(directory.path().join("LICENSE"), b"Apache-2.0\n").expect("write license");
    fs::write(directory.path().join("NOTICE"), b"Fixture notice\n").expect("write notice");
    let project = PluginProjectManifest::from_toml(
        r#"
schema_version = 1

[plugin]
id = "dev.vesper.installed-event"
name = "Installed Event Hook"
version = "1.0.0"
description = "Signed installed WASM EventHook fixture."
license = "Apache-2.0"
publisher = "dev.vesper.publisher"

[compatibility]
host_sdk = ">=0.4.0, <0.5.0"
abi_major = 1
abi_minor_min = 0
abi_minor_max = 0

[[capabilities]]
interface_id = "c7a69475-79b2-5b5e-a477-08844a5da5d1"
instance_id = "dev.vesper.installed-event.event"
interface_major = 1
interface_minor = 0
stability = "stable"

[[artifacts]]
transport = "wasm"
target = "wasm32-wasip2"
format = "wasm-component"
source = "event-hook.wasm"
path = "artifacts/wasm32-wasip2/event-hook.wasm"
architecture = "wasm32"
capabilities = [{ interface_id = "c7a69475-79b2-5b5e-a477-08844a5da5d1", instance_id = "dev.vesper.installed-event.event" }]

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
    .expect("plugin project");
    let key = PluginSigningKey::generate("dev.vesper.publisher").expect("signing key");
    let package_path = directory.path().join("event-hook.vesper-plugin");
    build_signed_plugin_package(&project, directory.path(), &key, &package_path)
        .expect("signed plugin package");
    let mut trust = PluginTrustStore::empty();
    trust
        .insert(key.public_key())
        .expect("trusted publisher key");
    let package =
        verify_signed_plugin_package(&package_path, &trust).expect("verified plugin package");
    let install_root = directory.path().join("installed");
    let installation =
        install_verified_plugin_package(&package, &install_root).expect("installed plugin package");
    let reference = reference(
        "dev.vesper.installed-event",
        Some("dev.vesper.installed-event.event"),
        PluginTransport::Wasm,
    );
    let host = PluginHostTarget::new(semver::Version::new(0, 4, 0), "wasm32-wasip2", "wasm32")
        .expect("WASM host target");
    let catalog = verify_installed_plugin_catalog(
        &install_root,
        &trust,
        &host,
        std::slice::from_ref(&reference),
        &[],
    )
    .expect("verified installed catalog");
    let registry = PluginRegistry::load_verified_installed_catalog(&catalog)
        .expect("registry from installed catalog");
    assert_eq!(
        registry.registered_interfaces()[0].artifact_path,
        installation
            .install_path
            .join("artifacts/wasm32-wasip2/event-hook.wasm")
    );
    drop(catalog);

    let lifecycle = empty_plugin_runtime();
    let hook = registry
        .resolve_pipeline_event_hook_in_scope(&reference, &lifecycle.root_scope())
        .expect("resolved installed EventHook");
    assert!(
        hook.capability()
            .on_event(&pipeline_event())
            .expect("execute installed EventHook")
            .accepted
    );
    lifecycle
        .shutdown(std::time::Duration::from_secs(1))
        .expect("installed WASM lifecycle close");
}

#[test]
fn wasm_artifact_rejects_unsupported_or_mismatched_interface_declarations() {
    let unsupported = WasmPluginArtifact::new(
        "dev.vesper.invalid",
        "/internal/invalid.wasm",
        [WasmPluginInterfaceDeclaration::new(
            [0; 16],
            WASM_PLUGIN_WIT_INTERFACE_MAJOR,
            WASM_PLUGIN_WIT_INTERFACE_MINOR,
            "dev.vesper.invalid.capability",
        )],
    )
    .unwrap_err();
    assert!(matches!(
        unsupported,
        WasmPluginArtifactError::UnsupportedInterface { interface_id } if interface_id == [0; 16]
    ));

    let mismatched = WasmPluginArtifact::new(
        "dev.vesper.invalid",
        "/internal/invalid.wasm",
        [WasmPluginInterfaceDeclaration::new(
            PIPELINE_EVENT_HOOK_INTERFACE_ID.0,
            WASM_PLUGIN_WIT_INTERFACE_MAJOR,
            WASM_PLUGIN_WIT_INTERFACE_MINOR + 1,
            "dev.vesper.invalid.event",
        )],
    )
    .unwrap_err();
    assert!(matches!(
        mismatched,
        WasmPluginArtifactError::UnsupportedInterfaceVersion { minor, .. }
            if minor == WASM_PLUGIN_WIT_INTERFACE_MINOR + 1
    ));
}

#[test]
fn wasm_registry_rejects_a_declared_capability_missing_from_the_component() {
    let component = write_component(component("event-hook-plugin", EVENT_ONLY_CORE_WAT));
    let artifact = artifact(
        "dev.vesper.missing-export",
        component.path(),
        [declaration(
            BENCHMARK_SINK_INTERFACE_ID.0,
            "dev.vesper.missing-export.benchmark",
        )],
    );
    let error = PluginRegistry::load_wasm_artifacts([artifact]).unwrap_err();
    assert!(matches!(
        error,
        PluginRegistryBuildError::WasmLoad {
            source: WasmPluginLoadError::Interface {
                source: WasmPluginHostError::Instantiation(_),
                ..
            },
            ..
        }
    ));
}

#[test]
fn wasm_registry_rejects_malformed_component_bytes() {
    let component = write_component(b"not a component".to_vec());
    let artifact = artifact(
        "dev.vesper.malformed",
        component.path(),
        [declaration(
            PIPELINE_EVENT_HOOK_INTERFACE_ID.0,
            "dev.vesper.malformed.event",
        )],
    );
    let error = PluginRegistry::load_wasm_artifacts([artifact]).unwrap_err();
    assert!(matches!(
        error,
        PluginRegistryBuildError::WasmLoad {
            source: WasmPluginLoadError::Interface {
                source: WasmPluginHostError::Compilation(_),
                ..
            },
            ..
        }
    ));
}

fn declaration(interface_id: [u8; 16], instance_id: &str) -> WasmPluginInterfaceDeclaration {
    WasmPluginInterfaceDeclaration::new(
        interface_id,
        WASM_PLUGIN_WIT_INTERFACE_MAJOR,
        WASM_PLUGIN_WIT_INTERFACE_MINOR,
        instance_id,
    )
}

fn artifact(
    plugin_id: &str,
    path: &Path,
    declarations: impl IntoIterator<Item = WasmPluginInterfaceDeclaration>,
) -> WasmPluginArtifact {
    WasmPluginArtifact::new(plugin_id, path, declarations).expect("valid WASM artifact")
}

fn reference(
    plugin_id: &str,
    instance_id: Option<&str>,
    transport: PluginTransport,
) -> PluginReference {
    PluginReference::new(plugin_id, instance_id.map(str::to_owned), transport)
        .expect("valid plugin reference")
}

fn pipeline_event() -> PipelineEvent {
    PipelineEvent {
        run_id: "run".to_owned(),
        session_id: "session".to_owned(),
        platform: "test".to_owned(),
        protocol: Some("fixture".to_owned()),
        event_name: "fixture.completed".to_owned(),
        timestamp_ns: 1,
        thread: Some("test-thread".to_owned()),
        resource_identity: Some("fixture:1".to_owned()),
        attributes: BTreeMap::new(),
        diagnostic: None,
    }
}

fn benchmark_batch() -> BenchmarkEventBatch {
    BenchmarkEventBatch {
        events: vec![BenchmarkEvent {
            run_id: "run".to_owned(),
            session_id: "session".to_owned(),
            platform: "test".to_owned(),
            source_protocol: Some("fixture".to_owned()),
            event_name: "fixture.completed".to_owned(),
            timestamp_ns: 1,
            elapsed_ns: 1,
            thread: Some("test-thread".to_owned()),
            attributes: BTreeMap::new(),
        }],
    }
}

fn empty_plugin_runtime() -> PluginRuntime {
    let catalog = PluginCatalog::from_records([]).expect("empty plugin catalog");
    let policy = PluginResolverPolicy::new(
        PluginArtifactTransport::Native,
        "aarch64-apple-darwin",
        "arm64",
        1,
        0,
    )
    .expect("empty runtime policy");
    let plan = PluginResolver::new(&catalog, policy)
        .resolve_plan(&[])
        .expect("empty plugin plan");
    PluginRuntime::new(plan)
}

fn component(world_name: &str, core_wat: &str) -> Vec<u8> {
    let mut resolve = wit_parser::Resolve::default();
    let package = resolve
        .push_str("vesper-plugin.wit", VESPER_PLUGIN_WIT)
        .expect("fixture WIT package");
    let world = resolve
        .select_world(&[package], Some(world_name))
        .expect("fixture WIT world");
    let mut module = wat::parse_str(core_wat).expect("fixture core module");
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
    )
    .expect("embedded component metadata");
    wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)
        .expect("fixture component module")
        .encode()
        .expect("encoded fixture component")
}

fn write_component(bytes: Vec<u8>) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temporary component");
    file.write_all(&bytes).expect("write temporary component");
    file.flush().expect("flush temporary component");
    file
}
