use player_core::MediaSource;
use player_platform_desktop::{
    merge_runtime_fallback_reason, probe_platform_desktop_source_with_options,
    runtime_fallback_events,
};
use player_runtime::{
    PlayerMediaInfo, PlayerRuntime, PlayerRuntimeAdapter, PlayerRuntimeAdapterBootstrap,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer,
    PlayerRuntimeBootstrap, PlayerRuntimeError,
    PlayerRuntimeErrorCode, PlayerRuntimeInitializer, PlayerRuntimeOptions, PlayerRuntimeResult,
    PlayerRuntimeStartup, PlayerRuntimeEvent, PlayerVideoDecodeInfo, PlayerVideoDecodeMode,
    register_default_runtime_adapter_factory,
};
use std::collections::VecDeque;

pub const WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID: &str = "windows_software_desktop";
pub const WINDOWS_NATIVE_FRAME_PLAYER_RUNTIME_ADAPTER_ID: &str = "windows_native_frame_desktop";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsNativeFrameBackendKind {
    D3D11,
    Dxva,
}

#[derive(Debug, Clone)]
pub struct WindowsNativeFrameRoadmap {
    pub adapter_id: &'static str,
    pub preferred_backend: WindowsNativeFrameBackendKind,
    pub accepted_handle_kinds: &'static [&'static str],
}

pub fn windows_native_frame_roadmap() -> WindowsNativeFrameRoadmap {
    WindowsNativeFrameRoadmap {
        adapter_id: WINDOWS_NATIVE_FRAME_PLAYER_RUNTIME_ADAPTER_ID,
        preferred_backend: WindowsNativeFrameBackendKind::D3D11,
        accepted_handle_kinds: &["D3D11Texture2D", "DxgiSurface"],
    }
}

#[derive(Debug, Clone)]
struct WindowsRuntimeDiagnostics {
    video_decode: PlayerVideoDecodeInfo,
}

struct WindowsRuntimeAdapterInitializer {
    inner: Box<dyn PlayerRuntimeAdapterInitializer>,
    diagnostics: WindowsRuntimeDiagnostics,
}

struct WindowsRuntimeAdapter {
    inner: Box<dyn PlayerRuntimeAdapter>,
    video_decode: PlayerVideoDecodeInfo,
    pending_runtime_fallback_events: VecDeque<PlayerRuntimeEvent>,
}

#[derive(Debug, Clone)]
pub struct WindowsHostRuntimeProbe {
    pub adapter_id: &'static str,
    pub capabilities: PlayerRuntimeAdapterCapabilities,
    pub media_info: PlayerMediaInfo,
    pub startup: PlayerRuntimeStartup,
}

pub fn windows_runtime_adapter_factory() -> &'static dyn PlayerRuntimeAdapterFactory {
    static FACTORY: WindowsSoftwarePlayerRuntimeAdapterFactory =
        WindowsSoftwarePlayerRuntimeAdapterFactory;
    &FACTORY
}

pub fn install_default_windows_runtime_adapter_factory() -> PlayerRuntimeResult<()> {
    register_default_runtime_adapter_factory(windows_runtime_adapter_factory())
}

pub fn open_windows_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
    open_windows_host_runtime_source_with_options(MediaSource::new(uri), options)
}

pub fn probe_windows_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<WindowsHostRuntimeProbe> {
    probe_windows_host_runtime_source_with_options(MediaSource::new(uri), options)
}

pub fn probe_windows_host_runtime_source_with_options(
    source: MediaSource,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<WindowsHostRuntimeProbe> {
    if !cfg!(target_os = "windows") {
        return Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "windows host runtime strategy can only be probed on Windows targets",
        ));
    }

    let initializer = PlayerRuntimeInitializer::probe_source_with_factory(
        source,
        options,
        windows_runtime_adapter_factory(),
    )?;

    Ok(WindowsHostRuntimeProbe {
        adapter_id: WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        capabilities: initializer.capabilities(),
        media_info: initializer.media_info(),
        startup: initializer.startup(),
    })
}

pub fn open_windows_host_runtime_source_with_options(
    source: MediaSource,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
    if !cfg!(target_os = "windows") {
        return Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "windows host runtime strategy can only be initialized on Windows targets",
        ));
    }

    PlayerRuntime::open_source_with_factory(source, options, windows_runtime_adapter_factory())
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsSoftwarePlayerRuntimeAdapterFactory;

impl PlayerRuntimeAdapterFactory for WindowsSoftwarePlayerRuntimeAdapterFactory {
    fn adapter_id(&self) -> &'static str {
        WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
    }

    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        if !cfg!(target_os = "windows") {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "windows desktop adapter can only be initialized on Windows targets",
            ));
        }

        let inner = probe_platform_desktop_source_with_options(
            WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            source,
            options,
        )?;
        let diagnostics = windows_runtime_diagnostics(&inner.media_info());
        Ok(Box::new(WindowsRuntimeAdapterInitializer { inner, diagnostics }))
    }
}

