use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ffi::c_void;
use std::mem::{offset_of, size_of};
use std::ptr::NonNull;
use std::sync::Arc;

use player_plugin_abi::{
    BENCHMARK_SINK_INTERFACE_ID, FRAME_PROCESSOR_INTERFACE_ID, NATIVE_DECODER_INTERFACE_ID,
    PIPELINE_EVENT_HOOK_INTERFACE_ID, POST_DOWNLOAD_PROCESSOR_INTERFACE_ID,
    SOURCE_NORMALIZER_PACKET_INTERFACE_ID, SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID,
    VESPER_BENCHMARK_SINK_REQUIRED_SIZE, VESPER_FRAME_PROCESSOR_REQUIRED_SIZE,
    VESPER_INTERFACE_MAJOR, VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES,
    VESPER_MAX_INTERFACES_PER_PLUGIN, VESPER_MAX_PLUGIN_ID_BYTES, VESPER_MAX_PLUGIN_NAME_BYTES,
    VESPER_NATIVE_DECODER_REQUIRED_SIZE, VESPER_PIPELINE_EVENT_HOOK_REQUIRED_SIZE,
    VESPER_PLUGIN_ABI_MAJOR, VESPER_PLUGIN_ABI_MINOR, VESPER_POST_DOWNLOAD_PROCESSOR_REQUIRED_SIZE,
    VESPER_SOURCE_NORMALIZER_PACKET_REQUIRED_SIZE, VESPER_SOURCE_NORMALIZER_RESOURCE_REQUIRED_SIZE,
    VesperBenchmarkSink, VesperByteSlice, VesperFrameProcessor, VesperInterfaceDescriptor,
    VesperInterfaceHeader, VesperInterfaceId, VesperNativeDecoder, VesperOwnedBytes,
    VesperPipelineEventHook, VesperPluginRoot, VesperPostDownloadProcessor,
    VesperSourceNormalizerPacket, VesperSourceNormalizerResource, VesperStatus, abi_contains,
    status,
};
use thiserror::Error;

use crate::LibraryHolder;
use player_plugin::{
    BenchmarkSink, FrameProcessorPluginFactory, NativeDecoderPluginFactory, PipelineEventHook,
    PluginReference, PluginTransport, PostDownloadProcessor, SourceNormalizerPacketPluginFactory,
    SourceNormalizerResourcePluginFactory,
};

mod frame_processor;
mod runtime;
mod session;
mod source_normalizer;
mod stable;

use runtime::NativeAbiBoundaryError;

pub(crate) use frame_processor::NativeAbiFrameProcessorPluginFactory;
pub(crate) use session::NativeAbiDecoderPluginFactory;
pub(crate) use source_normalizer::{
    NativeAbiSourceNormalizerPacketPluginFactory, NativeAbiSourceNormalizerResourcePluginFactory,
};
pub(crate) use stable::{
    NativeAbiBenchmarkSink, NativeAbiPipelineEventHook, NativeAbiPostDownloadProcessor,
};

