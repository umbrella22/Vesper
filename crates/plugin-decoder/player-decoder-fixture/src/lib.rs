#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

use player_plugin::{
    DecoderBitstreamFormat, DecoderCapabilities, DecoderCodecCapability, DecoderError,
    DecoderFrameFormat, DecoderMediaKind, DecoderNativeFrameMetadata,
    DecoderNativeFrameReleaseTracking, DecoderNativeHandleKind, DecoderNativeRequirements,
    DecoderOperationStatus, DecoderPacket, DecoderPacketResult, DecoderReceiveNativeFrameMetadata,
    DecoderSessionConfig, DecoderSessionInfo, VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
    VesperDecoderOpenSessionResult, VesperDecoderPluginApiV2,
    VesperDecoderReceiveNativeFrameResult, VesperPluginBytes, VesperPluginDescriptor,
    VesperPluginKind, VesperPluginProcessResult, VesperPluginResultStatus,
};

static PLUGIN_NAME: &[u8] = b"player-decoder-fixture\0";
const CONFIGURED_CODECS_ENV: &str = "VESPER_DECODER_FIXTURE_CODECS";
const DEFAULT_VIDEO_CODEC: &str = "fixture-video";

struct NativePluginBundle {
    api: VesperDecoderPluginApiV2,
    descriptor: VesperPluginDescriptor,
}

