use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn packaged_cli_scaffolds_native_plugins() {
    let parent = tempfile::tempdir().expect("temporary scaffold parent");
    let directory = parent.path().join("rust-gain");
    scaffold(&directory, "dev.vesper.rust-gain", "rust");
    let cargo = fs::read_to_string(directory.join("Cargo.toml")).expect("scaffold Cargo.toml");
    let manifest: toml::Value = toml::from_str(&cargo).expect("parse scaffold Cargo.toml");
    assert_eq!(
        manifest["package"]["rust-version"].as_str(),
        Some(env!("CARGO_PKG_RUST_VERSION")),
    );
    let dependency = &manifest["dependencies"]["player-plugin"];
    assert_eq!(dependency["package"].as_str(), Some("vesper-player-plugin"));
    assert_eq!(
        dependency["version"].as_str(),
        Some(concat!("=", env!("CARGO_PKG_VERSION"))),
    );
    assert!(dependency.get("path").is_none());
    assert!(
        fs::read_to_string(directory.join("src/lib.rs"))
            .expect("scaffold Rust source")
            .contains("#[player_plugin::author::export]")
    );

    let directory = parent.path().join("cpp-gain");
    scaffold(&directory, "dev.vesper.cpp-gain", "cpp");
    assert!(directory.join("CMakeLists.txt").is_file());
    assert!(
        fs::read_to_string(directory.join("gain.cpp"))
            .expect("scaffold C++ source")
            .contains("vesper_native_plugin_entry")
    );
}

fn scaffold(directory: &Path, plugin_id: &str, language: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "new"])
        .arg(directory)
        .args(["--plugin-id", plugin_id, "--language", language])
        .output()
        .expect("run packaged scaffold command");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("scaffold JSON");
    assert_eq!(report["abiMajor"], 2);
    assert_eq!(report["entrySymbol"], "vesper_native_plugin_entry");
    assert_eq!(report["pluginId"], plugin_id);
    assert_eq!(report["language"], language);
}

#[test]
#[ignore = "builds an external native fixture against the source SDK"]
fn rust_author_scaffold_builds_against_checkout_and_passes_conformance() {
    let parent = tempfile::tempdir().expect("temporary external consumer parent");
    let sdk = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/player-plugin")
        .canonicalize()
        .expect("checkout SDK path");
    let directory = parent.path().join("rust-gain");
    let plugin_id = "dev.vesper.rust-gain";
    scaffold(&directory, plugin_id, "rust");
    let manifest_path = directory.join("Cargo.toml");
    let source_manifest = fs::read_to_string(&manifest_path).expect("generated Cargo manifest");
    let mut manifest: toml::Value = toml::from_str(&source_manifest).expect("parse Cargo manifest");
    manifest["dependencies"]["player-plugin"]
        .as_table_mut()
        .expect("SDK dependency table")
        .insert(
            "path".to_owned(),
            toml::Value::String(sdk.to_str().expect("UTF-8 SDK path").to_owned()),
        );
    fs::write(
        &manifest_path,
        toml::to_string(&manifest).expect("serialize test manifest"),
    )
    .expect("write checkout consumer manifest");

    let build = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(&directory)
        .args([
            "build",
            "--offline",
            "--message-format=json-render-diagnostics",
        ])
        .env("CARGO_TARGET_DIR", parent.path().join("cargo-target"))
        .output()
        .expect("build external native author");
    assert!(
        build.status.success(),
        "external gain build failed:\n{}\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    let artifact = String::from_utf8(build.stdout)
        .expect("Cargo JSON UTF-8")
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|message| {
            if message["reason"] != "compiler-artifact"
                || !message["target"]["crate_types"]
                    .as_array()?
                    .iter()
                    .any(|kind| kind == "cdylib")
            {
                return None;
            }
            message["filenames"].as_array()?.iter().find_map(|file| {
                let path = Path::new(file.as_str()?);
                (path.extension()?.to_str()? == std::env::consts::DLL_EXTENSION)
                    .then(|| path.to_path_buf())
            })
        })
        .expect("external native cdylib artifact");
    assert!(artifact.is_file());
    let plugin_manifest = directory.join("vesper-plugin.toml");
    let interface_id = uuid::Uuid::from_bytes(player_plugin_abi::VESPER_AUDIO_PROCESSOR_ID.bytes);
    fs::write(
        &plugin_manifest,
        format!(
            r#"schema_version = 1
[plugin]
id = "{plugin_id}"
name = "External Rust gain"
version = "0.1.0"
description = "A fixed-block native plugin ABI conformance fixture"
license = "Apache-2.0"
publisher = "dev.vesper"
[compatibility]
host_sdk = "={host_version}"
abi_major = 2
abi_minor_min = 0
abi_minor_max = 0
[[capabilities]]
interface_id = "{interface_id}"
instance_id = "{plugin_id}"
interface_major = 2
interface_minor = 0
stability = "stable"
"#,
            host_version = env!("CARGO_PKG_VERSION"),
        ),
    )
    .expect("write native plugin ABI conformance descriptor");
    let check = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "check"])
        .arg(&plugin_manifest)
        .arg("--artifact")
        .arg(&artifact)
        .args(["--transport", "native"])
        .output()
        .expect("invoke generated native plugin through checked loader");
    assert!(
        check.status.success(),
        "external gain conformance failed:\n{}\n{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(check.stderr.is_empty());
    let report: serde_json::Value =
        serde_json::from_slice(&check.stdout).expect("conformance JSON");
    assert_eq!(report["runtimePluginId"], plugin_id);
    assert_eq!(report["outcome"], "passed");
}
