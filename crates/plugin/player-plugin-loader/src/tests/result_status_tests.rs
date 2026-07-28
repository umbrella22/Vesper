use super::*;

const UNKNOWN_RESULT_STATUS: u32 = 2;

fn recorder_context(recorder: &FixtureFreeBytesRecorder) -> *mut c_void {
    (recorder as *const FixtureFreeBytesRecorder)
        .cast_mut()
        .cast::<c_void>()
}

fn assert_payload_reclaimed(
    recorder: &FixtureFreeBytesRecorder,
    baseline: usize,
    expected_len: usize,
) {
    let reclaimed = recorder
        .freed_lens()
        .into_iter()
        .skip(baseline)
        .collect::<Vec<_>>();
    assert_eq!(
        reclaimed
            .iter()
            .filter(|&&reclaimed_len| reclaimed_len == expected_len)
            .count(),
        1,
        "expected a {expected_len}-byte plugin payload to be reclaimed exactly once; observed {reclaimed:?}"
    );
}

fn unknown_payload(len: usize) -> VesperPluginBytes {
    VesperPluginBytes::from_vec(vec![0xa5; len])
}

fn load_post_download_processor(api: &VesperPostDownloadProcessorApi) -> LoadedDynamicPlugin {
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::PostDownloadProcessor as u32,
        plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (api as *const VesperPostDownloadProcessorApi).cast(),
    };
    LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load post-download fixture")
}

fn load_decoder(api: &VesperDecoderPluginApiV5) -> LoadedDynamicPlugin {
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::Decoder as u32,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (api as *const VesperDecoderPluginApiV5).cast(),
    };
    LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load decoder fixture")
}

fn load_frame_processor(api: &VesperFrameProcessorPluginApiV1) -> LoadedDynamicPlugin {
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::FrameProcessor as u32,
        plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (api as *const VesperFrameProcessorPluginApiV1).cast(),
    };
    LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load frame processor fixture")
}

fn load_source_normalizer(api: &VesperSourceNormalizerPluginApiV4) -> LoadedDynamicPlugin {
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };
    LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load source normalizer fixture")
}

fn decoder_config(codec: &str, media_kind: DecoderMediaKind) -> DecoderSessionConfig {
    DecoderSessionConfig {
        codec: codec.to_owned(),
        media_kind,
        ..DecoderSessionConfig::default()
    }
}

fn frame_processor_config() -> FrameProcessorSessionConfig {
    FrameProcessorSessionConfig {
        processor_index: 0,
        input_metadata: fixture_native_frame().metadata,
        max_in_flight_frames: Some(1),
    }
}

fn packet_session_config() -> SourceNormalizerPacketSessionConfig {
    SourceNormalizerPacketSessionConfig {
        runtime_profile: "fixture-packet".to_owned(),
        input: "file:///tmp/input.mp4".to_owned(),
        headers: Vec::new(),
        startup_timeout_ms: None,
        session_timeout_ms: None,
        preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
    }
}

fn resource_session_config() -> player_plugin::SourceNormalizerResourceSessionConfig {
    player_plugin::SourceNormalizerResourceSessionConfig {
        runtime_profile: "fixture-resource".to_owned(),
        input: "file:///tmp/input.mp4".to_owned(),
        headers: Vec::new(),
        output_root: "/tmp/vesper-source-normalizer-fixture".to_owned(),
        cache_policy: SourceNormalizerResourceCachePolicy::default(),
        preferred_route: Some(SourceNormalizerOutputRoute::Fmp4LocalStream),
        startup_timeout_ms: Some(10),
        read_idle_timeout_ms: Some(10),
    }
}

fn completed_download() -> CompletedDownloadInfo {
    CompletedDownloadInfo {
        asset_id: "unknown-status-fixture".to_owned(),
        task_id: Some("1".to_owned()),
        content_format: CompletedContentFormat::SingleFile {
            path: PathBuf::from("/tmp/input.mp4"),
        },
        metadata: DownloadMetadata::default(),
        streams: Vec::new(),
        assembly_mode: AssemblyMode::Single,
    }
}

