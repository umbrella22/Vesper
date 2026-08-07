// Release commands retain non-macOS compatibility stubs while their staging
// helpers are used only by the macOS implementation.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::{BTreeSet, VecDeque};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
#[cfg(target_os = "macos")]
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::external_process;
use crate::ffmpeg_source::{FfmpegSourceLock, FfmpegSourcePolicy, FfmpegSourcePolicyErrorKind};
use crate::ios::IosError;

const CORE_RELEASE_ASSETS: [&str; 3] = [
    "VesperPlayerKit-ios-arm64.framework.zip",
    "VesperPlayerKit-ios-simulator-arm64.framework.zip",
    "VesperPlayerKit.xcframework.zip",
];
const OPTIONAL_RELEASE_FRAMEWORKS: [&str; 7] = [
    "VesperFFmpegAVCodec",
    "VesperFFmpegAVFormat",
    "VesperFFmpegAVUtil",
    "VesperPlayerRemuxFfmpegPlugin",
    "VesperPlayerSourceNormalizerFfmpegPlugin",
    "VesperPlayerDecoderVideoToolboxPlugin",
    "VesperPlayerFrameProcessorDiagnosticPlugin",
];
const OPTIONAL_COMPLIANCE_ASSET: &str = "VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip";
const LEGACY_OPTIONAL_RUNTIME_ASSET: &str = "VesperPlayerFfmpegRuntime.xcframework.zip";
const OPTIONAL_AGGREGATE_INPUT_DIRECTORY: &str = "ios-optional-release-inputs-v1";
const FRAMEWORK_NAME: &str = "VesperPlayerKit.framework";
const BINARY_NAME: &str = "VesperPlayerKit";
const MAX_RELEASE_TREE_ENTRIES: usize = 100_000;
const MAX_RELEASE_TREE_DEPTH: usize = 64;
const MAX_RELEASE_TREE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_RELEASE_ASSET_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_RELEASE_DIRECTORY_ENTRIES: usize = 4096;
const MAX_RELEASE_TOOL_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_LIPO_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_RELEASE_JOURNAL_BYTES: u64 = 64 * 1024;
const RELEASE_JOURNAL_VERSION: u32 = 1;
const RELEASE_JOURNAL_FILE: &str = "ios-stage-release-transaction.json";

pub(crate) fn canonical_ffmpeg_release_source_lock(
    root: &Path,
) -> Result<FfmpegSourceLock, IosError> {
    let policy = FfmpegSourcePolicy::load(root).map_err(|error| match error.kind() {
        FfmpegSourcePolicyErrorKind::Storage => IosError::storage(error.to_string()),
        FfmpegSourcePolicyErrorKind::Invalid => IosError::conformance(error.to_string()),
    })?;
    Ok(policy.release().clone())
}

struct ReleaseLock {
    _file: File,
}

impl ReleaseLock {
    fn acquire(root: &Path) -> Result<Self, IosError> {
        let digest = Sha256::digest(root.as_os_str().as_encoded_bytes());
        let directory = env::temp_dir().join("vesper-player-cli-locks");
        fs::create_dir_all(&directory).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS release lock directory '{}': {error}",
                directory.display()
            ))
        })?;
        let path = directory.join(format!("ios-release-{}.lock", hex::encode(digest)));
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to open iOS release lock '{}': {error}",
                    path.display()
                ))
            })?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(IosError::compatibility(format!(
                "another iOS release staging command is already active for '{}'",
                root.display()
            ))),
            Err(TryLockError::Error(error)) => Err(IosError::storage(format!(
                "failed to lock iOS release staging for '{}': {error}",
                root.display()
            ))),
        }
    }
}

struct PreparedDirectory {
    path: PathBuf,
    identity: FileIdentity,
    parent: PathBuf,
    parent_identity: FileIdentity,
    created: Vec<(PathBuf, FileIdentity)>,
    committed: bool,
}

impl PreparedDirectory {
    fn prepare(path: &Path, label: &str) -> Result<Self, IosError> {
        let absolute = absolute_path(path, label)?;
        reject_symlink_components(&absolute, label)?;

        let mut ancestor = absolute.clone();
        let mut missing = Vec::new();
        loop {
            match fs::symlink_metadata(&ancestor) {
                Ok(metadata) => {
                    if !metadata.file_type().is_dir() {
                        return Err(IosError::storage(format!(
                            "{label} '{}' is not a regular non-symlink directory",
                            ancestor.display()
                        )));
                    }
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let name = ancestor.file_name().ok_or_else(|| {
                        IosError::storage(format!(
                            "{label} '{}' has no existing directory ancestor",
                            absolute.display()
                        ))
                    })?;
                    missing.push(name.to_os_string());
                    if !ancestor.pop() {
                        return Err(IosError::storage(format!(
                            "{label} '{}' has no existing directory ancestor",
                            absolute.display()
                        )));
                    }
                }
                Err(error) => {
                    return Err(IosError::storage(format!(
                        "failed to inspect {label} '{}': {error}",
                        ancestor.display()
                    )));
                }
            }
        }

        let mut current = fs::canonicalize(&ancestor).map_err(|error| {
            IosError::storage(format!(
                "failed to resolve {label} ancestor '{}': {error}",
                ancestor.display()
            ))
        })?;
        let mut created = Vec::with_capacity(missing.len());
        for name in missing.into_iter().rev() {
            current.push(name);
            fs::create_dir(&current).map_err(|error| {
                IosError::storage(format!(
                    "failed to create {label} '{}': {error}",
                    current.display()
                ))
            })?;
            let created_identity = directory_identity(&current, label)?;
            created.push((current.clone(), created_identity));
        }
        let parent = current.parent().map(Path::to_path_buf).ok_or_else(|| {
            IosError::storage(format!(
                "{label} '{}' must not be a filesystem root",
                current.display()
            ))
        })?;
        let identity = directory_identity(&current, label)?;
        let parent_identity = directory_identity(&parent, &format!("{label} parent"))?;
        Ok(Self {
            path: current,
            identity,
            parent,
            parent_identity,
            created,
            committed: false,
        })
    }

    fn validate(&self, label: &str) -> Result<(), IosError> {
        if directory_identity(&self.parent, &format!("{label} parent"))? != self.parent_identity
            || directory_identity(&self.path, label)? != self.identity
        {
            return Err(IosError::storage(format!(
                "{label} '{}' changed after validation",
                self.path.display()
            )));
        }
        Ok(())
    }

    fn commit(mut self) {
        self.committed = true;
    }

    fn commit_durable(self, label: &str) -> Result<(), IosError> {
        self.commit_durable_with_sync(label, sync_directory)
    }

    fn commit_durable_with_sync(
        mut self,
        label: &str,
        mut sync: impl FnMut(&Path) -> io::Result<()>,
    ) -> Result<(), IosError> {
        self.validate(label)?;
        for (path, identity) in &self.created {
            if directory_identity(path, label)? != *identity {
                return Err(IosError::storage(format!(
                    "{label} '{}' changed before durable commit",
                    path.display()
                )));
            }
        }

        let mut paths = vec![self.path.clone()];
        for (path, _) in self.created.iter().rev() {
            let parent = path.parent().ok_or_else(|| {
                IosError::storage(format!(
                    "{label} '{}' has no parent to synchronize",
                    path.display()
                ))
            })?;
            if paths.last().is_none_or(|previous| previous != parent) {
                paths.push(parent.to_path_buf());
            }
        }
        if self.created.is_empty() && paths.last() != Some(&self.parent) {
            paths.push(self.parent.clone());
        }
        for path in paths {
            sync(&path).map_err(|error| {
                IosError::storage(format!(
                    "failed to synchronize {label} '{}': {error}",
                    path.display()
                ))
            })?;
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for PreparedDirectory {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (path, identity) in self.created.iter().rev() {
            if directory_identity(path, "created iOS release directory").ok() == Some(*identity) {
                let _ = fs::remove_dir(path);
            }
        }
    }
}

pub(crate) fn stage_release(
    root: &Path,
    output_directory: Option<&Path>,
    include_optional_plugins: bool,
    package_artifacts_directory: Option<&Path>,
    package_artifacts_explicit: bool,
    output: &mut dyn Write,
) -> Result<(), IosError> {
    require_macos_stage_host()?;
    let _lock = ReleaseLock::acquire(root)?;
    let state_directory = PreparedDirectory::prepare(
        &root.join("lib/ios/VesperPlayerKit/.build/vesper-cli-state"),
        "iOS release transaction state",
    )?;
    let journal_path = state_directory.path.join(RELEASE_JOURNAL_FILE);
    recover_release_journal_interruptible(root, &journal_path)?;

    let requested_output = output_directory
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("dist/release/ios"));
    let output_directory = PreparedDirectory::prepare(&requested_output, "iOS release output")?;
    validate_release_output_location(root, &output_directory.path)?;
    let output_snapshot = directory_snapshot(&output_directory.path, "iOS release output")?;

    let default_package_artifacts = root.join("lib/ios/VesperPlayerOptionalPlugins/Artifacts");
    let requested_package_artifacts = package_artifacts_directory
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_package_artifacts.clone());
    let package_parent_path = requested_package_artifacts.parent().ok_or_else(|| {
        IosError::storage(format!(
            "iOS optional package artifacts path '{}' has no parent",
            requested_package_artifacts.display()
        ))
    })?;
    let package_parent = include_optional_plugins
        .then(|| PreparedDirectory::prepare(package_parent_path, "iOS package artifacts parent"))
        .transpose()?;
    let package_target = if let Some(parent) = package_parent.as_ref() {
        let name = requested_package_artifacts.file_name().ok_or_else(|| {
            IosError::storage(format!(
                "iOS optional package artifacts path '{}' has no file name",
                requested_package_artifacts.display()
            ))
        })?;
        let target = parent.path.join(name);
        validate_package_artifacts_location(
            root,
            &default_package_artifacts,
            &target,
            package_artifacts_explicit,
        )?;
        validate_non_overlapping_paths(&output_directory.path, &target)?;
        validate_existing_package_target(&target)?;
        Some(target)
    } else {
        None
    };
    let package_snapshot = package_target
        .as_deref()
        .map(|target| optional_directory_snapshot(target, "iOS package artifacts target"))
        .transpose()?;

    let build_root = root.join("lib/ios/VesperPlayerKit/.build/xcframework");
    let mut build_output = Vec::new();
    let mut build_diagnostics = Vec::new();
    let build_result =
        crate::ios_kit::build_for_release(root, &mut build_output, &mut build_diagnostics);
    let mut diagnostics = io::stderr().lock();
    diagnostics
        .write_all(&build_diagnostics)
        .map_err(output_error)?;
    build_result?;
    diagnostics.write_all(&build_output).map_err(output_error)?;
    output_directory.validate("iOS release output")?;
    validate_directory_snapshot(
        &output_directory.path,
        &output_snapshot,
        "iOS release output",
    )?;
    if let Some(parent) = package_parent.as_ref() {
        parent.validate("iOS package artifacts parent")?;
    }
    if let (Some(target), Some(expected)) = (package_target.as_deref(), package_snapshot.as_ref()) {
        validate_optional_directory_snapshot(target, expected, "iOS package artifacts target")?;
    }

    let device_framework = build_root.join(format!(
        "VesperPlayerKit-iOS.xcarchive/Products/Library/Frameworks/{FRAMEWORK_NAME}"
    ));
    let simulator_framework = build_root.join(format!(
        "VesperPlayerKit-iOS-Simulator.xcarchive/Products/Library/Frameworks/{FRAMEWORK_NAME}"
    ));
    let xcframework = build_root.join("VesperPlayerKit.xcframework");
    for (path, label) in [
        (&device_framework, "device VesperPlayerKit framework"),
        (&simulator_framework, "Simulator VesperPlayerKit framework"),
        (&xcframework, "VesperPlayerKit XCFramework"),
    ] {
        require_repository_directory(root, path, label)?;
        validate_tree(path, label)?;
    }

    let release_stage = tempfile::Builder::new()
        .prefix(".vesper-ios-release-stage-")
        .tempdir_in(&output_directory.parent)
        .map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS release staging directory beside '{}': {error}",
                output_directory.path.display()
            ))
        })?;
    stage_framework_archive(
        &device_framework,
        release_stage.path().join(CORE_RELEASE_ASSETS[0]).as_path(),
        FrameworkSlice::Device,
    )?;
    stage_framework_archive(
        &simulator_framework,
        release_stage.path().join(CORE_RELEASE_ASSETS[1]).as_path(),
        FrameworkSlice::Simulator,
    )?;
    create_zip(
        &xcframework,
        &release_stage.path().join(CORE_RELEASE_ASSETS[2]),
        "VesperPlayerKit XCFramework archive",
    )?;

    let package_stage = if let (true, Some(package_target_path)) =
        (include_optional_plugins, package_target.as_ref())
    {
        let parent = package_target_path.parent().ok_or_else(|| {
            IosError::storage("iOS optional package artifacts target has no parent")
        })?;
        let stage = tempfile::Builder::new()
            .prefix(".vesper-ios-package-stage-")
            .tempdir_in(parent)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create optional iOS package staging directory beside '{}': {error}",
                    package_target_path.display()
                ))
            })?;
        let content = stage.path().join("Artifacts");
        fs::create_dir(&content).map_err(|error| {
            IosError::storage(format!(
                "failed to create optional iOS package staging content '{}': {error}",
                content.display()
            ))
        })?;

        stage_optional_release_bundle(root, release_stage.path(), &content)?;
        output_directory.validate("iOS release output")?;
        validate_directory_snapshot(
            &output_directory.path,
            &output_snapshot,
            "iOS release output",
        )?;
        if let Some(parent) = package_parent.as_ref() {
            parent.validate("iOS package artifacts parent")?;
        }
        if let (Some(target), Some(expected)) =
            (package_target.as_deref(), package_snapshot.as_ref())
        {
            validate_optional_directory_snapshot(target, expected, "iOS package artifacts target")?;
        }
        validate_package_stage(&content)?;
        Some((stage, content))
    } else {
        None
    };

    let staged_assets = validate_release_stage(release_stage.path(), include_optional_plugins)?;
    promote_release_outputs(
        release_stage,
        &output_directory.path,
        &staged_assets,
        include_optional_plugins,
        package_stage,
        package_target.as_deref(),
        output_directory.identity,
        output_directory.parent_identity,
        output_snapshot,
        package_parent.as_ref().map(|parent| parent.identity),
        package_snapshot.flatten(),
        root,
        &journal_path,
        state_directory.identity,
    )?;

    if !include_optional_plugins {
        writeln!(
            output,
            "Skipped optional iOS plugin XCFrameworks. Set VESPER_IOS_INCLUDE_OPTIONAL_PLUGINS=1 to stage them."
        )
        .map_err(output_error)?;
    }
    writeln!(output, "Staged VesperPlayerKit iOS release assets into:")
        .and_then(|()| writeln!(output, "  {}", requested_output.display()))
        .map_err(output_error)?;

    output_directory.commit();
    if let Some(parent) = package_parent {
        parent.commit();
    }
    state_directory.commit();
    Ok(())
}

