use std::ffi::c_void;
use std::mem::{offset_of, size_of};

use crate::{VesperByteSlice, VesperInterfaceHeader, VesperOwnedBytes, VesperStatus, abi_size};

pub type VesperSessionId = u64;
pub type VesperLeaseId = u64;

pub const VESPER_INTERFACE_MAJOR: u16 = 1;
pub const VESPER_INTERFACE_MINOR: u16 = 0;

pub const VESPER_RELEASE_DISCARDED: u32 = 0;
pub const VESPER_RELEASE_PRESENTED: u32 = 1;

/// Host callbacks borrowed for one synchronous processing call.
///
/// The host must keep `context` alive until the processing call returns and
/// must make both callbacks safe for concurrent invocation from scoped plugin
/// worker threads during that call.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperProgressCallbacks {
    pub struct_size: u32,
    pub reserved: u32,
    pub context: *mut c_void,
    pub on_progress: Option<unsafe extern "C" fn(context: *mut c_void, ratio: f64) -> VesperStatus>,
    pub is_cancelled:
        Option<unsafe extern "C" fn(context: *mut c_void, out_cancelled: *mut u32) -> VesperStatus>,
}

impl Default for VesperProgressCallbacks {
    fn default() -> Self {
        Self {
            struct_size: abi_size::<Self>(),
            reserved: 0,
            context: std::ptr::null_mut(),
            on_progress: None,
            is_cancelled: None,
        }
    }
}

/// Host-initialized output carrying one plugin-owned JSON document.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperJsonOut {
    pub struct_size: u32,
    pub reserved: u32,
    pub payload: VesperOwnedBytes,
}

impl Default for VesperJsonOut {
    fn default() -> Self {
        Self {
            struct_size: abi_size::<Self>(),
            reserved: 0,
            payload: VesperOwnedBytes::empty(),
        }
    }
}

/// Host-initialized output for a session open operation.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperOpenSessionOut {
    pub struct_size: u32,
    pub reserved: u32,
    pub session_id: VesperSessionId,
    pub payload: VesperOwnedBytes,
}

impl Default for VesperOpenSessionOut {
    fn default() -> Self {
        Self {
            struct_size: abi_size::<Self>(),
            reserved: 0,
            session_id: 0,
            payload: VesperOwnedBytes::empty(),
        }
    }
}

/// Host-initialized output for a native frame receive operation.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperNativeFrameOut {
    pub struct_size: u32,
    pub requires_release: u32,
    pub metadata: VesperOwnedBytes,
    /// Same-process native handle payload. This is never the release identity.
    pub native_handle: u64,
    pub lease_id: VesperLeaseId,
}

impl Default for VesperNativeFrameOut {
    fn default() -> Self {
        Self {
            struct_size: abi_size::<Self>(),
            requires_release: 0,
            metadata: VesperOwnedBytes::empty(),
            native_handle: 0,
            lease_id: 0,
        }
    }
}

/// Host-initialized output for decoded PCM bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperPcmFrameOut {
    pub struct_size: u32,
    pub reserved: u32,
    pub metadata: VesperOwnedBytes,
    pub data: VesperOwnedBytes,
}

impl Default for VesperPcmFrameOut {
    fn default() -> Self {
        Self {
            struct_size: abi_size::<Self>(),
            reserved: 0,
            metadata: VesperOwnedBytes::empty(),
            data: VesperOwnedBytes::empty(),
        }
    }
}

/// Host-initialized output for a borrowed packet lease.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperPacketOut {
    pub struct_size: u32,
    pub reserved: u32,
    pub metadata: VesperOwnedBytes,
    pub data: VesperByteSlice,
    pub lease_id: VesperLeaseId,
}

impl Default for VesperPacketOut {
    fn default() -> Self {
        Self {
            struct_size: abi_size::<Self>(),
            reserved: 0,
            metadata: VesperOwnedBytes::empty(),
            data: VesperByteSlice::empty(),
            lease_id: 0,
        }
    }
}

pub type VesperGetJsonFn =
    unsafe extern "C" fn(context: *mut c_void, out: *mut VesperJsonOut) -> VesperStatus;
