use std::collections::VecDeque;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::external_process;

const FFMPEG_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_TOOL_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_OUTPUT_ENTRIES: usize = 128;
const MAX_OUTPUT_DEPTH: usize = 8;
const MAX_OUTPUT_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_OUTPUT_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaErrorKind {
    Storage,
    Compatibility,
    Conformance,
    Worker,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct MediaError {
    kind: MediaErrorKind,
    message: String,
}

impl MediaError {
    fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: MediaErrorKind::Storage,
            message: message.into(),
        }
    }

    fn compatibility(message: impl Into<String>) -> Self {
        Self {
            kind: MediaErrorKind::Compatibility,
            message: message.into(),
        }
    }

    fn conformance(message: impl Into<String>) -> Self {
        Self {
            kind: MediaErrorKind::Conformance,
            message: message.into(),
        }
    }

    fn worker(message: impl Into<String>) -> Self {
        Self {
            kind: MediaErrorKind::Worker,
            message: message.into(),
        }
    }

    pub(crate) const fn kind(&self) -> MediaErrorKind {
        self.kind
    }
}

pub(crate) fn generate_source_normalizer_fixtures(
    root: &Path,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), MediaError> {
    let media_root = root.join("fixtures/media");
    require_directory(&media_root, "media fixture directory")?;
    let source = media_root.join("tiny-h264-aac.m4v");
    require_regular_file(&source, "SourceNormalizer fixture source")?;
    let ffmpeg = env::var_os("FFMPEG_BIN").unwrap_or_else(|| OsString::from("ffmpeg"));
    let staging = tempfile::Builder::new()
        .prefix(".vesper-source-normalizer-stage-")
        .tempdir_in(&media_root)
        .map_err(|error| {
            MediaError::storage(format!(
                "failed to create SourceNormalizer fixture staging directory: {error}"
            ))
        })?;
    let generated = staging.path();
    fs::create_dir_all(generated.join("nonstandard")).map_err(|error| {
        MediaError::storage(format!(
            "failed to create HLS fixture staging directory: {error}"
        ))
    })?;
    fs::create_dir_all(generated.join("weird-dash")).map_err(|error| {
        MediaError::storage(format!(
            "failed to create DASH fixture staging directory: {error}"
        ))
    })?;

    run_ffmpeg(
        root,
        &ffmpeg,
        "H.264 FLV fixture generation",
        &[
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
            "-i".into(),
            source.as_os_str().to_owned(),
            "-map".into(),
            "0".into(),
            "-c".into(),
            "copy".into(),
            "-f".into(),
            "flv".into(),
            generated.join("tiny-h264-aac.flv").into_os_string(),
        ],
    )?;

    let hevc = run_ffmpeg_allow_failure(
        root,
        &ffmpeg,
        "HEVC FLV fixture generation",
        &[
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
            "-i".into(),
            source.as_os_str().to_owned(),
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            "0:a:0?".into(),
            "-c:v".into(),
            "libx265".into(),
            "-tag:v".into(),
            "hvc1".into(),
            "-preset".into(),
            "ultrafast".into(),
            "-x265-params".into(),
            "log-level=error".into(),
            "-c:a".into(),
            "copy".into(),
            "-f".into(),
            "flv".into(),
            generated.join("tiny-hevc-aac.flv").into_os_string(),
        ],
    )?;
    if !hevc.status.success() {
        let path = generated.join("tiny-hevc-aac.flv");
        if path.exists() {
            fs::remove_file(&path).map_err(|error| {
                MediaError::storage(format!(
                    "failed to remove incomplete optional HEVC fixture '{}': {error}",
                    path.display()
                ))
            })?;
        }
        writeln!(
            diagnostics,
            "warning: HEVC FLV fixture generation failed ({}): {}; supply fixtures/media/generated/tiny-hevc-aac.flv locally for that smoke case",
            hevc.status,
            process_diagnostics(&hevc)
        )
        .map_err(output_error)?;
    }

    run_ffmpeg(
        root,
        &ffmpeg,
        "fragmented MP4 fixture generation",
        &[
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
            "-i".into(),
            source.as_os_str().to_owned(),
            "-map".into(),
            "0".into(),
            "-c".into(),
            "copy".into(),
            "-movflags".into(),
            "frag_keyframe+empty_moov+default_base_moof".into(),
            generated
                .join("tiny-broken-progressive.mp4")
                .into_os_string(),
        ],
    )?;
    run_ffmpeg(
        root,
        &ffmpeg,
        "HLS fixture generation",
        &[
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
            "-i".into(),
            source.as_os_str().to_owned(),
            "-map".into(),
            "0".into(),
            "-c".into(),
            "copy".into(),
            "-f".into(),
            "hls".into(),
            "-hls_time".into(),
            "1".into(),
            "-hls_list_size".into(),
            "3".into(),
            "-hls_segment_type".into(),
            "fmp4".into(),
            "-hls_fmp4_init_filename".into(),
            "init.mp4".into(),
            "-hls_segment_filename".into(),
            generated
                .join("nonstandard/segment_%05d.m4s")
                .into_os_string(),
            generated.join("nonstandard/index.m3u8").into_os_string(),
        ],
    )?;
    run_ffmpeg(
        root,
        &ffmpeg,
        "DASH fixture generation",
        &[
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
            "-i".into(),
            source.as_os_str().to_owned(),
            "-map".into(),
            "0".into(),
            "-c".into(),
            "copy".into(),
            "-f".into(),
            "dash".into(),
            "-seg_duration".into(),
            "1".into(),
            "-use_template".into(),
            "1".into(),
            "-use_timeline".into(),
            "0".into(),
            "-init_seg_name".into(),
            "init-$RepresentationID$.mp4".into(),
            "-media_seg_name".into(),
            "chunk-$RepresentationID$-$Number%05d$.m4s".into(),
            generated.join("weird-dash/manifest.mpd").into_os_string(),
        ],
    )?;

    validate_generated_tree(generated)?;
    let target = media_root.join("generated");
    promote_generated_directory(staging, &target)?;
    writeln!(
        output,
        "Generated SourceNormalizer smoke fixtures under:\n  {}",
        target.display()
    )
    .map_err(output_error)
}

