use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use semver::Version;

const MAX_RELEASE_FILE_BYTES: usize = 32 * 1024 * 1024;
const MAX_RELEASE_SCAN_FILES: usize = 100_000;
const MAX_RELEASE_SCAN_ENTRIES: usize = 100_000;

const FLUTTER_PACKAGES: &[&str] = &[
    "vesper_player",
    "vesper_player_android",
    "vesper_player_external_playback",
    "vesper_player_ios",
    "vesper_player_macos",
    "vesper_player_platform_interface",
    "vesper_player_remux_ffmpeg",
    "vesper_player_source_normalizer_ffmpeg",
    "vesper_player_ui",
];

const FLUTTER_ANDROID_GRADLE_FILES: &[&str] = &[
    "lib/flutter/vesper_player_android/android/build.gradle",
    "lib/flutter/vesper_player_external_playback/android/build.gradle",
    "lib/flutter/vesper_player_source_normalizer_ffmpeg/android/build.gradle",
    "lib/flutter/vesper_player_remux_ffmpeg/android/build.gradle",
];

const FLUTTER_IOS_PACKAGE_FILES: &[&str] = &[
    "lib/flutter/vesper_player_ios/ios/vesper_player_ios/Package.swift",
    "lib/flutter/vesper_player_source_normalizer_ffmpeg/ios/vesper_player_source_normalizer_ffmpeg/Package.swift",
    "lib/flutter/vesper_player_remux_ffmpeg/ios/vesper_player_remux_ffmpeg/Package.swift",
];

const PRODUCT_CHANGELOG_FILES: &[&str] = &[
    "CHANGELOG.md",
    "lib/android/CHANGELOG.md",
    "lib/ios/VesperPlayerKit/CHANGELOG.md",
];

