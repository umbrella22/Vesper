use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use player_plugin::{PluginReference, PluginTransport};
use serde::Serialize;
use thiserror::Error;

use crate::plugin_scaffold_assets::{
    APACHE_2_LICENSE, CPP_CMAKE, CPP_README, CPP_SOURCE, RUST_CARGO_TEMPLATE, RUST_README,
    RUST_SOURCE_TEMPLATE,
};

#[derive(Debug, Clone, Copy, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginScaffoldLanguage {
    Rust,
    Cpp,
}

#[derive(Debug)]
pub struct PluginScaffoldRequest {
    pub directory: PathBuf,
    pub plugin_id: String,
    pub plugin_name: Option<String>,
    pub license: String,
    pub language: PluginScaffoldLanguage,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginScaffoldReport {
    pub directory: PathBuf,
    pub plugin_id: String,
    pub language: PluginScaffoldLanguage,
    pub abi_major: u32,
    pub entry_symbol: &'static str,
    pub build_file: PathBuf,
}

#[derive(Debug, Error)]
pub enum PluginScaffoldError {
    #[error("invalid plugin scaffold: {0}")]
    Invalid(String),
    #[error("failed to {action} '{}': {source}", path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub fn create_plugin_scaffold(
    request: PluginScaffoldRequest,
) -> Result<PluginScaffoldReport, PluginScaffoldError> {
    if request.directory.symlink_metadata().is_ok() {
        return Err(PluginScaffoldError::Invalid(format!(
            "destination '{}' already exists; refusing to replace it",
            request.directory.display()
        )));
    }
    let parent = request
        .directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let reference = PluginReference::new(&request.plugin_id, None, PluginTransport::Native)
        .map_err(|error| PluginScaffoldError::Invalid(error.to_string()))?;
    let component = reference.plugin_id().rsplit('.').next().unwrap_or_default();
    let plugin_name = request
        .plugin_name
        .as_deref()
        .unwrap_or(reference.plugin_id());
    if plugin_name.trim().is_empty() || plugin_name.chars().any(char::is_control) {
        return Err(PluginScaffoldError::Invalid(
            "invalid plugin name".to_owned(),
        ));
    }
    if request.license.trim().is_empty() || request.license.chars().any(char::is_control) {
        return Err(PluginScaffoldError::Invalid(
            "invalid license expression".to_owned(),
        ));
    }
    let staging = tempfile::Builder::new()
        .prefix(".vesper-plugin-new-")
        .tempdir_in(parent)
        .map_err(|source| PluginScaffoldError::Io {
            action: "create scaffold staging directory",
            path: parent.to_owned(),
            source,
        })?;
    let staging_path = staging.path();
    let (build_file, readme) = match request.language {
        PluginScaffoldLanguage::Rust => {
            let cargo = RUST_CARGO_TEMPLATE
                .replace("{{CRATE_NAME}}", &format!("\"vesper-plugin-{component}\""))
                .replace("{{RUST_VERSION}}", env!("CARGO_PKG_RUST_VERSION"))
                .replace("{{VESPER_SDK_VERSION}}", env!("CARGO_PKG_VERSION"))
                .replace(
                    "{{LICENSE}}",
                    &toml::Value::String(request.license.clone()).to_string(),
                );
            let source = RUST_SOURCE_TEMPLATE.replace(
                "{{PLUGIN_ID}}, {{PLUGIN_NAME}}",
                &format!("{:?}, {plugin_name:?}", request.plugin_id),
            );
            write_scaffold_file(staging_path, "Cargo.toml", &cargo)?;
            write_scaffold_file(staging_path, "src/lib.rs", &source)?;
            let readme = RUST_README.lines().take(3).collect::<Vec<_>>().join("\n")
                + "\n\nRun `cargo build --release` to build the native library. Linked hosts can call `gain_plugin()` directly. For an unpublished native plugin ABI SDK, point the `player-plugin` dependency at the source checkout.\n";
            ("Cargo.toml", readme)
        }
        PluginScaffoldLanguage::Cpp => {
            let target = format!("vesper_plugin_{}", component.replace('-', "_"));
            let cmake = CPP_CMAKE.replace("vesper_gain_cpp", &target);
            let source = CPP_SOURCE.replace(
                "bytes(\"dev.vesper.example.gain-cpp\"), bytes(\"C++ Fixed Gain\")",
                &format!(
                    "bytes({}), bytes({})",
                    cpp_string(&request.plugin_id),
                    cpp_string(plugin_name)
                ),
            );
            write_scaffold_file(staging_path, "CMakeLists.txt", &cmake)?;
            write_scaffold_file(staging_path, "gain.cpp", &source)?;
            ("CMakeLists.txt", CPP_README.to_owned())
        }
    };
    write_scaffold_file(staging_path, "README.md", &readme)?;
    let license = if request.license == "Apache-2.0" {
        APACHE_2_LICENSE.to_owned()
    } else {
        format!(
            "SPDX-License-Identifier: {}\n\nInclude the complete license text before distribution.\n",
            request.license
        )
    };
    write_scaffold_file(staging_path, "LICENSE", &license)?;
    write_scaffold_file(
        staging_path,
        "NOTICE",
        &format!("{plugin_name}\nCopyright (c) plugin contributors\n"),
    )?;
    fs::rename(staging_path, &request.directory).map_err(|source| PluginScaffoldError::Io {
        action: "publish scaffold directory",
        path: request.directory.clone(),
        source,
    })?;
    Ok(PluginScaffoldReport {
        build_file: request.directory.join(build_file),
        directory: request.directory,
        plugin_id: request.plugin_id,
        language: request.language,
        abi_major: 2,
        entry_symbol: "vesper_native_plugin_entry",
    })
}

fn cpp_string(value: &str) -> String {
    let mut literal = String::from("\"");
    for byte in value.bytes() {
        if byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\') || byte == b' ' {
            literal.push(char::from(byte));
        } else {
            // A fixed-width octal escape preserves UTF-8 bytes and stops before the next character.
            let _ = write!(literal, "\\{byte:03o}");
        }
    }
    literal.push('"');
    literal
}

