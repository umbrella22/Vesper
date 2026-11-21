//! Native playback projection. Original cache/download manifests remain unchanged.
use std::collections::{BTreeMap, BTreeSet};

use quick_xml::{
    Reader, Writer,
    events::{BytesStart, BytesText, Event},
};
use serde_json::{Value, json};
use url::Url;

use crate::failure;

// The host's existing manifest budget also bounds the XML tree and HLS line table.
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_XML_DEPTH: usize = 32;
const MAX_XML_NODES: usize = 8192;
const MAX_ORIGINS: usize = 8;

fn unsupported() -> Value {
    failure(
        "unsupported",
        "manifest is outside the Harmony AVC/AAC playback profile",
    )
}

struct Projection {
    secure: bool,
    base: Url,
    routes: BTreeMap<String, String>,
    origins: BTreeSet<String>,
    excluded: BTreeSet<String>,
}

impl Projection {
    fn resource(&mut self, text: &str) -> Result<String, Value> {
        self.resource_at(text, &self.base.clone())
    }

    fn resource_at(&mut self, text: &str, base: &Url) -> Result<String, Value> {
        let uri = base.join(text).map_err(|_| unsupported())?;
        if !matches!(uri.scheme(), "http" | "https")
            || !uri.username().is_empty()
            || uri.password().is_some()
            || (self.secure && uri.scheme() != "https")
        {
            return Err(unsupported());
        }
        let origin = uri.origin().ascii_serialization();
        self.origins.insert(origin.clone());
        if self.origins.len() > MAX_ORIGINS {
            return Err(failure(
                "capacity",
                "playback manifest exceeds eight origins",
            ));
        }
        let Some(local) = self.routes.get(&origin) else {
            return Ok(text.to_owned());
        };
        // URL resolution precedes the local capability prefix, so root-relative
        // paths and any number of parent segments cannot escape that prefix.
        Ok(format!(
            "{local}{}{}",
            uri.path(),
            uri.query()
                .map(|query| format!("?{query}"))
                .unwrap_or_default()
        ))
    }

    fn supported(&mut self, mime: &str, codecs: &str) -> bool {
        let compatible = (mime.is_empty() || matches!(mime, "video/mp4" | "audio/mp4"))
            && codecs
                .split(',')
                .map(str::trim)
                .filter(|codec| !codec.is_empty())
                .all(|codec| {
                    codec.starts_with("avc1")
                        || codec.starts_with("avc3")
                        || matches!(codec, "mp4a.40.2" | "mp4a.40.5" | "mp4a.40.29" | "wvtt")
                });
        if !compatible && self.excluded.len() < 16 {
            // Diagnostics identify codec families, never resource URLs or access data.
            self.excluded.insert(format!(
                "{}:{}",
                mime.chars().take(64).collect::<String>(),
                codecs.chars().take(128).collect::<String>()
            ));
        }
        compatible
    }
}

pub(crate) fn project(value: &Value) -> Result<Value, Value> {
    let text = value["manifest"].as_str().ok_or_else(unsupported)?;
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(failure("capacity", "playback manifest exceeds 256 KiB"));
    }
    let source = Url::parse(value["manifestUrl"].as_str().ok_or_else(unsupported)?)
        .map_err(|_| unsupported())?;
    if !matches!(source.scheme(), "http" | "https") {
        return Err(unsupported());
    }
    let mut projection = Projection {
        secure: source.scheme() == "https",
        base: source,
        routes: BTreeMap::new(),
        origins: BTreeSet::new(),
        excluded: BTreeSet::new(),
    };
    if let Some(routes) = value.get("routes") {
        let routes = routes.as_object().ok_or_else(unsupported)?;
        if routes.len() > MAX_ORIGINS {
            return Err(unsupported());
        }
        for (origin, local) in routes {
            let local = local.as_str().ok_or_else(unsupported)?;
            let uri = Url::parse(local).map_err(|_| unsupported())?;
            if uri.scheme() != "http"
                || uri.host_str() != Some("127.0.0.1")
                || uri.port().is_none()
                || !uri.username().is_empty()
                || uri.password().is_some()
                || uri.query().is_some()
                || uri.fragment().is_some()
                || uri.to_string().trim_end_matches('/') != local
            {
                return Err(failure(
                    "invalid_argument",
                    "playback routes require loopback HTTP capability roots",
                ));
            }
            projection.routes.insert(origin.clone(), local.to_owned());
        }
    }
    let manifest = match value["protocol"].as_str() {
        Some("hls") => hls(text, &mut projection)?,
        Some("dash") => dash(text, &mut projection)?,
        _ => return Err(unsupported()),
    };
    let output = json!({"manifest": manifest, "origins": projection.origins, "excludedCodecs": projection.excluded});
    if output.to_string().len() > crate::MAX_JSON_BYTES - 64 {
        return Err(failure(
            "capacity",
            "playback projection exceeds the bridge byte limit",
        ));
    }
    Ok(output)
}

