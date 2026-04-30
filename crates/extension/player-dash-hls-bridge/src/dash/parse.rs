use std::collections::HashMap;

use crate::{
    dash::model::{
        ByteRange, DashAdaptationKind, DashAdaptationSet, DashManifest, DashPeriod,
        DashRepresentation, DashSegmentBase, DashSegmentTemplate, DashSegmentTimelineEntry,
    },
    error::{DashHlsError, DashHlsResult},
};

pub fn parse_mpd(input: &str) -> DashHlsResult<DashManifest> {
    parse_mpd_with_base_uri(input, None)
}

pub fn parse_mpd_with_base_uri(
    input: &str,
    manifest_uri: Option<&str>,
) -> DashHlsResult<DashManifest> {
    let document = parse_xml_document(input)?;
    let mpd = document
        .children
        .iter()
        .find(|child| child.local_name() == "MPD")
        .ok_or_else(|| DashHlsError::InvalidMpd("missing MPD root".to_owned()))?;

    let mpd_base = child_text(mpd, "BaseURL")
        .map(|base| resolve_uri(manifest_uri.unwrap_or_default(), base))
        .unwrap_or_else(|| manifest_uri.unwrap_or_default().to_owned());
    let mpd_type = mpd.attr("type").unwrap_or("static");
    if !mpd_type.eq_ignore_ascii_case("static") {
        return Err(DashHlsError::UnsupportedMpd(format!(
            "MPD type `{mpd_type}` is not supported"
        )));
    }
    let duration_ms = mpd
        .attr("mediaPresentationDuration")
        .and_then(parse_iso8601_duration_ms);
    let min_buffer_time_ms = mpd
        .attr("minBufferTime")
        .and_then(parse_iso8601_duration_ms);

    let mut periods = Vec::new();
    for period in mpd.children_named("Period") {
        periods.push(parse_period(period, &mpd_base)?);
    }

    if periods.is_empty() {
        return Err(DashHlsError::UnsupportedMpd(
            "MPD must contain at least one Period".to_owned(),
        ));
    }

    Ok(DashManifest {
        duration_ms,
        min_buffer_time_ms,
        periods,
    })
}

fn parse_period(node: &XmlNode, inherited_base_uri: &str) -> DashHlsResult<DashPeriod> {
    let period_base = child_text(node, "BaseURL")
        .map(|base| resolve_uri(inherited_base_uri, base))
        .unwrap_or_else(|| inherited_base_uri.to_owned());
    let mut adaptation_sets = Vec::new();

    for adaptation in node.children_named("AdaptationSet") {
        adaptation_sets.push(parse_adaptation_set(adaptation, &period_base)?);
    }

    Ok(DashPeriod {
        id: node.attr("id").map(str::to_owned),
        adaptation_sets,
    })
}

