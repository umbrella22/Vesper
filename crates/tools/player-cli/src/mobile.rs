use std::collections::{HashSet, VecDeque};
use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use zip::ZipArchive;

use crate::{android, ios_kit};

const MAX_AAR_BYTES: u64 = 512 * 1024 * 1024;
const MAX_AAR_ENTRIES: usize = 4096;
const MAX_ARCHIVE_PATH_BYTES: usize = 512;
const MAX_ARCHIVE_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ARCHIVE_EXPANDED_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_COMPRESSION_RATIO: u64 = 1000;
const COMPRESSION_RATIO_FLOOR: u64 = 1024 * 1024;
const MAX_XCFRAMEWORK_ENTRIES: usize = 20_000;
const MAX_XCFRAMEWORK_DEPTH: usize = 32;
const MAX_XCFRAMEWORK_PATH_BYTES: usize = 4096;
const MAX_XCFRAMEWORK_FILE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_XCFRAMEWORK_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobileErrorKind {
    Storage,
    Compatibility,
    Conformance,
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileError {
    kind: MobileErrorKind,
    message: String,
}

impl MobileError {
    fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: MobileErrorKind::Storage,
            message: message.into(),
        }
    }

    fn compatibility(message: impl Into<String>) -> Self {
        Self {
            kind: MobileErrorKind::Compatibility,
            message: message.into(),
        }
    }

    fn conformance(message: impl Into<String>) -> Self {
        Self {
            kind: MobileErrorKind::Conformance,
            message: message.into(),
        }
    }

    fn worker(message: impl Into<String>) -> Self {
        Self {
            kind: MobileErrorKind::Worker,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> MobileErrorKind {
        self.kind
    }
}

impl std::fmt::Display for MobileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for MobileError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoRemuxMode {
    Android,
    Ios,
    All,
}

pub fn verify_no_remux(
    root: &Path,
    mode: NoRemuxMode,
    output: &mut dyn Write,
) -> Result<(), MobileError> {
    match mode {
        NoRemuxMode::Android => verify_android(root, output),
        NoRemuxMode::Ios => verify_ios(root, output),
        NoRemuxMode::All => {
            verify_android(root, output)?;
            verify_ios(root, output)
        }
    }
}

fn verify_android(root: &Path, output: &mut dyn Write) -> Result<(), MobileError> {
    android::build_aar(root, "assembleRelease", false).map_err(|error| match error.kind() {
        android::AndroidErrorKind::Usage => MobileError::conformance(error.to_string()),
        android::AndroidErrorKind::Storage => MobileError::storage(error.to_string()),
        android::AndroidErrorKind::Compatibility => MobileError::compatibility(error.to_string()),
        android::AndroidErrorKind::Conformance => MobileError::conformance(error.to_string()),
        android::AndroidErrorKind::Worker => MobileError::worker(error.to_string()),
    })?;

    let artifacts = [
        root.join(
            "lib/android/vesper-player-kit/build/outputs/aar/vesper-player-kit-release.aar",
        ),
        root.join(
            "lib/android/vesper-player-kit-compose/build/outputs/aar/vesper-player-kit-compose-release.aar",
        ),
    ];
    for artifact in artifacts {
        let size = verify_aar(&artifact)?;
        writeln!(
            output,
            "Verified Android host artifact without FFmpeg payload: {} ({size} bytes)",
            artifact.display()
        )
        .map_err(output_error)?;
        output.flush().map_err(output_error)?;
    }
    Ok(())
}

fn verify_aar(path: &Path) -> Result<u64, MobileError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            MobileError::storage(format!(
                "Expected Android AAR was not found: {}",
                path.display()
            ))
        } else {
            MobileError::storage(format!(
                "failed to inspect Android AAR {}: {error}",
                path.display()
            ))
        }
    })?;
    if !metadata.file_type().is_file() {
        return Err(MobileError::conformance(format!(
            "Android AAR is not a regular non-symlink file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_AAR_BYTES {
        return Err(MobileError::conformance(format!(
            "Android AAR exceeds {MAX_AAR_BYTES} bytes: {}",
            path.display()
        )));
    }

    let file = File::open(path).map_err(|error| {
        MobileError::storage(format!(
            "failed to open Android AAR {}: {error}",
            path.display()
        ))
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        MobileError::conformance(format!("invalid Android AAR {}: {error}", path.display()))
    })?;
    if archive.len() > MAX_AAR_ENTRIES {
        return Err(MobileError::conformance(format!(
            "Android AAR contains more than {MAX_AAR_ENTRIES} entries: {}",
            path.display()
        )));
    }

    let mut paths = HashSet::with_capacity(archive.len());
    let mut expanded_bytes = 0_u64;
    let mut unexpected_payload = None;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            MobileError::conformance(format!(
                "failed to inspect Android AAR entry {index} in {}: {error}",
                path.display()
            ))
        })?;
        let raw_name = entry.name_raw();
        if raw_name.is_empty() || raw_name.len() > MAX_ARCHIVE_PATH_BYTES {
            return Err(MobileError::conformance(format!(
                "Android AAR entry {index} has an empty or oversized path in {}",
                path.display()
            )));
        }
        let entry_name = std::str::from_utf8(raw_name).map_err(|_| {
            MobileError::conformance(format!(
                "Android AAR entry {index} path is not UTF-8 in {}",
                path.display()
            ))
        })?;
        let enclosed = entry.enclosed_name().ok_or_else(|| {
            MobileError::conformance(format!(
                "Android AAR contains an unsafe path '{entry_name}': {}",
                path.display()
            ))
        })?;
        if !paths.insert(entry_name.to_owned()) {
            return Err(MobileError::conformance(format!(
                "Android AAR repeats path '{entry_name}': {}",
                path.display()
            )));
        }
        if entry.is_dir() {
            continue;
        }
        if !entry.is_file() || zip_entry_is_symlink(entry.unix_mode()) {
            return Err(MobileError::conformance(format!(
                "Android AAR entry '{entry_name}' is not a regular non-symlink file: {}",
                path.display()
            )));
        }
        if entry.size() > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(MobileError::conformance(format!(
                "Android AAR entry '{entry_name}' exceeds {MAX_ARCHIVE_ENTRY_BYTES} bytes: {}",
                path.display()
            )));
        }
        expanded_bytes = expanded_bytes.checked_add(entry.size()).ok_or_else(|| {
            MobileError::conformance(format!(
                "Android AAR expanded size overflowed: {}",
                path.display()
            ))
        })?;
        if expanded_bytes > MAX_ARCHIVE_EXPANDED_BYTES {
            return Err(MobileError::conformance(format!(
                "Android AAR expands beyond {MAX_ARCHIVE_EXPANDED_BYTES} bytes: {}",
                path.display()
            )));
        }
        if entry.size() > COMPRESSION_RATIO_FLOOR
            && (entry.compressed_size() == 0
                || entry
                    .compressed_size()
                    .saturating_mul(MAX_COMPRESSION_RATIO)
                    < entry.size())
        {
            return Err(MobileError::conformance(format!(
                "Android AAR entry '{entry_name}' exceeds the {MAX_COMPRESSION_RATIO}:1 compression ratio limit: {}",
                path.display()
            )));
        }
        if enclosed
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_forbidden_payload_name)
        {
            unexpected_payload.get_or_insert_with(|| entry_name.to_owned());
        }
    }
    if let Some(entry) = unexpected_payload {
        return Err(MobileError::conformance(format!(
            "Unexpected FFmpeg payload was packaged into {}:\n  {entry}",
            path.display()
        )));
    }
    Ok(metadata.len())
}

