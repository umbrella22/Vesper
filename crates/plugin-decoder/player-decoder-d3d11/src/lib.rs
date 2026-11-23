#![cfg(any(test, target_os = "windows"))]
#![warn(clippy::undocumented_unsafe_blocks)]

//! D3D11 native-frame decoder plugin.
//!
//! Windows builds expose the native entry backed by D3D11 sessions.
//! Contract tests also run on other platforms.

use player_plugin::{
    DecoderBitstreamFormat, DecoderCapabilities, DecoderCodecCapability, DecoderFrameFormat,
    DecoderMediaKind, DecoderNativeDeviceContextKind, DecoderNativeHandleKind,
    DecoderNativeRequirements, NativeFramePipelineProfile,
};
#[cfg(test)]
use player_plugin::{
    DecoderError, DecoderNativeFrame, DecoderNativeFrameMetadata,
    DecoderNativeFrameReleaseTracking, DecoderPacket, DecoderPacketResult,
    DecoderReceiveNativeFrameOutput, DecoderSessionConfig, DecoderSessionInfo,
    NativeDecoderPluginFactory, NativeDecoderSession,
};

#[cfg(target_os = "windows")]
const PLUGIN_ID: &str = "io.github.umbrella22.vesper.decoder-d3d11";
const PLUGIN_NAME: &str = "player-decoder-d3d11";
#[cfg(target_os = "windows")]
const DEFAULT_WIDTH: u32 = 16;
#[cfg(target_os = "windows")]
const DEFAULT_HEIGHT: u32 = 16;

#[cfg(test)]
#[derive(Debug, Default)]
struct D3D11DecoderFactory;

#[cfg(test)]
impl NativeDecoderPluginFactory for D3D11DecoderFactory {
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
        Ok(Box::new(D3D11DecoderSession::open(config.clone())?))
    }
}

#[cfg(test)]
struct D3D11DecoderSession {
    codec: String,
    inner: Option<platform::SessionInner>,
    eof_received: bool,
    eof_sent: bool,
}

#[cfg(test)]
impl D3D11DecoderSession {
    fn open(config: DecoderSessionConfig) -> Result<Self, DecoderError> {
        if !decoder_capabilities().supports_codec(&config.codec, config.media_kind) {
            return Err(DecoderError::UnsupportedCodec {
                codec: config.codec,
            });
        }
        if config.require_cpu_output {
            return Err(DecoderError::UnsupportedCapability {
                capability: "cpu-video-frame-output".to_owned(),
            });
        }

        let inner = platform::SessionInner::open(&config)?;

        Ok(Self {
            codec: config.codec,
            inner: Some(inner),
            eof_received: false,
            eof_sent: false,
        })
    }

    fn inner_mut(&mut self) -> Result<&mut platform::SessionInner, DecoderError> {
        self.inner.as_mut().ok_or(DecoderError::NotConfigured)
    }

    fn send_packet_impl(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        if packet.discontinuity {
            self.inner_mut()?.reset_decode_state()?;
            self.eof_received = false;
            self.eof_sent = false;
        }

        if packet.end_of_stream {
            self.eof_received = true;
            return self.inner_mut()?.send_end_of_stream();
        }

        self.inner_mut()?.send_packet(packet, data)
    }

