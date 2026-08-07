use std::env;
use std::ffi::OsStr;
#[cfg(target_os = "macos")]
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

#[cfg(target_os = "macos")]
use crate::{external_process, ffmpeg};

#[cfg(all(target_os = "macos", test))]
use sha2::{Digest, Sha256};

#[cfg(unix)]
use player_platform_process::configure_background_process_group;

const FFPROBE_OUTPUT_LIMIT: usize = 4 * 1024;
const BASIC_PLAYER_LOG_LIMIT: usize = 2 * 1024 * 1024;
#[cfg(unix)]
const BASIC_PLAYER_LOG_TAIL_LIMIT: usize = 64 * 1024;
#[cfg(unix)]
const BASIC_PLAYER_CAPTURE_QUEUE_CAPACITY: usize = 64;
#[cfg(unix)]
const BASIC_PLAYER_CAPTURE_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(unix)]
const BASIC_PLAYER_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(unix)]
const BASIC_PLAYER_INTERRUPT_GRACE: Duration = Duration::from_secs(2);
#[cfg(unix)]
const BASIC_PLAYER_TERMINATION_GRACE: Duration = Duration::from_millis(500);
#[cfg(unix)]
const BASIC_PLAYER_REAP_TIMEOUT: Duration = Duration::from_secs(2);

const BASIC_PLAYER_REQUIRED_LOG_ENTRIES: [(&str, &str); 12] = [
    ("initialized desktop player", "runtime initialization"),
    (
        "selected sdkManagedNativeFrame route",
        "sdkManagedNativeFrame route selection",
    ),
    (
        "supports_external_video_surface=true",
        "external macOS video surface support",
    ),
    (
        "frame processor plugins: 1/1 supported",
        "diagnostic FrameProcessor support",
    ),
    (
        "macOS frame processor debug summary",
        "FrameProcessor debug summary",
    ),
    (
        "basic-player smoke script observed playback",
        "scripted playback observation",
    ),
    (
        "basic-player smoke script showed overlay",
        "scripted overlay refresh",
    ),
    (
        "basic-player smoke script paused playback",
        "scripted pause",
    ),
    (
        "basic-player smoke script resumed playback",
        "scripted resume",
    ),
    (
        "basic-player smoke script seeked to midpoint",
        "scripted seek",
    ),
    (
        "basic-player smoke script changed rate",
        "scripted playback-rate update",
    ),
    ("player playback ended", "playback completion"),
];

#[cfg(target_os = "macos")]
const DESKTOP_SOURCE_ARCHIVE_LIMIT: u64 = 1024 * 1024 * 1024;
#[cfg(target_os = "macos")]
const DESKTOP_SOURCE_ENTRY_LIMIT: usize = 100_000;
#[cfg(target_os = "macos")]
const DESKTOP_SOURCE_EXPANDED_LIMIT: u64 = 8 * 1024 * 1024 * 1024;
#[cfg(target_os = "macos")]
#[cfg(target_os = "macos")]
const DESKTOP_SOURCE_PATH_BYTES_LIMIT: usize = 4096;
#[cfg(target_os = "macos")]
const DESKTOP_SOURCE_PATH_DEPTH_LIMIT: usize = 64;
#[cfg(target_os = "macos")]
const DESKTOP_SOURCE_POLICY: crate::source_archive::SourceArchivePolicy =
    crate::source_archive::SourceArchivePolicy {
        maximum_archive_bytes: DESKTOP_SOURCE_ARCHIVE_LIMIT,
        maximum_entries: DESKTOP_SOURCE_ENTRY_LIMIT,
        maximum_expanded_bytes: DESKTOP_SOURCE_EXPANDED_LIMIT,
        maximum_path_bytes: DESKTOP_SOURCE_PATH_BYTES_LIMIT,
        maximum_path_depth: DESKTOP_SOURCE_PATH_DEPTH_LIMIT,
    };
#[cfg(target_os = "macos")]
const DESKTOP_REQUIRED_COMPONENTS: [&str; 6] = [
    "avcodec",
    "avfilter",
    "avformat",
    "avutil",
    "swresample",
    "swscale",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopErrorKind {
    Storage,
    Compatibility,
    Conformance,
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopError {
    kind: DesktopErrorKind,
    message: String,
}

impl DesktopError {
    fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: DesktopErrorKind::Storage,
            message: message.into(),
        }
    }

    fn compatibility(message: impl Into<String>) -> Self {
        Self {
            kind: DesktopErrorKind::Compatibility,
            message: message.into(),
        }
    }

    fn conformance(message: impl Into<String>) -> Self {
        Self {
            kind: DesktopErrorKind::Conformance,
            message: message.into(),
        }
    }

    fn worker(message: impl Into<String>) -> Self {
        Self {
            kind: DesktopErrorKind::Worker,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> DesktopErrorKind {
        self.kind
    }
}

impl std::fmt::Display for DesktopError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DesktopError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemuxMode {
    Loader,
    Download,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticsMode {
    Loader,
    Macos,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum D3d11Mode {
    Loader,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoToolboxMode {
    Loader,
    Playback,
    BasicPlayer,
    All,
}

impl VideoToolboxMode {
    const fn needs_frame_processor(self) -> bool {
        matches!(self, Self::Playback | Self::BasicPlayer | Self::All)
    }
}

#[derive(Debug)]
struct VideoToolboxAssets {
    decoder_plugin: PathBuf,
    smoke_source: PathBuf,
    frame_processor_plugin: Option<PathBuf>,
}

pub fn ensure_ffmpeg(root: &Path) -> Result<(), DesktopError> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = root;
        return Err(DesktopError::compatibility(
            "The repository-local desktop FFmpeg fallback is only supported on macOS.",
        ));
    }

    #[cfg(target_os = "macos")]
    {
        ensure_ffmpeg_macos(root)
    }
}

#[cfg(target_os = "macos")]
fn ensure_ffmpeg_macos(root: &Path) -> Result<(), DesktopError> {
    let install_directory = desktop_ffmpeg_install_directory(root);
    if desktop_install_complete(&install_directory)? {
        warn_if_desktop_install_is_hidden(&install_directory);
        println!("{}", install_directory.display());
        return Ok(());
    }

    let source = ffmpeg::resolve_desktop_source(root).map_err(map_ffmpeg_error)?;
    let archive = desktop_source_archive(root, &source)?;
    let extraction = tempfile::Builder::new()
        .prefix("vesper-desktop-ffmpeg-source-")
        .tempdir()
        .map_err(|error| {
            DesktopError::storage(format!("failed to create FFmpeg source staging: {error}"))
        })?;
    let source_directory = extract_desktop_source(&archive, extraction.path(), &source.version)?;
    let sdk_path = capture_tool_text(
        env::var_os("XCRUN").unwrap_or_else(|| OsString::from("xcrun")),
        &["--sdk", "macosx", "--show-sdk-path"],
        "macOS SDK lookup",
    )?;
    let clang_path = capture_tool_text(
        env::var_os("XCRUN").unwrap_or_else(|| OsString::from("xcrun")),
        &["--sdk", "macosx", "-f", "clang"],
        "Apple clang lookup",
    )?;
    let install_parent = install_directory.parent().ok_or_else(|| {
        DesktopError::storage(format!(
            "desktop FFmpeg install has no parent: {}",
            install_directory.display()
        ))
    })?;
    fs::create_dir_all(install_parent).map_err(|error| {
        DesktopError::storage(format!(
            "failed to create desktop FFmpeg install parent '{}': {error}",
            install_parent.display()
        ))
    })?;
    let install_stage = tempfile::Builder::new()
        .prefix(".vesper-desktop-ffmpeg-install-")
        .tempdir_in(install_parent)
        .map_err(|error| {
            DesktopError::storage(format!("failed to create FFmpeg install staging: {error}"))
        })?;
    let flags = format!("-isysroot {sdk_path} -mmacosx-version-min=11.0");
    let configure = source_directory.join("configure");
    require_regular_file(&configure, "FFmpeg configure script")?;
    let mut configure_command = Command::new(&configure);
    configure_command
        .current_dir(&source_directory)
        .arg(format!("--prefix={}", install_directory.display()))
        .arg(format!("--cc={clang_path}"))
        .arg(format!("--host-cc={clang_path}"))
        .arg(format!("--extra-cflags={flags} -w"))
        .arg(format!("--extra-ldflags={flags}"))
        .arg(format!("--host-cflags={flags} -w"))
        .arg(format!("--host-ldflags={flags}"))
        .args([
            "--disable-autodetect",
            "--disable-programs",
            "--disable-doc",
            "--disable-debug",
            "--enable-static",
            "--disable-shared",
            "--enable-pic",
        ])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    run_desktop_build(&mut configure_command, "desktop FFmpeg configure")?;

    let make = env::var_os("MAKE").unwrap_or_else(|| OsString::from("make"));
    let jobs = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4);
    let mut build = Command::new(&make);
    build
        .current_dir(&source_directory)
        .arg(format!("-j{jobs}"))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    run_desktop_build(&mut build, "desktop FFmpeg build")?;

    let mut install = Command::new(&make);
    install
        .current_dir(&source_directory)
        .arg("install")
        .env("DESTDIR", install_stage.path())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    run_desktop_build(&mut install, "desktop FFmpeg install")?;
    let staged_relative = install_directory.strip_prefix("/").map_err(|_| {
        DesktopError::storage(format!(
            "desktop FFmpeg install path is not absolute: {}",
            install_directory.display()
        ))
    })?;
    let staged_install = install_stage.path().join(staged_relative);
    require_complete_desktop_install(&staged_install)?;
    publish_desktop_install_deferred(&staged_install, &install_directory)?;
    warn_if_desktop_install_is_hidden(&install_directory);
    println!("{}", install_directory.display());
    Ok(())
}

#[cfg(target_os = "macos")]
fn desktop_ffmpeg_install_directory(root: &Path) -> PathBuf {
    env::var_os("VESPER_DESKTOP_FFMPEG_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .unwrap_or_else(|| root.join("third_party/ffmpeg/desktop"))
}

#[cfg(target_os = "macos")]
fn desktop_source_archive(
    root: &Path,
    source: &crate::ffmpeg_source::FfmpegBuildSource,
) -> Result<PathBuf, DesktopError> {
    let expected_sha256 = desktop_source_sha256(source)?;
    let configured = env::var_os("VESPER_DESKTOP_FFMPEG_SOURCE_ARCHIVE").map(PathBuf::from);
    let archive = configured.unwrap_or_else(|| {
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
        cache.join(&source.archive_name)
    });
    let archive = if archive.is_absolute() {
        archive
    } else {
        root.join(archive)
    };
    crate::source_archive::ensure_cached_archive(
        &archive,
        std::slice::from_ref(&source.source_url),
        expected_sha256.as_deref(),
        DESKTOP_SOURCE_POLICY,
        "desktop FFmpeg source archive",
    )
    .map_err(map_source_archive_error)
}

#[cfg(target_os = "macos")]
fn desktop_source_sha256(
    source: &crate::ffmpeg_source::FfmpegBuildSource,
) -> Result<Option<String>, DesktopError> {
    let configured = env::var("VESPER_DESKTOP_FFMPEG_SOURCE_SHA256")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            env::var("VESPER_FFMPEG_SOURCE_SHA256")
                .ok()
                .filter(|value| !value.is_empty())
        });
    let expected = configured.or_else(|| source.expected_sha256.clone());
    expected
        .map(|value| {
            let normalized = value.to_ascii_lowercase();
            if normalized.len() == 64 && normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                Ok(normalized)
            } else {
                Err(DesktopError::conformance(
                    "desktop FFmpeg source SHA-256 must contain exactly 64 hexadecimal characters",
                ))
            }
        })
        .transpose()
}