#[test]
fn process_result_unknown_status_reclaims_payload_and_reports_abi_violation() {
    let recorder = FixtureFreeBytesRecorder::default();
    let api = VesperPostDownloadProcessorApi {
        context: recorder_context(&recorder),
        free_bytes: Some(fixture_recording_free_bytes),
        process_json: Some(unknown_status_process_json),
        ..fixture_processor_api()
    };
    let plugin = load_post_download_processor(&api);
    let processor = plugin
        .post_download_processor()
        .expect("post-download processor fixture");
    let baseline = recorder.freed_lens().len();

    let error = processor
        .process(
            &completed_download(),
            Path::new("/tmp/output.mp4"),
            &RecordingProgress::default(),
        )
        .expect_err("raw status 2 must be rejected");

    assert!(matches!(error, ProcessorError::AbiViolation(_)));
    assert!(error.to_string().contains("unknown status 2"));
    assert_payload_reclaimed(&recorder, baseline, 7);
}

#[test]
fn decoder_open_result_unknown_status_reclaims_payload_and_closes_session() {
    let _guard = decoder_native_frame_release_test_guard();
    if let Ok(mut closes) = NATIVE_DECODER_CLOSES.lock() {
        closes.clear();
    }
    if let Ok(mut releases) = NATIVE_FRAME_RELEASES.lock() {
        releases.clear();
    }
    let recorder = FixtureFreeBytesRecorder::default();
    let api = VesperDecoderPluginApiV5 {
        context: recorder_context(&recorder),
        free_bytes: Some(fixture_recording_free_bytes),
        open_session_json: Some(unknown_status_decoder_open),
        close_session: Some(fixture_decoder_recording_close_session),
        ..fixture_native_decoder_api()
    };
    let plugin = load_decoder(&api);
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("decoder factory fixture");
    let baseline = recorder.freed_lens().len();

    let error = match factory
        .open_native_session(&decoder_config("fixture-video", DecoderMediaKind::Video))
    {
        Ok(_) => panic!("raw status 2 must be rejected"),
        Err(error) => error,
    };

    assert!(matches!(error, DecoderError::AbiViolation { .. }));
    assert!(error.to_string().contains("unknown status 2"));
    assert_payload_reclaimed(&recorder, baseline, 11);
    assert_eq!(
        NATIVE_DECODER_CLOSES
            .lock()
            .map(|closes| closes.len())
            .unwrap_or_default(),
        1
    );
}

#[test]
fn decoder_native_frame_result_unknown_status_reclaims_metadata_and_handle_via_close() {
    let _guard = decoder_native_frame_release_test_guard();
    if let Ok(mut closes) = NATIVE_DECODER_CLOSES.lock() {
        closes.clear();
    }
    if let Ok(mut reclaims) = NATIVE_DECODER_CLOSE_RECLAIMS.lock() {
        reclaims.clear();
    }
    let recorder = FixtureFreeBytesRecorder::default();
    let api = VesperDecoderPluginApiV5 {
        context: recorder_context(&recorder),
        free_bytes: Some(fixture_recording_free_bytes),
        receive_native_frame: Some(unknown_status_decoder_native_frame),
        close_session: Some(fixture_decoder_recording_close_session),
        ..fixture_native_decoder_api()
    };
    let plugin = load_decoder(&api);
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("decoder factory fixture");
    let mut session = factory
        .open_native_session(&decoder_config("fixture-video", DecoderMediaKind::Video))
        .expect("open decoder fixture");
    let baseline = recorder.freed_lens().len();

    let error = session
        .receive_native_frame()
        .expect_err("raw status 2 must be rejected");

    assert!(matches!(error, DecoderError::AbiViolation { .. }));
    assert!(error.to_string().contains("unknown status 2"));
    assert_payload_reclaimed(&recorder, baseline, 13);
    assert_eq!(
        NATIVE_DECODER_CLOSE_RECLAIMS
            .lock()
            .map(|reclaims| reclaims.len())
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        NATIVE_DECODER_CLOSES
            .lock()
            .map(|closes| closes.len())
            .unwrap_or_default(),
        1
    );
}

#[test]
fn decoder_pcm_result_unknown_status_reclaims_both_payloads_and_closes_session() {
    let _guard = decoder_native_frame_release_test_guard();
    if let Ok(mut closes) = NATIVE_DECODER_CLOSES.lock() {
        closes.clear();
    }
    let recorder = FixtureFreeBytesRecorder::default();
    let api = VesperDecoderPluginApiV5 {
        context: recorder_context(&recorder),
        free_bytes: Some(fixture_recording_free_bytes),
        receive_pcm_frame: Some(unknown_status_decoder_pcm_frame),
        close_session: Some(fixture_decoder_recording_close_session),
        ..fixture_native_decoder_pcm_api()
    };
    let plugin = load_decoder(&api);
    let factory = plugin
        .native_decoder_plugin_factory()
        .expect("decoder factory fixture");
    let mut session = factory
        .open_native_session(&decoder_config("fixture-audio", DecoderMediaKind::Audio))
        .expect("open PCM decoder fixture");
    let baseline = recorder.freed_lens().len();

    let error = session
        .receive_pcm_frame()
        .expect_err("raw status 2 must be rejected");

    assert!(matches!(error, DecoderError::AbiViolation { .. }));
    assert!(error.to_string().contains("unknown status 2"));
    assert_payload_reclaimed(&recorder, baseline, 17);
    assert_payload_reclaimed(&recorder, baseline, 19);
    assert_eq!(
        NATIVE_DECODER_CLOSES
            .lock()
            .map(|closes| closes.len())
            .unwrap_or_default(),
        1
    );
}

