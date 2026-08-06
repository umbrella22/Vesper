use std::collections::BTreeMap;
#[cfg(all(
    feature = "installed-catalog",
    target_os = "macos",
    target_arch = "aarch64"
))]
use std::fs;

use player_plugin::{
    BenchmarkEvent, BenchmarkEventBatch, PipelineEvent, PluginReference, PluginTransport,
};
use sha2::{Digest, Sha256};

#[cfg(all(
    feature = "installed-catalog",
    target_os = "macos",
    target_arch = "aarch64"
))]
use player_plugin_package::{
    PluginHostTarget, PluginProjectManifest, PluginSigningKey, PluginTrustStore,
    build_signed_plugin_package, install_verified_plugin_package, verify_installed_plugin_catalog,
    verify_signed_plugin_package,
};

use crate::{
    BenchmarkSinkPluginSession, EmbeddedPluginLocator, EmbeddedPluginRegistry,
    EmbeddedPluginRegistryError, LoadedNativePlugin, NativePluginArtifact, PluginInterfaceState,
    PluginRegistry, PluginRegistryBuildError, PluginSelectionError,
};

#[cfg(all(
    feature = "installed-catalog",
    target_os = "macos",
    target_arch = "aarch64"
))]
use super::workspace_root;
use super::{resolve_plugin_path, resolve_plugin_path_with_override};

fn pipeline_event(event_name: &str, session_id: &str) -> PipelineEvent {
    PipelineEvent {
        run_id: "fixture-run".to_owned(),
        session_id: session_id.to_owned(),
        platform: "test".to_owned(),
        protocol: None,
        event_name: event_name.to_owned(),
        timestamp_ns: 1,
        thread: None,
        resource_identity: Some(format!("download-task:{session_id}")),
        attributes: BTreeMap::new(),
        diagnostic: None,
    }
}

fn assert_native_decoder_plugin(
    environment_variable: &str,
    stem: &str,
    plugin_id: &str,
    instance_id: &str,
    plugin_name: &str,
) {
    let plugin_path = resolve_plugin_path_with_override(environment_variable, stem)
        .unwrap_or_else(|error| panic!("failed to resolve native decoder plugin path: {error}"));
    let plugin = LoadedNativePlugin::load_development(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load native decoder plugin shared library '{}': {error}",
            plugin_path.display()
        )
    });

    assert_eq!(plugin.plugin_id(), plugin_id);
    assert_eq!(plugin.plugin_name(), plugin_name);
    assert!(plugin.diagnostics().is_empty());
    let reference = PluginReference::new(
        plugin_id,
        Some(instance_id.to_owned()),
        PluginTransport::Native,
    )
    .expect("static native decoder reference should be valid");
    let decoder = plugin
        .resolve_native_decoder(&reference)
        .expect("native decoder interface should resolve");
    assert_eq!(decoder.name(), plugin_name);
}

#[test]
fn native_plugin_artifact_preserves_and_validates_the_declared_identity() {
    let artifact = NativePluginArtifact::new(
        "dev.vesper.plugin-fixture",
        "/internal/plugins/libvesper_plugin_fixture.so",
    )
    .expect("valid artifact identity");
    assert_eq!(artifact.plugin_id(), "dev.vesper.plugin-fixture");
    assert_eq!(
        artifact.path(),
        std::path::Path::new("/internal/plugins/libvesper_plugin_fixture.so")
    );

    assert!(
        NativePluginArtifact::new(
            " Dev.Vesper.Plugin-Fixture ",
            "/internal/plugins/libvesper_plugin_fixture.so",
        )
        .is_err()
    );
}

#[test]
#[ignore = "requires a built player-plugin-fixture shared library artifact"]
fn native_dynamic_loader_opens_safe_rust_fixture_and_calls_hook() {
    let plugin_path = resolve_plugin_path("vesper_plugin_fixture")
        .unwrap_or_else(|error| panic!("failed to resolve Plugin fixture path: {error}"));
    let plugin = LoadedNativePlugin::load_development(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load Plugin fixture shared library `{}`: {error}",
            plugin_path.display()
        )
    });

    assert_eq!(plugin.plugin_id(), "dev.vesper.plugin-fixture");
    assert_eq!(plugin.plugin_name(), "Vesper Plugin Fixture");
    assert!(plugin.diagnostics().is_empty());
    let reference =
        PluginReference::new("dev.vesper.plugin-fixture", None, PluginTransport::Native)
            .expect("valid fixture reference");
    let hook = plugin
        .resolve_pipeline_event_hook(&reference)
        .expect("unique event hook");
    let outcome = hook
        .on_event(&pipeline_event("download.task.completed", "fixture-task"))
        .expect("event hook outcome");
    assert!(outcome.accepted);
}

