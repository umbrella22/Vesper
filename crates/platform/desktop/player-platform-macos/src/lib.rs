use std::collections::VecDeque;
mod native;
mod system;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use player_backend_ffmpeg::{
    CompressedVideoPacket, FfmpegBackend, VideoDecodeInfo as BackendVideoDecodeInfo,
    VideoDecoderMode as BackendVideoDecoderMode, VideoPacketSource, VideoPacketStreamInfo,
};
use player_core::{MediaSource, MediaSourceProtocol};
use player_platform_apple::{VIDEOTOOLBOX_BACKEND_NAME, probe_videotoolbox_hardware_decode};
use player_platform_desktop::{
    DesktopVideoFrame, DesktopVideoFramePoll, DesktopVideoFramePresentation, DesktopVideoSource,
    DesktopVideoSourceBootstrap, DesktopVideoSourceFactory, merge_runtime_fallback_reason,
    open_platform_desktop_source_with_options_and_interrupt,
    open_platform_desktop_source_with_video_source_factory_and_options_and_interrupt,
    probe_platform_desktop_source_with_options,
    probe_platform_desktop_source_with_video_source_factory_and_options, runtime_fallback_events,
};
use player_plugin::{
    DecoderMediaKind, DecoderNativeFrame, DecoderNativeHandleKind, DecoderPacket,
    DecoderReceiveNativeFrameOutput, DecoderSessionConfig, NativeDecoderSession, VesperPluginKind,
};
use player_plugin_loader::{
    DecoderPluginCapabilitySummary, DecoderPluginCodecSummary, DecoderPluginMatchRequest,
    LoadedDynamicPlugin, PluginDiagnosticRecord, PluginDiagnosticStatus, PluginRegistry,
};
use player_runtime::{
    DecodedVideoFrame, PlaybackProgress, PlayerDecoderPluginVideoMode, PlayerMediaInfo,
    PlayerPluginCodecCapability, PlayerPluginDecoderCapabilitySummary, PlayerPluginDiagnostic,
    PlayerPluginDiagnosticStatus, PlayerRuntime, PlayerRuntimeAdapter,
    PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer,
    PlayerRuntimeBootstrap, PlayerRuntimeCommand, PlayerRuntimeCommandResult, PlayerRuntimeError,
    PlayerRuntimeErrorCode, PlayerRuntimeEvent, PlayerRuntimeInitializer, PlayerRuntimeOptions,
    PlayerRuntimeResult, PlayerRuntimeStartup, PlayerVideoDecodeInfo, PlayerVideoDecodeMode,
    PlayerVideoSurfaceTarget, PresentationState, register_default_runtime_adapter_factory,
};

pub const MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID: &str = "macos_software_desktop";
pub const MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID: &str = "macos_host";

pub use native::{
    MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID, MacosAvFoundationBridge,
    MacosAvFoundationBridgeBindings, MacosAvFoundationBridgeContext, MacosNativePlayerBridge,
    MacosNativePlayerProbe, MacosNativePlayerRuntimeAdapterFactory,
};
pub use system::{
    MacosMetalLayerPresenter, MacosSystemAvFoundationBridgeBindings, MacosVideoLayerFrame,
    MacosVideoLayerSurface, install_default_macos_system_native_runtime_adapter_factory,
    macos_system_native_runtime_adapter_factory, probe_source_with_avfoundation,
};

#[derive(Debug, Clone)]
pub struct MacosHostRuntimeProbe {
    pub adapter_id: &'static str,
    pub capabilities: PlayerRuntimeAdapterCapabilities,
    pub media_info: PlayerMediaInfo,
    pub startup: PlayerRuntimeStartup,
}

pub fn macos_runtime_adapter_factory() -> &'static dyn PlayerRuntimeAdapterFactory {
    static FACTORY: MacosSoftwarePlayerRuntimeAdapterFactory =
        MacosSoftwarePlayerRuntimeAdapterFactory;
    &FACTORY
}

pub fn macos_native_runtime_adapter_factory() -> &'static dyn PlayerRuntimeAdapterFactory {
    macos_system_native_runtime_adapter_factory()
}

pub fn macos_host_runtime_adapter_factory() -> &'static dyn PlayerRuntimeAdapterFactory {
    static FACTORY: MacosHostPlayerRuntimeAdapterFactory = MacosHostPlayerRuntimeAdapterFactory;
    &FACTORY
}

pub fn install_default_macos_runtime_adapter_factory() -> PlayerRuntimeResult<()> {
    install_default_macos_host_runtime_adapter_factory()
}

pub fn install_default_macos_host_runtime_adapter_factory() -> PlayerRuntimeResult<()> {
    register_default_runtime_adapter_factory(macos_host_runtime_adapter_factory())
}

pub fn install_default_macos_software_runtime_adapter_factory() -> PlayerRuntimeResult<()> {
    register_default_runtime_adapter_factory(macos_runtime_adapter_factory())
}

pub fn install_default_macos_native_runtime_adapter_factory() -> PlayerRuntimeResult<()> {
    register_default_runtime_adapter_factory(macos_native_runtime_adapter_factory())
}

pub fn open_macos_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
    open_macos_host_runtime_source_with_options(MediaSource::new(uri), options)
}

pub fn open_macos_software_runtime_uri_with_options_and_interrupt(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
    open_macos_software_runtime_source_with_options_and_interrupt(
        MediaSource::new(uri),
        options,
        interrupt_flag,
    )
}

pub fn probe_macos_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<MacosHostRuntimeProbe> {
    probe_macos_host_runtime_source_with_options(MediaSource::new(uri), options)
}

pub fn probe_macos_host_runtime_source_with_options(
    source: MediaSource,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<MacosHostRuntimeProbe> {
    if !cfg!(target_os = "macos") {
        return Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "macos host runtime strategy can only be probed on macOS targets",
        ));
    }

    let native_factory = macos_system_native_runtime_adapter_factory();
    match PlayerRuntimeInitializer::probe_source_with_factory(
        source.clone(),
        options.clone(),
        native_factory,
    ) {
        Ok(initializer) => Ok(MacosHostRuntimeProbe {
            adapter_id: native_factory.adapter_id(),
            capabilities: initializer.capabilities(),
            media_info: initializer.media_info(),
            startup: apply_decoder_plugin_diagnostics(
                initializer.startup(),
                &initializer.media_info(),
                &options,
            ),
        }),
        Err(native_error) => {
            let software_factory = macos_runtime_adapter_factory();
            let initializer = PlayerRuntimeInitializer::probe_source_with_factory(
                source,
                options.clone(),
                software_factory,
            )?;
            let mut startup = initializer.startup();
            if let Some(video_decode) = startup.video_decode.as_mut() {
                video_decode.fallback_reason = Some(format!(
                    "macos native host runtime probe failed; selected software desktop path: {}",
                    native_error.message()
                ));
            }
            startup =
                apply_decoder_plugin_diagnostics(startup, &initializer.media_info(), &options);

            Ok(MacosHostRuntimeProbe {
                adapter_id: software_factory.adapter_id(),
                capabilities: initializer.capabilities(),
                media_info: initializer.media_info(),
                startup,
            })
        }
    }
}

pub fn open_macos_host_runtime_source_with_options(
    source: MediaSource,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
    if !cfg!(target_os = "macos") {
        return Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "macos host runtime strategy can only be initialized on macOS targets",
        ));
    }

    let native_factory = macos_system_native_runtime_adapter_factory();

    let native_initializer = PlayerRuntimeInitializer::probe_source_with_factory(
        source.clone(),
        options.clone(),
        native_factory,
    );

    match native_initializer {
        Ok(initializer) if should_prefer_native_host_runtime(&initializer.media_info(), &options) => {
            let media_info = initializer.media_info();
            match initializer.initialize() {
                Ok(mut bootstrap) => {
                    bootstrap.startup =
                        apply_decoder_plugin_diagnostics(bootstrap.startup, &media_info, &options);
                    Ok(bootstrap)
                }
                Err(native_error) => open_software_fallback_runtime(
                    source,
                    options,
                    Some(format!(
                        "macos native host runtime failed to initialize; falling back to software desktop path: {}",
                        native_error.message()
                    )),
                ),
            }
        }
        Ok(initializer) => open_software_fallback_runtime(
            source,
            options,
            initializer.media_info().best_video.as_ref().map(|video| {
                format!(
                    "macos native host runtime requires an external video surface for {} playback; selected software desktop path",
                    video.codec
                )
            }),
        ),
        Err(native_error) => open_software_fallback_runtime(
            source,
            options,
            Some(format!(
                "macos native host runtime probe failed; selected software desktop path: {}",
                native_error.message()
            )),
        ),
    }
}

pub fn open_macos_software_runtime_source_with_options_and_interrupt(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
    let selection = probe_platform_desktop_source_with_options(
        MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        source.clone(),
        options.clone(),
    )
    .ok()
    .and_then(|initializer| {
        select_macos_native_frame_decoder(
            &source,
            &initializer.media_info(),
            &options,
            Some(interrupt_flag.clone()),
        )
    });
    let selected_plugin_name = selection
        .as_ref()
        .and_then(|selection| selection.plugin_name.clone());

    let open_result = match selection.clone() {
        Some(selection) => {
            open_platform_desktop_source_with_video_source_factory_and_options_and_interrupt(
                MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                source.clone(),
                options.clone(),
                interrupt_flag.clone(),
                Arc::new(MacosNativeFrameVideoSourceFactory {
                    plugin_path: selection.plugin_path,
                    video_surface: selection.video_surface,
                }),
                macos_native_frame_decoder_capabilities(),
            )
        }
        None => open_platform_desktop_source_with_options_and_interrupt(
            MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            source.clone(),
            options.clone(),
            interrupt_flag.clone(),
        ),
    };

    let PlayerRuntimeAdapterBootstrap {
        runtime,
        initial_frame,
        startup,
    } = match (open_result, selection) {
        (Ok(bootstrap), _) => bootstrap,
        (Err(native_error), Some(_)) => {
            let mut bootstrap = open_platform_desktop_source_with_options_and_interrupt(
                MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                source,
                options.clone(),
                interrupt_flag,
            )?;
            if let Some(video_decode) = bootstrap.startup.video_decode.as_mut() {
                video_decode.fallback_reason = Some(format!(
                    "native-frame decoder plugin initialization failed; selected FFmpeg software path: {}",
                    native_error.message()
                ));
            }
            bootstrap
        }
        (Err(error), None) => return Err(error),
    };
    let mut diagnostics = macos_runtime_diagnostics(runtime.media_info(), &options);
    if runtime.capabilities().supports_hardware_decode
        && runtime.capabilities().supports_external_video_surface
    {
        diagnostics.video_decode =
            macos_native_frame_decoder_video_decode_info(selected_plugin_name.as_deref());
        diagnostics.has_video_surface = true;
    }

    Ok(PlayerRuntime::from_adapter_bootstrap(
        MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(MacosRuntimeAdapter {
                inner: runtime,
                video_decode: diagnostics.video_decode.clone(),
                has_video_surface: diagnostics.has_video_surface,
                runtime_fallback: None,
                pending_runtime_fallback_events: VecDeque::new(),
            }),
            initial_frame,
            startup: apply_macos_runtime_diagnostics(startup, &diagnostics),
        },
    ))
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MacosHostPlayerRuntimeAdapterFactory;

#[derive(Debug, Default, Clone, Copy)]
pub struct MacosSoftwarePlayerRuntimeAdapterFactory;

#[allow(clippy::large_enum_variant)]
enum MacosHostRuntimeSelection {
    NativePreferred {
        initializer: Box<dyn PlayerRuntimeAdapterInitializer>,
        source: MediaSource,
        options: PlayerRuntimeOptions,
        software_fallback_factory: Arc<dyn MacosHostFallbackFactory>,
    },
    SoftwarePreferred {
        initializer: Box<dyn PlayerRuntimeAdapterInitializer>,
    },
}

struct MacosHostRuntimeAdapterInitializer {
    selection: MacosHostRuntimeSelection,
    capabilities: PlayerRuntimeAdapterCapabilities,
    media_info: PlayerMediaInfo,
    startup: PlayerRuntimeStartup,
}

trait MacosHostFallbackFactory: Send + Sync {
    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>>;
}

#[derive(Debug, Default)]
struct MacosSoftwareFallbackFactory;

#[derive(Debug, Clone)]
struct MacosRuntimeDiagnostics {
    video_decode: PlayerVideoDecodeInfo,
    plugin_diagnostics: Vec<PlayerPluginDiagnostic>,
    has_video_surface: bool,
}

struct MacosRuntimeAdapterInitializer {
    inner: Box<dyn PlayerRuntimeAdapterInitializer>,
    diagnostics: MacosRuntimeDiagnostics,
    fallback: Option<MacosRuntimeAdapterFallback>,
    runtime_fallback: Option<MacosRuntimeActiveFallback>,
}

struct MacosRuntimeAdapterFallback {
    inner: Box<dyn PlayerRuntimeAdapterInitializer>,
    diagnostics: MacosRuntimeDiagnostics,
    fallback_reason: String,
}

#[derive(Clone)]
struct MacosRuntimeActiveFallback {
    source: MediaSource,
    options: PlayerRuntimeOptions,
    fallback_reason: String,
}

struct MacosRuntimeAdapter {
    inner: Box<dyn PlayerRuntimeAdapter>,
    video_decode: PlayerVideoDecodeInfo,
    has_video_surface: bool,
    runtime_fallback: Option<MacosRuntimeActiveFallback>,
    pending_runtime_fallback_events: VecDeque<PlayerRuntimeEvent>,
}

impl PlayerRuntimeAdapterFactory for MacosHostPlayerRuntimeAdapterFactory {
    fn adapter_id(&self) -> &'static str {
        MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID
    }

    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        if !cfg!(target_os = "macos") {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "macos host runtime adapter can only be initialized on macOS targets",
            ));
        }

        probe_macos_host_runtime_initializer_with_factories(
            source,
            options,
            macos_system_native_runtime_adapter_factory(),
            Arc::new(MacosSoftwareFallbackFactory),
        )
    }
}

