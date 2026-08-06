#![cfg_attr(
    not(any(target_os = "macos", target_os = "ios")),
    allow(dead_code, unused_imports)
)]
#![warn(clippy::undocumented_unsafe_blocks)]

use player_plugin::{
    DecoderBitstreamFormat, DecoderCapabilities, DecoderCodecCapability, DecoderError,
    DecoderFrameFormat, DecoderMediaKind, DecoderNativeHandleKind, DecoderNativeRequirements,
    DecoderSessionConfig, NativeDecoderPluginFactory, NativeDecoderSession,
    NativeFrameColorMetadata, NativeFramePipelineProfile, Plugin, PluginBuildError,
    normalize_decoder_codec_identifier,
};

const PLUGIN_ID: &str = "io.github.ikaros.vesper.decoder-videotoolbox";
const INSTANCE_ID: &str = "io.github.ikaros.vesper.decoder-videotoolbox.native";
const PLUGIN_NAME: &str = "player-decoder-videotoolbox";
const VIDEO_TOOLBOX_NATIVE_FRAMES_SUPPORTED: bool =
    cfg!(any(target_os = "macos", target_os = "ios"));

#[derive(Debug, Default)]
struct VideoToolboxDecoderFactory;

impl NativeDecoderPluginFactory for VideoToolboxDecoderFactory {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn capabilities(&self) -> DecoderCapabilities {
        decoder_capabilities()
    }

    fn native_requirements(&self) -> DecoderNativeRequirements {
        decoder_native_requirements()
    }

    fn open_native_session(
        &self,
        config: &DecoderSessionConfig,
    ) -> Result<Box<dyn NativeDecoderSession>, DecoderError> {
        let Some(codec) = video_codec_kind(&config.codec) else {
            return Err(DecoderError::UnsupportedCodec {
                codec: config.codec.clone(),
            });
        };
        if config.media_kind != DecoderMediaKind::Video
            || !self
                .capabilities()
                .supports_codec(&config.codec, config.media_kind)
        {
            return Err(DecoderError::UnsupportedCodec {
                codec: config.codec.clone(),
            });
        }
        if config.require_cpu_output {
            return Err(DecoderError::UnsupportedCapability {
                capability: "cpu-video-frame-output".to_owned(),
            });
        }
        platform::open_session(config.clone(), codec)
    }
}

fn decoder_capabilities() -> DecoderCapabilities {
    DecoderCapabilities {
        codecs: if VIDEO_TOOLBOX_NATIVE_FRAMES_SUPPORTED {
            vec![
                video_codec_capability("H264"),
                video_codec_capability("AVC"),
                video_codec_capability("AVC1"),
                video_codec_capability("AVC3"),
                video_codec_capability("HEVC"),
                video_codec_capability("H265"),
                video_codec_capability("HVC1"),
                video_codec_capability("HEV1"),
                video_codec_capability("DVH1"),
                video_codec_capability("DVHE"),
            ]
        } else {
            Vec::new()
        },
        supports_hardware_decode: VIDEO_TOOLBOX_NATIVE_FRAMES_SUPPORTED,
        supports_cpu_video_frames: false,
        supports_audio_frames: false,
        supports_pcm_frames: false,
        supports_gpu_handles: VIDEO_TOOLBOX_NATIVE_FRAMES_SUPPORTED,
        supports_presentation_release: false,
        supports_flush: true,
        supports_drain: true,
        max_sessions: None,
    }
}

fn decoder_native_requirements() -> DecoderNativeRequirements {
    DecoderNativeRequirements {
        required_device_context_kinds: Vec::new(),
        output_handle_kinds: vec![DecoderNativeHandleKind::CvPixelBuffer],
        output_pipeline_profiles: vec![NativeFramePipelineProfile::VideoToolboxCvPixelBuffer],
        requires_native_device_context: false,
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
        output_formats: vec![DecoderFrameFormat::Nv12],
    }
}

fn legacy_color_space_label(color: &NativeFrameColorMetadata) -> Option<String> {
    match (
        color.primaries.as_deref(),
        color.transfer.as_deref(),
        color.matrix.as_deref(),
    ) {
        (Some(primaries), Some(transfer), Some(matrix)) => {
            Some(format!("{primaries}/{transfer}/{matrix}"))
        }
        (Some(primaries), _, _) => Some(primaries.to_owned()),
        (_, Some(transfer), _) => Some(transfer.to_owned()),
        (_, _, Some(matrix)) => Some(matrix.to_owned()),
        _ => None,
    }
}

