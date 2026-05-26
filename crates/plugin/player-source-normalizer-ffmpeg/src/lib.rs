#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use ffmpeg_next::{self as ffmpeg, codec, encoder, format, media};
use player_plugin::{
    DecoderBitstreamFormat, SourceNormalizerError, SourceNormalizerNormalizeLevel,
    SourceNormalizerOperationStatus, SourceNormalizerOutputRoute,
    SourceNormalizerPacketCapabilities, SourceNormalizerPacketMediaKind,
    SourceNormalizerPacketSeek, SourceNormalizerPacketSessionConfig,
    SourceNormalizerPacketStreamInfo, SourceNormalizerPacketTrackInfo,
    SourceNormalizerReadPacketMetadata, SourceNormalizerRequiredCapabilities,
    SourceNormalizerResourceCachePolicy, SourceNormalizerResourceCapabilities,
    SourceNormalizerResourceInfo, SourceNormalizerResourceSessionConfig,
    SourceNormalizerResourceSessionInfo, SourceNormalizerResourceSessionState,
    SourceNormalizerResourceSessionStatus, VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V3,
    VesperPluginBytes, VesperPluginDescriptor, VesperPluginKind, VesperPluginProcessResult,
    VesperPluginResultStatus, VesperSourceNormalizerOpenPacketSessionResult,
    VesperSourceNormalizerOpenResourceSessionResult, VesperSourceNormalizerPluginApiV3,
    VesperSourceNormalizerReadPacketResult,
};
use player_source_normalizer::{
    SourceNormalizerOutputContainer, SourceNormalizerProfile, SourceNormalizerProfileSet,
    SourceNormalizerSessionConfig, SourceRuntimeDetector, build_ffmpeg_command_plan,
};

static PLUGIN_NAME: &[u8] = b"player-source-normalizer-ffmpeg\0";
const DEFAULT_PROFILE_TOML: &str =
    include_str!("../../../../scripts/source-normalizer-profiles.toml");
const PROFILE_PATH_ENV: &str = "VESPER_SOURCE_NORMALIZER_PROFILE_PATH";
static NEXT_SESSION_SUFFIX: AtomicU64 = AtomicU64::new(1);

struct ResourceWorkerConfig {
    profile_name: String,
    profile: SourceNormalizerProfile,
    input: String,
    output_dir: PathBuf,
    output_path: PathBuf,
    route: SourceNormalizerOutputRoute,
    cache_policy: SourceNormalizerResourceCachePolicy,
    cancel_requested: Arc<AtomicBool>,
    state: Arc<Mutex<ResourceWorkerState>>,
}

struct PluginBundle {
    api: VesperSourceNormalizerPluginApiV3,
    descriptor: VesperPluginDescriptor,
}

struct PacketNormalizerSession {
    input: ffmpeg::format::context::Input,
    selected_stream_index: usize,
    time_base: ffmpeg::Rational,
    next_packet_handle: usize,
    leased_packet: Option<LeasedPacket>,
    closed: bool,
}

#[derive(Debug)]
struct ResourceNormalizerSession {
    info: SourceNormalizerResourceSessionInfo,
    output_dir: PathBuf,
    state: Arc<Mutex<ResourceWorkerState>>,
    cancel_requested: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    closed: bool,
}

#[derive(Debug, Clone)]
struct ResourceWorkerState {
    state: SourceNormalizerResourceSessionState,
    message: Option<String>,
}

impl Drop for PacketNormalizerSession {
    fn drop(&mut self) {
        self.leased_packet = None;
        self.closed = true;
    }
}

