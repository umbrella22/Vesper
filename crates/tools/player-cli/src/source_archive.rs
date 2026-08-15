// Deferred archive helpers are used by Apple release flows; other hosts keep
// the shared module available for explicit compatibility diagnostics.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read};
use std::marker::PhantomData;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use xz2::read::XzDecoder;

use crate::external_process::{self, ExternalProcessErrorKind};

const DOWNLOAD_CONNECT_TIMEOUT_SECONDS: &str = "15";
const DOWNLOAD_TOTAL_TIMEOUT_SECONDS: u64 = 15 * 60;
const DOWNLOAD_LOW_SPEED_BYTES_PER_SECOND: &str = "1024";
const DOWNLOAD_LOW_SPEED_SECONDS: &str = "60";
const DOWNLOAD_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceArchiveErrorKind {
    Storage,
    Conformance,
    Worker,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct SourceArchiveError {
    kind: SourceArchiveErrorKind,
    message: String,
}

impl SourceArchiveError {
    fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: SourceArchiveErrorKind::Storage,
            message: message.into(),
        }
    }

    fn conformance(message: impl Into<String>) -> Self {
        Self {
            kind: SourceArchiveErrorKind::Conformance,
            message: message.into(),
        }
    }

    fn worker(message: impl Into<String>) -> Self {
        Self {
            kind: SourceArchiveErrorKind::Worker,
            message: message.into(),
        }
    }

    pub(crate) const fn kind(&self) -> SourceArchiveErrorKind {
        self.kind
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SourceArchivePolicy {
    pub(crate) maximum_archive_bytes: u64,
    pub(crate) maximum_entries: usize,
    pub(crate) maximum_expanded_bytes: u64,
    pub(crate) maximum_path_bytes: usize,
    pub(crate) maximum_path_depth: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SourceArchiveFormat {
    TarGzip,
    TarXz,
}

pub(crate) fn ensure_cached_archive(
    archive: &Path,
    urls: &[String],
    expected_sha256: Option<&str>,
    policy: SourceArchivePolicy,
    label: &str,
) -> Result<PathBuf, SourceArchiveError> {
    let curl = env::var_os("CURL").unwrap_or_else(|| OsString::from("curl"));
    ensure_cached_archive_with_mode(
        archive,
        urls,
        expected_sha256,
        policy,
        label,
        curl.as_os_str(),
        ArchiveDownloadMode::Standalone(PhantomData),
    )
}

#[cfg(unix)]
pub(crate) fn ensure_cached_archive_in_deferral(
    archive: &Path,
    urls: &[String],
    expected_sha256: Option<&str>,
    policy: SourceArchivePolicy,
    label: &str,
    cancellation: &external_process::InterruptDeferral,
) -> Result<PathBuf, SourceArchiveError> {
    let curl = env::var_os("CURL").unwrap_or_else(|| OsString::from("curl"));
    ensure_cached_archive_with_mode(
        archive,
        urls,
        expected_sha256,
        policy,
        label,
        curl.as_os_str(),
        ArchiveDownloadMode::InDeferral(cancellation),
    )
}

enum ArchiveDownloadMode<'a> {
    Standalone(PhantomData<&'a ()>),
    #[cfg(unix)]
    InDeferral(&'a external_process::InterruptDeferral),
}

#[allow(clippy::too_many_arguments)]
fn ensure_cached_archive_with_mode(
    archive: &Path,
    urls: &[String],
    expected_sha256: Option<&str>,
    policy: SourceArchivePolicy,
    label: &str,
    curl: &OsStr,
    mode: ArchiveDownloadMode<'_>,
) -> Result<PathBuf, SourceArchiveError> {
    let expected_sha256 = expected_sha256.map(normalize_sha256).transpose()?;
    match fs::symlink_metadata(archive) {
        Ok(metadata) => {
            validate_archive_metadata(archive, &metadata, policy, label)?;
            verify_sha256(
                archive,
                expected_sha256.as_deref(),
                policy.maximum_archive_bytes,
                label,
            )?;
            return Ok(archive.to_path_buf());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(SourceArchiveError::storage(format!(
                "failed to inspect {label} '{}': {error}",
                archive.display()
            )));
        }
    }
    if urls.is_empty() {
        return Err(SourceArchiveError::conformance(format!(
            "no download URL was configured for missing {label}: {}",
            archive.display()
        )));
    }
    let parent = archive.parent().ok_or_else(|| {
        SourceArchiveError::storage(format!(
            "{label} has no parent directory: {}",
            archive.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        SourceArchiveError::storage(format!(
            "failed to create {label} cache '{}': {error}",
            parent.display()
        ))
    })?;

    let mut failures = Vec::new();
    for url in urls {
        let partial = tempfile::Builder::new()
            .prefix(".vesper-source-download-")
            .tempfile_in(parent)
            .map_err(|error| {
                SourceArchiveError::storage(format!(
                    "failed to create {label} download staging: {error}"
                ))
            })?;
        let mut command = Command::new(curl);
        command
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--connect-timeout",
                DOWNLOAD_CONNECT_TIMEOUT_SECONDS,
                "--max-time",
                &DOWNLOAD_TOTAL_TIMEOUT_SECONDS.to_string(),
                "--speed-limit",
                DOWNLOAD_LOW_SPEED_BYTES_PER_SECOND,
                "--speed-time",
                DOWNLOAD_LOW_SPEED_SECONDS,
                "--max-filesize",
                &policy.maximum_archive_bytes.to_string(),
                "--output",
            ])
            .arg(partial.path())
            .arg("--url")
            .arg(url);
        let process_label = format!("{label} download");
        let captured = match mode {
            ArchiveDownloadMode::Standalone(_) => {
                external_process::run_interruptible_capture_with_timeout(
                    &mut command,
                    &process_label,
                    DOWNLOAD_OUTPUT_LIMIT,
                    DOWNLOAD_OUTPUT_LIMIT,
                    Duration::from_secs(DOWNLOAD_TOTAL_TIMEOUT_SECONDS + 5),
                )
            }
            #[cfg(unix)]
            ArchiveDownloadMode::InDeferral(cancellation) => {
                external_process::run_interruptible_capture_in_deferral(
                    &mut command,
                    &process_label,
                    DOWNLOAD_OUTPUT_LIMIT,
                    DOWNLOAD_OUTPUT_LIMIT,
                    cancellation,
                )
            }
        }
        .map_err(map_process_error)?;
        if !captured.status.success() {
            failures.push(format!(
                "{url}: {} ({})",
                String::from_utf8_lossy(&captured.stderr).trim(),
                captured.status
            ));
            continue;
        }
        let metadata = fs::symlink_metadata(partial.path()).map_err(|error| {
            SourceArchiveError::storage(format!("failed to inspect downloaded {label}: {error}"))
        })?;
        validate_archive_metadata(partial.path(), &metadata, policy, label)?;
        verify_sha256(
            partial.path(),
            expected_sha256.as_deref(),
            policy.maximum_archive_bytes,
            label,
        )?;
        match partial.persist_noclobber(archive) {
            Ok(_) => return Ok(archive.to_path_buf()),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(archive).map_err(|inspect_error| {
                    SourceArchiveError::storage(format!(
                        "failed to inspect concurrently published {label} '{}': {inspect_error}",
                        archive.display()
                    ))
                })?;
                validate_archive_metadata(archive, &metadata, policy, label)?;
                verify_sha256(
                    archive,
                    expected_sha256.as_deref(),
                    policy.maximum_archive_bytes,
                    label,
                )?;
                return Ok(archive.to_path_buf());
            }
            Err(error) => {
                return Err(SourceArchiveError::storage(format!(
                    "failed to publish {label} '{}': {}",
                    archive.display(),
                    error.error
                )));
            }
        }
    }
    let details = failures.join("; ");
    Err(SourceArchiveError::worker(format!(
        "failed to download {label} from every configured URL{}",
        if details.is_empty() {
            String::new()
        } else {
            format!(": {details}")
        }
    )))
}

pub(crate) fn fetch_bounded_text(
    url: &str,
    maximum_bytes: usize,
    label: &str,
) -> Result<String, SourceArchiveError> {
    let curl = env::var_os("CURL").unwrap_or_else(|| OsString::from("curl"));
    let mut command = Command::new(curl);
    command
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--connect-timeout",
            DOWNLOAD_CONNECT_TIMEOUT_SECONDS,
            "--max-time",
            "30",
            "--url",
        ])
        .arg(url);
    let captured = external_process::run_interruptible_capture_with_timeout(
        &mut command,
        label,
        maximum_bytes,
        DOWNLOAD_OUTPUT_LIMIT,
        Duration::from_secs(35),
    )
    .map_err(map_process_error)?;
    if !captured.status.success() {
        return Err(SourceArchiveError::worker(format!(
            "{label} exited unsuccessfully ({}): {}",
            captured.status,
            String::from_utf8_lossy(&captured.stderr).trim()
        )));
    }
    String::from_utf8(captured.stdout).map_err(|error| {
        SourceArchiveError::conformance(format!("{label} returned non-UTF-8 text: {error}"))
    })
}