impl PlayerRuntimeAdapterFactory for MacosSoftwarePlayerRuntimeAdapterFactory {
    fn adapter_id(&self) -> &'static str {
        MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
    }

    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        if !cfg!(target_os = "macos") {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "macos desktop adapter can only be initialized on macOS targets",
            ));
        }

        let inner = probe_platform_desktop_source_with_options(
            MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            source.clone(),
            options.clone(),
        )?;
        let media_info = inner.media_info();
        if let Some(selection) =
            select_macos_native_frame_decoder(&source, &media_info, &options, None)
        {
            let capabilities = macos_native_frame_decoder_capabilities();
            let fallback_diagnostics = macos_runtime_diagnostics(&media_info, &options);
            let native_inner = probe_platform_desktop_source_with_video_source_factory_and_options(
                MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                source.clone(),
                options.clone(),
                Arc::new(MacosNativeFrameVideoSourceFactory {
                    plugin_path: selection.plugin_path.clone(),
                    video_surface: selection.video_surface,
                }),
                capabilities,
            )?;
            let media_info = native_inner.media_info();
            let mut diagnostics = macos_runtime_diagnostics(&media_info, &options);
            diagnostics.video_decode =
                macos_native_frame_decoder_video_decode_info(selection.plugin_name.as_deref());
            diagnostics.has_video_surface = true;

            return Ok(Box::new(MacosRuntimeAdapterInitializer {
                inner: native_inner,
                diagnostics,
                fallback: Some(MacosRuntimeAdapterFallback {
                    inner,
                    diagnostics: fallback_diagnostics,
                    fallback_reason: "native-frame decoder plugin initialization failed; selected FFmpeg software path".to_owned(),
                }),
                runtime_fallback: Some(MacosRuntimeActiveFallback {
                    source: source.clone(),
                    options: options.clone(),
                    fallback_reason:
                        "native-frame runtime failed during playback; selected FFmpeg software path"
                            .to_owned(),
                }),
            }));
        }

        let diagnostics = macos_runtime_diagnostics(&media_info, &options);

        Ok(Box::new(MacosRuntimeAdapterInitializer {
            inner,
            diagnostics,
            fallback: None,
            runtime_fallback: None,
        }))
    }
}

impl PlayerRuntimeAdapterInitializer for MacosHostRuntimeAdapterInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.capabilities.clone()
    }

    fn media_info(&self) -> PlayerMediaInfo {
        self.media_info.clone()
    }

    fn startup(&self) -> PlayerRuntimeStartup {
        self.startup.clone()
    }

    fn initialize(self: Box<Self>) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
        let Self {
            selection, startup, ..
        } = *self;

        match selection {
            MacosHostRuntimeSelection::NativePreferred {
                initializer,
                source,
                options,
                software_fallback_factory,
            } => match initializer.initialize() {
                Ok(mut bootstrap) => {
                    bootstrap.startup = startup;
                    Ok(bootstrap)
                }
                Err(native_error) => open_software_fallback_adapter_with_factory(
                    source,
                    options,
                    software_fallback_factory.as_ref(),
                    Some(format!(
                        "macos native host runtime failed to initialize; falling back to software desktop path: {}",
                        native_error.message()
                    )),
                ),
            },
            MacosHostRuntimeSelection::SoftwarePreferred { initializer } => {
                let mut bootstrap = initializer.initialize()?;
                bootstrap.startup = startup;
                Ok(bootstrap)
            }
        }
    }
}

impl PlayerRuntimeAdapterInitializer for MacosRuntimeAdapterInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.inner.capabilities()
    }

    fn media_info(&self) -> PlayerMediaInfo {
        self.inner.media_info()
    }

    fn startup(&self) -> PlayerRuntimeStartup {
        apply_macos_runtime_diagnostics(self.inner.startup(), &self.diagnostics)
    }

    fn initialize(self: Box<Self>) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
        let Self {
            inner,
            diagnostics,
            fallback,
            runtime_fallback,
        } = *self;

        match inner.initialize() {
            Ok(bootstrap) => Ok(wrap_macos_runtime_bootstrap(
                bootstrap,
                diagnostics,
                runtime_fallback,
            )),
            Err(native_error) => {
                let Some(fallback) = fallback else {
                    return Err(native_error);
                };
                let mut diagnostics = fallback.diagnostics;
                diagnostics.video_decode.fallback_reason = Some(merge_runtime_fallback_reason(
                    fallback.fallback_reason.as_str(),
                    native_error.message(),
                    diagnostics.video_decode.fallback_reason.take(),
                ));
                let mut bootstrap = fallback.inner.initialize()?;
                apply_video_decode_fallback_reason(
                    &mut bootstrap.startup,
                    diagnostics.video_decode.fallback_reason.clone(),
                );
                Ok(wrap_macos_runtime_bootstrap(bootstrap, diagnostics, None))
            }
        }
    }
}

fn wrap_macos_runtime_bootstrap(
    bootstrap: PlayerRuntimeAdapterBootstrap,
    diagnostics: MacosRuntimeDiagnostics,
    runtime_fallback: Option<MacosRuntimeActiveFallback>,
) -> PlayerRuntimeAdapterBootstrap {
    let PlayerRuntimeAdapterBootstrap {
        runtime,
        initial_frame,
        startup,
    } = bootstrap;

    PlayerRuntimeAdapterBootstrap {
        runtime: Box::new(MacosRuntimeAdapter {
            inner: runtime,
            video_decode: diagnostics.video_decode.clone(),
            has_video_surface: diagnostics.has_video_surface,
            runtime_fallback,
            pending_runtime_fallback_events: VecDeque::new(),
        }),
        initial_frame,
        startup: apply_macos_runtime_diagnostics(startup, &diagnostics),
    }
}

impl MacosHostFallbackFactory for MacosSoftwareFallbackFactory {
    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        macos_runtime_adapter_factory().probe_source_with_options(source, options)
    }
}

impl PlayerRuntimeAdapter for MacosRuntimeAdapter {
    fn source_uri(&self) -> &str {
        self.inner.source_uri()
    }

    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.inner.capabilities()
    }

    fn media_info(&self) -> &PlayerMediaInfo {
        self.inner.media_info()
    }

    fn presentation_state(&self) -> PresentationState {
        self.inner.presentation_state()
    }

    fn has_video_surface(&self) -> bool {
        self.has_video_surface || self.inner.has_video_surface()
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

    fn progress(&self) -> PlaybackProgress {
        self.inner.progress()
    }

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        let mut events = self
            .inner
            .drain_events()
            .into_iter()
            .map(|event| match event {
                PlayerRuntimeEvent::Initialized(startup) => PlayerRuntimeEvent::Initialized(
                    apply_video_decode_diagnostics(startup, &self.video_decode),
                ),
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
        command: PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
        match self.inner.dispatch(command.clone()) {
            Ok(result) => Ok(result),
            Err(error)
                if should_trigger_runtime_fallback_for_command(&command, &error)
                    && self.runtime_fallback.is_some() =>
            {
                self.activate_runtime_fallback(error.message())?;
                self.inner.dispatch(command)
            }
            Err(error) => Err(error),
        }
    }

    fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
        match self.inner.advance() {
            Ok(frame) => Ok(frame),
            Err(error)
                if should_trigger_runtime_fallback_for_advance(&error)
                    && self.runtime_fallback.is_some() =>
            {
                self.activate_runtime_fallback(error.message())?;
                self.inner.advance()
            }
            Err(error) => Err(error),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.inner.next_deadline()
    }
}

impl MacosRuntimeAdapter {
    fn activate_runtime_fallback(
        &mut self,
        runtime_error_message: &str,
    ) -> PlayerRuntimeResult<()> {
        let Some(fallback) = self.runtime_fallback.take() else {
            return Ok(());
        };

        self.activate_runtime_fallback_with(runtime_error_message, fallback, |source, options| {
            open_platform_desktop_source_with_options_and_interrupt(
                MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                source,
                options,
                Arc::new(AtomicBool::new(false)),
            )
        })
    }

    fn activate_runtime_fallback_with(
        &mut self,
        runtime_error_message: &str,
        fallback: MacosRuntimeActiveFallback,
        open_fallback: impl FnOnce(
            MediaSource,
            PlayerRuntimeOptions,
        ) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap>,
    ) -> PlayerRuntimeResult<()> {
        let progress = self.inner.progress();
        let playback_rate = self.inner.playback_rate();
        let was_playing = self.inner.presentation_state() == PresentationState::Playing;
        let mut bootstrap = open_fallback(fallback.source, fallback.options)?;

        let fallback_reason = merge_runtime_fallback_reason(
            fallback.fallback_reason.as_str(),
            runtime_error_message,
            None,
        );
        apply_video_decode_fallback_reason(&mut bootstrap.startup, Some(fallback_reason.clone()));

        let mut runtime = bootstrap.runtime;
        if !progress.position().is_zero() {
            let _ = runtime.dispatch(PlayerRuntimeCommand::SeekTo {
                position: progress.position(),
            })?;
        }
        if (playback_rate - 1.0).abs() > f32::EPSILON {
            let _ = runtime.dispatch(PlayerRuntimeCommand::SetPlaybackRate {
                rate: playback_rate,
            })?;
        }
        if was_playing {
            let _ = runtime.dispatch(PlayerRuntimeCommand::Play)?;
        }

        self.inner = runtime;
        if let Some(video_decode) = bootstrap.startup.video_decode.as_ref() {
            self.video_decode = video_decode.clone();
        }
        self.has_video_surface = false;
        self.pending_runtime_fallback_events
            .extend(runtime_fallback_events(runtime_error_message));

        Ok(())
    }
}

fn should_trigger_runtime_fallback_for_advance(error: &PlayerRuntimeError) -> bool {
    if error.code() != PlayerRuntimeErrorCode::BackendFailure {
        return false;
    }
    let message = error.message().to_ascii_lowercase();
    message.contains("failed to present decoded video frame")
        || message.contains("failed to present seeked video frame")
        || message.contains("present")
        || message.contains("native-frame decoder")
        || message.contains("videotoolbox")
}

fn should_trigger_runtime_fallback_for_command(
    command: &PlayerRuntimeCommand,
    error: &PlayerRuntimeError,
) -> bool {
    if error.code() != PlayerRuntimeErrorCode::BackendFailure {
        return false;
    }
    let message = error.message().to_ascii_lowercase();
    match command {
        PlayerRuntimeCommand::SeekTo { .. } => {
            message.contains("seek") || message.contains("present")
        }
        PlayerRuntimeCommand::Play => message.contains("play") || message.contains("present"),
        PlayerRuntimeCommand::SetPlaybackRate { .. } => {
            message.contains("rate") || message.contains("present")
        }
        _ => false,
    }
}

#[derive(Debug, Clone)]
struct MacosNativeFrameDecoderSelection {
    plugin_path: PathBuf,
    plugin_name: Option<String>,
    video_surface: PlayerVideoSurfaceTarget,
}

#[derive(Debug)]
struct MacosNativeFrameVideoSourceFactory {
    plugin_path: PathBuf,
    video_surface: PlayerVideoSurfaceTarget,
}

struct MacosNativeFrameVideoSource {
    packet_source: VideoPacketSource,
    stream_info: VideoPacketStreamInfo,
    shared: Arc<Mutex<MacosNativeFrameDecoderState>>,
    end_of_input_sent: bool,
}

struct MacosNativeFrameDecoderState {
    session: Box<dyn NativeDecoderSession>,
    presenter: MacosMetalLayerPresenter,
    outstanding_frames: Arc<AtomicUsize>,
    presentation_epoch: u64,
}

#[derive(Debug)]
struct MacosDeferredNativeFramePresentation {
    shared: Arc<Mutex<MacosNativeFrameDecoderState>>,
    frame: Option<DecoderNativeFrame>,
    presentation_epoch: u64,
}

impl std::fmt::Debug for MacosNativeFrameVideoSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosNativeFrameVideoSource")
            .field("codec", &self.stream_info.codec)
            .field("end_of_input_sent", &self.end_of_input_sent)
            .finish()
    }
}

impl std::fmt::Debug for MacosNativeFrameDecoderState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosNativeFrameDecoderState").finish()
    }
}

impl Drop for MacosNativeFrameDecoderState {
    fn drop(&mut self) {
        let outstanding = self.outstanding_frames.load(Ordering::SeqCst);
        if outstanding != 0 {
            eprintln!(
                "dropping macOS native-frame decoder state with unreleased frames: {outstanding}"
            );
        }
    }
}

impl DesktopVideoSourceFactory for MacosNativeFrameVideoSourceFactory {
    fn open_video_source(
        &self,
        source: MediaSource,
        _buffer_capacity: usize,
        interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> anyhow::Result<DesktopVideoSourceBootstrap> {
        let backend = FfmpegBackend::new().context("failed to initialize FFmpeg backend")?;
        let probe = backend
            .probe_with_interrupt(source.clone(), interrupt_flag.clone())
            .context("failed to probe media source for native-frame decoder")?;
        let packet_source = backend
            .open_video_packet_source_with_interrupt(source, interrupt_flag)
            .context("failed to open FFmpeg packet source for native-frame decoder")?;
        let stream_info = packet_source.stream_info().clone();
        let plugin = LoadedDynamicPlugin::load(&self.plugin_path).with_context(|| {
            format!(
                "failed to load native-frame decoder plugin {}",
                self.plugin_path.display()
            )
        })?;
        let factory = plugin.native_decoder_plugin_factory().ok_or_else(|| {
            anyhow::anyhow!("decoder plugin does not export a v2 native-frame API")
        })?;
        if !factory
            .capabilities()
            .supports_codec(&stream_info.codec, DecoderMediaKind::Video)
        {
            anyhow::bail!(
                "native-frame decoder plugin `{}` does not support {} video",
                factory.name(),
                stream_info.codec
            );
        }

        let session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: stream_info.codec.clone(),
                media_kind: DecoderMediaKind::Video,
                extradata: stream_info.extradata.clone(),
                width: stream_info.width,
                height: stream_info.height,
                prefer_hardware: true,
                require_cpu_output: false,
                ..DecoderSessionConfig::default()
            })
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let session_info = session.session_info();
        let presenter = MacosMetalLayerPresenter::new(self.video_surface)
            .map_err(|error| anyhow::anyhow!(error.message().to_owned()))?;
        let decode_info = BackendVideoDecodeInfo {
            selected_mode: BackendVideoDecoderMode::Hardware,
            hardware_available: true,
            hardware_backend: session_info
                .selected_hardware_backend
                .or_else(|| Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned())),
            decoder_name: session_info
                .decoder_name
                .unwrap_or_else(|| factory.name().to_owned()),
            fallback_reason: None,
        };
        let shared = Arc::new(Mutex::new(MacosNativeFrameDecoderState {
            session,
            presenter,
            outstanding_frames: Arc::new(AtomicUsize::new(0)),
            presentation_epoch: 0,
        }));

        Ok(DesktopVideoSourceBootstrap {
            source: Box::new(MacosNativeFrameVideoSource {
                packet_source,
                stream_info,
                shared,
                end_of_input_sent: false,
            }),
            decode_info,
            probe,
        })
    }
}

