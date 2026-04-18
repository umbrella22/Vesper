use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
mod desktop_download;
mod desktop_file_dialog;
mod desktop_overlay_ui;
mod desktop_presenter;
mod desktop_symbols;
mod desktop_ui;
mod host_ui;
#[cfg(target_os = "macos")]
mod macos_host_overlay;
use desktop_download::{
    DesktopDownloadController, PendingDownloadTask, PreparedDownloadTask,
    download_primary_action_label, download_progress_summary, download_status_label,
    draft_download_label, make_asset_id, normalized_progress_ratio, prepare_download_task,
};
use desktop_file_dialog::pick_local_media_file;
use desktop_overlay_ui::playback_stage_rect;
use desktop_presenter::DesktopUiPresenter;
use desktop_ui::{
    CONTROL_RATES, ControlAction, DesktopDownloadTaskViewData, DesktopOverlayViewModel,
    DesktopPendingDownloadTaskViewData, DesktopPlaylistItemViewData, DesktopSidebarTab,
    SeekPreview, playback_state_label,
};
use player_core::MediaSource;
#[cfg(not(target_os = "macos"))]
use player_host_desktop::open_desktop_host_runtime_uri_for_winit_window;
use player_host_desktop::{
    DesktopHostLaunchPlan as RuntimeLaunchPlan, canonical_desktop_host_local_path,
    normalize_desktop_host_source_uri, probe_desktop_host_launch_plan_uri,
};
#[cfg(target_os = "macos")]
use player_platform_macos::macos_runtime_adapter_factory;
use player_render_wgpu::{
    DisplayRect, RenderMode, RenderSurfaceConfig, RgbaVideoFrame, VideoFrameTexture, VideoRenderer,
    Yuv420pVideoFrame, default_window_attributes, preferred_backends,
};
use player_runtime::{
    DecodedAudioSummary, DecodedVideoFrame, MediaTrackCatalog, MediaTrackSelectionSnapshot,
    PlaybackProgress, PlayerMediaInfo, PlayerResilienceMetrics, PlayerRuntime,
    PlayerRuntimeBootstrap, PlayerRuntimeCommand, PlayerRuntimeEvent, PlayerRuntimeOptions,
    PlayerSnapshot, PlayerTimelineKind, PlayerTimelineSnapshot, PlayerVideoDecodeInfo,
    PlayerVideoDecodeMode, PresentationState, VideoPixelFormat,
};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

const SEEK_STEP: Duration = Duration::from_secs(5);
const NATIVE_SURFACE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const CONTROL_HIDE_DELAY: Duration = Duration::from_secs(2);
const CONTROL_FADE_DURATION: Duration = Duration::from_millis(220);
const CONTROL_FADE_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const HLS_DEMO_CLI_FLAG: &str = "--hls-demo";
const DASH_DEMO_CLI_FLAG: &str = "--dash-demo";
const DESKTOP_HLS_DEMO_URL: &str = "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8";
const DESKTOP_DASH_DEMO_URL: &str = "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd";

#[derive(Debug, Clone)]
struct DesktopPlaylistEntry {
    source_uri: String,
    label: String,
}

#[derive(Debug)]
enum PlannerEvent {
    Prepared {
        asset_id: String,
        prepared: PreparedDownloadTask,
    },
    Failed {
        asset_id: String,
        error: String,
    },
}

#[derive(Debug)]
enum LaunchEvent {
    Prepared {
        request_id: u64,
        source: String,
        label: String,
        launch_plan: RuntimeLaunchPlan,
    },
    Failed {
        request_id: u64,
        label: String,
        error: String,
    },
}

#[derive(Debug)]
enum FileDialogEvent {
    Selected(PathBuf),
    Cancelled,
    Failed(String),
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("info"))
        .with_target(false)
        .compact()
        .init();

    let source = resolve_media_source_uri()?;

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = DesktopPlayerApp::new(source);
    match event_loop.run_app(&mut app) {
        Ok(()) => Ok(()),
        Err(run_error) => {
            error!(?run_error, display = %run_error, "desktop event loop exited with error");
            Err(run_error.into())
        }
    }
}

struct DesktopPlayerApp {
    source: String,
    runtime: Option<PlayerRuntime>,
    last_frame: Option<DecodedVideoFrame>,
    render_config: RenderSurfaceConfig,
    uses_external_video_surface: bool,
    window: Option<Arc<Window>>,
    renderer: Option<VideoRenderer>,
    title_cache: Option<String>,
    cursor_position: Option<(f64, f64)>,
    pointer_inside_window: bool,
    controls_visible: bool,
    controls_hide_deadline: Option<Instant>,
    controls_opacity: f32,
    controls_animation_tick: Instant,
    seek_preview: Option<SeekPreview>,
    ui_presenter: Option<DesktopUiPresenter>,
    playlist_entries: Vec<DesktopPlaylistEntry>,
    active_playlist_index: usize,
    sidebar_tab: DesktopSidebarTab,
    download_controller: DesktopDownloadController,
    pending_downloads: Vec<PendingDownloadTask>,
    planner_tx: Sender<PlannerEvent>,
    planner_rx: Receiver<PlannerEvent>,
    launch_tx: Sender<LaunchEvent>,
    launch_rx: Receiver<LaunchEvent>,
    file_dialog_tx: Sender<FileDialogEvent>,
    file_dialog_rx: Receiver<FileDialogEvent>,
    next_launch_request_id: u64,
    active_launch_request_id: Option<u64>,
    open_file_dialog_pending: bool,
    host_message: Option<String>,
    download_message: Option<String>,
}

impl DesktopPlayerApp {
    fn new(source: String) -> Self {
        let initial_source = source;
        let initial_label = source_display_label(&initial_source);
        let (planner_tx, planner_rx) = mpsc::channel();
        let (launch_tx, launch_rx) = mpsc::channel();
        let (file_dialog_tx, file_dialog_rx) = mpsc::channel();
        Self {
            source: initial_source.clone(),
            runtime: None,
            last_frame: None,
            render_config: RenderSurfaceConfig::default(),
            uses_external_video_surface: false,
            window: None,
            renderer: None,
            title_cache: None,
            cursor_position: None,
            pointer_inside_window: true,
            controls_visible: true,
            controls_hide_deadline: None,
            controls_opacity: 1.0,
            controls_animation_tick: Instant::now(),
            seek_preview: None,
            ui_presenter: None,
            playlist_entries: vec![DesktopPlaylistEntry {
                source_uri: initial_source,
                label: initial_label,
            }],
            active_playlist_index: 0,
            sidebar_tab: DesktopSidebarTab::Playlist,
            download_controller: DesktopDownloadController::new(),
            pending_downloads: Vec::new(),
            planner_tx,
            planner_rx,
            launch_tx,
            launch_rx,
            file_dialog_tx,
            file_dialog_rx,
            next_launch_request_id: 1,
            active_launch_request_id: None,
            open_file_dialog_pending: false,
            host_message: None,
            download_message: None,
        }
    }