pub(crate) fn stage_optional_plugins_release(
    root: &Path,
    arguments: Vec<OsString>,
    output: &mut dyn Write,
) -> Result<(), IosError> {
    require_macos_stage_host()?;
    let request = parse_optional_release_arguments(root, arguments)?;
    if request.dry_run {
        return write_optional_release_dry_run(&request, output);
    }

    let _lock = ReleaseLock::acquire(root)?;
    let state_directory = PreparedDirectory::prepare(
        &root.join("lib/ios/VesperPlayerKit/.build/vesper-cli-state"),
        "iOS release transaction state",
    )?;
    let journal_path = state_directory.path.join(RELEASE_JOURNAL_FILE);
    recover_release_journal_interruptible(root, &journal_path)?;

    let output_directory =
        PreparedDirectory::prepare(&request.output_directory, "optional iOS release output")?;
    validate_release_output_location(root, &output_directory.path)?;
    let output_snapshot =
        directory_snapshot(&output_directory.path, "optional iOS release output")?;

    let default_package_artifacts = root.join("lib/ios/VesperPlayerOptionalPlugins/Artifacts");
    let package_artifacts_explicit = env::var_os("VESPER_IOS_OPTIONAL_PACKAGE_ARTIFACTS_DIR");
    let requested_package_artifacts = package_artifacts_explicit
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_package_artifacts.clone());
    let package_target = if requested_package_artifacts.is_absolute() {
        requested_package_artifacts
    } else {
        root.join(requested_package_artifacts)
    };
    let package_parent_path = package_target.parent().ok_or_else(|| {
        IosError::storage(format!(
            "iOS optional package artifacts path '{}' has no parent",
            package_target.display()
        ))
    })?;
    let package_parent =
        PreparedDirectory::prepare(package_parent_path, "iOS package artifacts parent")?;
    let package_name = package_target.file_name().ok_or_else(|| {
        IosError::storage(format!(
            "iOS optional package artifacts path '{}' has no file name",
            package_target.display()
        ))
    })?;
    let package_target = package_parent.path.join(package_name);
    validate_package_artifacts_location(
        root,
        &default_package_artifacts,
        &package_target,
        package_artifacts_explicit.is_some(),
    )?;
    validate_non_overlapping_paths(&output_directory.path, &package_target)?;
    validate_existing_package_target(&package_target)?;
    let package_snapshot =
        optional_directory_snapshot(&package_target, "iOS package artifacts target")?;

    let release_stage = tempfile::Builder::new()
        .prefix(".vesper-ios-release-stage-")
        .tempdir_in(&output_directory.parent)
        .map_err(|error| {
            IosError::storage(format!(
                "failed to create optional iOS release staging directory beside '{}': {error}",
                output_directory.path.display()
            ))
        })?;
    let package_owner = tempfile::Builder::new()
        .prefix(".vesper-ios-package-stage-")
        .tempdir_in(&package_parent.path)
        .map_err(|error| {
            IosError::storage(format!(
                "failed to create optional iOS package staging directory beside '{}': {error}",
                package_target.display()
            ))
        })?;
    let package_stage = package_owner.path().join("Artifacts");
    fs::create_dir(&package_stage).map_err(|error| {
        IosError::storage(format!(
            "failed to create optional iOS package staging directory '{}': {error}",
            package_stage.display()
        ))
    })?;
    stage_optional_release_bundle_with_profile(
        root,
        release_stage.path(),
        &package_stage,
        &request.profile,
    )?;
    let staged_assets = validate_optional_release_stage(release_stage.path())?;
    validate_package_stage(&package_stage)?;
    output_directory.validate("optional iOS release output")?;
    validate_directory_snapshot(
        &output_directory.path,
        &output_snapshot,
        "optional iOS release output",
    )?;
    package_parent.validate("iOS package artifacts parent")?;
    validate_optional_directory_snapshot(
        &package_target,
        &package_snapshot,
        "iOS package artifacts target",
    )?;
    promote_release_outputs(
        release_stage,
        &output_directory.path,
        &staged_assets,
        true,
        Some((package_owner, package_stage)),
        Some(&package_target),
        output_directory.identity,
        output_directory.parent_identity,
        output_snapshot,
        Some(package_parent.identity),
        package_snapshot,
        root,
        &journal_path,
        state_directory.identity,
    )?;
    output_directory.commit();
    package_parent.commit();
    state_directory.commit();

    writeln!(output, "Staged optional iOS plugin release assets into:")
        .and_then(|()| writeln!(output, "  {}", request.output_directory.display()))
        .map_err(output_error)
}

struct OptionalReleaseRequest {
    output_directory: PathBuf,
    profile: String,
    dry_run: bool,
}

fn parse_optional_release_arguments(
    root: &Path,
    arguments: Vec<OsString>,
) -> Result<OptionalReleaseRequest, IosError> {
    let mut output_directory = root.join("dist/release/ios");
    let mut profile = "source-normalizer".to_owned();
    let mut dry_run = false;
    let mut slices = BTreeSet::new();
    let mut index = 0;
    if let Some(first) = arguments.first()
        && !first.to_string_lossy().starts_with("--")
        && !matches!(first.to_str(), Some("ios-arm64" | "ios-simulator-arm64"))
    {
        output_directory = absolute_path(Path::new(first), "optional iOS release output")?;
        index = 1;
    }
    while index < arguments.len() {
        let value = arguments[index].to_str().ok_or_else(|| {
            IosError::compatibility("optional iOS release arguments must be valid UTF-8")
        })?;
        match value {
            "--profile" => {
                index += 1;
                profile = arguments
                    .get(index)
                    .and_then(|value| value.to_str())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| IosError::compatibility("--profile requires a UTF-8 value"))?
                    .to_owned();
            }
            value if value.starts_with("--profile=") => {
                profile = value.trim_start_matches("--profile=").to_owned();
                if profile.is_empty() {
                    return Err(IosError::compatibility("--profile requires a value"));
                }
            }
            "--dry-run" => dry_run = true,
            "ios-arm64" | "ios-simulator-arm64" => {
                if !slices.insert(value.to_owned()) {
                    return Err(IosError::compatibility(format!(
                        "optional iOS release slice must not be repeated: {value}"
                    )));
                }
            }
            _ => {
                return Err(IosError::compatibility(format!(
                    "unknown optional iOS release argument: {value}"
                )));
            }
        }
        index += 1;
    }
    if !slices.is_empty()
        && slices != BTreeSet::from(["ios-arm64".to_owned(), "ios-simulator-arm64".to_owned()])
    {
        return Err(IosError::compatibility(
            "optional iOS release requires both ios-arm64 and ios-simulator-arm64 slices",
        ));
    }
    if profile.len() > 128 || profile.chars().any(char::is_control) {
        return Err(IosError::compatibility(
            "optional iOS release profile must contain 1 to 128 non-control characters",
        ));
    }
    Ok(OptionalReleaseRequest {
        output_directory,
        profile,
        dry_run,
    })
}

fn write_optional_release_dry_run(
    request: &OptionalReleaseRequest,
    output: &mut dyn Write,
) -> Result<(), IosError> {
    writeln!(output, "Resolved optional iOS plugin release:")
        .and_then(|()| writeln!(output, "output={}", request.output_directory.display()))
        .and_then(|()| writeln!(output, "profile={}", request.profile))
        .and_then(|()| writeln!(output, "slices=ios-arm64,ios-simulator-arm64"))
        .map_err(output_error)
}

fn stage_optional_release_bundle(
    root: &Path,
    output_directory: &Path,
    package_artifacts: &Path,
) -> Result<(), IosError> {
    stage_optional_release_bundle_with_profile(
        root,
        output_directory,
        package_artifacts,
        "source-normalizer",
    )
}

#[cfg(target_os = "macos")]
fn stage_optional_release_bundle_with_profile(
    root: &Path,
    output_directory: &Path,
    package_artifacts: &Path,
    profile: &str,
) -> Result<(), IosError> {
    let plugin_guard = crate::ios_plugin::acquire_build_guard(root)?;
    let aggregate_inputs = persistent_optional_aggregate_inputs(root)?;
    let mut runtime_owner_selected = false;
    for plugin in &crate::ios_plugin::IOS_PLUGIN_SPECS {
        let mut arguments = vec![aggregate_inputs.as_os_str().to_owned()];
        if plugin.uses_ffmpeg {
            arguments.extend([OsString::from("--profile"), OsString::from(profile)]);
        }
        arguments.extend([
            OsString::from("ios-arm64"),
            OsString::from("ios-simulator-arm64"),
        ]);
        let mut diagnostics = Vec::new();
        // Each plugin release validates and atomically promotes its own outputs. The first
        // FFmpeg-backed plugin also owns the shared runtime transaction for the aggregate.
        let stage_runtime = plugin.uses_ffmpeg && !runtime_owner_selected;
        let result = crate::ios_plugin_release::stage_for_aggregate(
            root,
            plugin.id,
            arguments,
            stage_runtime,
            &plugin_guard,
            &mut io::sink(),
            &mut diagnostics,
        );
        io::stderr()
            .lock()
            .write_all(&diagnostics)
            .map_err(output_error)?;
        result?;
        runtime_owner_selected |= stage_runtime;
    }

    for framework in OPTIONAL_RELEASE_FRAMEWORKS {
        let source = optional_framework_source(root, framework)?;
        let destination = package_artifacts.join(format!("{framework}.xcframework"));
        copy_directory(&source, &destination, "optional iOS package XCFramework")?;
    }
    let framework_assets = OPTIONAL_RELEASE_FRAMEWORKS
        .iter()
        .map(|framework| OsString::from(format!("{framework}.xcframework.zip")))
        .collect::<Vec<_>>();
    copy_validated_optional_release_assets(&aggregate_inputs, output_directory, &framework_assets)?;
    crate::ios_optional_release::stage_ffmpeg_compliance_assets(
        root,
        output_directory,
        output_directory,
    )?;
    wait_for_optional_aggregate_copy_test_gate(root, &plugin_guard)?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn stage_optional_release_bundle_with_profile(
    root: &Path,
    output_directory: &Path,
    package_artifacts: &Path,
    profile: &str,
) -> Result<(), IosError> {
    let _ = (root, output_directory, package_artifacts, profile);
    Err(IosError::compatibility(
        "iOS release staging requires macOS",
    ))
}

#[cfg(target_os = "macos")]
fn wait_for_optional_aggregate_copy_test_gate(
    root: &Path,
    plugin_guard: &crate::ios_plugin::IosPluginBuildGuard,
) -> Result<(), IosError> {
    #[cfg(debug_assertions)]
    {
        use std::time::Instant;

        let Some(ready) = env::var_os("VESPER_TEST_IOS_OPTIONAL_AGGREGATE_COPY_READY") else {
            return Ok(());
        };
        crate::ios_plugin::validate_build_guard(root, plugin_guard)?;
        let release = env::var_os("VESPER_TEST_IOS_OPTIONAL_AGGREGATE_COPY_RELEASE")
            .ok_or_else(|| {
                IosError::worker(
                    "VESPER_TEST_IOS_OPTIONAL_AGGREGATE_COPY_RELEASE is required when VESPER_TEST_IOS_OPTIONAL_AGGREGATE_COPY_READY is set",
                )
            })?;
        fs::write(PathBuf::from(ready), b"ready\n").map_err(|error| {
            IosError::storage(format!(
                "failed to publish optional iOS aggregate copy test gate: {error}"
            ))
        })?;
        let release = PathBuf::from(release);
        let deadline = Instant::now() + Duration::from_secs(30);
        while !release.exists() {
            if Instant::now() >= deadline {
                return Err(IosError::worker(
                    "timed out waiting for optional iOS aggregate copy test gate",
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = (root, plugin_guard);
    Ok(())
}

fn persistent_optional_aggregate_inputs(root: &Path) -> Result<PathBuf, IosError> {
    let directory = PreparedDirectory::prepare(
        &root
            .join("lib/ios/VesperPlayerKit/.build")
            .join(OPTIONAL_AGGREGATE_INPUT_DIRECTORY),
        "optional iOS aggregate release inputs",
    )?;
    directory.validate("optional iOS aggregate release inputs")?;
    let path = directory.path.clone();
    // Nested plugin journals outlive the aggregate command, so their owner parent must too.
    directory.commit_durable("optional iOS aggregate release inputs")?;
    Ok(path)
}

fn copy_validated_optional_release_assets(
    source_directory: &Path,
    destination_directory: &Path,
    staged_assets: &[OsString],
) -> Result<(), IosError> {
    let cancellation =
        external_process::InterruptDeferral::start("optional iOS release input copy")
            .map_err(map_process_error)?;
    let result = (|| {
        let expected_source = directory_snapshot_with_cancellation(
            source_directory,
            "optional iOS aggregate release inputs",
            Some(&cancellation),
        )?;
        for name in staged_assets {
            check_release_scan_cancellation(
                Some(&cancellation),
                "optional iOS release input copy",
            )?;
            let source = source_directory.join(name);
            let metadata = fs::symlink_metadata(&source).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect optional iOS aggregate release input '{}': {error}",
                    source.display()
                ))
            })?;
            if !metadata.file_type().is_file() {
                return Err(IosError::storage(format!(
                    "optional iOS aggregate release input '{}' is not a regular non-symlink file",
                    source.display()
                )));
            }
            copy_preserved_release_file(
                &source,
                &destination_directory.join(name),
                &metadata,
                &cancellation,
            )?;
        }
        let current_source = directory_snapshot_with_cancellation(
            source_directory,
            "optional iOS aggregate release inputs",
            Some(&cancellation),
        )?;
        if current_source != expected_source {
            return Err(IosError::storage(format!(
                "optional iOS aggregate release inputs '{}' changed while they were copied",
                source_directory.display()
            )));
        }
        Ok(())
    })();
    let cancelled = cancellation.finish();
    match (result, cancelled) {
        (Ok(()), true) => Err(IosError::worker(
            "optional iOS release input copy was cancelled",
        )),
        (result, _) => result,
    }
}

fn optional_framework_source(root: &Path, framework: &str) -> Result<PathBuf, IosError> {
    let relative = match framework {
        "VesperFFmpegAVCodec" | "VesperFFmpegAVFormat" | "VesperFFmpegAVUtil" => {
            format!("player-ffmpeg-runtime/{framework}.xcframework")
        }
        _ => {
            let plugin = crate::ios_plugin::IOS_PLUGIN_SPECS
                .iter()
                .find(|plugin| plugin.framework_name == framework)
                .ok_or_else(|| {
                    IosError::worker(format!("unknown optional framework: {framework}"))
                })?;
            format!(
                "{}/{}.xcframework",
                plugin.build_directory, plugin.framework_name
            )
        }
    };
    let path = root.join("lib/ios/VesperPlayerKit/.build").join(relative);
    require_repository_directory(root, &path, "optional iOS XCFramework")?;
    Ok(path)
}

fn copy_directory(source: &Path, destination: &Path, label: &str) -> Result<(), IosError> {
    let mut command = Command::new(configured_tool("DITTO", "ditto"));
    command
        .args([
            OsStr::new("--norsrc"),
            source.as_os_str(),
            destination.as_os_str(),
        ])
        .env("COPYFILE_DISABLE", "1");
    require_success(&mut command, label)
}

#[derive(Clone, Copy)]
enum FrameworkSlice {
    Device,
    Simulator,
}

fn stage_framework_archive(
    source: &Path,
    output: &Path,
    slice: FrameworkSlice,
) -> Result<(), IosError> {
    let workspace = tempfile::tempdir().map_err(|error| {
        IosError::storage(format!(
            "failed to create framework staging directory: {error}"
        ))
    })?;
    let staged = workspace.path().join(FRAMEWORK_NAME);
    let mut copy = Command::new(configured_tool("DITTO", "ditto"));
    copy.args([
        OsStr::new("--norsrc"),
        source.as_os_str(),
        staged.as_os_str(),
    ])
    .env("COPYFILE_DISABLE", "1");
    require_success(&mut copy, "framework staging copy")?;
    prune_private_swift_modules(&staged)?;

    let source_binary = source.join(BINARY_NAME);
    let staged_binary = staged.join(BINARY_NAME);
    let source_architectures = lipo_architectures(&source_binary)?;
    match slice {
        FrameworkSlice::Device => {
            if source_architectures.as_slice() != ["arm64"] {
                return Err(IosError::conformance(format!(
                    "Expected arm64 device framework binary, got: {}",
                    source_architectures.join(" ")
                )));
            }
        }
        FrameworkSlice::Simulator if source_architectures.as_slice() == ["arm64"] => {}
        FrameworkSlice::Simulator if source_architectures.iter().any(|arch| arch == "arm64") => {
            let mut extract = Command::new(configured_tool("LIPO", "lipo"));
            extract
                .arg(&source_binary)
                .args(["-extract", "arm64", "-output"])
                .arg(&staged_binary);
            require_success(&mut extract, "Simulator arm64 framework extraction")?;
            let extracted = lipo_architectures(&staged_binary)?;
            if extracted.as_slice() != ["arm64"] {
                return Err(IosError::conformance(format!(
                    "Extracted Simulator framework is not arm64-only: {}",
                    extracted.join(" ")
                )));
            }
        }
        FrameworkSlice::Simulator => {
            return Err(IosError::conformance(format!(
                "Expected arm64 Simulator framework binary, got: {}",
                source_architectures.join(" ")
            )));
        }
    }
    validate_tree(&staged, "staged VesperPlayerKit framework")?;
    create_zip(&staged, output, "VesperPlayerKit framework archive")
}

fn create_zip(source: &Path, output: &Path, label: &str) -> Result<(), IosError> {
    let mut command = Command::new(configured_tool("DITTO", "ditto"));
    command
        .args(["--norsrc", "-c", "-k", "--keepParent"])
        .arg(source)
        .arg(output)
        .env("COPYFILE_DISABLE", "1");
    require_success(&mut command, label)?;
    validate_staged_file(output, label)
}

fn lipo_architectures(binary: &Path) -> Result<Vec<String>, IosError> {
    let mut command = Command::new(configured_tool("LIPO", "lipo"));
    command.args([OsStr::new("-info"), binary.as_os_str()]);
    let captured = external_process::run_interruptible_capture(
        &mut command,
        "lipo architecture inspection",
        MAX_LIPO_OUTPUT_BYTES,
        MAX_LIPO_OUTPUT_BYTES,
    )
    .map_err(map_process_error)?;
    require_captured_success(&captured.status, "lipo architecture inspection")?;
    if !captured.stderr.is_empty() {
        io::stderr()
            .lock()
            .write_all(&captured.stderr)
            .map_err(output_error)?;
    }
    let value = String::from_utf8(captured.stdout).map_err(|error| {
        IosError::conformance(format!(
            "lipo returned non-UTF-8 architecture output: {error}"
        ))
    })?;
    let architectures = value
        .lines()
        .find_map(|line| {
            line.split_once(" are: ")
                .or_else(|| line.split_once(" architecture: "))
                .map(|(_, architectures)| {
                    architectures
                        .split_ascii_whitespace()
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
        })
        .filter(|architectures| !architectures.is_empty())
        .ok_or_else(|| {
            IosError::conformance(format!(
                "Unable to parse lipo architecture output for '{}': {}",
                binary.display(),
                value.trim()
            ))
        })?;
    Ok(architectures)
}

fn prune_private_swift_modules(framework: &Path) -> Result<(), IosError> {
    let modules = framework.join("Modules");
    if !modules.exists() {
        return Ok(());
    }
    let cancellation = external_process::InterruptDeferral::start("Swift module pruning")
        .map_err(map_process_error)?;
    let result = walk_tree(&modules, "framework Modules", |path, metadata| {
        if metadata.file_type().is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "swiftmodule")
        {
            fs::remove_file(path).map_err(|error| {
                IosError::storage(format!(
                    "failed to remove private Swift module '{}': {error}",
                    path.display()
                ))
            })?;
        }
        if cancellation.is_cancelled() {
            return Err(IosError::worker("Swift module pruning was cancelled"));
        }
        Ok(())
    });
    let cancelled = cancellation.finish();
    if cancelled {
        Err(IosError::worker("Swift module pruning was cancelled"))
    } else {
        result
    }
}

fn validate_tree(root: &Path, label: &str) -> Result<(), IosError> {
    let cancellation = external_process::InterruptDeferral::start("iOS release tree validation")
        .map_err(map_process_error)?;
    let mut total_bytes = 0_u64;
    let result = walk_tree(root, label, |path, metadata| {
        if cancellation.is_cancelled() {
            return Err(IosError::worker(
                "iOS release tree validation was cancelled",
            ));
        }
        if metadata.file_type().is_file() {
            total_bytes = total_bytes.checked_add(metadata.len()).ok_or_else(|| {
                IosError::storage(format!("{label} size overflowed while scanning"))
            })?;
            if total_bytes > MAX_RELEASE_TREE_BYTES {
                return Err(IosError::storage(format!(
                    "{label} exceeds {MAX_RELEASE_TREE_BYTES} bytes: {}",
                    path.display()
                )));
            }
        }
        Ok(())
    });
    let cancelled = cancellation.finish();
    if cancelled {
        Err(IosError::worker(
            "iOS release tree validation was cancelled",
        ))
    } else {
        result
    }
}

fn walk_tree(
    root: &Path,
    label: &str,
    mut visit: impl FnMut(&Path, &fs::Metadata) -> Result<(), IosError>,
) -> Result<(), IosError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        IosError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            root.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(IosError::storage(format!(
            "{label} '{}' is not a regular non-symlink directory",
            root.display()
        )));
    }
    let mut queue = VecDeque::from([(root.to_path_buf(), 0_usize)]);
    let mut entries = 0_usize;
    while let Some((directory, depth)) = queue.pop_front() {
        if depth > MAX_RELEASE_TREE_DEPTH {
            return Err(IosError::storage(format!(
                "{label} exceeds traversal depth {MAX_RELEASE_TREE_DEPTH}: {}",
                directory.display()
            )));
        }
        let children = fs::read_dir(&directory).map_err(|error| {
            IosError::storage(format!(
                "failed to scan {label} '{}': {error}",
                directory.display()
            ))
        })?;
        for child in children {
            let child = child.map_err(|error| {
                IosError::storage(format!(
                    "failed to read {label} entry under '{}': {error}",
                    directory.display()
                ))
            })?;
            entries = entries
                .checked_add(1)
                .ok_or_else(|| IosError::storage(format!("{label} entry count overflowed")))?;
            if entries > MAX_RELEASE_TREE_ENTRIES {
                return Err(IosError::storage(format!(
                    "{label} exceeds {MAX_RELEASE_TREE_ENTRIES} entries"
                )));
            }
            let path = child.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect {label} entry '{}': {error}",
                    path.display()
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(IosError::storage(format!(
                    "{label} must not contain symlinks: {}",
                    path.display()
                )));
            }
            if metadata.file_type().is_dir() {
                queue.push_back((path.clone(), depth + 1));
            } else if !metadata.file_type().is_file() {
                return Err(IosError::storage(format!(
                    "{label} contains an unsupported file type: {}",
                    path.display()
                )));
            }
            visit(&path, &metadata)?;
        }
    }
    Ok(())
}

