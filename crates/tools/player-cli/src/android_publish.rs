use std::collections::BTreeSet;
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use md5::Md5;
use quick_xml::Reader;
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use serde::Deserialize;
use sha1::{Digest, Sha1};
use sha2::{Sha256, Sha512};
use tempfile::NamedTempFile;
use url::Url;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::android::{self, AndroidError};
use crate::external_process::{self, ExternalProcessErrorKind};
use crate::release;

const ARTIFACTS: [&str; 7] = [
    "vesper-player-kit",
    "vesper-player-kit-compose",
    "vesper-player-kit-compose-ui",
    "vesper-player-kit-ffmpeg-runtime",
    "vesper-player-kit-external-playback",
    "vesper-player-kit-source-normalizer-ffmpeg",
    "vesper-player-kit-remux-ffmpeg",
];
const MAVEN_GROUP_ID: &str = "io.github.umbrella22.vesper";
const MAVEN_PROJECT_URL: &str = "https://github.com/umbrella22/Vesper";
const MAVEN_LICENSE_NAME: &str = "Apache License, Version 2.0";
const MAVEN_LICENSE_URL: &str = "https://www.apache.org/licenses/LICENSE-2.0.txt";
const MAVEN_LGPL_LICENSE_NAME: &str = "GNU Lesser General Public License, Version 2.1 or later";
const MAVEN_LGPL_LICENSE_URL: &str = "https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html";
const MAVEN_DEVELOPER_ID: &str = "umbrella22";
const MAVEN_DEVELOPER_NAME: &str = "umbrella22";
const MAVEN_DEVELOPER_URL: &str = "https://github.com/umbrella22";
const MAVEN_SCM_CONNECTION: &str = "scm:git:https://github.com/umbrella22/Vesper.git";
const MAVEN_SCM_DEVELOPER_CONNECTION: &str = "scm:git:ssh://git@github.com/umbrella22/Vesper.git";
const MAX_REPOSITORY_ENTRIES: usize = 512;
const MAX_REPOSITORY_DEPTH: usize = 16;
const MAX_REPOSITORY_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_POM_BYTES: u64 = 1024 * 1024;
const MAX_CHECKSUM_BYTES: u64 = 1024;
const MAX_CURL_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const CENTRAL_REQUEST_TIMEOUT: Duration = Duration::from_secs(240);
const CENTRAL_POLL_ATTEMPTS: usize = 90;
const CENTRAL_POLL_INTERVAL: Duration = Duration::from_secs(10);
const BUNDLE_NAME: &str = "VesperPlayerKit-maven-central.zip";

#[derive(Clone, Copy)]
enum CentralRetryPolicy {
    Idempotent,
    Never,
}

