use std::ffi::{c_char, c_void};

use player_plugin::{
    DecoderBitstreamFormat, DecoderCapabilities, DecoderCodecCapability, DecoderError,
    DecoderFrameFormat, DecoderMediaKind, DecoderNativeDeviceContextKind,
    DecoderNativeFrameMetadata, DecoderNativeFrameReleaseTracking, DecoderNativeHandleKind,
    DecoderNativeRequirements, DecoderOperationStatus, DecoderPacket, DecoderPacketResult,
    DecoderReceiveNativeFrameMetadata, DecoderSessionConfig, DecoderSessionInfo,
    VESPER_DECODER_PLUGIN_ABI_VERSION_V2, VesperDecoderOpenSessionResult, VesperDecoderPluginApiV2,
    VesperDecoderReceiveNativeFrameResult, VesperPluginBytes, VesperPluginDescriptor,
    VesperPluginKind, VesperPluginProcessResult, VesperPluginResultStatus,
};

static PLUGIN_NAME: &[u8] = b"player-decoder-d3d11\0";
const HANDLE_KIND_D3D11_TEXTURE_2D: u32 = 6;
#[cfg(target_os = "windows")]
const DEFAULT_WIDTH: u32 = 16;
#[cfg(target_os = "windows")]
const DEFAULT_HEIGHT: u32 = 16;

struct PluginBundle {
    api: VesperDecoderPluginApiV2,
    descriptor: VesperPluginDescriptor,
}

struct D3D11DecoderSession {
    codec: String,
    inner: platform::SessionInner,
    eof_received: bool,
    eof_sent: bool,
}

impl D3D11DecoderSession {
    fn open(config: DecoderSessionConfig) -> Result<Self, DecoderError> {
        if !decoder_capabilities().supports_codec(&config.codec, config.media_kind) {
            return Err(DecoderError::UnsupportedCodec {
                codec: config.codec,
            });
        }
        if config.require_cpu_output {
            return Err(DecoderError::NotConfigured);
        }

        let inner = platform::SessionInner::open(&config)?;

        Ok(Self {
            codec: config.codec,
            inner,
            eof_received: false,
            eof_sent: false,
        })
    }

    fn send_packet(
        &mut self,
        packet: DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        if packet.discontinuity {
            self.inner.flush()?;
            self.eof_received = false;
            self.eof_sent = false;
        }

        if packet.end_of_stream {
            self.eof_received = true;
            return self.inner.send_end_of_stream();
        }

        self.inner.send_packet(&packet, data)
    }

    fn receive_native_frame(
        &mut self,
    ) -> Result<(DecoderReceiveNativeFrameMetadata, usize), DecoderError> {
        match self.inner.receive_native_frame()? {
            platform::ReceiveNativeFrame::Frame(frame) => {
                let metadata = DecoderNativeFrameMetadata {
                    media_kind: DecoderMediaKind::Video,
                    format: frame.format,
                    codec: self.codec.clone(),
                    pts_us: frame.pts_us,
                    duration_us: frame.duration_us,
                    width: frame.width,
                    height: frame.height,
                    coded_width: Some(frame.coded_width),
                    coded_height: Some(frame.coded_height),
                    visible_rect: None,
                    handle_kind: frame.handle_kind,
                    frame_id: Some(frame.frame_id),
                    release_tracking: Some(DecoderNativeFrameReleaseTracking {
                        frame_id: Some(frame.frame_id),
                        requires_release: true,
                    }),
                };
                Ok((
                    DecoderReceiveNativeFrameMetadata::frame(metadata),
                    frame.handle,
                ))
            }
            platform::ReceiveNativeFrame::NeedMoreInput => {
                if self.eof_received && !self.eof_sent {
                    self.eof_sent = true;
                    return Ok((DecoderReceiveNativeFrameMetadata::eof(), 0));
                }
                Ok((DecoderReceiveNativeFrameMetadata::need_more_input(), 0))
            }
            platform::ReceiveNativeFrame::Eof => {
                self.eof_sent = true;
                Ok((DecoderReceiveNativeFrameMetadata::eof(), 0))
            }
        }
    }

    fn release_native_frame(
        &mut self,
        handle_kind: u32,
        handle: usize,
    ) -> Result<(), DecoderError> {
        if handle_kind != HANDLE_KIND_D3D11_TEXTURE_2D || handle == 0 {
            return Err(DecoderError::abi_violation(
                "D3D11 decoder release received an invalid texture handle",
            ));
        }
        self.inner.release_frame_texture(handle)
    }

