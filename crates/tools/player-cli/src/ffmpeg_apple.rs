use std::path::Path;

use crate::ffmpeg::{FfmpegError, FfmpegRequest, NativeFfmpegProfile};
use crate::ffmpeg_source::FfmpegBuildSource;

pub(crate) fn run(
    root: &Path,
    request: &FfmpegRequest,
    profile: &NativeFfmpegProfile,
    source: &FfmpegBuildSource,
) -> Result<(), FfmpegError> {
    if !cfg!(target_os = "macos") {
        return Err(FfmpegError::compatibility(
            "building Apple FFmpeg prebuilts requires macOS",
        ));
    }

    #[cfg(target_os = "macos")]
    {
        implementation::run(root, request, profile, source)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, request, profile, source);
        unreachable!("the host gate rejects non-macOS Apple FFmpeg builds")
    }
}

pub(crate) fn verify_prebuilts(
    output_directory: &Path,
    slices: &[String],
    profile: &NativeFfmpegProfile,
) -> Result<Vec<std::path::PathBuf>, FfmpegError> {
    #[cfg(target_os = "macos")]
    {
        implementation::verify_prebuilts(output_directory, slices, profile)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (output_directory, slices, profile);
        Err(FfmpegError::compatibility(
            "verifying Apple FFmpeg prebuilts requires macOS",
        ))
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn run_holding_repository_lock(
    root: &Path,
    output_directory: &Path,
    slices: &[String],
    deployment_target: &str,
    profile: &NativeFfmpegProfile,
    source: &FfmpegBuildSource,
    diagnostics: &mut dyn std::io::Write,
    cancellation: &crate::external_process::InterruptDeferral,
) -> Result<(), FfmpegError> {
    implementation::run_holding_repository_lock(
        root,
        output_directory,
        slices,
        deployment_target,
        profile,
        source,
        diagnostics,
        cancellation,
        implementation::SourceIntegrityMode::BuildEnvironment,
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn run_holding_repository_lock_with_canonical_source(
    root: &Path,
    output_directory: &Path,
    slices: &[String],
    deployment_target: &str,
    profile: &NativeFfmpegProfile,
    source: &FfmpegBuildSource,
    diagnostics: &mut dyn std::io::Write,
    cancellation: &crate::external_process::InterruptDeferral,
) -> Result<(), FfmpegError> {
    implementation::run_holding_repository_lock(
        root,
        output_directory,
        slices,
        deployment_target,
        profile,
        source,
        diagnostics,
        cancellation,
        implementation::SourceIntegrityMode::CanonicalRelease,
    )
}

#[cfg(target_os = "macos")]
mod implementation {
    use std::collections::{BTreeMap, BTreeSet};
    use std::env;
    use std::ffi::OsString;
    use std::fs::{self, File};
    use std::io::{self, Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Component, Path, PathBuf};
    use std::process::Command;

    use sha2::{Digest, Sha256};

    use crate::external_process::{self, BoundedProcessOutput, ExternalProcessErrorKind};
    use crate::ffmpeg::{FfmpegError, FfmpegPlatform, FfmpegRequest, NativeFfmpegProfile};
    use crate::ffmpeg_source::FfmpegBuildSource;
    use crate::source_archive::{
        self, SourceArchiveErrorKind, SourceArchiveFormat, SourceArchivePolicy,
    };

    const DEFAULT_DEPLOYMENT_TARGET: &str = "17.0";
    const DEFAULT_SLICES: [&str; 2] = ["ios-arm64", "ios-simulator-arm64"];
    const MAX_COMMAND_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
    const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
    const MAX_METADATA_BYTES: u64 = 1024 * 1024;
    const MAX_LIBRARY_BYTES: u64 = 1024 * 1024 * 1024;
    const MAX_LIBRARY_ENTRIES: usize = 128;
    const MAX_LIBRARY_SYMLINK_DEPTH: usize = 8;
    const MAX_LIBXML2_HEADER_BYTES: u64 = 1024 * 1024;
    const MAX_GENERATION_ENTRIES: usize = 100_000;
    const MAX_GENERATION_DEPTH: usize = 64;
    const MAX_GENERATION_BYTES: u64 = 8 * 1024 * 1024 * 1024;

    const SOURCE_POLICY: SourceArchivePolicy = SourceArchivePolicy {
        maximum_archive_bytes: 1024 * 1024 * 1024,
        maximum_entries: 100_000,
        maximum_expanded_bytes: 8 * 1024 * 1024 * 1024,
        maximum_path_bytes: 4096,
        maximum_path_depth: 64,
    };

    #[derive(Clone, Copy)]
    pub(super) enum SourceIntegrityMode {
        BuildEnvironment,
        CanonicalRelease,
    }

    pub(super) fn run(
        root: &Path,
        request: &FfmpegRequest,
        profile: &NativeFfmpegProfile,
        source: &FfmpegBuildSource,
    ) -> Result<(), FfmpegError> {
        let guard = crate::ios_plugin::acquire_build_guard(root).map_err(map_ios_error)?;
        crate::ios_plugin::validate_build_guard(root, &guard).map_err(map_ios_error)?;
        let output_directory = crate::ffmpeg::apple_output_directory(
            root,
            request.output_directory.as_deref(),
            &profile.profile_hash,
        );
        let slices = selected_slices(&request.ios_slices)?;
        let deployment_target = deployment_target_from_environment()?;
        let cancellation = external_process::InterruptDeferral::start("Apple FFmpeg build")
            .map_err(map_process_error)?;
        let stderr = io::stderr();
        let mut diagnostics = stderr.lock();
        let result = run_transaction(
            root,
            &output_directory,
            &slices,
            &deployment_target,
            profile,
            source,
            &mut diagnostics,
            &cancellation,
            SourceIntegrityMode::BuildEnvironment,
        );
        let cancelled = cancellation.finish();
        match (result, cancelled) {
            (Ok(()), false) => Ok(()),
            (Ok(()), true) => Err(FfmpegError::worker(
                "Apple FFmpeg build was cancelled after completion",
            )),
            (Err(error), true) => Err(FfmpegError::worker(format!(
                "Apple FFmpeg build was cancelled; {error}"
            ))),
            (Err(error), false) => Err(error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_holding_repository_lock(
        root: &Path,
        output_directory: &Path,
        slices: &[String],
        deployment_target: &str,
        profile: &NativeFfmpegProfile,
        source: &FfmpegBuildSource,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
        source_integrity: SourceIntegrityMode,
    ) -> Result<(), FfmpegError> {
        let slices = selected_slices(slices)?;
        validate_deployment_target(deployment_target)?;
        run_transaction(
            root,
            output_directory,
            &slices,
            deployment_target,
            profile,
            source,
            diagnostics,
            cancellation,
            source_integrity,
        )
    }

    pub(super) fn verify_prebuilts(
        output_directory: &Path,
        slices: &[String],
        profile: &NativeFfmpegProfile,
    ) -> Result<Vec<PathBuf>, FfmpegError> {
        require_directory(output_directory, "Apple FFmpeg output directory")?;
        let slices = selected_slices(slices)?;
        let mut metadata_paths = Vec::with_capacity(slices.len());
        for slice in slices {
            let component =
                SliceDescriptor::parse(&slice, DEFAULT_DEPLOYMENT_TARGET)?.output_component;
            let staged = output_directory.join(component);
            validate_staged_install(&staged, profile)?;
            metadata_paths.push(staged.join("vesper-ffmpeg-build-metadata.txt"));
        }
        Ok(metadata_paths)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_transaction(
        root: &Path,
        output_directory: &Path,
        slices: &[String],
        deployment_target: &str,
        profile: &NativeFfmpegProfile,
        source: &FfmpegBuildSource,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
        source_integrity: SourceIntegrityMode,
    ) -> Result<(), FfmpegError> {
        check_cancellation(cancellation, "Apple FFmpeg preflight")?;
        validate_deployment_target(deployment_target)?;
        let tools = RequiredTools::resolve(profile.enable_dash)?;
        let archive = ensure_ffmpeg_source(root, source, source_integrity, cancellation)?;
        let source_sha256 = source_archive::sha256_file(
            &archive,
            SOURCE_POLICY.maximum_archive_bytes,
            "Apple FFmpeg source archive",
        )
        .map_err(map_source_error)?;
        let jobs = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(4);
        let work = tempfile::Builder::new()
            .prefix("vesper-apple-ffmpeg-")
            .tempdir()
            .map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to create Apple FFmpeg work directory: {error}"
                ))
            })?;
        let source_root = source_archive::extract_single_root(
            &archive,
            &work.path().join("source"),
            &format!("ffmpeg-{}", source.version),
            SourceArchiveFormat::TarXz,
            SOURCE_POLICY,
            "Apple FFmpeg source archive",
        )
        .map_err(map_source_error)?;
        require_executable_file(&source_root.join("configure"), "FFmpeg configure script")?;

        let output_parent = output_directory
            .parent()
            .ok_or_else(|| FfmpegError::storage("Apple FFmpeg output has no parent"))?;
        fs::create_dir_all(output_parent).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to create Apple FFmpeg output parent: {error}"
            ))
        })?;
        let generation = tempfile::Builder::new()
            .prefix(".vesper-apple-ffmpeg-generation-")
            .tempdir_in(output_parent)
            .map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to create Apple FFmpeg generation staging: {error}"
                ))
            })?;
        seed_generation(output_directory, generation.path())?;
        for slice in slices {
            check_cancellation(cancellation, "Apple FFmpeg build")?;
            let plan = AppleSlicePlan::new(
                slice,
                generation.path(),
                deployment_target,
                profile,
                source,
                &archive,
                &source_sha256,
                &tools,
                work.path(),
                cancellation,
            )?;
            if !profile.force && plan.is_current(profile)? {
                writeln!(
                    diagnostics,
                    "Apple FFmpeg prebuilt for {slice} is up to date for profile {}.",
                    profile.declared_profile
                )
                .map_err(diagnostics_error)?;
                continue;
            }
            build_slice(
                &source_root,
                &plan,
                profile,
                jobs,
                diagnostics,
                cancellation,
            )?;
        }
        for slice in slices {
            let component = SliceDescriptor::parse(slice, deployment_target)?.output_component;
            let staged = generation.path().join(component);
            validate_staged_install(&staged, profile)?;
        }
        check_cancellation(cancellation, "Apple FFmpeg generation publication")?;
        publish_generation(generation.path(), output_directory)?;
        writeln!(diagnostics, "Built Apple FFmpeg prebuilts into:")
            .and_then(|()| writeln!(diagnostics, "  {}", output_directory.display()))
            .and_then(|()| writeln!(diagnostics, "Using FFmpeg source archive:"))
            .and_then(|()| writeln!(diagnostics, "  {}", archive.display()))
            .and_then(|()| writeln!(diagnostics, "FFmpeg profile:"))
            .and_then(|()| writeln!(diagnostics, "  {}", profile.declared_profile))
            .and_then(|()| writeln!(diagnostics, "Selected slices:"))
            .map_err(diagnostics_error)?;
        for slice in slices {
            writeln!(diagnostics, "  {slice}").map_err(diagnostics_error)?;
        }
        diagnostics.flush().map_err(diagnostics_error)
    }

    struct RequiredTools {
        xcrun: PathBuf,
        make: PathBuf,
        pkg_config: Option<PathBuf>,
    }

    impl RequiredTools {
        fn resolve(require_pkg_config: bool) -> Result<Self, FfmpegError> {
            let xcrun = require_path_command("xcrun", "xcrun")?;
            let make = require_path_command("make", "make")?;
            let pkg_config = if require_pkg_config {
                Some(resolve_pkg_config()?)
            } else {
                None
            };
            Ok(Self {
                xcrun,
                make,
                pkg_config,
            })
        }
    }

    struct AppleSlicePlan {
        slice: String,
        output: PathBuf,
        build: PathBuf,
        configure_line: Vec<String>,
        metadata: String,
        pkg_config_directory: Option<PathBuf>,
        tools: RequiredSliceTools,
    }

    struct RequiredSliceTools {
        make: PathBuf,
    }

    impl AppleSlicePlan {
        #[allow(clippy::too_many_arguments)]
        fn new(
            slice: &str,
            output_directory: &Path,
            deployment_target: &str,
            profile: &NativeFfmpegProfile,
            source: &FfmpegBuildSource,
            source_archive: &Path,
            source_sha256: &str,
            tools: &RequiredTools,
            work: &Path,
            cancellation: &external_process::InterruptDeferral,
        ) -> Result<Self, FfmpegError> {
            let descriptor = SliceDescriptor::parse(slice, deployment_target)?;
            let sdk_path = resolve_sdk_path(
                &xcrun_path(
                    &tools.xcrun,
                    descriptor.sdk,
                    "--show-sdk-path",
                    cancellation,
                )?,
                descriptor.sdk,
            )?;
            let clang = xcrun_path(&tools.xcrun, descriptor.sdk, "-f-clang", cancellation)?;
            require_executable_file(&clang, "Apple clang")?;
            let output = output_directory.join(descriptor.output_component);
            let build = work.join(format!("build-{slice}"));
            fs::create_dir_all(&build).map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to create Apple FFmpeg build directory '{}': {error}",
                    build.display()
                ))
            })?;
            let pkg_config_directory = if profile.enable_dash {
                let directory = work.join(format!("pkgconfig-{slice}"));
                fs::create_dir_all(&directory).map_err(|error| {
                    FfmpegError::storage(format!(
                        "failed to create Apple libxml2 pkg-config directory '{}': {error}",
                        directory.display()
                    ))
                })?;
                write_libxml2_pkg_config(&directory, &sdk_path)?;
                Some(directory)
            } else {
                None
            };
            let cflags = format!(
                "-target {} -isysroot {} -fPIC -I{}/usr/include",
                descriptor.clang_target,
                sdk_path.display(),
                sdk_path.display()
            );
            let ldflags = format!(
                "-target {} -isysroot {} -L{}/usr/lib -lz -Wl,-headerpad_max_install_names",
                descriptor.clang_target,
                sdk_path.display(),
                sdk_path.display()
            );
            let mut configure_line = vec![
                "./configure".to_owned(),
                format!("--prefix={}", output.display()),
                "--install-name-dir=@rpath".to_owned(),
                "--enable-cross-compile".to_owned(),
                "--target-os=darwin".to_owned(),
                "--arch=arm64".to_owned(),
                format!("--cc={}", clang.display()),
                format!("--sysroot={}", sdk_path.display()),
                "--disable-programs".to_owned(),
                "--disable-doc".to_owned(),
                "--disable-autodetect".to_owned(),
                "--enable-static".to_owned(),
                "--enable-shared".to_owned(),
                "--enable-pic".to_owned(),
                format!("--extra-cflags={cflags}"),
                format!("--extra-ldflags={ldflags}"),
            ];
            configure_line.extend(profile.configure_arguments(FfmpegPlatform::Ios));
            if let Some(pkg_config) = &tools.pkg_config {
                configure_line.push(format!("--pkg-config={}", pkg_config.display()));
            }
            let metadata = profile.metadata_text(
                "apple",
                slice,
                &source.version,
                source_archive,
                &source.source_url,
                source_sha256,
                &configure_line,
            );
            Ok(Self {
                slice: slice.to_owned(),
                output,
                build,
                configure_line,
                metadata,
                pkg_config_directory,
                tools: RequiredSliceTools {
                    make: tools.make.clone(),
                },
            })
        }

        fn is_current(&self, profile: &NativeFfmpegProfile) -> Result<bool, FfmpegError> {
            if !regular_file_equals(
                &self.output.join("vesper-ffmpeg-build-metadata.txt"),
                self.metadata.as_bytes(),
            )? {
                return Ok(false);
            }
            let checksums =
                match read_checksum_records(&self.output.join("vesper-ffmpeg-library-sha256.txt"))?
                {
                    Some(checksums) => checksums,
                    None => return Ok(false),
                };
            let libraries = required_libraries(&self.output, profile)?;
            for library in libraries {
                if !regular_nonempty_file(
                    &self
                        .output
                        .join("lib/arm64")
                        .join(format!("lib{library}.a")),
                )? {
                    return Ok(false);
                }
                let dylib = self
                    .output
                    .join("lib/arm64")
                    .join(format!("lib{library}.dylib"));
                let actual = match hash_dylib(&dylib) {
                    Ok(actual) => actual,
                    Err(_) => return Ok(false),
                };
                if checksums.get(&library) != Some(&actual) {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }

    struct SliceDescriptor {
        sdk: &'static str,
        output_component: &'static str,
        clang_target: String,
    }

    impl SliceDescriptor {
        fn parse(slice: &str, deployment_target: &str) -> Result<Self, FfmpegError> {
            match slice {
                "ios-arm64" => Ok(Self {
                    sdk: "iphoneos",
                    output_component: "ios",
                    clang_target: format!("arm64-apple-ios{deployment_target}"),
                }),
                "ios-simulator-arm64" => Ok(Self {
                    sdk: "iphonesimulator",
                    output_component: "ios-simulator",
                    clang_target: format!("arm64-apple-ios{deployment_target}-simulator"),
                }),
                value => Err(FfmpegError::compatibility(format!(
                    "unsupported Apple FFmpeg slice: {value}; expected ios-arm64 or ios-simulator-arm64"
                ))),
            }
        }
    }

    fn build_slice(
        source_root: &Path,
        plan: &AppleSlicePlan,
        profile: &NativeFfmpegProfile,
        jobs: usize,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), FfmpegError> {
        writeln!(
            diagnostics,
            "Building Apple FFmpeg prebuilt for {}",
            plan.slice
        )
        .and_then(|()| writeln!(diagnostics, "  profile: {}", profile.declared_profile))
        .and_then(|()| writeln!(diagnostics, "  output: {}", plan.output.display()))
        .map_err(diagnostics_error)?;
        let configure = source_root.join("configure");
        let mut command = Command::new(&configure);
        command
            .current_dir(&plan.build)
            .args(&plan.configure_line[1..])
            .env("PKG_CONFIG_ALLOW_CROSS", "1");
        if let Some(directory) = &plan.pkg_config_directory {
            let search_path = joined_search_path(directory, env::var_os("PKG_CONFIG_PATH"))?;
            command
                .env("PKG_CONFIG_PATH", &search_path)
                .env("PKG_CONFIG_LIBDIR", &search_path);
        }
        run_required_command(
            &mut command,
            "Apple FFmpeg configure",
            diagnostics,
            cancellation,
        )?;
        let mut make = Command::new(&plan.tools.make);
        make.current_dir(&plan.build).arg(format!("-j{jobs}"));
        run_required_command(&mut make, "Apple FFmpeg build", diagnostics, cancellation)?;

        let parent = plan.output.parent().ok_or_else(|| {
            FfmpegError::storage(format!(
                "Apple FFmpeg output has no parent: {}",
                plan.output.display()
            ))
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to create Apple FFmpeg output parent '{}': {error}",
                parent.display()
            ))
        })?;
        let destdir = tempfile::Builder::new()
            .prefix(".vesper-apple-ffmpeg-install-")
            .tempdir_in(parent)
            .map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to create Apple FFmpeg install staging: {error}"
                ))
            })?;
        let mut install = Command::new(&plan.tools.make);
        install
            .current_dir(&plan.build)
            .arg(format!("DESTDIR={}", destdir.path().display()))
            .arg("install");
        run_required_command(
            &mut install,
            "Apple FFmpeg install",
            diagnostics,
            cancellation,
        )?;
        let staged = staged_install_path(destdir.path(), &plan.output)?;
        normalize_staged_layout(&staged)?;
        fs::write(
            staged.join("vesper-ffmpeg-build-metadata.txt"),
            &plan.metadata,
        )
        .map_err(|error| {
            FfmpegError::storage(format!(
                "failed to write Apple FFmpeg build metadata: {error}"
            ))
        })?;
        write_library_checksums(&staged, profile)?;
        validate_staged_install(&staged, profile)?;
        check_cancellation(cancellation, "Apple FFmpeg publication")?;
        publish_directory(&staged, &plan.output, "Apple FFmpeg prebuilt")
    }

    fn seed_generation(source: &Path, destination: &Path) -> Result<(), FfmpegError> {
        let metadata = match fs::symlink_metadata(source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(FfmpegError::storage(format!(
                    "failed to inspect existing Apple FFmpeg output '{}': {error}",
                    source.display()
                )));
            }
        };
        if !metadata.file_type().is_dir() {
            return Err(FfmpegError::conformance(
                "existing Apple FFmpeg output is not a regular directory",
            ));
        }
        let mut budget = GenerationCopyBudget::default();
        copy_generation_tree(source, destination, 0, &mut budget)
    }

    #[derive(Default)]
    struct GenerationCopyBudget {
        entries: usize,
        bytes: u64,
    }

    fn copy_generation_tree(
        source: &Path,
        destination: &Path,
        depth: usize,
        budget: &mut GenerationCopyBudget,
    ) -> Result<(), FfmpegError> {
        if depth > MAX_GENERATION_DEPTH {
            return Err(FfmpegError::conformance(format!(
                "Apple FFmpeg output exceeds {MAX_GENERATION_DEPTH} directory levels"
            )));
        }
        fs::create_dir_all(destination).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to create Apple FFmpeg generation directory '{}': {error}",
                destination.display()
            ))
        })?;
        let mut entries = fs::read_dir(source)
            .map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to enumerate existing Apple FFmpeg output '{}': {error}",
                    source.display()
                ))
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to read existing Apple FFmpeg output '{}': {error}",
                    source.display()
                ))
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            budget.entries = budget.entries.saturating_add(1);
            if budget.entries > MAX_GENERATION_ENTRIES {
                return Err(FfmpegError::conformance(format!(
                    "Apple FFmpeg output contains more than {MAX_GENERATION_ENTRIES} entries"
                )));
            }
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            let metadata = fs::symlink_metadata(&source_path).map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to inspect existing Apple FFmpeg output entry '{}': {error}",
                    source_path.display()
                ))
            })?;
            if metadata.file_type().is_dir() {
                copy_generation_tree(&source_path, &destination_path, depth + 1, budget)?;
                fs::set_permissions(&destination_path, metadata.permissions()).map_err(
                    |error| {
                        FfmpegError::storage(format!(
                            "failed to preserve Apple FFmpeg directory permissions '{}': {error}",
                            destination_path.display()
                        ))
                    },
                )?;
            } else if metadata.file_type().is_symlink() {
                let target = fs::read_link(&source_path).map_err(|error| {
                    FfmpegError::storage(format!(
                        "failed to read existing Apple FFmpeg symlink '{}': {error}",
                        source_path.display()
                    ))
                })?;
                validate_relative_symlink_target(&target, &source_path)?;
                std::os::unix::fs::symlink(&target, &destination_path).map_err(|error| {
                    FfmpegError::storage(format!(
                        "failed to copy existing Apple FFmpeg symlink '{}': {error}",
                        source_path.display()
                    ))
                })?;
            } else if metadata.file_type().is_file() {
                if metadata.len() > MAX_LIBRARY_BYTES {
                    return Err(FfmpegError::conformance(format!(
                        "existing Apple FFmpeg file is oversized: {}",
                        source_path.display()
                    )));
                }
                budget.bytes = budget.bytes.checked_add(metadata.len()).ok_or_else(|| {
                    FfmpegError::conformance("Apple FFmpeg generation byte budget overflowed")
                })?;
                if budget.bytes > MAX_GENERATION_BYTES {
                    return Err(FfmpegError::conformance(format!(
                        "Apple FFmpeg output exceeds {MAX_GENERATION_BYTES} bytes"
                    )));
                }
                fs::copy(&source_path, &destination_path).map_err(|error| {
                    FfmpegError::storage(format!(
                        "failed to copy existing Apple FFmpeg file '{}': {error}",
                        source_path.display()
                    ))
                })?;
                fs::set_permissions(&destination_path, metadata.permissions()).map_err(
                    |error| {
                        FfmpegError::storage(format!(
                            "failed to preserve Apple FFmpeg file permissions '{}': {error}",
                            destination_path.display()
                        ))
                    },
                )?;
            } else {
                return Err(FfmpegError::conformance(format!(
                    "existing Apple FFmpeg output contains an unsupported file type: {}",
                    source_path.display()
                )));
            }
        }
        Ok(())
    }

    fn validate_relative_symlink_target(target: &Path, source: &Path) -> Result<(), FfmpegError> {
        if target.components().count() != 1
            || !matches!(target.components().next(), Some(Component::Normal(_)))
        {
            return Err(FfmpegError::conformance(format!(
                "Apple FFmpeg output symlink has an unsafe target: {}",
                source.display()
            )));
        }
        Ok(())
    }

    fn publish_generation(generation: &Path, output_directory: &Path) -> Result<(), FfmpegError> {
        publish_generation_with_hook(generation, output_directory, || Ok(()))
    }

    fn publish_generation_with_hook<F>(
        generation: &Path,
        output_directory: &Path,
        before_publish: F,
    ) -> Result<(), FfmpegError>
    where
        F: FnOnce() -> Result<(), FfmpegError>,
    {
        let parent = output_directory
            .parent()
            .ok_or_else(|| FfmpegError::storage("Apple FFmpeg output has no parent"))?;
        fs::create_dir_all(parent).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to create Apple FFmpeg output parent: {error}"
            ))
        })?;
        let backup = tempfile::Builder::new()
            .prefix(".vesper-apple-ffmpeg-previous-")
            .tempdir_in(parent)
            .map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to create Apple FFmpeg rollback staging: {error}"
                ))
            })?;
        let had_output = match fs::symlink_metadata(output_directory) {
            Ok(metadata) if metadata.file_type().is_dir() => true,
            Ok(_) => {
                return Err(FfmpegError::conformance(
                    "Apple FFmpeg output is not a directory",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => {
                return Err(FfmpegError::storage(format!(
                    "failed to inspect Apple FFmpeg output: {error}"
                )));
            }
        };
        if had_output {
            fs::rename(output_directory, backup.path().join("output")).map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to stage previous Apple FFmpeg output: {error}"
                ))
            })?;
        }
        let result = before_publish().and_then(|()| {
            fs::rename(generation, output_directory).map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to publish Apple FFmpeg generation: {error}"
                ))
            })
        });
        if let Err(error) = result {
            if had_output
                && let Err(rollback) = fs::rename(backup.path().join("output"), output_directory)
            {
                let recovery = backup.keep();
                return Err(FfmpegError::storage(format!(
                    "failed to publish Apple FFmpeg generation: {error}; rollback failed: {rollback}; recovery output remains at {}",
                    recovery.join("output").display()
                )));
            }
            return Err(error);
        }
        Ok(())
    }

    fn normalize_staged_layout(staged: &Path) -> Result<(), FfmpegError> {
        let source = staged.join("lib");
        require_directory(&source, "staged Apple FFmpeg library directory")?;
        let holding = staged.join(".vesper-unarchived-lib");
        fs::rename(&source, &holding).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to prepare staged Apple FFmpeg libraries: {error}"
            ))
        })?;
        let destination = source.join("arm64");
        fs::create_dir_all(&destination).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to create staged Apple FFmpeg arm64 directory: {error}"
            ))
        })?;
        let mut library_entries = Vec::new();
        for entry in bounded_directory_entries(&holding, "staged Apple FFmpeg libraries")? {
            let name = entry.file_name();
            let Some(name_text) = name.to_str() else {
                return Err(FfmpegError::conformance(
                    "staged Apple FFmpeg library name is not UTF-8",
                ));
            };
            if !(name_text.ends_with(".a") || name_text.contains(".dylib")) {
                continue;
            }
            let path = entry.path();
            validate_library_entry(&path, &holding)?;
            library_entries.push((path, name));
        }
        if library_entries.is_empty() {
            return Err(FfmpegError::conformance(
                "Apple FFmpeg install did not produce any static or shared libraries",
            ));
        }
        for (path, name) in library_entries {
            let name_text = name.to_string_lossy();
            fs::rename(&path, destination.join(&name)).map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to stage Apple FFmpeg library '{name_text}': {error}"
                ))
            })?;
        }
        fs::remove_dir_all(&holding).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to remove intermediate Apple FFmpeg library directory: {error}"
            ))
        })?;
        require_directory(
            &staged.join("include"),
            "staged Apple FFmpeg include directory",
        )
    }

    fn validate_library_entry(path: &Path, parent: &Path) -> Result<(), FfmpegError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to inspect staged Apple FFmpeg library '{}': {error}",
                path.display()
            ))
        })?;
        if metadata.file_type().is_file() {
            if metadata.len() == 0 || metadata.len() > MAX_LIBRARY_BYTES {
                return Err(FfmpegError::conformance(format!(
                    "staged Apple FFmpeg library is empty or oversized: {}",
                    path.display()
                )));
            }
            return Ok(());
        }
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(path).map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to read staged Apple FFmpeg library symlink '{}': {error}",
                    path.display()
                ))
            })?;
            if target.components().count() != 1
                || !matches!(target.components().next(), Some(Component::Normal(_)))
            {
                return Err(FfmpegError::conformance(format!(
                    "staged Apple FFmpeg library symlink has an unsafe target: {}",
                    path.display()
                )));
            }
            let resolved = parent.join(target);
            if !regular_nonempty_file_through_safe_links(&resolved, parent)? {
                return Err(FfmpegError::conformance(format!(
                    "staged Apple FFmpeg library symlink target is missing: {}",
                    path.display()
                )));
            }
            return Ok(());
        }
        Err(FfmpegError::conformance(format!(
            "staged Apple FFmpeg library must be a regular file or safe relative symlink: {}",
            path.display()
        )))
    }

    fn regular_nonempty_file_through_safe_links(
        path: &Path,
        parent: &Path,
    ) -> Result<bool, FfmpegError> {
        let mut current = path.to_owned();
        let mut visited = BTreeSet::new();
        for depth in 0..=MAX_LIBRARY_SYMLINK_DEPTH {
            if !visited.insert(current.clone()) {
                return Err(FfmpegError::conformance(format!(
                    "staged Apple FFmpeg library symlink cycle detected: {}",
                    current.display()
                )));
            }
            let metadata = match fs::symlink_metadata(&current) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(error) => {
                    return Err(FfmpegError::storage(format!(
                        "failed to inspect staged Apple FFmpeg library '{}': {error}",
                        current.display()
                    )));
                }
            };
            if metadata.file_type().is_file() {
                return Ok(metadata.len() > 0 && metadata.len() <= MAX_LIBRARY_BYTES);
            }
            if !metadata.file_type().is_symlink() {
                return Ok(false);
            }
            if depth == MAX_LIBRARY_SYMLINK_DEPTH {
                return Err(FfmpegError::conformance(format!(
                    "staged Apple FFmpeg library symlink chain exceeds {MAX_LIBRARY_SYMLINK_DEPTH} links: {}",
                    current.display()
                )));
            }
            let target = fs::read_link(&current).map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to read staged Apple FFmpeg library symlink '{}': {error}",
                    current.display()
                ))
            })?;
            validate_relative_symlink_target(&target, &current)?;
            current = parent.join(target);
        }
        Err(FfmpegError::conformance(format!(
            "staged Apple FFmpeg library symlink chain could not be resolved: {}",
            current.display()
        )))
    }

    fn write_library_checksums(
        staged: &Path,
        profile: &NativeFfmpegProfile,
    ) -> Result<(), FfmpegError> {
        let libraries = required_libraries(staged, profile)?;
        if libraries.is_empty() {
            return Err(FfmpegError::conformance(
                "Apple FFmpeg install did not expose any unversioned shared libraries",
            ));
        }
        let mut records = String::new();
        for library in libraries {
            let path = staged.join("lib/arm64").join(format!("lib{library}.dylib"));
            let checksum = hash_dylib(&path)?;
            records.push_str(&format!("{library}_sha256={checksum}\n"));
        }
        fs::write(staged.join("vesper-ffmpeg-library-sha256.txt"), records).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to write Apple FFmpeg library checksums: {error}"
            ))
        })
    }

    fn required_libraries(
        root: &Path,
        profile: &NativeFfmpegProfile,
    ) -> Result<Vec<String>, FfmpegError> {
        if !profile.libraries.is_empty() {
            return Ok(profile.libraries.clone());
        }
        let directory = root.join("lib/arm64");
        let mut libraries = BTreeSet::new();
        for entry in bounded_directory_entries(&directory, "Apple FFmpeg library directory")? {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return Err(FfmpegError::conformance(
                    "Apple FFmpeg library name is not UTF-8",
                ));
            };
            if let Some(library) = name
                .strip_prefix("lib")
                .and_then(|name| name.strip_suffix(".dylib"))
                .filter(|name| !name.is_empty() && !name.contains('.'))
            {
                libraries.insert(library.to_owned());
            }
        }
        Ok(libraries.into_iter().collect())
    }

    fn validate_staged_install(
        staged: &Path,
        profile: &NativeFfmpegProfile,
    ) -> Result<(), FfmpegError> {
        require_directory(&staged.join("include"), "Apple FFmpeg include directory")?;
        let metadata = fs::symlink_metadata(staged.join("vesper-ffmpeg-build-metadata.txt"))
            .map_err(|error| {
                FfmpegError::conformance(format!(
                    "Apple FFmpeg build metadata is missing from '{}': {error}",
                    staged.display()
                ))
            })?;
        if !metadata.file_type().is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_METADATA_BYTES
        {
            return Err(FfmpegError::conformance(format!(
                "Apple FFmpeg build metadata must be a bounded non-empty regular file: {}",
                staged.join("vesper-ffmpeg-build-metadata.txt").display()
            )));
        }
        let checksums = read_checksum_records(&staged.join("vesper-ffmpeg-library-sha256.txt"))?
            .ok_or_else(|| FfmpegError::conformance("Apple FFmpeg checksum record is missing"))?;
        let libraries = required_libraries(staged, profile)?;
        if checksums.len() != libraries.len()
            || libraries
                .iter()
                .any(|library| !checksums.contains_key(library))
        {
            return Err(FfmpegError::conformance(
                "Apple FFmpeg checksum record does not exactly match the required libraries",
            ));
        }
        for library in libraries {
            validate_header_directory(&staged.join("include").join(format!("lib{library}")))?;
            let static_library = staged.join("lib/arm64").join(format!("lib{library}.a"));
            if !regular_nonempty_file(&static_library)? {
                return Err(FfmpegError::conformance(format!(
                    "Apple FFmpeg static library is missing: {}",
                    static_library.display()
                )));
            }
            let dylib = staged.join("lib/arm64").join(format!("lib{library}.dylib"));
            let actual = hash_dylib(&dylib)?;
            if checksums.get(&library) != Some(&actual) {
                return Err(FfmpegError::conformance(format!(
                    "Apple FFmpeg checksum record does not match {}",
                    dylib.display()
                )));
            }
        }
        Ok(())
    }

    fn validate_header_directory(path: &Path) -> Result<(), FfmpegError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            FfmpegError::conformance(format!(
                "Apple FFmpeg header directory is missing '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(FfmpegError::conformance(format!(
                "Apple FFmpeg header path must be a regular directory: {}",
                path.display()
            )));
        }
        let mut entries_seen = 0_usize;
        let mut files_seen = 0_usize;
        let mut pending = vec![(path.to_path_buf(), 0_usize)];
        while let Some((directory, depth)) = pending.pop() {
            if depth > MAX_GENERATION_DEPTH {
                return Err(FfmpegError::conformance(format!(
                    "Apple FFmpeg header directory exceeds {MAX_GENERATION_DEPTH} levels: {}",
                    path.display()
                )));
            }
            for entry in fs::read_dir(&directory).map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to enumerate Apple FFmpeg headers '{}': {error}",
                    directory.display()
                ))
            })? {
                entries_seen = entries_seen.saturating_add(1);
                if entries_seen > MAX_GENERATION_ENTRIES {
                    return Err(FfmpegError::conformance(format!(
                        "Apple FFmpeg header directory contains more than {MAX_GENERATION_ENTRIES} entries: {}",
                        path.display()
                    )));
                }
                let entry = entry.map_err(|error| {
                    FfmpegError::storage(format!(
                        "failed to inspect Apple FFmpeg header entry '{}': {error}",
                        directory.display()
                    ))
                })?;
                let entry_path = entry.path();
                let entry_metadata = fs::symlink_metadata(&entry_path).map_err(|error| {
                    FfmpegError::storage(format!(
                        "failed to inspect Apple FFmpeg header '{}': {error}",
                        entry_path.display()
                    ))
                })?;
                if entry_metadata.file_type().is_symlink() {
                    return Err(FfmpegError::conformance(format!(
                        "Apple FFmpeg headers contain a symlink: {}",
                        entry_path.display()
                    )));
                }
                if entry_metadata.file_type().is_dir() {
                    pending.push((entry_path, depth + 1));
                } else if entry_metadata.file_type().is_file() {
                    if entry_metadata.len() > MAX_METADATA_BYTES {
                        return Err(FfmpegError::conformance(format!(
                            "Apple FFmpeg header is oversized: {}",
                            entry_path.display()
                        )));
                    }
                    files_seen = files_seen.saturating_add(1);
                } else {
                    return Err(FfmpegError::conformance(format!(
                        "Apple FFmpeg headers contain an unsupported file type: {}",
                        entry_path.display()
                    )));
                }
            }
        }
        if files_seen == 0 {
            return Err(FfmpegError::conformance(format!(
                "Apple FFmpeg header directory is empty: {}",
                path.display()
            )));
        }
        Ok(())
    }

    fn read_checksum_records(path: &Path) -> Result<Option<BTreeMap<String, String>>, FfmpegError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(FfmpegError::storage(format!(
                    "failed to inspect Apple FFmpeg checksum record '{}': {error}",
                    path.display()
                )));
            }
        };
        if !metadata.file_type().is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_METADATA_BYTES
        {
            return Ok(None);
        }
        let text = fs::read_to_string(path).map_err(|error| {
            FfmpegError::conformance(format!(
                "failed to read UTF-8 Apple FFmpeg checksum record '{}': {error}",
                path.display()
            ))
        })?;
        let mut records = BTreeMap::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once("_sha256=") else {
                return Ok(None);
            };
            if key.is_empty()
                || value.len() != 64
                || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
                || records.insert(key.to_owned(), value.to_owned()).is_some()
            {
                return Ok(None);
            }
        }
        Ok(Some(records))
    }

    fn hash_dylib(path: &Path) -> Result<String, FfmpegError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            FfmpegError::conformance(format!(
                "Apple FFmpeg shared library is missing '{}': {error}",
                path.display()
            ))
        })?;
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(path).map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to read Apple FFmpeg dylib alias '{}': {error}",
                    path.display()
                ))
            })?;
            if target.components().count() != 1
                || !matches!(target.components().next(), Some(Component::Normal(_)))
            {
                return Err(FfmpegError::conformance(format!(
                    "Apple FFmpeg dylib alias has an unsafe target: {}",
                    path.display()
                )));
            }
        } else if !metadata.file_type().is_file() {
            return Err(FfmpegError::conformance(format!(
                "Apple FFmpeg dylib must be a regular file or safe relative symlink: {}",
                path.display()
            )));
        }
        hash_regular_file(path, "Apple FFmpeg shared library")
    }

    fn hash_regular_file(path: &Path, label: &str) -> Result<String, FfmpegError> {
        let metadata = fs::metadata(path).map_err(|error| {
            FfmpegError::conformance(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_LIBRARY_BYTES
        {
            return Err(FfmpegError::conformance(format!(
                "{label} must be a non-empty regular file within its size limit: {}",
                path.display()
            )));
        }
        let mut file = File::open(path).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to open {label} '{}': {error}",
                path.display()
            ))
        })?;
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let mut total = 0_u64;
        loop {
            let count = file.read(&mut buffer).map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to read {label} '{}': {error}",
                    path.display()
                ))
            })?;
            if count == 0 {
                break;
            }
            total = total
                .checked_add(count as u64)
                .ok_or_else(|| FfmpegError::conformance(format!("{label} size overflowed")))?;
            if total > MAX_LIBRARY_BYTES {
                return Err(FfmpegError::conformance(format!(
                    "{label} exceeds its size limit: {}",
                    path.display()
                )));
            }
            digest.update(&buffer[..count]);
        }
        Ok(format!("{:x}", digest.finalize()))
    }

    fn selected_slices(raw: &[String]) -> Result<Vec<String>, FfmpegError> {
        let values = if raw.is_empty() {
            DEFAULT_SLICES.iter().map(ToString::to_string).collect()
        } else {
            crate::ffmpeg::flatten_list_values(raw)
        };
        if values.is_empty() {
            return Err(FfmpegError::conformance(
                "no Apple FFmpeg slices were selected",
            ));
        }
        let mut seen = BTreeSet::new();
        for value in &values {
            SliceDescriptor::parse(value, DEFAULT_DEPLOYMENT_TARGET)?;
            if !seen.insert(value.clone()) {
                return Err(FfmpegError::conformance(format!(
                    "duplicate Apple FFmpeg slice: {value}"
                )));
            }
        }
        Ok(values)
    }

    fn deployment_target_from_environment() -> Result<String, FfmpegError> {
        let value = match env::var("VESPER_APPLE_IOS_DEPLOYMENT_TARGET") {
            Ok(value) if !value.is_empty() => value,
            Ok(_) | Err(env::VarError::NotPresent) => DEFAULT_DEPLOYMENT_TARGET.to_owned(),
            Err(env::VarError::NotUnicode(_)) => {
                return Err(FfmpegError::compatibility(
                    "VESPER_APPLE_IOS_DEPLOYMENT_TARGET must be valid UTF-8",
                ));
            }
        };
        validate_deployment_target(&value)?;
        Ok(value)
    }

    fn validate_deployment_target(value: &str) -> Result<(), FfmpegError> {
        let components = value.split('.').collect::<Vec<_>>();
        if components.is_empty()
            || components.len() > 3
            || components.iter().any(|component| {
                component.is_empty() || !component.bytes().all(|b| b.is_ascii_digit())
            })
        {
            return Err(FfmpegError::conformance(format!(
                "invalid iOS deployment target: {value}"
            )));
        }
        let major = components[0].parse::<u32>().map_err(|error| {
            FfmpegError::conformance(format!("invalid iOS deployment target {value}: {error}"))
        })?;
        if major < 17 {
            return Err(FfmpegError::compatibility(format!(
                "iOS deployment target {value} is below the supported floor 17.0"
            )));
        }
        Ok(())
    }

    fn ensure_ffmpeg_source(
        root: &Path,
        source: &FfmpegBuildSource,
        source_integrity: SourceIntegrityMode,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<PathBuf, FfmpegError> {
        let archive = configured_path(
            root,
            "VESPER_APPLE_FFMPEG_SOURCE_ARCHIVE",
            &source.archive_name,
        );
        let expected = match source_integrity {
            SourceIntegrityMode::BuildEnvironment => {
                environment_text("VESPER_APPLE_FFMPEG_EXPECTED_SOURCE_SHA256")?
                    .or(environment_text("VESPER_APPLE_FFMPEG_SOURCE_SHA256")?)
                    .or(environment_text("VESPER_FFMPEG_SOURCE_SHA256")?)
                    .or_else(|| source.expected_sha256.clone())
            }
            SourceIntegrityMode::CanonicalRelease => {
                Some(source.expected_sha256.clone().ok_or_else(|| {
                    FfmpegError::conformance(
                        "canonical Apple FFmpeg releases require a locked source checksum",
                    )
                })?)
            }
        };
        if expected.is_none() {
            eprintln!(
                "warning: Apple FFmpeg source {} has no pinned SHA-256; canonical releases must use the checked-in source lock",
                source.version
            );
        }
        source_archive::ensure_cached_archive_in_deferral(
            &archive,
            std::slice::from_ref(&source.source_url),
            expected.as_deref(),
            SOURCE_POLICY,
            "Apple FFmpeg source archive",
            cancellation,
        )
        .map_err(map_source_error)
    }

    fn configured_path(root: &Path, name: &str, file_name: &str) -> PathBuf {
        if let Some(value) = env::var_os(name).filter(|value| !value.is_empty()) {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                return path;
            }
            return root.join(path);
        }
        let cache = env::var_os("VESPER_THIRD_PARTY_SOURCE_CACHE_DIR")
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    root.join(path)
                }
            })
            .unwrap_or_else(|| root.join("third_party/_cache"));
        cache.join(file_name)
    }

    fn xcrun_path(
        xcrun: &Path,
        sdk: &str,
        operation: &str,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<PathBuf, FfmpegError> {
        let mut command = Command::new(xcrun);
        command.args(["--sdk", sdk]);
        let label = if operation == "--show-sdk-path" {
            command.arg(operation);
            format!("xcrun {sdk} SDK lookup")
        } else {
            command.args(["-f", "clang"]);
            format!("xcrun {sdk} clang lookup")
        };
        let output = external_process::run_interruptible_capture_in_deferral(
            &mut command,
            &label,
            MAX_TOOL_OUTPUT_BYTES,
            MAX_TOOL_OUTPUT_BYTES,
            cancellation,
        )
        .map_err(map_process_error)?;
        if !output.status.success() {
            return Err(FfmpegError::compatibility(format!(
                "{label} exited unsuccessfully ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let value = std::str::from_utf8(&output.stdout)
            .map_err(|error| {
                FfmpegError::conformance(format!("{label} returned non-UTF-8 output: {error}"))
            })?
            .trim();
        if value.is_empty() || value.lines().count() != 1 {
            return Err(FfmpegError::conformance(format!(
                "{label} must return exactly one non-empty path"
            )));
        }
        Ok(PathBuf::from(value))
    }

    fn resolve_sdk_path(path: &Path, sdk: &str) -> Result<PathBuf, FfmpegError> {
        let canonical = fs::canonicalize(path).map_err(|error| {
            FfmpegError::compatibility(format!(
                "failed to resolve {sdk} SDK path '{}': {error}",
                path.display()
            ))
        })?;
        require_directory(&canonical, &format!("{sdk} SDK"))?;
        Ok(canonical)
    }

    fn write_libxml2_pkg_config(directory: &Path, sdk: &Path) -> Result<(), FfmpegError> {
        let version = read_libxml2_version(sdk)?.unwrap_or_else(|| "2.0.0".to_owned());
        let prefix = sdk.join("usr");
        let source = format!(
            "prefix={}\nexec_prefix=${{prefix}}\nlibdir=${{prefix}}/lib\nincludedir=${{prefix}}/include\n\nName: libxml2\nDescription: Apple SDK libxml2\nVersion: {version}\nLibs: -L${{libdir}} -lxml2 -lz\nCflags: -I${{includedir}}/libxml2\n",
            prefix.display()
        );
        fs::write(directory.join("libxml-2.0.pc"), source).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to write Apple SDK libxml2 pkg-config file: {error}"
            ))
        })
    }

    fn read_libxml2_version(sdk: &Path) -> Result<Option<String>, FfmpegError> {
        let path = sdk.join("usr/include/libxml2/libxml/xmlversion.h");
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(FfmpegError::storage(format!(
                    "failed to inspect Apple SDK libxml2 version header '{}': {error}",
                    path.display()
                )));
            }
        };
        if !metadata.file_type().is_file() || metadata.len() > MAX_LIBXML2_HEADER_BYTES {
            return Err(FfmpegError::conformance(format!(
                "Apple SDK libxml2 version header must be a bounded regular file: {}",
                path.display()
            )));
        }
        let source = fs::read_to_string(&path).map_err(|error| {
            FfmpegError::conformance(format!(
                "failed to read Apple SDK libxml2 version header '{}': {error}",
                path.display()
            ))
        })?;
        let values = source
            .lines()
            .filter_map(|line| {
                line.strip_prefix("#define LIBXML_DOTTED_VERSION \"")
                    .and_then(|value| value.strip_suffix('"'))
            })
            .collect::<Vec<_>>();
        match values.as_slice() {
            [] => Ok(None),
            [value] if !value.is_empty() => Ok(Some((*value).to_owned())),
            _ => Err(FfmpegError::conformance(format!(
                "Apple SDK libxml2 version header '{}' contains an ambiguous version",
                path.display()
            ))),
        }
    }

    fn resolve_pkg_config() -> Result<PathBuf, FfmpegError> {
        if let Some(configured) = env::var_os("PKG_CONFIG").filter(|value| !value.is_empty()) {
            let path = PathBuf::from(&configured);
            if path.components().count() > 1 || path.is_absolute() {
                require_executable_file(&path, "pkg-config")?;
                return Ok(path);
            }
            return require_path_command(
                configured
                    .to_str()
                    .ok_or_else(|| FfmpegError::compatibility("PKG_CONFIG must be valid UTF-8"))?,
                "pkg-config",
            );
        }
        require_path_command("pkg-config", "pkg-config")
    }

    fn require_path_command(name: &str, label: &str) -> Result<PathBuf, FfmpegError> {
        let paths = env::var_os("PATH").ok_or_else(|| {
            FfmpegError::compatibility(format!("PATH is required to locate {label}"))
        })?;
        for directory in env::split_paths(&paths) {
            let candidate = directory.join(name);
            if is_executable_file(&candidate) {
                return Ok(candidate);
            }
        }
        Err(FfmpegError::compatibility(format!(
            "missing required command: {label}"
        )))
    }

    fn is_executable_file(path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok_and(|metadata| {
            metadata.file_type().is_file() && metadata.permissions().mode() & 0o111 != 0
        })
    }

    fn require_executable_file(path: &Path, label: &str) -> Result<(), FfmpegError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            FfmpegError::compatibility(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(FfmpegError::compatibility(format!(
                "{label} must be an executable regular file: {}",
                path.display()
            )));
        }
        Ok(())
    }

    fn require_directory(path: &Path, label: &str) -> Result<(), FfmpegError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            FfmpegError::compatibility(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(FfmpegError::compatibility(format!(
                "{label} must be a regular non-symlink directory: {}",
                path.display()
            )));
        }
        Ok(())
    }

    fn run_required_command(
        command: &mut Command,
        label: &str,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), FfmpegError> {
        let output = external_process::run_interruptible_capture_in_deferral(
            command,
            label,
            MAX_COMMAND_OUTPUT_BYTES,
            MAX_COMMAND_OUTPUT_BYTES,
            cancellation,
        )
        .map_err(map_process_error)?;
        write_process_output(diagnostics, &output)?;
        if output.status.success() {
            Ok(())
        } else if output.status.code().is_none_or(|code| code >= 128) {
            Err(FfmpegError::worker(format!(
                "{label} terminated abnormally ({})",
                output.status
            )))
        } else {
            Err(FfmpegError::conformance(format!(
                "{label} exited unsuccessfully ({})",
                output.status
            )))
        }
    }

    fn write_process_output(
        diagnostics: &mut dyn Write,
        output: &BoundedProcessOutput,
    ) -> Result<(), FfmpegError> {
        diagnostics
            .write_all(&output.stdout)
            .and_then(|()| diagnostics.write_all(&output.stderr))
            .and_then(|()| diagnostics.flush())
            .map_err(diagnostics_error)
    }

    fn staged_install_path(destdir: &Path, prefix: &Path) -> Result<PathBuf, FfmpegError> {
        if !prefix.is_absolute() {
            return Err(FfmpegError::conformance(format!(
                "Apple FFmpeg output must resolve to an absolute path: {}",
                prefix.display()
            )));
        }
        let relative = prefix.strip_prefix(Path::new("/")).map_err(|error| {
            FfmpegError::conformance(format!(
                "failed to resolve staged Apple FFmpeg install path '{}': {error}",
                prefix.display()
            ))
        })?;
        let staged = destdir.join(relative);
        require_directory(&staged, "staged Apple FFmpeg install")?;
        Ok(staged)
    }

    fn bounded_directory_entries(
        path: &Path,
        label: &str,
    ) -> Result<Vec<fs::DirEntry>, FfmpegError> {
        let entries = fs::read_dir(path).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to enumerate {label} '{}': {error}",
                path.display()
            ))
        })?;
        let mut result = Vec::new();
        for entry in entries {
            if result.len() >= MAX_LIBRARY_ENTRIES {
                return Err(FfmpegError::conformance(format!(
                    "{label} contains more than {MAX_LIBRARY_ENTRIES} entries"
                )));
            }
            result.push(entry.map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to read {label} entry '{}': {error}",
                    path.display()
                ))
            })?);
        }
        result.sort_by_key(|entry| entry.file_name());
        Ok(result)
    }

    fn publish_directory(source: &Path, target: &Path, label: &str) -> Result<(), FfmpegError> {
        let parent = target.parent().ok_or_else(|| {
            FfmpegError::storage(format!(
                "{label} target has no parent: {}",
                target.display()
            ))
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            FfmpegError::storage(format!("failed to create {label} parent: {error}"))
        })?;
        let backup = tempfile::Builder::new()
            .prefix(".vesper-apple-ffmpeg-backup-")
            .tempdir_in(parent)
            .map_err(|error| {
                FfmpegError::storage(format!("failed to create {label} backup: {error}"))
            })?;
        let previous = backup.path().join("previous");
        let had_target = match fs::symlink_metadata(target) {
            Ok(metadata) if metadata.file_type().is_dir() => true,
            Ok(_) => {
                return Err(FfmpegError::conformance(format!(
                    "{label} target is not a regular directory: {}",
                    target.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => {
                return Err(FfmpegError::storage(format!(
                    "failed to inspect {label} target '{}': {error}",
                    target.display()
                )));
            }
        };
        if had_target {
            fs::rename(target, &previous).map_err(|error| {
                FfmpegError::storage(format!("failed to stage previous {label}: {error}"))
            })?;
            if let Err(error) = fs::rename(source, target) {
                return match fs::rename(&previous, target) {
                    Ok(()) => Err(FfmpegError::storage(format!(
                        "failed to publish {label}: {error}; the previous output was restored"
                    ))),
                    Err(rollback) => {
                        let recovery = backup.keep();
                        Err(FfmpegError::storage(format!(
                            "failed to publish {label}: {error}; rollback failed: {rollback}; recovery output remains at {}",
                            recovery.join("previous").display()
                        )))
                    }
                };
            }
            Ok(())
        } else {
            fs::rename(source, target).map_err(|error| {
                FfmpegError::storage(format!("failed to publish {label}: {error}"))
            })
        }
    }

    fn regular_nonempty_file(path: &Path) -> Result<bool, FfmpegError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => Ok(metadata.file_type().is_file()
                && metadata.len() > 0
                && metadata.len() <= MAX_LIBRARY_BYTES),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(FfmpegError::storage(format!(
                "failed to inspect file '{}': {error}",
                path.display()
            ))),
        }
    }

    fn regular_file_equals(path: &Path, expected: &[u8]) -> Result<bool, FfmpegError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(FfmpegError::storage(format!(
                    "failed to inspect metadata '{}': {error}",
                    path.display()
                )));
            }
        };
        if !metadata.file_type().is_file()
            || metadata.len() > MAX_METADATA_BYTES
            || metadata.len() != expected.len() as u64
        {
            return Ok(false);
        }
        fs::read(path)
            .map(|actual| actual == expected)
            .map_err(|error| {
                FfmpegError::storage(format!(
                    "failed to read metadata '{}': {error}",
                    path.display()
                ))
            })
    }

    fn joined_search_path(
        local: &Path,
        existing: Option<OsString>,
    ) -> Result<OsString, FfmpegError> {
        let mut paths = vec![local.to_path_buf()];
        if let Some(existing) = existing {
            paths.extend(env::split_paths(&existing));
        }
        env::join_paths(paths).map_err(|error| {
            FfmpegError::conformance(format!("failed to construct PKG_CONFIG_PATH: {error}"))
        })
    }

    fn environment_text(name: &str) -> Result<Option<String>, FfmpegError> {
        match env::var(name) {
            Ok(value) if value.is_empty() => Ok(None),
            Ok(value) => Ok(Some(value)),
            Err(env::VarError::NotPresent) => Ok(None),
            Err(env::VarError::NotUnicode(_)) => Err(FfmpegError::compatibility(format!(
                "{name} must be valid UTF-8"
            ))),
        }
    }

    fn check_cancellation(
        cancellation: &external_process::InterruptDeferral,
        label: &str,
    ) -> Result<(), FfmpegError> {
        if cancellation.is_cancelled() {
            Err(FfmpegError::worker(format!("{label} was cancelled")))
        } else {
            Ok(())
        }
    }

    fn map_source_error(error: source_archive::SourceArchiveError) -> FfmpegError {
        match error.kind() {
            SourceArchiveErrorKind::Storage => FfmpegError::storage(error.to_string()),
            SourceArchiveErrorKind::Conformance => FfmpegError::conformance(error.to_string()),
            SourceArchiveErrorKind::Worker => FfmpegError::worker(error.to_string()),
        }
    }

    fn map_process_error(error: external_process::ExternalProcessError) -> FfmpegError {
        match error.kind() {
            ExternalProcessErrorKind::Compatibility => {
                FfmpegError::compatibility(error.to_string())
            }
            ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
                FfmpegError::worker(error.to_string())
            }
        }
    }

    fn map_ios_error(error: crate::ios::IosError) -> FfmpegError {
        match error.kind() {
            crate::ios::IosErrorKind::Storage => FfmpegError::storage(error.to_string()),
            crate::ios::IosErrorKind::Compatibility => {
                FfmpegError::compatibility(error.to_string())
            }
            crate::ios::IosErrorKind::Conformance => FfmpegError::conformance(error.to_string()),
            crate::ios::IosErrorKind::Worker => FfmpegError::worker(error.to_string()),
        }
    }

    fn diagnostics_error(error: io::Error) -> FfmpegError {
        FfmpegError::storage(format!("failed to write Apple FFmpeg diagnostics: {error}"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn profile() -> NativeFfmpegProfile {
            NativeFfmpegProfile {
                build_profile: "custom".to_owned(),
                declared_profile: "fixture".to_owned(),
                declared_platform: "ios",
                profile_hash: "fixture-hash".to_owned(),
                tls_backend: "none".to_owned(),
                enable_dash: false,
                libraries: vec!["avutil".to_owned()],
                demuxers: Vec::new(),
                muxers: Vec::new(),
                protocols: Vec::new(),
                decoders: Vec::new(),
                parsers: Vec::new(),
                bsfs: Vec::new(),
                extra_configure_args: Vec::new(),
                force: false,
                forbid_network: true,
                forbid_openssl: true,
            }
        }

        fn write_valid_prebuilt(output: &Path) -> PathBuf {
            let slice = output.join("ios");
            fs::create_dir_all(slice.join("include/libavutil")).expect("create header directory");
            fs::create_dir_all(slice.join("lib/arm64")).expect("create library directory");
            fs::write(slice.join("include/libavutil/avutil.h"), b"header").expect("write header");
            fs::write(slice.join("lib/arm64/libavutil.a"), b"static")
                .expect("write static library");
            fs::write(slice.join("lib/arm64/libavutil.dylib"), b"dynamic")
                .expect("write dynamic library");
            fs::write(
                slice.join("vesper-ffmpeg-build-metadata.txt"),
                b"fixture metadata",
            )
            .expect("write metadata");
            let checksum = format!("{:x}", Sha256::digest(b"dynamic"));
            fs::write(
                slice.join("vesper-ffmpeg-library-sha256.txt"),
                format!("avutil_sha256={checksum}\n"),
            )
            .expect("write checksum record");
            slice
        }

        #[test]
        fn generation_snapshot_preserves_unselected_slice() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let output = temporary.path().join("output");
            let device = output.join("ios");
            let simulator = output.join("ios-simulator");
            fs::create_dir_all(device.join("lib/arm64")).expect("create device fixture");
            fs::create_dir_all(&simulator).expect("create simulator fixture");
            fs::write(device.join("marker"), b"old-device").expect("write device marker");
            fs::write(simulator.join("marker"), b"preserved-simulator")
                .expect("write simulator marker");
            fs::write(device.join("lib/arm64/libavutil.1.dylib"), b"dylib")
                .expect("write dylib fixture");
            std::os::unix::fs::symlink(
                "libavutil.1.dylib",
                device.join("lib/arm64/libavutil.dylib"),
            )
            .expect("create dylib alias");

            let generation = temporary.path().join("generation");
            fs::create_dir(&generation).expect("create generation staging");
            seed_generation(&output, &generation).expect("snapshot existing generation");
            fs::write(generation.join("ios/marker"), b"new-device")
                .expect("replace selected slice");
            publish_generation(&generation, &output).expect("publish complete generation");

            assert_eq!(
                fs::read(output.join("ios/marker")).expect("read device marker"),
                b"new-device"
            );
            assert_eq!(
                fs::read(output.join("ios-simulator/marker")).expect("read simulator marker"),
                b"preserved-simulator"
            );
            assert_eq!(
                fs::read_link(output.join("ios/lib/arm64/libavutil.dylib"))
                    .expect("read copied dylib alias"),
                PathBuf::from("libavutil.1.dylib")
            );
        }

        #[test]
        fn generation_publication_restores_previous_output_on_failure() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let output = temporary.path().join("output");
            let generation = temporary.path().join("generation");
            fs::create_dir(&output).expect("create existing output");
            fs::create_dir(&generation).expect("create generation staging");
            fs::write(output.join("marker"), b"previous").expect("write previous marker");
            fs::write(generation.join("marker"), b"replacement").expect("write replacement marker");

            let error = publish_generation_with_hook(&generation, &output, || {
                Err(FfmpegError::worker("injected publication failure"))
            })
            .expect_err("reject injected publication failure");

            assert_eq!(error.kind(), crate::ffmpeg::FfmpegErrorKind::Worker);
            assert_eq!(
                fs::read(output.join("marker")).expect("read restored marker"),
                b"previous"
            );
            assert_eq!(
                fs::read(generation.join("marker")).expect("read unpublished generation"),
                b"replacement"
            );
        }

        #[test]
        fn generation_snapshot_rejects_escaping_symlinks() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let output = temporary.path().join("output");
            let generation = temporary.path().join("generation");
            fs::create_dir(&output).expect("create existing output");
            fs::create_dir(&generation).expect("create generation staging");
            std::os::unix::fs::symlink("../outside", output.join("unsafe"))
                .expect("create unsafe symlink");

            let error = seed_generation(&output, &generation)
                .expect_err("reject symlink escaping the generation root");

            assert_eq!(error.kind(), crate::ffmpeg::FfmpegErrorKind::Conformance);
        }

        #[test]
        fn strict_verification_accepts_complete_slice() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let output = temporary.path().join("output");
            let slice = write_valid_prebuilt(&output);

            let metadata = verify_prebuilts(&output, &["ios-arm64".to_owned()], &profile())
                .expect("verify complete Apple prebuilt");

            assert_eq!(
                metadata,
                vec![slice.join("vesper-ffmpeg-build-metadata.txt")]
            );
        }

        #[test]
        fn strict_verification_rejects_missing_headers() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let output = temporary.path().join("output");
            let slice = write_valid_prebuilt(&output);
            fs::remove_dir_all(slice.join("include/libavutil")).expect("remove headers");

            let error = verify_prebuilts(&output, &["ios-arm64".to_owned()], &profile())
                .expect_err("reject prebuilt without headers");

            assert_eq!(error.kind(), crate::ffmpeg::FfmpegErrorKind::Conformance);
        }

        #[test]
        fn strict_verification_rejects_extra_checksum_records() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let output = temporary.path().join("output");
            let slice = write_valid_prebuilt(&output);
            let checksum = format!("{:x}", Sha256::digest(b"dynamic"));
            fs::write(
                slice.join("vesper-ffmpeg-library-sha256.txt"),
                format!("avutil_sha256={checksum}\navcodec_sha256={checksum}\n"),
            )
            .expect("write drifted checksum record");

            let error = verify_prebuilts(&output, &["ios-arm64".to_owned()], &profile())
                .expect_err("reject checksum record drift");

            assert_eq!(error.kind(), crate::ffmpeg::FfmpegErrorKind::Conformance);
        }

        #[test]
        fn sdk_path_resolves_xcode_style_symlink() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let canonical = temporary.path().join("iPhoneOS.sdk");
            fs::create_dir(&canonical).expect("create canonical SDK directory");
            let versioned = temporary.path().join("iPhoneOS26.5.sdk");
            std::os::unix::fs::symlink("iPhoneOS.sdk", &versioned)
                .expect("create versioned SDK symlink");

            let resolved = resolve_sdk_path(&versioned, "iphoneos").expect("resolve SDK symlink");

            assert_eq!(
                resolved,
                fs::canonicalize(canonical).expect("canonicalize SDK")
            );
        }

        #[test]
        fn sdk_path_rejects_dangling_symlink() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let dangling = temporary.path().join("iPhoneOS26.5.sdk");
            std::os::unix::fs::symlink("missing.sdk", &dangling)
                .expect("create dangling SDK symlink");

            let error =
                resolve_sdk_path(&dangling, "iphoneos").expect_err("reject dangling SDK symlink");

            assert_eq!(error.kind(), crate::ffmpeg::FfmpegErrorKind::Compatibility);
        }

        #[test]
        fn sdk_path_rejects_non_directory() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let file = temporary.path().join("iPhoneOS26.5.sdk");
            fs::write(&file, b"not an SDK").expect("write non-directory SDK fixture");

            let error =
                resolve_sdk_path(&file, "iphoneos").expect_err("reject non-directory SDK path");

            assert_eq!(error.kind(), crate::ffmpeg::FfmpegErrorKind::Compatibility);
        }

        #[test]
        fn library_validation_accepts_chained_relative_symlinks() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let parent = temporary.path().join("lib");
            fs::create_dir(&parent).expect("create library directory");
            fs::write(parent.join("libavcodec.62.28.102.dylib"), b"dylib")
                .expect("write versioned library");
            std::os::unix::fs::symlink(
                "libavcodec.62.28.102.dylib",
                parent.join("libavcodec.62.dylib"),
            )
            .expect("create major-version library symlink");
            std::os::unix::fs::symlink("libavcodec.62.dylib", parent.join("libavcodec.dylib"))
                .expect("create unversioned library symlink");

            validate_library_entry(&parent.join("libavcodec.dylib"), &parent)
                .expect("accept chained library symlink");
        }

        #[test]
        fn library_validation_rejects_escaping_chained_symlinks() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let parent = temporary.path().join("lib");
            fs::create_dir(&parent).expect("create library directory");
            fs::write(temporary.path().join("outside.dylib"), b"outside")
                .expect("write outside library");
            std::os::unix::fs::symlink("../outside.dylib", parent.join("inner.dylib"))
                .expect("create escaping library symlink");
            std::os::unix::fs::symlink("inner.dylib", parent.join("libavcodec.dylib"))
                .expect("create outer library symlink");

            let error = validate_library_entry(&parent.join("libavcodec.dylib"), &parent)
                .expect_err("reject escaping chained library symlink");

            assert_eq!(error.kind(), crate::ffmpeg::FfmpegErrorKind::Conformance);
        }

        #[test]
        fn library_validation_rejects_symlink_cycles() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let parent = temporary.path().join("lib");
            fs::create_dir(&parent).expect("create library directory");
            std::os::unix::fs::symlink("second.dylib", parent.join("first.dylib"))
                .expect("create first cycle link");
            std::os::unix::fs::symlink("first.dylib", parent.join("second.dylib"))
                .expect("create second cycle link");
            std::os::unix::fs::symlink("first.dylib", parent.join("libavcodec.dylib"))
                .expect("create outer cycle link");

            let error = validate_library_entry(&parent.join("libavcodec.dylib"), &parent)
                .expect_err("reject symlink cycle");

            assert_eq!(error.kind(), crate::ffmpeg::FfmpegErrorKind::Conformance);
        }

        #[test]
        fn normalize_staged_layout_validates_before_moving_symlink_targets() {
            let temporary = tempfile::tempdir().expect("create temporary directory");
            let staged = temporary.path().join("staged");
            let library_directory = staged.join("lib");
            fs::create_dir_all(&library_directory).expect("create library directory");
            fs::create_dir_all(staged.join("include")).expect("create include directory");
            fs::write(
                library_directory.join("libavcodec.62.28.102.dylib"),
                b"dylib",
            )
            .expect("write versioned library");
            std::os::unix::fs::symlink(
                "libavcodec.62.28.102.dylib",
                library_directory.join("libavcodec.62.dylib"),
            )
            .expect("create major-version library symlink");
            std::os::unix::fs::symlink(
                "libavcodec.62.dylib",
                library_directory.join("libavcodec.dylib"),
            )
            .expect("create unversioned library symlink");

            normalize_staged_layout(&staged).expect("normalize staged library layout");

            assert_eq!(
                fs::read_link(staged.join("lib/arm64/libavcodec.dylib"))
                    .expect("read unversioned library symlink"),
                PathBuf::from("libavcodec.62.dylib")
            );
            assert!(
                fs::metadata(staged.join("lib/arm64/libavcodec.dylib"))
                    .expect("resolve normalized library")
                    .is_file()
            );
            assert!(!staged.join(".vesper-unarchived-lib").exists());
        }
    }
}
