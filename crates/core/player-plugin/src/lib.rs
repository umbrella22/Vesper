mod abi;
mod capability;
mod hook;
mod processor;

pub use abi::{
    VESPER_PLUGIN_ABI_VERSION, VESPER_PLUGIN_ENTRY_SYMBOL, VesperPipelineEventHookApi,
    VesperPluginBytes, VesperPluginDescriptor, VesperPluginEntryPoint, VesperPluginKind,
    VesperPluginProcessResult, VesperPluginProgressCallbacks, VesperPluginResultStatus,
    VesperPostDownloadProcessorApi,
};
pub use capability::ProcessorCapabilities;
pub use hook::{PipelineEvent, PipelineEventHook};
pub use processor::{
    CompletedContentFormat, CompletedDownloadInfo, ContentFormatKind, DownloadMetadata,
    OutputFormat, PostDownloadProcessor, ProcessorError, ProcessorOutput, ProcessorProgress,
};