#[cfg(all(target_os = "macos", test))]
fn verify_desktop_source_sha256(
    archive: &Path,
    expected: Option<&str>,
) -> Result<(), DesktopError> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let actual = crate::source_archive::sha256_file(
        archive,
        DESKTOP_SOURCE_ARCHIVE_LIMIT,
        "desktop FFmpeg source archive",
    )
    .map_err(map_source_archive_error)?;
    (actual == expected).then_some(()).ok_or_else(|| {
        DesktopError::conformance(format!(
            "desktop FFmpeg source archive checksum mismatch for '{}': expected {expected}, actual {actual}",
            archive.display()
        ))
    })
}

#[cfg(target_os = "macos")]
fn extract_desktop_source(
    archive: &Path,
    destination: &Path,
    version: &str,
) -> Result<PathBuf, DesktopError> {
    let source = crate::source_archive::extract_single_root(
        archive,
        destination,
        &format!("ffmpeg-{version}"),
        crate::source_archive::SourceArchiveFormat::TarXz,
        DESKTOP_SOURCE_POLICY,
        "FFmpeg source archive",
    )
    .map_err(map_source_archive_error)?;
    require_regular_file(&source.join("configure"), "FFmpeg configure script")?;
    Ok(source)
}

#[cfg(target_os = "macos")]
fn map_source_archive_error(error: crate::source_archive::SourceArchiveError) -> DesktopError {
    match error.kind() {
        crate::source_archive::SourceArchiveErrorKind::Storage => {
            DesktopError::storage(error.to_string())
        }
        crate::source_archive::SourceArchiveErrorKind::Conformance => {
            DesktopError::conformance(error.to_string())
        }
        crate::source_archive::SourceArchiveErrorKind::Worker => {
            DesktopError::worker(error.to_string())
        }
    }
}

