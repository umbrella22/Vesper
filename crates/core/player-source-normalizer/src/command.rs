use std::{collections::HashMap, path::PathBuf};

use crate::{
    SourceNormalizerError, SourceNormalizerOutputContainer, SourceNormalizerProfile,
    SourceNormalizerResult,
};

/// Configuration used to plan one source normalization session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceNormalizerSessionConfig {
    pub runtime_profile: String,
    pub input: String,
    pub output: PathBuf,
    pub ffmpeg_program: String,
    pub output_to_stdout: bool,
}

/// Planned FFmpeg command line for a source normalizer run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegCommandPlan {
    pub program: String,
    pub args: Vec<String>,
}

impl FfmpegCommandPlan {
    /// Returns the command as a displayable argv vector.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = Vec::with_capacity(self.args.len() + 1);
        argv.push(self.program.clone());
        argv.extend(self.args.clone());
        argv
    }
}

/// Builds an FFmpeg copy/remux command for a runtime profile.
pub fn build_ffmpeg_command_plan(
    profile: &SourceNormalizerProfile,
    config: &SourceNormalizerSessionConfig,
) -> SourceNormalizerResult<FfmpegCommandPlan> {
    validate_profile_options(profile, config)?;

    let mut args = vec!["-hide_banner".to_owned(), "-y".to_owned()];

    if let Some(input_demuxer) = &profile.input_demuxer {
        args.push("-f".to_owned());
        args.push(input_demuxer.clone());
    }

    push_options(&mut args, &profile.input_options);
    if should_apply_network_options(&config.input) {
        push_options(&mut args, &profile.network);
    }

    args.push("-i".to_owned());
    args.push(config.input.clone());
    args.push("-map".to_owned());
    args.push("0".to_owned());
    args.push("-c".to_owned());
    args.push("copy".to_owned());

    if !profile.bitstream_filters.is_empty() {
        args.push("-bsf:a".to_owned());
        args.push(profile.bitstream_filters.join(","));
    }

    push_output_options(&mut args, profile, config.output_to_stdout);

    args.push("-f".to_owned());
    args.push(output_muxer(profile.output_container).to_owned());
    if config.output_to_stdout {
        args.push("pipe:1".to_owned());
    } else {
        args.push(config.output.display().to_string());
    }

    Ok(FfmpegCommandPlan {
        program: config.ffmpeg_program.clone(),
        args,
    })
}

fn push_options(args: &mut Vec<String>, options: &HashMap<String, toml::Value>) {
    let mut keys = options.keys().collect::<Vec<_>>();
    keys.sort_unstable();
    for key in keys {
        let value = &options[key];
        match value {
            toml::Value::Boolean(true) => {
                args.push(format!("-{key}"));
                args.push("1".to_owned());
            }
            toml::Value::Boolean(false) => {
                args.push(format!("-{key}"));
                args.push("0".to_owned());
            }
            toml::Value::Array(values) => {
                let joined = values
                    .iter()
                    .filter_map(toml_value_to_arg)
                    .collect::<Vec<_>>()
                    .join(",");
                if !joined.is_empty() {
                    args.push(format!("-{key}"));
                    args.push(joined);
                }
            }
            _ => {
                if let Some(value) = toml_value_to_arg(value) {
                    args.push(format!("-{key}"));
                    args.push(value);
                }
            }
        }
    }
}

fn push_output_options(
    args: &mut Vec<String>,
    profile: &SourceNormalizerProfile,
    output_to_stdout: bool,
) {
    let mut output_options = profile.output_options.clone();
    if (profile.output_container == SourceNormalizerOutputContainer::Fmp4
        || profile.output_container == SourceNormalizerOutputContainer::LocalStreamEndpoint)
        && !output_options.contains_key("movflags")
    {
        let movflags = if output_to_stdout {
            "+frag_keyframe+empty_moov+default_base_moof"
        } else {
            "+faststart+frag_keyframe+empty_moov"
        };
        output_options.insert(
            "movflags".to_owned(),
            toml::Value::String(movflags.to_owned()),
        );
    }
    push_options(args, &output_options);
}

fn toml_value_to_arg(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(value) => Some(value.clone()),
        toml::Value::Integer(value) => Some(value.to_string()),
        toml::Value::Float(value) => Some(value.to_string()),
        toml::Value::Boolean(value) => Some(if *value { "1" } else { "0" }.to_owned()),
        _ => None,
    }
}

fn validate_profile_options(
    profile: &SourceNormalizerProfile,
    config: &SourceNormalizerSessionConfig,
) -> SourceNormalizerResult<()> {
    validate_option_group(
        &config.runtime_profile,
        "input_options",
        &profile.input_options,
    )?;
    validate_option_group(&config.runtime_profile, "network", &profile.network)?;
    validate_option_group(
        &config.runtime_profile,
        "output_options",
        &profile.output_options,
    )?;
    Ok(())
}

