use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{
    ANDROID_NATIVE_PLAYER_RUNTIME_ADAPTER_ID, AndroidExoPlaybackSnapshot, AndroidExoPlaybackState,
    AndroidExoPlayerBridge, AndroidExoPlayerBridgeBindings, AndroidExoPlayerBridgeContext,
    AndroidExoSeekableRange, AndroidExoStateTracker, AndroidHostBridgeSession, AndroidHostCommand,
    AndroidHostCommandSink, AndroidHostEvent, AndroidHostSnapshot, AndroidHostTimelineKind,
    AndroidManagedNativeSession, AndroidNativeCommandSink, AndroidNativeFrameDecoderSink,
    AndroidNativeFramePipelineFrameStatus, AndroidNativeFramePipelineOpenConfig,
    AndroidNativeFramePipelinePacketSource, AndroidNativeFramePipelineProcessedFrame,
    AndroidNativeFramePipelineProfile, AndroidNativeFramePipelineSession,
    AndroidNativeFramePresenterFrame, AndroidNativeFramePresenterProfile,
    AndroidNativeFramePresenterSink, AndroidNativeFramePresenterSubmitResult,
    AndroidNativeFrameProcessorChain, AndroidNativeFrameProcessorOwnedFrame,
    AndroidNativePlayerBridge, AndroidNativePlayerCommand, AndroidNativePlayerProbe,
    AndroidNativePlayerRuntimeAdapterFactory, AndroidNativePlayerSession,
    AndroidNativePlayerSessionBootstrap, AndroidOpaqueHandle,
    android_native_frame_pipeline_frame_json, android_native_frame_pipeline_open_json,
    required_android_decoder_implementation_name,
};
use player_model::MediaSource;
use player_platform_mobile::MobileCommandQueue;
use player_platform_mobile::{
    MobileNativeFramePipelineConfiguration, MobileSourceNormalizerConfiguration,
};
use player_platform_native_frame::{
    NativeFramePipelineError, NativeFrameProcessorReleaseError, NativeFrameProcessorReleaseResult,
};
use player_plugin::{
    DecoderCapabilities, DecoderCodecCapability, DecoderFrameFormat, DecoderMediaKind,
    DecoderNativeDeviceContext, DecoderNativeDeviceContextKind, DecoderNativeFrame,
    DecoderNativeFrameMetadata, DecoderNativeFrameReleaseTracking, DecoderNativeHandleKind,
    DecoderNativeRequirements, DecoderPacket, DecoderPacketResult, DecoderReceiveNativeFrameOutput,
    DecoderReceivePcmFrameOutput, DecoderSessionConfig, DecoderSessionInfo,
    FrameProcessorCapabilities, NativeDecoderPluginFactory, NativeDecoderSession, NativeFrame,
    NativeFrameColorMetadata, NativeFrameHdrMetadata, NativeFramePipelineProfile, NativeHandleKind,
    SourceNormalizerError, SourceNormalizerOperationStatus, SourceNormalizerPacket,
    SourceNormalizerPacketLease, SourceNormalizerPacketMediaKind, SourceNormalizerPacketSeek,
    SourceNormalizerPacketSession, SourceNormalizerPacketStreamInfo,
    SourceNormalizerPacketTrackInfo, SourceNormalizerReadPacketMetadata,
};
use player_plugin::{
    PipelineEvent, PipelineEventHook, PipelineEventHookOutcome, PluginReference, PluginTransport,
};
use player_runtime::{
    DecodedVideoFrame, FrameProcessorMode, MediaAbrMode, MediaAbrPolicy, MediaTrack,
    MediaTrackCatalog, MediaTrackKind, MediaTrackSelection, MediaTrackSelectionSnapshot,
    NativeFramePipelineMode, PlaybackProgress, PlayerError, PlayerErrorCategory, PlayerErrorCode,
    PlayerMediaInfo, PlayerPluginParticipation, PlayerResilienceMetrics, PlayerResult,
    PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterCapabilities,
    PlayerRuntimeAdapterFactory, PlayerRuntimeCommand, PlayerRuntimeCommandResult,
    PlayerRuntimeEvent, PlayerRuntimeOptions, PlayerRuntimeStartup, PlayerSnapshot,
    PlayerTimelineSnapshot, PresentationState, SourceNormalizerMode, SubtitleErrorDetails,
};

struct RecordingPipelineHook {
    event_names: Arc<Mutex<Vec<String>>>,
}

impl PipelineEventHook for RecordingPipelineHook {
    fn on_event(
        &self,
        event: &PipelineEvent,
    ) -> Result<PipelineEventHookOutcome, player_plugin::PipelineEventHookError> {
        self.event_names
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(event.event_name.clone());
        Ok(PipelineEventHookOutcome::accepted())
    }
}
#[test]
fn android_factory_exposes_native_capabilities() {
    let factory = AndroidNativePlayerRuntimeAdapterFactory::default();
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default(),
        )
        .expect("android probe should succeed");

    let capabilities = initializer.capabilities();
    assert_eq!(
        capabilities.adapter_id,
        ANDROID_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
    );
    assert!(capabilities.supports_external_video_surface);
    assert!(capabilities.supports_hardware_decode);
}

#[test]
fn android_frame_processor_config_reports_missing_plugin_diagnostic() {
    let factory = AndroidNativePlayerRuntimeAdapterFactory::default();
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default()
                .with_frame_processor_mode(FrameProcessorMode::DiagnosticsOnly),
        )
        .expect("android probe should succeed");

    let startup = initializer.startup();
    let diagnostic = startup
        .plugin_diagnostics
        .iter()
        .find(|diagnostic| diagnostic.plugin_kind.as_deref() == Some("frame_processor"))
        .expect("frame processor configuration should report a diagnostic");
    assert_eq!(diagnostic.participation, PlayerPluginParticipation::Unknown);
    assert!(
        diagnostic
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("no plugin paths")
    );
}

#[test]
fn android_factory_is_initialize_unsupported_without_bridge() {
    let factory = AndroidNativePlayerRuntimeAdapterFactory::default();
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default(),
        )
        .expect("android probe should succeed");

    let error = match initializer.initialize() {
        Ok(_) => panic!("android initialize should be unsupported without a bridge"),
        Err(error) => error,
    };
    assert_eq!(error.code(), PlayerErrorCode::Unsupported);
}

#[test]
fn android_command_sink_reports_poisoned_queue() {
    let queue = Arc::new(Mutex::new(VecDeque::new()));
    let poison_queue = queue.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poison_queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        panic!("poison android command queue");
    })
    .join();

    let mut sink = AndroidHostCommandSink::new(MobileCommandQueue::from_shared_for_tests(
        "android native",
        queue,
    ));
    let error = sink
        .submit_command(AndroidNativePlayerCommand::Play)
        .expect_err("poisoned queue should be reported");

    assert_eq!(
        error.category(),
        player_runtime::PlayerErrorCategory::Platform
    );
    assert!(error.message().contains("command queue lock poisoned"));
}

#[test]
fn android_factory_can_initialize_with_bridge() {
    let factory =
        AndroidNativePlayerRuntimeAdapterFactory::with_bridge(Arc::new(FakeAndroidBridge));
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default(),
        )
        .expect("android bridge probe should succeed");

    let bootstrap = initializer
        .initialize()
        .expect("android bridge initialize should succeed");
    assert!(bootstrap.initial_frame.is_none());
    assert_eq!(
        bootstrap.runtime.capabilities().backend_family,
        PlayerRuntimeAdapterBackendFamily::NativeAndroid
    );
}

#[test]
fn android_native_frame_pipeline_open_requires_explicit_native_frame_mode() {
    let error = AndroidNativeFramePipelineSession::open(test_native_frame_open_config(
        NativeFramePipelineMode::DiagnosticsOnly,
    ))
    .expect_err("diagnostics-only mode must not open the native-frame session");

    assert_eq!(error.code(), PlayerErrorCode::InvalidArgument);
    assert!(
        error
            .message()
            .contains("must be explicitly preferred or required")
    );
}

#[test]
fn android_native_frame_pipeline_rejects_raw_paths_without_development_policy() {
    let mut config = test_native_frame_open_config(NativeFramePipelineMode::PreferNativeFrame);
    config.source_normalizer.native_plugin_loading_policy =
        player_runtime::NativePluginLoadingPolicy::DenyRawPaths;
    config.native_frame_pipeline.native_plugin_loading_policy =
        player_runtime::NativePluginLoadingPolicy::DenyRawPaths;

    let error = AndroidNativeFramePipelineSession::open(config)
        .expect_err("raw paths must be rejected before any plugin opens");

    assert_eq!(error.code(), PlayerErrorCode::InvalidArgument);
    assert!(
        error
            .message()
            .contains("require explicit development loading policy")
    );
    assert!(!error.message().contains("failed to open plugin library"));
    assert!(!error.message().contains("dlopen"));
}

#[test]
fn android_native_frame_pipeline_selects_component_for_normalized_video_codec() {
    let mut config = test_native_frame_open_config(NativeFramePipelineMode::PreferNativeFrame);
    config.avc_decoder_implementation_name = Some(" c2.test.avc.decoder ".to_owned());

    assert_eq!(
        required_android_decoder_implementation_name(&config, "video/avc1.640028")
            .expect("AVC component should be selected"),
        " c2.test.avc.decoder "
    );
    assert_eq!(
        required_android_decoder_implementation_name(&config, "hev1.1.6.L93.B0")
            .expect("HEVC component should be selected"),
        "c2.test.hevc.decoder"
    );
}

#[test]
fn android_native_frame_pipeline_rejects_missing_hardware_component_before_decoder_open() {
    let mut config = test_native_frame_open_config(NativeFramePipelineMode::PreferNativeFrame);
    config.hevc_decoder_implementation_name = None;

    let error = required_android_decoder_implementation_name(&config, "hvc1")
        .expect_err("missing HEVC component must reject the native-frame route");

    assert_eq!(error.code(), PlayerErrorCode::Unsupported);
    assert!(error.message().contains("host-selected hardware decoder"));
}

#[test]
fn android_native_frame_pipeline_open_reports_missing_decoder_plugin_after_packet_source_open() {
    let config = test_native_frame_open_config(NativeFramePipelineMode::PreferNativeFrame);
    let source = MediaSource::new(config.source_uri.clone());

    let error = AndroidNativeFramePipelineSession::open_with_packet_source(
        config,
        source,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
    )
    .expect_err("missing decoder plugin should fail real decoder sink open");

    assert_eq!(error.code(), PlayerErrorCode::Unsupported);
    assert!(error.message().contains("no native decoder plugin"));
}

#[test]
fn android_native_frame_pipeline_open_records_contract_wire() {
    let session = test_native_frame_session(NativeFramePipelineMode::PreferNativeFrame, None);

    let json = android_native_frame_pipeline_open_json(42, &session).expect("open json");

    assert!(json.contains("\"route\":\"sdkManagedNativeFrame\""));
    assert!(json.contains("\"sourceInput\":\"sourceNormalizerPacket\""));
    assert!(json.contains("\"decoderAdapter\":\"MediaCodec\""));
    assert!(json.contains("\"presenterProfile\":\"SurfaceView\""));
    assert!(json.contains("\"selectedProfile\":\"hostTimedSurface\""));
    assert!(json.contains("\"presenterReady\":false"));
    assert!(json.contains("\"presenterConfigured\":false"));
    assert!(json.contains("\"presenterState\":\"waitingForSurface\""));
    assert!(json.contains("\"surfaceAttached\":false"));
    assert!(json.contains("\"pipelineProfile\":\"media_codec_surface_texture\""));
    assert!(json.contains("\"decoderPluginCount\":1"));
}

#[test]
fn android_native_frame_pipeline_sdk_owned_hardware_buffer_profile_requires_distinct_decoder_capability()
 {
    let factory = TestDecoderFactory {
        state: Arc::new(Mutex::new(TestDecoderFactoryState::default())),
        receive_outputs: VecDeque::new(),
        supports_presentation_release: true,
    };
    let capabilities = factory.capabilities();
    let native_requirements = factory.native_requirements();
    let requirements =
        AndroidNativeFramePipelineProfile::SdkOwnedHardwareBuffer.decoder_requirements("h264");

    let missing = requirements.missing_capabilities(&capabilities, &native_requirements);

    assert!(
        missing
            .iter()
            .any(|item| item.contains("MediaCodecHardwareBuffer")),
        "SDK-owned HardwareBuffer presenter must not reuse the SurfaceTexture decoder contract: {missing:?}"
    );
}

#[test]
fn android_native_frame_pipeline_rejects_hdr_programmable_processing() {
    let mut track = test_video_track();
    track.color = Some(NativeFrameColorMetadata {
        primaries: Some("bt2020".to_owned()),
        transfer: Some("smpte2084".to_owned()),
        matrix: Some("bt2020-ncl".to_owned()),
        range: Some("limited".to_owned()),
        bit_depth: Some(10),
    });
    track.hdr = Some(NativeFrameHdrMetadata {
        kind: "hdr10".to_owned(),
        mastering_display: None,
        content_light: None,
        dolby_vision: None,
    });

    let mut packet_session = TestPacketSession::new(vec![]);
    packet_session.video_track = track;
    let config = test_native_frame_open_config(NativeFramePipelineMode::PreferNativeFrame);
    let source = MediaSource::new(config.source_uri.clone());

    let error = AndroidNativeFramePipelineSession::open_with_packet_source(
        config,
        source,
        Some(test_packet_source_with_session(packet_session)),
    )
    .expect_err("HDR native-frame processing should be rejected before decoder selection");

    assert_eq!(error.code(), PlayerErrorCode::Unsupported);
    assert!(
        error
            .message()
            .contains("hdrProgrammableProcessingNotSupported"),
        "{}",
        error.message()
    );
}

