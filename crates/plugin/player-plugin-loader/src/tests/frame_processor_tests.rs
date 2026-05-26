use super::*;

#[test]
fn dynamic_frame_processor_plugin_adapter_round_trips_native_frame() {
    let _guard = frame_processor_test_guard();
    if let Ok(mut releases) = FRAME_PROCESSOR_RELEASES.lock() {
        releases.clear();
    }
    let api = fixture_frame_processor_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
        plugin_kind: VesperPluginKind::FrameProcessor,
        plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperFrameProcessorPluginApiV1).cast(),
    };

    let plugin =
        LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load frame processor");
    assert!(plugin.native_decoder_plugin_factory().is_none());
    let factory = plugin
        .frame_processor_plugin_factory()
        .expect("frame processor factory should be available");
    assert_eq!(factory.name(), "test-frame-processor");
    assert!(factory.capabilities().supports_video_frames);

    let input = fixture_native_frame();
    let mut session = factory
        .open_session(&FrameProcessorSessionConfig {
            processor_index: 3,
            input_metadata: input.metadata.clone(),
            max_in_flight_frames: Some(1),
        })
        .expect("open frame processor session");
    assert_eq!(
        session.session_info().processor_name.as_deref(),
        Some("test-frame-processor")
    );

    let submit = session
        .submit_frame(
            &input,
            &FrameProcessorSubmitFrame {
                metadata: input.metadata.clone(),
                present_deadline_us: Some(100_000),
            },
        )
        .expect("submit frame");
    assert_eq!(submit.status, FrameProcessorSubmitStatus::Accepted);

    let output = match session.receive_frame().expect("receive output") {
        FrameProcessorReceiveOutput::Frame(output) => output,
        other => panic!("expected processed frame, got {other:?}"),
    };
    assert_ne!(output.frame.handle, 0);
    assert_eq!(output.frame.metadata.pts_us, input.metadata.pts_us);
    assert_eq!(output.source_frame_id, input.metadata.frame_id);
    let output_handle = output.frame.handle;
    session
        .release_frame(output.frame)
        .expect("release processor output");
    assert_eq!(
        session.receive_frame().expect("pending"),
        FrameProcessorReceiveOutput::Pending
    );
    session.close().expect("close frame processor");
    assert!(
        FRAME_PROCESSOR_RELEASES
            .lock()
            .map(|releases| releases.contains(&output_handle))
            .unwrap_or(false)
    );
}

#[test]
fn dynamic_frame_processor_plugin_close_releases_unreturned_outputs() {
    let _guard = frame_processor_test_guard();
    if let Ok(mut releases) = FRAME_PROCESSOR_RELEASES.lock() {
        releases.clear();
    }
    let api = fixture_frame_processor_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
        plugin_kind: VesperPluginKind::FrameProcessor,
        plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperFrameProcessorPluginApiV1).cast(),
    };
    let plugin =
        LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load frame processor");
    let factory = plugin
        .frame_processor_plugin_factory()
        .expect("frame processor factory should be available");
    let input = fixture_native_frame();
    let mut session = factory
        .open_session(&FrameProcessorSessionConfig {
            processor_index: 0,
            input_metadata: input.metadata.clone(),
            max_in_flight_frames: Some(1),
        })
        .expect("open frame processor session");
    session
        .submit_frame(
            &input,
            &FrameProcessorSubmitFrame::new(input.metadata.clone()),
        )
        .expect("submit frame");
    let output = match session.receive_frame().expect("receive output") {
        FrameProcessorReceiveOutput::Frame(output) => output,
        other => panic!("expected processed frame, got {other:?}"),
    };
    let handle = output.frame.handle;

    session
        .close()
        .expect("close should release outstanding output");

    assert!(
        FRAME_PROCESSOR_RELEASES
            .lock()
            .map(|releases| releases.contains(&handle))
            .unwrap_or(false)
    );
}

