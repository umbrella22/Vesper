pub mod master;
pub mod media;
pub mod model;

pub use master::{
    build_hls_master_input_from_dash_manifest, build_hls_master_playlist, format_hls_frame_rate,
};
pub use media::{build_hls_media_input_from_sidx, build_hls_media_playlist};
pub use model::{
    HlsAudioRendition, HlsMasterInput, HlsMediaInput, HlsMediaSegment, HlsResolution,
    HlsSubtitleRendition, HlsVariant,
};

use crate::{
    dash::ByteRange,
    error::{DashHlsError, DashHlsResult},
};

pub(crate) fn bool_attr(value: bool) -> &'static str {
    if value { "YES" } else { "NO" }
}

pub(crate) fn playlist_byte_range(range: &ByteRange) -> player_hls_playlist::ByteRange {
    player_hls_playlist::ByteRange {
        start: range.start,
        end: range.end,
    }
}

pub(crate) fn media_playlist_error(error: player_hls_playlist::PlaylistError) -> DashHlsError {
    match error {
        player_hls_playlist::PlaylistError::TargetDurationExceeded { target_duration } => {
            DashHlsError::TargetDurationExceeded { target_duration }
        }
        error => DashHlsError::InvalidHlsInput(error.to_string()),
    }
}

pub(crate) fn vod_target_duration(durations: impl Iterator<Item = f64>) -> u64 {
    durations.fold(1.0_f64, f64::max).ceil() as u64
}

pub(crate) fn ensure_line_value(value: &str, field: &str) -> DashHlsResult<()> {
    if value.is_empty() {
        return Err(DashHlsError::InvalidHlsInput(format!(
            "{field} must not be empty"
        )));
    }
    if value.contains('\r') || value.contains('\n') {
        return Err(DashHlsError::InvalidHlsInput(format!(
            "{field} must not contain line breaks"
        )));
    }
    Ok(())
}

pub(crate) fn quoted_attr(value: &str, field: &str) -> DashHlsResult<String> {
    ensure_line_value(value, field)?;
    if value.contains('"') {
        return Err(DashHlsError::InvalidHlsInput(format!(
            "{field} must not contain quote characters"
        )));
    }
    Ok(format!("\"{value}\""))
}
