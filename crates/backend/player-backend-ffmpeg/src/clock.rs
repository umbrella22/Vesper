use std::time::Duration;

/// Selects the media position that should drive A/V presentation.
pub trait MasterClock {
    /// Returns the current master media position.
    fn playback_position(
        &self,
        audio_position: Option<Duration>,
        video_position: Option<Duration>,
    ) -> Option<Duration>;
}

/// Master clock strategy that keeps audio as the synchronization source.
#[derive(Debug, Clone, Copy, Default)]
pub struct AudioMasterClock;

impl AudioMasterClock {
    pub const fn new() -> Self {
        Self
    }
}

impl MasterClock for AudioMasterClock {
    fn playback_position(
        &self,
        audio_position: Option<Duration>,
        video_position: Option<Duration>,
    ) -> Option<Duration> {
        match (audio_position, video_position) {
            (Some(audio_position), _) => Some(audio_position),
            (None, Some(video_position)) => Some(video_position),
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{AudioMasterClock, MasterClock};

    #[test]
    fn audio_master_clock_prefers_audio_position() {
        let selected = AudioMasterClock::new().playback_position(
            Some(Duration::from_millis(0)),
            Some(Duration::from_millis(600)),
        );

        assert_eq!(selected, Some(Duration::from_millis(0)));
    }

    #[test]
    fn audio_master_clock_falls_back_to_video_position() {
        let selected =
            AudioMasterClock::new().playback_position(None, Some(Duration::from_millis(600)));

        assert_eq!(selected, Some(Duration::from_millis(600)));
    }
}
