#![warn(clippy::undocumented_unsafe_blocks)]

mod abi;
mod capability;
mod decoder;
mod hook;
mod processor;

pub use abi::{
    VESPER_PLUGIN_ABI_VERSION, VESPER_PLUGIN_ENTRY_SYMBOL, VesperDecoderOpenSessionResult,
    VesperDecoderPluginApi, VesperDecoderReceiveFrameResult, VesperPipelineEventHookApi,
    VesperPluginBytes, VesperPluginDescriptor, VesperPluginEntryPoint, VesperPluginKind,
    VesperPluginProcessResult, VesperPluginProgressCallbacks, VesperPluginResultStatus,
    VesperPostDownloadProcessorApi,
};
pub use capability::ProcessorCapabilities;
pub use decoder::{
    DecoderCapabilities, DecoderCodecCapability, DecoderError, DecoderFrame, DecoderFrameFormat,
    DecoderFrameMetadata, DecoderFramePlane, DecoderMediaKind, DecoderOperationStatus,
    DecoderPacket, DecoderPacketResult, DecoderPluginFactory, DecoderReceiveFrameMetadata,
    DecoderReceiveFrameOutput, DecoderReceiveFrameStatus, DecoderSession, DecoderSessionConfig,
    DecoderSessionInfo,
};
pub use hook::{PipelineEvent, PipelineEventHook};
pub use processor::{
    CompletedContentFormat, CompletedDownloadInfo, ContentFormatKind, DownloadMetadata,
    OutputFormat, PostDownloadProcessor, ProcessorError, ProcessorOutput, ProcessorProgress,
};