const FLUTTER_CHANGELOG_FILES: &[&str] = &[
    "lib/flutter/vesper_player/CHANGELOG.md",
    "lib/flutter/vesper_player_android/CHANGELOG.md",
    "lib/flutter/vesper_player_external_playback/CHANGELOG.md",
    "lib/flutter/vesper_player_platform_interface/CHANGELOG.md",
    "lib/flutter/vesper_player_ios/CHANGELOG.md",
    "lib/flutter/vesper_player_ui/CHANGELOG.md",
    "lib/flutter/vesper_player_source_normalizer_ffmpeg/CHANGELOG.md",
    "lib/flutter/vesper_player_remux_ffmpeg/CHANGELOG.md",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseErrorKind {
    Input,
    Storage,
    Verification,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseError {
    kind: ReleaseErrorKind,
    message: String,
}

impl ReleaseError {
    pub(crate) fn input(message: impl Into<String>) -> Self {
        Self {
            kind: ReleaseErrorKind::Input,
            message: message.into(),
        }
    }

    pub(crate) fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: ReleaseErrorKind::Storage,
            message: message.into(),
        }
    }

    fn verification(message: impl Into<String>) -> Self {
        Self {
            kind: ReleaseErrorKind::Verification,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> ReleaseErrorKind {
        self.kind
    }
}

impl fmt::Display for ReleaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ReleaseError {}

pub type ReleaseResult<T> = Result<T, ReleaseError>;

#[derive(Debug, Clone, Default)]
pub struct ReleaseEnvironment {
    release_ios_build: Option<String>,
    release_build: Option<String>,
    release_android_version_code: Option<String>,
    pub(crate) github_ref_name: Option<String>,
    pub(crate) github_server_url: Option<String>,
    pub(crate) github_repository: Option<String>,
    github_output: Option<PathBuf>,
    github_env: Option<PathBuf>,
}

impl ReleaseEnvironment {
    pub fn from_process() -> Self {
        Self {
            release_ios_build: nonempty_env("VESPER_RELEASE_IOS_BUILD"),
            release_build: nonempty_env("VESPER_RELEASE_BUILD"),
            release_android_version_code: nonempty_env("VESPER_RELEASE_ANDROID_VERSION_CODE"),
            github_ref_name: nonempty_env("GITHUB_REF_NAME"),
            github_server_url: nonempty_env("GITHUB_SERVER_URL"),
            github_repository: nonempty_env("GITHUB_REPOSITORY"),
            github_output: env::var_os("GITHUB_OUTPUT")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            github_env: env::var_os("GITHUB_ENV")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_release_values(
        ios_build: Option<&str>,
        legacy_build: Option<&str>,
        android_version_code: Option<&str>,
    ) -> Self {
        Self {
            release_ios_build: ios_build.map(str::to_owned),
            release_build: legacy_build.map(str::to_owned),
            release_android_version_code: android_version_code.map(str::to_owned),
            ..Self::default()
        }
    }
}

fn nonempty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, Default)]
pub struct ReleaseMetadataOptions {
    pub ios_build: Option<String>,
    pub android_version_code: Option<String>,
    pub release_date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseMetadata {
    version: ReleaseVersion,
    publication_version: String,
    ios_build: String,
    android_version_code: String,
    release_date: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseChannel {
    version: String,
    publication_version: String,
    stable: bool,
}

impl ReleaseChannel {
    pub fn output(&self) -> String {
        format!(
            "version={}\npublication_version={}\nstable={}\nprerelease={}\n",
            self.version, self.publication_version, self.stable, !self.stable
        )
    }
}

impl ReleaseMetadata {
    pub fn output(&self) -> String {
        format!(
            "version={}\npublication_version={}\nios_build={}\nandroid_version_code={}\nrelease_date={}\n",
            self.version,
            self.publication_version,
            self.ios_build,
            self.android_version_code,
            self.release_date
        )
    }

    pub fn version(&self) -> &str {
        self.version.as_str()
    }

    pub fn publication_version(&self) -> &str {
        &self.publication_version
    }

    pub fn ios_build(&self) -> &str {
        &self.ios_build
    }

    pub fn android_version_code(&self) -> &str {
        &self.android_version_code
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseVersion {
    value: String,
    major: u64,
    minor: u64,
    patch: u64,
}

impl ReleaseVersion {
    fn parse(value: &str) -> ReleaseResult<Self> {
        let components = value.split('.').collect::<Vec<_>>();
        if components.len() != 3
            || components.iter().any(|component| {
                component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit())
            })
        {
            return Err(ReleaseError::input(format!(
                "Version must be numeric major.minor.patch: {value}"
            )));
        }
        let major = parse_version_component(components[0], value)?;
        let minor = parse_version_component(components[1], value)?;
        let patch = parse_version_component(components[2], value)?;
        if minor > 99 || patch > 99 {
            return Err(ReleaseError::input(format!(
                "Release versions require minor and patch in 0..99: {value}"
            )));
        }
        Ok(Self {
            value: value.to_owned(),
            major,
            minor,
            patch,
        })
    }

    #[cfg(test)]
    fn from_tag(tag: &str) -> ReleaseResult<Self> {
        let without_ref = tag.strip_prefix("refs/tags/").unwrap_or(tag);
        let without_v = without_ref.strip_prefix('v').unwrap_or(without_ref);
        let expression =
            compile_regex(r"^([0-9]+)\.([0-9]+)\.([0-9]+)(?:[-+].*)?$", "release tag")?;
        let captures = expression.captures(without_v).ok_or_else(|| {
            ReleaseError::input(format!(
                "Release tag must look like vMAJOR.MINOR.PATCH or vMAJOR.MINOR.PATCH-rc.N: {tag}"
            ))
        })?;
        let major = capture_text(&captures, 1, "release tag major")?;
        let minor = capture_text(&captures, 2, "release tag minor")?;
        let patch = capture_text(&captures, 3, "release tag patch")?;
        Self::parse(&format!("{major}.{minor}.{patch}"))
    }

    fn android_code(&self) -> ReleaseResult<String> {
        let value = self
            .major
            .checked_mul(10_000)
            .and_then(|value| {
                self.minor
                    .checked_mul(100)
                    .and_then(|minor| value.checked_add(minor))
            })
            .and_then(|value| value.checked_add(self.patch))
            .ok_or_else(|| {
                ReleaseError::input(format!(
                    "Android versionCode overflows for release version {}",
                    self.value
                ))
            })?;
        Ok(value.to_string())
    }

    fn as_str(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

fn parse_version_component(component: &str, version: &str) -> ReleaseResult<u64> {
    component.parse::<u64>().map_err(|error| {
        ReleaseError::input(format!(
            "Version component is too large in {version}: {error}"
        ))
    })
}

#[cfg(test)]
fn capture_text<'a>(
    captures: &'a regex::Captures<'_>,
    index: usize,
    label: &str,
) -> ReleaseResult<&'a str> {
    captures
        .get(index)
        .map(|capture| capture.as_str())
        .ok_or_else(|| ReleaseError::input(format!("Unable to resolve {label}.")))
}

pub struct ReleaseContext {
    root: PathBuf,
    environment: ReleaseEnvironment,
}

impl ReleaseContext {
    pub fn new(root: PathBuf, environment: ReleaseEnvironment) -> Self {
        Self { root, environment }
    }

    pub fn default_notes_tag(&self) -> Option<&str> {
        self.environment.github_ref_name.as_deref()
    }

    #[cfg(test)]
    pub fn stable_version_from_tag(tag: &str) -> ReleaseResult<String> {
        let channel = Self::channel_from_tag(tag)?;
        if !channel.stable {
            return Err(ReleaseError::input(format!(
                "Stable publication requires an exact vMAJOR.MINOR.PATCH tag: {tag}"
            )));
        }
        Ok(channel.version)
    }

    pub fn publication_version_from_tag(tag: &str) -> ReleaseResult<String> {
        Ok(Self::channel_from_tag(tag)?.publication_version)
    }

    pub fn channel_from_tag(tag: &str) -> ReleaseResult<ReleaseChannel> {
        let value = tag.strip_prefix('v').ok_or_else(|| {
            ReleaseError::input(format!(
                "Release tag must use a v prefix and valid SemVer: {tag}"
            ))
        })?;
        let parsed = Version::parse(value).map_err(|error| {
            ReleaseError::input(format!("Release tag is not valid SemVer: {tag}: {error}"))
        })?;
        if !parsed.build.is_empty() {
            return Err(ReleaseError::input(format!(
                "Release tags must not contain build metadata: {tag}"
            )));
        }
        let version = ReleaseVersion::parse(&format!(
            "{}.{}.{}",
            parsed.major, parsed.minor, parsed.patch
        ))?;
        Ok(ReleaseChannel {
            version: version.value,
            publication_version: parsed.to_string(),
            stable: parsed.pre.is_empty(),
        })
    }

    pub fn metadata_for_version(
        &self,
        version: &str,
        options: ReleaseMetadataOptions,
    ) -> ReleaseResult<ReleaseMetadata> {
        let version = ReleaseVersion::parse(version)?;
        let publication_version = version.value.clone();
        self.resolve_metadata(version, publication_version, options, utc_today()?)
    }

    pub fn metadata_from_tag(
        &self,
        tag: &str,
        options: ReleaseMetadataOptions,
    ) -> ReleaseResult<ReleaseMetadata> {
        let channel = Self::channel_from_tag(tag.strip_prefix("refs/tags/").unwrap_or(tag))?;
        let version = ReleaseVersion::parse(&channel.version)?;
        let default_date = if options
            .release_date
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        {
            String::new()
        } else {
            release_date_from_tag(&self.root, tag)?
        };
        self.resolve_metadata(version, channel.publication_version, options, default_date)
    }

    fn resolve_metadata(
        &self,
        version: ReleaseVersion,
        publication_version: String,
        options: ReleaseMetadataOptions,
        default_date: String,
    ) -> ReleaseResult<ReleaseMetadata> {
        let android_version_code = nonempty(options.android_version_code)
            .or_else(|| self.environment.release_android_version_code.clone())
            .map(Ok)
            .unwrap_or_else(|| version.android_code())?;
        let ios_build = nonempty(options.ios_build)
            .or_else(|| self.environment.release_ios_build.clone())
            .or_else(|| self.environment.release_build.clone())
            .unwrap_or_else(|| android_version_code.clone());
        let release_date = nonempty(options.release_date).unwrap_or(default_date);
        validate_numeric_metadata("iOS build", &ios_build)?;
        validate_numeric_metadata("Android versionCode", &android_version_code)?;
        validate_release_date(&release_date)?;
        Ok(ReleaseMetadata {
            version,
            publication_version,
            ios_build,
            android_version_code,
            release_date,
        })
    }

    pub fn append_ci_metadata(&self, metadata: &ReleaseMetadata) -> ReleaseResult<()> {
        if let Some(path) = &self.environment.github_output {
            append_file(path, metadata.output().as_bytes(), "GITHUB_OUTPUT")?;
        }
        if let Some(path) = &self.environment.github_env {
            let contents = format!(
                "VESPER_RELEASE_VERSION={}\nVESPER_RELEASE_PUBLICATION_VERSION={}\nVESPER_RELEASE_BUILD={}\nVESPER_RELEASE_IOS_BUILD={}\nVESPER_RELEASE_ANDROID_VERSION_CODE={}\nVESPER_RELEASE_DATE={}\n",
                metadata.version,
                metadata.publication_version,
                metadata.ios_build(),
                metadata.ios_build(),
                metadata.android_version_code(),
                metadata.release_date
            );
            append_file(path, contents.as_bytes(), "GITHUB_ENV")?;
        }
        Ok(())
    }

    pub fn set_version(&self, metadata: &ReleaseMetadata) -> ReleaseResult<()> {
        let mut plan = EditPlan::default();
        let version = metadata.version.as_str();
        let publication_version = metadata.publication_version();

        let cargo_path = self.root.join("Cargo.toml");
        let cargo = read_required_text(&cargo_path)?;
        validate_toml(&cargo_path, &cargo)?;
        let cargo = replace_workspace_package_version(&cargo, version)?;
        validate_toml(&cargo_path, &cargo)?;
        plan.insert(cargo_path, cargo)?;

        let lock_path = self.root.join("Cargo.lock");
        if lock_path.is_file() {
            let lock = read_required_text(&lock_path)?;
            validate_toml(&lock_path, &lock)?;
            let lock = replace_cargo_lock_versions(&lock, version)?;
            validate_toml(&lock_path, &lock)?;
            plan.insert(lock_path, lock)?;
        }

        for manifest in collect_named_files(&self.root.join("crates"), "Cargo.toml")? {
            let source = read_required_text(&manifest)?;
            let updated = replace_path_dependency_versions(&source, version)?;
            if updated != source {
                validate_toml(&manifest, &updated)?;
                plan.insert(manifest, updated)?;
            }
        }

        let plugin_host_sdk = plugin_host_sdk_requirement(&metadata.version)?;
        let plugin_manifests = collect_plugin_manifests(&self.root)?;
        for manifest in plugin_manifests {
            let source = read_required_text(&manifest)?;
            validate_toml(&manifest, &source)?;
            let updated = replace_toml_section_string(
                &source,
                "plugin",
                "version",
                version,
                "plugin manifest version",
            )?;
            let updated = replace_toml_section_string(
                &updated,
                "compatibility",
                "host_sdk",
                &plugin_host_sdk,
                "plugin host SDK requirement",
            )?;
            validate_toml(&manifest, &updated)?;
            plan.insert(manifest, updated)?;
        }

        update_required_line(
            &mut plan,
            &self.root.join("lib/android/build.gradle.kts"),
            r#"^val vesperDefaultPublicationVersion = \"[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?\"$"#,
            &format!("val vesperDefaultPublicationVersion = \"{publication_version}\""),
            "Android library version",
        )?;

        for pubspec in collect_flutter_pubspecs(&self.root)? {
            let source = read_required_text(&pubspec)?;
            let updated = replace_pubspec_versions(&source, publication_version)?;
            plan.insert(pubspec, updated)?;
        }

        for relative in FLUTTER_ANDROID_GRADLE_FILES {
            update_required_line(
                &mut plan,
                &self.root.join(relative),
                r#"^version = \"[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?\"$"#,
                &format!("version = \"{publication_version}\""),
                "Flutter Android plugin version",
            )?;
        }

        for relative in FLUTTER_IOS_PACKAGE_FILES {
            update_required_line(
                &mut plan,
                &self.root.join(relative),
                r#"^private let vesperPlayerKitVersion: Version = \"[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?\"$"#,
                &format!("private let vesperPlayerKitVersion: Version = \"{publication_version}\""),
                "Flutter iOS native dependency version",
            )?;
        }

        let ios_project = self.root.join("lib/ios/VesperPlayerKit/project.yml");
        let source = read_required_text(&ios_project)?;
        let source = replace_line_exactly_once(
            &source,
            r#"^        CFBundleShortVersionString: \"[0-9]+\.[0-9]+\.[0-9]+\"$"#,
            &format!("        CFBundleShortVersionString: \"{version}\""),
            "iOS marketing version",
        )?;
        let source = replace_line_exactly_once(
            &source,
            r#"^        CFBundleVersion: \"[0-9]+\"$"#,
            &format!("        CFBundleVersion: \"{}\"", metadata.ios_build),
            "iOS build version",
        )?;
        plan.insert(ios_project, source)?;

        let info_plist = self
            .root
            .join("lib/ios/VesperPlayerKit/Sources/Generated-Info.plist");
        let source = read_required_text(&info_plist)?;
        let source = replace_plist_value(
            &source,
            "CFBundleShortVersionString",
            version,
            "iOS generated marketing version",
        )?;
        let source = replace_plist_value(
            &source,
            "CFBundleVersion",
            &metadata.ios_build,
            "iOS generated build version",
        )?;
        plan.insert(info_plist, source)?;

        let android_sample = self
            .root
            .join("examples/android-compose-host/app/build.gradle.kts");
        let source = read_required_text(&android_sample)?;
        let source = replace_line_exactly_once(
            &source,
            r"^        versionCode = [0-9]+$",
            &format!("        versionCode = {}", metadata.android_version_code),
            "Android sample versionCode",
        )?;
        let source = replace_line_exactly_once(
            &source,
            r#"^        versionName = \"[0-9]+\.[0-9]+\.[0-9]+\"$"#,
            &format!("        versionName = \"{version}\""),
            "Android sample versionName",
        )?;
        plan.insert(android_sample, source)?;

        update_required_line(
            &mut plan,
            &self.root.join("examples/flutter-host/pubspec.yaml"),
            r"^version: [0-9]+\.[0-9]+\.[0-9]+\+[0-9]+$",
            &format!("version: {version}+{}", metadata.android_version_code),
            "Flutter host version",
        )?;

        for relative in PRODUCT_CHANGELOG_FILES {
            let path = self.root.join(relative);
            if path.is_file() {
                let source = read_required_text(&path)?;
                let updated = update_changelog(&source, version, &metadata.release_date)?;
                plan.insert(path, updated)?;
            }
        }
        for relative in FLUTTER_CHANGELOG_FILES {
            let path = self.root.join(relative);
            if path.is_file() {
                let source = read_required_text(&path)?;
                let updated =
                    update_changelog(&source, publication_version, &metadata.release_date)?;
                plan.insert(path, updated)?;
            }
        }

        plan.commit()
    }

    pub fn verify_version(
        &self,
        version: &str,
        ios_build: Option<String>,
        android_version_code: Option<String>,
    ) -> ReleaseResult<()> {
        let version = ReleaseVersion::parse(version)?;
        let ios_build = match nonempty(ios_build) {
            Some(value) => value,
            None => read_ios_build(&self.root)?,
        };
        let android_version_code = match nonempty(android_version_code) {
            Some(value) => value,
            None => read_android_version_code(&self.root)?,
        };
        validate_numeric_metadata("iOS build", &ios_build)?;
        validate_numeric_metadata("Android versionCode", &android_version_code)?;
        let issues = verify_product_version(
            &self.root,
            &version,
            version.as_str(),
            &ios_build,
            &android_version_code,
        )?;
        if issues.is_empty() {
            return Ok(());
        }
        let mut message = issues.join("\n");
        message.push_str(&format!(
            "\nVersion verification failed with {} issue(s).",
            issues.len()
        ));
        Err(ReleaseError::verification(message))
    }

    pub fn verify_metadata(&self, metadata: &ReleaseMetadata) -> ReleaseResult<()> {
        let issues = verify_product_version(
            &self.root,
            &metadata.version,
            metadata.publication_version(),
            metadata.ios_build(),
            metadata.android_version_code(),
        )?;
        if issues.is_empty() {
            return Ok(());
        }
        let mut message = issues.join("\n");
        message.push_str(&format!(
            "\nVersion verification failed with {} issue(s).",
            issues.len()
        ));
        Err(ReleaseError::verification(message))
    }

    pub fn verify_current(&self) -> ReleaseResult<String> {
        let version = read_workspace_version(&self.root)?;
        let parsed = ReleaseVersion::parse(&version)?;
        let publication_version = read_android_publication_version(&self.root)?;
        let channel = Self::channel_from_tag(&format!("v{publication_version}"))?;
        if channel.version != version {
            return Err(ReleaseError::verification(format!(
                "Publication version base mismatch.\n  expected {version}, found {publication_version}"
            )));
        }
        let expected_build = parsed.android_code()?;
        let issues = verify_product_version(
            &self.root,
            &parsed,
            &publication_version,
            &expected_build,
            &expected_build,
        )?;
        if !issues.is_empty() {
            let mut message = issues.join("\n");
            message.push_str(&format!(
                "\nVersion verification failed with {} issue(s).",
                issues.len()
            ));
            return Err(ReleaseError::verification(message));
        }
        Ok(version)
    }

    pub fn generate_notes(&self, tag: &str, output: Option<&Path>) -> ReleaseResult<PathBuf> {
        crate::release_notes::generate(&self.root, &self.environment, tag, output)
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn validate_numeric_metadata(label: &str, value: &str) -> ReleaseResult<()> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ReleaseError::input(format!(
            "{label} must be numeric: {value}"
        )));
    }
    Ok(())
}

fn validate_release_date(value: &str) -> ReleaseResult<()> {
    let expression = compile_regex(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$", "release date")?;
    if !expression.is_match(value) {
        return Err(ReleaseError::input(format!(
            "Release date must be YYYY-MM-DD: {value}"
        )));
    }
    Ok(())
}

fn append_file(path: &Path, bytes: &[u8], label: &str) -> ReleaseResult<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| {
            ReleaseError::storage(format!(
                "failed to open {label} '{}': {error}",
                path.display()
            ))
        })?;
    file.write_all(bytes).map_err(|error| {
        ReleaseError::storage(format!(
            "failed to append {label} '{}': {error}",
            path.display()
        ))
    })
}

fn release_date_from_tag(root: &Path, tag: &str) -> ReleaseResult<String> {
    let ref_name = tag.strip_prefix("refs/tags/").unwrap_or(tag);
    if let Some(value) = git_output_optional(
        root,
        &[
            "for-each-ref",
            &format!("refs/tags/{ref_name}"),
            "--format=%(taggerdate:short)",
        ],
    )? && is_release_date(&value)
    {
        return Ok(value);
    }
    let commit_ref = format!("{tag}^{{commit}}");
    if let Some(value) = git_output_optional(
        root,
        &[
            "log",
            "-1",
            "--format=%cd",
            "--date=format:%Y-%m-%d",
            &commit_ref,
        ],
    )? && is_release_date(&value)
    {
        return Ok(value);
    }
    utc_today()
}

fn is_release_date(value: &str) -> bool {
    value.len() == 10
        && value.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

pub(crate) fn git_output(root: &Path, arguments: &[&str]) -> ReleaseResult<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(|error| ReleaseError::storage(format!("failed to run git: {error}")))?;
    if !output.status.success() {
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        return Err(ReleaseError::input(format!(
            "git {} failed: {}",
            arguments.join(" "),
            diagnostic.trim()
        )));
    }
    let value = String::from_utf8(output.stdout)
        .map_err(|error| ReleaseError::storage(format!("git output is not UTF-8: {error}")))?;
    Ok(value.trim_end_matches(['\r', '\n']).to_owned())
}

pub(crate) fn git_output_optional(
    root: &Path,
    arguments: &[&str],
) -> ReleaseResult<Option<String>> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(|error| ReleaseError::storage(format!("failed to run git: {error}")))?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8(output.stdout)
        .map_err(|error| ReleaseError::storage(format!("git output is not UTF-8: {error}")))?;
    Ok(Some(value.trim_end_matches(['\r', '\n']).to_owned()))
}

fn utc_today() -> ReleaseResult<String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            ReleaseError::storage(format!("system clock is before the Unix epoch: {error}"))
        })?;
    let days = i64::try_from(duration.as_secs() / 86_400)
        .map_err(|error| ReleaseError::storage(format!("UTC date is out of range: {error}")))?;
    let (year, month, day) = civil_from_days(days);
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let shifted = days_since_epoch + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

