use crate::error::{DashHlsError, DashHlsResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidxBox {
    pub timescale: u32,
    pub earliest_presentation_time: u64,
    pub first_offset: u64,
    pub references: Vec<SidxReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidxReference {
    pub reference_type: u8,
    pub referenced_size: u32,
    pub subsegment_duration: u32,
    pub starts_with_sap: bool,
    pub sap_type: u8,
    pub sap_delta_time: u32,
}

pub fn parse_sidx(data: &[u8]) -> DashHlsResult<SidxBox> {
    let mut cursor = 0;
    while cursor < data.len() {
        let header = Mp4BoxHeader::parse(data, cursor)?;
        if header.box_type == *b"sidx" {
            return parse_sidx_payload(&data[header.payload_start..header.end]);
        }
        cursor = header.end;
    }

    Err(DashHlsError::InvalidMp4(
        "missing sidx box in MP4 data".to_owned(),
    ))
}

pub fn remove_top_level_sidx_boxes(data: &[u8]) -> DashHlsResult<Vec<u8>> {
    let mut cursor = 0;
    let mut kept_ranges = Vec::new();
    let mut removed_sidx = false;

    while cursor < data.len() {
        let header = Mp4BoxHeader::parse(data, cursor)?;
        if header.box_type == *b"sidx" {
            removed_sidx = true;
        } else {
            kept_ranges.push(cursor..header.end);
        }
        cursor = header.end;
    }

    if !removed_sidx {
        return Ok(data.to_vec());
    }

    let mut output = Vec::with_capacity(data.len());
    for range in kept_ranges {
        output.extend_from_slice(&data[range]);
    }
    Ok(output)
}

fn parse_sidx_payload(payload: &[u8]) -> DashHlsResult<SidxBox> {
    let mut reader = Mp4Reader::new(payload);
    let version = reader.read_u8("sidx version")?;
    let _flags = reader.read_u24("sidx flags")?;
    let _reference_id = reader.read_u32("sidx reference_ID")?;
    let timescale = reader.read_u32("sidx timescale")?;
    if timescale == 0 {
        return Err(DashHlsError::InvalidMp4(
            "sidx timescale must be non-zero".to_owned(),
        ));
    }

    let (earliest_presentation_time, first_offset) = match version {
        0 => (
            u64::from(reader.read_u32("sidx earliest_presentation_time")?),
            u64::from(reader.read_u32("sidx first_offset")?),
        ),
        1 => (
            reader.read_u64("sidx earliest_presentation_time")?,
            reader.read_u64("sidx first_offset")?,
        ),
        _ => {
            return Err(DashHlsError::UnsupportedMp4(format!(
                "unsupported sidx version {version}"
            )));
        }
    };

    let _reserved = reader.read_u16("sidx reserved")?;
    let reference_count = reader.read_u16("sidx reference_count")?;
    let mut references = Vec::with_capacity(usize::from(reference_count));
    for _ in 0..reference_count {
        let reference = reader.read_u32("sidx reference")?;
        let subsegment_duration = reader.read_u32("sidx subsegment_duration")?;
        let sap = reader.read_u32("sidx SAP")?;
        references.push(SidxReference {
            reference_type: (reference >> 31) as u8,
            referenced_size: reference & 0x7fff_ffff,
            subsegment_duration,
            starts_with_sap: (sap & 0x8000_0000) != 0,
            sap_type: ((sap >> 28) & 0x07) as u8,
            sap_delta_time: sap & 0x0fff_ffff,
        });
    }

    Ok(SidxBox {
        timescale,
        earliest_presentation_time,
        first_offset,
        references,
    })
}

#[derive(Debug, Clone, Copy)]
struct Mp4BoxHeader {
    box_type: [u8; 4],
    payload_start: usize,
    end: usize,
}

impl Mp4BoxHeader {
    fn parse(data: &[u8], start: usize) -> DashHlsResult<Self> {
        let remaining = data.len().checked_sub(start).ok_or_else(|| {
            DashHlsError::InvalidMp4("MP4 box cursor is out of bounds".to_owned())
        })?;
        if remaining < 8 {
            return Err(DashHlsError::InvalidMp4(
                "truncated MP4 box header".to_owned(),
            ));
        }

        let size32 = read_u32_at(data, start, "MP4 box size")?;
        let box_type = read_box_type_at(data, start + 4)?;
        let (box_size, header_size) = match size32 {
            0 => (remaining, 8),
            1 => {
                if remaining < 16 {
                    return Err(DashHlsError::InvalidMp4(
                        "truncated extended MP4 box header".to_owned(),
                    ));
                }
                let size64 = read_u64_at(data, start + 8, "extended MP4 box size")?;
                let size = usize::try_from(size64).map_err(|_| {
                    DashHlsError::InvalidMp4("MP4 box size exceeds addressable memory".to_owned())
                })?;
                if size < 16 {
                    return Err(DashHlsError::InvalidMp4(
                        "extended MP4 box size is smaller than its header".to_owned(),
                    ));
                }
                (size, 16)
            }
            size if size < 8 => {
                return Err(DashHlsError::InvalidMp4(
                    "MP4 box size is smaller than its header".to_owned(),
                ));
            }
            size => (size as usize, 8),
        };

        if box_size > remaining {
            return Err(DashHlsError::InvalidMp4(
                "MP4 box exceeds input data".to_owned(),
            ));
        }

        Ok(Self {
            box_type,
            payload_start: start + header_size,
            end: start + box_size,
        })
    }
}

struct Mp4Reader<'a> {
    data: &'a [u8],
    cursor: usize,
}

impl<'a> Mp4Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, cursor: 0 }
    }

    fn read_u8(&mut self, field: &str) -> DashHlsResult<u8> {
        let value = *self
            .data
            .get(self.cursor)
            .ok_or_else(|| DashHlsError::InvalidMp4(format!("truncated MP4 field `{field}`")))?;
        self.cursor += 1;
        Ok(value)
    }

    fn read_u16(&mut self, field: &str) -> DashHlsResult<u16> {
        let end = self.cursor.checked_add(2).ok_or_else(|| {
            DashHlsError::InvalidMp4(format!("MP4 field `{field}` overflows cursor"))
        })?;
        let bytes = self
            .data
            .get(self.cursor..end)
            .ok_or_else(|| DashHlsError::InvalidMp4(format!("truncated MP4 field `{field}`")))?;
        self.cursor = end;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn read_u24(&mut self, field: &str) -> DashHlsResult<u32> {
        let end = self.cursor.checked_add(3).ok_or_else(|| {
            DashHlsError::InvalidMp4(format!("MP4 field `{field}` overflows cursor"))
        })?;
        let bytes = self
            .data
            .get(self.cursor..end)
            .ok_or_else(|| DashHlsError::InvalidMp4(format!("truncated MP4 field `{field}`")))?;
        self.cursor = end;
        Ok((u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]))
    }

    fn read_u32(&mut self, field: &str) -> DashHlsResult<u32> {
        let value = read_u32_at(self.data, self.cursor, field)?;
        self.cursor += 4;
        Ok(value)
    }

    fn read_u64(&mut self, field: &str) -> DashHlsResult<u64> {
        let value = read_u64_at(self.data, self.cursor, field)?;
        self.cursor += 8;
        Ok(value)
    }
}

