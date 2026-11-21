use crate::{
    BridgeError as Error, aac,
    rtp::{ParameterSets, VideoCodec, VideoConfig},
};

pub(crate) const TIMESCALE: u32 = 90_000;

// A bounded RBSP prefix for the slice header, or the complete, bounded SPS.
struct Bits {
    bytes: Vec<u8>,
    position: usize,
}

impl Bits {
    fn from_ebsp(bytes: &[u8]) -> Self {
        let mut rbsp = Vec::with_capacity(bytes.len());
        let mut zeros = 0;
        for &byte in bytes {
            if zeros == 2 && byte == 3 {
                zeros = 0;
                continue;
            }
            rbsp.push(byte);
            zeros = if byte == 0 { (zeros + 1).min(2) } else { 0 };
        }
        Self {
            bytes: rbsp,
            position: 0,
        }
    }

    fn bits(&mut self, count: u32) -> Option<u32> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1)
                | u32::from((self.bytes.get(self.position / 8)? >> (7 - self.position % 8)) & 1);
            self.position += 1;
        }
        Some(value)
    }

    fn ue(&mut self) -> Option<u32> {
        for zeros in 0..32 {
            if self.bits(1)? == 1 {
                return Some((1 << zeros) - 1 + self.bits(zeros)?);
            }
        }
        None
    }

    fn se(&mut self) -> Option<i32> {
        let zigzag = self.ue()?;
        let magnitude = i32::try_from(zigzag.div_ceil(2)).ok()?;
        Some(if zigzag % 2 == 0 {
            -magnitude
        } else {
            magnitude
        })
    }

    fn scaling_list(&mut self, coefficients: usize) -> Option<()> {
        let mut last = 8;
        let mut next = 8;
        for _ in 0..coefficients {
            if next != 0 {
                next = (last + self.se()?.rem_euclid(256)) % 256;
            }
            if next != 0 {
                last = next;
            }
        }
        Some(())
    }
}

pub(crate) fn slice_header(nal: &[u8]) -> Option<(u32, u32)> {
    let mut rbsp = Bits::from_ebsp(nal.get(1..nal.len().min(24))?);
    let first_mb = rbsp.ue()?;
    let slice_type = rbsp.ue()?;
    (slice_type <= 9).then_some((first_mb, slice_type % 5))
}

pub(crate) fn parameter_set_id(kind: u8, nal: &[u8]) -> Option<u8> {
    let (id, limit) = match kind {
        7 => {
            let mut bits = Bits::from_ebsp(nal.get(1..)?);
            bits.bits(24)?;
            (bits.ue()?, 31)
        }
        8 => (Bits::from_ebsp(nal.get(1..)?).ue()?, 255),
        32 => (u32::from(nal.get(2)? >> 4), 15),
        33 => (u32::from(hevc_sps_info(nal)?.0), 15),
        34 => (Bits::from_ebsp(nal.get(2..)?).ue()?, 63),
        _ => return None,
    };
    (id <= limit).then_some(id as u8)
}

pub(crate) fn referenced_sps_id(codec: VideoCodec, pps: &[u8]) -> Option<u8> {
    let header_size = match codec {
        VideoCodec::Avc => 1,
        VideoCodec::Hevc => 2,
    };
    let mut bits = Bits::from_ebsp(pps.get(header_size..)?);
    bits.ue()?; // PPS identifier.
    u8::try_from(bits.ue()?).ok()
}

