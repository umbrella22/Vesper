use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
#[cfg(not(target_os = "macos"))]
use player_host_desktop::open_desktop_host_runtime_uri_with_options_and_interrupt;
use player_host_desktop::{
    DesktopHostLaunchPlan as RuntimeLaunchPlan, canonical_desktop_host_local_path,
    normalize_desktop_host_source_uri, render_config_from_media_info,
    runtime_options_for_winit_window,
};
use player_model::MediaSource;
#[cfg(target_os = "macos")]
use player_platform_macos::{
    MacosVideoLayerFrame, MacosVideoLayerSurface,
    open_macos_host_runtime_uri_with_options_and_interrupt,
};
use player_render_wgpu::{
    DisplayRect, RenderFrameOutcome, RenderMode, RenderSurfaceConfig, RgbaVideoFrame,
    VideoFrameTexture, VideoRenderer, Yuv420pVideoFrame, default_window_attributes,
};
use player_runtime::{
    DecodedAudioSummary, DecodedVideoFrame, MediaTrackCatalog, MediaTrackSelectionSnapshot,
    PlaybackProgress, PlayerDecoderPluginVideoMode, PlayerMediaInfo, PlayerPluginDiagnostic,
    PlayerPluginDiagnosticStatus, PlayerResilienceMetrics, PlayerRuntime, PlayerRuntimeBootstrap,
    PlayerRuntimeCommand, PlayerRuntimeEvent, PlayerRuntimeOptions, PlayerSnapshot,
    PlayerTimelineKind, PlayerTimelineSnapshot, PlayerVideoDecodeInfo, PlayerVideoDecodeMode,
    PresentationState, VideoPixelFormat,
};
use tracing::{debug, error, info, warn};
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
const PLAYBACK_OVERLAY_REFRESH_INTERVAL: Duration = Duration::from_millis(125);
const POST_LAUNCH_PLAY_PAINT_FALLBACK: Duration = Duration::from_millis(120);
const HLS_DEMO_CLI_FLAG: &str = "--hls-demo";
const DASH_DEMO_CLI_FLAG: &str = "--dash-demo";
const DECODER_PLUGIN_PATHS_ENV: &str = "VESPER_DECODER_PLUGIN_PATHS";
const DECODER_PLUGIN_VIDEO_MODE_ENV: &str = "VESPER_DECODER_PLUGIN_VIDEO_MODE";
const DESKTOP_HLS_DEMO_URL: &str = "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8";
const DESKTOP_DASH_DEMO_URL: &str = "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd";
const MIN_WINDOW_INNER_WIDTH: u32 = 1280;
const MIN_WINDOW_INNER_HEIGHT: u32 = 540;

#[derive(Debug, Clone)]
struct DesktopPlaylistEntry {
    source_uri: String,
    label: String,
}

