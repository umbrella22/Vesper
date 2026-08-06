use std::env;
use std::path::Path;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(vesper_source_checkout)");

    let Ok(manifest_dir) =
        Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_owned()))
            .canonicalize()
    else {
        return;
    };
    let root = manifest_dir.join("../../..");
    let Ok(workspace_member) = root.join("crates/tools/player-cli").canonicalize() else {
        return;
    };
    if manifest_dir == workspace_member {
        println!("cargo::rustc-cfg=vesper_source_checkout");
    }
}
