use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

use player_audio_cpal::{AudioSink, default_output_config};

const PHYSICAL_AUDIO_TEST_ENV: &str = "VESPER_RUN_PHYSICAL_AUDIO_TESTS";

#[test]
#[ignore = "requires an explicitly enabled physical audio output device"]
fn physical_output_plays_only_the_latest_generation_and_drops_promptly()
-> Result<(), Box<dyn Error>> {
    if std::env::var(PHYSICAL_AUDIO_TEST_ENV).as_deref() != Ok("1") {
        return Err(format!("set {PHYSICAL_AUDIO_TEST_ENV}=1 to run this device test").into());
    }

    eprintln!("physical audio: resolving the default output");
    let output_config = default_output_config()?;
    let sample_rate = usize::try_from(output_config.sample_rate)?;
    let channels = usize::from(output_config.channels);
    eprintln!("physical audio: opening a paused output stream");
    let mut sink = AudioSink::new_default(output_config, Duration::ZERO, 1.0, true)?;
    let controller = sink.controller();

    let stale_generation = controller.begin_generation(Duration::ZERO, 1.0);
    let stale_samples = sample_rate.saturating_mul(channels) / 2;
    assert!(controller.append_samples(stale_generation, vec![0.0; stale_samples])?);

    let active_generation = controller.begin_generation(Duration::ZERO, 1.0);
    assert_ne!(active_generation, stale_generation);
    assert!(!controller.is_generation_active(stale_generation));
    assert!(!controller.append_samples(stale_generation, vec![0.0; channels])?);

    let active_samples = sample_rate.saturating_mul(channels) / 4;
    assert!(controller.append_samples(active_generation, vec![0.0; active_samples])?);
    controller.finish_generation(active_generation);
    eprintln!("physical audio: starting the latest generation");
    sink.play();

    let playback_deadline = Instant::now() + Duration::from_secs(5);
    while !sink.is_finished() && Instant::now() < playback_deadline {
        thread::sleep(Duration::from_millis(10));
    }

    assert!(
        sink.is_finished(),
        "physical output did not drain before the deadline"
    );
    assert!(
        sink.playback_position() >= Duration::from_millis(200),
        "physical output callback did not advance the active generation"
    );

    eprintln!("physical audio: dropping the stream");
    let drop_started = Instant::now();
    drop(controller);
    drop(sink);
    assert!(
        drop_started.elapsed() < Duration::from_secs(2),
        "physical output stream drop exceeded the shutdown deadline"
    );

    Ok(())
}
