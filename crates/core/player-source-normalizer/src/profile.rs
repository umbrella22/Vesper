use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    time::Duration,
};

use serde::Deserialize;

use crate::{SourceNormalizerError, SourceNormalizerResult};

/// Normalization work level supported by a source normalizer profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum NormalizeLevel {
    /// Remux/copy normalization with optional bitstream filters.
    #[serde(alias = "remux_only", alias = "RemuxOnly")]
    RemuxOnly = 1,
    /// Experimental packet repair level reserved for future work.
    ///
    /// Current implementations only guarantee remux/copy behavior; this level
    /// is parsed so profile documents can describe future capabilities without
    /// changing the wire spelling later.
    #[serde(alias = "packet_repair", alias = "PacketRepair")]
    PacketRepair = 2,
}

impl Default for NormalizeLevel {
    fn default() -> Self {
        Self::RemuxOnly
    }
}

/// Standard media output containers supported by the MVP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceNormalizerOutputContainer {
    Fmp4,
    Hls,
    ResourceUrl,
    LocalStreamEndpoint,
}

impl Default for SourceNormalizerOutputContainer {
    fn default() -> Self {
        Self::Fmp4
    }
}

/// Match rules used to select a source normalizer runtime profile.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SourceMatchRules {
    #[serde(default)]
    pub url_patterns: Vec<String>,
    #[serde(default)]
    pub mime: Vec<String>,
    #[serde(default)]
    pub byte_magic: Vec<String>,
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f32,
}

impl Default for SourceMatchRules {
    fn default() -> Self {
        Self {
            url_patterns: Vec::new(),
            mime: Vec::new(),
            byte_magic: Vec::new(),
            min_confidence: default_min_confidence(),
        }
    }
}

/// Timeout and buffering policy for a runtime profile.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SourceNormalizerRuntimePolicy {
    #[serde(default = "default_probe_timeout_ms")]
    pub probe_timeout_ms: u64,
    #[serde(default = "default_startup_buffer_bytes")]
    pub startup_buffer_bytes: u64,
    #[serde(default = "default_startup_buffer_timeout_ms")]
    pub startup_buffer_timeout_ms: u64,
    #[serde(default = "default_read_idle_timeout_ms")]
    pub read_idle_timeout_ms: u64,
    #[serde(default = "default_session_total_timeout_ms")]
    pub session_total_timeout_ms: u64,
    #[serde(default)]
    pub active_session_total_timeout_ms: Option<u64>,
    #[serde(default = "default_session_read_buffer_bytes")]
    pub session_read_buffer_bytes: u64,
    #[serde(default = "default_manifest_snapshot_bytes")]
    pub manifest_snapshot_bytes: u64,
    #[serde(default = "default_session_disk_soft_cap_bytes")]
    pub session_disk_soft_cap_bytes: u64,
    #[serde(default = "default_global_disk_soft_cap_bytes")]
    pub global_disk_soft_cap_bytes: u64,
    #[serde(default)]
    pub in_session_seek: bool,
    #[serde(default)]
    pub fallback_profile: Option<String>,
}

impl Default for SourceNormalizerRuntimePolicy {
    fn default() -> Self {
        Self {
            probe_timeout_ms: default_probe_timeout_ms(),
            startup_buffer_bytes: default_startup_buffer_bytes(),
            startup_buffer_timeout_ms: default_startup_buffer_timeout_ms(),
            read_idle_timeout_ms: default_read_idle_timeout_ms(),
            session_total_timeout_ms: default_session_total_timeout_ms(),
            active_session_total_timeout_ms: None,
            session_read_buffer_bytes: default_session_read_buffer_bytes(),
            manifest_snapshot_bytes: default_manifest_snapshot_bytes(),
            session_disk_soft_cap_bytes: default_session_disk_soft_cap_bytes(),
            global_disk_soft_cap_bytes: default_global_disk_soft_cap_bytes(),
            in_session_seek: false,
            fallback_profile: None,
        }
    }
}