#[cfg(all(
    feature = "installed-catalog",
    target_os = "macos",
    target_arch = "aarch64"
))]
#[test]
#[ignore = "requires the staged official frame-processor-diagnostic dylib"]
fn signed_installed_native_catalog_loads_and_resolves_the_official_frame_processor() {
    let root = workspace_root().expect("workspace root");
    let plugin_directory = root.join("plugins/frame-processor-diagnostic");
    let manifest_path = plugin_directory.join("vesper-plugin.toml");
    let manifest_source = fs::read_to_string(&manifest_path).expect("official plugin manifest");
    let project =
        PluginProjectManifest::from_toml(&manifest_source).expect("valid official plugin manifest");
    let temporary = tempfile::tempdir().expect("temporary native catalog");
    let signing_key = PluginSigningKey::generate("io.github.ikaros").expect("publisher key");
    let package_path = temporary.path().join("frame-processor.vesper-plugin");
    build_signed_plugin_package(&project, &plugin_directory, &signing_key, &package_path)
        .expect("signed Native plugin package");
    let mut trust_store = PluginTrustStore::empty();
    trust_store
        .insert(signing_key.public_key())
        .expect("trusted publisher key");
    let package =
        verify_signed_plugin_package(&package_path, &trust_store).expect("verified package");
    let install_root = temporary.path().join("installed");
    install_verified_plugin_package(&package, &install_root).expect("installed package");

    let reference = PluginReference::new(
        "dev.vesper.frame-processor-diagnostic",
        Some("dev.vesper.frame-processor-diagnostic.frame".to_owned()),
        PluginTransport::Native,
    )
    .expect("official FrameProcessor reference");
    let host = PluginHostTarget::new(
        semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("host SDK version"),
        "aarch64-apple-darwin",
        "arm64",
    )
    .expect("Native host target");
    let catalog = verify_installed_plugin_catalog(
        &install_root,
        &trust_store,
        &host,
        std::slice::from_ref(&reference),
        &[],
    )
    .expect("verified installed Native catalog");
    let registry = PluginRegistry::load_verified_installed_catalog(&catalog)
        .expect("registry from verified installed Native catalog");
    drop(catalog);

    let resolved = registry
        .resolve_frame_processor(&reference)
        .expect("resolved installed FrameProcessor");
    assert_eq!(resolved.reference(), &reference);
    let capabilities = resolved.capability().capabilities();
    assert!(capabilities.supports_video_frames);

    let mismatched_project = PluginProjectManifest::from_toml(
        &manifest_source.replace("interface_minor = 0", "interface_minor = 1"),
    )
    .expect("valid mismatched project manifest");
    let mismatched_package_path = temporary.path().join("mismatched.vesper-plugin");
    build_signed_plugin_package(
        &mismatched_project,
        &plugin_directory,
        &signing_key,
        &mismatched_package_path,
    )
    .expect("signed mismatched Native plugin package");
    let mismatched_package = verify_signed_plugin_package(&mismatched_package_path, &trust_store)
        .expect("verified mismatched package");
    let mismatched_install_root = temporary.path().join("mismatched-installed");
    install_verified_plugin_package(&mismatched_package, &mismatched_install_root)
        .expect("installed mismatched package");
    let mismatched_catalog = verify_installed_plugin_catalog(
        &mismatched_install_root,
        &trust_store,
        &host,
        std::slice::from_ref(&reference),
        &[],
    )
    .expect("verified mismatched installed catalog");
    assert!(matches!(
        PluginRegistry::load_verified_installed_catalog(&mismatched_catalog),
        Err(PluginRegistryBuildError::InstalledCapabilityMismatch { .. })
    ));
}

#[test]
#[ignore = "requires a built player-decoder-fixture shared library artifact"]
fn native_dynamic_loader_opens_decoder_fixture() {
    assert_native_decoder_plugin(
        "VESPER_DECODER_FIXTURE_PLUGIN_PATH",
        "vesper_decoder_fixture",
        "dev.vesper.decoder-fixture",
        "dev.vesper.decoder-fixture.native",
        "player-decoder-fixture",
    );
}

