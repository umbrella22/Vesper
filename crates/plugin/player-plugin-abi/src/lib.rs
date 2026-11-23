//! C-compatible contracts for native Vesper plugins.

#![warn(clippy::undocumented_unsafe_blocks)]

//! native plugin ABI contracts. The C header is generated from this module.

use core::ffi::{c_char, c_void};

pub const VESPER_PLUGIN_ABI_MAJOR: u16 = 2;
pub const VESPER_PLUGIN_ABI_MINOR: u16 = 0;
pub const VESPER_MAX_INTERFACES_PER_PLUGIN: u32 = 64;
pub const VESPER_MAX_LEASES_PER_SESSION: usize = 64;
/// Maximum UTF-8 byte length of a reverse-DNS plugin identity.
pub const VESPER_MAX_PLUGIN_ID_BYTES: usize = 255;
/// Maximum UTF-8 byte length of a human-readable plugin name.
pub const VESPER_MAX_PLUGIN_NAME_BYTES: usize = 255;
/// Maximum UTF-8 byte length of a capability instance identity.
pub const VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES: usize = 255;
/// Maximum size accepted for one borrowed source-normalizer packet payload.
pub const VESPER_MAX_PACKET_BYTES: u64 = 16 * 1024 * 1024;

pub type VesperStatus = u32;
pub const VESPER_OK: VesperStatus = 0;
pub const VESPER_NEED_MORE_INPUT: VesperStatus = 1;
pub const VESPER_TRY_AGAIN: VesperStatus = 2;
pub const VESPER_EOF: VesperStatus = 3;
pub const VESPER_PASSTHROUGH: VesperStatus = 4;
pub const VESPER_FORMAT_CHANGED: VesperStatus = 5;
pub const VESPER_ERR_INVALID_ARG: VesperStatus = 100;
pub const VESPER_ERR_UNSUPPORTED: VesperStatus = 101;
pub const VESPER_ERR_INTERNAL: VesperStatus = 102;
pub const VESPER_ERR_GPU_LOST: VesperStatus = 103;
pub const VESPER_ERR_EXHAUSTED: VesperStatus = 104;
pub const VESPER_ERR_ABORTED: VesperStatus = 105;
pub const VESPER_ERR_PANIC: VesperStatus = 106;

pub const VESPER_PIXEL_NV12: u32 = 1;
pub const VESPER_PIXEL_P010: u32 = 2;
pub const VESPER_PIXEL_YUV420P: u32 = 3;
pub const VESPER_PIXEL_RGBA8: u32 = 4;
pub const VESPER_PIXEL_BGRA8: u32 = 5;
pub const VESPER_PIXEL_RGBA16F: u32 = 6;
pub const VESPER_PIXEL_P016: u32 = 7;
pub const VESPER_HDR_SDR: u8 = 0;
pub const VESPER_HDR_HDR10: u8 = 1;
pub const VESPER_HDR_HLG: u8 = 2;
pub const VESPER_HDR_DOLBY_VISION: u8 = 3;
pub const VESPER_SAMPLE_F32_PLANAR: u32 = 1;
pub const VESPER_SAMPLE_F32_INTERLEAVED: u32 = 2;
pub const VESPER_SAMPLE_S16_INTERLEAVED: u32 = 3;
pub const VESPER_PACKET_KEYFRAME: u32 = 1;
pub const VESPER_PACKET_DISCONTINUITY: u32 = 2;
pub const VESPER_PACKET_EOS: u32 = 4;
pub const VESPER_PACKET_CONFIG_CHANGED: u32 = 8;
pub const VESPER_SEEK_BACKWARD: u32 = 1;
pub const VESPER_HANDLE_CV_PIXEL_BUFFER: u32 = 1;
pub const VESPER_HANDLE_MTL_TEXTURE: u32 = 2;
pub const VESPER_HANDLE_AHB: u32 = 3;
pub const VESPER_HANDLE_OH_NATIVE_BUFFER: u32 = 4;
pub const VESPER_HANDLE_VK_IMAGE: u32 = 5;
pub const VESPER_HANDLE_D3D11_TEXTURE2D: u32 = 6;
pub const VESPER_HANDLE_DMABUF: u32 = 7;
pub const VESPER_HANDLE_CPU_PLANES: u32 = 8;
pub const VESPER_SYNC_NONE: u32 = 0;
pub const VESPER_SYNC_MTL_EVENT: u32 = 1;
pub const VESPER_SYNC_VK_TIMELINE_SEMAPHORE: u32 = 2;
pub const VESPER_SYNC_FD: u32 = 3;
pub const VESPER_RELEASE_RENDERED: u32 = 1;
pub const VESPER_RELEASE_DROPPED: u32 = 2;
pub const VESPER_GPU_NONE: u32 = 0;
pub const VESPER_GPU_METAL: u32 = 1;
pub const VESPER_GPU_VULKAN: u32 = 2;
pub const VESPER_GPU_D3D11: u32 = 3;
pub const VESPER_PITCH_PRESERVE: u32 = 0;
pub const VESPER_PITCH_FOLLOW_RATE: u32 = 1;

