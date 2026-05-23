use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use serde::Deserialize;

use crate::{SourceNormalizerError, SourceNormalizerResult};

/// Validation policy attached to an FFmpeg build profile.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct FfmpegBuildValidation {
    #[serde(default)]
    pub forbid_network: bool,
    #[serde(default)]
    pub forbid_openssl: bool,
}

/// Resolved FFmpeg build profile capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct FfmpegBuildProfile {
    /// Parent profile names inherited by this resolved profile.
    ///
    /// TOML accepts either `extends = "base"` or `extends = ["base", "network"]`;
    /// the resolved value is always a list so multi-parent build profiles keep
    /// their merge order explicit.
    #[serde(default)]
    pub extends: Vec<String>,
    #[serde(default)]
    pub libraries: Vec<String>,
    #[serde(default)]
    pub demuxers: Vec<String>,
    #[serde(default)]
    pub muxers: Vec<String>,
    #[serde(default)]
    pub protocols: Vec<String>,
    #[serde(default)]
    pub parsers: Vec<String>,
    #[serde(default)]
    pub bsfs: Vec<String>,
    #[serde(default = "default_tls")]
    pub tls: String,
    #[serde(default)]
    pub validation: FfmpegBuildValidation,
}

impl FfmpegBuildProfile {
    /// Returns whether the profile has an enabled network protocol.
    pub fn enables_network_protocol(&self) -> bool {
        self.protocols
            .iter()
            .any(|protocol| is_network_protocol(protocol))
    }
}

/// A resolved set of FFmpeg build profiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegBuildProfileSet {
    profiles: HashMap<String, FfmpegBuildProfile>,
}

impl FfmpegBuildProfileSet {
    /// Loads and resolves FFmpeg build profiles from a TOML file.
    pub fn from_path(path: impl AsRef<Path>, platform: &str) -> SourceNormalizerResult<Self> {
        let path = path.as_ref();
        let content =
            fs::read_to_string(path).map_err(|source| SourceNormalizerError::ReadFile {
                path: path.to_path_buf(),
                source,
            })?;
        Self::from_toml_str(&content, platform).map_err(|error| match error {
            SourceNormalizerError::ParseToml { source, .. } => SourceNormalizerError::ParseToml {
                path: path.to_path_buf(),
                source,
            },
            other => other,
        })
    }

    /// Parses and resolves FFmpeg build profiles from TOML text for a platform.
    pub fn from_toml_str(input: &str, platform: &str) -> SourceNormalizerResult<Self> {
        let document: FfmpegProfilesDocument =
            toml::from_str(input).map_err(|source| SourceNormalizerError::ParseToml {
                path: Path::new("<inline>").to_path_buf(),
                source,
            })?;
        let raw_profiles = document.profile;
        let mut resolved = HashMap::new();

        for name in raw_profiles.keys() {
            let mut stack = Vec::new();
            let profile =
                resolve_profile(name, platform, &raw_profiles, &mut resolved, &mut stack)?;
            resolved.insert(name.clone(), profile);
        }

        Ok(Self { profiles: resolved })
    }

    /// Returns a build profile by name.
    pub fn get(&self, name: &str) -> Option<&FfmpegBuildProfile> {
        self.profiles.get(name)
    }

    /// Returns a build profile by name or an error.
    pub fn require(&self, name: &str) -> SourceNormalizerResult<&FfmpegBuildProfile> {
        self.get(name)
            .ok_or_else(|| SourceNormalizerError::UnknownFfmpegProfile {
                profile: name.to_owned(),
            })
    }
}