#[test]
fn frame_processor_open_result_unknown_status_reclaims_payload_and_closes_session() {
    let _guard = frame_processor_test_guard();
    if let Ok(mut closes) = FRAME_PROCESSOR_CLOSES.lock() {
        *closes = 0;
    }
    if let Ok(mut releases) = FRAME_PROCESSOR_RELEASES.lock() {
        releases.clear();
    }
    let recorder = FixtureFreeBytesRecorder::default();
    let api = VesperFrameProcessorPluginApiV1 {
        context: recorder_context(&recorder),
        free_bytes: Some(fixture_recording_free_bytes),
        open_session_json: Some(unknown_status_frame_processor_open),
        close_session: Some(fixture_frame_processor_recording_close_session),
        ..fixture_frame_processor_api()
    };
    let plugin = load_frame_processor(&api);
    let factory = plugin
        .frame_processor_plugin_factory()
        .expect("frame processor factory fixture");
    let baseline = recorder.freed_lens().len();

    let error = match factory.open_session(&frame_processor_config()) {
        Ok(_) => panic!("raw status 2 must be rejected"),
        Err(error) => error,
    };

    assert!(matches!(error, FrameProcessorError::AbiViolation { .. }));
    assert!(error.to_string().contains("unknown status 2"));
    assert_payload_reclaimed(&recorder, baseline, 23);
    assert_eq!(
        FRAME_PROCESSOR_CLOSES
            .lock()
            .map(|closes| *closes)
            .unwrap_or_default(),
        1
    );
}

#[test]
fn frame_processor_receive_result_unknown_status_reclaims_metadata_and_handle_via_close() {
    let _guard = frame_processor_test_guard();
    if let Ok(mut closes) = FRAME_PROCESSOR_CLOSES.lock() {
        *closes = 0;
    }
    if let Ok(mut reclaims) = FRAME_PROCESSOR_CLOSE_RECLAIMS.lock() {
        reclaims.clear();
    }
    let recorder = FixtureFreeBytesRecorder::default();
    let api = VesperFrameProcessorPluginApiV1 {
        context: recorder_context(&recorder),
        free_bytes: Some(fixture_recording_free_bytes),
        receive_frame: Some(unknown_status_frame_processor_receive),
        close_session: Some(fixture_frame_processor_recording_close_session),
        ..fixture_frame_processor_api()
    };
    let plugin = load_frame_processor(&api);
    let factory = plugin
        .frame_processor_plugin_factory()
        .expect("frame processor factory fixture");
    let mut session = factory
        .open_session(&frame_processor_config())
        .expect("open frame processor fixture");
    let baseline = recorder.freed_lens().len();

    let error = session
        .receive_frame()
        .expect_err("raw status 2 must be rejected");

    assert!(matches!(error, FrameProcessorError::AbiViolation { .. }));
    assert!(error.to_string().contains("unknown status 2"));
    assert_payload_reclaimed(&recorder, baseline, 29);
    assert_eq!(
        FRAME_PROCESSOR_CLOSE_RECLAIMS
            .lock()
            .map(|releases| releases.len())
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        FRAME_PROCESSOR_CLOSES
            .lock()
            .map(|closes| *closes)
            .unwrap_or_default(),
        1
    );
}

#[test]
fn source_normalizer_packet_open_unknown_status_reclaims_payload_and_closes_session() {
    let _guard = source_normalizer_packet_test_guard();
    reset_source_normalizer_session_closes();
    let recorder = FixtureFreeBytesRecorder::default();
    let api = VesperSourceNormalizerPluginApiV4 {
        context: recorder_context(&recorder),
        free_bytes: Some(fixture_recording_free_bytes),
        open_packet_session_json: Some(unknown_status_packet_session_open),
        ..fixture_source_normalizer_packet_api()
    };
    let plugin = load_source_normalizer(&api);
    let factory = plugin
        .source_normalizer_packet_plugin_factory()
        .expect("packet source normalizer factory fixture");
    let baseline = recorder.freed_lens().len();

    let error = match factory.open_packet_session(&packet_session_config()) {
        Ok(_) => panic!("raw status 2 must be rejected"),
        Err(error) => error,
    };

    assert!(matches!(error, SourceNormalizerError::AbiViolation { .. }));
    assert!(error.to_string().contains("unknown status 2"));
    assert_payload_reclaimed(&recorder, baseline, 31);
    assert_eq!(source_normalizer_packet_closes(), 1);
}