impl PlayerRuntimeAdapterInitializer for WindowsRuntimeAdapterInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.inner.capabilities()
    }

    fn media_info(&self) -> PlayerMediaInfo {
        self.inner.media_info()
    }

    fn startup(&self) -> PlayerRuntimeStartup {
        apply_windows_runtime_diagnostics(self.inner.startup(), &self.diagnostics)
    }

    fn initialize(self: Box<Self>) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
        let Self { inner, diagnostics } = *self;
        let bootstrap = inner.initialize()?;
        Ok(wrap_windows_runtime_bootstrap(bootstrap, diagnostics))
    }
}

impl PlayerRuntimeAdapter for WindowsRuntimeAdapter {
    fn source_uri(&self) -> &str {
        self.inner.source_uri()
    }

    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.inner.capabilities()
    }

    fn media_info(&self) -> &PlayerMediaInfo {
        self.inner.media_info()
    }

    fn presentation_state(&self) -> player_runtime::PresentationState {
        self.inner.presentation_state()
    }

    fn has_video_surface(&self) -> bool {
        self.inner.has_video_surface()
    }

    fn is_interrupted(&self) -> bool {
        self.inner.is_interrupted()
    }

    fn is_buffering(&self) -> bool {
        self.inner.is_buffering()
    }

    fn playback_rate(&self) -> f32 {
        self.inner.playback_rate()
    }

    fn progress(&self) -> player_runtime::PlaybackProgress {
        self.inner.progress()
    }

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        let mut events = self
            .inner
            .drain_events()
            .into_iter()
            .map(|event| match event {
                PlayerRuntimeEvent::Initialized(startup) => {
                    PlayerRuntimeEvent::Initialized(apply_video_decode_diagnostics(
                        startup,
                        &self.video_decode,
                    ))
                }
                other => other,
            })
            .collect::<Vec<_>>();
        while let Some(event) = self.pending_runtime_fallback_events.pop_back() {
            events.insert(0, event);
        }
        events
    }

    fn dispatch(
        &mut self,
        command: player_runtime::PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<player_runtime::PlayerRuntimeCommandResult> {
        self.inner.dispatch(command)
    }

    fn advance(&mut self) -> PlayerRuntimeResult<Option<player_runtime::DecodedVideoFrame>> {
        self.inner.advance()
    }

    fn next_deadline(&self) -> Option<std::time::Instant> {
        self.inner.next_deadline()
    }
}

fn windows_runtime_diagnostics(media_info: &PlayerMediaInfo) -> WindowsRuntimeDiagnostics {
    let roadmap = windows_native_frame_roadmap();
    let fallback_reason = media_info.best_video.as_ref().map(|video| {
        merge_runtime_fallback_reason(
            "windows native-frame roadmap is not implemented yet; selected software desktop path",
            &format!(
                "{} target prefers {:?} with handles {}",
                video.codec,
                roadmap.preferred_backend,
                roadmap.accepted_handle_kinds.join(", ")
            ),
            None,
        )
    });
    WindowsRuntimeDiagnostics {
        video_decode: PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Software,
            hardware_available: media_info.best_video.is_some(),
            hardware_backend: Some(format!("{:?}", roadmap.preferred_backend)),
            fallback_reason,
        },
    }
}

fn apply_windows_runtime_diagnostics(
    mut startup: PlayerRuntimeStartup,
    diagnostics: &WindowsRuntimeDiagnostics,
) -> PlayerRuntimeStartup {
    startup.video_decode = Some(match startup.video_decode.take() {
        Some(existing) if existing.fallback_reason.is_some() => existing,
        Some(mut existing) => {
            existing.hardware_backend = diagnostics.video_decode.hardware_backend.clone();
            existing.hardware_available = diagnostics.video_decode.hardware_available;
            existing.fallback_reason = diagnostics.video_decode.fallback_reason.clone();
            existing
        }
        None => diagnostics.video_decode.clone(),
    });
    startup
}

fn apply_video_decode_diagnostics(
    mut startup: PlayerRuntimeStartup,
    video_decode: &PlayerVideoDecodeInfo,
) -> PlayerRuntimeStartup {
    if startup.video_decode.is_none() {
        startup.video_decode = Some(video_decode.clone());
    }
    startup
}

