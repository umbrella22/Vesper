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
    if uri.starts_with("file://")
        || uri.starts_with("content://")
        || is_likely_local_file_path(uri)
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
    use super::{MediaSource, MediaSourceKind, MediaSourceProtocol};

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
}
