use std::cell::RefCell;
use std::ffi::{c_char, c_void};
use std::mem::size_of;
use std::num::NonZeroU64;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use player_plugin::DecoderNativeFrameMetadata;
use player_plugin_abi::author::LeaseSlots;
use player_plugin_abi::checked::{bytes, read_bytes, read_sized, require_size, sized_pointer};
use player_plugin_abi::*;

use crate::{
    DecoderBitstreamFormat, DecoderMediaKind, DecoderNativeDeviceContext, DecoderPacket,
    DecoderReceiveNativeFrameOutput, DecoderSessionConfig, MediaCodecDecoderSession, PLUGIN_ID,
    PLUGIN_NAME,
};

const AVC1: u32 = u32::from_be_bytes(*b"avc1");
const AVC3: u32 = u32::from_be_bytes(*b"avc3");
const HVC1: u32 = u32::from_be_bytes(*b"hvc1");
const HEV1: u32 = u32::from_be_bytes(*b"hev1");
const IMAGE_FORMAT_PRIVATE: i32 = 0x22;
const USAGE_GPU_SAMPLED_IMAGE: u64 = 1 << 8;
const IMGREADER_NO_BUFFER_AVAILABLE: i32 = -30001;
const IMGREADER_MAX_IMAGES_ACQUIRED: i32 = -30002;
const MAX_EXTRADATA_BYTES: usize = 1024 * 1024;
const MAX_PACKET_BYTES: usize = 64 * 1024 * 1024;
const MAX_OPTIONS_BYTES: usize = 4096;

thread_local! {
    static LAST_OPEN_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

struct StartError {
    status: VesperStatus,
    detail: String,
}

impl StartError {
    fn unsupported(detail: String) -> Self {
        Self {
            status: VESPER_ERR_UNSUPPORTED,
            detail,
        }
    }
}

#[link(name = "mediandk")]
unsafe extern "C" {
    fn AImageReader_newWithUsage(
        width: i32,
        height: i32,
        format: i32,
        usage: u64,
        max_images: i32,
        reader: *mut *mut c_void,
    ) -> i32;
    fn AImageReader_getWindow(reader: *mut c_void, window: *mut *mut c_void) -> i32;
    fn AImageReader_acquireNextImageAsync(
        reader: *mut c_void,
        image: *mut *mut c_void,
        acquire_fence: *mut i32,
    ) -> i32;
    fn AImageReader_delete(reader: *mut c_void);
    fn AImage_getHardwareBuffer(image: *const c_void, buffer: *mut *mut c_void) -> i32;
    fn AImage_deleteAsync(image: *mut c_void, release_fence: i32);
}

struct Reader {
    raw: NonNull<c_void>,
    capacity: usize,
    acquired: AtomicUsize,
}

// SAFETY: the session serializes reader operations. An image may be returned
// concurrently by the GPU completion thread. Arc prevents early reader deletion.
unsafe impl Send for Reader {}
// SAFETY: the acquired counter is atomic and only the session acquires images.
unsafe impl Sync for Reader {}

impl Reader {
    fn new(width: i32, height: i32, capacity: i32) -> Result<Arc<Self>, StartError> {
        let mut raw = ptr::null_mut();
        // SAFETY: raw is an output and dimensions were validated at session open.
        let status = unsafe {
            AImageReader_newWithUsage(
                width,
                height,
                IMAGE_FORMAT_PRIVATE,
                USAGE_GPU_SAMPLED_IMAGE,
                capacity,
                &mut raw,
            )
        };
        let reader = NonNull::new(raw).filter(|_| status == 0).ok_or_else(|| {
            StartError::unsupported(format!("AImageReader_newWithUsage returned {status}"))
        })?;
        Ok(Arc::new(Self {
            raw: reader,
            capacity: capacity.max(1) as usize,
            acquired: AtomicUsize::new(0),
        }))
    }

    fn window(&self) -> Result<usize, StartError> {
        let mut window = ptr::null_mut();
        // SAFETY: the reader owns the native window and out is writable.
        let status = unsafe { AImageReader_getWindow(self.raw.as_ptr(), &mut window) };
        if status != 0 || window.is_null() {
            return Err(StartError::unsupported(format!(
                "AImageReader_getWindow returned {status}"
            )));
        }
        Ok(window as usize)
    }

    fn acquire(self: &Arc<Self>) -> Result<Option<Image>, VesperStatus> {
        let mut raw = ptr::null_mut();
        let mut fence = -1;
        // SAFETY: the decoder session is the sole reader caller. Both outputs are writable.
        let status =
            unsafe { AImageReader_acquireNextImageAsync(self.raw.as_ptr(), &mut raw, &mut fence) };
        match status {
            0 => (),
            IMGREADER_NO_BUFFER_AVAILABLE | IMGREADER_MAX_IMAGES_ACQUIRED => return Ok(None),
            _ => return Err(VESPER_ERR_INTERNAL),
        }
        // SAFETY: a successful acquisition transfers ownership of any nonnegative fd.
        let fence = (fence >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fence) });
        let raw = NonNull::new(raw).ok_or(VESPER_ERR_INTERNAL)?;
        self.acquired.fetch_add(1, Ordering::Relaxed);
        Ok(Some(Image {
            raw,
            fence,
            reader: Arc::clone(self),
        }))
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: every acquired image holds a reader reference until it is returned.
        unsafe { AImageReader_delete(self.raw.as_ptr()) };
    }
}