fn validate_package_stage(path: &Path) -> Result<(), IosError> {
    validate_tree(path, "optional iOS package artifacts")?;
    let expected = OPTIONAL_RELEASE_FRAMEWORKS
        .iter()
        .map(|name| OsString::from(format!("{name}.xcframework")))
        .collect::<BTreeSet<_>>();
    let actual = read_top_level_names(path, "optional iOS package artifacts")?;
    if actual != expected {
        return Err(IosError::conformance(format!(
            "optional iOS package artifacts have an unexpected top-level set: {}",
            display_names(&actual)
        )));
    }
    for name in expected {
        let metadata = fs::symlink_metadata(path.join(&name)).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect optional package artifact '{}': {error}",
                path.join(&name).display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(IosError::conformance(format!(
                "optional package artifact '{}' is not a regular non-symlink directory",
                path.join(name).display()
            )));
        }
    }
    Ok(())
}

fn validate_release_stage(path: &Path, include_optional: bool) -> Result<Vec<OsString>, IosError> {
    let actual = read_top_level_names(path, "iOS release staging directory")?;
    let mut expected = CORE_RELEASE_ASSETS
        .iter()
        .map(OsString::from)
        .collect::<BTreeSet<_>>();
    if include_optional {
        expected.extend(
            OPTIONAL_RELEASE_FRAMEWORKS
                .iter()
                .map(|name| OsString::from(format!("{name}.xcframework.zip"))),
        );
        expected.insert(OsString::from(OPTIONAL_COMPLIANCE_ASSET));
        let sources = actual
            .iter()
            .filter(|name| optional_source_asset_name(name))
            .cloned()
            .collect::<Vec<_>>();
        if sources.len() != 1 {
            return Err(IosError::conformance(format!(
                "complete iOS release staging requires exactly one FFmpeg source asset, found {}",
                sources.len()
            )));
        }
        expected.insert(sources[0].clone());
    }
    if actual != expected {
        return Err(IosError::conformance(format!(
            "iOS release staging has an unexpected top-level asset set: {}",
            display_names(&actual)
        )));
    }
    for name in &actual {
        validate_staged_file(&path.join(name), "iOS release asset")?;
    }
    Ok(actual.into_iter().collect())
}

fn validate_optional_release_stage(path: &Path) -> Result<Vec<OsString>, IosError> {
    let actual = read_top_level_names(path, "optional iOS release staging directory")?;
    let mut expected = OPTIONAL_RELEASE_FRAMEWORKS
        .iter()
        .map(|name| OsString::from(format!("{name}.xcframework.zip")))
        .collect::<BTreeSet<_>>();
    expected.insert(OsString::from(OPTIONAL_COMPLIANCE_ASSET));
    let sources = actual
        .iter()
        .filter(|name| optional_source_asset_name(name))
        .cloned()
        .collect::<Vec<_>>();
    if sources.len() != 1 {
        return Err(IosError::conformance(format!(
            "optional iOS release staging requires exactly one FFmpeg source asset, found {}",
            sources.len()
        )));
    }
    expected.insert(sources[0].clone());
    if actual != expected {
        return Err(IosError::conformance(format!(
            "optional iOS release staging has an unexpected top-level asset set: {}",
            display_names(&actual)
        )));
    }
    for name in &actual {
        validate_staged_file(&path.join(name), "optional iOS release asset")?;
    }
    Ok(actual.into_iter().collect())
}

fn read_top_level_names(path: &Path, label: &str) -> Result<BTreeSet<OsString>, IosError> {
    let entries = fs::read_dir(path).map_err(|error| {
        IosError::storage(format!(
            "failed to scan {label} '{}': {error}",
            path.display()
        ))
    })?;
    let mut names = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            IosError::storage(format!(
                "failed to read {label} entry under '{}': {error}",
                path.display()
            ))
        })?;
        if names.len() >= MAX_RELEASE_DIRECTORY_ENTRIES {
            return Err(IosError::storage(format!(
                "{label} exceeds {MAX_RELEASE_DIRECTORY_ENTRIES} top-level entries"
            )));
        }
        names.insert(entry.file_name());
    }
    Ok(names)
}

fn validate_staged_file(path: &Path, label: &str) -> Result<(), IosError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(IosError::conformance(format!(
            "{label} '{}' is not a regular non-symlink file",
            path.display()
        )));
    }
    if metadata.len() == 0 || metadata.len() > MAX_RELEASE_ASSET_BYTES {
        return Err(IosError::conformance(format!(
            "{label} '{}' has invalid size {}; expected 1..={MAX_RELEASE_ASSET_BYTES} bytes",
            path.display(),
            metadata.len()
        )));
    }
    Ok(())
}

fn require_success(command: &mut Command, label: &str) -> Result<(), IosError> {
    command.stdin(Stdio::null());
    let captured = external_process::run_interruptible_capture(
        command,
        label,
        MAX_RELEASE_TOOL_OUTPUT_BYTES,
        MAX_RELEASE_TOOL_OUTPUT_BYTES,
    )
    .map_err(map_process_error)?;
    let mut diagnostics = io::stderr().lock();
    if !captured.stderr.is_empty() {
        diagnostics
            .write_all(&captured.stderr)
            .map_err(output_error)?;
    }
    if !captured.status.success() && !captured.stdout.is_empty() {
        diagnostics
            .write_all(&captured.stdout)
            .map_err(output_error)?;
    }
    require_captured_success(&captured.status, label)
}

fn require_captured_success(status: &ExitStatus, label: &str) -> Result<(), IosError> {
    if status.success() {
        Ok(())
    } else if status.code().is_none() {
        Err(IosError::worker(format!("{label} crashed ({status})")))
    } else {
        Err(IosError::conformance(format!(
            "{label} exited unsuccessfully ({status})"
        )))
    }
}

fn map_process_error(error: external_process::ExternalProcessError) -> IosError {
    match error.kind() {
        external_process::ExternalProcessErrorKind::Compatibility => {
            IosError::compatibility(error.to_string())
        }
        external_process::ExternalProcessErrorKind::Worker
        | external_process::ExternalProcessErrorKind::Cancelled => {
            IosError::worker(error.to_string())
        }
    }
}

fn configured_tool(variable: &str, default: &str) -> OsString {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from(default))
}

fn output_error(error: io::Error) -> IosError {
    IosError::worker(format!(
        "failed to write iOS release command output: {error}"
    ))
}

#[cfg(target_os = "macos")]
fn require_macos_stage_host() -> Result<(), IosError> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn require_macos_stage_host() -> Result<(), IosError> {
    Err(IosError::compatibility(
        "iOS release staging requires macOS",
    ))
}

fn absolute_path(path: &Path, label: &str) -> Result<PathBuf, IosError> {
    for component in path.components() {
        if matches!(component, Component::CurDir | Component::ParentDir) {
            return Err(IosError::storage(format!(
                "{label} '{}' contains a non-canonical path component",
                path.display()
            )));
        }
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        let current = env::current_dir().map_err(|error| {
            IosError::storage(format!("failed to resolve current directory: {error}"))
        })?;
        let current = fs::canonicalize(&current).map_err(|error| {
            IosError::storage(format!(
                "failed to canonicalize current directory '{}': {error}",
                current.display()
            ))
        })?;
        Ok(current.join(path))
    }
}

