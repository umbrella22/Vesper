use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use semver::Version;
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;
use zip::ZipArchive;

use crate::{
    external_process, ffmpeg,
    ffmpeg_source::{FfmpegSourcePolicy, FfmpegSourcePolicyErrorKind},
    gradle,
};
use player_cli::{EmbeddedRegistryFragment, EmbeddedRegistryTarget, PluginProjectManifest};

const DEFAULT_ANDROID_ABI: &str = "arm64-v8a";
const ANDROID_RUST_TARGET: &str = "aarch64-linux-android";
const DEFAULT_ANDROID_NDK_VERSION: &str = "29.0.14206865";
const ANDROID_JNI_LIBRARY: &str = "libvesper_player_android.so";
const ANDROID_DECODER_LIBRARY: &str = "libvesper_decoder_mediacodec.so";
const ANDROID_SOURCE_NORMALIZER_LIBRARY: &str = "libvesper_source_normalizer_ffmpeg.so";
const ANDROID_REMUX_LIBRARY: &str = "libvesper_remux_ffmpeg.so";
const ANDROID_FRAME_PROCESSOR_LIBRARY: &str = "libvesper_frame_processor_diagnostic.so";
const ANDROID_PERFORMANCE_DIAGNOSTICS_LIBRARY: &str = "libvesper_performance_diagnostics.so";
const ANDROID_RELAY_LIBRARY: &str = "libvesper_player_relay_ffmpeg.so";
const FFMPEG_PROFILE_HASH: &str = "profile-hash.txt";
const SOURCE_NORMALIZER_PROFILE_METADATA: &str = "source-normalizer-profile.txt";
const REMUX_PROFILE_METADATA: &str = "remux-profile.txt";
const MAX_ANDROID_NDK_DIRECTORY_ENTRIES: usize = 128;
const MAX_ANDROID_PLUGIN_OUTPUT_ENTRIES: usize = 4096;
const MAX_ANDROID_PLUGIN_OUTPUT_DEPTH: usize = 16;
const MAX_ANDROID_FFMPEG_METADATA_BYTES: u64 = 64 * 1024;
const MAX_ANDROID_RELEASE_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ANDROID_RELEASE_ARCHIVE_ENTRIES: usize = 4096;
const MAX_ANDROID_RELEASE_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ANDROID_RELEASE_EXPANDED_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ANDROID_RELEASE_PATH_BYTES: usize = 512;
const MAX_ANDROID_RELEASE_PATH_DEPTH: usize = 32;
const MAX_ANDROID_RELEASE_COMPRESSION_RATIO: u64 = 1000;
const ANDROID_RELEASE_COMPRESSION_RATIO_FLOOR: u64 = 1024 * 1024;
const MAX_ANDROID_SAMPLE_APK_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ANDROID_SAMPLE_APK_ENTRIES: usize = 8192;
const MAX_ANDROID_SAMPLE_APK_EXPANDED_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ANDROID_SAMPLE_APK_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ANDROID_SAMPLE_APK_PATH_BYTES: usize = 512;
const MAX_ANDROID_SAMPLE_APK_COMPRESSION_RATIO: u64 = 1000;
const ANDROID_SAMPLE_APK_COMPRESSION_RATIO_FLOOR: u64 = 1024 * 1024;
const PARENT_SUPERVISES_PROCESS_GROUP_ENV: &str = "VESPER_ANDROID_PARENT_SUPERVISES_PROCESS_GROUP";
const PARENT_HOLDS_JNI_LOCK_ENV: &str = "VESPER_ANDROID_PARENT_HOLDS_JNI_LOCK";
const HOST_JNI_STAGING_ENV: &str = "VESPER_ANDROID_HOST_JNI_LIBS";
const DECODER_JNI_STAGING_ENV: &str = "VESPER_ANDROID_DECODER_JNI_LIBS";
const SOURCE_NORMALIZER_JNI_STAGING_ENV: &str = "VESPER_ANDROID_SOURCE_NORMALIZER_JNI_LIBS";
const SOURCE_NORMALIZER_ASSETS_STAGING_ENV: &str = "VESPER_ANDROID_SOURCE_NORMALIZER_ASSETS";
const REMUX_JNI_STAGING_ENV: &str = "VESPER_ANDROID_REMUX_JNI_LIBS";
const REMUX_ASSETS_STAGING_ENV: &str = "VESPER_ANDROID_REMUX_ASSETS";
const FRAME_PROCESSOR_JNI_STAGING_ENV: &str = "VESPER_ANDROID_FRAME_PROCESSOR_JNI_LIBS";
const PERFORMANCE_DIAGNOSTICS_JNI_STAGING_ENV: &str =
    "VESPER_ANDROID_PERFORMANCE_DIAGNOSTICS_JNI_LIBS";
const FFMPEG_RUNTIME_JNI_STAGING_ENV: &str = "VESPER_ANDROID_FFMPEG_RUNTIME_JNI_LIBS";
const FFMPEG_RUNTIME_ASSETS_STAGING_ENV: &str = "VESPER_ANDROID_FFMPEG_RUNTIME_ASSETS";
const EXTERNAL_RELAY_JNI_STAGING_ENV: &str = "VESPER_ANDROID_EXTERNAL_RELAY_JNI_LIBS";
const EXTERNAL_RELAY_ASSETS_STAGING_ENV: &str = "VESPER_ANDROID_EXTERNAL_RELAY_ASSETS";
const ANDROID_FFMPEG_VERSION_ENV: &str = "VESPER_ANDROID_FFMPEG_VERSION";
const ANDROID_FFMPEG_SOURCE_URL_ENV: &str = "VESPER_ANDROID_FFMPEG_SOURCE_URL";
const ANDROID_FFMPEG_SOURCE_SHA256_ENV: &str = "VESPER_ANDROID_FFMPEG_SOURCE_SHA256";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AndroidErrorKind {
    Usage,
    Storage,
    Compatibility,
    Conformance,
    Worker,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct AndroidError {
    kind: AndroidErrorKind,
    message: String,
}

struct MavenStagingOptions<'a> {
    repository_directory: &'a Path,
    group_id: &'a str,
    version: &'a str,
    signing_key: &'a str,
    signing_passphrase: Option<&'a str>,
}

impl AndroidError {
    pub(crate) fn usage(message: impl Into<String>) -> Self {
        Self {
            kind: AndroidErrorKind::Usage,
            message: message.into(),
        }
    }

    pub(crate) fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: AndroidErrorKind::Storage,
            message: message.into(),
        }
    }

    pub(crate) fn compatibility(message: impl Into<String>) -> Self {
        Self {
            kind: AndroidErrorKind::Compatibility,
            message: message.into(),
        }
    }

    pub(crate) fn conformance(message: impl Into<String>) -> Self {
        Self {
            kind: AndroidErrorKind::Conformance,
            message: message.into(),
        }
    }

    pub(crate) fn worker(message: impl Into<String>) -> Self {
        Self {
            kind: AndroidErrorKind::Worker,
            message: message.into(),
        }
    }

    pub(crate) const fn kind(&self) -> AndroidErrorKind {
        self.kind
    }

    fn with_suffix(self, suffix: impl std::fmt::Display) -> Self {
        Self {
            kind: self.kind,
            message: format!("{}; {suffix}", self.message),
        }
    }
}

pub(crate) fn include_optional_plugins(cli_value: Option<bool>) -> bool {
    cli_value.unwrap_or_else(|| {
        env::var_os("VESPER_ANDROID_INCLUDE_OPTIONAL_PLUGINS").is_some_and(|value| {
            matches!(value.to_str(), Some("1" | "true" | "TRUE" | "yes" | "YES"))
        })
    })
}

#[derive(Debug, Clone)]
struct AndroidFfmpegReleaseLock {
    version: String,
    source_url: String,
    source_sha256: String,
}

impl AndroidFfmpegReleaseLock {
    fn load(root: &Path) -> Result<Self, AndroidError> {
        let policy = FfmpegSourcePolicy::load(root).map_err(|error| match error.kind() {
            FfmpegSourcePolicyErrorKind::Storage => AndroidError::storage(error.to_string()),
            FfmpegSourcePolicyErrorKind::Invalid => AndroidError::conformance(error.to_string()),
        })?;
        let release = policy.release();
        Ok(Self {
            version: release.version().to_string(),
            source_url: release.source_url().to_owned(),
            source_sha256: release.source_sha256().to_owned(),
        })
    }

    fn apply(&self, command: &mut Command) {
        command
            .env(ANDROID_FFMPEG_VERSION_ENV, &self.version)
            .env(ANDROID_FFMPEG_SOURCE_URL_ENV, &self.source_url)
            .env(ANDROID_FFMPEG_SOURCE_SHA256_ENV, &self.source_sha256);
    }
}

#[derive(Debug, Clone, Copy)]
enum RuntimeFreePlugin {
    DecoderMediaCodec,
    FrameProcessorDiagnostic,
    PerformanceDiagnostics,
}

#[derive(Debug, Clone, Copy)]
enum FfmpegPlugin {
    Remux,
    SourceNormalizer,
}

impl FfmpegPlugin {
    fn parse(value: &str) -> Result<Self, AndroidError> {
        match value {
            "remux" => Ok(Self::Remux),
            "source-normalizer" => Ok(Self::SourceNormalizer),
            _ => Err(AndroidError::conformance(format!(
                "unsupported internal Android FFmpeg plugin: {value}"
            ))),
        }
    }

    const fn crate_name(self) -> &'static str {
        match self {
            Self::Remux => "player-remux-ffmpeg",
            Self::SourceNormalizer => "player-source-normalizer-ffmpeg",
        }
    }

    const fn library_name(self) -> &'static str {
        match self {
            Self::Remux => ANDROID_REMUX_LIBRARY,
            Self::SourceNormalizer => ANDROID_SOURCE_NORMALIZER_LIBRARY,
        }
    }

    const fn profile_metadata_name(self) -> &'static str {
        match self {
            Self::Remux => REMUX_PROFILE_METADATA,
            Self::SourceNormalizer => SOURCE_NORMALIZER_PROFILE_METADATA,
        }
    }

    const fn display_name(self) -> &'static str {
        match self {
            Self::Remux => "remux",
            Self::SourceNormalizer => "SourceNormalizer",
        }
    }

    const fn default_profile(self) -> &'static str {
        match self {
            Self::Remux => "download-remux",
            Self::SourceNormalizer => "default",
        }
    }
}

#[derive(Debug)]
struct FfmpegPluginRequest {
    output_directory: PathBuf,
    metadata_directory: Option<PathBuf>,
    release: bool,
    profile: String,
}

impl RuntimeFreePlugin {
    fn parse(value: &str) -> Result<Self, AndroidError> {
        match value {
            "decoder-mediacodec" => Ok(Self::DecoderMediaCodec),
            "frame-processor-diagnostic" => Ok(Self::FrameProcessorDiagnostic),
            "performance-diagnostics" => Ok(Self::PerformanceDiagnostics),
            _ => Err(AndroidError::conformance(format!(
                "unsupported internal Android runtime-free plugin: {value}"
            ))),
        }
    }

    const fn crate_name(self) -> &'static str {
        match self {
            Self::DecoderMediaCodec => "player-decoder-mediacodec",
            Self::FrameProcessorDiagnostic => "player-frame-processor-diagnostic",
            Self::PerformanceDiagnostics => "player-performance-diagnostics",
        }
    }

    const fn library_name(self) -> &'static str {
        match self {
            Self::DecoderMediaCodec => ANDROID_DECODER_LIBRARY,
            Self::FrameProcessorDiagnostic => ANDROID_FRAME_PROCESSOR_LIBRARY,
            Self::PerformanceDiagnostics => ANDROID_PERFORMANCE_DIAGNOSTICS_LIBRARY,
        }
    }

    const fn public_command(self) -> &'static str {
        match self {
            Self::DecoderMediaCodec => "decoder-mediacodec-plugin",
            Self::FrameProcessorDiagnostic => "frame-processor-plugin",
            Self::PerformanceDiagnostics => "performance-diagnostics-plugin",
        }
    }
}

#[derive(Debug)]
struct RuntimeFreePluginRequest {
    output_directory: PathBuf,
    release: bool,
}

pub(crate) fn build_runtime_free_plugin(
    root: &Path,
    plugin: &str,
    arguments: &[OsString],
    output: &mut dyn Write,
) -> Result<(), AndroidError> {
    let plugin = RuntimeFreePlugin::parse(plugin)?;
    let request = parse_runtime_free_plugin_request(root, plugin, arguments)?;
    let _build_lock = AndroidBuildLock::acquire(root, "runtime-free-plugin")?;
    let selected_abis = resolve_selected_abis(&[])?;
    let cargo_ndk = require_path_command(
        "cargo-ndk",
        &format!(
            "cargo-ndk is required to build Android {} plugins.\nInstall it with: cargo install cargo-ndk",
            plugin.crate_name()
        ),
    )?;
    let cargo = require_path_command("cargo", "cargo is required to build Android plugins")?;
    require_rust_target(ANDROID_RUST_TARGET)?;

    let sdk_root = android_sdk_root()?;
    let ndk_version = android_ndk_version();
    let ndk_root = resolve_ndk_root(&sdk_root, &ndk_version)?;
    let target = external_generated_directory_target(&request.output_directory)?;
    let staging = tempfile::Builder::new()
        .prefix(".vesper-android-plugin-stage-")
        .tempdir_in(&target.canonical_parent)
        .map_err(|error| {
            AndroidError::storage(format!(
                "failed to create Android plugin staging directory beside '{}': {error}",
                target.path.display()
            ))
        })?;

    for abi in &selected_abis {
        let mut command = Command::new(&cargo_ndk);
        command
            .current_dir(root)
            .arg("ndk")
            .arg("-o")
            .arg(staging.path())
            .arg("-t")
            .arg(abi)
            .arg("build")
            .arg("-p")
            .arg(plugin.crate_name());
        if request.release {
            command.arg("--release");
        }
        command
            .env("ANDROID_SDK_ROOT", &sdk_root)
            .env("ANDROID_NDK_ROOT", &ndk_root)
            .env("ANDROID_NDK_HOME", &ndk_root)
            .env("CARGO", &cargo)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        require_success(
            &mut command,
            &format!("Android {} plugin build", plugin.crate_name()),
        )?;
    }

    validate_runtime_free_plugin_output(staging.path(), plugin, &selected_abis)?;
    promote_generated_directory(staging, &target)?;

    writeln!(output).map_err(output_error)?;
    writeln!(
        output,
        "Built Android {} plugin libraries into:",
        plugin.crate_name()
    )
    .map_err(output_error)?;
    writeln!(output, "  {}", request.output_directory.display()).map_err(output_error)?;
    output.flush().map_err(output_error)
}

fn parse_runtime_free_plugin_request(
    root: &Path,
    plugin: RuntimeFreePlugin,
    arguments: &[OsString],
) -> Result<RuntimeFreePluginRequest, AndroidError> {
    let Some(output) = arguments.first() else {
        return Err(AndroidError::usage(format!(
            "Usage: vesper android {} <output-dir> [debug|release]",
            plugin.public_command(),
        )));
    };
    if output.is_empty() {
        return Err(AndroidError::usage(
            "Android plugin output directory must not be empty",
        ));
    }
    let release = match arguments.get(1).map(OsString::as_os_str) {
        None => false,
        Some(value) if value == OsStr::new("debug") => false,
        Some(value) if value == OsStr::new("release") => true,
        Some(value) => {
            return Err(AndroidError::usage(format!(
                "unexpected Android plugin build profile: {}",
                value.to_string_lossy()
            )));
        }
    };
    if arguments.len() > 2 {
        return Err(AndroidError::usage(format!(
            "unexpected Android plugin arguments: {}",
            arguments[2..]
                .iter()
                .map(|value| value.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
        )));
    }
    let output_directory = PathBuf::from(output);
    Ok(RuntimeFreePluginRequest {
        output_directory: if output_directory.is_absolute() {
            output_directory
        } else {
            root.join(output_directory)
        },
        release,
    })
}

pub(crate) fn build_ffmpeg_plugin(
    root: &Path,
    plugin: &str,
    arguments: &[OsString],
    output: &mut dyn Write,
) -> Result<(), AndroidError> {
    let plugin = FfmpegPlugin::parse(plugin)?;
    let request = parse_ffmpeg_plugin_request(root, plugin, arguments)?;
    let _build_lock = AndroidBuildLock::acquire(root, "ffmpeg-plugin")?;
    let selected_abis = resolve_selected_abis(&[])?;
    let cargo_ndk = require_path_command(
        "cargo-ndk",
        &format!(
            "cargo-ndk is required to build Android {} plugins.\nInstall it with: cargo install cargo-ndk",
            plugin.crate_name()
        ),
    )?;
    let cargo = require_path_command("cargo", "cargo is required to build Android plugins")?;
    require_rust_target(ANDROID_RUST_TARGET)?;

    let sdk_root = android_sdk_root()?;
    let ndk_version = android_ndk_version();
    let ndk_root = resolve_ndk_root(&sdk_root, &ndk_version)?;
    let ffmpeg_root = android_ffmpeg_output_directory(root);
    let mut ffmpeg_request = android_ffmpeg_request(&request.profile, &selected_abis);
    ffmpeg_request.output_directory = Some(ffmpeg_root.clone());
    let profile =
        ffmpeg::resolve_profile_identity(root, &ffmpeg_request, ffmpeg::FfmpegPlatform::Android)
            .map_err(map_ffmpeg_error)?;
    ffmpeg::run(root, &ffmpeg_request, output).map_err(map_ffmpeg_error)?;

    let output_stage = StagedGeneratedDirectory::new_external(
        &request.output_directory,
        ".vesper-android-ffmpeg-plugin-stage-",
        "Android FFmpeg plugin output",
    )?;
    let metadata_stage = request
        .metadata_directory
        .as_deref()
        .map(|path| {
            StagedGeneratedDirectory::new_external(
                path,
                ".vesper-android-ffmpeg-plugin-metadata-stage-",
                "Android FFmpeg plugin metadata",
            )
        })
        .transpose()?;

    if let Some(stage) = metadata_stage.as_ref() {
        write_ffmpeg_plugin_profile_metadata(stage.path(), plugin, &profile.name, &profile.hash)?;
    }

    for abi in &selected_abis {
        let ffmpeg_abi_directory = ffmpeg_root.join(abi);
        let pkgconfig_directory = ffmpeg_abi_directory.join("lib/pkgconfig");
        require_regular_directory(
            &pkgconfig_directory,
            &format!("shared FFmpeg runtime pkg-config directory for ABI {abi}"),
        )?;
        let metadata_path = ffmpeg_abi_directory.join("vesper-ffmpeg-build-metadata.txt");
        let configure_metadata = read_optional_ffmpeg_metadata(&metadata_path)?;
        if let Some(stage) = metadata_stage.as_ref()
            && metadata_path.exists()
        {
            copy_ffmpeg_plugin_build_metadata(
                &metadata_path,
                &stage
                    .path()
                    .join(format!("{abi}-vesper-ffmpeg-build-metadata.txt")),
                &profile.hash,
            )?;
        }

        let mut command = Command::new(&cargo_ndk);
        command
            .current_dir(root)
            .arg("ndk")
            .arg("-o")
            .arg(output_stage.path())
            .arg("-t")
            .arg(abi)
            .arg("build")
            .arg("-p")
            .arg(plugin.crate_name());
        if request.release {
            command.arg("--release");
        }
        command
            .env("ANDROID_SDK_ROOT", &sdk_root)
            .env("ANDROID_NDK_ROOT", &ndk_root)
            .env("ANDROID_NDK_HOME", &ndk_root)
            .env("CARGO", &cargo)
            .env("PKG_CONFIG_ALLOW_CROSS", "1")
            .env("PKG_CONFIG_PATH", &pkgconfig_directory)
            .env("VESPER_FFMPEG_PROFILE_HASH", &profile.hash)
            .env("VESPER_FFMPEG_CONFIGURE_METADATA", configure_metadata)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        require_success(
            &mut command,
            &format!("Android {} plugin build", plugin.crate_name()),
        )?;
    }

    validate_ffmpeg_plugin_output(output_stage.path(), plugin, &selected_abis)?;
    output_stage.validate()?;
    if let Some(stage) = metadata_stage.as_ref() {
        validate_ffmpeg_plugin_metadata(stage.path(), plugin, &selected_abis)?;
        stage.validate()?;
    }

    let cancellation = external_process::InterruptDeferral::start("Android FFmpeg plugin output")
        .map_err(|error| AndroidError::worker(error.to_string()))?;
    let mut stages = vec![output_stage];
    if let Some(stage) = metadata_stage {
        stages.push(stage);
    }
    let promotion = promote_staged_directories(stages, &cancellation);
    let cancelled = cancellation.finish();
    match (promotion, cancelled) {
        (Ok(()), true) => {
            return Err(AndroidError::worker(
                "Android FFmpeg plugin build was cancelled after its outputs were committed",
            ));
        }
        (Err(error), true) => {
            return Err(AndroidError::worker(format!(
                "Android FFmpeg plugin build was cancelled; {error}"
            )));
        }
        (Err(error), false) => return Err(error),
        (Ok(()), false) => {}
    }

    writeln!(output).map_err(output_error)?;
    writeln!(
        output,
        "Built Android {} plugin libraries into:",
        plugin.crate_name()
    )
    .map_err(output_error)?;
    writeln!(output, "  {}", request.output_directory.display()).map_err(output_error)?;
    writeln!(output, "FFmpeg profile:").map_err(output_error)?;
    writeln!(output, "  {}", request.profile).map_err(output_error)?;
    if matches!(plugin, FfmpegPlugin::Remux) {
        writeln!(
            output,
            "The plugin no longer copies FFmpeg runtime libraries; package vesper-player-kit-ffmpeg-runtime instead."
        )
        .map_err(output_error)?;
    }
    output.flush().map_err(output_error)
}

fn parse_ffmpeg_plugin_request(
    root: &Path,
    plugin: FfmpegPlugin,
    arguments: &[OsString],
) -> Result<FfmpegPluginRequest, AndroidError> {
    let Some(output) = arguments.first() else {
        return Err(AndroidError::usage(ffmpeg_plugin_usage(plugin)));
    };
    if output.is_empty() {
        return Err(AndroidError::usage(
            "Android plugin output directory must not be empty",
        ));
    }

    let mut index = 1_usize;
    let mut release = false;
    if let Some(value) = arguments.get(index).map(OsString::as_os_str) {
        if value == OsStr::new("debug") {
            index += 1;
        } else if value == OsStr::new("release") {
            release = true;
            index += 1;
        }
    }
    let mut profile = plugin.default_profile().to_owned();
    let mut metadata_directory = None;
    while index < arguments.len() {
        let argument = &arguments[index];
        let Some(text) = argument.to_str() else {
            return Err(AndroidError::usage(
                "Android FFmpeg plugin options must be valid UTF-8",
            ));
        };
        match text {
            "--profile" => {
                index += 1;
                let value = arguments.get(index).and_then(|value| value.to_str());
                profile = value
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| AndroidError::usage("--profile requires a value"))?
                    .to_owned();
            }
            "--metadata-dir" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| AndroidError::usage("--metadata-dir requires a value"))?;
                metadata_directory = Some(resolve_output_path(root, value));
            }
            _ if text.starts_with("--profile=") => {
                profile = text
                    .strip_prefix("--profile=")
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| AndroidError::usage("--profile requires a value"))?
                    .to_owned();
            }
            _ if text.starts_with("--metadata-dir=") => {
                let value = text
                    .strip_prefix("--metadata-dir=")
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| AndroidError::usage("--metadata-dir requires a value"))?;
                metadata_directory = Some(resolve_output_path(root, OsStr::new(value)));
            }
            _ => {
                return Err(AndroidError::usage(format!(
                    "unexpected Android FFmpeg plugin argument: {text}\n{}",
                    ffmpeg_plugin_usage(plugin)
                )));
            }
        }
        index += 1;
    }

    Ok(FfmpegPluginRequest {
        output_directory: resolve_output_path(root, output),
        metadata_directory,
        release,
        profile,
    })
}

fn ffmpeg_plugin_usage(plugin: FfmpegPlugin) -> &'static str {
    match plugin {
        FfmpegPlugin::Remux => {
            "Usage: vesper android remux-plugin <output-dir> [debug|release] [--profile <name>] [--metadata-dir <dir>]"
        }
        FfmpegPlugin::SourceNormalizer => {
            "Usage: vesper android source-normalizer-plugin <output-dir> [debug|release] [--profile <name>] [--metadata-dir <dir>]"
        }
    }
}

fn resolve_output_path(root: &Path, path: &OsStr) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn android_ffmpeg_request(profile: &str, abis: &[String]) -> ffmpeg::FfmpegRequest {
    ffmpeg::FfmpegRequest {
        profile: Some(profile.to_owned()),
        platform: Some(ffmpeg::FfmpegPlatform::Android),
        list_profiles: false,
        dry_run: false,
        verify_only: false,
        output_directory: None,
        android_artifact: ffmpeg::AndroidArtifact::Prebuilts,
        android_abis: abis.to_vec(),
        ios_slices: Vec::new(),
        extra_libraries: Vec::new(),
        extra_demuxers: Vec::new(),
        extra_muxers: Vec::new(),
        extra_protocols: Vec::new(),
        extra_decoders: Vec::new(),
        extra_parsers: Vec::new(),
        extra_bsfs: Vec::new(),
        extra_configure_args: Vec::new(),
        tls_backend: None,
        force: false,
        acknowledge_gpl_nonfree: false,
    }
}

