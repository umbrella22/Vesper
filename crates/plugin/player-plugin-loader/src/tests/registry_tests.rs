use super::*;

#[test]
fn descriptor_rejects_unknown_plugin_kind_before_reading_other_fields() {
    let descriptor = VesperPluginDescriptor {
        abi_version: u32::MAX,
        plugin_kind: 7,
        plugin_name: std::ptr::without_provenance(1),
        api: std::ptr::without_provenance(1),
    };

    let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect_err("unknown plugin kind must be rejected");

    assert!(matches!(
        error,
        PluginLoadError::UnsupportedPluginKind { raw: 7 }
    ));
}

#[test]
fn plugin_diagnostic_status_wire_names_match_runtime_contract() {
    assert_eq!(PluginDiagnosticStatus::Loaded.wire_name(), "loaded");
    assert_eq!(PluginDiagnosticStatus::LoadFailed.wire_name(), "loadFailed");
    assert_eq!(
        PluginDiagnosticStatus::UnsupportedKind.wire_name(),
        "unsupportedKind"
    );
    assert_eq!(
        PluginDiagnosticStatus::DecoderSupported.wire_name(),
        "decoderSupported"
    );
    assert_eq!(
        PluginDiagnosticStatus::DecoderUnsupported.wire_name(),
        "decoderUnsupported"
    );
    assert_eq!(
        PluginDiagnosticStatus::FrameProcessorSupported.wire_name(),
        "frameProcessorSupported"
    );
    assert_eq!(
        PluginDiagnosticStatus::FrameProcessorUnsupported.wire_name(),
        "frameProcessorUnsupported"
    );
    assert_eq!(
        PluginDiagnosticStatus::SourceNormalizerSupported.wire_name(),
        "sourceNormalizerSupported"
    );
    assert_eq!(
        PluginDiagnosticStatus::SourceNormalizerUnsupported.wire_name(),
        "sourceNormalizerUnsupported"
    );
}

#[test]
fn plugin_registry_reports_missing_decoder_path() {
    let registry = PluginRegistry::inspect_decoder_support(
        [PathBuf::from("/tmp/missing-vesper-decoder-plugin")],
        DecoderPluginMatchRequest::video("fixture-video"),
    );

    let records = registry.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, PluginDiagnosticStatus::LoadFailed);
    assert!(
        records[0]
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("failed to open plugin library")
    );
}

#[test]
fn plugin_registry_reports_non_decoder_plugin() {
    let api = fixture_processor_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::PostDownloadProcessor as u32,
        plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperPostDownloadProcessorApi).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
    let record = PluginDiagnosticRecord::from_loaded_plugin(
        PathBuf::from("fixture-processor"),
        &plugin,
        Some(&DecoderPluginMatchRequest::video("fixture-video")),
    );

    assert_eq!(record.status, PluginDiagnosticStatus::UnsupportedKind);
    assert_eq!(record.plugin_name.as_deref(), Some("fixture-processor"));
    assert!(
        record
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("not a decoder plugin")
    );
}

#[test]
fn plugin_registry_reports_decoder_codec_match() {
    let api = fixture_native_decoder_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::Decoder as u32,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV5).cast(),
    };
    let plugin =
        LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load decoder plugin");
    let record = PluginDiagnosticRecord::from_loaded_plugin(
        PathBuf::from("fixture-decoder"),
        &plugin,
        Some(&DecoderPluginMatchRequest::video("fixture-video")),
    );

    assert_eq!(record.status, PluginDiagnosticStatus::DecoderSupported);
    assert_eq!(record.plugin_name.as_deref(), Some("fixture-decoder"));
    let Some(PluginCapabilitySummary::Decoder(capabilities)) = record.capability_summary.as_ref()
    else {
        panic!("expected decoder capabilities");
    };
    assert!(
        capabilities
            .codecs
            .iter()
            .any(|codec| codec == "Video:fixture-video")
    );
    assert!(
        capabilities
            .typed_codecs
            .contains(&DecoderPluginCodecSummary {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
            })
    );
}

