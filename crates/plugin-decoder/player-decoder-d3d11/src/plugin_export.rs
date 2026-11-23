use crate::{PLUGIN_ID, PLUGIN_NAME, decoder_capabilities, decoder_native_requirements, platform};
use player_plugin::{
    DecoderBitstreamFormat, DecoderMediaKind, DecoderNativeDeviceContext, DecoderPacket,
    DecoderSessionConfig,
};
use player_plugin_abi::author::LeaseSlots;
use player_plugin_abi::checked::{bytes, read_bytes, read_sized, require_size, sized_pointer};
use player_plugin_abi::*;
use std::ffi::c_void;
use std::mem::size_of;
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

const MAX_EXTRADATA: usize = 1024 * 1024;
const MAX_PACKET: usize = 64 * 1024 * 1024;
const MAX_OPTIONS: usize = 4096;
type FrameLeases = Mutex<LeaseSlots<platform::OutstandingTexture>>;

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    width: Option<u32>,
    height: Option<u32>,
    coded_width: Option<u32>,
    coded_height: Option<u32>,
    bitstream_format: Option<DecoderBitstreamFormat>,
    #[serde(default)]
    share_textures: bool,
}

struct Session {
    decoder: Option<platform::SessionInner>,
    leases: Arc<FrameLeases>,
    format: VesperVideoFormat,
    pending: Option<(platform::NativeFrame, platform::OutstandingTexture)>,
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
    // SAFETY: the host exclusively accesses its live session handle.
    let session = unsafe { &mut *raw.cast::<Session>() };
    if session.poisoned {
        return VESPER_ERR_ABORTED;
    }
    let status = guard(|| call(session));
    session.poisoned |= status == VESPER_ERR_PANIC;
    status
}

unsafe extern "C" fn open(
    _owner: *mut c_void,
    gpu: *const VesperGpuContext,
    codec: u32,
    extra: VesperBytes,
    params: VesperBytes,
    out: *mut *mut VesperSession,
) -> VesperStatus {
    guard(|| {
        if !sized_pointer(out) {
            return VESPER_ERR_INVALID_ARG;
        }
        // SAFETY: out is the host's writable session result slot.
        unsafe { out.write(ptr::null_mut()) };
        if gpu.is_null() {
            return VESPER_ERR_UNSUPPORTED;
        }
        // SAFETY: the caller lends readable descriptors and slices for this call.
        let (Ok(gpu), Ok(extra), Ok(params)) = (unsafe {
            (
                read_sized(gpu),
                read_bytes(extra, MAX_EXTRADATA),
                read_bytes(params, MAX_OPTIONS),
            )
        }) else {
            return VESPER_ERR_INVALID_ARG;
        };
        if gpu.backend != VESPER_GPU_D3D11 {
            return VESPER_ERR_UNSUPPORTED;
        }
        // SAFETY: the D3D11 backend tag identifies the active union member.
        let device = unsafe { gpu.data.d3d11.device };
        if device.is_null() {
            return VESPER_ERR_INVALID_ARG;
        }
        let (codec, bitstream) = match &codec.to_be_bytes() {
            b"avc1" | b"avc3" => ("h264", DecoderBitstreamFormat::Avcc),
            b"hvc1" | b"hev1" => ("hevc", DecoderBitstreamFormat::Hvcc),
            _ => return VESPER_ERR_UNSUPPORTED,
        };
        let options: Options = if params.is_empty() {
            Options::default()
        } else if let Ok(options) = serde_json::from_slice(params) {
            options
        } else {
            return VESPER_ERR_INVALID_ARG;
        };
        let config = DecoderSessionConfig {
            codec: codec.to_owned(),
            media_kind: DecoderMediaKind::Video,
            bitstream_format: Some(options.bitstream_format.unwrap_or(bitstream)),
            width: options.width,
            height: options.height,
            coded_width: options.coded_width,
            coded_height: options.coded_height,
            extradata: extra.to_vec(),
            native_device_context: Some(DecoderNativeDeviceContext::D3D11Device {
                device_ptr: device as usize,
            }),
            ..Default::default()
        };
        let Ok(decoder) =
            platform::SessionInner::open_with_sharing(&config, options.share_textures)
        else {
            return VESPER_ERR_UNSUPPORTED;
        };
        let session = Box::new(Session {
            decoder: Some(decoder),
            leases: Arc::new(Mutex::new(LeaseSlots::default())),
            format: VesperVideoFormat::default(),
            pending: None,
            poisoned: false,
        });
        // SAFETY: the initialized session transfers once to the host.
        unsafe { out.write(Box::into_raw(session).cast()) };
        VESPER_OK
    })
}

