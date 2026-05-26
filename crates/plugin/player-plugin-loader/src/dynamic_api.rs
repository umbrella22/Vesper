use super::*;

#[derive(Debug, Error)]
pub enum PluginLoadError {
    #[error("failed to open plugin library at {path}: {source}")]
    OpenLibrary {
        path: String,
        #[source]
        source: libloading::Error,
    },
    #[error("failed to resolve plugin entry symbol `{symbol}`: {source}")]
    ResolveEntrySymbol {
        symbol: &'static str,
        #[source]
        source: libloading::Error,
    },
    #[error("plugin descriptor pointer is null")]
    NullDescriptor,
    #[error("plugin ABI version mismatch: expected {expected}, got {actual}")]
    AbiVersionMismatch { expected: u32, actual: u32 },
    #[error("plugin field `{field}` is missing")]
    MissingField { field: &'static str },
    #[error("plugin field `{field}` is not valid UTF-8")]
    InvalidUtf8 { field: &'static str },
    #[error("failed to decode plugin capabilities JSON: {0}")]
    DecodeCapabilities(#[source] serde_json::Error),
    #[error("plugin capabilities payload violates ABI: {0}")]
    CapabilitiesAbiViolation(String),
}

#[derive(Debug)]
pub struct LoadedDynamicPlugin {
    name: String,
    plugin_kind: VesperPluginKind,
    post_download_processor: Option<Arc<DynamicPostDownloadProcessor>>,
    pipeline_event_hook: Option<Arc<DynamicPipelineEventHook>>,
    benchmark_sink: Option<Arc<DynamicBenchmarkSink>>,
    native_decoder_plugin_factory: Option<Arc<DynamicNativeDecoderPluginFactory>>,
    frame_processor_plugin_factory: Option<Arc<DynamicFrameProcessorPluginFactory>>,
    source_normalizer_packet_plugin_factory:
        Option<Arc<DynamicSourceNormalizerPacketPluginFactory>>,
    source_normalizer_resource_plugin_factory:
        Option<Arc<DynamicSourceNormalizerResourcePluginFactory>>,
}

impl LoadedDynamicPlugin {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PluginLoadError> {
        let path = path.as_ref();
        let path_string = path.display().to_string();
        // SAFETY: `path` comes from the caller, and the resulting `Library` is
        // stored in `LibraryHolder` so any symbols borrowed from it stay valid.
        let library =
            unsafe { Library::new(path) }.map_err(|source| PluginLoadError::OpenLibrary {
                path: path_string,
                source,
            })?;

        // SAFETY: the symbol name is a static NUL-terminated byte string and
        // the plugin contract requires it to have the `VesperPluginEntryPoint`
        // signature.
        let entry = unsafe { library.get::<VesperPluginEntryPoint>(VESPER_PLUGIN_ENTRY_SYMBOL) }
            .map_err(|source| PluginLoadError::ResolveEntrySymbol {
                symbol: "vesper_plugin_entry",
                source,
            })?;

        // SAFETY: the plugin entry point follows the shared ABI and returns a
        // process-lifetime descriptor pointer when loading succeeds.
        let descriptor_ptr = unsafe { entry() };
        let descriptor =
            // SAFETY: `descriptor_ptr` came from `vesper_plugin_entry`; the ABI
            // guarantees it points to a valid descriptor or null on failure.
            unsafe { descriptor_ptr.as_ref() }.ok_or(PluginLoadError::NullDescriptor)?;
        let library = Arc::new(LibraryHolder { library });
        Self::from_descriptor(Some(library), descriptor)
    }

    pub fn plugin_name(&self) -> &str {
        &self.name
    }

    pub fn plugin_kind(&self) -> VesperPluginKind {
        self.plugin_kind
    }

    pub fn post_download_processor(&self) -> Option<Arc<dyn PostDownloadProcessor>> {
        self.post_download_processor
            .clone()
            .map(|processor| processor as Arc<dyn PostDownloadProcessor>)
    }

    pub fn pipeline_event_hook(&self) -> Option<Arc<dyn PipelineEventHook>> {
        self.pipeline_event_hook
            .clone()
            .map(|hook| hook as Arc<dyn PipelineEventHook>)
    }

    pub fn benchmark_sink(&self) -> Option<Arc<dyn BenchmarkSink>> {
        self.benchmark_sink
            .clone()
            .map(|sink| sink as Arc<dyn BenchmarkSink>)
    }

    pub fn native_decoder_plugin_factory(&self) -> Option<Arc<dyn NativeDecoderPluginFactory>> {
        self.native_decoder_plugin_factory
            .clone()
            .map(|factory| factory as Arc<dyn NativeDecoderPluginFactory>)
    }

    pub fn frame_processor_plugin_factory(&self) -> Option<Arc<dyn FrameProcessorPluginFactory>> {
        self.frame_processor_plugin_factory
            .clone()
            .map(|factory| factory as Arc<dyn FrameProcessorPluginFactory>)
    }

    pub fn source_normalizer_packet_plugin_factory(
        &self,
    ) -> Option<Arc<dyn SourceNormalizerPacketPluginFactory>> {
        self.source_normalizer_packet_plugin_factory
            .clone()
            .map(|factory| factory as Arc<dyn SourceNormalizerPacketPluginFactory>)
    }

    pub fn source_normalizer_resource_plugin_factory(
        &self,
    ) -> Option<Arc<dyn SourceNormalizerResourcePluginFactory>> {
        self.source_normalizer_resource_plugin_factory
            .clone()
            .map(|factory| factory as Arc<dyn SourceNormalizerResourcePluginFactory>)
    }

    pub(crate) fn from_descriptor(
        library: Option<Arc<LibraryHolder>>,
        descriptor: &VesperPluginDescriptor,
    ) -> Result<Self, PluginLoadError> {
        let expected_abi_version = match descriptor.plugin_kind {
            VesperPluginKind::PostDownloadProcessor => VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
            VesperPluginKind::PipelineEventHook | VesperPluginKind::BenchmarkSink => {
                VESPER_PLUGIN_ABI_VERSION_V2
            }
            VesperPluginKind::Decoder => VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            VesperPluginKind::FrameProcessor => VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
            VesperPluginKind::SourceNormalizer => {
                if descriptor.abi_version != VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2
                    && descriptor.abi_version != VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V3
                {
                    return Err(PluginLoadError::AbiVersionMismatch {
                        expected: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V3,
                        actual: descriptor.abi_version,
                    });
                }
                descriptor.abi_version
            }
        };
        if descriptor.plugin_kind != VesperPluginKind::SourceNormalizer
            && descriptor.abi_version != expected_abi_version
        {
            return Err(PluginLoadError::AbiVersionMismatch {
                expected: expected_abi_version,
                actual: descriptor.abi_version,
            });
        }

        let descriptor_name = c_string_field(descriptor.plugin_name, "plugin_name")?;
        match descriptor.plugin_kind {
            VesperPluginKind::PostDownloadProcessor => {
                let api_ptr = descriptor.api.cast::<VesperPostDownloadProcessorApi>();
                let api =
                    // SAFETY: `descriptor.api` must point at the ABI table that
                    // matches `plugin_kind` when the plugin exports a valid
                    // descriptor.
                    unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                        field: "post_download_processor_api",
                    })?;
                let processor = DynamicPostDownloadProcessor::new(
                    library,
                    descriptor_name.clone(),
                    CheckedPostDownloadProcessorApi::try_from(*api)?,
                )?;
                Ok(Self {
                    name: descriptor_name,
                    plugin_kind: descriptor.plugin_kind,
                    post_download_processor: Some(Arc::new(processor)),
                    pipeline_event_hook: None,
                    benchmark_sink: None,
                    native_decoder_plugin_factory: None,
                    frame_processor_plugin_factory: None,
                    source_normalizer_packet_plugin_factory: None,
                    source_normalizer_resource_plugin_factory: None,
                })
            }
            VesperPluginKind::PipelineEventHook => {
                let api_ptr = descriptor.api.cast::<VesperPipelineEventHookApi>();
                let api =
                    // SAFETY: `descriptor.api` must point at the ABI table that
                    // matches `plugin_kind` when the plugin exports a valid
                    // descriptor.
                    unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                        field: "pipeline_event_hook_api",
                    })?;
                let hook = DynamicPipelineEventHook::new(
                    library,
                    descriptor_name.clone(),
                    CheckedPipelineEventHookApi::try_from(*api)?,
                )?;
                Ok(Self {
                    name: descriptor_name,
                    plugin_kind: descriptor.plugin_kind,
                    post_download_processor: None,
                    pipeline_event_hook: Some(Arc::new(hook)),
                    benchmark_sink: None,
                    native_decoder_plugin_factory: None,
                    frame_processor_plugin_factory: None,
                    source_normalizer_packet_plugin_factory: None,
                    source_normalizer_resource_plugin_factory: None,
                })
            }
            VesperPluginKind::BenchmarkSink => {
                let api_ptr = descriptor.api.cast::<VesperBenchmarkSinkApi>();
                let api =
                    // SAFETY: `descriptor.api` must point at the ABI table that
                    // matches `plugin_kind` when the plugin exports a valid
                    // descriptor.
                    unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                        field: "benchmark_sink_api",
                    })?;
                let sink = DynamicBenchmarkSink::new(
                    library,
                    descriptor_name.clone(),
                    CheckedBenchmarkSinkApi::try_from(*api)?,
                )?;
                Ok(Self {
                    name: descriptor_name,
                    plugin_kind: descriptor.plugin_kind,
                    post_download_processor: None,
                    pipeline_event_hook: None,
                    benchmark_sink: Some(Arc::new(sink)),
                    native_decoder_plugin_factory: None,
                    frame_processor_plugin_factory: None,
                    source_normalizer_packet_plugin_factory: None,
                    source_normalizer_resource_plugin_factory: None,
                })
            }
            VesperPluginKind::Decoder => {
                let api_ptr = descriptor.api.cast::<VesperDecoderPluginApiV2>();
                let api =
                    // SAFETY: `descriptor.api` must point at the v2 decoder ABI table
                    // when the plugin exports a valid decoder descriptor.
                    unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                        field: "decoder_plugin_api_v2",
                    })?;
                let factory = DynamicNativeDecoderPluginFactory::new(
                    library,
                    descriptor_name.clone(),
                    CheckedNativeDecoderPluginApi::try_from(*api)?,
                )?;
                Ok(Self {
                    name: descriptor_name,
                    plugin_kind: descriptor.plugin_kind,
                    post_download_processor: None,
                    pipeline_event_hook: None,
                    benchmark_sink: None,
                    native_decoder_plugin_factory: Some(Arc::new(factory)),
                    frame_processor_plugin_factory: None,
                    source_normalizer_packet_plugin_factory: None,
                    source_normalizer_resource_plugin_factory: None,
                })
            }
            VesperPluginKind::FrameProcessor => {
                let api_ptr = descriptor.api.cast::<VesperFrameProcessorPluginApiV1>();
                let api =
                    // SAFETY: `descriptor.api` must point at the v1 frame processor
                    // ABI table when the plugin exports a valid frame processor descriptor.
                    unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                        field: "frame_processor_plugin_api_v1",
                    })?;
                let factory = DynamicFrameProcessorPluginFactory::new(
                    library,
                    descriptor_name.clone(),
                    CheckedFrameProcessorPluginApi::try_from(*api)?,
                )?;
                Ok(Self {
                    name: descriptor_name,
                    plugin_kind: descriptor.plugin_kind,
                    post_download_processor: None,
                    pipeline_event_hook: None,
                    benchmark_sink: None,
                    native_decoder_plugin_factory: None,
                    frame_processor_plugin_factory: Some(Arc::new(factory)),
                    source_normalizer_packet_plugin_factory: None,
                    source_normalizer_resource_plugin_factory: None,
                })
            }
            VesperPluginKind::SourceNormalizer => {
                let (packet_factory, resource_factory) = if descriptor.abi_version
                    == VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V3
                {
                    let api_ptr = descriptor.api.cast::<VesperSourceNormalizerPluginApiV3>();
                    let api =
                            // SAFETY: `descriptor.api` must point at the v3 source normalizer
                            // ABI table when the plugin exports a valid v3 descriptor.
                            unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                                field: "source_normalizer_plugin_api_v3",
                            })?;
                    let packet_api = CheckedSourceNormalizerPacketPluginApi::try_from(*api)?;
                    let resource_api = CheckedSourceNormalizerResourcePluginApi::try_from(*api)?;
                    let packet_factory = DynamicSourceNormalizerPacketPluginFactory::new(
                        library.clone(),
                        descriptor_name.clone(),
                        packet_api,
                    )?;
                    let resource_factory = DynamicSourceNormalizerResourcePluginFactory::new(
                        library,
                        descriptor_name.clone(),
                        resource_api,
                    )?;
                    (
                        Some(Arc::new(packet_factory)),
                        Some(Arc::new(resource_factory)),
                    )
                } else {
                    let api_ptr = descriptor.api.cast::<VesperSourceNormalizerPluginApiV2>();
                    let api =
                            // SAFETY: `descriptor.api` must point at the v2 source normalizer
                            // ABI table when the plugin exports a valid source normalizer descriptor.
                            unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                                field: "source_normalizer_plugin_api_v2",
                            })?;
                    let factory = DynamicSourceNormalizerPacketPluginFactory::new(
                        library,
                        descriptor_name.clone(),
                        CheckedSourceNormalizerPacketPluginApi::try_from(*api)?,
                    )?;
                    (Some(Arc::new(factory)), None)
                };
                Ok(Self {
                    name: descriptor_name,
                    plugin_kind: descriptor.plugin_kind,
                    post_download_processor: None,
                    pipeline_event_hook: None,
                    benchmark_sink: None,
                    native_decoder_plugin_factory: None,
                    frame_processor_plugin_factory: None,
                    source_normalizer_packet_plugin_factory: packet_factory,
                    source_normalizer_resource_plugin_factory: resource_factory,
                })
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct LibraryHolder {
    #[allow(dead_code)]
    library: Library,
}

