use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{Map, Value};
use thiserror::Error;

const MAX_CONTRACT_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_CONTRACT_SCAN_FILES: usize = 20_000;

const FLUTTER_MODELS_ROOT: &str = "lib/flutter/vesper_player_platform_interface/lib/src";
const FLUTTER_MODELS_BARREL: &str =
    "lib/flutter/vesper_player_platform_interface/lib/src/models.dart";

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("{0}")]
    Drift(String),
    #[error("{0}")]
    Storage(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractVerification {
    output: String,
}

impl ContractVerification {
    pub fn output(&self) -> &str {
        &self.output
    }
}

struct Repository<'a> {
    root: &'a Path,
}

impl<'a> Repository<'a> {
    fn new(root: &'a Path) -> Self {
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn require_file(&self, relative: &str) -> Result<(), ContractError> {
        let path = self.path(relative);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| contract_drift(format!("Missing contract source: {relative}")))?;
        if !metadata.file_type().is_file() {
            return Err(contract_drift(format!(
                "Missing contract source: {relative}"
            )));
        }
        Ok(())
    }

    fn read_text(&self, relative: &str) -> Result<String, ContractError> {
        read_regular_text(&self.path(relative), relative)
    }

    fn read_json(&self, relative: &str) -> Result<Value, ContractError> {
        let text = self.read_text(relative)?;
        serde_json::from_str(&text)
            .map_err(|error| contract_drift(format!("invalid JSON in {relative}: {error}")))
    }

    fn files_under(&self, relative: &str) -> Result<Vec<PathBuf>, ContractError> {
        collect_regular_files(&self.path(relative))
    }