impl SourceNormalizerRuntimePolicy {
    /// Active playback session total timeout.
    ///
    /// `None` and `Some(0)` both mean "unbounded"; positive values are
    /// interpreted as milliseconds.
    pub fn active_session_total_timeout(&self) -> Option<Duration> {
        self.active_session_total_timeout_ms
            .filter(|timeout_ms| *timeout_ms > 0)
            .map(Duration::from_millis)
    }
}

/// FFmpeg build capabilities required by a runtime profile.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct SourceNormalizerRequiredCapabilities {
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
    #[serde(default)]
    pub tls: Option<String>,
    #[serde(default)]
    pub network: bool,
}

/// Fully resolved runtime profile used by detection, validation, and command planning.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SourceNormalizerProfile {
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub level: NormalizeLevel,
    #[serde(default)]
    pub output_container: SourceNormalizerOutputContainer,
    #[serde(default)]
    pub seekable: bool,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub input_demuxer: Option<String>,
    #[serde(default)]
    pub bitstream_filters: Vec<String>,
    #[serde(default)]
    pub timestamp_repair: bool,
    #[serde(default)]
    pub match_rules: SourceMatchRules,
    #[serde(default)]
    /// FFmpeg input options. Inherited options use shallow key override.
    pub input_options: HashMap<String, toml::Value>,
    #[serde(default)]
    /// FFmpeg output options. Inherited options use shallow key override.
    pub output_options: HashMap<String, toml::Value>,
    #[serde(default)]
    /// FFmpeg network options. Inherited options use shallow key override.
    pub network: HashMap<String, toml::Value>,
    #[serde(default)]
    pub runtime: SourceNormalizerRuntimePolicy,
    #[serde(default)]
    pub required_capabilities: SourceNormalizerRequiredCapabilities,
}

impl SourceNormalizerProfile {
    /// Returns the runtime fallback profile for this profile, if any.
    pub fn fallback_profile(&self) -> Option<&str> {
        self.runtime.fallback_profile.as_deref()
    }
}

/// A resolved set of source normalizer runtime profiles.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceNormalizerProfileSet {
    profiles: HashMap<String, SourceNormalizerProfile>,
}

impl SourceNormalizerProfileSet {
    /// Loads and resolves runtime profiles from a TOML file.
    pub fn from_path(path: impl AsRef<Path>) -> SourceNormalizerResult<Self> {
        let path = path.as_ref();
        let content =
            fs::read_to_string(path).map_err(|source| SourceNormalizerError::ReadFile {
                path: path.to_path_buf(),
                source,
            })?;
        Self::from_toml_str(&content).map_err(|error| match error {
            SourceNormalizerError::ParseToml { source, .. } => SourceNormalizerError::ParseToml {
                path: path.to_path_buf(),
                source,
            },
            other => other,
        })
    }

    /// Parses and resolves runtime profiles from TOML text.
    pub fn from_toml_str(input: &str) -> SourceNormalizerResult<Self> {
        let document: RuntimeProfilesDocument =
            toml::from_str(input).map_err(|source| SourceNormalizerError::ParseToml {
                path: Path::new("<inline>").to_path_buf(),
                source,
            })?;

        let raw_profiles = document.runtime.source_normalizer.ok_or_else(|| {
            SourceNormalizerError::InvalidRuntimeProfile {
                profile: "runtime.source-normalizer".to_owned(),
                message: "missing [runtime.source-normalizer] table".to_owned(),
            }
        })?;

        let mut resolved = HashMap::new();
        for name in raw_profiles.keys() {
            let mut stack = Vec::new();
            let profile = resolve_profile(name, &raw_profiles, &mut resolved, &mut stack)?;
            resolved.insert(name.clone(), profile);
        }

        Ok(Self { profiles: resolved })
    }

    /// Returns a profile by name.
    pub fn get(&self, name: &str) -> Option<&SourceNormalizerProfile> {
        self.profiles.get(name)
    }

