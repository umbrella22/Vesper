use super::*;
use player_plugin::{
    VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2, VesperSourceNormalizerPluginApiV2,
};

#[test]
fn dual_source_normalizer_context_outlives_each_factory_and_destroys_once() {
    let _guard = source_normalizer_packet_test_guard();
    let destroys = std::sync::atomic::AtomicUsize::new(0);
    let api = VesperSourceNormalizerPluginApiV4 {
        context: (&destroys as *const std::sync::atomic::AtomicUsize)
            .cast_mut()
            .cast::<c_void>(),
        destroy: Some(count_source_normalizer_destroy),
        ..fixture_source_normalizer_dual_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load dual source normalizer");
    let packet_factory = plugin
        .source_normalizer_packet_plugin_factory()
        .expect("packet factory");
    let resource_factory = plugin
        .source_normalizer_resource_plugin_factory()
        .expect("resource factory");
    let mut resource_session = resource_factory
        .open_resource_session(&player_plugin::SourceNormalizerResourceSessionConfig {
            runtime_profile: "fixture-resource".to_owned(),
            input: "file:///tmp/input.mp4".to_owned(),
            headers: Vec::new(),
            output_root: "/tmp/vesper-source-normalizer-fixture".to_owned(),
            cache_policy: SourceNormalizerResourceCachePolicy::default(),
            preferred_route: Some(SourceNormalizerOutputRoute::Fmp4LocalStream),
            startup_timeout_ms: Some(10),
            read_idle_timeout_ms: Some(10),
        })
        .expect("open resource session");

    drop(plugin);
    drop(packet_factory);
    drop(resource_factory);
    assert_eq!(destroys.load(std::sync::atomic::Ordering::SeqCst), 0);
    resource_session
        .wait_for_update(std::time::Duration::from_millis(1))
        .expect("resource session remains valid after packet factory drops");
    resource_session.close().expect("close resource session");
    assert_eq!(destroys.load(std::sync::atomic::Ordering::SeqCst), 0);
    drop(resource_session);
    assert_eq!(destroys.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn dual_source_normalizer_context_outlives_both_session_families_in_reverse_drop_order() {
    let _guard = source_normalizer_packet_test_guard();
    let destroys = std::sync::atomic::AtomicUsize::new(0);
    let api = VesperSourceNormalizerPluginApiV4 {
        context: (&destroys as *const std::sync::atomic::AtomicUsize)
            .cast_mut()
            .cast::<c_void>(),
        destroy: Some(count_source_normalizer_destroy),
        ..fixture_source_normalizer_dual_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load dual source normalizer");
    let packet_factory = plugin
        .source_normalizer_packet_plugin_factory()
        .expect("packet factory");
    let resource_factory = plugin
        .source_normalizer_resource_plugin_factory()
        .expect("resource factory");
    let mut packet_session = packet_factory
        .open_packet_session(&SourceNormalizerPacketSessionConfig {
            runtime_profile: "fixture-packet".to_owned(),
            input: "file:///tmp/input.mp4".to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        })
        .expect("open packet session");
    let mut resource_session = resource_factory
        .open_resource_session(&player_plugin::SourceNormalizerResourceSessionConfig {
            runtime_profile: "fixture-resource".to_owned(),
            input: "file:///tmp/input.mp4".to_owned(),
            headers: Vec::new(),
            output_root: "/tmp/vesper-source-normalizer-fixture".to_owned(),
            cache_policy: SourceNormalizerResourceCachePolicy::default(),
            preferred_route: Some(SourceNormalizerOutputRoute::Fmp4LocalStream),
            startup_timeout_ms: Some(10),
            read_idle_timeout_ms: Some(10),
        })
        .expect("open resource session");

    drop(plugin);
    drop(resource_factory);
    drop(packet_factory);
    assert_eq!(destroys.load(std::sync::atomic::Ordering::SeqCst), 0);

    resource_session.close().expect("close resource session");
    drop(resource_session);
    assert_eq!(destroys.load(std::sync::atomic::Ordering::SeqCst), 0);

    packet_session.close().expect("close packet session");
    drop(packet_session);
    assert_eq!(destroys.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn source_normalizer_constructor_failure_destroys_context_once() {
    let destroys = std::sync::atomic::AtomicUsize::new(0);
    let api = VesperSourceNormalizerPluginApiV4 {
        context: (&destroys as *const std::sync::atomic::AtomicUsize)
            .cast_mut()
            .cast::<c_void>(),
        destroy: Some(count_source_normalizer_destroy),
        packet_capabilities_json: Some(malformed_source_normalizer_capabilities),
        ..fixture_source_normalizer_packet_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };

    LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect_err("malformed capabilities should reject the plugin");
    assert_eq!(destroys.load(std::sync::atomic::Ordering::SeqCst), 1);
}

unsafe extern "C" fn count_source_normalizer_destroy(context: *mut c_void) {
    // SAFETY: these tests pass a live AtomicUsize as the plugin context.
    if let Some(counter) = unsafe { context.cast::<std::sync::atomic::AtomicUsize>().as_ref() } {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

unsafe extern "C" fn malformed_source_normalizer_capabilities(
    _context: *mut c_void,
) -> VesperPluginBytes {
    VesperPluginBytes::from_vec(b"{".to_vec())
}

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
fn dynamic_source_normalizer_packet_release_failure_keeps_outstanding_handle_for_retry() {
    let _guard = source_normalizer_packet_test_guard();
    reset_source_normalizer_packet_releases();
    reset_source_normalizer_session_closes();
    let api = VesperSourceNormalizerPluginApiV4 {
        release_packet: Some(fixture_source_normalizer_release_packet_error),
        ..fixture_source_normalizer_packet_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load source normalizer packet plugin");
    let factory = plugin
        .source_normalizer_packet_plugin_factory()
        .expect("packet factory should be available");
    let mut session = factory
        .open_packet_session(&SourceNormalizerPacketSessionConfig {
            runtime_profile: "fixture-packet".to_owned(),
            input: "file:///tmp/input.mp4".to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        })
        .expect("open packet session");

    let packet = session.read_packet().expect("read first packet");
    let handle = packet.handle;
    drop(packet);

    let error = session
        .flush()
        .expect_err("flush should fail when release fails");
    assert!(error.to_string().contains("release packet failed"));
    assert_eq!(source_normalizer_packet_releases(), vec![handle]);

    let error = session
        .release_packet(handle)
        .expect_err("explicit retry should still see the outstanding handle");
    assert!(error.to_string().contains("release packet failed"));
    assert_eq!(source_normalizer_packet_releases(), vec![handle, handle]);

    let error = session
        .close()
        .expect_err("close reports the retained release failure");
    assert!(error.to_string().contains("release packet failed"));
    assert_eq!(
        source_normalizer_packet_releases(),
        vec![handle, handle, handle]
    );
    assert_eq!(source_normalizer_packet_closes(), 1);
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
fn dynamic_source_normalizer_packet_read_releases_handle_when_metadata_is_malformed() {
    let _guard = source_normalizer_packet_test_guard();
    reset_source_normalizer_packet_releases();
    let api = VesperSourceNormalizerPluginApiV4 {
        read_packet: Some(fixture_source_normalizer_read_packet_malformed_metadata),
        ..fixture_source_normalizer_packet_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load source normalizer packet plugin");
    let factory = plugin
        .source_normalizer_packet_plugin_factory()
        .expect("packet factory should be available");
    let mut session = factory
        .open_packet_session(&SourceNormalizerPacketSessionConfig {
            runtime_profile: "fixture-packet".to_owned(),
            input: "file:///tmp/input.mp4".to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        })
        .expect("open packet session");

    let error = session
        .read_packet()
        .expect_err("malformed packet metadata should fail");

    assert!(error.to_string().contains("read_packet"));
    assert_eq!(source_normalizer_packet_releases(), vec![0x52]);
}

#[test]
fn dynamic_source_normalizer_packet_open_closes_session_when_success_payload_is_malformed() {
    let _guard = source_normalizer_packet_test_guard();
    reset_source_normalizer_session_closes();
    let api = VesperSourceNormalizerPluginApiV4 {
        open_packet_session_json: Some(
            fixture_source_normalizer_open_packet_session_malformed_payload_json,
        ),
        ..fixture_source_normalizer_packet_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load source normalizer packet plugin");
    let factory = plugin
        .source_normalizer_packet_plugin_factory()
        .expect("packet factory should be available");

    let error = match factory.open_packet_session(&SourceNormalizerPacketSessionConfig {
        runtime_profile: "fixture-packet".to_owned(),
        input: "file:///tmp/input.mp4".to_owned(),
        headers: Vec::new(),
        startup_timeout_ms: None,
        session_timeout_ms: None,
        preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
    }) {
        Ok(_) => panic!("malformed success payload should fail"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("open_packet_session"));
    assert_eq!(source_normalizer_packet_closes(), 1);
}

#[test]
fn dynamic_source_normalizer_resource_open_closes_session_when_success_payload_is_malformed() {
    let _guard = source_normalizer_packet_test_guard();
    reset_source_normalizer_session_closes();
    let api = VesperSourceNormalizerPluginApiV4 {
        open_resource_session_json: Some(
            fixture_source_normalizer_open_resource_session_malformed_payload_json,
        ),
        ..fixture_source_normalizer_dual_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load source normalizer resource plugin");
    let factory = plugin
        .source_normalizer_resource_plugin_factory()
        .expect("resource factory should be available");

    let error =
        match factory.open_resource_session(&player_plugin::SourceNormalizerResourceSessionConfig {
            runtime_profile: "fixture-resource".to_owned(),
            input: "file:///tmp/input.mp4".to_owned(),
            headers: Vec::new(),
            output_root: "/tmp/vesper-source-normalizer-fixture".to_owned(),
            cache_policy: SourceNormalizerResourceCachePolicy::default(),
            preferred_route: Some(SourceNormalizerOutputRoute::Fmp4LocalStream),
            startup_timeout_ms: Some(10),
            read_idle_timeout_ms: Some(10),
        }) {
            Ok(_) => panic!("malformed success payload should fail"),
            Err(error) => error,
        };

    assert!(error.to_string().contains("open_resource_session"));
    assert_eq!(source_normalizer_resource_closes(), 1);
}

#[test]
fn dynamic_source_normalizer_packet_plugin_rejects_missing_release_callback() {
    let api = VesperSourceNormalizerPluginApiV4 {
        release_packet: None,
        ..fixture_source_normalizer_packet_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };

    let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect_err("packet ABI requires release_packet");

    assert!(matches!(
        error,
        PluginLoadError::CapabilitiesAbiViolation(message)
            if message.contains("exports no complete packet or resource callback group")
    ));
}

#[test]
fn dynamic_source_normalizer_resource_plugin_rejects_missing_wait_callback() {
    let api = VesperSourceNormalizerPluginApiV4 {
        wait_resource_session_update: None,
        ..fixture_source_normalizer_dual_api()
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("packet callback group should still load");
    assert!(
        plugin.source_normalizer_resource_plugin_factory().is_none(),
        "resource callback group without wait must not be exposed"
    );
}

#[test]
fn dynamic_source_normalizer_resource_session_wait_for_update_decodes_status() {
    let _guard = source_normalizer_packet_test_guard();
    let api = fixture_source_normalizer_dual_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load source normalizer v4 plugin");
    let factory = plugin
        .source_normalizer_resource_plugin_factory()
        .expect("resource factory");
    let mut session = factory
        .open_resource_session(&player_plugin::SourceNormalizerResourceSessionConfig {
            runtime_profile: "fixture-resource".to_owned(),
            input: "file:///tmp/input.mp4".to_owned(),
            headers: Vec::new(),
            output_root: "/tmp/vesper-source-normalizer-fixture".to_owned(),
            cache_policy: SourceNormalizerResourceCachePolicy::default(),
            preferred_route: Some(SourceNormalizerOutputRoute::Fmp4LocalStream),
            startup_timeout_ms: Some(10),
            read_idle_timeout_ms: Some(10),
        })
        .expect("open resource session");

    let wait = session
        .wait_for_update(std::time::Duration::from_millis(1))
        .expect("wait for update");
    assert!(wait.updated);
    session.close().expect("close resource session");
}

#[test]
fn plugin_registry_reports_source_normalizer_packet_current_support() {
    let api = fixture_source_normalizer_packet_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load source normalizer packet plugin");
    let record = PluginDiagnosticRecord::from_loaded_source_normalizer_plugin(
        PathBuf::from("test-source-normalizer-packet"),
        &plugin,
    )
    .expect("source normalizer diagnostics returns one or more records");

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
            .is_some_and(|message| message.contains("source normalizer packet route"))
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
fn dynamic_source_normalizer_plugin_rejects_legacy_v2_signature() {
    let api = VesperSourceNormalizerPluginApiV2 {
        context: std::ptr::null_mut(),
        destroy: None,
        name: Some(fixture_source_normalizer_packet_name),
        packet_capabilities_json: Some(fixture_source_normalizer_packet_capabilities_json),
        open_packet_session_json: Some(fixture_source_normalizer_open_packet_session_json),
        read_packet: Some(fixture_source_normalizer_read_packet),
        release_packet: Some(fixture_source_normalizer_release_packet),
        seek_packet_session_json: Some(fixture_source_normalizer_seek_packet_session_json),
        flush_packet_session: Some(fixture_source_normalizer_flush_packet_session),
        close_packet_session: Some(fixture_source_normalizer_close_packet_session),
        free_bytes: Some(fixture_free_bytes),
    };
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV2).cast(),
    };

    let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect_err("legacy source normalizer ABI should be rejected");

    assert!(matches!(
        error,
        PluginLoadError::AbiVersionMismatch {
            expected,
            actual: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2
        } if expected == VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT.to_string()
    ));
}

#[test]
fn plugin_registry_reports_current_source_normalizer_packet_and_resource_support() {
    let api = fixture_source_normalizer_dual_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
        plugin_kind: VesperPluginKind::SourceNormalizer as u32,
        plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperSourceNormalizerPluginApiV4).cast(),
    };
    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
        .expect("load source normalizer v4 plugin");
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

#[test]
fn plugin_registry_packet_profile_selection_ignores_resource_only_match() {
    let registry = PluginRegistry::from_records(vec![
        resource_source_normalizer_record("resource-only", &["flv"]),
        packet_source_normalizer_record("packet-generic", &["generic-fallback"]),
        packet_source_normalizer_record("packet-flv", &["flv"]),
    ]);

    assert_eq!(
        registry
            .best_source_normalizer_packet_for_profile("FLV")
            .and_then(|record| record.plugin_name.as_deref()),
        Some("packet-flv")
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
        message: Some("source normalizer resource route".to_owned()),
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
        message: Some("source normalizer packet route".to_owned()),
    }
}