#[test]
fn source_normalizer_resource_open_unknown_status_reclaims_payload_and_closes_session() {
    let _guard = source_normalizer_packet_test_guard();
    reset_source_normalizer_session_closes();
    let recorder = FixtureFreeBytesRecorder::default();
    let api = VesperSourceNormalizerPluginApiV4 {
        context: recorder_context(&recorder),
        free_bytes: Some(fixture_recording_free_bytes),
        open_resource_session_json: Some(unknown_status_resource_session_open),
        ..fixture_source_normalizer_dual_api()
    };
    let plugin = load_source_normalizer(&api);
    let factory = plugin
        .source_normalizer_resource_plugin_factory()
        .expect("resource source normalizer factory fixture");
    let baseline = recorder.freed_lens().len();

    let error = match factory.open_resource_session(&resource_session_config()) {
        Ok(_) => panic!("raw status 2 must be rejected"),
        Err(error) => error,
    };

    assert!(matches!(error, SourceNormalizerError::AbiViolation { .. }));
    assert!(error.to_string().contains("unknown status 2"));
    assert_payload_reclaimed(&recorder, baseline, 37);
    assert_eq!(source_normalizer_resource_closes(), 1);
}

#[test]
fn source_normalizer_read_unknown_status_reclaims_metadata_and_packet_handle() {
    let _guard = source_normalizer_packet_test_guard();
    reset_source_normalizer_packet_releases();
    reset_source_normalizer_session_closes();
    let recorder = FixtureFreeBytesRecorder::default();
    let api = VesperSourceNormalizerPluginApiV4 {
        context: recorder_context(&recorder),
        free_bytes: Some(fixture_recording_free_bytes),
        read_packet: Some(unknown_status_packet_read),
        ..fixture_source_normalizer_packet_api()
    };
    let plugin = load_source_normalizer(&api);
    let factory = plugin
        .source_normalizer_packet_plugin_factory()
        .expect("packet source normalizer factory fixture");
    let mut session = factory
        .open_packet_session(&packet_session_config())
        .expect("open packet source normalizer fixture");
    let baseline = recorder.freed_lens().len();

    let error = session
        .read_packet()
        .expect_err("raw status 2 must be rejected");

    assert!(matches!(error, SourceNormalizerError::AbiViolation { .. }));
    assert!(error.to_string().contains("unknown status 2"));
    assert_payload_reclaimed(&recorder, baseline, 41);
    assert_eq!(source_normalizer_packet_releases(), vec![0x53]);
    session.close().expect("close poisoned packet session");
    assert_eq!(source_normalizer_packet_closes(), 1);
}

unsafe extern "C" fn unknown_status_process_json(
    _context: *mut c_void,
    _input_json: *const u8,
    _input_json_len: usize,
    _output_path: *const c_char,
    _progress: player_plugin::VesperPluginProgressCallbacks,
) -> VesperPluginProcessResult {
    VesperPluginProcessResult {
        status: UNKNOWN_RESULT_STATUS,
        payload: unknown_payload(7),
    }
}

unsafe extern "C" fn unknown_status_decoder_open(
    _context: *mut c_void,
    _config_json: *const u8,
    _config_json_len: usize,
) -> VesperDecoderOpenSessionResult {
    VesperDecoderOpenSessionResult {
        status: UNKNOWN_RESULT_STATUS,
        session: Box::into_raw(Box::new(FixtureDecoderSession::default())).cast::<c_void>(),
        payload: unknown_payload(11),
    }
}

unsafe extern "C" fn unknown_status_decoder_native_frame(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperDecoderReceiveNativeFrameResult {
    // SAFETY: this fixture receives the session allocated by the matching
    // decoder open callback and uses it only for the synchronous ABI call.
    let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
        return VesperDecoderReceiveNativeFrameResult {
            status: UNKNOWN_RESULT_STATUS,
            metadata: unknown_payload(13),
            handle: 0,
        };
    };
    let handle = Box::into_raw(Box::new(vec![1, 2, 3, 4])) as usize;
    session.pending_unknown_frame_handle = Some(handle);
    VesperDecoderReceiveNativeFrameResult {
        status: UNKNOWN_RESULT_STATUS,
        metadata: unknown_payload(13),
        handle,
    }
}

