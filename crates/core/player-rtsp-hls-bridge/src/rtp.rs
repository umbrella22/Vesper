use std::collections::BTreeMap;

use crate::mp4::{
    compatible_sequence_parameters, parameter_set_id, referenced_sps_id, slice_header,
};

const MAX_ACCESS_UNIT_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_PARAMETER_SET_BYTES: usize = 4096;
const MAX_PARAMETER_SETS_PER_TYPE: usize = 16;

pub(crate) struct Packet<'a> {
    pub payload_type: u8,
    pub marker: bool,
    pub sequence: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub payload: &'a [u8],
}

impl<'a> Packet<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        let header = bytes.first_chunk::<12>()?;
        if header[0] >> 6 != 2 {
            return None;
        }
        let csrc_end = 12 + 4 * usize::from(header[0] & 15);
        let mut payload = bytes.get(csrc_end..)?;
        if header[0] & 0x10 != 0 {
            let ext = payload.first_chunk::<4>()?;
            let words = usize::from(u16::from_be_bytes([ext[2], ext[3]]));
            payload = payload.get(4 + words * 4..)?;
        }
        if header[0] & 0x20 != 0 {
            let padding = usize::from(*payload.last()?);
            if padding == 0 {
                return None;
            }
            payload = payload.get(..payload.len().checked_sub(padding)?)?;
        }
        Some(Self {
            marker: header[1] & 0x80 != 0,
            payload_type: header[1] & 0x7f,
            sequence: u16::from_be_bytes([header[2], header[3]]),
            timestamp: u32::from_be_bytes([header[4], header[5], header[6], header[7]]),
            ssrc: u32::from_be_bytes([header[8], header[9], header[10], header[11]]),
            payload,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum VideoCodec {
    #[default]
    Avc,
    Hevc,
}

pub(crate) type ParameterSets = BTreeMap<u8, Vec<u8>>;

#[derive(Default, PartialEq, Eq, Clone)]
pub(crate) struct VideoConfig {
    pub codec: VideoCodec,
    pub vps: ParameterSets,
    pub sps: ParameterSets,
    pub pps: ParameterSets,
}

impl VideoConfig {
    pub fn parameter_set_type(&self, nal: &[u8]) -> Option<u8> {
        match self.codec {
            VideoCodec::Avc => match nal.first()? & 0x9f {
                kind @ (7 | 8) => Some(kind),
                _ => None,
            },
            VideoCodec::Hevc => hevc_nal_type(nal).filter(|kind| (32..=34).contains(kind)),
        }
    }

    pub fn set_parameter_set(&mut self, kind: u8, nal: &[u8]) -> Option<()> {
        if nal.len() > MAX_PARAMETER_SET_BYTES || self.parameter_set_type(nal)? != kind {
            return None;
        }
        let id = parameter_set_id(kind, nal)?;
        let sets = match kind {
            7 | 33 => &mut self.sps,
            8 | 34 => &mut self.pps,
            32 => &mut self.vps,
            _ => return None,
        };
        if sets.len() >= MAX_PARAMETER_SETS_PER_TYPE && !sets.contains_key(&id) {
            return None;
        }
        sets.insert(id, nal.to_vec());
        Some(())
    }

    fn apply_parameter_sets(&mut self, pending: &mut Self) -> Option<bool> {
        // A single in-band SPS with different sample-entry properties can
        // replace the previous SPS even when the encoder changes its ID.
        if self.sps.len() == 1
            && pending.sps.len() == 1
            && let (Some((&old_id, old)), Some((&new_id, new))) =
                (self.sps.first_key_value(), pending.sps.first_key_value())
            && old_id != new_id
            && !compatible_sequence_parameters(self.codec, old, new)
        {
            let mut next = self.clone();
            next.sps.remove(&old_id);
            next.pps
                .retain(|_, pps| referenced_sps_id(self.codec, pps) != Some(old_id));
            next.merge_parameter_sets(pending)?;
            *self = next;
            return Some(true);
        }
        self.merge_parameter_sets(pending)
    }

    fn merge_parameter_sets(&mut self, pending: &mut Self) -> Option<bool> {
        let groups = [
            (&mut self.sps, &mut pending.sps),
            (&mut self.pps, &mut pending.pps),
            (&mut self.vps, &mut pending.vps),
        ];
        for (current, pending) in &groups {
            let added = pending
                .keys()
                .filter(|id| !current.contains_key(id))
                .count();
            if current.len() + added > MAX_PARAMETER_SETS_PER_TYPE {
                return None;
            }
        }
        let mut changed = false;
        for (current, pending) in groups {
            for (id, nal) in std::mem::take(pending) {
                if current.get(&id) != Some(&nal) {
                    current.insert(id, nal);
                    changed = true;
                }
            }
        }
        Some(changed)
    }
}

pub(crate) struct AccessUnit {
    pub timestamp: u32,
    pub sync: bool,
    pub bytes: Vec<u8>,
}

pub(crate) struct VideoAssembler {
    pub config: VideoConfig,
    expected: Option<u16>,
    timestamp: Option<u32>,
    broken_timestamp: Option<u32>,
    need_idr: bool,
    sync: bool,
    vcl: bool,
    bytes: Vec<u8>,
    fragment: Option<usize>,
    pub damaged_units: u64,
    pub unsupported_timing: bool,
    pub config_changes: u64,
    pending_config: VideoConfig,
    ssrc: Option<u32>,
}

impl VideoAssembler {
    pub fn new(config: VideoConfig) -> Self {
        let pending_config = VideoConfig {
            codec: config.codec,
            ..VideoConfig::default()
        };
        Self {
            config,
            expected: None,
            timestamp: None,
            broken_timestamp: None,
            need_idr: true,
            sync: false,
            vcl: false,
            bytes: Vec::new(),
            fragment: None,
            damaged_units: 0,
            unsupported_timing: false,
            config_changes: 0,
            pending_config,
            ssrc: None,
        }
    }

    pub fn push(&mut self, packet: &Packet<'_>) -> Option<AccessUnit> {
        if self.ssrc != Some(packet.ssrc) {
            if self.ssrc.is_some() {
                self.reject_packet();
            }
            self.ssrc = Some(packet.ssrc);
        }
        let mut gap = false;
        if let Some(expected) = self.expected {
            let difference = packet.sequence.wrapping_sub(expected) as i16;
            if difference < 0 {
                return None;
            }
            gap = difference > 0;
        }
        self.expected = Some(packet.sequence.wrapping_add(1));
        if gap {
            self.discard(Some(packet.timestamp));
        } else if self.timestamp.is_some_and(|ts| ts != packet.timestamp) {
            self.discard(None);
        }
        if self.broken_timestamp == Some(packet.timestamp) {
            return None;
        }
        self.broken_timestamp = None;
        self.timestamp = Some(packet.timestamp);
        if self.append_payload(packet.payload).is_none() {
            self.discard(Some(packet.timestamp));
            return None;
        }
        if !packet.marker {
            return None;
        }
        if self.fragment.is_some() {
            self.discard(Some(packet.timestamp));
            return None;
        }
        let Some(config_changed) = self.config.apply_parameter_sets(&mut self.pending_config)
        else {
            self.discard(Some(packet.timestamp));
            return None;
        };
        if config_changed {
            self.config_changes = self.config_changes.saturating_add(1);
            self.need_idr = true;
        }
        let publish = self.vcl
            && (!self.need_idr || self.sync)
            && !self.config.sps.is_empty()
            && !self.config.pps.is_empty()
            && (self.config.codec == VideoCodec::Avc || !self.config.vps.is_empty());
        let unit = publish.then(|| AccessUnit {
            timestamp: packet.timestamp,
            sync: self.sync,
            bytes: std::mem::take(&mut self.bytes),
        });
        if publish {
            self.need_idr = false;
        }
        self.clear_unit();
        unit
    }

    pub fn reject_packet(&mut self) {
        self.discard(self.timestamp);
        self.expected = None;
    }

    fn discard(&mut self, timestamp: Option<u32>) {
        self.damaged_units = self.damaged_units.saturating_add(1);
        self.need_idr = true;
        self.clear_unit();
        self.broken_timestamp = timestamp;
    }

    fn clear_unit(&mut self) {
        self.timestamp = None;
        self.bytes.clear();
        self.sync = false;
        self.vcl = false;
        self.fragment = None;
        self.pending_config.vps.clear();
        self.pending_config.sps.clear();
        self.pending_config.pps.clear();
    }

    fn append_payload(&mut self, payload: &[u8]) -> Option<()> {
        if self.config.codec == VideoCodec::Hevc {
            return self.append_hevc_payload(payload);
        }
        let header = *payload.first()?;
        if header & 0x80 != 0 {
            return None;
        }
        let kind = header & 31;
        if kind != 28 && self.fragment.is_some() {
            return None;
        }
        match kind {
            1..=23 => self.append_nal(payload),
            24 => self.append_aggregation(&payload[1..]),
            28 => self.append_fragment(header, payload.get(1..)?),
            _ => None,
        }
    }

    fn append_aggregation(&mut self, mut rest: &[u8]) -> Option<()> {
        if rest.is_empty() {
            return None;
        }
        while !rest.is_empty() {
            let size = usize::from(u16::from_be_bytes(*rest.first_chunk::<2>()?));
            let (nal, remaining) = rest.get(2..)?.split_at_checked(size)?;
            self.append_nal(nal)?;
            rest = remaining;
        }
        Some(())
    }

    fn append_hevc_payload(&mut self, payload: &[u8]) -> Option<()> {
        let header = *payload.first_chunk::<2>()?;
        let kind = hevc_nal_type(&header)?;
        if kind != 49 && self.fragment.is_some() {
            return None;
        }
        match kind {
            48 => self.append_aggregation(payload.get(2..)?),
            49 => {
                let (&fu, body) = payload.get(2..)?.split_first()?;
                let nal_type = fu & 0x3f;
                if nal_type >= 48 || body.is_empty() {
                    return None;
                }
                let reconstructed = [(header[0] & 0x81) | (nal_type << 1), header[1]];
                self.append_fragment_bytes(&reconstructed, fu, body, nal_type)
            }
            0..=47 => self.append_nal(payload),
            _ => None,
        }
    }

    fn append_nal(&mut self, nal: &[u8]) -> Option<()> {
        if self.config.codec == VideoCodec::Hevc {
            let kind = hevc_nal_type(nal)?;
            if matches!(kind, 32..=34) {
                return self.pending_config.set_parameter_set(kind, nal);
            }
            self.mark_hevc_nal(kind, nal.get(2).copied())?;
        } else {
            self.mark_avc_nal(nal)?;
            if matches!(nal[0] & 31, 7 | 8) {
                return Some(());
            }
        }
        self.extend(&u32::try_from(nal.len()).ok()?.to_be_bytes())?;
        self.extend(nal)
    }

    fn mark_avc_nal(&mut self, nal: &[u8]) -> Option<()> {
        let header = *nal.first()?;
        let kind = header & 31;
        if header & 0x80 != 0 || !(1..=23).contains(&kind) {
            return None;
        }
        if matches!(kind, 7 | 8) {
            return self.pending_config.set_parameter_set(kind, nal);
        }
        self.mark_nal(
            kind,
            matches!(kind, 1 | 5).then(|| slice_header(nal)).flatten(),
        )
    }

    fn mark_nal(&mut self, kind: u8, header: Option<(u32, u32)>) -> Option<()> {
        match kind {
            1 | 5 => {
                let (first_mb, slice_type) = header?;
                if slice_type == 1 {
                    self.unsupported_timing = true;
                    return None;
                }
                if !self.vcl && first_mb != 0 {
                    return None;
                }
                if self.vcl && self.sync != (kind == 5) {
                    return None;
                }
                self.sync = kind == 5;
                self.vcl = true;
            }
            6 | 9..=12 => (),
            _ => return None,
        }
        Some(())
    }

    fn mark_hevc_nal(&mut self, kind: u8, first_payload_byte: Option<u8>) -> Option<()> {
        match kind {
            // RADL and RASL images can precede a CRA in presentation order.
            // They are omitted from the low-delay, random-access segment route.
            6..=9 | 35..=40 => (),
            0..=5 | 16..=21 => {
                let first_slice = first_payload_byte? & 0x80 != 0;
                let random_access = (16..=21).contains(&kind);
                if first_slice == self.vcl || (self.vcl && self.sync != random_access) {
                    return None;
                }
                self.sync = random_access;
                self.vcl = true;
            }
            _ => return None,
        }
        Some(())
    }

    fn append_fragment(&mut self, indicator: u8, payload: &[u8]) -> Option<()> {
        let (&header, body) = payload.split_first()?;
        let nal_header = (indicator & 0xe0) | (header & 0x1f);
        let kind = nal_header & 31;
        if header & 0x20 != 0 || !(1..=23).contains(&kind) || body.is_empty() {
            return None;
        }
        self.append_fragment_bytes(&[nal_header], header, body, kind)
    }

    fn append_fragment_bytes(
        &mut self,
        nal_header: &[u8],
        marker: u8,
        body: &[u8],
        kind: u8,
    ) -> Option<()> {
        let start = marker & 0x80 != 0;
        let end = marker & 0x40 != 0;
        if start && end {
            return None;
        }
        if start {
            if self.fragment.is_some() {
                return None;
            }
            self.fragment = Some(self.bytes.len());
            self.extend(&[0; 4])?;
            self.extend(nal_header)?;
        } else {
            let offset = self.fragment? + 4;
            if self.bytes.get(offset..offset + nal_header.len()) != Some(nal_header) {
                return None;
            }
        }
        self.extend(body)?;
        if end {
            let offset = self.fragment.take()?;
            let nal = self.bytes.get(offset + 4..)?;
            if matches!(
                (self.config.codec, kind),
                (VideoCodec::Avc, 7 | 8) | (VideoCodec::Hevc, 32..=34)
            ) {
                if nal.len() > MAX_PARAMETER_SET_BYTES {
                    return None;
                }
                let parameter = nal.to_vec();
                self.bytes.truncate(offset);
                self.pending_config.set_parameter_set(kind, &parameter)?;
            } else {
                let size = u32::try_from(nal.len()).ok()?;
                match self.config.codec {
                    VideoCodec::Avc => {
                        let header = slice_header(nal);
                        self.mark_nal(kind, header)?;
                    }
                    VideoCodec::Hevc => {
                        let header = nal.get(2).copied();
                        self.mark_hevc_nal(kind, header)?;
                    }
                }
                self.bytes[offset..offset + 4].copy_from_slice(&size.to_be_bytes());
            }
        }
        Some(())
    }

    fn extend(&mut self, bytes: &[u8]) -> Option<()> {
        if bytes.len() > MAX_ACCESS_UNIT_BYTES - self.bytes.len() {
            return None;
        }
        self.bytes.extend_from_slice(bytes);
        Some(())
    }
}

pub(crate) fn hevc_nal_type(data: &[u8]) -> Option<u8> {
    let &[first, second] = data.first_chunk::<2>()?;
    // Single-layer HEVC only, with a valid temporal_id_plus1.
    (first & 0x81 == 0 && second & 0xf8 == 0 && second & 7 != 0).then_some((first >> 1) & 0x3f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fragment_hides_dependent_frames_until_a_complete_idr() {
        let mut decoder = VideoAssembler::new(VideoConfig {
            sps: [(0, vec![0x67])].into(),
            pps: [(0, vec![0x68])].into(),
            ..VideoConfig::default()
        });
        let packet = |sequence, timestamp, marker, payload| Packet {
            payload_type: 96,
            ssrc: 1,
            sequence,
            timestamp,
            marker,
            payload,
        };
        assert!(
            decoder
                .push(&packet(65535, 10, false, &[0x7c, 0x85, 0xb0]))
                .is_none()
        );
        assert!(
            decoder
                .push(&packet(1, 10, true, &[0x7c, 0x45, 3]))
                .is_none()
        );
        assert!(decoder.push(&packet(2, 11, true, &[0x61, 0xc0])).is_none());
        let recovered = decoder
            .push(&packet(3, 12, true, &[0x65, 0xb0]))
            .expect("IDR");
        assert_eq!(recovered.timestamp, 12);
        assert_eq!(recovered.bytes, [0, 0, 0, 2, 0x65, 0xb0]);
        assert!(recovered.sync);
        assert_eq!(decoder.damaged_units, 1);
    }

    #[test]
    fn accepts_an_aggregation_and_fu_across_sequence_rollover() {
        let mut decoder = VideoAssembler::new(VideoConfig::default());
        let parse = |seq: u16, marker: bool, payload: &[u8]| {
            let mut data = vec![0x80, if marker { 224 } else { 96 }];
            data.extend(seq.to_be_bytes());
            data.extend([0, 0, 0, 12, 0, 0, 0, 1]);
            data.extend(payload);
            data
        };
        let sps = [0x67, 0x42, 0xc0, 0x1e, 0x80];
        let pps = [0x68, 0xc0];
        let data = parse(
            65534,
            false,
            &[0x78, 0, 5, 0x67, 0x42, 0xc0, 0x1e, 0x80, 0, 2, 0x68, 0xc0],
        );
        let first = Packet::parse(&data).expect("RTP");
        assert_eq!((first.payload_type, first.ssrc), (96, 1));
        assert!(decoder.push(&first).is_none());
        let data = parse(65535, false, &[0x7c, 0x85, 0xb0]);
        assert!(decoder.push(&Packet::parse(&data).expect("RTP")).is_none());
        let data = parse(0, true, &[0x7c, 0x45, 2]);
        let unit = decoder
            .push(&Packet::parse(&data).expect("RTP"))
            .expect("IDR");
        assert_eq!(unit.bytes, [0, 0, 0, 3, 0x65, 0xb0, 2]);
        assert_eq!(decoder.config.sps[&0], sps);
        assert_eq!(decoder.config.pps[&0], pps);
    }

    #[test]
    fn updating_one_in_band_parameter_set_preserves_other_ids() {
        let mut config = VideoConfig::default();
        config.set_parameter_set(8, &[0x68, 0xc0]).expect("PPS 0");
        config.set_parameter_set(8, &[0x68, 0x50]).expect("PPS 1");
        let mut decoder = VideoAssembler::new(config);
        assert!(
            decoder
                .push(&Packet {
                    payload_type: 96,
                    marker: true,
                    sequence: 1,
                    timestamp: 0,
                    ssrc: 1,
                    payload: &[0x68, 0xd0],
                })
                .is_none()
        );
        assert_eq!(decoder.config_changes, 1);
        assert_eq!(decoder.config.pps[&0], [0x68, 0xd0]);
        assert_eq!(decoder.config.pps[&1], [0x68, 0x50]);
    }
}