fn parse_adaptation_set(
    node: &XmlNode,
    inherited_base_uri: &str,
) -> DashHlsResult<DashAdaptationSet> {
    let adaptation_base = child_text(node, "BaseURL")
        .map(|base| resolve_uri(inherited_base_uri, base))
        .unwrap_or_else(|| inherited_base_uri.to_owned());
    let mime_type = node.attr("mimeType").map(str::to_owned);
    let kind = adaptation_kind(
        node.attr("contentType"),
        mime_type.as_deref(),
        node.attr("lang"),
    );
    let inherited_segment_base = parse_segment_base(node)?;
    let inherited_segment_template = parse_segment_template(node)?;
    let mut representations = Vec::new();

    for representation in node.children_named("Representation") {
        let id = representation
            .attr("id")
            .map(str::to_owned)
            .unwrap_or_else(|| format!("representation-{}", representations.len()));
        let base_url = child_text(representation, "BaseURL")
            .map(|base| resolve_uri(&adaptation_base, base))
            .unwrap_or_else(|| adaptation_base.clone());
        let representation_mime_type = representation
            .attr("mimeType")
            .map(str::to_owned)
            .or_else(|| mime_type.clone())
            .unwrap_or_default();
        let codecs = representation
            .attr("codecs")
            .or_else(|| node.attr("codecs"))
            .unwrap_or_default()
            .to_owned();
        let segment_base =
            parse_segment_base(representation)?.or_else(|| inherited_segment_base.clone());
        let segment_template =
            parse_segment_template(representation)?.or_else(|| inherited_segment_template.clone());

        representations.push(DashRepresentation {
            id,
            base_url,
            mime_type: representation_mime_type,
            codecs,
            bandwidth: representation.attr("bandwidth").and_then(parse_u64),
            width: representation.attr("width").and_then(parse_u32),
            height: representation.attr("height").and_then(parse_u32),
            frame_rate: representation.attr("frameRate").map(str::to_owned),
            audio_sampling_rate: representation.attr("audioSamplingRate").map(str::to_owned),
            segment_base,
            segment_template,
        });
    }

    Ok(DashAdaptationSet {
        id: node.attr("id").map(str::to_owned),
        kind,
        mime_type,
        language: node.attr("lang").map(str::to_owned),
        representations,
    })
}

fn adaptation_kind(
    content_type: Option<&str>,
    mime_type: Option<&str>,
    language: Option<&str>,
) -> DashAdaptationKind {
    let content_type = content_type.unwrap_or_default().to_ascii_lowercase();
    let mime_type = mime_type.unwrap_or_default().to_ascii_lowercase();
    if content_type == "video" || mime_type.starts_with("video/") {
        DashAdaptationKind::Video
    } else if content_type == "audio" || mime_type.starts_with("audio/") {
        DashAdaptationKind::Audio
    } else if content_type == "text"
        || content_type == "subtitle"
        || mime_type.contains("vtt")
        || language.is_some() && mime_type.starts_with("text/")
    {
        DashAdaptationKind::Subtitle
    } else {
        DashAdaptationKind::Unknown
    }
}

fn parse_segment_base(node: &XmlNode) -> DashHlsResult<Option<DashSegmentBase>> {
    let Some(segment_base) = node.children_named("SegmentBase").next() else {
        return Ok(None);
    };
    let Some(index_range_value) = segment_base.attr("indexRange") else {
        return Ok(None);
    };
    let Some(initialization_value) = segment_base
        .children_named("Initialization")
        .next()
        .and_then(|node| node.attr("range"))
    else {
        return Ok(None);
    };
    let index_range = parse_byte_range(index_range_value)?;
    let initialization = parse_byte_range(initialization_value)?;

    Ok(Some(DashSegmentBase {
        initialization,
        index_range,
    }))
}

fn parse_segment_template(node: &XmlNode) -> DashHlsResult<Option<DashSegmentTemplate>> {
    let Some(segment_template) = node.children_named("SegmentTemplate").next() else {
        return Ok(None);
    };
    let duration = parse_positive_u64(
        segment_template.attr("duration"),
        "SegmentTemplate duration",
    )?;
    let timescale = segment_template
        .attr("timescale")
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                DashHlsError::InvalidMpd("SegmentTemplate timescale must be positive".to_owned())
            })
        })
        .transpose()?
        .unwrap_or(1);
    if timescale == 0 {
        return Err(DashHlsError::InvalidMpd(
            "SegmentTemplate timescale must be positive".to_owned(),
        ));
    }
    let start_number = segment_template
        .attr("startNumber")
        .and_then(parse_u64)
        .unwrap_or(1);
    let presentation_time_offset = segment_template
        .attr("presentationTimeOffset")
        .and_then(parse_u64)
        .unwrap_or(0);
    let timeline = parse_segment_timeline(segment_template)?;
    if duration.is_none() && timeline.is_empty() {
        return Err(DashHlsError::UnsupportedMpd(
            "SegmentTemplate requires duration or SegmentTimeline".to_owned(),
        ));
    }
    let initialization = segment_template.attr("initialization").unwrap_or_default();
    let media = segment_template.attr("media").unwrap_or_default();
    if initialization.is_empty() || media.is_empty() {
        return Err(DashHlsError::UnsupportedMpd(
            "SegmentTemplate must provide initialization and media templates".to_owned(),
        ));
    }

    Ok(Some(DashSegmentTemplate {
        timescale,
        duration,
        start_number,
        presentation_time_offset,
        initialization: initialization.to_owned(),
        media: media.to_owned(),
        timeline,
    }))
}

