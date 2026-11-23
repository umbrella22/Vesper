use std::env;
use std::path::Path;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
        return;
    }
    let root = Path::new("../../..");
    let include = root.join("include");
    let reference = root.join("plugins/video-filter-reference");
    let sources = [reference.join("metal.mm"), reference.join("metal_host.mm")];
    for path in sources.iter().chain(&[
        include.join("vesper_plugin.h"),
        reference.join("metal_host.h"),
    ]) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    // Match the host kit deployment floor even when using a newer Xcode SDK.
    let deployment_target = if env::var("CARGO_CFG_TARGET_ABI").as_deref() == Ok("sim") {
        "-mios-simulator-version-min=17.0"
    } else {
        "-miphoneos-version-min=17.0"
    };
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .flag(deployment_target)
        .flag("-fobjc-arc")
        .flag("-fblocks")
        .define("VESPER_REFERENCE_EMBEDDED", None)
        .include(include)
        .files(sources)
        .compile("vesper_ios_reference_metal");
    for framework in ["Foundation", "CoreVideo", "Metal"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
