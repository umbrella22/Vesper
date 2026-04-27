#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::Library;
use player_plugin::{
    CompletedDownloadInfo, DecoderCapabilities, DecoderCodecCapability, DecoderError,
    DecoderMediaKind, DecoderNativeFrame, DecoderNativeHandleKind, DecoderOperationStatus,
    DecoderPacket, DecoderPacketResult, DecoderPluginFactory, DecoderReceiveFrameMetadata,
    DecoderReceiveFrameOutput, DecoderReceiveFrameStatus, DecoderReceiveNativeFrameMetadata,
    DecoderReceiveNativeFrameOutput, DecoderSession, DecoderSessionConfig, DecoderSessionInfo,
    NativeDecoderPluginFactory, NativeDecoderSession, PipelineEvent, PipelineEventHook,
    PostDownloadProcessor, ProcessorCapabilities, ProcessorError, ProcessorOutput,
    ProcessorProgress, VESPER_DECODER_PLUGIN_ABI_VERSION_V2, VESPER_PLUGIN_ABI_VERSION,
    VESPER_PLUGIN_ENTRY_SYMBOL, VesperDecoderOpenSessionResult, VesperDecoderPluginApi,
    VesperDecoderPluginApiV2, VesperDecoderReceiveFrameResult,
    VesperDecoderReceiveNativeFrameResult, VesperPipelineEventHookApi, VesperPluginBytes,
    VesperPluginDescriptor, VesperPluginEntryPoint, VesperPluginKind, VesperPluginProcessResult,
    VesperPluginProgressCallbacks, VesperPluginResultStatus, VesperPostDownloadProcessorApi,
};
use serde::de::DeserializeOwned;
use thiserror::Error;

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

#[derive(Debug, Error)]
enum PluginPayloadError {
    #[error("plugin payload pointer is null while len is {len}")]
    NullPayloadWithLength { len: usize },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug)]
pub struct LoadedDynamicPlugin {
    name: String,
    plugin_kind: VesperPluginKind,
    post_download_processor: Option<Arc<DynamicPostDownloadProcessor>>,
    pipeline_event_hook: Option<Arc<DynamicPipelineEventHook>>,
    decoder_plugin_factory: Option<Arc<DynamicDecoderPluginFactory>>,
    native_decoder_plugin_factory: Option<Arc<DynamicNativeDecoderPluginFactory>>,
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

    pub fn decoder_plugin_factory(&self) -> Option<Arc<dyn DecoderPluginFactory>> {
        self.decoder_plugin_factory
            .clone()
            .map(|factory| factory as Arc<dyn DecoderPluginFactory>)
    }

    pub fn native_decoder_plugin_factory(&self) -> Option<Arc<dyn NativeDecoderPluginFactory>> {
        self.native_decoder_plugin_factory
            .clone()
            .map(|factory| factory as Arc<dyn NativeDecoderPluginFactory>)
    }

    fn from_descriptor(
        library: Option<Arc<LibraryHolder>>,
        descriptor: &VesperPluginDescriptor,
    ) -> Result<Self, PluginLoadError> {
        if descriptor.abi_version != VESPER_PLUGIN_ABI_VERSION
            && descriptor.abi_version != VESPER_DECODER_PLUGIN_ABI_VERSION_V2
        {
            return Err(PluginLoadError::AbiVersionMismatch {
                expected: VESPER_PLUGIN_ABI_VERSION,
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
                    decoder_plugin_factory: None,
                    native_decoder_plugin_factory: None,
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
                    decoder_plugin_factory: None,
                    native_decoder_plugin_factory: None,
                })
            }
            VesperPluginKind::Decoder => {
                if descriptor.abi_version == VESPER_DECODER_PLUGIN_ABI_VERSION_V2 {
                    let api_ptr = descriptor.api.cast::<VesperDecoderPluginApiV2>();
                    let api =
                        // SAFETY: `descriptor.api` must point at the ABI table that
                        // matches `plugin_kind` and `abi_version`.
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
                        decoder_plugin_factory: None,
                        native_decoder_plugin_factory: Some(Arc::new(factory)),
                    })
                } else {
                    let api_ptr = descriptor.api.cast::<VesperDecoderPluginApi>();
                    let api =
                        // SAFETY: `descriptor.api` must point at the ABI table that
                        // matches `plugin_kind` when the plugin exports a valid
                        // descriptor.
                        unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                            field: "decoder_plugin_api",
                        })?;
                    let factory = DynamicDecoderPluginFactory::new(
                        library,
                        descriptor_name.clone(),
                        CheckedDecoderPluginApi::try_from(*api)?,
                    )?;
                    Ok(Self {
                        name: descriptor_name,
                        plugin_kind: descriptor.plugin_kind,
                        post_download_processor: None,
                        pipeline_event_hook: None,
                        decoder_plugin_factory: Some(Arc::new(factory)),
                        native_decoder_plugin_factory: None,
                    })
                }
            }
        }
    }
}

/// Codec/media request used when matching decoder plugin capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderPluginMatchRequest {
    pub codec: String,
    pub media_kind: DecoderMediaKind,
}

impl DecoderPluginMatchRequest {
    pub fn video(codec: impl Into<String>) -> Self {
        Self {
            codec: codec.into(),
            media_kind: DecoderMediaKind::Video,
        }
    }
}

/// Structured codec entry reported by one decoder plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderPluginCodecSummary {
    pub codec: String,
    pub media_kind: DecoderMediaKind,
}

impl From<&DecoderCodecCapability> for DecoderPluginCodecSummary {
    fn from(capability: &DecoderCodecCapability) -> Self {
        Self {
            codec: capability.codec.clone(),
            media_kind: capability.media_kind,
        }
    }
}

/// Compact capability summary for one decoder plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderPluginCapabilitySummary {
    pub typed_codecs: Vec<DecoderPluginCodecSummary>,
    pub codecs: Vec<String>,
    pub supports_native_frame_output: bool,
    pub supports_hardware_decode: bool,
    pub supports_cpu_video_frames: bool,
    pub supports_audio_frames: bool,
    pub supports_gpu_handles: bool,
    pub supports_flush: bool,
    pub supports_drain: bool,
    pub max_sessions: Option<u32>,
}

impl From<&DecoderCapabilities> for DecoderPluginCapabilitySummary {
    fn from(capabilities: &DecoderCapabilities) -> Self {
        Self::from_capabilities(capabilities, false)
    }
}

impl DecoderPluginCapabilitySummary {
    fn from_capabilities(
        capabilities: &DecoderCapabilities,
        supports_native_frame_output: bool,
    ) -> Self {
        let typed_codecs = capabilities
            .codecs
            .iter()
            .map(DecoderPluginCodecSummary::from)
            .collect::<Vec<_>>();
        let codecs = capabilities
            .codecs
            .iter()
            .map(|codec| format!("{:?}:{}", codec.media_kind, codec.codec))
            .collect();
        Self {
            typed_codecs,
            codecs,
            supports_native_frame_output,
            supports_hardware_decode: capabilities.supports_hardware_decode,
            supports_cpu_video_frames: capabilities.supports_cpu_video_frames,
            supports_audio_frames: capabilities.supports_audio_frames,
            supports_gpu_handles: capabilities.supports_gpu_handles,
            supports_flush: capabilities.supports_flush,
            supports_drain: capabilities.supports_drain,
            max_sessions: capabilities.max_sessions,
        }
    }
}

/// Loader-side diagnostic status for one plugin path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginDiagnosticStatus {
    Loaded,
    LoadFailed,
    UnsupportedKind,
    DecoderSupported,
    DecoderUnsupported,
}

/// Structured diagnostic record for one dynamic plugin path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDiagnosticRecord {
    pub path: PathBuf,
    pub status: PluginDiagnosticStatus,
    pub plugin_name: Option<String>,
    pub plugin_kind: Option<VesperPluginKind>,
    pub decoder_capabilities: Option<DecoderPluginCapabilitySummary>,
    pub message: Option<String>,
}

impl PluginDiagnosticRecord {
    pub fn from_loaded_plugin(
        path: impl Into<PathBuf>,
        plugin: &LoadedDynamicPlugin,
        decoder_match: Option<&DecoderPluginMatchRequest>,
    ) -> Self {
        let path = path.into();
        match decoder_factory_summary(plugin) {
            Some((name, capabilities, native_frame_output)) => match decoder_match {
                Some(request)
                    if capabilities.supports_codec(&request.codec, request.media_kind) =>
                {
                    Self {
                        path,
                        status: PluginDiagnosticStatus::DecoderSupported,
                        plugin_name: Some(name.clone()),
                        plugin_kind: Some(plugin.plugin_kind()),
                        decoder_capabilities: Some(
                            DecoderPluginCapabilitySummary::from_capabilities(
                                &capabilities,
                                native_frame_output,
                            ),
                        ),
                        message: Some(format!(
                            "{} advertises {:?} {} support{}",
                            name,
                            request.media_kind,
                            request.codec,
                            if native_frame_output {
                                " with native-frame output"
                            } else {
                                ""
                            }
                        )),
                    }
                }
                Some(request) => Self {
                    path,
                    status: PluginDiagnosticStatus::DecoderUnsupported,
                    plugin_name: Some(name.clone()),
                    plugin_kind: Some(plugin.plugin_kind()),
                    decoder_capabilities: Some(DecoderPluginCapabilitySummary::from_capabilities(
                        &capabilities,
                        native_frame_output,
                    )),
                    message: Some(format!(
                        "{} does not advertise {:?} {} support",
                        name, request.media_kind, request.codec
                    )),
                },
                None => Self {
                    path,
                    status: PluginDiagnosticStatus::Loaded,
                    plugin_name: Some(name.clone()),
                    plugin_kind: Some(plugin.plugin_kind()),
                    decoder_capabilities: Some(DecoderPluginCapabilitySummary::from_capabilities(
                        &capabilities,
                        native_frame_output,
                    )),
                    message: Some(format!(
                        "{} decoder plugin loaded{}",
                        name,
                        if native_frame_output {
                            " with native-frame output"
                        } else {
                            ""
                        }
                    )),
                },
            },
            None => Self {
                path,
                status: PluginDiagnosticStatus::UnsupportedKind,
                plugin_name: Some(plugin.plugin_name().to_owned()),
                plugin_kind: Some(plugin.plugin_kind()),
                decoder_capabilities: None,
                message: Some(format!("{} is not a decoder plugin", plugin.plugin_name())),
            },
        }
    }

    pub fn load_failed(path: impl Into<PathBuf>, error: PluginLoadError) -> Self {
        let path = path.into();
        Self {
            path,
            status: PluginDiagnosticStatus::LoadFailed,
            plugin_name: None,
            plugin_kind: None,
            decoder_capabilities: None,
            message: Some(error.to_string()),
        }
    }

