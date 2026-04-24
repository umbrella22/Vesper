mod native;
mod system;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use player_core::MediaSource;
use player_platform_apple::{VIDEOTOOLBOX_BACKEND_NAME, probe_videotoolbox_hardware_decode};
use player_platform_desktop::{
    open_platform_desktop_source_with_options_and_interrupt,
    probe_platform_desktop_source_with_options,
};
use player_plugin::VesperPluginKind;
use player_plugin_loader::{
    DecoderPluginMatchRequest, PluginDiagnosticRecord, PluginDiagnosticStatus, PluginRegistry,
};
use player_runtime::{
    DecodedVideoFrame, PlaybackProgress, PlayerMediaInfo, PlayerPluginDiagnostic,
    PlayerPluginDiagnosticStatus, PlayerRuntime, PlayerRuntimeAdapter,
    PlayerRuntimeAdapterBootstrap, PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory,
    PlayerRuntimeAdapterInitializer, PlayerRuntimeBootstrap, PlayerRuntimeCommand,
    PlayerRuntimeCommandResult, PlayerRuntimeError, PlayerRuntimeErrorCode, PlayerRuntimeEvent,
    PlayerRuntimeInitializer, PlayerRuntimeOptions, PlayerRuntimeResult, PlayerRuntimeStartup,
    PlayerVideoDecodeInfo, PlayerVideoDecodeMode, PresentationState,
    register_default_runtime_adapter_factory,
};

pub const MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID: &str = "macos_software_desktop";
pub const MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID: &str = "macos_host";

pub use native::{
    MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID, MacosAvFoundationBridge,
    MacosAvFoundationBridgeBindings, MacosAvFoundationBridgeContext, MacosNativePlayerBridge,
    MacosNativePlayerProbe, MacosNativePlayerRuntimeAdapterFactory,
};
pub use system::{
    MacosSystemAvFoundationBridgeBindings,
    install_default_macos_system_native_runtime_adapter_factory,
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
    let PlayerRuntimeAdapterBootstrap {
        runtime,
        initial_frame,
        startup,
    } = open_platform_desktop_source_with_options_and_interrupt(
        MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        source,
        options.clone(),
        interrupt_flag,
    )?;
    let video_decode = macos_video_decode_info(runtime.media_info());
    let video_decode = apply_decoder_plugin_diagnostics_to_video_decode(
        video_decode,
        runtime.media_info(),
        &options,
    );

    Ok(PlayerRuntime::from_adapter_bootstrap(
        MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(MacosRuntimeAdapter {
                inner: runtime,
                video_decode: video_decode.clone(),
            }),
            initial_frame,
            startup: apply_video_decode_diagnostics(startup, &video_decode),
        },
    ))
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MacosHostPlayerRuntimeAdapterFactory;

#[derive(Debug, Default, Clone, Copy)]
pub struct MacosSoftwarePlayerRuntimeAdapterFactory;

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

struct MacosRuntimeAdapterInitializer {
    inner: Box<dyn PlayerRuntimeAdapterInitializer>,
    video_decode: PlayerVideoDecodeInfo,
}

struct MacosRuntimeAdapter {
    inner: Box<dyn PlayerRuntimeAdapter>,
    video_decode: PlayerVideoDecodeInfo,
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
            source,
            options.clone(),
        )?;
        let media_info = inner.media_info();
        let video_decode = apply_decoder_plugin_diagnostics_to_video_decode(
            macos_video_decode_info(&media_info),
            &media_info,
            &options,
        );

        Ok(Box::new(MacosRuntimeAdapterInitializer {
            inner,
            video_decode,
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
        apply_video_decode_diagnostics(self.inner.startup(), &self.video_decode)
    }

    fn initialize(self: Box<Self>) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
        let Self {
            inner,
            video_decode,
        } = *self;
        let PlayerRuntimeAdapterBootstrap {
            runtime,
            initial_frame,
            startup,
        } = inner.initialize()?;

        Ok(PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(MacosRuntimeAdapter {
                inner: runtime,
                video_decode: video_decode.clone(),
            }),
            initial_frame,
            startup: apply_video_decode_diagnostics(startup, &video_decode),
        })
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
        self.inner
            .drain_events()
            .into_iter()
            .map(|event| match event {
                PlayerRuntimeEvent::Initialized(startup) => PlayerRuntimeEvent::Initialized(
                    apply_video_decode_diagnostics(startup, &self.video_decode),
                ),
                other => other,
            })
            .collect()
    }

    fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
        self.inner.dispatch(command)
    }

    fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
        self.inner.advance()
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.inner.next_deadline()
    }
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
    let supported_plugins = registry.decoder_supported_plugin_names();

    if !supported_plugins.is_empty() {
        return Some(format!(
            "decoder plugin ABI v1 discovered candidate(s) for {} video: {}; macOS host diagnostics record them but playback still uses the native-first/FFmpeg fallback strategy",
            best_video.codec,
            supported_plugins.join(", ")
        ));
    }

    let load_notes = registry.diagnostic_notes();
    Some(format!(
        "decoder plugin paths were configured, but no decoder plugin advertised {} video support{}",
        best_video.codec,
        if load_notes.is_empty() {
            String::new()
        } else {
            format!(" ({})", load_notes.join("; "))
        }
    ))
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
    }
}