    fn flush(&mut self) {
        let _ = self.inner.flush();
        self.eof_received = false;
        self.eof_sent = false;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_plugin_entry() -> *const VesperPluginDescriptor {
    let mut bundle = Box::new(PluginBundle {
        api: VesperDecoderPluginApiV2 {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(decoder_name),
            capabilities_json: Some(decoder_capabilities_json),
            native_requirements_json: Some(decoder_native_requirements_json),
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

unsafe extern "C" fn decoder_native_requirements_json(_context: *mut c_void) -> VesperPluginBytes {
    serialize_payload(&decoder_native_requirements())
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

    match D3D11DecoderSession::open(config) {
        Ok(session) => {
            let info = DecoderSessionInfo {
                decoder_name: Some("player-decoder-d3d11".to_owned()),
                selected_hardware_backend: Some("D3D11".to_owned()),
                output_format: Some(DecoderFrameFormat::Bgra8888),
            };
            open_success(Box::into_raw(Box::new(session)).cast::<c_void>(), &info)
        }
        Err(error) => open_error(error),
    }
}

unsafe extern "C" fn decoder_send_packet(
    _context: *mut c_void,
    session: *mut c_void,
    packet_json: *const u8,
    packet_json_len: usize,
    packet_data: *const u8,
    packet_data_len: usize,
) -> VesperPluginProcessResult {
    let Some(session) = (unsafe { session.cast::<D3D11DecoderSession>().as_mut() }) else {
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

    let packet_data = if packet_data.is_null() || packet_data_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(packet_data, packet_data_len) }
    };

    match session.send_packet(packet, packet_data) {
        Ok(result) => process_success(&result),
        Err(error) => process_error(error),
    }
}

unsafe extern "C" fn decoder_receive_native_frame(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperDecoderReceiveNativeFrameResult {
    let Some(session) = (unsafe { session.cast::<D3D11DecoderSession>().as_mut() }) else {
        return native_frame_error(DecoderError::NotConfigured);
    };

    match session.receive_native_frame() {
        Ok((metadata, handle)) => native_frame_success(&metadata, handle),
        Err(error) => native_frame_error(error),
    }
}

unsafe extern "C" fn decoder_release_native_frame(
    _context: *mut c_void,
    session: *mut c_void,
    handle_kind: u32,
    handle: usize,
) -> VesperPluginProcessResult {
    let Some(session) = (unsafe { session.cast::<D3D11DecoderSession>().as_mut() }) else {
        return process_error(DecoderError::NotConfigured);
    };

    match session.release_native_frame(handle_kind, handle) {
        Ok(()) => process_success(&DecoderOperationStatus { completed: true }),
        Err(error) => process_error(error),
    }
}

unsafe extern "C" fn decoder_flush_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    let Some(session) = (unsafe { session.cast::<D3D11DecoderSession>().as_mut() }) else {
        return process_error(DecoderError::NotConfigured);
    };
    session.flush();
    process_success(&DecoderOperationStatus { completed: true })
}

unsafe extern "C" fn decoder_close_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    if session.is_null() {
        return process_error(DecoderError::NotConfigured);
    }
    let _ = unsafe { Box::from_raw(session.cast::<D3D11DecoderSession>()) };
    process_success(&DecoderOperationStatus { completed: true })
}

unsafe extern "C" fn free_plugin_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
    let _ = unsafe { payload.into_vec() };
}

fn decoder_capabilities() -> DecoderCapabilities {
    DecoderCapabilities {
        codecs: [
            ("H264", "baseline/main/high"),
            ("AVC", "baseline/main/high"),
            ("AVC1", "baseline/main/high"),
            ("HEVC", "main/main10"),
            ("H265", "main/main10"),
            ("HVC1", "main/main10"),
            ("HEV1", "main/main10"),
        ]
        .into_iter()
        .map(|(codec, profile)| DecoderCodecCapability {
            codec: codec.to_owned(),
            media_kind: DecoderMediaKind::Video,
            profiles: vec![profile.to_owned()],
            output_formats: vec![DecoderFrameFormat::Bgra8888],
        })
        .collect(),
        supports_hardware_decode: cfg!(target_os = "windows"),
        supports_cpu_video_frames: false,
        supports_audio_frames: false,
        supports_gpu_handles: cfg!(target_os = "windows"),
        supports_flush: true,
        supports_drain: true,
        max_sessions: Some(1),
    }
}

fn decoder_native_requirements() -> DecoderNativeRequirements {
    DecoderNativeRequirements {
        required_device_context_kinds: vec![DecoderNativeDeviceContextKind::D3D11Device],
        output_handle_kinds: vec![DecoderNativeHandleKind::D3D11Texture2D],
        requires_native_device_context: true,
        accepted_bitstream_formats: vec![
            DecoderBitstreamFormat::AnnexB,
            DecoderBitstreamFormat::Avcc,
            DecoderBitstreamFormat::Hvcc,
        ],
    }
}

fn decode_json<T: serde::de::DeserializeOwned>(
    data: *const u8,
    len: usize,
) -> Result<T, DecoderError> {
    if data.is_null() && len > 0 {
        return Err(DecoderError::abi_violation(
            "decoder JSON pointer was null with non-zero len",
        ));
    }
    let payload = if data.is_null() || len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    serde_json::from_slice(payload).map_err(|error| DecoderError::payload_codec(error.to_string()))
}

fn serialize_payload<T: serde::Serialize>(value: &T) -> VesperPluginBytes {
    match serde_json::to_vec(value) {
        Ok(payload) => VesperPluginBytes::from_vec(payload),
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

fn process_success<T: serde::Serialize>(value: &T) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: serialize_payload(value),
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

#[cfg(target_os = "windows")]
mod platform {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::mem::ManuallyDrop;
    use std::ptr;
    use std::sync::OnceLock;

    use player_plugin::{
        DecoderBitstreamFormat, DecoderError, DecoderFrameFormat, DecoderNativeDeviceContextKind,
        DecoderNativeHandleKind, DecoderPacket, DecoderPacketResult, DecoderSessionConfig,
    };
    use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
    use windows::Win32::Media::MediaFoundation::{
        IMFActivate, IMFDXGIBuffer, IMFMediaType, IMFSample, IMFTransform, MF_E_NOTACCEPTING,
        MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
        MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
        MF_MT_MPEG_SEQUENCE_HEADER, MF_MT_MPEG2_ONE_FRAME_PER_PACKET, MF_MT_SUBTYPE, MF_VERSION,
        MFCreateDXGIDeviceManager, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
        MFMediaType_Video, MFStartup, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_HARDWARE,
        MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT, MFT_MESSAGE_COMMAND_DRAIN,
        MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
        MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
        MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER, MFT_REGISTER_TYPE_INFO, MFTEnumEx,
        MFVideoFormat_H264, MFVideoFormat_H264_ES, MFVideoFormat_HEVC, MFVideoFormat_NV12,
        MFVideoInterlace_Progressive,
    };
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::core::{IUnknown, Interface};

    const HNS_PER_MICROSECOND: i64 = 10;

    pub enum ReceiveNativeFrame {
        Frame(NativeFrame),
        NeedMoreInput,
        Eof,
    }

    pub struct NativeFrame {
        pub pts_us: Option<i64>,
        pub duration_us: Option<i64>,
        pub width: u32,
        pub height: u32,
        pub coded_width: u32,
        pub coded_height: u32,
        pub format: DecoderFrameFormat,
        pub handle_kind: DecoderNativeHandleKind,
        pub handle: usize,
        pub frame_id: u64,
    }

    pub struct SessionInner {
        decoder: IMFTransform,
        width: u32,
        height: u32,
        outstanding_textures: HashMap<usize, ID3D11Texture2D>,
        stream_started: bool,
        draining: bool,
        eof_sent: bool,
        next_frame_id: u64,
    }

    impl SessionInner {
        pub fn open(config: &DecoderSessionConfig) -> Result<Self, DecoderError> {
            ensure_media_foundation_started()?;
            let Some(context) = config.native_device_context.as_ref() else {
                return Err(DecoderError::NotConfigured);
            };
            if context.kind != DecoderNativeDeviceContextKind::D3D11Device || context.handle == 0 {
                return Err(DecoderError::NotConfigured);
            }
            let raw = context.handle as *mut c_void;
            let device = unsafe {
                ID3D11Device::from_raw_borrowed(&raw)
                    .map(|device| device.clone())
                    .ok_or_else(|| {
                        DecoderError::abi_violation(
                            "D3D11 decoder received an invalid D3D11Device handle",
                        )
                    })?
            };
            let width = config.width.unwrap_or(super::DEFAULT_WIDTH).max(1);
            let height = config.height.unwrap_or(super::DEFAULT_HEIGHT).max(1);
            let coded_width = config.coded_width.unwrap_or(width).max(1);
            let coded_height = config.coded_height.unwrap_or(height).max(1);
            let input_subtype = codec_input_subtype(config)?;
            let decoder = open_hardware_decoder(&device, input_subtype)?;
            configure_decoder(
                &decoder,
                &device,
                config,
                input_subtype,
                coded_width,
                coded_height,
            )?;
            Ok(Self {
                decoder,
                width,
                height,
                outstanding_textures: HashMap::new(),
                stream_started: false,
                draining: false,
                eof_sent: false,
                next_frame_id: 1,
            })
        }

        pub fn send_packet(
            &mut self,
            packet: &DecoderPacket,
            data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            if data.is_empty() {
                return Ok(DecoderPacketResult { accepted: true });
            }
            self.start_stream_if_needed()?;
            let sample = create_input_sample(packet, data)?;
            match unsafe { self.decoder.ProcessInput(0, &sample, 0) } {
                Ok(()) => Ok(DecoderPacketResult { accepted: true }),
                Err(error) if error.code() == MF_E_NOTACCEPTING => {
                    Ok(DecoderPacketResult { accepted: false })
                }
                Err(error) => Err(mf_error("IMFTransform::ProcessInput", error)),
            }
        }

        pub fn send_end_of_stream(&mut self) -> Result<DecoderPacketResult, DecoderError> {
            if self.draining {
                return Ok(DecoderPacketResult { accepted: true });
            }
            self.start_stream_if_needed()?;
            unsafe {
                self.decoder
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
                    .map_err(|error| mf_error("MFT_MESSAGE_NOTIFY_END_OF_STREAM", error))?;
                self.decoder
                    .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                    .map_err(|error| mf_error("MFT_MESSAGE_COMMAND_DRAIN", error))?;
            }
            self.draining = true;
            self.eof_sent = false;
            Ok(DecoderPacketResult { accepted: true })
        }

        pub fn receive_native_frame(&mut self) -> Result<ReceiveNativeFrame, DecoderError> {
            if self.eof_sent {
                return Ok(ReceiveNativeFrame::Eof);
            }

            let mut output = MFT_OUTPUT_DATA_BUFFER::default();
            let mut status = 0u32;
            match unsafe {
                self.decoder
                    .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
            } {
                Ok(()) => {
                    let _events = unsafe { ManuallyDrop::take(&mut output.pEvents) };
                    let sample =
                        unsafe { ManuallyDrop::take(&mut output.pSample) }.ok_or_else(|| {
                            DecoderError::internal(
                                "D3D11 Media Foundation decoder returned no output sample",
                            )
                        })?;
                    self.native_frame_from_sample(sample)
                        .map(ReceiveNativeFrame::Frame)
                }
                Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    if self.draining {
                        self.eof_sent = true;
                        Ok(ReceiveNativeFrame::Eof)
                    } else {
                        Ok(ReceiveNativeFrame::NeedMoreInput)
                    }
                }
                Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    self.set_output_type()?;
                    Ok(ReceiveNativeFrame::NeedMoreInput)
                }
                Err(error) => Err(mf_error("IMFTransform::ProcessOutput", error)),
            }
        }

        pub fn release_frame_texture(&mut self, handle: usize) -> Result<(), DecoderError> {
            self.outstanding_textures
                .remove(&handle)
                .map(|_| ())
                .ok_or_else(|| {
                    DecoderError::abi_violation(
                        "D3D11 decoder release received an unknown texture handle",
                    )
                })
        }

        pub fn flush(&mut self) -> Result<(), DecoderError> {
            unsafe {
                self.decoder
                    .ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)
                    .map_err(|error| mf_error("MFT_MESSAGE_COMMAND_FLUSH", error))?;
            }
            self.outstanding_textures.clear();
            self.draining = false;
            self.eof_sent = false;
            Ok(())
        }

        fn start_stream_if_needed(&mut self) -> Result<(), DecoderError> {
            if self.stream_started {
                return Ok(());
            }
            unsafe {
                self.decoder
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                    .map_err(|error| mf_error("MFT_MESSAGE_NOTIFY_BEGIN_STREAMING", error))?;
                self.decoder
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                    .map_err(|error| mf_error("MFT_MESSAGE_NOTIFY_START_OF_STREAM", error))?;
            }
            self.stream_started = true;
            Ok(())
        }

        fn set_output_type(&self) -> Result<(), DecoderError> {
            let output_type =
                create_video_media_type(MFVideoFormat_NV12, self.width, self.height, None, true)?;
            unsafe {
                self.decoder
                    .SetOutputType(0, &output_type, 0)
                    .map_err(|error| mf_error("IMFTransform::SetOutputType", error))
            }
        }

        fn native_frame_from_sample(
            &mut self,
            sample: IMFSample,
        ) -> Result<NativeFrame, DecoderError> {
            let pts_us =
                unsafe { sample.GetSampleTime().ok() }.map(|value| value / HNS_PER_MICROSECOND);
            let duration_us =
                unsafe { sample.GetSampleDuration().ok() }.map(|value| value / HNS_PER_MICROSECOND);
            let buffer = unsafe { sample.GetBufferByIndex(0) }
                .map_err(|error| mf_error("IMFSample::GetBufferByIndex", error))?;
            let dxgi_buffer: IMFDXGIBuffer = buffer
                .cast()
                .map_err(|error| mf_error("IMFMediaBuffer::cast<IMFDXGIBuffer>", error))?;
            let mut resource = ptr::null_mut();
            unsafe {
                dxgi_buffer
                    .GetResource(&ID3D11Texture2D::IID, &mut resource)
                    .map_err(|error| mf_error("IMFDXGIBuffer::GetResource", error))?;
            }
            if resource.is_null() {
                return Err(DecoderError::internal(
                    "D3D11 Media Foundation decoder returned a null texture resource",
                ));
            }
            let texture = unsafe { ID3D11Texture2D::from_raw(resource) };
            let handle = texture.as_raw() as usize;
            self.outstanding_textures.insert(handle, texture);
            let frame_id = self.next_frame_id;
            self.next_frame_id = self.next_frame_id.saturating_add(1);
            Ok(NativeFrame {
                pts_us,
                duration_us,
                width: self.width,
                height: self.height,
                coded_width: self.width,
                coded_height: self.height,
                format: DecoderFrameFormat::Nv12,
                handle_kind: DecoderNativeHandleKind::D3D11Texture2D,
                handle,
                frame_id,
            })
        }
    }

    fn ensure_media_foundation_started() -> Result<(), DecoderError> {
        static STARTED: OnceLock<Result<(), String>> = OnceLock::new();
        STARTED
            .get_or_init(|| unsafe { MFStartup(MF_VERSION, 0) }.map_err(|error| error.to_string()))
            .clone()
            .map_err(|message| DecoderError::internal(format!("MFStartup failed: {message}")))
    }

    fn open_hardware_decoder(
        device: &ID3D11Device,
        input_subtype: windows::core::GUID,
    ) -> Result<IMFTransform, DecoderError> {
        let input = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: input_subtype,
        };
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };
        let mut activates = ptr::null_mut::<Option<IMFActivate>>();
        let mut count = 0u32;
        unsafe {
            MFTEnumEx(
                MFT_CATEGORY_VIDEO_DECODER,
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
                Some(&input),
                Some(&output),
                &mut activates,
                &mut count,
            )
            .map_err(|error| mf_error("MFTEnumEx", error))?;
        }
        if activates.is_null() || count == 0 {
            return Err(DecoderError::NotConfigured);
        }

        let mut selected = None;
        let entries = unsafe { std::slice::from_raw_parts_mut(activates, count as usize) };
        for entry in entries.iter_mut() {
            if selected.is_none() {
                selected = entry.take();
            } else {
                let _ = entry.take();
            }
        }
        unsafe { CoTaskMemFree(Some(activates.cast::<c_void>())) };

        let activate =
            selected.ok_or_else(|| DecoderError::internal("MFTEnumEx returned an empty entry"))?;
        let decoder = unsafe { activate.ActivateObject::<IMFTransform>() }
            .map_err(|error| mf_error("IMFActivate::ActivateObject<IMFTransform>", error))?;
        let mut token = 0u32;
        let mut manager = None;
        unsafe {
            MFCreateDXGIDeviceManager(&mut token, &mut manager)
                .map_err(|error| mf_error("MFCreateDXGIDeviceManager", error))?;
        }
        let manager = manager.ok_or_else(|| {
            DecoderError::internal("MFCreateDXGIDeviceManager returned no device manager")
        })?;
        let unknown: IUnknown = device
            .cast()
            .map_err(|error| mf_error("ID3D11Device::cast<IUnknown>", error))?;
        unsafe {
            manager
                .ResetDevice(&unknown, token)
                .map_err(|error| mf_error("IMFDXGIDeviceManager::ResetDevice", error))?;
            decoder
                .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)
                .map_err(|error| mf_error("MFT_MESSAGE_SET_D3D_MANAGER", error))?;
        }
        Ok(decoder)
    }

    fn configure_decoder(
        decoder: &IMFTransform,
        _device: &ID3D11Device,
        config: &DecoderSessionConfig,
        input_subtype: windows::core::GUID,
        width: u32,
        height: u32,
    ) -> Result<(), DecoderError> {
        let input_type = create_video_media_type(
            input_subtype,
            width,
            height,
            (!config.extradata.is_empty()).then_some(config.extradata.as_slice()),
            false,
        )?;
        unsafe {
            decoder
                .SetInputType(0, &input_type, 0)
                .map_err(|error| mf_error("IMFTransform::SetInputType", error))?;
        }
        let output_type = create_video_media_type(MFVideoFormat_NV12, width, height, None, true)?;
        unsafe {
            decoder
                .SetOutputType(0, &output_type, 0)
                .map_err(|error| mf_error("IMFTransform::SetOutputType", error))?;
        }
        Ok(())
    }

    fn create_video_media_type(
        subtype: windows::core::GUID,
        width: u32,
        height: u32,
        extradata: Option<&[u8]>,
        all_samples_independent: bool,
    ) -> Result<IMFMediaType, DecoderError> {
        let media_type =
            unsafe { MFCreateMediaType() }.map_err(|error| mf_error("MFCreateMediaType", error))?;
        let frame_size = (u64::from(width) << 32) | u64::from(height);
        unsafe {
            media_type
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(|error| mf_error("IMFMediaType::SetGUID(MF_MT_MAJOR_TYPE)", error))?;
            media_type
                .SetGUID(&MF_MT_SUBTYPE, &subtype)
                .map_err(|error| mf_error("IMFMediaType::SetGUID(MF_MT_SUBTYPE)", error))?;
            media_type
                .SetUINT64(&MF_MT_FRAME_SIZE, frame_size)
                .map_err(|error| mf_error("IMFMediaType::SetUINT64(MF_MT_FRAME_SIZE)", error))?;
            media_type
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .map_err(|error| {
                    mf_error("IMFMediaType::SetUINT32(MF_MT_INTERLACE_MODE)", error)
                })?;
            media_type
                .SetUINT32(&MF_MT_MPEG2_ONE_FRAME_PER_PACKET, 1)
                .map_err(|error| {
                    mf_error(
                        "IMFMediaType::SetUINT32(MF_MT_MPEG2_ONE_FRAME_PER_PACKET)",
                        error,
                    )
                })?;
            if all_samples_independent {
                media_type
                    .SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)
                    .map_err(|error| {
                        mf_error(
                            "IMFMediaType::SetUINT32(MF_MT_ALL_SAMPLES_INDEPENDENT)",
                            error,
                        )
                    })?;
            }
            if let Some(extradata) = extradata {
                media_type
                    .SetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, extradata)
                    .map_err(|error| {
                        mf_error("IMFMediaType::SetBlob(MF_MT_MPEG_SEQUENCE_HEADER)", error)
                    })?;
            }
        }
        Ok(media_type)
    }

    fn create_input_sample(packet: &DecoderPacket, data: &[u8]) -> Result<IMFSample, DecoderError> {
        let buffer_len = u32::try_from(data.len()).map_err(|_| {
            DecoderError::internal("D3D11 Media Foundation packet is too large for IMFMediaBuffer")
        })?;
        let buffer = unsafe { MFCreateMemoryBuffer(buffer_len) }
            .map_err(|error| mf_error("MFCreateMemoryBuffer", error))?;
        let mut destination = ptr::null_mut();
        unsafe {
            buffer
                .Lock(&mut destination, None, None)
                .map_err(|error| mf_error("IMFMediaBuffer::Lock", error))?;
            if !destination.is_null() {
                ptr::copy_nonoverlapping(data.as_ptr(), destination, data.len());
            }
            buffer
                .Unlock()
                .map_err(|error| mf_error("IMFMediaBuffer::Unlock", error))?;
            buffer
                .SetCurrentLength(buffer_len)
                .map_err(|error| mf_error("IMFMediaBuffer::SetCurrentLength", error))?;
        }
        let sample =
            unsafe { MFCreateSample() }.map_err(|error| mf_error("MFCreateSample", error))?;
        unsafe {
            sample
                .AddBuffer(&buffer)
                .map_err(|error| mf_error("IMFSample::AddBuffer", error))?;
            if let Some(pts_us) = packet.pts_us {
                sample
                    .SetSampleTime(pts_us.saturating_mul(HNS_PER_MICROSECOND))
                    .map_err(|error| mf_error("IMFSample::SetSampleTime", error))?;
            }
            if let Some(duration_us) = packet.duration_us {
                sample
                    .SetSampleDuration(duration_us.saturating_mul(HNS_PER_MICROSECOND))
                    .map_err(|error| mf_error("IMFSample::SetSampleDuration", error))?;
            }
        }
        Ok(sample)
    }

    fn codec_input_subtype(
        config: &DecoderSessionConfig,
    ) -> Result<windows::core::GUID, DecoderError> {
        let codec = config.codec.to_ascii_uppercase();
        let bitstream = config.bitstream_format.as_ref();
        match codec.as_str() {
            "H264" | "AVC1" | "AVC3" => match bitstream {
                Some(DecoderBitstreamFormat::AnnexB) => Ok(MFVideoFormat_H264_ES),
                _ => Ok(MFVideoFormat_H264),
            },
            "HEVC" | "H265" | "HVC1" | "HEV1" => Ok(MFVideoFormat_HEVC),
            _ => Err(DecoderError::UnsupportedCodec {
                codec: config.codec.clone(),
            }),
        }
    }

    fn mf_error(context: &str, error: windows::core::Error) -> DecoderError {
        DecoderError::internal(format!("{context} failed: {error}"))
    }
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
mod platform {
    use player_plugin::{
        DecoderError, DecoderFrameFormat, DecoderNativeHandleKind, DecoderPacket,
        DecoderPacketResult, DecoderSessionConfig,
    };

