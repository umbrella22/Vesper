use super::*;
use std::mem::size_of;
use std::num::NonZeroU64;

use player_plugin_abi::author::LeaseSlots;
use player_plugin_abi::checked::{read_bytes, read_sized, require_size, sized_pointer};
use player_plugin_abi::*;

const AVC1: u32 = u32::from_be_bytes(*b"avc1");
const AVC3: u32 = u32::from_be_bytes(*b"avc3");
const HVC1: u32 = u32::from_be_bytes(*b"hvc1");
const HEV1: u32 = u32::from_be_bytes(*b"hev1");
// HEVCDecoderConfigurationRecord packs bitDepthLumaMinus8 in byte 17.
const HVCC_LUMA_DEPTH_OFFSET: usize = 17;
const MAX_EXTRADATA: usize = 1024 * 1024;
const MAX_PACKET_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    reorder_depth: Option<u32>,
}

struct RetainedFrame(PendingNativeFrame);

impl Drop for RetainedFrame {
    fn drop(&mut self) {
        // SAFETY: one retained CoreVideo reference follows the frame out of the callback queue.
        unsafe { CFRelease(self.0.pixel_buffer.cast_const()) };
    }
}

type FrameLeases = Mutex<LeaseSlots<RetainedFrame>>;

struct Session {
    decoder: VideoToolboxDecoderSession,
    format: VesperVideoFormat,
    pending: Option<RetainedFrame>,
    leases: Arc<FrameLeases>,
    poisoned: bool,
}

fn guard(call: impl FnOnce() -> VesperStatus) -> VesperStatus {
    catch_unwind(AssertUnwindSafe(call)).unwrap_or_else(|payload| {
        std::mem::forget(payload);
        VESPER_ERR_PANIC
    })
}

fn with_session(
    raw: *mut VesperSession,
    call: impl FnOnce(&mut Session) -> VesperStatus,
) -> VesperStatus {
    if !sized_pointer(raw.cast::<Session>()) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the host owns a live handle from open and serializes all session calls.
    let session = unsafe { &mut *raw.cast::<Session>() };
    if session.poisoned {
        return VESPER_ERR_ABORTED;
    }
    let status = guard(|| call(session));
    if status == VESPER_ERR_PANIC {
        session.poisoned = true;
    }
    status
}

unsafe extern "C" fn open(
    _context: *mut c_void,
    _gpu: *const VesperGpuContext,
    fourcc: u32,
    extradata: VesperBytes,
    params: VesperBytes,
    out: *mut *mut VesperSession,
) -> VesperStatus {
    guard(|| {
        if !sized_pointer(out) {
            return VESPER_ERR_INVALID_ARG;
        }
        // SAFETY: out is an aligned, caller-owned result slot.
        unsafe { out.write(ptr::null_mut()) };
        // SAFETY: the host supplies readable borrowed initialization data.
        let (Ok(extra), Ok(params)) = (unsafe { read_bytes(extradata, MAX_EXTRADATA) }, unsafe {
            read_bytes(params, 4096)
        }) else {
            return VESPER_ERR_INVALID_ARG;
        };
        let (codec, codec_name, bitstream, format) = match fourcc {
            AVC1 | AVC3 => (
                VideoCodecKind::H264,
                "avc1",
                DecoderBitstreamFormat::Avcc,
                DecoderFrameFormat::Nv12,
            ),
            HVC1 | HEV1 => {
                let Some(depth) = extra.get(HVCC_LUMA_DEPTH_OFFSET).map(|byte| byte & 7) else {
                    return VESPER_ERR_INVALID_ARG;
                };
                let format = match depth {
                    0 => DecoderFrameFormat::Nv12,
                    2 => DecoderFrameFormat::P010,
                    _ => return VESPER_ERR_UNSUPPORTED,
                };
                (
                    VideoCodecKind::Hevc,
                    "hvc1",
                    DecoderBitstreamFormat::Hvcc,
                    format,
                )
            }
            _ => return VESPER_ERR_UNSUPPORTED,
        };
        if extra.first() != Some(&1) {
            return VESPER_ERR_INVALID_ARG;
        }
        let options = if params.is_empty() {
            Options::default()
        } else if let Ok(options) = serde_json::from_slice::<Options>(params) {
            options
        } else {
            return VESPER_ERR_INVALID_ARG;
        };
        let config = DecoderSessionConfig {
            codec: codec_name.to_owned(),
            media_kind: DecoderMediaKind::Video,
            extradata: extra.to_vec(),
            reorder_depth: options.reorder_depth,
            bitstream_format: Some(bitstream),
            ..Default::default()
        };
        let Ok(decoder) = VideoToolboxDecoderSession::new(config, codec, true, format, true) else {
            return VESPER_ERR_UNSUPPORTED;
        };
        let session = Box::new(Session {
            decoder,
            format: VesperVideoFormat::default(),
            pending: None,
            leases: Arc::new(Mutex::new(LeaseSlots::default())),
            poisoned: false,
        });
        // SAFETY: transfer the fresh session to this call's writable output.
        unsafe { out.write(Box::into_raw(session).cast()) };
        VESPER_OK
    })
}

