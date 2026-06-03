#![warn(clippy::undocumented_unsafe_blocks)]

use std::borrow::Cow;
use std::ffi::{c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

use player_plugin::{
    DecoderBitstreamFormat, DecoderCapabilities, DecoderCodecCapability, DecoderError,
    DecoderFrameFormat, DecoderMediaKind, DecoderNativeDeviceContext,
    DecoderNativeDeviceContextKind, DecoderNativeHandleKind, DecoderNativeRequirements,
    DecoderOperationStatus, DecoderPacket, DecoderReceiveNativeFrameMetadata, DecoderSessionConfig,
    DecoderSessionInfo, NativeFramePipelineProfile, VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT,
    VesperDecoderOpenSessionResult, VesperDecoderPluginApiV5,
    VesperDecoderReceiveNativeFrameResult, VesperDecoderReceivePcmFrameResult, VesperPluginBytes,
    VesperPluginDescriptor, VesperPluginKind, VesperPluginProcessResult, VesperPluginResultStatus,
};

static PLUGIN_NAME: &[u8] = b"player-decoder-mediacodec\0";
const MEDIACODEC_SUPPORTED: bool = cfg!(target_os = "android");
const MEDIACODEC_SURFACE_TEXTURE_FORMAT: &str = "mediacodec_surface_texture";

struct PluginBundle {
    api: VesperDecoderPluginApiV5,
    descriptor: VesperPluginDescriptor,
}

#[derive(Debug)]
struct MediaCodecDecoderSession {
    codec: String,
    width: u32,
    height: u32,
    native_window_ptr: usize,
    #[cfg(target_os = "android")]
    backend: android_media::AndroidMediaCodecBackend,
    closed: bool,
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_plugin_entry() -> *const VesperPluginDescriptor {
    catch_unwind(AssertUnwindSafe(vesper_plugin_entry_impl)).unwrap_or(std::ptr::null())
}

fn vesper_plugin_entry_impl() -> *const VesperPluginDescriptor {
    let mut bundle = Box::new(PluginBundle {
        api: VesperDecoderPluginApiV5 {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(decoder_name),
            capabilities_json: Some(decoder_capabilities_json),
            native_requirements_json: Some(decoder_native_requirements_json),
            open_session_json: Some(decoder_open_session_json),
            send_packet: Some(decoder_send_packet),
            receive_native_frame: Some(decoder_receive_native_frame),
            release_native_frame: Some(decoder_release_native_frame),
            flush_session: Some(decoder_flush_session),
            close_session: Some(decoder_close_session),
            free_bytes: Some(free_plugin_bytes),
            receive_pcm_frame: Some(decoder_receive_pcm_frame),
            release_native_frame2: Some(decoder_release_native_frame_with_presentation),
        },
        descriptor: VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: PLUGIN_NAME.as_ptr().cast::<c_char>(),
            api: std::ptr::null(),
        },
    });
    bundle.descriptor.api = (&bundle.api as *const VesperDecoderPluginApiV5).cast::<c_void>();
    let bundle = Box::leak(bundle);
    &bundle.descriptor
}

// SAFETY: The plugin loader calls this callback through the decoder ABI and
// keeps the returned static plugin name pointer borrowed only.
unsafe extern "C" fn decoder_name(_context: *mut c_void) -> *const c_char {
    catch_unwind(AssertUnwindSafe(|| PLUGIN_NAME.as_ptr().cast::<c_char>()))
        .unwrap_or(std::ptr::null())
}

// SAFETY: The plugin loader calls this callback through the decoder ABI and
// releases the returned bytes with this plugin's `free_bytes` callback.
unsafe extern "C" fn decoder_capabilities_json(_context: *mut c_void) -> VesperPluginBytes {
    catch_decoder_bytes(|| serialize_payload(&decoder_capabilities()))
}

// SAFETY: The plugin loader calls this callback through the decoder ABI and
// releases the returned bytes with this plugin's `free_bytes` callback.
unsafe extern "C" fn decoder_native_requirements_json(_context: *mut c_void) -> VesperPluginBytes {
    catch_decoder_bytes(|| serialize_payload(&decoder_native_requirements()))
}

// SAFETY: The plugin loader provides a valid JSON buffer for the duration of
// this synchronous callback and owns the returned session pointer.
unsafe extern "C" fn decoder_open_session_json(
    _context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperDecoderOpenSessionResult {
    catch_decoder_open(|| {
        let config = match decode_json::<DecoderSessionConfig>(config_json, config_json_len) {
            Ok(config) => config,
            Err(error) => return open_error(error),
        };
        match MediaCodecDecoderSession::open(config) {
            Ok(session) => {
                let info = session.session_info();
                open_success(Box::into_raw(Box::new(session)).cast::<c_void>(), &info)
            }
            Err(error) => open_error(error),
        }
    })
}

// SAFETY: The plugin loader passes a session pointer created by this plugin and
// valid packet JSON/data buffers for the duration of this synchronous callback.
unsafe extern "C" fn decoder_send_packet(
    _context: *mut c_void,
    session: *mut c_void,
    packet_json: *const u8,
    packet_json_len: usize,
    packet_data: *const u8,
    packet_data_len: usize,
) -> VesperPluginProcessResult {
    catch_decoder_process(|| {
        let Some(session) = media_codec_session_mut(session) else {
            return process_error(DecoderError::NotConfigured);
        };
        let packet = match decode_json::<DecoderPacket>(packet_json, packet_json_len) {
            Ok(packet) => packet,
            Err(error) => return process_error(error),
        };
        if packet_data.is_null() && packet_data_len > 0 {
            return process_error(DecoderError::abi_violation(
                "packet data pointer was null with non-zero len",
            ));
        }
        let packet_data = if packet_data_len == 0 {
            &[]
        } else {
            // SAFETY: the ABI caller provides the compressed packet byte slice
            // for the duration of this synchronous callback.
            unsafe { std::slice::from_raw_parts(packet_data, packet_data_len) }
        };
        session.send_packet(&packet, packet_data)
    })
}

// SAFETY: The plugin loader passes a live session pointer created by this
// plugin and consumes the returned frame metadata/handle synchronously.
unsafe extern "C" fn decoder_receive_native_frame(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperDecoderReceiveNativeFrameResult {
    catch_decoder_native_frame(|| {
        let Some(session) = media_codec_session_mut(session) else {
            return native_frame_error(DecoderError::NotConfigured);
        };
        match session.receive_native_frame() {
            Ok((metadata, handle)) => native_frame_success(&metadata, handle),
            Err(error) => native_frame_error(error),
        }
    })
}

// SAFETY: The plugin loader passes a live session pointer and a frame handle
// previously returned by this plugin for non-presenting release.
unsafe extern "C" fn decoder_release_native_frame(
    _context: *mut c_void,
    session: *mut c_void,
    handle_kind: u32,
    handle: usize,
) -> VesperPluginProcessResult {
    decoder_release_native_frame_impl(session, handle_kind, handle, false)
}

// SAFETY: The plugin loader passes a live session pointer and a frame handle
// previously returned by this plugin, plus the host presentation decision.
unsafe extern "C" fn decoder_release_native_frame_with_presentation(
    _context: *mut c_void,
    session: *mut c_void,
    handle_kind: u32,
    handle: usize,
    presented: bool,
) -> VesperPluginProcessResult {
    decoder_release_native_frame_impl(session, handle_kind, handle, presented)
}

fn decoder_release_native_frame_impl(
    session: *mut c_void,
    handle_kind: u32,
    handle: usize,
    presented: bool,
) -> VesperPluginProcessResult {
    catch_decoder_process(|| {
        let Some(session) = media_codec_session_mut(session) else {
            return process_error(DecoderError::NotConfigured);
        };
        if handle_kind != mediacodec_surface_texture_handle_kind_code() {
            return process_error(DecoderError::abi_violation(format!(
                "MediaCodec plugin expected SurfaceTexture handle kind, got {handle_kind}"
            )));
        }
        if handle == 0 {
            return process_error(DecoderError::abi_violation(
                "MediaCodec plugin received a null native frame handle",
            ));
        }
        session.release_native_frame(handle, presented)
    })
}

// SAFETY: The plugin loader passes a live session pointer created by this
// plugin and serializes flush with close for the same session.
unsafe extern "C" fn decoder_flush_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_decoder_process(|| {
        let Some(session) = media_codec_session_mut(session) else {
            return process_error(DecoderError::NotConfigured);
        };
        session.flush()
    })
}

// SAFETY: The plugin loader passes a session pointer created by this plugin
// with `Box::into_raw`; each non-null pointer is closed exactly once.
unsafe extern "C" fn decoder_close_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_decoder_process(|| {
        if session.is_null() {
            return process_success(&DecoderOperationStatus { completed: true });
        }
        // SAFETY: `session` was allocated by this plugin with `Box::into_raw`
        // and is closed exactly once by the dynamic loader.
        let mut session = unsafe { Box::from_raw(session.cast::<MediaCodecDecoderSession>()) };
        session.close()
    })
}

// SAFETY: The plugin loader passes a session pointer through the decoder ABI;
// MediaCodec currently reports unsupported PCM output without dereferencing it.
unsafe extern "C" fn decoder_receive_pcm_frame(
    _context: *mut c_void,
    _session: *mut c_void,
) -> VesperDecoderReceivePcmFrameResult {
    catch_decoder_pcm_frame(|| {
        pcm_frame_error(DecoderError::UnsupportedCapability {
            capability: "audio-pcm-output".to_owned(),
        })
    })
}

// SAFETY: The plugin loader returns only byte payloads allocated by this plugin
// to this callback for deallocation.
unsafe extern "C" fn free_plugin_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
    // SAFETY: payloads returned by this plugin are allocated from Vec<u8> with
    // capacity equal to len in this dynamic library.
    unsafe {
        let _ = payload.into_vec();
    }
}

