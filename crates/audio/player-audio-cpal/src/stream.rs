use std::sync::Arc;

use anyhow::{Context, Result};
use cpal::traits::DeviceTrait;
use cpal::{FromSample, OutputCallbackInfo, Sample, SampleFormat, SizedSample, Stream};
use rtrb::Consumer;

use crate::ring::{AudioRingSample, STALE_DRAIN_MULTIPLIER};
use crate::timeline::SharedPlaybackState;
use crate::types::AudioOutputConfig;

pub(crate) fn build_output_stream(
    device: &cpal::Device,
    output_config: &AudioOutputConfig,
    mut consumer: Consumer<AudioRingSample>,
    state: Arc<SharedPlaybackState>,
) -> Result<Stream> {
    let error_callback = |error| eprintln!("audio output stream error: {error}");
    let sample_rate = output_config.sample_rate;
    let channels = output_config.channels;

    macro_rules! build_stream {
        ($sample_type:ty, $context:literal) => {{
            let state = state.clone();
            device
                .build_output_stream(
                    output_config.stream_config.clone(),
                    move |data: &mut [$sample_type], info| {
                        write_output_data(data, &mut consumer, &state, sample_rate, channels, info)
                    },
                    error_callback,
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

pub(crate) fn write_output_data<T>(
    data: &mut [T],
    consumer: &mut Consumer<AudioRingSample>,
    state: &SharedPlaybackState,
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

    let current_generation = state.ring_generation();
    let max_pops = data.len().saturating_mul(STALE_DRAIN_MULTIPLIER).max(1);
    let mut written = 0usize;
    let mut popped = 0usize;
    let mut played = 0usize;

    while written < data.len() && popped < max_pops {
        let Ok(sample) = consumer.pop() else {
            break;
        };
        popped = popped.saturating_add(1);
        if sample.generation != current_generation {
            continue;
        }

        data[written] = T::from_sample(sample.value);
        written = written.saturating_add(1);
        played = played.saturating_add(1);
    }

    state.mark_samples_played(played);

    for output in &mut data[written..] {
        *output = T::EQUILIBRIUM;
    }

    if state.is_current_generation_complete_and_drained() {
        state.set_finished(true);
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
