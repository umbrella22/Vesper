use super::*;

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
        plugin_kind: VesperPluginKind::PostDownloadProcessor,
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
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
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
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
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
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperDecoderPluginApiV2).cast(),
    };
    let decoder =
        LoadedDynamicPlugin::from_descriptor(None, &decoder_descriptor).expect("load decoder");
    let processor_api = fixture_processor_api();
    let processor_descriptor = VesperPluginDescriptor {
        abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::PostDownloadProcessor,
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
        abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::Decoder,
        plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
        api: (&native_api as *const VesperDecoderPluginApiV2).cast(),
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