fn reject_symlink_components(path: &Path, label: &str) -> Result<(), IosError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata)
                if metadata.file_type().is_symlink() && !allowed_system_path_alias(&current) =>
            {
                return Err(IosError::storage(format!(
                    "{label} path must not contain symlinks: {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(IosError::storage(format!(
                    "failed to inspect {label} path '{}': {error}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn allowed_system_path_alias(path: &Path) -> bool {
    path == Path::new("/tmp") || path == Path::new("/var")
}

#[cfg(not(target_os = "macos"))]
fn allowed_system_path_alias(_path: &Path) -> bool {
    false
}

fn require_repository_directory(root: &Path, path: &Path, label: &str) -> Result<(), IosError> {
    require_repository_path(root, path, label, true)
}

fn require_repository_path(
    root: &Path,
    path: &Path,
    label: &str,
    directory: bool,
) -> Result<(), IosError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        IosError::storage(format!(
            "{label} '{}' is outside repository root '{}'",
            path.display(),
            root.display()
        ))
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(IosError::storage(format!(
                "{label} '{}' contains an invalid path component",
                path.display()
            )));
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                current.display()
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(IosError::storage(format!(
                "{label} path must not contain symlinks: {}",
                current.display()
            )));
        }
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    let valid = if directory {
        metadata.file_type().is_dir()
    } else {
        metadata.file_type().is_file()
    };
    if valid {
        Ok(())
    } else {
        Err(IosError::storage(format!(
            "{label} '{}' has an invalid file type",
            path.display()
        )))
    }
}

fn validate_release_output_location(root: &Path, output: &Path) -> Result<(), IosError> {
    if output == root || root.starts_with(output) {
        return Err(IosError::storage(format!(
            "iOS release output '{}' must not contain the repository root '{}'",
            output.display(),
            root.display()
        )));
    }
    for protected in [".git", "crates", "lib", "scripts", "target", "third_party"] {
        let protected = root.join(protected);
        if output == protected || output.starts_with(&protected) {
            return Err(IosError::storage(format!(
                "iOS release output '{}' overlaps protected repository path '{}'",
                output.display(),
                protected.display()
            )));
        }
    }
    Ok(())
}

fn validate_package_artifacts_location(
    root: &Path,
    default: &Path,
    target: &Path,
    explicit: bool,
) -> Result<(), IosError> {
    if target.file_name() != Some(OsStr::new("Artifacts")) {
        return Err(IosError::storage(format!(
            "iOS package artifacts target '{}' must end in a dedicated 'Artifacts' directory",
            target.display()
        )));
    }
    if target == root || root.starts_with(target) {
        return Err(IosError::storage(format!(
            "iOS package artifacts target '{}' must not contain the repository root",
            target.display()
        )));
    }
    if target.starts_with(root) && target != default && !explicit {
        return Err(IosError::storage(format!(
            "ambient repository-local iOS package artifacts target must be '{}', got '{}'",
            default.display(),
            target.display()
        )));
    }
    if target.starts_with(root) && target != default {
        let release_root = root.join("dist");
        if !target.starts_with(&release_root) {
            return Err(IosError::storage(format!(
                "explicit repository-local iOS package artifacts target '{}' must be under '{}'",
                target.display(),
                release_root.display()
            )));
        }
        validate_release_output_location(root, target)?;
    }
    Ok(())
}

fn validate_existing_package_target(target: &Path) -> Result<(), IosError> {
    let metadata = match fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(IosError::storage(format!(
                "failed to inspect existing iOS package artifacts target '{}': {error}",
                target.display()
            )));
        }
    };
    if !metadata.file_type().is_dir() {
        return Err(IosError::storage(format!(
            "iOS package artifacts target '{}' is not a regular non-symlink directory",
            target.display()
        )));
    }
    validate_tree(target, "existing iOS package artifacts")?;

    let allowed = OPTIONAL_RELEASE_FRAMEWORKS
        .iter()
        .map(|framework| OsString::from(format!("{framework}.xcframework")))
        .collect::<BTreeSet<_>>();
    let entries = fs::read_dir(target).map_err(|error| {
        IosError::storage(format!(
            "failed to scan existing iOS package artifacts '{}': {error}",
            target.display()
        ))
    })?;
    let mut count = 0_usize;
    for entry in entries {
        let entry = entry.map_err(|error| {
            IosError::storage(format!(
                "failed to read existing iOS package artifacts entry under '{}': {error}",
                target.display()
            ))
        })?;
        count = count.checked_add(1).ok_or_else(|| {
            IosError::storage("existing iOS package artifacts entry count overflowed")
        })?;
        if count > OPTIONAL_RELEASE_FRAMEWORKS.len() {
            return Err(IosError::storage(format!(
                "existing iOS package artifacts '{}' contain unmanaged entries",
                target.display()
            )));
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect existing iOS package artifact '{}': {error}",
                entry.path().display()
            ))
        })?;
        if !metadata.file_type().is_dir() || !allowed.contains(&entry.file_name()) {
            return Err(IosError::storage(format!(
                "existing iOS package artifacts target '{}' contains unmanaged entry '{}'",
                target.display(),
                entry.file_name().to_string_lossy()
            )));
        }
    }
    Ok(())
}

fn validate_non_overlapping_paths(left: &Path, right: &Path) -> Result<(), IosError> {
    if left == right || left.starts_with(right) || right.starts_with(left) {
        Err(IosError::storage(format!(
            "iOS release output '{}' and package artifacts target '{}' must not overlap",
            left.display(),
            right.display()
        )))
    } else {
        Ok(())
    }
}

fn optional_source_asset_name(name: &OsStr) -> bool {
    name.to_str()
        .and_then(|name| {
            name.strip_prefix("VesperPlayerOptionalPlugins-FFmpeg-")
                .and_then(|name| name.strip_suffix("-source.tar.xz"))
        })
        .is_some_and(|version| {
            !version.is_empty()
                && version.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
                })
        })
}

fn optional_release_asset_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    OPTIONAL_RELEASE_FRAMEWORKS
        .iter()
        .map(|framework| format!("{framework}.xcframework.zip"))
        .any(|asset| asset == name)
        || matches!(
            name,
            OPTIONAL_COMPLIANCE_ASSET | LEGACY_OPTIONAL_RUNTIME_ASSET
        )
        || optional_source_asset_name(OsStr::new(name))
}

fn display_names(names: &BTreeSet<OsString>) -> String {
    names
        .iter()
        .map(|name| name.to_string_lossy())
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct FileIdentity {
    volume_or_device: u64,
    file_index: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct DirectorySnapshot {
    identity: FileIdentity,
    digest: String,
}

fn directory_snapshot(path: &Path, label: &str) -> Result<DirectorySnapshot, IosError> {
    let cancellation =
        external_process::InterruptDeferral::start(label).map_err(map_process_error)?;
    let result = directory_snapshot_with_cancellation(path, label, Some(&cancellation));
    let cancelled = cancellation.finish();
    match (result, cancelled) {
        (Ok(_), true) => Err(IosError::worker(format!("{label} scan was cancelled"))),
        (result, _) => result,
    }
}

fn optional_directory_snapshot(
    path: &Path,
    label: &str,
) -> Result<Option<DirectorySnapshot>, IosError> {
    let cancellation =
        external_process::InterruptDeferral::start(label).map_err(map_process_error)?;
    let result = optional_directory_snapshot_with_cancellation(path, label, Some(&cancellation));
    let cancelled = cancellation.finish();
    match (result, cancelled) {
        (Ok(_), true) => Err(IosError::worker(format!("{label} scan was cancelled"))),
        (result, _) => result,
    }
}

fn optional_directory_snapshot_with_cancellation(
    path: &Path,
    label: &str,
    cancellation: Option<&external_process::InterruptDeferral>,
) -> Result<Option<DirectorySnapshot>, IosError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            directory_snapshot_with_cancellation(path, label, cancellation).map(Some)
        }
        Ok(_) => Err(IosError::storage(format!(
            "{label} '{}' is not a regular non-symlink directory",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(IosError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))),
    }
}

fn validate_directory_snapshot(
    path: &Path,
    expected: &DirectorySnapshot,
    label: &str,
) -> Result<(), IosError> {
    let current = directory_snapshot(path, label)?;
    if &current == expected {
        Ok(())
    } else {
        Err(IosError::storage(format!(
            "{label} '{}' changed after validation",
            path.display()
        )))
    }
}

fn validate_optional_directory_snapshot(
    path: &Path,
    expected: &Option<DirectorySnapshot>,
    label: &str,
) -> Result<(), IosError> {
    let current = optional_directory_snapshot(path, label)?;
    if &current == expected {
        Ok(())
    } else {
        Err(IosError::storage(format!(
            "{label} '{}' changed after validation",
            path.display()
        )))
    }
}

fn directory_snapshot_with_cancellation(
    path: &Path,
    label: &str,
    cancellation: Option<&external_process::InterruptDeferral>,
) -> Result<DirectorySnapshot, IosError> {
    let identity = directory_identity(path, label)?;
    let mut hasher = Sha256::new();
    hasher.update(b"vesper-directory-snapshot-v1\0");
    let mut pending = VecDeque::from([(path.to_path_buf(), PathBuf::new(), 0_usize)]);
    let mut entries = 0_usize;
    let mut total_bytes = 0_u64;

    while let Some((directory, relative_directory, depth)) = pending.pop_front() {
        check_release_scan_cancellation(cancellation, label)?;
        if depth > MAX_RELEASE_TREE_DEPTH {
            return Err(IosError::storage(format!(
                "{label} '{}' exceeds tree depth {MAX_RELEASE_TREE_DEPTH}",
                path.display()
            )));
        }
        let children = fs::read_dir(&directory).map_err(|error| {
            IosError::storage(format!(
                "failed to scan {label} directory '{}': {error}",
                directory.display()
            ))
        })?;
        let mut children = children
            .map(|entry| {
                entry.map_err(|error| {
                    IosError::storage(format!(
                        "failed to read {label} entry under '{}': {error}",
                        directory.display()
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(fs::DirEntry::file_name);

        for child in children {
            check_release_scan_cancellation(cancellation, label)?;
            entries = entries
                .checked_add(1)
                .ok_or_else(|| IosError::storage(format!("{label} entry count overflowed")))?;
            if entries > MAX_RELEASE_TREE_ENTRIES {
                return Err(IosError::storage(format!(
                    "{label} '{}' exceeds {MAX_RELEASE_TREE_ENTRIES} entries",
                    path.display()
                )));
            }
            let child_path = child.path();
            let relative = relative_directory.join(child.file_name());
            let metadata = fs::symlink_metadata(&child_path).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect {label} entry '{}': {error}",
                    child_path.display()
                ))
            })?;
            hash_snapshot_path(&mut hasher, &relative);
            hasher.update(metadata_mode(&metadata).to_le_bytes());
            if metadata.file_type().is_dir() {
                hasher.update(b"D");
                pending.push_back((child_path, relative, depth + 1));
            } else if metadata.file_type().is_file() {
                hasher.update(b"F");
                hasher.update(metadata.len().to_le_bytes());
                total_bytes = total_bytes.checked_add(metadata.len()).ok_or_else(|| {
                    IosError::storage(format!("{label} expanded size overflowed"))
                })?;
                if total_bytes > MAX_RELEASE_TREE_BYTES {
                    return Err(IosError::storage(format!(
                        "{label} '{}' exceeds {MAX_RELEASE_TREE_BYTES} bytes",
                        path.display()
                    )));
                }
                hash_snapshot_file(&child_path, &metadata, &mut hasher, cancellation, label)?;
            } else {
                return Err(IosError::storage(format!(
                    "{label} contains a symlink or special file: {}",
                    child_path.display()
                )));
            }
        }
    }
    if directory_identity(path, label)? != identity {
        return Err(IosError::storage(format!(
            "{label} '{}' changed while it was scanned",
            path.display()
        )));
    }
    Ok(DirectorySnapshot {
        identity,
        digest: hex::encode(hasher.finalize()),
    })
}

fn hash_snapshot_path(hasher: &mut Sha256, path: &Path) {
    let bytes = path.as_os_str().as_encoded_bytes();
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn hash_snapshot_file(
    path: &Path,
    metadata: &fs::Metadata,
    hasher: &mut Sha256,
    cancellation: Option<&external_process::InterruptDeferral>,
    label: &str,
) -> Result<(), IosError> {
    let expected_identity = path_identity(path, metadata)?;
    let mut file = open_regular_file_nofollow(path).map_err(|error| {
        IosError::storage(format!(
            "failed to open {label} file '{}': {error}",
            path.display()
        ))
    })?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut read_bytes = 0_u64;
    loop {
        check_release_scan_cancellation(cancellation, label)?;
        let count = file.read(&mut buffer).map_err(|error| {
            IosError::storage(format!(
                "failed to read {label} file '{}': {error}",
                path.display()
            ))
        })?;
        if count == 0 {
            break;
        }
        read_bytes = read_bytes
            .checked_add(count as u64)
            .ok_or_else(|| IosError::storage(format!("{label} file size overflowed")))?;
        hasher.update(&buffer[..count]);
    }
    let final_metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::storage(format!(
            "failed to re-inspect {label} file '{}': {error}",
            path.display()
        ))
    })?;
    if !final_metadata.file_type().is_file()
        || path_identity(path, &final_metadata)? != expected_identity
        || final_metadata.len() != metadata.len()
        || read_bytes != metadata.len()
    {
        return Err(IosError::storage(format!(
            "{label} file '{}' changed while it was scanned",
            path.display()
        )));
    }
    Ok(())
}

fn check_release_scan_cancellation(
    cancellation: Option<&external_process::InterruptDeferral>,
    label: &str,
) -> Result<(), IosError> {
    if cancellation.is_some_and(external_process::InterruptDeferral::is_cancelled) {
        Err(IosError::worker(format!("{label} scan was cancelled")))
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn open_regular_file_nofollow(path: &Path) -> io::Result<File> {
    use rustix::fs::{Mode, OFlags, open};

    open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(io::Error::from)
}

#[cfg(not(unix))]
fn open_regular_file_nofollow(path: &Path) -> io::Result<File> {
    File::open(path)
}

#[cfg(unix)]
fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;

    metadata.mode()
}

#[cfg(not(unix))]
fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum JournalDecision {
    Rollback,
    Commit,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DirectoryPromotionRecord {
    parent: PathBuf,
    parent_identity: FileIdentity,
    target: PathBuf,
    source: PathBuf,
    owner: PathBuf,
    owner_identity: FileIdentity,
    old: Option<DirectorySnapshot>,
    new: DirectorySnapshot,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReleasePromotionJournal {
    version: u32,
    transaction_id: [u8; 16],
    root: PathBuf,
    root_identity: FileIdentity,
    state_directory: PathBuf,
    state_directory_identity: FileIdentity,
    journal_parent_identity: FileIdentity,
    decision: JournalDecision,
    package_enabled: bool,
    release: DirectoryPromotionRecord,
    package: Option<DirectoryPromotionRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PromotionPlacement {
    Before,
    After,
    CommitCleanupPending,
    RollbackCleanupPending,
    CommittedAndCleaned,
    RolledBackAndCleaned,
}

fn populate_release_candidate(
    candidate: &Path,
    output: &Path,
    staged_assets: &[OsString],
    expected_output: &DirectorySnapshot,
    cancellation: &external_process::InterruptDeferral,
) -> Result<(), IosError> {
    let staged = staged_assets.iter().cloned().collect::<BTreeSet<_>>();
    let entries = fs::read_dir(output).map_err(|error| {
        IosError::storage(format!(
            "failed to scan existing iOS release output '{}': {error}",
            output.display()
        ))
    })?;
    let mut count = 0_usize;
    let mut preserved = BTreeSet::new();
    for entry in entries {
        check_release_scan_cancellation(Some(cancellation), "iOS release candidate")?;
        let entry = entry.map_err(|error| {
            IosError::storage(format!(
                "failed to read existing iOS release output entry under '{}': {error}",
                output.display()
            ))
        })?;
        count = count
            .checked_add(1)
            .ok_or_else(|| IosError::storage("iOS release output entry count overflowed"))?;
        if count > MAX_RELEASE_DIRECTORY_ENTRIES {
            return Err(IosError::storage(format!(
                "iOS release output exceeds {MAX_RELEASE_DIRECTORY_ENTRIES} top-level entries"
            )));
        }
        let name = entry.file_name();
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect existing iOS release output entry '{}': {error}",
                entry.path().display()
            ))
        })?;
        let owned = staged.contains(&name) || optional_release_asset_name(&name);
        if owned {
            if !metadata.file_type().is_file() {
                return Err(IosError::storage(format!(
                    "managed iOS release asset '{}' is not a regular non-symlink file",
                    entry.path().display()
                )));
            }
            continue;
        }
        if !metadata.file_type().is_file() {
            return Err(IosError::storage(format!(
                "unmanaged iOS release entry '{}' must be a regular non-symlink file",
                entry.path().display()
            )));
        }
        copy_preserved_release_file(
            &entry.path(),
            &candidate.join(&name),
            &metadata,
            cancellation,
        )?;
        preserved.insert(name);
    }

    let current =
        directory_snapshot_with_cancellation(output, "iOS release output", Some(cancellation))?;
    if &current != expected_output {
        return Err(IosError::storage(format!(
            "iOS release output '{}' changed while the replacement candidate was prepared",
            output.display()
        )));
    }

    let candidate_names = read_top_level_names(candidate, "iOS release candidate")?;
    let mut expected_names = staged;
    expected_names.extend(preserved);
    if candidate_names != expected_names {
        return Err(IosError::storage(format!(
            "iOS release candidate '{}' does not contain the expected owned and preserved files",
            candidate.display()
        )));
    }
    Ok(())
}

fn copy_preserved_release_file(
    source: &Path,
    target: &Path,
    metadata: &fs::Metadata,
    cancellation: &external_process::InterruptDeferral,
) -> Result<(), IosError> {
    let source_identity = path_identity(source, metadata)?;
    let mut input = open_regular_file_nofollow(source).map_err(|error| {
        IosError::storage(format!(
            "failed to open preserved iOS release file '{}': {error}",
            source.display()
        ))
    })?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(target)
        .map_err(|error| {
            IosError::storage(format!(
                "failed to create preserved iOS release file '{}': {error}",
                target.display()
            ))
        })?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut copied = 0_u64;
    loop {
        check_release_scan_cancellation(Some(cancellation), "iOS release candidate")?;
        let count = input.read(&mut buffer).map_err(|error| {
            IosError::storage(format!(
                "failed to read preserved iOS release file '{}': {error}",
                source.display()
            ))
        })?;
        if count == 0 {
            break;
        }
        copied = copied
            .checked_add(count as u64)
            .ok_or_else(|| IosError::storage("preserved iOS release file size overflowed"))?;
        if copied > MAX_RELEASE_ASSET_BYTES {
            return Err(IosError::storage(format!(
                "preserved iOS release file '{}' exceeds {MAX_RELEASE_ASSET_BYTES} bytes",
                source.display()
            )));
        }
        output.write_all(&buffer[..count]).map_err(|error| {
            IosError::storage(format!(
                "failed to write preserved iOS release file '{}': {error}",
                target.display()
            ))
        })?;
    }
    if copied != metadata.len() {
        return Err(IosError::storage(format!(
            "preserved iOS release file '{}' changed while it was copied",
            source.display()
        )));
    }
    output
        .set_permissions(metadata.permissions())
        .and_then(|()| output.sync_all())
        .map_err(|error| {
            IosError::storage(format!(
                "failed to sync preserved iOS release file '{}': {error}",
                target.display()
            ))
        })?;
    let final_metadata = fs::symlink_metadata(source).map_err(|error| {
        IosError::storage(format!(
            "failed to re-inspect preserved iOS release file '{}': {error}",
            source.display()
        ))
    })?;
    if !final_metadata.file_type().is_file()
        || path_identity(source, &final_metadata)? != source_identity
        || final_metadata.len() != metadata.len()
    {
        return Err(IosError::storage(format!(
            "preserved iOS release file '{}' changed while it was copied",
            source.display()
        )));
    }
    Ok(())
}

fn sync_directory_tree(
    path: &Path,
    label: &str,
    cancellation: &external_process::InterruptDeferral,
) -> Result<(), IosError> {
    let mut pending = VecDeque::from([(path.to_path_buf(), 0_usize)]);
    let mut directories = Vec::new();
    let mut entries = 0_usize;
    while let Some((directory, depth)) = pending.pop_front() {
        check_release_scan_cancellation(Some(cancellation), label)?;
        if depth > MAX_RELEASE_TREE_DEPTH {
            return Err(IosError::storage(format!(
                "{label} '{}' exceeds tree depth {MAX_RELEASE_TREE_DEPTH}",
                path.display()
            )));
        }
        directories.push(directory.clone());
        let children = fs::read_dir(&directory).map_err(|error| {
            IosError::storage(format!(
                "failed to scan {label} directory '{}': {error}",
                directory.display()
            ))
        })?;
        for child in children {
            check_release_scan_cancellation(Some(cancellation), label)?;
            let child = child.map_err(|error| {
                IosError::storage(format!(
                    "failed to read {label} entry under '{}': {error}",
                    directory.display()
                ))
            })?;
            entries = entries
                .checked_add(1)
                .ok_or_else(|| IosError::storage(format!("{label} entry count overflowed")))?;
            if entries > MAX_RELEASE_TREE_ENTRIES {
                return Err(IosError::storage(format!(
                    "{label} '{}' exceeds {MAX_RELEASE_TREE_ENTRIES} entries",
                    path.display()
                )));
            }
            let metadata = fs::symlink_metadata(child.path()).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect {label} entry '{}': {error}",
                    child.path().display()
                ))
            })?;
            if metadata.file_type().is_dir() {
                pending.push_back((child.path(), depth + 1));
            } else if metadata.file_type().is_file() {
                let file = open_regular_file_nofollow(&child.path()).map_err(|error| {
                    IosError::storage(format!(
                        "failed to open {label} file '{}': {error}",
                        child.path().display()
                    ))
                })?;
                file.sync_all().map_err(|error| {
                    IosError::storage(format!(
                        "failed to sync {label} file '{}': {error}",
                        child.path().display()
                    ))
                })?;
            } else {
                return Err(IosError::storage(format!(
                    "{label} contains a symlink or special file: {}",
                    child.path().display()
                )));
            }
        }
    }
    for directory in directories.into_iter().rev() {
        sync_directory(&directory).map_err(|error| {
            IosError::storage(format!(
                "failed to sync {label} directory '{}': {error}",
                directory.display()
            ))
        })?;
    }
    Ok(())
}

