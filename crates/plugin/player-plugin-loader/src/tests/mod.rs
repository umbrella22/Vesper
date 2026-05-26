use super::{
    DecoderPluginCodecSummary, DecoderPluginMatchRequest, LoadedDynamicPlugin,
    PluginCapabilitySummary, PluginDiagnosticRecord, PluginDiagnosticStatus, PluginLoadError,
    PluginRegistry,
};
use player_plugin::{
    AssemblyMode, BenchmarkEvent, BenchmarkEventBatch, BenchmarkSinkReport, BenchmarkSinkStatus,
    CompletedContentFormat, CompletedDownloadInfo, ContentFormatKind, DecoderBitstreamFormat,
    DecoderCapabilities, DecoderCodecCapability, DecoderError, DecoderFrameFormat,
    DecoderMediaKind, DecoderNativeDeviceContext, DecoderNativeDeviceContextKind,
    DecoderNativeFrameMetadata, DecoderNativeFrameReleaseTracking, DecoderNativeHandleKind,
    DecoderNativeRequirements, DecoderOperationStatus, DecoderPacket, DecoderPacketResult,
    DecoderReceiveNativeFrameMetadata, DecoderReceiveNativeFrameOutput, DecoderSessionConfig,
    DecoderSessionInfo, DownloadMetadata, FrameProcessorCapabilities, FrameProcessorError,
    FrameProcessorFrameTimings, FrameProcessorOperationStatus, FrameProcessorReceiveFrameMetadata,
    FrameProcessorReceiveOutput, FrameProcessorSessionConfig, FrameProcessorSessionInfo,
    FrameProcessorSubmitFrame, FrameProcessorSubmitResult, FrameProcessorSubmitStatus, NativeFrame,
    NativeFrameMetadata, NativeFrameReleaseTracking, NativeHandleKind, OutputFormat, PipelineEvent,
    ProcessorCapabilities, ProcessorError, ProcessorOutput, ProcessorProgress,
    SourceNormalizerError, SourceNormalizerNormalizeLevel, SourceNormalizerOperationStatus,
    SourceNormalizerPacket, SourceNormalizerPacketCapabilities, SourceNormalizerPacketMediaKind,
    SourceNormalizerPacketPluginFactory, SourceNormalizerPacketSeek, SourceNormalizerPacketSession,
    SourceNormalizerPacketSessionConfig, SourceNormalizerPacketStreamInfo,
    SourceNormalizerPacketTrackInfo, SourceNormalizerReadPacketMetadata,
    SourceNormalizerReadPacketStatus, SourceNormalizerRequiredCapabilities,
    VESPER_DECODER_PLUGIN_ABI_VERSION_V3, VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
    VESPER_PLUGIN_ABI_VERSION_V2, VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
    VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2, VesperBenchmarkSinkApi,
    VesperDecoderOpenSessionResult, VesperDecoderPluginApiV2,
    VesperDecoderReceiveNativeFrameResult, VesperFrameProcessorOpenSessionResult,
    VesperFrameProcessorPluginApiV1, VesperFrameProcessorReceiveFrameResult,
    VesperPipelineEventHookApi, VesperPluginBytes, VesperPluginDescriptor, VesperPluginKind,
    VesperPluginProcessResult, VesperPluginResultStatus, VesperPostDownloadProcessorApi,
    VesperSourceNormalizerOpenPacketSessionResult, VesperSourceNormalizerPluginApiV2,
    VesperSourceNormalizerReadPacketResult,
};
use std::collections::BTreeMap;
use std::env;
use std::ffi::{c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

static PROCESSOR_NAME: &[u8] = b"fixture-processor\0";
static HOOK_NAME: &[u8] = b"fixture-hook\0";
static SINK_NAME: &[u8] = b"fixture-benchmark-sink\0";
static DECODER_NAME: &[u8] = b"fixture-decoder\0";
static FRAME_PROCESSOR_NAME: &[u8] = b"test-frame-processor\0";
static SOURCE_NORMALIZER_PACKET_NAME: &[u8] = b"test-source-normalizer-packet\0";
static EVENTS: LazyLock<Mutex<Vec<PipelineEvent>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static BENCHMARK_BATCHES: LazyLock<Mutex<Vec<BenchmarkEventBatch>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));
static NATIVE_FRAME_RELEASES: LazyLock<Mutex<Vec<usize>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));
static FRAME_PROCESSOR_RELEASES: LazyLock<Mutex<Vec<usize>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));
static SOURCE_NORMALIZER_PACKET_RELEASES: LazyLock<Mutex<Vec<usize>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));
static FRAME_PROCESSOR_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static SOURCE_NORMALIZER_PACKET_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn frame_processor_test_guard() -> std::sync::MutexGuard<'static, ()> {
    FRAME_PROCESSOR_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn source_normalizer_packet_test_guard() -> std::sync::MutexGuard<'static, ()> {
    SOURCE_NORMALIZER_PACKET_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn reset_source_normalizer_packet_releases() {
    SOURCE_NORMALIZER_PACKET_RELEASES
        .lock()
        .map(|mut releases| releases.clear())
        .unwrap_or_default();
}

fn source_normalizer_packet_releases() -> Vec<usize> {
    SOURCE_NORMALIZER_PACKET_RELEASES
        .lock()
        .map(|releases| releases.clone())
        .unwrap_or_default()
}

fn fixture_source_normalizer_packet_factory() -> Arc<dyn SourceNormalizerPacketPluginFactory> {
    let api = fixture_source_normalizer_packet_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2,
        plugin_kind: VesperPluginKind::SourceNormalizer,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV2).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load source normalizer packet plugin");
    plugin
        .source_normalizer_packet_plugin_factory()
        .expect("packet factory should be available")
}

fn fixture_source_normalizer_packet_session() -> Box<dyn SourceNormalizerPacketSession> {
    fixture_source_normalizer_packet_factory()
        .open_packet_session(&SourceNormalizerPacketSessionConfig {
            runtime_profile: "fixture-packet".to_owned(),
            input: "file:///tmp/input.mp4".to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        })
        .expect("open packet session")
}

#[derive(Default)]
struct RecordingProgress {
    ratios: Mutex<Vec<f32>>,
}

