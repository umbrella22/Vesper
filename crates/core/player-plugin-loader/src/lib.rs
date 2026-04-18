#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;
use std::sync::Arc;

use libloading::Library;
use player_plugin::{
    CompletedDownloadInfo, PipelineEvent, PipelineEventHook, PostDownloadProcessor,
    ProcessorCapabilities, ProcessorError, ProcessorOutput, ProcessorProgress,
    VESPER_PLUGIN_ABI_VERSION, VESPER_PLUGIN_ENTRY_SYMBOL, VesperPipelineEventHookApi,
    VesperPluginBytes, VesperPluginDescriptor, VesperPluginEntryPoint, VesperPluginKind,
    VesperPluginProcessResult, VesperPluginProgressCallbacks, VesperPluginResultStatus,
    VesperPostDownloadProcessorApi,
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
    post_download_processor: Option<Arc<DynamicPostDownloadProcessor>>,
    pipeline_event_hook: Option<Arc<DynamicPipelineEventHook>>,
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

    fn from_descriptor(
        library: Option<Arc<LibraryHolder>>,
        descriptor: &VesperPluginDescriptor,
    ) -> Result<Self, PluginLoadError> {
        if descriptor.abi_version != VESPER_PLUGIN_ABI_VERSION {
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
                    post_download_processor: Some(Arc::new(processor)),
                    pipeline_event_hook: None,
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
                    post_download_processor: None,
                    pipeline_event_hook: Some(Arc::new(hook)),
                })
            }
        }
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
    use super::LoadedDynamicPlugin;
    use player_plugin::{
        CompletedContentFormat, CompletedDownloadInfo, ContentFormatKind, DownloadMetadata,
        OutputFormat, PipelineEvent, ProcessorCapabilities, ProcessorError, ProcessorOutput,
        ProcessorProgress, VESPER_PLUGIN_ABI_VERSION, VesperPipelineEventHookApi,
        VesperPluginBytes, VesperPluginDescriptor, VesperPluginKind, VesperPluginProcessResult,
        VesperPluginResultStatus, VesperPostDownloadProcessorApi,
    };
    use std::env;
    use std::ffi::{c_char, c_void};
    use std::path::{Path, PathBuf};
    use std::sync::{LazyLock, Mutex};

    static PROCESSOR_NAME: &[u8] = b"fixture-processor\0";
    static HOOK_NAME: &[u8] = b"fixture-hook\0";
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

    unsafe extern "C" fn fixture_processor_name(_context: *mut c_void) -> *const c_char {
        PROCESSOR_NAME.as_ptr().cast::<c_char>()
    }

    unsafe extern "C" fn fixture_hook_name(_context: *mut c_void) -> *const c_char {
        HOOK_NAME.as_ptr().cast::<c_char>()
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
        let library_name = shared_library_name("player_ffmpeg");
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
                    "could not find `{library_name}` under `{}`; build it first with `cargo build -p player-ffmpeg` or set VESPER_PLAYER_FFMPEG_PLUGIN_PATH",
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