/// Fixed-layout RFC 4122 bytes in network order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct VesperInterfaceId {
    pub bytes: [u8; 16],
}

pub const VESPER_VIDEO_DECODER_ID: VesperInterfaceId = VesperInterfaceId {
    bytes: [
        0x0c, 0x42, 0x81, 0x5b, 0xfc, 0x32, 0x57, 0x1a, 0xbc, 0xea, 0x86, 0x79, 0x48, 0x29, 0xe3,
        0x9b,
    ],
};

pub const VESPER_VIDEO_FILTER_ID: VesperInterfaceId = VesperInterfaceId {
    bytes: [
        0xb3, 0xb9, 0x1a, 0x08, 0x47, 0x21, 0x55, 0xa4, 0xbe, 0x67, 0xc5, 0x59, 0xff, 0xa3, 0x49,
        0xf1,
    ],
};

pub const VESPER_AUDIO_PROCESSOR_ID: VesperInterfaceId = VesperInterfaceId {
    bytes: [
        0xa6, 0xda, 0x8f, 0x51, 0xe4, 0x72, 0x51, 0x3c, 0x9f, 0xcc, 0x9a, 0x28, 0xb2, 0x6c, 0xa4,
        0x6b,
    ],
};

pub const VESPER_SOURCE_NORMALIZER_ID: VesperInterfaceId = VesperInterfaceId {
    bytes: [
        0xbf, 0xb9, 0x37, 0xfd, 0xaa, 0x2c, 0x52, 0x69, 0xa4, 0x92, 0x66, 0x99, 0xff, 0x1d, 0x58,
        0xfa,
    ],
};

pub const VESPER_SOURCE_NORMALIZER_RESOURCE_ID: VesperInterfaceId = VesperInterfaceId {
    bytes: [
        0x19, 0x46, 0x36, 0x72, 0x8b, 0x42, 0x57, 0xc9, 0xb1, 0xf7, 0x70, 0xf5, 0x16, 0x5d, 0x33,
        0x71,
    ],
};

pub const VESPER_POST_DOWNLOAD_PROCESSOR_ID: VesperInterfaceId = VesperInterfaceId {
    bytes: [
        0x00, 0x1e, 0xe1, 0x0d, 0x4c, 0x34, 0x52, 0xc4, 0xbd, 0x30, 0x62, 0x51, 0x63, 0x7f, 0x47,
        0xa6,
    ],
};

/// A fixed-layout borrowed slice. A null pointer is valid when len is zero.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VesperBytes {
    pub data: *const u8,
    pub len: u64,
}

/// ITU-T H.273 code points. Unspecified primaries, transfer or matrix use 2.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VesperColorInfo {
    pub primaries: u8,
    pub transfer: u8,
    pub matrix: u8,
    /// 0 is unspecified, 1 is narrow, and 2 is full.
    pub range: u8,
    pub bit_depth: u8,
    pub hdr_kind: u8,
    pub reserved: [u8; 2],
}

