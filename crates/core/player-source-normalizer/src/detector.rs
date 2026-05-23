use crate::{SourceNormalizerProfile, SourceNormalizerProfileSet};

/// Probe context supplied to source detectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeContext {
    pub url: String,
    pub mime: Option<String>,
    pub headers: Vec<(String, String)>,
    pub timeout_ms: u64,
}

/// Runtime profile candidate produced by source detection.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeProfileCandidate {
    pub runtime_profile: String,
    pub confidence: f32,
    pub required_capabilities: crate::SourceNormalizerRequiredCapabilities,
    pub diagnostic_reason: String,
}

/// Probe result returned by source detectors.
#[derive(Debug, Clone, PartialEq)]
pub enum ProbeResult {
    Candidate(RuntimeProfileCandidate),
    NoCandidate { diagnostic_reason: String },
    Unsupported { diagnostic_reason: String },
}

/// Source detectors only propose runtime profiles.
///
/// They do not select a decoder, presenter, frame processor, or plugin path.
pub trait SourceDetector: Send + Sync {
    fn id(&self) -> &'static str;

    fn probe_url(&self, context: &ProbeContext) -> ProbeResult;

    fn probe_headers(&self, context: &ProbeContext) -> ProbeResult;

    fn probe_bytes(&self, context: &ProbeContext, header: &[u8]) -> ProbeResult;
}

/// Profile-backed detector for URL, MIME, and byte magic hints.
#[derive(Debug, Clone)]
pub struct SourceRuntimeDetector {
    profiles: SourceNormalizerProfileSet,
}

impl SourceRuntimeDetector {
    /// Creates a detector from resolved runtime profiles.
    pub fn new(profiles: SourceNormalizerProfileSet) -> Self {
        Self { profiles }
    }

    /// Returns candidates sorted by confidence, priority, and profile name.
    pub fn probe_candidates(
        &self,
        context: &ProbeContext,
        header: Option<&[u8]>,
    ) -> Vec<RuntimeProfileCandidate> {
        let mut candidates = Vec::new();
        for (name, profile) in self.profiles.profiles_by_priority() {
            if let Some(candidate) = match_profile(name, profile, context, header) {
                candidates.push((profile.priority, candidate));
            }
        }

        candidates.sort_by(|(left_priority, left), (right_priority, right)| {
            right
                .confidence
                .partial_cmp(&left.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right_priority.cmp(left_priority))
                .then_with(|| left.runtime_profile.cmp(&right.runtime_profile))
        });
        candidates
            .into_iter()
            .map(|(_, candidate)| candidate)
            .collect()
    }
}

impl SourceDetector for SourceRuntimeDetector {
    fn id(&self) -> &'static str {
        "source-runtime-profile-detector"
    }

    fn probe_url(&self, context: &ProbeContext) -> ProbeResult {
        match self.probe_candidates(context, None).into_iter().next() {
            Some(candidate) => ProbeResult::Candidate(candidate),
            None => ProbeResult::NoCandidate {
                diagnostic_reason: "no runtime profile matched URL hints".to_owned(),
            },
        }
    }

    fn probe_headers(&self, context: &ProbeContext) -> ProbeResult {
        // Header probing currently uses the normalized hints in ProbeContext,
        // including MIME. This remains the extension point for future
        // header-specific filtering without changing detector callers.
        self.probe_url(context)
    }

    fn probe_bytes(&self, context: &ProbeContext, header: &[u8]) -> ProbeResult {
        match self
            .probe_candidates(context, Some(header))
            .into_iter()
            .next()
        {
            Some(candidate) => ProbeResult::Candidate(candidate),
            None => ProbeResult::NoCandidate {
                diagnostic_reason: "no runtime profile matched byte hints".to_owned(),
            },
        }
    }
}