    fn receive_native_frame_impl(
        &mut self,
    ) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
        match self.inner_mut()?.receive_native_frame()? {
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
                    d3d11_subresource_index: None,
                    pipeline_profile: Some(NativeFramePipelineProfile::D3D11Texture2D),
                    color_space: None,
                    hdr_metadata: None,
                    color: None,
                    hdr: None,
                    sync_info: None,
                    transform: None,
                    frame_id: Some(frame.frame_id),
                    release_tracking: Some(DecoderNativeFrameReleaseTracking {
                        frame_id: Some(frame.frame_id),
                        requires_release: true,
                    }),
                };
                Ok(DecoderReceiveNativeFrameOutput::Frame(DecoderNativeFrame {
                    metadata,
                    handle: frame.handle,
                    lease_token: None,
                }))
            }
            platform::ReceiveNativeFrame::NeedMoreInput => {
                if self.eof_received && !self.eof_sent {
                    self.eof_sent = true;
                    return Ok(DecoderReceiveNativeFrameOutput::Eof);
                }
                Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput)
            }
            platform::ReceiveNativeFrame::Eof => {
                self.eof_sent = true;
                Ok(DecoderReceiveNativeFrameOutput::Eof)
            }
        }
    }

    fn release_native_frame_impl(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
        if frame.metadata.handle_kind != DecoderNativeHandleKind::D3D11Texture2D
            || frame.handle == 0
        {
            return Err(DecoderError::abi_violation(
                "D3D11 decoder release received an invalid texture handle",
            ));
        }
        let frame_id = frame.metadata.frame_id.ok_or_else(|| {
            DecoderError::abi_violation("D3D11 decoder release is missing its frame id")
        })?;
        self.inner_mut()?
            .release_frame_texture(frame_id, frame.handle)
    }

    fn flush_impl(&mut self) -> Result<(), DecoderError> {
        self.inner_mut()?.flush()?;
        self.eof_received = false;
        self.eof_sent = false;
        Ok(())
    }

    fn close_impl(&mut self) -> Result<(), DecoderError> {
        self.eof_received = false;
        self.eof_sent = false;
        let Some(inner) = self.inner.take() else {
            return Ok(());
        };
        inner.close()
    }
}

#[cfg(test)]
impl NativeDecoderSession for D3D11DecoderSession {
    fn session_info(&self) -> DecoderSessionInfo {
        DecoderSessionInfo {
            decoder_name: Some(PLUGIN_NAME.to_owned()),
            selected_hardware_backend: Some("D3D11".to_owned()),
            output_format: Some(DecoderFrameFormat::Nv12),
        }
    }

    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        self.send_packet_impl(packet, data)
    }

    fn receive_native_frame(&mut self) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
        self.receive_native_frame_impl()
    }

    fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
        self.release_native_frame_impl(frame)
    }

    fn flush(&mut self) -> Result<(), DecoderError> {
        self.flush_impl()
    }

    fn close(&mut self) -> Result<(), DecoderError> {
        self.close_impl()
    }
}

#[cfg(test)]
impl Drop for D3D11DecoderSession {
    fn drop(&mut self) {
        let _ = self.close_impl();
    }
}

fn decoder_capabilities() -> DecoderCapabilities {
    DecoderCapabilities {
        codecs: if cfg!(target_os = "windows") {
            [
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
                output_formats: vec![DecoderFrameFormat::Nv12],
            })
            .collect()
        } else {
            Vec::new()
        },
        supports_hardware_decode: cfg!(target_os = "windows"),
        supports_cpu_video_frames: false,
        supports_audio_frames: false,
        supports_pcm_frames: false,
        supports_gpu_handles: cfg!(target_os = "windows"),
        supports_presentation_release: false,
        supports_flush: true,
        supports_drain: true,
        max_sessions: Some(1),
    }
}

fn decoder_native_requirements() -> DecoderNativeRequirements {
    DecoderNativeRequirements {
        required_device_context_kinds: vec![DecoderNativeDeviceContextKind::D3D11Device],
        output_handle_kinds: vec![DecoderNativeHandleKind::D3D11Texture2D],
        output_pipeline_profiles: vec![NativeFramePipelineProfile::D3D11Texture2D],
        requires_native_device_context: true,
        accepted_bitstream_formats: vec![
            DecoderBitstreamFormat::AnnexB,
            DecoderBitstreamFormat::Avcc,
            DecoderBitstreamFormat::Hvcc,
        ],
    }
}

#[cfg(target_os = "windows")]
mod plugin_export;

#[cfg(target_os = "windows")]
mod platform {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::marker::PhantomData;
    use std::mem::ManuallyDrop;
    use std::ptr;
    use std::rc::Rc;
    use std::sync::OnceLock;