    pub fn summary(&self) -> String {
        self.message
            .clone()
            .or_else(|| self.plugin_name.clone())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

fn decoder_factory_summary(
    plugin: &LoadedDynamicPlugin,
) -> Option<(String, DecoderCapabilities, bool)> {
    if let Some(factory) = plugin.native_decoder_plugin_factory() {
        return Some((factory.name().to_owned(), factory.capabilities(), true));
    }
    plugin
        .decoder_plugin_factory()
        .map(|factory| (factory.name().to_owned(), factory.capabilities(), false))
}

/// Aggregated loader-side report for inspected dynamic plugin paths.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginRegistryReport {
    pub total: usize,
    pub loaded: usize,
    pub failed: usize,
    pub decoder_supported: usize,
    pub decoder_unsupported: usize,
    pub unsupported_kind: usize,
    pub best_supported_decoder_name: Option<String>,
    pub diagnostic_notes: Vec<String>,
}

/// Structured report for dynamic plugins loaded from host-provided paths.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginRegistry {
    records: Vec<PluginDiagnosticRecord>,
}

impl PluginRegistry {
    pub fn inspect_decoder_support(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
        request: DecoderPluginMatchRequest,
    ) -> Self {
        let records = paths
            .into_iter()
            .map(|path| {
                let path = path.as_ref().to_path_buf();
                match LoadedDynamicPlugin::load(&path) {
                    Ok(plugin) => {
                        PluginDiagnosticRecord::from_loaded_plugin(path, &plugin, Some(&request))
                    }
                    Err(error) => PluginDiagnosticRecord::load_failed(path, error),
                }
            })
            .collect();
        Self { records }
    }

    pub fn from_records(records: Vec<PluginDiagnosticRecord>) -> Self {
        Self { records }
    }

    pub fn records(&self) -> &[PluginDiagnosticRecord] {
        &self.records
    }

    pub fn best_decoder_for(
        &self,
        request: &DecoderPluginMatchRequest,
    ) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::DecoderSupported
                && record
                    .decoder_capabilities
                    .as_ref()
                    .is_some_and(|capabilities| {
                        capabilities.typed_codecs.iter().any(|codec| {
                            codec.media_kind == request.media_kind
                                && codec.codec.eq_ignore_ascii_case(&request.codec)
                        })
                    })
        })
    }

    pub fn best_native_decoder_for(
        &self,
        request: &DecoderPluginMatchRequest,
    ) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::DecoderSupported
                && record
                    .decoder_capabilities
                    .as_ref()
                    .is_some_and(|capabilities| {
                        capabilities.supports_native_frame_output
                            && capabilities.typed_codecs.iter().any(|codec| {
                                codec.media_kind == request.media_kind
                                    && codec.codec.eq_ignore_ascii_case(&request.codec)
                            })
                    })
        })
    }

    pub fn supports_decoder(&self, request: &DecoderPluginMatchRequest) -> bool {
        self.best_decoder_for(request).is_some()
    }

    pub fn supports_native_decoder(&self, request: &DecoderPluginMatchRequest) -> bool {
        self.best_native_decoder_for(request).is_some()
    }

    pub fn decoder_supported_plugin_names(&self) -> Vec<&str> {
        self.records
            .iter()
            .filter(|record| record.status == PluginDiagnosticStatus::DecoderSupported)
            .filter_map(|record| record.plugin_name.as_deref())
            .collect()
    }

    pub fn diagnostic_notes(&self) -> Vec<String> {
        self.records
            .iter()
            .filter(|record| record.status != PluginDiagnosticStatus::DecoderSupported)
            .map(PluginDiagnosticRecord::summary)
            .collect()
    }

    pub fn report(&self) -> PluginRegistryReport {
        let mut report = PluginRegistryReport {
            total: self.records.len(),
            ..PluginRegistryReport::default()
        };

        for record in &self.records {
            match record.status {
                PluginDiagnosticStatus::Loaded => {
                    report.loaded += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::LoadFailed => {
                    report.failed += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::UnsupportedKind => {
                    report.loaded += 1;
                    report.unsupported_kind += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::DecoderSupported => {
                    report.loaded += 1;
                    report.decoder_supported += 1;
                    if report.best_supported_decoder_name.is_none() {
                        report.best_supported_decoder_name = record.plugin_name.clone();
                    }
                }
                PluginDiagnosticStatus::DecoderUnsupported => {
                    report.loaded += 1;
                    report.decoder_unsupported += 1;
                    report.diagnostic_notes.push(record.summary());
                }
            }
        }

        report
    }
}

#[derive(Debug)]
struct LibraryHolder {
    #[allow(dead_code)]
    library: Library,
}

type DestroyFn = unsafe extern "C" fn(context: *mut c_void);
type NameFn = unsafe extern "C" fn(context: *mut c_void) -> *const c_char;
type CapabilitiesJsonFn = unsafe extern "C" fn(context: *mut c_void) -> VesperPluginBytes;
type FreeBytesFn = unsafe extern "C" fn(context: *mut c_void, payload: VesperPluginBytes);
type ProcessJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    input_json: *const u8,
    input_json_len: usize,
    output_path: *const c_char,
    progress: VesperPluginProgressCallbacks,
) -> VesperPluginProcessResult;
type OnEventJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    event_json: *const u8,
    event_json_len: usize,
) -> bool;
type DecoderOpenSessionJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperDecoderOpenSessionResult;
type DecoderSendPacketFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    packet_json: *const u8,
    packet_json_len: usize,
    packet_data: *const u8,
    packet_data_len: usize,
) -> VesperPluginProcessResult;
type DecoderReceiveFrameFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
) -> VesperDecoderReceiveFrameResult;
type DecoderReceiveNativeFrameFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
) -> VesperDecoderReceiveNativeFrameResult;
type DecoderReleaseNativeFrameFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    handle_kind: u32,
    handle: usize,
) -> VesperPluginProcessResult;
type DecoderSessionOperationFn =
    unsafe extern "C" fn(context: *mut c_void, session: *mut c_void) -> VesperPluginProcessResult;

#[derive(Debug, Clone, Copy)]
struct CheckedPostDownloadProcessorApi {
    context: *mut c_void,
    destroy: Option<DestroyFn>,
    name: Option<NameFn>,
    capabilities_json: CapabilitiesJsonFn,
    free_bytes: FreeBytesFn,
    process_json: ProcessJsonFn,
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
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct CheckedPipelineEventHookApi {
    context: *mut c_void,
    destroy: Option<DestroyFn>,
    name: Option<NameFn>,
    on_event_json: OnEventJsonFn,
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
struct CheckedDecoderPluginApi {
    context: *mut c_void,
    destroy: Option<DestroyFn>,
    name: Option<NameFn>,
    capabilities_json: CapabilitiesJsonFn,
    free_bytes: FreeBytesFn,
    open_session_json: DecoderOpenSessionJsonFn,
    send_packet: DecoderSendPacketFn,
    receive_frame: DecoderReceiveFrameFn,
    flush_session: DecoderSessionOperationFn,
    close_session: DecoderSessionOperationFn,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through
// `DecoderPluginFactory`.
unsafe impl Send for CheckedDecoderPluginApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedDecoderPluginApi {}

impl TryFrom<VesperDecoderPluginApi> for CheckedDecoderPluginApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperDecoderPluginApi) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            capabilities_json: api.capabilities_json.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api.capabilities_json",
            })?,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api.free_bytes",
            })?,
            open_session_json: api.open_session_json.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api.open_session_json",
            })?,
            send_packet: api.send_packet.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api.send_packet",
            })?,
            receive_frame: api.receive_frame.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api.receive_frame",
            })?,
            flush_session: api.flush_session.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api.flush_session",
            })?,
            close_session: api.close_session.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api.close_session",
            })?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct CheckedNativeDecoderPluginApi {
    context: *mut c_void,
    destroy: Option<DestroyFn>,
    name: Option<NameFn>,
    capabilities_json: CapabilitiesJsonFn,
    free_bytes: FreeBytesFn,
    open_session_json: DecoderOpenSessionJsonFn,
    send_packet: DecoderSendPacketFn,
    receive_native_frame: DecoderReceiveNativeFrameFn,
    release_native_frame: DecoderReleaseNativeFrameFn,
    flush_session: DecoderSessionOperationFn,
    close_session: DecoderSessionOperationFn,
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

#[derive(Debug)]
struct DynamicPostDownloadProcessorInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedPostDownloadProcessorApi,
    capabilities: ProcessorCapabilities,
}

impl Drop for DynamicPostDownloadProcessorInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
struct DynamicPostDownloadProcessor {
    inner: Arc<DynamicPostDownloadProcessorInner>,
}

impl DynamicPostDownloadProcessor {
    fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedPostDownloadProcessorApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "processor_name")?
            }
        } else {
            fallback_name
        };
        let capabilities = decode_plugin_bytes::<ProcessorCapabilities>(
            // SAFETY: the validated API guarantees `capabilities_json` and
            // `free_bytes` are present and use the shared `VesperPluginBytes`
            // ownership contract documented in `player-plugin`.
            unsafe { (api.capabilities_json)(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;

        Ok(Self {
            inner: Arc::new(DynamicPostDownloadProcessorInner {
                library,
                name,
                api,
                capabilities,
            }),
        })
    }
}

impl PostDownloadProcessor for DynamicPostDownloadProcessor {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn supported_input_formats(&self) -> &[player_plugin::ContentFormatKind] {
        &self.inner.capabilities.supported_input_formats
    }

    fn capabilities(&self) -> ProcessorCapabilities {
        self.inner.capabilities.clone()
    }

    fn process(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        let input_json = serde_json::to_vec(input).map_err(|error| {
            ProcessorError::PayloadCodec(format!(
                "serialize dynamic plugin input for `{}` failed: {error}",
                self.inner.name
            ))
        })?;
        let output_path = CString::new(output_path.to_string_lossy().as_bytes()).map_err(|_| {
            ProcessorError::OutputPath(format!(
                "output path for plugin `{}` contains interior NUL",
                self.inner.name
            ))
        })?;

        let mut adapter = ProgressAdapter { progress };
        let callbacks = VesperPluginProgressCallbacks {
            context: (&mut adapter as *mut ProgressAdapter<'_>).cast::<c_void>(),
            on_progress: Some(progress_on_progress),
            is_cancelled: Some(progress_is_cancelled),
        };

        // SAFETY: the validated plugin API guarantees `process_json` is present.
        // `input_json` and `output_path` live for the duration of the call, and
        // the ABI contract documents that `callbacks.context` is only valid
        // during this synchronous invocation.
        let result = unsafe {
            (self.inner.api.process_json)(
                self.inner.api.context,
                input_json.as_ptr(),
                input_json.len(),
                output_path.as_ptr(),
                callbacks,
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => decode_plugin_bytes::<ProcessorOutput>(
                result.payload,
                self.inner.api.free_bytes,
                self.inner.api.context,
            )
            .map_err(|error| map_plugin_payload_error(&self.inner.name, "success", error)),
            VesperPluginResultStatus::Failure => decode_plugin_bytes::<ProcessorError>(
                result.payload,
                self.inner.api.free_bytes,
                self.inner.api.context,
            )
            .map_err(|error| map_plugin_payload_error(&self.inner.name, "error", error))
            .and_then(Err),
        }
    }
}

#[derive(Debug)]
struct DynamicPipelineEventHookInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    #[allow(dead_code)]
    name: String,
    api: CheckedPipelineEventHookApi,
}

impl Drop for DynamicPipelineEventHookInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
struct DynamicPipelineEventHook {
    inner: Arc<DynamicPipelineEventHookInner>,
}

impl DynamicPipelineEventHook {
    fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedPipelineEventHookApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "hook_name")?
            }
        } else {
            fallback_name
        };

        Ok(Self {
            inner: Arc::new(DynamicPipelineEventHookInner { library, name, api }),
        })
    }
}