impl RecordingProgress {
    fn ratios(&self) -> Vec<f32> {
        self.ratios
            .lock()
            .map(|ratios| ratios.clone())
            .unwrap_or_default()
    }
}

impl ProcessorProgress for RecordingProgress {
    fn on_progress(&self, ratio: f32) {
        if let Ok(mut ratios) = self.ratios.lock() {
            ratios.push(ratio);
        }
    }
}

fn fixture_processor_api() -> VesperPostDownloadProcessorApi {
    VesperPostDownloadProcessorApi {
        context: std::ptr::null_mut(),
        destroy: None,
        name: Some(fixture_processor_name),
        capabilities_json: Some(fixture_processor_capabilities_json),
        free_bytes: Some(fixture_free_bytes),
        process_json: Some(fixture_processor_process_json),
        assemble_json: Some(fixture_processor_process_json),
    }
}

fn fixture_hook_api() -> VesperPipelineEventHookApi {
    VesperPipelineEventHookApi {
        context: std::ptr::null_mut(),
        destroy: None,
        name: Some(fixture_hook_name),
        on_event_json: Some(fixture_hook_on_event_json),
    }
}

fn fixture_benchmark_sink_api() -> VesperBenchmarkSinkApi {
    VesperBenchmarkSinkApi {
        context: std::ptr::null_mut(),
        destroy: None,
        name: Some(fixture_benchmark_sink_name),
        free_bytes: Some(fixture_free_bytes),
        on_event_batch_json: Some(fixture_benchmark_on_event_batch_json),
        flush_json: Some(fixture_benchmark_flush_json),
    }
}

fn fixture_native_decoder_api() -> VesperDecoderPluginApiV2 {
    VesperDecoderPluginApiV2 {
        context: std::ptr::null_mut(),
        destroy: None,
        name: Some(fixture_decoder_name),
        capabilities_json: Some(fixture_native_decoder_capabilities_json),
        native_requirements_json: Some(fixture_native_decoder_requirements_json),
        free_bytes: Some(fixture_free_bytes),
        open_session_json: Some(fixture_native_decoder_open_session_json),
        send_packet: Some(fixture_decoder_send_packet),
        receive_native_frame: Some(fixture_decoder_receive_native_frame),
        release_native_frame: Some(fixture_decoder_release_native_frame),
        flush_session: Some(fixture_decoder_flush_session),
        close_session: Some(fixture_decoder_close_session),
    }
}

fn fixture_frame_processor_api() -> VesperFrameProcessorPluginApiV1 {
    VesperFrameProcessorPluginApiV1 {
        context: std::ptr::null_mut(),
        destroy: None,
        name: Some(fixture_frame_processor_name),
        capabilities_json: Some(fixture_frame_processor_capabilities_json),
        free_bytes: Some(fixture_free_bytes),
        open_session_json: Some(fixture_frame_processor_open_session_json),
        submit_frame_json: Some(fixture_frame_processor_submit_frame_json),
        receive_frame: Some(fixture_frame_processor_receive_frame),
        release_frame: Some(fixture_frame_processor_release_frame),
        flush_session: Some(fixture_frame_processor_flush_session),
        close_session: Some(fixture_frame_processor_close_session),
    }
}

fn fixture_source_normalizer_packet_api() -> VesperSourceNormalizerPluginApiV2 {
    VesperSourceNormalizerPluginApiV2 {
        context: std::ptr::null_mut(),
        destroy: None,
        name: Some(fixture_source_normalizer_packet_name),
        packet_capabilities_json: Some(fixture_source_normalizer_packet_capabilities_json),
        open_packet_session_json: Some(fixture_source_normalizer_open_packet_session_json),
        read_packet: Some(fixture_source_normalizer_read_packet),
        release_packet: Some(fixture_source_normalizer_release_packet),
        seek_packet_session_json: Some(fixture_source_normalizer_seek_packet_session_json),
        flush_packet_session: Some(fixture_source_normalizer_flush_packet_session),
        close_packet_session: Some(fixture_source_normalizer_close_packet_session),
        free_bytes: Some(fixture_free_bytes),
    }
}

unsafe extern "C" fn fixture_processor_name(_context: *mut c_void) -> *const c_char {
    PROCESSOR_NAME.as_ptr().cast::<c_char>()
}

unsafe extern "C" fn fixture_hook_name(_context: *mut c_void) -> *const c_char {
    HOOK_NAME.as_ptr().cast::<c_char>()
}

unsafe extern "C" fn fixture_benchmark_sink_name(_context: *mut c_void) -> *const c_char {
    SINK_NAME.as_ptr().cast::<c_char>()
}

unsafe extern "C" fn fixture_decoder_name(_context: *mut c_void) -> *const c_char {
    DECODER_NAME.as_ptr().cast::<c_char>()
}

unsafe extern "C" fn fixture_frame_processor_name(_context: *mut c_void) -> *const c_char {
    FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>()
}

unsafe extern "C" fn fixture_source_normalizer_packet_name(_context: *mut c_void) -> *const c_char {
    SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>()
}

unsafe extern "C" fn fixture_benchmark_on_event_batch_json(
    _context: *mut c_void,
    batch_json: *const u8,
    batch_json_len: usize,
) -> VesperPluginProcessResult {
    let batch = decode_fixture_json::<BenchmarkEventBatch>(batch_json, batch_json_len)
        .expect("decode benchmark batch");
    let accepted_events = batch.events.len() as u64;
    if let Ok(mut batches) = BENCHMARK_BATCHES.lock() {
        batches.push(batch);
    }
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: VesperPluginBytes::from_vec(
            serde_json::to_vec(&BenchmarkSinkStatus { accepted_events })
                .expect("serialize benchmark status"),
        ),
    }
}

unsafe extern "C" fn fixture_benchmark_flush_json(
    _context: *mut c_void,
) -> VesperPluginProcessResult {
    let accepted_events = BENCHMARK_BATCHES
        .lock()
        .map(|batches| {
            batches
                .iter()
                .map(|batch| batch.events.len() as u64)
                .sum::<u64>()
        })
        .unwrap_or_default();
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: VesperPluginBytes::from_vec(
            serde_json::to_vec(&BenchmarkSinkReport {
                accepted_events,
                dropped_events: 0,
                plugin_errors: Vec::new(),
            })
            .expect("serialize benchmark report"),
        ),
    }
}

