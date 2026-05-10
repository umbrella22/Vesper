#![warn(clippy::undocumented_unsafe_blocks)]

mod abi;
mod benchmark;
mod capability;
mod decoder;
mod hook;
mod processor;

pub use abi::{
    VESPER_DECODER_PLUGIN_ABI_VERSION_V2, VESPER_PLUGIN_ABI_VERSION_V2, VESPER_PLUGIN_ENTRY_SYMBOL,
    VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3, VesperBenchmarkSinkApi,
    VesperDecoderOpenSessionResult, VesperDecoderPluginApiV2,
    VesperDecoderReceiveNativeFrameResult, VesperPipelineEventHookApi, VesperPluginBytes,
    VesperPluginDescriptor, VesperPluginEntryPoint, VesperPluginKind, VesperPluginProcessResult,
    VesperPluginProgressCallbacks, VesperPluginResultStatus, VesperPostDownloadProcessorApi,
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
    DecoderOperationStatus, DecoderPacket, DecoderPacketResult, DecoderReceiveFrameStatus,
    DecoderReceiveNativeFrameMetadata, DecoderReceiveNativeFrameOutput, DecoderSessionConfig,
    DecoderSessionInfo, DecoderVisibleRect, NativeDecoderPluginFactory, NativeDecoderSession,
};
pub use hook::{PipelineEvent, PipelineEventHook};
pub use processor::{
    AssemblyMode, CompletedContentFormat, CompletedDownloadInfo, CompletedStream,
    ContentFormatKind, DownloadMetadata, OutputFormat, PostDownloadProcessor, ProcessorError,
    ProcessorOutput, ProcessorProgress, StreamKind,
};
