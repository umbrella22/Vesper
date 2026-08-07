#![deny(unsafe_code)]

mod android;
mod android_subtitle;
mod boundary;
mod cli_error;
mod contract;
mod desktop;
mod external_process;
mod ffi;
mod ffmpeg;
mod ffmpeg_android;
mod ffmpeg_apple;
mod ffmpeg_source;
mod flutter;
mod gradle;
mod ios;
mod ios_core_release;
mod ios_ffi;
mod ios_kit;
mod ios_native_frame;
mod ios_optional_device;
mod ios_optional_release;
mod ios_plugin;
mod ios_plugin_release;
mod ios_release;
mod ios_subtitle;
mod media;
mod mobile;
mod plugin_build;
mod plugin_inspection;
mod plugin_scaffold;
mod plugin_scaffold_assets;
mod release;
mod release_notes;
mod source_archive;
mod subtitle;
mod worker_protocol;
mod worker_supervisor;

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use cli_error::{CliError, CliResult};
use player_cli::{
    EmbeddedRegistryFragment, EmbeddedRegistryTarget, PluginArtifactTransport, PluginDescriptor,
    PluginProjectManifest, PluginSigningKey, PluginTrustStore, build_signed_plugin_package,
    install_verified_plugin_package, list_installed_plugins, uninstall_plugin,
    verify_signed_plugin_package,
};
use player_plugin_wasm_host::MAX_WASM_PLUGIN_COMPONENT_BYTES;

use plugin_build::{
    PluginArtifactSelector, PluginBuildError, PluginBuildProfile, PluginBuildRequest,
    build_plugin_artifact, select_plugin_artifact,
};
use plugin_inspection::{
    PluginInspectionOperation, PluginInspectionOutcome, PluginInspectionReport, inspect_manifest,
    inspect_wasm_plugin,
};
use plugin_scaffold::{PluginScaffoldCapability, PluginScaffoldRequest, create_plugin_scaffold};
use worker_protocol::{
    PLUGIN_WORKER_START_GATE, PluginWorkerRequest, PluginWorkerResponse, read_worker_request,
    write_worker_response,
};
use worker_supervisor::supervise_native_worker;

type PathIoHook<'a> = &'a mut dyn FnMut(&Path) -> io::Result<()>;
#[cfg(target_os = "macos")]
type PathHook<'a> = &'a mut dyn FnMut(&Path);
#[cfg(target_os = "macos")]
type TempDirIoHook<'a> = &'a mut dyn FnMut(tempfile::TempDir) -> io::Result<()>;

const MAX_PLUGIN_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_PLUGIN_KEY_FILE_BYTES: usize = 64 * 1024;
const MAX_PLUGIN_TRUST_STORE_BYTES: usize = 1024 * 1024;

#[cfg(test)]
fn source_checkout_root() -> Option<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().ok()?;
    let root = manifest_dir.join("../../..");
    let workspace_member = root.join("crates/tools/player-cli").canonicalize().ok()?;
    (manifest_dir == workspace_member).then_some(root)
}

#[derive(Debug, Parser)]
#[command(
    name = "vesper",
    version,
    about = "Vesper Player SDK command-line tools"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Android(AndroidArgs),
    Ios(IosArgs),
    Ffmpeg(FfmpegArgs),
    Plugin(PluginArgs),
    Contract(ContractArgs),
    Ffi(FfiArgs),
    Desktop(DesktopArgs),
    Media(MediaArgs),
    Mobile(MobileArgs),
    Flutter(FlutterArgs),
    Release(ReleaseArgs),
    #[command(name = "__plugin-worker", hide = true)]
    PluginWorker(PluginWorkerArgs),
}

#[derive(Debug, Args)]
struct MediaArgs {
    /// Repository root. Defaults to VESPER_REPO_ROOT or the current directory.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: MediaCommand,
}

#[derive(Debug, Subcommand)]
enum MediaCommand {
    /// Generates the bounded local media fixtures used by SourceNormalizer smoke tests.
    #[command(name = "generate-source-normalizer-fixtures")]
    GenerateSourceNormalizerFixtures,
}

#[derive(Debug, Args)]
struct IosArgs {
    /// Repository root. Defaults to VESPER_REPO_ROOT or the current directory.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: IosCommand,
}

#[derive(Debug, Subcommand)]
enum IosCommand {
    /// Builds the Rust FFI archives and XCFramework consumed by the iOS host kit.
    Ffi(IosFfiArgs),
    /// Builds the FFmpeg-backed post-download remux plugin libraries.
    #[command(name = "remux-plugin")]
    RemuxPlugin(IosPluginBuildArgs),
    /// Builds the FFmpeg-backed source normalizer plugin libraries.
    #[command(name = "source-normalizer-plugin")]
    SourceNormalizerPlugin(IosPluginBuildArgs),
    /// Builds the diagnostic frame processor plugin libraries.
    #[command(name = "frame-processor-plugin")]
    FrameProcessorPlugin(IosPluginBuildArgs),
    /// Builds the internal VideoToolbox decoder plugin libraries.
    #[command(name = "decoder-videotoolbox-plugin", hide = true)]
    DecoderVideoToolboxPlugin(IosPluginBuildArgs),
    /// Builds and stages the FFmpeg-backed remux plugin XCFramework release.
    #[command(name = "stage-remux-plugin-release")]
    StageRemuxPluginRelease(IosPluginReleaseArgs),
    /// Builds and stages the FFmpeg-backed source normalizer XCFramework release.
    #[command(name = "stage-source-normalizer-plugin-release")]
    StageSourceNormalizerPluginRelease(IosPluginReleaseArgs),
    /// Builds and stages the diagnostic frame processor XCFramework release.
    #[command(name = "stage-frame-processor-plugin-release")]
    StageFrameProcessorPluginRelease(IosPluginReleaseArgs),
    /// Builds and stages the internal VideoToolbox decoder XCFramework release.
    #[command(name = "stage-decoder-videotoolbox-plugin-release", hide = true)]
    StageDecoderVideoToolboxPluginRelease(IosPluginReleaseArgs),
    /// Builds the complete VesperPlayerKit device and Simulator XCFramework output.
    KitXcframework,
    /// Regenerates the checked-in Swift-to-Rust bridge shim as one transaction.
    SyncBridgeShim,
    /// Verifies generated sources, C syntax, and available Rust archive exports.
    VerifyBridgeShim(IosVerifyBridgeShimArgs),
    /// Verifies the optional-plugin layout embedded in an App Store app bundle.
    VerifyAppStoreLayout(IosVerifyAppStoreLayoutArgs),
    /// Runs the experimental iOS native-frame Swift smoke verification.
    VerifyNativeFrame(IosVerifyNativeFrameArgs),
    /// Verifies optional plugin XCFramework and FFmpeg compliance release assets.
    VerifyOptionalPluginsRelease(IosVerifyOptionalPluginsReleaseArgs),
    /// Runs the Release optional-plugin acceptance suite on a physical iOS device.
    VerifyOptionalPluginsDevice(IosVerifyOptionalPluginsDeviceArgs),
    /// Verifies core or complete iOS release archives.
    VerifyRelease(IosVerifyReleaseArgs),
    /// Builds and stages VesperPlayerKit release archives.
    StageRelease(IosStageReleaseArgs),
    /// Stages the iOS FFmpeg runtime release artifacts.
    #[command(name = "ffmpeg-runtime-release")]
    FfmpegRuntimeRelease(IosWorkerArgs),
    /// Stages the complete optional-plugin release bundle.
    #[command(name = "stage-optional-plugins-release")]
    StageOptionalPluginsRelease(IosWorkerArgs),
    /// Verifies iOS subtitle behavior.
    #[command(name = "verify-subtitles")]
    VerifySubtitles(IosVerifySubtitlesArgs),
}

