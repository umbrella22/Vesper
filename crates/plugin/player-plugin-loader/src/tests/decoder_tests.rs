use super::*;

#[test]
fn dynamic_decoder_plugin_rejects_legacy_descriptor_abi() {
    let api = fixture_native_decoder_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: 1,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
    };

    let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect_err("legacy ABI descriptors should be rejected");

    assert!(matches!(
        error,
        PluginLoadError::AbiVersionMismatch {
            expected: 4,
            actual: 1
        }
    ));
}

#[test]
fn dynamic_decoder_plugin_surfaces_error_payloads() {
    let api = fixture_native_decoder_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
    };
    let plugin =
        LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load decoder plugin");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("decoder factory should be available");

    let error = match factory.open_native_session(&DecoderSessionConfig {
        codec: "missing-codec".to_owned(),
        media_kind: DecoderMediaKind::Video,
        ..DecoderSessionConfig::default()
    }) {
        Ok(_) => panic!("unsupported codec should fail"),
        Err(error) => error,
    };

    assert!(matches!(error, DecoderError::UnsupportedCodec { .. }));
}

#[test]
fn dynamic_native_decoder_plugin_adapter_round_trips_native_frame() {
    let api = fixture_native_decoder_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load native decoder plugin");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("native decoder factory should be available");
    assert_eq!(factory.name(), "fixture-decoder");
    assert!(factory.capabilities().supports_hardware_decode);
    assert!(factory.capabilities().supports_gpu_handles);

    let mut session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: "fixture-video".to_owned(),
            media_kind: DecoderMediaKind::Video,
            prefer_hardware: true,
            require_cpu_output: false,
            ..DecoderSessionConfig::default()
        })
        .expect("open native decoder session");
    assert_eq!(
        session.session_info().selected_hardware_backend.as_deref(),
        Some("fixture-native")
    );

    let send = session
        .send_packet(
            &DecoderPacket {
                pts_us: Some(2_000),
                key_frame: true,
                ..DecoderPacket::default()
            },
            &[9, 8, 7, 6],
        )
        .expect("send native packet");
    assert!(send.accepted);

    let frame = session
        .receive_native_frame()
        .expect("receive native frame");
    let frame = match frame {
        DecoderReceiveNativeFrameOutput::Frame(frame) => frame,
        other => panic!("expected native frame, got {other:?}"),
    };
    assert_ne!(frame.handle, 0);
    assert_eq!(frame.metadata.pts_us, Some(2_000));
    assert_eq!(
        frame.metadata.handle_kind,
        DecoderNativeHandleKind::IoSurface
    );
    session
        .release_native_frame(frame)
        .expect("release native frame");
    assert_eq!(
        session.receive_native_frame().expect("need more input"),
        DecoderReceiveNativeFrameOutput::NeedMoreInput
    );
    session.close().expect("close native session");
}

#[test]
fn dynamic_native_decoder_plugin_v3_reports_pcm_receive_unsupported() {
    let api = fixture_native_decoder_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load native decoder plugin");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("native decoder factory should be available");
    let mut session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: "fixture-video".to_owned(),
            media_kind: DecoderMediaKind::Video,
            ..DecoderSessionConfig::default()
        })
        .expect("open native decoder session");

    let error = session
        .receive_pcm_frame()
        .expect_err("v3 decoder table should not expose PCM receive");
    assert!(matches!(
        error,
        DecoderError::UnsupportedCapability { capability }
            if capability == "audio-pcm-output"
    ));
}

#[test]
fn dynamic_native_decoder_plugin_rejects_pcm_capabilities_without_pcm_callback() {
    let api = VesperDecoderPluginApiV2 {
        capabilities_json: Some(fixture_native_decoder_pcm_capabilities_json),
        ..fixture_native_decoder_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
    };

    let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect_err("PCM capability without receive_pcm_frame should fail at load time");

    assert!(matches!(
        error,
        PluginLoadError::CapabilitiesAbiViolation(message)
            if message.contains("advertises PCM frame output")
                && message.contains("receive_pcm_frame")
    ));
}

