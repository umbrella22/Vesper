use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use player_hls_playlist::{
    DurationPrecision, Initialization, MediaPlaylistOptions, MediaSegment, PlaylistKind,
    write_media_playlist,
};

use crate::BridgeError as Error;
use crate::mp4::{self, Sample, TIMESCALE};
use crate::rtp::{AccessUnit, VideoConfig};

const TARGET_DURATION: u32 = 2;
const MIN_SEGMENT_TICKS: u32 = 3 * TIMESCALE / 4;
const PREFERRED_SEGMENT_TICKS: u32 = TARGET_DURATION * TIMESCALE;
// EXTINF is serialized to microseconds and then rounded to whole seconds.
const MAX_SEGMENT_TICKS: u32 = PREFERRED_SEGMENT_TICKS + TIMESCALE / 2 - 1;
const MAX_SAMPLE_INTERVAL_TICKS: u32 = 2 * TIMESCALE;
const MIN_WINDOW_TICKS: u32 = 3 * TARGET_DURATION * TIMESCALE;
const MAX_SEGMENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_SEGMENT_SAMPLES: usize = 512;
const MAX_RETAINED_BYTES: usize = 48 * 1024 * 1024;
const MAX_RETAINED_SEGMENTS: usize = 48;
// A retired segment remains available for its own duration plus the longest
// advertised window (less than 8.5 seconds at this target duration).
const RETIREMENT_GRACE: Duration = Duration::from_secs(11);

pub(crate) struct Segment {
    sequence: u64,
    duration: u32,
    init: Arc<Init>,
    bytes: Arc<[u8]>,
    discontinuity: bool,
}

struct Init {
    generation: u64,
    bytes: Arc<[u8]>,
}

impl Segment {
    fn retained_bytes(&self) -> usize {
        self.bytes.len() + self.init.bytes.len()
    }
}

#[derive(Default)]
pub(crate) struct Window {
    live: VecDeque<Segment>,
    retired: VecDeque<(Instant, Segment)>,
    visible_ticks: u32,
    retained_bytes: usize,
    discontinuity_sequence: u64,
    pub next_sequence: u64,
}

impl Window {
    pub fn ready(&self) -> bool {
        self.live.len() >= 2
    }

    pub fn publish(&mut self, segment: Segment, now: Instant) -> Result<(), Error> {
        self.expire(now);
        let retained_bytes = self.retained_bytes + segment.retained_bytes();
        if retained_bytes > MAX_RETAINED_BYTES
            || self.live.len() + self.retired.len() >= MAX_RETAINED_SEGMENTS
        {
            return Err(Error::MediaLimit);
        }
        self.retained_bytes = retained_bytes;
        self.next_sequence += 1;
        self.visible_ticks += segment.duration;
        self.live.push_back(segment);
        while self
            .live
            .front()
            .is_some_and(|front| self.visible_ticks - front.duration >= MIN_WINDOW_TICKS)
        {
            if let Some(old) = self.live.pop_front() {
                self.visible_ticks -= old.duration;
                self.discontinuity_sequence += u64::from(old.discontinuity);
                self.retired.push_back((now + RETIREMENT_GRACE, old));
            }
        }
        Ok(())
    }

    fn expire(&mut self, now: Instant) {
        while self
            .retired
            .front()
            .is_some_and(|(expiry, _)| now >= *expiry)
        {
            if let Some((_, segment)) = self.retired.pop_front() {
                self.retained_bytes -= segment.retained_bytes();
            }
        }
    }

    pub fn media_sequence(&self) -> Option<u64> {
        self.live.front().map(|first| first.sequence)
    }

    pub fn playlist(&self) -> Option<String> {
        if !self.ready() {
            return None;
        }
        let mut active_init = None;
        let segments = self.live.iter().map(|segment| {
            let generation = Some(segment.init.generation);
            let initialization = (active_init != generation).then(|| Initialization {
                uri: format!("init-{}.mp4", segment.init.generation).into(),
                byte_range: None,
            });
            active_init = generation;
            MediaSegment {
                duration_seconds: f64::from(segment.duration) / f64::from(TIMESCALE),
                uri: format!("seg-{}.m4s", segment.sequence).into(),
                byte_range: None,
                discontinuity: segment.discontinuity,
                initialization,
            }
        });
        // Segmenter bounds all durations. The resource URIs above are generated locally.
        write_media_playlist(
            MediaPlaylistOptions {
                kind: PlaylistKind::Live,
                target_duration: u64::from(TARGET_DURATION),
                media_sequence: self.media_sequence()?,
                discontinuity_sequence: Some(self.discontinuity_sequence),
                independent_segments: true,
                duration_precision: DurationPrecision::Microseconds,
            },
            segments,
        )
        .ok()
    }