    fn overlay_view_model(
        &self,
        snapshot: &player_runtime::PlayerSnapshot,
    ) -> DesktopOverlayViewModel {
        let playlist_items = self
            .playlist_entries
            .iter()
            .enumerate()
            .map(|(index, entry)| DesktopPlaylistItemViewData {
                label: entry.label.clone(),
                status: if index == self.active_playlist_index {
                    "CURRENT".to_owned()
                } else {
                    "READY".to_owned()
                },
                is_active: index == self.active_playlist_index,
            })
            .collect::<Vec<_>>();
        let pending_downloads = self
            .pending_downloads
            .iter()
            .map(|task| DesktopPendingDownloadTaskViewData {
                asset_id: task.asset_id.clone(),
                label: task.label.clone(),
                source_uri: task.source_uri.clone(),
            })
            .collect::<Vec<_>>();
        let download_tasks = self
            .download_controller
            .tasks()
            .into_iter()
            .filter(|task| task.status != player_runtime::DownloadTaskStatus::Removed)
            .map(|task| {
                let export_state = self.download_controller.export_state(task.task_id);
                let completed_path = self
                    .download_controller
                    .exported_path(task.task_id)
                    .map(|path| path.display().to_string())
                    .or_else(|| {
                        task.asset_index
                            .completed_path
                            .as_ref()
                            .map(|path| path.display().to_string())
                    });
                DesktopDownloadTaskViewData {
                    task_id: task.task_id.get(),
                    label: self
                        .download_controller
                        .label_for_asset(task.asset_id.as_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| source_display_label(task.source.source.uri())),
                    status: download_status_label(task.status).to_owned(),
                    progress_summary: download_progress_summary(&task),
                    progress_ratio: normalized_progress_ratio(&task.progress),
                    completed_path,
                    error_message: task
                        .error_summary
                        .as_ref()
                        .map(|error| error.message.clone()),
                    primary_action_label: download_primary_action_label(task.status)
                        .map(str::to_owned),
                    export_action_label: (task.status
                        == player_runtime::DownloadTaskStatus::Completed
                        && task.source.content_format
                            != player_runtime::DownloadContentFormat::SingleFile)
                        .then_some("EXPORT MP4".to_owned()),
                    is_export_enabled: task.status == player_runtime::DownloadTaskStatus::Completed
                        && !export_state.in_progress
                        && task.source.content_format
                            != player_runtime::DownloadContentFormat::SingleFile,
                    is_remove_enabled: task.status != player_runtime::DownloadTaskStatus::Removed,
                    is_exporting: export_state.in_progress,
                    export_progress: export_state
                        .ratio
                        .or_else(|| normalized_progress_ratio(&task.progress)),
                }
            })
            .collect::<Vec<_>>();
        let source_label = self
            .playlist_entries
            .get(self.active_playlist_index)
            .map(|entry| entry.label.clone())
            .unwrap_or_else(|| source_display_label(&self.source));
        let subtitle = active_source_subtitle(snapshot);

        DesktopOverlayViewModel {
            source_label,
            playback_state_label: playback_state_label(snapshot.state).to_owned(),
            subtitle,
            controls_opacity: self.controls_opacity,
            cursor_position: if self.pointer_inside_window {
                self.cursor_position
                    .map(|(x, y)| (x.max(0.0).round() as u32, y.max(0.0).round() as u32))
            } else {
                None
            },
            sidebar_tab: self.sidebar_tab,
            playlist_items,
            pending_downloads,
            download_tasks,
            host_message: self.host_message.clone(),
            download_message: self.download_message.clone(),
            export_plugin_installed: self.download_controller.export_plugin_installed(),
        }
    }

    fn host_snapshot(&self) -> PlayerSnapshot {
        let source = MediaSource::new(self.source.clone());
        PlayerSnapshot {
            source_uri: self.source.clone(),
            state: PresentationState::Ready,
            has_video_surface: true,
            is_interrupted: false,
            is_buffering: self.active_launch_request_id.is_some(),
            playback_rate: 1.0,
            progress: PlaybackProgress::new(Duration::ZERO, None),
            timeline: PlayerTimelineSnapshot {
                kind: PlayerTimelineKind::Vod,
                is_seekable: false,
                seekable_range: None,
                live_edge: None,
                position: Duration::ZERO,
                duration: None,
            },
            media_info: PlayerMediaInfo {
                source_uri: self.source.clone(),
                source_kind: source.kind(),
                source_protocol: source.protocol(),
                duration: None,
                bit_rate: None,
                audio_streams: 0,
                video_streams: 0,
                best_video: None,
                best_audio: None,
                track_catalog: MediaTrackCatalog::default(),
                track_selection: MediaTrackSelectionSnapshot::default(),
            },
            resilience_metrics: PlayerResilienceMetrics::default(),
        }
    }

    fn placeholder_frame_texture(&self) -> VideoFrameTexture {
        let width = self.render_config.width.max(1);
        let height = self.render_config.height.max(1);
        let mut bytes = vec![0; width as usize * height as usize * 4];
        for chunk in bytes.chunks_exact_mut(4) {
            chunk.copy_from_slice(&[8, 12, 18, 255]);
        }
        VideoFrameTexture::Rgba(RgbaVideoFrame {
            width,
            height,
            bytes,
        })
    }

    fn current_source_label(&self) -> String {
        self.playlist_entries
            .get(self.active_playlist_index)
            .map(|entry| entry.label.clone())
            .unwrap_or_else(|| source_display_label(&self.source))
    }

    fn register_playlist_source(&mut self, source_uri: &str, label: Option<String>) {
        if let Some(index) = self
            .playlist_entries
            .iter()
            .position(|entry| entry.source_uri == source_uri)
        {
            if let Some(label) = label {
                self.playlist_entries[index].label = label;
            }
            self.active_playlist_index = index;
            return;
        }

        self.playlist_entries.push(DesktopPlaylistEntry {
            source_uri: source_uri.to_owned(),
            label: label.unwrap_or_else(|| source_display_label(source_uri)),
        });
        self.active_playlist_index = self.playlist_entries.len().saturating_sub(1);
    }

    fn stage_display_rect_for_size(&self, size: PhysicalSize<u32>) -> DisplayRect {
        let stage_rect = playback_stage_rect(size.width, size.height);
        DisplayRect {
            x: stage_rect.x,
            y: stage_rect.y,
            width: stage_rect.width.max(1),
            height: stage_rect.height.max(1),
        }
    }