#[test]
fn android_native_frame_processor_metadata_can_describe_explicit_hardware_buffer_hdr_profile() {
    let mut track = test_video_track();
    track.color = Some(NativeFrameColorMetadata {
        primaries: Some("bt2020".to_owned()),
        transfer: Some("smpte2084".to_owned()),
        matrix: Some("bt2020-ncl".to_owned()),
        range: Some("limited".to_owned()),
        bit_depth: Some(10),
    });
    track.hdr = Some(NativeFrameHdrMetadata {
        kind: "hdr10".to_owned(),
        mastering_display: None,
        content_light: None,
        dolby_vision: None,
    });

    let metadata = super::android_frame_processor_input_metadata(
        &track,
        AndroidNativeFramePipelineProfile::SdkOwnedHardwareBuffer,
    );

    assert_eq!(
        metadata.handle_kind,
        NativeHandleKind::MediaCodecHardwareBuffer
    );
    assert_eq!(
        metadata.pipeline_profile,
        Some(NativeFramePipelineProfile::MediaCodecHardwareBuffer)
    );
    assert!(metadata.requires_hdr_preservation());
}

#[test]
fn android_native_frame_processor_capability_gate_requires_hdr_preservation() {
    let mut track = test_video_track();
    track.hdr = Some(NativeFrameHdrMetadata {
        kind: "hlg".to_owned(),
        mastering_display: None,
        content_light: None,
        dolby_vision: None,
    });
    let metadata = super::android_frame_processor_input_metadata(
        &track,
        AndroidNativeFramePipelineProfile::SdkOwnedHardwareBuffer,
    );
    let capabilities = FrameProcessorCapabilities {
        accepted_input_handle_kinds: vec![NativeHandleKind::MediaCodecHardwareBuffer],
        output_handle_kinds: vec![NativeHandleKind::MediaCodecHardwareBuffer],
        accepted_input_pipeline_profiles: vec![
            NativeFramePipelineProfile::MediaCodecHardwareBuffer,
        ],
        output_pipeline_profiles: vec![NativeFramePipelineProfile::MediaCodecHardwareBuffer],
        supports_video_frames: true,
        preserves_color_metadata: true,
        preserves_hdr_metadata: false,
        ..Default::default()
    };

    let error = super::validate_android_frame_processor_capabilities(
        "test-processor",
        &capabilities,
        &metadata,
    )
    .expect_err("HDR processor must preserve HDR metadata");

    assert!(
        error.message().contains("preservesHdrMetadata"),
        "{}",
        error.message()
    );
}

#[test]
fn android_native_frame_pipeline_tracks_presenter_surface_lifecycle() {
    let mut session = test_native_frame_session(NativeFramePipelineMode::PreferNativeFrame, None);

    assert!(!session.presenter_ready());
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");
    let attached = session.status_wire(42, Some("attached".to_owned()));

    assert!(!session.presenter_ready());
    assert!(!attached.presenter_ready);
    assert!(!attached.presenter_configured);
    assert_eq!(attached.presenter_state, "waitingForPresenter");
    assert!(attached.surface_attached);
    assert_eq!(attached.surface_profile, Some("SurfaceView"));
    assert_eq!(attached.counters.presenter_attach_count, 1);

    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("idempotent attach");
    assert_eq!(
        session
            .status_wire(42, None)
            .counters
            .presenter_attach_count,
        1
    );

    session.detach_presenter_surface();
    let detached = session.status_wire(42, Some("detached".to_owned()));
    assert!(!session.presenter_ready());
    assert!(!detached.presenter_ready);
    assert!(!detached.presenter_configured);
    assert_eq!(detached.presenter_state, "waitingForSurface");
    assert!(!detached.surface_attached);
    assert_eq!(detached.surface_profile, None);
    assert_eq!(detached.counters.presenter_detach_count, 1);
}

#[test]
fn android_native_frame_pipeline_waits_for_surface_before_android_native_window_decoder_open() {
    let decoder_open_state = Arc::new(Mutex::new(TestDecoderFactoryState::default()));
    let mut session = test_native_frame_session_with_decoder_open_plan(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_open_plan(
            decoder_open_state.clone(),
            true,
            VecDeque::from([DecoderReceiveNativeFrameOutput::NeedMoreInput]),
        )),
        None,
    );

    let before_surface = session.advance().expect("advance should wait for surface");
    assert_eq!(
        before_surface.status,
        AndroidNativeFramePipelineFrameStatus::Pending
    );
    assert!(
        before_surface
            .message
            .as_deref()
            .is_some_and(|message| message.contains("presenter is waiting"))
    );
    assert!(
        decoder_open_state
            .lock()
            .expect("factory state")
            .opened_configs
            .is_empty()
    );

    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");
    session
        .configure_presenter_sink(test_presenter_sink_with_decoder_context(
            Arc::new(Mutex::new(TestPresenterSinkState::default())),
            Some(0xabc),
        ))
        .expect("configure presenter sink should open decoder");

    let opened_configs = &decoder_open_state
        .lock()
        .expect("factory state")
        .opened_configs;
    assert_eq!(opened_configs.len(), 1);
    assert_eq!(
        opened_configs[0]
            .required_decoder_implementation_name
            .as_deref(),
        Some("c2.test.avc.decoder")
    );
    assert_eq!(
        opened_configs[0]
            .native_device_context
            .as_ref()
            .and_then(DecoderNativeDeviceContext::android_native_window_ptr),
        Some(0xabc)
    );
    assert!(session.presenter_ready());
    assert_eq!(session.status_wire(42, None).presenter_state, "ready");
}

#[test]
fn android_native_frame_pipeline_surface_reattach_reopens_decoder_with_new_window() {
    let decoder_open_state = Arc::new(Mutex::new(TestDecoderFactoryState::default()));
    let mut session = test_native_frame_session_with_decoder_open_plan(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_open_plan(
            decoder_open_state.clone(),
            true,
            VecDeque::from([DecoderReceiveNativeFrameOutput::NeedMoreInput]),
        )),
        None,
    );
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");
    session
        .configure_presenter_sink(test_presenter_sink_with_decoder_context(
            Arc::new(Mutex::new(TestPresenterSinkState::default())),
            Some(0xabc),
        ))
        .expect("first presenter sink should open decoder");

    session.detach_presenter_surface();
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("reattach presenter surface");
    session
        .configure_presenter_sink(test_presenter_sink_with_decoder_context(
            Arc::new(Mutex::new(TestPresenterSinkState::default())),
            Some(0xdef),
        ))
        .expect("second presenter sink should reopen decoder");

    let state = decoder_open_state.lock().expect("factory state");
    assert_eq!(state.close_count, 1);
    assert_eq!(state.opened_configs.len(), 2);
    assert_eq!(
        state.opened_configs[0]
            .native_device_context
            .as_ref()
            .and_then(DecoderNativeDeviceContext::android_native_window_ptr),
        Some(0xabc)
    );
    assert_eq!(
        state.opened_configs[1]
            .native_device_context
            .as_ref()
            .and_then(DecoderNativeDeviceContext::android_native_window_ptr),
        Some(0xdef)
    );
}

#[test]
fn android_native_frame_pipeline_detach_closes_presenter_sink() {
    let presenter_state = Arc::new(Mutex::new(TestPresenterSinkState::default()));
    let mut session = test_native_frame_session(NativeFramePipelineMode::PreferNativeFrame, None);
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");
    session.set_presenter_sink(test_presenter_sink(presenter_state.clone()));
    assert!(!session.presenter_ready());

    session.detach_presenter_surface();

    assert!(!session.presenter_ready());
    assert_eq!(
        presenter_state.lock().expect("presenter state").close_count,
        1
    );
    let status = session.status_wire(42, None);
    assert!(!status.presenter_configured);
    assert_eq!(status.presenter_state, "waitingForSurface");
}

#[test]
fn android_native_frame_pipeline_replacing_presenter_sink_closes_previous_sink() {
    let first_presenter_state = Arc::new(Mutex::new(TestPresenterSinkState::default()));
    let second_presenter_state = Arc::new(Mutex::new(TestPresenterSinkState::default()));
    let mut session = test_native_frame_session(NativeFramePipelineMode::PreferNativeFrame, None);
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");

    session.set_presenter_sink(test_presenter_sink(first_presenter_state.clone()));
    session.set_presenter_sink(test_presenter_sink(second_presenter_state.clone()));

    assert_eq!(
        first_presenter_state
            .lock()
            .expect("first presenter state")
            .close_count,
        1
    );
    assert_eq!(
        second_presenter_state
            .lock()
            .expect("second presenter state")
            .close_count,
        0
    );
    assert!(!session.presenter_ready());
}

#[test]
fn android_native_frame_pipeline_rejects_mismatched_presenter_surface() {
    let mut session = test_native_frame_session(NativeFramePipelineMode::PreferNativeFrame, None);

    let error = session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceTexture)
        .expect_err("TextureView presenter should not attach to SurfaceView pipeline");

    assert_eq!(error.code(), PlayerErrorCode::InvalidArgument);
    assert!(error.message().contains("expected SurfaceView surface"));
    assert!(!session.presenter_ready());
}

#[test]
fn android_native_frame_pipeline_advance_is_pending_without_packet_source() {
    let mut session = test_native_frame_session(NativeFramePipelineMode::PreferNativeFrame, None);

    let result = session.advance().expect("advance should be non-fatal");
    let json =
        android_native_frame_pipeline_frame_json(result, session.status_wire(42, None).counters)
            .expect("frame json");

    assert!(json.contains("\"status\":\"pending\""));
    assert!(json.contains("packet source is not configured"));
}

#[test]
fn android_native_frame_pipeline_queues_source_normalizer_video_packet_before_decoder_handoff() {
    let mut session = test_native_frame_session(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Audio, 0, 64),
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
        ])),
    );

    let result = session.advance().expect("advance should queue packet");

    assert_eq!(
        result.status,
        AndroidNativeFramePipelineFrameStatus::Pending
    );
    assert!(
        result
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("presenter is waiting")
    );
    assert!(session.has_pending_packet());
    assert_eq!(session.pending_packet_data_len(), Some(128));
    assert_eq!(session.pending_packet_stream_index(), Some(1));
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.skipped_audio_packets, 1);
    assert_eq!(counters.source_packets_read, 1);
    assert_eq!(counters.source_packet_bytes, 128);
}

#[test]
fn android_native_frame_pipeline_sends_queued_packet_to_decoder_sink() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState::default()));
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
    );

    let result = session.advance().expect("advance should send packet");

    assert_eq!(
        result.status,
        AndroidNativeFramePipelineFrameStatus::Pending
    );
    assert!(
        result
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("decoder needs more input")
    );
    assert!(!session.has_pending_packet());
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.source_packets_read, 1);
    assert_eq!(counters.source_packet_bytes, 128);
    assert_eq!(counters.decoder_packets_sent, 1);
    assert_eq!(counters.decoder_packet_bytes, 128);
    let state = sink_state.lock().expect("sink state");
    assert_eq!(state.sent_packets.len(), 1);
    assert_eq!(state.sent_packets[0].stream_index, 1);
    assert_eq!(state.sent_packets[0].data_len, 128);
    assert!(state.sent_packets[0].key_frame);
}

#[test]
fn android_native_frame_pipeline_waits_for_presenter_before_decoder_handoff() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState::default()));
    let mut session = test_native_frame_session_with_all_components(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
        None,
        None,
    );
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");

    let result = session
        .advance()
        .expect("advance should wait for presenter");

    assert_eq!(
        result.status,
        AndroidNativeFramePipelineFrameStatus::Pending
    );
    assert!(
        result
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("presenter is waiting")
    );
    assert!(session.has_pending_packet());
    assert!(
        sink_state
            .lock()
            .expect("sink state")
            .sent_packets
            .is_empty()
    );
    let status = session.status_wire(42, None);
    assert!(!status.presenter_ready);
    assert!(!status.presenter_configured);
    assert_eq!(status.presenter_state, "waitingForPresenter");
}

#[test]
fn android_native_frame_pipeline_detach_releases_host_timed_pending_frame() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
            test_decoder_native_frame(71, 188_000),
        )]),
        ..TestDecoderSinkState::default()
    }));
    let presenter_state = Arc::new(Mutex::new(TestPresenterSinkState {
        requires_host_release: true,
        ..TestPresenterSinkState::default()
    }));
    let mut session = test_native_frame_session_with_all_components(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
        None,
        Some(test_presenter_sink(presenter_state.clone())),
    );
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");
    let result = session
        .advance()
        .expect("advance should produce host-timed frame");
    assert_eq!(result.status, AndroidNativeFramePipelineFrameStatus::Frame);
    assert!(result.requires_host_release);
    assert_eq!(session.pending_frame_count(), 1);

    session.detach_presenter_surface();

    assert_eq!(session.pending_frame_count(), 0);
    assert_eq!(
        sink_state.lock().expect("sink state").released_frames,
        vec![(71, false)]
    );
    assert_eq!(
        presenter_state.lock().expect("presenter state").close_count,
        1
    );
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.decoded_frames, 1);
    assert_eq!(counters.released_frames, 1);
    assert_eq!(counters.presented_frames, 0);
}

#[test]
fn android_native_frame_pipeline_receives_decoder_native_frame() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
            test_decoder_native_frame(7, 44_000),
        )]),
        ..TestDecoderSinkState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
    );

    let result = session.advance().expect("advance should receive a frame");

    assert_eq!(result.status, AndroidNativeFramePipelineFrameStatus::Frame);
    assert_eq!(result.handle, Some(1));
    let frame = result.frame.expect("frame payload");
    assert_eq!(frame.handle, 7);
    assert_eq!(frame.presentation_time_us, 44_000);
    assert_eq!(frame.duration_us, Some(33_333));
    assert_eq!(frame.width, 1_920);
    assert_eq!(frame.height, 1_080);
    assert_eq!(frame.frame_id, Some(7));
    assert_eq!(session.pending_frame_count(), 1);

    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.source_packets_read, 1);
    assert_eq!(counters.decoder_packets_sent, 1);
    assert_eq!(counters.decoded_frames, 1);
    assert_eq!(sink_state.lock().expect("sink state").receive_count, 1);
}