    pub fn resource(&mut self, name: &str) -> Option<Arc<[u8]>> {
        self.expire(Instant::now());
        let mut segments = self
            .live
            .iter()
            .chain(self.retired.iter().map(|(_, item)| item));
        if let Some(id) = name
            .strip_prefix("seg-")
            .and_then(|name| name.strip_suffix(".m4s"))
        {
            let sequence = id.parse::<u64>().ok()?;
            segments
                .find(|item| item.sequence == sequence)
                .map(|item| Arc::clone(&item.bytes))
        } else if let Some(id) = name
            .strip_prefix("init-")
            .and_then(|name| name.strip_suffix(".mp4"))
        {
            let generation = id.parse::<u64>().ok()?;
            segments
                .find(|item| item.init.generation == generation)
                .map(|item| Arc::clone(&item.init.bytes))
        } else {
            None
        }
    }
}

#[derive(Clone, Copy)]
struct SegmentBoundary {
    sample_count: usize,
    bytes: usize,
    elapsed: u32,
}

#[derive(Default)]
pub(crate) struct Segmenter {
    init: Option<Arc<Init>>,
    decode_time: u64,
    elapsed: u32,
    bytes: usize,
    samples: Vec<Sample>,
    pending_boundary: Option<SegmentBoundary>,
    previous_ts: u32,
    audio_period_start: Option<i64>,
    discontinuity: bool,
}

impl Segmenter {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn push(
        &mut self,
        unit: AccessUnit,
        config: &VideoConfig,
        sequence: u64,
        audio: Option<(&mut crate::aac::Timeline, i64)>,
    ) -> Result<Option<Segment>, Error> {
        let mut segment = None;
        let mut audio = audio;
        if let Some(previous) = self.samples.last_mut() {
            let duration = unit.timestamp.wrapping_sub(self.previous_ts);
            if duration == 0 || duration > MAX_SAMPLE_INTERVAL_TICKS {
                return Err(Error::UnsupportedTiming);
            }
            previous.duration = duration;
            self.elapsed += duration;
            // Prefer the last skipped sync point when the combined GOPs exceed
            // the preferred length. A single GOP can use the EXTINF rounding margin.
            let boundary = self
                .pending_boundary
                .filter(|_| self.elapsed > PREFERRED_SEGMENT_TICKS)
                .or_else(|| {
                    (unit.sync && (MIN_SEGMENT_TICKS..=MAX_SEGMENT_TICKS).contains(&self.elapsed))
                        .then(|| self.boundary())
                });
            if let Some(boundary) = boundary {
                let fragment_audio = audio.as_mut().and_then(|(timeline, video_time)| {
                    let start = *video_time - i64::from(self.elapsed);
                    timeline.fragment(
                        self.audio_period_start?,
                        start,
                        start + i64::from(boundary.elapsed),
                    )
                });
                segment = Some(self.finish(sequence, boundary, fragment_audio)?);
            }
            if self.elapsed > MAX_SEGMENT_TICKS {
                // Drop the overlong GOP. The ingest watchdog bounds the wait
                // for a publishable fragment if the origin keeps sending long GOPs.
                self.reset();
            }
        }
        if self.init.is_none() {
            if !unit.sync {
                return Ok(segment);
            }
            let generation = sequence + u64::from(segment.is_some());
            self.init = Some(Arc::new(Init {
                generation,
                bytes: mp4::init(config, audio.as_ref().map(|(timeline, _)| &timeline.config))?
                    .into(),
            }));
            self.discontinuity = generation != 0;
            self.audio_period_start = audio.as_ref().map(|(_, video_time)| *video_time);
        }
        if unit.sync && !self.samples.is_empty() {
            self.pending_boundary = Some(self.boundary());
        }
        self.bytes += unit.bytes.len();
        if self.bytes > MAX_SEGMENT_BYTES || self.samples.len() >= MAX_SEGMENT_SAMPLES {
            return Err(Error::MediaLimit);
        }
        self.samples.push(Sample {
            duration: 0,
            sync: unit.sync,
            bytes: unit.bytes,
        });
        self.previous_ts = unit.timestamp;
        Ok(segment)
    }

    fn boundary(&self) -> SegmentBoundary {
        SegmentBoundary {
            sample_count: self.samples.len(),
            bytes: self.bytes,
            elapsed: self.elapsed,
        }
    }

