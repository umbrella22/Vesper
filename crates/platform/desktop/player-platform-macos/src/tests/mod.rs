use std::collections::VecDeque;
#[cfg(target_os = "macos")]
use std::os::raw::c_void;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use super::macos_runtime_adapter_factory;
use super::{
    FrameProcessorDebugState, MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID,
    MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID, MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
    MacosFrameProcessorChain, MacosHostPlayerRuntimeAdapterFactory, MacosNativeFrameDecoderState,
    MacosNativeFramePacketSendStatus, MacosNativeFramePacketSource, MacosNativeFramePrefetchWakeup,
    MacosNativeFrameVideoSource, MacosNativeFrameWorkerEvent, MacosRuntimeActiveFallback,
    MacosRuntimeAdapter, MacosRuntimeAdapterFallback, MacosRuntimeAdapterInitializer,
    MacosRuntimeDiagnostics, MacosSoftwarePlayerRuntimeAdapterFactory,
    MacosSourceNormalizationOutcome, apply_decoder_plugin_diagnostics,
    apply_decoder_plugin_diagnostics_to_video_decode,
    apply_decoder_plugin_registry_to_video_decode, apply_source_normalizer_open_diagnostics,
    attach_source_normalizer_to_runtime, flush_and_seek_macos_native_frame_source,
    macos_native_frame_decoder_video_decode_info, macos_runtime_diagnostics,
    macos_video_decode_info, mark_source_normalizer_packet_stream_participated,
    open_macos_host_runtime_source_with_options,
    open_macos_software_runtime_source_with_options_and_interrupt,
    prepare_source_normalizer_for_open, present_and_release_native_frame_with_presenter,
    present_if_current_epoch_and_release, probe_macos_host_runtime_initializer_with_factories,
    probe_macos_host_runtime_source_with_options, process_macos_native_frame,
    release_native_frame_with_counter, runtime_status_from_loader, send_macos_native_frame_packet,
    should_forward_strict_frame_processor_fallback_error,
    should_trigger_runtime_fallback_for_advance, should_trigger_runtime_fallback_for_command,
    source_normalizer_packet_decoder_unavailable_message, source_normalizer_packet_stream_details,
    source_normalizer_packet_stream_summary,
    source_normalizer_packet_stream_summary_with_audio_decoder_readiness,
    spawn_macos_native_frame_prefetch_worker, strict_frame_processor_fallback_enabled,
    without_source_normalizer_options,
};
use player_backend_ffmpeg::{
    CompressedVideoPacket, FfmpegBackend, VideoPacketSource, VideoPacketStreamInfo,
};
use player_model::MediaSource;
use player_platform_apple::VIDEOTOOLBOX_BACKEND_NAME;
use player_platform_desktop::{DesktopVideoFramePoll, DesktopVideoSource};
use player_platform_native_frame::{
    NativeFrameProcessorChainCore, NativeFrameProcessorNode, NativeFrameProcessorProcessedFrame,
};
use player_plugin::{
    DecoderBitstreamFormat, DecoderError, DecoderMediaKind, DecoderNativeFrame,
    DecoderNativeFrameMetadata, DecoderNativeHandleKind, DecoderPacket, DecoderPacketResult,
    DecoderReceiveNativeFrameOutput, DecoderSessionConfig, DecoderSessionInfo, FrameProcessorError,
    FrameProcessorFrameTimings, FrameProcessorOutputFrame, FrameProcessorReceiveOutput,
    FrameProcessorSession, FrameProcessorSessionInfo, FrameProcessorSubmitFrame,
    FrameProcessorSubmitResult, FrameProcessorSubmitStatus, NativeDecoderSession, NativeFrame,
    SourceNormalizerError, SourceNormalizerOperationStatus, SourceNormalizerPacket,
    SourceNormalizerPacketLease, SourceNormalizerPacketMediaKind, SourceNormalizerPacketSeek,
    SourceNormalizerPacketSession, SourceNormalizerPacketStreamInfo,
    SourceNormalizerPacketTrackInfo, SourceNormalizerReadPacketMetadata,
    SourceNormalizerReadPacketStatus, VesperPluginKind,
};
use player_plugin_loader::{
    DecoderPluginCapabilitySummary, DecoderPluginCodecSummary, LoadedDynamicPlugin,
    PluginCapabilitySummary, PluginDiagnosticRecord, PluginDiagnosticStatus, PluginRegistry,
};
use player_runtime::{
    DecodedVideoFrame, FrameProcessorMode, FrameProcessorPolicy, FrameProcessorPolicyAction,
    FrameProcessorWarningKind, PlaybackProgress, PlayerAudioInfo, PlayerError, PlayerErrorCode,
    PlayerMediaInfo, PlayerPluginCapabilitySummary, PlayerPluginDiagnostic,
    PlayerPluginDiagnosticStatus, PlayerPluginParticipation, PlayerResult, PlayerRuntime,
    PlayerRuntimeAdapter, PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer,
    PlayerRuntimeCommand, PlayerRuntimeCommandResult, PlayerRuntimeEvent, PlayerRuntimeOptions,
    PlayerRuntimeStartup, PlayerRuntimeWarning, PlayerVideoDecodeInfo, PlayerVideoDecodeMode,
    PlayerVideoInfo, PlayerVideoSurfaceKind, PlayerVideoSurfaceTarget, PresentationState,
    SourceNormalizerMode,
};
#[cfg(target_os = "macos")]
use player_runtime::{PlayerDecoderPluginVideoMode, PlayerRuntimeInitializer};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn player_macos_test_create_player_layer() -> *mut c_void;
    fn player_macos_test_release_object(handle: *mut c_void);
}

#[test]
fn macos_factory_matches_host_support() {
    let factory = MacosSoftwarePlayerRuntimeAdapterFactory;

    if cfg!(target_os = "macos") {
        let Some(test_video_path) = test_video_path() else {
            eprintln!(
                "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
            );
            return;
        };
        let result = factory.probe_source_with_options(
            MediaSource::new(test_video_path),
            PlayerRuntimeOptions::default(),
        );
        let initializer = result.expect("macos host should support the macos desktop adapter");
        let capabilities = initializer.capabilities();
        let startup = initializer.startup();
        let video_decode = startup
            .video_decode
            .expect("macos initializer should report video decode diagnostics");
        assert_eq!(
            capabilities.adapter_id,
            MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
        );
        assert_eq!(
            capabilities.backend_family,
            PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
        );
        assert_eq!(video_decode.selected_mode, PlayerVideoDecodeMode::Software);
        assert_eq!(
            video_decode.hardware_backend.as_deref(),
            Some(VIDEOTOOLBOX_BACKEND_NAME)
        );
        assert!(video_decode.fallback_reason.is_some());
    } else {
        let result = factory.probe_source_with_options(
            MediaSource::new("fixture.mp4"),
            PlayerRuntimeOptions::default(),
        );
        let error = match result {
            Ok(_) => panic!("non-macos hosts should reject the macos adapter"),
            Err(error) => error,
        };
        assert_eq!(error.code(), PlayerErrorCode::Unsupported);
    }
}

#[test]
fn macos_host_factory_without_surface_prefers_software_path() {
    if !cfg!(target_os = "macos") {
        return;
    }

    let Some(test_video_path) = test_video_path() else {
        eprintln!(
            "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
        );
        return;
    };
    let factory = MacosHostPlayerRuntimeAdapterFactory;
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new(test_video_path),
            PlayerRuntimeOptions::default(),
        )
        .expect("macos host factory probe should succeed");

    let capabilities = initializer.capabilities();
    let startup = initializer.startup();

    assert_eq!(factory.adapter_id(), MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID);
    assert_eq!(
        capabilities.backend_family,
        PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
    );
    assert_eq!(
        capabilities.adapter_id,
        MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
    );
    assert!(
        startup
            .video_decode
            .as_ref()
            .and_then(|info| info.fallback_reason.as_deref())
            .unwrap_or_default()
            .contains("requires an external video surface")
    );
}

#[test]
#[cfg(target_os = "macos")]
fn macos_host_factory_with_surface_prefers_native_path() {
    let Some(test_video_path) = test_video_path() else {
        eprintln!(
            "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
        );
        return;
    };
    let layer_handle = unsafe { player_macos_test_create_player_layer() };
    assert!(
        !layer_handle.is_null(),
        "test player layer handle should be created"
    );

    let factory = MacosHostPlayerRuntimeAdapterFactory;
    let options = PlayerRuntimeOptions::default().with_video_surface(PlayerVideoSurfaceTarget {
        kind: PlayerVideoSurfaceKind::PlayerLayer,
        handle: layer_handle as usize,
    });
    let initializer = factory
        .probe_source_with_options(MediaSource::new(test_video_path), options)
        .expect("macos host factory should prefer native when a valid surface exists");

    let capabilities = initializer.capabilities();
    let bootstrap = initializer
        .initialize()
        .expect("native-backed host initializer should initialize");

    assert_eq!(
        capabilities.backend_family,
        PlayerRuntimeAdapterBackendFamily::NativeMacos
    );
    assert_eq!(
        capabilities.adapter_id,
        MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
    );
    assert_eq!(
        bootstrap.runtime.capabilities().backend_family,
        PlayerRuntimeAdapterBackendFamily::NativeMacos
    );

    unsafe {
        player_macos_test_release_object(layer_handle);
    }
}

#[test]
fn macos_host_strategy_routes_explicit_native_frame_request_to_plugin_path() {
    let native_factory = FakeStrategyFactory {
        capabilities: PlayerRuntimeAdapterCapabilities {
            adapter_id: MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID,
            backend_family: PlayerRuntimeAdapterBackendFamily::NativeMacos,
            supports_audio_output: true,
            supports_frame_output: false,
            supports_external_video_surface: true,
            supports_seek: true,
            supports_stop: true,
            supports_playback_rate: true,
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(3.0),
            natural_playback_rate_max: Some(2.0),
            supports_hardware_decode: true,
            supports_streaming: true,
            supports_hdr: true,
        },
        media_info: media_info_with_codec("H264"),
        startup: startup_with_video_decode(PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Hardware,
            hardware_available: true,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: None,
        }),
        initialize_error: None,
        advance_error: None,
    };
    let software_factory = FakeStrategyFactory {
        capabilities: PlayerRuntimeAdapterCapabilities {
            adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
            supports_audio_output: true,
            supports_frame_output: false,
            supports_external_video_surface: true,
            supports_seek: true,
            supports_stop: true,
            supports_playback_rate: true,
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(3.0),
            natural_playback_rate_max: Some(2.0),
            supports_hardware_decode: true,
            supports_streaming: true,
            supports_hdr: true,
        },
        media_info: media_info_with_codec("H264"),
        startup: startup_with_video_decode(PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Hardware,
            hardware_available: true,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: None,
        }),
        initialize_error: None,
        advance_error: None,
    };
    unsafe {
        std::env::set_var("VESPER_MACOS_TEST_FORCE_PRESENTER_FAILURE", "1");
    }
    let options = PlayerRuntimeOptions::default()
        .with_video_surface(PlayerVideoSurfaceTarget {
            kind: PlayerVideoSurfaceKind::PlayerLayer,
            handle: 0x1234,
        })
        .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame);

    let initializer = probe_macos_host_runtime_initializer_with_factories(
        MediaSource::new("fixture.mp4"),
        options,
        &native_factory,
        Arc::new(software_factory),
    )
    .expect("host strategy probe should route to desktop plugin path");

    assert_eq!(
        initializer.capabilities().backend_family,
        PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
    );
    assert_eq!(
        initializer.capabilities().adapter_id,
        MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
    );
    assert!(
        initializer
            .startup()
            .video_decode
            .as_ref()
            .and_then(|info| info.fallback_reason.as_deref())
            .unwrap_or_default()
            .contains("selected sdkManagedNativeFrame route")
    );
}

#[test]
fn host_strategy_initializer_falls_back_to_software_when_native_initialize_fails() {
    let native_factory = FakeStrategyFactory {
        capabilities: PlayerRuntimeAdapterCapabilities {
            adapter_id: MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID,
            backend_family: PlayerRuntimeAdapterBackendFamily::NativeMacos,
            supports_audio_output: true,
            supports_frame_output: false,
            supports_external_video_surface: true,
            supports_seek: true,
            supports_stop: true,
            supports_playback_rate: true,
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(3.0),
            natural_playback_rate_max: Some(2.0),
            supports_hardware_decode: true,
            supports_streaming: true,
            supports_hdr: true,
        },
        media_info: media_info_with_codec("H264"),
        startup: startup_with_video_decode(PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Hardware,
            hardware_available: true,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: None,
        }),
        initialize_error: Some(PlayerError::new(
            PlayerErrorCode::BackendFailure,
            "native init failed",
        )),
        advance_error: None,
    };
    let software_factory = FakeStrategyFactory {
        capabilities: PlayerRuntimeAdapterCapabilities {
            adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
            supports_audio_output: true,
            supports_frame_output: true,
            supports_external_video_surface: false,
            supports_seek: true,
            supports_stop: true,
            supports_playback_rate: true,
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(3.0),
            natural_playback_rate_max: Some(2.0),
            supports_hardware_decode: false,
            supports_streaming: true,
            supports_hdr: false,
        },
        media_info: media_info_with_codec("H264"),
        startup: startup_with_video_decode(PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Software,
            hardware_available: true,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: None,
        }),
        initialize_error: None,
        advance_error: None,
    };
    let options = PlayerRuntimeOptions::default().with_video_surface(PlayerVideoSurfaceTarget {
        kind: PlayerVideoSurfaceKind::PlayerLayer,
        handle: 0x1234,
    });
    let initializer = probe_macos_host_runtime_initializer_with_factories(
        MediaSource::new("fixture.mp4"),
        options,
        &native_factory,
        Arc::new(software_factory.clone()),
    )
    .expect("host strategy probe should succeed");

    assert_eq!(
        initializer.capabilities().backend_family,
        PlayerRuntimeAdapterBackendFamily::NativeMacos
    );

    let bootstrap = initializer
        .initialize()
        .expect("host strategy initialize should fall back to software");

    assert_eq!(
        bootstrap.runtime.capabilities().backend_family,
        PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
    );
    assert!(
        bootstrap
            .startup
            .video_decode
            .as_ref()
            .and_then(|info| info.fallback_reason.as_deref())
            .unwrap_or_default()
            .contains("native init failed")
    );
}

#[test]
fn strict_frame_processor_fallback_error_is_forwarded_without_host_wrapper() {
    let mut strict_options = PlayerRuntimeOptions::default()
        .with_frame_processor_library_paths([PathBuf::from("fixture-frame-processor")])
        .with_frame_processor_mode(FrameProcessorMode::RequireProcessed);
    assert!(strict_frame_processor_fallback_enabled(&strict_options));
    let strict_error = PlayerError::new(
        PlayerErrorCode::BackendFailure,
        "native-frame frame processor initialization failed in strict mode: unsupported native handle kind: CvPixelBuffer",
    );

    assert!(should_forward_strict_frame_processor_fallback_error(
        strict_frame_processor_fallback_enabled(&strict_options),
        &strict_error
    ));

    strict_options.frame_processor_mode = FrameProcessorMode::PreferProcessed;
    assert!(!should_forward_strict_frame_processor_fallback_error(
        strict_frame_processor_fallback_enabled(&strict_options),
        &strict_error
    ));
}

#[test]
fn software_runtime_initializer_falls_back_when_native_frame_initialize_fails() {
    let native_inner = Box::new(FakeStrategyInitializer {
        capabilities: PlayerRuntimeAdapterCapabilities {
            adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
            supports_audio_output: true,
            supports_frame_output: false,
            supports_external_video_surface: true,
            supports_seek: true,
            supports_stop: true,
            supports_playback_rate: true,
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(3.0),
            natural_playback_rate_max: Some(2.0),
            supports_hardware_decode: true,
            supports_streaming: true,
            supports_hdr: true,
        },
        media_info: media_info_with_codec("H264"),
        startup: startup_with_video_decode(PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Hardware,
            hardware_available: true,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: None,
        }),
        initialize_error: Some(PlayerError::new(
            PlayerErrorCode::BackendFailure,
            "native-frame init failed",
        )),
        advance_error: None,
    });
    let fallback_inner = Box::new(FakeStrategyInitializer {
        capabilities: PlayerRuntimeAdapterCapabilities {
            adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
            supports_audio_output: true,
            supports_frame_output: true,
            supports_external_video_surface: false,
            supports_seek: true,
            supports_stop: true,
            supports_playback_rate: true,
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(3.0),
            natural_playback_rate_max: Some(2.0),
            supports_hardware_decode: false,
            supports_streaming: true,
            supports_hdr: false,
        },
        media_info: media_info_with_codec("H264"),
        startup: startup_with_video_decode(PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Software,
            hardware_available: true,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: Some("software fallback ready".to_owned()),
        }),
        initialize_error: None,
        advance_error: None,
    });
    let diagnostics = MacosRuntimeDiagnostics {
        video_decode: macos_native_frame_decoder_video_decode_info(Some("fixture-native")),
        plugin_diagnostics: Vec::new(),
        has_video_surface: true,
    };
    let fallback_diagnostics = MacosRuntimeDiagnostics {
        video_decode: startup_with_video_decode(PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Software,
            hardware_available: true,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: Some("software fallback ready".to_owned()),
        })
        .video_decode
        .expect("fallback video decode"),
        plugin_diagnostics: Vec::new(),
        has_video_surface: false,
    };

    let initializer = Box::new(MacosRuntimeAdapterInitializer {
        inner: native_inner,
        diagnostics,
        fallback: Some(MacosRuntimeAdapterFallback {
            inner: fallback_inner,
            diagnostics: fallback_diagnostics,
            fallback_reason:
                "native-frame decoder plugin initialization failed; selected softwareDecoder route"
                    .to_owned(),
        }),
        runtime_fallback: None,
        strict_frame_processor_error_prefix: None,
    });

    let bootstrap = initializer
        .initialize()
        .expect("software runtime initializer should fall back");

    assert_eq!(
        bootstrap.runtime.capabilities().backend_family,
        PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
    );
    assert!(bootstrap.runtime.capabilities().supports_frame_output);
    assert!(
        !bootstrap
            .runtime
            .capabilities()
            .supports_external_video_surface
    );
    assert!(
        bootstrap
            .startup
            .video_decode
            .as_ref()
            .and_then(|info| info.fallback_reason.as_deref())
            .unwrap_or_default()
            .contains("native-frame init failed")
    );
}

#[test]
fn software_runtime_initializer_returns_native_frame_error_without_fallback() {
    let native_inner = Box::new(FakeStrategyInitializer {
        capabilities: PlayerRuntimeAdapterCapabilities {
            adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
            supports_audio_output: true,
            supports_frame_output: false,
            supports_external_video_surface: true,
            supports_seek: true,
            supports_stop: true,
            supports_playback_rate: true,
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(3.0),
            natural_playback_rate_max: Some(2.0),
            supports_hardware_decode: true,
            supports_streaming: true,
            supports_hdr: true,
        },
        media_info: media_info_with_codec("H264"),
        startup: startup_with_video_decode(PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Hardware,
            hardware_available: true,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: None,
        }),
        initialize_error: Some(PlayerError::new(
            PlayerErrorCode::BackendFailure,
            "native-frame init failed",
        )),
        advance_error: None,
    });
    let diagnostics = MacosRuntimeDiagnostics {
        video_decode: macos_native_frame_decoder_video_decode_info(Some("fixture-native")),
        plugin_diagnostics: Vec::new(),
        has_video_surface: true,
    };
    let initializer = Box::new(MacosRuntimeAdapterInitializer {
        inner: native_inner,
        diagnostics,
        fallback: None,
        runtime_fallback: None,
        strict_frame_processor_error_prefix: Some(
            "native-frame frame processor initialization failed in strict mode".to_owned(),
        ),
    });

    let error = match initializer.initialize() {
        Ok(_) => panic!("strict native-frame initializer should not fall back"),
        Err(error) => error,
    };

    assert_eq!(error.code(), PlayerErrorCode::BackendFailure);
    assert!(
        error
            .message()
            .contains("frame processor initialization failed in strict mode")
    );
    assert!(error.message().contains("native-frame init failed"));
}