fn attributes(text: &str) -> Result<Vec<(String, String, bool)>, Value> {
    let mut result = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        if result.len() >= 64 {
            return Err(unsupported());
        }
        let (key, value) = remaining.split_once('=').ok_or_else(unsupported)?;
        if key.is_empty() || result.iter().any(|(name, _, _)| name == key) {
            return Err(unsupported());
        }
        let quoted = value.starts_with('"');
        let (value, rest) = if quoted {
            let end = value[1..].find('"').ok_or_else(unsupported)? + 1;
            let rest = &value[end + 1..];
            if !rest.is_empty() && !rest.starts_with(',') {
                return Err(unsupported());
            }
            (&value[1..end], rest.strip_prefix(',').unwrap_or(rest))
        } else {
            value.split_once(',').unwrap_or((value, ""))
        };
        result.push((key.to_owned(), value.to_owned(), quoted));
        remaining = rest;
    }
    Ok(result)
}

fn attribute<'a>(items: &'a [(String, String, bool)], name: &str) -> &'a str {
    items
        .iter()
        .find(|(key, _, _)| key == name)
        .map(|(_, value, _)| value.as_str())
        .unwrap_or("")
}

fn hls(text: &str, projection: &mut Projection) -> Result<String, Value> {
    if !text.starts_with("#EXTM3U") {
        return Err(unsupported());
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut keep = vec![true; lines.len()];
    let mut audio = BTreeSet::new();
    let mut variants = 0;
    let mut accepted = 0;
    for (index, line) in lines.iter().enumerate() {
        if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:")
            && attribute(&attributes(attrs)?, "METHOD") != "NONE"
        {
            return Err(failure(
                "unsupported",
                "protected HLS cannot enter ordinary playback transport",
            ));
        }
        if line.starts_with("#EXT-X-SESSION-KEY:") {
            return Err(unsupported());
        }
        if line.starts_with("#EXT-X-I-FRAME-STREAM-INF:") {
            // The playback projection carries full media variants. I-frame
            // renditions serve trick play and cannot make a master playable.
            variants += 1;
            keep[index] = false;
            continue;
        }
        if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let attrs = attributes(attrs)?;
            let compatible = projection.supported("", attribute(&attrs, "CODECS"));
            keep[index] = compatible;
            variants += 1;
            // Comments and blank lines may precede the variant URI.
            let child = (index + 1..lines.len())
                .find(|&next| !lines[next].is_empty() && !lines[next].starts_with('#'))
                .ok_or_else(unsupported)?;
            keep[child] = compatible;
            if compatible {
                accepted += 1;
                audio.insert(attribute(&attrs, "AUDIO").to_owned());
            }
        }
    }
    if variants > 0 && accepted == 0 {
        return Err(unsupported());
    }
    let mut output = String::new();
    for (index, line) in lines.iter().enumerate() {
        if !keep[index] {
            continue;
        }
        if let Some((tag, fields)) = line
            .split_once(':')
            .filter(|(tag, _)| tag.starts_with("#EXT"))
            && (fields.contains("URI=") || tag == "#EXT-X-MEDIA")
        {
            let mut attrs = attributes(fields)?;
            if tag == "#EXT-X-MEDIA"
                && attribute(&attrs, "TYPE") == "AUDIO"
                && variants > 0
                && !audio.contains(attribute(&attrs, "GROUP-ID"))
            {
                continue;
            }
            for (key, value, _) in &mut attrs {
                if key == "URI" {
                    *value = projection.resource(value)?;
                }
            }
            output.push_str(tag);
            output.push(':');
            for (offset, (key, value, quoted)) in attrs.iter().enumerate() {
                if offset > 0 {
                    output.push(',');
                }
                output.push_str(key);
                output.push('=');
                if *quoted {
                    output.push('"');
                }
                output.push_str(value);
                if *quoted {
                    output.push('"');
                }
            }
            output.push('\n');
            continue;
        }
        output.push_str(&if line.is_empty() || line.starts_with('#') {
            (*line).to_owned()
        } else {
            projection.resource(line)?
        });
        output.push('\n');
    }
    Ok(output)
}