fn plugin_kind_label(kind: VesperPluginKind) -> &'static str {
    match kind {
        VesperPluginKind::PostDownloadProcessor => "post_download_processor",
        VesperPluginKind::PipelineEventHook => "pipeline_event_hook",
        VesperPluginKind::Decoder => "decoder",
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
            if let Some(fallback_reason) = fallback_reason {
                if let Some(video_decode) = bootstrap.startup.video_decode.as_mut() {
                    video_decode.fallback_reason =
                        Some(match video_decode.fallback_reason.take() {
                            Some(existing) if !existing.is_empty() => {
                                format!("{fallback_reason}; {existing}")
                            }
                            _ => fallback_reason,
                        });
                }
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
    use std::os::raw::c_void;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::{
        MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID, MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID,
        MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID, MacosHostPlayerRuntimeAdapterFactory,
        MacosSoftwarePlayerRuntimeAdapterFactory, apply_decoder_plugin_diagnostics,
        apply_decoder_plugin_diagnostics_to_video_decode, macos_video_decode_info,
        open_macos_host_runtime_source_with_options,
        probe_macos_host_runtime_initializer_with_factories,
        probe_macos_host_runtime_source_with_options,
    };
    use player_core::MediaSource;
    use player_platform_apple::VIDEOTOOLBOX_BACKEND_NAME;
    use player_runtime::{
        DecodedVideoFrame, PlaybackProgress, PlayerMediaInfo, PlayerPluginDiagnosticStatus,
        PlayerRuntimeAdapter, PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
        PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory,
        PlayerRuntimeAdapterInitializer, PlayerRuntimeCommand, PlayerRuntimeCommandResult,
        PlayerRuntimeError, PlayerRuntimeErrorCode, PlayerRuntimeEvent, PlayerRuntimeOptions,
        PlayerRuntimeResult, PlayerRuntimeStartup, PlayerVideoDecodeInfo, PlayerVideoDecodeMode,
        PlayerVideoInfo, PlayerVideoSurfaceKind, PlayerVideoSurfaceTarget, PresentationState,
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
    fn macos_host_factory_with_surface_prefers_native_path() {
        if !cfg!(target_os = "macos") {
            return;
        }

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
                .contains("decoder plugin paths were configured")
        );
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
            startup
                .video_decode
                .as_ref()
                .and_then(|info| info.fallback_reason.as_deref())
                .unwrap_or_default()
                .contains("decoder plugin paths were configured")
        );
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
    fn macos_host_runtime_with_surface_prefers_native() {
        if !cfg!(target_os = "macos") {
            return;
        }

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

    fn startup_with_video_decode(video_decode: PlayerVideoDecodeInfo) -> PlayerRuntimeStartup {
        PlayerRuntimeStartup {
            ffmpeg_initialized: false,
            audio_output: None,
            decoded_audio: None,
            video_decode: Some(video_decode),
            plugin_diagnostics: Vec::new(),
        }
    }

    fn test_video_path() -> Option<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../test-video.mp4");
        path.canonicalize()
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
    }

    #[derive(Clone)]
    struct FakeStrategyFactory {
        capabilities: PlayerRuntimeAdapterCapabilities,
        media_info: PlayerMediaInfo,
        startup: PlayerRuntimeStartup,
        initialize_error: Option<PlayerRuntimeError>,
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
                    events: VecDeque::new(),
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
        events: VecDeque<PlayerRuntimeEvent>,
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
            PresentationState::Ready
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
            _command: PlayerRuntimeCommand,
        ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
            Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "fake runtime dispatch is not implemented",
            ))
        }

        fn advance(&mut self) -> PlayerRuntimeResult<Option<DecodedVideoFrame>> {
            Ok(None)
        }

        fn next_deadline(&self) -> Option<Instant> {
            None
        }
    }
}
