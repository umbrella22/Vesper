use std::collections::VecDeque;
use std::ffi::{c_char, c_void};

use player_plugin::{
    DecoderCapabilities, DecoderCodecCapability, DecoderError, DecoderFrameFormat,
    DecoderMediaKind, DecoderNativeFrameMetadata, DecoderNativeHandleKind, DecoderOperationStatus,
    DecoderPacket, DecoderPacketResult, DecoderReceiveNativeFrameMetadata, DecoderSessionConfig,
    DecoderSessionInfo, VESPER_DECODER_PLUGIN_ABI_VERSION_V2, VesperDecoderOpenSessionResult,
    VesperDecoderPluginApiV2, VesperDecoderReceiveNativeFrameResult, VesperPluginBytes,
    VesperPluginDescriptor, VesperPluginKind, VesperPluginProcessResult, VesperPluginResultStatus,
};

static PLUGIN_NAME: &[u8] = b"player-decoder-d3d11\0";
const HANDLE_KIND_D3D11_TEXTURE_2D: u32 = 6;
const DEFAULT_WIDTH: u32 = 16;
const DEFAULT_HEIGHT: u32 = 16;

struct PluginBundle {
    api: VesperDecoderPluginApiV2,
    descriptor: VesperPluginDescriptor,
}

#[derive(Debug, Clone)]
struct PendingFrame {
    pts_us: Option<i64>,
    duration_us: Option<i64>,
    key_frame: bool,
    data_len: usize,
}

struct D3D11DecoderSession {
    codec: String,
    width: u32,
    height: u32,
    inner: platform::SessionInner,
    pending_frames: VecDeque<PendingFrame>,
    eof_received: bool,
    eof_sent: bool,
    color_seed: u8,
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

        let width = config.width.unwrap_or(DEFAULT_WIDTH).max(1);
        let height = config.height.unwrap_or(DEFAULT_HEIGHT).max(1);
        let inner = platform::SessionInner::open(config.native_device_context.as_ref())?;

