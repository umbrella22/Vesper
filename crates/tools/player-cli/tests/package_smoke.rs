use std::fs;
use std::process::Command;

#[test]
fn packaged_cli_scaffolds_and_inspects_native_and_wasm_plugins() {
    let parent = tempfile::tempdir().expect("temporary scaffold parent");

    for (directory_name, transport, capability) in [
        ("native-plugin", "native", "post-download"),
        ("wasm-plugin", "wasm", "event-hook"),
    ] {
        let directory = parent.path().join(directory_name);
        let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
            .args(["plugin", "new"])
            .arg(&directory)
            .args([
                "--plugin-id",
                &format!("dev.vesper.{directory_name}"),
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
        let cargo = fs::read_to_string(directory.join("Cargo.toml")).expect("scaffold Cargo.toml");
        assert!(!cargo.contains("path ="));
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
