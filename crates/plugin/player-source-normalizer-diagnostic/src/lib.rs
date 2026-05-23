#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};

use player_plugin::{
    DecoderBitstreamFormat, SourceNormalizerError, SourceNormalizerNormalizeLevel,
    SourceNormalizerOperationStatus, SourceNormalizerPacket, SourceNormalizerPacketCapabilities,
    SourceNormalizerPacketMediaKind, SourceNormalizerPacketSeek,
    SourceNormalizerPacketSessionConfig, SourceNormalizerPacketStreamInfo,
    SourceNormalizerPacketTrackInfo, SourceNormalizerReadPacketMetadata,
    SourceNormalizerRequiredCapabilities, VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2,
    VesperPluginBytes, VesperPluginDescriptor, VesperPluginKind, VesperPluginProcessResult,
    VesperPluginResultStatus, VesperSourceNormalizerOpenPacketSessionResult,
    VesperSourceNormalizerPluginApiV2, VesperSourceNormalizerReadPacketResult,
};

static PLUGIN_NAME: &[u8] = b"player-source-normalizer-diagnostic\0";
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
static DIAGNOSTIC_PACKET_BYTES: &[u8] = b"vesper-diagnostic-source-normalizer-packet";

struct PluginBundle {
    api: VesperSourceNormalizerPluginApiV2,
    descriptor: VesperPluginDescriptor,
}

#[derive(Debug)]
struct DiagnosticPacketSession {
    emitted_packet: bool,
    leased_packet: Option<DiagnosticPacketLease>,
    last_seek_millis: Option<u64>,
    closed: bool,
}