#[test]
fn runtime_advance_backend_failure_falls_back_to_software_runtime() {
    let native_runtime = Box::new(FakeStrategyRuntime {
        capabilities: PlayerRuntimeAdapterCapabilities {
            adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
            supports_audio_output: true,
            supports_frame_output: false,
            supports_external_video_surface: true,
            supports_seek: true,
            supports_stop: true,
            supports_playback_rate: true,
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(3.0),
            natural_playback_rate_max: Some(2.0),
            supports_hardware_decode: true,
            supports_streaming: true,
            supports_hdr: true,
        },
        media_info: media_info_with_codec("H264"),
        playback_rate: 1.5,
        progress: PlaybackProgress::new(Duration::from_secs(5), Some(Duration::from_secs(30))),
        state: PresentationState::Playing,
        events: VecDeque::new(),
        advance_error: Some(PlayerError::new(
            PlayerErrorCode::BackendFailure,
            "forced presenter failure",
        )),
        dispatch_error: None,
    });
    let fallback_source = MediaSource::new("fixture.mp4");
    let fallback_options = PlayerRuntimeOptions::default();
    let adapter = MacosRuntimeAdapter {
        inner: native_runtime,
        video_decode: PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Hardware,
            hardware_available: true,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: None,
        },
        plugin_diagnostics: Vec::new(),
        has_video_surface: true,
        runtime_fallback: Some(MacosRuntimeActiveFallback {
            source: fallback_source.clone(),
            options: fallback_options.clone(),
            fallback_reason:
                "native-frame runtime failed during playback; selected softwareDecoder route"
                    .to_owned(),
        }),
        pending_runtime_fallback_events: VecDeque::new(),
        source_normalizer_packet_session: None,
    };
    let mut adapter = adapter;

    let fallback = adapter
        .runtime_fallback
        .clone()
        .expect("runtime fallback config should exist");
    adapter
        .activate_runtime_fallback_with(
            "forced presenter failure",
            fallback,
            |_source, _options| Ok(test_fallback_bootstrap()),
        )
        .expect("advance should fall back instead of failing");

    assert!(adapter.inner.capabilities().supports_frame_output);
    assert!(!adapter.inner.capabilities().supports_external_video_surface);
    assert_eq!(adapter.playback_rate(), 1.5);
    assert_eq!(adapter.progress().position(), Duration::from_secs(5));
    assert_eq!(adapter.presentation_state(), PresentationState::Playing);
    let events = adapter.drain_events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, PlayerRuntimeEvent::Error(_)))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::VideoSurfaceChanged { attached: false }
    )));
    assert!(
        adapter
            .video_decode
            .fallback_reason
            .as_deref()
            .unwrap_or_default()
            .contains("forced presenter failure")
    );
}

#[test]
fn runtime_fallback_marks_selected_plugin_diagnostics_as_fallback() {
    let native_runtime = Box::new(FakeStrategyRuntime {
        capabilities: PlayerRuntimeAdapterCapabilities {
            adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
            supports_audio_output: true,
            supports_frame_output: false,
            supports_external_video_surface: true,
            supports_seek: true,
            supports_stop: true,
            supports_playback_rate: true,
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(3.0),
            natural_playback_rate_max: Some(2.0),
            supports_hardware_decode: true,
            supports_streaming: true,
            supports_hdr: true,
        },
        media_info: media_info_with_codec("H264"),
        playback_rate: 1.0,
        progress: PlaybackProgress::new(Duration::ZERO, Some(Duration::from_secs(30))),
        state: PresentationState::Paused,
        events: VecDeque::new(),
        advance_error: None,
        dispatch_error: None,
    });
    let mut adapter = MacosRuntimeAdapter {
        inner: native_runtime,
        video_decode: PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Hardware,
            hardware_available: true,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: None,
        },
        plugin_diagnostics: Vec::new(),
        has_video_surface: true,
        runtime_fallback: Some(MacosRuntimeActiveFallback {
            source: MediaSource::new("fixture.mp4"),
            options: PlayerRuntimeOptions::default(),
            fallback_reason:
                "native-frame runtime failed during playback; selected softwareDecoder route"
                    .to_owned(),
        }),
        pending_runtime_fallback_events: VecDeque::new(),
        source_normalizer_packet_session: None,
    };
    let fallback = adapter
        .runtime_fallback
        .take()
        .expect("runtime fallback config should exist");

    adapter
        .activate_runtime_fallback_with(
            "forced presenter failure",
            fallback,
            |_source, _options| Ok(test_fallback_bootstrap_with_plugin_diagnostic()),
        )
        .expect("runtime fallback should succeed");

    let diagnostic = adapter
        .plugin_diagnostics
        .iter()
        .find(|diagnostic| diagnostic.plugin_name.as_deref() == Some("fixture-decoder"))
        .expect("fallback diagnostic should be preserved");
    assert_eq!(
        diagnostic.participation,
        PlayerPluginParticipation::Fallback
    );
    assert!(
        diagnostic
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("forced presenter failure")
    );
    assert!(diagnostic.details.iter().any(|detail| {
        detail.key == "fallbackReason" && detail.value.contains("forced presenter failure")
    }));

    let initialized = adapter.drain_events().into_iter().find_map(|event| {
        if let PlayerRuntimeEvent::Initialized(startup) = event {
            Some(startup)
        } else {
            None
        }
    });
    let startup = initialized.expect("fallback runtime should emit initialized diagnostics");
    assert!(startup.plugin_diagnostics.iter().any(|diagnostic| {
        diagnostic.plugin_name.as_deref() == Some("fixture-decoder")
            && diagnostic.participation == PlayerPluginParticipation::Fallback
            && diagnostic
                .details
                .iter()
                .any(|detail| detail.key == "fallbackReason")
    }));
}

#[test]
fn runtime_dispatch_seek_backend_failure_falls_back_to_software_runtime() {
    let native_runtime = Box::new(FakeStrategyRuntime {
        capabilities: PlayerRuntimeAdapterCapabilities {
            adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
            supports_audio_output: true,
            supports_frame_output: false,
            supports_external_video_surface: true,
            supports_seek: true,
            supports_stop: true,
            supports_playback_rate: true,
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(3.0),
            natural_playback_rate_max: Some(2.0),
            supports_hardware_decode: true,
            supports_streaming: true,
            supports_hdr: true,
        },
        media_info: media_info_with_codec("H264"),
        playback_rate: 1.25,
        progress: PlaybackProgress::new(Duration::from_secs(2), Some(Duration::from_secs(30))),
        state: PresentationState::Playing,
        events: VecDeque::new(),
        advance_error: None,
        dispatch_error: Some(PlayerError::new(
            PlayerErrorCode::BackendFailure,
            "forced seek failure",
        )),
    });
    let mut adapter = MacosRuntimeAdapter {
        inner: native_runtime,
        video_decode: PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Hardware,
            hardware_available: true,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: None,
        },
        plugin_diagnostics: Vec::new(),
        has_video_surface: true,
        runtime_fallback: Some(MacosRuntimeActiveFallback {
            source: MediaSource::new("fixture.mp4"),
            options: PlayerRuntimeOptions::default(),
            fallback_reason:
                "native-frame runtime failed during playback; selected softwareDecoder route"
                    .to_owned(),
        }),
        pending_runtime_fallback_events: VecDeque::new(),
        source_normalizer_packet_session: None,
    };
    let fallback = adapter
        .runtime_fallback
        .take()
        .expect("runtime fallback config should exist");
    let result = adapter
        .activate_runtime_fallback_with("forced seek failure", fallback, |_source, _options| {
            Ok(test_fallback_bootstrap())
        })
        .and_then(|()| {
            adapter.dispatch(PlayerRuntimeCommand::SeekTo {
                position: Duration::from_secs(7),
            })
        })
        .expect("dispatch should succeed after fallback");

    assert!(result.applied);
    assert!(adapter.inner.capabilities().supports_frame_output);
    assert!(!adapter.inner.capabilities().supports_external_video_surface);
    assert_eq!(adapter.progress().position(), Duration::from_secs(7));
    assert_eq!(adapter.playback_rate(), 1.25);
    assert_eq!(adapter.presentation_state(), PresentationState::Playing);
}

#[test]
fn runtime_dispatch_play_and_rate_backend_failure_fall_back_to_software_runtime() {
    for command in [
        PlayerRuntimeCommand::Play,
        PlayerRuntimeCommand::SetPlaybackRate { rate: 1.75 },
    ] {
        let mut adapter = MacosRuntimeAdapter {
            inner: Box::new(FakeStrategyRuntime {
                capabilities: PlayerRuntimeAdapterCapabilities {
                    adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                    backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
                    supports_audio_output: true,
                    supports_frame_output: false,
                    supports_external_video_surface: true,
                    supports_seek: true,
                    supports_stop: true,
                    supports_playback_rate: true,
                    playback_rate_min: Some(0.5),
                    playback_rate_max: Some(3.0),
                    natural_playback_rate_max: Some(2.0),
                    supports_hardware_decode: true,
                    supports_streaming: true,
                    supports_hdr: true,
                },
                media_info: media_info_with_codec("H264"),
                playback_rate: 1.25,
                progress: PlaybackProgress::new(
                    Duration::from_secs(2),
                    Some(Duration::from_secs(30)),
                ),
                state: PresentationState::Paused,
                events: VecDeque::new(),
                advance_error: None,
                dispatch_error: Some(PlayerError::new(
                    PlayerErrorCode::BackendFailure,
                    match command {
                        PlayerRuntimeCommand::Play => "forced play failure",
                        PlayerRuntimeCommand::SetPlaybackRate { .. } => "forced rate failure",
                        _ => unreachable!(),
                    },
                )),
            }),
            video_decode: PlayerVideoDecodeInfo {
                selected_mode: PlayerVideoDecodeMode::Hardware,
                hardware_available: true,
                hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
                fallback_reason: None,
            },
            plugin_diagnostics: Vec::new(),
            has_video_surface: true,
            runtime_fallback: Some(MacosRuntimeActiveFallback {
                source: MediaSource::new("fixture.mp4"),
                options: PlayerRuntimeOptions::default(),
                fallback_reason:
                    "native-frame runtime failed during playback; selected softwareDecoder route"
                        .to_owned(),
            }),
            pending_runtime_fallback_events: VecDeque::new(),
            source_normalizer_packet_session: None,
        };
        let fallback = adapter
            .runtime_fallback
            .take()
            .expect("runtime fallback config should exist");

        let result = adapter
            .activate_runtime_fallback_with(
                match command {
                    PlayerRuntimeCommand::Play => "forced play failure",
                    PlayerRuntimeCommand::SetPlaybackRate { .. } => "forced rate failure",
                    _ => unreachable!(),
                },
                fallback,
                |_source, _options| Ok(test_fallback_bootstrap()),
            )
            .and_then(|()| adapter.dispatch(command.clone()))
            .expect("dispatch should succeed after fallback");

        assert!(result.applied);
        assert!(adapter.inner.capabilities().supports_frame_output);
        assert!(!adapter.inner.capabilities().supports_external_video_surface);
    }
}

#[test]
fn runtime_fallback_trigger_only_matches_expected_paths() {
    assert!(should_trigger_runtime_fallback_for_advance(
        &PlayerError::new(
            PlayerErrorCode::BackendFailure,
            "failed to present decoded video frame"
        )
    ));
    assert!(should_trigger_runtime_fallback_for_advance(
        &PlayerError::new(
            PlayerErrorCode::BackendFailure,
            "failed to present seeked video frame"
        )
    ));
    assert!(!should_trigger_runtime_fallback_for_advance(
        &PlayerError::new(
            PlayerErrorCode::BackendFailure,
            "failed to decode audio stream"
        )
    ));
    assert!(should_trigger_runtime_fallback_for_advance(
        &PlayerError::new(
            PlayerErrorCode::BackendFailure,
            "native-frame decoder state is poisoned"
        )
    ));
    assert!(!should_trigger_runtime_fallback_for_advance(
        &PlayerError::new(
            PlayerErrorCode::SeekFailure,
            "failed to present decoded video frame"
        )
    ));
    assert!(should_trigger_runtime_fallback_for_command(
        &PlayerRuntimeCommand::SeekTo {
            position: Duration::from_secs(1)
        },
        &PlayerError::new(PlayerErrorCode::BackendFailure, "forced seek failure")
    ));
    assert!(should_trigger_runtime_fallback_for_command(
        &PlayerRuntimeCommand::Play,
        &PlayerError::new(PlayerErrorCode::BackendFailure, "forced play failure")
    ));
    assert!(should_trigger_runtime_fallback_for_command(
        &PlayerRuntimeCommand::SetPlaybackRate { rate: 1.5 },
        &PlayerError::new(PlayerErrorCode::BackendFailure, "forced rate failure")
    ));
    assert!(!should_trigger_runtime_fallback_for_command(
        &PlayerRuntimeCommand::Pause,
        &PlayerError::new(PlayerErrorCode::BackendFailure, "forced pause failure")
    ));
    assert!(!should_trigger_runtime_fallback_for_command(
        &PlayerRuntimeCommand::Stop,
        &PlayerError::new(PlayerErrorCode::BackendFailure, "forced stop failure")
    ));
}

#[test]
fn runtime_dispatch_pause_and_stop_do_not_trigger_fallback() {
    for command in [PlayerRuntimeCommand::Pause, PlayerRuntimeCommand::Stop] {
        let mut adapter = MacosRuntimeAdapter {
            inner: Box::new(FakeStrategyRuntime {
                capabilities: PlayerRuntimeAdapterCapabilities {
                    adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                    backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
                    supports_audio_output: true,
                    supports_frame_output: false,
                    supports_external_video_surface: true,
                    supports_seek: true,
                    supports_stop: true,
                    supports_playback_rate: true,
                    playback_rate_min: Some(0.5),
                    playback_rate_max: Some(3.0),
                    natural_playback_rate_max: Some(2.0),
                    supports_hardware_decode: true,
                    supports_streaming: true,
                    supports_hdr: true,
                },
                media_info: media_info_with_codec("H264"),
                playback_rate: 1.0,
                progress: PlaybackProgress::new(
                    Duration::from_secs(2),
                    Some(Duration::from_secs(30)),
                ),
                state: PresentationState::Playing,
                events: VecDeque::new(),
                advance_error: None,
                dispatch_error: Some(PlayerError::new(
                    PlayerErrorCode::BackendFailure,
                    match command {
                        PlayerRuntimeCommand::Pause => "forced pause failure",
                        PlayerRuntimeCommand::Stop => "forced stop failure",
                        _ => unreachable!(),
                    },
                )),
            }),
            video_decode: PlayerVideoDecodeInfo {
                selected_mode: PlayerVideoDecodeMode::Hardware,
                hardware_available: true,
                hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
                fallback_reason: None,
            },
            plugin_diagnostics: Vec::new(),
            has_video_surface: true,
            runtime_fallback: Some(MacosRuntimeActiveFallback {
                source: MediaSource::new("fixture.mp4"),
                options: PlayerRuntimeOptions::default(),
                fallback_reason:
                    "native-frame runtime failed during playback; selected softwareDecoder route"
                        .to_owned(),
            }),
            pending_runtime_fallback_events: VecDeque::new(),
            source_normalizer_packet_session: None,
        };

        let error = adapter
            .dispatch(command)
            .expect_err("pause/stop should not fallback");
        assert_eq!(error.code(), PlayerErrorCode::BackendFailure);
        assert!(adapter.runtime_fallback.is_some());
        assert!(adapter.inner.capabilities().supports_external_video_surface);
    }
}

#[test]
fn macos_video_decode_info_marks_h264_as_hardware_candidate() {
    let info = macos_video_decode_info(&media_info_with_codec("H264"));

    assert_eq!(info.selected_mode, PlayerVideoDecodeMode::Software);
    assert_eq!(
        info.hardware_backend.as_deref(),
        Some(VIDEOTOOLBOX_BACKEND_NAME)
    );
    assert!(info.fallback_reason.is_some());
}

#[test]
fn macos_video_decode_info_marks_unknown_codec_as_software_only() {
    let info = macos_video_decode_info(&media_info_with_codec("VP8"));

    assert_eq!(info.selected_mode, PlayerVideoDecodeMode::Software);
    assert!(!info.hardware_available);
    assert_eq!(
        info.hardware_backend.as_deref(),
        Some(VIDEOTOOLBOX_BACKEND_NAME)
    );
    assert!(
        info.fallback_reason
            .as_deref()
            .unwrap_or_default()
            .contains("VP8")
    );
}

#[test]
fn macos_video_decode_info_without_plugin_paths_keeps_fallback_clean() {
    let media_info = media_info_with_codec("fixture-video");
    let info = apply_decoder_plugin_diagnostics_to_video_decode(
        macos_video_decode_info(&media_info),
        &media_info,
        &PlayerRuntimeOptions::default(),
    );

    assert!(
        !info
            .fallback_reason
            .as_deref()
            .unwrap_or_default()
            .contains("decoder plugin")
    );
}

#[test]
fn macos_video_decode_info_records_configured_decoder_plugin_paths() {
    let media_info = media_info_with_codec("fixture-video");
    let info = apply_decoder_plugin_diagnostics_to_video_decode(
        macos_video_decode_info(&media_info),
        &media_info,
        &PlayerRuntimeOptions::default()
            .with_decoder_plugin_library_paths([PathBuf::from("/tmp/missing-decoder-plugin")]),
    );

    assert!(
        info.fallback_reason
            .as_deref()
            .unwrap_or_default()
            .contains("decoder plugin paths configured")
    );
    let fallback = info.fallback_reason.as_deref().unwrap_or_default();
    assert!(fallback.contains("/tmp/missing-decoder-plugin"));
    assert!(!fallback.contains("failed to open plugin library"));
    assert!(!fallback.contains("dlopen"));
}

#[test]
fn macos_native_frame_preference_without_plugin_paths_reports_software_route() {
    let media_info = media_info_with_codec("H264");
    let info = super::apply_native_frame_plugin_preference_to_video_decode(
        macos_video_decode_info(&media_info),
        &media_info,
        &PlayerRuntimeOptions::default()
            .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame),
    );

    let fallback = info.fallback_reason.as_deref().unwrap_or_default();
    assert!(fallback.contains("selected softwareDecoder route"));
    assert!(fallback.contains("no decoder plugin paths are configured"));
}

#[test]
fn macos_native_frame_preference_without_surface_reports_software_route() {
    let media_info = media_info_with_codec("H264");
    let info = super::apply_native_frame_plugin_preference_to_video_decode(
        macos_video_decode_info(&media_info),
        &media_info,
        &PlayerRuntimeOptions::default()
            .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame)
            .with_decoder_plugin_library_paths([PathBuf::from("fixture-decoder")]),
    );

    let fallback = info.fallback_reason.as_deref().unwrap_or_default();
    assert!(fallback.contains("selected softwareDecoder route"));
    assert!(fallback.contains("no macOS video surface is available"));
}

#[test]
fn macos_startup_records_decoder_plugin_registry_diagnostics() {
    let media_info = media_info_with_codec("fixture-video");
    let startup = apply_decoder_plugin_diagnostics(
        startup_with_video_decode(macos_video_decode_info(&media_info)),
        &media_info,
        &PlayerRuntimeOptions::default()
            .with_decoder_plugin_library_paths([PathBuf::from("/tmp/missing-decoder-plugin")]),
    );

    assert_eq!(startup.plugin_diagnostics.len(), 1);
    assert_eq!(
        startup.plugin_diagnostics[0].status,
        PlayerPluginDiagnosticStatus::LoadFailed
    );
    assert!(
        startup.plugin_diagnostics[0]
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("failed to open plugin library")
    );
    assert!(
        startup
            .video_decode
            .as_ref()
            .and_then(|info| info.fallback_reason.as_deref())
            .unwrap_or_default()
            .contains("decoder plugin paths configured")
    );
}

#[test]
fn macos_source_normalizer_disabled_keeps_original_source() {
    let original = MediaSource::new("file:///tmp/original.flv");
    let outcome = prepare_source_normalizer_for_open(
        original.clone(),
        &PlayerRuntimeOptions::default().with_source_normalizer_plugin_library_paths([
            PathBuf::from("/tmp/missing-source-normalizer"),
        ]),
    )
    .expect("disabled source normalizer should not inspect plugin paths");

    assert_eq!(outcome.source.uri(), original.uri());
    assert!(outcome.packet_session.is_none());
    assert!(outcome.diagnostics.is_empty());
}

#[test]
fn macos_source_normalizer_prefer_missing_plugin_falls_back_with_diagnostics() {
    let outcome = prepare_source_normalizer_for_open(
        MediaSource::new("file:///tmp/original.flv"),
        &PlayerRuntimeOptions::default()
            .with_source_normalizer_plugin_library_paths([PathBuf::from(
                "/tmp/missing-source-normalizer",
            )])
            .with_source_normalizer_mode(SourceNormalizerMode::PreferNormalized),
    )
    .expect("prefer mode should fall back when a plugin is missing");

    assert_eq!(outcome.source.uri(), "file:///tmp/original.flv");
    assert!(outcome.packet_session.is_none());
    assert!(outcome.diagnostics.iter().any(|diagnostic| {
        diagnostic.status == PlayerPluginDiagnosticStatus::LoadFailed
            && diagnostic
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("failed to open plugin library")
    }));
}