fn validate_option_group(
    runtime_profile: &str,
    group: &str,
    options: &HashMap<String, toml::Value>,
) -> SourceNormalizerResult<()> {
    for (key, value) in options {
        validate_option_key(runtime_profile, group, key)?;
        validate_option_value(runtime_profile, group, key, value)?;
    }
    Ok(())
}

fn validate_option_key(
    runtime_profile: &str,
    group: &str,
    key: &str,
) -> SourceNormalizerResult<()> {
    if key.is_empty() {
        return invalid_option(
            runtime_profile,
            group,
            key,
            "FFmpeg option key must not be empty",
        );
    }
    if key.starts_with('-') {
        return invalid_option(
            runtime_profile,
            group,
            key,
            "FFmpeg option key must not include a leading '-'",
        );
    }
    if key.chars().any(char::is_whitespace) {
        return invalid_option(
            runtime_profile,
            group,
            key,
            "FFmpeg option key must not contain whitespace",
        );
    }
    Ok(())
}

fn validate_option_value(
    runtime_profile: &str,
    group: &str,
    key: &str,
    value: &toml::Value,
) -> SourceNormalizerResult<()> {
    match value {
        toml::Value::Table(_) => invalid_option(
            runtime_profile,
            group,
            key,
            "nested TOML tables are not supported for FFmpeg options",
        ),
        toml::Value::Array(values)
            if values
                .iter()
                .any(|value| matches!(value, toml::Value::Table(_))) =>
        {
            invalid_option(
                runtime_profile,
                group,
                key,
                "nested TOML tables are not supported in FFmpeg option arrays",
            )
        }
        _ => Ok(()),
    }
}

fn invalid_option<T>(
    runtime_profile: &str,
    group: &str,
    key: &str,
    reason: &str,
) -> SourceNormalizerResult<T> {
    Err(SourceNormalizerError::InvalidRuntimeProfile {
        profile: runtime_profile.to_owned(),
        message: format!(
            "{reason}: {group}.{key}; use the exact FFmpeg option name without implicit conversion"
        ),
    })
}

fn should_apply_network_options(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("tcp://")
        || lower.starts_with("tls://")
}