#[derive(Debug)]
struct DiagnosticPacketLease {
    handle: usize,
    data: Vec<u8>,
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_plugin_entry() -> *const VesperPluginDescriptor {
    catch_unwind(AssertUnwindSafe(vesper_plugin_entry_impl)).unwrap_or(std::ptr::null())
}

fn vesper_plugin_entry_impl() -> *const VesperPluginDescriptor {
    let mut bundle = Box::new(PluginBundle {
        api: VesperSourceNormalizerPluginApiV2 {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(normalizer_name),
            packet_capabilities_json: Some(normalizer_packet_capabilities_json),
            open_packet_session_json: Some(normalizer_open_packet_session_json),
            read_packet: Some(normalizer_read_packet),
            release_packet: Some(normalizer_release_packet),
            seek_packet_session_json: Some(normalizer_seek_packet_session_json),
            flush_packet_session: Some(normalizer_flush_packet_session),
            close_packet_session: Some(normalizer_close_packet_session),
            free_bytes: Some(free_plugin_bytes),
        },
        descriptor: VesperPluginDescriptor {
            abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::SourceNormalizer,
            plugin_name: PLUGIN_NAME.as_ptr().cast::<c_char>(),
            api: std::ptr::null(),
        },
    });
    bundle.descriptor.api =
        (&bundle.api as *const VesperSourceNormalizerPluginApiV2).cast::<c_void>();
    let bundle = Box::leak(bundle);
    &bundle.descriptor
}

unsafe extern "C" fn normalizer_name(_context: *mut c_void) -> *const c_char {
    catch_unwind(AssertUnwindSafe(|| PLUGIN_NAME.as_ptr().cast::<c_char>()))
        .unwrap_or(std::ptr::null())
}

unsafe extern "C" fn normalizer_packet_capabilities_json(
    _context: *mut c_void,
) -> VesperPluginBytes {
    catch_normalizer_bytes(|| serialize_payload(&diagnostic_packet_capabilities()))
}

unsafe extern "C" fn normalizer_open_packet_session_json(
    _context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperSourceNormalizerOpenPacketSessionResult {
    catch_normalizer_open(|| {
        let config = match decode_json::<SourceNormalizerPacketSessionConfig>(
            config_json,
            config_json_len,
        ) {
            Ok(config) => config,
            Err(error) => return open_error(error),
        };
        if config.input.is_empty() {
            return open_error(SourceNormalizerError::invalid_input(
                "input must not be empty",
            ));
        }
        let capabilities = diagnostic_packet_capabilities();
        if !capabilities.supports_runtime_profile(&config.runtime_profile) {
            return open_error(SourceNormalizerError::UnsupportedRuntimeProfile {
                profile: config.runtime_profile,
            });
        }
        if config.preferred_media_kind != SourceNormalizerPacketMediaKind::Video {
            return open_error(SourceNormalizerError::unsupported_operation(
                "non-video packet streams",
            ));
        }

        let session_number = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
        let session_id = format!("diagnostic-packet-{session_number}");
        let stream_info = SourceNormalizerPacketStreamInfo {
            session_id: Some(session_id),
            normalizer_name: Some("player-source-normalizer-diagnostic".to_owned()),
            runtime_profile: Some(config.runtime_profile),
            selected_backend: Some("diagnostic-packet".to_owned()),
            tracks: vec![diagnostic_video_track()],
            selected_track_index: Some(0),
            duration_millis: Some(1_000),
            seekable: true,
        };
        let session = Box::into_raw(Box::new(DiagnosticPacketSession {
            emitted_packet: false,
            leased_packet: None,
            last_seek_millis: None,
            closed: false,
        }));
        VesperSourceNormalizerOpenPacketSessionResult {
            status: VesperPluginResultStatus::Success,
            session: session.cast::<c_void>(),
            payload: serialize_payload(&stream_info),
        }
    })
}

unsafe extern "C" fn normalizer_read_packet(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperSourceNormalizerReadPacketResult {
    catch_normalizer_read(|| {
        // SAFETY: `session` is the opaque pointer returned by this plugin's
        // open callback and remains owned by the host until close.
        let Some(session) = (unsafe { session.cast::<DiagnosticPacketSession>().as_mut() }) else {
            return read_packet_error(SourceNormalizerError::NotConfigured);
        };
        if session.closed {
            return read_packet_error(SourceNormalizerError::NotConfigured);
        }
        if session.leased_packet.is_some() {
            return read_packet_error(SourceNormalizerError::abi_violation(
                "previous packet lease has not been released",
            ));
        }
        if session.emitted_packet {
            return VesperSourceNormalizerReadPacketResult {
                status: VesperPluginResultStatus::Success,
                metadata: serialize_payload(&SourceNormalizerReadPacketMetadata::end_of_stream()),
                data: std::ptr::null(),
                data_len: 0,
                packet_handle: 0,
            };
        }

        let handle = 1;
        session.emitted_packet = true;
        let packet = session.leased_packet.insert(DiagnosticPacketLease {
            handle,
            data: DIAGNOSTIC_PACKET_BYTES.to_vec(),
        });
        let metadata = SourceNormalizerReadPacketMetadata::packet(SourceNormalizerPacket {
            pts_us: session
                .last_seek_millis
                .map(|millis| i64::try_from(millis.saturating_mul(1_000)).unwrap_or(i64::MAX))
                .or(Some(0)),
            dts_us: Some(0),
            duration_us: Some(33_333),
            stream_index: 0,
            key_frame: true,
            discontinuity: session.last_seek_millis.is_some(),
            end_of_stream: false,
        });
        VesperSourceNormalizerReadPacketResult {
            status: VesperPluginResultStatus::Success,
            metadata: serialize_payload(&metadata),
            data: packet.data.as_ptr(),
            data_len: packet.data.len(),
            packet_handle: packet.handle,
        }
    })
}

unsafe extern "C" fn normalizer_release_packet(
    _context: *mut c_void,
    session: *mut c_void,
    packet_handle: usize,
) -> VesperPluginProcessResult {
    catch_normalizer_process(|| {
        // SAFETY: `session` is the opaque pointer returned by this plugin's
        // open callback and remains owned by the host until close.
        let Some(session) = (unsafe { session.cast::<DiagnosticPacketSession>().as_mut() }) else {
            return process_error(SourceNormalizerError::NotConfigured);
        };
        if session.closed {
            return process_error(SourceNormalizerError::NotConfigured);
        }
        match session.leased_packet.take() {
            Some(packet) if packet.handle == packet_handle => process_success(),
            Some(packet) => {
                session.leased_packet = Some(packet);
                process_error(SourceNormalizerError::abi_violation(format!(
                    "unknown packet handle {packet_handle}"
                )))
            }
            None => process_error(SourceNormalizerError::abi_violation(
                "no packet lease is outstanding",
            )),
        }
    })
}

unsafe extern "C" fn normalizer_seek_packet_session_json(
    _context: *mut c_void,
    session: *mut c_void,
    seek_json: *const u8,
    seek_json_len: usize,
) -> VesperPluginProcessResult {
    catch_normalizer_process(|| {
        // SAFETY: `session` is the opaque pointer returned by this plugin's
        // open callback and remains owned by the host until close.
        let Some(session) = (unsafe { session.cast::<DiagnosticPacketSession>().as_mut() }) else {
            return process_error(SourceNormalizerError::NotConfigured);
        };
        if session.closed {
            return process_error(SourceNormalizerError::NotConfigured);
        }
        let seek = match decode_json::<SourceNormalizerPacketSeek>(seek_json, seek_json_len) {
            Ok(seek) => seek,
            Err(error) => return process_error(error),
        };
        session.leased_packet = None;
        session.emitted_packet = false;
        session.last_seek_millis = Some(seek.position_millis);
        process_success()
    })
}

unsafe extern "C" fn normalizer_flush_packet_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_normalizer_process(|| {
        // SAFETY: `session` is the opaque pointer returned by this plugin's
        // open callback and remains owned by the host until close.
        let Some(session) = (unsafe { session.cast::<DiagnosticPacketSession>().as_mut() }) else {
            return process_error(SourceNormalizerError::NotConfigured);
        };
        if session.closed {
            return process_error(SourceNormalizerError::NotConfigured);
        }
        session.leased_packet = None;
        session.emitted_packet = false;
        process_success()
    })
}

unsafe extern "C" fn normalizer_close_packet_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_normalizer_process(|| {
        if session.is_null() {
            return process_error(SourceNormalizerError::NotConfigured);
        }
        // SAFETY: the session pointer was allocated with `Box::into_raw` by
        // this plugin and close is called once by the host.
        // SAFETY: the session pointer was allocated with `Box::into_raw` by
        // this plugin and close is called once by the host.
        drop(unsafe { Box::from_raw(session.cast::<DiagnosticPacketSession>()) });
        process_success()
    })
}

fn diagnostic_packet_capabilities() -> SourceNormalizerPacketCapabilities {
    SourceNormalizerPacketCapabilities {
        supported_runtime_profiles: vec![
            "diagnostic-packet".to_owned(),
            "diagnostic-fmp4".to_owned(),
            "diagnostic-hls".to_owned(),
        ],
        max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
        media_kinds: vec![SourceNormalizerPacketMediaKind::Video],
        codecs: vec!["H264".to_owned()],
        bitstream_formats: vec![DecoderBitstreamFormat::Avcc],
        supports_seek: true,
        supports_flush: true,
        required_capabilities: SourceNormalizerRequiredCapabilities::default(),
        max_sessions: Some(1),
    }
}

fn diagnostic_video_track() -> SourceNormalizerPacketTrackInfo {
    SourceNormalizerPacketTrackInfo {
        stream_index: 0,
        media_kind: SourceNormalizerPacketMediaKind::Video,
        codec: "H264".to_owned(),
        extradata: vec![1, 66, 0, 30],
        bitstream_format: Some(DecoderBitstreamFormat::Avcc),
        width: Some(16),
        height: Some(16),
        coded_width: Some(16),
        coded_height: Some(16),
        sample_rate: None,
        channels: None,
        frame_rate: Some(30.0),
        time_base_num: Some(1),
        time_base_den: Some(90_000),
    }
}

fn decode_json<T: serde::de::DeserializeOwned>(
    data: *const u8,
    len: usize,
) -> Result<T, SourceNormalizerError> {
    if data.is_null() && len > 0 {
        return Err(SourceNormalizerError::abi_violation(
            "plugin JSON pointer was null with non-zero len",
        ));
    }
    let payload = if data.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the ABI caller keeps the byte range alive for this synchronous
        // callback.
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    serde_json::from_slice(payload)
        .map_err(|error| SourceNormalizerError::payload_codec(error.to_string()))
}

fn open_error(error: SourceNormalizerError) -> VesperSourceNormalizerOpenPacketSessionResult {
    VesperSourceNormalizerOpenPacketSessionResult {
        status: VesperPluginResultStatus::Failure,
        session: std::ptr::null_mut(),
        payload: serialize_payload(&error),
    }
}

fn read_packet_error(error: SourceNormalizerError) -> VesperSourceNormalizerReadPacketResult {
    VesperSourceNormalizerReadPacketResult {
        status: VesperPluginResultStatus::Failure,
        metadata: serialize_payload(&error),
        data: std::ptr::null(),
        data_len: 0,
        packet_handle: 0,
    }
}

fn process_success() -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: serialize_payload(&SourceNormalizerOperationStatus {
            completed: true,
            message: None,
        }),
    }
}