impl MediaCodecDecoderSession {
    fn open(config: DecoderSessionConfig) -> Result<Self, DecoderError> {
        if !MEDIACODEC_SUPPORTED {
            return Err(DecoderError::UnsupportedCapability {
                capability: "android-mediacodec-decoder".to_owned(),
            });
        }
        if config.media_kind != DecoderMediaKind::Video {
            return Err(DecoderError::UnsupportedCapability {
                capability: "video-native-frame-output".to_owned(),
            });
        }
        if !decoder_capabilities().supports_codec(&config.codec, config.media_kind) {
            return Err(DecoderError::UnsupportedCodec {
                codec: config.codec,
            });
        }
        let native_window_ptr = android_native_window_ptr(config.native_device_context.as_ref())?;
        #[cfg(target_os = "android")]
        let backend = android_media::AndroidMediaCodecBackend::open(&config, native_window_ptr)?;
        Ok(Self {
            codec: config.codec,
            width: config.width.unwrap_or(config.coded_width.unwrap_or(0)),
            height: config.height.unwrap_or(config.coded_height.unwrap_or(0)),
            native_window_ptr,
            #[cfg(target_os = "android")]
            backend,
            closed: false,
        })
    }

    fn session_info(&self) -> DecoderSessionInfo {
        let _ = (
            self.codec.as_str(),
            self.width,
            self.height,
            self.native_window_ptr,
        );
        DecoderSessionInfo {
            decoder_name: Some("player-decoder-mediacodec".to_owned()),
            selected_hardware_backend: Some("MediaCodec".to_owned()),
            output_format: Some(mediacodec_surface_texture_format()),
        }
    }

    fn send_packet(&mut self, packet: &DecoderPacket, data: &[u8]) -> VesperPluginProcessResult {
        if self.closed {
            return process_error(DecoderError::NotConfigured);
        }
        if packet.media_kind != DecoderMediaKind::Video {
            return process_error(DecoderError::UnsupportedCapability {
                capability: "video-packet-input".to_owned(),
            });
        }
        #[cfg(target_os = "android")]
        match self.backend.send_packet(packet, data) {
            Ok(result) => process_success(&result),
            Err(error) => process_error(error),
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (packet, data);
            process_error(DecoderError::UnsupportedCapability {
                capability: "android-mediacodec-decoder".to_owned(),
            })
        }
    }

    fn receive_native_frame(
        &mut self,
    ) -> Result<(DecoderReceiveNativeFrameMetadata, usize), DecoderError> {
        if self.closed {
            return Err(DecoderError::NotConfigured);
        }
        #[cfg(target_os = "android")]
        {
            self.backend.receive_native_frame(&self.codec)
        }
        #[cfg(not(target_os = "android"))]
        {
            Ok((DecoderReceiveNativeFrameMetadata::need_more_input(), 0))
        }
    }

    fn release_native_frame(
        &mut self,
        handle: usize,
        presented: bool,
    ) -> VesperPluginProcessResult {
        if self.closed {
            return process_error(DecoderError::NotConfigured);
        }
        #[cfg(target_os = "android")]
        match self.backend.release_native_frame(handle, presented) {
            Ok(()) => process_success(&DecoderOperationStatus { completed: true }),
            Err(error) => process_error(error),
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (handle, presented);
            process_success(&DecoderOperationStatus { completed: true })
        }
    }

    fn flush(&mut self) -> VesperPluginProcessResult {
        if self.closed {
            return process_error(DecoderError::NotConfigured);
        }
        #[cfg(target_os = "android")]
        if let Err(error) = self.backend.flush() {
            return process_error(error);
        }
        process_success(&DecoderOperationStatus { completed: true })
    }

    fn close(&mut self) -> VesperPluginProcessResult {
        if self.closed {
            return process_success(&DecoderOperationStatus { completed: true });
        }
        #[cfg(target_os = "android")]
        if let Err(error) = self.backend.close() {
            self.closed = true;
            return process_error(error);
        }
        self.closed = true;
        process_success(&DecoderOperationStatus { completed: true })
    }
}

fn decoder_capabilities() -> DecoderCapabilities {
    DecoderCapabilities {
        codecs: vec![
            video_codec_capability("H264"),
            video_codec_capability("AVC1"),
            video_codec_capability("HEVC"),
            video_codec_capability("H265"),
            video_codec_capability("HVC1"),
            video_codec_capability("HEV1"),
        ],
        supports_hardware_decode: MEDIACODEC_SUPPORTED,
        supports_cpu_video_frames: false,
        supports_audio_frames: false,
        supports_pcm_frames: false,
        supports_gpu_handles: MEDIACODEC_SUPPORTED,
        supports_presentation_release: MEDIACODEC_SUPPORTED,
        supports_flush: true,
        supports_drain: true,
        max_sessions: None,
    }
}

fn decoder_native_requirements() -> DecoderNativeRequirements {
    DecoderNativeRequirements {
        required_device_context_kinds: vec![DecoderNativeDeviceContextKind::AndroidNativeWindow],
        output_handle_kinds: vec![DecoderNativeHandleKind::MediaCodecSurfaceTexture],
        output_pipeline_profiles: vec![NativeFramePipelineProfile::MediaCodecSurfaceTexture],
        requires_native_device_context: true,
        accepted_bitstream_formats: vec![
            DecoderBitstreamFormat::Avcc,
            DecoderBitstreamFormat::Hvcc,
        ],
    }
}

fn video_codec_capability(codec: &str) -> DecoderCodecCapability {
    DecoderCodecCapability {
        codec: codec.to_owned(),
        media_kind: DecoderMediaKind::Video,
        profiles: Vec::new(),
        output_formats: vec![mediacodec_surface_texture_format()],
    }
}

fn mediacodec_surface_texture_format() -> DecoderFrameFormat {
    DecoderFrameFormat::Unknown(MEDIACODEC_SURFACE_TEXTURE_FORMAT.to_owned())
}

fn android_native_window_ptr(
    context: Option<&DecoderNativeDeviceContext>,
) -> Result<usize, DecoderError> {
    let Some(context) = context else {
        return Err(DecoderError::UnsupportedCapability {
            capability: "android-native-window-device-context".to_owned(),
        });
    };
    let Some(window_ptr) = context.android_native_window_ptr() else {
        return Err(DecoderError::UnsupportedCapability {
            capability: "android-native-window-device-context".to_owned(),
        });
    };
    if window_ptr == 0 {
        return Err(DecoderError::abi_violation(
            "Android native window device context pointer is null",
        ));
    }
    Ok(window_ptr)
}