#[derive(Debug, Args)]
struct IosFfiArgs {
    /// Cargo build profile for the Rust FFI archives.
    #[arg(value_enum, default_value = "release")]
    profile: IosFfiProfileArg,
    /// Builds one platform slice instead of the complete XCFramework.
    #[arg(long, value_enum)]
    platform: Option<IosFfiPlatformArg>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum IosFfiPlatformArg {
    Device,
    Simulator,
}

impl From<IosFfiPlatformArg> for ios_ffi::IosFfiPlatform {
    fn from(value: IosFfiPlatformArg) -> Self {
        match value {
            IosFfiPlatformArg::Device => Self::Device,
            IosFfiPlatformArg::Simulator => Self::Simulator,
        }
    }
}

#[derive(Debug, Args)]
struct IosPluginBuildArgs {
    /// Output directory for raw device and Simulator plugin libraries.
    output_directory: PathBuf,
    /// Legacy-compatible Cargo profile, FFmpeg options, and slice tokens.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    arguments: Vec<OsString>,
}

#[derive(Debug, Args)]
struct IosPluginReleaseArgs {
    /// Legacy-compatible output directory, profile options, dry-run flag, and slice tokens.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    arguments: Vec<OsString>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum IosFfiProfileArg {
    Debug,
    Release,
}

impl From<IosFfiProfileArg> for ios_ffi::IosFfiProfile {
    fn from(value: IosFfiProfileArg) -> Self {
        match value {
            IosFfiProfileArg::Debug => Self::Debug,
            IosFfiProfileArg::Release => Self::Release,
        }
    }
}

#[derive(Debug, Args)]
struct IosVerifyBridgeShimArgs {
    /// Rust FFI archive to verify instead of discovering checked-in iOS artifacts.
    #[arg(long)]
    archive: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct IosVerifyAppStoreLayoutArgs {
    /// Built iOS application bundle to verify.
    app_path: PathBuf,
    /// Verifies every optional framework and the containing application signature.
    #[arg(long)]
    verify_signatures: bool,
}

#[derive(Debug, Args)]
struct IosVerifyNativeFrameArgs {
    /// Legacy-compatible profile and smoke-mode tokens.
    #[arg(value_enum)]
    tokens: Vec<IosVerifyNativeFrameTokenArg>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum IosVerifyNativeFrameTokenArg {
    Debug,
    Release,
    SwiftSmoke,
}

impl IosVerifyNativeFrameTokenArg {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
            Self::SwiftSmoke => "swift-smoke",
        }
    }
}

#[derive(Debug, Args)]
struct IosVerifyOptionalPluginsReleaseArgs {
    /// Release directory. Defaults to dist/release/ios under the repository root.
    release_directory: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct IosVerifyOptionalPluginsDeviceArgs {
    /// Release directory whose verified optional-plugin archives are tested.
    release_directory: PathBuf,
    /// Physical iOS device UDID used by xcodebuild.
    #[arg(long)]
    device: String,
    /// Apple Development Team identifier used for automatic code signing.
    #[arg(long)]
    development_team: String,
    /// New directory that receives DerivedData and the XCResult bundle.
    #[arg(long)]
    output_directory: PathBuf,
    /// Allows Xcode to update provisioning profiles for the connected device.
    #[arg(long)]
    allow_provisioning_updates: bool,
}

#[derive(Debug, Args)]
struct IosVerifyReleaseArgs {
    /// Release directory. Defaults to dist/release/ios under the repository root.
    release_directory: Option<PathBuf>,
    /// Release artifact set to verify.
    #[arg(long, value_enum, default_value = "core")]
    scope: IosReleaseScopeArg,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum IosReleaseScopeArg {
    Core,
    Complete,
}

#[derive(Debug, Args)]
struct IosStageReleaseArgs {
    /// Release output directory. Defaults to dist/release/ios under the repository root.
    output_directory: Option<PathBuf>,
    /// Includes optional plugin XCFrameworks and FFmpeg compliance assets.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "true",
        value_name = "BOOL"
    )]
    include_optional_plugins: Option<bool>,
    /// Local Swift package Artifacts directory for optional XCFrameworks.
    #[arg(long)]
    package_artifacts_directory: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct IosWorkerArgs {
    /// Arguments forwarded to the platform worker.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    arguments: Vec<OsString>,
}

#[derive(Debug, Args)]
struct IosVerifySubtitlesArgs {
    /// Verification scope. Device and complete scopes require --device.
    #[arg(long, value_enum, default_value = "regression")]
    scope: IosSubtitleScopeArg,
    /// Physical iOS device identifier used by device and complete scopes.
    #[arg(long)]
    device: Option<String>,
    /// iOS Simulator identifier. Regression scopes auto-select when omitted.
    #[arg(long)]
    simulator: Option<String>,
    /// New evidence directory. Defaults under devnotes/evidence/subtitle/ios.
    #[arg(long)]
    evidence_dir: Option<PathBuf>,
    /// Apple development Team ID. Overrides VESPER_IOS_DEVELOPMENT_TEAM.
    #[arg(long)]
    development_team: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum IosSubtitleScopeArg {
    Regression,
    Device,
    Complete,
}

impl From<IosSubtitleScopeArg> for subtitle::SubtitleScope {
    fn from(value: IosSubtitleScopeArg) -> Self {
        match value {
            IosSubtitleScopeArg::Regression => Self::Regression,
            IosSubtitleScopeArg::Device => Self::Device,
            IosSubtitleScopeArg::Complete => Self::Complete,
        }
    }
}

#[derive(Debug, Args)]
struct AndroidArgs {
    /// Repository root. Defaults to VESPER_REPO_ROOT or the current directory.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: AndroidCommand,
}

#[derive(Debug, Subcommand)]
enum AndroidCommand {
    /// Builds Rust JNI libraries for the Android host kit.
    Jni(AndroidJniArgs),
    /// Builds Android host-kit AAR modules with cached Gradle.
    Aar(AndroidAarArgs),
    /// Builds the Android FFmpeg-backed post-download remux plugin.
    #[command(name = "remux-plugin")]
    RemuxPlugin(AndroidWorkerArgs),
    /// Builds the Android FFmpeg-backed SourceNormalizer plugin.
    #[command(name = "source-normalizer-plugin")]
    SourceNormalizerPlugin(AndroidWorkerArgs),
    /// Builds the Android MediaCodec decoder plugin.
    #[command(name = "decoder-mediacodec-plugin")]
    DecoderMediacodecPlugin(AndroidWorkerArgs),
    /// Builds the Android diagnostic FrameProcessor plugin.
    #[command(name = "frame-processor-plugin")]
    FrameProcessorPlugin(AndroidWorkerArgs),
    /// Stages Android host-kit release artifacts.
    #[command(name = "stage-release")]
    StageRelease(AndroidStageReleaseArgs),
    /// Stages Android sample APKs.
    #[command(name = "sample-apks")]
    SampleApks(AndroidSampleApksArgs),
    /// Provisions the Android instrumentation JNI fixture through Rust.
    #[command(name = "provision-test-jni", hide = true)]
    ProvisionTestJni(AndroidProvisionTestJniArgs),
    /// Builds the optional external-playback relay JNI library through Rust.
    #[command(name = "external-playback-jni", hide = true)]
    ExternalPlaybackJni(AndroidExternalPlaybackJniArgs),
    /// Verifies Android subtitle behavior.
    #[command(name = "verify-subtitles")]
    VerifySubtitles(AndroidVerifySubtitlesArgs),
    /// Internal Rust worker used by the temporary Android shell compatibility shims.
    #[command(name = "__runtime-free-plugin", hide = true)]
    RuntimeFreePlugin(AndroidRuntimeFreePluginArgs),
    /// Internal Rust worker used by the temporary Android FFmpeg plugin shims.
    #[command(name = "__ffmpeg-plugin", hide = true)]
    FfmpegPlugin(AndroidFfmpegPluginArgs),
}

#[derive(Debug, Args)]
struct AndroidJniArgs {
    /// Build profile. The legacy contract treats values other than `release` as debug.
    profile: Option<String>,
    /// Android ABIs. Defaults to RUST_ANDROID_ABIS, then arm64-v8a.
    abis: Vec<String>,
}

#[derive(Debug, Args)]
struct AndroidAarArgs {
    /// AAR-producing Gradle module task. Defaults to assembleRelease.
    #[arg(value_parser = parse_android_aar_task)]
    module_task: Option<String>,
    /// Includes optional Android plugin modules. The environment is used when omitted.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "true",
        value_name = "BOOL"
    )]
    include_optional_plugins: Option<bool>,
}

#[derive(Debug, Args)]
struct AndroidStageReleaseArgs {
    /// Output directory. Defaults to dist/release/android.
    output_directory: Option<PathBuf>,
    /// Android ABIs. Defaults to RUST_ANDROID_ABIS, then arm64-v8a.
    abis: Vec<String>,
    /// Includes optional Android plugin AARs. The environment is used when omitted.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "true",
        value_name = "BOOL"
    )]
    include_optional_plugins: Option<bool>,
}

#[derive(Debug, Args)]
struct AndroidSampleApksArgs {
    /// Output directory. Defaults to dist/release/android-samples.
    output_directory: Option<PathBuf>,
    /// Android ABIs. Defaults to RUST_ANDROID_ABIS, then arm64-v8a.
    abis: Vec<String>,
}

#[derive(Debug, Args)]
struct AndroidProvisionTestJniArgs {
    /// Android instrumentation JNI output directory.
    output_directory: PathBuf,
    /// Native build profile. Defaults to debug.
    #[arg(long, default_value = "debug")]
    profile: String,
    /// FFmpeg profile used by the SourceNormalizer fixture.
    #[arg(long, default_value = "default")]
    ffmpeg_profile: String,
}

#[derive(Debug, Args)]
struct AndroidExternalPlaybackJniArgs {
    /// Relay JNI output directory.
    output_directory: PathBuf,
    /// Relay metadata asset root.
    #[arg(long)]
    assets_directory: PathBuf,
    /// Native build profile. Defaults to debug.
    #[arg(long, default_value = "debug")]
    profile: String,
    /// FFmpeg profile used by the relay.
    #[arg(long, default_value = "default")]
    ffmpeg_profile: String,
    /// Reuse existing shared FFmpeg runtime artifacts instead of rebuilding them.
    #[arg(long)]
    skip_ffmpeg_runtime: bool,
}

#[derive(Debug, Args)]
struct AndroidVerifySubtitlesArgs {
    /// Verification scope. Device and complete scopes require --device.
    #[arg(long, value_enum, default_value = "regression")]
    scope: AndroidSubtitleScopeArg,
    /// Physical Android device serial used by device and complete scopes.
    #[arg(long)]
    device: Option<String>,
    /// New evidence directory. Defaults under devnotes/evidence/subtitle/android.
    #[arg(long)]
    evidence_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AndroidSubtitleScopeArg {
    Regression,
    Device,
    Complete,
}

impl From<AndroidSubtitleScopeArg> for subtitle::SubtitleScope {
    fn from(value: AndroidSubtitleScopeArg) -> Self {
        match value {
            AndroidSubtitleScopeArg::Regression => Self::Regression,
            AndroidSubtitleScopeArg::Device => Self::Device,
            AndroidSubtitleScopeArg::Complete => Self::Complete,
        }
    }
}

#[derive(Debug, Args)]
struct AndroidWorkerArgs {
    /// Arguments forwarded to the platform worker.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    arguments: Vec<OsString>,
}

#[derive(Debug, Args)]
struct AndroidRuntimeFreePluginArgs {
    /// Internal plugin identifier.
    plugin: String,
    /// Legacy-compatible output directory and build profile.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    arguments: Vec<OsString>,
}

#[derive(Debug, Args)]
struct AndroidFfmpegPluginArgs {
    /// Internal plugin identifier.
    plugin: String,
    /// Legacy-compatible output directory, profile, and metadata options.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    arguments: Vec<OsString>,
}

#[derive(Debug, Args)]
struct FfmpegArgs {
    /// Repository root. Defaults to VESPER_REPO_ROOT or the current directory.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    /// Declared FFmpeg profile. Defaults to platform environment, then `default`.
    #[arg(long)]
    profile: Option<String>,
    /// Build platform. Required unless --list-profiles is used.
    #[arg(long, value_enum, required_unless_present = "list_profiles")]
    platform: Option<FfmpegPlatformArg>,
    /// Lists declared profiles without resolving or building them.
    #[arg(long)]
    list_profiles: bool,
    /// Prints the resolved profile and worker arguments without building.
    #[arg(long)]
    dry_run: bool,
    /// Validates existing artifacts without building.
    #[arg(long)]
    verify_only: bool,
    /// Overrides the FFmpeg prebuilt output directory.
    #[arg(long)]
    output_dir: Option<PathBuf>,
    /// Android artifact to build.
    #[arg(long, value_enum, default_value = "runtime-aar")]
    android_artifact: FfmpegAndroidArtifactArg,
    /// Android ABI, repeatable or comma-separated.
    #[arg(long, action = clap::ArgAction::Append)]
    abi: Vec<String>,
    /// iOS slice, repeatable or comma-separated.
    #[arg(long, action = clap::ArgAction::Append)]
    slice: Vec<String>,
    /// Adds FFmpeg libraries to the resolved profile.
    #[arg(long, alias = "enable-libraries", action = clap::ArgAction::Append)]
    extra_libraries: Vec<String>,
    /// Adds FFmpeg demuxers to the resolved profile.
    #[arg(long, alias = "enable-demuxers", action = clap::ArgAction::Append)]
    extra_demuxers: Vec<String>,
    /// Adds FFmpeg muxers to the resolved profile.
    #[arg(long, alias = "enable-muxers", action = clap::ArgAction::Append)]
    extra_muxers: Vec<String>,
    /// Adds FFmpeg protocols to the resolved profile.
    #[arg(long, alias = "enable-protocols", action = clap::ArgAction::Append)]
    extra_protocols: Vec<String>,
    /// Adds FFmpeg decoders to the resolved profile.
    #[arg(long, alias = "enable-decoders", action = clap::ArgAction::Append)]
    extra_decoders: Vec<String>,
    /// Adds FFmpeg parsers to the resolved profile.
    #[arg(long, alias = "enable-parsers", action = clap::ArgAction::Append)]
    extra_parsers: Vec<String>,
    /// Adds FFmpeg bitstream filters to the resolved profile.
    #[arg(long, alias = "enable-bsfs", action = clap::ArgAction::Append)]
    extra_bsfs: Vec<String>,
    /// Adds a raw FFmpeg configure argument.
    #[arg(
        long,
        action = clap::ArgAction::Append,
        allow_hyphen_values = true
    )]
    extra_configure_arg: Vec<String>,
    /// Overrides the resolved TLS backend.
    #[arg(long)]
    tls_backend: Option<String>,
    /// Rebuilds even when metadata matches.
    #[arg(long)]
    force: bool,
    /// Acknowledges GPL or nonfree FFmpeg configure flags.
    #[arg(long)]
    acknowledge_gpl_nonfree: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FfmpegPlatformArg {
    Android,
    Ios,
    All,
}