unsafe extern "C" fn get_format(
    raw: *mut VesperSession,
    out: *mut VesperVideoFormat,
) -> VesperStatus {
    with_session(raw, |session| {
        // SAFETY: the host provides a sized writable format descriptor.
        if unsafe { require_size(out, size_of::<VesperVideoFormat>()) }.is_err() {
            return VESPER_ERR_INVALID_ARG;
        }
        if session.format.format_id == 0 {
            return VESPER_NEED_MORE_INPUT;
        }
        // SAFETY: the writable prefix is large enough for the current ABI format.
        unsafe { out.write(session.format) };
        VESPER_OK
    })
}

unsafe extern "C" fn send_packet(
    raw: *mut VesperSession,
    packet: *const VesperPacket,
) -> VesperStatus {
    with_session(raw, |session| {
        // SAFETY: the host supplies a readable descriptor and data for this call.
        let Ok(packet) = (unsafe { read_sized(packet) }) else {
            return VESPER_ERR_INVALID_ARG;
        };
        if packet.flags & VESPER_PACKET_CONFIG_CHANGED != 0 {
            return VESPER_ERR_UNSUPPORTED;
        }
        let eos = packet.flags & VESPER_PACKET_EOS != 0;
        let discontinuity = packet.flags & VESPER_PACKET_DISCONTINUITY != 0;
        if session.pending.is_some() && !discontinuity {
            return VESPER_TRY_AGAIN;
        }
        // SAFETY: the packet byte view borrows data only inside this call.
        let Ok(data) = (unsafe {
            read_bytes(
                VesperBytes {
                    data: packet.data,
                    len: packet.len,
                },
                MAX_PACKET_BYTES,
            )
        }) else {
            return VESPER_ERR_INVALID_ARG;
        };
        if eos != data.is_empty() {
            return VESPER_ERR_INVALID_ARG;
        }
        if discontinuity {
            session.pending = None;
        }
        let time = |ts| (ts != i64::MIN).then_some(ts);
        let packet = DecoderPacket {
            pts_us: time(packet.pts_us),
            dts_us: time(packet.dts_us),
            duration_us: time(packet.duration_us),
            stream_index: packet.stream_index,
            media_kind: DecoderMediaKind::Video,
            key_frame: packet.flags & VESPER_PACKET_KEYFRAME != 0,
            discontinuity,
            end_of_stream: eos,
        };
        match session.decoder.send_packet(&packet, data) {
            Ok(result) if result.accepted => VESPER_OK,
            Ok(_) | Err(DecoderError::InvalidPacket { .. }) => VESPER_ERR_INVALID_ARG,
            Err(_) => VESPER_ERR_INTERNAL,
        }
    })
}