pub(crate) struct MavenPublishRequest<'a> {
    pub(crate) tag: &'a str,
    pub(crate) portal_namespace: &'a str,
    pub(crate) dry_run: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentList {
    #[serde(default)]
    deployments: Vec<Deployment>,
    #[serde(default)]
    page_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Deployment {
    deployment_id: String,
    deployment_name: String,
    deployment_state: String,
    #[serde(default)]
    deployment_components: Vec<DeploymentComponent>,
}

#[derive(Debug, Clone, Deserialize)]
struct DeploymentComponent {
    purl: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentStatus {
    deployment_state: String,
    #[serde(default)]
    purls: Vec<String>,
}

struct CentralAuth {
    config: NamedTempFile,
}

#[derive(Default)]
struct PomDependency {
    group_id: Option<String>,
    artifact_id: Option<String>,
    version: Option<String>,
    scope: Option<String>,
}

impl CentralAuth {
    fn from_environment() -> Result<Self, AndroidError> {
        let username = required_secret("MAVEN_CENTRAL_USERNAME")?;
        let password = required_secret("MAVEN_CENTRAL_PASSWORD")?;
        let authorization = BASE64.encode(format!("{username}:{password}"));
        let mut config = tempfile::Builder::new()
            .prefix("vesper-central-auth.")
            .tempfile()
            .map_err(|error| {
                AndroidError::storage(format!(
                    "failed to create temporary Central Portal auth configuration: {error}"
                ))
            })?;
        writeln!(config, "header = \"Authorization: Bearer {authorization}\"")
            .and_then(|_| config.flush())
            .map_err(|error| {
                AndroidError::storage(format!(
                    "failed to write temporary Central Portal auth configuration: {error}"
                ))
            })?;
        Ok(Self { config })
    }

    fn apply(&self, command: &mut Command) {
        command.arg("--config").arg(self.config.path());
    }
}

pub(crate) fn publish(
    root: &Path,
    request: MavenPublishRequest<'_>,
    output: &mut dyn Write,
) -> Result<(), AndroidError> {
    let version = release::ReleaseContext::publication_version_from_tag(request.tag)
        .map_err(|error| AndroidError::usage(error.to_string()))?;
    validate_namespace(request.portal_namespace)?;
    validate_namespace_prefix(request.portal_namespace, MAVEN_GROUP_ID)?;
    let deployment_name = format!("{MAVEN_GROUP_ID}:VesperPlayerKit {}", request.tag);

    let auth = if request.dry_run {
        None
    } else {
        Some(CentralAuth::from_environment()?)
    };
    if let Some(auth) = auth.as_ref()
        && reconcile_existing_deployment(auth, &deployment_name, MAVEN_GROUP_ID, &version, output)?
    {
        return Ok(());
    }

    let signing_key = required_secret("MAVEN_GPG_PRIVATE_KEY")?;
    let signing_passphrase = optional_secret("MAVEN_GPG_PASSPHRASE");
    let release_directory = root.join("dist/release");
    fs::create_dir_all(&release_directory).map_err(|error| {
        AndroidError::storage(format!(
            "failed to create Maven release directory '{}': {error}",
            release_directory.display()
        ))
    })?;
    let staging = tempfile::Builder::new()
        .prefix(".vesper-maven-central-stage-")
        .tempdir_in(&release_directory)
        .map_err(|error| {
            AndroidError::storage(format!(
                "failed to create Maven Central staging directory: {error}"
            ))
        })?;
    let repository = staging.path().join("repository");
    android::stage_maven_publications(
        root,
        &repository,
        MAVEN_GROUP_ID,
        &version,
        &signing_key,
        signing_passphrase.as_deref(),
    )?;
    remove_generated_metadata(&repository)?;
    validate_repository(&repository, MAVEN_GROUP_ID, &version)?;

    let bundle = release_directory.join(BUNDLE_NAME);
    create_bundle(&repository, &bundle)?;
    writeln!(
        output,
        "Verified Maven Central bundle:\n  {}",
        bundle.display()
    )
    .map_err(output_error)?;

    if request.dry_run {
        writeln!(
            output,
            "Dry run complete; Central Portal was not contacted."
        )
        .map_err(output_error)?;
        return output.flush().map_err(output_error);
    }

    let auth = auth
        .as_ref()
        .ok_or_else(|| AndroidError::worker("Central Portal authentication was not initialized"))?;
    let deployment_id = upload_bundle(auth, &bundle, &deployment_name)?;
    writeln!(output, "Central Portal deployment: {deployment_id}").map_err(output_error)?;
    wait_for_deployment(auth, &deployment_id, MAVEN_GROUP_ID, &version, output)?;
    output.flush().map_err(output_error)
}

fn required_secret(name: &str) -> Result<String, AndroidError> {
    optional_secret(name).ok_or_else(|| {
        AndroidError::compatibility(format!("{name} is required for Maven Central publication"))
    })
}

fn optional_secret(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn validate_namespace(namespace: &str) -> Result<(), AndroidError> {
    let components = namespace.split('.').collect::<Vec<_>>();
    if components.len() < 2
        || components.iter().any(|component| {
            component.is_empty()
                || component.len() > 63
                || !component
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                || !component
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
        })
    {
        return Err(AndroidError::usage(format!(
            "Maven namespace must be a dotted identifier such as io.github.umbrella22.vesper: {namespace}"
        )));
    }
    Ok(())
}

fn validate_namespace_prefix(namespace: &str, group_id: &str) -> Result<(), AndroidError> {
    if group_id == namespace
        || group_id
            .strip_prefix(namespace)
            .is_some_and(|suffix| suffix.starts_with('.'))
    {
        return Ok(());
    }
    Err(AndroidError::usage(format!(
        "Maven Central namespace {namespace} does not authorize groupId {group_id}"
    )))
}

fn remove_generated_metadata(repository: &Path) -> Result<(), AndroidError> {
    if !repository.is_dir() {
        return Err(AndroidError::conformance(format!(
            "Gradle did not create the Maven repository '{}'.",
            repository.display()
        )));
    }
    let mut pending = vec![repository.to_path_buf()];
    let mut visited = 0_usize;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| {
            AndroidError::storage(format!(
                "failed to inspect Maven staging directory '{}': {error}",
                directory.display()
            ))
        })? {
            let entry = entry.map_err(|error| {
                AndroidError::storage(format!("failed to read Maven staging entry: {error}"))
            })?;
            visited += 1;
            if visited > MAX_REPOSITORY_ENTRIES {
                return Err(AndroidError::conformance(format!(
                    "Maven staging repository exceeds {MAX_REPOSITORY_ENTRIES} entries"
                )));
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                AndroidError::storage(format!(
                    "failed to inspect Maven staging path '{}': {error}",
                    path.display()
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(AndroidError::conformance(format!(
                    "Maven staging repository contains a symbolic link: {}",
                    path.display()
                )));
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("maven-metadata"))
            {
                fs::remove_file(&path).map_err(|error| {
                    AndroidError::storage(format!(
                        "failed to remove generated Maven metadata '{}': {error}",
                        path.display()
                    ))
                })?;
            }
        }
    }
    Ok(())
}

fn validate_repository(
    repository: &Path,
    namespace: &str,
    version: &str,
) -> Result<(), AndroidError> {
    let namespace_path = namespace.replace('.', "/");
    let expected_root = repository.join(&namespace_path);
    let files = collect_repository_files(repository)?;
    let mut expected_primary = BTreeSet::new();

    for artifact in ARTIFACTS {
        let version_directory = expected_root.join(artifact).join(version);
        for name in [
            format!("{artifact}-{version}.aar"),
            format!("{artifact}-{version}-sources.jar"),
            format!("{artifact}-{version}-javadoc.jar"),
            format!("{artifact}-{version}.pom"),
        ] {
            expected_primary.insert(version_directory.join(name));
        }
        let module = version_directory.join(format!("{artifact}-{version}.module"));
        if module.is_file() {
            expected_primary.insert(module);
        }
        validate_pom(
            &version_directory.join(format!("{artifact}-{version}.pom")),
            namespace,
            artifact,
            version,
        )?;
    }

    for primary in &expected_primary {
        validate_primary_publication(primary)?;
    }

    for path in files {
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("maven-metadata"))
        {
            return Err(AndroidError::conformance(format!(
                "Maven Central bundle must not contain generated metadata: {}",
                path.display()
            )));
        }
        let relative = path.strip_prefix(repository).map_err(|error| {
            AndroidError::conformance(format!(
                "Maven staging path escaped its repository root: {error}"
            ))
        })?;
        if !relative.starts_with(&namespace_path) {
            return Err(AndroidError::conformance(format!(
                "Maven staging contains a file outside namespace {namespace}: {}",
                relative.display()
            )));
        }
        if !is_expected_publication_file(&path, &expected_primary) {
            return Err(AndroidError::conformance(format!(
                "Maven staging contains an unexpected file: {}",
                relative.display()
            )));
        }
        if path.extension().and_then(|value| value.to_str()) == Some("md5") {
            verify_checksum(&strip_suffix(&path, ".md5")?, "md5")?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("sha1") {
            verify_checksum(&strip_suffix(&path, ".sha1")?, "sha1")?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("sha256") {
            verify_checksum(&strip_suffix(&path, ".sha256")?, "sha256")?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("sha512") {
            verify_checksum(&strip_suffix(&path, ".sha512")?, "sha512")?;
        }
    }
    Ok(())
}

fn collect_repository_files(repository: &Path) -> Result<Vec<PathBuf>, AndroidError> {
    let root = repository.canonicalize().map_err(|error| {
        AndroidError::storage(format!(
            "failed to resolve Maven repository '{}': {error}",
            repository.display()
        ))
    })?;
    let mut directories = vec![root.clone()];
    let mut files = Vec::new();
    let mut total_bytes = 0_u64;
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| {
            AndroidError::storage(format!(
                "failed to enumerate Maven directory '{}': {error}",
                directory.display()
            ))
        })? {
            let entry = entry.map_err(|error| {
                AndroidError::storage(format!("failed to read Maven repository entry: {error}"))
            })?;
            let path = entry.path();
            let relative = path.strip_prefix(&root).map_err(|error| {
                AndroidError::conformance(format!(
                    "Maven repository entry escaped its root: {error}"
                ))
            })?;
            if relative.components().count() > MAX_REPOSITORY_DEPTH
                || relative
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err(AndroidError::conformance(format!(
                    "Maven repository contains an invalid or over-depth path: {}",
                    relative.display()
                )));
            }
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                AndroidError::storage(format!(
                    "failed to inspect Maven repository entry '{}': {error}",
                    path.display()
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(AndroidError::conformance(format!(
                    "Maven repository contains a symbolic link: {}",
                    relative.display()
                )));
            }
            if metadata.is_dir() {
                directories.push(path);
            } else if metadata.is_file() {
                if metadata.len() == 0 {
                    return Err(AndroidError::conformance(format!(
                        "Maven repository contains an empty file: {}",
                        relative.display()
                    )));
                }
                total_bytes = total_bytes
                    .checked_add(metadata.len())
                    .ok_or_else(|| AndroidError::conformance("Maven repository size overflow"))?;
                if total_bytes > MAX_REPOSITORY_BYTES {
                    return Err(AndroidError::conformance(format!(
                        "Maven repository exceeds {MAX_REPOSITORY_BYTES} bytes"
                    )));
                }
                files.push(path);
                if files.len() > MAX_REPOSITORY_ENTRIES {
                    return Err(AndroidError::conformance(format!(
                        "Maven repository exceeds {MAX_REPOSITORY_ENTRIES} files"
                    )));
                }
            } else {
                return Err(AndroidError::conformance(format!(
                    "Maven repository contains a special file: {}",
                    relative.display()
                )));
            }
        }
    }
    files.sort();
    Ok(files)
}

fn is_expected_publication_file(path: &Path, primary: &BTreeSet<PathBuf>) -> bool {
    primary.iter().any(|base| {
        path == base
            || path == append_suffix(base, ".asc")
            || [".md5", ".sha1", ".sha256", ".sha512"]
                .iter()
                .any(|suffix| path == append_suffix(base, suffix))
    })
}

fn validate_primary_publication(primary: &Path) -> Result<(), AndroidError> {
    require_regular_nonempty(primary, "Maven publication file")?;
    require_regular_nonempty(&append_suffix(primary, ".asc"), "Maven signature")?;
    verify_checksum(primary, "md5")?;
    verify_checksum(primary, "sha1")
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.display()))
}

fn strip_suffix(path: &Path, suffix: &str) -> Result<PathBuf, AndroidError> {
    let text = path.to_str().ok_or_else(|| {
        AndroidError::conformance(format!("Maven path is not valid UTF-8: {}", path.display()))
    })?;
    text.strip_suffix(suffix).map(PathBuf::from).ok_or_else(|| {
        AndroidError::conformance(format!(
            "Maven path does not end with {suffix}: {}",
            path.display()
        ))
    })
}

fn require_regular_nonempty(path: &Path, label: &str) -> Result<(), AndroidError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AndroidError::conformance(format!("missing {label} '{}': {error}", path.display()))
    })?;
    if !metadata.file_type().is_file() || metadata.len() == 0 {
        return Err(AndroidError::conformance(format!(
            "{label} is not a non-empty regular file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn verify_checksum(path: &Path, algorithm: &str) -> Result<(), AndroidError> {
    require_regular_nonempty(path, "checksum target")?;
    let checksum_path = append_suffix(path, &format!(".{algorithm}"));
    let expected = read_bounded_text(&checksum_path, MAX_CHECKSUM_BYTES, "checksum")?;
    let expected = expected.trim();
    let actual = match algorithm {
        "md5" => digest_file::<Md5>(path)?,
        "sha1" => digest_file::<Sha1>(path)?,
        "sha256" => digest_file::<Sha256>(path)?,
        "sha512" => digest_file::<Sha512>(path)?,
        _ => {
            return Err(AndroidError::worker(format!(
                "unsupported checksum algorithm: {algorithm}"
            )));
        }
    };
    if !expected.eq_ignore_ascii_case(&actual) {
        return Err(AndroidError::conformance(format!(
            "{algorithm} checksum mismatch for '{}': expected {expected}, actual {actual}",
            path.display()
        )));
    }
    Ok(())
}

fn digest_file<D>(path: &Path) -> Result<String, AndroidError>
where
    D: Digest + Default,
{
    let mut input = File::open(path).map_err(|error| {
        AndroidError::storage(format!("failed to open '{}': {error}", path.display()))
    })?;
    let mut digest = D::default();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer).map_err(|error| {
            AndroidError::storage(format!("failed to read '{}': {error}", path.display()))
        })?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn validate_pom(
    path: &Path,
    namespace: &str,
    artifact: &str,
    version: &str,
) -> Result<(), AndroidError> {
    let (expected_name, expected_description) = pom_identity(artifact).ok_or_else(|| {
        AndroidError::worker(format!(
            "missing POM metadata policy for artifact {artifact}"
        ))
    })?;
    let source = read_bounded_text(path, MAX_POM_BYTES, "POM")?;
    let mut reader = Reader::from_str(&source);
    reader.config_mut().trim_text(true);
    let mut stack = Vec::<String>::new();
    let mut model_version = None;
    let mut group_id = None;
    let mut artifact_id = None;
    let mut pom_version = None;
    let mut packaging = None;
    let mut name = None;
    let mut description = None;
    let mut project_url = None;
    let mut license_names = Vec::new();
    let mut license_urls = Vec::new();
    let mut developer_ids = Vec::new();
    let mut developer_names = Vec::new();
    let mut developer_urls = Vec::new();
    let mut scm_connection = None;
    let mut scm_developer_connection = None;
    let mut scm_url = None;
    let mut current_dependency = None::<PomDependency>;
    let mut dependencies = Vec::<PomDependency>::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                stack.push(String::from_utf8_lossy(event.name().as_ref()).into_owned());
                if stack
                    .iter()
                    .map(String::as_str)
                    .eq(["project", "dependencies", "dependency"])
                    && current_dependency
                        .replace(PomDependency::default())
                        .is_some()
                {
                    return Err(AndroidError::conformance(format!(
                        "POM '{}' contains nested dependency elements",
                        path.display()
                    )));
                }
            }
            Ok(Event::End(event)) => {
                let name = String::from_utf8_lossy(event.name().as_ref()).into_owned();
                if name == "dependency"
                    && stack.iter().map(String::as_str).eq([
                        "project",
                        "dependencies",
                        "dependency",
                    ])
                {
                    dependencies.push(current_dependency.take().ok_or_else(|| {
                        AndroidError::conformance(format!(
                            "POM '{}' closed a dependency without opening it",
                            path.display()
                        ))
                    })?);
                }
                if stack.pop().as_deref() != Some(name.as_str()) {
                    return Err(AndroidError::conformance(format!(
                        "POM '{}' has mismatched XML elements",
                        path.display()
                    )));
                }
            }
            Ok(Event::Text(text)) => {
                let decoded = text.decode().map_err(|error| {
                    AndroidError::conformance(format!(
                        "POM '{}' contains invalid text encoding: {error}",
                        path.display()
                    ))
                })?;
                let value = unescape(&decoded).map_err(|error| {
                    AndroidError::conformance(format!(
                        "POM '{}' contains invalid XML escaping: {error}",
                        path.display()
                    ))
                })?;
                let value = value.into_owned();
                match stack
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    ["project", "modelVersion"] => {
                        assign_unique_pom_value(path, "modelVersion", &mut model_version, value)?
                    }
                    ["project", "groupId"] => {
                        assign_unique_pom_value(path, "groupId", &mut group_id, value)?
                    }
                    ["project", "artifactId"] => {
                        assign_unique_pom_value(path, "artifactId", &mut artifact_id, value)?
                    }
                    ["project", "version"] => {
                        assign_unique_pom_value(path, "version", &mut pom_version, value)?
                    }
                    ["project", "packaging"] => {
                        assign_unique_pom_value(path, "packaging", &mut packaging, value)?
                    }
                    ["project", "name"] => assign_unique_pom_value(path, "name", &mut name, value)?,
                    ["project", "description"] => {
                        assign_unique_pom_value(path, "description", &mut description, value)?
                    }
                    ["project", "url"] => {
                        assign_unique_pom_value(path, "url", &mut project_url, value)?
                    }
                    ["project", "licenses", "license", "name"] => license_names.push(value),
                    ["project", "licenses", "license", "url"] => license_urls.push(value),
                    ["project", "developers", "developer", "id"] => developer_ids.push(value),
                    ["project", "developers", "developer", "name"] => developer_names.push(value),
                    ["project", "developers", "developer", "url"] => developer_urls.push(value),
                    ["project", "scm", "connection"] => {
                        assign_unique_pom_value(path, "scm.connection", &mut scm_connection, value)?
                    }
                    ["project", "scm", "developerConnection"] => assign_unique_pom_value(
                        path,
                        "scm.developerConnection",
                        &mut scm_developer_connection,
                        value,
                    )?,
                    ["project", "scm", "url"] => {
                        assign_unique_pom_value(path, "scm.url", &mut scm_url, value)?
                    }
                    ["project", "dependencies", "dependency", "groupId"] => {
                        let dependency = current_dependency.as_mut().ok_or_else(|| {
                            AndroidError::conformance(format!(
                                "POM '{}' contains dependency fields outside a dependency",
                                path.display()
                            ))
                        })?;
                        assign_unique_pom_value(
                            path,
                            "dependency.groupId",
                            &mut dependency.group_id,
                            value,
                        )?;
                    }
                    ["project", "dependencies", "dependency", "artifactId"] => {
                        let dependency = current_dependency.as_mut().ok_or_else(|| {
                            AndroidError::conformance(format!(
                                "POM '{}' contains dependency fields outside a dependency",
                                path.display()
                            ))
                        })?;
                        assign_unique_pom_value(
                            path,
                            "dependency.artifactId",
                            &mut dependency.artifact_id,
                            value,
                        )?;
                    }
                    ["project", "dependencies", "dependency", "version"] => {
                        let dependency = current_dependency.as_mut().ok_or_else(|| {
                            AndroidError::conformance(format!(
                                "POM '{}' contains dependency fields outside a dependency",
                                path.display()
                            ))
                        })?;
                        assign_unique_pom_value(
                            path,
                            "dependency.version",
                            &mut dependency.version,
                            value,
                        )?;
                    }
                    ["project", "dependencies", "dependency", "scope"] => {
                        let dependency = current_dependency.as_mut().ok_or_else(|| {
                            AndroidError::conformance(format!(
                                "POM '{}' contains dependency fields outside a dependency",
                                path.display()
                            ))
                        })?;
                        assign_unique_pom_value(
                            path,
                            "dependency.scope",
                            &mut dependency.scope,
                            value,
                        )?;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(AndroidError::conformance(format!(
                    "POM '{}' is invalid XML: {error}",
                    path.display()
                )));
            }
        }
    }
    if !stack.is_empty() {
        return Err(AndroidError::conformance(format!(
            "POM '{}' has unclosed XML elements",
            path.display()
        )));
    }

    let actual_coordinates = (
        group_id.as_deref(),
        artifact_id.as_deref(),
        pom_version.as_deref(),
    );
    let expected_coordinates = (Some(namespace), Some(artifact), Some(version));
    if actual_coordinates != expected_coordinates {
        return Err(AndroidError::conformance(format!(
            "POM '{}' has coordinates {:?}, expected {:?}",
            path.display(),
            actual_coordinates,
            expected_coordinates
        )));
    }

    let (expected_license_name, expected_license_url) = pom_license(artifact).ok_or_else(|| {
        AndroidError::worker(format!(
            "missing POM license policy for artifact {artifact}"
        ))
    })?;
    let expected_license_names = [expected_license_name.to_owned()];
    let expected_license_urls = [expected_license_url.to_owned()];
    let expected_developer_ids = [MAVEN_DEVELOPER_ID.to_owned()];
    let expected_developer_names = [MAVEN_DEVELOPER_NAME.to_owned()];
    let expected_developer_urls = [MAVEN_DEVELOPER_URL.to_owned()];
    let metadata_is_valid = model_version.as_deref() == Some("4.0.0")
        && packaging.as_deref() == Some("aar")
        && name.as_deref() == Some(expected_name)
        && description.as_deref() == Some(expected_description)
        && project_url.as_deref() == Some(MAVEN_PROJECT_URL)
        && license_names.as_slice() == expected_license_names.as_slice()
        && license_urls.as_slice() == expected_license_urls.as_slice()
        && developer_ids.as_slice() == expected_developer_ids.as_slice()
        && developer_names.as_slice() == expected_developer_names.as_slice()
        && developer_urls.as_slice() == expected_developer_urls.as_slice()
        && scm_connection.as_deref() == Some(MAVEN_SCM_CONNECTION)
        && scm_developer_connection.as_deref() == Some(MAVEN_SCM_DEVELOPER_CONNECTION)
        && scm_url.as_deref() == Some(MAVEN_PROJECT_URL);
    if !metadata_is_valid {
        return Err(AndroidError::conformance(format!(
            "POM '{}' does not contain the required Maven Central metadata for {artifact}",
            path.display()
        )));
    }
    validate_internal_pom_dependencies(path, namespace, artifact, version, &dependencies)?;
    Ok(())
}

fn validate_internal_pom_dependencies(
    path: &Path,
    namespace: &str,
    artifact: &str,
    version: &str,
    dependencies: &[PomDependency],
) -> Result<(), AndroidError> {
    let actual = dependencies
        .iter()
        .filter(|dependency| dependency.group_id.as_deref() == Some(namespace))
        .map(|dependency| {
            let artifact_id = dependency.artifact_id.as_deref().ok_or_else(|| {
                AndroidError::conformance(format!(
                    "POM '{}' contains an internal dependency without artifactId",
                    path.display()
                ))
            })?;
            let dependency_version = dependency.version.as_deref().ok_or_else(|| {
                AndroidError::conformance(format!(
                    "POM '{}' contains internal dependency {artifact_id} without version",
                    path.display()
                ))
            })?;
            let scope = dependency.scope.as_deref().ok_or_else(|| {
                AndroidError::conformance(format!(
                    "POM '{}' contains internal dependency {artifact_id} without scope",
                    path.display()
                ))
            })?;
            Ok((
                artifact_id.to_owned(),
                dependency_version.to_owned(),
                scope.to_owned(),
            ))
        })
        .collect::<Result<BTreeSet<_>, AndroidError>>()?;
    let expected_artifacts: &[&str] = match artifact {
        "vesper-player-kit" | "vesper-player-kit-ffmpeg-runtime" => &[],
        "vesper-player-kit-compose" => &["vesper-player-kit"],
        "vesper-player-kit-compose-ui" => &["vesper-player-kit-compose"],
        "vesper-player-kit-external-playback" => {
            &["vesper-player-kit", "vesper-player-kit-ffmpeg-runtime"]
        }
        "vesper-player-kit-source-normalizer-ffmpeg" | "vesper-player-kit-remux-ffmpeg" => {
            &["vesper-player-kit", "vesper-player-kit-ffmpeg-runtime"]
        }
        _ => {
            return Err(AndroidError::worker(format!(
                "missing internal dependency policy for artifact {artifact}"
            )));
        }
    };
    let expected = expected_artifacts
        .iter()
        .map(|artifact_id| {
            (
                (*artifact_id).to_owned(),
                version.to_owned(),
                "compile".to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(AndroidError::conformance(format!(
            "POM '{}' does not contain the required same-version internal dependency closure for {artifact}",
            path.display()
        )));
    }
    Ok(())
}

fn assign_unique_pom_value(
    path: &Path,
    field: &str,
    target: &mut Option<String>,
    value: String,
) -> Result<(), AndroidError> {
    if value.is_empty() {
        return Err(AndroidError::conformance(format!(
            "POM '{}' contains an empty {field}",
            path.display()
        )));
    }
    if target.replace(value).is_some() {
        return Err(AndroidError::conformance(format!(
            "POM '{}' contains duplicate {field} values",
            path.display()
        )));
    }
    Ok(())
}

fn pom_identity(artifact: &str) -> Option<(&'static str, &'static str)> {
    match artifact {
        "vesper-player-kit" => Some((
            "Vesper Player Android Kit",
            "Android host kit for Vesper Player applications.",
        )),
        "vesper-player-kit-compose" => Some((
            "Vesper Player Android Compose Adapter",
            "Jetpack Compose lifecycle and surface adapter for Vesper Player.",
        )),
        "vesper-player-kit-compose-ui" => Some((
            "Vesper Player Android Compose UI",
            "Optional Jetpack Compose controls and player UI for Vesper Player.",
        )),
        "vesper-player-kit-ffmpeg-runtime" => Some((
            "Vesper Player Android FFmpeg Runtime",
            "Optional Android FFmpeg runtime libraries for Vesper Player plugins. Redistributed FFmpeg components keep their upstream license terms.",
        )),
        "vesper-player-kit-external-playback" => Some((
            "Vesper Player Android External Playback",
            "Optional Cast, DLNA, local relay, and relay format adaptation integration for Vesper Player Android hosts.",
        )),
        "vesper-player-kit-source-normalizer-ffmpeg" => Some((
            "Vesper Player Android SourceNormalizer FFmpeg Plugin",
            "Optional Android SourceNormalizer plugin for Vesper Player. The artifact depends on the FFmpeg runtime artifact for libav/libsw dependencies.",
        )),
        "vesper-player-kit-remux-ffmpeg" => Some((
            "Vesper Player Android Remux FFmpeg Plugin",
            "Optional Android post-download remux plugin for Vesper Player. The artifact depends on the FFmpeg runtime artifact for libav dependencies.",
        )),
        _ => None,
    }
}

fn pom_license(artifact: &str) -> Option<(&'static str, &'static str)> {
    match artifact {
        "vesper-player-kit-ffmpeg-runtime" => {
            Some((MAVEN_LGPL_LICENSE_NAME, MAVEN_LGPL_LICENSE_URL))
        }
        artifact if ARTIFACTS.contains(&artifact) => Some((MAVEN_LICENSE_NAME, MAVEN_LICENSE_URL)),
        _ => None,
    }
}

fn read_bounded_text(path: &Path, maximum_bytes: u64, label: &str) -> Result<String, AndroidError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AndroidError::conformance(format!("missing {label} '{}': {error}", path.display()))
    })?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > maximum_bytes {
        return Err(AndroidError::conformance(format!(
            "{label} '{}' is empty, non-regular, or exceeds {maximum_bytes} bytes",
            path.display()
        )));
    }
    let mut source = String::with_capacity(metadata.len() as usize);
    File::open(path)
        .and_then(|mut input| input.read_to_string(&mut source))
        .map_err(|error| {
            AndroidError::storage(format!(
                "failed to read {label} '{}': {error}",
                path.display()
            ))
        })?;
    Ok(source)
}

