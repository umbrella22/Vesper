use serde_json::Value;

use crate::{failure, success};

// These fields carry u64 identities or quantities in the shared sequence wire.
// The Harmony wire represents them as decimal strings before JavaScript sees them.
fn decimal_field(key: &str) -> bool {
    matches!(
        key,
        "sessionGeneration"
            | "activationEpoch"
            | "sourceRevision"
            | "expectedSourceRevision"
            | "requestId"
            | "resolutionAttemptId"
            | "warmupTaskId"
            | "taskId"
            | "expectedMemoryBytes"
            | "expectedDiskBytes"
            | "expectedBytes"
            | "actualBytes"
            | "cacheBytes"
            | "evictedEntries"
    )
}

pub(crate) fn sequence_numbers(value: &mut Value, incoming: bool) -> Result<(), Value> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if decimal_field(key) && !child.is_null() {
                    if incoming {
                        let text = child.as_str().ok_or_else(|| {
                            failure(
                                "invalid_argument",
                                "sequence u64 fields require decimal strings",
                            )
                        })?;
                        if text.is_empty()
                            || text.len() > 20
                            || !text.bytes().all(|byte| byte.is_ascii_digit())
                        {
                            return Err(failure(
                                "invalid_argument",
                                "sequence u64 field is invalid",
                            ));
                        }
                        *child = Value::from(text.parse::<u64>().map_err(|_| {
                            failure(
                                "invalid_argument",
                                "sequence u64 field exceeds its representation",
                            )
                        })?);
                    } else if let Some(number) = child.as_u64() {
                        *child = Value::String(number.to_string());
                    }
                } else {
                    sequence_numbers(child, incoming)?;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                sequence_numbers(item, incoming)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn sequence_response(text: &str) -> String {
    let Ok(mut envelope) = serde_json::from_str::<Value>(text) else {
        return failure("invalid_response", "shared sequence returned invalid JSON").to_string();
    };
    if envelope["ok"] == true {
        let mut value = envelope
            .get_mut("result")
            .map(Value::take)
            .unwrap_or(Value::Null);
        if let Err(error) = sequence_numbers(&mut value, false) {
            return error.to_string();
        }
        success(value).to_string()
    } else {
        envelope.to_string()
    }
}

pub(crate) fn dash(text: &str, value: &Value) -> String {
    if !matches!(
        value["operation"].as_str(),
        Some(
            "parse_manifest"
                | "parse_sidx"
                | "selected_playable_representations"
                | "media_segments"
        )
    ) {
        return failure(
            "unsupported",
            "DASH operation is outside the startup profile",
        )
        .to_string();
    }
    if value["operation"] == "parse_manifest" {
        let mpd = value["mpd"].as_str().unwrap_or("");
        let mut reader = quick_xml::Reader::from_str(mpd);
        loop {
            use quick_xml::events::Event;
            match reader.read_event() {
                Ok(Event::Start(tag) | Event::Empty(tag)) => {
                    if tag.local_name().as_ref() == "ContentProtection" {
                        return failure(
                            "unsupported",
                            "protected DASH cannot enter the ordinary startup cache",
                        )
                        .to_string();
                    }
                    for attribute in tag.attributes() {
                        let Ok(attribute) = attribute else {
                            return failure("unsupported", "DASH XML attributes are invalid")
                                .to_string();
                        };
                        // Template presentation offsets are preserved by the
                        // shared model. Explicit Period starts still need mapping.
                        let unsupported_offset =
                            if attribute.key.as_ref() == "presentationTimeOffset" {
                                let offset = attribute
                                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                    .ok()
                                    .and_then(|value| value.trim().parse::<u64>().ok());
                                offset.is_none()
                                    || (tag.local_name().as_ref() != "SegmentTemplate"
                                        && offset != Some(0))
                            } else {
                                false
                            };
                        if (tag.local_name().as_ref() == "Period"
                            && attribute.key.as_ref() == "start")
                            || unsupported_offset
                        {
                            return failure("unsupported", "DASH Period starts or unrepresented presentation offsets are outside the Harmony playback profile").to_string();
                        }
                    }
                }
                Ok(Event::DocType(_)) | Err(_) => {
                    return failure("unsupported", "DASH XML is outside the startup profile")
                        .to_string();
                }
                Ok(Event::Eof) => break,
                _ => {}
            }
        }
    }
    match player_dash_hls_bridge::ops::execute_json(text) {
        Ok(output) if output.len() <= crate::MAX_JSON_BYTES - 64 => {
            match serde_json::from_str::<Value>(&output) {
                Ok(value) if exact_numbers(&value) => success(value).to_string(),
                _ => failure(
                    "unsupported",
                    "DASH values exceed the host's exact numeric representation",
                )
                .to_string(),
            }
        }
        Ok(_) => failure("capacity", "DASH output exceeds the bridge byte limit").to_string(),
        Err(_) => failure(
            "unsupported",
            "DASH input is invalid or outside the supported profile",
        )
        .to_string(),
    }
}

fn exact_numbers(value: &Value) -> bool {
    match value {
        Value::Number(number) => number
            .as_u64()
            .is_none_or(|number| number <= 9_007_199_254_740_991),
        Value::Array(items) => items.iter().all(exact_numbers),
        Value::Object(map) => map.values().all(exact_numbers),
        _ => true,
    }
}