    pub struct SessionInner;

    pub enum ReceiveNativeFrame {
        Frame(NativeFrame),
        NeedMoreInput,
        Eof,
    }

    pub struct NativeFrame {
        pub pts_us: Option<i64>,
        pub duration_us: Option<i64>,
        pub width: u32,
        pub height: u32,
        pub coded_width: u32,
        pub coded_height: u32,
        pub format: DecoderFrameFormat,
        pub handle_kind: DecoderNativeHandleKind,
        pub handle: usize,
        pub frame_id: u64,
    }

    impl SessionInner {
        pub fn open(_config: &DecoderSessionConfig) -> Result<Self, DecoderError> {
            Err(DecoderError::NotConfigured)
        }

        pub fn send_packet(
            &mut self,
            _packet: &DecoderPacket,
            _data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            Err(DecoderError::NotConfigured)
        }

        pub fn send_end_of_stream(&mut self) -> Result<DecoderPacketResult, DecoderError> {
            Err(DecoderError::NotConfigured)
        }

        pub fn receive_native_frame(&mut self) -> Result<ReceiveNativeFrame, DecoderError> {
            Err(DecoderError::NotConfigured)
        }

        pub fn release_frame_texture(&mut self, _handle: usize) -> Result<(), DecoderError> {
            Err(DecoderError::NotConfigured)
        }

