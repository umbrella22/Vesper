// Apple release commands keep their policy types available on other hosts for
// compatibility diagnostics even though the release implementation is absent.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use semver::{Version, VersionReq};
use serde::Deserialize;

pub(crate) const FFMPEG_SOURCE_POLICY_PATH: &str = "scripts/ffmpeg-source-policy.toml";
const MAX_FFMPEG_SOURCE_POLICY_BYTES: u64 = 64 * 1024;
const SOURCE_ASSET_PREFIX: &str = "VesperPlayerOptionalPlugins-FFmpeg-";
const SOURCE_ASSET_SUFFIX: &str = "-source.tar.xz";
const FFMPEG_ARCHIVE_PREFIX: &str = "ffmpeg-";
const FFMPEG_ARCHIVE_SUFFIX: &str = ".tar.xz";
const MAX_RELEASE_INDEX_BYTES: usize = 8 * 1024 * 1024;

#[cfg(test)]
const TEST_FFMPEG_SOURCE_POLICY: &str = r#"
schema_version = 1

[compatibility]
requirement = ">=8.1.0, <8.2.0"
default_series = "8.1"

[release]
version = "8.1.2"
source_url = "https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz"
source_sha256 = "464beb5e7bf0c311e68b45ae2f04e9cc2af88851abb4082231742a74d97b524c"
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FfmpegSourcePolicyErrorKind {
    Storage,
    Invalid,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct FfmpegSourcePolicyError {
    kind: FfmpegSourcePolicyErrorKind,
    message: String,
}

impl FfmpegSourcePolicyError {
    fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: FfmpegSourcePolicyErrorKind::Storage,
            message: message.into(),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self {
            kind: FfmpegSourcePolicyErrorKind::Invalid,
            message: message.into(),
        }
    }

    pub(crate) const fn kind(&self) -> FfmpegSourcePolicyErrorKind {
        self.kind
    }
}

/// The checked-in source policy contains two deliberately separate contracts:
/// compatibility for ordinary builds and an exact lock for release artifacts.
#[derive(Debug, Clone)]
pub(crate) struct FfmpegSourcePolicy {
    compatibility_requirement_text: String,
    compatibility_requirement: VersionReq,
    default_series: String,
    release: FfmpegSourceLock,
}

impl FfmpegSourcePolicy {
    pub(crate) fn load(root: &Path) -> Result<Self, FfmpegSourcePolicyError> {
        Self::load_from_path(&root.join(FFMPEG_SOURCE_POLICY_PATH))
    }