    fn tree_contains(&self, relative: &str, needle: &str) -> Result<bool, ContractError> {
        for path in self.files_under(relative)? {
            if regular_file_contains(&path, needle)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn flutter_models_contain(&self, needle: &str) -> Result<bool, ContractError> {
        if self.read_text(FLUTTER_MODELS_BARREL)?.contains(needle) {
            return Ok(true);
        }
        for path in self.files_under(FLUTTER_MODELS_ROOT)? {
            if path.extension().and_then(|value| value.to_str()) == Some("dart")
                && regular_file_contains(&path, needle)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

enum SearchTarget {
    FlutterModels,
    File(&'static str),
    Tree(&'static str),
}

struct WireExpectation {
    target: SearchTarget,
    needle: fn(&str) -> String,
}

pub fn resolve_repository_root(explicit: Option<&Path>) -> Result<PathBuf, ContractError> {
    let candidate = match explicit {
        Some(path) => path.to_path_buf(),
        None => env::var_os("VESPER_REPO_ROOT")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| {
                env::current_dir().map_err(|error| {
                    ContractError::Storage(format!(
                        "failed to determine the current directory: {error}"
                    ))
                })
            })?,
    };
    let root = candidate.canonicalize().map_err(|error| {
        ContractError::Storage(format!(
            "failed to resolve repository root '{}': {error}",
            candidate.display()
        ))
    })?;
    let cargo_manifest = root.join("Cargo.toml");
    if !cargo_manifest.is_file() {
        return Err(ContractError::Storage(format!(
            "repository root '{}' does not contain Cargo.toml",
            root.display()
        )));
    }
    Ok(root)
}

pub fn verify(root: &Path) -> Result<ContractVerification, ContractError> {
    let repository = Repository::new(root);
    for fixture in [
        "fixtures/contracts/player_error.json",
        "fixtures/contracts/plugin_diagnostics.json",
        "fixtures/contracts/download_task_snapshot.json",
        "fixtures/contracts/system_playback_configuration.json",
        "fixtures/contracts/subtitle_error.json",
        "fixtures/contracts/subtitle_state.json",
    ] {
        repository.require_file(fixture)?;
    }

    let mut output = Vec::new();
    verify_dto_drift(&repository, &mut output)?;
    verify_binary_library_names_in_repository(&repository)?;
    output.push(
        "Verified Rust and mobile distribution binary library names use libvesper_* outputs."
            .to_owned(),
    );

    require_flutter_models_text(&repository, "unsupported")?;
    require_text(
        &repository,
        "lib/android/vesper-player-kit/src/main/java/io/github/umbrella22/vesper/player/android/VesperPlayerError.kt",
        "unsupported",
    )?;
    require_text(
        &repository,
        "lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/Bridge/VesperPlayerError.swift",
        "case unsupported",
    )?;
    require_text_in_tree(
        &repository,
        "crates/ffi/player-ffi/src/c_api",
        "PlayerFfiErrorCode::Unsupported",
    )?;

    for needle in [
        "decoderSupported",
        "frameProcessorSupported",
        "sourceNormalizerSupported",
        "VesperPluginParticipation",
    ] {
        require_flutter_models_text(&repository, needle)?;
    }
    for needle in [
        "DecoderSupported",
        "FrameProcessorSupported",
        "SourceNormalizerSupported",
        "PlayerPluginParticipation",
    ] {
        require_text(&repository, "crates/core/player-runtime/src/lib.rs", needle)?;
    }
    for needle in [
        "DecoderSupported",
        "FrameProcessorSupported",
        "SourceNormalizerSupported",
        "PlayerFfiPluginParticipation",
    ] {
        require_text_in_tree(&repository, "crates/ffi/player-ffi/src/c_api", needle)?;
    }
    for needle in [
        "\"participation\": \"participated\"",
        "\"participation\": \"available\"",
        "\"participation\": \"bypassed\"",
    ] {
        require_text(
            &repository,
            "fixtures/contracts/plugin_diagnostics.json",
            needle,
        )?;
    }

    require_text(
        &repository,
        "lib/flutter/vesper_player_platform_interface/lib/src/download_models.dart",
        "dashSegments",
    )?;
    require_text(
        &repository,
        "lib/android/vesper-player-kit/src/main/java/io/github/umbrella22/vesper/player/android/VesperDownloadTypes.kt",
        "DashSegments",
    )?;
    require_text(
        &repository,
        "lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/Download/Manager/VesperDownloadConfiguration.swift",
        "case dashSegments",
    )?;

    require_flutter_models_text(&repository, "continueAudio")?;
    require_text(
        &repository,
        "lib/android/vesper-player-kit/src/main/java/io/github/umbrella22/vesper/player/android/PlayerBridge.kt",
        "ContinueAudio",
    )?;
    require_text(
        &repository,
        "lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/Bridge/PlayerBridgeSystemPlaybackModels.swift",
        "case continueAudio",
    )?;

    output.push(
        "Contract fixtures match the checked Rust, Android, iOS, and Flutter wire names."
            .to_owned(),
    );
    Ok(ContractVerification {
        output: format!("{}\n", output.join("\n")),
    })
}

fn verify_dto_drift(
    repository: &Repository<'_>,
    output: &mut Vec<String>,
) -> Result<(), ContractError> {
    let player_error_path = "fixtures/contracts/player_error.json";
    let player_error = json_object(repository.read_json(player_error_path)?, player_error_path)?;
    require_json_keys(
        &player_error,
        player_error_path,
        &["message", "code", "category", "retriable", "details"],
    )?;

    require_exact_json_keys(
        repository,
        "fixtures/contracts/subtitle_error.json",
        &[
            "domain",
            "code",
            "phase",
            "trackId",
            "retriable",
            "message",
            "commandId",
            "sourceEpoch",
        ],
    )?;
    output.push("checked subtitle error fields".to_owned());

    require_exact_json_keys(
        repository,
        "fixtures/contracts/subtitle_state.json",
        &[
            "catalogState",
            "selectionState",
            "advertisedTrackCount",
            "selectableTrackCount",
            "catalogError",
            "selectionError",
        ],
    )?;
    output.push("checked subtitle state fields".to_owned());

    let player_error_values = vec![
        required_string(&player_error, player_error_path, "code")?,
        required_string(&player_error, player_error_path, "category")?,
    ];
    check_wire_values(
        repository,
        "player error code/category",
        &player_error_values,
        &[
            WireExpectation {
                target: SearchTarget::FlutterModels,
                needle: identity,
            },
            WireExpectation {
                target: SearchTarget::File(
                    "lib/android/vesper-player-kit/src/main/java/io/github/umbrella22/vesper/player/android/VesperPlayerError.kt",
                ),
                needle: quoted,
            },
            WireExpectation {
                target: SearchTarget::File(
                    "lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/Bridge/VesperPlayerError.swift",
                ),
                needle: swift_enum_case,
            },
            WireExpectation {
                target: SearchTarget::File("crates/model/player-model/src/error.rs"),
                needle: camel_to_rust_variant,
            },
            WireExpectation {
                target: SearchTarget::Tree("crates/ffi/player-ffi/src/c_api"),
                needle: camel_to_rust_variant,
            },
            WireExpectation {
                target: SearchTarget::Tree("crates/ffi/player-ffi-ios/src"),
                needle: camel_to_rust_variant,
            },
        ],
        output,
    )?;

    let diagnostics_path = "fixtures/contracts/plugin_diagnostics.json";
    let diagnostics = json_array(repository.read_json(diagnostics_path)?, diagnostics_path)?;
    let mut plugin_values = Vec::new();
    for record in diagnostics {
        let record = value_object(&record, diagnostics_path)?;
        require_json_keys(
            record,
            diagnostics_path,
            &[
                "path",
                "pluginName",
                "pluginKind",
                "status",
                "participation",
                "message",
                "capability",
            ],
        )?;
        push_unique(
            &mut plugin_values,
            required_string_ref(record, diagnostics_path, "status")?,
        );
        push_unique(
            &mut plugin_values,
            required_string_ref(record, diagnostics_path, "participation")?,
        );
        let capability = record
            .get("capability")
            .and_then(Value::as_object)
            .ok_or_else(|| contract_drift("expected plugin diagnostic capability object"))?;
        push_unique(
            &mut plugin_values,
            required_string_ref(capability, diagnostics_path, "kind")?,
        );
    }
    check_wire_values(
        repository,
        "plugin diagnostics",
        &plugin_values,
        &[
            WireExpectation {
                target: SearchTarget::File(diagnostics_path),
                needle: quoted,
            },
            WireExpectation {
                target: SearchTarget::FlutterModels,
                needle: identity,
            },
            WireExpectation {
                target: SearchTarget::File("crates/core/player-runtime/src/lib.rs"),
                needle: camel_to_rust_variant,
            },
            WireExpectation {
                target: SearchTarget::Tree("crates/ffi/player-ffi/src/c_api"),
                needle: camel_to_rust_variant,
            },
        ],
        output,
    )?;

    let download_path = "fixtures/contracts/download_task_snapshot.json";
    let download = json_object(repository.read_json(download_path)?, download_path)?;
    require_json_keys(
        &download,
        download_path,
        &[
            "taskId",
            "assetId",
            "source",
            "profile",
            "state",
            "progress",
            "assetIndex",
            "error",
        ],
    )?;
    let source = required_object(&download, download_path, "source")?;
    let profile = required_object(&download, download_path, "profile")?;
    let asset_index = required_object(&download, download_path, "assetIndex")?;
    let streams = required_array(asset_index, download_path, "streams")?;
    let first_stream = streams
        .first()
        .and_then(Value::as_object)
        .ok_or_else(|| contract_drift("expected the download asset index to contain a stream"))?;
    let mut download_values = Vec::new();
    for value in [
        required_string_ref(source, download_path, "contentFormat")?,
        required_string_ref(&download, download_path, "state")?,
        required_string_ref(asset_index, download_path, "contentFormat")?,
        required_string_ref(first_stream, download_path, "kind")?,
    ] {
        push_unique(&mut download_values, value);
    }
    check_wire_values(
        repository,
        "download snapshot",
        &download_values,
        &[
            WireExpectation {
                target: SearchTarget::FlutterModels,
                needle: identity,
            },
            WireExpectation {
                target: SearchTarget::Tree(
                    "lib/android/vesper-player-kit/src/main/java/io/github/umbrella22/vesper/player/android",
                ),
                needle: kotlin_variant,
            },
            WireExpectation {
                target: SearchTarget::Tree("lib/ios/VesperPlayerKit/Sources/VesperPlayerKit"),
                needle: swift_case,
            },
            WireExpectation {
                target: SearchTarget::File("crates/core/player-download/src/download/types.rs"),
                needle: camel_to_rust_variant,
            },
            WireExpectation {
                target: SearchTarget::Tree("crates/ffi/player-ffi-ios/src"),
                needle: camel_to_rust_variant,
            },
        ],
        output,
    )?;

    let mut download_output_values = Vec::new();
    if let Some(value) = profile
        .get("targetOutputFormat")
        .filter(|value| !value.is_null())
        .and_then(Value::as_str)
    {
        download_output_values.push(value.to_owned());
    }
    check_wire_values(
        repository,
        "download output format",
        &download_output_values,
        &[
            WireExpectation {
                target: SearchTarget::File(
                    "lib/flutter/vesper_player_platform_interface/lib/src/download_models.dart",
                ),
                needle: identity,
            },
            WireExpectation {
                target: SearchTarget::Tree(
                    "lib/android/vesper-player-kit/src/main/java/io/github/umbrella22/vesper/player/android",
                ),
                needle: kotlin_variant,
            },
            WireExpectation {
                target: SearchTarget::Tree("lib/ios/VesperPlayerKit/Sources/VesperPlayerKit"),
                needle: swift_case,
            },
            WireExpectation {
                target: SearchTarget::File("crates/plugin/player-plugin/src/processor.rs"),
                needle: camel_to_rust_variant,
            },
            WireExpectation {
                target: SearchTarget::Tree("crates/ffi/player-ffi-ios/src"),
                needle: camel_to_rust_variant,
            },
        ],
        output,
    )?;

    let system_path = "fixtures/contracts/system_playback_configuration.json";
    let system = json_object(repository.read_json(system_path)?, system_path)?;
    require_json_keys(
        &system,
        system_path,
        &[
            "enabled",
            "backgroundMode",
            "showSystemControls",
            "showSeekActions",
            "metadata",
            "controls",
        ],
    )?;
    let controls = required_object(&system, system_path, "controls")?;
    let buttons = required_array(controls, system_path, "compactButtons")?;
    let mut system_values = Vec::new();
    push_unique(
        &mut system_values,
        required_string_ref(&system, system_path, "backgroundMode")?,
    );
    for button in buttons {
        let button = value_object(button, system_path)?;
        push_unique(
            &mut system_values,
            required_string_ref(button, system_path, "kind")?,
        );
    }
    check_wire_values(
        repository,
        "system playback",
        &system_values,
        &[
            WireExpectation {
                target: SearchTarget::FlutterModels,
                needle: identity,
            },
            WireExpectation {
                target: SearchTarget::File(
                    "lib/android/vesper-player-kit/src/main/java/io/github/umbrella22/vesper/player/android/PlayerBridge.kt",
                ),
                needle: kotlin_variant,
            },
            WireExpectation {
                target: SearchTarget::File(
                    "lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/Bridge/PlayerBridgeSystemPlaybackModels.swift",
                ),
                needle: swift_case,
            },
        ],
        output,
    )?;

    check_wire_values(
        repository,
        "external playback",
        &strings(&[
            "cast",
            "dlna",
            "auto",
            "always",
            "never",
            "hls",
            "routeConnected",
            "routeDisconnected",
            "loaded",
            "playing",
            "paused",
            "stopped",
            "suspended",
            "error",
            "discoveryDiagnostic",
        ]),
        &[
            WireExpectation {
                target: SearchTarget::FlutterModels,
                needle: identity,
            },
            WireExpectation {
                target: SearchTarget::File(
                    "lib/android/vesper-player-kit-external-playback/src/main/java/io/github/umbrella22/vesper/player/android/external/VesperExternalPlaybackModels.kt",
                ),
                needle: kotlin_variant,
            },
            WireExpectation {
                target: SearchTarget::File(
                    "lib/flutter/vesper_player_external_playback/android/src/main/kotlin/io/github/umbrella22/vesper/player/flutter/externalplayback/VesperPlayerExternalPlaybackPlugin.kt",
                ),
                needle: identity,
            },
        ],
        output,
    )?;
    check_wire_values(
        repository,
        "external fallback default",
        &strings(&["mpegTs"]),
        &[
            WireExpectation {
                target: SearchTarget::FlutterModels,
                needle: identity,
            },
            WireExpectation {
                target: SearchTarget::File(
                    "lib/android/vesper-player-kit-external-playback/src/main/java/io/github/umbrella22/vesper/player/android/external/VesperExternalPlaybackModels.kt",
                ),
                needle: kotlin_variant,
            },
        ],
        output,
    )?;
    check_wire_values(
        repository,
        "external playback result status",
        &strings(&["success", "unavailable", "unsupported", "failed"]),
        &[
            WireExpectation {
                target: SearchTarget::FlutterModels,
                needle: identity,
            },
            WireExpectation {
                target: SearchTarget::File(
                    "lib/flutter/vesper_player_external_playback/android/src/main/kotlin/io/github/umbrella22/vesper/player/flutter/externalplayback/VesperPlayerExternalPlaybackPlugin.kt",
                ),
                needle: identity,
            },
            WireExpectation {
                target: SearchTarget::File(
                    "lib/android/vesper-player-kit-external-playback/src/main/java/io/github/umbrella22/vesper/player/android/external/VesperExternalPlaybackModels.kt",
                ),
                needle: kotlin_variant,
            },
        ],
        output,
    )?;
    output.push("DTO contract drift check passed.".to_owned());
    Ok(())
}

pub fn verify_binary_library_names(root: &Path) -> Result<(), ContractError> {
    verify_binary_library_names_in_repository(&Repository::new(root))
}

fn verify_binary_library_names_in_repository(
    repository: &Repository<'_>,
) -> Result<(), ContractError> {
    let mut failures = Vec::new();
    for manifest in repository.files_under("crates")? {
        if manifest.file_name().and_then(|value| value.to_str()) != Some("Cargo.toml") {
            continue;
        }
        let display = relative_display(repository.root, &manifest);
        let source = read_regular_text(&manifest, &display)?;
        let parsed: toml::Value = toml::from_str(&source)
            .map_err(|error| contract_drift(format!("failed to parse {display}: {error}")))?;
        let Some(lib) = parsed.get("lib").and_then(toml::Value::as_table) else {
            continue;
        };
        let has_binary = lib
            .get("crate-type")
            .and_then(toml::Value::as_array)
            .is_some_and(|types| {
                types
                    .iter()
                    .any(|value| matches!(value.as_str(), Some("cdylib" | "staticlib")))
            });
        if !has_binary {
            continue;
        }
        match lib.get("name").and_then(toml::Value::as_str) {
            None => failures.push(format!("{display}: missing explicit [lib] name")),
            Some(name) if !name.starts_with("vesper_") => failures.push(format!(
                "{display}: binary [lib] name must start with vesper_: {name}"
            )),
            Some(_) => {}
        }
    }

    let binary_reference =
        Regex::new(r"libplayer_[A-Za-z0-9_./$(){}-]*\.(?:so|dylib|a)|-lplayer_[A-Za-z0-9_]+")
            .map_err(|error| {
                ContractError::Storage(format!("invalid binary-name regex: {error}"))
            })?;
    let mut reference_matches = Vec::new();
    for relative in [
        "lib/android",
        "lib/ios",
        "lib/flutter",
        "examples/android-compose-host",
        "examples/ios-swift-host",
        "examples/flutter-host",
        "scripts/ios",
    ] {
        for path in repository.files_under(relative)? {
            if has_excluded_distribution_component(repository.root, &path) {
                continue;
            }
            collect_line_matches(
                repository.root,
                &path,
                &binary_reference,
                &mut reference_matches,
            )?;
        }
    }
    for relative in ["README.md", "README.zh-CN.md"] {
        let path = repository.path(relative);
        collect_line_matches(
            repository.root,
            &path,
            &binary_reference,
            &mut reference_matches,
        )?;
    }
    if !reference_matches.is_empty() {
        failures.push(format!(
            "Found mobile distribution references to libplayer_* binaries:\n{}",
            reference_matches.join("\n")
        ));
    }

    let mut binary_files = Vec::new();
    for relative in [
        "lib/android",
        "lib/ios",
        "lib/flutter",
        "examples/android-compose-host",
        "examples/ios-swift-host",
        "examples/flutter-host",
    ] {
        for path in repository.files_under(relative)? {
            if has_excluded_distribution_component(repository.root, &path) {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name.starts_with("libplayer_")
                && ["so", "dylib", "a"].contains(
                    &path
                        .extension()
                        .and_then(|value| value.to_str())
                        .unwrap_or(""),
                )
            {
                binary_files.push(relative_display(repository.root, &path));
            }
        }
    }
    binary_files.sort();
    if !binary_files.is_empty() {
        failures.push(format!(
            "Found mobile distribution binary files using libplayer_* names:\n{}",
            binary_files.join("\n")
        ));
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(ContractError::Drift(format!(
            "{}\nBinary library naming verification failed with {} issue(s).",
            failures.join("\n"),
            failures.len()
        )))
    }
}

fn check_wire_values(
    repository: &Repository<'_>,
    label: &str,
    values: &[String],
    expectations: &[WireExpectation],
    output: &mut Vec<String>,
) -> Result<(), ContractError> {
    for value in values {
        for expectation in expectations {
            let needle = (expectation.needle)(value);
            match expectation.target {
                SearchTarget::FlutterModels => {
                    if !repository.flutter_models_contain(&needle)? {
                        return Err(contract_drift(format!(
                            "expected Flutter platform interface models to contain {needle:?}"
                        )));
                    }
                }
                SearchTarget::File(path) => require_text(repository, path, &needle)?,
                SearchTarget::Tree(path) => require_text_in_tree(repository, path, &needle)?,
            }
        }
    }
    output.push(format!("checked {label}: {}", values.join(", ")));
    Ok(())
}

fn require_text(
    repository: &Repository<'_>,
    path: &str,
    needle: &str,
) -> Result<(), ContractError> {
    if repository.read_text(path)?.contains(needle) {
        Ok(())
    } else {
        Err(contract_drift(format!(
            "expected {path} to contain {needle:?}"
        )))
    }
}

fn require_text_in_tree(
    repository: &Repository<'_>,
    path: &str,
    needle: &str,
) -> Result<(), ContractError> {
    if repository.tree_contains(path, needle)? {
        Ok(())
    } else {
        Err(contract_drift(format!(
            "expected {path} tree to contain {needle:?}"
        )))
    }
}

fn require_flutter_models_text(
    repository: &Repository<'_>,
    needle: &str,
) -> Result<(), ContractError> {
    if repository.flutter_models_contain(needle)? {
        Ok(())
    } else {
        Err(contract_drift(format!(
            "expected Flutter platform interface models to contain {needle:?}"
        )))
    }
}

fn require_exact_json_keys(
    repository: &Repository<'_>,
    path: &str,
    expected: &[&str],
) -> Result<(), ContractError> {
    let object = json_object(repository.read_json(path)?, path)?;
    let mut actual = object.keys().cloned().collect::<Vec<_>>();
    let mut expected = expected
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    if actual == expected {
        Ok(())
    } else {
        Err(contract_drift(format!(
            "expected {path} JSON keys {expected:?}, found {actual:?}"
        )))
    }
}

fn require_json_keys(
    object: &Map<String, Value>,
    path: &str,
    keys: &[&str],
) -> Result<(), ContractError> {
    for key in keys {
        if !object.contains_key(*key) {
            return Err(contract_drift(format!("expected {path} JSON key {key:?}")));
        }
    }
    Ok(())
}

fn json_object(value: Value, path: &str) -> Result<Map<String, Value>, ContractError> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| contract_drift(format!("expected {path} to contain a JSON object")))
}

fn json_array(value: Value, path: &str) -> Result<Vec<Value>, ContractError> {
    value
        .as_array()
        .cloned()
        .ok_or_else(|| contract_drift(format!("expected {path} to contain a JSON array")))
}

fn value_object<'a>(value: &'a Value, path: &str) -> Result<&'a Map<String, Value>, ContractError> {
    value
        .as_object()
        .ok_or_else(|| contract_drift(format!("expected an object in {path}")))
}

fn required_object<'a>(
    object: &'a Map<String, Value>,
    path: &str,
    key: &str,
) -> Result<&'a Map<String, Value>, ContractError> {
    object
        .get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| contract_drift(format!("expected {path} JSON object {key:?}")))
}