fn verify_ios(root: &Path, output: &mut dyn Write) -> Result<(), MobileError> {
    if env::consts::OS != "macos" {
        return Err(MobileError::compatibility(
            "iOS host artifact verification only runs on macOS.",
        ));
    }
    let mut diagnostics = io::stderr().lock();
    ios_kit::build(root, output, &mut diagnostics).map_err(|error| match error.kind() {
        crate::ios::IosErrorKind::Storage => MobileError::storage(error.to_string()),
        crate::ios::IosErrorKind::Compatibility => MobileError::compatibility(error.to_string()),
        crate::ios::IosErrorKind::Conformance => MobileError::conformance(error.to_string()),
        crate::ios::IosErrorKind::Worker => MobileError::worker(error.to_string()),
    })?;

    let framework =
        root.join("lib/ios/VesperPlayerKit/.build/xcframework/VesperPlayerKit.xcframework");
    let scan = scan_xcframework(&framework)?;
    if let Some(path) = scan.unexpected_payload {
        return Err(MobileError::conformance(format!(
            "Unexpected FFmpeg payload was packaged into {}:\n  {}",
            framework.display(),
            path.display()
        )));
    }
    if scan.framework_binaries.is_empty() {
        return Err(MobileError::conformance(format!(
            "No VesperPlayerKit framework binaries were found in {}",
            framework.display()
        )));
    }
    for binary in scan.framework_binaries {
        verify_ios_linkage(&binary)?;
    }
    let size_kb = directory_size_kb(&framework)?;
    writeln!(
        output,
        "Verified iOS host artifact without FFmpeg payload: {} ({size_kb} KB)",
        framework.display()
    )
    .map_err(output_error)?;
    output.flush().map_err(output_error)
}

