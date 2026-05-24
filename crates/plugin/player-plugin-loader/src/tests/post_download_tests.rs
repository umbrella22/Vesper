use super::*;

#[test]
fn dynamic_post_download_processor_adapter_round_trips_json() {
    let api = fixture_processor_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::PostDownloadProcessor,
        plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperPostDownloadProcessorApi).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
    let processor = plugin
        .post_download_processor()
        .expect("processor should be available");
    let progress = RecordingProgress::default();
    let output = processor
        .process(
            &CompletedDownloadInfo {
                asset_id: "asset-a".to_owned(),
                task_id: Some("1".to_owned()),
                content_format: CompletedContentFormat::SingleFile {
                    path: PathBuf::from("/tmp/input.mp4"),
                },
                metadata: DownloadMetadata::default(),
                streams: Vec::new(),
                assembly_mode: AssemblyMode::Single,
            },
            PathBuf::from("/tmp/output.mp4").as_path(),
            &progress,
        )
        .expect("process should succeed");

    assert_eq!(
        processor.capabilities(),
        ProcessorCapabilities {
            supported_input_formats: vec![ContentFormatKind::SingleFile],
            output_formats: vec![OutputFormat::Mp4],
            supports_cancellation: true,
            supports_assembly: false,
            supported_assembly_modes: Vec::new(),
        }
    );
    assert_eq!(
        output,
        ProcessorOutput::MuxedFile {
            path: PathBuf::from("/tmp/output.mp4"),
            format: OutputFormat::Mp4,
        }
    );
    assert_eq!(progress.ratios(), vec![0.5, 1.0]);
}

#[test]
fn dynamic_post_download_processor_assembly_adapter_round_trips_json() {
    let api = fixture_processor_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::PostDownloadProcessor,
        plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperPostDownloadProcessorApi).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
    let processor = plugin
        .post_download_processor()
        .expect("processor should be available");
    let progress = RecordingProgress::default();
    let output = processor
        .assemble(
            &CompletedDownloadInfo {
                asset_id: "asset-a".to_owned(),
                task_id: Some("1".to_owned()),
                content_format: CompletedContentFormat::SingleFile {
                    path: PathBuf::from("/tmp/input.mp4"),
                },
                metadata: DownloadMetadata::default(),
                streams: Vec::new(),
                assembly_mode: AssemblyMode::Single,
            },
            PathBuf::from("/tmp/assembled.mp4").as_path(),
            &progress,
        )
        .expect("assemble should succeed");

    assert_eq!(
        output,
        ProcessorOutput::MuxedFile {
            path: PathBuf::from("/tmp/assembled.mp4"),
            format: OutputFormat::Mp4,
        }
    );
    assert_eq!(progress.ratios(), vec![0.5, 1.0]);
}

#[test]
fn dynamic_post_download_processor_rejects_v2_descriptor() {
    let api = fixture_processor_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_PLUGIN_ABI_VERSION_V2,
        plugin_kind: VesperPluginKind::PostDownloadProcessor,
        plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperPostDownloadProcessorApi).cast(),
    };

    let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect_err("post-download processors require ABI v3");

    assert!(matches!(
        error,
        PluginLoadError::AbiVersionMismatch {
            expected: 3,
            actual: 2
        }
    ));
}

#[test]
fn dynamic_post_download_processor_rejects_missing_assembly_entry() {
    let api = VesperPostDownloadProcessorApi {
        assemble_json: None,
        ..fixture_processor_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::PostDownloadProcessor,
        plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperPostDownloadProcessorApi).cast(),
    };

    let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect_err("post-download ABI v3 requires assemble_json");

    assert!(matches!(
        error,
        PluginLoadError::MissingField {
            field: "post_download_processor_api.assemble_json"
        }
    ));
}