#[cfg(target_os = "macos")]
fn desktop_install_complete(root: &Path) -> Result<bool, DesktopError> {
    for component in DESKTOP_REQUIRED_COMPONENTS {
        for relative in [
            format!("lib/pkgconfig/lib{component}.pc"),
            format!("lib/lib{component}.a"),
        ] {
            if !regular_nonempty_file(&root.join(relative))? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(target_os = "macos")]
fn require_complete_desktop_install(root: &Path) -> Result<(), DesktopError> {
    if desktop_install_complete(root)? {
        Ok(())
    } else {
        Err(DesktopError::conformance(format!(
            "staged desktop FFmpeg install is incomplete: {}",
            root.display()
        )))
    }
}

#[cfg(target_os = "macos")]
fn warn_if_desktop_install_is_hidden(install: &Path) {
    let pkg_config = install.join("lib/pkgconfig");
    let custom_install = env::var_os("VESPER_DESKTOP_FFMPEG_DIR").is_some();
    let configured_path = env::var_os("PKG_CONFIG_PATH");
    let visible = configured_path
        .as_ref()
        .is_some_and(|value| env::split_paths(value).any(|entry| entry == pkg_config));
    if custom_install && !visible {
        eprintln!(
            "warning: add '{}' to PKG_CONFIG_PATH before Cargo builds that use this custom desktop FFmpeg install",
            pkg_config.display()
        );
    }
}

#[cfg(target_os = "macos")]
fn capture_tool_text(
    executable: OsString,
    arguments: &[&str],
    label: &str,
) -> Result<String, DesktopError> {
    let mut command = Command::new(executable);
    command.args(arguments);
    let captured = external_process::run_interruptible_capture_with_timeout(
        &mut command,
        label,
        64 * 1024,
        64 * 1024,
        Duration::from_secs(30),
    )
    .map_err(map_external_process_error)?;
    if !captured.status.success() {
        return Err(DesktopError::compatibility(format!(
            "{label} exited unsuccessfully ({})",
            captured.status
        )));
    }
    let value = String::from_utf8(captured.stdout)
        .map_err(|_| DesktopError::compatibility(format!("{label} returned non-UTF-8 output")))?;
    let value = value.trim();
    if value.is_empty() || value.contains('\n') {
        return Err(DesktopError::compatibility(format!(
            "{label} returned an invalid path"
        )));
    }
    Ok(value.to_owned())
}

#[cfg(target_os = "macos")]
fn run_desktop_build(command: &mut Command, label: &str) -> Result<(), DesktopError> {
    let status =
        external_process::run_interruptible(command, label).map_err(map_external_process_error)?;
    if status.success() {
        Ok(())
    } else {
        Err(DesktopError::conformance(format!(
            "{label} exited unsuccessfully ({status})"
        )))
    }
}

#[cfg(target_os = "macos")]
fn publish_desktop_install(source: &Path, target: &Path) -> Result<(), DesktopError> {
    publish_desktop_install_with_hook(source, target, None)
}

#[cfg(target_os = "macos")]
fn publish_desktop_install_with_hook(
    source: &Path,
    target: &Path,
    mut before_publish: Option<crate::PathIoHook<'_>>,
) -> Result<(), DesktopError> {
    let previous = match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_dir() => true,
        Ok(_) => {
            return Err(DesktopError::compatibility(format!(
                "desktop FFmpeg install target must be a directory: {}",
                target.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(DesktopError::storage(format!(
                "failed to inspect desktop FFmpeg install '{}': {error}",
                target.display()
            )));
        }
    };
    let backup = if previous {
        let parent = target
            .parent()
            .ok_or_else(|| DesktopError::storage("desktop FFmpeg install target has no parent"))?;
        let reserved = tempfile::Builder::new()
            .prefix(".vesper-desktop-ffmpeg-backup-")
            .tempdir_in(parent)
            .map_err(|error| {
                DesktopError::storage(format!("failed to reserve FFmpeg backup: {error}"))
            })?
            .keep();
        fs::remove_dir(&reserved).map_err(|error| {
            DesktopError::storage(format!("failed to prepare FFmpeg backup: {error}"))
        })?;
        fs::rename(target, &reserved).map_err(|error| {
            DesktopError::storage(format!(
                "failed to preserve previous FFmpeg install: {error}"
            ))
        })?;
        Some(reserved)
    } else {
        None
    };
    if let Some(hook) = before_publish.as_mut() {
        hook(target).map_err(|error| {
            DesktopError::storage(format!(
                "failed to finish preparing desktop FFmpeg publication: {error}"
            ))
        })?;
    }
    if let Err(error) = fs::rename(source, target) {
        if let Some(backup) = backup.as_ref()
            && let Err(rollback_error) = fs::rename(backup, target)
        {
            return Err(DesktopError::storage(format!(
                "failed to publish desktop FFmpeg install: {error}; failed to restore the previous install: {rollback_error}; recovery data was preserved at '{}'",
                backup.display()
            )));
        }
        return Err(DesktopError::storage(format!(
            "failed to publish desktop FFmpeg install: {error}"
        )));
    }
    if let Some(backup) = backup {
        fs::remove_dir_all(&backup).map_err(|error| {
            DesktopError::storage(format!(
                "desktop FFmpeg was published, but the previous install could not be removed from '{}': {error}",
                backup.display()
            ))
        })?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn publish_desktop_install_deferred(source: &Path, target: &Path) -> Result<(), DesktopError> {
    let cancellation = external_process::InterruptDeferral::start("desktop FFmpeg publication")
        .map_err(map_external_process_error)?;
    let result = publish_desktop_install(source, target);
    let cancelled = cancellation.finish();
    match (result, cancelled) {
        (Ok(()), false) => Ok(()),
        (Ok(()), true) => Err(DesktopError::worker(
            "desktop FFmpeg publication completed after cancellation",
        )),
        (Err(error), false) => Err(error),
        (Err(error), true) => Err(DesktopError::worker(format!(
            "desktop FFmpeg publication was cancelled; {error}"
        ))),
    }
}

#[cfg(target_os = "macos")]
fn regular_nonempty_file(path: &Path) -> Result<bool, DesktopError> {
    regular_nonempty_bounded_file(path, u64::MAX)
}

#[cfg(target_os = "macos")]
fn regular_nonempty_bounded_file(path: &Path, maximum: u64) -> Result<bool, DesktopError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            Ok(metadata.file_type().is_file() && metadata.len() > 0 && metadata.len() <= maximum)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(DesktopError::storage(format!(
            "failed to inspect '{}': {error}",
            path.display()
        ))),
    }
}

#[cfg(target_os = "macos")]
fn require_regular_file(path: &Path, label: &str) -> Result<(), DesktopError> {
    if regular_nonempty_file(path)? {
        Ok(())
    } else {
        Err(DesktopError::conformance(format!(
            "{label} must be a non-empty regular file: {}",
            path.display()
        )))
    }
}

#[cfg(target_os = "macos")]
fn map_ffmpeg_error(error: ffmpeg::FfmpegError) -> DesktopError {
    match error.kind() {
        ffmpeg::FfmpegErrorKind::Storage => DesktopError::storage(error.to_string()),
        ffmpeg::FfmpegErrorKind::Compatibility => DesktopError::compatibility(error.to_string()),
        ffmpeg::FfmpegErrorKind::Conformance => DesktopError::conformance(error.to_string()),
        ffmpeg::FfmpegErrorKind::Worker => DesktopError::worker(error.to_string()),
    }
}

#[cfg(target_os = "macos")]
fn map_external_process_error(error: external_process::ExternalProcessError) -> DesktopError {
    match error.kind() {
        external_process::ExternalProcessErrorKind::Compatibility => {
            DesktopError::compatibility(error.to_string())
        }
        external_process::ExternalProcessErrorKind::Worker
        | external_process::ExternalProcessErrorKind::Cancelled => {
            DesktopError::worker(error.to_string())
        }
    }
}

pub fn verify_remux(
    root: &Path,
    tokens: &[String],
    output: &mut dyn io::Write,
) -> Result<(), DesktopError> {
    let (profile, mode) = parse_remux_tokens(tokens)?;
    let override_path = env::var_os("VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let target_dir = target_directory(root);
    let plugin_path = match override_path {
        Some(path) => {
            if !path.is_file() {
                return Err(DesktopError::storage(format!(
                    "VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH points to a missing file: {}",
                    path.display()
                )));
            }
            path
        }
        None => {
            let mut command = cargo_command(root);
            command.arg("build").arg("-p").arg("player-remux-ffmpeg");
            if profile == BuildProfile::Release {
                command.arg("--release");
            }
            run_command(&mut command, "Cargo player-remux-ffmpeg build")?;
            resolve_plugin_path(
                &target_dir,
                profile,
                &shared_library_name("vesper_remux_ffmpeg")?,
                "VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH",
                "player-remux-ffmpeg",
            )?
        }
    };
    writeln!(
        output,
        "Using player-remux-ffmpeg plugin: {}",
        normalize_runtime_path(&plugin_path).display()
    )
    .map_err(output_error)?;

    let plugin_path = normalize_runtime_path(&plugin_path);
    match mode {
        RemuxMode::Loader => run_loader_test(root, &plugin_path),
        RemuxMode::Download => run_download_test(root, &plugin_path),
        RemuxMode::All => {
            run_loader_test(root, &plugin_path)?;
            run_download_test(root, &plugin_path)
        }
    }
}

pub fn verify_decoder_diagnostics(
    root: &Path,
    tokens: &[String],
    output: &mut dyn io::Write,
) -> Result<(), DesktopError> {
    let (profile, mode) = parse_diagnostics_tokens(tokens)?;
    let override_path = env::var_os("VESPER_DECODER_FIXTURE_PLUGIN_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let target_dir = target_directory(root);
    let plugin_path = match override_path {
        Some(path) => {
            if !path.is_file() {
                return Err(DesktopError::storage(format!(
                    "VESPER_DECODER_FIXTURE_PLUGIN_PATH points to a missing file: {}",
                    path.display()
                )));
            }
            path
        }
        None => {
            let mut command = cargo_command(root);
            command.arg("build").arg("-p").arg("player-decoder-fixture");
            if profile == BuildProfile::Release {
                command.arg("--release");
            }
            run_command(&mut command, "Cargo player-decoder-fixture build")?;
            resolve_plugin_path(
                &target_dir,
                profile,
                &shared_library_name("vesper_decoder_fixture")?,
                "VESPER_DECODER_FIXTURE_PLUGIN_PATH",
                "player-decoder-fixture",
            )?
        }
    };
    let plugin_path = normalize_runtime_path(&plugin_path);
    writeln!(
        output,
        "Using decoder fixture plugin: {}",
        plugin_path.display()
    )
    .map_err(output_error)?;
    writeln!(output, "Fixture decoder codecs: {}", fixture_codecs()).map_err(output_error)?;

    match mode {
        DiagnosticsMode::Loader => {
            run_decoder_loader_test(root, &plugin_path)?;
        }
        DiagnosticsMode::Macos => {
            run_decoder_macos_test(root, &plugin_path, output)?;
        }
        DiagnosticsMode::All => {
            run_decoder_loader_test(root, &plugin_path)?;
            run_decoder_macos_test(root, &plugin_path, output)?;
        }
    }
    Ok(())
}

pub fn verify_decoder_d3d11(
    root: &Path,
    tokens: &[String],
    output: &mut dyn io::Write,
) -> Result<(), DesktopError> {
    if env::consts::OS != "windows" {
        return Err(DesktopError::compatibility(
            "D3D11 decoder verification only runs on Windows.",
        ));
    }
    let (profile, mode) = parse_d3d11_tokens(tokens)?;
    let override_path = env::var_os("VESPER_DECODER_D3D11_PLUGIN_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let target_dir = target_directory(root);
    let plugin_path = match override_path {
        Some(path) => {
            if !path.is_file() {
                return Err(DesktopError::storage(format!(
                    "VESPER_DECODER_D3D11_PLUGIN_PATH points to a missing file: {}",
                    path.display()
                )));
            }
            path
        }
        None => {
            let mut command = cargo_command(root);
            command.arg("build").arg("-p").arg("player-decoder-d3d11");
            if profile == BuildProfile::Release {
                command.arg("--release");
            }
            run_command(&mut command, "Cargo player-decoder-d3d11 build")?;
            resolve_plugin_path(
                &target_dir,
                profile,
                &shared_library_name_for("windows", "vesper_decoder_d3d11")?,
                "VESPER_DECODER_D3D11_PLUGIN_PATH",
                "player-decoder-d3d11",
            )?
        }
    };
    writeln!(
        output,
        "Using D3D11 decoder plugin: {}",
        plugin_path.display()
    )
    .map_err(output_error)?;

    if mode == D3d11Mode::All {
        let mut command = cargo_command(root);
        command
            .args(["test", "-p", "player-decoder-d3d11"])
            .env("VESPER_DECODER_D3D11_PLUGIN_PATH", &plugin_path);
        run_command(&mut command, "D3D11 decoder crate tests")?;
    }
    let mut command = cargo_command(root);
    command
        .args([
            "test",
            "-p",
            "player-plugin-loader",
            "tests::native_dynamic_tests::native_dynamic_loader_opens_d3d11_decoder",
            "--",
            "--ignored",
            "--exact",
        ])
        .env("VESPER_DECODER_D3D11_PLUGIN_PATH", plugin_path);
    run_command(&mut command, "D3D11 decoder loader verification")
}

pub fn verify_decoder_videotoolbox(
    root: &Path,
    tokens: &[String],
    output: &mut dyn io::Write,
) -> Result<(), DesktopError> {
    if env::consts::OS != "macos" {
        return Err(DesktopError::compatibility(
            "VideoToolbox decoder verification only runs on macOS.",
        ));
    }

    let (profile, mode) = parse_videotoolbox_tokens(tokens)?;
    let target_dir = target_directory(root);
    let decoder_plugin = resolve_or_build_plugin(
        root,
        &target_dir,
        profile,
        "player-decoder-videotoolbox",
        "vesper_decoder_videotoolbox",
        "VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH",
    )?;
    writeln!(
        output,
        "Using VideoToolbox decoder plugin: {}",
        decoder_plugin.display()
    )
    .map_err(output_error)?;

    let smoke_source = resolve_videotoolbox_smoke_source(root, &target_dir)?;
    require_videotoolbox_b_frames(root, &smoke_source)?;
    writeln!(
        output,
        "Using VideoToolbox smoke source: {}",
        smoke_source.display()
    )
    .map_err(output_error)?;

    let frame_processor_plugin = if mode.needs_frame_processor() {
        let plugin = resolve_or_build_plugin(
            root,
            &target_dir,
            profile,
            "player-frame-processor-diagnostic",
            "vesper_frame_processor_diagnostic",
            "VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH",
        )?;
        writeln!(
            output,
            "Using diagnostic frame processor plugin: {}",
            plugin.display()
        )
        .map_err(output_error)?;
        Some(plugin)
    } else {
        None
    };
    output.flush().map_err(output_error)?;

    let assets = VideoToolboxAssets {
        decoder_plugin,
        smoke_source,
        frame_processor_plugin,
    };
    match mode {
        VideoToolboxMode::Loader => run_videotoolbox_loader_tests(root, &assets),
        VideoToolboxMode::Playback => run_videotoolbox_playback_tests(root, &assets),
        VideoToolboxMode::BasicPlayer => {
            run_videotoolbox_basic_player(root, &target_dir, profile, &assets, output)
        }
        VideoToolboxMode::All => {
            run_videotoolbox_loader_tests(root, &assets)?;
            run_videotoolbox_playback_tests(root, &assets)?;
            run_videotoolbox_basic_player(root, &target_dir, profile, &assets, output)
        }
    }
}

fn parse_remux_tokens(tokens: &[String]) -> Result<(BuildProfile, RemuxMode), DesktopError> {
    let mut profile = BuildProfile::Debug;
    let mut mode = RemuxMode::All;
    for token in tokens {
        match token.as_str() {
            "debug" => profile = BuildProfile::Debug,
            "release" => profile = BuildProfile::Release,
            "loader" => mode = RemuxMode::Loader,
            "download" | "example" => mode = RemuxMode::Download,
            "all" => mode = RemuxMode::All,
            _ => {
                return Err(DesktopError::storage(format!(
                    "invalid desktop remux argument '{token}'. Usage: verify-remux [debug|release] [loader|download|example|all]"
                )));
            }
        }
    }
    Ok((profile, mode))
}

fn parse_diagnostics_tokens(
    tokens: &[String],
) -> Result<(BuildProfile, DiagnosticsMode), DesktopError> {
    let mut profile = BuildProfile::Debug;
    let mut mode = DiagnosticsMode::All;
    for token in tokens {
        match token.as_str() {
            "debug" => profile = BuildProfile::Debug,
            "release" => profile = BuildProfile::Release,
            "loader" => mode = DiagnosticsMode::Loader,
            "macos" => mode = DiagnosticsMode::Macos,
            "all" => mode = DiagnosticsMode::All,
            _ => {
                return Err(DesktopError::storage(format!(
                    "invalid decoder diagnostics argument '{token}'. Usage: verify-decoder-diagnostics [debug|release] [loader|macos|all]"
                )));
            }
        }
    }
    Ok((profile, mode))
}

fn parse_d3d11_tokens(tokens: &[String]) -> Result<(BuildProfile, D3d11Mode), DesktopError> {
    let mut profile = BuildProfile::Debug;
    let mut mode = D3d11Mode::Loader;
    for token in tokens {
        match token.as_str() {
            "debug" => profile = BuildProfile::Debug,
            "release" => profile = BuildProfile::Release,
            "loader" => mode = D3d11Mode::Loader,
            "all" => mode = D3d11Mode::All,
            _ => {
                return Err(DesktopError::storage(format!(
                    "invalid D3D11 verification argument '{token}'. Usage: verify-decoder-d3d11 [debug|release] [loader|all]"
                )));
            }
        }
    }
    Ok((profile, mode))
}

fn parse_videotoolbox_tokens(
    tokens: &[String],
) -> Result<(BuildProfile, VideoToolboxMode), DesktopError> {
    let mut profile = BuildProfile::Debug;
    let mut mode = VideoToolboxMode::Loader;
    for token in tokens {
        match token.as_str() {
            "debug" => profile = BuildProfile::Debug,
            "release" => profile = BuildProfile::Release,
            "loader" => mode = VideoToolboxMode::Loader,
            "playback" => mode = VideoToolboxMode::Playback,
            "basic-player" => mode = VideoToolboxMode::BasicPlayer,
            "all" => mode = VideoToolboxMode::All,
            _ => {
                return Err(DesktopError::storage(format!(
                    "invalid VideoToolbox verification argument '{token}'. Usage: verify-decoder-videotoolbox [debug|release] [loader|playback|basic-player|all]"
                )));
            }
        }
    }
    Ok((profile, mode))
}

fn run_loader_test(root: &Path, plugin_path: &Path) -> Result<(), DesktopError> {
    let mut command = cargo_command(root);
    command
        .args([
            "test",
            "-p",
            "player-plugin-loader",
            "tests::native_dynamic_tests::native_dynamic_loader_opens_ffmpeg_post_download_processor",
            "--",
            "--ignored",
            "--exact",
        ])
        .env("VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH", plugin_path);
    run_command(&mut command, "FFmpeg remux loader verification")
}

fn run_download_test(root: &Path, plugin_path: &Path) -> Result<(), DesktopError> {
    require_tool("ffmpeg")?;
    require_tool("ffprobe")?;
    let fixture = root.join("fixtures/media/tiny-h264-aac.m4v");
    if !fixture.is_file() {
        if is_ci_environment() {
            eprintln!(
                "Desktop remux fixture is missing in CI, skipping example remux verification: {}",
                fixture.display()
            );
            return Ok(());
        }
        return Err(DesktopError::storage(format!(
            "Desktop remux fixture is missing: {}",
            fixture.display()
        )));
    }
    let mut command = cargo_command(root);
    command
        .args([
            "test",
            "-p",
            "player-platform-desktop",
            "download::tests::desktop_export_remuxes_downloaded_hls_fixture_to_mp4_via_dynamic_plugin",
            "--",
            "--ignored",
            "--exact",
        ])
        .env("VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH", plugin_path);
    run_command(&mut command, "FFmpeg remux download verification")
}

fn run_decoder_loader_test(root: &Path, plugin_path: &Path) -> Result<(), DesktopError> {
    let mut command = cargo_command(root);
    command
        .args([
            "test",
            "-p",
            "player-plugin-loader",
            "tests::native_dynamic_tests::native_dynamic_loader_opens_decoder_fixture",
            "--",
            "--ignored",
            "--exact",
        ])
        .env("VESPER_DECODER_FIXTURE_PLUGIN_PATH", plugin_path)
        .env("VESPER_DECODER_PLUGIN_PATHS", plugin_path)
        .env("VESPER_DECODER_FIXTURE_CODECS", fixture_codecs());
    run_command(&mut command, "decoder fixture loader verification")
}

fn run_decoder_macos_test(
    root: &Path,
    plugin_path: &Path,
    output: &mut dyn io::Write,
) -> Result<(), DesktopError> {
    if env::consts::OS != "macos" {
        writeln!(
            output,
            "Skipping macOS decoder diagnostics test on {}.",
            env::consts::OS
        )
        .map_err(output_error)?;
        return Ok(());
    }
    let mut command = cargo_command(root);
    command
        .args([
            "test",
            "-p",
            "player-platform-macos",
            "tests::macos_runtime_diagnostics_loads_real_decoder_fixture_library",
            "--",
            "--ignored",
            "--exact",
        ])
        .env("VESPER_DECODER_FIXTURE_PLUGIN_PATH", plugin_path)
        .env("VESPER_DECODER_PLUGIN_PATHS", plugin_path)
        .env("VESPER_DECODER_FIXTURE_CODECS", fixture_codecs());
    run_command(&mut command, "macOS decoder diagnostics verification")
}

fn resolve_or_build_plugin(
    root: &Path,
    target_dir: &Path,
    profile: BuildProfile,
    package: &str,
    library_stem: &str,
    environment_name: &str,
) -> Result<PathBuf, DesktopError> {
    let override_path = env::var_os(environment_name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    if let Some(path) = override_path {
        if !path.is_file() {
            return Err(DesktopError::storage(format!(
                "{environment_name} points to a missing file: {}",
                path.display()
            )));
        }
        return Ok(normalize_runtime_path(&path));
    }

    let mut command = cargo_command(root);
    command.arg("build").arg("-p").arg(package);
    if profile == BuildProfile::Release {
        command.arg("--release");
    }
    run_command(&mut command, &format!("Cargo {package} build"))?;
    resolve_plugin_path(
        target_dir,
        profile,
        &shared_library_name(library_stem)?,
        environment_name,
        package,
    )
    .map(|path| normalize_runtime_path(&path))
}

fn resolve_videotoolbox_smoke_source(
    root: &Path,
    target_dir: &Path,
) -> Result<PathBuf, DesktopError> {
    if let Some(path) = env::var_os("VESPER_DECODER_VIDEOTOOLBOX_SOURCE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        if !path.is_file() {
            return Err(DesktopError::storage(format!(
                "VESPER_DECODER_VIDEOTOOLBOX_SOURCE points to a missing file: {}",
                path.display()
            )));
        }
        return Ok(path);
    }

    let generated = target_dir.join("videotoolbox-smoke-h264-bframes.mp4");
    if generated.is_file() {
        return Ok(generated);
    }
    require_tool("ffmpeg").map_err(|_| {
        DesktopError::storage(
            "ffmpeg is required to generate the VideoToolbox smoke source; install ffmpeg or set VESPER_DECODER_VIDEOTOOLBOX_SOURCE.",
        )
    })?;
    fs::create_dir_all(target_dir).map_err(|error| {
        DesktopError::storage(format!(
            "failed to create VideoToolbox smoke source directory {}: {error}",
            target_dir.display()
        ))
    })?;
    let mut command = Command::new("ffmpeg");
    command.current_dir(root).args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x180:rate=24:duration=2",
        "-c:v",
        "libx264",
        "-profile:v",
        "main",
        "-level:v",
        "3.1",
        "-bf",
        "3",
        "-g",
        "48",
        "-x264-params",
        "b-adapt=0:keyint=48:min-keyint=48:scenecut=0",
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
    ]);
    command.arg(&generated);
    run_command(&mut command, "VideoToolbox smoke source generation")?;
    Ok(generated)
}

fn require_videotoolbox_b_frames(root: &Path, source: &Path) -> Result<(), DesktopError> {
    require_tool("ffprobe").map_err(|_| {
        DesktopError::storage(
            "ffprobe is required to validate the VideoToolbox B-frame smoke source.",
        )
    })?;
    let mut command = Command::new("ffprobe");
    command.current_dir(root).args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=has_b_frames",
        "-of",
        "default=nw=1:nk=1",
    ]);
    command.arg(source);
    let bytes = run_bounded_stdout(
        &mut command,
        "VideoToolbox smoke source ffprobe",
        FFPROBE_OUTPUT_LIMIT,
    )?;
    let value = std::str::from_utf8(&bytes)
        .ok()
        .map(str::trim)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| {
            DesktopError::conformance(format!(
                "Could not read has_b_frames from VideoToolbox smoke source: {}",
                source.display()
            ))
        })?;
    if value == 0 {
        return Err(DesktopError::conformance(format!(
            "VideoToolbox smoke source must contain B-frames: {}",
            source.display()
        )));
    }
    Ok(())
}

fn run_bounded_stdout(
    command: &mut Command,
    label: &str,
    limit: usize,
) -> Result<Vec<u8>, DesktopError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            DesktopError::storage(format!(
                "Required command is unavailable for {label}: {error}"
            ))
        } else {
            DesktopError::worker(format!("failed to run {label}: {error}"))
        }
    })?;
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(DesktopError::worker(format!(
            "failed to capture {label} output"
        )));
    };
    let mut bytes = Vec::with_capacity(limit.min(4096));
    let mut buffer = [0_u8; 1024];
    let mut exceeded = false;
    loop {
        let count = stdout.read(&mut buffer).map_err(|error| {
            let _ = child.kill();
            let _ = child.wait();
            DesktopError::worker(format!("failed to read {label} output: {error}"))
        })?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(bytes.len());
        let retained = remaining.min(count);
        bytes.extend_from_slice(&buffer[..retained]);
        exceeded |= retained < count;
    }
    let status = child
        .wait()
        .map_err(|error| DesktopError::worker(format!("failed to reap {label}: {error}")))?;
    if !status.success() {
        return Err(DesktopError::conformance(format!(
            "{label} exited unsuccessfully ({status})"
        )));
    }
    if exceeded {
        return Err(DesktopError::conformance(format!(
            "{label} output exceeded {limit} bytes"
        )));
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Copy)]
struct VideoToolboxCargoTest {
    package: &'static str,
    test: &'static str,
    label: &'static str,
    ignored: bool,
    strict_frame_processor: bool,
}

