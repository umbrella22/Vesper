use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Result;
use rtrb::Producer;

use crate::ring::{AudioRingSample, ring_generation};

#[derive(Debug)]
pub(crate) struct PlaybackTimelineState {
    generation: u64,
    media_start: Duration,
    playback_rate: f32,
}

pub(crate) struct SharedPlaybackState {
    timeline: Mutex<PlaybackTimelineState>,
    producer: Mutex<Producer<AudioRingSample>>,
    generation: AtomicU64,
    ring_generation: AtomicU32,
    completed_generation: AtomicU64,
    queued_samples: AtomicUsize,
    played_samples: AtomicUsize,
    paused: AtomicBool,
    finished: AtomicBool,
}

impl std::fmt::Debug for SharedPlaybackState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedPlaybackState")
            .field("generation", &self.generation.load(Ordering::Relaxed))
            .field(
                "completed_generation",
                &self.completed_generation.load(Ordering::Relaxed),
            )
            .field(
                "queued_samples",
                &self.queued_samples.load(Ordering::Relaxed),
            )
            .field(
                "played_samples",
                &self.played_samples.load(Ordering::Relaxed),
            )
            .field("paused", &self.paused.load(Ordering::Relaxed))
            .field("finished", &self.finished.load(Ordering::Relaxed))
            .finish()
    }
}

impl SharedPlaybackState {
    pub(crate) fn new(
        producer: Producer<AudioRingSample>,
        media_start: Duration,
        playback_rate: f32,
        start_paused: bool,
    ) -> Self {
        Self {
            timeline: Mutex::new(PlaybackTimelineState {
                generation: 0,
                media_start,
                playback_rate: sanitize_playback_rate(playback_rate),
            }),
            producer: Mutex::new(producer),
            generation: AtomicU64::new(0),
            ring_generation: AtomicU32::new(0),
            completed_generation: AtomicU64::new(0),
            queued_samples: AtomicUsize::new(0),
            played_samples: AtomicUsize::new(0),
            paused: AtomicBool::new(start_paused),
            finished: AtomicBool::new(false),
        }
    }

    pub(crate) fn begin_generation(
        &self,
        channels: u16,
        media_start: Duration,
        playback_rate: f32,
    ) -> u64 {
        let mut generation = 0u64;
        if let Ok(mut timeline) = self.timeline.lock() {
            timeline.generation = timeline.generation.saturating_add(1);
            timeline.media_start = media_start;
            timeline.playback_rate = sanitize_playback_rate(playback_rate);
            generation = timeline.generation;
            self.generation.store(generation, Ordering::Release);
            self.ring_generation
                .store(ring_generation(generation), Ordering::Release);
            self.completed_generation.store(0, Ordering::Release);
            self.queued_samples.store(0, Ordering::Release);
            self.played_samples.store(0, Ordering::Release);
        }

        let _ = channels;
        self.finished.store(false, Ordering::SeqCst);
        generation
    }

    pub(crate) fn append_samples(&self, generation: u64, samples: Vec<f32>) -> Result<bool> {
        let Ok(timeline) = self.timeline.lock() else {
            anyhow::bail!("audio playback timeline lock is poisoned");
        };
        if timeline.generation != generation {
            return Ok(false);
        }
        let ring_generation = ring_generation(generation);
        drop(timeline);

        if self.generation.load(Ordering::Acquire) != generation {
            return Ok(false);
        }

        let mut producer = self
            .producer
            .lock()
            .map_err(|_| anyhow::anyhow!("audio output ring producer lock is poisoned"))?;
        let available_slots = producer.slots();
        if available_slots < samples.len() {
            anyhow::bail!(
                "audio output ring is full: {} samples requested, {} slots available",
                samples.len(),
                available_slots
            );
        }

        let sample_count = samples.len();
        let chunk = producer
            .write_chunk_uninit(sample_count)
            .map_err(|error| anyhow::anyhow!("audio output ring write failed: {error}"))?;
        let written = chunk.fill_from_iter(samples.into_iter().map(|value| AudioRingSample {
            generation: ring_generation,
            value,
        }));
        if written != sample_count {
            anyhow::bail!(
                "audio output ring accepted {} of {} samples",
                written,
                sample_count
            );
        }

        if self.generation.load(Ordering::Acquire) != generation {
            return Ok(false);
        }

        self.queued_samples
            .fetch_add(sample_count, Ordering::AcqRel);
        self.finished.store(false, Ordering::SeqCst);
        Ok(true)
    }

    pub(crate) fn finish_generation(&self, generation: u64) {
        if let Ok(timeline) = self.timeline.lock()
            && timeline.generation == generation
        {
            self.completed_generation
                .store(generation, Ordering::Release);
            if self.is_current_generation_complete_and_drained() {
                self.finished.store(true, Ordering::SeqCst);
            }
        }
    }

    pub(crate) fn is_generation_active(&self, generation: u64) -> bool {
        self.timeline
            .lock()
            .map(|timeline| timeline.generation == generation)
            .unwrap_or(false)
    }

    pub(crate) fn buffered_samples(&self, generation: u64) -> Option<usize> {
        self.timeline.lock().ok().and_then(|timeline| {
            if timeline.generation != generation {
                return None;
            }

            Some(
                self.queued_samples
                    .load(Ordering::Acquire)
                    .saturating_sub(self.played_samples.load(Ordering::Acquire)),
            )
        })
    }