#[derive(Default)]
struct EditPlan {
    edits: BTreeMap<PathBuf, PlannedEdit>,
}

struct PlannedEdit {
    original: Vec<u8>,
    updated: Vec<u8>,
    permissions: Permissions,
}

impl EditPlan {
    fn insert(&mut self, path: PathBuf, updated: String) -> ReleaseResult<()> {
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            ReleaseError::storage(format!("failed to inspect '{}': {error}", path.display()))
        })?;
        if !metadata.file_type().is_file() {
            return Err(ReleaseError::storage(format!(
                "release metadata path '{}' is not a regular non-symlink file",
                path.display()
            )));
        }
        let original = fs::read(&path).map_err(|error| {
            ReleaseError::storage(format!("failed to read '{}': {error}", path.display()))
        })?;
        let updated = updated.into_bytes();
        if original != updated {
            self.edits.insert(
                path,
                PlannedEdit {
                    original,
                    updated,
                    permissions: metadata.permissions(),
                },
            );
        }
        Ok(())
    }

    fn commit(self) -> ReleaseResult<()> {
        let mut staged = Vec::with_capacity(self.edits.len());
        for (path, edit) in self.edits {
            let parent = parent_directory(&path);
            let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
                ReleaseError::storage(format!(
                    "failed to stage release metadata '{}': {error}",
                    path.display()
                ))
            })?;
            temporary
                .as_file()
                .set_permissions(edit.permissions.clone())
                .and_then(|()| temporary.write_all(&edit.updated))
                .and_then(|()| temporary.as_file().sync_all())
                .map_err(|error| {
                    ReleaseError::storage(format!(
                        "failed to write staged release metadata '{}': {error}",
                        path.display()
                    ))
                })?;
            staged.push((path, edit.original, edit.permissions, temporary));
        }

        let mut committed = Vec::new();
        for (path, original, permissions, temporary) in staged {
            if let Err(error) = temporary.persist(&path) {
                let rollback = rollback_edits(&committed);
                return Err(commit_error(&path, &error.error, rollback));
            }
            if let Err(error) = sync_parent_directory(&path) {
                committed.push((path.clone(), original, permissions));
                let rollback = rollback_edits(&committed);
                return Err(commit_error(&path, &error, rollback));
            }
            committed.push((path, original, permissions));
        }
        Ok(())
    }
}

fn rollback_edits(committed: &[(PathBuf, Vec<u8>, Permissions)]) -> Vec<String> {
    let mut failures = Vec::new();
    for (path, original, permissions) in committed.iter().rev() {
        if let Err(error) = atomic_replace(path, original, permissions.clone()) {
            failures.push(format!("{}: {error}", path.display()));
        }
    }
    failures
}

fn commit_error(path: &Path, error: &dyn fmt::Display, rollback: Vec<String>) -> ReleaseError {
    let mut message = format!(
        "failed to atomically replace release metadata '{}': {error}",
        path.display()
    );
    if !rollback.is_empty() {
        message.push_str("\nrollback also failed for:\n  ");
        message.push_str(&rollback.join("\n  "));
    }
    ReleaseError::storage(message)
}

fn atomic_replace(path: &Path, bytes: &[u8], permissions: Permissions) -> std::io::Result<()> {
    let parent = parent_directory(path);
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.as_file().set_permissions(permissions)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    sync_parent_directory(path)
}