fn create_bundle(repository: &Path, destination: &Path) -> Result<(), AndroidError> {
    let files = collect_repository_files(repository)?;
    let parent = destination.parent().ok_or_else(|| {
        AndroidError::storage(format!(
            "Maven Central bundle has no parent directory: {}",
            destination.display()
        ))
    })?;
    let temporary = tempfile::Builder::new()
        .prefix(".vesper-maven-central-bundle-")
        .tempfile_in(parent)
        .map_err(|error| {
            AndroidError::storage(format!("failed to create temporary Maven bundle: {error}"))
        })?;
    let mut archive = ZipWriter::new(temporary.reopen().map_err(|error| {
        AndroidError::storage(format!("failed to reopen temporary Maven bundle: {error}"))
    })?);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    for path in files {
        let relative = path.strip_prefix(repository).map_err(|error| {
            AndroidError::conformance(format!("Maven bundle path escaped repository: {error}"))
        })?;
        let name = relative.to_str().ok_or_else(|| {
            AndroidError::conformance(format!(
                "Maven bundle path is not valid UTF-8: {}",
                relative.display()
            ))
        })?;
        archive
            .start_file(name.replace('\\', "/"), options)
            .map_err(|error| {
                AndroidError::storage(format!(
                    "failed to start Maven bundle entry '{}': {error}",
                    relative.display()
                ))
            })?;
        let mut input = File::open(&path).map_err(|error| {
            AndroidError::storage(format!("failed to open '{}': {error}", path.display()))
        })?;
        std::io::copy(&mut input, &mut archive).map_err(|error| {
            AndroidError::storage(format!(
                "failed to write Maven bundle entry '{}': {error}",
                relative.display()
            ))
        })?;
    }
    let completed = archive.finish().map_err(|error| {
        AndroidError::storage(format!("failed to finish Maven Central bundle: {error}"))
    })?;
    completed.sync_all().map_err(|error| {
        AndroidError::storage(format!(
            "failed to synchronize Maven Central bundle: {error}"
        ))
    })?;
    if destination.exists() {
        fs::remove_file(destination).map_err(|error| {
            AndroidError::storage(format!(
                "failed to replace old Maven Central bundle '{}': {error}",
                destination.display()
            ))
        })?;
    }
    temporary.persist(destination).map_err(|error| {
        AndroidError::storage(format!(
            "failed to commit Maven Central bundle '{}': {}",
            destination.display(),
            error.error
        ))
    })?;
    Ok(())
}