impl From<FfmpegPlatformArg> for ffmpeg::FfmpegPlatform {
    fn from(value: FfmpegPlatformArg) -> Self {
        match value {
            FfmpegPlatformArg::Android => Self::Android,
            FfmpegPlatformArg::Ios => Self::Ios,
            FfmpegPlatformArg::All => Self::All,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FfmpegAndroidArtifactArg {
    RuntimeAar,
    Prebuilts,
}

impl From<FfmpegAndroidArtifactArg> for ffmpeg::AndroidArtifact {
    fn from(value: FfmpegAndroidArtifactArg) -> Self {
        match value {
            FfmpegAndroidArtifactArg::RuntimeAar => Self::RuntimeAar,
            FfmpegAndroidArtifactArg::Prebuilts => Self::Prebuilts,
        }
    }
}

fn parse_android_aar_task(value: &str) -> Result<String, String> {
    let is_test_variant = |variant: &str| {
        ["AndroidTest", "UnitTest", "TestFixtures"]
            .iter()
            .any(|suffix| variant.ends_with(suffix))
    };
    let is_assemble = value
        .strip_prefix("assemble")
        .is_some_and(|variant| !is_test_variant(variant));
    let is_bundle_aar = value
        .strip_prefix("bundle")
        .and_then(|value| value.strip_suffix("Aar"))
        .is_some_and(|variant| !variant.is_empty() && !is_test_variant(variant));
    if is_assemble || is_bundle_aar {
        Ok(value.to_owned())
    } else {
        Err(format!(
            "'{value}' is not an AAR-producing Gradle task; expected assemble*, or bundle*Aar"
        ))
    }
}

#[derive(Debug, Args)]
struct DesktopArgs {
    /// Repository root. Defaults to VESPER_REPO_ROOT or the current directory.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: DesktopCommand,
}

#[derive(Debug, Subcommand)]
enum DesktopCommand {
    /// Installs the repository-local desktop FFmpeg fallback when needed.
    #[command(name = "ensure-ffmpeg")]
    EnsureFfmpeg,
    /// Verifies the FFmpeg-backed post-download remux plugin.
    #[command(name = "verify-remux")]
    VerifyRemux(DesktopVerifyRemuxArgs),
    /// Verifies the decoder fixture plugin and macOS runtime diagnostics.
    #[command(name = "verify-decoder-diagnostics")]
    VerifyDecoderDiagnostics(DesktopVerifyRemuxArgs),
    /// Verifies the Windows D3D11 decoder plugin.
    #[command(name = "verify-decoder-d3d11")]
    VerifyDecoderD3d11(DesktopVerifyRemuxArgs),
    /// Verifies the macOS VideoToolbox decoder and native-frame playback path.
    #[command(name = "verify-decoder-videotoolbox")]
    VerifyDecoderVideoToolbox(DesktopVerifyRemuxArgs),
}

#[derive(Debug, Args)]
struct DesktopVerifyRemuxArgs {
    /// Positional profile and mode tokens accepted by the selected verification command.
    #[arg(value_name = "PROFILE_OR_MODE")]
    tokens: Vec<String>,
}

#[derive(Debug, Args)]
struct MobileArgs {
    /// Repository root. Defaults to VESPER_REPO_ROOT or the current directory.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: MobileCommand,
}

#[derive(Debug, Subcommand)]
enum MobileCommand {
    /// Verifies that default Android and iOS host artifacts exclude FFmpeg payloads.
    #[command(name = "verify-no-remux")]
    VerifyNoRemux(MobileVerifyNoRemuxArgs),
    /// Verifies Rust and mobile distribution binary library naming contracts.
    #[command(name = "verify-binary-names")]
    VerifyBinaryNames,
}

#[derive(Debug, Args)]
struct MobileVerifyNoRemuxArgs {
    /// Host artifact set to build and inspect.
    #[arg(value_enum, default_value = "all")]
    mode: MobileVerifyNoRemuxMode,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum MobileVerifyNoRemuxMode {
    Android,
    Ios,
    All,
}

impl From<MobileVerifyNoRemuxMode> for mobile::NoRemuxMode {
    fn from(value: MobileVerifyNoRemuxMode) -> Self {
        match value {
            MobileVerifyNoRemuxMode::Android => Self::Android,
            MobileVerifyNoRemuxMode::Ios => Self::Ios,
            MobileVerifyNoRemuxMode::All => Self::All,
        }
    }
}

#[derive(Debug, Args)]
struct FlutterArgs {
    /// Repository root. Defaults to VESPER_REPO_ROOT or the current directory.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: FlutterCommand,
}

#[derive(Debug, Subcommand)]
enum FlutterCommand {
    /// Stages publishable Flutter packages with release dependency metadata.
    #[command(name = "stage-pub")]
    StagePub(FlutterPubArgs),
    /// Runs `flutter pub publish --dry-run` for staged packages.
    #[command(name = "pub-dry-run")]
    PubDryRun(FlutterPubArgs),
    /// Publishes staged packages to pub.dev.
    #[command(name = "pub-publish")]
    PubPublish(FlutterPubArgs),
    /// Writes local path dependency overrides for the Flutter workspace.
    #[command(name = "local-overrides")]
    LocalOverrides(FlutterLocalOverridesArgs),
    /// Compiles the Android implementations of the Flutter plugins.
    #[command(name = "verify-android-plugin")]
    VerifyAndroidPlugin(FlutterLocalOverridesArgs),
    /// Verifies that a release APK contains arm64 Flutter AOT code and no test fixtures.
    #[command(name = "verify-android-release")]
    VerifyAndroidRelease(FlutterVerifyAndroidReleaseArgs),
}

#[derive(Debug, Args)]
struct FlutterVerifyAndroidReleaseArgs {
    /// Flutter Android release APK to inspect without extracting it.
    apk: PathBuf,
}

#[derive(Debug, Args)]
struct FlutterLocalOverridesArgs {
    /// Includes optional Flutter plugin packages. The environment is used when omitted.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "true",
        value_name = "BOOL"
    )]
    include_optional_plugins: Option<bool>,
}

#[derive(Debug, Args)]
struct FlutterPubArgs {
    /// Staging directory. Defaults to dist/release/flutter-pub under the repository root.
    output_directory: Option<PathBuf>,
    /// Package version. Defaults to the vesper_player pubspec version.
    version: Option<String>,
    /// Includes optional Flutter plugin packages. The environment is used when omitted.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "true",
        value_name = "BOOL"
    )]
    include_optional_plugins: Option<bool>,
}

#[derive(Debug, Args)]
struct FfiArgs {
    /// Repository root. Defaults to VESPER_REPO_ROOT or the current directory.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: FfiCommand,
}

#[derive(Debug, Subcommand)]
enum FfiCommand {
    /// Generates and atomically replaces the checked-in C header.
    Generate,
    /// Updates the checked-in C header only when generated content differs.
    Sync,
    /// Verifies that the checked-in C header matches cbindgen output.
    Verify,
    /// Builds and optionally runs the plain C host smoke example.
    #[command(name = "c-host-smoke")]
    CHostSmoke(FfiCHostSmokeArgs),
}

#[derive(Debug, Args)]
struct FfiCHostSmokeArgs {
    /// Media source passed to the C host. Defaults to the bundled smoke fixture.
    source: Option<PathBuf>,
    /// Builds the C host without running it.
    #[arg(long)]
    build_only: bool,
}

#[derive(Debug, Args)]
struct ReleaseArgs {
    /// Repository root. Defaults to VESPER_REPO_ROOT or the current directory.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: ReleaseCommand,
}

#[derive(Debug, Subcommand)]
enum ReleaseCommand {
    /// Generates bilingual GitHub release notes from a verified Git tag.
    Notes(ReleaseNotesArgs),
    /// Atomically updates product version metadata and changelog headings.
    SetVersion(ReleaseSetVersionArgs),
    /// Resolves a tag, updates metadata, verifies it, and emits CI values.
    PrepareFromTag(ReleaseTagArgs),
    /// Resolves a tag and emits CI values without changing repository files.
    MetadataFromTag(ReleaseTagArgs),
    /// Verifies all product metadata against one numeric version.
    VerifyVersion(ReleaseVerifyVersionArgs),
    /// Verifies product metadata against the current workspace version.
    VerifyCurrent,
}

#[derive(Debug, Args)]
struct ReleaseNotesArgs {
    /// Git tag. Defaults to GITHUB_REF_NAME.
    tag: Option<String>,
    /// Release notes output path. Defaults to dist/release/RELEASE_NOTES.md.
    output: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ReleaseSetVersionArgs {
    /// Numeric major.minor.patch product version.
    version: String,
    #[command(flatten)]
    metadata: ReleaseMetadataArgs,
}

#[derive(Debug, Args)]
struct ReleaseTagArgs {
    /// Release tag, including an optional v prefix and prerelease suffix.
    tag: String,
    #[command(flatten)]
    metadata: ReleaseMetadataArgs,
}

#[derive(Debug, Args)]
struct ReleaseMetadataArgs {
    /// Numeric iOS CFBundleVersion. Defaults to release environment or versionCode.
    #[arg(long)]
    ios_build: Option<String>,
    /// Numeric Android versionCode. Defaults to release environment or the version tuple.
    #[arg(long)]
    android_version_code: Option<String>,
    /// Release date in YYYY-MM-DD form.
    #[arg(long)]
    date: Option<String>,
}

impl From<ReleaseMetadataArgs> for release::ReleaseMetadataOptions {
    fn from(value: ReleaseMetadataArgs) -> Self {
        Self {
            ios_build: value.ios_build,
            android_version_code: value.android_version_code,
            release_date: value.date,
        }
    }
}

#[derive(Debug, Args)]
struct ReleaseVerifyVersionArgs {
    /// Numeric major.minor.patch product version.
    version: String,
    /// Expected numeric iOS CFBundleVersion; defaults to the current value.
    #[arg(long)]
    ios_build: Option<String>,
    /// Expected numeric Android versionCode; defaults to the current value.
    #[arg(long)]
    android_version_code: Option<String>,
}

#[derive(Debug, Args)]
struct ContractArgs {
    /// Repository root. Defaults to VESPER_REPO_ROOT or the current directory.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: ContractCommand,
}

#[derive(Debug, Subcommand)]
enum ContractCommand {
    /// Verifies cross-language DTO fixtures and binary naming contracts.
    Verify,
    /// Scans boundary lifecycle and forward-compatibility invariants.
    Boundary(ContractBoundaryArgs),
}

#[derive(Debug, Args)]
struct ContractBoundaryArgs {
    /// Emits focused warning candidates in addition to failures.
    #[arg(long)]
    warnings: bool,
    /// Emits the broad warning candidate set in addition to failures.
    #[arg(long)]
    all_warnings: bool,
}

#[derive(Debug, Args)]
struct PluginArgs {
    #[command(subcommand)]
    command: PluginCommand,
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// Creates a safe Rust Native or WASM plugin project.
    New(PluginNewArgs),
    /// Builds and stages one manifest-declared Rust plugin artifact.
    Build(PluginBuildArgs),
    /// Inspects manifest metadata or one concrete plugin artifact.
    Inspect(PluginInspectArgs),
    /// Runs bounded conformance checks against one concrete plugin artifact.
    Check(PluginCheckArgs),
    /// Emits the artifact-independent canonical descriptor as JSON.
    Descriptor(PluginDescriptorArgs),
    /// Emits one validated mobile registry fragment as JSON.
    RegistryFragment(PluginRegistryFragmentArgs),
    /// Builds a deterministic signed Vesper plugin package.
    Package(PluginPackageArgs),
    /// Verifies package integrity and publisher trust without loading code.
    Verify(PluginVerifyArgs),
    /// Verifies and atomically installs a signed plugin package.
    Install(PluginInstallArgs),
    /// Removes one installed plugin version.
    Uninstall(PluginUninstallArgs),
    /// Lists verified plugin versions installed under a root.
    List(PluginListArgs),
    /// Manages publisher signing keys.
    Key(PluginKeyArgs),
}

#[derive(Debug, Args)]
struct PluginBuildArgs {
    /// Path to vesper-plugin.toml.
    manifest: PathBuf,
    /// Path to Cargo.toml; relative paths are resolved from the current directory.
    #[arg(long)]
    cargo_manifest: Option<PathBuf>,
    /// Artifact transport selector.
    #[arg(long, value_enum)]
    transport: Option<PluginTransportArg>,
    /// Exact manifest artifact target selector.
    #[arg(long)]
    target: Option<String>,
    /// Exact manifest artifact architecture selector.
    #[arg(long)]
    architecture: Option<String>,
    /// Cargo profile used for the build.
    #[arg(long, value_enum, default_value_t = PluginBuildProfile::Dev)]
    profile: PluginBuildProfile,
    /// Optional Cargo package selector for workspace projects.
    #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new())]
    package: Option<String>,
    /// Cargo build deadline in milliseconds.
    #[arg(
        long,
        default_value_t = 900_000,
        value_parser = clap::value_parser!(u64).range(1..=3_600_000)
    )]
    timeout_ms: u64,
}