#[player_plugin::export]
fn videotoolbox_decoder_plugin() -> Result<Plugin, PluginBuildError> {
    Plugin::builder(PLUGIN_ID, PLUGIN_NAME)?
        .with_native_decoder(INSTANCE_ID, VideoToolboxDecoderFactory)?
        .build()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoCodecKind {
    H264,
    Hevc,
}

fn video_codec_kind(codec: &str) -> Option<VideoCodecKind> {
    match normalize_decoder_codec_identifier(codec).as_str() {
        "h264" | "avc" | "avc1" | "avc3" => Some(VideoCodecKind::H264),
        "hevc" | "h265" | "hvc1" | "hev1" | "dvh1" | "dvhe" => Some(VideoCodecKind::Hevc),
        _ => None,
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod platform {
    use std::collections::VecDeque;
    use std::ffi::c_void;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::ptr;
    use std::sync::{Arc, Mutex, MutexGuard};

    use player_plugin::{
        DecoderBitstreamFormat, DecoderError, DecoderFrameFormat, DecoderMediaKind,
        DecoderNativeFrame, DecoderNativeFrameMetadata, DecoderNativeFrameReleaseTracking,
        DecoderNativeHandleKind, DecoderPacket, DecoderPacketResult,
        DecoderReceiveNativeFrameOutput, DecoderSessionConfig, DecoderSessionInfo,
        NativeDecoderSession, NativeFrameColorMetadata, NativeFrameHdrMetadata,
        NativeFramePipelineProfile,
    };

    use super::{VideoCodecKind, legacy_color_space_label};

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
    const K_CF_NUMBER_SINT32_TYPE: i32 = 3;
    const K_CV_PIXEL_FORMAT_TYPE_420YPCBCR8_BIPLANAR_VIDEO_RANGE: i32 = fourcc(*b"420v");
    const K_CV_PIXEL_FORMAT_TYPE_420YPCBCR8_BIPLANAR_FULL_RANGE: u32 = fourcc_u32(*b"420f");
    const K_CV_PIXEL_FORMAT_TYPE_420YPCBCR10_BIPLANAR_VIDEO_RANGE: i32 = fourcc(*b"x420");
    const K_CV_PIXEL_FORMAT_TYPE_420YPCBCR10_BIPLANAR_FULL_RANGE: u32 = fourcc_u32(*b"xf20");
    const MAX_REORDER_DEPTH: usize = 16;
    const MAX_PENDING_NATIVE_FRAMES: usize = MAX_REORDER_DEPTH + 1;

    const fn fourcc(code: [u8; 4]) -> i32 {
        ((code[0] as i32) << 24)
            | ((code[1] as i32) << 16)
            | ((code[2] as i32) << 8)
            | code[3] as i32
    }

    const fn fourcc_u32(code: [u8; 4]) -> u32 {
        ((code[0] as u32) << 24)
            | ((code[1] as u32) << 16)
            | ((code[2] as u32) << 8)
            | code[3] as u32
    }

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
        fn CVPixelBufferGetPixelFormatType(pixel_buffer: CVPixelBufferRef) -> u32;
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

    pub(super) fn open_session(
        config: DecoderSessionConfig,
        codec: VideoCodecKind,
    ) -> Result<Box<dyn NativeDecoderSession>, DecoderError> {
        Ok(Box::new(VideoToolboxDecoderSession::new(config, codec)?))
    }

    struct VideoToolboxDecoderSession {
        codec: VideoCodecKind,
        codec_name: String,
        width: u32,
        height: u32,
        color: Option<NativeFrameColorMetadata>,
        hdr: Option<NativeFrameHdrMetadata>,
        requested_output_format: DecoderFrameFormat,
        bitstream_format: Option<DecoderBitstreamFormat>,
        nal_length_size: usize,
        parameter_sets: Vec<Vec<u8>>,
        format_description: CMFormatDescriptionRef,
        decompression_session: VTDecompressionSessionRef,
        callback_state: Arc<CallbackState>,
        callback_state_ref_con: *const CallbackState,
        reorder_depth: usize,
        end_of_stream_sent: bool,
        closed: bool,
    }

    // SAFETY: VideoToolbox/CoreFoundation refs are retained and released by
    // this session; access is serialized by the host-side decoder session.
    unsafe impl Send for VideoToolboxDecoderSession {}

    impl VideoToolboxDecoderSession {
        fn new(config: DecoderSessionConfig, codec: VideoCodecKind) -> Result<Self, DecoderError> {
            let mut parsed = if config.extradata.is_empty() {
                None
            } else {
                Some(parse_extradata(codec, &config.extradata)?)
            };
            let requested_output_format = requested_output_format();
            let mut session = Self {
                codec,
                codec_name: config.codec,
                width: config.width.unwrap_or_default(),
                height: config.height.unwrap_or_default(),
                color: config.color,
                hdr: config.hdr,
                requested_output_format,
                bitstream_format: config.bitstream_format,
                nal_length_size: parsed.as_ref().map_or(4, |parsed| parsed.nal_length_size),
                parameter_sets: parsed
                    .take()
                    .map(|parsed| parsed.parameter_sets)
                    .unwrap_or_default(),
                format_description: ptr::null_mut(),
                decompression_session: ptr::null_mut(),
                callback_state: Arc::new(CallbackState::default()),
                callback_state_ref_con: ptr::null(),
                reorder_depth: bounded_reorder_depth(config.reorder_depth),
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
                output_format: Some(self.requested_output_format.clone()),
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
            // SAFETY: waiting after each submitted frame keeps this native ABI
            // path synchronous until the host provides an async native-frame queue.
            let wait_status = unsafe {
                VTDecompressionSessionWaitForAsynchronousFrames(self.decompression_session)
            };
            os_status_result(
                "VTDecompressionSessionWaitForAsynchronousFrames",
                wait_status,
            )?;
            Ok(DecoderPacketResult { accepted: true })
        }

        fn receive_native_frame(
            &mut self,
        ) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
            let mut frames = pending_frames(&self.callback_state);
            let Some(frame) = dequeue_pending_native_frame(
                &mut frames,
                self.reorder_depth,
                self.end_of_stream_sent,
            ) else {
                return if self.end_of_stream_sent {
                    Ok(DecoderReceiveNativeFrameOutput::Eof)
                } else {
                    Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput)
                };
            };
            // SAFETY: `frame.pixel_buffer` is a retained CVPixelBuffer from the
            // VideoToolbox callback and remains valid until host release.
            let pixel_format = unsafe { CVPixelBufferGetPixelFormatType(frame.pixel_buffer) };
            let format = decoder_frame_format_from_pixel_format(pixel_format);
            let color = color_metadata_for_pixel_format(self.color.clone(), &format);
            Ok(DecoderReceiveNativeFrameOutput::Frame(DecoderNativeFrame {
                metadata: DecoderNativeFrameMetadata {
                    media_kind: DecoderMediaKind::Video,
                    format,
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
                    coded_width: None,
                    coded_height: None,
                    visible_rect: None,
                    handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
                    pipeline_profile: Some(NativeFramePipelineProfile::VideoToolboxCvPixelBuffer),
                    color_space: color.as_ref().and_then(legacy_color_space_label),
                    hdr_metadata: self.hdr.as_ref().map(|hdr| hdr.kind.clone()),
                    color,
                    hdr: self.hdr.clone(),
                    sync_info: None,
                    transform: None,
                    frame_id: Some(frame.pixel_buffer as u64),
                    release_tracking: Some(DecoderNativeFrameReleaseTracking {
                        frame_id: Some(frame.pixel_buffer as u64),
                        requires_release: true,
                    }),
                },
                handle: frame.pixel_buffer as usize,
                lease_token: None,
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
            if !self.callback_state_ref_con.is_null() {
                // SAFETY: this raw pointer was created by `Arc::into_raw` when
                // the VideoToolbox session was created and is reclaimed exactly
                // once after the session has been invalidated.
                unsafe { drop(Arc::from_raw(self.callback_state_ref_con)) };
                self.callback_state_ref_con = ptr::null();
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
            let pixel_buffer_attributes =
                match create_pixel_buffer_attributes(&self.requested_output_format) {
                    Ok(attributes) => attributes,
                    Err(error) => {
                        // SAFETY: format_description was created by CoreMedia
                        // and has not been transferred to a session.
                        unsafe { CFRelease(format_description as CFTypeRef) };
                        return Err(error);
                    }
                };
            let callback_state_ref_con = Arc::into_raw(self.callback_state.clone());
            let callback = VTDecompressionOutputCallbackRecord {
                decompression_output_callback: Some(decompression_output_callback),
                decompression_output_ref_con: callback_state_ref_con.cast_mut().cast(),
            };
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
                // SAFETY: the raw callback state was not transferred to a live
                // VideoToolbox session because creation failed.
                unsafe { drop(Arc::from_raw(callback_state_ref_con)) };
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
            self.callback_state_ref_con = callback_state_ref_con;
            Ok(())
        }

        fn normalized_sample_data(&self, data: &[u8]) -> Result<Vec<u8>, DecoderError> {
            match &self.bitstream_format {
                Some(DecoderBitstreamFormat::AnnexB) => {
                    annexb_to_length_prefixed(data, self.nal_length_size)
                }
                Some(DecoderBitstreamFormat::Avcc) | Some(DecoderBitstreamFormat::Hvcc) => {
                    Ok(data.to_vec())
                }
                Some(DecoderBitstreamFormat::Unknown(_)) | None => {
                    normalize_sample_data(data, self.nal_length_size)
                }
            }
        }

        fn release_queued_frames(&mut self) {
            let mut frames = pending_frames(&self.callback_state);
            while let Some(frame) = frames.pop_front() {
                // SAFETY: queued frames are retained in the callback and still
                // owned by this session.
                unsafe { CFRelease(frame.pixel_buffer as CFTypeRef) };
            }
        }
    }

    impl NativeDecoderSession for VideoToolboxDecoderSession {
        fn session_info(&self) -> DecoderSessionInfo {
            VideoToolboxDecoderSession::session_info(self)
        }

        fn send_packet(
            &mut self,
            packet: &DecoderPacket,
            data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            VideoToolboxDecoderSession::send_packet(self, packet, data)
        }

        fn receive_native_frame(
            &mut self,
        ) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
            VideoToolboxDecoderSession::receive_native_frame(self)
        }

        fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
            if self.closed {
                return Err(DecoderError::NotConfigured);
            }
            if frame.metadata.handle_kind != DecoderNativeHandleKind::CvPixelBuffer {
                return Err(DecoderError::abi_violation(format!(
                    "VideoToolbox plugin expected CVPixelBuffer handle kind, got {:?}",
                    frame.metadata.handle_kind
                )));
            }
            if frame.handle == 0 {
                return Err(DecoderError::abi_violation(
                    "VideoToolbox plugin received a null native frame handle",
                ));
            }
            // SAFETY: receive_native_frame transfers one retained
            // CVPixelBuffer reference into this session-owned frame lease.
            unsafe { CFRelease(frame.handle as CFTypeRef) };
            Ok(())
        }

        fn flush(&mut self) -> Result<(), DecoderError> {
            VideoToolboxDecoderSession::flush(self)
        }

        fn close(&mut self) -> Result<(), DecoderError> {
            VideoToolboxDecoderSession::close(self)
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

    fn pending_frames(state: &CallbackState) -> MutexGuard<'_, VecDeque<PendingNativeFrame>> {
        state
            .frames
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    #[derive(Debug)]
    struct PendingNativeFrame {
        pixel_buffer: CVPixelBufferRef,
        pts_us: Option<i64>,
        duration_us: Option<i64>,
        width: u32,
        height: u32,
    }

    // SAFETY: `PendingNativeFrame` owns a retained pixel buffer reference, and
    // release is serialized by the decoder session queue.
    unsafe impl Send for PendingNativeFrame {}

    unsafe extern "C" fn decompression_output_callback(
        decompression_output_ref_con: *mut c_void,
        _source_frame_ref_con: *mut c_void,
        status: OSStatus,
        _info_flags: u32,
        image_buffer: CVImageBufferRef,
        presentation_time_stamp: CMTime,
        presentation_duration: CMTime,
    ) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            decompression_output_callback_impl(
                decompression_output_ref_con,
                status,
                image_buffer,
                presentation_time_stamp,
                presentation_duration,
            );
        }));
    }

    fn decompression_output_callback_impl(
        decompression_output_ref_con: *mut c_void,
        status: OSStatus,
        image_buffer: CVImageBufferRef,
        presentation_time_stamp: CMTime,
        presentation_duration: CMTime,
    ) {
        if status != NO_ERR || image_buffer.is_null() || decompression_output_ref_con.is_null() {
            return;
        }
        // SAFETY: VideoToolbox passes the `CallbackState` pointer configured
        // when creating the decompression session.
        let callback_state = unsafe { &*(decompression_output_ref_con.cast::<CallbackState>()) };
        // SAFETY: VideoToolbox provides a valid image buffer for this callback;
        // retaining transfers frame ownership into the plugin queue.
        let retained = unsafe { CFRetain(image_buffer as CFTypeRef) }.cast_mut();
        if retained.is_null() {
            return;
        }
        let pixel_buffer = retained.cast::<c_void>();
        // SAFETY: `pixel_buffer` is the retained CVPixelBuffer passed by
        // VideoToolbox for this callback.
        let width =
            u32::try_from(unsafe { CVPixelBufferGetWidth(pixel_buffer) }).unwrap_or(u32::MAX);
        // SAFETY: same retained CVPixelBuffer as above.
        let height =
            u32::try_from(unsafe { CVPixelBufferGetHeight(pixel_buffer) }).unwrap_or(u32::MAX);
        let frame = PendingNativeFrame {
            pixel_buffer,
            pts_us: cm_time_to_us(presentation_time_stamp),
            duration_us: cm_time_to_us(presentation_duration),
            width,
            height,
        };
        let mut frames = pending_frames(callback_state);
        if let Err(frame) = enqueue_pending_native_frame(&mut frames, frame) {
            // SAFETY: release the retain above if the bounded queue cannot
            // accept another frame.
            unsafe { CFRelease(frame.pixel_buffer as CFTypeRef) };
        }
    }

    fn enqueue_pending_native_frame(
        frames: &mut VecDeque<PendingNativeFrame>,
        frame: PendingNativeFrame,
    ) -> Result<(), PendingNativeFrame> {
        if frames.len() >= MAX_PENDING_NATIVE_FRAMES {
            return Err(frame);
        }
        let insert_at = frame.pts_us.and_then(|pts_us| {
            frames.iter().position(|queued| {
                queued
                    .pts_us
                    .is_none_or(|queued_pts_us| pts_us < queued_pts_us)
            })
        });
        if let Some(insert_at) = insert_at {
            frames.insert(insert_at, frame);
        } else {
            frames.push_back(frame);
        }
        Ok(())
    }

    fn dequeue_pending_native_frame(
        frames: &mut VecDeque<PendingNativeFrame>,
        reorder_depth: usize,
        draining: bool,
    ) -> Option<PendingNativeFrame> {
        if !draining && frames.len() <= reorder_depth {
            return None;
        }
        frames.pop_front()
    }

    fn bounded_reorder_depth(reorder_depth: Option<u32>) -> usize {
        usize::try_from(reorder_depth.unwrap_or_default())
            .unwrap_or(MAX_REORDER_DEPTH)
            .min(MAX_REORDER_DEPTH)
    }

    struct ParsedVideoConfig {
        nal_length_size: usize,
        parameter_sets: Vec<Vec<u8>>,
    }

    fn parse_extradata(
        codec: VideoCodecKind,
        extradata: &[u8],
    ) -> Result<ParsedVideoConfig, DecoderError> {
        match codec {
            VideoCodecKind::H264 if extradata.first() == Some(&1) => {
                parse_avcc_extradata(extradata)
            }
            VideoCodecKind::Hevc if extradata.first() == Some(&1) => {
                parse_hvcc_extradata(extradata)
            }
            _ if has_annexb_start_code(extradata) => parse_annexb_parameter_sets(codec, extradata)
                .ok_or_else(|| DecoderError::InvalidPacket {
                    message: "extradata did not contain complete Annex B parameter sets".to_owned(),
                }),
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
            (VideoCodecKind::H264, Some(7 | 8)) | (VideoCodecKind::Hevc, Some(32..=34))
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

    fn requested_output_format() -> DecoderFrameFormat {
        DecoderFrameFormat::Nv12
    }

    fn pixel_format_for_requested_output(output_format: &DecoderFrameFormat) -> i32 {
        match output_format {
            DecoderFrameFormat::P010 => K_CV_PIXEL_FORMAT_TYPE_420YPCBCR10_BIPLANAR_VIDEO_RANGE,
            _ => K_CV_PIXEL_FORMAT_TYPE_420YPCBCR8_BIPLANAR_VIDEO_RANGE,
        }
    }

    fn decoder_frame_format_from_pixel_format(pixel_format: u32) -> DecoderFrameFormat {
        match pixel_format {
            value if value == K_CV_PIXEL_FORMAT_TYPE_420YPCBCR8_BIPLANAR_VIDEO_RANGE as u32 => {
                DecoderFrameFormat::Nv12
            }
            K_CV_PIXEL_FORMAT_TYPE_420YPCBCR8_BIPLANAR_FULL_RANGE => DecoderFrameFormat::Nv12,
            value if value == K_CV_PIXEL_FORMAT_TYPE_420YPCBCR10_BIPLANAR_VIDEO_RANGE as u32 => {
                DecoderFrameFormat::P010
            }
            K_CV_PIXEL_FORMAT_TYPE_420YPCBCR10_BIPLANAR_FULL_RANGE => DecoderFrameFormat::P010,
            other => DecoderFrameFormat::Unknown(format!("cvpixelbuffer_{}", fourcc_label(other))),
        }
    }

    fn color_metadata_for_pixel_format(
        mut color: Option<NativeFrameColorMetadata>,
        format: &DecoderFrameFormat,
    ) -> Option<NativeFrameColorMetadata> {
        if matches!(format, DecoderFrameFormat::P010) {
            color
                .get_or_insert_with(|| NativeFrameColorMetadata {
                    primaries: None,
                    transfer: None,
                    matrix: None,
                    range: None,
                    bit_depth: None,
                })
                .bit_depth
                .get_or_insert(10);
        }
        color
    }

    fn fourcc_label(value: u32) -> String {
        let bytes = value.to_be_bytes();
        if bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        {
            String::from_utf8_lossy(&bytes).trim().to_owned()
        } else {
            format!("{value:#010x}")
        }
    }

    fn create_pixel_buffer_attributes(
        output_format: &DecoderFrameFormat,
    ) -> Result<CFDictionaryRef, DecoderError> {
        let pixel_format_value = pixel_format_for_requested_output(output_format);
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

    fn normalize_sample_data(data: &[u8], nal_length_size: usize) -> Result<Vec<u8>, DecoderError> {
        if length_prefixed_sample_is_well_formed(data, nal_length_size) {
            return Ok(data.to_vec());
        }
        if has_annexb_start_code(data) {
            return annexb_to_length_prefixed(data, nal_length_size);
        }
        Ok(data.to_vec())
    }

    fn length_prefixed_sample_is_well_formed(data: &[u8], nal_length_size: usize) -> bool {
        if data.len() <= nal_length_size || !(1..=4).contains(&nal_length_size) {
            return false;
        }

        let mut offset = 0usize;
        let mut nal_count = 0usize;
        while offset < data.len() {
            if data.len().saturating_sub(offset) < nal_length_size {
                return false;
            }
            let nal_len = read_nal_length(&data[offset..offset + nal_length_size]);
            offset = offset.saturating_add(nal_length_size);
            if nal_len == 0 {
                return false;
            }
            let Some(next_offset) = offset.checked_add(nal_len) else {
                return false;
            };
            if next_offset > data.len() {
                return false;
            }
            offset = next_offset;
            nal_count = nal_count.saturating_add(1);
        }

        nal_count > 0
    }

    fn read_nal_length(bytes: &[u8]) -> usize {
        bytes
            .iter()
            .fold(0usize, |length, byte| (length << 8) | usize::from(*byte))
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
            K_CV_PIXEL_FORMAT_TYPE_420YPCBCR10_BIPLANAR_VIDEO_RANGE, MAX_PENDING_NATIVE_FRAMES,
            MAX_REORDER_DEPTH, PendingNativeFrame, VideoCodecKind, annexb_to_length_prefixed,
            bounded_reorder_depth, color_metadata_for_pixel_format,
            decoder_frame_format_from_pixel_format, dequeue_pending_native_frame,
            enqueue_pending_native_frame, length_prefixed_sample_is_well_formed,
            normalize_sample_data, parse_annexb_parameter_sets, parse_avcc_extradata,
            requested_output_format,
        };
        use player_plugin::{DecoderFrameFormat, NativeFrameColorMetadata, NativeFrameHdrMetadata};
        use std::collections::VecDeque;

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

        #[test]
        fn avcc_sample_with_start_code_like_length_stays_length_prefixed() {
            let mut packet = vec![0, 0, 1, 0];
            packet.push(0x41);
            packet.extend(std::iter::repeat_n(0xaa, 255));

            assert!(length_prefixed_sample_is_well_formed(&packet, 4));
            let normalized =
                normalize_sample_data(&packet, 4).expect("length-prefixed sample should normalize");

            assert_eq!(normalized, packet);
        }

        #[test]
        fn hdr_metadata_does_not_request_p010_output() {
            let _color = NativeFrameColorMetadata {
                primaries: Some("bt2020".to_owned()),
                transfer: Some("smpte2084".to_owned()),
                matrix: Some("bt2020-ncl".to_owned()),
                range: Some("limited".to_owned()),
                bit_depth: Some(10),
            };
            let _hdr = NativeFrameHdrMetadata {
                kind: "hdr10".to_owned(),
                mastering_display: None,
                content_light: None,
                dolby_vision: None,
            };

            assert_eq!(requested_output_format(), DecoderFrameFormat::Nv12);
        }

        #[test]
        fn cvpixelbuffer_10bit_format_reports_p010_and_bit_depth() {
            let format = decoder_frame_format_from_pixel_format(
                K_CV_PIXEL_FORMAT_TYPE_420YPCBCR10_BIPLANAR_VIDEO_RANGE as u32,
            );
            let color = color_metadata_for_pixel_format(None, &format);

            assert_eq!(format, DecoderFrameFormat::P010);
            assert_eq!(color.and_then(|color| color.bit_depth), Some(10));
        }

        #[test]
        fn pending_native_frame_queue_rejects_frames_over_limit() {
            let mut frames = VecDeque::new();
            for handle in 1..=MAX_PENDING_NATIVE_FRAMES {
                enqueue_pending_native_frame(&mut frames, pending_frame(handle))
                    .expect("frame should fit under limit");
            }

            let rejected = enqueue_pending_native_frame(&mut frames, pending_frame(999))
                .expect_err("queue should reject frames over limit");

            assert_eq!(frames.len(), MAX_PENDING_NATIVE_FRAMES);
            assert_eq!(rejected.pixel_buffer as usize, 999);
        }

        #[test]
        fn pending_native_frame_queue_orders_b_frames_by_presentation_time() {
            let mut frames = VecDeque::new();
            for (handle, pts_us) in [(1, 0), (2, 66_667), (3, 33_333)] {
                let mut frame = pending_frame(handle);
                frame.pts_us = Some(pts_us);
                enqueue_pending_native_frame(&mut frames, frame)
                    .expect("frame should fit under limit");
            }

            assert_eq!(
                frames.iter().map(|frame| frame.pts_us).collect::<Vec<_>>(),
                vec![Some(0), Some(33_333), Some(66_667)]
            );
        }

        #[test]
        fn pending_native_frame_queue_holds_reorder_window_until_ready() {
            let mut frames = VecDeque::new();
            for (handle, pts_us) in [(1, 0), (2, 66_667), (3, 33_333)] {
                let mut frame = pending_frame(handle);
                frame.pts_us = Some(pts_us);
                enqueue_pending_native_frame(&mut frames, frame)
                    .expect("frame should fit under limit");
            }

            assert!(dequeue_pending_native_frame(&mut frames, 3, false).is_none());
            let mut fourth = pending_frame(4);
            fourth.pts_us = Some(100_000);
            enqueue_pending_native_frame(&mut frames, fourth)
                .expect("frame should fit under limit");

            assert_eq!(
                dequeue_pending_native_frame(&mut frames, 3, false).and_then(|frame| frame.pts_us),
                Some(0)
            );
            assert_eq!(
                dequeue_pending_native_frame(&mut frames, 3, true).and_then(|frame| frame.pts_us),
                Some(33_333)
            );
        }

        #[test]
        fn video_reorder_depth_is_capped_at_sixteen_frames() {
            assert_eq!(bounded_reorder_depth(None), 0);
            assert_eq!(bounded_reorder_depth(Some(4)), 4);
            assert_eq!(bounded_reorder_depth(Some(64)), MAX_REORDER_DEPTH);
        }

        fn pending_frame(handle: usize) -> PendingNativeFrame {
            PendingNativeFrame {
                pixel_buffer: handle as *mut _,
                pts_us: None,
                duration_us: None,
                width: 0,
                height: 0,
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
mod platform {
    use player_plugin::{DecoderError, DecoderSessionConfig, NativeDecoderSession};

    use super::VideoCodecKind;

    pub(super) fn open_session(
        _config: DecoderSessionConfig,
        _codec: VideoCodecKind,
    ) -> Result<Box<dyn NativeDecoderSession>, DecoderError> {
        Err(DecoderError::UnsupportedCapability {
            capability: "apple-videotoolbox-platform".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        VIDEO_TOOLBOX_NATIVE_FRAMES_SUPPORTED, VideoToolboxDecoderFactory, decoder_capabilities,
        video_codec_kind,
    };
    use player_plugin::{DecoderMediaKind, DecoderSessionConfig, NativeDecoderPluginFactory};

    #[test]
    fn exports_plugin_entry() {
        assert!(!super::vesper_plugin_entry().is_null());
    }

    #[test]
    fn capabilities_advertise_video_hardware_native_frames() {
        let capabilities = decoder_capabilities();

        assert_eq!(
            capabilities.supports_codec("H264", DecoderMediaKind::Video),
            VIDEO_TOOLBOX_NATIVE_FRAMES_SUPPORTED
        );
        assert_eq!(
            capabilities.supports_codec("video/avc1.640028", DecoderMediaKind::Video),
            VIDEO_TOOLBOX_NATIVE_FRAMES_SUPPORTED
        );
        assert_eq!(
            capabilities.supports_codec("dvh1.05.06", DecoderMediaKind::Video),
            VIDEO_TOOLBOX_NATIVE_FRAMES_SUPPORTED
        );
        assert!(capabilities.supports_gpu_handles == super::VIDEO_TOOLBOX_NATIVE_FRAMES_SUPPORTED);
        assert!(!capabilities.supports_cpu_video_frames);
    }

    #[test]
    fn codec_aliases_match_video_toolbox_targets() {
        assert!(video_codec_kind("avc1").is_some());
        assert!(video_codec_kind("hvc1").is_some());
        assert!(video_codec_kind("H265").is_some());
        assert!(video_codec_kind("dvh1.05.06").is_some());
        assert!(video_codec_kind("dvhe.08.07").is_some());
        assert!(video_codec_kind("video/avc1.640028").is_some());
        assert!(video_codec_kind("avc1garbage").is_none());
        assert!(video_codec_kind("vp9").is_none());
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    fn factory_opens_profile_qualified_h264_and_rejects_cpu_output() {
        let factory = VideoToolboxDecoderFactory;
        let mut session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: "video/avc1.640028".to_owned(),
                media_kind: DecoderMediaKind::Video,
                ..DecoderSessionConfig::default()
            })
            .expect("open VideoToolbox session without eager format description");
        assert_eq!(
            session.session_info().selected_hardware_backend.as_deref(),
            Some("VideoToolbox")
        );
        session.close().expect("close VideoToolbox session");

        let error = match factory.open_native_session(&DecoderSessionConfig {
            codec: "H264".to_owned(),
            media_kind: DecoderMediaKind::Video,
            require_cpu_output: true,
            ..DecoderSessionConfig::default()
        }) {
            Ok(_) => panic!("CPU output must be rejected"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            player_plugin::DecoderError::UnsupportedCapability { capability }
                if capability == "cpu-video-frame-output"
        ));
    }
}