fn dimensions(sps: &[u8]) -> Option<(u16, u16)> {
    if sps.first()? & 0x9f != 7 {
        return None;
    }
    let mut b = Bits::from_ebsp(sps.get(1..)?);
    let profile = b.bits(8)?;
    b.bits(16)?;
    b.ue()?;
    match profile {
        66 | 77 | 88 => (),
        100 => {
            if b.ue()? != 1 || b.ue()? != 0 || b.ue()? != 0 {
                return None;
            }
            b.bits(1)?;
            if b.bits(1)? == 1 {
                for i in 0..8 {
                    if b.bits(1)? == 1 {
                        b.scaling_list(if i < 6 { 16 } else { 64 })?;
                    }
                }
            }
        }
        _ => return None,
    }
    b.ue()?;
    match b.ue()? {
        0 => {
            b.ue()?;
        }
        1 => {
            b.bits(1)?;
            b.se()?;
            b.se()?;
            let cycle = b.ue()?;
            if cycle > 255 {
                return None;
            }
            for _ in 0..cycle {
                b.se()?;
            }
        }
        2 => (),
        _ => return None,
    }
    b.ue()?;
    b.bits(1)?;
    let width = b.ue()?.checked_add(1)?.checked_mul(16)?;
    let height = b.ue()?.checked_add(1)?.checked_mul(16)?;
    if b.bits(1)? != 1 {
        return None;
    }
    b.bits(1)?;
    let (width, height) = if b.bits(1)? == 1 {
        let crop_x = b.ue()?.checked_add(b.ue()?)?.checked_mul(2)?;
        let crop_y = b.ue()?.checked_add(b.ue()?)?.checked_mul(2)?;
        (width.checked_sub(crop_x)?, height.checked_sub(crop_y)?)
    } else {
        (width, height)
    };
    if width == 0 || height == 0 {
        return None;
    }
    Some((u16::try_from(width).ok()?, u16::try_from(height).ok()?))
}

pub(crate) fn compatible_sequence_parameters(codec: VideoCodec, left: &[u8], right: &[u8]) -> bool {
    match codec {
        VideoCodec::Avc => {
            left.get(1..4) == right.get(1..4)
                && dimensions(left)
                    .zip(dimensions(right))
                    .is_some_and(|(a, b)| a == b)
        }
        VideoCodec::Hevc => hevc_sps_info(left)
            .zip(hevc_sps_info(right))
            .is_some_and(|((_, a), (_, b))| a == b),
    }
}

#[derive(Default)]
struct Boxes(Vec<u8>);

impl Boxes {
    fn atom(&mut self, kind: &[u8; 4], write: impl FnOnce(&mut Self)) {
        let start = self.0.len();
        self.u32(0);
        self.bytes(kind);
        write(self);
        // Each box fits in the bridge's bounded segment or initialization buffer.
        let size = (self.0.len() - start) as u32;
        self.0[start..start + 4].copy_from_slice(&size.to_be_bytes());
    }

    fn full(&mut self, kind: &[u8; 4], version_and_flags: u32, write: impl FnOnce(&mut Self)) {
        self.atom(kind, |b| {
            b.u32(version_and_flags);
            write(b);
        });
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_be_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_be_bytes());
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.0.extend(bytes);
    }

    fn zero(&mut self, size: usize) {
        self.0.resize(self.0.len() + size, 0);
    }

    fn identity(&mut self) {
        for value in [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
            self.u32(value);
        }
    }
}

struct VideoEntry {
    format: &'static [u8; 4],
    box_type: &'static [u8; 4],
    configuration: Vec<u8>,
    width: u16,
    height: u16,
}

fn video_entry(config: &VideoConfig) -> Option<VideoEntry> {
    match config.codec {
        VideoCodec::Avc => {
            let first_sps = config.sps.values().next()?;
            let size = dimensions(first_sps)?;
            let profile = first_sps.get(1..4)?;
            // One sample entry must describe the SPSs in its decoder record.
            if config
                .sps
                .values()
                .skip(1)
                .any(|sps| sps.get(1..4) != Some(profile) || dimensions(sps) != Some(size))
            {
                return None;
            }
            let mut b = Boxes::default();
            b.bytes(&[1]);
            b.bytes(profile);
            b.bytes(&[0xff, 0xe0 | u8::try_from(config.sps.len()).ok()?]);
            write_parameter_sets(&mut b, &config.sps)?;
            b.bytes(&[u8::try_from(config.pps.len()).ok()?]);
            write_parameter_sets(&mut b, &config.pps)?;
            if profile[0] == 100 {
                b.bytes(&[0xfd, 0xf8, 0xf8, 0]); // 8-bit 4:2:0, no SPS extensions.
            }
            Some(VideoEntry {
                format: b"avc1",
                box_type: b"avcC",
                configuration: b.0,
                width: size.0,
                height: size.1,
            })
        }
        VideoCodec::Hevc => hevc_entry(config),
    }
}