pub(crate) type DestroyFn = unsafe extern "C" fn(context: *mut c_void);
pub(crate) type NameFn = unsafe extern "C" fn(context: *mut c_void) -> *const c_char;
pub(crate) type CapabilitiesJsonFn =
    unsafe extern "C" fn(context: *mut c_void) -> VesperPluginBytes;
pub(crate) type FreeBytesFn =
    unsafe extern "C" fn(context: *mut c_void, payload: VesperPluginBytes);
pub(crate) type ProcessJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    input_json: *const u8,
    input_json_len: usize,
    output_path: *const c_char,
    progress: VesperPluginProgressCallbacks,
) -> VesperPluginProcessResult;
pub(crate) type OnEventJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    event_json: *const u8,
    event_json_len: usize,
) -> bool;
pub(crate) type OnBenchmarkEventBatchJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    batch_json: *const u8,
    batch_json_len: usize,
) -> VesperPluginProcessResult;
pub(crate) type BenchmarkFlushJsonFn =
    unsafe extern "C" fn(context: *mut c_void) -> VesperPluginProcessResult;
pub(crate) type DecoderOpenSessionJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperDecoderOpenSessionResult;
pub(crate) type DecoderSendPacketFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    packet_json: *const u8,
    packet_json_len: usize,
    packet_data: *const u8,
    packet_data_len: usize,
) -> VesperPluginProcessResult;
pub(crate) type DecoderReceiveNativeFrameFn =
    unsafe extern "C" fn(
        context: *mut c_void,
        session: *mut c_void,
    ) -> VesperDecoderReceiveNativeFrameResult;
pub(crate) type DecoderReleaseNativeFrameFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    handle_kind: u32,
    handle: usize,
) -> VesperPluginProcessResult;
pub(crate) type DecoderSessionOperationFn =
    unsafe extern "C" fn(context: *mut c_void, session: *mut c_void) -> VesperPluginProcessResult;
pub(crate) type FrameProcessorOpenSessionJsonFn =
    unsafe extern "C" fn(
        context: *mut c_void,
        config_json: *const u8,
        config_json_len: usize,
    ) -> VesperFrameProcessorOpenSessionResult;
pub(crate) type FrameProcessorSubmitFrameJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    submit_json: *const u8,
    submit_json_len: usize,
    handle: usize,
)
    -> VesperPluginProcessResult;
pub(crate) type FrameProcessorReceiveFrameFn =
    unsafe extern "C" fn(
        context: *mut c_void,
        session: *mut c_void,
    ) -> VesperFrameProcessorReceiveFrameResult;
pub(crate) type FrameProcessorReleaseFrameFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    handle_kind: u32,
    handle: usize,
) -> VesperPluginProcessResult;
pub(crate) type FrameProcessorSessionOperationFn =
    unsafe extern "C" fn(context: *mut c_void, session: *mut c_void) -> VesperPluginProcessResult;
