use std::sync::Arc;

use anyhow::{Context, Result};
use cpal::traits::DeviceTrait;
use cpal::{FromSample, OutputCallbackInfo, Sample, SampleFormat, SizedSample, Stream};

use crate::ring::AudioRingBlock;
use crate::timeline::SharedPlaybackState;
use crate::types::AudioOutputConfig;

pub(crate) fn build_output_stream(
    device: &cpal::Device,
    output_config: &AudioOutputConfig,
    state: Arc<SharedPlaybackState>,
) -> Result<Stream> {
    let sample_rate = output_config.sample_rate;
    let channels = output_config.channels;

    macro_rules! build_stream {
        ($sample_type:ty, $context:literal) => {{
            let state = state.clone();
            let error_state = state.clone();
            let mut cursor = AudioCallbackCursor::default();
            device
                .build_output_stream(
                    output_config.stream_config.clone(),
                    move |data: &mut [$sample_type], info| {
                        write_output_data(data, &state, &mut cursor, sample_rate, channels, info)
                    },
                    move |_error| error_state.record_stream_error(),
                    None,
                )
                .context($context)
        }};
    }

    match output_config.sample_format {
        SampleFormat::I8 => build_stream!(i8, "failed to build i8 audio output stream"),
        SampleFormat::I16 => build_stream!(i16, "failed to build i16 audio output stream"),
        SampleFormat::I24 => build_stream!(cpal::I24, "failed to build i24 audio output stream"),
        SampleFormat::I32 => build_stream!(i32, "failed to build i32 audio output stream"),
        SampleFormat::I64 => build_stream!(i64, "failed to build i64 audio output stream"),
        SampleFormat::U8 => build_stream!(u8, "failed to build u8 audio output stream"),
        SampleFormat::U16 => build_stream!(u16, "failed to build u16 audio output stream"),
        SampleFormat::U24 => build_stream!(cpal::U24, "failed to build u24 audio output stream"),
        SampleFormat::U32 => build_stream!(u32, "failed to build u32 audio output stream"),
        SampleFormat::U64 => build_stream!(u64, "failed to build u64 audio output stream"),
        SampleFormat::F32 => build_stream!(f32, "failed to build f32 audio output stream"),
        SampleFormat::F64 => build_stream!(f64, "failed to build f64 audio output stream"),
        sample_format => anyhow::bail!("unsupported default audio sample format: {sample_format}"),
    }
}

#[derive(Debug, Default)]
struct AudioCallbackCursor {
    block: Option<AudioRingBlock>,
    offset: usize,
}

impl AudioCallbackCursor {
    fn next_sample(&mut self, state: &SharedPlaybackState, generation: u64) -> Option<f32> {
        loop {
            if state.generation() != generation {
                return None;
            }
            if let Some(block) = self.block.as_ref()
                && block.generation == generation
                && self.offset < block.len
            {
                let sample = block.samples[self.offset];
                self.offset += 1;
                if self.offset == block.len {
                    self.block = None;
                    self.offset = 0;
                }
                return Some(sample);
            }

            if self
                .block
                .as_ref()
                .is_some_and(|block| block.generation == state.generation())
            {
                return None;
            }

            self.block = state.pop_audio_block();
            self.offset = 0;
            self.block.as_ref()?;
        }
    }
}

fn write_output_data<T>(
    data: &mut [T],
    state: &SharedPlaybackState,
    cursor: &mut AudioCallbackCursor,
    sample_rate: u32,
    channels: u16,
    info: &OutputCallbackInfo,
) where
    T: Sample + SizedSample + FromSample<f32>,
{
    if state.is_paused() {
        fill_silence(data);
        return;
    }

    let mut counted_generation = None;
    let mut played = 0usize;

    for output in data.iter_mut() {
        let current_generation = state.generation();
        let Some(sample) = cursor.next_sample(state, current_generation) else {
            *output = T::EQUILIBRIUM;
            continue;
        };
        if counted_generation != Some(current_generation) {
            if let Some(generation) = counted_generation {
                state.mark_samples_played(generation, played);
            }
            counted_generation = Some(current_generation);
            played = 0;
        }
        *output = T::from_sample(sample);
        played = played.saturating_add(1);
    }

    if let Some(generation) = counted_generation {
        state.mark_samples_played(generation, played);
    }

    if let Some(generation) = counted_generation
        && state.is_generation_complete_and_drained(generation)
    {
        state.mark_generation_finished(generation);
    }

    let _ = (sample_rate, channels, info);
}

fn fill_silence<T>(data: &mut [T])
where
    T: Sample,
{
    for output in data {
        *output = T::EQUILIBRIUM;
    }
}