fn write_parameter_sets(b: &mut Boxes, sets: &ParameterSets) -> Option<()> {
    if sets.is_empty() {
        return None;
    }
    for nal in sets.values() {
        b.u16(u16::try_from(nal.len()).ok()?);
        b.bytes(nal);
    }
    Some(())
}

#[derive(PartialEq, Eq)]
struct HevcSpsInfo {
    dimensions: (u16, u16),
    configuration_header: Vec<u8>,
}

fn hevc_sps_info(sps: &[u8]) -> Option<(u8, HevcSpsInfo)> {
    let mut bits = Bits::from_ebsp(sps.get(2..)?);
    bits.bits(4)?;
    let sub_layers = bits.bits(3)?;
    if sub_layers > 6 {
        return None;
    }
    let nested = bits.bits(1)?;
    let mut decoder_config = Boxes::default();
    decoder_config.bytes(&[1]);
    // profile space, tier, profile, compatibility flags, constraints, and level.
    for _ in 0..12 {
        decoder_config.bytes(&[bits.bits(8)? as u8]);
    }
    let mut sub_layer_flags = [0; 6];
    for flags in sub_layer_flags.iter_mut().take(sub_layers as usize) {
        *flags = bits.bits(2)?;
    }
    if sub_layers > 0 {
        bits.bits(2 * (8 - sub_layers))?;
    }
    for flags in sub_layer_flags {
        if flags & 2 != 0 {
            bits.bits(32)?;
            bits.bits(32)?;
            bits.bits(24)?;
        }
        if flags & 1 != 0 {
            bits.bits(8)?;
        }
    }
    let id = u8::try_from(bits.ue()?).ok()?;
    let chroma = bits.ue()?;
    if chroma != 1 {
        return None;
    } // Single-layer 4:2:0 only.
    let mut width = bits.ue()?;
    let mut height = bits.ue()?;
    if bits.bits(1)? != 0 {
        let left = bits.ue()?;
        let right = bits.ue()?;
        let top = bits.ue()?;
        let bottom = bits.ue()?;
        width = width.checked_sub(left.checked_add(right)?.checked_mul(2)?)?;
        height = height.checked_sub(top.checked_add(bottom)?.checked_mul(2)?)?;
    }
    let luma = bits.ue()?;
    let chroma_bits = bits.ue()?;
    if width == 0 || height == 0 || !matches!(luma, 0 | 2) || luma != chroma_bits {
        return None;
    }
    decoder_config.u16(0xf000);
    decoder_config.bytes(&[
        0xfc,
        0xfc | chroma as u8,
        0xf8 | luma as u8,
        0xf8 | chroma_bits as u8,
    ]);
    decoder_config.u16(0);
    decoder_config.bytes(&[(((sub_layers + 1) << 3) | (nested << 2) | 3) as u8, 3]);
    Some((
        id,
        HevcSpsInfo {
            dimensions: (u16::try_from(width).ok()?, u16::try_from(height).ok()?),
            configuration_header: decoder_config.0,
        },
    ))
}

fn hevc_entry(config: &VideoConfig) -> Option<VideoEntry> {
    let mut spss = config.sps.values();
    let (_, info) = hevc_sps_info(spss.next()?)?;
    for sps in spss {
        if hevc_sps_info(sps)?.1 != info {
            return None;
        }
    }
    let mut decoder_config = Boxes(info.configuration_header);
    for (kind, sets) in [(32, &config.vps), (33, &config.sps), (34, &config.pps)] {
        decoder_config.bytes(&[0x80 | kind]);
        decoder_config.u16(u16::try_from(sets.len()).ok()?);
        write_parameter_sets(&mut decoder_config, sets)?;
    }
    Some(VideoEntry {
        format: b"hvc1",
        box_type: b"hvcC",
        configuration: decoder_config.0,
        width: info.dimensions.0,
        height: info.dimensions.1,
    })
}