unsafe extern "C" fn get_format(
    raw: *mut VesperSession,
    out: *mut VesperVideoFormat,
) -> VesperStatus {
    with_session(raw, |session| {
        // SAFETY: the host supplies a size-initialized output descriptor.
        if unsafe { require_size(out, size_of::<VesperVideoFormat>()) }.is_err() {
            return VESPER_ERR_INVALID_ARG;
        }
        if session.format.format_id == 0 {
            return VESPER_NEED_MORE_INPUT;
        }
        // SAFETY: the writable output prefix passed validation.
        unsafe { out.write(session.format) };
        VESPER_OK
    })
}

unsafe extern "C" fn send_packet(
    raw: *mut VesperSession,
    input: *const VesperPacket,
) -> VesperStatus {
    with_session(raw, |session| {
        // SAFETY: the host borrows a size-initialized descriptor for this call.
        let Ok(input) = (unsafe { read_sized(input) }) else {
            return VESPER_ERR_INVALID_ARG;
        };
        // SAFETY: the host lends packet storage for this call. MF copies the bytes.
        let Ok(payload) = (unsafe {
            read_bytes(
                VesperBytes {
                    data: input.data,
                    len: input.len,
                },
                MAX_PACKET,
            )
        }) else {
            return VESPER_ERR_INVALID_ARG;
        };
        if input.flags & VESPER_PACKET_CONFIG_CHANGED != 0 {
            return VESPER_ERR_UNSUPPORTED;
        }
        let Some(decoder) = &mut session.decoder else {
            return VESPER_ERR_ABORTED;
        };
        if input.flags & VESPER_PACKET_DISCONTINUITY != 0 {
            session.pending = None;
            if decoder.reset_decode_state().is_err() {
                return VESPER_ERR_INTERNAL;
            }
        }
        let result = if input.flags & VESPER_PACKET_EOS != 0 {
            decoder.send_end_of_stream()
        } else {
            decoder.send_packet(
                &DecoderPacket {
                    media_kind: DecoderMediaKind::Video,
                    pts_us: (input.pts_us != i64::MIN).then_some(input.pts_us),
                    dts_us: (input.dts_us != i64::MIN).then_some(input.dts_us),
                    duration_us: (input.duration_us != i64::MIN).then_some(input.duration_us),
                    key_frame: input.flags & VESPER_PACKET_KEYFRAME != 0,
                    ..Default::default()
                },
                payload,
            )
        };
        match result {
            Ok(result) if result.accepted => VESPER_OK,
            Ok(_) => VESPER_TRY_AGAIN,
            Err(_) => VESPER_ERR_INTERNAL,
        }
    })
}

unsafe extern "C" fn receive_frame(
    raw: *mut VesperSession,
    out: *mut VesperVideoFrame,
) -> VesperStatus {
    with_session(raw, |session| {
        // SAFETY: the host supplies a size-initialized output descriptor.
        if unsafe { require_size(out, size_of::<VesperVideoFrame>()) }.is_err() {
            return VESPER_ERR_INVALID_ARG;
        }
        if session
            .leases
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .is_full()
        {
            return VESPER_TRY_AGAIN;
        }
        let (frame, texture) = if let Some(pending) = session.pending.take() {
            pending
        } else {
            let Some(decoder) = &mut session.decoder else {
                return VESPER_ERR_ABORTED;
            };
            let frame = match decoder.receive_native_frame() {
                Ok(platform::ReceiveNativeFrame::Frame(frame)) => frame,
                Ok(platform::ReceiveNativeFrame::NeedMoreInput) => return VESPER_NEED_MORE_INPUT,
                Ok(platform::ReceiveNativeFrame::Eof) => return VESPER_EOF,
                Err(_) => return VESPER_ERR_INTERNAL,
            };
            let Some(texture) = decoder.take_frame_texture(frame.frame_id) else {
                return VESPER_ERR_INTERNAL;
            };
            (frame, texture)
        };
        let format = VesperVideoFormat {
            struct_size: size_of::<VesperVideoFormat>() as u32,
            pixel_format: VESPER_PIXEL_NV12,
            format_id: session.format.format_id,
            width: frame.width,
            height: frame.height,
            coded_width: frame.coded_width,
            coded_height: frame.coded_height,
            color: frame.color,
            ..Default::default()
        };
        if format != session.format {
            let Some(format_id) = format.format_id.checked_add(1) else {
                return VESPER_ERR_EXHAUSTED;
            };
            session.format = VesperVideoFormat {
                format_id,
                ..format
            };
            session.pending = Some((frame, texture));
            return VESPER_FORMAT_CHANGED;
        }
        let subresource_index = texture.subresource_index;
        let lease = session
            .leases
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(texture);
        let Ok(lease) = lease else {
            return VESPER_ERR_EXHAUSTED;
        };
        let frame = VesperVideoFrame {
            struct_size: size_of::<VesperVideoFrame>() as u32,
            format_id: format.format_id,
            pts_us: frame.pts_us.unwrap_or(i64::MIN),
            duration_us: frame.duration_us.unwrap_or(i64::MIN),
            handle_kind: VESPER_HANDLE_D3D11_TEXTURE2D,
            plane_count: 1,
            handles: [frame.handle as u64, u64::from(subresource_index), 0],
            lease_id: lease.get(),
            release_ctx: Arc::into_raw(Arc::clone(&session.leases)).cast_mut().cast(),
            release_fn: Some(release_frame),
            ..Default::default()
        };
        // SAFETY: the output is writable and transfers a live lease to the host.
        unsafe { out.write(frame) };
        VESPER_OK
    })
}

