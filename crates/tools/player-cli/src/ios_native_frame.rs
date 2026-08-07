use std::io::Write;
use std::path::Path;

use crate::ios::IosError;

pub(crate) fn ensure_supported_host() -> Result<(), IosError> {
    if !cfg!(target_os = "macos") {
        return Err(IosError::compatibility(
            "iOS native-frame pipeline verification requires macOS",
        ));
    }
    if std::env::consts::ARCH != "aarch64" {
        return Err(IosError::compatibility(
            "iOS native-frame pipeline verification requires Apple Silicon",
        ));
    }
    Ok(())
}

pub(crate) fn verify(
    root: &Path,
    tokens: &[&str],
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), IosError> {
    ensure_supported_host()?;

    #[cfg(target_os = "macos")]
    {
        implementation::verify(root, tokens, output, diagnostics)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, tokens, output, diagnostics);
        unreachable!("the host gate rejects non-macOS verification")
    }
}

#[cfg(target_os = "macos")]
mod implementation {
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::IosError;
    use crate::external_process::{
        self, BoundedProcessOutput, ExternalProcessErrorKind, InterruptDeferral,
    };
    use crate::ios_plugin::{self, IosPluginSlice};

    const MAX_PROCESS_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
    const MAX_STAGED_DYLIB_BYTES: u64 = 512 * 1024 * 1024;
    const MAX_RUNTIME_DIRECTORY_ENTRIES: usize = 256;
    const MAX_XCTESTRUN_DIRECTORY_ENTRIES: usize = 256;
    const SIMULATOR_SLICE: IosPluginSlice = IosPluginSlice::SimulatorArm64;
    const SMOKE_TEST: &str = "VesperPlayerKitTests/VesperPlayerControllerStateTests/testNativeFramePipelineRealPluginPlaybackPresentsSeeksAndReleasesLocalMp4";
    const SUMMARY_MARKER: &[u8] = b"real iOS native-frame smoke presentedFrames=";
    const FAILURE_MARKERS: [(&[u8], &str); 2] = [
        (
            b"native-frame release failed",
            "iOS native-frame smoke reported a frame release failure",
        ),
        (
            b"invalid iOS native-frame pending frame handle",
            "iOS native-frame smoke reported an invalid pending-frame handle",
        ),
    ];
    const PRIVATE_CHILD_ENVIRONMENT: [&str; 16] = [
        "VESPER_APPLE_SH_INCLUDED",
        "VESPER_COMMON_SH_INCLUDED",
        "VESPER_FFMPEG_SH_INCLUDED",
        "VESPER_FFMPEG_PROFILE_SH_INCLUDED",
        "VESPER_FFMPEG_VALIDATE_SH_INCLUDED",
        "VESPER_IOS_FRAMEWORK_SH_INCLUDED",
        "VESPER_IOS_FFI_PREBUILT",
        "VESPER_IOS_NATIVE_FRAME_SMOKE_CONFIG",
        "VESPER_IOS_NATIVE_FRAME_SMOKE_ENABLED",
        "VESPER_IOS_NATIVE_FRAME_STAGING_DIR",
        "VESPER_IOS_SOURCE_NORMALIZER_PLUGIN_PATH",
        "VESPER_IOS_DECODER_VIDEOTOOLBOX_PLUGIN_PATH",
        "VESPER_IOS_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH",
        "BASH_ENV",
        "ENV",
        "CDPATH",
    ];

    struct Tools {
        install_name_tool: PathBuf,
        otool: PathBuf,
        xcodebuild: PathBuf,
        xcodegen: PathBuf,
        plutil: PathBuf,
        xcrun: PathBuf,
    }

    #[derive(Default)]
    struct VerificationOutput {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    }

    #[derive(Clone, Copy)]
    enum BuildProfile {
        Debug,
        Release,
    }

