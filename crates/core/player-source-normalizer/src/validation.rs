use std::collections::HashSet;

use crate::{
    FfmpegBuildProfile, SourceNormalizerError, SourceNormalizerProfile, SourceNormalizerResult,
};

/// Result of validating a runtime profile against an FFmpeg build profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityValidationReport {
    pub runtime_profile: String,
    pub ffmpeg_profile: String,
    pub supported: bool,
    pub reasons: Vec<String>,
}

impl CapabilityValidationReport {
    /// Validates a runtime profile against a resolved FFmpeg build profile.
    pub fn validate(
        runtime_profile_name: impl Into<String>,
        runtime_profile: &SourceNormalizerProfile,
        ffmpeg_profile_name: impl Into<String>,
        ffmpeg_profile: &FfmpegBuildProfile,
    ) -> Self {
        let runtime_profile_name = runtime_profile_name.into();
        let ffmpeg_profile_name = ffmpeg_profile_name.into();
        let mut reasons = Vec::new();
        let required = &runtime_profile.required_capabilities;

        missing_values(
            "library",
            &required.libraries,
            &ffmpeg_profile.libraries,
            &mut reasons,
        );
        missing_values(
            "demuxer",
            &required.demuxers,
            &ffmpeg_profile.demuxers,
            &mut reasons,
        );
        missing_values(
            "muxer",
            &required.muxers,
            &ffmpeg_profile.muxers,
            &mut reasons,
        );
        missing_values(
            "protocol",
            &required.protocols,
            &ffmpeg_profile.protocols,
            &mut reasons,
        );
        missing_values(
            "parser",
            &required.parsers,
            &ffmpeg_profile.parsers,
            &mut reasons,
        );
        missing_values(
            "bitstream filter",
            &required.bsfs,
            &ffmpeg_profile.bsfs,
            &mut reasons,
        );

        if required.network && ffmpeg_profile.validation.forbid_network {
            reasons.push(
                "runtime profile requires network, but build profile forbids network".to_owned(),
            );
        }
        if required.network && !ffmpeg_profile.enables_network_protocol() {
            reasons.push(
                "runtime profile requires network, but build profile has no network protocols"
                    .to_owned(),
            );
        }
        if let Some(tls) = &required.tls {
            if tls != "none" && ffmpeg_profile.tls == "none" {
                reasons.push(format!(
                    "runtime profile requires TLS `{tls}`, but build profile TLS is none"
                ));
            }
            if ffmpeg_profile.validation.forbid_openssl
                && tls.contains("openssl")
                && ffmpeg_profile.tls.contains("openssl")
            {
                reasons.push("build profile forbids OpenSSL".to_owned());
            }
        }

        Self {
            runtime_profile: runtime_profile_name,
            ffmpeg_profile: ffmpeg_profile_name,
            supported: reasons.is_empty(),
            reasons,
        }
    }

    /// Converts an unsupported report into a typed error.
    pub fn ensure_supported(&self) -> SourceNormalizerResult<()> {
        if self.supported {
            Ok(())
        } else {
            Err(SourceNormalizerError::CapabilityMismatch {
                profile: self.runtime_profile.clone(),
                ffmpeg_profile: self.ffmpeg_profile.clone(),
                reasons: self.reasons.join("; "),
            })
        }
    }
}

fn missing_values(
    label: &str,
    required: &[String],
    available: &[String],
    reasons: &mut Vec<String>,
) {
    let available = available.iter().map(String::as_str).collect::<HashSet<_>>();
    for value in required {
        if !available.contains(value.as_str()) {
            reasons.push(format!("missing {label} `{value}`"));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        FfmpegBuildProfileSet, SourceNormalizerProfileSet, SourceNormalizerRequiredCapabilities,
    };

    use super::*;

    #[test]
    fn rejects_network_runtime_profile_against_local_only_build_profile() {
        let runtime = SourceNormalizerProfile {
            required_capabilities: SourceNormalizerRequiredCapabilities {
                libraries: vec!["avformat".to_owned()],
                protocols: vec!["http".to_owned()],
                tls: Some("native-or-openssl".to_owned()),
                network: true,
                ..SourceNormalizerRequiredCapabilities::default()
            },
            ..SourceNormalizerProfile::default()
        };
        let ffmpeg = FfmpegBuildProfile {
            libraries: vec!["avformat".to_owned()],
            protocols: vec!["file".to_owned(), "pipe".to_owned()],
            tls: "none".to_owned(),
            validation: crate::FfmpegBuildValidation {
                forbid_network: true,
                forbid_openssl: true,
            },
            ..FfmpegBuildProfile::default()
        };

        let report = CapabilityValidationReport::validate("flv", &runtime, "default", &ffmpeg);

        assert!(!report.supported);
        assert!(
            report
                .reasons
                .iter()
                .any(|reason| reason.contains("forbids network"))
        );
        assert!(
            report
                .reasons
                .iter()
                .any(|reason| reason.contains("missing protocol `http`"))
        );
    }

    #[test]
    fn validates_sample_profiles_against_matching_build_profile() {
        let runtime = SourceNormalizerProfileSet::from_toml_str(
            r#"
[runtime.source-normalizer.local]

[runtime.source-normalizer.local.required_capabilities]
libraries = ["avformat"]
protocols = ["file", "pipe"]
muxers = ["mp4"]
"#,
        )
        .expect("runtime");
        let ffmpeg = FfmpegBuildProfileSet::from_toml_str(
            r#"
[profile.local]
libraries = ["avformat"]
protocols = ["file", "pipe"]
muxers = ["mp4"]
"#,
            "desktop",
        )
        .expect("ffmpeg");

        let report = CapabilityValidationReport::validate(
            "local",
            runtime.require("local").expect("local"),
            "local",
            ffmpeg.require("local").expect("local"),
        );

        assert!(report.supported);
    }
}
