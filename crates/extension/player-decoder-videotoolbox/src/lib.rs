use std::ffi::{c_char, c_void};

use player_plugin::{
    DecoderCapabilities, DecoderCodecCapability, DecoderError, DecoderFrameFormat,
    DecoderMediaKind, DecoderOperationStatus, DecoderPacket, DecoderPacketResult,
    DecoderReceiveNativeFrameMetadata, DecoderSessionConfig, DecoderSessionInfo,
    VESPER_DECODER_PLUGIN_ABI_VERSION_V2, VesperDecoderOpenSessionResult, VesperDecoderPluginApiV2,
    VesperDecoderReceiveNativeFrameResult, VesperPluginBytes, VesperPluginDescriptor,
    VesperPluginKind, VesperPluginProcessResult, VesperPluginResultStatus,
};

static PLUGIN_NAME: &[u8] = b"player-decoder-videotoolbox\0";

struct PluginBundle {
    api: VesperDecoderPluginApiV2,
    descriptor: VesperPluginDescriptor,
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_plugin_entry() -> *const VesperPluginDescriptor {
    let mut bundle = Box::new(PluginBundle {
        api: VesperDecoderPluginApiV2 {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(decoder_name),
            capabilities_json: Some(decoder_capabilities_json),
            free_bytes: Some(free_plugin_bytes),
            open_session_json: Some(decoder_open_session_json),
            send_packet: Some(decoder_send_packet),
            receive_native_frame: Some(decoder_receive_native_frame),
            release_native_frame: Some(decoder_release_native_frame),
            flush_session: Some(decoder_flush_session),
            close_session: Some(decoder_close_session),
        },
        descriptor: VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: PLUGIN_NAME.as_ptr().cast::<c_char>(),
            api: std::ptr::null(),
        },
    });
    bundle.descriptor.api = (&bundle.api as *const VesperDecoderPluginApiV2).cast::<c_void>();
    let bundle = Box::leak(bundle);
    &bundle.descriptor
}

unsafe extern "C" fn decoder_name(_context: *mut c_void) -> *const c_char {
    PLUGIN_NAME.as_ptr().cast::<c_char>()
}

unsafe extern "C" fn decoder_capabilities_json(_context: *mut c_void) -> VesperPluginBytes {
    serialize_payload(&decoder_capabilities())
}

unsafe extern "C" fn decoder_open_session_json(
    _context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperDecoderOpenSessionResult {
    let config = match decode_json::<DecoderSessionConfig>(config_json, config_json_len) {
        Ok(config) => config,
        Err(error) => return open_error(error),
    };
    if !decoder_capabilities().supports_codec(&config.codec, config.media_kind) {
        return open_error(DecoderError::UnsupportedCodec {
            codec: config.codec,
        });
    }

    platform::open_session(config)
}

unsafe extern "C" fn decoder_send_packet(
    _context: *mut c_void,
    session: *mut c_void,
    packet_json: *const u8,
    packet_json_len: usize,
    packet_data: *const u8,
    packet_data_len: usize,
) -> VesperPluginProcessResult {
    let packet = match decode_json::<DecoderPacket>(packet_json, packet_json_len) {
        Ok(packet) => packet,
        Err(error) => return process_error(error),
    };
    let data = if packet_data.is_null() || packet_data_len == 0 {
        &[]
    } else {
        // SAFETY: host passes a borrowed packet byte range that is valid for
        // this call; the plugin copies it before returning when needed.
        unsafe { std::slice::from_raw_parts(packet_data, packet_data_len) }
    };

    platform::send_packet(session, &packet, data)
}

unsafe extern "C" fn decoder_receive_native_frame(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperDecoderReceiveNativeFrameResult {
    platform::receive_native_frame(session)
}

unsafe extern "C" fn decoder_release_native_frame(
    _context: *mut c_void,
    session: *mut c_void,
    handle_kind: u32,
    handle: usize,
) -> VesperPluginProcessResult {
    platform::release_native_frame(session, handle_kind, handle)
}

unsafe extern "C" fn decoder_flush_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    platform::flush_session(session)
}

unsafe extern "C" fn decoder_close_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    platform::close_session(session)
}

unsafe extern "C" fn free_plugin_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
    // SAFETY: payloads returned by this plugin are allocated from Vec<u8> with
    // capacity equal to len in this dynamic library.
    unsafe {
        let _ = payload.into_vec();
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
        supports_hardware_decode: cfg!(target_os = "macos"),
        supports_cpu_video_frames: false,
        supports_audio_frames: false,
        supports_gpu_handles: cfg!(target_os = "macos"),
        supports_flush: true,
        supports_drain: true,
        max_sessions: None,
    }
}

fn video_codec_capability(codec: &str) -> DecoderCodecCapability {
    DecoderCodecCapability {
        codec: codec.to_owned(),
        media_kind: DecoderMediaKind::Video,
        profiles: Vec::new(),
        output_formats: vec![DecoderFrameFormat::Nv12],
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

fn open_error(error: DecoderError) -> VesperDecoderOpenSessionResult {
    VesperDecoderOpenSessionResult {
        status: VesperPluginResultStatus::Failure,
        session: std::ptr::null_mut(),
        payload: serialize_payload(&error),
    }
}

fn open_success(session: *mut c_void, info: &DecoderSessionInfo) -> VesperDecoderOpenSessionResult {
    VesperDecoderOpenSessionResult {
        status: VesperPluginResultStatus::Success,
        session,
        payload: serialize_payload(info),
    }
}

fn process_success(status: &DecoderOperationStatus) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: serialize_payload(status),
    }
}

fn packet_success(result: &DecoderPacketResult) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: serialize_payload(result),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoCodecKind {
    H264,
    Hevc,
}