#[derive(Debug)]
struct XcframeworkScan {
    framework_binaries: Vec<PathBuf>,
    unexpected_payload: Option<PathBuf>,
}

fn scan_xcframework(root: &Path) -> Result<XcframeworkScan, MobileError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            MobileError::storage(format!(
                "Expected iOS XCFramework was not found: {}",
                root.display()
            ))
        } else {
            MobileError::storage(format!(
                "failed to inspect iOS XCFramework {}: {error}",
                root.display()
            ))
        }
    })?;
    if !metadata.file_type().is_dir() {
        return Err(MobileError::conformance(format!(
            "iOS XCFramework is not a regular non-symlink directory: {}",
            root.display()
        )));
    }

    let mut pending = VecDeque::from([(root.to_path_buf(), 0_usize)]);
    let mut entries = 0_usize;
    let mut total_bytes = 0_u64;
    let mut framework_binaries = Vec::new();
    let mut unexpected_payload = None;
    while let Some((directory, depth)) = pending.pop_front() {
        if depth > MAX_XCFRAMEWORK_DEPTH {
            return Err(MobileError::conformance(format!(
                "iOS XCFramework exceeds directory depth {MAX_XCFRAMEWORK_DEPTH}: {}",
                root.display()
            )));
        }
        let children = fs::read_dir(&directory).map_err(|error| {
            MobileError::storage(format!(
                "failed to read iOS XCFramework directory {}: {error}",
                directory.display()
            ))
        })?;
        for child in children {
            let child = child.map_err(|error| {
                MobileError::storage(format!(
                    "failed to read an iOS XCFramework entry in {}: {error}",
                    directory.display()
                ))
            })?;
            entries = entries.checked_add(1).ok_or_else(|| {
                MobileError::conformance("iOS XCFramework entry count overflowed")
            })?;
            if entries > MAX_XCFRAMEWORK_ENTRIES {
                return Err(MobileError::conformance(format!(
                    "iOS XCFramework contains more than {MAX_XCFRAMEWORK_ENTRIES} entries: {}",
                    root.display()
                )));
            }
            let path = child.path();
            if path.to_string_lossy().len() > MAX_XCFRAMEWORK_PATH_BYTES {
                return Err(MobileError::conformance(format!(
                    "iOS XCFramework contains an oversized path: {}",
                    path.display()
                )));
            }
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                MobileError::storage(format!(
                    "failed to inspect iOS XCFramework entry {}: {error}",
                    path.display()
                ))
            })?;
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                return Err(MobileError::conformance(format!(
                    "iOS XCFramework contains a symlink: {}",
                    path.display()
                )));
            }
            if file_type.is_dir() {
                pending.push_back((path, depth + 1));
                continue;
            }
            if !file_type.is_file() {
                return Err(MobileError::conformance(format!(
                    "iOS XCFramework contains a non-regular entry: {}",
                    path.display()
                )));
            }
            if metadata.len() > MAX_XCFRAMEWORK_FILE_BYTES {
                return Err(MobileError::conformance(format!(
                    "iOS XCFramework file exceeds {MAX_XCFRAMEWORK_FILE_BYTES} bytes: {}",
                    path.display()
                )));
            }
            total_bytes = total_bytes
                .checked_add(metadata.len())
                .ok_or_else(|| MobileError::conformance("iOS XCFramework size sum overflowed"))?;
            if total_bytes > MAX_XCFRAMEWORK_TOTAL_BYTES {
                return Err(MobileError::conformance(format!(
                    "iOS XCFramework exceeds {MAX_XCFRAMEWORK_TOTAL_BYTES} total file bytes: {}",
                    root.display()
                )));
            }
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_forbidden_payload_name)
                && unexpected_payload
                    .as_ref()
                    .is_none_or(|current| path < *current)
            {
                unexpected_payload = Some(path.clone());
            }
            if is_vesper_framework_binary(&path) {
                framework_binaries.push(path);
            }
        }
    }
    framework_binaries.sort();
    Ok(XcframeworkScan {
        framework_binaries,
        unexpected_payload,
    })
}

fn is_vesper_framework_binary(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == "VesperPlayerKit")
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "VesperPlayerKit.framework")
}

fn verify_ios_linkage(binary: &Path) -> Result<(), MobileError> {
    let mut command = Command::new("otool");
    command.arg("-L").arg(binary);
    let output = run_bounded_output(
        &mut command,
        "otool linkage inspection",
        MAX_TOOL_OUTPUT_BYTES,
    )?;
    let text = std::str::from_utf8(&output).map_err(|_| {
        MobileError::conformance(format!(
            "otool output was not UTF-8 for framework binary: {}",
            binary.display()
        ))
    })?;
    if contains_forbidden_linkage(text) {
        return Err(MobileError::conformance(format!(
            "Unexpected FFmpeg linkage found in framework binary: {}\n{text}",
            binary.display()
        )));
    }
    Ok(())
}

