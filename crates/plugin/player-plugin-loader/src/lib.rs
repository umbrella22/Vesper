#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::Library;
use player_plugin::{
    BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, BenchmarkSinkReport,
    BenchmarkSinkStatus, CompletedDownloadInfo, DecoderCapabilities, DecoderCodecCapability,
    DecoderError, DecoderFrameFormat, DecoderMediaKind, DecoderNativeFrame,
    DecoderNativeRequirements, DecoderOperationStatus, DecoderPacket, DecoderPacketResult,
    DecoderPcmFrame, DecoderReceiveFrameStatus, DecoderReceiveNativeFrameMetadata,
    DecoderReceiveNativeFrameOutput, DecoderReceivePcmFrameMetadata, DecoderReceivePcmFrameOutput,
    DecoderSessionConfig, DecoderSessionInfo, FrameProcessorCapabilities, FrameProcessorError,
    FrameProcessorOperationStatus, FrameProcessorOutputFrame, FrameProcessorPluginFactory,
    FrameProcessorReceiveFrameMetadata, FrameProcessorReceiveOutput, FrameProcessorReceiveStatus,
    FrameProcessorSession, FrameProcessorSessionConfig, FrameProcessorSessionInfo,
    FrameProcessorSubmitFrame, FrameProcessorSubmitResult, NativeDecoderPluginFactory,
    NativeDecoderSession, NativeFrame, NativeFramePipelineProfile, NativeHandleKind, PipelineEvent,
    PipelineEventHook, PostDownloadProcessor, ProcessorCapabilities, ProcessorError,
    ProcessorOutput, ProcessorProgress, SourceNormalizerError, SourceNormalizerOperationStatus,
    SourceNormalizerPacketCapabilities, SourceNormalizerPacketLease,
    SourceNormalizerPacketPluginFactory, SourceNormalizerPacketSeek, SourceNormalizerPacketSession,
    SourceNormalizerPacketSessionConfig, SourceNormalizerPacketStreamInfo,
    SourceNormalizerReadPacketMetadata, SourceNormalizerReadPacketStatus,
    SourceNormalizerResourceCapabilities, SourceNormalizerResourcePluginFactory,
    SourceNormalizerResourceSession, SourceNormalizerResourceSessionConfig,
    SourceNormalizerResourceSessionInfo, SourceNormalizerResourceSessionStatus,
    SourceNormalizerResourceSessionWaitStatus, VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT,
    VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_CURRENT, VESPER_PLUGIN_ABI_VERSION_V2,
    VESPER_PLUGIN_ENTRY_SYMBOL, VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
    VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT, VesperBenchmarkSinkApi,
    VesperDecoderOpenSessionResult, VesperDecoderPluginApiV5,
    VesperDecoderReceiveNativeFrameResult, VesperDecoderReceivePcmFrameResult,
    VesperFrameProcessorOpenSessionResult, VesperFrameProcessorPluginApiV1,
    VesperFrameProcessorReceiveFrameResult, VesperPipelineEventHookApi, VesperPluginBytes,
    VesperPluginDescriptor, VesperPluginEntryPoint, VesperPluginKind, VesperPluginProcessResult,
    VesperPluginProgressCallbacks, VesperPluginResultStatus, VesperPostDownloadProcessorApi,
    VesperSourceNormalizerOpenPacketSessionResult, VesperSourceNormalizerOpenResourceSessionResult,
    VesperSourceNormalizerPluginApiV4, VesperSourceNormalizerReadPacketResult,
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

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_owned();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic payload".to_owned()
}

fn plugin_panic_message(
    plugin_name: &str,
    operation: &str,
    payload: &(dyn std::any::Any + Send),
) -> String {
    format!(
        "plugin `{plugin_name}` panicked during `{operation}`: {}",
        panic_payload_message(payload)
    )
}

pub(crate) fn catch_decoder_plugin_call<T>(
    plugin_name: &str,
    operation: &'static str,
    f: impl FnOnce() -> T,
) -> Result<T, DecoderError> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        DecoderError::abi_violation(plugin_panic_message(
            plugin_name,
            operation,
            payload.as_ref(),
        ))
    })
}

pub(crate) fn catch_source_normalizer_plugin_call<T>(
    plugin_name: &str,
    operation: &'static str,
    f: impl FnOnce() -> T,
) -> Result<T, SourceNormalizerError> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        SourceNormalizerError::abi_violation(plugin_panic_message(
            plugin_name,
            operation,
            payload.as_ref(),
        ))
    })
}

pub(crate) fn catch_frame_processor_plugin_call<T>(
    plugin_name: &str,
    operation: &'static str,
    f: impl FnOnce() -> T,
) -> Result<T, FrameProcessorError> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        FrameProcessorError::abi_violation(plugin_panic_message(
            plugin_name,
            operation,
            payload.as_ref(),
        ))
    })
}

#[cfg(test)]
mod tests;