fn parse_segment_timeline(node: &XmlNode) -> DashHlsResult<Vec<DashSegmentTimelineEntry>> {
    let Some(timeline) = node.children_named("SegmentTimeline").next() else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    for entry in timeline.children_named("S") {
        let duration =
            parse_positive_u64(entry.attr("d"), "SegmentTimeline S@d")?.ok_or_else(|| {
                DashHlsError::InvalidMpd(
                    "SegmentTimeline S must provide positive duration".to_owned(),
                )
            })?;
        let repeat_count = match entry.attr("r") {
            Some(value) => {
                let parsed = value.parse::<i32>().map_err(|_| {
                    DashHlsError::InvalidMpd(format!(
                        "invalid SegmentTimeline repeat count {value}"
                    ))
                })?;
                if parsed < -1 {
                    return Err(DashHlsError::InvalidMpd(format!(
                        "invalid SegmentTimeline repeat count {value}"
                    )));
                }
                parsed
            }
            None => 0,
        };
        entries.push(DashSegmentTimelineEntry {
            start_time: entry.attr("t").and_then(parse_u64),
            duration,
            repeat_count,
        });
    }
    if entries.is_empty() {
        return Err(DashHlsError::InvalidMpd(
            "SegmentTimeline must contain at least one S entry".to_owned(),
        ));
    }
    Ok(entries)
}

fn parse_positive_u64(value: Option<&str>, field: &str) -> DashHlsResult<Option<u64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parsed = value
        .parse::<u64>()
        .map_err(|_| DashHlsError::InvalidMpd(format!("{field} must be a positive integer")))?;
    if parsed == 0 {
        return Err(DashHlsError::InvalidMpd(format!(
            "{field} must be a positive integer"
        )));
    }
    Ok(Some(parsed))
}

fn parse_byte_range(value: &str) -> DashHlsResult<ByteRange> {
    let (start, end) = value
        .split_once('-')
        .ok_or_else(|| DashHlsError::InvalidMpd(format!("invalid byte range {value}")))?;
    let start = start
        .trim()
        .parse()
        .map_err(|_| DashHlsError::InvalidMpd(format!("invalid byte range {value}")))?;
    let end = end
        .trim()
        .parse()
        .map_err(|_| DashHlsError::InvalidMpd(format!("invalid byte range {value}")))?;
    if end < start {
        return Err(DashHlsError::InvalidMpd(format!(
            "invalid byte range {value}"
        )));
    }
    Ok(ByteRange { start, end })
}

fn parse_iso8601_duration_ms(value: &str) -> Option<u64> {
    let mut rest = value.strip_prefix('P')?;
    if let Some(date_time_split) = rest.find('T') {
        let date = &rest[..date_time_split];
        if !date.is_empty() {
            return None;
        }
        rest = &rest[date_time_split + 1..];
    } else {
        return None;
    }

    let mut number = String::new();
    let mut seconds = 0.0_f64;
    for ch in rest.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            number.push(ch);
            continue;
        }

        let value: f64 = number.parse().ok()?;
        number.clear();
        match ch {
            'H' => seconds += value * 3600.0,
            'M' => seconds += value * 60.0,
            'S' => seconds += value,
            _ => return None,
        }
    }

    if !number.is_empty() || !seconds.is_finite() || seconds < 0.0 {
        return None;
    }

    Some((seconds * 1000.0).round() as u64)
}

