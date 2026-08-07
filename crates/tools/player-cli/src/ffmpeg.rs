// Apple release commands remain parseable on other hosts so they can return
// explicit compatibility errors before entering their platform implementation.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use indexmap::IndexMap;
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::external_process::{self, ExternalProcessErrorKind};
use crate::ffmpeg_source::{
    FfmpegBuildSource, FfmpegBuildSourceInputs, FfmpegSourcePolicy, FfmpegSourcePolicyError,
    FfmpegSourcePolicyErrorKind,
};

const PROFILE_CONFIG_PATH: &str = "scripts/ffmpeg-profiles.toml";
const MAX_PROFILE_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 64 * 1024;
const MAX_SOURCE_CACHE_ENTRIES: usize = 10_000;
const MAX_RELEASE_INDEX_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 1024 * 1024;
const FFMPEG_RELEASE_INDEX_URL: &str = "https://ffmpeg.org/releases/";
const RELEASE_INDEX_CONNECT_TIMEOUT_SECONDS: &str = "10";
const RELEASE_INDEX_TOTAL_TIMEOUT_SECONDS: &str = "30";
const SUPPORTED_ANDROID_ABIS: [&str; 1] = ["arm64-v8a"];
const SUPPORTED_IOS_SLICES: [&str; 2] = ["ios-arm64", "ios-simulator-arm64"];

const NETWORK_PROTOCOLS: [&str; 30] = [
    "async",
    "cache",
    "concatf",
    "crypto",
    "data",
    "ffrtmpcrypt",
    "ftp",
    "gopher",
    "gophers",
    "hls",
    "http",
    "httpproxy",
    "https",
    "icecast",
    "mmsh",
    "mmst",
    "rtmp",
    "rtmpe",
    "rtmps",
    "rtmpt",
    "rtmpte",
    "rtmpts",
    "rtp",
    "sctp",
    "srtp",
    "subfile",
    "tcp",
    "tls",
    "udp",
    "unix",
];

