#![deny(unsafe_code)]

mod benchmark;
mod capability;
mod decoder;
mod frame_processor;
mod hook;
mod native_frame;
mod plugin_reference;
mod processor;
mod protocol;
mod sdk;
pub mod source_normalizer;

pub use player_plugin_macros::export;

pub use benchmark::{
    BenchmarkEvent, BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, BenchmarkSinkReport,
    BenchmarkSinkStatus, BenchmarkThresholdViolation, MAX_BENCHMARK_BATCH_EVENTS,
    MAX_BENCHMARK_THRESHOLD_VIOLATIONS,
};
pub use capability::ProcessorCapabilities;
pub use decoder::{
    DecoderBitstreamFormat, DecoderCapabilities, DecoderCodecCapability, DecoderError,
    DecoderFrameFormat, DecoderMediaKind, DecoderNativeDeviceContext,
    DecoderNativeDeviceContextKind, DecoderNativeFrame, DecoderNativeFrameMetadata,
    DecoderNativeFrameReleaseTracking, DecoderNativeHandleKind, DecoderNativeRequirements,
    DecoderOperationStatus, DecoderPacket, DecoderPacketResult, DecoderPcmFrame,
    DecoderPcmFrameMetadata, DecoderPcmSampleLayout, DecoderReceiveFrameStatus,
    DecoderReceiveNativeFrameMetadata, DecoderReceiveNativeFrameOutput,
    DecoderReceivePcmFrameMetadata, DecoderReceivePcmFrameOutput, DecoderSessionConfig,
    DecoderSessionInfo, DecoderSessionRequirements, DecoderVisibleRect, NativeDecoderPluginFactory,
    NativeDecoderSession, normalize_decoder_codec_identifier,
};
pub use frame_processor::{
    FrameProcessorCapabilities, FrameProcessorError, FrameProcessorFrameTimings,
    FrameProcessorInputFrame, FrameProcessorOperationStatus, FrameProcessorOutputFrame,
    FrameProcessorPluginFactory, FrameProcessorReceiveFrameMetadata, FrameProcessorReceiveOutput,
    FrameProcessorReceiveStatus, FrameProcessorSession, FrameProcessorSessionConfig,
    FrameProcessorSessionInfo, FrameProcessorSessionRequirements, FrameProcessorSubmitFrame,
    FrameProcessorSubmitResult, FrameProcessorSubmitStatus,
};
pub use hook::{
    PipelineEvent, PipelineEventHook, PipelineEventHookError, PipelineEventHookOutcome,
};
pub use native_frame::{
    NativeFrame, NativeFrameColorMetadata, NativeFrameContentLightMetadata,
    NativeFrameDolbyVisionMetadata, NativeFrameHdrMetadata, NativeFrameLeaseToken,
    NativeFrameMasteringDisplayMetadata, NativeFrameMetadata, NativeFramePipelineProfile,
    NativeFrameReleaseTracking, NativeFrameSyncInfo, NativeFrameTransform, NativeHandleKind,
    VisibleRect,
};
pub use plugin_reference::{PluginReference, PluginReferenceError, PluginTransport};
pub use processor::{
    AssemblyMode, CompletedContentFormat, CompletedDownloadInfo, CompletedStream,
    ContentFormatKind, DownloadMetadata, OutputFormat, PostDownloadProcessor, ProcessorError,
    ProcessorOutput, ProcessorProgress, StreamKind,
};
pub use protocol::{
    MAX_PIPELINE_EVENT_INPUT_BYTES, MAX_PLUGIN_ATTRIBUTE_KEY_BYTES,
    MAX_PLUGIN_ATTRIBUTE_VALUE_BYTES, MAX_PLUGIN_ATTRIBUTES, MAX_PLUGIN_DIAGNOSTIC_MESSAGE_BYTES,
    MAX_PLUGIN_DIAGNOSTICS, MAX_PLUGIN_ERROR_MESSAGE_BYTES, MAX_PLUGIN_EVENT_ID_BYTES,
    MAX_PLUGIN_EVENT_NAME_BYTES, MAX_PLUGIN_MEASUREMENTS, MAX_PLUGIN_PLATFORM_BYTES,
    MAX_PLUGIN_PROTOCOL_BYTES, MAX_PLUGIN_RESOURCE_IDENTITY_BYTES, MAX_PLUGIN_THREAD_BYTES,
    MAX_SOURCE_NORMALIZER_PACKET_BYTES, PluginDiagnostic, PluginDiagnosticSeverity,
    PluginMeasurement, PluginProtocolViolation,
};
pub use sdk::{Plugin, PluginBuildError, PluginBuilder, PluginCapability};
pub use source_normalizer::{
    SourceNormalizerError, SourceNormalizerNormalizeLevel, SourceNormalizerOperationStatus,
    SourceNormalizerOutputRoute, SourceNormalizerPacket, SourceNormalizerPacketCapabilities,
    SourceNormalizerPacketLease, SourceNormalizerPacketMediaKind,
    SourceNormalizerPacketPluginFactory, SourceNormalizerPacketSeek, SourceNormalizerPacketSession,
    SourceNormalizerPacketSessionConfig, SourceNormalizerPacketSessionRequirements,
    SourceNormalizerPacketStreamInfo, SourceNormalizerPacketTrackInfo,
    SourceNormalizerReadPacketMetadata, SourceNormalizerReadPacketStatus,
    SourceNormalizerRequiredCapabilities, SourceNormalizerResourceCachePolicy,
    SourceNormalizerResourceCapabilities, SourceNormalizerResourceInfo,
    SourceNormalizerResourcePluginFactory, SourceNormalizerResourceSession,
    SourceNormalizerResourceSessionConfig, SourceNormalizerResourceSessionInfo,
    SourceNormalizerResourceSessionRequirements, SourceNormalizerResourceSessionState,
    SourceNormalizerResourceSessionStatus, SourceNormalizerResourceSessionWaitStatus,
    SourceNormalizerSessionCapabilities, SourceNormalizerSessionRequirements,
    validate_source_normalizer_plugin_input,
};

#[doc(hidden)]
pub mod __private {
    pub use player_plugin_abi::VesperPluginRoot;

    pub fn export_plugin<R>(factory: fn() -> R) -> *const VesperPluginRoot
    where
        R: crate::sdk::PluginFactoryResult,
    {
        crate::sdk::export_plugin(factory)
    }
}
