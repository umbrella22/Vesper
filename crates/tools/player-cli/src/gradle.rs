use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const MAX_GRADLE_WRAPPER_PROPERTIES_BYTES: u64 = 64 * 1024;
const MAX_GRADLE_CACHE_ENTRIES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GradleErrorKind {
    Storage,
    Compatibility,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct GradleError {
    kind: GradleErrorKind,
    message: String,
}

impl GradleError {
    fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: GradleErrorKind::Storage,
            message: message.into(),
        }
    }

    fn compatibility(message: impl Into<String>) -> Self {
        Self {
            kind: GradleErrorKind::Compatibility,
            message: message.into(),
        }
    }

    pub(crate) const fn kind(&self) -> GradleErrorKind {
        self.kind
    }
}

pub(crate) fn service_home(project_directory: &Path) -> PathBuf {
    let configured_home = env::var_os("GRADLE_USER_HOME");
    service_home_with_override(project_directory, configured_home.as_deref())
}

fn service_home_with_override(
    project_directory: &Path,
    configured_home: Option<&OsStr>,
) -> PathBuf {
    configured_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| project_directory.join(".gradle/gradle-user-home"))
}

pub(crate) fn resolve(
    project_directory: &Path,
    fallback_project_directory: Option<&Path>,
) -> Result<PathBuf, GradleError> {
    if env::var_os("CI").as_deref() == Some(OsStr::new("true")) {
        return resolve_path_executable("gradle")?.ok_or_else(|| {
            GradleError::compatibility(
                "CI=true but no CI-provisioned gradle executable was found in PATH.\n\
                 Install Gradle with gradle/actions/setup-gradle or expose a CI-provisioned Gradle binary.",
            )
        });
    }

    let wrapper_properties = project_directory.join("gradle/wrapper/gradle-wrapper.properties");
    let wrapper_version = read_wrapper_version(project_directory, &wrapper_properties)?;
    let distributions = project_directory.join(".gradle/wrapper/dists");
    if let Some(gradle) = find_cached_gradle(
        project_directory,
        &distributions,
        wrapper_version.as_deref(),
    )? {
        return Ok(gradle);
    }

    let fallback_distributions =
        fallback_project_directory.map(|fallback| fallback.join(".gradle/wrapper/dists"));
    if let (Some(fallback), Some(fallback_distributions)) =
        (fallback_project_directory, fallback_distributions.as_ref())
        && let Some(gradle) =
            find_cached_gradle(fallback, fallback_distributions, wrapper_version.as_deref())?
    {
        return Ok(gradle);
    }

    let displayed_version = wrapper_version.as_deref().unwrap_or("unknown");
    let mut checked_distributions = format!("  {}", distributions.display());
    if let Some(fallback_distributions) = fallback_distributions {
        checked_distributions.push_str(&format!("\n  {}", fallback_distributions.display()));
    }
    Err(GradleError::compatibility(format!(
        "No local cached Gradle distribution was found for local Android work.\n\n\
         Project wrapper version:\n  {displayed_version}\n\n\
         Checked local distributions under:\n{checked_distributions}\n\n\
         Do not use gradlew for local agent work because it may download Gradle.\n\
         Seed the project-local wrapper cache, or run in CI with setup-gradle and CI=true.\n\n\
         Project wrapper intentionally not invoked:\n  {}",
        project_directory.join("gradlew").display()
    )))
}

fn read_wrapper_version(
    containment_root: &Path,
    path: &Path,
) -> Result<Option<String>, GradleError> {
    read_wrapper_version_with_hook(containment_root, path, None)
}

