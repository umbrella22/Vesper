use std::path::Path;

use anyhow::{Context, Result};
use player_core::{MediaSource, MediaSourceKind, MediaSourceProtocol};
use player_render_wgpu::RenderSurfaceConfig;
use player_runtime::{
    DEFAULT_VIDEO_PREFETCH_CAPACITY, PlayerMediaInfo, PlayerRuntimeAdapterCapabilities,
    PlayerRuntimeBootstrap, PlayerRuntimeOptions, PlayerRuntimeStartup,
};
use winit::window::Window;

#[cfg(target_os = "macos")]
use player_runtime::{PlayerVideoSurfaceKind, PlayerVideoSurfaceTarget};

#[cfg(target_os = "linux")]
use player_platform_linux::{
    open_linux_host_runtime_uri_with_options, probe_linux_host_runtime_uri_with_options,
};
#[cfg(target_os = "macos")]
use player_platform_macos::{
    open_macos_host_runtime_uri_with_options, probe_macos_host_runtime_uri_with_options,
};
#[cfg(target_os = "windows")]
use player_platform_windows::{
    open_windows_host_runtime_uri_with_options, probe_windows_host_runtime_uri_with_options,
};
#[cfg(target_os = "macos")]
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[derive(Debug, Clone)]
pub struct DesktopHostLaunchPlan {
    pub source: String,
    pub render_config: RenderSurfaceConfig,
}

const DESKTOP_REMOTE_VIDEO_PREFETCH_CAPACITY: usize = 48;
const DESKTOP_STREAMING_VIDEO_PREFETCH_CAPACITY: usize = 96;

#[derive(Debug, Clone)]
pub struct DesktopHostRuntimeProbe {
    pub adapter_id: &'static str,
    pub capabilities: PlayerRuntimeAdapterCapabilities,
    pub media_info: PlayerMediaInfo,
    pub startup: PlayerRuntimeStartup,
}

#[derive(Debug, Clone)]
pub struct DesktopHostLaunchProbe {
    pub launch_plan: DesktopHostLaunchPlan,
    pub runtime_probe: DesktopHostRuntimeProbe,
}

pub fn probe_desktop_host_launch_plan_uri(
    uri: impl Into<String>,
) -> Result<DesktopHostLaunchProbe> {
    probe_desktop_host_launch_plan_uri_with_options(uri, PlayerRuntimeOptions::default())
}

pub fn probe_desktop_host_launch_plan_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> Result<DesktopHostLaunchProbe> {
    let source = normalize_desktop_host_source_uri(uri.into())?;
    let runtime_probe = probe_desktop_host_runtime_uri_with_options(source.clone(), options)?;
    let launch_plan = DesktopHostLaunchPlan {
        source,
        render_config: render_config_from_media_info(&runtime_probe.media_info),
    };

    Ok(DesktopHostLaunchProbe {
        launch_plan,
        runtime_probe,
    })
}

pub fn probe_desktop_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> Result<DesktopHostRuntimeProbe> {
    let source = normalize_desktop_host_source_uri(uri.into())?;
    let options = desktop_runtime_options_for_source(&source, options);

    #[cfg(target_os = "macos")]
    {
        let probe = probe_macos_host_runtime_uri_with_options(source, options)?;
        return Ok(DesktopHostRuntimeProbe {
            adapter_id: probe.adapter_id,
            capabilities: probe.capabilities,
            media_info: probe.media_info,
            startup: probe.startup,
        });
    }

    #[cfg(target_os = "linux")]
    {
        let probe = probe_linux_host_runtime_uri_with_options(source, options)?;
        return Ok(DesktopHostRuntimeProbe {
            adapter_id: probe.adapter_id,
            capabilities: probe.capabilities,
            media_info: probe.media_info,
            startup: probe.startup,
        });
    }

    #[cfg(target_os = "windows")]
    {
        let probe = probe_windows_host_runtime_uri_with_options(source, options)?;
        return Ok(DesktopHostRuntimeProbe {
            adapter_id: probe.adapter_id,
            capabilities: probe.capabilities,
            media_info: probe.media_info,
            startup: probe.startup,
        });
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (source, options);
        anyhow::bail!("desktop host helper only supports macOS, Linux, and Windows targets")
    }
}

pub fn normalize_desktop_host_source_uri(source: impl AsRef<str>) -> Result<String> {
    let source = source.as_ref();
    if is_remote_or_virtual_source_uri(source) {
        return Ok(source.to_owned());
    }

    canonical_desktop_host_local_path(Path::new(source))
}