pub(crate) fn sha256_file(
    path: &Path,
    maximum_bytes: u64,
    label: &str,
) -> Result<String, SourceArchiveError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        SourceArchiveError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > maximum_bytes {
        return Err(SourceArchiveError::conformance(format!(
            "{label} must be a non-empty regular file no larger than {maximum_bytes} bytes: {}",
            path.display()
        )));
    }
    let mut file = File::open(path).map_err(|error| {
        SourceArchiveError::storage(format!(
            "failed to open {label} '{}': {error}",
            path.display()
        ))
    })?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            SourceArchiveError::storage(format!(
                "failed to hash {label} '{}': {error}",
                path.display()
            ))
        })?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| SourceArchiveError::conformance(format!("{label} size overflowed")))?;
        if total > maximum_bytes {
            return Err(SourceArchiveError::conformance(format!(
                "{label} exceeds {maximum_bytes} bytes: {}",
                path.display()
            )));
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

pub(crate) fn extract_single_root(
    archive: &Path,
    destination: &Path,
    expected_root: &str,
    format: SourceArchiveFormat,
    policy: SourceArchivePolicy,
    label: &str,
) -> Result<PathBuf, SourceArchiveError> {
    let file = File::open(archive).map_err(|error| {
        SourceArchiveError::storage(format!(
            "failed to open {label} '{}': {error}",
            archive.display()
        ))
    })?;
    let reader: Box<dyn Read> = match format {
        SourceArchiveFormat::TarGzip => Box::new(GzDecoder::new(file)),
        SourceArchiveFormat::TarXz => Box::new(XzDecoder::new(file)),
    };
    extract_tar_reader(reader, destination, expected_root, policy, label)?;
    let root = destination.join(expected_root);
    let metadata = fs::symlink_metadata(&root).map_err(|error| {
        SourceArchiveError::conformance(format!(
            "{label} did not produce expected root '{}': {error}",
            root.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(SourceArchiveError::conformance(format!(
            "{label} root is not a regular directory: {}",
            root.display()
        )));
    }
    Ok(root)
}

fn extract_tar_reader(
    reader: Box<dyn Read>,
    destination: &Path,
    expected_root: &str,
    policy: SourceArchivePolicy,
    label: &str,
) -> Result<(), SourceArchiveError> {
    fs::create_dir_all(destination).map_err(|error| {
        SourceArchiveError::storage(format!(
            "failed to create {label} extraction directory '{}': {error}",
            destination.display()
        ))
    })?;
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|error| SourceArchiveError::conformance(format!("invalid {label}: {error}")))?;
    let mut count = 0_usize;
    let mut expanded = 0_u64;
    let mut paths = HashSet::new();
    let expected_root_component = Path::new(expected_root);
    if expected_root_component.components().count() != 1
        || !matches!(
            expected_root_component.components().next(),
            Some(Component::Normal(_))
        )
    {
        return Err(SourceArchiveError::conformance(format!(
            "invalid expected {label} root: {expected_root}"
        )));
    }
    for entry in entries {
        let mut entry = entry.map_err(|error| {
            SourceArchiveError::conformance(format!("invalid {label} entry: {error}"))
        })?;
        count = count.checked_add(1).ok_or_else(|| {
            SourceArchiveError::conformance(format!("{label} entry count overflowed"))
        })?;
        if count > policy.maximum_entries {
            return Err(SourceArchiveError::conformance(format!(
                "{label} exceeds {} entries",
                policy.maximum_entries
            )));
        }
        expanded = expanded.checked_add(entry.size()).ok_or_else(|| {
            SourceArchiveError::conformance(format!("{label} expanded size overflowed"))
        })?;
        if expanded > policy.maximum_expanded_bytes {
            return Err(SourceArchiveError::conformance(format!(
                "{label} expands beyond {} bytes",
                policy.maximum_expanded_bytes
            )));
        }
        let kind = entry.header().entry_type();
        if !kind.is_dir() && !kind.is_file() {
            return Err(SourceArchiveError::conformance(format!(
                "{label} contains an unsupported entry type"
            )));
        }
        let path = entry
            .path()
            .map_err(|error| {
                SourceArchiveError::conformance(format!("invalid {label} path: {error}"))
            })?
            .to_path_buf();
        let path_text = path.to_str().ok_or_else(|| {
            SourceArchiveError::conformance(format!("{label} contains a non-UTF-8 path"))
        })?;
        if path_text.len() > policy.maximum_path_bytes {
            return Err(SourceArchiveError::conformance(format!(
                "{label} path exceeds {} bytes",
                policy.maximum_path_bytes
            )));
        }
        let components = path.components().collect::<Vec<_>>();
        if components.is_empty() || components.len() > policy.maximum_path_depth {
            return Err(SourceArchiveError::conformance(format!(
                "{label} path has an invalid depth: {}",
                path.display()
            )));
        }
        if components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(SourceArchiveError::conformance(format!(
                "{label} contains an unsafe path: {}",
                path.display()
            )));
        }
        if components.first().and_then(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        }) != Some(expected_root)
        {
            return Err(SourceArchiveError::conformance(format!(
                "{label} must contain only the top-level directory '{expected_root}', found '{}'",
                path.display()
            )));
        }
        if !paths.insert(path.to_path_buf()) {
            return Err(SourceArchiveError::conformance(format!(
                "{label} contains a duplicate path: {}",
                path.display()
            )));
        }
        if !entry.unpack_in(destination).map_err(|error| {
            SourceArchiveError::conformance(format!(
                "failed to unpack {label} entry '{}': {error}",
                path.display()
            ))
        })? {
            return Err(SourceArchiveError::conformance(format!(
                "{label} entry escaped the staging directory: {}",
                path.display()
            )));
        }
    }
    if count == 0 {
        return Err(SourceArchiveError::conformance(format!("empty {label}")));
    }
    Ok(())
}

fn validate_archive_metadata(
    path: &Path,
    metadata: &fs::Metadata,
    policy: SourceArchivePolicy,
    label: &str,
) -> Result<(), SourceArchiveError> {
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > policy.maximum_archive_bytes
    {
        return Err(SourceArchiveError::conformance(format!(
            "{label} must be a non-empty regular file no larger than {} bytes: {}",
            policy.maximum_archive_bytes,
            path.display()
        )));
    }
    Ok(())
}

fn verify_sha256(
    archive: &Path,
    expected: Option<&str>,
    maximum_bytes: u64,
    label: &str,
) -> Result<(), SourceArchiveError> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let actual = sha256_file(archive, maximum_bytes, label)?;
    if actual == expected {
        Ok(())
    } else {
        Err(SourceArchiveError::conformance(format!(
            "{label} checksum mismatch for '{}': expected {expected}, actual {actual}",
            archive.display()
        )))
    }
}