enum Node {
    Element(Element),
    Raw(Event<'static>),
}
struct Element {
    tag: BytesStart<'static>,
    children: Vec<Node>,
}

fn xml_attribute(tag: &BytesStart<'_>, name: &str) -> Result<String, Value> {
    for item in tag.attributes() {
        let item = item.map_err(|_| unsupported())?;
        if item.key.as_ref() == name {
            return item
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|value| value.into_owned())
                .map_err(|_| unsupported());
        }
    }
    Ok(String::new())
}

fn xml_nodes(text: &str) -> Result<Vec<Node>, Value> {
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Element> = Vec::new();
    let mut roots = Vec::new();
    let mut count = 0;
    loop {
        count += 1;
        if count > MAX_XML_NODES || stack.len() > MAX_XML_DEPTH {
            return Err(failure("capacity", "playback XML exceeds its tree budget"));
        }
        let event = reader.read_event().map_err(|_| unsupported())?;
        if let Event::Start(tag) | Event::Empty(tag) = &event
            && (tag.local_name().as_ref() == "ContentProtection"
                || !xml_attribute(tag, "xml:base")?.is_empty())
        {
            return Err(unsupported());
        }
        let node = match event {
            Event::Start(tag) => {
                stack.push(Element {
                    tag: tag.into_owned(),
                    children: Vec::new(),
                });
                continue;
            }
            Event::Empty(tag) => Node::Element(Element {
                tag: tag.into_owned(),
                children: Vec::new(),
            }),
            Event::End(_) => Node::Element(stack.pop().ok_or_else(unsupported)?),
            Event::Eof => {
                if !stack.is_empty() {
                    return Err(unsupported());
                }
                break;
            }
            Event::DocType(_) => return Err(unsupported()),
            event => Node::Raw(event.into_owned()),
        };
        if let Some(parent) = stack.last_mut() {
            parent.children.push(node);
        } else {
            roots.push(node);
        }
    }
    Ok(roots)
}

fn filter_dash(element: &mut Element, projection: &mut Projection) -> Result<(), Value> {
    if element.tag.local_name().as_ref() == "ContentProtection" {
        return Err(unsupported());
    }
    if element.tag.local_name().as_ref() == "Period" {
        let mut retained = Vec::new();
        let mut media_seen = 0;
        let mut media_kept = 0;
        for child in element.children.drain(..) {
            let Node::Element(mut adaptation) = child else {
                retained.push(child);
                continue;
            };
            if adaptation.tag.local_name().as_ref() != "AdaptationSet" {
                retained.push(Node::Element(adaptation));
                continue;
            }
            let mime = xml_attribute(&adaptation.tag, "mimeType")?;
            let codecs = xml_attribute(&adaptation.tag, "codecs")?;
            let kind = xml_attribute(&adaptation.tag, "contentType")?;
            let mut children = Vec::new();
            let mut had_representations = false;
            let mut accepted = 0;
            for child in adaptation.children.drain(..) {
                if let Node::Element(representation) = &child
                    && representation.tag.local_name().as_ref() == "Representation"
                {
                    had_representations = true;
                    let declared_mime = xml_attribute(&representation.tag, "mimeType")?;
                    let declared_codecs = xml_attribute(&representation.tag, "codecs")?;
                    let mime = if declared_mime.is_empty() {
                        &mime
                    } else {
                        &declared_mime
                    };
                    let codecs = if declared_codecs.is_empty() {
                        &codecs
                    } else {
                        &declared_codecs
                    };
                    let media = matches!(kind.as_str(), "audio" | "video")
                        || mime.starts_with("audio/")
                        || mime.starts_with("video/");
                    if media {
                        media_seen += 1;
                    }
                    if media && !projection.supported(mime, codecs) {
                        continue;
                    }
                    if media {
                        media_kept += 1;
                    }
                    accepted += 1;
                }
                children.push(child);
            }
            if had_representations && accepted == 0 {
                continue;
            }
            adaptation.children = children;
            retained.push(Node::Element(adaptation));
        }
        element.children = retained;
        if media_seen > 0 && media_kept == 0 {
            return Err(unsupported());
        }
    }
    for child in &mut element.children {
        if let Node::Element(child) = child {
            filter_dash(child, projection)?;
        }
    }
    Ok(())
}

