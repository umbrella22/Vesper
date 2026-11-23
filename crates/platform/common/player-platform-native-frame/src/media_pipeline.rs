use std::time::Duration;

use player_plugin_abi::{VESPER_PACKET_EOS, VesperPacket, VesperVideoFormat};
use player_plugin_loader::native::{
    NativeNormalizedPacket, NativePacketStream, NativePacketStreamPoll, NativeVideoDecoder,
    NativeVideoFrame, VideoDecoderOutput,
};

use super::NativeFramePipelineError;

/// An owned input packet. The byte storage is retained if a decoder cannot consume it yet.
#[derive(Debug)]
pub struct MediaPacket<T> {
    pub pts_us: Option<i64>,
    pub dts_us: Option<i64>,
    pub duration_us: Option<i64>,
    pub flags: u32,
    pub stream_index: u32,
    pub bytes: T,
}

pub enum MediaPacketPoll<T> {
    Packet(MediaPacket<T>),
    Pending,
    EndOfStream,
}

pub trait MediaPacketSource: Send {
    type Storage: AsRef<[u8]> + Send;

    fn next_video_packet(
        &mut self,
    ) -> Result<MediaPacketPoll<Self::Storage>, NativeFramePipelineError>;
    fn seek(&mut self, position: Duration) -> Result<(), NativeFramePipelineError>;
    fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
        Ok(())
    }
}

/// Selects video from a normalizer stream without copying its packet bytes.
pub struct NormalizedVideoPackets {
    stream: NativePacketStream,
    video_index: u32,
}

impl NormalizedVideoPackets {
    pub fn new(stream: NativePacketStream, video_index: u32) -> Self {
        Self {
            stream,
            video_index,
        }
    }
}

impl MediaPacketSource for NormalizedVideoPackets {
    type Storage = NativeNormalizedPacket;

    fn next_video_packet(
        &mut self,
    ) -> Result<MediaPacketPoll<Self::Storage>, NativeFramePipelineError> {
        const PACKET_STEPS_PER_TICK: usize = 8;
        let timestamp = |time| (time != i64::MIN).then_some(time);
        for _ in 0..PACKET_STEPS_PER_TICK {
            let bytes = match self.stream.read().map_err(packet_error)? {
                NativePacketStreamPoll::Packet(bytes) => bytes,
                NativePacketStreamPoll::Pending => return Ok(MediaPacketPoll::Pending),
                NativePacketStreamPoll::EndOfStream => return Ok(MediaPacketPoll::EndOfStream),
            };
            let raw = bytes.descriptor();
            if raw.stream_index == self.video_index {
                return Ok(MediaPacketPoll::Packet(MediaPacket {
                    pts_us: timestamp(raw.pts_us),
                    dts_us: timestamp(raw.dts_us),
                    duration_us: timestamp(raw.duration_us),
                    flags: raw.flags,
                    stream_index: raw.stream_index,
                    bytes,
                }));
            }
        }
        Ok(MediaPacketPoll::Pending)
    }

    fn seek(&mut self, position: Duration) -> Result<(), NativeFramePipelineError> {
        self.stream.seek(position).map_err(packet_error)
    }

    fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
        self.stream.flush().map_err(packet_error)
    }
}

fn packet_error(error: player_plugin::author::PluginError) -> NativeFramePipelineError {
    NativeFramePipelineError::new("nativeSourceNormalizer", error.to_string())
}

#[allow(
    clippy::large_enum_variant,
    reason = "Store the frame inline to avoid an extra allocation on every successful poll."
)]
pub enum NativeMediaVideoPoll {
    Frame {
        lease: NativeVideoFrame,
        format: VesperVideoFormat,
    },
    Pending,
    EndOfStream,
}

/// Drives the native packet-to-native-video route without retaining GPU completions.
/// A presenter owns each frame lease until the last GPU read. It can safely
/// complete a prior frame after seek or after dropping this pipeline.
pub struct NativeMediaPipeline<S: MediaPacketSource> {
    source: S,
    decoder: NativeVideoDecoder,
    packet: Option<MediaPacket<S::Storage>>,
    format: Option<VesperVideoFormat>,
    input_ended: bool,
    drain_sent: bool,
    output_ended: bool,
}

