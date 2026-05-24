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