fn parse_u64(value: &str) -> Option<u64> {
    value.trim().parse().ok()
}

fn parse_u32(value: &str) -> Option<u32> {
    value.trim().parse().ok()
}

fn child_text<'a>(node: &'a XmlNode, name: &str) -> Option<&'a str> {
    node.children_named(name)
        .find_map(|child| (!child.text.trim().is_empty()).then(|| child.text.trim()))
}

fn resolve_uri(base_uri: &str, reference: &str) -> String {
    let reference = reference.trim();
    if reference.is_empty() {
        return base_uri.to_owned();
    }
    if has_uri_scheme(reference) || base_uri.is_empty() {
        return reference.to_owned();
    }

    if reference.starts_with('/') {
        if let Some(authority_end) = authority_end(base_uri) {
            return format!("{}{}", &base_uri[..authority_end], reference);
        }
        return reference.to_owned();
    }

    let base_dir = if base_uri.ends_with('/') {
        base_uri.to_owned()
    } else {
        match base_uri.rfind('/') {
            Some(index) => base_uri[..=index].to_owned(),
            None => String::new(),
        }
    };
    normalize_relative_uri(&(base_dir + reference))
}

fn has_uri_scheme(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };
    value[..colon]
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn authority_end(uri: &str) -> Option<usize> {
    let scheme_end = uri.find("://")? + 3;
    let path_start = uri[scheme_end..]
        .find('/')
        .map(|offset| scheme_end + offset)
        .unwrap_or(uri.len());
    Some(path_start)
}

fn normalize_relative_uri(uri: &str) -> String {
    let Some(authority_end) = authority_end(uri) else {
        return normalize_path(uri);
    };
    let prefix = &uri[..authority_end];
    let path = &uri[authority_end..];
    format!("{prefix}{}", normalize_path(path))
}

fn normalize_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let trailing = path.ends_with('/');
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }

    let mut normalized = String::new();
    if absolute {
        normalized.push('/');
    }
    normalized.push_str(&parts.join("/"));
    if trailing && !normalized.ends_with('/') {
        normalized.push('/');
    }
    if normalized.is_empty() {
        if absolute {
            "/".to_owned()
        } else {
            ".".to_owned()
        }
    } else {
        normalized
    }
}

#[derive(Debug, Clone)]
struct XmlNode {
    name: String,
    attributes: HashMap<String, String>,
    text: String,
    children: Vec<XmlNode>,
}

impl XmlNode {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attributes: HashMap::new(),
            text: String::new(),
            children: Vec::new(),
        }
    }

    fn local_name(&self) -> &str {
        local_name(&self.name)
    }

    fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str).or_else(|| {
            self.attributes
                .iter()
                .find_map(|(key, value)| (local_name(key) == name).then_some(value.as_str()))
        })
    }

    fn children_named<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a XmlNode> + 'a {
        let name = name.to_owned();
        self.children
            .iter()
            .filter(move |child| child.local_name() == name.as_str())
    }
}

fn local_name(name: &str) -> &str {
    name.rsplit_once(':')
        .map(|(_, local)| local)
        .unwrap_or(name)
}