unsafe extern "C" fn fixture_processor_capabilities_json(
    _context: *mut c_void,
) -> VesperPluginBytes {
    let capabilities = ProcessorCapabilities {
        supported_input_formats: vec![ContentFormatKind::SingleFile],
        output_formats: vec![OutputFormat::Mp4],
        supports_cancellation: true,
        supports_assembly: false,
        supported_assembly_modes: Vec::new(),
    };
    let payload = serde_json::to_vec(&capabilities).expect("serialize capabilities");
    VesperPluginBytes::from_vec(payload)
}

unsafe extern "C" fn fixture_processor_process_json(
    _context: *mut c_void,
    input_json: *const u8,
    input_json_len: usize,
    output_path: *const c_char,
    progress: player_plugin::VesperPluginProgressCallbacks,
) -> VesperPluginProcessResult {
    // SAFETY: the fixture passes a valid input buffer for the duration of
    // this synchronous callback.
    let input_json = unsafe { std::slice::from_raw_parts(input_json, input_json_len) };
    let input: CompletedDownloadInfo =
        serde_json::from_slice(input_json).expect("deserialize input");
    assert_eq!(input.asset_id, "asset-a");

    if let Some(on_progress) = progress.on_progress {
        // SAFETY: the host-side fixture keeps `progress.context` alive for
        // the duration of this synchronous call.
        unsafe { on_progress(progress.context, 0.5) };
        // SAFETY: same as above for the second progress update.
        unsafe { on_progress(progress.context, 1.0) };
    }

    // SAFETY: the fixture provides a valid NUL-terminated UTF-8 path.
    let output_path = unsafe { std::ffi::CStr::from_ptr(output_path) }
        .to_str()
        .expect("output path utf8");
    let output = ProcessorOutput::MuxedFile {
        path: PathBuf::from(output_path),
        format: OutputFormat::Mp4,
    };
    let payload = serde_json::to_vec(&output).expect("serialize output");
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: VesperPluginBytes::from_vec(payload),
    }
}

unsafe extern "C" fn fixture_native_decoder_capabilities_json(
    _context: *mut c_void,
) -> VesperPluginBytes {
    let capabilities = DecoderCapabilities {
        codecs: vec![DecoderCodecCapability {
            codec: "fixture-video".to_owned(),
            media_kind: DecoderMediaKind::Video,
            profiles: vec!["baseline".to_owned()],
            output_formats: vec![DecoderFrameFormat::Nv12],
        }],
        supports_hardware_decode: true,
        supports_cpu_video_frames: false,
        supports_audio_frames: false,
        supports_gpu_handles: true,
        supports_flush: true,
        supports_drain: true,
        max_sessions: Some(1),
    };
    VesperPluginBytes::from_vec(serde_json::to_vec(&capabilities).expect("serialize caps"))
}

unsafe extern "C" fn fixture_native_decoder_requirements_json(
    _context: *mut c_void,
) -> VesperPluginBytes {
    let requirements = DecoderNativeRequirements {
        required_device_context_kinds: Vec::new(),
        output_handle_kinds: vec![DecoderNativeHandleKind::IoSurface],
        requires_native_device_context: false,
        accepted_bitstream_formats: vec![DecoderBitstreamFormat::Unknown("fixture".to_owned())],
    };
    VesperPluginBytes::from_vec(
        serde_json::to_vec(&requirements).expect("serialize native requirements"),
    )
}

