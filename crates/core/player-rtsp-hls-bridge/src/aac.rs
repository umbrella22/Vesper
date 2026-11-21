use crate::rtp::Packet;

pub(crate) const FRAME_SAMPLES: u32 = 1024;
const SAMPLE_RATES: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];
const MAX_AUS_PER_PACKET: usize = 32;

#[derive(Clone)]
pub(crate) struct Config {
    pub sample_rate: u32,
    pub channels: u16,
    pub specific: [u8; 2],
}

impl Config {
    pub fn parse(encoding: &str, fmtp: &str) -> Option<Self> {
        let mut fields = encoding.split('/');
        if !fields.next()?.eq_ignore_ascii_case("MPEG4-GENERIC") {
            return None;
        }
        let sample_rate = fields.next()?.parse().ok()?;
        let channels = fields.next()?.parse().ok()?;
        if fields.next().is_some() || !matches!(channels, 1 | 2) {
            return None;
        }
        let parameter = |name: &str| {
            fmtp.split(';')
                .filter_map(|field| field.trim().split_once('='))
                .find_map(|(key, value)| {
                    key.trim()
                        .eq_ignore_ascii_case(name)
                        .then_some(value.trim())
                })
        };
        if !parameter("mode")?.eq_ignore_ascii_case("AAC-hbr")
            || parameter("sizeLength")? != "13"
            || parameter("indexLength")? != "3"
            || parameter("indexDeltaLength")? != "3"
            || parameter("constantDuration").is_some_and(|duration| duration != "1024")
            || [
                "ctsDeltaLength",
                "dtsDeltaLength",
                "randomAccessIndication",
                "streamStateIndication",
                "auxiliaryDataSizeLength",
                "maxDisplacement",
            ]
            .into_iter()
            .any(|name| parameter(name).is_some_and(|value| value != "0"))
        {
            return None;
        }
        let mut specific = [0; 2];
        let value = parameter("config")?;
        // AAC-LC can carry a sync extension signaling SBR absent, as in
        // FFmpeg's 0x56e500 suffix. Keep only its two-byte LC specific config.
        let core = value.get(..4)?;
        if let Some(extension) = value.get(4..)
            && !extension.is_empty()
            && !extension.eq_ignore_ascii_case("56e500")
        {
            return None;
        }
        hex::decode_to_slice(core, &mut specific).ok()?;
        let asc = u16::from_be_bytes(specific);
        if asc >> 11 != 2
            || asc & 7 != 0
            || SAMPLE_RATES.get(usize::from((asc >> 7) & 15)) != Some(&sample_rate)
            || (asc >> 3) & 15 != channels
        {
            return None;
        }
        // The version-zero MP4 audio sample entry has a 16-bit integer rate.
        if sample_rate > u32::from(u16::MAX) {
            return None;
        }
        Some(Self {
            sample_rate,
            channels,
            specific,
        })
    }
}

pub(crate) struct Unit {
    pub timestamp: u32,
    pub bytes: Vec<u8>,
}

/// Complete, non-interleaved AAC-hbr access units carried by one RTP packet.
pub(crate) fn unpack(packet: &Packet<'_>) -> Option<Vec<Unit>> {
    let payload = packet.payload;
    let bits = u16::from_be_bytes(*payload.first_chunk::<2>()?);
    if bits == 0 || !bits.is_multiple_of(16) {
        return None;
    }
    let count = usize::from(bits / 16);
    if count > MAX_AUS_PER_PACKET {
        return None;
    }
    let (headers, mut data) = payload.get(2..)?.split_at_checked(count * 2)?;
    let mut timestamp = packet.timestamp;
    let mut units = Vec::with_capacity(count);
    for (index, bytes) in headers.as_chunks::<2>().0.iter().enumerate() {
        let header = u16::from_be_bytes(*bytes);
        let size = usize::from(header >> 3);
        let index_or_delta = u32::from(header & 7);
        if index != 0 {
            timestamp = timestamp.wrapping_add(FRAME_SAMPLES * (index_or_delta + 1));
        }
        let (unit, tail) = data.split_at_checked(size)?;
        let bytes = strip_adts(unit)?;
        if bytes.is_empty() {
            return None;
        }
        units.push(Unit {
            timestamp,
            bytes: bytes.to_vec(),
        });
        data = tail;
    }
    data.is_empty().then_some(units)
}

fn strip_adts(data: &[u8]) -> Option<&[u8]> {
    if !data
        .get(..2)
        .is_some_and(|header| header[0] == 0xff && header[1] & 0xf6 == 0xf0)
    {
        return Some(data);
    }
    let &[_, flags, _, length_high, length_middle, length_low, blocks] = data.first_chunk::<7>()?;
    let length = (usize::from(length_high & 3) << 11)
        | (usize::from(length_middle) << 3)
        | usize::from(length_low >> 5);
    if length != data.len() || blocks & 3 != 0 {
        return None;
    }
    data.get(if flags & 1 != 0 { 7 } else { 9 }..)
}

use std::collections::VecDeque;
use std::time::Duration;