fn reconcile_existing_deployment(
    auth: &CentralAuth,
    deployment_name: &str,
    group_id: &str,
    version: &str,
    output: &mut dyn Write,
) -> Result<bool, AndroidError> {
    let mut url = Url::parse("https://central.sonatype.com/api/v1/publisher/deployments")
        .map_err(|error| AndroidError::worker(format!("invalid Central Portal URL: {error}")))?;
    url.query_pairs_mut()
        .append_pair("deploymentName", deployment_name)
        .append_pair("size", "100")
        .append_pair("sortField", "createTimestamp")
        .append_pair("sortDirection", "desc");
    let response = central_request(
        auth,
        "GET",
        url.as_str(),
        None,
        CentralRetryPolicy::Idempotent,
        "Central deployment list",
    )?;
    let list: DeploymentList = serde_json::from_slice(&response).map_err(|error| {
        AndroidError::worker(format!(
            "Central Portal returned an invalid deployment list: {error}"
        ))
    })?;
    if list.page_count > 1 {
        return Err(AndroidError::conformance(format!(
            "Central Portal returned more than 100 deployments matching {deployment_name}; reconcile them in the Portal"
        )));
    }
    let exact = list
        .deployments
        .into_iter()
        .filter(|deployment| deployment.deployment_name == deployment_name)
        .collect::<Vec<_>>();
    if let Some(published) = exact
        .iter()
        .find(|deployment| deployment.deployment_state == "PUBLISHED")
    {
        validate_component_purls(
            published
                .deployment_components
                .iter()
                .filter_map(|component| component.purl.as_deref()),
            group_id,
            version,
        )?;
        writeln!(
            output,
            "Maven Central deployment {} is already published; skipping upload.",
            published.deployment_id
        )
        .map_err(output_error)?;
        return Ok(true);
    }
    let active = exact
        .iter()
        .filter(|deployment| {
            matches!(
                deployment.deployment_state.as_str(),
                "PENDING" | "VALIDATING" | "VALIDATED" | "PUBLISHING"
            )
        })
        .collect::<Vec<_>>();
    if active.len() > 1 {
        return Err(AndroidError::conformance(format!(
            "Central Portal has multiple active deployments named {deployment_name}; reconcile them before retrying"
        )));
    }
    if let Some(deployment) = active.first() {
        writeln!(
            output,
            "Resuming Central Portal deployment {} in state {}.",
            deployment.deployment_id, deployment.deployment_state
        )
        .map_err(output_error)?;
        wait_for_deployment(auth, &deployment.deployment_id, group_id, version, output)?;
        return Ok(true);
    }
    if let Some(failed) = exact
        .iter()
        .find(|deployment| deployment.deployment_state == "FAILED")
    {
        return Err(AndroidError::conformance(format!(
            "Central Portal deployment {} named {deployment_name} already failed; inspect or drop it in the Portal before retrying",
            failed.deployment_id
        )));
    }
    Ok(false)
}