const TLS_PROTOCOLS: [&str; 4] = ["https", "tls", "rtmps", "rtmpts"];
const SUPPORTED_LIBRARIES: [&str; 7] = [
    "avcodec",
    "avformat",
    "avutil",
    "avfilter",
    "avdevice",
    "swscale",
    "swresample",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FfmpegErrorKind {
    Storage,
    Compatibility,
    Conformance,
    Worker,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct FfmpegError {
    kind: FfmpegErrorKind,
    message: String,
}

impl FfmpegError {
    pub(crate) fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: FfmpegErrorKind::Storage,
            message: message.into(),
        }
    }

    pub(crate) fn compatibility(message: impl Into<String>) -> Self {
        Self {
            kind: FfmpegErrorKind::Compatibility,
            message: message.into(),
        }
    }

    pub(crate) fn conformance(message: impl Into<String>) -> Self {
        Self {
            kind: FfmpegErrorKind::Conformance,
            message: message.into(),
        }
    }

    pub(crate) fn worker(message: impl Into<String>) -> Self {
        Self {
            kind: FfmpegErrorKind::Worker,
            message: message.into(),
        }
    }

    pub(crate) const fn kind(&self) -> FfmpegErrorKind {
        self.kind
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FfmpegPlatform {
    Android,
    Ios,
    All,
}

impl FfmpegPlatform {
    pub(crate) const fn profile_name(self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::Ios => "ios",
            Self::All => "all",
        }
    }

    const fn hash_platform(self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::Ios => "apple",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AndroidArtifact {
    RuntimeAar,
    Prebuilts,
}

#[derive(Debug)]
pub(crate) struct FfmpegRequest {
    pub(crate) profile: Option<String>,
    pub(crate) platform: Option<FfmpegPlatform>,
    pub(crate) list_profiles: bool,
    pub(crate) dry_run: bool,
    pub(crate) verify_only: bool,
    pub(crate) output_directory: Option<PathBuf>,
    pub(crate) android_artifact: AndroidArtifact,
    pub(crate) android_abis: Vec<String>,
    pub(crate) ios_slices: Vec<String>,
    pub(crate) extra_libraries: Vec<String>,
    pub(crate) extra_demuxers: Vec<String>,
    pub(crate) extra_muxers: Vec<String>,
    pub(crate) extra_protocols: Vec<String>,
    pub(crate) extra_decoders: Vec<String>,
    pub(crate) extra_parsers: Vec<String>,
    pub(crate) extra_bsfs: Vec<String>,
    pub(crate) extra_configure_args: Vec<String>,
    pub(crate) tls_backend: Option<String>,
    pub(crate) force: bool,
    pub(crate) acknowledge_gpl_nonfree: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FfmpegProfileIdentity {
    pub(crate) name: String,
    pub(crate) hash: String,
    pub(crate) forbid_network: bool,
    pub(crate) forbid_openssl: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeFfmpegProfile {
    pub(crate) build_profile: String,
    pub(crate) declared_profile: String,
    pub(crate) declared_platform: &'static str,
    pub(crate) profile_hash: String,
    pub(crate) tls_backend: String,
    pub(crate) enable_dash: bool,
    pub(crate) libraries: Vec<String>,
    pub(crate) demuxers: Vec<String>,
    pub(crate) muxers: Vec<String>,
    pub(crate) protocols: Vec<String>,
    pub(crate) decoders: Vec<String>,
    pub(crate) parsers: Vec<String>,
    pub(crate) bsfs: Vec<String>,
    pub(crate) extra_configure_args: Vec<String>,
    pub(crate) force: bool,
    pub(crate) forbid_network: bool,
    pub(crate) forbid_openssl: bool,
}

impl NativeFfmpegProfile {
    fn from_prepared(platform: FfmpegPlatform, prepared: &PreparedProfile) -> Self {
        let resolved = &prepared.resolved;
        Self {
            build_profile: "custom".to_owned(),
            declared_profile: prepared.name.clone(),
            declared_platform: platform.profile_name(),
            profile_hash: resolved.profile_hash(platform),
            tls_backend: resolved.tls.clone(),
            enable_dash: resolved.dash_enabled(),
            libraries: resolved.libraries.clone(),
            demuxers: resolved.demuxers.clone(),
            muxers: resolved.muxers.clone(),
            protocols: resolved.protocols.clone(),
            decoders: resolved.decoders.clone(),
            parsers: resolved.parsers.clone(),
            bsfs: resolved.bsfs.clone(),
            extra_configure_args: resolved.extra_configure_args.clone(),
            force: prepared.force,
            forbid_network: resolved.forbid_network,
            forbid_openssl: resolved.forbid_openssl,
        }
    }

    pub(crate) fn configure_arguments(&self, platform: FfmpegPlatform) -> Vec<String> {
        let mut arguments = Vec::new();
        if self.build_profile != "legacy" {
            arguments.extend([
                "--disable-everything".to_owned(),
                "--disable-programs".to_owned(),
                "--disable-doc".to_owned(),
                "--disable-debug".to_owned(),
                "--disable-autodetect".to_owned(),
                "--disable-decoders".to_owned(),
                "--disable-encoders".to_owned(),
                "--disable-parsers".to_owned(),
                "--disable-bsfs".to_owned(),
                "--disable-protocols".to_owned(),
                "--disable-demuxers".to_owned(),
                "--disable-muxers".to_owned(),
            ]);
            for library in ["avdevice", "avfilter", "swscale", "swresample"] {
                if !self.libraries.iter().any(|enabled| enabled == library) {
                    arguments.push(format!("--disable-{library}"));
                }
            }
        }
        arguments.push(
            if self.build_profile == "legacy"
                || self
                    .protocols
                    .iter()
                    .any(|protocol| is_network_protocol(protocol))
            {
                "--enable-network"
            } else {
                "--disable-network"
            }
            .to_owned(),
        );
        match (platform, self.tls_backend.as_str()) {
            (FfmpegPlatform::Android, "openssl") => {
                arguments.extend([
                    "--enable-openssl".to_owned(),
                    "--enable-version3".to_owned(),
                ]);
            }
            (FfmpegPlatform::Ios, "securetransport") => {
                arguments.push("--enable-securetransport".to_owned());
            }
            (FfmpegPlatform::Android | FfmpegPlatform::Ios, "none") => {
                arguments.extend([
                    "--disable-openssl".to_owned(),
                    "--disable-gnutls".to_owned(),
                    "--disable-mbedtls".to_owned(),
                    "--disable-securetransport".to_owned(),
                ]);
            }
            _ => {}
        }
        if self.enable_dash {
            arguments.push("--enable-libxml2".to_owned());
        }
        arguments.extend(
            self.libraries
                .iter()
                .map(|value| format!("--enable-{value}")),
        );
        arguments.extend(
            self.demuxers
                .iter()
                .map(|value| format!("--enable-demuxer={value}")),
        );
        arguments.extend(
            self.muxers
                .iter()
                .map(|value| format!("--enable-muxer={value}")),
        );
        arguments.extend(
            self.protocols
                .iter()
                .map(|value| format!("--enable-protocol={value}")),
        );
        arguments.extend(
            self.decoders
                .iter()
                .map(|value| format!("--enable-decoder={value}")),
        );
        arguments.extend(
            self.parsers
                .iter()
                .map(|value| format!("--enable-parser={value}")),
        );
        arguments.extend(
            self.bsfs
                .iter()
                .map(|value| format!("--enable-bsf={value}")),
        );
        arguments.extend(self.extra_configure_args.iter().cloned());
        arguments
    }

    pub(crate) fn external_dependencies(&self) -> Vec<&'static str> {
        let mut dependencies = Vec::with_capacity(2);
        if self.tls_backend == "openssl" {
            dependencies.push("openssl");
        }
        if self.enable_dash {
            dependencies.push("libxml2");
        }
        dependencies
    }

    pub(crate) fn license_flags(&self) -> Vec<&'static str> {
        let mut flags = Vec::with_capacity(4);
        if self.tls_backend == "openssl" {
            flags.extend(["version3", "openssl"]);
        }
        if self
            .extra_configure_args
            .iter()
            .any(|argument| flag_matches(argument, "--enable-gpl"))
        {
            flags.push("gpl");
        }
        if self
            .extra_configure_args
            .iter()
            .any(|argument| flag_matches(argument, "--enable-nonfree"))
        {
            flags.push("nonfree");
        }
        flags
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn metadata_text(
        &self,
        platform: &str,
        target: &str,
        ffmpeg_version: &str,
        source_archive: &Path,
        source_url: &str,
        source_sha256: &str,
        configure_line: &[String],
    ) -> String {
        format!(
            "Vesper FFmpeg build metadata v2\nplatform={platform}\ntarget={target}\nprofile={}\ndeclared_profile={}\ndeclared_platform={}\nprofile_hash={}\ntls_backend={}\nenable_dash={}\nlibraries={}\ndemuxers={}\nmuxers={}\nprotocols={}\ndecoders={}\nparsers={}\nbsfs={}\nexternal_dependencies={}\nlicense_flags={}\nffmpeg_version={ffmpeg_version}\nsource_archive={}\nsource_url={source_url}\nsource_sha256={source_sha256}\nconfigure_line={}\n",
            self.build_profile,
            self.declared_profile,
            self.declared_platform,
            self.profile_hash,
            self.tls_backend,
            u8::from(self.enable_dash),
            self.libraries.join(","),
            self.demuxers.join(","),
            self.muxers.join(","),
            self.protocols.join(","),
            self.decoders.join(","),
            self.parsers.join(","),
            self.bsfs.join(","),
            self.external_dependencies().join(","),
            self.license_flags().join(","),
            source_archive.display(),
            join_shell_quoted(configure_line),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppleReleaseProfile {
    pub(crate) declared_profile: String,
    pub(crate) profile_hash: String,
    pub(crate) output_directory: PathBuf,
    pub(crate) runtime_libraries: Vec<String>,
    pub(crate) worker_arguments: Vec<String>,
    pub(crate) native_profile: NativeFfmpegProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppleRawProfile {
    pub(crate) profile: String,
    pub(crate) profile_hash: String,
    pub(crate) output_directory: PathBuf,
    pub(crate) slices: Vec<String>,
    pub(crate) worker_arguments: Vec<String>,
    pub(crate) native_profile: NativeFfmpegProfile,
    pub(crate) source: Option<FfmpegBuildSource>,
}

#[derive(Debug, Default)]
struct AppleRawEnvironment {
    profile: Option<String>,
    tls_backend: Option<String>,
    enable_dash: Option<String>,
    force: bool,
    acknowledge_gpl_nonfree: bool,
    libraries: Vec<String>,
    demuxers: Vec<String>,
    muxers: Vec<String>,
    protocols: Vec<String>,
    decoders: Vec<String>,
    parsers: Vec<String>,
    bsfs: Vec<String>,
    extra_configure_args: Vec<String>,
    has_overlay: bool,
}

#[derive(Debug)]
struct AppleRawOptions {
    profile: String,
    tls_backend: Option<String>,
    enable_dash: Option<String>,
    force: bool,
    acknowledge_gpl_nonfree: bool,
    libraries: Vec<String>,
    demuxers: Vec<String>,
    muxers: Vec<String>,
    protocols: Vec<String>,
    decoders: Vec<String>,
    parsers: Vec<String>,
    bsfs: Vec<String>,
    extra_configure_args: Vec<String>,
    slices: Vec<String>,
    has_overlay: bool,
}

pub(crate) fn run(
    root: &Path,
    request: &FfmpegRequest,
    output: &mut dyn Write,
) -> Result<(), FfmpegError> {
    let profiles = FfmpegProfiles::load(root)?;
    if request.list_profiles {
        for name in profiles.names() {
            writeln!(output, "{name}").map_err(output_error)?;
        }
        return Ok(());
    }

    let platform = request
        .platform
        .ok_or_else(|| FfmpegError::conformance("--platform is required for FFmpeg builds"))?;
    validate_target_selectors(request, platform)?;
    let platforms: &[FfmpegPlatform] = match platform {
        FfmpegPlatform::Android => &[FfmpegPlatform::Android],
        FfmpegPlatform::Ios => &[FfmpegPlatform::Ios],
        FfmpegPlatform::All => &[FfmpegPlatform::Android, FfmpegPlatform::Ios],
    };

    for platform in platforms {
        let prepared = prepare_profile(&profiles, request, *platform)?;
        let profile_name = &prepared.name;
        let resolved = &prepared.resolved;
        let native_profile = NativeFfmpegProfile::from_prepared(*platform, &prepared);
        let worker_arguments =
            resolved.worker_arguments(prepared.force, prepared.acknowledge_gpl_nonfree);
        if request.dry_run {
            write_dry_run(output, profile_name, *platform, resolved, &worker_arguments)?;
            continue;
        }
        if request.verify_only {
            verify_existing_artifacts(root, request, *platform, resolved, &native_profile)?;
            continue;
        }
        run_platform_worker(root, request, *platform, &native_profile)?;
    }
    Ok(())
}

pub(crate) fn resolve_profile_identity(
    root: &Path,
    request: &FfmpegRequest,
    platform: FfmpegPlatform,
) -> Result<FfmpegProfileIdentity, FfmpegError> {
    let profiles = FfmpegProfiles::load(root)?;
    let prepared = prepare_profile(&profiles, request, platform)?;
    Ok(FfmpegProfileIdentity {
        name: prepared.name,
        hash: prepared.resolved.profile_hash(platform),
        forbid_network: prepared.resolved.forbid_network,
        forbid_openssl: prepared.resolved.forbid_openssl,
    })
}

pub(crate) fn resolve_apple_release_profile(
    root: &Path,
    declared_profile: &str,
    slices: &[String],
) -> Result<AppleReleaseProfile, FfmpegError> {
    let request = FfmpegRequest {
        profile: Some(declared_profile.to_owned()),
        platform: Some(FfmpegPlatform::Ios),
        list_profiles: false,
        dry_run: false,
        verify_only: false,
        output_directory: None,
        android_artifact: AndroidArtifact::Prebuilts,
        android_abis: Vec::new(),
        ios_slices: slices.to_vec(),
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
    };
    validate_target_selectors(&request, FfmpegPlatform::Ios)?;
    let profiles = FfmpegProfiles::load(root)?;
    let prepared = prepare_profile(&profiles, &request, FfmpegPlatform::Ios)?;
    let profile_hash = prepared.resolved.profile_hash(FfmpegPlatform::Ios);
    let output_directory = apple_output_directory(root, None, &profile_hash);
    let native_profile = NativeFfmpegProfile::from_prepared(FfmpegPlatform::Ios, &prepared);
    Ok(AppleReleaseProfile {
        declared_profile: prepared.name,
        profile_hash,
        output_directory,
        runtime_libraries: prepared.resolved.libraries.clone(),
        worker_arguments: prepared
            .resolved
            .worker_arguments(prepared.force, prepared.acknowledge_gpl_nonfree),
        native_profile,
    })
}

pub(crate) fn resolve_apple_raw_profile(
    root: &Path,
    arguments: &[OsString],
    output_directory: Option<&Path>,
    ignore_environment_overlays: bool,
) -> Result<AppleRawProfile, FfmpegError> {
    let environment = if ignore_environment_overlays {
        AppleRawEnvironment::default()
    } else {
        AppleRawEnvironment::load()?
    };
    let output_directory = output_directory
        .map(|path| root_relative_path(root, path.as_os_str()))
        .or_else(|| apple_raw_output_environment(root));
    resolve_apple_raw_profile_with_environment(root, arguments, output_directory, environment)
}

pub(crate) fn resolve_apple_raw_profile_with_source(
    root: &Path,
    arguments: &[OsString],
    output_directory: Option<&Path>,
    ignore_environment_overlays: bool,
) -> Result<AppleRawProfile, FfmpegError> {
    let mut profile = resolve_apple_raw_profile(
        root,
        arguments,
        output_directory,
        ignore_environment_overlays,
    )?;
    profile.source = Some(resolve_apple_raw_source(root)?);
    Ok(profile)
}

pub(crate) fn resolve_apple_raw_source(root: &Path) -> Result<FfmpegBuildSource, FfmpegError> {
    resolve_worker_source(root, FfmpegPlatform::Ios)
}

fn resolve_apple_raw_profile_with_environment(
    root: &Path,
    arguments: &[OsString],
    output_directory: Option<PathBuf>,
    environment: AppleRawEnvironment,
) -> Result<AppleRawProfile, FfmpegError> {
    let options = AppleRawOptions::parse(arguments, environment)?;
    let resolved = options.resolve()?;
    let profile_hash = resolved.profile_hash();
    let output_directory = output_directory.unwrap_or_else(|| {
        let base = root.join("third_party/ffmpeg/apple");
        if profile_hash == "legacy" {
            base
        } else {
            base.join("profiles").join(&profile_hash)
        }
    });
    let native_profile = resolved.native_profile(&profile_hash);
    Ok(AppleRawProfile {
        profile: resolved.profile.clone(),
        profile_hash,
        output_directory,
        slices: resolved.slices.clone(),
        worker_arguments: resolved.worker_arguments(),
        native_profile,
        source: None,
    })
}

impl AppleRawEnvironment {
    fn load() -> Result<Self, FfmpegError> {
        let (profile, _) = apple_raw_environment_scalar("PROFILE")?;
        let (tls_backend, tls_overlay) = apple_raw_environment_scalar("TLS_BACKEND")?;
        let (enable_dash, dash_overlay) = apple_raw_environment_scalar("ENABLE_DASH")?;
        let (force, _) = apple_raw_environment_scalar("FORCE")?;
        let (acknowledge_gpl_nonfree, _) = apple_raw_environment_scalar("ACKNOWLEDGE_GPL_NONFREE")?;
        let (libraries, libraries_overlay) = apple_raw_environment_list("ENABLE_LIBRARIES", true)?;
        let (demuxers, demuxers_overlay) = apple_raw_environment_list("ENABLE_DEMUXERS", true)?;
        let (muxers, muxers_overlay) = apple_raw_environment_list("ENABLE_MUXERS", true)?;
        let (protocols, protocols_overlay) = apple_raw_environment_list("ENABLE_PROTOCOLS", true)?;
        let (decoders, decoders_overlay) = apple_raw_environment_list("ENABLE_DECODERS", true)?;
        let (parsers, parsers_overlay) = apple_raw_environment_list("ENABLE_PARSERS", true)?;
        let (bsfs, bsfs_overlay) = apple_raw_environment_list("ENABLE_BSFS", true)?;
        let (extra_configure_args, extra_overlay) =
            apple_raw_environment_list("EXTRA_CONFIGURE_ARGS", false)?;
        Ok(Self {
            profile,
            tls_backend,
            enable_dash,
            force: force.as_deref() == Some("1"),
            acknowledge_gpl_nonfree: acknowledge_gpl_nonfree.as_deref() == Some("1"),
            libraries,
            demuxers,
            muxers,
            protocols,
            decoders,
            parsers,
            bsfs,
            extra_configure_args,
            has_overlay: tls_overlay
                || dash_overlay
                || libraries_overlay
                || demuxers_overlay
                || muxers_overlay
                || protocols_overlay
                || decoders_overlay
                || parsers_overlay
                || bsfs_overlay
                || extra_overlay,
        })
    }
}

impl AppleRawOptions {
    fn parse(
        arguments: &[OsString],
        environment: AppleRawEnvironment,
    ) -> Result<Self, FfmpegError> {
        let mut options = Self {
            profile: environment.profile.unwrap_or_else(|| "legacy".to_owned()),
            tls_backend: environment.tls_backend,
            enable_dash: environment.enable_dash,
            force: environment.force,
            acknowledge_gpl_nonfree: environment.acknowledge_gpl_nonfree,
            libraries: environment.libraries,
            demuxers: environment.demuxers,
            muxers: environment.muxers,
            protocols: environment.protocols,
            decoders: environment.decoders,
            parsers: environment.parsers,
            bsfs: environment.bsfs,
            extra_configure_args: environment.extra_configure_args,
            slices: Vec::new(),
            has_overlay: environment.has_overlay,
        };
        let mut index = 0;
        while index < arguments.len() {
            let argument = arguments[index].to_str().ok_or_else(|| {
                FfmpegError::conformance("Apple FFmpeg build arguments must be valid UTF-8")
            })?;
            if argument == "--" {
                for value in &arguments[index + 1..] {
                    options
                        .slices
                        .push(parse_raw_utf8_argument(value, "Apple FFmpeg slice")?);
                }
                break;
            }
            if matches!(argument, "--ffmpeg-profile" | "--profile") {
                options.profile = required_raw_argument(arguments, index, argument)?;
                index += 2;
                continue;
            }
            if let Some(value) = argument
                .strip_prefix("--ffmpeg-profile=")
                .or_else(|| argument.strip_prefix("--profile="))
            {
                options.profile = value.to_owned();
                index += 1;
                continue;
            }
            if parse_raw_list_option(
                arguments,
                &mut index,
                argument,
                "--enable-libraries",
                &mut options.libraries,
            )? || parse_raw_list_option(
                arguments,
                &mut index,
                argument,
                "--enable-demuxers",
                &mut options.demuxers,
            )? || parse_raw_list_option(
                arguments,
                &mut index,
                argument,
                "--enable-muxers",
                &mut options.muxers,
            )? || parse_raw_list_option(
                arguments,
                &mut index,
                argument,
                "--enable-protocols",
                &mut options.protocols,
            )? || parse_raw_list_option(
                arguments,
                &mut index,
                argument,
                "--enable-decoders",
                &mut options.decoders,
            )? || parse_raw_list_option(
                arguments,
                &mut index,
                argument,
                "--enable-parsers",
                &mut options.parsers,
            )? || parse_raw_list_option(
                arguments,
                &mut index,
                argument,
                "--enable-bsfs",
                &mut options.bsfs,
            )? {
                options.has_overlay = true;
                continue;
            }
            if argument == "--extra-configure-arg" {
                options
                    .extra_configure_args
                    .push(required_raw_argument(arguments, index, argument)?);
                options.has_overlay = true;
                index += 2;
                continue;
            }
            if let Some(value) = argument.strip_prefix("--extra-configure-arg=") {
                options.extra_configure_args.push(value.to_owned());
                options.has_overlay = true;
                index += 1;
                continue;
            }
            if argument == "--tls-backend" {
                options.tls_backend = Some(required_raw_argument(arguments, index, argument)?);
                options.has_overlay = true;
                index += 2;
                continue;
            }
            if let Some(value) = argument.strip_prefix("--tls-backend=") {
                options.tls_backend = Some(value.to_owned());
                options.has_overlay = true;
                index += 1;
                continue;
            }
            match argument {
                "--enable-dash" => {
                    options.enable_dash = Some("1".to_owned());
                    options.has_overlay = true;
                }
                "--disable-dash" => {
                    options.enable_dash = Some("0".to_owned());
                    options.has_overlay = true;
                }
                "--force" => options.force = true,
                "--acknowledge-gpl-nonfree" => options.acknowledge_gpl_nonfree = true,
                value if value.starts_with("--") => {
                    return Err(FfmpegError::conformance(format!(
                        "Unknown FFmpeg build option: {value}"
                    )));
                }
                value => options.slices.push(value.to_owned()),
            }
            index += 1;
        }
        if options.slices.is_empty() {
            options.slices = SUPPORTED_IOS_SLICES
                .iter()
                .map(ToString::to_string)
                .collect();
        }
        let mut selected = Vec::new();
        for slice in &options.slices {
            if !SUPPORTED_IOS_SLICES.contains(&slice.as_str()) {
                return Err(FfmpegError::compatibility(format!(
                    "Unsupported Apple slice: {slice}. Supported slices: {}",
                    SUPPORTED_IOS_SLICES.join(", ")
                )));
            }
            if selected.contains(&slice.as_str()) {
                return Err(FfmpegError::conformance(format!(
                    "Duplicate FFmpeg iOS slice: {slice}"
                )));
            }
            selected.push(slice.as_str());
        }
        Ok(options)
    }

    fn resolve(self) -> Result<ResolvedAppleRawOptions, FfmpegError> {
        if !matches!(self.profile.as_str(), "legacy" | "remux-local" | "custom") {
            return Err(FfmpegError::conformance(format!(
                "Unsupported FFmpeg profile: {}. Supported profiles: legacy, remux-local, custom",
                self.profile
            )));
        }
        validate_raw_library_values(&self.libraries)?;
        for (label, values) in [
            ("demuxer", &self.demuxers),
            ("muxer", &self.muxers),
            ("protocol", &self.protocols),
            ("decoder", &self.decoders),
            ("parser", &self.parsers),
            ("bitstream-filter", &self.bsfs),
        ] {
            validate_raw_name_values(label, values)?;
        }

        let mut libraries = Vec::new();
        let mut demuxers = Vec::new();
        let mut muxers = Vec::new();
        let mut protocols = Vec::new();
        let mut decoders = Vec::new();
        let mut parsers = Vec::new();
        let mut bsfs = Vec::new();
        if self.profile == "remux-local" {
            append_unique(&mut libraries, ["avcodec", "avformat", "avutil"]);
            append_unique(
                &mut demuxers,
                [
                    "hls", "dash", "concat", "flv", "mov", "matroska", "mpegts", "aac",
                ],
            );
            append_unique(&mut muxers, ["mp4", "mov", "matroska"]);
            append_unique(&mut protocols, ["file", "pipe"]);
            append_unique(&mut decoders, ["aac", "h264"]);
            append_unique(
                &mut parsers,
                [
                    "aac",
                    "ac3",
                    "av1",
                    "flac",
                    "h264",
                    "hevc",
                    "mpeg4video",
                    "opus",
                    "vp8",
                    "vp9",
                ],
            );
            append_unique(
                &mut bsfs,
                [
                    "aac_adtstoasc",
                    "extract_extradata",
                    "h264_metadata",
                    "hevc_metadata",
                ],
            );
        }
        append_unique(&mut libraries, self.libraries.iter().map(String::as_str));
        append_unique(&mut demuxers, self.demuxers.iter().map(String::as_str));
        append_unique(&mut muxers, self.muxers.iter().map(String::as_str));
        append_unique(&mut protocols, self.protocols.iter().map(String::as_str));
        append_unique(&mut decoders, self.decoders.iter().map(String::as_str));
        append_unique(&mut parsers, self.parsers.iter().map(String::as_str));
        append_unique(&mut bsfs, self.bsfs.iter().map(String::as_str));

        let enable_dash = match self.enable_dash.as_deref() {
            Some("0") => false,
            Some("1") => true,
            Some(value) => {
                return Err(FfmpegError::conformance(format!(
                    "FFmpeg DASH toggle must be 0 or 1, got: {value}"
                )));
            }
            None if matches!(self.profile.as_str(), "legacy" | "remux-local") => true,
            None => demuxers.iter().any(|value| value == "dash"),
        };
        if !enable_dash {
            demuxers.retain(|value| value != "dash");
        }
        let tls_backend = self
            .tls_backend
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                if self.profile == "legacy" {
                    "securetransport".to_owned()
                } else {
                    "none".to_owned()
                }
            });
        if !matches!(tls_backend.as_str(), "none" | "securetransport") {
            return Err(FfmpegError::conformance(format!(
                "Unsupported Apple FFmpeg TLS backend: {tls_backend}. Supported values: none, securetransport"
            )));
        }
        if tls_backend == "none"
            && let Some(protocol) = protocols
                .iter()
                .find(|protocol| TLS_PROTOCOLS.contains(&protocol.as_str()))
        {
            return Err(FfmpegError::conformance(format!(
                "Protocol {protocol} requires a TLS backend, but --tls-backend none was selected"
            )));
        }
        let restricted = self.extra_configure_args.iter().any(|argument| {
            flag_matches(argument, "--enable-gpl") || flag_matches(argument, "--enable-nonfree")
        });
        if restricted && !self.acknowledge_gpl_nonfree {
            return Err(FfmpegError::conformance(
                "Refusing to build FFmpeg with GPL or nonfree flags without explicit acknowledgement",
            ));
        }
        Ok(ResolvedAppleRawOptions {
            profile: self.profile,
            tls_backend,
            enable_dash,
            force: self.force,
            acknowledge_gpl_nonfree: self.acknowledge_gpl_nonfree,
            libraries,
            demuxers,
            muxers,
            protocols,
            decoders,
            parsers,
            bsfs,
            extra_configure_args: self.extra_configure_args,
            slices: self.slices,
            has_overlay: self.has_overlay,
        })
    }
}

#[derive(Debug)]
struct ResolvedAppleRawOptions {
    profile: String,
    tls_backend: String,
    enable_dash: bool,
    force: bool,
    acknowledge_gpl_nonfree: bool,
    libraries: Vec<String>,
    demuxers: Vec<String>,
    muxers: Vec<String>,
    protocols: Vec<String>,
    decoders: Vec<String>,
    parsers: Vec<String>,
    bsfs: Vec<String>,
    extra_configure_args: Vec<String>,
    slices: Vec<String>,
    has_overlay: bool,
}

impl ResolvedAppleRawOptions {
    fn native_profile(&self, profile_hash: &str) -> NativeFfmpegProfile {
        NativeFfmpegProfile {
            build_profile: self.profile.clone(),
            declared_profile: self.profile.clone(),
            declared_platform: "ios",
            profile_hash: profile_hash.to_owned(),
            tls_backend: self.tls_backend.clone(),
            enable_dash: self.enable_dash,
            libraries: self.libraries.clone(),
            demuxers: self.demuxers.clone(),
            muxers: self.muxers.clone(),
            protocols: self.protocols.clone(),
            decoders: self.decoders.clone(),
            parsers: self.parsers.clone(),
            bsfs: self.bsfs.clone(),
            extra_configure_args: self.extra_configure_args.clone(),
            force: self.force,
            forbid_network: self
                .protocols
                .iter()
                .all(|protocol| !is_network_protocol(protocol)),
            forbid_openssl: true,
        }
    }

    fn profile_hash(&self) -> String {
        if self.profile == "legacy" && !self.has_overlay {
            return "legacy".to_owned();
        }
        let seed = format!(
            "platform=apple\nprofile={}\ntls_backend={}\nenable_dash={}\nlibraries={}\ndemuxers={}\nmuxers={}\nprotocols={}\ndecoders={}\nparsers={}\nbsfs={}\nextra_configure_args={}",
            self.profile,
            self.tls_backend,
            u8::from(self.enable_dash),
            self.libraries.join(","),
            self.demuxers.join(","),
            self.muxers.join(","),
            self.protocols.join(","),
            self.decoders.join(","),
            self.parsers.join(","),
            self.bsfs.join(","),
            join_shell_quoted(&self.extra_configure_args),
        );
        let digest = hex::encode(Sha256::digest(seed.as_bytes()));
        format!("{}-{}", self.profile, &digest[..12])
    }

    fn worker_arguments(&self) -> Vec<String> {
        let mut arguments = vec!["--ffmpeg-profile".to_owned(), self.profile.clone()];
        if self.profile != "legacy" || self.has_overlay {
            arguments.extend(["--tls-backend".to_owned(), self.tls_backend.clone()]);
            push_csv_argument(&mut arguments, "--enable-libraries", &self.libraries);
            push_csv_argument(&mut arguments, "--enable-demuxers", &self.demuxers);
            push_csv_argument(&mut arguments, "--enable-muxers", &self.muxers);
            push_csv_argument(&mut arguments, "--enable-protocols", &self.protocols);
            push_csv_argument(&mut arguments, "--enable-decoders", &self.decoders);
            push_csv_argument(&mut arguments, "--enable-parsers", &self.parsers);
            push_csv_argument(&mut arguments, "--enable-bsfs", &self.bsfs);
            for argument in &self.extra_configure_args {
                arguments.push(format!("--extra-configure-arg={argument}"));
            }
            arguments.push(if self.enable_dash {
                "--enable-dash".to_owned()
            } else {
                "--disable-dash".to_owned()
            });
        }
        if self.force {
            arguments.push("--force".to_owned());
        }
        if self.acknowledge_gpl_nonfree {
            arguments.push("--acknowledge-gpl-nonfree".to_owned());
        }
        arguments.push("--".to_owned());
        arguments.extend(self.slices.iter().cloned());
        arguments
    }
}

fn parse_raw_list_option(
    arguments: &[OsString],
    index: &mut usize,
    argument: &str,
    option: &str,
    target: &mut Vec<String>,
) -> Result<bool, FfmpegError> {
    if argument == option {
        let value = required_raw_argument(arguments, *index, option)?;
        target.extend(split_raw_list(&value).map(str::to_owned));
        *index += 2;
        return Ok(true);
    }
    if let Some(value) = argument.strip_prefix(&format!("{option}=")) {
        target.extend(split_raw_list(value).map(str::to_owned));
        *index += 1;
        return Ok(true);
    }
    Ok(false)
}

fn required_raw_argument(
    arguments: &[OsString],
    index: usize,
    option: &str,
) -> Result<String, FfmpegError> {
    let value = arguments
        .get(index + 1)
        .ok_or_else(|| FfmpegError::conformance(format!("{option} requires a value")))?;
    let value = parse_raw_utf8_argument(value, option)?;
    if value.is_empty() {
        return Err(FfmpegError::conformance(format!(
            "{option} requires a value"
        )));
    }
    Ok(value)
}

fn parse_raw_utf8_argument(value: &OsStr, label: &str) -> Result<String, FfmpegError> {
    value
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| FfmpegError::conformance(format!("{label} must be valid UTF-8")))
}

fn split_raw_list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|character: char| character == ',' || is_shell_ifs_whitespace(character))
        .filter(|value| !value.is_empty())
}

fn split_shell_words(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(is_shell_ifs_whitespace)
        .filter(|value| !value.is_empty())
}

fn is_shell_ifs_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n')
}

fn validate_raw_library_values(values: &[String]) -> Result<(), FfmpegError> {
    if let Some(value) = values
        .iter()
        .find(|value| !SUPPORTED_LIBRARIES.contains(&value.as_str()))
    {
        return Err(FfmpegError::conformance(format!(
            "Unsupported FFmpeg library name: {value}. Supported libraries: {}",
            SUPPORTED_LIBRARIES.join(", ")
        )));
    }
    Ok(())
}

fn validate_raw_name_values(label: &str, values: &[String]) -> Result<(), FfmpegError> {
    if let Some(value) = values.iter().find(|value| {
        value.is_empty()
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-')
            })
    }) {
        return Err(FfmpegError::conformance(format!(
            "Invalid FFmpeg {label} name: {value}"
        )));
    }
    Ok(())
}

