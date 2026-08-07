use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use player_cli::ios_bridge_shim::{
    self, BRIDGE_SHIM_HEADER_FILE, BRIDGE_SHIM_SOURCE_FILE, GeneratedShim,
};
use player_plugin_loader::{
    EmbeddedPluginLocator, EmbeddedPluginRegistry, MAX_EMBEDDED_PLUGIN_REGISTRY_BYTES,
};
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::external_process;

const MAX_BRIDGE_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_RUST_FFI_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARCHIVE_DISCOVERY_DEPTH: usize = 16;
const MAX_ARCHIVE_DISCOVERY_ENTRIES: usize = 4096;
const MAX_DISCOVERED_ARCHIVES: usize = 32;
const MAX_NM_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
const IOS_FFI_ARCHIVE_NAME: &str = "libvesper_player_ffi_ios.a";
const MAX_APP_BUNDLE_ENTRIES: usize = 100_000;
const MAX_APP_BUNDLE_DEPTH: usize = 64;
const MAX_APP_INFO_PLIST_BYTES: u64 = 1024 * 1024;
const MAX_APP_PROFILE_RECORD_BYTES: usize = 4 * 1024;
const MAX_APPLE_TOOL_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_APP_AOT_BINARY_BYTES: u64 = 512 * 1024 * 1024;
const EMBEDDED_PLUGIN_REGISTRY_FILE: &str = "vesper-plugin-registry.json";
const IOS_DEVICE_PLUGIN_TARGET: &str = "aarch64-apple-ios";
const IOS_PLUGIN_ARCHITECTURE: &str = "arm64";

const APP_STORE_FRAMEWORKS: [&str; 7] = [
    "VesperFFmpegAVCodec",
    "VesperFFmpegAVFormat",
    "VesperFFmpegAVUtil",
    "VesperPlayerRemuxFfmpegPlugin",
    "VesperPlayerSourceNormalizerFfmpegPlugin",
    "VesperPlayerDecoderVideoToolboxPlugin",
    "VesperPlayerFrameProcessorDiagnosticPlugin",
];

const APP_STORE_FFMPEG_FRAMEWORKS: [&str; 5] = [
    "VesperFFmpegAVCodec",
    "VesperFFmpegAVFormat",
    "VesperFFmpegAVUtil",
    "VesperPlayerRemuxFfmpegPlugin",
    "VesperPlayerSourceNormalizerFfmpegPlugin",
];

const APP_STORE_PLUGIN_FRAMEWORKS: [&str; 4] = [
    "VesperPlayerRemuxFfmpegPlugin",
    "VesperPlayerSourceNormalizerFfmpegPlugin",
    "VesperPlayerDecoderVideoToolboxPlugin",
    "VesperPlayerFrameProcessorDiagnosticPlugin",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidatedFramework {
    path: PathBuf,
    bundle_identifier: String,
    minimum_os: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FrameworkBundleMetadata {
    bundle_identifier: String,
    minimum_os: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeSignatureMetadata {
    identifier: String,
    team_identifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppleMachOBuildMetadata {
    pub(crate) platform: String,
    pub(crate) minimum_os: String,
}

const FOUNDATION_FRAMEWORK_VALIDATION: &str = r#"
import Darwin
import Foundation

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

let frameworkPath = CommandLine.arguments[1]
let expectedExecutable = CommandLine.arguments[2]
let expectedIdentifier = CommandLine.arguments[3]
guard let bundle = Bundle(path: frameworkPath),
      let info = bundle.infoDictionary,
      !info.isEmpty else {
    fail("Foundation could not read framework metadata: \(frameworkPath)")
}
guard bundle.bundleIdentifier == expectedIdentifier else {
    fail("Foundation read an unexpected bundle identifier: \(frameworkPath)")
}
guard info["CFBundleExecutable"] as? String == expectedExecutable,
      info["CFBundlePackageType"] as? String == "FMWK" else {
    fail("Foundation read invalid framework metadata: \(frameworkPath)")
}
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IosErrorKind {
    Storage,
    Compatibility,
    Conformance,
    Worker,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct IosError {
    kind: IosErrorKind,
    message: String,
}

impl IosError {
    pub(crate) fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: IosErrorKind::Storage,
            message: message.into(),
        }
    }

    pub(crate) fn compatibility(message: impl Into<String>) -> Self {
        Self {
            kind: IosErrorKind::Compatibility,
            message: message.into(),
        }
    }

    pub(crate) fn conformance(message: impl Into<String>) -> Self {
        Self {
            kind: IosErrorKind::Conformance,
            message: message.into(),
        }
    }

    pub(crate) fn worker(message: impl Into<String>) -> Self {
        Self {
            kind: IosErrorKind::Worker,
            message: message.into(),
        }
    }

    pub(crate) const fn kind(&self) -> IosErrorKind {
        self.kind
    }
}

struct BridgePaths {
    root: PathBuf,
    manifest: PathBuf,
    shim_directory: PathBuf,
    header: PathBuf,
    source: PathBuf,
    rust_ffi_source: PathBuf,
    archive_root: PathBuf,
}

impl BridgePaths {
    fn new(root: &Path) -> Self {
        let shim_directory = root.join("lib/ios/VesperPlayerKit/Sources/VesperPlayerKitBridgeShim");
        Self {
            root: root.to_path_buf(),
            manifest: root.join("scripts/ios/bridge-shim/manifest.json"),
            header: shim_directory.join(BRIDGE_SHIM_HEADER_FILE),
            source: shim_directory.join(BRIDGE_SHIM_SOURCE_FILE),
            shim_directory,
            rust_ffi_source: root.join("crates/ffi/player-ffi-ios/src/lib.rs"),
            archive_root: root.join("lib/ios/VesperPlayerKit/Artifacts/rust-player-ffi"),
        }
    }
}

struct IosBridgeLock {
    _file: File,
}

impl IosBridgeLock {
    fn acquire(root: &Path) -> Result<Self, IosError> {
        let canonical_root = fs::canonicalize(root).map_err(|error| {
            IosError::storage(format!(
                "failed to resolve iOS bridge lock root '{}': {error}",
                root.display()
            ))
        })?;
        let root_digest = Sha256::digest(canonical_root.as_os_str().as_encoded_bytes());
        let lock_directory = env::temp_dir().join("vesper-player-cli-locks");
        fs::create_dir_all(&lock_directory).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS bridge lock directory '{}': {error}",
                lock_directory.display()
            ))
        })?;
        let lock_path =
            lock_directory.join(format!("ios-bridge-shim-{}.lock", hex::encode(root_digest)));
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to open iOS bridge lock '{}': {error}",
                    lock_path.display()
                ))
            })?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(IosError::compatibility(format!(
                "another iOS bridge shim command is already active for '{}'",
                root.display()
            ))),
            Err(TryLockError::Error(error)) => Err(IosError::storage(format!(
                "failed to lock iOS bridge shim command for '{}': {error}",
                root.display()
            ))),
        }
    }
}

pub(crate) fn sync_bridge_shim(root: &Path, output: &mut dyn Write) -> Result<(), IosError> {
    let _lock = IosBridgeLock::acquire(root)?;
    let paths = BridgePaths::new(root);
    validate_repository_path(
        root,
        &paths.manifest,
        RepositoryPathKind::File,
        false,
        "bridge manifest",
    )?;
    validate_repository_path(
        root,
        &paths.shim_directory,
        RepositoryPathKind::Directory,
        true,
        "checked-in bridge directory",
    )?;
    validate_repository_path(
        root,
        &paths.header,
        RepositoryPathKind::File,
        true,
        "checked-in bridge header",
    )?;
    validate_repository_path(
        root,
        &paths.source,
        RepositoryPathKind::File,
        true,
        "checked-in bridge source",
    )?;
    let generated = generate_bridge(&paths)?;
    validate_forbidden_download_casts(generated.source())?;

    let current_header = read_optional_bounded_file(
        &paths.header,
        MAX_BRIDGE_FILE_BYTES,
        "checked-in bridge header",
    )?;
    let current_source = read_optional_bounded_file(
        &paths.source,
        MAX_BRIDGE_FILE_BYTES,
        "checked-in bridge source",
    )?;
    if current_header.as_deref() == Some(generated.header().as_bytes())
        && current_source.as_deref() == Some(generated.source().as_bytes())
    {
        writeln!(output, "VesperPlayerKit bridge shim synchronized.").map_err(output_error)?;
        return output.flush().map_err(output_error);
    }

    let parent = require_regular_directory_parent(root, &paths.shim_directory)?;
    let staging = tempfile::Builder::new()
        .prefix(".vesper-ios-bridge-stage-")
        .tempdir_in(&parent)
        .map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS bridge staging directory in '{}': {error}",
                parent.display()
            ))
        })?;
    ios_bridge_shim::write_generated_directory(staging.path(), &generated)
        .map_err(|error| IosError::storage(error.to_string()))?;
    sync_generated_files(staging.path())?;

    let deferral = external_process::InterruptDeferral::start("iOS bridge shim promotion")
        .map_err(|error| IosError::worker(error.to_string()))?;
    if deferral.is_cancelled() {
        let _ = deferral.finish();
        return Err(IosError::worker("iOS bridge shim promotion was cancelled"));
    }
    promote_generated_directory(staging, &paths.shim_directory)?;
    let _ = deferral.finish();

    writeln!(output, "VesperPlayerKit bridge shim synchronized.").map_err(output_error)?;
    output.flush().map_err(output_error)
}

pub(crate) fn verify_bridge_shim(
    root: &Path,
    archive: Option<&Path>,
    output: &mut dyn Write,
) -> Result<(), IosError> {
    let _lock = IosBridgeLock::acquire(root)?;
    let paths = BridgePaths::new(root);
    validate_repository_path(
        root,
        &paths.manifest,
        RepositoryPathKind::File,
        false,
        "bridge manifest",
    )?;
    validate_repository_path(
        root,
        &paths.shim_directory,
        RepositoryPathKind::Directory,
        true,
        "checked-in bridge directory",
    )?;
    validate_repository_path(
        root,
        &paths.header,
        RepositoryPathKind::File,
        true,
        "checked-in bridge header",
    )?;
    validate_repository_path(
        root,
        &paths.source,
        RepositoryPathKind::File,
        true,
        "checked-in bridge source",
    )?;
    validate_repository_path(
        root,
        &paths.rust_ffi_source,
        RepositoryPathKind::File,
        false,
        "player-ffi-ios Rust source",
    )?;
    let generated = generate_bridge(&paths)?;
    let current_header = read_optional_bounded_file(
        &paths.header,
        MAX_BRIDGE_FILE_BYTES,
        "checked-in bridge header",
    )?;
    if current_header.as_deref() != Some(generated.header().as_bytes()) {
        write_unified_diff(
            output,
            Path::new("generated/include/VesperPlayerKitBridgeShim.h"),
            generated.header(),
            &paths.header,
            &String::from_utf8_lossy(current_header.as_deref().unwrap_or_default()),
        )?;
        return Err(IosError::conformance(
            "VesperPlayerKitBridgeShim.h is out of sync with the Rust generator.\nRun: ./scripts/vesper ios sync-bridge-shim",
        ));
    }

    let current_source = read_optional_bounded_file(
        &paths.source,
        MAX_BRIDGE_FILE_BYTES,
        "checked-in bridge source",
    )?;
    if current_source.as_deref() != Some(generated.source().as_bytes()) {
        write_unified_diff(
            output,
            Path::new("generated/VesperPlayerKitBridgeShim.c"),
            generated.source(),
            &paths.source,
            &String::from_utf8_lossy(current_source.as_deref().unwrap_or_default()),
        )?;
        return Err(IosError::conformance(
            "VesperPlayerKitBridgeShim.c is out of sync with the Rust generator.\nRun: ./scripts/vesper ios sync-bridge-shim",
        ));
    }

    run_clang_syntax_check(&paths)?;
    validate_forbidden_download_casts(generated.source())?;
    validate_rust_source_exports(&paths.rust_ffi_source, generated.required_ffi_symbols())?;

    let archives = resolve_archives(&paths, archive)?;
    if archives.is_empty() {
        writeln!(
            output,
            "No Rust FFI archive found; source-level bridge symbol verification only."
        )
        .map_err(output_error)?;
    } else {
        for archive in archives {
            validate_archive_exports(&archive, generated.required_ffi_symbols())?;
        }
    }
    writeln!(output, "VesperPlayerKit bridge shim is valid.").map_err(output_error)?;
    output.flush().map_err(output_error)
}

