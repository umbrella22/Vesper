use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{failure, success};

struct Registration {
    descriptor: Value,
    expires_at_ms: Option<u64>,
    expiry_deadline: Option<Instant>,
    expired: Cell<bool>,
    descriptor_bytes: usize,
    closed: bool,
    invalidated: bool,
    lease_count: usize,
}

/// Source registrations and retained leases have independent identities.
pub struct SourceRegistry {
    limit: usize,
    next_source: u64,
    next_lease: u64,
    closed: bool,
    invalidated: bool,
    sources: BTreeMap<u64, Registration>,
    leases: BTreeMap<u64, u64>,
}

type Result<T> = std::result::Result<T, Value>;

fn argument() -> Value {
    failure("invalid_argument", "source command has an invalid field")
}

fn id(value: &Value, field: &str) -> Result<u64> {
    let text = value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(argument)?;
    if text.is_empty() || text.len() > 20 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(argument());
    }
    text.parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(argument)
}

pub(crate) fn validate_descriptor(value: &Value) -> Result<()> {
    if value.to_string().len() > 64 * 1024 {
        return Err(failure(
            "capacity",
            "source descriptor exceeds the bridge byte limit",
        ));
    }
    let uri = value
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(argument)?;
    if uri.is_empty() || uri.len() > 8192 || uri.chars().any(char::is_control) {
        return Err(argument());
    }
    if value
        .get("drmConfiguration")
        .is_some_and(|drm| !drm.is_null())
    {
        return Err(failure(
            "unsupported",
            "protected playback is not enabled on this route",
        ));
    }
    if !matches!(
        value.get("kind").and_then(Value::as_str),
        Some("local" | "remote")
    ) {
        return Err(failure("unsupported", "unknown source kind"));
    }
    if !matches!(
        value.get("protocol").and_then(Value::as_str),
        Some("file" | "progressive" | "hls" | "dash" | "rtsp")
    ) {
        return Err(failure(
            "unsupported",
            "source protocol has no Harmony execution route",
        ));
    }
    if let Some(headers) = value.get("headers") {
        let map = headers.as_object().ok_or_else(argument)?;
        if map.len() > 64 {
            return Err(failure("capacity", "too many request headers"));
        }
        let mut bytes = 0_usize;
        for (name, value) in map {
            let text = value.as_str().ok_or_else(argument)?;
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c))
                || text.contains(['\r', '\n', '\0'])
            {
                return Err(argument());
            }
            bytes += name.len() + text.len();
        }
        if bytes > 16 * 1024 {
            return Err(failure(
                "capacity",
                "request headers exceed their byte limit",
            ));
        }
    }
    if let Some(subtitles) = value.get("externalSubtitles") {
        let items = subtitles
            .as_array()
            .filter(|items| items.len() <= 16)
            .ok_or_else(argument)?;
        let mut identities = BTreeSet::new();
        for subtitle in items {
            let identity = subtitle["id"]
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
                .ok_or_else(argument)?;
            if !identities.insert(identity) {
                return Err(failure(
                    "invalid_argument",
                    "external subtitle identities must be unique within a source",
                ));
            }
            for flag in ["isDefault", "isForced"] {
                if subtitle.get(flag).is_some_and(|value| !value.is_boolean()) {
                    return Err(argument());
                }
            }
            for (field, maximum) in [("label", 2048), ("language", 64)] {
                if subtitle.get(field).is_some_and(|value| {
                    value.as_str().is_none_or(|text| {
                        text.len() > maximum || text.chars().any(char::is_control)
                    })
                }) {
                    return Err(argument());
                }
            }
            let uri = subtitle["uri"]
                .as_str()
                .filter(|uri| {
                    !uri.is_empty() && uri.len() <= 8192 && !uri.chars().any(char::is_control)
                })
                .ok_or_else(argument)?;
            if !matches!(
                subtitle["mimeType"].as_str(),
                Some("text/vtt" | "application/x-subrip")
            ) {
                return Err(failure(
                    "unsupported",
                    "external subtitles currently require SRT or WebVTT",
                ));
            }
            // Apply the same URI/header validation as the owning descriptor.
            let descriptor = serde_json::json!({"uri":uri,"kind":"remote","protocol":"progressive","headers":subtitle.get("headers").cloned().unwrap_or_else(|| serde_json::json!({}))});
            validate_descriptor(&descriptor)?;
        }
    }
    Ok(())
}

impl SourceRegistry {
    pub(crate) fn new(config: &Value) -> Result<Self> {
        let limit = match config.get("maxSources") {
            None => 128,
            Some(value) => value
                .as_u64()
                .filter(|limit| (1..=512).contains(limit))
                .ok_or_else(argument)? as usize,
        };
        Ok(Self {
            limit,
            next_source: 1,
            next_lease: 1,
            closed: false,
            invalidated: false,
            sources: BTreeMap::new(),
            leases: BTreeMap::new(),
        })
    }