    impl BuildProfile {
        const fn as_str(self) -> &'static str {
            match self {
                Self::Debug => "debug",
                Self::Release => "release",
            }
        }
    }

    pub(super) fn verify(
        root: &Path,
        tokens: &[&str],
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        let profile = parse_profile(tokens)?;
        let tools = resolve_tools()?;
        let cancellation = InterruptDeferral::start("iOS native-frame verification")
            .map_err(map_external_process_error)?;
        let result = verify_transaction(root, profile, &tools, &cancellation);
        let cancelled = cancellation.finish();
        if cancelled {
            return Err(IosError::worker(
                "iOS native-frame verification was cancelled",
            ));
        }
        let report = result?;
        diagnostics
            .write_all(&report.stderr)
            .map_err(output_error)?;
        output.write_all(&report.stdout).map_err(output_error)
    }

    fn parse_profile(tokens: &[&str]) -> Result<BuildProfile, IosError> {
        let mut profile = BuildProfile::Debug;
        for token in tokens {
            match *token {
                "debug" => profile = BuildProfile::Debug,
                "release" => profile = BuildProfile::Release,
                "swift-smoke" => {}
                _ => {
                    return Err(IosError::conformance(format!(
                        "unsupported iOS native-frame verification token: {token}"
                    )));
                }
            }
        }
        Ok(profile)
    }

    fn verify_transaction(
        root: &Path,
        profile: BuildProfile,
        tools: &Tools,
        cancellation: &InterruptDeferral,
    ) -> Result<VerificationOutput, IosError> {
        let wrapper =
            require_executable_file(&root.join("scripts/vesper"), "Vesper Rust CLI launcher")?;
        let temporary = tempfile::Builder::new()
            .prefix("vesper-ios-native-frame-")
            .tempdir()
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create private iOS native-frame verifier state: {error}"
                ))
            })?;
        let staging = temporary.path().join("staging");
        fs::create_dir(&staging).map_err(|error| {
            IosError::storage(format!(
                "failed to create iOS native-frame staging directory '{}': {error}",
                staging.display()
            ))
        })?;

        let mut report = VerificationOutput::default();
        let source = resolve_smoke_source(root, cancellation)?;
        let ffmpeg_output = ios_plugin::resolve_ffmpeg_output_directory_in_deferral(
            root,
            &[SIMULATOR_SLICE],
            &mut report.stderr,
            cancellation,
        )?;
        let runtime = ffmpeg_output.join("ios-simulator/lib/arm64");
        require_directory(&runtime, "Apple FFmpeg simulator runtime")?;

        build_plugins(
            root,
            &wrapper,
            profile,
            &ffmpeg_output,
            cancellation,
            &mut report,
        )?;
        copy_runtime_dylibs(&runtime, &staging, tools, cancellation)?;
        stage_plugin_dylibs(root, &staging, tools, cancellation)?;

        writeln!(
            report.stdout,
            "Using iOS native-frame smoke source: {}",
            source.display()
        )
        .map_err(output_error)?;
        writeln!(
            report.stdout,
            "Using staged iOS native-frame plugins: {}",
            staging.display()
        )
        .map_err(output_error)?;

        let config = temporary.path().join("smoke.plist");
        write_smoke_config(&config, &source, &staging, tools, cancellation)?;
        let destination = resolve_simulator_destination(tools, cancellation)?;
        run_xcode_smoke(
            root,
            &config,
            &temporary.path().join("xcode-derived-data"),
            &destination,
            tools,
            cancellation,
            &mut report,
        )?;
        Ok(report)
    }

    fn resolve_tools() -> Result<Tools, IosError> {
        Ok(Tools {
            install_name_tool: resolve_path_command("install_name_tool")?,
            otool: resolve_path_command("otool")?,
            xcodebuild: resolve_path_command("xcodebuild")?,
            xcodegen: resolve_path_command("xcodegen")?,
            plutil: require_executable_file(Path::new("/usr/bin/plutil"), "plutil")?,
            xcrun: require_executable_file(Path::new("/usr/bin/xcrun"), "xcrun")?,
        })
    }

    fn resolve_path_command(command: &str) -> Result<PathBuf, IosError> {
        use nix::unistd::{AccessFlags, access};

        let paths = env::var_os("PATH").unwrap_or_default();
        env::split_paths(&paths)
            .map(|directory| directory.join(command))
            .find(|candidate| {
                fs::metadata(candidate).is_ok_and(|metadata| metadata.is_file())
                    && access(candidate, AccessFlags::X_OK).is_ok()
            })
            .ok_or_else(|| IosError::compatibility(format!("Missing required command: {command}")))
    }

    fn require_executable_file(path: &Path, label: &str) -> Result<PathBuf, IosError> {
        use nix::unistd::{AccessFlags, access};

        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() || access(path, AccessFlags::X_OK).is_err() {
            return Err(IosError::compatibility(format!(
                "{label} must be an executable regular non-symlink file: {}",
                path.display()
            )));
        }
        Ok(path.to_path_buf())
    }

    fn require_directory(path: &Path, label: &str) -> Result<(), IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::compatibility(format!("Missing {label}: {} ({error})", path.display()))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(IosError::compatibility(format!(
                "{label} must be a regular non-symlink directory: {}",
                path.display()
            )));
        }
        Ok(())
    }

    fn resolve_smoke_source(
        root: &Path,
        cancellation: &InterruptDeferral,
    ) -> Result<PathBuf, IosError> {
        if let Some(source) = env::var_os("VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE") {
            let source = PathBuf::from(source);
            let metadata = fs::metadata(&source).map_err(|error| {
                IosError::conformance(format!(
                    "VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE points to a missing file: {} ({error})",
                    source.display()
                ))
            })?;
            if !metadata.is_file() {
                return Err(IosError::conformance(format!(
                    "VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE must point to a file: {}",
                    source.display()
                )));
            }
            return Ok(source);
        }

        let target = root.join("target");
        prepare_directory(&target, "iOS native-frame generated media directory")?;
        let generated = target.join("ios-native-frame-smoke-h264-aac.mp4");
        match fs::symlink_metadata(&generated) {
            Ok(metadata)
                if metadata.file_type().is_file()
                    && metadata.len() > 0
                    && metadata.len() <= MAX_STAGED_DYLIB_BYTES =>
            {
                return Ok(generated);
            }
            Ok(_) => {
                return Err(IosError::conformance(format!(
                    "cached iOS native-frame smoke source is not a bounded regular file: {}",
                    generated.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(IosError::storage(format!(
                    "failed to inspect cached iOS native-frame smoke source '{}': {error}",
                    generated.display()
                )));
            }
        }
        let ffmpeg = resolve_path_command("ffmpeg").map_err(|_| {
            IosError::compatibility(
                "ffmpeg is required to generate the iOS native-frame smoke source; install ffmpeg or set VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE.",
            )
        })?;
        let temporary = tempfile::Builder::new()
            .prefix(".ios-native-frame-smoke-")
            .suffix(".mp4")
            .tempfile_in(&target)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create temporary iOS native-frame smoke source: {error}"
                ))
            })?;
        let mut command = Command::new(ffmpeg);
        command.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24:duration=3",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=3",
            "-c:v",
            "libx264",
            "-profile:v",
            "baseline",
            "-level:v",
            "3.1",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "96k",
            "-shortest",
            "-movflags",
            "+faststart",
        ]);
        command.arg(temporary.path());
        let result = run_process(
            &mut command,
            "native-frame smoke media generation",
            cancellation,
        )?;
        ensure_success(result, "native-frame smoke media generation")?;
        let metadata = fs::metadata(temporary.path()).map_err(|error| {
            IosError::conformance(format!(
                "ffmpeg did not create the iOS native-frame smoke source '{}': {error}",
                temporary.path().display()
            ))
        })?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_STAGED_DYLIB_BYTES {
            return Err(IosError::conformance(format!(
                "ffmpeg created an invalid iOS native-frame smoke source: {}",
                temporary.path().display()
            )));
        }
        temporary.persist_noclobber(&generated).map_err(|error| {
            IosError::storage(format!(
                "failed to atomically publish iOS native-frame smoke source '{}': {}",
                generated.display(),
                error.error
            ))
        })?;
        Ok(generated)
    }

    fn prepare_directory(path: &Path, label: &str) -> Result<(), IosError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
            Ok(_) => Err(IosError::storage(format!(
                "{label} must be a regular non-symlink directory: {}",
                path.display()
            ))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir_all(path)
                .map_err(|error| {
                    IosError::storage(format!(
                        "failed to create {label} '{}': {error}",
                        path.display()
                    ))
                }),
            Err(error) => Err(IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))),
        }
    }

    fn build_plugins(
        root: &Path,
        wrapper: &Path,
        profile: BuildProfile,
        ffmpeg_output: &Path,
        cancellation: &InterruptDeferral,
        report: &mut VerificationOutput,
    ) -> Result<(), IosError> {
        let build_root = root.join("target/ios-native-frame-smoke");
        let current_cli = env::current_exe().map_err(|error| {
            IosError::storage(format!("failed to resolve the running Vesper CLI: {error}"))
        })?;
        for (command_name, output_directory, skip_prebuilds) in [
            (
                "source-normalizer-plugin",
                build_root.join("player-source-normalizer-ffmpeg"),
                true,
            ),
            (
                "decoder-videotoolbox-plugin",
                build_root.join("player-decoder-videotoolbox"),
                false,
            ),
            (
                "frame-processor-plugin",
                build_root.join("player-frame-processor-diagnostic"),
                false,
            ),
        ] {
            let mut command = Command::new(wrapper);
            command
                .current_dir(root)
                .args(["ios", command_name])
                .arg(&output_directory)
                .arg(profile.as_str())
                .arg(SIMULATOR_SLICE.as_str())
                .env("VESPER_REPO_ROOT", root)
                .env("VESPER_CLI", &current_cli)
                .env("VESPER_APPLE_FFMPEG_OUTPUT_DIR", ffmpeg_output);
            if skip_prebuilds {
                command.env("VESPER_SKIP_APPLE_FFMPEG_PREBUILDS", "1");
            }
            clear_private_child_environment(&mut command);
            let result = run_process(
                &mut command,
                &format!("iOS native-frame {command_name} build"),
                cancellation,
            )?;
            require_cli_success(
                result,
                &format!("iOS native-frame {command_name} build"),
                report,
            )?;
        }
        Ok(())
    }

    fn copy_runtime_dylibs(
        source: &Path,
        staging: &Path,
        tools: &Tools,
        cancellation: &InterruptDeferral,
    ) -> Result<(), IosError> {
        let canonical_source = fs::canonicalize(source).map_err(|error| {
            IosError::storage(format!(
                "failed to resolve Apple FFmpeg simulator runtime '{}': {error}",
                source.display()
            ))
        })?;
        let mut inputs = Vec::new();
        let mut entry_count = 0_usize;
        for entry in fs::read_dir(source).map_err(|error| {
            IosError::storage(format!(
                "failed to enumerate Apple FFmpeg simulator runtime '{}': {error}",
                source.display()
            ))
        })? {
            entry_count = entry_count.saturating_add(1);
            if entry_count > MAX_RUNTIME_DIRECTORY_ENTRIES {
                return Err(IosError::conformance(format!(
                    "Apple FFmpeg simulator runtime contains more than {MAX_RUNTIME_DIRECTORY_ENTRIES} entries"
                )));
            }
            let entry = entry.map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect Apple FFmpeg simulator runtime entry: {error}"
                ))
            })?;
            let name = entry.file_name();
            let bytes = name.as_encoded_bytes();
            if bytes.starts_with(b"lib") && bytes.ends_with(b".dylib") {
                inputs.push((name, entry.path()));
            }
        }
        inputs.sort_by(|left, right| left.0.as_encoded_bytes().cmp(right.0.as_encoded_bytes()));
        if inputs.is_empty() {
            return Err(IosError::compatibility(format!(
                "Missing FFmpeg runtime dylibs in: {}",
                source.display()
            )));
        }

        let mut staged = Vec::with_capacity(inputs.len());
        for (name, input) in inputs {
            let metadata = fs::symlink_metadata(&input).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect FFmpeg runtime dylib '{}': {error}",
                    input.display()
                ))
            })?;
            let source = fs::canonicalize(&input).map_err(|error| {
                IosError::storage(format!(
                    "failed to resolve FFmpeg runtime dylib '{}': {error}",
                    input.display()
                ))
            })?;
            if source.parent() != Some(canonical_source.as_path()) {
                return Err(IosError::conformance(format!(
                    "FFmpeg runtime dylib must resolve inside '{}': {}",
                    canonical_source.display(),
                    input.display()
                )));
            }
            if !metadata.file_type().is_file() && !metadata.file_type().is_symlink() {
                return Err(IosError::conformance(format!(
                    "FFmpeg runtime dylib must be a regular file or contained symlink: {}",
                    input.display()
                )));
            }
            copy_bounded_file(&source, &staging.join(&name), "FFmpeg runtime dylib")?;
            staged.push(staging.join(name));
        }
        for dylib in staged {
            prepare_staged_dylib(&dylib, tools, cancellation)?;
        }
        Ok(())
    }

    fn stage_plugin_dylibs(
        root: &Path,
        staging: &Path,
        tools: &Tools,
        cancellation: &InterruptDeferral,
    ) -> Result<(), IosError> {
        let build_root = root.join("target/ios-native-frame-smoke");
        for (directory, name) in [
            (
                "player-source-normalizer-ffmpeg",
                "libvesper_source_normalizer_ffmpeg.dylib",
            ),
            (
                "player-decoder-videotoolbox",
                "libvesper_decoder_videotoolbox.dylib",
            ),
            (
                "player-frame-processor-diagnostic",
                "libvesper_frame_processor_diagnostic.dylib",
            ),
        ] {
            let source = build_root
                .join(directory)
                .join("iphonesimulator")
                .join(name);
            let destination = staging.join(name);
            copy_bounded_file(&source, &destination, "iOS native-frame plugin dylib")?;
            prepare_staged_dylib(&destination, tools, cancellation)?;
        }
        Ok(())
    }

    fn copy_bounded_file(source: &Path, destination: &Path, label: &str) -> Result<(), IosError> {
        let metadata = fs::metadata(source).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                source.display()
            ))
        })?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_STAGED_DYLIB_BYTES {
            return Err(IosError::conformance(format!(
                "{label} must be a nonempty file no larger than {MAX_STAGED_DYLIB_BYTES} bytes: {}",
                source.display()
            )));
        }
        if destination.exists() {
            return Err(IosError::conformance(format!(
                "duplicate iOS native-frame staged dylib: {}",
                destination.display()
            )));
        }
        fs::copy(source, destination).map_err(|error| {
            IosError::storage(format!(
                "failed to copy {label} '{}' to '{}': {error}",
                source.display(),
                destination.display()
            ))
        })?;
        Ok(())
    }

    fn prepare_staged_dylib(
        dylib: &Path,
        tools: &Tools,
        cancellation: &InterruptDeferral,
    ) -> Result<(), IosError> {
        let name = dylib.file_name().ok_or_else(|| {
            IosError::conformance(format!(
                "staged dylib has no file name: {}",
                dylib.display()
            ))
        })?;
        let mut install_name = Command::new(&tools.install_name_tool);
        install_name
            .arg("-id")
            .arg(Path::new("@rpath").join(name))
            .arg(dylib);
        let result = run_process(
            &mut install_name,
            "staged dylib install-name update",
            cancellation,
        )?;
        ensure_success(result, "staged dylib install-name update")?;

        let mut inspect = Command::new(&tools.otool);
        inspect.args(["-l"]).arg(dylib);
        let result = run_process(&mut inspect, "staged dylib rpath inspection", cancellation)?;
        let result = ensure_success(result, "staged dylib rpath inspection")?;
        if !has_exact_loader_rpath(&result.stdout) {
            let mut add = Command::new(&tools.install_name_tool);
            add.args(["-add_rpath", "@loader_path"]).arg(dylib);
            let result = run_process(&mut add, "staged dylib rpath update", cancellation)?;
            ensure_success(result, "staged dylib rpath update")?;
        }
        Ok(())
    }

    fn write_smoke_config(
        path: &Path,
        source: &Path,
        staging: &Path,
        tools: &Tools,
        cancellation: &InterruptDeferral,
    ) -> Result<(), IosError> {
        if path.exists() {
            return Err(IosError::storage(format!(
                "owned iOS native-frame smoke config already exists: {}",
                path.display()
            )));
        }
        let mut create = Command::new(&tools.plutil);
        create.args(["-create", "xml1"]).arg(path);
        let result = run_process(
            &mut create,
            "native-frame smoke plist creation",
            cancellation,
        )?;
        ensure_success(result, "native-frame smoke plist creation")?;

        let runtime_profile =
            read_utf8_environment("VESPER_IOS_SOURCE_NORMALIZER_RUNTIME_PROFILE")?
                .unwrap_or_default();
        for (key, value) in [
            ("VESPER_IOS_NATIVE_FRAME_SMOKE_ENABLED", OsString::from("1")),
            (
                "VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE",
                source.as_os_str().to_os_string(),
            ),
            (
                "VESPER_IOS_SOURCE_NORMALIZER_PLUGIN_PATH",
                staging
                    .join("libvesper_source_normalizer_ffmpeg.dylib")
                    .into_os_string(),
            ),
            (
                "VESPER_IOS_DECODER_VIDEOTOOLBOX_PLUGIN_PATH",
                staging
                    .join("libvesper_decoder_videotoolbox.dylib")
                    .into_os_string(),
            ),
            (
                "VESPER_IOS_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH",
                staging
                    .join("libvesper_frame_processor_diagnostic.dylib")
                    .into_os_string(),
            ),
            (
                "VESPER_IOS_SOURCE_NORMALIZER_RUNTIME_PROFILE",
                runtime_profile.into(),
            ),
        ] {
            set_plist_string(path, key, &value, false, tools, cancellation)?;
        }
        Ok(())
    }

    fn run_xcode_smoke(
        root: &Path,
        config: &Path,
        derived_data: &Path,
        destination: &str,
        tools: &Tools,
        cancellation: &InterruptDeferral,
        report: &mut VerificationOutput,
    ) -> Result<(), IosError> {
        let project_directory = root.join("lib/ios/VesperPlayerKit");
        let manifest = project_directory.join("project.yml");
        let metadata = fs::symlink_metadata(&manifest).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect VesperPlayerKit XcodeGen manifest '{}': {error}",
                manifest.display()
            ))
        })?;
        if !metadata.file_type().is_file() {
            return Err(IosError::conformance(format!(
                "VesperPlayerKit XcodeGen manifest must be a regular non-symlink file: {}",
                manifest.display()
            )));
        }
        let mut generate = Command::new(&tools.xcodegen);
        generate
            .current_dir(&project_directory)
            .args(["generate", "--spec"])
            .arg(&manifest);
        clear_private_child_environment(&mut generate);
        let result = run_process(
            &mut generate,
            "VesperPlayerKit Xcode project generation",
            cancellation,
        )?;
        require_success(result, "VesperPlayerKit Xcode project generation", report)?;
        let project = project_directory.join("VesperPlayerKit.xcodeproj");
        require_directory(&project, "generated VesperPlayerKit Xcode project")?;
        let mut build = Command::new(&tools.xcodebuild);
        build
            .args(["build-for-testing", "-project"])
            .arg(&project)
            .args([
                "-scheme",
                "VesperPlayerKit",
                "-destination",
                destination,
                "-derivedDataPath",
            ])
            .arg(derived_data)
            .args(["CODE_SIGNING_ALLOWED=NO", "CODE_SIGNING_REQUIRED=NO"]);
        clear_private_child_environment(&mut build);
        let build_result = run_process(&mut build, "iOS native-frame Xcode build", cancellation)?;
        require_success(build_result, "iOS native-frame Xcode build", report)?;
        let xctestrun = discover_xctestrun(&derived_data.join("Build/Products"))?;
        set_plist_string(
            &xctestrun,
            "VesperPlayerKitTests.EnvironmentVariables.VESPER_IOS_NATIVE_FRAME_SMOKE_CONFIG",
            config.as_os_str(),
            true,
            tools,
            cancellation,
        )?;
        set_plist_string(
            &xctestrun,
            "VesperPlayerKitTests.EnvironmentVariables.VESPER_FRAME_PROCESSOR_DIAGNOSTIC_MODE",
            OsStr::new("noop"),
            true,
            tools,
            cancellation,
        )?;

        let mut test = Command::new(&tools.xcodebuild);
        test.args(["test-without-building", "-xctestrun"])
            .arg(&xctestrun)
            .args(["-destination", destination, "-only-testing", SMOKE_TEST]);
        clear_private_child_environment(&mut test);
        let result = run_process(&mut test, "iOS native-frame Xcode smoke", cancellation)?;
        let mut log = Vec::with_capacity(result.stdout.len() + result.stderr.len());
        log.extend_from_slice(&result.stdout);
        log.extend_from_slice(&result.stderr);
        let log_path = persist_log(&log)?;
        if !result.status.success() {
            return Err(IosError::worker(format!(
                "{}\nLog: {}",
                process_failure_detail(
                    "iOS native-frame Xcode smoke",
                    result.status.code(),
                    &result.stdout,
                    &result.stderr,
                ),
                log_path.display()
            )));
        }
        for (marker, message) in FAILURE_MARKERS {
            if contains_bytes(&log, marker) {
                return Err(IosError::conformance(format!(
                    "{message}.\nLog: {}",
                    log_path.display()
                )));
            }
        }
        match count_bytes(&log, SUMMARY_MARKER) {
            1 => {}
            0 => {
                return Err(IosError::conformance(format!(
                    "iOS native-frame smoke did not report its real playback summary.\nLog: {}",
                    log_path.display()
                )));
            }
            count => {
                return Err(IosError::conformance(format!(
                    "iOS native-frame smoke reported {count} playback summaries; expected exactly one.\nLog: {}",
                    log_path.display()
                )));
            }
        }
        writeln!(
            report.stdout,
            "Running iOS native-frame Swift smoke; log: {}",
            log_path.display()
        )
        .map_err(output_error)?;
        report.stdout.extend_from_slice(&result.stdout);
        report.stderr.extend_from_slice(&result.stderr);
        writeln!(
            report.stdout,
            "iOS native-frame Swift smoke passed; log: {}",
            log_path.display()
        )
        .map_err(output_error)
    }

    fn resolve_simulator_destination(
        tools: &Tools,
        cancellation: &InterruptDeferral,
    ) -> Result<String, IosError> {
        if let Some(destination) = read_utf8_environment("VESPER_IOS_NATIVE_FRAME_DESTINATION")? {
            if destination.is_empty()
                || destination.len() > 1024
                || destination.chars().any(char::is_control)
                || !destination.starts_with("platform=iOS Simulator,")
            {
                return Err(IosError::compatibility(
                    "VESPER_IOS_NATIVE_FRAME_DESTINATION must be a bounded iOS Simulator destination",
                ));
            }
            return Ok(destination);
        }

        let mut command = Command::new(&tools.xcrun);
        command.args(["simctl", "list", "devices", "available", "--json"]);
        let result = run_process(
            &mut command,
            "available iOS Simulator discovery",
            cancellation,
        )?;
        let result = ensure_success(result, "available iOS Simulator discovery")?;
        select_simulator_destination(&result.stdout)
    }

    fn select_simulator_destination(bytes: &[u8]) -> Result<String, IosError> {
        let document: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
            IosError::conformance(format!(
                "xcrun simctl returned invalid Simulator JSON: {error}"
            ))
        })?;
        let devices = document
            .get("devices")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                IosError::conformance("xcrun simctl omitted its Simulator device map")
            })?;
        let mut selected = None::<(Vec<u32>, String, String)>;
        for (runtime, records) in devices {
            let Some(version) = simulator_runtime_version(runtime) else {
                continue;
            };
            if version.first().is_none_or(|major| *major < 17) {
                continue;
            }
            let Some(records) = records.as_array() else {
                return Err(IosError::conformance(
                    "xcrun simctl returned a malformed Simulator device list",
                ));
            };
            for record in records {
                if record
                    .get("isAvailable")
                    .and_then(serde_json::Value::as_bool)
                    == Some(false)
                {
                    continue;
                }
                let Some(name) = record.get("name").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let Some(identifier) = record.get("udid").and_then(serde_json::Value::as_str)
                else {
                    continue;
                };
                if !name.starts_with("iPhone ") || uuid::Uuid::parse_str(identifier).is_err() {
                    continue;
                }
                let candidate = (version.clone(), name.to_owned(), identifier.to_owned());
                if selected.as_ref().is_none_or(|current| candidate > *current) {
                    selected = Some(candidate);
                }
            }
        }
        let (_, _, identifier) = selected.ok_or_else(|| {
            IosError::compatibility(
                "No available iPhone Simulator is installed for iOS native-frame verification",
            )
        })?;
        Ok(format!("platform=iOS Simulator,id={identifier}"))
    }

    fn simulator_runtime_version(identifier: &str) -> Option<Vec<u32>> {
        let version = identifier.strip_prefix("com.apple.CoreSimulator.SimRuntime.iOS-")?;
        let components = version
            .split('-')
            .map(str::parse::<u32>)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        (!components.is_empty()).then_some(components)
    }

    fn set_plist_string(
        plist: &Path,
        key: &str,
        value: &OsStr,
        replace_existing: bool,
        tools: &Tools,
        cancellation: &InterruptDeferral,
    ) -> Result<(), IosError> {
        let operation = if replace_existing {
            let mut inspect = Command::new(&tools.plutil);
            inspect.args(["-extract", key, "raw", "-o", "-"]).arg(plist);
            let result = run_process(&mut inspect, "plist value inspection", cancellation)?;
            if result.status.success() {
                "-replace"
            } else {
                "-insert"
            }
        } else {
            "-insert"
        };
        let mut command = Command::new(&tools.plutil);
        command
            .arg(operation)
            .arg(key)
            .arg("-string")
            .arg(value)
            .arg(plist);
        let result = run_process(&mut command, "plist string update", cancellation)?;
        ensure_success(result, "plist string update")?;
        Ok(())
    }

    fn discover_xctestrun(directory: &Path) -> Result<PathBuf, IosError> {
        let mut matches = Vec::new();
        let mut entry_count = 0_usize;
        for entry in fs::read_dir(directory).map_err(|error| {
            IosError::conformance(format!(
                "failed to enumerate VesperPlayerKit xctestrun files under '{}': {error}",
                directory.display()
            ))
        })? {
            entry_count = entry_count.saturating_add(1);
            if entry_count > MAX_XCTESTRUN_DIRECTORY_ENTRIES {
                return Err(IosError::conformance(format!(
                    "VesperPlayerKit Xcode products contain more than {MAX_XCTESTRUN_DIRECTORY_ENTRIES} entries"
                )));
            }
            let entry = entry.map_err(|error| {
                IosError::storage(format!("failed to inspect Xcode product entry: {error}"))
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect Xcode product '{}': {error}",
                    path.display()
                ))
            })?;
            if metadata.file_type().is_file() && path.extension() == Some(OsStr::new("xctestrun")) {
                matches.push(path);
            }
        }
        match matches.as_slice() {
            [path] => Ok(path.clone()),
            _ => Err(IosError::conformance(format!(
                "Expected exactly one VesperPlayerKit xctestrun file, found {} under: {}",
                matches.len(),
                directory.display()
            ))),
        }
    }

    fn persist_log(bytes: &[u8]) -> Result<PathBuf, IosError> {
        let mut temporary = tempfile::Builder::new()
            .prefix("vesper-ios-native-frame-smoke.")
            .suffix(".log")
            .tempfile()
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS native-frame smoke log: {error}"
                ))
            })?;
        temporary
            .write_all(bytes)
            .and_then(|()| temporary.flush())
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to write iOS native-frame smoke log: {error}"
                ))
            })?;
        let (_file, path) = temporary.keep().map_err(|error| {
            IosError::storage(format!(
                "failed to preserve iOS native-frame smoke log: {}",
                error.error
            ))
        })?;
        Ok(path)
    }

    fn run_process(
        command: &mut Command,
        label: &str,
        cancellation: &InterruptDeferral,
    ) -> Result<BoundedProcessOutput, IosError> {
        external_process::run_interruptible_capture_in_deferral(
            command,
            label,
            MAX_PROCESS_OUTPUT_BYTES,
            MAX_PROCESS_OUTPUT_BYTES,
            cancellation,
        )
        .map_err(map_external_process_error)
    }

    fn require_success(
        result: BoundedProcessOutput,
        label: &str,
        report: &mut VerificationOutput,
    ) -> Result<BoundedProcessOutput, IosError> {
        let result = ensure_success(result, label)?;
        report.stdout.extend_from_slice(&result.stdout);
        report.stderr.extend_from_slice(&result.stderr);
        Ok(result)
    }

    fn require_cli_success(
        result: BoundedProcessOutput,
        label: &str,
        report: &mut VerificationOutput,
    ) -> Result<BoundedProcessOutput, IosError> {
        let result = ensure_cli_success(result, label)?;
        report.stdout.extend_from_slice(&result.stdout);
        report.stderr.extend_from_slice(&result.stderr);
        Ok(result)
    }

    fn ensure_success(
        result: BoundedProcessOutput,
        label: &str,
    ) -> Result<BoundedProcessOutput, IosError> {
        if result.status.success() {
            Ok(result)
        } else {
            Err(IosError::worker(process_failure_detail(
                label,
                result.status.code(),
                &result.stdout,
                &result.stderr,
            )))
        }
    }

    fn ensure_cli_success(
        result: BoundedProcessOutput,
        label: &str,
    ) -> Result<BoundedProcessOutput, IosError> {
        if result.status.success() {
            return Ok(result);
        }
        let detail =
            process_failure_detail(label, result.status.code(), &result.stdout, &result.stderr);
        Err(match result.status.code() {
            Some(3) => IosError::storage(detail),
            Some(4) => IosError::compatibility(detail),
            Some(5) => IosError::conformance(detail),
            Some(6) | None => IosError::worker(detail),
            _ => IosError::worker(detail),
        })
    }

    fn process_failure_detail(
        label: &str,
        status: Option<i32>,
        stdout: &[u8],
        stderr: &[u8],
    ) -> String {
        let stdout = String::from_utf8_lossy(stdout);
        let stderr = String::from_utf8_lossy(stderr);
        let stdout = stdout.trim_end_matches('\n');
        let stderr = stderr.trim_end_matches('\n');
        match (stdout.is_empty(), stderr.is_empty()) {
            (false, false) => format!("{stdout}\n{stderr}"),
            (false, true) => stdout.to_owned(),
            (true, false) => stderr.to_owned(),
            (true, true) => format!("{label} failed with status {}", status.unwrap_or(-1)),
        }
    }

    fn map_external_process_error(error: external_process::ExternalProcessError) -> IosError {
        match error.kind() {
            ExternalProcessErrorKind::Compatibility => IosError::compatibility(error.to_string()),
            ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
                IosError::worker(error.to_string())
            }
        }
    }

    fn clear_private_child_environment(command: &mut Command) {
        command.env("LC_ALL", "C");
        for name in PRIVATE_CHILD_ENVIRONMENT {
            command.env_remove(name);
        }
    }

    fn read_utf8_environment(name: &str) -> Result<Option<String>, IosError> {
        match env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(env::VarError::NotPresent) => Ok(None),
            Err(env::VarError::NotUnicode(_)) => Err(IosError::compatibility(format!(
                "{name} must be valid UTF-8"
            ))),
        }
    }

    fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
        !needle.is_empty()
            && haystack
                .windows(needle.len())
                .any(|window| window == needle)
    }

    fn count_bytes(haystack: &[u8], needle: &[u8]) -> usize {
        if needle.is_empty() {
            return 0;
        }
        haystack
            .windows(needle.len())
            .filter(|window| *window == needle)
            .count()
    }

    fn has_exact_loader_rpath(output: &[u8]) -> bool {
        output.split(|byte| *byte == b'\n').any(|line| {
            let Some(offset) = line
                .trim_ascii()
                .strip_prefix(b"path @loader_path (offset ")
                .and_then(|value| value.strip_suffix(b")"))
            else {
                return false;
            };
            !offset.is_empty() && offset.iter().all(u8::is_ascii_digit)
        })
    }

    fn output_error(error: io::Error) -> IosError {
        IosError::storage(format!(
            "failed to write iOS native-frame verification output: {error}"
        ))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn simulator_selection_uses_newest_available_iphone_runtime() {
            let document = br#"{
                "devices": {
                    "com.apple.CoreSimulator.SimRuntime.iOS-18-5": [
                        {"name":"iPhone 16","udid":"00000000-0000-0000-0000-000000000016","isAvailable":true}
                    ],
                    "com.apple.CoreSimulator.SimRuntime.iOS-26-1": [
                        {"name":"iPad Pro","udid":"00000000-0000-0000-0000-000000000099","isAvailable":true},
                        {"name":"iPhone 17","udid":"00000000-0000-0000-0000-000000000017","isAvailable":true},
                        {"name":"iPhone 17 Pro","udid":"00000000-0000-0000-0000-000000000018","isAvailable":false}
                    ]
                }
            }"#;

            assert_eq!(
                select_simulator_destination(document).expect("select available iPhone"),
                "platform=iOS Simulator,id=00000000-0000-0000-0000-000000000017"
            );
        }

        #[test]
        fn simulator_selection_rejects_malformed_or_missing_devices() {
            for document in [
                br#"{"devices":[]}"#.as_slice(),
                br#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-26-1":{}}}"#
                    .as_slice(),
                br#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-26-1":[{"name":"iPad Pro","udid":"00000000-0000-0000-0000-000000000099","isAvailable":true}]}}"#
                    .as_slice(),
                br#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-16-4":[{"name":"iPhone 14","udid":"00000000-0000-0000-0000-000000000014","isAvailable":true}]}}"#
                    .as_slice(),
            ] {
                assert!(select_simulator_destination(document).is_err());
            }
        }

        #[test]
        fn loader_rpath_matching_requires_one_exact_otool_record() {
            assert!(has_exact_loader_rpath(
                b"    path @loader_path (offset 12)\n"
            ));
            for output in [
                b"path @loader_path/.. (offset 12)\n".as_slice(),
                b"path @loader_path (offset 12) trailing\n".as_slice(),
                b"name contains @loader_path\n".as_slice(),
            ] {
                assert!(!has_exact_loader_rpath(output));
            }
        }
    }
}