unsafe extern "C" fn release_frame(ctx: *mut c_void, id: u64, _disposition: u32) {
    guard(|| {
        let Some(id) = NonZeroU64::new(id) else {
            return VESPER_ERR_INVALID_ARG;
        };
        let ctx = ctx.cast::<FrameLeases>();
        if !sized_pointer(ctx) {
            return VESPER_ERR_INVALID_ARG;
        }
        // SAFETY: each outstanding frame keeps the lease pool alive independently of the decoder.
        let leases = unsafe { &*ctx };
        let released = leases
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .release(id);
        if released.is_none() {
            return VESPER_ERR_INVALID_ARG;
        }
        drop(released);
        // SAFETY: successful release consumes this frame's reference to the pool.
        unsafe { drop(Arc::from_raw(ctx)) };
        VESPER_OK
    });
}

unsafe extern "C" fn flush(raw: *mut VesperSession) -> VesperStatus {
    with_session(raw, |session| {
        session.pending = None;
        match session.decoder.as_mut().map(platform::SessionInner::flush) {
            Some(Ok(())) => VESPER_OK,
            _ => VESPER_ERR_INTERNAL,
        }
    })
}

unsafe extern "C" fn destroy(raw: *mut VesperSession) {
    guard(|| {
        if !raw.is_null() {
            // SAFETY: destroy consumes the host-owned session once.
            let mut session = unsafe { Box::from_raw(raw.cast::<Session>()) };
            if let Some(decoder) = session.decoder.take() {
                let _ = decoder.close();
            }
        }
        VESPER_OK
    });
}

unsafe extern "C" fn capabilities(_owner: *mut c_void, out: *mut VesperBytes) -> VesperStatus {
    static JSON: OnceLock<Result<Vec<u8>, serde_json::Error>> = OnceLock::new();
    guard(|| {
        if !sized_pointer(out) {
            return VESPER_ERR_INVALID_ARG;
        }
        // SAFETY: the host provides a writable result slot.
        unsafe { out.write(VesperBytes::default()) };
        let Ok(json) = JSON.get_or_init(|| {
            let mut capabilities = decoder_capabilities();
            capabilities.supports_presentation_release = true;
            serde_json::to_vec(&serde_json::json!({
                "decoder": capabilities,
                "nativeRequirements": decoder_native_requirements(),
            }))
        }) else {
            return VESPER_ERR_INTERNAL;
        };
        // SAFETY: data is immutable for the root's lifetime.
        unsafe { out.write(bytes(json)) };
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
    _owner: *mut c_void,
    index: u32,
    out: *mut VesperInterfaceId,
) -> VesperStatus {
    if index != 0 || !sized_pointer(out) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the caller provides a writable UUID slot.
    unsafe { out.write(VESPER_VIDEO_DECODER_ID) };
    VESPER_OK
}

unsafe extern "C" fn query(
    _owner: *mut c_void,
    id: *const VesperInterfaceId,
    out: *mut *const c_void,
) -> VesperStatus {
    if !sized_pointer(id) || !sized_pointer(out) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the caller supplies a readable UUID and writable pointer slot.
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
                data: PLUGIN_ID.as_ptr(),
                len: PLUGIN_ID.len() as u64,
            },
            plugin_name: VesperBytes {
                data: PLUGIN_NAME.as_ptr(),
                len: PLUGIN_NAME.len() as u64,
            },
            interface_count: 1,
            interface_at: Some(interface_at),
            query_interface: Some(query),
            destroy_owner: None,
        }
    }
}