fn wrap_windows_runtime_bootstrap(
    bootstrap: PlayerRuntimeAdapterBootstrap,
    diagnostics: WindowsRuntimeDiagnostics,
) -> PlayerRuntimeAdapterBootstrap {
    let PlayerRuntimeAdapterBootstrap {
        runtime,
        initial_frame,
        startup,
    } = bootstrap;
    PlayerRuntimeAdapterBootstrap {
        runtime: Box::new(WindowsRuntimeAdapter {
            inner: runtime,
            video_decode: diagnostics.video_decode.clone(),
            pending_runtime_fallback_events: runtime_fallback_events("windows native-frame roadmap placeholder"),
        }),
        initial_frame,
        startup: apply_windows_runtime_diagnostics(startup, &diagnostics),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        WINDOWS_NATIVE_FRAME_PLAYER_RUNTIME_ADAPTER_ID, WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        WindowsSoftwarePlayerRuntimeAdapterFactory, windows_native_frame_roadmap,
        windows_runtime_diagnostics,
        open_windows_host_runtime_source_with_options,
        probe_windows_host_runtime_source_with_options,
    };
    use player_core::MediaSource;
    use player_runtime::{
        PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterFactory, PlayerRuntimeErrorCode,
        PlayerRuntimeOptions, PlayerVideoDecodeMode,
    };

    #[test]
    fn windows_factory_matches_host_support() {
        let factory = WindowsSoftwarePlayerRuntimeAdapterFactory;

        if cfg!(target_os = "windows") {
            let Some(test_video_path) = test_video_path() else {
                eprintln!("skipping Windows fixture-backed test: test-video.mp4 is unavailable");
                return;
            };
            let result = factory.probe_source_with_options(
                MediaSource::new(test_video_path),
                PlayerRuntimeOptions::default(),
            );
            let initializer =
                result.expect("windows host should support the windows desktop adapter");
            let capabilities = initializer.capabilities();
            assert_eq!(
                capabilities.adapter_id,
                WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
            );
            assert_eq!(
                capabilities.backend_family,
                PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
            );
        } else {
            let result = factory.probe_source_with_options(
                MediaSource::new("fixture.mp4"),
                PlayerRuntimeOptions::default(),
            );
            let error = match result {
                Ok(_) => panic!("non-windows hosts should reject the windows adapter"),
                Err(error) => error,
            };
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    #[test]
    fn windows_host_probe_matches_factory_support() {
        if cfg!(target_os = "windows") {
            let Some(test_video_path) = test_video_path() else {
                eprintln!("skipping Windows fixture-backed test: test-video.mp4 is unavailable");
                return;
            };
            let result = probe_windows_host_runtime_source_with_options(
                MediaSource::new(test_video_path),
                PlayerRuntimeOptions::default(),
            );
            let probe = result.expect("windows host should support the windows host runtime probe");
            assert_eq!(probe.adapter_id, WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID);
            assert_eq!(
                probe.capabilities.backend_family,
                PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
            );
        } else {
            let result = probe_windows_host_runtime_source_with_options(
                MediaSource::new("fixture.mp4"),
                PlayerRuntimeOptions::default(),
            );
            let error = result.expect_err("non-windows hosts should reject the windows host probe");
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    #[test]
    fn windows_host_open_matches_factory_support() {
        if cfg!(target_os = "windows") {
            let Some(test_video_path) = test_video_path() else {
                eprintln!("skipping Windows fixture-backed test: test-video.mp4 is unavailable");
                return;
            };
            let result = open_windows_host_runtime_source_with_options(
                MediaSource::new(test_video_path),
                PlayerRuntimeOptions::default(),
            );
            let bootstrap =
                result.expect("windows host should support the windows host runtime open helper");
            assert_eq!(
                bootstrap.runtime.adapter_id(),
                WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
            );
        } else {
            let result = open_windows_host_runtime_source_with_options(
                MediaSource::new("fixture.mp4"),
                PlayerRuntimeOptions::default(),
            );
            let error = match result {
                Ok(_) => panic!("non-windows hosts should reject the windows host opener"),
                Err(error) => error,
            };
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    #[test]
    fn windows_native_frame_roadmap_prefers_d3d11_handles() {
        let roadmap = windows_native_frame_roadmap();

        assert_eq!(roadmap.adapter_id, WINDOWS_NATIVE_FRAME_PLAYER_RUNTIME_ADAPTER_ID);
        assert_eq!(format!("{:?}", roadmap.preferred_backend), "D3D11");
        assert_eq!(roadmap.accepted_handle_kinds, ["D3D11Texture2D", "DxgiSurface"]);
    }

    #[test]
    fn windows_runtime_diagnostics_stay_software_while_advertising_roadmap() {
        let diagnostics = windows_runtime_diagnostics(&player_runtime::PlayerMediaInfo {
            source_uri: "fixture.mp4".to_owned(),
            source_kind: player_runtime::MediaSourceKind::Local,
            source_protocol: player_runtime::MediaSourceProtocol::File,
            duration: None,
            bit_rate: None,
            audio_streams: 0,
            video_streams: 1,
            best_video: Some(player_runtime::PlayerVideoInfo {
                codec: "H264".to_owned(),
                width: 1920,
                height: 1080,
                frame_rate: Some(60.0),
            }),
            best_audio: None,
            track_catalog: Default::default(),
            track_selection: Default::default(),
        });

        assert_eq!(diagnostics.video_decode.selected_mode, PlayerVideoDecodeMode::Software);
        assert_eq!(diagnostics.video_decode.hardware_backend.as_deref(), Some("D3D11"));
        let fallback = diagnostics.video_decode.fallback_reason.as_deref().unwrap_or_default();
        assert!(fallback.contains("windows native-frame roadmap is not implemented yet"));
        assert!(fallback.contains("D3D11Texture2D"));
        assert!(fallback.contains("DxgiSurface"));
    }

    fn test_video_path() -> Option<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../test-video.mp4");
        path.canonicalize()
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
    }
}