#[test]
fn android_native_frame_pipeline_presenter_accepts_and_releases_frame() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
            test_decoder_native_frame(81, 366_000),
        )]),
        ..TestDecoderSinkState::default()
    }));
    let presenter_state = Arc::new(Mutex::new(TestPresenterSinkState::default()));
    let mut session = test_native_frame_session_with_all_components(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
        None,
        Some(test_presenter_sink(presenter_state.clone())),
    );
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");

    let result = session.advance().expect("advance should present frame");

    assert_eq!(
        result.status,
        AndroidNativeFramePipelineFrameStatus::Presented
    );
    assert_eq!(session.pending_frame_count(), 0);
    assert_eq!(
        presenter_state
            .lock()
            .expect("presenter state")
            .submitted_handles,
        vec![1]
    );
    assert_eq!(
        sink_state.lock().expect("sink state").released_frames,
        vec![(81, true)]
    );
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.presenter_submit_count, 1);
    assert_eq!(counters.presented_frames, 1);
    assert_eq!(counters.released_frames, 1);
}

#[test]
fn android_native_frame_pipeline_presenter_can_defer_release_to_host_clock() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
            test_decoder_native_frame(82, 566_000),
        )]),
        ..TestDecoderSinkState::default()
    }));
    let presenter_state = Arc::new(Mutex::new(TestPresenterSinkState {
        requires_host_release: true,
        ..TestPresenterSinkState::default()
    }));
    let mut session = test_native_frame_session_with_all_components(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
        None,
        Some(test_presenter_sink(presenter_state.clone())),
    );
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");

    let result = session
        .advance()
        .expect("advance should hand frame to host");

    assert_eq!(result.status, AndroidNativeFramePipelineFrameStatus::Frame);
    assert!(result.requires_host_release);
    assert_eq!(result.handle, Some(1));
    assert_eq!(session.pending_frame_count(), 1);
    assert!(
        sink_state
            .lock()
            .expect("sink state")
            .released_frames
            .is_empty()
    );

    session
        .release_frame(1, true)
        .expect("host-timed release should present");

    assert_eq!(session.pending_frame_count(), 0);
    assert_eq!(
        sink_state.lock().expect("sink state").released_frames,
        vec![(82, true)]
    );
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.presenter_submit_count, 1);
    assert_eq!(counters.presented_frames, 1);
    assert_eq!(counters.released_frames, 1);
}

#[test]
fn android_native_frame_pipeline_presenter_backpressure_discards_frame() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
            test_decoder_native_frame(91, 466_000),
        )]),
        ..TestDecoderSinkState::default()
    }));
    let presenter_state = Arc::new(Mutex::new(TestPresenterSinkState {
        accept_frames: false,
        ..TestPresenterSinkState::default()
    }));
    let mut session = test_native_frame_session_with_all_components(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
        None,
        Some(test_presenter_sink(presenter_state.clone())),
    );
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");

    let result = session.advance().expect("advance should discard frame");

    assert_eq!(
        result.status,
        AndroidNativeFramePipelineFrameStatus::Pending
    );
    assert_eq!(result.handle, None);
    assert_eq!(session.pending_frame_count(), 0);
    assert_eq!(
        sink_state.lock().expect("sink state").released_frames,
        vec![(91, false)]
    );
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.presenter_submit_count, 0);
    assert_eq!(counters.presenter_backpressure_count, 1);
    assert_eq!(counters.backpressure_count, 1);
    assert_eq!(counters.released_frames, 1);
    assert_eq!(counters.presented_frames, 0);
}

#[test]
fn android_native_frame_pipeline_release_frame_releases_decoder_native_frame() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
            test_decoder_native_frame(11, 66_000),
        )]),
        ..TestDecoderSinkState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
    );
    let result = session.advance().expect("advance should receive a frame");
    let handle = result.handle.expect("pending frame handle");

    session
        .release_frame(handle, true)
        .expect("pending decoder frame should release");

    assert_eq!(session.pending_frame_count(), 0);
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.released_frames, 1);
    assert_eq!(counters.presented_frames, 1);
    assert_eq!(
        sink_state.lock().expect("sink state").released_frames,
        vec![(11, true)]
    );
}

#[test]
fn android_native_frame_pipeline_release_frame_forwards_presented_flag_to_decoder_sink() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([
            DecoderReceiveNativeFrameOutput::Frame(test_decoder_native_frame(12, 99_000)),
            DecoderReceiveNativeFrameOutput::Frame(test_decoder_native_frame(13, 132_000)),
        ]),
        ..TestDecoderSinkState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
        ])),
        Some(test_decoder_sink(sink_state.clone())),
    );

    let first = session
        .advance()
        .expect("first advance should receive a frame")
        .handle
        .expect("first pending frame handle");
    session
        .release_frame(first, false)
        .expect("discarded decoder frame should release");

    let second = session
        .advance()
        .expect("second advance should receive a frame")
        .handle
        .expect("second pending frame handle");
    session
        .release_frame(second, true)
        .expect("presented decoder frame should release");

    assert_eq!(
        sink_state.lock().expect("sink state").released_frames,
        vec![(12, false), (13, true)]
    );
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.released_frames, 2);
    assert_eq!(counters.presented_frames, 1);
}

#[test]
fn android_native_frame_pipeline_release_frame_failure_does_not_double_release_processor_outputs() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
            test_decoder_native_frame(41, 166_000),
        )]),
        ..TestDecoderSinkState::default()
    }));
    let processor_state = Arc::new(Mutex::new(TestProcessorChainState::default()));
    let mut session = test_native_frame_session_with_decoder_sink_and_processor_chain(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
        Some(test_processor_chain(processor_state.clone())),
    );
    let handle = session
        .advance()
        .expect("advance should process frame")
        .handle
        .expect("pending frame handle");

    // Simulate a transient decoder-frame release failure. The shared release
    // order releases processor outputs first, then keeps the decoder frame
    // pending for retry without double-releasing the processor output.
    sink_state.lock().expect("sink state").release_error =
        Some("transient decoder release failure".to_owned());
    let error = session
        .release_frame(handle, true)
        .expect_err("decoder release failure should surface as an error");
    assert_eq!(error.code(), PlayerErrorCode::DecodeFailure);
    assert_eq!(session.pending_frame_count(), 1);
    assert_eq!(
        processor_state
            .lock()
            .expect("processor state")
            .released_outputs
            .len(),
        1
    );
    assert!(
        sink_state
            .lock()
            .expect("sink state")
            .released_frames
            .is_empty()
    );

    // Clear the failure and retry: the decoder frame releases and the already
    // released processor output is not released again.
    sink_state.lock().expect("sink state").release_error = None;
    session
        .release_frame(handle, true)
        .expect("retry should release the pending frame");
    assert_eq!(session.pending_frame_count(), 0);
    assert_eq!(
        sink_state.lock().expect("sink state").released_frames,
        vec![(41, true)]
    );
    assert_eq!(
        processor_state
            .lock()
            .expect("processor state")
            .released_outputs
            .len(),
        1,
        "processor outputs must not be double-released after decoder retry"
    );
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.released_frames, 1);
    assert_eq!(counters.presented_frames, 1);
}

#[test]
fn android_native_frame_pipeline_processor_release_failure_does_not_increment_release_counters() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
            test_decoder_native_frame(42, 177_000),
        )]),
        ..TestDecoderSinkState::default()
    }));
    let processor_state = Arc::new(Mutex::new(TestProcessorChainState::default()));
    let mut session = test_native_frame_session_with_decoder_sink_and_processor_chain(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
        Some(test_processor_chain(processor_state.clone())),
    );
    let handle = session
        .advance()
        .expect("advance should process frame")
        .handle
        .expect("pending frame handle");
    processor_state
        .lock()
        .expect("processor state")
        .fail_release = Some("processor release failed".to_owned());

    let error = session
        .release_frame(handle, true)
        .expect_err("processor release failure should surface");

    assert_eq!(error.code(), PlayerErrorCode::DecodeFailure);
    assert!(
        sink_state
            .lock()
            .expect("sink state")
            .released_frames
            .is_empty()
    );
    assert_eq!(session.pending_frame_count(), 1);
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.released_frames, 0);
    assert_eq!(counters.presented_frames, 0);
}

#[test]
fn android_native_frame_pipeline_retries_decoder_open_after_transient_failure() {
    let decoder_open_state = Arc::new(Mutex::new(TestDecoderFactoryState {
        remaining_open_failures: 1,
        ..TestDecoderFactoryState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_open_plan(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
        ])),
        Some(test_decoder_open_plan(
            decoder_open_state.clone(),
            true,
            VecDeque::from([DecoderReceiveNativeFrameOutput::NeedMoreInput]),
        )),
        None,
    );
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");
    session.set_presenter_sink(test_presenter_sink_with_decoder_context(
        Arc::new(Mutex::new(TestPresenterSinkState::default())),
        Some(0xabc),
    ));

    // First advance hits the transient open failure and propagates the error,
    // but the open plan must be retained for a retry.
    session
        .advance()
        .expect_err("transient decoder open failure should surface");
    assert!(!session.presenter_ready());
    assert!(
        decoder_open_state
            .lock()
            .expect("factory state")
            .opened_configs
            .is_empty()
    );

    // Second advance retries the open and succeeds.
    session
        .advance()
        .expect("decoder open should succeed on retry");
    assert!(session.presenter_ready());
    assert_eq!(
        decoder_open_state
            .lock()
            .expect("factory state")
            .opened_configs
            .len(),
        1
    );
}

#[test]
fn android_native_frame_pipeline_does_not_reuse_frame_handles_across_flush() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([
            DecoderReceiveNativeFrameOutput::Frame(test_decoder_native_frame(51, 166_000)),
            DecoderReceiveNativeFrameOutput::Frame(test_decoder_native_frame(52, 199_000)),
        ]),
        ..TestDecoderSinkState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
        ])),
        Some(test_decoder_sink(sink_state.clone())),
    );

    let first = session
        .advance()
        .expect("first advance should receive a frame")
        .handle
        .expect("first pending frame handle");

    // Flushing releases the pending map. A reused handle would let a stale host
    // handle alias the next frame (ABA), so the handle must keep increasing.
    session.flush().expect("flush should succeed");
    assert_eq!(session.pending_frame_count(), 0);

    let second = session
        .advance()
        .expect("second advance should receive a frame")
        .handle
        .expect("second pending frame handle");

    assert_ne!(
        first, second,
        "frame handles must not be reused after a flush"
    );
    assert!(second > first, "frame handles must increase monotonically");
}

#[test]
fn android_native_frame_pipeline_processes_decoder_frame_before_presenting() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
            test_decoder_native_frame(31, 166_000),
        )]),
        ..TestDecoderSinkState::default()
    }));
    let processor_state = Arc::new(Mutex::new(TestProcessorChainState::default()));
    let mut session = test_native_frame_session_with_decoder_sink_and_processor_chain(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
        Some(test_processor_chain(processor_state.clone())),
    );

    let result = session.advance().expect("advance should process frame");

    assert_eq!(result.status, AndroidNativeFramePipelineFrameStatus::Frame);
    let handle = result.handle.expect("pending frame handle");
    let frame = result.frame.expect("processed frame payload");
    assert_eq!(frame.handle, 10_031);
    assert_eq!(frame.presentation_time_us, 166_000);
    assert_eq!(frame.frame_id, Some(10_031));
    assert_eq!(
        processor_state
            .lock()
            .expect("processor state")
            .processed_profiles,
        vec![NativeFramePipelineProfile::MediaCodecSurfaceTexture]
    );
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.decoded_frames, 1);
    assert_eq!(counters.processed_frames, 1);

    session
        .release_frame(handle, true)
        .expect("processed frame should release");

    assert_eq!(
        sink_state.lock().expect("sink state").released_frames,
        vec![(31, true)]
    );
    assert_eq!(
        processor_state
            .lock()
            .expect("processor state")
            .released_outputs,
        vec![10_031]
    );
}

#[test]
fn android_native_frame_pipeline_flush_and_seek_release_processor_outputs() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([
            DecoderReceiveNativeFrameOutput::Frame(test_decoder_native_frame(41, 200_000)),
            DecoderReceiveNativeFrameOutput::Frame(test_decoder_native_frame(42, 233_333)),
        ]),
        ..TestDecoderSinkState::default()
    }));
    let processor_state = Arc::new(Mutex::new(TestProcessorChainState::default()));
    let mut session = test_native_frame_session_with_decoder_sink_and_processor_chain(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
        ])),
        Some(test_decoder_sink(sink_state.clone())),
        Some(test_processor_chain(processor_state.clone())),
    );

    session.advance().expect("first processed frame");
    session.flush().expect("flush should succeed");
    assert_eq!(
        processor_state
            .lock()
            .expect("processor state")
            .released_outputs,
        vec![10_041]
    );

    session.advance().expect("second processed frame");
    session
        .seek(Duration::from_millis(500))
        .expect("seek should succeed");
    let processor_state = processor_state.lock().expect("processor state");
    assert_eq!(processor_state.released_outputs, vec![10_041, 10_042]);
    assert_eq!(processor_state.flush_count, 2);
    assert_eq!(
        sink_state.lock().expect("sink state").released_frames,
        vec![(41, false), (42, false)]
    );
}

#[test]
fn android_native_frame_pipeline_processor_failure_releases_decoder_frame() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
            test_decoder_native_frame(51, 266_000),
        )]),
        ..TestDecoderSinkState::default()
    }));
    let processor_state = Arc::new(Mutex::new(TestProcessorChainState {
        fail_process: Some("processor exploded".to_owned()),
        ..TestProcessorChainState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_sink_and_processor_chain(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
        Some(test_processor_chain(processor_state)),
    );

    let error = session
        .advance()
        .expect_err("processor error should fail advance");

    assert_eq!(error.code(), PlayerErrorCode::DecodeFailure);
    assert!(error.message().contains("processor exploded"));
    assert_eq!(session.pending_frame_count(), 0);
    assert_eq!(
        sink_state.lock().expect("sink state").released_frames,
        vec![(51, false)]
    );
}

#[test]
fn android_native_frame_pipeline_flush_and_seek_release_pending_decoder_frames() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([
            DecoderReceiveNativeFrameOutput::Frame(test_decoder_native_frame(21, 100_000)),
            DecoderReceiveNativeFrameOutput::Frame(test_decoder_native_frame(22, 133_333)),
        ]),
        ..TestDecoderSinkState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
        ])),
        Some(test_decoder_sink(sink_state.clone())),
    );

    session.advance().expect("first frame");
    assert_eq!(session.pending_frame_count(), 1);
    session.flush().expect("flush should succeed");
    assert_eq!(session.pending_frame_count(), 0);
    assert_eq!(
        sink_state.lock().expect("sink state").released_frames,
        vec![(21, false)]
    );

    session.advance().expect("second frame");
    assert_eq!(session.pending_frame_count(), 1);
    session
        .seek(Duration::from_millis(250))
        .expect("seek should succeed");
    assert_eq!(session.pending_frame_count(), 0);
    assert_eq!(
        sink_state.lock().expect("sink state").released_frames,
        vec![(21, false), (22, false)]
    );
    assert_eq!(session.status_wire(42, None).counters.released_frames, 2);
}

