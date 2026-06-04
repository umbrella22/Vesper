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

    match output_config.sample_format {
        SampleFormat::F32 => device
            .build_output_stream(
                &output_config.stream_config,
                {
                    let state = state.clone();
                    move |data: &mut [f32], info| {
                        write_output_data(data, &mut consumer, &state, sample_rate, channels, info)
                    }
                },
                error_callback,
                None,
            )
            .context("failed to build f32 audio output stream"),
        SampleFormat::I16 => device
            .build_output_stream(
                &output_config.stream_config,
                {
                    let state = state.clone();
                    move |data: &mut [i16], info| {
                        write_output_data(data, &mut consumer, &state, sample_rate, channels, info)
                    }
                },
                error_callback,
                None,
            )
            .context("failed to build i16 audio output stream"),
        SampleFormat::U16 => device
            .build_output_stream(
                &output_config.stream_config,
                move |data: &mut [u16], info| {
                    write_output_data(data, &mut consumer, &state, sample_rate, channels, info)
                },
                error_callback,
                None,
            )
            .context("failed to build u16 audio output stream"),
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