fn process_error(error: SourceNormalizerError) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Failure,
        payload: serialize_payload(&error),
    }
}

unsafe extern "C" fn free_plugin_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the payload was produced by this dynamic library and has not
        // been reclaimed yet.
        let _ = unsafe { payload.into_vec() };
    }));
}

fn serialize_payload<T: serde::Serialize>(value: &T) -> VesperPluginBytes {
    match serde_json::to_vec(value) {
        Ok(payload) => VesperPluginBytes::from_vec(payload),
        Err(error) => VesperPluginBytes::from_vec(error.to_string().into_bytes()),
    }
}

fn catch_normalizer_bytes(operation: impl FnOnce() -> VesperPluginBytes) -> VesperPluginBytes {
    catch_unwind(AssertUnwindSafe(operation)).unwrap_or_else(|_| {
        serialize_payload(&SourceNormalizerError::abi_violation(
            "source normalizer callback panicked",
        ))
    })
}

fn catch_normalizer_open(
    operation: impl FnOnce() -> VesperSourceNormalizerOpenPacketSessionResult,
) -> VesperSourceNormalizerOpenPacketSessionResult {
    catch_unwind(AssertUnwindSafe(operation))
        .unwrap_or_else(|_| open_error(SourceNormalizerError::internal("callback panicked")))
}