#[test]
fn android_native_frame_pipeline_decoder_eof_marks_end_of_stream() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Eof]),
        ..TestDecoderSinkState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state)),
    );

    let result = session
        .advance()
        .expect("advance should surface decoder eof");

    assert_eq!(
        result.status,
        AndroidNativeFramePipelineFrameStatus::EndOfStream
    );
    assert!(session.status_wire(42, None).end_of_stream);
}

#[test]
fn android_native_frame_pipeline_keeps_pending_packet_when_decoder_backpressures() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        accept_packets: false,
        ..TestDecoderSinkState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            96,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
    );

    let first = session
        .advance()
        .expect("advance should observe backpressure");

    assert_eq!(first.status, AndroidNativeFramePipelineFrameStatus::Pending);
    assert!(
        first
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("did not accept")
    );
    assert!(session.has_pending_packet());
    assert_eq!(session.pending_packet_data_len(), Some(96));
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.decoder_packets_sent, 0);
    assert_eq!(counters.backpressure_count, 1);
    assert_eq!(counters.decoder_backpressure_count, 1);
    assert_eq!(sink_state.lock().expect("sink state").sent_packets.len(), 1);

    sink_state.lock().expect("sink state").accept_packets = true;
    let second = session
        .advance()
        .expect("advance should retry pending packet");

    assert_eq!(
        second.status,
        AndroidNativeFramePipelineFrameStatus::Pending
    );
    assert!(!session.has_pending_packet());
    let counters = session.status_wire(42, None).counters;
    assert_eq!(counters.decoder_packets_sent, 1);
    assert_eq!(counters.decoder_backpressure_count, 1);
    assert_eq!(sink_state.lock().expect("sink state").sent_packets.len(), 2);
}

#[test]
fn android_native_frame_pipeline_decoder_send_error_is_decode_failure() {
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            64,
        )])),
        Some(test_decoder_sink(Arc::new(Mutex::new(
            TestDecoderSinkState {
                send_error: Some("simulated send failure".to_owned()),
                ..TestDecoderSinkState::default()
            },
        )))),
    );

    let error = session
        .advance()
        .expect_err("decoder send error should fail advance");

    assert_eq!(error.code(), PlayerErrorCode::DecodeFailure);
    assert!(error.message().contains("simulated send failure"));
    assert!(session.has_pending_packet());
}

#[test]
fn android_native_frame_pipeline_flush_and_seek_flush_decoder_sink() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        accept_packets: false,
        ..TestDecoderSinkState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            80,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
    );

    session
        .advance()
        .expect("advance should retain backpressured packet");
    assert!(session.has_pending_packet());

    session.flush().expect("flush should succeed");
    assert!(!session.has_pending_packet());
    assert_eq!(session.status_wire(42, None).counters.flush_count, 1);
    assert_eq!(sink_state.lock().expect("sink state").flush_count, 1);

    session
        .seek(Duration::from_millis(250))
        .expect("seek should succeed");
    assert!(!session.has_pending_packet());
    assert_eq!(session.status_wire(42, None).counters.seek_count, 1);
    assert_eq!(sink_state.lock().expect("sink state").flush_count, 2);
}

#[test]
fn android_native_frame_pipeline_flush_failure_is_reported_to_host() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        flush_error: Some("simulated decoder flush failure".to_owned()),
        ..TestDecoderSinkState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![])),
        Some(test_decoder_sink(sink_state.clone())),
    );

    let error = session
        .flush()
        .expect_err("decoder flush failure should be propagated");

    assert_eq!(error.code(), PlayerErrorCode::DecodeFailure);
    assert!(error.message().contains("simulated decoder flush failure"));
    assert_eq!(sink_state.lock().expect("sink state").flush_count, 1);
    assert_eq!(session.status_wire(42, None).counters.flush_count, 0);
}

#[test]
fn android_native_frame_pipeline_flush_reports_pending_decoder_release_failure() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
            test_decoder_native_frame(61, 300_000),
        )]),
        ..TestDecoderSinkState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
    );

    session.advance().expect("frame should become pending");
    sink_state.lock().expect("sink state").release_error =
        Some("simulated decoder release failure".to_owned());

    let error = session
        .flush()
        .expect_err("pending frame release failure should propagate");

    assert_eq!(error.code(), PlayerErrorCode::DecodeFailure);
    assert!(
        error
            .message()
            .contains("simulated decoder release failure")
    );
    assert_eq!(session.pending_frame_count(), 1);
    assert_eq!(session.status_wire(42, None).counters.released_frames, 0);
    assert_eq!(session.status_wire(42, None).counters.flush_count, 0);
}

#[test]
fn android_native_frame_pipeline_seek_failure_is_reported_to_host() {
    let mut packet_session = TestPacketSession::new(Vec::new());
    packet_session.seek_error = Some("simulated packet seek failure".to_owned());
    let mut session = test_native_frame_session(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source_with_session(packet_session)),
    );

    let error = session
        .seek(Duration::from_millis(250))
        .expect_err("packet seek failure should be propagated");

    assert_eq!(error.code(), PlayerErrorCode::DecodeFailure);
    assert!(error.message().contains("simulated packet seek failure"));
    assert_eq!(session.status_wire(42, None).counters.seek_count, 0);
}

#[test]
fn android_native_frame_pipeline_seek_reports_pending_processor_release_failure() {
    let sink_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([DecoderReceiveNativeFrameOutput::Frame(
            test_decoder_native_frame(62, 333_333),
        )]),
        ..TestDecoderSinkState::default()
    }));
    let processor_state = Arc::new(Mutex::new(TestProcessorChainState::default()));
    let mut session = test_native_frame_session_with_decoder_sink_and_processor_chain(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
        Some(test_decoder_sink(sink_state.clone())),
        Some(test_processor_chain(processor_state.clone())),
    );

    session
        .advance()
        .expect("processed frame should become pending");
    processor_state
        .lock()
        .expect("processor state")
        .fail_release = Some("simulated processor release failure".to_owned());

    let error = session
        .seek(Duration::from_millis(250))
        .expect_err("pending processor output release failure should propagate");

    assert_eq!(error.code(), PlayerErrorCode::DecodeFailure);
    assert!(
        error
            .message()
            .contains("simulated processor release failure")
    );
    assert_eq!(session.pending_frame_count(), 1);
    assert_eq!(session.status_wire(42, None).counters.released_frames, 0);
    assert_eq!(session.status_wire(42, None).counters.seek_count, 0);
}

#[test]
fn android_native_frame_pipeline_packet_need_more_data_stays_pending_without_queue() {
    let mut session = test_native_frame_session(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::need_more_data(
            "waiting for moof",
        )])),
    );

    let result = session.advance().expect("advance should stay pending");

    assert_eq!(
        result.status,
        AndroidNativeFramePipelineFrameStatus::Pending
    );
    assert_eq!(
        result.message.as_deref(),
        Some("source normalizer packet source needs more data")
    );
    assert!(!session.has_pending_packet());
    assert_eq!(
        session.status_wire(42, None).counters.source_packets_read,
        0
    );
}

#[test]
fn android_native_frame_pipeline_packet_eos_marks_end_of_stream() {
    let mut session = test_native_frame_session(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::end_of_stream()])),
    );

    let result = session.advance().expect("advance should report eos");

    assert_eq!(
        result.status,
        AndroidNativeFramePipelineFrameStatus::EndOfStream
    );
    assert!(session.status_wire(42, None).end_of_stream);
}

#[test]
fn android_native_frame_pipeline_seek_clears_packet_end_of_stream() {
    let mut session = test_native_frame_session(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::end_of_stream()])),
    );

    let result = session.advance().expect("advance should report eos");
    assert_eq!(
        result.status,
        AndroidNativeFramePipelineFrameStatus::EndOfStream
    );
    assert!(session.status_wire(42, None).end_of_stream);

    session
        .seek(Duration::ZERO)
        .expect("seek after eos should succeed");

    assert!(
        !session
            .status_wire(42, Some("seeked".to_owned()))
            .end_of_stream
    );
    assert_eq!(session.status_wire(42, None).counters.seek_count, 1);
}

#[test]
fn android_native_frame_pipeline_flush_and_seek_clear_pending_packet_source() {
    let mut session = test_native_frame_session(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![TestPacketRead::packet(
            SourceNormalizerPacketMediaKind::Video,
            1,
            128,
        )])),
    );

    session.advance().expect("advance should queue packet");
    assert!(session.has_pending_packet());

    session.flush().expect("flush should succeed");
    assert!(!session.has_pending_packet());
    assert_eq!(session.status_wire(42, None).counters.flush_count, 1);

    session
        .seek(Duration::from_millis(250))
        .expect("seek should succeed");
    assert!(!session.has_pending_packet());
    assert_eq!(session.status_wire(42, None).counters.seek_count, 1);
}

#[test]
fn android_native_frame_pipeline_release_flush_and_seek_clear_pending_frames() {
    let decoder_state = Arc::new(Mutex::new(TestDecoderSinkState {
        receive_outputs: VecDeque::from([
            DecoderReceiveNativeFrameOutput::Frame(test_decoder_native_frame(100, 100)),
            DecoderReceiveNativeFrameOutput::Frame(test_decoder_native_frame(200, 200)),
            DecoderReceiveNativeFrameOutput::Frame(test_decoder_native_frame(300, 300)),
        ]),
        ..TestDecoderSinkState::default()
    }));
    let mut session = test_native_frame_session_with_decoder_sink(
        NativeFramePipelineMode::PreferNativeFrame,
        Some(test_packet_source(vec![
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
            TestPacketRead::packet(SourceNormalizerPacketMediaKind::Video, 1, 128),
        ])),
        Some(test_decoder_sink(decoder_state)),
    );
    let first = session
        .advance()
        .expect("first frame")
        .handle
        .expect("first handle");
    let second = session
        .advance()
        .expect("second frame")
        .handle
        .expect("second handle");

    session
        .release_frame(first, true)
        .expect("known frame should release");
    assert_eq!(session.pending_frame_count(), 1);
    assert_eq!(session.status_wire(42, None).counters.presented_frames, 1);

    session.flush().expect("flush should succeed");
    assert_eq!(session.pending_frame_count(), 0);
    assert_eq!(session.status_wire(42, None).counters.flush_count, 1);
    assert_eq!(session.status_wire(42, None).counters.released_frames, 2);

    let third = session
        .advance()
        .expect("third frame")
        .handle
        .expect("third handle");
    assert_ne!(second, third);
    session
        .seek(Duration::from_millis(500))
        .expect("seek should succeed");
    assert_eq!(session.pending_frame_count(), 0);
    assert_eq!(session.status_wire(42, None).counters.seek_count, 1);
    assert_eq!(session.status_wire(42, None).counters.released_frames, 3);
}

#[test]
fn android_native_frame_pipeline_rejects_unknown_frame_release_handle() {
    let mut session = test_native_frame_session(NativeFramePipelineMode::PreferNativeFrame, None);

    let error = session
        .release_frame(99, false)
        .expect_err("unknown frame handle should fail");

    assert_eq!(error.code(), PlayerErrorCode::DecodeFailure);
}

#[test]
fn android_state_tracker_maps_ready_pause_and_end() {
    let mut tracker = AndroidExoStateTracker::default();

    let ready = tracker.observe(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: false,
        playback_rate: 1.0,
        position: Duration::ZERO,
        duration: Some(Duration::from_secs(12)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(12),
        }),
        live_edge: None,
    });
    assert_eq!(ready.presentation_state, PresentationState::Ready);
    assert_eq!(ready.emitted_events.len(), 1);

    let playing = tracker.observe(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.0,
        position: Duration::from_secs(1),
        duration: Some(Duration::from_secs(12)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(12),
        }),
        live_edge: None,
    });
    assert_eq!(playing.presentation_state, PresentationState::Playing);

    let paused = tracker.observe(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: false,
        playback_rate: 1.0,
        position: Duration::from_secs(3),
        duration: Some(Duration::from_secs(12)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(12),
        }),
        live_edge: None,
    });
    assert_eq!(paused.presentation_state, PresentationState::Paused);

    let finished = tracker.observe(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ended,
        play_when_ready: false,
        playback_rate: 1.0,
        position: Duration::from_secs(12),
        duration: Some(Duration::from_secs(12)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(12),
        }),
        live_edge: None,
    });
    assert_eq!(finished.presentation_state, PresentationState::Finished);
    assert!(
        finished
            .emitted_events
            .iter()
            .any(|event| matches!(event, player_runtime::PlayerRuntimeEvent::Ended))
    );
}

#[test]
fn android_state_tracker_reports_playback_rate_changes() {
    let mut tracker = AndroidExoStateTracker::default();

    let first = tracker.observe(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: false,
        playback_rate: 1.0,
        position: Duration::ZERO,
        duration: None,
        is_live: false,
        is_seekable: false,
        seekable_range: None,
        live_edge: None,
    });
    assert!(first.emitted_events.iter().all(|event| !matches!(
        event,
        player_runtime::PlayerRuntimeEvent::PlaybackRateChanged { .. }
    )));

    let second = tracker.observe(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.5,
        position: Duration::from_millis(500),
        duration: None,
        is_live: false,
        is_seekable: false,
        seekable_range: None,
        live_edge: None,
    });
    assert_eq!(second.playback_rate, 1.5);
    assert!(second.emitted_events.iter().any(|event| matches!(
        event,
        player_runtime::PlayerRuntimeEvent::PlaybackRateChanged { rate }
        if (*rate - 1.5).abs() < f32::EPSILON
    )));
}

