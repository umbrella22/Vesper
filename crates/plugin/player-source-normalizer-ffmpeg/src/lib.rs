#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{CString, c_char, c_void};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{
    Arc, Condvar, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ffmpeg_next::util::format::Pixel;
use ffmpeg_next::{self as ffmpeg, codec, encoder, format, media};
use player_plugin::{
    DecoderBitstreamFormat, NativeFrameColorMetadata, NativeFrameContentLightMetadata,
    NativeFrameDolbyVisionMetadata, NativeFrameHdrMetadata, NativeFrameMasteringDisplayMetadata,
    Plugin, PluginBuildError, SourceNormalizerError, SourceNormalizerNormalizeLevel,
    SourceNormalizerOperationStatus, SourceNormalizerOutputRoute, SourceNormalizerPacket,
    SourceNormalizerPacketCapabilities, SourceNormalizerPacketLease,
    SourceNormalizerPacketMediaKind, SourceNormalizerPacketPluginFactory,
    SourceNormalizerPacketSeek, SourceNormalizerPacketSession, SourceNormalizerPacketSessionConfig,
    SourceNormalizerPacketStreamInfo, SourceNormalizerPacketTrackInfo,
    SourceNormalizerReadPacketMetadata, SourceNormalizerRequiredCapabilities,
    SourceNormalizerResourceCachePolicy, SourceNormalizerResourceCapabilities,
    SourceNormalizerResourceInfo, SourceNormalizerResourcePluginFactory,
    SourceNormalizerResourceSession, SourceNormalizerResourceSessionConfig,
    SourceNormalizerResourceSessionInfo, SourceNormalizerResourceSessionState,
    SourceNormalizerResourceSessionStatus, SourceNormalizerResourceSessionWaitStatus,
};
use player_source_normalizer::{
    SourceNormalizerOutputContainer, SourceNormalizerProfile, SourceNormalizerProfileSet,
    SourceNormalizerSessionConfig, SourceRuntimeDetector, build_ffmpeg_command_plan,
};
use std::os::raw::c_int;
use url::Url;

const PLUGIN_ID: &str = "io.github.umbrella22.vesper.source-normalizer-ffmpeg";
const PACKET_INSTANCE_ID: &str = "io.github.umbrella22.vesper.source-normalizer-ffmpeg.packet";
const RESOURCE_INSTANCE_ID: &str = "io.github.umbrella22.vesper.source-normalizer-ffmpeg.resource";
const PLUGIN_NAME: &str = "player-source-normalizer-ffmpeg";
const DEFAULT_PROFILE_TOML: &str =
    include_str!("../../../../scripts/source-normalizer-profiles.toml");
const PROFILE_PATH_ENV: &str = "VESPER_SOURCE_NORMALIZER_PROFILE_PATH";
const MAX_SKIPPED_NON_AV_PACKETS: usize = 10_000;
const RESOURCE_CLOSE_JOIN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RESOURCE_SESSIONS: u32 = 64;
const RESOURCE_CLEANUP_QUEUE_CAPACITY: usize = MAX_RESOURCE_SESSIONS as usize;
const RESOURCE_CLEANUP_POLL_INTERVAL: Duration = Duration::from_millis(25);
static NEXT_SESSION_SUFFIX: AtomicU64 = AtomicU64::new(1);
static RESOURCE_CLEANUP_SCHEDULER: OnceLock<Result<ResourceCleanupScheduler, String>> =
    OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FfmpegInputAuthority {
    Local,
    RemoteHttp,
}

impl FfmpegInputAuthority {
    fn protocol_whitelist(self) -> &'static str {
        match self {
            Self::Local => "file,crypto",
            Self::RemoteHttp => "http,https,tcp,tls,crypto",
        }
    }

    fn protocol_blacklist(self) -> &'static str {
        match self {
            Self::Local => "concat,subfile,http,https,tcp,tls",
            Self::RemoteHttp => "file,concat,subfile",
        }
    }
}

struct ResourceWorkerConfig {
    profile_name: String,
    profile: SourceNormalizerProfile,
    input: String,
    output_dir: PathBuf,
    output_path: PathBuf,
    route: SourceNormalizerOutputRoute,
    cache_policy: SourceNormalizerResourceCachePolicy,
    startup_timeout_ms: Option<u64>,
    read_idle_timeout_ms: Option<u64>,
    cancel_requested: Arc<AtomicBool>,
    shared: Arc<ResourceWorkerShared>,
}

struct PacketNormalizerSession {
    stream_info: SourceNormalizerPacketStreamInfo,
    input: TimedFfmpegInput,
    selected_video_stream_index: usize,
    selected_audio_stream_index: Option<usize>,
    tracks: Vec<SourceNormalizerPacketTrackInfo>,
    next_packet_handle: usize,
    leased_packet: Option<LeasedPacket>,
    closed: bool,
}

struct TimedFfmpegInput {
    input: ffmpeg::format::context::Input,
    interrupt: Box<FfmpegInterruptState>,
    read_idle_timeout_ms: Option<u64>,
}

struct FfmpegInterruptState {
    clock_started_at: Instant,
    operation_started_ms: AtomicU64,
    operation_timeout_ms: AtomicU64,
    cancel_requested: Option<Arc<AtomicBool>>,
}

#[derive(Debug)]
struct ResourceCleanupJob {
    worker: JoinHandle<()>,
    output_dir: PathBuf,
    permit: ResourceCleanupPermit,
}

#[derive(Debug)]
struct ResourceCleanupScheduler {
    queue: Arc<ResourceCleanupQueue>,
    permit_pool: Arc<ResourceCleanupPermitPool>,
}

#[derive(Debug)]
struct ResourceCleanupQueue {
    pending: Mutex<Vec<ResourceCleanupJob>>,
    changed: Condvar,
    capacity: usize,
}

#[derive(Debug)]
struct ResourceCleanupPermitPool {
    available: Mutex<usize>,
    capacity: usize,
}

#[derive(Debug)]
struct ResourceCleanupPermit {
    pool: Arc<ResourceCleanupPermitPool>,
}

#[derive(Debug)]
struct ResourceNormalizerSession {
    info: SourceNormalizerResourceSessionInfo,
    output_dir: PathBuf,
    shared: Arc<ResourceWorkerShared>,
    observed_sequence: u64,
    cancel_requested: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    cleanup_permit: Option<ResourceCleanupPermit>,
    closed: bool,
}

#[derive(Debug)]
struct ResourceOutputDirGuard {
    output_dir: PathBuf,
    armed: bool,
}

#[derive(Debug)]
struct ResourceWorkerShared {
    state: Mutex<ResourceWorkerState>,
    changed: Condvar,
}

#[derive(Debug, Clone)]
struct ResourceWorkerState {
    state: SourceNormalizerResourceSessionState,
    message: Option<String>,
    tracks: Vec<SourceNormalizerPacketTrackInfo>,
    sequence: u64,
    worker_finished: bool,
}

impl ResourceCleanupScheduler {
    fn start(capacity: usize, thread_name: &str) -> Result<Self, String> {
        let queue = Arc::new(ResourceCleanupQueue {
            pending: Mutex::new(Vec::with_capacity(capacity)),
            changed: Condvar::new(),
            capacity,
        });
        let worker_queue = queue.clone();
        thread::Builder::new()
            .name(thread_name.to_owned())
            .spawn(move || run_resource_cleanup_worker(worker_queue))
            .map_err(|error| format!("failed to start resource cleanup worker: {error}"))?;
        Ok(Self {
            queue,
            permit_pool: Arc::new(ResourceCleanupPermitPool {
                available: Mutex::new(capacity),
                capacity,
            }),
        })
    }

    fn try_acquire(&self) -> Option<ResourceCleanupPermit> {
        self.permit_pool.try_acquire()
    }

    fn try_schedule(&self, job: ResourceCleanupJob) -> Result<(), ResourceCleanupJob> {
        let mut pending = self
            .queue
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if pending.len() >= self.queue.capacity {
            return Err(job);
        }
        pending.push(job);
        self.queue.changed.notify_one();
        Ok(())
    }
}

impl ResourceCleanupPermitPool {
    fn try_acquire(self: &Arc<Self>) -> Option<ResourceCleanupPermit> {
        let mut available = self
            .available
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if *available == 0 {
            return None;
        }
        *available -= 1;
        Some(ResourceCleanupPermit { pool: self.clone() })
    }
}

impl Drop for ResourceCleanupPermit {
    fn drop(&mut self) {
        let mut available = self
            .pool
            .available
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if *available < self.pool.capacity {
            *available += 1;
        }
    }
}

impl Drop for PacketNormalizerSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl ResourceOutputDirGuard {
    fn new(output_dir: PathBuf) -> Self {
        Self {
            output_dir,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ResourceOutputDirGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = cleanup_resource_output_dir(&self.output_dir);
        }
    }
}

#[derive(Debug)]
struct LeasedPacket {
    handle: usize,
    data: Vec<u8>,
}

impl TimedFfmpegInput {
    fn new(
        input: ffmpeg::format::context::Input,
        interrupt: Box<FfmpegInterruptState>,
        read_idle_timeout_ms: Option<u64>,
    ) -> Self {
        Self {
            input,
            interrupt,
            read_idle_timeout_ms,
        }
    }

    fn cancel(&self) {
        self.interrupt.request_cancel();
    }

    fn read_packet(
        &mut self,
    ) -> Result<Option<(ffmpeg::Stream<'_>, ffmpeg::Packet)>, SourceNormalizerError> {
        let _operation = self.interrupt.begin_operation(self.read_idle_timeout_ms);
        let mut packet = ffmpeg::Packet::empty();
        match packet.read(&mut self.input) {
            Ok(()) => {
                let stream_index = packet.stream();
                let stream = self.input.stream(stream_index).ok_or_else(|| {
                    SourceNormalizerError::internal(format!(
                        "packet referenced missing stream index {stream_index}"
                    ))
                })?;
                Ok(Some((stream, packet)))
            }
            Err(ffmpeg::Error::Eof) => Ok(None),
            Err(error) => {
                if let Some(message) = self.interrupt.interrupt_message("read packet") {
                    Err(SourceNormalizerError::internal(message))
                } else {
                    Err(SourceNormalizerError::internal(format!(
                        "failed to read input packet: {error}"
                    )))
                }
            }
        }
    }

    fn seek_packet(&mut self, timestamp: i64) -> Result<(), SourceNormalizerError> {
        let _operation = self.interrupt.begin_operation(self.read_idle_timeout_ms);
        self.input.seek(timestamp, ..timestamp).map_err(|error| {
            self.interrupt
                .interrupt_message("seek packet input")
                .map(SourceNormalizerError::internal)
                .unwrap_or_else(|| {
                    SourceNormalizerError::internal(format!("failed to seek packet input: {error}"))
                })
        })
    }
}

impl Deref for TimedFfmpegInput {
    type Target = ffmpeg::format::context::Input;

    fn deref(&self) -> &Self::Target {
        &self.input
    }
}

impl DerefMut for TimedFfmpegInput {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.input
    }
}

impl FfmpegInterruptState {
    fn new(cancel_requested: Option<Arc<AtomicBool>>) -> Self {
        Self {
            clock_started_at: Instant::now(),
            operation_started_ms: AtomicU64::new(0),
            operation_timeout_ms: AtomicU64::new(0),
            cancel_requested,
        }
    }

