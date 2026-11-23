use player_plugin::{PluginReference, PluginTransport};
use sha2::{Digest, Sha256};

use crate::{
    EmbeddedPluginLocator, EmbeddedPluginRegistry, EmbeddedPluginRegistryError, LoadedNativePlugin,
    NativePluginArtifact, PluginInterfaceState, PluginRegistry, PluginRegistryBuildError,
    PluginSelectionError,
};

use super::{resolve_plugin_path, resolve_plugin_path_with_override};

#[cfg(any(target_os = "macos", target_os = "windows"))]
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
fn native_dynamic_loader_opens_fixture_and_resolves_post_download() {
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
    let processor = plugin
        .resolve_post_download(&reference)
        .expect("post-download fixture");
    assert_eq!(
        processor.supported_input_formats(),
        &[player_plugin::ContentFormatKind::SingleFile]
    );
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires a built player-decoder-videotoolbox shared library artifact"]
fn native_dynamic_loader_opens_videotoolbox_decoder() {
    assert_native_decoder_plugin(
        "VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH",
        "vesper_decoder_videotoolbox",
        "io.github.umbrella22.vesper.decoder-videotoolbox",
        "io.github.umbrella22.vesper.decoder-videotoolbox.native",
        "player-decoder-videotoolbox",
    );
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "requires a built player-decoder-d3d11 shared library artifact"]
fn native_dynamic_loader_opens_d3d11_decoder() {
    assert_native_decoder_plugin(
        "VESPER_DECODER_D3D11_PLUGIN_PATH",
        "vesper_decoder_d3d11",
        "io.github.umbrella22.vesper.decoder-d3d11",
        "io.github.umbrella22.vesper.decoder-d3d11.native",
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

    assert_eq!(
        plugin.plugin_id(),
        "io.github.umbrella22.vesper.remux-ffmpeg"
    );
    assert_eq!(plugin.plugin_name(), "player-remux-ffmpeg");
    assert!(plugin.diagnostics().is_empty());
    let reference = PluginReference::new(
        "io.github.umbrella22.vesper.remux-ffmpeg",
        Some("io.github.umbrella22.vesper.remux-ffmpeg.post-download".to_owned()),
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
        "io.github.umbrella22.vesper.source-normalizer-ffmpeg"
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
    assert!(instance_ids.contains(&"io.github.umbrella22.vesper.source-normalizer-ffmpeg.packet"));
    assert!(
        instance_ids.contains(&"io.github.umbrella22.vesper.source-normalizer-ffmpeg.resource")
    );

    let reference = PluginReference::new(
        "io.github.umbrella22.vesper.source-normalizer-ffmpeg",
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
    assert_eq!(registry.registered_interfaces().len(), 1);
    assert!(
        registry
            .registered_interfaces()
            .iter()
            .all(|interface| interface.interface.state == PluginInterfaceState::Available)
    );

    let implicit = PluginReference::new("dev.vesper.plugin-fixture", None, PluginTransport::Native)
        .expect("valid fixture reference");
    let resolved = registry
        .resolve_post_download(&implicit)
        .expect("unique post-download processor");
    assert_eq!(
        resolved.reference().capability_instance_id(),
        Some("dev.vesper.plugin-fixture.post-download")
    );
    let processor = resolved.capability();
    drop(resolved);
    drop(registry);
    assert_eq!(processor.name(), "Vesper Plugin Fixture");
    assert_eq!(
        processor.capabilities().supported_input_formats,
        [player_plugin::ContentFormatKind::SingleFile]
    );
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
    assert_eq!(registry.registered_interfaces().len(), 1);

    let reference =
        PluginReference::new("dev.vesper.plugin-fixture", None, PluginTransport::Native)
            .expect("valid fixture reference");
    let resolved = registry
        .resolve_post_download(&reference)
        .expect("inspection retains the native plugin root");
    assert_eq!(
        resolved.reference().capability_instance_id(),
        Some("dev.vesper.plugin-fixture.post-download")
    );
}

#[test]
#[ignore = "requires a built player-plugin-fixture shared library artifact"]
fn native_registry_rejects_duplicate_root_identity_and_transport_fallback() {
    let plugin_path = resolve_plugin_path("vesper_plugin_fixture")
        .unwrap_or_else(|error| panic!("failed to resolve Plugin fixture path: {error}"));
    let duplicate = PluginRegistry::load_native_development([&plugin_path, &plugin_path])
        .expect_err("duplicate root identity");
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
        .resolve_post_download(&wasm)
        .expect_err("transport fallback must be rejected");
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
        .expect_err("catalog and Root ABI identity mismatch");
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
    let checksum = hex::encode(Sha256::digest(&bytes));
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
    assert_eq!(registry.registered_interfaces().len(), 1);

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

#[test]
#[cfg(unix)]
#[ignore = "requires a built player-plugin-fixture shared library artifact"]
fn registered_native_artifact_retains_verified_library_without_reopening_path() {
    let fixture = resolve_plugin_path("vesper_plugin_fixture").expect("built plugin fixture");
    let directory = tempfile::tempdir().expect("temporary plugin directory");
    let path = directory
        .path()
        .join(fixture.file_name().expect("library filename"));
    std::fs::copy(&fixture, &path).expect("copy fixture");
    let checksum = hex::encode(Sha256::digest(std::fs::read(&path).expect("fixture bytes")));
    let embedded = EmbeddedPluginRegistry::parse(
        fixture_embedded_registry_json(&checksum).as_bytes(),
        "aarch64-linux-android",
        "arm64-v8a",
    )
    .expect("embedded registry");
    let registry = embedded
        .load_native(|_| Ok(path.clone()))
        .expect("verified library");
    let reference = registry
        .post_download_references()
        .expect("post-download references")
        .remove(0);
    let artifact = registry
        .native_artifact(&reference)
        .expect("retained artifact");
    drop(registry);
    std::fs::remove_file(&path).expect("remove original locator");

    let consumer =
        PluginRegistry::load_native_artifacts([artifact]).expect("must reuse the loaded library");
    let processor = consumer
        .resolve_post_download(&reference)
        .expect("retained processor");
    assert_eq!(processor.capability().name(), "Vesper Plugin Fixture");
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
                    }}
                ]
            }}]
        }}"#,
        manifest_checksum = "0".repeat(64),
    )
}