fn upload_bundle(
    auth: &CentralAuth,
    bundle: &Path,
    deployment_name: &str,
) -> Result<String, AndroidError> {
    let mut url = Url::parse("https://central.sonatype.com/api/v1/publisher/upload")
        .map_err(|error| AndroidError::worker(format!("invalid Central upload URL: {error}")))?;
    url.query_pairs_mut()
        .append_pair("name", deployment_name)
        .append_pair("publishingType", "AUTOMATIC");
    let form = format!("bundle=@{};type=application/octet-stream", bundle.display());
    let response = central_request(
        auth,
        "POST",
        url.as_str(),
        Some(&form),
        CentralRetryPolicy::Never,
        "Central bundle upload",
    )?;
    let deployment_id = String::from_utf8(response).map_err(|error| {
        AndroidError::worker(format!(
            "Central Portal deployment id is not valid UTF-8: {error}"
        ))
    })?;
    let deployment_id = deployment_id.trim();
    if deployment_id.len() < 32
        || deployment_id.len() > 64
        || !deployment_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        return Err(AndroidError::worker(format!(
            "Central Portal returned an invalid deployment id: {deployment_id}"
        )));
    }
    Ok(deployment_id.to_owned())
}

fn wait_for_deployment(
    auth: &CentralAuth,
    deployment_id: &str,
    group_id: &str,
    version: &str,
    output: &mut dyn Write,
) -> Result<(), AndroidError> {
    for attempt in 1..=CENTRAL_POLL_ATTEMPTS {
        let mut url = Url::parse("https://central.sonatype.com/api/v1/publisher/status").map_err(
            |error| AndroidError::worker(format!("invalid Central status URL: {error}")),
        )?;
        url.query_pairs_mut().append_pair("id", deployment_id);
        let response = central_request(
            auth,
            "POST",
            url.as_str(),
            None,
            CentralRetryPolicy::Idempotent,
            "Central deployment status",
        )?;
        let status: DeploymentStatus = serde_json::from_slice(&response).map_err(|error| {
            AndroidError::worker(format!(
                "Central Portal returned invalid status for {deployment_id}: {error}"
            ))
        })?;
        match status.deployment_state.as_str() {
            "PUBLISHED" => {
                validate_component_purls(
                    status.purls.iter().map(String::as_str),
                    group_id,
                    version,
                )?;
                writeln!(
                    output,
                    "Published Maven Central deployment {deployment_id}."
                )
                .map_err(output_error)?;
                return Ok(());
            }
            "FAILED" => {
                let body = String::from_utf8_lossy(&response);
                return Err(AndroidError::conformance(format!(
                    "Central Portal deployment {deployment_id} failed: {body}"
                )));
            }
            "PENDING" | "VALIDATING" | "VALIDATED" | "PUBLISHING" => {
                writeln!(
                    output,
                    "Central Portal state: {} (attempt {attempt}/{CENTRAL_POLL_ATTEMPTS})",
                    status.deployment_state
                )
                .map_err(output_error)?;
            }
            state => {
                return Err(AndroidError::worker(format!(
                    "Central Portal returned an unexpected deployment state: {state}"
                )));
            }
        }
        if attempt < CENTRAL_POLL_ATTEMPTS {
            thread::sleep(CENTRAL_POLL_INTERVAL);
        }
    }
    Err(AndroidError::worker(format!(
        "timed out waiting for Maven Central deployment {deployment_id}"
    )))
}