fn catch_normalizer_read(
    operation: impl FnOnce() -> VesperSourceNormalizerReadPacketResult,
) -> VesperSourceNormalizerReadPacketResult {
    catch_unwind(AssertUnwindSafe(operation))
        .unwrap_or_else(|_| read_packet_error(SourceNormalizerError::internal("callback panicked")))
}

fn catch_normalizer_process(
    operation: impl FnOnce() -> VesperPluginProcessResult,
) -> VesperPluginProcessResult {
    catch_unwind(AssertUnwindSafe(operation))
        .unwrap_or_else(|_| process_error(SourceNormalizerError::internal("callback panicked")))
}

#[cfg(test)]
mod tests {
    use super::{
        decode_json, diagnostic_packet_capabilities, free_plugin_bytes,
        normalizer_close_packet_session, normalizer_open_packet_session_json,
        normalizer_packet_capabilities_json, normalizer_read_packet, normalizer_release_packet,
        normalizer_seek_packet_session_json, vesper_plugin_entry,
    };
    use player_plugin::{
        SourceNormalizerError, SourceNormalizerPacketMediaKind, SourceNormalizerPacketSeek,
        SourceNormalizerPacketSessionConfig, SourceNormalizerPacketStreamInfo,
        SourceNormalizerReadPacketMetadata, SourceNormalizerReadPacketStatus,
        VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2, VesperPluginBytes, VesperPluginKind,
        VesperPluginResultStatus,
    };
    use std::ffi::CStr;

    #[test]
    fn exported_descriptor_metadata_is_stable() {
        let descriptor_ptr = vesper_plugin_entry();
        assert!(!descriptor_ptr.is_null());
        // SAFETY: the entry point returns a process-lifetime descriptor pointer.
        let descriptor = unsafe { &*descriptor_ptr };
        assert_eq!(
            descriptor.abi_version,
            VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2
        );
        assert_eq!(descriptor.plugin_kind, VesperPluginKind::SourceNormalizer);
        // SAFETY: plugin_name is a valid NUL-terminated static string.
        let name = unsafe { CStr::from_ptr(descriptor.plugin_name) }
            .to_str()
            .expect("plugin name utf8");
        assert_eq!(name, "player-source-normalizer-diagnostic");
        assert!(!descriptor.api.is_null());
    }

    #[test]
    fn packet_capabilities_json_decodes() {
        let capabilities = diagnostic_packet_capabilities();

        assert!(
            capabilities
                .supported_runtime_profiles
                .contains(&"diagnostic-packet".to_owned())
        );
        assert!(capabilities.supports_codec("h264"));

        // SAFETY: the callback ignores context and returns a plugin-owned payload.
        let payload = unsafe { normalizer_packet_capabilities_json(std::ptr::null_mut()) };
        let decoded: player_plugin::SourceNormalizerPacketCapabilities = take_plugin_bytes(payload);
        assert_eq!(decoded, capabilities);
    }