fn apple_raw_environment_scalar(suffix: &str) -> Result<(Option<String>, bool), FfmpegError> {
    apple_raw_environment_scalar_from_values(
        suffix,
        env::var_os(format!("VESPER_APPLE_FFMPEG_{suffix}")),
        env::var_os(format!("VESPER_FFMPEG_{suffix}")),
    )
}

fn apple_raw_environment_scalar_from_values(
    suffix: &str,
    apple: Option<OsString>,
    generic: Option<OsString>,
) -> Result<(Option<String>, bool), FfmpegError> {
    let apple_name = format!("VESPER_APPLE_FFMPEG_{suffix}");
    let apple = environment_value_from_os(&apple_name, apple)?;
    if apple.is_some() {
        return Ok((apple, true));
    }
    let generic_name = format!("VESPER_FFMPEG_{suffix}");
    let generic = environment_value_from_os(&generic_name, generic)?;
    let present = generic.is_some();
    Ok((generic, present))
}

fn apple_raw_environment_list(
    suffix: &str,
    split_commas: bool,
) -> Result<(Vec<String>, bool), FfmpegError> {
    let generic = environment_value(&format!("VESPER_FFMPEG_{suffix}"))?;
    let apple = environment_value(&format!("VESPER_APPLE_FFMPEG_{suffix}"))?;
    let present = generic.is_some() || apple.is_some();
    let mut values = Vec::new();
    for value in [generic, apple].into_iter().flatten() {
        if split_commas {
            values.extend(split_raw_list(&value).map(str::to_owned));
        } else {
            values.extend(split_shell_words(&value).map(str::to_owned));
        }
    }
    Ok((values, present))
}