const VIDEOTOOLBOX_LOADER_TESTS: [VideoToolboxCargoTest; 6] = [
    VideoToolboxCargoTest {
        package: "player-plugin-loader",
        test: "tests::native_dynamic_tests::native_dynamic_loader_opens_videotoolbox_decoder",
        label: "VideoToolbox decoder loader verification",
        ignored: true,
        strict_frame_processor: false,
    },
    VideoToolboxCargoTest {
        package: "player-platform-macos",
        test: "tests::macos_runtime_diagnostics_loads_real_videotoolbox_decoder_library",
        label: "VideoToolbox macOS runtime verification",
        ignored: true,
        strict_frame_processor: false,
    },
    VideoToolboxCargoTest {
        package: "player-platform-macos",
        test: "tests::macos_videotoolbox_decoder_decodes_ffmpeg_packets_headless",
        label: "VideoToolbox headless decode verification",
        ignored: true,
        strict_frame_processor: false,
    },
    VideoToolboxCargoTest {
        package: "player-platform-macos",
        test: "tests::macos_videotoolbox_decoder_flush_seek_and_eof_headless",
        label: "VideoToolbox headless lifecycle verification",
        ignored: true,
        strict_frame_processor: false,
    },
    VideoToolboxCargoTest {
        package: "player-platform-macos",
        test: "tests::macos_native_frame_source_switch_releases_old_source_and_decodes_new_source",
        label: "VideoToolbox source switch cleanup verification",
        ignored: false,
        strict_frame_processor: false,
    },
    VideoToolboxCargoTest {
        package: "player-platform-macos",
        test: "tests::source_normalizer_packet_source_drop_after_backpressure_has_no_outstanding_lease",
        label: "source normalizer lease cleanup verification",
        ignored: false,
        strict_frame_processor: false,
    },
];

