use super::*;

#[test]
fn dynamic_source_normalizer_packet_plugin_round_trips_packet_lifecycle() {
    let _guard = source_normalizer_packet_test_guard();
    reset_source_normalizer_packet_releases();
    let factory = fixture_source_normalizer_packet_factory();
    assert_eq!(factory.name(), "test-source-normalizer-packet");
    assert!(factory.packet_capabilities().supports_codec("h264"));

    let mut session = fixture_source_normalizer_packet_session();
    assert_eq!(
        session.stream_info().normalizer_name.as_deref(),
        Some("test-source-normalizer-packet")
    );

    let packet = session.read_packet().expect("read first packet");
    assert_eq!(
        packet.metadata.status,
        SourceNormalizerReadPacketStatus::Packet
    );
    assert_eq!(packet.data, &[0, 0, 1, 9]);
    let handle = packet.handle;
    drop(packet);

    assert!(
        session.read_packet().is_err(),
        "loader should require release before another read"
    );
    session.release_packet(handle).expect("release packet");
    assert_eq!(source_normalizer_packet_releases(), vec![handle]);
    assert!(
        session.release_packet(handle).is_err(),
        "double release should fail before calling the plugin again"
    );

    let eos = session.read_packet().expect("read eos");
    assert_eq!(
        eos.metadata.status,
        SourceNormalizerReadPacketStatus::EndOfStream
    );
    assert_eq!(eos.handle, 0);
    session.close().expect("close packet session");
    assert!(
        session.read_packet().is_err(),
        "read after close should report not configured"
    );
}

#[test]
fn dynamic_source_normalizer_packet_plugin_seek_releases_outstanding_packet() {
    let _guard = source_normalizer_packet_test_guard();
    reset_source_normalizer_packet_releases();
    let mut session = fixture_source_normalizer_packet_session();

    let packet = session.read_packet().expect("read first packet");
    let handle = packet.handle;
    drop(packet);

    let status = session
        .seek(&SourceNormalizerPacketSeek {
            position_millis: 250,
            exact: false,
        })
        .expect("seek should release outstanding packet");
    assert!(status.completed);
    assert_eq!(source_normalizer_packet_releases(), vec![handle]);

    let packet = session.read_packet().expect("read packet after seek");
    let metadata = packet.metadata.clone();
    let handle_after_seek = packet.handle;
    drop(packet);
    let packet = metadata.packet.expect("packet metadata");
    assert_eq!(packet.pts_us, Some(250_000));
    assert!(packet.discontinuity);

    session
        .release_packet(handle_after_seek)
        .expect("release packet after seek");
}

#[test]
fn dynamic_source_normalizer_packet_plugin_flush_releases_outstanding_packet() {
    let _guard = source_normalizer_packet_test_guard();
    reset_source_normalizer_packet_releases();
    let mut session = fixture_source_normalizer_packet_session();

    let packet = session.read_packet().expect("read first packet");
    let handle = packet.handle;
    drop(packet);

    let status = session
        .flush()
        .expect("flush should release outstanding packet");
    assert!(status.completed);
    assert_eq!(source_normalizer_packet_releases(), vec![handle]);

    let packet = session.read_packet().expect("read packet after flush");
    assert_eq!(
        packet.metadata.status,
        SourceNormalizerReadPacketStatus::Packet
    );
    assert_eq!(
        packet
            .metadata
            .packet
            .as_ref()
            .and_then(|packet| packet.pts_us),
        Some(1_000)
    );
    let handle_after_flush = packet.handle;
    drop(packet);

    session
        .release_packet(handle_after_flush)
        .expect("release packet after flush");
}

#[test]
fn dynamic_source_normalizer_packet_plugin_drop_releases_outstanding_packet() {
    let _guard = source_normalizer_packet_test_guard();
    reset_source_normalizer_packet_releases();
    let mut session = fixture_source_normalizer_packet_session();

    let packet = session.read_packet().expect("read first packet");
    let handle = packet.handle;
    drop(packet);

    drop(session);
    assert_eq!(source_normalizer_packet_releases(), vec![handle]);
}