#[derive(Debug)]
enum JournalPersistenceFailure {
    BeforePublish(IosError),
    AfterPublish(IosError),
}

impl JournalPersistenceFailure {
    fn publication_may_be_visible(&self) -> bool {
        matches!(self, Self::AfterPublish(_))
    }

    fn into_error(self) -> IosError {
        match self {
            Self::BeforePublish(error) | Self::AfterPublish(error) => error,
        }
    }
}

fn persist_release_journal(
    path: &Path,
    journal: &ReleasePromotionJournal,
    replace: bool,
) -> Result<(), JournalPersistenceFailure> {
    persist_release_journal_with_sync(path, journal, replace, sync_directory)
}

fn persist_release_journal_with_sync(
    path: &Path,
    journal: &ReleasePromotionJournal,
    replace: bool,
    sync_parent: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), JournalPersistenceFailure> {
    let parent = (|| {
        let parent = path.parent().ok_or_else(|| {
            IosError::storage(format!(
                "iOS release journal '{}' has no parent",
                path.display()
            ))
        })?;
        let bytes = serde_json::to_vec_pretty(journal).map_err(|error| {
            IosError::worker(format!("failed to serialize iOS release journal: {error}"))
        })?;
        if bytes.len() as u64 > MAX_RELEASE_JOURNAL_BYTES {
            return Err(IosError::worker(format!(
                "iOS release journal exceeds {MAX_RELEASE_JOURNAL_BYTES} bytes"
            )));
        }
        let mut temporary = tempfile::Builder::new()
            .prefix(".ios-release-journal-")
            .tempfile_in(parent)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS release journal beside '{}': {error}",
                    path.display()
                ))
            })?;
        temporary.write_all(&bytes).map_err(|error| {
            IosError::storage(format!(
                "failed to write iOS release journal '{}': {error}",
                temporary.path().display()
            ))
        })?;
        temporary.as_file_mut().sync_all().map_err(|error| {
            IosError::storage(format!(
                "failed to sync iOS release journal '{}': {error}",
                temporary.path().display()
            ))
        })?;
        if replace {
            temporary.persist(path).map_err(|error| {
                IosError::storage(format!(
                    "failed to replace iOS release journal '{}': {}",
                    path.display(),
                    error.error
                ))
            })?;
        } else {
            temporary.persist_noclobber(path).map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS release journal '{}': {}",
                    path.display(),
                    error.error
                ))
            })?;
        }
        Ok(parent.to_path_buf())
    })()
    .map_err(JournalPersistenceFailure::BeforePublish)?;

    sync_parent(&parent).map_err(|error| {
        JournalPersistenceFailure::AfterPublish(IosError::storage(format!(
            "failed to sync iOS release journal directory '{}': {error}",
            parent.display()
        )))
    })
}

struct LoadedReleaseJournal {
    journal: ReleasePromotionJournal,
    identity: FileIdentity,
}

fn read_release_journal(path: &Path) -> Result<Option<LoadedReleaseJournal>, IosError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(IosError::storage(format!(
                "failed to inspect iOS release journal '{}': {error}",
                path.display()
            )));
        }
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_RELEASE_JOURNAL_BYTES {
        return Err(IosError::storage(format!(
            "iOS release journal '{}' is not a bounded regular non-symlink file",
            path.display()
        )));
    }
    let identity = path_identity(path, &metadata)?;
    let file = open_regular_file_nofollow(path).map_err(|error| {
        IosError::storage(format!(
            "failed to open iOS release journal '{}': {error}",
            path.display()
        ))
    })?;
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len()).unwrap_or(MAX_RELEASE_JOURNAL_BYTES as usize),
    );
    file.take(MAX_RELEASE_JOURNAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            IosError::storage(format!(
                "failed to read iOS release journal '{}': {error}",
                path.display()
            ))
        })?;
    if bytes.len() as u64 > MAX_RELEASE_JOURNAL_BYTES {
        return Err(IosError::storage(format!(
            "iOS release journal '{}' exceeds {MAX_RELEASE_JOURNAL_BYTES} bytes",
            path.display()
        )));
    }
    let final_metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::storage(format!(
            "failed to re-inspect iOS release journal '{}': {error}",
            path.display()
        ))
    })?;
    if !final_metadata.file_type().is_file()
        || path_identity(path, &final_metadata)? != identity
        || final_metadata.len() != metadata.len()
    {
        return Err(IosError::storage(format!(
            "iOS release journal '{}' changed while it was read",
            path.display()
        )));
    }
    serde_json::from_slice(&bytes)
        .map(|journal| Some(LoadedReleaseJournal { journal, identity }))
        .map_err(|error| {
            IosError::storage(format!(
                "failed to parse iOS release journal '{}': {error}",
                path.display()
            ))
        })
}

fn confirm_release_journal_durable(
    path: &Path,
    expected_identity: FileIdentity,
    sync_parent: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), IosError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::storage(format!(
            "failed to inspect iOS release journal '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() || path_identity(path, &metadata)? != expected_identity {
        return Err(IosError::storage(format!(
            "iOS release journal '{}' changed before durability confirmation",
            path.display()
        )));
    }
    let file = open_regular_file_nofollow(path).map_err(|error| {
        IosError::storage(format!(
            "failed to reopen iOS release journal '{}': {error}",
            path.display()
        ))
    })?;
    let opened_metadata = file.metadata().map_err(|error| {
        IosError::storage(format!(
            "failed to inspect opened iOS release journal '{}': {error}",
            path.display()
        ))
    })?;
    if !opened_metadata.file_type().is_file()
        || path_identity(path, &opened_metadata)? != expected_identity
    {
        return Err(IosError::storage(format!(
            "iOS release journal '{}' changed while confirming durability",
            path.display()
        )));
    }
    file.sync_all().map_err(|error| {
        IosError::storage(format!(
            "failed to sync iOS release journal '{}': {error}",
            path.display()
        ))
    })?;
    let parent = path.parent().ok_or_else(|| {
        IosError::storage(format!(
            "iOS release journal '{}' has no parent",
            path.display()
        ))
    })?;
    sync_parent(parent).map_err(|error| {
        IosError::storage(format!(
            "failed to confirm iOS release journal directory '{}': {error}",
            parent.display()
        ))
    })?;
    let final_metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::storage(format!(
            "failed to re-inspect iOS release journal '{}': {error}",
            path.display()
        ))
    })?;
    if !final_metadata.file_type().is_file()
        || path_identity(path, &final_metadata)? != expected_identity
    {
        return Err(IosError::storage(format!(
            "iOS release journal '{}' changed while confirming durability",
            path.display()
        )));
    }
    Ok(())
}

fn remove_release_journal(path: &Path, expected_identity: FileIdentity) -> Result<(), IosError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::storage(format!(
            "failed to inspect iOS release journal '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() || path_identity(path, &metadata)? != expected_identity {
        return Err(IosError::storage(format!(
            "iOS release journal '{}' changed before cleanup",
            path.display()
        )));
    }
    fs::remove_file(path).map_err(|error| {
        IosError::storage(format!(
            "failed to remove iOS release journal '{}': {error}",
            path.display()
        ))
    })?;
    let parent = path.parent().ok_or_else(|| {
        IosError::storage(format!(
            "iOS release journal '{}' has no parent",
            path.display()
        ))
    })?;
    sync_directory(parent).map_err(|error| {
        IosError::storage(format!(
            "failed to sync iOS release journal directory '{}': {error}",
            parent.display()
        ))
    })
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
fn recover_release_journal(root: &Path, path: &Path) -> Result<(), IosError> {
    recover_release_journal_with_cancellation(root, path, None)
}

fn recover_release_journal_interruptible(root: &Path, path: &Path) -> Result<(), IosError> {
    let cancellation =
        external_process::InterruptDeferral::start("iOS release transaction recovery")
            .map_err(map_process_error)?;
    let result = recover_release_journal_with_cancellation(root, path, Some(&cancellation));
    let cancelled = cancellation.finish();
    match (result, cancelled) {
        (Ok(()), true) => Err(IosError::worker(
            "iOS release transaction recovery was cancelled",
        )),
        (Err(error), true) if error.kind() != crate::ios::IosErrorKind::Worker => Err(
            append_error(error, "iOS release transaction recovery was cancelled"),
        ),
        (result, _) => result,
    }
}

fn recover_release_journal_with_cancellation(
    root: &Path,
    path: &Path,
    cancellation: Option<&external_process::InterruptDeferral>,
) -> Result<(), IosError> {
    let mut journal_parent_sync = sync_directory;
    let mut recovery_parent_sync = sync_directory;
    recover_release_journal_with_sync(
        root,
        path,
        cancellation,
        &mut journal_parent_sync,
        &mut recovery_parent_sync,
    )
}

fn recover_release_journal_with_sync(
    root: &Path,
    path: &Path,
    cancellation: Option<&external_process::InterruptDeferral>,
    journal_parent_sync: &mut impl FnMut(&Path) -> io::Result<()>,
    recovery_parent_sync: &mut impl FnMut(&Path) -> io::Result<()>,
) -> Result<(), IosError> {
    let Some(loaded) = read_release_journal(path)? else {
        return Ok(());
    };
    let journal = loaded.journal;
    validate_release_journal(root, &journal)?;
    confirm_release_journal_durable(path, loaded.identity, |parent| journal_parent_sync(parent))?;
    let recovery_cancellation = match journal.decision {
        JournalDecision::Rollback => cancellation,
        JournalDecision::Commit => None,
    };
    preflight_recovery_record(&journal.release, journal.decision, recovery_cancellation)?;
    if let Some(package) = journal.package.as_ref() {
        preflight_recovery_record(package, journal.decision, recovery_cancellation)?;
    }
    check_release_scan_cancellation(recovery_cancellation, "iOS release transaction recovery")?;
    match journal.decision {
        JournalDecision::Rollback => {
            if let Some(package) = journal.package.as_ref() {
                finish_promotion_record_with_parent_sync(
                    package,
                    JournalDecision::Rollback,
                    recovery_cancellation,
                    recovery_parent_sync,
                )?;
            }
            check_release_scan_cancellation(
                recovery_cancellation,
                "iOS release transaction recovery",
            )?;
            finish_promotion_record_with_parent_sync(
                &journal.release,
                JournalDecision::Rollback,
                recovery_cancellation,
                recovery_parent_sync,
            )?;
        }
        JournalDecision::Commit => {
            finish_promotion_record_with_parent_sync(
                &journal.release,
                JournalDecision::Commit,
                recovery_cancellation,
                recovery_parent_sync,
            )?;
            check_release_scan_cancellation(
                recovery_cancellation,
                "iOS release transaction recovery",
            )?;
            if let Some(package) = journal.package.as_ref() {
                finish_promotion_record_with_parent_sync(
                    package,
                    JournalDecision::Commit,
                    recovery_cancellation,
                    recovery_parent_sync,
                )?;
            }
        }
    }
    check_release_scan_cancellation(recovery_cancellation, "iOS release transaction recovery")?;
    remove_release_journal(path, loaded.identity)
}

fn preflight_recovery_record(
    record: &DirectoryPromotionRecord,
    decision: JournalDecision,
    cancellation: Option<&external_process::InterruptDeferral>,
) -> Result<(), IosError> {
    let placement = classify_promotion_record_with_cancellation(record, cancellation)?;
    let compatible = match decision {
        JournalDecision::Rollback => matches!(
            placement,
            PromotionPlacement::Before
                | PromotionPlacement::After
                | PromotionPlacement::RollbackCleanupPending
                | PromotionPlacement::RolledBackAndCleaned
        ),
        JournalDecision::Commit => matches!(
            placement,
            PromotionPlacement::Before
                | PromotionPlacement::After
                | PromotionPlacement::CommitCleanupPending
                | PromotionPlacement::CommittedAndCleaned
        ),
    };
    if compatible {
        Ok(())
    } else {
        Err(IosError::storage(format!(
            "iOS release transaction target '{}' cannot satisfy its durable decision",
            record.target.display()
        )))
    }
}

fn validate_release_journal(
    root: &Path,
    journal: &ReleasePromotionJournal,
) -> Result<(), IosError> {
    if journal.version != RELEASE_JOURNAL_VERSION {
        return Err(IosError::storage(format!(
            "unsupported iOS release journal version {}",
            journal.version
        )));
    }
    let canonical_root = fs::canonicalize(root).map_err(|error| {
        IosError::storage(format!(
            "failed to resolve iOS release journal root '{}': {error}",
            root.display()
        ))
    })?;
    if journal.root != canonical_root
        || directory_identity(&canonical_root, "iOS release journal root")? != journal.root_identity
    {
        return Err(IosError::storage(
            "iOS release journal belongs to a different repository identity",
        ));
    }
    if journal.transaction_id == [0; 16] {
        return Err(IosError::storage(
            "iOS release journal has an invalid nil transaction identity",
        ));
    }
    let expected_state_directory =
        canonical_root.join("lib/ios/VesperPlayerKit/.build/vesper-cli-state");
    if journal.state_directory != expected_state_directory
        || directory_identity(
            &expected_state_directory,
            "iOS release journal state directory",
        )? != journal.state_directory_identity
        || journal.state_directory_identity != journal.journal_parent_identity
    {
        return Err(IosError::storage(
            "iOS release journal state directory changed identity",
        ));
    }
    if journal.package_enabled != journal.package.is_some() {
        return Err(IosError::storage(
            "iOS release journal package leg does not match its transaction shape",
        ));
    }
    validate_promotion_record(root, &journal.release, false)?;
    if let Some(package) = journal.package.as_ref() {
        validate_promotion_record(root, package, true)?;
        validate_non_overlapping_paths(&journal.release.target, &package.target)?;
    }
    Ok(())
}

fn validate_promotion_record(
    root: &Path,
    record: &DirectoryPromotionRecord,
    package: bool,
) -> Result<(), IosError> {
    for path in [
        &record.parent,
        &record.target,
        &record.source,
        &record.owner,
    ] {
        if !path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        {
            return Err(IosError::storage(format!(
                "iOS release journal contains an invalid path '{}'",
                path.display()
            )));
        }
        reject_symlink_components(path, "iOS release journal path")?;
    }
    if record.target.parent() != Some(record.parent.as_path())
        || record.owner.parent() != Some(record.parent.as_path())
    {
        return Err(IosError::storage(
            "iOS release journal paths do not share the recorded parent",
        ));
    }
    if directory_identity(&record.parent, "iOS release journal parent")? != record.parent_identity {
        return Err(IosError::storage(format!(
            "iOS release journal parent '{}' changed identity",
            record.parent.display()
        )));
    }
    if package {
        if record.target.file_name() != Some(OsStr::new("Artifacts"))
            || record.source != record.owner.join("Artifacts")
            || !record.owner.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with(".vesper-ios-package-stage-")
            })
        {
            return Err(IosError::storage(
                "iOS release journal contains invalid package staging paths",
            ));
        }
        validate_package_artifacts_location(
            root,
            &root.join("lib/ios/VesperPlayerOptionalPlugins/Artifacts"),
            &record.target,
            true,
        )?;
    } else {
        if record.source != record.owner
            || !record.owner.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with(".vesper-ios-release-stage-")
            })
        {
            return Err(IosError::storage(
                "iOS release journal contains invalid release staging paths",
            ));
        }
        validate_release_output_location(root, &record.target)?;
    }
    Ok(())
}