fn required_array<'a>(
    object: &'a Map<String, Value>,
    path: &str,
    key: &str,
) -> Result<&'a [Value], ContractError> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| contract_drift(format!("expected {path} JSON array {key:?}")))
}

fn required_string(
    object: &Map<String, Value>,
    path: &str,
    key: &str,
) -> Result<String, ContractError> {
    required_string_ref(object, path, key).map(str::to_owned)
}

fn required_string_ref<'a>(
    object: &'a Map<String, Value>,
    path: &str,
    key: &str,
) -> Result<&'a str, ContractError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| contract_drift(format!("expected {path} JSON string {key:?}")))
}

fn collect_regular_files(root: &Path) -> Result<Vec<PathBuf>, ContractError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        ContractError::Storage(format!("failed to inspect '{}': {error}", root.display()))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(ContractError::Storage(format!(
            "contract scan root '{}' is not a directory",
            root.display()
        )));
    }
    let mut directories = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| {
            ContractError::Storage(format!(
                "failed to read directory '{}': {error}",
                directory.display()
            ))
        })? {
            let entry = entry.map_err(|error| {
                ContractError::Storage(format!(
                    "failed to read an entry under '{}': {error}",
                    directory.display()
                ))
            })?;
            let file_type = entry.file_type().map_err(|error| {
                ContractError::Storage(format!(
                    "failed to inspect '{}': {error}",
                    entry.path().display()
                ))
            })?;
            if file_type.is_dir() {
                if is_generated_directory_name(&entry.file_name()) {
                    continue;
                }
                directories.push(entry.path());
            } else if file_type.is_file() {
                files.push(entry.path());
                if files.len() > MAX_CONTRACT_SCAN_FILES {
                    return Err(ContractError::Storage(format!(
                        "contract scan under '{}' exceeds {MAX_CONTRACT_SCAN_FILES} files",
                        root.display()
                    )));
                }
            }
        }
    }
    files.sort();
    Ok(files)
}