pub(crate) fn verify_app_store_layout(
    app_path: &Path,
    verify_signatures: bool,
    output: &mut dyn Write,
) -> Result<(), IosError> {
    let preflight = external_process::InterruptDeferral::start("iOS App Store layout preflight")
        .map_err(map_external_process_error)?;
    require_app_bundle_directory(app_path)?;
    scan_app_bundle(app_path, &preflight)?;
    scan_flutter_aot_markers(app_path, &preflight)?;

    if preflight.finish() {
        return Err(app_store_cancelled_error());
    }

    let profile_hash = verify_embedded_optional_framework_contract(app_path, verify_signatures)?;

    writeln!(
        output,
        "Verified App Store-compatible optional iOS framework layout:"
    )
    .map_err(output_error)?;
    writeln!(output, "  {}", app_path.display()).map_err(output_error)?;
    writeln!(output, "  FFmpeg profile hash: {profile_hash}").map_err(output_error)?;
    for framework_name in APP_STORE_FRAMEWORKS {
        writeln!(output, "  {framework_name}.framework").map_err(output_error)?;
    }
    output.flush().map_err(output_error)
}

pub(crate) fn verify_embedded_optional_framework_contract(
    app_path: &Path,
    verify_signatures: bool,
) -> Result<String, IosError> {
    require_app_bundle_directory(app_path)?;
    let frameworks_directory = app_path.join("Frameworks");
    require_bundle_directory(
        &frameworks_directory,
        "App bundle is missing its Frameworks directory",
    )?;
    let legacy_runtime = frameworks_directory.join("VesperPlayerFfmpegRuntime.framework");
    if path_exists_without_following_symlinks(&legacy_runtime)? {
        return Err(IosError::conformance(format!(
            "The legacy umbrella FFmpeg runtime framework is not distributable:\n  {}",
            legacy_runtime.display()
        )));
    }

    let mut validated_frameworks = BTreeMap::new();
    for framework_name in APP_STORE_FRAMEWORKS {
        let framework = validate_app_store_framework(&frameworks_directory, framework_name)?;
        validated_frameworks.insert(framework_name.to_owned(), framework);
    }
    let embedded_plugins = validate_embedded_plugin_frameworks(&frameworks_directory)?;
    for framework_name in APP_STORE_PLUGIN_FRAMEWORKS {
        if !embedded_plugins.contains_key(framework_name) {
            return Err(IosError::conformance(format!(
                "Required iOS plugin framework is missing {EMBEDDED_PLUGIN_REGISTRY_FILE}:\n  {}/{framework_name}.framework",
                frameworks_directory.display()
            )));
        }
    }
    for (name, framework) in &embedded_plugins {
        match validated_frameworks.get(name) {
            Some(validated) if validated != framework => {
                return Err(IosError::conformance(format!(
                    "Embedded iOS plugin framework metadata changed during verification: {}",
                    framework.path.display()
                )));
            }
            Some(_) => {}
            None => {
                validated_frameworks.insert(name.clone(), framework.clone());
            }
        }
    }
    let profile_hash = validate_app_store_profile_hashes(&frameworks_directory)?;
    verify_app_does_not_link_optional_frameworks(app_path, embedded_plugins.keys())?;
    if verify_signatures {
        verify_app_store_signatures(app_path, &validated_frameworks)?;
    }
    Ok(profile_hash)
}

fn validate_embedded_plugin_frameworks(
    frameworks_directory: &Path,
) -> Result<BTreeMap<String, ValidatedFramework>, IosError> {
    let fragments = discover_embedded_plugin_fragments(frameworks_directory)?;
    let registry = EmbeddedPluginRegistry::parse_fragments(
        fragments.iter().map(|fragment| fragment.json.as_slice()),
        IOS_DEVICE_PLUGIN_TARGET,
        IOS_PLUGIN_ARCHITECTURE,
    )
    .map_err(|error| {
        IosError::conformance(format!(
            "Embedded iOS plugin registry fragments are invalid: {error}"
        ))
    })?;

    let mut frameworks = BTreeMap::new();
    for fragment in &fragments {
        let parsed = EmbeddedPluginRegistry::parse(
            &fragment.json,
            IOS_DEVICE_PLUGIN_TARGET,
            IOS_PLUGIN_ARCHITECTURE,
        )
        .map_err(|error| {
            IosError::conformance(format!(
                "Embedded iOS plugin registry fragment '{}' is invalid: {error}",
                fragment.path.display()
            ))
        })?;
        let bundle_identifier =
            validate_registry_fragment_binding(&parsed, &fragment.framework_name, &fragment.path)?;
        let framework = validate_framework_bundle(
            frameworks_directory,
            &fragment.framework_name,
            "Missing embedded plugin framework",
        )?;
        let metadata = FrameworkBundleMetadata {
            bundle_identifier: framework.bundle_identifier.clone(),
            minimum_os: framework.minimum_os.clone(),
        };
        validate_registry_framework_metadata(
            &parsed,
            &bundle_identifier,
            &metadata,
            &fragment.path,
        )?;
        frameworks.insert(fragment.framework_name.clone(), framework);
    }

    if frameworks.len() != registry.artifacts().len() {
        return Err(IosError::conformance(format!(
            "Embedded iOS plugin registry describes {} artifacts in {} framework fragments",
            registry.artifacts().len(),
            frameworks.len()
        )));
    }
    Ok(frameworks)
}

#[derive(Debug)]
struct EmbeddedPluginFragment {
    framework_name: String,
    path: PathBuf,
    json: Vec<u8>,
}

fn discover_embedded_plugin_fragments(
    frameworks_directory: &Path,
) -> Result<Vec<EmbeddedPluginFragment>, IosError> {
    let entries = fs::read_dir(frameworks_directory).map_err(|error| {
        IosError::storage(format!(
            "failed to scan embedded iOS plugin frameworks '{}': {error}",
            frameworks_directory.display()
        ))
    })?;
    let mut fragments = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            IosError::storage(format!(
                "failed to read an embedded iOS framework entry under '{}': {error}",
                frameworks_directory.display()
            ))
        })?;
        let framework_path = entry.path();
        let metadata = fs::symlink_metadata(&framework_path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect embedded iOS framework '{}': {error}",
                framework_path.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            continue;
        }
        let Some(file_name) = framework_path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let Some(framework_name) = file_name.strip_suffix(".framework") else {
            continue;
        };
        if framework_name.is_empty() {
            continue;
        }

        let path = framework_path.join(EMBEDDED_PLUGIN_REGISTRY_FILE);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(IosError::storage(format!(
                    "failed to inspect embedded iOS plugin registry fragment '{}': {error}",
                    path.display()
                )));
            }
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(_) => {
                return Err(IosError::conformance(format!(
                    "Embedded iOS plugin registry fragment is not a regular non-symlink file: {}",
                    path.display()
                )));
            }
        }
        let json = read_bounded_file(
            &path,
            MAX_EMBEDDED_PLUGIN_REGISTRY_BYTES,
            "embedded iOS plugin registry fragment",
        )?;
        fragments.push(EmbeddedPluginFragment {
            framework_name: framework_name.to_owned(),
            path,
            json,
        });
    }
    fragments.sort_by(|left, right| left.framework_name.cmp(&right.framework_name));
    Ok(fragments)
}

fn validate_registry_fragment_binding(
    registry: &EmbeddedPluginRegistry,
    framework_name: &str,
    fragment_path: &Path,
) -> Result<String, IosError> {
    let [artifact] = registry.artifacts() else {
        return Err(IosError::conformance(format!(
            "Embedded iOS plugin registry fragment must describe exactly one framework: {}",
            fragment_path.display()
        )));
    };
    let EmbeddedPluginLocator::AppleFramework {
        name,
        bundle_identifier,
    } = artifact.locator()
    else {
        return Err(IosError::conformance(format!(
            "Embedded iOS plugin registry fragment does not describe an Apple framework: {}",
            fragment_path.display()
        )));
    };
    if name != framework_name {
        return Err(IosError::conformance(format!(
            "Embedded iOS plugin registry locator does not match its containing framework: {}\n  locator:   {name}.framework\n  container: {framework_name}.framework",
            fragment_path.display()
        )));
    }
    Ok(bundle_identifier.clone())
}

fn validate_registry_framework_metadata(
    registry: &EmbeddedPluginRegistry,
    declared_bundle_identifier: &str,
    actual: &FrameworkBundleMetadata,
    fragment_path: &Path,
) -> Result<(), IosError> {
    if declared_bundle_identifier != actual.bundle_identifier {
        return Err(IosError::conformance(format!(
            "Embedded iOS plugin registry bundle identifier does not match Info.plist: {}\n  registry: {}\n  plist:    {}",
            fragment_path.display(),
            declared_bundle_identifier,
            actual.bundle_identifier
        )));
    }
    if registry.minimum_os() != Some(actual.minimum_os.as_str()) {
        return Err(IosError::conformance(format!(
            "Embedded iOS plugin registry minimum OS does not match Info.plist: {}\n  registry: {}\n  plist:    {}",
            fragment_path.display(),
            registry.minimum_os().unwrap_or("<missing>"),
            actual.minimum_os
        )));
    }
    Ok(())
}

fn require_app_bundle_directory(path: &Path) -> Result<(), IosError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::storage(format!(
            "App bundle is unavailable '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(IosError::storage(format!(
            "App bundle '{}' is not a regular non-symlink directory",
            path.display()
        )));
    }
    Ok(())
}

fn require_bundle_directory(path: &Path, message: &str) -> Result<(), IosError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(metadata) if metadata.file_type().is_symlink() => Err(IosError::conformance(format!(
            "App Store bundle paths must not contain symlinks: {}",
            path.display()
        ))),
        Ok(_) => Err(IosError::conformance(format!(
            "{message}: {} is not a directory",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(IosError::conformance(
            format!("{message}: {}", path.display()),
        )),
        Err(error) => Err(IosError::storage(format!(
            "failed to inspect app bundle directory '{}': {error}",
            path.display()
        ))),
    }
}

fn require_bundle_file(path: &Path, message: &str, maximum_bytes: u64) -> Result<(), IosError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => {
            if metadata.len() > maximum_bytes {
                return Err(IosError::storage(format!(
                    "App Store bundle file '{}' exceeds {maximum_bytes} bytes",
                    path.display()
                )));
            }
            Ok(())
        }
        Ok(metadata) if metadata.file_type().is_symlink() => Err(IosError::conformance(format!(
            "App Store bundle paths must not contain symlinks: {}",
            path.display()
        ))),
        Ok(_) => Err(IosError::conformance(format!(
            "{message}: {} is not a regular file",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(IosError::conformance(
            format!("{message}: {}", path.display()),
        )),
        Err(error) => Err(IosError::storage(format!(
            "failed to inspect app bundle file '{}': {error}",
            path.display()
        ))),
    }
}

fn path_exists_without_following_symlinks(path: &Path) -> Result<bool, IosError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(IosError::conformance(format!(
            "App Store bundle paths must not contain symlinks: {}",
            path.display()
        ))),
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(IosError::storage(format!(
            "failed to inspect app bundle path '{}': {error}",
            path.display()
        ))),
    }
}