#[test]
fn dynamic_native_decoder_plugin_v4_round_trips_pcm_frame_bytes() {
    let api = fixture_native_decoder_pcm_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V4,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV4).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load native decoder plugin");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("native decoder factory should be available");
    assert!(
        factory
            .capabilities()
            .supports_codec("fixture-audio", DecoderMediaKind::Audio)
    );
    let mut session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: "fixture-audio".to_owned(),
            media_kind: DecoderMediaKind::Audio,
            ..DecoderSessionConfig::default()
        })
        .expect("open native decoder session");

    session
        .send_packet(
            &DecoderPacket {
                pts_us: Some(7_000),
                media_kind: DecoderMediaKind::Audio,
                key_frame: true,
                ..DecoderPacket::default()
            },
            &[1, 2, 3, 4],
        )
        .expect("send packet");

    let frame = match session.receive_pcm_frame().expect("receive PCM frame") {
        DecoderReceivePcmFrameOutput::Frame(frame) => frame,
        other => panic!("expected PCM frame, got {other:?}"),
    };
    assert_eq!(frame.metadata.media_kind, DecoderMediaKind::Audio);
    assert_eq!(frame.metadata.format, DecoderFrameFormat::F32);
    assert_eq!(frame.metadata.pts_us, Some(7_000));
    assert_eq!(frame.metadata.sample_rate, 48_000);
    assert_eq!(frame.metadata.channels, 2);
    assert_eq!(frame.metadata.channel_layout.as_deref(), Some("stereo"));
    assert_eq!(frame.data, vec![1, 2, 3, 4]);
    assert_eq!(
        session.receive_pcm_frame().expect("need more input"),
        DecoderReceivePcmFrameOutput::NeedMoreInput
    );
}

#[test]
fn dynamic_native_decoder_plugin_receives_audio_pcm_session_preferences() {
    let context = FixtureDecoderRecordingContext::default();
    let api = VesperDecoderPluginApiV4 {
        context: (&context as *const FixtureDecoderRecordingContext)
            .cast_mut()
            .cast::<c_void>(),
        open_session_json: Some(fixture_native_decoder_recording_open_session_json),
        ..fixture_native_decoder_pcm_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V4,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV4).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load native decoder plugin");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("native decoder factory should be available");

    let _session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: "fixture-audio".to_owned(),
            media_kind: DecoderMediaKind::Audio,
            extradata: vec![0x12, 0x10],
            bitstream_format: Some(DecoderBitstreamFormat::Unknown("adts".to_owned())),
            sample_rate: Some(48_000),
            channels: Some(2),
            channel_layout: Some("stereo".to_owned()),
            target_pcm_format: Some(DecoderFrameFormat::F32),
            target_pcm_sample_layout: Some(DecoderPcmSampleLayout::Interleaved),
            ..DecoderSessionConfig::default()
        })
        .expect("open native audio decoder session");

    let configs = context.opened_configs();
    assert_eq!(configs.len(), 1);
    let config = &configs[0];
    assert_eq!(config.media_kind, DecoderMediaKind::Audio);
    assert_eq!(config.sample_rate, Some(48_000));
    assert_eq!(config.channels, Some(2));
    assert_eq!(config.channel_layout.as_deref(), Some("stereo"));
    assert_eq!(config.target_pcm_format, Some(DecoderFrameFormat::F32));
    assert_eq!(
        config.target_pcm_sample_layout,
        Some(DecoderPcmSampleLayout::Interleaved)
    );
}