fn directory_size_kb(path: &Path) -> Result<u64, MobileError> {
    let mut command = Command::new("du");
    command.arg("-sk").arg(path);
    let output = run_bounded_output(&mut command, "XCFramework size inspection", 4096)?;
    std::str::from_utf8(&output)
        .ok()
        .and_then(|text| text.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| {
            MobileError::conformance(format!(
                "Could not read XCFramework size for {}",
                path.display()
            ))
        })
}

fn contains_forbidden_linkage(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "player_remux_ffmpeg",
        "vesper_remux_ffmpeg",
        "libavcodec",
        "libavformat",
        "libavutil",
        "libavfilter",
        "libavdevice",
        "libswresample",
        "libswscale",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn is_forbidden_payload_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if !is_dynamic_library_name(&lower) {
        return false;
    }
    (lower.starts_with("lib") && lower.contains("remux_ffmpeg"))
        || lower.starts_with("libvesper_player_relay_ffmpeg")
        || [
            "libavcodec",
            "libavformat",
            "libavutil",
            "libavfilter",
            "libavdevice",
            "libswresample",
            "libswscale",
            "libssl",
            "libcrypto",
            "libxml2",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn is_dynamic_library_name(name: &str) -> bool {
    name.ends_with(".dylib") || name.ends_with(".so") || name.contains(".so.")
}

fn zip_entry_is_symlink(unix_mode: Option<u32>) -> bool {
    unix_mode.is_some_and(|mode| mode & 0o170000 == 0o120000)
}

fn run_bounded_output(
    command: &mut Command,
    label: &str,
    limit: usize,
) -> Result<Vec<u8>, MobileError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            MobileError::storage(format!(
                "Required command is unavailable for {label}: {error}"
            ))
        } else {
            MobileError::worker(format!("failed to run {label}: {error}"))
        }
    })?;
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(MobileError::worker(format!(
            "failed to capture {label} output"
        )));
    };
    let mut output = Vec::with_capacity(limit.min(4096));
    let mut buffer = [0_u8; 4096];
    let mut exceeded = false;
    loop {
        let count = stdout.read(&mut buffer).map_err(|error| {
            let _ = child.kill();
            let _ = child.wait();
            MobileError::worker(format!("failed to read {label} output: {error}"))
        })?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.len());
        let retained = remaining.min(count);
        output.extend_from_slice(&buffer[..retained]);
        exceeded |= retained < count;
    }
    let status = child
        .wait()
        .map_err(|error| MobileError::worker(format!("failed to reap {label}: {error}")))?;
    if !status.success() {
        return Err(MobileError::conformance(format!(
            "{label} exited unsuccessfully ({status})"
        )));
    }
    if exceeded {
        return Err(MobileError::conformance(format!(
            "{label} output exceeded {limit} bytes"
        )));
    }
    Ok(output)
}

fn output_error(error: io::Error) -> MobileError {
    MobileError::worker(format!(
        "failed to write mobile verification output: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_payload_names_cover_versioned_shared_libraries() {
        for name in [
            "libvesper_remux_ffmpeg.so",
            "libvesper_player_relay_ffmpeg.so",
            "libavcodec.so.61",
            "libavformat.61.dylib",
            "libcrypto.3.dylib",
        ] {
            assert!(is_forbidden_payload_name(name), "{name}");
        }
        assert!(!is_forbidden_payload_name("libvesper_player_android.so"));
        assert!(!is_forbidden_payload_name("FFmpegNotice.txt"));
    }

    #[test]
    fn linkage_scan_only_matches_binary_dependencies() {
        assert!(contains_forbidden_linkage(
            "@rpath/libavcodec.61.dylib (compatibility version 61.0.0)"
        ));
        assert!(!contains_forbidden_linkage(
            "@rpath/VesperPlayerKit.framework/VesperPlayerKit"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn xcframework_scan_rejects_symlinks_without_following_them() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temporary XCFramework scan");
        let root = directory.path().join("VesperPlayerKit.xcframework");
        let framework = root.join("ios-arm64/VesperPlayerKit.framework");
        fs::create_dir_all(&framework).expect("create framework directory");
        fs::write(framework.join("VesperPlayerKit"), b"framework").expect("write framework binary");
        symlink(
            framework.join("VesperPlayerKit"),
            framework.join("libavcodec.dylib"),
        )
        .expect("create framework symlink");

        let error = scan_xcframework(&root).expect_err("reject framework symlink");
        assert_eq!(error.kind(), MobileErrorKind::Conformance);
        assert!(error.to_string().contains("contains a symlink"));
    }
}