#[derive(Debug, Default)]
struct FixtureDecoderSession {
    last_pts_us: Option<i64>,
    pending_frame: Option<Vec<u8>>,
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_plugin_entry() -> *const VesperPluginDescriptor {
    catch_unwind(AssertUnwindSafe(vesper_plugin_entry_impl)).unwrap_or(std::ptr::null())
}

fn vesper_plugin_entry_impl() -> *const VesperPluginDescriptor {
    let mut bundle = Box::new(NativePluginBundle {
        api: VesperDecoderPluginApiV2 {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(decoder_name),
            capabilities_json: Some(native_decoder_capabilities_json),
            native_requirements_json: Some(native_decoder_requirements_json),
            free_bytes: Some(free_plugin_bytes),
            open_session_json: Some(native_decoder_open_session_json),
            send_packet: Some(decoder_send_packet),
            receive_native_frame: Some(decoder_receive_native_frame),
            release_native_frame: Some(decoder_release_native_frame),
            flush_session: Some(decoder_flush_session),
            close_session: Some(decoder_close_session),
        },
        descriptor: VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
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
    catch_unwind(AssertUnwindSafe(|| PLUGIN_NAME.as_ptr().cast::<c_char>()))
        .unwrap_or(std::ptr::null())
}

unsafe extern "C" fn native_decoder_capabilities_json(_context: *mut c_void) -> VesperPluginBytes {
    catch_decoder_bytes(|| {
        let mut capabilities = decoder_capabilities();
        capabilities.supports_hardware_decode = true;
        capabilities.supports_cpu_video_frames = false;
        capabilities.supports_gpu_handles = true;
        for codec in &mut capabilities.codecs {
            codec.output_formats = vec![DecoderFrameFormat::Nv12];
        }
        serialize_payload(&capabilities)
    })
}

unsafe extern "C" fn native_decoder_requirements_json(_context: *mut c_void) -> VesperPluginBytes {
    catch_decoder_bytes(|| {
        serialize_payload(&DecoderNativeRequirements {
            required_device_context_kinds: Vec::new(),
            output_handle_kinds: vec![DecoderNativeHandleKind::IoSurface],
            requires_native_device_context: false,
            accepted_bitstream_formats: vec![DecoderBitstreamFormat::Unknown("fixture".to_owned())],
        })
    })
}

unsafe extern "C" fn native_decoder_open_session_json(
    _context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperDecoderOpenSessionResult {
    catch_decoder_open(|| {
        let config = match decode_json::<DecoderSessionConfig>(config_json, config_json_len) {
            Ok(config) => config,
            Err(error) => return open_error(error),
        };
        if !decoder_capabilities().supports_codec(&config.codec, config.media_kind) {
            return open_error(DecoderError::UnsupportedCodec {
                codec: config.codec,
            });
        }

        let session = Box::into_raw(Box::new(FixtureDecoderSession::default()));
        let info = DecoderSessionInfo {
            decoder_name: Some("player-decoder-fixture".to_owned()),
            selected_hardware_backend: Some("fixture-native".to_owned()),
            output_format: Some(DecoderFrameFormat::Nv12),
        };

        VesperDecoderOpenSessionResult {
            status: VesperPluginResultStatus::Success,
            session: session.cast::<c_void>(),
            payload: serialize_payload(&info),
        }
    })
}

unsafe extern "C" fn decoder_send_packet(
    _context: *mut c_void,
    session: *mut c_void,
    packet_json: *const u8,
    packet_json_len: usize,
    packet_data: *const u8,
    packet_data_len: usize,
) -> VesperPluginProcessResult {
    catch_decoder_process(|| {
        // SAFETY: `session` is the opaque pointer returned by this plugin's
        // open callback and remains owned by the host until close.
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
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

        let data = if packet_data.is_null() || packet_data_len == 0 {
            Vec::new()
        } else {
            // SAFETY: the ABI caller provides a valid packet byte slice for the
            // duration of this synchronous call.
            let slice = unsafe { std::slice::from_raw_parts(packet_data, packet_data_len) };
            slice.to_vec()
        };
        session.last_pts_us = packet.pts_us;
        session.pending_frame = Some(data);
        process_success(&DecoderPacketResult { accepted: true })
    })
}

unsafe extern "C" fn decoder_receive_native_frame(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperDecoderReceiveNativeFrameResult {
    catch_decoder_native_frame(|| {
        // SAFETY: `session` is the opaque pointer returned by this plugin's
        // open callback and remains owned by the host until close.
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
            return native_frame_error(DecoderError::NotConfigured);
        };
        let Some(data) = session.pending_frame.take() else {
            return native_frame_success(&DecoderReceiveNativeFrameMetadata::need_more_input(), 0);
        };
        let handle = Box::into_raw(Box::new(data)) as usize;
        let metadata = DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Nv12,
            codec: DEFAULT_VIDEO_CODEC.to_owned(),
            pts_us: session.last_pts_us,
            duration_us: Some(33_333),
            width: 2,
            height: 2,
            coded_width: Some(2),
            coded_height: Some(2),
            visible_rect: None,
            handle_kind: DecoderNativeHandleKind::IoSurface,
            frame_id: Some(handle as u64),
            release_tracking: Some(DecoderNativeFrameReleaseTracking {
                frame_id: Some(handle as u64),
                requires_release: true,
            }),
        };
        native_frame_success(&DecoderReceiveNativeFrameMetadata::frame(metadata), handle)
    })
}

unsafe extern "C" fn decoder_release_native_frame(
    _context: *mut c_void,
    _session: *mut c_void,
    handle_kind: u32,
    handle: usize,
) -> VesperPluginProcessResult {
    catch_decoder_process(|| {
        if handle_kind != 2 || handle == 0 {
            return process_error(DecoderError::abi_violation(
                "fixture native frame release received an invalid handle",
            ));
        }
        // SAFETY: `handle` was returned by this plugin as `Box<Vec<u8>>` from
        // `decoder_receive_native_frame` and is released exactly once.
        let _ = unsafe { Box::from_raw(handle as *mut Vec<u8>) };
        process_success(&DecoderOperationStatus { completed: true })
    })
}

unsafe extern "C" fn decoder_flush_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_decoder_process(|| {
        // SAFETY: `session` is the opaque pointer returned by this plugin's
        // open callback and remains owned by the host until close.
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
            return process_error(DecoderError::NotConfigured);
        };
        session.pending_frame = None;
        process_success(&DecoderOperationStatus { completed: true })
    })
}

unsafe extern "C" fn decoder_close_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_decoder_process(|| {
        if session.is_null() {
            return process_error(DecoderError::NotConfigured);
        }
        // SAFETY: `session` was allocated by `native_decoder_open_session_json`
        // and is consumed exactly once by this close callback.
        let _ = unsafe { Box::from_raw(session.cast::<FixtureDecoderSession>()) };
        process_success(&DecoderOperationStatus { completed: true })
    })
}

unsafe extern "C" fn free_plugin_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: payloads returned by this plugin are allocated from Vec<u8>
        // inside this dynamic library and have not been reclaimed yet.
        let _ = unsafe { payload.into_vec() };
    }));
}