const VIDEOTOOLBOX_PLAYBACK_TESTS: [VideoToolboxCargoTest; 5] = [
    VideoToolboxCargoTest {
        package: "player-platform-macos",
        test: "tests::macos_native_frame_decoder_plugin_runtime_probes_with_surface",
        label: "VideoToolbox surface playback verification",
        ignored: true,
        strict_frame_processor: false,
    },
    VideoToolboxCargoTest {
        package: "player-platform-macos",
        test: "tests::macos_native_frame_runtime_reopens_as_software_after_presenter_failure",
        label: "VideoToolbox software fallback verification",
        ignored: true,
        strict_frame_processor: false,
    },
    VideoToolboxCargoTest {
        package: "player-platform-macos",
        test: "tests::macos_native_frame_runtime_loads_frame_processor_diagnostic_plugin",
        label: "VideoToolbox FrameProcessor verification",
        ignored: true,
        strict_frame_processor: false,
    },
    VideoToolboxCargoTest {
        package: "player-platform-macos",
        test: "tests::macos_native_frame_strict_frame_processor_failure_does_not_fallback_to_software",
        label: "strict FrameProcessor runtime verification",
        ignored: true,
        strict_frame_processor: true,
    },
    VideoToolboxCargoTest {
        package: "player-platform-macos",
        test: "tests::macos_host_strict_frame_processor_failure_forwards_software_error_message",
        label: "strict FrameProcessor host verification",
        ignored: true,
        strict_frame_processor: true,
    },
];

fn run_videotoolbox_loader_tests(
    root: &Path,
    assets: &VideoToolboxAssets,
) -> Result<(), DesktopError> {
    run_videotoolbox_cargo_tests(root, assets, &VIDEOTOOLBOX_LOADER_TESTS)
}

fn run_videotoolbox_playback_tests(
    root: &Path,
    assets: &VideoToolboxAssets,
) -> Result<(), DesktopError> {
    run_videotoolbox_cargo_tests(root, assets, &VIDEOTOOLBOX_PLAYBACK_TESTS)
}

fn run_videotoolbox_cargo_tests(
    root: &Path,
    assets: &VideoToolboxAssets,
    tests: &[VideoToolboxCargoTest],
) -> Result<(), DesktopError> {
    for test in tests {
        let mut command = cargo_command(root);
        command
            .arg("test")
            .arg("-p")
            .arg(test.package)
            .arg(test.test)
            .arg("--");
        if test.ignored {
            command.arg("--ignored");
        }
        command
            .arg("--exact")
            .env(
                "VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH",
                &assets.decoder_plugin,
            )
            .env("VESPER_DECODER_VIDEOTOOLBOX_SOURCE", &assets.smoke_source);
        if let Some(frame_processor) = &assets.frame_processor_plugin {
            command.env(
                "VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH",
                frame_processor,
            );
        }
        if test.strict_frame_processor {
            command.env(
                "VESPER_FRAME_PROCESSOR_DIAGNOSTIC_MODE",
                "unsupported-handle",
            );
        }
        run_command(&mut command, test.label)?;
    }
    Ok(())
}

