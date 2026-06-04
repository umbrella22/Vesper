use cpal::{SampleFormat, StreamConfig};

#[derive(Debug, Clone)]
pub struct AudioOutputConfig {
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_format: SampleFormat,
    pub stream_config: StreamConfig,
}

#[derive(Debug, Clone)]
pub struct AudioOutputDescriptor {
    pub default_output_device: Option<String>,
    pub default_output_config: Option<AudioOutputConfig>,
}