#[test]
fn dynamic_native_decoder_plugin_v4_reclaims_pcm_data_on_malformed_metadata() {
    let recorder = FixtureFreeBytesRecorder::default();
    let api = VesperDecoderPluginApiV4 {
        context: (&recorder as *const FixtureFreeBytesRecorder)
            .cast_mut()
            .cast::<c_void>(),
        free_bytes: Some(fixture_recording_free_bytes),
        receive_pcm_frame: Some(fixture_decoder_receive_pcm_frame_missing_frame),
        ..fixture_native_decoder_pcm_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V4,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV4).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load native decoder plugin");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("native decoder factory should be available");
    let mut session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: "fixture-audio".to_owned(),
            media_kind: DecoderMediaKind::Audio,
            ..DecoderSessionConfig::default()
        })
        .expect("open native audio decoder session");

    // Snapshot freed buffers up to (and including) session setup so the assertion
    // only inspects what `receive_pcm_frame` reclaims.
    let freed_before = recorder.freed_lens().len();
    let error = session
        .receive_pcm_frame()
        .expect_err("frame status without frame metadata is an ABI violation");
    assert!(matches!(error, DecoderError::AbiViolation { .. }));

    // The 4096-byte PCM buffer owned by the result must be reclaimed even though
    // the metadata was malformed; otherwise the plugin allocation leaks.
    let freed_after = &recorder.freed_lens()[freed_before..];
    assert!(
        freed_after.contains(&4096),
        "receive_pcm_frame error path must free the PCM data buffer, freed: {freed_after:?}"
    );
}

#[test]
fn dynamic_native_decoder_plugin_close_releases_unreturned_native_frames() {
    let api = fixture_native_decoder_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load native decoder plugin");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("native decoder factory should be available");
    let mut session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: "fixture-video".to_owned(),
            media_kind: DecoderMediaKind::Video,
            prefer_hardware: true,
            require_cpu_output: false,
            ..DecoderSessionConfig::default()
        })
        .expect("open native decoder session");

    session
        .send_packet(
            &DecoderPacket {
                pts_us: Some(3_000),
                key_frame: true,
                ..DecoderPacket::default()
            },
            &[1, 2, 3, 4],
        )
        .expect("send native packet");
    let frame = match session
        .receive_native_frame()
        .expect("receive native frame")
    {
        DecoderReceiveNativeFrameOutput::Frame(frame) => frame,
        other => panic!("expected native frame, got {other:?}"),
    };
    let handle = frame.handle;

    session
        .close()
        .expect("close should release outstanding frame");

    assert!(native_frame_releases().contains(&handle));
}

#[test]
fn dynamic_native_decoder_plugin_rejects_duplicate_native_frame_release() {
    let api = fixture_native_decoder_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load native decoder plugin");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("native decoder factory should be available");
    let mut session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: "fixture-video".to_owned(),
            media_kind: DecoderMediaKind::Video,
            prefer_hardware: true,
            require_cpu_output: false,
            ..DecoderSessionConfig::default()
        })
        .expect("open native decoder session");

    session
        .send_packet(
            &DecoderPacket {
                pts_us: Some(4_000),
                key_frame: true,
                ..DecoderPacket::default()
            },
            &[5, 6, 7, 8],
        )
        .expect("send native packet");
    let frame = match session
        .receive_native_frame()
        .expect("receive native frame")
    {
        DecoderReceiveNativeFrameOutput::Frame(frame) => frame,
        other => panic!("expected native frame, got {other:?}"),
    };
    let duplicate = frame.clone();

    session
        .release_native_frame(frame)
        .expect("first release should succeed");
    let error = session
        .release_native_frame(duplicate)
        .expect_err("duplicate release should be rejected before plugin callback");

    assert!(matches!(error, DecoderError::AbiViolation { .. }));
}

#[test]
fn dynamic_native_decoder_plugin_exposes_native_requirements() {
    let api = fixture_native_decoder_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load native decoder plugin");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("native decoder factory should be available");
    let requirements = factory.native_requirements();

    assert!(
        requirements
            .output_handle_kinds
            .contains(&DecoderNativeHandleKind::IoSurface)
    );
    assert!(!requirements.requires_native_device_context);
}

