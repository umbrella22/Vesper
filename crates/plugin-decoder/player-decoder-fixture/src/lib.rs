#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

use player_plugin::{
    DecoderBitstreamFormat, DecoderCapabilities, DecoderCodecCapability, DecoderError,
    DecoderFrameFormat, DecoderMediaKind, DecoderNativeFrameMetadata,
    DecoderNativeFrameReleaseTracking, DecoderNativeHandleKind, DecoderNativeRequirements,
    DecoderOperationStatus, DecoderPacket, DecoderPacketResult, DecoderPcmFrameMetadata,
    DecoderPcmSampleLayout, DecoderReceiveNativeFrameMetadata, DecoderReceivePcmFrameMetadata,
    DecoderSessionConfig, DecoderSessionInfo, NativeFramePipelineProfile,
    VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT, VesperDecoderOpenSessionResult,
    VesperDecoderPluginApiV5, VesperDecoderReceiveNativeFrameResult,
    VesperDecoderReceivePcmFrameResult, VesperPluginBytes, VesperPluginDescriptor,
    VesperPluginKind, VesperPluginProcessResult, VesperPluginResultStatus,
};

static PLUGIN_NAME: &[u8] = b"player-decoder-fixture\0";
const CONFIGURED_CODECS_ENV: &str = "VESPER_DECODER_FIXTURE_CODECS";
const DEFAULT_VIDEO_CODEC: &str = "fixture-video";

struct NativePluginBundle {
    api: VesperDecoderPluginApiV5,
    descriptor: VesperPluginDescriptor,
}

