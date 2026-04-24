use std::ffi::{c_char, c_void};

use player_plugin::{
    DecoderCapabilities, DecoderCodecCapability, DecoderError, DecoderFrameFormat,
    DecoderFrameMetadata, DecoderFramePlane, DecoderMediaKind, DecoderOperationStatus,
    DecoderPacket, DecoderPacketResult, DecoderReceiveFrameMetadata, DecoderSessionConfig,
    DecoderSessionInfo, VESPER_PLUGIN_ABI_VERSION, VesperDecoderOpenSessionResult,
    VesperDecoderPluginApi, VesperDecoderReceiveFrameResult, VesperPluginBytes,
    VesperPluginDescriptor, VesperPluginKind, VesperPluginProcessResult, VesperPluginResultStatus,
};

static PLUGIN_NAME: &[u8] = b"player-decoder-fixture\0";

struct PluginBundle {
    api: VesperDecoderPluginApi,
    descriptor: VesperPluginDescriptor,
}

#[derive(Debug, Default)]
struct FixtureDecoderSession {
    last_pts_us: Option<i64>,
    pending_frame: Option<Vec<u8>>,
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_plugin_entry() -> *const VesperPluginDescriptor {
    let mut bundle = Box::new(PluginBundle {
        api: VesperDecoderPluginApi {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(decoder_name),
            capabilities_json: Some(decoder_capabilities_json),
            free_bytes: Some(free_plugin_bytes),
            open_session_json: Some(decoder_open_session_json),
            send_packet: Some(decoder_send_packet),
            receive_frame: Some(decoder_receive_frame),
            flush_session: Some(decoder_flush_session),
            close_session: Some(decoder_close_session),
        },
        descriptor: VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: PLUGIN_NAME.as_ptr().cast::<c_char>(),
            api: std::ptr::null(),
        },
    });
    bundle.descriptor.api = (&bundle.api as *const VesperDecoderPluginApi).cast::<c_void>();
    let bundle = Box::leak(bundle);
    &bundle.descriptor
}

unsafe extern "C" fn decoder_name(_context: *mut c_void) -> *const c_char {
    PLUGIN_NAME.as_ptr().cast::<c_char>()
}

unsafe extern "C" fn decoder_capabilities_json(_context: *mut c_void) -> VesperPluginBytes {
    let capabilities = DecoderCapabilities {
        codecs: vec![DecoderCodecCapability {
            codec: "fixture-video".to_owned(),
            media_kind: DecoderMediaKind::Video,
            profiles: vec!["baseline".to_owned()],
            output_formats: vec![DecoderFrameFormat::Rgba8888],
        }],
        supports_hardware_decode: false,
        supports_cpu_video_frames: true,
        supports_audio_frames: false,
        supports_gpu_handles: false,
        supports_flush: true,
        supports_drain: true,
        max_sessions: Some(1),
    };
    serialize_payload(&capabilities)
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
    if config.codec != "fixture-video" || config.media_kind != DecoderMediaKind::Video {
        return open_error(DecoderError::UnsupportedCodec {
            codec: config.codec,
        });
    }

    let session = Box::into_raw(Box::new(FixtureDecoderSession::default()));
    let info = DecoderSessionInfo {
        decoder_name: Some("player-decoder-fixture".to_owned()),
        selected_hardware_backend: None,
        output_format: Some(DecoderFrameFormat::Rgba8888),
    };

    VesperDecoderOpenSessionResult {
        status: VesperPluginResultStatus::Success,
        session: session.cast::<c_void>(),
        payload: serialize_payload(&info),
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
    let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
        return process_error(DecoderError::NotConfigured);
    };
    let packet = match decode_json::<DecoderPacket>(packet_json, packet_json_len) {
        Ok(packet) => packet,
        Err(error) => return process_error(error),
    };
    let data = if packet_data.is_null() || packet_data_len == 0 {
        Vec::new()
    } else {
        let slice = unsafe { std::slice::from_raw_parts(packet_data, packet_data_len) };
        slice.to_vec()
    };
    session.last_pts_us = packet.pts_us;
    session.pending_frame = Some(data);
    process_success(&DecoderPacketResult { accepted: true })
}

unsafe extern "C" fn decoder_receive_frame(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperDecoderReceiveFrameResult {
    let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
        return frame_error(DecoderError::NotConfigured);
    };
    let Some(data) = session.pending_frame.take() else {
        return frame_success(&DecoderReceiveFrameMetadata::need_more_input(), Vec::new());
    };
    let metadata = DecoderFrameMetadata {
        media_kind: DecoderMediaKind::Video,
        format: DecoderFrameFormat::Rgba8888,
        pts_us: session.last_pts_us,
        duration_us: Some(33_333),
        width: Some(2),
        height: Some(2),
        sample_rate: None,
        channels: None,
        planes: vec![DecoderFramePlane {
            offset: 0,
            len: data.len(),
            stride: Some(8),
        }],
    };
    frame_success(&DecoderReceiveFrameMetadata::frame(metadata), data)
}

unsafe extern "C" fn decoder_flush_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
        return process_error(DecoderError::NotConfigured);
    };
    session.pending_frame = None;
    process_success(&DecoderOperationStatus { completed: true })
}

unsafe extern "C" fn decoder_close_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    if session.is_null() {
        return process_error(DecoderError::NotConfigured);
    }
    let _ = unsafe { Box::from_raw(session.cast::<FixtureDecoderSession>()) };
    process_success(&DecoderOperationStatus { completed: true })
}

unsafe extern "C" fn free_plugin_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
    let _ = unsafe { payload.into_vec() };
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

fn frame_success(
    metadata: &DecoderReceiveFrameMetadata,
    data: Vec<u8>,
) -> VesperDecoderReceiveFrameResult {
    VesperDecoderReceiveFrameResult {
        status: VesperPluginResultStatus::Success,
        metadata: serialize_payload(metadata),
        data: VesperPluginBytes::from_vec(data),
    }
}

fn frame_error(error: DecoderError) -> VesperDecoderReceiveFrameResult {
    VesperDecoderReceiveFrameResult {
        status: VesperPluginResultStatus::Failure,
        metadata: serialize_payload(&error),
        data: VesperPluginBytes::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::vesper_plugin_entry;
    use player_plugin::{VESPER_PLUGIN_ABI_VERSION, VesperPluginKind};

    #[test]
    fn exported_descriptor_matches_decoder_plugin_metadata() {
        let descriptor = unsafe { vesper_plugin_entry().as_ref() }.expect("descriptor");

        assert_eq!(descriptor.abi_version, VESPER_PLUGIN_ABI_VERSION);
        assert_eq!(descriptor.plugin_kind, VesperPluginKind::Decoder);
        assert!(!descriptor.api.is_null());
        assert!(!descriptor.plugin_name.is_null());
    }
}