/// ST 2086 chromaticities use units of 1/50000. Luminance uses units of 1/10000 nit.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VesperHdrStaticMetadata {
    pub display_primaries_x: [u16; 3],
    pub display_primaries_y: [u16; 3],
    pub white_point_x: u16,
    pub white_point_y: u16,
    pub max_luminance_nits_x10000: u32,
    pub min_luminance_nits_x10000: u32,
    pub max_cll: u16,
    pub max_fall: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VesperVideoFormat {
    pub struct_size: u32,
    pub pixel_format: u32,
    /// Starts at 1 and strictly increases when the session signals a format change.
    pub format_id: u64,
    pub width: u32,
    pub height: u32,
    pub coded_width: u32,
    pub coded_height: u32,
    pub crop_x: u32,
    pub crop_y: u32,
    pub crop_w: u32,
    pub crop_h: u32,
    pub rotation_deg: i16,
    pub mirror_flags: u8,
    pub has_hdr_static: u8,
    pub color: VesperColorInfo,
    pub hdr_static: VesperHdrStaticMetadata,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VesperAudioFormat {
    pub struct_size: u32,
    pub sample_format: u32,
    pub format_id: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub max_block_frames: u16,
    pub channel_layout: u64,
}

/// Timestamps are in microseconds. INT64_MIN marks an unknown timestamp.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VesperPacket {
    pub struct_size: u32,
    pub flags: u32,
    pub pts_us: i64,
    pub dts_us: i64,
    pub duration_us: i64,
    pub stream_index: u32,
    pub data: *const u8,
    pub len: u64,
}