pub(crate) type SourceNormalizerSeekSessionJsonFn =
    unsafe extern "C" fn(
        context: *mut c_void,
        session: *mut c_void,
        seek_json: *const u8,
        seek_json_len: usize,
    ) -> VesperPluginProcessResult;
pub(crate) type SourceNormalizerSessionOperationFn =
    unsafe extern "C" fn(context: *mut c_void, session: *mut c_void) -> VesperPluginProcessResult;
pub(crate) type SourceNormalizerOpenPacketSessionJsonFn =
    unsafe extern "C" fn(
        context: *mut c_void,
        config_json: *const u8,
        config_json_len: usize,
    ) -> VesperSourceNormalizerOpenPacketSessionResult;
pub(crate) type SourceNormalizerReadPacketFn =
    unsafe extern "C" fn(
        context: *mut c_void,
        session: *mut c_void,
    ) -> VesperSourceNormalizerReadPacketResult;
pub(crate) type SourceNormalizerReleasePacketFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    packet_handle: usize,
)
    -> VesperPluginProcessResult;
pub(crate) type SourceNormalizerOpenResourceSessionJsonFn =
    unsafe extern "C" fn(
        context: *mut c_void,
        config_json: *const u8,
        config_json_len: usize,
    ) -> VesperSourceNormalizerOpenResourceSessionResult;