fn decoder_capabilities() -> DecoderCapabilities {
    DecoderCapabilities {
        codecs: configured_video_codecs(),
        supports_hardware_decode: false,
        supports_cpu_video_frames: true,
        supports_audio_frames: false,
        supports_gpu_handles: false,
        supports_flush: true,
        supports_drain: true,
        max_sessions: Some(1),
    }
}

fn configured_video_codecs() -> Vec<DecoderCodecCapability> {
    let configured =
        std::env::var_os(CONFIGURED_CODECS_ENV).map(|value| value.to_string_lossy().into_owned());
    video_codecs_from_configured_list(configured.as_deref())
}

fn video_codecs_from_configured_list(configured: Option<&str>) -> Vec<DecoderCodecCapability> {
    let mut codecs = configured
        .into_iter()
        .flat_map(|value| value.split([',', ';']))
        .map(str::trim)
        .filter(|codec| !codec.is_empty())
        .fold(Vec::<String>::new(), |mut codecs, codec| {
            if !codecs
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(codec))
            {
                codecs.push(codec.to_owned());
            }
            codecs
        });

    if codecs.is_empty() {
        codecs.push(DEFAULT_VIDEO_CODEC.to_owned());
    }

    codecs
        .into_iter()
        .map(|codec| DecoderCodecCapability {
            codec,
            media_kind: DecoderMediaKind::Video,
            profiles: vec!["fixture".to_owned()],
            output_formats: vec![DecoderFrameFormat::Rgba8888],
        })
        .collect()
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
        // SAFETY: the ABI caller provides a valid JSON byte range for the
        // duration of this synchronous callback.
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

fn plugin_panic_error() -> DecoderError {
    DecoderError::internal("decoder plugin callback panicked")
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_VIDEO_CODEC, FixtureDecoderSession, decoder_send_packet, vesper_plugin_entry,
        video_codecs_from_configured_list,
    };
    use player_plugin::{
        DecoderError, DecoderPacket, VESPER_DECODER_PLUGIN_ABI_VERSION_V3, VesperPluginKind,
        VesperPluginResultStatus,
    };
    use std::ffi::c_void;

    #[test]
    fn exported_descriptor_matches_decoder_plugin_metadata() {
        // SAFETY: the fixture entry point returns a process-lifetime descriptor
        // pointer or null; this test immediately borrows it.
        let descriptor = unsafe { vesper_plugin_entry().as_ref() }.expect("descriptor");

        assert_eq!(descriptor.abi_version, VESPER_DECODER_PLUGIN_ABI_VERSION_V3);
        assert_eq!(descriptor.plugin_kind, VesperPluginKind::Decoder);
        assert!(!descriptor.api.is_null());
        assert!(!descriptor.plugin_name.is_null());
    }

    #[test]
    fn configured_codec_list_defaults_to_fixture_video() {
        let codecs = video_codecs_from_configured_list(None);

        assert_eq!(codecs.len(), 1);
        assert_eq!(codecs[0].codec, DEFAULT_VIDEO_CODEC);
    }

    #[test]
    fn configured_codec_list_accepts_comma_or_semicolon_separated_video_codecs() {
        let codecs = video_codecs_from_configured_list(Some("H264, HEVC;h264"));
        let names = codecs
            .into_iter()
            .map(|codec| codec.codec)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["H264", "HEVC"]);
    }

    #[test]
    fn send_packet_rejects_null_packet_data_with_non_zero_len() {
        let packet_json = serde_json::to_vec(&DecoderPacket::default()).expect("packet json");
        let mut session = FixtureDecoderSession::default();

        // SAFETY: all pointers passed to the callback are valid for this
        // synchronous test call.
        let result = unsafe {
            decoder_send_packet(
                std::ptr::null_mut(),
                (&mut session as *mut FixtureDecoderSession).cast::<c_void>(),
                packet_json.as_ptr(),
                packet_json.len(),
                std::ptr::null(),
                1,
            )
        };

        assert_eq!(result.status, VesperPluginResultStatus::Failure);
        // SAFETY: the fixture plugin produced this payload in the current
        // dynamic library and the test has not reclaimed it yet.
        let payload = unsafe { result.payload.into_vec() };
        let error = serde_json::from_slice::<DecoderError>(&payload).expect("decoder error");
        assert!(matches!(error, DecoderError::AbiViolation { .. }));
    }
}