/// Release each nonzero lease once, after GPU completion, even after session destruction.
/// The callback and its context must be valid on the completion thread.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VesperVideoFrame {
    pub struct_size: u32,
    pub flags: u32,
    pub format_id: u64,
    pub pts_us: i64,
    pub duration_us: i64,
    pub handle_kind: u32,
    pub plane_count: u32,
    pub handles: [u64; 3],
    pub strides: [u32; 3],
    pub sync_kind: u32,
    pub sync_handle: u64,
    pub sync_value: u64,
    pub lease_id: u64,
    pub release_ctx: *mut c_void,
    pub release_fn: Option<unsafe extern "C" fn(ctx: *mut c_void, lease_id: u64, disposition: u32)>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperMetalContext {
    pub device: *mut c_void,
    pub command_queue: *mut c_void,
    pub command_buffer: *mut c_void,
    pub texture_cache: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperVulkanContext {
    pub instance: *mut c_void,
    pub physical_device: *mut c_void,
    pub device: *mut c_void,
    pub queue: *mut c_void,
    pub command_buffer: *mut c_void,
    pub get_instance_proc_addr: *const c_void,
    pub queue_family: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperD3D11Context {
    pub device: *mut c_void,
    pub immediate_context: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union VesperGpuContextData {
    pub metal: VesperMetalContext,
    pub vulkan: VesperVulkanContext,
    pub d3d11: VesperD3D11Context,
    pub reserved: [u64; 8],
}

/// GPU objects are borrowed. Only the host submits the shared command buffer.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperGpuContext {
    pub struct_size: u32,
    pub backend: u32,
    pub data: VesperGpuContextData,
}

/// PCM is borrowed for this call. Input planes are read-only unless process_block
/// is called with the same writable descriptor as input and output. Each output
/// contains frames for exactly one rate_epoch, including during EOS drain.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VesperAudioBuffer {
    pub struct_size: u32,
    pub flags: u32,
    pub format_id: u64,
    pub pts_us: i64,
    pub rate_epoch: u64,
    pub frame_count: u32,
    pub frame_capacity: u32,
    pub planes: [*mut c_void; 8],
}

/// Session handles are exclusively owned and serialized by the host.
pub type VesperSession = c_void;
pub type VesperGetJson =
    Option<unsafe extern "C" fn(ctx: *mut c_void, out: *mut VesperBytes) -> VesperStatus>;
pub type VesperFlush = Option<unsafe extern "C" fn(session: *mut VesperSession) -> VesperStatus>;
pub type VesperDestroy = Option<unsafe extern "C" fn(session: *mut VesperSession)>;
pub type VesperGetLastError =
    Option<unsafe extern "C" fn(session: *mut VesperSession, out_len: *mut u32) -> *const c_char>;
pub type VesperOpenSource = Option<
    unsafe extern "C" fn(
        ctx: *mut c_void,
        uri: VesperBytes,
        params: VesperBytes,
        out_session: *mut *mut VesperSession,
    ) -> VesperStatus,
>;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperVideoDecoderVtable {
    pub struct_size: u32,
    pub query_capabilities_json: VesperGetJson,
    pub open: Option<
        unsafe extern "C" fn(
            ctx: *mut c_void,
            gpu: *const VesperGpuContext,
            codec_fourcc: u32,
            extradata: VesperBytes,
            params: VesperBytes,
            out_session: *mut *mut VesperSession,
        ) -> VesperStatus,
    >,
    pub get_format: Option<
        unsafe extern "C" fn(
            session: *mut VesperSession,
            out: *mut VesperVideoFormat,
        ) -> VesperStatus,
    >,
    pub send_packet: Option<
        unsafe extern "C" fn(
            session: *mut VesperSession,
            packet: *const VesperPacket,
        ) -> VesperStatus,
    >,
    pub receive_frame: Option<
        unsafe extern "C" fn(
            session: *mut VesperSession,
            out: *mut VesperVideoFrame,
        ) -> VesperStatus,
    >,
    pub flush: VesperFlush,
    pub destroy: VesperDestroy,
    /// Optional UTF-8 detail, read on the failing call's thread before the next
    /// decoder call. A null session requests the detail of an unsuccessful open.
    /// The returned bytes are borrowed until the next decoder call on this thread.
    pub get_last_error: VesperGetLastError,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperVideoFilterVtable {
    pub struct_size: u32,
    pub query_capabilities_json: VesperGetJson,
    pub open: Option<
        unsafe extern "C" fn(
            ctx: *mut c_void,
            gpu: *const VesperGpuContext,
            input: *const VesperVideoFormat,
            params: VesperBytes,
            output: *mut VesperVideoFormat,
            out_session: *mut *mut VesperSession,
        ) -> VesperStatus,
    >,
    pub set_param: Option<
        unsafe extern "C" fn(
            session: *mut VesperSession,
            key: VesperBytes,
            value: VesperBytes,
        ) -> VesperStatus,
    >,
    /// A null input drains the filter. PASSTHROUGH keeps the input lease.
    pub process: Option<
        unsafe extern "C" fn(
            session: *mut VesperSession,
            gpu: *const VesperGpuContext,
            input: *const VesperVideoFrame,
            out: *mut VesperVideoFrame,
        ) -> VesperStatus,
    >,
    pub get_format: Option<
        unsafe extern "C" fn(
            session: *mut VesperSession,
            out: *mut VesperVideoFormat,
        ) -> VesperStatus,
    >,
    pub flush: VesperFlush,
    pub destroy: VesperDestroy,
    pub get_last_error: VesperGetLastError,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperAudioProcessorVtable {
    pub struct_size: u32,
    pub query_capabilities_json: VesperGetJson,
    pub open: Option<
        unsafe extern "C" fn(
            ctx: *mut c_void,
            input: *const VesperAudioFormat,
            params: VesperBytes,
            output: *mut VesperAudioFormat,
            out_latency_frames: *mut u32,
            out_session: *mut *mut VesperSession,
        ) -> VesperStatus,
    >,
    pub set_playback_policy: Option<
        unsafe extern "C" fn(
            session: *mut VesperSession,
            rate: f32,
            pitch_mode: u32,
            rate_epoch: u64,
        ) -> VesperStatus,
    >,
    pub set_param: Option<
        unsafe extern "C" fn(
            session: *mut VesperSession,
            key: VesperBytes,
            value: VesperBytes,
        ) -> VesperStatus,
    >,
    pub process_block: Option<
        unsafe extern "C" fn(
            session: *mut VesperSession,
            input: *const VesperAudioBuffer,
            out: *mut VesperAudioBuffer,
        ) -> VesperStatus,
    >,
    /// A null input marks EOS. A non-null input may be partially consumed.
    pub push: Option<
        unsafe extern "C" fn(
            session: *mut VesperSession,
            input: *const VesperAudioBuffer,
            out_consumed_frames: *mut u32,
        ) -> VesperStatus,
    >,
    /// EOF is returned only after buffered output from every epoch is drained.
    pub pull: Option<
        unsafe extern "C" fn(
            session: *mut VesperSession,
            out: *mut VesperAudioBuffer,
            out_produced_frames: *mut u32,
        ) -> VesperStatus,
    >,
    pub latency_frames: Option<unsafe extern "C" fn(session: *mut VesperSession) -> u32>,
    pub flush: VesperFlush,
    pub destroy: VesperDestroy,
    pub get_last_error: VesperGetLastError,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperSourceNormalizerVtable {
    pub struct_size: u32,
    pub query_capabilities_json: VesperGetJson,
    pub open: VesperOpenSource,
    pub get_stream_info: Option<
        unsafe extern "C" fn(session: *mut VesperSession, out: *mut VesperBytes) -> VesperStatus,
    >,
    /// On OK the packet borrows bytes until its nonzero lease is released.
    pub read_packet: Option<
        unsafe extern "C" fn(
            session: *mut VesperSession,
            out: *mut VesperPacket,
            out_lease_id: *mut u64,
        ) -> VesperStatus,
    >,
    pub release_packet:
        Option<unsafe extern "C" fn(session: *mut VesperSession, lease_id: u64) -> VesperStatus>,
    pub seek: Option<
        unsafe extern "C" fn(session: *mut VesperSession, pts_us: i64, flags: u32) -> VesperStatus,
    >,
    pub flush: VesperFlush,
    pub destroy: VesperDestroy,
    pub get_last_error: VesperGetLastError,
}

/// Resource production is independent of packet streaming. Destroy releases the
/// resource after the platform player has finished consuming it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperSourceNormalizerResourceVtable {
    pub struct_size: u32,
    pub query_capabilities_json: VesperGetJson,
    pub open: VesperOpenSource,
    pub get_resource_json: Option<
        unsafe extern "C" fn(session: *mut VesperSession, out: *mut VesperBytes) -> VesperStatus,
    >,
    pub destroy: VesperDestroy,
    pub get_last_error: VesperGetLastError,
    /// Returns OK on change or TRY_AGAIN on timeout. The timeout is in milliseconds.
    pub wait_for_update:
        Option<unsafe extern "C" fn(session: *mut VesperSession, timeout_ms: u32) -> VesperStatus>,
    /// Requests cancellation. OK means the worker stopped, TRY_AGAIN means cancellation is pending.
    pub cancel: Option<unsafe extern "C" fn(session: *mut VesperSession) -> VesperStatus>,
}

/// The table and context are borrowed until the operation returns. Callbacks may
/// run on workers joined within the operation.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperProgressCallbacks {
    pub struct_size: u32,
    pub ctx: *mut c_void,
    pub progress: Option<unsafe extern "C" fn(ctx: *mut c_void, processed: u64, total: u64)>,
    pub is_cancelled: Option<unsafe extern "C" fn(ctx: *mut c_void) -> u8>,
}

pub type VesperDownloadOperation = Option<
    unsafe extern "C" fn(
        ctx: *mut c_void,
        request: VesperBytes,
        progress: *const VesperProgressCallbacks,
        out: *mut VesperBytes,
    ) -> VesperStatus,
>;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperPostDownloadProcessorVtable {
    pub struct_size: u32,
    /// Capabilities JSON is immutable for the root lifetime.
    pub capabilities_json: VesperGetJson,
    pub process_json: VesperDownloadOperation,
    pub assemble_json: VesperDownloadOperation,
    /// Releases a result returned by process_json or assemble_json.
    pub release_result: Option<unsafe extern "C" fn(ctx: *mut c_void, result: VesperBytes)>,
}

/// Successful entry returns a root valid until destroy_owner. Roots without
/// destroy_owner may be shared. Queries return immutable, root-owned tables.
/// Callers initialize output structs with their writable size. Writers may fill
/// only the ABI prefix they know. Failed opens leave the session output null.
/// Diagnostic slices and other borrowed JSON remain valid until the next call
/// to the same owning context. Unknown statuses are returned unchanged.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperPluginRoot {
    pub struct_size: u32,
    pub abi_major: u16,
    pub abi_minor: u16,
    pub owner: *mut c_void,
    pub plugin_id: VesperBytes,
    pub plugin_name: VesperBytes,
    pub interface_count: u32,
    pub interface_at: Option<
        unsafe extern "C" fn(
            owner: *mut c_void,
            index: u32,
            id: *mut VesperInterfaceId,
        ) -> VesperStatus,
    >,
    pub query_interface: Option<
        unsafe extern "C" fn(
            owner: *mut c_void,
            id: *const VesperInterfaceId,
            out_vtable: *mut *const c_void,
        ) -> VesperStatus,
    >,
    pub destroy_owner: Option<unsafe extern "C" fn(owner: *mut c_void)>,
}

unsafe extern "C" {
    pub fn vesper_native_plugin_entry() -> *const VesperPluginRoot;
}

#[doc(hidden)]
pub mod author;
#[doc(hidden)]
pub mod checked;
#[doc(hidden)]
pub mod export;

/// NUL-terminated symbol used for native plugin admission.
pub const VESPER_PLUGIN_ENTRY_SYMBOL: &std::ffi::CStr = c"vesper_native_plugin_entry";
/// Signature of a linked or dynamically resolved native plugin entry.
pub type VesperPluginEntryPoint = unsafe extern "C" fn() -> *const VesperPluginRoot;