    fn finish(
        &mut self,
        sequence: u64,
        boundary: SegmentBoundary,
        audio: Option<(u64, Vec<Sample>)>,
    ) -> Result<Segment, Error> {
        let init = self.init.clone().ok_or(Error::InvalidResponse)?;
        let bytes = mp4::fragment(
            sequence + 1,
            self.decode_time,
            &self.samples[..boundary.sample_count],
            audio
                .as_ref()
                .map(|(time, samples)| (*time, samples.as_slice())),
        )?;
        let segment = Segment {
            sequence,
            duration: boundary.elapsed,
            init,
            bytes: bytes.into(),
            discontinuity: self.discontinuity,
        };
        self.samples.drain(..boundary.sample_count);
        self.bytes -= boundary.bytes;
        self.decode_time += u64::from(boundary.elapsed);
        self.elapsed -= boundary.elapsed;
        self.pending_boundary = None;
        self.discontinuity = false;
        Ok(segment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video_config() -> VideoConfig {
        VideoConfig {
            sps: [(
                0,
                vec![
                    0x67, 0x42, 0xc0, 0x1e, 0xda, 0x02, 0x80, 0xbf, 0xe5, 0xc0, 0x44, 0, 0, 3, 0,
                    4, 0, 0, 3, 0, 0xf2, 0x3c, 0x58, 0xba, 0x80,
                ],
            )]
            .into(),
            pps: [(0, vec![0x68, 0xce, 0x0f, 0xc8])].into(),
            ..VideoConfig::default()
        }
    }

    #[test]
    fn short_gop_can_close_before_the_combined_segment_exceeds_the_preferred_length() {
        let config = video_config();
        let mut segmenter = Segmenter::default();
        let mut durations = Vec::new();
        for frame in 0..=22 {
            let unit = AccessUnit {
                timestamp: frame * TIMESCALE / 10,
                sync: matches!(frame, 0 | 6 | 22),
                bytes: vec![0, 0, 0, 2, 0x61, 0xc0],
            };
            if let Some(segment) = segmenter
                .push(unit, &config, durations.len() as u64, None)
                .expect("each GOP fits in the segment budget")
            {
                durations.push(segment.duration);
                assert!(segmenter.samples[0].sync);
            }
        }
        assert_eq!(durations, [6 * TIMESCALE / 10, 16 * TIMESCALE / 10]);
        assert_eq!(segmenter.decode_time, u64::from(22 * TIMESCALE / 10));
    }

    #[test]
    fn target_rounding_margin_is_published_and_an_overlong_gop_is_skipped() {
        let config = video_config();
        let now = Instant::now();
        let mut segmenter = Segmenter::default();
        let mut window = Window::default();
        let near_target = PREFERRED_SEGMENT_TICKS + TIMESCALE / 1_000;
        let at_limit = near_target + MAX_SEGMENT_TICKS;
        let after_long_gop = at_limit + MAX_SEGMENT_TICKS + 1;
        for (timestamp, sync) in [
            (0, true),
            (TIMESCALE, false),
            (near_target, true),
            (near_target + TIMESCALE, false),
            (at_limit, true),
            (at_limit + TIMESCALE, false),
            (after_long_gop, true),
            (after_long_gop + TIMESCALE, true),
        ] {
            let unit = AccessUnit {
                timestamp,
                sync,
                bytes: vec![0, 0, 0, 2, 0x61, 0xc0],
            };
            if let Some(segment) = segmenter
                .push(unit, &config, window.next_sequence, None)
                .expect("long GOPs are discarded without poisoning the window")
            {
                window.publish(segment, now).expect("segment");
            }
        }
        let playlist = window.playlist().expect("playlist");
        assert!(playlist.contains("#EXT-X-TARGETDURATION:2\n"));
        assert!(playlist.contains("#EXTINF:2.001000,\nseg-0.m4s\n"));
        assert!(playlist.contains("#EXTINF:2.499989,\nseg-1.m4s\n"));
        assert!(playlist.contains(
            "#EXT-X-DISCONTINUITY\n#EXT-X-MAP:URI=\"init-2.mp4\"\n#EXTINF:1.000000,\nseg-2.m4s\n"
        ));
        assert_eq!(window.next_sequence, 3);
    }

    #[test]
    fn slid_segment_remains_fetchable_for_the_hls_grace_period() {
        let mut window = Window::default();
        let now = Instant::now();
        let init = Arc::new(Init {
            generation: 0,
            bytes: Arc::from(&b"init"[..]),
        });
        for sequence in 0..7 {
            window
                .publish(
                    Segment {
                        sequence,
                        duration: TIMESCALE,
                        init: init.clone(),
                        bytes: Arc::from(&b"video"[..]),
                        discontinuity: sequence == 0,
                    },
                    now,
                )
                .expect("segment");
        }
        assert_eq!(window.media_sequence(), Some(1));
        assert!(
            window
                .playlist()
                .expect("playlist")
                .contains("#EXT-X-DISCONTINUITY-SEQUENCE:1\n")
        );
        assert_eq!(window.resource("seg-0.m4s").as_deref(), Some(&b"video"[..]));
        window.expire(now + RETIREMENT_GRACE);
        assert!(window.resource("seg-0.m4s").is_none());
    }
}