#[test]
#[ignore = "requires a built player-decoder-videotoolbox shared library artifact"]
fn native_dynamic_loader_opens_videotoolbox_decoder() {
    assert_native_decoder_plugin(
        "VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH",
        "vesper_decoder_videotoolbox",
        "io.github.ikaros.vesper.decoder-videotoolbox",
        "io.github.ikaros.vesper.decoder-videotoolbox.native",
        "player-decoder-videotoolbox",
    );
}

#[test]
#[ignore = "requires a built player-decoder-d3d11 shared library artifact"]
fn native_dynamic_loader_opens_d3d11_decoder() {
    assert_native_decoder_plugin(
        "VESPER_DECODER_D3D11_PLUGIN_PATH",
        "vesper_decoder_d3d11",
        "io.github.ikaros.vesper.decoder-d3d11",
        "io.github.ikaros.vesper.decoder-d3d11.native",
        "player-decoder-d3d11",
    );
}

#[test]
#[ignore = "requires a built player-remux-ffmpeg shared library artifact"]
fn native_dynamic_loader_opens_ffmpeg_post_download_processor() {
    let plugin_path = resolve_plugin_path_with_override(
        "VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH",
        "vesper_remux_ffmpeg",
    )
    .unwrap_or_else(|error| panic!("failed to resolve FFmpeg remux plugin path: {error}"));
    let plugin = LoadedNativePlugin::load_development(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load FFmpeg remux plugin shared library '{}': {error}",
            plugin_path.display()
        )
    });

    assert_eq!(plugin.plugin_id(), "io.github.ikaros.vesper.remux-ffmpeg");
    assert_eq!(plugin.plugin_name(), "player-remux-ffmpeg");
    assert!(plugin.diagnostics().is_empty());
    let reference = PluginReference::new(
        "io.github.ikaros.vesper.remux-ffmpeg",
        Some("io.github.ikaros.vesper.remux-ffmpeg.post-download".to_owned()),
        PluginTransport::Native,
    )
    .expect("static remux reference should be valid");
    let processor = plugin
        .resolve_post_download(&reference)
        .expect("post-download interface should resolve");
    assert_eq!(processor.name(), "player-remux-ffmpeg");
}

#[test]
#[ignore = "requires a built player-source-normalizer-ffmpeg shared library artifact"]
fn native_dynamic_loader_opens_ffmpeg_packet_and_resource_interfaces() {
    let plugin_path = resolve_plugin_path("vesper_source_normalizer_ffmpeg")
        .unwrap_or_else(|error| panic!("failed to resolve FFmpeg plugin path: {error}"));
    let plugin = LoadedNativePlugin::load_development(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load FFmpeg plugin shared library `{}`: {error}",
            plugin_path.display()
        )
    });

    assert_eq!(
        plugin.plugin_id(),
        "io.github.ikaros.vesper.source-normalizer-ffmpeg"
    );
    assert_eq!(plugin.plugin_name(), "player-source-normalizer-ffmpeg");
    assert!(plugin.diagnostics().is_empty());
    assert_eq!(plugin.interfaces().len(), 2);
    assert!(
        plugin
            .interfaces()
            .iter()
            .all(|interface| interface.state == PluginInterfaceState::Available)
    );
    let instance_ids = plugin
        .interfaces()
        .iter()
        .map(|interface| interface.metadata.instance_id.as_str())
        .collect::<Vec<_>>();
    assert!(instance_ids.contains(&"io.github.ikaros.vesper.source-normalizer-ffmpeg.packet"));
    assert!(instance_ids.contains(&"io.github.ikaros.vesper.source-normalizer-ffmpeg.resource"));

    let reference = PluginReference::new(
        "io.github.ikaros.vesper.source-normalizer-ffmpeg",
        None,
        PluginTransport::Native,
    )
    .expect("valid FFmpeg plugin reference");
    let packet = plugin
        .resolve_source_packet(&reference)
        .expect("unique packet SourceNormalizer interface");
    assert!(
        packet
            .packet_capabilities()
            .supported_runtime_profiles
            .contains(&"generic-fallback".to_owned())
    );
    let resource = plugin
        .resolve_source_resource(&reference)
        .expect("unique resource SourceNormalizer interface");
    assert_eq!(resource.resource_capabilities().max_sessions, Some(64));
}