fn scan_app_bundle(
    app_path: &Path,
    cancellation: &external_process::InterruptDeferral,
) -> Result<(), IosError> {
    let frameworks_directory = app_path.join("Frameworks");
    let fixture_pattern = app_store_fixture_path_pattern()?;
    let mut pending = VecDeque::from([(app_path.to_path_buf(), 0_usize)]);
    let mut entries = 0_usize;
    while let Some((directory, depth)) = pending.pop_front() {
        check_app_store_cancellation(cancellation)?;
        if depth > MAX_APP_BUNDLE_DEPTH {
            return Err(IosError::storage(format!(
                "App bundle traversal exceeds depth {MAX_APP_BUNDLE_DEPTH}: {}",
                directory.display()
            )));
        }
        let children = fs::read_dir(&directory).map_err(|error| {
            IosError::storage(format!(
                "failed to read app bundle directory '{}': {error}",
                directory.display()
            ))
        })?;
        for child in children {
            check_app_store_cancellation(cancellation)?;
            let child = child.map_err(|error| {
                IosError::storage(format!(
                    "failed to read an app bundle entry under '{}': {error}",
                    directory.display()
                ))
            })?;
            entries = entries.saturating_add(1);
            if entries > MAX_APP_BUNDLE_ENTRIES {
                return Err(IosError::storage(format!(
                    "App bundle traversal exceeds {MAX_APP_BUNDLE_ENTRIES} entries: {}",
                    app_path.display()
                )));
            }
            let path = child.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect app bundle entry '{}': {error}",
                    path.display()
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(IosError::conformance(format!(
                    "App Store bundle paths must not contain symlinks: {}",
                    path.display()
                )));
            }

            let relative = path.strip_prefix(app_path).map_err(|_| {
                IosError::worker(format!(
                    "app bundle traversal escaped '{}': {}",
                    app_path.display(),
                    path.display()
                ))
            })?;
            let relative_text = relative_path_text(relative);
            if metadata.file_type().is_file() && fixture_pattern.is_match(&relative_text) {
                return Err(IosError::conformance(format!(
                    "Release directory contains test fixture resources: {}\n  {relative_text}",
                    app_path.display()
                )));
            }

            if let Ok(framework_relative) = path.strip_prefix(&frameworks_directory) {
                let name = path.file_name().unwrap_or_else(|| OsStr::new(""));
                if metadata.file_type().is_file() && is_standalone_dylib_name(name) {
                    return Err(IosError::conformance(format!(
                        "App bundles validated by this release gate must not ship standalone dylibs:\n  {}",
                        path.display()
                    )));
                }
                if metadata.file_type().is_dir()
                    && framework_relative.components().count() >= 2
                    && is_nested_framework_directory_name(name)
                {
                    return Err(IosError::conformance(format!(
                        "App bundles must not contain nested framework directories:\n  {}",
                        path.display()
                    )));
                }
            }

            if metadata.file_type().is_dir() {
                pending.push_back((path, depth + 1));
            }
        }
    }
    Ok(())
}

fn is_standalone_dylib_name(name: &OsStr) -> bool {
    name.to_string_lossy().contains(".dylib")
}

fn is_nested_framework_directory_name(name: &OsStr) -> bool {
    name == OsStr::new("Frameworks") || name.to_string_lossy().ends_with(".framework")
}

fn relative_path_text(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn app_store_fixture_path_pattern() -> Result<&'static Regex, IosError> {
    static PATTERN: OnceLock<Result<Regex, String>> = OnceLock::new();
    match PATTERN.get_or_init(|| {
        Regex::new(
            r"(?i)(^|/)(subtitle_contract|test[-_]?fixtures?|test[-_]?assets?|testdata)(/|$)|(^|/)fixtures/(contracts|media)(/|$)|(^|/)(tiny-aac\.m4a|tiny-h264-aac(-mediacodec)?\.m4v)$",
        )
        .map_err(|error| error.to_string())
    }) {
        Ok(pattern) => Ok(pattern),
        Err(error) => Err(IosError::worker(format!(
            "invalid App Store fixture path validator: {error}"
        ))),
    }
}

fn scan_flutter_aot_markers(
    app_path: &Path,
    cancellation: &external_process::InterruptDeferral,
) -> Result<(), IosError> {
    const MARKERS: [&[u8]; 6] = [
        b"assets/subtitle_contract",
        b"fixtures/contracts",
        b"fixtures/media",
        b"tiny-aac.m4a",
        b"tiny-h264-aac.m4v",
        b"tiny-h264-aac-mediacodec.m4v",
    ];

    let binary = app_path.join("Frameworks/App.framework/App");
    let metadata = match fs::symlink_metadata(&binary) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(IosError::storage(format!(
                "failed to inspect Flutter AOT binary '{}': {error}",
                binary.display()
            )));
        }
        Ok(metadata) => metadata,
    };
    if !metadata.file_type().is_file() {
        return Err(IosError::conformance(format!(
            "Flutter AOT binary '{}' is not a regular non-symlink file",
            binary.display()
        )));
    }
    if metadata.len() > MAX_APP_AOT_BINARY_BYTES {
        return Err(IosError::storage(format!(
            "Flutter AOT binary '{}' exceeds {MAX_APP_AOT_BINARY_BYTES} bytes",
            binary.display()
        )));
    }

    let mut file = File::open(&binary).map_err(|error| {
        IosError::storage(format!(
            "failed to open Flutter AOT binary '{}': {error}",
            binary.display()
        ))
    })?;
    let overlap_length = MARKERS
        .iter()
        .map(|marker| marker.len())
        .max()
        .unwrap_or_default()
        .saturating_sub(1);
    let mut overlap = Vec::new();
    let mut chunk = [0_u8; 64 * 1024];
    let mut inspected_bytes = 0_u64;
    loop {
        check_app_store_cancellation(cancellation)?;
        let count = file.read(&mut chunk).map_err(|error| {
            IosError::storage(format!(
                "failed to scan Flutter AOT binary '{}': {error}",
                binary.display()
            ))
        })?;
        if count == 0 {
            return Ok(());
        }
        inspected_bytes = inspected_bytes.saturating_add(count as u64);
        if inspected_bytes > MAX_APP_AOT_BINARY_BYTES {
            return Err(IosError::storage(format!(
                "Flutter AOT binary '{}' exceeds {MAX_APP_AOT_BINARY_BYTES} bytes while scanning",
                binary.display()
            )));
        }
        let mut window = Vec::with_capacity(overlap.len() + count);
        window.extend_from_slice(&overlap);
        window.extend(chunk[..count].iter().map(u8::to_ascii_lowercase));
        if MARKERS.iter().any(|marker| {
            window
                .windows(marker.len())
                .any(|candidate| candidate == *marker)
        }) {
            return Err(IosError::conformance(format!(
                "Release binary contains test fixture markers: {}",
                binary.display()
            )));
        }
        let keep = overlap_length.min(window.len());
        overlap.clear();
        overlap.extend_from_slice(&window[window.len() - keep..]);
    }
}

fn check_app_store_cancellation(
    cancellation: &external_process::InterruptDeferral,
) -> Result<(), IosError> {
    if cancellation.is_cancelled() {
        Err(app_store_cancelled_error())
    } else {
        Ok(())
    }
}

fn app_store_cancelled_error() -> IosError {
    IosError::worker("iOS App Store layout verification was cancelled")
}

fn validate_app_store_framework(
    frameworks_directory: &Path,
    framework_name: &str,
) -> Result<ValidatedFramework, IosError> {
    let framework = validate_framework_bundle(
        frameworks_directory,
        framework_name,
        "Missing required top-level optional framework",
    )?;
    let binary = framework.path.join(framework_name);
    validate_framework_dependencies(&binary, frameworks_directory, framework_name)?;
    Ok(framework)
}

fn validate_framework_bundle(
    frameworks_directory: &Path,
    framework_name: &str,
    missing_message: &str,
) -> Result<ValidatedFramework, IosError> {
    let framework = frameworks_directory.join(format!("{framework_name}.framework"));
    require_bundle_directory(&framework, missing_message)?;
    let info_plist = framework.join("Info.plist");
    require_bundle_file(
        &info_plist,
        "Missing framework Info.plist",
        MAX_APP_INFO_PLIST_BYTES,
    )?;
    let binary = framework.join(framework_name);
    require_bundle_file(
        &binary,
        "Missing framework executable",
        MAX_APP_AOT_BINARY_BYTES,
    )?;
    if path_exists_without_following_symlinks(&framework.join("Resources"))? {
        return Err(IosError::conformance(format!(
            "iOS shallow frameworks must keep resources at the framework root:\n  {}/Resources\nA top-level Resources directory makes Foundation ignore the root Info.plist.",
            framework.display()
        )));
    }

    let metadata = validate_framework_plist(&framework, framework_name, &info_plist)?;
    validate_foundation_bundle(&framework, framework_name, &metadata.bundle_identifier)?;
    validate_framework_install_name(&binary, framework_name)?;
    validate_framework_platform(&binary, framework_name, &metadata.minimum_os)?;
    validate_framework_architecture(&binary, framework_name)?;
    Ok(ValidatedFramework {
        path: framework,
        bundle_identifier: metadata.bundle_identifier,
        minimum_os: metadata.minimum_os,
    })
}

fn validate_framework_plist(
    framework: &Path,
    framework_name: &str,
    info_plist: &Path,
) -> Result<FrameworkBundleMetadata, IosError> {
    let object = read_plist_dictionary(info_plist, "framework")?;
    let package_type = required_plist_string(&object, "CFBundlePackageType", info_plist)?;
    let executable = required_plist_string(&object, "CFBundleExecutable", info_plist)?;
    if package_type != "FMWK" || executable != framework_name {
        return Err(IosError::conformance(format!(
            "Invalid framework bundle metadata: {}",
            framework.display()
        )));
    }
    let bundle_identifier = required_plist_string(&object, "CFBundleIdentifier", info_plist)?;
    for key in [
        "CFBundleName",
        "CFBundleVersion",
        "CFBundleShortVersionString",
    ] {
        let _ = required_plist_string(&object, key, info_plist)?;
    }
    let minimum_os = required_plist_string(&object, "MinimumOSVersion", info_plist)?;
    let bundle_platform = object
        .get("CFBundleSupportedPlatforms")
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.first())
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let sdk_name = object
        .get("DTPlatformName")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if bundle_platform != "iPhoneOS" || sdk_name != "iphoneos" {
        return Err(IosError::conformance(format!(
            "Unexpected framework bundle platform metadata: {}\n  CFBundleSupportedPlatforms[0]: {} (expected iPhoneOS)\n  DTPlatformName: {} (expected iphoneos)",
            framework.display(),
            display_missing(bundle_platform),
            display_missing(sdk_name)
        )));
    }
    Ok(FrameworkBundleMetadata {
        bundle_identifier: bundle_identifier.to_owned(),
        minimum_os: minimum_os.to_owned(),
    })
}

pub(crate) fn read_plist_dictionary(
    info_plist: &Path,
    bundle_kind: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, IosError> {
    let plutil = configured_tool("PLUTIL", "plutil");
    let mut lint = Command::new(&plutil);
    lint.arg("-lint").arg(info_plist).stdin(Stdio::null());
    let lint_label = format!("{bundle_kind} plist lint");
    let _ = run_apple_tool(&mut lint, &lint_label, MAX_APP_INFO_PLIST_BYTES as usize)?;

    let mut convert = Command::new(plutil);
    convert
        .args(["-convert", "json", "-o", "-"])
        .arg(info_plist)
        .stdin(Stdio::null());
    let conversion_label = format!("{bundle_kind} plist JSON conversion");
    let json = run_apple_tool(
        &mut convert,
        &conversion_label,
        MAX_APP_INFO_PLIST_BYTES as usize,
    )?;
    let value: serde_json::Value = serde_json::from_slice(&json).map_err(|error| {
        IosError::conformance(format!(
            "Unable to parse {bundle_kind} metadata '{}': {error}",
            info_plist.display()
        ))
    })?;
    value.as_object().cloned().ok_or_else(|| {
        IosError::conformance(format!(
            "{bundle_kind} metadata is not a dictionary: {}",
            info_plist.display()
        ))
    })
}

pub(crate) fn required_plist_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
    path: &Path,
) -> Result<&'a str, IosError> {
    object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            IosError::conformance(format!(
                "Incomplete framework bundle metadata: {} ({key})",
                path.display()
            ))
        })
}