#[test]
fn macos_source_normalizer_skips_native_adaptive_sources() {
    let original = MediaSource::new("https://example.test/live/master.m3u8");
    let outcome = prepare_source_normalizer_for_open(
        original.clone(),
        &PlayerRuntimeOptions::default()
            .with_source_normalizer_plugin_library_paths([PathBuf::from(
                "/tmp/missing-source-normalizer",
            )])
            .with_source_normalizer_mode(SourceNormalizerMode::RequireNormalized),
    )
    .expect("native adaptive sources should bypass packet source normalization");

    assert_eq!(outcome.source.uri(), original.uri());
    assert!(outcome.packet_session.is_none());
    assert!(outcome.packet_stream_info.is_none());
    assert_eq!(outcome.diagnostics.len(), 1);
    assert!(
        outcome.diagnostics[0]
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("skipped for HLS adaptive source")
    );
    assert!(
        outcome.diagnostics[0]
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("selected systemPlayer route")
    );
}

#[test]
fn macos_source_normalizer_require_missing_plugin_fails() {
    let result = prepare_source_normalizer_for_open(
        MediaSource::new("file:///tmp/original.flv"),
        &PlayerRuntimeOptions::default()
            .with_source_normalizer_plugin_library_paths([PathBuf::from(
                "/tmp/missing-source-normalizer",
            )])
            .with_source_normalizer_mode(SourceNormalizerMode::RequireNormalized),
    );
    let error = match result {
        Ok(_) => panic!("require mode should fail when no plugin is available"),
        Err(error) => error,
    };

    assert_eq!(error.code(), PlayerErrorCode::Unsupported);
    assert!(
        error
            .message()
            .contains("no supported source normalizer plugin")
    );
}

#[test]
fn macos_source_normalizer_diagnostics_are_attached_once_opened() {
    let normalization = MacosSourceNormalizationOutcome {
        source: MediaSource::new("/tmp/normalized.mp4"),
        packet_session: None,
        packet_stream_info: None,
        diagnostics: vec![PlayerPluginDiagnostic {
            path: String::new(),
            plugin_name: Some("fixture-normalizer".to_owned()),
            plugin_kind: Some("source_normalizer".to_owned()),
            status: PlayerPluginDiagnosticStatus::Loaded,
            message: Some("source normalizer selected profile fixture".to_owned()),
            capability: None,
            participation: PlayerPluginParticipation::Participated,
            details: Vec::new(),
        }],
        selected_profile: Some("fixture".to_owned()),
        normalized_endpoint: Some("/tmp/normalized.mp4".to_owned()),
        ready_latency: Some(Duration::from_millis(7)),
    };
    let startup = apply_source_normalizer_open_diagnostics(
        startup_with_video_decode(macos_video_decode_info(&media_info_with_codec("H264"))),
        &normalization,
    );

    assert!(startup.plugin_diagnostics.iter().any(|diagnostic| {
        diagnostic.plugin_kind.as_deref() == Some("source_normalizer")
            && diagnostic
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("selected profile")
    }));
}

#[test]
fn macos_source_normalizer_packet_stream_is_participated_after_decoder_handoff() {
    let stream_info = fake_source_normalizer_packet_stream_info_with_audio("H264", "aac");
    let packet_summary = source_normalizer_packet_stream_summary(&stream_info);
    assert!(packet_summary.contains("tracks video=1 audio=1"));
    assert!(packet_summary.contains("selectedVideoStreamIndex=0"));
    assert!(packet_summary.contains("selectedVideoMediaKind=video"));
    assert!(packet_summary.contains("selectedVideoCodec=H264"));
    assert!(packet_summary.contains("audioStreamIndex=1"));
    assert!(packet_summary.contains("audioMediaKind=audio"));
    assert!(packet_summary.contains("audioTrackCodec=aac"));
    assert!(packet_summary.contains("seekable=true"));
    assert!(packet_summary.contains("durationMs=1000"));
    assert!(packet_summary.contains("route=sdkManagedNativeFrame"));
    let packet_details = source_normalizer_packet_stream_details(&stream_info);
    assert!(
        packet_details
            .iter()
            .any(|detail| { detail.key == "selectedVideoStreamIndex" && detail.value == "0" })
    );
    assert!(
        packet_details
            .iter()
            .any(|detail| { detail.key == "selectedVideoMediaKind" && detail.value == "video" })
    );
    assert!(
        packet_details
            .iter()
            .any(|detail| { detail.key == "audioStreamIndex" && detail.value == "1" })
    );
    assert!(
        packet_details
            .iter()
            .any(|detail| { detail.key == "audioMediaKind" && detail.value == "audio" })
    );
    assert!(
        packet_details
            .iter()
            .any(|detail| { detail.key == "route" && detail.value == "sdkManagedNativeFrame" })
    );
    let packet_summary_without_audio_decoder =
        source_normalizer_packet_stream_summary_with_audio_decoder_readiness(&stream_info, None);
    assert!(packet_summary_without_audio_decoder.contains("audioDecoderPlugin=none"));
    assert!(packet_summary_without_audio_decoder.contains("audioDecoderPluginReady=false"));
    assert!(packet_summary_without_audio_decoder.contains("audioDecoder=none"));
    let audio_decoder_registry =
        PluginRegistry::from_records(vec![decoder_audio_pcm_plugin_record(
            "aac",
            "pcm-audio-decoder",
        )]);
    let packet_summary_with_audio_decoder =
        source_normalizer_packet_stream_summary_with_audio_decoder_readiness(
            &stream_info,
            Some(&audio_decoder_registry),
        );
    assert!(packet_summary_with_audio_decoder.contains("audioDecoderPlugin=pcm-audio-decoder"));
    assert!(packet_summary_with_audio_decoder.contains("audioDecoderPluginReady=true"));
    assert!(packet_summary_with_audio_decoder.contains("audioDecoder=none"));
    let no_audio_summary = source_normalizer_packet_stream_summary_with_audio_decoder_readiness(
        &fake_source_normalizer_packet_stream_info("H264"),
        Some(&audio_decoder_registry),
    );
    assert!(no_audio_summary.contains("audioTrackCodec=none"));
    assert!(no_audio_summary.contains("audioDecoderPlugin=none"));
    assert!(no_audio_summary.contains("audioDecoderPluginReady=false"));
    let mut normalization = MacosSourceNormalizationOutcome {
        source: MediaSource::new("/tmp/normalized.mp4"),
        packet_session: Some(Arc::new(Mutex::new(Some(Box::new(
            FakeSourceNormalizerPacketSession::new(stream_info.clone()),
        ))))),
        packet_stream_info: Some(stream_info.clone()),
        diagnostics: vec![PlayerPluginDiagnostic {
            path: String::new(),
            plugin_name: Some("fixture-normalizer".to_owned()),
            plugin_kind: Some("source_normalizer".to_owned()),
            status: PlayerPluginDiagnosticStatus::Loaded,
            message: Some(
                format!(
                    "source normalizer selected profile fixture via fixture-normalizer; ready in 1 ms; output packet_stream; {packet_summary}; waiting for sdkManagedNativeFrame decoder handoff"
                )
                    .to_owned(),
            ),
            capability: None,
            participation: PlayerPluginParticipation::Selected,
            details: source_normalizer_packet_stream_details(&stream_info),
        }],
        selected_profile: Some("fixture".to_owned()),
        normalized_endpoint: Some("vesper-source-normalizer-packet://fixture".to_owned()),
        ready_latency: Some(Duration::from_millis(1)),
    };

    mark_source_normalizer_packet_stream_participated(
        &mut normalization,
        Some("fixture-videotoolbox"),
    );

    let diagnostic = normalization
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.plugin_kind.as_deref() == Some("source_normalizer"))
        .expect("source normalizer diagnostic");
    assert_eq!(
        diagnostic.participation,
        PlayerPluginParticipation::Participated
    );
    assert!(
        diagnostic
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("tracks video=1 audio=1")
    );
    assert!(
        diagnostic
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("audioTrackCodec=aac")
    );
    assert!(
        diagnostic
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("selectedVideoStreamIndex=0")
    );
    assert!(
        diagnostic
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("handed to fixture-videotoolbox")
    );
    assert!(
        diagnostic
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("macOS sdkManagedNativeFrame presenter")
    );
}

#[test]
fn macos_source_normalizer_session_guard_keeps_runtime_source() {
    let stream_info = fake_source_normalizer_packet_stream_info("H264");
    let bootstrap = PlayerRuntime::from_adapter_bootstrap(
        MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(FakeStrategyRuntime {
                capabilities: default_software_capabilities(),
                media_info: media_info_with_source_uri("/tmp/normalized.mp4", "H264"),
                playback_rate: 1.0,
                progress: PlaybackProgress::new(Duration::ZERO, None),
                state: PresentationState::Ready,
                events: VecDeque::new(),
                advance_error: None,
                dispatch_error: None,
            }),
            initial_frame: None,
            startup: startup_with_video_decode(macos_video_decode_info(&media_info_with_codec(
                "H264",
            ))),
        },
    );
    let bootstrap = attach_source_normalizer_to_runtime(
        bootstrap,
        MacosSourceNormalizationOutcome {
            source: MediaSource::new("/tmp/normalized.mp4"),
            packet_session: Some(Arc::new(Mutex::new(Some(Box::new(
                FakeSourceNormalizerPacketSession::new(stream_info),
            ))))),
            packet_stream_info: None,
            diagnostics: Vec::new(),
            selected_profile: Some("fixture".to_owned()),
            normalized_endpoint: Some("/tmp/normalized.mp4".to_owned()),
            ready_latency: Some(Duration::from_millis(1)),
        },
    );

    assert_eq!(bootstrap.runtime.source_uri(), "/tmp/normalized.mp4");
}

#[test]
fn macos_source_normalizer_packet_decoder_requires_strict_decoder_inputs() {
    let stream_info = fake_source_normalizer_packet_stream_info("H264");
    let normalization = MacosSourceNormalizationOutcome {
        source: MediaSource::new("file:///tmp/original.mp4"),
        packet_session: Some(Arc::new(Mutex::new(Some(Box::new(
            FakeSourceNormalizerPacketSession::new(stream_info.clone()),
        ))))),
        packet_stream_info: Some(stream_info),
        diagnostics: Vec::new(),
        selected_profile: Some("fixture-packet".to_owned()),
        normalized_endpoint: Some("vesper-source-normalizer-packet://fake-session".to_owned()),
        ready_latency: Some(Duration::from_millis(1)),
    };

    let message = source_normalizer_packet_decoder_unavailable_message(
        &normalization,
        &PlayerRuntimeOptions::default()
            .with_source_normalizer_mode(SourceNormalizerMode::RequireNormalized),
    )
    .expect("missing decoder mode should produce diagnostics");

    assert!(message.contains("requires native-frame decoder plugin mode"));
}

#[test]
fn macos_source_normalizer_options_are_cleared_for_fallback_reopen() {
    let options = PlayerRuntimeOptions::default()
        .with_source_normalizer_plugin_library_paths([PathBuf::from("plugin")])
        .with_source_normalizer_mode(SourceNormalizerMode::PreferNormalized);
    let cleared = without_source_normalizer_options(options);

    assert_eq!(
        cleared.source_normalizer_mode,
        SourceNormalizerMode::Disabled
    );
    assert!(cleared.source_normalizer_plugin_library_paths.is_empty());
}

#[test]
#[ignore = "requires a built player-decoder-fixture shared library artifact"]
fn macos_runtime_diagnostics_loads_real_decoder_fixture_library() {
    let Some(plugin_path) = std::env::var_os("VESPER_DECODER_PLUGIN_PATHS")
        .and_then(|paths| std::env::split_paths(&paths).next())
    else {
        eprintln!(
            "skipping decoder fixture diagnostics test: VESPER_DECODER_PLUGIN_PATHS is not set"
        );
        return;
    };
    if !plugin_path.is_file() {
        eprintln!(
            "skipping decoder fixture diagnostics test: plugin path is missing: {}",
            plugin_path.display()
        );
        return;
    }

    for codec in ["fixture-video", "H264", "HEVC"] {
        let media_info = media_info_with_codec(codec);
        let diagnostics = macos_runtime_diagnostics(
            &media_info,
            &PlayerRuntimeOptions::default()
                .with_decoder_plugin_library_paths([plugin_path.clone()]),
        );

        assert_eq!(diagnostics.plugin_diagnostics.len(), 1);
        assert_eq!(
            diagnostics.plugin_diagnostics[0].status,
            PlayerPluginDiagnosticStatus::DecoderSupported
        );
        assert_eq!(
            diagnostics.plugin_diagnostics[0].plugin_name.as_deref(),
            Some("player-decoder-fixture")
        );
        let fallback = diagnostics
            .video_decode
            .fallback_reason
            .as_deref()
            .unwrap_or_default();
        assert!(fallback.contains(codec));
        assert!(fallback.contains("diagnostic-only"));
    }
}

#[test]
#[ignore = "requires a built player-decoder-videotoolbox shared library artifact"]
fn macos_runtime_diagnostics_loads_real_videotoolbox_decoder_library() {
    let Some(plugin_path) =
        std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
    else {
        eprintln!(
            "skipping VideoToolbox decoder diagnostics test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
        );
        return;
    };
    if !plugin_path.is_file() {
        eprintln!(
            "skipping VideoToolbox decoder diagnostics test: plugin path is missing: {}",
            plugin_path.display()
        );
        return;
    }

    for codec in ["H264", "HEVC"] {
        let media_info = media_info_with_codec(codec);
        let diagnostics = macos_runtime_diagnostics(
            &media_info,
            &PlayerRuntimeOptions::default()
                .with_decoder_plugin_library_paths([plugin_path.clone()]),
        );

        assert_eq!(diagnostics.plugin_diagnostics.len(), 1);
        let diagnostic = &diagnostics.plugin_diagnostics[0];
        assert_eq!(
            diagnostic.status,
            PlayerPluginDiagnosticStatus::DecoderSupported
        );
        assert_eq!(
            diagnostic.plugin_name.as_deref(),
            Some("player-decoder-videotoolbox")
        );
        assert!(matches!(
            diagnostic.capability.as_ref(),
            Some(PlayerPluginCapabilitySummary::Decoder(capabilities))
                if capabilities.supports_native_frame_output
        ));
        let fallback = diagnostics
            .video_decode
            .fallback_reason
            .as_deref()
            .unwrap_or_default();
        assert!(fallback.contains("player-decoder-videotoolbox native-frame"));
    }
}

#[test]
#[ignore = "requires a built player-decoder-videotoolbox shared library and a local H264/HEVC source"]
fn macos_videotoolbox_decoder_decodes_ffmpeg_packets_headless() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let Some(plugin_path) =
        std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
    else {
        eprintln!(
            "skipping VideoToolbox packet decode test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
        );
        return;
    };
    if !plugin_path.is_file() {
        eprintln!(
            "skipping VideoToolbox packet decode test: plugin path is missing: {}",
            plugin_path.display()
        );
        return;
    }
    let Some(source) = videotoolbox_smoke_source_path() else {
        eprintln!(
            "skipping VideoToolbox packet decode test: no local H264/HEVC smoke source found"
        );
        return;
    };

    let backend = FfmpegBackend::new().expect("FFmpeg should initialize");
    let mut packet_source = backend
        .open_video_packet_source(MediaSource::new(source.clone()))
        .unwrap_or_else(|error| panic!("failed to open packet source `{source}`: {error}"));
    let stream_info = packet_source.stream_info().clone();
    let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load VideoToolbox decoder plugin `{}`: {error}",
            plugin_path.display()
        )
    });
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("VideoToolbox plugin should export a native decoder factory");
    if !factory
        .capabilities()
        .supports_codec(&stream_info.codec, DecoderMediaKind::Video)
    {
        eprintln!(
            "skipping VideoToolbox packet decode test: source codec {} is not supported",
            stream_info.codec
        );
        return;
    }

    let mut session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: stream_info.codec.clone(),
            media_kind: DecoderMediaKind::Video,
            extradata: stream_info.extradata.clone(),
            width: stream_info.width,
            height: stream_info.height,
            prefer_hardware: true,
            require_cpu_output: false,
            ..DecoderSessionConfig::default()
        })
        .expect("VideoToolbox native session should open");

    let mut submitted_packets = 0usize;
    let mut accepted_packets = 0usize;
    let mut decoded_frames = 0usize;
    let mut decoded_pts = Vec::new();
    while submitted_packets < 120 {
        let Some(packet) = packet_source
            .next_packet()
            .expect("packet demux should succeed")
        else {
            break;
        };
        submitted_packets += 1;
        let send_result = session
            .send_packet(
                &DecoderPacket {
                    pts_us: packet.pts_us,
                    dts_us: packet.dts_us,
                    duration_us: packet.duration_us,
                    stream_index: packet.stream_index,
                    media_kind: DecoderMediaKind::Video,
                    key_frame: packet.key_frame,
                    discontinuity: packet.discontinuity,
                    end_of_stream: false,
                },
                &packet.data,
            )
            .expect("VideoToolbox should accept compressed packet");
        if !send_result.accepted {
            continue;
        }
        accepted_packets += 1;

        loop {
            match session
                .receive_native_frame()
                .expect("VideoToolbox frame receive should succeed")
            {
                DecoderReceiveNativeFrameOutput::Frame(frame) => {
                    assert_eq!(
                        frame.metadata.handle_kind,
                        DecoderNativeHandleKind::CvPixelBuffer
                    );
                    assert!(frame.handle != 0);
                    assert!(frame.metadata.width > 0);
                    assert!(frame.metadata.height > 0);
                    decoded_pts.push(frame.metadata.pts_us);
                    session
                        .release_native_frame(frame)
                        .expect("native frame release should succeed");
                    decoded_frames += 1;
                }
                DecoderReceiveNativeFrameOutput::NeedMoreInput => break,
                DecoderReceiveNativeFrameOutput::Eof => break,
            }
        }
    }

    assert!(
        decoded_frames > 0,
        "VideoToolbox did not produce a CVPixelBuffer after {submitted_packets} packets from {source}"
    );
    assert!(
        decoded_frames >= accepted_packets.saturating_sub(2),
        "VideoToolbox output was sparse for {source}: decoded {decoded_frames} frames from {accepted_packets} accepted packets; pts={decoded_pts:?}"
    );
    assert!(
        decoded_pts
            .iter()
            .flatten()
            .any(|pts| *pts > 0 && *pts < 1_000_000),
        "VideoToolbox output did not include non-keyframe-era PTS values from the first second: pts={decoded_pts:?}"
    );
}

#[test]
#[ignore = "requires a built player-decoder-videotoolbox shared library and a local H264/HEVC source"]
fn macos_videotoolbox_decoder_flush_seek_and_eof_headless() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let Some(plugin_path) =
        std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
    else {
        eprintln!(
            "skipping VideoToolbox lifecycle test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
        );
        return;
    };
    if !plugin_path.is_file() {
        eprintln!(
            "skipping VideoToolbox lifecycle test: plugin path is missing: {}",
            plugin_path.display()
        );
        return;
    }
    let Some(source) = videotoolbox_smoke_source_path() else {
        eprintln!("skipping VideoToolbox lifecycle test: no local H264/HEVC smoke source found");
        return;
    };

    let (mut packet_source, mut session) =
        open_videotoolbox_smoke_packet_source_and_session(&plugin_path, &source);
    assert!(
        decode_one_videotoolbox_frame(packet_source.as_mut(), session.as_mut(), 120),
        "VideoToolbox should decode a frame before flush/seek"
    );

    session.flush().expect("VideoToolbox flush should succeed");
    packet_source
        .seek_to(Duration::from_millis(0))
        .expect("packet source seek should succeed after flush");
    assert!(
        decode_one_videotoolbox_frame(packet_source.as_mut(), session.as_mut(), 120),
        "VideoToolbox should decode a frame after flush/seek"
    );

    drain_videotoolbox_session_to_eof(packet_source.as_mut(), session.as_mut())
        .expect("VideoToolbox should report EOF after packet drain");
    session.close().expect("VideoToolbox session should close");
}

