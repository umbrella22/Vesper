use std::io::Write;
use std::path::Path;

use crate::ios::IosError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IosFfiProfile {
    Debug,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IosFfiPlatform {
    Device,
    Simulator,
}

impl IosFfiProfile {
    #[cfg(target_os = "macos")]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

pub(crate) fn ensure_supported_host() -> Result<(), IosError> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(IosError::compatibility(
            "building the iOS Rust FFI XCFramework requires macOS",
        ))
    }
}

pub(crate) fn build(
    root: &Path,
    profile: IosFfiProfile,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), IosError> {
    ensure_supported_host()?;

    #[cfg(target_os = "macos")]
    {
        implementation::build(root, profile, output, diagnostics)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, profile, output, diagnostics);
        unreachable!("the host gate rejects non-macOS iOS FFI builds")
    }
}

pub(crate) fn build_platform(
    root: &Path,
    platform: IosFfiPlatform,
    profile: IosFfiProfile,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), IosError> {
    ensure_supported_host()?;

    #[cfg(target_os = "macos")]
    {
        implementation::build_platform(root, platform, profile, output, diagnostics)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, platform, profile, output, diagnostics);
        unreachable!("the host gate rejects non-macOS iOS FFI builds")
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub(crate) struct IosFfiBuildGuard {
    inner: implementation::IosFfiBuildLock,
}

#[cfg(target_os = "macos")]
pub(crate) fn acquire_build_guard(root: &Path) -> Result<IosFfiBuildGuard, IosError> {
    implementation::IosFfiBuildLock::acquire(root).map(|inner| IosFfiBuildGuard { inner })
}

#[cfg(target_os = "macos")]
pub(crate) fn build_full_in_deferral_holding_lock(
    root: &Path,
    profile: IosFfiProfile,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    cancellation: &crate::external_process::InterruptDeferral,
    cargo_target_directory: Option<&Path>,
    guard: &IosFfiBuildGuard,
) -> Result<(), IosError> {
    implementation::build_full_in_deferral_holding_lock(
        root,
        profile,
        output,
        diagnostics,
        cancellation,
        cargo_target_directory,
        &guard.inner,
    )
}

#[cfg(target_os = "macos")]
mod implementation {
    use std::collections::BTreeSet;
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs::{self, File, OpenOptions, TryLockError};
    use std::io::{self, Read, Write};
    use std::path::{Component, Path, PathBuf};
    use std::process::{Command, ExitStatus, Stdio};

    use serde::Deserialize;

    use super::{IosError, IosFfiPlatform, IosFfiProfile};
    use crate::external_process::{self, BoundedProcessOutput, ExternalProcessErrorKind};

    const BUILD_MODE_ENV: &str = "VESPER_BUILD_IOS_PLAYER_FFI_MODE";
    const CATALYST_POLICY_ENV: &str = "VESPER_BUILD_APPLE_CATALYST";
    const DEVICE_TARGET: &str = "aarch64-apple-ios";
    const SIMULATOR_TARGET: &str = "aarch64-apple-ios-sim";
    const STATIC_LIBRARY_NAME: &str = "libvesper_player_ffi_ios.a";
    const PUBLIC_HEADER_NAME: &str = "player_ffi_resolver.h";
    const MAX_PROCESS_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
    const MAX_CARGO_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
    const MAX_LIPO_OUTPUT_BYTES: usize = 1024 * 1024;
    const MAX_XCFRAMEWORK_MANIFEST_BYTES: u64 = 1024 * 1024;
    const MAX_PUBLIC_HEADER_BYTES: u64 = 1024 * 1024;
    const MAX_STATIC_LIBRARY_BYTES: u64 = 512 * 1024 * 1024;
    const MAX_GENERATED_TREE_ENTRIES: usize = 4096;
    const MAX_GENERATED_TREE_DEPTH: usize = 32;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ApplePlatform {
        Device,
        Simulator,
    }

    impl ApplePlatform {
        const fn platform_name(self) -> &'static str {
            match self {
                Self::Device => "iphoneos",
                Self::Simulator => "iphonesimulator",
            }
        }

        const fn rust_target(self) -> &'static str {
            match self {
                Self::Device => DEVICE_TARGET,
                Self::Simulator => SIMULATOR_TARGET,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum BuildMode {
        Full,
        Platform(ApplePlatform),
    }

    #[derive(Debug)]
    struct RequiredTools {
        cargo: PathBuf,
        rustc: PathBuf,
        xcrun: PathBuf,
        xcodebuild: Option<PathBuf>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct XcframeworkManifest {
        available_libraries: Vec<XcframeworkLibrary>,
        #[serde(rename = "CFBundlePackageType")]
        cf_bundle_package_type: String,
        #[serde(rename = "XCFrameworkFormatVersion")]
        format_version: String,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct XcframeworkLibrary {
        binary_path: String,
        headers_path: String,
        library_identifier: String,
        library_path: String,
        supported_architectures: Vec<String>,
        supported_platform: String,
        #[serde(default)]
        supported_platform_variant: Option<String>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FileIdentity {
        device: u64,
        inode: u64,
    }

    #[derive(Debug)]
    struct OutputTarget {
        path: PathBuf,
        parent: PathBuf,
        parent_identity: FileIdentity,
        initial_identity: Option<FileIdentity>,
    }

    #[derive(Debug)]
    struct OutputPlan {
        artifacts_parent: PathBuf,
        distribution_root: PathBuf,
        target: OutputTarget,
        source_is_staging_root: bool,
        platform: Option<ApplePlatform>,
    }

    #[derive(Debug)]
    struct BuildOutcome {
        distribution_root: PathBuf,
        warnings: Vec<String>,
    }

    #[derive(Debug)]
    pub(super) struct IosFfiBuildLock {
        _file: File,
    }

    #[derive(Debug, Deserialize)]
    struct CargoMessage {
        reason: String,
        #[serde(default)]
        target: Option<CargoTarget>,
        #[serde(default)]
        filenames: Vec<PathBuf>,
        #[serde(default)]
        message: Option<CargoDiagnostic>,
    }

    #[derive(Debug, Deserialize)]
    struct CargoTarget {
        name: String,
        #[serde(default)]
        kind: Vec<String>,
        #[serde(default)]
        crate_types: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct CargoDiagnostic {
        rendered: Option<String>,
    }

    pub(super) fn build(
        root: &Path,
        profile: IosFfiProfile,
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        let cancellation = external_process::InterruptDeferral::start("iOS FFI build")
            .map_err(map_external_process_error)?;
        let result = build_transaction(root, profile, diagnostics, &cancellation, None, None, None);
        let cancelled = cancellation.finish();
        report_build_result(result, cancelled, output, diagnostics)
    }

    pub(super) fn build_platform(
        root: &Path,
        platform: IosFfiPlatform,
        profile: IosFfiProfile,
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        let cancellation = external_process::InterruptDeferral::start("iOS FFI platform build")
            .map_err(map_external_process_error)?;
        let platform = match platform {
            IosFfiPlatform::Device => ApplePlatform::Device,
            IosFfiPlatform::Simulator => ApplePlatform::Simulator,
        };
        let result = build_transaction(
            root,
            profile,
            diagnostics,
            &cancellation,
            Some(BuildMode::Platform(platform)),
            None,
            None,
        );
        let cancelled = cancellation.finish();
        report_build_result(result, cancelled, output, diagnostics)
    }

    fn report_build_result(
        result: Result<BuildOutcome, IosError>,
        cancelled: bool,
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        match result {
            Ok(outcome) => {
                for warning in outcome.warnings {
                    let _ = writeln!(diagnostics, "warning: {warning}");
                }
                if cancelled {
                    diagnostics.flush().map_err(diagnostics_error)?;
                    return Err(IosError::worker(format!(
                        "iOS FFI build was cancelled after the output was committed at '{}'",
                        outcome.distribution_root.display()
                    )));
                }
                writeln!(output).map_err(output_error)?;
                writeln!(output, "Built player-ffi Apple artifacts into:").map_err(output_error)?;
                writeln!(output, "  {}", outcome.distribution_root.display())
                    .map_err(output_error)?;
                output.flush().map_err(output_error)
            }
            Err(error) if cancelled => Err(IosError::worker(format!(
                "iOS FFI build was cancelled; {error}"
            ))),
            Err(error) => Err(error),
        }
    }

    pub(super) fn build_full_in_deferral_holding_lock(
        root: &Path,
        profile: IosFfiProfile,
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
        cargo_target_directory: Option<&Path>,
        guard: &IosFfiBuildLock,
    ) -> Result<(), IosError> {
        let outcome = build_transaction(
            root,
            profile,
            diagnostics,
            cancellation,
            Some(BuildMode::Full),
            cargo_target_directory,
            Some(guard),
        )?;
        for warning in outcome.warnings {
            let _ = writeln!(diagnostics, "warning: {warning}");
        }
        writeln!(output).map_err(output_error)?;
        writeln!(output, "Built player-ffi Apple artifacts into:").map_err(output_error)?;
        writeln!(output, "  {}", outcome.distribution_root.display()).map_err(output_error)?;
        output.flush().map_err(output_error)
    }

    fn build_transaction(
        root: &Path,
        profile: IosFfiProfile,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
        requested_mode: Option<BuildMode>,
        requested_cargo_target_directory: Option<&Path>,
        held_lock: Option<&IosFfiBuildLock>,
    ) -> Result<BuildOutcome, IosError> {
        let mode = match requested_mode {
            Some(mode) => mode,
            None => resolve_build_mode()?,
        };
        if requested_mode.is_none() {
            validate_catalyst_policy()?;
        }
        let tools = resolve_required_tools(mode)?;
        validate_required_rust_targets(&tools.rustc, mode, cancellation)?;
        let cargo_target_directory = match requested_cargo_target_directory {
            Some(path) => path.to_path_buf(),
            None => resolve_cargo_target_directory(root)?,
        };
        let owned_lock = held_lock
            .is_none()
            .then(|| IosFfiBuildLock::acquire(root))
            .transpose()?;
        let _active_lock = held_lock.or(owned_lock.as_ref()).ok_or_else(|| {
            IosError::worker("iOS FFI build transaction is missing its repository lock")
        })?;
        let plan = OutputPlan::preflight(root, mode)?;
        let headers = require_repository_directory(
            root,
            Path::new("lib/ios/VesperPlayerKit/Sources/VesperPlayerFFIResolver/include"),
            "iOS FFI headers directory",
        )?;
        let staging = tempfile::Builder::new()
            .prefix(".vesper-ios-ffi-stage-")
            .tempdir_in(&plan.artifacts_parent)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS FFI staging directory in '{}': {error}",
                    plan.artifacts_parent.display()
                ))
            })?;

        let platforms = match mode {
            BuildMode::Full => vec![ApplePlatform::Device, ApplePlatform::Simulator],
            BuildMode::Platform(platform) => vec![platform],
        };
        for platform in platforms {
            build_platform_archive(
                root,
                platform,
                profile,
                &cargo_target_directory,
                staging.path(),
                &tools,
                diagnostics,
                cancellation,
            )?;
        }
        if matches!(mode, BuildMode::Full) {
            create_xcframework(staging.path(), &headers, &tools, diagnostics, cancellation)?;
            validate_full_staging_output(staging.path())?;
        }

        let source = if plan.source_is_staging_root {
            staging.path().to_path_buf()
        } else {
            let platform = plan.platform.ok_or_else(|| {
                IosError::worker("iOS FFI platform output plan is missing its platform")
            })?;
            staging.path().join(platform.platform_name())
        };
        let warnings = promote_staged_output(staging, &source, &plan.target, cancellation)?;
        Ok(BuildOutcome {
            distribution_root: plan.distribution_root,
            warnings,
        })
    }

    fn resolve_build_mode() -> Result<BuildMode, IosError> {
        let mode = read_optional_utf8_environment(BUILD_MODE_ENV)?;
        match mode.as_deref().unwrap_or("full") {
            "full" => Ok(BuildMode::Full),
            "platform" => match read_optional_utf8_environment("PLATFORM_NAME")?.as_deref() {
                Some("iphoneos") => Ok(BuildMode::Platform(ApplePlatform::Device)),
                Some("iphonesimulator") => Ok(BuildMode::Platform(ApplePlatform::Simulator)),
                Some("macosx") => Err(IosError::compatibility(
                    "Mac Catalyst is outside the supported iOS FFI artifact boundary",
                )),
                Some(value) => Err(IosError::compatibility(format!(
                    "unsupported PLATFORM_NAME for iOS FFI platform mode: {value}"
                ))),
                None => Err(IosError::compatibility(
                    "PLATFORM_NAME is required for iOS FFI platform mode",
                )),
            },
            value => Err(IosError::compatibility(format!(
                "unsupported {BUILD_MODE_ENV} value: {value}; expected full or platform"
            ))),
        }
    }

    fn validate_catalyst_policy() -> Result<(), IosError> {
        let policy = read_optional_utf8_environment(CATALYST_POLICY_ENV)?;
        match policy.as_deref().unwrap_or("auto") {
            "0" | "false" | "FALSE" | "no" | "NO" | "auto" => Ok(()),
            "1" | "true" | "TRUE" | "yes" | "YES" => Err(IosError::compatibility(
                "Mac Catalyst is outside the supported iOS FFI artifact boundary",
            )),
            value => Err(IosError::compatibility(format!(
                "unsupported {CATALYST_POLICY_ENV} value: {value}"
            ))),
        }
    }

    fn read_optional_utf8_environment(name: &str) -> Result<Option<String>, IosError> {
        match env::var(name) {
            Ok(value) if value.is_empty() => Ok(None),
            Ok(value) => Ok(Some(value)),
            Err(env::VarError::NotPresent) => Ok(None),
            Err(env::VarError::NotUnicode(_)) => Err(IosError::compatibility(format!(
                "{name} must be valid UTF-8"
            ))),
        }
    }

    fn resolve_required_tools(mode: BuildMode) -> Result<RequiredTools, IosError> {
        Ok(RequiredTools {
            cargo: require_path_command("cargo")?,
            rustc: require_path_command("rustc")?,
            xcrun: require_path_command("xcrun")?,
            xcodebuild: matches!(mode, BuildMode::Full)
                .then(|| require_path_command("xcodebuild"))
                .transpose()?,
        })
    }

    fn require_path_command(name: &str) -> Result<PathBuf, IosError> {
        let paths = env::var_os("PATH").unwrap_or_default();
        resolve_path_command(&paths, name)
            .ok_or_else(|| IosError::compatibility(format!("Missing required command: {name}")))
    }

    fn resolve_path_command(paths: &OsStr, name: &str) -> Option<PathBuf> {
        use nix::unistd::{AccessFlags, access};

        env::split_paths(paths).find_map(|directory| {
            let directory = if directory.as_os_str().is_empty() {
                env::current_dir().ok()?
            } else {
                directory
            };
            let candidate = directory.join(name);
            let metadata = fs::metadata(&candidate).ok()?;
            (metadata.is_file() && access(&candidate, AccessFlags::X_OK).is_ok())
                .then_some(candidate)
        })
    }

    fn validate_required_rust_targets(
        rustc: &Path,
        mode: BuildMode,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let targets: &[&str] = match mode {
            BuildMode::Full => &[DEVICE_TARGET, SIMULATOR_TARGET],
            BuildMode::Platform(ApplePlatform::Device) => &[DEVICE_TARGET],
            BuildMode::Platform(ApplePlatform::Simulator) => &[SIMULATOR_TARGET],
        };
        let mut missing = Vec::new();
        for target in targets {
            check_cancellation(cancellation, "Rust target validation")?;
            if !rust_target_is_installed(rustc, target, cancellation)? {
                missing.push(*target);
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(IosError::compatibility(format!(
                "Required Rust Apple targets are missing:\n  {}\n\nInstall them with:\n  rustup target add {}",
                missing.join("\n  "),
                missing.join(" ")
            )))
        }
    }

    fn rust_target_is_installed(
        rustc: &Path,
        target: &str,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<bool, IosError> {
        let mut command = Command::new(rustc);
        command
            .args(["--print", "target-libdir", "--target", target])
            .stdin(Stdio::null());
        let result = run_process_capture(
            &mut command,
            &format!("Rust target query for {target}"),
            cancellation,
        )?;
        if result.status.code().is_none_or(|code| code >= 128) {
            return Err(IosError::worker(format!(
                "rustc terminated abnormally while checking target {target} ({})",
                result.status
            )));
        }
        if !result.status.success() {
            return Ok(false);
        }
        let target_libdir = String::from_utf8(result.stdout).map_err(|error| {
            IosError::compatibility(format!(
                "rustc returned a non-UTF-8 target directory for {target}: {error}"
            ))
        })?;
        Ok(Path::new(target_libdir.trim()).is_dir())
    }

    fn resolve_cargo_target_directory(root: &Path) -> Result<PathBuf, IosError> {
        let configured = env::var_os("CARGO_TARGET_DIR").filter(|value| !value.is_empty());
        let path = configured
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("target"));
        if path.is_absolute() {
            Ok(path)
        } else {
            env::current_dir()
                .map(|directory| directory.join(path))
                .map_err(|error| {
                    IosError::storage(format!(
                        "failed to resolve the relative Cargo target directory: {error}"
                    ))
                })
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the FFI archive build boundary keeps platform, toolchain, staging, diagnostics, and cancellation state explicit"
    )]
    fn build_platform_archive(
        root: &Path,
        platform: ApplePlatform,
        profile: IosFfiProfile,
        cargo_target_directory: &Path,
        staging: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let artifact = run_cargo_build(
            root,
            platform.rust_target(),
            profile,
            cargo_target_directory,
            &tools.cargo,
            diagnostics,
            cancellation,
        )?;
        let platform_directory = staging.join(platform.platform_name());
        fs::create_dir(&platform_directory).map_err(|error| {
            IosError::storage(format!(
                "failed to create staged {} FFI directory '{}': {error}",
                platform.platform_name(),
                platform_directory.display()
            ))
        })?;
        let destination = platform_directory.join(STATIC_LIBRARY_NAME);
        copy_static_library(&artifact, &destination, cancellation)?;
        if profile == IosFfiProfile::Release {
            let mut strip = Command::new(&tools.xcrun);
            strip.args(["strip", "-S", "-x"]).arg(&destination);
            run_required_command(
                strip,
                &format!("strip {} iOS FFI archive", platform.platform_name()),
                diagnostics,
                cancellation,
            )?;
        }
        let mut ranlib = Command::new(&tools.xcrun);
        ranlib.arg("ranlib").arg(&destination);
        run_required_command(
            ranlib,
            &format!("index {} iOS FFI archive", platform.platform_name()),
            diagnostics,
            cancellation,
        )?;
        validate_static_library(&destination)
    }

    fn run_cargo_build(
        root: &Path,
        target: &str,
        profile: IosFfiProfile,
        cargo_target_directory: &Path,
        cargo: &Path,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<PathBuf, IosError> {
        let mut command = Command::new(cargo);
        command
            .current_dir(root)
            .args(["build", "--locked", "--manifest-path"])
            .arg(root.join("Cargo.toml"))
            .args(["--target", target, "--target-dir"])
            .arg(cargo_target_directory)
            .args([
                "-p",
                "player-ffi-ios",
                "--message-format=json-render-diagnostics",
            ])
            .env_remove("CARGO_TARGET_DIR")
            .stdin(Stdio::null());
        if profile == IosFfiProfile::Release {
            command.arg("--release");
        }
        let label = format!("player-ffi-ios {} build", profile.as_str());
        let result = run_process_capture(&mut command, &label, cancellation)?;
        if result.status.code().is_none_or(|code| code >= 128) {
            diagnostics
                .write_all(&result.stderr)
                .map_err(diagnostics_error)?;
            diagnostics.flush().map_err(diagnostics_error)?;
            return Err(IosError::worker(format!(
                "{label} terminated abnormally ({})",
                result.status
            )));
        }
        let (artifact, rendered) = parse_cargo_output(&result.stdout, cancellation)?;
        diagnostics
            .write_all(&rendered)
            .map_err(diagnostics_error)?;
        diagnostics
            .write_all(&result.stderr)
            .map_err(diagnostics_error)?;
        diagnostics.flush().map_err(diagnostics_error)?;
        if !result.status.success() {
            return Err(IosError::conformance(format!(
                "{label} exited unsuccessfully ({})",
                result.status
            )));
        }
        let candidates = artifact.ok_or_else(|| {
            IosError::conformance(format!(
                "{label} did not report a {STATIC_LIBRARY_NAME} staticlib artifact"
            ))
        })?;
        if candidates.len() != 1 {
            return Err(IosError::conformance(format!(
                "{label} reported {} matching staticlib artifacts; expected exactly one",
                candidates.len()
            )));
        }
        candidates
            .into_iter()
            .next()
            .ok_or_else(|| IosError::worker("the validated Cargo artifact set became empty"))
    }

    fn parse_cargo_output(
        bytes: &[u8],
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(Option<BTreeSet<PathBuf>>, Vec<u8>), IosError> {
        let mut candidates = BTreeSet::new();
        let mut rendered = Vec::new();
        for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
            check_cancellation(cancellation, "Cargo output parsing")?;
            if line.is_empty() {
                continue;
            }
            if line.len() > MAX_CARGO_MESSAGE_BYTES {
                return Err(IosError::conformance(format!(
                    "Cargo JSON message {} exceeds the {} byte limit",
                    index + 1,
                    MAX_CARGO_MESSAGE_BYTES
                )));
            }
            let message: CargoMessage = serde_json::from_slice(line).map_err(|error| {
                IosError::conformance(format!(
                    "Cargo emitted malformed JSON on message {}: {error}",
                    index + 1
                ))
            })?;
            if let Some(diagnostic) = message.message
                && let Some(text) = diagnostic.rendered
            {
                rendered.extend_from_slice(text.as_bytes());
            }
            let Some(target) = message.target else {
                continue;
            };
            if message.reason != "compiler-artifact"
                || target.name != "vesper_player_ffi_ios"
                || (!target.kind.iter().any(|kind| kind == "staticlib")
                    && !target.crate_types.iter().any(|kind| kind == "staticlib"))
            {
                continue;
            }
            for filename in message.filenames {
                if filename.file_name() == Some(OsStr::new(STATIC_LIBRARY_NAME)) {
                    if !filename.is_absolute() {
                        return Err(IosError::conformance(format!(
                            "Cargo reported a relative iOS FFI artifact path: {}",
                            filename.display()
                        )));
                    }
                    candidates.insert(filename);
                }
            }
        }
        Ok(((!candidates.is_empty()).then_some(candidates), rendered))
    }

    fn copy_static_library(
        source: &Path,
        destination: &Path,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        validate_static_library(source)?;
        let expected = fs::symlink_metadata(source)
            .map_err(|error| {
                IosError::conformance(format!(
                    "failed to inspect Cargo iOS FFI artifact '{}': {error}",
                    source.display()
                ))
            })?
            .len();
        let input = File::open(source).map_err(|error| {
            IosError::conformance(format!(
                "failed to open Cargo iOS FFI artifact '{}': {error}",
                source.display()
            ))
        })?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create staged iOS FFI archive '{}': {error}",
                    destination.display()
                ))
            })?;
        let mut input = input.take(MAX_STATIC_LIBRARY_BYTES + 1);
        let mut copied = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            check_cancellation(cancellation, "iOS FFI archive copy")?;
            let count = input.read(&mut buffer).map_err(|error| {
                IosError::storage(format!(
                    "failed to read Cargo iOS FFI artifact '{}': {error}",
                    source.display()
                ))
            })?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count]).map_err(|error| {
                IosError::storage(format!(
                    "failed to copy iOS FFI archive into '{}': {error}",
                    destination.display()
                ))
            })?;
            copied = copied.checked_add(count as u64).ok_or_else(|| {
                IosError::conformance("iOS FFI archive copy byte count overflowed")
            })?;
        }
        if copied != expected || copied > MAX_STATIC_LIBRARY_BYTES {
            return Err(IosError::conformance(format!(
                "Cargo iOS FFI artifact '{}' changed while it was copied or exceeds {} bytes",
                source.display(),
                MAX_STATIC_LIBRARY_BYTES
            )));
        }
        output.sync_all().map_err(|error| {
            IosError::storage(format!(
                "failed to sync staged iOS FFI archive '{}': {error}",
                destination.display()
            ))
        })
    }

    fn validate_static_library(path: &Path) -> Result<(), IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::conformance(format!(
                "iOS FFI static library '{}' is unavailable: {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_STATIC_LIBRARY_BYTES
        {
            return Err(IosError::conformance(format!(
                "iOS FFI static library '{}' must be a non-empty regular non-symlink file no larger than {} bytes",
                path.display(),
                MAX_STATIC_LIBRARY_BYTES
            )));
        }
        Ok(())
    }

    fn create_xcframework(
        staging: &Path,
        headers: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let xcodebuild = tools.xcodebuild.as_ref().ok_or_else(|| {
            IosError::worker("full iOS FFI build is missing its preflighted xcodebuild path")
        })?;
        let output = staging.join("VesperPlayerFFI.xcframework");
        let mut command = Command::new(xcodebuild);
        command
            .arg("-create-xcframework")
            .arg("-library")
            .arg(staging.join("iphoneos").join(STATIC_LIBRARY_NAME))
            .arg("-headers")
            .arg(headers)
            .arg("-library")
            .arg(staging.join("iphonesimulator").join(STATIC_LIBRARY_NAME))
            .arg("-headers")
            .arg(headers)
            .arg("-output")
            .arg(&output)
            .stdin(Stdio::null());
        run_required_command(
            command,
            "create iOS FFI XCFramework",
            diagnostics,
            cancellation,
        )?;
        validate_generated_tree(&output, "iOS FFI XCFramework", cancellation)?;
        let expected_entries = BTreeSet::from([
            OsString::from("Info.plist"),
            OsString::from("ios-arm64"),
            OsString::from("ios-arm64-simulator"),
        ]);
        let entries =
            read_directory_names(&output, "iOS FFI XCFramework root", expected_entries.len())?;
        if entries != expected_entries {
            return Err(IosError::conformance(format!(
                "iOS FFI XCFramework '{}' has an unexpected root artifact set",
                output.display()
            )));
        }
        let manifest = output.join("Info.plist");
        validate_required_file(
            &manifest,
            "iOS FFI XCFramework manifest",
            MAX_XCFRAMEWORK_MANIFEST_BYTES,
        )?;
        validate_xcframework_manifest(&tools.xcrun, &output, &manifest, diagnostics, cancellation)
    }

    fn validate_xcframework_manifest(
        xcrun: &Path,
        xcframework: &Path,
        manifest_path: &Path,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let mut command = Command::new(xcrun);
        command
            .args(["plutil", "-convert", "json", "-o", "-"])
            .arg(manifest_path)
            .stdin(Stdio::null());
        let bytes = capture_required_stdout(
            &mut command,
            "iOS FFI XCFramework manifest parsing",
            MAX_XCFRAMEWORK_MANIFEST_BYTES as usize,
            diagnostics,
            cancellation,
        )?;
        let manifest: XcframeworkManifest = serde_json::from_slice(&bytes).map_err(|error| {
            IosError::conformance(format!(
                "iOS FFI XCFramework manifest '{}' is invalid: {error}",
                manifest_path.display()
            ))
        })?;
        if manifest.cf_bundle_package_type != "XFWK" || manifest.format_version != "1.0" {
            return Err(IosError::conformance(format!(
                "iOS FFI XCFramework manifest '{}' has an unsupported package type or format version",
                manifest_path.display()
            )));
        }
        if manifest.available_libraries.len() != 2 {
            return Err(IosError::conformance(format!(
                "iOS FFI XCFramework manifest '{}' must declare exactly two libraries",
                manifest_path.display()
            )));
        }

        let mut identifiers = BTreeSet::new();
        for library in manifest.available_libraries {
            let expected_variant = match library.library_identifier.as_str() {
                "ios-arm64" => None,
                "ios-arm64-simulator" => Some("simulator"),
                identifier => {
                    return Err(IosError::conformance(format!(
                        "iOS FFI XCFramework declares unsupported library identifier '{identifier}'"
                    )));
                }
            };
            if !identifiers.insert(library.library_identifier.clone()) {
                return Err(IosError::conformance(format!(
                    "iOS FFI XCFramework declares duplicate library identifier '{}'",
                    library.library_identifier
                )));
            }
            if library.binary_path != STATIC_LIBRARY_NAME
                || library.headers_path != "Headers"
                || library.library_path != STATIC_LIBRARY_NAME
                || library.supported_architectures != ["arm64"]
                || library.supported_platform != "ios"
                || library.supported_platform_variant.as_deref() != expected_variant
            {
                return Err(IosError::conformance(format!(
                    "iOS FFI XCFramework library '{}' violates the arm64 iOS slice contract",
                    library.library_identifier
                )));
            }
            let slice = xcframework.join(&library.library_identifier);
            let headers = slice.join(&library.headers_path);
            validate_required_directory(&headers, "iOS FFI XCFramework headers directory")?;
            let expected_headers = BTreeSet::from([OsString::from(PUBLIC_HEADER_NAME)]);
            let header_entries = read_directory_names(
                &headers,
                "iOS FFI XCFramework headers directory",
                expected_headers.len(),
            )?;
            if header_entries != expected_headers {
                return Err(IosError::conformance(format!(
                    "iOS FFI XCFramework headers directory '{}' has an unexpected header set",
                    headers.display()
                )));
            }
            validate_required_file(
                &headers.join(PUBLIC_HEADER_NAME),
                "iOS FFI XCFramework public header",
                MAX_PUBLIC_HEADER_BYTES,
            )?;
            let binary = slice.join(&library.binary_path);
            validate_static_library(&binary)?;
            validate_arm64_binary(xcrun, &binary, diagnostics, cancellation)?;
        }
        let expected = BTreeSet::from(["ios-arm64".to_owned(), "ios-arm64-simulator".to_owned()]);
        if identifiers != expected {
            return Err(IosError::conformance(format!(
                "iOS FFI XCFramework manifest '{}' has an incomplete library set",
                manifest_path.display()
            )));
        }
        Ok(())
    }

    fn validate_arm64_binary(
        xcrun: &Path,
        binary: &Path,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let mut command = Command::new(xcrun);
        command
            .args(["lipo", "-archs"])
            .arg(binary)
            .stdin(Stdio::null());
        let output = capture_required_stdout(
            &mut command,
            "iOS FFI binary architecture inspection",
            MAX_LIPO_OUTPUT_BYTES,
            diagnostics,
            cancellation,
        )?;
        let output = std::str::from_utf8(&output).map_err(|error| {
            IosError::conformance(format!(
                "binary architecture output for '{}' is not UTF-8: {error}",
                binary.display()
            ))
        })?;
        let architectures = output.split_ascii_whitespace().collect::<BTreeSet<_>>();
        if architectures != BTreeSet::from(["arm64"]) {
            return Err(IosError::conformance(format!(
                "iOS FFI binary '{}' must contain only arm64; found: {}",
                binary.display(),
                output.trim()
            )));
        }
        Ok(())
    }

    fn capture_required_stdout(
        command: &mut Command,
        label: &str,
        maximum_stdout_bytes: usize,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<Vec<u8>, IosError> {
        let result = external_process::run_interruptible_capture_in_deferral(
            command,
            label,
            maximum_stdout_bytes,
            MAX_PROCESS_OUTPUT_BYTES,
            cancellation,
        )
        .map_err(map_external_process_error)?;
        diagnostics
            .write_all(&result.stderr)
            .map_err(diagnostics_error)?;
        if !result.status.success() {
            diagnostics
                .write_all(&result.stdout)
                .map_err(diagnostics_error)?;
        }
        diagnostics.flush().map_err(diagnostics_error)?;
        classify_process_status(result.status, label)?;
        Ok(result.stdout)
    }

    fn validate_required_file(path: &Path, label: &str, maximum: u64) -> Result<(), IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::conformance(format!(
                "{label} '{}' is unavailable: {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > maximum {
            return Err(IosError::conformance(format!(
                "{label} '{}' must be a non-empty regular non-symlink file no larger than {maximum} bytes",
                path.display()
            )));
        }
        Ok(())
    }

    fn validate_required_directory(path: &Path, label: &str) -> Result<(), IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::conformance(format!(
                "{label} '{}' is unavailable: {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(IosError::conformance(format!(
                "{label} '{}' must be a regular non-symlink directory",
                path.display()
            )));
        }
        Ok(())
    }

    fn run_required_command(
        mut command: Command,
        label: &str,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let result = run_process_capture(&mut command, label, cancellation)?;
        diagnostics
            .write_all(&result.stdout)
            .map_err(diagnostics_error)?;
        diagnostics
            .write_all(&result.stderr)
            .map_err(diagnostics_error)?;
        diagnostics.flush().map_err(diagnostics_error)?;
        classify_process_status(result.status, label)
    }

    fn classify_process_status(status: ExitStatus, label: &str) -> Result<(), IosError> {
        if status.success() {
            Ok(())
        } else if status.code().is_none_or(|code| code >= 128) {
            Err(IosError::worker(format!(
                "{label} terminated abnormally ({status})"
            )))
        } else {
            Err(IosError::conformance(format!(
                "{label} exited unsuccessfully ({status})"
            )))
        }
    }

    fn run_process_capture(
        command: &mut Command,
        label: &str,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<BoundedProcessOutput, IosError> {
        let result = external_process::run_interruptible_capture_in_deferral(
            command,
            label,
            MAX_PROCESS_OUTPUT_BYTES,
            MAX_PROCESS_OUTPUT_BYTES,
            cancellation,
        );
        result.map_err(map_external_process_error)
    }

    fn validate_full_staging_output(staging: &Path) -> Result<(), IosError> {
        let expected = BTreeSet::from([
            OsString::from("VesperPlayerFFI.xcframework"),
            OsString::from("iphoneos"),
            OsString::from("iphonesimulator"),
        ]);
        let entries =
            read_directory_names(staging, "full iOS FFI staging directory", expected.len())?;
        if entries != expected {
            return Err(IosError::conformance(format!(
                "full iOS FFI staging directory '{}' has an unexpected artifact set",
                staging.display()
            )));
        }
        for platform in [ApplePlatform::Device, ApplePlatform::Simulator] {
            let directory = staging.join(platform.platform_name());
            let platform_entries =
                read_directory_names(&directory, "iOS FFI platform directory", 1)?;
            if platform_entries != BTreeSet::from([OsString::from(STATIC_LIBRARY_NAME)]) {
                return Err(IosError::conformance(format!(
                    "iOS FFI platform directory '{}' has an unexpected artifact set",
                    directory.display()
                )));
            }
            validate_static_library(&directory.join(STATIC_LIBRARY_NAME))?;
        }
        Ok(())
    }

    fn read_directory_names(
        path: &Path,
        label: &str,
        maximum_entries: usize,
    ) -> Result<BTreeSet<OsString>, IosError> {
        let mut names = BTreeSet::new();
        let entries = fs::read_dir(path).map_err(|error| {
            IosError::conformance(format!(
                "failed to read {label} '{}': {error}",
                path.display()
            ))
        })?;
        for entry in entries {
            if names.len() >= maximum_entries {
                return Err(IosError::conformance(format!(
                    "{label} '{}' contains more than {maximum_entries} entries",
                    path.display()
                )));
            }
            let entry = entry.map_err(|error| {
                IosError::conformance(format!(
                    "failed to inspect an entry in {label} '{}': {error}",
                    path.display()
                ))
            })?;
            names.insert(entry.file_name());
        }
        Ok(names)
    }

    fn validate_generated_tree(
        path: &Path,
        label: &str,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        validate_generated_tree_with_limits(
            path,
            label,
            cancellation,
            MAX_GENERATED_TREE_ENTRIES,
            MAX_GENERATED_TREE_DEPTH,
        )
    }

    fn validate_generated_tree_with_limits(
        path: &Path,
        label: &str,
        cancellation: &external_process::InterruptDeferral,
        maximum_entries: usize,
        maximum_depth: usize,
    ) -> Result<(), IosError> {
        if maximum_entries == 0 {
            return Err(IosError::conformance(format!(
                "{label} entry limit must include the root directory"
            )));
        }
        let mut pending = vec![(path.to_path_buf(), 0_usize)];
        let mut entries = 1_usize;
        while let Some((current, depth)) = pending.pop() {
            check_cancellation(cancellation, label)?;
            if depth > maximum_depth {
                return Err(IosError::conformance(format!(
                    "{label} '{}' exceeds the maximum tree depth of {maximum_depth}",
                    path.display()
                )));
            }
            let metadata = fs::symlink_metadata(&current).map_err(|error| {
                IosError::conformance(format!(
                    "failed to inspect {label} entry '{}': {error}",
                    current.display()
                ))
            })?;
            if metadata.file_type().is_symlink()
                || (!metadata.file_type().is_dir() && !metadata.file_type().is_file())
            {
                return Err(IosError::conformance(format!(
                    "{label} entry '{}' is not a regular file or directory",
                    current.display()
                )));
            }
            if metadata.file_type().is_dir() {
                let child_depth = depth.checked_add(1).ok_or_else(|| {
                    IosError::conformance(format!("{label} tree depth overflowed"))
                })?;
                for entry in fs::read_dir(&current).map_err(|error| {
                    IosError::conformance(format!(
                        "failed to read {label} directory '{}': {error}",
                        current.display()
                    ))
                })? {
                    if entries >= maximum_entries {
                        return Err(IosError::conformance(format!(
                            "{label} '{}' exceeds the maximum entry count of {maximum_entries}",
                            path.display()
                        )));
                    }
                    if child_depth > maximum_depth {
                        return Err(IosError::conformance(format!(
                            "{label} '{}' exceeds the maximum tree depth of {maximum_depth}",
                            path.display()
                        )));
                    }
                    let entry = entry.map_err(|error| {
                        IosError::conformance(format!(
                            "failed to inspect {label} directory '{}': {error}",
                            current.display()
                        ))
                    })?;
                    entries += 1;
                    pending.push((entry.path(), child_depth));
                }
            }
        }
        Ok(())
    }

    impl OutputPlan {
        fn preflight(root: &Path, mode: BuildMode) -> Result<Self, IosError> {
            let artifacts_parent = require_repository_directory(
                root,
                Path::new("lib/ios/VesperPlayerKit/Artifacts"),
                "iOS FFI artifacts parent",
            )?;
            let distribution_root = artifacts_parent.join("rust-player-ffi");
            let distribution_identity =
                optional_directory_identity(&distribution_root, "iOS FFI distribution directory")?;
            let (target_path, source_is_staging_root, platform) = match mode {
                BuildMode::Full => (distribution_root.clone(), true, None),
                BuildMode::Platform(platform) if distribution_identity.is_none() => {
                    (distribution_root.clone(), true, Some(platform))
                }
                BuildMode::Platform(platform) => (
                    distribution_root.join(platform.platform_name()),
                    false,
                    Some(platform),
                ),
            };
            let target = OutputTarget::preflight(target_path)?;
            Ok(Self {
                artifacts_parent,
                distribution_root,
                target,
                source_is_staging_root,
                platform,
            })
        }
    }

    impl OutputTarget {
        fn preflight(path: PathBuf) -> Result<Self, IosError> {
            let parent = path.parent().ok_or_else(|| {
                IosError::storage(format!(
                    "iOS FFI output '{}' has no parent directory",
                    path.display()
                ))
            })?;
            let parent_metadata = fs::symlink_metadata(parent).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect iOS FFI output parent '{}': {error}",
                    parent.display()
                ))
            })?;
            if !parent_metadata.file_type().is_dir() {
                return Err(IosError::compatibility(format!(
                    "iOS FFI output parent '{}' must be a regular non-symlink directory",
                    parent.display()
                )));
            }
            Ok(Self {
                path: path.clone(),
                parent: parent.to_path_buf(),
                parent_identity: file_identity(&parent_metadata),
                initial_identity: optional_directory_identity(&path, "iOS FFI output")?,
            })
        }

        fn revalidate(&self) -> Result<bool, IosError> {
            self.revalidate_parent()?;
            self.revalidate_target()
        }

        fn revalidate_parent(&self) -> Result<(), IosError> {
            let parent_metadata = fs::symlink_metadata(&self.parent).map_err(|error| {
                IosError::storage(format!(
                    "failed to recheck iOS FFI output parent '{}': {error}",
                    self.parent.display()
                ))
            })?;
            if !parent_metadata.file_type().is_dir()
                || file_identity(&parent_metadata) != self.parent_identity
            {
                return Err(IosError::compatibility(format!(
                    "iOS FFI output parent '{}' changed after validation",
                    self.parent.display()
                )));
            }
            Ok(())
        }

        fn revalidate_target(&self) -> Result<bool, IosError> {
            let current = optional_directory_identity(&self.path, "iOS FFI output")?;
            if current != self.initial_identity {
                return Err(IosError::compatibility(format!(
                    "iOS FFI output '{}' changed after validation",
                    self.path.display()
                )));
            }
            Ok(current.is_some())
        }
    }

    fn promote_staged_output(
        staging: tempfile::TempDir,
        source: &Path,
        target: &OutputTarget,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<Vec<String>, IosError> {
        promote_staged_output_with_hooks(
            staging,
            source,
            target,
            cancellation,
            None,
            None,
            None,
            None,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the promotion helper exposes explicit failure-injection hooks for each transaction phase"
    )]
    fn promote_staged_output_with_hooks(
        staging: tempfile::TempDir,
        source: &Path,
        target: &OutputTarget,
        cancellation: &external_process::InterruptDeferral,
        mut before_exchange: Option<crate::PathIoHook<'_>>,
        mut after_publish: Option<crate::PathIoHook<'_>>,
        mut after_commit: Option<crate::PathHook<'_>>,
        mut cleanup_staging: Option<crate::TempDirIoHook<'_>>,
    ) -> Result<Vec<String>, IosError> {
        let source = source.to_path_buf();
        let source_identity = directory_identity(&source, "staged iOS FFI output")?;
        let staging_parent = staging
            .path()
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| IosError::storage("iOS FFI staging directory has no parent"))?;
        if cancellation.is_cancelled() {
            return Err(IosError::worker("iOS FFI output promotion was cancelled"));
        }
        let had_previous = target.revalidate()?;
        if let Some(hook) = before_exchange.as_mut() {
            hook(&target.path).map_err(|error| IosError::storage(error.to_string()))?;
        }
        target.revalidate()?;
        check_cancellation(cancellation, "iOS FFI output promotion")?;
        if had_previous {
            exchange_paths(&source, &target.path).map_err(|error| {
                IosError::storage(format!(
                    "failed to atomically exchange iOS FFI output '{}': {error}",
                    target.path.display()
                ))
            })?;
            let previous_identity = directory_identity(&source, "previous iOS FFI output").ok();
            let promoted_identity =
                directory_identity(&target.path, "promoted iOS FFI output").ok();
            if previous_identity != target.initial_identity
                || promoted_identity != Some(source_identity)
            {
                return Err(rollback_or_preserve(
                    staging,
                    &source,
                    target,
                    true,
                    source_identity,
                    IosError::compatibility(format!(
                        "iOS FFI output '{}' changed during atomic promotion",
                        target.path.display()
                    )),
                ));
            }
        } else {
            rename_noreplace(&source, &target.path).map_err(|error| {
                IosError::storage(format!(
                    "failed to atomically publish iOS FFI output '{}': {error}",
                    target.path.display()
                ))
            })?;
        }
        if let Some(hook) = after_publish.as_mut()
            && let Err(hook_error) = hook(&target.path)
        {
            return Err(rollback_or_preserve(
                staging,
                &source,
                target,
                had_previous,
                source_identity,
                IosError::storage(format!(
                    "post-publication iOS FFI test hook failed: {hook_error}"
                )),
            ));
        }
        if let Err(error) = sync_directory(&target.parent)
            .and_then(|()| target.revalidate_parent())
            .and_then(|()| {
                let promoted = directory_identity(&target.path, "promoted iOS FFI output")?;
                if promoted == source_identity {
                    Ok(())
                } else {
                    Err(IosError::compatibility(format!(
                        "iOS FFI output '{}' changed during promotion",
                        target.path.display()
                    )))
                }
            })
        {
            return Err(rollback_or_preserve(
                staging,
                &source,
                target,
                had_previous,
                source_identity,
                error,
            ));
        }
        if let Some(hook) = after_commit.as_mut() {
            hook(&target.path);
        }
        let staging_path = staging.path().to_path_buf();
        let mut warnings = Vec::new();
        let close_result = if let Some(cleanup) = cleanup_staging.as_mut() {
            cleanup(staging)
        } else {
            staging.close()
        };
        if let Err(error) = close_result
            && error.kind() != io::ErrorKind::NotFound
        {
            warnings.push(format!(
                "iOS FFI staging cleanup failed for '{}': {error}; retry cleanup before the next build",
                staging_path.display()
            ));
        }
        if let Err(error) = sync_directory(&staging_parent) {
            warnings.push(format!(
                "iOS FFI staging parent sync failed for '{}': {error}",
                staging_parent.display()
            ));
        }
        Ok(warnings)
    }

    fn rollback_or_preserve(
        staging: tempfile::TempDir,
        source: &Path,
        target: &OutputTarget,
        had_previous: bool,
        source_identity: FileIdentity,
        error: IosError,
    ) -> IosError {
        let current_target = match optional_directory_identity(
            &target.path,
            "published iOS FFI output during rollback",
        ) {
            Ok(identity) => identity,
            Err(identity_error) => {
                return preserve_staging_quarantine(
                    staging,
                    error,
                    format!("published output ownership could not be verified: {identity_error}"),
                );
            }
        };
        if current_target != Some(source_identity) {
            return preserve_staging_quarantine(
                staging,
                error,
                "published output ownership changed before rollback",
            );
        }
        if had_previous {
            let current_previous = match optional_directory_identity(
                source,
                "previous iOS FFI output during rollback",
            ) {
                Ok(identity) => identity,
                Err(identity_error) => {
                    return preserve_staging_quarantine(
                        staging,
                        error,
                        format!(
                            "previous output ownership could not be verified: {identity_error}"
                        ),
                    );
                }
            };
            if current_previous != target.initial_identity {
                return preserve_staging_quarantine(
                    staging,
                    error,
                    "previous output ownership changed before rollback",
                );
            }
        }
        let rollback = if had_previous {
            exchange_paths(source, &target.path)
        } else {
            rename_noreplace(&target.path, source)
        };
        match rollback {
            Ok(()) => match staging.close() {
                Ok(()) => error,
                Err(cleanup) => append_error(error, format!("rollback cleanup failed: {cleanup}")),
            },
            Err(rollback_error) => preserve_staging_quarantine(
                staging,
                error,
                format!("rollback failed: {rollback_error}"),
            ),
        }
    }

    fn preserve_staging_quarantine(
        staging: tempfile::TempDir,
        error: IosError,
        reason: impl std::fmt::Display,
    ) -> IosError {
        let preserved = staging.keep();
        append_error(
            error,
            format!(
                "rollback skipped: {reason}; staging quarantine was preserved under '{}'",
                preserved.display()
            ),
        )
    }

    fn require_repository_directory(
        root: &Path,
        relative: &Path,
        label: &str,
    ) -> Result<PathBuf, IosError> {
        let mut current = root.to_path_buf();
        for component in relative.components() {
            let Component::Normal(component) = component else {
                return Err(IosError::compatibility(format!(
                    "{label} must use a relative normalized repository path"
                )));
            };
            current.push(component);
            let metadata = fs::symlink_metadata(&current).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect {label} '{}': {error}",
                    current.display()
                ))
            })?;
            if !metadata.file_type().is_dir() {
                return Err(IosError::compatibility(format!(
                    "{label} '{}' must be a regular non-symlink directory",
                    current.display()
                )));
            }
        }
        Ok(current)
    }

    fn optional_directory_identity(
        path: &Path,
        label: &str,
    ) -> Result<Option<FileIdentity>, IosError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_dir() => Ok(Some(file_identity(&metadata))),
            Ok(_) => Err(IosError::compatibility(format!(
                "{label} '{}' must be a regular non-symlink directory",
                path.display()
            ))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))),
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
            return Err(IosError::conformance(format!(
                "{label} '{}' must be a regular non-symlink directory",
                path.display()
            )));
        }
        Ok(file_identity(&metadata))
    }

    fn file_identity(metadata: &fs::Metadata) -> FileIdentity {
        use std::os::unix::fs::MetadataExt;

        FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }

    fn exchange_paths(left: &Path, right: &Path) -> io::Result<()> {
        use rustix::fs::{CWD, RenameFlags, renameat_with};

        renameat_with(CWD, left, CWD, right, RenameFlags::EXCHANGE).map_err(io::Error::from)
    }

    fn rename_noreplace(source: &Path, target: &Path) -> io::Result<()> {
        use rustix::fs::{CWD, RenameFlags, renameat_with};

        renameat_with(CWD, source, CWD, target, RenameFlags::NOREPLACE).map_err(io::Error::from)
    }

    fn sync_directory(path: &Path) -> Result<(), IosError> {
        File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to sync iOS FFI directory '{}': {error}",
                    path.display()
                ))
            })
    }

    impl IosFfiBuildLock {
        pub(super) fn acquire(root: &Path) -> Result<Self, IosError> {
            use std::os::unix::fs::OpenOptionsExt;

            let metadata = fs::symlink_metadata(root).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect iOS FFI lock repository root '{}': {error}",
                    root.display()
                ))
            })?;
            if !metadata.file_type().is_dir() {
                return Err(IosError::compatibility(format!(
                    "iOS FFI lock repository root '{}' must be a regular non-symlink directory",
                    root.display()
                )));
            }
            let path = root.join(".vesper-ios-ffi.lock");
            let mut create = OpenOptions::new();
            create
                .create_new(true)
                .read(true)
                .write(true)
                .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW);
            let file = match create.open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let mut open = OpenOptions::new();
                    open.read(true)
                        .write(true)
                        .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW);
                    open.open(&path).map_err(|open_error| {
                        IosError::compatibility(format!(
                            "failed to open regular non-symlink iOS FFI build lock '{}': {open_error}",
                            path.display()
                        ))
                    })?
                }
                Err(error) => {
                    return Err(IosError::storage(format!(
                        "failed to create iOS FFI build lock '{}': {error}",
                        path.display()
                    )));
                }
            };
            let metadata = file.metadata().map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect opened iOS FFI build lock '{}': {error}",
                    path.display()
                ))
            })?;
            if !metadata.file_type().is_file() {
                return Err(IosError::compatibility(format!(
                    "iOS FFI build lock '{}' must be a regular non-symlink file",
                    path.display()
                )));
            }
            match file.try_lock() {
                Ok(()) => Ok(Self { _file: file }),
                Err(TryLockError::WouldBlock) => Err(IosError::compatibility(format!(
                    "another iOS FFI build is already active for '{}'",
                    root.display()
                ))),
                Err(TryLockError::Error(error)) => Err(IosError::storage(format!(
                    "failed to lock iOS FFI build for '{}': {error}",
                    root.display()
                ))),
            }
        }
    }

    fn append_error(error: IosError, suffix: impl std::fmt::Display) -> IosError {
        let message = format!("{error}; {suffix}");
        match error.kind() {
            crate::ios::IosErrorKind::Storage => IosError::storage(message),
            crate::ios::IosErrorKind::Compatibility => IosError::compatibility(message),
            crate::ios::IosErrorKind::Conformance => IosError::conformance(message),
            crate::ios::IosErrorKind::Worker => IosError::worker(message),
        }
    }

    fn diagnostics_error(error: io::Error) -> IosError {
        IosError::storage(format!("failed to write iOS FFI diagnostics: {error}"))
    }

    fn output_error(error: io::Error) -> IosError {
        IosError::storage(format!("failed to write iOS FFI output: {error}"))
    }

    fn check_cancellation(
        cancellation: &external_process::InterruptDeferral,
        label: &str,
    ) -> Result<(), IosError> {
        if cancellation.is_cancelled() {
            Err(IosError::worker(format!("{label} was cancelled")))
        } else {
            Ok(())
        }
    }

    fn map_external_process_error(
        error: crate::external_process::ExternalProcessError,
    ) -> IosError {
        match error.kind() {
            ExternalProcessErrorKind::Compatibility => IosError::compatibility(error.to_string()),
            ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
                IosError::worker(error.to_string())
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::{Mutex, MutexGuard};

        static PROMOTION_TEST_LOCK: Mutex<()> = Mutex::new(());

        fn promotion_test_guard() -> MutexGuard<'static, ()> {
            PROMOTION_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }

        #[test]
        fn promotion_rejects_a_concurrent_target_replacement_before_publish() {
            let _guard = promotion_test_guard();
            let directory = tempfile::tempdir().expect("temporary iOS FFI promotion boundary");
            let parent = directory.path().join("Artifacts");
            let output = parent.join("rust-player-ffi");
            fs::create_dir_all(&output).expect("create previous iOS FFI output");
            fs::write(output.join("previous.txt"), b"previous")
                .expect("write previous iOS FFI output");
            let target =
                OutputTarget::preflight(output.clone()).expect("preflight iOS FFI output target");
            let staging = tempfile::Builder::new()
                .prefix(".vesper-ios-ffi-stage-")
                .tempdir_in(&parent)
                .expect("create staged iOS FFI output");
            fs::write(staging.path().join("replacement.txt"), b"replacement")
                .expect("write staged iOS FFI output");
            let source = staging.path().to_path_buf();
            let displaced_original = parent.join("displaced-original");
            let mut replace_target = |path: &Path| {
                fs::rename(path, &displaced_original)?;
                fs::create_dir(path)?;
                fs::write(path.join("concurrent.txt"), b"concurrent")
            };
            let cancellation =
                external_process::InterruptDeferral::start("iOS FFI target replacement test")
                    .expect("start iOS FFI promotion cancellation scope");

            let error = promote_staged_output_with_hooks(
                staging,
                &source,
                &target,
                &cancellation,
                Some(&mut replace_target),
                None,
                None,
                None,
            )
            .expect_err("reject an iOS FFI output replaced during promotion");
            assert!(!cancellation.finish());

            assert!(error.to_string().contains("changed after validation"));
            assert_eq!(
                fs::read(output.join("concurrent.txt")).expect("read concurrent iOS FFI output"),
                b"concurrent"
            );
            assert_eq!(
                fs::read(displaced_original.join("previous.txt"))
                    .expect("read independently displaced iOS FFI output"),
                b"previous"
            );
            assert!(!source.exists());
        }

        #[test]
        fn promotion_rejects_a_replaced_parent_before_exchange() {
            let _guard = promotion_test_guard();
            let directory = tempfile::tempdir().expect("temporary iOS FFI promotion parent");
            let parent = directory.path().join("Artifacts");
            let output = parent.join("rust-player-ffi");
            fs::create_dir_all(&output).expect("create previous iOS FFI output");
            fs::write(output.join("previous.txt"), b"previous")
                .expect("write previous iOS FFI output");
            let target = OutputTarget::preflight(output).expect("preflight iOS FFI output target");
            let staging = tempfile::Builder::new()
                .prefix(".vesper-ios-ffi-stage-")
                .tempdir_in(&parent)
                .expect("create staged iOS FFI output");
            fs::write(staging.path().join("replacement.txt"), b"replacement")
                .expect("write staged iOS FFI output");
            let source = staging.path().to_path_buf();
            let moved_parent = directory.path().join("moved-artifacts");
            let mut replace_parent = |_path: &Path| {
                fs::rename(&parent, &moved_parent)?;
                fs::create_dir(&parent)
            };
            let cancellation =
                external_process::InterruptDeferral::start("iOS FFI parent replacement test")
                    .expect("start iOS FFI promotion cancellation scope");

            let error = promote_staged_output_with_hooks(
                staging,
                &source,
                &target,
                &cancellation,
                Some(&mut replace_parent),
                None,
                None,
                None,
            )
            .expect_err("reject a replaced iOS FFI output parent");
            assert!(!cancellation.finish());

            assert!(error.to_string().contains("changed after validation"));
            assert!(
                fs::read_dir(&parent)
                    .expect("inspect replacement iOS FFI output parent")
                    .next()
                    .is_none()
            );
            assert_eq!(
                fs::read(moved_parent.join("rust-player-ffi/previous.txt"))
                    .expect("read preserved iOS FFI output"),
                b"previous"
            );
            assert!(
                fs::read_dir(&moved_parent)
                    .expect("inspect moved iOS FFI output parent")
                    .filter_map(Result::ok)
                    .any(|entry| entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".vesper-ios-ffi-stage-"))
            );
        }

        #[test]
        fn promotion_cancels_immediately_before_publish() {
            use nix::sys::signal::{Signal, raise};

            let _guard = promotion_test_guard();
            let directory = tempfile::tempdir().expect("temporary cancelled iOS FFI promotion");
            let parent = directory.path().join("Artifacts");
            let output = parent.join("rust-player-ffi");
            fs::create_dir_all(&output).expect("create previous iOS FFI output");
            fs::write(output.join("previous.txt"), b"previous")
                .expect("write previous iOS FFI output");
            let target =
                OutputTarget::preflight(output.clone()).expect("preflight iOS FFI output target");
            let staging = tempfile::Builder::new()
                .prefix(".vesper-ios-ffi-stage-")
                .tempdir_in(&parent)
                .expect("create staged iOS FFI output");
            fs::write(staging.path().join("replacement.txt"), b"replacement")
                .expect("write staged iOS FFI output");
            let source = staging.path().to_path_buf();
            let cancellation =
                external_process::InterruptDeferral::start("iOS FFI pre-publish cancellation test")
                    .expect("start iOS FFI promotion cancellation scope");
            let mut cancel_before_publish = |_path: &Path| {
                raise(Signal::SIGINT).expect("cancel iOS FFI promotion before publish");
                Ok(())
            };

            let error = promote_staged_output_with_hooks(
                staging,
                &source,
                &target,
                &cancellation,
                Some(&mut cancel_before_publish),
                None,
                None,
                None,
            )
            .expect_err("cancel iOS FFI output before publishing it");
            assert!(cancellation.finish());

            assert_eq!(error.kind(), crate::ios::IosErrorKind::Worker);
            assert!(error.to_string().contains("output promotion was cancelled"));
            assert_eq!(
                fs::read(output.join("previous.txt")).expect("read previous iOS FFI output"),
                b"previous"
            );
            assert!(!output.join("replacement.txt").exists());
            assert!(!source.exists());
        }

        #[test]
        fn rollback_preserves_a_concurrent_post_publish_target() {
            let _guard = promotion_test_guard();
            let directory =
                tempfile::tempdir().expect("temporary post-publish iOS FFI replacement");
            let parent = directory.path().join("Artifacts");
            let output = parent.join("rust-player-ffi");
            fs::create_dir_all(&output).expect("create previous iOS FFI output");
            fs::write(output.join("previous.txt"), b"previous")
                .expect("write previous iOS FFI output");
            let target =
                OutputTarget::preflight(output.clone()).expect("preflight iOS FFI output target");
            let staging = tempfile::Builder::new()
                .prefix(".vesper-ios-ffi-stage-")
                .tempdir_in(&parent)
                .expect("create staged iOS FFI output");
            fs::write(staging.path().join("replacement.txt"), b"replacement")
                .expect("write staged iOS FFI output");
            let source = staging.path().to_path_buf();
            let displaced_published = parent.join("displaced-published");
            let mut replace_after_publish = |path: &Path| {
                fs::rename(path, &displaced_published)?;
                fs::create_dir(path)?;
                fs::write(path.join("concurrent.txt"), b"concurrent")
            };
            let cancellation = external_process::InterruptDeferral::start(
                "iOS FFI post-publish target replacement test",
            )
            .expect("start iOS FFI promotion cancellation scope");

            let error = promote_staged_output_with_hooks(
                staging,
                &source,
                &target,
                &cancellation,
                None,
                Some(&mut replace_after_publish),
                None,
                None,
            )
            .expect_err("reject an iOS FFI output replaced after publication");
            assert!(!cancellation.finish());

            assert!(error.to_string().contains("changed during promotion"));
            assert!(error.to_string().contains("rollback skipped"));
            assert!(error.to_string().contains(&source.display().to_string()));
            assert_eq!(
                fs::read(output.join("concurrent.txt"))
                    .expect("read untouched concurrent iOS FFI output"),
                b"concurrent"
            );
            assert_eq!(
                fs::read(source.join("previous.txt"))
                    .expect("read quarantined previous iOS FFI output"),
                b"previous"
            );
            assert_eq!(
                fs::read(displaced_published.join("replacement.txt"))
                    .expect("read independently displaced published iOS FFI output"),
                b"replacement"
            );
        }

        #[test]
        fn promotion_reports_cancellation_after_commit_without_rollback() {
            use nix::sys::signal::{Signal, raise};

            let _guard = promotion_test_guard();
            let directory = tempfile::tempdir().expect("temporary committed iOS FFI promotion");
            let parent = directory.path().join("Artifacts");
            let output = parent.join("rust-player-ffi");
            fs::create_dir_all(&output).expect("create previous iOS FFI output");
            fs::write(output.join("previous.txt"), b"previous")
                .expect("write previous iOS FFI output");
            let target =
                OutputTarget::preflight(output.clone()).expect("preflight iOS FFI output target");
            let staging = tempfile::Builder::new()
                .prefix(".vesper-ios-ffi-stage-")
                .tempdir_in(&parent)
                .expect("create staged iOS FFI output");
            fs::write(staging.path().join("replacement.txt"), b"replacement")
                .expect("write staged iOS FFI output");
            let source = staging.path().to_path_buf();
            let cancellation =
                external_process::InterruptDeferral::start("iOS FFI committed cancellation test")
                    .expect("start iOS FFI promotion cancellation scope");
            let mut cancel_after_commit = |_path: &Path| {
                raise(Signal::SIGINT).expect("cancel iOS FFI promotion after commit");
            };

            let error = promote_staged_output_with_hooks(
                staging,
                &source,
                &target,
                &cancellation,
                None,
                None,
                Some(&mut cancel_after_commit),
                None,
            )
            .expect("report cancellation after committing iOS FFI output");
            assert!(cancellation.finish());

            assert!(error.is_empty());
            assert_eq!(
                fs::read(output.join("replacement.txt"))
                    .expect("read committed iOS FFI replacement"),
                b"replacement"
            );
            assert!(!output.join("previous.txt").exists());
            assert!(!source.exists());
        }

        #[test]
        fn committed_build_cancellation_returns_worker_error_without_stdout() {
            let committed = PathBuf::from("/committed/rust-player-ffi");
            let outcome = BuildOutcome {
                distribution_root: committed.clone(),
                warnings: vec!["cleanup warning".to_owned()],
            };
            let mut output = Vec::new();
            let mut diagnostics = Vec::new();

            let error = report_build_result(Ok(outcome), true, &mut output, &mut diagnostics)
                .expect_err("report cancellation after committing the iOS FFI output");

            assert_eq!(error.kind(), crate::ios::IosErrorKind::Worker);
            assert!(error.to_string().contains("cancelled"));
            assert!(error.to_string().contains("committed"));
            assert!(error.to_string().contains(&committed.display().to_string()));
            assert!(output.is_empty());
            assert_eq!(diagnostics, b"warning: cleanup warning\n");
        }

        #[test]
        fn generated_tree_rejects_width_before_pending_exceeds_the_limit() {
            let directory = tempfile::tempdir().expect("temporary wide iOS FFI tree");
            for index in 0..4 {
                fs::write(directory.path().join(format!("entry-{index}")), b"fixture")
                    .expect("write wide iOS FFI fixture entry");
            }
            let cancellation = external_process::InterruptDeferral::start("wide iOS FFI tree test")
                .expect("start wide iOS FFI tree cancellation scope");

            let error = validate_generated_tree_with_limits(
                directory.path(),
                "wide iOS FFI tree",
                &cancellation,
                4,
                MAX_GENERATED_TREE_DEPTH,
            )
            .expect_err("reject a tree wider than its entry budget");
            assert!(!cancellation.finish());
            assert!(error.to_string().contains("maximum entry count of 4"));
        }

        #[test]
        fn promotion_reports_committed_staging_cleanup_failure() {
            let _guard = promotion_test_guard();
            let directory = tempfile::tempdir().expect("temporary iOS FFI cleanup promotion");
            let parent = directory.path().join("Artifacts");
            let output = parent.join("rust-player-ffi");
            fs::create_dir_all(&output).expect("create previous iOS FFI output");
            fs::write(output.join("previous.txt"), b"previous")
                .expect("write previous iOS FFI output");
            let target =
                OutputTarget::preflight(output.clone()).expect("preflight iOS FFI output target");
            let staging = tempfile::Builder::new()
                .prefix(".vesper-ios-ffi-stage-")
                .tempdir_in(&parent)
                .expect("create staged iOS FFI output");
            fs::write(staging.path().join("replacement.txt"), b"replacement")
                .expect("write staged iOS FFI output");
            let source = staging.path().to_path_buf();
            let staging_path = staging.path().to_path_buf();
            let cancellation =
                external_process::InterruptDeferral::start("iOS FFI cleanup warning test")
                    .expect("start iOS FFI promotion cancellation scope");
            let mut fail_cleanup = |staging: tempfile::TempDir| {
                let preserved = staging.keep();
                Err(io::Error::other(format!(
                    "test cleanup failure; preserved at {}",
                    preserved.display()
                )))
            };

            let warnings = promote_staged_output_with_hooks(
                staging,
                &source,
                &target,
                &cancellation,
                None,
                None,
                None,
                Some(&mut fail_cleanup),
            )
            .expect("commit succeeds when post-commit cleanup fails");
            assert!(!cancellation.finish());

            assert!(warnings.iter().any(|warning| {
                warning.contains("staging cleanup failed")
                    && warning.contains(&staging_path.display().to_string())
            }));
            assert_eq!(
                fs::read(output.join("replacement.txt"))
                    .expect("read committed iOS FFI replacement"),
                b"replacement"
            );
            assert!(staging_path.is_dir());
            fs::remove_dir_all(&staging_path).expect("remove preserved test staging");
        }

        #[test]
        fn repository_lock_survives_artifacts_parent_replacement() {
            let _guard = promotion_test_guard();
            let directory = tempfile::tempdir().expect("temporary iOS FFI lock repository");
            let root = directory.path().join("repository");
            let artifacts = root.join("lib/ios/VesperPlayerKit/Artifacts");
            fs::create_dir_all(&artifacts).expect("create iOS FFI lock repository");

            let first = IosFfiBuildLock::acquire(&root).expect("acquire first iOS FFI lock");
            let moved = root.join("lib/ios/VesperPlayerKit/moved-artifacts");
            fs::rename(&artifacts, &moved).expect("move iOS FFI artifacts parent");
            fs::create_dir_all(&artifacts).expect("recreate iOS FFI artifacts parent");

            let error = IosFfiBuildLock::acquire(&root)
                .expect_err("repository lock must survive output parent replacement");
            assert!(
                error
                    .to_string()
                    .contains("another iOS FFI build is already active")
            );
            drop(first);
        }

        #[test]
        fn archive_copy_observes_transaction_cancellation() {
            use nix::sys::signal::{Signal, raise};

            let _guard = promotion_test_guard();
            let directory = tempfile::tempdir().expect("temporary cancelled iOS FFI copy");
            let source = directory.path().join(STATIC_LIBRARY_NAME);
            let destination = directory.path().join("staged.a");
            fs::write(&source, vec![7_u8; 128 * 1024]).expect("write iOS FFI copy source");
            let cancellation =
                external_process::InterruptDeferral::start("iOS FFI archive copy test")
                    .expect("start iOS FFI copy cancellation scope");
            raise(Signal::SIGINT).expect("cancel iOS FFI archive copy");

            let error = copy_static_library(&source, &destination, &cancellation)
                .expect_err("cancel iOS FFI archive copy outside a child process");
            assert!(cancellation.finish());

            assert!(error.to_string().contains("archive copy was cancelled"));
            assert_eq!(
                fs::metadata(&destination)
                    .expect("inspect cancelled staged archive")
                    .len(),
                0
            );
        }
    }
}
