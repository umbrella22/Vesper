use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use player_backend_ffmpeg::FfmpegBackend;
use player_model::MediaSource;

fn tiny_fixture_source() -> MediaSource {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/media/tiny-h264-aac.m4v");
    assert!(
        fixture.is_file(),
        "missing FFmpeg integration fixture at {}",
        fixture.display()
    );
    MediaSource::new(fixture.to_string_lossy().into_owned())
}

#[test]
fn probe_decode_audio_video_and_seek_fixture() -> Result<()> {
    let backend = FfmpegBackend::new()?;
    let source = tiny_fixture_source();

    let probe = backend.probe(source.clone())?;
    assert_eq!(probe.video_streams, 1);
    assert_eq!(probe.audio_streams, 1);
    assert!(
        probe
            .duration
            .is_some_and(|duration| duration >= Duration::from_secs(1)),
        "probe duration should cover the generated fixture"
    );

    let best_video = probe.best_video.as_ref().expect("fixture has video");
    assert_eq!(best_video.width, 128);
    assert_eq!(best_video.height, 72);

    let best_audio = probe.best_audio.as_ref().expect("fixture has audio");
    assert_eq!(best_audio.sample_rate, 48_000);
    assert!(best_audio.channels >= 1);

    let mut video = backend.open_video_source(source.clone())?;
    let first_frame = video
        .next_frame()?
        .expect("fixture should decode a first frame");
    assert_eq!(first_frame.width, 128);
    assert_eq!(first_frame.height, 72);
    assert!(!first_frame.bytes.is_empty());

    let second_frame = video
        .next_frame()?
        .expect("fixture should decode a second frame");
    assert!(second_frame.presentation_time >= first_frame.presentation_time);

    let seeked_frame = video
        .seek_to(Duration::from_millis(500))?
        .expect("fixture should decode after seek");
    assert!(!seeked_frame.bytes.is_empty());

    let audio = backend.decode_audio_track(source, 48_000, 2)?;
    assert_eq!(audio.sample_rate, 48_000);
    assert_eq!(audio.channels, 2);
    assert!(!audio.samples.is_empty());
    assert!(audio.duration() >= Duration::from_secs(1));

    Ok(())
}