fn run_videotoolbox_basic_player(
    root: &Path,
    target_dir: &Path,
    profile: BuildProfile,
    assets: &VideoToolboxAssets,
    output: &mut dyn io::Write,
) -> Result<(), DesktopError> {
    let timeout_seconds = basic_player_timeout_seconds()?;
    let frame_processor = assets.frame_processor_plugin.as_ref().ok_or_else(|| {
        DesktopError::compatibility(
            "basic-player VideoToolbox verification requires a FrameProcessor plugin",
        )
    })?;

    let mut build = cargo_command(root);
    build.arg("build").arg("-p").arg("basic-player");
    if profile == BuildProfile::Release {
        build.arg("--release");
    }
    run_command(&mut build, "Cargo basic-player build")?;

    let basic_player = target_dir.join(profile.as_str()).join("basic-player");
    if !is_executable_file(&basic_player) {
        return Err(DesktopError::storage(format!(
            "Could not find built basic-player binary: {}",
            basic_player.display()
        )));
    }

    let temporary_log = tempfile::Builder::new()
        .prefix("vesper-basic-player-videotoolbox.")
        .suffix(".log")
        .tempfile()
        .map_err(|error| {
            DesktopError::storage(format!("failed to create basic-player smoke log: {error}"))
        })?;
    let log_path = temporary_log.path().to_path_buf();
    let (mut log_file, _) = temporary_log.keep().map_err(|error| {
        DesktopError::storage(format!(
            "failed to preserve basic-player smoke log {}: {}",
            log_path.display(),
            error.error
        ))
    })?;

    writeln!(
        output,
        "Running basic-player VideoToolbox smoke; log: {}",
        log_path.display()
    )
    .map_err(output_error)?;
    output.flush().map_err(output_error)?;

    let frame_processor_window = env::var_os("VESPER_FRAME_PROCESSOR_DEBUG_WINDOW")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "24".into());
    let playback_window = env::var_os("VESPER_PLAYBACK_DEBUG_WINDOW")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "24".into());
    let mut command = Command::new(&basic_player);
    command
        .current_dir(root)
        .arg(&assets.smoke_source)
        .env("VESPER_PLUGIN_DEVELOPMENT_MODE", "1")
        .env("VESPER_DECODER_PLUGIN_VIDEO_MODE", "native-frame")
        .env("VESPER_DECODER_PLUGIN_PATHS", &assets.decoder_plugin)
        .env("VESPER_FRAME_PROCESSOR_MODE", "prefer-processed")
        .env("VESPER_FRAME_PROCESSOR_PLUGIN_PATHS", frame_processor)
        .env("VESPER_FRAME_PROCESSOR_DIAGNOSTIC_MODE", "noop")
        .env("VESPER_FRAME_PROCESSOR_DEBUG", "1")
        .env(
            "VESPER_FRAME_PROCESSOR_DEBUG_WINDOW",
            frame_processor_window,
        )
        .env("VESPER_PLAYBACK_DEBUG", "1")
        .env("VESPER_PLAYBACK_DEBUG_WINDOW", playback_window)
        .env("VESPER_BASIC_PLAYER_SMOKE_SCRIPT", "1");

    let outcome = run_basic_player_process(
        &mut command,
        Duration::from_secs(timeout_seconds),
        &mut log_file,
        &log_path,
    )?;
    log_file.flush().map_err(|error| {
        DesktopError::worker(format!(
            "failed to flush basic-player smoke log {}: {error}",
            log_path.display()
        ))
    })?;
    validate_basic_player_outcome(&outcome, &log_path)?;
    writeln!(
        output,
        "basic-player VideoToolbox smoke passed; log: {}",
        log_path.display()
    )
    .map_err(output_error)
}

fn basic_player_timeout_seconds() -> Result<u64, DesktopError> {
    let value = env::var("VESPER_BASIC_PLAYER_SMOKE_TIMEOUT_SECONDS")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "60".to_owned());
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DesktopError::storage(
            "VESPER_BASIC_PLAYER_SMOKE_TIMEOUT_SECONDS must be a positive integer.",
        ));
    }
    value.parse::<u64>().map_err(|_| {
        DesktopError::storage(
            "VESPER_BASIC_PLAYER_SMOKE_TIMEOUT_SECONDS must be a positive integer.",
        )
    })
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    true
}

#[derive(Debug)]
struct BasicPlayerOutcome {
    status: ExitStatus,
    log: BasicPlayerLogScanner,
    truncated: bool,
}

#[derive(Debug)]
struct BasicPlayerLogScanner {
    required_entries: [bool; BASIC_PLAYER_REQUIRED_LOG_ENTRIES.len()],
    #[cfg(unix)]
    launch_failure: bool,
    deadline_misses: bool,
    dropped_outputs: bool,
    tail: Vec<u8>,
}

impl Default for BasicPlayerLogScanner {
    fn default() -> Self {
        Self {
            required_entries: [false; BASIC_PLAYER_REQUIRED_LOG_ENTRIES.len()],
            #[cfg(unix)]
            launch_failure: false,
            deadline_misses: false,
            dropped_outputs: false,
            tail: Vec::new(),
        }
    }
}

impl BasicPlayerLogScanner {
    #[cfg(unix)]
    fn scan(&mut self, bytes: &[u8]) {
        let mut combined = Vec::with_capacity(self.tail.len().saturating_add(bytes.len()));
        combined.extend_from_slice(&self.tail);
        combined.extend_from_slice(bytes);
        let text = String::from_utf8_lossy(&combined);
        for (index, (pattern, _)) in BASIC_PLAYER_REQUIRED_LOG_ENTRIES.iter().enumerate() {
            self.required_entries[index] |= text.contains(pattern);
        }
        self.launch_failure |=
            text.contains("desktop launch failed") || text.contains("panicked at");
        self.deadline_misses |= metric_has_nonzero_value(&text, "deadline_misses=");
        self.dropped_outputs |= metric_has_nonzero_value(&text, "dropped_outputs=");

        let retained_start = combined.len().saturating_sub(BASIC_PLAYER_LOG_TAIL_LIMIT);
        self.tail.clear();
        self.tail.extend_from_slice(&combined[retained_start..]);
    }

    #[cfg(unix)]
    fn playback_ended(&self) -> bool {
        self.required_entries[BASIC_PLAYER_REQUIRED_LOG_ENTRIES.len() - 1]
    }

    fn diagnostic_tail(&self) -> String {
        let text = String::from_utf8_lossy(&self.tail);
        let lines: Vec<&str> = text.lines().collect();
        let start = lines.len().saturating_sub(80);
        if lines.is_empty() {
            "Last log lines: (log was empty)".to_owned()
        } else {
            format!("Last log lines:\n{}", lines[start..].join("\n"))
        }
    }
}

#[cfg(unix)]
fn metric_has_nonzero_value(text: &str, prefix: &str) -> bool {
    let mut remainder = text;
    while let Some(index) = remainder.find(prefix) {
        let digits = remainder[index + prefix.len()..]
            .bytes()
            .take_while(u8::is_ascii_digit);
        if digits.into_iter().any(|digit| digit != b'0') {
            return true;
        }
        remainder = &remainder[index + prefix.len()..];
    }
    false
}

fn validate_basic_player_outcome(
    outcome: &BasicPlayerOutcome,
    log_path: &Path,
) -> Result<(), DesktopError> {
    for (index, (_, description)) in BASIC_PLAYER_REQUIRED_LOG_ENTRIES.iter().enumerate() {
        if !outcome.log.required_entries[index] {
            return Err(DesktopError::conformance(format!(
                "basic-player smoke did not report {description}.\nLog: {}\n{}",
                log_path.display(),
                outcome.log.diagnostic_tail()
            )));
        }
    }
    if outcome.log.deadline_misses {
        return Err(DesktopError::conformance(format!(
            "basic-player smoke reported FrameProcessor deadline misses.\nLog: {}\n{}",
            log_path.display(),
            outcome.log.diagnostic_tail()
        )));
    }
    if outcome.log.dropped_outputs {
        return Err(DesktopError::conformance(format!(
            "basic-player smoke reported dropped FrameProcessor outputs.\nLog: {}\n{}",
            log_path.display(),
            outcome.log.diagnostic_tail()
        )));
    }
    if outcome.truncated {
        return Err(DesktopError::conformance(format!(
            "basic-player smoke log exceeded the {BASIC_PLAYER_LOG_LIMIT}-byte capture limit.\nLog: {}",
            log_path.display()
        )));
    }
    if !basic_player_status_is_expected(&outcome.status) {
        return Err(DesktopError::conformance(format!(
            "basic-player smoke exited unexpectedly with status {}.\nLog: {}\n{}",
            outcome.status,
            log_path.display(),
            outcome.log.diagnostic_tail()
        )));
    }
    Ok(())
}

fn basic_player_status_is_expected(status: &ExitStatus) -> bool {
    if status.success() || matches!(status.code(), Some(130 | 143)) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        matches!(status.signal(), Some(signal_hook::consts::SIGINT | 15))
    }
    #[cfg(not(unix))]
    false
}

#[cfg(unix)]
#[derive(Debug)]
enum BasicPlayerCaptureEvent {
    Data(Vec<u8>),
    ReadError(String),
}

#[cfg(unix)]
#[derive(Debug, Default)]
struct BasicPlayerCaptureState {
    scanner: BasicPlayerLogScanner,
    bytes_written: usize,
    truncated: bool,
    read_error: Option<String>,
}