pub(crate) fn atomic_write_output(path: &Path, bytes: &[u8]) -> ReleaseResult<()> {
    let parent = parent_directory(path);
    fs::create_dir_all(parent).map_err(|error| {
        ReleaseError::storage(format!(
            "failed to create release output directory '{}': {error}",
            parent.display()
        ))
    })?;
    let permissions = fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| metadata.file_type().is_file())
        .map(|metadata| metadata.permissions());
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        ReleaseError::storage(format!(
            "failed to stage release output '{}': {error}",
            path.display()
        ))
    })?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions)
            .map_err(|error| {
                ReleaseError::storage(format!(
                    "failed to preserve release output permissions '{}': {error}",
                    path.display()
                ))
            })?;
    }
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| {
            ReleaseError::storage(format!(
                "failed to write staged release output '{}': {error}",
                path.display()
            ))
        })?;
    temporary.persist(path).map_err(|error| {
        ReleaseError::storage(format!(
            "failed to atomically replace release output '{}': {}",
            path.display(),
            error.error
        ))
    })?;
    sync_parent_directory(path).map_err(|error| {
        ReleaseError::storage(format!(
            "failed to sync release output directory '{}': {error}",
            parent.display()
        ))
    })
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> std::io::Result<()> {
    File::open(parent_directory(path))?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn update_required_line(
    plan: &mut EditPlan,
    path: &Path,
    pattern: &str,
    replacement: &str,
    label: &str,
) -> ReleaseResult<()> {
    let source = read_required_text(path)?;
    let updated = replace_line_exactly_once(&source, pattern, replacement, label)?;
    plan.insert(path.to_path_buf(), updated)
}

fn read_required_text(path: &Path) -> ReleaseResult<String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        ReleaseError::storage(format!("failed to inspect '{}': {error}", path.display()))
    })?;
    if !metadata.file_type().is_file() {
        return Err(ReleaseError::storage(format!(
            "release metadata path '{}' is not a regular non-symlink file",
            path.display()
        )));
    }
    if metadata.len() > MAX_RELEASE_FILE_BYTES as u64 {
        return Err(ReleaseError::storage(format!(
            "release metadata '{}' exceeds {MAX_RELEASE_FILE_BYTES} bytes",
            path.display()
        )));
    }
    let file = File::open(path).map_err(|error| {
        ReleaseError::storage(format!("failed to open '{}': {error}", path.display()))
    })?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_RELEASE_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            ReleaseError::storage(format!("failed to read '{}': {error}", path.display()))
        })?;
    if bytes.len() > MAX_RELEASE_FILE_BYTES {
        return Err(ReleaseError::storage(format!(
            "release metadata '{}' exceeds {MAX_RELEASE_FILE_BYTES} bytes",
            path.display()
        )));
    }
    String::from_utf8(bytes).map_err(|error| {
        ReleaseError::storage(format!(
            "release metadata '{}' is not UTF-8: {error}",
            path.display()
        ))
    })
}

fn validate_toml(path: &Path, source: &str) -> ReleaseResult<()> {
    toml::from_str::<toml::Value>(source)
        .map(|_| ())
        .map_err(|error| {
            ReleaseError::storage(format!("invalid TOML in '{}': {error}", path.display()))
        })
}

fn compile_regex(pattern: &str, label: &str) -> ReleaseResult<Regex> {
    Regex::new(pattern).map_err(|error| {
        ReleaseError::storage(format!("invalid internal {label} expression: {error}"))
    })
}

fn replace_workspace_package_version(source: &str, version: &str) -> ReleaseResult<String> {
    let mut in_section = false;
    let mut replacements = 0;
    let output = map_lines(source, |line| {
        if line == "[workspace.package]" {
            in_section = true;
            return None;
        }
        if in_section && line.starts_with('[') {
            in_section = false;
        }
        if in_section && line.starts_with("version = \"") {
            replacements += 1;
            return Some(format!("version = \"{version}\""));
        }
        None
    });
    require_replacement_count(replacements, 1, "Cargo workspace version")?;
    Ok(output)
}

fn replace_toml_section_string(
    source: &str,
    section: &str,
    key: &str,
    value: &str,
    label: &str,
) -> ReleaseResult<String> {
    let section_header = format!("[{section}]");
    let key_prefix = format!("{key} = \"");
    let mut in_section = false;
    let mut replacements = 0;
    let output = map_lines(source, |line| {
        if line == section_header {
            in_section = true;
            return None;
        }
        if in_section && line.starts_with('[') {
            in_section = false;
        }
        if in_section && line.starts_with(&key_prefix) {
            replacements += 1;
            return Some(format!("{key} = \"{value}\""));
        }
        None
    });
    require_replacement_count(replacements, 1, label)?;
    Ok(output)
}

fn replace_cargo_lock_versions(source: &str, version: &str) -> ReleaseResult<String> {
    let name_expression = compile_regex(r#"^name = \"([^\"]+)\"$"#, "Cargo.lock package name")?;
    let version_expression =
        compile_regex(r#"^version = \"[^\"]+\"$"#, "Cargo.lock package version")?;
    let mut current_name: Option<String> = None;
    let mut replacements = 0;
    let output = map_lines(source, |line| {
        if line == "[[package]]" {
            current_name = None;
            return None;
        }
        if let Some(captures) = name_expression.captures(line) {
            current_name = captures.get(1).map(|capture| capture.as_str().to_owned());
            return None;
        }
        if version_expression.is_match(line)
            && current_name
                .as_deref()
                .is_some_and(|name| name == "basic-player" || name.starts_with("player-"))
        {
            replacements += 1;
            return Some(format!("version = \"{version}\""));
        }
        None
    });
    if replacements == 0 {
        return Err(ReleaseError::storage(
            "Cargo.lock does not contain Vesper workspace packages",
        ));
    }
    Ok(output)
}

fn replace_path_dependency_versions(source: &str, version: &str) -> ReleaseResult<String> {
    let dependency = compile_regex(
        r#"^(player-[A-Za-z0-9_-]+ = \{[^\r\n]*version = \"?)([0-9]+\.[0-9]+\.[0-9]+)(\"?[^\r\n]*path = [^\r\n]*\})$"#,
        "Cargo path dependency version",
    )?;
    Ok(map_lines(source, |line| {
        let captures = dependency.captures(line)?;
        let prefix = captures.get(1)?.as_str();
        let suffix = captures.get(3)?.as_str();
        Some(format!("{prefix}{version}{suffix}"))
    }))
}

fn replace_pubspec_versions(source: &str, version: &str) -> ReleaseResult<String> {
    let package_version = compile_regex(
        r"^version: [0-9]+\.[0-9]+\.[0-9]+(?:[+-][A-Za-z0-9.-]+)?$",
        "Flutter package version",
    )?;
    let dependency_names = FLUTTER_PACKAGES
        .iter()
        .map(|name| regex::escape(name))
        .collect::<Vec<_>>()
        .join("|");
    let dependency = compile_regex(
        &format!(r"^  ({dependency_names}): \^[0-9]+\.[0-9]+\.[0-9]+(?:[+-][A-Za-z0-9.-]+)?$"),
        "Flutter package dependency version",
    )?;
    let mut version_replacements = 0;
    let output = map_lines(source, |line| {
        if package_version.is_match(line) {
            version_replacements += 1;
            return Some(format!("version: {version}"));
        }
        dependency.captures(line).and_then(|captures| {
            captures
                .get(1)
                .map(|name| format!("  {}: ^{version}", name.as_str()))
        })
    });
    require_replacement_count(version_replacements, 1, "Flutter package version")?;
    Ok(output)
}

fn replace_plist_value(source: &str, key: &str, value: &str, label: &str) -> ReleaseResult<String> {
    let expression = compile_regex(
        &format!(
            r"(?m)(^\s*<key>{}</key>\r?\n\s*<string>)[^<]+(</string>$)",
            regex::escape(key)
        ),
        label,
    )?;
    let count = expression.find_iter(source).count();
    require_replacement_count(count, 1, label)?;
    Ok(expression
        .replace(source, |captures: &regex::Captures<'_>| {
            format!(
                "{}{}{}",
                captures.get(1).map_or("", |capture| capture.as_str()),
                value,
                captures.get(2).map_or("", |capture| capture.as_str())
            )
        })
        .into_owned())
}

fn replace_line_exactly_once(
    source: &str,
    pattern: &str,
    replacement: &str,
    label: &str,
) -> ReleaseResult<String> {
    let expression = compile_regex(pattern, label)?;
    let mut replacements = 0;
    let output = map_lines(source, |line| {
        if expression.is_match(line) {
            replacements += 1;
            return Some(replacement.to_owned());
        }
        None
    });
    require_replacement_count(replacements, 1, label)?;
    Ok(output)
}

fn require_replacement_count(count: usize, expected: usize, label: &str) -> ReleaseResult<()> {
    if count != expected {
        return Err(ReleaseError::storage(format!(
            "expected {expected} {label} field(s), found {count}"
        )));
    }
    Ok(())
}

fn map_lines(mut source: &str, mut replace: impl FnMut(&str) -> Option<String>) -> String {
    let mut output = String::with_capacity(source.len());
    while !source.is_empty() {
        let (line_with_ending, remaining) = match source.find('\n') {
            Some(index) => source.split_at(index + 1),
            None => (source, ""),
        };
        source = remaining;
        let (line, ending) = if let Some(line) = line_with_ending.strip_suffix("\r\n") {
            (line, "\r\n")
        } else if let Some(line) = line_with_ending.strip_suffix('\n') {
            (line, "\n")
        } else {
            (line_with_ending, "")
        };
        if let Some(replacement) = replace(line) {
            output.push_str(&replacement);
        } else {
            output.push_str(line);
        }
        output.push_str(ending);
    }
    output
}

fn update_changelog(source: &str, version: &str, release_date: &str) -> ReleaseResult<String> {
    let version_heading_prefix = format!("## {version} - ");
    let exact = compile_regex(
        &format!(
            r"(?m)^## {} - (?:Unreleased|[0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}})$",
            regex::escape(version)
        ),
        "changelog version heading",
    )?;
    let heading = format!("## {version} - {release_date}");
    let updated = if exact.is_match(source) {
        exact.replace_all(source, heading.as_str()).into_owned()
    } else if source
        .lines()
        .any(|line| line.starts_with(&version_heading_prefix))
    {
        return Err(ReleaseError::storage(format!(
            "Unable to update changelog heading for {version}."
        )));
    } else {
        let unreleased = compile_regex(
            r"(?m)^## [0-9]+\.[0-9]+\.[0-9]+ - Unreleased$",
            "unreleased changelog heading",
        )?;
        if unreleased.is_match(source) {
            unreleased.replace(source, heading.as_str()).into_owned()
        } else if let Some(rest) = source.strip_prefix("# Changelog\n\n") {
            format!(
                "# Changelog\n\n{heading}\n\n- Prepared package metadata for the {version} release.\n\n{rest}"
            )
        } else {
            format!(
                "# Changelog\n\n{heading}\n\n- Prepared package metadata for the {version} release.\n\n{source}"
            )
        }
    };
    if !updated.lines().any(|line| line == heading) {
        return Err(ReleaseError::storage(format!(
            "Unable to update changelog heading for {version}."
        )));
    }
    Ok(updated)
}

fn collect_flutter_pubspecs(root: &Path) -> ReleaseResult<Vec<PathBuf>> {
    let directory = root.join("lib/flutter");
    let mut paths = Vec::new();
    let entries = fs::read_dir(&directory).map_err(|error| {
        ReleaseError::storage(format!("failed to read '{}': {error}", directory.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            ReleaseError::storage(format!("failed to read '{}': {error}", directory.display()))
        })?;
        let path = entry.path().join("pubspec.yaml");
        if path.is_file() {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(ReleaseError::storage(
            "no Flutter package pubspec files were found",
        ));
    }
    Ok(paths)
}

fn collect_plugin_manifests(root: &Path) -> ReleaseResult<Vec<PathBuf>> {
    let directory = root.join("plugins");
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    collect_named_files(&directory, "vesper-plugin.toml")
}

fn collect_named_files(root: &Path, name: &str) -> ReleaseResult<Vec<PathBuf>> {
    collect_named_files_with_limit(root, name, MAX_RELEASE_SCAN_ENTRIES)
}

fn collect_named_files_with_limit(
    root: &Path,
    name: &str,
    max_entries: usize,
) -> ReleaseResult<Vec<PathBuf>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut scanned_entries = 0_usize;
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|error| {
            ReleaseError::storage(format!("failed to read '{}': {error}", directory.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                ReleaseError::storage(format!(
                    "failed to read directory entry in '{}': {error}",
                    directory.display()
                ))
            })?;
            scanned_entries = scanned_entries.saturating_add(1);
            if scanned_entries > max_entries {
                return Err(ReleaseError::storage(format!(
                    "release scan exceeds {max_entries} directory entries"
                )));
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                ReleaseError::storage(format!("failed to inspect '{}': {error}", path.display()))
            })?;
            if metadata.file_type().is_dir() {
                if !is_release_scan_cache_directory(&path) {
                    pending.push(path);
                }
            } else if metadata.file_type().is_file()
                && path.file_name().is_some_and(|file_name| file_name == name)
            {
                files.push(path);
                if files.len() > MAX_RELEASE_SCAN_FILES {
                    return Err(ReleaseError::storage(format!(
                        "release scan exceeds {MAX_RELEASE_SCAN_FILES} files"
                    )));
                }
            }
        }
    }
    files.sort();
    Ok(files)
}

