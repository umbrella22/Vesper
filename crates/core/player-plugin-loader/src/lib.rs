use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;
use std::sync::Arc;

use libloading::Library;
use player_plugin::{
    CompletedDownloadInfo, PipelineEvent, PipelineEventHook, PostDownloadProcessor,
    ProcessorCapabilities, ProcessorError, ProcessorOutput, ProcessorProgress,
    VESPER_PLUGIN_ABI_VERSION, VESPER_PLUGIN_ENTRY_SYMBOL, VesperPipelineEventHookApi,
    VesperPluginBytes, VesperPluginDescriptor, VesperPluginEntryPoint, VesperPluginKind,
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
        let library =
            unsafe { Library::new(path) }.map_err(|source| PluginLoadError::OpenLibrary {
                path: path_string,
                source,
            })?;

        let entry = unsafe { library.get::<VesperPluginEntryPoint>(VESPER_PLUGIN_ENTRY_SYMBOL) }
            .map_err(|source| PluginLoadError::ResolveEntrySymbol {
                symbol: "vesper_plugin_entry",
                source,
            })?;

        let descriptor_ptr = unsafe { entry() };
        let descriptor =
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
                let api = unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                    field: "post_download_processor_api",
                })?;
                let processor =
                    DynamicPostDownloadProcessor::new(library, descriptor_name.clone(), *api)?;
                Ok(Self {
                    name: descriptor_name,
                    post_download_processor: Some(Arc::new(processor)),
                    pipeline_event_hook: None,
                })
            }
            VesperPluginKind::PipelineEventHook => {
                let api_ptr = descriptor.api.cast::<VesperPipelineEventHookApi>();
                let api = unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                    field: "pipeline_event_hook_api",
                })?;
                let hook = DynamicPipelineEventHook::new(library, descriptor_name.clone(), *api)?;
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

#[derive(Debug)]
struct DynamicPostDownloadProcessorInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: VesperPostDownloadProcessorApi,
    capabilities: ProcessorCapabilities,
}

impl Drop for DynamicPostDownloadProcessorInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
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
        api: VesperPostDownloadProcessorApi,
    ) -> Result<Self, PluginLoadError> {
        ensure_processor_api(&api)?;
        let name = if let Some(name_fn) = api.name {
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
            unsafe { api.capabilities_json.expect("checked")(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(PluginLoadError::DecodeCapabilities)?;

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
            ProcessorError::MuxFailed(format!(
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

        let result = unsafe {
            self.inner.api.process_json.expect("checked")(
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
            .map_err(|error| {
                ProcessorError::MuxFailed(format!(
                    "decode plugin `{}` success payload failed: {error}",
                    self.inner.name
                ))
            }),
            VesperPluginResultStatus::Failure => decode_plugin_bytes::<ProcessorError>(
                result.payload,
                self.inner.api.free_bytes,
                self.inner.api.context,
            )
            .map_err(|error| {
                ProcessorError::MuxFailed(format!(
                    "decode plugin `{}` error payload failed: {error}",
                    self.inner.name
                ))
            })
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
    api: VesperPipelineEventHookApi,
}

impl Drop for DynamicPipelineEventHookInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
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
        api: VesperPipelineEventHookApi,
    ) -> Result<Self, PluginLoadError> {
        if api.on_event_json.is_none() {
            return Err(PluginLoadError::MissingField {
                field: "pipeline_event_hook_api.on_event_json",
            });
        }

        let name = if let Some(name_fn) = api.name {
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

        let _ = unsafe {
            self.inner.api.on_event_json.expect("checked")(
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
    let adapter = unsafe { &*(context.cast::<ProgressAdapter<'_>>()) };
    adapter.progress.on_progress(ratio);
}

unsafe extern "C" fn progress_is_cancelled(context: *mut c_void) -> bool {
    let adapter = unsafe { &*(context.cast::<ProgressAdapter<'_>>()) };
    adapter.progress.is_cancelled()
}

fn ensure_processor_api(api: &VesperPostDownloadProcessorApi) -> Result<(), PluginLoadError> {
    if api.capabilities_json.is_none() {
        return Err(PluginLoadError::MissingField {
            field: "post_download_processor_api.capabilities_json",
        });
    }
    if api.free_bytes.is_none() {
        return Err(PluginLoadError::MissingField {
            field: "post_download_processor_api.free_bytes",
        });
    }
    if api.process_json.is_none() {
        return Err(PluginLoadError::MissingField {
            field: "post_download_processor_api.process_json",
        });
    }
    Ok(())
}

fn c_string_field(pointer: *const c_char, field: &'static str) -> Result<String, PluginLoadError> {
    if pointer.is_null() {
        return Err(PluginLoadError::MissingField { field });
    }

    let value = unsafe { CStr::from_ptr(pointer) };
    value
        .to_str()
        .map(|value| value.to_owned())
        .map_err(|_| PluginLoadError::InvalidUtf8 { field })
}

fn decode_plugin_bytes<T: DeserializeOwned>(
    payload: VesperPluginBytes,
    free_bytes: Option<unsafe extern "C" fn(*mut c_void, VesperPluginBytes)>,
    context: *mut c_void,
) -> Result<T, serde_json::Error> {
    let bytes = if payload.data.is_null() || payload.len == 0 {
        Vec::new()
    } else {
        let slice = unsafe { std::slice::from_raw_parts(payload.data, payload.len) };
        slice.to_vec()
    };

    if let Some(free_bytes) = free_bytes {
        unsafe { free_bytes(context, payload) };
    }

    serde_json::from_slice(&bytes)
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
        let input_json = unsafe { std::slice::from_raw_parts(input_json, input_json_len) };
        let input: CompletedDownloadInfo =
            serde_json::from_slice(input_json).expect("deserialize input");
        assert_eq!(input.asset_id, "asset-a");

        if let Some(on_progress) = progress.on_progress {
            unsafe { on_progress(progress.context, 0.5) };
            unsafe { on_progress(progress.context, 1.0) };
        }

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

    unsafe extern "C" fn fixture_hook_on_event_json(
        _context: *mut c_void,
        event_json: *const u8,
        event_json_len: usize,
    ) -> bool {
        let event_json = unsafe { std::slice::from_raw_parts(event_json, event_json_len) };
        let event: PipelineEvent = serde_json::from_slice(event_json).expect("deserialize event");
        if let Ok(mut events) = EVENTS.lock() {
            events.push(event);
        }
        true
    }

    unsafe extern "C" fn fixture_free_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
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