        pub fn flush(&mut self) -> Result<(), DecoderError> {
            Err(DecoderError::NotConfigured)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HANDLE_KIND_D3D11_TEXTURE_2D, decode_json, decoder_capabilities,
        decoder_native_requirements, vesper_plugin_entry,
    };
    use player_plugin::{
        DecoderBitstreamFormat, DecoderError, DecoderMediaKind, DecoderNativeDeviceContext,
        DecoderNativeDeviceContextKind, DecoderNativeHandleKind, DecoderNativeRequirements,
        DecoderReceiveFrameStatus, DecoderReceiveNativeFrameMetadata, DecoderSessionConfig,
        VESPER_DECODER_PLUGIN_ABI_VERSION_V2, VesperDecoderPluginApiV2, VesperPluginKind,
        VesperPluginResultStatus,
    };

    #[test]
    fn exported_descriptor_matches_native_decoder_plugin_metadata() {
        let descriptor = unsafe { vesper_plugin_entry().as_ref() }.expect("descriptor");

        assert_eq!(descriptor.abi_version, VESPER_DECODER_PLUGIN_ABI_VERSION_V2);
        assert_eq!(descriptor.plugin_kind, VesperPluginKind::Decoder);
        assert!(!descriptor.api.is_null());
        assert!(!descriptor.plugin_name.is_null());
    }