#[derive(Debug, Args)]
struct PluginNewArgs {
    /// New plugin project directory; it must not already exist.
    directory: PathBuf,
    /// Valid reverse-DNS plugin identity.
    #[arg(long)]
    plugin_id: String,
    /// Valid reverse-DNS publisher identity.
    #[arg(long)]
    publisher: String,
    /// SPDX license expression for the plugin project.
    #[arg(long)]
    license: String,
    /// Rust plugin transport.
    #[arg(long, value_enum)]
    transport: PluginTransportArg,
    /// Capability to scaffold; pass the option more than once for multiple capabilities.
    #[arg(long, value_enum, required = true)]
    capability: Vec<PluginScaffoldCapability>,
    /// Human-readable plugin name.
    #[arg(long)]
    name: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PluginTransportArg {
    Native,
    Wasm,
}

impl From<PluginTransportArg> for PluginArtifactTransport {
    fn from(value: PluginTransportArg) -> Self {
        match value {
            PluginTransportArg::Native => Self::Native,
            PluginTransportArg::Wasm => Self::Wasm,
        }
    }
}

#[derive(Debug, Args)]
struct PluginInspectArgs {
    /// Path to vesper-plugin.toml.
    manifest: PathBuf,
    /// Validates and reports manifest metadata without accessing an artifact.
    #[arg(long, conflicts_with_all = ["artifact", "transport"])]
    manifest_only: bool,
    /// Native library or WASM component to inspect.
    #[arg(
        long,
        required_unless_present = "manifest_only",
        requires = "transport"
    )]
    artifact: Option<PathBuf>,
    /// Artifact transport; selection is never automatic.
    #[arg(
        long,
        value_enum,
        required_unless_present = "manifest_only",
        requires = "artifact"
    )]
    transport: Option<PluginTransportArg>,
    /// Worker deadline in milliseconds for native artifact inspection.
    #[arg(
        long,
        default_value_t = 10_000,
        value_parser = clap::value_parser!(u64).range(1..=300_000),
        conflicts_with = "manifest_only"
    )]
    timeout_ms: u64,
}

#[derive(Debug, Args)]
struct PluginCheckArgs {
    /// Path to vesper-plugin.toml.
    manifest: PathBuf,
    /// Native library or WASM component to check.
    #[arg(long)]
    artifact: PathBuf,
    /// Artifact transport; selection is never automatic.
    #[arg(long, value_enum)]
    transport: PluginTransportArg,
    /// Worker deadline in milliseconds for native artifact checks.
    #[arg(
        long,
        default_value_t = 30_000,
        value_parser = clap::value_parser!(u64).range(1..=300_000)
    )]
    timeout_ms: u64,
}

#[derive(Debug, Args)]
struct PluginWorkerArgs {
    #[arg(long, hide = true)]
    request: PathBuf,
    #[arg(long, hide = true)]
    response: PathBuf,
}

#[derive(Debug, Args)]
struct PluginPackageArgs {
    /// Path to vesper-plugin.toml.
    manifest: PathBuf,
    /// Publisher signing key generated by vesper.
    #[arg(long)]
    signing_key: PathBuf,
    /// Destination with the .vesper-plugin extension.
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Args)]
struct PluginVerifyArgs {
    /// Signed .vesper-plugin package.
    package: PathBuf,
    /// Host-configured publisher trust store.
    #[arg(long)]
    trust_store: PathBuf,
}

#[derive(Debug, Args)]
struct PluginInstallArgs {
    /// Signed .vesper-plugin package.
    package: PathBuf,
    /// Host-configured publisher trust store.
    #[arg(long)]
    trust_store: PathBuf,
    /// Root directory for verified plugin installations.
    #[arg(long)]
    root: PathBuf,
}

#[derive(Debug, Args)]
struct PluginUninstallArgs {
    /// Reverse-DNS plugin identity.
    #[arg(long)]
    plugin_id: String,
    /// Exact semantic version to remove.
    #[arg(long)]
    version: String,
    /// Root directory for verified plugin installations.
    #[arg(long)]
    root: PathBuf,
}

#[derive(Debug, Args)]
struct PluginListArgs {
    /// Root directory for verified plugin installations.
    #[arg(long)]
    root: PathBuf,
}

#[derive(Debug, Args)]
struct PluginKeyArgs {
    #[command(subcommand)]
    command: PluginKeyCommand,
}

#[derive(Debug, Subcommand)]
enum PluginKeyCommand {
    /// Generates a signing key and adds its public key to a trust store.
    Generate(PluginKeyGenerateArgs),
}

#[derive(Debug, Args)]
struct PluginKeyGenerateArgs {
    #[arg(long)]
    publisher: String,
    #[arg(long)]
    signing_key_output: PathBuf,
    #[arg(long)]
    trust_store_output: PathBuf,
}

#[derive(Debug, Args)]
struct PluginDescriptorArgs {
    /// Path to vesper-plugin.toml.
    manifest: PathBuf,
    /// Emit only the canonical descriptor SHA-256.
    #[arg(long)]
    hash_only: bool,
    /// Atomically write the result instead of emitting it to stdout.
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RegistryPlatform {
    Android,
    Ios,
}

#[derive(Debug, Args)]
struct PluginRegistryFragmentArgs {
    /// Path to vesper-plugin.toml.
    manifest: PathBuf,
    #[arg(long, value_enum)]
    platform: RegistryPlatform,
    #[arg(long)]
    target: String,
    #[arg(long)]
    architecture: String,
    #[arg(long)]
    minimum_os: String,
    #[arg(long)]
    locator_name: String,
    /// Android runtime shared library to hash.
    #[arg(long, required_if_eq("platform", "android"))]
    artifact: Option<PathBuf>,
    /// Apple framework bundle identifier.
    #[arg(long, required_if_eq("platform", "ios"))]
    bundle_identifier: Option<String>,
    /// Atomically write the fragment instead of emitting it to stdout.
    #[arg(long)]
    output: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "{error}");
            ExitCode::from(error.kind().exit_code())
        }
    }
}

fn run(cli: Cli) -> CliResult<()> {
    match cli.command {
        Command::Android(arguments) => run_android(arguments),
        Command::Ios(arguments) => run_ios(arguments),
        Command::Ffmpeg(arguments) => run_ffmpeg(arguments),
        Command::Plugin(plugin) => match plugin.command {
            PluginCommand::New(arguments) => new_plugin(arguments),
            PluginCommand::Build(arguments) => build_plugin(arguments),
            PluginCommand::Inspect(arguments) => inspect_plugin(arguments),
            PluginCommand::Check(arguments) => check_plugin(arguments),
            PluginCommand::Descriptor(arguments) => emit_descriptor(arguments),
            PluginCommand::RegistryFragment(arguments) => emit_registry_fragment(arguments),
            PluginCommand::Package(arguments) => package_plugin(arguments),
            PluginCommand::Verify(arguments) => verify_plugin(arguments),
            PluginCommand::Install(arguments) => install_plugin(arguments),
            PluginCommand::Uninstall(arguments) => uninstall_installed_plugin(arguments),
            PluginCommand::List(arguments) => list_plugins(arguments),
            PluginCommand::Key(arguments) => match arguments.command {
                PluginKeyCommand::Generate(arguments) => generate_plugin_key(arguments),
            },
        },
        Command::Contract(arguments) => run_contract(arguments),
        Command::Ffi(arguments) => run_ffi(arguments),
        Command::Desktop(arguments) => run_desktop(arguments),
        Command::Media(arguments) => run_media(arguments),
        Command::Mobile(arguments) => run_mobile(arguments),
        Command::Flutter(arguments) => run_flutter(arguments),
        Command::Release(arguments) => run_release(arguments),
        Command::PluginWorker(arguments) => run_plugin_worker(arguments),
    }
}

fn run_media(arguments: MediaArgs) -> CliResult<()> {
    let root = contract::resolve_repository_root(arguments.root.as_deref())
        .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
    match arguments.command {
        MediaCommand::GenerateSourceNormalizerFixtures => {
            let stdout = io::stdout();
            let stderr = io::stderr();
            let mut output = stdout.lock();
            let mut diagnostics = stderr.lock();
            media::generate_source_normalizer_fixtures(&root, &mut output, &mut diagnostics)
                .map_err(|error| match error.kind() {
                    media::MediaErrorKind::Storage => {
                        CliError::manifest_or_package(error.to_string())
                    }
                    media::MediaErrorKind::Compatibility => {
                        CliError::compatibility(error.to_string())
                    }
                    media::MediaErrorKind::Conformance => CliError::conformance(error.to_string()),
                    media::MediaErrorKind::Worker => CliError::worker(error.to_string()),
                })
        }
    }
}

fn run_ffmpeg(arguments: FfmpegArgs) -> CliResult<()> {
    let root = contract::resolve_repository_root(arguments.root.as_deref())
        .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
    let stdout = io::stdout();
    let mut output = stdout.lock();
    ffmpeg::run(
        &root,
        &ffmpeg::FfmpegRequest {
            profile: arguments.profile,
            platform: arguments.platform.map(Into::into),
            list_profiles: arguments.list_profiles,
            dry_run: arguments.dry_run,
            verify_only: arguments.verify_only,
            output_directory: arguments.output_dir,
            android_artifact: arguments.android_artifact.into(),
            android_abis: arguments.abi,
            ios_slices: arguments.slice,
            extra_libraries: arguments.extra_libraries,
            extra_demuxers: arguments.extra_demuxers,
            extra_muxers: arguments.extra_muxers,
            extra_protocols: arguments.extra_protocols,
            extra_decoders: arguments.extra_decoders,
            extra_parsers: arguments.extra_parsers,
            extra_bsfs: arguments.extra_bsfs,
            extra_configure_args: arguments.extra_configure_arg,
            tls_backend: arguments.tls_backend,
            force: arguments.force,
            acknowledge_gpl_nonfree: arguments.acknowledge_gpl_nonfree,
        },
        &mut output,
    )
    .map_err(map_ffmpeg_error)
}

