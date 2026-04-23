use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use player_core::{MediaSource, MediaSourceKind, MediaSourceProtocol};
use player_render_wgpu::RenderSurfaceConfig;
use player_runtime::{
    DEFAULT_VIDEO_PREFETCH_CAPACITY, InMemoryPreloadBudgetProvider, PlayerBufferingPolicy,
    PlayerBufferingPreset, PlayerMediaInfo, PlayerRuntimeAdapterCapabilities,
    PlayerRuntimeBootstrap, PlayerRuntimeOptions, PlayerRuntimeResult, PlayerRuntimeStartup,
    PreloadCandidate, PreloadEvent, PreloadExecutor, PreloadPlanner, PreloadSnapshot,
    PreloadTaskId, PreloadTaskSnapshot,
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
const DESKTOP_LOCAL_VIDEO_PREFETCH_CAPACITY: usize = 48;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopPreloadCommand {
    Warmup { task: PreloadTaskSnapshot },
    Cancel { task_id: PreloadTaskId },
}

#[derive(Debug, Default, Clone)]
pub struct DesktopNoopPreloadExecutor {
    commands: Arc<Mutex<Vec<DesktopPreloadCommand>>>,
}

impl DesktopNoopPreloadExecutor {
    pub fn drain_commands(&self) -> Vec<DesktopPreloadCommand> {
        self.commands
            .lock()
            .map(|mut commands| commands.drain(..).collect())
            .unwrap_or_default()
    }
}

impl PreloadExecutor for DesktopNoopPreloadExecutor {
    fn warmup(&mut self, task: &PreloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        if let Ok(mut commands) = self.commands.lock() {
            commands.push(DesktopPreloadCommand::Warmup { task: task.clone() });
        }
        Ok(())
    }

    fn cancel(&mut self, task_id: PreloadTaskId) -> PlayerRuntimeResult<()> {
        if let Ok(mut commands) = self.commands.lock() {
            commands.push(DesktopPreloadCommand::Cancel { task_id });
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct DesktopPreloadBridgeSession {
    planner: PreloadPlanner<InMemoryPreloadBudgetProvider, DesktopNoopPreloadExecutor>,
}

impl DesktopPreloadBridgeSession {
    pub fn new(budget_provider: InMemoryPreloadBudgetProvider) -> Self {
        Self {
            planner: PreloadPlanner::new(budget_provider, DesktopNoopPreloadExecutor::default()),
        }
    }

    pub fn plan(
        &mut self,
        candidates: impl IntoIterator<Item = PreloadCandidate>,
        now: Instant,
    ) -> Vec<PreloadTaskId> {
        self.planner.plan(candidates, now)
    }

    pub fn complete(
        &mut self,
        task_id: PreloadTaskId,
    ) -> PlayerRuntimeResult<Option<PreloadTaskSnapshot>> {
        self.planner.complete(task_id)
    }

    pub fn fail(
        &mut self,
        task_id: PreloadTaskId,
        error: player_runtime::PlayerRuntimeError,
    ) -> PlayerRuntimeResult<Option<PreloadTaskSnapshot>> {
        self.planner.fail(task_id, error)
    }

    pub fn cancel(
        &mut self,
        task_id: PreloadTaskId,
    ) -> PlayerRuntimeResult<Option<PreloadTaskSnapshot>> {
        self.planner.cancel(task_id)
    }

    pub fn drain_commands(&self) -> Vec<DesktopPreloadCommand> {
        self.planner.executor().drain_commands()
    }

    pub fn snapshot(&self) -> PreloadSnapshot {
        self.planner.snapshot()
    }

    pub fn drain_events(&mut self) -> Vec<PreloadEvent> {
        self.planner.drain_events()
    }
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
    let source = MediaSource::new(source.to_owned());
    if options.video_prefetch_capacity == DEFAULT_VIDEO_PREFETCH_CAPACITY {
        options.video_prefetch_capacity = match (source.kind(), source.protocol()) {
            (MediaSourceKind::Local, _) => DESKTOP_LOCAL_VIDEO_PREFETCH_CAPACITY,
            (MediaSourceKind::Remote, MediaSourceProtocol::Hls)
            | (MediaSourceKind::Remote, MediaSourceProtocol::Dash) => {
                DESKTOP_STREAMING_VIDEO_PREFETCH_CAPACITY
            }
            (MediaSourceKind::Remote, MediaSourceProtocol::Progressive) => {
                DESKTOP_REMOTE_VIDEO_PREFETCH_CAPACITY
            }
            _ => options.video_prefetch_capacity,
        };
    }

    if options.buffering_policy.preset == PlayerBufferingPreset::Default
        && options.buffering_policy.min_buffer.is_none()
        && options.buffering_policy.max_buffer.is_none()
        && options.buffering_policy.buffer_for_playback.is_none()
        && options.buffering_policy.buffer_for_rebuffer.is_none()
        && source.kind() == MediaSourceKind::Local
    {
        options.buffering_policy = PlayerBufferingPolicy::balanced();
    }

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
        DesktopNoopPreloadExecutor, DesktopPreloadBridgeSession, DesktopPreloadCommand,
        canonical_desktop_host_local_path, desktop_runtime_options_for_source,
        normalize_desktop_host_source_uri, render_config_from_media_info,
    };
    use player_core::MediaSource;
    use player_render_wgpu::RenderSurfaceConfig;
    use player_runtime::{
        DEFAULT_VIDEO_PREFETCH_CAPACITY, InMemoryPreloadBudgetProvider, MediaSourceKind,
        MediaSourceProtocol, MediaTrackCatalog, MediaTrackSelectionSnapshot, PlayerMediaInfo,
        PlayerRuntimeError, PlayerRuntimeErrorCode, PlayerRuntimeOptions, PlayerVideoInfo,
        PreloadBudget, PreloadBudgetScope, PreloadCandidate, PreloadCandidateKind, PreloadConfig,
        PreloadEvent, PreloadExecutor, PreloadPriority, PreloadSelectionHint, PreloadTaskStatus,
    };
    use std::time::{Duration, Instant};

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
            track_catalog: MediaTrackCatalog::default(),
            track_selection: MediaTrackSelectionSnapshot::default(),
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
            track_catalog: MediaTrackCatalog::default(),
            track_selection: MediaTrackSelectionSnapshot::default(),
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
            track_catalog: MediaTrackCatalog::default(),
            track_selection: MediaTrackSelectionSnapshot::default(),
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

    #[test]
    fn desktop_noop_preload_executor_records_warmup_and_cancel_commands() {
        let mut executor = DesktopNoopPreloadExecutor::default();
        let task = sample_preload_candidate("https://example.com/current.m3u8");
        let mut session =
            DesktopPreloadBridgeSession::new(InMemoryPreloadBudgetProvider::new(test_budget(1)));
        let task_id = session
            .plan([task], Instant::now())
            .into_iter()
            .next()
            .expect("desktop preload task should be planned");

        let commands = session.drain_commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0],
            DesktopPreloadCommand::Warmup { task } if task.task_id == task_id
        ));

        let _ = executor.cancel(task_id);
        assert_eq!(
            executor.drain_commands(),
            vec![DesktopPreloadCommand::Cancel { task_id }]
        );
    }

    #[test]
    fn desktop_preload_bridge_releases_budget_after_completion() {
        let mut session =
            DesktopPreloadBridgeSession::new(InMemoryPreloadBudgetProvider::new(test_budget(1)));
        let now = Instant::now();

        let first_task_id = session
            .plan(
                [sample_preload_candidate("https://example.com/current.m3u8")],
                now,
            )
            .into_iter()
            .next()
            .expect("first desktop preload task should be planned");
        let _ = session.drain_commands();

        assert!(
            session
                .plan(
                    [sample_preload_candidate(
                        "https://example.com/neighbor.m3u8"
                    )],
                    now
                )
                .is_empty()
        );

        let completed = session
            .complete(first_task_id)
            .expect("desktop complete should succeed")
            .expect("desktop task should exist");
        assert_eq!(completed.status, PreloadTaskStatus::Completed);

        let follow_up = session.plan(
            [sample_preload_candidate(
                "https://example.com/neighbor.m3u8",
            )],
            now,
        );
        assert_eq!(follow_up.len(), 1);
    }

    #[test]
    fn desktop_preload_bridge_records_failure_event() {
        let mut session =
            DesktopPreloadBridgeSession::new(InMemoryPreloadBudgetProvider::new(test_budget(1)));
        let task_id = session
            .plan(
                [sample_preload_candidate("https://example.com/current.m3u8")],
                Instant::now(),
            )
            .into_iter()
            .next()
            .expect("desktop preload task should be planned");

        let failed = session
            .fail(
                task_id,
                PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "desktop preload noop executor failed",
                ),
            )
            .expect("desktop fail should succeed")
            .expect("desktop task should exist");
        assert_eq!(failed.status, PreloadTaskStatus::Failed);

        let events = session.drain_events();
        assert!(
            events.iter().any(
                |event| matches!(event, PreloadEvent::Failed(task) if task.task_id == task_id)
            )
        );
    }

    fn test_budget(max_concurrent_tasks: u32) -> PreloadBudget {
        PreloadBudget {
            max_concurrent_tasks,
            max_memory_bytes: 64,
            max_disk_bytes: 64,
            warmup_window: Duration::from_secs(30),
        }
    }

    fn sample_preload_candidate(uri: &str) -> PreloadCandidate {
        PreloadCandidate {
            source: MediaSource::new(uri),
            scope: PreloadBudgetScope::App,
            kind: PreloadCandidateKind::Current,
            selection_hint: PreloadSelectionHint::CurrentItem,
            config: PreloadConfig {
                priority: PreloadPriority::Critical,
                ttl: None,
                expected_memory_bytes: 1,
                expected_disk_bytes: 1,
                warmup_window: None,
            },
        }
    }
}