    /// Returns a profile by name or an error.
    pub fn require(&self, name: &str) -> SourceNormalizerResult<&SourceNormalizerProfile> {
        self.get(name)
            .ok_or_else(|| SourceNormalizerError::UnknownRuntimeProfile {
                profile: name.to_owned(),
            })
    }

    /// Returns sorted profile names.
    pub fn names(&self) -> Vec<&str> {
        let mut names = self.profiles.keys().map(String::as_str).collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    /// Returns profiles sorted by priority descending and name ascending.
    pub fn profiles_by_priority(&self) -> Vec<(&str, &SourceNormalizerProfile)> {
        let mut profiles = self
            .profiles
            .iter()
            .map(|(name, profile)| (name.as_str(), profile))
            .collect::<Vec<_>>();
        profiles.sort_by(|(left_name, left), (right_name, right)| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left_name.cmp(right_name))
        });
        profiles
    }
}

#[derive(Debug, Deserialize)]
struct RuntimeProfilesDocument {
    runtime: RuntimeProfilesRoot,
}

#[derive(Debug, Deserialize)]
struct RuntimeProfilesRoot {
    #[serde(rename = "source-normalizer")]
    source_normalizer: Option<HashMap<String, RawSourceNormalizerProfile>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawSourceNormalizerProfile {
    extends: Option<String>,
    level: Option<NormalizeLevel>,
    output_container: Option<SourceNormalizerOutputContainer>,
    seekable: Option<bool>,
    priority: Option<i32>,
    input_demuxer: Option<String>,
    bitstream_filters: Option<Vec<String>>,
    timestamp_repair: Option<bool>,
    #[serde(rename = "match")]
    match_rules: Option<SourceMatchRules>,
    input_options: Option<HashMap<String, toml::Value>>,
    output_options: Option<HashMap<String, toml::Value>>,
    network: Option<HashMap<String, toml::Value>>,
    runtime: Option<SourceNormalizerRuntimePolicy>,
    required_capabilities: Option<SourceNormalizerRequiredCapabilities>,
}

fn resolve_profile(
    name: &str,
    raw_profiles: &HashMap<String, RawSourceNormalizerProfile>,
    resolved: &mut HashMap<String, SourceNormalizerProfile>,
    stack: &mut Vec<String>,
) -> SourceNormalizerResult<SourceNormalizerProfile> {
    if let Some(profile) = resolved.get(name) {
        return Ok(profile.clone());
    }
    let raw =
        raw_profiles
            .get(name)
            .ok_or_else(|| SourceNormalizerError::UnknownRuntimeProfile {
                profile: name.to_owned(),
            })?;

    if stack.iter().any(|entry| entry == name) {
        let mut chain = stack.clone();
        chain.push(name.to_owned());
        return Err(SourceNormalizerError::RuntimeProfileCycle {
            chain: chain.join(" -> "),
        });
    }

    stack.push(name.to_owned());
    let mut profile = if let Some(parent) = &raw.extends {
        resolve_profile(parent, raw_profiles, resolved, stack)?
    } else {
        SourceNormalizerProfile::default()
    };
    apply_raw_profile(&mut profile, raw);
    if let Some(fallback) = profile.fallback_profile()
        && !raw_profiles.contains_key(fallback)
    {
        let chain = stack.join(" -> ");
        return Err(SourceNormalizerError::InvalidRuntimeProfile {
            profile: name.to_owned(),
            message: format!("fallback profile `{fallback}` does not exist in `{chain}`"),
        });
    }
    stack.pop();

    validate_profile(name, &profile)?;
    resolved.insert(name.to_owned(), profile.clone());
    Ok(profile)
}

fn apply_raw_profile(profile: &mut SourceNormalizerProfile, raw: &RawSourceNormalizerProfile) {
    if let Some(extends) = &raw.extends {
        profile.extends = Some(extends.clone());
    }
    if let Some(level) = raw.level {
        profile.level = level;
    }
    if let Some(output_container) = raw.output_container {
        profile.output_container = output_container;
    }
    if let Some(seekable) = raw.seekable {
        profile.seekable = seekable;
    }
    if let Some(priority) = raw.priority {
        profile.priority = priority;
    }
    if let Some(input_demuxer) = &raw.input_demuxer {
        profile.input_demuxer = Some(input_demuxer.clone());
    }
    if let Some(bitstream_filters) = &raw.bitstream_filters {
        profile.bitstream_filters = bitstream_filters.clone();
    }
    if let Some(timestamp_repair) = raw.timestamp_repair {
        profile.timestamp_repair = timestamp_repair;
    }
    if let Some(match_rules) = &raw.match_rules {
        profile.match_rules = match_rules.clone();
    }
    merge_table(&mut profile.input_options, &raw.input_options);
    merge_table(&mut profile.output_options, &raw.output_options);
    merge_table(&mut profile.network, &raw.network);
    if let Some(runtime) = &raw.runtime {
        profile.runtime = runtime.clone();
    }
    if let Some(required_capabilities) = &raw.required_capabilities {
        profile.required_capabilities =
            merge_required_capabilities(&profile.required_capabilities, required_capabilities);
    }
}

fn validate_profile(name: &str, profile: &SourceNormalizerProfile) -> SourceNormalizerResult<()> {
    for magic in &profile.match_rules.byte_magic {
        validate_byte_magic(name, magic)?;
    }
    Ok(())
}

fn validate_byte_magic(profile: &str, magic: &str) -> SourceNormalizerResult<()> {
    let normalized = magic
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if normalized.is_empty() {
        return Err(SourceNormalizerError::InvalidRuntimeProfile {
            profile: profile.to_owned(),
            message: "byte magic must not be empty".to_owned(),
        });
    }
    if normalized.len() % 2 != 0 {
        return Err(SourceNormalizerError::InvalidRuntimeProfile {
            profile: profile.to_owned(),
            message: format!("byte magic `{magic}` must contain complete hex bytes"),
        });
    }
    if !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(SourceNormalizerError::InvalidRuntimeProfile {
            profile: profile.to_owned(),
            message: format!("byte magic `{magic}` must be hexadecimal"),
        });
    }
    Ok(())
}