fn display_missing(value: &str) -> &str {
    if value.is_empty() { "<missing>" } else { value }
}

pub(crate) fn validate_foundation_bundle(
    framework: &Path,
    framework_name: &str,
    bundle_identifier: &str,
) -> Result<(), IosError> {
    let module_cache = env::var_os("VESPER_SWIFT_MODULE_CACHE_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("vesper-swift-module-cache"));
    fs::create_dir_all(&module_cache).map_err(|error| {
        IosError::compatibility(format!(
            "failed to create Swift module cache '{}': {error}",
            module_cache.display()
        ))
    })?;
    let metadata = fs::symlink_metadata(&module_cache).map_err(|error| {
        IosError::compatibility(format!(
            "failed to inspect Swift module cache '{}': {error}",
            module_cache.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(IosError::compatibility(format!(
            "Swift module cache '{}' is not a regular non-symlink directory",
            module_cache.display()
        )));
    }

    let mut command = Command::new(configured_tool("SWIFT", "swift"));
    command
        .env("CLANG_MODULE_CACHE_PATH", &module_cache)
        .env("SWIFT_MODULECACHE_PATH", &module_cache)
        .arg("-e")
        .arg(FOUNDATION_FRAMEWORK_VALIDATION)
        .arg(framework)
        .arg(framework_name)
        .arg(bundle_identifier)
        .stdin(Stdio::null());
    let _ = run_apple_tool(
        &mut command,
        "Foundation framework metadata validation",
        MAX_APPLE_TOOL_OUTPUT_BYTES,
    )?;
    Ok(())
}

pub(crate) fn validate_framework_install_name(
    binary: &Path,
    framework_name: &str,
) -> Result<(), IosError> {
    let mut command = Command::new(configured_tool("OTOOL", "otool"));
    command.arg("-D").arg(binary).stdin(Stdio::null());
    let output = run_apple_tool(
        &mut command,
        "framework install-name inspection",
        MAX_APPLE_TOOL_OUTPUT_BYTES,
    )?;
    let output = parse_tool_utf8(&output, "otool -D")?;
    let install_name = output.lines().skip(1).find(|line| !line.trim().is_empty());
    let expected = format!("@rpath/{framework_name}.framework/{framework_name}");
    if install_name.map(str::trim) != Some(expected.as_str()) {
        return Err(IosError::conformance(format!(
            "Unexpected framework install name for {}:\n  actual:   {}\n  expected: {expected}",
            binary.display(),
            install_name.map(str::trim).unwrap_or("<missing>")
        )));
    }
    Ok(())
}

fn validate_framework_dependencies(
    binary: &Path,
    frameworks_directory: &Path,
    framework_name: &str,
) -> Result<(), IosError> {
    let dependencies = read_macho_dependencies(binary)?;
    for dependency in &dependencies {
        if let Some(bundle) = dependency
            .strip_prefix("@rpath/")
            .and_then(|value| value.split_once('/').map(|(bundle, _)| bundle))
            .filter(|bundle| bundle.starts_with("VesperFFmpeg") && bundle.ends_with(".framework"))
        {
            let sibling = frameworks_directory.join(bundle);
            if !matches!(
                fs::symlink_metadata(&sibling),
                Ok(metadata) if metadata.file_type().is_dir()
            ) {
                return Err(IosError::conformance(format!(
                    "Missing sibling FFmpeg framework required by {}:\n  {}",
                    binary.display(),
                    sibling.display()
                )));
            }
        }
    }

    validate_framework_dependency_list(&binary.display().to_string(), framework_name, &dependencies)
}

pub(crate) fn validate_framework_dependency_list(
    label: &str,
    framework_name: &str,
    dependencies: &[String],
) -> Result<(), IosError> {
    for dependency in dependencies {
        if is_unwrapped_ffmpeg_dependency(dependency) {
            return Err(IosError::conformance(format!(
                "Unwrapped FFmpeg dependency remains in {label}:\n  {dependency}"
            )));
        }
    }
    let expected_dependencies = expected_framework_dependencies(framework_name);
    for expected_name in expected_dependencies {
        let expected = format!("@rpath/{expected_name}.framework/{expected_name}");
        if !dependencies.iter().any(|actual| actual == &expected) {
            return Err(IosError::conformance(format!(
                "{label} is missing dynamic dependency {expected}"
            )));
        }
    }
    let self_dependency = format!("@rpath/{framework_name}.framework/{framework_name}");
    for dependency in dependencies {
        if dependency == &self_dependency
            || dependency.starts_with("/usr/lib/")
            || dependency.starts_with("/System/Library/Frameworks/")
            || expected_dependencies.iter().any(|expected_name| {
                dependency == &format!("@rpath/{expected_name}.framework/{expected_name}")
            })
        {
            continue;
        }
        return Err(IosError::conformance(format!(
            "{label} has unexpected non-system dynamic dependency:\n  {dependency}"
        )));
    }
    Ok(())
}

pub(crate) fn expected_framework_dependencies(framework_name: &str) -> &'static [&'static str] {
    match framework_name {
        "VesperFFmpegAVCodec" => &["VesperFFmpegAVUtil"],
        "VesperFFmpegAVFormat" => &["VesperFFmpegAVCodec", "VesperFFmpegAVUtil"],
        "VesperPlayerRemuxFfmpegPlugin" | "VesperPlayerSourceNormalizerFfmpegPlugin" => &[
            "VesperFFmpegAVCodec",
            "VesperFFmpegAVFormat",
            "VesperFFmpegAVUtil",
        ],
        _ => &[],
    }
}

pub(crate) fn read_macho_dependencies(binary: &Path) -> Result<Vec<String>, IosError> {
    let mut command = Command::new(configured_tool("OTOOL", "otool"));
    command.arg("-L").arg(binary).stdin(Stdio::null());
    let output = run_apple_tool(
        &mut command,
        "Mach-O dependency inspection",
        MAX_APPLE_TOOL_OUTPUT_BYTES,
    )?;
    let output = parse_tool_utf8(&output, "otool -L")?;
    parse_otool_dependencies(output, &binary.display().to_string())
}

pub(crate) fn parse_otool_dependencies(output: &str, label: &str) -> Result<Vec<String>, IosError> {
    let mut dependencies = Vec::new();
    for line in output.lines().skip(1) {
        let line = line.trim_start();
        if line.is_empty() {
            continue;
        }
        let dependency = line
            .split_once(" (compatibility version ")
            .map(|(dependency, _)| dependency)
            .ok_or_else(|| {
                IosError::conformance(format!(
                    "Malformed otool dependency record for {label}: {line}"
                ))
            })?;
        dependencies.push(dependency.to_owned());
    }
    Ok(dependencies)
}

fn is_unwrapped_ffmpeg_dependency(dependency: &str) -> bool {
    let name = dependency.rsplit('/').next().unwrap_or(dependency);
    [
        "libavcodec",
        "libavdevice",
        "libavfilter",
        "libavformat",
        "libavutil",
        "libpostproc",
        "libswresample",
        "libswscale",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix) && name.ends_with(".dylib"))
}

fn validate_framework_platform(
    binary: &Path,
    framework_name: &str,
    expected_minimum_os: &str,
) -> Result<(), IosError> {
    let mut command = Command::new(configured_tool("XCRUN", "xcrun"));
    command
        .args(["vtool", "-show-build"])
        .arg(binary)
        .stdin(Stdio::null());
    let output = run_apple_tool(
        &mut command,
        "framework Mach-O platform inspection",
        MAX_APPLE_TOOL_OUTPUT_BYTES,
    )?;
    let output = parse_tool_utf8(&output, "xcrun vtool")?;
    let metadata = parse_vtool_build_metadata(output, &binary.display().to_string())?;
    if metadata.platform != "IOS" {
        return Err(IosError::conformance(format!(
            "Unexpected framework Mach-O build platform: {}/{}\n  actual:   {}\n  expected: IOS",
            binary.parent().unwrap_or_else(|| Path::new(".")).display(),
            framework_name,
            display_missing(&metadata.platform)
        )));
    }
    if normalize_apple_version(&metadata.minimum_os, "Mach-O minimum OS")?
        != normalize_apple_version(expected_minimum_os, "framework MinimumOSVersion")?
    {
        return Err(IosError::conformance(format!(
            "Framework Mach-O minimum OS does not match Info.plist: {}\n  Mach-O: {}\n  plist:  {}",
            binary.display(),
            metadata.minimum_os,
            expected_minimum_os
        )));
    }
    Ok(())
}

pub(crate) fn parse_vtool_build_metadata(
    output: &str,
    label: &str,
) -> Result<AppleMachOBuildMetadata, IosError> {
    #[derive(Clone, Copy)]
    enum BuildCommand {
        None,
        Modern,
        LegacyIos,
    }

    let mut command = BuildCommand::None;
    let mut platform = None::<String>;
    let mut minimum_os = None::<String>;
    for line in output.lines() {
        let columns = line.split_ascii_whitespace().collect::<Vec<_>>();
        if columns.first() == Some(&"Load") && columns.get(1) == Some(&"command") {
            command = BuildCommand::None;
            continue;
        }
        match columns.as_slice() {
            ["cmd", "LC_BUILD_VERSION"] => command = BuildCommand::Modern,
            ["cmd", "LC_VERSION_MIN_IPHONEOS"] => {
                command = BuildCommand::LegacyIos;
                set_unique_macho_field(&mut platform, "IOS", "platform", label)?;
            }
            ["platform", value] if matches!(command, BuildCommand::Modern) => {
                set_unique_macho_field(&mut platform, value, "platform", label)?;
            }
            ["minos", value] if matches!(command, BuildCommand::Modern) => {
                set_unique_macho_field(&mut minimum_os, value, "minimum OS", label)?;
            }
            ["version", value] if matches!(command, BuildCommand::LegacyIos) => {
                set_unique_macho_field(&mut minimum_os, value, "minimum OS", label)?;
            }
            _ => {}
        }
    }
    let platform = platform.ok_or_else(|| {
        IosError::conformance(format!("Mach-O build metadata omits its platform: {label}"))
    })?;
    let minimum_os = minimum_os.ok_or_else(|| {
        IosError::conformance(format!(
            "Mach-O build metadata omits its minimum OS: {label}"
        ))
    })?;
    let _ = normalize_apple_version(&minimum_os, "Mach-O minimum OS")?;
    Ok(AppleMachOBuildMetadata {
        platform,
        minimum_os,
    })
}

fn set_unique_macho_field(
    field: &mut Option<String>,
    value: &str,
    name: &str,
    label: &str,
) -> Result<(), IosError> {
    if field.replace(value.to_owned()).is_some() {
        return Err(IosError::conformance(format!(
            "Mach-O build metadata repeats its {name}: {label}"
        )));
    }
    Ok(())
}

pub(crate) fn normalize_apple_version(
    value: &str,
    label: &str,
) -> Result<(u16, u16, u16), IosError> {
    let components = value.split('.').collect::<Vec<_>>();
    if components.is_empty()
        || components.len() > 3
        || components.iter().any(|component| {
            component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(IosError::conformance(format!(
            "{label} is not a supported Apple version: {value}"
        )));
    }
    let mut parsed = [0_u16; 3];
    for (index, component) in components.iter().enumerate() {
        parsed[index] = component.parse::<u16>().map_err(|error| {
            IosError::conformance(format!(
                "{label} is not a supported Apple version ({value}): {error}"
            ))
        })?;
    }
    Ok((parsed[0], parsed[1], parsed[2]))
}

pub(crate) fn validate_framework_architecture(
    binary: &Path,
    framework_name: &str,
) -> Result<(), IosError> {
    let mut command = Command::new(configured_tool("LIPO", "lipo"));
    command.arg("-archs").arg(binary).stdin(Stdio::null());
    let output = run_apple_tool(
        &mut command,
        "framework architecture inspection",
        MAX_APPLE_TOOL_OUTPUT_BYTES,
    )?;
    let architectures = parse_tool_utf8(&output, "lipo -archs")?.trim();
    if architectures != "arm64" {
        return Err(IosError::conformance(format!(
            "Optional iOS App Store frameworks must contain only arm64:\n  {} ({})",
            binary.display(),
            display_missing(architectures)
        )));
    }
    let _ = framework_name;
    Ok(())
}

fn validate_app_store_profile_hashes(frameworks_directory: &Path) -> Result<String, IosError> {
    let mut expected = None::<String>;
    for framework_name in APP_STORE_FFMPEG_FRAMEWORKS {
        let path = frameworks_directory
            .join(format!("{framework_name}.framework"))
            .join("profile-hash.txt");
        require_bundle_file(
            &path,
            &format!("Missing FFmpeg profile hash for {framework_name}"),
            MAX_APP_PROFILE_RECORD_BYTES as u64,
        )?;
        let bytes = read_bounded_file(&path, MAX_APP_PROFILE_RECORD_BYTES, "FFmpeg profile hash")?;
        let profile = parse_profile_record(&bytes, &path)?;
        match &expected {
            None => expected = Some(profile.to_owned()),
            Some(expected) if expected == profile => {}
            Some(expected) => {
                return Err(IosError::conformance(format!(
                    "FFmpeg profile hash mismatch in the app bundle:\n  expected: {expected}\n  actual:   {profile} ({framework_name})"
                )));
            }
        }
    }
    expected.ok_or_else(|| IosError::worker("App Store FFmpeg framework list is empty"))
}

fn parse_profile_record<'a>(bytes: &'a [u8], path: &Path) -> Result<&'a str, IosError> {
    let record = bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .unwrap_or(bytes);
    let record = std::str::from_utf8(record).map_err(|error| {
        IosError::conformance(format!(
            "FFmpeg profile hash is not UTF-8 '{}': {error}",
            path.display()
        ))
    })?;
    if record.is_empty() {
        return Err(IosError::conformance(format!(
            "Empty FFmpeg profile hash: {}",
            path.display()
        )));
    }
    if record
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(IosError::conformance(format!(
            "FFmpeg profile hash must be one exact non-whitespace record: {}",
            path.display()
        )));
    }
    Ok(record)
}