    #[test]
    fn direct_packet_lifecycle_returns_synthetic_packet() {
        let config = SourceNormalizerPacketSessionConfig {
            runtime_profile: "diagnostic-packet".to_owned(),
            input: "file:///tmp/input.mp4".to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        };
        let config_json = serde_json::to_vec(&config).expect("serialize config");
        // SAFETY: the JSON buffer remains alive for this synchronous callback.
        let open = unsafe {
            normalizer_open_packet_session_json(
                std::ptr::null_mut(),
                config_json.as_ptr(),
                config_json.len(),
            )
        };
        assert_eq!(open.status, VesperPluginResultStatus::Success);
        assert!(!open.session.is_null());
        let info: SourceNormalizerPacketStreamInfo = take_plugin_bytes(open.payload);
        assert_eq!(
            info.normalizer_name.as_deref(),
            Some("player-source-normalizer-diagnostic")
        );

        // SAFETY: the session pointer was returned by this plugin's open call.
        let packet = unsafe { normalizer_read_packet(std::ptr::null_mut(), open.session) };
        assert_eq!(packet.status, VesperPluginResultStatus::Success);
        assert!(!packet.data.is_null());
        assert!(packet.data_len > 0);
        assert_eq!(packet.packet_handle, 1);
        let metadata: SourceNormalizerReadPacketMetadata = take_plugin_bytes(packet.metadata);
        assert_eq!(metadata.status, SourceNormalizerReadPacketStatus::Packet);

        // SAFETY: the handle was returned by the preceding read.
        let release = unsafe {
            normalizer_release_packet(std::ptr::null_mut(), open.session, packet.packet_handle)
        };
        assert_eq!(release.status, VesperPluginResultStatus::Success);
        take_plugin_bytes::<player_plugin::SourceNormalizerOperationStatus>(release.payload);

        // SAFETY: the session pointer remains open after releasing the first packet.
        let eos = unsafe { normalizer_read_packet(std::ptr::null_mut(), open.session) };
        assert_eq!(eos.status, VesperPluginResultStatus::Success);
        assert_eq!(eos.packet_handle, 0);
        let metadata: SourceNormalizerReadPacketMetadata = take_plugin_bytes(eos.metadata);
        assert_eq!(
            metadata.status,
            SourceNormalizerReadPacketStatus::EndOfStream
        );

        // SAFETY: the session pointer was returned by open and is closed once.
        let close = unsafe { normalizer_close_packet_session(std::ptr::null_mut(), open.session) };
        assert_eq!(close.status, VesperPluginResultStatus::Success);
        take_plugin_bytes::<player_plugin::SourceNormalizerOperationStatus>(close.payload);
    }

    #[test]
    fn read_requires_release_before_next_packet() {
        let config = SourceNormalizerPacketSessionConfig {
            runtime_profile: "diagnostic-packet".to_owned(),
            input: "file:///tmp/input.mp4".to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        };
        let config_json = serde_json::to_vec(&config).expect("serialize config");
        // SAFETY: the JSON buffer remains alive for this synchronous callback.
        let open = unsafe {
            normalizer_open_packet_session_json(
                std::ptr::null_mut(),
                config_json.as_ptr(),
                config_json.len(),
            )
        };
        take_plugin_bytes::<SourceNormalizerPacketStreamInfo>(open.payload);

        // SAFETY: the session pointer was returned by this plugin's open call.
        let first = unsafe { normalizer_read_packet(std::ptr::null_mut(), open.session) };
        assert_eq!(first.status, VesperPluginResultStatus::Success);
        take_plugin_bytes::<SourceNormalizerReadPacketMetadata>(first.metadata);

        // SAFETY: the same still-open session is used intentionally without release.
        let second = unsafe { normalizer_read_packet(std::ptr::null_mut(), open.session) };
        assert_eq!(second.status, VesperPluginResultStatus::Failure);
        let error: SourceNormalizerError = take_plugin_bytes(second.metadata);
        assert!(matches!(error, SourceNormalizerError::AbiViolation { .. }));

        // SAFETY: cleanup the outstanding packet and close the session.
        let release = unsafe {
            normalizer_release_packet(std::ptr::null_mut(), open.session, first.packet_handle)
        };
        take_plugin_bytes::<player_plugin::SourceNormalizerOperationStatus>(release.payload);
        // SAFETY: the session pointer was returned by open and is closed once.
        let close = unsafe { normalizer_close_packet_session(std::ptr::null_mut(), open.session) };
        take_plugin_bytes::<player_plugin::SourceNormalizerOperationStatus>(close.payload);
    }