    fn sync_renderer_stage_viewport(&mut self) {
        let size = self
            .window
            .as_ref()
            .map(|window| window.inner_size())
            .unwrap_or(PhysicalSize::new(
                self.render_config.width.max(1),
                self.render_config.height.max(1),
            ));
        let stage_rect = self.stage_display_rect_for_size(size);
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_video_viewport(Some(stage_rect));
        }
    }

    fn request_source_launch(&mut self, source: String, label: Option<String>) -> Result<()> {
        let label = label.unwrap_or_else(|| source_display_label(&source));
        let request_id = self.next_launch_request_id;
        self.next_launch_request_id = self.next_launch_request_id.saturating_add(1);
        self.active_launch_request_id = Some(request_id);
        self.host_message = Some(format!("LOADING {label}"));
        self.seek_preview = None;
        self.show_controls();
        self.title_cache = None;
        self.update_window_title();
        self.refresh_overlay()?;

        let launch_tx = self.launch_tx.clone();
        thread::spawn(move || {
            let event = match build_launch_plan(source.clone()) {
                Ok(launch_plan) => LaunchEvent::Prepared {
                    request_id,
                    source,
                    label,
                    launch_plan,
                },
                Err(error) => LaunchEvent::Failed {
                    request_id,
                    label,
                    error: error.to_string(),
                },
            };
            let _ = launch_tx.send(event);
        });

        Ok(())
    }

    fn queue_download_planner(&mut self, source_uri: String, label: String) {
        let asset_prefix = match MediaSource::new(source_uri.clone()).protocol() {
            player_core::MediaSourceProtocol::Hls => "hls",
            player_core::MediaSourceProtocol::Dash => "dash",
            _ => "file",
        };
        let asset_id = make_asset_id(asset_prefix);
        let draft_label = draft_download_label(&label, &source_uri);
        self.pending_downloads.push(PendingDownloadTask {
            asset_id: asset_id.clone(),
            label: draft_label,
            source_uri: source_uri.clone(),
        });
        self.sidebar_tab = DesktopSidebarTab::Downloads;
        self.download_message = Some(format!("Preparing {source_uri}"));

        let planner_tx = self.planner_tx.clone();
        thread::spawn(move || {
            let source = MediaSource::new(source_uri.clone());
            let event = match prepare_download_task(&asset_id, &source, &label) {
                Ok(prepared) => PlannerEvent::Prepared { asset_id, prepared },
                Err(error) => PlannerEvent::Failed {
                    asset_id,
                    error: error.to_string(),
                },
            };
            let _ = planner_tx.send(event);
        });
    }

    fn drain_planner_events(&mut self) -> Result<bool> {
        let mut changed = false;
        loop {
            match self.planner_rx.try_recv() {
                Ok(PlannerEvent::Prepared { asset_id, prepared }) => {
                    self.pending_downloads
                        .retain(|task| task.asset_id != asset_id);
                    let resolved_label = prepared.resolved_label.clone();
                    self.download_controller.create_prepared_task(
                        asset_id,
                        resolved_label.clone(),
                        prepared,
                    )?;
                    self.download_message = Some(format!("Queued {resolved_label}"));
                    changed = true;
                }
                Ok(PlannerEvent::Failed { asset_id, error }) => {
                    self.pending_downloads
                        .retain(|task| task.asset_id != asset_id);
                    self.download_message = Some(error);
                    changed = true;
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        Ok(changed)
    }

    fn drain_launch_events(&mut self) -> Result<bool> {
        let mut changed = false;
        loop {
            match self.launch_rx.try_recv() {
                Ok(LaunchEvent::Prepared {
                    request_id,
                    source,
                    label,
                    launch_plan,
                }) => {
                    if self.active_launch_request_id != Some(request_id) {
                        continue;
                    }
                    self.active_launch_request_id = None;
                    self.host_message = None;
                    let window = self
                        .window
                        .as_ref()
                        .cloned()
                        .context("window missing while activating launch plan")?;
                    match self.activate_launch_plan(launch_plan, window.clone()) {
                        Ok(()) => {
                            self.register_playlist_source(&source, Some(label));
                            self.sync_ui_presenter();
                            self.refresh_overlay()?;
                            window.request_redraw();
                        }
                        Err(error) => {
                            warn!(
                                ?error,
                                source = source.as_str(),
                                "failed to activate media source"
                            );
                            self.host_message = Some("FAILED TO OPEN SOURCE".to_owned());
                            self.refresh_overlay()?;
                        }
                    }
                    changed = true;
                }
                Ok(LaunchEvent::Failed {
                    request_id,
                    label,
                    error,
                }) => {
                    if self.active_launch_request_id != Some(request_id) {
                        continue;
                    }
                    self.active_launch_request_id = None;
                    self.host_message = Some(format!("FAILED TO LOAD {label}"));
                    warn!(
                        label = label.as_str(),
                        error = error.as_str(),
                        "failed to prepare media launch plan"
                    );
                    self.refresh_overlay()?;
                    changed = true;
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        Ok(changed)
    }

    fn drain_download_updates(&mut self) -> Result<bool> {
        let updates = self.download_controller.poll();
        if let Some(message) = updates.messages.last() {
            self.download_message = Some(message.clone());
        }
        Ok(updates.changed)
    }

    fn drain_file_dialog_events(&mut self) -> Result<bool> {
        let mut changed = false;
        loop {
            match self.file_dialog_rx.try_recv() {
                Ok(FileDialogEvent::Selected(path)) => {
                    self.open_file_dialog_pending = false;
                    info!(path = %path.display(), "opening selected local media file");
                    self.open_dropped_file(path)?;
                    changed = true;
                }
                Ok(FileDialogEvent::Cancelled) => {
                    self.open_file_dialog_pending = false;
                    info!("local media file selection cancelled");
                }
                Ok(FileDialogEvent::Failed(error)) => {
                    self.open_file_dialog_pending = false;
                    warn!(
                        error = error.as_str(),
                        "failed to open local media file picker"
                    );
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        Ok(changed)
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.window.is_some() {
            return Ok(());
        }

        let window = Arc::new(event_loop.create_window(
            default_window_attributes(self.render_config).with_title(self.window_title()),
        )?);
        match DesktopUiPresenter::attach(window.as_ref()) {
            Ok(presenter) => {
                self.ui_presenter = Some(presenter);
            }
            Err(error) => {
                warn!(
                    ?error,
                    "failed to initialize desktop UI presenter; keyboard controls remain available"
                );
            }
        }
        self.window = Some(window.clone());
        self.request_source_launch(self.source.clone(), Some(self.current_source_label()))?;

        Ok(())
    }

    fn activate_launch_plan(
        &mut self,
        launch_plan: RuntimeLaunchPlan,
        window: Arc<Window>,
    ) -> Result<()> {
        let (
            PlayerRuntimeBootstrap {
                runtime,
                initial_frame,
                startup,
            },
            capabilities,
        ) = open_basic_player_runtime_for_window(&launch_plan.source, window.as_ref())?;

        info!(
            adapter_id = runtime.adapter_id(),
            source = launch_plan.source.as_str(),
            decoded_audio = startup.decoded_audio.as_ref().map(audio_summary),
            video_decode = startup.video_decode.as_ref().map(video_decode_summary),
            initial_pixel_format = initial_frame.as_ref().map(video_pixel_format_label),
            supports_frame_output = capabilities.supports_frame_output,
            supports_external_video_surface = capabilities.supports_external_video_surface,
            "initialized desktop player"
        );

        self.source = launch_plan.source;
        self.render_config = launch_plan.render_config;
        self.uses_external_video_surface = capabilities.supports_external_video_surface;
        self.runtime = Some(runtime);
        self.seek_preview = None;
        self.host_message = None;

        if capabilities.supports_frame_output {
            let initial_frame =
                initial_frame.context("desktop runtime did not provide an initial frame")?;
            if self.renderer.is_none() {
                let mut renderer = pollster::block_on(VideoRenderer::new(
                    window.clone(),
                    (initial_frame.width, initial_frame.height),
                ))?;
                renderer.set_render_mode(RenderMode::Fit);
                self.renderer = Some(renderer);
            } else if let Some(renderer) = self.renderer.as_mut() {
                renderer.set_render_mode(RenderMode::Fit);
            }
            self.sync_renderer_stage_viewport();
            self.last_frame = None;
            self.apply_frame(initial_frame)?;
        } else {
            self.renderer = None;
            self.last_frame = None;
        }

        self.title_cache = None;
        self.show_controls();
        self.update_window_title();
        self.sync_ui_presenter();
        self.dispatch_command(PlayerRuntimeCommand::Play)?;

        Ok(())
    }

    fn runtime(&self) -> Result<&PlayerRuntime> {
        self.runtime
            .as_ref()
            .context("player runtime is not initialized")
    }

    fn runtime_mut(&mut self) -> Result<&mut PlayerRuntime> {
        self.runtime
            .as_mut()
            .context("player runtime is not initialized")
    }

    fn handle_redraw(&mut self) -> Result<()> {
        let Some(window) = self.window.as_ref() else {
            return Ok(());
        };
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(());
        };

        window.pre_present_notify();
        renderer.render()
    }

    fn advance_playback(&mut self) -> Result<bool> {
        let Some(runtime) = self.runtime.as_mut() else {
            return Ok(false);
        };
        let Some(frame) = runtime.advance()? else {
            return Ok(false);
        };

        self.apply_frame(frame)?;
        Ok(true)
    }

    fn apply_frame(&mut self, frame: DecodedVideoFrame) -> Result<()> {
        self.last_frame = Some(frame);
        self.refresh_overlay()
    }

    fn refresh_overlay(&mut self) -> Result<()> {
        if self.runtime.is_none() {
            return self.refresh_host_overlay();
        }
        let window = self
            .window
            .as_ref()
            .cloned()
            .context("window missing while playback is active")?;
        let Some(frame) = self.last_frame.as_ref() else {
            if self.host_message.is_some() {
                return self.refresh_host_overlay();
            }
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.clear_overlay();
            }
            return Ok(());
        };
        let frame_texture = video_frame_texture(frame);
        let window_size = window.inner_size();
        let snapshot = self.runtime()?.snapshot();
        let overlay_view_model = self.overlay_view_model(&snapshot);
        let overlay = self.ui_presenter.as_ref().and_then(|presenter| {
            presenter.overlay_frame(
                window_size,
                &snapshot,
                self.seek_preview,
                &overlay_view_model,
            )
        });
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(());
        };
        if window_size.width == 0 || window_size.height == 0 {
            renderer.clear_overlay();
            return Ok(());
        }

        renderer.upload_frame(&frame_texture);
        if let Some(overlay) = overlay {
            renderer.upload_overlay(&overlay);
        } else {
            renderer.clear_overlay();
        }

        window.request_redraw();

        Ok(())
    }

    fn refresh_host_overlay(&mut self) -> Result<()> {
        let window = self
            .window
            .as_ref()
            .cloned()
            .context("window missing while host overlay is active")?;
        let window_size = window.inner_size();
        if window_size.width == 0 || window_size.height == 0 {
            return Ok(());
        }
        if self.host_message.is_none() {
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.clear_overlay();
                window.request_redraw();
            }
            return Ok(());
        }
        if self.renderer.is_none() {
            let mut renderer = pollster::block_on(VideoRenderer::new(
                window.clone(),
                (
                    self.render_config.width.max(1),
                    self.render_config.height.max(1),
                ),
            ))?;
            renderer.set_render_mode(RenderMode::Fit);
            self.renderer = Some(renderer);
        }
        self.sync_renderer_stage_viewport();

        let snapshot = self.host_snapshot();
        let overlay_view_model = self.overlay_view_model(&snapshot);
        let overlay = self.ui_presenter.as_ref().and_then(|presenter| {
            presenter.overlay_frame(window_size, &snapshot, None, &overlay_view_model)
        });
        let frame_texture = self.placeholder_frame_texture();
        let renderer = self
            .renderer
            .as_mut()
            .context("renderer missing while host overlay is active")?;
        renderer.upload_frame(&frame_texture);
        if let Some(overlay) = overlay {
            renderer.upload_overlay(&overlay);
        } else {
            renderer.clear_overlay();
        }
        window.request_redraw();
        Ok(())
    }

    fn update_window_title(&mut self) {
        if let Some(window) = self.window.as_ref() {
            let title = self.window_title();
            if self.title_cache.as_deref() != Some(title.as_str()) {
                window.set_title(&title);
                self.title_cache = Some(title);
            }
        }
    }

    fn window_title(&self) -> String {
        let source_name = Path::new(&self.source)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("media");
        let Some(runtime) = self.runtime.as_ref() else {
            return format!("Vesper basic player - Opening - {source_name}");
        };
        let snapshot = runtime.snapshot();
        let state = match snapshot.state {
            PresentationState::Ready => "Ready",
            PresentationState::Playing => "Playing",
            PresentationState::Paused => "Paused",
            PresentationState::Finished => "Finished",
        };
        let video_label = snapshot
            .media_info
            .best_video
            .as_ref()
            .map(|video| format!("{}x{}", video.width, video.height))
            .unwrap_or_else(|| "unknown".to_owned());
        let rate = snapshot.playback_rate;
        let progress = format_playback_progress(snapshot.progress);

        format!(
            "Vesper basic player - {state} - {source_name} - {video_label} - {progress} - {rate:.1}x"
        )
    }

    fn dispatch_command(&mut self, command: PlayerRuntimeCommand) -> Result<()> {
        let result = {
            let runtime = self.runtime_mut()?;
            runtime.dispatch(command)?
        };
        if let Some(frame) = result.frame {
            self.apply_frame(frame)?;
        }
        self.log_runtime_events();
        self.update_window_title();
        self.sync_ui_presenter();
        self.refresh_overlay()?;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }

        let _ = result.snapshot;
        Ok(())
    }

    fn seek_by(&mut self, delta: Duration, forward: bool) -> Result<()> {
        let current_position = self.runtime()?.snapshot().progress.position();
        let position = if forward {
            current_position.saturating_add(delta)
        } else {
            current_position.saturating_sub(delta)
        };

        self.dispatch_command(PlayerRuntimeCommand::SeekTo { position })
    }

    fn seek_to(&mut self, position: Duration) -> Result<()> {
        self.dispatch_command(PlayerRuntimeCommand::SeekTo { position })
    }

    fn begin_seek_drag(&mut self) -> Result<bool> {
        if self.runtime.is_none() {
            return Ok(false);
        }
        let Some((cursor_x, cursor_y)) = self.cursor_position else {
            return Ok(false);
        };
        let Some(window) = self.window.as_ref() else {
            return Ok(false);
        };
        let Some(presenter) = self.ui_presenter.as_ref() else {
            return Ok(false);
        };
        let snapshot = self.runtime()?.snapshot();
        let overlay_view_model = self.overlay_view_model(&snapshot);
        let window_size = window.inner_size();
        let Some(preview) = presenter.seek_preview_at(
            window_size,
            cursor_x,
            cursor_y,
            &snapshot,
            &overlay_view_model,
        ) else {
            return Ok(false);
        };

        self.seek_preview = Some(preview);
        self.show_controls();
        self.refresh_overlay()?;
        Ok(true)
    }

    fn update_seek_drag(&mut self) -> Result<()> {
        if self.runtime.is_none() {
            return Ok(());
        }
        if self.seek_preview.is_none() {
            return Ok(());
        }

        let Some((cursor_x, _)) = self.cursor_position else {
            return Ok(());
        };
        let Some(window) = self.window.as_ref() else {
            return Ok(());
        };
        let Some(presenter) = self.ui_presenter.as_ref() else {
            return Ok(());
        };
        let snapshot = self.runtime()?.snapshot();
        let overlay_view_model = self.overlay_view_model(&snapshot);
        let window_size = window.inner_size();
        if let Some(preview) =
            presenter.seek_preview_for_drag(window_size, cursor_x, &snapshot, &overlay_view_model)
        {
            self.seek_preview = Some(preview);
            self.refresh_overlay()?;
        }

        Ok(())
    }

    fn commit_seek_drag(&mut self) -> Result<bool> {
        let Some(preview) = self.seek_preview.take() else {
            return Ok(false);
        };
        info!(
            origin = "seek_drag",
            position_secs = preview.position.as_secs_f64(),
            ratio = preview.ratio,
            "desktop UI seek committed"
        );
        self.seek_to(preview.position)?;
        Ok(true)
    }

    fn toggle_pause(&mut self) {
        if let Err(error) = self.dispatch_command(PlayerRuntimeCommand::TogglePause) {
            error!(?error, "failed to toggle pause state");
        }
    }

    fn open_media_source(&mut self, source: String) -> Result<()> {
        let label = source_display_label(&source);
        self.open_media_source_with_label(source, Some(label))
    }

    fn open_media_source_with_label(
        &mut self,
        source: String,
        label: Option<String>,
    ) -> Result<()> {
        self.request_source_launch(source, label)
    }

    fn open_dropped_file(&mut self, path: PathBuf) -> Result<()> {
        let source = canonical_desktop_host_local_path(&path)?;
        info!(source = source.as_str(), "opening dropped media source");
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| source_display_label(&source));
        self.open_media_source_with_label(source, Some(label))
    }

    fn request_open_file_dialog(&mut self) -> Result<()> {
        if self.open_file_dialog_pending {
            return Ok(());
        }

        self.open_file_dialog_pending = true;
        self.show_controls();
        self.refresh_overlay()?;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }

        let file_dialog_tx = self.file_dialog_tx.clone();
        thread::spawn(move || {
            let event = match pick_local_media_file() {
                Ok(Some(path)) => FileDialogEvent::Selected(path),
                Ok(None) => FileDialogEvent::Cancelled,
                Err(error) => FileDialogEvent::Failed(error.to_string()),
            };
            let _ = file_dialog_tx.send(event);
        });

        Ok(())
    }

    fn set_playback_rate(&mut self, rate: f32) -> Result<()> {
        let result = {
            let runtime = self.runtime_mut()?;
            runtime.set_playback_rate(rate)?
        };
        if let Some(frame) = result.frame {
            self.apply_frame(frame)?;
        }
        self.log_runtime_events();
        self.update_window_title();
        self.sync_ui_presenter();
        self.refresh_overlay()?;

        Ok(())
    }

    fn step_playback_rate(&mut self, step: i32) -> Result<()> {
        let current_rate = self.runtime()?.snapshot().playback_rate;
        let index = CONTROL_RATES
            .iter()
            .position(|(rate, _)| (*rate - current_rate).abs() < 0.05)
            .unwrap_or(1);
        let target = index
            .saturating_add_signed(step as isize)
            .clamp(0, CONTROL_RATES.len().saturating_sub(1));
        self.set_playback_rate(CONTROL_RATES[target].0)
    }

    fn handle_pointer_click(&mut self) -> Result<()> {
        if self.runtime.is_none() {
            return Ok(());
        }
        if self.renderer.is_none() {
            return Ok(());
        }

        let Some((cursor_x, cursor_y)) = self.cursor_position else {
            return Ok(());
        };
        let Some(window) = self.window.as_ref() else {
            return Ok(());
        };
        let Some(presenter) = self.ui_presenter.as_ref() else {
            return Ok(());
        };

        let window_size = window.inner_size();
        let snapshot = self.runtime()?.snapshot();
        let overlay_view_model = self.overlay_view_model(&snapshot);
        if let Some(action) = presenter.control_action_at(
            window_size,
            cursor_x,
            cursor_y,
            &snapshot,
            &overlay_view_model,
        ) {
            self.perform_control_action_logged("pointer_click", action)?;
        }

        Ok(())
    }

    fn perform_control_action_logged(
        &mut self,
        origin: &'static str,
        action: ControlAction,
    ) -> Result<()> {
        log_control_action(origin, action);
        self.perform_control_action(action)
    }

    fn perform_control_action(&mut self, action: ControlAction) -> Result<()> {
        let result = match action {
            ControlAction::SeekStart => self.seek_to(Duration::ZERO),
            ControlAction::SeekBack => self.seek_by(SEEK_STEP, false),
            ControlAction::TogglePause => self.dispatch_command(PlayerRuntimeCommand::TogglePause),
            ControlAction::Stop => self.dispatch_command(PlayerRuntimeCommand::Stop),
            ControlAction::SeekForward => self.seek_by(SEEK_STEP, true),
            ControlAction::SeekEnd => {
                let target = self
                    .runtime()?
                    .snapshot()
                    .media_info
                    .duration
                    .unwrap_or(Duration::ZERO);
                self.seek_to(target)
            }
            ControlAction::SetRate(rate) => self.set_playback_rate(rate),
            ControlAction::SeekToRatio(ratio) => {
                let snapshot = self.runtime()?.snapshot();
                let Some(position) = snapshot.timeline.position_for_ratio(f64::from(ratio)) else {
                    return Ok(());
                };
                self.seek_to(position)
            }
            ControlAction::OpenLocalFile => self.request_open_file_dialog(),
            ControlAction::OpenHlsDemo => self.open_media_source_with_label(
                DESKTOP_HLS_DEMO_URL.to_owned(),
                Some("HLS DEMO".to_owned()),
            ),
            ControlAction::OpenDashDemo => self.open_media_source_with_label(
                DESKTOP_DASH_DEMO_URL.to_owned(),
                Some("DASH DEMO".to_owned()),
            ),
            ControlAction::SelectSidebarTab(tab) => {
                self.sidebar_tab = tab;
                Ok(())
            }
            ControlAction::FocusPlaylistItem(index) => {
                let Some(entry) = self.playlist_entries.get(index).cloned() else {
                    return Ok(());
                };
                self.open_media_source_with_label(entry.source_uri, Some(entry.label))
            }
            ControlAction::CreateDownloadHlsDemo => {
                self.queue_download_planner(DESKTOP_HLS_DEMO_URL.to_owned(), "HLS DEMO".to_owned());
                Ok(())
            }
            ControlAction::CreateDownloadDashDemo => {
                self.queue_download_planner(
                    DESKTOP_DASH_DEMO_URL.to_owned(),
                    "DASH DEMO".to_owned(),
                );
                Ok(())
            }
            ControlAction::CreateDownloadCurrentSource => {
                self.queue_download_planner(self.source.clone(), self.current_source_label());
                Ok(())
            }
            ControlAction::DownloadPrimaryAction(task_id) => {
                self.download_controller
                    .trigger_primary_action(player_runtime::DownloadTaskId::from_raw(task_id))?;
                self.sidebar_tab = DesktopSidebarTab::Downloads;
                Ok(())
            }
            ControlAction::DownloadExport(task_id) => {
                self.download_controller
                    .request_export(player_runtime::DownloadTaskId::from_raw(task_id))?;
                self.sidebar_tab = DesktopSidebarTab::Downloads;
                Ok(())
            }
            ControlAction::DownloadRemove(task_id) => {
                self.download_controller
                    .remove_task(player_runtime::DownloadTaskId::from_raw(task_id))?;
                Ok(())
            }
        };

        self.sync_ui_presenter();
        self.refresh_overlay()?;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        result
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.resize(size);
        }
        self.sync_renderer_stage_viewport();
        if let Err(error) = self.refresh_overlay() {
            error!(?error, "failed to refresh overlay during resize");
        }
        self.sync_ui_presenter();
    }

    fn log_runtime_events(&mut self) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        for event in runtime.drain_events() {
            log_runtime_event(event);
        }
    }

    fn sync_ui_presenter(&self) {
        if let (Some(runtime), Some(ui_presenter), Some(window)) = (
            self.runtime.as_ref(),
            self.ui_presenter.as_ref(),
            self.window.as_ref(),
        ) {
            let snapshot = runtime.snapshot();
            let overlay_view_model = self.overlay_view_model(&snapshot);
            ui_presenter.sync(&snapshot, &overlay_view_model, window.inner_size());
        }
    }

    fn drain_ui_presenter_actions(&mut self) -> Result<()> {
        if let Some(ui_presenter) = self.ui_presenter.as_ref() {
            for action in ui_presenter.drain_actions() {
                self.perform_control_action_logged("presenter", action)?;
            }
        }

        Ok(())
    }

    fn show_controls(&mut self) {
        self.controls_visible = true;
        self.controls_hide_deadline = self
            .controls_should_auto_hide()
            .then_some(Instant::now() + CONTROL_HIDE_DELAY);
    }

    fn schedule_controls_hide(&mut self) {
        if self.controls_forced_visible() {
            return;
        }
        self.controls_hide_deadline = Some(Instant::now());
    }

    fn update_controls_visibility(&mut self) -> Result<bool> {
        let now = Instant::now();
        let mut changed = false;

        if self.controls_forced_visible() {
            if !self.controls_visible {
                self.controls_visible = true;
                changed = true;
            }
            self.controls_hide_deadline = None;
        } else if let Some(hide_deadline) = self.controls_hide_deadline
            && now >= hide_deadline
        {
            self.controls_hide_deadline = None;
            if self.controls_visible {
                self.controls_visible = false;
                changed = true;
            }
        }

        let elapsed = now.saturating_duration_since(self.controls_animation_tick);
        self.controls_animation_tick = now;
        let target_opacity = if self.controls_visible { 1.0 } else { 0.0 };
        let step = (elapsed.as_secs_f32() / CONTROL_FADE_DURATION.as_secs_f32()).clamp(0.0, 1.0);
        let previous_opacity = self.controls_opacity;
        if self.controls_opacity < target_opacity {
            self.controls_opacity = (self.controls_opacity + step).min(target_opacity);
        } else if self.controls_opacity > target_opacity {
            self.controls_opacity = (self.controls_opacity - step).max(target_opacity);
        }
        if (self.controls_opacity - previous_opacity).abs() > f32::EPSILON {
            changed = true;
        }

        Ok(changed)
    }

    fn controls_forced_visible(&self) -> bool {
        self.host_message.is_some()
            || self.active_launch_request_id.is_some()
            || self.seek_preview.is_some()
    }

    fn controls_should_auto_hide(&self) -> bool {
        self.pointer_inside_window && !self.controls_forced_visible()
    }

    fn controls_animation_deadline(&self) -> Option<Instant> {
        let target_opacity = if self.controls_visible { 1.0 } else { 0.0 };
        ((self.controls_opacity - target_opacity).abs() > 0.01)
            .then_some(Instant::now() + CONTROL_FADE_FRAME_INTERVAL)
    }
}