impl PipelineEventHook for DynamicPipelineEventHook {
    fn on_event(&self, event: &PipelineEvent) {
        let Ok(event_json) = serde_json::to_vec(event) else {
            return;
        };

        // SAFETY: the validated hook API guarantees `on_event_json` is present,
        // and `event_json` remains alive for the duration of this synchronous
        // callback.
        let _ = unsafe {
            (self.inner.api.on_event_json)(
                self.inner.api.context,
                event_json.as_ptr(),
                event_json.len(),
            )
        };
    }
}

#[derive(Debug)]
struct DynamicDecoderPluginFactoryInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedDecoderPluginApi,
    capabilities: DecoderCapabilities,
}

impl Drop for DynamicDecoderPluginFactoryInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
struct DynamicDecoderPluginFactory {
    inner: Arc<DynamicDecoderPluginFactoryInner>,
}

impl DynamicDecoderPluginFactory {
    fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedDecoderPluginApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "decoder_name")?
            }
        } else {
            fallback_name
        };
        let capabilities = decode_plugin_bytes::<DecoderCapabilities>(
            // SAFETY: the validated API guarantees `capabilities_json` and
            // `free_bytes` are present and use the shared `VesperPluginBytes`
            // ownership contract documented in `player-plugin`.
            unsafe { (api.capabilities_json)(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;

        Ok(Self {
            inner: Arc::new(DynamicDecoderPluginFactoryInner {
                library,
                name,
                api,
                capabilities,
            }),
        })
    }
}

impl DecoderPluginFactory for DynamicDecoderPluginFactory {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn capabilities(&self) -> DecoderCapabilities {
        self.inner.capabilities.clone()
    }

    fn open_session(
        &self,
        config: &DecoderSessionConfig,
    ) -> Result<Box<dyn DecoderSession>, DecoderError> {
        let config_json = serde_json::to_vec(config).map_err(|error| {
            DecoderError::payload_codec(format!(
                "serialize decoder config for `{}` failed: {error}",
                self.inner.name
            ))
        })?;

        // SAFETY: the validated plugin API guarantees `open_session_json` is
        // present, and `config_json` remains alive for the duration of this
        // synchronous callback.
        let result = unsafe {
            (self.inner.api.open_session_json)(
                self.inner.api.context,
                config_json.as_ptr(),
                config_json.len(),
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => {
                if result.session.is_null() {
                    reclaim_decoder_payload(
                        result.payload,
                        self.inner.api.free_bytes,
                        self.inner.api.context,
                    );
                    return Err(DecoderError::abi_violation(format!(
                        "decoder plugin `{}` returned a null session pointer",
                        self.inner.name
                    )));
                }
                let session_info = decode_plugin_bytes_or_default::<DecoderSessionInfo>(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                )
                .map_err(|error| map_decoder_payload_error(&self.inner.name, "open", error))?;
                Ok(Box::new(DynamicDecoderSession {
                    factory: self.inner.clone(),
                    session: result.session,
                    session_info,
                    closed: false,
                }))
            }
            VesperPluginResultStatus::Failure => {
                let error = decode_decoder_error_payload(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                    &self.inner.name,
                    "open",
                );
                Err(error)
            }
        }
    }
}

#[derive(Debug)]
struct DynamicNativeDecoderPluginFactoryInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedNativeDecoderPluginApi,
    capabilities: DecoderCapabilities,
}

impl Drop for DynamicNativeDecoderPluginFactoryInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
struct DynamicNativeDecoderPluginFactory {
    inner: Arc<DynamicNativeDecoderPluginFactoryInner>,
}

impl DynamicNativeDecoderPluginFactory {
    fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedNativeDecoderPluginApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "decoder_name")?
            }
        } else {
            fallback_name
        };
        let capabilities = decode_plugin_bytes::<DecoderCapabilities>(
            // SAFETY: the validated API guarantees `capabilities_json` and
            // `free_bytes` are present and use the shared `VesperPluginBytes`
            // ownership contract documented in `player-plugin`.
            unsafe { (api.capabilities_json)(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;

        Ok(Self {
            inner: Arc::new(DynamicNativeDecoderPluginFactoryInner {
                library,
                name,
                api,
                capabilities,
            }),
        })
    }
}

impl NativeDecoderPluginFactory for DynamicNativeDecoderPluginFactory {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn capabilities(&self) -> DecoderCapabilities {
        self.inner.capabilities.clone()
    }

    fn open_native_session(
        &self,
        config: &DecoderSessionConfig,
    ) -> Result<Box<dyn NativeDecoderSession>, DecoderError> {
        let config_json = serde_json::to_vec(config).map_err(|error| {
            DecoderError::payload_codec(format!(
                "serialize native decoder config for `{}` failed: {error}",
                self.inner.name
            ))
        })?;

        // SAFETY: the validated plugin API guarantees `open_session_json` is
        // present, and `config_json` remains alive for the duration of this
        // synchronous callback.
        let result = unsafe {
            (self.inner.api.open_session_json)(
                self.inner.api.context,
                config_json.as_ptr(),
                config_json.len(),
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => {
                if result.session.is_null() {
                    reclaim_decoder_payload(
                        result.payload,
                        self.inner.api.free_bytes,
                        self.inner.api.context,
                    );
                    return Err(DecoderError::abi_violation(format!(
                        "native decoder plugin `{}` returned a null session pointer",
                        self.inner.name
                    )));
                }
                let session_info = decode_plugin_bytes_or_default::<DecoderSessionInfo>(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                )
                .map_err(|error| {
                    map_decoder_payload_error(&self.inner.name, "open_native", error)
                })?;
                Ok(Box::new(DynamicNativeDecoderSession {
                    factory: self.inner.clone(),
                    session: result.session,
                    session_info,
                    closed: false,
                }))
            }
            VesperPluginResultStatus::Failure => {
                let error = decode_decoder_error_payload(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                    &self.inner.name,
                    "open_native",
                );
                Err(error)
            }
        }
    }
}

#[derive(Debug)]
struct DynamicDecoderSession {
    factory: Arc<DynamicDecoderPluginFactoryInner>,
    session: *mut c_void,
    session_info: DecoderSessionInfo,
    closed: bool,
}

// SAFETY: the dynamic decoder session is only exposed through `DecoderSession:
// Send`; the plugin ABI requires the opaque session pointer to be safe to move
// across threads when exported through this API.
unsafe impl Send for DynamicDecoderSession {}

impl DynamicDecoderSession {
    fn ensure_open(&self) -> Result<(), DecoderError> {
        if self.closed || self.session.is_null() {
            Err(DecoderError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn decode_operation_result(
        &self,
        result: VesperPluginProcessResult,
        operation: &'static str,
    ) -> Result<(), DecoderError> {
        match result.status {
            VesperPluginResultStatus::Success => {
                let _ = decode_plugin_bytes_or_default::<DecoderOperationStatus>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| map_decoder_payload_error(&self.factory.name, operation, error))?;
                Ok(())
            }
            VesperPluginResultStatus::Failure => Err(decode_decoder_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                operation,
            )),
        }
    }
}

impl DecoderSession for DynamicDecoderSession {
    fn session_info(&self) -> DecoderSessionInfo {
        self.session_info.clone()
    }

    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        self.ensure_open()?;
        let packet_json = serde_json::to_vec(packet).map_err(|error| {
            DecoderError::payload_codec(format!(
                "serialize decoder packet for `{}` failed: {error}",
                self.factory.name
            ))
        })?;
        let data_ptr = if data.is_empty() {
            std::ptr::null()
        } else {
            data.as_ptr()
        };

        // SAFETY: the validated plugin API guarantees `send_packet` is present.
        // The JSON and packet data buffers remain alive for this synchronous call.
        let result = unsafe {
            (self.factory.api.send_packet)(
                self.factory.api.context,
                self.session,
                packet_json.as_ptr(),
                packet_json.len(),
                data_ptr,
                data.len(),
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => decode_plugin_bytes_or_default::<
                DecoderPacketResult,
            >(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
            )
            .map_err(|error| map_decoder_payload_error(&self.factory.name, "send_packet", error)),
            VesperPluginResultStatus::Failure => Err(decode_decoder_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "send_packet",
            )),
        }
    }

    fn receive_frame(&mut self) -> Result<DecoderReceiveFrameOutput, DecoderError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `receive_frame` is present
        // and returns plugin-owned byte buffers reclaimed below.
        let result =
            unsafe { (self.factory.api.receive_frame)(self.factory.api.context, self.session) };
        let data_result = copy_plugin_bytes(
            result.data,
            self.factory.api.free_bytes,
            self.factory.api.context,
        );

        match result.status {
            VesperPluginResultStatus::Success => {
                let metadata = decode_plugin_bytes::<DecoderReceiveFrameMetadata>(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_decoder_payload_error(&self.factory.name, "receive_frame", error)
                })?;
                let data = data_result.map_err(|error| {
                    map_decoder_payload_error(&self.factory.name, "receive_frame_data", error)
                })?;
                match metadata.status {
                    DecoderReceiveFrameStatus::Frame => {
                        let frame = metadata.frame.ok_or_else(|| {
                            DecoderError::abi_violation(format!(
                                "decoder plugin `{}` returned frame status without frame metadata",
                                self.factory.name
                            ))
                        })?;
                        Ok(DecoderReceiveFrameOutput::Frame(
                            player_plugin::DecoderFrame {
                                metadata: frame,
                                data,
                            },
                        ))
                    }
                    DecoderReceiveFrameStatus::NeedMoreInput => {
                        Ok(DecoderReceiveFrameOutput::NeedMoreInput)
                    }
                    DecoderReceiveFrameStatus::Eof => Ok(DecoderReceiveFrameOutput::Eof),
                }
            }
            VesperPluginResultStatus::Failure => {
                let _ = data_result;
                Err(decode_decoder_error_payload(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                    &self.factory.name,
                    "receive_frame",
                ))
            }
        }
    }

    fn flush(&mut self) -> Result<(), DecoderError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `flush_session` is present.
        let result =
            unsafe { (self.factory.api.flush_session)(self.factory.api.context, self.session) };
        self.decode_operation_result(result, "flush")
    }

    fn close(&mut self) -> Result<(), DecoderError> {
        if self.closed || self.session.is_null() {
            return Ok(());
        }
        // SAFETY: the validated plugin API guarantees `close_session` is present
        // and consumes or releases the opaque session pointer exactly once.
        let result =
            unsafe { (self.factory.api.close_session)(self.factory.api.context, self.session) };
        self.closed = true;
        self.session = std::ptr::null_mut();
        self.decode_operation_result(result, "close")
    }
}