fn output_muxer(container: SourceNormalizerOutputContainer) -> &'static str {
    match container {
        SourceNormalizerOutputContainer::Fmp4 => "mp4",
        SourceNormalizerOutputContainer::Hls => "hls",
        SourceNormalizerOutputContainer::ResourceUrl => "mp4",
        SourceNormalizerOutputContainer::LocalStreamEndpoint => "mp4",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::SourceNormalizerProfileSet;

    use super::*;

    #[test]
    fn command_uses_copy_remux_without_transcode_flags() {
        let profiles = SourceNormalizerProfileSet::from_toml_str(
            r#"
[runtime.source-normalizer.flv]
input_demuxer = "flv"
output_container = "fmp4"
bitstream_filters = ["aac_adtstoasc"]

[runtime.source-normalizer.flv.input_options]
probesize = 5000000
"#,
        )
        .expect("profiles");
        let profile = profiles.require("flv").expect("flv");
        let plan = build_ffmpeg_command_plan(
            profile,
            &SourceNormalizerSessionConfig {
                runtime_profile: "flv".to_owned(),
                input: "input.flv".to_owned(),
                output: PathBuf::from("output.mp4"),
                ffmpeg_program: "ffmpeg".to_owned(),
                output_to_stdout: false,
            },
        )
        .expect("command plan");

        assert!(plan.args.windows(2).any(|pair| pair == ["-c", "copy"]));
        assert!(plan.args.windows(2).any(|pair| pair == ["-f", "flv"]));
        assert!(
            plan.args
                .windows(2)
                .any(|pair| pair == ["-bsf:a", "aac_adtstoasc"])
        );
        assert!(!plan.args.iter().any(|arg| arg == "libx264"));
        assert!(!plan.args.iter().any(|arg| arg == "-vf"));
    }

    #[test]
    fn command_preserves_ffmpeg_option_underscores() {
        let profiles = SourceNormalizerProfileSet::from_toml_str(
            r#"
[runtime.source-normalizer.base]
output_container = "fmp4"

[runtime.source-normalizer.base.input_options]
fpsprobesize = 0

[runtime.source-normalizer.base.network]
reconnect_at_eof = true
"#,
        )
        .expect("profiles");
        let profile = profiles.require("base").expect("base");
        let plan = build_ffmpeg_command_plan(
            profile,
            &SourceNormalizerSessionConfig {
                runtime_profile: "base".to_owned(),
                input: "input.mp4".to_owned(),
                output: PathBuf::from("output.mp4"),
                ffmpeg_program: "ffmpeg".to_owned(),
                output_to_stdout: false,
            },
        )
        .expect("command plan");

        assert!(plan.args.iter().any(|arg| arg == "-fpsprobesize"));
        assert!(!plan.args.iter().any(|arg| arg == "-reconnect_at_eof"));
        assert!(!plan.args.iter().any(|arg| arg == "-fps-probesize"));
    }

    #[test]
    fn command_applies_network_options_only_for_remote_inputs() {
        let profiles = SourceNormalizerProfileSet::from_toml_str(
            r#"
[runtime.source-normalizer.base]
output_container = "fmp4"

[runtime.source-normalizer.base.network]
reconnect_at_eof = true
"#,
        )
        .expect("profiles");
        let profile = profiles.require("base").expect("base");
        let local = build_ffmpeg_command_plan(
            profile,
            &SourceNormalizerSessionConfig {
                runtime_profile: "base".to_owned(),
                input: "input.mp4".to_owned(),
                output: PathBuf::from("output.mp4"),
                ffmpeg_program: "ffmpeg".to_owned(),
                output_to_stdout: false,
            },
        )
        .expect("local command plan");
        let remote = build_ffmpeg_command_plan(
            profile,
            &SourceNormalizerSessionConfig {
                runtime_profile: "base".to_owned(),
                input: "https://example.test/input.mp4".to_owned(),
                output: PathBuf::from("output.mp4"),
                ffmpeg_program: "ffmpeg".to_owned(),
                output_to_stdout: false,
            },
        )
        .expect("remote command plan");

        assert!(!local.args.iter().any(|arg| arg == "-reconnect_at_eof"));
        assert!(remote.args.iter().any(|arg| arg == "-reconnect_at_eof"));
    }

    #[test]
    fn command_can_write_fmp4_to_stdout_without_transcode_flags() {
        let profiles = SourceNormalizerProfileSet::from_toml_str(
            r#"
[runtime.source-normalizer.base]
output_container = "fmp4"
"#,
        )
        .expect("profiles");
        let profile = profiles.require("base").expect("base");
        let plan = build_ffmpeg_command_plan(
            profile,
            &SourceNormalizerSessionConfig {
                runtime_profile: "base".to_owned(),
                input: "input.mp4".to_owned(),
                output: PathBuf::from("ignored.mp4"),
                ffmpeg_program: "ffmpeg".to_owned(),
                output_to_stdout: true,
            },
        )
        .expect("command plan");

        assert!(plan.args.windows(2).any(|pair| pair == ["-c", "copy"]));
        assert_eq!(plan.args.last().map(String::as_str), Some("pipe:1"));
        assert!(
            plan.args.windows(2).any(|pair| {
                pair == ["-movflags", "+frag_keyframe+empty_moov+default_base_moof"]
            })
        );
        assert!(!plan.args.iter().any(|arg| arg == "libx264"));
        assert!(!plan.args.iter().any(|arg| arg == "-vf"));
    }

    #[test]
    fn command_rejects_invalid_ffmpeg_option_keys() {
        for (key, expected) in [
            ("", "must not be empty"),
            ("-probesize", "must not include a leading '-'"),
            ("probe size", "must not contain whitespace"),
        ] {
            let toml = format!(
                r#"
[runtime.source-normalizer.base]
output_container = "fmp4"

[runtime.source-normalizer.base.input_options]
"{key}" = 1
"#
            );
            let profiles = SourceNormalizerProfileSet::from_toml_str(&toml).expect("profiles");
            let profile = profiles.require("base").expect("base");
            let error = build_ffmpeg_command_plan(
                profile,
                &SourceNormalizerSessionConfig {
                    runtime_profile: "base".to_owned(),
                    input: "input.mp4".to_owned(),
                    output: PathBuf::from("output.mp4"),
                    ffmpeg_program: "ffmpeg".to_owned(),
                    output_to_stdout: false,
                },
            )
            .expect_err("invalid option key should fail");

            assert!(format!("{error}").contains(expected));
            assert!(format!("{error}").contains("exact FFmpeg option name"));
        }
    }

    #[test]
    fn command_rejects_nested_ffmpeg_option_tables() {
        let profiles = SourceNormalizerProfileSet::from_toml_str(
            r#"
[runtime.source-normalizer.base]
output_container = "fmp4"

[runtime.source-normalizer.base.input_options.headers]
User-Agent = "Vesper"
"#,
        )
        .expect("profiles");
        let profile = profiles.require("base").expect("base");
        let error = build_ffmpeg_command_plan(
            profile,
            &SourceNormalizerSessionConfig {
                runtime_profile: "base".to_owned(),
                input: "input.mp4".to_owned(),
                output: PathBuf::from("output.mp4"),
                ffmpeg_program: "ffmpeg".to_owned(),
                output_to_stdout: false,
            },
        )
        .expect_err("nested option tables should fail");

        assert!(format!("{error}").contains("nested TOML tables are not supported"));
        assert!(format!("{error}").contains("input_options.headers"));
    }
}