fn parse_xml_document(input: &str) -> DashHlsResult<XmlNode> {
    let mut stack = vec![XmlNode::new("#document")];
    let mut cursor = 0;

    while let Some(tag_start_offset) = input[cursor..].find('<') {
        let tag_start = cursor + tag_start_offset;
        append_text(&mut stack, &input[cursor..tag_start]);

        if input[tag_start..].starts_with("<!--") {
            let end = input[tag_start + 4..]
                .find("-->")
                .ok_or_else(|| DashHlsError::InvalidMpd("unterminated XML comment".to_owned()))?;
            cursor = tag_start + 4 + end + 3;
            continue;
        }
        if input[tag_start..].starts_with("<?") {
            let end = input[tag_start + 2..].find("?>").ok_or_else(|| {
                DashHlsError::InvalidMpd("unterminated XML declaration".to_owned())
            })?;
            cursor = tag_start + 2 + end + 2;
            continue;
        }
        if input[tag_start..].starts_with("<!") {
            let end = find_tag_end(input, tag_start)?;
            cursor = end + 1;
            continue;
        }

        let tag_end = find_tag_end(input, tag_start)?;
        let body = input[tag_start + 1..tag_end].trim();
        if let Some(end_name) = body.strip_prefix('/') {
            close_node(&mut stack, end_name.trim())?;
        } else {
            let self_closing = body.ends_with('/');
            let body = if self_closing {
                body[..body.len().saturating_sub(1)].trim()
            } else {
                body
            };
            let (name, attributes) = parse_start_tag(body)?;
            let node = XmlNode {
                name,
                attributes,
                text: String::new(),
                children: Vec::new(),
            };
            if self_closing {
                let parent = stack.last_mut().ok_or_else(|| {
                    DashHlsError::InvalidMpd("XML parser stack underflow".to_owned())
                })?;
                parent.children.push(node);
            } else {
                stack.push(node);
            }
        }
        cursor = tag_end + 1;
    }

    append_text(&mut stack, &input[cursor..]);
    if stack.len() != 1 {
        return Err(DashHlsError::InvalidMpd(
            "unclosed XML element in MPD".to_owned(),
        ));
    }
    stack
        .pop()
        .ok_or_else(|| DashHlsError::InvalidMpd("empty XML parser stack".to_owned()))
}

fn append_text(stack: &mut [XmlNode], text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(current) = stack.last_mut() {
        current.text.push_str(&decode_xml_entities(text));
    }
}

fn close_node(stack: &mut Vec<XmlNode>, expected_name: &str) -> DashHlsResult<()> {
    if stack.len() <= 1 {
        return Err(DashHlsError::InvalidMpd(
            "unexpected XML closing tag".to_owned(),
        ));
    }
    let node = stack
        .pop()
        .ok_or_else(|| DashHlsError::InvalidMpd("XML parser stack underflow".to_owned()))?;
    if node.name != expected_name {
        return Err(DashHlsError::InvalidMpd(format!(
            "mismatched XML closing tag: expected {}, got {}",
            node.name, expected_name
        )));
    }
    let parent = stack
        .last_mut()
        .ok_or_else(|| DashHlsError::InvalidMpd("XML parser stack underflow".to_owned()))?;
    parent.children.push(node);
    Ok(())
}

fn find_tag_end(input: &str, tag_start: usize) -> DashHlsResult<usize> {
    let mut quote = None;
    for (offset, ch) in input[tag_start + 1..].char_indices() {
        match (quote, ch) {
            (Some(current), next) if current == next => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, '>') => return Ok(tag_start + 1 + offset),
            _ => {}
        }
    }
    Err(DashHlsError::InvalidMpd("unterminated XML tag".to_owned()))
}

fn parse_start_tag(body: &str) -> DashHlsResult<(String, HashMap<String, String>)> {
    let mut chars = body.char_indices();
    let name_end = chars
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
        .unwrap_or(body.len());
    let name = body[..name_end].trim();
    if name.is_empty() {
        return Err(DashHlsError::InvalidMpd(
            "empty XML element name".to_owned(),
        ));
    }
    let attributes = parse_attributes(&body[name_end..])?;
    Ok((name.to_owned(), attributes))
}

