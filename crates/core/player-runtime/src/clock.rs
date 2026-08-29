//! Playback clock abstractions shared by runtime adapters.

use std::time::{Duration, Instant};

/// Source of the current playback position.
pub trait MediaClock {
    /// Returns the media position represented by this clock.
    fn playback_position(&self) -> Duration;
}

/// Clock source currently used for playback synchronization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackClockSource {
    /// The rendered audio position is available and drives playback time.
    AudioMaster,
    /// No rendered audio position is available, so the host video position drives playback time.
    VideoFallback,
    /// Neither audio nor video has supplied a usable media position.
    Unavailable,
}

/// Host-observed A/V clock state for one playback generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackClockSnapshot {
    generation: u64,
    position: Option<Duration>,
    source: PlaybackClockSource,
    av_drift: Option<Duration>,
    drift_warning: bool,
}

impl PlaybackClockSnapshot {
    /// Returns the generation that produced this snapshot.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the selected master media position, when one is available.
    pub fn position(&self) -> Option<Duration> {
        self.position
    }

    /// Returns the host clock source selected for this snapshot.
    pub fn source(&self) -> PlaybackClockSource {
        self.source
    }

    /// Returns the observed audio/video difference when both positions are available.
    pub fn av_drift(&self) -> Option<Duration> {
        self.av_drift
    }

    /// Returns whether the observed A/V difference exceeds the configured limit.
    pub fn has_drift_warning(&self) -> bool {
        self.drift_warning
    }
}

/// Rejects a host clock sample that belongs to an inactive playback generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StalePlaybackClockGeneration {
    active_generation: u64,
    observed_generation: u64,
}

impl StalePlaybackClockGeneration {
    /// Returns the generation currently accepted by the clock.
    pub fn active_generation(&self) -> u64 {
        self.active_generation
    }

    /// Returns the stale generation carried by the rejected observation.
    pub fn observed_generation(&self) -> u64 {
        self.observed_generation
    }
}

impl std::fmt::Display for StalePlaybackClockGeneration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "playback clock observation for stale generation {} while {} is active",
            self.observed_generation, self.active_generation
        )
    }
}

impl std::error::Error for StalePlaybackClockGeneration {}

/// Chooses rendered audio as master and falls back to host-observed video time.
#[derive(Debug)]
pub struct PlaybackClockCoordinator {
    generation: u64,
    max_av_drift: Duration,
    audio_position: Option<Duration>,
    video_position: Option<Duration>,
}

impl PlaybackClockCoordinator {
    /// Creates a coordinator for an active generation and drift-warning threshold.
    pub fn new(generation: u64, max_av_drift: Duration) -> Self {
        Self {
            generation,
            max_av_drift,
            audio_position: None,
            video_position: None,
        }
    }

    /// Starts a new generation and discards observations from the previous one.
    pub fn begin_generation(&mut self, generation: u64) {
        self.generation = generation;
        self.audio_position = None;
        self.video_position = None;
    }

    /// Returns the generation currently accepted by the coordinator.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Records a host-observed video media position.
    pub fn observe_video(
        &mut self,
        generation: u64,
        position: Duration,
    ) -> Result<PlaybackClockSnapshot, StalePlaybackClockGeneration> {
        self.ensure_generation(generation)?;
        self.video_position = Some(position);
        Ok(self.snapshot())
    }

    /// Records rendered audio media time or declares the audio sink unavailable.
    pub fn observe_audio(
        &mut self,
        generation: u64,
        rendered_position: Option<Duration>,
    ) -> Result<PlaybackClockSnapshot, StalePlaybackClockGeneration> {
        self.ensure_generation(generation)?;
        self.audio_position = rendered_position;
        Ok(self.snapshot())
    }

    fn ensure_generation(&self, generation: u64) -> Result<(), StalePlaybackClockGeneration> {
        if generation == self.generation {
            Ok(())
        } else {
            Err(StalePlaybackClockGeneration {
                active_generation: self.generation,
                observed_generation: generation,
            })
        }
    }

    fn snapshot(&self) -> PlaybackClockSnapshot {
        let (position, source) = self
            .audio_position
            .map(|position| (position, PlaybackClockSource::AudioMaster))
            .or_else(|| {
                self.video_position
                    .map(|position| (position, PlaybackClockSource::VideoFallback))
            })
            .map_or(
                (None, PlaybackClockSource::Unavailable),
                |(position, source)| (Some(position), source),
            );
        let av_drift = self
            .audio_position
            .zip(self.video_position)
            .map(|(audio, video)| audio.abs_diff(video));

        PlaybackClockSnapshot {
            generation: self.generation,
            position,
            source,
            av_drift,
            drift_warning: av_drift.is_some_and(|drift| drift > self.max_av_drift),
        }
    }
}