fn is_release_scan_cache_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                "build"
                    | "target"
                    | ".git"
                    | ".gradle"
                    | ".dart_tool"
                    | ".build"
                    | "Pods"
                    | "node_modules"
            )
        })
}

fn read_workspace_version(root: &Path) -> ReleaseResult<String> {
    let path = root.join("Cargo.toml");
    let source = read_required_text(&path)?;
    let value = toml::from_str::<toml::Value>(&source).map_err(|error| {
        ReleaseError::storage(format!("invalid TOML in '{}': {error}", path.display()))
    })?;
    value
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ReleaseError::storage("Unable to resolve current workspace version."))
}

fn read_ios_build(root: &Path) -> ReleaseResult<String> {
    let path = root.join("lib/ios/VesperPlayerKit/project.yml");
    let source = read_required_text(&path)?;
    let expression = compile_regex(r#"(?m)^\s*CFBundleVersion: \"([0-9]+)\""#, "iOS build")?;
    expression
        .captures(&source)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str().to_owned())
        .ok_or_else(|| ReleaseError::input("Unable to resolve current iOS build version."))
}

fn read_android_version_code(root: &Path) -> ReleaseResult<String> {
    let path = root.join("examples/android-compose-host/app/build.gradle.kts");
    let source = read_required_text(&path)?;
    let expression = compile_regex(r"(?m)^\s*versionCode = ([0-9]+)", "Android versionCode")?;
    expression
        .captures(&source)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str().to_owned())
        .ok_or_else(|| ReleaseError::input("Unable to resolve current Android versionCode."))
}

fn read_android_publication_version(root: &Path) -> ReleaseResult<String> {
    let path = root.join("lib/android/build.gradle.kts");
    let source = read_required_text(&path)?;
    let expression = compile_regex(
        r#"(?m)^val vesperDefaultPublicationVersion = \"([^\"]+)\"$"#,
        "Android publication version",
    )?;
    expression
        .captures(&source)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str().to_owned())
        .ok_or_else(|| ReleaseError::input("Unable to resolve current publication version."))
}