#[test]
fn dynamic_native_decoder_plugin_receives_native_device_context() {
    let api = fixture_native_decoder_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load native decoder plugin");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("native decoder factory should be available");

    let session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: "fixture-video".to_owned(),
            media_kind: DecoderMediaKind::Video,
            prefer_hardware: true,
            require_cpu_output: false,
            native_device_context: Some(DecoderNativeDeviceContext::D3D11Device { device_ptr: 42 }),
            ..DecoderSessionConfig::default()
        })
        .expect("open native decoder session");

    assert_eq!(
        session.session_info().selected_hardware_backend.as_deref(),
        Some("fixture-native-d3d11-device-42")
    );
}

#[test]
fn dynamic_native_decoder_plugin_rejects_null_native_frame_handles() {
    let api = VesperDecoderPluginApiV2 {
        receive_native_frame: Some(fixture_decoder_receive_null_native_frame),
        ..fixture_native_decoder_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load native decoder plugin");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("native decoder factory should be available");
    let mut session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: "fixture-video".to_owned(),
            media_kind: DecoderMediaKind::Video,
            ..DecoderSessionConfig::default()
        })
        .expect("open native decoder session");
    session
        .send_packet(&DecoderPacket::default(), &[1])
        .expect("send packet");

    let error = session
        .receive_native_frame()
        .expect_err("null native frame handle should fail");
    assert!(matches!(error, DecoderError::AbiViolation { .. }));
}

#[test]
fn dynamic_native_decoder_plugin_rejects_old_v2_abi_revision() {
    let api = fixture_native_decoder_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_PLUGIN_ABI_VERSION_V2,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
    };

    let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect_err("old native-frame v2 ABI revision should be rejected");

    assert!(matches!(
        error,
        PluginLoadError::AbiVersionMismatch { actual: 2, .. }
    ));
}

#[test]
#[ignore = "requires a built player-decoder-fixture shared library artifact"]
fn dynamic_loader_opens_real_decoder_fixture_shared_library() {
    let plugin_path = resolve_decoder_fixture_plugin_path()
        .unwrap_or_else(|error| panic!("failed to resolve fixture decoder path: {error}"));

    let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load decoder fixture shared library `{}`: {error}",
            plugin_path.display()
        )
    });

    assert_eq!(plugin.plugin_name(), "player-decoder-fixture");
    assert!(plugin.post_download_processor().is_none());
    assert!(plugin.pipeline_event_hook().is_none());
    assert!(plugin.native_decoder_plugin_factory().is_some());
}

#[test]
#[ignore = "requires a built player-decoder-fixture shared library artifact"]
fn dynamic_loader_opens_real_decoder_fixture_shared_library_as_native_v2() {
    let plugin_path = resolve_decoder_fixture_plugin_path()
        .unwrap_or_else(|error| panic!("failed to resolve fixture decoder path: {error}"));

    let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load decoder fixture shared library `{}` as v2: {error}",
            plugin_path.display()
        )
    });

    assert_eq!(plugin.plugin_name(), "player-decoder-fixture");
    assert!(plugin.post_download_processor().is_none());
    assert!(plugin.pipeline_event_hook().is_none());
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("player-decoder-fixture should export a native decoder factory in v2 mode");
    assert!(factory.capabilities().supports_hardware_decode);
    assert!(factory.capabilities().supports_gpu_handles);
}