pub type VesperJsonCallFn = unsafe extern "C" fn(
    context: *mut c_void,
    input_json: VesperByteSlice,
    out: *mut VesperJsonOut,
) -> VesperStatus;
pub type VesperOpenSessionFn = unsafe extern "C" fn(
    context: *mut c_void,
    config_json: VesperByteSlice,
    out: *mut VesperOpenSessionOut,
) -> VesperStatus;
pub type VesperSessionOperationFn = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperJsonOut,
) -> VesperStatus;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperPostDownloadProcessor {
    pub header: VesperInterfaceHeader,
    pub capabilities_json: Option<VesperGetJsonFn>,
    pub process_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            input_json: VesperByteSlice,
            output_path: VesperByteSlice,
            progress: *const VesperProgressCallbacks,
            out: *mut VesperJsonOut,
        ) -> VesperStatus,
    >,
    pub assemble_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            input_json: VesperByteSlice,
            output_path: VesperByteSlice,
            progress: *const VesperProgressCallbacks,
            out: *mut VesperJsonOut,
        ) -> VesperStatus,
    >,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperPipelineEventHook {
    pub header: VesperInterfaceHeader,
    pub on_event_json: Option<VesperJsonCallFn>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperBenchmarkSink {
    pub header: VesperInterfaceHeader,
    pub on_event_batch_json: Option<VesperJsonCallFn>,
    /// Optional append-only tail. Absence maps to an empty report.
    pub flush_json: Option<VesperGetJsonFn>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperNativeDecoder {
    pub header: VesperInterfaceHeader,
    pub capabilities_json: Option<VesperGetJsonFn>,
    pub native_requirements_json: Option<VesperGetJsonFn>,
    pub open_session_json: Option<VesperOpenSessionFn>,
    pub send_packet: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session_id: VesperSessionId,
            packet_json: VesperByteSlice,
            packet_data: VesperByteSlice,
            out: *mut VesperJsonOut,
        ) -> VesperStatus,
    >,
    pub receive_native_frame: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session_id: VesperSessionId,
            out: *mut VesperNativeFrameOut,
        ) -> VesperStatus,
    >,
    pub release_native_frame: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session_id: VesperSessionId,
            lease_id: VesperLeaseId,
            disposition: u32,
            out: *mut VesperJsonOut,
        ) -> VesperStatus,
    >,
    pub flush_session: Option<VesperSessionOperationFn>,
    pub close_session: Option<VesperSessionOperationFn>,
    /// Optional append-only tail enabled by decoder capabilities.
    pub receive_pcm_frame: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session_id: VesperSessionId,
            out: *mut VesperPcmFrameOut,
        ) -> VesperStatus,
    >,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperFrameProcessor {
    pub header: VesperInterfaceHeader,
    pub capabilities_json: Option<VesperGetJsonFn>,
    pub open_session_json: Option<VesperOpenSessionFn>,
    pub submit_frame_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session_id: VesperSessionId,
            submit_json: VesperByteSlice,
            native_handle: u64,
            out: *mut VesperJsonOut,
        ) -> VesperStatus,
    >,
    pub receive_frame: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session_id: VesperSessionId,
            out: *mut VesperNativeFrameOut,
        ) -> VesperStatus,
    >,
    pub release_frame: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session_id: VesperSessionId,
            lease_id: VesperLeaseId,
            out: *mut VesperJsonOut,
        ) -> VesperStatus,
    >,
    pub flush_session: Option<VesperSessionOperationFn>,
    pub close_session: Option<VesperSessionOperationFn>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperSourceNormalizerPacket {
    pub header: VesperInterfaceHeader,
    pub capabilities_json: Option<VesperGetJsonFn>,
    pub open_session_json: Option<VesperOpenSessionFn>,
    pub read_packet: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session_id: VesperSessionId,
            out: *mut VesperPacketOut,
        ) -> VesperStatus,
    >,
    pub release_packet: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session_id: VesperSessionId,
            lease_id: VesperLeaseId,
            out: *mut VesperJsonOut,
        ) -> VesperStatus,
    >,
    pub flush_session: Option<VesperSessionOperationFn>,
    pub close_session: Option<VesperSessionOperationFn>,
    /// Optional append-only tail for seekable packet sources.
    pub seek_session_json: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session_id: VesperSessionId,
            seek_json: VesperByteSlice,
            out: *mut VesperJsonOut,
        ) -> VesperStatus,
    >,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperSourceNormalizerResource {
    pub header: VesperInterfaceHeader,
    pub capabilities_json: Option<VesperGetJsonFn>,
    pub open_session_json: Option<VesperOpenSessionFn>,
    pub poll_session: Option<VesperSessionOperationFn>,
    pub wait_session_update: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            session_id: VesperSessionId,
            timeout_ms: u64,
            out: *mut VesperJsonOut,
        ) -> VesperStatus,
    >,
    pub cancel_session: Option<VesperSessionOperationFn>,
    pub close_session: Option<VesperSessionOperationFn>,
}

pub const VESPER_POST_DOWNLOAD_PROCESSOR_REQUIRED_SIZE: u32 =
    size_of::<VesperPostDownloadProcessor>() as u32;
pub const VESPER_PIPELINE_EVENT_HOOK_REQUIRED_SIZE: u32 =
    size_of::<VesperPipelineEventHook>() as u32;
pub const VESPER_BENCHMARK_SINK_REQUIRED_SIZE: u32 =
    offset_of!(VesperBenchmarkSink, flush_json) as u32;