fn mediacodec_surface_texture_handle_kind_code() -> u32 {
    10
}

fn media_codec_session_mut<'a>(session: *mut c_void) -> Option<&'a mut MediaCodecDecoderSession> {
    if session.is_null() {
        return None;
    }
    // SAFETY: `session` must be the opaque pointer returned by this plugin's
    // open callback and remains owned by the host until close.
    unsafe { session.cast::<MediaCodecDecoderSession>().as_mut() }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn codec_mime(codec: &str) -> Option<&'static str> {
    if codec.eq_ignore_ascii_case("h264") || codec.eq_ignore_ascii_case("avc1") {
        Some("video/avc")
    } else if codec.eq_ignore_ascii_case("hevc")
        || codec.eq_ignore_ascii_case("h265")
        || codec.eq_ignore_ascii_case("hvc1")
        || codec.eq_ignore_ascii_case("hev1")
    {
        Some("video/hevc")
    } else {
        None
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn split_avcc_extradata(extradata: &[u8]) -> Vec<Vec<u8>> {
    if extradata.len() < 7 || extradata[0] != 1 {
        return vec![extradata.to_vec()];
    }
    let mut offset = 5;
    let sps_count = extradata[offset] & 0x1f;
    offset += 1;
    let mut units = Vec::new();
    for _ in 0..sps_count {
        let Some(length) = read_be_u16(extradata, offset) else {
            return vec![extradata.to_vec()];
        };
        offset += 2;
        let end = offset.saturating_add(usize::from(length));
        if end > extradata.len() {
            return vec![extradata.to_vec()];
        }
        units.push(with_annex_b_start_code(&extradata[offset..end]));
        offset = end;
    }
    if offset >= extradata.len() {
        return units;
    }
    let pps_count = extradata[offset];
    offset += 1;
    for _ in 0..pps_count {
        let Some(length) = read_be_u16(extradata, offset) else {
            return vec![extradata.to_vec()];
        };
        offset += 2;
        let end = offset.saturating_add(usize::from(length));
        if end > extradata.len() {
            return vec![extradata.to_vec()];
        }
        units.push(with_annex_b_start_code(&extradata[offset..end]));
        offset = end;
    }
    if units.is_empty() {
        vec![extradata.to_vec()]
    } else {
        units
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn split_hvcc_extradata(extradata: &[u8]) -> Vec<Vec<u8>> {
    if extradata.len() < 23 || extradata[0] != 1 {
        return vec![extradata.to_vec()];
    }
    let mut offset = 22;
    let array_count = extradata[offset];
    offset += 1;
    let mut units = Vec::new();
    for _ in 0..array_count {
        if offset + 3 > extradata.len() {
            return vec![extradata.to_vec()];
        }
        offset += 1;
        let Some(nal_count) = read_be_u16(extradata, offset) else {
            return vec![extradata.to_vec()];
        };
        offset += 2;
        for _ in 0..nal_count {
            let Some(length) = read_be_u16(extradata, offset) else {
                return vec![extradata.to_vec()];
            };
            offset += 2;
            let end = offset.saturating_add(usize::from(length));
            if end > extradata.len() {
                return vec![extradata.to_vec()];
            }
            units.push(with_annex_b_start_code(&extradata[offset..end]));
            offset = end;
        }
    }
    if units.is_empty() {
        vec![extradata.to_vec()]
    } else {
        units
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn codec_config_buffers(config: &DecoderSessionConfig) -> Vec<Vec<u8>> {
    if config.extradata.is_empty() {
        return Vec::new();
    }
    match config.bitstream_format.as_ref() {
        Some(DecoderBitstreamFormat::Avcc) => split_avcc_extradata(&config.extradata),
        Some(DecoderBitstreamFormat::Hvcc) => split_hvcc_extradata(&config.extradata),
        _ => vec![config.extradata.clone()],
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn nal_length_size_for_config(config: &DecoderSessionConfig) -> usize {
    match config.bitstream_format.as_ref() {
        Some(DecoderBitstreamFormat::Avcc) => avcc_nal_length_size(&config.extradata),
        Some(DecoderBitstreamFormat::Hvcc) => hvcc_nal_length_size(&config.extradata),
        _ => None,
    }
    .unwrap_or(4)
    .clamp(1, 4)
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn avcc_nal_length_size(extradata: &[u8]) -> Option<usize> {
    if extradata.len() >= 5 && extradata[0] == 1 {
        Some(usize::from((extradata[4] & 0x03) + 1))
    } else {
        None
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn hvcc_nal_length_size(extradata: &[u8]) -> Option<usize> {
    if extradata.len() >= 22 && extradata[0] == 1 {
        Some(usize::from((extradata[21] & 0x03) + 1))
    } else {
        None
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn packet_data_for_mediacodec<'a>(
    bitstream_format: Option<&DecoderBitstreamFormat>,
    nal_length_size: usize,
    data: &'a [u8],
) -> Result<Cow<'a, [u8]>, DecoderError> {
    match bitstream_format {
        Some(DecoderBitstreamFormat::Avcc) | Some(DecoderBitstreamFormat::Hvcc) => {
            if data.is_empty() || has_annex_b_start_code(data) {
                Ok(Cow::Borrowed(data))
            } else {
                length_prefixed_sample_to_annex_b(data, nal_length_size).map(Cow::Owned)
            }
        }
        _ => Ok(Cow::Borrowed(data)),
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn length_prefixed_sample_to_annex_b(
    data: &[u8],
    nal_length_size: usize,
) -> Result<Vec<u8>, DecoderError> {
    let nal_length_size = nal_length_size.clamp(1, 4);
    let mut offset = 0;
    let mut output = Vec::with_capacity(data.len().saturating_add(4));
    while offset < data.len() {
        if offset.saturating_add(nal_length_size) > data.len() {
            return Err(DecoderError::InvalidPacket {
                message: format!(
                    "length-prefixed MediaCodec sample ended inside a {nal_length_size}-byte NAL length field"
                ),
            });
        }
        let mut nal_len = 0_usize;
        for byte in &data[offset..offset + nal_length_size] {
            nal_len = (nal_len << 8) | usize::from(*byte);
        }
        offset += nal_length_size;
        if nal_len == 0 {
            continue;
        }
        let end = offset.saturating_add(nal_len);
        if end > data.len() {
            return Err(DecoderError::InvalidPacket {
                message: format!(
                    "length-prefixed MediaCodec sample declared NAL length {nal_len} beyond packet size"
                ),
            });
        }
        output.extend_from_slice(&[0, 0, 0, 1]);
        output.extend_from_slice(&data[offset..end]);
        offset = end;
    }
    if output.is_empty() && !data.is_empty() {
        return Err(DecoderError::InvalidPacket {
            message: "length-prefixed MediaCodec sample did not contain a NAL unit".to_owned(),
        });
    }
    Ok(output)
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn has_annex_b_start_code(data: &[u8]) -> bool {
    data.starts_with(&[0, 0, 1]) || data.starts_with(&[0, 0, 0, 1])
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn with_annex_b_start_code(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 4);
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(data);
    out
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn read_be_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn packet_input_data_for_mediacodec<'a>(
    packet: &DecoderPacket,
    bitstream_format: Option<&DecoderBitstreamFormat>,
    nal_length_size: usize,
    data: &'a [u8],
) -> Result<Cow<'a, [u8]>, DecoderError> {
    if packet.end_of_stream {
        Ok(Cow::Borrowed(&[]))
    } else {
        packet_data_for_mediacodec(bitstream_format, nal_length_size, data)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
enum MediaCodecOutputBufferKind {
    CodecConfig,
    Eof,
    Frame { pending_eos: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
enum MediaCodecPendingOutputEosAction {
    WaitForOutstandingRelease,
    ReportEof,
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn mediacodec_output_buffer_kind(flags: u32, size: i32) -> MediaCodecOutputBufferKind {
    const CODEC_CONFIG: u32 = 2;
    const END_OF_STREAM: u32 = 4;
    if flags & CODEC_CONFIG != 0 {
        MediaCodecOutputBufferKind::CodecConfig
    } else if flags & END_OF_STREAM != 0 && size <= 0 {
        MediaCodecOutputBufferKind::Eof
    } else {
        MediaCodecOutputBufferKind::Frame {
            pending_eos: flags & END_OF_STREAM != 0,
        }
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn mediacodec_pending_output_eos_action(
    outstanding_frame_count: usize,
) -> MediaCodecPendingOutputEosAction {
    if outstanding_frame_count == 0 {
        MediaCodecPendingOutputEosAction::ReportEof
    } else {
        MediaCodecPendingOutputEosAction::WaitForOutstandingRelease
    }
}

#[cfg(target_os = "android")]
mod android_media {
    use std::collections::HashMap;
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::ptr::NonNull;

    use player_plugin::{
        DecoderBitstreamFormat, DecoderError, DecoderMediaKind, DecoderNativeFrameMetadata,
        DecoderNativeFrameReleaseTracking, DecoderNativeHandleKind, DecoderPacket,
        DecoderPacketResult, DecoderReceiveNativeFrameMetadata, DecoderVisibleRect,
        NativeFramePipelineProfile,
    };

    use super::{
        MediaCodecOutputBufferKind, MediaCodecPendingOutputEosAction, codec_config_buffers,
        codec_mime, mediacodec_output_buffer_kind, mediacodec_pending_output_eos_action,
        mediacodec_surface_texture_format, nal_length_size_for_config,
        packet_input_data_for_mediacodec,
    };

    const AMEDIA_OK: MediaStatus = 0;
    const AMEDIACODEC_INFO_TRY_AGAIN_LATER: isize = -1;
    const AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED: isize = -2;
    const AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED: isize = -3;
    const AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM: u32 = 4;
    const DEQUEUE_TIMEOUT_US: i64 = 0;
    const AMEDIAFORMAT_KEY_CROP_LEFT_FALLBACK: &CStr = c"crop-left";
    const AMEDIAFORMAT_KEY_CROP_RIGHT_FALLBACK: &CStr = c"crop-right";
    const AMEDIAFORMAT_KEY_CROP_TOP_FALLBACK: &CStr = c"crop-top";
    const AMEDIAFORMAT_KEY_CROP_BOTTOM_FALLBACK: &CStr = c"crop-bottom";

    type MediaStatus = i32;

    #[repr(C)]
    struct AMediaCodec {
        _private: [u8; 0],
    }

    #[repr(C)]
    struct AMediaFormat {
        _private: [u8; 0],
    }

    #[repr(C)]
    struct ANativeWindow {
        _private: [u8; 0],
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy, Default)]
    struct AMediaCodecBufferInfo {
        offset: i32,
        size: i32,
        presentation_time_us: i64,
        flags: u32,
    }

    #[link(name = "mediandk")]
    unsafe extern "C" {
        static AMEDIAFORMAT_KEY_MIME: *const c_char;
        static AMEDIAFORMAT_KEY_WIDTH: *const c_char;
        static AMEDIAFORMAT_KEY_HEIGHT: *const c_char;
        static AMEDIAFORMAT_KEY_CSD_0: *const c_char;
        static AMEDIAFORMAT_KEY_CSD_1: *const c_char;
        static AMEDIAFORMAT_KEY_CSD_2: *const c_char;

        fn AMediaFormat_new() -> *mut AMediaFormat;
        fn AMediaFormat_delete(format: *mut AMediaFormat) -> MediaStatus;
        fn AMediaFormat_setString(
            format: *mut AMediaFormat,
            name: *const c_char,
            value: *const c_char,
        );
        fn AMediaFormat_setInt32(format: *mut AMediaFormat, name: *const c_char, value: i32);
        fn AMediaFormat_getInt32(
            format: *const AMediaFormat,
            name: *const c_char,
            out: *mut i32,
        ) -> bool;
        fn AMediaFormat_setBuffer(
            format: *mut AMediaFormat,
            name: *const c_char,
            data: *const c_void,
            size: usize,
        );

        fn AMediaCodec_createDecoderByType(mime_type: *const c_char) -> *mut AMediaCodec;
        fn AMediaCodec_configure(
            codec: *mut AMediaCodec,
            format: *const AMediaFormat,
            surface: *mut ANativeWindow,
            crypto: *mut c_void,
            flags: u32,
        ) -> MediaStatus;
        fn AMediaCodec_start(codec: *mut AMediaCodec) -> MediaStatus;
        fn AMediaCodec_stop(codec: *mut AMediaCodec) -> MediaStatus;
        fn AMediaCodec_flush(codec: *mut AMediaCodec) -> MediaStatus;
        fn AMediaCodec_delete(codec: *mut AMediaCodec) -> MediaStatus;
        fn AMediaCodec_dequeueInputBuffer(codec: *mut AMediaCodec, timeout_us: i64) -> isize;
        fn AMediaCodec_getInputBuffer(
            codec: *mut AMediaCodec,
            index: usize,
            out_size: *mut usize,
        ) -> *mut u8;
        fn AMediaCodec_queueInputBuffer(
            codec: *mut AMediaCodec,
            index: usize,
            offset: isize,
            size: usize,
            time_us: u64,
            flags: u32,
        ) -> MediaStatus;
        fn AMediaCodec_dequeueOutputBuffer(
            codec: *mut AMediaCodec,
            info: *mut AMediaCodecBufferInfo,
            timeout_us: i64,
        ) -> isize;
        fn AMediaCodec_getOutputFormat(codec: *mut AMediaCodec) -> *mut AMediaFormat;
        fn AMediaCodec_releaseOutputBuffer(
            codec: *mut AMediaCodec,
            index: usize,
            render: bool,
        ) -> MediaStatus;
    }

    #[derive(Debug)]
    pub(super) struct AndroidMediaCodecBackend {
        codec: NonNull<AMediaCodec>,
        output_format: MediaCodecOutputFormat,
        bitstream_format: Option<DecoderBitstreamFormat>,
        nal_length_size: usize,
        outstanding: HashMap<usize, MediaCodecOutputFrame>,
        saw_input_eos: bool,
        saw_output_eos: bool,
        pending_output_eos: bool,
        closed: bool,
    }

    #[derive(Debug, Clone, Copy)]
    struct MediaCodecOutputFrame {
        index: usize,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct MediaCodecOutputFormat {
        width: u32,
        height: u32,
        coded_width: u32,
        coded_height: u32,
        visible_rect: DecoderVisibleRect,
    }

    impl MediaCodecOutputFormat {
        fn new(width: u32, height: u32) -> Self {
            Self {
                width,
                height,
                coded_width: width,
                coded_height: height,
                visible_rect: DecoderVisibleRect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                },
            }
        }
    }

    impl AndroidMediaCodecBackend {
        pub(super) fn open(
            config: &player_plugin::DecoderSessionConfig,
            native_window_ptr: usize,
        ) -> Result<Self, DecoderError> {
            let mime = codec_mime(&config.codec).ok_or_else(|| DecoderError::UnsupportedCodec {
                codec: config.codec.clone(),
            })?;
            let mime = CString::new(mime).map_err(|error| {
                DecoderError::payload_codec(format!("invalid MediaCodec MIME string: {error}"))
            })?;
            let width = config.width.or(config.coded_width).unwrap_or(0);
            let height = config.height.or(config.coded_height).unwrap_or(0);
            if width == 0 || height == 0 {
                return Err(DecoderError::InvalidPacket {
                    message: "MediaCodec video config requires non-zero width and height"
                        .to_owned(),
                });
            }
            let format = MediaFormat::new()?;
            // SAFETY: `format` is a valid AMediaFormat and the key pointers are
            // process-lifetime NDK constants.
            unsafe {
                AMediaFormat_setString(format.as_ptr(), AMEDIAFORMAT_KEY_MIME, mime.as_ptr());
                AMediaFormat_setInt32(format.as_ptr(), AMEDIAFORMAT_KEY_WIDTH, width as i32);
                AMediaFormat_setInt32(format.as_ptr(), AMEDIAFORMAT_KEY_HEIGHT, height as i32);
            }
            for (index, buffer) in codec_config_buffers(config).iter().take(3).enumerate() {
                let key = match index {
                    0 => unsafe { AMEDIAFORMAT_KEY_CSD_0 },
                    1 => unsafe { AMEDIAFORMAT_KEY_CSD_1 },
                    _ => unsafe { AMEDIAFORMAT_KEY_CSD_2 },
                };
                // SAFETY: `buffer` is borrowed for this call only and
                // AMediaFormat copies the provided bytes.
                unsafe {
                    AMediaFormat_setBuffer(
                        format.as_ptr(),
                        key,
                        buffer.as_ptr().cast::<c_void>(),
                        buffer.len(),
                    );
                }
            }
            // SAFETY: the MIME CString is valid and NUL-terminated.
            let codec = NonNull::new(unsafe { AMediaCodec_createDecoderByType(mime.as_ptr()) })
                .ok_or_else(|| DecoderError::UnsupportedCapability {
                    capability: "android-mediacodec-create-decoder".to_owned(),
                })?;
            let surface = native_window_ptr as *mut ANativeWindow;
            // SAFETY: `codec`, `format`, and `surface` are valid for this call.
            let configure_status = unsafe {
                AMediaCodec_configure(
                    codec.as_ptr(),
                    format.as_ptr(),
                    surface,
                    std::ptr::null_mut(),
                    0,
                )
            };
            if configure_status != AMEDIA_OK {
                // SAFETY: codec was created above and has not been deleted yet.
                unsafe {
                    let _ = AMediaCodec_delete(codec.as_ptr());
                }
                return Err(media_status_error(
                    "AMediaCodec_configure",
                    configure_status,
                ));
            }
            // SAFETY: `codec` is configured and ready to start.
            let start_status = unsafe { AMediaCodec_start(codec.as_ptr()) };
            if start_status != AMEDIA_OK {
                // SAFETY: codec was created above and has not been deleted yet.
                unsafe {
                    let _ = AMediaCodec_delete(codec.as_ptr());
                }
                return Err(media_status_error("AMediaCodec_start", start_status));
            }
            Ok(Self {
                codec,
                output_format: MediaCodecOutputFormat::new(width, height),
                bitstream_format: config.bitstream_format.clone(),
                nal_length_size: nal_length_size_for_config(config),
                outstanding: HashMap::new(),
                saw_input_eos: false,
                saw_output_eos: false,
                pending_output_eos: false,
                closed: false,
            })
        }

        pub(super) fn send_packet(
            &mut self,
            packet: &DecoderPacket,
            data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            if self.closed {
                return Err(DecoderError::NotConfigured);
            }
            if self.saw_input_eos {
                return Err(DecoderError::Eof);
            }
            let index = match self.dequeue_input()? {
                Some(index) => index,
                None => return Ok(DecoderPacketResult { accepted: false }),
            };
            let flags = if packet.end_of_stream {
                AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM
            } else {
                0
            };
            let pts_us = packet.pts_us.or(packet.dts_us).unwrap_or_default().max(0) as u64;
            let input_data = packet_input_data_for_mediacodec(
                packet,
                self.bitstream_format.as_ref(),
                self.nal_length_size,
                data,
            )?;
            if !packet.end_of_stream {
                self.copy_input(index, input_data.as_ref())?;
            }
            // SAFETY: `index` came from dequeueInputBuffer and the copied data
            // length fits that input buffer.
            let status = unsafe {
                AMediaCodec_queueInputBuffer(
                    self.codec.as_ptr(),
                    index,
                    0,
                    input_data.len(),
                    pts_us,
                    flags,
                )
            };
            if status != AMEDIA_OK {
                return Err(media_status_error("AMediaCodec_queueInputBuffer", status));
            }
            if packet.end_of_stream {
                self.saw_input_eos = true;
            }
            Ok(DecoderPacketResult { accepted: true })
        }

        pub(super) fn receive_native_frame(
            &mut self,
            codec_name: &str,
        ) -> Result<(DecoderReceiveNativeFrameMetadata, usize), DecoderError> {
            if self.closed {
                return Err(DecoderError::NotConfigured);
            }
            if self.saw_output_eos {
                return Ok((DecoderReceiveNativeFrameMetadata::eof(), 0));
            }
            if self.pending_output_eos {
                match mediacodec_pending_output_eos_action(self.outstanding.len()) {
                    MediaCodecPendingOutputEosAction::ReportEof => {
                        self.pending_output_eos = false;
                        self.saw_output_eos = true;
                        return Ok((DecoderReceiveNativeFrameMetadata::eof(), 0));
                    }
                    MediaCodecPendingOutputEosAction::WaitForOutstandingRelease => {
                        return Ok((DecoderReceiveNativeFrameMetadata::need_more_input(), 0));
                    }
                }
            }
            loop {
                let mut info = AMediaCodecBufferInfo::default();
                // SAFETY: `codec` is valid and `info` is writable.
                let output = unsafe {
                    AMediaCodec_dequeueOutputBuffer(
                        self.codec.as_ptr(),
                        &mut info,
                        DEQUEUE_TIMEOUT_US,
                    )
                };
                match output {
                    AMEDIACODEC_INFO_TRY_AGAIN_LATER => {
                        return Ok((DecoderReceiveNativeFrameMetadata::need_more_input(), 0));
                    }
                    AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED => {
                        self.update_output_format()?;
                        continue;
                    }
                    AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED => continue,
                    value if value < 0 => {
                        return Err(DecoderError::internal(format!(
                            "AMediaCodec_dequeueOutputBuffer returned {value}"
                        )));
                    }
                    value => {
                        let index = usize::try_from(value).map_err(|_| {
                            DecoderError::internal("MediaCodec output index overflowed usize")
                        })?;
                        let output_kind = mediacodec_output_buffer_kind(info.flags, info.size);
                        match output_kind {
                            MediaCodecOutputBufferKind::CodecConfig => {
                                self.release_output_index(index, false)?;
                                continue;
                            }
                            MediaCodecOutputBufferKind::Eof => {
                                self.release_output_index(index, false)?;
                                self.saw_output_eos = true;
                                return Ok((DecoderReceiveNativeFrameMetadata::eof(), 0));
                            }
                            MediaCodecOutputBufferKind::Frame { pending_eos } => {
                                if pending_eos {
                                    self.pending_output_eos = true;
                                }
                            }
                        }
                        let handle = output_handle(index)?;
                        if self.outstanding.contains_key(&handle) {
                            self.release_output_index(index, false)?;
                            return Err(DecoderError::abi_violation(format!(
                                "MediaCodec output handle collision for index {index}"
                            )));
                        }
                        self.outstanding
                            .insert(handle, MediaCodecOutputFrame { index });
                        let output_format = self.output_format.clone();
                        let metadata = DecoderNativeFrameMetadata {
                            media_kind: DecoderMediaKind::Video,
                            format: mediacodec_surface_texture_format(),
                            codec: codec_name.to_owned(),
                            pts_us: Some(info.presentation_time_us),
                            duration_us: None,
                            width: output_format.width,
                            height: output_format.height,
                            coded_width: Some(output_format.coded_width),
                            coded_height: Some(output_format.coded_height),
                            visible_rect: Some(output_format.visible_rect),
                            handle_kind: DecoderNativeHandleKind::MediaCodecSurfaceTexture,
                            pipeline_profile: Some(
                                NativeFramePipelineProfile::MediaCodecSurfaceTexture,
                            ),
                            color_space: None,
                            hdr_metadata: None,
                            sync_info: None,
                            transform: None,
                            frame_id: Some(handle as u64),
                            release_tracking: Some(DecoderNativeFrameReleaseTracking {
                                frame_id: Some(handle as u64),
                                requires_release: true,
                            }),
                        };
                        return Ok((DecoderReceiveNativeFrameMetadata::frame(metadata), handle));
                    }
                }
            }
        }

        pub(super) fn release_native_frame(
            &mut self,
            handle: usize,
            presented: bool,
        ) -> Result<(), DecoderError> {
            let Some(frame) = self.outstanding.remove(&handle) else {
                return Err(DecoderError::abi_violation(format!(
                    "unknown MediaCodec output frame handle {handle}"
                )));
            };
            self.release_output_index(frame.index, presented)
        }

        pub(super) fn flush(&mut self) -> Result<(), DecoderError> {
            self.release_all_outstanding(false)?;
            // SAFETY: `codec` is valid and started.
            let status = unsafe { AMediaCodec_flush(self.codec.as_ptr()) };
            if status != AMEDIA_OK {
                return Err(media_status_error("AMediaCodec_flush", status));
            }
            self.saw_input_eos = false;
            self.saw_output_eos = false;
            self.pending_output_eos = false;
            Ok(())
        }

        pub(super) fn close(&mut self) -> Result<(), DecoderError> {
            if self.closed {
                return Ok(());
            }
            let release_result = self.release_all_outstanding(false);
            // SAFETY: `codec` is valid until delete completes.
            let stop_status = unsafe { AMediaCodec_stop(self.codec.as_ptr()) };
            // SAFETY: `codec` is valid and deleted exactly once here.
            let delete_status = unsafe { AMediaCodec_delete(self.codec.as_ptr()) };
            self.closed = true;
            release_result?;
            if stop_status != AMEDIA_OK {
                return Err(media_status_error("AMediaCodec_stop", stop_status));
            }
            if delete_status != AMEDIA_OK {
                return Err(media_status_error("AMediaCodec_delete", delete_status));
            }
            Ok(())
        }

        fn dequeue_input(&mut self) -> Result<Option<usize>, DecoderError> {
            // SAFETY: `codec` is valid and started.
            let index =
                unsafe { AMediaCodec_dequeueInputBuffer(self.codec.as_ptr(), DEQUEUE_TIMEOUT_US) };
            match index {
                AMEDIACODEC_INFO_TRY_AGAIN_LATER => Ok(None),
                value if value < 0 => Err(DecoderError::internal(format!(
                    "AMediaCodec_dequeueInputBuffer returned {value}"
                ))),
                value => usize::try_from(value)
                    .map(Some)
                    .map_err(|_| DecoderError::internal("MediaCodec input index overflowed usize")),
            }
        }

        fn copy_input(&mut self, index: usize, data: &[u8]) -> Result<(), DecoderError> {
            let mut capacity = 0_usize;
            // SAFETY: `index` came from dequeueInputBuffer and `capacity` is writable.
            let buffer =
                unsafe { AMediaCodec_getInputBuffer(self.codec.as_ptr(), index, &mut capacity) };
            let Some(buffer) = NonNull::new(buffer) else {
                return Err(DecoderError::internal(
                    "AMediaCodec_getInputBuffer returned null",
                ));
            };
            if data.len() > capacity {
                return Err(DecoderError::InvalidPacket {
                    message: format!(
                        "MediaCodec input packet is larger than input buffer: {} > {}",
                        data.len(),
                        capacity
                    ),
                });
            }
            // SAFETY: `buffer` points to an input buffer with at least `capacity`
            // bytes and `data.len() <= capacity`.
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), buffer.as_ptr(), data.len());
            }
            Ok(())
        }

        fn update_output_format(&mut self) -> Result<(), DecoderError> {
            // SAFETY: `codec` is valid and started. The returned AMediaFormat is
            // owned by the caller and released by the MediaFormat wrapper below.
            let Some(format) =
                NonNull::new(unsafe { AMediaCodec_getOutputFormat(self.codec.as_ptr()) })
            else {
                return Err(DecoderError::internal(
                    "AMediaCodec_getOutputFormat returned null",
                ));
            };
            let format = MediaFormat { format };
            self.output_format = output_format_from_media_format(&format, &self.output_format);
            Ok(())
        }

        fn release_all_outstanding(&mut self, presented: bool) -> Result<(), DecoderError> {
            let outstanding = std::mem::take(&mut self.outstanding);
            let mut first_error = None;
            for (_, frame) in outstanding {
                if let Err(error) = self.release_output_index(frame.index, presented)
                    && first_error.is_none()
                {
                    first_error = Some(error);
                }
            }
            match first_error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }

        fn release_output_index(
            &mut self,
            index: usize,
            presented: bool,
        ) -> Result<(), DecoderError> {
            // SAFETY: `index` is an outstanding output buffer index from this codec.
            let status =
                unsafe { AMediaCodec_releaseOutputBuffer(self.codec.as_ptr(), index, presented) };
            if status == AMEDIA_OK {
                Ok(())
            } else {
                Err(media_status_error(
                    "AMediaCodec_releaseOutputBuffer",
                    status,
                ))
            }
        }
    }

    impl Drop for AndroidMediaCodecBackend {
        fn drop(&mut self) {
            let _ = self.close();
        }
    }

    struct MediaFormat {
        format: NonNull<AMediaFormat>,
    }

    impl MediaFormat {
        fn new() -> Result<Self, DecoderError> {
            // SAFETY: AMediaFormat_new returns a new object or null.
            NonNull::new(unsafe { AMediaFormat_new() })
                .map(|format| Self { format })
                .ok_or_else(|| DecoderError::internal("AMediaFormat_new returned null"))
        }

        fn as_ptr(&self) -> *mut AMediaFormat {
            self.format.as_ptr()
        }

        fn get_i32(&self, key: *const c_char) -> Option<i32> {
            if key.is_null() {
                return None;
            }
            let mut value = 0;
            // SAFETY: `self.format` is a valid AMediaFormat, `key` is a
            // process-lifetime NDK key, and `value` is writable.
            if unsafe { AMediaFormat_getInt32(self.format.as_ptr(), key, &mut value) } {
                Some(value)
            } else {
                None
            }
        }
    }

    impl Drop for MediaFormat {
        fn drop(&mut self) {
            // SAFETY: `format` was created by AMediaFormat_new and is deleted once here.
            unsafe {
                let _ = AMediaFormat_delete(self.format.as_ptr());
            }
        }
    }

    fn output_handle(index: usize) -> Result<usize, DecoderError> {
        index
            .checked_add(1)
            .ok_or_else(|| DecoderError::internal("MediaCodec output handle overflowed usize"))
    }

    fn output_format_from_media_format(
        format: &MediaFormat,
        fallback: &MediaCodecOutputFormat,
    ) -> MediaCodecOutputFormat {
        let coded_width = format
            .get_i32(unsafe { AMEDIAFORMAT_KEY_WIDTH })
            .and_then(non_negative_i32_to_u32)
            .unwrap_or(fallback.coded_width);
        let coded_height = format
            .get_i32(unsafe { AMEDIAFORMAT_KEY_HEIGHT })
            .and_then(non_negative_i32_to_u32)
            .unwrap_or(fallback.coded_height);
        let crop_left = format
            .get_i32(AMEDIAFORMAT_KEY_CROP_LEFT_FALLBACK.as_ptr())
            .and_then(non_negative_i32_to_u32);
        let crop_right = format
            .get_i32(AMEDIAFORMAT_KEY_CROP_RIGHT_FALLBACK.as_ptr())
            .and_then(non_negative_i32_to_u32);
        let crop_top = format
            .get_i32(AMEDIAFORMAT_KEY_CROP_TOP_FALLBACK.as_ptr())
            .and_then(non_negative_i32_to_u32);
        let crop_bottom = format
            .get_i32(AMEDIAFORMAT_KEY_CROP_BOTTOM_FALLBACK.as_ptr())
            .and_then(non_negative_i32_to_u32);
        let visible_rect = match (crop_left, crop_right, crop_top, crop_bottom) {
            (Some(left), Some(right), Some(top), Some(bottom))
                if right >= left && bottom >= top =>
            {
                DecoderVisibleRect {
                    x: left,
                    y: top,
                    width: right.saturating_sub(left).saturating_add(1),
                    height: bottom.saturating_sub(top).saturating_add(1),
                }
            }
            _ => DecoderVisibleRect {
                x: 0,
                y: 0,
                width: coded_width,
                height: coded_height,
            },
        };
        normalize_output_format(coded_width, coded_height, visible_rect)
    }

    fn normalize_output_format(
        coded_width: u32,
        coded_height: u32,
        visible_rect: DecoderVisibleRect,
    ) -> MediaCodecOutputFormat {
        let fallback_width = coded_width.max(1);
        let fallback_height = coded_height.max(1);
        let visible_width = visible_rect.width.min(fallback_width).max(1);
        let visible_height = visible_rect.height.min(fallback_height).max(1);
        MediaCodecOutputFormat {
            width: visible_width,
            height: visible_height,
            coded_width: fallback_width,
            coded_height: fallback_height,
            visible_rect,
        }
    }

    fn non_negative_i32_to_u32(value: i32) -> Option<u32> {
        u32::try_from(value).ok()
    }

    fn media_status_error(operation: &str, status: MediaStatus) -> DecoderError {
        DecoderError::internal(format!("{operation} failed with media_status_t={status}"))
    }
}

fn decode_json<T>(data: *const u8, len: usize) -> Result<T, DecoderError>
where
    T: serde::de::DeserializeOwned,
{
    if data.is_null() && len > 0 {
        return Err(DecoderError::payload_codec(
            "JSON payload pointer is null while len is non-zero",
        ));
    }
    let slice = if len == 0 {
        &[]
    } else {
        // SAFETY: the ABI caller provides a valid JSON byte slice for the
        // duration of this call.
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    serde_json::from_slice(slice).map_err(|error| DecoderError::payload_codec(error.to_string()))
}

fn serialize_payload<T>(payload: &T) -> VesperPluginBytes
where
    T: serde::Serialize,
{
    match serde_json::to_vec(payload) {
        Ok(bytes) => VesperPluginBytes::from_vec(bytes),
        Err(error) => VesperPluginBytes::from_vec(error.to_string().into_bytes()),
    }
}

fn open_success(session: *mut c_void, info: &DecoderSessionInfo) -> VesperDecoderOpenSessionResult {
    VesperDecoderOpenSessionResult {
        status: VesperPluginResultStatus::Success,
        session,
        payload: serialize_payload(info),
    }
}

fn open_error(error: DecoderError) -> VesperDecoderOpenSessionResult {
    VesperDecoderOpenSessionResult {
        status: VesperPluginResultStatus::Failure,
        session: std::ptr::null_mut(),
        payload: serialize_payload(&error),
    }
}

fn process_success<T>(payload: &T) -> VesperPluginProcessResult
where
    T: serde::Serialize,
{
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: serialize_payload(payload),
    }
}

fn process_error(error: DecoderError) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Failure,
        payload: serialize_payload(&error),
    }
}

fn native_frame_success(
    metadata: &DecoderReceiveNativeFrameMetadata,
    handle: usize,
) -> VesperDecoderReceiveNativeFrameResult {
    VesperDecoderReceiveNativeFrameResult {
        status: VesperPluginResultStatus::Success,
        metadata: serialize_payload(metadata),
        handle,
    }
}

fn native_frame_error(error: DecoderError) -> VesperDecoderReceiveNativeFrameResult {
    VesperDecoderReceiveNativeFrameResult {
        status: VesperPluginResultStatus::Failure,
        metadata: serialize_payload(&error),
        handle: 0,
    }
}

fn pcm_frame_error(error: DecoderError) -> VesperDecoderReceivePcmFrameResult {
    VesperDecoderReceivePcmFrameResult {
        status: VesperPluginResultStatus::Failure,
        metadata: serialize_payload(&error),
        data: VesperPluginBytes::default(),
    }
}

fn catch_decoder_bytes(f: impl FnOnce() -> VesperPluginBytes) -> VesperPluginBytes {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| serialize_payload(&plugin_panic_error()))
}

fn catch_decoder_open(
    f: impl FnOnce() -> VesperDecoderOpenSessionResult,
) -> VesperDecoderOpenSessionResult {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| open_error(plugin_panic_error()))
}

fn catch_decoder_process(
    f: impl FnOnce() -> VesperPluginProcessResult,
) -> VesperPluginProcessResult {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| process_error(plugin_panic_error()))
}

fn catch_decoder_native_frame(
    f: impl FnOnce() -> VesperDecoderReceiveNativeFrameResult,
) -> VesperDecoderReceiveNativeFrameResult {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| native_frame_error(plugin_panic_error()))
}

fn catch_decoder_pcm_frame(
    f: impl FnOnce() -> VesperDecoderReceivePcmFrameResult,
) -> VesperDecoderReceivePcmFrameResult {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| pcm_frame_error(plugin_panic_error()))
}

fn plugin_panic_error() -> DecoderError {
    DecoderError::internal("decoder plugin callback panicked")
}

#[cfg(test)]
mod tests {
    use super::{
        MediaCodecOutputBufferKind, MediaCodecPendingOutputEosAction, android_native_window_ptr,
        codec_config_buffers, codec_mime, decoder_capabilities, decoder_native_requirements,
        decoder_open_session_json, length_prefixed_sample_to_annex_b,
        mediacodec_output_buffer_kind, mediacodec_pending_output_eos_action,
        mediacodec_surface_texture_format, nal_length_size_for_config, packet_data_for_mediacodec,
        packet_input_data_for_mediacodec, split_avcc_extradata, split_hvcc_extradata,
        vesper_plugin_entry,
    };
    use player_plugin::{
        DecoderBitstreamFormat, DecoderError, DecoderFrameFormat, DecoderMediaKind,
        DecoderNativeDeviceContext, DecoderNativeDeviceContextKind, DecoderNativeHandleKind,
        DecoderPacket, DecoderSessionConfig, NativeFramePipelineProfile,
        VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT, VesperPluginKind, VesperPluginResultStatus,
    };

    #[test]
    fn exported_descriptor_matches_decoder_plugin_metadata() {
        // SAFETY: the MediaCodec entry point returns a process-lifetime
        // descriptor pointer or null; this test immediately borrows it.
        let descriptor = unsafe { vesper_plugin_entry().as_ref() }.expect("descriptor");

        assert_eq!(
            descriptor.abi_version,
            VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT
        );
        assert_eq!(descriptor.plugin_kind, VesperPluginKind::Decoder);
        assert!(!descriptor.api.is_null());
        assert!(!descriptor.plugin_name.is_null());
    }

    #[test]
    fn capabilities_advertise_android_mediacodec_video_contract() {
        let capabilities = decoder_capabilities();

        assert!(capabilities.supports_codec("h264", DecoderMediaKind::Video));
        assert!(capabilities.supports_codec("HEV1", DecoderMediaKind::Video));
        assert!(!capabilities.supports_audio_frames);
        assert!(!capabilities.supports_cpu_video_frames);
        assert_eq!(
            capabilities.supports_hardware_decode,
            cfg!(target_os = "android")
        );
        assert_eq!(
            capabilities.supports_gpu_handles,
            cfg!(target_os = "android")
        );
        for capability in &capabilities.codecs {
            assert_eq!(
                capability.output_formats,
                vec![DecoderFrameFormat::Unknown(
                    "mediacodec_surface_texture".to_owned()
                )]
            );
        }
    }

    #[test]
    fn session_info_reports_opaque_surface_texture_format() {
        assert_eq!(
            mediacodec_surface_texture_format(),
            DecoderFrameFormat::Unknown("mediacodec_surface_texture".to_owned())
        );
    }

    #[test]
    fn native_requirements_need_android_native_window_and_surface_output() {
        let requirements = decoder_native_requirements();

        assert_eq!(
            requirements.required_device_context_kinds,
            vec![DecoderNativeDeviceContextKind::AndroidNativeWindow]
        );
        assert_eq!(
            requirements.output_handle_kinds,
            vec![DecoderNativeHandleKind::MediaCodecSurfaceTexture]
        );
        assert_eq!(
            requirements.output_pipeline_profiles,
            vec![NativeFramePipelineProfile::MediaCodecSurfaceTexture]
        );
        assert!(requirements.requires_native_device_context);
        assert_eq!(
            requirements.accepted_bitstream_formats,
            vec![DecoderBitstreamFormat::Avcc, DecoderBitstreamFormat::Hvcc]
        );
    }

    #[test]
    fn native_requirements_do_not_advertise_hardware_buffer_before_runtime_support() {
        let requirements = decoder_native_requirements();

        assert!(
            !requirements
                .output_handle_kinds
                .contains(&DecoderNativeHandleKind::MediaCodecHardwareBuffer)
        );
        assert!(
            !requirements
                .output_pipeline_profiles
                .contains(&NativeFramePipelineProfile::MediaCodecHardwareBuffer)
        );
    }

    #[test]
    fn android_native_window_context_rejects_missing_or_wrong_context() {
        assert!(matches!(
            android_native_window_ptr(None),
            Err(DecoderError::UnsupportedCapability { .. })
        ));
        assert!(matches!(
            android_native_window_ptr(Some(&DecoderNativeDeviceContext::D3D11Device {
                device_ptr: 7,
            })),
            Err(DecoderError::UnsupportedCapability { .. })
        ));
        assert!(matches!(
            android_native_window_ptr(Some(&DecoderNativeDeviceContext::AndroidNativeWindow {
                window_ptr: 0,
            })),
            Err(DecoderError::AbiViolation { .. })
        ));
        assert_eq!(
            android_native_window_ptr(Some(&DecoderNativeDeviceContext::AndroidNativeWindow {
                window_ptr: 0x1234,
            })),
            Ok(0x1234)
        );
    }

    #[test]
    fn codec_mime_maps_supported_android_video_codecs() {
        assert_eq!(codec_mime("h264"), Some("video/avc"));
        assert_eq!(codec_mime("AVC1"), Some("video/avc"));
        assert_eq!(codec_mime("hevc"), Some("video/hevc"));
        assert_eq!(codec_mime("HVC1"), Some("video/hevc"));
        assert_eq!(codec_mime("vp9"), None);
    }

    #[test]
    fn avcc_extradata_splits_sps_and_pps_as_annex_b_csd_buffers() {
        let avcc = vec![
            1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, 4, 0x67, 0x64, 0, 0x1f, 1, 0, 2, 0x68, 0xeb,
        ];

        assert_eq!(
            split_avcc_extradata(&avcc),
            vec![
                vec![0, 0, 0, 1, 0x67, 0x64, 0, 0x1f],
                vec![0, 0, 0, 1, 0x68, 0xeb],
            ]
        );
    }

    #[test]
    fn hvcc_extradata_splits_parameter_arrays_as_annex_b_csd_buffers() {
        let hvcc = vec![
            1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0xa1, 0, 1, 0, 3,
            0x40, 0x01, 0x0c,
        ];

        assert_eq!(
            split_hvcc_extradata(&hvcc),
            vec![vec![0, 0, 0, 1, 0x40, 0x01, 0x0c]]
        );
    }

    #[test]
    fn codec_config_buffers_fall_back_to_raw_extradata_for_unknown_format() {
        let config = DecoderSessionConfig {
            codec: "h264".to_owned(),
            media_kind: DecoderMediaKind::Video,
            extradata: vec![1, 2, 3],
            bitstream_format: Some(DecoderBitstreamFormat::Unknown("fixture".to_owned())),
            ..DecoderSessionConfig::default()
        };

        assert_eq!(codec_config_buffers(&config), vec![vec![1, 2, 3]]);
    }

    #[test]
    fn mediacodec_packet_data_converts_avcc_samples_to_annex_b() {
        let config = DecoderSessionConfig {
            codec: "h264".to_owned(),
            media_kind: DecoderMediaKind::Video,
            extradata: vec![1, 0x64, 0, 0x1f, 0xff],
            bitstream_format: Some(DecoderBitstreamFormat::Avcc),
            ..DecoderSessionConfig::default()
        };
        let sample = vec![0, 0, 0, 3, 0x65, 0x88, 0x84, 0, 0, 0, 2, 0x41, 0x9a];

        let converted = packet_data_for_mediacodec(
            config.bitstream_format.as_ref(),
            nal_length_size_for_config(&config),
            &sample,
        )
        .expect("AVCC sample converts");

        assert_eq!(
            converted.as_ref(),
            &[0, 0, 0, 1, 0x65, 0x88, 0x84, 0, 0, 0, 1, 0x41, 0x9a]
        );
    }

    #[test]
    fn mediacodec_packet_data_keeps_existing_annex_b_samples_borrowed() {
        let sample = vec![0, 0, 0, 1, 0x65, 0x88, 0x84];
        let converted = packet_data_for_mediacodec(Some(&DecoderBitstreamFormat::Avcc), 4, &sample)
            .expect("Annex B sample is accepted");

        assert!(matches!(converted, std::borrow::Cow::Borrowed(_)));
        assert_eq!(converted.as_ref(), sample.as_slice());
    }

    #[test]
    fn mediacodec_packet_data_honors_hvcc_nal_length_size() {
        let mut extradata = vec![0; 22];
        extradata[0] = 1;
        extradata[21] = 0x01;
        let config = DecoderSessionConfig {
            codec: "hevc".to_owned(),
            media_kind: DecoderMediaKind::Video,
            extradata,
            bitstream_format: Some(DecoderBitstreamFormat::Hvcc),
            ..DecoderSessionConfig::default()
        };
        let sample = vec![0, 3, 0x26, 0x01, 0xaf];

        let converted = packet_data_for_mediacodec(
            config.bitstream_format.as_ref(),
            nal_length_size_for_config(&config),
            &sample,
        )
        .expect("HVCC sample converts");

        assert_eq!(converted.as_ref(), &[0, 0, 0, 1, 0x26, 0x01, 0xaf]);
    }

    #[test]
    fn mediacodec_eos_packet_queues_empty_input_data() {
        let packet = DecoderPacket {
            end_of_stream: true,
            ..DecoderPacket::default()
        };

        let input = packet_input_data_for_mediacodec(
            &packet,
            Some(&DecoderBitstreamFormat::Avcc),
            4,
            &[0, 0, 0, 1, 0x65],
        )
        .expect("EOS packet input");

        assert!(input.is_empty());
    }

    #[test]
    fn mediacodec_output_buffer_kind_keeps_final_frame_with_eos_flag() {
        assert_eq!(
            mediacodec_output_buffer_kind(4, 128),
            MediaCodecOutputBufferKind::Frame { pending_eos: true }
        );
        assert_eq!(
            mediacodec_output_buffer_kind(4, 0),
            MediaCodecOutputBufferKind::Eof
        );
        assert_eq!(
            mediacodec_output_buffer_kind(2, 0),
            MediaCodecOutputBufferKind::CodecConfig
        );
    }

    #[test]
    fn mediacodec_pending_output_eos_waits_for_last_frame_release() {
        assert_eq!(
            mediacodec_pending_output_eos_action(1),
            MediaCodecPendingOutputEosAction::WaitForOutstandingRelease
        );
        assert_eq!(
            mediacodec_pending_output_eos_action(0),
            MediaCodecPendingOutputEosAction::ReportEof
        );
    }

    #[test]
    fn length_prefixed_sample_rejects_truncated_nal() {
        let error = length_prefixed_sample_to_annex_b(&[0, 0, 0, 4, 0x65], 4)
            .expect_err("truncated sample is invalid");

        assert!(matches!(error, DecoderError::InvalidPacket { .. }));
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn open_session_reports_unsupported_outside_android() {
        let config = DecoderSessionConfig {
            codec: "h264".to_owned(),
            media_kind: DecoderMediaKind::Video,
            native_device_context: Some(DecoderNativeDeviceContext::AndroidNativeWindow {
                window_ptr: 0x1234,
            }),
            ..DecoderSessionConfig::default()
        };
        let config_json = serde_json::to_vec(&config).expect("config json");

        // SAFETY: all pointers passed to the callback are valid for this
        // synchronous test call.
        let result = unsafe {
            decoder_open_session_json(
                std::ptr::null_mut(),
                config_json.as_ptr(),
                config_json.len(),
            )
        };

        assert_eq!(result.status, VesperPluginResultStatus::Failure);
        assert!(result.session.is_null());
        // SAFETY: the payload was produced by this plugin in the current
        // dynamic library and the test consumes it once.
        let payload = unsafe { result.payload.into_vec() };
        let error = serde_json::from_slice::<DecoderError>(&payload).expect("decoder error");
        assert!(matches!(
            error,
            DecoderError::UnsupportedCapability { capability }
                if capability == "android-mediacodec-decoder"
        ));
    }
}