#[test]
#[ignore = "requires a built player-decoder-videotoolbox shared library and a local H264/HEVC source"]
#[cfg(target_os = "macos")]
fn macos_native_frame_decoder_plugin_runtime_probes_with_surface() {
    let Some(plugin_path) =
        std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
    else {
        eprintln!(
            "skipping native-frame runtime test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
        );
        return;
    };
    if !plugin_path.is_file() {
        eprintln!(
            "skipping native-frame runtime test: plugin path is missing: {}",
            plugin_path.display()
        );
        return;
    }
    let Some(source) = videotoolbox_smoke_source_path() else {
        eprintln!("skipping native-frame runtime test: no local H264/HEVC smoke source found");
        return;
    };

    let layer_handle = unsafe { player_macos_test_create_player_layer() };
    assert!(
        !layer_handle.is_null(),
        "test player layer handle should be created"
    );

    let options = PlayerRuntimeOptions::default()
        .with_video_surface(PlayerVideoSurfaceTarget {
            kind: PlayerVideoSurfaceKind::PlayerLayer,
            handle: layer_handle as usize,
        })
        .with_decoder_plugin_library_paths([plugin_path])
        .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame);
    let initializer = PlayerRuntimeInitializer::probe_source_with_factory(
        MediaSource::new(source),
        options,
        macos_runtime_adapter_factory(),
    )
    .expect("native-frame plugin runtime should probe");

    assert!(initializer.capabilities().supports_external_video_surface);
    assert!(!initializer.capabilities().supports_frame_output);
    assert!(initializer.capabilities().supports_hardware_decode);
    assert_eq!(
        initializer
            .startup()
            .video_decode
            .as_ref()
            .map(|decode| decode.selected_mode),
        Some(PlayerVideoDecodeMode::Hardware)
    );

    unsafe {
        player_macos_test_release_object(layer_handle);
    }
}

#[test]
#[ignore = "requires built player-decoder-videotoolbox and player-frame-processor-diagnostic shared libraries plus a local H264/HEVC source"]
#[cfg(target_os = "macos")]
fn macos_native_frame_runtime_loads_frame_processor_diagnostic_plugin() {
    let Some(decoder_plugin_path) =
        std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
    else {
        eprintln!(
            "skipping native-frame frame processor test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
        );
        return;
    };
    if !decoder_plugin_path.is_file() {
        eprintln!(
            "skipping native-frame frame processor test: decoder plugin path is missing: {}",
            decoder_plugin_path.display()
        );
        return;
    }
    let Some(frame_processor_path) =
        std::env::var_os("VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH").map(PathBuf::from)
    else {
        eprintln!(
            "skipping native-frame frame processor test: VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH is not set"
        );
        return;
    };
    if !frame_processor_path.is_file() {
        eprintln!(
            "skipping native-frame frame processor test: frame processor plugin path is missing: {}",
            frame_processor_path.display()
        );
        return;
    }
    let Some(source) = videotoolbox_smoke_source_path() else {
        eprintln!(
            "skipping native-frame frame processor test: no local H264/HEVC smoke source found"
        );
        return;
    };

    let layer_handle = unsafe { player_macos_test_create_player_layer() };
    assert!(
        !layer_handle.is_null(),
        "test player layer handle should be created"
    );

    let options = PlayerRuntimeOptions::default()
        .with_video_surface(PlayerVideoSurfaceTarget {
            kind: PlayerVideoSurfaceKind::PlayerLayer,
            handle: layer_handle as usize,
        })
        .with_decoder_plugin_library_paths([decoder_plugin_path])
        .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame)
        .with_frame_processor_library_paths([frame_processor_path])
        .with_frame_processor_mode(FrameProcessorMode::PreferProcessed);
    let bootstrap = open_macos_host_runtime_source_with_options(MediaSource::new(source), options)
        .expect("macOS host runtime should open the native-frame frame processor path");
    unsafe {
        std::env::remove_var("VESPER_MACOS_TEST_FORCE_PRESENTER_FAILURE");
    }

    assert!(
        bootstrap
            .runtime
            .capabilities()
            .supports_external_video_surface
    );
    assert!(
        bootstrap
            .startup
            .plugin_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.status
                == PlayerPluginDiagnosticStatus::FrameProcessorSupported
                && diagnostic.plugin_name.as_deref() == Some("player-frame-processor-diagnostic")),
        "expected frame processor support diagnostic, got {:?}",
        bootstrap.startup.plugin_diagnostics
    );
    assert!(
        bootstrap
            .startup
            .video_decode
            .as_ref()
            .and_then(|info| info.fallback_reason.as_deref())
            .unwrap_or_default()
            .contains("selected sdkManagedNativeFrame route for VideoToolbox playback"),
        "expected native-frame decoder selection diagnostic, got {:?}",
        bootstrap.startup.video_decode
    );

    unsafe {
        player_macos_test_release_object(layer_handle);
    }
}

#[test]
#[ignore = "requires built player-decoder-videotoolbox and player-frame-processor-diagnostic shared libraries plus a local H264/HEVC source"]
#[cfg(target_os = "macos")]
fn macos_native_frame_strict_frame_processor_failure_does_not_fallback_to_software() {
    let Some(decoder_plugin_path) =
        std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
    else {
        eprintln!(
            "skipping strict frame processor fallback test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
        );
        return;
    };
    if !decoder_plugin_path.is_file() {
        eprintln!(
            "skipping strict frame processor fallback test: decoder plugin path is missing: {}",
            decoder_plugin_path.display()
        );
        return;
    }
    let Some(frame_processor_path) =
        std::env::var_os("VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH").map(PathBuf::from)
    else {
        eprintln!(
            "skipping strict frame processor fallback test: VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH is not set"
        );
        return;
    };
    if !frame_processor_path.is_file() {
        eprintln!(
            "skipping strict frame processor fallback test: frame processor plugin path is missing: {}",
            frame_processor_path.display()
        );
        return;
    }
    let Some(source) = videotoolbox_smoke_source_path() else {
        eprintln!(
            "skipping strict frame processor fallback test: no local H264/HEVC smoke source found"
        );
        return;
    };

    let layer_handle = unsafe { player_macos_test_create_player_layer() };
    assert!(
        !layer_handle.is_null(),
        "test player layer handle should be created"
    );

    let options = PlayerRuntimeOptions::default()
        .with_video_surface(PlayerVideoSurfaceTarget {
            kind: PlayerVideoSurfaceKind::PlayerLayer,
            handle: layer_handle as usize,
        })
        .with_decoder_plugin_library_paths([decoder_plugin_path])
        .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame)
        .with_frame_processor_library_paths([frame_processor_path])
        .with_frame_processor_mode(FrameProcessorMode::RequireProcessed);
    let error = match open_macos_software_runtime_source_with_options_and_interrupt(
        MediaSource::new(source),
        options,
        Arc::new(AtomicBool::new(false)),
    ) {
        Ok(_) => panic!("strict frame processor initialization should not fall back"),
        Err(error) => error,
    };
    unsafe {
        player_macos_test_release_object(layer_handle);
    }

    assert_eq!(error.code(), PlayerErrorCode::BackendFailure);
    assert!(
        error
            .message()
            .contains("frame processor initialization failed in strict mode"),
        "unexpected strict frame processor error: {}",
        error
    );
}

#[test]
#[ignore = "requires built player-decoder-videotoolbox and player-frame-processor-diagnostic shared libraries plus a local H264/HEVC source"]
#[cfg(target_os = "macos")]
fn macos_host_strict_frame_processor_failure_forwards_software_error_message() {
    let Some(decoder_plugin_path) =
        std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
    else {
        eprintln!(
            "skipping host strict frame processor error test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
        );
        return;
    };
    if !decoder_plugin_path.is_file() {
        eprintln!(
            "skipping host strict frame processor error test: decoder plugin path is missing: {}",
            decoder_plugin_path.display()
        );
        return;
    }
    let Some(frame_processor_path) =
        std::env::var_os("VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH").map(PathBuf::from)
    else {
        eprintln!(
            "skipping host strict frame processor error test: VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH is not set"
        );
        return;
    };
    if !frame_processor_path.is_file() {
        eprintln!(
            "skipping host strict frame processor error test: frame processor plugin path is missing: {}",
            frame_processor_path.display()
        );
        return;
    }
    let Some(source) = videotoolbox_smoke_source_path() else {
        eprintln!(
            "skipping host strict frame processor error test: no local H264/HEVC smoke source found"
        );
        return;
    };

    let layer_handle = unsafe { player_macos_test_create_player_layer() };
    assert!(
        !layer_handle.is_null(),
        "test player layer handle should be created"
    );

    let options = PlayerRuntimeOptions::default()
        .with_video_surface(PlayerVideoSurfaceTarget {
            kind: PlayerVideoSurfaceKind::PlayerLayer,
            handle: layer_handle as usize,
        })
        .with_decoder_plugin_library_paths([decoder_plugin_path])
        .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame)
        .with_frame_processor_library_paths([frame_processor_path])
        .with_frame_processor_mode(FrameProcessorMode::RequireProcessed);
    let error = match open_macos_host_runtime_source_with_options(MediaSource::new(source), options)
    {
        Ok(_) => {
            unsafe {
                player_macos_test_release_object(layer_handle);
            }
            panic!("strict host frame processor initialization should fail");
        }
        Err(error) => error,
    };
    unsafe {
        player_macos_test_release_object(layer_handle);
    }

    assert_eq!(error.code(), PlayerErrorCode::BackendFailure);
    assert!(
        error
            .message()
            .contains("frame processor initialization failed in strict mode"),
        "unexpected strict frame processor error: {}",
        error
    );
    assert!(
        !error.message().contains("software fallback also failed"),
        "strict frame processor error should not be wrapped as a fallback failure: {}",
        error
    );
}

#[test]
#[ignore = "requires a built player-decoder-videotoolbox shared library and a local H264/HEVC source"]
#[cfg(target_os = "macos")]
fn macos_native_frame_runtime_reopens_as_software_after_presenter_failure() {
    let Some(plugin_path) =
        std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
    else {
        eprintln!(
            "skipping native-frame reopen test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
        );
        return;
    };
    if !plugin_path.is_file() {
        eprintln!(
            "skipping native-frame reopen test: plugin path is missing: {}",
            plugin_path.display()
        );
        return;
    }
    let Some(source) = videotoolbox_smoke_source_path() else {
        eprintln!("skipping native-frame reopen test: no local H264/HEVC smoke source found");
        return;
    };

    let layer_handle = unsafe { player_macos_test_create_player_layer() };
    assert!(
        !layer_handle.is_null(),
        "test player layer handle should be created"
    );

    let options = PlayerRuntimeOptions::default()
        .with_video_surface(PlayerVideoSurfaceTarget {
            kind: PlayerVideoSurfaceKind::PlayerLayer,
            handle: layer_handle as usize,
        })
        .with_decoder_plugin_library_paths([plugin_path])
        .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame);
    let bootstrap = open_macos_software_runtime_source_with_options_and_interrupt(
        MediaSource::new(source),
        options,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("native-frame runtime open should succeed before presenter failure fallback");
    if bootstrap.runtime.capabilities().supports_frame_output
        && !bootstrap
            .runtime
            .capabilities()
            .supports_external_video_surface
    {
        assert!(
            bootstrap
                .startup
                .video_decode
                .as_ref()
                .and_then(|info| info.fallback_reason.as_deref())
                .unwrap_or_default()
                .contains("native-frame decoder plugin initialization failed"),
            "expected initialization fallback diagnostics when native-frame open falls back before presenter failure"
        );
        unsafe {
            player_macos_test_release_object(layer_handle);
        }
        return;
    }
    let mut runtime = bootstrap.runtime;
    let initial_rate = runtime.playback_rate();

    unsafe {
        std::env::set_var("VESPER_MACOS_TEST_FORCE_PRESENTER_FAILURE", "1");
    }
    let _ = runtime
        .dispatch(PlayerRuntimeCommand::Play)
        .expect("play should succeed");
    let _ = runtime
        .dispatch(PlayerRuntimeCommand::SetPlaybackRate { rate: 1.25 })
        .expect("set playback rate should succeed before fallback");
    let _ = runtime
        .dispatch(PlayerRuntimeCommand::SeekTo {
            position: Duration::ZERO,
        })
        .expect("seek should trigger presenter failure fallback instead of failing");

    assert!(runtime.capabilities().supports_frame_output);
    assert!(!runtime.capabilities().supports_external_video_surface);
    assert_eq!(runtime.presentation_state(), PresentationState::Playing);
    assert!(runtime.playback_rate() >= initial_rate);
    let resume_position = runtime.progress().position();
    let _ = runtime
        .dispatch(PlayerRuntimeCommand::SeekTo {
            position: resume_position,
        })
        .expect("seek should continue to work after fallback");
    let _ = runtime
        .dispatch(PlayerRuntimeCommand::SetPlaybackRate { rate: 1.0 })
        .expect("rate change should continue to work after fallback");
    let _ = runtime
        .dispatch(PlayerRuntimeCommand::Play)
        .expect("play should remain valid after fallback");
    let mut saw_surface_detached = false;
    let mut saw_runtime_fallback_error = false;
    let events = runtime.drain_events();
    for event in &events {
        if matches!(
            event,
            PlayerRuntimeEvent::VideoSurfaceChanged { attached: false }
        ) {
            saw_surface_detached = true;
        }
        if let PlayerRuntimeEvent::Error(error) = event
            && error.message().contains("runtime fallback activated")
        {
            saw_runtime_fallback_error = true;
        }
    }
    assert!(
        saw_surface_detached,
        "expected native surface detachment event after fallback, got {events:?}"
    );
    assert!(
        saw_runtime_fallback_error,
        "expected explicit runtime fallback error event after fallback, got {events:?}"
    );
    unsafe {
        std::env::remove_var("VESPER_MACOS_TEST_FORCE_PRESENTER_FAILURE");
    }

    unsafe {
        player_macos_test_release_object(layer_handle);
    }
}

#[test]
fn macos_software_direct_open_records_decoder_plugin_registry_diagnostics() {
    if !cfg!(target_os = "macos") {
        return;
    }

    let Some(test_video_path) = test_video_path() else {
        eprintln!(
            "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
        );
        return;
    };
    let bootstrap = open_macos_software_runtime_source_with_options_and_interrupt(
        MediaSource::new(test_video_path),
        PlayerRuntimeOptions::default()
            .with_decoder_plugin_library_paths([PathBuf::from("/tmp/missing-decoder-plugin")]),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("macos software direct open should succeed");

    assert_eq!(bootstrap.startup.plugin_diagnostics.len(), 1);
    assert_eq!(
        bootstrap.startup.plugin_diagnostics[0].status,
        PlayerPluginDiagnosticStatus::LoadFailed
    );
    assert!(
        bootstrap
            .startup
            .video_decode
            .as_ref()
            .and_then(|info| info.fallback_reason.as_deref())
            .unwrap_or_default()
            .contains("decoder plugin paths configured")
    );
}

#[test]
fn macos_decoder_plugin_registry_reports_supported_candidate_as_diagnostic_only() {
    let media_info = media_info_with_codec("fixture-video");
    let registry = PluginRegistry::from_records(vec![decoder_plugin_record(
        PluginDiagnosticStatus::DecoderSupported,
        "fixture-video",
        "fixture-decoder advertises Video fixture-video support",
    )]);
    let info = apply_decoder_plugin_registry_to_video_decode(
        macos_video_decode_info(&media_info),
        &media_info,
        &registry,
        None,
    );

    assert_eq!(info.selected_mode, PlayerVideoDecodeMode::Software);
    assert!(
        info.fallback_reason
            .as_deref()
            .unwrap_or_default()
            .contains("diagnostic-only")
    );
    assert!(
        info.fallback_reason
            .as_deref()
            .unwrap_or_default()
            .contains("fixture-decoder")
    );
    assert!(
        info.fallback_reason
            .as_deref()
            .unwrap_or_default()
            .contains("available for sdkManagedNativeFrame route")
    );
}

#[test]
fn macos_decoder_plugin_registry_labels_native_frame_candidates() {
    let media_info = media_info_with_codec("fixture-video");
    let registry = PluginRegistry::from_records(vec![decoder_native_plugin_record(
        PluginDiagnosticStatus::DecoderSupported,
        "fixture-video",
        "fixture-decoder advertises Video fixture-video support with native-frame output",
    )]);
    let info = apply_decoder_plugin_registry_to_video_decode(
        macos_video_decode_info(&media_info),
        &media_info,
        &registry,
        Some("fixture-decoder"),
    );

    assert_eq!(info.selected_mode, PlayerVideoDecodeMode::Software);
    let fallback = info.fallback_reason.as_deref().unwrap_or_default();
    assert!(fallback.contains("decoder plugin found 1/1 candidate(s)"));
    assert!(fallback.contains("fixture-decoder native-frame"));
    assert!(fallback.contains("selected sdkManagedNativeFrame route via fixture-decoder"));
    assert!(fallback.contains("audioTrack=none"));
    assert!(fallback.contains("audioDecoderPlugin=none"));
    assert!(fallback.contains("audioDecoderPluginReady=false"));
    assert!(fallback.contains("audioDecoder=none"));
    assert!(fallback.contains("audioOutput=none"));
    assert!(fallback.contains("clockSource=video"));
    assert!(fallback.contains("presenter=macOS MetalLayer"));
}

#[test]
fn macos_decoder_plugin_registry_reports_audio_decoder_readiness_when_pcm_plugin_exists() {
    let media_info = media_info_with_audio_codec("fixture-video", "aac");
    let registry = PluginRegistry::from_records(vec![
        decoder_native_plugin_record(
            PluginDiagnosticStatus::DecoderSupported,
            "fixture-video",
            "fixture-decoder advertises Video fixture-video support with native-frame output",
        ),
        decoder_audio_pcm_plugin_record("aac", "pcm-audio-decoder"),
    ]);
    let info = apply_decoder_plugin_registry_to_video_decode(
        macos_video_decode_info(&media_info),
        &media_info,
        &registry,
        Some("fixture-decoder"),
    );

    let fallback = info.fallback_reason.as_deref().unwrap_or_default();
    assert!(fallback.contains("selected sdkManagedNativeFrame route via fixture-decoder"));
    assert!(fallback.contains("audioTrackCodec=aac"));
    assert!(fallback.contains("audioDecoderPlugin=pcm-audio-decoder"));
    assert!(fallback.contains("audioDecoderPluginReady=true"));
    assert!(fallback.contains("audioDecoder=none"));
    assert!(fallback.contains("audioOutput=none"));
    assert!(fallback.contains("clockSource=video"));
}

#[test]
fn macos_decoder_plugin_registry_reports_missing_audio_pcm_plugin_for_audio_track() {
    let media_info = media_info_with_audio_codec("fixture-video", "aac");
    let registry = PluginRegistry::from_records(vec![decoder_native_plugin_record(
        PluginDiagnosticStatus::DecoderSupported,
        "fixture-video",
        "fixture-decoder advertises Video fixture-video support with native-frame output",
    )]);
    let info = apply_decoder_plugin_registry_to_video_decode(
        macos_video_decode_info(&media_info),
        &media_info,
        &registry,
        Some("fixture-decoder"),
    );

    let fallback = info.fallback_reason.as_deref().unwrap_or_default();
    assert!(fallback.contains("audioTrackCodec=aac"));
    assert!(fallback.contains("audioDecoderPlugin=none"));
    assert!(fallback.contains("audioDecoderPluginReady=false"));
    assert!(fallback.contains("audioDecoder=none"));
    assert!(fallback.contains("clockSource=video"));
}

#[test]
fn macos_decoder_plugin_registry_mismatch_does_not_change_decode_mode() {
    let media_info = media_info_with_codec("fixture-video");
    let original = macos_video_decode_info(&media_info);
    let registry = PluginRegistry::from_records(vec![decoder_plugin_record(
        PluginDiagnosticStatus::DecoderUnsupported,
        "other-video",
        "fixture-decoder does not advertise Video fixture-video support",
    )]);
    let info = apply_decoder_plugin_registry_to_video_decode(
        original.clone(),
        &media_info,
        &registry,
        None,
    );

    assert_eq!(info.selected_mode, original.selected_mode);
    assert!(
        info.fallback_reason
            .as_deref()
            .unwrap_or_default()
            .contains("0/1 supported")
    );
}

#[test]
fn macos_loader_statuses_convert_through_shared_wire_contract() {
    assert_eq!(
        runtime_status_from_loader(PluginDiagnosticStatus::Loaded),
        PlayerPluginDiagnosticStatus::Loaded
    );
    assert_eq!(
        runtime_status_from_loader(PluginDiagnosticStatus::LoadFailed),
        PlayerPluginDiagnosticStatus::LoadFailed
    );
    assert_eq!(
        runtime_status_from_loader(PluginDiagnosticStatus::UnsupportedKind),
        PlayerPluginDiagnosticStatus::UnsupportedKind
    );
    assert_eq!(
        runtime_status_from_loader(PluginDiagnosticStatus::DecoderSupported),
        PlayerPluginDiagnosticStatus::DecoderSupported
    );
    assert_eq!(
        runtime_status_from_loader(PluginDiagnosticStatus::DecoderUnsupported),
        PlayerPluginDiagnosticStatus::DecoderUnsupported
    );
    assert_eq!(
        runtime_status_from_loader(PluginDiagnosticStatus::FrameProcessorSupported),
        PlayerPluginDiagnosticStatus::FrameProcessorSupported
    );
    assert_eq!(
        runtime_status_from_loader(PluginDiagnosticStatus::FrameProcessorUnsupported),
        PlayerPluginDiagnosticStatus::FrameProcessorUnsupported
    );
    assert_eq!(
        runtime_status_from_loader(PluginDiagnosticStatus::SourceNormalizerSupported),
        PlayerPluginDiagnosticStatus::SourceNormalizerSupported
    );
    assert_eq!(
        runtime_status_from_loader(PluginDiagnosticStatus::SourceNormalizerUnsupported),
        PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported
    );
}

#[test]
fn macos_decoder_plugin_paths_do_not_match_when_source_has_no_video_stream() {
    let media_info = media_info_without_video();
    let startup = apply_decoder_plugin_diagnostics(
        startup_with_video_decode(macos_video_decode_info(&media_info)),
        &media_info,
        &PlayerRuntimeOptions::default()
            .with_decoder_plugin_library_paths([PathBuf::from("/tmp/missing-decoder-plugin")]),
    );

    assert!(startup.plugin_diagnostics.is_empty());
    let fallback = startup
        .video_decode
        .as_ref()
        .and_then(|info| info.fallback_reason.as_deref())
        .unwrap_or_default();
    assert!(fallback.contains("source does not expose a decodable video stream"));
    assert!(!fallback.contains("decoder plugin"));
}

#[test]
fn macos_host_runtime_without_surface_falls_back_to_software() {
    if !cfg!(target_os = "macos") {
        return;
    }

    let Some(test_video_path) = test_video_path() else {
        eprintln!(
            "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
        );
        return;
    };
    let bootstrap = open_macos_host_runtime_source_with_options(
        MediaSource::new(test_video_path),
        PlayerRuntimeOptions::default(),
    )
    .expect("host runtime should fall back to software without a video surface");

    assert_eq!(
        bootstrap.runtime.adapter_id(),
        MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
    );
    assert!(
        bootstrap
            .startup
            .video_decode
            .as_ref()
            .and_then(|info| info.fallback_reason.as_deref())
            .unwrap_or_default()
            .contains("requires an external video surface")
    );
}

#[test]
#[cfg(target_os = "macos")]
fn macos_host_runtime_with_surface_prefers_native() {
    let Some(test_video_path) = test_video_path() else {
        eprintln!(
            "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
        );
        return;
    };
    let layer_handle = unsafe { player_macos_test_create_player_layer() };
    assert!(
        !layer_handle.is_null(),
        "test player layer handle should be created"
    );

    let options = PlayerRuntimeOptions::default().with_video_surface(PlayerVideoSurfaceTarget {
        kind: PlayerVideoSurfaceKind::PlayerLayer,
        handle: layer_handle as usize,
    });
    let bootstrap =
        open_macos_host_runtime_source_with_options(MediaSource::new(test_video_path), options)
            .expect("host runtime should prefer native playback when a valid surface exists");

    assert_eq!(
        bootstrap.runtime.adapter_id(),
        MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
    );

    unsafe {
        player_macos_test_release_object(layer_handle);
    }
}

#[test]
fn macos_host_runtime_probe_prefers_native_probe() {
    if !cfg!(target_os = "macos") {
        return;
    }

    let Some(test_video_path) = test_video_path() else {
        eprintln!(
            "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
        );
        return;
    };
    let probe = probe_macos_host_runtime_source_with_options(
        MediaSource::new(test_video_path),
        PlayerRuntimeOptions::default(),
    )
    .expect("host runtime probe should succeed");

    assert_eq!(probe.adapter_id, MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID);
    assert_eq!(
        probe.capabilities.backend_family,
        PlayerRuntimeAdapterBackendFamily::NativeMacos
    );
}

#[test]
fn release_native_frame_tracking_decrements_outstanding_count() {
    let outstanding_frames = Arc::new(AtomicUsize::new(1));
    let mut session = FakeNativeDecoderSession::default();
    let frame = DecoderNativeFrame {
        metadata: DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: player_plugin::DecoderFrameFormat::Nv12,
            codec: "h264".to_owned(),
            pts_us: Some(1_000),
            duration_us: Some(33_000),
            width: 1920,
            height: 1080,
            coded_width: Some(1920),
            coded_height: Some(1080),
            visible_rect: None,
            handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
            pipeline_profile: Some(
                player_plugin::NativeFramePipelineProfile::VideoToolboxCvPixelBuffer,
            ),
            color_space: None,
            hdr_metadata: None,
            sync_info: None,
            transform: None,
            frame_id: Some(7),
            release_tracking: None,
        },
        handle: 7,
    };

    release_native_frame_with_counter(&mut session, outstanding_frames.as_ref(), frame)
        .expect("release should succeed");

    assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
    assert_eq!(session.released_handles, 1);
}