/// Wall-clock-backed media clock with pause and playback-rate support.
#[derive(Debug)]
pub struct PlaybackClock {
    wall_start: Instant,
    media_start: Duration,
    playback_rate: f32,
    paused_at: Option<Instant>,
    paused_total: Duration,
}

impl PlaybackClock {
    /// Creates a clock starting at the provided media time.
    pub fn new(first_frame_time: Duration, playback_rate: f32) -> Self {
        Self {
            wall_start: Instant::now(),
            media_start: first_frame_time,
            playback_rate: sanitize_playback_rate(playback_rate),
            paused_at: None,
            paused_total: Duration::ZERO,
        }
    }

    /// Returns the current media position.
    pub fn playback_position(&self) -> Duration {
        <Self as MediaClock>::playback_position(self)
    }

    /// Returns the sanitized playback rate.
    pub fn playback_rate(&self) -> f32 {
        self.playback_rate
    }

    /// Freezes media position until [`resume`](Self::resume) is called.
    pub fn pause(&mut self) {
        if self.paused_at.is_none() {
            self.paused_at = Some(Instant::now());
        }
    }

    /// Resumes media position advancement after a pause.
    pub fn resume(&mut self) {
        if let Some(paused_at) = self.paused_at.take() {
            self.paused_total += Instant::now().saturating_duration_since(paused_at);
        }
    }
}

impl MediaClock for PlaybackClock {
    fn playback_position(&self) -> Duration {
        let elapsed = if let Some(paused_at) = self.paused_at {
            paused_at.saturating_duration_since(self.wall_start)
        } else {
            Instant::now().saturating_duration_since(self.wall_start)
        };

        self.media_start
            + Duration::from_secs_f64(
                elapsed.saturating_sub(self.paused_total).as_secs_f64()
                    * f64::from(self.playback_rate),
            )
    }
}

fn sanitize_playback_rate(playback_rate: f32) -> f32 {
    if playback_rate.is_finite() && playback_rate > 0.0 {
        playback_rate
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{PlaybackClock, PlaybackClockCoordinator, PlaybackClockSource};

    #[test]
    fn playback_clock_sanitizes_invalid_rate() {
        let clock = PlaybackClock::new(Duration::from_secs(1), f32::NAN);

        assert_eq!(clock.playback_rate(), 1.0);
    }

    #[test]
    fn rendered_audio_is_master_and_video_follows_with_drift_warning() {
        let mut clock = PlaybackClockCoordinator::new(7, Duration::from_millis(80));

        let video = clock.observe_video(7, Duration::from_secs(10)).unwrap();
        assert_eq!(video.source(), PlaybackClockSource::VideoFallback);

        let audio = clock
            .observe_audio(7, Some(Duration::from_secs(10)))
            .unwrap();
        assert_eq!(audio.source(), PlaybackClockSource::AudioMaster);
        assert_eq!(audio.position(), Some(Duration::from_secs(10)));

        let drifted = clock
            .observe_video(7, Duration::from_millis(10_200))
            .unwrap();
        assert_eq!(drifted.source(), PlaybackClockSource::AudioMaster);
        assert_eq!(drifted.position(), Some(Duration::from_secs(10)));
        assert!(drifted.has_drift_warning());
    }

    #[test]
    fn unavailable_audio_falls_back_to_video_and_rejects_stale_generation() {
        let mut clock = PlaybackClockCoordinator::new(3, Duration::from_millis(80));
        clock.observe_video(3, Duration::from_secs(4)).unwrap();
        clock
            .observe_audio(3, Some(Duration::from_secs(4)))
            .unwrap();

        let fallback = clock.observe_audio(3, None).unwrap();
        assert_eq!(fallback.source(), PlaybackClockSource::VideoFallback);
        assert_eq!(fallback.position(), Some(Duration::from_secs(4)));

        assert!(clock.observe_video(2, Duration::from_secs(2)).is_err());
        clock.begin_generation(4);
        assert!(
            clock
                .observe_audio(3, Some(Duration::from_secs(5)))
                .is_err()
        );
        assert_eq!(clock.generation(), 4);
    }

    #[test]
    fn unavailable_audio_without_video_does_not_claim_video_fallback() {
        let mut clock = PlaybackClockCoordinator::new(3, Duration::from_millis(80));

        let snapshot = clock.observe_audio(3, None).unwrap();

        assert_eq!(snapshot.source(), PlaybackClockSource::Unavailable);
        assert_eq!(snapshot.position(), None);
        assert_eq!(snapshot.av_drift(), None);
        assert!(!snapshot.has_drift_warning());
    }
}