    fn validate_access(&self, source: u64, now: u64) -> Result<&Registration> {
        let source = self
            .sources
            .get(&source)
            .ok_or_else(|| failure("stale_source", "source registration is unavailable"))?;
        if self.invalidated || source.invalidated {
            return Err(failure("invalidated", "source access was invalidated"));
        }
        if source.expired.get()
            || source.expires_at_ms.is_some_and(|expiry| expiry <= now)
            || source
                .expiry_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            source.expired.set(true);
            return Err(failure("expired", "source registration has expired"));
        }
        Ok(source)
    }

    fn collect_closed(&mut self) {
        self.sources
            .retain(|_, source| !source.closed || source.lease_count > 0);
    }

    pub(crate) fn execute(&mut self, value: Value, now: u64) -> Value {
        match self.try_execute(&value, now) {
            Ok(value) => success(value),
            Err(error) => error,
        }
    }

    fn try_execute(&mut self, value: &Value, now: u64) -> Result<Value> {
        match value.get("op").and_then(Value::as_str) {
            Some("register") => {
                if self.closed {
                    return Err(failure("closed", "source session is closed"));
                }
                if self.sources.len() >= self.limit {
                    return Err(failure("capacity", "source registry capacity reached"));
                }
                let descriptor = value.get("source").ok_or_else(argument)?;
                validate_descriptor(descriptor)?;
                let descriptor_bytes = descriptor.to_string().len();
                if self
                    .sources
                    .values()
                    .map(|source| source.descriptor_bytes)
                    .sum::<usize>()
                    + descriptor_bytes
                    > 8 * 1024 * 1024
                {
                    return Err(failure(
                        "capacity",
                        "source registry descriptor byte budget reached",
                    ));
                }
                let expiry = match value.get("expiresAtEpochMs") {
                    None | Some(Value::Null) => None,
                    Some(value) => Some(
                        value
                            .as_u64()
                            .filter(|expiry| *expiry <= 9_007_199_254_740_991)
                            .ok_or_else(argument)?,
                    ),
                };
                if expiry.is_some_and(|expiry| expiry <= now) {
                    return Err(failure("expired", "source registration has expired"));
                }
                let expiry_deadline = expiry
                    .map(|expiry| {
                        Instant::now()
                            .checked_add(Duration::from_millis(expiry - now))
                            .ok_or_else(argument)
                    })
                    .transpose()?;
                let next = self
                    .next_source
                    .checked_add(1)
                    .ok_or_else(|| failure("capacity", "source identity space exhausted"))?;
                let id = self.next_source;
                self.sources.insert(
                    id,
                    Registration {
                        descriptor: descriptor.clone(),
                        expires_at_ms: expiry,
                        expiry_deadline,
                        expired: Cell::new(false),
                        descriptor_bytes,
                        closed: false,
                        invalidated: false,
                        lease_count: 0,
                    },
                );
                self.next_source = next;
                Ok(json!({"sourceId": id.to_string(), "sourceRevision": "1"}))
            }
            Some("acquire" | "retainLease") => {
                let retaining = value["op"] == "retainLease";
                let source_id = if retaining {
                    *self
                        .leases
                        .get(&id(value, "leaseId")?)
                        .ok_or_else(|| failure("stale_lease", "source lease was released"))?
                } else {
                    id(value, "sourceId")?
                };
                let source = self.validate_access(source_id, now)?;
                if !retaining && (source.closed || self.closed) {
                    return Err(failure("closed", "source registration is closed"));
                }
                if self.leases.len() >= 512 {
                    return Err(failure("capacity", "source lease capacity reached"));
                }
                let descriptor = source.descriptor.clone();
                let expiry = source.expires_at_ms;
                let lease = self.next_lease;
                self.next_lease = lease
                    .checked_add(1)
                    .ok_or_else(|| failure("capacity", "lease identity space exhausted"))?;
                self.leases.insert(lease, source_id);
                if let Some(source) = self.sources.get_mut(&source_id) {
                    source.lease_count += 1;
                }
                Ok(
                    json!({"sourceId": source_id.to_string(), "leaseId": lease.to_string(), "descriptor": descriptor, "expiresAtEpochMs": expiry}),
                )
            }
            Some("validateLease") => {
                let lease = id(value, "leaseId")?;
                let source_id = *self
                    .leases
                    .get(&lease)
                    .ok_or_else(|| failure("stale_lease", "source lease was released"))?;
                self.validate_access(source_id, now)?;
                Ok(json!({"sourceId": source_id.to_string(), "sourceRevision": "1"}))
            }
            Some("releaseLease") => {
                let lease = id(value, "leaseId")?;
                if let Some(source_id) = self.leases.remove(&lease)
                    && let Some(source) = self.sources.get_mut(&source_id)
                {
                    source.lease_count -= 1;
                }
                self.collect_closed();
                Ok(json!({}))
            }
            Some("closeSource" | "invalidateSource") => {
                let source_id = id(value, "sourceId")?;
                if let Some(source) = self.sources.get_mut(&source_id) {
                    source.closed = true;
                    source.invalidated |= value["op"] == "invalidateSource";
                }
                self.collect_closed();
                Ok(json!({}))
            }
            Some("close" | "invalidate") => {
                self.closed = true;
                self.invalidated |= value["op"] == "invalidate";
                for source in self.sources.values_mut() {
                    source.closed = true;
                }
                self.collect_closed();
                Ok(json!({}))
            }
            Some("snapshot") => Ok(
                json!({"closed": self.closed, "invalidated": self.invalidated,
                "registrations": self.sources.len(), "leases": self.leases.len(), "capacity": self.limit}),
            ),
            _ => Err(failure("unsupported", "unknown source registry operation")),
        }
    }
}