    fn begin_operation(&self, timeout_ms: Option<u64>) -> FfmpegInterruptOperation<'_> {
        let timeout_ms = timeout_ms.unwrap_or(0);
        if timeout_ms == 0 {
            self.operation_timeout_ms.store(0, Ordering::SeqCst);
            self.operation_started_ms.store(0, Ordering::SeqCst);
        } else {
            let started_ms = self.elapsed_millis().saturating_add(1);
            self.operation_started_ms
                .store(started_ms, Ordering::SeqCst);
            self.operation_timeout_ms
                .store(timeout_ms.max(1), Ordering::SeqCst);
        }
        FfmpegInterruptOperation { state: self }
    }

    fn request_cancel(&self) {
        if let Some(cancel_requested) = &self.cancel_requested {
            cancel_requested.store(true, Ordering::SeqCst);
        }
    }

    fn should_interrupt(&self) -> bool {
        if self
            .cancel_requested
            .as_ref()
            .map(|cancel_requested| cancel_requested.load(Ordering::SeqCst))
            .unwrap_or(false)
        {
            return true;
        }
        self.operation_timed_out()
    }

    fn interrupt_message(&self, operation: &str) -> Option<String> {
        if self
            .cancel_requested
            .as_ref()
            .map(|cancel_requested| cancel_requested.load(Ordering::SeqCst))
            .unwrap_or(false)
        {
            return Some(format!("FFmpeg {operation} cancelled"));
        }
        if self.operation_timed_out() {
            return Some(format!("FFmpeg {operation} timed out"));
        }
        None
    }

    fn operation_timed_out(&self) -> bool {
        let timeout_ms = self.operation_timeout_ms.load(Ordering::SeqCst);
        let started_ms = self.operation_started_ms.load(Ordering::SeqCst);
        if timeout_ms == 0 || started_ms == 0 {
            return false;
        }
        self.elapsed_millis()
            .saturating_sub(started_ms.saturating_sub(1))
            >= timeout_ms
    }

    fn elapsed_millis(&self) -> u64 {
        u64::try_from(self.clock_started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn clear_operation(&self) {
        self.operation_timeout_ms.store(0, Ordering::SeqCst);
        self.operation_started_ms.store(0, Ordering::SeqCst);
    }
}

struct FfmpegInterruptOperation<'a> {
    state: &'a FfmpegInterruptState,
}

impl Drop for FfmpegInterruptOperation<'_> {
    fn drop(&mut self) {
        self.state.clear_operation();
    }
}

extern "C" fn ffmpeg_interrupt_callback(opaque: *mut c_void) -> c_int {
    if opaque.is_null() {
        return 0;
    }
    // SAFETY: `opaque` is set to a stable `FfmpegInterruptState` allocation
    // owned by `TimedFfmpegInput` or by the resource worker while the FFmpeg
    // context using this callback is alive.
    let state = unsafe { &*(opaque.cast::<FfmpegInterruptState>()) };
    if state.should_interrupt() { 1 } else { 0 }
}

#[derive(Debug, Default)]
struct FfmpegPacketFactory;

impl SourceNormalizerPacketPluginFactory for FfmpegPacketFactory {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn packet_capabilities(&self) -> SourceNormalizerPacketCapabilities {
        packet_capabilities_from_profiles(load_profile_set())
    }

    fn open_packet_session(
        &self,
        config: &SourceNormalizerPacketSessionConfig,
    ) -> Result<Box<dyn SourceNormalizerPacketSession>, SourceNormalizerError> {
        if config.input.is_empty() {
            return Err(SourceNormalizerError::invalid_input(
                "input must not be empty",
            ));
        }

        let profile_set = load_profile_set()?;
        let profile_name = if config.runtime_profile.is_empty() {
            detect_profile_name(&profile_set, &config.input)
        } else {
            config.runtime_profile.clone()
        };
        let profile = profile_set.require(&profile_name).map_err(map_core_error)?;
        validate_packet_profile(&profile_name, profile)?;

        let input = open_ffmpeg_input(
            &config.input,
            profile,
            config.startup_timeout_ms,
            config.session_timeout_ms,
            None,
        )?;
        let Some(stream) = input.streams().best(ffmpeg::media::Type::Video) else {
            return Err(SourceNormalizerError::invalid_input(
                "input does not contain a video stream",
            ));
        };
        let video_stream_index = stream.index();
        let video_track = packet_track_info(&stream)?;
        let mut tracks = vec![video_track];
        let selected_audio_stream_index =
            if let Some(audio_stream) = input.streams().best(ffmpeg::media::Type::Audio) {
                let audio_stream_index = audio_stream.index();
                tracks.push(audio_track_info(&audio_stream)?);
                Some(audio_stream_index)
            } else {
                None
            };
        let duration_millis = duration_millis_from_av_duration(input.duration());
        let seekable = input.duration() > 0;
        let stream_info = SourceNormalizerPacketStreamInfo {
            session_id: Some(format!("ffmpeg-packet-{}", unique_session_suffix())),
            normalizer_name: Some("player-source-normalizer-ffmpeg".to_owned()),
            runtime_profile: Some(profile_name),
            selected_backend: Some("ffmpeg-next".to_owned()),
            tracks: tracks.clone(),
            selected_track_index: Some(u32::try_from(video_stream_index).unwrap_or(u32::MAX)),
            duration_millis,
            seekable,
        };
        Ok(Box::new(PacketNormalizerSession {
            stream_info,
            input,
            selected_video_stream_index: video_stream_index,
            selected_audio_stream_index,
            tracks,
            next_packet_handle: 1,
            leased_packet: None,
            closed: false,
        }))
    }
}

impl SourceNormalizerPacketSession for PacketNormalizerSession {
    fn stream_info(&self) -> SourceNormalizerPacketStreamInfo {
        self.stream_info.clone()
    }

    fn read_packet(&mut self) -> Result<SourceNormalizerPacketLease<'_>, SourceNormalizerError> {
        if self.closed {
            return Err(SourceNormalizerError::NotConfigured);
        }
        if self.leased_packet.is_some() {
            return Err(SourceNormalizerError::abi_violation(
                "previous packet lease has not been released",
            ));
        }