#[test]
fn android_managed_session_replays_from_start_when_finished() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands.clone());
    let mut session = AndroidManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

    session.apply_snapshot(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ended,
        play_when_ready: false,
        playback_rate: 1.0,
        position: Duration::from_secs(9),
        duration: Some(Duration::from_secs(9)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(9),
        }),
        live_edge: None,
    });

    let result = session
        .dispatch(PlayerRuntimeCommand::Play)
        .expect("play from finished should be bridged");

    assert!(result.applied);
    assert_eq!(result.snapshot.state, PresentationState::Playing);
    assert_eq!(
        *commands.lock().expect("commands lock"),
        vec![
            AndroidNativePlayerCommand::SeekTo {
                position: Duration::ZERO,
            },
            AndroidNativePlayerCommand::Play,
        ]
    );
}

#[test]
fn android_managed_session_validates_pause_and_playback_rate() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands.clone());
    let mut session = AndroidManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

    let pause_error = session
        .dispatch(PlayerRuntimeCommand::Pause)
        .expect_err("pause before play should be invalid");
    assert_eq!(pause_error.code(), PlayerErrorCode::InvalidState);

    let rate_error = session
        .dispatch(PlayerRuntimeCommand::SetPlaybackRate { rate: 4.0 })
        .expect_err("out-of-range playback rate should fail");
    assert_eq!(rate_error.code(), PlayerErrorCode::InvalidArgument);
    assert!(commands.lock().expect("commands lock").is_empty());
}

#[test]
fn android_managed_session_updates_from_native_snapshot() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands);
    let mut session = AndroidManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

    session.apply_snapshot(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.25,
        position: Duration::from_millis(750),
        duration: Some(Duration::from_secs(5)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(5),
        }),
        live_edge: None,
    });

    assert_eq!(session.presentation_state(), PresentationState::Playing);
    assert!((session.playback_rate() - 1.25).abs() < f32::EPSILON);
    assert_eq!(session.progress().position(), Duration::from_millis(750));
    let events = session.drain_events();
    assert!(events.iter().any(|event| matches!(
        event,
        player_runtime::PlayerRuntimeEvent::PlaybackRateChanged { rate }
        if (*rate - 1.25).abs() < f32::EPSILON
    )));
}

#[test]
fn android_managed_session_sample_uses_media_info_duration_without_mutating_state() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands);
    let session = AndroidManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);
    let snapshot = AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.0,
        position: Duration::from_secs(3),
        duration: None,
        is_live: false,
        is_seekable: true,
        seekable_range: None,
        live_edge: None,
    };

    let sampled = session.sample_timeline(&snapshot);

    assert_eq!(sampled.kind, player_runtime::PlayerTimelineKind::Vod);
    assert_eq!(sampled.position, Duration::from_secs(3));
    assert_eq!(sampled.duration, Some(Duration::from_secs(12)));
    assert_eq!(
        sampled.seekable_range.expect("VOD range").end,
        Duration::from_secs(12)
    );
    assert_eq!(session.pending_update_count(), 0);
}

#[test]
fn android_managed_session_sample_preserves_live_dvr_coordinates() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands);
    let session =
        AndroidManagedNativeSession::new("https://example.com/live.m3u8", test_media_info(), sink);
    let snapshot = AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.0,
        position: Duration::from_secs(84),
        duration: None,
        is_live: true,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::from_secs(60),
            end: Duration::from_secs(120),
        }),
        live_edge: Some(Duration::from_secs(120)),
    };

    let sampled = session.sample_timeline(&snapshot);

    assert_eq!(sampled.kind, player_runtime::PlayerTimelineKind::LiveDvr);
    assert_eq!(sampled.position, Duration::from_secs(84));
    assert_eq!(sampled.duration, Some(Duration::from_secs(60)));
    assert_eq!(
        sampled.seekable_range.expect("DVR range").start,
        Duration::from_secs(60)
    );
    assert_eq!(sampled.live_edge, Some(Duration::from_secs(120)));
    assert_eq!(session.pending_update_count(), 0);
}

#[test]
fn android_managed_session_controller_delivers_async_updates() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands);
    let (mut session, controller) =
        AndroidManagedNativeSession::with_controller("placeholder.mp4", test_media_info(), sink);

    controller.apply_snapshot(AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.5,
        position: Duration::from_secs(2),
        duration: Some(Duration::from_secs(12)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(12),
        }),
        live_edge: None,
    });
    controller.report_seek_completed(Duration::from_secs(3));
    controller.report_retry_scheduled(2, Duration::from_millis(1_500));
    controller.report_error(PlayerErrorCode::BackendFailure, "bridge callback failed");

    let events = session.drain_events();
    assert_eq!(session.presentation_state(), PresentationState::Playing);
    assert!((session.playback_rate() - 1.5).abs() < f32::EPSILON);
    assert_eq!(session.progress().position(), Duration::from_secs(3));
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::SeekCompleted { position } if *position == Duration::from_secs(3)
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::RetryScheduled { attempt: 2, delay }
        if *delay == Duration::from_millis(1_500)
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::Error(error)
        if error.code() == PlayerErrorCode::BackendFailure
    )));
    assert_eq!(session.snapshot().resilience_metrics.retry_count, 2);
}

#[test]
fn android_managed_session_controller_delivers_media_info_updates() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands);
    let (mut session, controller) = AndroidManagedNativeSession::with_controller(
        "https://example.com/master.m3u8",
        test_media_info(),
        sink,
    );

    let track_catalog = MediaTrackCatalog {
        tracks: vec![
            MediaTrack {
                id: "video-720p".to_owned(),
                kind: MediaTrackKind::Video,
                label: Some("720p".to_owned()),
                language: None,
                codec: Some("avc1.64001f".to_owned()),
                bit_rate: Some(2_000_000),
                width: Some(1280),
                height: Some(720),
                frame_rate: Some(30.0),
                channels: None,
                sample_rate: None,
                is_default: true,
                is_forced: false,
            },
            MediaTrack {
                id: "audio-en".to_owned(),
                kind: MediaTrackKind::Audio,
                label: Some("English".to_owned()),
                language: Some("en".to_owned()),
                codec: Some("mp4a.40.2".to_owned()),
                bit_rate: Some(128_000),
                width: None,
                height: None,
                frame_rate: None,
                channels: Some(2),
                sample_rate: Some(48_000),
                is_default: true,
                is_forced: false,
            },
        ],
        adaptive_video: true,
        adaptive_audio: false,
    };
    let track_selection = MediaTrackSelectionSnapshot {
        video: MediaTrackSelection::track("video-720p"),
        audio: MediaTrackSelection::track("audio-en"),
        subtitle: MediaTrackSelection::disabled(),
        abr_policy: MediaAbrPolicy {
            mode: MediaAbrMode::FixedTrack,
            track_id: Some("video-720p".to_owned()),
            max_bit_rate: None,
            max_width: None,
            max_height: None,
        },
    };

    controller.report_media_info(track_catalog.clone(), track_selection.clone());

    let events = session.drain_events();
    assert_eq!(session.media_info().track_catalog, track_catalog);
    assert_eq!(session.media_info().track_selection, track_selection);
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::MetadataReady(media_info)
        if media_info.track_catalog == track_catalog
            && media_info.track_selection == track_selection
    )));
}

#[test]
fn android_managed_session_dispatches_video_track_selection() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands.clone());
    let mut session = AndroidManagedNativeSession::new(
        "https://example.com/master.m3u8",
        test_media_info_with_tracks(),
        sink,
    );

    let result = session
        .dispatch(PlayerRuntimeCommand::SetVideoTrackSelection {
            selection: MediaTrackSelection::track("video-720p"),
        })
        .expect("video track selection should dispatch");

    assert!(result.applied);
    assert_eq!(
        session.media_info().track_selection.video,
        MediaTrackSelection::track("video-720p"),
    );
    assert_eq!(
        session.media_info().track_selection.abr_policy,
        MediaAbrPolicy {
            mode: MediaAbrMode::FixedTrack,
            track_id: Some("video-720p".to_owned()),
            max_bit_rate: None,
            max_width: None,
            max_height: None,
        },
    );
    assert_eq!(
        *commands.lock().expect("commands lock"),
        vec![AndroidNativePlayerCommand::SetVideoTrackSelection {
            selection: MediaTrackSelection::track("video-720p"),
        }],
    );
    let events = session.drain_events();
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::MetadataReady(media_info)
        if media_info.track_selection.video == MediaTrackSelection::track("video-720p")
            && media_info.track_selection.abr_policy.mode == MediaAbrMode::FixedTrack
    )));
}

#[test]
fn android_managed_session_dispatches_constrained_abr_policy() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands.clone());
    let mut session = AndroidManagedNativeSession::new(
        "https://example.com/master.m3u8",
        test_media_info_with_tracks(),
        sink,
    );

    let policy = MediaAbrPolicy {
        mode: MediaAbrMode::Constrained,
        track_id: None,
        max_bit_rate: Some(1_000_000),
        max_width: Some(960),
        max_height: Some(540),
    };
    let result = session
        .dispatch(PlayerRuntimeCommand::SetAbrPolicy {
            policy: policy.clone(),
        })
        .expect("constrained ABR should dispatch");

    assert!(result.applied);
    assert_eq!(session.media_info().track_selection.abr_policy, policy);
    assert_eq!(
        *commands.lock().expect("commands lock"),
        vec![AndroidNativePlayerCommand::SetAbrPolicy {
            policy: policy.clone(),
        }],
    );
    let events = session.drain_events();
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::MetadataReady(media_info)
        if media_info.track_selection.abr_policy == policy
    )));
}

#[test]
fn android_managed_session_rejects_unknown_video_track_selection() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands);
    let mut session = AndroidManagedNativeSession::new(
        "https://example.com/master.m3u8",
        test_media_info_with_tracks(),
        sink,
    );

    let error = session
        .dispatch(PlayerRuntimeCommand::SetVideoTrackSelection {
            selection: MediaTrackSelection::track("missing-video"),
        })
        .expect_err("missing video track should fail");

    assert_eq!(error.code(), PlayerErrorCode::InvalidArgument);
}

#[test]
fn android_exoplayer_bridge_bindings_can_initialize_managed_session() {
    let bridge = AndroidExoPlayerBridge::new(
        AndroidExoPlayerBridgeContext {
            java_vm: AndroidOpaqueHandle(1),
            exo_player: AndroidOpaqueHandle(2),
            video_surface: None,
        },
        Arc::new(FakeAndroidExoBindings::default()),
    );
    let factory = AndroidNativePlayerRuntimeAdapterFactory::with_bridge(Arc::new(bridge));
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default(),
        )
        .expect("android exo bridge probe should succeed");

    let bootstrap = initializer
        .initialize()
        .expect("android exo bridge initialize should succeed");
    assert!(bootstrap.initial_frame.is_none());
    assert_eq!(
        bootstrap.runtime.capabilities().backend_family,
        PlayerRuntimeAdapterBackendFamily::NativeAndroid
    );
}

#[test]
fn android_host_snapshot_conversion_preserves_timeline_shape() {
    let snapshot = PlayerSnapshot {
        source_uri: "placeholder.mp4".to_owned(),
        state: PresentationState::Playing,
        has_video_surface: true,
        is_interrupted: false,
        is_buffering: true,
        playback_rate: 1.5,
        progress: PlaybackProgress::new(Duration::from_secs(5), Some(Duration::from_secs(20))),
        timeline: PlayerTimelineSnapshot::vod(
            PlaybackProgress::new(Duration::from_secs(5), Some(Duration::from_secs(20))),
            true,
        ),
        media_info: test_media_info(),
        resilience_metrics: PlayerResilienceMetrics::default(),
    };

    let host = AndroidHostSnapshot::from_player_snapshot(&snapshot);
    assert_eq!(host.playback_state, PresentationState::Playing);
    assert!(host.is_buffering);
    assert_eq!(host.position_ms, 5_000);
    assert_eq!(host.duration_ms, Some(20_000));
    assert_eq!(host.seekable_range.expect("seekable range").end_ms, 20_000);
}

#[test]
fn android_host_snapshot_conversion_uses_effective_live_edge_for_live_dvr() {
    let snapshot = PlayerSnapshot {
        source_uri: "https://example.com/live.m3u8".to_owned(),
        state: PresentationState::Playing,
        has_video_surface: true,
        is_interrupted: false,
        is_buffering: false,
        playback_rate: 1.0,
        progress: PlaybackProgress::new(Duration::from_secs(84), None),
        timeline: PlayerTimelineSnapshot::live_dvr(
            PlaybackProgress::new(Duration::from_secs(84), None),
            player_runtime::PlayerSeekableRange {
                start: Duration::ZERO,
                end: Duration::from_secs(120),
            },
            None,
        ),
        media_info: test_media_info(),
        resilience_metrics: PlayerResilienceMetrics::default(),
    };

    let host = AndroidHostSnapshot::from_player_snapshot(&snapshot);
    assert_eq!(host.timeline_kind, AndroidHostTimelineKind::LiveDvr);
    assert_eq!(host.live_edge_ms, Some(120_000));
    assert_eq!(host.position_ms, 84_000);
}

#[test]
fn android_host_event_conversion_maps_runtime_events() {
    let rate = AndroidHostEvent::from_runtime_event(&PlayerRuntimeEvent::PlaybackRateChanged {
        rate: 1.25,
    });
    assert!(matches!(
        rate,
        Some(AndroidHostEvent::PlaybackRateChanged { rate })
        if (rate - 1.25).abs() < f32::EPSILON
    ));

    let seek = AndroidHostEvent::from_runtime_event(&PlayerRuntimeEvent::SeekCompleted {
        position: Duration::from_millis(1250),
    });
    assert!(matches!(
        seek,
        Some(AndroidHostEvent::SeekCompleted { position_ms: 1250 })
    ));

    let retry = AndroidHostEvent::from_runtime_event(&PlayerRuntimeEvent::RetryScheduled {
        attempt: 3,
        delay: Duration::from_secs(2),
    });
    assert!(matches!(
        retry,
        Some(AndroidHostEvent::RetryScheduled {
            attempt: 3,
            delay_ms: 2_000,
        })
    ));

    let initialized = AndroidHostEvent::from_runtime_event(&PlayerRuntimeEvent::Initialized(
        PlayerRuntimeStartup {
            ffmpeg_initialized: false,
            audio_output: None,
            decoded_audio: None,
            video_decode: None,
            plugin_diagnostics: Vec::new(),
        },
    ));
    assert!(initialized.is_none());
}