fn map_ffmpeg_error(error: ffmpeg::FfmpegError) -> CliError {
    match error.kind() {
        ffmpeg::FfmpegErrorKind::Storage => CliError::manifest_or_package(error.to_string()),
        ffmpeg::FfmpegErrorKind::Compatibility => CliError::compatibility(error.to_string()),
        ffmpeg::FfmpegErrorKind::Conformance => CliError::conformance(error.to_string()),
        ffmpeg::FfmpegErrorKind::Worker => CliError::worker(error.to_string()),
    }
}

fn run_ios(arguments: IosArgs) -> CliResult<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let requested_root = arguments.root;
    match arguments.command {
        IosCommand::Ffi(arguments) => {
            ios_ffi::ensure_supported_host().map_err(map_ios_error)?;
            let root = contract::resolve_repository_root(requested_root.as_deref())
                .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
            let stderr = io::stderr();
            let mut diagnostics = stderr.lock();
            match arguments.platform {
                Some(platform) => ios_ffi::build_platform(
                    &root,
                    platform.into(),
                    arguments.profile.into(),
                    &mut output,
                    &mut diagnostics,
                ),
                None => ios_ffi::build(
                    &root,
                    arguments.profile.into(),
                    &mut output,
                    &mut diagnostics,
                ),
            }
            .map_err(map_ios_error)
        }
        IosCommand::RemuxPlugin(arguments) => run_ios_plugin_build(
            requested_root.as_deref(),
            ios_plugin::IosPluginId::RemuxFfmpeg,
            arguments,
            &mut output,
        ),
        IosCommand::SourceNormalizerPlugin(arguments) => run_ios_plugin_build(
            requested_root.as_deref(),
            ios_plugin::IosPluginId::SourceNormalizerFfmpeg,
            arguments,
            &mut output,
        ),
        IosCommand::FrameProcessorPlugin(arguments) => run_ios_plugin_build(
            requested_root.as_deref(),
            ios_plugin::IosPluginId::FrameProcessorDiagnostic,
            arguments,
            &mut output,
        ),
        IosCommand::DecoderVideoToolboxPlugin(arguments) => run_ios_plugin_build(
            requested_root.as_deref(),
            ios_plugin::IosPluginId::DecoderVideoToolbox,
            arguments,
            &mut output,
        ),
        IosCommand::StageRemuxPluginRelease(arguments) => run_ios_plugin_release(
            requested_root.as_deref(),
            ios_plugin::IosPluginId::RemuxFfmpeg,
            arguments,
            &mut output,
        ),
        IosCommand::StageSourceNormalizerPluginRelease(arguments) => run_ios_plugin_release(
            requested_root.as_deref(),
            ios_plugin::IosPluginId::SourceNormalizerFfmpeg,
            arguments,
            &mut output,
        ),
        IosCommand::StageFrameProcessorPluginRelease(arguments) => run_ios_plugin_release(
            requested_root.as_deref(),
            ios_plugin::IosPluginId::FrameProcessorDiagnostic,
            arguments,
            &mut output,
        ),
        IosCommand::StageDecoderVideoToolboxPluginRelease(arguments) => run_ios_plugin_release(
            requested_root.as_deref(),
            ios_plugin::IosPluginId::DecoderVideoToolbox,
            arguments,
            &mut output,
        ),
        IosCommand::KitXcframework => {
            ios_kit::ensure_supported_host().map_err(map_ios_error)?;
            let root = contract::resolve_repository_root(requested_root.as_deref())
                .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
            let stderr = io::stderr();
            let mut diagnostics = stderr.lock();
            ios_kit::build(&root, &mut output, &mut diagnostics).map_err(map_ios_error)
        }
        IosCommand::SyncBridgeShim => {
            let root = contract::resolve_repository_root(requested_root.as_deref())
                .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
            ios::sync_bridge_shim(&root, &mut output).map_err(map_ios_error)
        }
        IosCommand::VerifyBridgeShim(arguments) => {
            let root = contract::resolve_repository_root(requested_root.as_deref())
                .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
            ios::verify_bridge_shim(&root, arguments.archive.as_deref(), &mut output)
                .map_err(map_ios_error)
        }
        IosCommand::VerifyAppStoreLayout(arguments) => ios::verify_app_store_layout(
            &arguments.app_path,
            arguments.verify_signatures,
            &mut output,
        )
        .map_err(map_ios_error),
        IosCommand::VerifyNativeFrame(arguments) => {
            ios_native_frame::ensure_supported_host().map_err(map_ios_error)?;
            let root = contract::resolve_repository_root(requested_root.as_deref())
                .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
            let tokens = arguments
                .tokens
                .into_iter()
                .map(IosVerifyNativeFrameTokenArg::as_str)
                .collect::<Vec<_>>();
            let stderr = io::stderr();
            let mut diagnostics = stderr.lock();
            ios_native_frame::verify(&root, &tokens, &mut output, &mut diagnostics)
                .map_err(map_ios_error)
        }
        IosCommand::VerifyOptionalPluginsRelease(arguments) => {
            ios_optional_release::ensure_supported_host().map_err(map_ios_error)?;
            let root = contract::resolve_repository_root(requested_root.as_deref())
                .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
            let stderr = io::stderr();
            let mut diagnostics = stderr.lock();
            ios_optional_release::verify_optional_plugins_release(
                &root,
                arguments.release_directory.as_deref(),
                &mut output,
                &mut diagnostics,
            )
            .map_err(map_ios_error)
        }
        IosCommand::VerifyOptionalPluginsDevice(arguments) => {
            ios_optional_device::ensure_supported_host().map_err(map_ios_error)?;
            let root = contract::resolve_repository_root(requested_root.as_deref())
                .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
            let stderr = io::stderr();
            let mut diagnostics = stderr.lock();
            ios_optional_device::verify(
                &root,
                ios_optional_device::IosOptionalPluginDeviceRequest {
                    release_directory: arguments.release_directory,
                    device: arguments.device,
                    development_team: arguments.development_team,
                    output_directory: arguments.output_directory,
                    allow_provisioning_updates: arguments.allow_provisioning_updates,
                },
                &mut output,
                &mut diagnostics,
            )
            .map_err(map_ios_error)
        }
        IosCommand::VerifyRelease(arguments) => {
            ios_optional_release::ensure_release_supported_host().map_err(map_ios_error)?;
            let root = contract::resolve_repository_root(requested_root.as_deref())
                .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
            let stderr = io::stderr();
            let mut diagnostics = stderr.lock();
            ios_optional_release::verify_release(
                &root,
                arguments.release_directory.as_deref(),
                matches!(arguments.scope, IosReleaseScopeArg::Complete),
                &mut output,
                &mut diagnostics,
            )
            .map_err(map_ios_error)
        }
        IosCommand::StageRelease(arguments) => {
            let root = contract::resolve_repository_root(requested_root.as_deref())
                .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
            let include_optional_plugins =
                arguments.include_optional_plugins.unwrap_or_else(|| {
                    matches!(
                        std::env::var("VESPER_IOS_INCLUDE_OPTIONAL_PLUGINS").as_deref(),
                        Ok("1" | "true" | "TRUE" | "yes" | "YES")
                    )
                });
            let package_artifacts_explicit = arguments.package_artifacts_directory.is_some();
            let package_artifacts_directory = arguments.package_artifacts_directory.or_else(|| {
                std::env::var_os("VESPER_IOS_OPTIONAL_PACKAGE_ARTIFACTS_DIR").map(Into::into)
            });
            ios_release::stage_release(
                &root,
                arguments.output_directory.as_deref(),
                include_optional_plugins,
                package_artifacts_directory.as_deref(),
                package_artifacts_explicit,
                &mut output,
            )
            .map_err(map_ios_error)
        }
        IosCommand::FfmpegRuntimeRelease(arguments) => {
            let root = contract::resolve_repository_root(requested_root.as_deref())
                .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
            let stderr = io::stderr();
            let mut diagnostics = stderr.lock();
            ios_plugin_release::stage_ffmpeg_runtime(
                &root,
                arguments.arguments,
                &mut output,
                &mut diagnostics,
            )
            .map_err(map_ios_error)
        }
        IosCommand::StageOptionalPluginsRelease(arguments) => {
            let root = root_for_worker(requested_root.as_deref())?;
            let stdout = io::stdout();
            let mut output = stdout.lock();
            ios_release::stage_optional_plugins_release(&root, arguments.arguments, &mut output)
                .map_err(map_ios_error)
        }
        IosCommand::VerifySubtitles(arguments) => {
            let root = root_for_worker(requested_root.as_deref())?;
            let development_team = match arguments.development_team {
                Some(team) => Some(team),
                None => std::env::var_os("VESPER_IOS_DEVELOPMENT_TEAM")
                    .map(|value| {
                        value.into_string().map_err(|_| {
                            CliError::usage("VESPER_IOS_DEVELOPMENT_TEAM must contain valid UTF-8")
                        })
                    })
                    .transpose()?,
            };
            let stderr = io::stderr();
            let mut diagnostics = stderr.lock();
            ios_subtitle::verify(
                &root,
                ios_subtitle::IosSubtitleRequest {
                    scope: arguments.scope.into(),
                    device_id: arguments.device,
                    simulator_id: arguments.simulator,
                    evidence_directory: arguments.evidence_dir,
                    development_team,
                },
                &mut output,
                &mut diagnostics,
            )
            .map_err(map_subtitle_error)
        }
    }
}

fn root_for_worker(requested_root: Option<&Path>) -> CliResult<PathBuf> {
    contract::resolve_repository_root(requested_root)
        .map_err(|error| CliError::manifest_or_package(error.to_string()))
}

fn run_ios_plugin_build(
    requested_root: Option<&Path>,
    plugin_id: ios_plugin::IosPluginId,
    arguments: IosPluginBuildArgs,
    output: &mut dyn Write,
) -> CliResult<()> {
    ios_plugin::ensure_supported_host().map_err(map_ios_error)?;
    let root = contract::resolve_repository_root(requested_root)
        .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
    let stderr = io::stderr();
    let mut diagnostics = stderr.lock();
    ios_plugin::build(
        &root,
        ios_plugin::IosPluginBuildRequest {
            plugin_id,
            output_directory: arguments.output_directory,
            arguments: arguments.arguments,
            environment: ios_plugin::IosPluginBuildEnvironment::default(),
        },
        output,
        &mut diagnostics,
    )
    .map_err(map_ios_error)
}

fn run_ios_plugin_release(
    requested_root: Option<&Path>,
    plugin_id: ios_plugin::IosPluginId,
    arguments: IosPluginReleaseArgs,
    output: &mut dyn Write,
) -> CliResult<()> {
    ios_plugin_release::ensure_supported_host().map_err(map_ios_error)?;
    let root = contract::resolve_repository_root(requested_root)
        .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
    let stderr = io::stderr();
    let mut diagnostics = stderr.lock();
    ios_plugin_release::stage(
        &root,
        plugin_id,
        arguments.arguments,
        output,
        &mut diagnostics,
    )
    .map_err(map_ios_error)
}

