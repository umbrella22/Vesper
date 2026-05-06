use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationState {
    Ready,
    Playing,
    Paused,
    Finished,
}

#[derive(Debug, Clone, Copy)]
pub struct PlaybackProgress {
    position: Duration,
    duration: Option<Duration>,
    ratio: Option<f64>,
}

impl PlaybackProgress {
    pub fn new(position: Duration, duration: Option<Duration>) -> Self {
        let ratio = duration
            .filter(|duration| !duration.is_zero())
            .map(|duration| position.as_secs_f64() / duration.as_secs_f64())
            .map(|ratio| ratio.clamp(0.0, 1.0));

        Self {
            position,
            duration,
            ratio,
        }
    }

    pub fn position(&self) -> Duration {
        self.position
    }

    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    pub fn ratio(&self) -> Option<f64> {
        self.ratio
    }
}

#[derive(Debug, Clone)]
pub struct PlaybackSessionModel {
    duration: Option<Duration>,
    frame_interval: Duration,
    started: bool,
    paused: bool,
    finished: bool,
}

impl PlaybackSessionModel {
    pub fn new(duration: Option<Duration>, frame_rate: Option<f64>) -> Self {
        let frame_interval = frame_rate
            .filter(|frame_rate| *frame_rate > 0.0)
            .map(|frame_rate| Duration::from_secs_f64(1.0 / frame_rate))
            .unwrap_or(Duration::from_millis(33));

        Self {
            duration,
            frame_interval,
            started: false,
            paused: false,
            finished: false,
        }
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn should_hold_output(&self) -> bool {
        !self.started || self.paused
    }

    pub fn start_or_resume(&mut self) {
        self.started = true;
        self.paused = false;
    }

    pub fn pause_playback(&mut self) {
        if self.started {
            self.paused = true;
        }
    }

    pub fn reset_to_ready(&mut self) {
        self.started = false;
        self.paused = false;
        self.finished = false;
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.started = true;
        self.paused = paused;
    }

    pub fn toggle_pause(&mut self) -> bool {
        if !self.started {
            self.started = true;
            self.paused = false;
            return self.paused;
        }

        self.paused = !self.paused;
        self.paused
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn set_finished(&mut self, finished: bool) {
        self.finished = finished;
    }

    pub fn sync_finished(&mut self, video_finished: bool, audio_finished: bool) -> bool {
        let finished = video_finished && audio_finished;
        let changed = self.finished != finished;
        self.finished = finished;
        changed
    }

    pub fn progress(&self, position: Duration) -> PlaybackProgress {
        let clamped_position = self
            .duration
            .map(|duration| position.min(duration))
            .unwrap_or(position);

        PlaybackProgress::new(clamped_position, self.duration)
    }

    pub fn clamp_seek_position(&self, position: Duration) -> Duration {
        let Some(duration) = self.duration else {
            return position;
        };

        if duration > self.frame_interval {
            position.min(duration.saturating_sub(self.frame_interval))
        } else {
            Duration::ZERO
        }
    }

    pub fn presentation_state(&self) -> PresentationState {
        if self.finished {
            PresentationState::Finished
        } else if !self.started {
            PresentationState::Ready
        } else if self.paused {
            PresentationState::Paused
        } else {
            PresentationState::Playing
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_session_model_transitions_between_ready_playing_paused_and_finished() {
        let mut session = PlaybackSessionModel::new(Some(Duration::from_secs(10)), Some(30.0));

        assert_eq!(session.presentation_state(), PresentationState::Ready);
        assert!(session.should_hold_output());

        session.start_or_resume();
        assert_eq!(session.presentation_state(), PresentationState::Playing);
        assert!(!session.should_hold_output());

        session.pause_playback();
        assert_eq!(session.presentation_state(), PresentationState::Paused);
        assert!(session.should_hold_output());

        session.start_or_resume();
        assert_eq!(session.presentation_state(), PresentationState::Playing);

        session.sync_finished(true, true);
        assert_eq!(session.presentation_state(), PresentationState::Finished);

        session.reset_to_ready();
        assert_eq!(session.presentation_state(), PresentationState::Ready);
        assert!(session.should_hold_output());
    }

    #[test]
    fn playback_session_model_clamps_seek_before_terminal_frame_boundary() {
        let session = PlaybackSessionModel::new(Some(Duration::from_secs(5)), Some(25.0));

        let clamped = session.clamp_seek_position(Duration::from_secs(5));

        assert_eq!(clamped, Duration::from_secs_f64(5.0 - 1.0 / 25.0));
    }

    #[test]
    fn playback_session_progress_clamps_to_declared_duration() {
        let session = PlaybackSessionModel::new(Some(Duration::from_secs(5)), Some(30.0));

        let progress = session.progress(Duration::from_secs(9));

        assert_eq!(progress.position(), Duration::from_secs(5));
        assert_eq!(progress.duration(), Some(Duration::from_secs(5)));
        assert_eq!(progress.ratio(), Some(1.0));
    }
}