#[test]
fn android_host_event_conversion_preserves_subtitle_error_details() {
    let details = SubtitleErrorDetails::new(
        "future_subtitle_code",
        "future_phase",
        Some("opaque-track".to_owned()),
        true,
        "selection failed",
    )
    .with_transaction(Some(42), Some(9));
    let event = AndroidHostEvent::from_runtime_event(&PlayerRuntimeEvent::Error(
        PlayerError::with_taxonomy(
            PlayerErrorCode::Timeout,
            PlayerErrorCategory::Playback,
            true,
            "selection failed",
        )
        .with_subtitle_details(details.clone()),
    ));

    assert_eq!(
        event,
        Some(AndroidHostEvent::Error {
            code: PlayerErrorCode::Timeout,
            category: PlayerErrorCategory::Playback,
            retriable: true,
            message: "selection failed".to_owned(),
            subtitle_details: Some(details),
        })
    );
}

#[test]
fn android_host_bridge_session_drains_native_commands() {
    let mut session = AndroidHostBridgeSession::new("placeholder.mp4");
    session
        .dispatch_command(PlayerRuntimeCommand::Play)
        .expect("play should dispatch");
    session
        .dispatch_command(PlayerRuntimeCommand::SetPlaybackRate { rate: 1.5 })
        .expect("rate should dispatch");

    let commands = session.drain_native_commands();
    assert_eq!(
        commands,
        vec![
            AndroidHostCommand::Play,
            AndroidHostCommand::SetPlaybackRate { rate: 1.5 },
        ]
    );
}

#[test]
fn android_host_bridge_session_reports_surface_and_seek_events() {
    let mut session = AndroidHostBridgeSession::new("placeholder.mp4");
    session.set_surface_attached(true);
    session.report_seek_completed(Duration::from_millis(900));

    let events = session.drain_events();
    assert!(events.iter().any(|event| matches!(
        event,
        AndroidHostEvent::VideoSurfaceChanged { attached: true }
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AndroidHostEvent::SeekCompleted { position_ms: 900 }))
    );
}

#[test]
fn android_host_bridge_session_forwards_events_to_pipeline_hooks() {
    let event_names = Arc::new(Mutex::new(Vec::new()));
    let reference = PluginReference::new(
        "dev.vesper.android-playback-hook",
        Some("dev.vesper.android-playback-hook.primary".to_owned()),
        PluginTransport::Native,
    )
    .expect("valid event hook reference");
    let registration = player_runtime::PipelineEventHookRegistration::new(
        reference,
        Arc::new(RecordingPipelineHook {
            event_names: event_names.clone(),
        }),
    )
    .expect("valid event hook registration");
    let mut session = AndroidHostBridgeSession::new_with_pipeline_event_hooks(
        "https://example.com/master.m3u8",
        vec![registration],
    )
    .expect("hook session should initialize");

    session.report_seek_completed(Duration::from_millis(900));
    let events = session.drain_events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AndroidHostEvent::SeekCompleted { position_ms: 900 }))
    );
    assert!(session.flush_pipeline_event_hooks(Duration::from_secs(1)));
    let reports = session.drain_pipeline_event_hook_reports();
    assert_eq!(reports.reports.len(), 1);
    assert_eq!(reports.reports[0].event_name, "playback.seek_completed");
    assert_eq!(
        event_names
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_slice(),
        ["playback.seek_completed"]
    );
    assert!(session.close_pipeline_event_hooks());
    assert!(session.close_pipeline_event_hooks());
}

#[test]
fn android_host_bridge_session_forwards_first_frame_to_pipeline_hooks() {
    let event_names = Arc::new(Mutex::new(Vec::new()));
    let reference = PluginReference::new(
        "dev.vesper.android-first-frame-hook",
        Some("dev.vesper.android-first-frame-hook.primary".to_owned()),
        PluginTransport::Native,
    )
    .expect("valid event hook reference");
    let registration = player_runtime::PipelineEventHookRegistration::new(
        reference,
        Arc::new(RecordingPipelineHook {
            event_names: event_names.clone(),
        }),
    )
    .expect("valid event hook registration");
    let mut session = AndroidHostBridgeSession::new_with_pipeline_event_hooks(
        "https://example.com/master.m3u8",
        vec![registration],
    )
    .expect("hook session should initialize");

    session.report_first_frame(Duration::from_millis(123), 1920, 1080);
    let _ = session.drain_events();
    assert!(session.flush_pipeline_event_hooks(Duration::from_secs(1)));
    let reports = session.drain_pipeline_event_hook_reports();
    assert_eq!(reports.reports.len(), 1);
    assert_eq!(reports.reports[0].event_name, "playback.first_frame_ready");
    assert_eq!(
        event_names
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_slice(),
        ["playback.first_frame_ready"]
    );
    assert!(session.close_pipeline_event_hooks());
}

#[test]
fn android_host_bridge_session_uses_media_info_duration_for_hls_vod_snapshot() {
    let mut session = AndroidHostBridgeSession::new("https://example.com/master.m3u8");
    session.session.media_info.duration = Some(Duration::from_secs(24));

    let snapshot = session.snapshot();
    assert_eq!(snapshot.timeline_kind, AndroidHostTimelineKind::Vod);
    assert!(snapshot.is_seekable);
    assert_eq!(snapshot.duration_ms, Some(24_000));
    assert_eq!(
        snapshot.seekable_range.expect("seekable range").end_ms,
        24_000
    );
}

#[test]
fn android_host_bridge_session_promotes_unknown_hls_duration_to_live_snapshot() {
    let mut session = AndroidHostBridgeSession::new("https://example.com/master.m3u8");

    let snapshot = session.snapshot();
    assert_eq!(snapshot.timeline_kind, AndroidHostTimelineKind::Live);
    assert!(!snapshot.is_seekable);
    assert!(snapshot.seekable_range.is_none());
    assert_eq!(snapshot.duration_ms, None);
    assert_eq!(snapshot.live_edge_ms, None);
}

#[test]
fn android_host_bridge_session_promotes_live_seekable_window_to_live_dvr_snapshot() {
    let mut session = AndroidHostBridgeSession::new("https://example.com/live.m3u8");
    session.apply_exo_snapshot(AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.0,
        position: Duration::from_secs(84),
        duration: None,
        is_live: true,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(120),
        }),
        live_edge: Some(Duration::from_secs(120)),
    });

    let snapshot = session.snapshot();
    assert_eq!(snapshot.timeline_kind, AndroidHostTimelineKind::LiveDvr);
    assert!(snapshot.is_seekable);
    assert_eq!(
        snapshot.seekable_range.expect("seekable range").end_ms,
        120_000
    );
    assert_eq!(snapshot.live_edge_ms, Some(120_000));
    assert_eq!(snapshot.position_ms, 84_000);
    assert_eq!(snapshot.duration_ms, Some(120_000));
}

struct FakeAndroidBridge;

#[derive(Default)]
struct FakeAndroidExoBindings {
    commands: Arc<Mutex<Vec<AndroidNativePlayerCommand>>>,
}

struct RecordingAndroidCommandSink {
    commands: Arc<Mutex<Vec<AndroidNativePlayerCommand>>>,
}

impl RecordingAndroidCommandSink {
    fn new(commands: Arc<Mutex<Vec<AndroidNativePlayerCommand>>>) -> Self {
        Self { commands }
    }
}

impl AndroidNativeCommandSink for RecordingAndroidCommandSink {
    fn submit_command(&mut self, command: AndroidNativePlayerCommand) -> PlayerResult<()> {
        self.commands.lock().expect("commands lock").push(command);
        Ok(())
    }
}

impl AndroidExoPlayerBridgeBindings for FakeAndroidExoBindings {
    fn probe_source(
        &self,
        _context: &AndroidExoPlayerBridgeContext,
        source: &MediaSource,
        _options: &PlayerRuntimeOptions,
    ) -> PlayerResult<AndroidNativePlayerProbe> {
        Ok(AndroidNativePlayerProbe {
            media_info: PlayerMediaInfo {
                source_uri: source.uri().to_owned(),
                source_kind: source.kind(),
                source_protocol: source.protocol(),
                duration: Some(Duration::from_secs(1)),
                bit_rate: None,
                audio_streams: 1,
                video_streams: 1,
                best_video: None,
                best_audio: None,
                track_catalog: Default::default(),
                track_selection: Default::default(),
            },
            startup: PlayerRuntimeStartup {
                ffmpeg_initialized: false,
                audio_output: None,
                decoded_audio: None,
                video_decode: None,
                plugin_diagnostics: Vec::new(),
            },
        })
    }

    fn create_command_sink(
        &self,
        _context: AndroidExoPlayerBridgeContext,
        _source: &MediaSource,
        _options: &PlayerRuntimeOptions,
        _media_info: &PlayerMediaInfo,
        _startup: &PlayerRuntimeStartup,
        controller: super::AndroidManagedNativeSessionController,
    ) -> PlayerResult<Box<dyn AndroidNativeCommandSink>> {
        controller.apply_snapshot(AndroidExoPlaybackSnapshot {
            playback_state: AndroidExoPlaybackState::Ready,
            play_when_ready: false,
            playback_rate: 1.0,
            position: Duration::ZERO,
            duration: Some(Duration::from_secs(1)),
            is_live: false,
            is_seekable: true,
            seekable_range: Some(AndroidExoSeekableRange {
                start: Duration::ZERO,
                end: Duration::from_secs(1),
            }),
            live_edge: None,
        });
        Ok(Box::new(RecordingAndroidCommandSink::new(
            self.commands.clone(),
        )))
    }
}

fn test_media_info() -> PlayerMediaInfo {
    PlayerMediaInfo {
        source_uri: "placeholder.mp4".to_owned(),
        source_kind: player_runtime::MediaSourceKind::Local,
        source_protocol: player_runtime::MediaSourceProtocol::File,
        duration: Some(Duration::from_secs(12)),
        bit_rate: None,
        audio_streams: 1,
        video_streams: 1,
        best_video: None,
        best_audio: None,
        track_catalog: Default::default(),
        track_selection: Default::default(),
    }
}

fn test_native_frame_open_config(
    mode: NativeFramePipelineMode,
) -> AndroidNativeFramePipelineOpenConfig {
    AndroidNativeFramePipelineOpenConfig {
        source_uri: "file:///tmp/video.mp4".to_owned(),
        source_normalizer: MobileSourceNormalizerConfiguration {
            mode: SourceNormalizerMode::PreflightOnly,
            plugin_artifacts: Vec::new(),
            plugin_library_paths: vec!["/tmp/libsource_normalizer.so".into()],
            native_plugin_loading_policy:
                player_runtime::NativePluginLoadingPolicy::DevelopmentRawPaths,
            runtime_profile: Some("default".to_owned()),
        },
        native_frame_pipeline: MobileNativeFramePipelineConfiguration {
            mode,
            decoder_plugin_artifacts: Vec::new(),
            decoder_plugin_library_paths: vec!["/tmp/libmediacodec_decoder.so".into()],
            frame_processor_plugin_artifacts: Vec::new(),
            frame_processor_plugin_library_paths: vec!["/tmp/libframe_processor.so".into()],
            native_plugin_loading_policy:
                player_runtime::NativePluginLoadingPolicy::DevelopmentRawPaths,
            max_in_flight_frames: Some(2),
        },
        avc_decoder_implementation_name: Some("c2.test.avc.decoder".to_owned()),
        hevc_decoder_implementation_name: Some("c2.test.hevc.decoder".to_owned()),
        presenter_profile: AndroidNativeFramePresenterProfile::SurfaceView,
    }
}

fn test_native_frame_session(
    mode: NativeFramePipelineMode,
    packet_source: Option<AndroidNativeFramePipelinePacketSource>,
) -> AndroidNativeFramePipelineSession {
    let config = test_native_frame_open_config(mode);
    let source = MediaSource::new(config.source_uri.clone());
    AndroidNativeFramePipelineSession::open_with_packet_source_without_decoder_sink(
        config,
        source,
        packet_source,
    )
    .expect("native-frame contract should open with required paths")
}

fn test_native_frame_session_with_decoder_sink(
    mode: NativeFramePipelineMode,
    packet_source: Option<AndroidNativeFramePipelinePacketSource>,
    decoder_sink: Option<Box<dyn AndroidNativeFrameDecoderSink>>,
) -> AndroidNativeFramePipelineSession {
    let mut session = test_native_frame_session_with_all_components(
        mode,
        packet_source,
        decoder_sink,
        None,
        Some(test_presenter_sink(Arc::new(Mutex::new(
            TestPresenterSinkState {
                requires_host_release: true,
                ..TestPresenterSinkState::default()
            },
        )))),
    );
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");
    session
}

fn test_native_frame_session_with_decoder_sink_and_processor_chain(
    mode: NativeFramePipelineMode,
    packet_source: Option<AndroidNativeFramePipelinePacketSource>,
    decoder_sink: Option<Box<dyn AndroidNativeFrameDecoderSink>>,
    processor_chain: Option<Box<dyn AndroidNativeFrameProcessorChain>>,
) -> AndroidNativeFramePipelineSession {
    let mut session = test_native_frame_session_with_all_components(
        mode,
        packet_source,
        decoder_sink,
        processor_chain,
        Some(test_presenter_sink(Arc::new(Mutex::new(
            TestPresenterSinkState {
                requires_host_release: true,
                ..TestPresenterSinkState::default()
            },
        )))),
    );
    session
        .attach_presenter_surface(AndroidNativeFramePresenterProfile::SurfaceView)
        .expect("attach presenter surface");
    session
}