#[derive(Debug, Default)]
struct FixtureDecoderSession {
    last_pts_us: Option<i64>,
    pending_frame: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
struct FixtureFrameProcessorSession {
    pending_output: Option<NativeFrame>,
    pending_source_frame_id: Option<u64>,
}

struct FixtureSourceNormalizerPacketSession {
    emitted_packet: bool,
    leased_packet: Option<FixtureSourceNormalizerPacketLease>,
    last_seek: Option<u64>,
}

struct FixtureSourceNormalizerPacketLease {
    handle: usize,
    data: Vec<u8>,
}

unsafe extern "C" fn fixture_native_decoder_open_session_json(
    _context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperDecoderOpenSessionResult {
    let config = decode_fixture_json::<DecoderSessionConfig>(config_json, config_json_len);
    let config = match config {
        Ok(config) => config,
        Err(error) => return decoder_open_error(error),
    };
    if config.codec != "fixture-video" || config.media_kind != DecoderMediaKind::Video {
        return decoder_open_error(DecoderError::UnsupportedCodec {
            codec: config.codec,
        });
    }

    let session = Box::into_raw(Box::new(FixtureDecoderSession::default()));
    let selected_hardware_backend = match config.native_device_context.as_ref() {
        Some(DecoderNativeDeviceContext::D3D11Device { device_ptr }) => {
            Some(format!("fixture-native-d3d11-device-{device_ptr}"))
        }
        _ => Some("fixture-native".to_owned()),
    };
    let info = DecoderSessionInfo {
        decoder_name: Some("fixture-decoder".to_owned()),
        selected_hardware_backend,
        output_format: Some(DecoderFrameFormat::Nv12),
    };
    VesperDecoderOpenSessionResult {
        status: VesperPluginResultStatus::Success,
        session: session.cast::<c_void>(),
        payload: VesperPluginBytes::from_vec(serde_json::to_vec(&info).expect("serialize info")),
    }
}

unsafe extern "C" fn fixture_decoder_send_packet(
    _context: *mut c_void,
    session: *mut c_void,
    packet_json: *const u8,
    packet_json_len: usize,
    packet_data: *const u8,
    packet_data_len: usize,
) -> VesperPluginProcessResult {
    // SAFETY: fixture tests pass the session pointer allocated by the
    // matching open-session callback for this ABI table.
    let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
        return decoder_process_error(DecoderError::NotConfigured);
    };
    let packet = match decode_fixture_json::<DecoderPacket>(packet_json, packet_json_len) {
        Ok(packet) => packet,
        Err(error) => return decoder_process_error(error),
    };
    let data = if packet_data.is_null() || packet_data_len == 0 {
        Vec::new()
    } else {
        // SAFETY: the host-side fixture passes a valid packet buffer for the
        // duration of this synchronous callback.
        unsafe { std::slice::from_raw_parts(packet_data, packet_data_len) }.to_vec()
    };
    session.last_pts_us = packet.pts_us;
    session.pending_frame = Some(data);
    let result = DecoderPacketResult { accepted: true };
    decoder_process_success(&result)
}

unsafe extern "C" fn fixture_decoder_receive_native_frame(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperDecoderReceiveNativeFrameResult {
    // SAFETY: fixture tests pass the session pointer allocated by the
    // matching open-session callback for this ABI table.
    let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
        return decoder_native_frame_error(DecoderError::NotConfigured);
    };
    let Some(data) = session.pending_frame.take() else {
        return decoder_native_frame_success(
            &DecoderReceiveNativeFrameMetadata::need_more_input(),
            0,
        );
    };
    let handle = Box::into_raw(Box::new(data)) as usize;
    let metadata = DecoderNativeFrameMetadata {
        media_kind: DecoderMediaKind::Video,
        format: DecoderFrameFormat::Nv12,
        codec: "fixture-video".to_owned(),
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
    decoder_native_frame_success(&DecoderReceiveNativeFrameMetadata::frame(metadata), handle)
}

unsafe extern "C" fn fixture_decoder_receive_null_native_frame(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperDecoderReceiveNativeFrameResult {
    // SAFETY: fixture tests pass the session pointer allocated by the
    // matching open-session callback for this ABI table.
    let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
        return decoder_native_frame_error(DecoderError::NotConfigured);
    };
    if session.pending_frame.take().is_none() {
        return decoder_native_frame_success(
            &DecoderReceiveNativeFrameMetadata::need_more_input(),
            0,
        );
    };
    let metadata = DecoderNativeFrameMetadata {
        media_kind: DecoderMediaKind::Video,
        format: DecoderFrameFormat::Nv12,
        codec: "fixture-video".to_owned(),
        pts_us: session.last_pts_us,
        duration_us: Some(33_333),
        width: 2,
        height: 2,
        coded_width: Some(2),
        coded_height: Some(2),
        visible_rect: None,
        handle_kind: DecoderNativeHandleKind::IoSurface,
        frame_id: None,
        release_tracking: Some(DecoderNativeFrameReleaseTracking {
            frame_id: None,
            requires_release: true,
        }),
    };
    decoder_native_frame_success(&DecoderReceiveNativeFrameMetadata::frame(metadata), 0)
}

unsafe extern "C" fn fixture_decoder_release_native_frame(
    _context: *mut c_void,
    _session: *mut c_void,
    handle_kind: u32,
    handle: usize,
) -> VesperPluginProcessResult {
    if handle_kind != 2 || handle == 0 {
        return decoder_process_error(DecoderError::abi_violation(
            "fixture native frame release received an invalid handle",
        ));
    }
    if let Ok(mut releases) = NATIVE_FRAME_RELEASES.lock() {
        releases.push(handle);
    }
    // SAFETY: the handle was allocated with `Box::into_raw` in this test
    // fixture and is released exactly once here.
    let _ = unsafe { Box::from_raw(handle as *mut Vec<u8>) };
    decoder_process_success(&DecoderOperationStatus { completed: true })
}

unsafe extern "C" fn fixture_decoder_flush_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    // SAFETY: fixture tests pass the session pointer allocated by the
    // matching open-session callback for this ABI table.
    let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
        return decoder_process_error(DecoderError::NotConfigured);
    };
    session.pending_frame = None;
    decoder_process_success(&DecoderOperationStatus { completed: true })
}

unsafe extern "C" fn fixture_decoder_close_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    if session.is_null() {
        return decoder_process_error(DecoderError::NotConfigured);
    }
    // SAFETY: the session pointer was allocated with `Box::into_raw` by
    // the matching open-session callback and close is called once.
    let _ = unsafe { Box::from_raw(session.cast::<FixtureDecoderSession>()) };
    decoder_process_success(&DecoderOperationStatus { completed: true })
}

unsafe extern "C" fn fixture_frame_processor_capabilities_json(
    _context: *mut c_void,
) -> VesperPluginBytes {
    let capabilities = FrameProcessorCapabilities {
        accepted_input_handle_kinds: vec![NativeHandleKind::IoSurface],
        output_handle_kinds: vec![NativeHandleKind::IoSurface],
        supports_video_frames: true,
        supports_in_place_passthrough: true,
        preserves_dimensions: true,
        may_change_dimensions: false,
        preserves_color_metadata: true,
        preserves_hdr_metadata: true,
        supports_flush: true,
        max_sessions: Some(1),
        max_in_flight_frames: Some(1),
    };
    VesperPluginBytes::from_vec(
        serde_json::to_vec(&capabilities).expect("serialize frame processor caps"),
    )
}

