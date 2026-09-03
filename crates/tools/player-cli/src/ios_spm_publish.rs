use std::collections::{BTreeMap, VecDeque};
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use tempfile::NamedTempFile;
use url::Url;

use crate::external_process::{self, BoundedProcessOutput, ExternalProcessErrorKind};
use crate::ios::IosError;
use crate::release;

const DEFAULT_SPM_REPOSITORY: &str = "umbrella22/VesperPlayerKit";
const MAX_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_UI_SOURCE_ENTRIES: usize = 256;
const MAX_UI_SOURCE_DEPTH: usize = 16;
const MAX_UI_SOURCE_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_UI_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PROCESS_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const SWIFT_TIMEOUT: Duration = Duration::from_secs(120);
const GIT_TIMEOUT: Duration = Duration::from_secs(300);
const MANAGED_FILES: [&str; 3] = ["Package.swift", "README.md", "LICENSE"];
const UI_SOURCE_PATH: &str = "Sources/VesperPlayerKitUI";
const OPTIONAL_BINARY_TARGETS: [&str; 6] = [
    "VesperFFmpegAVCodec",
    "VesperFFmpegAVFormat",
    "VesperFFmpegAVUtil",
    "VesperPlayerRemuxFfmpegPlugin",
    "VesperPlayerSourceNormalizerFfmpegPlugin",
    "VesperPlayerPerformanceDiagnosticsPlugin",
];

struct PublishedBinaryArtifact {
    target: &'static str,
    url: String,
    checksum: String,
}

pub(crate) struct SpmPublishRequest<'a> {
    pub(crate) tag: &'a str,
    pub(crate) archive: &'a Path,
    pub(crate) source_repository: Option<&'a str>,
    pub(crate) repository: Option<&'a str>,
    pub(crate) dry_run: bool,
    pub(crate) output_directory: Option<&'a Path>,
}

struct GitAuth {
    config: NamedTempFile,
}

impl GitAuth {
    fn from_environment() -> Result<Self, IosError> {
        let token = env::var("SPM_PUBLISH_TOKEN")
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                IosError::compatibility(
                    "SPM_PUBLISH_TOKEN is required for remote Swift package publication",
                )
            })?;
        let authorization = BASE64.encode(format!("x-access-token:{token}"));
        let mut config = tempfile::Builder::new()
            .prefix("vesper-spm-git-auth.")
            .tempfile()
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to create temporary Swift package Git auth configuration: {error}"
                ))
            })?;
        writeln!(
            config,
            "[http \"https://github.com/\"]\n\textraHeader = Authorization: Basic {authorization}"
        )
        .and_then(|_| config.flush())
        .map_err(|error| {
            IosError::storage(format!(
                "failed to write temporary Swift package Git auth configuration: {error}"
            ))
        })?;
        Ok(Self { config })
    }

    fn apply(&self, command: &mut Command) {
        command
            .env("GIT_CONFIG_GLOBAL", self.config.path())
            .env("GIT_CONFIG_NOSYSTEM", "1");
    }
}