#[cfg(unix)]
fn run_basic_player_process(
    command: &mut Command,
    timeout: Duration,
    log_file: &mut fs::File,
    log_path: &Path,
) -> Result<BasicPlayerOutcome, DesktopError> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{RecvTimeoutError, sync_channel};
    use std::time::Instant;

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_background_process_group(command);
    let mut child = command.spawn().map_err(|error| {
        DesktopError::worker(format!("failed to start basic-player smoke: {error}"))
    })?;
    let process_group = i32::try_from(child.id()).map_err(|_| {
        let _ = child.kill();
        let _ = child.wait();
        DesktopError::worker("basic-player process id cannot be represented as a process group")
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        abort_basic_player(&mut child, process_group);
        DesktopError::worker("failed to capture basic-player stdout")
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        abort_basic_player(&mut child, process_group);
        DesktopError::worker("failed to capture basic-player stderr")
    })?;

    let (sender, receiver) = sync_channel(BASIC_PLAYER_CAPTURE_QUEUE_CAPACITY);
    spawn_basic_player_capture(stdout, sender.clone());
    spawn_basic_player_capture(stderr, sender.clone());
    drop(sender);

    let cancelled = Arc::new(AtomicBool::new(false));
    let signal_id =
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&cancelled)).map_err(
            |error| {
                abort_basic_player(&mut child, process_group);
                DesktopError::worker(format!(
                    "failed to install basic-player cancellation handler: {error}"
                ))
            },
        )?;
    let signal_guard = DesktopSignalRegistration(signal_id);
    let started = Instant::now();
    let mut capture = BasicPlayerCaptureState::default();
    let mut stop_reason = None;
    let status = loop {
        drain_basic_player_capture(&receiver, log_file, &mut capture)?;
        if capture.read_error.is_some() {
            stop_reason = Some(BasicPlayerStopReason::CaptureFailure);
        } else if cancelled.load(Ordering::Acquire) {
            stop_reason = Some(BasicPlayerStopReason::Cancelled);
        } else if capture.scanner.playback_ended() || capture.scanner.launch_failure {
            stop_reason = Some(BasicPlayerStopReason::ObservedTerminalLog);
        } else if started.elapsed() >= timeout {
            stop_reason = Some(BasicPlayerStopReason::TimedOut);
        }

        if let Some(reason) = stop_reason {
            break terminate_basic_player(&mut child, process_group, reason)?;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                cleanup_basic_player_descendants(process_group);
                break status;
            }
            Ok(None) => {}
            Err(error) => {
                abort_basic_player(&mut child, process_group);
                return Err(DesktopError::worker(format!(
                    "failed to poll basic-player smoke: {error}"
                )));
            }
        }
        std::thread::sleep(BASIC_PLAYER_POLL_INTERVAL);
    };
    drop(signal_guard);

    let drain_deadline = Instant::now() + BASIC_PLAYER_CAPTURE_DRAIN_TIMEOUT;
    loop {
        match receiver.recv_timeout(BASIC_PLAYER_POLL_INTERVAL) {
            Ok(event) => handle_basic_player_capture(event, log_file, &mut capture)?,
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) if Instant::now() < drain_deadline => {}
            Err(RecvTimeoutError::Timeout) => {
                return Err(DesktopError::worker(format!(
                    "basic-player output streams did not close after process exit; log: {}",
                    log_path.display()
                )));
            }
        }
    }
    log_file.flush().map_err(|error| {
        DesktopError::worker(format!(
            "failed to flush basic-player smoke log {}: {error}",
            log_path.display()
        ))
    })?;

    if let Some(error) = capture.read_error {
        return Err(DesktopError::worker(format!(
            "failed to capture basic-player output: {error}; log: {}",
            log_path.display()
        )));
    }
    match stop_reason {
        Some(BasicPlayerStopReason::Cancelled) => {
            return Err(DesktopError::worker(format!(
                "basic-player smoke was cancelled; log: {}",
                log_path.display()
            )));
        }
        Some(BasicPlayerStopReason::TimedOut) => {
            return Err(DesktopError::worker(format!(
                "basic-player smoke timed out after {} seconds; log: {}",
                timeout.as_secs(),
                log_path.display()
            )));
        }
        Some(BasicPlayerStopReason::CaptureFailure)
        | Some(BasicPlayerStopReason::ObservedTerminalLog)
        | None => {}
    }
    Ok(BasicPlayerOutcome {
        status,
        log: capture.scanner,
        truncated: capture.truncated,
    })
}

#[cfg(not(unix))]
fn run_basic_player_process(
    _command: &mut Command,
    _timeout: Duration,
    _log_file: &mut fs::File,
    _log_path: &Path,
) -> Result<BasicPlayerOutcome, DesktopError> {
    Err(DesktopError::compatibility(
        "basic-player VideoToolbox smoke process supervision requires Unix",
    ))
}

#[cfg(unix)]
fn spawn_basic_player_capture<R>(
    mut reader: R,
    sender: std::sync::mpsc::SyncSender<BasicPlayerCaptureEvent>,
) where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if sender
                        .send(BasicPlayerCaptureEvent::Data(buffer[..count].to_vec()))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(BasicPlayerCaptureEvent::ReadError(error.to_string()));
                    break;
                }
            }
        }
    });
}

#[cfg(unix)]
fn drain_basic_player_capture(
    receiver: &std::sync::mpsc::Receiver<BasicPlayerCaptureEvent>,
    log_file: &mut fs::File,
    capture: &mut BasicPlayerCaptureState,
) -> Result<(), DesktopError> {
    use std::sync::mpsc::TryRecvError;

    for _ in 0..BASIC_PLAYER_CAPTURE_QUEUE_CAPACITY {
        match receiver.try_recv() {
            Ok(event) => handle_basic_player_capture(event, log_file, capture)?,
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
    Ok(())
}

#[cfg(unix)]
fn handle_basic_player_capture(
    event: BasicPlayerCaptureEvent,
    log_file: &mut fs::File,
    capture: &mut BasicPlayerCaptureState,
) -> Result<(), DesktopError> {
    match event {
        BasicPlayerCaptureEvent::Data(bytes) => {
            capture.scanner.scan(&bytes);
            let remaining = BASIC_PLAYER_LOG_LIMIT.saturating_sub(capture.bytes_written);
            let retained = remaining.min(bytes.len());
            if retained > 0 {
                log_file.write_all(&bytes[..retained]).map_err(|error| {
                    DesktopError::worker(format!("failed to write basic-player smoke log: {error}"))
                })?;
                capture.bytes_written += retained;
            }
            capture.truncated |= retained < bytes.len();
        }
        BasicPlayerCaptureEvent::ReadError(error) => {
            if capture.read_error.is_none() {
                capture.read_error = Some(error);
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BasicPlayerStopReason {
    ObservedTerminalLog,
    Cancelled,
    TimedOut,
    CaptureFailure,
}

#[cfg(unix)]
fn terminate_basic_player(
    child: &mut std::process::Child,
    process_group: i32,
    reason: BasicPlayerStopReason,
) -> Result<ExitStatus, DesktopError> {
    use nix::sys::signal::Signal;
    use std::time::Instant;

    match child.try_wait() {
        Ok(Some(status)) => {
            cleanup_basic_player_descendants(process_group);
            return Ok(status);
        }
        Ok(None) => {}
        Err(error) => {
            return Err(DesktopError::worker(format!(
                "failed to poll basic-player smoke before termination: {error}"
            )));
        }
    }

    let initial_signal = match reason {
        BasicPlayerStopReason::ObservedTerminalLog | BasicPlayerStopReason::Cancelled => {
            Signal::SIGINT
        }
        BasicPlayerStopReason::TimedOut | BasicPlayerStopReason::CaptureFailure => Signal::SIGTERM,
    };
    let initial_error = signal_basic_player_group(process_group, initial_signal).err();
    if initial_error.is_some() {
        let _ = child.kill();
    }
    let initial_grace = if initial_signal == Signal::SIGINT {
        BASIC_PLAYER_INTERRUPT_GRACE
    } else {
        BASIC_PLAYER_TERMINATION_GRACE
    };
    if let Some(status) = poll_basic_player_exit(child, Instant::now() + initial_grace)? {
        cleanup_basic_player_descendants(process_group);
        return initial_error.map_or(Ok(status), Err);
    }

    let termination_error = signal_basic_player_group(process_group, Signal::SIGTERM).err();
    if let Some(status) =
        poll_basic_player_exit(child, Instant::now() + BASIC_PLAYER_TERMINATION_GRACE)?
    {
        cleanup_basic_player_descendants(process_group);
        return initial_error.or(termination_error).map_or(Ok(status), Err);
    }

    let kill_error = signal_basic_player_group(process_group, Signal::SIGKILL).err();
    let _ = child.kill();
    if let Some(status) = poll_basic_player_exit(child, Instant::now() + BASIC_PLAYER_REAP_TIMEOUT)?
    {
        return initial_error
            .or(termination_error)
            .or(kill_error)
            .map_or(Ok(status), Err);
    }
    Err(DesktopError::worker(
        "basic-player smoke could not be reaped after termination",
    ))
}

#[cfg(unix)]
fn poll_basic_player_exit(
    child: &mut std::process::Child,
    deadline: std::time::Instant,
) -> Result<Option<ExitStatus>, DesktopError> {
    while std::time::Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) => std::thread::sleep(BASIC_PLAYER_POLL_INTERVAL),
            Err(error) => {
                return Err(DesktopError::worker(format!(
                    "failed to reap basic-player smoke: {error}"
                )));
            }
        }
    }
    Ok(None)
}

#[cfg(unix)]
fn signal_basic_player_group(
    process_group: i32,
    signal: nix::sys::signal::Signal,
) -> Result<(), DesktopError> {
    use nix::errno::Errno;
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;

    match killpg(Pid::from_raw(process_group), signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(DesktopError::worker(format!(
            "failed to signal basic-player process group: {error}"
        ))),
    }
}

#[cfg(unix)]
fn cleanup_basic_player_descendants(process_group: i32) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    let process_group = Pid::from_raw(process_group);
    let _ = killpg(process_group, Signal::SIGTERM);
    std::thread::sleep(Duration::from_millis(20));
    let _ = killpg(process_group, Signal::SIGKILL);
}

#[cfg(unix)]
fn abort_basic_player(child: &mut std::process::Child, process_group: i32) {
    let _ = terminate_basic_player(child, process_group, BasicPlayerStopReason::CaptureFailure);
}

#[cfg(unix)]
struct DesktopSignalRegistration(signal_hook::SigId);

#[cfg(unix)]
impl Drop for DesktopSignalRegistration {
    fn drop(&mut self) {
        signal_hook::low_level::unregister(self.0);
    }
}

fn fixture_codecs() -> String {
    env::var("VESPER_DECODER_FIXTURE_CODECS")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "fixture-video,H264,HEVC".to_owned())
}

fn cargo_command(root: &Path) -> Command {
    let cargo = env::var_os("CARGO")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.current_dir(root);
    command
}

fn target_directory(root: &Path) -> PathBuf {
    target_directory_from_override(root, env::var_os("CARGO_TARGET_DIR").as_deref())
}

fn target_directory_from_override(root: &Path, value: Option<&OsStr>) -> PathBuf {
    match value {
        Some(value) if !value.is_empty() => {
            let path = Path::new(value);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                root.join(path)
            }
        }
        _ => root.join("target"),
    }
}

