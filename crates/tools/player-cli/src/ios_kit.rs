use std::io::Write;
use std::path::Path;

use crate::ios::IosError;

pub(crate) fn ensure_supported_host() -> Result<(), IosError> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(IosError::compatibility(
            "building the VesperPlayerKit XCFramework requires macOS",
        ))
    }
}

pub(crate) fn build(
    root: &Path,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), IosError> {
    ensure_supported_host()?;

    #[cfg(target_os = "macos")]
    {
        implementation::build(root, output, diagnostics)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, output, diagnostics);
        unreachable!("the host gate rejects non-macOS iOS kit builds")
    }
}

pub(crate) fn build_for_release(
    root: &Path,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), IosError> {
    ensure_supported_host()?;

    #[cfg(target_os = "macos")]
    {
        implementation::build_for_release(root, output, diagnostics)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, output, diagnostics);
        unreachable!("the host gate rejects non-macOS iOS kit release builds")
    }
}

#[cfg(target_os = "macos")]
mod implementation {
    use std::collections::BTreeSet;
    use std::env;
    use std::ffi::OsString;
    use std::fs::{self, File, OpenOptions, TryLockError};
    use std::io::{self, Write};
    use std::path::{Component, Path, PathBuf};
    use std::process::{Command, ExitStatus, Stdio};

    use serde::Deserialize;

    use super::IosError;
    use crate::external_process::{self, ExternalProcessErrorKind};
    use crate::ios_ffi::{self, IosFfiProfile};

    const FRAMEWORK_NAME: &str = "VesperPlayerKit.framework";
    const OUTPUT_DIRECTORY_NAME: &str = "xcframework";
    const DEVICE_ARCHIVE_NAME: &str = "VesperPlayerKit-iOS.xcarchive";
    const SIMULATOR_ARCHIVE_NAME: &str = "VesperPlayerKit-iOS-Simulator.xcarchive";
    const XCFRAMEWORK_NAME: &str = "VesperPlayerKit.xcframework";
    const DEFAULT_SIMULATOR_ARCHITECTURE: &str = "arm64";
    const MAX_SIMULATOR_ARCHITECTURE_ENV_BYTES: usize = 4096;
    const MAX_SIMULATOR_ARCHITECTURE_TOKENS: usize = 16;
    const MAX_PROCESS_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
    const MAX_LIPO_OUTPUT_BYTES: usize = 1024 * 1024;
    const MAX_XCFRAMEWORK_MANIFEST_BYTES: u64 = 1024 * 1024;
    const MAX_GENERATED_TREE_ENTRIES: usize = 65_536;
    const MAX_GENERATED_TREE_DEPTH: usize = 64;
    const MAX_GENERATED_TREE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
    const MAX_FRAMEWORK_BINARY_BYTES: u64 = 1024 * 1024 * 1024;

    #[derive(Debug)]
    struct RequiredTools {
        xcodegen: PathBuf,
        xcodebuild: PathBuf,
        ditto: PathBuf,
        xcrun: PathBuf,
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
    struct BuildOutcome {
        output_path: PathBuf,
        ffi_output: Vec<u8>,
        warnings: Vec<String>,
    }

    #[derive(Debug)]
    struct IosKitBuildLock {
        _file: File,
    }

    pub(super) fn build(
        root: &Path,
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        build_with_configuration(root, output, diagnostics, None, None)
    }

    pub(super) fn build_for_release(
        root: &Path,
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        let cargo_target_directory = root.join("target");
        build_with_configuration(
            root,
            output,
            diagnostics,
            Some(vec![DEFAULT_SIMULATOR_ARCHITECTURE.to_owned()]),
            Some(&cargo_target_directory),
        )
    }

    fn build_with_configuration(
        root: &Path,
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
        simulator_architectures: Option<Vec<String>>,
        cargo_target_directory: Option<&Path>,
    ) -> Result<(), IosError> {
        let cancellation = external_process::InterruptDeferral::start("iOS kit XCFramework build")
            .map_err(map_external_process_error)?;
        let result = build_transaction(
            root,
            diagnostics,
            &cancellation,
            simulator_architectures,
            cargo_target_directory,
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
                diagnostics
                    .flush()
                    .map_err(|error| diagnostics_error("flush", error))?;
                if cancelled {
                    return Err(IosError::worker(format!(
                        "iOS kit XCFramework build was cancelled after the output was committed at '{}'",
                        outcome.output_path.display()
                    )));
                }
                output
                    .write_all(&outcome.ffi_output)
                    .map_err(output_error)?;
                writeln!(output).map_err(output_error)?;
                writeln!(output, "Built VesperPlayerKit XCFramework at:").map_err(output_error)?;
                writeln!(output, "  {}", outcome.output_path.display()).map_err(output_error)?;
                output.flush().map_err(output_error)
            }
            Err(error) if cancelled => Err(IosError::worker(format!(
                "iOS kit XCFramework build was cancelled; {error}"
            ))),
            Err(error) => Err(error),
        }
    }