        Ok(Self {
            codec: config.codec,
            width,
            height,
            inner,
            pending_frames: VecDeque::new(),
            eof_received: false,
            eof_sent: false,
            color_seed: 0,
        })
    }

    fn send_packet(&mut self, packet: DecoderPacket, data_len: usize) -> DecoderPacketResult {
        if packet.discontinuity {
            self.pending_frames.clear();
            self.eof_received = false;
            self.eof_sent = false;
        }

        if packet.end_of_stream {
            self.eof_received = true;
            return DecoderPacketResult { accepted: true };
        }

        self.pending_frames.push_back(PendingFrame {
            pts_us: packet.pts_us,
            duration_us: packet.duration_us,
            key_frame: packet.key_frame,
            data_len,
        });
        DecoderPacketResult { accepted: true }
    }

    fn receive_native_frame(
        &mut self,
    ) -> Result<(DecoderReceiveNativeFrameMetadata, usize), DecoderError> {
        let Some(frame) = self.pending_frames.pop_front() else {
            if self.eof_received && !self.eof_sent {
                self.eof_sent = true;
                return Ok((DecoderReceiveNativeFrameMetadata::eof(), 0));
            }
            return Ok((DecoderReceiveNativeFrameMetadata::need_more_input(), 0));
        };

        self.color_seed = self.color_seed.wrapping_add(29);
        let handle = self.inner.create_frame_texture(
            self.width,
            self.height,
            self.color_seed,
            frame.data_len,
            frame.key_frame,
        )?;
        let metadata = DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Bgra8888,
            codec: self.codec.clone(),
            pts_us: frame.pts_us,
            duration_us: frame.duration_us.or(Some(33_333)),
            width: self.width,
            height: self.height,
            handle_kind: DecoderNativeHandleKind::D3D11Texture2D,
        };
        Ok((DecoderReceiveNativeFrameMetadata::frame(metadata), handle))
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
        self.pending_frames.clear();
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

    process_success(&session.send_packet(packet, packet_data_len))
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

    use player_plugin::{DecoderError, DecoderNativeDeviceContext, DecoderNativeDeviceContextKind};
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_SUBRESOURCE_DATA,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::core::Interface;

    pub struct SessionInner {
        device: ID3D11Device,
        outstanding_textures: HashMap<usize, ID3D11Texture2D>,
    }

    impl SessionInner {
        pub fn open(context: Option<&DecoderNativeDeviceContext>) -> Result<Self, DecoderError> {
            let Some(context) = context else {
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
            Ok(Self {
                device,
                outstanding_textures: HashMap::new(),
            })
        }

        pub fn create_frame_texture(
            &mut self,
            width: u32,
            height: u32,
            color_seed: u8,
            data_len: usize,
            key_frame: bool,
        ) -> Result<usize, DecoderError> {
            let pixels = bgra_test_pattern_pixels(width, height, color_seed, data_len, key_frame)?;
            let pitch = width
                .checked_mul(4)
                .ok_or_else(|| DecoderError::internal("D3D11 decoder texture pitch overflowed"))?;
            let desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET).0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let initial_data = D3D11_SUBRESOURCE_DATA {
                pSysMem: pixels.as_ptr().cast::<c_void>(),
                SysMemPitch: pitch,
                SysMemSlicePitch: 0,
            };
            let mut texture = None;
            unsafe {
                self.device
                    .CreateTexture2D(&desc, Some(&initial_data), Some(&mut texture))
            }
            .map_err(|error| {
                DecoderError::internal(format!("ID3D11Device::CreateTexture2D failed: {error}"))
            })?;
            let texture = texture.ok_or_else(|| {
                DecoderError::internal("ID3D11Device::CreateTexture2D returned null")
            })?;
            let handle = texture.as_raw() as usize;
            self.outstanding_textures.insert(handle, texture);
            Ok(handle)
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
    }

    fn bgra_test_pattern_pixels(
        width: u32,
        height: u32,
        color_seed: u8,
        data_len: usize,
        key_frame: bool,
    ) -> Result<Vec<u8>, DecoderError> {
        let len = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .map(|len| len as usize)
            .ok_or_else(|| DecoderError::internal("D3D11 decoder texture size overflowed"))?;
        let mut pixels = vec![0; len];
        let key_bias = if key_frame { 64 } else { 0 };
        let packet_bias = (data_len as u8).wrapping_mul(3);
        for y in 0..height {
            for x in 0..width {
                let offset = ((y * width + x) * 4) as usize;
                pixels[offset] = color_seed.wrapping_add((x as u8).wrapping_mul(5));
                pixels[offset + 1] = packet_bias.wrapping_add((y as u8).wrapping_mul(7));
                pixels[offset + 2] = 96u8.wrapping_add(key_bias);
                pixels[offset + 3] = 255;
            }
        }
        Ok(pixels)
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use player_plugin::{DecoderError, DecoderNativeDeviceContext};

    pub struct SessionInner;

    impl SessionInner {
        pub fn open(_context: Option<&DecoderNativeDeviceContext>) -> Result<Self, DecoderError> {
            Err(DecoderError::NotConfigured)
        }

        pub fn create_frame_texture(
            &mut self,
            _width: u32,
            _height: u32,
            _color_seed: u8,
            _data_len: usize,
            _key_frame: bool,
        ) -> Result<usize, DecoderError> {
            Err(DecoderError::NotConfigured)
        }

        pub fn release_frame_texture(&mut self, _handle: usize) -> Result<(), DecoderError> {
            Err(DecoderError::NotConfigured)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HANDLE_KIND_D3D11_TEXTURE_2D, decode_json, decoder_capabilities, vesper_plugin_entry,
    };
    use player_plugin::{
        DecoderError, DecoderMediaKind, DecoderNativeDeviceContext, DecoderNativeDeviceContextKind,
        DecoderReceiveFrameStatus, DecoderReceiveNativeFrameMetadata, DecoderSessionConfig,
        VESPER_DECODER_PLUGIN_ABI_VERSION_V2, VesperPluginKind, VesperPluginResultStatus,
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