#[test]
#[ignore = "requires a built player-plugin-fixture shared library artifact"]
fn native_registry_retains_root_and_returns_canonical_selection() {
    let plugin_path = resolve_plugin_path("vesper_plugin_fixture")
        .unwrap_or_else(|error| panic!("failed to resolve Plugin fixture path: {error}"));
    let registry = PluginRegistry::load_native_development([&plugin_path])
        .unwrap_or_else(|error| panic!("failed to build plugin registry: {error}"));
    assert_eq!(registry.registered_interfaces().len(), 3);
    assert!(
        registry
            .registered_interfaces()
            .iter()
            .all(|interface| interface.interface.state == PluginInterfaceState::Available)
    );

    let implicit = PluginReference::new("dev.vesper.plugin-fixture", None, PluginTransport::Native)
        .expect("valid fixture reference");
    let resolved = registry
        .resolve_pipeline_event_hook(&implicit)
        .expect("unique event hook");
    assert_eq!(
        resolved.reference().capability_instance_id(),
        Some("dev.vesper.plugin-fixture.event-hook")
    );
    let hook = resolved.capability();
    let post_download = registry
        .resolve_post_download(&implicit)
        .expect("unique post-download processor");
    assert_eq!(
        post_download.reference().capability_instance_id(),
        Some("dev.vesper.plugin-fixture.post-download")
    );
    assert_eq!(post_download.capability().name(), "Vesper Plugin Fixture");

    let benchmark = BenchmarkSinkPluginSession::from_registry(&registry, [implicit.clone()])
        .expect("unique benchmark sink");
    assert_eq!(
        benchmark.references()[0].capability_instance_id(),
        Some("dev.vesper.plugin-fixture.benchmark")
    );
    let report = benchmark.on_event_batch(&BenchmarkEventBatch {
        events: vec![BenchmarkEvent {
            run_id: "run".to_owned(),
            session_id: "session".to_owned(),
            platform: "test".to_owned(),
            source_protocol: None,
            event_name: "frame".to_owned(),
            timestamp_ns: 1,
            elapsed_ns: 1,
            thread: None,
            attributes: BTreeMap::new(),
        }],
    });
    assert_eq!(report.accepted_events, 1);
    drop(resolved);
    drop(post_download);
    drop(benchmark);
    drop(registry);
    let outcome = hook
        .on_event(&pipeline_event(
            "download.task.completed",
            "after-registry-drop",
        ))
        .expect("retained event hook");
    assert!(outcome.accepted);
}

#[test]
#[ignore = "requires a built player-plugin-fixture shared library artifact"]
fn native_inspection_keeps_the_loaded_root_without_legacy_symbol_fallback() {
    let plugin_path = resolve_plugin_path("vesper_plugin_fixture")
        .unwrap_or_else(|error| panic!("failed to resolve Plugin fixture path: {error}"));
    let registry = PluginRegistry::inspect_decoder_support_development(
        [&plugin_path],
        crate::DecoderPluginMatchRequest::video("h264"),
    );

    assert_eq!(registry.records().len(), 1);
    assert_eq!(
        registry.records()[0].status,
        crate::PluginDiagnosticStatus::UnsupportedKind
    );
    assert_eq!(registry.registered_interfaces().len(), 3);

    let reference =
        PluginReference::new("dev.vesper.plugin-fixture", None, PluginTransport::Native)
            .expect("valid fixture reference");
    let resolved = registry
        .resolve_pipeline_event_hook(&reference)
        .expect("inspection retains the native plugin root");
    assert_eq!(
        resolved.reference().capability_instance_id(),
        Some("dev.vesper.plugin-fixture.event-hook")
    );
}