pub const VESPER_NATIVE_DECODER_REQUIRED_SIZE: u32 =
    offset_of!(VesperNativeDecoder, receive_pcm_frame) as u32;
pub const VESPER_FRAME_PROCESSOR_REQUIRED_SIZE: u32 = size_of::<VesperFrameProcessor>() as u32;
pub const VESPER_SOURCE_NORMALIZER_PACKET_REQUIRED_SIZE: u32 =
    offset_of!(VesperSourceNormalizerPacket, seek_session_json) as u32;
pub const VESPER_SOURCE_NORMALIZER_RESOURCE_REQUIRED_SIZE: u32 =
    size_of::<VesperSourceNormalizerResource>() as u32;

pub const fn abi_contains(struct_size: u32, field_offset: u32, field_size: u32) -> bool {
    match field_offset.checked_add(field_size) {
        Some(required) => struct_size >= required,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use std::mem::{offset_of, size_of};

    use super::*;

    #[test]
    fn wire_ids_and_tokens_have_fixed_widths() {
        assert_eq!(size_of::<crate::VesperInterfaceId>(), 16);
        assert_eq!(size_of::<VesperSessionId>(), 8);
        assert_eq!(size_of::<VesperLeaseId>(), 8);
        assert_eq!(offset_of!(VesperInterfaceHeader, struct_size), 0);
        assert_eq!(offset_of!(VesperInterfaceHeader, interface_id), 4);
        assert_eq!(offset_of!(VesperInterfaceHeader, major), 20);
        assert_eq!(offset_of!(VesperInterfaceHeader, minor), 22);
    }

    #[test]
    fn every_typed_table_starts_with_the_common_header() {
        assert_eq!(offset_of!(VesperPostDownloadProcessor, header), 0);
        assert_eq!(offset_of!(VesperPipelineEventHook, header), 0);
        assert_eq!(offset_of!(VesperBenchmarkSink, header), 0);
        assert_eq!(offset_of!(VesperNativeDecoder, header), 0);
        assert_eq!(offset_of!(VesperFrameProcessor, header), 0);
        assert_eq!(offset_of!(VesperSourceNormalizerPacket, header), 0);
        assert_eq!(offset_of!(VesperSourceNormalizerResource, header), 0);
    }

    #[test]
    fn required_prefixes_reject_truncation_and_accept_extensions() {
        let required = [
            VESPER_POST_DOWNLOAD_PROCESSOR_REQUIRED_SIZE,
            VESPER_PIPELINE_EVENT_HOOK_REQUIRED_SIZE,
            VESPER_BENCHMARK_SINK_REQUIRED_SIZE,
            VESPER_NATIVE_DECODER_REQUIRED_SIZE,
            VESPER_FRAME_PROCESSOR_REQUIRED_SIZE,
            VESPER_SOURCE_NORMALIZER_PACKET_REQUIRED_SIZE,
            VESPER_SOURCE_NORMALIZER_RESOURCE_REQUIRED_SIZE,
        ];
        for size in required {
            assert!(!abi_contains(size - 1, 0, size));
            assert!(abi_contains(size, 0, size));
            assert!(abi_contains(size + 64, 0, size));
        }
    }

    #[test]
    fn optional_tails_are_excluded_from_required_prefixes() {
        assert_eq!(
            VESPER_BENCHMARK_SINK_REQUIRED_SIZE,
            offset_of!(VesperBenchmarkSink, flush_json) as u32
        );
        assert_eq!(
            VESPER_NATIVE_DECODER_REQUIRED_SIZE,
            offset_of!(VesperNativeDecoder, receive_pcm_frame) as u32
        );
        assert_eq!(
            VESPER_SOURCE_NORMALIZER_PACKET_REQUIRED_SIZE,
            offset_of!(VesperSourceNormalizerPacket, seek_session_json) as u32
        );
    }

    #[test]
    fn host_initialized_outputs_zero_all_owned_resources() {
        let json = VesperJsonOut::default();
        let open = VesperOpenSessionOut::default();
        let frame = VesperNativeFrameOut::default();
        let pcm = VesperPcmFrameOut::default();
        let packet = VesperPacketOut::default();

        assert_eq!(json.payload, VesperOwnedBytes::empty());
        assert_eq!(open.session_id, 0);
        assert_eq!(open.payload, VesperOwnedBytes::empty());
        assert_eq!(frame.native_handle, 0);
        assert_eq!(frame.lease_id, 0);
        assert_eq!(frame.requires_release, 0);
        assert_eq!(pcm.metadata, VesperOwnedBytes::empty());
        assert_eq!(pcm.data, VesperOwnedBytes::empty());
        assert_eq!(packet.data, VesperByteSlice::empty());
        assert_eq!(packet.lease_id, 0);
    }
}