    use player_plugin::{
        DecoderBitstreamFormat, DecoderError, DecoderFrameFormat, DecoderNativeHandleKind,
        DecoderPacket, DecoderPacketResult, DecoderSessionConfig,
        normalize_decoder_codec_identifier,
    };
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CREATE_DEVICE_SINGLETHREADED, ID3D11Device, ID3D11Texture2D,
    };
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
    use windows::Win32::System::Com::{
        COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
    };
    use windows::core::{AgileReference, IUnknown, Interface};

    const HNS_PER_MICROSECOND: i64 = 10;

    pub enum ReceiveNativeFrame {
        Frame(NativeFrame),
        NeedMoreInput,
        Eof,
    }

    pub struct NativeFrame {
        pub color: player_plugin_abi::VesperColorInfo,
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
        decoder: AgileReference<IMFTransform>,
        width: u32,
        height: u32,
        outstanding_textures: HashMap<u64, OutstandingTexture>,
        stream_started: bool,
        draining: bool,
        eof_sent: bool,
        next_frame_id: u64,
        output_color: Option<player_plugin_abi::VesperColorInfo>,
    }

    pub(super) struct OutstandingTexture {
        handle: usize,
        pub(super) subresource_index: u32,
        _texture: ID3D11Texture2D,
        // The sample retains the decoder surface slot, including its array slice.
        _sample: AgileReference<IMFSample>,
    }

    struct ComApartmentScope {
        uninitialize_on_drop: bool,
        _thread_bound: PhantomData<Rc<()>>,
    }

    impl ComApartmentScope {
        fn enter() -> Result<Self, DecoderError> {
            // SAFETY: the reserved pointer is null. Each successful call is
            // balanced by this thread-bound scope's `Drop` implementation.
            let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if status == RPC_E_CHANGED_MODE {
                return Ok(Self {
                    uninitialize_on_drop: false,
                    _thread_bound: PhantomData,
                });
            }
            status
                .ok()
                .map_err(|error| mf_error("CoInitializeEx(COINIT_MULTITHREADED)", error))?;
            Ok(Self {
                uninitialize_on_drop: true,
                _thread_bound: PhantomData,
            })
        }
    }

    impl Drop for ComApartmentScope {
        fn drop(&mut self) {
            if self.uninitialize_on_drop {
                // SAFETY: the scope is not `Send` and balances a successful
                // `CoInitializeEx` call made on this same thread.
                unsafe { CoUninitialize() };
            }
        }
    }

    impl SessionInner {
        #[cfg(test)]
        pub fn open(config: &DecoderSessionConfig) -> Result<Self, DecoderError> {
            Self::open_with_sharing(config, false)
        }

        pub(super) fn open_with_sharing(
            config: &DecoderSessionConfig,
            share_textures: bool,
        ) -> Result<Self, DecoderError> {
            let _apartment = ComApartmentScope::enter()?;
            ensure_media_foundation_started()?;
            let Some(context) = config.native_device_context.as_ref() else {
                return Err(DecoderError::NotConfigured);
            };
            let Some(device_ptr) = context.d3d11_device_ptr() else {
                return Err(DecoderError::NotConfigured);
            };
            if device_ptr == 0 {
                return Err(DecoderError::NotConfigured);
            }
            let raw = device_ptr as *mut c_void;
            // SAFETY: The host supplies a live ID3D11Device for this call. Cloning the borrowed
            // interface takes a COM reference before the host borrow ends.
            let device = unsafe {
                ID3D11Device::from_raw_borrowed(&raw)
                    .cloned()
                    .ok_or_else(|| {
                        DecoderError::abi_violation(
                            "D3D11 decoder received an invalid D3D11Device handle",
                        )
                    })?
            };
            // SAFETY: The owned device reference is live for this read of its creation flags.
            let creation_flags = unsafe { device.GetCreationFlags() };
            if creation_flags & D3D11_CREATE_DEVICE_SINGLETHREADED.0 != 0 {
                return Err(DecoderError::UnsupportedCapability {
                    capability: "multithreaded-d3d11-device".to_owned(),
                });
            }
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
                share_textures,
            )?;
            let decoder = AgileReference::new(&decoder)
                .map_err(|error| mf_error("AgileReference<IMFTransform>::new", error))?;
            Ok(Self {
                decoder,
                width,
                height,
                outstanding_textures: HashMap::new(),
                stream_started: false,
                draining: false,
                eof_sent: false,
                next_frame_id: 1,
                output_color: None,
            })
        }

        pub fn send_packet(
            &mut self,
            packet: &DecoderPacket,
            data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            let apartment = ComApartmentScope::enter()?;
            if data.is_empty() {
                return Ok(DecoderPacketResult { accepted: true });
            }
            self.start_stream_if_needed_in_apartment(&apartment)?;
            let sample = create_input_sample(packet, data)?;
            let decoder = self.resolve_decoder(&apartment)?;
            // SAFETY: The transform is resolved in this apartment. The input sample and its buffer
            // remain live through ProcessInput on stream zero.
            match unsafe { decoder.ProcessInput(0, &sample, 0) } {
                Ok(()) => Ok(DecoderPacketResult { accepted: true }),
                Err(error) if error.code() == MF_E_NOTACCEPTING => {
                    Ok(DecoderPacketResult { accepted: false })
                }
                Err(error) => Err(mf_error("IMFTransform::ProcessInput", error)),
            }
        }

        pub fn send_end_of_stream(&mut self) -> Result<DecoderPacketResult, DecoderError> {
            let apartment = ComApartmentScope::enter()?;
            if self.draining {
                return Ok(DecoderPacketResult { accepted: true });
            }
            self.start_stream_if_needed_in_apartment(&apartment)?;
            let decoder = self.resolve_decoder(&apartment)?;
            // SAFETY: The transform is resolved in the current apartment. Neither end-of-stream
            // notification takes a pointer payload.
            unsafe {
                decoder
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
                    .map_err(|error| mf_error("MFT_MESSAGE_NOTIFY_END_OF_STREAM", error))?;
                decoder
                    .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                    .map_err(|error| mf_error("MFT_MESSAGE_COMMAND_DRAIN", error))?;
            }
            self.draining = true;
            self.eof_sent = false;
            Ok(DecoderPacketResult { accepted: true })
        }

        pub fn receive_native_frame(&mut self) -> Result<ReceiveNativeFrame, DecoderError> {
            let apartment = ComApartmentScope::enter()?;
            if self.eof_sent {
                return Ok(ReceiveNativeFrame::Eof);
            }

            let mut output = MFT_OUTPUT_DATA_BUFFER::default();
            let mut status = 0u32;
            let decoder = self.resolve_decoder(&apartment)?;
            // SAFETY: ProcessOutput receives one initialized, writable output entry and status word.
            // Both outlive the call on the apartment-local transform.
            match unsafe {
                decoder.ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
            } {
                Ok(()) => {
                    // SAFETY: On success the entry owns its optional event collection. Take that COM
                    // reference once so it is released on drop.
                    let _events = unsafe { ManuallyDrop::take(&mut output.pEvents) };
                    let sample =
                        // SAFETY: The successful output entry transfers its optional sample reference.
                        // The ManuallyDrop field is not read a second time.
                        unsafe { ManuallyDrop::take(&mut output.pSample) }.ok_or_else(|| {
                            DecoderError::internal(
                                "D3D11 Media Foundation decoder returned no output sample",
                            )
                        })?;
                    let color = *self
                        .output_color
                        .get_or_insert_with(|| read_output_color(&decoder));
                    self.native_frame_from_sample(sample, color)
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
                    self.output_color = None;
                    self.set_output_type_in_apartment(&apartment)?;
                    Ok(ReceiveNativeFrame::NeedMoreInput)
                }
                Err(error) => Err(mf_error("IMFTransform::ProcessOutput", error)),
            }
        }

        pub(super) fn take_frame_texture(&mut self, frame_id: u64) -> Option<OutstandingTexture> {
            self.outstanding_textures.remove(&frame_id)
        }

        pub fn release_frame_texture(
            &mut self,
            frame_id: u64,
            handle: usize,
        ) -> Result<(), DecoderError> {
            let _apartment = ComApartmentScope::enter()?;
            let Some(texture) = self.outstanding_textures.get(&frame_id) else {
                return Err(DecoderError::abi_violation(
                    "D3D11 decoder release received an unknown frame id",
                ));
            };
            if texture.handle != handle {
                return Err(DecoderError::abi_violation(
                    "D3D11 decoder release handle does not match its frame id",
                ));
            }
            self.outstanding_textures.remove(&frame_id);
            Ok(())
        }

        pub fn reset_decode_state(&mut self) -> Result<(), DecoderError> {
            let apartment = ComApartmentScope::enter()?;
            self.reset_decode_state_in_apartment(&apartment)
        }

        fn reset_decode_state_in_apartment(
            &mut self,
            apartment: &ComApartmentScope,
        ) -> Result<(), DecoderError> {
            let decoder = self.resolve_decoder(apartment)?;
            // SAFETY: The apartment-local transform is live and the flush command takes no payload.
            unsafe {
                decoder
                    .ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)
                    .map_err(|error| mf_error("MFT_MESSAGE_COMMAND_FLUSH", error))?;
            }
            self.draining = false;
            self.eof_sent = false;
            Ok(())
        }

        pub fn flush(&mut self) -> Result<(), DecoderError> {
            let apartment = ComApartmentScope::enter()?;
            self.reset_decode_state_in_apartment(&apartment)?;
            self.outstanding_textures.clear();
            Ok(())
        }

        pub fn close(mut self) -> Result<(), DecoderError> {
            let apartment = ComApartmentScope::enter()?;
            let flush_result = self.reset_decode_state_in_apartment(&apartment);
            self.outstanding_textures.clear();
            let Self {
                decoder,
                outstanding_textures,
                ..
            } = self;
            drop(outstanding_textures);
            drop(decoder);
            drop(apartment);
            flush_result
        }

        fn start_stream_if_needed_in_apartment(
            &mut self,
            apartment: &ComApartmentScope,
        ) -> Result<(), DecoderError> {
            if self.stream_started {
                return Ok(());
            }
            let decoder = self.resolve_decoder(apartment)?;
            // SAFETY: These stream notifications take no payload and use the transform
            // resolved for the calling apartment.
            unsafe {
                decoder
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                    .map_err(|error| mf_error("MFT_MESSAGE_NOTIFY_BEGIN_STREAMING", error))?;
                decoder
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                    .map_err(|error| mf_error("MFT_MESSAGE_NOTIFY_START_OF_STREAM", error))?;
            }
            self.stream_started = true;
            Ok(())
        }

        fn set_output_type_in_apartment(
            &self,
            apartment: &ComApartmentScope,
        ) -> Result<(), DecoderError> {
            let output_type =
                create_video_media_type(MFVideoFormat_NV12, self.width, self.height, None, true)?;
            let decoder = self.resolve_decoder(apartment)?;
            // SAFETY: The local media type and apartment-local transform remain live
            // through the stream-zero type negotiation.
            unsafe {
                decoder
                    .SetOutputType(0, &output_type, 0)
                    .map_err(|error| mf_error("IMFTransform::SetOutputType", error))
            }
        }

        fn resolve_decoder(
            &self,
            _apartment: &ComApartmentScope,
        ) -> Result<IMFTransform, DecoderError> {
            self.decoder
                .resolve()
                .map_err(|error| mf_error("AgileReference<IMFTransform>::resolve", error))
        }

        fn native_frame_from_sample(
            &mut self,
            sample: IMFSample,
            color: player_plugin_abi::VesperColorInfo,
        ) -> Result<NativeFrame, DecoderError> {
            let pts_us =
                // SAFETY: The sample owns a live IMFSample reference throughout this timestamp query.
                unsafe { sample.GetSampleTime().ok() }.map(|value| value / HNS_PER_MICROSECOND);
            let duration_us =
                // SAFETY: The same sample reference remains live for the duration query.
                unsafe { sample.GetSampleDuration().ok() }.map(|value| value / HNS_PER_MICROSECOND);
            // SAFETY: The sample is live. GetBufferByIndex reports a missing index via HRESULT.
            let buffer = unsafe { sample.GetBufferByIndex(0) }
                .map_err(|error| mf_error("IMFSample::GetBufferByIndex", error))?;
            let dxgi_buffer: IMFDXGIBuffer = buffer
                .cast()
                .map_err(|error| mf_error("IMFMediaBuffer::cast<IMFDXGIBuffer>", error))?;
            let mut resource = ptr::null_mut();
            // SAFETY: The DXGI buffer is live and the out-pointer is writable. The requested
            // IID fixes the interface type returned on success.
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
            // SAFETY: GetResource succeeded for the texture IID and returned a non-null owned
            // reference. from_raw takes that reference exactly once.
            let texture = unsafe { ID3D11Texture2D::from_raw(resource) };
            let mut desc = Default::default();
            // SAFETY: both COM objects are live and desc is writable.
            let subresource_index = unsafe {
                texture.GetDesc(&mut desc);
                dxgi_buffer.GetSubresourceIndex()
            }
            .map_err(|error| mf_error("IMFDXGIBuffer::GetSubresourceIndex", error))?;
            if desc.MipLevels != 1 || subresource_index >= desc.ArraySize {
                return Err(DecoderError::abi_violation(
                    "invalid D3D11 decoder subresource",
                ));
            }
            let sample = AgileReference::new(&sample)
                .map_err(|error| mf_error("AgileReference<IMFSample>::new", error))?;
            let handle = texture.as_raw() as usize;
            let frame_id = self.next_frame_id;
            self.next_frame_id = self.next_frame_id.checked_add(1).ok_or_else(|| {
                DecoderError::internal("D3D11 decoder frame id space is exhausted")
            })?;
            self.outstanding_textures.insert(
                frame_id,
                OutstandingTexture {
                    handle,
                    subresource_index,
                    _texture: texture,
                    _sample: sample,
                },
            );
            Ok(NativeFrame {
                color,
                pts_us,
                duration_us,
                width: self.width,
                height: self.height,
                coded_width: desc.Width,
                coded_height: desc.Height,
                format: DecoderFrameFormat::Nv12,
                handle_kind: DecoderNativeHandleKind::D3D11Texture2D,
                handle,
                frame_id,
            })
        }
    }

    #[allow(non_upper_case_globals)] // Media Foundation SDK enum constants.
    fn read_output_color(decoder: &IMFTransform) -> player_plugin_abi::VesperColorInfo {
        use player_plugin_abi::{
            VESPER_HDR_HDR10, VESPER_HDR_HLG, VESPER_HDR_SDR, VesperColorInfo,
        };
        use windows::Win32::Media::MediaFoundation::*;

        // SAFETY: the apartment-local transform has produced a sample on stream zero.
        let media_type = unsafe { decoder.GetOutputCurrentType(0) }.ok();
        let attribute = |key: &windows::core::GUID| {
            // SAFETY: the media type and GUID remain live through GetUINT32.
            media_type
                .as_ref()
                .and_then(|media_type| unsafe { media_type.GetUINT32(key) }.ok())
                .unwrap_or(0)
        };
        let primaries = match MFVideoPrimaries(attribute(&MF_MT_VIDEO_PRIMARIES) as i32) {
            MFVideoPrimaries_BT709 => 1,
            MFVideoPrimaries_BT470_2_SysM => 4,
            MFVideoPrimaries_BT470_2_SysBG => 5,
            MFVideoPrimaries_SMPTE170M | MFVideoPrimaries_SMPTE_C => 6,
            MFVideoPrimaries_SMPTE240M => 7,
            MFVideoPrimaries_BT2020 => 9,
            MFVideoPrimaries_XYZ => 10,
            MFVideoPrimaries_DCI_P3 => 11,
            MFVideoPrimaries_EBU3213 => 22,
            other => {
                if other != MFVideoPrimaries_Unknown {
                    tracing::debug!(raw = other.0, "Unmapped Media Foundation video primaries");
                }
                2
            }
        };
        let transfer = match MFVideoTransferFunction(attribute(&MF_MT_TRANSFER_FUNCTION) as i32) {
            MFVideoTransFunc_709 => 1,
            MFVideoTransFunc_22 => 4,
            MFVideoTransFunc_28 => 5,
            MFVideoTransFunc_240M => 7,
            MFVideoTransFunc_10 => 8,
            MFVideoTransFunc_Log_100 => 9,
            MFVideoTransFunc_Log_316 => 10,
            MFVideoTransFunc_sRGB => 13,
            MFVideoTransFunc_2084 => 16,
            MFVideoTransFunc_HLG => 18,
            other => {
                if other != MFVideoTransFunc_Unknown {
                    tracing::debug!(raw = other.0, "Unmapped Media Foundation transfer function");
                }
                2
            }
        };
        let matrix = match MFVideoTransferMatrix(attribute(&MF_MT_YUV_MATRIX) as i32) {
            MFVideoTransferMatrix_BT709 => 1,
            MFVideoTransferMatrix_BT601 if primaries == 5 => 5,
            MFVideoTransferMatrix_BT601 => 6,
            MFVideoTransferMatrix_SMPTE240M => 7,
            MFVideoTransferMatrix_BT2020_10 | MFVideoTransferMatrix_BT2020_12 => 9,
            other => {
                if other != MFVideoTransferMatrix_Unknown {
                    tracing::debug!(raw = other.0, "Unmapped Media Foundation YUV matrix");
                }
                2
            }
        };
        let range = match MFNominalRange(attribute(&MF_MT_VIDEO_NOMINAL_RANGE) as i32) {
            MFNominalRange_0_255 => 2,
            MFNominalRange_16_235 => 1,
            other => {
                if other != MFNominalRange_Unknown {
                    tracing::debug!(raw = other.0, "Unmapped Media Foundation nominal range");
                }
                0
            }
        };
        VesperColorInfo {
            primaries,
            transfer,
            matrix,
            range,
            bit_depth: 8,
            hdr_kind: match transfer {
                16 => VESPER_HDR_HDR10,
                18 => VESPER_HDR_HLG,
                _ => VESPER_HDR_SDR,
            },
            ..Default::default()
        }
    }

    fn ensure_media_foundation_started() -> Result<(), DecoderError> {
        static STARTED: OnceLock<Result<(), String>> = OnceLock::new();
        STARTED
            // SAFETY: OnceLock serializes startup. The caller has entered COM and the version
            // and startup flags are Media Foundation constants.
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
        // SAFETY: Both type filters are initialized. The output pointer and count remain
        // writable through the call and are consumed only on success.
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
        // SAFETY: MFTEnumEx returned a non-null array of count interface entries. We have
        // exclusive access until the matching CoTaskMemFree.
        let entries = unsafe { std::slice::from_raw_parts_mut(activates, count as usize) };
        for entry in entries.iter_mut() {
            if selected.is_none() {
                selected = entry.take();
            } else {
                let _ = entry.take();
            }
        }
        // SAFETY: Each COM reference was taken out of the array. The slice is no longer used
        // and the array is freed once with its matching allocator.
        unsafe { CoTaskMemFree(Some(activates.cast::<c_void>())) };

        let activate =
            selected.ok_or_else(|| DecoderError::internal("MFTEnumEx returned an empty entry"))?;
        // SAFETY: The selected activation object owns a live reference. The requested IID
        // determines the transform interface returned by ActivateObject.
        let decoder = unsafe { activate.ActivateObject::<IMFTransform>() }
            .map_err(|error| mf_error("IMFActivate::ActivateObject<IMFTransform>", error))?;
        let mut token = 0u32;
        let mut manager = None;
        // SAFETY: The reset token and manager out-parameter are initialized and writable
        // for the duration of the call.
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
        // SAFETY: The reset token came from this manager. The device and manager references
        // remain live through ResetDevice and the transform message.
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
        share_textures: bool,
    ) -> Result<(), DecoderError> {
        let input_type = create_video_media_type(
            input_subtype,
            width,
            height,
            (!config.extradata.is_empty()).then_some(config.extradata.as_slice()),
            false,
        )?;
        // SAFETY: The live transform borrows the initialized input type for this
        // stream-zero configuration call.
        unsafe {
            decoder
                .SetInputType(0, &input_type, 0)
                .map_err(|error| mf_error("IMFTransform::SetInputType", error))?;
        }
        if share_textures {
            // SAFETY: the transform is live in the current COM apartment. A
            // transform may ignore the request. The host still checks every texture.
            unsafe {
                if let Ok(attributes) = decoder.GetOutputStreamAttributes(0) {
                    let _ = attributes.SetUINT32(
                        &windows::Win32::Media::MediaFoundation::MF_SA_D3D11_SHARED_WITHOUT_MUTEX,
                        1,
                    );
                }
            }
        }
        let output_type = create_video_media_type(MFVideoFormat_NV12, width, height, None, true)?;
        // SAFETY: The live transform and initialized NV12 output type remain valid
        // for this stream-zero configuration call.
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
            // SAFETY: Media Foundation has been started and returns an owned media-type interface.
            unsafe { MFCreateMediaType() }.map_err(|error| mf_error("MFCreateMediaType", error))?;
        let frame_size = (u64::from(width) << 32) | u64::from(height);
        // SAFETY: The media type is owned here. All GUIDs and the optional sequence header
        // remain valid while Media Foundation copies their attribute values.
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
        // SAFETY: The checked u32 length describes the fresh buffer returned by Media Foundation.
        let buffer = unsafe { MFCreateMemoryBuffer(buffer_len) }
            .map_err(|error| mf_error("MFCreateMemoryBuffer", error))?;
        let mut destination = ptr::null_mut();
        // SAFETY: The new buffer has capacity for data.len() bytes and cannot overlap data.
        // A successful lock supplies the write pointer until the matching unlock.
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
            // SAFETY: Media Foundation is started and returns a new owned sample reference.
            unsafe { MFCreateSample() }.map_err(|error| mf_error("MFCreateSample", error))?;
        // SAFETY: The sample and buffer references are live for AddBuffer. The timestamp and
        // duration setters take scalar values.
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
        let codec = normalize_decoder_codec_identifier(&config.codec);
        let bitstream = config.bitstream_format.as_ref();
        match codec.as_str() {
            "h264" | "avc" | "avc1" | "avc3" => match bitstream {
                Some(DecoderBitstreamFormat::AnnexB) => Ok(MFVideoFormat_H264_ES),
                _ => Ok(MFVideoFormat_H264),
            },
            "hevc" | "h265" | "hvc1" | "hev1" => Ok(MFVideoFormat_HEVC),
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

        pub fn release_frame_texture(
            &mut self,
            _frame_id: u64,
            _handle: usize,
        ) -> Result<(), DecoderError> {
            Err(DecoderError::NotConfigured)
        }

        pub fn reset_decode_state(&mut self) -> Result<(), DecoderError> {
            Err(DecoderError::NotConfigured)
        }

        pub fn flush(&mut self) -> Result<(), DecoderError> {
            Err(DecoderError::NotConfigured)
        }

        pub fn close(self) -> Result<(), DecoderError> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_plugin::{
        DecoderBitstreamFormat, DecoderMediaKind, DecoderNativeDeviceContext,
        DecoderNativeDeviceContextKind, DecoderNativeHandleKind, NativeFramePipelineProfile,
    };

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
        assert_eq!(
            capabilities.supports_codec("H264", DecoderMediaKind::Video),
            cfg!(target_os = "windows")
        );
        assert_eq!(
            capabilities.supports_codec("hvc1", DecoderMediaKind::Video),
            cfg!(target_os = "windows")
        );
        assert!(
            capabilities
                .codecs
                .iter()
                .all(|codec| { codec.output_formats == vec![DecoderFrameFormat::Nv12] })
        );
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
        assert_eq!(
            requirements.output_pipeline_profiles,
            vec![NativeFramePipelineProfile::D3D11Texture2D]
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

    #[cfg(target_os = "windows")]
    #[test]
    fn open_session_rejects_missing_device_context() {
        let factory = D3D11DecoderFactory;
        let error = match factory.open_native_session(&DecoderSessionConfig {
            codec: "H264".to_owned(),
            media_kind: DecoderMediaKind::Video,
            prefer_hardware: true,
            ..DecoderSessionConfig::default()
        }) {
            Ok(_) => panic!("missing D3D11 device context must be rejected"),
            Err(error) => error,
        };

        assert_eq!(error, DecoderError::NotConfigured);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn open_session_rejects_cpu_output_before_platform_setup() {
        let factory = D3D11DecoderFactory;
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
            DecoderError::UnsupportedCapability { capability }
                if capability == "cpu-video-frame-output"
        ));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn non_windows_factory_does_not_advertise_or_open_codecs() {
        let factory = D3D11DecoderFactory;
        assert!(factory.capabilities().codecs.is_empty());

        let error = match factory.open_native_session(&DecoderSessionConfig {
            codec: "H264".to_owned(),
            media_kind: DecoderMediaKind::Video,
            ..DecoderSessionConfig::default()
        }) {
            Ok(_) => panic!("non-Windows D3D11 session must be rejected"),
            Err(error) => error,
        };
        assert!(matches!(error, DecoderError::UnsupportedCodec { .. }));
    }

    #[test]
    fn device_context_kind_uses_d3d11_device_contract() {
        let context = DecoderNativeDeviceContext::D3D11Device { device_ptr: 42 };

        assert_eq!(context.kind(), DecoderNativeDeviceContextKind::D3D11Device);
        assert_eq!(context.d3d11_device_ptr(), Some(42));
    }
}
