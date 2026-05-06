use std::time::Duration;

use anyhow::{Context, Result};

use crate::time::{duration_to_av_timestamp, timestamp_to_micros};
use crate::{CompressedVideoPacket, VideoPacketSource, VideoPacketStreamInfo};

impl VideoPacketSource {
    pub fn stream_info(&self) -> &VideoPacketStreamInfo {
        &self.stream_info
    }

    pub fn next_packet(&mut self) -> Result<Option<CompressedVideoPacket>> {
        for (stream, packet) in self.input.packets() {
            if stream.index() != self.stream_index {
                continue;
            }

            let data = packet.data().map(<[u8]>::to_vec).unwrap_or_default();
            let stream_index = u32::try_from(self.stream_index).unwrap_or(u32::MAX);
            return Ok(Some(CompressedVideoPacket {
                pts_us: packet
                    .pts()
                    .and_then(|timestamp| timestamp_to_micros(timestamp, self.time_base)),
                dts_us: packet
                    .dts()
                    .and_then(|timestamp| timestamp_to_micros(timestamp, self.time_base)),
                duration_us: timestamp_to_micros(packet.duration(), self.time_base)
                    .filter(|duration| *duration > 0),
                stream_index,
                key_frame: packet.is_key(),
                discontinuity: false,
                data,
            }));
        }

        Ok(None)
    }

    pub fn seek_to(&mut self, position: Duration) -> Result<()> {
        let timestamp = duration_to_av_timestamp(position);
        self.input.seek(timestamp, ..timestamp).with_context(|| {
            format!(
                "failed to seek video packet source to {:.3}s",
                position.as_secs_f64()
            )
        })
    }
}