fn map_ios_error(error: ios::IosError) -> CliError {
    match error.kind() {
        ios::IosErrorKind::Storage => CliError::manifest_or_package(error.to_string()),
        ios::IosErrorKind::Compatibility => CliError::compatibility(error.to_string()),
        ios::IosErrorKind::Conformance => CliError::conformance(error.to_string()),
        ios::IosErrorKind::Worker => CliError::worker(error.to_string()),
    }
}

fn run_android(arguments: AndroidArgs) -> CliResult<()> {
    let root = contract::resolve_repository_root(arguments.root.as_deref())
        .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
    match arguments.command {
        AndroidCommand::Jni(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            android::build_jni(
                &root,
                arguments.profile.as_deref().unwrap_or("debug"),
                &arguments.abis,
                &mut output,
            )
            .map_err(map_android_error)
        }
        AndroidCommand::Aar(arguments) => android::build_aar(
            &root,
            arguments
                .module_task
                .as_deref()
                .unwrap_or("assembleRelease"),
            android::include_optional_plugins(arguments.include_optional_plugins),
        )
        .map_err(map_android_error),
        AndroidCommand::RemuxPlugin(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            android::build_ffmpeg_plugin(&root, "remux", &arguments.arguments, &mut output)
                .map_err(map_android_error)
        }
        AndroidCommand::SourceNormalizerPlugin(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            android::build_ffmpeg_plugin(
                &root,
                "source-normalizer",
                &arguments.arguments,
                &mut output,
            )
            .map_err(map_android_error)
        }
        AndroidCommand::DecoderMediacodecPlugin(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            android::build_runtime_free_plugin(
                &root,
                "decoder-mediacodec",
                &arguments.arguments,
                &mut output,
            )
            .map_err(map_android_error)
        }
        AndroidCommand::FrameProcessorPlugin(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            android::build_runtime_free_plugin(
                &root,
                "frame-processor-diagnostic",
                &arguments.arguments,
                &mut output,
            )
            .map_err(map_android_error)
        }
        AndroidCommand::StageRelease(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            android::stage_release(
                &root,
                arguments.output_directory.as_deref(),
                &arguments.abis,
                android::include_optional_plugins(arguments.include_optional_plugins),
                &mut output,
            )
            .map_err(map_android_error)
        }
        AndroidCommand::SampleApks(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            android::sample_apks(
                &root,
                arguments.output_directory.as_deref(),
                &arguments.abis,
                &mut output,
            )
            .map_err(map_android_error)
        }
        AndroidCommand::ProvisionTestJni(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            android::provision_test_jni(
                &root,
                &arguments.output_directory,
                &arguments.profile,
                &arguments.ffmpeg_profile,
                &mut output,
            )
            .map_err(map_android_error)
        }
        AndroidCommand::ExternalPlaybackJni(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            android::build_external_playback_jni(
                &root,
                &arguments.output_directory,
                &arguments.assets_directory,
                &arguments.profile,
                &arguments.ffmpeg_profile,
                arguments.skip_ffmpeg_runtime,
                &mut output,
            )
            .map_err(map_android_error)
        }
        AndroidCommand::VerifySubtitles(arguments) => {
            let stdout = io::stdout();
            let stderr = io::stderr();
            let mut output = stdout.lock();
            let mut diagnostics = stderr.lock();
            android_subtitle::verify(
                &root,
                android_subtitle::SubtitleRequest {
                    scope: arguments.scope.into(),
                    device_id: arguments.device,
                    evidence_directory: arguments.evidence_dir,
                },
                &mut output,
                &mut diagnostics,
            )
            .map_err(map_subtitle_error)
        }
        AndroidCommand::RuntimeFreePlugin(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            android::build_runtime_free_plugin(
                &root,
                &arguments.plugin,
                &arguments.arguments,
                &mut output,
            )
            .map_err(map_android_error)
        }
        AndroidCommand::FfmpegPlugin(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            android::build_ffmpeg_plugin(
                &root,
                &arguments.plugin,
                &arguments.arguments,
                &mut output,
            )
            .map_err(map_android_error)
        }
    }
}

fn map_android_error(error: android::AndroidError) -> CliError {
    match error.kind() {
        android::AndroidErrorKind::Usage => CliError::usage(error.to_string()),
        android::AndroidErrorKind::Storage => CliError::manifest_or_package(error.to_string()),
        android::AndroidErrorKind::Compatibility => CliError::compatibility(error.to_string()),
        android::AndroidErrorKind::Conformance => CliError::conformance(error.to_string()),
        android::AndroidErrorKind::Worker => CliError::worker(error.to_string()),
    }
}

fn map_subtitle_error(error: subtitle::SubtitleError) -> CliError {
    match error.kind() {
        subtitle::SubtitleErrorKind::Usage => CliError::usage(error.to_string()),
        subtitle::SubtitleErrorKind::Storage => CliError::manifest_or_package(error.to_string()),
        subtitle::SubtitleErrorKind::Compatibility => CliError::compatibility(error.to_string()),
        subtitle::SubtitleErrorKind::Conformance => CliError::conformance(error.to_string()),
        subtitle::SubtitleErrorKind::Worker => CliError::worker(error.to_string()),
    }
}

fn run_desktop(arguments: DesktopArgs) -> CliResult<()> {
    let root = contract::resolve_repository_root(arguments.root.as_deref())
        .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
    match arguments.command {
        DesktopCommand::EnsureFfmpeg => desktop::ensure_ffmpeg(&root).map_err(|error| match error
            .kind()
        {
            desktop::DesktopErrorKind::Storage => CliError::manifest_or_package(error.to_string()),
            desktop::DesktopErrorKind::Compatibility => CliError::compatibility(error.to_string()),
            desktop::DesktopErrorKind::Conformance => CliError::conformance(error.to_string()),
            desktop::DesktopErrorKind::Worker => CliError::worker(error.to_string()),
        }),
        DesktopCommand::VerifyRemux(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            desktop::verify_remux(&root, &arguments.tokens, &mut output).map_err(
                |error| match error.kind() {
                    desktop::DesktopErrorKind::Storage => {
                        CliError::manifest_or_package(error.to_string())
                    }
                    desktop::DesktopErrorKind::Compatibility => {
                        CliError::compatibility(error.to_string())
                    }
                    desktop::DesktopErrorKind::Conformance => {
                        CliError::conformance(error.to_string())
                    }
                    desktop::DesktopErrorKind::Worker => CliError::worker(error.to_string()),
                },
            )
        }
        DesktopCommand::VerifyDecoderDiagnostics(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            desktop::verify_decoder_diagnostics(&root, &arguments.tokens, &mut output).map_err(
                |error| match error.kind() {
                    desktop::DesktopErrorKind::Storage => {
                        CliError::manifest_or_package(error.to_string())
                    }
                    desktop::DesktopErrorKind::Compatibility => {
                        CliError::compatibility(error.to_string())
                    }
                    desktop::DesktopErrorKind::Conformance => {
                        CliError::conformance(error.to_string())
                    }
                    desktop::DesktopErrorKind::Worker => CliError::worker(error.to_string()),
                },
            )
        }
        DesktopCommand::VerifyDecoderD3d11(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            desktop::verify_decoder_d3d11(&root, &arguments.tokens, &mut output).map_err(|error| {
                match error.kind() {
                    desktop::DesktopErrorKind::Storage => {
                        CliError::manifest_or_package(error.to_string())
                    }
                    desktop::DesktopErrorKind::Compatibility => {
                        CliError::compatibility(error.to_string())
                    }
                    desktop::DesktopErrorKind::Conformance => {
                        CliError::conformance(error.to_string())
                    }
                    desktop::DesktopErrorKind::Worker => CliError::worker(error.to_string()),
                }
            })
        }
        DesktopCommand::VerifyDecoderVideoToolbox(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            desktop::verify_decoder_videotoolbox(&root, &arguments.tokens, &mut output).map_err(
                |error| match error.kind() {
                    desktop::DesktopErrorKind::Storage => {
                        CliError::manifest_or_package(error.to_string())
                    }
                    desktop::DesktopErrorKind::Compatibility => {
                        CliError::compatibility(error.to_string())
                    }
                    desktop::DesktopErrorKind::Conformance => {
                        CliError::conformance(error.to_string())
                    }
                    desktop::DesktopErrorKind::Worker => CliError::worker(error.to_string()),
                },
            )
        }
    }
}

fn run_mobile(arguments: MobileArgs) -> CliResult<()> {
    let root = contract::resolve_repository_root(arguments.root.as_deref())
        .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
    match arguments.command {
        MobileCommand::VerifyNoRemux(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            mobile::verify_no_remux(&root, arguments.mode.into(), &mut output).map_err(|error| {
                match error.kind() {
                    mobile::MobileErrorKind::Storage => {
                        CliError::manifest_or_package(error.to_string())
                    }
                    mobile::MobileErrorKind::Compatibility => {
                        CliError::compatibility(error.to_string())
                    }
                    mobile::MobileErrorKind::Conformance => {
                        CliError::conformance(error.to_string())
                    }
                    mobile::MobileErrorKind::Worker => CliError::worker(error.to_string()),
                }
            })
        }
        MobileCommand::VerifyBinaryNames => contract::verify_binary_library_names(&root)
            .map_err(|error| match error {
                contract::ContractError::Drift(message) => CliError::conformance(message),
                contract::ContractError::Storage(message) => CliError::manifest_or_package(message),
            })
            .and_then(|()| {
                emit_bytes(
                    b"Verified Rust and mobile distribution binary names use libvesper_* outputs.\n",
                    None,
                )
            }),
    }
}

fn run_flutter(arguments: FlutterArgs) -> CliResult<()> {
    let root = contract::resolve_repository_root(arguments.root.as_deref())
        .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
    match arguments.command {
        FlutterCommand::StagePub(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            let include_optional =
                flutter::include_optional_plugins(arguments.include_optional_plugins);
            flutter::stage_pub_packages(
                &root,
                arguments.output_directory.as_deref(),
                arguments.version.as_deref(),
                include_optional,
                &mut output,
            )
            .map_err(map_flutter_error)
        }
        FlutterCommand::PubDryRun(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            let include_optional =
                flutter::include_optional_plugins(arguments.include_optional_plugins);
            flutter::dry_run_pub_packages(
                &root,
                arguments.output_directory.as_deref(),
                arguments.version.as_deref(),
                include_optional,
                &mut output,
            )
            .map_err(map_flutter_error)
        }
        FlutterCommand::PubPublish(arguments) => {
            let stdout = io::stdout();
            let stderr = io::stderr();
            let mut output = stdout.lock();
            let mut diagnostics = stderr.lock();
            let include_optional =
                flutter::include_optional_plugins(arguments.include_optional_plugins);
            flutter::publish_pub_packages(
                &root,
                arguments.output_directory.as_deref(),
                arguments.version.as_deref(),
                include_optional,
                &mut output,
                &mut diagnostics,
            )
            .map_err(map_flutter_error)
        }
        FlutterCommand::LocalOverrides(arguments) => {
            let stdout = io::stdout();
            let mut output = stdout.lock();
            let include_optional =
                flutter::include_optional_plugins(arguments.include_optional_plugins);
            flutter::write_local_overrides(&root, include_optional, &mut output)
                .map_err(map_flutter_error)
        }
        FlutterCommand::VerifyAndroidPlugin(arguments) => {
            let include_optional =
                flutter::include_optional_plugins(arguments.include_optional_plugins);
            flutter::verify_android_plugin(&root, include_optional).map_err(map_flutter_error)
        }
        FlutterCommand::VerifyAndroidRelease(arguments) => {
            android::validate_android_sample_apk(&arguments.apk, "arm64-v8a", true)
                .map_err(map_android_error)?;
            emit_bytes(
                format!(
                    "Verified Flutter Android release APK excludes test fixtures: {}\n",
                    arguments.apk.display()
                )
                .as_bytes(),
                None,
            )
        }
    }
}