#[derive(Debug, Default)]
struct FixtureDecoderSession {
    codec: String,
    media_kind: DecoderMediaKind,
    last_pts_us: Option<i64>,
    pending_frame: Option<Vec<u8>>,
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_plugin_entry() -> *const VesperPluginDescriptor {
    catch_unwind(AssertUnwindSafe(vesper_plugin_entry_impl)).unwrap_or(std::ptr::null())
}

fn vesper_plugin_entry_impl() -> *const VesperPluginDescriptor {
    let mut bundle = Box::new(NativePluginBundle {
        api: VesperDecoderPluginApiV5 {
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
            receive_pcm_frame: Some(decoder_receive_pcm_frame),
            release_native_frame2: None,
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
            if codec.media_kind == DecoderMediaKind::Video {
                codec.output_formats = vec![DecoderFrameFormat::Nv12];
            }
        }
        serialize_payload(&capabilities)
    })
}

unsafe extern "C" fn native_decoder_requirements_json(_context: *mut c_void) -> VesperPluginBytes {
    catch_decoder_bytes(|| {
        serialize_payload(&DecoderNativeRequirements {
            required_device_context_kinds: Vec::new(),
            output_handle_kinds: vec![DecoderNativeHandleKind::IoSurface],
            output_pipeline_profiles: vec![NativeFramePipelineProfile::Unknown(
                "io_surface".to_owned(),
            )],
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

        let output_format = match config.media_kind {
            DecoderMediaKind::Audio => DecoderFrameFormat::F32,
            DecoderMediaKind::Video => DecoderFrameFormat::Nv12,
        };
        let session = Box::into_raw(Box::new(FixtureDecoderSession {
            codec: config.codec,
            media_kind: config.media_kind,
            last_pts_us: None,
            pending_frame: None,
        }));
        let info = DecoderSessionInfo {
            decoder_name: Some("player-decoder-fixture".to_owned()),
            selected_hardware_backend: Some("fixture-native".to_owned()),
            output_format: Some(output_format),
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
        if session.media_kind != DecoderMediaKind::Video {
            return native_frame_error(DecoderError::UnsupportedCapability {
                capability: "video-native-frame-output".to_owned(),
            });
        }
        let Some(data) = session.pending_frame.take() else {
            return native_frame_success(&DecoderReceiveNativeFrameMetadata::need_more_input(), 0);
        };
        let handle = Box::into_raw(Box::new(data)) as usize;
        let metadata = DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Nv12,
            codec: session.codec.clone(),
            pts_us: session.last_pts_us,
            duration_us: Some(33_333),
            width: 2,
            height: 2,
            coded_width: Some(2),
            coded_height: Some(2),
            visible_rect: None,
            handle_kind: DecoderNativeHandleKind::IoSurface,
            pipeline_profile: Some(NativeFramePipelineProfile::Unknown("io_surface".to_owned())),
            color_space: None,
            hdr_metadata: None,
            color: None,
            hdr: None,
            sync_info: None,
            transform: None,
            frame_id: Some(handle as u64),
            release_tracking: Some(DecoderNativeFrameReleaseTracking {
                frame_id: Some(handle as u64),
                requires_release: true,
            }),
        };
        native_frame_success(&DecoderReceiveNativeFrameMetadata::frame(metadata), handle)
    })
}

unsafe extern "C" fn decoder_receive_pcm_frame(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperDecoderReceivePcmFrameResult {
    catch_decoder_pcm_frame(|| {
        // SAFETY: `session` is the opaque pointer returned by this plugin's
        // open callback and remains owned by the host until close.
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
            return pcm_frame_error(DecoderError::NotConfigured);
        };
        if session.media_kind != DecoderMediaKind::Audio {
            return pcm_frame_error(DecoderError::UnsupportedCapability {
                capability: "audio-pcm-output".to_owned(),
            });
        }
        let Some(data) = session.pending_frame.take() else {
            return pcm_frame_success(&DecoderReceivePcmFrameMetadata::need_more_input(), None);
        };
        let mut metadata = DecoderPcmFrameMetadata::audio(
            session.codec.clone(),
            DecoderFrameFormat::F32,
            48_000,
            2,
            DecoderPcmSampleLayout::Interleaved,
            1_024,
        );
        metadata.pts_us = session.last_pts_us;
        metadata.duration_us = Some(21_333);
        metadata.channel_layout = Some("stereo".to_owned());
        pcm_frame_success(&DecoderReceivePcmFrameMetadata::frame(metadata), Some(data))
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
    let mut codecs = configured_video_codecs();
    codecs.push(DecoderCodecCapability {
        codec: "fixture-audio".to_owned(),
        media_kind: DecoderMediaKind::Audio,
        profiles: vec!["fixture".to_owned()],
        output_formats: vec![DecoderFrameFormat::F32],
    });
    DecoderCapabilities {
        codecs,
        supports_hardware_decode: false,
        supports_cpu_video_frames: true,
        supports_audio_frames: true,
        supports_pcm_frames: true,
        supports_gpu_handles: false,
        supports_presentation_release: false,
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

fn pcm_frame_success(
    metadata: &DecoderReceivePcmFrameMetadata,
    data: Option<Vec<u8>>,
) -> VesperDecoderReceivePcmFrameResult {
    VesperDecoderReceivePcmFrameResult {
        status: VesperPluginResultStatus::Success,
        metadata: serialize_payload(metadata),
        data: data
            .map(VesperPluginBytes::from_vec)
            .unwrap_or_else(VesperPluginBytes::null),
    }
}

fn pcm_frame_error(error: DecoderError) -> VesperDecoderReceivePcmFrameResult {
    VesperDecoderReceivePcmFrameResult {
        status: VesperPluginResultStatus::Failure,
        metadata: serialize_payload(&error),
        data: VesperPluginBytes::null(),
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
        DEFAULT_VIDEO_CODEC, FixtureDecoderSession, decoder_capabilities,
        decoder_receive_pcm_frame, decoder_send_packet, native_decoder_open_session_json,
        vesper_plugin_entry, video_codecs_from_configured_list,
    };
    use player_plugin::{
        DecoderError, DecoderFrameFormat, DecoderMediaKind, DecoderPacket,
        DecoderReceiveFrameStatus, DecoderReceivePcmFrameMetadata, DecoderSessionConfig,
        VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT, VesperPluginKind, VesperPluginResultStatus,
    };
    use std::ffi::c_void;

    #[test]
    fn exported_descriptor_matches_decoder_plugin_metadata() {
        // SAFETY: the fixture entry point returns a process-lifetime descriptor
        // pointer or null; this test immediately borrows it.
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
    fn decoder_capabilities_advertise_audio_pcm_contract() {
        let capabilities = decoder_capabilities();

        assert!(capabilities.supports_audio_frames);
        assert!(capabilities.supports_codec("fixture-audio", DecoderMediaKind::Audio));
        assert!(capabilities.codecs.iter().any(|codec| {
            codec.codec == "fixture-audio"
                && codec.media_kind == DecoderMediaKind::Audio
                && codec.output_formats == vec![DecoderFrameFormat::F32]
        }));
        assert!(capabilities.codecs.iter().any(|codec| {
            codec.codec == DEFAULT_VIDEO_CODEC
                && codec.media_kind == DecoderMediaKind::Video
                && codec.output_formats == vec![DecoderFrameFormat::Rgba8888]
        }));
    }

    #[test]
    fn open_session_reports_pcm_output_for_audio_config() {
        let config = DecoderSessionConfig {
            codec: "fixture-audio".to_owned(),
            media_kind: DecoderMediaKind::Audio,
            ..DecoderSessionConfig::default()
        };
        let config_json = serde_json::to_vec(&config).expect("config json");

        // SAFETY: all pointers passed to the callback are valid for this
        // synchronous test call.
        let result = unsafe {
            native_decoder_open_session_json(
                std::ptr::null_mut(),
                config_json.as_ptr(),
                config_json.len(),
            )
        };

        assert_eq!(result.status, VesperPluginResultStatus::Success);
        assert!(!result.session.is_null());
        // SAFETY: the payload was produced by this fixture and is consumed once.
        let payload = unsafe { result.payload.into_vec() };
        let info =
            serde_json::from_slice::<player_plugin::DecoderSessionInfo>(&payload).expect("info");
        assert_eq!(info.output_format, Some(DecoderFrameFormat::F32));
        // SAFETY: the session pointer was allocated by the open callback above.
        let _ = unsafe { Box::from_raw(result.session.cast::<FixtureDecoderSession>()) };
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

    #[test]
    fn receive_pcm_frame_round_trips_pending_audio_packet() {
        let mut session = FixtureDecoderSession {
            codec: "fixture-audio".to_owned(),
            media_kind: DecoderMediaKind::Audio,
            last_pts_us: Some(7_000),
            pending_frame: Some(vec![1, 2, 3, 4]),
        };

        // SAFETY: the session pointer is valid for this synchronous test call.
        let result = unsafe {
            decoder_receive_pcm_frame(
                std::ptr::null_mut(),
                (&mut session as *mut FixtureDecoderSession).cast::<c_void>(),
            )
        };

        assert_eq!(result.status, VesperPluginResultStatus::Success);
        // SAFETY: fixture payload ownership is transferred to this test.
        let metadata_payload = unsafe { result.metadata.into_vec() };
        let metadata = serde_json::from_slice::<DecoderReceivePcmFrameMetadata>(&metadata_payload)
            .expect("pcm metadata");
        assert_eq!(metadata.status, DecoderReceiveFrameStatus::Frame);
        let frame = metadata.frame.expect("pcm frame metadata");
        assert_eq!(frame.media_kind, DecoderMediaKind::Audio);
        assert_eq!(frame.codec, "fixture-audio");
        assert_eq!(frame.format, DecoderFrameFormat::F32);
        assert_eq!(frame.pts_us, Some(7_000));
        assert_eq!(frame.duration_us, Some(21_333));
        assert_eq!(frame.sample_rate, 48_000);
        assert_eq!(frame.channels, 2);
        assert_eq!(frame.channel_layout.as_deref(), Some("stereo"));
        // SAFETY: fixture payload ownership is transferred to this test.
        let data = unsafe { result.data.into_vec() };
        assert_eq!(data, vec![1, 2, 3, 4]);

        // SAFETY: the same session remains valid for this synchronous test call.
        let result = unsafe {
            decoder_receive_pcm_frame(
                std::ptr::null_mut(),
                (&mut session as *mut FixtureDecoderSession).cast::<c_void>(),
            )
        };
        assert_eq!(result.status, VesperPluginResultStatus::Success);
        // SAFETY: fixture payload ownership is transferred to this test.
        let metadata_payload = unsafe { result.metadata.into_vec() };
        let metadata = serde_json::from_slice::<DecoderReceivePcmFrameMetadata>(&metadata_payload)
            .expect("need more metadata");
        assert_eq!(metadata.status, DecoderReceiveFrameStatus::NeedMoreInput);
    }
}