fn merge_table(
    target: &mut HashMap<String, toml::Value>,
    source: &Option<HashMap<String, toml::Value>>,
) {
    if let Some(source) = source {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn merge_required_capabilities(
    parent: &SourceNormalizerRequiredCapabilities,
    child: &SourceNormalizerRequiredCapabilities,
) -> SourceNormalizerRequiredCapabilities {
    SourceNormalizerRequiredCapabilities {
        libraries: merge_unique(&parent.libraries, &child.libraries),
        demuxers: merge_unique(&parent.demuxers, &child.demuxers),
        muxers: merge_unique(&parent.muxers, &child.muxers),
        protocols: merge_unique(&parent.protocols, &child.protocols),
        parsers: merge_unique(&parent.parsers, &child.parsers),
        bsfs: merge_unique(&parent.bsfs, &child.bsfs),
        tls: child.tls.clone().or_else(|| parent.tls.clone()),
        network: parent.network || child.network,
    }
}

fn merge_unique(parent: &[String], child: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    for value in parent.iter().chain(child) {
        if seen.insert(value.clone()) {
            merged.push(value.clone());
        }
    }
    merged
}

impl Default for SourceNormalizerProfile {
    fn default() -> Self {
        Self {
            extends: None,
            level: NormalizeLevel::RemuxOnly,
            output_container: SourceNormalizerOutputContainer::Fmp4,
            seekable: false,
            priority: 0,
            input_demuxer: None,
            bitstream_filters: Vec::new(),
            timestamp_repair: false,
            match_rules: SourceMatchRules::default(),
            input_options: HashMap::new(),
            output_options: HashMap::new(),
            network: HashMap::new(),
            runtime: SourceNormalizerRuntimePolicy::default(),
            required_capabilities: SourceNormalizerRequiredCapabilities::default(),
        }
    }
}

fn default_min_confidence() -> f32 {
    0.5
}

fn default_probe_timeout_ms() -> u64 {
    30_000
}

fn default_startup_buffer_bytes() -> u64 {
    1_048_576
}

fn default_startup_buffer_timeout_ms() -> u64 {
    5_000
}

fn default_read_idle_timeout_ms() -> u64 {
    10_000
}

fn default_session_total_timeout_ms() -> u64 {
    40_000
}

fn default_session_read_buffer_bytes() -> u64 {
    4 * 1024 * 1024
}

fn default_manifest_snapshot_bytes() -> u64 {
    512 * 1024
}

fn default_session_disk_soft_cap_bytes() -> u64 {
    512 * 1024 * 1024
}

fn default_global_disk_soft_cap_bytes() -> u64 {
    1536 * 1024 * 1024
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_PROFILE_TOML: &str =
        include_str!("../../../../scripts/source-normalizer-profiles.toml");

    const SAMPLE: &str = r#"
[runtime.source-normalizer.base]
level = "remux_only"
output_container = "fmp4"
priority = 0

[runtime.source-normalizer.base.runtime]
fallback_profile = "generic-fallback"

[runtime.source-normalizer.base.required_capabilities]
libraries = ["avformat"]
protocols = ["file", "pipe"]

[runtime.source-normalizer.flv]
extends = "base"
priority = 20
input_demuxer = "flv"
bitstream_filters = ["aac_adtstoasc"]

[runtime.source-normalizer.flv.match]
url_patterns = ["*.flv"]
mime = ["video/x-flv"]
min_confidence = 0.75

[runtime.source-normalizer.flv.required_capabilities]
demuxers = ["flv"]
protocols = ["http"]
network = true

[runtime.source-normalizer.generic-fallback]
extends = "base"
priority = -1
"#;

    #[test]
    fn parses_and_resolves_inheritance() {
        let profiles = SourceNormalizerProfileSet::from_toml_str(SAMPLE).expect("profiles");
        let flv = profiles.require("flv").expect("flv");

        assert_eq!(flv.priority, 20);
        assert_eq!(flv.input_demuxer.as_deref(), Some("flv"));
        assert_eq!(
            flv.runtime.fallback_profile.as_deref(),
            Some("generic-fallback")
        );
        assert_eq!(
            flv.required_capabilities.protocols,
            vec!["file", "pipe", "http"]
        );
        assert!(flv.required_capabilities.network);
    }

    #[test]
    fn normalize_level_toml_names_are_stable() {
        let profiles = SourceNormalizerProfileSet::from_toml_str(
            r#"
[runtime.source-normalizer.remux]
level = "remux_only"

[runtime.source-normalizer.repair]
level = "packet_repair"
"#,
        )
        .expect("profiles");

        assert_eq!(
            profiles.require("remux").expect("remux").level,
            NormalizeLevel::RemuxOnly
        );
        assert_eq!(
            profiles.require("repair").expect("repair").level,
            NormalizeLevel::PacketRepair
        );
    }

    #[test]
    fn option_tables_use_shallow_override_for_inheritance() {
        let profiles = SourceNormalizerProfileSet::from_toml_str(
            r#"
[runtime.source-normalizer.parent]

[runtime.source-normalizer.parent.input_options]
shared = "parent"
parent_only = "kept"
nested = { parent = true, keep = true }

[runtime.source-normalizer.child]
extends = "parent"

[runtime.source-normalizer.child.input_options]
shared = "child"
child_only = "added"
nested = { child = true }
"#,
        )
        .expect("profiles");
        let child = profiles.require("child").expect("child");

        assert_eq!(
            child.input_options.get("shared"),
            Some(&toml::Value::String("child".to_owned()))
        );
        assert_eq!(
            child.input_options.get("parent_only"),
            Some(&toml::Value::String("kept".to_owned()))
        );
        assert_eq!(
            child.input_options.get("child_only"),
            Some(&toml::Value::String("added".to_owned()))
        );
        assert_eq!(
            child
                .input_options
                .get("nested")
                .and_then(toml::Value::as_table)
                .and_then(|table| table.get("parent")),
            None
        );
        assert_eq!(
            child
                .input_options
                .get("nested")
                .and_then(toml::Value::as_table)
                .and_then(|table| table.get("child")),
            Some(&toml::Value::Boolean(true))
        );
    }

    #[test]
    fn detects_inheritance_cycle() {
        let input = r#"
[runtime.source-normalizer.a]
extends = "b"

[runtime.source-normalizer.b]
extends = "a"
"#;
        let error = SourceNormalizerProfileSet::from_toml_str(input).expect_err("cycle");
        assert!(matches!(
            error,
            SourceNormalizerError::RuntimeProfileCycle { .. }
        ));
    }

    #[test]
    fn validates_fallback_profile_exists() {
        let input = r#"
[runtime.source-normalizer.base]

[runtime.source-normalizer.base.runtime]
fallback_profile = "missing"
"#;
        let error = SourceNormalizerProfileSet::from_toml_str(input).expect_err("fallback");
        assert!(matches!(
            error,
            SourceNormalizerError::InvalidRuntimeProfile { .. }
        ));
    }

    #[test]
    fn validates_inherited_fallback_profile_exists_with_context() {
        let input = r#"
[runtime.source-normalizer.base]

[runtime.source-normalizer.base.runtime]
fallback_profile = "missing"

[runtime.source-normalizer.child]
extends = "base"
"#;
        let error = SourceNormalizerProfileSet::from_toml_str(input).expect_err("fallback");
        assert!(matches!(
            error,
            SourceNormalizerError::InvalidRuntimeProfile { ref profile, ref message }
                if profile == "base" && message.contains("base")
        ));
    }

    #[test]
    fn validates_byte_magic_hex_at_profile_load_time() {
        let input = r#"
[runtime.source-normalizer.flv]

[runtime.source-normalizer.flv.match]
byte_magic = ["46 4c 5"]
"#;
        let error = SourceNormalizerProfileSet::from_toml_str(input).expect_err("byte magic");
        assert!(matches!(
            error,
            SourceNormalizerError::InvalidRuntimeProfile { ref profile, ref message }
                if profile == "flv" && message.contains("complete hex bytes")
        ));
    }

    #[test]
    fn profiles_sort_by_priority_then_name() {
        let profiles = SourceNormalizerProfileSet::from_toml_str(SAMPLE).expect("profiles");
        let names = profiles
            .profiles_by_priority()
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["flv", "base", "generic-fallback"]);
    }

    #[test]
    fn default_source_normalizer_profiles_resolve_expected_semantics() {
        let profiles =
            SourceNormalizerProfileSet::from_toml_str(DEFAULT_PROFILE_TOML).expect("profiles");

        let base = profiles.require("base").expect("base");
        assert_eq!(base.level, NormalizeLevel::RemuxOnly);
        assert_eq!(base.output_container, SourceNormalizerOutputContainer::Fmp4);
        assert_eq!(base.runtime.probe_timeout_ms, 30_000);
        assert_eq!(
            base.runtime.fallback_profile.as_deref(),
            Some("generic-fallback")
        );
        assert_eq!(
            base.input_options.get("probesize"),
            Some(&toml::Value::Integer(5_000_000))
        );
        assert_eq!(
            base.network.get("timeout"),
            Some(&toml::Value::Integer(10_000_000))
        );
        assert!(!base.required_capabilities.network);

        let flv = profiles.require("flv").expect("flv");
        assert_eq!(flv.input_demuxer.as_deref(), Some("flv"));
        assert_eq!(
            flv.network.get("reconnect_at_eof"),
            Some(&toml::Value::Boolean(true))
        );
        assert_eq!(
            flv.output_options.get("movflags"),
            Some(&toml::Value::String(
                "+frag_keyframe+empty_moov+default_base_moof".to_owned()
            ))
        );
        assert_eq!(flv.runtime.active_session_total_timeout_ms, Some(0));
        assert_eq!(flv.runtime.session_read_buffer_bytes, 4 * 1024 * 1024);
        assert_eq!(flv.runtime.manifest_snapshot_bytes, 512 * 1024);
        assert_eq!(flv.runtime.session_disk_soft_cap_bytes, 512 * 1024 * 1024);
        assert_eq!(flv.runtime.global_disk_soft_cap_bytes, 1536 * 1024 * 1024);
        assert!(flv.required_capabilities.network);
        assert!(
            flv.required_capabilities
                .demuxers
                .contains(&"flv".to_owned())
        );
        assert!(
            flv.required_capabilities
                .protocols
                .contains(&"https".to_owned())
        );
        assert_eq!(
            flv.runtime.fallback_profile.as_deref(),
            Some("generic-fallback")
        );

        let hevc = profiles.require("bilibili-hevc-flv").expect("hevc");
        assert_eq!(
            hevc.input_options.get("analyzeduration"),
            Some(&toml::Value::Integer(50_000_000))
        );
        assert_eq!(
            hevc.input_options.get("probesize"),
            Some(&toml::Value::Integer(10_000_000))
        );
        assert_eq!(
            hevc.output_options.get("movflags"),
            Some(&toml::Value::String(
                "+frag_keyframe+empty_moov+default_base_moof".to_owned()
            ))
        );

        let hls = profiles.require("hls-nonstandard").expect("hls");
        assert_eq!(hls.output_container, SourceNormalizerOutputContainer::Hls);
        assert_eq!(
            hls.output_options.get("hls_list_size"),
            Some(&toml::Value::Integer(6))
        );
        assert_eq!(
            hls.output_options.get("hls_flags"),
            Some(&toml::Value::String(
                "delete_segments+append_list+omit_endlist+independent_segments".to_owned()
            ))
        );
        assert_eq!(hls.runtime.active_session_total_timeout_ms, Some(0));
        assert!(hls.required_capabilities.network);
        assert!(
            hls.required_capabilities
                .protocols
                .contains(&"crypto".to_owned())
        );

        let dash = profiles.require("dash-weird").expect("dash");
        assert_eq!(dash.output_container, SourceNormalizerOutputContainer::Hls);
        assert!(dash.required_capabilities.network);
        assert_eq!(
            dash.input_options.get("allowed_extensions"),
            Some(&toml::Value::String(".mpd".to_owned()))
        );

        let standalone = SourceNormalizerProfile::default();
        assert_eq!(standalone.runtime, SourceNormalizerRuntimePolicy::default());
    }

    #[test]
    fn active_session_total_timeout_ms_none_is_unbounded() {
        let policy = SourceNormalizerRuntimePolicy {
            active_session_total_timeout_ms: None,
            ..SourceNormalizerRuntimePolicy::default()
        };

        assert_eq!(policy.active_session_total_timeout(), None);
    }

    #[test]
    fn active_session_total_timeout_ms_zero_is_unbounded() {
        let policy = SourceNormalizerRuntimePolicy {
            active_session_total_timeout_ms: Some(0),
            ..SourceNormalizerRuntimePolicy::default()
        };

        assert_eq!(policy.active_session_total_timeout(), None);
    }

    #[test]
    fn active_session_total_timeout_ms_positive_sets_deadline() {
        let policy = SourceNormalizerRuntimePolicy {
            active_session_total_timeout_ms: Some(1),
            ..SourceNormalizerRuntimePolicy::default()
        };

        assert_eq!(
            policy.active_session_total_timeout(),
            Some(Duration::from_millis(1))
        );
    }
}