impl Drop for DynamicDecoderSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[derive(Debug)]
struct DynamicNativeDecoderSession {
    factory: Arc<DynamicNativeDecoderPluginFactoryInner>,
    session: *mut c_void,
    session_info: DecoderSessionInfo,
    closed: bool,
}

// SAFETY: the dynamic native decoder session is only exposed through
// `NativeDecoderSession: Send`; the plugin ABI requires the opaque session
// pointer to be safe to move across threads when exported through this API.
unsafe impl Send for DynamicNativeDecoderSession {}

impl DynamicNativeDecoderSession {
    fn ensure_open(&self) -> Result<(), DecoderError> {
        if self.closed || self.session.is_null() {
            Err(DecoderError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn decode_operation_result(
        &self,
        result: VesperPluginProcessResult,
        operation: &'static str,
    ) -> Result<(), DecoderError> {
        match result.status {
            VesperPluginResultStatus::Success => {
                let _ = decode_plugin_bytes_or_default::<DecoderOperationStatus>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| map_decoder_payload_error(&self.factory.name, operation, error))?;
                Ok(())
            }
            VesperPluginResultStatus::Failure => Err(decode_decoder_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                operation,
            )),
        }
    }
}

impl NativeDecoderSession for DynamicNativeDecoderSession {
    fn session_info(&self) -> DecoderSessionInfo {
        self.session_info.clone()
    }

    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        self.ensure_open()?;
        let packet_json = serde_json::to_vec(packet).map_err(|error| {
            DecoderError::payload_codec(format!(
                "serialize native decoder packet for `{}` failed: {error}",
                self.factory.name
            ))
        })?;
        let data_ptr = if data.is_empty() {
            std::ptr::null()
        } else {
            data.as_ptr()
        };

        // SAFETY: the validated plugin API guarantees `send_packet` is present.
        // The JSON and packet data buffers remain alive for this synchronous call.
        let result = unsafe {
            (self.factory.api.send_packet)(
                self.factory.api.context,
                self.session,
                packet_json.as_ptr(),
                packet_json.len(),
                data_ptr,
                data.len(),
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => decode_plugin_bytes_or_default::<
                DecoderPacketResult,
            >(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
            )
            .map_err(|error| map_decoder_payload_error(&self.factory.name, "send_packet", error)),
            VesperPluginResultStatus::Failure => Err(decode_decoder_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "send_packet",
            )),
        }
    }

    fn receive_native_frame(&mut self) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `receive_native_frame` is
        // present and returns plugin-owned byte buffers reclaimed below.
        let result = unsafe {
            (self.factory.api.receive_native_frame)(self.factory.api.context, self.session)
        };

        match result.status {
            VesperPluginResultStatus::Success => {
                let metadata = decode_plugin_bytes::<DecoderReceiveNativeFrameMetadata>(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_decoder_payload_error(&self.factory.name, "receive_native_frame", error)
                })?;
                match metadata.status {
                    DecoderReceiveFrameStatus::Frame => {
                        if result.handle == 0 {
                            return Err(DecoderError::abi_violation(format!(
                                "native decoder plugin `{}` returned frame status with a null handle",
                                self.factory.name
                            )));
                        }
                        let frame = metadata.frame.ok_or_else(|| {
                            DecoderError::abi_violation(format!(
                                "native decoder plugin `{}` returned frame status without frame metadata",
                                self.factory.name
                            ))
                        })?;
                        Ok(DecoderReceiveNativeFrameOutput::Frame(DecoderNativeFrame {
                            metadata: frame,
                            handle: result.handle,
                        }))
                    }
                    DecoderReceiveFrameStatus::NeedMoreInput => {
                        Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput)
                    }
                    DecoderReceiveFrameStatus::Eof => Ok(DecoderReceiveNativeFrameOutput::Eof),
                }
            }
            VesperPluginResultStatus::Failure => Err(decode_decoder_error_payload(
                result.metadata,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "receive_native_frame",
            )),
        }
    }

    fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
        self.ensure_open()?;
        let handle_kind = native_handle_kind_code(&frame.metadata.handle_kind)?;
        // SAFETY: the validated plugin API guarantees `release_native_frame` is
        // present. The handle came from the same plugin session.
        let result = unsafe {
            (self.factory.api.release_native_frame)(
                self.factory.api.context,
                self.session,
                handle_kind,
                frame.handle,
            )
        };
        self.decode_operation_result(result, "release_native_frame")
    }

    fn flush(&mut self) -> Result<(), DecoderError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `flush_session` is present.
        let result =
            unsafe { (self.factory.api.flush_session)(self.factory.api.context, self.session) };
        self.decode_operation_result(result, "flush")
    }

    fn close(&mut self) -> Result<(), DecoderError> {
        if self.closed || self.session.is_null() {
            return Ok(());
        }
        // SAFETY: the validated plugin API guarantees `close_session` is present
        // and consumes or releases the opaque session pointer exactly once.
        let result =
            unsafe { (self.factory.api.close_session)(self.factory.api.context, self.session) };
        self.closed = true;
        self.session = std::ptr::null_mut();
        self.decode_operation_result(result, "close")
    }
}

impl Drop for DynamicNativeDecoderSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn native_handle_kind_code(handle_kind: &DecoderNativeHandleKind) -> Result<u32, DecoderError> {
    match handle_kind {
        DecoderNativeHandleKind::CvPixelBuffer => Ok(1),
        DecoderNativeHandleKind::IoSurface => Ok(2),
        DecoderNativeHandleKind::MetalTexture => Ok(3),
        DecoderNativeHandleKind::DmaBuf => Ok(4),
        DecoderNativeHandleKind::VaapiSurface => Ok(5),
        DecoderNativeHandleKind::D3D11Texture2D => Ok(6),
        DecoderNativeHandleKind::DxgiSurface => Ok(7),
        DecoderNativeHandleKind::VulkanImage => Ok(8),
        DecoderNativeHandleKind::Unknown(kind) => Err(DecoderError::abi_violation(format!(
            "native decoder handle kind `{kind}` cannot be released through ABI v2"
        ))),
    }
}

struct ProgressAdapter<'a> {
    progress: &'a dyn ProcessorProgress,
}

unsafe extern "C" fn progress_on_progress(context: *mut c_void, ratio: f32) {
    // SAFETY: `context` is created from `ProgressAdapter` immediately before the
    // synchronous `process_json` call and remains valid until that call returns.
    let adapter = unsafe { &*(context.cast::<ProgressAdapter<'_>>()) };
    adapter.progress.on_progress(ratio);
}

unsafe extern "C" fn progress_is_cancelled(context: *mut c_void) -> bool {
    // SAFETY: `context` is created from `ProgressAdapter` immediately before the
    // synchronous `process_json` call and remains valid until that call returns.
    let adapter = unsafe { &*(context.cast::<ProgressAdapter<'_>>()) };
    adapter.progress.is_cancelled()
}

fn c_string_field(pointer: *const c_char, field: &'static str) -> Result<String, PluginLoadError> {
    if pointer.is_null() {
        return Err(PluginLoadError::MissingField { field });
    }

    // SAFETY: `pointer` has been checked for null and the plugin ABI requires
    // all string fields to be valid NUL-terminated strings.
    let value = unsafe { CStr::from_ptr(pointer) };
    value
        .to_str()
        .map(|value| value.to_owned())
        .map_err(|_| PluginLoadError::InvalidUtf8 { field })
}

fn map_plugin_payload_error(
    plugin_name: &str,
    payload_kind: &str,
    error: PluginPayloadError,
) -> ProcessorError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => ProcessorError::AbiViolation(format!(
            "plugin `{plugin_name}` returned {payload_kind} payload with null data pointer and len {len}"
        )),
        PluginPayloadError::Json(error) => ProcessorError::PayloadCodec(format!(
            "decode plugin `{plugin_name}` {payload_kind} payload failed: {error}"
        )),
    }
}

fn map_capabilities_payload_error(error: PluginPayloadError) -> PluginLoadError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => {
            PluginLoadError::CapabilitiesAbiViolation(format!(
                "plugin returned capabilities payload with null data pointer and len {len}"
            ))
        }
        PluginPayloadError::Json(error) => PluginLoadError::DecodeCapabilities(error),
    }
}

fn map_decoder_payload_error(
    plugin_name: &str,
    payload_kind: &str,
    error: PluginPayloadError,
) -> DecoderError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => DecoderError::abi_violation(format!(
            "decoder plugin `{plugin_name}` returned {payload_kind} payload with null data pointer and len {len}"
        )),
        PluginPayloadError::Json(error) => DecoderError::payload_codec(format!(
            "decode decoder plugin `{plugin_name}` {payload_kind} payload failed: {error}"
        )),
    }
}

fn decode_decoder_error_payload(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
    plugin_name: &str,
    payload_kind: &str,
) -> DecoderError {
    decode_plugin_bytes::<DecoderError>(payload, free_bytes, context)
        .unwrap_or_else(|error| map_decoder_payload_error(plugin_name, payload_kind, error))
}

fn decode_plugin_bytes_or_default<T: Default + DeserializeOwned>(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
) -> Result<T, PluginPayloadError> {
    if payload.data.is_null() && payload.len == 0 {
        // SAFETY: this is a no-op for the null/empty payload and keeps the
        // ownership rule symmetric for all plugin-returned buffers.
        unsafe { free_bytes(context, payload) };
        return Ok(T::default());
    }
    decode_plugin_bytes(payload, free_bytes, context)
}

fn copy_plugin_bytes(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
) -> Result<Vec<u8>, PluginPayloadError> {
    let payload_has_null_data = payload.data.is_null();
    let bytes = if payload_has_null_data || payload.len == 0 {
        Vec::new()
    } else {
        // SAFETY: the plugin ABI requires non-null payloads to point to
        // `payload.len` initialized bytes until `free_bytes` is called.
        let slice = unsafe { std::slice::from_raw_parts(payload.data, payload.len) };
        slice.to_vec()
    };

    // SAFETY: `free_bytes` is the validated deallocator paired with this
    // payload, and the payload is not used again after this call.
    unsafe { free_bytes(context, payload) };

    if payload_has_null_data && payload.len > 0 {
        return Err(PluginPayloadError::NullPayloadWithLength { len: payload.len });
    }

    Ok(bytes)
}

fn reclaim_decoder_payload(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
) {
    // SAFETY: `free_bytes` is the validated deallocator paired with this
    // payload, and the payload is intentionally discarded.
    unsafe { free_bytes(context, payload) };
}