#[test]
fn dynamic_post_download_processor_reports_payload_codec_errors() {
    let api = VesperPostDownloadProcessorApi {
        process_json: Some(fixture_payload_codec_process_json),
        ..fixture_processor_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::PostDownloadProcessor,
        plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperPostDownloadProcessorApi).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
    let processor = plugin
        .post_download_processor()
        .expect("processor should be available");
    let error = processor
        .process(
            &CompletedDownloadInfo {
                asset_id: "asset-a".to_owned(),
                task_id: Some("1".to_owned()),
                content_format: CompletedContentFormat::SingleFile {
                    path: PathBuf::from("/tmp/input.mp4"),
                },
                metadata: DownloadMetadata::default(),
                streams: Vec::new(),
                assembly_mode: AssemblyMode::Single,
            },
            Path::new("/tmp/output.mp4"),
            &RecordingProgress::default(),
        )
        .expect_err("invalid payload should fail");

    assert!(matches!(error, ProcessorError::PayloadCodec(_)));
    assert!(error.to_string().contains("success payload"));
}

#[test]
fn dynamic_post_download_processor_reports_abi_violations() {
    let api = VesperPostDownloadProcessorApi {
        process_json: Some(fixture_null_payload_process_json),
        ..fixture_processor_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::PostDownloadProcessor,
        plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperPostDownloadProcessorApi).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
    let processor = plugin
        .post_download_processor()
        .expect("processor should be available");
    let error = processor
        .process(
            &CompletedDownloadInfo {
                asset_id: "asset-a".to_owned(),
                task_id: Some("1".to_owned()),
                content_format: CompletedContentFormat::SingleFile {
                    path: PathBuf::from("/tmp/input.mp4"),
                },
                metadata: DownloadMetadata::default(),
                streams: Vec::new(),
                assembly_mode: AssemblyMode::Single,
            },
            Path::new("/tmp/output.mp4"),
            &RecordingProgress::default(),
        )
        .expect_err("null payload pointer should fail");

    assert!(matches!(error, ProcessorError::AbiViolation(_)));
    assert!(error.to_string().contains("null data pointer"));
}

#[test]
#[ignore = "requires a built player-remux-ffmpeg shared library artifact"]
fn dynamic_loader_opens_real_vesper_remux_ffmpeg_shared_library() {
    let plugin_path = resolve_vesper_remux_ffmpeg_plugin_path().unwrap_or_else(|error| {
        panic!("failed to resolve player-remux-ffmpeg plugin path: {error}")
    });

    let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
        panic!(
            "failed to load player-remux-ffmpeg shared library `{}`: {error}",
            plugin_path.display()
        )
    });

    assert_eq!(plugin.plugin_name(), "player-remux-ffmpeg");
    assert!(plugin.pipeline_event_hook().is_none());

    let processor = plugin
        .post_download_processor()
        .expect("player-remux-ffmpeg should export a post-download processor");
    assert_eq!(processor.name(), "player-remux-ffmpeg");
    assert_eq!(
        processor.capabilities(),
        ProcessorCapabilities {
            supported_input_formats: vec![
                ContentFormatKind::HlsSegments,
                ContentFormatKind::DashSegments,
                ContentFormatKind::FlvSegments,
                ContentFormatKind::SingleFile,
            ],
            output_formats: vec![OutputFormat::Mp4, OutputFormat::Mkv],
            supports_cancellation: true,
            supports_assembly: true,
            supported_assembly_modes: vec![
                AssemblyMode::SeparateAudioVideo,
                AssemblyMode::MultiAudio,
                AssemblyMode::WithSubtitles,
                AssemblyMode::Generic,
            ],
        }
    );

    let progress = RecordingProgress::default();
    let output = processor
        .process(
            &CompletedDownloadInfo {
                asset_id: "asset-a".to_owned(),
                task_id: Some("1".to_owned()),
                content_format: CompletedContentFormat::SingleFile {
                    path: PathBuf::from("/tmp/input.mp4"),
                },
                metadata: DownloadMetadata::default(),
                streams: Vec::new(),
                assembly_mode: AssemblyMode::Single,
            },
            Path::new("/tmp/output.mp4"),
            &progress,
        )
        .expect("single-file input should be skipped by player-remux-ffmpeg");

    assert_eq!(output, ProcessorOutput::Skipped);
    assert!(progress.ratios().is_empty());
}