fn open_basic_player_runtime_for_window(
    source: &str,
    _window: &Window,
) -> Result<(
    PlayerRuntimeBootstrap,
    player_runtime::PlayerRuntimeAdapterCapabilities,
)> {
    #[cfg(target_os = "macos")]
    {
        // basic-player 的桌面控制层现在统一依赖 Rust overlay，因此在 macOS 上
        // 显式锁定 software desktop runtime。对于 HLS / DASH master manifest，
        // host strategy 在探测阶段可能暂时拿不到 best_video，随后误判成 native path，
        // 导致 overlay 失效、旧 frame 残留，并让切源看起来像“没有生效”。
        let bootstrap = PlayerRuntime::open_uri_with_options_and_factory(
            source.to_owned(),
            PlayerRuntimeOptions::default(),
            macos_runtime_adapter_factory(),
        )?;
        let capabilities = bootstrap.runtime.capabilities();
        return Ok((bootstrap, capabilities));
    }

    #[cfg(not(target_os = "macos"))]
    {
        open_desktop_host_runtime_uri_for_winit_window(source.to_owned(), _window)
    }
}

fn video_frame_texture(frame: &DecodedVideoFrame) -> VideoFrameTexture {
    match frame.pixel_format {
        VideoPixelFormat::Rgba8888 => VideoFrameTexture::Rgba(RgbaVideoFrame {
            width: frame.width,
            height: frame.height,
            bytes: frame.bytes.clone(),
        }),
        VideoPixelFormat::Yuv420p => VideoFrameTexture::Yuv420p(Yuv420pVideoFrame {
            width: frame.width,
            height: frame.height,
            bytes: frame.bytes.clone(),
        }),
    }
}