fn shared_library_name(stem: &str) -> Result<String, DesktopError> {
    shared_library_name_for(env::consts::OS, stem)
}

fn shared_library_name_for(platform: &str, stem: &str) -> Result<String, DesktopError> {
    match platform {
        "macos" => Ok(format!("lib{stem}.dylib")),
        "linux" => Ok(format!("lib{stem}.so")),
        "windows" => Ok(format!("{stem}.dll")),
        platform => Err(DesktopError::compatibility(format!(
            "unsupported desktop platform for dynamic plugin verification: {platform}"
        ))),
    }
}

fn normalize_runtime_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

fn resolve_plugin_path(
    target_dir: &Path,
    profile: BuildProfile,
    library_name: &str,
    environment_name: &str,
    crate_label: &str,
) -> Result<PathBuf, DesktopError> {
    let candidates = [
        target_dir.join(profile.as_str()).join(library_name),
        target_dir
            .join(profile.as_str())
            .join("deps")
            .join(library_name),
        target_dir.join("debug").join(library_name),
        target_dir.join("debug").join("deps").join(library_name),
        target_dir.join("release").join(library_name),
        target_dir.join("release").join("deps").join(library_name),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            DesktopError::storage(format!(
                "Could not find {library_name} under {}; build {crate_label} first or set {environment_name}.",
                target_dir.display()
            ))
        })
}

fn require_tool(tool: &str) -> Result<(), DesktopError> {
    if command_available(tool) {
        Ok(())
    } else {
        Err(DesktopError::storage(format!(
            "Required tool is unavailable: {tool}"
        )))
    }
}

fn command_available(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.is_file();
    }
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&paths).any(|directory| {
        let candidate = directory.join(command);
        if candidate.is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            for extension in [".exe", ".cmd", ".bat"] {
                if directory.join(format!("{command}{extension}")).is_file() {
                    return true;
                }
            }
        }
        false
    })
}

fn is_ci_environment() -> bool {
    env::var("CI").is_ok_and(|value| value == "true") || env::var_os("GITHUB_ACTIONS").is_some()
}

fn run_command(command: &mut Command, label: &str) -> Result<(), DesktopError> {
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = command.status().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            DesktopError::storage(format!(
                "Required command is unavailable for {label}: {error}"
            ))
        } else {
            DesktopError::worker(format!("failed to run {label}: {error}"))
        }
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(DesktopError::conformance(format!(
            "{label} exited unsuccessfully ({status})"
        )))
    }
}

fn output_error(error: io::Error) -> DesktopError {
    DesktopError::worker(format!(
        "failed to write desktop verification output: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    fn write_complete_desktop_install(root: &Path) {
        fs::create_dir_all(root.join("lib/pkgconfig")).expect("create pkg-config directory");
        for component in DESKTOP_REQUIRED_COMPONENTS {
            fs::write(
                root.join(format!("lib/pkgconfig/lib{component}.pc")),
                b"Version: 8.1.2\n",
            )
            .expect("write pkg-config fixture");
            fs::write(root.join(format!("lib/lib{component}.a")), b"archive\n")
                .expect("write static archive fixture");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn desktop_install_requires_every_linked_component() {
        let directory = tempfile::tempdir().expect("create desktop install fixture");
        let root = directory.path().join("install");
        fs::create_dir_all(root.join("lib/pkgconfig")).expect("create partial install");
        fs::write(root.join("lib/pkgconfig/libavutil.pc"), b"Version: 8.1.2\n")
            .expect("write partial pkg-config metadata");
        assert!(!desktop_install_complete(&root).expect("inspect partial install"));

        write_complete_desktop_install(&root);
        assert!(desktop_install_complete(&root).expect("inspect complete install"));
        fs::remove_file(root.join("lib/libavfilter.a")).expect("remove required archive");
        assert!(!desktop_install_complete(&root).expect("inspect incomplete archive set"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn desktop_source_checksum_rejects_mismatched_archives() {
        let directory = tempfile::tempdir().expect("create desktop source fixture");
        let archive = directory.path().join("ffmpeg.tar.xz");
        fs::write(&archive, b"not the locked archive").expect("write source fixture");
        let expected = hex::encode(Sha256::digest(b"locked archive"));
        let error = verify_desktop_source_sha256(&archive, Some(&expected))
            .expect_err("reject mismatched source archive");
        assert!(error.to_string().contains("checksum mismatch"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn desktop_publication_reports_and_preserves_failed_rollback() {
        let directory = tempfile::tempdir().expect("create desktop publication fixture");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::create_dir_all(&source).expect("create replacement install");
        fs::write(source.join("new.txt"), b"new\n").expect("write replacement file");
        fs::create_dir_all(&target).expect("create previous install");
        fs::write(target.join("old.txt"), b"old\n").expect("write previous file");
        let mut occupy_target = |path: &Path| {
            fs::create_dir_all(path)?;
            fs::write(path.join("concurrent.txt"), b"concurrent\n")
        };

        let error = publish_desktop_install_with_hook(&source, &target, Some(&mut occupy_target))
            .expect_err("publication and rollback must fail");
        let message = error.to_string();
        assert!(message.contains("failed to restore the previous install"));
        let marker = "recovery data was preserved at '";
        let recovery = message
            .split_once(marker)
            .and_then(|(_, suffix)| suffix.strip_suffix('\''))
            .expect("extract recovery path");
        let recovery = Path::new(recovery);
        assert_eq!(
            fs::read(recovery.join("old.txt")).expect("read recovered previous install"),
            b"old\n"
        );
        assert_eq!(
            fs::read(target.join("concurrent.txt")).expect("read concurrent target"),
            b"concurrent\n"
        );
        assert!(source.join("new.txt").is_file());
    }

    #[test]
    fn remux_arguments_accept_the_legacy_order_and_example_alias() {
        assert_eq!(
            parse_remux_tokens(&["loader".to_owned(), "release".to_owned()]).expect("tokens"),
            (BuildProfile::Release, RemuxMode::Loader)
        );
        assert_eq!(
            parse_remux_tokens(&["debug".to_owned(), "example".to_owned()]).expect("tokens"),
            (BuildProfile::Debug, RemuxMode::Download)
        );
    }

    #[test]
    fn diagnostics_arguments_accept_loader_and_macos_modes() {
        assert_eq!(
            parse_diagnostics_tokens(&["release".to_owned(), "loader".to_owned()]).expect("tokens"),
            (BuildProfile::Release, DiagnosticsMode::Loader)
        );
    }

    #[test]
    fn d3d11_contract_uses_the_windows_dll_name() {
        assert_eq!(
            shared_library_name_for("windows", "vesper_decoder_d3d11").expect("name"),
            "vesper_decoder_d3d11.dll"
        );
        assert_eq!(
            parse_d3d11_tokens(&["release".to_owned(), "all".to_owned()]).expect("tokens"),
            (BuildProfile::Release, D3d11Mode::All)
        );
    }

    #[test]
    fn videotoolbox_arguments_preserve_legacy_modes_and_order() {
        assert_eq!(
            parse_videotoolbox_tokens(&["all".to_owned(), "release".to_owned()]).expect("tokens"),
            (BuildProfile::Release, VideoToolboxMode::All)
        );
        assert_eq!(
            parse_videotoolbox_tokens(&["debug".to_owned(), "basic-player".to_owned()])
                .expect("tokens"),
            (BuildProfile::Debug, VideoToolboxMode::BasicPlayer)
        );
        assert_eq!(
            parse_videotoolbox_tokens(&[]).expect("default tokens"),
            (BuildProfile::Debug, VideoToolboxMode::Loader)
        );
    }

    #[cfg(unix)]
    #[test]
    fn basic_player_log_scanner_handles_split_markers_and_nonzero_metrics() {
        let mut scanner = BasicPlayerLogScanner::default();
        scanner.scan(b"initialized desktop pla");
        scanner.scan(b"yer\ndeadline_misses=0 dropped_outputs=000\n");
        assert!(scanner.required_entries[0]);
        assert!(!scanner.deadline_misses);
        assert!(!scanner.dropped_outputs);

        scanner.scan(b"deadline_misses=2 dropped_outputs=10\n");
        assert!(scanner.deadline_misses);
        assert!(scanner.dropped_outputs);
    }

    #[test]
    fn target_directory_honors_relative_and_absolute_overrides() {
        let root = Path::new("/tmp/vesper");
        assert_eq!(
            target_directory_from_override(root, None),
            root.join("target")
        );
        assert_eq!(
            target_directory_from_override(root, Some(OsStr::new("custom-target"))),
            root.join("custom-target")
        );
        assert_eq!(
            target_directory_from_override(root, Some(OsStr::new("/private/tmp/vesper-target"))),
            PathBuf::from("/private/tmp/vesper-target")
        );
        assert_eq!(
            target_directory_from_override(root, Some(OsStr::new(""))),
            root.join("target")
        );
    }
}