    #[test]
    fn capabilities_advertise_windows_d3d11_native_frames() {
        let capabilities = decoder_capabilities();

        assert_eq!(
            capabilities.supports_hardware_decode,
            cfg!(target_os = "windows")
        );
        assert_eq!(
            capabilities.supports_gpu_handles,
            cfg!(target_os = "windows")
        );
        assert!(!capabilities.supports_cpu_video_frames);
        assert!(capabilities.supports_codec("H264", DecoderMediaKind::Video));
        assert!(capabilities.supports_codec("hvc1", DecoderMediaKind::Video));
    }

    #[test]
    fn native_requirements_advertise_d3d11_device_and_bitstreams() {
        let requirements = decoder_native_requirements();

        assert!(requirements.requires_native_device_context);
        assert_eq!(
            requirements.required_device_context_kinds,
            vec![DecoderNativeDeviceContextKind::D3D11Device]
        );
        assert_eq!(
            requirements.output_handle_kinds,
            vec![DecoderNativeHandleKind::D3D11Texture2D]
        );
        assert!(
            requirements
                .accepted_bitstream_formats
                .contains(&DecoderBitstreamFormat::Avcc)
        );
        assert!(
            requirements
                .accepted_bitstream_formats
                .contains(&DecoderBitstreamFormat::Hvcc)
        );
    }