fn map_flutter_error(error: flutter::FlutterError) -> CliError {
    match error.kind() {
        flutter::FlutterErrorKind::Storage => CliError::manifest_or_package(error.to_string()),
        flutter::FlutterErrorKind::Compatibility => CliError::compatibility(error.to_string()),
        flutter::FlutterErrorKind::Conformance => CliError::conformance(error.to_string()),
        flutter::FlutterErrorKind::Worker => CliError::worker(error.to_string()),
    }
}

fn run_ffi(arguments: FfiArgs) -> CliResult<()> {
    let root = contract::resolve_repository_root(arguments.root.as_deref())
        .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
    let stdout = io::stdout();
    let mut output = stdout.lock();
    match arguments.command {
        FfiCommand::Generate => ffi::run_header(&root, ffi::FfiHeaderMode::Generate, &mut output),
        FfiCommand::Sync => ffi::run_header(&root, ffi::FfiHeaderMode::Sync, &mut output),
        FfiCommand::Verify => ffi::run_header(&root, ffi::FfiHeaderMode::Verify, &mut output),
        FfiCommand::CHostSmoke(arguments) => ffi::run_c_host_smoke(
            &root,
            arguments.build_only,
            arguments.source.as_deref(),
            &mut output,
        ),
    }
    .map_err(map_ffi_error)
}

fn map_ffi_error(error: ffi::FfiError) -> CliError {
    match error.kind() {
        ffi::FfiErrorKind::Storage => CliError::manifest_or_package(error.to_string()),
        ffi::FfiErrorKind::Conformance => CliError::conformance(error.to_string()),
        ffi::FfiErrorKind::Worker => CliError::worker(error.to_string()),
    }
}

fn run_release(arguments: ReleaseArgs) -> CliResult<()> {
    let root = contract::resolve_repository_root(arguments.root.as_deref())
        .map_err(|error| CliError::manifest_or_package(error.to_string()))?;
    let context = release::ReleaseContext::new(root, release::ReleaseEnvironment::from_process());
    match arguments.command {
        ReleaseCommand::Notes(arguments) => {
            let tag = arguments
                .tag
                .or_else(|| context.default_notes_tag().map(str::to_owned))
                .ok_or_else(|| {
                    CliError::manifest_or_package("release notes requires a tag or GITHUB_REF_NAME")
                })?;
            let output = context
                .generate_notes(&tag, arguments.output.as_deref())
                .map_err(map_release_error)?;
            emit_bytes(
                format!(
                    "Generated VesperPlayerKit release notes at:\n  {}\n",
                    output.display()
                )
                .as_bytes(),
                None,
            )
        }
        ReleaseCommand::SetVersion(arguments) => {
            let metadata = context
                .metadata_for_version(&arguments.version, arguments.metadata.into())
                .map_err(map_release_error)?;
            context.set_version(&metadata).map_err(map_release_error)?;
            emit_bytes(
                format!(
                    "Updated Vesper product version to {}.\n",
                    metadata.version()
                )
                .as_bytes(),
                None,
            )
        }
        ReleaseCommand::PrepareFromTag(arguments) => {
            let metadata = context
                .metadata_from_tag(&arguments.tag, arguments.metadata.into())
                .map_err(map_release_error)?;
            context.set_version(&metadata).map_err(map_release_error)?;
            context
                .verify_version(
                    metadata.version(),
                    Some(metadata.ios_build().to_owned()),
                    Some(metadata.android_version_code().to_owned()),
                )
                .map_err(map_release_error)?;
            context
                .append_ci_metadata(&metadata)
                .map_err(map_release_error)?;
            let output = format!(
                "Updated Vesper product version to {}.\nVerified Vesper product version {}.\n{}",
                metadata.version(),
                metadata.version(),
                metadata.output()
            );
            emit_bytes(output.as_bytes(), None)
        }
        ReleaseCommand::MetadataFromTag(arguments) => {
            let metadata = context
                .metadata_from_tag(&arguments.tag, arguments.metadata.into())
                .map_err(map_release_error)?;
            context
                .append_ci_metadata(&metadata)
                .map_err(map_release_error)?;
            emit_bytes(metadata.output().as_bytes(), None)
        }
        ReleaseCommand::VerifyVersion(arguments) => {
            context
                .verify_version(
                    &arguments.version,
                    arguments.ios_build,
                    arguments.android_version_code,
                )
                .map_err(map_release_error)?;
            emit_bytes(
                format!("Verified Vesper product version {}.\n", arguments.version).as_bytes(),
                None,
            )
        }
        ReleaseCommand::VerifyCurrent => {
            let version = context.verify_current().map_err(map_release_error)?;
            emit_bytes(
                format!("Verified Vesper product version {version}.\n").as_bytes(),
                None,
            )
        }
    }
}

fn map_release_error(error: release::ReleaseError) -> CliError {
    match error.kind() {
        release::ReleaseErrorKind::Verification => CliError::conformance(error.to_string()),
        release::ReleaseErrorKind::Input | release::ReleaseErrorKind::Storage => {
            CliError::manifest_or_package(error.to_string())
        }
    }
}

fn run_contract(arguments: ContractArgs) -> CliResult<()> {
    let root = contract::resolve_repository_root(arguments.root.as_deref())
        .map_err(|error| CliError::conformance(error.to_string()))?;
    match arguments.command {
        ContractCommand::Verify => {
            let verification = contract::verify(&root)
                .map_err(|error| CliError::conformance(error.to_string()))?;
            emit_bytes(verification.output().as_bytes(), None)
        }
        ContractCommand::Boundary(arguments) => {
            let report = boundary::scan(
                &root,
                boundary::BoundaryScanOptions {
                    show_warnings: arguments.warnings,
                    show_all_warnings: arguments.all_warnings,
                },
            )
            .map_err(CliError::conformance)?;
            emit_bytes(report.output().as_bytes(), None)?;
            if let Some(failure) = report.failure() {
                return Err(CliError::conformance(failure));
            }
            Ok(())
        }
    }
}

fn build_plugin(arguments: PluginBuildArgs) -> CliResult<()> {
    let project = read_project_manifest(&arguments.manifest)?;
    let artifact = select_plugin_artifact(
        &project,
        &PluginArtifactSelector {
            transport: arguments.transport.map(Into::into),
            target: arguments.target,
            architecture: arguments.architecture,
        },
    )
    .map_err(map_plugin_build_error)?;
    let base_directory = arguments
        .manifest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let cargo_manifest = arguments
        .cargo_manifest
        .unwrap_or_else(|| base_directory.join("Cargo.toml"));
    validate_regular_file_for_json(&cargo_manifest, "Cargo manifest")?;
    let cargo_manifest = if cargo_manifest.is_absolute() {
        cargo_manifest
    } else {
        std::env::current_dir()
            .map_err(|error| {
                CliError::worker(format!(
                    "failed to resolve Cargo manifest '{}': {error}",
                    cargo_manifest.display()
                ))
            })?
            .join(cargo_manifest)
    };
    let cargo_directory = cargo_manifest
        .parent()
        .ok_or_else(|| {
            CliError::manifest_or_package(format!(
                "Cargo manifest '{}' has no parent directory",
                cargo_manifest.display()
            ))
        })?
        .to_path_buf();
    let destination = if artifact.source.is_absolute() {
        artifact.source.clone()
    } else {
        base_directory.join(&artifact.source)
    };
    validate_path_for_json_report(&destination, "built plugin artifact")?;
    let report = build_plugin_artifact(PluginBuildRequest {
        plugin_id: project.descriptor().plugin.id.clone(),
        cargo_manifest,
        working_directory: cargo_directory,
        artifact,
        destination,
        profile: arguments.profile,
        package: arguments.package,
        timeout: Duration::from_millis(arguments.timeout_ms),
    })
    .map_err(map_plugin_build_error)?;
    emit_json_result(&report)
}

fn map_plugin_build_error(error: PluginBuildError) -> CliError {
    match error {
        PluginBuildError::Storage(message) => CliError::manifest_or_package(message),
        PluginBuildError::Compatibility(message) => CliError::compatibility(message),
        PluginBuildError::Conformance(message) => CliError::conformance(message),
        PluginBuildError::Worker(message) => CliError::worker(message),
    }
}

fn new_plugin(arguments: PluginNewArgs) -> CliResult<()> {
    validate_path_for_json_report(&arguments.directory, "plugin scaffold directory")?;
    let result = create_plugin_scaffold(PluginScaffoldRequest {
        directory: arguments.directory,
        plugin_id: arguments.plugin_id,
        plugin_name: arguments.name,
        publisher: arguments.publisher,
        license: arguments.license,
        transport: arguments.transport.into(),
        capabilities: arguments.capability,
    });
    let report = result.map_err(|error| {
        if error.is_compatibility() {
            CliError::compatibility(error.to_string())
        } else {
            CliError::manifest_or_package(error.to_string())
        }
    })?;
    emit_json_result(&report)
}

fn inspect_plugin(arguments: PluginInspectArgs) -> CliResult<()> {
    let project = read_project_manifest(&arguments.manifest)?;
    let descriptor = project.descriptor().clone();
    if arguments.manifest_only {
        let report = inspect_manifest(&descriptor, PluginInspectionOperation::Inspect, None);
        return emit_inspection_report(report);
    }
    let artifact = arguments.artifact.ok_or_else(|| {
        CliError::manifest_or_package("--artifact is required unless --manifest-only is used")
    })?;
    let transport = arguments.transport.ok_or_else(|| {
        CliError::manifest_or_package("--transport is required unless --manifest-only is used")
    })?;
    inspect_or_check_artifact(
        descriptor,
        &artifact,
        transport.into(),
        PluginInspectionOperation::Inspect,
        Duration::from_millis(arguments.timeout_ms),
    )
}

fn check_plugin(arguments: PluginCheckArgs) -> CliResult<()> {
    let project = read_project_manifest(&arguments.manifest)?;
    inspect_or_check_artifact(
        project.descriptor().clone(),
        &arguments.artifact,
        arguments.transport.into(),
        PluginInspectionOperation::Check,
        Duration::from_millis(arguments.timeout_ms),
    )
}

fn inspect_or_check_artifact(
    descriptor: PluginDescriptor,
    artifact: &Path,
    transport: PluginArtifactTransport,
    operation: PluginInspectionOperation,
    native_worker_timeout: Duration,
) -> CliResult<()> {
    let preflight = inspect_manifest(&descriptor, operation, Some(transport));
    if preflight.outcome() != PluginInspectionOutcome::Passed {
        return emit_inspection_report(preflight);
    }
    validate_regular_file_for_json(artifact, "plugin artifact")?;

    let report = match transport {
        PluginArtifactTransport::Native => {
            let artifact_utf8 = artifact.to_str().ok_or_else(|| {
                CliError::manifest_or_package(
                    "plugin artifact path must be valid UTF-8 because it crosses the worker JSON boundary",
                )
            })?;
            let request =
                PluginWorkerRequest::new(1, operation, artifact_utf8.to_owned(), descriptor);
            supervise_native_worker(request, native_worker_timeout)?
        }
        PluginArtifactTransport::Wasm => {
            let bytes = read_bounded_regular_file(
                artifact,
                MAX_WASM_PLUGIN_COMPONENT_BYTES,
                "WASM plugin component",
            )?;
            inspect_wasm_plugin(&descriptor, &bytes, operation).map_err(|error| {
                CliError::worker(format!(
                    "failed to initialize the WASM plugin host: {error}"
                ))
            })?
        }
    };
    emit_inspection_report(report)
}