#[test]
fn dynamic_frame_processor_plugin_does_not_release_passthrough_outputs() {
    let _guard = frame_processor_test_guard();
    if let Ok(mut releases) = FRAME_PROCESSOR_RELEASES.lock() {
        releases.clear();
    }
    let api = fixture_frame_processor_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
        plugin_kind: VesperPluginKind::FrameProcessor,
        plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperFrameProcessorPluginApiV1).cast(),
    };
    let plugin =
        LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load frame processor");
    let factory = plugin
        .frame_processor_plugin_factory()
        .expect("frame processor factory should be available");
    let mut input = fixture_native_frame();
    input.metadata.release_tracking = Some(NativeFrameReleaseTracking {
        frame_id: input.metadata.frame_id,
        requires_release: false,
    });
    let mut session = factory
        .open_session(&FrameProcessorSessionConfig {
            processor_index: 0,
            input_metadata: input.metadata.clone(),
            max_in_flight_frames: Some(1),
        })
        .expect("open frame processor session");
    session
        .submit_frame(
            &input,
            &FrameProcessorSubmitFrame::new(input.metadata.clone()),
        )
        .expect("submit frame");
    let output = match session.receive_frame().expect("receive output") {
        FrameProcessorReceiveOutput::Frame(output) => output,
        other => panic!("expected processed frame, got {other:?}"),
    };

    assert_eq!(
        output
            .frame
            .metadata
            .release_tracking
            .as_ref()
            .map(|tracking| tracking.requires_release),
        Some(false)
    );
    assert!(
        session.release_frame(output.frame).is_err(),
        "loader should not track passthrough output for processor release"
    );
    session
        .close()
        .expect("close should not release passthrough output");
    assert!(
        FRAME_PROCESSOR_RELEASES
            .lock()
            .map(|releases| releases.is_empty())
            .unwrap_or(false)
    );
}

#[test]
fn dynamic_frame_processor_plugin_rejects_missing_submit_entry() {
    let api = VesperFrameProcessorPluginApiV1 {
        submit_frame_json: None,
        ..fixture_frame_processor_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
        plugin_kind: VesperPluginKind::FrameProcessor,
        plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperFrameProcessorPluginApiV1).cast(),
    };

    let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect_err("frame processor ABI requires submit_frame_json");

    assert!(matches!(
        error,
        PluginLoadError::MissingField {
            field: "frame_processor_plugin_api_v1.submit_frame_json"
        }
    ));
}

#[test]
fn dynamic_frame_processor_plugin_rejects_old_abi_revision() {
    let api = fixture_frame_processor_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_PLUGIN_ABI_VERSION_V2,
        plugin_kind: VesperPluginKind::FrameProcessor,
        plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperFrameProcessorPluginApiV1).cast(),
    };

    let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect_err("wrong frame processor ABI revision should be rejected");

    assert!(matches!(
        error,
        PluginLoadError::AbiVersionMismatch {
            expected: 1,
            actual: 2
        }
    ));
}

#[test]
fn plugin_registry_reports_frame_processor_support() {
    let api = fixture_frame_processor_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
        plugin_kind: VesperPluginKind::FrameProcessor,
        plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperFrameProcessorPluginApiV1).cast(),
    };
    let plugin =
        LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load frame processor");
    let record = PluginDiagnosticRecord::from_loaded_frame_processor_plugin(
        PathBuf::from("test-frame-processor"),
        &plugin,
    );

    assert_eq!(
        record.status,
        PluginDiagnosticStatus::FrameProcessorSupported
    );
    assert_eq!(record.plugin_name.as_deref(), Some("test-frame-processor"));
    assert!(matches!(
        record.capability_summary,
        Some(PluginCapabilitySummary::FrameProcessor(_))
    ));

    let registry = PluginRegistry::from_records(vec![record]);
    let report = registry.report();
    assert_eq!(report.frame_processor_supported, 1);
    assert_eq!(
        report.best_supported_frame_processor_name.as_deref(),
        Some("test-frame-processor")
    );
    assert_eq!(
        registry.frame_processor_supported_plugin_names(),
        vec!["test-frame-processor"]
    );
}

#[test]
#[ignore = "requires a built player-frame-processor-diagnostic shared library artifact"]
fn dynamic_loader_opens_real_frame_processor_diagnostic_shared_library() {
    let plugin_path = resolve_frame_processor_diagnostic_plugin_path().unwrap_or_else(|error| {
        panic!("failed to resolve frame processor diagnostic plugin path: {error}")
    });

    let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load frame processor diagnostic shared library `{}`: {error}",
            plugin_path.display()
        )
    });

    assert_eq!(plugin.plugin_name(), "player-frame-processor-diagnostic");
    assert!(plugin.post_download_processor().is_none());
    assert!(plugin.pipeline_event_hook().is_none());
    assert!(plugin.native_decoder_plugin_factory().is_none());
    let factory = plugin
        .frame_processor_plugin_factory()
        .expect("player-frame-processor-diagnostic should export a frame processor factory");
    assert_eq!(factory.name(), "player-frame-processor-diagnostic");
    assert!(factory.capabilities().supports_video_frames);
}