struct PendingLaunchActivation {
    request_id: u64,
    source: String,
    label: String,
    prepare_duration: Duration,
    launch_plan: RuntimeLaunchPlan,
    prepared_bootstrap: PlayerRuntimeBootstrap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceLaunchStatus {
    Loading,
    Failed,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
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

#[allow(clippy::large_enum_variant)]
enum LaunchEvent {
    Prepared {
        request_id: u64,
        source: String,
        label: String,
        prepare_duration: Duration,
        launch_plan: RuntimeLaunchPlan,
        prepared_bootstrap: PlayerRuntimeBootstrap,
    },
    Failed {
        request_id: u64,
        label: String,
        prepare_duration: Duration,
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

    let source = resolve_initial_media_source_uri()?;

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
    source: Option<String>,
    runtime: Option<PlayerRuntime>,
    last_frame: Option<DecodedVideoFrame>,
    render_config: RenderSurfaceConfig,
    uses_external_video_surface: bool,
    window: Option<Arc<Window>>,
    renderer: Option<VideoRenderer>,
    #[cfg(target_os = "macos")]
    native_video_surface: Option<MacosVideoLayerSurface>,
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
    pending_launch_activation: Option<PendingLaunchActivation>,
    pending_launch_activation_needs_paint: bool,
    pending_post_launch_play: bool,
    pending_post_launch_play_needs_paint: bool,
    pending_post_launch_play_paint_deadline: Option<Instant>,
    file_dialog_tx: Sender<FileDialogEvent>,
    file_dialog_rx: Receiver<FileDialogEvent>,
    next_launch_request_id: u64,
    active_launch_request_id: Option<u64>,
    active_launch_cancel_flag: Option<Arc<AtomicBool>>,
    launch_status: Option<SourceLaunchStatus>,
    open_file_dialog_pending: bool,
    host_message: Option<String>,
    download_message: Option<String>,
    last_plugin_diagnostics_summary: Option<String>,
    overlay_dirty: bool,
    last_overlay_refresh_at: Option<Instant>,
    source_generation: u64,
    frame_sequence: u64,
    first_frame_upload_logged: bool,
    first_frame_present_logged: bool,
    last_uploaded_frame_sequence: Option<u64>,
}

impl DesktopPlayerApp {
    fn new(source: Option<String>) -> Self {
        let playlist_entries = source
            .as_ref()
            .map(|initial_source| DesktopPlaylistEntry {
                source_uri: initial_source.clone(),
                label: source_display_label(initial_source),
            })
            .into_iter()
            .collect();
        let (planner_tx, planner_rx) = mpsc::channel();
        let (launch_tx, launch_rx) = mpsc::channel();
        let (file_dialog_tx, file_dialog_rx) = mpsc::channel();
        Self {
            source,
            runtime: None,
            last_frame: None,
            render_config: RenderSurfaceConfig::default(),
            uses_external_video_surface: false,
            window: None,
            renderer: None,
            #[cfg(target_os = "macos")]
            native_video_surface: None,
            title_cache: None,
            cursor_position: None,
            pointer_inside_window: true,
            controls_visible: true,
            controls_hide_deadline: None,
            controls_opacity: 1.0,
            controls_animation_tick: Instant::now(),
            seek_preview: None,
            ui_presenter: None,
            playlist_entries,
            active_playlist_index: 0,
            sidebar_tab: DesktopSidebarTab::Playlist,
            download_controller: DesktopDownloadController::new(),
            pending_downloads: Vec::new(),
            planner_tx,
            planner_rx,
            launch_tx,
            launch_rx,
            pending_launch_activation: None,
            pending_launch_activation_needs_paint: false,
            pending_post_launch_play: false,
            pending_post_launch_play_needs_paint: false,
            pending_post_launch_play_paint_deadline: None,
            file_dialog_tx,
            file_dialog_rx,
            next_launch_request_id: 1,
            active_launch_request_id: None,
            active_launch_cancel_flag: None,
            launch_status: None,
            open_file_dialog_pending: false,
            host_message: None,
            download_message: None,
            last_plugin_diagnostics_summary: None,
            overlay_dirty: false,
            last_overlay_refresh_at: None,
            source_generation: 0,
            frame_sequence: 0,
            first_frame_upload_logged: false,
            first_frame_present_logged: false,
            last_uploaded_frame_sequence: None,
        }
    }

    fn overlay_view_model(
        &self,
        snapshot: &player_runtime::PlayerSnapshot,
    ) -> DesktopOverlayViewModel {
        let playlist_items = self.playlist_item_view_data();
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
            .unwrap_or_else(|| self.current_source_label());
        let subtitle = match self.last_plugin_diagnostics_summary.as_deref() {
            Some(summary) if !summary.is_empty() => {
                format!("{} · {summary}", active_source_subtitle(snapshot))
            }
            _ => active_source_subtitle(snapshot),
        };

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
        let source_uri = self.source.clone().unwrap_or_default();
        let source = MediaSource::new(source_uri.clone());
        PlayerSnapshot {
            source_uri: source_uri.clone(),
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
                source_uri,
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
            .or_else(|| {
                self.source
                    .as_ref()
                    .map(|source| source_display_label(source))
            })
            .unwrap_or_else(|| "Drop a video to start".to_owned())
    }

    fn active_playlist_status_label(&self) -> &'static str {
        if self.active_launch_request_id.is_some() {
            return "LOADING";
        }
        if self.launch_status == Some(SourceLaunchStatus::Failed) {
            return "FAILED";
        }
        "CURRENT"
    }

    fn playlist_item_view_data(&self) -> Vec<DesktopPlaylistItemViewData> {
        self.playlist_entries
            .iter()
            .enumerate()
            .map(|(index, entry)| DesktopPlaylistItemViewData {
                label: entry.label.clone(),
                status: if index == self.active_playlist_index {
                    self.active_playlist_status_label().to_owned()
                } else {
                    "READY".to_owned()
                },
                is_active: index == self.active_playlist_index,
            })
            .collect()
    }

    fn reset_active_playback_for_launch(&mut self) {
        self.runtime = None;
        self.last_frame = None;
        self.last_overlay_refresh_at = None;
        self.uses_external_video_surface = false;
        #[cfg(target_os = "macos")]
        {
            self.native_video_surface = None;
        }
        self.seek_preview = None;
        self.frame_sequence = 0;
        self.first_frame_upload_logged = false;
        self.first_frame_present_logged = false;
        self.last_uploaded_frame_sequence = None;
        self.pending_post_launch_play_paint_deadline = None;
    }

    fn replace_active_launch_cancel_flag(&mut self) -> Arc<AtomicBool> {
        if let Some(cancel_flag) = self.active_launch_cancel_flag.take() {
            cancel_flag.store(true, Ordering::SeqCst);
        }

        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_launch_cancel_flag = Some(cancel_flag.clone());
        cancel_flag
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

    fn stage_display_rect_for_size(
        &self,
        size: PhysicalSize<u32>,
        window_scale_factor: f64,
    ) -> DisplayRect {
        let stage_rect = playback_stage_rect(size.width, size.height, window_scale_factor);
        DisplayRect {
            x: stage_rect.x,
            y: stage_rect.y,
            width: stage_rect.width.max(1),
            height: stage_rect.height.max(1),
        }
    }

    fn sync_renderer_stage_viewport(&mut self) {
        let (size, window_scale_factor) = self
            .window
            .as_ref()
            .map(|window| (window.inner_size(), window.scale_factor()))
            .unwrap_or((
                PhysicalSize::new(
                    self.render_config.width.max(1),
                    self.render_config.height.max(1),
                ),
                1.0,
            ));
        let stage_rect = self.stage_display_rect_for_size(size, window_scale_factor);
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_video_viewport(Some(stage_rect));
        }
    }

    #[cfg(target_os = "macos")]
    fn native_video_layer_frame_for_window(&self, window: &Window) -> MacosVideoLayerFrame {
        let stage_rect =
            self.stage_display_rect_for_size(window.inner_size(), window.scale_factor());
        let scale_factor = window.scale_factor().max(1.0);
        MacosVideoLayerFrame {
            x: f64::from(stage_rect.x) / scale_factor,
            y: f64::from(stage_rect.y) / scale_factor,
            width: f64::from(stage_rect.width.max(1)) / scale_factor,
            height: f64::from(stage_rect.height.max(1)) / scale_factor,
        }
    }

    #[cfg(target_os = "macos")]
    fn ensure_native_video_surface(
        &mut self,
        window: &Window,
    ) -> Result<player_runtime::PlayerVideoSurfaceTarget> {
        let frame = self.native_video_layer_frame_for_window(window);
        if let Some(surface) = self.native_video_surface.as_ref() {
            surface.update_frame(frame)?;
            return Ok(surface.target());
        }

        let host_surface =
            runtime_options_for_winit_window(window, PlayerRuntimeOptions::default())?
                .video_surface
                .context("macOS window did not expose an NSView video surface")?;
        let surface = MacosVideoLayerSurface::new(host_surface, frame)?;
        let target = surface.target();
        self.native_video_surface = Some(surface);
        Ok(target)
    }

    #[cfg(target_os = "macos")]
    fn sync_native_video_surface_frame(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let frame = self.native_video_layer_frame_for_window(window.as_ref());
        if let Some(surface) = self.native_video_surface.as_ref()
            && let Err(error) = surface.update_frame(frame)
        {
            warn!(?error, "failed to update macOS native video surface frame");
        }
    }

    fn request_source_launch(&mut self, source: String, label: Option<String>) -> Result<()> {
        let label = label.unwrap_or_else(|| source_display_label(&source));
        let request_id = self.next_launch_request_id;
        self.next_launch_request_id = self.next_launch_request_id.saturating_add(1);
        self.source_generation = self.source_generation.wrapping_add(1);
        let source_generation = self.source_generation;
        let cancel_flag = self.replace_active_launch_cancel_flag();
        self.source = Some(source.clone());
        self.register_playlist_source(&source, Some(label.clone()));
        self.active_launch_request_id = Some(request_id);
        self.pending_launch_activation = None;
        self.pending_launch_activation_needs_paint = false;
        self.pending_post_launch_play = false;
        self.pending_post_launch_play_needs_paint = false;
        self.pending_post_launch_play_paint_deadline = None;
        self.launch_status = Some(SourceLaunchStatus::Loading);
        self.reset_active_playback_for_launch();
        self.host_message = Some(format!("LOADING {label}"));
        self.show_controls();
        self.title_cache = None;
        self.update_window_title();
        self.sync_ui_presenter();
        self.refresh_overlay()?;

        let window = self
            .window
            .as_ref()
            .cloned()
            .context("window missing while preparing desktop source launch")?;

        #[cfg(target_os = "macos")]
        let runtime_options = {
            let video_surface = self.ensure_native_video_surface(window.as_ref())?;
            let options = basic_player_runtime_options().with_video_surface(video_surface);
            runtime_options_for_winit_window(window.as_ref(), options)?
        };
        #[cfg(not(target_os = "macos"))]
        let runtime_options =
            runtime_options_for_winit_window(window.as_ref(), basic_player_runtime_options())?;

        let launch_tx = self.launch_tx.clone();
        thread::spawn(move || {
            let prepare_started_at = Instant::now();
            let event = match open_basic_player_runtime_for_source(
                &source,
                runtime_options,
                cancel_flag.clone(),
            ) {
                Ok((prepared_bootstrap, capabilities)) => {
                    let launch_plan = RuntimeLaunchPlan {
                        source: source.clone(),
                        render_config: render_config_from_media_info(
                            prepared_bootstrap.runtime.media_info(),
                        ),
                    };
                    info!(
                        source = source.as_str(),
                        source_generation,
                        adapter_id = prepared_bootstrap.runtime.adapter_id(),
                        prepare_ms = prepare_started_at.elapsed().as_millis(),
                        supports_frame_output = capabilities.supports_frame_output,
                        supports_external_video_surface =
                            capabilities.supports_external_video_surface,
                        "prepared desktop runtime off the main thread"
                    );
                    LaunchEvent::Prepared {
                        request_id,
                        source,
                        label,
                        prepare_duration: prepare_started_at.elapsed(),
                        launch_plan,
                        prepared_bootstrap,
                    }
                }
                Err(error) => LaunchEvent::Failed {
                    request_id,
                    label,
                    prepare_duration: prepare_started_at.elapsed(),
                    error: error.to_string(),
                },
            };
            if cancel_flag.load(Ordering::SeqCst) {
                return;
            }
            let _ = launch_tx.send(event);
        });

        Ok(())
    }

    fn queue_download_planner(&mut self, source_uri: String, label: String) {
        let asset_prefix = match MediaSource::new(source_uri.clone()).protocol() {
            player_model::MediaSourceProtocol::Hls => "hls",
            player_model::MediaSourceProtocol::Dash => "dash",
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
                    prepare_duration,
                    launch_plan,
                    prepared_bootstrap,
                }) => {
                    if self.active_launch_request_id != Some(request_id) {
                        continue;
                    }
                    self.pending_launch_activation = Some(PendingLaunchActivation {
                        request_id,
                        source,
                        label,
                        prepare_duration,
                        launch_plan,
                        prepared_bootstrap,
                    });
                    self.pending_launch_activation_needs_paint = true;
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                    changed = true;
                }
                Ok(LaunchEvent::Failed {
                    request_id,
                    label,
                    prepare_duration,
                    error,
                }) => {
                    if self.active_launch_request_id != Some(request_id) {
                        continue;
                    }
                    self.active_launch_request_id = None;
                    self.active_launch_cancel_flag = None;
                    self.pending_launch_activation = None;
                    self.pending_launch_activation_needs_paint = false;
                    self.launch_status = Some(SourceLaunchStatus::Failed);
                    self.host_message = Some(format!("FAILED TO LOAD {label}"));
                    warn!(
                        label = label.as_str(),
                        prepare_ms = prepare_duration.as_millis(),
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

    fn activate_pending_launch_if_needed(&mut self) -> Result<bool> {
        if self.pending_launch_activation_needs_paint {
            self.pending_launch_activation_needs_paint = false;
            return Ok(false);
        }

        let Some(pending) = self.pending_launch_activation.take() else {
            return Ok(false);
        };

        if self.active_launch_request_id != Some(pending.request_id) {
            return Ok(false);
        }

        let window = self
            .window
            .as_ref()
            .cloned()
            .context("window missing while activating deferred launch plan")?;

        info!(
            source = pending.source.as_str(),
            prepare_ms = pending.prepare_duration.as_millis(),
            "desktop launch plan prepared"
        );

        let activation_result = self.commit_launch_bootstrap(
            pending.launch_plan,
            pending.prepared_bootstrap,
            pending.prepare_duration,
            window.clone(),
        );

        match activation_result {
            Ok(()) => {
                self.active_launch_request_id = None;
                self.active_launch_cancel_flag = None;
                self.launch_status = None;
                self.register_playlist_source(&pending.source, Some(pending.label));
                self.sync_ui_presenter();
                self.refresh_overlay_ui_only()?;
                window.request_redraw();
            }
            Err(error) => {
                self.active_launch_request_id = None;
                self.active_launch_cancel_flag = None;
                self.launch_status = Some(SourceLaunchStatus::Failed);
                warn!(
                    ?error,
                    source = pending.source.as_str(),
                    "failed to activate media source"
                );
                self.host_message = Some("FAILED TO OPEN SOURCE".to_owned());
                self.refresh_overlay()?;
            }
        }

        Ok(true)
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

        let window = Arc::new(
            event_loop.create_window(
                default_window_attributes(self.render_config)
                    .with_title(self.window_title())
                    .with_min_inner_size(PhysicalSize::new(
                        MIN_WINDOW_INNER_WIDTH,
                        MIN_WINDOW_INNER_HEIGHT,
                    )),
            )?,
        );
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
        self.ensure_renderer_ready(window.clone())?;
        if let Some(source) = self.source.clone() {
            self.request_source_launch(source, Some(self.current_source_label()))?;
        } else {
            self.host_message = Some("DROP A MEDIA FILE OR OPEN ONE".to_owned());
            self.update_window_title();
            self.sync_ui_presenter();
            self.refresh_overlay_ui_only()?;
            window.request_redraw();
        }

        Ok(())
    }

    fn commit_launch_bootstrap(
        &mut self,
        launch_plan: RuntimeLaunchPlan,
        PlayerRuntimeBootstrap {
            runtime,
            initial_frame,
            startup,
        }: PlayerRuntimeBootstrap,
        runtime_open_duration: Duration,
        window: Arc<Window>,
    ) -> Result<()> {
        let activation_started_at = Instant::now();
        let capabilities = runtime.capabilities();
        let mut renderer_ready_duration = None;

        info!(
            adapter_id = runtime.adapter_id(),
            source = launch_plan.source.as_str(),
            decoded_audio = startup.decoded_audio.as_ref().map(audio_summary),
            video_decode = startup.video_decode.as_ref().map(video_decode_summary),
            plugin_diagnostics = plugin_diagnostics_summary(&startup.plugin_diagnostics).as_deref(),
            initial_pixel_format = initial_frame.as_ref().map(video_pixel_format_label),
            supports_frame_output = capabilities.supports_frame_output,
            supports_external_video_surface = capabilities.supports_external_video_surface,
            runtime_open_ms = runtime_open_duration.as_millis(),
            "initialized desktop player"
        );

        let activated_source = launch_plan.source.clone();
        self.source = Some(launch_plan.source);
        self.render_config = launch_plan.render_config;
        self.uses_external_video_surface = capabilities.supports_external_video_surface;
        self.runtime = Some(runtime);
        self.seek_preview = None;
        self.host_message = None;
        self.last_plugin_diagnostics_summary =
            plugin_diagnostics_summary(&startup.plugin_diagnostics);

        if capabilities.supports_frame_output {
            #[cfg(target_os = "macos")]
            {
                self.native_video_surface = None;
            }
            let initial_frame =
                initial_frame.context("desktop runtime did not provide an initial frame")?;
            let renderer_started_at = Instant::now();
            self.ensure_renderer_ready_with_frame_size(
                window.clone(),
                (initial_frame.width, initial_frame.height),
            )?;
            renderer_ready_duration = Some(renderer_started_at.elapsed());
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.set_render_mode(RenderMode::Fit);
            }
            self.sync_renderer_stage_viewport();
            self.last_frame = None;
            self.apply_frame(initial_frame)?;
        } else {
            self.renderer = None;
            self.last_frame = None;
            #[cfg(target_os = "macos")]
            {
                if capabilities.supports_external_video_surface {
                    self.sync_native_video_surface_frame();
                } else {
                    self.native_video_surface = None;
                }
            }
        }

        self.title_cache = None;
        self.show_controls();
        self.update_window_title();
        self.sync_ui_presenter();
        self.pending_post_launch_play = true;
        self.pending_post_launch_play_needs_paint = true;
        self.pending_post_launch_play_paint_deadline =
            Some(Instant::now() + POST_LAUNCH_PLAY_PAINT_FALLBACK);
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }

        info!(
            source = activated_source.as_str(),
            runtime_open_ms = runtime_open_duration.as_millis(),
            renderer_ready_ms = renderer_ready_duration.map(|duration| duration.as_millis() as u64),
            activation_ms = activation_started_at.elapsed().as_millis(),
            "desktop launch plan activated"
        );

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
        if self.renderer.is_none() {
            return Ok(());
        };

        window.pre_present_notify();
        let outcome = self
            .renderer
            .as_mut()
            .context("renderer missing during redraw")?
            .render_with_outcome()?;
        self.observe_render_outcome(outcome);
        Ok(())
    }

    fn observe_render_outcome(&mut self, outcome: RenderFrameOutcome) {
        match outcome {
            RenderFrameOutcome::Presented => {
                if self.pending_post_launch_play_needs_paint {
                    self.pending_post_launch_play_needs_paint = false;
                    self.pending_post_launch_play_paint_deadline = None;
                    info!(
                        source_generation = self.source_generation,
                        "desktop deferred play paint barrier satisfied"
                    );
                }
                if self.last_uploaded_frame_sequence.is_some() && !self.first_frame_present_logged {
                    self.first_frame_present_logged = true;
                    info!(
                        source_generation = self.source_generation,
                        frame_sequence = ?self.last_uploaded_frame_sequence,
                        "desktop renderer presented first playback frame"
                    );
                } else {
                    debug!(
                        source_generation = self.source_generation,
                        frame_sequence = ?self.last_uploaded_frame_sequence,
                        "desktop renderer presented frame"
                    );
                }
            }
            RenderFrameOutcome::Timeout
            | RenderFrameOutcome::Occluded
            | RenderFrameOutcome::SurfaceReconfigured => {
                if self.pending_post_launch_play_needs_paint {
                    info!(
                        source_generation = self.source_generation,
                        outcome = ?outcome,
                        "desktop renderer skipped while waiting to start playback"
                    );
                } else {
                    debug!(
                        source_generation = self.source_generation,
                        outcome = ?outcome,
                        "desktop renderer skipped frame"
                    );
                }
            }
        }
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
        self.frame_sequence = self.frame_sequence.wrapping_add(1);
        let frame_sequence = self.frame_sequence;
        debug!(
            source_generation = self.source_generation,
            frame_sequence,
            presentation_time_secs = frame.presentation_time.as_secs_f64(),
            width = frame.width,
            height = frame.height,
            pixel_format = video_pixel_format_label(&frame),
            "desktop playback frame ready for upload"
        );
        self.last_frame = Some(frame);
        self.refresh_playback_frame()
    }

    fn refresh_playback_frame(&mut self) -> Result<()> {
        let upload_overlay = self.overlay_dirty || self.playback_overlay_tick_due();
        self.refresh_overlay_with_video_frame(true, upload_overlay)
    }

    fn refresh_overlay(&mut self) -> Result<()> {
        self.refresh_overlay_with_video_frame(true, true)
    }

    fn refresh_overlay_ui_only(&mut self) -> Result<()> {
        self.refresh_overlay_with_video_frame(false, true)
    }

    fn refresh_overlay_with_video_frame(
        &mut self,
        upload_video_frame: bool,
        upload_overlay: bool,
    ) -> Result<()> {
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
            if self.uses_external_video_surface {
                self.sync_ui_presenter();
                self.overlay_dirty = false;
                self.last_overlay_refresh_at = Some(Instant::now());
                return Ok(());
            }
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.clear_overlay();
            }
            self.overlay_dirty = false;
            return Ok(());
        };
        let window_size = window.inner_size();
        let window_scale_factor = window.scale_factor();
        let overlay = if upload_overlay {
            let snapshot = self.runtime()?.snapshot();
            let overlay_view_model = self.overlay_view_model(&snapshot);
            self.ui_presenter.as_ref().and_then(|presenter| {
                presenter.overlay_frame(
                    window_size,
                    window_scale_factor,
                    &snapshot,
                    self.seek_preview,
                    &overlay_view_model,
                )
            })
        } else {
            None
        };
        let frame_texture = upload_video_frame.then(|| video_frame_texture(frame));
        let Some(renderer) = self.renderer.as_mut() else {
            if self.uses_external_video_surface {
                self.sync_ui_presenter();
                self.last_overlay_refresh_at = Some(Instant::now());
            }
            self.overlay_dirty = false;
            return Ok(());
        };
        if window_size.width == 0 || window_size.height == 0 {
            renderer.clear_overlay();
            self.overlay_dirty = false;
            return Ok(());
        }

        if let Some(frame_texture) = frame_texture.as_ref() {
            renderer.upload_frame(frame_texture);
            self.last_uploaded_frame_sequence = Some(self.frame_sequence);
            if !self.first_frame_upload_logged {
                self.first_frame_upload_logged = true;
                info!(
                    source_generation = self.source_generation,
                    frame_sequence = self.frame_sequence,
                    "desktop renderer uploaded first playback frame"
                );
            } else {
                debug!(
                    source_generation = self.source_generation,
                    frame_sequence = self.frame_sequence,
                    "desktop renderer uploaded playback frame"
                );
            }
        }
        if upload_overlay {
            if let Some(overlay) = overlay {
                renderer.upload_overlay(&overlay);
            } else {
                renderer.clear_overlay();
            }
            self.overlay_dirty = false;
            self.last_overlay_refresh_at = Some(Instant::now());
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
                self.overlay_dirty = false;
                self.last_overlay_refresh_at = Some(Instant::now());
                window.request_redraw();
            }
            return Ok(());
        }
        self.ensure_renderer_ready(window.clone())?;
        self.sync_renderer_stage_viewport();

        let snapshot = self.host_snapshot();
        let overlay_view_model = self.overlay_view_model(&snapshot);
        let overlay = self.ui_presenter.as_ref().and_then(|presenter| {
            presenter.overlay_frame(
                window_size,
                window.scale_factor(),
                &snapshot,
                None,
                &overlay_view_model,
            )
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
        self.overlay_dirty = false;
        self.last_overlay_refresh_at = Some(Instant::now());
        window.request_redraw();
        Ok(())
    }

    fn playback_overlay_tick_due(&self) -> bool {
        if self.last_overlay_refresh_at.is_none() {
            return true;
        }
        if self.controls_opacity <= 0.01 || self.seek_preview.is_some() {
            return false;
        }
        let Some(runtime) = self.runtime.as_ref() else {
            return false;
        };
        if runtime.snapshot().state != PresentationState::Playing {
            return false;
        }
        self.last_overlay_refresh_at
            .is_some_and(|last_refresh| last_refresh.elapsed() >= PLAYBACK_OVERLAY_REFRESH_INTERVAL)
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

    fn ensure_renderer_ready(&mut self, window: Arc<Window>) -> Result<()> {
        self.ensure_renderer_ready_with_frame_size(
            window,
            (
                self.render_config.width.max(1),
                self.render_config.height.max(1),
            ),
        )
    }

    fn ensure_renderer_ready_with_frame_size(
        &mut self,
        window: Arc<Window>,
        frame_size: (u32, u32),
    ) -> Result<()> {
        if self.renderer.is_none() {
            let mut renderer = pollster::block_on(VideoRenderer::new(window, frame_size))?;
            renderer.set_render_mode(RenderMode::Fit);
            self.renderer = Some(renderer);
        }
        Ok(())
    }

    fn window_title(&self) -> String {
        let Some(source) = self.source.as_ref() else {
            return "Vesper basic player - Drop media to start".to_owned();
        };
        let source_name = Path::new(source)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("media");
        let Some(runtime) = self.runtime.as_ref() else {
            let launch_state = match self.launch_status {
                Some(SourceLaunchStatus::Loading) => "Opening",
                Some(SourceLaunchStatus::Failed) => "Failed",
                None => "Opening",
            };
            return format!("Vesper basic player - {launch_state} - {source_name}");
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
        let frame = result.frame;
        let snapshot = result.snapshot;
        self.log_runtime_events();
        self.update_window_title();
        self.sync_ui_presenter();
        if let Some(frame) = frame {
            self.overlay_dirty = true;
            self.apply_frame(frame)?;
        } else {
            self.refresh_overlay_ui_only()?;
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }

        let _ = snapshot;
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
        let window_scale_factor = window.scale_factor();
        let Some(preview) = presenter.seek_preview_at(
            window_size,
            window_scale_factor,
            cursor_x,
            cursor_y,
            &snapshot,
            &overlay_view_model,
        ) else {
            return Ok(false);
        };

        self.seek_preview = Some(preview);
        self.show_controls();
        self.refresh_overlay_ui_only()?;
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
        if let Some(preview) = presenter.seek_preview_for_drag(
            window_size,
            window.scale_factor(),
            cursor_x,
            &snapshot,
            &overlay_view_model,
        ) {
            self.seek_preview = Some(preview);
            self.overlay_dirty = true;
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
        self.refresh_overlay_ui_only()?;
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
        let frame = result.frame;
        self.log_runtime_events();
        self.update_window_title();
        self.sync_ui_presenter();
        if let Some(frame) = frame {
            self.overlay_dirty = true;
            self.apply_frame(frame)?;
        } else {
            self.refresh_overlay_ui_only()?;
        }

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
        if self.renderer.is_none() && !self.uses_external_video_surface {
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
        let window_scale_factor = window.scale_factor();
        let snapshot = self
            .runtime
            .as_ref()
            .map(PlayerRuntime::snapshot)
            .unwrap_or_else(|| self.host_snapshot());
        let overlay_view_model = self.overlay_view_model(&snapshot);
        if let Some(action) = presenter.control_action_at(
            window_size,
            window_scale_factor,
            cursor_x,
            cursor_y,
            &snapshot,
            &overlay_view_model,
        ) && self.is_control_action_available(action)
        {
            self.perform_control_action_logged("pointer_click", action)?;
        }

        Ok(())
    }

    fn is_control_action_available(&self, action: ControlAction) -> bool {
        if self.runtime.is_some() {
            return true;
        }

        match action {
            ControlAction::OpenLocalFile
            | ControlAction::OpenHlsDemo
            | ControlAction::OpenDashDemo
            | ControlAction::SelectSidebarTab(_)
            | ControlAction::FocusPlaylistItem(_)
            | ControlAction::CreateDownloadHlsDemo
            | ControlAction::CreateDownloadDashDemo
            | ControlAction::DownloadPrimaryAction(_)
            | ControlAction::DownloadExport(_)
            | ControlAction::DownloadRemove(_) => true,
            ControlAction::CreateDownloadCurrentSource => self.source.is_some(),
            _ => false,
        }
    }

    fn perform_control_action_logged(
        &mut self,
        origin: &'static str,
        action: ControlAction,
    ) -> Result<()> {
        log_control_action(origin, action);
        self.perform_control_action(action)
    }

    fn perform_keyboard_control_action(
        &mut self,
        event_loop: &ActiveEventLoop,
        log_label: &'static str,
        action: ControlAction,
    ) {
        log_keyboard_action(log_label);
        if !self.is_control_action_available(action) {
            return;
        }
        if let Err(error) = self.perform_control_action(action) {
            error!(?error, "failed to handle keyboard control action");
            event_loop.exit();
        }
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
                let Some(source) = self.source.clone() else {
                    self.download_message = Some("No current source to download".to_owned());
                    self.sidebar_tab = DesktopSidebarTab::Downloads;
                    return Ok(());
                };
                self.queue_download_planner(source, self.current_source_label());
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
        self.refresh_overlay_ui_only()?;
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
        #[cfg(target_os = "macos")]
        self.sync_native_video_surface_frame();
        if let Err(error) = self.refresh_overlay_ui_only() {
            error!(?error, "failed to refresh overlay during resize");
        }
        self.sync_ui_presenter();
    }

    fn log_runtime_events(&mut self) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        let events = runtime.drain_events();
        for event in events {
            if let PlayerRuntimeEvent::Initialized(startup) = &event {
                self.last_plugin_diagnostics_summary =
                    plugin_diagnostics_summary(&startup.plugin_diagnostics);
                self.overlay_dirty = true;
            }
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
            ui_presenter.sync(
                &snapshot,
                &overlay_view_model,
                window.inner_size(),
                window.scale_factor(),
                self.seek_preview,
                self.uses_external_video_surface,
            );
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

    fn dispatch_pending_post_launch_play_if_needed(&mut self) -> Result<bool> {
        if !self.pending_post_launch_play {
            return Ok(false);
        }
        if self.pending_post_launch_play_needs_paint {
            let now = Instant::now();
            let deadline = self
                .pending_post_launch_play_paint_deadline
                .get_or_insert(now + POST_LAUNCH_PLAY_PAINT_FALLBACK);
            if now < *deadline {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
                return Ok(false);
            }

            self.pending_post_launch_play_needs_paint = false;
            self.pending_post_launch_play_paint_deadline = None;
            info!(
                source_generation = self.source_generation,
                fallback_wait_ms = POST_LAUNCH_PLAY_PAINT_FALLBACK.as_millis(),
                "desktop deferred play paint barrier timed out"
            );
            return Ok(false);
        }

        self.pending_post_launch_play = false;
        self.dispatch_command(PlayerRuntimeCommand::Play)?;
        Ok(true)
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

#[cfg(target_os = "macos")]
fn open_basic_player_runtime_for_source(
    source: &str,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
) -> Result<(
    PlayerRuntimeBootstrap,
    player_runtime::PlayerRuntimeAdapterCapabilities,
)> {
    let bootstrap = open_macos_host_runtime_uri_with_options_and_interrupt(
        source.to_owned(),
        options,
        interrupt_flag,
    )?;
    let capabilities = bootstrap.runtime.capabilities();
    Ok((bootstrap, capabilities))
}

#[cfg(not(target_os = "macos"))]
fn open_basic_player_runtime_for_source(
    source: &str,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
) -> Result<(
    PlayerRuntimeBootstrap,
    player_runtime::PlayerRuntimeAdapterCapabilities,
)> {
    open_desktop_host_runtime_uri_with_options_and_interrupt(
        source.to_owned(),
        options,
        interrupt_flag,
    )
}

fn basic_player_runtime_options() -> PlayerRuntimeOptions {
    PlayerRuntimeOptions::default()
        .with_decoder_plugin_library_paths(decoder_plugin_library_paths_from_env())
        .with_decoder_plugin_video_mode(decoder_plugin_video_mode_from_env())
}

fn decoder_plugin_library_paths_from_env() -> Vec<PathBuf> {
    std::env::var_os(DECODER_PLUGIN_PATHS_ENV)
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default()
}

fn decoder_plugin_video_mode_from_env() -> PlayerDecoderPluginVideoMode {
    decoder_plugin_video_mode_from_value(std::env::var(DECODER_PLUGIN_VIDEO_MODE_ENV).ok())
}

fn decoder_plugin_video_mode_from_value(value: Option<String>) -> PlayerDecoderPluginVideoMode {
    match value {
        Some(value) if value.eq_ignore_ascii_case("native-frame") => {
            PlayerDecoderPluginVideoMode::PreferNativeFrame
        }
        _ => PlayerDecoderPluginVideoMode::DiagnosticsOnly,
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
            WindowEvent::ScaleFactorChanged { .. } => {
                self.sync_renderer_stage_viewport();
                #[cfg(target_os = "macos")]
                self.sync_native_video_surface_frame();
                self.sync_ui_presenter();
                if let Err(error) = self.refresh_overlay_ui_only() {
                    error!(
                        ?error,
                        "failed to refresh overlay after scale-factor change"
                    );
                    event_loop.exit();
                    return;
                }
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
                self.overlay_dirty = true;
            }
            WindowEvent::CursorLeft { .. } => {
                if self.seek_preview.is_some() {
                    return;
                }
                self.pointer_inside_window = false;
                self.schedule_controls_hide();
                self.overlay_dirty = true;
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
                Key::Named(NamedKey::Space) => self.perform_keyboard_control_action(
                    event_loop,
                    "toggle_pause",
                    ControlAction::TogglePause,
                ),
                Key::Named(NamedKey::ArrowLeft) => self.perform_keyboard_control_action(
                    event_loop,
                    "seek_back",
                    ControlAction::SeekBack,
                ),
                Key::Named(NamedKey::ArrowRight) => self.perform_keyboard_control_action(
                    event_loop,
                    "seek_forward",
                    ControlAction::SeekForward,
                ),
                Key::Named(NamedKey::Home) => self.perform_keyboard_control_action(
                    event_loop,
                    "seek_start",
                    ControlAction::SeekStart,
                ),
                Key::Named(NamedKey::End) => self.perform_keyboard_control_action(
                    event_loop,
                    "seek_end",
                    ControlAction::SeekEnd,
                ),
                Key::Character(text) if text.eq_ignore_ascii_case("s") => {
                    self.perform_keyboard_control_action(event_loop, "stop", ControlAction::Stop)
                }
                Key::Character("[") if self.runtime.is_some() => {
                    log_keyboard_action("rate_down");
                    if let Err(error) = self.step_playback_rate(-1) {
                        error!(?error, "failed to step playback rate backward");
                        event_loop.exit();
                    }
                }
                Key::Character("]") if self.runtime.is_some() => {
                    log_keyboard_action("rate_up");
                    if let Err(error) = self.step_playback_rate(1) {
                        error!(?error, "failed to step playback rate forward");
                        event_loop.exit();
                    }
                }
                Key::Character("0") => self.perform_keyboard_control_action(
                    event_loop,
                    "set_rate_0_5x",
                    ControlAction::SetRate(0.5),
                ),
                Key::Character("1") => self.perform_keyboard_control_action(
                    event_loop,
                    "set_rate_1x",
                    ControlAction::SetRate(1.0),
                ),
                Key::Character("2") => self.perform_keyboard_control_action(
                    event_loop,
                    "set_rate_2x",
                    ControlAction::SetRate(2.0),
                ),
                Key::Character("3") => self.perform_keyboard_control_action(
                    event_loop,
                    "set_rate_3x",
                    ControlAction::SetRate(3.0),
                ),
                Key::Character(text) if text.eq_ignore_ascii_case("h") => self
                    .perform_keyboard_control_action(
                        event_loop,
                        "open_hls_demo",
                        ControlAction::OpenHlsDemo,
                    ),
                Key::Character(text) if text.eq_ignore_ascii_case("o") => self
                    .perform_keyboard_control_action(
                        event_loop,
                        "open_local_file",
                        ControlAction::OpenLocalFile,
                    ),
                Key::Character(text) if text.eq_ignore_ascii_case("d") => self
                    .perform_keyboard_control_action(
                        event_loop,
                        "open_dash_demo",
                        ControlAction::OpenDashDemo,
                    ),
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
                    self.overlay_dirty = true;
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

        match self.activate_pending_launch_if_needed() {
            Ok(changed) => {
                if changed {
                    self.sync_ui_presenter();
                }
            }
            Err(error) => {
                error!(?error, "failed to activate deferred desktop media launch");
                event_loop.exit();
                return;
            }
        }

        match self.dispatch_pending_post_launch_play_if_needed() {
            Ok(changed) => {
                if changed {
                    self.sync_ui_presenter();
                }
            }
            Err(error) => {
                error!(?error, "failed to start deferred desktop playback");
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
                    self.overlay_dirty = true;
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
                    self.overlay_dirty = true;
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

        let frame_advanced = match self.advance_playback() {
            Ok(frame_advanced) => frame_advanced,
            Err(error) => {
                error!(?error, "failed to advance playback");
                event_loop.exit();
                return;
            }
        };

        if !frame_advanced && self.overlay_dirty {
            self.sync_ui_presenter();
            if let Err(error) = self.refresh_overlay_ui_only() {
                error!(?error, "failed to refresh dirty overlay");
                event_loop.exit();
                return;
            }
        }

        self.log_runtime_events();
        self.update_window_title();
        self.sync_ui_presenter();
        let controls_animation_deadline = self.controls_animation_deadline();
        if self.pending_launch_activation.is_some()
            || self.pending_post_launch_play
            || self.overlay_dirty
        {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if let Some(runtime) = self.runtime.as_ref() {
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

fn resolve_initial_media_source_uri() -> Result<Option<String>> {
    if let Some(source) = std::env::args().nth(1) {
        return resolve_media_source_argument(source).map(Some);
    }

    Ok(None)
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

fn plugin_diagnostics_summary(records: &[PlayerPluginDiagnostic]) -> Option<String> {
    if records.is_empty() {
        return None;
    }

    let total = records.len();
    let supported_decoders = records
        .iter()
        .filter(|record| record.status == PlayerPluginDiagnosticStatus::DecoderSupported)
        .map(|record| {
            let name = record.plugin_name.as_deref().unwrap_or("unknown-decoder");
            if record
                .decoder_capabilities
                .as_ref()
                .is_some_and(|capabilities| capabilities.supports_native_frame_output)
            {
                format!("{name} native-frame")
            } else {
                name.to_owned()
            }
        })
        .collect::<Vec<_>>();
    if !supported_decoders.is_empty() {
        return Some(format!(
            "decoder plugins: {}/{} supported ({}); diagnostic-only, playback still uses native-first/FFmpeg fallback",
            supported_decoders.len(),
            total,
            supported_decoders.join(", ")
        ));
    }

    let failed_count = records
        .iter()
        .filter(|record| record.status == PlayerPluginDiagnosticStatus::LoadFailed)
        .count();
    let loaded_count = total.saturating_sub(failed_count);
    let unsupported_codec_count = records
        .iter()
        .filter(|record| record.status == PlayerPluginDiagnosticStatus::DecoderUnsupported)
        .count();
    let unsupported_kind_count = records
        .iter()
        .filter(|record| record.status == PlayerPluginDiagnosticStatus::UnsupportedKind)
        .count();

    Some(format!(
        "decoder plugins: 0/{total} supported, {loaded_count} loaded, {failed_count} failed, {unsupported_codec_count} unsupported codec, {unsupported_kind_count} non-decoder"
    ))
}

fn log_runtime_event(event: PlayerRuntimeEvent) {
    match event {
        PlayerRuntimeEvent::Initialized(startup) => {
            info!(
                ffmpeg_initialized = startup.ffmpeg_initialized,
                audio_output = ?startup.audio_output,
                decoded_audio = startup.decoded_audio.as_ref().map(audio_summary),
                video_decode = startup.video_decode.as_ref().map(video_decode_summary),
                plugin_diagnostics = plugin_diagnostics_summary(&startup.plugin_diagnostics).as_deref(),
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
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        ControlAction, DASH_DEMO_CLI_FLAG, DESKTOP_DASH_DEMO_URL, DESKTOP_HLS_DEMO_URL,
        DesktopPlayerApp, HLS_DEMO_CLI_FLAG, SourceLaunchStatus,
        decoder_plugin_video_mode_from_value, resolve_media_source_argument,
    };
    use player_render_wgpu::RenderFrameOutcome;
    use player_runtime::PlayerDecoderPluginVideoMode;
    use std::time::{Duration, Instant};

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

    #[test]
    fn decoder_plugin_video_mode_defaults_to_diagnostics_only() {
        assert_eq!(
            decoder_plugin_video_mode_from_value(None),
            PlayerDecoderPluginVideoMode::DiagnosticsOnly
        );
        assert_eq!(
            decoder_plugin_video_mode_from_value(Some("diagnostics".to_owned())),
            PlayerDecoderPluginVideoMode::DiagnosticsOnly
        );
    }

    #[test]
    fn decoder_plugin_video_mode_native_frame_opt_in_is_case_insensitive() {
        assert_eq!(
            decoder_plugin_video_mode_from_value(Some("native-frame".to_owned())),
            PlayerDecoderPluginVideoMode::PreferNativeFrame
        );
        assert_eq!(
            decoder_plugin_video_mode_from_value(Some("NATIVE-FRAME".to_owned())),
            PlayerDecoderPluginVideoMode::PreferNativeFrame
        );
    }

    #[test]
    fn playlist_row_reflects_loading_and_failed_launch_states() {
        let mut app = DesktopPlayerApp::new(Some("file:///tmp/local.mp4".to_owned()));
        app.register_playlist_source(DESKTOP_HLS_DEMO_URL, Some("HLS DEMO".to_owned()));

        app.active_launch_request_id = Some(7);
        app.launch_status = Some(SourceLaunchStatus::Loading);
        let loading_items = app.playlist_item_view_data();
        assert_eq!(loading_items[app.active_playlist_index].status, "LOADING");
        assert!(loading_items[app.active_playlist_index].is_active);

        app.active_launch_request_id = None;
        app.launch_status = Some(SourceLaunchStatus::Failed);
        let failed_items = app.playlist_item_view_data();
        assert_eq!(failed_items[app.active_playlist_index].status, "FAILED");
        assert!(failed_items[app.active_playlist_index].is_active);
    }

    #[test]
    fn host_overlay_only_exposes_runtime_free_actions_while_loading() {
        let app = DesktopPlayerApp::new(Some("file:///tmp/local.mp4".to_owned()));

        assert!(app.is_control_action_available(ControlAction::OpenLocalFile));
        assert!(app.is_control_action_available(ControlAction::OpenHlsDemo));
        assert!(app.is_control_action_available(ControlAction::FocusPlaylistItem(0)));
        assert!(!app.is_control_action_available(ControlAction::TogglePause));
        assert!(!app.is_control_action_available(ControlAction::SeekBack));
        assert!(!app.is_control_action_available(ControlAction::SetRate(1.5)));
    }

    #[test]
    fn empty_start_has_no_default_media_or_playlist_entry() {
        let app = DesktopPlayerApp::new(None);

        assert!(app.source.is_none());
        assert!(app.playlist_item_view_data().is_empty());
        assert_eq!(app.current_source_label(), "Drop a video to start");
        assert!(app.is_control_action_available(ControlAction::OpenLocalFile));
        assert!(app.is_control_action_available(ControlAction::OpenHlsDemo));
        assert!(!app.is_control_action_available(ControlAction::CreateDownloadCurrentSource));
        assert!(!app.is_control_action_available(ControlAction::TogglePause));
    }

    #[test]
    fn replacing_launch_cancel_flag_interrupts_previous_prepare() {
        let mut app = DesktopPlayerApp::new(Some("file:///tmp/local.mp4".to_owned()));

        let first = app.replace_active_launch_cancel_flag();
        assert!(!first.load(std::sync::atomic::Ordering::SeqCst));

        let second = app.replace_active_launch_cancel_flag();
        assert!(first.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!second.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn deferred_post_launch_play_waits_for_present_or_short_timeout() {
        let mut app = DesktopPlayerApp::new(Some("file:///tmp/local.mp4".to_owned()));
        app.pending_post_launch_play = true;
        app.pending_post_launch_play_needs_paint = true;
        app.pending_post_launch_play_paint_deadline = Some(Instant::now() + Duration::from_secs(1));

        assert!(!app.dispatch_pending_post_launch_play_if_needed().unwrap());
        assert!(app.pending_post_launch_play);
        assert!(app.pending_post_launch_play_needs_paint);

        app.observe_render_outcome(RenderFrameOutcome::Presented);
        assert!(!app.pending_post_launch_play_needs_paint);
        assert!(app.pending_post_launch_play_paint_deadline.is_none());

        app.pending_post_launch_play_needs_paint = true;
        app.pending_post_launch_play_paint_deadline =
            Some(Instant::now() - Duration::from_millis(1));
        assert!(!app.dispatch_pending_post_launch_play_if_needed().unwrap());
        assert!(!app.pending_post_launch_play_needs_paint);
        assert!(app.pending_post_launch_play);
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
        player_model::MediaSourceProtocol::Hls => "HLS",
        player_model::MediaSourceProtocol::Dash => "DASH",
        player_model::MediaSourceProtocol::Progressive => "FILE",
        player_model::MediaSourceProtocol::File => "LOCAL",
        player_model::MediaSourceProtocol::Content => "CONTENT",
        player_model::MediaSourceProtocol::Unknown => "SOURCE",
    };
    let resolution = snapshot
        .media_info
        .best_video
        .as_ref()
        .map(|video| format!("{}X{}", video.width, video.height))
        .unwrap_or_else(|| "UNKNOWN".to_owned());
    format!("{protocol} {resolution}")
}