        let selected_video_stream_index = self.selected_video_stream_index;
        let selected_audio_stream_index = self.selected_audio_stream_index;
        let tracks = self.tracks.clone();
        let mut skipped_packets = 0usize;
        loop {
            match self.input.read_packet() {
                Ok(Some((stream, packet)))
                    if stream.index() == selected_video_stream_index
                        || selected_audio_stream_index == Some(stream.index()) =>
                {
                    let stream_index = stream.index();
                    let track = u32::try_from(stream_index).ok().and_then(|stream_index| {
                        tracks
                            .iter()
                            .find(|track| track.stream_index == stream_index)
                            .cloned()
                    });
                    let media_kind = track
                        .as_ref()
                        .map(|track| track.media_kind)
                        .unwrap_or(SourceNormalizerPacketMediaKind::Video);
                    let time_base = stream.time_base();
                    let data = packet.data().map(<[u8]>::to_vec).unwrap_or_default();
                    let handle = self.next_packet_handle;
                    self.next_packet_handle = self.next_packet_handle.saturating_add(1).max(1);
                    let metadata =
                        SourceNormalizerReadPacketMetadata::packet(SourceNormalizerPacket {
                            pts_us: packet
                                .pts()
                                .and_then(|timestamp| timestamp_to_micros(timestamp, time_base)),
                            dts_us: packet
                                .dts()
                                .and_then(|timestamp| timestamp_to_micros(timestamp, time_base)),
                            duration_us: timestamp_to_micros(packet.duration(), time_base)
                                .filter(|duration| *duration > 0),
                            stream_index: u32::try_from(stream_index).unwrap_or(u32::MAX),
                            media_kind,
                            key_frame: packet.is_key(),
                            discontinuity: false,
                            sample_rate: track.as_ref().and_then(|track| track.sample_rate),
                            channels: track.as_ref().and_then(|track| track.channels),
                            channel_layout: track.and_then(|track| track.channel_layout),
                            sample_format: None,
                            frame_count: packet.duration().checked_abs().and_then(|duration| {
                                if media_kind == SourceNormalizerPacketMediaKind::Audio {
                                    u32::try_from(duration).ok()
                                } else {
                                    None
                                }
                            }),
                            end_of_stream: false,
                        });
                    let leased = self.leased_packet.insert(LeasedPacket { handle, data });
                    return Ok(SourceNormalizerPacketLease {
                        metadata,
                        data: &leased.data,
                        handle: leased.handle,
                    });
                }
                Ok(Some((_stream, _packet))) => {
                    skipped_packets = skipped_packets.saturating_add(1);
                    if skipped_packets > MAX_SKIPPED_NON_AV_PACKETS {
                        return Err(SourceNormalizerError::internal(format!(
                            "skipped too many non-selected packets ({skipped_packets})"
                        )));
                    }
                }
                Ok(None) => {
                    return Ok(SourceNormalizerPacketLease {
                        metadata: SourceNormalizerReadPacketMetadata::end_of_stream(),
                        data: &[],
                        handle: 0,
                    });
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn release_packet(&mut self, packet_handle: usize) -> Result<(), SourceNormalizerError> {
        if self.closed {
            return Err(SourceNormalizerError::NotConfigured);
        }
        match self.leased_packet.as_ref() {
            Some(packet) if packet.handle == packet_handle => {
                self.leased_packet = None;
                Ok(())
            }
            Some(_packet) => Err(SourceNormalizerError::abi_violation(format!(
                "unknown packet handle {packet_handle}"
            ))),
            None => Err(SourceNormalizerError::abi_violation(
                "no packet lease is outstanding",
            )),
        }
    }

    fn seek(
        &mut self,
        seek: &SourceNormalizerPacketSeek,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        if self.closed {
            return Err(SourceNormalizerError::NotConfigured);
        }
        self.leased_packet = None;
        let timestamp = seek
            .position_millis
            .saturating_mul(1_000)
            .min(i64::MAX as u64) as i64;
        self.input.seek_packet(timestamp)?;
        Ok(SourceNormalizerOperationStatus {
            completed: true,
            message: Some(format!("seeked to {} ms", seek.position_millis)),
        })
    }

    fn flush(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        if self.closed {
            return Err(SourceNormalizerError::NotConfigured);
        }
        self.leased_packet = None;
        Ok(SourceNormalizerOperationStatus {
            completed: true,
            message: None,
        })
    }

    fn close(&mut self) -> Result<(), SourceNormalizerError> {
        if self.closed {
            return Ok(());
        }
        self.input.cancel();
        self.leased_packet = None;
        self.closed = true;
        Ok(())
    }
}

#[derive(Debug, Default)]
struct FfmpegResourceFactory;

impl SourceNormalizerResourcePluginFactory for FfmpegResourceFactory {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn resource_capabilities(&self) -> SourceNormalizerResourceCapabilities {
        resource_capabilities_from_profiles(load_profile_set())
    }

    fn open_resource_session(
        &self,
        config: &SourceNormalizerResourceSessionConfig,
    ) -> Result<Box<dyn SourceNormalizerResourceSession>, SourceNormalizerError> {
        if config.input.is_empty() {
            return Err(SourceNormalizerError::invalid_input(
                "input must not be empty",
            ));
        }
        if config.output_root.is_empty() {
            return Err(SourceNormalizerError::configuration(
                "output_root must not be empty",
            ));
        }

        let profile_set = load_profile_set()?;
        let profile_name = if config.runtime_profile.is_empty() {
            detect_profile_name(&profile_set, &config.input)
        } else {
            config.runtime_profile.clone()
        };
        let profile = profile_set.require(&profile_name).map_err(map_core_error)?;
        let route = resource_route_for_profile(profile, config.preferred_route)?;
        let cleanup_permit =
            resource_cleanup_scheduler().and_then(acquire_resource_cleanup_permit)?;
        let session_id = format!("ffmpeg-resource-{}", unique_session_suffix());
        let output_dir = Path::new(&config.output_root).join(&session_id);
        let output_path = output_path_for_route(&output_dir, route);
        let command_plan = build_ffmpeg_command_plan(
            profile,
            &SourceNormalizerSessionConfig {
                runtime_profile: profile_name.clone(),
                input: config.input.clone(),
                output: output_path.clone(),
                ffmpeg_program: "ffmpeg".to_owned(),
                output_to_stdout: false,
            },
        )
        .map_err(map_core_error)?;
        std::fs::create_dir_all(&output_dir).map_err(|error| {
            SourceNormalizerError::internal(format!(
                "failed to create resource output directory: {error}"
            ))
        })?;
        let mut output_dir_guard = ResourceOutputDirGuard::new(output_dir.clone());
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
        let shared = Arc::new(ResourceWorkerShared {
            state: Mutex::new(ResourceWorkerState {
                state: SourceNormalizerResourceSessionState::Starting,
                message: Some(format!(
                    "resource session starting; argv={}",
                    command_plan.argv().join(" ")
                )),
                tracks: Vec::new(),
                sequence: 1,
                worker_finished: false,
            }),
            changed: Condvar::new(),
        });
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let worker_shared = shared.clone();
        let worker_cancel = cancel_requested.clone();
        let worker_profile = profile.clone();
        let worker_profile_name = profile_name.clone();
        let worker_input = config.input.clone();
        let worker_output_dir = output_dir.clone();
        let worker_output_path = output_path.clone();
        let worker_route = route;
        let worker_cache_policy = config.cache_policy.clone();
        let worker_startup_timeout_ms = config.startup_timeout_ms;
        let worker_read_idle_timeout_ms = config.read_idle_timeout_ms;
        let worker = thread::Builder::new()
            .name("vesper-source-normalizer-resource".to_owned())
            .spawn(move || {
                let terminal_shared = worker_shared.clone();
                run_resource_worker_with_panic_guard(terminal_shared, || {
                    run_resource_worker(ResourceWorkerConfig {
                        profile_name: worker_profile_name,
                        profile: worker_profile,
                        input: worker_input,
                        output_dir: worker_output_dir,
                        output_path: worker_output_path,
                        route: worker_route,
                        cache_policy: worker_cache_policy,
                        startup_timeout_ms: worker_startup_timeout_ms,
                        read_idle_timeout_ms: worker_read_idle_timeout_ms,
                        cancel_requested: worker_cancel,
                        shared: worker_shared,
                    });
                });
            })
            .map_err(|error| {
                SourceNormalizerError::internal(format!("failed to start resource worker: {error}"))
            })?;
        let session = ResourceNormalizerSession {
            info: info.clone(),
            output_dir,
            shared,
            observed_sequence: 0,
            cancel_requested,
            worker: Some(worker),
            cleanup_permit: Some(cleanup_permit),
            closed: false,
        };
        output_dir_guard.disarm();
        Ok(Box::new(session))
    }
}

impl SourceNormalizerResourceSession for ResourceNormalizerSession {
    fn session_info(&self) -> SourceNormalizerResourceSessionInfo {
        self.info.clone()
    }

    fn poll(&mut self) -> Result<SourceNormalizerResourceSessionStatus, SourceNormalizerError> {
        if self.closed {
            return Err(SourceNormalizerError::NotConfigured);
        }
        let worker_state = resource_worker_state(&self.shared);
        self.observed_sequence = worker_state.sequence;
        let mut info = self.info.clone();
        info.resources = resource_infos_for_route(
            &self.output_dir,
            Path::new(info.primary_resource_path.as_deref().unwrap_or_default()),
            info.output_route,
            matches!(
                worker_state.state,
                SourceNormalizerResourceSessionState::Starting
                    | SourceNormalizerResourceSessionState::Running
                    | SourceNormalizerResourceSessionState::Ready
            ),
        );
        if !worker_state.tracks.is_empty() {
            info.tracks = worker_state.tracks;
        }
        info.disk_bytes_used = disk_usage_bytes(&self.output_dir);
        Ok(SourceNormalizerResourceSessionStatus {
            state: worker_state.state,
            info: Some(info),
            message: worker_state.message,
            disk_bytes_used: disk_usage_bytes(&self.output_dir),
        })
    }

    fn wait_for_update(
        &mut self,
        timeout: Duration,
    ) -> Result<SourceNormalizerResourceSessionWaitStatus, SourceNormalizerError> {
        if self.closed {
            return Err(SourceNormalizerError::NotConfigured);
        }
        let timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
        Ok(wait_resource_worker_update(
            &self.shared,
            &mut self.observed_sequence,
            timeout_ms,
        ))
    }

    fn cancel(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        if self.closed {
            return Err(SourceNormalizerError::NotConfigured);
        }
        self.cancel_requested.store(true, Ordering::SeqCst);
        let completed = request_resource_worker_cancel(&self.shared);
        Ok(SourceNormalizerOperationStatus {
            completed,
            message: Some(if completed {
                "resource session already stopped".to_owned()
            } else {
                "resource session cancellation requested".to_owned()
            }),
        })
    }

    fn close(&mut self) -> Result<(), SourceNormalizerError> {
        self.close_inner()
    }
}

impl ResourceNormalizerSession {
    fn close_inner(&mut self) -> Result<(), SourceNormalizerError> {
        if self.closed {
            return Ok(());
        }
        self.cancel_requested.store(true, Ordering::SeqCst);
        request_resource_worker_cancel(&self.shared);
        let cleanup_permit = self.cleanup_permit.take().ok_or_else(|| {
            SourceNormalizerError::abi_violation("resource cleanup permit is missing")
        })?;
        let worker = self.worker.take();
        let worker_joinable = worker.as_ref().is_none_or(|_| {
            wait_resource_worker_joinable(&self.shared, RESOURCE_CLOSE_JOIN_TIMEOUT)
        });
        match (worker, worker_joinable) {
            (Some(worker), true) => {
                let join_failed = worker.join().is_err();
                if let Some(message) = cleanup_resource_output_dir(&self.output_dir) {
                    self.cleanup_permit = Some(cleanup_permit);
                    return Err(SourceNormalizerError::internal(message));
                }
                if join_failed {
                    self.cleanup_permit = Some(cleanup_permit);
                    return Err(SourceNormalizerError::internal(
                        "resource worker panicked during cleanup",
                    ));
                }
            }
            (Some(worker), false) => {
                if let Err(job) =
                    schedule_resource_cleanup(worker, self.output_dir.clone(), cleanup_permit)
                {
                    self.worker = Some(job.worker);
                    self.cleanup_permit = Some(job.permit);
                    return Err(SourceNormalizerError::resource_exhausted(
                        "resource cleanup queue is full; retry close",
                    ));
                }
            }
            (None, _) => {
                if let Some(message) = cleanup_resource_output_dir(&self.output_dir) {
                    self.cleanup_permit = Some(cleanup_permit);
                    return Err(SourceNormalizerError::internal(message));
                }
            }
        }
        self.closed = true;
        Ok(())
    }
}

impl Drop for ResourceNormalizerSession {
    fn drop(&mut self) {
        if self.close_inner().is_err() {
            let _ = self.close_inner();
        }
    }
}

#[player_plugin::export]
fn ffmpeg_source_normalizer_plugin() -> Result<Plugin, PluginBuildError> {
    Plugin::builder(PLUGIN_ID, PLUGIN_NAME)?
        .with_source_normalizer_packet(PACKET_INSTANCE_ID, FfmpegPacketFactory)?
        .with_source_normalizer_resource(RESOURCE_INSTANCE_ID, FfmpegResourceFactory)?
        .build()
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
        if validate_packet_profile(name, profile).is_err() {
            continue;
        }
        profiles.push(name.to_owned());
        merge_required_capabilities(&mut required, &profile.required_capabilities);
    }
    SourceNormalizerPacketCapabilities {
        supported_runtime_profiles: profiles,
        max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
        media_kinds: vec![
            SourceNormalizerPacketMediaKind::Video,
            SourceNormalizerPacketMediaKind::Audio,
        ],
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
        return SourceNormalizerResourceCapabilities {
            max_sessions: Some(MAX_RESOURCE_SESSIONS),
            ..SourceNormalizerResourceCapabilities::default()
        };
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
        max_sessions: Some(MAX_RESOURCE_SESSIONS),
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

fn resource_worker_state(shared: &Arc<ResourceWorkerShared>) -> ResourceWorkerState {
    shared
        .state
        .lock()
        .map(|state| state.clone())
        .unwrap_or_else(|error| error.into_inner().clone())
}

fn set_resource_worker_state(
    shared: &Arc<ResourceWorkerShared>,
    new_state: SourceNormalizerResourceSessionState,
    message: Option<String>,
) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    state.state = new_state;
    state.message = message;
    state.sequence = state.sequence.wrapping_add(1);
    shared.changed.notify_all();
}

fn set_resource_worker_finished_state(
    shared: &Arc<ResourceWorkerShared>,
    new_state: SourceNormalizerResourceSessionState,
    message: Option<String>,
) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    state.state = new_state;
    state.message = message;
    state.worker_finished = true;
    state.sequence = state.sequence.wrapping_add(1);
    shared.changed.notify_all();
}

fn set_resource_worker_tracks(
    shared: &Arc<ResourceWorkerShared>,
    tracks: Vec<SourceNormalizerPacketTrackInfo>,
) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    state.tracks = tracks;
    state.sequence = state.sequence.wrapping_add(1);
    shared.changed.notify_all();
}

fn notify_resource_worker_state(shared: &Arc<ResourceWorkerShared>) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    state.sequence = state.sequence.wrapping_add(1);
    shared.changed.notify_all();
}

fn request_resource_worker_cancel(shared: &Arc<ResourceWorkerShared>) -> bool {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let completed = state.worker_finished;
    if !completed {
        state.message = Some("resource session cancellation requested".to_owned());
        state.sequence = state.sequence.wrapping_add(1);
    }
    shared.changed.notify_all();
    completed
}

fn wait_resource_worker_update(
    shared: &Arc<ResourceWorkerShared>,
    observed_sequence: &mut u64,
    timeout_ms: u64,
) -> SourceNormalizerResourceSessionWaitStatus {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if state.sequence != *observed_sequence {
        *observed_sequence = state.sequence;
        return SourceNormalizerResourceSessionWaitStatus { updated: true };
    }
    if timeout_ms == 0 {
        return SourceNormalizerResourceSessionWaitStatus { updated: false };
    }
    let timeout = Duration::from_millis(timeout_ms);
    let (state_after_wait, _wait_result) = shared
        .changed
        .wait_timeout(state, timeout)
        .unwrap_or_else(|error| error.into_inner());
    state = state_after_wait;
    let updated = state.sequence != *observed_sequence;
    *observed_sequence = state.sequence;
    SourceNormalizerResourceSessionWaitStatus { updated }
}

fn wait_resource_worker_joinable(shared: &Arc<ResourceWorkerShared>, timeout: Duration) -> bool {
    let state = shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if resource_worker_state_is_joinable(&state) {
        return true;
    }
    let (state, _wait_result) = shared
        .changed
        .wait_timeout_while(state, timeout, |state| {
            !resource_worker_state_is_joinable(state)
        })
        .unwrap_or_else(|error| error.into_inner());
    resource_worker_state_is_joinable(&state)
}

fn resource_worker_state_is_joinable(state: &ResourceWorkerState) -> bool {
    state.worker_finished && resource_worker_state_is_terminal(state.state)
}

fn resource_worker_state_is_terminal(state: SourceNormalizerResourceSessionState) -> bool {
    matches!(
        state,
        SourceNormalizerResourceSessionState::Ready
            | SourceNormalizerResourceSessionState::Failed
            | SourceNormalizerResourceSessionState::Cancelled
    )
}

fn cleanup_resource_output_dir(output_dir: &Path) -> Option<String> {
    match std::fs::remove_dir_all(output_dir) {
        Ok(()) => None,
        Err(error) if !output_dir.exists() => {
            let _ = error;
            None
        }
        Err(error) => Some(format!("cleanup failed: {error}")),
    }
}

fn resource_cleanup_scheduler() -> Result<&'static ResourceCleanupScheduler, SourceNormalizerError>
{
    match RESOURCE_CLEANUP_SCHEDULER.get_or_init(|| {
        ResourceCleanupScheduler::start(
            RESOURCE_CLEANUP_QUEUE_CAPACITY,
            "vesper-source-normalizer-cleanup",
        )
    }) {
        Ok(scheduler) => Ok(scheduler),
        Err(message) => Err(SourceNormalizerError::internal(message.clone())),
    }
}