#[test]
fn present_failure_still_releases_native_frame() {
    let outstanding_frames = Arc::new(AtomicUsize::new(1));
    let mut session = FakeNativeDecoderSession::default();
    let frame = DecoderNativeFrame {
        metadata: DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: player_plugin::DecoderFrameFormat::Nv12,
            codec: "h264".to_owned(),
            pts_us: Some(2_000),
            duration_us: Some(33_000),
            width: 1280,
            height: 720,
            coded_width: Some(1280),
            coded_height: Some(720),
            visible_rect: None,
            handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
            pipeline_profile: Some(
                player_plugin::NativeFramePipelineProfile::VideoToolboxCvPixelBuffer,
            ),
            color_space: None,
            hdr_metadata: None,
            sync_info: None,
            transform: None,
            frame_id: Some(11),
            release_tracking: None,
        },
        handle: 11,
    };

    let error = present_and_release_native_frame_with_presenter(
        &mut session,
        outstanding_frames.as_ref(),
        frame,
        |_handle| Err("forced presenter failure".to_owned()),
    )
    .expect_err("present failure should bubble up");

    assert!(error.to_string().contains("forced presenter failure"));
    assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
    assert_eq!(session.released_handles, 1);
}

#[test]
fn stale_presentation_epoch_releases_frame_without_presenting() {
    let outstanding_frames = Arc::new(AtomicUsize::new(1));
    let present_called = Arc::new(AtomicBool::new(false));
    let mut session = FakeNativeDecoderSession::default();
    let frame = DecoderNativeFrame {
        metadata: DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: player_plugin::DecoderFrameFormat::Nv12,
            codec: "h264".to_owned(),
            pts_us: Some(3_000),
            duration_us: Some(33_000),
            width: 640,
            height: 360,
            coded_width: Some(640),
            coded_height: Some(360),
            visible_rect: None,
            handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
            pipeline_profile: Some(
                player_plugin::NativeFramePipelineProfile::VideoToolboxCvPixelBuffer,
            ),
            color_space: None,
            hdr_metadata: None,
            sync_info: None,
            transform: None,
            frame_id: Some(13),
            release_tracking: None,
        },
        handle: 13,
    };

    let result = present_if_current_epoch_and_release(
        &mut session,
        outstanding_frames.as_ref(),
        2,
        1,
        frame,
        |_frame| {
            present_called.store(true, Ordering::SeqCst);
            Ok(())
        },
    );

    assert!(result.is_ok());
    assert!(!present_called.load(Ordering::SeqCst));
    assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
    assert_eq!(
        session.session_info().decoder_name.as_deref(),
        Some("released=1")
    );
}

#[test]
fn native_frame_source_seek_flushes_before_packet_seek_and_resets_eof() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let session_state = RecordingNativeDecoderState::shared(events.clone());
    let packet_source = FakeNativeFramePacketSource::with_seek_packets(
        Vec::new(),
        vec![test_compressed_packet(250_000)],
        events.clone(),
    );
    let outstanding_frames = Arc::new(AtomicUsize::new(0));
    let mut source = native_frame_source_for_test(
        packet_source,
        session_state.clone(),
        outstanding_frames.clone(),
        true,
        true,
    );

    let frame = source
        .seek_to(Duration::from_millis(250))
        .expect("seek should succeed")
        .expect("seek should decode a frame");

    let events = events
        .lock()
        .map(|events| events.clone())
        .unwrap_or_default();
    assert!(
        contains_ordered_events(&events, &["flush", "packet_seek", "send_packet"]),
        "seek should flush before packet seek and first post-seek packet: {events:?}"
    );
    assert_eq!(
        session_state
            .lock()
            .map(|state| state.flush_count)
            .unwrap_or_default(),
        1
    );
    assert_eq!(frame.presentation_time, Duration::from_micros(250_000));
    drop(frame);
    assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
}

#[test]
fn native_frame_source_seek_flushes_processor_before_decoder_and_packet_session() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let processor_state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
        events: Some(events.clone()),
        ..RecordingFrameProcessorState::default()
    }));
    let shared = Arc::new(std::sync::Mutex::new(MacosNativeFrameDecoderState {
        frame_processor_chain: Some(frame_processor_chain_for_test(
            FrameProcessorMode::DiagnosticsOnly,
            vec![RecordingFrameProcessorSession::new(processor_state.clone())],
        )),
        presenter: None,
        presentation_epoch: 0,
    }));
    let session_state = RecordingNativeDecoderState::shared(events.clone());
    let session: Arc<Mutex<Box<dyn NativeDecoderSession>>> =
        Arc::new(Mutex::new(Box::new(RecordingNativeDecoderSession {
            state: session_state.clone(),
        })));
    let mut packet_source =
        FakeNativeFramePacketSource::with_seek_packets(Vec::new(), Vec::new(), events.clone());

    flush_and_seek_macos_native_frame_source(
        &session,
        &shared,
        &mut packet_source,
        Duration::from_millis(250),
    )
    .expect("flush and seek should succeed");

    let events = events
        .lock()
        .map(|events| events.clone())
        .unwrap_or_default();
    assert!(
        contains_ordered_events(&events, &["processor_flush", "flush", "packet_seek"]),
        "seek should flush processor before decoder and packet session: {events:?}"
    );
    assert_eq!(
        processor_state
            .lock()
            .map(|state| state.flush_count)
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        session_state
            .lock()
            .map(|state| state.flush_count)
            .unwrap_or_default(),
        1
    );
}

#[test]
fn native_frame_source_seek_releases_queued_prefetched_frame() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let session_state = RecordingNativeDecoderState::shared(events.clone());
    let packet_source = FakeNativeFramePacketSource::with_seek_packets(
        vec![test_compressed_packet(100_000)],
        vec![test_compressed_packet(250_000)],
        events,
    );
    let outstanding_frames = Arc::new(AtomicUsize::new(0));
    let mut source = native_frame_source_for_test(
        packet_source,
        session_state.clone(),
        outstanding_frames.clone(),
        false,
        false,
    );

    wait_for_buffered_native_frame(&source, 1);
    assert_eq!(source.buffered_frame_count(), 1);
    assert_eq!(outstanding_frames.load(Ordering::SeqCst), 1);

    let frame = source
        .seek_to(Duration::from_millis(250))
        .expect("seek should succeed")
        .expect("seek should decode a replacement frame");

    assert_eq!(frame.presentation_time, Duration::from_micros(250_000));
    assert_eq!(
        session_state
            .lock()
            .map(|state| state.released_handles)
            .unwrap_or_default(),
        1,
        "seek should release the queued pre-seek native frame"
    );
    assert_eq!(outstanding_frames.load(Ordering::SeqCst), 1);
    drop(frame);
    assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
    assert_eq!(
        session_state
            .lock()
            .map(|state| state.released_handles)
            .unwrap_or_default(),
        2
    );
}

#[test]
fn stale_prefetch_frame_release_does_not_decrement_current_generation_buffer_count() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let session_state = RecordingNativeDecoderState::shared(events.clone());
    let packet_source =
        FakeNativeFramePacketSource::with_seek_packets(Vec::new(), Vec::new(), events);
    let outstanding_frames = Arc::new(AtomicUsize::new(1));
    let mut source = native_frame_source_for_test(
        packet_source,
        session_state.clone(),
        outstanding_frames.clone(),
        false,
        true,
    );
    source.generation = 1;
    source.current_generation.store(1, Ordering::SeqCst);
    source.buffered_frame_count.store(1, Ordering::SeqCst);

    source
        .handle_prefetch_event(MacosNativeFrameWorkerEvent::Frame {
            generation: 0,
            frame: macos_frame_processor_frame_for_test(9, Some(100_000)),
        })
        .expect("stale prefetch event should be released");

    assert_eq!(
        source.buffered_frame_count(),
        1,
        "stale frames were not counted for the current generation"
    );
    assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
    assert_eq!(
        session_state
            .lock()
            .map(|state| state.released_handles)
            .unwrap_or_default(),
        1
    );
}

#[test]
fn dropping_native_frame_source_releases_queued_processor_output() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let processor_state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
        output_handle_offset: 1_000,
        ..RecordingFrameProcessorState::default()
    }));
    let session_state = RecordingNativeDecoderState::shared(events.clone());
    let outstanding_frames = Arc::new(AtomicUsize::new(1));
    let session: Arc<std::sync::Mutex<Box<dyn NativeDecoderSession>>> = Arc::new(
        std::sync::Mutex::new(Box::new(RecordingNativeDecoderSession {
            state: session_state.clone(),
        })),
    );
    let shared = Arc::new(std::sync::Mutex::new(MacosNativeFrameDecoderState {
        frame_processor_chain: Some(frame_processor_chain_for_test(
            FrameProcessorMode::DiagnosticsOnly,
            vec![RecordingFrameProcessorSession::new(processor_state.clone())],
        )),
        presenter: None,
        presentation_epoch: 0,
    }));
    let (command_tx, _command_rx) = mpsc::channel();
    let (frame_tx, frame_rx) = mpsc::channel();
    let current_generation = Arc::new(AtomicU64::new(0));
    let buffered_frame_count = Arc::new(AtomicUsize::new(1));
    let prefetch_wakeup = Arc::new(MacosNativeFramePrefetchWakeup::default());
    frame_tx
        .send(MacosNativeFrameWorkerEvent::Frame {
            generation: 0,
            frame: macos_frame_processor_frame_for_test_with_processor_output(
                9,
                Some(100_000),
                1_009,
            ),
        })
        .expect("queued frame event should be sent");
    let source = MacosNativeFrameVideoSource {
        stream_info: test_video_packet_stream_info(),
        session,
        shared,
        outstanding_frames: outstanding_frames.clone(),
        command_tx,
        frame_rx,
        generation: 0,
        current_generation,
        buffered_frame_count,
        prefetch_limit: Arc::new(AtomicUsize::new(1)),
        prefetch_wakeup,
        end_of_input_sent: false,
        end_of_stream_received: false,
        worker: None,
    };

    drop(source);

    assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
    assert_eq!(
        session_state
            .lock()
            .map(|state| state.released_handles)
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        processor_state
            .lock()
            .map(|state| state.released_handles.clone())
            .unwrap_or_default(),
        vec![1_009]
    );
}

#[test]
fn dropping_native_frame_source_closes_processor_and_decoder_sessions() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let processor_state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
        events: Some(events.clone()),
        ..RecordingFrameProcessorState::default()
    }));
    let session_state = RecordingNativeDecoderState::shared(events.clone());
    let packet_source =
        FakeNativeFramePacketSource::with_seek_packets(Vec::new(), Vec::new(), events.clone());
    let outstanding_frames = Arc::new(AtomicUsize::new(0));
    let source = native_frame_source_for_test(
        packet_source,
        session_state.clone(),
        outstanding_frames,
        false,
        false,
    );
    {
        let mut shared = source
            .shared
            .lock()
            .expect("shared native-frame state should not be poisoned");
        shared.frame_processor_chain = Some(frame_processor_chain_for_test(
            FrameProcessorMode::DiagnosticsOnly,
            vec![RecordingFrameProcessorSession::new(processor_state.clone())],
        ));
    }

    drop(source);

    assert_eq!(
        processor_state
            .lock()
            .map(|state| state.close_count)
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        session_state
            .lock()
            .map(|state| state.close_count)
            .unwrap_or_default(),
        1
    );
    let events = events
        .lock()
        .map(|events| events.clone())
        .unwrap_or_default();
    assert!(
        contains_ordered_events(&events, &["processor_flush", "processor_close", "close"]),
        "drop should flush/close processor before decoder close: {events:?}"
    );
}

#[test]
fn macos_native_frame_source_switch_releases_old_source_and_decodes_new_source() {
    let old_events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let old_processor_state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
        events: Some(old_events.clone()),
        output_handle_offset: 1_000,
        ..RecordingFrameProcessorState::default()
    }));
    let old_session_state = RecordingNativeDecoderState::shared(old_events.clone());
    let old_session: Arc<std::sync::Mutex<Box<dyn NativeDecoderSession>>> = Arc::new(
        std::sync::Mutex::new(Box::new(RecordingNativeDecoderSession {
            state: old_session_state.clone(),
        })),
    );
    let old_shared = Arc::new(std::sync::Mutex::new(MacosNativeFrameDecoderState {
        frame_processor_chain: Some(frame_processor_chain_for_test(
            FrameProcessorMode::DiagnosticsOnly,
            vec![RecordingFrameProcessorSession::new(
                old_processor_state.clone(),
            )],
        )),
        presenter: None,
        presentation_epoch: 0,
    }));
    let (old_command_tx, _old_command_rx) = mpsc::channel();
    let (old_frame_tx, old_frame_rx) = mpsc::channel();
    let old_outstanding_frames = Arc::new(AtomicUsize::new(1));
    old_frame_tx
        .send(MacosNativeFrameWorkerEvent::Frame {
            generation: 0,
            frame: macos_frame_processor_frame_for_test_with_processor_output(
                9,
                Some(100_000),
                1_009,
            ),
        })
        .expect("queued old-source frame event should be sent");
    let old_source = MacosNativeFrameVideoSource {
        stream_info: test_video_packet_stream_info(),
        session: old_session,
        shared: old_shared,
        outstanding_frames: old_outstanding_frames.clone(),
        command_tx: old_command_tx,
        frame_rx: old_frame_rx,
        generation: 0,
        current_generation: Arc::new(AtomicU64::new(0)),
        buffered_frame_count: Arc::new(AtomicUsize::new(1)),
        prefetch_limit: Arc::new(AtomicUsize::new(1)),
        prefetch_wakeup: Arc::new(MacosNativeFramePrefetchWakeup::default()),
        end_of_input_sent: false,
        end_of_stream_received: false,
        worker: None,
    };

    drop(old_source);

    assert_eq!(old_outstanding_frames.load(Ordering::SeqCst), 0);
    assert_eq!(
        old_session_state
            .lock()
            .map(|state| (state.released_handles, state.close_count))
            .unwrap_or_default(),
        (1, 1),
        "source switch should release queued old-source native frames and close the decoder"
    );
    assert_eq!(
        old_processor_state
            .lock()
            .map(|state| {
                (
                    state.released_handles.clone(),
                    state.flush_count,
                    state.close_count,
                )
            })
            .unwrap_or_default(),
        (vec![1_009], 1, 1),
        "source switch should release queued processor outputs and close the processor"
    );
    let old_events = old_events
        .lock()
        .map(|events| events.clone())
        .unwrap_or_default();
    assert!(
        contains_ordered_events(
            &old_events,
            &["processor_flush", "processor_close", "close"]
        ),
        "source switch should flush/close old processor before decoder close: {old_events:?}"
    );

    let new_events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let new_session_state = RecordingNativeDecoderState::shared(new_events);
    let new_outstanding_frames = Arc::new(AtomicUsize::new(0));
    let new_packet_source = FakeNativeFramePacketSource::with_seek_packets(
        vec![test_compressed_packet(500_000)],
        Vec::new(),
        Arc::new(std::sync::Mutex::new(Vec::new())),
    );
    let mut new_source = native_frame_source_for_test(
        new_packet_source,
        new_session_state.clone(),
        new_outstanding_frames.clone(),
        false,
        false,
    );

    let new_frame = new_source
        .recv_frame()
        .expect("new source should receive after source switch")
        .expect("new source should decode a native frame");

    assert_eq!(new_frame.presentation_time, Duration::from_micros(500_000));
    assert_eq!(new_outstanding_frames.load(Ordering::SeqCst), 1);
    drop(new_frame);
    assert_eq!(new_outstanding_frames.load(Ordering::SeqCst), 0);
    drop(new_source);
    assert_eq!(
        new_session_state
            .lock()
            .map(|state| state.close_count)
            .unwrap_or_default(),
        1,
        "new source should own and close its decoder independently"
    );
}

