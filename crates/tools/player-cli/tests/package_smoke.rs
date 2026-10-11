use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn packaged_cli_scaffolds_and_inspects_native_and_wasm_plugins() {
    let parent = tempfile::tempdir().expect("temporary scaffold parent");

    for (directory_name, transport, capability) in [
        ("native-plugin", "native", "post-download"),
        ("native-event-hook", "native", "event-hook"),
        ("native-benchmark", "native", "benchmark"),
        ("wasm-plugin", "wasm", "event-hook"),
    ] {
        let directory = parent.path().join(directory_name);
        scaffold(
            &directory,
            &format!("dev.vesper.{directory_name}"),
            transport,
            capability,
        );
        let cargo = fs::read_to_string(directory.join("Cargo.toml")).expect("scaffold Cargo.toml");
        assert!(!cargo.contains("path ="));
        let cargo_manifest: toml::Value =
            toml::from_str(&cargo).expect("parse scaffold Cargo.toml");
        assert_eq!(
            cargo_manifest["package"]["rust-version"].as_str(),
            Some(env!("CARGO_PKG_RUST_VERSION")),
            "scaffold MSRV must match the published CLI's MSRV"
        );
        let published_sdk = if transport == "native" {
            format!(
                "player-plugin = {{ package = \"vesper-player-plugin\", version = \"={}\" }}",
                env!("CARGO_PKG_VERSION")
            )
        } else {
            format!(
                "player-plugin-wasm = {{ package = \"vesper-player-plugin-wasm\", version = \"={}\" }}",
                env!("CARGO_PKG_VERSION")
            )
        };
        assert!(cargo.contains(&published_sdk), "Cargo.toml: {cargo}");

        let inspect = Command::new(env!("CARGO_BIN_EXE_vesper"))
            .args(["plugin", "inspect"])
            .arg(directory.join("vesper-plugin.toml"))
            .arg("--manifest-only")
            .output()
            .expect("inspect packaged scaffold manifest");
        assert_eq!(
            inspect.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&inspect.stderr)
        );
        assert!(inspect.stderr.is_empty());

        if transport == "wasm" {
            assert_eq!(
                fs::read(directory.join("wit/plugin.wit")).expect("scaffold WIT"),
                player_plugin_wasm_host::VESPER_PLUGIN_WIT.as_bytes()
            );
        }
    }
}

fn scaffold(directory: &Path, plugin_id: &str, transport: &str, capability: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "new"])
        .arg(directory)
        .args([
            "--plugin-id",
            plugin_id,
            "--publisher",
            "dev.vesper",
            "--license",
            "Apache-2.0",
            "--transport",
            transport,
            "--capability",
            capability,
        ])
        .output()
        .expect("run packaged scaffold command");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
}

#[test]
#[ignore = "builds external Native fixtures; run explicitly for author SDK migrations"]
fn native_author_scaffolds_build_against_checkout_and_pass_conformance() {
    let parent = tempfile::tempdir().expect("temporary external consumer parent");
    let sdk = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/player-plugin")
        .canonicalize()
        .expect("checkout SDK path");
    let target = parent.path().join("cargo-target");

    for (directory_name, capability) in [
        ("native-event-hook", "event-hook"),
        ("native-benchmark", "benchmark"),
    ] {
        let directory = parent.path().join(directory_name);
        scaffold(
            &directory,
            &format!("dev.vesper.{directory_name}"),
            "native",
            capability,
        );
        let manifest_path = directory.join("Cargo.toml");
        let source_manifest = fs::read_to_string(&manifest_path).expect("generated Cargo manifest");
        let mut manifest: toml::Value =
            toml::from_str(&source_manifest).expect("parse Cargo manifest");
        // Preserve the generated version pin and add a checkout path only for
        // this unreleased source-API test. No registry-backed build is claimed.
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
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .expect("build external Native author");
        assert!(
            build.status.success(),
            "external {capability} build failed:\n{}\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
        let artifact = String::from_utf8(build.stdout)
            .expect("Cargo JSON UTF-8")
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find_map(|message| {
                if message["reason"] == "compiler-artifact"
                    && message["target"]["crate_types"]
                        .as_array()?
                        .iter()
                        .any(|kind| kind == "cdylib")
                {
                    message["filenames"][0]
                        .as_str()
                        .map(std::path::PathBuf::from)
                } else {
                    None
                }
            })
            .expect("external Native cdylib artifact");
        assert!(artifact.is_file());
        let check = Command::new(env!("CARGO_BIN_EXE_vesper"))
            .args(["plugin", "check"])
            .arg(directory.join("vesper-plugin.toml"))
            .arg("--artifact")
            .arg(&artifact)
            .args(["--transport", "native"])
            .output()
            .expect("invoke generated Native plugin through checked loader");
        assert!(
            check.status.success(),
            "external {capability} conformance failed:\n{}\n{}",
            String::from_utf8_lossy(&check.stdout),
            String::from_utf8_lossy(&check.stderr)
        );
        assert!(check.stderr.is_empty());
    }
}