fn classify_promotion_record_with_cancellation(
    record: &DirectoryPromotionRecord,
    cancellation: Option<&external_process::InterruptDeferral>,
) -> Result<PromotionPlacement, IosError> {
    let target = optional_directory_snapshot_with_cancellation(
        &record.target,
        "iOS release transaction target",
        cancellation,
    )?;
    let owner_identity =
        optional_directory_identity(&record.owner, "iOS release transaction recovery owner")?;
    let source = optional_directory_snapshot_with_cancellation(
        &record.source,
        "iOS release transaction source",
        cancellation,
    );

    if target == record.old {
        return match owner_identity {
            None => Ok(PromotionPlacement::RolledBackAndCleaned),
            Some(identity) if identity == record.owner_identity => match source {
                Ok(source) if source.as_ref() == Some(&record.new) => {
                    Ok(PromotionPlacement::Before)
                }
                Err(error) if error.kind() == crate::ios::IosErrorKind::Worker => Err(error),
                _ => Ok(PromotionPlacement::RollbackCleanupPending),
            },
            Some(_) => Err(unknown_promotion_placement(record)),
        };
    }

    if target.as_ref() == Some(&record.new) {
        if record.owner == record.source && record.old.is_none() {
            return if owner_identity.is_none() {
                Ok(PromotionPlacement::After)
            } else {
                Err(unknown_promotion_placement(record))
            };
        }

        let expected_owner_identity = record
            .old
            .as_ref()
            .filter(|_| record.owner == record.source)
            .map_or(record.owner_identity, |old| old.identity);
        return match owner_identity {
            None => Ok(PromotionPlacement::CommittedAndCleaned),
            Some(identity) if identity == expected_owner_identity => match source {
                Ok(source) if source == record.old => Ok(PromotionPlacement::After),
                Err(error) if error.kind() == crate::ios::IosErrorKind::Worker => Err(error),
                _ => Ok(PromotionPlacement::CommitCleanupPending),
            },
            Some(_) => Err(unknown_promotion_placement(record)),
        };
    }

    Err(unknown_promotion_placement(record))
}

fn unknown_promotion_placement(record: &DirectoryPromotionRecord) -> IosError {
    IosError::storage(format!(
        "iOS release transaction paths '{}' and '{}' have unknown identities; recovery stopped",
        record.target.display(),
        record.source.display()
    ))
}

fn optional_directory_identity(path: &Path, label: &str) -> Result<Option<FileIdentity>, IosError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => path_identity(path, &metadata).map(Some),
        Ok(_) => Err(IosError::storage(format!(
            "{label} '{}' is not a regular non-symlink directory",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(IosError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))),
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
fn apply_promotion_record(record: &DirectoryPromotionRecord) -> Result<(), IosError> {
    apply_promotion_record_with_cancellation(record, None)
}

fn apply_promotion_record_with_cancellation(
    record: &DirectoryPromotionRecord,
    cancellation: Option<&external_process::InterruptDeferral>,
) -> Result<(), IosError> {
    if classify_promotion_record_with_cancellation(record, cancellation)?
        != PromotionPlacement::Before
    {
        return Err(IosError::storage(format!(
            "iOS release transaction target '{}' is not ready for promotion",
            record.target.display()
        )));
    }
    if record.old.is_some() {
        exchange_paths(&record.source, &record.target).map_err(|error| {
            IosError::storage(format!(
                "failed to atomically exchange iOS release directory '{}': {error}",
                record.target.display()
            ))
        })?;
    } else {
        rename_noreplace(&record.source, &record.target).map_err(|error| {
            IosError::storage(format!(
                "failed to atomically publish iOS release directory '{}': {error}",
                record.target.display()
            ))
        })?;
    }
    sync_directory(&record.parent).map_err(|error| {
        IosError::storage(format!(
            "failed to sync iOS release transaction parent '{}': {error}",
            record.parent.display()
        ))
    })?;
    if classify_promotion_record_with_cancellation(record, cancellation)?
        == PromotionPlacement::After
    {
        Ok(())
    } else {
        Err(IosError::storage(format!(
            "iOS release transaction target '{}' has an unexpected identity after promotion",
            record.target.display()
        )))
    }
}

fn rollback_promotion_record_with_cancellation(
    record: &DirectoryPromotionRecord,
    cancellation: Option<&external_process::InterruptDeferral>,
) -> Result<(), IosError> {
    if classify_promotion_record_with_cancellation(record, cancellation)?
        != PromotionPlacement::After
    {
        return Err(IosError::storage(format!(
            "iOS release transaction target '{}' is not ready for rollback",
            record.target.display()
        )));
    }
    if record.old.is_some() {
        exchange_paths(&record.source, &record.target).map_err(|error| {
            IosError::storage(format!(
                "failed to restore previous iOS release directory '{}': {error}",
                record.target.display()
            ))
        })?;
    } else {
        rename_noreplace(&record.target, &record.source).map_err(|error| {
            IosError::storage(format!(
                "failed to remove newly published iOS release directory '{}': {error}",
                record.target.display()
            ))
        })?;
    }
    sync_directory(&record.parent).map_err(|error| {
        IosError::storage(format!(
            "failed to sync restored iOS release transaction parent '{}': {error}",
            record.parent.display()
        ))
    })?;
    if classify_promotion_record_with_cancellation(record, cancellation)?
        == PromotionPlacement::Before
    {
        Ok(())
    } else {
        Err(IosError::storage(format!(
            "iOS release transaction target '{}' has an unexpected identity after rollback",
            record.target.display()
        )))
    }
}

fn finish_promotion_record_with_parent_sync(
    record: &DirectoryPromotionRecord,
    decision: JournalDecision,
    cancellation: Option<&external_process::InterruptDeferral>,
    sync_parent: &mut impl FnMut(&Path) -> io::Result<()>,
) -> Result<(), IosError> {
    let placement = classify_promotion_record_with_cancellation(record, cancellation)?;
    match (decision, placement) {
        (JournalDecision::Rollback, PromotionPlacement::After) => {
            rollback_promotion_record_with_cancellation(record, cancellation)?;
        }
        (JournalDecision::Rollback, PromotionPlacement::Before)
        | (JournalDecision::Rollback, PromotionPlacement::RollbackCleanupPending)
        | (JournalDecision::Rollback, PromotionPlacement::RolledBackAndCleaned)
        | (JournalDecision::Commit, PromotionPlacement::After)
        | (JournalDecision::Commit, PromotionPlacement::CommitCleanupPending)
        | (JournalDecision::Commit, PromotionPlacement::CommittedAndCleaned) => {}
        (JournalDecision::Commit, PromotionPlacement::Before) => {
            apply_promotion_record_with_cancellation(record, cancellation)?;
        }
        (JournalDecision::Rollback, PromotionPlacement::CommitCleanupPending)
        | (JournalDecision::Rollback, PromotionPlacement::CommittedAndCleaned)
        | (JournalDecision::Commit, PromotionPlacement::RollbackCleanupPending)
        | (JournalDecision::Commit, PromotionPlacement::RolledBackAndCleaned) => {
            return Err(IosError::storage(format!(
                "iOS release transaction target '{}' cannot satisfy its durable decision",
                record.target.display()
            )));
        }
    }

    let placement = classify_promotion_record_with_cancellation(record, cancellation)?;
    let cleanup_needed = matches!(
        (decision, placement),
        (JournalDecision::Rollback, PromotionPlacement::Before)
            | (
                JournalDecision::Rollback,
                PromotionPlacement::RollbackCleanupPending
            )
            | (JournalDecision::Commit, PromotionPlacement::After)
            | (
                JournalDecision::Commit,
                PromotionPlacement::CommitCleanupPending
            )
    );
    if cleanup_needed {
        check_release_scan_cancellation(cancellation, "iOS release recovery cleanup")?;
        let expected_owner_identity = match (decision, record.owner == record.source) {
            (JournalDecision::Rollback, true) => record.new.identity,
            (JournalDecision::Commit, true) => record
                .old
                .as_ref()
                .map(|snapshot| snapshot.identity)
                .ok_or_else(|| {
                    IosError::storage(format!(
                        "iOS release recovery owner '{}' is absent after publishing a new target",
                        record.owner.display()
                    ))
                })?,
            (_, false) => record.owner_identity,
        };
        let metadata = fs::symlink_metadata(&record.owner).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect iOS release recovery owner '{}': {error}",
                record.owner.display()
            ))
        })?;
        if !metadata.file_type().is_dir()
            || path_identity(&record.owner, &metadata)? != expected_owner_identity
        {
            return Err(IosError::storage(format!(
                "iOS release recovery owner '{}' changed before cleanup",
                record.owner.display()
            )));
        }
        fs::remove_dir_all(&record.owner).map_err(|error| {
            IosError::storage(format!(
                "failed to remove iOS release recovery owner '{}': {error}",
                record.owner.display()
            ))
        })?;
    }
    let placement = classify_promotion_record_with_cancellation(record, cancellation)?;
    let satisfied = match decision {
        JournalDecision::Rollback => placement == PromotionPlacement::RolledBackAndCleaned,
        JournalDecision::Commit => {
            placement == PromotionPlacement::CommittedAndCleaned
                || (record.owner == record.source
                    && record.old.is_none()
                    && placement == PromotionPlacement::After)
        }
    };
    if satisfied {
        sync_parent(&record.parent).map_err(|error| {
            IosError::storage(format!(
                "failed to sync iOS release recovery parent '{}': {error}",
                record.parent.display()
            ))
        })
    } else {
        Err(IosError::storage(format!(
            "iOS release transaction target '{}' did not reach its durable cleanup state",
            record.target.display()
        )))
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "release promotion is a single transaction boundary with explicit identity, snapshot, journal, and cancellation inputs"
)]
fn promote_release_outputs(
    release_stage: tempfile::TempDir,
    output_directory: &Path,
    staged_assets: &[OsString],
    include_optional_plugins: bool,
    package_stage: Option<(tempfile::TempDir, PathBuf)>,
    package_target: Option<&Path>,
    output_identity: FileIdentity,
    output_parent_identity: FileIdentity,
    output_snapshot: DirectorySnapshot,
    package_parent_identity: Option<FileIdentity>,
    package_snapshot: Option<DirectorySnapshot>,
    root: &Path,
    journal_path: &Path,
    state_directory_identity: FileIdentity,
) -> Result<(), IosError> {
    let cancellation = external_process::InterruptDeferral::start("iOS release output promotion")
        .map_err(map_process_error)?;
    let result = (|| {
        if output_snapshot.identity != output_identity {
            return Err(IosError::storage(
                "iOS release output identity changed before promotion",
            ));
        }
        populate_release_candidate(
            release_stage.path(),
            output_directory,
            staged_assets,
            &output_snapshot,
            &cancellation,
        )?;
        sync_directory_tree(release_stage.path(), "iOS release candidate", &cancellation)?;
        let release_new = directory_snapshot_with_cancellation(
            release_stage.path(),
            "iOS release candidate",
            Some(&cancellation),
        )?;
        let output_parent = output_directory.parent().ok_or_else(|| {
            IosError::storage("iOS release output has no parent during promotion")
        })?;
        if directory_identity(output_parent, "iOS release output parent")? != output_parent_identity
            || directory_snapshot_with_cancellation(
                output_directory,
                "iOS release output",
                Some(&cancellation),
            )? != output_snapshot
        {
            return Err(IosError::storage(format!(
                "iOS release output '{}' changed before promotion",
                output_directory.display()
            )));
        }
        let release_record = DirectoryPromotionRecord {
            parent: output_parent.to_path_buf(),
            parent_identity: output_parent_identity,
            target: output_directory.to_path_buf(),
            source: release_stage.path().to_path_buf(),
            owner: release_stage.path().to_path_buf(),
            owner_identity: directory_identity(
                release_stage.path(),
                "iOS release candidate owner",
            )?,
            old: Some(output_snapshot),
            new: release_new,
        };

        let mut package_owner = None;
        let package_record = if include_optional_plugins {
            let (owner, source) = package_stage.ok_or_else(|| {
                IosError::storage("optional iOS package staging state is unavailable")
            })?;
            let target = package_target
                .ok_or_else(|| IosError::storage("optional iOS package target is unavailable"))?;
            let parent = target
                .parent()
                .ok_or_else(|| IosError::storage("optional iOS package target has no parent"))?;
            let parent_identity = package_parent_identity.ok_or_else(|| {
                IosError::storage("optional iOS package parent identity is unavailable")
            })?;
            if directory_identity(parent, "iOS package artifacts parent")? != parent_identity
                || optional_directory_snapshot_with_cancellation(
                    target,
                    "iOS package artifacts target",
                    Some(&cancellation),
                )? != package_snapshot
            {
                return Err(IosError::storage(format!(
                    "iOS package artifacts target '{}' changed before promotion",
                    target.display()
                )));
            }
            sync_directory_tree(&source, "iOS package artifacts candidate", &cancellation)?;
            let new = directory_snapshot_with_cancellation(
                &source,
                "iOS package artifacts candidate",
                Some(&cancellation),
            )?;
            let record = DirectoryPromotionRecord {
                parent: parent.to_path_buf(),
                parent_identity,
                target: target.to_path_buf(),
                source,
                owner: owner.path().to_path_buf(),
                owner_identity: directory_identity(
                    owner.path(),
                    "iOS package artifacts candidate owner",
                )?,
                old: package_snapshot,
                new,
            };
            package_owner = Some(owner);
            Some(record)
        } else {
            if package_stage.is_some()
                || package_target.is_some()
                || package_parent_identity.is_some()
                || package_snapshot.is_some()
            {
                return Err(IosError::storage(
                    "unexpected optional iOS package staging state",
                ));
            }
            None
        };

        if cancellation.is_cancelled() {
            return Err(IosError::worker(
                "iOS release output promotion was cancelled",
            ));
        }
        let canonical_root = fs::canonicalize(root).map_err(|error| {
            IosError::storage(format!(
                "failed to resolve iOS release transaction root '{}': {error}",
                root.display()
            ))
        })?;
        let state_directory = journal_path.parent().ok_or_else(|| {
            IosError::storage(format!(
                "iOS release journal '{}' has no parent",
                journal_path.display()
            ))
        })?;
        if directory_identity(state_directory, "iOS release transaction state directory")?
            != state_directory_identity
        {
            return Err(IosError::storage(format!(
                "iOS release transaction state directory '{}' changed before journal creation",
                state_directory.display()
            )));
        }
        let mut transaction_id = [0_u8; 16];
        getrandom::fill(&mut transaction_id).map_err(|error| {
            IosError::storage(format!(
                "failed to obtain system randomness for iOS release transaction: {error}"
            ))
        })?;
        let mut journal = ReleasePromotionJournal {
            version: RELEASE_JOURNAL_VERSION,
            transaction_id,
            root: canonical_root.clone(),
            root_identity: directory_identity(&canonical_root, "iOS release transaction root")?,
            state_directory: state_directory.to_path_buf(),
            state_directory_identity,
            journal_parent_identity: state_directory_identity,
            decision: JournalDecision::Rollback,
            package_enabled: package_record.is_some(),
            release: release_record,
            package: package_record,
        };
        validate_release_journal(root, &journal)?;
        if let Err(failure) = persist_release_journal(journal_path, &journal, false) {
            let publication_may_be_visible = failure.publication_may_be_visible();
            let mut error = failure.into_error();
            if publication_may_be_visible {
                let release_owner_path = release_stage.keep();
                if release_owner_path != journal.release.owner {
                    error = append_error(
                        error,
                        "iOS release candidate owner changed after journal publication",
                    );
                }
                if let Some(owner) = package_owner.take() {
                    let owner_path = owner.keep();
                    if journal
                        .package
                        .as_ref()
                        .is_none_or(|record| record.owner != owner_path)
                    {
                        error = append_error(
                            error,
                            "iOS package candidate owner changed after journal publication",
                        );
                    }
                }
            }
            return Err(error);
        }

        let release_owner_path = release_stage.keep();
        if release_owner_path != journal.release.owner {
            return Err(rollback_release_journal_error(
                root,
                journal_path,
                IosError::worker("iOS release candidate owner changed while journaling"),
                &cancellation,
            ));
        }
        if let Some(owner) = package_owner {
            let owner_path = owner.keep();
            if journal
                .package
                .as_ref()
                .is_none_or(|record| record.owner != owner_path)
            {
                return Err(rollback_release_journal_error(
                    root,
                    journal_path,
                    IosError::worker("iOS package candidate owner changed while journaling"),
                    &cancellation,
                ));
            }
        }

        if let Err(error) =
            apply_promotion_record_with_cancellation(&journal.release, Some(&cancellation))
        {
            return Err(rollback_release_journal_error(
                root,
                journal_path,
                error,
                &cancellation,
            ));
        }
        if cancellation.is_cancelled() {
            return Err(rollback_release_journal_error(
                root,
                journal_path,
                IosError::worker("iOS release output promotion was cancelled"),
                &cancellation,
            ));
        }
        if let Some(package) = journal.package.as_ref()
            && let Err(error) =
                apply_promotion_record_with_cancellation(package, Some(&cancellation))
        {
            return Err(rollback_release_journal_error(
                root,
                journal_path,
                error,
                &cancellation,
            ));
        }
        let promoted_outputs_valid = (|| {
            if classify_promotion_record_with_cancellation(&journal.release, Some(&cancellation))?
                != PromotionPlacement::After
            {
                return Ok(false);
            }
            if journal
                .package
                .as_ref()
                .map(|record| {
                    classify_promotion_record_with_cancellation(record, Some(&cancellation))
                })
                .transpose()?
                .is_some_and(|placement| placement != PromotionPlacement::After)
            {
                return Ok(false);
            }
            Ok(true)
        })();
        match promoted_outputs_valid {
            Ok(true) => {}
            Ok(false) => {
                return Err(rollback_release_journal_error(
                    root,
                    journal_path,
                    IosError::storage("iOS release outputs changed before durable commit"),
                    &cancellation,
                ));
            }
            Err(error) => {
                return Err(rollback_release_journal_error(
                    root,
                    journal_path,
                    error,
                    &cancellation,
                ));
            }
        }
        if cancellation.is_cancelled() {
            return Err(rollback_release_journal_error(
                root,
                journal_path,
                IosError::worker("iOS release output promotion was cancelled"),
                &cancellation,
            ));
        }

        journal.decision = JournalDecision::Commit;
        match persist_release_journal(journal_path, &journal, true) {
            Ok(()) => {}
            Err(JournalPersistenceFailure::BeforePublish(error)) => {
                return Err(rollback_release_journal_error(
                    root,
                    journal_path,
                    error,
                    &cancellation,
                ));
            }
            Err(JournalPersistenceFailure::AfterPublish(error)) => {
                return Err(append_error(
                    error,
                    "the commit decision may be visible but is not confirmed durable; recovery is required before another release staging run",
                ));
            }
        }
        // The commit decision is durable. Finish both legs and remove the
        // journal before reporting a cancellation observed after this point.
        recover_release_journal_with_cancellation(root, journal_path, None)?;
        Ok(true)
    })();
    let cancelled = cancellation.finish();
    match result {
        Ok(true) if cancelled => Err(IosError::worker(
            "iOS release output promotion was cancelled after its durable decision",
        )),
        Ok(true) => Ok(()),
        Ok(false) => Err(IosError::worker(
            "iOS release output promotion ended without a durable decision",
        )),
        Err(error) if cancelled && error.kind() != crate::ios::IosErrorKind::Worker => Err(
            append_error(error, "iOS release output promotion was cancelled"),
        ),
        Err(error) => Err(error),
    }
}