    pub(crate) fn playback_rate(&self) -> f32 {
        self.timeline
            .lock()
            .map(|timeline| timeline.playback_rate)
            .unwrap_or(1.0)
    }

    pub(crate) fn playback_position(&self, sample_rate: u32, channels: u16) -> Duration {
        let channels = usize::from(channels.max(1));
        let Ok(timeline) = self.timeline.lock() else {
            return Duration::ZERO;
        };
        media_time_for_sample_offset(
            timeline.media_start,
            timeline.playback_rate,
            sample_rate,
            channels,
            self.played_samples.load(Ordering::Acquire),
        )
    }

    pub(crate) fn is_current_generation_complete_and_drained(&self) -> bool {
        let generation = self.generation.load(Ordering::Acquire);
        generation != 0
            && self.completed_generation.load(Ordering::Acquire) == generation
            && self.played_samples.load(Ordering::Acquire)
                >= self.queued_samples.load(Ordering::Acquire)
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    pub(crate) fn set_finished(&self, finished: bool) {
        self.finished.store(finished, Ordering::SeqCst);
    }

    pub(crate) fn ring_generation(&self) -> u32 {
        self.ring_generation.load(Ordering::Acquire)
    }

    pub(crate) fn mark_samples_played(&self, played: usize) {
        if played > 0 {
            self.played_samples.fetch_add(played, Ordering::AcqRel);
            self.finished.store(false, Ordering::SeqCst);
        }
    }
}

fn duration_from_frames(frames: u64, sample_rate: u32) -> Duration {
    if sample_rate == 0 {
        return Duration::ZERO;
    }

    Duration::from_secs_f64((frames as f64) / f64::from(sample_rate))
}

fn media_time_for_sample_offset(
    media_start: Duration,
    playback_rate: f32,
    sample_rate: u32,
    channels: usize,
    sample_offset: usize,
) -> Duration {
    let frame_offset = sample_offset / channels.max(1);
    media_start
        + Duration::from_secs_f64(
            duration_from_frames(frame_offset as u64, sample_rate).as_secs_f64()
                * f64::from(playback_rate),
        )
}

pub(crate) fn sanitize_playback_rate(playback_rate: f32) -> f32 {
    if playback_rate.is_finite() && playback_rate > 0.0 {
        playback_rate
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use rtrb::RingBuffer;

    use super::{SharedPlaybackState, sanitize_playback_rate};
    use crate::ring::AudioRingSample;

    fn state_with_capacity(capacity: usize) -> Arc<SharedPlaybackState> {
        let (producer, _consumer) = RingBuffer::<AudioRingSample>::new(capacity);
        Arc::new(SharedPlaybackState::new(
            producer,
            Duration::from_millis(0),
            1.0,
            true,
        ))
    }

    #[test]
    fn begin_generation_resets_accounting_and_sanitizes_rate() {
        let state = state_with_capacity(16);
        let generation = state.begin_generation(2, Duration::from_millis(250), f32::NAN);

        assert_eq!(generation, 1);
        assert_eq!(state.playback_rate(), 1.0);
        assert_eq!(state.buffered_samples(generation), Some(0));
        assert!(state.is_generation_active(generation));
    }

    #[test]
    fn append_samples_tracks_buffered_samples_for_active_generation() {
        let state = state_with_capacity(16);
        let generation = state.begin_generation(2, Duration::ZERO, 1.0);

        assert!(
            state
                .append_samples(generation, vec![0.0, 0.1, 0.2, 0.3])
                .unwrap()
        );
        assert_eq!(state.buffered_samples(generation), Some(4));

        state.mark_samples_played(2);
        assert_eq!(state.buffered_samples(generation), Some(2));
    }

    #[test]
    fn append_samples_rejects_stale_generation_without_counting() {
        let state = state_with_capacity(16);
        let first = state.begin_generation(2, Duration::ZERO, 1.0);
        let second = state.begin_generation(2, Duration::ZERO, 1.0);

        assert!(!state.append_samples(first, vec![0.0, 0.1]).unwrap());
        assert_eq!(state.buffered_samples(first), None);
        assert_eq!(state.buffered_samples(second), Some(0));
    }

    #[test]
    fn finish_generation_marks_drained_generation_finished() {
        let state = state_with_capacity(16);
        let generation = state.begin_generation(2, Duration::ZERO, 1.0);
        state.append_samples(generation, vec![0.0, 0.1]).unwrap();

        state.finish_generation(generation);
        assert!(!state.is_finished());

        state.mark_samples_played(2);
        state.finish_generation(generation);
        assert!(state.is_finished());
    }

    #[test]
    fn playback_position_uses_frames_channels_and_rate() {
        let state = state_with_capacity(16);
        let generation = state.begin_generation(2, Duration::from_secs(1), 2.0);
        state
            .append_samples(generation, vec![0.0, 0.1, 0.2, 0.3])
            .unwrap();
        state.mark_samples_played(4);

        assert_eq!(
            state.playback_position(1_000, 2),
            Duration::from_millis(1_004)
        );
    }

    #[test]
    fn invalid_playback_rates_fall_back_to_normal_speed() {
        assert_eq!(sanitize_playback_rate(0.0), 1.0);
        assert_eq!(sanitize_playback_rate(-1.0), 1.0);
        assert_eq!(sanitize_playback_rate(f32::INFINITY), 1.0);
        assert_eq!(sanitize_playback_rate(1.25), 1.25);
    }
}