#[derive(Debug, Clone, Copy)]
pub(crate) struct CheckedPostDownloadProcessorApi {
    pub(crate) context: *mut c_void,
    pub(crate) destroy: Option<DestroyFn>,
    pub(crate) name: Option<NameFn>,
    pub(crate) capabilities_json: CapabilitiesJsonFn,
    pub(crate) free_bytes: FreeBytesFn,
    pub(crate) process_json: ProcessJsonFn,
    pub(crate) assemble_json: ProcessJsonFn,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through
// `PostDownloadProcessor`.
unsafe impl Send for CheckedPostDownloadProcessorApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedPostDownloadProcessorApi {}

impl TryFrom<VesperPostDownloadProcessorApi> for CheckedPostDownloadProcessorApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperPostDownloadProcessorApi) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            capabilities_json: api.capabilities_json.ok_or(PluginLoadError::MissingField {
                field: "post_download_processor_api.capabilities_json",
            })?,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "post_download_processor_api.free_bytes",
            })?,
            process_json: api.process_json.ok_or(PluginLoadError::MissingField {
                field: "post_download_processor_api.process_json",
            })?,
            assemble_json: api.assemble_json.ok_or(PluginLoadError::MissingField {
                field: "post_download_processor_api.assemble_json",
            })?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CheckedPipelineEventHookApi {
    pub(crate) context: *mut c_void,
    pub(crate) destroy: Option<DestroyFn>,
    pub(crate) name: Option<NameFn>,
    pub(crate) on_event_json: OnEventJsonFn,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through
// `PipelineEventHook`.
unsafe impl Send for CheckedPipelineEventHookApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedPipelineEventHookApi {}

impl TryFrom<VesperPipelineEventHookApi> for CheckedPipelineEventHookApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperPipelineEventHookApi) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            on_event_json: api.on_event_json.ok_or(PluginLoadError::MissingField {
                field: "pipeline_event_hook_api.on_event_json",
            })?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CheckedBenchmarkSinkApi {
    pub(crate) context: *mut c_void,
    pub(crate) destroy: Option<DestroyFn>,
    pub(crate) name: Option<NameFn>,
    pub(crate) free_bytes: FreeBytesFn,
    pub(crate) on_event_batch_json: OnBenchmarkEventBatchJsonFn,
    pub(crate) flush_json: Option<BenchmarkFlushJsonFn>,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through `BenchmarkSink`.
unsafe impl Send for CheckedBenchmarkSinkApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedBenchmarkSinkApi {}

impl TryFrom<VesperBenchmarkSinkApi> for CheckedBenchmarkSinkApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperBenchmarkSinkApi) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "benchmark_sink_api.free_bytes",
            })?,
            on_event_batch_json: api
                .on_event_batch_json
                .ok_or(PluginLoadError::MissingField {
                    field: "benchmark_sink_api.on_event_batch_json",
                })?,
            flush_json: api.flush_json,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CheckedNativeDecoderPluginApi {
    pub(crate) context: *mut c_void,
    pub(crate) destroy: Option<DestroyFn>,
    pub(crate) name: Option<NameFn>,
    pub(crate) capabilities_json: CapabilitiesJsonFn,
    pub(crate) native_requirements_json: CapabilitiesJsonFn,
    pub(crate) free_bytes: FreeBytesFn,
    pub(crate) open_session_json: DecoderOpenSessionJsonFn,
    pub(crate) send_packet: DecoderSendPacketFn,
    pub(crate) receive_native_frame: DecoderReceiveNativeFrameFn,
    pub(crate) release_native_frame: DecoderReleaseNativeFrameFn,
    pub(crate) flush_session: DecoderSessionOperationFn,
    pub(crate) close_session: DecoderSessionOperationFn,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through
// `NativeDecoderPluginFactory`.
unsafe impl Send for CheckedNativeDecoderPluginApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedNativeDecoderPluginApi {}

impl TryFrom<VesperDecoderPluginApiV2> for CheckedNativeDecoderPluginApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperDecoderPluginApiV2) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            capabilities_json: api.capabilities_json.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api_v2.capabilities_json",
            })?,
            native_requirements_json: api.native_requirements_json.ok_or(
                PluginLoadError::MissingField {
                    field: "decoder_plugin_api_v2.native_requirements_json",
                },
            )?,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api_v2.free_bytes",
            })?,
            open_session_json: api.open_session_json.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api_v2.open_session_json",
            })?,
            send_packet: api.send_packet.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api_v2.send_packet",
            })?,
            receive_native_frame: api.receive_native_frame.ok_or(
                PluginLoadError::MissingField {
                    field: "decoder_plugin_api_v2.receive_native_frame",
                },
            )?,
            release_native_frame: api.release_native_frame.ok_or(
                PluginLoadError::MissingField {
                    field: "decoder_plugin_api_v2.release_native_frame",
                },
            )?,
            flush_session: api.flush_session.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api_v2.flush_session",
            })?,
            close_session: api.close_session.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api_v2.close_session",
            })?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CheckedFrameProcessorPluginApi {
    pub(crate) context: *mut c_void,
    pub(crate) destroy: Option<DestroyFn>,
    pub(crate) name: Option<NameFn>,
    pub(crate) capabilities_json: CapabilitiesJsonFn,
    pub(crate) free_bytes: FreeBytesFn,
    pub(crate) open_session_json: FrameProcessorOpenSessionJsonFn,
    pub(crate) submit_frame_json: FrameProcessorSubmitFrameJsonFn,
    pub(crate) receive_frame: FrameProcessorReceiveFrameFn,
    pub(crate) release_frame: FrameProcessorReleaseFrameFn,
    pub(crate) flush_session: FrameProcessorSessionOperationFn,
    pub(crate) close_session: FrameProcessorSessionOperationFn,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through
// `FrameProcessorPluginFactory`.
unsafe impl Send for CheckedFrameProcessorPluginApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedFrameProcessorPluginApi {}

impl TryFrom<VesperFrameProcessorPluginApiV1> for CheckedFrameProcessorPluginApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperFrameProcessorPluginApiV1) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            capabilities_json: api.capabilities_json.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.capabilities_json",
            })?,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.free_bytes",
            })?,
            open_session_json: api.open_session_json.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.open_session_json",
            })?,
            submit_frame_json: api.submit_frame_json.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.submit_frame_json",
            })?,
            receive_frame: api.receive_frame.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.receive_frame",
            })?,
            release_frame: api.release_frame.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.release_frame",
            })?,
            flush_session: api.flush_session.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.flush_session",
            })?,
            close_session: api.close_session.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.close_session",
            })?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CheckedSourceNormalizerPacketPluginApi {
    pub(crate) context: *mut c_void,
    pub(crate) destroy: Option<DestroyFn>,
    pub(crate) name: Option<NameFn>,
    pub(crate) packet_capabilities_json: CapabilitiesJsonFn,
    pub(crate) free_bytes: FreeBytesFn,
    pub(crate) open_packet_session_json: SourceNormalizerOpenPacketSessionJsonFn,
    pub(crate) read_packet: SourceNormalizerReadPacketFn,
    pub(crate) release_packet: SourceNormalizerReleasePacketFn,
    pub(crate) seek_packet_session_json: Option<SourceNormalizerSeekSessionJsonFn>,
    pub(crate) flush_packet_session: SourceNormalizerSessionOperationFn,
    pub(crate) close_packet_session: SourceNormalizerSessionOperationFn,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through
// `SourceNormalizerPacketPluginFactory`.
unsafe impl Send for CheckedSourceNormalizerPacketPluginApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedSourceNormalizerPacketPluginApi {}

impl TryFrom<VesperSourceNormalizerPluginApiV2> for CheckedSourceNormalizerPacketPluginApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperSourceNormalizerPluginApiV2) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            packet_capabilities_json: api.packet_capabilities_json.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v2.packet_capabilities_json",
                },
            )?,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "source_normalizer_plugin_api_v2.free_bytes",
            })?,
            open_packet_session_json: api.open_packet_session_json.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v2.open_packet_session_json",
                },
            )?,
            read_packet: api.read_packet.ok_or(PluginLoadError::MissingField {
                field: "source_normalizer_plugin_api_v2.read_packet",
            })?,
            release_packet: api.release_packet.ok_or(PluginLoadError::MissingField {
                field: "source_normalizer_plugin_api_v2.release_packet",
            })?,
            seek_packet_session_json: api.seek_packet_session_json,
            flush_packet_session: api.flush_packet_session.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v2.flush_packet_session",
                },
            )?,
            close_packet_session: api.close_packet_session.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v2.close_packet_session",
                },
            )?,
        })
    }
}