fn emit_inspection_report(report: PluginInspectionReport) -> CliResult<()> {
    let operation = report.operation.as_str();
    let outcome = report.outcome();
    emit_json_result(&report)?;
    match outcome {
        PluginInspectionOutcome::Passed => Ok(()),
        PluginInspectionOutcome::CompatibilityFailure => Err(CliError::compatibility(format!(
            "plugin {operation} found compatibility failures"
        ))),
        PluginInspectionOutcome::ConformanceFailure => Err(CliError::conformance(format!(
            "plugin {operation} found conformance failures"
        ))),
    }
}

fn run_plugin_worker(arguments: PluginWorkerArgs) -> CliResult<()> {
    let mut gate = [0_u8; PLUGIN_WORKER_START_GATE.len()];
    io::stdin().lock().read_exact(&mut gate).map_err(|error| {
        CliError::worker(format!("failed to read plugin worker start gate: {error}"))
    })?;
    if &gate != PLUGIN_WORKER_START_GATE {
        return Err(CliError::worker("invalid plugin worker start gate"));
    }
    let request = read_worker_request(&arguments.request).map_err(CliError::worker)?;
    request.validate().map_err(CliError::worker)?;
    let report = plugin_inspection::inspect_native_plugin(
        &request.descriptor,
        Path::new(&request.library_path_utf8),
        request.operation,
    );
    let response = PluginWorkerResponse::new(request.request_id, report);
    write_worker_response(&arguments.response, &response).map_err(CliError::worker)
}

fn emit_descriptor(arguments: PluginDescriptorArgs) -> CliResult<()> {
    let descriptor = read_descriptor(&arguments.manifest)?;
    let canonical = descriptor
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let bytes = if arguments.hash_only {
        format!("{}\n", canonical.sha256()).into_bytes()
    } else {
        let mut bytes = canonical.json().to_vec();
        bytes.push(b'\n');
        bytes
    };
    emit_bytes(&bytes, arguments.output.as_deref())
}

fn emit_registry_fragment(arguments: PluginRegistryFragmentArgs) -> CliResult<()> {
    let descriptor = read_descriptor(&arguments.manifest)?;
    let canonical = descriptor
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let target = match arguments.platform {
        RegistryPlatform::Android => EmbeddedRegistryTarget::AndroidNativeLibrary {
            target: arguments.target,
            architecture: arguments.architecture,
            minimum_os: arguments.minimum_os,
            library_name: arguments.locator_name,
            artifact_path: arguments
                .artifact
                .ok_or_else(|| "--artifact is required for Android".to_owned())?,
        },
        RegistryPlatform::Ios => EmbeddedRegistryTarget::AppleFramework {
            target: arguments.target,
            architecture: arguments.architecture,
            minimum_os: arguments.minimum_os,
            framework_name: arguments.locator_name,
            bundle_identifier: arguments
                .bundle_identifier
                .ok_or_else(|| "--bundle-identifier is required for iOS".to_owned())?,
        },
    };
    let fragment = EmbeddedRegistryFragment::generate(&canonical, &target)
        .map_err(|error| error.to_string())?;
    emit_bytes(fragment.canonical_json(), arguments.output.as_deref())
}

fn package_plugin(arguments: PluginPackageArgs) -> CliResult<()> {
    validate_path_for_json_report(&arguments.output, "plugin package output path")?;
    let project = read_project_manifest(&arguments.manifest)?;
    let key_bytes = read_bounded_regular_file(
        &arguments.signing_key,
        MAX_PLUGIN_KEY_FILE_BYTES,
        "plugin signing key",
    )?;
    let signing_key = PluginSigningKey::from_json(&key_bytes).map_err(|error| error.to_string())?;
    let base_directory = arguments
        .manifest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let report =
        build_signed_plugin_package(&project, base_directory, &signing_key, &arguments.output)
            .map_err(|error| error.to_string())?;
    emit_json_result(&report)
}

fn verify_plugin(arguments: PluginVerifyArgs) -> CliResult<()> {
    validate_path_for_json_report(&arguments.package, "plugin package path")?;
    let trust_store = read_trust_store(&arguments.trust_store)?;
    let verified = verify_signed_plugin_package(&arguments.package, &trust_store)
        .map_err(|error| error.to_string())?;
    emit_json_result(verified.verification())
}

fn install_plugin(arguments: PluginInstallArgs) -> CliResult<()> {
    validate_path_for_json_report(&arguments.root, "plugin install root")?;
    let trust_store = read_trust_store(&arguments.trust_store)?;
    let verified = verify_signed_plugin_package(&arguments.package, &trust_store)
        .map_err(|error| error.to_string())?;
    let report = install_verified_plugin_package(&verified, &arguments.root)
        .map_err(|error| error.to_string())?;
    emit_json_result(&report)
}

fn uninstall_installed_plugin(arguments: PluginUninstallArgs) -> CliResult<()> {
    let removed = uninstall_plugin(&arguments.root, &arguments.plugin_id, &arguments.version)
        .map_err(|error| error.to_string())?;
    emit_json_result(&serde_json::json!({
        "plugin_id": arguments.plugin_id,
        "version": arguments.version,
        "removed": removed,
    }))
}

fn list_plugins(arguments: PluginListArgs) -> CliResult<()> {
    validate_path_for_json_report(&arguments.root, "plugin install root")?;
    let plugins = list_installed_plugins(&arguments.root).map_err(|error| error.to_string())?;
    emit_json_result(&plugins)
}

fn generate_plugin_key(arguments: PluginKeyGenerateArgs) -> CliResult<()> {
    validate_path_for_json_report(&arguments.signing_key_output, "signing key output path")?;
    validate_path_for_json_report(&arguments.trust_store_output, "trust store output path")?;
    if arguments.signing_key_output == arguments.trust_store_output {
        return Err(CliError::manifest_or_package(
            "signing key and trust store outputs must be different paths",
        ));
    }
    let key = PluginSigningKey::generate(arguments.publisher).map_err(|error| error.to_string())?;
    let mut trust_store = if arguments.trust_store_output.exists() {
        let bytes = read_bounded_regular_file(
            &arguments.trust_store_output,
            MAX_PLUGIN_TRUST_STORE_BYTES,
            "plugin trust store",
        )?;
        PluginTrustStore::from_json(&bytes).map_err(|error| error.to_string())?
    } else {
        PluginTrustStore::empty()
    };
    trust_store
        .insert(key.public_key())
        .map_err(|error| error.to_string())?;
    let mut key_bytes = key.to_json().map_err(|error| error.to_string())?;
    key_bytes.push(b'\n');
    write_new_sensitive_file(&arguments.signing_key_output, &key_bytes)?;
    let mut trust_bytes = trust_store.to_json().map_err(|error| error.to_string())?;
    trust_bytes.push(b'\n');
    emit_bytes(&trust_bytes, Some(&arguments.trust_store_output))?;
    emit_json_result(&serde_json::json!({
        "publisher": key.publisher(),
        "keyId": key.key_id(),
        "signingKey": arguments.signing_key_output,
        "trustStore": arguments.trust_store_output,
    }))
}

fn validate_path_for_json_report(path: &Path, label: &str) -> CliResult<()> {
    path.to_str().map(|_| ()).ok_or_else(|| {
        CliError::manifest_or_package(format!(
            "{label} must be valid UTF-8 because it is included in JSON output"
        ))
    })
}

fn validate_regular_file_for_json(path: &Path, label: &str) -> CliResult<()> {
    validate_path_for_json_report(path, label)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        CliError::manifest_or_package(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(CliError::manifest_or_package(format!(
            "{label} '{}' is not a regular non-symlink file",
            path.display()
        )));
    }
    Ok(())
}

fn emit_json_result<T: serde::Serialize>(value: &T) -> CliResult<()> {
    let mut bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    emit_bytes(&bytes, None)
}

fn emit_bytes(bytes: &[u8], output: Option<&Path>) -> CliResult<()> {
    let Some(output) = output else {
        return io::stdout()
            .lock()
            .write_all(bytes)
            .map_err(|error| CliError::manifest_or_package(error.to_string()));
    };
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(CliError::manifest_or_package(format!(
            "output directory `{}` is not a directory",
            parent.display()
        )));
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("failed to create output staging file: {error}"))?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| format!("failed to write output staging file: {error}"))?;
    temporary.persist(output).map_err(|error| {
        format!(
            "failed to atomically replace output `{}`: {}",
            output.display(),
            error.error
        )
    })?;
    Ok(())
}

fn write_new_sensitive_file(path: &Path, bytes: &[u8]) -> CliResult<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(CliError::manifest_or_package(format!(
            "signing key output directory '{}' is not a directory",
            parent.display()
        )));
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("failed to create signing key staging file: {error}"))?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| format!("failed to write signing key staging file: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("failed to restrict signing key permissions: {error}"))?;
    }
    temporary.persist_noclobber(path).map_err(|error| {
        format!(
            "refusing to replace signing key '{}': {}",
            path.display(),
            error.error
        )
    })?;
    Ok(())
}

fn read_project_manifest(path: &Path) -> CliResult<PluginProjectManifest> {
    let bytes = read_bounded_regular_file(path, MAX_PLUGIN_MANIFEST_BYTES, "plugin manifest")?;
    let source = String::from_utf8(bytes)
        .map_err(|error| format!("plugin manifest '{}' is not UTF-8: {error}", path.display()))?;
    PluginProjectManifest::from_toml(&source)
        .map_err(|error| CliError::manifest_or_package(error.to_string()))
}

fn read_trust_store(path: &Path) -> CliResult<PluginTrustStore> {
    let bytes =
        read_bounded_regular_file(path, MAX_PLUGIN_TRUST_STORE_BYTES, "plugin trust store")?;
    PluginTrustStore::from_json(&bytes)
        .map_err(|error| CliError::manifest_or_package(error.to_string()))
}

fn read_bounded_regular_file(path: &Path, maximum_bytes: usize, label: &str) -> CliResult<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect {label} '{}': {error}", path.display()))?;
    if !metadata.file_type().is_file() {
        return Err(CliError::manifest_or_package(format!(
            "{label} '{}' is not a regular non-symlink file",
            path.display()
        )));
    }
    if metadata.len() > maximum_bytes as u64 {
        return Err(CliError::manifest_or_package(format!(
            "{label} '{}' exceeds {maximum_bytes} bytes",
            path.display()
        )));
    }
    let file = File::open(path)
        .map_err(|error| format!("failed to open {label} '{}': {error}", path.display()))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((maximum_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read {label} '{}': {error}", path.display()))?;
    if bytes.len() > maximum_bytes {
        return Err(CliError::manifest_or_package(format!(
            "{label} '{}' exceeds {maximum_bytes} bytes",
            path.display()
        )));
    }
    Ok(bytes)
}

fn read_descriptor(path: &Path) -> CliResult<PluginDescriptor> {
    let project = read_project_manifest(path)?;
    Ok(project.descriptor().clone())
}
