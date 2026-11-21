//! Private wire contract shared by the AVPlayer host adapters.

use crate::{Bridge, BridgeError, PendingBridge, Phase, SessionRetirement};
use serde_json::{Value, json};

/// A host-owned bridge whose socket creation and ingest run in background workers.
pub enum Session {
    Opening(PendingBridge),
    Active(Bridge),
    Failed(BridgeError),
}

impl Session {
    pub fn start(uri: &str) -> Result<Self, BridgeError> {
        Bridge::start_in_background(uri).map(Self::Opening)
    }

    /// Retains the reservation's completion signal without retaining any worker.
    pub fn retirement(&self) -> Option<SessionRetirement> {
        match self {
            Self::Opening(pending) => Some(pending.retirement.clone()),
            Self::Active(bridge) => Some(bridge.shared.reservation.retirement.clone()),
            Self::Failed(_) => None,
        }
    }

    pub fn status(&mut self) -> Value {
        if let Self::Opening(pending) = self {
            match pending.poll() {
                Ok(None) => return json!({"phase": "connecting"}),
                Ok(Some(bridge)) => *self = Self::Active(bridge),
                Err(error) => *self = Self::Failed(error),
            }
        }
        match self {
            Self::Opening(_) => json!({"phase": "connecting"}),
            Self::Failed(error) => failure(error.code(), error.message()),
            Self::Active(bridge) => {
                let snapshot = bridge.snapshot();
                let phase = match snapshot.phase {
                    Phase::Connecting => "connecting",
                    Phase::Reconnecting => "reconnecting",
                    Phase::Ready => "ready",
                    Phase::Failed(error) => return failure(error.code(), error.message()),
                };
                let mut reply = json!({"phase": phase, "audioOmitted": snapshot.audio_omitted});
                if snapshot.phase == Phase::Ready {
                    reply["playlistUri"] = json!(bridge.playlist_url());
                }
                reply
            }
        }
    }
}

fn failure(code: &str, message: &str) -> Value {
    json!({"phase": "failed", "code": code, "message": message})
}