fn validate_component_purls<'a>(
    actual: impl Iterator<Item = &'a str>,
    group_id: &str,
    version: &str,
) -> Result<(), AndroidError> {
    let reported = actual.map(str::to_owned).collect::<BTreeSet<_>>();
    let actual = reported
        .iter()
        .map(|purl| canonical_central_aar_component_purl(purl).to_owned())
        .collect::<BTreeSet<_>>();
    let expected = ARTIFACTS
        .iter()
        .map(|artifact| format!("pkg:maven/{group_id}/{artifact}@{version}"))
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(AndroidError::conformance(format!(
            "Central deployment components do not match the expected coordinates\n  expected: {}\n  actual: {}",
            expected.into_iter().collect::<Vec<_>>().join(", "),
            reported.into_iter().collect::<Vec<_>>().join(", ")
        )));
    }
    Ok(())
}

fn canonical_central_aar_component_purl(purl: &str) -> &str {
    // Central reports an AAR both as its Maven coordinate and as a packaging-qualified PURL.
    purl.strip_suffix("?type=aar").unwrap_or(purl)
}

fn central_request(
    auth: &CentralAuth,
    method: &str,
    url: &str,
    form: Option<&str>,
    retry_policy: CentralRetryPolicy,
    label: &str,
) -> Result<Vec<u8>, AndroidError> {
    let mut command = Command::new("curl");
    command.args([
        "--fail-with-body",
        "--silent",
        "--show-error",
        "--connect-timeout",
        "30",
        "--max-time",
        "180",
        "--request",
        method,
    ]);
    if matches!(retry_policy, CentralRetryPolicy::Idempotent) {
        command.args(["--retry", "3", "--retry-all-errors"]);
    }
    auth.apply(&mut command);
    if let Some(form) = form {
        command.arg("--form").arg(form);
    }
    command.arg(url);
    let result = external_process::run_interruptible_capture_with_timeout(
        &mut command,
        label,
        MAX_CURL_OUTPUT_BYTES,
        MAX_CURL_OUTPUT_BYTES,
        CENTRAL_REQUEST_TIMEOUT,
    )
    .map_err(map_process_error)?;
    if !result.status.success() {
        let diagnostics = String::from_utf8_lossy(&result.stderr);
        let body = String::from_utf8_lossy(&result.stdout);
        return Err(AndroidError::worker(format!(
            "{label} failed with {}: {}{}{}",
            result.status,
            diagnostics.trim(),
            if diagnostics.is_empty() || body.is_empty() {
                ""
            } else {
                ": "
            },
            body.trim()
        )));
    }
    Ok(result.stdout)
}

