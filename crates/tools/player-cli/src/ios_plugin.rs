use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::ios::IosError;

pub(crate) const IOS_PLUGIN_SPECS: [IosPluginSpec; 4] = [
    IosPluginSpec {
        id: IosPluginId::RemuxFfmpeg,
        crate_name: "player-remux-ffmpeg",
        dylib_name: "libvesper_remux_ffmpeg.dylib",
        framework_name: "VesperPlayerRemuxFfmpegPlugin",
        bundle_identifier: "io.github.ikaros.vesper.player.remux-ffmpeg-plugin",
        build_directory: "player-remux-ffmpeg-plugin",
        release_ffmpeg_profile: Some("default"),
        uses_ffmpeg: true,
        link_headerpad: true,
        description: "remux plugin",
    },
    IosPluginSpec {
        id: IosPluginId::SourceNormalizerFfmpeg,
        crate_name: "player-source-normalizer-ffmpeg",
        dylib_name: "libvesper_source_normalizer_ffmpeg.dylib",
        framework_name: "VesperPlayerSourceNormalizerFfmpegPlugin",
        bundle_identifier: "io.github.ikaros.vesper.player.source-normalizer-ffmpeg-plugin",
        build_directory: "player-source-normalizer-ffmpeg-plugin",
        release_ffmpeg_profile: Some("source-normalizer"),
        uses_ffmpeg: true,
        link_headerpad: false,
        description: "source normalizer plugin",
    },
    IosPluginSpec {
        id: IosPluginId::DecoderVideoToolbox,
        crate_name: "player-decoder-videotoolbox",
        dylib_name: "libvesper_decoder_videotoolbox.dylib",
        framework_name: "VesperPlayerDecoderVideoToolboxPlugin",
        bundle_identifier: "io.github.ikaros.vesper.player.decoder-videotoolbox-plugin",
        build_directory: "player-decoder-videotoolbox-plugin",
        release_ffmpeg_profile: None,
        uses_ffmpeg: false,
        link_headerpad: false,
        description: "VideoToolbox decoder plugin",
    },
    IosPluginSpec {
        id: IosPluginId::FrameProcessorDiagnostic,
        crate_name: "player-frame-processor-diagnostic",
        dylib_name: "libvesper_frame_processor_diagnostic.dylib",
        framework_name: "VesperPlayerFrameProcessorDiagnosticPlugin",
        bundle_identifier: "io.github.ikaros.vesper.player.frame-processor-diagnostic-plugin",
        build_directory: "player-frame-processor-diagnostic-plugin",
        release_ffmpeg_profile: None,
        uses_ffmpeg: false,
        link_headerpad: false,
        description: "frame processor plugin",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IosPluginId {
    RemuxFfmpeg,
    SourceNormalizerFfmpeg,
    DecoderVideoToolbox,
    FrameProcessorDiagnostic,
}

impl IosPluginId {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RemuxFfmpeg => "remux-ffmpeg",
            Self::SourceNormalizerFfmpeg => "source-normalizer-ffmpeg",
            Self::DecoderVideoToolbox => "decoder-videotoolbox",
            Self::FrameProcessorDiagnostic => "frame-processor-diagnostic",
        }
    }

    pub(crate) fn spec(self) -> &'static IosPluginSpec {
        IOS_PLUGIN_SPECS
            .iter()
            .find(|spec| spec.id == self)
            .unwrap_or_else(|| unreachable!("every iOS plugin ID has a descriptor"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IosPluginSpec {
    pub(crate) id: IosPluginId,
    pub(crate) crate_name: &'static str,
    pub(crate) dylib_name: &'static str,
    pub(crate) framework_name: &'static str,
    pub(crate) bundle_identifier: &'static str,
    pub(crate) build_directory: &'static str,
    pub(crate) release_ffmpeg_profile: Option<&'static str>,
    pub(crate) uses_ffmpeg: bool,
    pub(crate) link_headerpad: bool,
    pub(crate) description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IosPluginBuildProfile {
    Debug,
    Release,
}

impl IosPluginBuildProfile {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum IosPluginSlice {
    DeviceArm64,
    SimulatorArm64,
}

impl IosPluginSlice {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::DeviceArm64 => "ios-arm64",
            Self::SimulatorArm64 => "ios-simulator-arm64",
        }
    }

    pub(crate) const fn rust_target(self) -> &'static str {
        match self {
            Self::DeviceArm64 => "aarch64-apple-ios",
            Self::SimulatorArm64 => "aarch64-apple-ios-sim",
        }
    }
}

#[derive(Debug)]
pub(crate) struct IosPluginBuildRequest {
    pub(crate) plugin_id: IosPluginId,
    pub(crate) output_directory: PathBuf,
    pub(crate) arguments: Vec<OsString>,
    pub(crate) environment: IosPluginBuildEnvironment,
}

#[derive(Debug, Default)]
pub(crate) struct IosPluginBuildEnvironment {
    pub(crate) ios_deployment_target: Option<String>,
    pub(crate) declared_ffmpeg_profile: Option<String>,
    pub(crate) declared_ffmpeg_platform: Option<String>,
    pub(crate) ffmpeg_output_directory: Option<PathBuf>,
    pub(crate) ffmpeg_input_fingerprints: BTreeMap<IosPluginSlice, String>,
    pub(crate) skip_ffmpeg_prebuilds: Option<bool>,
    pub(crate) ffmpeg_overlays_resolved: bool,
}

pub(crate) fn ensure_supported_host() -> Result<(), IosError> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(IosError::compatibility(
            "building iOS plugin libraries requires macOS",
        ))
    }
}

