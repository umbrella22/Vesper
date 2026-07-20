//! Shared MPD fixtures for subtitle contract tests.
//!
//! These fixtures mirror the host's current MPD output shape byte-for-byte
//! (single text adaptation set with `mimeType="text/vtt"` and
//! `codecs="wvtt"`, single `<S d="...">` SegmentTimeline entry, no
//! `initialization` on subtitle SegmentTemplate, unique `Representation@id`).
//! They exist so the SDK cannot hide behind idealized remote `.vtt` fixtures
//! that bypass the real DASH-to-HLS bridge path.

/// Single WebVTT subtitle adaptation set with a unique representation id.
///
/// Mirrors `sampleWebVttSubtitleMpd` in
/// `lib/ios/VesperPlayerKit/Tests/VesperPlayerKitTests/VesperDashBridgeTestSupport.swift`
/// so Rust and iOS exercise the same input bytes.
pub fn webvtt_subtitle_single() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="subs" contentType="text" mimeType="text/vtt" lang="en">
      <SegmentTemplate timescale="1000" media="sub-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-en" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
  </Period>
</MPD>"#
}

/// Two WebVTT subtitle adaptation sets with different languages and unique
/// representation ids. Used to verify stable identity across multi-subtitle
/// manifests and reorder-resilient catalog publishing.
pub fn webvtt_subtitle_multi_language() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="subs-en" contentType="text" mimeType="text/vtt" lang="en">
      <SegmentTemplate timescale="1000" media="sub-en-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-en" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
    <AdaptationSet id="subs-zh" contentType="text" mimeType="text/vtt" lang="zh">
      <SegmentTemplate timescale="1000" media="sub-zh-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-zh" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
  </Period>
</MPD>"#
}

/// Two subtitle representations sharing the same `Representation@id`.
///
/// The plan (section 3.2) requires this to surface a structured
/// `subtitle_track_identity_ambiguous` failure rather than mask the collision
/// with a sequential `-2` suffix.
pub fn webvtt_subtitle_duplicate_id() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="subs-en" contentType="text" mimeType="text/vtt" lang="en">
      <SegmentTemplate timescale="1000" media="sub-en-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-en" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
    <AdaptationSet id="subs-zh" contentType="text" mimeType="text/vtt" lang="zh">
      <SegmentTemplate timescale="1000" media="sub-zh-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-en" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
  </Period>
</MPD>"#
}

/// Two subtitle adaptation sets both marked `<Role value="main"/>`.
///
/// The plan (section 3.3) requires this to surface a structured
/// `subtitle_default_track_ambiguous` failure rather than letting the
/// platform silently pick the first one.
pub fn webvtt_subtitle_multi_default() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="subs-en" contentType="text" mimeType="text/vtt" lang="en">
      <Role schemeIdUri="urn:mpeg:dash:role:2011" value="main"/>
      <SegmentTemplate timescale="1000" media="sub-en-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-en" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
    <AdaptationSet id="subs-zh" contentType="text" mimeType="text/vtt" lang="zh">
      <Role schemeIdUri="urn:mpeg:dash:role:2011" value="main"/>
      <SegmentTemplate timescale="1000" media="sub-zh-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-zh" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
  </Period>
</MPD>"#
}

/// Subtitle adaptation set using standard `<Label>` plus `<Role>` elements
/// for both default (`main`) and forced (`forced-subtitle`) narratives.
pub fn webvtt_subtitle_with_label_and_role() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="subs-en" contentType="text" mimeType="text/vtt" lang="en">
      <Label>English</Label>
      <Role schemeIdUri="urn:mpeg:dash:role:2011" value="main"/>
      <SegmentTemplate timescale="1000" media="sub-en-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-en" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
    <AdaptationSet id="subs-en-forced" contentType="text" mimeType="text/vtt" lang="en">
      <Label>English (Forced)</Label>
      <Role schemeIdUri="urn:mpeg:dash:role:2011" value="forced-subtitle"/>
      <SegmentTemplate timescale="1000" media="sub-en-forced-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-en-forced" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
  </Period>
</MPD>"#
}

/// Subtitle adaptation set using the legacy `AdaptationSet@label` attribute
/// only (no `<Label>` child, no `<Role>`). The SDK keeps reading this
/// compatibility attribute so existing host output works while hosts migrate
/// to standard elements.
pub fn webvtt_subtitle_legacy_label_attr() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="subs" contentType="text" mimeType="text/vtt" lang="en" label="English">
      <SegmentTemplate timescale="1000" media="sub-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-en" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
  </Period>
</MPD>"#
}

/// Subtitle adaptation set that resolves to an absolute `file://` URI for
/// its segment base. Mirrors the host's actual `bili_dash_manifest_builder`
/// output shape where the subtitle file is materialized at an absolute path
/// rather than a relative template. The fixture intentionally matches real
/// host output instead of an idealized remote `.vtt`.
pub fn webvtt_subtitle_absolute_file_uri() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="subs" contentType="text" mimeType="text/vtt" lang="en">
      <SegmentTemplate timescale="1000" media="sub-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-en" bandwidth="1200" codecs="wvtt">
        <BaseURL>file:///tmp/host-materialized/subtitle-en.vtt</BaseURL>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>"#
}