#[derive(Debug, Deserialize)]
struct FfmpegProfilesDocument {
    #[serde(default)]
    profile: HashMap<String, RawFfmpegBuildProfile>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawFfmpegBuildProfile {
    #[serde(default, deserialize_with = "deserialize_extends")]
    extends: Vec<String>,
    #[serde(default)]
    libraries: Vec<String>,
    #[serde(default)]
    demuxers: Vec<String>,
    #[serde(default)]
    muxers: Vec<String>,
    #[serde(default)]
    protocols: Vec<String>,
    #[serde(default)]
    parsers: Vec<String>,
    #[serde(default)]
    bsfs: Vec<String>,
    tls: Option<String>,
    #[serde(default)]
    validation: FfmpegBuildValidation,
    #[serde(default)]
    platform_overrides: HashMap<String, RawFfmpegBuildProfileOverride>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawFfmpegBuildProfileOverride {
    #[serde(default)]
    libraries: Vec<String>,
    #[serde(default)]
    demuxers: Vec<String>,
    #[serde(default)]
    muxers: Vec<String>,
    #[serde(default)]
    protocols: Vec<String>,
    #[serde(default)]
    parsers: Vec<String>,
    #[serde(default)]
    bsfs: Vec<String>,
    tls: Option<String>,
    #[serde(default)]
    validation: FfmpegBuildValidation,
}

fn resolve_profile(
    name: &str,
    platform: &str,
    raw_profiles: &HashMap<String, RawFfmpegBuildProfile>,
    resolved: &mut HashMap<String, FfmpegBuildProfile>,
    stack: &mut Vec<String>,
) -> SourceNormalizerResult<FfmpegBuildProfile> {
    if let Some(profile) = resolved.get(name) {
        return Ok(profile.clone());
    }
    let raw =
        raw_profiles
            .get(name)
            .ok_or_else(|| SourceNormalizerError::UnknownFfmpegProfile {
                profile: name.to_owned(),
            })?;

    if stack.iter().any(|entry| entry == name) {
        let mut chain = stack.clone();
        chain.push(name.to_owned());
        return Err(SourceNormalizerError::FfmpegProfileCycle {
            chain: chain.join(" -> "),
        });
    }

    stack.push(name.to_owned());
    let mut profile = FfmpegBuildProfile::default();
    for parent in &raw.extends {
        let parent = resolve_profile(parent, platform, raw_profiles, resolved, stack)?;
        merge_profile(&mut profile, &parent);
    }
    apply_raw_profile(&mut profile, raw);
    if let Some(override_profile) = raw.platform_overrides.get(platform) {
        apply_raw_override(&mut profile, override_profile);
    }
    if profile.tls.is_empty() {
        profile.tls = default_tls();
    }
    stack.pop();

    resolved.insert(name.to_owned(), profile.clone());
    Ok(profile)
}

fn merge_profile(target: &mut FfmpegBuildProfile, source: &FfmpegBuildProfile) {
    append_unique_all(&mut target.extends, &source.extends);
    append_unique_all(&mut target.libraries, &source.libraries);
    append_unique_all(&mut target.demuxers, &source.demuxers);
    append_unique_all(&mut target.muxers, &source.muxers);
    append_unique_all(&mut target.protocols, &source.protocols);
    append_unique_all(&mut target.parsers, &source.parsers);
    append_unique_all(&mut target.bsfs, &source.bsfs);
    if target.tls.is_empty() || target.tls == "none" {
        target.tls = source.tls.clone();
    }
    target.validation.forbid_network |= source.validation.forbid_network;
    target.validation.forbid_openssl |= source.validation.forbid_openssl;
}

fn apply_raw_profile(target: &mut FfmpegBuildProfile, raw: &RawFfmpegBuildProfile) {
    append_unique_all(&mut target.extends, &raw.extends);
    append_unique_all(&mut target.libraries, &raw.libraries);
    append_unique_all(&mut target.demuxers, &raw.demuxers);
    append_unique_all(&mut target.muxers, &raw.muxers);
    append_unique_all(&mut target.protocols, &raw.protocols);
    append_unique_all(&mut target.parsers, &raw.parsers);
    append_unique_all(&mut target.bsfs, &raw.bsfs);
    if let Some(tls) = &raw.tls {
        target.tls = tls.clone();
    }
    target.validation.forbid_network |= raw.validation.forbid_network;
    target.validation.forbid_openssl |= raw.validation.forbid_openssl;
}

fn apply_raw_override(target: &mut FfmpegBuildProfile, raw: &RawFfmpegBuildProfileOverride) {
    append_unique_all(&mut target.libraries, &raw.libraries);
    append_unique_all(&mut target.demuxers, &raw.demuxers);
    append_unique_all(&mut target.muxers, &raw.muxers);
    append_unique_all(&mut target.protocols, &raw.protocols);
    append_unique_all(&mut target.parsers, &raw.parsers);
    append_unique_all(&mut target.bsfs, &raw.bsfs);
    if let Some(tls) = &raw.tls {
        target.tls = tls.clone();
    }
    target.validation.forbid_network |= raw.validation.forbid_network;
    target.validation.forbid_openssl |= raw.validation.forbid_openssl;
}

fn append_unique_all(target: &mut Vec<String>, source: &[String]) {
    let mut seen = target.iter().cloned().collect::<HashSet<_>>();
    for value in source {
        if seen.insert(value.clone()) {
            target.push(value.clone());
        }
    }
}

fn is_network_protocol(protocol: &str) -> bool {
    matches!(
        protocol,
        "http" | "https" | "tcp" | "tls" | "udp" | "rtmp" | "rtmps" | "rtsp"
    )
}

fn default_tls() -> String {
    "none".to_owned()
}

fn deserialize_extends<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Extends {
        One(String),
        Many(Vec<String>),
    }

    let value = Option::<Extends>::deserialize(deserializer)?;
    Ok(match value {
        Some(Extends::One(value)) => vec![value],
        Some(Extends::Many(values)) => values,
        None => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[profile.base]
libraries = ["avcodec"]
protocols = ["file"]

[profile.remux]
extends = "base"
libraries = ["avformat"]
protocols = ["pipe"]

[profile.remux.validation]
forbid_network = true

[profile.remux.platform_overrides.ios]
demuxers = ["mov"]
protocols = ["data"]
"#;

    #[test]
    fn resolves_build_profile_inheritance_and_platform_overrides() {
        let profiles = FfmpegBuildProfileSet::from_toml_str(SAMPLE, "ios").expect("profiles");
        let remux = profiles.require("remux").expect("remux");

        assert_eq!(remux.libraries, vec!["avcodec", "avformat"]);
        assert_eq!(remux.protocols, vec!["file", "pipe", "data"]);
        assert_eq!(remux.demuxers, vec!["mov"]);
        assert!(remux.validation.forbid_network);
    }

    #[test]
    fn detects_build_profile_cycles() {
        let input = r#"
[profile.a]
extends = "b"

[profile.b]
extends = "a"
"#;
        let error = FfmpegBuildProfileSet::from_toml_str(input, "android").expect_err("cycle");
        assert!(matches!(
            error,
            SourceNormalizerError::FfmpegProfileCycle { .. }
        ));
    }
}