#[test]
fn plugin_registry_reports_decoder_codec_mismatch() {
    let api = fixture_native_decoder_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::Decoder as u32,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV5).cast(),
    };
    let plugin =
        LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load decoder plugin");
    let record = PluginDiagnosticRecord::from_loaded_plugin(
        PathBuf::from("fixture-decoder"),
        &plugin,
        Some(&DecoderPluginMatchRequest::video("unknown-video")),
    );

    assert_eq!(record.status, PluginDiagnosticStatus::DecoderUnsupported);
    assert!(
        record
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("does not advertise")
    );
}

#[test]
fn plugin_registry_report_counts_and_best_decoder_are_stable() {
    let api = fixture_native_decoder_api();
    let decoder_descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::Decoder as u32,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV5).cast(),
    };
    let decoder =
        LoadedDynamicPlugin::from_descriptor(None, &decoder_descriptor).expect("load decoder");
    let processor_api = fixture_processor_api();
    let processor_descriptor = VesperPluginDescriptor {
        abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::PostDownloadProcessor as u32,
        plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&processor_api as *const VesperPostDownloadProcessorApi).cast(),
    };
    let processor =
        LoadedDynamicPlugin::from_descriptor(None, &processor_descriptor).expect("load processor");

    let request = DecoderPluginMatchRequest::video("fixture-video");
    let registry = PluginRegistry::from_records(vec![
        PluginDiagnosticRecord::from_loaded_plugin(
            PathBuf::from("fixture-decoder-supported"),
            &decoder,
            Some(&request),
        ),
        PluginDiagnosticRecord::from_loaded_plugin(
            PathBuf::from("fixture-decoder-unsupported"),
            &decoder,
            Some(&DecoderPluginMatchRequest::video("missing-video")),
        ),
        PluginDiagnosticRecord::from_loaded_plugin(
            PathBuf::from("fixture-processor"),
            &processor,
            Some(&request),
        ),
        PluginDiagnosticRecord::load_failed(
            PathBuf::from("missing-plugin"),
            PluginLoadError::NullDescriptor,
        ),
    ]);
    let report = registry.report();

    assert!(registry.supports_decoder(&request));
    assert_eq!(
        registry
            .best_decoder_for(&request)
            .and_then(|record| record.plugin_name.as_deref()),
        Some("fixture-decoder")
    );
    assert_eq!(report.total, 4);
    assert_eq!(report.loaded, 3);
    assert_eq!(report.failed, 1);
    assert_eq!(report.decoder_supported, 1);
    assert_eq!(report.decoder_unsupported, 1);
    assert_eq!(report.unsupported_kind, 1);
    assert_eq!(
        report.best_supported_decoder_name.as_deref(),
        Some("fixture-decoder")
    );
    assert_eq!(report.diagnostic_notes.len(), 3);
    assert!(
        report
            .diagnostic_notes
            .iter()
            .any(|note| note == "fixture-decoder does not advertise Video missing-video support")
    );
}

#[test]
fn plugin_registry_prefers_native_decoder_candidates_when_requested() {
    let native_api = fixture_native_decoder_api();
    let native_descriptor = VesperPluginDescriptor {
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::Decoder as u32,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&native_api as *const VesperDecoderPluginApiV5).cast(),
    };
    let native_decoder = LoadedDynamicPlugin::from_descriptor(None, &native_descriptor)
        .expect("load native decoder");
    let request = DecoderPluginMatchRequest::video("fixture-video");
    let registry = PluginRegistry::from_records(vec![PluginDiagnosticRecord::from_loaded_plugin(
        PathBuf::from("fixture-native-decoder"),
        &native_decoder,
        Some(&request),
    )]);

    assert!(registry.supports_decoder(&request));
    assert!(registry.supports_native_decoder(&request));
    let native_record = registry
        .best_native_decoder_for(&request)
        .expect("native decoder should be selected");
    assert_eq!(native_record.path, PathBuf::from("fixture-native-decoder"));
    assert!(matches!(
        native_record.capability_summary.as_ref(),
        Some(PluginCapabilitySummary::Decoder(capabilities))
            if capabilities.supports_native_frame_output
    ));
}