#[test]
fn dynamic_source_normalizer_packet_plugin_rejects_missing_release_callback() {
    let api = VesperSourceNormalizerPluginApiV2 {
        release_packet: None,
        ..fixture_source_normalizer_packet_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2,
        plugin_kind: VesperPluginKind::SourceNormalizer,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV2).cast(),
    };

    let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect_err("packet ABI requires release_packet");

    assert!(matches!(
        error,
        PluginLoadError::MissingField {
            field: "source_normalizer_plugin_api_v2.release_packet"
        }
    ));
}

#[test]
fn plugin_registry_reports_source_normalizer_packet_v2_support() {
    let api = fixture_source_normalizer_packet_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2,
        plugin_kind: VesperPluginKind::SourceNormalizer,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV2).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load source normalizer packet plugin");
    let record = PluginDiagnosticRecord::from_loaded_source_normalizer_plugin(
        PathBuf::from("test-source-normalizer-packet"),
        &plugin,
    );

    assert_eq!(
        record.status,
        PluginDiagnosticStatus::SourceNormalizerSupported
    );
    assert_eq!(
        record.plugin_name.as_deref(),
        Some("test-source-normalizer-packet")
    );
    assert!(matches!(
        record.capability_summary,
        Some(PluginCapabilitySummary::SourceNormalizerPacket(_))
    ));
    assert!(
        record
            .message
            .as_deref()
            .is_some_and(|message| message.contains("source_normalizer_packet_v2"))
    );

    let registry = PluginRegistry::from_records(vec![record]);
    assert_eq!(
        registry
            .best_source_normalizer_packet()
            .and_then(|record| record.plugin_name.as_deref()),
        Some("test-source-normalizer-packet")
    );
    assert_eq!(
        registry
            .best_source_normalizer_for_profile("fixture-packet")
            .and_then(|record| record.plugin_name.as_deref()),
        Some("test-source-normalizer-packet")
    );
}

#[test]
fn plugin_registry_reports_v3_source_normalizer_packet_and_resource_support() {
    let api = fixture_source_normalizer_dual_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V3,
        plugin_kind: VesperPluginKind::SourceNormalizer,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV3).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load source normalizer v3 plugin");
    let registry = PluginRegistry::from_records(
        PluginDiagnosticRecord::from_loaded_source_normalizer_plugin_records(
            PathBuf::from("test-source-normalizer-dual"),
            &plugin,
        ),
    );

    assert_eq!(registry.records().len(), 2);
    assert_eq!(
        registry
            .best_source_normalizer_packet()
            .and_then(|record| record.plugin_name.as_deref()),
        Some("test-source-normalizer-packet")
    );
    assert_eq!(
        registry
            .best_source_normalizer_resource()
            .and_then(|record| record.plugin_name.as_deref()),
        Some("test-source-normalizer-packet")
    );
    assert_eq!(
        registry
            .best_source_normalizer_for_profile("fixture-packet")
            .and_then(|record| record.capability_summary.as_ref()),
        Some(&PluginCapabilitySummary::SourceNormalizerPacket(
            SourceNormalizerPacketPluginCapabilitySummary {
                supported_runtime_profiles: vec!["fixture-packet".to_owned()],
                max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
                media_kinds: vec![SourceNormalizerPacketMediaKind::Video],
                codecs: vec!["H264".to_owned()],
                bitstream_formats: vec![DecoderBitstreamFormat::Avcc],
                supports_seek: true,
                supports_flush: true,
                required_capabilities: SourceNormalizerRequiredCapabilities::default(),
                max_sessions: Some(1),
            }
        ))
    );
    assert_eq!(
        registry
            .best_source_normalizer_resource_for_profile("fixture-resource")
            .and_then(|record| record.capability_summary.as_ref()),
        Some(&PluginCapabilitySummary::SourceNormalizerResource(
            SourceNormalizerResourcePluginCapabilitySummary {
                supported_runtime_profiles: vec!["fixture-resource".to_owned()],
                supported_output_routes: vec!["fmp4LocalStream".to_owned()],
                max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
                content_types: vec!["video/mp4".to_owned()],
                supports_growing_resources: true,
                supports_range_reads: true,
                supports_cancel: true,
                required_capabilities: SourceNormalizerRequiredCapabilities::default(),
                cache_policy: SourceNormalizerResourceCachePolicy::default(),
                max_sessions: Some(1),
            }
        ))
    );
}