fn apple_raw_output_environment(root: &Path) -> Option<PathBuf> {
    for name in ["VESPER_APPLE_FFMPEG_OUTPUT_DIR", "VESPER_FFMPEG_OUTPUT_DIR"] {
        if let Some(value) = env::var_os(name).filter(|value| !value.is_empty()) {
            return Some(root_relative_path(root, &value));
        }
    }
    None
}

pub(crate) fn apple_output_directory(
    root: &Path,
    cli_output: Option<&Path>,
    profile_hash: &str,
) -> PathBuf {
    if let Some(path) = cli_output {
        return root_relative_path(root, path.as_os_str());
    }
    for name in ["VESPER_APPLE_FFMPEG_OUTPUT_DIR", "VESPER_FFMPEG_OUTPUT_DIR"] {
        if let Some(value) = env::var_os(name).filter(|value| !value.is_empty()) {
            return root_relative_path(root, &value);
        }
    }
    root.join("third_party/ffmpeg/apple/profiles")
        .join(profile_hash)
}

pub(crate) fn android_output_directory(
    root: &Path,
    request: &FfmpegRequest,
    profile_hash: &str,
) -> PathBuf {
    android_output_directory_from(
        root,
        request.output_directory.as_deref(),
        env::var_os("VESPER_ANDROID_FFMPEG_OUTPUT_DIR")
            .filter(|value| !value.is_empty())
            .as_deref(),
        env::var_os("VESPER_FFMPEG_OUTPUT_DIR")
            .filter(|value| !value.is_empty())
            .as_deref(),
        profile_hash,
    )
}

fn android_output_directory_from(
    root: &Path,
    cli_output: Option<&Path>,
    android_environment_output: Option<&OsStr>,
    generic_environment_output: Option<&OsStr>,
    profile_hash: &str,
) -> PathBuf {
    if let Some(path) = cli_output {
        return root_relative_path(root, path.as_os_str());
    }
    if let Some(path) = android_environment_output.or(generic_environment_output) {
        return root_relative_path(root, path);
    }
    let base = root.join("third_party/ffmpeg/android");
    if profile_hash == "legacy" {
        base
    } else {
        base.join("profiles").join(profile_hash)
    }
}

fn root_relative_path(root: &Path, value: &OsStr) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn validate_target_selectors(
    request: &FfmpegRequest,
    platform: FfmpegPlatform,
) -> Result<(), FfmpegError> {
    if platform == FfmpegPlatform::Android && !request.ios_slices.is_empty() {
        return Err(FfmpegError::conformance(
            "--slice is only valid for iOS or all-platform FFmpeg builds",
        ));
    }
    if platform == FfmpegPlatform::Ios && !request.android_abis.is_empty() {
        return Err(FfmpegError::conformance(
            "--abi is only valid for Android or all-platform FFmpeg builds",
        ));
    }
    validate_selector_values(
        "Android ABI",
        "--abi",
        &request.android_abis,
        &SUPPORTED_ANDROID_ABIS,
    )?;
    validate_selector_values(
        "iOS slice",
        "--slice",
        &request.ios_slices,
        &SUPPORTED_IOS_SLICES,
    )
}