struct Image {
    raw: NonNull<c_void>,
    fence: Option<OwnedFd>,
    reader: Arc<Reader>,
}

// SAFETY: the image has one owner and Android allows the consumer to return it
// from a completion thread while the reader continues to acquire other images.
unsafe impl Send for Image {}

impl Image {
    fn is_ready(&mut self) -> Result<bool, VesperStatus> {
        let Some(fence) = &self.fence else {
            return Ok(true);
        };
        let mut fd = libc::pollfd {
            fd: fence.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll reads and writes one live record. Zero timeout never blocks.
        let status = unsafe { libc::poll(&mut fd, 1, 0) };
        if status == 0
            || (status < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR))
        {
            return Ok(false);
        }
        if status < 0
            || fd.revents & (libc::POLLERR | libc::POLLNVAL) != 0
            || fd.revents & libc::POLLIN == 0
        {
            return Err(VESPER_ERR_INTERNAL);
        }
        self.fence.take();
        Ok(true)
    }

    fn buffer(&self) -> Result<u64, VesperStatus> {
        let mut buffer = ptr::null_mut();
        // SAFETY: the image remains leased while the returned handle is in use.
        let status = unsafe { AImage_getHardwareBuffer(self.raw.as_ptr(), &mut buffer) };
        if status != 0 || buffer.is_null() {
            return Err(VESPER_ERR_UNSUPPORTED);
        }
        Ok(buffer as u64)
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        // Forward an unsignalled acquire fence when discarding an unused image.
        // After a GPU submission, the host calls release only following retirement.
        let fence = self.fence.take().map_or(-1, IntoRawFd::into_raw_fd);
        // SAFETY: this consumes our one image and the optional owned fence fd.
        unsafe { AImage_deleteAsync(self.raw.as_ptr(), fence) };
        self.reader.acquired.fetch_sub(1, Ordering::Release);
    }
}

struct Decoder {
    codec: MediaCodecDecoderSession,
    reader: Arc<Reader>,
}

impl Decoder {
    fn open(config: &DecoderSessionConfig) -> Result<Self, StartError> {
        let dimensions = |value: Option<u32>| {
            value
                .and_then(|value| i32::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| StartError {
                    status: VESPER_ERR_INVALID_ARG,
                    detail: "MediaCodec requires positive video dimensions".to_owned(),
                })
        };
        let width = dimensions(config.width)?;
        let height = dimensions(config.height)?;
        match crate::open_with_capacity_backoff(&crate::IMAGE_READER_CAPACITIES, |capacity| {
            let reader = Reader::new(width, height, capacity)?;
            let mut config = config.clone();
            config.native_device_context = Some(DecoderNativeDeviceContext::AndroidNativeWindow {
                window_ptr: reader.window()?,
            });
            let codec = MediaCodecDecoderSession::open(config)
                .map_err(|error| StartError::unsupported(error.to_string()))?;
            Ok::<_, StartError>(Self { codec, reader })
        }) {
            Ok((decoder, _)) => Ok(decoder),
            Err((last_capacity, error)) => Err(StartError {
                status: error.status,
                detail: format!(
                    "{}; codec={}, decoder={}, AImageReader={}x{} format=PRIVATE usage=GPU_SAMPLED_IMAGE maxImages tried {:?} (last={last_capacity})",
                    error.detail,
                    config.codec,
                    config
                        .required_decoder_implementation_name
                        .as_deref()
                        .unwrap_or("(missing)"),
                    width,
                    height,
                    crate::IMAGE_READER_CAPACITIES,
                ),
            }),
        }
    }
}

