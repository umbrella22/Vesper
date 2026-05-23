use std::{fmt, path::PathBuf};

use thiserror::Error;

/// Result type used by source normalization operations.
pub type SourceNormalizerResult<T> = Result<T, SourceNormalizerError>;

/// Diagnostic context for an FFmpeg command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegCommandDiagnostic {
    pub program: String,
    pub args: Vec<String>,
    pub pid: Option<u32>,
}

impl FfmpegCommandDiagnostic {
    /// Creates command diagnostic context from a program and argv tail.
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            pid: None,
        }
    }

    /// Returns the display-safe command line used for diagnostics.
    pub fn display_command(&self) -> String {
        let mut argv = Vec::with_capacity(self.args.len() + 1);
        argv.push(self.program.as_str());
        argv.extend(self.args.iter().map(String::as_str));
        argv.join(" ")
    }
}

impl fmt::Display for FfmpegCommandDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.display_command())
    }
}

/// Errors reported by source normalization profile and command planning.
#[derive(Debug, Error)]
pub enum SourceNormalizerError {
    #[error("failed to read {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse TOML from {path}: {source}")]
    ParseToml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("unknown source normalizer runtime profile: {profile}")]
    UnknownRuntimeProfile { profile: String },
    #[error("unknown FFmpeg build profile: {profile}")]
    UnknownFfmpegProfile { profile: String },
    #[error("runtime profile inheritance cycle: {chain}")]
    RuntimeProfileCycle { chain: String },
    #[error("FFmpeg profile inheritance cycle: {chain}")]
    FfmpegProfileCycle { chain: String },
    #[error("invalid source normalizer profile `{profile}`: {message}")]
    InvalidRuntimeProfile { profile: String, message: String },
    #[error(
        "source normalizer profile `{profile}` is not supported by FFmpeg profile `{ffmpeg_profile}`: {reasons}"
    )]
    CapabilityMismatch {
        profile: String,
        ffmpeg_profile: String,
        reasons: String,
    },
    #[error("failed to spawn FFmpeg command `{command}`: {source}")]
    SpawnFfmpeg {
        command: FfmpegCommandDiagnostic,
        #[source]
        source: std::io::Error,
    },
    #[error("FFmpeg command `{command}` exited with status {status}: {stderr}")]
    FfmpegFailed {
        command: FfmpegCommandDiagnostic,
        status: String,
        stderr: String,
    },
}
