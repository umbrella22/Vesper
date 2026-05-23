#![deny(unsafe_code)]

//! Source normalization profile, detection, validation, and command planning.
//!
//! This crate models the pre-decode source/container normalization layer. It
//! does not expose a plugin ABI and does not decode or transcode media frames.

mod command;
mod detector;
mod error;
mod ffmpeg_profile;
mod profile;
mod validation;

pub use command::{FfmpegCommandPlan, SourceNormalizerSessionConfig, build_ffmpeg_command_plan};
pub use detector::{
    ProbeContext, ProbeResult, RuntimeProfileCandidate, SourceDetector, SourceRuntimeDetector,
};
pub use error::{FfmpegCommandDiagnostic, SourceNormalizerError, SourceNormalizerResult};
pub use ffmpeg_profile::{FfmpegBuildProfile, FfmpegBuildProfileSet, FfmpegBuildValidation};
pub use profile::{
    NormalizeLevel, SourceMatchRules, SourceNormalizerOutputContainer, SourceNormalizerProfile,
    SourceNormalizerProfileSet, SourceNormalizerRequiredCapabilities,
    SourceNormalizerRuntimePolicy,
};
pub use validation::CapabilityValidationReport;