fn video_codec_kind(codec: &str) -> Option<VideoCodecKind> {
    if codec.eq_ignore_ascii_case("h264") || codec.eq_ignore_ascii_case("avc1") {
        Some(VideoCodecKind::H264)
    } else if codec.eq_ignore_ascii_case("hevc")
        || codec.eq_ignore_ascii_case("h265")
        || codec.eq_ignore_ascii_case("hvc1")
        || codec.eq_ignore_ascii_case("hev1")
    {
        Some(VideoCodecKind::Hevc)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::collections::VecDeque;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::{Arc, Mutex};

    use player_plugin::{
        DecoderError, DecoderFrameFormat, DecoderMediaKind, DecoderNativeFrameMetadata,
        DecoderNativeHandleKind, DecoderPacket, DecoderPacketResult,
        DecoderReceiveNativeFrameMetadata, DecoderSessionConfig, DecoderSessionInfo,
        VesperDecoderOpenSessionResult, VesperDecoderReceiveNativeFrameResult,
        VesperPluginProcessResult,
    };

    use super::{
        DecoderOperationStatus, VideoCodecKind, native_frame_error, native_frame_success,
        open_error, open_success, packet_success, process_error, process_success, video_codec_kind,
    };

    type OSStatus = i32;
    type CFTypeRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFIndex = isize;
    type CFNumberRef = *const c_void;
    type CFStringRef = *const c_void;
    type CMFormatDescriptionRef = *mut c_void;
    type CMBlockBufferRef = *mut c_void;
    type CMSampleBufferRef = *mut c_void;
    type CVImageBufferRef = *mut c_void;
    type CVPixelBufferRef = *mut c_void;
    type VTDecompressionSessionRef = *mut c_void;

    const NO_ERR: OSStatus = 0;
    const CM_TIME_FLAGS_VALID: u32 = 1;
    const CV_PIXEL_BUFFER_HANDLE_KIND: u32 = 1;
    const K_CF_NUMBER_SINT32_TYPE: i32 = 3;
    const K_CV_PIXEL_FORMAT_TYPE_420YPCBCR8_BIPLANAR_VIDEO_RANGE: i32 = 875_704_438;

    #[repr(C)]
    struct CFDictionaryKeyCallBacks {
        version: CFIndex,
        retain: Option<unsafe extern "C" fn(CFAllocatorRef, *const c_void) -> *const c_void>,
        release: Option<unsafe extern "C" fn(CFAllocatorRef, *const c_void)>,
        copy_description: Option<unsafe extern "C" fn(*const c_void) -> CFStringRef>,
        equal: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> bool>,
        hash: Option<unsafe extern "C" fn(*const c_void) -> usize>,
    }

    #[repr(C)]
    struct CFDictionaryValueCallBacks {
        version: CFIndex,
        retain: Option<unsafe extern "C" fn(CFAllocatorRef, *const c_void) -> *const c_void>,
        release: Option<unsafe extern "C" fn(CFAllocatorRef, *const c_void)>,
        copy_description: Option<unsafe extern "C" fn(*const c_void) -> CFStringRef>,
        equal: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> bool>,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    struct CMTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    struct CMSampleTimingInfo {
        duration: CMTime,
        presentation_time_stamp: CMTime,
        decode_time_stamp: CMTime,
    }

    #[repr(C)]
    struct VTDecompressionOutputCallbackRecord {
        decompression_output_callback: Option<
            unsafe extern "C" fn(
                decompression_output_ref_con: *mut c_void,
                source_frame_ref_con: *mut c_void,
                status: OSStatus,
                info_flags: u32,
                image_buffer: CVImageBufferRef,
                presentation_time_stamp: CMTime,
                presentation_duration: CMTime,
            ),
        >,
        decompression_output_ref_con: *mut c_void,
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
        static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks;
        fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
        fn CFRelease(cf: CFTypeRef);
        fn CFNumberCreate(
            allocator: CFAllocatorRef,
            the_type: i32,
            value_ptr: *const c_void,
        ) -> CFNumberRef;
        fn CFDictionaryCreate(
            allocator: CFAllocatorRef,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: CFIndex,
            key_callbacks: *const CFDictionaryKeyCallBacks,
            value_callbacks: *const CFDictionaryValueCallBacks,
        ) -> CFDictionaryRef;
    }

    #[link(name = "CoreVideo", kind = "framework")]
    unsafe extern "C" {
        static kCVPixelBufferPixelFormatTypeKey: CFStringRef;
        static kCVPixelBufferIOSurfacePropertiesKey: CFStringRef;
        fn CVPixelBufferGetWidth(pixel_buffer: CVPixelBufferRef) -> usize;
        fn CVPixelBufferGetHeight(pixel_buffer: CVPixelBufferRef) -> usize;
    }

    #[link(name = "CoreMedia", kind = "framework")]
    unsafe extern "C" {
        fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
            allocator: CFAllocatorRef,
            parameter_set_count: usize,
            parameter_set_pointers: *const *const u8,
            parameter_set_sizes: *const usize,
            nal_unit_header_length: i32,
            format_description_out: *mut CMFormatDescriptionRef,
        ) -> OSStatus;

        fn CMVideoFormatDescriptionCreateFromHEVCParameterSets(
            allocator: CFAllocatorRef,
            parameter_set_count: usize,
            parameter_set_pointers: *const *const u8,
            parameter_set_sizes: *const usize,
            nal_unit_header_length: i32,
            extensions: CFDictionaryRef,
            format_description_out: *mut CMFormatDescriptionRef,
        ) -> OSStatus;

        fn CMBlockBufferCreateWithMemoryBlock(
            structure_allocator: CFAllocatorRef,
            memory_block: *mut c_void,
            block_length: usize,
            block_allocator: CFAllocatorRef,
            custom_block_source: *const c_void,
            offset_to_data: usize,
            data_length: usize,
            flags: u32,
            block_buffer_out: *mut CMBlockBufferRef,
        ) -> OSStatus;

        fn CMBlockBufferReplaceDataBytes(
            source_bytes: *const c_void,
            destination_buffer: CMBlockBufferRef,
            offset_into_destination: usize,
            data_length: usize,
        ) -> OSStatus;

        fn CMSampleBufferCreateReady(
            allocator: CFAllocatorRef,
            data_buffer: CMBlockBufferRef,
            format_description: CMFormatDescriptionRef,
            num_samples: isize,
            num_sample_timing_entries: isize,
            sample_timing_array: *const CMSampleTimingInfo,
            num_sample_size_entries: isize,
            sample_size_array: *const usize,
            sample_buffer_out: *mut CMSampleBufferRef,
        ) -> OSStatus;
    }

    #[link(name = "VideoToolbox", kind = "framework")]
    unsafe extern "C" {
        fn VTDecompressionSessionCreate(
            allocator: CFAllocatorRef,
            video_format_description: CMFormatDescriptionRef,
            video_decoder_specification: CFDictionaryRef,
            destination_image_buffer_attributes: CFDictionaryRef,
            output_callback: *const VTDecompressionOutputCallbackRecord,
            decompression_session_out: *mut VTDecompressionSessionRef,
        ) -> OSStatus;

        fn VTDecompressionSessionDecodeFrame(
            session: VTDecompressionSessionRef,
            sample_buffer: CMSampleBufferRef,
            decode_flags: u32,
            source_frame_ref_con: *mut c_void,
            info_flags_out: *mut u32,
        ) -> OSStatus;

        fn VTDecompressionSessionWaitForAsynchronousFrames(
            session: VTDecompressionSessionRef,
        ) -> OSStatus;

        fn VTDecompressionSessionFinishDelayedFrames(
            session: VTDecompressionSessionRef,
        ) -> OSStatus;

        fn VTDecompressionSessionInvalidate(session: VTDecompressionSessionRef);
    }

    pub fn open_session(config: DecoderSessionConfig) -> VesperDecoderOpenSessionResult {
        let Some(codec) = video_codec_kind(&config.codec) else {
            return open_error(DecoderError::UnsupportedCodec {
                codec: config.codec,
            });
        };

        let session = match VideoToolboxDecoderSession::new(config, codec) {
            Ok(session) => session,
            Err(error) => return open_error(error),
        };
        let info = session.session_info();
        let session = Box::into_raw(Box::new(session)).cast::<c_void>();
        open_success(session, &info)
    }

    pub fn send_packet(
        session: *mut c_void,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> VesperPluginProcessResult {
        let Some(session) = (unsafe { session.cast::<VideoToolboxDecoderSession>().as_mut() })
        else {
            return process_error(DecoderError::NotConfigured);
        };

        match session.send_packet(packet, data) {
            Ok(result) => packet_success(&result),
            Err(error) => process_error(error),
        }
    }

    pub fn receive_native_frame(session: *mut c_void) -> VesperDecoderReceiveNativeFrameResult {
        let Some(session) = (unsafe { session.cast::<VideoToolboxDecoderSession>().as_mut() })
        else {
            return native_frame_error(DecoderError::NotConfigured);
        };

        match session.receive_native_frame() {
            Ok(NativeReceiveResult::Frame(frame)) => native_frame_success(
                &DecoderReceiveNativeFrameMetadata::frame(frame.metadata),
                frame.handle,
            ),
            Ok(NativeReceiveResult::NeedMoreInput) => {
                native_frame_success(&DecoderReceiveNativeFrameMetadata::need_more_input(), 0)
            }
            Ok(NativeReceiveResult::Eof) => {
                native_frame_success(&DecoderReceiveNativeFrameMetadata::eof(), 0)
            }
            Err(error) => native_frame_error(error),
        }
    }

    pub fn release_native_frame(
        session: *mut c_void,
        handle_kind: u32,
        handle: usize,
    ) -> VesperPluginProcessResult {
        if handle_kind != CV_PIXEL_BUFFER_HANDLE_KIND {
            return process_error(DecoderError::abi_violation(format!(
                "VideoToolbox plugin expected CVPixelBuffer handle kind, got {handle_kind}"
            )));
        }
        if handle == 0 {
            return process_error(DecoderError::abi_violation(
                "VideoToolbox plugin received a null native frame handle",
            ));
        }
        if session.is_null() {
            return process_error(DecoderError::NotConfigured);
        }

        // SAFETY: ownership of this retained CVPixelBuffer handle was
        // transferred to the host by receive_native_frame.
        unsafe { CFRelease(handle as CFTypeRef) };
        process_success(&DecoderOperationStatus { completed: true })
    }

    pub fn flush_session(session: *mut c_void) -> VesperPluginProcessResult {
        let Some(session) = (unsafe { session.cast::<VideoToolboxDecoderSession>().as_mut() })
        else {
            return process_error(DecoderError::NotConfigured);
        };
        match session.flush() {
            Ok(()) => process_success(&DecoderOperationStatus { completed: true }),
            Err(error) => process_error(error),
        }
    }

    pub fn close_session(session: *mut c_void) -> VesperPluginProcessResult {
        if session.is_null() {
            return process_error(DecoderError::NotConfigured);
        }
        // SAFETY: the session pointer was returned by open_session and is
        // consumed exactly once by the loader close path.
        let mut session = unsafe { Box::from_raw(session.cast::<VideoToolboxDecoderSession>()) };
        match session.close() {
            Ok(()) => process_success(&DecoderOperationStatus { completed: true }),
            Err(error) => process_error(error),
        }
    }

    struct VideoToolboxDecoderSession {
        codec: VideoCodecKind,
        codec_name: String,
        width: u32,
        height: u32,
        nal_length_size: usize,
        parameter_sets: Vec<Vec<u8>>,
        format_description: CMFormatDescriptionRef,
        decompression_session: VTDecompressionSessionRef,
        callback_state: Arc<CallbackState>,
        end_of_stream_sent: bool,
        closed: bool,
    }

    // VideoToolbox/CoreFoundation refs are retained and released by this
    // session; access is serialized by the host-side decoder session.
    unsafe impl Send for VideoToolboxDecoderSession {}

    impl VideoToolboxDecoderSession {
        fn new(config: DecoderSessionConfig, codec: VideoCodecKind) -> Result<Self, DecoderError> {
            let mut parsed = if config.extradata.is_empty() {
                None
            } else {
                Some(parse_extradata(codec, &config.extradata)?)
            };
            let mut session = Self {
                codec,
                codec_name: config.codec,
                width: config.width.unwrap_or_default(),
                height: config.height.unwrap_or_default(),
                nal_length_size: parsed.as_ref().map_or(4, |parsed| parsed.nal_length_size),
                parameter_sets: parsed
                    .take()
                    .map(|parsed| parsed.parameter_sets)
                    .unwrap_or_default(),
                format_description: ptr::null_mut(),
                decompression_session: ptr::null_mut(),
                callback_state: Arc::new(CallbackState::default()),
                end_of_stream_sent: false,
                closed: false,
            };
            if !session.parameter_sets.is_empty() {
                session.create_decompression_session()?;
            }
            Ok(session)
        }

        fn session_info(&self) -> DecoderSessionInfo {
            DecoderSessionInfo {
                decoder_name: Some("player-decoder-videotoolbox".to_owned()),
                selected_hardware_backend: Some("VideoToolbox".to_owned()),
                output_format: Some(DecoderFrameFormat::Nv12),
            }
        }

        fn send_packet(
            &mut self,
            packet: &DecoderPacket,
            data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            if self.closed {
                return Err(DecoderError::NotConfigured);
            }
            if packet.discontinuity {
                self.flush()?;
            }
            if packet.end_of_stream {
                if !self.decompression_session.is_null() {
                    // SAFETY: the session belongs to this object.
                    let finish_status = unsafe {
                        VTDecompressionSessionFinishDelayedFrames(self.decompression_session)
                    };
                    os_status_result("VTDecompressionSessionFinishDelayedFrames", finish_status)?;
                    // SAFETY: the session belongs to this object.
                    let status = unsafe {
                        VTDecompressionSessionWaitForAsynchronousFrames(self.decompression_session)
                    };
                    os_status_result("VTDecompressionSessionWaitForAsynchronousFrames", status)?;
                }
                self.end_of_stream_sent = true;
                return Ok(DecoderPacketResult { accepted: true });
            }
            if data.is_empty() {
                return Ok(DecoderPacketResult { accepted: false });
            }

            self.ensure_decompression_session(data)?;
            let sample_data = self.normalized_sample_data(data)?;
            if sample_data.is_empty() {
                return Ok(DecoderPacketResult { accepted: false });
            }
            let sample_buffer = create_sample_buffer(
                self.format_description,
                &sample_data,
                packet.pts_us,
                packet.dts_us,
                packet.duration_us,
            )?;
            let mut info_flags = 0_u32;
            // SAFETY: the VideoToolbox session and sample buffer were created
            // by this plugin and remain valid for the duration of the call.
            let decode_status = unsafe {
                VTDecompressionSessionDecodeFrame(
                    self.decompression_session,
                    sample_buffer,
                    0,
                    ptr::null_mut(),
                    &mut info_flags,
                )
            };
            // SAFETY: sample_buffer is a retained CoreFoundation object from
            // create_sample_buffer.
            unsafe { CFRelease(sample_buffer as CFTypeRef) };
            os_status_result("VTDecompressionSessionDecodeFrame", decode_status)?;
            // SAFETY: waiting after each submitted frame keeps the v2 ABI simple
            // until the host owns an async native-frame queue.
            let wait_status = unsafe {
                VTDecompressionSessionWaitForAsynchronousFrames(self.decompression_session)
            };
            os_status_result(
                "VTDecompressionSessionWaitForAsynchronousFrames",
                wait_status,
            )?;
            Ok(DecoderPacketResult { accepted: true })
        }

        fn receive_native_frame(&mut self) -> Result<NativeReceiveResult, DecoderError> {
            let mut frames = self
                .callback_state
                .frames
                .lock()
                .map_err(|_| DecoderError::internal("VideoToolbox frame queue is poisoned"))?;
            let Some(frame) = frames.pop_front() else {
                return if self.end_of_stream_sent {
                    Ok(NativeReceiveResult::Eof)
                } else {
                    Ok(NativeReceiveResult::NeedMoreInput)
                };
            };
            Ok(NativeReceiveResult::Frame(NativeFrame {
                metadata: DecoderNativeFrameMetadata {
                    media_kind: DecoderMediaKind::Video,
                    format: DecoderFrameFormat::Nv12,
                    codec: self.codec_name.clone(),
                    pts_us: frame.pts_us,
                    duration_us: frame.duration_us,
                    width: if frame.width == 0 {
                        self.width
                    } else {
                        frame.width
                    },
                    height: if frame.height == 0 {
                        self.height
                    } else {
                        frame.height
                    },
                    handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
                },
                handle: frame.pixel_buffer as usize,
            }))
        }

        fn flush(&mut self) -> Result<(), DecoderError> {
            self.end_of_stream_sent = false;
            if !self.decompression_session.is_null() {
                // SAFETY: the session belongs to this object.
                let status = unsafe {
                    VTDecompressionSessionWaitForAsynchronousFrames(self.decompression_session)
                };
                os_status_result("VTDecompressionSessionWaitForAsynchronousFrames", status)?;
            }
            self.release_queued_frames();
            Ok(())
        }

        fn close(&mut self) -> Result<(), DecoderError> {
            if self.closed {
                return Ok(());
            }
            let flush_result = self.flush();
            if !self.decompression_session.is_null() {
                // SAFETY: the session belongs to this object and is invalidated
                // once before its CoreFoundation reference is released.
                unsafe {
                    VTDecompressionSessionInvalidate(self.decompression_session);
                    CFRelease(self.decompression_session as CFTypeRef);
                }
                self.decompression_session = ptr::null_mut();
            }
            if !self.format_description.is_null() {
                // SAFETY: format_description is retained by creation.
                unsafe { CFRelease(self.format_description as CFTypeRef) };
                self.format_description = ptr::null_mut();
            }
            self.release_queued_frames();
            self.closed = true;
            flush_result
        }

        fn ensure_decompression_session(&mut self, data: &[u8]) -> Result<(), DecoderError> {
            if !self.decompression_session.is_null() {
                return Ok(());
            }
            if self.parameter_sets.is_empty() {
                let Some(parsed) = parse_annexb_parameter_sets(self.codec, data) else {
                    return Err(DecoderError::InvalidPacket {
                        message:
                            "VideoToolbox session requires H264/HEVC parameter sets before decoding"
                                .to_owned(),
                    });
                };
                self.nal_length_size = parsed.nal_length_size;
                self.parameter_sets = parsed.parameter_sets;
            }
            self.create_decompression_session()
        }

        fn create_decompression_session(&mut self) -> Result<(), DecoderError> {
            if self.parameter_sets.is_empty() {
                return Err(DecoderError::InvalidPacket {
                    message: "missing VideoToolbox parameter sets".to_owned(),
                });
            }
            let format_description =
                create_format_description(self.codec, self.nal_length_size, &self.parameter_sets)?;
            let callback = VTDecompressionOutputCallbackRecord {
                decompression_output_callback: Some(decompression_output_callback),
                decompression_output_ref_con: Arc::as_ptr(&self.callback_state).cast_mut().cast(),
            };
            let pixel_buffer_attributes = create_pixel_buffer_attributes()?;
            let mut decompression_session = ptr::null_mut();
            // SAFETY: format_description and callback are valid for the call.
            let status = unsafe {
                VTDecompressionSessionCreate(
                    ptr::null(),
                    format_description,
                    ptr::null(),
                    pixel_buffer_attributes,
                    &callback,
                    &mut decompression_session,
                )
            };
            // SAFETY: pixel_buffer_attributes was created by CoreFoundation for
            // this session creation call.
            unsafe { CFRelease(pixel_buffer_attributes as CFTypeRef) };
            if status != NO_ERR {
                // SAFETY: format_description was created by CoreMedia.
                unsafe { CFRelease(format_description as CFTypeRef) };
                return Err(os_status_error("VTDecompressionSessionCreate", status));
            }
            if !self.format_description.is_null() {
                // SAFETY: replacing a previous retained format description.
                unsafe { CFRelease(self.format_description as CFTypeRef) };
            }
            self.format_description = format_description;
            self.decompression_session = decompression_session;
            Ok(())
        }

        fn normalized_sample_data(&self, data: &[u8]) -> Result<Vec<u8>, DecoderError> {
            if has_annexb_start_code(data) {
                annexb_to_length_prefixed(data, self.nal_length_size)
            } else {
                Ok(data.to_vec())
            }
        }

        fn release_queued_frames(&mut self) {
            if let Ok(mut frames) = self.callback_state.frames.lock() {
                while let Some(frame) = frames.pop_front() {
                    // SAFETY: queued frames are retained in the callback and
                    // still owned by this session.
                    unsafe { CFRelease(frame.pixel_buffer as CFTypeRef) };
                }
            }
        }
    }

    impl Drop for VideoToolboxDecoderSession {
        fn drop(&mut self) {
            let _ = self.close();
        }
    }

    #[derive(Default)]
    struct CallbackState {
        frames: Mutex<VecDeque<PendingNativeFrame>>,
    }

    struct PendingNativeFrame {
        pixel_buffer: CVPixelBufferRef,
        pts_us: Option<i64>,
        duration_us: Option<i64>,
        width: u32,
        height: u32,
    }

    unsafe impl Send for PendingNativeFrame {}

    struct NativeFrame {
        metadata: DecoderNativeFrameMetadata,
        handle: usize,
    }

    enum NativeReceiveResult {
        Frame(NativeFrame),
        NeedMoreInput,
        Eof,
    }

    unsafe extern "C" fn decompression_output_callback(
        decompression_output_ref_con: *mut c_void,
        _source_frame_ref_con: *mut c_void,
        status: OSStatus,
        _info_flags: u32,
        image_buffer: CVImageBufferRef,
        presentation_time_stamp: CMTime,
        presentation_duration: CMTime,
    ) {
        if status != NO_ERR || image_buffer.is_null() || decompression_output_ref_con.is_null() {
            return;
        }
        let callback_state = unsafe { &*(decompression_output_ref_con.cast::<CallbackState>()) };
        // SAFETY: VideoToolbox provides a valid image buffer for this callback;
        // retaining transfers frame ownership into the plugin queue.
        let retained = unsafe { CFRetain(image_buffer as CFTypeRef) }.cast_mut();
        if retained.is_null() {
            return;
        }
        let pixel_buffer = retained.cast::<c_void>();
        let width =
            u32::try_from(unsafe { CVPixelBufferGetWidth(pixel_buffer) }).unwrap_or(u32::MAX);
        let height =
            u32::try_from(unsafe { CVPixelBufferGetHeight(pixel_buffer) }).unwrap_or(u32::MAX);
        let frame = PendingNativeFrame {
            pixel_buffer,
            pts_us: cm_time_to_us(presentation_time_stamp),
            duration_us: cm_time_to_us(presentation_duration),
            width,
            height,
        };
        match callback_state.frames.lock() {
            Ok(mut frames) => frames.push_back(frame),
            Err(_) => {
                // SAFETY: release the retain above if the queue is unavailable.
                unsafe { CFRelease(pixel_buffer as CFTypeRef) };
            }
        }
    }

    struct ParsedVideoConfig {
        nal_length_size: usize,
        parameter_sets: Vec<Vec<u8>>,
    }

    fn parse_extradata(
        codec: VideoCodecKind,
        extradata: &[u8],
    ) -> Result<ParsedVideoConfig, DecoderError> {
        if has_annexb_start_code(extradata) {
            return parse_annexb_parameter_sets(codec, extradata).ok_or_else(|| {
                DecoderError::InvalidPacket {
                    message: "extradata did not contain complete Annex B parameter sets".to_owned(),
                }
            });
        }
        match codec {
            VideoCodecKind::H264 => parse_avcc_extradata(extradata),
            VideoCodecKind::Hevc => parse_hvcc_extradata(extradata),
        }
    }

    fn parse_avcc_extradata(extradata: &[u8]) -> Result<ParsedVideoConfig, DecoderError> {
        if extradata.len() < 7 || extradata[0] != 1 {
            return Err(DecoderError::InvalidPacket {
                message: "H264 extradata is not an AVCDecoderConfigurationRecord".to_owned(),
            });
        }
        let nal_length_size = usize::from((extradata[4] & 0x03) + 1);
        let mut offset = 5;
        let sps_count = usize::from(extradata[offset] & 0x1f);
        offset += 1;
        let mut parameter_sets = Vec::new();
        for _ in 0..sps_count {
            parameter_sets.push(read_len_prefixed_parameter_set(extradata, &mut offset)?);
        }
        if offset >= extradata.len() {
            return Err(DecoderError::InvalidPacket {
                message: "H264 extradata is missing PPS entries".to_owned(),
            });
        }
        let pps_count = usize::from(extradata[offset]);
        offset += 1;
        for _ in 0..pps_count {
            parameter_sets.push(read_len_prefixed_parameter_set(extradata, &mut offset)?);
        }
        require_parameter_sets(VideoCodecKind::H264, &parameter_sets)?;
        Ok(ParsedVideoConfig {
            nal_length_size,
            parameter_sets,
        })
    }

    fn parse_hvcc_extradata(extradata: &[u8]) -> Result<ParsedVideoConfig, DecoderError> {
        if extradata.len() < 23 || extradata[0] != 1 {
            return Err(DecoderError::InvalidPacket {
                message: "HEVC extradata is not an HEVCDecoderConfigurationRecord".to_owned(),
            });
        }
        let nal_length_size = usize::from((extradata[21] & 0x03) + 1);
        let array_count = usize::from(extradata[22]);
        let mut offset = 23;
        let mut parameter_sets = Vec::new();
        for _ in 0..array_count {
            if offset + 3 > extradata.len() {
                return Err(DecoderError::InvalidPacket {
                    message: "HEVC extradata array header is truncated".to_owned(),
                });
            }
            let nal_type = extradata[offset] & 0x3f;
            offset += 1;
            let nal_count = read_u16(extradata, &mut offset)?;
            for _ in 0..nal_count {
                let parameter_set = read_len_prefixed_parameter_set(extradata, &mut offset)?;
                if matches!(nal_type, 32..=34) {
                    parameter_sets.push(parameter_set);
                }
            }
        }
        let parameter_sets = primary_parameter_sets_by_type(VideoCodecKind::Hevc, parameter_sets);
        require_parameter_sets(VideoCodecKind::Hevc, &parameter_sets)?;
        Ok(ParsedVideoConfig {
            nal_length_size,
            parameter_sets,
        })
    }

    fn read_len_prefixed_parameter_set(
        data: &[u8],
        offset: &mut usize,
    ) -> Result<Vec<u8>, DecoderError> {
        let len = usize::from(read_u16(data, offset)?);
        if *offset + len > data.len() {
            return Err(DecoderError::InvalidPacket {
                message: "parameter set length exceeds payload".to_owned(),
            });
        }
        let parameter_set = data[*offset..*offset + len].to_vec();
        *offset += len;
        if parameter_set.is_empty() {
            return Err(DecoderError::InvalidPacket {
                message: "parameter set is empty".to_owned(),
            });
        }
        Ok(parameter_set)
    }

    fn read_u16(data: &[u8], offset: &mut usize) -> Result<u16, DecoderError> {
        if *offset + 2 > data.len() {
            return Err(DecoderError::InvalidPacket {
                message: "payload is truncated while reading u16".to_owned(),
            });
        }
        let value = u16::from_be_bytes([data[*offset], data[*offset + 1]]);
        *offset += 2;
        Ok(value)
    }

    fn parse_annexb_parameter_sets(
        codec: VideoCodecKind,
        data: &[u8],
    ) -> Option<ParsedVideoConfig> {
        let parameter_sets = annexb_nalus(data)
            .into_iter()
            .filter(|nal| is_parameter_set(codec, nal))
            .map(|nal| nal.to_vec())
            .collect::<Vec<_>>();
        let parameter_sets = primary_parameter_sets_by_type(codec, parameter_sets);
        require_parameter_sets(codec, &parameter_sets).ok()?;
        Some(ParsedVideoConfig {
            nal_length_size: 4,
            parameter_sets,
        })
    }

    fn require_parameter_sets(
        codec: VideoCodecKind,
        parameter_sets: &[Vec<u8>],
    ) -> Result<(), DecoderError> {
        let has_type = |nal_type| {
            parameter_sets
                .iter()
                .any(|parameter_set| nal_unit_type(codec, parameter_set) == Some(nal_type))
        };
        let complete = match codec {
            VideoCodecKind::H264 => has_type(7) && has_type(8),
            VideoCodecKind::Hevc => has_type(32) && has_type(33) && has_type(34),
        };
        if complete {
            Ok(())
        } else {
            Err(DecoderError::InvalidPacket {
                message: "missing required H264/HEVC parameter sets".to_owned(),
            })
        }
    }

    fn primary_parameter_sets_by_type(
        codec: VideoCodecKind,
        parameter_sets: Vec<Vec<u8>>,
    ) -> Vec<Vec<u8>> {
        let mut selected: Vec<Vec<u8>> = Vec::new();
        for parameter_set in parameter_sets {
            let Some(nal_type) = nal_unit_type(codec, &parameter_set) else {
                continue;
            };
            if selected
                .iter()
                .any(|selected| nal_unit_type(codec, selected) == Some(nal_type))
            {
                continue;
            }
            selected.push(parameter_set);
        }
        selected
    }

    fn is_parameter_set(codec: VideoCodecKind, nal: &[u8]) -> bool {
        matches!(
            (codec, nal_unit_type(codec, nal)),
            (VideoCodecKind::H264, Some(7 | 8)) | (VideoCodecKind::Hevc, Some(32 | 33 | 34))
        )
    }

    fn nal_unit_type(codec: VideoCodecKind, nal: &[u8]) -> Option<u8> {
        match codec {
            VideoCodecKind::H264 => nal.first().map(|byte| byte & 0x1f),
            VideoCodecKind::Hevc => {
                if nal.len() < 2 {
                    None
                } else {
                    Some((nal[0] >> 1) & 0x3f)
                }
            }
        }
    }

    fn create_format_description(
        codec: VideoCodecKind,
        nal_length_size: usize,
        parameter_sets: &[Vec<u8>],
    ) -> Result<CMFormatDescriptionRef, DecoderError> {
        let pointers = parameter_sets
            .iter()
            .map(|parameter_set| parameter_set.as_ptr())
            .collect::<Vec<_>>();
        let sizes = parameter_sets.iter().map(Vec::len).collect::<Vec<_>>();
        let mut format_description = ptr::null_mut();
        let nal_length_size =
            i32::try_from(nal_length_size).map_err(|_| DecoderError::InvalidPacket {
                message: "NAL length size does not fit i32".to_owned(),
            })?;
        let status = match codec {
            VideoCodecKind::H264 => {
                // SAFETY: parameter set pointers/sizes are valid for this call.
                unsafe {
                    CMVideoFormatDescriptionCreateFromH264ParameterSets(
                        ptr::null(),
                        parameter_sets.len(),
                        pointers.as_ptr(),
                        sizes.as_ptr(),
                        nal_length_size,
                        &mut format_description,
                    )
                }
            }
            VideoCodecKind::Hevc => {
                // SAFETY: parameter set pointers/sizes are valid for this call.
                unsafe {
                    CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                        ptr::null(),
                        parameter_sets.len(),
                        pointers.as_ptr(),
                        sizes.as_ptr(),
                        nal_length_size,
                        ptr::null(),
                        &mut format_description,
                    )
                }
            }
        };
        os_status_result("CMVideoFormatDescriptionCreate", status)?;
        if format_description.is_null() {
            return Err(DecoderError::internal(
                "CoreMedia returned a null format description",
            ));
        }
        Ok(format_description)
    }

    fn create_sample_buffer(
        format_description: CMFormatDescriptionRef,
        data: &[u8],
        pts_us: Option<i64>,
        dts_us: Option<i64>,
        duration_us: Option<i64>,
    ) -> Result<CMSampleBufferRef, DecoderError> {
        let mut block_buffer = ptr::null_mut();
        // SAFETY: CoreMedia allocates a block buffer large enough for data.
        let create_block_status = unsafe {
            CMBlockBufferCreateWithMemoryBlock(
                ptr::null(),
                ptr::null_mut(),
                data.len(),
                ptr::null(),
                ptr::null(),
                0,
                data.len(),
                0,
                &mut block_buffer,
            )
        };
        os_status_result("CMBlockBufferCreateWithMemoryBlock", create_block_status)?;
        // SAFETY: block_buffer was allocated above and data is valid.
        let replace_status = unsafe {
            CMBlockBufferReplaceDataBytes(data.as_ptr().cast(), block_buffer, 0, data.len())
        };
        if replace_status != NO_ERR {
            // SAFETY: block_buffer was created above.
            unsafe { CFRelease(block_buffer as CFTypeRef) };
            return Err(os_status_error(
                "CMBlockBufferReplaceDataBytes",
                replace_status,
            ));
        }

        let timing = CMSampleTimingInfo {
            duration: cm_time_from_us(duration_us),
            presentation_time_stamp: cm_time_from_us(pts_us),
            decode_time_stamp: cm_time_from_us(dts_us),
        };
        let sample_size = data.len();
        let mut sample_buffer = ptr::null_mut();
        // SAFETY: all CoreMedia refs and sample size/timing pointers are valid
        // for the call.
        let sample_status = unsafe {
            CMSampleBufferCreateReady(
                ptr::null(),
                block_buffer,
                format_description,
                1,
                1,
                &timing,
                1,
                &sample_size,
                &mut sample_buffer,
            )
        };
        // SAFETY: sample_buffer retains block_buffer on success.
        unsafe { CFRelease(block_buffer as CFTypeRef) };
        os_status_result("CMSampleBufferCreateReady", sample_status)?;
        if sample_buffer.is_null() {
            return Err(DecoderError::internal(
                "CoreMedia returned a null sample buffer",
            ));
        }
        Ok(sample_buffer)
    }

    fn create_pixel_buffer_attributes() -> Result<CFDictionaryRef, DecoderError> {
        let pixel_format_value = K_CV_PIXEL_FORMAT_TYPE_420YPCBCR8_BIPLANAR_VIDEO_RANGE;
        // SAFETY: CoreFoundation objects created here are released before
        // returning, except for the dictionary returned to the caller.
        unsafe {
            let pixel_format = CFNumberCreate(
                ptr::null(),
                K_CF_NUMBER_SINT32_TYPE,
                (&pixel_format_value as *const i32).cast(),
            );
            if pixel_format.is_null() {
                return Err(DecoderError::internal(
                    "failed to create CVPixelBuffer pixel format attribute",
                ));
            }
            let empty_iosurface_properties = CFDictionaryCreate(
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            );
            if empty_iosurface_properties.is_null() {
                CFRelease(pixel_format as CFTypeRef);
                return Err(DecoderError::internal(
                    "failed to create IOSurface pixel buffer attributes",
                ));
            }
            let keys = [
                kCVPixelBufferPixelFormatTypeKey.cast::<c_void>(),
                kCVPixelBufferIOSurfacePropertiesKey.cast::<c_void>(),
            ];
            let values = [
                pixel_format.cast::<c_void>(),
                empty_iosurface_properties.cast::<c_void>(),
            ];
            let attributes = CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                keys.len() as CFIndex,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            );
            CFRelease(pixel_format as CFTypeRef);
            CFRelease(empty_iosurface_properties as CFTypeRef);
            if attributes.is_null() {
                return Err(DecoderError::internal(
                    "failed to create CVPixelBuffer attributes dictionary",
                ));
            }
            Ok(attributes)
        }
    }

    fn annexb_to_length_prefixed(
        data: &[u8],
        nal_length_size: usize,
    ) -> Result<Vec<u8>, DecoderError> {
        if !(1..=4).contains(&nal_length_size) {
            return Err(DecoderError::InvalidPacket {
                message: format!("unsupported NAL length size {nal_length_size}"),
            });
        }
        let nalus = annexb_nalus(data);
        let mut output = Vec::with_capacity(data.len());
        for nalu in nalus {
            if nalu.is_empty() {
                continue;
            }
            let nalu_len = u32::try_from(nalu.len()).map_err(|_| DecoderError::InvalidPacket {
                message: "NAL unit exceeds u32 length".to_owned(),
            })?;
            let len_bytes = nalu_len.to_be_bytes();
            output.extend_from_slice(&len_bytes[4 - nal_length_size..]);
            output.extend_from_slice(nalu);
        }
        Ok(output)
    }

    fn annexb_nalus(data: &[u8]) -> Vec<&[u8]> {
        let mut nalus = Vec::new();
        let mut cursor = 0;
        while let Some((start, code_len)) = find_start_code(data, cursor) {
            let nalu_start = start + code_len;
            let next = find_start_code(data, nalu_start).map_or(data.len(), |(next, _)| next);
            let nalu = trim_trailing_zeroes(&data[nalu_start..next]);
            if !nalu.is_empty() {
                nalus.push(nalu);
            }
            cursor = next;
        }
        nalus
    }

    fn has_annexb_start_code(data: &[u8]) -> bool {
        find_start_code(data, 0).is_some()
    }

    fn find_start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
        let mut index = from;
        while index + 3 <= data.len() {
            if data[index..].starts_with(&[0, 0, 1]) {
                return Some((index, 3));
            }
            if index + 4 <= data.len() && data[index..].starts_with(&[0, 0, 0, 1]) {
                return Some((index, 4));
            }
            index += 1;
        }
        None
    }

    fn trim_trailing_zeroes(mut data: &[u8]) -> &[u8] {
        while data.last() == Some(&0) {
            data = &data[..data.len() - 1];
        }
        data
    }

    fn cm_time_from_us(value_us: Option<i64>) -> CMTime {
        match value_us {
            Some(value) => CMTime {
                value,
                timescale: 1_000_000,
                flags: CM_TIME_FLAGS_VALID,
                epoch: 0,
            },
            None => CMTime {
                value: 0,
                timescale: 0,
                flags: 0,
                epoch: 0,
            },
        }
    }

    fn cm_time_to_us(time: CMTime) -> Option<i64> {
        if time.flags & CM_TIME_FLAGS_VALID == 0 || time.timescale <= 0 {
            return None;
        }
        Some(time.value.saturating_mul(1_000_000) / i64::from(time.timescale))
    }

    fn os_status_result(action: &str, status: OSStatus) -> Result<(), DecoderError> {
        if status == NO_ERR {
            Ok(())
        } else {
            Err(os_status_error(action, status))
        }
    }

    fn os_status_error(action: &str, status: OSStatus) -> DecoderError {
        DecoderError::internal(format!("{action} failed with OSStatus {status}"))
    }

    #[cfg(test)]
    mod tests {
        use super::{
            VideoCodecKind, annexb_to_length_prefixed, parse_annexb_parameter_sets,
            parse_avcc_extradata,
        };

        #[test]
        fn avcc_extradata_parser_reads_sps_and_pps() {
            let extradata = [
                1, 100, 0, 31, 0xff, 0xe1, 0, 4, 0x67, 0x64, 0, 31, 1, 0, 4, 0x68, 0xee, 0x3c, 0x80,
            ];
            let parsed = parse_avcc_extradata(&extradata).expect("AVCC should parse");

            assert_eq!(parsed.nal_length_size, 4);
            assert_eq!(parsed.parameter_sets.len(), 2);
            assert_eq!(parsed.parameter_sets[0][0] & 0x1f, 7);
            assert_eq!(parsed.parameter_sets[1][0] & 0x1f, 8);
        }

        #[test]
        fn annexb_parser_extracts_hevc_parameter_sets() {
            let packet = [
                0, 0, 0, 1, 0x40, 1, 0xaa, 0, 0, 1, 0x42, 1, 0xbb, 0, 0, 1, 0x44, 1, 0xcc,
            ];
            let parsed = parse_annexb_parameter_sets(VideoCodecKind::Hevc, &packet)
                .expect("HEVC Annex B parameter sets should parse");

            assert_eq!(parsed.nal_length_size, 4);
            assert_eq!(parsed.parameter_sets.len(), 3);
        }

        #[test]
        fn annexb_to_length_prefixed_writes_big_endian_lengths() {
            let packet = [0, 0, 1, 0x65, 1, 2, 0, 0, 0, 1, 0x41, 3];
            let converted = annexb_to_length_prefixed(&packet, 4).expect("Annex B should convert");

            assert_eq!(converted, vec![0, 0, 0, 3, 0x65, 1, 2, 0, 0, 0, 2, 0x41, 3]);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::ffi::c_void;

    use player_plugin::{
        DecoderError, DecoderPacket, DecoderSessionConfig, VesperDecoderOpenSessionResult,
        VesperDecoderReceiveNativeFrameResult, VesperPluginProcessResult,
    };

    use super::{native_frame_error, open_error, process_error};

    pub fn open_session(_config: DecoderSessionConfig) -> VesperDecoderOpenSessionResult {
        open_error(DecoderError::internal(
            "VideoToolbox decoder plugin is only available on macOS",
        ))
    }

    pub fn send_packet(
        _session: *mut c_void,
        _packet: &DecoderPacket,
        _data: &[u8],
    ) -> VesperPluginProcessResult {
        process_error(DecoderError::NotConfigured)
    }

    pub fn receive_native_frame(_session: *mut c_void) -> VesperDecoderReceiveNativeFrameResult {
        native_frame_error(DecoderError::NotConfigured)
    }

    pub fn release_native_frame(
        _session: *mut c_void,
        _handle_kind: u32,
        _handle: usize,
    ) -> VesperPluginProcessResult {
        process_error(DecoderError::NotConfigured)
    }

    pub fn flush_session(_session: *mut c_void) -> VesperPluginProcessResult {
        process_error(DecoderError::NotConfigured)
    }

    pub fn close_session(_session: *mut c_void) -> VesperPluginProcessResult {
        process_error(DecoderError::NotConfigured)
    }
}

#[cfg(test)]
mod tests {
    use super::{decoder_capabilities, video_codec_kind};

    #[test]
    fn capabilities_advertise_video_hardware_native_frames() {
        let capabilities = decoder_capabilities();

        assert!(capabilities.supports_codec("H264", player_plugin::DecoderMediaKind::Video));
        assert!(capabilities.supports_codec("avc1", player_plugin::DecoderMediaKind::Video));
        assert!(capabilities.supports_codec("HEVC", player_plugin::DecoderMediaKind::Video));
        assert!(capabilities.supports_codec("hvc1", player_plugin::DecoderMediaKind::Video));
        assert!(capabilities.supports_gpu_handles == cfg!(target_os = "macos"));
        assert!(!capabilities.supports_cpu_video_frames);
    }

    #[test]
    fn codec_aliases_match_video_toolbox_targets() {
        assert!(video_codec_kind("avc1").is_some());
        assert!(video_codec_kind("hvc1").is_some());
        assert!(video_codec_kind("H265").is_some());
        assert!(video_codec_kind("vp9").is_none());
    }
}