fn rollback_release_journal_error(
    root: &Path,
    path: &Path,
    error: IosError,
    _cancellation: &external_process::InterruptDeferral,
) -> IosError {
    // Once a rollback decision is durable, cancellation is deferred until the
    // old output pair has been restored or the recovery journal reports an error.
    match recover_release_journal_with_cancellation(root, path, None) {
        Ok(()) => error,
        Err(recovery) => append_error(error, recovery.to_string()),
    }
}

fn append_error(error: IosError, suffix: impl AsRef<str>) -> IosError {
    let message = format!("{error}; {}", suffix.as_ref());
    match error.kind() {
        crate::ios::IosErrorKind::Storage => IosError::storage(message),
        crate::ios::IosErrorKind::Compatibility => IosError::compatibility(message),
        crate::ios::IosErrorKind::Conformance => IosError::conformance(message),
        crate::ios::IosErrorKind::Worker => IosError::worker(message),
    }
}

fn directory_identity(path: &Path, label: &str) -> Result<FileIdentity, IosError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(IosError::storage(format!(
            "{label} '{}' is not a regular non-symlink directory",
            path.display()
        )));
    }
    path_identity(path, &metadata)
}

fn path_identity(path: &Path, metadata: &fs::Metadata) -> Result<FileIdentity, IosError> {
    platform_path_identity(path, metadata).map_err(|error| {
        IosError::storage(format!(
            "failed to identify iOS release path '{}': {error}",
            path.display()
        ))
    })
}

#[cfg(unix)]
fn platform_path_identity(_path: &Path, metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    Ok(FileIdentity {
        volume_or_device: metadata.dev(),
        file_index: metadata.ino(),
    })
}

#[cfg(windows)]
fn platform_path_identity(path: &Path, _metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    let handle = winapi_util::Handle::from_path_any(path)?;
    let information = winapi_util::file::information(&handle)?;
    Ok(FileIdentity {
        volume_or_device: information.volume_serial_number(),
        file_index: information.file_index(),
    })
}

#[cfg(not(any(unix, windows)))]
fn platform_path_identity(_path: &Path, _metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "file identity is unsupported on this host",
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
fn exchange_paths(left: &Path, right: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, left, CWD, right, RenameFlags::EXCHANGE).map_err(io::Error::from)
}

#[cfg(windows)]
fn exchange_paths(_left: &Path, _right: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic directory exchange is unavailable on Windows",
    ))
}

#[cfg(unix)]
fn rename_noreplace(source: &Path, target: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, source, CWD, target, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

#[cfg(windows)]
fn rename_noreplace(source: &Path, target: &Path) -> io::Result<()> {
    match fs::symlink_metadata(target) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "target already exists",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::rename(source, target),
        Err(error) => Err(error),
    }
}