fn run_ffmpeg(
    root: &Path,
    executable: &OsStr,
    label: &str,
    arguments: &[OsString],
) -> Result<(), MediaError> {
    let captured = run_ffmpeg_allow_failure(root, executable, label, arguments)?;
    if captured.status.success() {
        Ok(())
    } else {
        Err(MediaError::conformance(format!(
            "{label} exited unsuccessfully ({}): {}",
            captured.status,
            process_diagnostics(&captured)
        )))
    }
}

fn run_ffmpeg_allow_failure(
    root: &Path,
    executable: &OsStr,
    label: &str,
    arguments: &[OsString],
) -> Result<external_process::BoundedProcessOutput, MediaError> {
    let mut command = Command::new(executable);
    command.current_dir(root).args(arguments);
    external_process::run_interruptible_capture_with_timeout(
        &mut command,
        label,
        MAX_TOOL_OUTPUT_BYTES,
        MAX_TOOL_OUTPUT_BYTES,
        FFMPEG_TIMEOUT,
    )
    .map_err(map_process_error)
}

fn process_diagnostics(captured: &external_process::BoundedProcessOutput) -> String {
    let stderr = String::from_utf8_lossy(&captured.stderr);
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_owned();
    }
    let stdout = String::from_utf8_lossy(&captured.stdout);
    let stdout = stdout.trim();
    if stdout.is_empty() {
        String::from("no process diagnostics")
    } else {
        stdout.to_owned()
    }
}

