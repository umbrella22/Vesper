pub(crate) const AUDIO_RING_CAPACITY_SECONDS: usize = 8;
pub(crate) const AUDIO_RING_MIN_CAPACITY_SAMPLES: usize = 16_384;
pub(crate) const STALE_DRAIN_MULTIPLIER: usize = 4;

#[derive(Debug, Clone, Copy)]
pub(crate) struct AudioRingSample {
    pub generation: u32,
    pub value: f32,
}

pub(crate) fn audio_ring_capacity_samples(sample_rate: u32, channels: usize) -> usize {
    (sample_rate as usize)
        .saturating_mul(channels.max(1))
        .saturating_mul(AUDIO_RING_CAPACITY_SECONDS)
        .max(AUDIO_RING_MIN_CAPACITY_SAMPLES)
}

pub(crate) fn ring_generation(generation: u64) -> u32 {
    generation as u32
}

#[cfg(test)]
mod tests {
    use super::{AUDIO_RING_MIN_CAPACITY_SAMPLES, audio_ring_capacity_samples, ring_generation};

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
    fn ring_generation_uses_low_bits_for_callback_samples() {
        assert_eq!(ring_generation(1), 1);
        assert_eq!(ring_generation(u64::from(u32::MAX) + 2), 1);
    }
}
