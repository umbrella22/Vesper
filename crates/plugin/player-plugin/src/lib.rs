#![deny(unsafe_code)]

mod audio;
mod benchmark;
mod capability;
mod catalog;
mod decoder;
mod frame_processor;
mod hook;
mod invocation;
mod native_frame;
mod plan;
mod plugin_reference;
mod processor;
mod protocol;
mod resolver;
mod scope;
mod sdk;
pub mod source_normalizer;

pub use player_plugin_macros::export;

pub use audio::{
    AudioPitchMode, AudioPlaybackPolicy, AudioProcessorCapabilities, AudioProcessorChain,
    AudioProcessorError, AudioProcessorOperationStatus, AudioProcessorPluginFactory,
    AudioProcessorSession, AudioProcessorSessionConfig, AudioProcessorSessionInfo,
    AudioProcessorSubmitStatus,
};
pub use benchmark::{
    BenchmarkEvent, BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, BenchmarkSinkReport,
    BenchmarkSinkStatus, BenchmarkThresholdViolation, MAX_BENCHMARK_BATCH_EVENTS,
    MAX_BENCHMARK_THRESHOLD_VIOLATIONS,
};
pub use capability::ProcessorCapabilities;
pub use catalog::{
    CanonicalPluginArtifactDescriptor, MAX_PLUGIN_ARCHITECTURE_BYTES,
    MAX_PLUGIN_ARTIFACT_CAPABILITIES, MAX_PLUGIN_ARTIFACT_PATH_BYTES,
    MAX_PLUGIN_CATALOG_DIAGNOSTICS, MAX_PLUGIN_CATALOG_RECORDS, MAX_PLUGIN_CATALOG_SOURCE_BYTES,
    MAX_PLUGIN_PROVISIONS, MAX_PLUGIN_REQUIREMENTS, MAX_PLUGIN_RUNTIME_DEPENDENCIES,
    MAX_PLUGIN_TARGET_BYTES, PLUGIN_CATALOG_MIGRATION_VERSION, PLUGIN_CATALOG_SCHEMA_VERSION,
    PluginArtifactCapability, PluginArtifactDescriptor, PluginArtifactFormat,
    PluginArtifactTransport, PluginCatalog, PluginCatalogDiagnostic, PluginCatalogError,
    PluginCatalogRecord, PluginCatalogSource, PluginProvision, PluginRequirement,
    PluginResourcePolicy, PluginRuntimeDependency, PluginRuntimeLinkage,
    validate_plugin_provisions, validate_plugin_requirements,
};
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
pub use invocation::{
    PluginInvocationPolicy, PluginInvocationPolicyError, PluginInvocationWorkload,
};
pub use native_frame::{
    NativeFrame, NativeFrameColorMetadata, NativeFrameContentLightMetadata,
    NativeFrameDolbyVisionMetadata, NativeFrameHdrMetadata, NativeFrameLeaseToken,
    NativeFrameMasteringDisplayMetadata, NativeFrameMetadata, NativeFramePipelineProfile,
    NativeFrameReleaseTracking, NativeFrameSyncInfo, NativeFrameTransform, NativeHandleKind,
    VisibleRect,
};
pub use plan::{
    MAX_PLUGIN_PLAN_PROVIDERS, PLUGIN_PLAN_SCHEMA_VERSION, PluginPlan, PluginPlanError,
    PluginPlanPolicy, PluginPlanProvider,
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
pub use resolver::{
    MAX_PLUGIN_RESOLUTION_CONSTRAINTS, MAX_PLUGIN_RESOLUTION_STATES, PluginResolution,
    PluginResolutionError, PluginResolvedProvider, PluginResolver, PluginResolverPolicy,
};
pub use scope::{
    DEFAULT_PLUGIN_RUNTIME_SHUTDOWN_TIMEOUT, DEFAULT_PLUGIN_SCOPE_CLOSE_TIMEOUT,
    MAX_PLUGIN_CORRELATION_ID_BYTES, MAX_PLUGIN_RUNTIME_OWNER_REGISTRATIONS,
    MAX_PLUGIN_RUNTIME_SCOPE_REGISTRATIONS, MAX_PLUGIN_SCOPE_CHILDREN, MAX_PLUGIN_SCOPE_DEPTH,
    MAX_PLUGIN_SCOPE_OWNERS, MAX_PLUGIN_SCOPE_QUARANTINE_RECORDS, MAX_PLUGIN_SCOPE_REASON_BYTES,
    PluginActivePlaybackCorrelation, PluginNextPrewarmCorrelation, PluginOwnerDisposalError,
    PluginOwnerToken, PluginPlaybackAttachment, PluginPlaybackAttachmentToken,
    PluginPlaybackAuthority, PluginPlaybackError, PluginPlaybackRole,
    PluginPlaybackTransitionReport, PluginRuntime, PluginScope, PluginScopeCloseReport,
    PluginScopeError, PluginScopeKind, PluginScopeQuarantine, PluginScopeQuarantineReason,
    PluginScopeResource, PluginScopeState, PluginSessionCorrelation,
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
