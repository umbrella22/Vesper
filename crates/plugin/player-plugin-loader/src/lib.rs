#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::Library;
use player_plugin::{
    BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, BenchmarkSinkReport,
    BenchmarkSinkStatus, CompletedDownloadInfo, DecoderCapabilities, DecoderCodecCapability,
    DecoderError, DecoderMediaKind, DecoderNativeFrame, DecoderNativeRequirements,
    DecoderOperationStatus, DecoderPacket, DecoderPacketResult, DecoderReceiveFrameStatus,
    DecoderReceiveNativeFrameMetadata, DecoderReceiveNativeFrameOutput, DecoderSessionConfig,
    DecoderSessionInfo, FrameProcessorCapabilities, FrameProcessorError,
    FrameProcessorOperationStatus, FrameProcessorOutputFrame, FrameProcessorPluginFactory,
    FrameProcessorReceiveFrameMetadata, FrameProcessorReceiveOutput, FrameProcessorReceiveStatus,
    FrameProcessorSession, FrameProcessorSessionConfig, FrameProcessorSessionInfo,
    FrameProcessorSubmitFrame, FrameProcessorSubmitResult, NativeDecoderPluginFactory,
    NativeDecoderSession, NativeFrame, NativeHandleKind, PipelineEvent, PipelineEventHook,
    PostDownloadProcessor, ProcessorCapabilities, ProcessorError, ProcessorOutput,
    ProcessorProgress, SourceNormalizerError, SourceNormalizerOperationStatus,
    SourceNormalizerPacketCapabilities, SourceNormalizerPacketLease,
    SourceNormalizerPacketPluginFactory, SourceNormalizerPacketSeek, SourceNormalizerPacketSession,
    SourceNormalizerPacketSessionConfig, SourceNormalizerPacketStreamInfo,
    SourceNormalizerReadPacketMetadata, SourceNormalizerReadPacketStatus,
    SourceNormalizerResourceCapabilities, SourceNormalizerResourcePluginFactory,
    SourceNormalizerResourceSession, SourceNormalizerResourceSessionConfig,
    SourceNormalizerResourceSessionInfo, SourceNormalizerResourceSessionStatus,
    VESPER_DECODER_PLUGIN_ABI_VERSION_V3, VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
    VESPER_PLUGIN_ABI_VERSION_V2, VESPER_PLUGIN_ENTRY_SYMBOL,
    VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3, VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2,
    VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V3, VesperBenchmarkSinkApi,
    VesperDecoderOpenSessionResult, VesperDecoderPluginApiV2,
    VesperDecoderReceiveNativeFrameResult, VesperFrameProcessorOpenSessionResult,
    VesperFrameProcessorPluginApiV1, VesperFrameProcessorReceiveFrameResult,
    VesperPipelineEventHookApi, VesperPluginBytes, VesperPluginDescriptor, VesperPluginEntryPoint,
    VesperPluginKind, VesperPluginProcessResult, VesperPluginProgressCallbacks,
    VesperPluginResultStatus, VesperPostDownloadProcessorApi,
    VesperSourceNormalizerOpenPacketSessionResult, VesperSourceNormalizerOpenResourceSessionResult,
    VesperSourceNormalizerPluginApiV2, VesperSourceNormalizerPluginApiV3,
    VesperSourceNormalizerReadPacketResult,
};
use serde::de::DeserializeOwned;
use thiserror::Error;

mod benchmark;
mod decoder;
mod diagnostics;
mod dynamic_api;
mod frame_processor;
mod payload;
mod pipeline_event;
mod post_download;
mod registry;
mod source_normalizer;

pub use benchmark::BenchmarkSinkPluginSession;
pub use diagnostics::{
    DecoderPluginCapabilitySummary, DecoderPluginCodecSummary, DecoderPluginMatchRequest,
    FrameProcessorPluginCapabilitySummary, PluginCapabilitySummary, PluginDiagnosticRecord,
    PluginDiagnosticStatus, SourceNormalizerPacketPluginCapabilitySummary,
    SourceNormalizerResourcePluginCapabilitySummary,
};
pub use dynamic_api::{LoadedDynamicPlugin, PluginLoadError};
pub use registry::{PluginRegistry, PluginRegistryReport};

pub(crate) use benchmark::DynamicBenchmarkSink;
pub(crate) use decoder::DynamicNativeDecoderPluginFactory;
pub(crate) use dynamic_api::{
    CheckedBenchmarkSinkApi, CheckedFrameProcessorPluginApi, CheckedNativeDecoderPluginApi,
    CheckedPipelineEventHookApi, CheckedPostDownloadProcessorApi,
    CheckedSourceNormalizerPacketPluginApi, CheckedSourceNormalizerResourcePluginApi, FreeBytesFn,
    LibraryHolder, ProcessJsonFn, native_handle_kind_code,
};
pub(crate) use frame_processor::DynamicFrameProcessorPluginFactory;
pub(crate) use payload::*;
pub(crate) use pipeline_event::DynamicPipelineEventHook;
pub(crate) use post_download::DynamicPostDownloadProcessor;
pub(crate) use source_normalizer::{
    DynamicSourceNormalizerPacketPluginFactory, DynamicSourceNormalizerResourcePluginFactory,
};

#[cfg(test)]
mod tests;