fn write_scaffold_file(
    root: &Path,
    relative_path: &str,
    contents: &str,
) -> Result<(), PluginScaffoldError> {
    let path = root.join(relative_path);
    let parent = path.parent().unwrap_or(root);
    fs::create_dir_all(parent).map_err(|source| PluginScaffoldError::Io {
        action: "create scaffold directory",
        path: parent.to_owned(),
        source,
    })?;
    fs::write(&path, contents).map_err(|source| PluginScaffoldError::Io {
        action: "write scaffold file",
        path,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_scaffold_assets_match_repository_sources() {
        let Some(root) = crate::source_checkout_root() else {
            return;
        };
        for (path, embedded) in [
            (
                "templates/vesper-plugin/rust-native/Cargo.toml.template",
                RUST_CARGO_TEMPLATE,
            ),
            (
                "templates/vesper-plugin/rust-native/src/lib.rs.template",
                RUST_SOURCE_TEMPLATE,
            ),
            ("templates/vesper-plugin/rust-native/README.md", RUST_README),
            (
                "templates/vesper-plugin/cpp-cmake/CMakeLists.txt",
                CPP_CMAKE,
            ),
            ("templates/vesper-plugin/cpp-cmake/gain.cpp", CPP_SOURCE),
            ("templates/vesper-plugin/cpp-cmake/README.md", CPP_README),
            ("LICENSE", APACHE_2_LICENSE),
        ] {
            let actual = fs::read_to_string(root.join(path)).expect("read repository asset");
            assert_eq!(actual, embedded, "embedded asset differs from {path}");
        }
    }
}
