use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_queue::ArrayQueue;

use crate::ring::{AudioRingBlock, audio_ring_block_capacity_samples};

const BACKPRESSURE_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);
pub(crate) const MAX_ACCOUNTED_AUDIO_SAMPLES: usize = i32::MAX as usize;
const PLAYED_SAMPLE_COUNT_MASK: u64 = i32::MAX as u64;
const FINISHED_GENERATION_MASK: u64 = 1 << 31;

fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

const fn pack_played_accounting(generation: u32, played_samples: u32) -> u64 {
    pack_played_accounting_state(generation, played_samples, false)
}

const fn pack_played_accounting_state(generation: u32, played_samples: u32, finished: bool) -> u64 {
    ((generation as u64) << 32)
        | ((played_samples as u64) & PLAYED_SAMPLE_COUNT_MASK)
        | if finished {
            FINISHED_GENERATION_MASK
        } else {
            0
        }
}

const fn played_generation(accounting: u64) -> u32 {
    (accounting >> 32) as u32
}

const fn played_sample_count(accounting: u64) -> u32 {
    (accounting & PLAYED_SAMPLE_COUNT_MASK) as u32
}

const fn played_generation_is_finished(accounting: u64) -> bool {
    accounting & FINISHED_GENERATION_MASK != 0
}

#[derive(Debug)]
pub(crate) struct PlaybackTimelineState {
    generation: u64,
    media_start: Duration,
    playback_rate: f32,
}

#[derive(Debug, Default)]
struct AudioQueueWriterState {
    pending: Option<AudioRingBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBufferWindowWaitResult {
    Ready,
    Inactive,
    Cancelled,
    TimedOut,
}

pub(crate) struct SharedPlaybackState {
    timeline: Mutex<PlaybackTimelineState>,
    queue: Arc<ArrayQueue<AudioRingBlock>>,
    queue_writer: Mutex<AudioQueueWriterState>,
    sample_capacity: usize,
    block_capacity: usize,
    backpressure_wait: Mutex<()>,
    backpressure_changed: Condvar,
    backpressure_sequence: AtomicU64,
    generation: AtomicU64,
    completed_generation: AtomicU64,
    queued_samples: AtomicUsize,
    played_accounting: AtomicU64,
    paused: AtomicBool,
    stream_error_sequence: AtomicU64,
    observed_stream_error_sequence: AtomicU64,
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
                &played_sample_count(self.played_accounting.load(Ordering::Relaxed)),
            )
            .field("paused", &self.paused.load(Ordering::Relaxed))
            .field(
                "finished",
                &played_generation_is_finished(self.played_accounting.load(Ordering::Relaxed)),
            )
            .finish()
    }
}

impl SharedPlaybackState {
    pub(crate) fn new(
        queue: Arc<ArrayQueue<AudioRingBlock>>,
        sample_capacity: usize,
        channels: u16,
        media_start: Duration,
        playback_rate: f32,
        start_paused: bool,
    ) -> Self {
        let block_capacity = audio_ring_block_capacity_samples(usize::from(channels)).unwrap_or(1);
        Self {
            timeline: Mutex::new(PlaybackTimelineState {
                generation: 0,
                media_start,
                playback_rate: sanitize_playback_rate(playback_rate),
            }),
            queue,
            queue_writer: Mutex::new(AudioQueueWriterState::default()),
            sample_capacity,
            block_capacity,
            backpressure_wait: Mutex::new(()),
            backpressure_changed: Condvar::new(),
            backpressure_sequence: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            completed_generation: AtomicU64::new(0),
            queued_samples: AtomicUsize::new(0),
            played_accounting: AtomicU64::new(pack_played_accounting(0, 0)),
            paused: AtomicBool::new(start_paused),
            stream_error_sequence: AtomicU64::new(0),
            observed_stream_error_sequence: AtomicU64::new(0),
        }
    }