fn read_wrapper_version_with_hook(
    containment_root: &Path,
    path: &Path,
    mut after_validation: Option<crate::PathIoHook<'_>>,
) -> Result<Option<String>, GradleError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(GradleError::storage(format!(
                "failed to inspect Gradle wrapper properties '{}': {error}",
                path.display()
            )));
        }
    };
    if !metadata.file_type().is_file() {
        return Err(GradleError::compatibility(format!(
            "Gradle wrapper properties '{}' is not a regular non-symlink file",
            path.display()
        )));
    }
    let canonical = path.canonicalize().map_err(|error| {
        GradleError::storage(format!(
            "failed to resolve Gradle wrapper properties '{}': {error}",
            path.display()
        ))
    })?;
    if !canonical.starts_with(containment_root) {
        return Err(GradleError::compatibility(format!(
            "Gradle wrapper properties '{}' resolves outside Android project '{}'",
            path.display(),
            containment_root.display()
        )));
    }
    if metadata.len() > MAX_GRADLE_WRAPPER_PROPERTIES_BYTES {
        return Err(GradleError::storage(format!(
            "Gradle wrapper properties '{}' exceeds {MAX_GRADLE_WRAPPER_PROPERTIES_BYTES} bytes",
            path.display()
        )));
    }
    let expected_identity = path_file_identity(path, &metadata).map_err(|error| {
        GradleError::storage(format!(
            "failed to identify Gradle wrapper properties '{}': {error}",
            path.display()
        ))
    })?;
    if let Some(hook) = after_validation.as_mut() {
        hook(path).map_err(|error| {
            GradleError::storage(format!(
                "failed to finish validating Gradle wrapper properties '{}': {error}",
                path.display()
            ))
        })?;
    }

    let mut file = File::open(path).map_err(|error| {
        GradleError::storage(format!(
            "failed to open Gradle wrapper properties '{}': {error}",
            path.display()
        ))
    })?;
    let opened_metadata = file.metadata().map_err(|error| {
        GradleError::storage(format!(
            "failed to inspect opened Gradle wrapper properties '{}': {error}",
            path.display()
        ))
    })?;
    let path_metadata = fs::symlink_metadata(path).map_err(|error| {
        GradleError::storage(format!(
            "failed to recheck Gradle wrapper properties '{}': {error}",
            path.display()
        ))
    })?;
    if !opened_metadata.is_file() || !path_metadata.file_type().is_file() {
        return Err(GradleError::compatibility(format!(
            "Gradle wrapper properties '{}' is not a regular non-symlink file",
            path.display()
        )));
    }
    let opened_identity = opened_file_identity(&file, &opened_metadata).map_err(|error| {
        GradleError::storage(format!(
            "failed to identify opened Gradle wrapper properties '{}': {error}",
            path.display()
        ))
    })?;
    let current_identity = path_file_identity(path, &path_metadata).map_err(|error| {
        GradleError::storage(format!(
            "failed to re-identify Gradle wrapper properties '{}': {error}",
            path.display()
        ))
    })?;
    if opened_identity != expected_identity || opened_identity != current_identity {
        return Err(GradleError::compatibility(format!(
            "Gradle wrapper properties '{}' changed after validation",
            path.display()
        )));
    }
    let canonical = path.canonicalize().map_err(|error| {
        GradleError::storage(format!(
            "failed to re-resolve Gradle wrapper properties '{}': {error}",
            path.display()
        ))
    })?;
    if !canonical.starts_with(containment_root) {
        return Err(GradleError::compatibility(format!(
            "Gradle wrapper properties '{}' resolves outside Android project '{}'",
            path.display(),
            containment_root.display()
        )));
    }
    if opened_metadata.len() > MAX_GRADLE_WRAPPER_PROPERTIES_BYTES {
        return Err(GradleError::storage(format!(
            "Gradle wrapper properties '{}' exceeds {MAX_GRADLE_WRAPPER_PROPERTIES_BYTES} bytes",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(opened_metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_GRADLE_WRAPPER_PROPERTIES_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            GradleError::storage(format!(
                "failed to read Gradle wrapper properties '{}': {error}",
                path.display()
            ))
        })?;
    if bytes.len() as u64 > MAX_GRADLE_WRAPPER_PROPERTIES_BYTES {
        return Err(GradleError::storage(format!(
            "Gradle wrapper properties '{}' exceeds {MAX_GRADLE_WRAPPER_PROPERTIES_BYTES} bytes",
            path.display()
        )));
    }
    let source = String::from_utf8(bytes).map_err(|error| {
        GradleError::storage(format!(
            "failed to read Gradle wrapper properties '{}': {error}",
            path.display()
        ))
    })?;
    let distribution_url = source
        .lines()
        .find_map(|line| line.strip_prefix("distributionUrl="))
        .ok_or_else(|| {
            GradleError::compatibility(format!(
                "Gradle wrapper properties '{}' does not contain distributionUrl",
                path.display()
            ))
        })?;
    parse_distribution_version(distribution_url)
        .map(Some)
        .ok_or_else(|| {
            GradleError::compatibility(format!(
                "Gradle wrapper properties '{}' contains an unsupported distributionUrl",
                path.display()
            ))
        })
}

fn parse_distribution_version(distribution_url: &str) -> Option<String> {
    let archive_with_query = distribution_url.rsplit('/').next()?;
    let archive = archive_with_query
        .split(['?', '#'])
        .next()
        .unwrap_or(archive_with_query);
    let distribution = archive.strip_prefix("gradle-")?.strip_suffix(".zip")?;
    let version = distribution
        .strip_suffix("-bin")
        .or_else(|| distribution.strip_suffix("-all"))?;
    if version.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        Some(version.to_owned())
    } else {
        None
    }
}

fn find_cached_gradle(
    containment_root: &Path,
    distributions: &Path,
    wrapper_version: Option<&str>,
) -> Result<Option<PathBuf>, GradleError> {
    match fs::symlink_metadata(distributions) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(GradleError::storage(format!(
                "failed to inspect Gradle cache directory '{}': {error}",
                distributions.display()
            )));
        }
    }
    let canonical_distributions = distributions.canonicalize().map_err(|error| {
        GradleError::storage(format!(
            "failed to resolve Gradle cache directory '{}': {error}",
            distributions.display()
        ))
    })?;
    if !canonical_distributions.starts_with(containment_root) {
        return Err(GradleError::compatibility(format!(
            "Gradle cache directory '{}' resolves outside Android project '{}'",
            distributions.display(),
            containment_root.display()
        )));
    }
    let mut candidates = Vec::new();
    let mut inspected_entries = 0;
    for distribution in regular_child_directories(&canonical_distributions, &mut inspected_entries)?
    {
        for cache_key in regular_child_directories(&distribution, &mut inspected_entries)? {
            if let Some(version) = wrapper_version {
                let installation = cache_key.join(format!("gradle-{version}"));
                if is_regular_non_symlink_directory(&installation) {
                    collect_cached_gradle_candidates(
                        &canonical_distributions,
                        &installation,
                        &mut candidates,
                    );
                }
            } else {
                for installation in regular_child_directories(&cache_key, &mut inspected_entries)? {
                    collect_cached_gradle_candidates(
                        &canonical_distributions,
                        &installation,
                        &mut candidates,
                    );
                }
            }
        }
    }
    candidates.sort();
    Ok(candidates.pop())
}