#[test]
#[ignore = "requires a built player-decoder-fixture shared library artifact"]
fn dynamic_loader_opens_real_decoder_fixture_shared_library_audio_pcm_path() {
    let plugin_path = resolve_decoder_fixture_plugin_path()
        .unwrap_or_else(|error| panic!("failed to resolve fixture decoder path: {error}"));

    let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load decoder fixture shared library `{}` for PCM path: {error}",
            plugin_path.display()
        )
    });
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("player-decoder-fixture should export a native decoder factory");
    assert!(
        factory
            .capabilities()
            .supports_codec("fixture-audio", DecoderMediaKind::Audio)
    );

    let mut session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: "fixture-audio".to_owned(),
            media_kind: DecoderMediaKind::Audio,
            sample_rate: Some(48_000),
            channels: Some(2),
            target_pcm_format: Some(DecoderFrameFormat::F32),
            target_pcm_sample_layout: Some(DecoderPcmSampleLayout::Interleaved),
            ..DecoderSessionConfig::default()
        })
        .expect("real fixture decoder should open an audio PCM session");
    assert_eq!(
        session.session_info().output_format,
        Some(DecoderFrameFormat::F32)
    );

    session
        .send_packet(
            &DecoderPacket {
                pts_us: Some(11_000),
                media_kind: DecoderMediaKind::Audio,
                ..DecoderPacket::default()
            },
            &[10, 20, 30, 40],
        )
        .expect("real fixture decoder should accept an audio packet");

    let frame = match session.receive_pcm_frame().expect("receive real PCM frame") {
        DecoderReceivePcmFrameOutput::Frame(frame) => frame,
        other => panic!("expected PCM frame from real fixture decoder, got {other:?}"),
    };
    assert_eq!(frame.metadata.media_kind, DecoderMediaKind::Audio);
    assert_eq!(frame.metadata.codec, "fixture-audio");
    assert_eq!(frame.metadata.format, DecoderFrameFormat::F32);
    assert_eq!(frame.metadata.pts_us, Some(11_000));
    assert_eq!(frame.metadata.sample_rate, 48_000);
    assert_eq!(frame.metadata.channels, 2);
    assert_eq!(frame.data, vec![10, 20, 30, 40]);
}

#[test]
#[ignore = "requires a built player-decoder-videotoolbox shared library artifact"]
fn dynamic_loader_opens_real_videotoolbox_decoder_shared_library() {
    let plugin_path = resolve_decoder_videotoolbox_plugin_path().unwrap_or_else(|error| {
        panic!("failed to resolve VideoToolbox decoder plugin path: {error}")
    });

    let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load VideoToolbox decoder shared library `{}`: {error}",
            plugin_path.display()
        )
    });

    assert_eq!(plugin.plugin_name(), "player-decoder-videotoolbox");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("player-decoder-videotoolbox should export a native decoder factory");
    let capabilities = factory.capabilities();
    assert!(capabilities.supports_codec("H264", DecoderMediaKind::Video));
    assert!(capabilities.supports_codec("HEVC", DecoderMediaKind::Video));
    assert!(capabilities.supports_hardware_decode);
    assert!(capabilities.supports_gpu_handles);

    let session = factory
        .open_native_session(&DecoderSessionConfig {
            codec: "H264".to_owned(),
            media_kind: DecoderMediaKind::Video,
            width: Some(1920),
            height: Some(1080),
            prefer_hardware: true,
            ..DecoderSessionConfig::default()
        })
        .expect("VideoToolbox plugin should open a lazy native session");
    assert_eq!(
        session.session_info().selected_hardware_backend.as_deref(),
        Some("VideoToolbox")
    );
}

#[test]
#[ignore = "requires a built player-decoder-d3d11 shared library artifact"]
fn dynamic_loader_opens_real_d3d11_decoder_shared_library() {
    let plugin_path = resolve_decoder_d3d11_plugin_path()
        .unwrap_or_else(|error| panic!("failed to resolve D3D11 decoder plugin path: {error}"));

    let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load D3D11 decoder shared library `{}`: {error}",
            plugin_path.display()
        )
    });

    assert_eq!(plugin.plugin_name(), "player-decoder-d3d11");
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("player-decoder-d3d11 should export a native decoder factory");
    let capabilities = factory.capabilities();
    assert!(capabilities.supports_codec("H264", DecoderMediaKind::Video));
    assert!(capabilities.supports_hardware_decode);
    assert!(capabilities.supports_gpu_handles);

    let requirements = factory.native_requirements();
    assert!(requirements.requires_native_device_context);
    assert!(
        requirements
            .required_device_context_kinds
            .contains(&DecoderNativeDeviceContextKind::D3D11Device)
    );
    assert!(
        requirements
            .output_handle_kinds
            .contains(&DecoderNativeHandleKind::D3D11Texture2D)
    );
}
