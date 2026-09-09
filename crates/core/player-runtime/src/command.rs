use std::time::Duration;

use super::{DecodedVideoFrame, MediaAbrPolicy, MediaTrackSelection, PlayerSnapshot};

/// Command sent to a runtime adapter.
#[derive(Debug, Clone)]
pub enum PlayerRuntimeCommand {
    /// Start or resume playback.
    Play,
    /// Pause playback.
    Pause,
    /// Toggle between playing and paused states.
    TogglePause,
    /// Seek to an absolute media position.
    SeekTo { position: Duration },
    /// Set playback rate.
    SetPlaybackRate { rate: f32 },
    /// Set video track selection.
    SetVideoTrackSelection { selection: MediaTrackSelection },
    /// Set audio track selection.
    SetAudioTrackSelection { selection: MediaTrackSelection },
    /// Set subtitle track selection.
    SetSubtitleTrackSelection { selection: MediaTrackSelection },
    /// Set adaptive bitrate policy with an optional catalog revision precondition.
    SetAbrPolicy {
        policy: MediaAbrPolicy,
        expected_catalog_revision: Option<u64>,
    },
    /// Stop playback when the adapter supports it.
    Stop,
}

/// Result returned after a runtime command is dispatched.
#[derive(Debug)]
pub struct PlayerRuntimeCommandResult {
    /// Whether the adapter applied the command.
    pub applied: bool,
    /// Optional frame produced while applying the command.
    pub frame: Option<DecodedVideoFrame>,
    /// Snapshot after command handling.
    pub snapshot: PlayerSnapshot,
}