fn collect_cached_gradle_candidates(
    containment_root: &Path,
    installation: &Path,
    candidates: &mut Vec<PathBuf>,
) {
    let binary_directory = installation.join("bin");
    if !is_regular_non_symlink_directory(&binary_directory) {
        return;
    }
    for candidate in executable_candidates(&binary_directory, "gradle") {
        if let Some(canonical) = resolve_cached_gradle_executable(&candidate, containment_root) {
            candidates.push(canonical);
        }
    }
}

fn is_regular_non_symlink_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false)
}

fn regular_child_directories(
    directory: &Path,
    inspected_entries: &mut usize,
) -> Result<Vec<PathBuf>, GradleError> {
    let entries = fs::read_dir(directory).map_err(|error| {
        GradleError::storage(format!(
            "failed to inspect Gradle cache directory '{}': {error}",
            directory.display()
        ))
    })?;
    let mut directories = Vec::new();
    for entry in entries {
        *inspected_entries = inspected_entries.saturating_add(1);
        if *inspected_entries > MAX_GRADLE_CACHE_ENTRIES {
            return Err(GradleError::compatibility(format!(
                "Gradle cache contains more than {MAX_GRADLE_CACHE_ENTRIES} entries under '{}'",
                directory.display()
            )));
        }
        let entry = entry.map_err(|error| {
            GradleError::storage(format!(
                "failed to inspect an entry in Gradle cache directory '{}': {error}",
                directory.display()
            ))
        })?;
        let file_type = entry.file_type().map_err(|error| {
            GradleError::storage(format!(
                "failed to inspect Gradle cache entry '{}': {error}",
                entry.path().display()
            ))
        })?;
        if file_type.is_dir() {
            directories.push(entry.path());
        }
    }
    directories.sort();
    Ok(directories)
}