fn acquire_resource_cleanup_permit(
    scheduler: &ResourceCleanupScheduler,
) -> Result<ResourceCleanupPermit, SourceNormalizerError> {
    scheduler.try_acquire().ok_or_else(|| {
        SourceNormalizerError::resource_exhausted(format!(
            "resource session capacity exhausted; at most {} workers may be active or awaiting cleanup",
            scheduler.permit_pool.capacity
        ))
    })
}

fn schedule_resource_cleanup(
    worker: JoinHandle<()>,
    output_dir: PathBuf,
    permit: ResourceCleanupPermit,
) -> Result<(), ResourceCleanupJob> {
    let job = ResourceCleanupJob {
        worker,
        output_dir,
        permit,
    };
    match resource_cleanup_scheduler() {
        Ok(scheduler) => scheduler.try_schedule(job),
        Err(_) => Err(job),
    }
}

fn run_resource_cleanup_worker(queue: Arc<ResourceCleanupQueue>) {
    loop {
        let ready = {
            let mut pending = queue
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while pending.is_empty() {
                pending = queue
                    .changed
                    .wait(pending)
                    .unwrap_or_else(|error| error.into_inner());
            }
            let (mut pending, _) = queue
                .changed
                .wait_timeout(pending, RESOURCE_CLEANUP_POLL_INTERVAL)
                .unwrap_or_else(|error| error.into_inner());
            let mut ready = Vec::new();
            let mut index = 0;
            while index < pending.len() {
                if pending[index].worker.is_finished() {
                    ready.push(pending.swap_remove(index));
                } else {
                    index += 1;
                }
            }
            ready
        };
        for job in ready {
            cleanup_resource_job(job);
        }
    }
}

fn cleanup_resource_job(job: ResourceCleanupJob) {
    let ResourceCleanupJob {
        worker,
        output_dir,
        permit,
    } = job;
    let _ = worker.join();
    let _ = cleanup_resource_output_dir(&output_dir);
    drop(permit);
}

fn run_resource_worker(config: ResourceWorkerConfig) {
    set_resource_worker_state(
        &config.shared,
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
            set_resource_worker_finished_state(&config.shared, state, Some(message));
        }
        Err(error) => {
            let state = if config.cancel_requested.load(Ordering::SeqCst) {
                SourceNormalizerResourceSessionState::Cancelled
            } else {
                SourceNormalizerResourceSessionState::Failed
            };
            set_resource_worker_finished_state(&config.shared, state, Some(error.to_string()));
        }
    }
}