unsafe extern "C" fn receive_frame(
    raw: *mut VesperSession,
    out: *mut VesperVideoFrame,
) -> VesperStatus {
    with_session(raw, |session| {
        // SAFETY: the host provides a sized writable frame descriptor.
        if unsafe { require_size(out, size_of::<VesperVideoFrame>()) }.is_err() {
            return VESPER_ERR_INVALID_ARG;
        }
        // SAFETY: make the failed/backpressured result lease-free.
        unsafe {
            out.write(VesperVideoFrame {
                struct_size: size_of::<VesperVideoFrame>() as u32,
                ..Default::default()
            })
        };
        if session
            .leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_full()
        {
            return VESPER_TRY_AGAIN;
        }
        let frame = match session
            .pending
            .take()
            .or_else(|| session.decoder.take_ready_frame().map(RetainedFrame))
        {
            Some(frame) => frame,
            None if session.decoder.end_of_stream_sent => return VESPER_EOF,
            None => return VESPER_NEED_MORE_INPUT,
        };
        let borrowed = &frame.0;
        // SAFETY: the frame owns a retained CVPixelBuffer throughout describe and insertion.
        let pixel_format = unsafe { CVPixelBufferGetPixelFormatType(borrowed.pixel_buffer) };
        let (pixel_format, range, bit_depth) = match pixel_format {
            value if value == K_CV_PIXEL_FORMAT_TYPE_420YPCBCR8_BIPLANAR_VIDEO_RANGE as u32 => {
                (VESPER_PIXEL_NV12, 1, 8)
            }
            K_CV_PIXEL_FORMAT_TYPE_420YPCBCR8_BIPLANAR_FULL_RANGE => (VESPER_PIXEL_NV12, 2, 8),
            value if value == K_CV_PIXEL_FORMAT_TYPE_420YPCBCR10_BIPLANAR_VIDEO_RANGE as u32 => {
                (VESPER_PIXEL_P010, 1, 10)
            }
            K_CV_PIXEL_FORMAT_TYPE_420YPCBCR10_BIPLANAR_FULL_RANGE => (VESPER_PIXEL_P010, 2, 10),
            _ => return VESPER_ERR_UNSUPPORTED,
        };
        let color_code = |key, code_point: unsafe extern "C" fn(CFStringRef) -> i32| {
            // SAFETY: the frame retains the pixel buffer through the attachment read.
            unsafe {
                let value = CVBufferGetAttachment(borrowed.pixel_buffer, key, ptr::null_mut());
                if value.is_null() || CFGetTypeID(value) != CFStringGetTypeID() {
                    2
                } else {
                    u8::try_from(code_point(value)).unwrap_or(2)
                }
            }
        };
        // SAFETY: these three exported keys are immutable CoreVideo strings.
        let (primaries, transfer, matrix) = unsafe {
            (
                color_code(
                    kCVImageBufferColorPrimariesKey,
                    CVColorPrimariesGetIntegerCodePointForString,
                ),
                color_code(
                    kCVImageBufferTransferFunctionKey,
                    CVTransferFunctionGetIntegerCodePointForString,
                ),
                color_code(
                    kCVImageBufferYCbCrMatrixKey,
                    CVYCbCrMatrixGetIntegerCodePointForString,
                ),
            )
        };
        let format = VesperVideoFormat {
            struct_size: size_of::<VesperVideoFormat>() as u32,
            pixel_format,
            format_id: session.format.format_id,
            width: borrowed.width,
            height: borrowed.height,
            coded_width: borrowed.width,
            coded_height: borrowed.height,
            crop_w: borrowed.width,
            crop_h: borrowed.height,
            // CoreVideo carries the authoritative per-frame CICP attachments.
            color: VesperColorInfo {
                primaries,
                transfer,
                matrix,
                range,
                bit_depth,
                hdr_kind: match transfer {
                    16 => VESPER_HDR_HDR10,
                    18 => VESPER_HDR_HLG,
                    _ => VESPER_HDR_SDR,
                },
                ..Default::default()
            },
            ..Default::default()
        };
        if format != session.format {
            let Some(next_id) = format.format_id.checked_add(1) else {
                return VESPER_ERR_EXHAUSTED;
            };
            session.format = VesperVideoFormat {
                format_id: next_id,
                ..format
            };
            session.pending = Some(frame);
            return VESPER_FORMAT_CHANGED;
        }
        let mut output = VesperVideoFrame {
            struct_size: size_of::<VesperVideoFrame>() as u32,
            format_id: session.format.format_id,
            pts_us: borrowed.pts_us.unwrap_or(i64::MIN),
            duration_us: borrowed.duration_us.unwrap_or(i64::MIN),
            handle_kind: VESPER_HANDLE_CV_PIXEL_BUFFER,
            plane_count: 1,
            handles: [borrowed.pixel_buffer as u64, 0, 0],
            ..Default::default()
        };
        let id = session
            .leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(frame);
        let Ok(id) = id else {
            return VESPER_ERR_EXHAUSTED;
        };
        output.lease_id = id.get();
        output.release_ctx = Arc::into_raw(Arc::clone(&session.leases)).cast_mut().cast();
        output.release_fn = Some(release_frame);
        // SAFETY: the output is writable, sized and owns the inserted lease now.
        unsafe { out.write(output) };
        VESPER_OK
    })
}

unsafe extern "C" fn release_frame(ctx: *mut c_void, id: u64, _disposition: u32) {
    let _status = guard(|| {
        let Some(id) = NonZeroU64::new(id) else {
            return VESPER_ERR_INVALID_ARG;
        };
        if !sized_pointer(ctx.cast::<FrameLeases>()) {
            return VESPER_ERR_INVALID_ARG;
        }
        // SAFETY: each outstanding lease holds a strong reference to this pool.
        let leases = unsafe { &*ctx.cast::<FrameLeases>() };
        let frame = leases.lock().unwrap_or_else(|e| e.into_inner()).release(id);
        if frame.is_none() {
            return VESPER_ERR_INVALID_ARG;
        }
        drop(frame);
        // SAFETY: the successful slot release consumes exactly the matching strong reference.
        unsafe { drop(Arc::from_raw(ctx.cast::<FrameLeases>())) };
        VESPER_OK
    });
}

