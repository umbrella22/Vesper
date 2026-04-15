use std::ffi::{c_char, c_void};

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VesperPluginKind {
    PostDownloadProcessor = 1,
    PipelineEventHook = 2,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VesperPluginResultStatus {
    Success = 0,
    Failure = 1,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VesperPluginBytes {
    pub data: *mut u8,
    pub len: usize,
}

impl Default for VesperPluginBytes {
    fn default() -> Self {
        Self::null()
    }
}

impl VesperPluginBytes {
    pub const fn null() -> Self {
        Self {
            data: std::ptr::null_mut(),
            len: 0,
        }
    }

    pub fn from_vec(mut bytes: Vec<u8>) -> Self {
        let result = Self {
            data: bytes.as_mut_ptr(),
            len: bytes.len(),
        };
        std::mem::forget(bytes);
        result
    }

    /// # Safety
    ///
    /// The caller must ensure `self` was allocated by `Vec<u8>` in the current
    /// dynamic library and has not already been reclaimed.
    pub unsafe fn into_vec(self) -> Vec<u8> {
        if self.data.is_null() || self.len == 0 {
            return Vec::new();
        }

        // SAFETY: guaranteed by the caller contract above.
        unsafe { Vec::from_raw_parts(self.data, self.len, self.len) }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperPluginProgressCallbacks {
    pub context: *mut c_void,
    pub on_progress: Option<unsafe extern "C" fn(context: *mut c_void, ratio: f32)>,
    pub is_cancelled: Option<unsafe extern "C" fn(context: *mut c_void) -> bool>,
}

// SAFETY: the callback table is only used behind `ProcessorProgress`, and host/plugin
// implementors must guarantee that the callback context is safe to invoke across the
// declared `Send + Sync` boundary.
unsafe impl Send for VesperPluginProgressCallbacks {}
// SAFETY: same reasoning as above; shared access to the callback context is part of
// the ABI contract between host and plugin.
unsafe impl Sync for VesperPluginProgressCallbacks {}

impl Default for VesperPluginProgressCallbacks {
    fn default() -> Self {
        Self {
            context: std::ptr::null_mut(),
            on_progress: None,
            is_cancelled: None,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperPluginProcessResult {
    pub status: VesperPluginResultStatus,
    pub payload: VesperPluginBytes,
}

impl Default for VesperPluginProcessResult {
    fn default() -> Self {
        Self {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes::null(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperPostDownloadProcessorApi {
    pub context: *mut c_void,
    pub destroy: Option<unsafe extern "C" fn(context: *mut c_void)>,
    pub name: Option<unsafe extern "C" fn(context: *mut c_void) -> *const c_char>,
    pub capabilities_json: Option<unsafe extern "C" fn(context: *mut c_void) -> VesperPluginBytes>,
    pub free_bytes: Option<unsafe extern "C" fn(context: *mut c_void, payload: VesperPluginBytes)>,
    pub process_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            input_json: *const u8,
            input_json_len: usize,
            output_path: *const c_char,
            progress: VesperPluginProgressCallbacks,
        ) -> VesperPluginProcessResult,
    >,
}

// SAFETY: host-side wrappers only expose this API behind `PostDownloadProcessor`,
// and plugin authors must uphold the declared `Send + Sync` contract for the
// underlying context pointer and callbacks.
unsafe impl Send for VesperPostDownloadProcessorApi {}
// SAFETY: same reasoning as above; the plugin context is required to be safe for
// concurrent shared access when exposed as a post-download processor.
unsafe impl Sync for VesperPostDownloadProcessorApi {}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperPipelineEventHookApi {
    pub context: *mut c_void,
    pub destroy: Option<unsafe extern "C" fn(context: *mut c_void)>,
    pub name: Option<unsafe extern "C" fn(context: *mut c_void) -> *const c_char>,
    pub on_event_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            event_json: *const u8,
            event_json_len: usize,
        ) -> bool,
    >,
}

// SAFETY: host-side wrappers only expose this API behind `PipelineEventHook`,
// and plugin authors must uphold the declared `Send + Sync` contract for the
// underlying context pointer and callbacks.
unsafe impl Send for VesperPipelineEventHookApi {}
// SAFETY: same reasoning as above; the plugin context is required to be safe for
// concurrent shared access when exposed as a pipeline event hook.
unsafe impl Sync for VesperPipelineEventHookApi {}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperPluginDescriptor {
    pub abi_version: u32,
    pub plugin_kind: VesperPluginKind,
    pub plugin_name: *const c_char,
    pub api: *const c_void,
}

pub type VesperPluginEntryPoint = unsafe extern "C" fn() -> *const VesperPluginDescriptor;

pub const VESPER_PLUGIN_ABI_VERSION: u32 = 1;
pub const VESPER_PLUGIN_ENTRY_SYMBOL: &[u8] = b"vesper_plugin_entry\0";
