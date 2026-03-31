use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSourceKind {
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSourceProtocol {
    Unknown,
    File,
    Content,
    Progressive,
    Hls,
    Dash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSource {
    uri: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaTrackKind {
    Video,
    Audio,
    Subtitle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaTrack {
    pub id: String,
    pub kind: MediaTrackKind,
    pub label: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub bit_rate: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
    pub channels: Option<u16>,
    pub sample_rate: Option<u32>,
    pub is_default: bool,
    pub is_forced: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MediaTrackCatalog {
    pub tracks: Vec<MediaTrack>,
    pub adaptive_video: bool,
    pub adaptive_audio: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaTrackSelectionMode {
    Auto,
    Disabled,
    Track,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaTrackSelection {
    pub mode: MediaTrackSelectionMode,
    pub track_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAbrMode {
    Auto,
    Constrained,
    FixedTrack,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaAbrPolicy {
    pub mode: MediaAbrMode,
    pub track_id: Option<String>,
    pub max_bit_rate: Option<u64>,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaTrackSelectionSnapshot {
    pub video: MediaTrackSelection,
    pub audio: MediaTrackSelection,
    pub subtitle: MediaTrackSelection,
    pub abr_policy: MediaAbrPolicy,
}

impl MediaSource {
    pub fn new(uri: impl Into<String>) -> Self {
        Self { uri: uri.into() }
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn kind(&self) -> MediaSourceKind {
        classify_media_source_kind(&self.uri)
    }

    pub fn protocol(&self) -> MediaSourceProtocol {
        classify_media_source_protocol(&self.uri)
    }
}

impl MediaTrackSelection {
    pub fn auto() -> Self {
        Self {
            mode: MediaTrackSelectionMode::Auto,
            track_id: None,
        }
    }

    pub fn disabled() -> Self {
        Self {
            mode: MediaTrackSelectionMode::Disabled,
            track_id: None,
        }
    }

    pub fn track(track_id: impl Into<String>) -> Self {
        Self {
            mode: MediaTrackSelectionMode::Track,
            track_id: Some(track_id.into()),
        }
    }
}

impl Default for MediaTrackSelection {
    fn default() -> Self {
        Self::auto()
    }
}

impl Default for MediaAbrPolicy {
    fn default() -> Self {
        Self {
            mode: MediaAbrMode::Auto,
            track_id: None,
            max_bit_rate: None,
            max_width: None,
            max_height: None,
        }
    }
}

impl Default for MediaTrackSelectionSnapshot {
    fn default() -> Self {
        Self {
            video: MediaTrackSelection::auto(),
            audio: MediaTrackSelection::auto(),
            subtitle: MediaTrackSelection::disabled(),
            abr_policy: MediaAbrPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackState {
    Idle,
    Loading,
    Ready,
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone)]
pub struct DecodedVideoFrame {
    pub presentation_time: Duration,
    pub width: u32,
    pub height: u32,
    pub bytes_per_row: u32,
    pub pixel_format: VideoPixelFormat,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoPixelFormat {
    Rgba8888,
    Yuv420p,
}

fn classify_media_source_kind(uri: &str) -> MediaSourceKind {
    if uri.starts_with("file://") || uri.starts_with("content://") || is_likely_local_file_path(uri)
    {
        MediaSourceKind::Local
    } else {
        MediaSourceKind::Remote
    }
}

fn classify_media_source_protocol(uri: &str) -> MediaSourceProtocol {
    let lower = uri.to_ascii_lowercase();
    let lower_without_fragment = lower
        .split_once('#')
        .map(|(path, _)| path)
        .unwrap_or(lower.as_str());
    let lower_path = lower_without_fragment
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(lower_without_fragment);

    if lower.starts_with("file://") {
        return MediaSourceProtocol::File;
    }

    if lower.starts_with("content://") {
        return MediaSourceProtocol::Content;
    }

    if is_likely_local_file_path(uri) {
        return MediaSourceProtocol::File;
    }

    if lower_path.ends_with(".m3u8") {
        return MediaSourceProtocol::Hls;
    }

    if lower_path.ends_with(".mpd") {
        return MediaSourceProtocol::Dash;
    }

    if lower.starts_with("http://") || lower.starts_with("https://") {
        return MediaSourceProtocol::Progressive;
    }

    MediaSourceProtocol::Unknown
}

fn is_likely_local_file_path(uri: &str) -> bool {
    if uri.is_empty() {
        return false;
    }

    if uri.starts_with('/') || uri.starts_with("./") || uri.starts_with("../") {
        return true;
    }

    if uri.starts_with("\\\\") || uri.starts_with(".\\") || uri.starts_with("..\\") {
        return true;
    }

    let bytes = uri.as_bytes();
    if bytes.len() >= 3
        && bytes[1] == b':'
        && bytes[0].is_ascii_alphabetic()
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }

    !uri.contains("://") && !uri.starts_with("content:")
}

#[cfg(test)]
mod tests {
    use super::{
        MediaAbrMode, MediaSource, MediaSourceKind, MediaSourceProtocol, MediaTrackSelection,
        MediaTrackSelectionMode, MediaTrackSelectionSnapshot,
    };

    #[test]
    fn classifies_local_sources() {
        let file_source = MediaSource::new("file:///tmp/video.mp4");
        assert_eq!(file_source.kind(), MediaSourceKind::Local);
        assert_eq!(file_source.protocol(), MediaSourceProtocol::File);

        let content_source = MediaSource::new("content://media/external/video/1");
        assert_eq!(content_source.kind(), MediaSourceKind::Local);
        assert_eq!(content_source.protocol(), MediaSourceProtocol::Content);

        let unix_path = MediaSource::new("/tmp/video.mp4");
        assert_eq!(unix_path.kind(), MediaSourceKind::Local);
        assert_eq!(unix_path.protocol(), MediaSourceProtocol::File);

        let relative_path = MediaSource::new("fixtures/video.mp4");
        assert_eq!(relative_path.kind(), MediaSourceKind::Local);
        assert_eq!(relative_path.protocol(), MediaSourceProtocol::File);
    }

    #[test]
    fn classifies_remote_streaming_sources() {
        let hls = MediaSource::new("https://example.com/master.m3u8");
        assert_eq!(hls.kind(), MediaSourceKind::Remote);
        assert_eq!(hls.protocol(), MediaSourceProtocol::Hls);

        let dash = MediaSource::new("https://example.com/manifest.mpd");
        assert_eq!(dash.protocol(), MediaSourceProtocol::Dash);

        let hls_with_query = MediaSource::new("https://example.com/master.m3u8?token=abc");
        assert_eq!(hls_with_query.protocol(), MediaSourceProtocol::Hls);

        let dash_with_fragment =
            MediaSource::new("https://example.com/manifest.mpd#representation");
        assert_eq!(dash_with_fragment.protocol(), MediaSourceProtocol::Dash);

        let progressive = MediaSource::new("https://example.com/video.mp4");
        assert_eq!(progressive.protocol(), MediaSourceProtocol::Progressive);
    }

    #[test]
    fn track_selection_helpers_build_expected_modes() {
        assert_eq!(
            MediaTrackSelection::auto().mode,
            MediaTrackSelectionMode::Auto
        );
        assert_eq!(
            MediaTrackSelection::disabled().mode,
            MediaTrackSelectionMode::Disabled
        );

        let selected = MediaTrackSelection::track("video-main");
        assert_eq!(selected.mode, MediaTrackSelectionMode::Track);
        assert_eq!(selected.track_id.as_deref(), Some("video-main"));
    }

    #[test]
    fn default_track_selection_snapshot_starts_in_auto_mode() {
        let snapshot = MediaTrackSelectionSnapshot::default();

        assert_eq!(snapshot.video.mode, MediaTrackSelectionMode::Auto);
        assert_eq!(snapshot.audio.mode, MediaTrackSelectionMode::Auto);
        assert_eq!(snapshot.subtitle.mode, MediaTrackSelectionMode::Disabled);
        assert_eq!(snapshot.abr_policy.mode, MediaAbrMode::Auto);
    }
}
