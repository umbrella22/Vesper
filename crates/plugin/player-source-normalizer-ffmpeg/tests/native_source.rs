use std::path::PathBuf;
use std::time::{Duration, Instant};

use player_plugin::author::PluginError;
use player_plugin::{
    SourceNormalizerOutputRoute, SourceNormalizerPacketMediaKind,
    SourceNormalizerPacketSessionConfig, SourceNormalizerResourceCachePolicy,
    SourceNormalizerResourceSessionConfig, SourceNormalizerResourceSessionState,
};
use player_plugin_abi::VESPER_TRY_AGAIN;
use player_plugin_loader::native::{NativePacketStreamPoll, NativePlugin};

fn fixture() -> String {
    std::env::var_os("VESPER_NATIVE_MEDIA_FIXTURE")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .expect("VESPER_NATIVE_MEDIA_FIXTURE must point to an AVC/AAC MP4 fixture")
        .to_string_lossy()
        .into_owned()
}

fn plugin() -> NativePlugin {
    // SAFETY: the entry is statically linked from the first-party normalizer.
    unsafe { NativePlugin::from_entry(vesper_source_normalizer_ffmpeg::vesper_native_plugin_entry) }
        .expect("native source normalizer root")
}

#[test]
#[ignore = "requires VESPER_NATIVE_MEDIA_FIXTURE"]
fn packet_lease_keeps_demuxer_live_through_seek_and_owner_drop() {
    let mut stream = plugin()
        .open_packet_stream(&SourceNormalizerPacketSessionConfig {
            runtime_profile: "generic-fallback".into(),
            input: fixture(),
            headers: Vec::new(),
            startup_timeout_ms: Some(3000),
            session_timeout_ms: Some(10000),
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        })
        .expect("open native packet stream");
    assert!(stream.stream_info().seekable);
    assert!(
        stream
            .stream_info()
            .tracks
            .iter()
            .any(|track| track.media_kind == SourceNormalizerPacketMediaKind::Audio)
    );
    let NativePacketStreamPoll::Packet(first) = stream.read().expect("read first packet") else {
        panic!("expected packet");
    };
    assert!(!first.as_ref().is_empty());
    let original_size = first.as_ref().len();
    assert!(matches!(
        stream.read().expect("check backpressure"),
        NativePacketStreamPoll::Pending
    ));
    assert_eq!(
        stream.seek(Duration::ZERO),
        Err(PluginError::Status(VESPER_TRY_AGAIN))
    );
    drop(first);
    stream.seek(Duration::ZERO).expect("seek after return");
    let NativePacketStreamPoll::Packet(next) = stream.read().expect("read after seek") else {
        panic!("expected packet after seek");
    };
    let expected = next.as_ref().to_vec();
    assert!(original_size > 0 && !expected.is_empty());
    drop(stream);
    std::thread::spawn(move || assert_eq!(next.as_ref(), expected))
        .join()
        .expect("late lease return");
}

#[test]
#[ignore = "requires VESPER_NATIVE_OFFSET_TS_FIXTURE with a nonzero start and one-second keyframes"]
fn packet_seek_uses_media_relative_time_for_offset_ts() {
    let input = std::env::var("VESPER_NATIVE_OFFSET_TS_FIXTURE").expect("offset TS fixture path");
    let mut stream = plugin()
        .open_packet_stream(&SourceNormalizerPacketSessionConfig {
            runtime_profile: "generic-fallback".into(),
            input,
            headers: Vec::new(),
            startup_timeout_ms: Some(3000),
            session_timeout_ms: Some(10000),
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        })
        .expect("open offset TS through the native packet ABI");
    assert!(stream.stream_info().seekable);
    let NativePacketStreamPoll::Packet(first) = stream.read().expect("first packet") else {
        panic!("expected first packet");
    };
    let start_pts = first.descriptor().pts_us;
    assert!(start_pts >= 8_000_000, "fixture start PTS: {start_pts}");
    drop(first);

    stream
        .seek(Duration::from_secs(6))
        .expect("seek from media start");
    let NativePacketStreamPoll::Packet(packet) = stream.read().expect("packet after seek") else {
        panic!("expected packet after seek");
    };
    let position = packet.descriptor().pts_us - start_pts;
    assert!(
        (4_900_000..=6_100_000).contains(&position),
        "seek returned a packet at media offset {position} us, start PTS {start_pts} us"
    );
}

#[test]
#[ignore = "requires VESPER_NATIVE_MEDIA_FIXTURE"]
fn resource_mode_produces_a_file_with_an_independent_lifetime() {
    let output = tempfile::tempdir().expect("resource output directory");
    let mut resource = plugin()
        .open_source_resource(&SourceNormalizerResourceSessionConfig {
            runtime_profile: "generic-fallback".into(),
            input: fixture(),
            headers: Vec::new(),
            output_root: output.path().to_string_lossy().into_owned(),
            cache_policy: SourceNormalizerResourceCachePolicy::default(),
            preferred_route: Some(SourceNormalizerOutputRoute::Fmp4LocalStream),
            startup_timeout_ms: Some(3000),
            read_idle_timeout_ms: Some(4000),
        })
        .expect("open native resource session");
    let deadline = Instant::now() + Duration::from_secs(6);
    let path = loop {
        let status = resource.poll().expect("poll resource");
        if matches!(
            status.state,
            SourceNormalizerResourceSessionState::Ready
                | SourceNormalizerResourceSessionState::Completed
        ) {
            let path = status
                .info
                .expect("resource metadata")
                .primary_resource_path
                .expect("resource path");
            break PathBuf::from(path);
        }
        assert!(
            Instant::now() < deadline,
            "resource state {:?}, {:?}",
            status.state,
            status.message
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(path.starts_with(output.path()));
    assert!(path.metadata().expect("read ready resource metadata").len() > 0);
    drop(resource);
    assert!(!path.exists());
}