fn is_generated_directory_name(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some("build" | ".build" | ".gradle" | ".dart_tool" | "target")
    )
}

fn read_regular_text(path: &Path, label: &str) -> Result<String, ContractError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| ContractError::Storage(format!("failed to inspect {label}: {error}")))?;
    if !metadata.file_type().is_file() {
        return Err(ContractError::Storage(format!(
            "{label} is not a regular non-symlink file"
        )));
    }
    if metadata.len() > MAX_CONTRACT_FILE_BYTES as u64 {
        return Err(ContractError::Storage(format!(
            "{label} exceeds {MAX_CONTRACT_FILE_BYTES} bytes"
        )));
    }
    let file = File::open(path)
        .map_err(|error| ContractError::Storage(format!("failed to open {label}: {error}")))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_CONTRACT_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| ContractError::Storage(format!("failed to read {label}: {error}")))?;
    if bytes.len() > MAX_CONTRACT_FILE_BYTES {
        return Err(ContractError::Storage(format!(
            "{label} exceeds {MAX_CONTRACT_FILE_BYTES} bytes"
        )));
    }
    String::from_utf8(bytes)
        .map_err(|error| ContractError::Storage(format!("{label} is not UTF-8: {error}")))
}

fn regular_file_contains(path: &Path, needle: &str) -> Result<bool, ContractError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        ContractError::Storage(format!("failed to inspect '{}': {error}", path.display()))
    })?;
    if metadata.len() > MAX_CONTRACT_FILE_BYTES as u64 {
        return Err(ContractError::Storage(format!(
            "contract source '{}' exceeds {MAX_CONTRACT_FILE_BYTES} bytes",
            path.display()
        )));
    }
    let bytes = fs::read(path).map_err(|error| {
        ContractError::Storage(format!("failed to read '{}': {error}", path.display()))
    })?;
    Ok(std::str::from_utf8(&bytes).is_ok_and(|text| text.contains(needle)))
}

