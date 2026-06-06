#![warn(clippy::undocumented_unsafe_blocks)]

mod abi;
mod benchmark;
mod capability;
mod decoder;
mod frame_processor;
mod hook;
mod native_frame;
mod processor;
pub mod source_normalizer;

pub use abi::{
    VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT, VESPER_DECODER_PLUGIN_ABI_VERSION_V2,
    VESPER_DECODER_PLUGIN_ABI_VERSION_V3, VESPER_DECODER_PLUGIN_ABI_VERSION_V4,
    VESPER_DECODER_PLUGIN_ABI_VERSION_V5, VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_CURRENT,
    VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1, VESPER_PLUGIN_ABI_VERSION_V2,
    VESPER_PLUGIN_ENTRY_SYMBOL, VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
    VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
    VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2, VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V3,
    VesperBenchmarkSinkApi, VesperDecoderOpenSessionResult, VesperDecoderPluginApiV2,
    VesperDecoderPluginApiV4, VesperDecoderPluginApiV5, VesperDecoderReceiveNativeFrameResult,
    VesperDecoderReceivePcmFrameResult, VesperFrameProcessorOpenSessionResult,
    VesperFrameProcessorPluginApiV1, VesperFrameProcessorReceiveFrameResult,
    VesperPipelineEventHookApi, VesperPluginBytes, VesperPluginDescriptor, VesperPluginEntryPoint,
    VesperPluginKind, VesperPluginProcessResult, VesperPluginProgressCallbacks,
    VesperPluginResultStatus, VesperPostDownloadProcessorApi,
    VesperSourceNormalizerOpenPacketSessionResult, VesperSourceNormalizerOpenResourceSessionResult,
    VesperSourceNormalizerPluginApiV2, VesperSourceNormalizerPluginApiV3,
    VesperSourceNormalizerReadPacketResult,
};
pub use benchmark::{
    BenchmarkEvent, BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, BenchmarkSinkReport,
    BenchmarkSinkStatus,
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
    NativeDecoderSession,
};
pub use frame_processor::{
    FrameProcessorCapabilities, FrameProcessorError, FrameProcessorFrameTimings,
    FrameProcessorOperationStatus, FrameProcessorOutputFrame, FrameProcessorPluginFactory,
    FrameProcessorReceiveFrameMetadata, FrameProcessorReceiveOutput, FrameProcessorReceiveStatus,
    FrameProcessorSession, FrameProcessorSessionConfig, FrameProcessorSessionInfo,
    FrameProcessorSessionRequirements, FrameProcessorSubmitFrame, FrameProcessorSubmitResult,
    FrameProcessorSubmitStatus,
};
pub use hook::{PipelineEvent, PipelineEventHook};
pub use native_frame::{
    NativeFrame, NativeFrameColorMetadata, NativeFrameContentLightMetadata,
    NativeFrameDolbyVisionMetadata, NativeFrameHdrMetadata, NativeFrameMasteringDisplayMetadata,
    NativeFrameMetadata, NativeFramePipelineProfile, NativeFrameReleaseTracking,
    NativeFrameSyncInfo, NativeFrameTransform, NativeHandleKind, VisibleRect,
};
pub use processor::{
    AssemblyMode, CompletedContentFormat, CompletedDownloadInfo, CompletedStream,
    ContentFormatKind, DownloadMetadata, OutputFormat, PostDownloadProcessor, ProcessorError,
    ProcessorOutput, ProcessorProgress, StreamKind,
};
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
    SourceNormalizerResourceSessionStatus, SourceNormalizerSessionCapabilities,
    SourceNormalizerSessionRequirements,
};