pub(crate) fn init(config: &VideoConfig, audio: Option<&aac::Config>) -> Result<Vec<u8>, Error> {
    let entry = video_entry(config).ok_or(Error::UnsupportedVideo)?;
    let (width, height) = (entry.width, entry.height);
    let mut file = Boxes::default();
    file.atom(b"ftyp", |b| {
        b.bytes(b"iso6\0\0\0\x01iso6mp41");
        b.bytes(entry.format);
    });
    file.atom(b"moov", |b| {
        b.full(b"mvhd", 0, |b| {
            b.zero(8); // Creation and modification times.
            b.u32(1000);
            b.u32(0); // Fragmented movie duration is unknown.
            b.u32(0x0001_0000);
            b.u16(0x0100);
            b.zero(10);
            b.identity();
            b.zero(24);
            b.u32(if audio.is_some() { 3 } else { 2 }); // Next track ID.
        });
        b.atom(b"trak", |b| {
            b.full(b"tkhd", 3, |b| {
                b.zero(8);
                b.u32(1); // Track ID.
                b.zero(24); // Reserved fields, duration, layer, group, and volume.
                b.identity();
                b.u32(u32::from(width) << 16);
                b.u32(u32::from(height) << 16);
            });
            b.atom(b"mdia", |b| {
                b.full(b"mdhd", 0, |b| {
                    b.zero(8);
                    b.u32(TIMESCALE);
                    b.u32(0);
                    b.u16(0x55c4); // ISO 639-2 und.
                    b.u16(0);
                });
                b.full(b"hdlr", 0, |b| {
                    b.u32(0);
                    b.bytes(b"vide");
                    b.zero(12);
                    b.bytes(b"Video\0");
                });
                b.atom(b"minf", |b| {
                    b.full(b"vmhd", 1, |b| b.zero(8));
                    b.atom(b"dinf", |b| {
                        b.full(b"dref", 0, |b| {
                            b.u32(1);
                            b.full(b"url ", 1, |_| {});
                        });
                    });
                    b.atom(b"stbl", |b| {
                        b.full(b"stsd", 0, |b| {
                            b.u32(1);
                            b.atom(entry.format, |b| {
                                b.zero(6);
                                b.u16(1); // Data reference index.
                                b.zero(16);
                                b.u16(width);
                                b.u16(height);
                                b.u32(0x0048_0000);
                                b.u32(0x0048_0000);
                                b.u32(0);
                                b.u16(1);
                                b.zero(32); // Compressor name.
                                b.u16(24);
                                b.u16(u16::MAX);
                                b.atom(entry.box_type, |b| b.bytes(&entry.configuration));
                            });
                        });
                        for empty_table in [b"stts", b"stsc", b"stco"] {
                            b.full(empty_table, 0, |b| b.u32(0));
                        }
                        b.full(b"stsz", 0, |b| b.zero(8));
                    });
                });
            });
        });
        if let Some(audio) = audio {
            audio_track(b, audio);
        }
        b.atom(b"mvex", |b| {
            for track in 1..=if audio.is_some() { 2 } else { 1 } {
                b.full(b"trex", 0, |b| {
                    b.u32(track);
                    b.u32(1);
                    b.zero(12);
                });
            }
        });
    });
    Ok(file.0)
}

fn audio_track(b: &mut Boxes, config: &aac::Config) {
    b.atom(b"trak", |b| {
        b.full(b"tkhd", 3, |b| {
            b.zero(8);
            b.u32(2);
            b.zero(20);
            b.u16(0x0100); // Audio track volume.
            b.u16(0);
            b.identity();
            b.zero(8);
        });
        b.atom(b"mdia", |b| {
            b.full(b"mdhd", 0, |b| {
                b.zero(8);
                b.u32(config.sample_rate);
                b.u32(0);
                b.u16(0x55c4);
                b.u16(0);
            });
            b.full(b"hdlr", 0, |b| {
                b.u32(0);
                b.bytes(b"soun");
                b.zero(12);
                b.bytes(b"Audio\0");
            });
            b.atom(b"minf", |b| {
                b.full(b"smhd", 0, |b| b.zero(4));
                b.atom(b"dinf", |b| {
                    b.full(b"dref", 0, |b| {
                        b.u32(1);
                        b.full(b"url ", 1, |_| {});
                    })
                });
                b.atom(b"stbl", |b| {
                    b.full(b"stsd", 0, |b| {
                        b.u32(1);
                        b.atom(b"mp4a", |b| {
                            b.zero(6);
                            b.u16(1);
                            b.zero(8);
                            b.u16(config.channels);
                            b.u16(16);
                            b.zero(4);
                            b.u32(config.sample_rate << 16);
                            b.full(b"esds", 0, |b| {
                                b.bytes(&[0x03, 25, 0, 2, 0, 0x04, 17, 0x40, 0x15]);
                                b.zero(11); // Buffer size and optional bitrate declarations.
                                b.bytes(&[0x05, 2]);
                                b.bytes(&config.specific);
                                b.bytes(&[0x06, 1, 2]);
                            });
                        });
                    });
                    for empty_table in [b"stts", b"stsc", b"stco"] {
                        b.full(empty_table, 0, |b| b.u32(0));
                    }
                    b.full(b"stsz", 0, |b| b.zero(8));
                });
            });
        });
    });
}

