use std::env;
use std::path::{Path, PathBuf};

mod native_dynamic_tests;
mod participation_projection_tests;
mod resolver_tests;
#[cfg(feature = "wasm")]
mod wasm_registry_tests;

fn resolve_plugin_path(stem: &str) -> Result<PathBuf, String> {
    let workspace_root = workspace_root()?;
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                workspace_root.join(path)
            }
        })
        .unwrap_or_else(|| workspace_root.join("target"));
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_owned());
    let library_name = shared_library_name(stem);
    let candidates = [
        target_dir.join(&profile).join(&library_name),
        target_dir.join(&profile).join("deps").join(&library_name),
        target_dir.join("debug").join(&library_name),
        target_dir.join("debug").join("deps").join(&library_name),
        target_dir.join("release").join(&library_name),
        target_dir.join("release").join("deps").join(&library_name),
    ];

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            format!(
                "could not find `{library_name}` under `{}`; build the plugin crate first",
                target_dir.display()
            )
        })
}

fn resolve_plugin_path_with_override(
    environment_variable: &str,
    stem: &str,
) -> Result<PathBuf, String> {
    if let Some(path) = env::var_os(environment_variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "{environment_variable} points to a missing plugin artifact: {}",
            path.display()
        ));
    }
    resolve_plugin_path(stem)
}

fn shared_library_name(stem: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{stem}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{stem}.dylib")
    } else {
        format!("lib{stem}.so")
    }
}

fn workspace_root() -> Result<PathBuf, String> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .ok_or_else(|| "failed to derive workspace root from CARGO_MANIFEST_DIR".to_owned())
}