fn resolve_cached_gradle_executable(path: &Path, containment_root: &Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || !current_process_can_execute(path) {
        return None;
    }
    let canonical = path.canonicalize().ok()?;
    canonical.starts_with(containment_root).then_some(canonical)
}

fn resolve_path_executable(command: &str) -> Result<Option<PathBuf>, GradleError> {
    let Some(paths) = env::var_os("PATH") else {
        return Ok(None);
    };
    for directory in env::split_paths(&paths) {
        for candidate in executable_candidates(&directory, command) {
            let Ok(metadata) = fs::metadata(&candidate) else {
                continue;
            };
            if metadata.is_file() && current_process_can_execute(&candidate) {
                return candidate.canonicalize().map(Some).map_err(|error| {
                    GradleError::storage(format!(
                        "failed to resolve PATH executable '{}': {error}",
                        candidate.display()
                    ))
                });
            }
        }
    }
    Ok(None)
}

fn executable_candidates(directory: &Path, command: &str) -> Vec<PathBuf> {
    #[cfg(not(windows))]
    let candidates = vec![directory.join(command)];
    #[cfg(windows)]
    let candidates = {
        [".exe", ".cmd", ".bat"]
            .into_iter()
            .map(|extension| directory.join(format!("{command}{extension}")))
            .collect()
    };
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    volume_or_device: u64,
    file_index: u64,
}

#[cfg(unix)]
fn path_file_identity(_path: &Path, metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    Ok(FileIdentity {
        volume_or_device: metadata.dev(),
        file_index: metadata.ino(),
    })
}

#[cfg(unix)]
fn opened_file_identity(_file: &File, metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    path_file_identity(Path::new(""), metadata)
}

#[cfg(windows)]
fn path_file_identity(path: &Path, _metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    let handle = winapi_util::Handle::from_path_any(path)?;
    windows_handle_identity(&handle)
}

#[cfg(windows)]
fn opened_file_identity(file: &File, _metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    let handle = winapi_util::HandleRef::from_file(file);
    windows_handle_identity(&handle)
}