pub(crate) struct Sample {
    pub duration: u32,
    pub sync: bool,
    pub bytes: Vec<u8>,
}

pub(crate) fn fragment(
    sequence: u64,
    decode_time: u64,
    samples: &[Sample],
    audio: Option<(u64, &[Sample])>,
) -> Result<Vec<u8>, Error> {
    let sequence = u32::try_from(sequence).map_err(|_| Error::MediaLimit)?;
    let mut file = Boxes::default();
    file.atom(b"styp", |b| b.bytes(b"msdh\0\0\0\0msdhmsix"));
    let moof = file.0.len();
    let mut video_offset = 0;
    let mut audio_offset = None;
    file.atom(b"moof", |b| {
        b.full(b"mfhd", 0, |b| b.u32(sequence));
        video_offset = track_fragment(b, 1, decode_time, samples);
        if let Some((time, samples)) = audio {
            audio_offset = Some(track_fragment(b, 2, time, samples));
        }
    });
    let data_offset = file.0.len() - moof + 8;
    let video_bytes = samples
        .iter()
        .map(|sample| sample.bytes.len())
        .sum::<usize>();
    for (field, offset) in [
        (Some(video_offset), data_offset),
        (audio_offset, data_offset + video_bytes),
    ] {
        if let Some(field) = field {
            let offset = u32::try_from(offset).map_err(|_| Error::MediaLimit)?;
            file.0[field..field + 4].copy_from_slice(&offset.to_be_bytes());
        }
    }
    file.atom(b"mdat", |b| {
        for sample in samples
            .iter()
            .chain(audio.into_iter().flat_map(|(_, samples)| samples))
        {
            b.bytes(&sample.bytes);
        }
    });
    Ok(file.0)
}

fn track_fragment(b: &mut Boxes, track: u32, time: u64, samples: &[Sample]) -> usize {
    let mut data_offset = 0;
    b.atom(b"traf", |b| {
        b.full(b"tfhd", 0x0002_0000, |b| b.u32(track));
        b.full(b"tfdt", 0x0100_0000, |b| b.bytes(&time.to_be_bytes()));
        b.full(b"trun", 0x0000_0701, |b| {
            b.u32(samples.len() as u32);
            data_offset = b.0.len();
            b.u32(0);
            for sample in samples {
                b.u32(sample.duration);
                b.u32(sample.bytes.len() as u32);
                b.u32(if sample.sync {
                    0x0200_0000
                } else {
                    0x0101_0000
                });
            }
        });
    });
    data_offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_header_contains_the_full_matrix_and_uncropped_dimensions() {
        // Constrained Baseline SPS for a 640x360 progressive image.
        let avc = VideoConfig {
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
        };
        let init = init(&avc, None).expect("init");
        let kind = init
            .windows(4)
            .position(|kind| kind == b"tkhd")
            .expect("tkhd");
        let atom = &init[kind - 4..];
        assert_eq!(&atom[0..4], &92u32.to_be_bytes());
        assert_eq!(&atom[80..84], &0x4000_0000u32.to_be_bytes());
        assert_eq!(&atom[84..88], &(640u32 << 16).to_be_bytes());
        assert_eq!(&atom[88..92], &(360u32 << 16).to_be_bytes());
    }
}