fn map_ffmpeg_error(error: ffmpeg::FfmpegError) -> AndroidError {
    match error.kind() {
        ffmpeg::FfmpegErrorKind::Storage => AndroidError::storage(error.to_string()),
        ffmpeg::FfmpegErrorKind::Compatibility => AndroidError::compatibility(error.to_string()),
        ffmpeg::FfmpegErrorKind::Conformance => AndroidError::conformance(error.to_string()),
        ffmpeg::FfmpegErrorKind::Worker => AndroidError::worker(error.to_string()),
    }
}

fn android_ffmpeg_output_directory(root: &Path) -> PathBuf {
    let path = env::var_os("VESPER_ANDROID_FFMPEG_OUTPUT_DIR")
        .or_else(|| env::var_os("VESPER_FFMPEG_OUTPUT_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("third_party/ffmpeg/android"));
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn require_regular_directory(path: &Path, label: &str) -> Result<(), AndroidError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AndroidError::compatibility(format!("Missing {label}:\n  {}\n{error}", path.display()))
    })?;
    if metadata.file_type().is_dir() {
        Ok(())
    } else {
        Err(AndroidError::conformance(format!(
            "{label} is not a regular non-symlink directory:\n  {}",
            path.display()
        )))
    }
}

fn read_optional_ffmpeg_metadata(path: &Path) -> Result<String, AndroidError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => {
            return Err(AndroidError::storage(format!(
                "failed to inspect FFmpeg build metadata '{}': {error}",
                path.display()
            )));
        }
    };
    if !metadata.file_type().is_file() {
        return Err(AndroidError::conformance(format!(
            "FFmpeg build metadata must be a regular non-symlink file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_ANDROID_FFMPEG_METADATA_BYTES {
        return Err(AndroidError::conformance(format!(
            "FFmpeg build metadata exceeds {MAX_ANDROID_FFMPEG_METADATA_BYTES} bytes: {}",
            path.display()
        )));
    }
    let bytes = fs::read(path).map_err(|error| {
        AndroidError::storage(format!(
            "failed to read FFmpeg build metadata '{}': {error}",
            path.display()
        ))
    })?;
    Ok(bytes
        .into_iter()
        .map(|byte| match byte {
            b'\n' => ';',
            0x20..=0x7e => char::from(byte),
            _ => '?',
        })
        .collect())
}

fn write_ffmpeg_plugin_profile_metadata(
    root: &Path,
    plugin: FfmpegPlugin,
    profile_name: &str,
    profile_hash: &str,
) -> Result<(), AndroidError> {
    fs::write(root.join(FFMPEG_PROFILE_HASH), format!("{profile_hash}\n"))
        .and_then(|_| {
            fs::write(
                root.join(plugin.profile_metadata_name()),
                format!("profile={profile_name}\nplatform=android\nprofile_hash={profile_hash}\n"),
            )
        })
        .map_err(|error| {
            AndroidError::storage(format!(
                "failed to write Android {} profile metadata: {error}",
                plugin.display_name()
            ))
        })
}

fn copy_ffmpeg_plugin_build_metadata(
    source: &Path,
    destination: &Path,
    profile_hash: &str,
) -> Result<(), AndroidError> {
    let mut bytes = fs::read(source).map_err(|error| {
        AndroidError::storage(format!(
            "failed to read FFmpeg build metadata '{}': {error}",
            source.display()
        ))
    })?;
    bytes.extend_from_slice(format!("profile_hash={profile_hash}\n").as_bytes());
    fs::write(destination, bytes).map_err(|error| {
        AndroidError::storage(format!(
            "failed to write FFmpeg plugin metadata '{}': {error}",
            destination.display()
        ))
    })
}

fn validate_ffmpeg_plugin_output(
    root: &Path,
    plugin: FfmpegPlugin,
    selected_abis: &[String],
) -> Result<(), AndroidError> {
    for abi in selected_abis {
        let artifact = root.join(abi).join(plugin.library_name());
        let metadata = fs::symlink_metadata(&artifact).map_err(|error| {
            AndroidError::conformance(format!(
                "Android {} plugin build did not produce '{}': {error}",
                plugin.crate_name(),
                artifact.display()
            ))
        })?;
        if !metadata.file_type().is_file() {
            return Err(AndroidError::conformance(format!(
                "Android {} plugin output '{}' is not a regular non-symlink file",
                plugin.crate_name(),
                artifact.display()
            )));
        }
    }
    validate_plugin_output_tree(root, plugin.crate_name())
}

fn validate_ffmpeg_plugin_metadata(
    root: &Path,
    plugin: FfmpegPlugin,
    selected_abis: &[String],
) -> Result<(), AndroidError> {
    for name in [FFMPEG_PROFILE_HASH, plugin.profile_metadata_name()] {
        require_regular_file(
            &root.join(name),
            &format!("Android {} metadata", plugin.display_name()),
        )?;
    }
    for abi in selected_abis {
        require_regular_file(
            &root.join(format!("{abi}-vesper-ffmpeg-build-metadata.txt")),
            &format!("Android {} FFmpeg metadata", plugin.display_name()),
        )?;
    }
    Ok(())
}

fn external_generated_directory_target(
    output_directory: &Path,
) -> Result<GeneratedDirectoryTarget, AndroidError> {
    let parent = output_directory.parent().ok_or_else(|| {
        AndroidError::compatibility(format!(
            "Android plugin output '{}' has no parent directory",
            output_directory.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        AndroidError::storage(format!(
            "failed to create Android plugin output parent '{}': {error}",
            parent.display()
        ))
    })?;
    let metadata = fs::symlink_metadata(parent).map_err(|error| {
        AndroidError::storage(format!(
            "failed to inspect Android plugin output parent '{}': {error}",
            parent.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(AndroidError::compatibility(format!(
            "Android plugin output parent '{}' is not a regular non-symlink directory",
            parent.display()
        )));
    }
    let canonical_parent = parent.canonicalize().map_err(|error| {
        AndroidError::storage(format!(
            "failed to resolve Android plugin output parent '{}': {error}",
            parent.display()
        ))
    })?;
    let file_name = output_directory.file_name().ok_or_else(|| {
        AndroidError::compatibility(format!(
            "Android plugin output '{}' has no directory name",
            output_directory.display()
        ))
    })?;
    GeneratedDirectoryTarget::preflight(&canonical_parent, canonical_parent.join(file_name))
}

fn validate_runtime_free_plugin_output(
    root: &Path,
    plugin: RuntimeFreePlugin,
    selected_abis: &[String],
) -> Result<(), AndroidError> {
    for abi in selected_abis {
        let artifact = root.join(abi).join(plugin.library_name());
        let metadata = fs::symlink_metadata(&artifact).map_err(|error| {
            AndroidError::conformance(format!(
                "Android {} plugin build did not produce '{}': {error}",
                plugin.crate_name(),
                artifact.display()
            ))
        })?;
        if !metadata.file_type().is_file() {
            return Err(AndroidError::conformance(format!(
                "Android {} plugin output '{}' is not a regular non-symlink file",
                plugin.crate_name(),
                artifact.display()
            )));
        }
    }
    validate_plugin_output_tree(root, plugin.crate_name())
}

fn validate_plugin_output_tree(root: &Path, crate_name: &str) -> Result<(), AndroidError> {
    validate_android_output_tree(root, crate_name, true)
}

fn validate_android_output_tree(
    root: &Path,
    label: &str,
    reject_runtime_libraries: bool,
) -> Result<(), AndroidError> {
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    let mut entries_seen = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_ANDROID_PLUGIN_OUTPUT_DEPTH {
            return Err(AndroidError::conformance(format!(
                "Android {} plugin output exceeds {MAX_ANDROID_PLUGIN_OUTPUT_DEPTH} directory levels",
                label
            )));
        }
        for entry in fs::read_dir(&directory).map_err(|error| {
            AndroidError::storage(format!(
                "failed to enumerate Android plugin output '{}': {error}",
                directory.display()
            ))
        })? {
            entries_seen = entries_seen.saturating_add(1);
            if entries_seen > MAX_ANDROID_PLUGIN_OUTPUT_ENTRIES {
                return Err(AndroidError::conformance(format!(
                    "Android {} plugin output contains more than {MAX_ANDROID_PLUGIN_OUTPUT_ENTRIES} entries",
                    label
                )));
            }
            let entry = entry.map_err(|error| {
                AndroidError::storage(format!(
                    "failed to read an Android plugin output entry: {error}"
                ))
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                AndroidError::storage(format!(
                    "failed to inspect Android plugin output '{}': {error}",
                    path.display()
                ))
            })?;
            if metadata.file_type().is_dir() {
                pending.push((path, depth + 1));
                continue;
            }
            if !metadata.file_type().is_file() {
                return Err(AndroidError::conformance(format!(
                    "Android {} plugin output contains a link or special file: {}",
                    label,
                    path.display()
                )));
            }
            let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
            if reject_runtime_libraries && is_forbidden_android_runtime_library(name) {
                return Err(AndroidError::conformance(format!(
                    "{} must not bundle FFmpeg runtime libraries:\n  {}",
                    label,
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

fn is_forbidden_android_runtime_library(name: &str) -> bool {
    is_android_runtime_shared_object(name)
        && [
            "libav",
            "libsw",
            "libpostproc",
            "libssl",
            "libcrypto",
            "libxml2",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn parent_managed_jni_output(root: &Path) -> Result<Option<PathBuf>, AndroidError> {
    let configured_output = env::var_os(HOST_JNI_STAGING_ENV);
    let parent_lock_marker = env::var_os(PARENT_HOLDS_JNI_LOCK_ENV);
    match (configured_output, parent_lock_marker) {
        (None, None) => Ok(None),
        (Some(path), Some(marker)) if marker == OsStr::new("1") => {
            let output_parent = require_contained_directory(
                root,
                &root.join("lib/android/vesper-player-kit/src/main"),
                "Android JNI output parent",
            )?;
            let output = require_contained_directory(
                root,
                Path::new(&path),
                "parent-managed Android host JNI staging directory",
            )?;
            let has_expected_name = output
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with(".vesper-android-host-jni-stage-"));
            if output.parent() != Some(output_parent.as_path()) || !has_expected_name {
                return Err(AndroidError::compatibility(format!(
                    "{HOST_JNI_STAGING_ENV} must identify a parent-created host JNI staging directory directly under '{}'",
                    output_parent.display()
                )));
            }
            Ok(Some(output))
        }
        (Some(_), None) => Err(AndroidError::compatibility(format!(
            "{HOST_JNI_STAGING_ENV} requires {PARENT_HOLDS_JNI_LOCK_ENV}=1"
        ))),
        (None, Some(_)) => Err(AndroidError::compatibility(format!(
            "{PARENT_HOLDS_JNI_LOCK_ENV} requires {HOST_JNI_STAGING_ENV}"
        ))),
        (Some(_), Some(_)) => Err(AndroidError::compatibility(format!(
            "{PARENT_HOLDS_JNI_LOCK_ENV} must be exactly 1"
        ))),
    }
}

pub(crate) fn build_jni(
    root: &Path,
    profile: &str,
    requested_abis: &[String],
    output: &mut dyn Write,
) -> Result<(), AndroidError> {
    let parent_managed_output = parent_managed_jni_output(root)?;
    let _build_lock = if parent_managed_output.is_some() {
        None
    } else {
        Some(AndroidBuildLock::acquire(root, "jni")?)
    };
    let selected_abis = resolve_selected_abis(requested_abis)?;
    let cargo_ndk = require_path_command(
        "cargo-ndk",
        "cargo-ndk is required to build Android JNI libraries.\nInstall it with: cargo install cargo-ndk",
    )?;
    let cargo = require_path_command("cargo", "cargo is required to build Android JNI libraries")?;
    require_rust_target(ANDROID_RUST_TARGET)?;

    let sdk_root = android_sdk_root()?;
    let ndk_version = android_ndk_version();
    let ndk_root = resolve_ndk_root(&sdk_root, &ndk_version)?;
    let output_path = match parent_managed_output {
        Some(path) => path,
        None => require_contained_directory(
            root,
            &root.join("lib/android/vesper-player-kit/src/main"),
            "Android JNI output parent",
        )?
        .join("jniLibs"),
    };
    let target = GeneratedDirectoryTarget::preflight(root, output_path)?;
    let staging = tempfile::Builder::new()
        .prefix(".vesper-android-jni-stage-")
        .tempdir_in(&target.canonical_parent)
        .map_err(|error| {
            AndroidError::storage(format!(
                "failed to create Android JNI staging directory beside '{}': {error}",
                target.path.display()
            ))
        })?;

    let mut command = Command::new(cargo_ndk);
    command
        .current_dir(root)
        .arg("ndk")
        .arg("-o")
        .arg(staging.path());
    for abi in &selected_abis {
        command.arg("-t").arg(abi);
    }
    command.arg("build").arg("-p").arg("player-jni-android");
    if profile == "release" {
        command.arg("--release");
    }
    command
        .env("ANDROID_SDK_ROOT", &sdk_root)
        .env("ANDROID_NDK_ROOT", &ndk_root)
        .env("ANDROID_NDK_HOME", &ndk_root)
        .env("CARGO", cargo)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    require_success(&mut command, "Android JNI library build")?;

    for abi in &selected_abis {
        let artifact = staging.path().join(abi).join(ANDROID_JNI_LIBRARY);
        let metadata = fs::symlink_metadata(&artifact).map_err(|error| {
            AndroidError::conformance(format!(
                "Android JNI build did not produce '{}': {error}",
                artifact.display()
            ))
        })?;
        if !metadata.file_type().is_file() {
            return Err(AndroidError::conformance(format!(
                "Android JNI build output '{}' is not a regular non-symlink file",
                artifact.display()
            )));
        }
    }
    promote_generated_directory(staging, &target)?;

    writeln!(output).map_err(output_error)?;
    writeln!(output, "Built Android JNI libraries into:").map_err(output_error)?;
    writeln!(output, "  {}", target.path.display()).map_err(output_error)?;
    writeln!(output, "Selected Android ABIs:").map_err(output_error)?;
    for abi in selected_abis {
        writeln!(output, "  {abi}").map_err(output_error)?;
    }
    output.flush().map_err(output_error)
}

pub(crate) fn build_aar(
    root: &Path,
    module_task: &str,
    include_optional: bool,
) -> Result<(), AndroidError> {
    build_aar_for_abis(root, module_task, include_optional, &[])
}

pub(crate) fn stage_maven_publications(
    root: &Path,
    repository_directory: &Path,
    group_id: &str,
    version: &str,
    signing_key: &str,
    signing_passphrase: Option<&str>,
) -> Result<(), AndroidError> {
    let options = MavenStagingOptions {
        repository_directory,
        group_id,
        version,
        signing_key,
        signing_passphrase,
    };
    build_aar_for_abis_with_maven(
        root,
        "publishReleasePublicationToCentralStagingRepository",
        &[DEFAULT_ANDROID_ABI.to_owned()],
        &options,
    )
}

pub(crate) fn stage_release(
    root: &Path,
    output_directory: Option<&Path>,
    requested_abis: &[String],
    include_optional: bool,
    output: &mut dyn Write,
) -> Result<(), AndroidError> {
    let selected_abis = resolve_selected_abis(requested_abis)?;
    let output_directory = output_directory
        .map(|path| resolve_output_path(root, path.as_os_str()))
        .unwrap_or_else(|| root.join("dist/release/android"));
    let ffmpeg_release_lock = include_optional
        .then(|| AndroidFfmpegReleaseLock::load(root))
        .transpose()?;
    let cancellation = external_process::InterruptDeferral::start("Android release staging")
        .map_err(|error| AndroidError::worker(error.to_string()))?;
    let result = stage_release_transaction(
        root,
        &output_directory,
        &selected_abis,
        include_optional,
        ffmpeg_release_lock.as_ref(),
        &cancellation,
    );
    let cancelled = cancellation.finish();
    match (result, cancelled) {
        (Ok(_), true) => Err(AndroidError::worker(
            "Android release staging was cancelled after its outputs were committed",
        )),
        (Err(error), true) => Err(AndroidError::worker(format!(
            "Android release staging was cancelled; {error}"
        ))),
        (Err(error), false) => Err(error),
        (Ok(artifacts), false) => {
            writeln!(output, "Staged VesperPlayerKit Android AARs:").map_err(output_error)?;
            for artifact in artifacts {
                writeln!(output, "  {}", output_directory.join(artifact).display())
                    .map_err(output_error)?;
            }
            if !include_optional {
                writeln!(
                    output,
                    "Skipped optional Android plugin AARs. Use --include-optional-plugins to stage them."
                )
                .map_err(output_error)?;
            }
            output.flush().map_err(output_error)
        }
    }
}

/// Build the two Android sample applications and publish their APKs as one
/// transaction. The sample release intentionally uses the same FFmpeg source
/// lock as the optional Android release so that the APKs and corresponding
/// source asset cannot drift to different patch versions.
pub(crate) fn sample_apks(
    root: &Path,
    output_directory: Option<&Path>,
    requested_abis: &[String],
    output: &mut dyn Write,
) -> Result<(), AndroidError> {
    let selected_abis = resolve_selected_abis(requested_abis)?;
    let output_directory = output_directory
        .map(|path| resolve_output_path(root, path.as_os_str()))
        .unwrap_or_else(|| root.join("dist/release/android-samples"));
    let ffmpeg_release_lock = AndroidFfmpegReleaseLock::load(root)?;
    let cancellation = external_process::InterruptDeferral::start("Android sample APK staging")
        .map_err(|error| AndroidError::worker(error.to_string()))?;
    let result = sample_apks_transaction(
        root,
        &output_directory,
        &selected_abis,
        &ffmpeg_release_lock,
        &cancellation,
    );
    let cancelled = cancellation.finish();
    match (result, cancelled) {
        (Ok(_), true) => Err(AndroidError::worker(
            "Android sample APK staging was cancelled after its outputs were committed",
        )),
        (Err(error), true) => Err(AndroidError::worker(format!(
            "Android sample APK staging was cancelled; {error}"
        ))),
        (Err(error), false) => Err(error),
        (Ok(artifacts), false) => {
            writeln!(output, "Staged Android sample APKs:").map_err(output_error)?;
            for artifact in artifacts {
                writeln!(output, "  {}", output_directory.join(artifact).display())
                    .map_err(output_error)?;
            }
            output.flush().map_err(output_error)
        }
    }
}

/// Build the JNI fixture consumed by Android instrumentation tests.
///
/// Gradle invokes this typed command instead of owning a second shell build
/// graph. All intermediate directories are private and the final output is
/// promoted only after every ABI and library invariant has been checked.
pub(crate) fn provision_test_jni(
    root: &Path,
    output_directory: &Path,
    build_profile: &str,
    ffmpeg_profile: &str,
    output: &mut dyn Write,
) -> Result<(), AndroidError> {
    if !matches!(build_profile, "debug" | "release") {
        return Err(AndroidError::usage(
            "Android instrumentation JNI profile must be debug or release",
        ));
    }
    if ffmpeg_profile.is_empty() {
        return Err(AndroidError::usage(
            "Android instrumentation FFmpeg profile must not be empty",
        ));
    }
    let expected = root.join("lib/android/vesper-player-kit/build/generated/androidTestJniLibs");
    let output_directory = resolve_output_path(root, output_directory.as_os_str());
    if output_directory != expected {
        return Err(AndroidError::compatibility(format!(
            "Android instrumentation JNI output must stay inside the player-kit generated build directory: {}",
            expected.display()
        )));
    }

    let _build_lock = AndroidBuildLock::acquire(root, "android-test-jni")?;
    let selected_abis = resolve_selected_abis(&[])?;
    let temporary = tempfile::tempdir().map_err(|error| {
        AndroidError::storage(format!(
            "failed to create Android instrumentation JNI staging directory: {error}"
        ))
    })?;
    let decoder = temporary.path().join("decoder");
    let source_normalizer = temporary.path().join("source-normalizer");
    let source_metadata = temporary.path().join("source-normalizer-metadata");
    let runtime = temporary.path().join("runtime");

    let mut sink = io::sink();
    build_runtime_free_plugin(
        root,
        "decoder-mediacodec",
        &[
            decoder.clone().into_os_string(),
            OsString::from(build_profile),
        ],
        &mut sink,
    )?;
    build_ffmpeg_plugin(
        root,
        "source-normalizer",
        &[
            source_normalizer.clone().into_os_string(),
            OsString::from(build_profile),
            OsString::from("--profile"),
            OsString::from(ffmpeg_profile),
            OsString::from("--metadata-dir"),
            source_metadata.clone().into_os_string(),
        ],
        &mut sink,
    )?;

    let ffmpeg_output = android_ffmpeg_output_directory(root);
    let use_openssl =
        ffmpeg_runtime_uses_external_dependency(&ffmpeg_output, &selected_abis, "openssl")?;
    let use_libxml2 =
        ffmpeg_runtime_uses_external_dependency(&ffmpeg_output, &selected_abis, "libxml2")?;
    stage_ffmpeg_runtime_libraries(
        &runtime,
        &ffmpeg_output,
        &root.join("third_party/openssl/android"),
        &root.join("third_party/libxml2/android"),
        use_openssl,
        use_libxml2,
        &selected_abis,
    )?;

    let final_stage = StagedGeneratedDirectory::new_external(
        &output_directory,
        ".vesper-android-test-jni-stage-",
        "Android instrumentation JNI output",
    )?;
    copy_android_abi_files(
        &decoder,
        final_stage.path(),
        &selected_abis,
        "Android decoder instrumentation output",
    )?;
    copy_android_abi_files(
        &source_normalizer,
        final_stage.path(),
        &selected_abis,
        "Android SourceNormalizer instrumentation output",
    )?;
    copy_android_abi_files(
        &runtime,
        final_stage.path(),
        &selected_abis,
        "Android FFmpeg instrumentation output",
    )?;
    for abi in &selected_abis {
        for library in [
            ANDROID_DECODER_LIBRARY,
            ANDROID_SOURCE_NORMALIZER_LIBRARY,
            "libavcodec.so",
            "libavformat.so",
            "libavutil.so",
        ] {
            require_regular_file(
                &final_stage.path().join(abi).join(library),
                "Android instrumentation JNI library",
            )?;
        }
    }
    validate_android_output_tree(final_stage.path(), "instrumentation JNI", false)?;
    let cancellation =
        external_process::InterruptDeferral::start("Android instrumentation JNI output")
            .map_err(|error| AndroidError::worker(error.to_string()))?;
    let promotion = promote_staged_directories(vec![final_stage], &cancellation);
    let cancelled = cancellation.finish();
    match (promotion, cancelled) {
        (Ok(()), false) => {}
        (Ok(()), true) => {
            return Err(AndroidError::worker(
                "Android instrumentation JNI output was cancelled after publication",
            ));
        }
        (Err(error), true) => {
            return Err(AndroidError::worker(format!(
                "Android instrumentation JNI output was cancelled; {error}"
            )));
        }
        (Err(error), false) => return Err(error),
    }
    writeln!(
        output,
        "Provisioned Android instrumentation JNI libraries into:"
    )
    .map_err(output_error)?;
    writeln!(output, "  {}", output_directory.display()).map_err(output_error)?;
    output.flush().map_err(output_error)
}

/// Build the optional external-playback relay JNI library and its profile
/// receipt without invoking an Android shell worker from Gradle.
pub(crate) fn build_external_playback_jni(
    root: &Path,
    output_directory: &Path,
    assets_directory: &Path,
    build_profile: &str,
    ffmpeg_profile: &str,
    skip_ffmpeg_runtime: bool,
    output: &mut dyn Write,
) -> Result<(), AndroidError> {
    if !matches!(build_profile, "debug" | "release") {
        return Err(AndroidError::usage(
            "Android external playback native profile must be debug or release",
        ));
    }
    if ffmpeg_profile.is_empty() {
        return Err(AndroidError::usage(
            "Android external playback FFmpeg profile must not be empty",
        ));
    }
    let output_directory = resolve_output_path(root, output_directory.as_os_str());
    let assets_directory = resolve_output_path(root, assets_directory.as_os_str());
    let _build_lock = AndroidBuildLock::acquire(root, "external-playback-jni")?;
    let selected_abis = resolve_selected_abis(&[])?;
    let ffmpeg_root = android_ffmpeg_output_directory(root);
    let mut ffmpeg_request = android_ffmpeg_request(ffmpeg_profile, &selected_abis);
    ffmpeg_request.output_directory = Some(ffmpeg_root.clone());
    let profile =
        ffmpeg::resolve_profile_identity(root, &ffmpeg_request, ffmpeg::FfmpegPlatform::Android)
            .map_err(map_ffmpeg_error)?;
    if !skip_ffmpeg_runtime {
        let mut sink = io::sink();
        ffmpeg::run(root, &ffmpeg_request, &mut sink).map_err(map_ffmpeg_error)?;
    } else {
        require_regular_directory(&ffmpeg_root, "shared Android FFmpeg output")?;
    }

    let sdk_root = android_sdk_root()?;
    let ndk_version = android_ndk_version();
    let ndk_root = resolve_ndk_root(&sdk_root, &ndk_version)?;
    let cargo_ndk = require_path_command(
        "cargo-ndk",
        "cargo-ndk is required to build the Android external playback relay",
    )?;
    let cargo = require_path_command("cargo", "cargo is required to build Android plugins")?;
    require_rust_target(ANDROID_RUST_TARGET)?;

    let relay_stage = StagedGeneratedDirectory::new_external(
        &output_directory,
        ".vesper-android-relay-stage-",
        "Android external playback relay output",
    )?;
    fs::create_dir_all(&assets_directory).map_err(|error| {
        AndroidError::storage(format!(
            "failed to create Android external playback asset directory '{}': {error}",
            assets_directory.display()
        ))
    })?;
    let assets_stage = StagedGeneratedDirectory::new_nested(
        root,
        assets_directory.join("vesper-relay-ffmpeg"),
        ".vesper-android-relay-assets-stage-",
        "Android external playback relay metadata",
    )?;
    fs::write(
        assets_stage.path().join(FFMPEG_PROFILE_HASH),
        format!("{}\n", profile.hash),
    )
    .map_err(|error| {
        AndroidError::storage(format!(
            "failed to write Android external playback profile receipt: {error}"
        ))
    })?;

    for abi in &selected_abis {
        let ffmpeg_abi_directory = ffmpeg_root.join(abi);
        let pkgconfig_directory = ffmpeg_abi_directory.join("lib/pkgconfig");
        require_regular_directory(
            &pkgconfig_directory,
            &format!("shared FFmpeg runtime pkg-config directory for ABI {abi}"),
        )?;
        let configure_metadata = read_optional_ffmpeg_metadata(
            &ffmpeg_abi_directory.join("vesper-ffmpeg-build-metadata.txt"),
        )?;
        let mut command = Command::new(&cargo_ndk);
        command
            .current_dir(root)
            .args(["ndk", "-o"])
            .arg(relay_stage.path())
            .args(["-t", abi, "build", "-p", "player-relay-ffmpeg-android"]);
        if build_profile == "release" {
            command.arg("--release");
        }
        command
            .env("ANDROID_SDK_ROOT", &sdk_root)
            .env("ANDROID_NDK_ROOT", &ndk_root)
            .env("ANDROID_NDK_HOME", &ndk_root)
            .env("CARGO", &cargo)
            .env("PKG_CONFIG_ALLOW_CROSS", "1")
            .env("PKG_CONFIG_PATH", &pkgconfig_directory)
            .env("VESPER_FFMPEG_PROFILE_HASH", &profile.hash)
            .env("VESPER_FFMPEG_CONFIGURE_METADATA", configure_metadata)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        require_success(&mut command, "Android external playback relay build")?;
    }
    validate_plugin_output_tree(relay_stage.path(), "Android external playback relay")?;
    for abi in &selected_abis {
        require_regular_file(
            &relay_stage.path().join(abi).join(ANDROID_RELAY_LIBRARY),
            "Android external playback relay library",
        )?;
    }

    let cancellation =
        external_process::InterruptDeferral::start("Android external playback relay output")
            .map_err(|error| AndroidError::worker(error.to_string()))?;
    let promotion = promote_staged_directories(vec![relay_stage, assets_stage], &cancellation);
    let cancelled = cancellation.finish();
    match (promotion, cancelled) {
        (Ok(()), false) => {}
        (Ok(()), true) => {
            return Err(AndroidError::worker(
                "Android external playback relay output was cancelled after publication",
            ));
        }
        (Err(error), true) => {
            return Err(AndroidError::worker(format!(
                "Android external playback relay output was cancelled; {error}"
            )));
        }
        (Err(error), false) => return Err(error),
    }
    writeln!(
        output,
        "Built Android external playback relay JNI libraries into:"
    )
    .map_err(output_error)?;
    writeln!(output, "  {}", output_directory.display()).map_err(output_error)?;
    output.flush().map_err(output_error)
}

fn copy_android_abi_files(
    source: &Path,
    target: &Path,
    selected_abis: &[String],
    label: &str,
) -> Result<(), AndroidError> {
    for abi in selected_abis {
        let source_abi = source.join(abi);
        let metadata = fs::symlink_metadata(&source_abi).map_err(|error| {
            AndroidError::conformance(format!(
                "{label} is missing ABI directory '{}': {error}",
                source_abi.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(AndroidError::conformance(format!(
                "{label} ABI directory '{}' is not a regular non-symlink directory",
                source_abi.display()
            )));
        }
        let target_abi = target.join(abi);
        fs::create_dir_all(&target_abi).map_err(|error| {
            AndroidError::storage(format!(
                "failed to create {label} ABI directory '{}': {error}",
                target_abi.display()
            ))
        })?;
        let entries = fs::read_dir(&source_abi).map_err(|error| {
            AndroidError::storage(format!(
                "failed to enumerate {label} ABI directory '{}': {error}",
                source_abi.display()
            ))
        })?;
        let mut count = 0_usize;
        for entry in entries {
            count += 1;
            if count > MAX_ANDROID_PLUGIN_OUTPUT_ENTRIES {
                return Err(AndroidError::conformance(format!(
                    "{label} contains more than {MAX_ANDROID_PLUGIN_OUTPUT_ENTRIES} files"
                )));
            }
            let entry = entry.map_err(|error| {
                AndroidError::storage(format!("failed to read {label} entry: {error}"))
            })?;
            let source_file = entry.path();
            let metadata = fs::symlink_metadata(&source_file).map_err(|error| {
                AndroidError::storage(format!(
                    "failed to inspect {label} entry '{}': {error}",
                    source_file.display()
                ))
            })?;
            if !metadata.file_type().is_file() {
                return Err(AndroidError::conformance(format!(
                    "{label} contains a link, directory, or special file: {}",
                    source_file.display()
                )));
            }
            let name = source_file.file_name().ok_or_else(|| {
                AndroidError::conformance(format!("{label} contains an unnamed file"))
            })?;
            let destination = target_abi.join(name);
            match fs::symlink_metadata(&destination) {
                Ok(_) => {
                    return Err(AndroidError::conformance(format!(
                        "{label} would overwrite an existing ABI library: {}",
                        destination.display()
                    )));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(AndroidError::storage(format!(
                        "failed to inspect Android ABI destination '{}': {error}",
                        destination.display()
                    )));
                }
            }
            fs::copy(&source_file, &destination).map_err(|error| {
                AndroidError::storage(format!(
                    "failed to copy {label} entry '{}': {error}",
                    source_file.display()
                ))
            })?;
        }
        if count == 0 {
            return Err(AndroidError::conformance(format!(
                "{label} contains no files for ABI {abi}"
            )));
        }
    }
    Ok(())
}

fn stage_ffmpeg_runtime_libraries(
    output: &Path,
    ffmpeg_output: &Path,
    openssl_output: &Path,
    libxml2_output: &Path,
    use_openssl: bool,
    use_libxml2: bool,
    selected_abis: &[String],
) -> Result<(), AndroidError> {
    fs::create_dir_all(output).map_err(|error| {
        AndroidError::storage(format!(
            "failed to create Android FFmpeg runtime staging directory '{}': {error}",
            output.display()
        ))
    })?;
    for abi in selected_abis {
        let runtime_lib_dir = ffmpeg_output.join(abi).join("lib");
        require_regular_directory(
            &runtime_lib_dir,
            &format!("Android FFmpeg runtime library directory for ABI {abi}"),
        )?;
        let target = output.join(abi);
        fs::create_dir_all(&target).map_err(|error| {
            AndroidError::storage(format!(
                "failed to create Android FFmpeg runtime ABI directory '{}': {error}",
                target.display()
            ))
        })?;
        let mut staged = 0_usize;
        for entry in fs::read_dir(&runtime_lib_dir).map_err(|error| {
            AndroidError::storage(format!(
                "failed to enumerate Android FFmpeg runtime directory '{}': {error}",
                runtime_lib_dir.display()
            ))
        })? {
            let entry = entry.map_err(|error| {
                AndroidError::storage(format!(
                    "failed to read Android FFmpeg runtime entry: {error}"
                ))
            })?;
            let source = entry.path();
            let name = source
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or_default();
            let metadata = fs::symlink_metadata(&source).map_err(|error| {
                AndroidError::storage(format!(
                    "failed to inspect Android FFmpeg runtime entry '{}': {error}",
                    source.display()
                ))
            })?;
            if metadata.file_type().is_file() && name.starts_with("lib") && name.ends_with(".so") {
                fs::copy(&source, target.join(name)).map_err(|error| {
                    AndroidError::storage(format!(
                        "failed to stage Android FFmpeg runtime library '{}': {error}",
                        source.display()
                    ))
                })?;
                staged += 1;
            } else if !metadata.file_type().is_file() && !metadata.file_type().is_dir() {
                return Err(AndroidError::conformance(format!(
                    "Android FFmpeg runtime directory contains a link or special file: {}",
                    source.display()
                )));
            }
        }
        if use_openssl {
            copy_required_runtime_libraries(
                &openssl_output.join(abi).join("lib"),
                &target,
                &["libssl", "libcrypto"],
            )?;
        }
        if use_libxml2 {
            copy_required_runtime_libraries(
                &libxml2_output.join(abi).join("lib"),
                &target,
                &["libxml2"],
            )?;
        }
        if staged == 0 {
            return Err(AndroidError::conformance(format!(
                "No Android FFmpeg runtime libraries were found for ABI {abi}"
            )));
        }
    }
    Ok(())
}

fn ffmpeg_runtime_uses_external_dependency(
    ffmpeg_output: &Path,
    selected_abis: &[String],
    dependency: &str,
) -> Result<bool, AndroidError> {
    let mut expected = None;
    for abi in selected_abis {
        let metadata_path = ffmpeg_output
            .join(abi)
            .join("vesper-ffmpeg-build-metadata.txt");
        let metadata = read_optional_ffmpeg_metadata(&metadata_path)?;
        if metadata.is_empty() {
            return Err(AndroidError::compatibility(format!(
                "Missing Android FFmpeg build metadata for ABI {abi}: {}",
                metadata_path.display()
            )));
        }
        let dependencies = metadata
            .split(';')
            .find_map(|field| field.strip_prefix("external_dependencies="))
            .ok_or_else(|| {
                AndroidError::conformance(format!(
                    "Android FFmpeg build metadata does not declare external_dependencies: {}",
                    metadata_path.display()
                ))
            })?;
        let uses_dependency = dependencies
            .split(',')
            .map(str::trim)
            .any(|candidate| candidate == dependency);
        match expected {
            Some(expected) if expected != uses_dependency => {
                return Err(AndroidError::conformance(format!(
                    "Android FFmpeg external dependency `{dependency}` differs across selected ABIs"
                )));
            }
            None => expected = Some(uses_dependency),
            _ => {}
        }
    }
    Ok(expected.unwrap_or(false))
}

fn copy_required_runtime_libraries(
    source_directory: &Path,
    target_directory: &Path,
    prefixes: &[&str],
) -> Result<(), AndroidError> {
    require_regular_directory(
        source_directory,
        "declared Android runtime dependency directory",
    )?;
    let mut copied = vec![false; prefixes.len()];
    for entry in fs::read_dir(source_directory).map_err(|error| {
        AndroidError::storage(format!(
            "failed to enumerate declared Android runtime directory '{}': {error}",
            source_directory.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            AndroidError::storage(format!(
                "failed to read declared Android runtime entry: {error}"
            ))
        })?;
        let source = entry.path();
        let name = source
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or_default();
        let metadata = fs::symlink_metadata(&source).map_err(|error| {
            AndroidError::storage(format!(
                "failed to inspect declared Android runtime entry '{}': {error}",
                source.display()
            ))
        })?;
        if metadata.file_type().is_file() && name.ends_with(".so") {
            for (index, prefix) in prefixes.iter().enumerate() {
                if name.starts_with(prefix) {
                    fs::copy(&source, target_directory.join(name)).map_err(|error| {
                        AndroidError::storage(format!(
                            "failed to stage declared Android runtime library '{}': {error}",
                            source.display()
                        ))
                    })?;
                    copied[index] = true;
                    break;
                }
            }
        } else if !metadata.file_type().is_file() && !metadata.file_type().is_dir() {
            return Err(AndroidError::conformance(format!(
                "declared Android runtime directory contains a link or special file: {}",
                source.display()
            )));
        }
    }
    for (prefix, copied) in prefixes.iter().zip(copied) {
        if !copied {
            return Err(AndroidError::compatibility(format!(
                "Declared Android runtime dependency `{prefix}` has no shared library in {}",
                source_directory.display()
            )));
        }
    }
    Ok(())
}

fn sample_apks_transaction(
    root: &Path,
    output_directory: &Path,
    selected_abis: &[String],
    ffmpeg_release_lock: &AndroidFfmpegReleaseLock,
    cancellation: &external_process::InterruptDeferral,
) -> Result<Vec<String>, AndroidError> {
    let _build_lock = AndroidBuildLock::acquire(root, "sample-apks")?;
    let compose_project = require_contained_directory(
        root,
        &root.join("examples/android-compose-host"),
        "Android Compose sample project",
    )?;
    let flutter_project = require_contained_directory(
        root,
        &root.join("examples/flutter-host"),
        "Flutter sample project",
    )?;
    let flutter_android_project = require_contained_directory(
        root,
        &flutter_project.join("android"),
        "Flutter Android sample project",
    )?;
    let library_project = root.join("lib/android");
    let compose_gradle =
        gradle::resolve(&compose_project, Some(&library_project)).map_err(map_gradle_error)?;
    let flutter = resolve_path_command("flutter");
    let flutter_gradle = flutter
        .is_none()
        .then(|| gradle::resolve(&flutter_android_project, Some(&compose_project)))
        .transpose()
        .map_err(map_gradle_error)?;
    let compose_gradle_user_home = gradle::service_home(&compose_project);
    let flutter_gradle_user_home = gradle::service_home(&flutter_android_project);
    let current_cli = env::current_exe().map_err(|error| {
        AndroidError::storage(format!(
            "failed to resolve the current Vesper CLI for Android sample APKs: {error}"
        ))
    })?;
    require_executable_file(&current_cli, "Vesper CLI for Android sample APKs")?;
    let stage = StagedGeneratedDirectory::new_external(
        output_directory,
        ".vesper-android-samples-stage-",
        "Android sample APK output",
    )?;
    let optional_build = build_optional_android_plugins(
        root,
        &library_project,
        selected_abis,
        Some(ffmpeg_release_lock),
        cancellation,
    )?;
    validate_optional_profile_receipts(&optional_build)?;
    let selected_abis_csv = selected_abis.join(",");
    let mut artifacts = Vec::with_capacity(selected_abis.len() * 2);

    for abi in selected_abis {
        let mut compose = Command::new(&compose_gradle);
        compose
            .current_dir(root)
            .arg("-p")
            .arg(&compose_project)
            .arg(format!("-Pvesper.player.android.abis={abi}"))
            .arg(":app:assembleRelease")
            .env("GRADLE_USER_HOME", &compose_gradle_user_home)
            .env("VESPER_CLI", &current_cli)
            .env(PARENT_SUPERVISES_PROCESS_GROUP_ENV, "1")
            .env("RUST_ANDROID_ABIS", &selected_abis_csv)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        configure_optional_android_build_environment(
            &mut compose,
            &optional_build,
            Some(ffmpeg_release_lock),
        );
        require_success_in_deferral(
            &mut compose,
            "Android Compose sample APK build",
            cancellation,
        )?;
        let compose_apk = discover_single_apk(
            &compose_project.join("app/build/outputs/apk/release"),
            "Android Compose sample APK",
        )?;
        validate_android_sample_apk(&compose_apk, abi, false)?;
        let compose_name = format!("VesperPlayerAndroidComposeHost-android-{abi}-debug-signed.apk");
        copy_sample_apk(&compose_apk, &stage.path().join(&compose_name))?;
        artifacts.push(compose_name);

        let flutter_apk = if let Some(flutter) = flutter.as_ref() {
            let mut pub_get = Command::new(flutter);
            pub_get
                .current_dir(&flutter_project)
                .arg("pub")
                .arg("get")
                .env("GRADLE_USER_HOME", &flutter_gradle_user_home)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            require_success_in_deferral(
                &mut pub_get,
                "Flutter sample dependency resolution",
                cancellation,
            )?;
            let mut build = Command::new(flutter);
            build
                .current_dir(&flutter_project)
                .args([
                    OsString::from("build"),
                    OsString::from("apk"),
                    OsString::from("--release"),
                    OsString::from("--target-platform"),
                    OsString::from("android-arm64"),
                    OsString::from("--split-per-abi"),
                ])
                .env("GRADLE_USER_HOME", &flutter_gradle_user_home)
                .env("VESPER_CLI", &current_cli)
                .env(PARENT_SUPERVISES_PROCESS_GROUP_ENV, "1")
                .env("RUST_ANDROID_ABIS", &selected_abis_csv)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            configure_optional_android_build_environment(
                &mut build,
                &optional_build,
                Some(ffmpeg_release_lock),
            );
            require_success_in_deferral(&mut build, "Flutter sample APK build", cancellation)?;
            flutter_project.join("build/app/outputs/flutter-apk/app-arm64-v8a-release.apk")
        } else {
            let gradle = flutter_gradle.as_ref().ok_or_else(|| {
                AndroidError::compatibility(
                    "Flutter is unavailable and no cached Gradle distribution was found for the Flutter Android sample project",
                )
            })?;
            let mut build = Command::new(gradle);
            build
                .current_dir(root)
                .arg("-p")
                .arg(&flutter_android_project)
                .arg(format!("-Pvesper.player.android.abis={abi}"))
                .arg(format!("-Pvesper.player.android.app.abis={abi}"))
                .arg(":app:assembleRelease")
                .env("GRADLE_USER_HOME", &flutter_gradle_user_home)
                .env("VESPER_CLI", &current_cli)
                .env(PARENT_SUPERVISES_PROCESS_GROUP_ENV, "1")
                .env("RUST_ANDROID_ABIS", &selected_abis_csv)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            configure_optional_android_build_environment(
                &mut build,
                &optional_build,
                Some(ffmpeg_release_lock),
            );
            require_success_in_deferral(
                &mut build,
                "Flutter Android Gradle sample APK build",
                cancellation,
            )?;
            flutter_project.join("build/app/outputs/flutter-apk/app-arm64-v8a-release.apk")
        };
        require_regular_file(&flutter_apk, "Flutter sample APK")?;
        validate_android_sample_apk(&flutter_apk, abi, true)?;
        let flutter_name = format!("VesperPlayerFlutterHost-android-{abi}-debug-signed.apk");
        copy_sample_apk(&flutter_apk, &stage.path().join(&flutter_name))?;
        artifacts.push(flutter_name);
    }

    stage.validate()?;
    promote_staged_directories(vec![stage], cancellation)?;
    Ok(artifacts)
}

fn discover_single_apk(directory: &Path, label: &str) -> Result<PathBuf, AndroidError> {
    let metadata = fs::symlink_metadata(directory).map_err(|error| {
        AndroidError::conformance(format!(
            "{label} output directory '{}' is unavailable: {error}",
            directory.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(AndroidError::conformance(format!(
            "{label} output path '{}' is not a regular non-symlink directory",
            directory.display()
        )));
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| {
        AndroidError::storage(format!(
            "failed to inspect {label} output directory '{}': {error}",
            directory.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            AndroidError::storage(format!("failed to read {label} output entry: {error}"))
        })?;
        let path = entry.path();
        if path.extension().and_then(OsStr::to_str) != Some("apk") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            AndroidError::storage(format!(
                "failed to inspect {label} candidate '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() {
            return Err(AndroidError::conformance(format!(
                "{label} candidate '{}' is not a regular non-symlink file",
                path.display()
            )));
        }
        candidates.push(path);
        if candidates.len() > 32 {
            return Err(AndroidError::conformance(format!(
                "{label} output directory '{}' contains too many APK candidates",
                directory.display()
            )));
        }
    }
    candidates.sort();
    match candidates.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(AndroidError::conformance(format!(
            "expected exactly one {label}, found none in '{}'",
            directory.display()
        ))),
        _ => Err(AndroidError::conformance(format!(
            "expected exactly one {label}, found {} in '{}'",
            candidates.len(),
            directory.display()
        ))),
    }
}

fn copy_sample_apk(source: &Path, target: &Path) -> Result<(), AndroidError> {
    require_regular_file(source, "Android sample APK")?;
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        AndroidError::storage(format!(
            "failed to inspect Android sample APK '{}': {error}",
            source.display()
        ))
    })?;
    if metadata.len() > MAX_ANDROID_SAMPLE_APK_BYTES {
        return Err(AndroidError::conformance(format!(
            "Android sample APK '{}' exceeds {MAX_ANDROID_SAMPLE_APK_BYTES} bytes",
            source.display()
        )));
    }
    fs::copy(source, target).map_err(|error| {
        AndroidError::storage(format!(
            "failed to stage Android sample APK '{}' as '{}': {error}",
            source.display(),
            target.display()
        ))
    })?;
    require_regular_file(target, "staged Android sample APK")
}

pub(crate) fn validate_android_sample_apk(
    path: &Path,
    expected_abi: &str,
    require_flutter_aot: bool,
) -> Result<(), AndroidError> {
    let index = scan_android_sample_apk(path, expected_abi)?;
    for library in [
        ANDROID_JNI_LIBRARY,
        ANDROID_SOURCE_NORMALIZER_LIBRARY,
        ANDROID_REMUX_LIBRARY,
        ANDROID_FRAME_PROCESSOR_LIBRARY,
        ANDROID_RELAY_LIBRARY,
        "libavcodec.so",
        "libavformat.so",
        "libavutil.so",
    ] {
        let entry = format!("lib/{expected_abi}/{library}");
        if !index.files.contains(&entry) {
            return Err(AndroidError::conformance(format!(
                "Android sample APK '{}' is missing required native library {entry}",
                path.display()
            )));
        }
        let binary = read_android_archive_entry(path, &entry, MAX_ANDROID_SAMPLE_APK_ENTRY_BYTES)?;
        if !binary.starts_with(b"\x7fELF") {
            return Err(AndroidError::conformance(format!(
                "Android sample APK '{}' contains a non-ELF native library {entry}",
                path.display()
            )));
        }
    }
    validate_android_sample_registry_entries(
        path,
        expected_abi,
        require_flutter_aot,
        &index.registry_entries,
    )?;
    let receipts = [
        (
            "SourceNormalizer",
            "assets/vesper-source-normalizer-ffmpeg/profile-hash.txt",
        ),
        ("remux", "assets/vesper-remux-ffmpeg/profile-hash.txt"),
        (
            "FFmpeg runtime",
            "assets/vesper-ffmpeg-runtime/profile-hash.txt",
        ),
        (
            "external playback relay",
            "assets/vesper-relay-ffmpeg/profile-hash.txt",
        ),
    ]
    .map(|(label, entry)| read_archive_profile_receipt(path, entry, label))
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    if !receipts.windows(2).all(|pair| pair[0] == pair[1]) {
        return Err(AndroidError::conformance(format!(
            "Android sample APK '{}' contains mismatched optional FFmpeg profile receipts: SourceNormalizer={}, remux={}, runtime={}, relay={}",
            path.display(),
            receipts[0],
            receipts[1],
            receipts[2],
            receipts[3],
        )));
    }
    if require_flutter_aot {
        let entry = format!("lib/{expected_abi}/libapp.so");
        if !index.files.contains(&entry) {
            return Err(AndroidError::conformance(format!(
                "Flutter Android sample APK '{}' is missing required {entry}",
                path.display()
            )));
        }
        let binary = read_android_archive_entry(path, &entry, MAX_ANDROID_SAMPLE_APK_ENTRY_BYTES)?;
        const MARKERS: [&[u8]; 6] = [
            b"assets/subtitle_contract",
            b"fixtures/contracts",
            b"fixtures/media",
            b"tiny-aac.m4a",
            b"tiny-h264-aac.m4v",
            b"tiny-h264-aac-mediacodec.m4v",
        ];
        if MARKERS
            .iter()
            .any(|marker| contains_ascii_case_insensitive(&binary, marker))
        {
            return Err(AndroidError::conformance(format!(
                "Android sample Flutter AOT binary contains test fixture markers: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn validate_android_sample_registry_entries(
    path: &Path,
    expected_abi: &str,
    require_flutter_aot: bool,
    actual: &BTreeSet<String>,
) -> Result<(), AndroidError> {
    let mut plugin_ids = vec![
        "io.github.umbrella22.vesper.remux-ffmpeg",
        "io.github.umbrella22.vesper.source-normalizer-ffmpeg",
        "dev.vesper.frame-processor-diagnostic",
    ];
    if !require_flutter_aot {
        plugin_ids.push("io.github.umbrella22.vesper.decoder-mediacodec");
    }
    let expected = plugin_ids
        .into_iter()
        .map(|plugin_id| format!("assets/vesper/plugins/{expected_abi}/{plugin_id}.json"))
        .collect::<BTreeSet<_>>();
    if actual == &expected {
        return Ok(());
    }

    let missing = expected.difference(actual).cloned().collect::<Vec<_>>();
    let unexpected = actual.difference(&expected).cloned().collect::<Vec<_>>();
    Err(AndroidError::conformance(format!(
        "Android sample APK '{}' plugin registries do not match the expected set: missing=[{}], unexpected=[{}]",
        path.display(),
        missing.join(", "),
        unexpected.join(", "),
    )))
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.windows(needle.len()).any(|window| {
            window
                .iter()
                .zip(needle)
                .all(|(left, right)| left.eq_ignore_ascii_case(right))
        })
}

fn scan_android_sample_apk(
    path: &Path,
    expected_abi: &str,
) -> Result<AndroidArchiveIndex, AndroidError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AndroidError::storage(format!(
            "failed to inspect Android sample APK '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_ANDROID_SAMPLE_APK_BYTES {
        return Err(AndroidError::conformance(format!(
            "Android sample APK '{}' is not a bounded regular non-symlink file",
            path.display()
        )));
    }
    let file = File::open(path).map_err(|error| {
        AndroidError::storage(format!(
            "failed to open Android sample APK '{}': {error}",
            path.display()
        ))
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        AndroidError::conformance(format!(
            "invalid Android sample APK '{}': {error}",
            path.display()
        ))
    })?;
    if archive.is_empty() || archive.len() > MAX_ANDROID_SAMPLE_APK_ENTRIES {
        return Err(AndroidError::conformance(format!(
            "Android sample APK '{}' must contain 1 to {MAX_ANDROID_SAMPLE_APK_ENTRIES} entries, found {}",
            path.display(),
            archive.len()
        )));
    }
    let mut nodes = BTreeMap::new();
    let mut files = BTreeSet::new();
    let mut runtime_libraries = BTreeSet::new();
    let mut registry_entries = BTreeSet::new();
    let mut expanded_bytes = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            AndroidError::conformance(format!(
                "failed to inspect entry {index} in Android sample APK '{}': {error}",
                path.display()
            ))
        })?;
        let name = std::str::from_utf8(entry.name_raw()).map_err(|error| {
            AndroidError::conformance(format!(
                "Android sample APK '{}' contains a non-UTF-8 path: {error}",
                path.display()
            ))
        })?;
        let kind = android_archive_entry_kind(&entry, name, path)?;
        let canonical = validate_android_sample_apk_path(name, kind, path)?;
        insert_android_sample_apk_node(&mut nodes, &canonical, kind, path)?;
        if entry.encrypted() {
            return Err(AndroidError::conformance(format!(
                "Android sample APK '{}' contains encrypted entry '{name}'",
                path.display()
            )));
        }
        if entry.size() > MAX_ANDROID_SAMPLE_APK_ENTRY_BYTES {
            return Err(AndroidError::conformance(format!(
                "Android sample APK entry '{name}' exceeds {MAX_ANDROID_SAMPLE_APK_ENTRY_BYTES} bytes"
            )));
        }
        expanded_bytes = expanded_bytes.checked_add(entry.size()).ok_or_else(|| {
            AndroidError::conformance("Android sample APK expanded size overflowed")
        })?;
        if expanded_bytes > MAX_ANDROID_SAMPLE_APK_EXPANDED_BYTES {
            return Err(AndroidError::conformance(format!(
                "Android sample APK '{}' expands beyond {MAX_ANDROID_SAMPLE_APK_EXPANDED_BYTES} bytes",
                path.display()
            )));
        }
        if kind == AndroidArchiveNodeKind::File
            && entry.size() > ANDROID_SAMPLE_APK_COMPRESSION_RATIO_FLOOR
            && (entry.compressed_size() == 0
                || entry
                    .compressed_size()
                    .saturating_mul(MAX_ANDROID_SAMPLE_APK_COMPRESSION_RATIO)
                    < entry.size())
        {
            return Err(AndroidError::conformance(format!(
                "Android sample APK entry '{name}' exceeds the {MAX_ANDROID_SAMPLE_APK_COMPRESSION_RATIO}:1 compression ratio limit"
            )));
        }
        if kind == AndroidArchiveNodeKind::Directory {
            continue;
        }
        if is_android_test_fixture_path(&canonical) {
            return Err(AndroidError::conformance(format!(
                "Android sample APK '{}' contains test fixture resource '{canonical}'",
                path.display()
            )));
        }
        if let Some(abi) = canonical
            .strip_prefix("lib/")
            .and_then(|value| value.split('/').next())
            && abi != expected_abi
        {
            return Err(AndroidError::conformance(format!(
                "Android sample APK '{}' contains unexpected native ABI '{abi}' in '{canonical}'",
                path.display()
            )));
        }
        if android_archive_is_runtime_library(&canonical) {
            runtime_libraries.insert(canonical.clone());
        }
        if canonical.starts_with("assets/vesper/plugins/") {
            registry_entries.insert(canonical.clone());
        }
        files.insert(canonical);
    }
    Ok(AndroidArchiveIndex {
        files,
        runtime_libraries,
        registry_entries,
    })
}

fn validate_android_sample_apk_path(
    path: &str,
    kind: AndroidArchiveNodeKind,
    archive: &Path,
) -> Result<String, AndroidError> {
    if path.is_empty()
        || path.len() > MAX_ANDROID_SAMPLE_APK_PATH_BYTES
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.chars().any(char::is_control)
    {
        return Err(AndroidError::conformance(format!(
            "Android sample APK '{}' contains invalid path {path:?}",
            archive.display()
        )));
    }
    let canonical = if kind == AndroidArchiveNodeKind::Directory {
        path.strip_suffix('/').unwrap_or(path)
    } else {
        if path.ends_with('/') {
            return Err(AndroidError::conformance(format!(
                "Android sample APK '{}' has a file path ending in '/': {path}",
                archive.display()
            )));
        }
        path
    };
    let components = canonical.split('/').collect::<Vec<_>>();
    if canonical.is_empty()
        || components.len() > MAX_ANDROID_RELEASE_PATH_DEPTH
        || components
            .iter()
            .any(|component| component.is_empty() || matches!(*component, "." | ".."))
    {
        return Err(AndroidError::conformance(format!(
            "Android sample APK '{}' contains traversing, empty, or over-depth path '{path}'",
            archive.display()
        )));
    }
    Ok(canonical.to_owned())
}

fn stage_release_transaction(
    root: &Path,
    output_directory: &Path,
    selected_abis: &[String],
    include_optional: bool,
    ffmpeg_release_lock: Option<&AndroidFfmpegReleaseLock>,
    cancellation: &external_process::InterruptDeferral,
) -> Result<Vec<String>, AndroidError> {
    let _aar_build_lock = AndroidBuildLock::acquire(root, "aar")?;
    let _jni_build_lock = AndroidBuildLock::acquire(root, "jni")?;
    let release_stage = StagedGeneratedDirectory::new_external(
        output_directory,
        ".vesper-android-release-stage-",
        "Android release output",
    )?;
    let mut stages = build_aar_stages(
        root,
        "assembleRelease",
        include_optional,
        selected_abis,
        ffmpeg_release_lock,
        None,
        cancellation,
    )?;
    let artifacts = android_release_artifacts(root, &selected_abis[0], include_optional);
    for artifact in &artifacts {
        copy_release_artifact(
            &artifact.source,
            &release_stage.path().join(&artifact.file_name),
        )?;
    }
    verify_android_release_artifacts(
        root,
        release_stage.path(),
        &artifacts,
        &selected_abis[0],
        ffmpeg_release_lock,
    )?;
    release_stage.validate()?;
    stages.push(release_stage);
    promote_staged_directories(stages, cancellation)?;
    Ok(artifacts
        .into_iter()
        .map(|artifact| artifact.file_name)
        .collect())
}

#[derive(Clone, Copy)]
enum AndroidReleaseArtifactKind {
    Core {
        requires_host_jni: bool,
    },
    FfmpegRuntime,
    Relay,
    Plugin {
        plugin_id: &'static str,
        library_name: &'static str,
        manifest: &'static str,
        profile_receipt: Option<(&'static str, &'static str, &'static str)>,
    },
}

struct AndroidReleaseArtifact {
    source: PathBuf,
    file_name: String,
    kind: AndroidReleaseArtifactKind,
}

fn android_release_artifacts(
    root: &Path,
    abi: &str,
    include_optional: bool,
) -> Vec<AndroidReleaseArtifact> {
    let project = root.join("lib/android");
    let mut artifacts = vec![
        AndroidReleaseArtifact {
            source: project.join(
                "vesper-player-kit/build/outputs/aar/vesper-player-kit-release.aar",
            ),
            file_name: format!("VesperPlayerKit-android-{abi}.aar"),
            kind: AndroidReleaseArtifactKind::Core {
                requires_host_jni: true,
            },
        },
        AndroidReleaseArtifact {
            source: project.join(
                "vesper-player-kit-compose/build/outputs/aar/vesper-player-kit-compose-release.aar",
            ),
            file_name: format!("VesperPlayerKitCompose-android-{abi}.aar"),
            kind: AndroidReleaseArtifactKind::Core {
                requires_host_jni: false,
            },
        },
        AndroidReleaseArtifact {
            source: project.join(
                "vesper-player-kit-compose-ui/build/outputs/aar/vesper-player-kit-compose-ui-release.aar",
            ),
            file_name: format!("VesperPlayerKitComposeUi-android-{abi}.aar"),
            kind: AndroidReleaseArtifactKind::Core {
                requires_host_jni: false,
            },
        },
    ];
    if include_optional {
        artifacts.extend([
            AndroidReleaseArtifact {
                source: project.join(
                    "vesper-player-kit-external-playback/build/outputs/aar/vesper-player-kit-external-playback-release.aar",
                ),
                file_name: format!("VesperPlayerKitExternalPlayback-android-{abi}.aar"),
                kind: AndroidReleaseArtifactKind::Relay,
            },
            AndroidReleaseArtifact {
                source: project.join(
                    "vesper-player-kit-ffmpeg-runtime/build/outputs/aar/vesper-player-kit-ffmpeg-runtime-release.aar",
                ),
                file_name: format!("VesperPlayerKitFfmpegRuntime-android-{abi}.aar"),
                kind: AndroidReleaseArtifactKind::FfmpegRuntime,
            },
            AndroidReleaseArtifact {
                source: project.join(
                    "vesper-player-kit-decoder-mediacodec/build/outputs/aar/vesper-player-kit-decoder-mediacodec-release.aar",
                ),
                file_name: format!("VesperPlayerKitDecoderMediaCodec-android-{abi}.aar"),
                kind: AndroidReleaseArtifactKind::Plugin {
                    plugin_id: "io.github.umbrella22.vesper.decoder-mediacodec",
                    library_name: "vesper_decoder_mediacodec",
                    manifest: "plugins/decoder-mediacodec/vesper-plugin.toml",
                    profile_receipt: None,
                },
            },
            AndroidReleaseArtifact {
                source: project.join(
                    "vesper-player-kit-source-normalizer-ffmpeg/build/outputs/aar/vesper-player-kit-source-normalizer-ffmpeg-release.aar",
                ),
                file_name: format!("VesperPlayerKitSourceNormalizerFfmpeg-android-{abi}.aar"),
                kind: AndroidReleaseArtifactKind::Plugin {
                    plugin_id: "io.github.umbrella22.vesper.source-normalizer-ffmpeg",
                    library_name: "vesper_source_normalizer_ffmpeg",
                    manifest: "plugins/source-normalizer-ffmpeg/vesper-plugin.toml",
                    profile_receipt: Some((
                        "source-normalizer",
                        "SourceNormalizer",
                        "assets/vesper-source-normalizer-ffmpeg/profile-hash.txt",
                    )),
                },
            },
            AndroidReleaseArtifact {
                source: project.join(
                    "vesper-player-kit-remux-ffmpeg/build/outputs/aar/vesper-player-kit-remux-ffmpeg-release.aar",
                ),
                file_name: format!("VesperPlayerKitRemuxFfmpeg-android-{abi}.aar"),
                kind: AndroidReleaseArtifactKind::Plugin {
                    plugin_id: "io.github.umbrella22.vesper.remux-ffmpeg",
                    library_name: "vesper_remux_ffmpeg",
                    manifest: "plugins/remux-ffmpeg/vesper-plugin.toml",
                    profile_receipt: Some((
                        "remux",
                        "remux",
                        "assets/vesper-remux-ffmpeg/profile-hash.txt",
                    )),
                },
            },
            AndroidReleaseArtifact {
                source: project.join(
                    "vesper-player-kit-frame-processor-diagnostic/build/outputs/aar/vesper-player-kit-frame-processor-diagnostic-release.aar",
                ),
                file_name: format!("VesperPlayerKitFrameProcessorDiagnostic-android-{abi}.aar"),
                kind: AndroidReleaseArtifactKind::Plugin {
                    plugin_id: "dev.vesper.frame-processor-diagnostic",
                    library_name: "vesper_frame_processor_diagnostic",
                    manifest: "plugins/frame-processor-diagnostic/vesper-plugin.toml",
                    profile_receipt: None,
                },
            },
            AndroidReleaseArtifact {
                source: project.join(
                    "vesper-player-kit-performance-diagnostics/build/outputs/aar/vesper-player-kit-performance-diagnostics-release.aar",
                ),
                file_name: format!(
                    "VesperPlayerKitPerformanceDiagnostics-android-{abi}.aar"
                ),
                kind: AndroidReleaseArtifactKind::Plugin {
                    plugin_id: "io.github.umbrella22.vesper.performance-diagnostics",
                    library_name: "vesper_performance_diagnostics",
                    manifest: "plugins/performance-diagnostics/vesper-plugin.toml",
                    profile_receipt: None,
                },
            },
        ]);
    }
    artifacts
}

fn copy_release_artifact(source: &Path, target: &Path) -> Result<(), AndroidError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        AndroidError::conformance(format!(
            "Android release build did not produce '{}': {error}",
            source.display()
        ))
    })?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_ANDROID_RELEASE_ARCHIVE_BYTES {
        return Err(AndroidError::conformance(format!(
            "Android release artifact '{}' is not a bounded regular non-symlink file",
            source.display()
        )));
    }
    fs::copy(source, target).map_err(|error| {
        AndroidError::storage(format!(
            "failed to stage Android release artifact '{}' as '{}': {error}",
            source.display(),
            target.display()
        ))
    })?;
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AndroidArchiveNodeKind {
    Directory,
    File,
}

#[derive(Debug)]
struct AndroidArchiveIndex {
    files: BTreeSet<String>,
    runtime_libraries: BTreeSet<String>,
    registry_entries: BTreeSet<String>,
}

fn verify_android_release_artifacts(
    root: &Path,
    staged_directory: &Path,
    artifacts: &[AndroidReleaseArtifact],
    abi: &str,
    ffmpeg_release_lock: Option<&AndroidFfmpegReleaseLock>,
) -> Result<(), AndroidError> {
    let mut profile_receipts = BTreeMap::new();
    let mut runtime_carriers = 0_usize;
    for artifact in artifacts {
        let path = staged_directory.join(&artifact.file_name);
        let index = scan_android_release_archive(&path, abi)?;
        let carries_runtime = !index.runtime_libraries.is_empty();
        match artifact.kind {
            AndroidReleaseArtifactKind::Core { requires_host_jni } => {
                if carries_runtime {
                    return Err(AndroidError::conformance(format!(
                        "core Android AAR must not carry FFmpeg runtime libraries: {}",
                        path.display()
                    )));
                }
                if !index.registry_entries.is_empty() {
                    return Err(AndroidError::conformance(format!(
                        "core Android AAR must not carry plugin registry fragments: {}",
                        path.display()
                    )));
                }
                if requires_host_jni {
                    let entry = format!("jni/{abi}/{ANDROID_JNI_LIBRARY}");
                    if !index.files.contains(&entry) {
                        return Err(AndroidError::conformance(format!(
                            "Android host AAR is missing required JNI library {entry}: {}",
                            path.display()
                        )));
                    }
                }
            }
            AndroidReleaseArtifactKind::FfmpegRuntime => {
                runtime_carriers += usize::from(carries_runtime);
                for library in ["libavcodec.so", "libavformat.so", "libavutil.so"] {
                    let entry = format!("jni/{abi}/{library}");
                    if !index.files.contains(&entry) {
                        return Err(AndroidError::conformance(format!(
                            "Android FFmpeg runtime AAR is missing {entry}: {}",
                            path.display()
                        )));
                    }
                }
                let receipt = "assets/vesper-ffmpeg-runtime/profile-hash.txt";
                profile_receipts.insert(
                    "runtime",
                    read_archive_profile_receipt(&path, receipt, "FFmpeg runtime")?,
                );
                if let Some(lock) = ffmpeg_release_lock {
                    verify_ffmpeg_runtime_release_metadata(&path, abi, lock)?;
                }
            }
            AndroidReleaseArtifactKind::Relay => {
                reject_unexpected_runtime(&path, &index)?;
                let library = format!("jni/{abi}/{ANDROID_RELAY_LIBRARY}");
                if !index.files.contains(&library) {
                    return Err(AndroidError::conformance(format!(
                        "Android external playback AAR is missing {library}: {}",
                        path.display()
                    )));
                }
                let receipt = "assets/vesper-relay-ffmpeg/profile-hash.txt";
                profile_receipts.insert(
                    "relay",
                    read_archive_profile_receipt(&path, receipt, "external playback relay")?,
                );
            }
            AndroidReleaseArtifactKind::Plugin {
                plugin_id,
                library_name,
                manifest,
                profile_receipt,
            } => {
                reject_unexpected_runtime(&path, &index)?;
                verify_android_plugin_registry(
                    root,
                    &path,
                    &index,
                    abi,
                    plugin_id,
                    library_name,
                    manifest,
                )?;
                if let Some((receipt_key, receipt_label, receipt)) = profile_receipt {
                    profile_receipts.insert(
                        receipt_key,
                        read_archive_profile_receipt(&path, receipt, receipt_label)?,
                    );
                }
            }
        }
    }
    if artifacts.len() > 3 {
        if runtime_carriers != 1 {
            return Err(AndroidError::conformance(format!(
                "optional Android release must contain exactly one FFmpeg runtime carrier, found {runtime_carriers}"
            )));
        }
        let source = profile_receipts.get("source-normalizer");
        let remux = profile_receipts.get("remux");
        let runtime = profile_receipts.get("runtime");
        let relay = profile_receipts.get("relay");
        if source.is_none() || source != remux || remux != runtime || runtime != relay {
            return Err(AndroidError::conformance(format!(
                "optional Android release FFmpeg profile receipts do not match: SourceNormalizer={}, remux={}, runtime={}, relay={}",
                source.map_or("<missing>", String::as_str),
                remux.map_or("<missing>", String::as_str),
                runtime.map_or("<missing>", String::as_str),
                relay.map_or("<missing>", String::as_str),
            )));
        }
    }
    Ok(())
}

fn reject_unexpected_runtime(path: &Path, index: &AndroidArchiveIndex) -> Result<(), AndroidError> {
    if let Some(entry) = index.runtime_libraries.first() {
        Err(AndroidError::conformance(format!(
            "Android feature AAR must not bundle FFmpeg runtime library '{entry}': {}",
            path.display()
        )))
    } else {
        Ok(())
    }
}

fn scan_android_release_archive(
    path: &Path,
    expected_abi: &str,
) -> Result<AndroidArchiveIndex, AndroidError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AndroidError::storage(format!(
            "failed to inspect Android release archive '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_ANDROID_RELEASE_ARCHIVE_BYTES {
        return Err(AndroidError::conformance(format!(
            "Android release archive '{}' is not a bounded regular non-symlink file",
            path.display()
        )));
    }
    let file = File::open(path).map_err(|error| {
        AndroidError::storage(format!(
            "failed to open Android release archive '{}': {error}",
            path.display()
        ))
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        AndroidError::conformance(format!(
            "invalid Android release archive '{}': {error}",
            path.display()
        ))
    })?;
    if archive.is_empty() || archive.len() > MAX_ANDROID_RELEASE_ARCHIVE_ENTRIES {
        return Err(AndroidError::conformance(format!(
            "Android release archive '{}' must contain 1 to {MAX_ANDROID_RELEASE_ARCHIVE_ENTRIES} entries, found {}",
            path.display(),
            archive.len()
        )));
    }
    let mut nodes = BTreeMap::new();
    let mut files = BTreeSet::new();
    let mut runtime_libraries = BTreeSet::new();
    let mut registry_entries = BTreeSet::new();
    let mut expanded_bytes = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            AndroidError::conformance(format!(
                "failed to inspect entry {index} in '{}': {error}",
                path.display()
            ))
        })?;
        let raw_name = entry.name_raw();
        let name = std::str::from_utf8(raw_name).map_err(|error| {
            AndroidError::conformance(format!(
                "Android release archive '{}' contains a non-UTF-8 path: {error}",
                path.display()
            ))
        })?;
        let kind = android_archive_entry_kind(&entry, name, path)?;
        let canonical = validate_android_archive_path(name, kind, path)?;
        insert_android_archive_node(&mut nodes, &canonical, kind, path)?;
        if entry.encrypted() {
            return Err(AndroidError::conformance(format!(
                "Android release archive '{}' contains encrypted entry '{name}'",
                path.display()
            )));
        }
        if entry.size() > MAX_ANDROID_RELEASE_ENTRY_BYTES {
            return Err(AndroidError::conformance(format!(
                "Android release archive entry '{name}' exceeds {MAX_ANDROID_RELEASE_ENTRY_BYTES} bytes"
            )));
        }
        expanded_bytes = expanded_bytes.checked_add(entry.size()).ok_or_else(|| {
            AndroidError::conformance("Android release archive expanded size overflowed")
        })?;
        if expanded_bytes > MAX_ANDROID_RELEASE_EXPANDED_BYTES {
            return Err(AndroidError::conformance(format!(
                "Android release archive '{}' expands beyond {MAX_ANDROID_RELEASE_EXPANDED_BYTES} bytes",
                path.display()
            )));
        }
        if kind == AndroidArchiveNodeKind::File
            && entry.size() > ANDROID_RELEASE_COMPRESSION_RATIO_FLOOR
            && (entry.compressed_size() == 0
                || entry
                    .compressed_size()
                    .saturating_mul(MAX_ANDROID_RELEASE_COMPRESSION_RATIO)
                    < entry.size())
        {
            return Err(AndroidError::conformance(format!(
                "Android release archive entry '{name}' exceeds the {MAX_ANDROID_RELEASE_COMPRESSION_RATIO}:1 compression ratio limit"
            )));
        }
        if kind == AndroidArchiveNodeKind::Directory {
            continue;
        }
        if is_android_test_fixture_path(&canonical) {
            return Err(AndroidError::conformance(format!(
                "Android release archive '{}' contains test fixture resource '{canonical}'",
                path.display()
            )));
        }
        if let Some(abi) = android_archive_jni_abi(&canonical)
            && abi != expected_abi
        {
            return Err(AndroidError::conformance(format!(
                "Android release archive '{}' contains unexpected JNI ABI '{abi}' in '{canonical}'",
                path.display()
            )));
        }
        if android_archive_is_runtime_library(&canonical) {
            runtime_libraries.insert(canonical.clone());
        }
        if canonical.starts_with("assets/vesper/plugins/") {
            registry_entries.insert(canonical.clone());
        }
        files.insert(canonical);
    }
    Ok(AndroidArchiveIndex {
        files,
        runtime_libraries,
        registry_entries,
    })
}

fn android_archive_entry_kind<R: Read>(
    entry: &zip::read::ZipFile<'_, R>,
    name: &str,
    path: &Path,
) -> Result<AndroidArchiveNodeKind, AndroidError> {
    const TYPE_MASK: u32 = 0o170000;
    const DIRECTORY: u32 = 0o040000;
    const REGULAR: u32 = 0o100000;
    let file_type = entry.unix_mode().map(|mode| mode & TYPE_MASK).unwrap_or(0);
    if entry.is_dir() && matches!(file_type, 0 | DIRECTORY) {
        Ok(AndroidArchiveNodeKind::Directory)
    } else if entry.is_file() && matches!(file_type, 0 | REGULAR) {
        Ok(AndroidArchiveNodeKind::File)
    } else {
        Err(AndroidError::conformance(format!(
            "Android release archive '{}' contains a symlink or unsupported entry '{name}'",
            path.display()
        )))
    }
}

fn validate_android_archive_path(
    path: &str,
    kind: AndroidArchiveNodeKind,
    archive: &Path,
) -> Result<String, AndroidError> {
    if path.is_empty()
        || path.len() > MAX_ANDROID_RELEASE_PATH_BYTES
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.chars().any(char::is_control)
    {
        return Err(AndroidError::conformance(format!(
            "Android release archive '{}' contains invalid path {path:?}",
            archive.display()
        )));
    }
    let canonical = if kind == AndroidArchiveNodeKind::Directory {
        path.strip_suffix('/').unwrap_or(path)
    } else {
        if path.ends_with('/') {
            return Err(AndroidError::conformance(format!(
                "Android release archive '{}' has a file path ending in '/': {path}",
                archive.display()
            )));
        }
        path
    };
    let components = canonical.split('/').collect::<Vec<_>>();
    if canonical.is_empty()
        || components.len() > MAX_ANDROID_RELEASE_PATH_DEPTH
        || components
            .iter()
            .any(|component| component.is_empty() || matches!(*component, "." | ".."))
    {
        return Err(AndroidError::conformance(format!(
            "Android release archive '{}' contains traversing, empty, or over-depth path '{path}'",
            archive.display()
        )));
    }
    Ok(canonical.to_owned())
}

fn insert_android_archive_node(
    nodes: &mut BTreeMap<String, AndroidArchiveNodeKind>,
    path: &str,
    kind: AndroidArchiveNodeKind,
    archive: &Path,
) -> Result<(), AndroidError> {
    let normalized: String = path.nfc().case_fold().nfc().collect();
    if nodes.contains_key(&normalized) {
        return Err(AndroidError::conformance(format!(
            "Android release archive '{}' contains duplicate or Unicode/case-colliding path '{path}'",
            archive.display()
        )));
    }
    for (existing, existing_kind) in nodes.iter() {
        if *existing_kind == AndroidArchiveNodeKind::File
            && normalized
                .strip_prefix(existing)
                .is_some_and(|suffix| suffix.starts_with('/'))
        {
            return Err(AndroidError::conformance(format!(
                "Android release archive '{}' path descends from a regular file: {path}",
                archive.display()
            )));
        }
        if kind == AndroidArchiveNodeKind::File
            && existing
                .strip_prefix(&normalized)
                .is_some_and(|suffix| suffix.starts_with('/'))
        {
            return Err(AndroidError::conformance(format!(
                "Android release archive '{}' regular file conflicts with a descendant: {path}",
                archive.display()
            )));
        }
    }
    nodes.insert(normalized, kind);
    Ok(())
}

fn insert_android_sample_apk_node(
    nodes: &mut BTreeMap<String, AndroidArchiveNodeKind>,
    path: &str,
    kind: AndroidArchiveNodeKind,
    archive: &Path,
) -> Result<(), AndroidError> {
    // aapt2 legitimately emits case-sensitive resource names such as
    // `res/-L.png` and `res/-l.png`; APK paths must therefore not use the
    // case-folded collision policy applied to distributable AAR archives.
    let normalized: String = path.nfc().collect();
    if nodes.contains_key(&normalized) {
        return Err(AndroidError::conformance(format!(
            "Android sample APK '{}' contains duplicate or Unicode-colliding path '{path}'",
            archive.display()
        )));
    }
    for (existing, existing_kind) in nodes.iter() {
        if *existing_kind == AndroidArchiveNodeKind::File
            && normalized
                .strip_prefix(existing)
                .is_some_and(|suffix| suffix.starts_with('/'))
        {
            return Err(AndroidError::conformance(format!(
                "Android sample APK '{}' path descends from a regular file: {path}",
                archive.display()
            )));
        }
        if kind == AndroidArchiveNodeKind::File
            && existing
                .strip_prefix(&normalized)
                .is_some_and(|suffix| suffix.starts_with('/'))
        {
            return Err(AndroidError::conformance(format!(
                "Android sample APK '{}' regular file conflicts with a descendant: {path}",
                archive.display()
            )));
        }
    }
    nodes.insert(normalized, kind);
    Ok(())
}

fn android_archive_jni_abi(path: &str) -> Option<&str> {
    let mut components = path.split('/');
    if components.next()? != "jni" {
        return None;
    }
    components.next()
}

fn android_archive_is_runtime_library(path: &str) -> bool {
    let Some(name) = path.rsplit('/').next() else {
        return false;
    };
    is_android_runtime_shared_object(name)
        && [
            "libav",
            "libsw",
            "libpostproc",
            "libxml2",
            "libssl",
            "libcrypto",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn is_android_runtime_shared_object(name: &str) -> bool {
    name.strip_suffix(".so").is_some()
        || name
            .split_once(".so.")
            .is_some_and(|(base, suffix)| !base.is_empty() && !suffix.is_empty())
}

fn is_android_test_fixture_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let components = lower.split('/').collect::<Vec<_>>();
    if components.iter().any(|component| {
        matches!(
            *component,
            "subtitle_contract"
                | "testfixture"
                | "test-fixture"
                | "test_fixture"
                | "testfixtures"
                | "test-fixtures"
                | "test_fixtures"
                | "testasset"
                | "test-asset"
                | "test_asset"
                | "testassets"
                | "test-assets"
                | "test_assets"
                | "testdata"
        )
    }) {
        return true;
    }
    components
        .windows(2)
        .any(|pair| pair[0] == "fixtures" && matches!(pair[1], "contracts" | "media"))
        || matches!(
            components.last().copied(),
            Some("tiny-aac.m4a" | "tiny-h264-aac.m4v" | "tiny-h264-aac-mediacodec.m4v")
        )
}

fn verify_android_plugin_registry(
    root: &Path,
    archive_path: &Path,
    index: &AndroidArchiveIndex,
    abi: &str,
    plugin_id: &str,
    library_name: &str,
    manifest: &str,
) -> Result<(), AndroidError> {
    let registry_entry = format!("assets/vesper/plugins/{abi}/{plugin_id}.json");
    if index.registry_entries.len() != 1 || !index.registry_entries.contains(&registry_entry) {
        return Err(AndroidError::conformance(format!(
            "Android plugin AAR '{}' must contain exactly registry fragment '{registry_entry}'",
            archive_path.display()
        )));
    }
    let library_entry = format!("jni/{abi}/lib{library_name}.so");
    if !index.files.contains(&library_entry) {
        return Err(AndroidError::conformance(format!(
            "Android plugin AAR '{}' is missing runtime library '{library_entry}'",
            archive_path.display()
        )));
    }
    let library = read_android_archive_entry(
        archive_path,
        &library_entry,
        MAX_ANDROID_RELEASE_ENTRY_BYTES,
    )?;
    let actual_registry = read_android_archive_entry(
        archive_path,
        &registry_entry,
        MAX_ANDROID_FFMPEG_METADATA_BYTES,
    )?;
    let manifest_path = root.join(manifest);
    let metadata = fs::symlink_metadata(&manifest_path).map_err(|error| {
        AndroidError::storage(format!(
            "failed to inspect Android plugin descriptor '{}': {error}",
            manifest_path.display()
        ))
    })?;
    if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
        return Err(AndroidError::conformance(format!(
            "Android plugin descriptor '{}' is not a bounded regular non-symlink file",
            manifest_path.display()
        )));
    }
    let manifest_source = fs::read_to_string(&manifest_path).map_err(|error| {
        AndroidError::storage(format!(
            "failed to read Android plugin descriptor '{}': {error}",
            manifest_path.display()
        ))
    })?;
    let project = PluginProjectManifest::from_toml(&manifest_source).map_err(|error| {
        AndroidError::conformance(format!(
            "invalid Android plugin project manifest '{}': {error}",
            manifest_path.display()
        ))
    })?;
    let descriptor = project
        .descriptor()
        .clone()
        .canonicalize()
        .map_err(|error| {
            AndroidError::conformance(format!(
                "invalid Android plugin descriptor in project manifest '{}': {error}",
                manifest_path.display()
            ))
        })?;
    let mut library_file = tempfile::NamedTempFile::new().map_err(|error| {
        AndroidError::storage(format!(
            "failed to create Android plugin registry verification file: {error}"
        ))
    })?;
    library_file.write_all(&library).map_err(|error| {
        AndroidError::storage(format!(
            "failed to stage Android plugin registry verification bytes: {error}"
        ))
    })?;
    library_file.flush().map_err(|error| {
        AndroidError::storage(format!(
            "failed to flush Android plugin registry verification bytes: {error}"
        ))
    })?;
    let fragment = EmbeddedRegistryFragment::generate(
        &descriptor,
        &EmbeddedRegistryTarget::AndroidNativeLibrary {
            target: ANDROID_RUST_TARGET.to_owned(),
            architecture: abi.to_owned(),
            minimum_os: "26".to_owned(),
            library_name: library_name.to_owned(),
            artifact_path: library_file.path().to_path_buf(),
        },
    )
    .map_err(|error| {
        AndroidError::conformance(format!(
            "failed to generate expected Android plugin registry fragment: {error}"
        ))
    })?;
    if fragment.file_name() != format!("{plugin_id}.json")
        || fragment.canonical_json() != actual_registry
    {
        return Err(AndroidError::conformance(format!(
            "Android plugin registry fragment does not describe the packaged runtime library: {}",
            archive_path.display()
        )));
    }
    Ok(())
}

fn read_archive_profile_receipt(
    archive_path: &Path,
    entry: &str,
    label: &str,
) -> Result<String, AndroidError> {
    let bytes = read_android_archive_entry(archive_path, entry, MAX_ANDROID_FFMPEG_METADATA_BYTES)?;
    parse_profile_receipt(
        &bytes,
        label,
        &format!("{}:{entry}", archive_path.display()),
    )
}

fn parse_profile_receipt(
    bytes: &[u8],
    label: &str,
    location: &str,
) -> Result<String, AndroidError> {
    let value = std::str::from_utf8(bytes).map_err(|error| {
        AndroidError::conformance(format!(
            "{label} profile receipt '{location}' is not UTF-8: {error}"
        ))
    })?;
    let value = value.trim();
    if value.is_empty()
        || value.len() > 256
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(AndroidError::conformance(format!(
            "{label} profile receipt '{location}' is malformed"
        )));
    }
    Ok(value.to_owned())
}

fn verify_ffmpeg_runtime_release_metadata(
    archive_path: &Path,
    abi: &str,
    lock: &AndroidFfmpegReleaseLock,
) -> Result<(), AndroidError> {
    let entry = format!("assets/vesper-ffmpeg-runtime/{abi}-metadata.txt");
    let bytes =
        read_android_archive_entry(archive_path, &entry, MAX_ANDROID_FFMPEG_METADATA_BYTES)?;
    let source = std::str::from_utf8(&bytes).map_err(|error| {
        AndroidError::conformance(format!(
            "FFmpeg runtime metadata '{entry}' is not UTF-8: {error}"
        ))
    })?;
    let mut values = BTreeMap::new();
    for (line_number, line) in source.lines().enumerate() {
        if line_number == 0 && !line.contains('=') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            AndroidError::conformance(format!(
                "invalid FFmpeg runtime metadata line {} in '{}',",
                line_number + 1,
                archive_path.display()
            ))
        })?;
        if key.is_empty() || values.insert(key, value).is_some() {
            return Err(AndroidError::conformance(format!(
                "duplicate or empty FFmpeg runtime metadata key '{key}' in '{}'",
                archive_path.display()
            )));
        }
    }
    for (key, expected) in [
        ("ffmpeg_version", lock.version.as_str()),
        ("source_url", lock.source_url.as_str()),
        ("source_sha256", lock.source_sha256.as_str()),
    ] {
        let actual = values.get(key).copied().ok_or_else(|| {
            AndroidError::conformance(format!(
                "FFmpeg runtime metadata is missing '{key}': {}",
                archive_path.display()
            ))
        })?;
        if actual != expected {
            return Err(AndroidError::conformance(format!(
                "FFmpeg runtime metadata '{key}' is '{actual}', expected '{expected}': {}",
                archive_path.display()
            )));
        }
    }
    Ok(())
}

fn read_android_archive_entry(
    archive_path: &Path,
    entry_name: &str,
    maximum_bytes: u64,
) -> Result<Vec<u8>, AndroidError> {
    let file = File::open(archive_path).map_err(|error| {
        AndroidError::storage(format!(
            "failed to open Android archive '{}': {error}",
            archive_path.display()
        ))
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        AndroidError::conformance(format!(
            "invalid Android archive '{}': {error}",
            archive_path.display()
        ))
    })?;
    let mut entry = archive.by_name(entry_name).map_err(|error| {
        AndroidError::conformance(format!(
            "Android archive '{}' is missing entry '{entry_name}': {error}",
            archive_path.display()
        ))
    })?;
    if !entry.is_file() || entry.size() > maximum_bytes {
        return Err(AndroidError::conformance(format!(
            "Android archive entry '{entry_name}' is not a bounded regular file"
        )));
    }
    let mut bytes = Vec::with_capacity(entry.size().min(maximum_bytes) as usize);
    Read::by_ref(&mut entry)
        .take(maximum_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            AndroidError::storage(format!(
                "failed to read Android archive entry '{entry_name}': {error}"
            ))
        })?;
    if bytes.len() as u64 > maximum_bytes {
        return Err(AndroidError::conformance(format!(
            "Android archive entry '{entry_name}' exceeds {maximum_bytes} bytes"
        )));
    }
    Ok(bytes)
}

fn build_aar_for_abis(
    root: &Path,
    module_task: &str,
    include_optional: bool,
    requested_abis: &[String],
) -> Result<(), AndroidError> {
    build_aar_for_abis_with_options(root, module_task, include_optional, requested_abis, None)
}

fn build_aar_for_abis_with_maven(
    root: &Path,
    module_task: &str,
    requested_abis: &[String],
    options: &MavenStagingOptions<'_>,
) -> Result<(), AndroidError> {
    build_aar_for_abis_with_options(root, module_task, false, requested_abis, Some(options))
}

fn build_aar_for_abis_with_options(
    root: &Path,
    module_task: &str,
    include_optional: bool,
    requested_abis: &[String],
    maven: Option<&MavenStagingOptions<'_>>,
) -> Result<(), AndroidError> {
    let cancellation = external_process::InterruptDeferral::start("Android AAR build")
        .map_err(|error| AndroidError::worker(error.to_string()))?;
    let result = build_aar_transaction(
        root,
        module_task,
        include_optional,
        requested_abis,
        maven,
        &cancellation,
    );
    let cancelled = cancellation.finish();
    match (result, cancelled) {
        (Ok(()), true) => Err(AndroidError::worker(
            "Android AAR build was cancelled after its outputs were committed",
        )),
        (Err(error), true) => Err(AndroidError::worker(format!(
            "Android AAR build was cancelled; {error}"
        ))),
        (result, false) => result,
    }
}

fn build_aar_transaction(
    root: &Path,
    module_task: &str,
    include_optional: bool,
    requested_abis: &[String],
    maven: Option<&MavenStagingOptions<'_>>,
    cancellation: &external_process::InterruptDeferral,
) -> Result<(), AndroidError> {
    let _aar_build_lock = AndroidBuildLock::acquire(root, "aar")?;
    let _jni_build_lock = AndroidBuildLock::acquire(root, "jni")?;
    let ffmpeg_release_lock = maven
        .is_some()
        .then(|| AndroidFfmpegReleaseLock::load(root))
        .transpose()?;
    let stages = build_aar_stages(
        root,
        module_task,
        include_optional,
        requested_abis,
        ffmpeg_release_lock.as_ref(),
        maven,
        cancellation,
    )?;
    promote_staged_directories(stages, cancellation)
}

fn build_aar_stages(
    root: &Path,
    module_task: &str,
    include_optional: bool,
    requested_abis: &[String],
    ffmpeg_release_lock: Option<&AndroidFfmpegReleaseLock>,
    maven: Option<&MavenStagingOptions<'_>>,
    cancellation: &external_process::InterruptDeferral,
) -> Result<Vec<StagedGeneratedDirectory>, AndroidError> {
    let selected_abis = resolve_selected_abis(requested_abis)?;
    let selected_abis_csv = selected_abis.join(",");
    let project =
        require_contained_directory(root, &root.join("lib/android"), "Android library project")?;
    let fallback = root.join("examples/android-compose-host");
    let gradle = gradle::resolve(&project, Some(&fallback)).map_err(map_gradle_error)?;
    let gradle_user_home = gradle::service_home(&project);
    let host_stage = StagedGeneratedDirectory::new(
        root,
        project.join("vesper-player-kit/src/main/jniLibs"),
        ".vesper-android-host-jni-stage-",
        "Android host JNI output",
    )?;

    let mut tasks = vec![
        format!(":vesper-player-kit:{module_task}"),
        format!(":vesper-player-kit-compose:{module_task}"),
        format!(":vesper-player-kit-compose-ui:{module_task}"),
    ];
    let needs_external_playback_distribution = maven.is_some();
    let optional_build = if include_optional || needs_external_playback_distribution {
        let optional_build = build_optional_android_plugins(
            root,
            &project,
            &selected_abis,
            ffmpeg_release_lock,
            cancellation,
        )?;
        append_optional_distribution_tasks(
            &mut tasks,
            module_task,
            include_optional,
            needs_external_playback_distribution,
        );
        Some(optional_build)
    } else {
        None
    };
    let vesper_cli = optional_build
        .as_ref()
        .map(|build| build.vesper_cli.clone())
        .map(Ok)
        .unwrap_or_else(|| {
            env::current_exe().map_err(|error| {
                AndroidError::storage(format!("failed to resolve the current Vesper CLI: {error}"))
            })
        })?;
    require_executable_file(&vesper_cli, "Vesper CLI for Android Gradle tasks")?;

    let mut command = Command::new(gradle);
    command
        .current_dir(root)
        .env("GRADLE_USER_HOME", gradle_user_home)
        .env("VESPER_CLI", vesper_cli)
        .env(PARENT_SUPERVISES_PROCESS_GROUP_ENV, "1")
        .env(PARENT_HOLDS_JNI_LOCK_ENV, "1")
        .env(HOST_JNI_STAGING_ENV, host_stage.path())
        .env("RUST_ANDROID_ABIS", &selected_abis_csv)
        .arg("-p")
        .arg(&project)
        .arg(format!("-Pvesper.player.android.abis={selected_abis_csv}"))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(maven) = maven {
        command
            .arg(format!(
                "-Pvesper.maven.repositoryDirectory={}",
                maven.repository_directory.display()
            ))
            .arg(format!("-Pvesper.maven.groupId={}", maven.group_id))
            .arg(format!("-Pvesper.mavenVersion={}", maven.version))
            .env("MAVEN_GPG_PRIVATE_KEY", maven.signing_key)
            .env_remove("MAVEN_GPG_PASSPHRASE");
        if let Some(passphrase) = maven.signing_passphrase {
            command.env("MAVEN_GPG_PASSPHRASE", passphrase);
        }
    }
    command.args(tasks);
    if let Some(build) = optional_build.as_ref() {
        configure_optional_android_build_environment(&mut command, build, ffmpeg_release_lock);
    }
    require_success_in_deferral(&mut command, "Android AAR Gradle build", cancellation)?;
    validate_staged_host_jni(&host_stage, &selected_abis)?;
    let mut stages = Vec::with_capacity(if optional_build.is_some() { 12 } else { 1 });
    stages.push(host_stage);
    if let Some(build) = optional_build {
        validate_required_staged_file(
            &build.external_relay,
            Path::new(&selected_abis[0]).join(ANDROID_RELAY_LIBRARY),
            "external playback relay library",
        )?;
        validate_required_staged_file(
            &build.external_relay_assets,
            PathBuf::from(FFMPEG_PROFILE_HASH),
            "external playback relay profile hash",
        )?;
        validate_optional_profile_receipts(&build)?;
        stages.extend(build.into_stages());
    }
    for stage in &stages {
        stage.validate()?;
    }
    Ok(stages)
}

fn append_optional_distribution_tasks(
    tasks: &mut Vec<String>,
    module_task: &str,
    include_optional: bool,
    publish_maven: bool,
) {
    tasks.extend([
        format!(":vesper-player-kit-ffmpeg-runtime:{module_task}"),
        format!(":vesper-player-kit-external-playback:{module_task}"),
        format!(":vesper-player-kit-source-normalizer-ffmpeg:{module_task}"),
        format!(":vesper-player-kit-remux-ffmpeg:{module_task}"),
    ]);
    if include_optional {
        tasks.extend([
            format!(":vesper-player-kit-decoder-mediacodec:{module_task}"),
            format!(":vesper-player-kit-frame-processor-diagnostic:{module_task}"),
        ]);
    }
    if include_optional || publish_maven {
        tasks.push(format!(
            ":vesper-player-kit-performance-diagnostics:{module_task}"
        ));
    }
}

fn validate_staged_host_jni(
    stage: &StagedGeneratedDirectory,
    selected_abis: &[String],
) -> Result<(), AndroidError> {
    for abi in selected_abis {
        validate_required_staged_file(
            stage,
            Path::new(abi).join(ANDROID_JNI_LIBRARY),
            "host JNI library",
        )?;
    }
    Ok(())
}

struct OptionalAndroidBuild {
    decoder: StagedGeneratedDirectory,
    source_normalizer: StagedGeneratedDirectory,
    source_normalizer_assets: StagedGeneratedDirectory,
    remux: StagedGeneratedDirectory,
    remux_assets: StagedGeneratedDirectory,
    frame_processor: StagedGeneratedDirectory,
    performance_diagnostics: StagedGeneratedDirectory,
    ffmpeg_runtime: StagedGeneratedDirectory,
    ffmpeg_runtime_assets: StagedGeneratedDirectory,
    external_relay: StagedGeneratedDirectory,
    external_relay_assets: StagedGeneratedDirectory,
    vesper_cli: PathBuf,
}

fn configure_optional_android_build_environment(
    command: &mut Command,
    build: &OptionalAndroidBuild,
    ffmpeg_release_lock: Option<&AndroidFfmpegReleaseLock>,
) {
    command
        .env(DECODER_JNI_STAGING_ENV, build.decoder.path())
        .env(
            SOURCE_NORMALIZER_JNI_STAGING_ENV,
            build.source_normalizer.path(),
        )
        .env(
            SOURCE_NORMALIZER_ASSETS_STAGING_ENV,
            build.source_normalizer_assets.packaging_root(),
        )
        .env(REMUX_JNI_STAGING_ENV, build.remux.path())
        .env(
            REMUX_ASSETS_STAGING_ENV,
            build.remux_assets.packaging_root(),
        )
        .env(
            FRAME_PROCESSOR_JNI_STAGING_ENV,
            build.frame_processor.path(),
        )
        .env(
            PERFORMANCE_DIAGNOSTICS_JNI_STAGING_ENV,
            build.performance_diagnostics.path(),
        )
        .env(FFMPEG_RUNTIME_JNI_STAGING_ENV, build.ffmpeg_runtime.path())
        .env(
            FFMPEG_RUNTIME_ASSETS_STAGING_ENV,
            build.ffmpeg_runtime_assets.packaging_root(),
        )
        .env(EXTERNAL_RELAY_JNI_STAGING_ENV, build.external_relay.path())
        .env(
            EXTERNAL_RELAY_ASSETS_STAGING_ENV,
            build.external_relay_assets.packaging_root(),
        )
        .env("VESPER_ANDROID_SKIP_FFMPEG_RUNTIME_BUILD", "1");
    if let Some(lock) = ffmpeg_release_lock {
        lock.apply(command);
    }
}

impl OptionalAndroidBuild {
    fn into_stages(self) -> Vec<StagedGeneratedDirectory> {
        vec![
            self.decoder,
            self.source_normalizer,
            self.source_normalizer_assets,
            self.remux,
            self.remux_assets,
            self.frame_processor,
            self.performance_diagnostics,
            self.ffmpeg_runtime,
            self.ffmpeg_runtime_assets,
            self.external_relay,
            self.external_relay_assets,
        ]
    }
}

fn build_optional_android_plugins(
    root: &Path,
    project: &Path,
    selected_abis: &[String],
    ffmpeg_release_lock: Option<&AndroidFfmpegReleaseLock>,
    cancellation: &external_process::InterruptDeferral,
) -> Result<OptionalAndroidBuild, AndroidError> {
    let selected_abis_csv = selected_abis.join(",");
    let decoder_module = project.join("vesper-player-kit-decoder-mediacodec");
    let source_normalizer_module = project.join("vesper-player-kit-source-normalizer-ffmpeg");
    let remux_module = project.join("vesper-player-kit-remux-ffmpeg");
    let frame_processor_module = project.join("vesper-player-kit-frame-processor-diagnostic");
    let performance_diagnostics_module = project.join("vesper-player-kit-performance-diagnostics");
    let decoder = StagedGeneratedDirectory::new(
        root,
        decoder_module.join("src/main/jniLibs"),
        ".vesper-android-decoder-stage-",
        "Android MediaCodec decoder plugin output",
    )?;
    let source_normalizer = StagedGeneratedDirectory::new(
        root,
        source_normalizer_module.join("src/main/jniLibs"),
        ".vesper-android-source-normalizer-stage-",
        "Android SourceNormalizer plugin output",
    )?;
    let source_normalizer_assets = StagedGeneratedDirectory::new_nested(
        root,
        source_normalizer_module.join("src/main/assets/vesper-source-normalizer-ffmpeg"),
        ".vesper-android-source-normalizer-metadata-stage-",
        "Android SourceNormalizer plugin metadata",
    )?;
    let remux = StagedGeneratedDirectory::new(
        root,
        remux_module.join("src/main/jniLibs"),
        ".vesper-android-remux-stage-",
        "Android remux plugin output",
    )?;
    let remux_assets = StagedGeneratedDirectory::new_nested(
        root,
        remux_module.join("src/main/assets/vesper-remux-ffmpeg"),
        ".vesper-android-remux-metadata-stage-",
        "Android remux plugin metadata",
    )?;
    let frame_processor = StagedGeneratedDirectory::new(
        root,
        frame_processor_module.join("src/main/jniLibs"),
        ".vesper-android-frame-processor-stage-",
        "Android FrameProcessor plugin output",
    )?;
    let performance_diagnostics = StagedGeneratedDirectory::new(
        root,
        performance_diagnostics_module.join("src/main/jniLibs"),
        ".vesper-android-performance-diagnostics-stage-",
        "Android performance diagnostics plugin output",
    )?;
    let ffmpeg_runtime = StagedGeneratedDirectory::new(
        root,
        project.join("vesper-player-kit-ffmpeg-runtime/src/main/jniLibs"),
        ".vesper-android-ffmpeg-runtime-stage-",
        "Android FFmpeg runtime output",
    )?;
    let ffmpeg_runtime_assets = StagedGeneratedDirectory::new_nested(
        root,
        project.join("vesper-player-kit-ffmpeg-runtime/src/main/assets/vesper-ffmpeg-runtime"),
        ".vesper-android-ffmpeg-runtime-assets-stage-",
        "Android FFmpeg runtime metadata",
    )?;
    let external_relay = StagedGeneratedDirectory::new(
        root,
        project.join("vesper-player-kit-external-playback/src/main/jniLibs"),
        ".vesper-android-external-relay-stage-",
        "Android external playback relay output",
    )?;
    let external_relay_assets = StagedGeneratedDirectory::new_nested(
        root,
        project.join("vesper-player-kit-external-playback/src/main/assets/vesper-relay-ffmpeg"),
        ".vesper-android-external-relay-assets-stage-",
        "Android external playback relay metadata",
    )?;
    let current_cli = env::current_exe().map_err(|error| {
        AndroidError::storage(format!(
            "failed to resolve the current Vesper CLI for optional Android plugins: {error}"
        ))
    })?;
    require_executable_file(&current_cli, "Vesper CLI for optional Android plugins")?;
    let commands = [
        (
            vec![
                OsString::from("ffmpeg"),
                OsString::from("--root"),
                root.as_os_str().to_owned(),
                OsString::from("--platform"),
                OsString::from("android"),
                OsString::from("--profile"),
                OsString::from("default"),
                OsString::from("--android-artifact"),
                OsString::from("runtime-aar"),
                OsString::from("--abi"),
                OsString::from(selected_abis_csv.clone()),
            ],
            "Android FFmpeg runtime build",
        ),
        (
            vec![
                OsString::from("android"),
                OsString::from("--root"),
                root.as_os_str().to_owned(),
                OsString::from("__runtime-free-plugin"),
                OsString::from("decoder-mediacodec"),
                decoder.path().as_os_str().to_owned(),
                OsString::from("release"),
            ],
            "Android MediaCodec decoder plugin build",
        ),
        (
            vec![
                OsString::from("android"),
                OsString::from("--root"),
                root.as_os_str().to_owned(),
                OsString::from("__ffmpeg-plugin"),
                OsString::from("source-normalizer"),
                source_normalizer.path().as_os_str().to_owned(),
                OsString::from("release"),
                OsString::from("--profile"),
                OsString::from("default"),
                OsString::from("--metadata-dir"),
                source_normalizer_assets.path().as_os_str().to_owned(),
            ],
            "Android SourceNormalizer FFmpeg plugin build",
        ),
        (
            vec![
                OsString::from("android"),
                OsString::from("--root"),
                root.as_os_str().to_owned(),
                OsString::from("__ffmpeg-plugin"),
                OsString::from("remux"),
                remux.path().as_os_str().to_owned(),
                OsString::from("release"),
                OsString::from("--profile"),
                OsString::from("default"),
                OsString::from("--metadata-dir"),
                remux_assets.path().as_os_str().to_owned(),
            ],
            "Android remux FFmpeg plugin build",
        ),
        (
            vec![
                OsString::from("android"),
                OsString::from("--root"),
                root.as_os_str().to_owned(),
                OsString::from("__runtime-free-plugin"),
                OsString::from("frame-processor-diagnostic"),
                frame_processor.path().as_os_str().to_owned(),
                OsString::from("release"),
            ],
            "Android FrameProcessor diagnostic plugin build",
        ),
        (
            vec![
                OsString::from("android"),
                OsString::from("--root"),
                root.as_os_str().to_owned(),
                OsString::from("__runtime-free-plugin"),
                OsString::from("performance-diagnostics"),
                performance_diagnostics.path().as_os_str().to_owned(),
                OsString::from("release"),
            ],
            "Android performance diagnostics plugin build",
        ),
        (
            vec![
                OsString::from("android"),
                OsString::from("--root"),
                root.as_os_str().to_owned(),
                OsString::from("external-playback-jni"),
                external_relay.path().as_os_str().to_owned(),
                OsString::from("--assets-directory"),
                external_relay_assets
                    .packaging_root()
                    .as_os_str()
                    .to_owned(),
                OsString::from("--profile"),
                OsString::from("release"),
                OsString::from("--ffmpeg-profile"),
                OsString::from("default"),
                OsString::from("--skip-ffmpeg-runtime"),
            ],
            "Android external playback relay build",
        ),
    ];
    for (arguments, label) in commands {
        let mut command = Command::new(&current_cli);
        command
            .current_dir(root)
            .env(PARENT_SUPERVISES_PROCESS_GROUP_ENV, "1")
            .env("RUST_ANDROID_ABIS", &selected_abis_csv)
            .env(FFMPEG_RUNTIME_JNI_STAGING_ENV, ffmpeg_runtime.path())
            .env(
                FFMPEG_RUNTIME_ASSETS_STAGING_ENV,
                ffmpeg_runtime_assets.packaging_root(),
            )
            .env(
                "VESPER_ANDROID_FFMPEG_OUTPUT_DIR",
                android_ffmpeg_output_directory(root),
            )
            .args(arguments)
            .stdin(Stdio::inherit())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        if let Some(lock) = ffmpeg_release_lock {
            lock.apply(&mut command);
        }
        require_success_in_deferral(&mut command, label, cancellation)?;
    }

    let build = OptionalAndroidBuild {
        decoder,
        source_normalizer,
        source_normalizer_assets,
        remux,
        remux_assets,
        frame_processor,
        performance_diagnostics,
        ffmpeg_runtime,
        ffmpeg_runtime_assets,
        external_relay,
        external_relay_assets,
        vesper_cli: current_cli,
    };
    validate_optional_android_artifacts(&build, selected_abis)?;

    for stage in [
        &build.decoder,
        &build.source_normalizer,
        &build.source_normalizer_assets,
        &build.remux,
        &build.remux_assets,
        &build.frame_processor,
        &build.performance_diagnostics,
        &build.ffmpeg_runtime,
        &build.ffmpeg_runtime_assets,
        &build.external_relay,
        &build.external_relay_assets,
    ] {
        stage.validate()?;
        stage.target.revalidate_target()?;
    }
    let _ = cancellation;
    Ok(build)
}

fn validate_optional_android_artifacts(
    build: &OptionalAndroidBuild,
    selected_abis: &[String],
) -> Result<(), AndroidError> {
    for abi in selected_abis {
        validate_required_staged_file(
            &build.decoder,
            Path::new(abi).join(ANDROID_DECODER_LIBRARY),
            "MediaCodec decoder plugin library",
        )?;
        validate_required_staged_file(
            &build.source_normalizer,
            Path::new(abi).join(ANDROID_SOURCE_NORMALIZER_LIBRARY),
            "SourceNormalizer plugin library",
        )?;
        validate_required_staged_file(
            &build.remux,
            Path::new(abi).join(ANDROID_REMUX_LIBRARY),
            "remux plugin library",
        )?;
        validate_required_staged_file(
            &build.frame_processor,
            Path::new(abi).join(ANDROID_FRAME_PROCESSOR_LIBRARY),
            "FrameProcessor plugin library",
        )?;
        validate_required_staged_file(
            &build.performance_diagnostics,
            Path::new(abi).join(ANDROID_PERFORMANCE_DIAGNOSTICS_LIBRARY),
            "performance diagnostics plugin library",
        )?;
        for library in ["libavcodec.so", "libavformat.so", "libavutil.so"] {
            validate_required_staged_file(
                &build.ffmpeg_runtime,
                Path::new(abi).join(library),
                "FFmpeg runtime library",
            )?;
        }
        validate_required_staged_file(
            &build.source_normalizer_assets,
            PathBuf::from(format!("{abi}-vesper-ffmpeg-build-metadata.txt")),
            "SourceNormalizer FFmpeg build metadata",
        )?;
        validate_required_staged_file(
            &build.remux_assets,
            PathBuf::from(format!("{abi}-vesper-ffmpeg-build-metadata.txt")),
            "remux FFmpeg build metadata",
        )?;
    }
    validate_required_staged_file(
        &build.source_normalizer_assets,
        PathBuf::from(FFMPEG_PROFILE_HASH),
        "SourceNormalizer profile hash",
    )?;
    validate_required_staged_file(
        &build.remux_assets,
        PathBuf::from(FFMPEG_PROFILE_HASH),
        "remux profile hash",
    )?;
    validate_required_staged_file(
        &build.remux_assets,
        PathBuf::from(REMUX_PROFILE_METADATA),
        "remux profile metadata",
    )?;
    validate_required_staged_file(
        &build.source_normalizer_assets,
        PathBuf::from(SOURCE_NORMALIZER_PROFILE_METADATA),
        "SourceNormalizer profile metadata",
    )?;
    validate_required_staged_file(
        &build.ffmpeg_runtime_assets,
        PathBuf::from(FFMPEG_PROFILE_HASH),
        "FFmpeg runtime profile hash",
    )
}

fn validate_optional_profile_receipts(build: &OptionalAndroidBuild) -> Result<(), AndroidError> {
    let source_normalizer = read_profile_hash(
        &build
            .source_normalizer_assets
            .path()
            .join(FFMPEG_PROFILE_HASH),
        "SourceNormalizer",
    )?;
    let runtime = read_profile_hash(
        &build.ffmpeg_runtime_assets.path().join(FFMPEG_PROFILE_HASH),
        "FFmpeg runtime",
    )?;
    let remux = read_profile_hash(
        &build.remux_assets.path().join(FFMPEG_PROFILE_HASH),
        "remux",
    )?;
    let relay = read_profile_hash(
        &build.external_relay_assets.path().join(FFMPEG_PROFILE_HASH),
        "external playback relay",
    )?;
    if source_normalizer == remux && remux == runtime && runtime == relay {
        Ok(())
    } else {
        Err(AndroidError::conformance(format!(
            "optional Android FFmpeg profile receipts do not match: SourceNormalizer={source_normalizer}, remux={remux}, runtime={runtime}, relay={relay}"
        )))
    }
}

fn read_profile_hash(path: &Path, label: &str) -> Result<String, AndroidError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AndroidError::conformance(format!(
            "failed to inspect {label} profile receipt '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_ANDROID_FFMPEG_METADATA_BYTES {
        return Err(AndroidError::conformance(format!(
            "{label} profile receipt '{}' is not a bounded regular non-symlink file",
            path.display()
        )));
    }
    let bytes = fs::read(path).map_err(|error| {
        AndroidError::storage(format!(
            "failed to read {label} profile receipt '{}': {error}",
            path.display()
        ))
    })?;
    let value = std::str::from_utf8(&bytes).map_err(|error| {
        AndroidError::conformance(format!(
            "{label} profile receipt '{}' is not UTF-8: {error}",
            path.display()
        ))
    })?;
    let value = value.trim();
    if value.is_empty()
        || value.len() > 256
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(AndroidError::conformance(format!(
            "{label} profile receipt '{}' is malformed",
            path.display()
        )));
    }
    Ok(value.to_owned())
}

fn validate_required_staged_file(
    stage: &StagedGeneratedDirectory,
    relative_path: PathBuf,
    description: &str,
) -> Result<(), AndroidError> {
    stage.validate()?;
    if let Some(relative_parent) = relative_path.parent()
        && !relative_parent.as_os_str().is_empty()
    {
        let parent = stage.path().join(relative_parent);
        let metadata = fs::symlink_metadata(&parent).map_err(|error| {
            AndroidError::conformance(format!(
                "{} did not stage required {description} parent '{}': {error}",
                stage.label,
                parent.display()
            ))
        })?;
        if !metadata.file_type().is_dir() {
            return Err(AndroidError::conformance(format!(
                "{} required {description} parent '{}' is not a regular non-symlink directory",
                stage.label,
                parent.display()
            )));
        }
    }
    let artifact = stage.path().join(relative_path);
    let metadata = fs::symlink_metadata(&artifact).map_err(|error| {
        AndroidError::conformance(format!(
            "{} did not stage required {description} '{}': {error}",
            stage.label,
            artifact.display()
        ))
    })?;
    if metadata.file_type().is_file() {
        Ok(())
    } else {
        Err(AndroidError::conformance(format!(
            "{} required {description} '{}' is not a regular non-symlink file",
            stage.label,
            artifact.display()
        )))
    }
}

pub(crate) fn resolve_selected_abis(requested: &[String]) -> Result<Vec<String>, AndroidError> {
    resolve_selected_abis_from(requested, env::var_os("RUST_ANDROID_ABIS").as_deref())
}

fn resolve_selected_abis_from(
    requested: &[String],
    environment: Option<&OsStr>,
) -> Result<Vec<String>, AndroidError> {
    let selected = if requested.is_empty() {
        environment
            .filter(|value| !value.is_empty())
            .map(|value| {
                value
                    .to_string_lossy()
                    .split(|character: char| character == ',' || character.is_whitespace())
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![DEFAULT_ANDROID_ABI.to_owned()])
    } else {
        requested.to_vec()
    };
    if selected.is_empty() {
        return Err(AndroidError::compatibility(
            "No Android ABIs were selected.",
        ));
    }
    for abi in &selected {
        if abi != DEFAULT_ANDROID_ABI {
            return Err(AndroidError::compatibility(format!(
                "Unsupported Android ABI: {abi}\nSupported ABIs: {DEFAULT_ANDROID_ABI}"
            )));
        }
    }
    if selected.len() != 1 {
        return Err(AndroidError::compatibility(format!(
            "Android ABI selection must contain exactly one unique {DEFAULT_ANDROID_ABI} entry"
        )));
    }
    Ok(selected)
}

pub(crate) fn android_sdk_root() -> Result<PathBuf, AndroidError> {
    if let Some(root) = env::var_os("ANDROID_SDK_ROOT").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(root));
    }
    if let Some(root) = env::var_os("ANDROID_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(root));
    }
    let home = env::var_os("HOME").filter(|value| !value.is_empty()).ok_or_else(|| {
        AndroidError::compatibility(
            "ANDROID_SDK_ROOT, ANDROID_HOME, and HOME are unavailable; Android SDK location cannot be resolved",
        )
    })?;
    Ok(PathBuf::from(home).join("Library/Android/sdk"))
}

pub(crate) fn android_ndk_version() -> OsString {
    env::var_os("ANDROID_NDK_VERSION")
        .filter(|value| !value.is_empty())
        .or_else(|| env::var_os("VESPER_ANDROID_NDK_VERSION").filter(|value| !value.is_empty()))
        .unwrap_or_else(|| OsString::from(DEFAULT_ANDROID_NDK_VERSION))
}

pub(crate) fn resolve_ndk_root(
    sdk_root: &Path,
    ndk_version: &OsStr,
) -> Result<PathBuf, AndroidError> {
    if let Some(root) = env::var_os("ANDROID_NDK_ROOT").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(root));
    }
    let requested = sdk_root.join("ndk").join(ndk_version);
    if requested.join("source.properties").is_file() {
        return Ok(requested);
    }

    let ndk_parent = sdk_root.join("ndk");
    let mut candidates = Vec::new();
    let mut inspected_entries = 0_usize;
    match fs::read_dir(&ndk_parent) {
        Ok(entries) => {
            for entry in entries {
                if inspected_entries >= MAX_ANDROID_NDK_DIRECTORY_ENTRIES {
                    return Err(AndroidError::compatibility(format!(
                        "Android NDK directory '{}' contains more than {MAX_ANDROID_NDK_DIRECTORY_ENTRIES} entries; refusing an unbounded installation scan",
                        ndk_parent.display()
                    )));
                }
                inspected_entries += 1;
                let entry = entry.map_err(|error| {
                    AndroidError::storage(format!(
                        "failed to inspect Android NDK directory '{}': {error}",
                        ndk_parent.display()
                    ))
                })?;
                let file_type = entry.file_type().map_err(|error| {
                    AndroidError::storage(format!(
                        "failed to inspect Android NDK candidate '{}': {error}",
                        entry.path().display()
                    ))
                })?;
                if file_type.is_dir() && entry.path().join("source.properties").is_file() {
                    candidates.push(entry.path());
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(AndroidError::storage(format!(
                "failed to inspect Android NDK directory '{}': {error}",
                ndk_parent.display()
            )));
        }
    }
    candidates.sort_by(|left, right| compare_ndk_paths(left, right));
    if let Some(candidate) = candidates.pop() {
        return Ok(candidate);
    }

    Err(AndroidError::compatibility(format!(
        "Android NDK is missing or incomplete at:\n  {}\n\nExpected a complete NDK installation containing:\n  <ndk-dir>/source.properties\n\nInstall Android NDK {} from Android Studio:\n  Settings > Languages & Frameworks > Android SDK > SDK Tools > NDK (Side by side)\nIf Android Studio installed a different NDK version, set ANDROID_NDK_ROOT before running this command.",
        requested.display(),
        ndk_version.to_string_lossy()
    )))
}

fn compare_ndk_paths(left: &Path, right: &Path) -> Ordering {
    let left_name = left.file_name().and_then(OsStr::to_str).unwrap_or("");
    let right_name = right.file_name().and_then(OsStr::to_str).unwrap_or("");
    match (Version::parse(left_name), Version::parse(right_name)) {
        (Ok(left_version), Ok(right_version)) => left_version.cmp(&right_version),
        _ => left.file_name().cmp(&right.file_name()),
    }
}

pub(crate) fn require_rust_target(target: &str) -> Result<(), AndroidError> {
    let rustc = require_path_command("rustc", "rustc is required to verify Android Rust targets")?;
    let result = Command::new(rustc)
        .args(["--print", "target-libdir", "--target", target])
        .output()
        .map_err(|error| {
            AndroidError::worker(format!("failed to inspect Rust target {target}: {error}"))
        })?;
    let target_libdir = String::from_utf8(result.stdout).map_err(|error| {
        AndroidError::compatibility(format!(
            "rustc returned a non-UTF-8 target directory for {target}: {error}"
        ))
    })?;
    if result.status.success() && Path::new(target_libdir.trim()).is_dir() {
        return Ok(());
    }
    Err(AndroidError::compatibility(format!(
        "Required Rust Android targets are missing:\n  {target}\n\nInstall them with:\n  rustup target add {target}"
    )))
}

fn require_success(command: &mut Command, label: &str) -> Result<(), AndroidError> {
    let status = run_command(command, label)?;
    require_success_status(status, label)
}

fn require_success_in_deferral(
    command: &mut Command,
    label: &str,
    cancellation: &external_process::InterruptDeferral,
) -> Result<(), AndroidError> {
    let status = run_command_in_deferral(command, label, cancellation)?;
    require_success_status(status, label)
}

fn require_success_status(status: ExitStatus, label: &str) -> Result<(), AndroidError> {
    if status.success() {
        Ok(())
    } else {
        let message = format!("{label} exited unsuccessfully ({status})");
        let error = match status.code() {
            Some(2) => AndroidError::usage(message),
            Some(3) => AndroidError::storage(message),
            Some(4) => AndroidError::compatibility(message),
            Some(5) => AndroidError::conformance(message),
            Some(6) => AndroidError::worker(message),
            _ => AndroidError::conformance(message),
        };
        Err(error)
    }
}

fn run_command(command: &mut Command, label: &str) -> Result<ExitStatus, AndroidError> {
    #[cfg(unix)]
    let status =
        if env::var_os(PARENT_SUPERVISES_PROCESS_GROUP_ENV).as_deref() == Some(OsStr::new("1")) {
            external_process::run_inherited_process_group(command, label)
        } else {
            external_process::run_interruptible(command, label)
        }
        .map_err(|error| AndroidError::worker(error.to_string()))?;
    #[cfg(not(unix))]
    let status = external_process::run_interruptible(command, label)
        .map_err(|error| AndroidError::worker(error.to_string()))?;
    Ok(status)
}

fn run_command_in_deferral(
    command: &mut Command,
    label: &str,
    cancellation: &external_process::InterruptDeferral,
) -> Result<ExitStatus, AndroidError> {
    #[cfg(unix)]
    let status = external_process::run_interruptible_in_deferral(command, label, cancellation)
        .map_err(|error| AndroidError::worker(error.to_string()))?;
    #[cfg(windows)]
    let status = external_process::run_interruptible_in_deferral(command, label, cancellation)
        .map_err(|error| AndroidError::worker(error.to_string()))?;
    #[cfg(not(any(unix, windows)))]
    let status = external_process::run_interruptible(command, label)
        .map_err(|error| AndroidError::worker(error.to_string()))?;
    Ok(status)
}

fn require_path_command(command: &str, message: &str) -> Result<PathBuf, AndroidError> {
    resolve_path_command(command).ok_or_else(|| AndroidError::compatibility(message))
}

fn resolve_path_command(command: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    for directory in env::split_paths(&paths) {
        let canonical_directory = if directory.as_os_str().is_empty() {
            env::current_dir().ok()?.canonicalize().ok()?
        } else {
            let Ok(canonical) = directory.canonicalize() else {
                continue;
            };
            canonical
        };
        for candidate in executable_candidates(&canonical_directory, command) {
            let Ok(metadata) = fs::metadata(&candidate) else {
                continue;
            };
            if metadata.is_file() && current_process_can_execute(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn executable_candidates(directory: &Path, command: &str) -> Vec<PathBuf> {
    #[cfg(not(windows))]
    let candidates = vec![directory.join(command)];
    #[cfg(windows)]
    let candidates = [".exe", ".cmd", ".bat"]
        .into_iter()
        .map(|extension| directory.join(format!("{command}{extension}")))
        .collect();
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
    root: &Path,
    path: &Path,
    label: &str,
) -> Result<PathBuf, AndroidError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AndroidError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(AndroidError::compatibility(format!(
            "{label} '{}' is not a regular non-symlink directory",
            path.display()
        )));
    }
    let canonical = path.canonicalize().map_err(|error| {
        AndroidError::storage(format!(
            "failed to resolve {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !canonical.starts_with(root) {
        return Err(AndroidError::compatibility(format!(
            "{label} '{}' resolves outside repository root '{}'",
            path.display(),
            root.display()
        )));
    }
    Ok(canonical)
}

fn require_regular_file(path: &Path, label: &str) -> Result<(), AndroidError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AndroidError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if metadata.file_type().is_file() {
        Ok(())
    } else {
        Err(AndroidError::compatibility(format!(
            "{label} '{}' is not a regular non-symlink file",
            path.display()
        )))
    }
}

fn require_executable_file(path: &Path, label: &str) -> Result<(), AndroidError> {
    require_regular_file(path, label)?;
    if current_process_can_execute(path) {
        Ok(())
    } else {
        Err(AndroidError::compatibility(format!(
            "{label} '{}' is not executable",
            path.display()
        )))
    }
}

pub(crate) struct AndroidBuildLock {
    _file: File,
}

impl AndroidBuildLock {
    pub(crate) fn acquire(root: &Path, operation: &str) -> Result<Self, AndroidError> {
        let lock_directory = root.join("lib/android/.gradle/vesper-build-locks");
        fs::create_dir_all(&lock_directory).map_err(|error| {
            AndroidError::storage(format!(
                "failed to create Android build lock directory '{}': {error}",
                lock_directory.display()
            ))
        })?;
        let lock_path = lock_directory.join(format!("vesper-android-{operation}.lock"));
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&lock_path).map_err(|inspect_error| {
                    AndroidError::storage(format!(
                        "failed to inspect Android build lock '{}': {inspect_error}",
                        lock_path.display()
                    ))
                })?;
                if !metadata.file_type().is_file() {
                    return Err(AndroidError::compatibility(format!(
                        "Android build lock '{}' is not a regular non-symlink file",
                        lock_path.display()
                    )));
                }
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&lock_path)
                    .map_err(|open_error| {
                        AndroidError::storage(format!(
                            "failed to open Android build lock '{}': {open_error}",
                            lock_path.display()
                        ))
                    })?
            }
            Err(error) => {
                return Err(AndroidError::storage(format!(
                    "failed to create Android build lock '{}': {error}",
                    lock_path.display()
                )));
            }
        };
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(AndroidError::compatibility(format!(
                "another Android {operation} build is already active for '{}'",
                root.display()
            ))),
            Err(TryLockError::Error(error)) => Err(AndroidError::storage(format!(
                "failed to lock Android {operation} build for '{}': {error}",
                root.display()
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    volume_or_device: u64,
    file_index: u64,
}

#[derive(Clone, Debug)]
struct GeneratedDirectoryTarget {
    path: PathBuf,
    canonical_parent: PathBuf,
    parent_identity: FileIdentity,
    initial_identity: Option<FileIdentity>,
}

struct StagedGeneratedDirectory {
    staging: tempfile::TempDir,
    content_path: PathBuf,
    target: GeneratedDirectoryTarget,
    label: &'static str,
}

impl StagedGeneratedDirectory {
    fn new_external(
        target_path: &Path,
        staging_prefix: &str,
        label: &'static str,
    ) -> Result<Self, AndroidError> {
        let target = external_generated_directory_target(target_path)?;
        let staging = tempfile::Builder::new()
            .prefix(staging_prefix)
            .tempdir_in(&target.canonical_parent)
            .map_err(|error| {
                AndroidError::storage(format!(
                    "failed to create {label} staging directory beside '{}': {error}",
                    target.path.display()
                ))
            })?;
        let content_path = staging.path().to_path_buf();
        Ok(Self {
            staging,
            content_path,
            target,
            label,
        })
    }

    fn new(
        root: &Path,
        target_path: PathBuf,
        staging_prefix: &str,
        label: &'static str,
    ) -> Result<Self, AndroidError> {
        let target = GeneratedDirectoryTarget::preflight(root, target_path)?;
        let staging = tempfile::Builder::new()
            .prefix(staging_prefix)
            .tempdir_in(&target.canonical_parent)
            .map_err(|error| {
                AndroidError::storage(format!(
                    "failed to create {label} staging directory beside '{}': {error}",
                    target.path.display()
                ))
            })?;
        let content_path = staging.path().to_path_buf();
        Ok(Self {
            staging,
            content_path,
            target,
            label,
        })
    }

    fn new_nested(
        root: &Path,
        target_path: PathBuf,
        staging_prefix: &str,
        label: &'static str,
    ) -> Result<Self, AndroidError> {
        let target = GeneratedDirectoryTarget::preflight(root, target_path)?;
        let staging = tempfile::Builder::new()
            .prefix(staging_prefix)
            .tempdir_in(&target.canonical_parent)
            .map_err(|error| {
                AndroidError::storage(format!(
                    "failed to create {label} staging directory beside '{}': {error}",
                    target.path.display()
                ))
            })?;
        let content_path = staging.path().join(target.path.file_name().ok_or_else(|| {
            AndroidError::compatibility(format!(
                "Android generated directory '{}' has no file name",
                target.path.display()
            ))
        })?);
        fs::create_dir(&content_path).map_err(|error| {
            AndroidError::storage(format!(
                "failed to create nested {label} staging directory '{}': {error}",
                content_path.display()
            ))
        })?;
        Ok(Self {
            staging,
            content_path,
            target,
            label,
        })
    }

    fn path(&self) -> &Path {
        &self.content_path
    }

    fn packaging_root(&self) -> &Path {
        self.staging.path()
    }

    fn validate(&self) -> Result<(), AndroidError> {
        let metadata = fs::symlink_metadata(self.path()).map_err(|error| {
            AndroidError::conformance(format!(
                "{} staging directory '{}' is unavailable after its build: {error}",
                self.label,
                self.path().display()
            ))
        })?;
        if metadata.file_type().is_dir() {
            Ok(())
        } else {
            Err(AndroidError::conformance(format!(
                "{} staging path '{}' is not a regular non-symlink directory",
                self.label,
                self.path().display()
            )))
        }
    }

    fn into_source(self) -> GeneratedDirectorySource {
        GeneratedDirectorySource {
            owner: self.staging,
            path: self.content_path,
        }
    }
}

impl GeneratedDirectoryTarget {
    fn preflight(root: &Path, path: PathBuf) -> Result<Self, AndroidError> {
        let parent = path.parent().ok_or_else(|| {
            AndroidError::compatibility(format!(
                "Android generated directory '{}' has no parent",
                path.display()
            ))
        })?;
        let canonical_parent = require_contained_directory(root, parent, "Android output parent")?;
        Self::preflight_with_canonical_parent(path, canonical_parent)
    }

    fn preflight_with_canonical_parent(
        path: PathBuf,
        canonical_parent: PathBuf,
    ) -> Result<Self, AndroidError> {
        let target_path = canonical_parent.join(path.file_name().ok_or_else(|| {
            AndroidError::compatibility(format!(
                "Android generated directory '{}' has no file name",
                path.display()
            ))
        })?);
        let parent_metadata = fs::symlink_metadata(&canonical_parent).map_err(|error| {
            AndroidError::storage(format!(
                "failed to inspect Android output parent '{}': {error}",
                canonical_parent.display()
            ))
        })?;
        let parent_identity =
            path_file_identity(&canonical_parent, &parent_metadata).map_err(|error| {
                AndroidError::storage(format!(
                    "failed to identify Android output parent '{}': {error}",
                    canonical_parent.display()
                ))
            })?;
        let initial_identity = match fs::symlink_metadata(&target_path) {
            Ok(metadata) if metadata.file_type().is_dir() => Some(
                path_file_identity(&target_path, &metadata).map_err(|error| {
                    AndroidError::storage(format!(
                        "failed to identify Android generated directory '{}': {error}",
                        target_path.display()
                    ))
                })?,
            ),
            Ok(_) => {
                return Err(AndroidError::compatibility(format!(
                    "Android generated directory '{}' is not a regular non-symlink directory",
                    target_path.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(AndroidError::storage(format!(
                    "failed to inspect Android generated directory '{}': {error}",
                    target_path.display()
                )));
            }
        };
        Ok(Self {
            path: target_path,
            canonical_parent,
            parent_identity,
            initial_identity,
        })
    }

    fn revalidate_parent(&self) -> Result<(), AndroidError> {
        let metadata = fs::symlink_metadata(&self.canonical_parent).map_err(|error| {
            AndroidError::storage(format!(
                "failed to recheck Android output parent '{}': {error}",
                self.canonical_parent.display()
            ))
        })?;
        let identity = path_file_identity(&self.canonical_parent, &metadata).map_err(|error| {
            AndroidError::storage(format!(
                "failed to re-identify Android output parent '{}': {error}",
                self.canonical_parent.display()
            ))
        })?;
        if !metadata.file_type().is_dir() || identity != self.parent_identity {
            return Err(AndroidError::compatibility(format!(
                "Android output parent '{}' changed after validation",
                self.canonical_parent.display()
            )));
        }
        Ok(())
    }

    fn revalidate_target(&self) -> Result<bool, AndroidError> {
        self.revalidate_parent()?;
        match (self.initial_identity, fs::symlink_metadata(&self.path)) {
            (None, Err(error)) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            (None, Ok(_)) => Err(AndroidError::compatibility(format!(
                "Android generated directory '{}' appeared after validation",
                self.path.display()
            ))),
            (None, Err(error)) => Err(AndroidError::storage(format!(
                "failed to recheck Android generated directory '{}': {error}",
                self.path.display()
            ))),
            (Some(expected), Ok(metadata)) if metadata.file_type().is_dir() => {
                let identity = path_file_identity(&self.path, &metadata).map_err(|error| {
                    AndroidError::storage(format!(
                        "failed to re-identify Android generated directory '{}': {error}",
                        self.path.display()
                    ))
                })?;
                if identity == expected {
                    Ok(true)
                } else {
                    Err(AndroidError::compatibility(format!(
                        "Android generated directory '{}' changed after validation",
                        self.path.display()
                    )))
                }
            }
            (Some(_), Ok(_)) => Err(AndroidError::compatibility(format!(
                "Android generated directory '{}' changed after validation",
                self.path.display()
            ))),
            (Some(_), Err(error)) => Err(AndroidError::storage(format!(
                "failed to recheck Android generated directory '{}': {error}",
                self.path.display()
            ))),
        }
    }
}

fn promote_generated_directory(
    staging: tempfile::TempDir,
    target: &GeneratedDirectoryTarget,
) -> Result<(), AndroidError> {
    let source = GeneratedDirectorySource {
        path: staging.path().to_path_buf(),
        owner: staging,
    };
    let deferral = external_process::InterruptDeferral::start("Android output promotion")
        .map_err(|error| AndroidError::worker(error.to_string()))?;
    let promotion = begin_generated_directory_promotion(source, target.clone(), None)?;
    if deferral.is_cancelled() {
        promotion.rollback()?;
        let _ = deferral.finish();
        return Err(AndroidError::worker(
            "Android output promotion was cancelled",
        ));
    }
    // An interrupt after this commit point is consumed after the transaction completes.
    promotion.commit()?;
    let _ = deferral.finish();
    Ok(())
}

#[cfg(test)]
fn promote_generated_directory_with_hook(
    staging: tempfile::TempDir,
    target: &GeneratedDirectoryTarget,
    before_exchange: Option<crate::PathIoHook<'_>>,
) -> Result<(), AndroidError> {
    let source = GeneratedDirectorySource {
        path: staging.path().to_path_buf(),
        owner: staging,
    };
    begin_generated_directory_promotion(source, target.clone(), before_exchange)?.commit()
}

#[derive(Debug)]
struct GeneratedDirectorySource {
    owner: tempfile::TempDir,
    path: PathBuf,
}

#[derive(Debug)]
struct GeneratedDirectoryPromotion {
    source: GeneratedDirectorySource,
    target: GeneratedDirectoryTarget,
    promoted_identity: FileIdentity,
    had_previous: bool,
}

#[derive(Debug)]
struct GeneratedDirectoryTransaction {
    promotions: Vec<GeneratedDirectoryPromotion>,
}

impl GeneratedDirectoryTransaction {
    fn begin(stages: Vec<StagedGeneratedDirectory>) -> Result<Self, AndroidError> {
        let mut promotions = Vec::with_capacity(stages.len());
        for stage in stages {
            let target = stage.target.clone();
            match begin_generated_directory_promotion(stage.into_source(), target, None) {
                Ok(promotion) => promotions.push(promotion),
                Err(error) => {
                    return Err(rollback_promotions(promotions, error));
                }
            }
        }
        Ok(Self { promotions })
    }

    fn commit(self) -> Result<(), AndroidError> {
        let validation_error = self
            .promotions
            .iter()
            .find_map(|promotion| promotion.validate_promoted_target().err());
        if let Some(error) = validation_error {
            return Err(rollback_promotions(self.promotions, error));
        }
        for promotion in self.promotions {
            promotion.commit_without_validation();
        }
        Ok(())
    }

    fn rollback(self) -> Result<(), AndroidError> {
        rollback_all(self.promotions)
    }
}

impl GeneratedDirectoryPromotion {
    fn validate_promoted_target(&self) -> Result<(), AndroidError> {
        validate_promoted_target(&self.target, self.promoted_identity)
    }

    fn commit(self) -> Result<(), AndroidError> {
        if let Err(error) = self.validate_promoted_target() {
            return Err(if self.had_previous {
                preserve_previous_output(self.source, &self.target, error)
            } else {
                error
            });
        }
        self.commit_without_validation();
        Ok(())
    }

    fn commit_without_validation(self) {
        drop(self.source.owner);
    }

    fn rollback(self) -> Result<(), AndroidError> {
        let Self {
            source,
            target,
            promoted_identity,
            had_previous,
        } = self;
        if let Err(error) = validate_promoted_target(&target, promoted_identity) {
            return Err(if had_previous {
                preserve_previous_output(source, &target, error)
            } else {
                error
            });
        }
        if had_previous {
            if let Err(error) = exchange_directories(&source.path, &target.path) {
                return Err(preserve_previous_output(
                    source,
                    &target,
                    AndroidError::storage(format!(
                        "failed to roll back Android generated directory '{}': {error}",
                        target.path.display()
                    )),
                ));
            }
            let restored_identity = directory_identity(&target.path, "restored Android output");
            if restored_identity.as_ref().ok() != target.initial_identity.as_ref() {
                if exchange_directories(&source.path, &target.path).is_ok() {
                    return Err(preserve_previous_output(
                        source,
                        &target,
                        restored_identity.err().unwrap_or_else(|| {
                            AndroidError::compatibility(format!(
                                "restored Android output '{}' has an unexpected identity",
                                target.path.display()
                            ))
                        }),
                    ));
                }
                let preserved = source.owner.keep();
                return Err(AndroidError::storage(format!(
                    "restored Android output '{}' could not be validated or exchanged back; replacement output was preserved under '{}'",
                    target.path.display(),
                    preserved.display()
                )));
            }
        } else {
            rename_directory_noreplace(&target.path, &source.path).map_err(|error| {
                AndroidError::storage(format!(
                    "failed to remove newly promoted Android output '{}': {error}",
                    target.path.display()
                ))
            })?;
        }
        drop(source.owner);
        Ok(())
    }
}

fn validate_promoted_target(
    target: &GeneratedDirectoryTarget,
    promoted_identity: FileIdentity,
) -> Result<(), AndroidError> {
    target.revalidate_parent()?;
    let identity = directory_identity(&target.path, "Android promoted output")?;
    if identity == promoted_identity {
        Ok(())
    } else {
        Err(AndroidError::compatibility(format!(
            "Android promoted output '{}' changed before the transaction completed",
            target.path.display()
        )))
    }
}

fn promote_staged_directories(
    stages: Vec<StagedGeneratedDirectory>,
    cancellation: &external_process::InterruptDeferral,
) -> Result<(), AndroidError> {
    let transaction = GeneratedDirectoryTransaction::begin(stages)?;
    if cancellation.is_cancelled() {
        transaction.rollback()?;
        return Err(AndroidError::worker(
            "Android output promotion was cancelled",
        ));
    }
    // An interrupt after this commit point is consumed after the transaction completes.
    transaction.commit()
}

fn begin_generated_directory_promotion(
    source: GeneratedDirectorySource,
    target: GeneratedDirectoryTarget,
    mut before_exchange: Option<crate::PathIoHook<'_>>,
) -> Result<GeneratedDirectoryPromotion, AndroidError> {
    let source_identity = directory_identity(&source.path, "Android staging output")?;
    let had_previous = target.revalidate_target()?;
    if let Some(hook) = before_exchange.as_mut() {
        hook(&target.path).map_err(|error| AndroidError::storage(error.to_string()))?;
    }
    target.revalidate_parent()?;
    if had_previous {
        exchange_directories(&source.path, &target.path).map_err(|error| {
            AndroidError::storage(format!(
                "failed to atomically exchange Android generated directory '{}': {error}",
                target.path.display()
            ))
        })?;
        let previous_identity = directory_identity(&source.path, "previous Android output");
        let promoted_identity = directory_identity(&target.path, "promoted Android output");
        if previous_identity.as_ref().ok() != target.initial_identity.as_ref()
            || promoted_identity.as_ref().ok() != Some(&source_identity)
        {
            let validation_error = AndroidError::compatibility(format!(
                "Android generated directory '{}' changed during atomic promotion",
                target.path.display()
            ));
            return match exchange_directories(&source.path, &target.path) {
                Ok(()) => Err(validation_error),
                Err(rollback_error) => Err(preserve_previous_output(
                    source,
                    &target,
                    validation_error.with_suffix(format!(
                        "failed to reverse the atomic exchange: {rollback_error}"
                    )),
                )),
            };
        }
    } else {
        rename_directory_noreplace(&source.path, &target.path).map_err(|error| {
            AndroidError::storage(format!(
                "failed to atomically promote Android generated directory '{}': {error}",
                target.path.display()
            ))
        })?;
    }
    if let Err(error) = target.revalidate_parent() {
        return Err(if had_previous {
            preserve_previous_output(source, &target, error)
        } else {
            error
        });
    }
    let promoted_identity = match directory_identity(&target.path, "promoted Android output") {
        Ok(identity) => identity,
        Err(error) => {
            return Err(if had_previous {
                preserve_previous_output(source, &target, error)
            } else {
                error
            });
        }
    };
    if promoted_identity != source_identity {
        return Err(preserve_previous_output(
            source,
            &target,
            AndroidError::compatibility(format!(
                "Android generated directory '{}' changed after atomic promotion",
                target.path.display()
            )),
        ));
    }
    Ok(GeneratedDirectoryPromotion {
        source,
        target,
        promoted_identity,
        had_previous,
    })
}

fn rollback_promotions(
    promotions: Vec<GeneratedDirectoryPromotion>,
    error: AndroidError,
) -> AndroidError {
    match rollback_all(promotions) {
        Ok(()) => error,
        Err(rollback_error) => error.with_suffix(rollback_error),
    }
}

fn rollback_all(promotions: Vec<GeneratedDirectoryPromotion>) -> Result<(), AndroidError> {
    let mut rollback_errors = Vec::new();
    for promotion in promotions.into_iter().rev() {
        if let Err(error) = promotion.rollback() {
            rollback_errors.push(error.to_string());
        }
    }
    if rollback_errors.is_empty() {
        Ok(())
    } else {
        Err(AndroidError::storage(format!(
            "failed to roll back Android generated output transaction: {}",
            rollback_errors.join("; ")
        )))
    }
}

fn preserve_previous_output(
    source: GeneratedDirectorySource,
    target: &GeneratedDirectoryTarget,
    error: AndroidError,
) -> AndroidError {
    let parent_is_stable = target.revalidate_parent().is_ok();
    let preserved = source.owner.keep();
    if parent_is_stable {
        error.with_suffix(format!(
            "previous Android output was preserved under '{}'",
            preserved.display()
        ))
    } else {
        error.with_suffix(
            "previous Android output remains under the original output parent, whose current path cannot be determined",
        )
    }
}

fn directory_identity(path: &Path, label: &str) -> Result<FileIdentity, AndroidError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AndroidError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(AndroidError::compatibility(format!(
            "{label} '{}' is not a regular non-symlink directory",
            path.display()
        )));
    }
    path_file_identity(path, &metadata).map_err(|error| {
        AndroidError::storage(format!(
            "failed to identify {label} '{}': {error}",
            path.display()
        ))
    })
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
    exchange_directories_by_rename(left, right)
}

#[cfg(any(windows, test))]
fn exchange_directories_by_rename(left: &Path, right: &Path) -> io::Result<()> {
    let parent = right.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Android output directory has no parent",
        )
    })?;
    let backup_container = tempfile::Builder::new()
        .prefix(".vesper-android-exchange-")
        .tempdir_in(parent)?;
    let backup = backup_container.path().join("previous");
    fs::rename(right, &backup)?;

    if let Err(exchange_error) = fs::rename(left, right) {
        return match fs::rename(&backup, right) {
            Ok(()) => Err(exchange_error),
            Err(rollback_error) => {
                let preserved = backup_container.keep();
                Err(io::Error::other(format!(
                    "failed to exchange Android output: {exchange_error}; failed to restore the previous output: {rollback_error}; previous output was preserved under '{}'",
                    preserved.display()
                )))
            }
        };
    }

    if let Err(exchange_error) = fs::rename(&backup, left) {
        let restore_replacement = fs::rename(right, left);
        let restore_previous = fs::rename(&backup, right);
        if restore_replacement.is_ok() && restore_previous.is_ok() {
            return Err(exchange_error);
        }
        let preserved = backup_container.keep();
        return Err(io::Error::other(format!(
            "failed to finish exchanging Android output: {exchange_error}; replacement rollback: {}; previous-output rollback: {}; recovery data was preserved under '{}'",
            format_io_result(&restore_replacement),
            format_io_result(&restore_previous),
            preserved.display()
        )));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn format_io_result(result: &io::Result<()>) -> String {
    match result {
        Ok(()) => "ok".to_owned(),
        Err(error) => error.to_string(),
    }
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
        "atomic directory exchange is unsupported on this host",
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
        "atomic no-replace directory promotion is unsupported on this host",
    ))
}

fn map_gradle_error(error: gradle::GradleError) -> AndroidError {
    match error.kind() {
        gradle::GradleErrorKind::Storage => AndroidError::storage(error.to_string()),
        gradle::GradleErrorKind::Compatibility => AndroidError::compatibility(error.to_string()),
    }
}

#[cfg(unix)]
fn path_file_identity(_path: &Path, metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    Ok(FileIdentity {
        volume_or_device: metadata.dev(),
        file_index: metadata.ino(),
    })
}

#[cfg(windows)]
fn path_file_identity(path: &Path, _metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    let handle = winapi_util::Handle::from_path_any(path)?;
    let information = winapi_util::file::information(&handle)?;
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

fn output_error(error: io::Error) -> AndroidError {
    AndroidError::storage(format!("failed to write Android command output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    #[test]
    fn maven_distribution_includes_diagnostics_without_experimental_plugins() {
        let mut tasks = Vec::new();

        append_optional_distribution_tasks(&mut tasks, "publishRelease", false, true);

        assert!(
            tasks.contains(&":vesper-player-kit-performance-diagnostics:publishRelease".to_owned())
        );
        assert!(!tasks.iter().any(|task| task.contains("decoder-mediacodec")));
        assert!(
            !tasks
                .iter()
                .any(|task| task.contains("frame-processor-diagnostic"))
        );
    }

    #[cfg(vesper_source_checkout)]
    #[test]
    fn optional_android_generated_targets_preflight_in_clean_checkout() {
        let root = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."))
            .expect("canonical workspace root");
        for relative in [
            "lib/android/vesper-player-kit-decoder-mediacodec/src/main/jniLibs",
            "lib/android/vesper-player-kit-source-normalizer-ffmpeg/src/main/jniLibs",
            "lib/android/vesper-player-kit-source-normalizer-ffmpeg/src/main/assets/vesper-source-normalizer-ffmpeg",
            "lib/android/vesper-player-kit-frame-processor-diagnostic/src/main/jniLibs",
            "lib/android/vesper-player-kit-performance-diagnostics/src/main/jniLibs",
            "lib/android/vesper-player-kit-ffmpeg-runtime/src/main/jniLibs",
            "lib/android/vesper-player-kit-ffmpeg-runtime/src/main/assets/vesper-ffmpeg-runtime",
            "lib/android/vesper-player-kit-external-playback/src/main/jniLibs",
            "lib/android/vesper-player-kit-external-playback/src/main/assets/vesper-relay-ffmpeg",
        ] {
            GeneratedDirectoryTarget::preflight(&root, root.join(relative)).unwrap_or_else(
                |error| panic!("preflight optional Android target {relative}: {error}"),
            );
        }
    }

    fn write_android_archive(path: &Path, entries: &[(&str, &[u8], Option<u32>)]) {
        let file = File::create(path).expect("create Android archive fixture");
        let mut archive = ZipWriter::new(file);
        for (name, bytes, mode) in entries {
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Stored)
                .unix_permissions(mode.unwrap_or(0o100644));
            archive
                .start_file(*name, options)
                .expect("start Android archive entry");
            archive
                .write_all(bytes)
                .expect("write Android archive entry");
        }
        archive.finish().expect("finish Android archive fixture");
    }

    #[test]
    fn runtime_free_arguments_resolve_relative_output_and_profile() {
        let root = Path::new("/tmp/vesper-runtime-free-root");
        let request = parse_runtime_free_plugin_request(
            root,
            RuntimeFreePlugin::DecoderMediaCodec,
            &[OsString::from("plugin output"), OsString::from("release")],
        )
        .expect("parse runtime-free plugin arguments");
        assert_eq!(request.output_directory, root.join("plugin output"));
        assert!(request.release);

        let default_request = parse_runtime_free_plugin_request(
            root,
            RuntimeFreePlugin::FrameProcessorDiagnostic,
            &[OsString::from("plugin-output")],
        )
        .expect("parse default runtime-free plugin profile");
        assert!(!default_request.release);
    }

    #[test]
    fn release_archive_scan_rejects_unsafe_paths_and_runtime_in_feature_aars() {
        let directory = tempfile::tempdir().expect("temporary Android release archives");
        let valid = directory.path().join("valid.aar");
        write_android_archive(
            &valid,
            &[
                ("AndroidManifest.xml", b"manifest", None),
                (
                    "jni/arm64-v8a/libvesper_decoder_mediacodec.so",
                    b"plugin",
                    None,
                ),
            ],
        );
        let index = scan_android_release_archive(&valid, "arm64-v8a")
            .expect("validate bounded Android archive");
        assert!(index.runtime_libraries.is_empty());

        let traversal = directory.path().join("traversal.aar");
        write_android_archive(&traversal, &[("../escape", b"bad", None)]);
        assert!(
            scan_android_release_archive(&traversal, "arm64-v8a")
                .expect_err("reject traversing Android archive")
                .to_string()
                .contains("traversing")
        );

        let collision = directory.path().join("collision.aar");
        write_android_archive(
            &collision,
            &[
                ("Assets/Value", b"one", None),
                ("assets/value", b"two", None),
            ],
        );
        assert!(
            scan_android_release_archive(&collision, "arm64-v8a")
                .expect_err("reject case-colliding Android archive")
                .to_string()
                .contains("colliding")
        );

        let runtime = directory.path().join("runtime-in-plugin.aar");
        write_android_archive(
            &runtime,
            &[("jni/arm64-v8a/libavcodec.so", b"runtime", None)],
        );
        let index = scan_android_release_archive(&runtime, "arm64-v8a")
            .expect("scan runtime-carrying archive");
        assert!(reject_unexpected_runtime(&runtime, &index).is_err());

        let versioned_runtime = directory.path().join("versioned-runtime-in-plugin.aar");
        write_android_archive(
            &versioned_runtime,
            &[("jni/arm64-v8a/libavcodec.so.61", b"runtime", None)],
        );
        let index = scan_android_release_archive(&versioned_runtime, "arm64-v8a")
            .expect("scan versioned runtime-carrying archive");
        assert!(
            reject_unexpected_runtime(&versioned_runtime, &index).is_err(),
            "versioned FFmpeg runtime names must not bypass feature AAR isolation"
        );
    }

    #[test]
    fn sample_apk_scan_preserves_case_sensitive_aapt_resources() {
        let directory = tempfile::tempdir().expect("temporary Android sample APK");
        let apk = directory.path().join("sample.apk");
        write_android_archive(
            &apk,
            &[
                ("AndroidManifest.xml", b"manifest", None),
                ("res/-L.png", b"upper", None),
                ("res/-l.png", b"lower", None),
                ("lib/arm64-v8a/libvesper_player_android.so", b"host", None),
            ],
        );

        let index = scan_android_sample_apk(&apk, "arm64-v8a")
            .expect("accept case-sensitive aapt resource paths");

        assert!(index.files.contains("res/-L.png"));
        assert!(index.files.contains("res/-l.png"));
    }

    #[test]
    fn sample_registry_validation_rejects_legacy_or_extra_identities() {
        let archive = Path::new("sample.apk");
        let abi = "arm64-v8a";
        let mut registries = [
            "dev.vesper.frame-processor-diagnostic",
            "io.github.umbrella22.vesper.decoder-mediacodec",
            "io.github.umbrella22.vesper.remux-ffmpeg",
            "io.github.umbrella22.vesper.source-normalizer-ffmpeg",
        ]
        .into_iter()
        .map(|plugin_id| format!("assets/vesper/plugins/{abi}/{plugin_id}.json"))
        .collect::<BTreeSet<_>>();

        validate_android_sample_registry_entries(archive, abi, false, &registries)
            .expect("accept exact Compose registry set");
        registries.insert(format!(
            "assets/vesper/plugins/{abi}/io.github.ikaros.vesper.remux-ffmpeg.json"
        ));
        let error = validate_android_sample_registry_entries(archive, abi, false, &registries)
            .expect_err("reject legacy plugin registry");
        assert!(error.to_string().contains("unexpected="));
        assert!(error.to_string().contains("io.github.ikaros"));

        registries.retain(|entry| !entry.contains("decoder-mediacodec"));
        registries.retain(|entry| !entry.contains("io.github.ikaros"));
        validate_android_sample_registry_entries(archive, abi, true, &registries)
            .expect("accept exact Flutter registry set");
    }

    #[test]
    fn release_archive_scan_rejects_test_fixtures_and_foreign_abis() {
        let directory = tempfile::tempdir().expect("temporary Android release archives");
        let fixture = directory.path().join("fixture.aar");
        write_android_archive(
            &fixture,
            &[("assets/subtitle_contract/sample.srt", b"fixture", None)],
        );
        assert!(
            scan_android_release_archive(&fixture, "arm64-v8a")
                .expect_err("reject Android test fixture")
                .to_string()
                .contains("test fixture")
        );

        let foreign = directory.path().join("foreign.aar");
        write_android_archive(
            &foreign,
            &[("jni/x86_64/libvesper_player_android.so", b"foreign", None)],
        );
        assert!(
            scan_android_release_archive(&foreign, "arm64-v8a")
                .expect_err("reject foreign Android ABI")
                .to_string()
                .contains("unexpected JNI ABI")
        );
    }

    #[test]
    fn runtime_free_arguments_reject_unknown_or_trailing_values() {
        for arguments in [
            vec![OsString::from("output"), OsString::from("optimized")],
            vec![
                OsString::from("output"),
                OsString::from("release"),
                OsString::from("unexpected"),
            ],
        ] {
            let error = parse_runtime_free_plugin_request(
                Path::new("/tmp/root"),
                RuntimeFreePlugin::DecoderMediaCodec,
                &arguments,
            )
            .expect_err("reject invalid runtime-free arguments");
            assert_eq!(error.kind(), AndroidErrorKind::Usage);
            assert!(error.to_string().contains("unexpected Android plugin"));
        }

        let error = parse_runtime_free_plugin_request(
            Path::new("/tmp/root"),
            RuntimeFreePlugin::DecoderMediaCodec,
            &[],
        )
        .expect_err("reject a missing runtime-free output");
        assert_eq!(error.kind(), AndroidErrorKind::Usage);
        assert!(
            error
                .to_string()
                .contains("vesper android decoder-mediacodec-plugin")
        );
    }

    #[test]
    fn ffmpeg_plugin_arguments_preserve_profile_metadata_and_relative_paths() {
        let root = Path::new("/tmp/vesper-ffmpeg-plugin-root");
        let request = parse_ffmpeg_plugin_request(
            root,
            FfmpegPlugin::SourceNormalizer,
            &[
                OsString::from("plugin output"),
                OsString::from("release"),
                OsString::from("--profile"),
                OsString::from("default"),
                OsString::from("--metadata-dir"),
                OsString::from("metadata output"),
            ],
        )
        .expect("parse Android FFmpeg plugin arguments");
        assert_eq!(request.output_directory, root.join("plugin output"));
        assert_eq!(
            request.metadata_directory,
            Some(root.join("metadata output"))
        );
        assert!(request.release);
        assert_eq!(request.profile, "default");

        let remux = parse_ffmpeg_plugin_request(
            root,
            FfmpegPlugin::Remux,
            &[
                OsString::from("plugin-output"),
                OsString::from("--metadata-dir=remux-metadata"),
            ],
        )
        .expect("parse default Android remux profile");
        assert_eq!(remux.profile, "download-remux");
        assert_eq!(remux.metadata_directory, Some(root.join("remux-metadata")));
    }

    #[test]
    fn ffmpeg_plugin_arguments_reject_invalid_options_and_missing_values() {
        let root = Path::new("/tmp/vesper-ffmpeg-plugin-root");
        for (plugin, arguments) in [
            (
                FfmpegPlugin::Remux,
                vec![OsString::from("output"), OsString::from("--metadata-dir")],
            ),
            (
                FfmpegPlugin::SourceNormalizer,
                vec![OsString::from("output"), OsString::from("--unknown")],
            ),
            (
                FfmpegPlugin::SourceNormalizer,
                vec![OsString::from("output"), OsString::from("--profile")],
            ),
        ] {
            assert!(parse_ffmpeg_plugin_request(root, plugin, &arguments).is_err());
        }
    }

    #[test]
    fn ffmpeg_plugin_output_scan_rejects_runtime_libraries() {
        let directory = tempfile::tempdir().expect("temporary Android FFmpeg plugin output");
        let abi = directory.path().join("arm64-v8a");
        fs::create_dir_all(&abi).expect("create Android FFmpeg plugin ABI output");
        fs::write(abi.join("libvesper_remux_ffmpeg.so"), b"plugin")
            .expect("write Android FFmpeg plugin");
        validate_ffmpeg_plugin_output(
            directory.path(),
            FfmpegPlugin::Remux,
            &["arm64-v8a".to_owned()],
        )
        .expect("validate Android FFmpeg plugin output");

        fs::write(abi.join("libavformat.so"), b"runtime").expect("write forbidden runtime");
        assert!(
            validate_ffmpeg_plugin_output(
                directory.path(),
                FfmpegPlugin::Remux,
                &["arm64-v8a".to_owned()],
            )
            .expect_err("reject Android FFmpeg runtime library")
            .to_string()
            .contains("must not bundle FFmpeg runtime")
        );
    }

    #[test]
    fn ffmpeg_plugin_output_scan_rejects_versioned_libpostproc_runtime() {
        let directory = tempfile::tempdir().expect("temporary Android FFmpeg plugin output");
        let abi = directory.path().join("arm64-v8a");
        fs::create_dir_all(&abi).expect("create Android FFmpeg plugin ABI output");
        fs::write(abi.join("libvesper_remux_ffmpeg.so"), b"plugin")
            .expect("write Android FFmpeg plugin");
        fs::write(abi.join("libpostproc.so.58"), b"runtime")
            .expect("write versioned libpostproc runtime");

        let error = validate_ffmpeg_plugin_output(
            directory.path(),
            FfmpegPlugin::Remux,
            &["arm64-v8a".to_owned()],
        )
        .expect_err("reject versioned libpostproc runtime");
        assert!(error.to_string().contains("must not bundle FFmpeg runtime"));
        assert!(is_forbidden_android_runtime_library("libpostproc.so"));
        assert!(is_forbidden_android_runtime_library("libpostproc.so.58"));
    }

    #[test]
    fn ffmpeg_runtime_staging_ignores_pkgconfig_directories() {
        let directory = tempfile::tempdir().expect("temporary Android FFmpeg runtime");
        let runtime = directory.path().join("runtime/arm64-v8a/lib");
        fs::create_dir_all(runtime.join("pkgconfig"))
            .expect("create Android FFmpeg pkg-config directory");
        fs::write(
            runtime.join("pkgconfig/libavcodec.pc"),
            b"prefix=/fixture\n",
        )
        .expect("write Android FFmpeg pkg-config fixture");
        fs::write(runtime.join("libavcodec.so"), b"runtime")
            .expect("write Android FFmpeg runtime fixture");

        let output = directory.path().join("staged");
        stage_ffmpeg_runtime_libraries(
            &output,
            &directory.path().join("runtime"),
            &directory.path().join("openssl"),
            &directory.path().join("libxml2"),
            false,
            false,
            &["arm64-v8a".to_owned()],
        )
        .expect("stage runtime alongside pkg-config metadata");

        assert!(output.join("arm64-v8a/libavcodec.so").is_file());
        assert!(!output.join("arm64-v8a/pkgconfig").exists());
    }

    #[test]
    fn ffmpeg_runtime_metadata_drives_declared_dependency_staging() {
        let directory = tempfile::tempdir().expect("temporary Android FFmpeg runtime");
        let abi = "arm64-v8a";
        let ffmpeg = directory.path().join("ffmpeg");
        let runtime = ffmpeg.join(abi).join("lib");
        fs::create_dir_all(&runtime).expect("create Android FFmpeg runtime directory");
        fs::write(runtime.join("libavformat.so"), b"runtime")
            .expect("write Android FFmpeg runtime fixture");
        fs::write(
            ffmpeg.join(abi).join("vesper-ffmpeg-build-metadata.txt"),
            b"Vesper FFmpeg build metadata v2\nexternal_dependencies=libxml2\n",
        )
        .expect("write Android FFmpeg build metadata");
        let libxml2 = directory.path().join("libxml2").join(abi).join("lib");
        fs::create_dir_all(&libxml2).expect("create Android libxml2 runtime directory");
        fs::write(libxml2.join("libxml2.so"), b"libxml2")
            .expect("write Android libxml2 runtime fixture");
        let selected_abis = [abi.to_owned()];

        let uses_libxml2 =
            ffmpeg_runtime_uses_external_dependency(&ffmpeg, &selected_abis, "libxml2")
                .expect("read declared libxml2 dependency");
        assert!(uses_libxml2);
        assert!(
            !ffmpeg_runtime_uses_external_dependency(&ffmpeg, &selected_abis, "openssl")
                .expect("read absent OpenSSL dependency")
        );

        let output = directory.path().join("staged");
        stage_ffmpeg_runtime_libraries(
            &output,
            &ffmpeg,
            &directory.path().join("openssl"),
            &directory.path().join("libxml2"),
            false,
            uses_libxml2,
            &selected_abis,
        )
        .expect("stage declared Android FFmpeg dependencies");
        assert!(output.join(abi).join("libavformat.so").is_file());
        assert!(output.join(abi).join("libxml2.so").is_file());
    }

    #[test]
    fn ffmpeg_runtime_staging_rejects_a_missing_declared_dependency() {
        let directory = tempfile::tempdir().expect("temporary Android FFmpeg runtime");
        let abi = "arm64-v8a";
        let ffmpeg = directory.path().join("ffmpeg");
        let runtime = ffmpeg.join(abi).join("lib");
        fs::create_dir_all(&runtime).expect("create Android FFmpeg runtime directory");
        fs::write(runtime.join("libavformat.so"), b"runtime")
            .expect("write Android FFmpeg runtime fixture");
        fs::write(
            ffmpeg.join(abi).join("vesper-ffmpeg-build-metadata.txt"),
            b"Vesper FFmpeg build metadata v2\nexternal_dependencies=libxml2\n",
        )
        .expect("write Android FFmpeg build metadata");
        let selected_abis = [abi.to_owned()];

        let error = stage_ffmpeg_runtime_libraries(
            &directory.path().join("staged"),
            &ffmpeg,
            &directory.path().join("openssl"),
            &directory.path().join("missing-libxml2"),
            false,
            true,
            &selected_abis,
        )
        .expect_err("reject a missing metadata-declared Android dependency");
        assert!(
            error
                .to_string()
                .contains("declared Android runtime dependency directory")
        );
    }

    #[test]
    fn ffmpeg_runtime_dependency_metadata_rejects_cross_abi_drift() {
        let directory = tempfile::tempdir().expect("temporary Android FFmpeg metadata");
        for (abi, dependencies) in [("arm64-v8a", "libxml2"), ("x86_64", "")] {
            let abi_root = directory.path().join(abi);
            fs::create_dir_all(&abi_root).expect("create Android FFmpeg ABI metadata directory");
            fs::write(
                abi_root.join("vesper-ffmpeg-build-metadata.txt"),
                format!("Vesper FFmpeg build metadata v2\nexternal_dependencies={dependencies}\n"),
            )
            .expect("write Android FFmpeg ABI metadata");
        }

        let error = ffmpeg_runtime_uses_external_dependency(
            directory.path(),
            &["arm64-v8a".to_owned(), "x86_64".to_owned()],
            "libxml2",
        )
        .expect_err("reject cross-ABI Android FFmpeg dependency drift");
        assert!(error.to_string().contains("differs across selected ABIs"));
    }

    #[test]
    fn instrumentation_output_explicitly_allows_the_shared_ffmpeg_runtime() {
        let directory = tempfile::tempdir().expect("temporary Android instrumentation output");
        let abi = directory.path().join("arm64-v8a");
        fs::create_dir(&abi).expect("create Android instrumentation ABI directory");
        fs::write(abi.join("libavcodec.so"), b"runtime")
            .expect("write Android instrumentation FFmpeg runtime");

        assert!(
            validate_android_output_tree(directory.path(), "instrumentation JNI", false).is_ok()
        );
        assert!(validate_plugin_output_tree(directory.path(), "feature plugin").is_err());
    }

    #[test]
    fn android_abi_copy_rejects_destination_collisions() {
        let directory = tempfile::tempdir().expect("temporary Android ABI staging");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::create_dir_all(source.join("arm64-v8a")).expect("create source ABI directory");
        fs::create_dir_all(target.join("arm64-v8a")).expect("create target ABI directory");
        fs::write(
            source.join("arm64-v8a/libvesper_decoder_mediacodec.so"),
            b"runtime",
        )
        .expect("write source library");
        fs::write(
            target.join("arm64-v8a/libvesper_decoder_mediacodec.so"),
            b"decoder",
        )
        .expect("write existing target library");

        let error = copy_android_abi_files(
            &source,
            &target,
            &["arm64-v8a".to_owned()],
            "Android instrumentation output",
        )
        .expect_err("reject destination collision");
        assert_eq!(error.kind(), AndroidErrorKind::Conformance);
        assert_eq!(
            fs::read(target.join("arm64-v8a/libvesper_decoder_mediacodec.so"))
                .expect("read existing target library"),
            b"decoder"
        );
    }

    #[test]
    fn runtime_free_scan_rejects_only_forbidden_shared_runtime_names() {
        let directory = tempfile::tempdir().expect("temporary runtime-free plugin output");
        let nested = directory.path().join("arm64-v8a");
        fs::create_dir(&nested).expect("create runtime-free ABI directory");
        let missing = validate_runtime_free_plugin_output(
            directory.path(),
            RuntimeFreePlugin::DecoderMediaCodec,
            &["arm64-v8a".to_owned()],
        )
        .expect_err("reject missing runtime-free plugin artifact");
        assert_eq!(missing.kind(), AndroidErrorKind::Conformance);

        fs::write(nested.join("libvesper_decoder_mediacodec.so"), b"plugin")
            .expect("write runtime-free plugin");
        validate_runtime_free_plugin_output(
            directory.path(),
            RuntimeFreePlugin::DecoderMediaCodec,
            &["arm64-v8a".to_owned()],
        )
        .expect("scan runtime-free plugin output");

        let forbidden = nested.join("libavcodec.so");
        fs::write(&forbidden, b"unexpected runtime").expect("write forbidden runtime library");
        assert!(
            validate_runtime_free_plugin_output(
                directory.path(),
                RuntimeFreePlugin::DecoderMediaCodec,
                &["arm64-v8a".to_owned()],
            )
            .expect_err("reject forbidden runtime-free plugin output")
            .to_string()
            .contains(&forbidden.display().to_string())
        );
        assert!(is_forbidden_android_runtime_library("libavcodec.so.61"));
        assert!(!is_forbidden_android_runtime_library("libcrypto.a"));
    }

    #[test]
    fn external_generated_directory_target_promotes_beside_output() {
        let directory = tempfile::tempdir().expect("temporary external Android output");
        let output = directory.path().join("new-parent/plugin-output");
        let target = external_generated_directory_target(&output)
            .expect("preflight external Android output");
        let staging = tempfile::Builder::new()
            .prefix(".vesper-android-external-test-")
            .tempdir_in(&target.canonical_parent)
            .expect("create adjacent external Android staging directory");
        fs::write(staging.path().join("plugin.so"), b"plugin")
            .expect("write staged external Android plugin");

        promote_generated_directory(staging, &target)
            .expect("promote external Android plugin output");

        assert_eq!(
            fs::read(output.join("plugin.so")).expect("read promoted external Android plugin"),
            b"plugin"
        );
    }

    #[test]
    fn abi_selection_is_cli_then_environment_then_arm64_default() {
        assert_eq!(
            resolve_selected_abis_from(&["arm64-v8a".to_owned()], Some(OsStr::new("x86_64")),)
                .expect("CLI ABI"),
            ["arm64-v8a"]
        );
        assert!(
            resolve_selected_abis_from(&["x86_64".to_owned()], Some(OsStr::new("arm64-v8a")))
                .expect_err("reject unsupported ABI")
                .to_string()
                .contains("Supported ABIs: arm64-v8a")
        );
        assert_eq!(
            resolve_selected_abis_from(&[], Some(OsStr::new("arm64-v8a")))
                .expect("environment ABI"),
            ["arm64-v8a"]
        );
        assert_eq!(
            resolve_selected_abis_from(&[], None).expect("default ABI"),
            ["arm64-v8a"]
        );
    }

    #[test]
    fn ndk_directory_order_is_version_aware() {
        assert_eq!(
            compare_ndk_paths(Path::new("9.0.1"), Path::new("29.0.1")),
            Ordering::Less
        );
        assert_eq!(
            compare_ndk_paths(Path::new("29.0.2"), Path::new("29.0.10")),
            Ordering::Less
        );
    }

    #[test]
    fn android_build_lock_rejects_concurrent_operations_and_releases_on_drop() {
        let directory = tempfile::tempdir().expect("temporary Android build lock");
        let first = AndroidBuildLock::acquire(directory.path(), "aar")
            .expect("acquire first Android AAR build lock");
        assert!(
            directory
                .path()
                .join("lib/android/.gradle/vesper-build-locks/vesper-android-aar.lock")
                .is_file()
        );
        assert!(!directory.path().join(".gradle").exists());
        let error = AndroidBuildLock::acquire(directory.path(), "aar")
            .err()
            .expect("reject a concurrent Android AAR build lock");
        assert!(error.to_string().contains("already active"));
        drop(first);
        AndroidBuildLock::acquire(directory.path(), "aar")
            .expect("reacquire Android AAR build lock after release");
    }

    #[test]
    fn rename_exchange_fallback_swaps_existing_directories() {
        let directory = tempfile::tempdir().expect("temporary Android exchange fallback");
        let left = directory.path().join("staged");
        let right = directory.path().join("published");
        fs::create_dir(&left).expect("create staged Android directory");
        fs::create_dir(&right).expect("create published Android directory");
        fs::write(left.join("new.txt"), b"new").expect("write staged Android artifact");
        fs::write(right.join("old.txt"), b"old").expect("write published Android artifact");

        exchange_directories_by_rename(&left, &right)
            .expect("exchange existing Android directories with rename fallback");

        assert_eq!(
            fs::read(right.join("new.txt")).expect("read promoted Android artifact"),
            b"new"
        );
        assert_eq!(
            fs::read(left.join("old.txt")).expect("read previous Android artifact"),
            b"old"
        );
        assert_eq!(
            fs::read_dir(directory.path())
                .expect("inspect Android exchange fallback parent")
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(".vesper-"))
                .count(),
            0
        );
    }

    #[test]
    fn generated_directory_promotion_preserves_backup_when_parent_changes() {
        let directory = tempfile::tempdir().expect("temporary Android promotion boundary");
        let repository = directory.path().join("repository");
        let output_parent = repository.join("lib/android/vesper-player-kit/src/main");
        let output = output_parent.join("jniLibs");
        fs::create_dir_all(&output).expect("create previous Android JNI output");
        fs::write(output.join("previous.txt"), b"previous")
            .expect("write previous Android JNI output");
        let canonical_repository = repository
            .canonicalize()
            .expect("resolve Android promotion repository");
        let canonical_parent = canonical_repository.join("lib/android/vesper-player-kit/src/main");
        let canonical_output = canonical_parent.join("jniLibs");
        let target = GeneratedDirectoryTarget::preflight(&canonical_repository, canonical_output)
            .expect("preflight Android generated directory");
        let staging = tempfile::Builder::new()
            .prefix(".vesper-android-parent-hook-test-")
            .tempdir_in(&target.canonical_parent)
            .expect("create replacement Android JNI output");
        fs::write(staging.path().join("replacement.txt"), b"replacement")
            .expect("write replacement Android JNI output");
        let moved_parent =
            canonical_repository.join("lib/android/vesper-player-kit/src/moved-main");
        let mut replace_parent = |_path: &Path| -> io::Result<()> {
            fs::rename(&canonical_parent, &moved_parent)?;
            fs::create_dir(&canonical_parent)
        };

        let error =
            promote_generated_directory_with_hook(staging, &target, Some(&mut replace_parent))
                .expect_err("reject an Android output parent replaced after backup");

        assert!(error.to_string().contains("changed after validation"));
        assert!(
            fs::read_dir(&canonical_parent)
                .expect("inspect replacement Android output parent")
                .next()
                .is_none()
        );
        assert_eq!(
            fs::read(moved_parent.join("jniLibs/previous.txt"))
                .expect("read preserved Android JNI output"),
            b"previous"
        );
    }

    #[test]
    fn generated_directory_promotion_reverses_a_concurrent_target_replacement() {
        let directory = tempfile::tempdir().expect("temporary Android promotion boundary");
        let repository = directory.path().join("repository");
        let output_parent = repository.join("lib/android/vesper-player-kit/src/main");
        let output = output_parent.join("jniLibs");
        fs::create_dir_all(&output).expect("create previous Android JNI output");
        fs::write(output.join("previous.txt"), b"previous")
            .expect("write previous Android JNI output");
        let canonical_repository = repository
            .canonicalize()
            .expect("resolve Android promotion repository");
        let canonical_output =
            canonical_repository.join("lib/android/vesper-player-kit/src/main/jniLibs");
        let target = GeneratedDirectoryTarget::preflight(&canonical_repository, canonical_output)
            .expect("preflight Android generated directory");
        let staging = tempfile::Builder::new()
            .prefix(".vesper-android-appearance-test-")
            .tempdir_in(&target.canonical_parent)
            .expect("create replacement Android JNI output");
        fs::write(staging.path().join("replacement.txt"), b"replacement")
            .expect("write replacement Android JNI output");
        let displaced_original = target.canonical_parent.join("displaced-original");
        let mut replace_target = |path: &Path| {
            fs::rename(path, &displaced_original)?;
            fs::create_dir(path)?;
            fs::write(path.join("concurrent.txt"), b"concurrent")
        };

        let error =
            promote_generated_directory_with_hook(staging, &target, Some(&mut replace_target))
                .expect_err("reject an Android output replaced during promotion");

        assert!(
            error
                .to_string()
                .contains("changed during atomic promotion")
        );
        assert_eq!(
            fs::read(target.path.join("concurrent.txt")).expect("read concurrent Android output"),
            b"concurrent"
        );
        assert_eq!(
            fs::read(displaced_original.join("previous.txt"))
                .expect("read independently displaced Android output"),
            b"previous"
        );
    }

    #[test]
    fn generated_directory_transaction_rolls_back_prior_exchanges() {
        let directory = tempfile::tempdir().expect("temporary Android transaction boundary");
        let repository = directory.path().join("repository");
        let output_parent = repository.join("lib/android/optional");
        fs::create_dir_all(&output_parent).expect("create Android transaction output parent");
        let canonical_repository = repository
            .canonicalize()
            .expect("resolve Android transaction repository");
        let canonical_parent = canonical_repository.join("lib/android/optional");
        let mut stages = Vec::new();
        for index in 0..5 {
            let target = canonical_parent.join(format!("output-{index}"));
            fs::create_dir(&target).expect("create previous Android transaction output");
            fs::write(target.join("previous.txt"), format!("previous-{index}"))
                .expect("write previous Android transaction output");
            let stage = StagedGeneratedDirectory::new(
                &canonical_repository,
                target,
                ".vesper-android-transaction-test-",
                "Android transaction test output",
            )
            .expect("create Android transaction stage");
            fs::write(
                stage.path().join("replacement.txt"),
                format!("replacement-{index}"),
            )
            .expect("write Android transaction replacement");
            stages.push(stage);
        }
        let replaced_target = canonical_parent.join("output-2");
        fs::rename(
            &replaced_target,
            canonical_parent.join("displaced-output-2"),
        )
        .expect("displace second Android transaction output");
        fs::create_dir(&replaced_target).expect("replace second Android transaction output");
        fs::write(replaced_target.join("concurrent.txt"), b"concurrent")
            .expect("write concurrent Android transaction output");
        let error = GeneratedDirectoryTransaction::begin(stages)
            .expect_err("reject a stale target in a multi-directory transaction");

        assert!(error.to_string().contains("changed after validation"));
        assert_eq!(
            fs::read_to_string(canonical_parent.join("output-0/previous.txt"))
                .expect("read rolled-back first Android output"),
            "previous-0"
        );
        assert_eq!(
            fs::read(canonical_parent.join("output-2/concurrent.txt"))
                .expect("read concurrent middle Android output"),
            b"concurrent"
        );
        assert_eq!(
            fs::read_to_string(canonical_parent.join("output-1/previous.txt"))
                .expect("read rolled-back second Android output"),
            "previous-1"
        );
        for index in 3..5 {
            assert_eq!(
                fs::read_to_string(canonical_parent.join(format!("output-{index}/previous.txt")))
                    .expect("read untouched Android transaction output"),
                format!("previous-{index}")
            );
        }
    }
}