    #[test]
    fn seek_resets_synthetic_packet() {
        let config = SourceNormalizerPacketSessionConfig {
            runtime_profile: "diagnostic-packet".to_owned(),
            input: "file:///tmp/input.mp4".to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        };
        let config_json = serde_json::to_vec(&config).expect("serialize config");
        // SAFETY: the JSON buffer remains alive for this synchronous callback.
        let open = unsafe {
            normalizer_open_packet_session_json(
                std::ptr::null_mut(),
                config_json.as_ptr(),
                config_json.len(),
            )
        };
        take_plugin_bytes::<SourceNormalizerPacketStreamInfo>(open.payload);

        let seek = SourceNormalizerPacketSeek {
            position_millis: 123,
            exact: false,
        };
        let seek_json = serde_json::to_vec(&seek).expect("serialize seek");
        // SAFETY: the session pointer and JSON buffer remain valid for this call.
        let seek = unsafe {
            normalizer_seek_packet_session_json(
                std::ptr::null_mut(),
                open.session,
                seek_json.as_ptr(),
                seek_json.len(),
            )
        };
        assert_eq!(seek.status, VesperPluginResultStatus::Success);
        take_plugin_bytes::<player_plugin::SourceNormalizerOperationStatus>(seek.payload);

        // SAFETY: the session pointer remains open after seek.
        let packet = unsafe { normalizer_read_packet(std::ptr::null_mut(), open.session) };
        let metadata: SourceNormalizerReadPacketMetadata = take_plugin_bytes(packet.metadata);
        assert_eq!(
            metadata.packet.and_then(|packet| packet.pts_us),
            Some(123_000)
        );

        // SAFETY: cleanup the outstanding packet and close the session.
        let release = unsafe {
            normalizer_release_packet(std::ptr::null_mut(), open.session, packet.packet_handle)
        };
        take_plugin_bytes::<player_plugin::SourceNormalizerOperationStatus>(release.payload);
        // SAFETY: the session pointer was returned by open and is closed once.
        let close = unsafe { normalizer_close_packet_session(std::ptr::null_mut(), open.session) };
        take_plugin_bytes::<player_plugin::SourceNormalizerOperationStatus>(close.payload);
    }

    #[test]
    fn open_rejects_empty_input() {
        let config = SourceNormalizerPacketSessionConfig {
            runtime_profile: "diagnostic-packet".to_owned(),
            input: String::new(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        };
        let config_json = serde_json::to_vec(&config).expect("serialize config");
        // SAFETY: the JSON buffer remains alive for this synchronous callback.
        let open = unsafe {
            normalizer_open_packet_session_json(
                std::ptr::null_mut(),
                config_json.as_ptr(),
                config_json.len(),
            )
        };
        assert_eq!(open.status, VesperPluginResultStatus::Failure);
        assert!(open.session.is_null());
        let error: SourceNormalizerError = take_plugin_bytes(open.payload);
        assert!(matches!(error, SourceNormalizerError::InvalidInput { .. }));
    }

    #[test]
    fn decode_json_rejects_null_non_empty_payload() {
        let error = decode_json::<SourceNormalizerPacketSessionConfig>(std::ptr::null(), 3)
            .expect_err("null non-empty payload should fail");

        assert!(matches!(error, SourceNormalizerError::AbiViolation { .. }));
    }

    #[test]
    fn free_bytes_accepts_null_payload() {
        // SAFETY: freeing a null/empty payload is a no-op by the shared bytes
        // contract.
        unsafe { free_plugin_bytes(std::ptr::null_mut(), VesperPluginBytes::null()) };
    }

    fn take_plugin_bytes<T: serde::de::DeserializeOwned>(payload: VesperPluginBytes) -> T {
        // SAFETY: test payloads are allocated by this diagnostic plugin and
        // have not been reclaimed before this helper.
        let bytes = unsafe { payload.into_vec() };
        serde_json::from_slice(&bytes).expect("deserialize payload")
    }
}