#[cfg(not(any(unix, windows)))]
fn rename_noreplace(source: &Path, target: &Path) -> io::Result<()> {
    match fs::symlink_metadata(target) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "target already exists",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::rename(source, target),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn durable_prepared_directory_commit_syncs_leaf_and_created_parents() {
        let directory = tempfile::tempdir().expect("temporary prepared directory fixture");
        let target = directory.path().join("first/second/aggregate");
        let prepared = PreparedDirectory::prepare(&target, "fixture aggregate directory")
            .expect("prepare nested aggregate directory");
        let mut synced = Vec::new();

        prepared
            .commit_durable_with_sync("fixture aggregate directory", |path| {
                synced.push(path.to_path_buf());
                Ok(())
            })
            .expect("durably commit nested aggregate directory");

        let canonical_fixture =
            fs::canonicalize(directory.path()).expect("canonical fixture directory");
        let canonical_target = canonical_fixture.join("first/second/aggregate");
        assert_eq!(
            synced,
            vec![
                canonical_target.clone(),
                canonical_target
                    .parent()
                    .expect("aggregate parent")
                    .to_path_buf(),
                canonical_target
                    .parent()
                    .and_then(Path::parent)
                    .expect("aggregate grandparent")
                    .to_path_buf(),
                canonical_fixture,
            ]
        );
        assert!(target.is_dir());
    }

    #[test]
    fn failed_durable_prepared_directory_commit_does_not_publish_success() {
        let directory = tempfile::tempdir().expect("temporary prepared directory fixture");
        let target = directory.path().join("aggregate/inputs");
        let prepared = PreparedDirectory::prepare(&target, "fixture aggregate directory")
            .expect("prepare aggregate directory");
        let mut calls = 0_usize;

        let error = prepared
            .commit_durable_with_sync("fixture aggregate directory", |_| {
                calls += 1;
                if calls == 2 {
                    Err(io::Error::other("injected directory sync failure"))
                } else {
                    Ok(())
                }
            })
            .expect_err("surface aggregate directory sync failure");

        assert!(
            error
                .to_string()
                .contains("injected directory sync failure")
        );
        assert_eq!(calls, 2);
        assert!(!target.exists());
        assert!(!directory.path().join("aggregate").exists());
    }

    struct RecoveryFixture {
        _directory: tempfile::TempDir,
        root: PathBuf,
        journal_path: PathBuf,
        journal: ReleasePromotionJournal,
    }

    fn snapshot(path: &Path, label: &str) -> DirectorySnapshot {
        directory_snapshot_with_cancellation(path, label, None).expect("snapshot fixture directory")
    }

    fn recovery_fixture() -> RecoveryFixture {
        let directory = tempfile::tempdir().expect("temporary iOS recovery fixture");
        let root = directory.path().join("repository");
        let state = root.join("lib/ios/VesperPlayerKit/.build/vesper-cli-state");
        let release_parent = root.join("dist/release");
        let release_target = release_parent.join("ios");
        let release_source = release_parent.join(".vesper-ios-release-stage-fixture");
        let package_parent = root.join("dist/package");
        let package_target = package_parent.join("Artifacts");
        let package_owner = package_parent.join(".vesper-ios-package-stage-fixture");
        let package_source = package_owner.join("Artifacts");
        for path in [
            &state,
            &release_target,
            &release_source,
            &package_target,
            &package_source,
        ] {
            fs::create_dir_all(path).expect("create iOS recovery fixture directory");
        }
        fs::write(release_target.join("old.txt"), b"old release\n")
            .expect("write old release fixture");
        fs::write(release_source.join("new.txt"), b"new release\n")
            .expect("write new release fixture");
        fs::create_dir_all(package_target.join("VesperFFmpegAVCodec.xcframework"))
            .expect("create old package fixture");
        fs::write(
            package_target.join("VesperFFmpegAVCodec.xcframework/old.txt"),
            b"old package\n",
        )
        .expect("write old package fixture");
        fs::create_dir_all(package_source.join("VesperFFmpegAVCodec.xcframework"))
            .expect("create new package fixture");
        fs::write(
            package_source.join("VesperFFmpegAVCodec.xcframework/new.txt"),
            b"new package\n",
        )
        .expect("write new package fixture");

        let release = DirectoryPromotionRecord {
            parent: release_parent.clone(),
            parent_identity: directory_identity(&release_parent, "release parent")
                .expect("identify release parent"),
            target: release_target.clone(),
            source: release_source.clone(),
            owner: release_source.clone(),
            owner_identity: directory_identity(&release_source, "release owner")
                .expect("identify release owner"),
            old: Some(snapshot(&release_target, "old release")),
            new: snapshot(&release_source, "new release"),
        };
        let package = DirectoryPromotionRecord {
            parent: package_parent.clone(),
            parent_identity: directory_identity(&package_parent, "package parent")
                .expect("identify package parent"),
            target: package_target.clone(),
            source: package_source.clone(),
            owner: package_owner.clone(),
            owner_identity: directory_identity(&package_owner, "package owner")
                .expect("identify package owner"),
            old: Some(snapshot(&package_target, "old package")),
            new: snapshot(&package_source, "new package"),
        };
        let canonical_root = fs::canonicalize(&root).expect("canonical recovery root");
        let canonical_state = fs::canonicalize(&state).expect("canonical recovery state");
        let state_identity = directory_identity(&canonical_state, "recovery state")
            .expect("identify recovery state");
        let journal = ReleasePromotionJournal {
            version: RELEASE_JOURNAL_VERSION,
            transaction_id: [1; 16],
            root: canonical_root.clone(),
            root_identity: directory_identity(&canonical_root, "recovery root")
                .expect("identify recovery root"),
            state_directory: canonical_state,
            state_directory_identity: state_identity,
            journal_parent_identity: state_identity,
            decision: JournalDecision::Rollback,
            package_enabled: true,
            release,
            package: Some(package),
        };
        RecoveryFixture {
            _directory: directory,
            root,
            journal_path: state.join(RELEASE_JOURNAL_FILE),
            journal,
        }
    }

    #[test]
    fn recovery_rolls_back_a_partial_directory_exchange() {
        let fixture = recovery_fixture();
        persist_release_journal(&fixture.journal_path, &fixture.journal, false)
            .expect("persist rollback journal");
        apply_promotion_record(&fixture.journal.release).expect("promote release fixture");

        recover_release_journal(&fixture.root, &fixture.journal_path)
            .expect("recover partial release promotion");
        assert!(fixture.journal.release.target.join("old.txt").is_file());
        assert!(!fixture.journal.release.target.join("new.txt").exists());
        let package = fixture.journal.package.as_ref().expect("package record");
        assert!(
            package
                .target
                .join("VesperFFmpegAVCodec.xcframework/old.txt")
                .is_file()
        );
        assert!(!fixture.journal_path.exists());
        assert!(!fixture.journal.release.owner.exists());
        assert!(!package.owner.exists());
    }

    #[test]
    fn recovery_rolls_back_both_exchanges_before_commit_decision() {
        let fixture = recovery_fixture();
        persist_release_journal(&fixture.journal_path, &fixture.journal, false)
            .expect("persist rollback journal");
        apply_promotion_record(&fixture.journal.release).expect("promote release fixture");
        let package = fixture.journal.package.as_ref().expect("package record");
        apply_promotion_record(package).expect("promote package fixture");

        recover_release_journal(&fixture.root, &fixture.journal_path)
            .expect("recover uncommitted release promotion");
        assert!(fixture.journal.release.target.join("old.txt").is_file());
        assert!(
            package
                .target
                .join("VesperFFmpegAVCodec.xcframework/old.txt")
                .is_file()
        );
        assert!(!fixture.journal_path.exists());
    }

    #[test]
    fn recovery_finishes_cleanup_after_durable_commit_decision() {
        let mut fixture = recovery_fixture();
        persist_release_journal(&fixture.journal_path, &fixture.journal, false)
            .expect("persist rollback journal");
        apply_promotion_record(&fixture.journal.release).expect("promote release fixture");
        let package = fixture.journal.package.as_ref().expect("package record");
        apply_promotion_record(package).expect("promote package fixture");
        fixture.journal.decision = JournalDecision::Commit;
        persist_release_journal(&fixture.journal_path, &fixture.journal, true)
            .expect("persist commit decision");

        recover_release_journal(&fixture.root, &fixture.journal_path)
            .expect("finish committed release promotion");
        assert!(fixture.journal.release.target.join("new.txt").is_file());
        assert!(!fixture.journal.release.target.join("old.txt").exists());
        assert!(
            package
                .target
                .join("VesperFFmpegAVCodec.xcframework/new.txt")
                .is_file()
        );
        assert!(!fixture.journal_path.exists());
        assert!(!fixture.journal.release.owner.exists());
        assert!(!package.owner.exists());
    }

    fn remove_initial_package_target(fixture: &mut RecoveryFixture) {
        let package = fixture.journal.package.as_mut().expect("package record");
        fs::remove_dir_all(&package.target).expect("remove initial package target");
        package.old = None;
    }

    #[test]
    fn recovery_commits_and_cleans_an_initial_package_install() {
        let mut fixture = recovery_fixture();
        remove_initial_package_target(&mut fixture);
        persist_release_journal(&fixture.journal_path, &fixture.journal, false)
            .expect("persist initial-install rollback journal");
        apply_promotion_record(&fixture.journal.release).expect("promote release fixture");
        let package = fixture.journal.package.as_ref().expect("package record");
        apply_promotion_record(package).expect("promote initial package fixture");
        fixture.journal.decision = JournalDecision::Commit;
        persist_release_journal(&fixture.journal_path, &fixture.journal, true)
            .expect("persist initial-install commit decision");

        recover_release_journal(&fixture.root, &fixture.journal_path)
            .expect("finish initial package install");
        assert!(
            package
                .target
                .join("VesperFFmpegAVCodec.xcframework/new.txt")
                .is_file()
        );
        assert!(!package.owner.exists());
        assert!(!fixture.journal_path.exists());
    }

    #[test]
    fn recovery_rolls_back_an_initial_package_install() {
        let mut fixture = recovery_fixture();
        remove_initial_package_target(&mut fixture);
        persist_release_journal(&fixture.journal_path, &fixture.journal, false)
            .expect("persist initial-install rollback journal");
        apply_promotion_record(&fixture.journal.release).expect("promote release fixture");
        let package = fixture.journal.package.as_ref().expect("package record");
        apply_promotion_record(package).expect("promote initial package fixture");

        recover_release_journal(&fixture.root, &fixture.journal_path)
            .expect("roll back initial package install");
        assert!(!package.target.exists());
        assert!(!package.owner.exists());
        assert!(fixture.journal.release.target.join("old.txt").is_file());
        assert!(!fixture.journal_path.exists());
    }

    #[test]
    fn recovery_resumes_partial_commit_cleanup() {
        let mut fixture = recovery_fixture();
        persist_release_journal(&fixture.journal_path, &fixture.journal, false)
            .expect("persist rollback journal");
        apply_promotion_record(&fixture.journal.release).expect("promote release fixture");
        let package = fixture.journal.package.as_ref().expect("package record");
        apply_promotion_record(package).expect("promote package fixture");
        fixture.journal.decision = JournalDecision::Commit;
        persist_release_journal(&fixture.journal_path, &fixture.journal, true)
            .expect("persist commit decision");
        fs::remove_dir_all(&package.source).expect("simulate partial package cleanup");

        recover_release_journal(&fixture.root, &fixture.journal_path)
            .expect("resume committed package cleanup");
        assert!(
            package
                .target
                .join("VesperFFmpegAVCodec.xcframework/new.txt")
                .is_file()
        );
        assert!(!package.owner.exists());
        assert!(!fixture.journal_path.exists());
    }

    #[test]
    fn recovery_preflights_every_leg_before_mutation() {
        let fixture = recovery_fixture();
        persist_release_journal(&fixture.journal_path, &fixture.journal, false)
            .expect("persist rollback journal");
        apply_promotion_record(&fixture.journal.release).expect("promote release fixture");
        let package = fixture.journal.package.as_ref().expect("package record");
        apply_promotion_record(package).expect("promote package fixture");
        fs::write(fixture.journal.release.source.join("old.txt"), b"changed\n")
            .expect("corrupt release recovery source");

        let error = recover_release_journal(&fixture.root, &fixture.journal_path)
            .expect_err("reject a journal with one corrupted leg");
        assert!(
            error
                .to_string()
                .contains("cannot satisfy its durable decision")
        );
        assert!(fixture.journal.release.target.join("new.txt").is_file());
        assert!(
            package
                .target
                .join("VesperFFmpegAVCodec.xcframework/new.txt")
                .is_file()
        );
        assert!(fixture.journal_path.is_file());
    }

    #[test]
    fn recovery_rejects_a_journal_with_an_omitted_package_leg() {
        let fixture = recovery_fixture();
        persist_release_journal(&fixture.journal_path, &fixture.journal, false)
            .expect("persist rollback journal");
        let mut value = serde_json::to_value(&fixture.journal).expect("serialize fixture journal");
        value
            .as_object_mut()
            .expect("journal object")
            .remove("package");
        fs::write(
            &fixture.journal_path,
            serde_json::to_vec_pretty(&value).expect("serialize malformed journal"),
        )
        .expect("write malformed journal");

        recover_release_journal(&fixture.root, &fixture.journal_path)
            .expect_err("reject omitted package leg");
        assert!(fixture.journal.release.target.join("old.txt").is_file());
        assert!(
            fixture
                .journal
                .package
                .as_ref()
                .expect("package record")
                .target
                .join("VesperFFmpegAVCodec.xcframework/old.txt")
                .is_file()
        );
        assert!(fixture.journal_path.is_file());
    }

    #[test]
    fn managed_release_asset_names_are_exact() {
        assert!(optional_release_asset_name(OsStr::new(
            "VesperPlayerRemuxFfmpegPlugin.xcframework.zip"
        )));
        assert!(optional_release_asset_name(OsStr::new(
            LEGACY_OPTIONAL_RUNTIME_ASSET
        )));
        assert!(!optional_release_asset_name(OsStr::new(
            "VesperPlayerCustomerPluginNotes.xcframework.zip"
        )));
        assert!(!optional_release_asset_name(OsStr::new(
            "VesperFFmpegCustomer.xcframework.zip"
        )));
        assert!(!optional_release_asset_name(OsStr::new(
            "VesperPlayerOptionalPlugins-FFmpeg-release notes"
        )));
        assert!(!optional_release_asset_name(OsStr::new(
            "VesperPlayerOptionalPlugins-FFmpeg-8.1.2 candidate-source.tar.xz"
        )));
    }

    #[test]
    fn journal_cleanup_rejects_a_replaced_regular_file() {
        let fixture = recovery_fixture();
        persist_release_journal(&fixture.journal_path, &fixture.journal, false)
            .expect("persist rollback journal");
        let loaded = read_release_journal(&fixture.journal_path)
            .expect("read rollback journal")
            .expect("rollback journal");
        let displaced = fixture.journal_path.with_extension("displaced");
        fs::rename(&fixture.journal_path, &displaced).expect("displace rollback journal");
        fs::write(&fixture.journal_path, b"replacement\n").expect("write replacement file");

        remove_release_journal(&fixture.journal_path, loaded.identity)
            .expect_err("reject replacement journal file");
        assert_eq!(
            fs::read(&fixture.journal_path).expect("read replacement file"),
            b"replacement\n"
        );
        assert!(displaced.is_file());
    }

    #[test]
    fn journal_parent_sync_failure_is_reported_after_publication() {
        let mut fixture = recovery_fixture();
        persist_release_journal(&fixture.journal_path, &fixture.journal, false)
            .expect("persist rollback journal");
        fixture.journal.decision = JournalDecision::Commit;

        let failure = persist_release_journal_with_sync(
            &fixture.journal_path,
            &fixture.journal,
            true,
            |_| Err(io::Error::other("injected parent sync failure")),
        )
        .expect_err("surface parent sync failure");
        assert!(matches!(
            failure,
            JournalPersistenceFailure::AfterPublish(_)
        ));
        let visible = read_release_journal(&fixture.journal_path)
            .expect("read visible commit journal")
            .expect("visible commit journal");
        assert_eq!(visible.journal.decision, JournalDecision::Commit);
    }

    #[test]
    fn recovery_confirms_commit_journal_durability_before_cleanup() {
        let mut fixture = recovery_fixture();
        persist_release_journal(&fixture.journal_path, &fixture.journal, false)
            .expect("persist rollback journal");
        apply_promotion_record(&fixture.journal.release).expect("promote release fixture");
        let package = fixture.journal.package.as_ref().expect("package record");
        apply_promotion_record(package).expect("promote package fixture");
        fixture.journal.decision = JournalDecision::Commit;
        let failure = persist_release_journal_with_sync(
            &fixture.journal_path,
            &fixture.journal,
            true,
            |_| Err(io::Error::other("injected commit parent sync failure")),
        )
        .expect_err("surface commit parent sync failure");
        assert!(matches!(
            failure,
            JournalPersistenceFailure::AfterPublish(_)
        ));

        let mut journal_sync_calls = 0_usize;
        let mut journal_sync = |_: &Path| {
            journal_sync_calls += 1;
            Err(io::Error::other("injected durability confirmation failure"))
        };
        let mut recovery_sync = |_: &Path| -> io::Result<()> {
            panic!("recovery cleanup must not start before journal durability is confirmed")
        };
        let error = recover_release_journal_with_sync(
            &fixture.root,
            &fixture.journal_path,
            None,
            &mut journal_sync,
            &mut recovery_sync,
        )
        .expect_err("reject cleanup without a durable commit journal");
        assert!(
            error
                .to_string()
                .contains("failed to confirm iOS release journal directory")
        );
        assert_eq!(journal_sync_calls, 1);
        assert!(fixture.journal.release.target.join("new.txt").is_file());
        assert!(fixture.journal.release.owner.exists());
        assert!(package.owner.exists());
        assert!(fixture.journal_path.is_file());

        recover_release_journal(&fixture.root, &fixture.journal_path)
            .expect("finish cleanup after durability confirmation succeeds");
        assert!(!fixture.journal.release.owner.exists());
        assert!(!package.owner.exists());
        assert!(!fixture.journal_path.exists());
    }

    #[test]
    fn recovery_retries_parent_sync_after_cleanup_is_already_visible() {
        let mut fixture = recovery_fixture();
        persist_release_journal(&fixture.journal_path, &fixture.journal, false)
            .expect("persist rollback journal");
        apply_promotion_record(&fixture.journal.release).expect("promote release fixture");
        let package = fixture.journal.package.as_ref().expect("package record");
        apply_promotion_record(package).expect("promote package fixture");
        fixture.journal.decision = JournalDecision::Commit;
        persist_release_journal(&fixture.journal_path, &fixture.journal, true)
            .expect("persist commit decision");

        let mut journal_sync = |path: &Path| sync_directory(path);
        let mut first_cleanup_sync = |_: &Path| {
            Err(io::Error::other(
                "injected release cleanup parent sync failure",
            ))
        };
        recover_release_journal_with_sync(
            &fixture.root,
            &fixture.journal_path,
            None,
            &mut journal_sync,
            &mut first_cleanup_sync,
        )
        .expect_err("surface cleanup parent sync failure");
        assert!(!fixture.journal.release.owner.exists());
        assert!(package.owner.exists());
        assert!(fixture.journal_path.is_file());

        let mut retry_sync_calls = 0_usize;
        let mut retry_cleanup_sync = |_: &Path| {
            retry_sync_calls += 1;
            Err(io::Error::other(
                "injected retry cleanup parent sync failure",
            ))
        };
        recover_release_journal_with_sync(
            &fixture.root,
            &fixture.journal_path,
            None,
            &mut journal_sync,
            &mut retry_cleanup_sync,
        )
        .expect_err("retry the cleanup parent sync");
        assert_eq!(retry_sync_calls, 1);
        assert!(package.owner.exists());
        assert!(fixture.journal_path.is_file());

        recover_release_journal(&fixture.root, &fixture.journal_path)
            .expect("finish cleanup after parent sync succeeds");
        assert!(!package.owner.exists());
        assert!(!fixture.journal_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn durable_commit_cleanup_defers_sigint_until_journal_removal() {
        use nix::sys::signal::{Signal, raise};

        const CHILD_ENV: &str = "VESPER_IOS_RELEASE_COMMIT_SIGINT_FIXTURE";
        if env::var_os(CHILD_ENV).is_some() {
            let mut fixture = recovery_fixture();
            persist_release_journal(&fixture.journal_path, &fixture.journal, false)
                .expect("persist rollback journal");
            apply_promotion_record(&fixture.journal.release).expect("promote release fixture");
            let package = fixture.journal.package.as_ref().expect("package record");
            apply_promotion_record(package).expect("promote package fixture");
            fixture.journal.decision = JournalDecision::Commit;
            persist_release_journal(&fixture.journal_path, &fixture.journal, true)
                .expect("persist commit decision");

            let cancellation =
                external_process::InterruptDeferral::start("iOS durable commit cleanup test")
                    .expect("start commit cleanup cancellation scope");
            raise(Signal::SIGINT).expect("raise commit cleanup cancellation");
            assert!(cancellation.is_cancelled());

            recover_release_journal_with_cancellation(
                &fixture.root,
                &fixture.journal_path,
                Some(&cancellation),
            )
            .expect("finish durable commit cleanup despite cancellation");
            assert!(cancellation.finish());
            assert!(fixture.journal.release.target.join("new.txt").is_file());
            assert!(
                package
                    .target
                    .join("VesperFFmpegAVCodec.xcframework/new.txt")
                    .is_file()
            );
            assert!(!fixture.journal.release.owner.exists());
            assert!(!package.owner.exists());
            assert!(!fixture.journal_path.exists());
            return;
        }

        let status = Command::new(env::current_exe().expect("locate iOS release test binary"))
            .args([
                "--exact",
                "ios_release::tests::durable_commit_cleanup_defers_sigint_until_journal_removal",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .status()
            .expect("run isolated durable commit cancellation fixture");
        assert!(status.success());
    }
}