#[cfg(windows)]
fn windows_handle_identity<H: winapi_util::AsHandleRef>(handle: H) -> io::Result<FileIdentity> {
    let information = winapi_util::file::information(handle)?;
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

#[cfg(not(any(unix, windows)))]
fn opened_file_identity(_file: &File, _metadata: &fs::Metadata) -> io::Result<FileIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "file identity is unsupported on this host",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn service_home_defaults_to_the_android_project_and_honors_non_empty_overrides() {
        let project = Path::new("/workspace/examples/android-host");

        assert_eq!(
            service_home_with_override(project, None),
            project.join(".gradle/gradle-user-home")
        );
        assert_eq!(
            service_home_with_override(project, Some(OsStr::new(""))),
            project.join(".gradle/gradle-user-home")
        );
        assert_eq!(
            service_home_with_override(project, Some(OsStr::new("/tmp/gradle-home"))),
            PathBuf::from("/tmp/gradle-home")
        );
    }

    #[test]
    fn distribution_parser_preserves_the_complete_version_identity() {
        assert_eq!(
            parse_distribution_version(
                "https\\://services.gradle.org/distributions/gradle-9.6.0-bin.zip"
            )
            .as_deref(),
            Some("9.6.0")
        );
        assert_eq!(
            parse_distribution_version(
                "https\\://services.gradle.org/distributions/gradle-9.0-rc-1-all.zip"
            )
            .as_deref(),
            Some("9.0-rc-1")
        );
        assert_eq!(
            parse_distribution_version(
                "https\\://example.invalid/gradle-9.6.0-bin.zip?token=fixture"
            )
            .as_deref(),
            Some("9.6.0")
        );
        assert_eq!(
            parse_distribution_version(
                "https\\://services.gradle.org/distributions/not-gradle.zip"
            ),
            None
        );
        for unsafe_version in [
            "9/../../outside",
            "9\\..\\..\\outside",
            "9:outside",
            "9%2foutside",
        ] {
            assert_eq!(
                parse_distribution_version(&format!(
                    "https\\://example.invalid/gradle-{unsafe_version}-bin.zip"
                )),
                None,
                "{unsafe_version}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn wrapper_read_rejects_replacement_after_validation() {
        let directory = tempfile::tempdir().expect("temporary Gradle wrapper project");
        let project = directory
            .path()
            .canonicalize()
            .expect("resolve Gradle wrapper project");
        let wrapper_directory = project.join("gradle/wrapper");
        fs::create_dir_all(&wrapper_directory).expect("create Gradle wrapper directory");
        let wrapper = wrapper_directory.join("gradle-wrapper.properties");
        fs::write(
            &wrapper,
            b"distributionUrl=https\\://services.gradle.org/distributions/gradle-9.6.0-bin.zip\n",
        )
        .expect("write validated Gradle wrapper properties");
        let moved = wrapper_directory.join("validated-wrapper.properties");
        let mut replace_wrapper = |path: &Path| -> io::Result<()> {
            fs::rename(path, &moved)?;
            fs::write(
                path,
                b"distributionUrl=https\\://services.gradle.org/distributions/gradle-8.0-bin.zip\n",
            )
        };

        let error = read_wrapper_version_with_hook(&project, &wrapper, Some(&mut replace_wrapper))
            .expect_err("reject wrapper replacement after validation");

        assert!(error.to_string().contains("changed after validation"));
    }

    #[test]
    fn wrapper_read_bounds_growth_after_validation() {
        let directory = tempfile::tempdir().expect("temporary Gradle wrapper project");
        let project = directory
            .path()
            .canonicalize()
            .expect("resolve Gradle wrapper project");
        let wrapper_directory = project.join("gradle/wrapper");
        fs::create_dir_all(&wrapper_directory).expect("create Gradle wrapper directory");
        let wrapper = wrapper_directory.join("gradle-wrapper.properties");
        fs::write(
            &wrapper,
            b"distributionUrl=https\\://services.gradle.org/distributions/gradle-9.6.0-bin.zip\n",
        )
        .expect("write validated Gradle wrapper properties");
        let mut grow_wrapper = |path: &Path| -> io::Result<()> {
            let mut file = fs::OpenOptions::new().append(true).open(path)?;
            file.write_all(&vec![b'x'; MAX_GRADLE_WRAPPER_PROPERTIES_BYTES as usize])
        };

        let error = read_wrapper_version_with_hook(&project, &wrapper, Some(&mut grow_wrapper))
            .expect_err("reject wrapper growth after validation");

        assert!(error.to_string().contains("exceeds"));
    }
}