type Images = Mutex<LeaseSlots<Image>>;

struct Session {
    decoder: Option<Decoder>,
    config: DecoderSessionConfig,
    pixel_format: u32,
    format: VesperVideoFormat,
    pending: Option<(DecoderNativeFrameMetadata, Option<Image>)>,
    images: Arc<Images>,
    poisoned: bool,
    last_error: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OpenOptions {
    width: u32,
    height: u32,
    hardware_decoder_name: String,
    pixel_format: u32,
}

fn guarded(call: impl FnOnce() -> VesperStatus) -> VesperStatus {
    catch_unwind(AssertUnwindSafe(call)).unwrap_or_else(|panic| {
        std::mem::forget(panic);
        VESPER_ERR_PANIC
    })
}

fn with_session(
    raw: *mut VesperSession,
    call: impl FnOnce(&mut Session) -> Result<VesperStatus, VesperStatus>,
) -> VesperStatus {
    if !sized_pointer(raw.cast::<Session>()) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the ABI host owns a live session and serializes its operations.
    let session = unsafe { &mut *raw.cast::<Session>() };
    if session.poisoned {
        return VESPER_ERR_ABORTED;
    }
    let status = guarded(|| call(session).unwrap_or_else(|status| status));
    if status == VESPER_ERR_PANIC {
        session.poisoned = true;
    }
    status
}

unsafe extern "C" fn open(
    _owner: *mut c_void,
    _gpu: *const VesperGpuContext,
    codec: u32,
    extradata: VesperBytes,
    params: VesperBytes,
    out: *mut *mut VesperSession,
) -> VesperStatus {
    guarded(|| {
        LAST_OPEN_ERROR.with(|detail| detail.borrow_mut().clear());
        if !sized_pointer(out) {
            return VESPER_ERR_INVALID_ARG;
        }
        // SAFETY: initialize the writable session output before validation.
        unsafe { out.write(ptr::null_mut()) };
        let (codec, bitstream) = match codec {
            AVC1 | AVC3 => ("h264", DecoderBitstreamFormat::Avcc),
            HVC1 | HEV1 => ("hevc", DecoderBitstreamFormat::Hvcc),
            _ => return VESPER_ERR_UNSUPPORTED,
        };
        // SAFETY: the host borrows both bounded byte spans through this call.
        let (Ok(extra), Ok(params)) = (
            unsafe { read_bytes(extradata, MAX_EXTRADATA_BYTES) },
            unsafe { read_bytes(params, MAX_OPTIONS_BYTES) },
        ) else {
            return VESPER_ERR_INVALID_ARG;
        };
        let Ok(opts) = serde_json::from_slice::<OpenOptions>(params) else {
            return VESPER_ERR_INVALID_ARG;
        };
        if !matches!(opts.pixel_format, VESPER_PIXEL_NV12 | VESPER_PIXEL_P010) {
            return VESPER_ERR_UNSUPPORTED;
        }
        let config = DecoderSessionConfig {
            codec: codec.to_owned(),
            media_kind: DecoderMediaKind::Video,
            bitstream_format: Some(bitstream),
            width: Some(opts.width),
            height: Some(opts.height),
            extradata: extra.to_vec(),
            prefer_hardware: true,
            required_decoder_implementation_name: Some(opts.hardware_decoder_name),
            ..Default::default()
        };
        let decoder = match Decoder::open(&config) {
            Ok(decoder) => decoder,
            Err(error) => {
                LAST_OPEN_ERROR.with(|detail| *detail.borrow_mut() = error.detail);
                return error.status;
            }
        };
        let session = Box::new(Session {
            decoder: Some(decoder),
            config,
            pixel_format: opts.pixel_format,
            format: VesperVideoFormat::default(),
            pending: None,
            images: Arc::new(Mutex::new(LeaseSlots::default())),
            poisoned: false,
            last_error: String::new(),
        });
        // SAFETY: transfer one new session to the caller's output.
        unsafe { out.write(Box::into_raw(session).cast()) };
        VESPER_OK
    })
}

unsafe extern "C" fn send_packet(
    raw: *mut VesperSession,
    packet: *const VesperPacket,
) -> VesperStatus {
    with_session(raw, |session| {
        // SAFETY: the host supplies a readable packet descriptor.
        let packet = unsafe { read_sized(packet) }.map_err(|_| VESPER_ERR_INVALID_ARG)?;
        if packet.flags & (VESPER_PACKET_CONFIG_CHANGED | VESPER_PACKET_DISCONTINUITY) != 0 {
            return Err(VESPER_ERR_UNSUPPORTED);
        }
        // SAFETY: packet payload is borrowed by the host for this call.
        let data = unsafe {
            read_bytes(
                VesperBytes {
                    data: packet.data,
                    len: packet.len,
                },
                MAX_PACKET_BYTES,
            )
        }
        .map_err(|_| VESPER_ERR_INVALID_ARG)?;
        let timestamp = |value| (value != i64::MIN).then_some(value);
        let metadata = DecoderPacket {
            pts_us: timestamp(packet.pts_us),
            dts_us: timestamp(packet.dts_us),
            duration_us: timestamp(packet.duration_us),
            stream_index: packet.stream_index,
            media_kind: DecoderMediaKind::Video,
            key_frame: packet.flags & VESPER_PACKET_KEYFRAME != 0,
            discontinuity: false,
            end_of_stream: packet.flags & VESPER_PACKET_EOS != 0,
        };
        let decoder = session.decoder.as_mut().ok_or(VESPER_ERR_ABORTED)?;
        let result = decoder
            .codec
            .send_packet_impl(&metadata, data)
            .map_err(|error| {
                tracing::debug!(%error, "MediaCodec AHardwareBuffer packet failed");
                VESPER_ERR_INTERNAL
            })?;
        Ok(if result.accepted {
            VESPER_OK
        } else {
            VESPER_TRY_AGAIN
        })
    })
}

fn video_format(
    frame: &DecoderNativeFrameMetadata,
    pixel_format: u32,
    format_id: u64,
) -> VesperVideoFormat {
    let color = frame.color.as_ref();
    VesperVideoFormat {
        struct_size: size_of::<VesperVideoFormat>() as u32,
        format_id,
        pixel_format,
        width: frame.width,
        height: frame.height,
        coded_width: frame.coded_width.unwrap_or(frame.width),
        coded_height: frame.coded_height.unwrap_or(frame.height),
        crop_x: frame.visible_rect.as_ref().map_or(0, |rect| rect.x),
        crop_y: frame.visible_rect.as_ref().map_or(0, |rect| rect.y),
        crop_w: frame
            .visible_rect
            .as_ref()
            .map_or(frame.width, |rect| rect.width),
        crop_h: frame
            .visible_rect
            .as_ref()
            .map_or(frame.height, |rect| rect.height),
        color: VesperColorInfo {
            primaries: match color.and_then(|value| value.primaries.as_deref()) {
                Some("bt709") => 1,
                Some("bt2020") => 9,
                _ => 2,
            },
            transfer: match color.and_then(|value| value.transfer.as_deref()) {
                Some("sdr-video") => 1,
                Some("linear") => 8,
                Some("st2084") => 16,
                Some("hlg") => 18,
                _ => 2,
            },
            matrix: match color.and_then(|value| value.matrix.as_deref()) {
                Some("bt709") => 1,
                Some("bt2020-ncl") => 9,
                Some("bt601") => 6,
                _ => 2,
            },
            range: match color.and_then(|value| value.range.as_deref()) {
                Some("limited") => 1,
                Some("full") => 2,
                _ => 0,
            },
            bit_depth: if pixel_format == VESPER_PIXEL_P010 {
                10
            } else {
                8
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

unsafe extern "C" fn receive(raw: *mut VesperSession, out: *mut VesperVideoFrame) -> VesperStatus {
    with_session(raw, |session| {
        // SAFETY: the host supplies a writable sized output descriptor.
        unsafe { require_size(out, size_of::<VesperVideoFrame>()) }
            .map_err(|_| VESPER_ERR_INVALID_ARG)?;
        // SAFETY: failures leave the output lease-free.
        unsafe {
            out.write(VesperVideoFrame {
                struct_size: size_of::<VesperVideoFrame>() as u32,
                ..Default::default()
            })
        };
        let decoder = session.decoder.as_mut().ok_or(VESPER_ERR_ABORTED)?;
        if session.pending.is_none() {
            if decoder.reader.acquired.load(Ordering::Acquire) >= decoder.reader.capacity {
                return Ok(VESPER_TRY_AGAIN);
            }
            let result = decoder.codec.receive_native_frame_impl().map_err(|error| {
                tracing::debug!(%error, "MediaCodec AHardwareBuffer receive failed");
                VESPER_ERR_INTERNAL
            })?;
            match result {
                DecoderReceiveNativeFrameOutput::Frame(frame) => {
                    let metadata = frame.metadata.clone();
                    decoder
                        .codec
                        .release_native_frame_impl(frame, true)
                        .map_err(|_| VESPER_ERR_INTERNAL)?;
                    session.pending = Some((metadata, None));
                }
                DecoderReceiveNativeFrameOutput::NeedMoreInput => {
                    return Ok(VESPER_NEED_MORE_INPUT);
                }
                DecoderReceiveNativeFrameOutput::Eof => return Ok(VESPER_EOF),
            }
        }
        let (metadata, image) = session.pending.as_mut().ok_or(VESPER_ERR_INTERNAL)?;
        if image.is_none() {
            *image = decoder.reader.acquire()?;
        }
        let Some(frame) = image else {
            return Ok(VESPER_TRY_AGAIN);
        };
        if !frame.is_ready()? {
            return Ok(VESPER_TRY_AGAIN);
        }
        let next = video_format(metadata, session.pixel_format, session.format.format_id);
        if next != session.format {
            session.format = next;
            session.format.format_id = next.format_id.checked_add(1).ok_or(VESPER_ERR_EXHAUSTED)?;
            return Ok(VESPER_FORMAT_CHANGED);
        }
        let buffer = frame.buffer()?;
        let pts_us = metadata.pts_us.unwrap_or(i64::MIN);
        let duration_us = metadata.duration_us.unwrap_or(i64::MIN);
        let image = image.take().ok_or(VESPER_ERR_INTERNAL)?;
        session.pending = None;
        let lease = session
            .images
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(image)
            .map_err(|_| VESPER_ERR_EXHAUSTED)?;
        // SAFETY: out has already been size-checked and the lease owns the reader image.
        unsafe {
            out.write(VesperVideoFrame {
                struct_size: size_of::<VesperVideoFrame>() as u32,
                format_id: session.format.format_id,
                pts_us,
                duration_us,
                handle_kind: VESPER_HANDLE_AHB,
                plane_count: 1,
                handles: [buffer, 0, 0],
                lease_id: lease.get(),
                release_fn: Some(release),
                release_ctx: Arc::into_raw(Arc::clone(&session.images)).cast_mut().cast(),
                ..Default::default()
            })
        };
        Ok(VESPER_OK)
    })
}

unsafe extern "C" fn release(ctx: *mut c_void, id: u64, _disposition: u32) {
    guarded(|| {
        let Some(id) = NonZeroU64::new(id) else {
            return VESPER_ERR_INVALID_ARG;
        };
        if !sized_pointer(ctx.cast::<Images>()) {
            return VESPER_ERR_INVALID_ARG;
        }
        // SAFETY: each admitted lease owns a strong reference to the shared lease slots.
        let images = unsafe { &*ctx.cast::<Images>() };
        let image = images
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .release(id);
        if image.is_none() {
            return VESPER_ERR_INVALID_ARG;
        }
        drop(image);
        // SAFETY: release consumes the matching slot and its one shared owner reference.
        unsafe { drop(Arc::from_raw(ctx.cast::<Images>())) };
        VESPER_OK
    });
}

unsafe extern "C" fn flush(raw: *mut VesperSession) -> VesperStatus {
    with_session(raw, |session| {
        session.pending.take();
        // A new reader isolates late images from the previous seek epoch.
        // Submitted frames keep their old reader alive until GPU completion.
        drop(session.decoder.take());
        session.last_error.clear();
        session.decoder = Some(Decoder::open(&session.config).map_err(|error| {
            session.last_error = error.detail;
            error.status
        })?);
        Ok(VESPER_OK)
    })
}

unsafe extern "C" fn get_format(
    raw: *mut VesperSession,
    out: *mut VesperVideoFormat,
) -> VesperStatus {
    with_session(raw, |session| {
        // SAFETY: the caller supplies a sized writable format descriptor.
        unsafe { require_size(out, size_of::<VesperVideoFormat>()) }
            .map_err(|_| VESPER_ERR_INVALID_ARG)?;
        if session.format.format_id == 0 {
            return Ok(VESPER_NEED_MORE_INPUT);
        }
        // SAFETY: out was size-checked above.
        unsafe { out.write(session.format) };
        Ok(VESPER_OK)
    })
}

unsafe extern "C" fn get_last_error(raw: *mut VesperSession, out_len: *mut u32) -> *const c_char {
    if !sized_pointer(out_len) {
        return ptr::null();
    }
    let span = |detail: &str| {
        if detail.is_empty() {
            (ptr::null(), 0)
        } else {
            (detail.as_ptr().cast(), detail.len() as u32)
        }
    };
    let (ptr, len) = if raw.is_null() {
        LAST_OPEN_ERROR
            .try_with(|detail| span(&detail.borrow()))
            .unwrap_or((ptr::null(), 0))
    } else if sized_pointer(raw.cast::<Session>()) {
        // SAFETY: the ABI borrows a live session exclusively until the next call.
        span(unsafe { &(*raw.cast::<Session>()).last_error })
    } else {
        (ptr::null(), 0)
    };
    // SAFETY: out_len is a caller-provided, aligned, writable length.
    unsafe { out_len.write(len) };
    ptr
}

unsafe extern "C" fn destroy(raw: *mut VesperSession) {
    guarded(|| {
        if !raw.is_null() {
            // SAFETY: destroy consumes the uniquely owned session from open.
            unsafe { drop(Box::from_raw(raw.cast::<Session>())) };
        }
        VESPER_OK
    });
}

unsafe extern "C" fn capabilities(_ctx: *mut c_void, out: *mut VesperBytes) -> VesperStatus {
    if !sized_pointer(out) {
        return VESPER_ERR_INVALID_ARG;
    }
    static JSON: std::sync::OnceLock<Result<Vec<u8>, serde_json::Error>> =
        std::sync::OnceLock::new();
    guarded(|| {
        // SAFETY: out is a writable descriptor, cleared even if serialization fails.
        unsafe { out.write(VesperBytes::default()) };
        let Ok(json) = JSON.get_or_init(|| {
            use player_plugin::{
                DecoderFrameFormat, DecoderNativeHandleKind, DecoderNativeRequirements,
                NativeFramePipelineProfile,
            };

            let mut decoder = crate::decoder_capabilities();
            decoder.supports_presentation_release = false;
            for codec in &mut decoder.codecs {
                codec.output_formats = vec![DecoderFrameFormat::Nv12];
            }
            let native_requirements = DecoderNativeRequirements {
                output_handle_kinds: vec![DecoderNativeHandleKind::MediaCodecHardwareBuffer],
                output_pipeline_profiles: vec![
                    NativeFramePipelineProfile::MediaCodecHardwareBuffer,
                ],
                accepted_bitstream_formats: vec![
                    DecoderBitstreamFormat::Avcc,
                    DecoderBitstreamFormat::Hvcc,
                ],
                ..Default::default()
            };
            serde_json::to_vec(&serde_json::json!({
                "decoder": decoder,
                "nativeRequirements": native_requirements,
                "protectedMedia": false,
            }))
        }) else {
            return VESPER_ERR_INTERNAL;
        };
        // SAFETY: the descriptor borrows immutable, process-lifetime JSON bytes.
        unsafe { out.write(bytes(json)) };
        VESPER_OK
    })
}

static TABLE: VesperVideoDecoderVtable = VesperVideoDecoderVtable {
    struct_size: size_of::<VesperVideoDecoderVtable>() as u32,
    query_capabilities_json: Some(capabilities),
    open: Some(open),
    get_format: Some(get_format),
    send_packet: Some(send_packet),
    receive_frame: Some(receive),
    flush: Some(flush),
    destroy: Some(destroy),
    get_last_error: Some(get_last_error),
};

unsafe extern "C" fn interface_at(
    _ctx: *mut c_void,
    index: u32,
    id: *mut VesperInterfaceId,
) -> VesperStatus {
    if index != 0 || !sized_pointer(id) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: id is a writable interface identifier slot.
    unsafe { id.write(VESPER_VIDEO_DECODER_ID) };
    VESPER_OK
}

unsafe extern "C" fn query(
    _ctx: *mut c_void,
    id: *const VesperInterfaceId,
    out: *mut *const c_void,
) -> VesperStatus {
    if !sized_pointer(id) || !sized_pointer(out) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the host provides a readable identifier and writable table slot.
    unsafe {
        out.write(ptr::null());
        if id.read() != VESPER_VIDEO_DECODER_ID {
            return VESPER_ERR_UNSUPPORTED;
        }
        out.write(ptr::from_ref(&TABLE).cast());
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
