use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
mod host_ui;
#[cfg(target_os = "macos")]
mod macos_host_overlay;
use host_ui::{CONTROL_RATES, ControlAction};
#[cfg(not(target_os = "macos"))]
use host_ui::{
    SeekPreview, control_action_at, render_control_overlay, seek_preview_at, seek_preview_for_drag,
};
use player_host_desktop::{
    DesktopHostLaunchPlan as RuntimeLaunchPlan, canonical_desktop_host_local_path,
    normalize_desktop_host_source_uri,
    open_desktop_host_runtime_uri_for_winit_window, probe_desktop_host_launch_plan_uri,
};
use player_render_wgpu::{
    RenderMode, RenderSurfaceConfig, RgbaVideoFrame, VideoFrameTexture, VideoRenderer,
    Yuv420pVideoFrame, default_window_attributes, preferred_backends,
};
use player_runtime::{
    DecodedAudioSummary, DecodedVideoFrame, PlaybackProgress, PlayerRuntime,
    PlayerRuntimeBootstrap, PlayerRuntimeCommand, PlayerRuntimeEvent, PlayerVideoDecodeInfo,
    PlayerVideoDecodeMode, PresentationState, VideoPixelFormat,
};
use tracing::{error, info};
#[cfg(target_os = "macos")]
use tracing::warn;
use tracing_subscriber::EnvFilter;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

#[cfg(target_os = "macos")]
use macos_host_overlay::MacosHostOverlay;

const SEEK_STEP: Duration = Duration::from_secs(5);
const NATIVE_SURFACE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const CONTROL_HIDE_DELAY: Duration = Duration::from_secs(2);
const HLS_DEMO_CLI_FLAG: &str = "--hls-demo";
const DASH_DEMO_CLI_FLAG: &str = "--dash-demo";
const DESKTOP_HLS_DEMO_URL: &str =
    "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8";
const DESKTOP_DASH_DEMO_URL: &str =
    "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd";

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("info"))
        .with_target(false)
        .compact()
        .init();

    let source = resolve_media_source_uri()?;
    let launch_plan = build_launch_plan(source)?;

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = DesktopPlayerApp::new(launch_plan);
    event_loop.run_app(&mut app)?;
    Ok(())
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
    #[cfg(not(target_os = "macos"))]
    seek_preview: Option<SeekPreview>,
    #[cfg(target_os = "macos")]
    host_overlay: Option<MacosHostOverlay>,
}

impl DesktopPlayerApp {
    fn new(launch_plan: RuntimeLaunchPlan) -> Self {
        Self {
            source: launch_plan.source,
            runtime: None,
            last_frame: None,
            render_config: launch_plan.render_config,
            uses_external_video_surface: false,
            window: None,
            renderer: None,
            title_cache: None,
            cursor_position: None,
            pointer_inside_window: true,
            controls_visible: true,
            controls_hide_deadline: None,
            #[cfg(not(target_os = "macos"))]
            seek_preview: None,
            #[cfg(target_os = "macos")]
            host_overlay: None,
        }
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.window.is_some() {
            return Ok(());
        }

        let window = Arc::new(event_loop.create_window(
            default_window_attributes(self.render_config).with_title(self.window_title()),
        )?);
        #[cfg(target_os = "macos")]
        match MacosHostOverlay::attach(window.as_ref()) {
            Ok(overlay) => {
                self.host_overlay = Some(overlay);
            }
            Err(error) => {
                warn!(
                    ?error,
                    "failed to attach macOS host overlay; keyboard controls remain available"
                );
            }
        }
        self.window = Some(window.clone());
        let launch_plan = RuntimeLaunchPlan {
            source: self.source.clone(),
            render_config: self.render_config,
        };
        self.activate_launch_plan(launch_plan, window)?;

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
        ) = open_desktop_host_runtime_uri_for_winit_window(&launch_plan.source, window.as_ref())?;

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

        if capabilities.supports_frame_output {
            let initial_frame =
                initial_frame.context("desktop runtime did not provide an initial frame")?;
            let mut renderer = pollster::block_on(VideoRenderer::new(
                window.clone(),
                (initial_frame.width, initial_frame.height),
            ))?;
            renderer.set_render_mode(RenderMode::Fit);
            self.renderer = Some(renderer);
            self.last_frame = None;
            self.apply_frame(initial_frame)?;
        } else {
            self.renderer = None;
            self.last_frame = None;
        }