fn read_u32_at(data: &[u8], offset: usize, field: &str) -> DashHlsResult<u32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| DashHlsError::InvalidMp4(format!("MP4 field `{field}` overflows input")))?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| DashHlsError::InvalidMp4(format!("truncated MP4 field `{field}`")))?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64_at(data: &[u8], offset: usize, field: &str) -> DashHlsResult<u64> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| DashHlsError::InvalidMp4(format!("MP4 field `{field}` overflows input")))?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| DashHlsError::InvalidMp4(format!("truncated MP4 field `{field}`")))?;
    Ok(u64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn read_box_type_at(data: &[u8], offset: usize) -> DashHlsResult<[u8; 4]> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| DashHlsError::InvalidMp4("MP4 box type overflows input".to_owned()))?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| DashHlsError::InvalidMp4("truncated MP4 box type".to_owned()))?;
    Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_0_sidx_after_other_boxes() {
        let mut input = mp4_box(*b"ftyp", &[0, 0, 0, 0]);
        input.extend(mp4_box(*b"sidx", &sidx_payload_v0()));

        let sidx = parse_sidx(&input).expect("valid sidx");

        assert_eq!(sidx.timescale, 1_000);
        assert_eq!(sidx.earliest_presentation_time, 500);
        assert_eq!(sidx.first_offset, 10);
        assert_eq!(
            sidx.references,
            vec![
                SidxReference {
                    reference_type: 0,
                    referenced_size: 100,
                    subsegment_duration: 2_000,
                    starts_with_sap: true,
                    sap_type: 1,
                    sap_delta_time: 0,
                },
                SidxReference {
                    reference_type: 0,
                    referenced_size: 150,
                    subsegment_duration: 3_000,
                    starts_with_sap: true,
                    sap_type: 2,
                    sap_delta_time: 5,
                },
            ]
        );
    }

    #[test]
    fn parses_version_1_sidx_with_extended_box_size() {
        let payload = sidx_payload_v1();
        let input = extended_mp4_box(*b"sidx", &payload);

        let sidx = parse_sidx(&input).expect("valid sidx");

        assert_eq!(sidx.timescale, 90_000);
        assert_eq!(sidx.earliest_presentation_time, 9_000_000_000);
        assert_eq!(sidx.first_offset, 24);
        assert_eq!(sidx.references.len(), 1);
        assert_eq!(sidx.references[0].referenced_size, 1_024);
    }

    #[test]
    fn rejects_truncated_box() {
        let mut input = mp4_box(*b"sidx", &sidx_payload_v0());
        input.truncate(input.len() - 2);

        let error = parse_sidx(&input).expect_err("truncated input should fail");

        assert!(matches!(error, DashHlsError::InvalidMp4(_)));
    }

    #[test]
    fn rejects_missing_sidx() {
        let input = mp4_box(*b"ftyp", &[0, 0, 0, 0]);

        let error = parse_sidx(&input).expect_err("missing sidx should fail");

        assert!(matches!(error, DashHlsError::InvalidMp4(_)));
    }

    fn sidx_payload_v0() -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend([0, 0, 0, 0]);
        payload.extend(1_u32.to_be_bytes());
        payload.extend(1_000_u32.to_be_bytes());
        payload.extend(500_u32.to_be_bytes());
        payload.extend(10_u32.to_be_bytes());
        payload.extend(0_u16.to_be_bytes());
        payload.extend(2_u16.to_be_bytes());
        push_reference(&mut payload, 100, 2_000, true, 1, 0);
        push_reference(&mut payload, 150, 3_000, true, 2, 5);
        payload
    }

    fn sidx_payload_v1() -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend([1, 0, 0, 0]);
        payload.extend(1_u32.to_be_bytes());
        payload.extend(90_000_u32.to_be_bytes());
        payload.extend(9_000_000_000_u64.to_be_bytes());
        payload.extend(24_u64.to_be_bytes());
        payload.extend(0_u16.to_be_bytes());
        payload.extend(1_u16.to_be_bytes());
        push_reference(&mut payload, 1_024, 90_000, true, 1, 0);
        payload
    }

    fn push_reference(
        payload: &mut Vec<u8>,
        referenced_size: u32,
        subsegment_duration: u32,
        starts_with_sap: bool,
        sap_type: u8,
        sap_delta_time: u32,
    ) {
        payload.extend((referenced_size & 0x7fff_ffff).to_be_bytes());
        payload.extend(subsegment_duration.to_be_bytes());
        let sap = (u32::from(starts_with_sap) << 31)
            | ((u32::from(sap_type) & 0x07) << 28)
            | (sap_delta_time & 0x0fff_ffff);
        payload.extend(sap.to_be_bytes());
    }

    fn mp4_box(box_type: [u8; 4], payload: &[u8]) -> Vec<u8> {
        let size = 8 + payload.len();
        let mut output = Vec::new();
        output.extend((size as u32).to_be_bytes());
        output.extend(box_type);
        output.extend(payload);
        output
    }

    fn extended_mp4_box(box_type: [u8; 4], payload: &[u8]) -> Vec<u8> {
        let size = 16 + payload.len();
        let mut output = Vec::new();
        output.extend(1_u32.to_be_bytes());
        output.extend(box_type);
        output.extend((size as u64).to_be_bytes());
        output.extend(payload);
        output
    }
}