fn verify_app_does_not_link_optional_frameworks<'a>(
    app_path: &Path,
    plugin_framework_names: impl IntoIterator<Item = &'a String>,
) -> Result<(), IosError> {
    let prohibited_frameworks = APP_STORE_FFMPEG_FRAMEWORKS
        .into_iter()
        .map(str::to_owned)
        .chain(plugin_framework_names.into_iter().cloned())
        .collect::<BTreeSet<_>>();
    for binary in app_linkage_binaries(app_path)? {
        let dependencies = read_macho_dependencies(&binary)?;
        for framework_name in &prohibited_frameworks {
            if let Some(dependency) = dependencies
                .iter()
                .find(|dependency| dependency_loads_framework(dependency, framework_name))
            {
                return Err(IosError::conformance(format!(
                    "App executable must not link optional plugin or FFmpeg frameworks before registry validation:\n  binary:     {}\n  dependency: {dependency}",
                    binary.display()
                )));
            }
        }
    }
    Ok(())
}

fn app_linkage_binaries(app_path: &Path) -> Result<Vec<PathBuf>, IosError> {
    let info_plist = app_path.join("Info.plist");
    require_bundle_file(
        &info_plist,
        "App bundle is missing Info.plist",
        MAX_APP_INFO_PLIST_BYTES,
    )?;
    let object = read_plist_dictionary(&info_plist, "application")?;
    let executable = object
        .get("CFBundleExecutable")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            IosError::conformance(format!(
                "Incomplete application bundle metadata: {} (CFBundleExecutable)",
                info_plist.display()
            ))
        })?;
    validate_bundle_executable_name(executable, &info_plist)?;

    let executable_path = app_path.join(executable);
    require_bundle_file(
        &executable_path,
        "App bundle is missing its executable",
        MAX_APP_AOT_BINARY_BYTES,
    )?;
    let mut binaries = vec![executable_path];
    for entry in fs::read_dir(app_path).map_err(|error| {
        IosError::storage(format!(
            "failed to scan application executables '{}': {error}",
            app_path.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            IosError::storage(format!(
                "failed to read an application entry under '{}': {error}",
                app_path.display()
            ))
        })?;
        let path = entry.path();
        if path == binaries[0]
            || !entry
                .file_name()
                .to_string_lossy()
                .ends_with(".debug.dylib")
        {
            continue;
        }
        require_bundle_file(
            &path,
            "Application debug dylib is not a regular file",
            MAX_APP_AOT_BINARY_BYTES,
        )?;
        binaries.push(path);
    }
    binaries.sort();
    Ok(binaries)
}

fn validate_bundle_executable_name(executable: &str, info_plist: &Path) -> Result<(), IosError> {
    let mut components = Path::new(executable).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(IosError::conformance(format!(
            "Application CFBundleExecutable must be one file name: {} ({executable})",
            info_plist.display()
        )));
    }
    Ok(())
}

fn dependency_loads_framework(dependency: &str, framework_name: &str) -> bool {
    let expected_suffix = format!("/{framework_name}.framework/{framework_name}");
    dependency == format!("@rpath{expected_suffix}") || dependency.ends_with(&expected_suffix)
}

fn verify_app_store_signatures(
    app_path: &Path,
    frameworks: &BTreeMap<String, ValidatedFramework>,
) -> Result<(), IosError> {
    let codesign = configured_tool("CODESIGN", "codesign");
    for framework in frameworks.values() {
        let mut command = Command::new(&codesign);
        command
            .args(["--verify", "--strict"])
            .arg(&framework.path)
            .stdin(Stdio::null());
        let _ = run_apple_tool(
            &mut command,
            "optional framework signature verification",
            MAX_APPLE_TOOL_OUTPUT_BYTES,
        )?;
    }
    let mut command = Command::new(&codesign);
    command
        .args(["--verify", "--strict", "--deep"])
        .arg(app_path)
        .stdin(Stdio::null());
    let _ = run_apple_tool(
        &mut command,
        "application signature verification",
        MAX_APPLE_TOOL_OUTPUT_BYTES,
    )?;

    let app_signature = read_code_signature_metadata(&codesign, app_path)?;
    if app_signature.team_identifier == "not set" {
        return Err(IosError::conformance(format!(
            "App Store application signature has no TeamIdentifier: {}",
            app_path.display()
        )));
    }
    for framework in frameworks.values() {
        let signature = read_code_signature_metadata(&codesign, &framework.path)?;
        validate_framework_signature_identity(
            framework,
            &app_signature.team_identifier,
            &signature,
        )?;
    }
    Ok(())
}

fn read_code_signature_metadata(
    codesign: &OsStr,
    path: &Path,
) -> Result<CodeSignatureMetadata, IosError> {
    let mut command = Command::new(codesign);
    command
        .args(["--display", "--verbose=4"])
        .arg(path)
        .stdin(Stdio::null());
    let result = external_process::run_interruptible_capture(
        &mut command,
        "code signature metadata inspection",
        MAX_APPLE_TOOL_OUTPUT_BYTES,
        MAX_APPLE_TOOL_OUTPUT_BYTES,
    )
    .map_err(map_external_process_error)?;
    if !result.status.success() {
        let stdout = String::from_utf8_lossy(&result.stdout);
        let stderr = String::from_utf8_lossy(&result.stderr);
        let diagnostic = [stdout.trim_end(), stderr.trim_end()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(IosError::conformance(format!(
            "code signature metadata inspection exited unsuccessfully ({}){}{}",
            result.status,
            if diagnostic.is_empty() { "" } else { ":\n" },
            diagnostic
        )));
    }
    let mut output = result.stdout;
    output.push(b'\n');
    output.extend_from_slice(&result.stderr);
    let output = parse_tool_utf8(&output, "codesign --display --verbose=4")?;
    parse_code_signature_metadata(output, path)
}

fn parse_code_signature_metadata(
    output: &str,
    path: &Path,
) -> Result<CodeSignatureMetadata, IosError> {
    Ok(CodeSignatureMetadata {
        identifier: parse_code_signature_field(output, "Identifier", path)?,
        team_identifier: parse_code_signature_field(output, "TeamIdentifier", path)?,
    })
}

fn parse_code_signature_field(output: &str, field: &str, path: &Path) -> Result<String, IosError> {
    let prefix = format!("{field}=");
    let mut value = None::<&str>;
    for candidate in output
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix(&prefix))
    {
        if candidate.is_empty() || value.is_some() {
            return Err(IosError::conformance(format!(
                "Repeated or empty {field} in code signature metadata: {}",
                path.display()
            )));
        }
        value = Some(candidate);
    }
    value.map(str::to_owned).ok_or_else(|| {
        IosError::conformance(format!(
            "Code signature metadata is missing {field}: {}",
            path.display()
        ))
    })
}

fn validate_framework_signature_identity(
    framework: &ValidatedFramework,
    app_team_identifier: &str,
    signature: &CodeSignatureMetadata,
) -> Result<(), IosError> {
    if signature.identifier != framework.bundle_identifier {
        return Err(IosError::conformance(format!(
            "Framework code-signing Identifier does not match its registry/plist identity: {}\n  expected: {}\n  actual:   {}",
            framework.path.display(),
            framework.bundle_identifier,
            signature.identifier
        )));
    }
    if signature.team_identifier != app_team_identifier {
        return Err(IosError::conformance(format!(
            "Framework TeamIdentifier does not match the containing application: {}\n  app:       {app_team_identifier}\n  framework: {}",
            framework.path.display(),
            signature.team_identifier
        )));
    }
    Ok(())
}

fn run_apple_tool(
    command: &mut Command,
    label: &str,
    stdout_maximum_bytes: usize,
) -> Result<Vec<u8>, IosError> {
    let result = external_process::run_interruptible_capture(
        command,
        label,
        stdout_maximum_bytes,
        MAX_APPLE_TOOL_OUTPUT_BYTES,
    )
    .map_err(map_external_process_error)?;
    if !result.status.success() {
        let stdout = String::from_utf8_lossy(&result.stdout);
        let stderr = String::from_utf8_lossy(&result.stderr);
        let diagnostic = [stdout.trim_end(), stderr.trim_end()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(IosError::conformance(format!(
            "{label} exited unsuccessfully ({}){}{}",
            result.status,
            if diagnostic.is_empty() { "" } else { ":\n" },
            diagnostic
        )));
    }
    if !result.stderr.is_empty() {
        let stderr = io::stderr();
        let mut diagnostics = stderr.lock();
        diagnostics
            .write_all(&result.stderr)
            .map_err(output_error)?;
        diagnostics.flush().map_err(output_error)?;
    }
    Ok(result.stdout)
}

fn parse_tool_utf8<'a>(output: &'a [u8], label: &str) -> Result<&'a str, IosError> {
    std::str::from_utf8(output).map_err(|error| {
        IosError::conformance(format!("{label} produced non-UTF-8 output: {error}"))
    })
}

fn generate_bridge(paths: &BridgePaths) -> Result<GeneratedShim, IosError> {
    ios_bridge_shim::generate_from_manifest(&paths.manifest)
        .map_err(|error| IosError::storage(error.to_string()))
}

