use std::path::PathBuf;

use thiserror::Error;

/// Result type used by source normalization operations.
pub type SourceNormalizerResult<T> = Result<T, SourceNormalizerError>;

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
    #[error("failed to spawn FFmpeg command `{program}`: {source}")]
    SpawnFfmpeg {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("FFmpeg exited with status {status}: {stderr}")]
    FfmpegFailed { status: String, stderr: String },
}