fn test_native_frame_session_with_all_components(
    mode: NativeFramePipelineMode,
    packet_source: Option<AndroidNativeFramePipelinePacketSource>,
    decoder_sink: Option<Box<dyn AndroidNativeFrameDecoderSink>>,
    processor_chain: Option<Box<dyn AndroidNativeFrameProcessorChain>>,
    presenter_sink: Option<Box<dyn AndroidNativeFramePresenterSink>>,
) -> AndroidNativeFramePipelineSession {
    let config = test_native_frame_open_config(mode);
    let source = MediaSource::new(config.source_uri.clone());
    AndroidNativeFramePipelineSession::open_with_all_components(
        config,
        source,
        packet_source,
        decoder_sink,
        processor_chain,
        presenter_sink,
    )
    .expect("native-frame contract should open with required paths")
}

fn test_native_frame_session_with_decoder_open_plan(
    mode: NativeFramePipelineMode,
    packet_source: Option<AndroidNativeFramePipelinePacketSource>,
    decoder_open_plan: Option<super::AndroidNativeFrameDecoderOpenPlan>,
    presenter_sink: Option<Box<dyn AndroidNativeFramePresenterSink>>,
) -> AndroidNativeFramePipelineSession {
    let config = test_native_frame_open_config(mode);
    let source = MediaSource::new(config.source_uri.clone());
    let mut session = AndroidNativeFramePipelineSession::open_with_components(
        config,
        source,
        packet_source,
        decoder_open_plan,
        None,
        None,
    )
    .expect("native-frame contract should open with required paths");
    if let Some(presenter_sink) = presenter_sink {
        session.set_presenter_sink(presenter_sink);
    }
    session
}

fn test_decoder_open_plan(
    state: Arc<Mutex<TestDecoderFactoryState>>,
    requires_android_native_window: bool,
    receive_outputs: VecDeque<DecoderReceiveNativeFrameOutput>,
) -> super::AndroidNativeFrameDecoderOpenPlan {
    super::AndroidNativeFrameDecoderOpenPlan {
        plugin_name: Some("test-mediacodec-decoder".to_owned()),
        plugin_path: "/tmp/libtest_mediacodec_decoder.so".into(),
        factory: Arc::new(TestDecoderFactory {
            state,
            receive_outputs,
            supports_presentation_release: true,
        }),
        video_track: test_video_track(),
        selected_profile: AndroidNativeFramePipelineProfile::HostTimedSurface,
        required_decoder_implementation_name: "c2.test.avc.decoder".to_owned(),
        requires_android_native_window,
    }
}

fn test_video_track() -> SourceNormalizerPacketTrackInfo {
    SourceNormalizerPacketTrackInfo {
        stream_index: 1,
        media_kind: SourceNormalizerPacketMediaKind::Video,
        codec: "h264".to_owned(),
        extradata: Vec::new(),
        bitstream_format: None,
        width: Some(1_920),
        height: Some(1_080),
        coded_width: Some(1_920),
        coded_height: Some(1_080),
        reorder_depth: None,
        sample_rate: None,
        channels: None,
        channel_layout: None,
        codec_delay_samples: None,
        priming_samples: None,
        trailing_padding_samples: None,
        seek_preroll_samples: None,
        color: None,
        hdr: None,
        frame_rate: Some(30.0),
        time_base_num: Some(1),
        time_base_den: Some(1_000_000),
    }
}

fn test_packet_source(reads: Vec<TestPacketRead>) -> AndroidNativeFramePipelinePacketSource {
    AndroidNativeFramePipelinePacketSource::new(
        Some("test-source-normalizer".to_owned()),
        "/tmp/libsource_normalizer_test.so".to_owned(),
        Box::new(TestPacketSession::new(reads)),
    )
}

fn test_packet_source_with_session(
    session: TestPacketSession,
) -> AndroidNativeFramePipelinePacketSource {
    AndroidNativeFramePipelinePacketSource::new(
        Some("test-source-normalizer".to_owned()),
        "/tmp/libsource_normalizer_test.so".to_owned(),
        Box::new(session),
    )
}

fn test_decoder_native_frame(handle: usize, pts_us: i64) -> DecoderNativeFrame {
    DecoderNativeFrame {
        metadata: DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Nv12,
            codec: "h264".to_owned(),
            pts_us: Some(pts_us),
            duration_us: Some(33_333),
            width: 1_920,
            height: 1_080,
            coded_width: Some(1_920),
            coded_height: Some(1_080),
            visible_rect: None,
            handle_kind: DecoderNativeHandleKind::MediaCodecSurfaceTexture,
            pipeline_profile: Some(NativeFramePipelineProfile::MediaCodecSurfaceTexture),
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
        },
        handle,
        lease_token: None,
    }
}

fn test_processor_output_frame(input: &DecoderNativeFrame) -> NativeFrame {
    let mut metadata = input.metadata.clone();
    metadata.frame_id = Some((input.handle + 10_000) as u64);
    metadata.release_tracking = Some(DecoderNativeFrameReleaseTracking {
        frame_id: metadata.frame_id,
        requires_release: true,
    });
    NativeFrame {
        metadata: metadata.into(),
        handle: input.handle + 10_000,
        lease_token: None,
    }
}

fn test_decoder_sink(
    state: Arc<Mutex<TestDecoderSinkState>>,
) -> Box<dyn AndroidNativeFrameDecoderSink> {
    Box::new(TestDecoderSink { state })
}