unsafe extern "C" fn fixture_frame_processor_open_session_json(
    _context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperFrameProcessorOpenSessionResult {
    let config = match decode_frame_processor_fixture_json::<FrameProcessorSessionConfig>(
        config_json,
        config_json_len,
    ) {
        Ok(config) => config,
        Err(error) => return frame_processor_open_error(error),
    };
    if config.input_metadata.handle_kind != NativeHandleKind::IoSurface {
        return frame_processor_open_error(FrameProcessorError::unsupported_handle(format!(
            "{:?}",
            config.input_metadata.handle_kind
        )));
    }

    let session = Box::into_raw(Box::new(FixtureFrameProcessorSession::default()));
    let info = FrameProcessorSessionInfo {
        processor_name: Some("test-frame-processor".to_owned()),
        selected_backend: Some("fixture-native".to_owned()),
        output_handle_kind: Some(NativeHandleKind::IoSurface),
        max_in_flight_frames: Some(1),
    };
    VesperFrameProcessorOpenSessionResult {
        status: VesperPluginResultStatus::Success,
        session: session.cast::<c_void>(),
        payload: VesperPluginBytes::from_vec(
            serde_json::to_vec(&info).expect("serialize frame processor info"),
        ),
    }
}

unsafe extern "C" fn fixture_frame_processor_submit_frame_json(
    _context: *mut c_void,
    session: *mut c_void,
    submit_json: *const u8,
    submit_json_len: usize,
    handle: usize,
) -> VesperPluginProcessResult {
    let Some(session) = (unsafe { session.cast::<FixtureFrameProcessorSession>().as_mut() }) else {
        return frame_processor_process_error(FrameProcessorError::NotConfigured);
    };
    let submit = match decode_frame_processor_fixture_json::<FrameProcessorSubmitFrame>(
        submit_json,
        submit_json_len,
    ) {
        Ok(submit) => submit,
        Err(error) => return frame_processor_process_error(error),
    };
    if handle == 0 {
        return frame_processor_process_error(FrameProcessorError::abi_violation(
            "fixture frame processor received a null input handle",
        ));
    }
    if session.pending_output.is_some() {
        return frame_processor_process_success(&FrameProcessorSubmitResult {
            status: FrameProcessorSubmitStatus::Backpressure,
            queue_depth: Some(1),
            in_flight_frames: Some(1),
            message: Some("fixture output is still pending".to_owned()),
        });
    }

    let mut output_metadata = submit.metadata.clone();
    let requires_release = submit
        .metadata
        .release_tracking
        .as_ref()
        .is_none_or(|tracking| tracking.requires_release);
    output_metadata.frame_id = if requires_release {
        Some(handle as u64 + 1)
    } else {
        submit.metadata.frame_id
    };
    output_metadata.release_tracking = Some(NativeFrameReleaseTracking {
        frame_id: output_metadata.frame_id,
        requires_release,
    });
    session.pending_source_frame_id = submit.metadata.frame_id;
    let output_handle = if requires_release {
        Box::into_raw(Box::new(vec![handle as u8])) as usize
    } else {
        handle
    };
    session.pending_output = Some(NativeFrame {
        metadata: output_metadata,
        handle: output_handle,
    });
    frame_processor_process_success(&FrameProcessorSubmitResult {
        status: FrameProcessorSubmitStatus::Accepted,
        queue_depth: Some(1),
        in_flight_frames: Some(1),
        message: None,
    })
}

unsafe extern "C" fn fixture_frame_processor_receive_frame(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperFrameProcessorReceiveFrameResult {
    let Some(session) = (unsafe { session.cast::<FixtureFrameProcessorSession>().as_mut() }) else {
        return frame_processor_receive_error(FrameProcessorError::NotConfigured);
    };
    let Some(output) = session.pending_output.take() else {
        return frame_processor_receive_success(&FrameProcessorReceiveFrameMetadata::pending(), 0);
    };
    let mut metadata = FrameProcessorReceiveFrameMetadata::frame(output.metadata.clone());
    metadata.timings = FrameProcessorFrameTimings {
        queue_wait_us: Some(10),
        process_time_us: Some(20),
        submit_to_ready_us: Some(30),
    };
    metadata.source_frame_id = session.pending_source_frame_id.take();
    frame_processor_receive_success(&metadata, output.handle)
}

unsafe extern "C" fn fixture_frame_processor_release_frame(
    _context: *mut c_void,
    _session: *mut c_void,
    handle_kind: u32,
    handle: usize,
) -> VesperPluginProcessResult {
    if handle_kind != 2 || handle == 0 {
        return frame_processor_process_error(FrameProcessorError::abi_violation(
            "fixture frame processor release received an invalid handle",
        ));
    }
    if let Ok(mut releases) = FRAME_PROCESSOR_RELEASES.lock() {
        releases.push(handle);
    }
    // SAFETY: the handle was allocated with `Box::into_raw` in this test
    // fixture and is released exactly once here.
    let _ = unsafe { Box::from_raw(handle as *mut Vec<u8>) };
    frame_processor_process_success(&FrameProcessorOperationStatus { completed: true })
}

unsafe extern "C" fn fixture_frame_processor_flush_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    let Some(session) = (unsafe { session.cast::<FixtureFrameProcessorSession>().as_mut() }) else {
        return frame_processor_process_error(FrameProcessorError::NotConfigured);
    };
    if let Some(output) = session.pending_output.take() {
        // SAFETY: pending fixture outputs are owned by this session and can
        // be reclaimed on flush when the host never received them.
        let _ = unsafe { Box::from_raw(output.handle as *mut Vec<u8>) };
    }
    frame_processor_process_success(&FrameProcessorOperationStatus { completed: true })
}

unsafe extern "C" fn fixture_frame_processor_close_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    if session.is_null() {
        return frame_processor_process_error(FrameProcessorError::NotConfigured);
    }
    // SAFETY: the session pointer was allocated with `Box::into_raw` by
    // the matching open-session callback and close is called once.
    let mut session = unsafe { Box::from_raw(session.cast::<FixtureFrameProcessorSession>()) };
    if let Some(output) = session.pending_output.take() {
        // SAFETY: pending fixture outputs are owned by this session and can
        // be reclaimed on close when the host never received them.
        let _ = unsafe { Box::from_raw(output.handle as *mut Vec<u8>) };
    }
    frame_processor_process_success(&FrameProcessorOperationStatus { completed: true })
}

unsafe extern "C" fn fixture_source_normalizer_packet_capabilities_json(
    _context: *mut c_void,
) -> VesperPluginBytes {
    let capabilities = SourceNormalizerPacketCapabilities {
        supported_runtime_profiles: vec!["fixture-packet".to_owned()],
        max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
        media_kinds: vec![SourceNormalizerPacketMediaKind::Video],
        codecs: vec!["H264".to_owned()],
        bitstream_formats: vec![DecoderBitstreamFormat::Avcc],
        supports_seek: true,
        supports_flush: true,
        required_capabilities: SourceNormalizerRequiredCapabilities::default(),
        max_sessions: Some(1),
    };
    VesperPluginBytes::from_vec(
        serde_json::to_vec(&capabilities).expect("serialize source normalizer packet caps"),
    )
}