#[test]
fn native_frame_source_sends_eof_once_and_keeps_terminal_eof() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let session_state = RecordingNativeDecoderState::shared(events.clone());
    let packet_source =
        FakeNativeFramePacketSource::with_seek_packets(Vec::new(), Vec::new(), events);
    let mut source = native_frame_source_for_test(
        packet_source,
        session_state.clone(),
        Arc::new(AtomicUsize::new(0)),
        false,
        false,
    );

    assert!(
        source
            .recv_frame()
            .expect("first receive should succeed")
            .is_none()
    );
    assert!(matches!(
        source
            .try_recv_frame()
            .expect("second poll should stay terminal"),
        DesktopVideoFramePoll::EndOfStream
    ));

    let sent_packets = session_state
        .lock()
        .map(|state| state.sent_packets.clone())
        .unwrap_or_default();
    assert_eq!(
        sent_packets
            .iter()
            .filter(|packet| packet.end_of_stream)
            .count(),
        1
    );
}

#[test]
fn source_normalizer_packet_source_preserves_metadata_and_releases_packet() {
    let stream_info = fake_source_normalizer_packet_stream_info("H264");
    let session = Arc::new(Mutex::new(Some(
        Box::new(FakeSourceNormalizerPacketSession::new(stream_info))
            as Box<dyn SourceNormalizerPacketSession>,
    )));
    let mut packet_source = super::SourceNormalizerPacketSource::new(session.clone());
    let session_state = RecordingNativeDecoderState::shared(Arc::new(Mutex::new(Vec::new())));
    let decoder_session: Arc<Mutex<Box<dyn NativeDecoderSession>>> =
        Arc::new(Mutex::new(Box::new(RecordingNativeDecoderSession {
            state: session_state.clone(),
        })));

    let send_status = packet_source
        .send_next_packet(&decoder_session)
        .expect("source normalizer packet should send");

    assert_eq!(send_status, MacosNativeFramePacketSendStatus::Sent);
    let sent_packets = session_state
        .lock()
        .map(|state| state.sent_packets.clone())
        .unwrap_or_default();
    assert_eq!(sent_packets.len(), 1);
    assert_eq!(sent_packets[0].pts_us, Some(0));
    assert_eq!(sent_packets[0].dts_us, Some(0));
    assert_eq!(sent_packets[0].duration_us, Some(41_667));
    assert!(sent_packets[0].key_frame);
    assert_eq!(sent_packets[0].stream_index, 0);
    assert_eq!(
        packet_source
            .send_next_packet(&decoder_session)
            .expect("second source normalizer packet read should see EOS"),
        MacosNativeFramePacketSendStatus::EndOfStream
    );
}

#[test]
fn source_normalizer_packet_source_skips_non_video_packets() {
    let mut stream_info = fake_source_normalizer_packet_stream_info("H264");
    stream_info.tracks.push(SourceNormalizerPacketTrackInfo {
        stream_index: 1,
        media_kind: SourceNormalizerPacketMediaKind::Audio,
        codec: "AAC".to_owned(),
        extradata: Vec::new(),
        bitstream_format: None,
        width: None,
        height: None,
        coded_width: None,
        coded_height: None,
        sample_rate: Some(48_000),
        channels: Some(2),
        channel_layout: Some("stereo".to_owned()),
        codec_delay_samples: None,
        priming_samples: Some(2_112),
        trailing_padding_samples: None,
        seek_preroll_samples: Some(1_024),
        frame_rate: None,
        time_base_num: Some(1),
        time_base_den: Some(48_000),
    });
    let released_handles = Arc::new(Mutex::new(Vec::new()));
    let session = Arc::new(Mutex::new(Some(Box::new(
        FakeSourceNormalizerPacketSession::with_release_log(
            stream_info,
            vec![
                fake_source_normalizer_audio_packet(),
                fake_source_normalizer_video_packet(),
            ],
            released_handles.clone(),
        ),
    )
        as Box<dyn SourceNormalizerPacketSession>)));
    let mut packet_source = super::SourceNormalizerPacketSource::new(session.clone());
    let session_state = RecordingNativeDecoderState::shared(Arc::new(Mutex::new(Vec::new())));
    let decoder_session: Arc<Mutex<Box<dyn NativeDecoderSession>>> =
        Arc::new(Mutex::new(Box::new(RecordingNativeDecoderSession {
            state: session_state.clone(),
        })));

    let send_status = packet_source
        .send_next_packet(&decoder_session)
        .expect("source normalizer video packet should send after skipping audio");

    assert_eq!(send_status, MacosNativeFramePacketSendStatus::Sent);
    let sent_packets = session_state
        .lock()
        .map(|state| state.sent_packets.clone())
        .unwrap_or_default();
    assert_eq!(sent_packets.len(), 1);
    assert_eq!(sent_packets[0].stream_index, 0);
    let released_handles = released_handles
        .lock()
        .expect("source normalizer release log mutex should not be poisoned")
        .clone();
    assert_eq!(released_handles, vec![1, 2]);
}

#[test]
fn source_normalizer_packet_source_skips_unselected_video_stream() {
    let mut stream_info = fake_source_normalizer_packet_stream_info("H264");
    stream_info.tracks.push(SourceNormalizerPacketTrackInfo {
        stream_index: 2,
        media_kind: SourceNormalizerPacketMediaKind::Video,
        codec: "H264".to_owned(),
        extradata: Vec::new(),
        bitstream_format: Some(DecoderBitstreamFormat::Avcc),
        width: Some(640),
        height: Some(360),
        coded_width: Some(640),
        coded_height: Some(360),
        sample_rate: None,
        channels: None,
        channel_layout: None,
        codec_delay_samples: None,
        priming_samples: None,
        trailing_padding_samples: None,
        seek_preroll_samples: None,
        frame_rate: Some(30.0),
        time_base_num: Some(1),
        time_base_den: Some(30_000),
    });
    let mut unselected_packet = fake_source_normalizer_video_packet();
    unselected_packet.stream_index = 2;
    unselected_packet.pts_us = Some(41_667);
    let selected_packet = fake_source_normalizer_video_packet();
    let released_handles = Arc::new(Mutex::new(Vec::new()));
    let session = Arc::new(Mutex::new(Some(Box::new(
        FakeSourceNormalizerPacketSession::with_release_log(
            stream_info,
            vec![unselected_packet, selected_packet],
            released_handles.clone(),
        ),
    )
        as Box<dyn SourceNormalizerPacketSession>)));
    let mut packet_source = super::SourceNormalizerPacketSource::new(session.clone());
    let session_state = RecordingNativeDecoderState::shared(Arc::new(Mutex::new(Vec::new())));
    let decoder_session: Arc<Mutex<Box<dyn NativeDecoderSession>>> =
        Arc::new(Mutex::new(Box::new(RecordingNativeDecoderSession {
            state: session_state.clone(),
        })));

    let send_status = packet_source
        .send_next_packet(&decoder_session)
        .expect("source normalizer selected video packet should send");

    assert_eq!(send_status, MacosNativeFramePacketSendStatus::Sent);
    let sent_packets = session_state
        .lock()
        .map(|state| state.sent_packets.clone())
        .unwrap_or_default();
    assert_eq!(sent_packets.len(), 1);
    assert_eq!(sent_packets[0].stream_index, 0);
    assert_eq!(sent_packets[0].pts_us, Some(0));
    let released_handles = released_handles
        .lock()
        .expect("source normalizer release log mutex should not be poisoned")
        .clone();
    assert_eq!(released_handles, vec![1, 2]);
}

#[test]
fn source_normalizer_packet_source_seek_flushes_packet_session() {
    let stream_info = fake_source_normalizer_packet_stream_info("H264");
    let events = Arc::new(Mutex::new(Vec::new()));
    let seek_positions = Arc::new(Mutex::new(Vec::new()));
    let session = Arc::new(Mutex::new(Some(Box::new(
        FakeSourceNormalizerPacketSession::with_seek_log(
            stream_info,
            events.clone(),
            seek_positions.clone(),
        ),
    )
        as Box<dyn SourceNormalizerPacketSession>)));
    let mut packet_source = super::SourceNormalizerPacketSource::new(session);

    packet_source
        .seek_to(Duration::from_millis(250))
        .expect("source normalizer packet seek should succeed");

    assert_eq!(
        events
            .lock()
            .expect("source normalizer event log mutex should not be poisoned")
            .clone(),
        vec!["source_normalizer_flush", "source_normalizer_seek"]
    );
    assert_eq!(
        seek_positions
            .lock()
            .expect("source normalizer seek log mutex should not be poisoned")
            .clone(),
        vec![250]
    );
}

#[test]
fn source_normalizer_packet_source_seek_drops_pending_packet_before_resume() {
    let stream_info = fake_source_normalizer_packet_stream_info("H264");
    let released_handles = Arc::new(Mutex::new(Vec::new()));
    let mut first_packet = fake_source_normalizer_video_packet();
    first_packet.pts_us = Some(100_000);
    let mut second_packet = fake_source_normalizer_video_packet();
    second_packet.pts_us = Some(500_000);
    let session = Arc::new(Mutex::new(Some(Box::new(
        FakeSourceNormalizerPacketSession::with_release_log(
            stream_info,
            vec![first_packet, second_packet],
            released_handles.clone(),
        ),
    )
        as Box<dyn SourceNormalizerPacketSession>)));
    let mut packet_source = super::SourceNormalizerPacketSource::new(session.clone());
    let session_state = RecordingNativeDecoderState::shared(Arc::new(Mutex::new(Vec::new())));
    if let Ok(mut state) = session_state.lock() {
        state.reject_next_packet = true;
    }
    let decoder_session: Arc<Mutex<Box<dyn NativeDecoderSession>>> =
        Arc::new(Mutex::new(Box::new(RecordingNativeDecoderSession {
            state: session_state.clone(),
        })));

    let first_status = packet_source
        .send_next_packet(&decoder_session)
        .expect("first source normalizer packet should be retained as pending");
    assert_eq!(first_status, MacosNativeFramePacketSendStatus::NeedMoreData);
    assert!(
        packet_source.pending.is_some(),
        "decoder backpressure should keep the first packet pending"
    );

    packet_source
        .seek_to(Duration::from_millis(500))
        .expect("source normalizer packet seek should flush stale pending packet");
    assert!(
        packet_source.pending.is_none(),
        "seek must drop stale pending packet before resuming"
    );

    let second_status = packet_source
        .send_next_packet(&decoder_session)
        .expect("post-seek source normalizer packet should send");
    assert_eq!(second_status, MacosNativeFramePacketSendStatus::Sent);
    let sent_packets = session_state
        .lock()
        .map(|state| state.sent_packets.clone())
        .unwrap_or_default();
    assert_eq!(sent_packets.len(), 2);
    assert_eq!(sent_packets[0].pts_us, Some(100_000));
    assert_eq!(sent_packets[1].pts_us, Some(500_000));
    let released_handles = released_handles
        .lock()
        .expect("source normalizer release log mutex should not be poisoned")
        .clone();
    assert_eq!(released_handles, vec![1, 2]);
}

#[test]
fn source_normalizer_packet_source_drop_after_backpressure_has_no_outstanding_lease() {
    let stream_info = fake_source_normalizer_packet_stream_info("H264");
    let released_handles = Arc::new(Mutex::new(Vec::new()));
    let session = Arc::new(Mutex::new(Some(Box::new(
        FakeSourceNormalizerPacketSession::with_release_log(
            stream_info,
            vec![fake_source_normalizer_video_packet()],
            released_handles.clone(),
        ),
    )
        as Box<dyn SourceNormalizerPacketSession>)));
    let mut packet_source = super::SourceNormalizerPacketSource::new(session.clone());
    let session_state = RecordingNativeDecoderState::shared(Arc::new(Mutex::new(Vec::new())));
    if let Ok(mut state) = session_state.lock() {
        state.reject_next_packet = true;
    }
    let decoder_session: Arc<Mutex<Box<dyn NativeDecoderSession>>> =
        Arc::new(Mutex::new(Box::new(RecordingNativeDecoderSession {
            state: session_state,
        })));

    let send_status = packet_source
        .send_next_packet(&decoder_session)
        .expect("source normalizer packet should become pending under decoder backpressure");
    assert_eq!(send_status, MacosNativeFramePacketSendStatus::NeedMoreData);
    assert!(
        packet_source.pending.is_some(),
        "decoder backpressure should keep copied packet data pending"
    );

    drop(packet_source);

    assert_eq!(
        released_handles
            .lock()
            .expect("source normalizer release log mutex should not be poisoned")
            .clone(),
        vec![1],
        "packet lease should already be released before pending copied packet data is dropped"
    );
    let mut guard = session
        .lock()
        .expect("source normalizer session mutex should not be poisoned");
    let session = guard
        .as_mut()
        .expect("source normalizer session should remain available");
    let eof = session
        .read_packet()
        .expect("drop after backpressure should leave no outstanding packet lease");
    assert_eq!(
        eof.metadata.status,
        SourceNormalizerReadPacketStatus::EndOfStream
    );
}

#[test]
fn native_frame_source_seek_after_eof_allows_packets_again() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let session_state = RecordingNativeDecoderState::shared(events.clone());
    let packet_source = FakeNativeFramePacketSource::with_seek_packets(
        Vec::new(),
        vec![test_compressed_packet(500_000)],
        events.clone(),
    );
    let outstanding_frames = Arc::new(AtomicUsize::new(0));
    let mut source = native_frame_source_for_test(
        packet_source,
        session_state.clone(),
        outstanding_frames.clone(),
        false,
        false,
    );

    assert!(
        source
            .recv_frame()
            .expect("initial eof should succeed")
            .is_none()
    );
    let frame = source
        .seek_to(Duration::from_millis(500))
        .expect("seek after eof should succeed")
        .expect("seek after eof should decode a frame");

    let events = events
        .lock()
        .map(|events| events.clone())
        .unwrap_or_default();
    assert!(
        contains_ordered_events(
            &events,
            &["send_eos", "flush", "packet_seek", "send_packet"]
        ),
        "seek after EOF should flush and resume packets in order: {events:?}"
    );
    assert_eq!(frame.presentation_time, Duration::from_micros(500_000));
    drop(frame);
    assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
}

#[test]
fn dropping_deferred_native_frame_releases_without_presenting() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let session_state = RecordingNativeDecoderState::shared(events.clone());
    let packet_source = FakeNativeFramePacketSource::with_seek_packets(
        vec![test_compressed_packet(1_000)],
        Vec::new(),
        events,
    );
    let outstanding_frames = Arc::new(AtomicUsize::new(0));
    let mut source = native_frame_source_for_test(
        packet_source,
        session_state.clone(),
        outstanding_frames.clone(),
        false,
        false,
    );

    let frame = source
        .recv_frame()
        .expect("frame receive should succeed")
        .expect("expected a deferred native frame");
    assert_eq!(outstanding_frames.load(Ordering::SeqCst), 1);

    drop(frame);

    assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
    assert_eq!(
        session_state
            .lock()
            .map(|state| state.released_handles)
            .unwrap_or_default(),
        1
    );
}

#[test]
fn frame_processor_prefer_mode_uses_processed_frame_and_releases_output() {
    let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
        output_handle_offset: 1_000,
        ..RecordingFrameProcessorState::default()
    }));
    let mut chain = frame_processor_chain_for_test(
        FrameProcessorMode::PreferProcessed,
        vec![RecordingFrameProcessorSession::new(state.clone())],
    );

    let processed = chain
        .process(test_native_frame(10, Some(33_000)))
        .expect("processor chain should produce a frame");

    assert_eq!(processed.presentation_frame().handle, 1_010);
    assert_eq!(processed.decoder_frame().handle, 10);
    assert_eq!(processed.processor_outputs().len(), 1);
    assert_eq!(chain.metrics().submitted_frame_count, 1);
    assert_eq!(chain.metrics().processed_frame_count, 1);

    chain.release_processor_outputs(processed.into_processor_outputs());
    assert_eq!(
        state
            .lock()
            .map(|state| state.released_handles.clone())
            .unwrap_or_default(),
        vec![1_010]
    );
}

#[test]
fn frame_processor_prefer_mode_accepts_in_place_passthrough_output() {
    let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
        output_handle_offset: 0,
        output_requires_release: Some(false),
        ..RecordingFrameProcessorState::default()
    }));
    let mut chain = frame_processor_chain_for_test(
        FrameProcessorMode::PreferProcessed,
        vec![RecordingFrameProcessorSession::new(state.clone())],
    );

    let processed = chain
        .process(test_native_frame(10, Some(33_000)))
        .expect("processor chain should accept in-place passthrough output");

    assert_eq!(processed.presentation_frame().handle, 10);
    assert!(processed.processor_outputs().is_empty());

    chain.release_processor_outputs(processed.into_processor_outputs());
    assert!(
        state
            .lock()
            .map(|state| state.released_handles.is_empty())
            .unwrap_or_default()
    );
}

#[test]
fn frame_processor_late_output_is_dropped_and_warns() {
    let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
        output_handle_offset: 2_000,
        submit_to_ready_us: Some(25_000),
        ..RecordingFrameProcessorState::default()
    }));
    let mut chain = frame_processor_chain_for_test(
        FrameProcessorMode::PreferProcessed,
        vec![RecordingFrameProcessorSession::new(state.clone())],
    );

    let processed = chain
        .process(test_native_frame(11, Some(66_000)))
        .expect("late output should bypass instead of failing in prefer mode");

    assert_eq!(processed.presentation_frame().handle, 11);
    assert!(processed.processor_outputs().is_empty());
    assert_eq!(chain.metrics().deadline_miss_count, 1);
    assert_eq!(chain.metrics().late_output_drop_count, 1);
    assert_eq!(chain.metrics().dropped_output_count, 1);
    assert_eq!(
        state
            .lock()
            .map(|state| state.released_handles.clone())
            .unwrap_or_default(),
        vec![2_011]
    );

    let events = chain.drain_events();
    assert!(
        events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(warning))
                if warning.kind == FrameProcessorWarningKind::LateOutputDropped
                    && warning.policy_action == FrameProcessorPolicyAction::DropOutput
                    && warning.processor_index == 0
                    && warning.output_handle_kind.as_deref() == Some("CvPixelBuffer")
                    && warning.submit_to_ready_us == Some(25_000)
                    && warning.deadline_overrun_us == Some(9_000)
        )),
        "late output should emit a processor-indexed warning"
    );
}

#[test]
fn frame_processor_diagnostics_mode_runs_processor_but_presents_original() {
    let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
        output_handle_offset: 4_000,
        ..RecordingFrameProcessorState::default()
    }));
    let mut chain = frame_processor_chain_for_test(
        FrameProcessorMode::DiagnosticsOnly,
        vec![RecordingFrameProcessorSession::new(state.clone())],
    );

    let processed = chain
        .process(test_native_frame(13, Some(120_000)))
        .expect("diagnostics mode should still run processor");

    assert_eq!(processed.presentation_frame().handle, 13);
    assert_eq!(processed.processor_outputs().len(), 1);
    assert_eq!(
        state
            .lock()
            .map(|state| state.submitted_handles.clone())
            .unwrap_or_default(),
        vec![13]
    );

    chain.release_processor_outputs(processed.into_processor_outputs());
    assert_eq!(
        state
            .lock()
            .map(|state| state.released_handles.clone())
            .unwrap_or_default(),
        vec![4_013]
    );
}

#[test]
fn frame_processor_backpressure_bypasses_and_reports_queue_state() {
    let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
        submit_status: Some(FrameProcessorSubmitStatus::Backpressure),
        forced_queue_depth: Some(3),
        forced_in_flight_frames: Some(2),
        ..RecordingFrameProcessorState::default()
    }));
    let mut chain = frame_processor_chain_for_test(
        FrameProcessorMode::PreferProcessed,
        vec![RecordingFrameProcessorSession::new(state)],
    );

    let processed = chain
        .process(test_native_frame(14, Some(140_000)))
        .expect("backpressure should bypass original in prefer mode");

    assert_eq!(processed.presentation_frame().handle, 14);
    assert_eq!(chain.metrics().bypassed_frame_count, 1);
    assert_eq!(chain.metrics().backpressure_count, 1);
    let events = chain.drain_events();
    assert!(
        events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(warning))
                if warning.kind == FrameProcessorWarningKind::Backpressure
                    && warning.policy_action == FrameProcessorPolicyAction::BypassOriginalFrame
                    && warning.queue_depth == Some(3)
                    && warning.in_flight_frames == Some(2)
        )),
        "backpressure should carry queue and in-flight state"
    );
}

#[test]
fn frame_processor_rejected_frame_fails_in_strict_mode() {
    let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
        submit_status: Some(FrameProcessorSubmitStatus::Rejected),
        ..RecordingFrameProcessorState::default()
    }));
    let mut chain = frame_processor_chain_for_test(
        FrameProcessorMode::RequireProcessed,
        vec![RecordingFrameProcessorSession::new(state)],
    );

    let error = chain
        .process(test_native_frame(15, Some(160_000)))
        .expect_err("strict mode should fail on rejected frame");

    assert!(error.0.to_string().contains("strict mode"));
    let events = chain.drain_events();
    assert!(
        events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(warning))
                if warning.kind == FrameProcessorWarningKind::Unsupported
                    && warning.policy_action == FrameProcessorPolicyAction::FailPlayback
                    && warning.processor_index == 0
        )),
        "strict rejected frame should emit unsupported warning before failing"
    );
}