#[test]
fn plugin_registry_selects_resource_source_normalizer_by_profile() {
    let registry = PluginRegistry::from_records(vec![
        resource_source_normalizer_record("generic-resource", &["generic-fallback"]),
        resource_source_normalizer_record("flv-resource", &["flv", "hevc-flv"]),
    ]);

    assert_eq!(
        registry
            .best_source_normalizer_resource_for_profile("flv")
            .and_then(|record| record.plugin_name.as_deref()),
        Some("flv-resource")
    );
    assert_eq!(
        registry
            .best_source_normalizer_resource_for_profile("HEVC-FLV")
            .and_then(|record| record.plugin_name.as_deref()),
        Some("flv-resource")
    );
}

#[test]
fn plugin_registry_resource_profile_selection_ignores_packet_only_match() {
    let registry = PluginRegistry::from_records(vec![
        packet_source_normalizer_record("packet-only", &["flv"]),
        resource_source_normalizer_record("resource-generic", &["generic-fallback"]),
    ]);

    assert_eq!(
        registry
            .best_source_normalizer_for_profile("flv")
            .and_then(|record| record.plugin_name.as_deref()),
        Some("packet-only")
    );
    assert!(
        registry
            .best_source_normalizer_resource_for_profile("flv")
            .is_none(),
        "mobile resource playback must not select a packet-only profile match"
    );
}

fn resource_source_normalizer_record(name: &str, profiles: &[&str]) -> PluginDiagnosticRecord {
    PluginDiagnosticRecord {
        path: PathBuf::from(format!("/plugins/{name}.so")),
        status: PluginDiagnosticStatus::SourceNormalizerSupported,
        plugin_name: Some(name.to_owned()),
        plugin_kind: Some(VesperPluginKind::SourceNormalizer),
        capability_summary: Some(PluginCapabilitySummary::SourceNormalizerResource(
            SourceNormalizerResourcePluginCapabilitySummary {
                supported_runtime_profiles: profiles
                    .iter()
                    .map(|profile| (*profile).to_owned())
                    .collect(),
                supported_output_routes: vec!["fmp4LocalStream".to_owned()],
                max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
                content_types: vec!["video/mp4".to_owned()],
                supports_growing_resources: true,
                supports_range_reads: true,
                supports_cancel: true,
                required_capabilities: SourceNormalizerRequiredCapabilities::default(),
                cache_policy: SourceNormalizerResourceCachePolicy::default(),
                max_sessions: Some(1),
            },
        )),
        message: Some("source_normalizer_resource_v3".to_owned()),
    }
}

fn packet_source_normalizer_record(name: &str, profiles: &[&str]) -> PluginDiagnosticRecord {
    PluginDiagnosticRecord {
        path: PathBuf::from(format!("/plugins/{name}.so")),
        status: PluginDiagnosticStatus::SourceNormalizerSupported,
        plugin_name: Some(name.to_owned()),
        plugin_kind: Some(VesperPluginKind::SourceNormalizer),
        capability_summary: Some(PluginCapabilitySummary::SourceNormalizerPacket(
            SourceNormalizerPacketPluginCapabilitySummary {
                supported_runtime_profiles: profiles
                    .iter()
                    .map(|profile| (*profile).to_owned())
                    .collect(),
                max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
                media_kinds: vec![SourceNormalizerPacketMediaKind::Video],
                codecs: vec!["h264".to_owned()],
                bitstream_formats: vec![DecoderBitstreamFormat::AnnexB],
                supports_seek: true,
                supports_flush: true,
                required_capabilities: SourceNormalizerRequiredCapabilities::default(),
                max_sessions: Some(1),
            },
        )),
        message: Some("source_normalizer_packet_v2".to_owned()),
    }
}