fn validate_generated_tree(root: &Path) -> Result<(), MediaError> {
    for relative in [
        "tiny-h264-aac.flv",
        "tiny-broken-progressive.mp4",
        "nonstandard/index.m3u8",
        "nonstandard/init.mp4",
        "weird-dash/manifest.mpd",
    ] {
        require_regular_file(&root.join(relative), "generated SourceNormalizer fixture")?;
    }
    let mut pending = VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut entries = 0usize;
    let mut total_bytes = 0u64;
    while let Some((directory, depth)) = pending.pop_front() {
        if depth > MAX_OUTPUT_DEPTH {
            return Err(MediaError::conformance(format!(
                "generated fixture tree exceeds depth {MAX_OUTPUT_DEPTH}"
            )));
        }
        for entry in fs::read_dir(&directory).map_err(|error| {
            MediaError::storage(format!(
                "failed to enumerate generated fixture directory '{}': {error}",
                directory.display()
            ))
        })? {
            let entry = entry.map_err(|error| {
                MediaError::storage(format!("failed to read generated fixture entry: {error}"))
            })?;
            entries = entries.checked_add(1).ok_or_else(|| {
                MediaError::conformance("generated fixture entry count overflowed")
            })?;
            if entries > MAX_OUTPUT_ENTRIES {
                return Err(MediaError::conformance(format!(
                    "generated fixture tree exceeds {MAX_OUTPUT_ENTRIES} entries"
                )));
            }
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
                MediaError::storage(format!(
                    "failed to inspect generated fixture '{}': {error}",
                    entry.path().display()
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(MediaError::conformance(format!(
                    "generated fixture tree contains a symlink: {}",
                    entry.path().display()
                )));
            }
            if metadata.is_dir() {
                pending.push_back((entry.path(), depth + 1));
            } else if metadata.is_file() {
                if metadata.len() > MAX_OUTPUT_FILE_BYTES {
                    return Err(MediaError::conformance(format!(
                        "generated fixture '{}' exceeds {MAX_OUTPUT_FILE_BYTES} bytes",
                        entry.path().display()
                    )));
                }
                total_bytes = total_bytes
                    .checked_add(metadata.len())
                    .ok_or_else(|| MediaError::conformance("generated fixture size overflowed"))?;
                if total_bytes > MAX_OUTPUT_TOTAL_BYTES {
                    return Err(MediaError::conformance(format!(
                        "generated fixture tree exceeds {MAX_OUTPUT_TOTAL_BYTES} bytes"
                    )));
                }
            } else {
                return Err(MediaError::conformance(format!(
                    "generated fixture tree contains an unsupported node: {}",
                    entry.path().display()
                )));
            }
        }
    }
    Ok(())
}

fn promote_generated_directory(
    staging: tempfile::TempDir,
    target: &Path,
) -> Result<(), MediaError> {
    let parent = target.parent().ok_or_else(|| {
        MediaError::storage(format!(
            "generated fixture target has no parent: {}",
            target.display()
        ))
    })?;
    require_directory(parent, "generated fixture parent")?;
    let previous = match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_dir() => Some(metadata),
        Ok(_) => {
            return Err(MediaError::compatibility(format!(
                "generated fixture target must be a directory: {}",
                target.display()
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(MediaError::storage(format!(
                "failed to inspect generated fixture target '{}': {error}",
                target.display()
            )));
        }
    };
    let cancellation = external_process::InterruptDeferral::start("fixture output promotion")
        .map_err(|error| MediaError::worker(error.to_string()))?;
    let backup = if previous.is_some() {
        let reserved = tempfile::Builder::new()
            .prefix(".vesper-source-normalizer-backup-")
            .tempdir_in(parent)
            .map_err(|error| {
                MediaError::storage(format!("failed to reserve fixture backup path: {error}"))
            })?
            .keep();
        fs::remove_dir(&reserved).map_err(|error| {
            MediaError::storage(format!("failed to prepare fixture backup path: {error}"))
        })?;
        fs::rename(target, &reserved).map_err(|error| {
            MediaError::storage(format!(
                "failed to preserve previous generated fixtures '{}': {error}",
                target.display()
            ))
        })?;
        Some(reserved)
    } else {
        None
    };
    if let Err(error) = fs::rename(staging.path(), target) {
        if let Some(backup) = backup.as_ref() {
            let _ = fs::rename(backup, target);
        }
        return Err(MediaError::storage(format!(
            "failed to publish generated fixtures '{}': {error}",
            target.display()
        )));
    }
    if cancellation.is_cancelled() {
        let _ = cancellation.finish();
        return Err(MediaError::worker(
            "fixture generation was cancelled after its output was published",
        ));
    }
    let _ = cancellation.finish();
    if let Some(backup) = backup {
        fs::remove_dir_all(&backup).map_err(|error| {
            MediaError::storage(format!(
                "generated fixtures were published, but the previous output could not be removed from '{}': {error}",
                backup.display()
            ))
        })?;
    }
    Ok(())
}

fn require_directory(path: &Path, label: &str) -> Result<(), MediaError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        MediaError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(MediaError::compatibility(format!(
            "{label} must be a directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn require_regular_file(path: &Path, label: &str) -> Result<(), MediaError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        MediaError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() || metadata.len() == 0 {
        return Err(MediaError::conformance(format!(
            "{label} must be a non-empty regular file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn map_process_error(error: external_process::ExternalProcessError) -> MediaError {
    match error.kind() {
        external_process::ExternalProcessErrorKind::Compatibility => {
            MediaError::compatibility(error.to_string())
        }
        external_process::ExternalProcessErrorKind::Worker
        | external_process::ExternalProcessErrorKind::Cancelled => {
            MediaError::worker(error.to_string())
        }
    }
}

fn output_error(error: std::io::Error) -> MediaError {
    MediaError::storage(format!(
        "failed to write fixture generation output: {error}"
    ))
}
