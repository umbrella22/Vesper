#![deny(unsafe_code)]
//! Renders one HLS media playlist snapshot. Live producers own the fixed target
//! duration for each URI and decide whether a new window can replace the last one.

use std::{borrow::Cow, fmt, fmt::Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistKind {
    Live,
    Vod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationPrecision {
    Milliseconds,
    Microseconds,
}

#[derive(Debug, Clone, Copy)]
pub struct MediaPlaylistOptions {
    pub kind: PlaylistKind,
    pub target_duration: u64,
    pub media_sequence: u64,
    pub discontinuity_sequence: Option<u64>,
    pub independent_segments: bool,
    pub duration_precision: DurationPrecision,
}

/// A byte range with inclusive endpoints.
#[derive(Debug, Clone, Copy)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug)]
pub struct Initialization<'a> {
    pub uri: Cow<'a, str>,
    pub byte_range: Option<ByteRange>,
}

#[derive(Debug)]
pub struct MediaSegment<'a> {
    pub duration_seconds: f64,
    pub uri: Cow<'a, str>,
    pub byte_range: Option<ByteRange>,
    pub discontinuity: bool,
    /// Supplies an EXT-X-MAP before this segment. The current map otherwise carries forward.
    pub initialization: Option<Initialization<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistError {
    EmptyPlaylist,
    InvalidTargetDuration,
    InvalidSegmentDuration,
    TargetDurationExceeded { target_duration: u64 },
    InvalidUri(&'static str),
    InvalidByteRange(&'static str),
}

impl fmt::Display for PlaylistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPlaylist => f.write_str("media playlist must contain at least one segment"),
            Self::InvalidTargetDuration => f.write_str("target duration must be positive"),
            Self::InvalidSegmentDuration => {
                f.write_str("segment duration must be finite and positive")
            }
            Self::TargetDurationExceeded { target_duration } => {
                write!(
                    f,
                    "segment exceeds the playlist target duration of {target_duration} seconds"
                )
            }
            Self::InvalidUri(field) => write!(f, "invalid {field} URI"),
            Self::InvalidByteRange(field) => write!(f, "invalid byte range for {field}"),
        }
    }
}

impl std::error::Error for PlaylistError {}

impl DurationPrecision {
    /// Checks a duration using the same EXTINF representation as the writer.
    pub fn validate(self, seconds: f64, target_duration: u64) -> Result<(), PlaylistError> {
        self.format_extinf(seconds, target_duration).map(|_| ())
    }

    fn format_extinf(self, seconds: f64, target_duration: u64) -> Result<String, PlaylistError> {
        if target_duration == 0 {
            return Err(PlaylistError::InvalidTargetDuration);
        }
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err(PlaylistError::InvalidSegmentDuration);
        }
        let encoded = match self {
            Self::Milliseconds => format!("{seconds:.3}"),
            Self::Microseconds => format!("{seconds:.6}"),
        };
        // RFC 8216 compares the EXTINF sent to the player, rounded to whole seconds.
        let encoded_duration = encoded
            .parse::<f64>()
            .map_err(|_| PlaylistError::InvalidSegmentDuration)?;
        if encoded_duration <= 0.0 {
            return Err(PlaylistError::InvalidSegmentDuration);
        }
        let rounded = encoded_duration.round();
        if rounded >= u64::MAX as f64 || rounded as u64 > target_duration {
            return Err(PlaylistError::TargetDurationExceeded { target_duration });
        }
        Ok(encoded)
    }
}

/// Checks that a URI can occupy a standalone HLS playlist line.
pub fn validate_uri_line(uri: &str, field: &'static str) -> Result<(), PlaylistError> {
    let line = uri.trim_start();
    if line.is_empty() || line.starts_with('#') || uri.contains(['\r', '\n']) {
        return Err(PlaylistError::InvalidUri(field));
    }
    Ok(())
}

/// Renders a finite, caller-owned window with an explicit target duration.
/// An invalid segment returns an error without returning a partial playlist.
pub fn write_media_playlist<'a>(
    options: MediaPlaylistOptions,
    segments: impl ExactSizeIterator<Item = MediaSegment<'a>>,
) -> Result<String, PlaylistError> {
    if segments.len() == 0 {
        return Err(PlaylistError::EmptyPlaylist);
    }
    if options.target_duration == 0 {
        return Err(PlaylistError::InvalidTargetDuration);
    }
    let mut output = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:{}\n#EXT-X-MEDIA-SEQUENCE:{}\n",
        options.target_duration, options.media_sequence,
    );
    if let Some(sequence) = options.discontinuity_sequence {
        let _ = writeln!(output, "#EXT-X-DISCONTINUITY-SEQUENCE:{sequence}");
    }
    if options.independent_segments {
        output.push_str("#EXT-X-INDEPENDENT-SEGMENTS\n");
    }
    if options.kind == PlaylistKind::Vod {
        output.push_str("#EXT-X-PLAYLIST-TYPE:VOD\n");
    }

    for segment in segments {
        validate_uri_line(&segment.uri, "media segment")?;
        if segment.discontinuity {
            output.push_str("#EXT-X-DISCONTINUITY\n");
        }
        if let Some(initialization) = segment.initialization {
            write_initialization(&mut output, initialization)?;
        }
        let extinf = options
            .duration_precision
            .format_extinf(segment.duration_seconds, options.target_duration)?;
        let _ = writeln!(output, "#EXTINF:{extinf},");
        if let Some(range) = segment.byte_range {
            let length = byte_range_length(range, "media segment")?;
            let _ = writeln!(output, "#EXT-X-BYTERANGE:{length}@{}", range.start);
        }
        let _ = writeln!(output, "{}", segment.uri);
    }
    if options.kind == PlaylistKind::Vod {
        output.push_str("#EXT-X-ENDLIST\n");
    }
    Ok(output)
}

fn write_initialization(
    output: &mut String,
    initialization: Initialization<'_>,
) -> Result<(), PlaylistError> {
    if initialization.uri.is_empty() || initialization.uri.contains(['\r', '\n', '"']) {
        return Err(PlaylistError::InvalidUri("initialization"));
    }
    let _ = write!(output, "#EXT-X-MAP:URI=\"{}\"", initialization.uri);
    if let Some(range) = initialization.byte_range {
        let length = byte_range_length(range, "initialization")?;
        let _ = write!(output, ",BYTERANGE=\"{length}@{}\"", range.start);
    }
    output.push('\n');
    Ok(())
}

fn byte_range_length(range: ByteRange, field: &'static str) -> Result<u64, PlaylistError> {
    range
        .end
        .checked_sub(range.start)
        .and_then(|size| size.checked_add(1))
        .ok_or(PlaylistError::InvalidByteRange(field))
}