fn match_profile(
    name: &str,
    profile: &SourceNormalizerProfile,
    context: &ProbeContext,
    header: Option<&[u8]>,
) -> Option<RuntimeProfileCandidate> {
    let mut confidence = 0.0_f32;
    let mut reasons = Vec::new();
    let rules = &profile.match_rules;

    if let Some(pattern) = rules
        .url_patterns
        .iter()
        .find(|pattern| wildcard_match(pattern, &context.url))
    {
        confidence += 0.45;
        reasons.push(format!("URL pattern matched: {pattern}"));
    }

    if let Some(mime) = &context.mime {
        if !rules.mime.is_empty()
            && rules
                .mime
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(mime))
        {
            confidence += 0.35;
            reasons.push("MIME type matched".to_owned());
        }
    }

    if let Some(header) = header {
        if let Some(magic) = rules
            .byte_magic
            .iter()
            .find(|magic| byte_magic_matches(magic, header))
        {
            confidence += 0.45;
            reasons.push(format!("byte magic matched: {magic}"));
        }
    }

    if confidence >= rules.min_confidence {
        Some(RuntimeProfileCandidate {
            runtime_profile: name.to_owned(),
            confidence,
            required_capabilities: profile.required_capabilities.clone(),
            diagnostic_reason: reasons.join("; "),
        })
    } else {
        None
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase().chars().collect::<Vec<_>>();
    let value = value.to_ascii_lowercase().chars().collect::<Vec<_>>();
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut star_index = None;
    let mut star_match_index = 0;

    while value_index < value.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == value[value_index] {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star_index = Some(pattern_index);
            star_match_index = value_index;
            pattern_index += 1;
        } else if let Some(previous_star_index) = star_index {
            pattern_index = previous_star_index + 1;
            star_match_index += 1;
            value_index = star_match_index;
        } else {
            return false;
        }
    }

    pattern[pattern_index..]
        .iter()
        .all(|candidate| *candidate == '*')
}

fn byte_magic_matches(magic: &str, header: &[u8]) -> bool {
    let normalized = magic
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if normalized.len() % 2 != 0 {
        return false;
    }
    let bytes = (0..normalized.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&normalized[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>();
    let Ok(bytes) = bytes else {
        return false;
    };
    header.starts_with(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourceNormalizerProfileSet;

    #[test]
    fn probes_candidates_by_url_mime_and_priority() {
        let profiles = SourceNormalizerProfileSet::from_toml_str(
            r#"
[runtime.source-normalizer.generic-fallback]
priority = -1

[runtime.source-normalizer.generic-fallback.match]
url_patterns = ["*"]
min_confidence = 0.1

[runtime.source-normalizer.flv]
priority = 20

[runtime.source-normalizer.flv.match]
url_patterns = ["*.flv"]
mime = ["video/x-flv"]
min_confidence = 0.75
"#,
        )
        .expect("profiles");
        let detector = SourceRuntimeDetector::new(profiles);
        let candidates = detector.probe_candidates(
            &ProbeContext {
                url: "https://example.test/live.flv".to_owned(),
                mime: Some("video/x-flv".to_owned()),
                headers: Vec::new(),
                timeout_ms: 1_000,
            },
            None,
        );

        assert_eq!(candidates[0].runtime_profile, "flv");
        assert!(candidates[0].confidence >= 0.75);
    }

    #[test]
    fn probes_byte_magic() {
        let profiles = SourceNormalizerProfileSet::from_toml_str(
            r#"
[runtime.source-normalizer.flv]

[runtime.source-normalizer.flv.match]
byte_magic = ["464c56"]
min_confidence = 0.4
"#,
        )
        .expect("profiles");
        let detector = SourceRuntimeDetector::new(profiles);
        let result = detector.probe_bytes(
            &ProbeContext {
                url: "file:///tmp/input.bin".to_owned(),
                mime: None,
                headers: Vec::new(),
                timeout_ms: 1_000,
            },
            b"FLV\x01",
        );

        assert!(matches!(result, ProbeResult::Candidate(_)));
    }

    #[test]
    fn probes_headers_with_mime_only_context() {
        let profiles = SourceNormalizerProfileSet::from_toml_str(
            r#"
[runtime.source-normalizer.flv]

[runtime.source-normalizer.flv.match]
mime = ["video/x-flv"]
min_confidence = 0.3
"#,
        )
        .expect("profiles");
        let detector = SourceRuntimeDetector::new(profiles);
        let result = detector.probe_headers(&ProbeContext {
            url: "https://example.test/stream".to_owned(),
            mime: Some("video/x-flv".to_owned()),
            headers: vec![("content-type".to_owned(), "video/x-flv".to_owned())],
            timeout_ms: 1_000,
        });

        match result {
            ProbeResult::Candidate(candidate) => {
                assert_eq!(candidate.runtime_profile, "flv");
                assert!(candidate.diagnostic_reason.contains("MIME type matched"));
            }
            other => panic!("expected MIME-only header candidate, got {other:?}"),
        }
    }

    #[test]
    fn wildcard_matching_handles_middle_stars() {
        assert!(wildcard_match(
            "*.bilibili.com/*hevc*",
            "https://a.bilibili.com/live/hevc/1"
        ));
        assert!(wildcard_match("*.m3u8", "https://example.test/live.m3u8"));
        assert!(!wildcard_match("*.mpd", "https://example.test/live.m3u8"));
    }
}