unsafe extern "C" fn fixture_source_normalizer_open_packet_session_json(
    _context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperSourceNormalizerOpenPacketSessionResult {
    let config = match decode_source_normalizer_fixture_json::<SourceNormalizerPacketSessionConfig>(
        config_json,
        config_json_len,
    ) {
        Ok(config) => config,
        Err(error) => return source_normalizer_packet_open_error(error),
    };
    if config.input.is_empty() {
        return source_normalizer_packet_open_error(SourceNormalizerError::invalid_input(
            "input must not be empty",
        ));
    }
    if !config
        .runtime_profile
        .eq_ignore_ascii_case("fixture-packet")
    {
        return source_normalizer_packet_open_error(
            SourceNormalizerError::UnsupportedRuntimeProfile {
                profile: config.runtime_profile,
            },
        );
    }
    let info = SourceNormalizerPacketStreamInfo {
        session_id: Some("fixture-packet-session".to_owned()),
        normalizer_name: Some("test-source-normalizer-packet".to_owned()),
        runtime_profile: Some("fixture-packet".to_owned()),
        selected_backend: Some("fixture".to_owned()),
        tracks: vec![SourceNormalizerPacketTrackInfo {
            stream_index: 0,
            media_kind: SourceNormalizerPacketMediaKind::Video,
            codec: "H264".to_owned(),
            extradata: vec![1, 2, 3],
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
        }],
        selected_track_index: Some(0),
        duration_millis: Some(1_000),
        seekable: true,
    };
    let session = Box::into_raw(Box::new(FixtureSourceNormalizerPacketSession {
        emitted_packet: false,
        leased_packet: None,
        last_seek: None,
    }));
    VesperSourceNormalizerOpenPacketSessionResult {
        status: VesperPluginResultStatus::Success,
        session: session.cast::<c_void>(),
        payload: VesperPluginBytes::from_vec(
            serde_json::to_vec(&info).expect("serialize source normalizer packet info"),
        ),
    }
}

unsafe extern "C" fn fixture_source_normalizer_read_packet(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperSourceNormalizerReadPacketResult {
    let Some(session) = (unsafe {
        session
            .cast::<FixtureSourceNormalizerPacketSession>()
            .as_mut()
    }) else {
        return source_normalizer_read_packet_error(SourceNormalizerError::NotConfigured);
    };
    if session.leased_packet.is_some() {
        return source_normalizer_read_packet_error(SourceNormalizerError::abi_violation(
            "previous packet is still leased",
        ));
    }
    if session.emitted_packet {
        return source_normalizer_read_packet_success(
            &SourceNormalizerReadPacketMetadata::end_of_stream(),
            None,
        );
    }

    session.emitted_packet = true;
    let handle = 0x51;
    session.leased_packet = Some(FixtureSourceNormalizerPacketLease {
        handle,
        data: vec![0, 0, 1, 9],
    });
    let packet = session.leased_packet.as_ref().expect("stored packet");
    source_normalizer_read_packet_success(
        &SourceNormalizerReadPacketMetadata::packet(SourceNormalizerPacket {
            pts_us: session
                .last_seek
                .map(|millis| i64::try_from(millis.saturating_mul(1_000)).unwrap_or(i64::MAX))
                .or(Some(1_000)),
            dts_us: Some(1_000),
            duration_us: Some(33_333),
            stream_index: 0,
            key_frame: true,
            discontinuity: session.last_seek.is_some(),
            end_of_stream: false,
        }),
        Some((packet.data.as_ptr(), packet.data.len(), packet.handle)),
    )
}

unsafe extern "C" fn fixture_source_normalizer_release_packet(
    _context: *mut c_void,
    session: *mut c_void,
    packet_handle: usize,
) -> VesperPluginProcessResult {
    let Some(session) = (unsafe {
        session
            .cast::<FixtureSourceNormalizerPacketSession>()
            .as_mut()
    }) else {
        return source_normalizer_process_error(SourceNormalizerError::NotConfigured);
    };
    match session.leased_packet.take() {
        Some(packet) if packet.handle == packet_handle => {
            if let Ok(mut releases) = SOURCE_NORMALIZER_PACKET_RELEASES.lock() {
                releases.push(packet_handle);
            }
            source_normalizer_process_success(&SourceNormalizerOperationStatus {
                completed: true,
                message: None,
            })
        }
        Some(packet) => {
            session.leased_packet = Some(packet);
            source_normalizer_process_error(SourceNormalizerError::abi_violation(
                "unexpected packet handle",
            ))
        }
        None => source_normalizer_process_error(SourceNormalizerError::abi_violation(
            "no packet is leased",
        )),
    }
}

unsafe extern "C" fn fixture_source_normalizer_seek_packet_session_json(
    _context: *mut c_void,
    session: *mut c_void,
    seek_json: *const u8,
    seek_json_len: usize,
) -> VesperPluginProcessResult {
    let Some(session) = (unsafe {
        session
            .cast::<FixtureSourceNormalizerPacketSession>()
            .as_mut()
    }) else {
        return source_normalizer_process_error(SourceNormalizerError::NotConfigured);
    };
    let seek = match decode_source_normalizer_fixture_json::<SourceNormalizerPacketSeek>(
        seek_json,
        seek_json_len,
    ) {
        Ok(seek) => seek,
        Err(error) => return source_normalizer_process_error(error),
    };
    session.leased_packet = None;
    session.emitted_packet = false;
    session.last_seek = Some(seek.position_millis);
    source_normalizer_process_success(&SourceNormalizerOperationStatus {
        completed: true,
        message: None,
    })
}

unsafe extern "C" fn fixture_source_normalizer_flush_packet_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    let Some(session) = (unsafe {
        session
            .cast::<FixtureSourceNormalizerPacketSession>()
            .as_mut()
    }) else {
        return source_normalizer_process_error(SourceNormalizerError::NotConfigured);
    };
    session.leased_packet = None;
    session.emitted_packet = false;
    source_normalizer_process_success(&SourceNormalizerOperationStatus {
        completed: true,
        message: None,
    })
}

unsafe extern "C" fn fixture_source_normalizer_close_packet_session(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperPluginProcessResult {
    if session.is_null() {
        return source_normalizer_process_error(SourceNormalizerError::NotConfigured);
    }
    // SAFETY: the session pointer was allocated with `Box::into_raw` by
    // the matching open-session callback and close is called once.
    let _ = unsafe { Box::from_raw(session.cast::<FixtureSourceNormalizerPacketSession>()) };
    source_normalizer_process_success(&SourceNormalizerOperationStatus {
        completed: true,
        message: None,
    })
}

unsafe extern "C" fn fixture_payload_codec_process_json(
    _context: *mut c_void,
    _input_json: *const u8,
    _input_json_len: usize,
    _output_path: *const c_char,
    _progress: player_plugin::VesperPluginProgressCallbacks,
) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: VesperPluginBytes::from_vec(b"not-json".to_vec()),
    }
}