fn validate_forbidden_download_casts(source: &str) -> Result<(), IosError> {
    static PATTERN: OnceLock<Result<Regex, String>> = OnceLock::new();
    let pattern = PATTERN
        .get_or_init(|| {
            Regex::new(
                r"\(\s*(?:const\s+)?(?:PlayerFfiDownload|VesperRuntimeDownload)[A-Za-z0-9_]*\s*\*\s*\)",
            )
            .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| IosError::worker(format!("invalid bridge cast validator: {error}")))?;
    if pattern.is_match(source) {
        Err(IosError::conformance(
            "Download bridge DTO pointer casts are not allowed in VesperPlayerKitBridgeShim.c.\nUse explicit input/output conversion helpers instead.",
        ))
    } else {
        Ok(())
    }
}

fn validate_rust_source_exports(path: &Path, required: &[String]) -> Result<(), IosError> {
    let source = read_bounded_utf8_file(
        path,
        MAX_RUST_FFI_SOURCE_BYTES,
        "player-ffi-ios Rust source",
    )?;
    let syntax = syn::parse_file(&source).map_err(|error| {
        IosError::conformance(format!(
            "player-ffi-ios Rust source '{}' cannot be parsed: {error}",
            path.display()
        ))
    })?;
    let mut exported = BTreeSet::new();
    collect_unconditional_rust_exports(&syntax.items, &mut exported);
    let missing = required
        .iter()
        .filter(|symbol| !exported.contains(symbol.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(IosError::conformance(format!(
            "VesperPlayerKitBridgeShim.c references Rust FFI symbols that are not exported by player-ffi-ios:\n{}",
            missing.join("\n")
        )))
    }
}

fn collect_unconditional_rust_exports(items: &[syn::Item], exported: &mut BTreeSet<String>) {
    for item in items {
        match item {
            syn::Item::Fn(function) if is_unconditional_c_export(function) => {
                exported.insert(function.sig.ident.to_string());
            }
            syn::Item::Mod(module)
                if !has_conditional_attribute(&module.attrs) && module.content.is_some() =>
            {
                if let Some((_, items)) = &module.content {
                    collect_unconditional_rust_exports(items, exported);
                }
            }
            _ => {}
        }
    }
}

fn is_unconditional_c_export(function: &syn::ItemFn) -> bool {
    matches!(function.vis, syn::Visibility::Public(_))
        && matches!(&function.sig.safety, syn::Safety::Unsafe(_))
        && function
            .sig
            .abi
            .as_ref()
            .and_then(|abi| abi.name.as_ref())
            .is_some_and(|name| name.value() == "C")
        && function.sig.ident.to_string().starts_with("player_ffi_")
        && !has_conditional_attribute(&function.attrs)
        && has_no_mangle_attribute(&function.attrs)
}

fn has_conditional_attribute(attributes: &[syn::Attribute]) -> bool {
    attributes
        .iter()
        .any(|attribute| attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr"))
}

fn has_no_mangle_attribute(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        if attribute.path().is_ident("no_mangle") {
            return true;
        }
        if !attribute.path().is_ident("unsafe") {
            return false;
        }
        let mut contains_no_mangle = false;
        let parsed = attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("no_mangle") {
                contains_no_mangle = true;
            }
            Ok(())
        });
        parsed.is_ok() && contains_no_mangle
    })
}

fn run_clang_syntax_check(paths: &BridgePaths) -> Result<(), IosError> {
    let clang = configured_tool("CLANG", "clang");
    let mut command = Command::new(clang);
    command
        .arg("-fsyntax-only")
        .arg("-I")
        .arg(&paths.shim_directory)
        .arg(&paths.source)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status =
        external_process::run_interruptible(&mut command, "VesperPlayerKit bridge clang check")
            .map_err(map_external_process_error)?;
    if status.success() {
        Ok(())
    } else {
        Err(IosError::conformance(format!(
            "VesperPlayerKit bridge clang check exited unsuccessfully ({status})"
        )))
    }
}

fn resolve_archives(
    paths: &BridgePaths,
    cli_archive: Option<&Path>,
) -> Result<Vec<PathBuf>, IosError> {
    if let Some(path) = cli_archive {
        validate_archive(path)?;
        return Ok(vec![path.to_path_buf()]);
    }
    if let Some(path) = env::var_os("VESPER_IOS_FFI_ARCHIVE").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(path);
        validate_archive(&path)?;
        return Ok(vec![path]);
    }
    if !validate_repository_path(
        &paths.root,
        &paths.archive_root,
        RepositoryPathKind::Directory,
        true,
        "Rust FFI archive root",
    )? {
        return Ok(Vec::new());
    }
    discover_archives(&paths.archive_root)
}

fn discover_archives(root: &Path) -> Result<Vec<PathBuf>, IosError> {
    match fs::symlink_metadata(root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(IosError::storage(format!(
                "failed to inspect Rust FFI archive root '{}': {error}",
                root.display()
            )));
        }
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(IosError::storage(format!(
                "Rust FFI archive root '{}' is not a regular non-symlink directory",
                root.display()
            )));
        }
        Ok(_) => {}
    }

    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut visited_entries = 0usize;
    let mut archives = Vec::new();
    while let Some((directory, depth)) = queue.pop_front() {
        if depth > MAX_ARCHIVE_DISCOVERY_DEPTH {
            return Err(IosError::storage(format!(
                "Rust FFI archive discovery exceeds depth {MAX_ARCHIVE_DISCOVERY_DEPTH}"
            )));
        }
        let entries = fs::read_dir(&directory).map_err(|error| {
            IosError::storage(format!(
                "failed to read Rust FFI archive directory '{}': {error}",
                directory.display()
            ))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                IosError::storage(format!(
                    "failed to read an entry under '{}': {error}",
                    directory.display()
                ))
            })?;
            visited_entries = visited_entries.saturating_add(1);
            if visited_entries > MAX_ARCHIVE_DISCOVERY_ENTRIES {
                return Err(IosError::storage(format!(
                    "Rust FFI archive discovery exceeds {MAX_ARCHIVE_DISCOVERY_ENTRIES} entries"
                )));
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect Rust FFI archive candidate '{}': {error}",
                    path.display()
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(IosError::storage(format!(
                    "Rust FFI archive discovery does not follow symlinks: {}",
                    path.display()
                )));
            }
            if metadata.file_type().is_dir() {
                queue.push_back((path, depth + 1));
            } else if metadata.file_type().is_file()
                && path.file_name() == Some(OsStr::new(IOS_FFI_ARCHIVE_NAME))
            {
                validate_archive_metadata(&path, &metadata)?;
                archives.push(path);
                if archives.len() > MAX_DISCOVERED_ARCHIVES {
                    return Err(IosError::storage(format!(
                        "Rust FFI archive discovery exceeds {MAX_DISCOVERED_ARCHIVES} archives"
                    )));
                }
            }
        }
    }
    archives.sort();
    Ok(archives)
}

fn validate_archive(path: &Path) -> Result<(), IosError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::storage(format!(
            "Rust FFI archive is unavailable '{}': {error}",
            path.display()
        ))
    })?;
    validate_archive_metadata(path, &metadata)
}

fn validate_archive_metadata(path: &Path, metadata: &fs::Metadata) -> Result<(), IosError> {
    if !metadata.file_type().is_file() {
        return Err(IosError::storage(format!(
            "Rust FFI archive '{}' is not a regular non-symlink file",
            path.display()
        )));
    }
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(IosError::storage(format!(
            "Rust FFI archive '{}' exceeds {MAX_ARCHIVE_BYTES} bytes",
            path.display()
        )));
    }
    Ok(())
}

fn validate_archive_exports(path: &Path, required: &[String]) -> Result<(), IosError> {
    validate_archive(path)?;
    let mut stdout = tempfile::NamedTempFile::new().map_err(|error| {
        IosError::storage(format!(
            "failed to create bounded nm stdout capture: {error}"
        ))
    })?;
    let mut stderr = tempfile::NamedTempFile::new().map_err(|error| {
        IosError::storage(format!(
            "failed to create bounded nm stderr capture: {error}"
        ))
    })?;
    let nm = configured_tool("NM", "nm");
    let mut command = Command::new(nm);
    #[cfg(target_os = "macos")]
    command.arg("--no-llvm-bc");
    command
        .arg("-gU")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.reopen().map_err(|error| {
            IosError::storage(format!("failed to open nm stdout capture: {error}"))
        })?))
        .stderr(Stdio::from(stderr.reopen().map_err(|error| {
            IosError::storage(format!("failed to open nm stderr capture: {error}"))
        })?));
    let status = external_process::run_interruptible(&mut command, "Rust FFI archive nm scan")
        .map_err(map_external_process_error)?;
    let stdout_bytes = read_bounded_capture(&mut stdout, "nm stdout")?;
    let stderr_bytes = read_bounded_capture(&mut stderr, "nm stderr")?;
    let stderr_text = String::from_utf8_lossy(&stderr_bytes);
    if !status.success() {
        if stdout_bytes.is_empty() || !is_known_nm_metadata_warning(&stderr_text) {
            return Err(IosError::conformance(format!(
                "nm could not read Rust FFI archive symbols: {}\n{}",
                path.display(),
                stderr_text.trim_end()
            )));
        }
        eprintln!(
            "nm reported non-fatal object metadata warnings while reading: {}",
            path.display()
        );
    }

    let symbols = parse_nm_symbols(&stdout_bytes);
    let missing = required
        .iter()
        .filter(|symbol| !symbols.contains(symbol.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(IosError::conformance(format!(
            "Rust FFI archive is missing symbols required by VesperPlayerKitBridgeShim.c: {}\n{}",
            path.display(),
            missing.join("\n")
        )))
    }
}

fn parse_nm_symbols(output: &[u8]) -> BTreeSet<String> {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| {
            let columns = line.split_ascii_whitespace().collect::<Vec<_>>();
            let symbol_type = columns.get(columns.len().checked_sub(2)?)?;
            if symbol_type.len() == 1
                && symbol_type
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphabetic)
            {
                columns.last().copied()
            } else {
                None
            }
        })
        .map(|symbol| symbol.strip_prefix('_').unwrap_or(symbol))
        .filter(|symbol| {
            symbol.starts_with("player_ffi_")
                && symbol
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
        .map(str::to_owned)
        .collect()
}

fn is_known_nm_metadata_warning(stderr: &str) -> bool {
    let mut warnings = stderr.lines().filter(|line| !line.trim().is_empty());
    let Some(first) = warnings.next() else {
        return false;
    };
    std::iter::once(first).chain(warnings).all(|line| {
        line.contains("Unknown attribute kind (")
            && line.contains("Producer:")
            && line.contains("Reader:")
    })
}

fn configured_tool(variable: &str, default: &str) -> OsString {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from(default))
}

fn map_external_process_error(error: external_process::ExternalProcessError) -> IosError {
    let kind = error.kind();
    let message = error.to_string();
    match kind {
        external_process::ExternalProcessErrorKind::Compatibility => {
            IosError::compatibility(message)
        }
        external_process::ExternalProcessErrorKind::Worker
        | external_process::ExternalProcessErrorKind::Cancelled => IosError::worker(message),
    }
}

fn read_optional_bounded_file(
    path: &Path,
    maximum_bytes: usize,
    label: &str,
) -> Result<Option<Vec<u8>>, IosError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(IosError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))),
        Ok(_) => read_bounded_file(path, maximum_bytes, label).map(Some),
    }
}

fn read_bounded_utf8_file(
    path: &Path,
    maximum_bytes: usize,
    label: &str,
) -> Result<String, IosError> {
    let bytes = read_bounded_file(path, maximum_bytes, label)?;
    String::from_utf8(bytes).map_err(|error| {
        IosError::storage(format!(
            "{label} '{}' is not UTF-8: {error}",
            path.display()
        ))
    })
}

fn read_bounded_file(path: &Path, maximum_bytes: usize, label: &str) -> Result<Vec<u8>, IosError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(IosError::storage(format!(
            "{label} '{}' is not a regular non-symlink file",
            path.display()
        )));
    }
    if metadata.len() > maximum_bytes as u64 {
        return Err(IosError::storage(format!(
            "{label} '{}' exceeds {maximum_bytes} bytes",
            path.display()
        )));
    }
    let mut file = File::open(path).map_err(|error| {
        IosError::storage(format!(
            "failed to open {label} '{}': {error}",
            path.display()
        ))
    })?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take((maximum_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            IosError::storage(format!(
                "failed to read {label} '{}': {error}",
                path.display()
            ))
        })?;
    if bytes.len() > maximum_bytes {
        return Err(IosError::storage(format!(
            "{label} '{}' exceeds {maximum_bytes} bytes",
            path.display()
        )));
    }
    Ok(bytes)
}