const ROOT_REQUIRED_SIZE: u32 = size_of::<VesperPluginRoot>() as u32;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NativePluginContractError {
    #[error("plugin root pointer is null")]
    NullRoot,
    #[error("plugin root is truncated: required {required} bytes, got {actual}")]
    TruncatedRoot { required: u32, actual: u32 },
    #[error(
        "plugin root ABI mismatch: host supports {expected_major}.{expected_minor}, plugin reports {actual_major}.{actual_minor}"
    )]
    RootVersionMismatch {
        expected_major: u16,
        expected_minor: u16,
        actual_major: u16,
        actual_minor: u16,
    },
    #[error("plugin root field `{field}` is missing")]
    MissingRootField { field: &'static str },
    #[error("plugin field `{field}` is empty")]
    EmptyField { field: &'static str },
    #[error("plugin field `{field}` is too large: limit {limit} bytes, got {actual}")]
    FieldTooLarge {
        field: &'static str,
        limit: usize,
        actual: u64,
    },
    #[error("plugin field `{field}` has a null pointer with non-zero length")]
    NullFieldData { field: &'static str },
    #[error("plugin field `{field}` is not valid UTF-8")]
    InvalidUtf8 { field: &'static str },
    #[error("plugin field `{field}` is not a valid reverse-DNS identity: {value}")]
    InvalidReverseDns { field: &'static str, value: String },
    #[error("plugin advertises too many interfaces: limit {limit}, got {actual}")]
    TooManyInterfaces { limit: u32, actual: u32 },
    #[error("plugin does not advertise any interfaces")]
    NoInterfaces,
    #[error("plugin callback `{callback}` returned failure status {status}")]
    CallbackFailure {
        callback: &'static str,
        status: VesperStatus,
    },
    #[error(
        "plugin interface descriptor {index} is truncated: required {required} bytes, got {actual}"
    )]
    TruncatedDescriptor {
        index: u32,
        required: u32,
        actual: u32,
    },
    #[error("plugin interface descriptor {index} has unsupported version {major}.{minor}")]
    UnsupportedInterfaceVersion { index: u32, major: u16, minor: u16 },
    #[error("plugin advertises duplicate interface {interface_id:?} instance `{instance_id}`")]
    DuplicateInterface {
        interface_id: VesperInterfaceId,
        instance_id: String,
    },
    #[error(
        "plugin query returned a null table for interface {interface_id:?} instance `{instance_id}`"
    )]
    NullInterface {
        interface_id: VesperInterfaceId,
        instance_id: String,
    },
    #[error(
        "plugin query returned a truncated interface header: required {required} bytes, got {actual}"
    )]
    TruncatedInterfaceHeader { required: u32, actual: u32 },
    #[error("plugin query returned metadata that differs from the enumerated interface")]
    InterfaceMetadataMismatch,
    #[error("plugin interface {interface_id:?} instance `{instance_id}` has a null context")]
    NullInterfaceContext {
        interface_id: VesperInterfaceId,
        instance_id: String,
    },
    #[error(
        "plugin interface {interface_id:?} instance `{instance_id}` is truncated: required {required} bytes, got {actual}"
    )]
    TruncatedInterface {
        interface_id: VesperInterfaceId,
        instance_id: String,
        required: u32,
        actual: u32,
    },
    #[error(
        "plugin interface {interface_id:?} instance `{instance_id}` is missing callback `{callback}`"
    )]
    MissingInterfaceCallback {
        interface_id: VesperInterfaceId,
        instance_id: String,
        callback: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginContractDiagnosticKind {
    Compatibility,
    ContractViolation,
}

impl NativePluginContractError {
    pub const fn diagnostic_kind(&self) -> PluginContractDiagnosticKind {
        match self {
            Self::RootVersionMismatch { .. } | Self::UnsupportedInterfaceVersion { .. } => {
                PluginContractDiagnosticKind::Compatibility
            }
            _ => PluginContractDiagnosticKind::ContractViolation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckedInterfaceDescriptor {
    pub(crate) interface_id: VesperInterfaceId,
    pub(crate) major: u16,
    pub(crate) minor: u16,
    pub(crate) instance_id: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CheckedInterfaceTable {
    PostDownload(VesperPostDownloadProcessor),
    PipelineEventHook(VesperPipelineEventHook),
    BenchmarkSink(VesperBenchmarkSink),
    NativeDecoder(VesperNativeDecoder),
    FrameProcessor(VesperFrameProcessor),
    SourceNormalizerPacket(VesperSourceNormalizerPacket),
    SourceNormalizerResource(VesperSourceNormalizerResource),
    Unknown,
}

#[derive(Debug, Clone)]
pub(crate) struct CheckedInterface {
    pub(crate) index: u32,
    pub(crate) descriptor: CheckedInterfaceDescriptor,
    pub(crate) table: CheckedInterfaceTable,
}

#[derive(Debug)]
pub(crate) struct InterfaceLoadDiagnostic {
    pub(crate) index: u32,
    pub(crate) descriptor: Option<CheckedInterfaceDescriptor>,
    pub(crate) error: NativePluginContractError,
}

struct PendingPluginOwner {
    owner: NonNull<c_void>,
    destroy_owner: unsafe extern "C" fn(owner: *mut c_void),
    armed: bool,
}

impl PendingPluginOwner {
    fn new(
        owner: NonNull<c_void>,
        destroy_owner: unsafe extern "C" fn(owner: *mut c_void),
    ) -> Self {
        Self {
            owner,
            destroy_owner,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PendingPluginOwner {
    fn drop(&mut self) {
        if self.armed {
            // SAFETY: the root transferred this unique owner to the host and
            // supplied the matching destroy callback. The guard is armed only
            // until ownership moves into `PluginOwner`.
            unsafe { (self.destroy_owner)(self.owner.as_ptr()) };
        }
    }
}

#[derive(Debug)]
pub(crate) struct PluginOwner {
    owner: NonNull<c_void>,
    free_bytes: unsafe extern "C" fn(owner: *mut c_void, bytes: VesperOwnedBytes),
    destroy_owner: unsafe extern "C" fn(owner: *mut c_void),
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
}

// SAFETY: the native root contract requires the owner and all interface factories
// to support concurrent shared calls. Sessions remain separately serialized.
unsafe impl Send for PluginOwner {}
// SAFETY: same contract as above; the pointer is never dereferenced by the
// loader and is only passed back to validated plugin callbacks.
unsafe impl Sync for PluginOwner {}

impl PluginOwner {
    pub(crate) fn free_bytes(&self, bytes: VesperOwnedBytes) {
        if bytes.data.is_null() && bytes.len == 0 {
            return;
        }
        // SAFETY: the allocation came from this root owner and the checked
        // wrapper transfers it back exactly once.
        unsafe { (self.free_bytes)(self.owner.as_ptr(), bytes) };
    }
}

impl Drop for PluginOwner {
    fn drop(&mut self) {
        // SAFETY: `owner` is unique to this root and `PluginOwner` is the only
        // object that invokes the validated destroy callback.
        unsafe { (self.destroy_owner)(self.owner.as_ptr()) };
    }
}

#[derive(Debug)]
pub(crate) struct CheckedPluginRoot {
    pub(crate) plugin_id: String,
    pub(crate) plugin_name: String,
    pub(crate) interfaces: Vec<CheckedInterface>,
    pub(crate) diagnostics: Vec<InterfaceLoadDiagnostic>,
    pub(crate) owner: Arc<PluginOwner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInterfaceMetadata {
    pub interface_id: [u8; 16],
    pub major: u16,
    pub minor: u16,
    pub instance_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginInterfaceState {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInterfaceRecord {
    pub metadata: PluginInterfaceMetadata,
    pub state: PluginInterfaceState,
}

impl From<&CheckedInterfaceDescriptor> for PluginInterfaceMetadata {
    fn from(descriptor: &CheckedInterfaceDescriptor) -> Self {
        Self {
            interface_id: descriptor.interface_id.0,
            major: descriptor.major,
            minor: descriptor.minor,
            instance_id: descriptor.instance_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInterfaceDiagnostic {
    pub index: Option<u32>,
    pub interface: Option<PluginInterfaceMetadata>,
    pub kind: PluginContractDiagnosticKind,
    pub message: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PluginSelectionError {
    #[error("plugin reference selects transport {actual:?}, but this root uses {expected:?}")]
    TransportMismatch {
        expected: PluginTransport,
        actual: PluginTransport,
    },
    #[error("plugin reference id `{requested}` does not match loaded plugin `{loaded}`")]
    PluginIdMismatch { requested: String, loaded: String },
    #[error("plugin `{plugin_id}` does not expose {interface} instance `{instance_id}`")]
    InstanceNotFound {
        plugin_id: String,
        interface: &'static str,
        instance_id: String,
    },
    #[error("plugin `{plugin_id}` does not expose interface {interface}")]
    InterfaceNotFound {
        plugin_id: String,
        interface: &'static str,
    },
    #[error(
        "plugin `{plugin_id}` exposes {interface} instance `{instance_id}`, but it is unavailable"
    )]
    InstanceUnavailable {
        plugin_id: String,
        interface: &'static str,
        instance_id: String,
    },
    #[error("plugin `{plugin_id}` exposes interface {interface}, but it is unavailable")]
    InterfaceUnavailable {
        plugin_id: String,
        interface: &'static str,
    },
    #[error(
        "plugin `{plugin_id}` exposes {count} instances of {interface}; capability_instance_id is required"
    )]
    Ambiguous {
        plugin_id: String,
        interface: &'static str,
        count: usize,
    },
    #[error("plugin `{plugin_id}` is not loaded for transport {transport:?}")]
    PluginNotFound {
        plugin_id: String,
        transport: PluginTransport,
    },
    #[error(
        "loaded plugin identity `{plugin_id}` or instance `{instance_id}` cannot form a canonical reference"
    )]
    InvalidLoadedIdentity {
        plugin_id: String,
        instance_id: String,
    },
}

pub struct LoadedNativePlugin {
    plugin_id: String,
    plugin_name: String,
    post_download: BTreeMap<String, Arc<dyn PostDownloadProcessor>>,
    pipeline_event_hooks: BTreeMap<String, Arc<dyn PipelineEventHook>>,
    benchmark_sinks: BTreeMap<String, Arc<dyn BenchmarkSink>>,
    native_decoders: BTreeMap<String, Arc<dyn NativeDecoderPluginFactory>>,
    frame_processors: BTreeMap<String, Arc<dyn FrameProcessorPluginFactory>>,
    source_packets: BTreeMap<String, Arc<dyn SourceNormalizerPacketPluginFactory>>,
    source_resources: BTreeMap<String, Arc<dyn SourceNormalizerResourcePluginFactory>>,
    advertised_instances: BTreeMap<[u8; 16], BTreeSet<String>>,
    interfaces: Vec<PluginInterfaceRecord>,
    diagnostics: Vec<PluginInterfaceDiagnostic>,
    _owner: Arc<PluginOwner>,
}

impl std::fmt::Debug for LoadedNativePlugin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedNativePlugin")
            .field("plugin_id", &self.plugin_id)
            .field("plugin_name", &self.plugin_name)
            .field("post_download_instances", &self.post_download.keys())
            .field(
                "pipeline_event_hook_instances",
                &self.pipeline_event_hooks.keys(),
            )
            .field("benchmark_sink_instances", &self.benchmark_sinks.keys())
            .field("native_decoder_instances", &self.native_decoders.keys())
            .field("frame_processor_instances", &self.frame_processors.keys())
            .field("source_packet_instances", &self.source_packets.keys())
            .field("source_resource_instances", &self.source_resources.keys())
            .field("advertised_instances", &self.advertised_instances)
            .field("interfaces", &self.interfaces)
            .field("diagnostics", &self.diagnostics)
            .finish()
    }
}

impl LoadedNativePlugin {
    pub(crate) fn from_checked(root: CheckedPluginRoot) -> Self {
        let CheckedPluginRoot {
            plugin_id,
            plugin_name,
            interfaces,
            diagnostics: root_diagnostics,
            owner,
        } = root;
        let mut advertised_instances = BTreeMap::<[u8; 16], BTreeSet<String>>::new();
        for descriptor in root_diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.descriptor.as_ref())
            .filter(|descriptor| is_known_interface(descriptor.interface_id))
        {
            advertised_instances
                .entry(descriptor.interface_id.0)
                .or_default()
                .insert(descriptor.instance_id.clone());
        }

        let mut loaded = Self {
            plugin_id,
            plugin_name,
            post_download: BTreeMap::new(),
            pipeline_event_hooks: BTreeMap::new(),
            benchmark_sinks: BTreeMap::new(),
            native_decoders: BTreeMap::new(),
            frame_processors: BTreeMap::new(),
            source_packets: BTreeMap::new(),
            source_resources: BTreeMap::new(),
            advertised_instances,
            interfaces: root_diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.descriptor.as_ref())
                .map(|descriptor| PluginInterfaceRecord {
                    metadata: PluginInterfaceMetadata::from(descriptor),
                    state: PluginInterfaceState::Unavailable,
                })
                .collect(),
            diagnostics: root_diagnostics
                .into_iter()
                .map(|diagnostic| PluginInterfaceDiagnostic {
                    index: Some(diagnostic.index),
                    interface: diagnostic
                        .descriptor
                        .as_ref()
                        .map(PluginInterfaceMetadata::from),
                    kind: diagnostic.error.diagnostic_kind(),
                    message: diagnostic.error.to_string(),
                })
                .collect(),
            _owner: owner.clone(),
        };

        for interface in interfaces {
            let index = interface.index;
            let descriptor = interface.descriptor;
            let instance_id = descriptor.instance_id.clone();
            let is_unknown = matches!(interface.table, CheckedInterfaceTable::Unknown);
            if !is_unknown {
                loaded
                    .advertised_instances
                    .entry(descriptor.interface_id.0)
                    .or_default()
                    .insert(instance_id.clone());
            }
            let result: Result<(), NativeAbiBoundaryError> = match interface.table {
                CheckedInterfaceTable::PostDownload(table) => NativeAbiPostDownloadProcessor::new(
                    &loaded.plugin_id,
                    loaded.plugin_name.clone(),
                    &instance_id,
                    owner.clone(),
                    table,
                )
                .map(|value| {
                    loaded.post_download.insert(instance_id, Arc::new(value));
                }),
                CheckedInterfaceTable::PipelineEventHook(table) => NativeAbiPipelineEventHook::new(
                    &loaded.plugin_id,
                    &instance_id,
                    owner.clone(),
                    table,
                )
                .map(|value| {
                    loaded
                        .pipeline_event_hooks
                        .insert(instance_id, Arc::new(value));
                }),
                CheckedInterfaceTable::BenchmarkSink(table) => NativeAbiBenchmarkSink::new(
                    &loaded.plugin_id,
                    loaded.plugin_name.clone(),
                    &instance_id,
                    owner.clone(),
                    table,
                )
                .map(|value| {
                    loaded.benchmark_sinks.insert(instance_id, Arc::new(value));
                }),
                CheckedInterfaceTable::NativeDecoder(table) => NativeAbiDecoderPluginFactory::new(
                    &loaded.plugin_id,
                    loaded.plugin_name.clone(),
                    &instance_id,
                    owner.clone(),
                    table,
                )
                .map(|value| {
                    loaded.native_decoders.insert(instance_id, Arc::new(value));
                }),
                CheckedInterfaceTable::FrameProcessor(table) => {
                    NativeAbiFrameProcessorPluginFactory::new(
                        &loaded.plugin_id,
                        loaded.plugin_name.clone(),
                        &instance_id,
                        owner.clone(),
                        table,
                    )
                    .map(|value| {
                        loaded.frame_processors.insert(instance_id, Arc::new(value));
                    })
                }
                CheckedInterfaceTable::SourceNormalizerPacket(table) => {
                    NativeAbiSourceNormalizerPacketPluginFactory::new(
                        &loaded.plugin_id,
                        loaded.plugin_name.clone(),
                        &instance_id,
                        owner.clone(),
                        table,
                    )
                    .map(|value| {
                        loaded.source_packets.insert(instance_id, Arc::new(value));
                    })
                }
                CheckedInterfaceTable::SourceNormalizerResource(table) => {
                    NativeAbiSourceNormalizerResourcePluginFactory::new(
                        &loaded.plugin_id,
                        loaded.plugin_name.clone(),
                        &instance_id,
                        owner.clone(),
                        table,
                    )
                    .map(|value| {
                        loaded.source_resources.insert(instance_id, Arc::new(value));
                    })
                }
                CheckedInterfaceTable::Unknown => Ok(()),
            };
            let metadata = PluginInterfaceMetadata::from(&descriptor);
            match result {
                Ok(()) => loaded.interfaces.push(PluginInterfaceRecord {
                    metadata,
                    state: if is_unknown {
                        PluginInterfaceState::Unknown
                    } else {
                        PluginInterfaceState::Available
                    },
                }),
                Err(error) => {
                    loaded.interfaces.push(PluginInterfaceRecord {
                        metadata: metadata.clone(),
                        state: PluginInterfaceState::Unavailable,
                    });
                    loaded.diagnostics.push(PluginInterfaceDiagnostic {
                        index: Some(index),
                        interface: Some(metadata),
                        kind: PluginContractDiagnosticKind::ContractViolation,
                        message: error.to_string(),
                    });
                }
            }
        }
        loaded
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }

    pub fn diagnostics(&self) -> &[PluginInterfaceDiagnostic] {
        &self.diagnostics
    }

    pub fn interfaces(&self) -> &[PluginInterfaceRecord] {
        &self.interfaces
    }

    pub fn unknown_interfaces(&self) -> impl Iterator<Item = &PluginInterfaceMetadata> {
        self.interfaces.iter().filter_map(|interface| {
            (interface.state == PluginInterfaceState::Unknown).then_some(&interface.metadata)
        })
    }

    pub fn resolve_post_download(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn PostDownloadProcessor>, PluginSelectionError> {
        self.resolve_post_download_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_post_download_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn PostDownloadProcessor>), PluginSelectionError> {
        self.resolve(
            reference,
            POST_DOWNLOAD_PROCESSOR_INTERFACE_ID,
            "PostDownloadProcessor",
            &self.post_download,
        )
    }

    pub fn resolve_pipeline_event_hook(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn PipelineEventHook>, PluginSelectionError> {
        self.resolve_pipeline_event_hook_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_pipeline_event_hook_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn PipelineEventHook>), PluginSelectionError> {
        self.resolve(
            reference,
            PIPELINE_EVENT_HOOK_INTERFACE_ID,
            "PipelineEventHook",
            &self.pipeline_event_hooks,
        )
    }

    pub fn resolve_benchmark_sink(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn BenchmarkSink>, PluginSelectionError> {
        self.resolve_benchmark_sink_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_benchmark_sink_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn BenchmarkSink>), PluginSelectionError> {
        self.resolve(
            reference,
            BENCHMARK_SINK_INTERFACE_ID,
            "BenchmarkSink",
            &self.benchmark_sinks,
        )
    }

    pub fn resolve_native_decoder(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn NativeDecoderPluginFactory>, PluginSelectionError> {
        self.resolve_native_decoder_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_native_decoder_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn NativeDecoderPluginFactory>), PluginSelectionError> {
        self.resolve(
            reference,
            NATIVE_DECODER_INTERFACE_ID,
            "NativeDecoder",
            &self.native_decoders,
        )
    }

    pub fn resolve_frame_processor(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn FrameProcessorPluginFactory>, PluginSelectionError> {
        self.resolve_frame_processor_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_frame_processor_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn FrameProcessorPluginFactory>), PluginSelectionError> {
        self.resolve(
            reference,
            FRAME_PROCESSOR_INTERFACE_ID,
            "FrameProcessor",
            &self.frame_processors,
        )
    }

    pub fn resolve_source_packet(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn SourceNormalizerPacketPluginFactory>, PluginSelectionError> {
        self.resolve_source_packet_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_source_packet_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn SourceNormalizerPacketPluginFactory>), PluginSelectionError> {
        self.resolve(
            reference,
            SOURCE_NORMALIZER_PACKET_INTERFACE_ID,
            "SourceNormalizerPacket",
            &self.source_packets,
        )
    }

    pub fn resolve_source_resource(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn SourceNormalizerResourcePluginFactory>, PluginSelectionError> {
        self.resolve_source_resource_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_source_resource_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn SourceNormalizerResourcePluginFactory>), PluginSelectionError>
    {
        self.resolve(
            reference,
            SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID,
            "SourceNormalizerResource",
            &self.source_resources,
        )
    }

    fn resolve<T: ?Sized>(
        &self,
        reference: &PluginReference,
        interface_id: VesperInterfaceId,
        interface: &'static str,
        instances: &BTreeMap<String, Arc<T>>,
    ) -> Result<(String, Arc<T>), PluginSelectionError> {
        if reference.transport() != PluginTransport::Native {
            return Err(PluginSelectionError::TransportMismatch {
                expected: PluginTransport::Native,
                actual: reference.transport(),
            });
        }
        if reference.plugin_id() != self.plugin_id {
            return Err(PluginSelectionError::PluginIdMismatch {
                requested: reference.plugin_id().to_owned(),
                loaded: self.plugin_id.clone(),
            });
        }
        if let Some(instance_id) = reference.capability_instance_id() {
            return instances
                .get(instance_id)
                .cloned()
                .map(|capability| (instance_id.to_owned(), capability))
                .ok_or_else(|| {
                    let advertised = self
                        .advertised_instances
                        .get(&interface_id.0)
                        .is_some_and(|values| values.contains(instance_id));
                    if advertised {
                        PluginSelectionError::InstanceUnavailable {
                            plugin_id: self.plugin_id.clone(),
                            interface,
                            instance_id: instance_id.to_owned(),
                        }
                    } else {
                        PluginSelectionError::InstanceNotFound {
                            plugin_id: self.plugin_id.clone(),
                            interface,
                            instance_id: instance_id.to_owned(),
                        }
                    }
                });
        }
        let advertised_count = self
            .advertised_instances
            .get(&interface_id.0)
            .map_or(0, BTreeSet::len);
        match advertised_count {
            0 => Err(PluginSelectionError::InterfaceNotFound {
                plugin_id: self.plugin_id.clone(),
                interface,
            }),
            1 => instances
                .iter()
                .next()
                .map(|(instance_id, capability)| (instance_id.clone(), capability.clone()))
                .ok_or_else(|| PluginSelectionError::InterfaceUnavailable {
                    plugin_id: self.plugin_id.clone(),
                    interface,
                }),
            count => Err(PluginSelectionError::Ambiguous {
                plugin_id: self.plugin_id.clone(),
                interface,
                count,
            }),
        }
    }
}

impl CheckedPluginRoot {
    pub(crate) unsafe fn from_raw(
        root_ptr: *const VesperPluginRoot,
        library: Option<Arc<LibraryHolder>>,
    ) -> Result<Self, NativePluginContractError> {
        if root_ptr.is_null() {
            return Err(NativePluginContractError::NullRoot);
        }

        // SAFETY: the entry contract guarantees a readable root prefix. Read
        // only `struct_size` before deciding whether the complete root prefix is
        // available.
        let struct_size = unsafe { root_ptr.cast::<u32>().read_unaligned() };
        if struct_size < ROOT_REQUIRED_SIZE {
            return Err(NativePluginContractError::TruncatedRoot {
                required: ROOT_REQUIRED_SIZE,
                actual: struct_size,
            });
        }
        // SAFETY: the size check above proves that the complete root prefix
        // is readable under the entry contract.
        let root = unsafe { root_ptr.read_unaligned() };
        if root.abi_major != VESPER_PLUGIN_ABI_MAJOR || root.abi_minor > VESPER_PLUGIN_ABI_MINOR {
            return Err(NativePluginContractError::RootVersionMismatch {
                expected_major: VESPER_PLUGIN_ABI_MAJOR,
                expected_minor: VESPER_PLUGIN_ABI_MINOR,
                actual_major: root.abi_major,
                actual_minor: root.abi_minor,
            });
        }
        let owner_ptr = NonNull::new(root.owner)
            .ok_or(NativePluginContractError::MissingRootField { field: "owner" })?;
        let destroy_owner =
            root.destroy_owner
                .ok_or(NativePluginContractError::MissingRootField {
                    field: "destroy_owner",
                })?;
        let mut pending_owner = PendingPluginOwner::new(owner_ptr, destroy_owner);
        let free_bytes = root
            .free_bytes
            .ok_or(NativePluginContractError::MissingRootField {
                field: "free_bytes",
            })?;
        let owner = Arc::new(PluginOwner {
            owner: owner_ptr,
            free_bytes,
            destroy_owner,
            library,
        });
        pending_owner.disarm();

        let plugin_id =
            // SAFETY: identity bytes are borrowed from the root owner until it
            // is destroyed, and they are copied before returning.
            unsafe { copy_utf8(root.plugin_id, "plugin_id", VESPER_MAX_PLUGIN_ID_BYTES, false) }?;
        if !is_reverse_dns(&plugin_id) {
            return Err(NativePluginContractError::InvalidReverseDns {
                field: "plugin_id",
                value: plugin_id,
            });
        }
        let plugin_name =
            // SAFETY: same root-owned identity contract as `plugin_id`.
            unsafe {
                copy_utf8(
                    root.plugin_name,
                    "plugin_name",
                    VESPER_MAX_PLUGIN_NAME_BYTES,
                    false,
                )
            }?;
        if root.interface_count > VESPER_MAX_INTERFACES_PER_PLUGIN {
            return Err(NativePluginContractError::TooManyInterfaces {
                limit: VESPER_MAX_INTERFACES_PER_PLUGIN,
                actual: root.interface_count,
            });
        }
        if root.interface_count == 0 {
            return Err(NativePluginContractError::NoInterfaces);
        }
        let interface_at =
            root.interface_at
                .ok_or(NativePluginContractError::MissingRootField {
                    field: "interface_at",
                })?;
        let query_interface =
            root.query_interface
                .ok_or(NativePluginContractError::MissingRootField {
                    field: "query_interface",
                })?;

        let mut interfaces = Vec::with_capacity(root.interface_count as usize);
        let mut diagnostics = Vec::new();
        let mut seen = HashSet::with_capacity(root.interface_count as usize);
        for index in 0..root.interface_count {
            // SAFETY: the validated root callbacks borrow only host-owned
            // inputs for each synchronous call. Returned descriptors and
            // tables remain backed by `owner` while copied and checked.
            match unsafe {
                load_interface(
                    owner.as_ref(),
                    interface_at,
                    query_interface,
                    index,
                    &mut seen,
                )
            } {
                Ok(interface) => interfaces.push(interface),
                Err((
                    _descriptor,
                    error @ NativePluginContractError::DuplicateInterface { .. },
                )) => {
                    return Err(error);
                }
                Err((descriptor, error)) => diagnostics.push(InterfaceLoadDiagnostic {
                    index,
                    descriptor,
                    error,
                }),
            }
        }

        Ok(Self {
            plugin_id,
            plugin_name,
            interfaces,
            diagnostics,
            owner,
        })
    }
}

unsafe fn load_interface(
    owner: &PluginOwner,
    interface_at: unsafe extern "C" fn(
        owner: *mut c_void,
        index: u32,
        out: *mut VesperInterfaceDescriptor,
    ) -> VesperStatus,
    query_interface: unsafe extern "C" fn(
        owner: *mut c_void,
        interface_id: *const VesperInterfaceId,
        instance_id: VesperByteSlice,
        requested_major: u16,
        minimum_minor: u16,
        out: *mut *const VesperInterfaceHeader,
    ) -> VesperStatus,
    index: u32,
    seen: &mut HashSet<(VesperInterfaceId, String)>,
) -> Result<
    CheckedInterface,
    (
        Option<CheckedInterfaceDescriptor>,
        NativePluginContractError,
    ),
> {
    let mut raw_descriptor = VesperInterfaceDescriptor::default();
    // SAFETY: all pointers are host-owned for this synchronous call and the
    // callback was validated before this helper was called.
    let result = unsafe { interface_at(owner.owner.as_ptr(), index, &mut raw_descriptor) };
    require_ok("interface_at", result).map_err(|error| (None, error))?;
    if raw_descriptor.struct_size < size_of::<VesperInterfaceDescriptor>() as u32 {
        return Err((
            None,
            NativePluginContractError::TruncatedDescriptor {
                index,
                required: size_of::<VesperInterfaceDescriptor>() as u32,
                actual: raw_descriptor.struct_size,
            },
        ));
    }
    let instance_id =
        // SAFETY: the descriptor borrows owner-backed bytes which are copied
        // before the next plugin call.
        unsafe {
            copy_utf8(
                raw_descriptor.instance_id,
                "capability_instance_id",
                VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES,
                false,
            )
        }
        .map_err(|error| (None, error))?;
    if !is_reverse_dns(&instance_id) {
        return Err((
            None,
            NativePluginContractError::InvalidReverseDns {
                field: "capability_instance_id",
                value: instance_id,
            },
        ));
    }
    let descriptor = CheckedInterfaceDescriptor {
        interface_id: raw_descriptor.interface_id,
        major: raw_descriptor.major,
        minor: raw_descriptor.minor,
        instance_id,
    };
    let key = (descriptor.interface_id, descriptor.instance_id.clone());
    if !seen.insert(key) {
        return Err((
            Some(descriptor.clone()),
            NativePluginContractError::DuplicateInterface {
                interface_id: descriptor.interface_id,
                instance_id: descriptor.instance_id.clone(),
            },
        ));
    }
    if !is_known_interface(descriptor.interface_id) {
        return Ok(CheckedInterface {
            index,
            descriptor,
            table: CheckedInterfaceTable::Unknown,
        });
    }
    if descriptor.major != VESPER_INTERFACE_MAJOR {
        return Err((
            Some(descriptor.clone()),
            NativePluginContractError::UnsupportedInterfaceVersion {
                index,
                major: descriptor.major,
                minor: descriptor.minor,
            },
        ));
    }

    let instance_bytes = VesperByteSlice {
        data: descriptor.instance_id.as_ptr(),
        len: descriptor.instance_id.len() as u64,
    };
    let mut table_ptr = std::ptr::null();
    // SAFETY: query inputs are borrowed for this call, and the output pointer
    // is host-owned. A successful result promises an owner-backed table.
    let result = unsafe {
        query_interface(
            owner.owner.as_ptr(),
            &descriptor.interface_id,
            instance_bytes,
            descriptor.major,
            0,
            &mut table_ptr,
        )
    };
    require_ok("query_interface", result).map_err(|error| (Some(descriptor.clone()), error))?;
    if table_ptr.is_null() {
        return Err((
            Some(descriptor.clone()),
            NativePluginContractError::NullInterface {
                interface_id: descriptor.interface_id,
                instance_id: descriptor.instance_id.clone(),
            },
        ));
    }
    let table =
        // SAFETY: query success promises a root-owned table. The helper reads
        // only fields covered by its advertised size.
        unsafe { check_interface_table(table_ptr, &descriptor) }
            .map_err(|error| (Some(descriptor.clone()), error))?;
    Ok(CheckedInterface {
        index,
        descriptor,
        table,
    })
}

fn is_known_interface(interface_id: VesperInterfaceId) -> bool {
    matches!(
        interface_id,
        POST_DOWNLOAD_PROCESSOR_INTERFACE_ID
            | PIPELINE_EVENT_HOOK_INTERFACE_ID
            | BENCHMARK_SINK_INTERFACE_ID
            | NATIVE_DECODER_INTERFACE_ID
            | FRAME_PROCESSOR_INTERFACE_ID
            | SOURCE_NORMALIZER_PACKET_INTERFACE_ID
            | SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID
    )
}

fn require_ok(
    callback: &'static str,
    value: VesperStatus,
) -> Result<(), NativePluginContractError> {
    if value == status::OK {
        Ok(())
    } else {
        Err(NativePluginContractError::CallbackFailure {
            callback,
            status: value,
        })
    }
}

unsafe fn copy_utf8(
    bytes: VesperByteSlice,
    field: &'static str,
    limit: usize,
    allow_empty: bool,
) -> Result<String, NativePluginContractError> {
    if bytes.len == 0 {
        return if allow_empty {
            Ok(String::new())
        } else {
            Err(NativePluginContractError::EmptyField { field })
        };
    }
    if bytes.len > limit as u64 {
        return Err(NativePluginContractError::FieldTooLarge {
            field,
            limit,
            actual: bytes.len,
        });
    }
    if bytes.data.is_null() {
        return Err(NativePluginContractError::NullFieldData { field });
    }
    let len = bytes.len as usize;
    // SAFETY: the caller guarantees the borrowed range is readable for the
    // root or callback lifetime, and the bounded length was checked above.
    let slice = unsafe { std::slice::from_raw_parts(bytes.data, len) };
    let value =
        std::str::from_utf8(slice).map_err(|_| NativePluginContractError::InvalidUtf8 { field })?;
    Ok(value.to_owned())
}

fn is_reverse_dns(value: &str) -> bool {
    let mut segments = value.split('.');
    let Some(first) = segments.next() else {
        return false;
    };
    let Some(second) = segments.next() else {
        return false;
    };
    valid_identity_segment(first)
        && valid_identity_segment(second)
        && segments.all(valid_identity_segment)
}

fn valid_identity_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    matches!(bytes.first(), Some(b'a'..=b'z'))
        && matches!(bytes.last(), Some(b'a'..=b'z' | b'0'..=b'9'))
        && bytes
            .iter()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

unsafe fn check_interface_table(
    table_ptr: *const VesperInterfaceHeader,
    descriptor: &CheckedInterfaceDescriptor,
) -> Result<CheckedInterfaceTable, NativePluginContractError> {
    // SAFETY: query success promises at least a readable `struct_size` word.
    let struct_size = unsafe { table_ptr.cast::<u32>().read_unaligned() };
    if struct_size < size_of::<VesperInterfaceHeader>() as u32 {
        return Err(NativePluginContractError::TruncatedInterfaceHeader {
            required: size_of::<VesperInterfaceHeader>() as u32,
            actual: struct_size,
        });
    }
    // SAFETY: the size check proves the common header prefix is readable.
    let header = unsafe { table_ptr.read_unaligned() };
    if header.interface_id != descriptor.interface_id
        || header.major != descriptor.major
        || header.minor != descriptor.minor
    {
        return Err(NativePluginContractError::InterfaceMetadataMismatch);
    }
    if header.context.is_null() {
        return Err(NativePluginContractError::NullInterfaceContext {
            interface_id: descriptor.interface_id,
            instance_id: descriptor.instance_id.clone(),
        });
    }

    macro_rules! full_table {
        ($type:ty, $required:expr, $variant:ident, [$($field:ident),+ $(,)?]) => {{
            ensure_table_size(descriptor, struct_size, $required)?;
            // SAFETY: the required size for these ABI tables covers the full
            // concrete table, including every field copied here.
            let table = unsafe { table_ptr.cast::<$type>().read_unaligned() };
            $(require_callback(descriptor, stringify!($field), table.$field.is_some())?;)+
            CheckedInterfaceTable::$variant(table)
        }};
    }

    let table = if descriptor.interface_id == POST_DOWNLOAD_PROCESSOR_INTERFACE_ID {
        full_table!(
            VesperPostDownloadProcessor,
            VESPER_POST_DOWNLOAD_PROCESSOR_REQUIRED_SIZE,
            PostDownload,
            [capabilities_json, process_json, assemble_json]
        )
    } else if descriptor.interface_id == PIPELINE_EVENT_HOOK_INTERFACE_ID {
        full_table!(
            VesperPipelineEventHook,
            VESPER_PIPELINE_EVENT_HOOK_REQUIRED_SIZE,
            PipelineEventHook,
            [on_event_json]
        )
    } else if descriptor.interface_id == BENCHMARK_SINK_INTERFACE_ID {
        ensure_table_size(descriptor, struct_size, VESPER_BENCHMARK_SINK_REQUIRED_SIZE)?;
        let on_event_batch_json =
            // SAFETY: the required prefix covers this field.
            unsafe {
                read_field::<Option<player_plugin_abi::VesperJsonCallFn>>(
                    table_ptr,
                    struct_size,
                    offset_of!(VesperBenchmarkSink, on_event_batch_json) as u32,
                )
            }
            .flatten();
        require_callback(
            descriptor,
            "on_event_batch_json",
            on_event_batch_json.is_some(),
        )?;
        let flush_json =
            // SAFETY: optional fields are copied only when their complete
            // storage is present in the advertised table size.
            unsafe {
                read_field::<Option<player_plugin_abi::VesperGetJsonFn>>(
                    table_ptr,
                    struct_size,
                    offset_of!(VesperBenchmarkSink, flush_json) as u32,
                )
            }
            .flatten();
        CheckedInterfaceTable::BenchmarkSink(VesperBenchmarkSink {
            header,
            on_event_batch_json,
            flush_json,
        })
    } else if descriptor.interface_id == NATIVE_DECODER_INTERFACE_ID {
        ensure_table_size(descriptor, struct_size, VESPER_NATIVE_DECODER_REQUIRED_SIZE)?;
        // SAFETY: each required field lies within the validated prefix.
        let mut table = unsafe { read_decoder_prefix(table_ptr, struct_size, header) };
        require_callback(
            descriptor,
            "capabilities_json",
            table.capabilities_json.is_some(),
        )?;
        require_callback(
            descriptor,
            "native_requirements_json",
            table.native_requirements_json.is_some(),
        )?;
        require_callback(
            descriptor,
            "open_session_json",
            table.open_session_json.is_some(),
        )?;
        require_callback(descriptor, "send_packet", table.send_packet.is_some())?;
        require_callback(
            descriptor,
            "receive_native_frame",
            table.receive_native_frame.is_some(),
        )?;
        require_callback(
            descriptor,
            "release_native_frame",
            table.release_native_frame.is_some(),
        )?;
        require_callback(descriptor, "flush_session", table.flush_session.is_some())?;
        require_callback(descriptor, "close_session", table.close_session.is_some())?;
        table.receive_pcm_frame =
            // SAFETY: optional tail read is size-gated.
            unsafe {
                read_field(
                    table_ptr,
                    struct_size,
                    offset_of!(VesperNativeDecoder, receive_pcm_frame) as u32,
                )
            }
            .flatten();
        CheckedInterfaceTable::NativeDecoder(table)
    } else if descriptor.interface_id == FRAME_PROCESSOR_INTERFACE_ID {
        full_table!(
            VesperFrameProcessor,
            VESPER_FRAME_PROCESSOR_REQUIRED_SIZE,
            FrameProcessor,
            [
                capabilities_json,
                open_session_json,
                submit_frame_json,
                receive_frame,
                release_frame,
                flush_session,
                close_session
            ]
        )
    } else if descriptor.interface_id == SOURCE_NORMALIZER_PACKET_INTERFACE_ID {
        ensure_table_size(
            descriptor,
            struct_size,
            VESPER_SOURCE_NORMALIZER_PACKET_REQUIRED_SIZE,
        )?;
        // SAFETY: each required field lies within the validated prefix.
        let mut table = unsafe { read_packet_prefix(table_ptr, struct_size, header) };
        require_callback(
            descriptor,
            "capabilities_json",
            table.capabilities_json.is_some(),
        )?;
        require_callback(
            descriptor,
            "open_session_json",
            table.open_session_json.is_some(),
        )?;
        require_callback(descriptor, "read_packet", table.read_packet.is_some())?;
        require_callback(descriptor, "release_packet", table.release_packet.is_some())?;
        require_callback(descriptor, "flush_session", table.flush_session.is_some())?;
        require_callback(descriptor, "close_session", table.close_session.is_some())?;
        table.seek_session_json =
            // SAFETY: optional tail read is size-gated.
            unsafe {
                read_field(
                    table_ptr,
                    struct_size,
                    offset_of!(VesperSourceNormalizerPacket, seek_session_json) as u32,
                )
            }
            .flatten();
        CheckedInterfaceTable::SourceNormalizerPacket(table)
    } else if descriptor.interface_id == SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID {
        full_table!(
            VesperSourceNormalizerResource,
            VESPER_SOURCE_NORMALIZER_RESOURCE_REQUIRED_SIZE,
            SourceNormalizerResource,
            [
                capabilities_json,
                open_session_json,
                poll_session,
                wait_session_update,
                cancel_session,
                close_session
            ]
        )
    } else {
        return Ok(CheckedInterfaceTable::Unknown);
    };
    Ok(table)
}

fn ensure_table_size(
    descriptor: &CheckedInterfaceDescriptor,
    actual: u32,
    required: u32,
) -> Result<(), NativePluginContractError> {
    if actual < required {
        Err(NativePluginContractError::TruncatedInterface {
            interface_id: descriptor.interface_id,
            instance_id: descriptor.instance_id.clone(),
            required,
            actual,
        })
    } else {
        Ok(())
    }
}

fn require_callback(
    descriptor: &CheckedInterfaceDescriptor,
    callback: &'static str,
    present: bool,
) -> Result<(), NativePluginContractError> {
    if present {
        Ok(())
    } else {
        Err(NativePluginContractError::MissingInterfaceCallback {
            interface_id: descriptor.interface_id,
            instance_id: descriptor.instance_id.clone(),
            callback,
        })
    }
}

unsafe fn read_field<T: Copy>(
    table_ptr: *const VesperInterfaceHeader,
    struct_size: u32,
    offset: u32,
) -> Option<T> {
    if !abi_contains(struct_size, offset, size_of::<T>() as u32) {
        return None;
    }
    // SAFETY: the size gate above proves the complete field lies in the table
    // allocation promised by the plugin. Unaligned reads avoid assumptions
    // about a maliciously offset base pointer.
    Some(unsafe {
        table_ptr
            .cast::<u8>()
            .add(offset as usize)
            .cast::<T>()
            .read_unaligned()
    })
}

unsafe fn read_decoder_prefix(
    table_ptr: *const VesperInterfaceHeader,
    struct_size: u32,
    header: VesperInterfaceHeader,
) -> VesperNativeDecoder {
    macro_rules! field {
        ($name:ident) => {
            // SAFETY: the caller validated the required decoder prefix.
            unsafe {
                read_field(
                    table_ptr,
                    struct_size,
                    offset_of!(VesperNativeDecoder, $name) as u32,
                )
            }
            .flatten()
        };
    }
    VesperNativeDecoder {
        header,
        capabilities_json: field!(capabilities_json),
        native_requirements_json: field!(native_requirements_json),
        open_session_json: field!(open_session_json),
        send_packet: field!(send_packet),
        receive_native_frame: field!(receive_native_frame),
        release_native_frame: field!(release_native_frame),
        flush_session: field!(flush_session),
        close_session: field!(close_session),
        receive_pcm_frame: None,
    }
}

unsafe fn read_packet_prefix(
    table_ptr: *const VesperInterfaceHeader,
    struct_size: u32,
    header: VesperInterfaceHeader,
) -> VesperSourceNormalizerPacket {
    macro_rules! field {
        ($name:ident) => {
            // SAFETY: the caller validated the required packet prefix.
            unsafe {
                read_field(
                    table_ptr,
                    struct_size,
                    offset_of!(VesperSourceNormalizerPacket, $name) as u32,
                )
            }
            .flatten()
        };
    }
    VesperSourceNormalizerPacket {
        header,
        capabilities_json: field!(capabilities_json),
        open_session_json: field!(open_session_json),
        read_packet: field!(read_packet),
        release_packet: field!(release_packet),
        flush_session: field!(flush_session),
        close_session: field!(close_session),
        seek_session_json: None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use player_plugin_abi::{VESPER_INTERFACE_MINOR, VesperJsonOut, VesperPluginEntryPoint};

    use super::*;

    const PLUGIN_ID: &[u8] = b"dev.vesper.fixture";
    const PLUGIN_NAME: &[u8] = b"Plugin fixture";
    const INSTANCE_ID: &[u8] = b"dev.vesper.fixture.event-hook";
    const SECOND_INSTANCE_ID: &[u8] = b"dev.vesper.fixture.event-hook-secondary";
    const BAD_INSTANCE_ID: &[u8] = b"dev.vesper.fixture.bad-frame";
    const UNKNOWN_INSTANCE_ID: &[u8] = b"dev.vesper.fixture.future";
    const UNKNOWN_INTERFACE_ID: VesperInterfaceId = VesperInterfaceId([0x55; 16]);

    struct FixtureOwner {
        destroyed: AtomicUsize,
        query_calls: AtomicUsize,
        table: *const VesperInterfaceHeader,
        table_size_override: u32,
    }

    unsafe extern "C" fn no_op_free(_owner: *mut c_void, _bytes: VesperOwnedBytes) {}

    unsafe extern "C" fn destroy(owner: *mut c_void) {
        // SAFETY: tests pass a live `FixtureOwner` as the root owner.
        let owner = unsafe { &*owner.cast::<FixtureOwner>() };
        owner.destroyed.fetch_add(1, Ordering::SeqCst);
    }

    unsafe extern "C" fn interface_at(
        _owner: *mut c_void,
        index: u32,
        out: *mut VesperInterfaceDescriptor,
    ) -> VesperStatus {
        if index != 0 || out.is_null() {
            return status::NOT_FOUND;
        }
        // SAFETY: the loader provides a live host-initialized output.
        let out = unsafe { &mut *out };
        *out = VesperInterfaceDescriptor {
            struct_size: size_of::<VesperInterfaceDescriptor>() as u32,
            interface_id: PIPELINE_EVENT_HOOK_INTERFACE_ID,
            major: VESPER_INTERFACE_MAJOR,
            minor: VESPER_INTERFACE_MINOR,
            instance_id: VesperByteSlice {
                data: INSTANCE_ID.as_ptr(),
                len: INSTANCE_ID.len() as u64,
            },
        };
        status::OK
    }

    unsafe extern "C" fn unknown_interface_at(
        _owner: *mut c_void,
        index: u32,
        out: *mut VesperInterfaceDescriptor,
    ) -> VesperStatus {
        if index != 0 || out.is_null() {
            return status::NOT_FOUND;
        }
        // SAFETY: the loader provides a live host-initialized output.
        unsafe {
            *out = VesperInterfaceDescriptor {
                struct_size: size_of::<VesperInterfaceDescriptor>() as u32,
                interface_id: UNKNOWN_INTERFACE_ID,
                major: 42,
                minor: 7,
                instance_id: VesperByteSlice {
                    data: UNKNOWN_INSTANCE_ID.as_ptr(),
                    len: UNKNOWN_INSTANCE_ID.len() as u64,
                },
            }
        };
        status::OK
    }

    unsafe extern "C" fn multiple_hooks_interface_at(
        _owner: *mut c_void,
        index: u32,
        out: *mut VesperInterfaceDescriptor,
    ) -> VesperStatus {
        let instance_id = match index {
            0 => INSTANCE_ID,
            1 => SECOND_INSTANCE_ID,
            _ => return status::NOT_FOUND,
        };
        if out.is_null() {
            return status::INVALID_ARGUMENT;
        }
        // SAFETY: the loader provides a live host-initialized output.
        unsafe {
            *out = VesperInterfaceDescriptor {
                struct_size: size_of::<VesperInterfaceDescriptor>() as u32,
                interface_id: PIPELINE_EVENT_HOOK_INTERFACE_ID,
                major: VESPER_INTERFACE_MAJOR,
                minor: VESPER_INTERFACE_MINOR,
                instance_id: VesperByteSlice {
                    data: instance_id.as_ptr(),
                    len: instance_id.len() as u64,
                },
            }
        };
        status::OK
    }

    unsafe extern "C" fn unknown_status_interface_at(
        _owner: *mut c_void,
        _index: u32,
        _out: *mut VesperInterfaceDescriptor,
    ) -> VesperStatus {
        0xffff_ff00
    }

    unsafe extern "C" fn mixed_interface_at(
        owner: *mut c_void,
        index: u32,
        out: *mut VesperInterfaceDescriptor,
    ) -> VesperStatus {
        if index == 1 {
            // SAFETY: this forwards the same validated callback arguments.
            return unsafe { interface_at(owner, 0, out) };
        }
        if index != 0 || out.is_null() {
            return status::NOT_FOUND;
        }
        // SAFETY: the loader provides a live host-initialized output.
        unsafe {
            *out = VesperInterfaceDescriptor {
                struct_size: size_of::<VesperInterfaceDescriptor>() as u32,
                interface_id: FRAME_PROCESSOR_INTERFACE_ID,
                major: VESPER_INTERFACE_MAJOR + 1,
                minor: 0,
                instance_id: VesperByteSlice {
                    data: BAD_INSTANCE_ID.as_ptr(),
                    len: BAD_INSTANCE_ID.len() as u64,
                },
            }
        };
        status::OK
    }

    unsafe extern "C" fn query_interface(
        owner: *mut c_void,
        interface_id: *const VesperInterfaceId,
        _instance_id: VesperByteSlice,
        requested_major: u16,
        _minimum_minor: u16,
        out: *mut *const VesperInterfaceHeader,
    ) -> VesperStatus {
        if owner.is_null() || interface_id.is_null() || out.is_null() {
            return status::INVALID_ARGUMENT;
        }
        // SAFETY: pointers come from the checked test root call.
        let owner = unsafe { &*owner.cast::<FixtureOwner>() };
        owner.query_calls.fetch_add(1, Ordering::SeqCst);
        // SAFETY: validated non-null above and borrowed for this call.
        let interface_id = unsafe { *interface_id };
        if interface_id != PIPELINE_EVENT_HOOK_INTERFACE_ID
            || requested_major != VESPER_INTERFACE_MAJOR
        {
            return status::NOT_FOUND;
        }
        if owner.table_size_override != 0 {
            // SAFETY: the fixture table is writable for the duration of the
            // test and restored by the caller after validation.
            unsafe {
                owner
                    .table
                    .cast_mut()
                    .cast::<u32>()
                    .write(owner.table_size_override)
            };
        }
        // SAFETY: validated non-null above.
        unsafe { *out = owner.table };
        status::OK
    }

    unsafe extern "C" fn on_event(
        _context: *mut c_void,
        _input: VesperByteSlice,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        if out.is_null() {
            status::INVALID_ARGUMENT
        } else {
            status::OK
        }
    }

    fn root_for(owner: &mut FixtureOwner, plugin_id: &[u8]) -> VesperPluginRoot {
        VesperPluginRoot {
            struct_size: size_of::<VesperPluginRoot>() as u32,
            abi_major: VESPER_PLUGIN_ABI_MAJOR,
            abi_minor: VESPER_PLUGIN_ABI_MINOR,
            owner: std::ptr::from_mut(owner).cast(),
            plugin_id: VesperByteSlice {
                data: plugin_id.as_ptr(),
                len: plugin_id.len() as u64,
            },
            plugin_name: VesperByteSlice {
                data: PLUGIN_NAME.as_ptr(),
                len: PLUGIN_NAME.len() as u64,
            },
            interface_count: 1,
            reserved: 0,
            interface_at: Some(interface_at),
            query_interface: Some(query_interface),
            free_bytes: Some(no_op_free),
            destroy_owner: Some(destroy),
        }
    }

    fn hook_table() -> VesperPipelineEventHook {
        VesperPipelineEventHook {
            header: VesperInterfaceHeader::new(
                size_of::<VesperPipelineEventHook>() as u32,
                PIPELINE_EVENT_HOOK_INTERFACE_ID,
                VESPER_INTERFACE_MAJOR,
                VESPER_INTERFACE_MINOR,
                NonNull::<u8>::dangling().as_ptr().cast(),
            ),
            on_event_json: Some(on_event),
        }
    }

    #[test]
    fn checked_root_enumerates_and_queries_typed_interface() {
        let mut hook = VesperPipelineEventHook {
            header: VesperInterfaceHeader::new(
                size_of::<VesperPipelineEventHook>() as u32,
                PIPELINE_EVENT_HOOK_INTERFACE_ID,
                VESPER_INTERFACE_MAJOR,
                VESPER_INTERFACE_MINOR,
                NonNull::<u8>::dangling().as_ptr().cast(),
            ),
            on_event_json: Some(on_event),
        };
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let root = root_for(&mut owner, PLUGIN_ID);
        let checked =
            // SAFETY: the complete fixture root and table outlive validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect("valid root");
        assert_eq!(checked.plugin_id, "dev.vesper.fixture");
        assert_eq!(checked.plugin_name, "Plugin fixture");
        assert_eq!(checked.interfaces.len(), 1);
        assert!(checked.diagnostics.is_empty());
        assert!(matches!(
            checked.interfaces[0].table,
            CheckedInterfaceTable::PipelineEventHook(_)
        ));
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 0);
        drop(checked);
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn validation_failure_after_owner_creation_destroys_once() {
        let mut hook = VesperPipelineEventHook {
            header: VesperInterfaceHeader::new(
                size_of::<VesperPipelineEventHook>() as u32,
                PIPELINE_EVENT_HOOK_INTERFACE_ID,
                VESPER_INTERFACE_MAJOR,
                VESPER_INTERFACE_MINOR,
                NonNull::<u8>::dangling().as_ptr().cast(),
            ),
            on_event_json: Some(on_event),
        };
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let root = root_for(&mut owner, b"not-reverse-dns");
        let error =
            // SAFETY: the complete fixture root outlives validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect_err("invalid identity");
        assert!(matches!(
            error,
            NativePluginContractError::InvalidReverseDns { .. }
        ));
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn missing_free_bytes_destroys_pending_owner_once() {
        let mut hook = VesperPipelineEventHook {
            header: VesperInterfaceHeader::new(
                size_of::<VesperPipelineEventHook>() as u32,
                PIPELINE_EVENT_HOOK_INTERFACE_ID,
                VESPER_INTERFACE_MAJOR,
                VESPER_INTERFACE_MINOR,
                NonNull::<u8>::dangling().as_ptr().cast(),
            ),
            on_event_json: Some(on_event),
        };
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let mut root = root_for(&mut owner, PLUGIN_ID);
        root.free_bytes = None;
        let error =
            // SAFETY: the complete fixture root outlives validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect_err("missing free_bytes");
        assert_eq!(
            error,
            NativePluginContractError::MissingRootField {
                field: "free_bytes"
            }
        );
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn maximum_length_plugin_name_is_accepted() {
        let mut hook = VesperPipelineEventHook {
            header: VesperInterfaceHeader::new(
                size_of::<VesperPipelineEventHook>() as u32,
                PIPELINE_EVENT_HOOK_INTERFACE_ID,
                VESPER_INTERFACE_MAJOR,
                VESPER_INTERFACE_MINOR,
                NonNull::<u8>::dangling().as_ptr().cast(),
            ),
            on_event_json: Some(on_event),
        };
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let plugin_name = vec![b'n'; VESPER_MAX_PLUGIN_NAME_BYTES];
        let mut root = root_for(&mut owner, PLUGIN_ID);
        root.plugin_name = VesperByteSlice {
            data: plugin_name.as_ptr(),
            len: plugin_name.len() as u64,
        };
        let checked =
            // SAFETY: the fixture root, name, and table outlive validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect("maximum name");
        assert_eq!(checked.plugin_name.len(), VESPER_MAX_PLUGIN_NAME_BYTES);
        drop(checked);
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unknown_future_interface_is_retained_without_querying_it() {
        let mut hook = VesperPipelineEventHook {
            header: VesperInterfaceHeader::new(
                size_of::<VesperPipelineEventHook>() as u32,
                PIPELINE_EVENT_HOOK_INTERFACE_ID,
                VESPER_INTERFACE_MAJOR,
                VESPER_INTERFACE_MINOR,
                NonNull::<u8>::dangling().as_ptr().cast(),
            ),
            on_event_json: Some(on_event),
        };
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let mut root = root_for(&mut owner, PLUGIN_ID);
        root.interface_at = Some(unknown_interface_at);
        let checked =
            // SAFETY: the complete fixture root outlives validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect("future interface");
        assert_eq!(checked.interfaces.len(), 1);
        assert_eq!(checked.interfaces[0].descriptor.major, 42);
        assert!(matches!(
            checked.interfaces[0].table,
            CheckedInterfaceTable::Unknown
        ));
        assert!(checked.diagnostics.is_empty());
        assert_eq!(owner.query_calls.load(Ordering::SeqCst), 0);
        let loaded = LoadedNativePlugin::from_checked(checked);
        let unknown = loaded
            .unknown_interfaces()
            .next()
            .expect("unknown interface metadata");
        assert_eq!(unknown.interface_id, UNKNOWN_INTERFACE_ID.0);
        assert_eq!(unknown.major, 42);
        assert_eq!(unknown.minor, 7);
        assert_eq!(unknown.instance_id, "dev.vesper.fixture.future");
        drop(loaded);
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn loaded_plugin_resolves_explicit_and_only_hook_instance() {
        let mut hook = hook_table();
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let root = root_for(&mut owner, PLUGIN_ID);
        let checked =
            // SAFETY: the complete fixture root and table outlive validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect("valid root");
        let loaded = LoadedNativePlugin::from_checked(checked);
        let implicit = PluginReference::new("dev.vesper.fixture", None, PluginTransport::Native)
            .expect("valid reference");
        let explicit = PluginReference::new(
            "dev.vesper.fixture",
            Some("dev.vesper.fixture.event-hook".to_owned()),
            PluginTransport::Native,
        )
        .expect("valid reference");

        let implicit_hook = loaded
            .resolve_pipeline_event_hook(&implicit)
            .expect("only instance");
        let explicit_hook = loaded
            .resolve_pipeline_event_hook(&explicit)
            .expect("explicit instance");
        assert!(Arc::ptr_eq(&implicit_hook, &explicit_hook));
        drop(loaded);
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 0);
        drop(implicit_hook);
        drop(explicit_hook);
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn loaded_plugin_requires_instance_for_multiple_hook_implementations() {
        let mut hook = hook_table();
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let mut root = root_for(&mut owner, PLUGIN_ID);
        root.interface_count = 2;
        root.interface_at = Some(multiple_hooks_interface_at);
        let checked =
            // SAFETY: the complete fixture root and table outlive validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect("valid root");
        let loaded = LoadedNativePlugin::from_checked(checked);
        let implicit = PluginReference::new("dev.vesper.fixture", None, PluginTransport::Native)
            .expect("valid reference");
        assert_eq!(
            loaded
                .resolve_pipeline_event_hook(&implicit)
                .err()
                .expect("ambiguous selection"),
            PluginSelectionError::Ambiguous {
                plugin_id: "dev.vesper.fixture".to_owned(),
                interface: "PipelineEventHook",
                count: 2,
            }
        );

        let explicit = PluginReference::new(
            "dev.vesper.fixture",
            Some("dev.vesper.fixture.event-hook-secondary".to_owned()),
            PluginTransport::Native,
        )
        .expect("valid reference");
        let selected = loaded
            .resolve_pipeline_event_hook(&explicit)
            .expect("explicit instance");
        drop(selected);
        drop(loaded);
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_wrapper_still_requires_explicit_instance_and_preserves_index() {
        let mut hook = hook_table();
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let root = root_for(&mut owner, PLUGIN_ID);
        let mut checked =
            // SAFETY: the complete fixture root and table outlive validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect("valid root");
        checked.interfaces.push(CheckedInterface {
            index: 1,
            descriptor: CheckedInterfaceDescriptor {
                interface_id: PIPELINE_EVENT_HOOK_INTERFACE_ID,
                major: VESPER_INTERFACE_MAJOR,
                minor: VESPER_INTERFACE_MINOR,
                instance_id: "dev.vesper.fixture.event-hook-secondary".to_owned(),
            },
            table: CheckedInterfaceTable::PipelineEventHook(VesperPipelineEventHook {
                header: VesperInterfaceHeader::new(
                    size_of::<VesperPipelineEventHook>() as u32,
                    PIPELINE_EVENT_HOOK_INTERFACE_ID,
                    VESPER_INTERFACE_MAJOR,
                    VESPER_INTERFACE_MINOR,
                    NonNull::<u8>::dangling().as_ptr().cast(),
                ),
                on_event_json: None,
            }),
        });

        let loaded = LoadedNativePlugin::from_checked(checked);
        assert_eq!(loaded.diagnostics().len(), 1);
        assert_eq!(loaded.interfaces().len(), 2);
        assert_eq!(
            loaded.interfaces()[0].state,
            PluginInterfaceState::Available
        );
        assert_eq!(
            loaded.interfaces()[1].state,
            PluginInterfaceState::Unavailable
        );
        let diagnostic = &loaded.diagnostics()[0];
        assert_eq!(diagnostic.index, Some(1));
        assert_eq!(
            diagnostic.kind,
            PluginContractDiagnosticKind::ContractViolation
        );
        let metadata = diagnostic.interface.as_ref().expect("interface metadata");
        assert_eq!(metadata.interface_id, PIPELINE_EVENT_HOOK_INTERFACE_ID.0);
        assert_eq!(metadata.major, VESPER_INTERFACE_MAJOR);
        assert_eq!(metadata.minor, VESPER_INTERFACE_MINOR);
        assert_eq!(
            metadata.instance_id,
            "dev.vesper.fixture.event-hook-secondary"
        );

        let implicit = PluginReference::new("dev.vesper.fixture", None, PluginTransport::Native)
            .expect("valid reference");
        assert_eq!(
            loaded
                .resolve_pipeline_event_hook(&implicit)
                .err()
                .expect("advertised ambiguity"),
            PluginSelectionError::Ambiguous {
                plugin_id: "dev.vesper.fixture".to_owned(),
                interface: "PipelineEventHook",
                count: 2,
            }
        );
        let unavailable = PluginReference::new(
            "dev.vesper.fixture",
            Some("dev.vesper.fixture.event-hook-secondary".to_owned()),
            PluginTransport::Native,
        )
        .expect("valid reference");
        assert_eq!(
            loaded
                .resolve_pipeline_event_hook(&unavailable)
                .err()
                .expect("unavailable instance"),
            PluginSelectionError::InstanceUnavailable {
                plugin_id: "dev.vesper.fixture".to_owned(),
                interface: "PipelineEventHook",
                instance_id: "dev.vesper.fixture.event-hook-secondary".to_owned(),
            }
        );
        drop(loaded);
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn loaded_plugin_rejects_transport_and_plugin_id_mismatch() {
        let mut hook = hook_table();
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let root = root_for(&mut owner, PLUGIN_ID);
        let checked =
            // SAFETY: the complete fixture root and table outlive validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect("valid root");
        let loaded = LoadedNativePlugin::from_checked(checked);

        let wasm = PluginReference::new("dev.vesper.fixture", None, PluginTransport::Wasm)
            .expect("valid reference");
        assert_eq!(
            loaded
                .resolve_pipeline_event_hook(&wasm)
                .err()
                .expect("transport mismatch"),
            PluginSelectionError::TransportMismatch {
                expected: PluginTransport::Native,
                actual: PluginTransport::Wasm,
            }
        );

        let other_plugin = PluginReference::new("dev.vesper.other", None, PluginTransport::Native)
            .expect("valid reference");
        assert_eq!(
            loaded
                .resolve_pipeline_event_hook(&other_plugin)
                .err()
                .expect("plugin id mismatch"),
            PluginSelectionError::PluginIdMismatch {
                requested: "dev.vesper.other".to_owned(),
                loaded: "dev.vesper.fixture".to_owned(),
            }
        );
        drop(loaded);
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unknown_root_callback_status_is_preserved_in_diagnostics() {
        let mut hook = hook_table();
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let mut root = root_for(&mut owner, PLUGIN_ID);
        root.interface_at = Some(unknown_status_interface_at);
        let checked =
            // SAFETY: the complete fixture root outlives validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect("isolated failure");
        let loaded = LoadedNativePlugin::from_checked(checked);
        assert_eq!(loaded.diagnostics().len(), 1);
        assert!(loaded.diagnostics()[0].message.contains("4294967040"));
        drop(loaded);
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn invalid_interface_does_not_block_valid_sibling() {
        let mut hook = VesperPipelineEventHook {
            header: VesperInterfaceHeader::new(
                size_of::<VesperPipelineEventHook>() as u32,
                PIPELINE_EVENT_HOOK_INTERFACE_ID,
                VESPER_INTERFACE_MAJOR,
                VESPER_INTERFACE_MINOR,
                NonNull::<u8>::dangling().as_ptr().cast(),
            ),
            on_event_json: Some(on_event),
        };
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let mut root = root_for(&mut owner, PLUGIN_ID);
        root.interface_count = 2;
        root.interface_at = Some(mixed_interface_at);
        let checked =
            // SAFETY: the complete fixture root and table outlive validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect("valid sibling");
        assert_eq!(checked.interfaces.len(), 1);
        assert!(matches!(
            checked.interfaces[0].table,
            CheckedInterfaceTable::PipelineEventHook(_)
        ));
        assert_eq!(checked.diagnostics.len(), 1);
        assert_eq!(checked.diagnostics[0].index, 0);
        assert!(matches!(
            checked.diagnostics[0].error,
            NativePluginContractError::UnsupportedInterfaceVersion { .. }
        ));
        assert_eq!(owner.query_calls.load(Ordering::SeqCst), 1);
        let loaded = LoadedNativePlugin::from_checked(checked);
        assert_eq!(loaded.diagnostics().len(), 1);
        assert_eq!(
            loaded.diagnostics()[0].kind,
            PluginContractDiagnosticKind::Compatibility
        );
        let implicit = PluginReference::new("dev.vesper.fixture", None, PluginTransport::Native)
            .expect("valid implicit reference");
        assert_eq!(
            loaded
                .resolve_frame_processor(&implicit)
                .err()
                .expect("known rejected interface is unavailable"),
            PluginSelectionError::InterfaceUnavailable {
                plugin_id: "dev.vesper.fixture".to_owned(),
                interface: "FrameProcessor",
            }
        );
        let explicit = PluginReference::new(
            "dev.vesper.fixture",
            Some(String::from_utf8_lossy(BAD_INSTANCE_ID).into_owned()),
            PluginTransport::Native,
        )
        .expect("valid explicit reference");
        assert_eq!(
            loaded
                .resolve_frame_processor(&explicit)
                .err()
                .expect("known rejected instance is unavailable"),
            PluginSelectionError::InstanceUnavailable {
                plugin_id: "dev.vesper.fixture".to_owned(),
                interface: "FrameProcessor",
                instance_id: String::from_utf8_lossy(BAD_INSTANCE_ID).into_owned(),
            }
        );
        drop(loaded);
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn zero_interface_root_is_rejected_and_destroyed() {
        let mut hook = VesperPipelineEventHook {
            header: VesperInterfaceHeader::new(
                size_of::<VesperPipelineEventHook>() as u32,
                PIPELINE_EVENT_HOOK_INTERFACE_ID,
                VESPER_INTERFACE_MAJOR,
                VESPER_INTERFACE_MINOR,
                NonNull::<u8>::dangling().as_ptr().cast(),
            ),
            on_event_json: Some(on_event),
        };
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let mut root = root_for(&mut owner, PLUGIN_ID);
        root.interface_count = 0;
        let error =
            // SAFETY: the complete fixture root outlives validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect_err("zero interfaces");
        assert_eq!(error, NativePluginContractError::NoInterfaces);
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn future_root_minor_is_rejected_as_a_compatibility_error() {
        let mut hook = hook_table();
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: 0,
        };
        let mut root = root_for(&mut owner, PLUGIN_ID);
        root.abi_minor = VESPER_PLUGIN_ABI_MINOR.saturating_add(1);

        let error =
            // SAFETY: the complete fixture root outlives validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect_err("future root minor");
        assert_eq!(
            error,
            NativePluginContractError::RootVersionMismatch {
                expected_major: VESPER_PLUGIN_ABI_MAJOR,
                expected_minor: VESPER_PLUGIN_ABI_MINOR,
                actual_major: VESPER_PLUGIN_ABI_MAJOR,
                actual_minor: VESPER_PLUGIN_ABI_MINOR.saturating_add(1),
            }
        );
        assert_eq!(
            error.diagnostic_kind(),
            PluginContractDiagnosticKind::Compatibility
        );
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn truncated_interface_is_isolated_before_callback_read() {
        let original_size = size_of::<VesperPipelineEventHook>() as u32;
        let mut hook = VesperPipelineEventHook {
            header: VesperInterfaceHeader::new(
                original_size,
                PIPELINE_EVENT_HOOK_INTERFACE_ID,
                VESPER_INTERFACE_MAJOR,
                VESPER_INTERFACE_MINOR,
                NonNull::<u8>::dangling().as_ptr().cast(),
            ),
            on_event_json: Some(on_event),
        };
        let mut owner = FixtureOwner {
            destroyed: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
            table: std::ptr::from_mut(&mut hook).cast(),
            table_size_override: VESPER_PIPELINE_EVENT_HOOK_REQUIRED_SIZE - 1,
        };
        let root = root_for(&mut owner, PLUGIN_ID);
        let checked =
            // SAFETY: the fixture root and header outlive validation.
            unsafe { CheckedPluginRoot::from_raw(&root, None) }.expect("isolated table failure");
        assert!(checked.interfaces.is_empty());
        assert_eq!(checked.diagnostics.len(), 1);
        assert!(matches!(
            checked.diagnostics[0].error,
            NativePluginContractError::TruncatedInterface { .. }
        ));
        assert_eq!(
            checked.diagnostics[0].error.diagnostic_kind(),
            PluginContractDiagnosticKind::ContractViolation
        );
        drop(checked);
        assert_eq!(owner.destroyed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn entry_signature_matches_root_pointer_contract() {
        unsafe extern "C" fn entry() -> *const VesperPluginRoot {
            std::ptr::null()
        }
        let _entry: VesperPluginEntryPoint = entry;
    }
}
