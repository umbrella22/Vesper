//! Private C layouts for the gain and Metal probe fixtures.

use std::ffi::c_void;

pub const VESPER_PROBE_OK: u32 = 0;
pub const VESPER_PROBE_NEED_INPUT: u32 = 1;
pub const VESPER_PROBE_TRY_AGAIN: u32 = 2;
pub const VESPER_PROBE_EOF: u32 = 3;
pub const VESPER_PROBE_INVALID: u32 = 100;
pub const VESPER_PROBE_INTERNAL: u32 = 102;
pub const VESPER_PROBE_F32_STEREO_FORMAT: u64 = 1;

#[repr(C)]
pub struct VesperProbeAudioBuffer {
    pub struct_size: u32,
    pub format_id: u64,
    pub pts_us: i64,
    pub rate_epoch: u64,
    pub flags: u32,
    pub frame_count: u32,
    pub frame_capacity: u32,
    pub planes: [*mut c_void; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VesperProbeGainApi {
    pub struct_size: u32,
    pub open: Option<unsafe extern "C" fn(gain: f32, out_session: *mut *mut c_void) -> u32>,
    pub process_block: Option<
        unsafe extern "C" fn(
            session: *mut c_void,
            input: *const VesperProbeAudioBuffer,
            output: *mut VesperProbeAudioBuffer,
        ) -> u32,
    >,
    pub push: Option<
        unsafe extern "C" fn(
            session: *mut c_void,
            input: *const VesperProbeAudioBuffer,
            consumed: *mut u32,
        ) -> u32,
    >,
    pub pull: Option<
        unsafe extern "C" fn(
            session: *mut c_void,
            output: *mut VesperProbeAudioBuffer,
            produced: *mut u32,
        ) -> u32,
    >,
    pub flush: Option<unsafe extern "C" fn(session: *mut c_void) -> u32>,
    pub destroy: Option<unsafe extern "C" fn(session: *mut c_void)>,
    pub set_playback_policy:
        Option<unsafe extern "C" fn(session: *mut c_void, rate: f32, epoch: u64) -> u32>,
    pub latency_frames: Option<unsafe extern "C" fn(session: *mut c_void) -> u32>,
}

pub const VESPER_PROBE_GPU_METAL: u32 = 1;
pub const VESPER_PROBE_VIDEO_NV12: u32 = 1;
pub const VESPER_PROBE_VIDEO_P010: u32 = 2;

#[repr(C)]
pub struct VesperProbeVideoFormat {
    pub struct_size: u32,
    pub pixel_format: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
pub struct VesperProbeGpuContext {
    pub struct_size: u32,
    pub backend: u32,
    pub device: *mut c_void,
    pub command_queue: *mut c_void,
    pub command_buffer: *mut c_void,
    pub texture_cache: *mut c_void,
}

#[repr(C)]
pub struct VesperProbeVideoFrame {
    pub struct_size: u32,
    pub width: u32,
    pub height: u32,
    pub pts_us: i64,
    pub pixel_buffer: u64,
    pub lease_id: u64,
    pub release_ctx: *mut c_void,
    pub release_fn: Option<unsafe extern "C" fn(context: *mut c_void, lease_id: u64)>,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct VesperProbeMetalStats {
    pub submitted: u32,
    pub released: u32,
    pub failed: u32,
    pub active: u32,
    pub retired: u32,
}