pub(crate) fn publish(
    root: &Path,
    request: SpmPublishRequest<'_>,
    output: &mut dyn Write,
) -> Result<(), IosError> {
    let version = release::ReleaseContext::publication_version_from_tag(request.tag)
        .map_err(|error| IosError::conformance(error.to_string()))?;
    let source_repository = resolve_repository(
        request.source_repository,
        "GITHUB_REPOSITORY",
        None,
        "source GitHub repository",
    )?;
    let package_repository = resolve_repository(
        request.repository,
        "SPM_REPOSITORY",
        Some(DEFAULT_SPM_REPOSITORY),
        "Swift package GitHub repository",
    )?;
    let core_archive = resolve_input(root, request.archive);
    validate_archive(&core_archive, "VesperPlayerKit")?;
    let archive_directory = core_archive.parent().ok_or_else(|| {
        IosError::conformance(format!(
            "released VesperPlayerKit archive has no parent directory: {}",
            core_archive.display()
        ))
    })?;
    let mut artifacts = Vec::with_capacity(OPTIONAL_BINARY_TARGETS.len() + 1);
    for target in std::iter::once("VesperPlayerKit").chain(OPTIONAL_BINARY_TARGETS) {
        let archive = if target == "VesperPlayerKit" {
            core_archive.clone()
        } else {
            archive_directory.join(format!("{target}.xcframework.zip"))
        };
        validate_archive(&archive, target)?;
        artifacts.push(PublishedBinaryArtifact {
            target,
            url: release_asset_url(&source_repository, request.tag, &archive)?,
            checksum: compute_checksum(root, &archive)?,
        });
    }

    let candidate_workspace = tempfile::Builder::new()
        .prefix("vesper-spm-candidate.")
        .tempdir()
        .map_err(|error| {
            IosError::storage(format!(
                "failed to create Swift package candidate workspace: {error}"
            ))
        })?;
    let candidate = candidate_workspace.path().join("package");
    generate_candidate(root, &candidate, &artifacts, &source_repository)?;
    validate_manifest(&candidate)?;

    if request.dry_run {
        let destination = request
            .output_directory
            .map(|path| resolve_output(root, path))
            .transpose()?
            .unwrap_or_else(|| root.join("dist/release/spm-index"));
        promote_candidate(&candidate, &destination)?;
        writeln!(
            output,
            "Verified Swift package dry-run for {version}:\n  {}\n  binary targets: {}",
            destination.display(),
            artifacts.len()
        )
        .map_err(output_error)?;
        return output.flush().map_err(output_error);
    }

    let auth = GitAuth::from_environment()?;
    publish_remote(&candidate, &package_repository, request.tag, &auth, output)?;
    writeln!(
        output,
        "Published Swift package {package_repository}@{} with {} binary targets.",
        request.tag,
        artifacts.len()
    )
    .map_err(output_error)?;
    output.flush().map_err(output_error)
}

fn resolve_repository(
    argument: Option<&str>,
    environment: &str,
    default: Option<&str>,
    label: &str,
) -> Result<String, IosError> {
    let value = argument
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| env::var(environment).ok().filter(|value| !value.is_empty()))
        .or_else(|| default.map(str::to_owned))
        .ok_or_else(|| {
            IosError::compatibility(format!(
                "{label} is required through --{} or {environment}",
                environment.to_ascii_lowercase()
            ))
        })?;
    validate_repository_slug(&value, label)?;
    Ok(value)
}

fn validate_repository_slug(value: &str, label: &str) -> Result<(), IosError> {
    let components = value.split('/').collect::<Vec<_>>();
    if components.len() != 2
        || components.iter().any(|component| {
            component.is_empty()
                || component.len() > 100
                || matches!(*component, "." | "..")
                || !component
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    {
        return Err(IosError::conformance(format!(
            "{label} must be an owner/repository GitHub slug: {value}"
        )));
    }
    Ok(())
}

fn resolve_input(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn resolve_output(root: &Path, path: &Path) -> Result<PathBuf, IosError> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(IosError::conformance(format!(
            "Swift package dry-run output must not contain '.' or '..': {}",
            path.display()
        )));
    }
    let output = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let dist = root.join("dist");
    if output == dist || !output.starts_with(&dist) {
        return Err(IosError::conformance(format!(
            "Swift package dry-run output must be a child directory under repository dist/: {}",
            output.display()
        )));
    }
    Ok(output)
}

fn validate_archive(path: &Path, target: &str) -> Result<(), IosError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::conformance(format!(
            "released {target} XCFramework archive '{}' is unavailable: {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > MAX_ARCHIVE_BYTES
    {
        return Err(IosError::conformance(format!(
            "released {target} XCFramework archive '{}' must be a non-empty regular file no larger than {MAX_ARCHIVE_BYTES} bytes",
            path.display()
        )));
    }
    let expected_name = format!("{target}.xcframework.zip");
    if path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
        return Err(IosError::conformance(format!(
            "Swift package publication requires {expected_name}, got '{}'",
            path.display()
        )));
    }
    Ok(())
}

fn compute_checksum(root: &Path, archive: &Path) -> Result<String, IosError> {
    let mut command = Command::new("swift");
    command
        .current_dir(root)
        .args(["package", "compute-checksum"])
        .arg(archive);
    let result = run_capture(&mut command, "Swift package checksum", SWIFT_TIMEOUT, false)?;
    let checksum = String::from_utf8(result.stdout).map_err(|error| {
        IosError::worker(format!(
            "Swift package checksum is not valid UTF-8: {error}"
        ))
    })?;
    let checksum = checksum.trim();
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(IosError::worker(format!(
            "swift package compute-checksum returned an invalid checksum: {checksum}"
        )));
    }
    Ok(checksum.to_ascii_lowercase())
}