fn video_pixel_format_label(frame: &DecodedVideoFrame) -> &'static str {
    match frame.pixel_format {
        VideoPixelFormat::Rgba8888 => "rgba8888",
        VideoPixelFormat::Yuv420p => "yuv420p",
    }
}

fn log_control_action(origin: &'static str, action: ControlAction) {
    match action {
        ControlAction::SetRate(rate) => {
            info!(origin, rate, "desktop UI control action");
        }
        ControlAction::SeekToRatio(ratio) => {
            info!(origin, ratio, "desktop UI control action");
        }
        _ => {
            info!(origin, action = ?action, "desktop UI control action");
        }
    }
}

fn log_keyboard_action(action: &'static str) {
    info!(origin = "keyboard", action, "desktop keyboard action");
}

impl ApplicationHandler for DesktopPlayerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.initialize(event_loop) {
            error!(?error, "failed to initialize desktop player");
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.resize(size);
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = Some((position.x, position.y));
                self.pointer_inside_window = true;
                self.show_controls();
                if let Err(error) = self.update_seek_drag() {
                    error!(?error, "failed to update seek drag preview");
                    event_loop.exit();
                    return;
                }
                self.sync_ui_presenter();
                if let Err(error) = self.refresh_overlay() {
                    error!(?error, "failed to refresh overlay after cursor movement");
                    event_loop.exit();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if self.seek_preview.is_some() {
                    return;
                }
                self.pointer_inside_window = false;
                self.schedule_controls_hide();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Err(error) = self.begin_seek_drag() {
                    error!(?error, "failed to start seek drag");
                    event_loop.exit();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                match self.commit_seek_drag() {
                    Ok(true) => return,
                    Ok(false) => {}
                    Err(error) => {
                        error!(?error, "failed to commit seek drag");
                        event_loop.exit();
                        return;
                    }
                }
                if let Err(error) = self.handle_pointer_click() {
                    error!(?error, "failed to handle control bar click");
                    event_loop.exit();
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        repeat: false,
                        ..
                    },
                ..
            } => match logical_key.as_ref() {
                Key::Named(NamedKey::Escape) => event_loop.exit(),
                Key::Named(NamedKey::Space) => {
                    log_keyboard_action("toggle_pause");
                    self.toggle_pause();
                }
                Key::Named(NamedKey::ArrowLeft) => {
                    log_keyboard_action("seek_back");
                    if let Err(error) = self.seek_by(SEEK_STEP, false) {
                        error!(?error, "failed to seek backward");
                        event_loop.exit();
                    }
                }
                Key::Named(NamedKey::ArrowRight) => {
                    log_keyboard_action("seek_forward");
                    if let Err(error) = self.seek_by(SEEK_STEP, true) {
                        error!(?error, "failed to seek forward");
                        event_loop.exit();
                    }
                }
                Key::Named(NamedKey::Home) => {
                    log_keyboard_action("seek_start");
                    if let Err(error) = self.seek_to(Duration::ZERO) {
                        error!(?error, "failed to seek to start");
                        event_loop.exit();
                    }
                }
                Key::Named(NamedKey::End) => match self.runtime() {
                    Ok(runtime) => {
                        log_keyboard_action("seek_end");
                        let target = runtime
                            .snapshot()
                            .media_info
                            .duration
                            .unwrap_or(Duration::ZERO);
                        if let Err(error) = self.seek_to(target) {
                            error!(?error, "failed to seek to end");
                            event_loop.exit();
                        }
                    }
                    Err(error) => {
                        error!(
                            ?error,
                            "failed to access player runtime while seeking to end"
                        );
                        event_loop.exit();
                    }
                },
                Key::Character(text) if text.eq_ignore_ascii_case("s") => {
                    log_keyboard_action("stop");
                    if let Err(error) = self.dispatch_command(PlayerRuntimeCommand::Stop) {
                        error!(?error, "failed to stop playback");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text == "[" => {
                    log_keyboard_action("rate_down");
                    if let Err(error) = self.step_playback_rate(-1) {
                        error!(?error, "failed to step playback rate backward");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text == "]" => {
                    log_keyboard_action("rate_up");
                    if let Err(error) = self.step_playback_rate(1) {
                        error!(?error, "failed to step playback rate forward");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text == "0" => {
                    log_keyboard_action("set_rate_0_5x");
                    if let Err(error) = self.set_playback_rate(0.5) {
                        error!(?error, "failed to set playback rate");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text == "1" => {
                    log_keyboard_action("set_rate_1x");
                    if let Err(error) = self.set_playback_rate(1.0) {
                        error!(?error, "failed to set playback rate");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text == "2" => {
                    log_keyboard_action("set_rate_2x");
                    if let Err(error) = self.set_playback_rate(2.0) {
                        error!(?error, "failed to set playback rate");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text == "3" => {
                    log_keyboard_action("set_rate_3x");
                    if let Err(error) = self.set_playback_rate(3.0) {
                        error!(?error, "failed to set playback rate");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text.eq_ignore_ascii_case("h") => {
                    log_keyboard_action("open_hls_demo");
                    if let Err(error) = self.open_media_source(DESKTOP_HLS_DEMO_URL.to_owned()) {
                        error!(?error, "failed to open desktop HLS demo source");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text.eq_ignore_ascii_case("o") => {
                    log_keyboard_action("open_local_file");
                    if let Err(error) = self.perform_control_action(ControlAction::OpenLocalFile) {
                        error!(?error, "failed to open local media file");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text.eq_ignore_ascii_case("d") => {
                    log_keyboard_action("open_dash_demo");
                    if let Err(error) = self.open_media_source(DESKTOP_DASH_DEMO_URL.to_owned()) {
                        error!(?error, "failed to open desktop DASH demo source");
                        event_loop.exit();
                    }
                }
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.handle_redraw() {
                    error!(?error, "failed to render frame");
                    event_loop.exit();
                }
            }
            WindowEvent::DroppedFile(path) => {
                if let Err(error) = self.open_dropped_file(path) {
                    error!(?error, "failed to open dropped media source");
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match self.update_controls_visibility() {
            Ok(changed) => {
                if changed {
                    self.sync_ui_presenter();
                    if let Err(error) = self.refresh_overlay() {
                        error!(
                            ?error,
                            "failed to refresh overlay after control visibility update"
                        );
                        event_loop.exit();
                        return;
                    }
                }
            }
            Err(error) => {
                error!(?error, "failed to update control visibility");
                event_loop.exit();
                return;
            }
        }

        match self.drain_launch_events() {
            Ok(changed) => {
                if changed {
                    self.sync_ui_presenter();
                }
            }
            Err(error) => {
                error!(?error, "failed to handle prepared desktop media launches");
                event_loop.exit();
                return;
            }
        }

        match self.drain_file_dialog_events() {
            Ok(changed) => {
                if changed {
                    self.sync_ui_presenter();
                }
            }
            Err(error) => {
                error!(?error, "failed to handle local file dialog events");
                event_loop.exit();
                return;
            }
        }

        match self.drain_planner_events() {
            Ok(changed) => {
                if changed {
                    self.sync_ui_presenter();
                    if let Err(error) = self.refresh_overlay() {
                        error!(?error, "failed to refresh overlay after planning downloads");
                        event_loop.exit();
                        return;
                    }
                }
            }
            Err(error) => {
                error!(?error, "failed to handle prepared desktop download tasks");
                event_loop.exit();
                return;
            }
        }

        match self.drain_download_updates() {
            Ok(changed) => {
                if changed {
                    self.sync_ui_presenter();
                    if let Err(error) = self.refresh_overlay() {
                        error!(
                            ?error,
                            "failed to refresh overlay after desktop download update"
                        );
                        event_loop.exit();
                        return;
                    }
                }
            }
            Err(error) => {
                error!(?error, "failed to process desktop download updates");
                event_loop.exit();
                return;
            }
        }

        if let Err(error) = self.drain_ui_presenter_actions() {
            error!(?error, "failed to handle desktop UI presenter action");
            event_loop.exit();
            return;
        }

        if let Err(error) = self.advance_playback() {
            error!(?error, "failed to advance playback");
            event_loop.exit();
            return;
        }

        self.log_runtime_events();
        self.update_window_title();
        self.sync_ui_presenter();
        let controls_animation_deadline = self.controls_animation_deadline();
        if let Some(runtime) = self.runtime.as_ref() {
            if let Some(deadline) = runtime.next_deadline() {
                let mut next_deadline = deadline;
                if let Some(hide_deadline) = self.controls_hide_deadline {
                    next_deadline = next_deadline.min(hide_deadline);
                }
                if let Some(animation_deadline) = controls_animation_deadline {
                    next_deadline = next_deadline.min(animation_deadline);
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(next_deadline));
            } else if self.uses_external_video_surface {
                let mut next_deadline = Instant::now() + NATIVE_SURFACE_POLL_INTERVAL;
                if let Some(hide_deadline) = self.controls_hide_deadline {
                    next_deadline = next_deadline.min(hide_deadline);
                }
                if let Some(animation_deadline) = controls_animation_deadline {
                    next_deadline = next_deadline.min(animation_deadline);
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(next_deadline));
            } else if let Some(hide_deadline) = self.controls_hide_deadline {
                let next_deadline = controls_animation_deadline
                    .map(|animation_deadline| hide_deadline.min(animation_deadline))
                    .unwrap_or(hide_deadline);
                event_loop.set_control_flow(ControlFlow::WaitUntil(next_deadline));
            } else if let Some(animation_deadline) = controls_animation_deadline {
                event_loop.set_control_flow(ControlFlow::WaitUntil(animation_deadline));
            } else {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        } else if self.active_launch_request_id.is_some() {
            if let Some(hide_deadline) = self.controls_hide_deadline {
                let next_deadline = controls_animation_deadline
                    .map(|animation_deadline| hide_deadline.min(animation_deadline))
                    .unwrap_or(hide_deadline);
                event_loop.set_control_flow(ControlFlow::WaitUntil(next_deadline));
            } else if let Some(animation_deadline) = controls_animation_deadline {
                event_loop.set_control_flow(ControlFlow::WaitUntil(animation_deadline));
            } else {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        } else if let Some(animation_deadline) = controls_animation_deadline {
            event_loop.set_control_flow(ControlFlow::WaitUntil(animation_deadline));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

fn build_launch_plan(source: String) -> Result<RuntimeLaunchPlan> {
    let launch_probe = probe_desktop_host_launch_plan_uri(source)?;
    let probe = &launch_probe.runtime_probe;

    info!(
        source = launch_probe.launch_plan.source.as_str(),
        adapter_id = probe.adapter_id,
        ffmpeg_initialized = probe.startup.ffmpeg_initialized,
        audio_output = ?probe.startup.audio_output,
        video_decode = probe.startup.video_decode.as_ref().map(video_decode_summary),
        preferred_backends = ?preferred_backends(),
        media_info = ?probe.media_info,
        supports_frame_output = probe.capabilities.supports_frame_output,
        supports_external_video_surface = probe.capabilities.supports_external_video_surface,
        "probed media runtime"
    );

    Ok(launch_probe.launch_plan)
}

fn resolve_media_source_uri() -> Result<String> {
    if let Some(source) = std::env::args().nth(1) {
        return resolve_media_source_argument(source);
    }

    let default_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-video.mp4");
    canonical_desktop_host_local_path(&default_path)
}

fn resolve_media_source_argument(source: String) -> Result<String> {
    if source == HLS_DEMO_CLI_FLAG {
        return Ok(DESKTOP_HLS_DEMO_URL.to_owned());
    }
    if source == DASH_DEMO_CLI_FLAG {
        return Ok(DESKTOP_DASH_DEMO_URL.to_owned());
    }
    normalize_desktop_host_source_uri(source)
}

fn audio_summary(track: &DecodedAudioSummary) -> String {
    format!(
        "{}ch @ {}Hz ({:.2}s)",
        track.channels,
        track.sample_rate,
        track.duration.as_secs_f64()
    )
}

fn video_decode_summary(info: &PlayerVideoDecodeInfo) -> String {
    let selected = match info.selected_mode {
        PlayerVideoDecodeMode::Software => "software",
        PlayerVideoDecodeMode::Hardware => "hardware",
    };
    let backend = info
        .hardware_backend
        .as_deref()
        .unwrap_or("unknown-backend");
    let fallback = info
        .fallback_reason
        .as_deref()
        .unwrap_or("no-fallback-reason");

    format!(
        "selected={selected} hardware_available={} backend={backend} fallback={fallback}",
        info.hardware_available
    )
}

fn log_runtime_event(event: PlayerRuntimeEvent) {
    match event {
        PlayerRuntimeEvent::Initialized(startup) => {
            info!(
                ffmpeg_initialized = startup.ffmpeg_initialized,
                audio_output = ?startup.audio_output,
                decoded_audio = startup.decoded_audio.as_ref().map(audio_summary),
                video_decode = startup.video_decode.as_ref().map(video_decode_summary),
                "player initialized"
            );
        }
        PlayerRuntimeEvent::MetadataReady(media_info) => {
            info!(media_info = ?media_info, "player metadata ready");
        }
        PlayerRuntimeEvent::FirstFrameReady(first_frame) => {
            info!(
                presentation_time = first_frame.presentation_time.as_secs_f64(),
                width = first_frame.width,
                height = first_frame.height,
                "player first frame ready"
            );
        }
        PlayerRuntimeEvent::PlaybackStateChanged(state) => {
            info!(state = ?state, "player playback state changed");
        }
        PlayerRuntimeEvent::InterruptionChanged { interrupted } => {
            info!(interrupted, "player interruption state changed");
        }
        PlayerRuntimeEvent::BufferingChanged { buffering } => {
            info!(buffering, "player buffering state changed");
        }
        PlayerRuntimeEvent::VideoSurfaceChanged { attached } => {
            info!(attached, "player video surface changed");
        }
        PlayerRuntimeEvent::AudioOutputChanged(audio_output) => {
            info!(audio_output = ?audio_output, "player audio output changed");
        }
        PlayerRuntimeEvent::PlaybackRateChanged { rate } => {
            info!(playback_rate = rate, "player playback rate changed");
        }
        PlayerRuntimeEvent::SeekCompleted { position } => {
            info!(position = position.as_secs_f64(), "player seek completed");
        }
        PlayerRuntimeEvent::RetryScheduled { attempt, delay } => {
            info!(
                attempt,
                delay_ms = delay.as_millis(),
                "player retry scheduled"
            );
        }
        PlayerRuntimeEvent::Error(error) => {
            error!(code = ?error.code(), message = error.message(), "player runtime error");
        }
        PlayerRuntimeEvent::Ended => {
            info!("player playback ended");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DASH_DEMO_CLI_FLAG, DESKTOP_DASH_DEMO_URL, DESKTOP_HLS_DEMO_URL, HLS_DEMO_CLI_FLAG,
        resolve_media_source_argument,
    };

    #[test]
    fn resolve_media_source_argument_maps_demo_flags() {
        assert_eq!(
            resolve_media_source_argument(HLS_DEMO_CLI_FLAG.to_owned()).expect("hls demo"),
            DESKTOP_HLS_DEMO_URL
        );
        assert_eq!(
            resolve_media_source_argument(DASH_DEMO_CLI_FLAG.to_owned()).expect("dash demo"),
            DESKTOP_DASH_DEMO_URL
        );
    }
}

fn format_playback_progress(progress: PlaybackProgress) -> String {
    let current = format_duration(progress.position());
    match progress.duration() {
        Some(duration) => {
            let ratio = progress.ratio().unwrap_or(0.0) * 100.0;
            format!("{current} / {} ({ratio:.1}%)", format_duration(duration))
        }
        None => current,
    }
}

fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;

    format!("{minutes:02}:{seconds:02}")
}

fn source_display_label(source_uri: &str) -> String {
    if source_uri == DESKTOP_HLS_DEMO_URL {
        return "HLS DEMO".to_owned();
    }
    if source_uri == DESKTOP_DASH_DEMO_URL {
        return "DASH DEMO".to_owned();
    }
    draft_download_label("", source_uri)
}

fn active_source_subtitle(snapshot: &player_runtime::PlayerSnapshot) -> String {
    let protocol = match MediaSource::new(snapshot.source_uri.clone()).protocol() {
        player_core::MediaSourceProtocol::Hls => "HLS",
        player_core::MediaSourceProtocol::Dash => "DASH",
        player_core::MediaSourceProtocol::Progressive => "FILE",
        player_core::MediaSourceProtocol::File => "LOCAL",
        player_core::MediaSourceProtocol::Content => "CONTENT",
        player_core::MediaSourceProtocol::Unknown => "SOURCE",
    };
    let resolution = snapshot
        .media_info
        .best_video
        .as_ref()
        .map(|video| format!("{}X{}", video.width, video.height))
        .unwrap_or_else(|| "UNKNOWN".to_owned());
    format!("{protocol} {resolution}")
}