    fn build_transaction(
        root: &Path,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
        simulator_architectures: Option<Vec<String>>,
        cargo_target_directory: Option<&Path>,
    ) -> Result<BuildOutcome, IosError> {
        let simulator_architectures = match simulator_architectures {
            Some(architectures) => architectures,
            None => resolve_simulator_architectures()?,
        };
        let tools = resolve_required_tools()?;
        let _lock = IosKitBuildLock::acquire(root)?;
        let project = require_repository_directory(
            root,
            Path::new("lib/ios/VesperPlayerKit"),
            "VesperPlayerKit project directory",
        )?;
        let project_manifest = require_repository_file(
            root,
            Path::new("lib/ios/VesperPlayerKit/project.yml"),
            "VesperPlayerKit XcodeGen manifest",
        )?;
        let build_parent = prepare_build_parent(&project)?;
        let target = OutputTarget::preflight(build_parent.join(OUTPUT_DIRECTORY_NAME))?;
        let staging = tempfile::Builder::new()
            .prefix(".vesper-ios-kit-stage-")
            .tempdir_in(&build_parent)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS kit staging directory in '{}': {error}",
                    build_parent.display()
                ))
            })?;
        let working = tempfile::Builder::new()
            .prefix("vesper-player-kit-xcframework.")
            .tempdir_in("/private/tmp")
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create the iOS kit Xcode work directory: {error}"
                ))
            })?;

        let ffi_guard = ios_ffi::acquire_build_guard(root)?;
        let mut ffi_output = Vec::new();
        ios_ffi::build_full_in_deferral_holding_lock(
            root,
            IosFfiProfile::Release,
            &mut ffi_output,
            diagnostics,
            cancellation,
            cargo_target_directory,
            &ffi_guard,
        )?;

        let mut xcodegen = Command::new(&tools.xcodegen);
        xcodegen
            .current_dir(&project)
            .arg("generate")
            .arg("--spec")
            .arg(&project_manifest)
            .stdin(Stdio::null());
        run_required_command(
            &mut xcodegen,
            "VesperPlayerKit Xcode project generation",
            diagnostics,
            cancellation,
        )?;
        let project_file = require_regular_directory(
            &project.join("VesperPlayerKit.xcodeproj"),
            "generated VesperPlayerKit Xcode project",
        )?;

        let derived_data = working.path().join("DerivedData");
        let device_archive = working.path().join(DEVICE_ARCHIVE_NAME);
        let simulator_build_archive = working
            .path()
            .join("VesperPlayerKit-iOS-Simulator-arm64.xcarchive");
        let simulator_archive = working.path().join(SIMULATOR_ARCHIVE_NAME);
        let xcframework = working.path().join(XCFRAMEWORK_NAME);

        build_archive(
            &tools.xcodebuild,
            &project_file,
            "iphoneos",
            &derived_data,
            &device_archive,
            None,
            diagnostics,
            cancellation,
        )?;
        for architecture in simulator_architectures {
            build_archive(
                &tools.xcodebuild,
                &project_file,
                "iphonesimulator",
                &derived_data,
                &simulator_build_archive,
                Some(&architecture),
                diagnostics,
                cancellation,
            )?;
        }
        copy_tree(
            &tools.ditto,
            &simulator_build_archive,
            &simulator_archive,
            "Simulator archive merge",
            diagnostics,
            cancellation,
        )?;

        let mut create = Command::new(&tools.xcodebuild);
        create
            .arg("-create-xcframework")
            .arg("-framework")
            .arg(framework_path(&device_archive))
            .arg("-framework")
            .arg(framework_path(&simulator_archive))
            .arg("-output")
            .arg(&xcframework)
            .stdin(Stdio::null());
        run_required_command(
            &mut create,
            "VesperPlayerKit XCFramework creation",
            diagnostics,
            cancellation,
        )?;

        for (source, name, label) in [
            (
                device_archive.as_path(),
                DEVICE_ARCHIVE_NAME,
                "device archive staging",
            ),
            (
                simulator_archive.as_path(),
                SIMULATOR_ARCHIVE_NAME,
                "Simulator archive staging",
            ),
            (
                xcframework.as_path(),
                XCFRAMEWORK_NAME,
                "XCFramework staging",
            ),
        ] {
            copy_tree(
                &tools.ditto,
                source,
                &staging.path().join(name),
                label,
                diagnostics,
                cancellation,
            )?;
        }
        validate_staged_output(staging.path(), &tools.xcrun, diagnostics, cancellation)?;
        let mut warnings = promote_staged_output(staging, &target, cancellation)?;
        let working_path = working.path().to_path_buf();
        if let Err(error) = working.close()
            && error.kind() != io::ErrorKind::NotFound
        {
            warnings.push(format!(
                "iOS kit Xcode work directory cleanup failed for '{}': {error}",
                working_path.display()
            ));
        }
        Ok(BuildOutcome {
            output_path: target.path.join(XCFRAMEWORK_NAME),
            ffi_output,
            warnings,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn build_archive(
        xcodebuild: &Path,
        project_file: &Path,
        sdk: &str,
        derived_data: &Path,
        archive: &Path,
        architecture: Option<&str>,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let mut command = Command::new(xcodebuild);
        command
            .arg("archive")
            .arg("-project")
            .arg(project_file)
            .arg("-scheme")
            .arg("VesperPlayerKit")
            .arg("-sdk")
            .arg(sdk)
            .arg("-derivedDataPath")
            .arg(derived_data)
            .arg("-archivePath")
            .arg(archive)
            .args([
                "SKIP_INSTALL=NO",
                "BUILD_LIBRARY_FOR_DISTRIBUTION=YES",
                "CODE_SIGNING_ALLOWED=NO",
                "CODE_SIGNING_REQUIRED=NO",
                "SUPPORTS_MACCATALYST=NO",
                "VESPER_IOS_FFI_PREBUILT=1",
            ])
            .stdin(Stdio::null());
        if let Some(architecture) = architecture {
            command
                .arg(format!("ARCHS={architecture}"))
                .arg("ONLY_ACTIVE_ARCH=YES");
        }
        run_required_command(
            &mut command,
            &format!("VesperPlayerKit {sdk} archive build"),
            diagnostics,
            cancellation,
        )
    }

    fn copy_tree(
        ditto: &Path,
        source: &Path,
        destination: &Path,
        label: &str,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let mut command = Command::new(ditto);
        command.arg(source).arg(destination).stdin(Stdio::null());
        run_required_command(&mut command, label, diagnostics, cancellation)
    }

    fn framework_path(archive: &Path) -> PathBuf {
        archive
            .join("Products/Library/Frameworks")
            .join(FRAMEWORK_NAME)
    }

    fn resolve_simulator_architectures() -> Result<Vec<String>, IosError> {
        let configured = match env::var("VESPER_IOS_SIMULATOR_ARCHS") {
            Ok(value) => value,
            Err(env::VarError::NotPresent) => DEFAULT_SIMULATOR_ARCHITECTURE.to_owned(),
            Err(env::VarError::NotUnicode(_)) => {
                return Err(IosError::compatibility(
                    "VESPER_IOS_SIMULATOR_ARCHS must be valid UTF-8",
                ));
            }
        };
        parse_simulator_architectures(&configured)
    }

    fn parse_simulator_architectures(configured: &str) -> Result<Vec<String>, IosError> {
        if configured.len() > MAX_SIMULATOR_ARCHITECTURE_ENV_BYTES {
            return Err(IosError::compatibility(format!(
                "VESPER_IOS_SIMULATOR_ARCHS exceeds {MAX_SIMULATOR_ARCHITECTURE_ENV_BYTES} bytes"
            )));
        }
        let mut architectures = Vec::new();
        let mut token_count = 0_usize;
        for token in
            configured.split(|character: char| character == ',' || character.is_ascii_whitespace())
        {
            if token.is_empty() {
                continue;
            }
            token_count = token_count.saturating_add(1);
            if token_count > MAX_SIMULATOR_ARCHITECTURE_TOKENS {
                return Err(IosError::compatibility(format!(
                    "VESPER_IOS_SIMULATOR_ARCHS exceeds {MAX_SIMULATOR_ARCHITECTURE_TOKENS} tokens"
                )));
            }
            if token != DEFAULT_SIMULATOR_ARCHITECTURE {
                return Err(IosError::compatibility(format!(
                    "unsupported iOS simulator architecture: {token}; supported values: arm64"
                )));
            }
            if !architectures.iter().any(|value| value == token) {
                architectures.push(token.to_owned());
            }
        }
        if architectures.is_empty() {
            return Err(IosError::compatibility(
                "no iOS simulator architectures were selected",
            ));
        }
        Ok(architectures)
    }

    fn resolve_required_tools() -> Result<RequiredTools, IosError> {
        Ok(RequiredTools {
            xcodegen: require_path_command(
                "xcodegen",
                "xcodegen is required to generate the VesperPlayerKit framework project; install it with: brew install xcodegen",
            )?,
            xcodebuild: require_path_command(
                "xcodebuild",
                "xcodebuild is required to build the VesperPlayerKit XCFramework",
            )?,
            ditto: require_path_command(
                "ditto",
                "ditto is required to stage the VesperPlayerKit XCFramework",
            )?,
            xcrun: require_path_command(
                "xcrun",
                "xcrun is required to validate VesperPlayerKit binary architectures",
            )?,
        })
    }

    fn require_path_command(name: &str, message: &str) -> Result<PathBuf, IosError> {
        use nix::unistd::{AccessFlags, access};

        let paths = env::var_os("PATH").unwrap_or_default();
        env::split_paths(&paths)
            .find_map(|directory| {
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
            .ok_or_else(|| IosError::compatibility(message))
    }

    fn run_required_command(
        command: &mut Command,
        label: &str,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let result = external_process::run_interruptible_capture_in_deferral(
            command,
            label,
            MAX_PROCESS_OUTPUT_BYTES,
            MAX_PROCESS_OUTPUT_BYTES,
            cancellation,
        )
        .map_err(map_external_process_error)?;
        diagnostics
            .write_all(&result.stdout)
            .map_err(|error| diagnostics_error("write stdout", error))?;
        diagnostics
            .write_all(&result.stderr)
            .map_err(|error| diagnostics_error("write stderr", error))?;
        diagnostics
            .flush()
            .map_err(|error| diagnostics_error("flush", error))?;
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

    fn validate_staged_output(
        staging: &Path,
        xcrun: &Path,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let expected = BTreeSet::from([
            OsString::from(DEVICE_ARCHIVE_NAME),
            OsString::from(SIMULATOR_ARCHIVE_NAME),
            OsString::from(XCFRAMEWORK_NAME),
        ]);
        let mut entries = BTreeSet::new();
        for entry in fs::read_dir(staging).map_err(|error| {
            IosError::conformance(format!(
                "failed to read staged iOS kit output '{}': {error}",
                staging.display()
            ))
        })? {
            if entries.len() >= expected.len() {
                return Err(IosError::conformance(format!(
                    "staged iOS kit output '{}' has an unexpected artifact set",
                    staging.display()
                )));
            }
            let entry = entry.map_err(|error| {
                IosError::conformance(format!(
                    "failed to inspect staged iOS kit output '{}': {error}",
                    staging.display()
                ))
            })?;
            entries.insert(entry.file_name());
        }
        if entries != expected {
            return Err(IosError::conformance(format!(
                "staged iOS kit output '{}' has an unexpected artifact set",
                staging.display()
            )));
        }
        for archive in [DEVICE_ARCHIVE_NAME, SIMULATOR_ARCHIVE_NAME] {
            let binary = framework_path(&staging.join(archive)).join("VesperPlayerKit");
            validate_required_file(
                &binary,
                "staged VesperPlayerKit framework binary",
                MAX_FRAMEWORK_BINARY_BYTES,
            )?;
            validate_arm64_binary(xcrun, &binary, diagnostics, cancellation)?;
        }
        let xcframework = staging.join(XCFRAMEWORK_NAME);
        let manifest = xcframework.join("Info.plist");
        validate_required_file(
            &manifest,
            "staged VesperPlayerKit XCFramework manifest",
            MAX_XCFRAMEWORK_MANIFEST_BYTES,
        )?;
        validate_xcframework_manifest(xcrun, &xcframework, &manifest, diagnostics, cancellation)?;
        validate_generated_tree(staging, cancellation)
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
            "VesperPlayerKit XCFramework manifest parsing",
            MAX_XCFRAMEWORK_MANIFEST_BYTES as usize,
            diagnostics,
            cancellation,
        )?;
        let manifest: XcframeworkManifest = serde_json::from_slice(&bytes).map_err(|error| {
            IosError::conformance(format!(
                "staged VesperPlayerKit XCFramework manifest '{}' is invalid: {error}",
                manifest_path.display()
            ))
        })?;
        if manifest.cf_bundle_package_type != "XFWK" || manifest.format_version != "1.0" {
            return Err(IosError::conformance(format!(
                "staged VesperPlayerKit XCFramework manifest '{}' has an unsupported package type or format version",
                manifest_path.display()
            )));
        }
        if manifest.available_libraries.len() != 2 {
            return Err(IosError::conformance(format!(
                "staged VesperPlayerKit XCFramework manifest '{}' must declare exactly two libraries",
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
                        "staged VesperPlayerKit XCFramework declares unsupported library identifier '{identifier}'"
                    )));
                }
            };
            if !identifiers.insert(library.library_identifier.clone()) {
                return Err(IosError::conformance(format!(
                    "staged VesperPlayerKit XCFramework declares duplicate library identifier '{}'",
                    library.library_identifier
                )));
            }
            if library.library_path != FRAMEWORK_NAME
                || library.binary_path != format!("{FRAMEWORK_NAME}/VesperPlayerKit")
                || library.supported_architectures != ["arm64"]
                || library.supported_platform != "ios"
                || library.supported_platform_variant.as_deref() != expected_variant
            {
                return Err(IosError::conformance(format!(
                    "staged VesperPlayerKit XCFramework library '{}' violates the arm64 iOS slice contract",
                    library.library_identifier
                )));
            }
            let binary = xcframework
                .join(&library.library_identifier)
                .join(&library.binary_path);
            validate_required_file(
                &binary,
                "staged VesperPlayerKit XCFramework slice binary",
                MAX_FRAMEWORK_BINARY_BYTES,
            )?;
            validate_arm64_binary(xcrun, &binary, diagnostics, cancellation)?;
        }
        let expected = BTreeSet::from(["ios-arm64".to_owned(), "ios-arm64-simulator".to_owned()]);
        if identifiers != expected {
            return Err(IosError::conformance(format!(
                "staged VesperPlayerKit XCFramework manifest '{}' has an incomplete library set",
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
            "VesperPlayerKit binary architecture inspection",
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
                "VesperPlayerKit binary '{}' must contain only arm64; found: {}",
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
            .map_err(|error| diagnostics_error("write stderr", error))?;
        if !result.status.success() {
            diagnostics
                .write_all(&result.stdout)
                .map_err(|error| diagnostics_error("write stdout", error))?;
        }
        diagnostics
            .flush()
            .map_err(|error| diagnostics_error("flush", error))?;
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

    fn validate_generated_tree(
        root: &Path,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        validate_generated_tree_with_limits(
            root,
            cancellation,
            MAX_GENERATED_TREE_ENTRIES,
            MAX_GENERATED_TREE_DEPTH,
            MAX_GENERATED_TREE_BYTES,
        )
    }

    fn validate_generated_tree_with_limits(
        root: &Path,
        cancellation: &external_process::InterruptDeferral,
        maximum_entries: usize,
        maximum_depth: usize,
        maximum_bytes: u64,
    ) -> Result<(), IosError> {
        if maximum_entries == 0 {
            return Err(IosError::conformance(
                "iOS kit output entry limit must include the root directory",
            ));
        }
        let mut pending = vec![(root.to_path_buf(), 0_usize)];
        let mut entries = 1_usize;
        let mut bytes = 0_u64;
        while let Some((current, depth)) = pending.pop() {
            check_cancellation(cancellation, "iOS kit output validation")?;
            if depth > maximum_depth {
                return Err(IosError::conformance(format!(
                    "iOS kit output '{}' exceeds the maximum tree depth of {maximum_depth}",
                    root.display()
                )));
            }
            let metadata = fs::symlink_metadata(&current).map_err(|error| {
                IosError::conformance(format!(
                    "failed to inspect iOS kit output entry '{}': {error}",
                    current.display()
                ))
            })?;
            if metadata.file_type().is_symlink()
                || (!metadata.file_type().is_dir() && !metadata.file_type().is_file())
            {
                return Err(IosError::conformance(format!(
                    "iOS kit output entry '{}' is not a regular file or directory",
                    current.display()
                )));
            }
            if metadata.file_type().is_file() {
                bytes = bytes
                    .checked_add(metadata.len())
                    .ok_or_else(|| IosError::conformance("iOS kit output byte count overflowed"))?;
                if bytes > maximum_bytes {
                    return Err(IosError::conformance(format!(
                        "iOS kit output '{}' exceeds {maximum_bytes} bytes",
                        root.display()
                    )));
                }
            } else {
                let child_depth = depth
                    .checked_add(1)
                    .ok_or_else(|| IosError::conformance("iOS kit output tree depth overflowed"))?;
                for entry in fs::read_dir(&current).map_err(|error| {
                    IosError::conformance(format!(
                        "failed to read iOS kit output directory '{}': {error}",
                        current.display()
                    ))
                })? {
                    if entries >= maximum_entries {
                        return Err(IosError::conformance(format!(
                            "iOS kit output '{}' exceeds the maximum entry count of {maximum_entries}",
                            root.display()
                        )));
                    }
                    if child_depth > maximum_depth {
                        return Err(IosError::conformance(format!(
                            "iOS kit output '{}' exceeds the maximum tree depth of {maximum_depth}",
                            root.display()
                        )));
                    }
                    let path = entry
                        .map_err(|error| {
                            IosError::conformance(format!(
                                "failed to inspect iOS kit output directory '{}': {error}",
                                current.display()
                            ))
                        })?
                        .path();
                    entries += 1;
                    pending.push((path, child_depth));
                }
            }
        }
        Ok(())
    }

    fn prepare_build_parent(project: &Path) -> Result<PathBuf, IosError> {
        let path = project.join(".build");
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => Ok(path),
            Ok(_) => Err(IosError::compatibility(format!(
                "iOS kit build parent '{}' must be a regular non-symlink directory",
                path.display()
            ))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&path).map_err(|error| {
                    IosError::storage(format!(
                        "failed to create iOS kit build parent '{}': {error}",
                        path.display()
                    ))
                })?;
                sync_directory(project)?;
                Ok(path)
            }
            Err(error) => Err(IosError::storage(format!(
                "failed to inspect iOS kit build parent '{}': {error}",
                path.display()
            ))),
        }
    }

    impl OutputTarget {
        fn preflight(path: PathBuf) -> Result<Self, IosError> {
            let parent = path.parent().ok_or_else(|| {
                IosError::storage(format!(
                    "iOS kit output '{}' has no parent directory",
                    path.display()
                ))
            })?;
            let parent_metadata = fs::symlink_metadata(parent).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect iOS kit output parent '{}': {error}",
                    parent.display()
                ))
            })?;
            if !parent_metadata.file_type().is_dir() {
                return Err(IosError::compatibility(format!(
                    "iOS kit output parent '{}' must be a regular non-symlink directory",
                    parent.display()
                )));
            }
            Ok(Self {
                path: path.clone(),
                parent: parent.to_path_buf(),
                parent_identity: file_identity(&parent_metadata),
                initial_identity: optional_directory_identity(&path, "iOS kit output")?,
            })
        }

        fn revalidate(&self) -> Result<bool, IosError> {
            let parent = fs::symlink_metadata(&self.parent).map_err(|error| {
                IosError::storage(format!(
                    "failed to recheck iOS kit output parent '{}': {error}",
                    self.parent.display()
                ))
            })?;
            if !parent.file_type().is_dir() || file_identity(&parent) != self.parent_identity {
                return Err(IosError::compatibility(format!(
                    "iOS kit output parent '{}' changed after validation",
                    self.parent.display()
                )));
            }
            let current = optional_directory_identity(&self.path, "iOS kit output")?;
            if current != self.initial_identity {
                return Err(IosError::compatibility(format!(
                    "iOS kit output '{}' changed after validation",
                    self.path.display()
                )));
            }
            Ok(current.is_some())
        }
    }

    fn promote_staged_output(
        staging: tempfile::TempDir,
        target: &OutputTarget,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<Vec<String>, IosError> {
        promote_staged_output_with_hooks(staging, target, cancellation, None, None)
    }

    fn promote_staged_output_with_hooks(
        staging: tempfile::TempDir,
        target: &OutputTarget,
        cancellation: &external_process::InterruptDeferral,
        mut before_exchange: Option<crate::PathIoHook<'_>>,
        mut after_publish: Option<crate::PathIoHook<'_>>,
    ) -> Result<Vec<String>, IosError> {
        let source = staging.path().to_path_buf();
        let source_identity = directory_identity(&source, "staged iOS kit output")?;
        check_cancellation(cancellation, "iOS kit output promotion")?;
        let had_previous = target.revalidate()?;
        if let Some(hook) = before_exchange.as_mut() {
            hook(&target.path).map_err(|error| IosError::storage(error.to_string()))?;
        }
        target.revalidate()?;
        check_cancellation(cancellation, "iOS kit output promotion")?;
        if had_previous {
            exchange_paths(&source, &target.path).map_err(|error| {
                IosError::storage(format!(
                    "failed to atomically exchange iOS kit output '{}': {error}",
                    target.path.display()
                ))
            })?;
        } else {
            rename_noreplace(&source, &target.path).map_err(|error| {
                IosError::storage(format!(
                    "failed to atomically publish iOS kit output '{}': {error}",
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
                    "post-publication iOS kit test hook failed: {hook_error}"
                )),
            ));
        }
        let previous_identity = had_previous
            .then(|| directory_identity(&source, "previous iOS kit output"))
            .transpose()
            .ok()
            .flatten();
        let promoted_identity = directory_identity(&target.path, "promoted iOS kit output").ok();
        if previous_identity != target.initial_identity
            || promoted_identity != Some(source_identity)
        {
            return Err(rollback_or_preserve(
                staging,
                &source,
                target,
                had_previous,
                source_identity,
                IosError::compatibility(format!(
                    "iOS kit output '{}' changed during atomic promotion",
                    target.path.display()
                )),
            ));
        }
        if let Err(error) = sync_directory(&target.parent).and_then(|()| {
            let promoted = directory_identity(&target.path, "promoted iOS kit output")?;
            if promoted == source_identity {
                Ok(())
            } else {
                Err(IosError::compatibility(format!(
                    "iOS kit output '{}' changed during promotion",
                    target.path.display()
                )))
            }
        }) {
            return Err(rollback_or_preserve(
                staging,
                &source,
                target,
                had_previous,
                source_identity,
                error,
            ));
        }

        let staging_path = staging.path().to_path_buf();
        let mut warnings = Vec::new();
        if let Err(error) = staging.close()
            && error.kind() != io::ErrorKind::NotFound
        {
            warnings.push(format!(
                "iOS kit previous output cleanup failed for '{}': {error}",
                staging_path.display()
            ));
        }
        if let Err(error) = sync_directory(&target.parent) {
            warnings.push(format!(
                "iOS kit output parent sync failed for '{}': {error}",
                target.parent.display()
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
            "published iOS kit output during rollback",
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
                "previous iOS kit output during rollback",
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

    fn require_repository_file(
        root: &Path,
        relative: &Path,
        label: &str,
    ) -> Result<PathBuf, IosError> {
        let parent = relative.parent().ok_or_else(|| {
            IosError::compatibility(format!("{label} must have a repository-relative parent"))
        })?;
        let parent = require_repository_directory(root, parent, label)?;
        let name = relative
            .file_name()
            .ok_or_else(|| IosError::compatibility(format!("{label} must have a file name")))?;
        let path = parent.join(name);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() {
            return Err(IosError::compatibility(format!(
                "{label} '{}' must be a regular non-symlink file",
                path.display()
            )));
        }
        Ok(path)
    }

    fn require_regular_directory(path: &Path, label: &str) -> Result<PathBuf, IosError> {
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
        Ok(path.to_path_buf())
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
                    "failed to sync iOS kit directory '{}': {error}",
                    path.display()
                ))
            })
    }

    impl IosKitBuildLock {
        fn acquire(root: &Path) -> Result<Self, IosError> {
            use std::os::unix::fs::OpenOptionsExt;

            let path = root.join(".vesper-ios-kit.lock");
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
                            "failed to open regular non-symlink iOS kit build lock '{}': {open_error}",
                            path.display()
                        ))
                    })?
                }
                Err(error) => {
                    return Err(IosError::storage(format!(
                        "failed to create iOS kit build lock '{}': {error}",
                        path.display()
                    )));
                }
            };
            let metadata = file.metadata().map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect opened iOS kit build lock '{}': {error}",
                    path.display()
                ))
            })?;
            if !metadata.file_type().is_file() {
                return Err(IosError::compatibility(format!(
                    "iOS kit build lock '{}' must be a regular non-symlink file",
                    path.display()
                )));
            }
            match file.try_lock() {
                Ok(()) => Ok(Self { _file: file }),
                Err(TryLockError::WouldBlock) => Err(IosError::compatibility(format!(
                    "another iOS kit build is already active for '{}'",
                    root.display()
                ))),
                Err(TryLockError::Error(error)) => Err(IosError::storage(format!(
                    "failed to lock the iOS kit build for '{}': {error}",
                    root.display()
                ))),
            }
        }
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

    fn append_error(error: IosError, suffix: impl std::fmt::Display) -> IosError {
        let message = format!("{error}; {suffix}");
        match error.kind() {
            crate::ios::IosErrorKind::Storage => IosError::storage(message),
            crate::ios::IosErrorKind::Compatibility => IosError::compatibility(message),
            crate::ios::IosErrorKind::Conformance => IosError::conformance(message),
            crate::ios::IosErrorKind::Worker => IosError::worker(message),
        }
    }

    fn diagnostics_error(operation: &str, error: io::Error) -> IosError {
        IosError::storage(format!(
            "failed to {operation} iOS kit diagnostics: {error}"
        ))
    }

    fn output_error(error: io::Error) -> IosError {
        IosError::storage(format!("failed to write iOS kit output: {error}"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn simulator_architectures_are_bounded_deduplicated_and_arm64_only() {
            assert_eq!(
                parse_simulator_architectures("arm64, arm64").expect("parse arm64 slices"),
                vec!["arm64"]
            );
            assert!(
                parse_simulator_architectures("x86_64")
                    .expect_err("reject Intel Simulator")
                    .to_string()
                    .contains("unsupported iOS simulator architecture")
            );
            assert!(
                parse_simulator_architectures(" , ")
                    .expect_err("reject an empty Simulator selection")
                    .to_string()
                    .contains("no iOS simulator architectures")
            );
        }

        #[test]
        fn whole_output_directory_is_published_in_one_transaction() {
            let directory = tempfile::tempdir().expect("temporary iOS kit transaction");
            let parent = directory.path().join(".build");
            let output = parent.join(OUTPUT_DIRECTORY_NAME);
            fs::create_dir_all(&output).expect("create previous iOS kit output");
            for name in [
                DEVICE_ARCHIVE_NAME,
                SIMULATOR_ARCHIVE_NAME,
                XCFRAMEWORK_NAME,
            ] {
                fs::write(output.join(name), format!("old {name}"))
                    .expect("write previous iOS kit marker");
            }
            let target = OutputTarget::preflight(output.clone())
                .expect("preflight iOS kit transaction output");
            let staging = tempfile::Builder::new()
                .prefix(".vesper-ios-kit-stage-")
                .tempdir_in(&parent)
                .expect("create iOS kit transaction staging");
            for name in [
                DEVICE_ARCHIVE_NAME,
                SIMULATOR_ARCHIVE_NAME,
                XCFRAMEWORK_NAME,
            ] {
                fs::write(staging.path().join(name), format!("new {name}"))
                    .expect("write staged iOS kit marker");
            }
            let cancellation =
                external_process::InterruptDeferral::start("iOS kit transaction test")
                    .expect("start iOS kit transaction cancellation scope");

            let warnings = promote_staged_output(staging, &target, &cancellation)
                .expect("publish the complete iOS kit output");
            assert!(!cancellation.finish());
            assert!(warnings.is_empty());
            for name in [
                DEVICE_ARCHIVE_NAME,
                SIMULATOR_ARCHIVE_NAME,
                XCFRAMEWORK_NAME,
            ] {
                assert_eq!(
                    fs::read_to_string(output.join(name)).expect("read published iOS kit marker"),
                    format!("new {name}")
                );
            }
        }

        #[test]
        fn generated_tree_rejects_width_before_the_pending_queue_exceeds_the_limit() {
            let directory = tempfile::tempdir().expect("temporary wide iOS kit tree");
            for index in 0..4 {
                fs::write(directory.path().join(format!("entry-{index}")), b"fixture")
                    .expect("write wide iOS kit fixture entry");
            }
            let cancellation = external_process::InterruptDeferral::start("wide iOS kit tree test")
                .expect("start wide tree cancellation scope");

            let error = validate_generated_tree_with_limits(
                directory.path(),
                &cancellation,
                4,
                MAX_GENERATED_TREE_DEPTH,
                MAX_GENERATED_TREE_BYTES,
            )
            .expect_err("reject a tree wider than its entry budget");
            assert!(!cancellation.finish());
            assert!(error.to_string().contains("maximum entry count of 4"));
        }

        #[test]
        fn committed_build_cancellation_returns_worker_error_without_stdout() {
            let committed = PathBuf::from("/committed/VesperPlayerKit.xcframework");
            let outcome = BuildOutcome {
                output_path: committed.clone(),
                ffi_output: b"nested FFI success output\n".to_vec(),
                warnings: vec!["cleanup warning".to_owned()],
            };
            let mut output = Vec::new();
            let mut diagnostics = Vec::new();

            let error = report_build_result(Ok(outcome), true, &mut output, &mut diagnostics)
                .expect_err("report cancellation after committing the iOS kit output");

            assert_eq!(error.kind(), crate::ios::IosErrorKind::Worker);
            assert!(error.to_string().contains("cancelled"));
            assert!(error.to_string().contains("committed"));
            assert!(error.to_string().contains(&committed.display().to_string()));
            assert!(output.is_empty());
            assert_eq!(diagnostics, b"warning: cleanup warning\n");
        }

        #[test]
        fn promotion_preserves_previous_output_when_post_publish_identity_is_unavailable() {
            let directory = tempfile::tempdir().expect("temporary iOS kit promotion failure");
            let parent = directory.path().join(".build");
            let output = parent.join(OUTPUT_DIRECTORY_NAME);
            fs::create_dir_all(&output).expect("create previous iOS kit output");
            fs::write(output.join("previous.txt"), b"previous")
                .expect("write previous iOS kit output");
            let target =
                OutputTarget::preflight(output.clone()).expect("preflight iOS kit output target");
            let staging = tempfile::Builder::new()
                .prefix(".vesper-ios-kit-stage-")
                .tempdir_in(&parent)
                .expect("create staged iOS kit output");
            fs::write(staging.path().join("replacement.txt"), b"replacement")
                .expect("write staged iOS kit output");
            let source = staging.path().to_path_buf();
            let displaced_published = parent.join("displaced-published");
            let mut remove_published_identity =
                |path: &Path| fs::rename(path, &displaced_published);
            let cancellation =
                external_process::InterruptDeferral::start("iOS kit identity failure test")
                    .expect("start promotion failure cancellation scope");

            let error = promote_staged_output_with_hooks(
                staging,
                &target,
                &cancellation,
                None,
                Some(&mut remove_published_identity),
            )
            .expect_err("reject unavailable post-publication identity");
            assert!(!cancellation.finish());

            assert!(
                error
                    .to_string()
                    .contains("changed during atomic promotion")
            );
            assert!(error.to_string().contains("rollback skipped"));
            assert_eq!(
                fs::read(source.join("previous.txt")).expect("read quarantined previous output"),
                b"previous"
            );
            assert_eq!(
                fs::read(displaced_published.join("replacement.txt"))
                    .expect("read independently displaced replacement"),
                b"replacement"
            );
            assert!(!output.exists());
        }
    }
}