unsafe extern "C" fn unknown_status_decoder_pcm_frame(
    _context: *mut c_void,
    _session: *mut c_void,
) -> VesperDecoderReceivePcmFrameResult {
    VesperDecoderReceivePcmFrameResult {
        status: UNKNOWN_RESULT_STATUS,
        metadata: unknown_payload(17),
        data: unknown_payload(19),
    }
}

unsafe extern "C" fn unknown_status_frame_processor_open(
    _context: *mut c_void,
    _config_json: *const u8,
    _config_json_len: usize,
) -> VesperFrameProcessorOpenSessionResult {
    VesperFrameProcessorOpenSessionResult {
        status: UNKNOWN_RESULT_STATUS,
        session: Box::into_raw(Box::new(FixtureFrameProcessorSession::default())).cast::<c_void>(),
        payload: unknown_payload(23),
    }
}

unsafe extern "C" fn unknown_status_frame_processor_receive(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperFrameProcessorReceiveFrameResult {
    // SAFETY: this fixture receives the session allocated by the matching
    // frame-processor open callback and uses it only for this ABI call.
    let Some(session) = (unsafe { session.cast::<FixtureFrameProcessorSession>().as_mut() }) else {
        return VesperFrameProcessorReceiveFrameResult {
            status: UNKNOWN_RESULT_STATUS,
            metadata: unknown_payload(29),
            handle: 0,
        };
    };
    let handle = Box::into_raw(Box::new(vec![1, 2, 3, 4])) as usize;
    session.pending_output = Some(NativeFrame {
        metadata: fixture_native_frame().metadata,
        handle,
    });
    VesperFrameProcessorReceiveFrameResult {
        status: UNKNOWN_RESULT_STATUS,
        metadata: unknown_payload(29),
        handle,
    }
}

unsafe extern "C" fn unknown_status_packet_session_open(
    _context: *mut c_void,
    _config_json: *const u8,
    _config_json_len: usize,
) -> VesperSourceNormalizerOpenPacketSessionResult {
    let session = FixtureSourceNormalizerPacketSession {
        emitted_packet: false,
        leased_packet: None,
        last_seek: None,
    };
    VesperSourceNormalizerOpenPacketSessionResult {
        status: UNKNOWN_RESULT_STATUS,
        session: Box::into_raw(Box::new(session)).cast::<c_void>(),
        payload: unknown_payload(31),
    }
}

unsafe extern "C" fn unknown_status_resource_session_open(
    _context: *mut c_void,
    _config_json: *const u8,
    _config_json_len: usize,
) -> VesperSourceNormalizerOpenResourceSessionResult {
    VesperSourceNormalizerOpenResourceSessionResult {
        status: UNKNOWN_RESULT_STATUS,
        session: Box::into_raw(Box::new(FixtureSourceNormalizerResourceSession)).cast::<c_void>(),
        payload: unknown_payload(37),
    }
}

unsafe extern "C" fn unknown_status_packet_read(
    _context: *mut c_void,
    session: *mut c_void,
) -> VesperSourceNormalizerReadPacketResult {
    // SAFETY: this fixture receives the session allocated by the matching
    // packet-session open callback and uses it only for this ABI call.
    let Some(session) = (unsafe {
        session
            .cast::<FixtureSourceNormalizerPacketSession>()
            .as_mut()
    }) else {
        return VesperSourceNormalizerReadPacketResult {
            status: UNKNOWN_RESULT_STATUS,
            metadata: unknown_payload(41),
            data: std::ptr::null(),
            data_len: 0,
            packet_handle: 0,
        };
    };
    let handle = 0x53;
    session.leased_packet = Some(FixtureSourceNormalizerPacketLease {
        handle,
        data: vec![0, 0, 1, 11],
    });
    let Some(packet) = session.leased_packet.as_ref() else {
        return VesperSourceNormalizerReadPacketResult {
            status: UNKNOWN_RESULT_STATUS,
            metadata: unknown_payload(41),
            data: std::ptr::null(),
            data_len: 0,
            packet_handle: 0,
        };
    };
    VesperSourceNormalizerReadPacketResult {
        status: UNKNOWN_RESULT_STATUS,
        metadata: unknown_payload(41),
        data: packet.data.as_ptr(),
        data_len: packet.data.len(),
        packet_handle: packet.handle,
    }
}
