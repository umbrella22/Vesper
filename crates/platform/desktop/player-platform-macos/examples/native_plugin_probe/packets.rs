use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use player_plugin::*;
use player_plugin_loader::LoadedNativePlugin;

const PACKETS: usize = 300;
const WARMUP: usize = 16;

pub fn config(source: &Path) -> Result<SourceNormalizerPacketSessionConfig> {
    let source = source.canonicalize()?;
    Ok(SourceNormalizerPacketSessionConfig {
        runtime_profile: "generic-fallback".into(),
        input: source.to_str().context("fixture path is not UTF-8")?.into(),
        headers: Vec::new(),
        startup_timeout_ms: Some(5_000),
        session_timeout_ms: Some(30_000),
        preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
    })
}

pub fn open(
    plugins: &Path,
    config: &SourceNormalizerPacketSessionConfig,
) -> Result<Box<dyn SourceNormalizerPacketSession>> {
    let normalizer = LoadedNativePlugin::load_development(
        plugins.join("libvesper_source_normalizer_ffmpeg.dylib"),
    )?;
    let reference = PluginReference::new(normalizer.plugin_id(), None, PluginTransport::Native)?;
    Ok(normalizer
        .resolve_source_packet(&reference)?
        .open_packet_session(config)?)
}

pub fn verify(plugins: &Path, source: &Path) -> Result<()> {
    let mut session = open(plugins, &config(source)?)?;
    let info = session.stream_info();
    ensure!(
        info.seekable && info.tracks.len() == 2,
        "unexpected packet tracks"
    );
    let stream = info
        .selected_track_index
        .context("normalizer selected no track")?;
    ensure!(
        info.tracks.iter().any(|track| {
            track.stream_index == stream
                && track.media_kind == SourceNormalizerPacketMediaKind::Video
        }),
        "normalizer selected no video track"
    );
    let first = first_video_keyframe(&mut *session, stream)?;
    for _ in 1..WARMUP + PACKETS {
        let packet = session.read_packet()?;
        ensure!(
            packet.metadata.packet.is_some(),
            "packet stream ended early"
        );
        let handle = packet.handle;
        session.release_packet(handle)?;
    }
    session.seek(&SourceNormalizerPacketSeek {
        position_millis: 0,
        exact: false,
    })?;
    ensure!(
        first_video_keyframe(&mut *session, stream)? == first,
        "seek to the start returned a different video keyframe"
    );
    session.close()?;
    Ok(())
}

fn first_video_keyframe(
    session: &mut dyn SourceNormalizerPacketSession,
    stream: u32,
) -> Result<(Option<i64>, Vec<u8>)> {
    // AAC priming can precede the first video packet and be skipped by a seek to zero.
    for _ in 0..WARMUP {
        let packet = session.read_packet()?;
        let metadata = packet
            .metadata
            .packet
            .context("packet stream ended before video")?;
        let selected = metadata.stream_index == stream;
        let keyframe =
            selected.then(|| (metadata.key_frame, metadata.pts_us, packet.data.to_vec()));
        let handle = packet.handle;
        session.release_packet(handle)?;
        if let Some((key_frame, pts_us, data)) = keyframe {
            ensure!(
                key_frame && !data.is_empty(),
                "invalid first video keyframe"
            );
            return Ok((pts_us, data));
        }
    }
    bail!("no video packet within the probe startup budget")
}
