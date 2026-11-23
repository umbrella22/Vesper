//! Packet ownership shared by the native source normalizer.

use super::*;

pub struct PacketSession {
    demuxer: PacketNormalizerSession,
    lease: Option<Lease>,
    next_lease_id: u64,
}

struct Lease {
    id: u64,
    packet: ffmpeg::Packet,
    time_base: ffmpeg::Rational,
}

/// Owns one FFmpeg packet buffer transferred out of the source slot.
pub struct PacketBytes(ffmpeg::Packet);

impl AsRef<[u8]> for PacketBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.data().unwrap_or_default()
    }
}

pub enum Read<'a> {
    Packet(Packet<'a>),
    TryAgain,
    EndOfStream,
}

pub struct Packet<'a> {
    pub lease_id: u64,
    pub stream_index: u32,
    pub pts_us: Option<i64>,
    pub dts_us: Option<i64>,
    pub duration_us: Option<i64>,
    pub key_frame: bool,
    pub data: &'a [u8],
}

impl PacketSession {
    pub fn open(
        config: &SourceNormalizerPacketSessionConfig,
    ) -> Result<Self, SourceNormalizerError> {
        Ok(Self {
            demuxer: PacketNormalizerSession::open(config)?,
            lease: None,
            next_lease_id: 1,
        })
    }

    pub fn get_stream_info(&self) -> &SourceNormalizerPacketStreamInfo {
        &self.demuxer.stream_info
    }

    /// A second read returns backpressure until the host releases the slot.
    pub fn read_packet(&mut self) -> Result<Read<'_>, SourceNormalizerError> {
        if self.lease.is_some() {
            return Ok(Read::TryAgain);
        }
        let video = self.demuxer.selected_video_stream_index;
        let audio = self.demuxer.selected_audio_stream_index;
        for _ in 0..=MAX_SKIPPED_NON_AV_PACKETS {
            let Some((stream, packet)) = self.demuxer.input.read_packet()? else {
                return Ok(Read::EndOfStream);
            };
            if packet.side_data().any(|data| {
                matches!(
                    data.kind(),
                    ffmpeg::codec::packet::side_data::Type::EncryptionInfo
                        | ffmpeg::codec::packet::side_data::Type::EncryptionInitInfo
                )
            }) {
                return Err(SourceNormalizerError::unsupported_operation(
                    "protected packet stream",
                ));
            }
            if stream.index() != video && Some(stream.index()) != audio {
                continue;
            }

            let id = self.next_lease_id;
            self.next_lease_id = id.checked_add(1).ok_or_else(|| {
                SourceNormalizerError::internal("exhausted probe packet lease IDs")
            })?;
            let stream_index = u32::try_from(stream.index())
                .map_err(|_| SourceNormalizerError::internal("probe stream index overflow"))?;
            let lease = self.lease.insert(Lease {
                id,
                packet,
                time_base: stream.time_base(),
            });
            let timestamp = |ts| timestamp_to_micros(ts, lease.time_base);
            return Ok(Read::Packet(Packet {
                lease_id: id,
                stream_index,
                pts_us: lease.packet.pts().and_then(timestamp),
                dts_us: lease.packet.dts().and_then(timestamp),
                duration_us: timestamp(lease.packet.duration()).filter(|duration| *duration > 0),
                key_frame: lease.packet.is_key(),
                data: lease.packet.data().unwrap_or_default(),
            }));
        }
        Err(SourceNormalizerError::internal(
            "probe exceeded the non-selected packet budget",
        ))
    }

    pub fn release_packet(&mut self, id: u64) -> Result<(), SourceNormalizerError> {
        self.take_packet(id).map(drop)
    }

    /// The Rust path can transfer the packet to the host for decoder retries.
    pub fn take_packet(&mut self, id: u64) -> Result<PacketBytes, SourceNormalizerError> {
        match self.lease.as_ref() {
            Some(lease) if lease.id == id => {
                self.lease.take().map(|lease| PacketBytes(lease.packet))
            }
            _ => None,
        }
        .ok_or_else(|| SourceNormalizerError::abi_violation("unknown probe packet lease"))
    }

    pub fn seek(&mut self, position_us: i64) -> Result<(), SourceNormalizerError> {
        if self.lease.is_some() {
            return Err(SourceNormalizerError::abi_violation(
                "release the outstanding probe packet before seek",
            ));
        }
        self.demuxer.input.seek_packet(position_us)
    }

    pub(crate) fn has_outstanding_packet(&self) -> bool {
        self.lease.is_some()
    }
}
