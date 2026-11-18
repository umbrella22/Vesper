//! Bounded Harmony host contracts. Media execution remains in Media Kit AVPlayer.
#![deny(unsafe_code)]

mod dash_live;
mod downloads;
mod playback_manifest;
mod sources;
mod wire;

use player_platform_mobile::MobileSequenceBridgeSession;
use serde_json::{Value, json};

/// Maximum UTF-8 request and response size at the internal bridge boundary.
pub const MAX_JSON_BYTES: usize = 1024 * 1024;

/// One independent host-owned source registry or authoritative shared sequence.
pub enum HarmonySession {
    Sources(sources::SourceRegistry),
    Sequence(MobileSequenceBridgeSession),
    Dash,
    DashLive(dash_live::DashLive),
    PlaybackManifest,
    Downloads(downloads::Downloads),
}

/// Non-sensitive error envelope; source URIs and credentials are never included.
pub fn failure(code: &str, message: &str) -> Value {
    json!({"ok": false, "error": {"code": code, "message": message}})
}

pub(crate) fn success(value: Value) -> Value {
    json!({"ok": true, "value": value})
}

impl HarmonySession {
    /// Creates a session without any player, decoder, Surface or audio session.
    pub fn from_config_json(text: &str) -> Result<Self, Value> {
        if text.len() > MAX_JSON_BYTES {
            return Err(failure(
                "capacity",
                "configuration exceeds the bridge byte limit",
            ));
        }
        let value: Value = serde_json::from_str(text)
            .map_err(|_| failure("invalid_json", "configuration is not valid JSON"))?;
        match value.get("kind").and_then(Value::as_str) {
            Some("sources") => sources::SourceRegistry::new(&value).map(Self::Sources),
            Some("sequence") => {
                let config = value.get("configuration").cloned().unwrap_or(Value::Null);
                MobileSequenceBridgeSession::from_config_json(&config.to_string())
                    .map(Self::Sequence)
                    .map_err(|_| failure("invalid_argument", "sequence configuration is invalid"))
            }
            Some("dash") => Ok(Self::Dash),
            Some("dashLive") => Ok(Self::DashLive(dash_live::DashLive::default())),
            Some("playbackManifest") => Ok(Self::PlaybackManifest),
            Some("downloads") => downloads::Downloads::new(&value).map(Self::Downloads),
            _ => Err(failure("unsupported", "unknown session kind")),
        }
    }

    /// Executes bounded, in-memory work. This method performs no host I/O.
    pub fn execute_json(&mut self, text: &str, now_epoch_ms: u64) -> String {
        if text.len() > MAX_JSON_BYTES {
            return failure("capacity", "command exceeds the bridge byte limit").to_string();
        }
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return failure("invalid_json", "command is not valid JSON").to_string();
        };
        match self {
            Self::Sources(sources) => sources.execute(value, now_epoch_ms).to_string(),
            Self::Sequence(sequence) => {
                wire::sequence_response(&match value.get("op").and_then(Value::as_str) {
                    Some("snapshot") => sequence.snapshot_json(),
                    Some("events") => sequence.drain_events_json(256),
                    Some("preloadIntents") => sequence.preload_intents_json(now_epoch_ms),
                    Some("command") => {
                        let mut command = value.get("command").cloned().unwrap_or(Value::Null);
                        if let Err(error) = wire::sequence_numbers(&mut command, true) {
                            return error.to_string();
                        }
                        sequence.execute_json(&command.to_string(), now_epoch_ms)
                    }
                    _ => failure("unsupported", "unknown sequence bridge operation").to_string(),
                })
            }
            Self::Dash => wire::dash(text, &value),
            Self::DashLive(session) => match session.execute(&value) {
                Ok(value) => success(value).to_string(),
                Err(error) => error.to_string(),
            },
            Self::PlaybackManifest => match playback_manifest::project(&value) {
                Ok(value) => success(value).to_string(),
                Err(error) => error.to_string(),
            },
            Self::Downloads(downloads) => downloads.execute(&value).to_string(),
        }
    }
}