#[test]
fn frame_processor_strict_deadline_failure_releases_processor_and_decoder_frames() {
    let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
        output_handle_offset: 3_000,
        submit_to_ready_us: Some(17_000),
        ..RecordingFrameProcessorState::default()
    }));
    let mut shared = MacosNativeFrameDecoderState {
        frame_processor_chain: Some(frame_processor_chain_for_test(
            FrameProcessorMode::RequireProcessed,
            vec![RecordingFrameProcessorSession::new(state.clone())],
        )),
        presenter: None,
        presentation_epoch: 0,
    };

    let error = process_macos_native_frame(&mut shared, test_native_frame(12, Some(99_000)))
        .expect_err("strict mode should fail playback on deadline miss");

    assert!(error.0.to_string().contains("strict mode"));
    assert_eq!(
        state
            .lock()
            .map(|state| state.released_handles.clone())
            .unwrap_or_default(),
        vec![3_012]
    );
}

#[test]
fn frame_processor_chain_flushes_sessions() {
    let first_state = Arc::new(std::sync::Mutex::new(
        RecordingFrameProcessorState::default(),
    ));
    let second_state = Arc::new(std::sync::Mutex::new(
        RecordingFrameProcessorState::default(),
    ));
    let mut chain = frame_processor_chain_for_test(
        FrameProcessorMode::DiagnosticsOnly,
        vec![
            RecordingFrameProcessorSession::new(first_state.clone()),
            RecordingFrameProcessorSession::new(second_state.clone()),
        ],
    );

    chain.flush();

    assert_eq!(
        first_state
            .lock()
            .map(|state| state.flush_count)
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        second_state
            .lock()
            .map(|state| state.flush_count)
            .unwrap_or_default(),
        1
    );
}

fn media_info_with_codec(codec: &str) -> PlayerMediaInfo {
    media_info_with_source_uri("fixture.mp4", codec)
}

fn media_info_with_audio_codec(video_codec: &str, audio_codec: &str) -> PlayerMediaInfo {
    PlayerMediaInfo {
        best_audio: Some(PlayerAudioInfo {
            codec: audio_codec.to_owned(),
            sample_rate: 48_000,
            channels: 2,
        }),
        ..media_info_with_codec(video_codec)
    }
}

fn media_info_with_source_uri(source_uri: &str, codec: &str) -> PlayerMediaInfo {
    PlayerMediaInfo {
        source_uri: source_uri.to_owned(),
        source_kind: player_runtime::MediaSourceKind::Local,
        source_protocol: player_runtime::MediaSourceProtocol::File,
        duration: None,
        bit_rate: None,
        audio_streams: 1,
        video_streams: 1,
        best_video: Some(PlayerVideoInfo {
            codec: codec.to_owned(),
            width: 960,
            height: 432,
            frame_rate: Some(30.0),
        }),
        best_audio: None,
        track_catalog: Default::default(),
        track_selection: Default::default(),
    }
}

fn default_software_capabilities() -> PlayerRuntimeAdapterCapabilities {
    PlayerRuntimeAdapterCapabilities {
        adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
        supports_audio_output: true,
        supports_frame_output: true,
        supports_external_video_surface: false,
        supports_seek: true,
        supports_stop: true,
        supports_playback_rate: true,
        playback_rate_min: Some(0.5),
        playback_rate_max: Some(3.0),
        natural_playback_rate_max: Some(2.0),
        supports_hardware_decode: false,
        supports_streaming: true,
        supports_hdr: false,
    }
}

fn media_info_without_video() -> PlayerMediaInfo {
    PlayerMediaInfo {
        video_streams: 0,
        best_video: None,
        ..media_info_with_codec("fixture-video")
    }
}

fn startup_with_video_decode(video_decode: PlayerVideoDecodeInfo) -> PlayerRuntimeStartup {
    PlayerRuntimeStartup {
        ffmpeg_initialized: false,
        audio_output: None,
        decoded_audio: None,
        video_decode: Some(video_decode),
        plugin_diagnostics: Vec::new(),
    }
}

fn decoder_plugin_record(
    status: PluginDiagnosticStatus,
    codec: &str,
    message: &str,
) -> PluginDiagnosticRecord {
    decoder_plugin_record_with_native_frame_output(status, codec, message, false)
}

fn decoder_native_plugin_record(
    status: PluginDiagnosticStatus,
    codec: &str,
    message: &str,
) -> PluginDiagnosticRecord {
    decoder_plugin_record_with_native_frame_output(status, codec, message, true)
}

fn decoder_plugin_record_with_native_frame_output(
    status: PluginDiagnosticStatus,
    codec: &str,
    message: &str,
    supports_native_frame_output: bool,
) -> PluginDiagnosticRecord {
    let decoder_capabilities = DecoderPluginCapabilitySummary {
        typed_codecs: vec![DecoderPluginCodecSummary {
            codec: codec.to_owned(),
            media_kind: DecoderMediaKind::Video,
        }],
        codecs: vec![format!("Video:{codec}")],
        supports_native_frame_output,
        native_requirements: None,
        supports_hardware_decode: false,
        supports_cpu_video_frames: !supports_native_frame_output,
        supports_audio_packets: false,
        supports_audio_frames: false,
        supports_pcm_frames: false,
        supports_gpu_handles: supports_native_frame_output,
        supports_presentation_release: false,
        supports_flush: true,
        supports_drain: true,
        max_sessions: Some(1),
    };
    PluginDiagnosticRecord {
        path: PathBuf::from("fixture-decoder"),
        status,
        plugin_name: Some("fixture-decoder".to_owned()),
        plugin_kind: Some(VesperPluginKind::Decoder),
        capability_summary: Some(PluginCapabilitySummary::Decoder(decoder_capabilities)),
        message: Some(message.to_owned()),
    }
}

fn decoder_audio_pcm_plugin_record(codec: &str, plugin_name: &str) -> PluginDiagnosticRecord {
    let decoder_capabilities = DecoderPluginCapabilitySummary {
        typed_codecs: vec![DecoderPluginCodecSummary {
            codec: codec.to_owned(),
            media_kind: DecoderMediaKind::Audio,
        }],
        codecs: vec![format!("Audio:{codec}")],
        supports_native_frame_output: false,
        native_requirements: None,
        supports_hardware_decode: false,
        supports_cpu_video_frames: false,
        supports_audio_packets: true,
        supports_audio_frames: true,
        supports_pcm_frames: true,
        supports_gpu_handles: false,
        supports_presentation_release: false,
        supports_flush: true,
        supports_drain: true,
        max_sessions: Some(1),
    };
    PluginDiagnosticRecord {
        path: PathBuf::from(plugin_name),
        status: PluginDiagnosticStatus::DecoderSupported,
        plugin_name: Some(plugin_name.to_owned()),
        plugin_kind: Some(VesperPluginKind::Decoder),
        capability_summary: Some(PluginCapabilitySummary::Decoder(decoder_capabilities)),
        message: Some(format!(
            "{plugin_name} advertises Audio {codec} PCM support"
        )),
    }
}

fn test_video_path() -> Option<String> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../fixtures/media/tiny-h264-aac.m4v");
    path.canonicalize()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

fn videotoolbox_smoke_source_path() -> Option<String> {
    if let Some(source) = std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_SOURCE")
        .map(|source| source.to_string_lossy().trim().to_owned())
        .filter(|source| !source.is_empty())
    {
        return Some(source);
    }

    [PathBuf::from("/Users/ikaros/Downloads/demo.mp4")]
        .into_iter()
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
        .or_else(test_video_path)
}

fn open_videotoolbox_smoke_packet_source_and_session(
    plugin_path: &Path,
    source: &str,
) -> (Box<VideoPacketSource>, Box<dyn NativeDecoderSession>) {
    let backend = FfmpegBackend::new().expect("FFmpeg should initialize");
    let packet_source = backend
        .open_video_packet_source(MediaSource::new(source.to_owned()))
        .unwrap_or_else(|error| panic!("failed to open packet source `{source}`: {error}"));
    let stream_info = packet_source.stream_info().clone();
    let plugin = LoadedDynamicPlugin::load(plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load VideoToolbox decoder plugin `{}`: {error}",
            plugin_path.display()
        )
    });
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("VideoToolbox plugin should export a native decoder factory");
    if !factory
        .capabilities()
        .supports_codec(&stream_info.codec, DecoderMediaKind::Video)
    {
        panic!(
            "VideoToolbox plugin does not support smoke source codec {}",
            stream_info.codec
        );
    }
    let session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: stream_info.codec.clone(),
            media_kind: DecoderMediaKind::Video,
            extradata: stream_info.extradata.clone(),
            width: stream_info.width,
            height: stream_info.height,
            prefer_hardware: true,
            require_cpu_output: false,
            ..DecoderSessionConfig::default()
        })
        .expect("VideoToolbox native session should open");
    (Box::new(packet_source), session)
}

fn decode_one_videotoolbox_frame(
    packet_source: &mut VideoPacketSource,
    session: &mut dyn NativeDecoderSession,
    max_packets: usize,
) -> bool {
    let mut submitted_packets = 0usize;
    while submitted_packets < max_packets {
        let Some(packet) = packet_source
            .next_packet()
            .expect("packet demux should succeed")
        else {
            return false;
        };
        submitted_packets = submitted_packets.saturating_add(1);
        let accepted = send_videotoolbox_packet(session, packet)
            .expect("VideoToolbox should accept compressed packet")
            .accepted;
        if !accepted {
            continue;
        }
        if receive_and_release_videotoolbox_frames(session).0 > 0 {
            return true;
        }
    }
    false
}

fn drain_videotoolbox_session_to_eof(
    packet_source: &mut VideoPacketSource,
    session: &mut dyn NativeDecoderSession,
) -> Result<(), &'static str> {
    while let Some(packet) = packet_source
        .next_packet()
        .expect("packet demux should succeed")
    {
        let _ = send_videotoolbox_packet(session, packet)
            .expect("VideoToolbox should accept compressed packet");
        if receive_and_release_videotoolbox_frames(session).1 {
            return Ok(());
        };
    }

    session
        .send_packet(
            &DecoderPacket {
                end_of_stream: true,
                ..DecoderPacket::default()
            },
            &[],
        )
        .expect("VideoToolbox should accept EOF packet");
    for _ in 0..16 {
        if receive_and_release_videotoolbox_frames(session).1 {
            return Ok(());
        }
    }
    Err("VideoToolbox did not emit EOF after end-of-stream packet")
}

fn send_videotoolbox_packet(
    session: &mut dyn NativeDecoderSession,
    packet: CompressedVideoPacket,
) -> Result<DecoderPacketResult, DecoderError> {
    session.send_packet(
        &DecoderPacket {
            pts_us: packet.pts_us,
            dts_us: packet.dts_us,
            duration_us: packet.duration_us,
            stream_index: packet.stream_index,
            media_kind: DecoderMediaKind::Video,
            key_frame: packet.key_frame,
            discontinuity: packet.discontinuity,
            end_of_stream: false,
        },
        &packet.data,
    )
}

fn receive_and_release_videotoolbox_frames(
    session: &mut dyn NativeDecoderSession,
) -> (usize, bool) {
    let mut decoded_frames = 0usize;
    loop {
        match session
            .receive_native_frame()
            .expect("VideoToolbox frame receive should succeed")
        {
            DecoderReceiveNativeFrameOutput::Frame(frame) => {
                assert_eq!(
                    frame.metadata.handle_kind,
                    DecoderNativeHandleKind::CvPixelBuffer
                );
                assert!(frame.handle != 0);
                assert!(frame.metadata.width > 0);
                assert!(frame.metadata.height > 0);
                session
                    .release_native_frame(frame)
                    .expect("native frame release should succeed");
                decoded_frames = decoded_frames.saturating_add(1);
            }
            DecoderReceiveNativeFrameOutput::NeedMoreInput => return (decoded_frames, false),
            DecoderReceiveNativeFrameOutput::Eof => return (decoded_frames, true),
        }
    }
}

fn test_fallback_bootstrap() -> PlayerRuntimeAdapterBootstrap {
    PlayerRuntimeAdapterBootstrap {
        runtime: Box::new(FakeStrategyRuntime {
            capabilities: PlayerRuntimeAdapterCapabilities {
                adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
                supports_audio_output: true,
                supports_frame_output: true,
                supports_external_video_surface: false,
                supports_seek: true,
                supports_stop: true,
                supports_playback_rate: true,
                playback_rate_min: Some(0.5),
                playback_rate_max: Some(3.0),
                natural_playback_rate_max: Some(2.0),
                supports_hardware_decode: false,
                supports_streaming: true,
                supports_hdr: false,
            },
            media_info: media_info_with_codec("H264"),
            playback_rate: 1.0,
            progress: PlaybackProgress::new(Duration::ZERO, Some(Duration::from_secs(30))),
            state: PresentationState::Ready,
            events: VecDeque::new(),
            advance_error: None,
            dispatch_error: None,
        }),
        initial_frame: None,
        startup: startup_with_video_decode(PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Software,
            hardware_available: true,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: Some("software fallback ready".to_owned()),
        }),
    }
}

fn test_fallback_bootstrap_with_plugin_diagnostic() -> PlayerRuntimeAdapterBootstrap {
    let mut bootstrap = test_fallback_bootstrap();
    bootstrap
        .startup
        .plugin_diagnostics
        .push(PlayerPluginDiagnostic {
            path: "/tmp/player-decoder-videotoolbox.dylib".to_owned(),
            plugin_name: Some("fixture-decoder".to_owned()),
            plugin_kind: Some("decoder".to_owned()),
            status: PlayerPluginDiagnosticStatus::DecoderSupported,
            message: Some("fixture decoder participated".to_owned()),
            capability: None,
            participation: PlayerPluginParticipation::Participated,
            details: Vec::new(),
        });
    bootstrap
}

#[derive(Clone)]
struct FakeStrategyFactory {
    capabilities: PlayerRuntimeAdapterCapabilities,
    media_info: PlayerMediaInfo,
    startup: PlayerRuntimeStartup,
    initialize_error: Option<PlayerError>,
    advance_error: Option<PlayerError>,
}

impl PlayerRuntimeAdapterFactory for FakeStrategyFactory {
    fn adapter_id(&self) -> &'static str {
        self.capabilities.adapter_id
    }

    fn probe_source_with_options(
        &self,
        _source: MediaSource,
        _options: PlayerRuntimeOptions,
    ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        Ok(Box::new(FakeStrategyInitializer {
            capabilities: self.capabilities.clone(),
            media_info: self.media_info.clone(),
            startup: self.startup.clone(),
            initialize_error: self.initialize_error.clone(),
            advance_error: self.advance_error.clone(),
        }))
    }
}

impl super::MacosHostFallbackFactory for FakeStrategyFactory {
    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        <Self as PlayerRuntimeAdapterFactory>::probe_source_with_options(self, source, options)
    }
}

struct FakeStrategyInitializer {
    capabilities: PlayerRuntimeAdapterCapabilities,
    media_info: PlayerMediaInfo,
    startup: PlayerRuntimeStartup,
    initialize_error: Option<PlayerError>,
    advance_error: Option<PlayerError>,
}

impl PlayerRuntimeAdapterInitializer for FakeStrategyInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.capabilities.clone()
    }

    fn media_info(&self) -> PlayerMediaInfo {
        self.media_info.clone()
    }

    fn startup(&self) -> PlayerRuntimeStartup {
        self.startup.clone()
    }

    fn initialize(self: Box<Self>) -> PlayerResult<PlayerRuntimeAdapterBootstrap> {
        let Self {
            capabilities,
            media_info,
            startup,
            initialize_error,
            advance_error,
        } = *self;

        if let Some(error) = initialize_error {
            return Err(error);
        }

        Ok(PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(FakeStrategyRuntime {
                capabilities,
                media_info,
                playback_rate: 1.0,
                progress: PlaybackProgress::new(Duration::ZERO, None),
                state: PresentationState::Ready,
                events: VecDeque::new(),
                advance_error,
                dispatch_error: None,
            }),
            initial_frame: None,
            startup,
        })
    }
}

struct FakeStrategyRuntime {
    capabilities: PlayerRuntimeAdapterCapabilities,
    media_info: PlayerMediaInfo,
    playback_rate: f32,
    progress: PlaybackProgress,
    state: PresentationState,
    events: VecDeque<PlayerRuntimeEvent>,
    advance_error: Option<PlayerError>,
    dispatch_error: Option<PlayerError>,
}

struct FakeSourceNormalizerPacketSession {
    stream_info: SourceNormalizerPacketStreamInfo,
    packets: VecDeque<SourceNormalizerPacket>,
    outstanding_handle: Option<usize>,
    released_handles: Arc<Mutex<Vec<usize>>>,
    events: Option<Arc<Mutex<Vec<&'static str>>>>,
    seek_positions: Arc<Mutex<Vec<u64>>>,
    closed: bool,
}

impl FakeSourceNormalizerPacketSession {
    fn new(stream_info: SourceNormalizerPacketStreamInfo) -> Self {
        Self {
            stream_info,
            packets: VecDeque::from([fake_source_normalizer_video_packet()]),
            outstanding_handle: None,
            released_handles: Arc::new(Mutex::new(Vec::new())),
            events: None,
            seek_positions: Arc::new(Mutex::new(Vec::new())),
            closed: false,
        }
    }

    fn with_release_log(
        stream_info: SourceNormalizerPacketStreamInfo,
        packets: Vec<SourceNormalizerPacket>,
        released_handles: Arc<Mutex<Vec<usize>>>,
    ) -> Self {
        Self {
            stream_info,
            packets: VecDeque::from(packets),
            outstanding_handle: None,
            released_handles,
            events: None,
            seek_positions: Arc::new(Mutex::new(Vec::new())),
            closed: false,
        }
    }

    fn with_seek_log(
        stream_info: SourceNormalizerPacketStreamInfo,
        events: Arc<Mutex<Vec<&'static str>>>,
        seek_positions: Arc<Mutex<Vec<u64>>>,
    ) -> Self {
        Self {
            stream_info,
            packets: VecDeque::from([fake_source_normalizer_video_packet()]),
            outstanding_handle: None,
            released_handles: Arc::new(Mutex::new(Vec::new())),
            events: Some(events),
            seek_positions,
            closed: false,
        }
    }
}

impl SourceNormalizerPacketSession for FakeSourceNormalizerPacketSession {
    fn stream_info(&self) -> SourceNormalizerPacketStreamInfo {
        self.stream_info.clone()
    }

    fn read_packet(&mut self) -> Result<SourceNormalizerPacketLease<'_>, SourceNormalizerError> {
        if self.closed {
            return Err(SourceNormalizerError::NotConfigured);
        }
        if self.outstanding_handle.is_some() {
            return Err(SourceNormalizerError::abi_violation(
                "fake packet still needs release",
            ));
        }
        let Some(packet) = self.packets.pop_front() else {
            return Ok(SourceNormalizerPacketLease {
                metadata: SourceNormalizerReadPacketMetadata::end_of_stream(),
                data: &[],
                handle: 0,
            });
        };
        let handle = self
            .released_handles
            .lock()
            .map(|handles| handles.len() + 1)
            .unwrap_or(1);
        self.outstanding_handle = Some(handle);
        Ok(SourceNormalizerPacketLease {
            metadata: SourceNormalizerReadPacketMetadata::packet(packet),
            data: &[0, 0, 1, 9],
            handle,
        })
    }

    fn release_packet(&mut self, packet_handle: usize) -> Result<(), SourceNormalizerError> {
        if self.outstanding_handle == Some(packet_handle) {
            self.outstanding_handle = None;
            if let Ok(mut released_handles) = self.released_handles.lock() {
                released_handles.push(packet_handle);
            }
            Ok(())
        } else {
            Err(SourceNormalizerError::abi_violation(
                "fake packet handle was not outstanding",
            ))
        }
    }

    fn seek(
        &mut self,
        seek: &SourceNormalizerPacketSeek,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        if let Some(events) = &self.events
            && let Ok(mut events) = events.lock()
        {
            events.push("source_normalizer_seek");
        }
        if let Ok(mut seek_positions) = self.seek_positions.lock() {
            seek_positions.push(seek.position_millis);
        }
        self.outstanding_handle = None;
        Ok(SourceNormalizerOperationStatus {
            completed: true,
            message: None,
        })
    }

    fn flush(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        if let Some(events) = &self.events
            && let Ok(mut events) = events.lock()
        {
            events.push("source_normalizer_flush");
        }
        self.outstanding_handle = None;
        Ok(SourceNormalizerOperationStatus {
            completed: true,
            message: None,
        })
    }

    fn close(&mut self) -> Result<(), SourceNormalizerError> {
        self.closed = true;
        self.outstanding_handle = None;
        Ok(())
    }
}