fn parse_attributes(input: &str) -> DashHlsResult<HashMap<String, String>> {
    let bytes = input.as_bytes();
    let mut cursor = 0;
    let mut attributes = HashMap::new();

    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }

        let key_start = cursor;
        while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() && bytes[cursor] != b'='
        {
            cursor += 1;
        }
        let key = input[key_start..cursor].trim();
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'=' {
            return Err(DashHlsError::InvalidMpd(format!(
                "missing value for XML attribute {key}"
            )));
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return Err(DashHlsError::InvalidMpd(format!(
                "missing value for XML attribute {key}"
            )));
        }

        let value = if matches!(bytes[cursor], b'"' | b'\'') {
            let quote = bytes[cursor];
            cursor += 1;
            let value_start = cursor;
            while cursor < bytes.len() && bytes[cursor] != quote {
                cursor += 1;
            }
            if cursor >= bytes.len() {
                return Err(DashHlsError::InvalidMpd(format!(
                    "unterminated XML attribute {key}"
                )));
            }
            let value = decode_xml_entities(&input[value_start..cursor]);
            cursor += 1;
            value
        } else {
            let value_start = cursor;
            while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            decode_xml_entities(&input[value_start..cursor])
        };
        attributes.insert(key.to_owned(), value);
    }

    Ok(attributes)
}

fn decode_xml_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_static_segment_base_vod_mpd() {
        let mpd = r#"
            <?xml version="1.0"?>
            <MPD type="static" mediaPresentationDuration="PT1M30.5S" minBufferTime="PT1.5S">
              <BaseURL>https://cdn.example.com/root/master.mpd</BaseURL>
              <Period id="p0">
                <AdaptationSet id="v" contentType="video" mimeType="video/mp4">
                  <BaseURL>video/</BaseURL>
                  <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720" frameRate="30000/1001">
                    <BaseURL>seg.m4s</BaseURL>
                    <SegmentBase indexRange="1000-1200">
                      <Initialization range="0-999"/>
                    </SegmentBase>
                  </Representation>
                </AdaptationSet>
                <AdaptationSet id="a" mimeType="audio/mp4" lang="ja">
                  <Representation id="a1" bandwidth="128000" codecs="mp4a.40.2" audioSamplingRate="48000">
                    <BaseURL>../audio/main.m4s</BaseURL>
                    <SegmentBase indexRange="800-950">
                      <Initialization range="0-799"/>
                    </SegmentBase>
                  </Representation>
                </AdaptationSet>
              </Period>
            </MPD>
        "#;

        let manifest =
            parse_mpd_with_base_uri(mpd, Some("https://origin.example.com/path/manifest.mpd"))
                .expect("valid MPD");

        assert_eq!(manifest.duration_ms, Some(90_500));
        assert_eq!(manifest.min_buffer_time_ms, Some(1_500));
        assert_eq!(manifest.periods.len(), 1);
        let video = &manifest.periods[0].adaptation_sets[0];
        assert_eq!(video.kind, DashAdaptationKind::Video);
        assert_eq!(
            video.representations[0].base_url,
            "https://cdn.example.com/root/video/seg.m4s"
        );
        assert_eq!(video.representations[0].width, Some(1280));
        assert_eq!(
            video.representations[0].segment_base,
            Some(DashSegmentBase {
                initialization: ByteRange::new(0, 999),
                index_range: ByteRange::new(1000, 1200),
            })
        );

        let audio = &manifest.periods[0].adaptation_sets[1];
        assert_eq!(audio.kind, DashAdaptationKind::Audio);
        assert_eq!(audio.language.as_deref(), Some("ja"));
        assert_eq!(
            audio.representations[0].base_url,
            "https://cdn.example.com/audio/main.m4s"
        );
    }

    #[test]
    fn rejects_missing_period() {
        let error = parse_mpd("<MPD />").expect_err("missing period should fail");
        assert!(matches!(error, DashHlsError::UnsupportedMpd(_)));
    }

    #[test]
    fn rejects_dynamic_mpd() {
        let error = parse_mpd(r#"<MPD type="dynamic"><Period /></MPD>"#)
            .expect_err("dynamic MPD should fail");
        assert!(matches!(error, DashHlsError::UnsupportedMpd(_)));
    }
}