impl DesktopVideoSource for MacosNativeFrameVideoSource {
    fn recv_frame(&mut self) -> anyhow::Result<Option<DesktopVideoFrame>> {
        loop {
            match self.poll_frame(true)? {
                DesktopVideoFramePoll::Ready(frame) => return Ok(Some(frame)),
                DesktopVideoFramePoll::EndOfStream => return Ok(None),
                DesktopVideoFramePoll::Pending => continue,
            }
        }
    }

    fn try_recv_frame(&mut self) -> anyhow::Result<DesktopVideoFramePoll> {
        self.poll_frame(false)
    }

    fn seek_to(&mut self, position: Duration) -> anyhow::Result<Option<DesktopVideoFrame>> {
        {
            let mut shared = self
                .shared
                .lock()
                .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
            shared
                .session
                .flush()
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            shared.presentation_epoch = shared.presentation_epoch.saturating_add(1);
        }
        self.packet_source.seek_to(position)?;
        self.end_of_input_sent = false;
        self.recv_frame()
    }

    fn buffered_frame_count(&self) -> usize {
        0
    }

    fn set_prefetch_limit(&self, _limit: usize) {}
}

impl MacosNativeFrameVideoSource {
    fn poll_frame(&mut self, blocking: bool) -> anyhow::Result<DesktopVideoFramePoll> {
        let mut packets_submitted = 0usize;
        loop {
            match self.receive_native_frame()? {
                DecoderReceiveNativeFrameOutput::Frame(frame) => {
                    return self
                        .deferred_desktop_frame(frame)
                        .map(DesktopVideoFramePoll::Ready);
                }
                DecoderReceiveNativeFrameOutput::Eof => {
                    return Ok(DesktopVideoFramePoll::EndOfStream);
                }
                DecoderReceiveNativeFrameOutput::NeedMoreInput => {}
            }

            if self.end_of_input_sent {
                return Ok(DesktopVideoFramePoll::Pending);
            }

            match self.packet_source.next_packet()? {
                Some(packet) => {
                    self.send_packet(packet)?;
                    packets_submitted = packets_submitted.saturating_add(1);
                    if !blocking && packets_submitted >= 4 {
                        return Ok(DesktopVideoFramePoll::Pending);
                    }
                }
                None => {
                    self.send_end_of_stream()?;
                    self.end_of_input_sent = true;
                }
            }
        }
    }

    fn receive_native_frame(&mut self) -> anyhow::Result<DecoderReceiveNativeFrameOutput> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
        let result = shared
            .session
            .receive_native_frame()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if matches!(result, DecoderReceiveNativeFrameOutput::Frame(_)) {
            shared.outstanding_frames.fetch_add(1, Ordering::SeqCst);
        }
        Ok(result)
    }

    fn send_packet(&mut self, packet: CompressedVideoPacket) -> anyhow::Result<()> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
        shared
            .session
            .send_packet(
                &DecoderPacket {
                    pts_us: packet.pts_us,
                    dts_us: packet.dts_us,
                    duration_us: packet.duration_us,
                    stream_index: packet.stream_index,
                    key_frame: packet.key_frame,
                    discontinuity: packet.discontinuity,
                    end_of_stream: false,
                },
                &packet.data,
            )
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(())
    }

    fn send_end_of_stream(&mut self) -> anyhow::Result<()> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
        shared
            .session
            .send_packet(
                &DecoderPacket {
                    stream_index: u32::try_from(self.stream_info.stream_index).unwrap_or(u32::MAX),
                    end_of_stream: true,
                    ..DecoderPacket::default()
                },
                &[],
            )
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    fn deferred_desktop_frame(
        &self,
        frame: DecoderNativeFrame,
    ) -> anyhow::Result<DesktopVideoFrame> {
        if frame.metadata.handle_kind != DecoderNativeHandleKind::CvPixelBuffer {
            let mut shared = self
                .shared
                .lock()
                .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
            let _ = shared.session.release_native_frame(frame);
            shared.outstanding_frames.fetch_sub(1, Ordering::SeqCst);
            anyhow::bail!("macOS native-frame presenter only accepts CVPixelBuffer handles");
        }
        let presentation_time = frame
            .metadata
            .pts_us
            .and_then(duration_from_micros)
            .unwrap_or(Duration::ZERO);
        let width = frame.metadata.width;
        let height = frame.metadata.height;
        Ok(DesktopVideoFrame::native_deferred(
            presentation_time,
            width,
            height,
            Box::new(MacosDeferredNativeFramePresentation {
                shared: self.shared.clone(),
                frame: Some(frame),
                presentation_epoch: shared_presentation_epoch(&self.shared)?,
            }),
        ))
    }
}

impl DesktopVideoFramePresentation for MacosDeferredNativeFramePresentation {
    fn present(mut self: Box<Self>) -> anyhow::Result<()> {
        let Some(frame) = self.frame.take() else {
            return Ok(());
        };
        present_and_release_native_frame(&self.shared, frame, self.presentation_epoch)
    }
}

impl Drop for MacosDeferredNativeFramePresentation {
    fn drop(&mut self) {
        if let Some(frame) = self.frame.take()
            && let Ok(mut shared) = self.shared.lock()
        {
            let _ = release_native_frame_and_track(&mut shared, frame);
        }
    }
}

fn release_native_frame_and_track(
    shared: &mut MacosNativeFrameDecoderState,
    frame: DecoderNativeFrame,
) -> Result<(), player_plugin::DecoderError> {
    release_native_frame_with_counter(
        shared.session.as_mut(),
        shared.outstanding_frames.as_ref(),
        frame,
    )
}

fn release_native_frame_with_counter(
    session: &mut dyn NativeDecoderSession,
    outstanding_frames: &AtomicUsize,
    frame: DecoderNativeFrame,
) -> Result<(), player_plugin::DecoderError> {
    let result = session.release_native_frame(frame);
    if result.is_ok() {
        outstanding_frames.fetch_sub(1, Ordering::SeqCst);
    }
    result
}

fn present_and_release_native_frame(
    shared: &Arc<Mutex<MacosNativeFrameDecoderState>>,
    frame: DecoderNativeFrame,
    presentation_epoch: u64,
) -> anyhow::Result<()> {
    let mut shared = shared
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
    let MacosNativeFrameDecoderState {
        session,
        presenter,
        outstanding_frames,
        presentation_epoch: current_epoch,
    } = &mut *shared;
    if *current_epoch != presentation_epoch {
        return release_native_frame_with_counter(
            session.as_mut(),
            outstanding_frames.as_ref(),
            frame,
        )
        .map_err(|error| anyhow::anyhow!(error.to_string()));
    }
    present_and_release_native_frame_with(
        session.as_mut(),
        presenter,
        outstanding_frames.as_ref(),
        frame,
    )
}

#[cfg(test)]
fn present_if_current_epoch_and_release(
    session: &mut dyn NativeDecoderSession,
    outstanding_frames: &AtomicUsize,
    current_epoch: u64,
    presentation_epoch: u64,
    frame: DecoderNativeFrame,
    present: impl FnOnce(DecoderNativeFrame) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    if current_epoch != presentation_epoch {
        return release_native_frame_with_counter(session, outstanding_frames, frame)
            .map_err(|error| anyhow::anyhow!(error.to_string()));
    }
    present(frame)
}

fn shared_presentation_epoch(
    shared: &Arc<Mutex<MacosNativeFrameDecoderState>>,
) -> anyhow::Result<u64> {
    shared
        .lock()
        .map(|state| state.presentation_epoch)
        .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))
}

fn present_and_release_native_frame_with(
    session: &mut dyn NativeDecoderSession,
    presenter: &mut MacosMetalLayerPresenter,
    outstanding_frames: &AtomicUsize,
    frame: DecoderNativeFrame,
) -> anyhow::Result<()> {
    present_and_release_native_frame_with_presenter(session, outstanding_frames, frame, |handle| {
        presenter
            .present_cv_pixel_buffer_handle(handle)
            .map_err(|error| error.message().to_owned())
    })
}

fn present_and_release_native_frame_with_presenter(
    session: &mut dyn NativeDecoderSession,
    outstanding_frames: &AtomicUsize,
    frame: DecoderNativeFrame,
    present: impl FnOnce(usize) -> Result<(), String>,
) -> anyhow::Result<()> {
    let present_result = present(frame.handle).map_err(|error| anyhow::anyhow!(error));
    let release_result = release_native_frame_with_counter(session, outstanding_frames, frame)
        .map_err(|error| anyhow::anyhow!(error.to_string()));

    present_result.and(release_result)
}

