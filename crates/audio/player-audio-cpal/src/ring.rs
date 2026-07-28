pub(crate) const AUDIO_RING_CAPACITY_SECONDS: usize = 8;
pub(crate) const AUDIO_RING_MIN_CAPACITY_SAMPLES: usize = 16_384;
pub(crate) const AUDIO_RING_BLOCK_SAMPLES: usize = 1_024;

#[derive(Debug)]
pub(crate) struct AudioRingBlock {
    pub generation: u64,
    pub len: usize,
    pub samples: [f32; AUDIO_RING_BLOCK_SAMPLES],
}

impl AudioRingBlock {
    pub(crate) fn empty(generation: u64) -> Self {
        Self {
            generation,
            len: 0,
            samples: [0.0; AUDIO_RING_BLOCK_SAMPLES],
        }
    }
}

pub(crate) fn audio_ring_capacity_samples(sample_rate: u32, channels: usize) -> usize {
    (sample_rate as usize)
        .saturating_mul(channels.max(1))
        .saturating_mul(AUDIO_RING_CAPACITY_SECONDS)
        .max(AUDIO_RING_MIN_CAPACITY_SAMPLES)
}

pub(crate) fn audio_ring_block_capacity_samples(channels: usize) -> Option<usize> {
    if channels == 0 || channels > AUDIO_RING_BLOCK_SAMPLES {
        return None;
    }
    Some((AUDIO_RING_BLOCK_SAMPLES / channels) * channels)
}

pub(crate) fn audio_ring_capacity_blocks(sample_capacity: usize, channels: usize) -> Option<usize> {
    let block_capacity = audio_ring_block_capacity_samples(channels)?;
    Some(sample_capacity.div_ceil(block_capacity).saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::{
        AUDIO_RING_BLOCK_SAMPLES, AUDIO_RING_MIN_CAPACITY_SAMPLES,
        audio_ring_block_capacity_samples, audio_ring_capacity_blocks, audio_ring_capacity_samples,
    };

    #[test]
    fn capacity_uses_minimum_for_small_or_zero_inputs() {
        assert_eq!(
            audio_ring_capacity_samples(0, 0),
            AUDIO_RING_MIN_CAPACITY_SAMPLES
        );
        assert_eq!(
            audio_ring_capacity_samples(1, 1),
            AUDIO_RING_MIN_CAPACITY_SAMPLES
        );
    }

    #[test]
    fn capacity_scales_with_sample_rate_channels_and_seconds() {
        assert_eq!(audio_ring_capacity_samples(48_000, 2), 768_000);
    }

    #[test]
    fn block_capacity_preserves_complete_interleaved_frames() {
        assert_eq!(audio_ring_block_capacity_samples(2), Some(1_024));
        assert_eq!(audio_ring_block_capacity_samples(3), Some(1_023));
        assert_eq!(audio_ring_block_capacity_samples(0), None);
        assert_eq!(
            audio_ring_block_capacity_samples(AUDIO_RING_BLOCK_SAMPLES + 1),
            None
        );
    }

    #[test]
    fn queue_capacity_includes_pending_or_callback_block() {
        assert_eq!(audio_ring_capacity_blocks(2_048, 2), Some(3));
        assert_eq!(audio_ring_capacity_blocks(2_046, 3), Some(3));
    }
}