#[derive(Debug)]
struct LeasedPacket {
    handle: usize,
    data: Vec<u8>,
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_plugin_entry() -> *const VesperPluginDescriptor {
    std::panic::catch_unwind(vesper_plugin_entry_impl).unwrap_or(std::ptr::null())
}

fn vesper_plugin_entry_impl() -> *const VesperPluginDescriptor {
    let mut bundle = Box::new(PluginBundle {
        api: VesperSourceNormalizerPluginApiV3 {
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
            resource_capabilities_json: Some(normalizer_resource_capabilities_json),
            open_resource_session_json: Some(normalizer_open_resource_session_json),
            poll_resource_session: Some(normalizer_poll_resource_session),
            cancel_resource_session: Some(normalizer_cancel_resource_session),
            close_resource_session: Some(normalizer_close_resource_session),
            free_bytes: Some(free_plugin_bytes),
        },
        descriptor: VesperPluginDescriptor {
            abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::SourceNormalizer,
            plugin_name: PLUGIN_NAME.as_ptr().cast::<c_char>(),
            api: std::ptr::null(),
        },
    });
    bundle.descriptor.api =
        (&bundle.api as *const VesperSourceNormalizerPluginApiV3).cast::<c_void>();
    let bundle = Box::leak(bundle);
    &bundle.descriptor
}

unsafe extern "C" fn normalizer_name(_context: *mut c_void) -> *const c_char {
    PLUGIN_NAME.as_ptr().cast::<c_char>()
}

unsafe extern "C" fn normalizer_packet_capabilities_json(
    _context: *mut c_void,
) -> VesperPluginBytes {
    catch_bytes(|| serialize_payload(&packet_capabilities_from_profiles(load_profile_set())))
}

unsafe extern "C" fn normalizer_resource_capabilities_json(
    _context: *mut c_void,
) -> VesperPluginBytes {
    catch_bytes(|| serialize_payload(&resource_capabilities_from_profiles(load_profile_set())))
}

unsafe extern "C" fn normalizer_open_packet_session_json(
    _context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperSourceNormalizerOpenPacketSessionResult {
    catch_packet_open(|| {
        let config = match decode_json::<SourceNormalizerPacketSessionConfig>(
            config_json,
            config_json_len,
        ) {
            Ok(config) => config,
            Err(error) => return packet_open_error(error),
        };
        if config.input.is_empty() {
            return packet_open_error(SourceNormalizerError::invalid_input(
                "input must not be empty",
            ));
        }

        let profile_set = match load_profile_set() {
            Ok(profile_set) => profile_set,
            Err(error) => return packet_open_error(error),
        };
        let profile_name = if config.runtime_profile.is_empty() {
            detect_profile_name(&profile_set, &config.input)
        } else {
            config.runtime_profile.clone()
        };
        let profile = match profile_set.require(&profile_name) {
            Ok(profile) => profile,
            Err(error) => return packet_open_error(map_core_error(error)),
        };
        if let Err(error) = validate_packet_profile(&profile_name, profile) {
            return packet_open_error(error);
        }

        let input = match open_ffmpeg_input(&config.input) {
            Ok(input) => input,
            Err(error) => return packet_open_error(error),
        };
        let Some(stream) = input.streams().best(ffmpeg::media::Type::Video) else {
            return packet_open_error(SourceNormalizerError::invalid_input(
                "input does not contain a video stream",
            ));
        };
        let stream_index = stream.index();
        let time_base = stream.time_base();
        let track = match packet_track_info(&stream) {
            Ok(track) => track,
            Err(error) => return packet_open_error(error),
        };
        let duration_millis = duration_millis_from_av_duration(input.duration());
        let seekable = input.duration() > 0;
        let stream_info = SourceNormalizerPacketStreamInfo {
            session_id: Some(format!("ffmpeg-packet-{}", unique_session_suffix())),
            normalizer_name: Some("player-source-normalizer-ffmpeg".to_owned()),
            runtime_profile: Some(profile_name),
            selected_backend: Some("ffmpeg-next".to_owned()),
            tracks: vec![track],
            selected_track_index: Some(u32::try_from(stream_index).unwrap_or(u32::MAX)),
            duration_millis,
            seekable,
        };
        let session = Box::into_raw(Box::new(PacketNormalizerSession {
            input,
            selected_stream_index: stream_index,
            time_base,
            next_packet_handle: 1,
            leased_packet: None,
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
    catch_read_packet(|| {
        let Some(session) = (unsafe { session.cast::<PacketNormalizerSession>().as_mut() }) else {
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

        loop {
            match session.input.packets().next() {
                Some((stream, packet)) if stream.index() == session.selected_stream_index => {
                    let data = packet.data().map(<[u8]>::to_vec).unwrap_or_default();
                    let handle = session.next_packet_handle;
                    session.next_packet_handle =
                        session.next_packet_handle.saturating_add(1).max(1);
                    let metadata = SourceNormalizerReadPacketMetadata::packet(
                        player_plugin::SourceNormalizerPacket {
                            pts_us: packet.pts().and_then(|timestamp| {
                                timestamp_to_micros(timestamp, session.time_base)
                            }),
                            dts_us: packet.dts().and_then(|timestamp| {
                                timestamp_to_micros(timestamp, session.time_base)
                            }),
                            duration_us: timestamp_to_micros(packet.duration(), session.time_base)
                                .filter(|duration| *duration > 0),
                            stream_index: u32::try_from(session.selected_stream_index)
                                .unwrap_or(u32::MAX),
                            key_frame: packet.is_key(),
                            discontinuity: false,
                            end_of_stream: false,
                        },
                    );
                    let leased = session.leased_packet.insert(LeasedPacket { handle, data });
                    return VesperSourceNormalizerReadPacketResult {
                        status: VesperPluginResultStatus::Success,
                        metadata: serialize_payload(&metadata),
                        data: leased.data.as_ptr(),
                        data_len: leased.data.len(),
                        packet_handle: leased.handle,
                    };
                }
                Some((_stream, _packet)) => {}
                None => {
                    return VesperSourceNormalizerReadPacketResult {
                        status: VesperPluginResultStatus::Success,
                        metadata: serialize_payload(
                            &SourceNormalizerReadPacketMetadata::end_of_stream(),
                        ),
                        data: std::ptr::null(),
                        data_len: 0,
                        packet_handle: 0,
                    };
                }
            }
        }
    })
}

unsafe extern "C" fn normalizer_release_packet(
    _context: *mut c_void,
    session: *mut c_void,
    packet_handle: usize,
) -> VesperPluginProcessResult {
    catch_process(|| {
        let Some(session) = (unsafe { session.cast::<PacketNormalizerSession>().as_mut() }) else {
            return process_error(SourceNormalizerError::NotConfigured);
        };
        if session.closed {
            return process_error(SourceNormalizerError::NotConfigured);
        }
        match session.leased_packet.take() {
            Some(packet) if packet.handle == packet_handle => {
                process_success(&SourceNormalizerOperationStatus {
                    completed: true,
                    message: None,
                })
            }
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
    catch_process(|| {
        let Some(session) = (unsafe { session.cast::<PacketNormalizerSession>().as_mut() }) else {
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
        let timestamp = seek
            .position_millis
            .saturating_mul(1_000)
            .min(i64::MAX as u64) as i64;
        match session.input.seek(timestamp, ..timestamp) {
            Ok(()) => process_success(&SourceNormalizerOperationStatus {
                completed: true,
                message: Some(format!("seeked to {} ms", seek.position_millis)),
            }),
            Err(error) => process_error(SourceNormalizerError::internal(format!(
                "failed to seek packet input: {error}"
            ))),
        }
    })
}

unsafe extern "C" fn normalizer_flush_packet_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_process(|| {
        let Some(session) = (unsafe { session.cast::<PacketNormalizerSession>().as_mut() }) else {
            return process_error(SourceNormalizerError::NotConfigured);
        };
        if session.closed {
            return process_error(SourceNormalizerError::NotConfigured);
        }
        session.leased_packet = None;
        process_success(&SourceNormalizerOperationStatus {
            completed: true,
            message: None,
        })
    })
}

unsafe extern "C" fn normalizer_close_packet_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_process(|| {
        if session.is_null() {
            return process_error(SourceNormalizerError::NotConfigured);
        }
        // SAFETY: the session pointer was allocated with `Box::into_raw` by
        // this plugin and close is called once by the host.
        drop(unsafe { Box::from_raw(session.cast::<PacketNormalizerSession>()) });
        process_success(&SourceNormalizerOperationStatus {
            completed: true,
            message: None,
        })
    })
}

unsafe extern "C" fn normalizer_open_resource_session_json(
    _context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperSourceNormalizerOpenResourceSessionResult {
    catch_resource_open(|| {
        let config = match decode_json::<SourceNormalizerResourceSessionConfig>(
            config_json,
            config_json_len,
        ) {
            Ok(config) => config,
            Err(error) => return resource_open_error(error),
        };
        if config.input.is_empty() {
            return resource_open_error(SourceNormalizerError::invalid_input(
                "input must not be empty",
            ));
        }
        if config.output_root.is_empty() {
            return resource_open_error(SourceNormalizerError::configuration(
                "output_root must not be empty",
            ));
        }

        let profile_set = match load_profile_set() {
            Ok(profile_set) => profile_set,
            Err(error) => return resource_open_error(error),
        };
        let profile_name = if config.runtime_profile.is_empty() {
            detect_profile_name(&profile_set, &config.input)
        } else {
            config.runtime_profile.clone()
        };
        let profile = match profile_set.require(&profile_name) {
            Ok(profile) => profile,
            Err(error) => return resource_open_error(map_core_error(error)),
        };
        let route = match resource_route_for_profile(profile, config.preferred_route) {
            Ok(route) => route,
            Err(error) => return resource_open_error(error),
        };
        let session_id = format!("ffmpeg-resource-{}", unique_session_suffix());
        let output_dir = Path::new(&config.output_root).join(&session_id);
        if let Err(error) = std::fs::create_dir_all(&output_dir) {
            return resource_open_error(SourceNormalizerError::internal(format!(
                "failed to create resource output directory: {error}"
            )));
        }
        let output_path = output_path_for_route(&output_dir, route);
        let command_plan = match build_ffmpeg_command_plan(
            profile,
            &SourceNormalizerSessionConfig {
                runtime_profile: profile_name.clone(),
                input: config.input.clone(),
                output: output_path.clone(),
                ffmpeg_program: "ffmpeg".to_owned(),
                output_to_stdout: false,
            },
        ) {
            Ok(plan) => plan,
            Err(error) => return resource_open_error(map_core_error(error)),
        };
        let container = match route {
            SourceNormalizerOutputRoute::Fmp4LocalStream => "fmp4",
            SourceNormalizerOutputRoute::HlsShortWindow => "hls",
            SourceNormalizerOutputRoute::PacketStream => "packet",
        }
        .to_owned();
        let content_type = content_type_for_route(route).to_owned();
        let resources = resource_infos_for_route(&output_dir, &output_path, route, true);
        let info = SourceNormalizerResourceSessionInfo {
            session_id: Some(session_id.clone()),
            normalizer_name: Some("player-source-normalizer-ffmpeg".to_owned()),
            runtime_profile: Some(profile_name.clone()),
            selected_backend: Some("ffmpeg-next-resource-worker".to_owned()),
            output_route: route,
            container,
            primary_resource_path: Some(output_path.display().to_string()),
            primary_content_type: Some(content_type.clone()),
            resources,
            tracks: Vec::new(),
            duration_millis: None,
            seekable: profile.seekable,
            disk_bytes_used: Some(0),
        };
        let state = Arc::new(Mutex::new(ResourceWorkerState {
            state: SourceNormalizerResourceSessionState::Starting,
            message: Some(format!(
                "resource session starting; argv={}",
                command_plan.argv().join(" ")
            )),
        }));
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let worker_state = state.clone();
        let worker_cancel = cancel_requested.clone();
        let worker_profile = profile.clone();
        let worker_profile_name = profile_name.clone();
        let worker_input = config.input.clone();
        let worker_output_dir = output_dir.clone();
        let worker_output_path = output_path.clone();
        let worker_route = route;
        let worker_cache_policy = config.cache_policy.clone();
        let worker = thread::spawn(move || {
            run_resource_worker(ResourceWorkerConfig {
                profile_name: worker_profile_name,
                profile: worker_profile,
                input: worker_input,
                output_dir: worker_output_dir,
                output_path: worker_output_path,
                route: worker_route,
                cache_policy: worker_cache_policy,
                cancel_requested: worker_cancel,
                state: worker_state,
            });
        });
        let session = Box::into_raw(Box::new(ResourceNormalizerSession {
            info: info.clone(),
            output_dir,
            state,
            cancel_requested,
            worker: Some(worker),
            closed: false,
        }));

        VesperSourceNormalizerOpenResourceSessionResult {
            status: VesperPluginResultStatus::Success,
            session: session.cast::<c_void>(),
            payload: serialize_payload(&info),
        }
    })
}

unsafe extern "C" fn normalizer_poll_resource_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_process(|| {
        let Some(session) = (unsafe { session.cast::<ResourceNormalizerSession>().as_mut() })
        else {
            return process_error(SourceNormalizerError::NotConfigured);
        };
        if session.closed {
            return process_error(SourceNormalizerError::NotConfigured);
        }
        let worker_state = resource_worker_state(&session.state);
        let mut info = session.info.clone();
        info.resources = resource_infos_for_route(
            &session.output_dir,
            Path::new(info.primary_resource_path.as_deref().unwrap_or_default()),
            info.output_route,
            matches!(
                worker_state.state,
                SourceNormalizerResourceSessionState::Starting
                    | SourceNormalizerResourceSessionState::Running
                    | SourceNormalizerResourceSessionState::Ready
            ),
        );
        info.disk_bytes_used = disk_usage_bytes(&session.output_dir);
        process_success(&SourceNormalizerResourceSessionStatus {
            state: worker_state.state,
            info: Some(info),
            message: worker_state.message,
            disk_bytes_used: disk_usage_bytes(&session.output_dir),
        })
    })
}

unsafe extern "C" fn normalizer_cancel_resource_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_process(|| {
        let Some(session) = (unsafe { session.cast::<ResourceNormalizerSession>().as_mut() })
        else {
            return process_error(SourceNormalizerError::NotConfigured);
        };
        if session.closed {
            return process_error(SourceNormalizerError::NotConfigured);
        }
        session.cancel_requested.store(true, Ordering::SeqCst);
        set_resource_worker_state(
            &session.state,
            SourceNormalizerResourceSessionState::Cancelled,
            Some("resource session cancellation requested".to_owned()),
        );
        process_success(&SourceNormalizerOperationStatus {
            completed: true,
            message: Some("resource session cancelled".to_owned()),
        })
    })
}

unsafe extern "C" fn normalizer_close_resource_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    catch_process(|| {
        if session.is_null() {
            return process_error(SourceNormalizerError::NotConfigured);
        }
        // SAFETY: the session pointer was allocated with `Box::into_raw` by
        // this plugin and close is called once by the host.
        let mut session = unsafe { Box::from_raw(session.cast::<ResourceNormalizerSession>()) };
        session.closed = true;
        session.cancel_requested.store(true, Ordering::SeqCst);
        let join_message = session
            .worker
            .take()
            .and_then(|worker| match worker.join() {
                Ok(()) => None,
                Err(_) => Some("resource worker panicked".to_owned()),
            });
        let cleanup_message = match std::fs::remove_dir_all(&session.output_dir) {
            Ok(()) => None,
            Err(error) if !session.output_dir.exists() => {
                let _ = error;
                None
            }
            Err(error) => Some(format!("cleanup failed: {error}")),
        };
        let message = [join_message, cleanup_message]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("; ");
        process_success(&SourceNormalizerOperationStatus {
            completed: true,
            message: if message.is_empty() {
                None
            } else {
                Some(message)
            },
        })
    })
}

fn load_profile_set() -> Result<SourceNormalizerProfileSet, SourceNormalizerError> {
    if let Ok(path) = std::env::var(PROFILE_PATH_ENV) {
        return SourceNormalizerProfileSet::from_path(path).map_err(map_core_error);
    }
    SourceNormalizerProfileSet::from_toml_str(DEFAULT_PROFILE_TOML).map_err(map_core_error)
}

fn packet_capabilities_from_profiles(
    profile_set: Result<SourceNormalizerProfileSet, SourceNormalizerError>,
) -> SourceNormalizerPacketCapabilities {
    let Ok(profile_set) = profile_set else {
        return SourceNormalizerPacketCapabilities::default();
    };
    let mut profiles = Vec::new();
    let mut required = SourceNormalizerRequiredCapabilities::default();
    for (name, profile) in profile_set.profiles_by_priority() {
        profiles.push(name.to_owned());
        merge_required_capabilities(&mut required, &profile.required_capabilities);
    }
    SourceNormalizerPacketCapabilities {
        supported_runtime_profiles: profiles,
        max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
        media_kinds: vec![SourceNormalizerPacketMediaKind::Video],
        codecs: vec!["H264".to_owned(), "HEVC".to_owned(), "AV1".to_owned()],
        bitstream_formats: vec![
            DecoderBitstreamFormat::Avcc,
            DecoderBitstreamFormat::Hvcc,
            DecoderBitstreamFormat::AnnexB,
        ],
        supports_seek: true,
        supports_flush: true,
        required_capabilities: required,
        max_sessions: None,
    }
}

fn resource_capabilities_from_profiles(
    profile_set: Result<SourceNormalizerProfileSet, SourceNormalizerError>,
) -> SourceNormalizerResourceCapabilities {
    let Ok(profile_set) = profile_set else {
        return SourceNormalizerResourceCapabilities::default();
    };
    let mut profiles = Vec::new();
    let mut routes = Vec::new();
    let mut content_types = Vec::new();
    let mut required = SourceNormalizerRequiredCapabilities::default();
    let mut cache_policy = SourceNormalizerResourceCachePolicy::default();
    for (name, profile) in profile_set.profiles_by_priority() {
        profiles.push(name.to_owned());
        if let Ok(route) = resource_route_for_profile(profile, None) {
            if !routes.contains(&route) {
                routes.push(route);
            }
            let content_type = content_type_for_route(route).to_owned();
            if !content_types.contains(&content_type) {
                content_types.push(content_type);
            }
        }
        merge_required_capabilities(&mut required, &profile.required_capabilities);
        cache_policy.session_read_buffer_bytes = cache_policy
            .session_read_buffer_bytes
            .min(profile.runtime.session_read_buffer_bytes);
        cache_policy.manifest_snapshot_bytes = cache_policy
            .manifest_snapshot_bytes
            .min(profile.runtime.manifest_snapshot_bytes);
        cache_policy.session_disk_soft_cap_bytes = cache_policy
            .session_disk_soft_cap_bytes
            .min(profile.runtime.session_disk_soft_cap_bytes);
        cache_policy.global_disk_soft_cap_bytes = cache_policy
            .global_disk_soft_cap_bytes
            .min(profile.runtime.global_disk_soft_cap_bytes);
    }
    SourceNormalizerResourceCapabilities {
        supported_runtime_profiles: profiles,
        supported_output_routes: routes,
        max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
        content_types,
        supports_growing_resources: true,
        supports_range_reads: true,
        supports_cancel: true,
        required_capabilities: required,
        cache_policy,
        max_sessions: None,
    }
}

fn merge_required_capabilities(
    target: &mut SourceNormalizerRequiredCapabilities,
    source: &player_source_normalizer::SourceNormalizerRequiredCapabilities,
) {
    extend_unique(&mut target.libraries, &source.libraries);
    extend_unique(&mut target.demuxers, &source.demuxers);
    extend_unique(&mut target.muxers, &source.muxers);
    extend_unique(&mut target.protocols, &source.protocols);
    extend_unique(&mut target.parsers, &source.parsers);
    extend_unique(&mut target.bitstream_filters, &source.bsfs);
    target.network |= source.network;
    if target.tls.is_none() {
        target.tls = source.tls.clone();
    }
}

fn extend_unique(target: &mut Vec<String>, source: &[String]) {
    for value in source {
        if !target.iter().any(|candidate| candidate == value) {
            target.push(value.clone());
        }
    }
}

fn detect_profile_name(profile_set: &SourceNormalizerProfileSet, input: &str) -> String {
    let detector = SourceRuntimeDetector::new(profile_set.clone());
    let context = player_source_normalizer::ProbeContext {
        url: input.to_owned(),
        mime: mime_hint_for_input(input).map(str::to_owned),
        headers: Vec::new(),
        timeout_ms: 1_000,
    };
    detector
        .probe_candidates(&context, None)
        .into_iter()
        .next()
        .map(|candidate| candidate.runtime_profile)
        .unwrap_or_else(|| "generic-fallback".to_owned())
}

fn validate_packet_profile(
    profile_name: &str,
    profile: &SourceNormalizerProfile,
) -> Result<(), SourceNormalizerError> {
    if profile.output_container == SourceNormalizerOutputContainer::Fmp4 {
        return Ok(());
    }

    Err(SourceNormalizerError::invalid_input(format!(
        "runtime profile `{profile_name}` outputs {:?}, which is not supported by the packet stream source normalizer; adaptive HLS/DASH sources should use native playback",
        profile.output_container
    )))
}

fn resource_route_for_profile(
    profile: &SourceNormalizerProfile,
    preferred_route: Option<SourceNormalizerOutputRoute>,
) -> Result<SourceNormalizerOutputRoute, SourceNormalizerError> {
    let route = match profile.output_container {
        SourceNormalizerOutputContainer::Fmp4
        | SourceNormalizerOutputContainer::LocalStreamEndpoint
        | SourceNormalizerOutputContainer::ResourceUrl => {
            SourceNormalizerOutputRoute::Fmp4LocalStream
        }
        SourceNormalizerOutputContainer::Hls => SourceNormalizerOutputRoute::HlsShortWindow,
    };
    if let Some(preferred_route) = preferred_route
        && preferred_route != route
    {
        return Err(SourceNormalizerError::unsupported_operation(format!(
            "profile outputs {}, but host requested {}",
            route.wire_name(),
            preferred_route.wire_name()
        )));
    }
    Ok(route)
}

fn output_path_for_route(output_dir: &Path, route: SourceNormalizerOutputRoute) -> PathBuf {
    match route {
        SourceNormalizerOutputRoute::Fmp4LocalStream => output_dir.join("normalized.mp4"),
        SourceNormalizerOutputRoute::HlsShortWindow => output_dir.join("index.m3u8"),
        SourceNormalizerOutputRoute::PacketStream => output_dir.join("packet-stream.bin"),
    }
}

fn resource_infos_for_route(
    output_dir: &Path,
    primary_path: &Path,
    route: SourceNormalizerOutputRoute,
    growing: bool,
) -> Vec<SourceNormalizerResourceInfo> {
    let mut resources = vec![SourceNormalizerResourceInfo {
        role: primary_resource_role(route).to_owned(),
        path: primary_path.display().to_string(),
        content_type: Some(content_type_for_route(route).to_owned()),
        byte_length: file_len(primary_path),
        growing,
    }];

    if route == SourceNormalizerOutputRoute::HlsShortWindow
        && let Ok(entries) = std::fs::read_dir(output_dir)
    {
        let mut segment_paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(|name| {
                            name.starts_with("segment_")
                                || name == "init.mp4"
                                || name.ends_with(".m4s")
                                || name.ends_with(".ts")
                        })
                        .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        segment_paths.sort();
        resources.extend(segment_paths.into_iter().map(|path| {
            let content_type = if path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.eq_ignore_ascii_case("ts"))
                .unwrap_or(false)
            {
                "video/mp2t"
            } else {
                "video/mp4"
            };
            SourceNormalizerResourceInfo {
                role: "segment".to_owned(),
                path: path.display().to_string(),
                content_type: Some(content_type.to_owned()),
                byte_length: file_len(&path),
                growing,
            }
        }));
    }

    resources
}

fn file_len(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn content_type_for_route(route: SourceNormalizerOutputRoute) -> &'static str {
    match route {
        SourceNormalizerOutputRoute::Fmp4LocalStream => "video/mp4",
        SourceNormalizerOutputRoute::HlsShortWindow => "application/vnd.apple.mpegurl",
        SourceNormalizerOutputRoute::PacketStream => "application/octet-stream",
    }
}

fn primary_resource_role(route: SourceNormalizerOutputRoute) -> &'static str {
    match route {
        SourceNormalizerOutputRoute::Fmp4LocalStream => "media",
        SourceNormalizerOutputRoute::HlsShortWindow => "playlist",
        SourceNormalizerOutputRoute::PacketStream => "packet_stream",
    }
}

fn disk_usage_bytes(path: &Path) -> Option<u64> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.is_file() {
        return Some(metadata.len());
    }
    if !metadata.is_dir() {
        return Some(0);
    }
    let mut total = 0u64;
    for entry in std::fs::read_dir(path).ok()? {
        let entry = entry.ok()?;
        total = total.saturating_add(disk_usage_bytes(&entry.path()).unwrap_or(0));
    }
    Some(total)
}

fn resource_worker_state(state: &Arc<Mutex<ResourceWorkerState>>) -> ResourceWorkerState {
    state
        .lock()
        .map(|state| state.clone())
        .unwrap_or_else(|error| error.into_inner().clone())
}

fn set_resource_worker_state(
    state: &Arc<Mutex<ResourceWorkerState>>,
    new_state: SourceNormalizerResourceSessionState,
    message: Option<String>,
) {
    let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
    state.state = new_state;
    state.message = message;
}

fn run_resource_worker(config: ResourceWorkerConfig) {
    set_resource_worker_state(
        &config.state,
        SourceNormalizerResourceSessionState::Running,
        Some("resource worker remuxing to disk-backed normalized output".to_owned()),
    );
    let result = remux_resource_to_disk(&config);
    match result {
        Ok(()) => {
            let state = if config.cancel_requested.load(Ordering::SeqCst) {
                SourceNormalizerResourceSessionState::Cancelled
            } else {
                SourceNormalizerResourceSessionState::Ready
            };
            let message = if state == SourceNormalizerResourceSessionState::Cancelled {
                "resource worker cancelled".to_owned()
            } else {
                "resource worker produced disk-backed normalized output".to_owned()
            };
            set_resource_worker_state(&config.state, state, Some(message));
        }
        Err(error) => {
            let state = if config.cancel_requested.load(Ordering::SeqCst) {
                SourceNormalizerResourceSessionState::Cancelled
            } else {
                SourceNormalizerResourceSessionState::Failed
            };
            set_resource_worker_state(&config.state, state, Some(error.to_string()));
        }
    }
}

fn remux_resource_to_disk(config: &ResourceWorkerConfig) -> Result<(), SourceNormalizerError> {
    if config.route == SourceNormalizerOutputRoute::PacketStream {
        return Err(SourceNormalizerError::unsupported_operation(
            "packet stream resource output is reserved for the native frame pipeline",
        ));
    }
    ffmpeg::init().map_err(|error| {
        SourceNormalizerError::internal(format!("failed to initialize FFmpeg: {error}"))
    })?;
    std::fs::create_dir_all(&config.output_dir).map_err(|error| {
        SourceNormalizerError::internal(format!(
            "failed to create resource output directory: {error}"
        ))
    })?;
    let _ = std::fs::remove_file(&config.output_path);

    let mut input_context = open_resource_input(&config.input, &config.profile)?;
    let mut output_context = open_resource_output(&config.output_path, config.route)?;
    enable_incremental_output(&mut output_context);

    let mut stream_mapping = vec![-1; input_context.nb_streams() as usize];
    let mut input_time_bases = vec![ffmpeg::Rational(0, 1); input_context.nb_streams() as usize];
    let mut output_stream_index = 0usize;
    let mut tracks = Vec::new();

    for (input_stream_index, input_stream) in input_context.streams().enumerate() {
        let medium = input_stream.parameters().medium();
        if medium != media::Type::Audio && medium != media::Type::Video {
            continue;
        }
        stream_mapping[input_stream_index] = i32::try_from(output_stream_index).unwrap_or(i32::MAX);
        input_time_bases[input_stream_index] = input_stream.time_base();
        output_stream_index = output_stream_index.saturating_add(1);
        if let Ok(track) = resource_track_info(&input_stream) {
            tracks.push(track);
        }

        let mut output_stream = output_context
            .add_stream(encoder::find(codec::Id::None))
            .map_err(|error| {
                SourceNormalizerError::internal(format!(
                    "failed to add normalized output stream: {error}"
                ))
            })?;
        output_stream.set_parameters(input_stream.parameters());
        // SAFETY: FFmpeg requires codec_tag to be cleared after copying codec
        // parameters into another muxer; the output stream owns these parameters.
        unsafe {
            (*output_stream.parameters().as_mut_ptr()).codec_tag = 0;
        }
    }

    if output_stream_index == 0 {
        return Err(SourceNormalizerError::invalid_input(
            "input does not contain audio or video streams that can be remuxed",
        ));
    }

    output_context.set_metadata(input_context.metadata().to_owned());
    write_resource_header(
        &mut output_context,
        &config.output_dir,
        config.route,
        &config.profile,
    )?;
    flush_output_context(&mut output_context);
    enforce_session_disk_quota(
        &config.output_dir,
        config.cache_policy.session_disk_soft_cap_bytes,
    )?;

    let output_time_bases = (0..output_stream_index)
        .map(|index| {
            output_context
                .stream(index)
                .map(|stream| stream.time_base())
                .unwrap_or(ffmpeg::Rational(0, 1))
        })
        .collect::<Vec<_>>();

    for (stream, mut packet) in input_context.packets() {
        if config.cancel_requested.load(Ordering::SeqCst) {
            return Ok(());
        }
        let input_stream_index = stream.index();
        let mapped_stream_index = stream_mapping[input_stream_index];
        if mapped_stream_index < 0 {
            continue;
        }
        let output_stream_index = usize::try_from(mapped_stream_index).unwrap_or_default();
        packet.rescale_ts(
            input_time_bases[input_stream_index],
            output_time_bases
                .get(output_stream_index)
                .copied()
                .unwrap_or(ffmpeg::Rational(0, 1)),
        );
        packet.set_position(-1);
        packet.set_stream(output_stream_index);
        packet
            .write_interleaved(&mut output_context)
            .map_err(|error| {
                SourceNormalizerError::internal(format!(
                    "failed to write normalized packet for profile `{}`: {error}",
                    config.profile_name
                ))
            })?;
        flush_output_context(&mut output_context);
        enforce_session_disk_quota(
            &config.output_dir,
            config.cache_policy.session_disk_soft_cap_bytes,
        )?;
    }

    output_context.write_trailer().map_err(|error| {
        SourceNormalizerError::internal(format!(
            "failed to finalize normalized output for profile `{}`: {error}",
            config.profile_name
        ))
    })?;
    flush_output_context(&mut output_context);
    let _ = tracks;
    Ok(())
}

fn open_resource_input(
    input: &str,
    profile: &SourceNormalizerProfile,
) -> Result<format::context::Input, SourceNormalizerError> {
    let mut options = ffmpeg::Dictionary::new();
    let mut has_options = false;
    apply_dictionary_options(&mut options, &profile.input_options);
    has_options |= !profile.input_options.is_empty();
    if should_apply_network_options(input) {
        apply_dictionary_options(&mut options, &profile.network);
        has_options |= !profile.network.is_empty();
    }
    let input_string = input.to_owned();
    if !has_options {
        ffmpeg::format::input(&input_string).map_err(|error| {
            SourceNormalizerError::invalid_input(format!("failed to open input: {error}"))
        })
    } else {
        ffmpeg::format::input_with_dictionary(&input_string, options).map_err(|error| {
            SourceNormalizerError::invalid_input(format!("failed to open input: {error}"))
        })
    }
}

fn open_resource_output(
    output_path: &Path,
    route: SourceNormalizerOutputRoute,
) -> Result<format::context::Output, SourceNormalizerError> {
    let output_path = output_path.to_string_lossy().into_owned();
    match route {
        SourceNormalizerOutputRoute::Fmp4LocalStream => {
            ffmpeg::format::output_as(&output_path, "mp4")
        }
        SourceNormalizerOutputRoute::HlsShortWindow => {
            ffmpeg::format::output_as(&output_path, "hls")
        }
        SourceNormalizerOutputRoute::PacketStream => {
            return Err(SourceNormalizerError::unsupported_operation(
                "packet stream resource output",
            ));
        }
    }
    .map_err(|error| {
        SourceNormalizerError::internal(format!("failed to create normalized output: {error}"))
    })
}

fn write_resource_header(
    output_context: &mut format::context::Output,
    output_dir: &Path,
    route: SourceNormalizerOutputRoute,
    profile: &SourceNormalizerProfile,
) -> Result<(), SourceNormalizerError> {
    let mut options = ffmpeg::Dictionary::new();
    apply_dictionary_options(&mut options, &profile.output_options);
    if route == SourceNormalizerOutputRoute::HlsShortWindow {
        let segment_pattern = output_dir
            .join("segment_%05d.m4s")
            .to_string_lossy()
            .into_owned();
        options.set("hls_segment_filename", &segment_pattern);
        if profile.output_options.get("hls_segment_type").is_none() {
            options.set("hls_segment_type", "fmp4");
        }
        if profile.output_options.get("hls_time").is_none() {
            options.set("hls_time", "3");
        }
        if profile.output_options.get("hls_list_size").is_none() {
            options.set("hls_list_size", "6");
        }
        if profile.output_options.get("hls_flags").is_none() {
            options.set(
                "hls_flags",
                "delete_segments+append_list+omit_endlist+independent_segments",
            );
        }
        if profile.output_options.get("hls_delete_threshold").is_none() {
            options.set("hls_delete_threshold", "2");
        }
    }
    output_context
        .write_header_with(options)
        .map(|_| ())
        .map_err(|error| {
            SourceNormalizerError::internal(format!(
                "failed to write normalized output header: {error}"
            ))
        })
}

fn apply_dictionary_options(
    dictionary: &mut ffmpeg::Dictionary<'_>,
    options: &std::collections::HashMap<String, toml::Value>,
) {
    for key in sorted_option_keys(options) {
        match &options[key] {
            toml::Value::Boolean(value) => {
                dictionary.set(key, if *value { "1" } else { "0" });
            }
            toml::Value::Array(values) => {
                let value = values
                    .iter()
                    .filter_map(toml_value_to_arg)
                    .collect::<Vec<_>>()
                    .join(",");
                if !value.is_empty() {
                    dictionary.set(key, &value);
                }
            }
            value => {
                if let Some(value) = toml_value_to_arg(value) {
                    dictionary.set(key, &value);
                }
            }
        }
    }
}

fn sorted_option_keys(options: &std::collections::HashMap<String, toml::Value>) -> Vec<&String> {
    let mut keys = options.keys().collect::<Vec<_>>();
    keys.sort_unstable();
    keys
}

fn toml_value_to_arg(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(value) => Some(value.clone()),
        toml::Value::Integer(value) => Some(value.to_string()),
        toml::Value::Float(value) => Some(value.to_string()),
        toml::Value::Boolean(value) => Some(if *value { "1" } else { "0" }.to_owned()),
        _ => None,
    }
}

fn should_apply_network_options(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("tcp://")
        || lower.starts_with("tls://")
}

fn enforce_session_disk_quota(
    output_dir: &Path,
    max_bytes: u64,
) -> Result<(), SourceNormalizerError> {
    if max_bytes == 0 {
        return Ok(());
    }
    let Some(used) = disk_usage_bytes(output_dir) else {
        return Ok(());
    };
    if used > max_bytes {
        return Err(SourceNormalizerError::internal(format!(
            "normalized resource session exceeded disk quota: used={used} limit={max_bytes}"
        )));
    }
    Ok(())
}

fn resource_track_info(
    stream: &ffmpeg::Stream<'_>,
) -> Result<SourceNormalizerPacketTrackInfo, SourceNormalizerError> {
    match stream.parameters().medium() {
        media::Type::Video => packet_track_info(stream),
        media::Type::Audio => audio_track_info(stream),
        _ => Err(SourceNormalizerError::unsupported_operation(
            "non-audio/video track",
        )),
    }
}

fn audio_track_info(
    stream: &ffmpeg::Stream<'_>,
) -> Result<SourceNormalizerPacketTrackInfo, SourceNormalizerError> {
    let parameters = stream.parameters();
    let codec =
        ffmpeg::codec::context::Context::from_parameters(parameters.clone()).map_err(|error| {
            SourceNormalizerError::internal(format!("failed to inspect stream: {error}"))
        })?;
    let codec_name = format!("{:?}", codec.id());
    Ok(SourceNormalizerPacketTrackInfo {
        stream_index: u32::try_from(stream.index()).unwrap_or(u32::MAX),
        media_kind: SourceNormalizerPacketMediaKind::Audio,
        codec: codec_name.clone(),
        extradata: codec_parameters_extradata(&parameters),
        bitstream_format: Some(bitstream_format_for_codec_name(&codec_name)),
        width: None,
        height: None,
        coded_width: None,
        coded_height: None,
        sample_rate: None,
        channels: None,
        frame_rate: None,
        time_base_num: Some(stream.time_base().numerator()),
        time_base_den: Some(stream.time_base().denominator()),
    })
}

fn enable_incremental_output(output_context: &mut format::context::Output) {
    // SAFETY: `output_context` owns a live AVFormatContext. The flags changed
    // here are public libavformat fields intended to request packet flushing.
    unsafe {
        let context = output_context.as_mut_ptr();
        if !context.is_null() {
            (*context).flags |= ffmpeg::ffi::AVFMT_FLAG_FLUSH_PACKETS;
            (*context).flush_packets = 1;
        }
    }
}

fn flush_output_context(output_context: &mut format::context::Output) {
    // SAFETY: `output_context` owns a live AVFormatContext. Its `pb` pointer is
    // managed by FFmpeg and may be null for muxers without an AVIOContext.
    unsafe {
        let context = output_context.as_mut_ptr();
        if !context.is_null() {
            let io_context = (*context).pb;
            if !io_context.is_null() {
                ffmpeg::ffi::avio_flush(io_context);
            }
        }
    }
}

fn mime_hint_for_input(input: &str) -> Option<&'static str> {
    let lower = input.to_ascii_lowercase();
    let path = lower
        .split_once('#')
        .map(|(path, _)| path)
        .unwrap_or(lower.as_str());
    let path = path.split_once('?').map(|(path, _)| path).unwrap_or(path);
    if path.ends_with(".flv") {
        Some("video/x-flv")
    } else if path.ends_with(".m3u8") {
        Some("application/vnd.apple.mpegurl")
    } else if path.ends_with(".mpd") {
        Some("application/dash+xml")
    } else {
        None
    }
}

fn open_ffmpeg_input(input: &str) -> Result<ffmpeg::format::context::Input, SourceNormalizerError> {
    ffmpeg::init().map_err(|error| {
        SourceNormalizerError::internal(format!("failed to initialize FFmpeg: {error}"))
    })?;
    ffmpeg::format::input(input).map_err(|error| {
        SourceNormalizerError::invalid_input(format!("failed to open input: {error}"))
    })
}

fn packet_track_info(
    stream: &ffmpeg::Stream<'_>,
) -> Result<SourceNormalizerPacketTrackInfo, SourceNormalizerError> {
    let parameters = stream.parameters();
    let codec =
        ffmpeg::codec::context::Context::from_parameters(parameters.clone()).map_err(|error| {
            SourceNormalizerError::internal(format!("failed to inspect stream: {error}"))
        })?;
    let codec_name = format!("{:?}", codec.id());
    let decoder = codec.decoder().video().map_err(|error| {
        SourceNormalizerError::internal(format!("failed to inspect video stream: {error}"))
    })?;
    Ok(SourceNormalizerPacketTrackInfo {
        stream_index: u32::try_from(stream.index()).unwrap_or(u32::MAX),
        media_kind: SourceNormalizerPacketMediaKind::Video,
        codec: codec_name.clone(),
        extradata: codec_parameters_extradata(&parameters),
        bitstream_format: Some(bitstream_format_for_codec_name(&codec_name)),
        width: Some(decoder.width()).filter(|width| *width > 0),
        height: Some(decoder.height()).filter(|height| *height > 0),
        coded_width: Some(decoder.width()).filter(|width| *width > 0),
        coded_height: Some(decoder.height()).filter(|height| *height > 0),
        sample_rate: None,
        channels: None,
        frame_rate: rational_to_f64(stream.avg_frame_rate())
            .or_else(|| rational_to_f64(stream.rate())),
        time_base_num: Some(stream.time_base().numerator()),
        time_base_den: Some(stream.time_base().denominator()),
    })
}

fn codec_parameters_extradata(parameters: &ffmpeg::codec::Parameters) -> Vec<u8> {
    // SAFETY: `parameters` is owned by FFmpeg and remains valid for this call;
    // extradata is copied into an owned Vec before returning.
    unsafe {
        let parameters = parameters.as_ptr();
        if parameters.is_null()
            || (*parameters).extradata.is_null()
            || (*parameters).extradata_size <= 0
        {
            return Vec::new();
        }
        let len = usize::try_from((*parameters).extradata_size).unwrap_or_default();
        std::slice::from_raw_parts((*parameters).extradata, len).to_vec()
    }
}

fn rational_to_f64(value: ffmpeg::Rational) -> Option<f64> {
    if value.numerator() <= 0 || value.denominator() <= 0 {
        return None;
    }
    Some(f64::from(value))
}

fn timestamp_to_micros(timestamp: i64, time_base: ffmpeg::Rational) -> Option<i64> {
    let numerator = i128::from(time_base.numerator());
    let denominator = i128::from(time_base.denominator());
    if denominator <= 0 {
        return None;
    }
    let value = i128::from(timestamp)
        .saturating_mul(numerator)
        .saturating_mul(1_000_000)
        / denominator;
    Some(value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
}

fn duration_millis_from_av_duration(duration_us: i64) -> Option<u64> {
    u64::try_from(duration_us)
        .ok()
        .map(|duration| duration / 1_000)
}

fn bitstream_format_for_codec_name(codec: &str) -> DecoderBitstreamFormat {
    if codec.eq_ignore_ascii_case("HEVC") || codec.eq_ignore_ascii_case("H265") {
        DecoderBitstreamFormat::Hvcc
    } else if codec.eq_ignore_ascii_case("H264") {
        DecoderBitstreamFormat::Avcc
    } else {
        DecoderBitstreamFormat::Unknown(codec.to_owned())
    }
}

fn unique_session_suffix() -> u128 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = u128::from(NEXT_SESSION_SUFFIX.fetch_add(1, Ordering::Relaxed));
    (nanos << 64) | counter
}

fn map_core_error(error: player_source_normalizer::SourceNormalizerError) -> SourceNormalizerError {
    use player_source_normalizer::SourceNormalizerError as CoreError;

    let message = error.to_string();
    match error {
        CoreError::UnknownRuntimeProfile { profile } => {
            SourceNormalizerError::UnsupportedRuntimeProfile { profile }
        }
        CoreError::ReadFile { .. }
        | CoreError::ParseToml { .. }
        | CoreError::UnknownFfmpegProfile { .. }
        | CoreError::RuntimeProfileCycle { .. }
        | CoreError::FfmpegProfileCycle { .. }
        | CoreError::InvalidRuntimeProfile { .. }
        | CoreError::CapabilityMismatch { .. } => SourceNormalizerError::configuration(message),
        CoreError::SpawnFfmpeg { command, .. } | CoreError::FfmpegFailed { command, .. } => {
            let _ = command;
            SourceNormalizerError::internal(message)
        }
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

fn packet_open_error(
    error: SourceNormalizerError,
) -> VesperSourceNormalizerOpenPacketSessionResult {
    VesperSourceNormalizerOpenPacketSessionResult {
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

fn process_error(error: SourceNormalizerError) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Failure,
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

fn resource_open_error(
    error: SourceNormalizerError,
) -> VesperSourceNormalizerOpenResourceSessionResult {
    VesperSourceNormalizerOpenResourceSessionResult {
        status: VesperPluginResultStatus::Failure,
        session: std::ptr::null_mut(),
        payload: serialize_payload(&error),
    }
}

unsafe extern "C" fn free_plugin_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
    // SAFETY: the payload was produced by this dynamic library and has not been
    // reclaimed yet.
    let _ = unsafe { payload.into_vec() };
}

fn serialize_payload<T: serde::Serialize>(value: &T) -> VesperPluginBytes {
    match serde_json::to_vec(value) {
        Ok(payload) => VesperPluginBytes::from_vec(payload),
        Err(error) => VesperPluginBytes::from_vec(error.to_string().into_bytes()),
    }
}

fn catch_bytes(operation: impl FnOnce() -> VesperPluginBytes) -> VesperPluginBytes {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)).unwrap_or_else(|_| {
        serialize_payload(&SourceNormalizerError::abi_violation(
            "source normalizer callback panicked",
        ))
    })
}

fn catch_packet_open(
    operation: impl FnOnce() -> VesperSourceNormalizerOpenPacketSessionResult,
) -> VesperSourceNormalizerOpenPacketSessionResult {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
        .unwrap_or_else(|_| packet_open_error(SourceNormalizerError::internal("callback panicked")))
}

fn catch_read_packet(
    operation: impl FnOnce() -> VesperSourceNormalizerReadPacketResult,
) -> VesperSourceNormalizerReadPacketResult {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
        .unwrap_or_else(|_| read_packet_error(SourceNormalizerError::internal("callback panicked")))
}

fn catch_resource_open(
    operation: impl FnOnce() -> VesperSourceNormalizerOpenResourceSessionResult,
) -> VesperSourceNormalizerOpenResourceSessionResult {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)).unwrap_or_else(|_| {
        resource_open_error(SourceNormalizerError::internal("callback panicked"))
    })
}

fn catch_process(
    operation: impl FnOnce() -> VesperPluginProcessResult,
) -> VesperPluginProcessResult {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
        .unwrap_or_else(|_| process_error(SourceNormalizerError::internal("callback panicked")))
}

#[cfg(test)]
mod tests {
    use super::{
        detect_profile_name, free_plugin_bytes, load_profile_set, normalizer_close_packet_session,
        normalizer_open_packet_session_json, normalizer_packet_capabilities_json,
        normalizer_read_packet, normalizer_release_packet, packet_capabilities_from_profiles,
        unique_session_suffix, validate_packet_profile,
    };
    use player_plugin::{
        SourceNormalizerError, SourceNormalizerOperationStatus, SourceNormalizerPacketCapabilities,
        SourceNormalizerPacketMediaKind, SourceNormalizerPacketSessionConfig,
        SourceNormalizerPacketStreamInfo, SourceNormalizerReadPacketMetadata,
        SourceNormalizerReadPacketStatus, VesperPluginBytes, VesperPluginResultStatus,
    };
    use std::path::PathBuf;

    #[test]
    fn packet_capabilities_from_default_profiles_are_serializable() {
        let capabilities = packet_capabilities_from_profiles(load_profile_set());

        assert!(
            capabilities
                .supported_runtime_profiles
                .contains(&"generic-fallback".to_owned())
        );
        assert_eq!(
            capabilities.media_kinds,
            vec![SourceNormalizerPacketMediaKind::Video]
        );
        assert!(capabilities.supports_codec("h264"));
        assert!(capabilities.supports_codec("hevc"));
        assert!(capabilities.supports_codec("av1"));

        // SAFETY: the callback ignores context and returns a plugin-owned payload.
        let payload = unsafe { normalizer_packet_capabilities_json(std::ptr::null_mut()) };
        let decoded: SourceNormalizerPacketCapabilities = take_plugin_bytes(payload);
        assert_eq!(decoded, capabilities);
    }

    #[test]
    fn open_rejects_empty_input() {
        let open = open_packet_session(SourceNormalizerPacketSessionConfig {
            runtime_profile: "generic-fallback".to_owned(),
            input: String::new(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        });

        assert_eq!(open.status, VesperPluginResultStatus::Failure);
        assert!(open.session.is_null());
        let error: SourceNormalizerError = take_plugin_bytes(open.payload);
        assert!(matches!(error, SourceNormalizerError::InvalidInput { .. }));
    }

    #[test]
    fn open_rejects_hls_profile_before_ffmpeg_probe() {
        let open = open_packet_session(SourceNormalizerPacketSessionConfig {
            runtime_profile: "hls-nonstandard".to_owned(),
            input: "https://example.test/master.m3u8".to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        });

        assert_eq!(open.status, VesperPluginResultStatus::Failure);
        assert!(open.session.is_null());
        let error: SourceNormalizerError = take_plugin_bytes(open.payload);
        assert!(matches!(error, SourceNormalizerError::InvalidInput { .. }));
        assert!(format!("{error}").contains("adaptive HLS/DASH sources"));
    }

    #[test]
    fn packet_profile_validation_allows_fmp4_profiles_only() {
        let profile_set = load_profile_set().expect("load default source normalizer profiles");
        let generic = profile_set
            .require("generic-fallback")
            .expect("generic profile exists");
        validate_packet_profile("generic-fallback", generic)
            .expect("generic fmp4 profile should be supported");

        let hls = profile_set
            .require("hls-nonstandard")
            .expect("hls profile exists");
        let error = validate_packet_profile("hls-nonstandard", hls)
            .expect_err("hls output profile should be rejected");
        assert!(matches!(error, SourceNormalizerError::InvalidInput { .. }));
    }

    #[test]
    fn hls_input_detects_hls_profile() {
        let profile_set = load_profile_set().expect("load default source normalizer profiles");

        assert_eq!(
            detect_profile_name(&profile_set, "https://example.test/master.m3u8"),
            "hls-nonstandard"
        );
    }

    #[test]
    fn fixture_packet_session_reads_releases_and_closes() {
        let fixture = fixture_path();
        if !fixture.is_file() {
            eprintln!(
                "skipping FFmpeg source normalizer fixture test: {} is unavailable",
                fixture.display()
            );
            return;
        }

        let open = open_packet_session(SourceNormalizerPacketSessionConfig {
            runtime_profile: "generic-fallback".to_owned(),
            input: fixture.to_string_lossy().into_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        });
        assert_eq!(open.status, VesperPluginResultStatus::Success);
        assert!(!open.session.is_null());
        let stream_info: SourceNormalizerPacketStreamInfo = take_plugin_bytes(open.payload);
        assert_eq!(
            stream_info.normalizer_name.as_deref(),
            Some("player-source-normalizer-ffmpeg")
        );
        assert!(!stream_info.tracks.is_empty());

        // SAFETY: the session pointer was returned by this plugin's open call.
        let packet = unsafe { normalizer_read_packet(std::ptr::null_mut(), open.session) };
        assert_eq!(packet.status, VesperPluginResultStatus::Success);
        assert_eq!(packet.packet_handle, 1);
        assert!(!packet.data.is_null());
        assert!(packet.data_len > 0);
        let metadata: SourceNormalizerReadPacketMetadata = take_plugin_bytes(packet.metadata);
        assert_eq!(metadata.status, SourceNormalizerReadPacketStatus::Packet);
        assert!(metadata.packet.is_some());

        // SAFETY: the handle was returned by the preceding read.
        let release = unsafe {
            normalizer_release_packet(std::ptr::null_mut(), open.session, packet.packet_handle)
        };
        assert_eq!(release.status, VesperPluginResultStatus::Success);
        let release_status: SourceNormalizerOperationStatus = take_plugin_bytes(release.payload);
        assert!(release_status.completed);

        // SAFETY: the session pointer was returned by open and is closed once.
        let close = unsafe { normalizer_close_packet_session(std::ptr::null_mut(), open.session) };
        assert_eq!(close.status, VesperPluginResultStatus::Success);
        let close_status: SourceNormalizerOperationStatus = take_plugin_bytes(close.payload);
        assert!(close_status.completed);
    }

    #[test]
    fn unique_session_suffix_is_monotonic_enough_for_collisions() {
        let first = unique_session_suffix();
        let second = unique_session_suffix();

        assert_ne!(first, second);
    }

    #[test]
    fn free_bytes_accepts_null_payload() {
        // SAFETY: freeing a null/empty payload is a no-op by the shared bytes
        // contract.
        unsafe { free_plugin_bytes(std::ptr::null_mut(), VesperPluginBytes::null()) };
    }

    fn open_packet_session(
        config: SourceNormalizerPacketSessionConfig,
    ) -> player_plugin::VesperSourceNormalizerOpenPacketSessionResult {
        let config_json = serde_json::to_vec(&config).expect("serialize config");
        // SAFETY: the JSON buffer remains alive for this synchronous callback.
        unsafe {
            normalizer_open_packet_session_json(
                std::ptr::null_mut(),
                config_json.as_ptr(),
                config_json.len(),
            )
        }
    }

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/media/tiny-h264-aac.m4v")
    }

    fn take_plugin_bytes<T: serde::de::DeserializeOwned>(payload: VesperPluginBytes) -> T {
        // SAFETY: test payloads are allocated by this plugin and have not been
        // reclaimed before this helper.
        let bytes = unsafe { payload.into_vec() };
        serde_json::from_slice(&bytes).expect("deserialize payload")
    }
}
