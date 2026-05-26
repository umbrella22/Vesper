#![warn(clippy::undocumented_unsafe_blocks)]

use std::collections::VecDeque;
mod native;
mod system;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use player_backend_ffmpeg::{
    CompressedVideoPacket, FfmpegBackend, VideoDecodeInfo as BackendVideoDecodeInfo,
    VideoDecoderMode as BackendVideoDecoderMode, VideoPacketSource, VideoPacketStreamInfo,
};
use player_model::{MediaSource, MediaSourceProtocol};
use player_platform_apple::{VIDEOTOOLBOX_BACKEND_NAME, probe_videotoolbox_hardware_decode};
use player_platform_desktop::{
    DesktopVideoFrame, DesktopVideoFramePoll, DesktopVideoFramePresentation, DesktopVideoSource,
    DesktopVideoSourceBootstrap, DesktopVideoSourceFactory, merge_runtime_fallback_reason,
    open_platform_desktop_source_with_options_and_interrupt,
    open_platform_desktop_source_with_video_source_factory_and_options_and_interrupt,
    probe_platform_desktop_source_with_options,
    probe_platform_desktop_source_with_video_source_factory_and_options, runtime_fallback_events,
};
use player_plugin::{
    DecoderBitstreamFormat, DecoderMediaKind, DecoderNativeFrame, DecoderNativeHandleKind,
    DecoderPacket, DecoderReceiveNativeFrameOutput, DecoderSessionConfig, FrameProcessorError,
    FrameProcessorOutputFrame, FrameProcessorReceiveOutput, FrameProcessorSession,
    FrameProcessorSessionConfig, FrameProcessorSubmitFrame, FrameProcessorSubmitResult,
    FrameProcessorSubmitStatus, NativeDecoderSession, NativeFrame, NativeFrameMetadata,
    NativeHandleKind, SourceNormalizerPacketMediaKind, SourceNormalizerPacketSeek,
    SourceNormalizerPacketSession, SourceNormalizerPacketSessionConfig,
    SourceNormalizerPacketTrackInfo, SourceNormalizerReadPacketMetadata,
    SourceNormalizerReadPacketStatus, VesperPluginKind,
};
use player_plugin_loader::{
    DecoderPluginCapabilitySummary, DecoderPluginCodecSummary, DecoderPluginMatchRequest,
    FrameProcessorPluginCapabilitySummary, LoadedDynamicPlugin, PluginCapabilitySummary,
    PluginDiagnosticRecord, PluginDiagnosticStatus, PluginRegistry,
    SourceNormalizerPacketPluginCapabilitySummary, SourceNormalizerResourcePluginCapabilitySummary,
};
use player_runtime::{
    DecodedVideoFrame, FrameProcessorMode, FrameProcessorPolicy, FrameProcessorPolicyAction,
    FrameProcessorWarning, FrameProcessorWarningKind, PlaybackProgress,
    PlayerDecoderPluginVideoMode, PlayerError, PlayerErrorCode, PlayerFrameProcessingMetrics,
    PlayerMediaInfo, PlayerPluginCapabilitySummary, PlayerPluginCodecCapability,
    PlayerPluginDecoderCapabilitySummary, PlayerPluginDiagnostic, PlayerPluginDiagnosticStatus,
    PlayerPluginFrameProcessorCapabilitySummary, PlayerPluginParticipation, PlayerResult,
    PlayerRuntime, PlayerRuntimeAdapter, PlayerRuntimeAdapterBackendFamily,
    PlayerRuntimeAdapterBootstrap, PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory,
    PlayerRuntimeAdapterInitializer, PlayerRuntimeBootstrap, PlayerRuntimeCommand,
    PlayerRuntimeCommandResult, PlayerRuntimeEvent, PlayerRuntimeInitializer, PlayerRuntimeOptions,
    PlayerRuntimeStartup, PlayerRuntimeWarning, PlayerVideoDecodeInfo, PlayerVideoDecodeMode,
    PlayerVideoSurfaceTarget, PresentationState, SourceNormalizerMode,
    register_default_runtime_adapter_factory,
};
use tracing::info;

pub const MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID: &str = "macos_software_desktop";
pub const MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID: &str = "macos_host";
const MACOS_NATIVE_FRAME_PREFETCH_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(50);
const MACOS_NATIVE_FRAME_DECODER_DRAIN_RETRY_INTERVAL: Duration = Duration::from_millis(1);
const FRAME_PROCESSOR_DEBUG_ENV: &str = "VESPER_FRAME_PROCESSOR_DEBUG";
const FRAME_PROCESSOR_DEBUG_TRACE_ENV: &str = "VESPER_FRAME_PROCESSOR_DEBUG_TRACE";
const FRAME_PROCESSOR_DEBUG_WINDOW_ENV: &str = "VESPER_FRAME_PROCESSOR_DEBUG_WINDOW";
const DEFAULT_FRAME_PROCESSOR_DEBUG_WINDOW: u64 = 120;
const SOURCE_NORMALIZER_STARTUP_TIMEOUT: Duration = Duration::from_millis(5_000);
const SOURCE_NORMALIZER_SESSION_TIMEOUT: Duration = Duration::from_millis(40_000);

pub use native::{
    MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID, MacosAvFoundationBridge,
    MacosAvFoundationBridgeBindings, MacosAvFoundationBridgeContext, MacosNativePlayerBridge,
    MacosNativePlayerProbe, MacosNativePlayerRuntimeAdapterFactory,
};
pub use system::{
    MacosMetalLayerPresenter, MacosSystemAvFoundationBridgeBindings, MacosVideoLayerFrame,
    MacosVideoLayerSurface, install_default_macos_system_native_runtime_adapter_factory,
    macos_system_native_runtime_adapter_factory, probe_source_with_avfoundation,
};

mod adapter;
mod diagnostics;
mod fallback;
mod frame_processor;
mod native_frame;
mod selection;
mod source_normalizer;

pub use adapter::*;
pub(crate) use diagnostics::*;
pub(crate) use fallback::*;
pub(crate) use frame_processor::*;
pub(crate) use native_frame::*;
pub(crate) use selection::*;
pub(crate) use source_normalizer::*;

#[cfg(test)]
mod tests;