    /// Load the policy for an ordinary source build. The environment override
    /// is retained for hermetic CI and local mirror tests; release verification
    /// uses load and therefore always reads the checked-in policy.
    pub(crate) fn load_for_build(root: &Path) -> Result<Self, FfmpegSourcePolicyError> {
        let path = std::env::var_os("VESPER_FFMPEG_SOURCE_POLICY_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join(FFMPEG_SOURCE_POLICY_PATH));
        Self::load_from_path(&path)
    }

    fn load_from_path(path: &Path) -> Result<Self, FfmpegSourcePolicyError> {
        let metadata = fs::metadata(path).map_err(|error| {
            FfmpegSourcePolicyError::storage(format!(
                "failed to inspect FFmpeg source policy '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.file_type().is_file() {
            return Err(FfmpegSourcePolicyError::invalid(format!(
                "FFmpeg source policy must be a regular file: {}",
                path.display()
            )));
        }
        if metadata.len() > MAX_FFMPEG_SOURCE_POLICY_BYTES {
            return Err(FfmpegSourcePolicyError::invalid(format!(
                "FFmpeg source policy exceeds {MAX_FFMPEG_SOURCE_POLICY_BYTES} bytes: {}",
                path.display()
            )));
        }
        let source = fs::read_to_string(path).map_err(|error| {
            FfmpegSourcePolicyError::storage(format!(
                "failed to read FFmpeg source policy '{}': {error}",
                path.display()
            ))
        })?;
        Self::parse(&source, path)
    }

    fn parse(source: &str, path: &Path) -> Result<Self, FfmpegSourcePolicyError> {
        let document: FfmpegSourcePolicyDocument = toml::from_str(source).map_err(|error| {
            FfmpegSourcePolicyError::invalid(format!(
                "invalid FFmpeg source policy '{}': {error}",
                path.display()
            ))
        })?;
        if document.schema_version != 1 {
            return Err(FfmpegSourcePolicyError::invalid(format!(
                "unsupported FFmpeg source policy schema version {} in '{}'",
                document.schema_version,
                path.display()
            )));
        }

        let compatibility_requirement = VersionReq::parse(&document.compatibility.requirement)
            .map_err(|error| {
                FfmpegSourcePolicyError::invalid(format!(
                    "invalid FFmpeg compatibility requirement '{}' in '{}': {error}",
                    document.compatibility.requirement,
                    path.display()
                ))
            })?;
        let (series_major, series_minor) = parse_series(&document.compatibility.default_series)
            .map_err(|message| {
                FfmpegSourcePolicyError::invalid(format!("{message} in '{}'", path.display()))
            })?;
        let series_floor = Version::new(series_major, series_minor, 0);
        if !compatibility_requirement.matches(&series_floor) {
            return Err(FfmpegSourcePolicyError::invalid(format!(
                "FFmpeg default series '{}' is outside compatibility requirement '{}' in '{}'",
                document.compatibility.default_series,
                document.compatibility.requirement,
                path.display()
            )));
        }

        let release_version =
            parse_ffmpeg_version(&document.release.version).map_err(|message| {
                FfmpegSourcePolicyError::invalid(format!("{message} in '{}'", path.display()))
            })?;
        if !compatibility_requirement.matches(&release_version) {
            return Err(FfmpegSourcePolicyError::invalid(format!(
                "locked FFmpeg version '{}' is outside compatibility requirement '{}' in '{}'",
                document.release.version,
                document.compatibility.requirement,
                path.display()
            )));
        }
        let expected_url = source_url_for_version(&release_version);
        if document.release.source_url != expected_url {
            return Err(FfmpegSourcePolicyError::invalid(format!(
                "locked FFmpeg source URL must be '{expected_url}' for version '{}' in '{}'",
                release_version,
                path.display()
            )));
        }
        validate_sha256(&document.release.source_sha256).map_err(|message| {
            FfmpegSourcePolicyError::invalid(format!("{message} in '{}'", path.display()))
        })?;

        Ok(Self {
            compatibility_requirement_text: document.compatibility.requirement,
            compatibility_requirement,
            default_series: document.compatibility.default_series,
            release: FfmpegSourceLock {
                version: release_version,
                source_url: document.release.source_url,
                source_sha256: document.release.source_sha256,
            },
        })
    }

    #[cfg(test)]
    pub(crate) fn test_fixture() -> Self {
        Self::parse(
            TEST_FFMPEG_SOURCE_POLICY,
            Path::new("fixture-ffmpeg-source-policy.toml"),
        )
        .expect("parse embedded FFmpeg source policy fixture")
    }

    pub(crate) fn compatibility_requirement(&self) -> &str {
        &self.compatibility_requirement_text
    }

    pub(crate) fn default_series(&self) -> &str {
        &self.default_series
    }

    pub(crate) fn release(&self) -> &FfmpegSourceLock {
        &self.release
    }

    pub(crate) fn parse_compatible_version(
        &self,
        value: &str,
    ) -> Result<Version, FfmpegSourcePolicyError> {
        let version = parse_ffmpeg_version(value).map_err(FfmpegSourcePolicyError::invalid)?;
        if !self.compatibility_requirement.matches(&version) {
            return Err(FfmpegSourcePolicyError::invalid(format!(
                "FFmpeg version '{version}' does not satisfy compatibility requirement '{}'",
                self.compatibility_requirement_text
            )));
        }
        Ok(version)
    }

    pub(crate) fn parse_compatible_source_asset(
        &self,
        asset_name: &str,
    ) -> Result<Version, FfmpegSourcePolicyError> {
        let version = source_asset_version(asset_name).ok_or_else(|| {
            FfmpegSourcePolicyError::invalid(format!(
                "invalid optional iOS FFmpeg source asset name: {asset_name}"
            ))
        })?;
        self.parse_compatible_version(version)
    }

    /// Validate a two-component source series and return its numeric parts.
    pub(crate) fn parse_compatible_series(
        &self,
        value: &str,
    ) -> Result<(u64, u64), FfmpegSourcePolicyError> {
        let (major, minor) = parse_series(value).map_err(FfmpegSourcePolicyError::invalid)?;
        let floor = Version::new(major, minor, 0);
        if !self.compatibility_requirement.matches(&floor) {
            return Err(FfmpegSourcePolicyError::invalid(format!(
                "FFmpeg source series '{value}' does not satisfy compatibility requirement '{}'",
                self.compatibility_requirement_text
            )));
        }
        Ok((major, minor))
    }

    /// Resolve an ordinary build version without consulting the release lock.
    pub(crate) fn resolve_series_version(
        &self,
        series: &str,
        cached_archive_names: impl IntoIterator<Item = String>,
        remote_index: Option<&str>,
    ) -> Result<SeriesResolution, FfmpegSourcePolicyError> {
        let (major, minor) = self.parse_compatible_series(series)?;
        let series = format!("{major}.{minor}");
        if let Some(version) =
            latest_series_archive_version(&series, cached_archive_names, FFMPEG_ARCHIVE_SUFFIX)
        {
            return Ok(SeriesResolution {
                version,
                used_fallback: false,
            });
        }
        if let Some(index) = remote_index
            && let Some(version) =
                latest_series_archive_version_from_index(&series, index, FFMPEG_ARCHIVE_SUFFIX)
        {
            return Ok(SeriesResolution {
                version,
                used_fallback: false,
            });
        }
        Ok(SeriesResolution {
            version: series,
            used_fallback: true,
        })
    }

    /// Resolve the source used by an ordinary build while preserving the
    /// platform-over-generic environment precedence used by the shell
    /// workers. Release callers should provide their exact lock separately.
    pub(crate) fn resolve_build_source(
        &self,
        inputs: &FfmpegBuildSourceInputs,
        cached_archive_names: impl IntoIterator<Item = String>,
        remote_index: Option<&str>,
    ) -> Result<FfmpegBuildSource, FfmpegSourcePolicyError> {
        if let Some(version) = inputs
            .platform_version
            .as_deref()
            .or(inputs.generic_version.as_deref())
        {
            return self.build_source(version.to_owned(), inputs.source_url.clone());
        }

        let series = inputs
            .platform_series
            .as_deref()
            .or(inputs.generic_series.as_deref())
            .unwrap_or(self.default_series());
        let resolution = self.resolve_series_version(series, cached_archive_names, remote_index)?;
        self.build_source(resolution.version, inputs.source_url.clone())
    }

    pub(crate) fn build_source(
        &self,
        version: String,
        source_url: Option<String>,
    ) -> Result<FfmpegBuildSource, FfmpegSourcePolicyError> {
        let normalized_version = normalize_series_version(&version)?;
        self.parse_compatible_version(&normalized_version)?;
        let archive_name = format!("{FFMPEG_ARCHIVE_PREFIX}{version}{FFMPEG_ARCHIVE_SUFFIX}");
        let source_url = source_url.unwrap_or_else(|| {
            format!("https://ffmpeg.org/releases/{FFMPEG_ARCHIVE_PREFIX}{version}{FFMPEG_ARCHIVE_SUFFIX}")
        });
        let expected_sha256 = (normalized_version == self.release.version.to_string())
            .then(|| self.release.source_sha256.clone());
        Ok(FfmpegBuildSource {
            version,
            archive_name,
            source_url,
            expected_sha256,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FfmpegSourceLock {
    version: Version,
    source_url: String,
    source_sha256: String,
}

impl FfmpegSourceLock {
    pub(crate) fn version(&self) -> &Version {
        &self.version
    }

    pub(crate) fn source_url(&self) -> &str {
        &self.source_url
    }

    pub(crate) fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    pub(crate) fn locked_build_source(&self) -> FfmpegBuildSource {
        let version = self.version.to_string();
        FfmpegBuildSource {
            archive_name: format!("{FFMPEG_ARCHIVE_PREFIX}{version}{FFMPEG_ARCHIVE_SUFFIX}"),
            version,
            source_url: self.source_url.clone(),
            expected_sha256: Some(self.source_sha256.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FfmpegBuildSource {
    pub(crate) version: String,
    pub(crate) archive_name: String,
    pub(crate) source_url: String,
    pub(crate) expected_sha256: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FfmpegBuildSourceInputs {
    pub(crate) platform_version: Option<String>,
    pub(crate) generic_version: Option<String>,
    pub(crate) platform_series: Option<String>,
    pub(crate) generic_series: Option<String>,
    pub(crate) source_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SeriesResolution {
    pub(crate) version: String,
    pub(crate) used_fallback: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FfmpegSourcePolicyDocument {
    schema_version: u32,
    compatibility: FfmpegCompatibilityDocument,
    release: FfmpegReleaseDocument,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FfmpegCompatibilityDocument {
    requirement: String,
    default_series: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FfmpegReleaseDocument {
    version: String,
    source_url: String,
    source_sha256: String,
}

pub(crate) fn source_archive_name_for_version(version: &Version) -> String {
    format!("ffmpeg-{version}.tar.xz")
}

pub(crate) fn source_url_for_version(version: &Version) -> String {
    format!(
        "https://ffmpeg.org/releases/{}",
        source_archive_name_for_version(version)
    )
}

#[allow(dead_code)]
pub(crate) fn source_asset_name_for_version(version: &Version) -> String {
    format!("{SOURCE_ASSET_PREFIX}{version}{SOURCE_ASSET_SUFFIX}")
}

pub(crate) fn source_asset_version(asset_name: &str) -> Option<&str> {
    asset_name
        .strip_prefix(SOURCE_ASSET_PREFIX)
        .and_then(|value| value.strip_suffix(SOURCE_ASSET_SUFFIX))
}

/// Find the highest patch archive in a bounded list of cache entry names.
/// ffmpeg-8.1.tar.xz is treated as patch zero, matching the shell worker.
pub(crate) fn latest_series_archive_version(
    series: &str,
    archive_names: impl IntoIterator<Item = String>,
    extension: &str,
) -> Option<String> {
    latest_package_series_archive_version("ffmpeg", series, archive_names, extension)
}

pub(crate) fn latest_package_series_archive_version(
    package: &str,
    series: &str,
    archive_names: impl IntoIterator<Item = String>,
    extension: &str,
) -> Option<String> {
    let (major, minor) = parse_series(series).ok()?;
    let canonical_series = format!("{major}.{minor}");
    let prefix = format!("{package}-{canonical_series}");
    let mut best: Option<(u64, String)> = None;
    for archive_name in archive_names {
        let Some(version) = archive_name
            .strip_prefix(&prefix)
            .and_then(|value| value.strip_suffix(extension))
        else {
            continue;
        };
        let patch = if version.is_empty() {
            0
        } else if let Some(patch) = version.strip_prefix('.') {
            if patch.is_empty() || !patch.bytes().all(|byte| byte.is_ascii_digit()) {
                continue;
            }
            match patch.parse::<u64>() {
                Ok(value) => value,
                Err(_) => continue,
            }
        } else {
            continue;
        };
        // Preserve the archive's spelling for patch zero. Both
        // `ffmpeg-8.1.tar.xz` and `ffmpeg-8.1.0.tar.xz` are valid inputs, but
        // they are different cache paths and the selected path must not be
        // reconstructed into the other spelling.
        let candidate = if version.is_empty() {
            canonical_series.clone()
        } else {
            format!("{canonical_series}{version}")
        };
        if best
            .as_ref()
            .is_none_or(|(current_patch, current_version)| {
                patch > *current_patch || (patch == *current_patch && candidate > *current_version)
            })
        {
            best = Some((patch, candidate));
        }
    }
    best.map(|(_, version)| version)
}

/// Parse the FFmpeg release index without depending on HTML structure. Only
/// canonical archive names are accepted, and the caller bounds the input size.
pub(crate) fn latest_series_archive_version_from_index(
    series: &str,
    index: &str,
    extension: &str,
) -> Option<String> {
    latest_package_series_archive_version_from_index("ffmpeg", series, index, extension)
}

pub(crate) fn latest_package_series_archive_version_from_index(
    package: &str,
    series: &str,
    index: &str,
    extension: &str,
) -> Option<String> {
    if index.len() > MAX_RELEASE_INDEX_BYTES {
        return None;
    }
    let (major, minor) = parse_series(series).ok()?;
    let canonical_series = format!("{major}.{minor}");
    let escaped = regex::escape(&canonical_series);
    let escaped_extension = regex::escape(extension);
    let escaped_package = regex::escape(package);
    let expression = Regex::new(&format!(
        r"{escaped_package}-{escaped}(?:\.([0-9]+))?{escaped_extension}"
    ))
    .ok()?;
    let names = expression.find_iter(index).filter_map(|matched| {
        let before = index[..matched.start()].chars().next_back();
        let after = index[matched.end()..].chars().next();
        (!before.is_some_and(is_archive_name_character)
            && !after.is_some_and(is_archive_name_character))
        .then(|| matched.as_str().to_owned())
    });
    latest_package_series_archive_version(package, &canonical_series, names, extension)
}

fn is_archive_name_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
}

fn normalize_series_version(value: &str) -> Result<String, FfmpegSourcePolicyError> {
    let mut parts = value.split('.');
    let major = parts
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .ok_or_else(|| {
            FfmpegSourcePolicyError::invalid(format!("invalid FFmpeg version '{value}'"))
        })?;
    let minor = parts
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .ok_or_else(|| {
            FfmpegSourcePolicyError::invalid(format!("invalid FFmpeg version '{value}'"))
        })?;
    let patch = match parts.next() {
        Some(part) => part.parse::<u64>().map_err(|_| {
            FfmpegSourcePolicyError::invalid(format!("invalid exact FFmpeg version '{value}'"))
        })?,
        None => 0,
    };
    if parts.next().is_some() {
        return Err(FfmpegSourcePolicyError::invalid(format!(
            "invalid exact FFmpeg version '{value}'"
        )));
    }
    Ok(format!("{major}.{minor}.{patch}"))
}

fn parse_ffmpeg_version(value: &str) -> Result<Version, String> {
    let version = Version::parse(value)
        .map_err(|error| format!("invalid exact FFmpeg version '{value}': {error}"))?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err(format!(
            "FFmpeg version '{value}' must not contain prerelease or build metadata"
        ));
    }
    Ok(version)
}

fn parse_series(value: &str) -> Result<(u64, u64), String> {
    let mut parts = value.split('.');
    let major = parts
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .ok_or_else(|| format!("invalid FFmpeg default series '{value}'"))?;
    let minor = parts
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .ok_or_else(|| format!("invalid FFmpeg default series '{value}'"))?;
    if parts.next().is_some() {
        return Err(format!("invalid FFmpeg default series '{value}'"));
    }
    Ok((major, minor))
}

fn validate_sha256(value: &str) -> Result<(), String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err("FFmpeg source SHA-256 must contain 64 lowercase hexadecimal characters".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<FfmpegSourcePolicy, FfmpegSourcePolicyError> {
        FfmpegSourcePolicy::parse(source, Path::new("fixture-policy.toml"))
    }

    #[test]
    fn checked_in_policy_is_valid() {
        let Some(root) = crate::source_checkout_root() else {
            return;
        };
        let policy = FfmpegSourcePolicy::load(&root).expect("load checked-in policy");
        assert_eq!(policy.default_series(), "8.1");
        assert_eq!(policy.release().version(), &Version::new(8, 1, 2));
    }

    #[test]
    fn compatibility_and_release_lock_are_distinct_contracts() {
        let policy = parse(TEST_FFMPEG_SOURCE_POLICY).expect("parse policy");
        assert_eq!(policy.compatibility_requirement(), ">=8.1.0, <8.2.0");
        assert_eq!(
            policy
                .parse_compatible_source_asset(
                    "VesperPlayerOptionalPlugins-FFmpeg-8.1.3-source.tar.xz"
                )
                .expect("8.1.3 is compatible"),
            Version::new(8, 1, 3)
        );
        assert!(policy.parse_compatible_version("8.2.0").is_err());
        assert_eq!(
            source_asset_name_for_version(policy.release().version()),
            "VesperPlayerOptionalPlugins-FFmpeg-8.1.2-source.tar.xz"
        );
    }

    #[test]
    fn release_lock_rejects_inconsistent_url_version_and_checksum() {
        for invalid in [
            TEST_FFMPEG_SOURCE_POLICY.replace("version = \"8.1.2\"", "version = \"8.1.3\""),
            TEST_FFMPEG_SOURCE_POLICY.replace(
                "https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz",
                "https://example.invalid/ffmpeg-8.1.2.tar.xz",
            ),
            TEST_FFMPEG_SOURCE_POLICY.replace(
                "464beb5e7bf0c311e68b45ae2f04e9cc2af88851abb4082231742a74d97b524c",
                "ABCDEF",
            ),
        ] {
            assert!(parse(&invalid).is_err());
        }
    }

    #[test]
    fn policy_rejects_unknown_fields_and_out_of_range_release() {
        assert!(
            parse(
                &TEST_FFMPEG_SOURCE_POLICY
                    .replace("schema_version = 1", "schema_version = 1\nextra = 1")
            )
            .is_err()
        );
        assert!(
            parse(
                &TEST_FFMPEG_SOURCE_POLICY
                    .replace(">=8.1.0, <8.2.0", ">=8.0.0, <8.1.0")
                    .replace("default_series = \"8.1\"", "default_series = \"8.0\"")
            )
            .is_err()
        );
    }

    #[test]
    fn cache_selection_uses_highest_patch_and_preserves_patch_zero_spelling() {
        let policy = parse(TEST_FFMPEG_SOURCE_POLICY).expect("parse policy");
        let source = policy
            .resolve_build_source(
                &FfmpegBuildSourceInputs::default(),
                [
                    "ffmpeg-8.1.2.tar.xz".to_owned(),
                    "ffmpeg-8.1.10.tar.xz".to_owned(),
                ],
                None,
            )
            .expect("resolve cached source");
        assert_eq!(source.version, "8.1.10");
        assert_eq!(source.expected_sha256, None);

        let source = policy
            .resolve_build_source(
                &FfmpegBuildSourceInputs::default(),
                ["ffmpeg-8.1.0.tar.xz".to_owned()],
                None,
            )
            .expect("resolve explicit patch-zero archive");
        assert_eq!(source.version, "8.1.0");
        assert_eq!(source.archive_name, "ffmpeg-8.1.0.tar.xz");
        assert_eq!(source.expected_sha256, None);

        let source = policy
            .resolve_build_source(
                &FfmpegBuildSourceInputs::default(),
                ["ffmpeg-8.1.tar.xz".to_owned()],
                None,
            )
            .expect("resolve series archive");
        assert_eq!(source.version, "8.1");
        assert_eq!(source.archive_name, "ffmpeg-8.1.tar.xz");
        assert_eq!(source.expected_sha256, None);

        for names in [
            [
                "ffmpeg-8.1.tar.xz".to_owned(),
                "ffmpeg-8.1.0.tar.xz".to_owned(),
            ],
            [
                "ffmpeg-8.1.0.tar.xz".to_owned(),
                "ffmpeg-8.1.tar.xz".to_owned(),
            ],
        ] {
            let source = policy
                .resolve_build_source(&FfmpegBuildSourceInputs::default(), names, None)
                .expect("resolve deterministic patch-zero archive");
            assert_eq!(source.version, "8.1.0");
            assert_eq!(source.archive_name, "ffmpeg-8.1.0.tar.xz");
        }
    }

    #[test]
    fn remote_index_filters_other_series_and_respects_size_limit() {
        let policy = parse(TEST_FFMPEG_SOURCE_POLICY).expect("parse policy");
        let source = policy
            .resolve_build_source(
                &FfmpegBuildSourceInputs::default(),
                std::iter::empty(),
                Some(
                    "<a>ffmpeg-8.1.2.tar.xz</a> <a>ffmpeg-8.1.10.tar.xz</a> \
                     <a>ffmpeg-8.1.99.tar.xz.asc</a> \
                     <a>ffmpeg-8.1.98.tar.xz.sha256</a> \
                     <a>ffmpeg-8.2.99.tar.xz</a>",
                ),
            )
            .expect("resolve remote source");
        assert_eq!(source.version, "8.1.10");
        assert_eq!(
            latest_series_archive_version_from_index(
                "8.1",
                "<a>ffmpeg-8.1.99.tar.xz.asc</a>",
                ".tar.xz"
            ),
            None
        );

        let oversized = "x".repeat(MAX_RELEASE_INDEX_BYTES + 1);
        assert!(latest_series_archive_version_from_index("8.1", &oversized, ".tar.xz").is_none());
    }

    #[test]
    fn explicit_and_series_environment_precedence_matches_shell_contract() {
        let policy = parse(TEST_FFMPEG_SOURCE_POLICY).expect("parse policy");
        let source = policy
            .resolve_build_source(
                &FfmpegBuildSourceInputs {
                    platform_version: Some("8.1.3".to_owned()),
                    generic_version: Some("8.1.2".to_owned()),
                    platform_series: Some("8.1".to_owned()),
                    generic_series: Some("8.1".to_owned()),
                    source_url: None,
                },
                std::iter::empty(),
                None,
            )
            .expect("resolve platform exact version");
        assert_eq!(source.version, "8.1.3");
        assert_eq!(source.expected_sha256, None);

        let source = policy
            .resolve_build_source(
                &FfmpegBuildSourceInputs {
                    platform_version: None,
                    generic_version: Some("8.1.2".to_owned()),
                    platform_series: Some("8.1".to_owned()),
                    generic_series: Some("8.1".to_owned()),
                    source_url: None,
                },
                std::iter::empty(),
                None,
            )
            .expect("resolve generic exact version");
        assert_eq!(source.version, "8.1.2");
        assert_eq!(
            source.expected_sha256.as_deref(),
            Some("464beb5e7bf0c311e68b45ae2f04e9cc2af88851abb4082231742a74d97b524c")
        );

        let source = policy
            .resolve_build_source(
                &FfmpegBuildSourceInputs {
                    platform_version: None,
                    generic_version: None,
                    platform_series: Some("8.1".to_owned()),
                    generic_series: Some("8.1".to_owned()),
                    source_url: None,
                },
                ["ffmpeg-8.1.5.tar.xz".to_owned()],
                None,
            )
            .expect("resolve platform series");
        assert_eq!(source.version, "8.1.5");
    }

    #[test]
    fn release_lock_remains_exact_and_is_not_used_as_build_default() {
        let policy = parse(TEST_FFMPEG_SOURCE_POLICY).expect("parse policy");
        assert_eq!(policy.release().version(), &Version::new(8, 1, 2));
        let locked = policy.release().locked_build_source();
        assert_eq!(locked.version, "8.1.2");
        assert_eq!(locked.archive_name, "ffmpeg-8.1.2.tar.xz");
        assert_eq!(
            locked.source_url,
            "https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz"
        );
        assert_eq!(
            locked.expected_sha256.as_deref(),
            Some("464beb5e7bf0c311e68b45ae2f04e9cc2af88851abb4082231742a74d97b524c")
        );
        let source = policy
            .resolve_build_source(
                &FfmpegBuildSourceInputs::default(),
                std::iter::empty(),
                None,
            )
            .expect("resolve default build source");
        assert_eq!(source.version, "8.1");
        assert_ne!(source.version, policy.release().version().to_string());
    }
}