fn read_bounded_capture(
    file: &mut tempfile::NamedTempFile,
    label: &str,
) -> Result<Vec<u8>, IosError> {
    let length = file
        .as_file()
        .metadata()
        .map_err(|error| IosError::storage(format!("failed to inspect {label} capture: {error}")))?
        .len();
    if length > MAX_NM_OUTPUT_BYTES as u64 {
        return Err(IosError::worker(format!(
            "{label} exceeds {MAX_NM_OUTPUT_BYTES} bytes"
        )));
    }
    file.as_file_mut()
        .seek(SeekFrom::Start(0))
        .map_err(|error| IosError::storage(format!("failed to rewind {label} capture: {error}")))?;
    let mut bytes = Vec::with_capacity(length as usize);
    file.as_file_mut()
        .take((MAX_NM_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| IosError::storage(format!("failed to read {label} capture: {error}")))?;
    if bytes.len() > MAX_NM_OUTPUT_BYTES {
        return Err(IosError::worker(format!(
            "{label} exceeds {MAX_NM_OUTPUT_BYTES} bytes"
        )));
    }
    Ok(bytes)
}

#[derive(Clone, Copy)]
enum RepositoryPathKind {
    File,
    Directory,
}

fn validate_repository_path(
    root: &Path,
    target: &Path,
    expected: RepositoryPathKind,
    allow_missing: bool,
    label: &str,
) -> Result<bool, IosError> {
    let relative = target.strip_prefix(root).map_err(|_| {
        IosError::storage(format!(
            "{label} '{}' is outside repository root '{}'",
            target.display(),
            root.display()
        ))
    })?;
    if relative.as_os_str().is_empty() {
        return Err(IosError::storage(format!(
            "{label} must not be the repository root"
        )));
    }

    let component_count = relative.components().count();
    let mut current = root.to_path_buf();
    for (index, component) in relative.components().enumerate() {
        let Component::Normal(value) = component else {
            return Err(IosError::storage(format!(
                "{label} '{}' contains an invalid path component",
                target.display()
            )));
        };
        current.push(value);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if allow_missing && error.kind() == io::ErrorKind::NotFound => {
                return Ok(false);
            }
            Err(error) => {
                return Err(IosError::storage(format!(
                    "failed to inspect {label} '{}': {error}",
                    current.display()
                )));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(IosError::storage(format!(
                "{label} path must not contain symlinks: {}",
                current.display()
            )));
        }
        let is_final = index + 1 == component_count;
        if !is_final && !metadata.file_type().is_dir() {
            return Err(IosError::storage(format!(
                "{label} parent is not a regular directory: {}",
                current.display()
            )));
        }
        if is_final {
            let matches_expected = match expected {
                RepositoryPathKind::File => metadata.file_type().is_file(),
                RepositoryPathKind::Directory => metadata.file_type().is_dir(),
            };
            if !matches_expected {
                let expected_name = match expected {
                    RepositoryPathKind::File => "file",
                    RepositoryPathKind::Directory => "directory",
                };
                return Err(IosError::storage(format!(
                    "{label} '{}' is not a regular non-symlink {expected_name}",
                    current.display()
                )));
            }
        }
    }
    Ok(true)
}

fn require_regular_directory_parent(root: &Path, target: &Path) -> Result<PathBuf, IosError> {
    let parent = target.parent().ok_or_else(|| {
        IosError::storage(format!(
            "iOS bridge path '{}' has no parent",
            target.display()
        ))
    })?;
    validate_repository_path(
        root,
        parent,
        RepositoryPathKind::Directory,
        false,
        "iOS bridge parent",
    )?;
    Ok(parent.to_path_buf())
}

fn sync_generated_files(directory: &Path) -> Result<(), IosError> {
    for relative in [BRIDGE_SHIM_HEADER_FILE, BRIDGE_SHIM_SOURCE_FILE] {
        let path = directory.join(relative);
        let file = File::open(&path).map_err(|error| {
            IosError::storage(format!(
                "failed to open staged iOS bridge file '{}': {error}",
                path.display()
            ))
        })?;
        file.sync_all().map_err(|error| {
            IosError::storage(format!(
                "failed to sync staged iOS bridge file '{}': {error}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

fn promote_generated_directory(staging: tempfile::TempDir, target: &Path) -> Result<(), IosError> {
    match fs::symlink_metadata(target) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() {
                return Err(IosError::storage(format!(
                    "iOS bridge output '{}' is not a regular non-symlink directory",
                    target.display()
                )));
            }
            exchange_directories(staging.path(), target).map_err(|error| {
                IosError::storage(format!(
                    "failed to atomically exchange iOS bridge output '{}': {error}",
                    target.display()
                ))
            })?;
            drop(staging);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            rename_directory_noreplace(staging.path(), target).map_err(|error| {
                IosError::storage(format!(
                    "failed to atomically publish iOS bridge output '{}': {error}",
                    target.display()
                ))
            })?;
            let _ = staging.keep();
            Ok(())
        }
        Err(error) => Err(IosError::storage(format!(
            "failed to inspect iOS bridge output '{}': {error}",
            target.display()
        ))),
    }
}

#[cfg(any(
    target_os = "android",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "redox",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos"
))]
fn exchange_directories(left: &Path, right: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, left, CWD, right, RenameFlags::EXCHANGE).map_err(io::Error::from)
}

#[cfg(windows)]
fn exchange_directories(left: &Path, right: &Path) -> io::Result<()> {
    let parent = right
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing output parent"))?;
    let backup_container = tempfile::Builder::new()
        .prefix(".vesper-ios-bridge-exchange-")
        .tempdir_in(parent)?;
    let backup = backup_container.path().join("previous");
    fs::rename(right, &backup)?;
    if let Err(error) = fs::rename(left, right) {
        if let Err(rollback) = fs::rename(&backup, right) {
            let preserved = backup_container.keep();
            return Err(io::Error::other(format!(
                "{error}; rollback failed: {rollback}; previous output preserved under '{}'",
                preserved.display()
            )));
        }
        return Err(error);
    }
    if let Err(error) = fs::rename(&backup, left) {
        let replacement_rollback = fs::rename(right, left);
        let previous_rollback = fs::rename(&backup, right);
        if replacement_rollback.is_err() || previous_rollback.is_err() {
            let preserved = backup_container.keep();
            return Err(io::Error::other(format!(
                "{error}; incomplete rollback; recovery data preserved under '{}'",
                preserved.display()
            )));
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(not(any(
    target_os = "android",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "redox",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    windows
)))]
fn exchange_directories(_left: &Path, _right: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "directory exchange is unsupported on this host",
    ))
}

#[cfg(any(
    target_os = "android",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "redox",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos"
))]
fn rename_directory_noreplace(source: &Path, target: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, source, CWD, target, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

#[cfg(windows)]
fn rename_directory_noreplace(source: &Path, target: &Path) -> io::Result<()> {
    fs::rename(source, target)
}

#[cfg(not(any(
    target_os = "android",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "redox",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    windows
)))]
fn rename_directory_noreplace(_source: &Path, _target: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no-replace directory rename is unsupported on this host",
    ))
}

fn write_unified_diff(
    output: &mut dyn Write,
    left_path: &Path,
    left: &str,
    right_path: &Path,
    right: &str,
) -> Result<(), IosError> {
    let left_lines = left.lines().collect::<Vec<_>>();
    let right_lines = right.lines().collect::<Vec<_>>();
    let common_prefix = left_lines
        .iter()
        .zip(&right_lines)
        .take_while(|(left, right)| left == right)
        .count();
    let maximum_suffix = left_lines
        .len()
        .saturating_sub(common_prefix)
        .min(right_lines.len().saturating_sub(common_prefix));
    let common_suffix = left_lines
        .iter()
        .rev()
        .zip(right_lines.iter().rev())
        .take(maximum_suffix)
        .take_while(|(left, right)| left == right)
        .count();
    let context_start = common_prefix.saturating_sub(3);
    let left_change_end = left_lines.len().saturating_sub(common_suffix);
    let right_change_end = right_lines.len().saturating_sub(common_suffix);
    let left_context_end = left_change_end.saturating_add(3).min(left_lines.len());
    let right_context_end = right_change_end.saturating_add(3).min(right_lines.len());

    writeln!(output, "--- {}", left_path.display()).map_err(output_error)?;
    writeln!(output, "+++ {}", right_path.display()).map_err(output_error)?;
    writeln!(
        output,
        "@@ -{},{} +{},{} @@",
        context_start + 1,
        left_context_end.saturating_sub(context_start),
        context_start + 1,
        right_context_end.saturating_sub(context_start)
    )
    .map_err(output_error)?;
    for line in &left_lines[context_start..common_prefix] {
        writeln!(output, " {line}").map_err(output_error)?;
    }
    for line in &left_lines[common_prefix..left_change_end] {
        writeln!(output, "-{line}").map_err(output_error)?;
    }
    for line in &right_lines[common_prefix..right_change_end] {
        writeln!(output, "+{line}").map_err(output_error)?;
    }
    for line in &left_lines[left_change_end..left_context_end] {
        writeln!(output, " {line}").map_err(output_error)?;
    }
    Ok(())
}