    pub(crate) fn begin_generation(
        &self,
        channels: u16,
        media_start: Duration,
        playback_rate: f32,
    ) -> u64 {
        let mut queue_writer = lock_or_recover(&self.queue_writer);
        let mut timeline = lock_or_recover(&self.timeline);
        let mut next_generation = (timeline.generation as u32).wrapping_add(1);
        if next_generation == 0 {
            next_generation = 1;
        }
        timeline.generation = u64::from(next_generation);
        timeline.media_start = media_start;
        timeline.playback_rate = sanitize_playback_rate(playback_rate);
        let generation = timeline.generation;
        self.generation.store(generation, Ordering::Release);
        queue_writer.pending = None;
        while self.queue.pop().is_some() {}
        self.completed_generation.store(0, Ordering::Release);
        self.queued_samples.store(0, Ordering::Release);
        self.played_accounting.store(
            pack_played_accounting(next_generation, 0),
            Ordering::Release,
        );

        let _ = channels;
        drop(timeline);
        self.notify_backpressure_waiters();
        generation
    }

    pub(crate) fn append_samples(&self, generation: u64, samples: Vec<f32>) -> Result<bool> {
        let mut queue_writer = lock_or_recover(&self.queue_writer);
        let timeline = lock_or_recover(&self.timeline);
        if timeline.generation != generation {
            drop(timeline);
            self.notify_backpressure_waiters();
            return Ok(false);
        }
        drop(timeline);

        if self.generation.load(Ordering::Acquire) != generation {
            self.notify_backpressure_waiters();
            return Ok(false);
        }

        let buffered_samples = self
            .queued_samples
            .load(Ordering::Acquire)
            .saturating_sub(self.played_samples_for_generation(generation));
        if buffered_samples.saturating_add(samples.len()) > self.sample_capacity {
            anyhow::bail!(
                "audio output ring is full: {} samples requested, {} sample slots available",
                samples.len(),
                self.sample_capacity.saturating_sub(buffered_samples)
            );
        }

        let pending_len = queue_writer
            .pending
            .as_ref()
            .filter(|pending| pending.generation == generation)
            .map_or(0, |pending| pending.len);
        let blocks_to_publish = pending_len
            .saturating_add(samples.len())
            .checked_div(self.block_capacity)
            .unwrap_or(usize::MAX);
        let available_blocks = self.queue.capacity().saturating_sub(self.queue.len());
        if blocks_to_publish > available_blocks {
            anyhow::bail!(
                "audio output ring is full: {} blocks requested, {} slots available",
                blocks_to_publish,
                available_blocks
            );
        }

        let sample_count = samples.len();
        let mut offset = 0usize;
        while offset < samples.len() {
            let pending = queue_writer
                .pending
                .get_or_insert_with(|| AudioRingBlock::empty(generation));
            if pending.generation != generation {
                anyhow::bail!("audio output queue retained a stale pending generation");
            }
            let copy_len = self
                .block_capacity
                .saturating_sub(pending.len)
                .min(samples.len().saturating_sub(offset));
            pending.samples[pending.len..pending.len + copy_len]
                .copy_from_slice(&samples[offset..offset + copy_len]);
            pending.len += copy_len;
            offset += copy_len;

            if pending.len == self.block_capacity {
                let Some(full_block) = queue_writer.pending.take() else {
                    anyhow::bail!("audio output queue lost its pending block");
                };
                self.queue.push(full_block).map_err(|block| {
                    queue_writer.pending = Some(block);
                    anyhow::anyhow!("audio output queue became full while appending")
                })?;
            }
        }

        if self.generation.load(Ordering::Acquire) != generation {
            self.notify_backpressure_waiters();
            return Ok(false);
        }

        self.queued_samples
            .fetch_add(sample_count, Ordering::AcqRel);
        let generation_key = generation as u32;
        let _ = self.played_accounting.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |accounting| {
                (played_generation(accounting) == generation_key).then(|| {
                    pack_played_accounting_state(
                        generation_key,
                        played_sample_count(accounting),
                        false,
                    )
                })
            },
        );
        self.notify_backpressure_waiters();
        Ok(true)
    }

    pub(crate) fn finish_generation(&self, generation: u64) {
        let mut queue_writer = lock_or_recover(&self.queue_writer);
        let timeline = lock_or_recover(&self.timeline);
        if timeline.generation == generation {
            if let Some(pending) = queue_writer.pending.take()
                && pending.generation == generation
                && pending.len > 0
                && let Err(pending) = self.queue.push(pending)
            {
                queue_writer.pending = Some(pending);
                drop(timeline);
                drop(queue_writer);
                self.notify_backpressure_waiters();
                return;
            }
            self.completed_generation
                .store(generation, Ordering::Release);
            if self.is_generation_complete_and_drained(generation) {
                self.mark_generation_finished(generation);
            }
        }
        drop(timeline);
        self.notify_backpressure_waiters();
    }

    pub(crate) fn is_generation_active(&self, generation: u64) -> bool {
        self.timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .generation
            == generation
    }

    pub(crate) fn buffered_samples(&self, generation: u64) -> Option<usize> {
        let timeline = lock_or_recover(&self.timeline);
        if timeline.generation != generation {
            return None;
        }

        Some(
            self.queued_samples
                .load(Ordering::Acquire)
                .saturating_sub(self.played_samples_for_generation(generation)),
        )
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
            self.played_samples_for_generation(timeline.generation),
        )
    }

    pub(crate) fn is_generation_complete_and_drained(&self, generation: u64) -> bool {
        generation != 0
            && self.generation.load(Ordering::Acquire) == generation
            && self.completed_generation.load(Ordering::Acquire) == generation
            && self.played_samples_for_generation(generation)
                >= self.queued_samples.load(Ordering::Acquire)
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    pub(crate) fn is_finished(&self) -> bool {
        let generation = self.generation.load(Ordering::Acquire);
        let accounting = self.played_accounting.load(Ordering::Acquire);
        generation != 0
            && u64::from(played_generation(accounting)) == generation
            && played_generation_is_finished(accounting)
    }

    pub(crate) fn mark_generation_finished(&self, generation: u64) {
        let Ok(generation) = u32::try_from(generation) else {
            return;
        };
        if self
            .played_accounting
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |accounting| {
                (played_generation(accounting) == generation).then(|| {
                    pack_played_accounting_state(generation, played_sample_count(accounting), true)
                })
            })
            .is_ok()
        {
            self.notify_backpressure_waiters();
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn pop_audio_block(&self) -> Option<AudioRingBlock> {
        self.queue.pop()
    }

    pub(crate) fn mark_samples_played(&self, generation: u64, played: usize) {
        let Ok(generation) = u32::try_from(generation) else {
            return;
        };
        let played = u32::try_from(played)
            .unwrap_or(PLAYED_SAMPLE_COUNT_MASK as u32)
            .min(PLAYED_SAMPLE_COUNT_MASK as u32);
        if played > 0
            && self
                .played_accounting
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |accounting| {
                    (played_generation(accounting) == generation).then(|| {
                        pack_played_accounting(
                            generation,
                            played_sample_count(accounting)
                                .saturating_add(played)
                                .min(PLAYED_SAMPLE_COUNT_MASK as u32),
                        )
                    })
                })
                .is_ok()
        {
            self.notify_backpressure_waiters();
        }
    }

    fn played_samples_for_generation(&self, generation: u64) -> usize {
        let accounting = self.played_accounting.load(Ordering::Acquire);
        if u64::from(played_generation(accounting)) != generation {
            return 0;
        }
        played_sample_count(accounting) as usize
    }

    pub(crate) fn notify_backpressure_waiters(&self) {
        self.backpressure_sequence.fetch_add(1, Ordering::Release);
        self.backpressure_changed.notify_all();
    }

    pub(crate) fn record_stream_error(&self) {
        self.stream_error_sequence.fetch_add(1, Ordering::Release);
        self.notify_backpressure_waiters();
    }

    pub(crate) fn take_stream_error(&self) -> bool {
        let latest = self.stream_error_sequence.load(Ordering::Acquire);
        let observed = self
            .observed_stream_error_sequence
            .swap(latest, Ordering::AcqRel);
        latest != observed
    }

    pub(crate) fn wait_for_buffered_samples_at_or_below(
        &self,
        generation: u64,
        target_samples: usize,
        timeout: Duration,
        should_cancel: impl Fn() -> bool,
    ) -> AudioBufferWindowWaitResult {
        let deadline = Instant::now().checked_add(timeout);
        let mut backpressure_wait = lock_or_recover(&self.backpressure_wait);

        loop {
            if should_cancel() {
                return AudioBufferWindowWaitResult::Cancelled;
            }
            if !self.is_generation_active(generation) {
                return AudioBufferWindowWaitResult::Inactive;
            }
            if self
                .buffered_samples(generation)
                .is_some_and(|buffered| buffered <= target_samples)
            {
                return AudioBufferWindowWaitResult::Ready;
            }

            let Some(deadline) = deadline else {
                return AudioBufferWindowWaitResult::TimedOut;
            };
            let now = Instant::now();
            if now >= deadline {
                return AudioBufferWindowWaitResult::TimedOut;
            }

            let observed_sequence = self.backpressure_sequence.load(Ordering::Acquire);
            let remaining = deadline.saturating_duration_since(now);
            let wait_duration = remaining.min(BACKPRESSURE_WAIT_POLL_INTERVAL);
            let (next_backpressure_wait, _) = self
                .backpressure_changed
                .wait_timeout(backpressure_wait, wait_duration)
                .unwrap_or_else(|error| error.into_inner());
            backpressure_wait = next_backpressure_wait;
            if self.backpressure_sequence.load(Ordering::Acquire) != observed_sequence {
                continue;
            }
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
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    use std::time::Instant;

    use crossbeam_queue::ArrayQueue;

    use super::{
        AudioBufferWindowWaitResult, SharedPlaybackState, pack_played_accounting,
        played_generation, sanitize_playback_rate,
    };
    use crate::ring::{AudioRingBlock, audio_ring_capacity_blocks};

    fn state_with_capacity(capacity: usize) -> Arc<SharedPlaybackState> {
        let channels = 2u16;
        let queue_capacity = audio_ring_capacity_blocks(capacity, usize::from(channels))
            .expect("test channel count should fit an audio block");
        Arc::new(SharedPlaybackState::new(
            Arc::new(ArrayQueue::<AudioRingBlock>::new(queue_capacity)),
            capacity,
            channels,
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

        state.mark_samples_played(generation, 2);
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

        state.mark_samples_played(generation, 2);
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
        state.mark_samples_played(generation, 4);

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

    #[test]
    fn buffer_window_wait_returns_ready_when_already_below_target() {
        let state = state_with_capacity(16);
        let generation = state.begin_generation(2, Duration::ZERO, 1.0);

        let result = state.wait_for_buffered_samples_at_or_below(
            generation,
            0,
            Duration::from_secs(1),
            || false,
        );

        assert_eq!(result, AudioBufferWindowWaitResult::Ready);
    }

    #[test]
    fn buffer_window_wait_returns_inactive_after_generation_change() {
        let state = state_with_capacity(16);
        let first = state.begin_generation(2, Duration::ZERO, 1.0);
        state.begin_generation(2, Duration::ZERO, 1.0);

        let result =
            state.wait_for_buffered_samples_at_or_below(first, 0, Duration::from_secs(1), || false);

        assert_eq!(result, AudioBufferWindowWaitResult::Inactive);
    }

    #[test]
    fn buffer_window_wait_returns_cancelled_when_requested() {
        let state = state_with_capacity(16);
        let generation = state.begin_generation(2, Duration::ZERO, 1.0);

        let result = state.wait_for_buffered_samples_at_or_below(
            generation,
            0,
            Duration::from_secs(1),
            || true,
        );

        assert_eq!(result, AudioBufferWindowWaitResult::Cancelled);
    }

    #[test]
    fn buffer_window_wait_times_out_without_progress() {
        let state = state_with_capacity(16);
        let generation = state.begin_generation(2, Duration::ZERO, 1.0);
        state
            .append_samples(generation, vec![0.0, 0.1, 0.2, 0.3])
            .expect("append should succeed");

        let started = Instant::now();
        let result = state.wait_for_buffered_samples_at_or_below(
            generation,
            0,
            Duration::from_millis(5),
            || false,
        );

        assert_eq!(result, AudioBufferWindowWaitResult::TimedOut);
        assert!(started.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn buffer_window_wait_wakes_when_samples_are_played() {
        let state = state_with_capacity(16);
        let generation = state.begin_generation(2, Duration::ZERO, 1.0);
        state
            .append_samples(generation, vec![0.0, 0.1, 0.2, 0.3])
            .expect("append should succeed");

        let waiter_state = state.clone();
        let waiter = std::thread::spawn(move || {
            waiter_state.wait_for_buffered_samples_at_or_below(
                generation,
                0,
                Duration::from_secs(1),
                || false,
            )
        });

        std::thread::sleep(Duration::from_millis(10));
        state.mark_samples_played(generation, 4);

        assert_eq!(
            waiter.join().expect("waiter should not panic"),
            AudioBufferWindowWaitResult::Ready
        );
    }

    #[test]
    fn generation_switch_drains_old_samples_before_accepting_new_samples() {
        let state = state_with_capacity(4);
        let first = state.begin_generation(2, Duration::ZERO, 1.0);
        assert!(
            state
                .append_samples(first, vec![0.0, 0.1, 0.2, 0.3])
                .expect("first generation should fill the queue")
        );

        let second = state.begin_generation(2, Duration::from_secs(1), 1.0);
        assert!(
            state
                .append_samples(second, vec![1.0, 1.1, 1.2, 1.3])
                .expect("new generation should reuse the full queue")
        );
        state.finish_generation(second);

        let block = state
            .pop_audio_block()
            .expect("new generation block should exist");
        assert_eq!(block.generation, second);
        assert_eq!(block.len, 4);
        assert_eq!(&block.samples[..block.len], &[1.0, 1.1, 1.2, 1.3]);
    }

    #[test]
    fn late_played_count_does_not_advance_new_generation() {
        let state = state_with_capacity(4);
        let first = state.begin_generation(2, Duration::ZERO, 1.0);
        state
            .append_samples(first, vec![0.0, 0.1])
            .expect("first generation append should succeed");
        let second = state.begin_generation(2, Duration::ZERO, 1.0);
        state
            .append_samples(second, vec![1.0, 1.1])
            .expect("second generation append should succeed");

        state.mark_samples_played(first, 2);

        assert_eq!(state.buffered_samples(second), Some(2));
        assert_eq!(state.playback_position(1_000, 2), Duration::ZERO);
    }

    #[test]
    fn callback_notification_does_not_acquire_wait_mutex() {
        let state = state_with_capacity(4);
        let _wait_guard = state
            .backpressure_wait
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let before = state.backpressure_sequence.load(Ordering::Acquire);

        state.notify_backpressure_waiters();

        assert_eq!(
            state.backpressure_sequence.load(Ordering::Acquire),
            before + 1
        );
    }

    #[test]
    fn stream_errors_are_consumed_once_per_observed_sequence() {
        let state = state_with_capacity(4);
        assert!(!state.take_stream_error());

        state.record_stream_error();
        assert!(state.take_stream_error());
        assert!(!state.take_stream_error());

        state.record_stream_error();
        state.record_stream_error();
        assert!(state.take_stream_error());
        assert!(!state.take_stream_error());
    }

    #[test]
    fn stale_accounting_compare_exchange_cannot_update_new_generation() {
        let state = state_with_capacity(4);
        let first = state.begin_generation(2, Duration::ZERO, 1.0);
        let stale_accounting = state.played_accounting.load(Ordering::Acquire);
        let second = state.begin_generation(2, Duration::ZERO, 1.0);

        assert_eq!(u64::from(played_generation(stale_accounting)), first);
        assert!(
            state
                .played_accounting
                .compare_exchange(
                    stale_accounting,
                    pack_played_accounting(first as u32, 2),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
        );
        assert_eq!(state.played_samples_for_generation(second), 0);
    }

    #[test]
    fn stale_finished_token_cannot_finish_new_generation() {
        let state = state_with_capacity(4);
        let first = state.begin_generation(2, Duration::ZERO, 1.0);
        let second = state.begin_generation(2, Duration::ZERO, 1.0);

        state.mark_generation_finished(first);

        assert!(!state.is_finished());
        state.mark_generation_finished(second);
        assert!(state.is_finished());
    }
}