fn normalize_sha256(value: &str) -> Result<String, SourceArchiveError> {
    let normalized = value.to_ascii_lowercase();
    if normalized.len() == 64 && normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(normalized)
    } else {
        Err(SourceArchiveError::conformance(
            "source archive SHA-256 must contain exactly 64 hexadecimal characters",
        ))
    }
}

fn map_process_error(error: external_process::ExternalProcessError) -> SourceArchiveError {
    match error.kind() {
        ExternalProcessErrorKind::Compatibility => {
            SourceArchiveError::conformance(error.to_string())
        }
        ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
            SourceArchiveError::worker(error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const TEST_POLICY: SourceArchivePolicy = SourceArchivePolicy {
        maximum_archive_bytes: 1024 * 1024,
        maximum_entries: 16,
        maximum_expanded_bytes: 1024 * 1024,
        maximum_path_bytes: 128,
        maximum_path_depth: 8,
    };

    #[cfg(unix)]
    #[test]
    fn cached_archive_download_reuses_an_active_interrupt_deferral() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("create download fixture");
        let curl = directory.path().join("curl");
        fs::write(
            &curl,
            concat!(
                "#!/bin/sh\n",
                "set -eu\n",
                "output=\n",
                "while [ \"$#\" -gt 0 ]; do\n",
                "  if [ \"$1\" = --output ]; then shift; output=$1; fi\n",
                "  shift\n",
                "done\n",
                "[ -n \"$output\" ]\n",
                "printf 'downloaded archive\\n' > \"$output\"\n",
            ),
        )
        .expect("write curl fixture");
        let mut permissions = fs::metadata(&curl)
            .expect("inspect curl fixture")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&curl, permissions).expect("make curl fixture executable");

        let archive = directory.path().join("cache/source.tar.xz");
        let cancellation = external_process::InterruptDeferral::start("outer source operation")
            .expect("start outer cancellation scope");
        let resolved = ensure_cached_archive_with_mode(
            &archive,
            &["https://example.invalid/source.tar.xz".to_owned()],
            None,
            TEST_POLICY,
            "fixture source archive",
            curl.as_os_str(),
            ArchiveDownloadMode::InDeferral(&cancellation),
        )
        .expect("download within the existing cancellation scope");
        assert!(!cancellation.finish());
        assert_eq!(resolved, archive);
        assert_eq!(
            fs::read(&resolved).expect("read downloaded archive"),
            b"downloaded archive\n"
        );
    }

    #[test]
    fn extracts_a_bounded_xz_source_tree() {
        let directory = tempfile::tempdir().expect("create source archive fixture");
        let archive_path = directory.path().join("source.tar.xz");
        let archive_file = File::create(&archive_path).expect("create source archive");
        let encoder = xz2::write::XzEncoder::new(archive_file, 6);
        let mut archive = tar::Builder::new(encoder);
        let bytes = b"#!/bin/sh\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, "ffmpeg-8.1.2/configure", &bytes[..])
            .expect("append source entry");
        let encoder = archive.into_inner().expect("finish tar archive");
        encoder.finish().expect("finish xz stream");

        let extraction = directory.path().join("extraction");
        let root = extract_single_root(
            &archive_path,
            &extraction,
            "ffmpeg-8.1.2",
            SourceArchiveFormat::TarXz,
            TEST_POLICY,
            "FFmpeg source archive",
        )
        .expect("extract source archive");
        assert!(root.join("configure").is_file());
    }

    #[test]
    fn rejects_an_archive_with_a_second_top_level_root() {
        let directory = tempfile::tempdir().expect("create source archive fixture");
        let archive_path = directory.path().join("source.tar.gz");
        let archive_file = File::create(&archive_path).expect("create source archive");
        let encoder = flate2::write::GzEncoder::new(archive_file, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let bytes = b"unexpected";
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, "other/file", &bytes[..])
            .expect("append source entry");
        let encoder = archive.into_inner().expect("finish tar archive");
        encoder.finish().expect("finish gzip stream");

        let error = extract_single_root(
            &archive_path,
            &directory.path().join("extraction"),
            "expected",
            SourceArchiveFormat::TarGzip,
            TEST_POLICY,
            "dependency source archive",
        )
        .expect_err("reject unexpected root");
        assert!(error.to_string().contains("top-level directory 'expected'"));
    }

    #[test]
    fn hashes_only_bounded_regular_files() {
        let directory = tempfile::tempdir().expect("create checksum fixture");
        let path = directory.path().join("source");
        let mut file = File::create(&path).expect("create checksum input");
        file.write_all(b"vesper").expect("write checksum input");
        assert_eq!(
            sha256_file(&path, 16, "source").expect("hash source"),
            "ecc9bddb65dc44516ca3eae954eb6d06cbfe4c250cae103f68d5cf346a7cf703"
        );
        assert!(sha256_file(&path, 3, "source").is_err());
    }
}