unsafe extern "C" fn fixture_null_payload_process_json(
    _context: *mut c_void,
    _input_json: *const u8,
    _input_json_len: usize,
    _output_path: *const c_char,
    _progress: player_plugin::VesperPluginProgressCallbacks,
) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: VesperPluginBytes {
            data: std::ptr::null_mut(),
            len: 4,
        },
    }
}

unsafe extern "C" fn fixture_hook_on_event_json(
    _context: *mut c_void,
    event_json: *const u8,
    event_json_len: usize,
) -> bool {
    // SAFETY: the fixture passes a valid event buffer for the duration of
    // this synchronous callback.
    let event_json = unsafe { std::slice::from_raw_parts(event_json, event_json_len) };
    let event: PipelineEvent = serde_json::from_slice(event_json).expect("deserialize event");
    if let Ok(mut events) = EVENTS.lock() {
        events.push(event);
    }
    true
}

fn decode_fixture_json<T: serde::de::DeserializeOwned>(
    data: *const u8,
    len: usize,
) -> Result<T, DecoderError> {
    if data.is_null() && len > 0 {
        return Err(DecoderError::abi_violation(
            "fixture JSON pointer was null with non-zero len",
        ));
    }
    let payload = if data.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: fixture callers pass a valid JSON buffer for the duration
        // of this synchronous callback.
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    serde_json::from_slice(payload).map_err(|error| DecoderError::payload_codec(error.to_string()))
}

fn decoder_open_error(error: DecoderError) -> VesperDecoderOpenSessionResult {
    VesperDecoderOpenSessionResult {
        status: VesperPluginResultStatus::Failure,
        session: std::ptr::null_mut(),
        payload: VesperPluginBytes::from_vec(serde_json::to_vec(&error).expect("serialize error")),
    }
}

fn decoder_process_success<T: serde::Serialize>(value: &T) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: VesperPluginBytes::from_vec(serde_json::to_vec(value).expect("serialize value")),
    }
}

fn decoder_process_error(error: DecoderError) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Failure,
        payload: VesperPluginBytes::from_vec(serde_json::to_vec(&error).expect("serialize error")),
    }
}

fn decoder_native_frame_success(
    metadata: &DecoderReceiveNativeFrameMetadata,
    handle: usize,
) -> VesperDecoderReceiveNativeFrameResult {
    VesperDecoderReceiveNativeFrameResult {
        status: VesperPluginResultStatus::Success,
        metadata: VesperPluginBytes::from_vec(
            serde_json::to_vec(metadata).expect("serialize native frame metadata"),
        ),
        handle,
    }
}

fn decoder_native_frame_error(error: DecoderError) -> VesperDecoderReceiveNativeFrameResult {
    VesperDecoderReceiveNativeFrameResult {
        status: VesperPluginResultStatus::Failure,
        metadata: VesperPluginBytes::from_vec(serde_json::to_vec(&error).expect("serialize error")),
        handle: 0,
    }
}

fn decode_frame_processor_fixture_json<T: serde::de::DeserializeOwned>(
    data: *const u8,
    len: usize,
) -> Result<T, FrameProcessorError> {
    if data.is_null() && len > 0 {
        return Err(FrameProcessorError::abi_violation(
            "fixture frame processor JSON pointer was null with non-zero len",
        ));
    }
    let payload = if data.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: fixture callers pass a valid JSON buffer for the duration
        // of this synchronous callback.
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    serde_json::from_slice(payload)
        .map_err(|error| FrameProcessorError::payload_codec(error.to_string()))
}

fn frame_processor_open_error(error: FrameProcessorError) -> VesperFrameProcessorOpenSessionResult {
    VesperFrameProcessorOpenSessionResult {
        status: VesperPluginResultStatus::Failure,
        session: std::ptr::null_mut(),
        payload: VesperPluginBytes::from_vec(
            serde_json::to_vec(&error).expect("serialize frame processor error"),
        ),
    }
}

fn frame_processor_process_success<T: serde::Serialize>(value: &T) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: VesperPluginBytes::from_vec(
            serde_json::to_vec(value).expect("serialize frame processor value"),
        ),
    }
}

fn frame_processor_process_error(error: FrameProcessorError) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Failure,
        payload: VesperPluginBytes::from_vec(
            serde_json::to_vec(&error).expect("serialize frame processor error"),
        ),
    }
}

fn frame_processor_receive_success(
    metadata: &FrameProcessorReceiveFrameMetadata,
    handle: usize,
) -> VesperFrameProcessorReceiveFrameResult {
    VesperFrameProcessorReceiveFrameResult {
        status: VesperPluginResultStatus::Success,
        metadata: VesperPluginBytes::from_vec(
            serde_json::to_vec(metadata).expect("serialize frame processor metadata"),
        ),
        handle,
    }
}

fn frame_processor_receive_error(
    error: FrameProcessorError,
) -> VesperFrameProcessorReceiveFrameResult {
    VesperFrameProcessorReceiveFrameResult {
        status: VesperPluginResultStatus::Failure,
        metadata: VesperPluginBytes::from_vec(
            serde_json::to_vec(&error).expect("serialize frame processor error"),
        ),
        handle: 0,
    }
}

fn decode_source_normalizer_fixture_json<T: serde::de::DeserializeOwned>(
    data: *const u8,
    len: usize,
) -> Result<T, SourceNormalizerError> {
    if data.is_null() && len > 0 {
        return Err(SourceNormalizerError::abi_violation(
            "fixture source normalizer JSON pointer was null with non-zero len",
        ));
    }
    let payload = if data.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: fixture callers pass a valid JSON buffer for the duration
        // of this synchronous callback.
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    serde_json::from_slice(payload)
        .map_err(|error| SourceNormalizerError::payload_codec(error.to_string()))
}

fn source_normalizer_packet_open_error(
    error: SourceNormalizerError,
) -> VesperSourceNormalizerOpenPacketSessionResult {
    VesperSourceNormalizerOpenPacketSessionResult {
        status: VesperPluginResultStatus::Failure,
        session: std::ptr::null_mut(),
        payload: VesperPluginBytes::from_vec(
            serde_json::to_vec(&error).expect("serialize source normalizer packet error"),
        ),
    }
}

fn source_normalizer_process_success<T: serde::Serialize>(value: &T) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Success,
        payload: VesperPluginBytes::from_vec(
            serde_json::to_vec(value).expect("serialize source normalizer value"),
        ),
    }
}