#[test]
#[ignore = "requires built decoder, frame processor, and source normalizer artifacts"]
fn native_inspection_resolves_experimental_capabilities_from_canonical_records() {
    let decoder_path = resolve_plugin_path("vesper_decoder_fixture")
        .unwrap_or_else(|error| panic!("failed to resolve decoder fixture: {error}"));
    let decoder_request = crate::DecoderPluginMatchRequest::video("fixture-video");
    let decoder_registry = PluginRegistry::inspect_decoder_support_development(
        [&decoder_path],
        decoder_request.clone(),
    );
    let decoder_record = decoder_registry
        .best_native_decoder_for(&decoder_request)
        .expect("native decoder record");
    let decoder_reference = decoder_registry
        .reference_for_record(decoder_record)
        .expect("canonical decoder reference");
    assert_eq!(
        decoder_reference.capability_instance_id(),
        Some("dev.vesper.decoder-fixture.native")
    );
    let decoder = decoder_registry
        .resolve_native_decoder(decoder_reference)
        .expect("resolved decoder")
        .capability();
    assert!(
        decoder
            .capabilities()
            .supports_codec("fixture-video", player_plugin::DecoderMediaKind::Video)
    );

    let frame_path = resolve_plugin_path("vesper_frame_processor_diagnostic")
        .unwrap_or_else(|error| panic!("failed to resolve frame processor fixture: {error}"));
    let frame_artifact =
        NativePluginArtifact::new("dev.vesper.frame-processor-diagnostic", &frame_path)
            .expect("valid frame processor artifact");
    let frame_registry =
        PluginRegistry::inspect_frame_processor_support_artifacts([frame_artifact]);
    let frame_record = frame_registry
        .records()
        .iter()
        .find(|record| record.status == crate::PluginDiagnosticStatus::FrameProcessorSupported)
        .expect("frame processor record");
    let frame_reference = frame_registry
        .reference_for_record(frame_record)
        .expect("canonical frame processor reference");
    let frame_processor = frame_registry
        .resolve_frame_processor(frame_reference)
        .expect("resolved frame processor")
        .capability();
    assert!(frame_processor.capabilities().supports_video_frames);

    let source_path = resolve_plugin_path("vesper_source_normalizer_diagnostic")
        .unwrap_or_else(|error| panic!("failed to resolve source normalizer fixture: {error}"));
    let source_artifact = NativePluginArtifact::new(
        "io.github.ikaros.vesper.source-normalizer-diagnostic",
        &source_path,
    )
    .expect("valid source normalizer artifact");
    let source_registry =
        PluginRegistry::inspect_source_normalizer_support_artifacts([source_artifact]);
    let source_record = source_registry
        .best_source_normalizer_packet_for_profile("diagnostic-packet")
        .expect("source normalizer packet record");
    let source_reference = source_registry
        .reference_for_record(source_record)
        .expect("canonical source packet reference");
    let source = source_registry
        .resolve_source_packet(source_reference)
        .expect("resolved source packet")
        .capability();
    assert!(
        source
            .packet_capabilities()
            .supported_runtime_profiles
            .iter()
            .any(|profile| profile == "diagnostic-packet")
    );
}

#[test]
#[ignore = "requires a built player-plugin-fixture shared library artifact"]
fn native_registry_rejects_duplicate_root_identity_and_transport_fallback() {
    let plugin_path = resolve_plugin_path("vesper_plugin_fixture")
        .unwrap_or_else(|error| panic!("failed to resolve Plugin fixture path: {error}"));
    let duplicate = PluginRegistry::load_native_development([&plugin_path, &plugin_path])
        .err()
        .expect("duplicate root identity");
    assert!(matches!(
        duplicate,
        PluginRegistryBuildError::DuplicatePluginIdentity {
            transport: PluginTransport::Native,
            ref plugin_id,
            ..
        } if plugin_id == "dev.vesper.plugin-fixture"
    ));

    let registry = PluginRegistry::load_native_development([&plugin_path])
        .unwrap_or_else(|error| panic!("failed to build plugin registry: {error}"));
    let wasm = PluginReference::new("dev.vesper.plugin-fixture", None, PluginTransport::Wasm)
        .expect("valid fixture reference");
    let error = registry
        .resolve_pipeline_event_hook(&wasm)
        .err()
        .expect("transport fallback must be rejected");
    assert_eq!(
        error,
        PluginSelectionError::PluginNotFound {
            plugin_id: "dev.vesper.plugin-fixture".to_owned(),
            transport: PluginTransport::Wasm,
        }
    );
}

#[test]
#[ignore = "requires a built player-plugin-fixture shared library artifact"]
fn native_registry_rejects_catalog_and_root_identity_mismatch() {
    let plugin_path = resolve_plugin_path("vesper_plugin_fixture")
        .unwrap_or_else(|error| panic!("failed to resolve Plugin fixture path: {error}"));
    let artifact = NativePluginArtifact::new("dev.vesper.other-plugin", &plugin_path)
        .expect("valid but intentionally mismatched identity");
    let error = PluginRegistry::load_native_artifacts([artifact])
        .err()
        .expect("catalog and Root ABI identity mismatch");
    assert!(matches!(
        error,
        PluginRegistryBuildError::PluginIdentityMismatch {
            ref expected_plugin_id,
            ref actual_plugin_id,
            ..
        } if expected_plugin_id == "dev.vesper.other-plugin"
            && actual_plugin_id == "dev.vesper.plugin-fixture"
    ));
}