impl<S: MediaPacketSource> NativeMediaPipeline<S> {
    pub fn new(source: S, decoder: NativeVideoDecoder) -> Self {
        Self {
            source,
            decoder,
            packet: None,
            format: None,
            input_ended: false,
            drain_sent: false,
            output_ended: false,
        }
    }

    pub fn advance(&mut self) -> Result<NativeMediaVideoPoll, NativeFramePipelineError> {
        // Bound the work per host tick even with a long gap before the next frame.
        const DECODER_STEPS_PER_TICK: usize = 8;
        if self.output_ended {
            return Ok(NativeMediaVideoPoll::EndOfStream);
        }
        for _ in 0..DECODER_STEPS_PER_TICK {
            match self.decoder.receive().map_err(decoder_error)? {
                VideoDecoderOutput::FormatChanged(format) => self.format = Some(format),
                VideoDecoderOutput::Frame(lease) => {
                    let format = self.format.ok_or_else(|| {
                        NativeFramePipelineError::new(
                            "receiveVideo",
                            "missing negotiated video format",
                        )
                    })?;
                    return Ok(NativeMediaVideoPoll::Frame { lease, format });
                }
                VideoDecoderOutput::NeedMoreInput if !self.drain_sent => {
                    if !self.send_next()? {
                        return Ok(NativeMediaVideoPoll::Pending);
                    }
                }
                VideoDecoderOutput::EndOfStream => {
                    self.output_ended = true;
                    return Ok(NativeMediaVideoPoll::EndOfStream);
                }
                VideoDecoderOutput::NeedMoreInput | VideoDecoderOutput::TryAgain => {
                    return Ok(NativeMediaVideoPoll::Pending);
                }
            }
        }
        Ok(NativeMediaVideoPoll::Pending)
    }

    fn send_next(&mut self) -> Result<bool, NativeFramePipelineError> {
        if self.packet.is_none() && !self.input_ended {
            match self.source.next_video_packet()? {
                MediaPacketPoll::Packet(packet) => self.packet = Some(packet),
                MediaPacketPoll::Pending => return Ok(false),
                MediaPacketPoll::EndOfStream => self.input_ended = true,
            }
        }
        let packet = self.packet.as_ref();
        let time = |value: Option<i64>| value.unwrap_or(i64::MIN);
        let descriptor = match packet {
            Some(packet) => VesperPacket {
                pts_us: time(packet.pts_us),
                dts_us: time(packet.dts_us),
                duration_us: time(packet.duration_us),
                stream_index: packet.stream_index,
                flags: packet.flags,
                ..Default::default()
            },
            None => VesperPacket {
                pts_us: i64::MIN,
                dts_us: i64::MIN,
                duration_us: i64::MIN,
                flags: VESPER_PACKET_EOS,
                ..Default::default()
            },
        };
        let bytes = packet.map_or(&[][..], |packet| packet.bytes.as_ref());
        if !self
            .decoder
            .send_packet(descriptor, bytes)
            .map_err(decoder_error)?
        {
            return Ok(false);
        }
        self.drain_sent = packet.is_none();
        self.packet = None;
        Ok(true)
    }

    pub fn flush(&mut self) -> Result<(), NativeFramePipelineError> {
        self.packet = None;
        self.source.flush()?;
        self.reset_decoder()
    }

    pub fn seek(&mut self, position: Duration) -> Result<(), NativeFramePipelineError> {
        // Release backpressured packet storage before mutating its producer.
        self.packet = None;
        self.source.seek(position)?;
        self.reset_decoder()
    }

    fn reset_decoder(&mut self) -> Result<(), NativeFramePipelineError> {
        self.input_ended = false;
        self.drain_sent = false;
        self.output_ended = false;
        self.decoder.flush().map_err(decoder_error)
    }
}

fn decoder_error(error: player_plugin::author::PluginError) -> NativeFramePipelineError {
    NativeFramePipelineError::new("nativeVideoDecoder", error.to_string())
}
