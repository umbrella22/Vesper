use anyhow::{Context, Result};
use cpal::StreamConfig;
use cpal::traits::{DeviceTrait, HostTrait};

use crate::types::{AudioOutputConfig, AudioOutputDescriptor};

pub fn detect_default_output() -> AudioOutputDescriptor {
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        return AudioOutputDescriptor {
            default_output_device: None,
            default_output_config: None,
        };
    };

    let default_output_config = device.default_output_config().ok().map(|config| {
        let sample_format = config.sample_format();
        let stream_config: StreamConfig = config.into();

        AudioOutputConfig {
            channels: stream_config.channels,
            sample_rate: stream_config.sample_rate,
            sample_format,
            stream_config,
        }
    });

    let default_output_device = default_output_config
        .as_ref()
        .and_then(|_| device.description().ok())
        .map(|description| description.name().to_owned());

    AudioOutputDescriptor {
        default_output_device,
        default_output_config,
    }
}

pub fn default_output_config() -> Result<AudioOutputConfig> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("no default audio output device available")?;
    let config = device
        .default_output_config()
        .context("failed to query default audio output configuration")?;
    let sample_format = config.sample_format();
    let stream_config: StreamConfig = config.into();

    Ok(AudioOutputConfig {
        channels: stream_config.channels,
        sample_rate: stream_config.sample_rate,
        sample_format,
        stream_config,
    })
}
