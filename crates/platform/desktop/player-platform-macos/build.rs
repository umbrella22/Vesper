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

    let root = manifest_dir.join("../../../..");
    let include = root.join("include");
    let filter_directory = root.join("plugins/video-filter-reference");
    let filter_sources = ["metal.mm", "metal_host.mm"];
    let filter_objects = ["vesper_reference_filter.o", "vesper_reference_metal_host.o"]
        .map(|name| out_dir.join(name));
    println!(
        "cargo:rerun-if-changed={}",
        include.join("vesper_plugin.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        filter_directory.join("metal_host.h").display()
    );
    for source in filter_sources {
        println!(
            "cargo:rerun-if-changed={}",
            filter_directory.join(source).display()
        );
    }

    let source = manifest_dir.join("src/avfoundation_probe.m");
    let object = out_dir.join("avfoundation_probe.o");
    let library = out_dir.join("libvesper_platform_macos_avfoundation_probe.a");

    run_command(
        "xcrun",
        &[
            "clang",
            "-fobjc-arc",
            "-fblocks",
            "-I",
            path_to_str(&include),
            "-I",
            path_to_str(&filter_directory),
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
    for (source, output) in filter_sources.into_iter().zip(&filter_objects) {
        run_command(
            "xcrun",
            &[
                "clang++",
                "-fobjc-arc",
                "-fblocks",
                "-std=c++17",
                "-fvisibility=hidden",
                "-DVESPER_REFERENCE_EMBEDDED",
                "-isysroot",
                sdk_path,
                "-mmacosx-version-min=11.0",
                "-I",
                path_to_str(&include),
                "-c",
                path_to_str(&filter_directory.join(source)),
                "-o",
                path_to_str(output),
            ],
            None,
        );
    }
    run_command(
        "xcrun",
        &[
            "libtool",
            "-static",
            "-o",
            path_to_str(&library),
            path_to_str(&object),
            path_to_str(&filter_objects[0]),
            path_to_str(&filter_objects[1]),
        ],
        None,
    );

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=vesper_platform_macos_avfoundation_probe");
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=CoreVideo");
    println!("cargo:rustc-link-lib=framework=AudioToolbox");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=Metal");
    println!("cargo:rustc-link-lib=c++");
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