fn xml_text(element: &Element) -> Result<String, Value> {
    let mut result = String::new();
    for child in &element.children {
        match child {
            Node::Raw(Event::Text(text)) => result.push_str(
                &quick_xml::escape::unescape(&text.xml_content(quick_xml::XmlVersion::Implicit1_0))
                    .map_err(|_| unsupported())?,
            ),
            Node::Raw(Event::GeneralRef(reference)) => result.push_str(
                &quick_xml::escape::unescape(&format!("&{};", reference.as_ref()))
                    .map_err(|_| unsupported())?,
            ),
            Node::Raw(Event::CData(text)) => {
                result.push_str(&text.xml_content(quick_xml::XmlVersion::Implicit1_0))
            }
            Node::Raw(Event::Comment(_)) => {}
            _ => return Err(unsupported()),
        }
    }
    Ok(result.trim().to_owned())
}

fn write_node(
    node: &Node,
    writer: &mut Writer<Vec<u8>>,
    projection: &mut Projection,
    inherited_base: &Url,
) -> Result<(), Value> {
    let Node::Element(element) = node else {
        if let Node::Raw(event) = node {
            writer
                .write_event(event.clone())
                .map_err(|_| unsupported())?;
        }
        return Ok(());
    };
    let local = element.tag.local_name();
    let local = local.as_ref();
    let bases: Vec<&Element> = element
        .children
        .iter()
        .filter_map(|child| match child {
            Node::Element(child) if child.tag.local_name().as_ref() == "BaseURL" => Some(child),
            _ => None,
        })
        .collect();
    if bases.len() > 1 {
        return Err(failure(
            "unsupported",
            "parallel DASH BaseURL alternatives are outside the Harmony playback profile",
        ));
    }
    let base = match bases.first() {
        Some(child) => inherited_base
            .join(&xml_text(child)?)
            .map_err(|_| unsupported())?,
        None => inherited_base.clone(),
    };
    let mut tag = element.tag.clone();
    tag.clear_attributes();
    for item in element.tag.attributes() {
        let item = item.map_err(|_| unsupported())?;
        let resource = match local {
            "SegmentTemplate" => matches!(
                item.key.as_ref(),
                "media" | "initialization" | "index" | "bitstreamSwitching"
            ),
            "SegmentURL" => matches!(item.key.as_ref(), "media" | "index"),
            "Initialization" | "RepresentationIndex" => item.key.as_ref() == "sourceURL",
            _ => false,
        };
        if resource {
            let value = item
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|_| unsupported())?;
            let value = projection.resource_at(&value, &base)?;
            let name = item.key.as_ref();
            tag.push_attribute((name, value.as_str()));
        } else {
            tag.push_attribute(item);
        }
    }
    writer
        .write_event(Event::Start(tag))
        .map_err(|_| unsupported())?;
    if matches!(local, "BaseURL" | "Location") {
        let uri = projection.resource_at(&xml_text(element)?, inherited_base)?;
        writer
            .write_event(Event::Text(BytesText::new(&uri)))
            .map_err(|_| unsupported())?;
    } else {
        for child in &element.children {
            let child_base = match child {
                Node::Element(child) if child.tag.local_name().as_ref() == "BaseURL" => {
                    inherited_base
                }
                _ => &base,
            };
            write_node(child, writer, projection, child_base)?;
        }
    }
    writer
        .write_event(Event::End(element.tag.to_end()))
        .map_err(|_| unsupported())?;
    Ok(())
}

fn dash(text: &str, projection: &mut Projection) -> Result<String, Value> {
    let mut nodes = xml_nodes(text)?;
    let mut mpds = 0;
    for node in &mut nodes {
        if let Node::Element(element) = node {
            if element.tag.local_name().as_ref() != "MPD" {
                return Err(unsupported());
            }
            mpds += 1;
            filter_dash(element, projection)?;
        }
    }
    if mpds != 1 {
        return Err(unsupported());
    }
    let mut writer = Writer::new(Vec::new());
    for node in &nodes {
        write_node(node, &mut writer, projection, &projection.base.clone())?;
    }
    String::from_utf8(writer.into_inner()).map_err(|_| unsupported())
}