impl PlayerRuntimeAdapter for FakeStrategyRuntime {
    fn source_uri(&self) -> &str {
        &self.media_info.source_uri
    }

    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.capabilities.clone()
    }

    fn media_info(&self) -> &PlayerMediaInfo {
        &self.media_info
    }

    fn presentation_state(&self) -> PresentationState {
        self.state
    }

    fn playback_rate(&self) -> f32 {
        self.playback_rate
    }

    fn progress(&self) -> PlaybackProgress {
        self.progress
    }

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.events.drain(..).collect()
    }

    fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        if let Some(error) = self.dispatch_error.take() {
            return Err(error);
        }
        match command {
            PlayerRuntimeCommand::Play => {
                self.state = PresentationState::Playing;
            }
            PlayerRuntimeCommand::SeekTo { position } => {
                self.progress = PlaybackProgress::new(position, self.progress.duration());
            }
            PlayerRuntimeCommand::SetPlaybackRate { rate } => {
                self.playback_rate = rate;
            }
            _ => {}
        }
        Ok(PlayerRuntimeCommandResult {
            applied: true,
            frame: None,
            snapshot: self.snapshot(),
        })
    }

    fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>> {
        if let Some(error) = self.advance_error.take() {
            return Err(error);
        }
        Ok(None)
    }

    fn next_deadline(&self) -> Option<Instant> {
        None
    }
}

#[derive(Debug)]
struct FakeNativeFramePacketSource {
    stream_info: VideoPacketStreamInfo,
    packets: VecDeque<CompressedVideoPacket>,
    seek_packets: Vec<CompressedVideoPacket>,
    events: Arc<std::sync::Mutex<Vec<&'static str>>>,
}

impl FakeNativeFramePacketSource {
    fn with_seek_packets(
        packets: Vec<CompressedVideoPacket>,
        seek_packets: Vec<CompressedVideoPacket>,
        events: Arc<std::sync::Mutex<Vec<&'static str>>>,
    ) -> Self {
        Self {
            stream_info: test_video_packet_stream_info(),
            packets: packets.into(),
            seek_packets,
            events,
        }
    }
}

impl MacosNativeFramePacketSource for FakeNativeFramePacketSource {
    fn send_next_packet(
        &mut self,
        decoder_session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    ) -> anyhow::Result<MacosNativeFramePacketSendStatus> {
        let Some(packet) = self.packets.pop_front() else {
            return Ok(MacosNativeFramePacketSendStatus::EndOfStream);
        };
        send_macos_native_frame_packet(decoder_session, packet)?;
        Ok(MacosNativeFramePacketSendStatus::Sent)
    }

    fn seek_to(&mut self, _position: Duration) -> anyhow::Result<()> {
        if let Ok(mut events) = self.events.lock() {
            events.push("packet_seek");
        }
        self.packets = self.seek_packets.clone().into();
        Ok(())
    }
}

#[derive(Debug, Default)]
struct RecordingNativeDecoderState {
    events: Arc<std::sync::Mutex<Vec<&'static str>>>,
    sent_packets: Vec<DecoderPacket>,
    queued_frames: VecDeque<DecoderReceiveNativeFrameOutput>,
    reject_next_packet: bool,
    next_handle: usize,
    released_handles: usize,
    flush_count: usize,
    close_count: usize,
}

impl RecordingNativeDecoderState {
    fn shared(events: Arc<std::sync::Mutex<Vec<&'static str>>>) -> Arc<std::sync::Mutex<Self>> {
        Arc::new(std::sync::Mutex::new(Self {
            events,
            next_handle: 100,
            ..Self::default()
        }))
    }
}

struct RecordingNativeDecoderSession {
    state: Arc<std::sync::Mutex<RecordingNativeDecoderState>>,
}

impl NativeDecoderSession for RecordingNativeDecoderSession {
    fn session_info(&self) -> DecoderSessionInfo {
        DecoderSessionInfo {
            decoder_name: Some("recording-native-decoder".to_owned()),
            selected_hardware_backend: Some("fixture-native".to_owned()),
            output_format: Some(player_plugin::DecoderFrameFormat::Nv12),
        }
    }

    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        _data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| DecoderError::internal("recording session state is poisoned"))?;
        if let Ok(mut events) = state.events.lock() {
            events.push(if packet.end_of_stream {
                "send_eos"
            } else {
                "send_packet"
            });
        }
        state.sent_packets.push(packet.clone());
        if state.reject_next_packet {
            state.reject_next_packet = false;
            return Ok(DecoderPacketResult { accepted: false });
        }
        if packet.end_of_stream {
            state
                .queued_frames
                .push_back(DecoderReceiveNativeFrameOutput::Eof);
        } else {
            let handle = state.next_handle;
            state.next_handle = state.next_handle.saturating_add(1);
            state
                .queued_frames
                .push_back(DecoderReceiveNativeFrameOutput::Frame(test_native_frame(
                    handle,
                    packet.pts_us,
                )));
        }
        Ok(DecoderPacketResult { accepted: true })
    }

    fn receive_native_frame(&mut self) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
        self.state
            .lock()
            .map_err(|_| DecoderError::internal("recording session state is poisoned"))
            .map(|mut state| {
                state
                    .queued_frames
                    .pop_front()
                    .unwrap_or(DecoderReceiveNativeFrameOutput::NeedMoreInput)
            })
    }

    fn release_native_frame(&mut self, _frame: DecoderNativeFrame) -> Result<(), DecoderError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| DecoderError::internal("recording session state is poisoned"))?;
        state.released_handles = state.released_handles.saturating_add(1);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), DecoderError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| DecoderError::internal("recording session state is poisoned"))?;
        if let Ok(mut events) = state.events.lock() {
            events.push("flush");
        }
        state.flush_count = state.flush_count.saturating_add(1);
        state.queued_frames.clear();
        Ok(())
    }

    fn close(&mut self) -> Result<(), DecoderError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| DecoderError::internal("recording session state is poisoned"))?;
        if let Ok(mut events) = state.events.lock() {
            events.push("close");
        }
        state.close_count = state.close_count.saturating_add(1);
        Ok(())
    }
}

fn native_frame_source_for_test(
    packet_source: FakeNativeFramePacketSource,
    session_state: Arc<std::sync::Mutex<RecordingNativeDecoderState>>,
    outstanding_frames: Arc<AtomicUsize>,
    end_of_input_sent: bool,
    end_of_stream_received: bool,
) -> MacosNativeFrameVideoSource {
    let stream_info = packet_source.stream_info.clone();
    let session: Arc<std::sync::Mutex<Box<dyn NativeDecoderSession>>> = Arc::new(
        std::sync::Mutex::new(Box::new(RecordingNativeDecoderSession {
            state: session_state,
        })),
    );
    let shared = Arc::new(std::sync::Mutex::new(MacosNativeFrameDecoderState {
        frame_processor_chain: None,
        presenter: None,
        presentation_epoch: 0,
    }));
    let (command_tx, command_rx) = mpsc::channel();
    let (frame_tx, frame_rx) = mpsc::channel();
    let current_generation = Arc::new(AtomicU64::new(0));
    let buffered_frame_count = Arc::new(AtomicUsize::new(0));
    let prefetch_limit = Arc::new(AtomicUsize::new(1));
    let prefetch_wakeup = Arc::new(MacosNativeFramePrefetchWakeup::default());
    let worker = spawn_macos_native_frame_prefetch_worker(
        Box::new(packet_source),
        session.clone(),
        shared.clone(),
        outstanding_frames.clone(),
        command_rx,
        frame_tx,
        current_generation.clone(),
        buffered_frame_count.clone(),
        prefetch_limit.clone(),
        prefetch_wakeup.clone(),
    )
    .expect("test prefetch worker should spawn");
    MacosNativeFrameVideoSource {
        stream_info,
        session,
        shared,
        outstanding_frames,
        command_tx,
        frame_rx,
        generation: 0,
        current_generation,
        buffered_frame_count,
        prefetch_limit,
        prefetch_wakeup,
        end_of_input_sent,
        end_of_stream_received,
        worker: Some(worker),
    }
}

fn contains_ordered_events(events: &[&'static str], expected: &[&'static str]) -> bool {
    let mut next_expected = 0;
    for event in events {
        if expected
            .get(next_expected)
            .is_some_and(|expected| expected == event)
        {
            next_expected += 1;
            if next_expected == expected.len() {
                return true;
            }
        }
    }
    expected.is_empty()
}

fn wait_for_buffered_native_frame(source: &MacosNativeFrameVideoSource, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(1);
    while source.buffered_frame_count() < expected {
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn test_video_packet_stream_info() -> VideoPacketStreamInfo {
    VideoPacketStreamInfo {
        stream_index: 0,
        codec: "H264".to_owned(),
        extradata: Vec::new(),
        width: Some(320),
        height: Some(180),
        frame_rate: Some(24.0),
    }
}

fn fake_source_normalizer_packet_stream_info(codec: &str) -> SourceNormalizerPacketStreamInfo {
    SourceNormalizerPacketStreamInfo {
        session_id: Some("fake-session".to_owned()),
        normalizer_name: Some("fake-normalizer".to_owned()),
        runtime_profile: Some("fixture-packet".to_owned()),
        selected_backend: Some("fake".to_owned()),
        tracks: vec![SourceNormalizerPacketTrackInfo {
            stream_index: 0,
            media_kind: SourceNormalizerPacketMediaKind::Video,
            codec: codec.to_owned(),
            extradata: Vec::new(),
            bitstream_format: Some(DecoderBitstreamFormat::Avcc),
            width: Some(320),
            height: Some(180),
            coded_width: Some(320),
            coded_height: Some(180),
            sample_rate: None,
            channels: None,
            channel_layout: None,
            codec_delay_samples: None,
            priming_samples: None,
            trailing_padding_samples: None,
            seek_preroll_samples: None,
            frame_rate: Some(24.0),
            time_base_num: Some(1),
            time_base_den: Some(24_000),
        }],
        selected_track_index: Some(0),
        duration_millis: Some(1_000),
        seekable: true,
    }
}

fn fake_source_normalizer_packet_stream_info_with_audio(
    video_codec: &str,
    audio_codec: &str,
) -> SourceNormalizerPacketStreamInfo {
    let mut stream_info = fake_source_normalizer_packet_stream_info(video_codec);
    stream_info.tracks.push(SourceNormalizerPacketTrackInfo {
        stream_index: 1,
        media_kind: SourceNormalizerPacketMediaKind::Audio,
        codec: audio_codec.to_owned(),
        extradata: Vec::new(),
        bitstream_format: None,
        width: None,
        height: None,
        coded_width: None,
        coded_height: None,
        sample_rate: Some(48_000),
        channels: Some(2),
        channel_layout: Some("stereo".to_owned()),
        codec_delay_samples: None,
        priming_samples: None,
        trailing_padding_samples: None,
        seek_preroll_samples: Some(1_024),
        frame_rate: None,
        time_base_num: Some(1),
        time_base_den: Some(48_000),
    });
    stream_info
}

fn fake_source_normalizer_video_packet() -> SourceNormalizerPacket {
    SourceNormalizerPacket {
        pts_us: Some(0),
        dts_us: Some(0),
        duration_us: Some(41_667),
        stream_index: 0,
        media_kind: SourceNormalizerPacketMediaKind::Video,
        key_frame: true,
        discontinuity: false,
        sample_rate: None,
        channels: None,
        channel_layout: None,
        sample_format: None,
        frame_count: None,
        end_of_stream: false,
    }
}

fn fake_source_normalizer_audio_packet() -> SourceNormalizerPacket {
    SourceNormalizerPacket {
        pts_us: Some(0),
        dts_us: Some(0),
        duration_us: Some(21_333),
        stream_index: 1,
        media_kind: SourceNormalizerPacketMediaKind::Audio,
        key_frame: true,
        discontinuity: false,
        sample_rate: Some(48_000),
        channels: Some(2),
        channel_layout: Some("stereo".to_owned()),
        sample_format: Some("fltp".to_owned()),
        frame_count: Some(1024),
        end_of_stream: false,
    }
}

fn test_compressed_packet(pts_us: i64) -> CompressedVideoPacket {
    CompressedVideoPacket {
        pts_us: Some(pts_us),
        dts_us: Some(pts_us),
        duration_us: Some(41_667),
        stream_index: 0,
        key_frame: true,
        discontinuity: false,
        data: vec![0, 0, 1, 9],
    }
}

fn test_native_frame(handle: usize, pts_us: Option<i64>) -> DecoderNativeFrame {
    DecoderNativeFrame {
        metadata: DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: player_plugin::DecoderFrameFormat::Nv12,
            codec: "H264".to_owned(),
            pts_us,
            duration_us: Some(41_667),
            width: 320,
            height: 180,
            coded_width: Some(320),
            coded_height: Some(180),
            visible_rect: None,
            handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
            pipeline_profile: Some(
                player_plugin::NativeFramePipelineProfile::VideoToolboxCvPixelBuffer,
            ),
            color_space: None,
            hdr_metadata: None,
            sync_info: None,
            transform: None,
            frame_id: Some(handle as u64),
            release_tracking: None,
        },
        handle,
    }
}

fn macos_frame_processor_frame_for_test(
    handle: usize,
    pts_us: Option<i64>,
) -> super::MacosFrameProcessorFrame {
    let frame = test_native_frame(handle, pts_us);
    super::MacosFrameProcessorFrame(NativeFrameProcessorProcessedFrame {
        decoder_frame: frame.clone(),
        presentation_frame: frame,
        processor_outputs: Vec::new(),
    })
}

fn macos_frame_processor_frame_for_test_with_processor_output(
    handle: usize,
    pts_us: Option<i64>,
    processor_handle: usize,
) -> super::MacosFrameProcessorFrame {
    let frame = test_native_frame(handle, pts_us);
    let mut processor_frame = frame.clone();
    processor_frame.handle = processor_handle;
    processor_frame.metadata.frame_id = Some(processor_handle as u64);
    super::MacosFrameProcessorFrame(NativeFrameProcessorProcessedFrame {
        decoder_frame: frame.clone(),
        presentation_frame: frame,
        processor_outputs: vec![super::ProcessorOwnedNativeFrame {
            processor_index: 0,
            frame: NativeFrame {
                metadata: processor_frame.metadata.into(),
                handle: processor_frame.handle,
            },
        }],
    })
}

#[derive(Default)]
struct FakeNativeDecoderSession {
    released_handles: usize,
}

impl NativeDecoderSession for FakeNativeDecoderSession {
    fn session_info(&self) -> DecoderSessionInfo {
        DecoderSessionInfo {
            decoder_name: Some(format!("released={}", self.released_handles)),
            selected_hardware_backend: None,
            output_format: None,
        }
    }

    fn send_packet(
        &mut self,
        _packet: &DecoderPacket,
        _data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        Ok(DecoderPacketResult { accepted: true })
    }

    fn receive_native_frame(&mut self) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
        Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput)
    }

    fn release_native_frame(&mut self, _frame: DecoderNativeFrame) -> Result<(), DecoderError> {
        self.released_handles = self.released_handles.saturating_add(1);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), DecoderError> {
        Ok(())
    }

    fn close(&mut self) -> Result<(), DecoderError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct RecordingFrameProcessorState {
    events: Option<Arc<std::sync::Mutex<Vec<&'static str>>>>,
    submit_status: Option<FrameProcessorSubmitStatus>,
    receive_pending: bool,
    output_handle_offset: usize,
    output_requires_release: Option<bool>,
    submit_to_ready_us: Option<u64>,
    forced_queue_depth: Option<u32>,
    forced_in_flight_frames: Option<u32>,
    submitted_handles: Vec<usize>,
    released_handles: Vec<usize>,
    flush_count: usize,
    close_count: usize,
}

struct RecordingFrameProcessorSession {
    state: Arc<std::sync::Mutex<RecordingFrameProcessorState>>,
    pending: Option<FrameProcessorOutputFrame>,
}

impl RecordingFrameProcessorSession {
    fn new(state: Arc<std::sync::Mutex<RecordingFrameProcessorState>>) -> Self {
        Self {
            state,
            pending: None,
        }
    }
}

impl FrameProcessorSession for RecordingFrameProcessorSession {
    fn session_info(&self) -> FrameProcessorSessionInfo {
        FrameProcessorSessionInfo {
            processor_name: Some("recording-frame-processor".to_owned()),
            selected_backend: Some("fixture".to_owned()),
            output_handle_kind: Some(player_plugin::NativeHandleKind::CvPixelBuffer),
            output_pipeline_profile: Some(
                player_plugin::NativeFramePipelineProfile::VideoToolboxCvPixelBuffer,
            ),
            max_in_flight_frames: Some(1),
        }
    }

    fn submit_frame(
        &mut self,
        frame: &NativeFrame,
        _submit: &FrameProcessorSubmitFrame,
    ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| FrameProcessorError::internal("recording processor poisoned"))?;
        state.submitted_handles.push(frame.handle);
        if let Some(status) = state.submit_status {
            return Ok(FrameProcessorSubmitResult {
                status,
                queue_depth: Some(state.forced_queue_depth.unwrap_or(0)),
                in_flight_frames: Some(state.forced_in_flight_frames.unwrap_or(0)),
                message: Some("forced submit status".to_owned()),
            });
        }
        if state.receive_pending {
            return Ok(FrameProcessorSubmitResult::default());
        }
        let mut output_metadata = frame.metadata.clone();
        output_metadata.frame_id = output_metadata
            .frame_id
            .map(|frame_id| frame_id.saturating_add(10_000));
        if let Some(requires_release) = state.output_requires_release {
            output_metadata.release_tracking = Some(player_plugin::NativeFrameReleaseTracking {
                frame_id: output_metadata.frame_id,
                requires_release,
            });
        }
        let output_handle = state.output_handle_offset.saturating_add(frame.handle);
        self.pending = Some(FrameProcessorOutputFrame {
            frame: NativeFrame {
                metadata: output_metadata,
                handle: output_handle,
            },
            timings: FrameProcessorFrameTimings {
                queue_wait_us: Some(0),
                process_time_us: state.submit_to_ready_us,
                submit_to_ready_us: state.submit_to_ready_us.or(Some(100)),
            },
            source_frame_id: frame.metadata.frame_id,
        });
        Ok(FrameProcessorSubmitResult::default())
    }

    fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError> {
        let receive_pending = self
            .state
            .lock()
            .map_err(|_| FrameProcessorError::internal("recording processor poisoned"))?
            .receive_pending;
        if receive_pending {
            Ok(FrameProcessorReceiveOutput::Pending)
        } else if let Some(output) = self.pending.take() {
            Ok(FrameProcessorReceiveOutput::Frame(output))
        } else {
            Ok(FrameProcessorReceiveOutput::Pending)
        }
    }

    fn release_frame(&mut self, frame: NativeFrame) -> Result<(), FrameProcessorError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| FrameProcessorError::internal("recording processor poisoned"))?;
        state.released_handles.push(frame.handle);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), FrameProcessorError> {
        self.pending = None;
        let mut state = self
            .state
            .lock()
            .map_err(|_| FrameProcessorError::internal("recording processor poisoned"))?;
        if let Some(events) = state.events.as_ref()
            && let Ok(mut events) = events.lock()
        {
            events.push("processor_flush");
        }
        state.flush_count = state.flush_count.saturating_add(1);
        Ok(())
    }

    fn close(&mut self) -> Result<(), FrameProcessorError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| FrameProcessorError::internal("recording processor poisoned"))?;
        if let Some(events) = state.events.as_ref()
            && let Ok(mut events) = events.lock()
        {
            events.push("processor_close");
        }
        state.close_count = state.close_count.saturating_add(1);
        Ok(())
    }
}

fn frame_processor_chain_for_test(
    mode: FrameProcessorMode,
    sessions: Vec<RecordingFrameProcessorSession>,
) -> MacosFrameProcessorChain {
    MacosFrameProcessorChain {
        core: NativeFrameProcessorChainCore::new(
            sessions
                .into_iter()
                .enumerate()
                .map(|(processor_index, session)| {
                    NativeFrameProcessorNode::new(
                        format!("recording-frame-processor-{processor_index}"),
                        processor_index,
                        Box::new(session),
                    )
                })
                .collect(),
            mode,
            FrameProcessorPolicy {
                frame_deadline: Duration::from_millis(16),
                late_output_tolerance: Duration::from_millis(4),
                max_chain_depth: 8,
                max_in_flight_frames_per_processor: 1,
            },
        ),
        debug: FrameProcessorDebugState::from_env(),
    }
}