fn run_resource_worker_with_panic_guard(shared: Arc<ResourceWorkerShared>, worker: impl FnOnce()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(worker)).is_err() {
        set_resource_worker_finished_state(
            &shared,
            SourceNormalizerResourceSessionState::Failed,
            Some("resource worker panicked".to_owned()),
        );
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

    let mut input_context = open_ffmpeg_input(
        &config.input,
        &config.profile,
        config.startup_timeout_ms,
        config.read_idle_timeout_ms,
        Some(config.cancel_requested.clone()),
    )?;
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
    set_resource_worker_tracks(&config.shared, tracks);

    output_context.set_metadata(input_context.metadata().to_owned());
    write_resource_header(
        &mut output_context,
        &config.output_dir,
        config.route,
        &config.profile,
    )?;
    flush_output_context(&mut output_context);
    let mut primary_resource_ready_notified = false;
    notify_if_primary_resource_has_bytes(
        &config.shared,
        &config.output_path,
        &mut primary_resource_ready_notified,
    );
    enforce_session_disk_quota(
        &config.output_dir,
        config.cache_policy.session_disk_soft_cap_bytes,
    )?;

    let output_time_bases = (0..output_stream_index)
        .map(|index| {
            output_context
                .stream(index)
                .map(|stream| stream.time_base())
                .ok_or_else(|| {
                    SourceNormalizerError::internal(format!(
                        "normalized output is missing stream index {index} after header"
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    loop {
        if config.cancel_requested.load(Ordering::SeqCst) {
            return Ok(());
        }
        let Some((stream, mut packet)) = input_context.read_packet()? else {
            break;
        };
        let input_stream_index = stream.index();
        let Some((output_stream_index, input_time_base)) =
            resource_packet_stream_route(&stream_mapping, &input_time_bases, input_stream_index)?
        else {
            continue;
        };
        let output_time_base = output_time_bases.get(output_stream_index).copied().ok_or_else(|| {
            SourceNormalizerError::internal(format!(
                "normalized output mapping references missing stream index {output_stream_index}"
            ))
        })?;
        packet.rescale_ts(input_time_base, output_time_base);
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
        notify_if_primary_resource_has_bytes(
            &config.shared,
            &config.output_path,
            &mut primary_resource_ready_notified,
        );
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
    notify_if_primary_resource_has_bytes(
        &config.shared,
        &config.output_path,
        &mut primary_resource_ready_notified,
    );
    Ok(())
}

fn resource_packet_stream_route(
    stream_mapping: &[i32],
    input_time_bases: &[ffmpeg::Rational],
    input_stream_index: usize,
) -> Result<Option<(usize, ffmpeg::Rational)>, SourceNormalizerError> {
    let mapped_stream_index = stream_mapping.get(input_stream_index).copied().ok_or_else(|| {
        SourceNormalizerError::unsupported_operation(format!(
            "dynamic input stream index {input_stream_index} appeared after the normalized output header"
        ))
    })?;
    if mapped_stream_index < 0 {
        return Ok(None);
    }
    let input_time_base = input_time_bases
        .get(input_stream_index)
        .copied()
        .ok_or_else(|| {
            SourceNormalizerError::unsupported_operation(format!(
                "dynamic input stream index {input_stream_index} has no header-time base"
            ))
        })?;
    let output_stream_index = usize::try_from(mapped_stream_index).map_err(|_| {
        SourceNormalizerError::internal(format!(
            "invalid normalized output stream mapping {mapped_stream_index}"
        ))
    })?;
    Ok(Some((output_stream_index, input_time_base)))
}

fn notify_if_primary_resource_has_bytes(
    shared: &Arc<ResourceWorkerShared>,
    output_path: &Path,
    already_notified: &mut bool,
) {
    if *already_notified {
        return;
    }
    let has_bytes = std::fs::metadata(output_path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false);
    if has_bytes {
        *already_notified = true;
        notify_resource_worker_state(shared);
    }
}

fn open_ffmpeg_input(
    input: &str,
    profile: &SourceNormalizerProfile,
    startup_timeout_ms: Option<u64>,
    read_idle_timeout_ms: Option<u64>,
    cancel_requested: Option<Arc<AtomicBool>>,
) -> Result<TimedFfmpegInput, SourceNormalizerError> {
    let authority = classify_ffmpeg_input_authority(input)?;
    let open_target = resolve_ffmpeg_open_target(input, authority)?;
    let mut options = ffmpeg::Dictionary::new();
    apply_dictionary_options(&mut options, &profile.input_options);
    if should_apply_network_options(input) {
        apply_dictionary_options(&mut options, &profile.network);
    }
    apply_timeout_dictionary_options(&mut options, startup_timeout_ms, read_idle_timeout_ms);
    enforce_ffmpeg_input_protocol_policy(&mut options, authority);
    open_timed_ffmpeg_input(
        &open_target,
        options,
        startup_timeout_ms,
        read_idle_timeout_ms,
        cancel_requested,
    )
}

fn resolve_ffmpeg_open_target(
    input: &str,
    authority: FfmpegInputAuthority,
) -> Result<String, SourceNormalizerError> {
    if authority != FfmpegInputAuthority::Local
        || Path::new(input).is_absolute()
        || is_windows_absolute_path(input)
    {
        return Ok(input.to_owned());
    }

    let Ok(url) = Url::parse(input) else {
        return Ok(input.to_owned());
    };
    if url.scheme() != "file" {
        return Ok(input.to_owned());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(SourceNormalizerError::invalid_input(
            "local file URL must not contain a query or fragment",
        ));
    }

    url.to_file_path()
        .map_err(|_| SourceNormalizerError::invalid_input("local file URL is not a valid path"))?
        .into_os_string()
        .into_string()
        .map_err(|_| SourceNormalizerError::invalid_input("local file URL path is not valid UTF-8"))
}

fn enforce_ffmpeg_input_protocol_policy(
    options: &mut ffmpeg::Dictionary<'_>,
    authority: FfmpegInputAuthority,
) {
    options.set("protocol_whitelist", authority.protocol_whitelist());
    options.set("protocol_blacklist", authority.protocol_blacklist());
}

fn classify_ffmpeg_input_authority(
    input: &str,
) -> Result<FfmpegInputAuthority, SourceNormalizerError> {
    if input.is_empty() {
        return Err(SourceNormalizerError::invalid_input(
            "input must not be empty",
        ));
    }
    let lower = input.to_ascii_lowercase();
    if lower.starts_with("concat:")
        || lower.starts_with("subfile:")
        || lower.starts_with("subfile,,")
    {
        return Err(SourceNormalizerError::invalid_input(
            "indirect FFmpeg concat and subfile inputs are not allowed",
        ));
    }
    if Path::new(input).is_absolute() || is_windows_absolute_path(input) {
        return Ok(FfmpegInputAuthority::Local);
    }

    match Url::parse(input) {
        Ok(url) => match url.scheme() {
            "http" | "https" if url.host_str().is_some() => Ok(FfmpegInputAuthority::RemoteHttp),
            "file"
                if url
                    .host_str()
                    .is_none_or(|host| host.eq_ignore_ascii_case("localhost")) =>
            {
                Ok(FfmpegInputAuthority::Local)
            }
            scheme => Err(SourceNormalizerError::invalid_input(format!(
                "FFmpeg input scheme `{scheme}` is not allowed; expected a local path, local file URL, or HTTP(S) URL"
            ))),
        },
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            if input.starts_with("//") || input.starts_with(r"\\") {
                Err(SourceNormalizerError::invalid_input(
                    "network file paths are not allowed as FFmpeg input",
                ))
            } else {
                Ok(FfmpegInputAuthority::Local)
            }
        }
        Err(error) => Err(SourceNormalizerError::invalid_input(format!(
            "invalid FFmpeg input URL: {error}"
        ))),
    }
}

fn is_windows_absolute_path(input: &str) -> bool {
    let bytes = input.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn open_timed_ffmpeg_input(
    input: &str,
    options: ffmpeg::Dictionary<'_>,
    startup_timeout_ms: Option<u64>,
    read_idle_timeout_ms: Option<u64>,
    cancel_requested: Option<Arc<AtomicBool>>,
) -> Result<TimedFfmpegInput, SourceNormalizerError> {
    ffmpeg::init().map_err(|error| {
        SourceNormalizerError::internal(format!("failed to initialize FFmpeg: {error}"))
    })?;
    let input = CString::new(input)
        .map_err(|_| SourceNormalizerError::invalid_input("input must not contain NUL bytes"))?;
    let mut interrupt = Box::new(FfmpegInterruptState::new(cancel_requested));
    let interrupt_ptr = (&mut *interrupt) as *mut FfmpegInterruptState;
    // SAFETY: ownership of the dictionary pointer is transferred to FFmpeg for
    // the open call below, then reclaimed with `Dictionary::own` before return.
    let mut raw_options = unsafe { options.disown() };
    // SAFETY: FFmpeg returns either a valid newly allocated AVFormatContext or
    // null; the pointer is checked before use and closed on every error path.
    let mut context = unsafe { ffmpeg::ffi::avformat_alloc_context() };
    if context.is_null() {
        // SAFETY: `raw_options` still belongs to this function because no
        // FFmpeg call has consumed it on this allocation-failure path.
        let _ = unsafe { ffmpeg::Dictionary::own(raw_options) };
        return Err(SourceNormalizerError::internal(
            "failed to allocate FFmpeg input context",
        ));
    }
    // SAFETY: `context` is a valid AVFormatContext from FFmpeg, and the opaque
    // pointer targets the boxed interrupt state kept alive by TimedFfmpegInput.
    unsafe {
        (*context).interrupt_callback = ffmpeg::ffi::AVIOInterruptCB {
            callback: Some(ffmpeg_interrupt_callback),
            opaque: interrupt_ptr.cast::<c_void>(),
        };
    }

    let open_result = {
        let _operation = interrupt.begin_operation(startup_timeout_ms);
        // SAFETY: `context`, `input`, and `raw_options` are valid FFmpeg values
        // for the duration of the call; `context` is closed on failure.
        unsafe {
            ffmpeg::ffi::avformat_open_input(
                &mut context,
                input.as_ptr(),
                ptr::null_mut(),
                &mut raw_options,
            )
        }
    };
    // SAFETY: FFmpeg may update the dictionary pointer; reclaiming it here
    // ensures any remaining entries are freed by the Rust wrapper.
    let _ = unsafe { ffmpeg::Dictionary::own(raw_options) };
    if open_result < 0 {
        close_raw_input_context(&mut context);
        return Err(ffmpeg_input_error(open_result, &interrupt, "open input"));
    }

    let stream_result = {
        let _operation = interrupt.begin_operation(startup_timeout_ms);
        // SAFETY: `context` is a successfully opened AVFormatContext and
        // remains owned by this function until wrapped below or closed on error.
        unsafe { ffmpeg::ffi::avformat_find_stream_info(context, ptr::null_mut()) }
    };
    if stream_result < 0 {
        close_raw_input_context(&mut context);
        return Err(ffmpeg_input_error(
            stream_result,
            &interrupt,
            "find stream info",
        ));
    }

    // SAFETY: `context` is a successfully opened input context; ownership is
    // transferred to ffmpeg-next's Input wrapper exactly once.
    let input = unsafe { ffmpeg::format::context::Input::wrap(context) };
    Ok(TimedFfmpegInput::new(
        input,
        interrupt,
        read_idle_timeout_ms,
    ))
}

fn close_raw_input_context(context: &mut *mut ffmpeg::ffi::AVFormatContext) {
    if (*context).is_null() {
        return;
    }
    // SAFETY: `context` is either null or an AVFormatContext still owned by
    // this function; FFmpeg nulls the pointer after closing it.
    unsafe {
        ffmpeg::ffi::avformat_close_input(context);
    }
}

fn ffmpeg_input_error(
    error_code: c_int,
    interrupt: &FfmpegInterruptState,
    operation: &str,
) -> SourceNormalizerError {
    if let Some(message) = interrupt.interrupt_message(operation) {
        return SourceNormalizerError::internal(message);
    }
    SourceNormalizerError::invalid_input(format!(
        "failed to {operation}: {}",
        ffmpeg::Error::from(error_code)
    ))
}

fn apply_timeout_dictionary_options(
    dictionary: &mut ffmpeg::Dictionary<'_>,
    startup_timeout_ms: Option<u64>,
    read_idle_timeout_ms: Option<u64>,
) {
    if let Some(timeout_us) = timeout_millis_to_ffmpeg_microseconds(startup_timeout_ms) {
        set_dictionary_default(dictionary, "timeout", &timeout_us);
        set_dictionary_default(dictionary, "stimeout", &timeout_us);
    }
    if let Some(timeout_us) = timeout_millis_to_ffmpeg_microseconds(read_idle_timeout_ms) {
        set_dictionary_default(dictionary, "rw_timeout", &timeout_us);
    }
}

fn timeout_millis_to_ffmpeg_microseconds(timeout_ms: Option<u64>) -> Option<String> {
    timeout_ms
        .filter(|timeout_ms| *timeout_ms > 0)
        .map(|timeout_ms| timeout_ms.saturating_mul(1_000).to_string())
}

fn set_dictionary_default(dictionary: &mut ffmpeg::Dictionary<'_>, key: &str, value: &str) {
    if dictionary.get(key).is_none() {
        dictionary.set(key, value);
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
        if !profile.output_options.contains_key("hls_segment_type") {
            options.set("hls_segment_type", "fmp4");
        }
        if !profile.output_options.contains_key("hls_time") {
            options.set("hls_time", "3");
        }
        if !profile.output_options.contains_key("hls_list_size") {
            options.set("hls_list_size", "6");
        }
        if !profile.output_options.contains_key("hls_flags") {
            options.set(
                "hls_flags",
                "delete_segments+append_list+omit_endlist+independent_segments",
            );
        }
        if !profile.output_options.contains_key("hls_delete_threshold") {
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
    let codec_name = format!("{:?}", parameters.id());
    let audio_parameters = audio_parameters_from_codec_parameters(&parameters);
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
        reorder_depth: None,
        sample_rate: audio_parameters.sample_rate,
        channels: audio_parameters.channels,
        channel_layout: audio_parameters.channel_layout,
        codec_delay_samples: audio_parameters.codec_delay_samples,
        priming_samples: audio_parameters.priming_samples,
        trailing_padding_samples: audio_parameters.trailing_padding_samples,
        seek_preroll_samples: audio_parameters.seek_preroll_samples,
        color: None,
        hdr: None,
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

fn packet_track_info(
    stream: &ffmpeg::Stream<'_>,
) -> Result<SourceNormalizerPacketTrackInfo, SourceNormalizerError> {
    let parameters = stream.parameters();
    let codec_name = format!("{:?}", parameters.id());
    let (width, height) = video_dimensions_from_parameters(&parameters);
    let color = video_color_metadata_from_parameters(&parameters);
    let side_data = hdr_side_data_summary(stream);
    let hdr =
        video_hdr_metadata_from_parameters(&parameters, &codec_name, color.as_ref(), &side_data);
    Ok(SourceNormalizerPacketTrackInfo {
        stream_index: u32::try_from(stream.index()).unwrap_or(u32::MAX),
        media_kind: SourceNormalizerPacketMediaKind::Video,
        codec: codec_name.clone(),
        extradata: codec_parameters_extradata(&parameters),
        bitstream_format: Some(bitstream_format_for_codec_name(&codec_name)),
        width,
        height,
        coded_width: width,
        coded_height: height,
        reorder_depth: video_reorder_depth_from_parameters(&parameters),
        sample_rate: None,
        channels: None,
        channel_layout: None,
        codec_delay_samples: None,
        priming_samples: None,
        trailing_padding_samples: None,
        seek_preroll_samples: None,
        color,
        hdr,
        frame_rate: rational_to_f64(stream.avg_frame_rate())
            .or_else(|| rational_to_f64(stream.rate())),
        time_base_num: Some(stream.time_base().numerator()),
        time_base_den: Some(stream.time_base().denominator()),
    })
}

fn video_reorder_depth_from_parameters(parameters: &ffmpeg::codec::Parameters) -> Option<u32> {
    // SAFETY: `parameters` points to a live AVCodecParameters owned by FFmpeg.
    // `video_delay` is copied synchronously and negative/zero values do not
    // describe a positive reorder window.
    unsafe {
        let raw = parameters.as_ptr();
        if raw.is_null() {
            return None;
        }
        u32::try_from((*raw).video_delay)
            .ok()
            .filter(|depth| *depth > 0)
    }
}

fn video_color_metadata_from_parameters(
    parameters: &ffmpeg::codec::Parameters,
) -> Option<NativeFrameColorMetadata> {
    // SAFETY: `parameters` points to a live AVCodecParameters owned by the input
    // stream. The color fields are scalar values copied synchronously.
    unsafe {
        let raw = parameters.as_ptr();
        if raw.is_null() {
            return None;
        }
        let color = NativeFrameColorMetadata {
            primaries: ffmpeg::util::color::Primaries::from((*raw).color_primaries)
                .name()
                .map(str::to_owned),
            transfer: ffmpeg::util::color::TransferCharacteristic::from((*raw).color_trc)
                .name()
                .map(str::to_owned),
            matrix: ffmpeg::util::color::Space::from((*raw).color_space)
                .name()
                .map(str::to_owned),
            range: ffmpeg::util::color::Range::from((*raw).color_range)
                .name()
                .map(str::to_owned),
            bit_depth: pixel_format_component_depth((*raw).format),
        };
        if color.primaries.is_none()
            && color.transfer.is_none()
            && color.matrix.is_none()
            && color.range.is_none()
            && color.bit_depth.is_none()
        {
            None
        } else {
            Some(color)
        }
    }
}

fn pixel_format_component_depth(raw_pixel_format: i32) -> Option<u8> {
    let pixel = Pixel::from(av_pixel_format_from_raw(raw_pixel_format)?);
    let descriptor = pixel.descriptor()?;
    // SAFETY: `descriptor` is a process-lifetime FFmpeg pixel-format descriptor.
    // We read the fixed component table synchronously without retaining pointers.
    unsafe {
        let descriptor = descriptor.as_ptr().as_ref()?;
        let component_count = usize::from(descriptor.nb_components).min(descriptor.comp.len());
        descriptor
            .comp
            .iter()
            .take(component_count)
            .filter_map(|component| u8::try_from(component.depth).ok())
            .filter(|depth| *depth > 0)
            .max()
    }
}

fn av_pixel_format_from_raw(raw_pixel_format: i32) -> Option<ffmpeg_next::ffi::AVPixelFormat> {
    if raw_pixel_format < 0 {
        return None;
    }

    // SAFETY: The FFmpeg descriptor iterator returns process-lifetime
    // descriptors. `av_pix_fmt_desc_get_id` maps each descriptor back to the
    // corresponding valid AVPixelFormat, so no raw integer is cast into the C
    // enum until FFmpeg has confirmed that the descriptor exists.
    unsafe {
        let mut descriptor = std::ptr::null();
        loop {
            descriptor = ffmpeg_next::ffi::av_pix_fmt_desc_next(descriptor);
            if descriptor.is_null() {
                return None;
            }
            let pixel_format = ffmpeg_next::ffi::av_pix_fmt_desc_get_id(descriptor);
            if pixel_format as i32 == raw_pixel_format {
                return Some(pixel_format);
            }
        }
    }
}

fn video_hdr_metadata_from_parameters(
    parameters: &ffmpeg::codec::Parameters,
    codec_name: &str,
    color: Option<&NativeFrameColorMetadata>,
    side_data: &HdrSideDataSummary,
) -> Option<NativeFrameHdrMetadata> {
    let codec_tag = codec_tag_from_parameters(parameters);
    let dolby_vision = side_data
        .dolby_vision
        .clone()
        .or_else(|| dolby_vision_metadata(codec_name, codec_tag));
    if let Some(dolby_vision) = dolby_vision {
        return Some(NativeFrameHdrMetadata {
            kind: "dolbyVision".to_owned(),
            mastering_display: side_data.mastering_display.clone(),
            content_light: side_data.content_light.clone(),
            dolby_vision: Some(dolby_vision),
        });
    }
    let transfer = color.and_then(|color| color.transfer.as_deref());
    let kind = match transfer.map(|transfer| transfer.to_ascii_lowercase()) {
        Some(transfer) if transfer.contains("smpte2084") || transfer.contains("pq") => {
            Some("hdr10")
        }
        Some(transfer) if transfer.contains("arib-std-b67") || transfer.contains("hlg") => {
            Some("hlg")
        }
        _ if side_data.has_mastering_display || side_data.has_content_light => Some("hdr10"),
        _ => None,
    }?;
    Some(NativeFrameHdrMetadata {
        kind: kind.to_owned(),
        mastering_display: side_data.mastering_display.clone(),
        content_light: side_data.content_light.clone(),
        dolby_vision: None,
    })
}

#[derive(Debug, Clone, Default)]
struct HdrSideDataSummary {
    has_mastering_display: bool,
    has_content_light: bool,
    mastering_display: Option<NativeFrameMasteringDisplayMetadata>,
    content_light: Option<NativeFrameContentLightMetadata>,
    dolby_vision: Option<NativeFrameDolbyVisionMetadata>,
}

fn hdr_side_data_summary(stream: &ffmpeg::Stream<'_>) -> HdrSideDataSummary {
    let mut summary = HdrSideDataSummary::default();
    for side_data in stream.side_data() {
        match side_data.kind() {
            ffmpeg::codec::packet::side_data::Type::MasteringDisplayMetadata => {
                summary.has_mastering_display = true;
                summary
                    .mastering_display
                    .get_or_insert(NativeFrameMasteringDisplayMetadata {
                        display_primaries: None,
                        white_point: None,
                        max_luminance_nits: None,
                        min_luminance_nits: None,
                    });
            }
            ffmpeg::codec::packet::side_data::Type::ContentLightLevel => {
                summary.has_content_light = true;
                summary
                    .content_light
                    .get_or_insert(NativeFrameContentLightMetadata {
                        max_content_light_level: None,
                        max_frame_average_light_level: None,
                    });
            }
            kind if format!("{kind:?}") == "DOVI_CONF" => {
                summary.dolby_vision = dolby_vision_metadata_from_dovi_conf(side_data.data());
            }
            _ => {}
        }
    }
    summary
}

fn codec_tag_from_parameters(parameters: &ffmpeg::codec::Parameters) -> Option<String> {
    // SAFETY: `parameters` points to a live AVCodecParameters owned by FFmpeg.
    // `codec_tag` is a copied FourCC-like scalar.
    unsafe {
        let raw = parameters.as_ptr();
        if raw.is_null() || (*raw).codec_tag == 0 {
            return None;
        }
        let bytes = (*raw).codec_tag.to_le_bytes();
        let tag = String::from_utf8_lossy(&bytes)
            .trim_matches(char::from(0))
            .to_owned();
        if tag.is_empty() { None } else { Some(tag) }
    }
}

fn dolby_vision_metadata(
    codec_name: &str,
    codec_tag: Option<String>,
) -> Option<NativeFrameDolbyVisionMetadata> {
    let codec_name = codec_name.to_ascii_lowercase();
    let codec_tag = codec_tag.unwrap_or_default().to_ascii_lowercase();
    if !(codec_name.contains("dovi")
        || codec_name.contains("dolby")
        || codec_tag == "dvh1"
        || codec_tag == "dvhe")
    {
        return None;
    }
    Some(NativeFrameDolbyVisionMetadata {
        profile: None,
        level: None,
        compatibility_id: None,
        has_rpu: true,
        has_el: false,
        has_bl: true,
    })
}

fn dolby_vision_metadata_from_dovi_conf(data: &[u8]) -> Option<NativeFrameDolbyVisionMetadata> {
    if data.len() < 8 {
        return None;
    }
    Some(NativeFrameDolbyVisionMetadata {
        profile: Some(data[2]),
        level: Some(data[3]),
        has_rpu: data[4] != 0,
        has_el: data[5] != 0,
        has_bl: data[6] != 0,
        compatibility_id: Some(data[7]),
    })
}

fn video_dimensions_from_parameters(
    parameters: &ffmpeg::codec::Parameters,
) -> (Option<u32>, Option<u32>) {
    // SAFETY: `parameters` points to a live AVCodecParameters owned by the input
    // stream. The width and height fields are plain values copied synchronously.
    unsafe {
        let parameters = parameters.as_ptr();
        if parameters.is_null() {
            return (None, None);
        }
        (
            u32::try_from((*parameters).width)
                .ok()
                .filter(|width| *width > 0),
            u32::try_from((*parameters).height)
                .ok()
                .filter(|height| *height > 0),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AudioCodecParameters {
    sample_rate: Option<u32>,
    channels: Option<u16>,
    channel_layout: Option<String>,
    codec_delay_samples: Option<u32>,
    priming_samples: Option<u32>,
    trailing_padding_samples: Option<u32>,
    seek_preroll_samples: Option<u32>,
}

fn audio_parameters_from_codec_parameters(
    parameters: &ffmpeg::codec::Parameters,
) -> AudioCodecParameters {
    // SAFETY: `parameters` points to a live AVCodecParameters owned by FFmpeg.
    // The code copies scalar audio metadata and writes FFmpeg's layout
    // description into an owned Rust buffer before returning.
    unsafe {
        let raw = parameters.as_ptr();
        if raw.is_null() {
            return AudioCodecParameters {
                sample_rate: None,
                channels: None,
                channel_layout: None,
                codec_delay_samples: None,
                priming_samples: None,
                trailing_padding_samples: None,
                seek_preroll_samples: None,
            };
        }
        let layout = (*raw).ch_layout;
        AudioCodecParameters {
            sample_rate: u32::try_from((*raw).sample_rate)
                .ok()
                .filter(|sample_rate| *sample_rate > 0),
            channels: u16::try_from(layout.nb_channels)
                .ok()
                .filter(|channels| *channels > 0),
            channel_layout: describe_channel_layout(&layout),
            codec_delay_samples: None,
            priming_samples: u32::try_from((*raw).initial_padding)
                .ok()
                .filter(|samples| *samples > 0),
            trailing_padding_samples: u32::try_from((*raw).trailing_padding)
                .ok()
                .filter(|samples| *samples > 0),
            seek_preroll_samples: u32::try_from((*raw).seek_preroll)
                .ok()
                .filter(|samples| *samples > 0),
        }
    }
}

fn describe_channel_layout(layout: &ffmpeg::ffi::AVChannelLayout) -> Option<String> {
    if layout.nb_channels <= 0 {
        return None;
    }
    let mut buffer = [0 as c_char; 128];
    // SAFETY: `layout` is a valid borrowed AVChannelLayout. FFmpeg writes at
    // most `buffer.len()` bytes including the trailing nul, and the buffer is
    // converted into an owned Rust string before returning.
    let written = unsafe {
        ffmpeg::ffi::av_channel_layout_describe(layout, buffer.as_mut_ptr(), buffer.len())
    };
    if written < 0 {
        return Some(format!("{}c", layout.nb_channels));
    }
    // SAFETY: FFmpeg guarantees a nul-terminated C string in `buffer` on
    // success for a non-zero buffer length.
    let description = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    if description.is_empty() {
        None
    } else {
        Some(description)
    }
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

#[cfg(test)]
mod tests {
    use super::{
        FfmpegInputAuthority, FfmpegPacketFactory, FfmpegResourceFactory, ResourceCleanupJob,
        ResourceCleanupPermit, ResourceCleanupPermitPool, ResourceCleanupScheduler,
        ResourceNormalizerSession, ResourceOutputDirGuard, ResourceWorkerShared,
        ResourceWorkerState, acquire_resource_cleanup_permit, classify_ffmpeg_input_authority,
        detect_profile_name, enforce_ffmpeg_input_protocol_policy, load_profile_set,
        notify_if_primary_resource_has_bytes, open_ffmpeg_input, packet_capabilities_from_profiles,
        pixel_format_component_depth, resolve_ffmpeg_open_target, resource_packet_stream_route,
        run_resource_worker_with_panic_guard, set_resource_worker_state, unique_session_suffix,
        validate_packet_profile, video_dimensions_from_parameters, wait_resource_worker_update,
    };
    use ffmpeg_next::util::format::Pixel;
    use player_plugin::{
        SourceNormalizerError, SourceNormalizerOutputRoute, SourceNormalizerPacketMediaKind,
        SourceNormalizerPacketPluginFactory, SourceNormalizerPacketSeek,
        SourceNormalizerPacketSession, SourceNormalizerPacketSessionConfig,
        SourceNormalizerReadPacketStatus, SourceNormalizerResourcePluginFactory,
        SourceNormalizerResourceSession, SourceNormalizerResourceSessionInfo,
        SourceNormalizerResourceSessionState,
    };
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Condvar, Mutex, atomic::AtomicBool, mpsc};
    use std::thread;
    use std::time::{Duration, Instant};

    fn test_resource_worker_shared() -> Arc<ResourceWorkerShared> {
        Arc::new(ResourceWorkerShared {
            state: Mutex::new(ResourceWorkerState {
                state: SourceNormalizerResourceSessionState::Starting,
                message: None,
                tracks: Vec::new(),
                sequence: 0,
                worker_finished: false,
            }),
            changed: Condvar::new(),
        })
    }

    fn test_resource_cleanup_permit() -> ResourceCleanupPermit {
        Arc::new(ResourceCleanupPermitPool {
            available: Mutex::new(1),
            capacity: 1,
        })
        .try_acquire()
        .expect("test cleanup permit")
    }

    fn test_resource_session(
        output_dir: PathBuf,
        shared: Arc<ResourceWorkerShared>,
        cancel_requested: Arc<AtomicBool>,
        worker: thread::JoinHandle<()>,
    ) -> ResourceNormalizerSession {
        ResourceNormalizerSession {
            info: SourceNormalizerResourceSessionInfo {
                session_id: Some("resource-lifecycle-test".to_owned()),
                normalizer_name: Some("player-source-normalizer-ffmpeg".to_owned()),
                runtime_profile: Some("generic-fallback".to_owned()),
                selected_backend: Some("test".to_owned()),
                output_route: SourceNormalizerOutputRoute::Fmp4LocalStream,
                container: "fmp4".to_owned(),
                primary_resource_path: None,
                primary_content_type: None,
                resources: Vec::new(),
                tracks: Vec::new(),
                duration_millis: None,
                seekable: true,
                disk_bytes_used: None,
            },
            output_dir,
            shared,
            observed_sequence: 0,
            cancel_requested,
            worker: Some(worker),
            cleanup_permit: Some(test_resource_cleanup_permit()),
            closed: false,
        }
    }

    fn wait_until_removed(path: &Path) {
        for _ in 0..100 {
            if !path.exists() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "path was not removed after deferred cleanup: {}",
            path.display()
        );
    }

    #[test]
    fn ffmpeg_input_policy_rejects_indirect_and_non_http_network_protocols() {
        for input in [
            "concat:/tmp/first.ts|/tmp/second.ts",
            "subfile,,start,0,end,32,,:/tmp/video.mp4",
            "tcp://example.test:9000",
            r"\\server\share\video.mp4",
        ] {
            let error = classify_ffmpeg_input_authority(input)
                .expect_err("unsafe FFmpeg input should be rejected");
            assert!(matches!(error, SourceNormalizerError::InvalidInput { .. }));
        }
    }

    #[test]
    fn ffmpeg_input_policy_preserves_local_and_http_sources() {
        assert_eq!(
            classify_ffmpeg_input_authority("/tmp/video.mp4").expect("local path"),
            FfmpegInputAuthority::Local
        );
        assert_eq!(
            classify_ffmpeg_input_authority("file:///tmp/video.mp4").expect("file URL"),
            FfmpegInputAuthority::Local
        );
        assert_eq!(
            classify_ffmpeg_input_authority("https://example.test/video.m3u8").expect("HTTPS URL"),
            FfmpegInputAuthority::RemoteHttp
        );
    }

    #[test]
    fn ffmpeg_open_target_decodes_percent_encoded_local_file_urls() {
        assert_eq!(
            resolve_ffmpeg_open_target(
                "file:///Volumes/%E5%96%B5%E5%96%B5%E5%B0%8F%E5%B1%8B/video%20clip.mp4",
                FfmpegInputAuthority::Local,
            )
            .expect("encoded local file URL"),
            "/Volumes/\u{55b5}\u{55b5}\u{5c0f}\u{5c4b}/video clip.mp4"
        );
        assert_eq!(
            resolve_ffmpeg_open_target(
                "https://example.test/video%20clip.mp4",
                FfmpegInputAuthority::RemoteHttp,
            )
            .expect("remote URL"),
            "https://example.test/video%20clip.mp4"
        );
    }

    #[test]
    fn ffmpeg_open_target_rejects_ambiguous_local_file_urls() {
        let error = resolve_ffmpeg_open_target(
            "file:///tmp/video.mp4?alternate=1",
            FfmpegInputAuthority::Local,
        )
        .expect_err("query-bearing file URL should be rejected");

        assert!(matches!(error, SourceNormalizerError::InvalidInput { .. }));
    }

    #[test]
    fn ffmpeg_input_policy_overrides_profile_protocol_widening() {
        let mut options = ffmpeg_next::Dictionary::new();
        options.set("protocol_whitelist", "file,http,https,concat,subfile");
        options.set("protocol_blacklist", "");

        enforce_ffmpeg_input_protocol_policy(&mut options, FfmpegInputAuthority::RemoteHttp);

        assert_eq!(
            options.get("protocol_whitelist"),
            Some("http,https,tcp,tls,crypto")
        );
        assert_eq!(
            options.get("protocol_blacklist"),
            Some("file,concat,subfile")
        );
    }

    #[test]
    fn ffmpeg_open_boundary_rejects_indirect_protocols_and_accepts_local_media() {
        let profiles = load_profile_set().expect("load source normalizer profiles");
        let profile = profiles
            .require("generic-fallback")
            .expect("generic source normalizer profile");

        for input in [
            "concat:/tmp/first.ts|/tmp/second.ts",
            "subfile,,start,0,end,32,,:/tmp/video.mp4",
        ] {
            let result = open_ffmpeg_input(input, profile, Some(1_000), Some(1_000), None);
            assert!(
                matches!(result, Err(SourceNormalizerError::InvalidInput { .. })),
                "unsafe FFmpeg input reached the open boundary: {input}"
            );
        }

        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/media/tiny-h264-aac.m4v")
            .canonicalize()
            .expect("canonical local media fixture");
        let fixture = fixture.to_string_lossy();
        let input = open_ffmpeg_input(&fixture, profile, Some(2_000), Some(2_000), None)
            .expect("local media should remain supported at the FFmpeg open boundary");
        assert!(input.nb_streams() > 0);
    }

    #[test]
    fn dynamic_resource_stream_after_header_is_typed_unsupported() {
        let error = resource_packet_stream_route(&[0], &[ffmpeg_next::Rational(1, 1_000)], 1)
            .expect_err("post-header stream should be rejected");

        assert!(matches!(
            error,
            SourceNormalizerError::UnsupportedOperation { operation }
                if operation.contains("dynamic input stream index 1")
        ));
    }

    #[test]
    fn resource_worker_panic_publishes_failed_terminal_state() {
        let shared = test_resource_worker_shared();

        run_resource_worker_with_panic_guard(shared.clone(), || panic!("test worker panic"));

        let state = super::resource_worker_state(&shared);
        assert_eq!(state.state, SourceNormalizerResourceSessionState::Failed);
        assert!(state.worker_finished);
        assert_eq!(state.message.as_deref(), Some("resource worker panicked"));
    }

    #[test]
    fn packet_capabilities_from_default_profiles_match_safe_factory() {
        let capabilities = packet_capabilities_from_profiles(load_profile_set());

        assert!(
            capabilities
                .supported_runtime_profiles
                .contains(&"generic-fallback".to_owned())
        );
        assert!(
            !capabilities
                .supported_runtime_profiles
                .contains(&"hls-nonstandard".to_owned())
        );
        assert!(
            !capabilities
                .supported_runtime_profiles
                .contains(&"dash-weird".to_owned())
        );
        assert_eq!(
            capabilities.media_kinds,
            vec![
                SourceNormalizerPacketMediaKind::Video,
                SourceNormalizerPacketMediaKind::Audio
            ]
        );
        assert!(capabilities.supports_codec("h264"));
        assert!(capabilities.supports_codec("hevc"));
        assert!(capabilities.supports_codec("av1"));
        assert!(capabilities.supports_seek);
        assert!(capabilities.supports_flush);
        assert_eq!(FfmpegPacketFactory.packet_capabilities(), capabilities);
    }

    #[test]
    fn resource_capabilities_report_64_session_limit() {
        let capabilities = FfmpegResourceFactory.resource_capabilities();

        assert_eq!(capabilities.max_sessions, Some(64));
    }

    #[test]
    fn resource_session_capacity_exhaustion_is_typed() {
        let scheduler =
            ResourceCleanupScheduler::start(1, "vesper-source-normalizer-capacity-error-test")
                .expect("start capacity test scheduler");
        let permit = acquire_resource_cleanup_permit(&scheduler).expect("acquire first permit");

        let error = acquire_resource_cleanup_permit(&scheduler)
            .expect_err("second permit must exceed capacity");
        assert!(matches!(
            error,
            SourceNormalizerError::ResourceExhausted { .. }
        ));

        drop(permit);
        assert!(acquire_resource_cleanup_permit(&scheduler).is_ok());
    }

    #[test]
    fn resource_output_guard_removes_abandoned_directory() {
        let output_dir = std::env::temp_dir().join(format!(
            "vesper-source-normalizer-open-guard-test-{}",
            unique_session_suffix()
        ));
        std::fs::create_dir_all(&output_dir).expect("create guarded output directory");

        {
            let _guard = ResourceOutputDirGuard::new(output_dir.clone());
        }

        assert!(!output_dir.exists());
    }

    #[test]
    fn exports_plugin_root_with_both_source_interfaces() {
        let entry: extern "C" fn() -> *const player_plugin::__private::VesperPluginRoot =
            super::vesper_plugin_entry;
        let root_ptr = entry();
        assert!(!root_ptr.is_null());
        // SAFETY: the generated entry returned one live root owner.
        let root = unsafe { root_ptr.read() };
        assert_eq!(root.interface_count, 2);
        // SAFETY: the generated owner is destroyed exactly once.
        unsafe { root.destroy_owner.expect("destroy owner")(root.owner) };
    }

    #[test]
    fn hdr_metadata_identifies_dolby_vision_codec_tag() {
        let hdr = super::dolby_vision_metadata("HEVC", Some("dvh1".to_owned()))
            .expect("dvh1 tag should be treated as Dolby Vision");

        assert!(hdr.has_rpu);
        assert!(hdr.has_bl);

        let structured = player_plugin::NativeFrameHdrMetadata {
            kind: "dolbyVision".to_owned(),
            mastering_display: None,
            content_light: None,
            dolby_vision: Some(hdr),
        };
        assert!(structured.is_dolby_vision());
    }

    #[test]
    fn hdr_metadata_decodes_dolby_vision_configuration_record() {
        let hdr = super::dolby_vision_metadata_from_dovi_conf(&[1, 0, 8, 6, 1, 0, 1, 1, 0])
            .expect("DOVI config should decode");

        assert_eq!(hdr.profile, Some(8));
        assert_eq!(hdr.level, Some(6));
        assert_eq!(hdr.compatibility_id, Some(1));
        assert!(hdr.has_rpu);
        assert!(hdr.has_bl);
        assert!(!hdr.has_el);
    }

    #[test]
    fn open_rejects_empty_input() {
        let result = open_packet_session(SourceNormalizerPacketSessionConfig {
            runtime_profile: "generic-fallback".to_owned(),
            input: String::new(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        });

        let error = match result {
            Ok(_) => panic!("empty input must be rejected"),
            Err(error) => error,
        };
        assert!(matches!(error, SourceNormalizerError::InvalidInput { .. }));
    }

    #[test]
    fn open_rejects_hls_profile_before_ffmpeg_probe() {
        let result = open_packet_session(SourceNormalizerPacketSessionConfig {
            runtime_profile: "hls-nonstandard".to_owned(),
            input: "https://example.test/master.m3u8".to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        });

        let error = match result {
            Ok(_) => panic!("HLS packet profile must be rejected"),
            Err(error) => error,
        };
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
    fn packet_track_dimensions_read_codec_parameters_without_decoder() {
        let mut parameters = ffmpeg_next::codec::Parameters::new();
        // SAFETY: the test owns the allocated AVCodecParameters and writes plain
        // metadata fields before passing the wrapper to the helper under test.
        unsafe {
            let raw = parameters.as_mut_ptr();
            (*raw).width = 1920;
            (*raw).height = 1080;
        }

        assert_eq!(
            video_dimensions_from_parameters(&parameters),
            (Some(1920), Some(1080))
        );
    }

    #[test]
    fn pixel_format_component_depth_reports_per_component_depth() {
        assert_eq!(
            pixel_format_component_depth(ffmpeg_next::ffi::AVPixelFormat::from(Pixel::RGB24) as i32),
            Some(8)
        );
        assert_eq!(
            pixel_format_component_depth(
                ffmpeg_next::ffi::AVPixelFormat::from(Pixel::YUV420P10) as i32
            ),
            Some(10)
        );
        assert_eq!(
            pixel_format_component_depth(ffmpeg_next::ffi::AVPixelFormat::from(Pixel::None) as i32),
            None
        );
        assert_eq!(pixel_format_component_depth(i32::MAX), None);
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
    fn resource_worker_state_change_wakes_waiter() {
        let shared = test_resource_worker_shared();
        let waiter_shared = shared.clone();
        let waiter = thread::spawn(move || {
            let mut observed_sequence = 0;
            wait_resource_worker_update(&waiter_shared, &mut observed_sequence, 1_000)
        });

        set_resource_worker_state(
            &shared,
            SourceNormalizerResourceSessionState::Running,
            Some("running".to_owned()),
        );

        let status = waiter.join().expect("waiter joins");
        assert!(status.updated);
    }

    #[test]
    fn resource_worker_primary_bytes_wake_waiter_once() {
        let shared = test_resource_worker_shared();
        let directory = std::env::temp_dir().join(format!(
            "vesper-source-normalizer-wait-test-{}",
            unique_session_suffix()
        ));
        std::fs::create_dir_all(&directory).expect("create test directory");
        let primary = directory.join("primary.mp4");
        let waiter_shared = shared.clone();
        let waiter = thread::spawn(move || {
            let mut observed_sequence = 0;
            wait_resource_worker_update(&waiter_shared, &mut observed_sequence, 1_000)
        });

        std::fs::write(&primary, [1u8, 2, 3]).expect("write primary resource");
        let mut notified = false;
        notify_if_primary_resource_has_bytes(&shared, &primary, &mut notified);
        notify_if_primary_resource_has_bytes(&shared, &primary, &mut notified);

        let status = waiter.join().expect("waiter joins");
        assert!(status.updated);
        assert!(notified);
        assert_eq!(super::resource_worker_state(&shared).sequence, 1);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn resource_worker_cancel_wakes_waiter() {
        let shared = test_resource_worker_shared();
        let waiter_shared = shared.clone();
        let waiter = thread::spawn(move || {
            let mut observed_sequence = 0;
            wait_resource_worker_update(&waiter_shared, &mut observed_sequence, 1_000)
        });

        set_resource_worker_state(
            &shared,
            SourceNormalizerResourceSessionState::Cancelled,
            Some("cancelled".to_owned()),
        );

        let status = waiter.join().expect("waiter joins");
        assert!(status.updated);
    }

    #[test]
    fn resource_close_after_cancel_defers_unfinished_worker_cleanup() {
        let shared = test_resource_worker_shared();
        let output_dir = std::env::temp_dir().join(format!(
            "vesper-source-normalizer-cancel-close-test-{}",
            unique_session_suffix()
        ));
        std::fs::create_dir_all(&output_dir).expect("create test output directory");
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let (worker_entered_sender, worker_entered_receiver) = mpsc::channel();
        let (release_worker_sender, release_worker_receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            worker_entered_sender
                .send(())
                .expect("signal worker entered");
            let _ = release_worker_receiver.recv_timeout(Duration::from_secs(5));
        });
        worker_entered_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("worker entered");
        let observed_shared = shared.clone();
        let mut session =
            test_resource_session(output_dir.clone(), shared, cancel_requested, worker);

        let cancel_status = session.cancel().expect("request resource cancellation");
        assert!(!cancel_status.completed);
        let cancel_state = super::resource_worker_state(&observed_shared);
        assert_eq!(
            cancel_state.state,
            SourceNormalizerResourceSessionState::Starting
        );
        assert!(!cancel_state.worker_finished);
        let started_at = Instant::now();
        session.close().expect("defer unfinished worker cleanup");
        let elapsed = started_at.elapsed();
        assert!(
            elapsed < Duration::from_millis(2_500),
            "close blocked for {elapsed:?}"
        );
        assert!(session.closed);
        session.close().expect("close remains idempotent");
        assert!(output_dir.is_dir());

        release_worker_sender
            .send(())
            .expect("release deferred cleanup worker");
        wait_until_removed(&output_dir);
    }

    #[test]
    fn resource_session_drop_cancels_and_defers_cleanup() {
        let shared = test_resource_worker_shared();
        let output_dir = std::env::temp_dir().join(format!(
            "vesper-source-normalizer-drop-test-{}",
            unique_session_suffix()
        ));
        std::fs::create_dir_all(&output_dir).expect("create test output directory");
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let observed_cancel = cancel_requested.clone();
        let (worker_entered_sender, worker_entered_receiver) = mpsc::channel();
        let (release_worker_sender, release_worker_receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            worker_entered_sender
                .send(())
                .expect("signal worker entered");
            let _ = release_worker_receiver.recv_timeout(Duration::from_secs(5));
        });
        worker_entered_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("worker entered");
        let session = test_resource_session(output_dir.clone(), shared, cancel_requested, worker);

        let started_at = Instant::now();
        drop(session);
        let elapsed = started_at.elapsed();

        assert!(
            elapsed < Duration::from_millis(2_500),
            "drop blocked for {elapsed:?}"
        );
        assert!(observed_cancel.load(std::sync::atomic::Ordering::SeqCst));
        assert!(output_dir.is_dir());
        release_worker_sender
            .send(())
            .expect("release deferred cleanup worker");
        wait_until_removed(&output_dir);
    }

    #[test]
    fn resource_cleanup_queue_keeps_output_until_worker_finishes() {
        let output_dir = std::env::temp_dir().join(format!(
            "vesper-source-normalizer-cleanup-fallback-test-{}",
            unique_session_suffix()
        ));
        std::fs::create_dir_all(&output_dir).expect("create test output directory");
        let (worker_entered_sender, worker_entered_receiver) = mpsc::channel();
        let (release_worker_sender, release_worker_receiver) = mpsc::channel();
        let (worker_checked_sender, worker_checked_receiver) = mpsc::channel();
        let worker_output_dir = output_dir.clone();
        let worker = thread::spawn(move || {
            worker_entered_sender
                .send(())
                .expect("signal worker entered");
            let _ = release_worker_receiver.recv_timeout(Duration::from_secs(5));
            assert!(
                worker_output_dir.is_dir(),
                "cleanup must not remove the output directory while its worker is running"
            );
            worker_checked_sender
                .send(())
                .expect("signal worker checked output directory");
        });
        worker_entered_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("worker entered");

        let scheduler =
            ResourceCleanupScheduler::start(1, "vesper-source-normalizer-cleanup-lifetime-test")
                .expect("start cleanup scheduler");
        let permit = scheduler.try_acquire().expect("acquire cleanup permit");
        scheduler
            .try_schedule(ResourceCleanupJob {
                worker,
                output_dir: output_dir.clone(),
                permit,
            })
            .expect("schedule cleanup job");

        assert!(
            output_dir.is_dir(),
            "cleanup queue must retain worker output until join"
        );
        release_worker_sender
            .send(())
            .expect("release cleanup worker");
        worker_checked_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("worker observed its output directory before cleanup");
        wait_until_removed(&output_dir);
    }

    #[test]
    fn resource_cleanup_permits_bound_active_workers() {
        let scheduler =
            ResourceCleanupScheduler::start(1, "vesper-source-normalizer-cleanup-permit-test")
                .expect("start cleanup scheduler");

        let permit = scheduler.try_acquire().expect("acquire first permit");
        assert!(
            scheduler.try_acquire().is_none(),
            "capacity must reject a second active worker"
        );

        drop(permit);
        assert!(
            scheduler.try_acquire().is_some(),
            "permit must return after worker cleanup ownership ends"
        );
    }

    #[test]
    fn resource_cleanup_reaper_does_not_block_behind_unfinished_worker() {
        let scheduler =
            ResourceCleanupScheduler::start(2, "vesper-source-normalizer-cleanup-order-test")
                .expect("start cleanup scheduler");
        let first_dir = std::env::temp_dir().join(format!(
            "vesper-source-normalizer-cleanup-first-test-{}",
            unique_session_suffix()
        ));
        let second_dir = std::env::temp_dir().join(format!(
            "vesper-source-normalizer-cleanup-second-test-{}",
            unique_session_suffix()
        ));
        std::fs::create_dir_all(&first_dir).expect("create first output directory");
        std::fs::create_dir_all(&second_dir).expect("create second output directory");
        let (release_first_sender, release_first_receiver) = mpsc::channel();
        let first_worker = thread::spawn(move || {
            let _ = release_first_receiver.recv_timeout(Duration::from_secs(5));
        });
        let second_worker = thread::spawn(|| {});
        let first_permit = scheduler.try_acquire().expect("acquire first permit");
        let second_permit = scheduler.try_acquire().expect("acquire second permit");

        scheduler
            .try_schedule(ResourceCleanupJob {
                worker: first_worker,
                output_dir: first_dir.clone(),
                permit: first_permit,
            })
            .expect("schedule unfinished worker");
        scheduler
            .try_schedule(ResourceCleanupJob {
                worker: second_worker,
                output_dir: second_dir.clone(),
                permit: second_permit,
            })
            .expect("schedule finished worker");

        for _ in 0..100 {
            if !second_dir.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !second_dir.exists(),
            "finished cleanup job must not wait behind an unfinished worker"
        );
        assert!(
            first_dir.is_dir(),
            "unfinished worker output must remain available"
        );

        release_first_sender
            .send(())
            .expect("release first cleanup worker");
        for _ in 0..100 {
            if !first_dir.exists() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("first output directory was not cleaned after its worker finished");
    }

    #[test]
    fn resource_worker_wait_timeout_reports_no_update() {
        let shared = test_resource_worker_shared();
        let mut observed_sequence = super::resource_worker_state(&shared).sequence;

        let status = wait_resource_worker_update(&shared, &mut observed_sequence, 1);

        assert!(!status.updated);
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

        let mut session = open_packet_session(SourceNormalizerPacketSessionConfig {
            runtime_profile: "generic-fallback".to_owned(),
            input: fixture.to_string_lossy().into_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        })
        .expect("open fixture packet session");
        let stream_info = session.stream_info();
        assert_eq!(
            stream_info.normalizer_name.as_deref(),
            Some("player-source-normalizer-ffmpeg")
        );
        assert!(stream_info.seekable);
        assert!(stream_info.duration_millis.is_some());
        assert!(
            stream_info
                .tracks
                .iter()
                .any(|track| track.media_kind == SourceNormalizerPacketMediaKind::Video)
        );
        let audio_track = stream_info
            .tracks
            .iter()
            .find(|track| track.media_kind == SourceNormalizerPacketMediaKind::Audio)
            .cloned()
            .expect("fixture exposes audio track");
        assert!(audio_track.sample_rate.is_some());
        assert!(audio_track.channels.is_some());
        assert!(audio_track.channel_layout.is_some());
        assert_eq!(
            stream_info
                .selected_track_index
                .and_then(|selected| stream_info
                    .tracks
                    .iter()
                    .find(|track| track.stream_index == selected)
                    .map(|track| track.media_kind)),
            Some(SourceNormalizerPacketMediaKind::Video)
        );

        let mut saw_audio_packet = false;
        let mut saw_video_packet = false;
        for _ in 0..32 {
            let (status, metadata_packet, handle, data_is_empty) = {
                let packet = session.read_packet().expect("read fixture packet");
                (
                    packet.metadata.status,
                    packet.metadata.packet,
                    packet.handle,
                    packet.data.is_empty(),
                )
            };
            if status == SourceNormalizerReadPacketStatus::EndOfStream {
                assert_eq!(handle, 0);
                assert!(data_is_empty);
                break;
            }
            assert_eq!(status, SourceNormalizerReadPacketStatus::Packet);
            assert!(handle > 0);
            assert!(!data_is_empty);
            let metadata_packet = metadata_packet.expect("packet metadata");
            match metadata_packet.media_kind {
                SourceNormalizerPacketMediaKind::Audio => {
                    saw_audio_packet = true;
                    assert_eq!(metadata_packet.stream_index, audio_track.stream_index);
                    assert_eq!(metadata_packet.sample_rate, audio_track.sample_rate);
                    assert_eq!(metadata_packet.channels, audio_track.channels);
                    assert_eq!(metadata_packet.channel_layout, audio_track.channel_layout);
                }
                SourceNormalizerPacketMediaKind::Video => {
                    saw_video_packet = true;
                    assert_eq!(
                        Some(metadata_packet.stream_index),
                        stream_info.selected_track_index
                    );
                }
                SourceNormalizerPacketMediaKind::Subtitle => {}
            }
            session
                .release_packet(handle)
                .expect("release fixture packet");
            if saw_audio_packet && saw_video_packet {
                break;
            }
        }
        assert!(saw_audio_packet);
        assert!(saw_video_packet);

        session.close().expect("close fixture packet session");
        session
            .close()
            .expect("fixture packet close remains idempotent");
    }

    #[test]
    fn fixture_packet_session_mismatched_release_keeps_lease_retryable() {
        let fixture = fixture_path();
        if !fixture.is_file() {
            eprintln!(
                "skipping FFmpeg source normalizer mismatched lease test: {} is unavailable",
                fixture.display()
            );
            return;
        }

        let mut session = open_packet_session(SourceNormalizerPacketSessionConfig {
            runtime_profile: "generic-fallback".to_owned(),
            input: fixture.to_string_lossy().into_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        })
        .expect("open fixture packet session");
        let packet_handle = {
            let packet = session.read_packet().expect("read fixture packet");
            assert!(packet.handle > 0);
            packet.handle
        };

        let mismatch_error = session
            .release_packet(packet_handle.saturating_add(1))
            .expect_err("mismatched handle must be rejected");
        assert!(format!("{mismatch_error}").contains("unknown packet handle"));

        session
            .release_packet(packet_handle)
            .expect("correct handle remains valid for retry");
        session.close().expect("close fixture packet session");
    }

    #[test]
    fn fixture_packet_session_flush_and_seek_clear_outstanding_lease() {
        let fixture = fixture_path();
        if !fixture.is_file() {
            eprintln!(
                "skipping FFmpeg source normalizer lease test: {} is unavailable",
                fixture.display()
            );
            return;
        }

        let mut session = open_packet_session(SourceNormalizerPacketSessionConfig {
            runtime_profile: "generic-fallback".to_owned(),
            input: fixture.to_string_lossy().into_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        })
        .expect("open fixture packet session");
        let packet_handle = {
            let packet = session.read_packet().expect("read fixture packet");
            assert!(packet.handle > 0);
            packet.handle
        };

        let flush_status = session.flush().expect("flush packet session");
        assert!(flush_status.completed);

        let stale_error = session
            .release_packet(packet_handle)
            .expect_err("flush must invalidate the outstanding packet handle");
        assert!(format!("{stale_error}").contains("no packet lease is outstanding"));

        let packet_after_flush_handle = {
            let packet = session
                .read_packet()
                .expect("read packet after flush invalidation");
            assert!(packet.handle > 0);
            packet.handle
        };

        let seek = SourceNormalizerPacketSeek {
            position_millis: 0,
            exact: false,
        };
        let seek_status = session.seek(&seek).expect("seek packet session");
        assert!(seek_status.completed);

        let stale_seek_error = session
            .release_packet(packet_after_flush_handle)
            .expect_err("seek must invalidate the outstanding packet handle");
        assert!(format!("{stale_seek_error}").contains("no packet lease is outstanding"));

        session.close().expect("close fixture packet session");
    }

    #[test]
    fn unique_session_suffix_is_monotonic_enough_for_collisions() {
        let first = unique_session_suffix();
        let second = unique_session_suffix();

        assert_ne!(first, second);
    }

    fn open_packet_session(
        config: SourceNormalizerPacketSessionConfig,
    ) -> Result<Box<dyn SourceNormalizerPacketSession>, SourceNormalizerError> {
        FfmpegPacketFactory.open_packet_session(&config)
    }

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/media/tiny-h264-aac.m4v")
    }
}