fn select_macos_native_frame_decoder(
    source: &MediaSource,
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Option<MacosNativeFrameDecoderSelection> {
    if options.decoder_plugin_video_mode != PlayerDecoderPluginVideoMode::PreferNativeFrame {
        return None;
    }
    let video_surface = options.video_surface?;
    if options.decoder_plugin_library_paths.is_empty() {
        return None;
    }
    let codec =
        native_frame_decoder_codec(source, media_info, interrupt_flag).unwrap_or_else(|| {
            media_info
                .best_video
                .as_ref()
                .map(|video| video.codec.clone())
                .unwrap_or_default()
        });
    if codec.is_empty() {
        return None;
    }
    let request = DecoderPluginMatchRequest::video(codec);
    let registry = PluginRegistry::inspect_decoder_support(
        &options.decoder_plugin_library_paths,
        request.clone(),
    );
    let record = registry.best_native_decoder_for(&request)?;
    Some(MacosNativeFrameDecoderSelection {
        plugin_path: record.path.clone(),
        plugin_name: record.plugin_name.clone(),
        video_surface,
    })
}

fn native_frame_decoder_codec(
    source: &MediaSource,
    media_info: &PlayerMediaInfo,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Option<String> {
    if let Some(best_video) = media_info.best_video.as_ref() {
        return Some(best_video.codec.clone());
    }
    if source.protocol() != MediaSourceProtocol::Hls {
        return None;
    }

    let backend = FfmpegBackend::new().ok()?;
    backend
        .open_video_packet_source_with_interrupt(source.clone(), interrupt_flag)
        .ok()
        .map(|packet_source| packet_source.stream_info().codec.clone())
}

fn macos_native_frame_decoder_video_decode_info(
    plugin_name: Option<&str>,
) -> PlayerVideoDecodeInfo {
    PlayerVideoDecodeInfo {
        selected_mode: PlayerVideoDecodeMode::Hardware,
        hardware_available: true,
        hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
        fallback_reason: plugin_name.map(|name| {
            format!("decoder plugin `{name}` selected for native-frame VideoToolbox playback")
        }),
    }
}

fn macos_native_frame_decoder_capabilities() -> PlayerRuntimeAdapterCapabilities {
    PlayerRuntimeAdapterCapabilities {
        adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
        supports_audio_output: true,
        supports_frame_output: false,
        supports_external_video_surface: true,
        supports_seek: true,
        supports_stop: true,
        supports_playback_rate: true,
        playback_rate_min: Some(player_runtime::MIN_PLAYBACK_RATE),
        playback_rate_max: Some(player_runtime::MAX_PLAYBACK_RATE),
        natural_playback_rate_max: Some(player_runtime::NATURAL_PLAYBACK_RATE_MAX),
        supports_hardware_decode: true,
        supports_streaming: true,
        supports_hdr: true,
    }
}

fn duration_from_micros(value: i64) -> Option<Duration> {
    if value < 0 {
        return None;
    }
    Some(Duration::from_micros(value as u64))
}

fn apply_video_decode_diagnostics(
    mut startup: PlayerRuntimeStartup,
    video_decode: &PlayerVideoDecodeInfo,
) -> PlayerRuntimeStartup {
    match startup.video_decode.as_mut() {
        Some(current) => {
            if !current.hardware_available {
                current.hardware_available = video_decode.hardware_available;
            }
            if current.hardware_backend.is_none() {
                current.hardware_backend = video_decode.hardware_backend.clone();
            }
            if current.fallback_reason.is_none() {
                current.fallback_reason = video_decode.fallback_reason.clone();
            }
        }
        None => {
            startup.video_decode = Some(video_decode.clone());
        }
    }
    startup
}

fn macos_runtime_diagnostics(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> MacosRuntimeDiagnostics {
    let mut video_decode = macos_video_decode_info(media_info);
    let mut plugin_diagnostics = Vec::new();

    if let Some(registry) = decoder_plugin_registry(media_info, options) {
        video_decode =
            apply_decoder_plugin_registry_to_video_decode(video_decode, media_info, &registry);
        plugin_diagnostics.extend(
            registry
                .records()
                .iter()
                .map(player_plugin_diagnostic_from_record),
        );
    }

    video_decode =
        apply_native_frame_plugin_preference_to_video_decode(video_decode, media_info, options);

    MacosRuntimeDiagnostics {
        video_decode,
        plugin_diagnostics,
        has_video_surface: false,
    }
}

fn apply_macos_runtime_diagnostics(
    startup: PlayerRuntimeStartup,
    diagnostics: &MacosRuntimeDiagnostics,
) -> PlayerRuntimeStartup {
    let mut startup = apply_video_decode_diagnostics(startup, &diagnostics.video_decode);
    for diagnostic in &diagnostics.plugin_diagnostics {
        if startup.plugin_diagnostics.iter().any(|existing| {
            existing.path == diagnostic.path && existing.status == diagnostic.status
        }) {
            continue;
        }
        startup.plugin_diagnostics.push(diagnostic.clone());
    }
    startup
}

fn macos_video_decode_info(media_info: &PlayerMediaInfo) -> PlayerVideoDecodeInfo {
    let Some(best_video) = media_info.best_video.as_ref() else {
        return PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Software,
            hardware_available: false,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: Some("source does not expose a decodable video stream".to_owned()),
        };
    };

    let support = probe_videotoolbox_hardware_decode(&best_video.codec);
    let fallback_reason = if support.hardware_available {
        Some(
            "system VideoToolbox hardware decode support detected; Apple platforms should prefer the native backend, while the software desktop path remains available as fallback"
                .to_owned(),
        )
    } else {
        support.fallback_reason.clone()
    };

    PlayerVideoDecodeInfo {
        selected_mode: PlayerVideoDecodeMode::Software,
        hardware_available: support.hardware_available,
        hardware_backend: support.hardware_backend,
        fallback_reason,
    }
}

fn apply_decoder_plugin_diagnostics(
    mut startup: PlayerRuntimeStartup,
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> PlayerRuntimeStartup {
    let Some(registry) = decoder_plugin_registry(media_info, options) else {
        return startup;
    };
    startup.plugin_diagnostics.extend(
        registry
            .records()
            .iter()
            .map(player_plugin_diagnostic_from_record),
    );
    if let Some(video_decode) = startup.video_decode.take() {
        startup.video_decode = Some(apply_decoder_plugin_registry_to_video_decode(
            video_decode,
            media_info,
            &registry,
        ));
    }
    startup
}

#[cfg(test)]
fn apply_decoder_plugin_diagnostics_to_video_decode(
    video_decode: PlayerVideoDecodeInfo,
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> PlayerVideoDecodeInfo {
    let Some(registry) = decoder_plugin_registry(media_info, options) else {
        return video_decode;
    };
    apply_decoder_plugin_registry_to_video_decode(video_decode, media_info, &registry)
}

fn apply_decoder_plugin_registry_to_video_decode(
    mut video_decode: PlayerVideoDecodeInfo,
    media_info: &PlayerMediaInfo,
    registry: &PluginRegistry,
) -> PlayerVideoDecodeInfo {
    if video_decode
        .fallback_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("decoder plugin"))
    {
        return video_decode;
    }

    if let Some(diagnostic) = decoder_plugin_diagnostic(media_info, registry) {
        video_decode.fallback_reason = Some(match video_decode.fallback_reason.take() {
            Some(existing) if !existing.is_empty() => format!("{existing}; {diagnostic}"),
            _ => diagnostic,
        });
    }

    video_decode
}

fn apply_native_frame_plugin_preference_to_video_decode(
    mut video_decode: PlayerVideoDecodeInfo,
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> PlayerVideoDecodeInfo {
    if options.decoder_plugin_video_mode != PlayerDecoderPluginVideoMode::PreferNativeFrame
        || video_decode.selected_mode == PlayerVideoDecodeMode::Hardware
    {
        return video_decode;
    }

    let Some(best_video) = media_info.best_video.as_ref() else {
        return video_decode;
    };

    let reason = if options.decoder_plugin_library_paths.is_empty() {
        Some(format!(
            "native-frame decoder plugin playback requested for {} video but no decoder plugin paths are configured; selected FFmpeg software path",
            best_video.codec
        ))
    } else if options.video_surface.is_none() {
        Some(format!(
            "native-frame decoder plugin playback requested for {} video but no macOS video surface is available; selected FFmpeg software path",
            best_video.codec
        ))
    } else {
        let request = DecoderPluginMatchRequest::video(best_video.codec.clone());
        let registry = PluginRegistry::inspect_decoder_support(
            &options.decoder_plugin_library_paths,
            request.clone(),
        );
        (!registry.supports_native_decoder(&request)).then(|| {
            format!(
                "native-frame decoder plugin playback requested for {} video but no matching native-frame decoder is available; selected FFmpeg software path",
                best_video.codec
            )
        })
    };

    if let Some(reason) = reason {
        video_decode.fallback_reason = Some(match video_decode.fallback_reason.take() {
            Some(existing) if !existing.is_empty() => format!("{existing}; {reason}"),
            _ => reason,
        });
    }

    video_decode
}

fn decoder_plugin_registry(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> Option<PluginRegistry> {
    let best_video = media_info.best_video.as_ref()?;
    if options.decoder_plugin_library_paths.is_empty() {
        return None;
    }
    Some(PluginRegistry::inspect_decoder_support(
        &options.decoder_plugin_library_paths,
        DecoderPluginMatchRequest::video(best_video.codec.clone()),
    ))
}

fn decoder_plugin_diagnostic(
    media_info: &PlayerMediaInfo,
    registry: &PluginRegistry,
) -> Option<String> {
    let best_video = media_info.best_video.as_ref()?;
    let request = DecoderPluginMatchRequest::video(best_video.codec.clone());
    let report = registry.report();
    let supported_plugins = decoder_plugin_supported_labels(registry);

    if registry.supports_decoder(&request) {
        return Some(format!(
            "decoder plugin found {}/{} candidate(s) for {} video: {}; diagnostic-only, playback still uses native-first/FFmpeg fallback",
            report.decoder_supported,
            report.total,
            best_video.codec,
            supported_plugins.join(", ")
        ));
    }

    let compact_notes = decoder_plugin_compact_notes(registry);
    Some(format!(
        "decoder plugin paths configured for {} video: {}/{} supported, {} unsupported codec, {} load failed, {} non-decoder{}",
        best_video.codec,
        report.decoder_supported,
        report.total,
        report.decoder_unsupported,
        report.failed,
        report.unsupported_kind,
        if compact_notes.is_empty() {
            String::new()
        } else {
            format!(" ({})", compact_notes.join("; "))
        }
    ))
}

fn decoder_plugin_supported_labels(registry: &PluginRegistry) -> Vec<String> {
    registry
        .records()
        .iter()
        .filter(|record| record.status == PluginDiagnosticStatus::DecoderSupported)
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
        .collect()
}

fn decoder_plugin_compact_notes(registry: &PluginRegistry) -> Vec<String> {
    let mut notes = Vec::new();
    let failed_paths = registry
        .records()
        .iter()
        .filter(|record| record.status == PluginDiagnosticStatus::LoadFailed)
        .map(|record| record.path.display().to_string())
        .collect::<Vec<_>>();
    if !failed_paths.is_empty() {
        notes.push(format!("load failed: {}", failed_paths.join(", ")));
    }

    let unsupported_codecs = registry
        .records()
        .iter()
        .filter(|record| record.status == PluginDiagnosticStatus::DecoderUnsupported)
        .map(plugin_diagnostic_label)
        .collect::<Vec<_>>();
    if !unsupported_codecs.is_empty() {
        notes.push(format!(
            "unsupported codec: {}",
            unsupported_codecs.join(", ")
        ));
    }

    let non_decoders = registry
        .records()
        .iter()
        .filter(|record| record.status == PluginDiagnosticStatus::UnsupportedKind)
        .map(plugin_diagnostic_label)
        .collect::<Vec<_>>();
    if !non_decoders.is_empty() {
        notes.push(format!("non-decoder: {}", non_decoders.join(", ")));
    }

    notes
}

fn plugin_diagnostic_label(record: &PluginDiagnosticRecord) -> String {
    record
        .plugin_name
        .clone()
        .unwrap_or_else(|| record.path.display().to_string())
}

fn player_plugin_diagnostic_from_record(record: &PluginDiagnosticRecord) -> PlayerPluginDiagnostic {
    PlayerPluginDiagnostic {
        path: record.path.display().to_string(),
        plugin_name: record.plugin_name.clone(),
        plugin_kind: record.plugin_kind.map(plugin_kind_label).map(str::to_owned),
        status: match record.status {
            PluginDiagnosticStatus::Loaded => PlayerPluginDiagnosticStatus::Loaded,
            PluginDiagnosticStatus::LoadFailed => PlayerPluginDiagnosticStatus::LoadFailed,
            PluginDiagnosticStatus::UnsupportedKind => {
                PlayerPluginDiagnosticStatus::UnsupportedKind
            }
            PluginDiagnosticStatus::DecoderSupported => {
                PlayerPluginDiagnosticStatus::DecoderSupported
            }
            PluginDiagnosticStatus::DecoderUnsupported => {
                PlayerPluginDiagnosticStatus::DecoderUnsupported
            }
        },
        message: record.message.clone(),
        decoder_capabilities: record
            .decoder_capabilities
            .as_ref()
            .map(player_decoder_capability_summary_from_loader),
    }
}

fn player_decoder_capability_summary_from_loader(
    summary: &DecoderPluginCapabilitySummary,
) -> PlayerPluginDecoderCapabilitySummary {
    PlayerPluginDecoderCapabilitySummary {
        codecs: summary
            .typed_codecs
            .iter()
            .map(player_decoder_codec_summary_from_loader)
            .collect(),
        legacy_codecs: summary.codecs.clone(),
        supports_native_frame_output: summary.supports_native_frame_output,
        supports_hardware_decode: summary.supports_hardware_decode,
        supports_cpu_video_frames: summary.supports_cpu_video_frames,
        supports_audio_frames: summary.supports_audio_frames,
        supports_gpu_handles: summary.supports_gpu_handles,
        supports_flush: summary.supports_flush,
        supports_drain: summary.supports_drain,
        max_sessions: summary.max_sessions,
    }
}

fn player_decoder_codec_summary_from_loader(
    summary: &DecoderPluginCodecSummary,
) -> PlayerPluginCodecCapability {
    PlayerPluginCodecCapability {
        media_kind: match summary.media_kind {
            DecoderMediaKind::Video => "video",
            DecoderMediaKind::Audio => "audio",
        }
        .to_owned(),
        codec: summary.codec.clone(),
    }
}

fn plugin_kind_label(kind: VesperPluginKind) -> &'static str {
    match kind {
        VesperPluginKind::PostDownloadProcessor => "post_download_processor",
        VesperPluginKind::PipelineEventHook => "pipeline_event_hook",
        VesperPluginKind::Decoder => "decoder",
        VesperPluginKind::BenchmarkSink => "benchmark_sink",
    }
}

fn should_prefer_native_host_runtime(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> bool {
    options.video_surface.is_some() || media_info.best_video.is_none()
}

fn probe_macos_host_runtime_initializer_with_factories(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    native_factory: &dyn PlayerRuntimeAdapterFactory,
    software_fallback_factory: Arc<dyn MacosHostFallbackFactory>,
) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
    match native_factory.probe_source_with_options(source.clone(), options.clone()) {
        Ok(initializer) => {
            let capabilities = initializer.capabilities();
            let media_info = initializer.media_info();
            let startup =
                apply_decoder_plugin_diagnostics(initializer.startup(), &media_info, &options);

            if should_prefer_native_host_runtime(&media_info, &options) {
                Ok(Box::new(MacosHostRuntimeAdapterInitializer {
                    selection: MacosHostRuntimeSelection::NativePreferred {
                        initializer,
                        source,
                        options,
                        software_fallback_factory,
                    },
                    capabilities,
                    media_info,
                    startup,
                }))
            } else {
                let fallback_reason = media_info.best_video.as_ref().map(|video| {
                    format!(
                        "macos native host runtime requires an external video surface for {} playback; selected software desktop path",
                        video.codec
                    )
                });
                probe_software_fallback_initializer(
                    source,
                    options,
                    software_fallback_factory.as_ref(),
                    fallback_reason,
                )
            }
        }
        Err(native_error) => probe_software_fallback_initializer(
            source,
            options,
            software_fallback_factory.as_ref(),
            Some(format!(
                "macos native host runtime probe failed; selected software desktop path: {}",
                native_error.message()
            )),
        ),
    }
}

fn probe_software_fallback_initializer(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    software_factory: &dyn MacosHostFallbackFactory,
    fallback_reason: Option<String>,
) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
    let initializer = software_factory.probe_source_with_options(source, options.clone())?;
    let capabilities = initializer.capabilities();
    let media_info = initializer.media_info();
    let mut startup = initializer.startup();
    apply_video_decode_fallback_reason(&mut startup, fallback_reason);
    startup = apply_decoder_plugin_diagnostics(startup, &media_info, &options);

    Ok(Box::new(MacosHostRuntimeAdapterInitializer {
        selection: MacosHostRuntimeSelection::SoftwarePreferred { initializer },
        capabilities,
        media_info,
        startup,
    }))
}

fn apply_video_decode_fallback_reason(
    startup: &mut PlayerRuntimeStartup,
    fallback_reason: Option<String>,
) {
    if let (Some(video_decode), Some(fallback_reason)) =
        (startup.video_decode.as_mut(), fallback_reason)
    {
        video_decode.fallback_reason = Some(match video_decode.fallback_reason.take() {
            Some(existing) if !existing.is_empty() => format!("{fallback_reason}; {existing}"),
            _ => fallback_reason,
        });
    }
}

fn open_software_fallback_runtime(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    fallback_reason: Option<String>,
) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
    match PlayerRuntime::open_source_with_factory(source, options, macos_runtime_adapter_factory())
    {
        Ok(mut bootstrap) => {
            if let Some(fallback_reason) = fallback_reason
                && let Some(video_decode) = bootstrap.startup.video_decode.as_mut()
            {
                video_decode.fallback_reason = Some(match video_decode.fallback_reason.take() {
                    Some(existing) if !existing.is_empty() => {
                        format!("{fallback_reason}; {existing}")
                    }
                    _ => fallback_reason,
                });
            }
            Ok(bootstrap)
        }
        Err(software_error) => match fallback_reason {
            Some(fallback_reason) => Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                format!(
                    "macos native host playback failed and software fallback also failed: native={}, software={}",
                    fallback_reason,
                    software_error.message()
                ),
            )),
            None => Err(software_error),
        },
    }
}