fn release_asset_url(repository: &str, tag: &str, archive: &Path) -> Result<String, IosError> {
    let (owner, name) = repository.split_once('/').ok_or_else(|| {
        IosError::worker(format!(
            "validated GitHub repository lost its separator: {repository}"
        ))
    })?;
    let asset = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            IosError::conformance(format!(
                "XCFramework archive name is not valid UTF-8: {}",
                archive.display()
            ))
        })?;
    let mut url = Url::parse("https://github.com/")
        .map_err(|error| IosError::worker(format!("invalid GitHub base URL: {error}")))?;
    url.path_segments_mut()
        .map_err(|_| IosError::worker("GitHub base URL cannot accept path segments"))?
        .extend([owner, name, "releases", "download", tag, asset]);
    Ok(url.into())
}

fn generate_candidate(
    root: &Path,
    destination: &Path,
    artifacts: &[PublishedBinaryArtifact],
    source_repository: &str,
) -> Result<(), IosError> {
    fs::create_dir_all(destination).map_err(|error| {
        IosError::storage(format!(
            "failed to create Swift package candidate '{}': {error}",
            destination.display()
        ))
    })?;
    let binary_targets = artifacts
        .iter()
        .map(|artifact| {
            format!(
                "        .binaryTarget(\n            name: \"{}\",\n            url: \"{}\",\n            checksum: \"{}\"\n        )",
                artifact.target, artifact.url, artifact.checksum
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let package = format!(
        r#"// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "VesperPlayerKit",
    platforms: [
        .iOS(.v17),
    ],
    products: [
        .library(name: "VesperPlayerKit", targets: ["VesperPlayerKit"]),
        .library(name: "VesperPlayerKitUI", targets: ["VesperPlayerKitUI"]),
        .library(
            name: "VesperPlayerSourceNormalizerFfmpeg",
            targets: [
                "VesperPlayerSourceNormalizerFfmpegPlugin",
                "VesperFFmpegAVCodec",
                "VesperFFmpegAVFormat",
                "VesperFFmpegAVUtil",
            ]
        ),
        .library(
            name: "VesperPlayerRemuxFfmpeg",
            targets: [
                "VesperPlayerRemuxFfmpegPlugin",
                "VesperFFmpegAVCodec",
                "VesperFFmpegAVFormat",
                "VesperFFmpegAVUtil",
            ]
        ),
        .library(
            name: "VesperPlayerPerformanceDiagnostics",
            targets: ["VesperPlayerPerformanceDiagnosticsPlugin"]
        ),
    ],
    targets: [
{binary_targets},
        .target(
            name: "VesperPlayerKitUI",
            dependencies: ["VesperPlayerKit"],
            path: "{UI_SOURCE_PATH}"
        ),
    ]
)
"#
    );
    write_file(&destination.join("Package.swift"), package.as_bytes())?;
    let readme = format!(
        "# VesperPlayerKit\n\nBinary Swift package distribution for [Vesper](https://github.com/{source_repository}).\n\nThe `VesperPlayerKit` product contains the released binary host kit. The `VesperPlayerKitUI` product layers the version-matched SwiftUI controls on top. `VesperPlayerSourceNormalizerFfmpeg` and `VesperPlayerRemuxFfmpeg` are opt-in capability products; each embeds its plugin plus the matching AVCodec, AVFormat, and AVUtil runtime components. `VesperPlayerPerformanceDiagnostics` is the opt-in BenchmarkSink diagnostics product. Decoder and FrameProcessor plugins are not part of these products.\n"
    );
    write_file(&destination.join("README.md"), readme.as_bytes())?;
    copy_regular_file(
        &root.join("LICENSE"),
        &destination.join("LICENSE"),
        128 * 1024,
    )?;
    copy_ui_sources(
        &root.join("lib/ios/VesperPlayerKit/Sources/VesperPlayerKitUI"),
        &destination.join(UI_SOURCE_PATH),
    )?;
    Ok(())
}

fn copy_ui_sources(source: &Path, destination: &Path) -> Result<(), IosError> {
    let source = source.canonicalize().map_err(|error| {
        IosError::storage(format!(
            "failed to resolve VesperPlayerKitUI sources '{}': {error}",
            source.display()
        ))
    })?;
    let mut pending = VecDeque::from([(source.clone(), PathBuf::new())]);
    let mut entries = 0_usize;
    let mut total_bytes = 0_u64;
    let mut swift_files = 0_usize;
    while let Some((directory, relative_directory)) = pending.pop_front() {
        let mut children = fs::read_dir(&directory)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to enumerate VesperPlayerKitUI source directory '{}': {error}",
                    directory.display()
                ))
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to read VesperPlayerKitUI source entry: {error}"
                ))
            })?;
        children.sort_by_key(fs::DirEntry::file_name);
        for child in children {
            entries += 1;
            if entries > MAX_UI_SOURCE_ENTRIES {
                return Err(IosError::conformance(format!(
                    "VesperPlayerKitUI source tree exceeds {MAX_UI_SOURCE_ENTRIES} entries"
                )));
            }
            let name = child.file_name();
            let name = name.to_str().ok_or_else(|| {
                IosError::conformance(format!(
                    "VesperPlayerKitUI source path is not valid UTF-8: {}",
                    child.path().display()
                ))
            })?;
            if name.starts_with('.') {
                continue;
            }
            let relative = relative_directory.join(name);
            validate_relative_path(&relative, MAX_UI_SOURCE_DEPTH, "VesperPlayerKitUI source")?;
            let metadata = fs::symlink_metadata(child.path()).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect VesperPlayerKitUI source '{}': {error}",
                    child.path().display()
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(IosError::conformance(format!(
                    "VesperPlayerKitUI source tree contains a symbolic link: {}",
                    relative.display()
                )));
            }
            if metadata.is_dir() {
                pending.push_back((child.path(), relative));
            } else if metadata.is_file() {
                if metadata.len() == 0 || metadata.len() > MAX_UI_SOURCE_FILE_BYTES {
                    return Err(IosError::conformance(format!(
                        "VesperPlayerKitUI source '{}' is empty or exceeds {MAX_UI_SOURCE_FILE_BYTES} bytes",
                        relative.display()
                    )));
                }
                total_bytes = total_bytes.checked_add(metadata.len()).ok_or_else(|| {
                    IosError::conformance("VesperPlayerKitUI source size overflow")
                })?;
                if total_bytes > MAX_UI_SOURCE_BYTES {
                    return Err(IosError::conformance(format!(
                        "VesperPlayerKitUI sources exceed {MAX_UI_SOURCE_BYTES} bytes"
                    )));
                }
                if child.path().extension().and_then(|value| value.to_str()) == Some("swift") {
                    swift_files += 1;
                }
                copy_regular_file(
                    &child.path(),
                    &destination.join(&relative),
                    MAX_UI_SOURCE_FILE_BYTES,
                )?;
            } else {
                return Err(IosError::conformance(format!(
                    "VesperPlayerKitUI source tree contains a special file: {}",
                    relative.display()
                )));
            }
        }
    }
    if swift_files == 0 {
        return Err(IosError::conformance(
            "VesperPlayerKitUI source tree contains no Swift files",
        ));
    }
    Ok(())
}