fn collect_line_matches(
    repository_root: &Path,
    path: &Path,
    expression: &Regex,
    matches: &mut Vec<String>,
) -> Result<(), ContractError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        ContractError::Storage(format!("failed to inspect '{}': {error}", path.display()))
    })?;
    if metadata.len() > MAX_CONTRACT_FILE_BYTES as u64 {
        return Ok(());
    }
    let bytes = fs::read(path).map_err(|error| {
        ContractError::Storage(format!("failed to read '{}': {error}", path.display()))
    })?;
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Ok(());
    };
    let display = relative_display(repository_root, path);
    for (index, line) in text.lines().enumerate() {
        if expression.is_match(line) {
            matches.push(format!("{display}:{}:{line}", index + 1));
        }
    }
    Ok(())
}

fn has_excluded_distribution_component(repository_root: &Path, path: &Path) -> bool {
    path.strip_prefix(repository_root)
        .unwrap_or(path)
        .components()
        .any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some("build" | ".build" | ".gradle" | ".dart_tool" | "target")
            )
        })
}

fn relative_display(repository_root: &Path, path: &Path) -> String {
    path.strip_prefix(repository_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn contract_drift(message: impl Into<String>) -> ContractError {
    ContractError::Drift(format!("Contract drift: {}", message.into()))
}

fn identity(value: &str) -> String {
    value.to_owned()
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

fn swift_enum_case(value: &str) -> String {
    format!("case {value}")
}

fn swift_case(value: &str) -> String {
    format!(".{value}")
}

fn camel_to_pascal(value: &str) -> String {
    value
        .split('_')
        .map(uppercase_first)
        .collect::<Vec<_>>()
        .join("")
}

fn camel_to_rust_variant(value: &str) -> String {
    let mut separated = String::with_capacity(value.len() + 4);
    let mut previous = None;
    for character in value.chars() {
        if character.is_ascii_uppercase()
            && previous
                .is_some_and(|prior: char| prior.is_ascii_lowercase() || prior.is_ascii_digit())
        {
            separated.push('_');
        }
        separated.push(character);
        previous = Some(character);
    }
    camel_to_pascal(&separated)
}

fn kotlin_variant(value: &str) -> String {
    match value {
        "mpegTs" => "MpegTs".to_owned(),
        "airPlay" => "AirPlay".to_owned(),
        "dlna" => "Dlna".to_owned(),
        "hls" => "Hls".to_owned(),
        _ => camel_to_pascal(value),
    }
}

fn uppercase_first(value: &str) -> String {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    let mut output = first.to_uppercase().collect::<String>();
    output.extend(characters);
    output
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_case_conversions_match_the_previous_contract_tool() {
        assert_eq!(
            camel_to_rust_variant("decoderSupported"),
            "DecoderSupported"
        );
        assert_eq!(camel_to_rust_variant("mpeg_ts"), "MpegTs");
        assert_eq!(kotlin_variant("mpegTs"), "MpegTs");
        assert_eq!(kotlin_variant("dlna"), "Dlna");
        assert_eq!(swift_case("continueAudio"), ".continueAudio");
    }

    #[test]
    fn binary_reference_expression_matches_only_distribution_names() {
        let expression =
            Regex::new(r"libplayer_[A-Za-z0-9_./$(){}-]*\.(?:so|dylib|a)|-lplayer_[A-Za-z0-9_]+")
                .expect("static regex");

        assert!(expression.is_match("jniLibs/libplayer_old.so"));
        assert!(expression.is_match("-lplayer_ffi_ios"));
        assert!(!expression.is_match("player_plugin_loader"));
        assert!(!expression.is_match("libvesper_player_android.so"));
    }
}
