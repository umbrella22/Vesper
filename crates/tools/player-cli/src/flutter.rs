use std::collections::VecDeque;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, Permissions};
use std::io::{self, Read, Write};
use std::path::Component;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::external_process::{self, ExternalProcessErrorKind};

const CORE_PACKAGES: [&str; 6] = [
    "vesper_player_platform_interface",
    "vesper_player_android",
    "vesper_player_ios",
    "vesper_player",
    "vesper_player_external_playback",
    "vesper_player_ui",
];
const OPTIONAL_PACKAGES: [&str; 1] = ["vesper_player_source_normalizer_ffmpeg"];
const MAX_LOCAL_OVERRIDES_FILE_BYTES: u64 = 1024 * 1024;
const MAX_FLUTTER_PUBSPEC_BYTES: u64 = 1024 * 1024;
const MAX_FLUTTER_STAGE_ENTRIES: usize = 100_000;
const MAX_FLUTTER_STAGE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const PUB_GET_PROPAGATION_RETRIES: usize = 6;
const PUB_GET_PROPAGATION_WAIT: Duration = Duration::from_secs(20);
const PUB_DEV_API_BASE_URL: &str = "https://pub.dev/api/packages/";
const PUB_DEV_LOOKUP_MAXIMUM_BYTES: usize = 2 * 1024 * 1024;
const PUB_DEV_LOOKUP_TIMEOUT: Duration = Duration::from_secs(45);
const PUB_DEV_NEW_PACKAGE_BURST_LIMIT: usize = 4;
const PUB_DEV_NEW_PACKAGE_BURST_WINDOW: Duration = Duration::from_secs(120);
const PUB_DEV_NEW_PACKAGE_BURST_BUFFER: Duration = Duration::from_secs(5);
const OPTIONAL_IOS_ARTIFACTS: [&str; 7] = [
    "VesperFFmpegAVCodec",
    "VesperFFmpegAVFormat",
    "VesperFFmpegAVUtil",
    "VesperPlayerRemuxFfmpegPlugin",
    "VesperPlayerSourceNormalizerFfmpegPlugin",
    "VesperPlayerDecoderVideoToolboxPlugin",
    "VesperPlayerFrameProcessorDiagnosticPlugin",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlutterErrorKind {
    Storage,
    Compatibility,
    Conformance,
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct FlutterError {
    kind: FlutterErrorKind,
    message: String,
}

impl FlutterError {
    fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: FlutterErrorKind::Storage,
            message: message.into(),
        }
    }

    fn compatibility(message: impl Into<String>) -> Self {
        Self {
            kind: FlutterErrorKind::Compatibility,
            message: message.into(),
        }
    }

    fn conformance(message: impl Into<String>) -> Self {
        Self {
            kind: FlutterErrorKind::Conformance,
            message: message.into(),
        }
    }

    fn worker(message: impl Into<String>) -> Self {
        Self {
            kind: FlutterErrorKind::Worker,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> FlutterErrorKind {
        self.kind
    }
}

pub fn verify_android_plugin(root: &Path, include_optional: bool) -> Result<(), FlutterError> {
    let project_directory = require_contained_directory(
        root,
        &root.join("examples/flutter-host/android"),
        "Flutter Android project",
    )?;
    let gradle =
        crate::gradle::resolve(&project_directory, None).map_err(|error| match error.kind() {
            crate::gradle::GradleErrorKind::Storage => FlutterError::storage(error.to_string()),
            crate::gradle::GradleErrorKind::Compatibility => {
                FlutterError::compatibility(error.to_string())
            }
        })?;
    let gradle_user_home = crate::gradle::service_home(&project_directory);
    let mut command = Command::new(gradle);
    command
        .current_dir(root)
        .env("GRADLE_USER_HOME", gradle_user_home)
        .arg("-p")
        .arg(&project_directory)
        .arg(":vesper_player_android:compileDebugKotlin")
        .arg(":vesper_player_external_playback:compileDebugKotlin");
    if include_optional {
        command.arg(":vesper_player_source_normalizer_ffmpeg:compileDebugKotlin");
    }
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let label = "Flutter Android plugin Gradle verification";
    let status = external_process::run_interruptible(&mut command, label)
        .map_err(|error| FlutterError::worker(error.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(FlutterError::conformance(format!(
            "{label} exited unsuccessfully ({status})"
        )))
    }
}

fn resolve_path_executable(command: &str) -> Result<Option<PathBuf>, FlutterError> {
    let Some(paths) = env::var_os("PATH") else {
        return Ok(None);
    };
    for directory in env::split_paths(&paths) {
        for candidate in executable_candidates(&directory, command) {
            let Ok(metadata) = fs::metadata(&candidate) else {
                continue;
            };
            if metadata.is_file() && current_process_can_execute(&candidate) {
                return candidate.canonicalize().map(Some).map_err(|error| {
                    FlutterError::storage(format!(
                        "failed to resolve PATH executable '{}': {error}",
                        candidate.display()
                    ))
                });
            }
        }
    }
    Ok(None)
}

fn executable_candidates(directory: &Path, command: &str) -> Vec<PathBuf> {
    #[cfg(not(windows))]
    let candidates = vec![directory.join(command)];
    #[cfg(windows)]
    let candidates = {
        [".exe", ".cmd", ".bat"]
            .into_iter()
            .map(|extension| directory.join(format!("{command}{extension}")))
            .collect()
    };
    candidates
}

#[cfg(unix)]
fn current_process_can_execute(path: &Path) -> bool {
    use nix::unistd::{AccessFlags, access};

    access(path, AccessFlags::X_OK).is_ok()
}

#[cfg(not(unix))]
fn current_process_can_execute(_path: &Path) -> bool {
    true
}

fn require_contained_directory(
    containment_root: &Path,
    path: &Path,
    label: &str,
) -> Result<PathBuf, FlutterError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        FlutterError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(FlutterError::storage(format!(
            "{label} '{}' is not a regular non-symlink directory",
            path.display()
        )));
    }
    let canonical = path.canonicalize().map_err(|error| {
        FlutterError::storage(format!(
            "failed to resolve {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !canonical.starts_with(containment_root) {
        return Err(FlutterError::compatibility(format!(
            "{label} '{}' resolves outside '{}'",
            path.display(),
            containment_root.display()
        )));
    }
    Ok(canonical)
}

pub fn include_optional_plugins(cli_value: Option<bool>) -> bool {
    cli_value.unwrap_or_else(|| {
        env::var_os("VESPER_FLUTTER_INCLUDE_OPTIONAL_PLUGINS").is_some_and(|value| {
            matches!(value.to_str(), Some("1" | "true" | "TRUE" | "yes" | "YES"))
        })
    })
}

pub fn write_local_overrides(
    root: &Path,
    include_optional: bool,
    output: &mut dyn Write,
) -> Result<(), FlutterError> {
    let canonical_root = root.canonicalize().map_err(|error| {
        FlutterError::storage(format!(
            "failed to resolve repository root '{}': {error}",
            root.display()
        ))
    })?;
    let packages = selected_packages(include_optional);
    let mut planned = Vec::with_capacity(packages.len() + 1);

    for package in &packages {
        let package_directory = root.join("lib/flutter").join(package);
        require_contained_regular_file(
            &canonical_root,
            &package_directory.join("pubspec.yaml"),
            "Flutter package pubspec",
        )?;
        let output_path = package_directory.join("pubspec_overrides.yaml");
        let contents = format_package_overrides(package, &packages);
        planned.push(PlannedOverride::preflight(
            &canonical_root,
            output_path,
            contents,
        )?);
    }

    let example_directory = root.join("examples/flutter-host");
    require_contained_regular_file(
        &canonical_root,
        &example_directory.join("pubspec.yaml"),
        "Flutter example pubspec",
    )?;
    let example_output = example_directory.join("pubspec_overrides.yaml");
    planned.push(PlannedOverride::preflight(
        &canonical_root,
        example_output,
        format_example_overrides(&packages),
    )?);

    OverrideTransaction { planned }.commit(None)?;

    writeln!(output, "Wrote local Flutter pubspec_overrides.yaml files.").map_err(output_error)?;
    for package in packages {
        writeln!(output, "  lib/flutter/{package}/pubspec_overrides.yaml").map_err(output_error)?;
    }
    writeln!(output, "  examples/flutter-host/pubspec_overrides.yaml").map_err(output_error)?;
    output.flush().map_err(output_error)
}

pub fn stage_pub_packages(
    root: &Path,
    output_directory: Option<&Path>,
    requested_version: Option<&str>,
    include_optional: bool,
    output: &mut dyn Write,
) -> Result<(), FlutterError> {
    let canonical_root = root.canonicalize().map_err(|error| {
        FlutterError::storage(format!(
            "failed to resolve repository root '{}': {error}",
            root.display()
        ))
    })?;
    let packages = selected_packages(include_optional);
    let version = resolve_flutter_package_version(&canonical_root, requested_version)?;
    let license = canonical_root.join("LICENSE");
    let license_snapshot =
        require_contained_regular_file(&canonical_root, &license, "workspace license")?;

    let mut budget = FlutterStageBudget::default();
    let mut package_plans = Vec::with_capacity(packages.len());
    for package in &packages {
        let source = require_contained_directory(
            &canonical_root,
            &canonical_root.join("lib/flutter").join(package),
            "Flutter package directory",
        )?;
        require_contained_regular_file(
            &canonical_root,
            &source.join("pubspec.yaml"),
            "Flutter package pubspec",
        )?;
        package_plans.push(FlutterPackageStagePlan {
            package,
            tree: collect_flutter_stage_tree(
                &source,
                StageCopyProfile::FlutterPackage,
                &mut budget,
            )?,
        });
    }

    let optional_ios = if include_optional {
        Some(preflight_optional_ios_package(
            &canonical_root,
            &mut budget,
        )?)
    } else {
        None
    };
    let mut protected_source_roots = package_plans
        .iter()
        .map(|plan| plan.tree.source_root.clone())
        .collect::<Vec<_>>();
    if let Some(optional_ios) = optional_ios.as_ref() {
        protected_source_roots.push(optional_ios.source_root.clone());
    }
    let default_output = canonical_root.join("dist/release/flutter-pub");
    let displayed_output = output_directory.unwrap_or(&default_output);
    let target = resolve_flutter_stage_target(
        &canonical_root,
        displayed_output,
        output_directory.is_none(),
        &protected_source_roots,
    )?;
    target.revalidate_parent()?;
    let staging = tempfile::Builder::new()
        .prefix(".vesper-flutter-pub-stage-")
        .tempdir_in(&target.canonical_parent)
        .map_err(|error| {
            FlutterError::storage(format!(
                "failed to create Flutter pub staging directory beside '{}': {error}",
                target.path.display()
            ))
        })?;

    let mut copy_budget = FlutterStageCopyBudget::default();
    for plan in &package_plans {
        let destination = staging.path().join(plan.package);
        copy_flutter_stage_tree(&plan.tree, &destination, &mut copy_budget)?;
        remove_staged_regular_file(&destination.join("LICENSE"), "package license")?;
        copy_regular_file(
            &canonical_root,
            &license,
            &destination.join("LICENSE"),
            "workspace license",
            &license_snapshot.metadata,
            license_snapshot.identity,
            &mut copy_budget,
        )?;
        if plan.package == OPTIONAL_PACKAGES[0]
            && let Some(optional_ios) = optional_ios.as_ref()
        {
            copy_flutter_stage_tree(
                optional_ios,
                &destination.join("ios/VesperPlayerOptionalPlugins"),
                &mut copy_budget,
            )?;
        }
        let pubspec = destination.join("pubspec.yaml");
        let source = read_bounded_utf8_file(
            &pubspec,
            MAX_FLUTTER_PUBSPEC_BYTES,
            "staged Flutter pubspec",
        )?;
        fs::write(&pubspec, rewrite_pubspec(&source, &version, &packages)).map_err(|error| {
            FlutterError::storage(format!(
                "failed to rewrite staged Flutter pubspec '{}': {error}",
                pubspec.display()
            ))
        })?;
    }

    promote_flutter_stage(staging, &target)?;
    writeln!(output, "Staged Flutter pub packages into:").map_err(output_error)?;
    writeln!(output, "  {}", displayed_output.display()).map_err(output_error)?;
    for package in &packages {
        writeln!(output, "  {package}").map_err(output_error)?;
    }
    if packages.len() == CORE_PACKAGES.len() {
        writeln!(
            output,
            "Skipped optional Flutter plugin packages. Set VESPER_FLUTTER_INCLUDE_OPTIONAL_PLUGINS=1 to stage them."
        )
        .map_err(output_error)?;
    }
    output.flush().map_err(output_error)
}

pub fn dry_run_pub_packages(
    root: &Path,
    output_directory: Option<&Path>,
    requested_version: Option<&str>,
    include_optional: bool,
    output: &mut dyn Write,
) -> Result<(), FlutterError> {
    stage_pub_packages(
        root,
        output_directory,
        requested_version,
        include_optional,
        output,
    )?;
    let stage = resolved_stage_after_staging(root, output_directory)?;
    let packages = selected_packages(include_optional);
    let flutter = require_path_executable("flutter", "Flutter SDK executable")?;
    for package in &packages {
        write_staged_package_overrides(&stage, package, &packages)?;
        writeln!(output, "::group::flutter pub publish --dry-run {package}")
            .map_err(output_error)?;
        output.flush().map_err(output_error)?;
        run_flutter_command(
            &flutter,
            &stage.join(package),
            &["pub", "get"],
            &format!("flutter pub get for {package}"),
        )?;
        run_flutter_command(
            &flutter,
            &stage.join(package),
            &["pub", "publish", "--dry-run"],
            &format!("flutter pub publish --dry-run for {package}"),
        )?;
        writeln!(output, "::endgroup::").map_err(output_error)?;
    }
    output.flush().map_err(output_error)
}

pub fn publish_pub_packages(
    root: &Path,
    output_directory: Option<&Path>,
    requested_version: Option<&str>,
    include_optional: bool,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), FlutterError> {
    let canonical_root = root.canonicalize().map_err(|error| {
        FlutterError::storage(format!(
            "failed to resolve repository root '{}': {error}",
            root.display()
        ))
    })?;
    let version = resolve_flutter_package_version(&canonical_root, requested_version)?;
    stage_pub_packages(
        root,
        output_directory,
        requested_version,
        include_optional,
        output,
    )?;
    let stage = resolved_stage_after_staging(root, output_directory)?;
    let packages = selected_packages(include_optional);
    let flutter = require_path_executable("flutter", "Flutter SDK executable")?;
    let mut recent_new_packages = VecDeque::new();
    for package in &packages {
        writeln!(output, "::group::flutter pub publish {package}").map_err(output_error)?;
        output.flush().map_err(output_error)?;
        if pub_dev_resource_exists(package, Some(&version))? {
            writeln!(
                output,
                "Skipping {package} {version}: this exact version is already published on pub.dev."
            )
            .map_err(output_error)?;
            writeln!(output, "::endgroup::").map_err(output_error)?;
            continue;
        }
        let creates_package = !pub_dev_resource_exists(package, None)?;
        if creates_package {
            wait_for_pub_dev_new_package_slot(&mut recent_new_packages, diagnostics)?;
        }
        run_pub_get_with_retry(&flutter, &stage.join(package), package, diagnostics)?;
        run_flutter_command(
            &flutter,
            &stage.join(package),
            &["pub", "publish", "--force"],
            &format!("flutter pub publish for {package}"),
        )?;
        if creates_package {
            recent_new_packages.push_back(Instant::now());
        }
        writeln!(output, "::endgroup::").map_err(output_error)?;
    }
    output.flush().map_err(output_error)?;
    diagnostics.flush().map_err(|error| {
        FlutterError::storage(format!(
            "failed to write Flutter command diagnostics: {error}"
        ))
    })
}

fn resolved_stage_after_staging(
    root: &Path,
    output_directory: Option<&Path>,
) -> Result<PathBuf, FlutterError> {
    let canonical_root = root.canonicalize().map_err(|error| {
        FlutterError::storage(format!(
            "failed to resolve repository root '{}': {error}",
            root.display()
        ))
    })?;
    let default_output = canonical_root.join("dist/release/flutter-pub");
    let target = resolve_flutter_stage_target(
        &canonical_root,
        output_directory.unwrap_or(&default_output),
        output_directory.is_none(),
        &[],
    )?;
    target.revalidate_parent()?;
    if !target.revalidate_initial_target()? {
        return Err(FlutterError::storage(format!(
            "staged Flutter pub directory '{}' disappeared after staging",
            target.path.display()
        )));
    }
    Ok(target.path)
}

fn write_staged_package_overrides(
    stage: &Path,
    package: &str,
    packages: &[&str],
) -> Result<(), FlutterError> {
    let package_directory = require_contained_directory(
        stage,
        &stage.join(package),
        "staged Flutter package directory",
    )?;
    let planned = PlannedOverride::preflight(
        stage,
        package_directory.join("pubspec_overrides.yaml"),
        format_package_overrides(package, packages),
    )?;
    OverrideTransaction {
        planned: vec![planned],
    }
    .commit(None)
}

fn require_path_executable(command: &str, label: &str) -> Result<PathBuf, FlutterError> {
    resolve_path_executable(command)?.ok_or_else(|| {
        FlutterError::compatibility(format!("{label} '{command}' was not found in PATH"))
    })
}

fn pub_dev_resource_exists(package: &str, version: Option<&str>) -> Result<bool, FlutterError> {
    let mut url = url::Url::parse(PUB_DEV_API_BASE_URL).map_err(|error| {
        FlutterError::compatibility(format!("invalid pub.dev API base URL: {error}"))
    })?;
    {
        let mut segments = url.path_segments_mut().map_err(|_| {
            FlutterError::compatibility("pub.dev API base URL cannot contain path segments")
        })?;
        segments.push(package);
        if let Some(version) = version {
            segments.push("versions");
            segments.push(version);
        }
    }

    let curl = env::var_os("CURL").unwrap_or_else(|| OsString::from("curl"));
    let label = match version {
        Some(version) => format!("pub.dev lookup for {package} {version}"),
        None => format!("pub.dev lookup for {package}"),
    };
    let mut command = Command::new(curl);
    command.args([
        "--silent",
        "--show-error",
        "--location",
        "--connect-timeout",
        "15",
        "--max-time",
        "30",
        "--retry",
        "3",
        "--retry-all-errors",
        "--write-out",
        "\n%{http_code}",
    ]);
    command.arg(url.as_str());
    let result = external_process::run_interruptible_capture_with_timeout(
        &mut command,
        &label,
        PUB_DEV_LOOKUP_MAXIMUM_BYTES,
        PUB_DEV_LOOKUP_MAXIMUM_BYTES,
        PUB_DEV_LOOKUP_TIMEOUT,
    )
    .map_err(|error| match error.kind() {
        ExternalProcessErrorKind::Compatibility => FlutterError::compatibility(error.to_string()),
        ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
            FlutterError::worker(error.to_string())
        }
    })?;
    if !result.status.success() {
        return Err(FlutterError::worker(format!(
            "{label} failed with {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }
    let response = String::from_utf8(result.stdout).map_err(|error| {
        FlutterError::worker(format!("{label} returned non-UTF-8 output: {error}"))
    })?;
    let (_, status) = response
        .rsplit_once('\n')
        .ok_or_else(|| FlutterError::worker(format!("{label} did not report an HTTP status")))?;
    match status.trim() {
        "200" => Ok(true),
        "404" => Ok(false),
        status => Err(FlutterError::worker(format!(
            "{label} returned unexpected HTTP status {status}"
        ))),
    }
}

fn wait_for_pub_dev_new_package_slot(
    recent: &mut VecDeque<Instant>,
    diagnostics: &mut dyn Write,
) -> Result<(), FlutterError> {
    let now = Instant::now();
    let Some(wait) = pub_dev_new_package_wait(recent, now) else {
        return Ok(());
    };
    writeln!(
        diagnostics,
        "pub.dev limits initial package creation to {PUB_DEV_NEW_PACKAGE_BURST_LIMIT} packages per two-minute window; waiting {} seconds before continuing.",
        wait.as_secs()
    )
    .map_err(|error| {
        FlutterError::storage(format!("failed to write Flutter command diagnostics: {error}"))
    })?;
    diagnostics.flush().map_err(|error| {
        FlutterError::storage(format!(
            "failed to write Flutter command diagnostics: {error}"
        ))
    })?;
    std::thread::sleep(wait);
    let _ = pub_dev_new_package_wait(recent, Instant::now());
    Ok(())
}

fn pub_dev_new_package_wait(recent: &mut VecDeque<Instant>, now: Instant) -> Option<Duration> {
    while recent
        .front()
        .is_some_and(|created| now.duration_since(*created) >= PUB_DEV_NEW_PACKAGE_BURST_WINDOW)
    {
        recent.pop_front();
    }
    let oldest = recent
        .front()
        .copied()
        .filter(|_| recent.len() >= PUB_DEV_NEW_PACKAGE_BURST_LIMIT)?;
    let elapsed = now.duration_since(oldest);
    Some(
        PUB_DEV_NEW_PACKAGE_BURST_WINDOW
            .saturating_sub(elapsed)
            .saturating_add(PUB_DEV_NEW_PACKAGE_BURST_BUFFER),
    )
}

fn run_flutter_command(
    flutter: &Path,
    working_directory: &Path,
    arguments: &[&str],
    label: &str,
) -> Result<(), FlutterError> {
    let mut command = Command::new(flutter);
    command
        .args(arguments)
        .current_dir(working_directory)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = external_process::run_interruptible(&mut command, label)
        .map_err(|error| FlutterError::worker(error.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(FlutterError::conformance(format!(
            "{label} exited unsuccessfully ({status})"
        )))
    }
}

fn run_pub_get_with_retry(
    flutter: &Path,
    working_directory: &Path,
    package: &str,
    diagnostics: &mut dyn Write,
) -> Result<(), FlutterError> {
    for attempt in 1..=PUB_GET_PROPAGATION_RETRIES + 1 {
        let label = format!("flutter pub get for {package}");
        let mut command = Command::new(flutter);
        command
            .args(["pub", "get"])
            .current_dir(working_directory)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let status = external_process::run_interruptible(&mut command, &label)
            .map_err(|error| FlutterError::worker(error.to_string()))?;
        if status.success() {
            return Ok(());
        }
        if attempt > PUB_GET_PROPAGATION_RETRIES {
            return Err(FlutterError::conformance(format!(
                "{label} exited unsuccessfully ({status})"
            )));
        }
        writeln!(
            diagnostics,
            "flutter pub get failed on attempt {attempt}; waiting for pub.dev package index propagation."
        )
        .map_err(|error| {
            FlutterError::storage(format!("failed to write Flutter command diagnostics: {error}"))
        })?;
        diagnostics.flush().map_err(|error| {
            FlutterError::storage(format!(
                "failed to write Flutter command diagnostics: {error}"
            ))
        })?;
        std::thread::sleep(PUB_GET_PROPAGATION_WAIT);
    }
    Err(FlutterError::worker(
        "Flutter pub get retry loop ended unexpectedly",
    ))
}

struct FlutterPackageStagePlan {
    package: &'static str,
    tree: FlutterStageTree,
}

struct FlutterStageTree {
    source_root: PathBuf,
    entries: Vec<FlutterStageEntry>,
}

struct FlutterStageEntry {
    relative: PathBuf,
    metadata: fs::Metadata,
    identity: FileIdentity,
    kind: FlutterStageEntryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    volume_or_device: u64,
    file_index: u64,
}

struct FileSnapshot {
    metadata: fs::Metadata,
    identity: FileIdentity,
}

#[derive(Clone, Copy)]
enum FlutterStageEntryKind {
    Directory,
    File,
}

#[derive(Clone, Copy)]
enum StageCopyProfile {
    FlutterPackage,
    OptionalIosPackage,
}

#[derive(Default)]
struct FlutterStageBudget {
    entries: usize,
    bytes: u64,
}

impl FlutterStageBudget {
    fn record_entry(&mut self, path: &Path) -> Result<(), FlutterError> {
        self.entries = self.entries.saturating_add(1);
        if self.entries > MAX_FLUTTER_STAGE_ENTRIES {
            return Err(FlutterError::compatibility(format!(
                "Flutter pub staging exceeds {MAX_FLUTTER_STAGE_ENTRIES} source entries at '{}'",
                path.display()
            )));
        }
        Ok(())
    }

    fn record_source_bytes(&mut self, path: &Path, file_bytes: u64) -> Result<(), FlutterError> {
        self.bytes = self.bytes.saturating_add(file_bytes);
        if self.bytes > MAX_FLUTTER_STAGE_BYTES {
            return Err(FlutterError::compatibility(format!(
                "Flutter pub staging exceeds {MAX_FLUTTER_STAGE_BYTES} source bytes at '{}'",
                path.display()
            )));
        }
        Ok(())
    }
}

#[derive(Default)]
struct FlutterStageCopyBudget {
    bytes: u64,
}

impl FlutterStageCopyBudget {
    fn reserve(&mut self, path: &Path, file_bytes: u64) -> Result<(), FlutterError> {
        let updated = self.bytes.checked_add(file_bytes).ok_or_else(|| {
            FlutterError::compatibility(format!(
                "Flutter pub staging output byte count overflowed at '{}'",
                path.display()
            ))
        })?;
        if updated > MAX_FLUTTER_STAGE_BYTES {
            return Err(FlutterError::compatibility(format!(
                "Flutter pub staging exceeds {MAX_FLUTTER_STAGE_BYTES} copied bytes at '{}'",
                path.display()
            )));
        }
        self.bytes = updated;
        Ok(())
    }
}

fn collect_flutter_stage_tree(
    source_root: &Path,
    profile: StageCopyProfile,
    budget: &mut FlutterStageBudget,
) -> Result<FlutterStageTree, FlutterError> {
    let mut directories = vec![(source_root.to_path_buf(), PathBuf::new())];
    let mut entries = Vec::new();
    while let Some((directory, relative_directory)) = directories.pop() {
        let metadata = fs::symlink_metadata(&directory).map_err(|error| {
            FlutterError::storage(format!(
                "failed to inspect Flutter staging directory '{}': {error}",
                directory.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(FlutterError::compatibility(format!(
                "Flutter staging directory '{}' is not a regular non-symlink directory",
                directory.display()
            )));
        }
        let canonical_directory = directory.canonicalize().map_err(|error| {
            FlutterError::storage(format!(
                "failed to resolve Flutter staging directory '{}': {error}",
                directory.display()
            ))
        })?;
        if !canonical_directory.starts_with(source_root) {
            return Err(FlutterError::compatibility(format!(
                "Flutter staging directory '{}' resolves outside source root '{}'",
                directory.display(),
                source_root.display()
            )));
        }
        let children = fs::read_dir(&directory).map_err(|error| {
            FlutterError::storage(format!(
                "failed to read Flutter staging directory '{}': {error}",
                directory.display()
            ))
        })?;
        let mut bounded_children = Vec::new();
        for child in children {
            let child = child.map_err(|error| {
                FlutterError::storage(format!(
                    "failed to read an entry under Flutter staging directory '{}': {error}",
                    directory.display()
                ))
            })?;
            budget.record_entry(&child.path())?;
            bounded_children.push(child);
        }
        let mut children = bounded_children;
        children.sort_by_key(std::fs::DirEntry::file_name);
        for child in children {
            let relative = relative_directory.join(child.file_name());
            let file_type = child.file_type().map_err(|error| {
                FlutterError::storage(format!(
                    "failed to inspect Flutter staging source '{}': {error}",
                    child.path().display()
                ))
            })?;
            if flutter_stage_path_is_excluded(&relative, profile) {
                continue;
            }
            if flutter_stage_path_is_generated_artifact(&relative, file_type.is_dir()) {
                return Err(FlutterError::conformance(format!(
                    "Refusing to stage Flutter pub package with generated local artifact: {}",
                    child.path().display()
                )));
            }
            if file_type.is_symlink() {
                return Err(FlutterError::compatibility(format!(
                    "Flutter pub staging source '{}' is a symlink",
                    child.path().display()
                )));
            }
            let metadata = child.metadata().map_err(|error| {
                FlutterError::storage(format!(
                    "failed to inspect Flutter staging source '{}': {error}",
                    child.path().display()
                ))
            })?;
            let identity = path_file_identity(&child.path(), &metadata).map_err(|error| {
                FlutterError::storage(format!(
                    "failed to identify Flutter staging source '{}': {error}",
                    child.path().display()
                ))
            })?;
            let kind = if file_type.is_dir() {
                FlutterStageEntryKind::Directory
            } else if file_type.is_file() {
                FlutterStageEntryKind::File
            } else {
                return Err(FlutterError::compatibility(format!(
                    "Flutter pub staging source '{}' is not a regular file or directory",
                    child.path().display()
                )));
            };
            if file_type.is_file() {
                budget.record_source_bytes(&child.path(), metadata.len())?;
            }
            entries.push(FlutterStageEntry {
                relative: relative.clone(),
                metadata,
                identity,
                kind,
            });
            if file_type.is_dir() {
                directories.push((child.path(), relative));
            }
        }
    }
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(FlutterStageTree {
        source_root: source_root.to_path_buf(),
        entries,
    })
}

fn flutter_stage_path_is_excluded(relative: &Path, profile: StageCopyProfile) -> bool {
    let file_name = relative.file_name().unwrap_or_else(|| OsStr::new(""));
    match profile {
        StageCopyProfile::FlutterPackage => {
            matches_os_str(
                file_name,
                &[
                    ".dart_tool",
                    ".gradle",
                    ".idea",
                    ".kotlin",
                    ".swiftpm",
                    ".build",
                    "build",
                    "Pods",
                    ".symlinks",
                    "pubspec.lock",
                    "pubspec_overrides.yaml",
                    "local.properties",
                ],
            ) || has_adjacent_components(relative, "Flutter", "ephemeral")
                || matches!(
                    relative.extension().and_then(OsStr::to_str),
                    Some("iml" | "xcworkspace" | "xcuserdata" | "xcuserstate")
                )
        }
        StageCopyProfile::OptionalIosPackage => matches_os_str(file_name, &[".build", ".swiftpm"]),
    }
}

fn flutter_stage_path_is_generated_artifact(relative: &Path, is_directory: bool) -> bool {
    let file_name = relative.file_name().unwrap_or_else(|| OsStr::new(""));
    (is_directory
        && matches_os_str(
            file_name,
            &[
                ".dart_tool",
                ".gradle",
                ".idea",
                ".kotlin",
                ".swiftpm",
                ".build",
                "build",
                "Pods",
                ".symlinks",
                "xcode-derived",
                "ModuleCache.noindex",
                "Intermediates.noindex",
            ],
        ))
        || (!is_directory
            && (matches_os_str(
                file_name,
                &["pubspec.lock", "pubspec_overrides.yaml", "local.properties"],
            ) || matches!(
                relative.extension().and_then(OsStr::to_str),
                Some("iml" | "xcuserstate")
            )))
}

fn matches_os_str(value: &OsStr, candidates: &[&str]) -> bool {
    candidates.iter().any(|candidate| value == *candidate)
}

fn has_adjacent_components(path: &Path, first: &str, second: &str) -> bool {
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    components
        .windows(2)
        .any(|pair| pair[0] == first && pair[1] == second)
}

fn copy_flutter_stage_tree(
    tree: &FlutterStageTree,
    destination_root: &Path,
    budget: &mut FlutterStageCopyBudget,
) -> Result<(), FlutterError> {
    fs::create_dir_all(destination_root).map_err(|error| {
        FlutterError::storage(format!(
            "failed to create Flutter staging directory '{}': {error}",
            destination_root.display()
        ))
    })?;
    for entry in &tree.entries {
        if matches!(entry.kind, FlutterStageEntryKind::Directory) {
            fs::create_dir_all(destination_root.join(&entry.relative)).map_err(|error| {
                FlutterError::storage(format!(
                    "failed to create staged Flutter directory '{}': {error}",
                    destination_root.join(&entry.relative).display()
                ))
            })?;
        }
    }
    for entry in &tree.entries {
        if matches!(entry.kind, FlutterStageEntryKind::File) {
            let source = tree.source_root.join(&entry.relative);
            let destination = destination_root.join(&entry.relative);
            copy_regular_file(
                &tree.source_root,
                &source,
                &destination,
                "Flutter package source",
                &entry.metadata,
                entry.identity,
                budget,
            )?;
            fs::set_permissions(&destination, entry.metadata.permissions()).map_err(|error| {
                FlutterError::storage(format!(
                    "failed to preserve staged Flutter file permissions '{}': {error}",
                    destination.display()
                ))
            })?;
        }
    }
    let mut directories = tree
        .entries
        .iter()
        .filter(|entry| matches!(entry.kind, FlutterStageEntryKind::Directory))
        .collect::<Vec<_>>();
    directories.sort_by_key(|entry| std::cmp::Reverse(entry.relative.components().count()));
    for entry in directories {
        let destination = destination_root.join(&entry.relative);
        fs::set_permissions(&destination, entry.metadata.permissions()).map_err(|error| {
            FlutterError::storage(format!(
                "failed to preserve staged Flutter directory permissions '{}': {error}",
                destination.display()
            ))
        })?;
    }
    Ok(())
}

fn copy_regular_file(
    containment_root: &Path,
    source: &Path,
    destination: &Path,
    label: &str,
    expected_metadata: &fs::Metadata,
    expected_identity: FileIdentity,
    budget: &mut FlutterStageCopyBudget,
) -> Result<(), FlutterError> {
    let mut source_file = File::open(source).map_err(|error| {
        FlutterError::storage(format!(
            "failed to open {label} '{}': {error}",
            source.display()
        ))
    })?;
    let opened_metadata = source_file.metadata().map_err(|error| {
        FlutterError::storage(format!(
            "failed to inspect opened {label} '{}': {error}",
            source.display()
        ))
    })?;
    let path_metadata = fs::symlink_metadata(source).map_err(|error| {
        FlutterError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            source.display()
        ))
    })?;
    if !expected_metadata.file_type().is_file()
        || !opened_metadata.is_file()
        || !path_metadata.file_type().is_file()
    {
        return Err(FlutterError::compatibility(format!(
            "{label} '{}' is not a regular non-symlink file",
            source.display()
        )));
    }
    let opened_identity =
        opened_file_identity(&source_file, &opened_metadata).map_err(|error| {
            FlutterError::storage(format!(
                "failed to identify opened {label} '{}': {error}",
                source.display()
            ))
        })?;
    let path_identity = path_file_identity(source, &path_metadata).map_err(|error| {
        FlutterError::storage(format!(
            "failed to identify {label} '{}': {error}",
            source.display()
        ))
    })?;
    if opened_metadata.len() != expected_metadata.len()
        || opened_identity != expected_identity
        || opened_identity != path_identity
    {
        return Err(FlutterError::compatibility(format!(
            "{label} '{}' changed after staging preflight",
            source.display()
        )));
    }
    let canonical_source = source.canonicalize().map_err(|error| {
        FlutterError::storage(format!(
            "failed to resolve {label} '{}': {error}",
            source.display()
        ))
    })?;
    if !canonical_source.starts_with(containment_root) {
        return Err(FlutterError::compatibility(format!(
            "{label} '{}' resolves outside source root '{}'",
            source.display(),
            containment_root.display()
        )));
    }
    let rechecked_metadata = fs::symlink_metadata(source).map_err(|error| {
        FlutterError::storage(format!(
            "failed to recheck {label} '{}': {error}",
            source.display()
        ))
    })?;
    let rechecked_identity = path_file_identity(source, &rechecked_metadata).map_err(|error| {
        FlutterError::storage(format!(
            "failed to identify rechecked {label} '{}': {error}",
            source.display()
        ))
    })?;
    if !rechecked_metadata.file_type().is_file() || opened_identity != rechecked_identity {
        return Err(FlutterError::compatibility(format!(
            "{label} '{}' changed while it was being opened",
            source.display()
        )));
    }
    budget.reserve(source, expected_metadata.len())?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            FlutterError::storage(format!(
                "failed to create staged Flutter file directory '{}': {error}",
                parent.display()
            ))
        })?;
    }
    let mut destination_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| {
            FlutterError::storage(format!(
                "failed to create staged {label} '{}': {error}",
                destination.display()
            ))
        })?;
    let copied = {
        let mut bounded_source = Read::by_ref(&mut source_file).take(expected_metadata.len());
        io::copy(&mut bounded_source, &mut destination_file)
    }
    .map_err(|error| {
        FlutterError::storage(format!(
            "failed to copy {label} '{}' to '{}': {error}",
            source.display(),
            destination.display()
        ))
    })?;
    let mut extra = [0_u8; 1];
    let has_extra = source_file.read(&mut extra).map_err(|error| {
        FlutterError::storage(format!(
            "failed to finish checking {label} '{}': {error}",
            source.display()
        ))
    })? != 0;
    if copied != expected_metadata.len() || has_extra {
        return Err(FlutterError::compatibility(format!(
            "{label} '{}' changed while it was being copied",
            source.display()
        )));
    }
    Ok(())
}

fn remove_staged_regular_file(path: &Path, label: &str) -> Result<(), FlutterError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => fs::remove_file(path).map_err(|error| {
            FlutterError::storage(format!(
                "failed to replace staged {label} '{}': {error}",
                path.display()
            ))
        }),
        Ok(_) => Err(FlutterError::compatibility(format!(
            "staged {label} '{}' is not a regular non-symlink file",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(FlutterError::storage(format!(
            "failed to inspect staged {label} '{}': {error}",
            path.display()
        ))),
    }
}

fn preflight_optional_ios_package(
    root: &Path,
    budget: &mut FlutterStageBudget,
) -> Result<FlutterStageTree, FlutterError> {
    let source = require_contained_directory(
        root,
        &root.join("lib/ios/VesperPlayerOptionalPlugins"),
        "canonical iOS optional plugin package",
    )?;
    require_contained_regular_file(
        root,
        &source.join("Package.swift"),
        "canonical iOS optional plugin package manifest",
    )?;
    for artifact in OPTIONAL_IOS_ARTIFACTS {
        require_contained_directory(
            root,
            &source
                .join("Artifacts")
                .join(format!("{artifact}.xcframework")),
            "optional iOS artifact for Flutter pub staging",
        )?;
    }
    collect_flutter_stage_tree(&source, StageCopyProfile::OptionalIosPackage, budget)
}

fn resolve_flutter_package_version(
    root: &Path,
    requested: Option<&str>,
) -> Result<String, FlutterError> {
    let version = match requested {
        Some(version) => version.to_owned(),
        None => {
            let pubspec = root.join("lib/flutter/vesper_player/pubspec.yaml");
            require_contained_regular_file(root, &pubspec, "Vesper Flutter package pubspec")?;
            read_bounded_utf8_file(
                &pubspec,
                MAX_FLUTTER_PUBSPEC_BYTES,
                "Vesper Flutter package pubspec",
            )?
            .lines()
            .find_map(|line| line.strip_prefix("version: "))
            .unwrap_or("")
            .to_owned()
        }
    };
    if is_valid_flutter_package_version(&version) {
        Ok(version)
    } else {
        Err(FlutterError::compatibility(format!(
            "Unable to resolve a valid Flutter package version: {version}"
        )))
    }
}

fn read_bounded_utf8_file(path: &Path, maximum: u64, label: &str) -> Result<String, FlutterError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        FlutterError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(FlutterError::compatibility(format!(
            "{label} '{}' is not a regular non-symlink file",
            path.display()
        )));
    }
    if metadata.len() > maximum {
        return Err(FlutterError::compatibility(format!(
            "{label} '{}' exceeds {maximum} bytes",
            path.display()
        )));
    }
    let mut file = File::open(path).map_err(|error| {
        FlutterError::storage(format!(
            "failed to open {label} '{}': {error}",
            path.display()
        ))
    })?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            FlutterError::storage(format!(
                "failed to read {label} '{}': {error}",
                path.display()
            ))
        })?;
    if bytes.len() as u64 > maximum {
        return Err(FlutterError::compatibility(format!(
            "{label} '{}' exceeds {maximum} bytes",
            path.display()
        )));
    }
    String::from_utf8(bytes).map_err(|error| {
        FlutterError::compatibility(format!(
            "{label} '{}' is not UTF-8: {error}",
            path.display()
        ))
    })
}

struct FlutterStageTarget {
    path: PathBuf,
    canonical_parent: PathBuf,
    parent_identity: FileIdentity,
    initial_identity: Option<FileIdentity>,
}

impl FlutterStageTarget {
    fn revalidate_parent(&self) -> Result<(), FlutterError> {
        let metadata = fs::symlink_metadata(&self.canonical_parent).map_err(|error| {
            FlutterError::storage(format!(
                "failed to recheck Flutter pub staging parent '{}': {error}",
                self.canonical_parent.display()
            ))
        })?;
        let identity = path_file_identity(&self.canonical_parent, &metadata).map_err(|error| {
            FlutterError::storage(format!(
                "failed to identify Flutter pub staging parent '{}': {error}",
                self.canonical_parent.display()
            ))
        })?;
        if !metadata.file_type().is_dir() || identity != self.parent_identity {
            return Err(FlutterError::compatibility(format!(
                "Flutter pub staging parent '{}' changed after validation",
                self.canonical_parent.display()
            )));
        }
        let canonical = self.canonical_parent.canonicalize().map_err(|error| {
            FlutterError::storage(format!(
                "failed to resolve Flutter pub staging parent '{}': {error}",
                self.canonical_parent.display()
            ))
        })?;
        if canonical != self.canonical_parent {
            return Err(FlutterError::compatibility(format!(
                "Flutter pub staging parent '{}' changed after validation",
                self.canonical_parent.display()
            )));
        }
        Ok(())
    }

    fn revalidate_initial_target(&self) -> Result<bool, FlutterError> {
        self.revalidate_parent()?;
        match (self.initial_identity, fs::symlink_metadata(&self.path)) {
            (None, Err(error)) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            (None, Ok(_)) => Err(FlutterError::compatibility(format!(
                "Flutter pub staging output '{}' appeared after validation",
                self.path.display()
            ))),
            (None, Err(error)) => Err(FlutterError::storage(format!(
                "failed to recheck Flutter pub staging output '{}': {error}",
                self.path.display()
            ))),
            (Some(expected), Ok(metadata)) if metadata.file_type().is_dir() => {
                let identity = path_file_identity(&self.path, &metadata).map_err(|error| {
                    FlutterError::storage(format!(
                        "failed to identify Flutter pub staging output '{}': {error}",
                        self.path.display()
                    ))
                })?;
                if identity == expected {
                    Ok(true)
                } else {
                    Err(FlutterError::compatibility(format!(
                        "Flutter pub staging output '{}' changed after validation",
                        self.path.display()
                    )))
                }
            }
            (Some(_), Ok(_)) => Err(FlutterError::compatibility(format!(
                "Flutter pub staging output '{}' changed after validation",
                self.path.display()
            ))),
            (Some(_), Err(error)) => Err(FlutterError::storage(format!(
                "failed to recheck Flutter pub staging output '{}': {error}",
                self.path.display()
            ))),
        }
    }
}

fn resolve_flutter_stage_target(
    root: &Path,
    requested: &Path,
    require_root_containment: bool,
    protected_source_roots: &[PathBuf],
) -> Result<FlutterStageTarget, FlutterError> {
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else if require_root_containment {
        root.join(requested)
    } else {
        env::current_dir()
            .map_err(|error| {
                FlutterError::storage(format!(
                    "failed to determine current directory for Flutter pub staging: {error}"
                ))
            })?
            .join(requested)
    };
    let existing_candidate = match fs::symlink_metadata(&candidate) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            Some(candidate.canonicalize().map_err(|error| {
                FlutterError::storage(format!(
                    "failed to resolve Flutter pub staging output '{}': {error}",
                    candidate.display()
                ))
            })?)
        }
        Ok(_) => {
            return Err(FlutterError::compatibility(format!(
                "Flutter pub staging output '{}' is not a regular non-symlink directory",
                candidate.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(FlutterError::storage(format!(
                "failed to inspect Flutter pub staging output '{}': {error}",
                candidate.display()
            )));
        }
    };

    let (target, canonical_parent) = if let Some(existing) = existing_candidate {
        if require_root_containment {
            let requested_parent = parent_directory(&candidate);
            let validated_parent = create_contained_stage_parent(root, requested_parent)?;
            let canonical_validated_parent = validated_parent.canonicalize().map_err(|error| {
                FlutterError::storage(format!(
                    "failed to resolve Flutter pub staging parent '{}': {error}",
                    validated_parent.display()
                ))
            })?;
            if canonical_validated_parent != parent_directory(&existing) {
                return Err(FlutterError::compatibility(format!(
                    "default Flutter pub staging output '{}' resolves outside repository root '{}'",
                    candidate.display(),
                    root.display()
                )));
            }
        }
        let parent = parent_directory(&existing).to_path_buf();
        (existing, parent)
    } else {
        let Component::Normal(file_name) = candidate.components().next_back().ok_or_else(|| {
            FlutterError::compatibility(format!(
                "Flutter pub staging output '{}' must name a directory",
                candidate.display()
            ))
        })?
        else {
            return Err(FlutterError::compatibility(format!(
                "Flutter pub staging output '{}' must end in a regular directory name",
                candidate.display()
            )));
        };
        let requested_parent = parent_directory(&candidate);
        let parent = if require_root_containment {
            create_contained_stage_parent(root, requested_parent)?
        } else {
            fs::create_dir_all(requested_parent).map_err(|error| {
                FlutterError::storage(format!(
                    "failed to create Flutter pub staging parent '{}': {error}",
                    requested_parent.display()
                ))
            })?;
            requested_parent.canonicalize().map_err(|error| {
                FlutterError::storage(format!(
                    "failed to resolve Flutter pub staging parent '{}': {error}",
                    requested_parent.display()
                ))
            })?
        };
        (parent.join(file_name), parent)
    };

    validate_flutter_stage_target(root, &target, protected_source_roots)?;
    let initial_identity = match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            Some(path_file_identity(&target, &metadata).map_err(|error| {
                FlutterError::storage(format!(
                    "failed to identify Flutter pub staging output '{}': {error}",
                    target.display()
                ))
            })?)
        }
        Ok(_) => {
            return Err(FlutterError::compatibility(format!(
                "Flutter pub staging output '{}' is not a regular non-symlink directory",
                target.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(FlutterError::storage(format!(
                "failed to inspect Flutter pub staging output '{}': {error}",
                target.display()
            )));
        }
    };
    let parent_metadata = fs::symlink_metadata(&canonical_parent).map_err(|error| {
        FlutterError::storage(format!(
            "failed to inspect Flutter pub staging parent '{}': {error}",
            canonical_parent.display()
        ))
    })?;
    if !parent_metadata.file_type().is_dir() {
        return Err(FlutterError::compatibility(format!(
            "Flutter pub staging parent '{}' is not a regular non-symlink directory",
            canonical_parent.display()
        )));
    }
    let parent_identity =
        path_file_identity(&canonical_parent, &parent_metadata).map_err(|error| {
            FlutterError::storage(format!(
                "failed to identify Flutter pub staging parent '{}': {error}",
                canonical_parent.display()
            ))
        })?;
    Ok(FlutterStageTarget {
        path: target,
        canonical_parent,
        parent_identity,
        initial_identity,
    })
}

fn validate_flutter_stage_target(
    root: &Path,
    target: &Path,
    protected_source_roots: &[PathBuf],
) -> Result<(), FlutterError> {
    if root.starts_with(target) {
        return Err(FlutterError::compatibility(format!(
            "Flutter pub staging output '{}' must not be the repository root or one of its ancestors",
            target.display()
        )));
    }
    let current_directory = env::current_dir()
        .and_then(|path| path.canonicalize())
        .map_err(|error| {
            FlutterError::storage(format!(
                "failed to resolve current directory for Flutter pub staging: {error}"
            ))
        })?;
    if current_directory.starts_with(target) {
        return Err(FlutterError::compatibility(format!(
            "Flutter pub staging output '{}' must not contain the current working directory '{}'",
            target.display(),
            current_directory.display()
        )));
    }
    let mut protected_sources = protected_source_roots.to_vec();
    for protected_alias in [
        root.join("lib/flutter"),
        root.join("lib/ios/VesperPlayerOptionalPlugins"),
    ] {
        let protected = match fs::symlink_metadata(&protected_alias) {
            Ok(_) => protected_alias.canonicalize().map_err(|error| {
                FlutterError::storage(format!(
                    "failed to resolve protected Flutter source directory '{}': {error}",
                    protected_alias.display()
                ))
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => protected_alias,
            Err(error) => {
                return Err(FlutterError::storage(format!(
                    "failed to inspect protected Flutter source directory '{}': {error}",
                    protected_alias.display()
                )));
            }
        };
        protected_sources.push(protected);
    }
    for protected in protected_sources {
        if target.starts_with(&protected) || protected.starts_with(target) {
            return Err(FlutterError::compatibility(format!(
                "Flutter pub staging output '{}' overlaps protected source directory '{}'",
                target.display(),
                protected.display()
            )));
        }
    }
    Ok(())
}

fn create_contained_stage_parent(root: &Path, parent: &Path) -> Result<PathBuf, FlutterError> {
    let relative = parent.strip_prefix(root).map_err(|_| {
        FlutterError::compatibility(format!(
            "default Flutter pub staging parent '{}' is outside repository root '{}'",
            parent.display(),
            root.display()
        ))
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(FlutterError::compatibility(format!(
                "default Flutter pub staging parent '{}' contains an unsupported path component",
                parent.display()
            )));
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => {
                return Err(FlutterError::compatibility(format!(
                    "default Flutter pub staging directory '{}' is not a regular non-symlink directory",
                    current.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| {
                    FlutterError::storage(format!(
                        "failed to create Flutter pub staging directory '{}': {error}",
                        current.display()
                    ))
                })?;
            }
            Err(error) => {
                return Err(FlutterError::storage(format!(
                    "failed to inspect Flutter pub staging directory '{}': {error}",
                    current.display()
                )));
            }
        }
        let canonical = current.canonicalize().map_err(|error| {
            FlutterError::storage(format!(
                "failed to resolve Flutter pub staging directory '{}': {error}",
                current.display()
            ))
        })?;
        if !canonical.starts_with(root) {
            return Err(FlutterError::compatibility(format!(
                "default Flutter pub staging directory '{}' resolves outside repository root '{}'",
                current.display(),
                root.display()
            )));
        }
    }
    Ok(current)
}

fn promote_flutter_stage(
    staging: tempfile::TempDir,
    target: &FlutterStageTarget,
) -> Result<(), FlutterError> {
    promote_flutter_stage_with_hook(staging, target, None)
}

fn promote_flutter_stage_with_hook(
    staging: tempfile::TempDir,
    target: &FlutterStageTarget,
    mut after_backup: Option<crate::PathIoHook<'_>>,
) -> Result<(), FlutterError> {
    target.revalidate_parent()?;
    let existing = target.revalidate_initial_target()?;
    if !existing {
        target.revalidate_parent()?;
        target.revalidate_initial_target()?;
        let staging_path = staging.keep();
        return fs::rename(&staging_path, &target.path).map_err(|error| {
            let cleanup = fs::remove_dir_all(&staging_path);
            let mut message = format!(
                "failed to promote Flutter pub staging directory '{}': {error}",
                target.path.display()
            );
            if let Err(cleanup_error) = cleanup {
                message.push_str(&format!(
                    "; failed to remove retained staging directory '{}': {cleanup_error}",
                    staging_path.display()
                ));
            }
            FlutterError::storage(message)
        });
    }

    let backup_container = tempfile::Builder::new()
        .prefix(".vesper-flutter-pub-backup-")
        .tempdir_in(&target.canonical_parent)
        .map_err(|error| {
            FlutterError::storage(format!(
                "failed to create Flutter pub staging backup beside '{}': {error}",
                target.path.display()
            ))
        })?;
    let backup = backup_container.path().join("previous");
    target.revalidate_parent()?;
    target.revalidate_initial_target()?;
    fs::rename(&target.path, &backup).map_err(|error| {
        FlutterError::storage(format!(
            "failed to back up existing Flutter pub staging output '{}': {error}",
            target.path.display()
        ))
    })?;

    let pre_promotion = target.revalidate_parent().and_then(|()| {
        if let Some(hook) = after_backup.as_mut() {
            hook(&target.path).map_err(|error| FlutterError::storage(error.to_string()))?;
        }
        target.revalidate_parent()
    });
    let (promotion, retained_staging) = match pre_promotion {
        Ok(()) => {
            let staging_path = staging.keep();
            match fs::rename(&staging_path, &target.path) {
                Ok(()) => (Ok(()), None),
                Err(error) => (
                    Err(FlutterError::storage(error.to_string())),
                    Some(staging_path),
                ),
            }
        }
        Err(error) => (Err(error), Some(staging.keep())),
    };
    if let Err(error) = promotion {
        let rollback = target.revalidate_parent().and_then(|()| {
            fs::rename(&backup, &target.path)
                .map_err(|error| FlutterError::storage(error.to_string()))
        });
        let staging_cleanup = retained_staging.map(|path| {
            target.revalidate_parent().and_then(|()| {
                fs::remove_dir_all(&path).map_err(|cleanup_error| {
                    FlutterError::storage(format!(
                        "failed to remove retained staging directory '{}': {cleanup_error}",
                        path.display()
                    ))
                })
            })
        });
        return match rollback {
            Ok(()) => {
                let mut message = format!(
                    "failed to promote Flutter pub staging output '{}': {error}",
                    target.path.display()
                );
                if let Some(Err(cleanup_error)) = staging_cleanup {
                    message.push_str(&format!("; {cleanup_error}"));
                }
                Err(FlutterError::storage(message))
            }
            Err(rollback_error) => {
                let parent_is_stable = target.revalidate_parent().is_ok();
                let preserved_container = backup_container.keep();
                let preserved_backup = preserved_container.join("previous");
                let mut message = if parent_is_stable {
                    format!(
                        "failed to promote Flutter pub staging output '{}': {error}; failed to restore previous output: {rollback_error}; previous output preserved at '{}'",
                        target.path.display(),
                        preserved_backup.display()
                    )
                } else {
                    format!(
                        "failed to promote Flutter pub staging output '{}': {error}; failed to restore previous output: {rollback_error}; previous output retained under the original staging parent, but that parent changed and the backup's current path cannot be determined",
                        target.path.display()
                    )
                };
                if let Some(Err(cleanup_error)) = staging_cleanup {
                    message.push_str(&format!("; {cleanup_error}"));
                }
                Err(FlutterError::storage(message))
            }
        };
    }
    backup_container.close().map_err(|error| {
        FlutterError::storage(format!(
            "promoted Flutter pub staging output '{}' but failed to remove its backup: {error}",
            target.path.display()
        ))
    })
}

fn selected_packages(include_optional: bool) -> Vec<&'static str> {
    let mut packages = CORE_PACKAGES.to_vec();
    if include_optional {
        packages.extend(OPTIONAL_PACKAGES);
    }
    packages
}

fn format_package_overrides(package: &str, packages: &[&str]) -> Vec<u8> {
    let mut contents = String::from("dependency_overrides:\n");
    for dependency in packages {
        if *dependency != package {
            contents.push_str("  ");
            contents.push_str(dependency);
            contents.push_str(":\n    path: ../");
            contents.push_str(dependency);
            contents.push('\n');
        }
    }
    contents.into_bytes()
}

fn format_example_overrides(packages: &[&str]) -> Vec<u8> {
    let mut contents = String::from("dependency_overrides:\n");
    for dependency in packages {
        contents.push_str("  ");
        contents.push_str(dependency);
        contents.push_str(":\n    path: ../../lib/flutter/");
        contents.push_str(dependency);
        contents.push('\n');
    }
    contents.into_bytes()
}

fn is_valid_flutter_package_version(version: &str) -> bool {
    let suffix_index = version.find(['+', '-']);
    let (core, suffix) = suffix_index.map_or((version, None), |index| {
        (&version[..index], Some(&version[index + 1..]))
    });
    let mut components = core.split('.');
    let valid_core = (0..3).all(|_| {
        components.next().is_some_and(|component| {
            !component.is_empty() && component.bytes().all(|b| b.is_ascii_digit())
        })
    }) && components.next().is_none();
    let valid_suffix = suffix.is_none_or(|suffix| {
        !suffix.is_empty()
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    });
    valid_core && valid_suffix
}

fn rewrite_pubspec(source: &str, version: &str, packages: &[&str]) -> String {
    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    let mut rewritten = String::with_capacity(source.len());
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let has_newline = line.ends_with('\n');
        let body = line.strip_suffix('\n').unwrap_or(line);

        if has_newline
            && (is_publish_to_none(body)
                || body.starts_with("repository:")
                || body.starts_with("issue_tracker:"))
        {
            index += 1;
            continue;
        }
        if body.starts_with("version: ") {
            push_rewritten_line(&mut rewritten, &format!("version: {version}"), has_newline);
            index += 1;
            continue;
        }
        if body.starts_with("homepage:") {
            rewritten.push_str("homepage: https://github.com/umbrella22/Vesper\n");
            rewritten.push_str("repository: https://github.com/umbrella22/Vesper\n");
            rewritten.push_str("issue_tracker: https://github.com/umbrella22/Vesper/issues");
            if has_newline {
                rewritten.push('\n');
            }
            index += 1;
            continue;
        }

        let mut replaced_dependency = false;
        for package in packages {
            let dependency_header = format!("  {package}:");
            let dependency_path = format!("    path: ../{package}");
            if body == dependency_header
                && lines.get(index + 1).is_some_and(|next| {
                    next.ends_with('\n')
                        && next.strip_suffix('\n').unwrap_or(next) == dependency_path
                })
            {
                rewritten.push_str(&format!("  {package}: ^{version}\n"));
                index += 2;
                replaced_dependency = true;
                break;
            }
            if let Some(current_version) = body.strip_prefix(&format!("  {package}: ^"))
                && has_newline
                && is_valid_flutter_package_version(current_version)
            {
                push_rewritten_line(
                    &mut rewritten,
                    &format!("  {package}: ^{version}"),
                    has_newline,
                );
                index += 1;
                replaced_dependency = true;
                break;
            }
        }
        if replaced_dependency {
            continue;
        }

        rewritten.push_str(line);
        index += 1;
    }
    rewritten
}

fn is_publish_to_none(line: &str) -> bool {
    line.strip_prefix("publish_to:")
        .is_some_and(|value| matches!(value.trim(), "none" | "'none'" | "\"none\""))
}

fn push_rewritten_line(output: &mut String, line: &str, has_newline: bool) {
    output.push_str(line);
    if has_newline {
        output.push('\n');
    }
}

fn require_contained_regular_file(
    containment_root: &Path,
    path: &Path,
    label: &str,
) -> Result<FileSnapshot, FlutterError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        FlutterError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(FlutterError::storage(format!(
            "{label} '{}' is not a regular non-symlink file",
            path.display()
        )));
    }
    let canonical = path.canonicalize().map_err(|error| {
        FlutterError::storage(format!(
            "failed to resolve {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !canonical.starts_with(containment_root) {
        return Err(FlutterError::compatibility(format!(
            "{label} '{}' resolves outside repository root '{}'",
            path.display(),
            containment_root.display()
        )));
    }
    let identity = path_file_identity(path, &metadata).map_err(|error| {
        FlutterError::storage(format!(
            "failed to identify {label} '{}': {error}",
            path.display()
        ))
    })?;
    Ok(FileSnapshot { metadata, identity })
}

struct OriginalOverride {
    bytes: Vec<u8>,
    permissions: Permissions,
}

struct PlannedOverride {
    path: PathBuf,
    canonical_parent: PathBuf,
    target_path: PathBuf,
    updated: Vec<u8>,
    original: Option<OriginalOverride>,
}

impl PlannedOverride {
    fn preflight(
        containment_root: &Path,
        path: PathBuf,
        updated: Vec<u8>,
    ) -> Result<Self, FlutterError> {
        let parent = parent_directory(&path);
        let parent_metadata = fs::symlink_metadata(parent).map_err(|error| {
            FlutterError::storage(format!(
                "failed to inspect Flutter override directory '{}': {error}",
                parent.display()
            ))
        })?;
        if !parent_metadata.file_type().is_dir() {
            return Err(FlutterError::storage(format!(
                "Flutter override directory '{}' is not a regular non-symlink directory",
                parent.display()
            )));
        }
        let canonical_parent = parent.canonicalize().map_err(|error| {
            FlutterError::storage(format!(
                "failed to resolve Flutter override directory '{}': {error}",
                parent.display()
            ))
        })?;
        if !canonical_parent.starts_with(containment_root) {
            return Err(FlutterError::compatibility(format!(
                "Flutter override directory '{}' resolves outside repository root '{}'",
                parent.display(),
                containment_root.display()
            )));
        }
        let file_name = path.file_name().ok_or_else(|| {
            FlutterError::storage(format!(
                "Flutter override output '{}' does not have a file name",
                path.display()
            ))
        })?;
        let target_path = canonical_parent.join(file_name);

        let original = match fs::symlink_metadata(&target_path) {
            Ok(metadata) => {
                if !metadata.file_type().is_file() {
                    return Err(FlutterError::storage(format!(
                        "Flutter override output '{}' is not a regular non-symlink file",
                        path.display()
                    )));
                }
                if metadata.len() > MAX_LOCAL_OVERRIDES_FILE_BYTES {
                    return Err(FlutterError::storage(format!(
                        "Flutter override output '{}' exceeds {MAX_LOCAL_OVERRIDES_FILE_BYTES} bytes",
                        path.display()
                    )));
                }
                Some(read_override_snapshot(&target_path).map_err(|error| {
                    FlutterError::storage(format!(
                        "failed to read Flutter override output '{}': {error}",
                        path.display()
                    ))
                })?)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(FlutterError::storage(format!(
                    "failed to inspect Flutter override output '{}': {error}",
                    path.display()
                )));
            }
        };

        Ok(Self {
            path,
            canonical_parent,
            target_path,
            updated,
            original,
        })
    }

    fn revalidate(&self) -> Result<(), FlutterError> {
        self.revalidate_parent()?;
        match (&self.original, fs::symlink_metadata(&self.target_path)) {
            (None, Err(error)) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            (None, Ok(_)) => Err(FlutterError::storage(format!(
                "Flutter override output '{}' changed after preflight",
                self.path.display()
            ))),
            (None, Err(error)) => Err(FlutterError::storage(format!(
                "failed to recheck Flutter override output '{}': {error}",
                self.path.display()
            ))),
            (Some(original), Ok(metadata)) if metadata.file_type().is_file() => {
                let current = read_override_snapshot(&self.target_path).map_err(|error| {
                    FlutterError::storage(format!(
                        "failed to recheck Flutter override output '{}': {error}",
                        self.path.display()
                    ))
                })?;
                if current.bytes == original.bytes
                    && permissions_match(&current.permissions, &original.permissions)
                {
                    Ok(())
                } else {
                    Err(FlutterError::storage(format!(
                        "Flutter override output '{}' changed after preflight",
                        self.path.display()
                    )))
                }
            }
            (Some(_), Ok(_)) => Err(FlutterError::storage(format!(
                "Flutter override output '{}' changed type after preflight",
                self.path.display()
            ))),
            (Some(_), Err(error)) => Err(FlutterError::storage(format!(
                "failed to recheck Flutter override output '{}': {error}",
                self.path.display()
            ))),
        }
    }

    fn revalidate_parent(&self) -> Result<(), FlutterError> {
        self.revalidate_parent_io().map_err(|error| {
            FlutterError::storage(format!(
                "failed to recheck Flutter override directory '{}': {error}",
                parent_directory(&self.path).display()
            ))
        })
    }

    fn revalidate_parent_io(&self) -> io::Result<()> {
        let current_parent = parent_directory(&self.path).canonicalize()?;
        if current_parent == self.canonical_parent {
            Ok(())
        } else {
            Err(io::Error::other("directory changed after preflight"))
        }
    }
}

struct StagedOverride {
    planned: PlannedOverride,
    temporary: tempfile::NamedTempFile,
}

struct OverrideTransaction {
    planned: Vec<PlannedOverride>,
}

impl OverrideTransaction {
    fn commit(self, fail_before_promotion: Option<usize>) -> Result<(), FlutterError> {
        let mut staged = Vec::with_capacity(self.planned.len());
        for planned in self.planned {
            planned.revalidate()?;
            let mut temporary = tempfile::NamedTempFile::new_in(&planned.canonical_parent)
                .map_err(|error| {
                    FlutterError::storage(format!(
                        "failed to stage Flutter override '{}': {error}",
                        planned.path.display()
                    ))
                })?;
            set_staged_permissions(&temporary, planned.original.as_ref())?;
            temporary
                .write_all(&planned.updated)
                .and_then(|()| temporary.as_file().sync_all())
                .map_err(|error| {
                    FlutterError::storage(format!(
                        "failed to write staged Flutter override '{}': {error}",
                        planned.path.display()
                    ))
                })?;
            staged.push(StagedOverride { planned, temporary });
        }

        let mut committed = Vec::new();
        for (index, staged_override) in staged.into_iter().enumerate() {
            let StagedOverride { planned, temporary } = staged_override;
            if fail_before_promotion == Some(index) {
                let rollback = rollback_overrides(&committed);
                return Err(transaction_error(
                    &planned.path,
                    &"injected promotion failure",
                    rollback,
                ));
            }
            if let Err(error) = planned.revalidate() {
                let rollback = rollback_overrides(&committed);
                return Err(transaction_error(&planned.path, &error, rollback));
            }
            if let Err(error) = temporary.persist(&planned.target_path) {
                let rollback = rollback_overrides(&committed);
                return Err(transaction_error(&planned.path, &error.error, rollback));
            }
            let committed_path = planned.target_path.clone();
            committed.push(planned);
            if let Err(error) = sync_parent_directory(&committed_path) {
                let rollback = rollback_overrides(&committed);
                return Err(transaction_error(&committed_path, &error, rollback));
            }
        }
        Ok(())
    }
}

fn set_staged_permissions(
    temporary: &tempfile::NamedTempFile,
    original: Option<&OriginalOverride>,
) -> Result<(), FlutterError> {
    if let Some(original) = original {
        temporary
            .as_file()
            .set_permissions(original.permissions.clone())
            .map_err(|error| {
                FlutterError::storage(format!(
                    "failed to preserve Flutter override permissions: {error}"
                ))
            })?;
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            temporary
                .as_file()
                .set_permissions(Permissions::from_mode(0o644))
                .map_err(|error| {
                    FlutterError::storage(format!(
                        "failed to set Flutter override permissions: {error}"
                    ))
                })?;
        }
    }
    Ok(())
}

fn rollback_overrides(committed: &[PlannedOverride]) -> Vec<String> {
    let mut failures = Vec::new();
    for planned in committed.iter().rev() {
        if let Err(error) = rollback_override(planned) {
            failures.push(format!("{}: {error}", planned.path.display()));
        }
    }
    failures
}

fn rollback_override(planned: &PlannedOverride) -> io::Result<()> {
    planned.revalidate_parent_io()?;
    let metadata = match fs::symlink_metadata(&planned.target_path) {
        Ok(metadata) if metadata.file_type().is_file() => metadata,
        Ok(_) => {
            return Err(io::Error::other(
                "output changed type before transaction rollback",
            ));
        }
        Err(error) => return Err(error),
    };
    if metadata.len() > MAX_LOCAL_OVERRIDES_FILE_BYTES
        || read_override_snapshot(&planned.target_path)?.bytes != planned.updated
    {
        return Err(io::Error::other(
            "output changed before transaction rollback",
        ));
    }

    if let Some(original) = &planned.original {
        atomic_replace(
            &planned.target_path,
            &original.bytes,
            original.permissions.clone(),
        )?;
    } else {
        fs::remove_file(&planned.target_path)?;
        sync_parent_directory(&planned.target_path)?;
    }
    Ok(())
}

fn read_override_snapshot(path: &Path) -> io::Result<OriginalOverride> {
    let mut file = File::open(path)?;
    let opened_metadata = file.metadata()?;
    let path_metadata = fs::symlink_metadata(path)?;
    if !opened_metadata.is_file() || !path_metadata.file_type().is_file() {
        return Err(io::Error::other("output is not a regular non-symlink file"));
    }
    if opened_file_identity(&file, &opened_metadata)? != path_file_identity(path, &path_metadata)? {
        return Err(io::Error::other("output changed while it was being opened"));
    }
    if opened_metadata.len() > MAX_LOCAL_OVERRIDES_FILE_BYTES {
        return Err(io::Error::other(format!(
            "output exceeds {MAX_LOCAL_OVERRIDES_FILE_BYTES} bytes"
        )));
    }
    let mut bytes = Vec::with_capacity(opened_metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_LOCAL_OVERRIDES_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_LOCAL_OVERRIDES_FILE_BYTES {
        return Err(io::Error::other(format!(
            "output exceeds {MAX_LOCAL_OVERRIDES_FILE_BYTES} bytes"
        )));
    }
    Ok(OriginalOverride {
        bytes,
        permissions: opened_metadata.permissions(),
    })
}

#[cfg(unix)]
fn path_file_identity(_path: &Path, metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    Ok(FileIdentity {
        volume_or_device: metadata.dev(),
        file_index: metadata.ino(),
    })
}

#[cfg(unix)]
fn opened_file_identity(_file: &File, metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    path_file_identity(Path::new(""), metadata)
}

#[cfg(windows)]
fn path_file_identity(path: &Path, _metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    let handle = winapi_util::Handle::from_path_any(path)?;
    windows_handle_identity(&handle)
}

#[cfg(windows)]
fn opened_file_identity(file: &File, _metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    let handle = winapi_util::HandleRef::from_file(file);
    windows_handle_identity(&handle)
}

#[cfg(windows)]
fn windows_handle_identity<H: winapi_util::AsHandleRef>(handle: H) -> io::Result<FileIdentity> {
    let information = winapi_util::file::information(handle)?;
    Ok(FileIdentity {
        volume_or_device: information.volume_serial_number(),
        file_index: information.file_index(),
    })
}

#[cfg(not(any(unix, windows)))]
fn path_file_identity(_path: &Path, _metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "file identity is unsupported on this host",
    ))
}

#[cfg(not(any(unix, windows)))]
fn opened_file_identity(_file: &File, _metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "file identity is unsupported on this host",
    ))
}

fn atomic_replace(path: &Path, bytes: &[u8], permissions: Permissions) -> io::Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(parent_directory(path))?;
    temporary.as_file().set_permissions(permissions)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    sync_parent_directory(path)
}

fn transaction_error(
    path: &Path,
    error: &dyn std::fmt::Display,
    rollback: Vec<String>,
) -> FlutterError {
    let mut message = format!(
        "failed to atomically replace Flutter override '{}': {error}",
        path.display()
    );
    if !rollback.is_empty() {
        message.push_str("\nrollback also failed for:\n  ");
        message.push_str(&rollback.join("\n  "));
    }
    FlutterError::storage(message)
}

#[cfg(unix)]
fn permissions_match(left: &Permissions, right: &Permissions) -> bool {
    use std::os::unix::fs::PermissionsExt;

    left.mode() == right.mode()
}

#[cfg(not(unix))]
fn permissions_match(left: &Permissions, right: &Permissions) -> bool {
    left.readonly() == right.readonly()
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> io::Result<()> {
    File::open(parent_directory(path))?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn output_error(error: io::Error) -> FlutterError {
    FlutterError::storage(format!("failed to write Flutter command output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pub_dev_new_package_wait_enforces_and_expires_the_burst_window() {
        let now = Instant::now();
        let mut recent = VecDeque::from([
            now - Duration::from_secs(30),
            now - Duration::from_secs(20),
            now - Duration::from_secs(10),
            now - Duration::from_secs(1),
        ]);
        assert_eq!(
            pub_dev_new_package_wait(&mut recent, now),
            Some(Duration::from_secs(95))
        );

        let later = now + Duration::from_secs(91);
        assert_eq!(pub_dev_new_package_wait(&mut recent, later), None);
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn flutter_package_version_matches_the_legacy_release_contract() {
        for version in ["0.4.0", "1.2.3-beta.1", "1.2.3+build.7", "01.002.0003"] {
            assert!(is_valid_flutter_package_version(version), "{version}");
        }
        for version in [
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "1.2.3+",
            "1.2.3+build+again",
            "1.2.3/other",
        ] {
            assert!(!is_valid_flutter_package_version(version), "{version}");
        }
    }

    #[test]
    fn pubspec_rewrite_preserves_unrelated_text_and_rewrites_local_dependencies() {
        let source = r#"name: fixture
publish_to: 'none'
version: 0.4.0
homepage: https://example.invalid/old
repository: https://example.invalid/repository
issue_tracker: https://example.invalid/issues
description: Keep this text exactly.
dependencies:
  vesper_player_platform_interface:
    path: ../vesper_player_platform_interface
  vesper_player_android: ^0.3.0-beta.1
  unrelated: ^9.0.0
"#;
        let rewritten = rewrite_pubspec(
            source,
            "1.2.3-beta.2",
            &["vesper_player_platform_interface", "vesper_player_android"],
        );

        assert_eq!(
            rewritten,
            r#"name: fixture
version: 1.2.3-beta.2
homepage: https://github.com/umbrella22/Vesper
repository: https://github.com/umbrella22/Vesper
issue_tracker: https://github.com/umbrella22/Vesper/issues
description: Keep this text exactly.
dependencies:
  vesper_player_platform_interface: ^1.2.3-beta.2
  vesper_player_android: ^1.2.3-beta.2
  unrelated: ^9.0.0
"#
        );
    }

    #[test]
    fn pubspec_rewrite_preserves_terminal_lines_not_matched_by_the_legacy_script() {
        for source in [
            "publish_to: none",
            "publish_to: 'none'",
            "publish_to: \"none\"",
            "repository: https://example.invalid/repository",
            "issue_tracker: https://example.invalid/issues",
        ] {
            assert_eq!(rewrite_pubspec(source, "1.2.3", &[]), source);
        }

        let path_dependency =
            "dependencies:\n  vesper_player_android:\n    path: ../vesper_player_android";
        assert_eq!(
            rewrite_pubspec(path_dependency, "1.2.3", &["vesper_player_android"]),
            path_dependency
        );

        let version_dependency = "dependencies:\n  vesper_player_android: ^0.4.0";
        assert_eq!(
            rewrite_pubspec(version_dependency, "1.2.3", &["vesper_player_android"]),
            version_dependency
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_gradle_candidates_exclude_the_posix_launcher() {
        assert_eq!(
            executable_candidates(Path::new("tools"), "gradle"),
            vec![
                PathBuf::from("tools/gradle.exe"),
                PathBuf::from("tools/gradle.cmd"),
                PathBuf::from("tools/gradle.bat"),
            ]
        );
    }

    #[test]
    fn stage_entry_budget_rejects_before_collecting_an_unbounded_directory() {
        let mut budget = FlutterStageBudget {
            entries: MAX_FLUTTER_STAGE_ENTRIES,
            bytes: 0,
        };
        let error = budget
            .record_entry(Path::new("next-entry"))
            .expect_err("reject a source entry beyond the traversal bound");

        assert!(error.to_string().contains("source entries"));
    }

    #[test]
    fn stage_copy_rejects_file_growth_after_preflight() {
        let source_directory = tempfile::tempdir().expect("temporary Flutter source");
        let destination_directory = tempfile::tempdir().expect("temporary Flutter destination");
        let source_root = source_directory
            .path()
            .canonicalize()
            .expect("resolve Flutter source root");
        let source = source_root.join("source.txt");
        let destination = destination_directory.path().join("copied.txt");
        fs::write(&source, b"before").expect("write source before preflight");
        let expected = fs::symlink_metadata(&source).expect("snapshot source metadata");
        let expected_identity =
            path_file_identity(&source, &expected).expect("snapshot source identity");
        fs::write(&source, b"after preflight growth").expect("grow source after preflight");

        let error = copy_regular_file(
            &source_root,
            &source,
            &destination,
            "fixture source",
            &expected,
            expected_identity,
            &mut FlutterStageCopyBudget::default(),
        )
        .expect_err("reject source growth after preflight");

        assert!(
            error
                .to_string()
                .contains("changed after staging preflight")
        );
        assert!(!destination.exists());
    }

    #[test]
    fn stage_copy_budget_rejects_before_creating_the_destination() {
        let source_directory = tempfile::tempdir().expect("temporary Flutter source");
        let destination_directory = tempfile::tempdir().expect("temporary Flutter destination");
        let source_root = source_directory
            .path()
            .canonicalize()
            .expect("resolve Flutter source root");
        let source = source_root.join("source.txt");
        let destination = destination_directory.path().join("copied.txt");
        fs::write(&source, b"bounded").expect("write bounded source");
        let expected = fs::symlink_metadata(&source).expect("snapshot source metadata");
        let expected_identity =
            path_file_identity(&source, &expected).expect("snapshot source identity");
        let mut budget = FlutterStageCopyBudget {
            bytes: MAX_FLUTTER_STAGE_BYTES,
        };

        let error = copy_regular_file(
            &source_root,
            &source,
            &destination,
            "fixture source",
            &expected,
            expected_identity,
            &mut budget,
        )
        .expect_err("reject copied bytes beyond the output bound");

        assert!(error.to_string().contains("copied bytes"));
        assert!(!destination.exists());
    }

    #[cfg(unix)]
    #[test]
    fn stage_copy_rejects_a_source_replaced_by_an_external_symlink() {
        use std::os::unix::fs::symlink;

        let source_directory = tempfile::tempdir().expect("temporary Flutter source");
        let destination_directory = tempfile::tempdir().expect("temporary Flutter destination");
        let external_directory = tempfile::tempdir().expect("external source directory");
        let source_root = source_directory
            .path()
            .canonicalize()
            .expect("resolve Flutter source root");
        let source = source_root.join("source.txt");
        let destination = destination_directory.path().join("copied.txt");
        let external = external_directory.path().join("external.txt");
        fs::write(&source, b"internal").expect("write internal source");
        fs::write(&external, b"external").expect("write external source");
        let expected = fs::symlink_metadata(&source).expect("snapshot source metadata");
        let expected_identity =
            path_file_identity(&source, &expected).expect("snapshot source identity");
        fs::remove_file(&source).expect("remove source after preflight");
        symlink(&external, &source).expect("replace source with external symlink");

        let error = copy_regular_file(
            &source_root,
            &source,
            &destination,
            "fixture source",
            &expected,
            expected_identity,
            &mut FlutterStageCopyBudget::default(),
        )
        .expect_err("reject external source replacement");

        assert!(error.to_string().contains("non-symlink file"));
        assert!(!destination.exists());
    }

    #[cfg(unix)]
    #[test]
    fn stage_promotion_rejects_a_parent_replaced_by_an_external_symlink() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temporary Flutter promotion boundary");
        let repository = directory.path().join("repository");
        fs::create_dir_all(&repository).expect("create promotion repository");
        let canonical_repository = repository
            .canonicalize()
            .expect("resolve promotion repository");
        let release = canonical_repository.join("dist/release");
        fs::create_dir_all(&release).expect("create release directory");
        let target = resolve_flutter_stage_target(
            &canonical_repository,
            &release.join("flutter-pub"),
            true,
            &[],
        )
        .expect("resolve Flutter stage target");
        let staging = tempfile::Builder::new()
            .prefix(".vesper-stage-parent-test-")
            .tempdir_in(&target.canonical_parent)
            .expect("create Flutter staging directory");
        fs::write(staging.path().join("sentinel"), b"staged").expect("write staged sentinel");

        let moved_release = canonical_repository.join("dist/moved-release");
        fs::rename(&release, &moved_release).expect("move validated release parent");
        let external = tempfile::tempdir().expect("external promotion target");
        symlink(external.path(), &release).expect("replace release parent with external symlink");

        let error = promote_flutter_stage(staging, &target)
            .expect_err("reject promotion through a replaced parent");
        assert!(error.to_string().contains("changed after validation"));
        assert!(
            fs::read_dir(external.path())
                .expect("inspect external promotion target")
                .next()
                .is_none()
        );
        assert!(
            fs::read_dir(&moved_release)
                .expect("inspect preserved original release parent")
                .any(|entry| entry
                    .expect("read preserved staging entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vesper-stage-parent-test-"))
        );
    }

    #[test]
    fn stage_promotion_cleans_staging_when_an_absent_target_appears() {
        let directory = tempfile::tempdir().expect("temporary Flutter promotion boundary");
        let repository = directory.path().join("repository");
        fs::create_dir_all(&repository).expect("create promotion repository");
        let canonical_repository = repository
            .canonicalize()
            .expect("resolve promotion repository");
        let release = canonical_repository.join("dist/release");
        fs::create_dir_all(&release).expect("create release directory");
        let target = resolve_flutter_stage_target(
            &canonical_repository,
            &release.join("flutter-pub"),
            true,
            &[],
        )
        .expect("resolve absent Flutter stage target");
        let staging = tempfile::Builder::new()
            .prefix(".vesper-stage-appearance-test-")
            .tempdir_in(&target.canonical_parent)
            .expect("create Flutter staging directory");
        let staging_path = staging.path().to_path_buf();
        fs::create_dir(&target.path).expect("make target appear after validation");

        let error = promote_flutter_stage(staging, &target)
            .expect_err("reject a target that appeared after validation");
        assert!(error.to_string().contains("appeared after validation"));
        assert!(!staging_path.exists());
    }

    #[test]
    fn failed_stage_rollback_preserves_the_previous_output_and_cleans_staging() {
        let directory = tempfile::tempdir().expect("temporary Flutter rollback boundary");
        let repository = directory.path().join("repository");
        fs::create_dir_all(&repository).expect("create rollback repository");
        let canonical_repository = repository
            .canonicalize()
            .expect("resolve rollback repository");
        let release = canonical_repository.join("dist/release");
        let output = release.join("flutter-pub");
        fs::create_dir_all(&output).expect("create previous Flutter stage");
        fs::write(output.join("previous.txt"), b"previous").expect("write previous Flutter stage");
        let target = resolve_flutter_stage_target(&canonical_repository, &output, true, &[])
            .expect("resolve existing Flutter stage target");
        let staging = tempfile::Builder::new()
            .prefix(".vesper-stage-rollback-test-")
            .tempdir_in(&target.canonical_parent)
            .expect("create replacement Flutter stage");
        let staging_path = staging.path().to_path_buf();
        fs::write(staging.path().join("replacement.txt"), b"replacement")
            .expect("write replacement Flutter stage");
        let mut obstruct_target = |path: &Path| -> io::Result<()> {
            fs::create_dir(path)?;
            fs::write(path.join("obstruction.txt"), b"obstruction")
        };

        let error = promote_flutter_stage_with_hook(staging, &target, Some(&mut obstruct_target))
            .expect_err("preserve backup when promotion and rollback both fail");

        assert!(error.to_string().contains("previous output preserved at"));
        assert!(!staging_path.exists());
        assert_eq!(
            fs::read(target.path.join("obstruction.txt"))
                .expect("read preserved target obstruction"),
            b"obstruction"
        );
        let preserved_backups = fs::read_dir(&target.canonical_parent)
            .expect("inspect rollback parent")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vesper-flutter-pub-backup-")
            })
            .collect::<Vec<_>>();
        assert_eq!(preserved_backups.len(), 1);
        assert_eq!(
            fs::read(preserved_backups[0].path().join("previous/previous.txt"))
                .expect("read preserved previous Flutter stage"),
            b"previous"
        );
    }

    #[test]
    fn stage_promotion_rechecks_parent_after_backup_hook() {
        let directory = tempfile::tempdir().expect("temporary Flutter promotion boundary");
        let repository = directory.path().join("repository");
        let release = repository.join("dist/release");
        let output = release.join("flutter-pub");
        fs::create_dir_all(&output).expect("create previous Flutter stage");
        fs::write(output.join("previous.txt"), b"previous").expect("write previous Flutter stage");
        let canonical_repository = repository
            .canonicalize()
            .expect("resolve promotion repository");
        let canonical_release = canonical_repository.join("dist/release");
        let canonical_output = canonical_release.join("flutter-pub");
        let target =
            resolve_flutter_stage_target(&canonical_repository, &canonical_output, true, &[])
                .expect("resolve existing Flutter stage target");
        let staging = tempfile::Builder::new()
            .prefix(".vesper-stage-parent-hook-test-")
            .tempdir_in(&target.canonical_parent)
            .expect("create replacement Flutter stage");
        fs::write(staging.path().join("replacement.txt"), b"replacement")
            .expect("write replacement Flutter stage");
        let moved_release = canonical_repository.join("dist/moved-release");
        let mut replace_parent = |_path: &Path| -> io::Result<()> {
            fs::rename(&canonical_release, &moved_release)?;
            fs::create_dir(&canonical_release)
        };

        let error = promote_flutter_stage_with_hook(staging, &target, Some(&mut replace_parent))
            .expect_err("reject a parent replaced after backup");

        assert!(error.to_string().contains("changed after validation"));
        assert!(
            error
                .to_string()
                .contains("current path cannot be determined")
        );
        assert!(
            fs::read_dir(&canonical_release)
                .expect("inspect replacement release parent")
                .next()
                .is_none()
        );
        let preserved_backup = fs::read_dir(&moved_release)
            .expect("inspect moved release parent")
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vesper-flutter-pub-backup-")
            })
            .expect("find preserved previous output backup");
        assert_eq!(
            fs::read(preserved_backup.path().join("previous/previous.txt"))
                .expect("read previous output from moved backup"),
            b"previous"
        );
    }

    #[test]
    fn transaction_rolls_back_existing_and_new_outputs() {
        let directory = tempfile::tempdir().expect("temporary Flutter transaction");
        let existing = directory.path().join("existing.yaml");
        let created = directory.path().join("created.yaml");
        let final_path = directory.path().join("final.yaml");
        let containment_root = directory
            .path()
            .canonicalize()
            .expect("resolve temporary Flutter transaction root");
        fs::write(&existing, b"original\n").expect("write original override");

        let transaction = OverrideTransaction {
            planned: vec![
                PlannedOverride::preflight(
                    &containment_root,
                    existing.clone(),
                    b"updated\n".to_vec(),
                )
                .expect("preflight existing output"),
                PlannedOverride::preflight(
                    &containment_root,
                    created.clone(),
                    b"created\n".to_vec(),
                )
                .expect("preflight new output"),
                PlannedOverride::preflight(
                    &containment_root,
                    final_path.clone(),
                    b"final\n".to_vec(),
                )
                .expect("preflight final output"),
            ],
        };
        let error = transaction
            .commit(Some(2))
            .expect_err("injected failure must roll back");

        assert!(error.to_string().contains("injected promotion failure"));
        assert_eq!(
            fs::read(existing).expect("read restored output"),
            b"original\n"
        );
        assert!(!created.exists());
        assert!(!final_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn rollback_refuses_a_parent_replaced_with_an_external_symlink() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temporary Flutter rollback boundary");
        let repository = directory.path().join("repository");
        let package = repository.join("package");
        fs::create_dir_all(&package).expect("create package directory");
        let canonical_repository = repository
            .canonicalize()
            .expect("resolve rollback repository");
        let output = package.join("pubspec_overrides.yaml");
        let planned =
            PlannedOverride::preflight(&canonical_repository, output, b"generated\n".to_vec())
                .expect("preflight absent override");
        fs::write(&planned.target_path, b"generated\n").expect("simulate committed override");

        let moved_package = repository.join("moved-package");
        fs::rename(&package, &moved_package).expect("move original package directory");
        let external_package = directory.path().join("external-package");
        fs::create_dir_all(&external_package).expect("create external package directory");
        let external_output = external_package.join("pubspec_overrides.yaml");
        fs::write(&external_output, b"generated\n").expect("seed external override");
        symlink(&external_package, &package).expect("replace package with external symlink");

        let error = rollback_override(&planned).expect_err("reject changed rollback parent");
        assert!(
            error
                .to_string()
                .contains("directory changed after preflight")
        );
        assert_eq!(
            fs::read(&external_output).expect("read preserved external override"),
            b"generated\n"
        );
        assert_eq!(
            fs::read(moved_package.join("pubspec_overrides.yaml"))
                .expect("read preserved committed override"),
            b"generated\n"
        );
    }
}