fn source_normalizer_process_error(error: SourceNormalizerError) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Failure,
        payload: VesperPluginBytes::from_vec(
            serde_json::to_vec(&error).expect("serialize source normalizer error"),
        ),
    }
}

fn source_normalizer_read_packet_success(
    metadata: &SourceNormalizerReadPacketMetadata,
    packet: Option<(*const u8, usize, usize)>,
) -> VesperSourceNormalizerReadPacketResult {
    let (data, data_len, packet_handle) = packet.unwrap_or((std::ptr::null(), 0, 0));
    VesperSourceNormalizerReadPacketResult {
        status: VesperPluginResultStatus::Success,
        metadata: VesperPluginBytes::from_vec(
            serde_json::to_vec(metadata).expect("serialize source normalizer packet metadata"),
        ),
        data,
        data_len,
        packet_handle,
    }
}

fn source_normalizer_read_packet_error(
    error: SourceNormalizerError,
) -> VesperSourceNormalizerReadPacketResult {
    VesperSourceNormalizerReadPacketResult {
        status: VesperPluginResultStatus::Failure,
        metadata: VesperPluginBytes::from_vec(
            serde_json::to_vec(&error).expect("serialize source normalizer packet error"),
        ),
        data: std::ptr::null(),
        data_len: 0,
        packet_handle: 0,
    }
}

fn fixture_native_frame() -> NativeFrame {
    NativeFrame {
        metadata: NativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Nv12,
            codec: "fixture-video".to_owned(),
            pts_us: Some(2_000),
            duration_us: Some(33_333),
            width: 2,
            height: 2,
            coded_width: Some(2),
            coded_height: Some(2),
            visible_rect: None,
            handle_kind: NativeHandleKind::IoSurface,
            frame_id: Some(41),
            release_tracking: Some(NativeFrameReleaseTracking {
                frame_id: Some(41),
                requires_release: true,
            }),
        },
        handle: 0xfeed,
    }
}

unsafe extern "C" fn fixture_free_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
    // SAFETY: the fixture only reclaims buffers it allocated with
    // `VesperPluginBytes::from_vec`.
    let _ = unsafe { payload.into_vec() };
}

fn native_frame_releases() -> Vec<usize> {
    NATIVE_FRAME_RELEASES
        .lock()
        .map(|releases| releases.clone())
        .unwrap_or_default()
}

fn resolve_vesper_remux_ffmpeg_plugin_path() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "environment variable VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH points to missing file `{}`",
            path.display()
        ));
    }

    resolve_plugin_path("vesper_remux_ffmpeg")
}

fn resolve_decoder_fixture_plugin_path() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("VESPER_DECODER_FIXTURE_PLUGIN_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "environment variable VESPER_DECODER_FIXTURE_PLUGIN_PATH points to missing file `{}`",
            path.display()
        ));
    }
    if let Some(paths) = env::var_os("VESPER_DECODER_PLUGIN_PATHS")
        && let Some(path) = env::split_paths(&paths).next()
    {
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "environment variable VESPER_DECODER_PLUGIN_PATHS points to missing file `{}`",
            path.display()
        ));
    }

    resolve_plugin_path("player_decoder_fixture")
}

fn resolve_decoder_videotoolbox_plugin_path() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "environment variable VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH points to missing file `{}`",
            path.display()
        ));
    }

    resolve_plugin_path("player_decoder_videotoolbox")
}

fn resolve_decoder_d3d11_plugin_path() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("VESPER_DECODER_D3D11_PLUGIN_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "environment variable VESPER_DECODER_D3D11_PLUGIN_PATH points to missing file `{}`",
            path.display()
        ));
    }

    resolve_plugin_path("player_decoder_d3d11")
}

fn resolve_frame_processor_diagnostic_plugin_path() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "environment variable VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH points to missing file `{}`",
            path.display()
        ));
    }
    if let Some(paths) = env::var_os("VESPER_FRAME_PROCESSOR_PLUGIN_PATHS")
        && let Some(path) = env::split_paths(&paths).next()
    {
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "environment variable VESPER_FRAME_PROCESSOR_PLUGIN_PATHS points to missing file `{}`",
            path.display()
        ));
    }

    resolve_plugin_path("player_frame_processor_diagnostic")
}

fn resolve_plugin_path(stem: &str) -> Result<PathBuf, String> {
    let workspace_root = workspace_root()?;
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                workspace_root.join(path)
            }
        })
        .unwrap_or_else(|| workspace_root.join("target"));
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_owned());
    let library_name = shared_library_name(stem);
    let candidates = [
        target_dir.join(&profile).join(&library_name),
        target_dir.join(&profile).join("deps").join(&library_name),
        target_dir.join("debug").join(&library_name),
        target_dir.join("debug").join("deps").join(&library_name),
        target_dir.join("release").join(&library_name),
        target_dir.join("release").join("deps").join(&library_name),
    ];

    candidates
            .into_iter()
            .find(|path| path.is_file())
            .ok_or_else(|| {
                format!(
                    "could not find `{library_name}` under `{}`; build the plugin crate first or set the matching plugin path environment variable",
                    target_dir.display()
                )
            })
}

fn shared_library_name(stem: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{stem}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{stem}.dylib")
    } else {
        format!("lib{stem}.so")
    }
}

fn workspace_root() -> Result<PathBuf, String> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .ok_or_else(|| "failed to derive workspace root from CARGO_MANIFEST_DIR".to_owned())
}

#[allow(dead_code)]
unsafe extern "C" fn fixture_error_process_json(
    _context: *mut c_void,
    _input_json: *const u8,
    _input_json_len: usize,
    _output_path: *const c_char,
    _progress: player_plugin::VesperPluginProgressCallbacks,
) -> VesperPluginProcessResult {
    let payload = serde_json::to_vec(&ProcessorError::UnsupportedFormat(
        ContentFormatKind::DashSegments,
    ))
    .expect("serialize error");
    VesperPluginProcessResult {
        status: VesperPluginResultStatus::Failure,
        payload: VesperPluginBytes::from_vec(payload),
    }
}

mod benchmark_pipeline_tests;
mod decoder_tests;
mod frame_processor_tests;
mod post_download_tests;
mod registry_tests;
mod source_normalizer_tests;