impl TryFrom<VesperSourceNormalizerPluginApiV3> for CheckedSourceNormalizerPacketPluginApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperSourceNormalizerPluginApiV3) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: None,
            name: api.name,
            packet_capabilities_json: api.packet_capabilities_json.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v3.packet_capabilities_json",
                },
            )?,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "source_normalizer_plugin_api_v3.free_bytes",
            })?,
            open_packet_session_json: api.open_packet_session_json.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v3.open_packet_session_json",
                },
            )?,
            read_packet: api.read_packet.ok_or(PluginLoadError::MissingField {
                field: "source_normalizer_plugin_api_v3.read_packet",
            })?,
            release_packet: api.release_packet.ok_or(PluginLoadError::MissingField {
                field: "source_normalizer_plugin_api_v3.release_packet",
            })?,
            seek_packet_session_json: api.seek_packet_session_json,
            flush_packet_session: api.flush_packet_session.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v3.flush_packet_session",
                },
            )?,
            close_packet_session: api.close_packet_session.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v3.close_packet_session",
                },
            )?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CheckedSourceNormalizerResourcePluginApi {
    pub(crate) context: *mut c_void,
    pub(crate) destroy: Option<DestroyFn>,
    pub(crate) name: Option<NameFn>,
    pub(crate) resource_capabilities_json: CapabilitiesJsonFn,
    pub(crate) free_bytes: FreeBytesFn,
    pub(crate) open_resource_session_json: SourceNormalizerOpenResourceSessionJsonFn,
    pub(crate) poll_resource_session: SourceNormalizerSessionOperationFn,
    pub(crate) cancel_resource_session: SourceNormalizerSessionOperationFn,
    pub(crate) close_resource_session: SourceNormalizerSessionOperationFn,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through
// `SourceNormalizerResourcePluginFactory`.
unsafe impl Send for CheckedSourceNormalizerResourcePluginApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedSourceNormalizerResourcePluginApi {}

impl TryFrom<VesperSourceNormalizerPluginApiV3> for CheckedSourceNormalizerResourcePluginApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperSourceNormalizerPluginApiV3) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            resource_capabilities_json: api.resource_capabilities_json.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v3.resource_capabilities_json",
                },
            )?,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "source_normalizer_plugin_api_v3.free_bytes",
            })?,
            open_resource_session_json: api.open_resource_session_json.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v3.open_resource_session_json",
                },
            )?,
            poll_resource_session: api.poll_resource_session.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v3.poll_resource_session",
                },
            )?,
            cancel_resource_session: api.cancel_resource_session.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v3.cancel_resource_session",
                },
            )?,
            close_resource_session: api.close_resource_session.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v3.close_resource_session",
                },
            )?,
        })
    }
}

pub(crate) fn native_handle_kind_code(handle_kind: &NativeHandleKind) -> Result<u32, String> {
    match handle_kind {
        NativeHandleKind::CvPixelBuffer => Ok(1),
        NativeHandleKind::IoSurface => Ok(2),
        NativeHandleKind::MetalTexture => Ok(3),
        NativeHandleKind::DmaBuf => Ok(4),
        NativeHandleKind::VaapiSurface => Ok(5),
        NativeHandleKind::D3D11Texture2D => Ok(6),
        NativeHandleKind::DxgiSurface => Ok(7),
        NativeHandleKind::VulkanImage => Ok(8),
        NativeHandleKind::Unknown(kind) => Err(format!(
            "native handle kind `{kind}` cannot be released through the dynamic plugin ABI"
        )),
    }
}