#[test]
#[ignore = "requires a built player-plugin-fixture shared library artifact"]
fn native_artifact_inspection_reports_catalog_and_root_identity_mismatch() {
    let plugin_path = resolve_plugin_path("vesper_plugin_fixture")
        .unwrap_or_else(|error| panic!("failed to resolve Plugin fixture path: {error}"));
    let artifact = NativePluginArtifact::new("dev.vesper.other-plugin", &plugin_path)
        .expect("valid but intentionally mismatched identity");

    let registry = PluginRegistry::inspect_frame_processor_support_artifacts([artifact]);

    let record = registry
        .records()
        .first()
        .expect("identity mismatch record");
    assert_eq!(record.status, crate::PluginDiagnosticStatus::LoadFailed);
    let message = record.message.as_deref().unwrap_or_default();
    assert!(message.contains("dev.vesper.other-plugin"));
    assert!(message.contains("dev.vesper.plugin-fixture"));
    assert!(registry.registered_interfaces().is_empty());
}

#[test]
#[ignore = "requires a built player-plugin-fixture shared library artifact"]
fn embedded_registry_verifies_checksum_identity_and_root_capabilities() {
    let plugin_path = resolve_plugin_path("vesper_plugin_fixture")
        .unwrap_or_else(|error| panic!("failed to resolve Plugin fixture path: {error}"));
    let bytes = std::fs::read(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to read Plugin fixture `{}`: {error}",
            plugin_path.display()
        )
    });
    let checksum = format!("{:x}", Sha256::digest(&bytes));
    let json = fixture_embedded_registry_json(&checksum);
    let embedded =
        EmbeddedPluginRegistry::parse(json.as_bytes(), "aarch64-linux-android", "arm64-v8a")
            .expect("valid embedded registry");
    let registry = embedded
        .load_native(|locator| {
            assert!(matches!(
                locator,
                EmbeddedPluginLocator::AndroidNativeLibrary { name }
                    if name == "VesperPluginFixture"
            ));
            Ok(plugin_path.clone())
        })
        .expect("checksum, identity, and capabilities match the fixture Root ABI");
    assert_eq!(registry.registered_interfaces().len(), 3);

    let mismatched = EmbeddedPluginRegistry::parse(
        fixture_embedded_registry_json(&"0".repeat(64)).as_bytes(),
        "aarch64-linux-android",
        "arm64-v8a",
    )
    .expect("structurally valid mismatched registry");
    let error = mismatched
        .load_native(|_| Ok(plugin_path.clone()))
        .expect_err("checksum mismatch must fail before plugin load");
    assert!(matches!(
        error,
        EmbeddedPluginRegistryError::ChecksumMismatch { .. }
    ));
}

fn fixture_embedded_registry_json(checksum: &str) -> String {
    format!(
        r#"{{
            "schema_version": 1,
            "target": "aarch64-linux-android",
            "architecture": "arm64-v8a",
            "minimum_os": "26",
            "artifacts": [{{
                "plugin_id": "dev.vesper.plugin-fixture",
                "transport": "native",
                "locator": {{
                    "kind": "android-native-library",
                    "name": "VesperPluginFixture"
                }},
                "integrity": {{
                    "kind": "sha256",
                    "digest": "{checksum}"
                }},
                "package": {{
                    "version": "0.4.0",
                    "publisher": "dev.vesper.publisher",
                    "descriptor_sha256": "{manifest_checksum}"
                }},
                "capabilities": [
                    {{
                        "interface_id": "e9479dbc-42d2-575e-b39e-a24bc512fbc7",
                        "instance_id": "dev.vesper.plugin-fixture.post-download",
                        "interface_major": 1,
                        "interface_minor": 0
                    }},
                    {{
                        "interface_id": "c7a69475-79b2-5b5e-a477-08844a5da5d1",
                        "instance_id": "dev.vesper.plugin-fixture.event-hook",
                        "interface_major": 1,
                        "interface_minor": 0
                    }},
                    {{
                        "interface_id": "2d8e5be8-b1de-5e83-8fe0-6118aabc5118",
                        "instance_id": "dev.vesper.plugin-fixture.benchmark",
                        "interface_major": 1,
                        "interface_minor": 0
                    }}
                ]
            }}]
        }}"#,
        manifest_checksum = "0".repeat(64),
    )
}