pub(crate) fn build(
    root: &Path,
    request: IosPluginBuildRequest,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), IosError> {
    ensure_supported_host()?;

    #[cfg(target_os = "macos")]
    {
        implementation::build(root, request, output, diagnostics)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, request, output, diagnostics);
        unreachable!("the host gate rejects non-macOS iOS plugin builds")
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub(crate) struct IosPluginBuildGuard {
    inner: implementation::IosPluginBuildLock,
}

#[cfg(target_os = "macos")]
pub(crate) fn acquire_build_guard(root: &Path) -> Result<IosPluginBuildGuard, IosError> {
    implementation::IosPluginBuildLock::acquire(root).map(|inner| IosPluginBuildGuard { inner })
}

#[cfg(target_os = "macos")]
pub(crate) fn validate_build_guard(
    root: &Path,
    guard: &IosPluginBuildGuard,
) -> Result<(), IosError> {
    guard.inner.validate_root(root)
}

#[cfg(target_os = "macos")]
pub(crate) fn build_in_deferral_holding_lock(
    root: &Path,
    request: IosPluginBuildRequest,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    cancellation: &crate::external_process::InterruptDeferral,
    guard: &IosPluginBuildGuard,
) -> Result<(), IosError> {
    implementation::build_in_deferral_holding_lock(
        root,
        request,
        output,
        diagnostics,
        cancellation,
        &guard.inner,
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn resolve_ffmpeg_output_directory_in_deferral(
    root: &Path,
    slices: &[IosPluginSlice],
    diagnostics: &mut dyn Write,
    cancellation: &crate::external_process::InterruptDeferral,
) -> Result<PathBuf, IosError> {
    implementation::resolve_ffmpeg_output_directory_in_deferral(
        root,
        slices,
        diagnostics,
        cancellation,
    )
}

#[cfg(target_os = "macos")]
mod implementation {
    use std::collections::BTreeSet;
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs::{self, File, OpenOptions, TryLockError};
    use std::io::{self, Read, Write};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Component, Path, PathBuf};
    use std::process::{Command, ExitStatus, Stdio};

    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};

    use crate::external_process::{self, BoundedProcessOutput, ExternalProcessErrorKind};

    use super::*;

    const DEFAULT_SLICES: [IosPluginSlice; 2] =
        [IosPluginSlice::DeviceArm64, IosPluginSlice::SimulatorArm64];
    const MAX_PROCESS_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
    pub(super) const MAX_FFMPEG_PROVENANCE_BYTES: u64 = 1024 * 1024;
    const MAX_DYLIB_BYTES: u64 = 512 * 1024 * 1024;
    const COPY_BUFFER_BYTES: usize = 1024 * 1024;
    const OUTPUT_MARKER_NAME: &str = ".vesper-ios-plugin-output";
    const OUTPUT_MARKER_FORMAT: &str = "vesper-ios-plugin-output";
    const MAX_OUTPUT_MARKER_BYTES: u64 = 16 * 1024;

    #[derive(Debug)]
    struct RequiredTools {
        cargo: PathBuf,
        rustc: PathBuf,
        install_name_tool: PathBuf,
        otool: PathBuf,
        lipo: PathBuf,
        xcrun: PathBuf,
    }

    #[derive(Debug)]
    struct ParsedBuildRequest {
        output_directory: PathBuf,
        profile: IosPluginBuildProfile,
        slices: Vec<IosPluginSlice>,
        deployment_target: String,
        ffmpeg: Option<FfmpegResolution>,
    }

    #[derive(Debug)]
    struct ParsedBuildPreflight {
        output_directory: PathBuf,
        profile: IosPluginBuildProfile,
        slices: Vec<IosPluginSlice>,
        deployment_target: String,
        ffmpeg: Option<crate::ffmpeg::AppleRawProfile>,
        environment: IosPluginBuildEnvironment,
    }

    #[derive(Debug)]
    struct FfmpegResolution {
        profile: String,
        output_directory: PathBuf,
        slices: Vec<FfmpegSliceInput>,
    }

    #[derive(Debug)]
    struct FfmpegSliceInput {
        slice: IosPluginSlice,
        ffmpeg_directory: PathBuf,
        cargo_cache_key: String,
        input_fingerprint: String,
    }

    #[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
    #[serde(deny_unknown_fields)]
    struct OutputOwnershipMarker {
        format: String,
        plugin_id: String,
        cargo_profile: String,
        slices: Vec<String>,
        #[serde(default)]
        ios_deployment_target: Option<String>,
        ffmpeg_profile: Option<String>,
        ffmpeg_inputs: Vec<OutputFfmpegInputMarker>,
    }

    #[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
    #[serde(deny_unknown_fields)]
    struct OutputFfmpegInputMarker {
        slice: String,
        input_fingerprint: String,
    }

    #[derive(Debug)]
    struct BuildOutcome {
        output_directory: PathBuf,
        ffmpeg_profile: Option<String>,
        ffmpeg_output_directory: Option<PathBuf>,
        slices: Vec<IosPluginSlice>,
        warnings: Vec<String>,
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
    pub(super) struct IosPluginBuildLock {
        _file: File,
        root_identity: FileIdentity,
    }

    pub(super) fn build(
        root: &Path,
        request: IosPluginBuildRequest,
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
    ) -> Result<(), IosError> {
        let plugin = request.plugin_id.spec();
        let cancellation = external_process::InterruptDeferral::start("iOS plugin build")
            .map_err(map_external_process_error)?;
        let result = build_transaction(root, plugin, request, diagnostics, &cancellation, None);
        let cancelled = cancellation.finish();
        report_build_result(plugin, result, cancelled, output, diagnostics)
    }

    pub(super) fn build_in_deferral_holding_lock(
        root: &Path,
        request: IosPluginBuildRequest,
        output: &mut dyn Write,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
        guard: &IosPluginBuildLock,
    ) -> Result<(), IosError> {
        let plugin = request.plugin_id.spec();
        let result = build_transaction(
            root,
            plugin,
            request,
            diagnostics,
            cancellation,
            Some(guard),
        );
        report_build_result(plugin, result, false, output, diagnostics)
    }

    pub(super) fn resolve_ffmpeg_output_directory_in_deferral(
        root: &Path,
        slices: &[IosPluginSlice],
        _diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<PathBuf, IosError> {
        check_cancellation(cancellation, "FFmpeg input resolution")?;
        let arguments = slices
            .iter()
            .map(|slice| OsString::from(slice.as_str()))
            .collect::<Vec<_>>();
        crate::ffmpeg::resolve_apple_raw_profile(root, &arguments, None, false)
            .map(|resolution| resolution.output_directory)
            .map_err(map_ffmpeg_error)
    }

    fn build_transaction(
        root: &Path,
        plugin: &IosPluginSpec,
        request: IosPluginBuildRequest,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
        held_lock: Option<&IosPluginBuildLock>,
    ) -> Result<BuildOutcome, IosError> {
        let owned_lock = held_lock
            .is_none()
            .then(|| IosPluginBuildLock::acquire(root))
            .transpose()?;
        let active_lock = held_lock.or(owned_lock.as_ref()).ok_or_else(|| {
            IosError::worker("iOS plugin build transaction is missing its repository lock")
        })?;
        active_lock.validate_root(root)?;
        let target = OutputTarget::preflight(root, plugin, &request.output_directory)?;
        let tools = resolve_required_tools()?;
        let preflight = parse_build_preflight(root, plugin, request)?;
        validate_required_rust_targets(&tools.rustc, &preflight.slices, cancellation)?;
        let parsed =
            hydrate_build_request(root, preflight, diagnostics, cancellation, active_lock)?;

        let staging = tempfile::Builder::new()
            .prefix(".vesper-ios-plugin-stage-")
            .tempdir_in(&target.parent)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS plugin staging directory in '{}': {error}",
                    target.parent.display()
                ))
            })?;
        for (index, slice) in parsed.slices.iter().copied().enumerate() {
            check_cancellation(cancellation, "iOS plugin build")?;
            let ffmpeg_input = parsed
                .ffmpeg
                .as_ref()
                .and_then(|resolution| resolution.slices.get(index));
            build_slice(
                root,
                plugin,
                parsed.profile,
                slice,
                ffmpeg_input,
                &parsed.deployment_target,
                staging.path(),
                &tools,
                diagnostics,
                cancellation,
            )?;
        }
        create_simulator_compatibility_copy(plugin, &parsed.slices, staging.path(), cancellation)?;
        let marker = output_ownership_marker(plugin, &parsed)?;
        write_output_marker(staging.path(), &marker)?;
        validate_staged_layout(plugin, &parsed.slices, &marker, staging.path())?;
        let warnings = promote_staged_output(staging, &target, cancellation)?;
        Ok(BuildOutcome {
            output_directory: parsed.output_directory,
            ffmpeg_profile: parsed.ffmpeg.as_ref().map(|value| value.profile.clone()),
            ffmpeg_output_directory: parsed
                .ffmpeg
                .as_ref()
                .map(|value| value.output_directory.clone()),
            slices: parsed.slices,
            warnings,
        })
    }

    fn parse_build_preflight(
        root: &Path,
        plugin: &IosPluginSpec,
        request: IosPluginBuildRequest,
    ) -> Result<ParsedBuildPreflight, IosError> {
        let IosPluginBuildRequest {
            output_directory,
            mut arguments,
            environment,
            ..
        } = request;
        let profile = match arguments.first().and_then(|value| value.to_str()) {
            Some("debug") => {
                arguments.remove(0);
                IosPluginBuildProfile::Debug
            }
            Some("release") => {
                arguments.remove(0);
                IosPluginBuildProfile::Release
            }
            _ => IosPluginBuildProfile::Debug,
        };
        let (slices, ffmpeg) = if plugin.uses_ffmpeg {
            let resolution = crate::ffmpeg::resolve_apple_raw_profile_with_source(
                root,
                &arguments,
                environment.ffmpeg_output_directory.as_deref(),
                environment.ffmpeg_overlays_resolved,
            )
            .map_err(map_ffmpeg_error)?;
            let slices = resolution
                .slices
                .iter()
                .map(|slice| parse_slice(OsStr::new(slice)))
                .collect::<Result<Vec<_>, _>>()?;
            (slices, Some(resolution))
        } else {
            (parse_non_ffmpeg_slices(&arguments)?, None)
        };
        validate_slice_selection(&slices)?;
        let deployment_target = resolve_ios_deployment_target(&environment)?;
        Ok(ParsedBuildPreflight {
            output_directory,
            profile,
            slices,
            deployment_target,
            ffmpeg,
            environment,
        })
    }

    fn hydrate_build_request(
        root: &Path,
        preflight: ParsedBuildPreflight,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
        guard: &IosPluginBuildLock,
    ) -> Result<ParsedBuildRequest, IosError> {
        let ParsedBuildPreflight {
            output_directory,
            profile,
            slices,
            deployment_target,
            ffmpeg,
            environment,
        } = preflight;
        let ffmpeg = match ffmpeg {
            Some(preflight) => {
                maybe_build_ffmpeg_prebuilts(
                    root,
                    &preflight,
                    &environment,
                    &deployment_target,
                    diagnostics,
                    cancellation,
                    guard,
                )?;
                let mut resolved = hydrate_ffmpeg_inputs(&preflight, cancellation)?;
                apply_ffmpeg_input_fingerprint_overrides(
                    &mut resolved,
                    &environment.ffmpeg_input_fingerprints,
                )?;
                Some(resolved)
            }
            None => None,
        };
        Ok(ParsedBuildRequest {
            output_directory,
            profile,
            slices,
            deployment_target,
            ffmpeg,
        })
    }

    fn hydrate_ffmpeg_inputs(
        preflight: &crate::ffmpeg::AppleRawProfile,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<FfmpegResolution, IosError> {
        let mut slices = Vec::with_capacity(preflight.slices.len());
        for value in &preflight.slices {
            check_cancellation(cancellation, "FFmpeg input provenance")?;
            let slice = parse_slice(OsStr::new(value))?;
            let ffmpeg_directory = preflight.output_directory.join(match slice {
                IosPluginSlice::DeviceArm64 => "ios",
                IosPluginSlice::SimulatorArm64 => "ios-simulator",
            });
            validate_ffmpeg_profile_hash(&ffmpeg_directory, &preflight.profile_hash)?;
            let cargo_cache_key = ffmpeg_directory_cache_key(&ffmpeg_directory)?;
            let input_fingerprint = ffmpeg_build_input_fingerprint(&ffmpeg_directory)?;
            slices.push(FfmpegSliceInput {
                slice,
                ffmpeg_directory,
                cargo_cache_key,
                input_fingerprint,
            });
        }
        Ok(FfmpegResolution {
            profile: preflight.profile.clone(),
            output_directory: preflight.output_directory.clone(),
            slices,
        })
    }

    fn validate_ffmpeg_profile_hash(
        directory: &Path,
        expected_profile_hash: &str,
    ) -> Result<(), IosError> {
        let path = directory.join("vesper-ffmpeg-build-metadata.txt");
        let metadata = read_ffmpeg_provenance_file(&path, "FFmpeg build metadata")?;
        let text = std::str::from_utf8(&metadata).map_err(|error| {
            IosError::conformance(format!(
                "FFmpeg build metadata '{}' is not UTF-8: {error}",
                path.display()
            ))
        })?;
        let mut actual_profile_hash = None;
        for line in text.lines() {
            if let Some(value) = line.strip_prefix("profile_hash=") {
                if actual_profile_hash.replace(value).is_some() {
                    return Err(IosError::conformance(format!(
                        "FFmpeg build metadata '{}' contains duplicate profile_hash records",
                        path.display()
                    )));
                }
            }
        }
        let Some(actual_profile_hash) = actual_profile_hash else {
            return Err(IosError::conformance(format!(
                "FFmpeg build metadata '{}' omits profile_hash",
                path.display()
            )));
        };
        if actual_profile_hash != expected_profile_hash {
            return Err(IosError::conformance(format!(
                "FFmpeg build metadata '{}' has profile_hash '{actual_profile_hash}', expected '{expected_profile_hash}'",
                path.display()
            )));
        }
        Ok(())
    }

    pub(super) fn ffmpeg_directory_cache_key(path: &Path) -> Result<String, IosError> {
        let bytes = path.as_os_str().as_bytes();
        let length = u64::try_from(bytes.len())
            .map_err(|_| IosError::conformance("FFmpeg input path is too long to fingerprint"))?;
        let mut digest = Sha256::new();
        digest.update(b"vesper-apple-ffmpeg-directory-v1\0");
        digest.update(length.to_le_bytes());
        digest.update(bytes);
        Ok(format!("path-{:x}", digest.finalize()))
    }

    pub(super) fn ffmpeg_build_input_fingerprint(directory: &Path) -> Result<String, IosError> {
        let metadata = read_ffmpeg_provenance_file(
            &directory.join("vesper-ffmpeg-build-metadata.txt"),
            "FFmpeg build metadata",
        )?;
        let library_checksums = read_ffmpeg_provenance_file(
            &directory.join("vesper-ffmpeg-library-sha256.txt"),
            "FFmpeg library checksum record",
        )?;
        Ok(format!(
            "{:x}-{:x}",
            Sha256::digest(&metadata),
            Sha256::digest(&library_checksums)
        ))
    }

    fn read_ffmpeg_provenance_file(path: &Path, label: &str) -> Result<Vec<u8>, IosError> {
        use std::os::unix::fs::OpenOptionsExt;

        validate_regular_file(path, MAX_FFMPEG_PROVENANCE_BYTES, label)?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW)
            .open(path)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to open {label} '{}': {error}",
                    path.display()
                ))
            })?;
        let metadata = file.metadata().map_err(|error| {
            IosError::storage(format!(
                "failed to inspect open {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_FFMPEG_PROVENANCE_BYTES
        {
            return Err(IosError::conformance(format!(
                "{label} '{}' must remain a non-empty regular file within its {MAX_FFMPEG_PROVENANCE_BYTES}-byte limit",
                path.display()
            )));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FFMPEG_PROVENANCE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to read {label} '{}': {error}",
                    path.display()
                ))
            })?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_FFMPEG_PROVENANCE_BYTES {
            return Err(IosError::conformance(format!(
                "{label} '{}' changed outside its non-empty {MAX_FFMPEG_PROVENANCE_BYTES}-byte limit while reading",
                path.display()
            )));
        }
        Ok(bytes)
    }

    fn apply_ffmpeg_input_fingerprint_overrides(
        resolution: &mut FfmpegResolution,
        overrides: &BTreeMap<IosPluginSlice, String>,
    ) -> Result<(), IosError> {
        if overrides.is_empty() {
            return Ok(());
        }
        if overrides.len() != resolution.slices.len() {
            return Err(IosError::conformance(
                "FFmpeg input fingerprint overrides do not match the resolved slices",
            ));
        }
        for input in &mut resolution.slices {
            let fingerprint = overrides.get(&input.slice).ok_or_else(|| {
                IosError::conformance(format!(
                    "FFmpeg input fingerprint override is missing for {}",
                    input.slice.as_str()
                ))
            })?;
            validate_cache_component(fingerprint, "FFmpeg input fingerprint")?;
            input.input_fingerprint = fingerprint.clone();
        }
        Ok(())
    }

    fn parse_non_ffmpeg_slices(arguments: &[OsString]) -> Result<Vec<IosPluginSlice>, IosError> {
        if arguments.is_empty() {
            return Ok(DEFAULT_SLICES.to_vec());
        }
        arguments
            .iter()
            .map(|argument| parse_slice(argument.as_os_str()))
            .collect()
    }

    fn parse_slice(value: &OsStr) -> Result<IosPluginSlice, IosError> {
        match value.to_str() {
            Some("ios-arm64") => Ok(IosPluginSlice::DeviceArm64),
            Some("ios-simulator-arm64") => Ok(IosPluginSlice::SimulatorArm64),
            Some(value) => Err(IosError::compatibility(format!(
                "unsupported Apple slice: {value}; expected ios-arm64 or ios-simulator-arm64"
            ))),
            None => Err(IosError::compatibility(
                "Apple slice names must be valid UTF-8",
            )),
        }
    }

    fn validate_slice_selection(slices: &[IosPluginSlice]) -> Result<(), IosError> {
        if slices.is_empty() {
            return Err(IosError::compatibility("no Apple slices were selected"));
        }
        if slices.iter().copied().collect::<BTreeSet<_>>().len() != slices.len() {
            return Err(IosError::conformance(
                "Apple slice selection contains a duplicate slice",
            ));
        }
        Ok(())
    }

    fn maybe_build_ffmpeg_prebuilts(
        root: &Path,
        preflight: &crate::ffmpeg::AppleRawProfile,
        environment: &IosPluginBuildEnvironment,
        deployment_target: &str,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
        guard: &IosPluginBuildLock,
    ) -> Result<(), IosError> {
        let skip = environment.skip_ffmpeg_prebuilds.unwrap_or_else(|| {
            env::var_os("VESPER_SKIP_APPLE_FFMPEG_PREBUILDS").as_deref() == Some(OsStr::new("1"))
        });
        if skip {
            return Ok(());
        }
        guard.validate_root(root)?;
        let mut profile = preflight.native_profile.clone();
        if let Some(declared_profile) = &environment.declared_ffmpeg_profile {
            profile.declared_profile = declared_profile.clone();
        }
        if let Some(declared_platform) = &environment.declared_ffmpeg_platform
            && declared_platform != "ios"
        {
            return Err(IosError::conformance(format!(
                "resolved Apple FFmpeg declared platform must be ios, got {declared_platform}"
            )));
        }
        let source = preflight.source.clone().ok_or_else(|| {
            IosError::conformance("resolved Apple FFmpeg profile did not carry source provenance")
        })?;
        crate::ffmpeg_apple::run_holding_repository_lock(
            root,
            &preflight.output_directory,
            &preflight.slices,
            deployment_target,
            &profile,
            &source,
            diagnostics,
            cancellation,
        )
        .map_err(map_ffmpeg_error)
    }

    fn resolve_ios_deployment_target(
        environment: &IosPluginBuildEnvironment,
    ) -> Result<String, IosError> {
        let inherited = match env::var_os("VESPER_APPLE_IOS_DEPLOYMENT_TARGET") {
            Some(value) => Some(value.into_string().map_err(|_| {
                IosError::compatibility("VESPER_APPLE_IOS_DEPLOYMENT_TARGET must be valid UTF-8")
            })?),
            None => None,
        };
        let value = environment
            .ios_deployment_target
            .clone()
            .or(inherited)
            .unwrap_or_else(|| "17.0".to_owned());
        validate_ios_deployment_target(&value)?;
        Ok(value)
    }

    pub(super) fn validate_ios_deployment_target(value: &str) -> Result<(), IosError> {
        let version =
            crate::ios::normalize_apple_version(value, "iOS deployment target").map_err(|_| {
                IosError::compatibility(format!("iOS plugin deployment target is invalid: {value}"))
            })?;
        if version < (17, 0, 0) {
            return Err(IosError::compatibility(format!(
                "iOS plugin deployment target {value} is below the supported iOS 17.0 floor"
            )));
        }
        Ok(())
    }

    fn resolve_required_tools() -> Result<RequiredTools, IosError> {
        Ok(RequiredTools {
            cargo: require_path_command("cargo")?,
            rustc: require_path_command("rustc")?,
            install_name_tool: require_path_command("install_name_tool")?,
            otool: require_path_command("otool")?,
            lipo: require_path_command("lipo")?,
            xcrun: require_path_command("xcrun")?,
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
        slices: &[IosPluginSlice],
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let mut missing = Vec::new();
        for slice in slices {
            check_cancellation(cancellation, "Rust target validation")?;
            if !rust_target_is_installed(rustc, slice.rust_target(), cancellation)? {
                missing.push(slice.rust_target());
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
        let target_libdir = PathBuf::from(OsString::from_vec(trim_ascii(&result.stdout).to_vec()));
        Ok(target_libdir.is_dir())
    }

    fn build_slice(
        root: &Path,
        plugin: &IosPluginSpec,
        profile: IosPluginBuildProfile,
        slice: IosPluginSlice,
        ffmpeg_input: Option<&FfmpegSliceInput>,
        deployment_target: &str,
        staging: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let cargo_target_directory =
            cargo_target_directory(root, plugin, ffmpeg_input, deployment_target)?;
        let mut command = Command::new(&tools.cargo);
        command
            .current_dir(root)
            .args(["build", "--locked", "--manifest-path"])
            .arg(root.join("Cargo.toml"))
            .args(["--target", slice.rust_target(), "-p", plugin.crate_name])
            .env("CARGO_TARGET_DIR", &cargo_target_directory)
            .env("IPHONEOS_DEPLOYMENT_TARGET", deployment_target)
            .stdin(Stdio::null());
        if profile == IosPluginBuildProfile::Release {
            command.arg("--release");
        }
        if let Some(input) = ffmpeg_input {
            command.env("FFMPEG_DIR", &input.ffmpeg_directory);
        }
        if plugin.link_headerpad {
            let mut rustflags = env::var_os("RUSTFLAGS").unwrap_or_default();
            if !rustflags.is_empty() {
                rustflags.push(" ");
            }
            rustflags.push("-C link-arg=-Wl,-headerpad_max_install_names");
            command.env("RUSTFLAGS", rustflags);
        }
        run_required_command(
            command,
            &format!("Cargo build for {}/{}", plugin.crate_name, slice.as_str()),
            diagnostics,
            cancellation,
        )?;
        let artifact = cargo_target_directory
            .join(slice.rust_target())
            .join(profile.as_str())
            .join(plugin.dylib_name);
        validate_regular_file(&artifact, MAX_DYLIB_BYTES, "built iOS plugin library")?;
        let destination = staged_slice_path(staging, plugin, slice);
        let parent = destination.parent().ok_or_else(|| {
            IosError::storage("staged iOS plugin library has no parent directory")
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            IosError::storage(format!(
                "failed to create staged iOS plugin directory '{}': {error}",
                parent.display()
            ))
        })?;
        copy_regular_file(&artifact, &destination, cancellation)?;
        prepare_plugin_binary(plugin, &destination, tools, diagnostics, cancellation)?;
        verify_arm64_binary(&destination, tools, diagnostics, cancellation)?;
        verify_macho_build_metadata(
            &destination,
            slice,
            deployment_target,
            tools,
            diagnostics,
            cancellation,
        )
    }

    fn cargo_target_directory(
        root: &Path,
        plugin: &IosPluginSpec,
        ffmpeg_input: Option<&FfmpegSliceInput>,
        deployment_target: &str,
    ) -> Result<PathBuf, IosError> {
        let base = root
            .join("target")
            .join(format!("{}-ios", plugin.crate_name))
            .join(format!(
                "minimum-ios-{}",
                deployment_target.replace('.', "_")
            ));
        match ffmpeg_input {
            Some(input) => {
                let cache_key = &input.cargo_cache_key;
                let fingerprint = &input.input_fingerprint;
                validate_cache_component(cache_key, "Cargo cache key")?;
                validate_cache_component(fingerprint, "FFmpeg input fingerprint")?;
                Ok(base.join(cache_key).join(fingerprint))
            }
            None => Ok(base),
        }
    }

    fn validate_cache_component(value: &str, label: &str) -> Result<(), IosError> {
        let path = Path::new(value);
        let mut components = path.components();
        if value.is_empty()
            || !matches!(components.next(), Some(Component::Normal(_)))
            || components.next().is_some()
        {
            return Err(IosError::conformance(format!(
                "{label} must be one normalized path component"
            )));
        }
        Ok(())
    }

    fn staged_slice_path(staging: &Path, plugin: &IosPluginSpec, slice: IosPluginSlice) -> PathBuf {
        match slice {
            IosPluginSlice::DeviceArm64 => staging.join("iphoneos").join(plugin.dylib_name),
            IosPluginSlice::SimulatorArm64 => staging
                .join("iphonesimulator")
                .join(slice.rust_target())
                .join(plugin.dylib_name),
        }
    }

    fn create_simulator_compatibility_copy(
        plugin: &IosPluginSpec,
        slices: &[IosPluginSlice],
        staging: &Path,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        if !slices.contains(&IosPluginSlice::SimulatorArm64) {
            return Ok(());
        }
        let source = staged_slice_path(staging, plugin, IosPluginSlice::SimulatorArm64);
        let destination = staging.join("iphonesimulator").join(plugin.dylib_name);
        copy_regular_file(&source, &destination, cancellation)
    }

    fn prepare_plugin_binary(
        plugin: &IosPluginSpec,
        binary: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let mut install_id = Command::new(&tools.install_name_tool);
        install_id
            .args(["-id", &format!("@rpath/{}", plugin.dylib_name)])
            .arg(binary);
        run_required_command(
            install_id,
            "iOS plugin install-name update",
            diagnostics,
            cancellation,
        )?;
        if !plugin.uses_ffmpeg {
            return Ok(());
        }
        let rpaths = read_rpaths(binary, tools, diagnostics, cancellation)?;
        for stale in [
            "@loader_path/VesperPlayerFfmpegRuntime.framework/Frameworks",
            "@loader_path/../VesperPlayerFfmpegRuntime.framework/Frameworks",
        ] {
            if rpaths.contains(stale) {
                let mut remove = Command::new(&tools.install_name_tool);
                remove.args(["-delete_rpath", stale]).arg(binary);
                run_required_command(
                    remove,
                    "iOS plugin stale rpath removal",
                    diagnostics,
                    cancellation,
                )?;
            }
        }
        for required in ["@loader_path", "@loader_path/.."] {
            if !rpaths.contains(required) {
                let mut add = Command::new(&tools.install_name_tool);
                add.args(["-add_rpath", required]).arg(binary);
                run_required_command(
                    add,
                    "iOS plugin loader rpath update",
                    diagnostics,
                    cancellation,
                )?;
            }
        }
        Ok(())
    }

    fn read_rpaths(
        binary: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<BTreeSet<String>, IosError> {
        let mut command = Command::new(&tools.otool);
        command.arg("-l").arg(binary);
        let result =
            run_process_capture(&mut command, "iOS plugin rpath inspection", cancellation)?;
        diagnostics
            .write_all(&result.stderr)
            .map_err(diagnostics_error)?;
        diagnostics.flush().map_err(diagnostics_error)?;
        classify_process_status(result.status, "iOS plugin rpath inspection")?;
        let output = std::str::from_utf8(&result.stdout).map_err(|error| {
            IosError::conformance(format!(
                "otool returned non-UTF-8 iOS plugin metadata: {error}"
            ))
        })?;
        let mut rpaths = BTreeSet::new();
        for line in output.lines() {
            let mut fields = line.split_ascii_whitespace();
            if fields.next() == Some("path")
                && let Some(path) = fields.next()
            {
                rpaths.insert(path.to_owned());
            }
        }
        Ok(rpaths)
    }

    fn verify_arm64_binary(
        binary: &Path,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let command = lipo_verify_arm64_command(&tools.lipo, binary);
        run_required_command(
            command,
            "iOS plugin arm64 architecture verification",
            diagnostics,
            cancellation,
        )
    }

    pub(super) fn lipo_verify_arm64_command(lipo: &Path, binary: &Path) -> Command {
        let mut command = Command::new(lipo);
        command.arg(binary).args(["-verify_arch", "arm64"]);
        command
    }

    fn verify_macho_build_metadata(
        binary: &Path,
        slice: IosPluginSlice,
        deployment_target: &str,
        tools: &RequiredTools,
        diagnostics: &mut dyn Write,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        let mut command = Command::new(&tools.xcrun);
        command
            .args(["vtool", "-show-build"])
            .arg(binary)
            .stdin(Stdio::null());
        let result = run_process_capture(
            &mut command,
            "iOS plugin Mach-O build metadata inspection",
            cancellation,
        )?;
        diagnostics
            .write_all(&result.stderr)
            .map_err(diagnostics_error)?;
        classify_process_status(result.status, "iOS plugin Mach-O build metadata inspection")?;
        let output = std::str::from_utf8(&result.stdout).map_err(|error| {
            IosError::conformance(format!(
                "xcrun vtool output is not UTF-8 for '{}': {error}",
                binary.display()
            ))
        })?;
        let metadata =
            crate::ios::parse_vtool_build_metadata(output, &binary.display().to_string())?;
        let expected_platform = match slice {
            IosPluginSlice::DeviceArm64 => "IOS",
            IosPluginSlice::SimulatorArm64 => "IOSSIMULATOR",
        };
        if metadata.platform != expected_platform {
            return Err(IosError::conformance(format!(
                "iOS plugin Mach-O platform mismatch for '{}': actual {}, expected {expected_platform}",
                binary.display(),
                metadata.platform
            )));
        }
        if crate::ios::normalize_apple_version(&metadata.minimum_os, "plugin Mach-O minimum OS")?
            != crate::ios::normalize_apple_version(deployment_target, "iOS deployment target")?
        {
            return Err(IosError::conformance(format!(
                "iOS plugin Mach-O minimum OS mismatch for '{}': actual {}, expected {}",
                binary.display(),
                metadata.minimum_os,
                deployment_target
            )));
        }
        Ok(())
    }

    fn validate_staged_layout(
        plugin: &IosPluginSpec,
        slices: &[IosPluginSlice],
        expected_marker: &OutputOwnershipMarker,
        staging: &Path,
    ) -> Result<(), IosError> {
        let mut expected_root = slices
            .iter()
            .map(|slice| match slice {
                IosPluginSlice::DeviceArm64 => OsString::from("iphoneos"),
                IosPluginSlice::SimulatorArm64 => OsString::from("iphonesimulator"),
            })
            .collect::<BTreeSet<_>>();
        expected_root.insert(OsString::from(OUTPUT_MARKER_NAME));
        if read_directory_names(staging, "iOS plugin staging directory", 3)? != expected_root {
            return Err(IosError::conformance(
                "iOS plugin staging directory has an unexpected artifact set",
            ));
        }
        let marker = read_output_marker(staging, plugin)?;
        if marker != *expected_marker {
            return Err(IosError::conformance(
                "staged iOS plugin output marker does not match its build inputs",
            ));
        }
        validate_plugin_slice_layout(plugin, slices, staging)
    }

    fn validate_plugin_slice_layout(
        plugin: &IosPluginSpec,
        slices: &[IosPluginSlice],
        directory_root: &Path,
    ) -> Result<(), IosError> {
        if slices.contains(&IosPluginSlice::DeviceArm64) {
            let directory = directory_root.join("iphoneos");
            let expected = BTreeSet::from([OsString::from(plugin.dylib_name)]);
            if read_directory_names(&directory, "iOS device plugin directory", 1)? != expected {
                return Err(IosError::conformance(
                    "iOS device plugin directory has an unexpected artifact set",
                ));
            }
            validate_regular_file(
                &directory.join(plugin.dylib_name),
                MAX_DYLIB_BYTES,
                "staged iOS device plugin library",
            )?;
        }
        if slices.contains(&IosPluginSlice::SimulatorArm64) {
            let directory = directory_root.join("iphonesimulator");
            let expected = BTreeSet::from([
                OsString::from(plugin.dylib_name),
                OsString::from(IosPluginSlice::SimulatorArm64.rust_target()),
            ]);
            if read_directory_names(&directory, "iOS Simulator plugin directory", 2)? != expected {
                return Err(IosError::conformance(
                    "iOS Simulator plugin directory has an unexpected artifact set",
                ));
            }
            let target_directory = directory.join(IosPluginSlice::SimulatorArm64.rust_target());
            let expected = BTreeSet::from([OsString::from(plugin.dylib_name)]);
            if read_directory_names(
                &target_directory,
                "iOS Simulator target plugin directory",
                1,
            )? != expected
            {
                return Err(IosError::conformance(
                    "iOS Simulator target plugin directory has an unexpected artifact set",
                ));
            }
            for binary in [
                directory.join(plugin.dylib_name),
                target_directory.join(plugin.dylib_name),
            ] {
                validate_regular_file(
                    &binary,
                    MAX_DYLIB_BYTES,
                    "staged iOS Simulator plugin library",
                )?;
            }
        }
        Ok(())
    }

    fn output_ownership_marker(
        plugin: &IosPluginSpec,
        request: &ParsedBuildRequest,
    ) -> Result<OutputOwnershipMarker, IosError> {
        let ffmpeg_inputs = match request.ffmpeg.as_ref() {
            Some(ffmpeg) => ffmpeg
                .slices
                .iter()
                .map(|input| {
                    Ok(OutputFfmpegInputMarker {
                        slice: input.slice.as_str().to_owned(),
                        input_fingerprint: input.input_fingerprint.clone(),
                    })
                })
                .collect::<Result<Vec<_>, IosError>>()?,
            None => Vec::new(),
        };
        Ok(OutputOwnershipMarker {
            format: OUTPUT_MARKER_FORMAT.to_owned(),
            plugin_id: plugin.id.as_str().to_owned(),
            cargo_profile: request.profile.as_str().to_owned(),
            slices: request
                .slices
                .iter()
                .map(|slice| slice.as_str().to_owned())
                .collect(),
            ios_deployment_target: Some(request.deployment_target.clone()),
            ffmpeg_profile: request.ffmpeg.as_ref().map(|ffmpeg| ffmpeg.profile.clone()),
            ffmpeg_inputs,
        })
    }

    fn write_output_marker(
        staging: &Path,
        marker_value: &OutputOwnershipMarker,
    ) -> Result<(), IosError> {
        let marker = staging.join(OUTPUT_MARKER_NAME);
        let mut bytes = serde_json::to_vec(marker_value).map_err(|error| {
            IosError::conformance(format!(
                "failed to serialize iOS plugin output marker: {error}"
            ))
        })?;
        bytes.push(b'\n');
        if u64::try_from(bytes.len()).map_or(true, |length| length > MAX_OUTPUT_MARKER_BYTES) {
            return Err(IosError::conformance(format!(
                "iOS plugin output marker exceeds its {MAX_OUTPUT_MARKER_BYTES}-byte schema limit"
            )));
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&marker)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS plugin output marker '{}': {error}",
                    marker.display()
                ))
            })?;
        file.write_all(&bytes).map_err(|error| {
            IosError::storage(format!(
                "failed to write iOS plugin output marker '{}': {error}",
                marker.display()
            ))
        })?;
        file.sync_all().map_err(|error| {
            IosError::storage(format!(
                "failed to sync iOS plugin output marker '{}': {error}",
                marker.display()
            ))
        })
    }

    fn read_output_marker(
        output: &Path,
        plugin: &IosPluginSpec,
    ) -> Result<OutputOwnershipMarker, IosError> {
        let marker = output.join(OUTPUT_MARKER_NAME);
        validate_regular_file(&marker, MAX_OUTPUT_MARKER_BYTES, "iOS plugin output marker")?;
        let file = OpenOptions::new()
            .read(true)
            .open(&marker)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to open iOS plugin output marker '{}': {error}",
                    marker.display()
                ))
            })?;
        let mut content = Vec::new();
        file.take(MAX_OUTPUT_MARKER_BYTES + 1)
            .read_to_end(&mut content)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to read iOS plugin output marker '{}': {error}",
                    marker.display()
                ))
            })?;
        if u64::try_from(content.len()).map_or(true, |length| length > MAX_OUTPUT_MARKER_BYTES) {
            return Err(IosError::compatibility(format!(
                "iOS plugin output marker '{}' exceeds its {MAX_OUTPUT_MARKER_BYTES}-byte schema limit",
                marker.display()
            )));
        }
        let value: OutputOwnershipMarker = serde_json::from_slice(&content).map_err(|error| {
            IosError::compatibility(format!(
                "iOS plugin output marker '{}' is invalid: {error}",
                marker.display()
            ))
        })?;
        validate_output_marker_fields(&value, plugin, &marker)?;
        Ok(value)
    }

    fn validate_output_marker_fields(
        marker: &OutputOwnershipMarker,
        plugin: &IosPluginSpec,
        marker_path: &Path,
    ) -> Result<(), IosError> {
        if marker.format != OUTPUT_MARKER_FORMAT || marker.plugin_id != plugin.id.as_str() {
            return Err(IosError::compatibility(format!(
                "iOS plugin output marker '{}' does not identify a compatible {} output",
                marker_path.display(),
                plugin.id.as_str()
            )));
        }
        if !matches!(marker.cargo_profile.as_str(), "debug" | "release") {
            return Err(IosError::compatibility(format!(
                "iOS plugin output marker '{}' has an unsupported Cargo profile",
                marker_path.display()
            )));
        }
        if let Some(deployment_target) = &marker.ios_deployment_target {
            validate_ios_deployment_target(deployment_target)?;
        }
        let slices = parse_marker_slices(&marker.slices, marker_path)?;
        if plugin.uses_ffmpeg {
            if marker.ffmpeg_profile.as_deref().is_none_or(str::is_empty)
                || marker.ffmpeg_inputs.len() != slices.len()
            {
                return Err(IosError::compatibility(format!(
                    "iOS plugin output marker '{}' has incomplete FFmpeg provenance",
                    marker_path.display()
                )));
            }
            let mut input_slices = BTreeSet::new();
            for input in &marker.ffmpeg_inputs {
                let slice = parse_slice(OsStr::new(&input.slice))?;
                if input.input_fingerprint.is_empty() || !input_slices.insert(slice) {
                    return Err(IosError::compatibility(format!(
                        "iOS plugin output marker '{}' has invalid FFmpeg input provenance",
                        marker_path.display()
                    )));
                }
            }
            if input_slices != slices.iter().copied().collect() {
                return Err(IosError::compatibility(format!(
                    "iOS plugin output marker '{}' has mismatched FFmpeg slices",
                    marker_path.display()
                )));
            }
        } else if marker.ffmpeg_profile.is_some() || !marker.ffmpeg_inputs.is_empty() {
            return Err(IosError::compatibility(format!(
                "iOS plugin output marker '{}' unexpectedly declares FFmpeg provenance",
                marker_path.display()
            )));
        }
        Ok(())
    }

    fn parse_marker_slices(
        values: &[String],
        marker_path: &Path,
    ) -> Result<Vec<IosPluginSlice>, IosError> {
        let slices = values
            .iter()
            .map(|value| parse_slice(OsStr::new(value)))
            .collect::<Result<Vec<_>, _>>()?;
        let unique = slices.iter().copied().collect::<BTreeSet<_>>();
        if slices.is_empty() || unique.len() != slices.len() {
            return Err(IosError::compatibility(format!(
                "iOS plugin output marker '{}' has an empty or repeated slice set",
                marker_path.display()
            )));
        }
        Ok(slices)
    }

    fn validate_existing_output_ownership(
        output: &Path,
        plugin: &IosPluginSpec,
    ) -> Result<(), IosError> {
        let marker = output.join(OUTPUT_MARKER_NAME);
        match fs::symlink_metadata(&marker) {
            Ok(_) => {
                let marker = read_output_marker(output, plugin)?;
                let slices = parse_marker_slices(&marker.slices, &output.join(OUTPUT_MARKER_NAME))?;
                let mut expected_root = slices
                    .iter()
                    .map(|slice| match slice {
                        IosPluginSlice::DeviceArm64 => OsString::from("iphoneos"),
                        IosPluginSlice::SimulatorArm64 => OsString::from("iphonesimulator"),
                    })
                    .collect::<BTreeSet<_>>();
                expected_root.insert(OsString::from(OUTPUT_MARKER_NAME));
                if read_directory_names(output, "existing iOS plugin output", 3)? != expected_root {
                    return Err(IosError::compatibility(format!(
                        "refusing to replace malformed builder-owned iOS plugin output directory '{}'",
                        output.display()
                    )));
                }
                validate_plugin_slice_layout(plugin, &slices, output).map_err(|error| {
                    IosError::compatibility(format!(
                        "refusing to replace malformed builder-owned iOS plugin output directory '{}': {error}",
                        output.display()
                    ))
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                validate_legacy_output_layout(output, plugin)
            }
            Err(error) => Err(IosError::storage(format!(
                "failed to inspect iOS plugin output marker '{}': {error}",
                marker.display()
            ))),
        }
    }

    fn validate_legacy_output_layout(
        output: &Path,
        plugin: &IosPluginSpec,
    ) -> Result<(), IosError> {
        let names = read_directory_names(output, "existing iOS plugin output", 2)?;
        let mut slices = Vec::new();
        if names.contains(OsStr::new("iphoneos")) {
            slices.push(IosPluginSlice::DeviceArm64);
        }
        if names.contains(OsStr::new("iphonesimulator")) {
            slices.push(IosPluginSlice::SimulatorArm64);
        }
        let expected = slices
            .iter()
            .map(|slice| match slice {
                IosPluginSlice::DeviceArm64 => OsString::from("iphoneos"),
                IosPluginSlice::SimulatorArm64 => OsString::from("iphonesimulator"),
            })
            .collect::<BTreeSet<_>>();
        if slices.is_empty() || names != expected {
            return Err(IosError::compatibility(format!(
                "refusing to replace unowned iOS plugin output directory '{}'; remove it explicitly or use a builder-owned output",
                output.display()
            )));
        }
        validate_plugin_slice_layout(plugin, &slices, output).map_err(|error| {
            IosError::compatibility(format!(
                "refusing to replace unowned iOS plugin output directory '{}': {error}",
                output.display()
            ))
        })
    }

    fn read_directory_names(
        path: &Path,
        label: &str,
        maximum_entries: usize,
    ) -> Result<BTreeSet<OsString>, IosError> {
        let mut names = BTreeSet::new();
        let entries = fs::read_dir(path).map_err(|error| {
            IosError::storage(format!(
                "failed to read {label} '{}': {error}",
                path.display()
            ))
        })?;
        for entry in entries {
            if names.len() >= maximum_entries {
                return Err(IosError::conformance(format!(
                    "{label} '{}' exceeds its {maximum_entries}-entry limit",
                    path.display()
                )));
            }
            let entry = entry.map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect {label} '{}': {error}",
                    path.display()
                ))
            })?;
            names.insert(entry.file_name());
        }
        Ok(names)
    }

    fn validate_regular_file(path: &Path, maximum_bytes: u64, label: &str) -> Result<(), IosError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            IosError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() || metadata.len() == 0 {
            return Err(IosError::conformance(format!(
                "{label} '{}' must be a non-empty regular non-symlink file",
                path.display()
            )));
        }
        if metadata.len() > maximum_bytes {
            return Err(IosError::conformance(format!(
                "{label} '{}' exceeds its {maximum_bytes}-byte limit",
                path.display()
            )));
        }
        Ok(())
    }

    fn copy_regular_file(
        source: &Path,
        destination: &Path,
        cancellation: &external_process::InterruptDeferral,
    ) -> Result<(), IosError> {
        validate_regular_file(source, MAX_DYLIB_BYTES, "iOS plugin copy source")?;
        let mut input = File::open(source).map_err(|error| {
            IosError::storage(format!(
                "failed to open iOS plugin copy source '{}': {error}",
                source.display()
            ))
        })?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create staged iOS plugin library '{}': {error}",
                    destination.display()
                ))
            })?;
        let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
        loop {
            check_cancellation(cancellation, "iOS plugin library copy")?;
            let read = input.read(&mut buffer).map_err(|error| {
                IosError::storage(format!(
                    "failed to read iOS plugin library '{}': {error}",
                    source.display()
                ))
            })?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read]).map_err(|error| {
                IosError::storage(format!(
                    "failed to write staged iOS plugin library '{}': {error}",
                    destination.display()
                ))
            })?;
        }
        output.sync_all().map_err(|error| {
            IosError::storage(format!(
                "failed to sync staged iOS plugin library '{}': {error}",
                destination.display()
            ))
        })?;
        let permissions = fs::metadata(source)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect iOS plugin library permissions '{}': {error}",
                    source.display()
                ))
            })?
            .permissions();
        fs::set_permissions(destination, permissions).map_err(|error| {
            IosError::storage(format!(
                "failed to preserve iOS plugin library permissions '{}': {error}",
                destination.display()
            ))
        })
    }

    impl OutputTarget {
        fn preflight(
            root: &Path,
            plugin: &IosPluginSpec,
            requested: &Path,
        ) -> Result<Self, IosError> {
            let file_name = requested
                .components()
                .next_back()
                .and_then(|component| match component {
                    Component::Normal(value) => Some(value.to_os_string()),
                    _ => None,
                })
                .ok_or_else(|| {
                    IosError::compatibility("the iOS plugin output must name a child directory")
                })?;
            let absolute = if requested.is_absolute() {
                requested.to_path_buf()
            } else {
                env::current_dir()
                    .map_err(|error| {
                        IosError::storage(format!(
                            "failed to resolve the iOS plugin output directory: {error}"
                        ))
                    })?
                    .join(requested)
            };
            let parent = absolute.parent().ok_or_else(|| {
                IosError::compatibility("the iOS plugin output must have a parent directory")
            })?;
            fs::create_dir_all(parent).map_err(|error| {
                IosError::storage(format!(
                    "failed to create iOS plugin output parent '{}': {error}",
                    parent.display()
                ))
            })?;
            let parent = fs::canonicalize(parent).map_err(|error| {
                IosError::storage(format!(
                    "failed to resolve iOS plugin output parent '{}': {error}",
                    parent.display()
                ))
            })?;
            let parent_metadata = fs::symlink_metadata(&parent).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect iOS plugin output parent '{}': {error}",
                    parent.display()
                ))
            })?;
            if !parent_metadata.file_type().is_dir() {
                return Err(IosError::compatibility(format!(
                    "iOS plugin output parent '{}' must be a regular non-symlink directory",
                    parent.display()
                )));
            }
            let path = parent.join(file_name);
            let canonical_root = fs::canonicalize(root).map_err(|error| {
                IosError::storage(format!(
                    "failed to resolve iOS plugin repository root '{}': {error}",
                    root.display()
                ))
            })?;
            if canonical_root == path || canonical_root.starts_with(&path) {
                return Err(IosError::compatibility(format!(
                    "refusing to replace repository root or ancestor as iOS plugin output: {}",
                    path.display()
                )));
            }
            let initial_identity = optional_directory_identity(&path, "iOS plugin output")?;
            if initial_identity.is_some() {
                validate_existing_output_ownership(&path, plugin)?;
            }
            Ok(Self {
                initial_identity,
                path,
                parent,
                parent_identity: file_identity(&parent_metadata),
            })
        }

        fn revalidate(&self) -> Result<bool, IosError> {
            let parent_metadata = fs::symlink_metadata(&self.parent).map_err(|error| {
                IosError::storage(format!(
                    "failed to recheck iOS plugin output parent '{}': {error}",
                    self.parent.display()
                ))
            })?;
            if !parent_metadata.file_type().is_dir()
                || file_identity(&parent_metadata) != self.parent_identity
            {
                return Err(IosError::compatibility(format!(
                    "iOS plugin output parent '{}' changed after validation",
                    self.parent.display()
                )));
            }
            let current = optional_directory_identity(&self.path, "iOS plugin output")?;
            if current != self.initial_identity {
                return Err(IosError::compatibility(format!(
                    "iOS plugin output '{}' changed after validation",
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
        let source = staging.path().to_path_buf();
        let source_identity = directory_identity(&source, "staged iOS plugin output")?;
        check_cancellation(cancellation, "iOS plugin output promotion")?;
        let had_previous = target.revalidate()?;
        check_cancellation(cancellation, "iOS plugin output promotion")?;
        if had_previous {
            exchange_paths(&source, &target.path).map_err(|error| {
                IosError::storage(format!(
                    "failed to atomically exchange iOS plugin output '{}': {error}",
                    target.path.display()
                ))
            })?;
            let previous_identity = directory_identity(&source, "previous iOS plugin output").ok();
            let promoted_identity =
                directory_identity(&target.path, "promoted iOS plugin output").ok();
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
                        "iOS plugin output '{}' changed during atomic promotion",
                        target.path.display()
                    )),
                ));
            }
        } else {
            rename_noreplace(&source, &target.path).map_err(|error| {
                IosError::storage(format!(
                    "failed to atomically publish iOS plugin output '{}': {error}",
                    target.path.display()
                ))
            })?;
        }
        if let Err(error) = sync_directory(&target.parent).and_then(|()| {
            let promoted = directory_identity(&target.path, "promoted iOS plugin output")?;
            if promoted == source_identity {
                Ok(())
            } else {
                Err(IosError::compatibility(format!(
                    "iOS plugin output '{}' changed during promotion",
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
                "iOS plugin staging cleanup failed for '{}': {error}",
                staging_path.display()
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
        let current_target = optional_directory_identity(
            &target.path,
            "published iOS plugin output during rollback",
        );
        if current_target.as_ref().ok() != Some(&Some(source_identity)) {
            return preserve_staging_quarantine(
                staging,
                error,
                "published output ownership changed before rollback",
            );
        }
        if had_previous {
            let current_previous =
                optional_directory_identity(source, "previous iOS plugin output during rollback");
            if current_previous.as_ref().ok() != Some(&target.initial_identity) {
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
                    "failed to sync iOS plugin directory '{}': {error}",
                    path.display()
                ))
            })
    }

    impl IosPluginBuildLock {
        pub(super) fn acquire(root: &Path) -> Result<Self, IosError> {
            use std::os::unix::fs::OpenOptionsExt;

            let metadata = fs::symlink_metadata(root).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect iOS plugin repository root '{}': {error}",
                    root.display()
                ))
            })?;
            if !metadata.file_type().is_dir() {
                return Err(IosError::compatibility(format!(
                    "iOS plugin repository root '{}' must be a regular non-symlink directory",
                    root.display()
                )));
            }
            let path = root.join(".vesper-ios-plugin.lock");
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
                            "failed to open regular non-symlink iOS plugin build lock '{}': {open_error}",
                            path.display()
                        ))
                    })?
                }
                Err(error) => {
                    return Err(IosError::storage(format!(
                        "failed to create iOS plugin build lock '{}': {error}",
                        path.display()
                    )));
                }
            };
            if !file
                .metadata()
                .map_err(|error| {
                    IosError::storage(format!(
                        "failed to inspect iOS plugin build lock '{}': {error}",
                        path.display()
                    ))
                })?
                .file_type()
                .is_file()
            {
                return Err(IosError::compatibility(format!(
                    "iOS plugin build lock '{}' must be a regular non-symlink file",
                    path.display()
                )));
            }
            match file.try_lock() {
                Ok(()) => Ok(Self {
                    _file: file,
                    root_identity: file_identity(&metadata),
                }),
                Err(TryLockError::WouldBlock) => Err(IosError::compatibility(format!(
                    "another iOS plugin build is already active for '{}'",
                    root.display()
                ))),
                Err(TryLockError::Error(error)) => Err(IosError::storage(format!(
                    "failed to lock iOS plugin build for '{}': {error}",
                    root.display()
                ))),
            }
        }

        pub(super) fn validate_root(&self, root: &Path) -> Result<(), IosError> {
            let metadata = fs::symlink_metadata(root).map_err(|error| {
                IosError::storage(format!(
                    "failed to recheck iOS plugin repository root '{}': {error}",
                    root.display()
                ))
            })?;
            if !metadata.file_type().is_dir() || file_identity(&metadata) != self.root_identity {
                return Err(IosError::compatibility(format!(
                    "iOS plugin build guard does not belong to repository root '{}'",
                    root.display()
                )));
            }
            Ok(())
        }
    }

    fn report_build_result(
        plugin: &IosPluginSpec,
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
                        "iOS {} build was cancelled after the output was committed at '{}'",
                        plugin.description,
                        outcome.output_directory.display()
                    )));
                }
                writeln!(output).map_err(output_error)?;
                writeln!(
                    output,
                    "Built iOS {} plugin libraries into:",
                    plugin.crate_name
                )
                .map_err(output_error)?;
                writeln!(output, "  {}", outcome.output_directory.display())
                    .map_err(output_error)?;
                if let (Some(directory), Some(profile)) =
                    (outcome.ffmpeg_output_directory, outcome.ffmpeg_profile)
                {
                    writeln!(output, "Using Apple FFmpeg prebuilts:").map_err(output_error)?;
                    writeln!(output, "  {}", directory.display()).map_err(output_error)?;
                    writeln!(output, "FFmpeg profile:").map_err(output_error)?;
                    writeln!(output, "  {profile}").map_err(output_error)?;
                    writeln!(output, "Selected slices:").map_err(output_error)?;
                    for slice in outcome.slices {
                        writeln!(output, "  {}", slice.as_str()).map_err(output_error)?;
                    }
                    writeln!(
                        output,
                        "This dylib is an intermediate build input; package it as {}.framework for app distribution.",
                        plugin.framework_name
                    )
                    .map_err(output_error)?;
                }
                output.flush().map_err(output_error)
            }
            Err(error) if cancelled => Err(IosError::worker(format!(
                "iOS {} build was cancelled; {error}",
                plugin.description
            ))),
            Err(error) => Err(error),
        }
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

    fn run_process_capture(
        command: &mut Command,
        label: &str,
        cancellation: &external_process::InterruptDeferral,
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

    fn trim_ascii(bytes: &[u8]) -> &[u8] {
        let start = bytes
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .unwrap_or(bytes.len());
        let end = bytes
            .iter()
            .rposition(|byte| !byte.is_ascii_whitespace())
            .map_or(start, |index| index + 1);
        &bytes[start..end]
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

    fn map_ffmpeg_error(error: crate::ffmpeg::FfmpegError) -> IosError {
        match error.kind() {
            crate::ffmpeg::FfmpegErrorKind::Storage => IosError::storage(error.to_string()),
            crate::ffmpeg::FfmpegErrorKind::Compatibility => {
                IosError::compatibility(error.to_string())
            }
            crate::ffmpeg::FfmpegErrorKind::Conformance => IosError::conformance(error.to_string()),
            crate::ffmpeg::FfmpegErrorKind::Worker => IosError::worker(error.to_string()),
        }
    }

    fn diagnostics_error(error: io::Error) -> IosError {
        IosError::storage(format!("failed to write iOS plugin diagnostics: {error}"))
    }

    fn output_error(error: io::Error) -> IosError {
        IosError::storage(format!("failed to write iOS plugin output: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_registry_preserves_raw_and_release_profile_boundaries() {
        let remux = IosPluginId::RemuxFfmpeg.spec();
        assert_eq!(remux.release_ffmpeg_profile, Some("default"));
        assert!(remux.uses_ffmpeg);
        assert!(remux.link_headerpad);

        let normalizer = IosPluginId::SourceNormalizerFfmpeg.spec();
        assert_eq!(normalizer.release_ffmpeg_profile, Some("source-normalizer"));
        assert!(normalizer.uses_ffmpeg);
        assert!(!normalizer.link_headerpad);

        for plugin in [
            IosPluginId::DecoderVideoToolbox,
            IosPluginId::FrameProcessorDiagnostic,
        ] {
            let spec = plugin.spec();
            assert_eq!(spec.release_ffmpeg_profile, None);
            assert!(!spec.uses_ffmpeg);
            assert!(!spec.link_headerpad);
        }
    }

    #[test]
    fn plugin_registry_has_unique_ids_and_distribution_names() {
        let ids = IOS_PLUGIN_SPECS
            .iter()
            .map(|spec| spec.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let crates = IOS_PLUGIN_SPECS
            .iter()
            .map(|spec| spec.crate_name)
            .collect::<std::collections::BTreeSet<_>>();
        let dylibs = IOS_PLUGIN_SPECS
            .iter()
            .map(|spec| spec.dylib_name)
            .collect::<std::collections::BTreeSet<_>>();
        let frameworks = IOS_PLUGIN_SPECS
            .iter()
            .map(|spec| spec.framework_name)
            .collect::<std::collections::BTreeSet<_>>();
        let bundles = IOS_PLUGIN_SPECS
            .iter()
            .map(|spec| spec.bundle_identifier)
            .collect::<std::collections::BTreeSet<_>>();
        let build_directories = IOS_PLUGIN_SPECS
            .iter()
            .map(|spec| spec.build_directory)
            .collect::<std::collections::BTreeSet<_>>();

        for values in [ids, crates, dylibs, frameworks, bundles, build_directories] {
            assert_eq!(values.len(), IOS_PLUGIN_SPECS.len());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn lipo_architecture_verification_places_the_binary_before_the_operation() {
        let command = implementation::lipo_verify_arm64_command(
            Path::new("/usr/bin/lipo"),
            Path::new("Fixture.framework/Fixture"),
        );
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                std::ffi::OsStr::new("Fixture.framework/Fixture"),
                std::ffi::OsStr::new("-verify_arch"),
                std::ffi::OsStr::new("arm64"),
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ios_plugin_deployment_target_enforces_the_product_floor() {
        for value in ["17.0", "17.0.1", "26.5"] {
            implementation::validate_ios_deployment_target(value)
                .expect("supported deployment target");
        }
        for value in ["", "16.9", "17.beta", "17.0.0.0"] {
            let error = implementation::validate_ios_deployment_target(value)
                .expect_err("unsupported deployment target must fail");
            assert_eq!(error.kind(), crate::ios::IosErrorKind::Compatibility);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ffmpeg_provenance_hashes_exact_bytes_and_paths_without_loss() {
        use sha2::{Digest, Sha256};

        let directory = tempfile::tempdir().expect("create FFmpeg provenance fixture");
        let metadata = [b'm', b'e', b't', b'a', 0, 0xff];
        let checksums = b"avutil_sha256=fixture\n";
        std::fs::write(
            directory.path().join("vesper-ffmpeg-build-metadata.txt"),
            metadata,
        )
        .expect("write exact FFmpeg metadata bytes");
        std::fs::write(
            directory.path().join("vesper-ffmpeg-library-sha256.txt"),
            checksums,
        )
        .expect("write exact FFmpeg checksum bytes");
        let fingerprint = implementation::ffmpeg_build_input_fingerprint(directory.path())
            .expect("fingerprint FFmpeg provenance");
        assert_eq!(
            fingerprint,
            format!(
                "{:x}-{:x}",
                Sha256::digest(metadata),
                Sha256::digest(checksums)
            )
        );

        let spaced = implementation::ffmpeg_directory_cache_key(Path::new("/tmp/a b"))
            .expect("fingerprint spaced path");
        let underscored = implementation::ffmpeg_directory_cache_key(Path::new("/tmp/a_b"))
            .expect("fingerprint underscored path");
        assert_ne!(spaced, underscored);
        assert!(spaced.starts_with("path-") && spaced.len() == 69);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ffmpeg_provenance_rejects_empty_symlink_and_oversized_inputs() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("create invalid FFmpeg provenance fixture");
        let metadata = directory.path().join("vesper-ffmpeg-build-metadata.txt");
        let checksums = directory.path().join("vesper-ffmpeg-library-sha256.txt");
        std::fs::write(&metadata, []).expect("write empty FFmpeg metadata");
        std::fs::write(&checksums, b"checksums\n").expect("write FFmpeg checksums");
        let error = implementation::ffmpeg_build_input_fingerprint(directory.path())
            .expect_err("reject empty FFmpeg provenance");
        assert_eq!(error.kind(), crate::ios::IosErrorKind::Conformance);

        std::fs::write(&metadata, b"metadata\n").expect("write valid FFmpeg metadata");
        std::fs::remove_file(&checksums).expect("remove FFmpeg checksum file");
        let target = directory.path().join("checksum-target.txt");
        std::fs::write(&target, b"checksums\n").expect("write checksum symlink target");
        symlink(&target, &checksums).expect("create checksum symlink");
        let error = implementation::ffmpeg_build_input_fingerprint(directory.path())
            .expect_err("reject symlink FFmpeg provenance");
        assert_eq!(error.kind(), crate::ios::IosErrorKind::Conformance);

        std::fs::remove_file(&checksums).expect("remove checksum symlink");
        std::fs::write(
            &checksums,
            vec![b'x'; implementation::MAX_FFMPEG_PROVENANCE_BYTES as usize + 1],
        )
        .expect("write oversized FFmpeg checksum record");
        let error = implementation::ffmpeg_build_input_fingerprint(directory.path())
            .expect_err("reject oversized FFmpeg provenance");
        assert_eq!(error.kind(), crate::ios::IosErrorKind::Conformance);
    }
}