use crate::mp4::{Sample, TIMESCALE};

const MAX_QUEUED_AUS: usize = 256;
const MAX_QUEUED_BYTES: usize = 1024 * 1024;

#[derive(Default)]
struct Clock {
    last: Option<(u32, i64)>,
}

impl Clock {
    fn track(&mut self, timestamp: u32, first_ticks: i64) -> Option<i64> {
        let ticks = match self.last {
            Some((previous, ticks)) => ticks + i64::from(timestamp.wrapping_sub(previous) as i32),
            None => first_ticks,
        };
        if self.last.is_some_and(|(_, previous)| ticks <= previous) {
            return None;
        }
        self.last = Some((timestamp, ticks));
        Some(ticks)
    }
}

pub(crate) struct Timeline {
    pub config: Config,
    video: Clock,
    audio: Clock,
    anchors: Option<(u32, u32)>,
    queue: VecDeque<(i64, Vec<u8>)>,
    bytes: usize,
    ssrc: Option<u32>,
}

impl Timeline {
    pub fn new(config: Config, anchors: Option<(u32, u32)>) -> Self {
        Self {
            config,
            video: Clock::default(),
            audio: Clock::default(),
            anchors,
            queue: VecDeque::new(),
            bytes: 0,
            ssrc: None,
        }
    }

    pub fn video_ticks(&mut self, timestamp: u32, arrival: Duration) -> Option<i64> {
        let initial = self.anchors.map_or_else(
            || duration_ticks(arrival, TIMESCALE),
            |(video, _)| i64::from(timestamp.wrapping_sub(video) as i32),
        );
        self.video.track(timestamp, initial)
    }

    pub fn push(&mut self, packet: &Packet<'_>, arrival: Duration) {
        if self.ssrc.is_some_and(|ssrc| ssrc != packet.ssrc) {
            return;
        }
        let Some(units) = unpack(packet) else {
            return;
        };
        self.ssrc = Some(packet.ssrc);
        for unit in units {
            let initial = self.anchors.map_or_else(
                || duration_ticks(arrival, self.config.sample_rate),
                |(_, audio)| i64::from(unit.timestamp.wrapping_sub(audio) as i32),
            );
            if let Some(ticks) = self.audio.track(unit.timestamp, initial) {
                self.bytes += unit.bytes.len();
                self.queue.push_back((ticks, unit.bytes));
                while self.queue.len() > MAX_QUEUED_AUS || self.bytes > MAX_QUEUED_BYTES {
                    if let Some((_, evicted)) = self.queue.pop_front() {
                        self.bytes -= evicted.len();
                    }
                }
            }
        }
    }

    pub fn fragment(
        &mut self,
        period_video: i64,
        start_video: i64,
        end_video: i64,
    ) -> Option<(u64, Vec<Sample>)> {
        let scale = |ticks| scale_ticks(ticks, self.config.sample_rate);
        let origin = scale(period_video);
        // A late packet from immediately before the boundary can be placed in
        // the next fragment with its original tfdt. Gaps retain their timing.
        let start = origin.max(scale(start_video) - i64::from(self.config.sample_rate / 8));
        let end = scale(end_video);
        let mut decode_start = None;
        let mut samples = Vec::new();
        while let Some((timestamp, _)) = self.queue.front() {
            let timestamp = *timestamp;
            if timestamp >= end {
                break;
            }
            if let Some(first) = decode_start
                && timestamp != first + i64::from(FRAME_SAMPLES) * samples.len() as i64
            {
                break;
            }
            let (_, bytes) = self.queue.pop_front()?;
            self.bytes -= bytes.len();
            if timestamp < start {
                continue;
            }
            decode_start.get_or_insert(timestamp);
            samples.push(Sample {
                duration: FRAME_SAMPLES,
                sync: true,
                bytes,
            });
        }
        Some((u64::try_from(decode_start? - origin).ok()?, samples))
    }
}

fn duration_ticks(duration: Duration, rate: u32) -> i64 {
    (duration.as_micros().saturating_mul(u128::from(rate)) / 1_000_000).min(i64::MAX as u128) as i64
}

fn scale_ticks(video_ticks: i64, rate: u32) -> i64 {
    (i128::from(video_ticks) * i128::from(rate) / i128::from(TIMESCALE)) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_aac_units_use_independent_timestamps_and_drop_adts_headers() {
        let mut bytes = vec![0, 32, 0, 64, 0, 8];
        bytes.extend([0xff, 0xf1, 0x4c, 0x40, 0x01, 0x1f, 0xfc, 0x59, 0x66]);
        // Eight-byte ADTS frame and one-byte raw AU.
        let packet = Packet {
            payload_type: 97,
            sequence: 2,
            timestamp: u32::MAX - 511,
            marker: true,
            ssrc: 5,
            payload: &bytes,
        };
        let units = unpack(&packet).expect("valid AAC packet");
        assert_eq!(units[0].bytes, [0x59]);
        assert_eq!(units[1].bytes, [0x66]);
        assert_eq!(units[1].timestamp, 512);
    }
}