pub fn canonical_desktop_host_local_path(path: &Path) -> Result<String> {
    let canonical_path = path
        .canonicalize()
        .with_context(|| format!("failed to resolve media source at {}", path.display()))?;

    Ok(canonical_path.to_string_lossy().into_owned())
}

pub fn open_desktop_host_runtime_uri_for_winit_window(
    uri: impl Into<String>,
    window: &Window,
) -> Result<(PlayerRuntimeBootstrap, PlayerRuntimeAdapterCapabilities)> {
    open_desktop_host_runtime_uri_for_winit_window_with_options(
        uri,
        window,
        PlayerRuntimeOptions::default(),
    )
}

pub fn open_desktop_host_runtime_uri_for_winit_window_with_options(
    uri: impl Into<String>,
    window: &Window,
    options: PlayerRuntimeOptions,
) -> Result<(PlayerRuntimeBootstrap, PlayerRuntimeAdapterCapabilities)> {
    let source = normalize_desktop_host_source_uri(uri.into())?;
    let options = runtime_options_for_winit_window(
        window,
        desktop_runtime_options_for_source(&source, options),
    )?;

    #[cfg(target_os = "macos")]
    {
        let bootstrap = open_macos_host_runtime_uri_with_options(source, options)?;
        let capabilities = bootstrap.runtime.capabilities();
        return Ok((bootstrap, capabilities));
    }

    #[cfg(target_os = "linux")]
    {
        let bootstrap = open_linux_host_runtime_uri_with_options(source, options)?;
        let capabilities = bootstrap.runtime.capabilities();
        return Ok((bootstrap, capabilities));
    }

    #[cfg(target_os = "windows")]
    {
        let bootstrap = open_windows_host_runtime_uri_with_options(source, options)?;
        let capabilities = bootstrap.runtime.capabilities();
        return Ok((bootstrap, capabilities));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (source, options);
        anyhow::bail!("desktop host helper only supports macOS, Linux, and Windows targets")
    }
}

pub fn runtime_options_for_winit_window(
    window: &Window,
    options: PlayerRuntimeOptions,
) -> Result<PlayerRuntimeOptions> {
    #[cfg(target_os = "macos")]
    {
        let mut options = options;
        if options.video_surface.is_none() {
            options = options.with_video_surface(macos_video_surface_target(window)?);
        }

        return Ok(options);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(options)
    }
}

pub fn render_config_from_media_info(media_info: &PlayerMediaInfo) -> RenderSurfaceConfig {
    media_info
        .best_video
        .as_ref()
        .map(|video| RenderSurfaceConfig {
            width: video.width.max(640),
            height: video.height.max(360),
        })
        .unwrap_or_default()
}

fn is_remote_or_virtual_source_uri(source: &str) -> bool {
    let lower = source.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("file://")
        || lower.starts_with("content://")
}

fn desktop_runtime_options_for_source(
    source: &str,
    mut options: PlayerRuntimeOptions,
) -> PlayerRuntimeOptions {
    if options.video_prefetch_capacity != DEFAULT_VIDEO_PREFETCH_CAPACITY {
        return options;
    }

    let source = MediaSource::new(source.to_owned());
    options.video_prefetch_capacity = match (source.kind(), source.protocol()) {
        (MediaSourceKind::Remote, MediaSourceProtocol::Hls)
        | (MediaSourceKind::Remote, MediaSourceProtocol::Dash) => {
            DESKTOP_STREAMING_VIDEO_PREFETCH_CAPACITY
        }
        (MediaSourceKind::Remote, MediaSourceProtocol::Progressive) => {
            DESKTOP_REMOTE_VIDEO_PREFETCH_CAPACITY
        }
        _ => options.video_prefetch_capacity,
    };

    options
}

