//! Playback clock abstractions shared by runtime adapters.

use std::time::{Duration, Instant};

pub trait MediaClock {
    fn playback_position(&self) -> Duration;
}

#[derive(Debug)]
pub struct PlaybackClock {
    wall_start: Instant,
    media_start: Duration,
    playback_rate: f32,
    paused_at: Option<Instant>,
    paused_total: Duration,
}

impl PlaybackClock {
    pub fn new(first_frame_time: Duration, playback_rate: f32) -> Self {
        Self {
            wall_start: Instant::now(),
            media_start: first_frame_time,
            playback_rate: sanitize_playback_rate(playback_rate),
            paused_at: None,
            paused_total: Duration::ZERO,
        }
    }

    pub fn playback_position(&self) -> Duration {
        <Self as MediaClock>::playback_position(self)
    }

    pub fn playback_rate(&self) -> f32 {
        self.playback_rate
    }

    pub fn pause(&mut self) {
        if self.paused_at.is_none() {
            self.paused_at = Some(Instant::now());
        }
    }

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

    use super::PlaybackClock;

    #[test]
    fn playback_clock_sanitizes_invalid_rate() {
        let clock = PlaybackClock::new(Duration::from_secs(1), f32::NAN);

        assert_eq!(clock.playback_rate(), 1.0);
    }
}