fn validate_relative_path(path: &Path, maximum_depth: usize, label: &str) -> Result<(), IosError> {
    if path.components().count() > maximum_depth
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(IosError::conformance(format!(
            "{label} contains an invalid or over-depth path: {}",
            path.display()
        )));
    }
    Ok(())
}

fn copy_regular_file(source: &Path, destination: &Path, maximum: u64) -> Result<(), IosError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        IosError::storage(format!("failed to inspect '{}': {error}", source.display()))
    })?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(IosError::conformance(format!(
            "Swift package input '{}' must be a non-empty regular file no larger than {maximum} bytes",
            source.display()
        )));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            IosError::storage(format!(
                "failed to create Swift package directory '{}': {error}",
                parent.display()
            ))
        })?;
    }
    fs::copy(source, destination).map_err(|error| {
        IosError::storage(format!(
            "failed to copy Swift package file '{}' to '{}': {error}",
            source.display(),
            destination.display()
        ))
    })?;
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), IosError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            IosError::storage(format!(
                "failed to create Swift package directory '{}': {error}",
                parent.display()
            ))
        })?;
    }
    fs::write(path, bytes).map_err(|error| {
        IosError::storage(format!(
            "failed to write Swift package file '{}': {error}",
            path.display()
        ))
    })
}

fn validate_manifest(package: &Path) -> Result<(), IosError> {
    let mut command = Command::new("swift");
    command
        .args(["package", "dump-package", "--package-path"])
        .arg(package);
    run_capture(
        &mut command,
        "Swift package manifest validation",
        SWIFT_TIMEOUT,
        false,
    )?;
    Ok(())
}

