use std::ffi::{c_char, c_void};

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VesperPluginKind {
    PostDownloadProcessor = 1,
    PipelineEventHook = 2,
    Decoder = 3,
    BenchmarkSink = 4,
    FrameProcessor = 5,
    SourceNormalizer = 6,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VesperPluginResultStatus {
    Success = 0,
    Failure = 1,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ABI-safe byte buffer transferred between the host and a plugin.
///
/// The producer must allocate the buffer from `Vec<u8>` inside the same dynamic
/// library that later reclaims it through the matching `free_bytes` callback.
/// `data` may be null only when `len == 0`.
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

    pub fn from_vec(bytes: Vec<u8>) -> Self {
        if bytes.is_empty() {
            return Self::null();
        }

        let mut boxed = bytes.into_boxed_slice();
        let result = Self {
            data: boxed.as_mut_ptr(),
            len: boxed.len(),
        };
        std::mem::forget(boxed);
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

        // SAFETY: guaranteed by the caller contract above. `from_vec` transfers
        // ownership as a boxed slice, so the allocation layout is exactly the
        // slice length recorded in the ABI payload.
        unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(self.data, self.len)).into_vec() }
    }
}

#[cfg(test)]
mod tests {
    use super::VesperPluginBytes;

    #[test]
    fn plugin_bytes_round_trip_vec_with_extra_capacity() {
        let mut bytes = Vec::with_capacity(64);
        bytes.extend_from_slice(b"decoder payload");
        assert!(bytes.capacity() > bytes.len());

        let payload = VesperPluginBytes::from_vec(bytes);
        assert!(!payload.data.is_null());
        assert_eq!(payload.len, b"decoder payload".len());

        // SAFETY: the payload was produced by `VesperPluginBytes::from_vec`
        // above and has not been reclaimed yet.
        let recovered = unsafe { payload.into_vec() };

        assert_eq!(recovered, b"decoder payload");
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// Host-provided callbacks used by a post-download processor to report progress.
///
/// `context` is borrowed for the duration of a single synchronous
/// `process_json` call. Plugins must not store it after `process_json`
/// returns, and callbacks must be invoked on the same thread before returning.
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
/// Result returned by `VesperPostDownloadProcessorApi::process_json`.
///
/// When `status` is `Success`, `payload` must encode a `ProcessorOutput` JSON
/// document. When `status` is `Failure`, it must encode a `ProcessorError`
/// JSON document.
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
/// C ABI exposed by a post-download processor plugin.
///
/// `capabilities_json`, `process_json`, and `assemble_json` must return
/// `VesperPluginBytes` values that are reclaimed with the matching `free_bytes`
/// callback from the same dynamic library. `process_json` and `assemble_json`
/// must complete synchronously and must not retain the
/// `VesperPluginProgressCallbacks::context` pointer after returning.
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
    pub assemble_json: Option<
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
/// C ABI exposed by a pipeline event hook plugin.
///
/// `on_event_json` receives one UTF-8 JSON event payload per call and must not
/// retain any host-owned pointers after the callback returns.
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
/// C ABI exposed by a benchmark sink plugin.
///
/// `on_event_batch_json` receives one UTF-8 JSON `BenchmarkEventBatch` payload
/// per call. `flush_json` returns a UTF-8 JSON `BenchmarkSinkReport` payload on
/// success. Plugins must not retain host-owned pointers after a callback
/// returns.
pub struct VesperBenchmarkSinkApi {
    pub context: *mut c_void,
    pub destroy: Option<unsafe extern "C" fn(context: *mut c_void)>,
    pub name: Option<unsafe extern "C" fn(context: *mut c_void) -> *const c_char>,
    pub free_bytes: Option<unsafe extern "C" fn(context: *mut c_void, payload: VesperPluginBytes)>,
    pub on_event_batch_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            batch_json: *const u8,
            batch_json_len: usize,
        ) -> VesperPluginProcessResult,
    >,
    pub flush_json: Option<unsafe extern "C" fn(context: *mut c_void) -> VesperPluginProcessResult>,
}

// SAFETY: host-side wrappers only expose this API behind `BenchmarkSink`, and
// plugin authors must uphold the declared `Send + Sync` contract for the
// underlying context pointer and callbacks.
unsafe impl Send for VesperBenchmarkSinkApi {}
// SAFETY: same reasoning as above; the plugin context is required to be safe for
// concurrent shared access when exposed as a benchmark sink.
unsafe impl Sync for VesperBenchmarkSinkApi {}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// Result returned by `VesperDecoderPluginApiV2::open_session_json`.
///
/// When `status` is `Success`, `session` must be a plugin-owned opaque session
/// pointer and `payload` may encode a `DecoderSessionInfo` JSON document. When
/// `status` is `Failure`, `session` must be null and `payload` must encode a
/// `DecoderError` JSON document.
pub struct VesperDecoderOpenSessionResult {
    pub status: VesperPluginResultStatus,
    pub session: *mut c_void,
    pub payload: VesperPluginBytes,
}

impl Default for VesperDecoderOpenSessionResult {
    fn default() -> Self {
        Self {
            status: VesperPluginResultStatus::Success,
            session: std::ptr::null_mut(),
            payload: VesperPluginBytes::null(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// Result returned by `VesperDecoderPluginApiV2::receive_native_frame`.
///
/// On success, `metadata` must encode a `DecoderReceiveNativeFrameMetadata`
/// JSON document. When that metadata reports a frame, `handle` is a plugin-owned
/// native frame handle that must later be released through
/// `release_native_frame`. On failure, `metadata` must encode a `DecoderError`
/// JSON document and `handle` must be zero.
pub struct VesperDecoderReceiveNativeFrameResult {
    pub status: VesperPluginResultStatus,
    pub metadata: VesperPluginBytes,
    pub handle: usize,
}

impl Default for VesperDecoderReceiveNativeFrameResult {
    fn default() -> Self {
        Self {
            status: VesperPluginResultStatus::Success,
            metadata: VesperPluginBytes::null(),
            handle: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// C ABI exposed by a decoder plugin that returns native frame handles.
///
/// The v2 decoder ABI keeps the v1 packet/session lifecycle but returns a
/// platform native frame handle from `receive_native_frame`. The host must call
/// `release_native_frame` exactly once for every successful frame handle.
pub struct VesperDecoderPluginApiV2 {
    pub context: *mut c_void,
    pub destroy: Option<unsafe extern "C" fn(context: *mut c_void)>,
    pub name: Option<unsafe extern "C" fn(context: *mut c_void) -> *const c_char>,
    pub capabilities_json: Option<unsafe extern "C" fn(context: *mut c_void) -> VesperPluginBytes>,
    pub native_requirements_json:
        Option<unsafe extern "C" fn(context: *mut c_void) -> VesperPluginBytes>,
    pub open_session_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            config_json: *const u8,
            config_json_len: usize,
        ) -> VesperDecoderOpenSessionResult,
    >,
    pub send_packet: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
            packet_json: *const u8,
            packet_json_len: usize,
            packet_data: *const u8,
            packet_data_len: usize,
        ) -> VesperPluginProcessResult,
    >,
    pub receive_native_frame: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperDecoderReceiveNativeFrameResult,
    >,
    pub release_native_frame: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
            handle_kind: u32,
            handle: usize,
        ) -> VesperPluginProcessResult,
    >,
    pub flush_session: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperPluginProcessResult,
    >,
    pub close_session: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperPluginProcessResult,
    >,
    pub free_bytes: Option<unsafe extern "C" fn(context: *mut c_void, payload: VesperPluginBytes)>,
}

// SAFETY: host-side wrappers only expose this API behind
// `NativeDecoderPluginFactory`, and plugin authors must uphold the declared
// `Send + Sync` contract for the underlying context pointer and callbacks.
unsafe impl Send for VesperDecoderPluginApiV2 {}
// SAFETY: same reasoning as above; the plugin context is required to be safe for
// concurrent shared access when exposed as a decoder plugin.
unsafe impl Sync for VesperDecoderPluginApiV2 {}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// Result returned by `VesperFrameProcessorPluginApiV1::open_session_json`.
///
/// When `status` is `Success`, `session` must be a plugin-owned opaque session
/// pointer and `payload` may encode a `FrameProcessorSessionInfo` JSON document.
/// When `status` is `Failure`, `session` must be null and `payload` must encode
/// a `FrameProcessorError` JSON document.
pub struct VesperFrameProcessorOpenSessionResult {
    pub status: VesperPluginResultStatus,
    pub session: *mut c_void,
    pub payload: VesperPluginBytes,
}

impl Default for VesperFrameProcessorOpenSessionResult {
    fn default() -> Self {
        Self {
            status: VesperPluginResultStatus::Success,
            session: std::ptr::null_mut(),
            payload: VesperPluginBytes::null(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// Result returned by `VesperFrameProcessorPluginApiV1::receive_frame`.
///
/// On success, `metadata` must encode a `FrameProcessorReceiveFrameMetadata`
/// JSON document. When that metadata reports a frame, `handle` is a
/// plugin-owned native frame handle that must later be released through
/// `release_frame`. On failure, `metadata` must encode a `FrameProcessorError`
/// JSON document and `handle` must be zero.
pub struct VesperFrameProcessorReceiveFrameResult {
    pub status: VesperPluginResultStatus,
    pub metadata: VesperPluginBytes,
    pub handle: usize,
}

impl Default for VesperFrameProcessorReceiveFrameResult {
    fn default() -> Self {
        Self {
            status: VesperPluginResultStatus::Success,
            metadata: VesperPluginBytes::null(),
            handle: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// C ABI exposed by a frame processor plugin.
///
/// The v1 frame processor ABI processes `NativeFrame -> NativeFrame` for video
/// frames only. `submit_frame_json` borrows the input handle for the duration of
/// a synchronous submit call; processor-owned output handles are returned by
/// `receive_frame` and must be released exactly once through `release_frame`.
pub struct VesperFrameProcessorPluginApiV1 {
    pub context: *mut c_void,
    pub destroy: Option<unsafe extern "C" fn(context: *mut c_void)>,
    pub name: Option<unsafe extern "C" fn(context: *mut c_void) -> *const c_char>,
    pub capabilities_json: Option<unsafe extern "C" fn(context: *mut c_void) -> VesperPluginBytes>,
    pub open_session_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            config_json: *const u8,
            config_json_len: usize,
        ) -> VesperFrameProcessorOpenSessionResult,
    >,
    pub submit_frame_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
            submit_json: *const u8,
            submit_json_len: usize,
            handle: usize,
        ) -> VesperPluginProcessResult,
    >,
    pub receive_frame: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperFrameProcessorReceiveFrameResult,
    >,
    pub release_frame: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
            handle_kind: u32,
            handle: usize,
        ) -> VesperPluginProcessResult,
    >,
    pub flush_session: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperPluginProcessResult,
    >,
    pub close_session: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperPluginProcessResult,
    >,
    pub free_bytes: Option<unsafe extern "C" fn(context: *mut c_void, payload: VesperPluginBytes)>,
}

// SAFETY: host-side wrappers only expose this API behind
// `FrameProcessorPluginFactory`, and plugin authors must uphold the declared
// `Send + Sync` contract for the underlying context pointer and callbacks.
unsafe impl Send for VesperFrameProcessorPluginApiV1 {}
// SAFETY: same reasoning as above; the plugin context is required to be safe for
// concurrent shared access when exposed as a frame processor plugin.
unsafe impl Sync for VesperFrameProcessorPluginApiV1 {}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// Result returned by `VesperSourceNormalizerPluginApiV2::open_packet_session_json`.
///
/// When `status` is `Success`, `session` must be a plugin-owned opaque session
/// pointer and `payload` must encode a `SourceNormalizerPacketStreamInfo` JSON
/// document. When `status` is `Failure`, `session` must be null and `payload`
/// must encode a `SourceNormalizerError` JSON document.
pub struct VesperSourceNormalizerOpenPacketSessionResult {
    pub status: VesperPluginResultStatus,
    pub session: *mut c_void,
    pub payload: VesperPluginBytes,
}

impl Default for VesperSourceNormalizerOpenPacketSessionResult {
    fn default() -> Self {
        Self {
            status: VesperPluginResultStatus::Success,
            session: std::ptr::null_mut(),
            payload: VesperPluginBytes::null(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// Result returned by `VesperSourceNormalizerPluginApiV3::open_resource_session_json`.
///
/// When `status` is `Success`, `session` must be a plugin-owned opaque session
/// pointer and `payload` must encode a `SourceNormalizerResourceSessionInfo`
/// JSON document. When `status` is `Failure`, `session` must be null and
/// `payload` must encode a `SourceNormalizerError` JSON document.
pub struct VesperSourceNormalizerOpenResourceSessionResult {
    pub status: VesperPluginResultStatus,
    pub session: *mut c_void,
    pub payload: VesperPluginBytes,
}

impl Default for VesperSourceNormalizerOpenResourceSessionResult {
    fn default() -> Self {
        Self {
            status: VesperPluginResultStatus::Success,
            session: std::ptr::null_mut(),
            payload: VesperPluginBytes::null(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// Result returned by `VesperSourceNormalizerPluginApiV2::read_packet`.
///
/// On success, `metadata` must encode `SourceNormalizerReadPacketMetadata`.
/// When that metadata reports a packet, `data/data_len` borrow plugin-owned
/// packet bytes until the host calls `release_packet` with `packet_handle`.
/// On failure, `metadata` must encode `SourceNormalizerError`.
pub struct VesperSourceNormalizerReadPacketResult {
    pub status: VesperPluginResultStatus,
    pub metadata: VesperPluginBytes,
    pub data: *const u8,
    pub data_len: usize,
    pub packet_handle: usize,
}

impl Default for VesperSourceNormalizerReadPacketResult {
    fn default() -> Self {
        Self {
            status: VesperPluginResultStatus::Success,
            metadata: VesperPluginBytes::null(),
            data: std::ptr::null(),
            data_len: 0,
            packet_handle: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// C ABI exposed by a packet-stream source normalizer plugin.
///
/// The v2 source normalizer ABI produces borrowed compressed packets. Packet
/// bytes remain plugin-owned and valid until the host calls `release_packet`
/// with the returned packet handle.
pub struct VesperSourceNormalizerPluginApiV2 {
    pub context: *mut c_void,
    pub destroy: Option<unsafe extern "C" fn(context: *mut c_void)>,
    pub name: Option<unsafe extern "C" fn(context: *mut c_void) -> *const c_char>,
    pub packet_capabilities_json:
        Option<unsafe extern "C" fn(context: *mut c_void) -> VesperPluginBytes>,
    pub open_packet_session_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            config_json: *const u8,
            config_json_len: usize,
        ) -> VesperSourceNormalizerOpenPacketSessionResult,
    >,
    pub read_packet: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperSourceNormalizerReadPacketResult,
    >,
    pub release_packet: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
            packet_handle: usize,
        ) -> VesperPluginProcessResult,
    >,
    pub seek_packet_session_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
            seek_json: *const u8,
            seek_json_len: usize,
        ) -> VesperPluginProcessResult,
    >,
    pub flush_packet_session: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperPluginProcessResult,
    >,
    pub close_packet_session: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperPluginProcessResult,
    >,
    pub free_bytes: Option<unsafe extern "C" fn(context: *mut c_void, payload: VesperPluginBytes)>,
}

// SAFETY: host-side wrappers only expose this API behind
// `SourceNormalizerPacketPluginFactory`, and plugin authors must uphold the
// declared `Send + Sync` contract for the underlying context pointer.
unsafe impl Send for VesperSourceNormalizerPluginApiV2 {}
// SAFETY: same reasoning as above; the plugin context is required to be safe for
// concurrent shared access when exposed as a source normalizer plugin.
unsafe impl Sync for VesperSourceNormalizerPluginApiV2 {}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// C ABI exposed by a source normalizer plugin with resource-output support.
///
/// V3 preserves the V2 packet callbacks and adds optional disk-backed resource
/// sessions for fMP4 local stream and short-window HLS routes. Resource session
/// payloads are JSON-encoded and all returned byte buffers must be reclaimed
/// with the matching `free_bytes` callback from the same dynamic library.
pub struct VesperSourceNormalizerPluginApiV3 {
    pub context: *mut c_void,
    pub destroy: Option<unsafe extern "C" fn(context: *mut c_void)>,
    pub name: Option<unsafe extern "C" fn(context: *mut c_void) -> *const c_char>,
    pub packet_capabilities_json:
        Option<unsafe extern "C" fn(context: *mut c_void) -> VesperPluginBytes>,
    pub open_packet_session_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            config_json: *const u8,
            config_json_len: usize,
        ) -> VesperSourceNormalizerOpenPacketSessionResult,
    >,
    pub read_packet: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperSourceNormalizerReadPacketResult,
    >,
    pub release_packet: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
            packet_handle: usize,
        ) -> VesperPluginProcessResult,
    >,
    pub seek_packet_session_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
            seek_json: *const u8,
            seek_json_len: usize,
        ) -> VesperPluginProcessResult,
    >,
    pub flush_packet_session: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperPluginProcessResult,
    >,
    pub close_packet_session: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperPluginProcessResult,
    >,
    pub resource_capabilities_json:
        Option<unsafe extern "C" fn(context: *mut c_void) -> VesperPluginBytes>,
    pub open_resource_session_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            config_json: *const u8,
            config_json_len: usize,
        ) -> VesperSourceNormalizerOpenResourceSessionResult,
    >,
    pub poll_resource_session: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperPluginProcessResult,
    >,
    pub cancel_resource_session: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperPluginProcessResult,
    >,
    pub close_resource_session: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session: *mut c_void,
        ) -> VesperPluginProcessResult,
    >,
    pub free_bytes: Option<unsafe extern "C" fn(context: *mut c_void, payload: VesperPluginBytes)>,
}

// SAFETY: host-side wrappers only expose this API behind source normalizer
// factory traits, and plugin authors must uphold the declared `Send + Sync`
// contract for the underlying context pointer.
unsafe impl Send for VesperSourceNormalizerPluginApiV3 {}
// SAFETY: same reasoning as above; the plugin context is required to be safe for
// concurrent shared access when exposed as a source normalizer plugin.
unsafe impl Sync for VesperSourceNormalizerPluginApiV3 {}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// Plugin descriptor exported by `vesper_plugin_entry`.
///
/// `plugin_name` must be a valid NUL-terminated UTF-8 string and `api` must
/// point to the ABI table matching `plugin_kind`.
pub struct VesperPluginDescriptor {
    pub abi_version: u32,
    pub plugin_kind: VesperPluginKind,
    pub plugin_name: *const c_char,
    pub api: *const c_void,
}

/// Entry point exported by every plugin dynamic library.
pub type VesperPluginEntryPoint = unsafe extern "C" fn() -> *const VesperPluginDescriptor;

/// ABI version used by non post-download plugin kinds.
pub const VESPER_PLUGIN_ABI_VERSION_V2: u32 = 2;
/// Legacy native-frame decoder ABI version.
pub const VESPER_DECODER_PLUGIN_ABI_VERSION_V2: u32 = VESPER_PLUGIN_ABI_VERSION_V2;
/// Native-frame decoder ABI with typed native context payloads.
pub const VESPER_DECODER_PLUGIN_ABI_VERSION_V3: u32 = 3;
/// ABI version used by post-download processors with assembly support.
pub const VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3: u32 = 3;
/// Initial ABI version used by native-frame processor plugins.
pub const VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1: u32 = 1;
/// ABI version used by packet-stream source normalizer plugins.
pub const VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2: u32 = 2;
/// Source normalizer ABI with disk-backed resource-output sessions.
pub const VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V3: u32 = 3;
/// Exported symbol name used to locate the plugin descriptor entry point.
pub const VESPER_PLUGIN_ENTRY_SYMBOL: &[u8] = b"vesper_plugin_entry\0";