#[test]
fn plugin_registry_selects_only_pcm_capable_audio_decoders() {
    let packet_only = DecoderCapabilities {
        codecs: vec![DecoderCodecCapability {
            codec: "aac".to_owned(),
            media_kind: DecoderMediaKind::Audio,
            profiles: Vec::new(),
            output_formats: Vec::new(),
        }],
        supports_audio_frames: false,
        ..DecoderCapabilities::default()
    };
    let packet_only_record = PluginDiagnosticRecord {
        path: PathBuf::from("packet-only-audio-decoder"),
        status: PluginDiagnosticStatus::DecoderSupported,
        plugin_name: Some("packet-only-audio-decoder".to_owned()),
        plugin_kind: Some(VesperPluginKind::Decoder),
        capability_summary: Some(PluginCapabilitySummary::Decoder(
            DecoderPluginCapabilitySummary::from(&packet_only),
        )),
        message: Some("packet-only-audio-decoder advertises Audio aac support".to_owned()),
    };
    let pcm = DecoderCapabilities {
        codecs: vec![DecoderCodecCapability {
            codec: "aac".to_owned(),
            media_kind: DecoderMediaKind::Audio,
            profiles: Vec::new(),
            output_formats: vec![DecoderFrameFormat::F32],
        }],
        supports_audio_frames: true,
        supports_pcm_frames: true,
        ..DecoderCapabilities::default()
    };
    let pcm_record = PluginDiagnosticRecord {
        path: PathBuf::from("pcm-audio-decoder"),
        status: PluginDiagnosticStatus::DecoderSupported,
        plugin_name: Some("pcm-audio-decoder".to_owned()),
        plugin_kind: Some(VesperPluginKind::Decoder),
        capability_summary: Some(PluginCapabilitySummary::Decoder(
            DecoderPluginCapabilitySummary::from(&pcm),
        )),
        message: Some("pcm-audio-decoder advertises Audio aac support".to_owned()),
    };
    let registry = PluginRegistry::from_records(vec![packet_only_record, pcm_record]);
    let request = DecoderPluginMatchRequest::audio("AAC");

    assert!(registry.supports_decoder(&request));
    assert!(registry.supports_pcm_audio_decoder(&request));
    assert_eq!(
        registry
            .best_decoder_for(&request)
            .and_then(|record| record.plugin_name.as_deref()),
        Some("packet-only-audio-decoder")
    );
    assert_eq!(
        registry
            .best_pcm_audio_decoder_for(&request)
            .and_then(|record| record.plugin_name.as_deref()),
        Some("pcm-audio-decoder")
    );
    assert!(
        !registry.supports_pcm_audio_decoder(&DecoderPluginMatchRequest::video("aac")),
        "audio PCM selection must not match video requests"
    );
}

#[test]
fn decoder_capability_summary_distinguishes_audio_packets_from_pcm_frames() {
    let packet_only = DecoderCapabilities {
        codecs: vec![DecoderCodecCapability {
            codec: "aac".to_owned(),
            media_kind: DecoderMediaKind::Audio,
            profiles: Vec::new(),
            output_formats: Vec::new(),
        }],
        supports_audio_frames: false,
        ..DecoderCapabilities::default()
    };
    let packet_summary = DecoderPluginCapabilitySummary::from(&packet_only);

    assert!(packet_summary.supports_audio_packets);
    assert!(!packet_summary.supports_audio_frames);
    assert!(!packet_summary.supports_pcm_frames);

    let pcm = DecoderCapabilities {
        codecs: vec![DecoderCodecCapability {
            codec: "aac".to_owned(),
            media_kind: DecoderMediaKind::Audio,
            profiles: Vec::new(),
            output_formats: vec![DecoderFrameFormat::F32],
        }],
        supports_audio_frames: true,
        supports_pcm_frames: true,
        ..DecoderCapabilities::default()
    };
    let pcm_summary = DecoderPluginCapabilitySummary::from(&pcm);

    assert!(pcm_summary.supports_audio_packets);
    assert!(pcm_summary.supports_audio_frames);
    assert!(pcm_summary.supports_pcm_frames);

    let video = DecoderCapabilities {
        codecs: vec![DecoderCodecCapability {
            codec: "h264".to_owned(),
            media_kind: DecoderMediaKind::Video,
            profiles: Vec::new(),
            output_formats: vec![DecoderFrameFormat::Nv12],
        }],
        supports_audio_frames: true,
        supports_pcm_frames: false,
        ..DecoderCapabilities::default()
    };
    let video_summary = DecoderPluginCapabilitySummary::from(&video);

    assert!(!video_summary.supports_audio_packets);
    assert!(video_summary.supports_audio_frames);
    assert!(!video_summary.supports_pcm_frames);
}