fn promote_candidate(candidate: &Path, destination: &Path) -> Result<(), IosError> {
    if candidate == destination {
        return Err(IosError::conformance(
            "Swift package candidate and output directory must differ",
        ));
    }
    let parent = destination.parent().ok_or_else(|| {
        IosError::storage(format!(
            "Swift package output has no parent directory: {}",
            destination.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        IosError::storage(format!(
            "failed to create Swift package output parent '{}': {error}",
            parent.display()
        ))
    })?;
    let stage = tempfile::Builder::new()
        .prefix(".vesper-spm-output-")
        .tempdir_in(parent)
        .map_err(|error| {
            IosError::storage(format!(
                "failed to create Swift package output stage: {error}"
            ))
        })?;
    let staged_package = stage.path().join("package");
    copy_managed_tree(candidate, &staged_package)?;
    if let Ok(metadata) = fs::symlink_metadata(destination) {
        if !metadata.file_type().is_dir() {
            return Err(IosError::conformance(format!(
                "Swift package output exists and is not a regular directory: {}",
                destination.display()
            )));
        }
        fs::remove_dir_all(destination).map_err(|error| {
            IosError::storage(format!(
                "failed to replace Swift package output '{}': {error}",
                destination.display()
            ))
        })?;
    }
    fs::rename(&staged_package, destination).map_err(|error| {
        IosError::storage(format!(
            "failed to commit Swift package output '{}': {error}",
            destination.display()
        ))
    })
}

fn publish_remote(
    candidate: &Path,
    repository: &str,
    tag: &str,
    auth: &GitAuth,
    output: &mut dyn Write,
) -> Result<(), IosError> {
    let workspace = tempfile::Builder::new()
        .prefix("vesper-spm-publish.")
        .tempdir()
        .map_err(|error| {
            IosError::storage(format!(
                "failed to create Swift package Git workspace: {error}"
            ))
        })?;
    let checkout = workspace.path().join("index");
    let remote = format!("https://github.com/{repository}.git");
    let mut clone = Command::new("git");
    clone.args(["clone", "--no-local", &remote]).arg(&checkout);
    auth.apply(&mut clone);
    run_capture(
        &mut clone,
        "Swift package repository clone",
        GIT_TIMEOUT,
        false,
    )?;

    let tag_ref = format!("refs/tags/{tag}");
    let mut verify_tag = git_command(&checkout);
    verify_tag.args([
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{tag_ref}^{{commit}}"),
    ]);
    let tag_status = run_capture(
        &mut verify_tag,
        "Swift package tag lookup",
        GIT_TIMEOUT,
        true,
    )?;
    if tag_status.status.success() {
        let mut checkout_tag = git_command(&checkout);
        checkout_tag.args(["checkout", "--detach", &tag_ref]);
        run_capture(
            &mut checkout_tag,
            "Swift package existing tag checkout",
            GIT_TIMEOUT,
            false,
        )?;
        compare_managed_trees(candidate, &checkout)?;
        writeln!(
            output,
            "Swift package tag {repository}@{tag} already matches; skipping push."
        )
        .map_err(output_error)?;
        return Ok(());
    }
    require_expected_probe_failure(&tag_status, "Swift package tag lookup")?;

    let branch = current_branch(&checkout)?;
    replace_managed_tree(candidate, &checkout)?;
    for (key, value) in [
        ("user.name", "github-actions[bot]"),
        (
            "user.email",
            "41898282+github-actions[bot]@users.noreply.github.com",
        ),
    ] {
        let mut config = git_command(&checkout);
        config.args(["config", key, value]);
        run_capture(
            &mut config,
            "Swift package Git identity",
            GIT_TIMEOUT,
            false,
        )?;
    }
    let mut add = git_command(&checkout);
    add.arg("add").arg("--");
    add.args(MANAGED_FILES).arg(UI_SOURCE_PATH);
    run_capture(&mut add, "Swift package Git staging", GIT_TIMEOUT, false)?;

    let mut diff = git_command(&checkout);
    diff.args(["diff", "--cached", "--quiet"]);
    let diff_status = run_capture(
        &mut diff,
        "Swift package staged change check",
        GIT_TIMEOUT,
        true,
    )?;
    if !diff_status.status.success() {
        require_expected_probe_failure(&diff_status, "Swift package staged change check")?;
        let mut commit = git_command(&checkout);
        commit.args(["commit", "-m", &format!("release: VesperPlayerKit {tag}")]);
        run_capture(&mut commit, "Swift package commit", GIT_TIMEOUT, false)?;
    }
    let mut create_tag = git_command(&checkout);
    create_tag.args(["tag", "-a", tag, "-m", &format!("VesperPlayerKit {tag}")]);
    run_capture(
        &mut create_tag,
        "Swift package tag creation",
        GIT_TIMEOUT,
        false,
    )?;

    let mut push = git_command(&checkout);
    push.args([
        "push",
        "--atomic",
        "origin",
        &format!("HEAD:refs/heads/{branch}"),
        &tag_ref,
    ]);
    auth.apply(&mut push);
    run_capture(&mut push, "Swift package atomic push", GIT_TIMEOUT, false)?;
    Ok(())
}

fn current_branch(checkout: &Path) -> Result<String, IosError> {
    let mut command = git_command(checkout);
    command.args(["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let result = run_capture(
        &mut command,
        "Swift package branch lookup",
        GIT_TIMEOUT,
        true,
    )?;
    let branch = if result.status.success() {
        String::from_utf8(result.stdout)
            .map_err(|error| {
                IosError::worker(format!("Swift package branch is not valid UTF-8: {error}"))
            })?
            .trim()
            .to_owned()
    } else {
        require_expected_probe_failure(&result, "Swift package branch lookup")?;
        "main".to_owned()
    };
    validate_branch_name(&branch)?;
    if !result.status.success() {
        let mut checkout_branch = git_command(checkout);
        checkout_branch.args(["checkout", "-B", &branch]);
        run_capture(
            &mut checkout_branch,
            "Swift package initial branch",
            GIT_TIMEOUT,
            false,
        )?;
    }
    Ok(branch)
}

fn validate_branch_name(branch: &str) -> Result<(), IosError> {
    if branch.is_empty()
        || branch.len() > 255
        || branch.starts_with('-')
        || branch.contains("..")
        || branch.contains("@{")
        || branch.ends_with('.')
        || branch.ends_with('/')
        || branch.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte == b' '
                || matches!(byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\')
        })
    {
        return Err(IosError::conformance(format!(
            "Swift package repository has an invalid default branch name: {branch}"
        )));
    }
    Ok(())
}

fn git_command(checkout: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(checkout);
    command
}

fn replace_managed_tree(candidate: &Path, checkout: &Path) -> Result<(), IosError> {
    for relative in MANAGED_FILES {
        let destination = checkout.join(relative);
        if let Ok(metadata) = fs::symlink_metadata(&destination) {
            if !metadata.file_type().is_file() {
                return Err(IosError::conformance(format!(
                    "managed Swift package path is not a regular file: {}",
                    destination.display()
                )));
            }
            fs::remove_file(&destination).map_err(|error| {
                IosError::storage(format!(
                    "failed to replace Swift package file '{}': {error}",
                    destination.display()
                ))
            })?;
        }
    }
    let ui_destination = checkout.join(UI_SOURCE_PATH);
    if let Ok(metadata) = fs::symlink_metadata(&ui_destination) {
        if !metadata.file_type().is_dir() {
            return Err(IosError::conformance(format!(
                "managed Swift package UI path is not a regular directory: {}",
                ui_destination.display()
            )));
        }
        fs::remove_dir_all(&ui_destination).map_err(|error| {
            IosError::storage(format!(
                "failed to replace Swift package UI sources '{}': {error}",
                ui_destination.display()
            ))
        })?;
    }
    copy_managed_tree(candidate, checkout)
}

fn copy_managed_tree(source: &Path, destination: &Path) -> Result<(), IosError> {
    for relative in MANAGED_FILES {
        copy_regular_file(
            &source.join(relative),
            &destination.join(relative),
            MAX_UI_SOURCE_FILE_BYTES,
        )?;
    }
    let files = collect_managed_files(source)?;
    for (relative, bytes) in files {
        if MANAGED_FILES
            .iter()
            .any(|managed| Path::new(managed) == relative)
        {
            continue;
        }
        write_file(&destination.join(relative), &bytes)?;
    }
    Ok(())
}

fn compare_managed_trees(expected: &Path, actual: &Path) -> Result<(), IosError> {
    let expected = collect_managed_files(expected)?;
    let actual = collect_managed_files(actual)?;
    if expected == actual {
        return Ok(());
    }
    let expected_names = expected.keys().collect::<Vec<_>>();
    let actual_names = actual.keys().collect::<Vec<_>>();
    let changed = expected
        .iter()
        .filter(|&(path, bytes)| actual.get(path).is_some_and(|actual| actual != bytes))
        .map(|(path, _)| path.display().to_string())
        .collect::<Vec<_>>();
    Err(IosError::conformance(format!(
        "existing Swift package tag differs from the requested release\n  expected files: {:?}\n  actual files: {:?}\n  changed files: {:?}",
        expected_names, actual_names, changed
    )))
}

fn collect_managed_files(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, IosError> {
    let mut files = BTreeMap::new();
    for relative in MANAGED_FILES {
        let path = root.join(relative);
        files.insert(PathBuf::from(relative), read_bounded_file(&path)?);
    }
    let ui_root = root.join(UI_SOURCE_PATH);
    let metadata = fs::symlink_metadata(&ui_root).map_err(|error| {
        IosError::conformance(format!(
            "Swift package UI source directory '{}' is unavailable: {error}",
            ui_root.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(IosError::conformance(format!(
            "Swift package UI source path is not a regular directory: {}",
            ui_root.display()
        )));
    }
    let mut pending = VecDeque::from([(ui_root, PathBuf::from(UI_SOURCE_PATH))]);
    while let Some((directory, relative_directory)) = pending.pop_front() {
        let mut children = fs::read_dir(&directory)
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to enumerate managed Swift package directory '{}': {error}",
                    directory.display()
                ))
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                IosError::storage(format!(
                    "failed to read managed Swift package entry: {error}"
                ))
            })?;
        children.sort_by_key(fs::DirEntry::file_name);
        for child in children {
            let name = child.file_name();
            let name = name.to_str().ok_or_else(|| {
                IosError::conformance(format!(
                    "managed Swift package path is not valid UTF-8: {}",
                    child.path().display()
                ))
            })?;
            let relative = relative_directory.join(name);
            validate_relative_path(&relative, MAX_UI_SOURCE_DEPTH + 2, "managed Swift package")?;
            let metadata = fs::symlink_metadata(child.path()).map_err(|error| {
                IosError::storage(format!(
                    "failed to inspect managed Swift package path '{}': {error}",
                    child.path().display()
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(IosError::conformance(format!(
                    "managed Swift package contains a symbolic link: {}",
                    relative.display()
                )));
            }
            if metadata.is_dir() {
                pending.push_back((child.path(), relative));
            } else if metadata.is_file() {
                files.insert(relative, read_bounded_file(&child.path())?);
                if files.len() > MAX_UI_SOURCE_ENTRIES + MANAGED_FILES.len() {
                    return Err(IosError::conformance(format!(
                        "managed Swift package exceeds {} files",
                        MAX_UI_SOURCE_ENTRIES + MANAGED_FILES.len()
                    )));
                }
            } else {
                return Err(IosError::conformance(format!(
                    "managed Swift package contains a special file: {}",
                    relative.display()
                )));
            }
        }
    }
    Ok(files)
}

fn read_bounded_file(path: &Path) -> Result<Vec<u8>, IosError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        IosError::conformance(format!(
            "managed Swift package file '{}' is unavailable: {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_UI_SOURCE_FILE_BYTES
    {
        return Err(IosError::conformance(format!(
            "managed Swift package file '{}' is empty, non-regular, or exceeds {MAX_UI_SOURCE_FILE_BYTES} bytes",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .and_then(|mut input| input.read_to_end(&mut bytes))
        .map_err(|error| {
            IosError::storage(format!(
                "failed to read managed Swift package file '{}': {error}",
                path.display()
            ))
        })?;
    Ok(bytes)
}

fn run_capture(
    command: &mut Command,
    label: &str,
    timeout: Duration,
    allow_failure: bool,
) -> Result<BoundedProcessOutput, IosError> {
    let result = external_process::run_interruptible_capture_with_timeout(
        command,
        label,
        MAX_PROCESS_OUTPUT_BYTES,
        MAX_PROCESS_OUTPUT_BYTES,
        timeout,
    )
    .map_err(map_process_error)?;
    if !allow_failure && !result.status.success() {
        let stdout = String::from_utf8_lossy(&result.stdout);
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(IosError::worker(format!(
            "{label} failed with {}: {}{}{}",
            result.status,
            stderr.trim(),
            if stderr.is_empty() || stdout.is_empty() {
                ""
            } else {
                ": "
            },
            stdout.trim()
        )));
    }
    Ok(result)
}

fn require_expected_probe_failure(
    result: &BoundedProcessOutput,
    label: &str,
) -> Result<(), IosError> {
    if result.status.code() == Some(1) {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    Err(IosError::worker(format!(
        "{label} failed unexpectedly with {}: {}{}{}",
        result.status,
        stderr.trim(),
        if stderr.is_empty() || stdout.is_empty() {
            ""
        } else {
            ": "
        },
        stdout.trim()
    )))
}

fn map_process_error(error: external_process::ExternalProcessError) -> IosError {
    match error.kind() {
        ExternalProcessErrorKind::Compatibility => IosError::compatibility(error.to_string()),
        ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
            IosError::worker(error.to_string())
        }
    }
}

fn output_error(error: std::io::Error) -> IosError {
    IosError::worker(format!(
        "failed to write Swift package publication output: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_slug_rejects_urls_and_traversal() {
        assert!(validate_repository_slug("umbrella22/VesperPlayerKit", "repository").is_ok());
        assert!(validate_repository_slug("https://github.com/a/b", "repository").is_err());
        assert!(validate_repository_slug("owner/../repo", "repository").is_err());
        assert!(validate_repository_slug("owner", "repository").is_err());
    }

    #[test]
    fn release_asset_url_encodes_path_segments() {
        let url = release_asset_url(
            "umbrella22/Vesper",
            "v0.4.0",
            Path::new("VesperPlayerKit.xcframework.zip"),
        )
        .expect("URL");
        assert_eq!(
            url,
            "https://github.com/umbrella22/Vesper/releases/download/v0.4.0/VesperPlayerKit.xcframework.zip"
        );
    }

    #[test]
    fn branch_validation_rejects_ref_injection() {
        assert!(validate_branch_name("main").is_ok());
        assert!(validate_branch_name("release/v0.4").is_ok());
        assert!(validate_branch_name("bad..branch").is_err());
        assert!(validate_branch_name("-force").is_err());
        assert!(validate_branch_name("bad branch").is_err());
    }

    #[test]
    fn dry_run_output_is_confined_to_repository_dist() {
        let root = Path::new("/workspace/vesper");
        assert_eq!(
            resolve_output(root, Path::new("dist/release/spm-index")).expect("dist output"),
            root.join("dist/release/spm-index")
        );
        assert!(resolve_output(root, Path::new("dist/../outside")).is_err());
        assert!(resolve_output(root, Path::new("release/spm-index")).is_err());
        assert!(resolve_output(root, Path::new("/tmp/spm-index")).is_err());
    }

    #[test]
    fn generated_manifest_exports_only_capability_level_optional_products() {
        let directory = tempfile::tempdir().expect("temporary Swift package fixture");
        let root = directory.path();
        fs::write(root.join("LICENSE"), "license\n").expect("write fixture license");
        let ui = root.join("lib/ios/VesperPlayerKit/Sources/VesperPlayerKitUI");
        fs::create_dir_all(&ui).expect("create fixture UI sources");
        fs::write(ui.join("Controls.swift"), "public struct Controls {}\n")
            .expect("write fixture UI source");
        let targets = std::iter::once("VesperPlayerKit")
            .chain(OPTIONAL_BINARY_TARGETS)
            .collect::<Vec<_>>();
        let artifacts = targets
            .iter()
            .map(|target| PublishedBinaryArtifact {
                target,
                url: format!("https://example.invalid/{target}.xcframework.zip"),
                checksum: "0".repeat(64),
            })
            .collect::<Vec<_>>();
        let candidate = root.join("candidate");

        generate_candidate(root, &candidate, &artifacts, "umbrella22/Vesper")
            .expect("generate Swift package candidate");

        let manifest =
            fs::read_to_string(candidate.join("Package.swift")).expect("read generated manifest");
        for product in [
            "VesperPlayerKit",
            "VesperPlayerKitUI",
            "VesperPlayerSourceNormalizerFfmpeg",
            "VesperPlayerRemuxFfmpeg",
            "VesperPlayerPerformanceDiagnostics",
        ] {
            assert!(manifest.contains(&format!("name: \"{product}\"")));
        }
        for excluded in [
            "VesperPlayerDecoderVideoToolboxPlugin",
            "VesperPlayerFrameProcessorDiagnosticPlugin",
        ] {
            assert!(!manifest.contains(excluded));
        }
        for target in targets {
            assert_eq!(
                manifest
                    .matches(&format!(".binaryTarget(\n            name: \"{target}\""))
                    .count(),
                1,
                "binary target {target} must be declared exactly once"
            );
        }
    }
}
