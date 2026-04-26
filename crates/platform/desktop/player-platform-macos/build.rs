use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/avfoundation_probe.m");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("out dir"));
    let sdk_path = run_command("xcrun", &["--sdk", "macosx", "--show-sdk-path"], None);
    let sdk_path = sdk_path.trim();

    let source = manifest_dir.join("src/avfoundation_probe.m");
    let object = out_dir.join("avfoundation_probe.o");
    let library = out_dir.join("libplayer_platform_macos_avfoundation_probe.a");

    run_command(
        "xcrun",
        &[
            "clang",
            "-fobjc-arc",
            "-fblocks",
            "-isysroot",
            sdk_path,
            "-mmacosx-version-min=11.0",
            "-c",
            path_to_str(&source),
            "-o",
            path_to_str(&object),
        ],
        None,
    );
    run_command(
        "xcrun",
        &[
            "libtool",
            "-static",
            "-o",
            path_to_str(&library),
            path_to_str(&object),
        ],
        None,
    );

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=player_platform_macos_avfoundation_probe");
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=CoreVideo");
    println!("cargo:rustc-link-lib=framework=AudioToolbox");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=Metal");
    println!("cargo:rustc-link-lib=framework=QuartzCore");
}

fn run_command(command: &str, args: &[&str], cwd: Option<&Path>) -> String {
    let mut process = Command::new(command);
    process.args(args);
    if let Some(cwd) = cwd {
        process.current_dir(cwd);
    }

    let output = process
        .output()
        .unwrap_or_else(|error| panic!("failed to run {command}: {error}"));
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "command `{command} {}` failed with status {}: {stderr}",
            args.join(" "),
            output.status
        );
    }

    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn path_to_str(path: &Path) -> &str {
    path.to_str()
        .unwrap_or_else(|| panic!("path is not valid UTF-8: {}", path.display()))
}