fn validate_selector_values(
    label: &str,
    option: &str,
    raw_values: &[String],
    supported: &[&str],
) -> Result<(), FfmpegError> {
    for raw in raw_values {
        if list_tokens(std::slice::from_ref(raw)).next().is_none() {
            return Err(FfmpegError::conformance(format!(
                "{option} requires a non-empty {label}"
            )));
        }
    }
    let mut selected = Vec::new();
    for value in list_tokens(raw_values) {
        if !supported.contains(&value) {
            return Err(FfmpegError::compatibility(format!(
                "Unsupported FFmpeg {label}: {value}. Supported values: {}",
                supported.join(", ")
            )));
        }
        if selected.contains(&value) {
            return Err(FfmpegError::conformance(format!(
                "Duplicate FFmpeg {label}: {value}"
            )));
        }
        selected.push(value);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FfmpegProfilesDocument {
    profile: IndexMap<String, ProfileDefinition>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ProfileParents {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileDefinition {
    #[serde(default)]
    extends: Option<ProfileParents>,
    #[serde(default)]
    libraries: Vec<String>,
    #[serde(default)]
    demuxers: Vec<String>,
    #[serde(default)]
    muxers: Vec<String>,
    #[serde(default)]
    protocols: Vec<String>,
    #[serde(default)]
    decoders: Vec<String>,
    #[serde(default)]
    parsers: Vec<String>,
    #[serde(default)]
    bsfs: Vec<String>,
    #[serde(default)]
    extra_configure_args: Vec<String>,
    #[serde(default)]
    tls: Option<String>,
    #[serde(default)]
    validation: ProfileValidation,
    #[serde(default)]
    platform_overrides: IndexMap<String, ProfileOverride>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileOverride {
    #[serde(default)]
    libraries: Vec<String>,
    #[serde(default)]
    demuxers: Vec<String>,
    #[serde(default)]
    muxers: Vec<String>,
    #[serde(default)]
    protocols: Vec<String>,
    #[serde(default)]
    decoders: Vec<String>,
    #[serde(default)]
    parsers: Vec<String>,
    #[serde(default)]
    bsfs: Vec<String>,
    #[serde(default)]
    extra_configure_args: Vec<String>,
    #[serde(default)]
    tls: Option<String>,
    #[serde(default)]
    validation: ProfileValidation,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileValidation {
    #[serde(default)]
    forbid_network: Option<bool>,
    #[serde(default)]
    forbid_openssl: Option<bool>,
}

#[derive(Debug)]
struct FfmpegProfiles {
    profiles: IndexMap<String, ProfileDefinition>,
}

impl FfmpegProfiles {
    fn load(root: &Path) -> Result<Self, FfmpegError> {
        let path = env::var_os("VESPER_FFMPEG_PROFILE_CONFIG_PATH")
            .filter(|value| !value.is_empty())
            .map(|value| root_relative_path(root, &value))
            .unwrap_or_else(|| root.join(PROFILE_CONFIG_PATH));
        let metadata = fs::metadata(&path).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to inspect FFmpeg profile config '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() {
            return Err(FfmpegError::conformance(format!(
                "FFmpeg profile config must be a regular file: {}",
                path.display()
            )));
        }
        if metadata.len() > MAX_PROFILE_CONFIG_BYTES {
            return Err(FfmpegError::conformance(format!(
                "FFmpeg profile config exceeds {MAX_PROFILE_CONFIG_BYTES} bytes: {}",
                path.display()
            )));
        }
        let source = fs::read_to_string(&path).map_err(|error| {
            FfmpegError::storage(format!(
                "failed to read FFmpeg profile config '{}': {error}",
                path.display()
            ))
        })?;
        Self::parse(&source, &path)
    }

    fn parse(source: &str, path: &Path) -> Result<Self, FfmpegError> {
        let document: FfmpegProfilesDocument = toml::from_str(source).map_err(|error| {
            FfmpegError::conformance(format!(
                "invalid FFmpeg profile config '{}': {error}",
                path.display()
            ))
        })?;
        if document.profile.is_empty() {
            return Err(FfmpegError::conformance(format!(
                "FFmpeg profile config contains no profiles: {}",
                path.display()
            )));
        }
        Ok(Self {
            profiles: document.profile,
        })
    }

    fn names(&self) -> impl Iterator<Item = &str> {
        self.profiles.keys().map(String::as_str)
    }

    fn resolve(
        &self,
        name: &str,
        platform: FfmpegPlatform,
    ) -> Result<ResolvedProfile, FfmpegError> {
        let mut resolved = ResolvedProfile::default();
        let mut stack = Vec::new();
        self.resolve_into(name, platform, &mut stack, &mut resolved)?;
        if resolved.tls.is_empty() {
            resolved.tls = "none".to_owned();
        }
        Ok(resolved)
    }

    fn resolve_into(
        &self,
        name: &str,
        platform: FfmpegPlatform,
        stack: &mut Vec<String>,
        resolved: &mut ResolvedProfile,
    ) -> Result<(), FfmpegError> {
        let profile = self.profiles.get(name).ok_or_else(|| {
            FfmpegError::conformance(format!(
                "Unknown FFmpeg profile: {name}\nKnown profiles:\n  {}",
                self.names().collect::<Vec<_>>().join("\n  ")
            ))
        })?;
        if stack.iter().any(|active| active == name) {
            return Err(FfmpegError::conformance(format!(
                "FFmpeg profile inheritance cycle at: {name}"
            )));
        }
        stack.push(name.to_owned());
        match profile.extends.as_ref() {
            None => {}
            Some(ProfileParents::One(parent)) => {
                if !parent.is_empty() {
                    self.resolve_into(parent, platform, stack, resolved)?;
                }
            }
            Some(ProfileParents::Many(parents)) => {
                for parent in parents {
                    if !parent.is_empty() {
                        self.resolve_into(parent, platform, stack, resolved)?;
                    }
                }
            }
        }
        resolved.apply_definition(profile);
        if let Some(overrides) = profile.platform_overrides.get(platform.profile_name()) {
            resolved.apply_override(overrides);
        }
        stack.pop();
        Ok(())
    }
}

#[derive(Debug)]
struct PreparedProfile {
    name: String,
    resolved: ResolvedProfile,
    force: bool,
    acknowledge_gpl_nonfree: bool,
}

fn prepare_profile(
    profiles: &FfmpegProfiles,
    request: &FfmpegRequest,
    platform: FfmpegPlatform,
) -> Result<PreparedProfile, FfmpegError> {
    let name = selected_profile_name(request, platform)?;
    let mut resolved = profiles.resolve(&name, platform)?;
    let environment = resolved.apply_environment_overlays(platform)?;
    resolved.apply_request_overlays(request);
    resolved.finalize_dash_policy(request, environment.enable_dash);
    let force = request.force || environment.force;
    let acknowledge_gpl_nonfree =
        request.acknowledge_gpl_nonfree || environment.acknowledge_gpl_nonfree;
    resolved.validate(platform, acknowledge_gpl_nonfree)?;
    Ok(PreparedProfile {
        name,
        resolved,
        force,
        acknowledge_gpl_nonfree,
    })
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ResolvedProfile {
    libraries: Vec<String>,
    demuxers: Vec<String>,
    muxers: Vec<String>,
    protocols: Vec<String>,
    decoders: Vec<String>,
    parsers: Vec<String>,
    bsfs: Vec<String>,
    extra_configure_args: Vec<String>,
    tls: String,
    forbid_network: bool,
    forbid_openssl: bool,
    enable_dash: Option<bool>,
}

impl ResolvedProfile {
    fn apply_definition(&mut self, profile: &ProfileDefinition) {
        self.apply_values(
            &profile.libraries,
            &profile.demuxers,
            &profile.muxers,
            &profile.protocols,
            &profile.decoders,
            &profile.parsers,
            &profile.bsfs,
            &profile.extra_configure_args,
            profile.tls.as_deref(),
            &profile.validation,
        );
    }

    fn apply_override(&mut self, profile: &ProfileOverride) {
        self.apply_values(
            &profile.libraries,
            &profile.demuxers,
            &profile.muxers,
            &profile.protocols,
            &profile.decoders,
            &profile.parsers,
            &profile.bsfs,
            &profile.extra_configure_args,
            profile.tls.as_deref(),
            &profile.validation,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_values(
        &mut self,
        libraries: &[String],
        demuxers: &[String],
        muxers: &[String],
        protocols: &[String],
        decoders: &[String],
        parsers: &[String],
        bsfs: &[String],
        extra_configure_args: &[String],
        tls: Option<&str>,
        validation: &ProfileValidation,
    ) {
        append_unique(&mut self.libraries, libraries.iter().map(String::as_str));
        append_unique(&mut self.demuxers, demuxers.iter().map(String::as_str));
        append_unique(&mut self.muxers, muxers.iter().map(String::as_str));
        append_unique(&mut self.protocols, protocols.iter().map(String::as_str));
        append_unique(&mut self.decoders, decoders.iter().map(String::as_str));
        append_unique(&mut self.parsers, parsers.iter().map(String::as_str));
        append_unique(&mut self.bsfs, bsfs.iter().map(String::as_str));
        append_unique(
            &mut self.extra_configure_args,
            extra_configure_args.iter().map(String::as_str),
        );
        if let Some(tls) = tls.filter(|value| !value.is_empty()) {
            self.tls = tls.to_owned();
        }
        if let Some(value) = validation.forbid_network {
            self.forbid_network = value;
        }
        if let Some(value) = validation.forbid_openssl {
            self.forbid_openssl = value;
        }
    }

    fn apply_request_overlays(&mut self, request: &FfmpegRequest) {
        append_unique(&mut self.libraries, list_tokens(&request.extra_libraries));
        append_unique(&mut self.demuxers, list_tokens(&request.extra_demuxers));
        append_unique(&mut self.muxers, list_tokens(&request.extra_muxers));
        append_unique(&mut self.protocols, list_tokens(&request.extra_protocols));
        append_unique(&mut self.decoders, list_tokens(&request.extra_decoders));
        append_unique(&mut self.parsers, list_tokens(&request.extra_parsers));
        append_unique(&mut self.bsfs, list_tokens(&request.extra_bsfs));
        self.extra_configure_args
            .extend(request.extra_configure_args.iter().cloned());
        if let Some(tls) = request
            .tls_backend
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            self.tls = tls.to_owned();
        }
    }

    fn apply_environment_overlays(
        &mut self,
        platform: FfmpegPlatform,
    ) -> Result<EnvironmentOptions, FfmpegError> {
        append_unique(
            &mut self.libraries,
            environment_list_values(platform, "ENABLE_LIBRARIES")?
                .iter()
                .map(String::as_str),
        );
        append_unique(
            &mut self.demuxers,
            environment_list_values(platform, "ENABLE_DEMUXERS")?
                .iter()
                .map(String::as_str),
        );
        append_unique(
            &mut self.muxers,
            environment_list_values(platform, "ENABLE_MUXERS")?
                .iter()
                .map(String::as_str),
        );
        append_unique(
            &mut self.protocols,
            environment_list_values(platform, "ENABLE_PROTOCOLS")?
                .iter()
                .map(String::as_str),
        );
        append_unique(
            &mut self.decoders,
            environment_list_values(platform, "ENABLE_DECODERS")?
                .iter()
                .map(String::as_str),
        );
        append_unique(
            &mut self.parsers,
            environment_list_values(platform, "ENABLE_PARSERS")?
                .iter()
                .map(String::as_str),
        );
        append_unique(
            &mut self.bsfs,
            environment_list_values(platform, "ENABLE_BSFS")?
                .iter()
                .map(String::as_str),
        );
        self.extra_configure_args
            .extend(environment_word_values(platform, "EXTRA_CONFIGURE_ARGS")?);
        if let Some(tls) = environment_scalar(platform, "TLS_BACKEND")? {
            self.tls = tls;
        }
        Ok(EnvironmentOptions {
            enable_dash: environment_bool(platform, "ENABLE_DASH")?,
            force: environment_bool(platform, "FORCE")?.unwrap_or(false),
            acknowledge_gpl_nonfree: environment_bool(platform, "ACKNOWLEDGE_GPL_NONFREE")?
                .unwrap_or(false),
        })
    }

    fn finalize_dash_policy(
        &mut self,
        request: &FfmpegRequest,
        environment_override: Option<bool>,
    ) {
        let cli_enables_dash = list_tokens(&request.extra_demuxers).any(|value| value == "dash");
        let enabled = if cli_enables_dash {
            true
        } else {
            environment_override
                .unwrap_or_else(|| self.demuxers.iter().any(|value| value == "dash"))
        };
        if !enabled {
            self.demuxers.retain(|value| value != "dash");
        }
        self.enable_dash = Some(enabled);
    }

    fn validate(
        &self,
        platform: FfmpegPlatform,
        acknowledge_gpl_nonfree: bool,
    ) -> Result<(), FfmpegError> {
        for library in &self.libraries {
            if !SUPPORTED_LIBRARIES.contains(&library.as_str()) {
                return Err(FfmpegError::conformance(format!(
                    "Unsupported FFmpeg library name: {library}\nSupported libraries: {}",
                    SUPPORTED_LIBRARIES.join(", ")
                )));
            }
        }
        let name = Regex::new(r"^[A-Za-z0-9_.+\-]+$")
            .map_err(|error| FfmpegError::worker(format!("invalid name validator: {error}")))?;
        for (label, values) in [
            ("demuxer", &self.demuxers),
            ("muxer", &self.muxers),
            ("protocol", &self.protocols),
            ("decoder", &self.decoders),
            ("parser", &self.parsers),
            ("bitstream-filter", &self.bsfs),
        ] {
            if let Some(value) = values.iter().find(|value| !name.is_match(value)) {
                return Err(FfmpegError::conformance(format!(
                    "Invalid FFmpeg {label} name: {value}"
                )));
            }
        }
        let tls_supported = match platform {
            FfmpegPlatform::Android => matches!(self.tls.as_str(), "none" | "openssl"),
            FfmpegPlatform::Ios => matches!(self.tls.as_str(), "none" | "securetransport"),
            FfmpegPlatform::All => false,
        };
        if !tls_supported {
            return Err(FfmpegError::conformance(format!(
                "Unsupported {} FFmpeg TLS backend: {}",
                platform.profile_name(),
                self.tls
            )));
        }
        if self.forbid_network {
            if let Some(protocol) = self
                .protocols
                .iter()
                .find(|protocol| is_network_protocol(protocol))
            {
                return Err(FfmpegError::conformance(format!(
                    "FFmpeg profile forbids network but enables protocol: {protocol}"
                )));
            }
            for argument in &self.extra_configure_args {
                if argument == "--enable-network" {
                    return Err(FfmpegError::conformance(format!(
                        "FFmpeg profile forbids network but enables configure flag: {argument}"
                    )));
                }
                if let Some(protocols) = argument.strip_prefix("--enable-protocol=") {
                    for protocol in protocols
                        .split(|character: char| character == ',' || character.is_whitespace())
                        .filter(|value| !value.is_empty())
                    {
                        if is_network_protocol(protocol) {
                            return Err(FfmpegError::conformance(format!(
                                "FFmpeg profile forbids network but enables protocol configure flag: {argument}"
                            )));
                        }
                    }
                }
            }
        }
        if self.forbid_openssl && self.tls == "openssl" {
            return Err(FfmpegError::conformance(
                "FFmpeg profile forbids OpenSSL but selects tls=openssl",
            ));
        }
        if self.forbid_openssl
            && self
                .extra_configure_args
                .iter()
                .any(|argument| flag_matches(argument, "--enable-openssl"))
        {
            return Err(FfmpegError::conformance(
                "FFmpeg profile forbids OpenSSL but enables a configure flag",
            ));
        }
        if self.tls == "none"
            && let Some(protocol) = self
                .protocols
                .iter()
                .find(|protocol| TLS_PROTOCOLS.contains(&protocol.as_str()))
        {
            return Err(FfmpegError::conformance(format!(
                "Protocol {protocol} requires a TLS backend, but --tls-backend none was selected"
            )));
        }
        let uses_restricted_license = self.extra_configure_args.iter().any(|argument| {
            flag_matches(argument, "--enable-gpl") || flag_matches(argument, "--enable-nonfree")
        });
        if uses_restricted_license && !acknowledge_gpl_nonfree {
            return Err(FfmpegError::conformance(
                "Refusing to build FFmpeg with GPL or nonfree flags without explicit acknowledgement",
            ));
        }
        Ok(())
    }

    fn worker_arguments(&self, force: bool, acknowledge_gpl_nonfree: bool) -> Vec<String> {
        let mut arguments = vec![
            "--ffmpeg-profile".to_owned(),
            "custom".to_owned(),
            "--tls-backend".to_owned(),
            self.tls.clone(),
        ];
        push_csv_argument(&mut arguments, "--enable-libraries", &self.libraries);
        push_csv_argument(&mut arguments, "--enable-demuxers", &self.demuxers);
        push_csv_argument(&mut arguments, "--enable-muxers", &self.muxers);
        push_csv_argument(&mut arguments, "--enable-protocols", &self.protocols);
        push_csv_argument(&mut arguments, "--enable-decoders", &self.decoders);
        push_csv_argument(&mut arguments, "--enable-parsers", &self.parsers);
        push_csv_argument(&mut arguments, "--enable-bsfs", &self.bsfs);
        for argument in &self.extra_configure_args {
            arguments.push("--extra-configure-arg".to_owned());
            arguments.push(argument.clone());
        }
        arguments.push(if self.dash_enabled() {
            "--enable-dash".to_owned()
        } else {
            "--disable-dash".to_owned()
        });
        if force {
            arguments.push("--force".to_owned());
        }
        if acknowledge_gpl_nonfree {
            arguments.push("--acknowledge-gpl-nonfree".to_owned());
        }
        arguments
    }

    fn profile_hash(&self, platform: FfmpegPlatform) -> String {
        let seed = format!(
            "platform={}\nprofile=custom\ntls_backend={}\nenable_dash={}\nlibraries={}\ndemuxers={}\nmuxers={}\nprotocols={}\ndecoders={}\nparsers={}\nbsfs={}\nextra_configure_args={}",
            platform.hash_platform(),
            self.tls,
            u8::from(self.dash_enabled()),
            self.libraries.join(","),
            self.demuxers.join(","),
            self.muxers.join(","),
            self.protocols.join(","),
            self.decoders.join(","),
            self.parsers.join(","),
            self.bsfs.join(","),
            join_shell_quoted(&self.extra_configure_args),
        );
        let digest = hex::encode(Sha256::digest(seed.as_bytes()));
        format!("custom-{}", &digest[..12])
    }

    fn dash_enabled(&self) -> bool {
        self.enable_dash
            .unwrap_or_else(|| self.demuxers.iter().any(|value| value == "dash"))
    }
}

#[derive(Debug, Clone, Copy)]
struct EnvironmentOptions {
    enable_dash: Option<bool>,
    force: bool,
    acknowledge_gpl_nonfree: bool,
}

fn selected_profile_name(
    request: &FfmpegRequest,
    platform: FfmpegPlatform,
) -> Result<String, FfmpegError> {
    if let Some(profile) = request.profile.as_deref() {
        return Ok(profile.to_owned());
    }
    Ok(environment_scalar(platform, "PROFILE")?.unwrap_or_else(|| "default".to_owned()))
}

fn environment_platform_key(platform: FfmpegPlatform) -> &'static str {
    match platform {
        FfmpegPlatform::Android => "ANDROID",
        FfmpegPlatform::Ios => "APPLE",
        FfmpegPlatform::All => "ALL",
    }
}

fn environment_value(name: &str) -> Result<Option<String>, FfmpegError> {
    environment_value_from_os(name, env::var_os(name))
}

fn environment_value_from_os(
    name: &str,
    value: Option<OsString>,
) -> Result<Option<String>, FfmpegError> {
    match value {
        None => Ok(None),
        Some(value) if value.is_empty() => Ok(None),
        Some(value) => value.into_string().map(Some).map_err(|_| {
            FfmpegError::conformance(format!("FFmpeg environment value is not UTF-8: {name}"))
        }),
    }
}

fn environment_scalar(
    platform: FfmpegPlatform,
    suffix: &str,
) -> Result<Option<String>, FfmpegError> {
    let specific = format!(
        "VESPER_{}_FFMPEG_{suffix}",
        environment_platform_key(platform)
    );
    if let Some(value) = environment_value(&specific)? {
        return Ok(Some(value));
    }
    environment_value(&format!("VESPER_FFMPEG_{suffix}"))
}

fn environment_list_values(
    platform: FfmpegPlatform,
    suffix: &str,
) -> Result<Vec<String>, FfmpegError> {
    let mut values = Vec::new();
    for name in [
        format!("VESPER_FFMPEG_{suffix}"),
        format!(
            "VESPER_{}_FFMPEG_{suffix}",
            environment_platform_key(platform)
        ),
    ] {
        if let Some(value) = environment_value(&name)? {
            values.extend(
                value
                    .split(|character: char| character == ',' || character.is_whitespace())
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
            );
        }
    }
    Ok(values)
}

fn environment_word_values(
    platform: FfmpegPlatform,
    suffix: &str,
) -> Result<Vec<String>, FfmpegError> {
    let mut values = Vec::new();
    for name in [
        format!("VESPER_FFMPEG_{suffix}"),
        format!(
            "VESPER_{}_FFMPEG_{suffix}",
            environment_platform_key(platform)
        ),
    ] {
        if let Some(value) = environment_value(&name)? {
            values.extend(value.split_whitespace().map(str::to_owned));
        }
    }
    Ok(values)
}

fn environment_bool(platform: FfmpegPlatform, suffix: &str) -> Result<Option<bool>, FfmpegError> {
    let Some(value) = environment_scalar(platform, suffix)? else {
        return Ok(None);
    };
    match value.as_str() {
        "1" | "true" | "TRUE" | "yes" | "YES" => Ok(Some(true)),
        "0" | "false" | "FALSE" | "no" | "NO" => Ok(Some(false)),
        _ => Err(FfmpegError::conformance(format!(
            "FFmpeg environment boolean VESPER_*_FFMPEG_{suffix} has invalid value: {value}"
        ))),
    }
}

fn append_unique<'a>(target: &mut Vec<String>, values: impl IntoIterator<Item = &'a str>) {
    for value in values {
        if !value.is_empty() && !target.iter().any(|existing| existing == value) {
            target.push(value.to_owned());
        }
    }
}

fn list_tokens(values: &[String]) -> impl Iterator<Item = &str> {
    values
        .iter()
        .flat_map(|value| {
            value.split(|character: char| character == ',' || character.is_whitespace())
        })
        .filter(|value| !value.is_empty())
}

fn push_csv_argument(arguments: &mut Vec<String>, option: &str, values: &[String]) {
    if !values.is_empty() {
        arguments.push(option.to_owned());
        arguments.push(values.join(","));
    }
}

fn is_network_protocol(value: &str) -> bool {
    NETWORK_PROTOCOLS.contains(&value)
}

fn flag_matches(argument: &str, flag: &str) -> bool {
    argument == flag
        || argument
            .strip_prefix(flag)
            .is_some_and(|suffix| suffix.starts_with('='))
}

fn join_shell_quoted(values: &[String]) -> String {
    values
        .iter()
        .map(|value| {
            if value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'_' | b'@' | b'%' | b'+' | b'=' | b':' | b',' | b'.' | b'/' | b'-'
                    )
            }) {
                value.clone()
            } else {
                format!("'{}'", value.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn write_dry_run(
    output: &mut dyn Write,
    profile_name: &str,
    platform: FfmpegPlatform,
    profile: &ResolvedProfile,
    arguments: &[String],
) -> Result<(), FfmpegError> {
    writeln!(output, "Resolved FFmpeg profile:").map_err(output_error)?;
    writeln!(output, "profile={profile_name}").map_err(output_error)?;
    writeln!(output, "platform={}", platform.profile_name()).map_err(output_error)?;
    for (key, values) in [
        ("libraries", &profile.libraries),
        ("demuxers", &profile.demuxers),
        ("muxers", &profile.muxers),
        ("protocols", &profile.protocols),
        ("decoders", &profile.decoders),
        ("parsers", &profile.parsers),
        ("bsfs", &profile.bsfs),
    ] {
        writeln!(output, "{key}={}", values.join(",")).map_err(output_error)?;
    }
    writeln!(output, "tls={}", profile.tls).map_err(output_error)?;
    writeln!(
        output,
        "validation.forbid_network={}",
        profile.forbid_network
    )
    .map_err(output_error)?;
    writeln!(
        output,
        "validation.forbid_openssl={}",
        profile.forbid_openssl
    )
    .map_err(output_error)?;
    writeln!(output, "profile_hash={}", profile.profile_hash(platform)).map_err(output_error)?;
    writeln!(output, "Build arguments:").map_err(output_error)?;
    for argument in arguments {
        writeln!(output, "  {}", bash_display_word(argument)).map_err(output_error)?;
    }
    Ok(())
}

fn bash_display_word(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'_' | b'@' | b'%' | b'+' | b'=' | b':' | b'.' | b'/' | b'-'
            )
    }) {
        return value.to_owned();
    }
    let mut escaped = String::with_capacity(value.len() * 2);
    for character in value.chars() {
        if character.is_ascii_alphanumeric()
            || matches!(
                character,
                '_' | '@' | '%' | '+' | '=' | ':' | '.' | '/' | '-'
            )
        {
            escaped.push(character);
        } else {
            escaped.push('\\');
            escaped.push(character);
        }
    }
    escaped
}

pub(crate) fn resolve_worker_source(
    root: &Path,
    platform: FfmpegPlatform,
) -> Result<FfmpegBuildSource, FfmpegError> {
    resolve_build_source_for_platform_key(root, environment_platform_key(platform))
}

pub(crate) fn resolve_desktop_source(root: &Path) -> Result<FfmpegBuildSource, FfmpegError> {
    resolve_build_source_for_platform_key(root, "DESKTOP")
}

fn resolve_build_source_for_platform_key(
    root: &Path,
    platform_name: &str,
) -> Result<FfmpegBuildSource, FfmpegError> {
    let policy = FfmpegSourcePolicy::load_for_build(root).map_err(|error| match error.kind() {
        FfmpegSourcePolicyErrorKind::Storage => FfmpegError::storage(error.to_string()),
        FfmpegSourcePolicyErrorKind::Invalid => FfmpegError::conformance(error.to_string()),
    })?;
    let platform_version = environment_value(&format!("VESPER_{platform_name}_FFMPEG_VERSION"))?;
    let generic_version = environment_value("VESPER_FFMPEG_VERSION")?;
    let platform_series = environment_value(&format!("VESPER_{platform_name}_FFMPEG_SERIES"))?;
    let generic_series = environment_value("VESPER_FFMPEG_SERIES")?;
    let source_url = environment_value(&format!("VESPER_{platform_name}_FFMPEG_SOURCE_URL"))?;
    let inputs = FfmpegBuildSourceInputs {
        platform_version,
        generic_version,
        platform_series,
        generic_series,
        source_url,
    };
    if inputs.platform_version.is_some() || inputs.generic_version.is_some() {
        return policy
            .resolve_build_source(&inputs, std::iter::empty(), None)
            .map_err(map_source_resolution_error);
    }
    let cached_archive_names = source_cache_archive_names(root)?;
    let mut remote_index = None;
    let series = inputs
        .platform_series
        .as_deref()
        .or(inputs.generic_series.as_deref())
        .unwrap_or(policy.default_series());
    let cached_resolution = policy
        .resolve_series_version(series, cached_archive_names.clone(), None)
        .map_err(map_source_resolution_error)?;
    if cached_resolution.used_fallback {
        remote_index = fetch_release_index()?;
    }

    policy
        .resolve_build_source(&inputs, cached_archive_names, remote_index.as_deref())
        .map_err(map_source_resolution_error)
}

fn map_source_resolution_error(error: FfmpegSourcePolicyError) -> FfmpegError {
    match error.kind() {
        FfmpegSourcePolicyErrorKind::Storage => FfmpegError::storage(error.to_string()),
        FfmpegSourcePolicyErrorKind::Invalid => FfmpegError::compatibility(error.to_string()),
    }
}

fn source_cache_archive_names(root: &Path) -> Result<Vec<String>, FfmpegError> {
    let cache_dir = env::var_os("VESPER_THIRD_PARTY_SOURCE_CACHE_DIR")
        .map(|value| root_relative_path(root, &value))
        .unwrap_or_else(|| root.join("third_party/_cache"));
    let entries = match fs::read_dir(&cache_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(FfmpegError::storage(format!(
                "failed to enumerate FFmpeg source cache '{}': {error}",
                cache_dir.display()
            )));
        }
    };
    let mut names = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_SOURCE_CACHE_ENTRIES {
            return Err(FfmpegError::conformance(format!(
                "FFmpeg source cache contains more than {MAX_SOURCE_CACHE_ENTRIES} entries: {}",
                cache_dir.display()
            )));
        }
        let entry = entry.map_err(|error| {
            FfmpegError::storage(format!(
                "failed to read FFmpeg source cache entry '{}': {error}",
                cache_dir.display()
            ))
        })?;
        let metadata = entry.metadata().map_err(|error| {
            FfmpegError::storage(format!(
                "failed to inspect FFmpeg source cache entry '{}': {error}",
                entry.path().display()
            ))
        })?;
        if metadata.file_type().is_file()
            && let Some(name) = entry.file_name().to_str()
        {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

fn fetch_release_index() -> Result<Option<String>, FfmpegError> {
    let curl = env::var_os("CURL").unwrap_or_else(|| OsStr::new("curl").to_owned());
    let mut command = Command::new(curl);
    command.args([
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--connect-timeout",
        RELEASE_INDEX_CONNECT_TIMEOUT_SECONDS,
        "--max-time",
        RELEASE_INDEX_TOTAL_TIMEOUT_SECONDS,
        FFMPEG_RELEASE_INDEX_URL,
    ]);
    let captured = match external_process::run_interruptible_capture(
        &mut command,
        "FFmpeg release index lookup",
        MAX_RELEASE_INDEX_BYTES,
        MAX_TOOL_OUTPUT_BYTES,
    ) {
        Ok(captured) => captured,
        Err(error) if error.kind() == ExternalProcessErrorKind::Cancelled => {
            return Err(map_process_error(error));
        }
        Err(_) => return Ok(None),
    };
    if !captured.status.success() {
        return Ok(None);
    }
    Ok(String::from_utf8(captured.stdout).ok())
}

fn run_platform_worker(
    root: &Path,
    request: &FfmpegRequest,
    platform: FfmpegPlatform,
    native_profile: &NativeFfmpegProfile,
) -> Result<(), FfmpegError> {
    let source = resolve_worker_source(root, platform)?;
    match platform {
        FfmpegPlatform::Android => {
            crate::ffmpeg_android::run(root, request, native_profile, &source)
        }
        FfmpegPlatform::Ios => crate::ffmpeg_apple::run(root, request, native_profile, &source),
        FfmpegPlatform::All => Err(FfmpegError::worker(
            "internal FFmpeg platform dispatch received 'all'",
        )),
    }
}

pub(crate) fn flatten_list_values(values: &[String]) -> Vec<String> {
    list_tokens(values).map(str::to_owned).collect()
}

fn map_process_error(error: external_process::ExternalProcessError) -> FfmpegError {
    match error.kind() {
        ExternalProcessErrorKind::Compatibility => FfmpegError::compatibility(error.to_string()),
        ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
            FfmpegError::worker(error.to_string())
        }
    }
}

fn verify_existing_artifacts(
    root: &Path,
    request: &FfmpegRequest,
    platform: FfmpegPlatform,
    profile: &ResolvedProfile,
    native_profile: &NativeFfmpegProfile,
) -> Result<(), FfmpegError> {
    match platform {
        FfmpegPlatform::Android => {
            let profile_hash = profile.profile_hash(FfmpegPlatform::Android);
            let prebuilts = android_output_directory(root, request, &profile_hash);
            verify_android_prebuilts(&prebuilts, request, profile, &profile_hash)?;
            if request.android_artifact == AndroidArtifact::RuntimeAar {
                let abis = if request.android_abis.is_empty() {
                    SUPPORTED_ANDROID_ABIS
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                } else {
                    flatten_list_values(&request.android_abis)
                };
                crate::ffmpeg_android::verify_runtime_aar(root, &abis, native_profile)?;
            }
            Ok(())
        }
        FfmpegPlatform::Ios => {
            let profile_hash = profile.profile_hash(FfmpegPlatform::Ios);
            let output_directory =
                apple_output_directory(root, request.output_directory.as_deref(), &profile_hash);
            let metadata_paths = crate::ffmpeg_apple::verify_prebuilts(
                &output_directory,
                &request.ios_slices,
                native_profile,
            )?;
            for metadata in metadata_paths {
                validate_metadata_file(&metadata, profile)?;
                validate_metadata_profile_hash(&metadata, &profile_hash)?;
            }
            Ok(())
        }
        FfmpegPlatform::All => Err(FfmpegError::worker(
            "internal FFmpeg verification dispatch received 'all'",
        )),
    }
}

fn verify_android_prebuilts(
    root: &Path,
    request: &FfmpegRequest,
    profile: &ResolvedProfile,
    expected_profile_hash: &str,
) -> Result<(), FfmpegError> {
    let abis = if request.android_abis.is_empty() {
        SUPPORTED_ANDROID_ABIS.to_vec()
    } else {
        list_tokens(&request.android_abis).collect()
    };
    for abi in abis {
        let abi_root = root.join(abi);
        let metadata = abi_root.join("vesper-ffmpeg-build-metadata.txt");
        validate_metadata_file(&metadata, profile)?;
        validate_metadata_profile_hash(&metadata, expected_profile_hash)?;
        for library in &profile.libraries {
            for artifact in [
                abi_root
                    .join("lib/pkgconfig")
                    .join(format!("lib{library}.pc")),
                abi_root.join("lib").join(format!("lib{library}.so")),
            ] {
                let file = fs::symlink_metadata(&artifact).map_err(|error| {
                    FfmpegError::conformance(format!(
                        "Android FFmpeg prebuilt is missing lib{library} for ABI {abi}: '{}': {error}",
                        artifact.display()
                    ))
                })?;
                if !file.file_type().is_file() || file.len() == 0 {
                    return Err(FfmpegError::conformance(format!(
                        "Android FFmpeg artifact must be a non-empty regular file: {}",
                        artifact.display()
                    )));
                }
            }
        }
    }
    Ok(())
}

fn validate_metadata_profile_hash(path: &Path, expected: &str) -> Result<(), FfmpegError> {
    let source = fs::read_to_string(path).map_err(|error| {
        FfmpegError::conformance(format!(
            "failed to read UTF-8 FFmpeg metadata '{}': {error}",
            path.display()
        ))
    })?;
    let record = parse_metadata_record(&source, path)?;
    let actual = required_metadata_value(&record, "profile_hash", path)?;
    if actual == expected {
        Ok(())
    } else {
        Err(FfmpegError::conformance(format!(
            "FFmpeg metadata profile_hash is '{actual}', expected '{expected}': {}",
            path.display()
        )))
    }
}

fn validate_metadata_file(path: &Path, profile: &ResolvedProfile) -> Result<(), FfmpegError> {
    let metadata = fs::metadata(path).map_err(|error| {
        FfmpegError::storage(format!(
            "failed to inspect FFmpeg metadata '{}': {error}",
            path.display()
        ))
    })?;
    if metadata.len() > MAX_METADATA_BYTES {
        return Err(FfmpegError::conformance(format!(
            "FFmpeg metadata exceeds {MAX_METADATA_BYTES} bytes: {}",
            path.display()
        )));
    }
    let source = fs::read_to_string(path).map_err(|error| {
        FfmpegError::conformance(format!(
            "failed to read UTF-8 FFmpeg metadata '{}': {error}",
            path.display()
        ))
    })?;
    let record = parse_metadata_record(&source, path)?;
    let configure = required_metadata_value(&record, "configure_line", path)?;
    let protocols = parse_metadata_csv(
        required_metadata_value(&record, "protocols", path)?,
        "protocols",
        path,
    )?;
    let external_dependencies = parse_metadata_csv(
        required_metadata_value(&record, "external_dependencies", path)?,
        "external_dependencies",
        path,
    )?;
    let license_flags = parse_metadata_csv(
        required_metadata_value(&record, "license_flags", path)?,
        "license_flags",
        path,
    )?;
    if profile.forbid_network {
        if !configure_flag(configure, "--disable-network") {
            return Err(FfmpegError::conformance(format!(
                "FFmpeg metadata does not include --disable-network: {}",
                path.display()
            )));
        }
        if configure_flag(configure, "--enable-network")
            || configure_enables_network_protocol(configure, path)?
            || protocols.iter().copied().any(is_network_protocol)
        {
            return Err(FfmpegError::conformance(format!(
                "FFmpeg metadata includes forbidden network capability: {}",
                path.display()
            )));
        }
    }
    if profile.forbid_openssl {
        if !configure_flag(configure, "--disable-openssl") {
            return Err(FfmpegError::conformance(format!(
                "FFmpeg metadata does not include --disable-openssl: {}",
                path.display()
            )));
        }
        if configure_flag(configure, "--enable-openssl")
            || external_dependencies.contains(&"openssl")
            || license_flags.contains(&"openssl")
        {
            return Err(FfmpegError::conformance(format!(
                "FFmpeg metadata includes forbidden OpenSSL capability: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn parse_metadata_record(
    source: &str,
    path: &Path,
) -> Result<BTreeMap<String, String>, FfmpegError> {
    if source.contains('\r')
        || source
            .chars()
            .any(|character| character.is_control() && character != '\n')
    {
        return Err(FfmpegError::conformance(format!(
            "FFmpeg metadata contains non-canonical control characters: {}",
            path.display()
        )));
    }
    let mut record = BTreeMap::new();
    for (index, line) in source.lines().enumerate() {
        if index == 0 && !line.contains('=') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            FfmpegError::conformance(format!(
                "invalid FFmpeg metadata line {} in '{}'",
                index + 1,
                path.display()
            ))
        })?;
        if key.is_empty() || record.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(FfmpegError::conformance(format!(
                "duplicate or empty FFmpeg metadata key '{key}' in '{}'",
                path.display()
            )));
        }
    }
    Ok(record)
}

fn required_metadata_value<'a>(
    record: &'a BTreeMap<String, String>,
    key: &str,
    path: &Path,
) -> Result<&'a str, FfmpegError> {
    record.get(key).map(String::as_str).ok_or_else(|| {
        FfmpegError::conformance(format!(
            "Missing FFmpeg metadata key '{key}': {}",
            path.display()
        ))
    })
}

fn configure_flag(configure: &str, flag: &str) -> bool {
    configure
        .split_ascii_whitespace()
        .any(|argument| flag_matches(argument, flag))
}

fn configure_enables_network_protocol(configure: &str, path: &Path) -> Result<bool, FfmpegError> {
    for argument in configure.split_ascii_whitespace() {
        let Some(protocols) = argument.strip_prefix("--enable-protocol=") else {
            continue;
        };
        let protocols = parse_metadata_csv(protocols, "configure_line --enable-protocol", path)?;
        if protocols.iter().copied().any(is_network_protocol) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn parse_metadata_csv<'a>(
    csv: &'a str,
    key: &str,
    path: &Path,
) -> Result<Vec<&'a str>, FfmpegError> {
    if csv.is_empty() {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    for value in csv.split(',') {
        let canonical = !value.is_empty()
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-')
            });
        if !canonical || values.contains(&value) {
            return Err(FfmpegError::conformance(format!(
                "FFmpeg metadata key '{key}' is not canonical in '{}': {csv}",
                path.display()
            )));
        }
        values.push(value);
    }
    Ok(values)
}

fn output_error(error: io::Error) -> FfmpegError {
    FfmpegError::storage(format!("failed to write FFmpeg CLI output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE_FIXTURE: &str = r#"
[profile.default]
libraries = ["avcodec", "avformat", "avutil"]
demuxers = ["dash", "mov"]
muxers = ["mp4"]
protocols = ["file", "pipe"]
parsers = ["h264"]
bsfs = ["h264_mp4toannexb"]
tls = "none"

[profile.default.validation]
forbid_network = true
forbid_openssl = true
"#;

    fn profile_fixture() -> FfmpegProfiles {
        FfmpegProfiles::parse(PROFILE_FIXTURE, Path::new("fixture-ffmpeg-profiles.toml"))
            .expect("parse FFmpeg profile fixture")
    }

    fn request() -> FfmpegRequest {
        FfmpegRequest {
            profile: Some("default".to_owned()),
            platform: Some(FfmpegPlatform::Android),
            list_profiles: false,
            dry_run: true,
            verify_only: false,
            output_directory: None,
            android_artifact: AndroidArtifact::RuntimeAar,
            android_abis: Vec::new(),
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

    #[test]
    fn checked_in_profile_resolution_preserves_legacy_order_and_hashes() {
        let Some(root) = crate::source_checkout_root() else {
            return;
        };
        let profiles = FfmpegProfiles::load(&root).expect("load profiles");
        assert_eq!(
            profiles.names().collect::<Vec<_>>(),
            vec![
                "base",
                "relay-remux",
                "download-remux",
                "default",
                "source-normalizer"
            ]
        );
        let android = profiles
            .resolve("default", FfmpegPlatform::Android)
            .expect("resolve Android profile");
        assert_eq!(
            android.profile_hash(FfmpegPlatform::Android),
            "custom-adf812db3806"
        );
        let ios = profiles
            .resolve("source-normalizer", FfmpegPlatform::Ios)
            .expect("resolve iOS profile");
        assert_eq!(ios.profile_hash(FfmpegPlatform::Ios), "custom-e1eabc3db7bc");
        let ios_default = profiles
            .resolve("default", FfmpegPlatform::Ios)
            .expect("resolve default iOS profile");
        assert_eq!(
            ios_default.profile_hash(FfmpegPlatform::Ios),
            "custom-e1eabc3db7bc"
        );
    }

    #[test]
    fn apple_raw_profiles_preserve_legacy_hashes_and_slice_order() {
        let repository = Path::new("/fixture/repository");
        let legacy = resolve_apple_raw_profile_with_environment(
            repository,
            &[],
            None,
            AppleRawEnvironment::default(),
        )
        .expect("resolve default raw Apple profile");
        assert_eq!(legacy.profile, "legacy");
        assert_eq!(legacy.profile_hash, "legacy");
        assert_eq!(
            legacy.output_directory,
            repository.join("third_party/ffmpeg/apple")
        );
        assert_eq!(legacy.slices, ["ios-arm64", "ios-simulator-arm64"]);
        assert!(
            !legacy
                .worker_arguments
                .contains(&"--tls-backend".to_owned())
        );
        assert!(
            !legacy
                .worker_arguments
                .contains(&"--enable-dash".to_owned())
        );

        let remux_arguments = [OsString::from("--profile"), OsString::from("remux-local")];
        let remux = resolve_apple_raw_profile_with_environment(
            repository,
            &remux_arguments,
            None,
            AppleRawEnvironment::default(),
        )
        .expect("resolve remux-local raw Apple profile");
        assert_eq!(remux.profile_hash, "remux-local-68dd2c99ba30");

        let ordered_slices = [
            OsString::from("ios-simulator-arm64"),
            OsString::from("ios-arm64"),
        ];
        let ordered = resolve_apple_raw_profile_with_environment(
            repository,
            &ordered_slices,
            None,
            AppleRawEnvironment::default(),
        )
        .expect("resolve explicitly ordered Apple slices");
        assert_eq!(ordered.slices, ["ios-simulator-arm64", "ios-arm64"]);

        let duplicate = [OsString::from("ios-arm64"), OsString::from("ios-arm64")];
        let error = resolve_apple_raw_profile_with_environment(
            repository,
            &duplicate,
            None,
            AppleRawEnvironment::default(),
        )
        .expect_err("reject duplicate Apple slices");
        assert_eq!(error.kind(), FfmpegErrorKind::Conformance);

        let combined = [OsString::from("ios-arm64,ios-simulator-arm64")];
        let error = resolve_apple_raw_profile_with_environment(
            repository,
            &combined,
            None,
            AppleRawEnvironment::default(),
        )
        .expect_err("reject a combined positional Apple slice token");
        assert_eq!(error.kind(), FfmpegErrorKind::Compatibility);
    }

    #[test]
    fn apple_raw_custom_profile_and_overlay_precedence_are_stable() {
        let repository = Path::new("/fixture/repository");
        let custom_arguments = [
            "--ffmpeg-profile",
            "custom",
            "--tls-backend",
            "none",
            "--enable-libraries",
            "avcodec,avformat,avutil",
            "--enable-protocols",
            "file,pipe",
            "--disable-dash",
        ]
        .map(OsString::from);
        let custom = resolve_apple_raw_profile_with_environment(
            repository,
            &custom_arguments,
            None,
            AppleRawEnvironment::default(),
        )
        .expect("resolve custom raw Apple profile");
        assert_eq!(custom.profile_hash, "custom-d76ea0adb781");

        let environment = AppleRawEnvironment {
            profile: Some("legacy".to_owned()),
            tls_backend: Some("securetransport".to_owned()),
            enable_dash: Some("1".to_owned()),
            libraries: vec!["avutil".to_owned(), "avcodec".to_owned()],
            protocols: vec!["pipe".to_owned(), "file".to_owned()],
            has_overlay: true,
            ..AppleRawEnvironment::default()
        };
        let cli_arguments = [
            "--profile",
            "custom",
            "--tls-backend",
            "none",
            "--enable-libraries",
            "avformat,avutil",
            "--enable-protocols",
            "data,pipe",
            "--disable-dash",
            "ios-simulator-arm64",
        ]
        .map(OsString::from);
        let resolved = AppleRawOptions::parse(&cli_arguments, environment)
            .and_then(AppleRawOptions::resolve)
            .expect("resolve raw Apple CLI over environment");
        assert_eq!(resolved.profile, "custom");
        assert_eq!(resolved.tls_backend, "none");
        assert!(!resolved.enable_dash);
        assert_eq!(resolved.libraries, ["avutil", "avcodec", "avformat"]);
        assert_eq!(resolved.protocols, ["pipe", "file", "data"]);
        assert_eq!(resolved.slices, ["ios-simulator-arm64"]);
    }

    #[cfg(unix)]
    #[test]
    fn apple_raw_scalar_ignores_a_non_utf8_generic_value_when_specific_wins() {
        use std::os::unix::ffi::OsStringExt;

        let (profile, present) = apple_raw_environment_scalar_from_values(
            "PROFILE",
            Some(OsString::from("legacy")),
            Some(OsString::from_vec(vec![0xff])),
        )
        .expect("ignore the losing generic profile");

        assert_eq!(profile.as_deref(), Some("legacy"));
        assert!(present);
    }

    #[test]
    fn apple_raw_empty_extra_argument_preserves_the_shell_seed() {
        let arguments = [OsString::from("--extra-configure-arg=")];
        let profile = resolve_apple_raw_profile_with_environment(
            Path::new("/fixture/repository"),
            &arguments,
            None,
            AppleRawEnvironment::default(),
        )
        .expect("resolve an empty extra configure argument");

        assert_eq!(profile.profile_hash, "legacy-915ff780539a");
        assert!(
            profile
                .worker_arguments
                .contains(&"--extra-configure-arg=".to_owned())
        );
    }

    #[test]
    fn apple_raw_lists_use_default_shell_ifs_boundaries() {
        for value in ["file\rpipe", "file\u{a0}pipe"] {
            let arguments = [OsString::from("--enable-protocols"), OsString::from(value)];
            let error = resolve_apple_raw_profile_with_environment(
                Path::new("/fixture/repository"),
                &arguments,
                None,
                AppleRawEnvironment::default(),
            )
            .expect_err("reject whitespace outside the default shell IFS");

            assert_eq!(error.kind(), FfmpegErrorKind::Conformance);
            assert!(error.to_string().contains("Invalid FFmpeg protocol name"));
        }
    }

    #[test]
    fn output_paths_are_root_scoped() {
        let root = Path::new("/fixture/repository");
        assert_eq!(
            root_relative_path(root, OsStr::new("relative/output")),
            root.join("relative/output")
        );
        assert_eq!(
            root_relative_path(root, OsStr::new("/absolute/output")),
            PathBuf::from("/absolute/output")
        );
    }

    #[test]
    fn network_and_openssl_overlays_are_rejected_by_profile_policy() {
        let profiles = profile_fixture();
        let mut resolved = profiles
            .resolve("default", FfmpegPlatform::Android)
            .expect("resolve profile");
        let mut protocol_request = request();
        protocol_request.extra_protocols.push("http".to_owned());
        resolved.apply_request_overlays(&protocol_request);
        assert!(resolved.validate(FfmpegPlatform::Android, false).is_err());

        let mut resolved = profiles
            .resolve("default", FfmpegPlatform::Android)
            .expect("resolve profile");
        let mut configure_request = request();
        configure_request
            .extra_configure_args
            .push("--enable-openssl".to_owned());
        resolved.apply_request_overlays(&configure_request);
        assert!(resolved.validate(FfmpegPlatform::Android, false).is_err());
    }

    #[test]
    fn malformed_unknown_and_cyclic_profile_documents_are_rejected() {
        let path = Path::new("fixture-profiles.toml");
        assert!(FfmpegProfiles::parse("[profile.bad]\nunknown = true\n", path).is_err());
        let profiles = FfmpegProfiles::parse(
            "[profile.first]\nextends = \"second\"\n[profile.second]\nextends = \"first\"\n",
            path,
        )
        .expect("parse cyclic profiles");
        assert!(profiles.resolve("first", FfmpegPlatform::Android).is_err());
    }

    #[test]
    fn target_selectors_enforce_the_arm64_platform_floor() {
        let mut android_request = request();
        android_request.android_abis.push("arm64-v8a".to_owned());
        validate_target_selectors(&android_request, FfmpegPlatform::Android)
            .expect("accept supported Android ABI");

        android_request.android_abis.push("x86_64".to_owned());
        let error = validate_target_selectors(&android_request, FfmpegPlatform::Android)
            .expect_err("reject unsupported Android ABI");
        assert_eq!(error.kind(), FfmpegErrorKind::Compatibility);

        let mut ios_request = request();
        ios_request
            .ios_slices
            .push("ios-simulator-x86_64".to_owned());
        let error = validate_target_selectors(&ios_request, FfmpegPlatform::Ios)
            .expect_err("reject unsupported iOS slice");
        assert_eq!(error.kind(), FfmpegErrorKind::Compatibility);

        let mut platform_request = request();
        platform_request.ios_slices.push("ios-arm64".to_owned());
        assert!(validate_target_selectors(&platform_request, FfmpegPlatform::Android).is_err());
    }

    #[test]
    fn metadata_validation_parses_capability_fields() {
        let temporary = tempfile::tempdir().expect("create metadata fixture");
        let path = temporary.path().join("vesper-ffmpeg-build-metadata.txt");
        fs::write(
            &path,
            "Vesper FFmpeg build metadata v2\nprotocols=file,pipe\nexternal_dependencies=libxml2\nlicense_flags=\nconfigure_line=./configure --disable-network --disable-openssl --enable-shared\n",
        )
        .expect("write metadata fixture");
        let profile = ResolvedProfile {
            forbid_network: true,
            forbid_openssl: true,
            ..ResolvedProfile::default()
        };
        validate_metadata_file(&path, &profile).expect("validate local-only metadata");
        fs::write(
            &path,
            "Vesper FFmpeg build metadata v2\nprotocols=file,http\nexternal_dependencies=libxml2\nlicense_flags=\nconfigure_line=./configure --disable-network --disable-openssl --enable-shared\n",
        )
        .expect("write network metadata fixture");
        assert!(validate_metadata_file(&path, &profile).is_err());

        for source in [
            "Vesper FFmpeg build metadata v2\nprotocols=file, http\nexternal_dependencies=libxml2\nlicense_flags=\nconfigure_line=./configure --disable-network --disable-openssl --enable-shared\n",
            "Vesper FFmpeg build metadata v2\nprotocols=file,pipe\nexternal_dependencies=libxml2, openssl\nlicense_flags=\nconfigure_line=./configure --disable-network --disable-openssl --enable-shared\n",
            "Vesper FFmpeg build metadata v2\nprotocols=file,pipe\nexternal_dependencies=libxml2\nlicense_flags=\nconfigure_line=./configure --disable-network --enable-protocol=file,http --disable-openssl --enable-shared\n",
        ] {
            fs::write(&path, source).expect("write non-canonical metadata fixture");
            assert!(validate_metadata_file(&path, &profile).is_err());
        }
    }

    #[test]
    fn native_build_profile_matches_the_custom_worker_contract() {
        let profiles = profile_fixture();
        let prepared = prepare_profile(&profiles, &request(), FfmpegPlatform::Android)
            .expect("prepare Android profile");
        let profile = NativeFfmpegProfile::from_prepared(FfmpegPlatform::Android, &prepared);
        let arguments = profile.configure_arguments(FfmpegPlatform::Android);

        assert_eq!(profile.declared_profile, "default");
        assert!(profile.profile_hash.starts_with("custom-"));
        assert!(arguments.contains(&"--disable-everything".to_owned()));
        assert!(arguments.contains(&"--disable-network".to_owned()));
        assert!(arguments.contains(&"--disable-openssl".to_owned()));
        assert!(arguments.contains(&"--enable-libxml2".to_owned()));
        assert!(arguments.contains(&"--enable-avcodec".to_owned()));
        assert_eq!(profile.external_dependencies(), ["libxml2"]);
        assert!(profile.license_flags().is_empty());
    }
}