fn output_error(error: io::Error) -> IosError {
    IosError::worker(format!("failed to write iOS command output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apple_registry_json(framework_name: &str, bundle_identifier: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "target": IOS_DEVICE_PLUGIN_TARGET,
            "architecture": IOS_PLUGIN_ARCHITECTURE,
            "minimum_os": "17.0",
            "artifacts": [{
                "plugin_id": "dev.vesper.fixture",
                "transport": "native",
                "locator": {
                    "kind": "apple-framework",
                    "name": framework_name,
                    "bundle_identifier": bundle_identifier
                },
                "integrity": {
                    "kind": "apple-code-signature",
                    "validation": "same-team-as-host-or-simulator-ad-hoc"
                },
                "package": {
                    "version": "1.0.0",
                    "publisher": "dev.vesper.publisher",
                    "descriptor_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                },
                "capabilities": [{
                    "interface_id": "e9479dbc-42d2-575e-b39e-a24bc512fbc7",
                    "instance_id": "dev.vesper.fixture.post-download",
                    "interface_major": 1,
                    "interface_minor": 0
                }]
            }]
        }))
        .expect("serialize registry fixture")
    }

    fn parsed_apple_registry(
        framework_name: &str,
        bundle_identifier: &str,
    ) -> EmbeddedPluginRegistry {
        EmbeddedPluginRegistry::parse(
            &apple_registry_json(framework_name, bundle_identifier),
            IOS_DEVICE_PLUGIN_TARGET,
            IOS_PLUGIN_ARCHITECTURE,
        )
        .expect("parse registry fixture")
    }

    #[test]
    fn registry_fragment_binding_preserves_framework_identity() {
        let registry = parsed_apple_registry("FixturePlugin", "dev.vesper.fixture-plugin");
        let fragment = Path::new("FixturePlugin.framework/vesper-plugin-registry.json");

        assert_eq!(
            validate_registry_fragment_binding(&registry, "FixturePlugin", fragment)
                .expect("matching framework binding"),
            "dev.vesper.fixture-plugin"
        );
        let error = validate_registry_fragment_binding(&registry, "OtherPlugin", fragment)
            .expect_err("locator drift must fail");
        assert_eq!(error.kind(), IosErrorKind::Conformance);
        assert!(error.to_string().contains("locator"));
    }

    #[test]
    fn registry_framework_metadata_rejects_bundle_and_minimum_os_drift() {
        let registry = parsed_apple_registry("FixturePlugin", "dev.vesper.fixture-plugin");
        let fragment = Path::new("FixturePlugin.framework/vesper-plugin-registry.json");
        let matching = FrameworkBundleMetadata {
            bundle_identifier: "dev.vesper.fixture-plugin".to_owned(),
            minimum_os: "17.0".to_owned(),
        };
        validate_registry_framework_metadata(
            &registry,
            "dev.vesper.fixture-plugin",
            &matching,
            fragment,
        )
        .expect("matching registry and plist metadata");

        let wrong_bundle = FrameworkBundleMetadata {
            bundle_identifier: "dev.vesper.other".to_owned(),
            ..matching.clone()
        };
        let error = validate_registry_framework_metadata(
            &registry,
            "dev.vesper.fixture-plugin",
            &wrong_bundle,
            fragment,
        )
        .expect_err("bundle identifier drift must fail");
        assert!(error.to_string().contains("bundle identifier"));

        let wrong_minimum_os = FrameworkBundleMetadata {
            minimum_os: "18.0".to_owned(),
            ..matching
        };
        let error = validate_registry_framework_metadata(
            &registry,
            "dev.vesper.fixture-plugin",
            &wrong_minimum_os,
            fragment,
        )
        .expect_err("minimum OS drift must fail");
        assert!(error.to_string().contains("minimum OS"));
    }

    #[test]
    fn vtool_build_metadata_parser_supports_modern_and_legacy_ios_commands() {
        let modern = parse_vtool_build_metadata(
            "Fixture:\nLoad command 9\n      cmd LC_BUILD_VERSION\n  cmdsize 32\n platform IOSSIMULATOR\n    minos 17.0\n      sdk 26.5\n",
            "modern fixture",
        )
        .expect("modern build metadata");
        assert_eq!(modern.platform, "IOSSIMULATOR");
        assert_eq!(modern.minimum_os, "17.0");

        let legacy = parse_vtool_build_metadata(
            "Fixture:\nLoad command 8\n      cmd LC_VERSION_MIN_IPHONEOS\n  cmdsize 16\n  version 17.0\n      sdk 26.5\n",
            "legacy fixture",
        )
        .expect("legacy iOS build metadata");
        assert_eq!(legacy.platform, "IOS");
        assert_eq!(legacy.minimum_os, "17.0");

        assert_eq!(
            normalize_apple_version("17", "fixture").expect("major version"),
            (17, 0, 0)
        );
        assert_eq!(
            normalize_apple_version("17.0.0", "fixture").expect("three-component version"),
            (17, 0, 0)
        );
    }

    #[test]
    fn vtool_build_metadata_parser_rejects_missing_repeated_and_invalid_fields() {
        for output in [
            "Fixture:\nLoad command 9\n cmd LC_BUILD_VERSION\n platform IOS\n",
            "Fixture:\nLoad command 9\n cmd LC_BUILD_VERSION\n platform IOS\n platform IOS\n minos 17.0\n",
            "Fixture:\nLoad command 9\n cmd LC_BUILD_VERSION\n platform IOS\n minos seventeen\n",
        ] {
            assert!(parse_vtool_build_metadata(output, "invalid fixture").is_err());
        }
        for version in ["", "17.0.0.0", "17.beta", "17..0"] {
            assert!(normalize_apple_version(version, "invalid fixture").is_err());
        }
    }

    #[test]
    fn code_signature_metadata_parser_requires_one_exact_identity_record() {
        let path = Path::new("FixturePlugin.framework");
        let metadata = parse_code_signature_metadata(
            "Executable=/tmp/FixturePlugin\nIdentifier=dev.vesper.fixture-plugin\nTeamIdentifier=ABCDE12345\n",
            path,
        )
        .expect("parse code signature metadata");
        assert_eq!(metadata.identifier, "dev.vesper.fixture-plugin");
        assert_eq!(metadata.team_identifier, "ABCDE12345");

        let repeated = parse_code_signature_metadata(
            "Identifier=dev.vesper.fixture-plugin\nIdentifier=dev.vesper.fixture-plugin\nTeamIdentifier=ABCDE12345\n",
            path,
        )
        .expect_err("repeated identity must fail");
        assert!(repeated.to_string().contains("Repeated"));

        let missing = parse_code_signature_metadata("Identifier=dev.vesper.fixture-plugin\n", path)
            .expect_err("missing team must fail");
        assert!(missing.to_string().contains("TeamIdentifier"));
    }

    #[test]
    fn framework_signature_identity_rejects_wrong_identifier_and_team() {
        let framework = ValidatedFramework {
            path: PathBuf::from("FixturePlugin.framework"),
            bundle_identifier: "dev.vesper.fixture-plugin".to_owned(),
            minimum_os: "17.0".to_owned(),
        };
        let matching = CodeSignatureMetadata {
            identifier: framework.bundle_identifier.clone(),
            team_identifier: "ABCDE12345".to_owned(),
        };
        validate_framework_signature_identity(&framework, "ABCDE12345", &matching)
            .expect("matching signature identity");

        let wrong_identifier = CodeSignatureMetadata {
            identifier: "dev.vesper.other".to_owned(),
            ..matching.clone()
        };
        let error =
            validate_framework_signature_identity(&framework, "ABCDE12345", &wrong_identifier)
                .expect_err("wrong signing identifier must fail");
        assert!(error.to_string().contains("Identifier"));

        let wrong_team = CodeSignatureMetadata {
            team_identifier: "OTHER67890".to_owned(),
            ..matching
        };
        let error = validate_framework_signature_identity(&framework, "ABCDE12345", &wrong_team)
            .expect_err("wrong signing team must fail");
        assert!(error.to_string().contains("TeamIdentifier"));
    }

    #[test]
    fn optional_framework_load_command_detection_requires_exact_framework_identity() {
        assert!(dependency_loads_framework(
            "@rpath/FixturePlugin.framework/FixturePlugin",
            "FixturePlugin"
        ));
        assert!(dependency_loads_framework(
            "@executable_path/Frameworks/FixturePlugin.framework/FixturePlugin",
            "FixturePlugin"
        ));
        assert!(!dependency_loads_framework(
            "@rpath/FixturePlugin.framework/FixturePluginHelper",
            "FixturePlugin"
        ));
        assert!(!dependency_loads_framework(
            "@rpath/OtherFixturePlugin.framework/FixturePlugin",
            "FixturePlugin"
        ));
    }

    #[test]
    fn otool_dependency_parser_preserves_paths_with_spaces() {
        let dependencies = parse_otool_dependencies(
            "/tmp/Fixture With Spaces.framework/Fixture With Spaces:\n\
\t@rpath/VesperFFmpegAVCodec.framework/VesperFFmpegAVCodec (compatibility version 1.0.0, current version 1.0.0)\n\
\t@rpath/Support Files/UnexpectedSibling.framework/UnexpectedSibling (compatibility version 1.0.0, current version 1.0.0)\n\
\t/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation (compatibility version 300.0.0, current version 3100.0.0)\n",
            "fixture binary",
        )
        .expect("parse otool dependencies");

        assert_eq!(
            dependencies,
            [
                "@rpath/VesperFFmpegAVCodec.framework/VesperFFmpegAVCodec",
                "@rpath/Support Files/UnexpectedSibling.framework/UnexpectedSibling",
                "/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation",
            ]
        );

        let error = parse_otool_dependencies(
            "fixture binary:\n\t@rpath/Malformed.framework/Malformed\n",
            "fixture binary",
        )
        .expect_err("malformed dependency record must fail");
        assert!(
            error
                .to_string()
                .contains("Malformed otool dependency record")
        );
    }

    #[test]
    fn framework_dependency_validation_enforces_exact_allowlist() {
        let valid = [
            "@rpath/VesperFFmpegAVCodec.framework/VesperFFmpegAVCodec".to_owned(),
            "@rpath/VesperFFmpegAVUtil.framework/VesperFFmpegAVUtil".to_owned(),
            "/usr/lib/libSystem.B.dylib".to_owned(),
            "/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation".to_owned(),
        ];
        validate_framework_dependency_list(
            "VesperFFmpegAVFormat binary",
            "VesperFFmpegAVFormat",
            &valid,
        )
        .expect("accept required framework and system dependencies");

        let missing = ["@rpath/VesperFFmpegAVCodec.framework/VesperFFmpegAVCodec".to_owned()];
        let error = validate_framework_dependency_list(
            "VesperFFmpegAVFormat binary",
            "VesperFFmpegAVFormat",
            &missing,
        )
        .expect_err("missing required framework dependency must fail");
        assert!(error.to_string().contains("VesperFFmpegAVUtil"));

        let unexpected = ["@rpath/Unexpected.framework/Unexpected".to_owned()];
        let error = validate_framework_dependency_list(
            "FixturePlugin binary",
            "FixturePlugin",
            &unexpected,
        )
        .expect_err("unexpected non-system dependency must fail");
        assert!(
            error
                .to_string()
                .contains("unexpected non-system dynamic dependency")
        );

        let unwrapped = ["/opt/ffmpeg/lib/libavcodec.62.dylib".to_owned()];
        let error =
            validate_framework_dependency_list("FixturePlugin binary", "FixturePlugin", &unwrapped)
                .expect_err("unwrapped FFmpeg dependency must fail");
        assert!(error.to_string().contains("Unwrapped FFmpeg dependency"));
    }

    #[test]
    fn app_bundle_executable_name_rejects_path_traversal() {
        let plist = Path::new("Fixture.app/Info.plist");
        validate_bundle_executable_name("Fixture", plist).expect("plain executable name");
        assert!(validate_bundle_executable_name("../Fixture", plist).is_err());
        assert!(validate_bundle_executable_name("Subdirectory/Fixture", plist).is_err());
        assert!(validate_bundle_executable_name("", plist).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn app_store_preflight_turns_sigint_into_a_worker_error() {
        use nix::sys::signal::{Signal, raise};

        const CHILD_ENV: &str = "VESPER_IOS_PREFLIGHT_SIGINT_FIXTURE";
        if env::var_os(CHILD_ENV).is_some() {
            let directory = tempfile::tempdir().expect("temporary App Store preflight fixture");
            let app = directory.path().join("Fixture.app");
            fs::create_dir_all(&app).expect("create App Store preflight fixture");
            let cancellation =
                external_process::InterruptDeferral::start("iOS App Store layout preflight test")
                    .expect("start App Store preflight cancellation scope");
            raise(Signal::SIGINT).expect("raise App Store preflight cancellation");

            let error = scan_app_bundle(&app, &cancellation)
                .expect_err("cancelled App Store preflight must stop scanning");
            assert_eq!(error.kind(), IosErrorKind::Worker);
            assert!(error.to_string().contains("was cancelled"));
            assert!(cancellation.finish());
            return;
        }

        let status = Command::new(env::current_exe().expect("locate iOS unit test executable"))
            .args([
                "--exact",
                "ios::tests::app_store_preflight_turns_sigint_into_a_worker_error",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .status()
            .expect("run isolated App Store preflight cancellation fixture");
        assert!(status.success());
    }

    #[cfg(unix)]
    #[test]
    fn app_store_deny_names_preserve_ascii_suffixes_around_non_utf8_bytes() {
        use std::os::unix::ffi::OsStringExt;

        let dylib = OsString::from_vec(b"libunexpected\xff.dylib.1".to_vec());
        let framework = OsString::from_vec(b"Nested\xff.framework".to_vec());
        assert!(is_standalone_dylib_name(&dylib));
        assert!(is_nested_framework_directory_name(&framework));
    }

    #[test]
    fn nm_parser_uses_only_symbol_table_lines() {
        let symbols = parse_nm_symbols(
            b"00000000 T _player_ffi_present\nplain player_ffi_second\nplayer_ffi_in_a_string\n",
        );
        assert!(symbols.contains("player_ffi_present"));
        assert!(!symbols.contains("player_ffi_second"));
        assert!(!symbols.contains("player_ffi_in_a_string"));
    }

    #[test]
    fn nm_metadata_warning_is_narrowly_recognized() {
        assert!(is_known_nm_metadata_warning(
            "nm: error: archive(member): Unknown attribute kind (105) (Producer: 'LLVM' Reader: 'APPLE')\n"
        ));
        assert!(!is_known_nm_metadata_warning("nm: archive is malformed\n"));
        assert!(!is_known_nm_metadata_warning(""));
    }
}