fn map_process_error(error: external_process::ExternalProcessError) -> AndroidError {
    match error.kind() {
        ExternalProcessErrorKind::Compatibility => AndroidError::compatibility(error.to_string()),
        ExternalProcessErrorKind::Worker | ExternalProcessErrorKind::Cancelled => {
            AndroidError::worker(error.to_string())
        }
    }
}

fn output_error(error: std::io::Error) -> AndroidError {
    AndroidError::worker(format!("failed to write Maven publication output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_validation_rejects_paths_and_empty_segments() {
        assert!(validate_namespace("io.github.umbrella22.vesper").is_ok());
        assert!(validate_namespace("io/github/umbrella22").is_err());
        assert!(validate_namespace("io..ikaros").is_err());
        assert!(validate_namespace("single").is_err());
    }

    #[test]
    fn portal_namespace_must_authorize_the_fixed_group_id() {
        assert!(validate_namespace_prefix("io.github.umbrella22", MAVEN_GROUP_ID).is_ok());
        assert!(validate_namespace_prefix(MAVEN_GROUP_ID, MAVEN_GROUP_ID).is_ok());
        assert!(validate_namespace_prefix("io.github.umbrella2", MAVEN_GROUP_ID).is_err());
        assert!(validate_namespace_prefix("io.github.other", MAVEN_GROUP_ID).is_err());
        assert!(
            validate_namespace_prefix("io.github.umbrella22.vesper.child", MAVEN_GROUP_ID).is_err()
        );
    }

    #[test]
    fn expected_publication_files_include_primary_checksums_without_signature_checksums() {
        let base = PathBuf::from("artifact-0.4.0.aar");
        let primary = BTreeSet::from([base.clone()]);
        assert!(is_expected_publication_file(&base, &primary));
        assert!(is_expected_publication_file(
            &PathBuf::from("artifact-0.4.0.aar.asc"),
            &primary
        ));
        assert!(is_expected_publication_file(
            &PathBuf::from("artifact-0.4.0.aar.sha1"),
            &primary
        ));
        assert!(!is_expected_publication_file(
            &PathBuf::from("artifact-0.4.0.aar.asc.md5"),
            &primary
        ));
        assert!(!is_expected_publication_file(
            &PathBuf::from("maven-metadata.xml"),
            &primary
        ));
    }

    #[test]
    fn primary_publication_accepts_javadoc_signature_without_signature_checksum() {
        let directory = tempfile::tempdir().expect("temporary Maven publication directory");
        let primary = directory.path().join("vesper-player-kit-0.5.0-javadoc.jar");
        fs::write(&primary, b"javadoc archive").expect("write javadoc archive");
        fs::write(append_suffix(&primary, ".asc"), b"armored signature")
            .expect("write javadoc signature");
        fs::write(
            append_suffix(&primary, ".md5"),
            digest_file::<Md5>(&primary).expect("digest javadoc archive with MD5"),
        )
        .expect("write javadoc MD5 checksum");
        fs::write(
            append_suffix(&primary, ".sha1"),
            digest_file::<Sha1>(&primary).expect("digest javadoc archive with SHA-1"),
        )
        .expect("write javadoc SHA-1 checksum");

        validate_primary_publication(&primary)
            .expect("signature checksum files are not required by Maven Central");
        assert!(!append_suffix(&primary, ".asc.md5").exists());
        assert!(!append_suffix(&primary, ".asc.sha1").exists());
    }

    #[test]
    fn pom_validation_requires_complete_maven_central_metadata() {
        let directory = tempfile::tempdir().expect("temporary POM directory");
        let path = directory.path().join("vesper-player-kit-0.4.0.pom");
        fs::write(&path, valid_test_pom("vesper-player-kit")).expect("write valid POM");

        validate_pom(&path, MAVEN_GROUP_ID, "vesper-player-kit", "0.4.0")
            .expect("valid Maven Central POM");
    }

    #[test]
    fn pom_validation_rejects_wrong_packaging_and_missing_scm() {
        let directory = tempfile::tempdir().expect("temporary POM directory");
        let wrong_packaging = directory.path().join("wrong-packaging.pom");
        fs::write(
            &wrong_packaging,
            valid_test_pom("vesper-player-kit")
                .replace("<packaging>aar</packaging>", "<packaging>jar</packaging>"),
        )
        .expect("write wrong-packaging POM");
        assert!(
            validate_pom(
                &wrong_packaging,
                MAVEN_GROUP_ID,
                "vesper-player-kit",
                "0.4.0",
            )
            .expect_err("reject wrong packaging")
            .to_string()
            .contains("required Maven Central metadata")
        );

        let missing_scm = directory.path().join("missing-scm.pom");
        let scm = format!(
            "<scm><connection>{MAVEN_SCM_CONNECTION}</connection><developerConnection>{MAVEN_SCM_DEVELOPER_CONNECTION}</developerConnection><url>{MAVEN_PROJECT_URL}</url></scm>"
        );
        fs::write(
            &missing_scm,
            valid_test_pom("vesper-player-kit").replace(&scm, ""),
        )
        .expect("write missing-SCM POM");
        assert!(validate_pom(&missing_scm, MAVEN_GROUP_ID, "vesper-player-kit", "0.4.0",).is_err());
    }

    #[test]
    fn pom_validation_requires_external_playback_dependency_closure_and_ffmpeg_license() {
        let directory = tempfile::tempdir().expect("temporary POM directory");
        let external = directory.path().join("external.pom");
        fs::write(
            &external,
            valid_test_pom("vesper-player-kit-external-playback"),
        )
        .expect("write external playback POM");
        validate_pom(
            &external,
            MAVEN_GROUP_ID,
            "vesper-player-kit-external-playback",
            "0.4.0",
        )
        .expect("valid external playback POM");

        let missing_runtime = directory.path().join("missing-runtime.pom");
        fs::write(
            &missing_runtime,
            valid_test_pom("vesper-player-kit-external-playback").replace(
                &internal_dependency_xml("vesper-player-kit-ffmpeg-runtime"),
                "",
            ),
        )
        .expect("write incomplete external playback POM");
        assert!(
            validate_pom(
                &missing_runtime,
                MAVEN_GROUP_ID,
                "vesper-player-kit-external-playback",
                "0.4.0",
            )
            .expect_err("reject missing FFmpeg runtime dependency")
            .to_string()
            .contains("dependency closure")
        );

        let runtime = directory.path().join("runtime.pom");
        fs::write(&runtime, valid_test_pom("vesper-player-kit-ffmpeg-runtime"))
            .expect("write FFmpeg runtime POM");
        validate_pom(
            &runtime,
            MAVEN_GROUP_ID,
            "vesper-player-kit-ffmpeg-runtime",
            "0.4.0",
        )
        .expect("valid FFmpeg runtime POM");
    }

    #[test]
    fn optional_plugin_poms_require_same_version_core_and_runtime_closure() {
        let directory = tempfile::tempdir().expect("temporary POM directory");
        for artifact in [
            "vesper-player-kit-source-normalizer-ffmpeg",
            "vesper-player-kit-remux-ffmpeg",
        ] {
            let valid = directory.path().join(format!("{artifact}.pom"));
            fs::write(&valid, valid_test_pom(artifact)).expect("write optional plugin POM");
            validate_pom(&valid, MAVEN_GROUP_ID, artifact, "0.4.0")
                .expect("valid optional plugin POM");

            for missing in ["vesper-player-kit", "vesper-player-kit-ffmpeg-runtime"] {
                let incomplete = directory
                    .path()
                    .join(format!("{artifact}-missing-{missing}.pom"));
                fs::write(
                    &incomplete,
                    valid_test_pom(artifact).replace(&internal_dependency_xml(missing), ""),
                )
                .expect("write incomplete optional plugin POM");
                assert!(
                    validate_pom(&incomplete, MAVEN_GROUP_ID, artifact, "0.4.0")
                        .expect_err("reject incomplete optional plugin closure")
                        .to_string()
                        .contains("dependency closure")
                );
            }
        }
    }

    #[test]
    fn pom_validation_accepts_prerelease_publication_versions() {
        let directory = tempfile::tempdir().expect("temporary POM directory");
        let path = directory.path().join("remux-prerelease.pom");
        fs::write(
            &path,
            valid_test_pom("vesper-player-kit-remux-ffmpeg").replace("0.4.0", "0.4.3-rc.1"),
        )
        .expect("write prerelease optional plugin POM");

        validate_pom(
            &path,
            MAVEN_GROUP_ID,
            "vesper-player-kit-remux-ffmpeg",
            "0.4.3-rc.1",
        )
        .expect("valid prerelease optional plugin POM");
    }

    fn valid_test_pom(artifact: &str) -> String {
        let (name, description) = pom_identity(artifact).expect("known test artifact");
        let (license_name, license_url) = pom_license(artifact).expect("known license policy");
        let dependencies = match artifact {
            "vesper-player-kit" | "vesper-player-kit-ffmpeg-runtime" => String::new(),
            "vesper-player-kit-compose" => internal_dependency_xml("vesper-player-kit"),
            "vesper-player-kit-compose-ui" => internal_dependency_xml("vesper-player-kit-compose"),
            "vesper-player-kit-external-playback"
            | "vesper-player-kit-source-normalizer-ffmpeg"
            | "vesper-player-kit-remux-ffmpeg" => format!(
                "{}{}",
                internal_dependency_xml("vesper-player-kit"),
                internal_dependency_xml("vesper-player-kit-ffmpeg-runtime")
            ),
            _ => panic!("unknown test artifact: {artifact}"),
        };
        let dependencies = if dependencies.is_empty() {
            String::new()
        } else {
            format!("<dependencies>{dependencies}</dependencies>")
        };
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><project><modelVersion>4.0.0</modelVersion><groupId>{MAVEN_GROUP_ID}</groupId><artifactId>{artifact}</artifactId><version>0.4.0</version><packaging>aar</packaging><name>{name}</name><description>{description}</description><url>{MAVEN_PROJECT_URL}</url><licenses><license><name>{license_name}</name><url>{license_url}</url></license></licenses><developers><developer><id>{MAVEN_DEVELOPER_ID}</id><name>{MAVEN_DEVELOPER_NAME}</name><url>{MAVEN_DEVELOPER_URL}</url></developer></developers><scm><connection>{MAVEN_SCM_CONNECTION}</connection><developerConnection>{MAVEN_SCM_DEVELOPER_CONNECTION}</developerConnection><url>{MAVEN_PROJECT_URL}</url></scm>{dependencies}</project>"
        )
    }

    fn internal_dependency_xml(artifact: &str) -> String {
        format!(
            "<dependency><groupId>{MAVEN_GROUP_ID}</groupId><artifactId>{artifact}</artifactId><version>0.4.0</version><scope>compile</scope></dependency>"
        )
    }

    #[test]
    fn component_purls_require_every_public_android_coordinate() {
        let complete = [
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit@0.4.0",
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit-compose@0.4.0",
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit-compose-ui@0.4.0",
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit-ffmpeg-runtime@0.4.0",
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit-external-playback@0.4.0",
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit-source-normalizer-ffmpeg@0.4.0",
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit-remux-ffmpeg@0.4.0",
        ];
        assert!(
            validate_component_purls(
                complete.iter().copied(),
                "io.github.umbrella22.vesper",
                "0.4.0"
            )
            .is_ok()
        );
        assert!(
            validate_component_purls(
                complete[..6].iter().copied(),
                "io.github.umbrella22.vesper",
                "0.4.0"
            )
            .is_err()
        );
        let mut extra = complete.to_vec();
        extra.push("pkg:maven/io.github.umbrella22.vesper/unexpected@0.4.0");
        assert!(
            validate_component_purls(
                extra.iter().copied(),
                "io.github.umbrella22.vesper",
                "0.4.0"
            )
            .is_err()
        );
    }

    #[test]
    fn component_purls_accept_central_aar_packaging_aliases_only() {
        let coordinates = [
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit@0.4.0",
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit-compose@0.4.0",
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit-compose-ui@0.4.0",
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit-ffmpeg-runtime@0.4.0",
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit-external-playback@0.4.0",
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit-source-normalizer-ffmpeg@0.4.0",
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit-remux-ffmpeg@0.4.0",
        ];
        let packaging_qualified = coordinates
            .iter()
            .map(|coordinate| format!("{coordinate}?type=aar"))
            .collect::<Vec<_>>();
        assert!(
            validate_component_purls(
                coordinates
                    .iter()
                    .copied()
                    .chain(packaging_qualified.iter().map(String::as_str)),
                "io.github.umbrella22.vesper",
                "0.4.0",
            )
            .is_ok()
        );

        let mut unsupported_qualifier = coordinates.to_vec();
        unsupported_qualifier[0] =
            "pkg:maven/io.github.umbrella22.vesper/vesper-player-kit@0.4.0?type=jar";
        assert!(
            validate_component_purls(
                unsupported_qualifier.iter().copied(),
                "io.github.umbrella22.vesper",
                "0.4.0",
            )
            .is_err()
        );
    }
}