unsafe extern "C" fn flush(raw: *mut VesperSession) -> VesperStatus {
    with_session(raw, |session| {
        session.pending = None;
        match session.decoder.flush() {
            Ok(()) => VESPER_OK,
            Err(_) => VESPER_ERR_INTERNAL,
        }
    })
}

unsafe extern "C" fn destroy(raw: *mut VesperSession) {
    let _status = guard(|| {
        if !raw.is_null() {
            // SAFETY: a destroy call consumes the handle transferred by open once.
            unsafe { drop(Box::from_raw(raw.cast::<Session>())) };
        }
        VESPER_OK
    });
}

unsafe extern "C" fn capabilities(_context: *mut c_void, out: *mut VesperBytes) -> VesperStatus {
    if !sized_pointer(out) {
        return VESPER_ERR_INVALID_ARG;
    }
    static JSON: std::sync::OnceLock<Result<Vec<u8>, serde_json::Error>> =
        std::sync::OnceLock::new();
    guard(|| {
        // SAFETY: initialize the writable output even on failure.
        unsafe { out.write(VesperBytes::default()) };
        let Ok(json) = JSON.get_or_init(|| {
            let mut decoder = crate::decoder_capabilities();
            decoder
                .codecs
                .retain(|codec| !matches!(codec.codec.as_str(), "DVH1" | "DVHE"));
            for codec in &mut decoder.codecs {
                if matches!(codec.codec.as_str(), "HEVC" | "H265" | "HVC1" | "HEV1") {
                    codec.output_formats.push(DecoderFrameFormat::P010);
                }
            }
            serde_json::to_vec(&serde_json::json!({
                "decoder": decoder,
                "nativeRequirements": crate::decoder_native_requirements(),
            }))
        }) else {
            return VESPER_ERR_INTERNAL;
        };
        // SAFETY: out is writable and the immutable JSON is resident.
        unsafe { out.write(checked::bytes(json)) };
        VESPER_OK
    })
}

static DECODER: VesperVideoDecoderVtable = VesperVideoDecoderVtable {
    struct_size: size_of::<VesperVideoDecoderVtable>() as u32,
    query_capabilities_json: Some(capabilities),
    open: Some(open),
    get_format: Some(get_format),
    send_packet: Some(send_packet),
    receive_frame: Some(receive_frame),
    flush: Some(flush),
    destroy: Some(destroy),
    get_last_error: None,
};

unsafe extern "C" fn interface_at(
    _context: *mut c_void,
    index: u32,
    id: *mut VesperInterfaceId,
) -> VesperStatus {
    if index != 0 || !sized_pointer(id) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: id is a writable POD output slot.
    unsafe { id.write(VESPER_VIDEO_DECODER_ID) };
    VESPER_OK
}

unsafe extern "C" fn query(
    _context: *mut c_void,
    id: *const VesperInterfaceId,
    out: *mut *const c_void,
) -> VesperStatus {
    if !sized_pointer(id) || !sized_pointer(out) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the host provides a readable UUID and a writable output slot.
    unsafe {
        out.write(ptr::null());
        if id.read() != VESPER_VIDEO_DECODER_ID {
            return VESPER_ERR_UNSUPPORTED;
        }
        out.write(ptr::from_ref(&DECODER).cast());
    }
    VESPER_OK
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_native_plugin_entry() -> *const VesperPluginRoot {
    &const {
        VesperPluginRoot {
            struct_size: size_of::<VesperPluginRoot>() as u32,
            abi_major: VESPER_PLUGIN_ABI_MAJOR,
            abi_minor: VESPER_PLUGIN_ABI_MINOR,
            owner: ptr::null_mut(),
            plugin_id: VesperBytes {
                data: crate::PLUGIN_ID.as_ptr(),
                len: crate::PLUGIN_ID.len() as u64,
            },
            plugin_name: VesperBytes {
                data: crate::PLUGIN_NAME.as_ptr(),
                len: crate::PLUGIN_NAME.len() as u64,
            },
            interface_count: 1,
            interface_at: Some(interface_at),
            query_interface: Some(query),
            destroy_owner: None,
        }
    }
}