        self.title_cache = None;
        self.show_controls();
        self.update_window_title();
        self.sync_host_overlay();
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
        let window = self
            .window
            .as_ref()
            .cloned()
            .context("window missing while playback is active")?;
        let Some(frame) = self.last_frame.as_ref() else {
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.clear_overlay();
            }
            return Ok(());
        };
        let frame_texture = video_frame_texture(frame);
        let window_size = window.inner_size();
        #[cfg(not(target_os = "macos"))]
        let overlay = if window_size.width == 0 || window_size.height == 0 || !self.controls_visible {
            None
        } else {
            render_control_overlay(
                window_size.width,
                window_size.height,
                &self.runtime()?.snapshot(),
                self.seek_preview,
            )
        };
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(());
        };
        if window_size.width == 0 || window_size.height == 0 {
            renderer.clear_overlay();
            return Ok(());
        }

        renderer.upload_frame(&frame_texture);

        #[cfg(target_os = "macos")]
        {
            renderer.clear_overlay();
        }

        #[cfg(not(target_os = "macos"))]
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
        self.sync_host_overlay();
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

    #[cfg(not(target_os = "macos"))]
    fn begin_seek_drag(&mut self) -> Result<bool> {
        let Some((cursor_x, cursor_y)) = self.cursor_position else {
            return Ok(false);
        };
        let Some(window) = self.window.as_ref() else {
            return Ok(false);
        };
        let snapshot = self.runtime()?.snapshot();
        let window_size = window.inner_size();
        let Some(preview) = seek_preview_at(
            window_size.width,
            window_size.height,
            cursor_x,
            cursor_y,
            &snapshot,
        ) else {
            return Ok(false);
        };

        self.seek_preview = Some(preview);
        self.show_controls();
        self.refresh_overlay()?;
        Ok(true)
    }

    #[cfg(not(target_os = "macos"))]
    fn update_seek_drag(&mut self) -> Result<()> {
        if self.seek_preview.is_none() {
            return Ok(());
        }

        let Some((cursor_x, _)) = self.cursor_position else {
            return Ok(());
        };
        let Some(window) = self.window.as_ref() else {
            return Ok(());
        };
        let snapshot = self.runtime()?.snapshot();
        let window_size = window.inner_size();
        if let Some(preview) =
            seek_preview_for_drag(window_size.width, window_size.height, cursor_x, &snapshot)
        {
            self.seek_preview = Some(preview);
            self.refresh_overlay()?;
        }

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn commit_seek_drag(&mut self) -> Result<bool> {
        let Some(preview) = self.seek_preview.take() else {
            return Ok(false);
        };
        self.seek_to(preview.position)?;
        Ok(true)
    }

    fn toggle_pause(&mut self) {
        if let Err(error) = self.dispatch_command(PlayerRuntimeCommand::TogglePause) {
            error!(?error, "failed to toggle pause state");
        }
    }

    fn open_media_source(&mut self, source: String) -> Result<()> {
        let launch_plan = build_launch_plan(source)?;
        let window = self
            .window
            .as_ref()
            .cloned()
            .context("window missing while opening a new media source")?;
        self.activate_launch_plan(launch_plan, window)
    }

    fn open_dropped_file(&mut self, path: PathBuf) -> Result<()> {
        let source = canonical_desktop_host_local_path(&path)?;
        info!(source = source.as_str(), "opening dropped media source");
        self.open_media_source(source)
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
        self.sync_host_overlay();
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
        #[cfg(target_os = "macos")]
        {
            return Ok(());
        }

        #[cfg(not(target_os = "macos"))]
        {
            if self.renderer.is_none() {
                return Ok(());
            }
            let Some((cursor_x, cursor_y)) = self.cursor_position else {
                return Ok(());
            };
            let Some(window) = self.window.as_ref() else {
                return Ok(());
            };

            let window_size = window.inner_size();
            if window_size.width == 0 || window_size.height == 0 {
                return Ok(());
            }

            if let Some(action) = control_action_at(
                window_size.width,
                window_size.height,
                cursor_x,
                cursor_y,
                &self.runtime()?.snapshot(),
            ) {
                self.perform_control_action(action)?;
            }

            Ok(())
        }
    }

    fn perform_control_action(&mut self, action: ControlAction) -> Result<()> {
        match action {
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
        }
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.resize(size);
        }
        if let Err(error) = self.refresh_overlay() {
            error!(?error, "failed to refresh overlay during resize");
        }
        self.sync_host_overlay();
    }

    fn log_runtime_events(&mut self) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        for event in runtime.drain_events() {
            log_runtime_event(event);
        }
    }

    fn sync_host_overlay(&self) {
        #[cfg(target_os = "macos")]
        if let (Some(runtime), Some(host_overlay)) =
            (self.runtime.as_ref(), self.host_overlay.as_ref())
        {
            host_overlay.update(&runtime.snapshot(), self.controls_visible);
        }
    }

    fn drain_host_overlay_actions(&mut self) -> Result<()> {
        #[cfg(target_os = "macos")]
        if let Some(host_overlay) = self.host_overlay.as_ref() {
            for action in host_overlay.drain_actions() {
                self.perform_control_action(action)?;
            }
        }

        Ok(())
    }

    fn show_controls(&mut self) {
        self.controls_visible = true;
        self.controls_hide_deadline = None;
    }

    fn schedule_controls_hide(&mut self) {
        self.controls_hide_deadline = Some(Instant::now() + CONTROL_HIDE_DELAY);
    }

    fn update_controls_visibility(&mut self) -> Result<()> {
        if self.pointer_inside_window {
            if !self.controls_visible {
                self.controls_visible = true;
                self.sync_host_overlay();
                self.refresh_overlay()?;
            }
            self.controls_hide_deadline = None;
            return Ok(());
        }

        if let Some(deadline) = self.controls_hide_deadline {
            if Instant::now() >= deadline && self.controls_visible {
                self.controls_visible = false;
                self.sync_host_overlay();
                self.refresh_overlay()?;
            }
        }

        Ok(())
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
                self.sync_host_overlay();
                #[cfg(not(target_os = "macos"))]
                if let Err(error) = self.update_seek_drag() {
                    error!(?error, "failed to update seek drag preview");
                    event_loop.exit();
                    return;
                }
                if let Err(error) = self.refresh_overlay() {
                    error!(?error, "failed to refresh overlay after cursor movement");
                    event_loop.exit();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                #[cfg(not(target_os = "macos"))]
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
            } =>
            {
                #[cfg(not(target_os = "macos"))]
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
                #[cfg(not(target_os = "macos"))]
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
                Key::Named(NamedKey::Space) => self.toggle_pause(),
                Key::Named(NamedKey::ArrowLeft) => {
                    if let Err(error) = self.seek_by(SEEK_STEP, false) {
                        error!(?error, "failed to seek backward");
                        event_loop.exit();
                    }
                }
                Key::Named(NamedKey::ArrowRight) => {
                    if let Err(error) = self.seek_by(SEEK_STEP, true) {
                        error!(?error, "failed to seek forward");
                        event_loop.exit();
                    }
                }
                Key::Named(NamedKey::Home) => {
                    if let Err(error) = self.seek_to(Duration::ZERO) {
                        error!(?error, "failed to seek to start");
                        event_loop.exit();
                    }
                }
                Key::Named(NamedKey::End) => match self.runtime() {
                    Ok(runtime) => {
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
                    if let Err(error) = self.dispatch_command(PlayerRuntimeCommand::Stop) {
                        error!(?error, "failed to stop playback");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text == "[" => {
                    if let Err(error) = self.step_playback_rate(-1) {
                        error!(?error, "failed to step playback rate backward");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text == "]" => {
                    if let Err(error) = self.step_playback_rate(1) {
                        error!(?error, "failed to step playback rate forward");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text == "0" => {
                    if let Err(error) = self.set_playback_rate(0.5) {
                        error!(?error, "failed to set playback rate");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text == "1" => {
                    if let Err(error) = self.set_playback_rate(1.0) {
                        error!(?error, "failed to set playback rate");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text == "2" => {
                    if let Err(error) = self.set_playback_rate(2.0) {
                        error!(?error, "failed to set playback rate");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text == "3" => {
                    if let Err(error) = self.set_playback_rate(3.0) {
                        error!(?error, "failed to set playback rate");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text.eq_ignore_ascii_case("h") => {
                    if let Err(error) = self.open_media_source(DESKTOP_HLS_DEMO_URL.to_owned()) {
                        error!(?error, "failed to open desktop HLS demo source");
                        event_loop.exit();
                    }
                }
                Key::Character(text) if text.eq_ignore_ascii_case("d") => {
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
        if let Err(error) = self.update_controls_visibility() {
            error!(?error, "failed to update control visibility");
            event_loop.exit();
            return;
        }

        if let Err(error) = self.drain_host_overlay_actions() {
            error!(?error, "failed to handle host overlay action");
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
        self.sync_host_overlay();
        if let Some(runtime) = self.runtime.as_ref() {
            if let Some(deadline) = runtime.next_deadline() {
                let mut next_deadline = deadline;
                if let Some(hide_deadline) = self.controls_hide_deadline {
                    next_deadline = next_deadline.min(hide_deadline);
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(next_deadline));
            } else if self.uses_external_video_surface {
                let mut next_deadline = Instant::now() + NATIVE_SURFACE_POLL_INTERVAL;
                if let Some(hide_deadline) = self.controls_hide_deadline {
                    next_deadline = next_deadline.min(hide_deadline);
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(next_deadline));
            } else if let Some(hide_deadline) = self.controls_hide_deadline {
                event_loop.set_control_flow(ControlFlow::WaitUntil(hide_deadline));
            } else {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
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