fn decode_plugin_bytes<T: DeserializeOwned>(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
) -> Result<T, PluginPayloadError> {
    let payload_has_null_data = payload.data.is_null();
    let bytes = if payload_has_null_data || payload.len == 0 {
        Vec::new()
    } else {
        // SAFETY: the plugin ABI requires non-null payloads to point to
        // `payload.len` initialized bytes until `free_bytes` is called.
        let slice = unsafe { std::slice::from_raw_parts(payload.data, payload.len) };
        slice.to_vec()
    };

    // SAFETY: `free_bytes` is the validated deallocator paired with this
    // payload, and the payload is not used again after this call.
    unsafe { free_bytes(context, payload) };

    if payload_has_null_data && payload.len > 0 {
        return Err(PluginPayloadError::NullPayloadWithLength { len: payload.len });
    }

    serde_json::from_slice(&bytes).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::{
        DecoderPluginCodecSummary, DecoderPluginMatchRequest, LoadedDynamicPlugin,
        PluginDiagnosticRecord, PluginDiagnosticStatus, PluginLoadError, PluginRegistry,
    };
    use player_plugin::{
        CompletedContentFormat, CompletedDownloadInfo, ContentFormatKind, DecoderCapabilities,
        DecoderCodecCapability, DecoderError, DecoderFrameFormat, DecoderFrameMetadata,
        DecoderFramePlane, DecoderMediaKind, DecoderNativeDeviceContext,
        DecoderNativeDeviceContextKind, DecoderNativeFrameMetadata, DecoderNativeHandleKind,
        DecoderOperationStatus, DecoderPacket, DecoderPacketResult, DecoderReceiveFrameMetadata,
        DecoderReceiveFrameOutput, DecoderReceiveNativeFrameMetadata,
        DecoderReceiveNativeFrameOutput, DecoderSessionConfig, DecoderSessionInfo,
        DownloadMetadata, OutputFormat, PipelineEvent, ProcessorCapabilities, ProcessorError,
        ProcessorOutput, ProcessorProgress, VESPER_DECODER_PLUGIN_ABI_VERSION_V2,
        VESPER_PLUGIN_ABI_VERSION, VesperDecoderOpenSessionResult, VesperDecoderPluginApi,
        VesperDecoderPluginApiV2, VesperDecoderReceiveFrameResult,
        VesperDecoderReceiveNativeFrameResult, VesperPipelineEventHookApi, VesperPluginBytes,
        VesperPluginDescriptor, VesperPluginKind, VesperPluginProcessResult,
        VesperPluginResultStatus, VesperPostDownloadProcessorApi,
    };
    use std::env;
    use std::ffi::{c_char, c_void};
    use std::path::{Path, PathBuf};
    use std::sync::{LazyLock, Mutex};

    static PROCESSOR_NAME: &[u8] = b"fixture-processor\0";
    static HOOK_NAME: &[u8] = b"fixture-hook\0";
    static DECODER_NAME: &[u8] = b"fixture-decoder\0";
    static EVENTS: LazyLock<Mutex<Vec<PipelineEvent>>> = LazyLock::new(|| Mutex::new(Vec::new()));

    #[derive(Default)]
    struct RecordingProgress {
        ratios: Mutex<Vec<f32>>,
    }

    impl RecordingProgress {
        fn ratios(&self) -> Vec<f32> {
            self.ratios
                .lock()
                .map(|ratios| ratios.clone())
                .unwrap_or_default()
        }
    }

    impl ProcessorProgress for RecordingProgress {
        fn on_progress(&self, ratio: f32) {
            if let Ok(mut ratios) = self.ratios.lock() {
                ratios.push(ratio);
            }
        }
    }

    #[test]
    fn dynamic_post_download_processor_adapter_round_trips_json() {
        let api = fixture_processor_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPostDownloadProcessorApi).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
        let processor = plugin
            .post_download_processor()
            .expect("processor should be available");
        let progress = RecordingProgress::default();
        let output = processor
            .process(
                &CompletedDownloadInfo {
                    asset_id: "asset-a".to_owned(),
                    task_id: Some("1".to_owned()),
                    content_format: CompletedContentFormat::SingleFile {
                        path: PathBuf::from("/tmp/input.mp4"),
                    },
                    metadata: DownloadMetadata::default(),
                },
                PathBuf::from("/tmp/output.mp4").as_path(),
                &progress,
            )
            .expect("process should succeed");

        assert_eq!(
            processor.capabilities(),
            ProcessorCapabilities {
                supported_input_formats: vec![ContentFormatKind::SingleFile],
                output_formats: vec![OutputFormat::Mp4],
                supports_cancellation: true,
            }
        );
        assert_eq!(
            output,
            ProcessorOutput::MuxedFile {
                path: PathBuf::from("/tmp/output.mp4"),
                format: OutputFormat::Mp4,
            }
        );
        assert_eq!(progress.ratios(), vec![0.5, 1.0]);
    }

    #[test]
    fn dynamic_pipeline_event_hook_adapter_round_trips_json() {
        if let Ok(mut events) = EVENTS.lock() {
            events.clear();
        }

        let api = fixture_hook_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::PipelineEventHook,
            plugin_name: HOOK_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPipelineEventHookApi).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load hook");
        let hook = plugin
            .pipeline_event_hook()
            .expect("event hook should be available");

        hook.on_event(&PipelineEvent::DownloadTaskCompleted {
            task_id: "42".to_owned(),
        });

        let events = EVENTS
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default();
        assert_eq!(
            events,
            vec![PipelineEvent::DownloadTaskCompleted {
                task_id: "42".to_owned(),
            }]
        );
    }

    #[test]
    fn dynamic_decoder_plugin_adapter_round_trips_cpu_frame() {
        let api = fixture_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApi).cast(),
        };

        let plugin =
            LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load decoder plugin");
        assert!(plugin.post_download_processor().is_none());
        assert!(plugin.pipeline_event_hook().is_none());

        let factory = plugin
            .decoder_plugin_factory()
            .expect("decoder factory should be available");
        assert_eq!(factory.name(), "fixture-decoder");
        assert!(
            factory
                .capabilities()
                .supports_codec("fixture-video", DecoderMediaKind::Video)
        );

        let mut session = factory
            .open_session(&DecoderSessionConfig {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
                require_cpu_output: true,
                ..DecoderSessionConfig::default()
            })
            .expect("open decoder session");
        assert_eq!(
            session.session_info().decoder_name.as_deref(),
            Some("fixture-decoder")
        );

        let send = session
            .send_packet(
                &DecoderPacket {
                    pts_us: Some(1_000),
                    key_frame: true,
                    ..DecoderPacket::default()
                },
                &[1, 2, 3, 4],
            )
            .expect("send packet");
        assert!(send.accepted);

        let frame = session.receive_frame().expect("receive frame");
        match frame {
            DecoderReceiveFrameOutput::Frame(frame) => {
                assert_eq!(frame.metadata.pts_us, Some(1_000));
                assert_eq!(frame.metadata.width, Some(2));
                assert_eq!(frame.metadata.height, Some(2));
                assert_eq!(frame.data, vec![1, 2, 3, 4]);
            }
            other => panic!("expected decoded frame, got {other:?}"),
        }

        assert_eq!(
            session.receive_frame().expect("need more input"),
            DecoderReceiveFrameOutput::NeedMoreInput
        );
        session.flush().expect("flush decoder");
        assert_eq!(
            session
                .receive_frame()
                .expect("need more input after flush"),
            DecoderReceiveFrameOutput::NeedMoreInput
        );
        session.close().expect("close decoder");
    }

    #[test]
    fn dynamic_decoder_plugin_surfaces_error_payloads() {
        let api = fixture_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApi).cast(),
        };
        let plugin =
            LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load decoder plugin");
        let factory = plugin
            .decoder_plugin_factory()
            .expect("decoder factory should be available");

        let error = match factory.open_session(&DecoderSessionConfig {
            codec: "missing-codec".to_owned(),
            media_kind: DecoderMediaKind::Video,
            ..DecoderSessionConfig::default()
        }) {
            Ok(_) => panic!("unsupported codec should fail"),
            Err(error) => error,
        };

        assert!(matches!(error, DecoderError::UnsupportedCodec { .. }));
    }

    #[test]
    fn dynamic_native_decoder_plugin_adapter_round_trips_native_frame() {
        let api = fixture_native_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect("load native decoder plugin");
        assert!(plugin.decoder_plugin_factory().is_none());
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("native decoder factory should be available");
        assert_eq!(factory.name(), "fixture-decoder");
        assert!(factory.capabilities().supports_hardware_decode);
        assert!(factory.capabilities().supports_gpu_handles);

        let mut session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
                prefer_hardware: true,
                require_cpu_output: false,
                ..DecoderSessionConfig::default()
            })
            .expect("open native decoder session");
        assert_eq!(
            session.session_info().selected_hardware_backend.as_deref(),
            Some("fixture-native")
        );

        let send = session
            .send_packet(
                &DecoderPacket {
                    pts_us: Some(2_000),
                    key_frame: true,
                    ..DecoderPacket::default()
                },
                &[9, 8, 7, 6],
            )
            .expect("send native packet");
        assert!(send.accepted);

        let frame = session
            .receive_native_frame()
            .expect("receive native frame");
        let frame = match frame {
            DecoderReceiveNativeFrameOutput::Frame(frame) => frame,
            other => panic!("expected native frame, got {other:?}"),
        };
        assert_ne!(frame.handle, 0);
        assert_eq!(frame.metadata.pts_us, Some(2_000));
        assert_eq!(
            frame.metadata.handle_kind,
            DecoderNativeHandleKind::IoSurface
        );
        session
            .release_native_frame(frame)
            .expect("release native frame");
        assert_eq!(
            session.receive_native_frame().expect("need more input"),
            DecoderReceiveNativeFrameOutput::NeedMoreInput
        );
        session.close().expect("close native session");
    }

    #[test]
    fn dynamic_native_decoder_plugin_receives_native_device_context() {
        let api = fixture_native_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect("load native decoder plugin");
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("native decoder factory should be available");

        let session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
                prefer_hardware: true,
                require_cpu_output: false,
                native_device_context: Some(DecoderNativeDeviceContext {
                    kind: DecoderNativeDeviceContextKind::D3D11Device,
                    handle: 42,
                }),
                ..DecoderSessionConfig::default()
            })
            .expect("open native decoder session");

        assert_eq!(
            session.session_info().selected_hardware_backend.as_deref(),
            Some("fixture-native-d3d11-device-42")
        );
    }

    #[test]
    fn dynamic_native_decoder_plugin_rejects_null_native_frame_handles() {
        let api = VesperDecoderPluginApiV2 {
            receive_native_frame: Some(fixture_decoder_receive_null_native_frame),
            ..fixture_native_decoder_api()
        };
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };
        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect("load native decoder plugin");
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("native decoder factory should be available");
        let mut session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
                ..DecoderSessionConfig::default()
            })
            .expect("open native decoder session");
        session
            .send_packet(&DecoderPacket::default(), &[1])
            .expect("send packet");

        let error = session
            .receive_native_frame()
            .expect_err("null native frame handle should fail");
        assert!(matches!(error, DecoderError::AbiViolation { .. }));
    }

    #[test]
    fn plugin_registry_reports_missing_decoder_path() {
        let registry = PluginRegistry::inspect_decoder_support(
            [PathBuf::from("/tmp/missing-vesper-decoder-plugin")],
            DecoderPluginMatchRequest::video("fixture-video"),
        );

        let records = registry.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, PluginDiagnosticStatus::LoadFailed);
        assert!(
            records[0]
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("failed to open plugin library")
        );
    }

    #[test]
    fn plugin_registry_reports_non_decoder_plugin() {
        let api = fixture_processor_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPostDownloadProcessorApi).cast(),
        };
        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
        let record = PluginDiagnosticRecord::from_loaded_plugin(
            PathBuf::from("fixture-processor"),
            &plugin,
            Some(&DecoderPluginMatchRequest::video("fixture-video")),
        );

        assert_eq!(record.status, PluginDiagnosticStatus::UnsupportedKind);
        assert_eq!(record.plugin_name.as_deref(), Some("fixture-processor"));
        assert!(
            record
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("not a decoder plugin")
        );
    }

    #[test]
    fn plugin_registry_reports_decoder_codec_match() {
        let api = fixture_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApi).cast(),
        };
        let plugin =
            LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load decoder plugin");
        let record = PluginDiagnosticRecord::from_loaded_plugin(
            PathBuf::from("fixture-decoder"),
            &plugin,
            Some(&DecoderPluginMatchRequest::video("fixture-video")),
        );

        assert_eq!(record.status, PluginDiagnosticStatus::DecoderSupported);
        assert_eq!(record.plugin_name.as_deref(), Some("fixture-decoder"));
        assert!(
            record
                .decoder_capabilities
                .as_ref()
                .expect("capabilities")
                .codecs
                .iter()
                .any(|codec| codec == "Video:fixture-video")
        );
        assert!(
            record
                .decoder_capabilities
                .as_ref()
                .expect("capabilities")
                .typed_codecs
                .contains(&DecoderPluginCodecSummary {
                    codec: "fixture-video".to_owned(),
                    media_kind: DecoderMediaKind::Video,
                })
        );
    }

    #[test]
    fn plugin_registry_reports_decoder_codec_mismatch() {
        let api = fixture_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApi).cast(),
        };
        let plugin =
            LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load decoder plugin");
        let record = PluginDiagnosticRecord::from_loaded_plugin(
            PathBuf::from("fixture-decoder"),
            &plugin,
            Some(&DecoderPluginMatchRequest::video("unknown-video")),
        );

        assert_eq!(record.status, PluginDiagnosticStatus::DecoderUnsupported);
        assert!(
            record
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("does not advertise")
        );
    }

    #[test]
    fn plugin_registry_report_counts_and_best_decoder_are_stable() {
        let api = fixture_decoder_api();
        let decoder_descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApi).cast(),
        };
        let decoder =
            LoadedDynamicPlugin::from_descriptor(None, &decoder_descriptor).expect("load decoder");
        let processor_api = fixture_processor_api();
        let processor_descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&processor_api as *const VesperPostDownloadProcessorApi).cast(),
        };
        let processor = LoadedDynamicPlugin::from_descriptor(None, &processor_descriptor)
            .expect("load processor");

        let request = DecoderPluginMatchRequest::video("fixture-video");
        let registry = PluginRegistry::from_records(vec![
            PluginDiagnosticRecord::from_loaded_plugin(
                PathBuf::from("fixture-decoder-supported"),
                &decoder,
                Some(&request),
            ),
            PluginDiagnosticRecord::from_loaded_plugin(
                PathBuf::from("fixture-decoder-unsupported"),
                &decoder,
                Some(&DecoderPluginMatchRequest::video("missing-video")),
            ),
            PluginDiagnosticRecord::from_loaded_plugin(
                PathBuf::from("fixture-processor"),
                &processor,
                Some(&request),
            ),
            PluginDiagnosticRecord::load_failed(
                PathBuf::from("missing-plugin"),
                PluginLoadError::NullDescriptor,
            ),
        ]);
        let report = registry.report();

        assert!(registry.supports_decoder(&request));
        assert_eq!(
            registry
                .best_decoder_for(&request)
                .and_then(|record| record.plugin_name.as_deref()),
            Some("fixture-decoder")
        );
        assert_eq!(report.total, 4);
        assert_eq!(report.loaded, 3);
        assert_eq!(report.failed, 1);
        assert_eq!(report.decoder_supported, 1);
        assert_eq!(report.decoder_unsupported, 1);
        assert_eq!(report.unsupported_kind, 1);
        assert_eq!(
            report.best_supported_decoder_name.as_deref(),
            Some("fixture-decoder")
        );
        assert_eq!(report.diagnostic_notes.len(), 3);
        assert!(
            report.diagnostic_notes.iter().any(
                |note| note == "fixture-decoder does not advertise Video missing-video support"
            )
        );
    }

    #[test]
    fn plugin_registry_prefers_native_decoder_candidates_when_requested() {
        let cpu_api = fixture_decoder_api();
        let cpu_descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&cpu_api as *const VesperDecoderPluginApi).cast(),
        };
        let cpu_decoder =
            LoadedDynamicPlugin::from_descriptor(None, &cpu_descriptor).expect("load decoder");
        let native_api = fixture_native_decoder_api();
        let native_descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&native_api as *const VesperDecoderPluginApiV2).cast(),
        };
        let native_decoder = LoadedDynamicPlugin::from_descriptor(None, &native_descriptor)
            .expect("load native decoder");
        let request = DecoderPluginMatchRequest::video("fixture-video");
        let registry = PluginRegistry::from_records(vec![
            PluginDiagnosticRecord::from_loaded_plugin(
                PathBuf::from("fixture-cpu-decoder"),
                &cpu_decoder,
                Some(&request),
            ),
            PluginDiagnosticRecord::from_loaded_plugin(
                PathBuf::from("fixture-native-decoder"),
                &native_decoder,
                Some(&request),
            ),
        ]);

        assert!(registry.supports_decoder(&request));
        assert!(registry.supports_native_decoder(&request));
        let native_record = registry
            .best_native_decoder_for(&request)
            .expect("native decoder should be selected");
        assert_eq!(native_record.path, PathBuf::from("fixture-native-decoder"));
        assert!(
            native_record
                .decoder_capabilities
                .as_ref()
                .is_some_and(|capabilities| capabilities.supports_native_frame_output)
        );
    }

    #[test]
    fn dynamic_post_download_processor_reports_payload_codec_errors() {
        let api = VesperPostDownloadProcessorApi {
            process_json: Some(fixture_payload_codec_process_json),
            ..fixture_processor_api()
        };
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPostDownloadProcessorApi).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
        let processor = plugin
            .post_download_processor()
            .expect("processor should be available");
        let error = processor
            .process(
                &CompletedDownloadInfo {
                    asset_id: "asset-a".to_owned(),
                    task_id: Some("1".to_owned()),
                    content_format: CompletedContentFormat::SingleFile {
                        path: PathBuf::from("/tmp/input.mp4"),
                    },
                    metadata: DownloadMetadata::default(),
                },
                Path::new("/tmp/output.mp4"),
                &RecordingProgress::default(),
            )
            .expect_err("invalid payload should fail");

        assert!(matches!(error, ProcessorError::PayloadCodec(_)));
        assert!(error.to_string().contains("success payload"));
    }

    #[test]
    fn dynamic_post_download_processor_reports_abi_violations() {
        let api = VesperPostDownloadProcessorApi {
            process_json: Some(fixture_null_payload_process_json),
            ..fixture_processor_api()
        };
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPostDownloadProcessorApi).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
        let processor = plugin
            .post_download_processor()
            .expect("processor should be available");
        let error = processor
            .process(
                &CompletedDownloadInfo {
                    asset_id: "asset-a".to_owned(),
                    task_id: Some("1".to_owned()),
                    content_format: CompletedContentFormat::SingleFile {
                        path: PathBuf::from("/tmp/input.mp4"),
                    },
                    metadata: DownloadMetadata::default(),
                },
                Path::new("/tmp/output.mp4"),
                &RecordingProgress::default(),
            )
            .expect_err("null payload pointer should fail");

        assert!(matches!(error, ProcessorError::AbiViolation(_)));
        assert!(error.to_string().contains("null data pointer"));
    }

    #[test]
    #[ignore = "requires a built player-ffmpeg shared library artifact"]
    fn dynamic_loader_opens_real_player_ffmpeg_shared_library() {
        let plugin_path = resolve_player_ffmpeg_plugin_path()
            .unwrap_or_else(|error| panic!("failed to resolve player-ffmpeg plugin path: {error}"));

        let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
            panic!(
                "failed to load player-ffmpeg shared library `{}`: {error}",
                plugin_path.display()
            )
        });

        assert_eq!(plugin.plugin_name(), "player-ffmpeg");
        assert!(plugin.pipeline_event_hook().is_none());

        let processor = plugin
            .post_download_processor()
            .expect("player-ffmpeg should export a post-download processor");
        assert_eq!(processor.name(), "player-ffmpeg");
        assert_eq!(
            processor.capabilities(),
            ProcessorCapabilities {
                supported_input_formats: vec![
                    ContentFormatKind::HlsSegments,
                    ContentFormatKind::DashSegments,
                ],
                output_formats: vec![OutputFormat::Mp4],
                supports_cancellation: true,
            }
        );

        let progress = RecordingProgress::default();
        let output = processor
            .process(
                &CompletedDownloadInfo {
                    asset_id: "asset-a".to_owned(),
                    task_id: Some("1".to_owned()),
                    content_format: CompletedContentFormat::SingleFile {
                        path: PathBuf::from("/tmp/input.mp4"),
                    },
                    metadata: DownloadMetadata::default(),
                },
                Path::new("/tmp/output.mp4"),
                &progress,
            )
            .expect("single-file input should be skipped by player-ffmpeg");

        assert_eq!(output, ProcessorOutput::Skipped);
        assert!(progress.ratios().is_empty());
    }

    #[test]
    #[ignore = "requires a built player-decoder-fixture shared library artifact"]
    fn dynamic_loader_opens_real_decoder_fixture_shared_library() {
        let plugin_path = resolve_decoder_fixture_plugin_path()
            .unwrap_or_else(|error| panic!("failed to resolve fixture decoder path: {error}"));

        let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
            panic!(
                "failed to load decoder fixture shared library `{}`: {error}",
                plugin_path.display()
            )
        });

        assert_eq!(plugin.plugin_name(), "player-decoder-fixture");
        assert!(plugin.post_download_processor().is_none());
        assert!(plugin.pipeline_event_hook().is_none());
        assert!(plugin.decoder_plugin_factory().is_some());
    }

    #[test]
    #[ignore = "requires a built player-decoder-fixture shared library artifact"]
    fn dynamic_loader_opens_real_decoder_fixture_shared_library_as_native_v2() {
        let plugin_path = resolve_decoder_fixture_plugin_path()
            .unwrap_or_else(|error| panic!("failed to resolve fixture decoder path: {error}"));
        // SAFETY: this ignored integration test runs a single plugin load with
        // an isolated process-wide fixture switch.
        unsafe { env::set_var("VESPER_DECODER_FIXTURE_ABI", "v2") };

        let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
            panic!(
                "failed to load decoder fixture shared library `{}` as v2: {error}",
                plugin_path.display()
            )
        });
        unsafe { env::remove_var("VESPER_DECODER_FIXTURE_ABI") };

        assert_eq!(plugin.plugin_name(), "player-decoder-fixture");
        assert!(plugin.post_download_processor().is_none());
        assert!(plugin.pipeline_event_hook().is_none());
        assert!(plugin.decoder_plugin_factory().is_none());
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("player-decoder-fixture should export a native decoder factory in v2 mode");
        assert!(factory.capabilities().supports_hardware_decode);
        assert!(factory.capabilities().supports_gpu_handles);
    }

    #[test]
    #[ignore = "requires a built player-decoder-videotoolbox shared library artifact"]
    fn dynamic_loader_opens_real_videotoolbox_decoder_shared_library() {
        let plugin_path = resolve_decoder_videotoolbox_plugin_path().unwrap_or_else(|error| {
            panic!("failed to resolve VideoToolbox decoder plugin path: {error}")
        });

        let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
            panic!(
                "failed to load VideoToolbox decoder shared library `{}`: {error}",
                plugin_path.display()
            )
        });

        assert_eq!(plugin.plugin_name(), "player-decoder-videotoolbox");
        assert!(plugin.decoder_plugin_factory().is_none());
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("player-decoder-videotoolbox should export a native decoder factory");
        let capabilities = factory.capabilities();
        assert!(capabilities.supports_codec("H264", DecoderMediaKind::Video));
        assert!(capabilities.supports_codec("HEVC", DecoderMediaKind::Video));
        assert!(capabilities.supports_hardware_decode);
        assert!(capabilities.supports_gpu_handles);

        let session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: "H264".to_owned(),
                media_kind: DecoderMediaKind::Video,
                width: Some(1920),
                height: Some(1080),
                prefer_hardware: true,
                ..DecoderSessionConfig::default()
            })
            .expect("VideoToolbox plugin should open a lazy native session");
        assert_eq!(
            session.session_info().selected_hardware_backend.as_deref(),
            Some("VideoToolbox")
        );
    }

    fn fixture_processor_api() -> VesperPostDownloadProcessorApi {
        VesperPostDownloadProcessorApi {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(fixture_processor_name),
            capabilities_json: Some(fixture_processor_capabilities_json),
            free_bytes: Some(fixture_free_bytes),
            process_json: Some(fixture_processor_process_json),
        }
    }

    fn fixture_hook_api() -> VesperPipelineEventHookApi {
        VesperPipelineEventHookApi {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(fixture_hook_name),
            on_event_json: Some(fixture_hook_on_event_json),
        }
    }

    fn fixture_decoder_api() -> VesperDecoderPluginApi {
        VesperDecoderPluginApi {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(fixture_decoder_name),
            capabilities_json: Some(fixture_decoder_capabilities_json),
            free_bytes: Some(fixture_free_bytes),
            open_session_json: Some(fixture_decoder_open_session_json),
            send_packet: Some(fixture_decoder_send_packet),
            receive_frame: Some(fixture_decoder_receive_frame),
            flush_session: Some(fixture_decoder_flush_session),
            close_session: Some(fixture_decoder_close_session),
        }
    }

    fn fixture_native_decoder_api() -> VesperDecoderPluginApiV2 {
        VesperDecoderPluginApiV2 {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(fixture_decoder_name),
            capabilities_json: Some(fixture_native_decoder_capabilities_json),
            free_bytes: Some(fixture_free_bytes),
            open_session_json: Some(fixture_native_decoder_open_session_json),
            send_packet: Some(fixture_decoder_send_packet),
            receive_native_frame: Some(fixture_decoder_receive_native_frame),
            release_native_frame: Some(fixture_decoder_release_native_frame),
            flush_session: Some(fixture_decoder_flush_session),
            close_session: Some(fixture_decoder_close_session),
        }
    }

    unsafe extern "C" fn fixture_processor_name(_context: *mut c_void) -> *const c_char {
        PROCESSOR_NAME.as_ptr().cast::<c_char>()
    }

    unsafe extern "C" fn fixture_hook_name(_context: *mut c_void) -> *const c_char {
        HOOK_NAME.as_ptr().cast::<c_char>()
    }

    unsafe extern "C" fn fixture_decoder_name(_context: *mut c_void) -> *const c_char {
        DECODER_NAME.as_ptr().cast::<c_char>()
    }

    unsafe extern "C" fn fixture_processor_capabilities_json(
        _context: *mut c_void,
    ) -> VesperPluginBytes {
        let capabilities = ProcessorCapabilities {
            supported_input_formats: vec![ContentFormatKind::SingleFile],
            output_formats: vec![OutputFormat::Mp4],
            supports_cancellation: true,
        };
        let payload = serde_json::to_vec(&capabilities).expect("serialize capabilities");
        VesperPluginBytes::from_vec(payload)
    }

    unsafe extern "C" fn fixture_processor_process_json(
        _context: *mut c_void,
        input_json: *const u8,
        input_json_len: usize,
        output_path: *const c_char,
        progress: player_plugin::VesperPluginProgressCallbacks,
    ) -> VesperPluginProcessResult {
        // SAFETY: the fixture passes a valid input buffer for the duration of
        // this synchronous callback.
        let input_json = unsafe { std::slice::from_raw_parts(input_json, input_json_len) };
        let input: CompletedDownloadInfo =
            serde_json::from_slice(input_json).expect("deserialize input");
        assert_eq!(input.asset_id, "asset-a");

        if let Some(on_progress) = progress.on_progress {
            // SAFETY: the host-side fixture keeps `progress.context` alive for
            // the duration of this synchronous call.
            unsafe { on_progress(progress.context, 0.5) };
            // SAFETY: same as above for the second progress update.
            unsafe { on_progress(progress.context, 1.0) };
        }

        // SAFETY: the fixture provides a valid NUL-terminated UTF-8 path.
        let output_path = unsafe { std::ffi::CStr::from_ptr(output_path) }
            .to_str()
            .expect("output path utf8");
        let output = ProcessorOutput::MuxedFile {
            path: PathBuf::from(output_path),
            format: OutputFormat::Mp4,
        };
        let payload = serde_json::to_vec(&output).expect("serialize output");
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes::from_vec(payload),
        }
    }

    unsafe extern "C" fn fixture_decoder_capabilities_json(
        _context: *mut c_void,
    ) -> VesperPluginBytes {
        let capabilities = DecoderCapabilities {
            codecs: vec![DecoderCodecCapability {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
                profiles: vec!["baseline".to_owned()],
                output_formats: vec![DecoderFrameFormat::Rgba8888],
            }],
            supports_hardware_decode: false,
            supports_cpu_video_frames: true,
            supports_audio_frames: false,
            supports_gpu_handles: false,
            supports_flush: true,
            supports_drain: true,
            max_sessions: Some(1),
        };
        VesperPluginBytes::from_vec(serde_json::to_vec(&capabilities).expect("serialize caps"))
    }

    unsafe extern "C" fn fixture_native_decoder_capabilities_json(
        _context: *mut c_void,
    ) -> VesperPluginBytes {
        let capabilities = DecoderCapabilities {
            codecs: vec![DecoderCodecCapability {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
                profiles: vec!["baseline".to_owned()],
                output_formats: vec![DecoderFrameFormat::Nv12],
            }],
            supports_hardware_decode: true,
            supports_cpu_video_frames: false,
            supports_audio_frames: false,
            supports_gpu_handles: true,
            supports_flush: true,
            supports_drain: true,
            max_sessions: Some(1),
        };
        VesperPluginBytes::from_vec(serde_json::to_vec(&capabilities).expect("serialize caps"))
    }

    #[derive(Debug, Default)]
    struct FixtureDecoderSession {
        last_pts_us: Option<i64>,
        pending_frame: Option<Vec<u8>>,
    }

    unsafe extern "C" fn fixture_decoder_open_session_json(
        _context: *mut c_void,
        config_json: *const u8,
        config_json_len: usize,
    ) -> VesperDecoderOpenSessionResult {
        let config = decode_fixture_json::<DecoderSessionConfig>(config_json, config_json_len);
        let config = match config {
            Ok(config) => config,
            Err(error) => {
                return decoder_open_error(error);
            }
        };
        if config.codec != "fixture-video" || config.media_kind != DecoderMediaKind::Video {
            return decoder_open_error(DecoderError::UnsupportedCodec {
                codec: config.codec,
            });
        }

        let session = Box::into_raw(Box::new(FixtureDecoderSession::default()));
        let info = DecoderSessionInfo {
            decoder_name: Some("fixture-decoder".to_owned()),
            selected_hardware_backend: None,
            output_format: Some(DecoderFrameFormat::Rgba8888),
        };
        VesperDecoderOpenSessionResult {
            status: VesperPluginResultStatus::Success,
            session: session.cast::<c_void>(),
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&info).expect("serialize info"),
            ),
        }
    }

    unsafe extern "C" fn fixture_native_decoder_open_session_json(
        _context: *mut c_void,
        config_json: *const u8,
        config_json_len: usize,
    ) -> VesperDecoderOpenSessionResult {
        let config = decode_fixture_json::<DecoderSessionConfig>(config_json, config_json_len);
        let config = match config {
            Ok(config) => config,
            Err(error) => return decoder_open_error(error),
        };
        if config.codec != "fixture-video" || config.media_kind != DecoderMediaKind::Video {
            return decoder_open_error(DecoderError::UnsupportedCodec {
                codec: config.codec,
            });
        }

        let session = Box::into_raw(Box::new(FixtureDecoderSession::default()));
        let selected_hardware_backend = match config.native_device_context.as_ref() {
            Some(context) if context.kind == DecoderNativeDeviceContextKind::D3D11Device => {
                Some(format!("fixture-native-d3d11-device-{}", context.handle))
            }
            _ => Some("fixture-native".to_owned()),
        };
        let info = DecoderSessionInfo {
            decoder_name: Some("fixture-decoder".to_owned()),
            selected_hardware_backend,
            output_format: Some(DecoderFrameFormat::Nv12),
        };
        VesperDecoderOpenSessionResult {
            status: VesperPluginResultStatus::Success,
            session: session.cast::<c_void>(),
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&info).expect("serialize info"),
            ),
        }
    }

    unsafe extern "C" fn fixture_decoder_send_packet(
        _context: *mut c_void,
        session: *mut c_void,
        packet_json: *const u8,
        packet_json_len: usize,
        packet_data: *const u8,
        packet_data_len: usize,
    ) -> VesperPluginProcessResult {
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
            return decoder_process_error(DecoderError::NotConfigured);
        };
        let packet = match decode_fixture_json::<DecoderPacket>(packet_json, packet_json_len) {
            Ok(packet) => packet,
            Err(error) => return decoder_process_error(error),
        };
        let data = if packet_data.is_null() || packet_data_len == 0 {
            Vec::new()
        } else {
            // SAFETY: the host-side fixture passes a valid packet buffer for the
            // duration of this synchronous callback.
            unsafe { std::slice::from_raw_parts(packet_data, packet_data_len) }.to_vec()
        };
        session.last_pts_us = packet.pts_us;
        session.pending_frame = Some(data);
        let result = DecoderPacketResult { accepted: true };
        decoder_process_success(&result)
    }

    unsafe extern "C" fn fixture_decoder_receive_frame(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperDecoderReceiveFrameResult {
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
            return decoder_frame_error(DecoderError::NotConfigured);
        };
        let Some(data) = session.pending_frame.take() else {
            return decoder_frame_success(
                &DecoderReceiveFrameMetadata::need_more_input(),
                Vec::new(),
            );
        };
        let metadata = DecoderFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Rgba8888,
            pts_us: session.last_pts_us,
            duration_us: Some(33_333),
            width: Some(2),
            height: Some(2),
            sample_rate: None,
            channels: None,
            planes: vec![DecoderFramePlane {
                offset: 0,
                len: data.len(),
                stride: Some(8),
            }],
        };
        decoder_frame_success(&DecoderReceiveFrameMetadata::frame(metadata), data)
    }

    unsafe extern "C" fn fixture_decoder_receive_native_frame(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperDecoderReceiveNativeFrameResult {
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
            return decoder_native_frame_error(DecoderError::NotConfigured);
        };
        let Some(data) = session.pending_frame.take() else {
            return decoder_native_frame_success(
                &DecoderReceiveNativeFrameMetadata::need_more_input(),
                0,
            );
        };
        let handle = Box::into_raw(Box::new(data)) as usize;
        let metadata = DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Nv12,
            codec: "fixture-video".to_owned(),
            pts_us: session.last_pts_us,
            duration_us: Some(33_333),
            width: 2,
            height: 2,
            handle_kind: DecoderNativeHandleKind::IoSurface,
        };
        decoder_native_frame_success(&DecoderReceiveNativeFrameMetadata::frame(metadata), handle)
    }

    unsafe extern "C" fn fixture_decoder_receive_null_native_frame(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperDecoderReceiveNativeFrameResult {
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
            return decoder_native_frame_error(DecoderError::NotConfigured);
        };
        if session.pending_frame.take().is_none() {
            return decoder_native_frame_success(
                &DecoderReceiveNativeFrameMetadata::need_more_input(),
                0,
            );
        };
        let metadata = DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Nv12,
            codec: "fixture-video".to_owned(),
            pts_us: session.last_pts_us,
            duration_us: Some(33_333),
            width: 2,
            height: 2,
            handle_kind: DecoderNativeHandleKind::IoSurface,
        };
        decoder_native_frame_success(&DecoderReceiveNativeFrameMetadata::frame(metadata), 0)
    }

    unsafe extern "C" fn fixture_decoder_release_native_frame(
        _context: *mut c_void,
        _session: *mut c_void,
        handle_kind: u32,
        handle: usize,
    ) -> VesperPluginProcessResult {
        if handle_kind != 2 || handle == 0 {
            return decoder_process_error(DecoderError::abi_violation(
                "fixture native frame release received an invalid handle",
            ));
        }
        let _ = unsafe { Box::from_raw(handle as *mut Vec<u8>) };
        decoder_process_success(&DecoderOperationStatus { completed: true })
    }

    unsafe extern "C" fn fixture_decoder_flush_session(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperPluginProcessResult {
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
            return decoder_process_error(DecoderError::NotConfigured);
        };
        session.pending_frame = None;
        decoder_process_success(&DecoderOperationStatus { completed: true })
    }

    unsafe extern "C" fn fixture_decoder_close_session(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperPluginProcessResult {
        if session.is_null() {
            return decoder_process_error(DecoderError::NotConfigured);
        }
        // SAFETY: the session pointer was allocated with `Box::into_raw` by
        // `fixture_decoder_open_session_json` and close is called once.
        let _ = unsafe { Box::from_raw(session.cast::<FixtureDecoderSession>()) };
        decoder_process_success(&DecoderOperationStatus { completed: true })
    }

    unsafe extern "C" fn fixture_payload_codec_process_json(
        _context: *mut c_void,
        _input_json: *const u8,
        _input_json_len: usize,
        _output_path: *const c_char,
        _progress: player_plugin::VesperPluginProgressCallbacks,
    ) -> VesperPluginProcessResult {
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes::from_vec(b"not-json".to_vec()),
        }
    }

    unsafe extern "C" fn fixture_null_payload_process_json(
        _context: *mut c_void,
        _input_json: *const u8,
        _input_json_len: usize,
        _output_path: *const c_char,
        _progress: player_plugin::VesperPluginProgressCallbacks,
    ) -> VesperPluginProcessResult {
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes {
                data: std::ptr::null_mut(),
                len: 4,
            },
        }
    }

    unsafe extern "C" fn fixture_hook_on_event_json(
        _context: *mut c_void,
        event_json: *const u8,
        event_json_len: usize,
    ) -> bool {
        // SAFETY: the fixture passes a valid event buffer for the duration of
        // this synchronous callback.
        let event_json = unsafe { std::slice::from_raw_parts(event_json, event_json_len) };
        let event: PipelineEvent = serde_json::from_slice(event_json).expect("deserialize event");
        if let Ok(mut events) = EVENTS.lock() {
            events.push(event);
        }
        true
    }

    fn decode_fixture_json<T: serde::de::DeserializeOwned>(
        data: *const u8,
        len: usize,
    ) -> Result<T, DecoderError> {
        if data.is_null() && len > 0 {
            return Err(DecoderError::abi_violation(
                "fixture JSON pointer was null with non-zero len",
            ));
        }
        let payload = if data.is_null() || len == 0 {
            &[]
        } else {
            // SAFETY: fixture callers pass a valid JSON buffer for the duration
            // of this synchronous callback.
            unsafe { std::slice::from_raw_parts(data, len) }
        };
        serde_json::from_slice(payload)
            .map_err(|error| DecoderError::payload_codec(error.to_string()))
    }

    fn decoder_open_error(error: DecoderError) -> VesperDecoderOpenSessionResult {
        VesperDecoderOpenSessionResult {
            status: VesperPluginResultStatus::Failure,
            session: std::ptr::null_mut(),
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize error"),
            ),
        }
    }

    fn decoder_process_success<T: serde::Serialize>(value: &T) -> VesperPluginProcessResult {
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(value).expect("serialize value"),
            ),
        }
    }

    fn decoder_process_error(error: DecoderError) -> VesperPluginProcessResult {
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Failure,
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize error"),
            ),
        }
    }

    fn decoder_frame_success(
        metadata: &DecoderReceiveFrameMetadata,
        data: Vec<u8>,
    ) -> VesperDecoderReceiveFrameResult {
        VesperDecoderReceiveFrameResult {
            status: VesperPluginResultStatus::Success,
            metadata: VesperPluginBytes::from_vec(
                serde_json::to_vec(metadata).expect("serialize frame metadata"),
            ),
            data: VesperPluginBytes::from_vec(data),
        }
    }

    fn decoder_frame_error(error: DecoderError) -> VesperDecoderReceiveFrameResult {
        VesperDecoderReceiveFrameResult {
            status: VesperPluginResultStatus::Failure,
            metadata: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize error"),
            ),
            data: VesperPluginBytes::null(),
        }
    }

    fn decoder_native_frame_success(
        metadata: &DecoderReceiveNativeFrameMetadata,
        handle: usize,
    ) -> VesperDecoderReceiveNativeFrameResult {
        VesperDecoderReceiveNativeFrameResult {
            status: VesperPluginResultStatus::Success,
            metadata: VesperPluginBytes::from_vec(
                serde_json::to_vec(metadata).expect("serialize native frame metadata"),
            ),
            handle,
        }
    }

    fn decoder_native_frame_error(error: DecoderError) -> VesperDecoderReceiveNativeFrameResult {
        VesperDecoderReceiveNativeFrameResult {
            status: VesperPluginResultStatus::Failure,
            metadata: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize error"),
            ),
            handle: 0,
        }
    }

    unsafe extern "C" fn fixture_free_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
        // SAFETY: the fixture only reclaims buffers it allocated with
        // `VesperPluginBytes::from_vec`.
        let _ = unsafe { payload.into_vec() };
    }

    fn resolve_player_ffmpeg_plugin_path() -> Result<PathBuf, String> {
        if let Some(path) = env::var_os("VESPER_PLAYER_FFMPEG_PLUGIN_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!(
                "environment variable VESPER_PLAYER_FFMPEG_PLUGIN_PATH points to missing file `{}`",
                path.display()
            ));
        }

        resolve_plugin_path("player_ffmpeg")
    }

    fn resolve_decoder_fixture_plugin_path() -> Result<PathBuf, String> {
        if let Some(path) = env::var_os("VESPER_DECODER_FIXTURE_PLUGIN_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!(
                "environment variable VESPER_DECODER_FIXTURE_PLUGIN_PATH points to missing file `{}`",
                path.display()
            ));
        }
        if let Some(paths) = env::var_os("VESPER_DECODER_PLUGIN_PATHS")
            && let Some(path) = env::split_paths(&paths).next()
        {
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!(
                "environment variable VESPER_DECODER_PLUGIN_PATHS points to missing file `{}`",
                path.display()
            ));
        }

        resolve_plugin_path("player_decoder_fixture")
    }

    fn resolve_decoder_videotoolbox_plugin_path() -> Result<PathBuf, String> {
        if let Some(path) = env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!(
                "environment variable VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH points to missing file `{}`",
                path.display()
            ));
        }

        resolve_plugin_path("player_decoder_videotoolbox")
    }

    fn resolve_plugin_path(stem: &str) -> Result<PathBuf, String> {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .map(Path::to_path_buf)
            .ok_or_else(|| "failed to derive workspace root from CARGO_MANIFEST_DIR".to_owned())?;
        let target_dir = env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    workspace_root.join(path)
                }
            })
            .unwrap_or_else(|| workspace_root.join("target"));
        let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_owned());
        let library_name = shared_library_name(stem);
        let candidates = [
            target_dir.join(&profile).join(&library_name),
            target_dir.join(&profile).join("deps").join(&library_name),
            target_dir.join("debug").join(&library_name),
            target_dir.join("debug").join("deps").join(&library_name),
            target_dir.join("release").join(&library_name),
            target_dir.join("release").join("deps").join(&library_name),
        ];

        candidates
            .into_iter()
            .find(|path| path.is_file())
            .ok_or_else(|| {
                format!(
                    "could not find `{library_name}` under `{}`; build the plugin crate first or set the matching plugin path environment variable",
                    target_dir.display()
                )
            })
    }

    fn shared_library_name(stem: &str) -> String {
        if cfg!(target_os = "windows") {
            format!("{stem}.dll")
        } else if cfg!(target_os = "macos") {
            format!("lib{stem}.dylib")
        } else {
            format!("lib{stem}.so")
        }
    }

    #[allow(dead_code)]
    unsafe extern "C" fn fixture_error_process_json(
        _context: *mut c_void,
        _input_json: *const u8,
        _input_json_len: usize,
        _output_path: *const c_char,
        _progress: player_plugin::VesperPluginProgressCallbacks,
    ) -> VesperPluginProcessResult {
        let payload = serde_json::to_vec(&ProcessorError::UnsupportedFormat(
            ContentFormatKind::DashSegments,
        ))
        .expect("serialize error");
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Failure,
            payload: VesperPluginBytes::from_vec(payload),
        }
    }
}