#[cfg(target_os = "macos")]
fn macos_video_surface_target(window: &Window) -> Result<PlayerVideoSurfaceTarget> {
    let handle = window
        .window_handle()
        .context("failed to resolve the macOS raw window handle")?;
    match handle.as_raw() {
        RawWindowHandle::AppKit(handle) => Ok(PlayerVideoSurfaceTarget {
            kind: PlayerVideoSurfaceKind::NsView,
            handle: handle.ns_view.as_ptr() as usize,
        }),
        raw => anyhow::bail!("expected an AppKit window handle on macOS, received {raw:?}"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        canonical_desktop_host_local_path, desktop_runtime_options_for_source,
        normalize_desktop_host_source_uri, render_config_from_media_info,
    };
    use player_render_wgpu::RenderSurfaceConfig;
    use player_runtime::{
        DEFAULT_VIDEO_PREFETCH_CAPACITY, MediaSourceKind, MediaSourceProtocol, PlayerMediaInfo,
        PlayerRuntimeOptions, PlayerVideoInfo,
    };

    const HLS_REMOTE_SOURCE: &str = "https://example.com/stream/master.m3u8";
    const DASH_REMOTE_SOURCE: &str = "https://example.com/stream/manifest.mpd";

    #[test]
    fn render_config_uses_best_video_dimensions() {
        let media_info = PlayerMediaInfo {
            source_uri: "test://video".into(),
            source_kind: MediaSourceKind::Remote,
            source_protocol: MediaSourceProtocol::Unknown,
            duration: None,
            bit_rate: None,
            audio_streams: 1,
            video_streams: 1,
            best_video: Some(PlayerVideoInfo {
                codec: "h264".into(),
                width: 3840,
                height: 2160,
                frame_rate: Some(60.0),
            }),
            best_audio: None,
        };

        assert_eq!(
            render_config_from_media_info(&media_info),
            RenderSurfaceConfig {
                width: 3840,
                height: 2160,
            }
        );
    }

    #[test]
    fn render_config_clamps_small_video_dimensions() {
        let media_info = PlayerMediaInfo {
            source_uri: "test://video".into(),
            source_kind: MediaSourceKind::Remote,
            source_protocol: MediaSourceProtocol::Unknown,
            duration: None,
            bit_rate: None,
            audio_streams: 0,
            video_streams: 1,
            best_video: Some(PlayerVideoInfo {
                codec: "h264".into(),
                width: 320,
                height: 180,
                frame_rate: Some(24.0),
            }),
            best_audio: None,
        };

        assert_eq!(
            render_config_from_media_info(&media_info),
            RenderSurfaceConfig {
                width: 640,
                height: 360,
            }
        );
    }

    #[test]
    fn render_config_defaults_without_video() {
        let media_info = PlayerMediaInfo {
            source_uri: "test://audio".into(),
            source_kind: MediaSourceKind::Remote,
            source_protocol: MediaSourceProtocol::Unknown,
            duration: None,
            bit_rate: None,
            audio_streams: 1,
            video_streams: 0,
            best_video: None,
            best_audio: None,
        };

        assert_eq!(
            render_config_from_media_info(&media_info),
            RenderSurfaceConfig::default()
        );
    }

    #[test]
    fn normalize_desktop_source_preserves_remote_url() {
        let source = normalize_desktop_host_source_uri(HLS_REMOTE_SOURCE)
            .expect("remote url should normalize");
        assert_eq!(source, HLS_REMOTE_SOURCE);
    }

    #[test]
    fn normalize_desktop_source_preserves_dash_url() {
        let source = normalize_desktop_host_source_uri(DASH_REMOTE_SOURCE)
            .expect("dash url should normalize");
        assert_eq!(source, DASH_REMOTE_SOURCE);
    }

    #[test]
    fn normalize_desktop_source_canonicalizes_local_path() {
        let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../test-video.mp4");
        let source = canonical_desktop_host_local_path(&fixture_path)
            .expect("local path should canonicalize");
        assert!(source.ends_with("test-video.mp4"));
    }

    #[test]
    fn desktop_runtime_options_expand_prefetch_for_streaming_sources() {
        let options =
            desktop_runtime_options_for_source(HLS_REMOTE_SOURCE, PlayerRuntimeOptions::default());
        assert!(options.video_prefetch_capacity > DEFAULT_VIDEO_PREFETCH_CAPACITY);
    }

    #[test]
    fn desktop_runtime_options_expand_prefetch_for_dash_sources() {
        let options =
            desktop_runtime_options_for_source(DASH_REMOTE_SOURCE, PlayerRuntimeOptions::default());
        assert!(options.video_prefetch_capacity > DEFAULT_VIDEO_PREFETCH_CAPACITY);
    }

    #[test]
    fn desktop_runtime_options_preserve_explicit_prefetch_override() {
        let options = desktop_runtime_options_for_source(
            HLS_REMOTE_SOURCE,
            PlayerRuntimeOptions {
                video_prefetch_capacity: 12,
                ..PlayerRuntimeOptions::default()
            },
        );
        assert_eq!(options.video_prefetch_capacity, 12);
    }
}