fn verify_product_version(
    root: &Path,
    version: &ReleaseVersion,
    publication_version: &str,
    ios_build: &str,
    android_version_code: &str,
) -> ReleaseResult<Vec<String>> {
    let mut issues = Vec::new();
    match read_workspace_version(root) {
        Ok(workspace_version) if workspace_version == version.as_str() => {}
        Ok(workspace_version) => issues.push(format!(
            "Cargo workspace version mismatch.\n  {}\n  expected {}, found {workspace_version}",
            root.join("Cargo.toml").display(),
            version.as_str()
        )),
        Err(_) => issues.push(format!(
            "Cargo workspace version mismatch.\n  {}",
            root.join("Cargo.toml").display()
        )),
    }
    expect_line(
        root,
        "lib/android/build.gradle.kts",
        &format!(
            r#"^val vesperDefaultPublicationVersion = \"{}\"$"#,
            regex::escape(publication_version)
        ),
        "Android library version mismatch.",
        &mut issues,
    )?;
    expect_line(
        root,
        "lib/ios/VesperPlayerKit/project.yml",
        &format!(
            r#"^        CFBundleShortVersionString: \"{}\"$"#,
            regex::escape(version.as_str())
        ),
        "iOS marketing version mismatch.",
        &mut issues,
    )?;
    expect_line(
        root,
        "lib/ios/VesperPlayerKit/project.yml",
        &format!(
            r#"^        CFBundleVersion: \"{}\"$"#,
            regex::escape(ios_build)
        ),
        "iOS build version mismatch.",
        &mut issues,
    )?;
    expect_line(
        root,
        "lib/ios/VesperPlayerKit/Sources/Generated-Info.plist",
        &format!(r"^\s*<string>{}</string>$", regex::escape(version.as_str())),
        "iOS generated marketing version mismatch.",
        &mut issues,
    )?;
    expect_line(
        root,
        "lib/ios/VesperPlayerKit/Sources/Generated-Info.plist",
        &format!(r"^\s*<string>{}</string>$", regex::escape(ios_build)),
        "iOS generated build version mismatch.",
        &mut issues,
    )?;
    expect_line(
        root,
        "examples/android-compose-host/app/build.gradle.kts",
        &format!(
            r#"^\s*versionName = \"{}\"$"#,
            regex::escape(version.as_str())
        ),
        "Android sample versionName mismatch.",
        &mut issues,
    )?;
    expect_line(
        root,
        "examples/android-compose-host/app/build.gradle.kts",
        &format!(
            r"^\s*versionCode = {}$",
            regex::escape(android_version_code)
        ),
        "Android sample versionCode mismatch.",
        &mut issues,
    )?;
    expect_line(
        root,
        "examples/flutter-host/pubspec.yaml",
        &format!(
            r"^version: {}\+{}$",
            regex::escape(version.as_str()),
            regex::escape(android_version_code)
        ),
        "Flutter host version mismatch.",
        &mut issues,
    )?;

    for pubspec in collect_flutter_pubspecs(root)? {
        expect_line_path(
            &pubspec,
            &format!(r"^version: {}$", regex::escape(publication_version)),
            "Flutter package version mismatch.",
            &mut issues,
        )?;
        verify_pubspec_dependencies(&pubspec, publication_version, &mut issues)?;
    }

    for relative in FLUTTER_ANDROID_GRADLE_FILES {
        expect_line(
            root,
            relative,
            &format!(r#"^version = \"{}\"$"#, regex::escape(publication_version)),
            "Flutter Android plugin Gradle version mismatch.",
            &mut issues,
        )?;
    }

    for relative in FLUTTER_IOS_PACKAGE_FILES {
        expect_line(
            root,
            relative,
            &format!(
                r#"^private let vesperPlayerKitVersion: Version = \"{}\"$"#,
                regex::escape(publication_version)
            ),
            "Flutter iOS native dependency version mismatch.",
            &mut issues,
        )?;
    }

    for relative in PRODUCT_CHANGELOG_FILES {
        expect_line(
            root,
            relative,
            &format!(
                r"^## {} - (?:Unreleased|[0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}})$",
                regex::escape(version.as_str())
            ),
            "Changelog version heading mismatch.",
            &mut issues,
        )?;
    }
    for relative in FLUTTER_CHANGELOG_FILES {
        expect_line(
            root,
            relative,
            &format!(
                r"^## {} - (?:Unreleased|[0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}})$",
                regex::escape(publication_version)
            ),
            "Flutter changelog version heading mismatch.",
            &mut issues,
        )?;
    }

    verify_cargo_lock(root, version.as_str(), &mut issues)?;
    verify_path_dependency_versions(root, version.as_str(), &mut issues)?;
    verify_plugin_manifest_versions(root, version, &mut issues)?;
    verify_stale_version_fields(root, &mut issues)?;
    verify_release_hardcoding(root, &mut issues)?;
    Ok(issues)
}

fn expect_line(
    root: &Path,
    relative: &str,
    pattern: &str,
    message: &str,
    issues: &mut Vec<String>,
) -> ReleaseResult<()> {
    expect_line_path(&root.join(relative), pattern, message, issues)
}

fn expect_line_path(
    path: &Path,
    pattern: &str,
    message: &str,
    issues: &mut Vec<String>,
) -> ReleaseResult<()> {
    let source = match read_required_text(path) {
        Ok(source) => source,
        Err(_) => {
            issues.push(format!("{message}\n  {}", path.display()));
            return Ok(());
        }
    };
    let expression = compile_regex(pattern, "version verification")?;
    if !source.lines().any(|line| expression.is_match(line)) {
        issues.push(format!("{message}\n  {}", path.display()));
    }
    Ok(())
}

fn verify_pubspec_dependencies(
    path: &Path,
    version: &str,
    issues: &mut Vec<String>,
) -> ReleaseResult<()> {
    let source = read_required_text(path)?;
    let names = FLUTTER_PACKAGES
        .iter()
        .map(|name| regex::escape(name))
        .collect::<Vec<_>>()
        .join("|");
    let any_internal = compile_regex(
        &format!(r"^  ({names}): \^([0-9]+\.[0-9]+\.[0-9]+(?:[+-][A-Za-z0-9.-]+)?)$"),
        "Flutter dependency constraint",
    )?;
    for line in source.lines() {
        if let Some(captures) = any_internal.captures(line)
            && captures
                .get(2)
                .is_some_and(|capture| capture.as_str() != version)
        {
            issues.push(format!(
                "Flutter package dependency version mismatch.\n  {}\n  {line}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn verify_cargo_lock(root: &Path, version: &str, issues: &mut Vec<String>) -> ReleaseResult<()> {
    let path = root.join("Cargo.lock");
    if !path.is_file() {
        return Ok(());
    }
    let source = read_required_text(&path)?;
    let value = toml::from_str::<toml::Value>(&source).map_err(|error| {
        ReleaseError::storage(format!("invalid TOML in '{}': {error}", path.display()))
    })?;
    let mut mismatches = Vec::new();
    if let Some(packages) = value.get("package").and_then(toml::Value::as_array) {
        for package in packages {
            let name = package.get("name").and_then(toml::Value::as_str);
            let package_version = package.get("version").and_then(toml::Value::as_str);
            if let (Some(name), Some(package_version)) = (name, package_version)
                && (name == "basic-player" || name.starts_with("player-"))
                && package_version != version
            {
                mismatches.push(format!("{name} {package_version}"));
            }
        }
    }
    if !mismatches.is_empty() {
        issues.push(format!(
            "Cargo.lock workspace package versions are not aligned with {version}:\n{}",
            mismatches.join("\n")
        ));
    }
    Ok(())
}

fn verify_path_dependency_versions(
    root: &Path,
    version: &str,
    issues: &mut Vec<String>,
) -> ReleaseResult<()> {
    let expression = compile_regex(
        r#"^player-[A-Za-z0-9_-]+ = \{[^\r\n]*version = \"([0-9]+\.[0-9]+\.[0-9]+)\"[^\r\n]*path = [^\r\n]*\}$"#,
        "Cargo path dependency version",
    )?;
    for manifest in collect_named_files(&root.join("crates"), "Cargo.toml")? {
        let source = read_required_text(&manifest)?;
        for line in source.lines() {
            if let Some(captures) = expression.captures(line)
                && captures
                    .get(1)
                    .is_some_and(|capture| capture.as_str() != version)
            {
                issues.push(format!(
                    "Cargo path dependency version mismatch.\n  {}\n  {line}",
                    manifest.display()
                ));
            }
        }
    }
    Ok(())
}

fn verify_plugin_manifest_versions(
    root: &Path,
    version: &ReleaseVersion,
    issues: &mut Vec<String>,
) -> ReleaseResult<()> {
    let expected_host_sdk = plugin_host_sdk_requirement(version)?;
    for manifest in collect_plugin_manifests(root)? {
        let source = read_required_text(&manifest)?;
        let value = toml::from_str::<toml::Value>(&source).map_err(|error| {
            ReleaseError::storage(format!("invalid TOML in '{}': {error}", manifest.display()))
        })?;
        let plugin_version = value
            .get("plugin")
            .and_then(|plugin| plugin.get("version"))
            .and_then(toml::Value::as_str);
        if plugin_version != Some(version.as_str()) {
            issues.push(format!(
                "Plugin manifest version mismatch.\n  {}\n  expected {}, found {}",
                manifest.display(),
                version.as_str(),
                plugin_version.unwrap_or("<missing>")
            ));
        }
        let host_sdk = value
            .get("compatibility")
            .and_then(|compatibility| compatibility.get("host_sdk"))
            .and_then(toml::Value::as_str);
        if host_sdk != Some(expected_host_sdk.as_str()) {
            issues.push(format!(
                "Plugin host SDK requirement mismatch.\n  {}\n  expected {}, found {}",
                manifest.display(),
                expected_host_sdk,
                host_sdk.unwrap_or("<missing>")
            ));
        }
    }
    Ok(())
}

fn plugin_host_sdk_requirement(version: &ReleaseVersion) -> ReleaseResult<String> {
    if version.major == 0 {
        let upper_minor = version.minor.checked_add(1).ok_or_else(|| {
            ReleaseError::input(format!(
                "Plugin host SDK upper bound overflows for release version {version}"
            ))
        })?;
        Ok(format!(">={version}, <0.{upper_minor}.0"))
    } else {
        let upper_major = version.major.checked_add(1).ok_or_else(|| {
            ReleaseError::input(format!(
                "Plugin host SDK upper bound overflows for release version {version}"
            ))
        })?;
        Ok(format!(">={version}, <{upper_major}.0.0"))
    }
}

fn verify_stale_version_fields(root: &Path, issues: &mut Vec<String>) -> ReleaseResult<()> {
    let expression = compile_regex(
        r#"version: 0\.2\.0|version = \"0\.2\.0\"|CFBundleShortVersionString: \"0\.2\.0\"|<string>0\.2\.0</string>"#,
        "stale product version",
    )?;
    let targets = [
        root.join("Cargo.toml"),
        root.join("CHANGELOG.md"),
        root.join("lib/android"),
        root.join("lib/flutter"),
        root.join("lib/ios/VesperPlayerKit"),
        root.join("examples/android-compose-host/app/build.gradle.kts"),
        root.join("examples/flutter-host/pubspec.yaml"),
    ];
    let matches = scan_targets(root, &targets, &expression, |path| {
        excluded_version_path(root, path)
    })?;
    if !matches.is_empty() {
        issues.push(format!(
            "Found stale 0.2.0 product version fields:\n{}",
            matches.join("\n")
        ));
    }
    Ok(())
}

fn verify_release_hardcoding(root: &Path, issues: &mut Vec<String>) -> ReleaseResult<()> {
    let expression = compile_regex(
        r#"release (?:set-version|verify-version) [0-9]+\.[0-9]+\.[0-9]+|flutter (?:stage-pub|pub-dry-run|pub-publish) [^\s]+ [0-9]+\.[0-9]+\.[0-9]+|VESPER_RELEASE_(?:VERSION|BUILD)=\"\$\{VESPER_RELEASE_[^:]+:-[0-9]"#,
        "release version hardcoding",
    )?;
    let targets = [root.join("scripts"), root.join(".github/workflows")];
    let matches = scan_targets(root, &targets, &expression, |path| {
        excluded_version_path(root, path)
            || path
                .strip_prefix(root)
                .is_ok_and(|relative| relative == Path::new("scripts/README.md"))
    })?;
    if !matches.is_empty() {
        issues.push(format!(
            "Found release script version hardcoding:\n{}",
            matches.join("\n")
        ));
    }
    Ok(())
}

fn excluded_version_path(root: &Path, path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "pubspec.lock")
        || path
            .strip_prefix(root)
            .unwrap_or(path)
            .components()
            .any(|component| {
                matches!(
                    component.as_os_str().to_str(),
                    Some(
                        "build"
                            | "target"
                            | "devnotes"
                            | ".git"
                            | ".gradle"
                            | ".dart_tool"
                            | ".build"
                            | "Pods"
                    )
                )
            })
}

fn scan_targets(
    root: &Path,
    targets: &[PathBuf],
    expression: &Regex,
    excluded: impl Fn(&Path) -> bool,
) -> ReleaseResult<Vec<String>> {
    let mut files = BTreeSet::new();
    let mut pending = Vec::new();
    for target in targets {
        if target.is_file() {
            files.insert(target.clone());
        } else if target.is_dir() {
            pending.push(target.clone());
        }
    }
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| {
            ReleaseError::storage(format!("failed to read '{}': {error}", directory.display()))
        })? {
            let entry = entry.map_err(|error| {
                ReleaseError::storage(format!(
                    "failed to read directory entry in '{}': {error}",
                    directory.display()
                ))
            })?;
            let path = entry.path();
            if excluded(&path) {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                ReleaseError::storage(format!("failed to inspect '{}': {error}", path.display()))
            })?;
            if metadata.file_type().is_dir() {
                pending.push(path);
            } else if metadata.file_type().is_file() {
                files.insert(path);
                if files.len() > MAX_RELEASE_SCAN_FILES {
                    return Err(ReleaseError::storage(format!(
                        "release scan exceeds {MAX_RELEASE_SCAN_FILES} files"
                    )));
                }
            }
        }
    }

    let mut matches = Vec::new();
    for path in files {
        let metadata = fs::metadata(&path).map_err(|error| {
            ReleaseError::storage(format!("failed to inspect '{}': {error}", path.display()))
        })?;
        if metadata.len() > MAX_RELEASE_FILE_BYTES as u64 {
            continue;
        }
        let bytes = fs::read(&path).map_err(|error| {
            ReleaseError::storage(format!("failed to read '{}': {error}", path.display()))
        })?;
        let Ok(source) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let display = path.strip_prefix(root).unwrap_or(&path).display();
        for (index, line) in source.lines().enumerate() {
            if expression.is_match(line) {
                matches.push(format!("{display}:{}:{line}", index + 1));
            }
        }
    }
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fixture(root: &Path, relative: &str, source: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
        fs::write(path, source).expect("write fixture");
    }

    fn release_fixture() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("temporary release fixture");
        let root = directory.path();
        write_fixture(
            root,
            "Cargo.toml",
            concat!(
                "[workspace]\n",
                "members = []\n\n",
                "[workspace.package]\n",
                "authors = [\"Vesper\"]\n",
                "version = \"0.4.0\"\n",
            ),
        );
        write_fixture(
            root,
            "Cargo.lock",
            concat!(
                "version = 4\n\n",
                "[[package]]\n",
                "name = \"player-fixture\"\n",
                "version = \"0.4.0\"\n",
            ),
        );
        write_fixture(
            root,
            "crates/player-fixture/Cargo.toml",
            concat!(
                "[package]\n",
                "name = \"player-fixture\"\n",
                "version = \"0.4.0\"\n",
                "edition = \"2024\"\n\n",
                "[dependencies]\n",
                "player-plugin = { version = \"0.4.0\", path = \"../player-plugin\" }\n",
            ),
        );
        write_fixture(
            root,
            "plugins/fixture/vesper-plugin.toml",
            concat!(
                "schema_version = 1\n\n",
                "[plugin]\n",
                "id = \"dev.vesper.fixture\"\n",
                "name = \"Fixture\"\n",
                "version = \"0.4.0\"\n",
                "description = \"Fixture plugin.\"\n",
                "license = \"Apache-2.0\"\n",
                "publisher = \"dev.vesper\"\n\n",
                "[compatibility]\n",
                "host_sdk = \">=0.4.0, <0.5.0\"\n",
                "abi_major = 1\n",
                "abi_minor_min = 0\n",
                "abi_minor_max = 0\n",
            ),
        );
        write_fixture(
            root,
            "lib/android/build.gradle.kts",
            concat!(
                "val vesperDefaultPublicationVersion = \"0.4.0\"\n",
                "val vesperPublicationVersion = vesperDefaultPublicationVersion\n",
                "allprojects {\n    version = vesperPublicationVersion\n}\n",
            ),
        );
        for package in [
            "vesper_player",
            "vesper_player_android",
            "vesper_player_external_playback",
            "vesper_player_ios",
            "vesper_player_platform_interface",
            "vesper_player_remux_ffmpeg",
            "vesper_player_source_normalizer_ffmpeg",
            "vesper_player_ui",
        ] {
            write_fixture(
                root,
                &format!("lib/flutter/{package}/pubspec.yaml"),
                "name: fixture\nversion: 0.4.0\n",
            );
        }
        for relative in FLUTTER_ANDROID_GRADLE_FILES {
            write_fixture(root, relative, "version = \"0.4.0\"\n");
        }
        for relative in FLUTTER_IOS_PACKAGE_FILES {
            write_fixture(
                root,
                relative,
                "private let vesperPlayerKitVersion: Version = \"0.4.0\"\n",
            );
        }
        write_fixture(
            root,
            "lib/ios/VesperPlayerKit/project.yml",
            concat!(
                "settings:\n",
                "        CFBundleShortVersionString: \"0.4.0\"\n",
                "        CFBundleVersion: \"400\"\n",
            ),
        );
        write_fixture(
            root,
            "lib/ios/VesperPlayerKit/Sources/Generated-Info.plist",
            concat!(
                "<plist>\n",
                "\t<key>CFBundleShortVersionString</key>\n",
                "\t<string>0.4.0</string>\n",
                "\t<key>CFBundleVersion</key>\n",
                "\t<string>400</string>\n",
                "</plist>\n",
            ),
        );
        write_fixture(
            root,
            "examples/android-compose-host/app/build.gradle.kts",
            concat!(
                "android {\n",
                "        versionCode = 400\n",
                "        versionName = \"0.4.0\"\n",
                "}\n",
            ),
        );
        write_fixture(
            root,
            "examples/flutter-host/pubspec.yaml",
            "name: fixture\nversion: 0.4.0+400\n",
        );
        for relative in PRODUCT_CHANGELOG_FILES
            .iter()
            .chain(FLUTTER_CHANGELOG_FILES.iter())
        {
            write_fixture(
                root,
                relative,
                "# Changelog\n\n## 0.4.0 - Unreleased\n\n- Work.\n",
            );
        }
        directory
    }

    #[test]
    fn release_versions_preserve_numeric_contract() {
        assert_eq!(
            ReleaseVersion::parse("0.3.1")
                .expect("version")
                .android_code()
                .expect("code"),
            "301"
        );
        assert_eq!(
            ReleaseVersion::parse("1.2.34")
                .expect("version")
                .android_code()
                .expect("code"),
            "10234"
        );
        assert!(ReleaseVersion::parse("0.100.0").is_err());
        assert!(ReleaseVersion::parse("0.4.100").is_err());
        assert!(ReleaseVersion::parse("0.4").is_err());
    }

    #[test]
    fn release_tags_publish_the_numeric_base_version() {
        assert_eq!(
            ReleaseVersion::from_tag("refs/tags/v0.4.0-rc.2")
                .expect("tag")
                .as_str(),
            "0.4.0"
        );
        assert_eq!(
            ReleaseVersion::from_tag("0.4.0+build.3")
                .expect("tag")
                .as_str(),
            "0.4.0"
        );
        assert!(ReleaseVersion::from_tag("release-0.4.0").is_err());
    }

    #[test]
    fn stable_release_tags_reject_prerelease_and_ambiguous_forms() {
        assert_eq!(
            ReleaseContext::stable_version_from_tag("v0.4.0")
                .expect("stable tag")
                .as_str(),
            "0.4.0"
        );
        assert!(ReleaseContext::stable_version_from_tag("0.4.0").is_err());
        assert!(ReleaseContext::stable_version_from_tag("v0.4.0-rc.1").is_err());
        assert!(ReleaseContext::stable_version_from_tag("refs/tags/v0.4.0").is_err());
        assert!(ReleaseContext::stable_version_from_tag("v00.4.0").is_err());
    }

    #[test]
    fn release_channels_classify_only_v_prefixed_semver() {
        assert_eq!(
            ReleaseContext::channel_from_tag("v0.4.0")
                .expect("stable channel")
                .output(),
            "version=0.4.0\npublication_version=0.4.0\nstable=true\nprerelease=false\n"
        );
        assert_eq!(
            ReleaseContext::channel_from_tag("v0.4.0-rc.2")
                .expect("prerelease channel")
                .output(),
            "version=0.4.0\npublication_version=0.4.0-rc.2\nstable=false\nprerelease=true\n"
        );
        assert!(ReleaseContext::channel_from_tag("0.4.0").is_err());
        assert!(ReleaseContext::channel_from_tag("v0.4.0+build.1").is_err());
        assert!(ReleaseContext::channel_from_tag("v0.100.0").is_err());
    }

    #[test]
    fn civil_date_conversion_matches_known_utc_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_664), (2026, 7, 30));
    }

    #[test]
    fn changelog_update_replaces_unreleased_heading() {
        let source = "# Changelog\n\n## 0.4.0 - Unreleased\n\n- Work.\n";
        assert_eq!(
            update_changelog(source, "0.5.0", "2026-08-01").expect("changelog"),
            "# Changelog\n\n## 0.5.0 - 2026-08-01\n\n- Work.\n"
        );
    }

    #[test]
    fn path_dependency_update_changes_only_versioned_player_paths() {
        let source = concat!(
            "player-plugin = { version = \"0.4.0\", path = \"../plugin\" }\n",
            "serde = { version = \"1.0.0\", path = \"../serde\" }\n",
        );
        assert_eq!(
            replace_path_dependency_versions(source, "0.5.0").expect("dependencies"),
            concat!(
                "player-plugin = { version = \"0.5.0\", path = \"../plugin\" }\n",
                "serde = { version = \"1.0.0\", path = \"../serde\" }\n",
            )
        );
    }

    #[test]
    fn named_file_collection_skips_caches_and_bounds_directory_entries() {
        let directory = tempfile::tempdir().expect("temporary manifest tree");
        let root = directory.path().join("crates");
        write_fixture(directory.path(), "crates/fixture/Cargo.toml", "[package]\n");
        write_fixture(
            directory.path(),
            "crates/fixture/target/generated/Cargo.toml",
            "[package]\n",
        );

        assert_eq!(
            collect_named_files(&root, "Cargo.toml").expect("collect manifests"),
            vec![root.join("fixture/Cargo.toml")]
        );
        assert!(collect_named_files_with_limit(&root, "Cargo.toml", 2).is_err());
    }

    #[test]
    fn metadata_precedence_is_cli_then_environment_then_computed_default() {
        let context = ReleaseContext::new(
            PathBuf::from("."),
            ReleaseEnvironment::with_release_values(Some("901"), Some("902"), Some("903")),
        );
        let environment = context
            .metadata_for_version(
                "1.2.3",
                ReleaseMetadataOptions {
                    release_date: Some("2026-01-02".to_owned()),
                    ..ReleaseMetadataOptions::default()
                },
            )
            .expect("environment metadata");
        assert_eq!(environment.ios_build(), "901");
        assert_eq!(environment.android_version_code(), "903");

        let cli = context
            .metadata_for_version(
                "1.2.3",
                ReleaseMetadataOptions {
                    ios_build: Some("801".to_owned()),
                    android_version_code: Some("802".to_owned()),
                    release_date: Some("2026-01-03".to_owned()),
                },
            )
            .expect("CLI metadata");
        assert_eq!(cli.ios_build(), "801");
        assert_eq!(cli.android_version_code(), "802");
    }

    #[test]
    fn edit_plan_validates_every_edit_before_writing() {
        let directory = tempfile::tempdir().expect("temporary edit plan");
        let first = directory.path().join("first.txt");
        let missing = directory.path().join("missing.txt");
        fs::write(&first, "keep\n").expect("write first file");

        let mut plan = EditPlan::default();
        plan.insert(first.clone(), "changed\n".to_owned())
            .expect("plan first edit");
        assert!(plan.insert(missing, "new\n".to_owned()).is_err());
        assert_eq!(
            fs::read_to_string(first).expect("read first file"),
            "keep\n"
        );
    }

    #[test]
    fn set_version_updates_and_verifies_the_complete_product_fixture() {
        let directory = release_fixture();
        let context = ReleaseContext::new(
            directory.path().to_path_buf(),
            ReleaseEnvironment::default(),
        );
        let metadata = context
            .metadata_for_version(
                "0.4.1",
                ReleaseMetadataOptions {
                    release_date: Some("2026-08-01".to_owned()),
                    ..ReleaseMetadataOptions::default()
                },
            )
            .expect("release metadata");
        context.set_version(&metadata).expect("set version");
        context
            .verify_version("0.4.1", Some("401".to_owned()), Some("401".to_owned()))
            .expect("verify version");
        assert!(
            fs::read_to_string(directory.path().join("Cargo.toml"))
                .expect("workspace manifest")
                .contains("version = \"0.4.1\"")
        );
        assert!(
            fs::read_to_string(directory.path().join("crates/player-fixture/Cargo.toml"))
                .expect("crate manifest")
                .contains("player-plugin = { version = \"0.4.1\"")
        );
        let plugin_manifest =
            fs::read_to_string(directory.path().join("plugins/fixture/vesper-plugin.toml"))
                .expect("plugin manifest");
        assert!(plugin_manifest.contains("version = \"0.4.1\""));
        assert!(plugin_manifest.contains("host_sdk = \">=0.4.1, <0.5.0\""));
    }

    #[test]
    fn prerelease_metadata_separates_product_and_publication_versions() {
        let directory = release_fixture();
        let context = ReleaseContext::new(
            directory.path().to_path_buf(),
            ReleaseEnvironment::default(),
        );
        let metadata = context
            .metadata_from_tag(
                "refs/tags/v0.4.1-rc.2",
                ReleaseMetadataOptions {
                    release_date: Some("2026-08-17".to_owned()),
                    ..ReleaseMetadataOptions::default()
                },
            )
            .expect("prerelease metadata");

        assert_eq!(metadata.version(), "0.4.1");
        assert_eq!(metadata.publication_version(), "0.4.1-rc.2");
        context
            .set_version(&metadata)
            .expect("set prerelease version");
        context
            .verify_metadata(&metadata)
            .expect("verify prerelease metadata");
        assert_eq!(
            context.verify_current().expect("verify current prerelease"),
            "0.4.1"
        );

        let cargo =
            fs::read_to_string(directory.path().join("Cargo.toml")).expect("workspace manifest");
        assert!(cargo.contains("version = \"0.4.1\""));
        let android = fs::read_to_string(directory.path().join("lib/android/build.gradle.kts"))
            .expect("Android publication version");
        assert!(android.contains("vesperDefaultPublicationVersion = \"0.4.1-rc.2\""));
        let pubspec = fs::read_to_string(
            directory
                .path()
                .join("lib/flutter/vesper_player/pubspec.yaml"),
        )
        .expect("Flutter package version");
        assert!(pubspec.contains("version: 0.4.1-rc.2"));
        let swift = fs::read_to_string(
            directory
                .path()
                .join("lib/flutter/vesper_player_ios/ios/vesper_player_ios/Package.swift"),
        )
        .expect("Flutter iOS dependency version");
        assert!(swift.contains("vesperPlayerKitVersion: Version = \"0.4.1-rc.2\""));
        let product_changelog =
            fs::read_to_string(directory.path().join("CHANGELOG.md")).expect("product changelog");
        assert!(product_changelog.contains("## 0.4.1 - 2026-08-17"));
        let flutter_changelog = fs::read_to_string(
            directory
                .path()
                .join("lib/flutter/vesper_player/CHANGELOG.md"),
        )
        .expect("Flutter changelog");
        assert!(flutter_changelog.contains("## 0.4.1-rc.2 - 2026-08-17"));
    }

    #[test]
    fn verification_rejects_bundled_plugin_version_drift() {
        let directory = release_fixture();
        let context = ReleaseContext::new(
            directory.path().to_path_buf(),
            ReleaseEnvironment::default(),
        );

        let error = context
            .verify_version("0.4.1", Some("401".to_owned()), Some("401".to_owned()))
            .expect_err("plugin drift must fail release verification");

        assert_eq!(error.kind(), ReleaseErrorKind::Verification);
        assert!(
            error
                .to_string()
                .contains("Plugin manifest version mismatch")
        );
        assert!(
            error
                .to_string()
                .contains("Plugin host SDK requirement mismatch")
        );
    }

    #[test]
    fn verification_reads_only_the_workspace_package_version() {
        let directory = release_fixture();
        write_fixture(
            directory.path(),
            "Cargo.toml",
            concat!(
                "[workspace]\n",
                "members = []\n\n",
                "[workspace.package]\n",
                "version = \"0.4.1\"\n\n",
                "[workspace.dependencies]\n",
                "fixture = { version = \"0.4.0\" }\n",
            ),
        );

        let issues = verify_product_version(
            directory.path(),
            &ReleaseVersion::parse("0.4.0").expect("release version"),
            "0.4.0",
            "400",
            "400",
        )
        .expect("verify fixture");

        assert!(issues.iter().any(|issue| {
            issue.contains("Cargo workspace version mismatch.")
                && issue.contains("expected 0.4.0, found 0.4.1")
        }));
    }

    #[test]
    fn set_version_preflight_failure_preserves_existing_files() {
        let directory = tempfile::tempdir().expect("temporary incomplete release fixture");
        write_fixture(
            directory.path(),
            "Cargo.toml",
            concat!(
                "[workspace]\n",
                "members = []\n\n",
                "[workspace.package]\n",
                "authors = [\"Vesper\"]\n",
                "version = \"0.4.0\"\n",
            ),
        );
        let cargo_path = directory.path().join("Cargo.toml");
        let before = fs::read(&cargo_path).expect("read original manifest");
        let context = ReleaseContext::new(
            directory.path().to_path_buf(),
            ReleaseEnvironment::default(),
        );
        let metadata = context
            .metadata_for_version(
                "0.4.1",
                ReleaseMetadataOptions {
                    release_date: Some("2026-08-01".to_owned()),
                    ..ReleaseMetadataOptions::default()
                },
            )
            .expect("release metadata");
        assert!(context.set_version(&metadata).is_err());
        assert_eq!(
            fs::read(cargo_path).expect("read preserved manifest"),
            before
        );
    }
}