    #[test]
    fn exported_descriptor_exposes_native_requirements_callback() {
        let descriptor = unsafe { vesper_plugin_entry().as_ref() }.expect("descriptor");
        let api = unsafe { descriptor.api.cast::<VesperDecoderPluginApiV2>().as_ref() }
            .expect("native decoder api");
        let callback = api
            .native_requirements_json
            .expect("native requirements callback");
        let payload = unsafe { callback(api.context) };
        let requirements = decode_json::<DecoderNativeRequirements>(payload.data, payload.len)
            .expect("requirements payload");

        assert!(requirements.requires_native_device_context);
        assert_eq!(
            requirements.output_handle_kinds,
            vec![DecoderNativeHandleKind::D3D11Texture2D]
        );
        unsafe { super::free_plugin_bytes(api.context, payload) };
    }

    #[test]
    fn open_session_rejects_missing_device_context() {
        let config = DecoderSessionConfig {
            codec: "H264".to_owned(),
            media_kind: DecoderMediaKind::Video,
            prefer_hardware: true,
            ..DecoderSessionConfig::default()
        };
        let payload = serde_json::to_vec(&config).expect("config json");

        let result = unsafe {
            super::decoder_open_session_json(std::ptr::null_mut(), payload.as_ptr(), payload.len())
        };

        assert_eq!(result.status, VesperPluginResultStatus::Failure);
        assert!(result.session.is_null());
        let error = decode_json::<DecoderError>(result.payload.data, result.payload.len)
            .expect("error payload");
        assert_eq!(error, DecoderError::NotConfigured);
        unsafe { super::free_plugin_bytes(std::ptr::null_mut(), result.payload) };
    }

    #[test]
    fn native_frame_metadata_round_trips_eof_status() {
        let metadata = DecoderReceiveNativeFrameMetadata {
            status: DecoderReceiveFrameStatus::Eof,
            frame: None,
        };
        let payload = super::serialize_payload(&metadata);
        let decoded = decode_json::<DecoderReceiveNativeFrameMetadata>(payload.data, payload.len)
            .expect("metadata payload");

        assert_eq!(decoded.status, DecoderReceiveFrameStatus::Eof);
        unsafe { super::free_plugin_bytes(std::ptr::null_mut(), payload) };
    }

    #[test]
    fn device_context_kind_uses_d3d11_device_contract() {
        let context = DecoderNativeDeviceContext {
            kind: DecoderNativeDeviceContextKind::D3D11Device,
            handle: 42,
        };

        assert_eq!(context.kind, DecoderNativeDeviceContextKind::D3D11Device);
        assert_eq!(HANDLE_KIND_D3D11_TEXTURE_2D, 6);
    }
}