fn test_processor_chain(
    state: Arc<Mutex<TestProcessorChainState>>,
) -> Box<dyn AndroidNativeFrameProcessorChain> {
    Box::new(TestProcessorChain { state })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestDecoderPacketRecord {
    pts_us: Option<i64>,
    dts_us: Option<i64>,
    duration_us: Option<i64>,
    stream_index: u32,
    key_frame: bool,
    data_len: usize,
}

#[derive(Debug)]
struct TestDecoderSinkState {
    accept_packets: bool,
    send_error: Option<String>,
    flush_error: Option<String>,
    release_error: Option<String>,
    sent_packets: Vec<TestDecoderPacketRecord>,
    receive_outputs: VecDeque<DecoderReceiveNativeFrameOutput>,
    receive_count: u32,
    released_frames: Vec<(usize, bool)>,
    flush_count: u32,
    close_count: u32,
}

impl Default for TestDecoderSinkState {
    fn default() -> Self {
        Self {
            accept_packets: true,
            send_error: None,
            flush_error: None,
            release_error: None,
            sent_packets: Vec::new(),
            receive_outputs: VecDeque::new(),
            receive_count: 0,
            released_frames: Vec::new(),
            flush_count: 0,
            close_count: 0,
        }
    }
}

struct TestDecoderSink {
    state: Arc<Mutex<TestDecoderSinkState>>,
}

impl AndroidNativeFrameDecoderSink for TestDecoderSink {
    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> PlayerResult<DecoderPacketResult> {
        let mut state = self.state.lock().expect("decoder sink state");
        if let Some(message) = state.send_error.clone() {
            return Err(player_runtime::PlayerError::new(
                PlayerErrorCode::DecodeFailure,
                message,
            ));
        }
        state.sent_packets.push(TestDecoderPacketRecord {
            pts_us: packet.pts_us,
            dts_us: packet.dts_us,
            duration_us: packet.duration_us,
            stream_index: packet.stream_index,
            key_frame: packet.key_frame,
            data_len: data.len(),
        });
        Ok(DecoderPacketResult {
            accepted: state.accept_packets,
        })
    }

    fn receive_native_frame(&mut self) -> PlayerResult<DecoderReceiveNativeFrameOutput> {
        let mut state = self.state.lock().expect("decoder sink state");
        state.receive_count += 1;
        Ok(state
            .receive_outputs
            .pop_front()
            .unwrap_or(DecoderReceiveNativeFrameOutput::NeedMoreInput))
    }

    fn release_native_frame(
        &mut self,
        frame: DecoderNativeFrame,
        presented: bool,
    ) -> PlayerResult<()> {
        let mut state = self.state.lock().expect("decoder sink state");
        if let Some(message) = state.release_error.clone() {
            return Err(player_runtime::PlayerError::new(
                PlayerErrorCode::DecodeFailure,
                message,
            ));
        }
        state.released_frames.push((frame.handle, presented));
        Ok(())
    }

    fn flush(&mut self) -> PlayerResult<()> {
        let mut state = self.state.lock().expect("decoder sink state");
        state.flush_count += 1;
        if let Some(message) = state.flush_error.clone() {
            return Err(player_runtime::PlayerError::new(
                PlayerErrorCode::DecodeFailure,
                message,
            ));
        }
        Ok(())
    }

    fn close(&mut self) -> PlayerResult<()> {
        self.state.lock().expect("decoder sink state").close_count += 1;
        Ok(())
    }
}

#[derive(Debug, Default)]
struct TestDecoderFactoryState {
    opened_configs: Vec<DecoderSessionConfig>,
    remaining_open_failures: u32,
    close_count: u32,
}

struct TestDecoderFactory {
    state: Arc<Mutex<TestDecoderFactoryState>>,
    receive_outputs: VecDeque<DecoderReceiveNativeFrameOutput>,
    supports_presentation_release: bool,
}

impl player_plugin::NativeDecoderPluginFactory for TestDecoderFactory {
    fn name(&self) -> &str {
        "test-mediacodec-decoder"
    }

    fn capabilities(&self) -> DecoderCapabilities {
        DecoderCapabilities {
            codecs: vec![DecoderCodecCapability {
                codec: "h264".to_owned(),
                media_kind: DecoderMediaKind::Video,
                profiles: Vec::new(),
                output_formats: vec![DecoderFrameFormat::Nv12],
            }],
            supports_hardware_decode: true,
            supports_cpu_video_frames: false,
            supports_audio_frames: false,
            supports_pcm_frames: false,
            supports_gpu_handles: true,
            supports_presentation_release: self.supports_presentation_release,
            supports_flush: true,
            supports_drain: true,
            max_sessions: None,
        }
    }

    fn native_requirements(&self) -> DecoderNativeRequirements {
        DecoderNativeRequirements {
            required_device_context_kinds: vec![
                DecoderNativeDeviceContextKind::AndroidNativeWindow,
            ],
            output_handle_kinds: vec![DecoderNativeHandleKind::MediaCodecSurfaceTexture],
            output_pipeline_profiles: vec![NativeFramePipelineProfile::MediaCodecSurfaceTexture],
            requires_native_device_context: true,
            accepted_bitstream_formats: Vec::new(),
        }
    }

    fn supports_native_frame_presentation_release(&self) -> bool {
        self.supports_presentation_release
    }

    fn open_native_session(
        &self,
        config: &DecoderSessionConfig,
    ) -> Result<Box<dyn NativeDecoderSession>, player_plugin::DecoderError> {
        {
            let mut state = self.state.lock().expect("factory state");
            if state.remaining_open_failures > 0 {
                state.remaining_open_failures -= 1;
                return Err(player_plugin::DecoderError::internal(
                    "transient decoder open failure".to_owned(),
                ));
            }
            state.opened_configs.push(config.clone());
        }
        Ok(Box::new(TestNativeDecoderSession {
            inner: TestDecoderSink {
                state: Arc::new(Mutex::new(TestDecoderSinkState {
                    receive_outputs: self.receive_outputs.clone(),
                    ..TestDecoderSinkState::default()
                })),
            },
            factory_state: self.state.clone(),
        }))
    }
}

struct TestNativeDecoderSession {
    inner: TestDecoderSink,
    factory_state: Arc<Mutex<TestDecoderFactoryState>>,
}

impl NativeDecoderSession for TestNativeDecoderSession {
    fn session_info(&self) -> DecoderSessionInfo {
        DecoderSessionInfo {
            decoder_name: Some("test-mediacodec-decoder".to_owned()),
            selected_hardware_backend: Some("MediaCodec".to_owned()),
            output_format: Some(DecoderFrameFormat::Nv12),
        }
    }

    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, player_plugin::DecoderError> {
        self.inner
            .send_packet(packet, data)
            .map_err(|error| player_plugin::DecoderError::internal(error.message().to_owned()))
    }

    fn receive_native_frame(
        &mut self,
    ) -> Result<DecoderReceiveNativeFrameOutput, player_plugin::DecoderError> {
        self.inner
            .receive_native_frame()
            .map_err(|error| player_plugin::DecoderError::internal(error.message().to_owned()))
    }

    fn receive_pcm_frame(
        &mut self,
    ) -> Result<DecoderReceivePcmFrameOutput, player_plugin::DecoderError> {
        Err(player_plugin::DecoderError::UnsupportedCapability {
            capability: "audio-pcm-output".to_owned(),
        })
    }

    fn release_native_frame(
        &mut self,
        frame: DecoderNativeFrame,
    ) -> Result<(), player_plugin::DecoderError> {
        self.inner
            .release_native_frame(frame, false)
            .map_err(|error| player_plugin::DecoderError::internal(error.message().to_owned()))
    }

    fn flush(&mut self) -> Result<(), player_plugin::DecoderError> {
        self.inner
            .flush()
            .map_err(|error| player_plugin::DecoderError::internal(error.message().to_owned()))
    }

    fn close(&mut self) -> Result<(), player_plugin::DecoderError> {
        self.factory_state
            .lock()
            .expect("factory state")
            .close_count += 1;
        self.inner
            .close()
            .map_err(|error| player_plugin::DecoderError::internal(error.message().to_owned()))
    }
}

fn test_presenter_sink(
    state: Arc<Mutex<TestPresenterSinkState>>,
) -> Box<dyn AndroidNativeFramePresenterSink> {
    Box::new(TestPresenterSink {
        state,
        decoder_context_window_ptr: None,
    })
}

fn test_presenter_sink_with_decoder_context(
    state: Arc<Mutex<TestPresenterSinkState>>,
    decoder_context_window_ptr: Option<usize>,
) -> Box<dyn AndroidNativeFramePresenterSink> {
    Box::new(TestPresenterSink {
        state,
        decoder_context_window_ptr,
    })
}

#[derive(Debug)]
struct TestPresenterSinkState {
    accept_frames: bool,
    requires_host_release: bool,
    submitted_handles: Vec<u64>,
    native_handles: Vec<usize>,
    flush_count: u32,
    close_count: u32,
}

impl Default for TestPresenterSinkState {
    fn default() -> Self {
        Self {
            accept_frames: true,
            requires_host_release: false,
            submitted_handles: Vec::new(),
            native_handles: Vec::new(),
            flush_count: 0,
            close_count: 0,
        }
    }
}

struct TestPresenterSink {
    state: Arc<Mutex<TestPresenterSinkState>>,
    decoder_context_window_ptr: Option<usize>,
}

impl AndroidNativeFramePresenterSink for TestPresenterSink {
    fn submit_frame(
        &mut self,
        frame: &AndroidNativeFramePresenterFrame,
    ) -> PlayerResult<AndroidNativeFramePresenterSubmitResult> {
        let mut state = self.state.lock().expect("presenter state");
        state.submitted_handles.push(frame.frame_handle);
        state.native_handles.push(frame.frame.handle);
        Ok(AndroidNativeFramePresenterSubmitResult {
            accepted: state.accept_frames,
            requires_host_release: state.requires_host_release,
            message: Some("test presenter submit".to_owned()),
        })
    }

    fn decoder_device_context(&self) -> Option<DecoderNativeDeviceContext> {
        self.decoder_context_window_ptr
            .map(|window_ptr| DecoderNativeDeviceContext::AndroidNativeWindow { window_ptr })
    }

    fn flush(&mut self) -> PlayerResult<()> {
        self.state.lock().expect("presenter state").flush_count += 1;
        Ok(())
    }

    fn close(&mut self) -> PlayerResult<()> {
        self.state.lock().expect("presenter state").close_count += 1;
        Ok(())
    }
}

#[derive(Debug, Default)]
struct TestProcessorChainState {
    fail_process: Option<String>,
    fail_release: Option<String>,
    processed_inputs: Vec<usize>,
    processed_profiles: Vec<NativeFramePipelineProfile>,
    released_outputs: Vec<usize>,
    flush_count: u32,
    close_count: u32,
}

struct TestProcessorChain {
    state: Arc<Mutex<TestProcessorChainState>>,
}

impl AndroidNativeFrameProcessorChain for TestProcessorChain {
    fn process_frame(
        &mut self,
        frame: DecoderNativeFrame,
        counters: &mut super::AndroidNativeFramePipelineCounters,
    ) -> PlayerResult<AndroidNativeFramePipelineProcessedFrame> {
        let mut state = self.state.lock().expect("processor state");
        if let Some(message) = state.fail_process.clone() {
            return Err(player_runtime::PlayerError::new(
                PlayerErrorCode::DecodeFailure,
                message,
            ));
        }
        state.processed_inputs.push(frame.handle);
        state
            .processed_profiles
            .push(frame.metadata.pipeline_profile.clone().unwrap_or_else(|| {
                NativeFramePipelineProfile::from_handle_kind(
                    &frame.metadata.handle_kind.clone().into(),
                )
            }));
        let output = test_processor_output_frame(&frame);
        counters.processed_frames += 1;
        Ok(AndroidNativeFramePipelineProcessedFrame {
            decoder_frame: frame,
            presentation_frame: output.clone().into(),
            processor_outputs: vec![AndroidNativeFrameProcessorOwnedFrame {
                processor_index: 0,
                frame: output,
            }],
        })
    }

    fn release_processor_outputs(
        &mut self,
        outputs: Vec<AndroidNativeFrameProcessorOwnedFrame>,
    ) -> Result<NativeFrameProcessorReleaseResult, NativeFrameProcessorReleaseError> {
        let mut state = self.state.lock().expect("processor state");
        if let Some(message) = state.fail_release.clone() {
            return Err(NativeFrameProcessorReleaseError {
                error: NativeFramePipelineError::new("releaseProcessorFrame", message),
                unreleased_outputs: outputs,
            });
        }
        state
            .released_outputs
            .extend(outputs.into_iter().map(|output| output.frame.handle));
        Ok(NativeFrameProcessorReleaseResult::default())
    }

    fn flush(&mut self) -> PlayerResult<()> {
        self.state.lock().expect("processor state").flush_count += 1;
        Ok(())
    }

    fn close(&mut self) -> PlayerResult<()> {
        self.state.lock().expect("processor state").close_count += 1;
        Ok(())
    }
}

#[derive(Debug, Clone)]
enum TestPacketRead {
    Packet {
        media_kind: SourceNormalizerPacketMediaKind,
        stream_index: u32,
        bytes: Vec<u8>,
    },
    NeedMoreData(String),
    EndOfStream,
}

impl TestPacketRead {
    fn packet(
        media_kind: SourceNormalizerPacketMediaKind,
        stream_index: u32,
        byte_len: usize,
    ) -> Self {
        Self::Packet {
            media_kind,
            stream_index,
            bytes: vec![stream_index as u8; byte_len],
        }
    }

    fn need_more_data(message: impl Into<String>) -> Self {
        Self::NeedMoreData(message.into())
    }

    fn end_of_stream() -> Self {
        Self::EndOfStream
    }
}

struct TestPacketSession {
    reads: std::collections::VecDeque<TestPacketRead>,
    video_track: SourceNormalizerPacketTrackInfo,
    current_data: Vec<u8>,
    outstanding_handle: Option<usize>,
    next_handle: usize,
    flush_error: Option<String>,
    seek_error: Option<String>,
    seek_positions: Vec<u64>,
}

impl TestPacketSession {
    fn new(reads: Vec<TestPacketRead>) -> Self {
        Self {
            reads: reads.into(),
            video_track: test_video_track(),
            current_data: Vec::new(),
            outstanding_handle: None,
            next_handle: 1,
            flush_error: None,
            seek_error: None,
            seek_positions: Vec::new(),
        }
    }
}

impl SourceNormalizerPacketSession for TestPacketSession {
    fn stream_info(&self) -> SourceNormalizerPacketStreamInfo {
        SourceNormalizerPacketStreamInfo {
            session_id: Some("test-packet-session".to_owned()),
            normalizer_name: Some("test-source-normalizer".to_owned()),
            runtime_profile: Some("test".to_owned()),
            selected_backend: None,
            tracks: vec![
                SourceNormalizerPacketTrackInfo {
                    stream_index: 0,
                    media_kind: SourceNormalizerPacketMediaKind::Audio,
                    codec: "aac".to_owned(),
                    extradata: Vec::new(),
                    bitstream_format: None,
                    width: None,
                    height: None,
                    coded_width: None,
                    coded_height: None,
                    reorder_depth: None,
                    sample_rate: Some(48_000),
                    channels: Some(2),
                    channel_layout: None,
                    codec_delay_samples: None,
                    priming_samples: None,
                    trailing_padding_samples: None,
                    seek_preroll_samples: None,
                    color: None,
                    hdr: None,
                    frame_rate: None,
                    time_base_num: None,
                    time_base_den: None,
                },
                self.video_track.clone(),
            ],
            selected_track_index: Some(1),
            duration_millis: Some(1_000),
            seekable: true,
        }
    }

    fn read_packet(&mut self) -> Result<SourceNormalizerPacketLease<'_>, SourceNormalizerError> {
        if self.outstanding_handle.is_some() {
            return Err(SourceNormalizerError::abi_violation(
                "test packet still has an outstanding lease",
            ));
        }
        match self
            .reads
            .pop_front()
            .unwrap_or(TestPacketRead::EndOfStream)
        {
            TestPacketRead::Packet {
                media_kind,
                stream_index,
                bytes,
            } => {
                let handle = self.next_handle;
                self.next_handle += 1;
                self.current_data = bytes;
                self.outstanding_handle = Some(handle);
                Ok(SourceNormalizerPacketLease {
                    metadata: SourceNormalizerReadPacketMetadata::packet(SourceNormalizerPacket {
                        pts_us: Some(1_000),
                        dts_us: Some(1_000),
                        duration_us: Some(33_333),
                        stream_index,
                        media_kind,
                        key_frame: true,
                        discontinuity: false,
                        sample_rate: None,
                        channels: None,
                        channel_layout: None,
                        sample_format: None,
                        frame_count: None,
                        end_of_stream: false,
                    }),
                    data: &self.current_data,
                    handle,
                })
            }
            TestPacketRead::NeedMoreData(message) => Ok(SourceNormalizerPacketLease {
                metadata: SourceNormalizerReadPacketMetadata::need_more_data(Some(message)),
                data: &[],
                handle: 0,
            }),
            TestPacketRead::EndOfStream => Ok(SourceNormalizerPacketLease {
                metadata: SourceNormalizerReadPacketMetadata::end_of_stream(),
                data: &[],
                handle: 0,
            }),
        }
    }

    fn release_packet(&mut self, packet_handle: usize) -> Result<(), SourceNormalizerError> {
        match self.outstanding_handle {
            Some(handle) if handle == packet_handle => {
                self.outstanding_handle = None;
                self.current_data.clear();
                Ok(())
            }
            _ => Err(SourceNormalizerError::abi_violation(
                "test packet release received an unknown handle",
            )),
        }
    }

    fn seek(
        &mut self,
        seek: &SourceNormalizerPacketSeek,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        if let Some(message) = self.seek_error.clone() {
            return Err(SourceNormalizerError::internal(message));
        }
        self.seek_positions.push(seek.position_millis);
        self.outstanding_handle = None;
        self.current_data.clear();
        Ok(SourceNormalizerOperationStatus {
            completed: true,
            message: None,
        })
    }

    fn flush(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        if let Some(message) = self.flush_error.clone() {
            return Err(SourceNormalizerError::internal(message));
        }
        self.outstanding_handle = None;
        self.current_data.clear();
        Ok(SourceNormalizerOperationStatus {
            completed: true,
            message: None,
        })
    }

    fn close(&mut self) -> Result<(), SourceNormalizerError> {
        self.outstanding_handle = None;
        self.current_data.clear();
        Ok(())
    }
}

fn test_media_info_with_tracks() -> PlayerMediaInfo {
    PlayerMediaInfo {
        source_uri: "https://example.com/master.m3u8".to_owned(),
        source_kind: player_runtime::MediaSourceKind::Remote,
        source_protocol: player_runtime::MediaSourceProtocol::Hls,
        duration: Some(Duration::from_secs(120)),
        bit_rate: None,
        audio_streams: 1,
        video_streams: 2,
        best_video: None,
        best_audio: None,
        track_catalog: MediaTrackCatalog {
            tracks: vec![
                MediaTrack {
                    id: "video-720p".to_owned(),
                    kind: MediaTrackKind::Video,
                    label: Some("720p".to_owned()),
                    language: None,
                    codec: Some("avc1.64001f".to_owned()),
                    bit_rate: Some(2_000_000),
                    width: Some(1280),
                    height: Some(720),
                    frame_rate: Some(30.0),
                    channels: None,
                    sample_rate: None,
                    is_default: true,
                    is_forced: false,
                },
                MediaTrack {
                    id: "audio-en".to_owned(),
                    kind: MediaTrackKind::Audio,
                    label: Some("English".to_owned()),
                    language: Some("en".to_owned()),
                    codec: Some("mp4a.40.2".to_owned()),
                    bit_rate: Some(128_000),
                    width: None,
                    height: None,
                    frame_rate: None,
                    channels: Some(2),
                    sample_rate: Some(48_000),
                    is_default: true,
                    is_forced: false,
                },
                MediaTrack {
                    id: "text-en".to_owned(),
                    kind: MediaTrackKind::Subtitle,
                    label: Some("English CC".to_owned()),
                    language: Some("en".to_owned()),
                    codec: Some("wvtt".to_owned()),
                    bit_rate: None,
                    width: None,
                    height: None,
                    frame_rate: None,
                    channels: None,
                    sample_rate: None,
                    is_default: true,
                    is_forced: false,
                },
            ],
            adaptive_video: true,
            adaptive_audio: false,
        },
        track_selection: Default::default(),
    }
}

impl AndroidNativePlayerBridge for FakeAndroidBridge {
    fn probe_source(
        &self,
        source: &MediaSource,
        _options: &PlayerRuntimeOptions,
    ) -> PlayerResult<AndroidNativePlayerProbe> {
        Ok(AndroidNativePlayerProbe {
            media_info: PlayerMediaInfo {
                source_uri: source.uri().to_owned(),
                source_kind: source.kind(),
                source_protocol: source.protocol(),
                duration: Some(Duration::from_secs(1)),
                bit_rate: None,
                audio_streams: 1,
                video_streams: 1,
                best_video: None,
                best_audio: None,
                track_catalog: Default::default(),
                track_selection: Default::default(),
            },
            startup: PlayerRuntimeStartup {
                ffmpeg_initialized: false,
                audio_output: None,
                decoded_audio: None,
                video_decode: None,
                plugin_diagnostics: Vec::new(),
            },
        })
    }

    fn initialize_session(
        &self,
        source: MediaSource,
        _options: PlayerRuntimeOptions,
        media_info: &PlayerMediaInfo,
        _startup: &PlayerRuntimeStartup,
    ) -> PlayerResult<AndroidNativePlayerSessionBootstrap> {
        Ok(AndroidNativePlayerSessionBootstrap {
            runtime: Box::new(FakeAndroidSession {
                source_uri: source.uri().to_owned(),
                media_info: media_info.clone(),
            }),
            initial_frame: None,
        })
    }
}

struct FakeAndroidSession {
    source_uri: String,
    media_info: PlayerMediaInfo,
}

impl AndroidNativePlayerSession for FakeAndroidSession {
    fn source_uri(&self) -> &str {
        &self.source_uri
    }

    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        super::android_native_capabilities()
    }

    fn media_info(&self) -> &PlayerMediaInfo {
        &self.media_info
    }

    fn presentation_state(&self) -> PresentationState {
        PresentationState::Ready
    }

    fn playback_rate(&self) -> f32 {
        1.0
    }

    fn progress(&self) -> PlaybackProgress {
        PlaybackProgress::new(Duration::ZERO, self.media_info.duration)
    }

    fn drain_events(&mut self) -> Vec<player_runtime::PlayerRuntimeEvent> {
        Vec::new()
    }

    fn dispatch(
        &mut self,
        _command: PlayerRuntimeCommand,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        Err(player_runtime::PlayerError::new(
            PlayerErrorCode::Unsupported,
            "fake android session does not implement commands",
        ))
    }

    fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>> {
        Ok(None)
    }

    fn next_deadline(&self) -> Option<Instant> {
        None
    }
}