fn open_software_fallback_adapter_with_factory(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    software_factory: &dyn MacosHostFallbackFactory,
    fallback_reason: Option<String>,
) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
    let initializer = software_factory.probe_source_with_options(source, options)?;
    let mut startup = initializer.startup();
    apply_video_decode_fallback_reason(&mut startup, fallback_reason);
    let mut bootstrap = initializer.initialize()?;
    bootstrap.startup = startup;
    Ok(bootstrap)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    #[cfg(target_os = "macos")]
    use std::os::raw::c_void;
    use std::path::{Path, PathBuf};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use std::time::{Duration, Instant};

    use super::{
        MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID, MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID,
        MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID, MacosHostPlayerRuntimeAdapterFactory,
        MacosRuntimeActiveFallback, MacosRuntimeAdapter, MacosRuntimeAdapterFallback,
        MacosRuntimeAdapterInitializer, MacosRuntimeDiagnostics,
        MacosSoftwarePlayerRuntimeAdapterFactory, apply_decoder_plugin_diagnostics,
        apply_decoder_plugin_diagnostics_to_video_decode,
        apply_decoder_plugin_registry_to_video_decode,
        macos_native_frame_decoder_video_decode_info, macos_runtime_adapter_factory,
        macos_runtime_diagnostics, macos_video_decode_info,
        open_macos_host_runtime_source_with_options,
        open_macos_software_runtime_source_with_options_and_interrupt,
        present_and_release_native_frame_with_presenter, present_if_current_epoch_and_release,
        probe_macos_host_runtime_initializer_with_factories,
        probe_macos_host_runtime_source_with_options, release_native_frame_with_counter,
        should_trigger_runtime_fallback_for_advance, should_trigger_runtime_fallback_for_command,
    };
    use player_backend_ffmpeg::FfmpegBackend;
    use player_core::MediaSource;
    use player_platform_apple::VIDEOTOOLBOX_BACKEND_NAME;
    use player_plugin::{
        DecoderError, DecoderMediaKind, DecoderNativeFrame, DecoderNativeFrameMetadata,
        DecoderNativeHandleKind, DecoderPacket, DecoderPacketResult,
        DecoderReceiveNativeFrameOutput, DecoderSessionConfig, DecoderSessionInfo,
        NativeDecoderSession, VesperPluginKind,
    };
    use player_plugin_loader::{
        DecoderPluginCapabilitySummary, DecoderPluginCodecSummary, LoadedDynamicPlugin,
        PluginDiagnosticRecord, PluginDiagnosticStatus, PluginRegistry,
    };
    use player_runtime::{
        DecodedVideoFrame, PlaybackProgress, PlayerDecoderPluginVideoMode, PlayerMediaInfo,
        PlayerPluginDiagnosticStatus, PlayerRuntimeAdapter, PlayerRuntimeAdapterBackendFamily,
        PlayerRuntimeAdapterBootstrap, PlayerRuntimeAdapterCapabilities,
        PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer, PlayerRuntimeCommand,
        PlayerRuntimeCommandResult, PlayerRuntimeError, PlayerRuntimeErrorCode, PlayerRuntimeEvent,
        PlayerRuntimeInitializer, PlayerRuntimeOptions, PlayerRuntimeResult, PlayerRuntimeStartup,
        PlayerVideoDecodeInfo, PlayerVideoDecodeMode, PlayerVideoInfo, PlayerVideoSurfaceKind,
        PlayerVideoSurfaceTarget, PresentationState,
    };

    #[cfg(target_os = "macos")]
    unsafe extern "C" {
        fn player_macos_test_create_player_layer() -> *mut c_void;
        fn player_macos_test_release_object(handle: *mut c_void);
    }

    #[test]
    fn macos_factory_matches_host_support() {
        let factory = MacosSoftwarePlayerRuntimeAdapterFactory;

        if cfg!(target_os = "macos") {
            let Some(test_video_path) = test_video_path() else {
                eprintln!("skipping macOS fixture-backed test: test-video.mp4 is unavailable");
                return;
            };
            let result = factory.probe_source_with_options(
                MediaSource::new(test_video_path),
                PlayerRuntimeOptions::default(),
            );
            let initializer = result.expect("macos host should support the macos desktop adapter");
            let capabilities = initializer.capabilities();
            let startup = initializer.startup();
            let video_decode = startup
                .video_decode
                .expect("macos initializer should report video decode diagnostics");
            assert_eq!(
                capabilities.adapter_id,
                MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
            );
            assert_eq!(
                capabilities.backend_family,
                PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
            );
            assert_eq!(video_decode.selected_mode, PlayerVideoDecodeMode::Software);
            assert_eq!(
                video_decode.hardware_backend.as_deref(),
                Some(VIDEOTOOLBOX_BACKEND_NAME)
            );
            assert!(video_decode.fallback_reason.is_some());
        } else {
            let result = factory.probe_source_with_options(
                MediaSource::new("fixture.mp4"),
                PlayerRuntimeOptions::default(),
            );
            let error = match result {
                Ok(_) => panic!("non-macos hosts should reject the macos adapter"),
                Err(error) => error,
            };
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    #[test]
    fn macos_host_factory_without_surface_prefers_software_path() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let Some(test_video_path) = test_video_path() else {
            eprintln!("skipping macOS fixture-backed test: test-video.mp4 is unavailable");
            return;
        };
        let factory = MacosHostPlayerRuntimeAdapterFactory;
        let initializer = factory
            .probe_source_with_options(
                MediaSource::new(test_video_path),
                PlayerRuntimeOptions::default(),
            )
            .expect("macos host factory probe should succeed");

        let capabilities = initializer.capabilities();
        let startup = initializer.startup();

        assert_eq!(factory.adapter_id(), MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID);
        assert_eq!(
            capabilities.backend_family,
            PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
        );
        assert_eq!(
            capabilities.adapter_id,
            MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
        );
        assert!(
            startup
                .video_decode
                .as_ref()
                .and_then(|info| info.fallback_reason.as_deref())
                .unwrap_or_default()
                .contains("requires an external video surface")
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_host_factory_with_surface_prefers_native_path() {
        let Some(test_video_path) = test_video_path() else {
            eprintln!("skipping macOS fixture-backed test: test-video.mp4 is unavailable");
            return;
        };
        let layer_handle = unsafe { player_macos_test_create_player_layer() };
        assert!(
            !layer_handle.is_null(),
            "test player layer handle should be created"
        );

        let factory = MacosHostPlayerRuntimeAdapterFactory;
        let options =
            PlayerRuntimeOptions::default().with_video_surface(PlayerVideoSurfaceTarget {
                kind: PlayerVideoSurfaceKind::PlayerLayer,
                handle: layer_handle as usize,
            });
        let initializer = factory
            .probe_source_with_options(MediaSource::new(test_video_path), options)
            .expect("macos host factory should prefer native when a valid surface exists");

        let capabilities = initializer.capabilities();
        let bootstrap = initializer
            .initialize()
            .expect("native-backed host initializer should initialize");

        assert_eq!(
            capabilities.backend_family,
            PlayerRuntimeAdapterBackendFamily::NativeMacos
        );
        assert_eq!(
            capabilities.adapter_id,
            MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
        );
        assert_eq!(
            bootstrap.runtime.capabilities().backend_family,
            PlayerRuntimeAdapterBackendFamily::NativeMacos
        );

        unsafe {
            player_macos_test_release_object(layer_handle);
        }
    }

    #[test]
    fn host_strategy_initializer_falls_back_to_software_when_native_initialize_fails() {
        let native_factory = FakeStrategyFactory {
            capabilities: PlayerRuntimeAdapterCapabilities {
                adapter_id: MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID,
                backend_family: PlayerRuntimeAdapterBackendFamily::NativeMacos,
                supports_audio_output: true,
                supports_frame_output: false,
                supports_external_video_surface: true,
                supports_seek: true,
                supports_stop: true,
                supports_playback_rate: true,
                playback_rate_min: Some(0.5),
                playback_rate_max: Some(3.0),
                natural_playback_rate_max: Some(2.0),
                supports_hardware_decode: true,
                supports_streaming: true,
                supports_hdr: true,
            },
            media_info: media_info_with_codec("H264"),
            startup: startup_with_video_decode(PlayerVideoDecodeInfo {
                selected_mode: PlayerVideoDecodeMode::Hardware,
                hardware_available: true,
                hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
                fallback_reason: None,
            }),
            initialize_error: Some(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "native init failed",
            )),
            advance_error: None,
        };
        let software_factory = FakeStrategyFactory {
            capabilities: PlayerRuntimeAdapterCapabilities {
                adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
                supports_audio_output: true,
                supports_frame_output: true,
                supports_external_video_surface: false,
                supports_seek: true,
                supports_stop: true,
                supports_playback_rate: true,
                playback_rate_min: Some(0.5),
                playback_rate_max: Some(3.0),
                natural_playback_rate_max: Some(2.0),
                supports_hardware_decode: false,
                supports_streaming: true,
                supports_hdr: false,
            },
            media_info: media_info_with_codec("H264"),
            startup: startup_with_video_decode(PlayerVideoDecodeInfo {
                selected_mode: PlayerVideoDecodeMode::Software,
                hardware_available: true,
                hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
                fallback_reason: None,
            }),
            initialize_error: None,
            advance_error: None,
        };
        let options =
            PlayerRuntimeOptions::default().with_video_surface(PlayerVideoSurfaceTarget {
                kind: PlayerVideoSurfaceKind::PlayerLayer,
                handle: 0x1234,
            });
        let initializer = probe_macos_host_runtime_initializer_with_factories(
            MediaSource::new("fixture.mp4"),
            options,
            &native_factory,
            Arc::new(software_factory.clone()),
        )
        .expect("host strategy probe should succeed");

        assert_eq!(
            initializer.capabilities().backend_family,
            PlayerRuntimeAdapterBackendFamily::NativeMacos
        );

        let bootstrap = initializer
            .initialize()
            .expect("host strategy initialize should fall back to software");

        assert_eq!(
            bootstrap.runtime.capabilities().backend_family,
            PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
        );
        assert!(
            bootstrap
                .startup
                .video_decode
                .as_ref()
                .and_then(|info| info.fallback_reason.as_deref())
                .unwrap_or_default()
                .contains("native init failed")
        );
    }

    #[test]
    fn software_runtime_initializer_falls_back_when_native_frame_initialize_fails() {
        let native_inner = Box::new(FakeStrategyInitializer {
            capabilities: PlayerRuntimeAdapterCapabilities {
                adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
                supports_audio_output: true,
                supports_frame_output: false,
                supports_external_video_surface: true,
                supports_seek: true,
                supports_stop: true,
                supports_playback_rate: true,
                playback_rate_min: Some(0.5),
                playback_rate_max: Some(3.0),
                natural_playback_rate_max: Some(2.0),
                supports_hardware_decode: true,
                supports_streaming: true,
                supports_hdr: true,
            },
            media_info: media_info_with_codec("H264"),
            startup: startup_with_video_decode(PlayerVideoDecodeInfo {
                selected_mode: PlayerVideoDecodeMode::Hardware,
                hardware_available: true,
                hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
                fallback_reason: None,
            }),
            initialize_error: Some(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "native-frame init failed",
            )),
            advance_error: None,
        });
        let fallback_inner = Box::new(FakeStrategyInitializer {
            capabilities: PlayerRuntimeAdapterCapabilities {
                adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
                supports_audio_output: true,
                supports_frame_output: true,
                supports_external_video_surface: false,
                supports_seek: true,
                supports_stop: true,
                supports_playback_rate: true,
                playback_rate_min: Some(0.5),
                playback_rate_max: Some(3.0),
                natural_playback_rate_max: Some(2.0),
                supports_hardware_decode: false,
                supports_streaming: true,
                supports_hdr: false,
            },
            media_info: media_info_with_codec("H264"),
            startup: startup_with_video_decode(PlayerVideoDecodeInfo {
                selected_mode: PlayerVideoDecodeMode::Software,
                hardware_available: true,
                hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
                fallback_reason: Some("software fallback ready".to_owned()),
            }),
            initialize_error: None,
            advance_error: None,
        });
        let diagnostics = MacosRuntimeDiagnostics {
            video_decode: macos_native_frame_decoder_video_decode_info(Some("fixture-native")),
            plugin_diagnostics: Vec::new(),
            has_video_surface: true,
        };
        let fallback_diagnostics = MacosRuntimeDiagnostics {
            video_decode: startup_with_video_decode(PlayerVideoDecodeInfo {
                selected_mode: PlayerVideoDecodeMode::Software,
                hardware_available: true,
                hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
                fallback_reason: Some("software fallback ready".to_owned()),
            })
            .video_decode
            .expect("fallback video decode"),
            plugin_diagnostics: Vec::new(),
            has_video_surface: false,
        };

        let initializer = Box::new(MacosRuntimeAdapterInitializer {
            inner: native_inner,
            diagnostics,
            fallback: Some(MacosRuntimeAdapterFallback {
                inner: fallback_inner,
                diagnostics: fallback_diagnostics,
                fallback_reason:
                    "native-frame decoder plugin initialization failed; selected FFmpeg software path"
                        .to_owned(),
            }),
            runtime_fallback: None,
        });

        let bootstrap = initializer
            .initialize()
            .expect("software runtime initializer should fall back");

        assert_eq!(
            bootstrap.runtime.capabilities().backend_family,
            PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
        );
        assert!(bootstrap.runtime.capabilities().supports_frame_output);
        assert!(
            !bootstrap
                .runtime
                .capabilities()
                .supports_external_video_surface
        );
        assert!(
            bootstrap
                .startup
                .video_decode
                .as_ref()
                .and_then(|info| info.fallback_reason.as_deref())
                .unwrap_or_default()
                .contains("native-frame init failed")
        );
    }

    #[test]
    fn runtime_advance_backend_failure_falls_back_to_software_runtime() {
        let native_runtime = Box::new(FakeStrategyRuntime {
            capabilities: PlayerRuntimeAdapterCapabilities {
                adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
                supports_audio_output: true,
                supports_frame_output: false,
                supports_external_video_surface: true,
                supports_seek: true,
                supports_stop: true,
                supports_playback_rate: true,
                playback_rate_min: Some(0.5),
                playback_rate_max: Some(3.0),
                natural_playback_rate_max: Some(2.0),
                supports_hardware_decode: true,
                supports_streaming: true,
                supports_hdr: true,
            },
            media_info: media_info_with_codec("H264"),
            playback_rate: 1.5,
            progress: PlaybackProgress::new(Duration::from_secs(5), Some(Duration::from_secs(30))),
            state: PresentationState::Playing,
            events: VecDeque::new(),
            advance_error: Some(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "forced presenter failure",
            )),
            dispatch_error: None,
        });
        let fallback_source = MediaSource::new("fixture.mp4");
        let fallback_options = PlayerRuntimeOptions::default();
        let adapter = MacosRuntimeAdapter {
            inner: native_runtime,
            video_decode: PlayerVideoDecodeInfo {
                selected_mode: PlayerVideoDecodeMode::Hardware,
                hardware_available: true,
                hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
                fallback_reason: None,
            },
            has_video_surface: true,
            runtime_fallback: Some(MacosRuntimeActiveFallback {
                source: fallback_source.clone(),
                options: fallback_options.clone(),
                fallback_reason:
                    "native-frame runtime failed during playback; selected FFmpeg software path"
                        .to_owned(),
            }),
            pending_runtime_fallback_events: VecDeque::new(),
        };
        let mut adapter = adapter;

        let fallback = adapter
            .runtime_fallback
            .clone()
            .expect("runtime fallback config should exist");
        adapter
            .activate_runtime_fallback_with(
                "forced presenter failure",
                fallback,
                |_source, _options| Ok(test_fallback_bootstrap()),
            )
            .expect("advance should fall back instead of failing");

        assert!(adapter.inner.capabilities().supports_frame_output);
        assert!(!adapter.inner.capabilities().supports_external_video_surface);
        assert_eq!(adapter.playback_rate(), 1.5);
        assert_eq!(adapter.progress().position(), Duration::from_secs(5));
        assert_eq!(adapter.presentation_state(), PresentationState::Playing);
        let events = adapter.drain_events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, PlayerRuntimeEvent::Error(_)))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            PlayerRuntimeEvent::VideoSurfaceChanged { attached: false }
        )));
        assert!(
            adapter
                .video_decode
                .fallback_reason
                .as_deref()
                .unwrap_or_default()
                .contains("forced presenter failure")
        );
    }

    #[test]
    fn runtime_dispatch_seek_backend_failure_falls_back_to_software_runtime() {
        let native_runtime = Box::new(FakeStrategyRuntime {
            capabilities: PlayerRuntimeAdapterCapabilities {
                adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
                supports_audio_output: true,
                supports_frame_output: false,
                supports_external_video_surface: true,
                supports_seek: true,
                supports_stop: true,
                supports_playback_rate: true,
                playback_rate_min: Some(0.5),
                playback_rate_max: Some(3.0),
                natural_playback_rate_max: Some(2.0),
                supports_hardware_decode: true,
                supports_streaming: true,
                supports_hdr: true,
            },
            media_info: media_info_with_codec("H264"),
            playback_rate: 1.25,
            progress: PlaybackProgress::new(Duration::from_secs(2), Some(Duration::from_secs(30))),
            state: PresentationState::Playing,
            events: VecDeque::new(),
            advance_error: None,
            dispatch_error: Some(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "forced seek failure",
            )),
        });
        let mut adapter = MacosRuntimeAdapter {
            inner: native_runtime,
            video_decode: PlayerVideoDecodeInfo {
                selected_mode: PlayerVideoDecodeMode::Hardware,
                hardware_available: true,
                hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
                fallback_reason: None,
            },
            has_video_surface: true,
            runtime_fallback: Some(MacosRuntimeActiveFallback {
                source: MediaSource::new("fixture.mp4"),
                options: PlayerRuntimeOptions::default(),
                fallback_reason:
                    "native-frame runtime failed during playback; selected FFmpeg software path"
                        .to_owned(),
            }),
            pending_runtime_fallback_events: VecDeque::new(),
        };
        let fallback = adapter
            .runtime_fallback
            .take()
            .expect("runtime fallback config should exist");
        let result = adapter
            .activate_runtime_fallback_with("forced seek failure", fallback, |_source, _options| {
                Ok(test_fallback_bootstrap())
            })
            .and_then(|()| {
                adapter.dispatch(PlayerRuntimeCommand::SeekTo {
                    position: Duration::from_secs(7),
                })
            })
            .expect("dispatch should succeed after fallback");

        assert!(result.applied);
        assert!(adapter.inner.capabilities().supports_frame_output);
        assert!(!adapter.inner.capabilities().supports_external_video_surface);
        assert_eq!(adapter.progress().position(), Duration::from_secs(7));
        assert_eq!(adapter.playback_rate(), 1.25);
        assert_eq!(adapter.presentation_state(), PresentationState::Playing);
    }

    #[test]
    fn runtime_dispatch_play_and_rate_backend_failure_fall_back_to_software_runtime() {
        for command in [
            PlayerRuntimeCommand::Play,
            PlayerRuntimeCommand::SetPlaybackRate { rate: 1.75 },
        ] {
            let mut adapter = MacosRuntimeAdapter {
                inner: Box::new(FakeStrategyRuntime {
                    capabilities: PlayerRuntimeAdapterCapabilities {
                        adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                        backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
                        supports_audio_output: true,
                        supports_frame_output: false,
                        supports_external_video_surface: true,
                        supports_seek: true,
                        supports_stop: true,
                        supports_playback_rate: true,
                        playback_rate_min: Some(0.5),
                        playback_rate_max: Some(3.0),
                        natural_playback_rate_max: Some(2.0),
                        supports_hardware_decode: true,
                        supports_streaming: true,
                        supports_hdr: true,
                    },
                    media_info: media_info_with_codec("H264"),
                    playback_rate: 1.25,
                    progress: PlaybackProgress::new(
                        Duration::from_secs(2),
                        Some(Duration::from_secs(30)),
                    ),
                    state: PresentationState::Paused,
                    events: VecDeque::new(),
                    advance_error: None,
                    dispatch_error: Some(PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::BackendFailure,
                        match command {
                            PlayerRuntimeCommand::Play => "forced play failure",
                            PlayerRuntimeCommand::SetPlaybackRate { .. } => "forced rate failure",
                            _ => unreachable!(),
                        },
                    )),
                }),
                video_decode: PlayerVideoDecodeInfo {
                    selected_mode: PlayerVideoDecodeMode::Hardware,
                    hardware_available: true,
                    hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
                    fallback_reason: None,
                },
                has_video_surface: true,
                runtime_fallback: Some(MacosRuntimeActiveFallback {
                    source: MediaSource::new("fixture.mp4"),
                    options: PlayerRuntimeOptions::default(),
                    fallback_reason:
                        "native-frame runtime failed during playback; selected FFmpeg software path"
                            .to_owned(),
                }),
                pending_runtime_fallback_events: VecDeque::new(),
            };
            let fallback = adapter
                .runtime_fallback
                .take()
                .expect("runtime fallback config should exist");

            let result = adapter
                .activate_runtime_fallback_with(
                    match command {
                        PlayerRuntimeCommand::Play => "forced play failure",
                        PlayerRuntimeCommand::SetPlaybackRate { .. } => "forced rate failure",
                        _ => unreachable!(),
                    },
                    fallback,
                    |_source, _options| Ok(test_fallback_bootstrap()),
                )
                .and_then(|()| adapter.dispatch(command.clone()))
                .expect("dispatch should succeed after fallback");

            assert!(result.applied);
            assert!(adapter.inner.capabilities().supports_frame_output);
            assert!(!adapter.inner.capabilities().supports_external_video_surface);
        }
    }

    #[test]
    fn runtime_fallback_trigger_only_matches_expected_paths() {
        assert!(should_trigger_runtime_fallback_for_advance(
            &PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "failed to present decoded video frame"
            )
        ));
        assert!(should_trigger_runtime_fallback_for_advance(
            &PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "failed to present seeked video frame"
            )
        ));
        assert!(!should_trigger_runtime_fallback_for_advance(
            &PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "failed to decode audio stream"
            )
        ));
        assert!(should_trigger_runtime_fallback_for_advance(
            &PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "native-frame decoder state is poisoned"
            )
        ));
        assert!(!should_trigger_runtime_fallback_for_advance(
            &PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::SeekFailure,
                "failed to present decoded video frame"
            )
        ));
        assert!(should_trigger_runtime_fallback_for_command(
            &PlayerRuntimeCommand::SeekTo {
                position: Duration::from_secs(1)
            },
            &PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "forced seek failure"
            )
        ));
        assert!(should_trigger_runtime_fallback_for_command(
            &PlayerRuntimeCommand::Play,
            &PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "forced play failure"
            )
        ));
        assert!(should_trigger_runtime_fallback_for_command(
            &PlayerRuntimeCommand::SetPlaybackRate { rate: 1.5 },
            &PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "forced rate failure"
            )
        ));
        assert!(!should_trigger_runtime_fallback_for_command(
            &PlayerRuntimeCommand::Pause,
            &PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "forced pause failure"
            )
        ));
        assert!(!should_trigger_runtime_fallback_for_command(
            &PlayerRuntimeCommand::Stop,
            &PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "forced stop failure"
            )
        ));
    }

    #[test]
    fn runtime_dispatch_pause_and_stop_do_not_trigger_fallback() {
        for command in [PlayerRuntimeCommand::Pause, PlayerRuntimeCommand::Stop] {
            let mut adapter = MacosRuntimeAdapter {
                inner: Box::new(FakeStrategyRuntime {
                    capabilities: PlayerRuntimeAdapterCapabilities {
                        adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                        backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
                        supports_audio_output: true,
                        supports_frame_output: false,
                        supports_external_video_surface: true,
                        supports_seek: true,
                        supports_stop: true,
                        supports_playback_rate: true,
                        playback_rate_min: Some(0.5),
                        playback_rate_max: Some(3.0),
                        natural_playback_rate_max: Some(2.0),
                        supports_hardware_decode: true,
                        supports_streaming: true,
                        supports_hdr: true,
                    },
                    media_info: media_info_with_codec("H264"),
                    playback_rate: 1.0,
                    progress: PlaybackProgress::new(
                        Duration::from_secs(2),
                        Some(Duration::from_secs(30)),
                    ),
                    state: PresentationState::Playing,
                    events: VecDeque::new(),
                    advance_error: None,
                    dispatch_error: Some(PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::BackendFailure,
                        match command {
                            PlayerRuntimeCommand::Pause => "forced pause failure",
                            PlayerRuntimeCommand::Stop => "forced stop failure",
                            _ => unreachable!(),
                        },
                    )),
                }),
                video_decode: PlayerVideoDecodeInfo {
                    selected_mode: PlayerVideoDecodeMode::Hardware,
                    hardware_available: true,
                    hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
                    fallback_reason: None,
                },
                has_video_surface: true,
                runtime_fallback: Some(MacosRuntimeActiveFallback {
                    source: MediaSource::new("fixture.mp4"),
                    options: PlayerRuntimeOptions::default(),
                    fallback_reason:
                        "native-frame runtime failed during playback; selected FFmpeg software path"
                            .to_owned(),
                }),
                pending_runtime_fallback_events: VecDeque::new(),
            };

            let error = adapter
                .dispatch(command)
                .expect_err("pause/stop should not fallback");
            assert_eq!(error.code(), PlayerRuntimeErrorCode::BackendFailure);
            assert!(adapter.runtime_fallback.is_some());
            assert!(adapter.inner.capabilities().supports_external_video_surface);
        }
    }

    #[test]
    fn macos_video_decode_info_marks_h264_as_hardware_candidate() {
        let info = macos_video_decode_info(&media_info_with_codec("H264"));

        assert_eq!(info.selected_mode, PlayerVideoDecodeMode::Software);
        assert_eq!(
            info.hardware_backend.as_deref(),
            Some(VIDEOTOOLBOX_BACKEND_NAME)
        );
        assert!(info.fallback_reason.is_some());
    }

    #[test]
    fn macos_video_decode_info_marks_unknown_codec_as_software_only() {
        let info = macos_video_decode_info(&media_info_with_codec("VP8"));

        assert_eq!(info.selected_mode, PlayerVideoDecodeMode::Software);
        assert!(!info.hardware_available);
        assert_eq!(
            info.hardware_backend.as_deref(),
            Some(VIDEOTOOLBOX_BACKEND_NAME)
        );
        assert!(
            info.fallback_reason
                .as_deref()
                .unwrap_or_default()
                .contains("VP8")
        );
    }

    #[test]
    fn macos_video_decode_info_without_plugin_paths_keeps_fallback_clean() {
        let media_info = media_info_with_codec("fixture-video");
        let info = apply_decoder_plugin_diagnostics_to_video_decode(
            macos_video_decode_info(&media_info),
            &media_info,
            &PlayerRuntimeOptions::default(),
        );

        assert!(
            !info
                .fallback_reason
                .as_deref()
                .unwrap_or_default()
                .contains("decoder plugin")
        );
    }

    #[test]
    fn macos_video_decode_info_records_configured_decoder_plugin_paths() {
        let media_info = media_info_with_codec("fixture-video");
        let info = apply_decoder_plugin_diagnostics_to_video_decode(
            macos_video_decode_info(&media_info),
            &media_info,
            &PlayerRuntimeOptions::default()
                .with_decoder_plugin_library_paths([PathBuf::from("/tmp/missing-decoder-plugin")]),
        );

        assert!(
            info.fallback_reason
                .as_deref()
                .unwrap_or_default()
                .contains("decoder plugin paths configured")
        );
        let fallback = info.fallback_reason.as_deref().unwrap_or_default();
        assert!(fallback.contains("/tmp/missing-decoder-plugin"));
        assert!(!fallback.contains("failed to open plugin library"));
        assert!(!fallback.contains("dlopen"));
    }

    #[test]
    fn macos_startup_records_decoder_plugin_registry_diagnostics() {
        let media_info = media_info_with_codec("fixture-video");
        let startup = apply_decoder_plugin_diagnostics(
            startup_with_video_decode(macos_video_decode_info(&media_info)),
            &media_info,
            &PlayerRuntimeOptions::default()
                .with_decoder_plugin_library_paths([PathBuf::from("/tmp/missing-decoder-plugin")]),
        );

        assert_eq!(startup.plugin_diagnostics.len(), 1);
        assert_eq!(
            startup.plugin_diagnostics[0].status,
            PlayerPluginDiagnosticStatus::LoadFailed
        );
        assert!(
            startup.plugin_diagnostics[0]
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("failed to open plugin library")
        );
        assert!(
            startup
                .video_decode
                .as_ref()
                .and_then(|info| info.fallback_reason.as_deref())
                .unwrap_or_default()
                .contains("decoder plugin paths configured")
        );
    }

    #[test]
    #[ignore = "requires a built player-decoder-fixture shared library artifact"]
    fn macos_runtime_diagnostics_loads_real_decoder_fixture_library() {
        let Some(plugin_path) = std::env::var_os("VESPER_DECODER_PLUGIN_PATHS")
            .and_then(|paths| std::env::split_paths(&paths).next())
        else {
            eprintln!(
                "skipping decoder fixture diagnostics test: VESPER_DECODER_PLUGIN_PATHS is not set"
            );
            return;
        };
        if !plugin_path.is_file() {
            eprintln!(
                "skipping decoder fixture diagnostics test: plugin path is missing: {}",
                plugin_path.display()
            );
            return;
        }

        for codec in ["fixture-video", "H264", "HEVC"] {
            let media_info = media_info_with_codec(codec);
            let diagnostics = macos_runtime_diagnostics(
                &media_info,
                &PlayerRuntimeOptions::default()
                    .with_decoder_plugin_library_paths([plugin_path.clone()]),
            );

            assert_eq!(diagnostics.plugin_diagnostics.len(), 1);
            assert_eq!(
                diagnostics.plugin_diagnostics[0].status,
                PlayerPluginDiagnosticStatus::DecoderSupported
            );
            assert_eq!(
                diagnostics.plugin_diagnostics[0].plugin_name.as_deref(),
                Some("player-decoder-fixture")
            );
            let fallback = diagnostics
                .video_decode
                .fallback_reason
                .as_deref()
                .unwrap_or_default();
            assert!(fallback.contains(codec));
            assert!(fallback.contains("diagnostic-only"));
        }
    }

    #[test]
    #[ignore = "requires a built player-decoder-videotoolbox shared library artifact"]
    fn macos_runtime_diagnostics_loads_real_videotoolbox_decoder_library() {
        let Some(plugin_path) =
            std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
        else {
            eprintln!(
                "skipping VideoToolbox decoder diagnostics test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
            );
            return;
        };
        if !plugin_path.is_file() {
            eprintln!(
                "skipping VideoToolbox decoder diagnostics test: plugin path is missing: {}",
                plugin_path.display()
            );
            return;
        }

        for codec in ["H264", "HEVC"] {
            let media_info = media_info_with_codec(codec);
            let diagnostics = macos_runtime_diagnostics(
                &media_info,
                &PlayerRuntimeOptions::default()
                    .with_decoder_plugin_library_paths([plugin_path.clone()]),
            );

            assert_eq!(diagnostics.plugin_diagnostics.len(), 1);
            let diagnostic = &diagnostics.plugin_diagnostics[0];
            assert_eq!(
                diagnostic.status,
                PlayerPluginDiagnosticStatus::DecoderSupported
            );
            assert_eq!(
                diagnostic.plugin_name.as_deref(),
                Some("player-decoder-videotoolbox")
            );
            assert!(
                diagnostic
                    .decoder_capabilities
                    .as_ref()
                    .is_some_and(|capabilities| capabilities.supports_native_frame_output)
            );
            let fallback = diagnostics
                .video_decode
                .fallback_reason
                .as_deref()
                .unwrap_or_default();
            assert!(fallback.contains("player-decoder-videotoolbox native-frame"));
        }
    }

    #[test]
    #[ignore = "requires a built player-decoder-videotoolbox shared library and a local H264/HEVC source"]
    fn macos_videotoolbox_decoder_decodes_ffmpeg_packets_headless() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let Some(plugin_path) =
            std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
        else {
            eprintln!(
                "skipping VideoToolbox packet decode test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
            );
            return;
        };
        if !plugin_path.is_file() {
            eprintln!(
                "skipping VideoToolbox packet decode test: plugin path is missing: {}",
                plugin_path.display()
            );
            return;
        }
        let Some(source) = videotoolbox_smoke_source_path() else {
            eprintln!(
                "skipping VideoToolbox packet decode test: no local H264/HEVC smoke source found"
            );
            return;
        };

        let backend = FfmpegBackend::new().expect("FFmpeg should initialize");
        let mut packet_source = backend
            .open_video_packet_source(MediaSource::new(source.clone()))
            .unwrap_or_else(|error| panic!("failed to open packet source `{source}`: {error}"));
        let stream_info = packet_source.stream_info().clone();
        let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
            panic!(
                "failed to load VideoToolbox decoder plugin `{}`: {error}",
                plugin_path.display()
            )
        });
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("VideoToolbox plugin should export a native decoder factory");
        if !factory
            .capabilities()
            .supports_codec(&stream_info.codec, DecoderMediaKind::Video)
        {
            eprintln!(
                "skipping VideoToolbox packet decode test: source codec {} is not supported",
                stream_info.codec
            );
            return;
        }

        let mut session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: stream_info.codec.clone(),
                media_kind: DecoderMediaKind::Video,
                extradata: stream_info.extradata.clone(),
                width: stream_info.width,
                height: stream_info.height,
                prefer_hardware: true,
                require_cpu_output: false,
                ..DecoderSessionConfig::default()
            })
            .expect("VideoToolbox native session should open");

        let mut submitted_packets = 0usize;
        let mut decoded_frames = 0usize;
        while submitted_packets < 120 && decoded_frames == 0 {
            let Some(packet) = packet_source
                .next_packet()
                .expect("packet demux should succeed")
            else {
                break;
            };
            submitted_packets += 1;
            let send_result = session
                .send_packet(
                    &DecoderPacket {
                        pts_us: packet.pts_us,
                        dts_us: packet.dts_us,
                        duration_us: packet.duration_us,
                        stream_index: packet.stream_index,
                        key_frame: packet.key_frame,
                        discontinuity: packet.discontinuity,
                        end_of_stream: false,
                    },
                    &packet.data,
                )
                .expect("VideoToolbox should accept compressed packet");
            if !send_result.accepted {
                continue;
            }

            loop {
                match session
                    .receive_native_frame()
                    .expect("VideoToolbox frame receive should succeed")
                {
                    DecoderReceiveNativeFrameOutput::Frame(frame) => {
                        assert_eq!(
                            frame.metadata.handle_kind,
                            DecoderNativeHandleKind::CvPixelBuffer
                        );
                        assert!(frame.handle != 0);
                        assert!(frame.metadata.width > 0);
                        assert!(frame.metadata.height > 0);
                        session
                            .release_native_frame(frame)
                            .expect("native frame release should succeed");
                        decoded_frames += 1;
                    }
                    DecoderReceiveNativeFrameOutput::NeedMoreInput => break,
                    DecoderReceiveNativeFrameOutput::Eof => break,
                }
            }
        }

        assert!(
            decoded_frames > 0,
            "VideoToolbox did not produce a CVPixelBuffer after {submitted_packets} packets from {source}"
        );
    }

    #[test]
    #[ignore = "requires a built player-decoder-videotoolbox shared library and a local H264/HEVC source"]
    #[cfg(target_os = "macos")]
    fn macos_native_frame_decoder_plugin_runtime_probes_with_surface() {
        let Some(plugin_path) =
            std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
        else {
            eprintln!(
                "skipping native-frame runtime test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
            );
            return;
        };
        if !plugin_path.is_file() {
            eprintln!(
                "skipping native-frame runtime test: plugin path is missing: {}",
                plugin_path.display()
            );
            return;
        }
        let Some(source) = videotoolbox_smoke_source_path() else {
            eprintln!("skipping native-frame runtime test: no local H264/HEVC smoke source found");
            return;
        };

        let layer_handle = unsafe { player_macos_test_create_player_layer() };
        assert!(
            !layer_handle.is_null(),
            "test player layer handle should be created"
        );

        let options = PlayerRuntimeOptions::default()
            .with_video_surface(PlayerVideoSurfaceTarget {
                kind: PlayerVideoSurfaceKind::PlayerLayer,
                handle: layer_handle as usize,
            })
            .with_decoder_plugin_library_paths([plugin_path])
            .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame);
        let initializer = PlayerRuntimeInitializer::probe_source_with_factory(
            MediaSource::new(source),
            options,
            macos_runtime_adapter_factory(),
        )
        .expect("native-frame plugin runtime should probe");

        assert!(initializer.capabilities().supports_external_video_surface);
        assert!(!initializer.capabilities().supports_frame_output);
        assert!(initializer.capabilities().supports_hardware_decode);
        assert_eq!(
            initializer
                .startup()
                .video_decode
                .as_ref()
                .map(|decode| decode.selected_mode),
            Some(PlayerVideoDecodeMode::Hardware)
        );

        unsafe {
            player_macos_test_release_object(layer_handle);
        }
    }

    #[test]
    #[ignore = "requires a built player-decoder-videotoolbox shared library and a local H264/HEVC source"]
    #[cfg(target_os = "macos")]
    fn macos_native_frame_runtime_reopens_as_software_after_presenter_failure() {
        let Some(plugin_path) =
            std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
        else {
            eprintln!(
                "skipping native-frame reopen test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
            );
            return;
        };
        if !plugin_path.is_file() {
            eprintln!(
                "skipping native-frame reopen test: plugin path is missing: {}",
                plugin_path.display()
            );
            return;
        }
        let Some(source) = videotoolbox_smoke_source_path() else {
            eprintln!("skipping native-frame reopen test: no local H264/HEVC smoke source found");
            return;
        };

        let layer_handle = unsafe { player_macos_test_create_player_layer() };
        assert!(
            !layer_handle.is_null(),
            "test player layer handle should be created"
        );

        unsafe {
            std::env::set_var("VESPER_MACOS_TEST_FORCE_PRESENTER_FAILURE", "1");
        }
        let options = PlayerRuntimeOptions::default()
            .with_video_surface(PlayerVideoSurfaceTarget {
                kind: PlayerVideoSurfaceKind::PlayerLayer,
                handle: layer_handle as usize,
            })
            .with_decoder_plugin_library_paths([plugin_path])
            .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame);
        let bootstrap = open_macos_software_runtime_source_with_options_and_interrupt(
            MediaSource::new(source),
            options,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("native-frame runtime open should succeed before presenter failure fallback");
        let mut runtime = bootstrap.runtime;
        let initial_rate = runtime.playback_rate();

        let _ = runtime
            .dispatch(PlayerRuntimeCommand::Play)
            .expect("play should succeed");
        let _ = runtime
            .dispatch(PlayerRuntimeCommand::SetPlaybackRate { rate: 1.25 })
            .expect("set playback rate should succeed before fallback");

        for _ in 0..240 {
            let _ = runtime
                .advance()
                .expect("advance should fallback instead of failing");
            if runtime.capabilities().supports_frame_output
                && !runtime.capabilities().supports_external_video_surface
            {
                break;
            }
        }

        assert!(runtime.capabilities().supports_frame_output);
        assert!(!runtime.capabilities().supports_external_video_surface);
        assert_eq!(runtime.presentation_state(), PresentationState::Playing);
        assert!(runtime.playback_rate() >= initial_rate);
        let resume_position = runtime.progress().position();
        let _ = runtime
            .dispatch(PlayerRuntimeCommand::SeekTo {
                position: resume_position,
            })
            .expect("seek should continue to work after fallback");
        let _ = runtime
            .dispatch(PlayerRuntimeCommand::SetPlaybackRate { rate: 1.0 })
            .expect("rate change should continue to work after fallback");
        let _ = runtime
            .dispatch(PlayerRuntimeCommand::Play)
            .expect("play should remain valid after fallback");
        let mut saw_surface_detached = false;
        let mut saw_runtime_fallback_error = false;
        for event in runtime.drain_events() {
            if matches!(
                event,
                PlayerRuntimeEvent::VideoSurfaceChanged { attached: false }
            ) {
                saw_surface_detached = true;
            }
            if let PlayerRuntimeEvent::Error(error) = event
                && error.message().contains("runtime fallback activated")
            {
                saw_runtime_fallback_error = true;
            }
        }
        assert!(
            saw_surface_detached,
            "expected native surface detachment event after fallback"
        );
        assert!(
            saw_runtime_fallback_error,
            "expected explicit runtime fallback error event after fallback"
        );
        unsafe {
            std::env::remove_var("VESPER_MACOS_TEST_FORCE_PRESENTER_FAILURE");
        }

        unsafe {
            player_macos_test_release_object(layer_handle);
        }
    }

    #[test]
    fn macos_software_direct_open_records_decoder_plugin_registry_diagnostics() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let Some(test_video_path) = test_video_path() else {
            eprintln!("skipping macOS fixture-backed test: test-video.mp4 is unavailable");
            return;
        };
        let bootstrap = open_macos_software_runtime_source_with_options_and_interrupt(
            MediaSource::new(test_video_path),
            PlayerRuntimeOptions::default()
                .with_decoder_plugin_library_paths([PathBuf::from("/tmp/missing-decoder-plugin")]),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("macos software direct open should succeed");

        assert_eq!(bootstrap.startup.plugin_diagnostics.len(), 1);
        assert_eq!(
            bootstrap.startup.plugin_diagnostics[0].status,
            PlayerPluginDiagnosticStatus::LoadFailed
        );
        assert!(
            bootstrap
                .startup
                .video_decode
                .as_ref()
                .and_then(|info| info.fallback_reason.as_deref())
                .unwrap_or_default()
                .contains("decoder plugin paths configured")
        );
    }

    #[test]
    fn macos_decoder_plugin_registry_reports_supported_candidate_as_diagnostic_only() {
        let media_info = media_info_with_codec("fixture-video");
        let registry = PluginRegistry::from_records(vec![decoder_plugin_record(
            PluginDiagnosticStatus::DecoderSupported,
            "fixture-video",
            "fixture-decoder advertises Video fixture-video support",
        )]);
        let info = apply_decoder_plugin_registry_to_video_decode(
            macos_video_decode_info(&media_info),
            &media_info,
            &registry,
        );

        assert_eq!(info.selected_mode, PlayerVideoDecodeMode::Software);
        assert!(
            info.fallback_reason
                .as_deref()
                .unwrap_or_default()
                .contains("diagnostic-only")
        );
        assert!(
            info.fallback_reason
                .as_deref()
                .unwrap_or_default()
                .contains("fixture-decoder")
        );
    }

    #[test]
    fn macos_decoder_plugin_registry_labels_native_frame_candidates() {
        let media_info = media_info_with_codec("fixture-video");
        let registry = PluginRegistry::from_records(vec![decoder_native_plugin_record(
            PluginDiagnosticStatus::DecoderSupported,
            "fixture-video",
            "fixture-decoder advertises Video fixture-video support with native-frame output",
        )]);
        let info = apply_decoder_plugin_registry_to_video_decode(
            macos_video_decode_info(&media_info),
            &media_info,
            &registry,
        );

        assert_eq!(info.selected_mode, PlayerVideoDecodeMode::Software);
        let fallback = info.fallback_reason.as_deref().unwrap_or_default();
        assert!(fallback.contains("decoder plugin found 1/1 candidate(s)"));
        assert!(fallback.contains("fixture-decoder native-frame"));
        assert!(fallback.contains("diagnostic-only"));
    }

    #[test]
    fn macos_decoder_plugin_registry_mismatch_does_not_change_decode_mode() {
        let media_info = media_info_with_codec("fixture-video");
        let original = macos_video_decode_info(&media_info);
        let registry = PluginRegistry::from_records(vec![decoder_plugin_record(
            PluginDiagnosticStatus::DecoderUnsupported,
            "other-video",
            "fixture-decoder does not advertise Video fixture-video support",
        )]);
        let info =
            apply_decoder_plugin_registry_to_video_decode(original.clone(), &media_info, &registry);

        assert_eq!(info.selected_mode, original.selected_mode);
        assert!(
            info.fallback_reason
                .as_deref()
                .unwrap_or_default()
                .contains("0/1 supported")
        );
    }

    #[test]
    fn macos_decoder_plugin_paths_do_not_match_when_source_has_no_video_stream() {
        let media_info = media_info_without_video();
        let startup = apply_decoder_plugin_diagnostics(
            startup_with_video_decode(macos_video_decode_info(&media_info)),
            &media_info,
            &PlayerRuntimeOptions::default()
                .with_decoder_plugin_library_paths([PathBuf::from("/tmp/missing-decoder-plugin")]),
        );

        assert!(startup.plugin_diagnostics.is_empty());
        let fallback = startup
            .video_decode
            .as_ref()
            .and_then(|info| info.fallback_reason.as_deref())
            .unwrap_or_default();
        assert!(fallback.contains("source does not expose a decodable video stream"));
        assert!(!fallback.contains("decoder plugin"));
    }

    #[test]
    fn macos_host_runtime_without_surface_falls_back_to_software() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let Some(test_video_path) = test_video_path() else {
            eprintln!("skipping macOS fixture-backed test: test-video.mp4 is unavailable");
            return;
        };
        let bootstrap = open_macos_host_runtime_source_with_options(
            MediaSource::new(test_video_path),
            PlayerRuntimeOptions::default(),
        )
        .expect("host runtime should fall back to software without a video surface");

        assert_eq!(
            bootstrap.runtime.adapter_id(),
            MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
        );
        assert!(
            bootstrap
                .startup
                .video_decode
                .as_ref()
                .and_then(|info| info.fallback_reason.as_deref())
                .unwrap_or_default()
                .contains("requires an external video surface")
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_host_runtime_with_surface_prefers_native() {
        let Some(test_video_path) = test_video_path() else {
            eprintln!("skipping macOS fixture-backed test: test-video.mp4 is unavailable");
            return;
        };
        let layer_handle = unsafe { player_macos_test_create_player_layer() };
        assert!(
            !layer_handle.is_null(),
            "test player layer handle should be created"
        );

        let options =
            PlayerRuntimeOptions::default().with_video_surface(PlayerVideoSurfaceTarget {
                kind: PlayerVideoSurfaceKind::PlayerLayer,
                handle: layer_handle as usize,
            });
        let bootstrap =
            open_macos_host_runtime_source_with_options(MediaSource::new(test_video_path), options)
                .expect("host runtime should prefer native playback when a valid surface exists");

        assert_eq!(
            bootstrap.runtime.adapter_id(),
            MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
        );

        unsafe {
            player_macos_test_release_object(layer_handle);
        }
    }

    #[test]
    fn macos_host_runtime_probe_prefers_native_probe() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let Some(test_video_path) = test_video_path() else {
            eprintln!("skipping macOS fixture-backed test: test-video.mp4 is unavailable");
            return;
        };
        let probe = probe_macos_host_runtime_source_with_options(
            MediaSource::new(test_video_path),
            PlayerRuntimeOptions::default(),
        )
        .expect("host runtime probe should succeed");

        assert_eq!(probe.adapter_id, MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID);
        assert_eq!(
            probe.capabilities.backend_family,
            PlayerRuntimeAdapterBackendFamily::NativeMacos
        );
    }

    #[test]
    fn release_native_frame_tracking_decrements_outstanding_count() {
        let outstanding_frames = Arc::new(AtomicUsize::new(1));
        let mut session = FakeNativeDecoderSession::default();
        let frame = DecoderNativeFrame {
            metadata: DecoderNativeFrameMetadata {
                media_kind: DecoderMediaKind::Video,
                format: player_plugin::DecoderFrameFormat::Nv12,
                codec: "h264".to_owned(),
                pts_us: Some(1_000),
                duration_us: Some(33_000),
                width: 1920,
                height: 1080,
                handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
            },
            handle: 7,
        };

        release_native_frame_with_counter(&mut session, outstanding_frames.as_ref(), frame)
            .expect("release should succeed");

        assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
        assert_eq!(session.released_handles, 1);
    }

    #[test]
    fn present_failure_still_releases_native_frame() {
        let outstanding_frames = Arc::new(AtomicUsize::new(1));
        let mut session = FakeNativeDecoderSession::default();
        let frame = DecoderNativeFrame {
            metadata: DecoderNativeFrameMetadata {
                media_kind: DecoderMediaKind::Video,
                format: player_plugin::DecoderFrameFormat::Nv12,
                codec: "h264".to_owned(),
                pts_us: Some(2_000),
                duration_us: Some(33_000),
                width: 1280,
                height: 720,
                handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
            },
            handle: 11,
        };

        let error = present_and_release_native_frame_with_presenter(
            &mut session,
            outstanding_frames.as_ref(),
            frame,
            |_handle| Err("forced presenter failure".to_owned()),
        )
        .expect_err("present failure should bubble up");

        assert!(error.to_string().contains("forced presenter failure"));
        assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
        assert_eq!(session.released_handles, 1);
    }

    #[test]
    fn stale_presentation_epoch_releases_frame_without_presenting() {
        let outstanding_frames = Arc::new(AtomicUsize::new(1));
        let present_called = Arc::new(AtomicBool::new(false));
        let mut session = FakeNativeDecoderSession::default();
        let frame = DecoderNativeFrame {
            metadata: DecoderNativeFrameMetadata {
                media_kind: DecoderMediaKind::Video,
                format: player_plugin::DecoderFrameFormat::Nv12,
                codec: "h264".to_owned(),
                pts_us: Some(3_000),
                duration_us: Some(33_000),
                width: 640,
                height: 360,
                handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
            },
            handle: 13,
        };

        let result = present_if_current_epoch_and_release(
            &mut session,
            outstanding_frames.as_ref(),
            2,
            1,
            frame,
            |_frame| {
                present_called.store(true, Ordering::SeqCst);
                Ok(())
            },
        );

        assert!(result.is_ok());
        assert!(!present_called.load(Ordering::SeqCst));
        assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
        assert_eq!(
            session.session_info().decoder_name.as_deref(),
            Some("released=1")
        );
    }

    fn media_info_with_codec(codec: &str) -> PlayerMediaInfo {
        PlayerMediaInfo {
            source_uri: "fixture.mp4".to_owned(),
            source_kind: player_runtime::MediaSourceKind::Local,
            source_protocol: player_runtime::MediaSourceProtocol::File,
            duration: None,
            bit_rate: None,
            audio_streams: 1,
            video_streams: 1,
            best_video: Some(PlayerVideoInfo {
                codec: codec.to_owned(),
                width: 960,
                height: 432,
                frame_rate: Some(30.0),
            }),
            best_audio: None,
            track_catalog: Default::default(),
            track_selection: Default::default(),
        }
    }

    fn media_info_without_video() -> PlayerMediaInfo {
        PlayerMediaInfo {
            video_streams: 0,
            best_video: None,
            ..media_info_with_codec("fixture-video")
        }
    }

    fn startup_with_video_decode(video_decode: PlayerVideoDecodeInfo) -> PlayerRuntimeStartup {
        PlayerRuntimeStartup {
            ffmpeg_initialized: false,
            audio_output: None,
            decoded_audio: None,
            video_decode: Some(video_decode),
            plugin_diagnostics: Vec::new(),
        }
    }

    fn decoder_plugin_record(
        status: PluginDiagnosticStatus,
        codec: &str,
        message: &str,
    ) -> PluginDiagnosticRecord {
        decoder_plugin_record_with_native_frame_output(status, codec, message, false)
    }

    fn decoder_native_plugin_record(
        status: PluginDiagnosticStatus,
        codec: &str,
        message: &str,
    ) -> PluginDiagnosticRecord {
        decoder_plugin_record_with_native_frame_output(status, codec, message, true)
    }

    fn decoder_plugin_record_with_native_frame_output(
        status: PluginDiagnosticStatus,
        codec: &str,
        message: &str,
        supports_native_frame_output: bool,
    ) -> PluginDiagnosticRecord {
        PluginDiagnosticRecord {
            path: PathBuf::from("fixture-decoder"),
            status,
            plugin_name: Some("fixture-decoder".to_owned()),
            plugin_kind: Some(VesperPluginKind::Decoder),
            decoder_capabilities: Some(DecoderPluginCapabilitySummary {
                typed_codecs: vec![DecoderPluginCodecSummary {
                    codec: codec.to_owned(),
                    media_kind: DecoderMediaKind::Video,
                }],
                codecs: vec![format!("Video:{codec}")],
                supports_native_frame_output,
                supports_hardware_decode: false,
                supports_cpu_video_frames: !supports_native_frame_output,
                supports_audio_frames: false,
                supports_gpu_handles: supports_native_frame_output,
                supports_flush: true,
                supports_drain: true,
                max_sessions: Some(1),
            }),
            message: Some(message.to_owned()),
        }
    }

    fn test_video_path() -> Option<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../test-video.mp4");
        path.canonicalize()
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
    }

    fn videotoolbox_smoke_source_path() -> Option<String> {
        if let Some(source) = std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_SOURCE")
            .map(|source| source.to_string_lossy().trim().to_owned())
            .filter(|source| !source.is_empty())
        {
            return Some(source);
        }

        [PathBuf::from("/Users/ikaros/Downloads/demo.mp4")]
            .into_iter()
            .find(|path| path.is_file())
            .map(|path| path.to_string_lossy().into_owned())
            .or_else(test_video_path)
    }

    fn test_fallback_bootstrap() -> PlayerRuntimeAdapterBootstrap {
        PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(FakeStrategyRuntime {
                capabilities: PlayerRuntimeAdapterCapabilities {
                    adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                    backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
                    supports_audio_output: true,
                    supports_frame_output: true,
                    supports_external_video_surface: false,
                    supports_seek: true,
                    supports_stop: true,
                    supports_playback_rate: true,
                    playback_rate_min: Some(0.5),
                    playback_rate_max: Some(3.0),
                    natural_playback_rate_max: Some(2.0),
                    supports_hardware_decode: false,
                    supports_streaming: true,
                    supports_hdr: false,
                },
                media_info: media_info_with_codec("H264"),
                playback_rate: 1.0,
                progress: PlaybackProgress::new(Duration::ZERO, Some(Duration::from_secs(30))),
                state: PresentationState::Ready,
                events: VecDeque::new(),
                advance_error: None,
                dispatch_error: None,
            }),
            initial_frame: None,
            startup: startup_with_video_decode(PlayerVideoDecodeInfo {
                selected_mode: PlayerVideoDecodeMode::Software,
                hardware_available: true,
                hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
                fallback_reason: Some("software fallback ready".to_owned()),
            }),
        }
    }

    #[derive(Clone)]
    struct FakeStrategyFactory {
        capabilities: PlayerRuntimeAdapterCapabilities,
        media_info: PlayerMediaInfo,
        startup: PlayerRuntimeStartup,
        initialize_error: Option<PlayerRuntimeError>,
        advance_error: Option<PlayerRuntimeError>,
    }

    impl PlayerRuntimeAdapterFactory for FakeStrategyFactory {
        fn adapter_id(&self) -> &'static str {
            self.capabilities.adapter_id
        }

        fn probe_source_with_options(
            &self,
            _source: MediaSource,
            _options: PlayerRuntimeOptions,
        ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
            Ok(Box::new(FakeStrategyInitializer {
                capabilities: self.capabilities.clone(),
                media_info: self.media_info.clone(),
                startup: self.startup.clone(),
                initialize_error: self.initialize_error.clone(),
                advance_error: self.advance_error.clone(),
            }))
        }
    }

    impl super::MacosHostFallbackFactory for FakeStrategyFactory {
        fn probe_source_with_options(
            &self,
            source: MediaSource,
            options: PlayerRuntimeOptions,
        ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
            <Self as PlayerRuntimeAdapterFactory>::probe_source_with_options(self, source, options)
        }
    }

    struct FakeStrategyInitializer {
        capabilities: PlayerRuntimeAdapterCapabilities,
        media_info: PlayerMediaInfo,
        startup: PlayerRuntimeStartup,
        initialize_error: Option<PlayerRuntimeError>,
        advance_error: Option<PlayerRuntimeError>,
    }

    impl PlayerRuntimeAdapterInitializer for FakeStrategyInitializer {
        fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
            self.capabilities.clone()
        }

        fn media_info(&self) -> PlayerMediaInfo {
            self.media_info.clone()
        }

        fn startup(&self) -> PlayerRuntimeStartup {
            self.startup.clone()
        }

        fn initialize(self: Box<Self>) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
            let Self {
                capabilities,
                media_info,
                startup,
                initialize_error,
                advance_error,
            } = *self;

            if let Some(error) = initialize_error {
                return Err(error);
            }

            Ok(PlayerRuntimeAdapterBootstrap {
                runtime: Box::new(FakeStrategyRuntime {
                    capabilities,
                    media_info,
                    playback_rate: 1.0,
                    progress: PlaybackProgress::new(Duration::ZERO, None),
                    state: PresentationState::Ready,
                    events: VecDeque::new(),
                    advance_error,
                    dispatch_error: None,
                }),
                initial_frame: None,
                startup,
            })
        }
    }

    struct FakeStrategyRuntime {
        capabilities: PlayerRuntimeAdapterCapabilities,
        media_info: PlayerMediaInfo,
        playback_rate: f32,
        progress: PlaybackProgress,
        state: PresentationState,
        events: VecDeque<PlayerRuntimeEvent>,
        advance_error: Option<PlayerRuntimeError>,
        dispatch_error: Option<PlayerRuntimeError>,
    }

    impl PlayerRuntimeAdapter for FakeStrategyRuntime {
        fn source_uri(&self) -> &str {
            &self.media_info.source_uri
        }

        fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
            self.capabilities.clone()
        }

        fn media_info(&self) -> &PlayerMediaInfo {
            &self.media_info
        }

        fn presentation_state(&self) -> PresentationState {
            self.state
        }

        fn playback_rate(&self) -> f32 {
            self.playback_rate
        }

        fn progress(&self) -> PlaybackProgress {
            self.progress
        }

        fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
            self.events.drain(..).collect()
        }

        fn dispatch(
            &mut self,
            command: PlayerRuntimeCommand,
        ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
            if let Some(error) = self.dispatch_error.take() {
                return Err(error);
            }
            match command {
                PlayerRuntimeCommand::Play => {
                    self.state = PresentationState::Playing;
                }
                PlayerRuntimeCommand::SeekTo { position } => {
                    self.progress = PlaybackProgress::new(position, self.progress.duration());
                }
                PlayerRuntimeCommand::SetPlaybackRate { rate } => {
                    self.playback_rate = rate;
                }
                _ => {}
            }
            Ok(PlayerRuntimeCommandResult {
                applied: true,
                frame: None,
                snapshot: self.snapshot(),
            })
        }

        fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
            if let Some(error) = self.advance_error.take() {
                return Err(error);
            }
            Ok(None)
        }

        fn next_deadline(&self) -> Option<Instant> {
            None
        }
    }

    #[derive(Default)]
    struct FakeNativeDecoderSession {
        released_handles: usize,
    }

    impl NativeDecoderSession for FakeNativeDecoderSession {
        fn session_info(&self) -> DecoderSessionInfo {
            DecoderSessionInfo {
                decoder_name: Some(format!("released={}", self.released_handles)),
                selected_hardware_backend: None,
                output_format: None,
            }
        }

        fn send_packet(
            &mut self,
            _packet: &DecoderPacket,
            _data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            Ok(DecoderPacketResult { accepted: true })
        }

        fn receive_native_frame(
            &mut self,
        ) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
            Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput)
        }

        fn release_native_frame(&mut self, _frame: DecoderNativeFrame) -> Result<(), DecoderError> {
            self.released_handles = self.released_handles.saturating_add(1);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), DecoderError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), DecoderError> {
            Ok(())
        }
    }
}
